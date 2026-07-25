use crate::desktop::LaunchDesktop;
use crate::ipc_framed::ChildConsoleMode;
use crate::logging;
use crate::proc_thread_attr::ProcThreadAttributeList;
use crate::winutil::argv_to_command_line;
use crate::winutil::format_last_error;
use crate::winutil::to_wide;
use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::ffi::c_void;
use std::path::Path;
use std::ptr;
use windows_sys::Win32::Foundation::CloseHandle;
use windows_sys::Win32::Foundation::GetLastError;
use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::Foundation::HANDLE_FLAG_INHERIT;
use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
use windows_sys::Win32::Foundation::MAX_PATH;
use windows_sys::Win32::Foundation::SetHandleInformation;
use windows_sys::Win32::Storage::FileSystem::ReadFile;
use windows_sys::Win32::System::Console::GetStdHandle;
use windows_sys::Win32::System::Console::STD_ERROR_HANDLE;
use windows_sys::Win32::System::Console::STD_INPUT_HANDLE;
use windows_sys::Win32::System::Console::STD_OUTPUT_HANDLE;
use windows_sys::Win32::System::Pipes::CreatePipe;
use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW;
use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;
use windows_sys::Win32::System::Threading::CREATE_SUSPENDED;
use windows_sys::Win32::System::Threading::CREATE_UNICODE_ENVIRONMENT;
use windows_sys::Win32::System::Threading::CreateProcessAsUserW;
use windows_sys::Win32::System::Threading::CreateProcessW;
use windows_sys::Win32::System::Threading::EXTENDED_STARTUPINFO_PRESENT;
use windows_sys::Win32::System::Threading::PROCESS_INFORMATION;
use windows_sys::Win32::System::Threading::STARTF_USESTDHANDLES;
use windows_sys::Win32::System::Threading::STARTUPINFOEXW;
use windows_sys::Win32::System::Threading::STARTUPINFOW;

pub struct CreatedProcess {
    pub process_info: PROCESS_INFORMATION,
    pub startup_info: STARTUPINFOW,
    _desktop: LaunchDesktop,
}

#[derive(Clone, Copy)]
pub enum ProcessExecutionMode {
    CurrentUser,
    Token(HANDLE),
}
fn append_batch_arg(command: &mut String, arg: &str) -> Result<()> {
    if arg.contains(['\0', '\r', '\n', '"']) {
        anyhow::bail!("batch file arguments may not contain NUL, quotes, or newlines");
    }
    const UNQUOTED: &str = r"#$*+-./:?@\_";
    let quoted = arg.is_empty()
        || arg.ends_with('\\')
        || arg.chars().any(|ch| {
            ch.is_control()
                || ch.is_ascii() && !(ch.is_ascii_alphanumeric() || UNQUOTED.contains(ch))
        });
    if quoted {
        command.push('"');
    }
    for ch in arg.chars() {
        if ch == '%' {
            command.push_str("%%cd:~,%");
        }
        command.push(ch);
    }
    if quoted {
        command.push('"');
    }
    Ok(())
}
fn batch_command_line(program: &Path, argv: &[String]) -> Result<String> {
    let mut command = "cmd.exe /e:ON /v:OFF /d /c \"".to_string();
    append_batch_arg(&mut command, &program.to_string_lossy())?;
    for arg in &argv[1..] {
        command.push(' ');
        append_batch_arg(&mut command, arg)?;
    }
    command.push('"');
    Ok(command)
}

fn command_prompt() -> Result<Vec<u16>> {
    let mut system = [0; MAX_PATH as usize];
    let len = unsafe { GetSystemDirectoryW(system.as_mut_ptr(), MAX_PATH) } as usize;
    if len == 0 || len >= system.len() {
        return Err(anyhow!("system directory unavailable"));
    }
    let mut path = system[..len].to_vec();
    path.extend(r"\cmd.exe".encode_utf16().chain([0]));
    Ok(path)
}

pub fn make_env_block(env: &HashMap<String, String>) -> Vec<u16> {
    let mut items: Vec<(String, String)> =
        env.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    items.sort_by(|a, b| {
        a.0.to_uppercase()
            .cmp(&b.0.to_uppercase())
            .then(a.0.cmp(&b.0))
    });
    let mut w: Vec<u16> = Vec::new();
    for (k, v) in items {
        let mut s = to_wide(format!("{k}={v}"));
        s.pop();
        w.extend_from_slice(&s);
        w.push(0);
    }
    if w.is_empty() {
        w.push(0);
    }
    w.push(0);
    w
}

unsafe fn ensure_inheritable_stdio(si: &mut STARTUPINFOW) -> Result<()> {
    for kind in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
        let h = GetStdHandle(kind);
        if h == 0 || h == INVALID_HANDLE_VALUE {
            return Err(anyhow!("GetStdHandle failed: {}", GetLastError()));
        }
        if SetHandleInformation(h, HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT) == 0 {
            return Err(anyhow!("SetHandleInformation failed: {}", GetLastError()));
        }
    }
    si.dwFlags |= STARTF_USESTDHANDLES;
    si.hStdInput = GetStdHandle(STD_INPUT_HANDLE);
    si.hStdOutput = GetStdHandle(STD_OUTPUT_HANDLE);
    si.hStdError = GetStdHandle(STD_ERROR_HANDLE);
    Ok(())
}

/// # Safety
/// Caller must provide a valid primary token handle (`h_token`) with appropriate access,
/// and the `argv`, `cwd`, and `env_map` must remain valid for the duration of the call.
// Low-level CreateProcessAsUserW wrapper mirrors the Windows API shape.
#[allow(clippy::too_many_arguments)]
pub unsafe fn create_process(
    user: ProcessExecutionMode,
    argv: &[String],
    cwd: &Path,
    env_map: &HashMap<String, String>,
    logs_base_dir: Option<&Path>,
    stdio: Option<(HANDLE, HANDLE, HANDLE)>,
    console_mode: ChildConsoleMode,
    use_private_desktop: bool,
) -> Result<CreatedProcess> {
    let request_path = env_map
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("PATH"))
        .map(|(_, value)| OsStr::new(value));
    let inherited_path = std::env::var_os("PATH");
    let resolved_program = matches!(user, ProcessExecutionMode::CurrentUser)
        .then(|| which::which_in(&argv[0], request_path.or(inherited_path.as_deref()), cwd))
        .transpose()
        .or_else(|err| {
            if request_path.is_some() {
                Err(err)
            } else {
                Ok(None)
            }
        })
        .with_context(|| format!("failed to resolve executable `{}`", argv[0]))?;
    let is_batch = resolved_program.as_ref().is_some_and(|program| {
        program
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("bat") || ext.eq_ignore_ascii_case("cmd"))
    });
    let cmdline_str = if is_batch {
        batch_command_line(
            resolved_program
                .as_deref()
                .context("resolved batch path missing")?,
            argv,
        )?
    } else {
        argv_to_command_line(argv)
    };
    let mut cmdline: Vec<u16> = to_wide(&cmdline_str);
    let env_block = make_env_block(env_map);
    let desktop = LaunchDesktop::prepare(use_private_desktop, logs_base_dir)?;
    let application_name = if is_batch {
        Some(command_prompt()?)
    } else if request_path.is_some() {
        resolved_program.as_ref().map(to_wide)
    } else {
        None
    };
    let application_name = application_name.as_ref().map_or(ptr::null(), Vec::as_ptr);
    let mut pi: PROCESS_INFORMATION = std::mem::zeroed();
    let cwd_wide = to_wide(cwd);
    let env_block_len = env_block.len();
    match stdio {
        Some((stdin_h, stdout_h, stderr_h)) => {
            let mut si: STARTUPINFOEXW = std::mem::zeroed();
            si.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
            // Some processes (e.g., PowerShell) can fail with STATUS_DLL_INIT_FAILED
            // if lpDesktop is not set when launching with a restricted token.
            // Point explicitly at the interactive desktop or a private desktop.
            si.StartupInfo.lpDesktop = desktop.startup_info_desktop();
            si.StartupInfo.dwFlags |= STARTF_USESTDHANDLES;
            si.StartupInfo.hStdInput = stdin_h;
            si.StartupInfo.hStdOutput = stdout_h;
            si.StartupInfo.hStdError = stderr_h;
            let mut inherited_handles = vec![stdin_h, stdout_h];
            if !inherited_handles.contains(&stderr_h) {
                inherited_handles.push(stderr_h);
            }
            for &handle in &inherited_handles {
                if SetHandleInformation(handle, HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT) == 0 {
                    return Err(anyhow!(
                        "SetHandleInformation failed for stdio handle: {}",
                        GetLastError()
                    ));
                }
            }
            let mut attrs = ProcThreadAttributeList::new(/*attr_count*/ 1)?;
            attrs.set_handle_list(inherited_handles)?;
            si.lpAttributeList = attrs.as_mut_ptr();

            let creation_flags = CREATE_UNICODE_ENVIRONMENT
                | EXTENDED_STARTUPINFO_PRESENT
                | if matches!(user, ProcessExecutionMode::CurrentUser) {
                    CREATE_SUSPENDED
                } else {
                    0
                }
                | match console_mode {
                    ChildConsoleMode::Inherit => 0,
                    ChildConsoleMode::NoWindow => CREATE_NO_WINDOW,
                };
            let ok = match user {
                ProcessExecutionMode::CurrentUser => CreateProcessW(
                    application_name,
                    cmdline.as_mut_ptr(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    1,
                    creation_flags,
                    env_block.as_ptr() as *mut c_void,
                    cwd_wide.as_ptr(),
                    &si.StartupInfo,
                    &mut pi,
                ),
                ProcessExecutionMode::Token(h_token) => CreateProcessAsUserW(
                    h_token,
                    std::ptr::null(),
                    cmdline.as_mut_ptr(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    1,
                    creation_flags,
                    env_block.as_ptr() as *mut c_void,
                    cwd_wide.as_ptr(),
                    &si.StartupInfo,
                    &mut pi,
                ),
            };
            if ok == 0 {
                let err = GetLastError() as i32;
                let msg = format!(
                    "CreateProcessAsUserW failed: {} ({}) | cwd={} | cmd={} | env_u16_len={} | si_flags={} | creation_flags={}",
                    err,
                    format_last_error(err),
                    cwd.display(),
                    cmdline_str,
                    env_block_len,
                    si.StartupInfo.dwFlags,
                    creation_flags,
                );
                logging::debug_log(&msg, logs_base_dir);
                return Err(std::io::Error::from_raw_os_error(err)).context(msg);
            }
            Ok(CreatedProcess {
                process_info: pi,
                startup_info: si.StartupInfo,
                _desktop: desktop,
            })
        }
        None => {
            let mut si: STARTUPINFOW = std::mem::zeroed();
            si.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
            si.lpDesktop = desktop.startup_info_desktop();
            ensure_inheritable_stdio(&mut si)?;

            let creation_flags = CREATE_UNICODE_ENVIRONMENT;
            let ProcessExecutionMode::Token(h_token) = user else {
                anyhow::bail!("current-user process creation requires explicit stdio");
            };
            let ok = CreateProcessAsUserW(
                h_token,
                std::ptr::null(),
                cmdline.as_mut_ptr(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                1,
                creation_flags,
                env_block.as_ptr() as *mut c_void,
                cwd_wide.as_ptr(),
                &si,
                &mut pi,
            );
            if ok == 0 {
                let err = GetLastError() as i32;
                let msg = format!(
                    "CreateProcessAsUserW failed: {} ({}) | cwd={} | cmd={} | env_u16_len={} | si_flags={} | creation_flags={}",
                    err,
                    format_last_error(err),
                    cwd.display(),
                    cmdline_str,
                    env_block_len,
                    si.dwFlags,
                    creation_flags,
                );
                logging::debug_log(&msg, logs_base_dir);
                return Err(std::io::Error::from_raw_os_error(err)).context(msg);
            }
            Ok(CreatedProcess {
                process_info: pi,
                startup_info: si,
                _desktop: desktop,
            })
        }
    }
}
/// Controls whether the child's stdin handle is kept open for writing.
#[allow(dead_code)]
pub enum StdinMode {
    Closed,
    Open,
}

/// Controls how stderr is wired for a pipe-spawned process.
#[allow(dead_code)]
pub enum StderrMode {
    MergeStdout,
    Separate,
    InheritOutput,
}

/// Handles returned by `spawn_process_with_pipes`.
#[allow(dead_code)]
pub struct PipeSpawnHandles {
    pub process: PROCESS_INFORMATION,
    pub stdin_write: Option<HANDLE>,
    pub stdout_read: HANDLE,
    pub stderr_read: Option<HANDLE>,
    pub(crate) desktop: LaunchDesktop,
}

/// Spawns a process with anonymous pipes and returns the relevant handles.
#[allow(clippy::too_many_arguments)]
pub fn spawn_process_with_pipes(
    execution_mode: ProcessExecutionMode,
    argv: &[String],
    cwd: &Path,
    env_map: &HashMap<String, String>,
    stdin_mode: StdinMode,
    stderr_mode: StderrMode,
    console_mode: ChildConsoleMode,
    use_private_desktop: bool,
    logs_base_dir: Option<&Path>,
) -> Result<PipeSpawnHandles> {
    let mut in_r: HANDLE = 0;
    let mut in_w: HANDLE = 0;
    let mut out_r: HANDLE = 0;
    let mut out_w: HANDLE = 0;
    let mut err_r: HANDLE = 0;
    let mut err_w: HANDLE = 0;
    unsafe {
        if CreatePipe(&mut in_r, &mut in_w, ptr::null_mut(), 0) == 0 {
            return Err(anyhow!("CreatePipe stdin failed: {}", GetLastError()));
        }
        if !matches!(stderr_mode, StderrMode::InheritOutput)
            && CreatePipe(&mut out_r, &mut out_w, ptr::null_mut(), 0) == 0
        {
            CloseHandle(in_r);
            CloseHandle(in_w);
            return Err(anyhow!("CreatePipe stdout failed: {}", GetLastError()));
        }
        if matches!(stderr_mode, StderrMode::Separate)
            && CreatePipe(&mut err_r, &mut err_w, ptr::null_mut(), 0) == 0
        {
            CloseHandle(in_r);
            CloseHandle(in_w);
            CloseHandle(out_r);
            CloseHandle(out_w);
            return Err(anyhow!("CreatePipe stderr failed: {}", GetLastError()));
        }
    }

    let stderr_handle = match stderr_mode {
        StderrMode::MergeStdout => out_w,
        StderrMode::Separate => err_w,
        StderrMode::InheritOutput => unsafe { GetStdHandle(STD_ERROR_HANDLE) },
    };

    let stdout_handle = if matches!(stderr_mode, StderrMode::InheritOutput) {
        unsafe { GetStdHandle(STD_OUTPUT_HANDLE) }
    } else {
        out_w
    };
    let stdio = Some((in_r, stdout_handle, stderr_handle));
    let spawn_result = unsafe {
        create_process(
            execution_mode,
            argv,
            cwd,
            env_map,
            logs_base_dir,
            stdio,
            console_mode,
            use_private_desktop,
        )
    };
    let created = match spawn_result {
        Ok(v) => v,
        Err(err) => {
            unsafe {
                CloseHandle(in_r);
                CloseHandle(in_w);
                if !matches!(stderr_mode, StderrMode::InheritOutput) {
                    CloseHandle(out_r);
                    CloseHandle(out_w);
                }
                if matches!(stderr_mode, StderrMode::Separate) {
                    CloseHandle(err_r);
                    CloseHandle(err_w);
                }
            }
            return Err(err);
        }
    };
    let CreatedProcess {
        process_info: pi,
        _desktop: desktop,
        ..
    } = created;

    unsafe {
        CloseHandle(in_r);
        if !matches!(stderr_mode, StderrMode::InheritOutput) {
            CloseHandle(out_w);
        }
        if matches!(stderr_mode, StderrMode::Separate) {
            CloseHandle(err_w);
        }
        if matches!(stdin_mode, StdinMode::Closed) {
            CloseHandle(in_w);
        }
    }

    Ok(PipeSpawnHandles {
        process: pi,
        stdin_write: match stdin_mode {
            StdinMode::Open => Some(in_w),
            StdinMode::Closed => None,
        },
        stdout_read: if matches!(stderr_mode, StderrMode::InheritOutput) {
            INVALID_HANDLE_VALUE
        } else {
            out_r
        },
        stderr_read: match stderr_mode {
            StderrMode::Separate => Some(err_r),
            StderrMode::MergeStdout | StderrMode::InheritOutput => None,
        },
        desktop,
    })
}

/// Reads a HANDLE until EOF and invokes `on_chunk` for each read.
pub fn read_handle_loop<F>(handle: HANDLE, mut on_chunk: F) -> std::thread::JoinHandle<()>
where
    F: FnMut(&[u8]) + Send + 'static,
{
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        loop {
            let mut read_bytes: u32 = 0;
            let ok = unsafe {
                ReadFile(
                    handle,
                    buf.as_mut_ptr(),
                    buf.len() as u32,
                    &mut read_bytes,
                    ptr::null_mut(),
                )
            };
            if ok == 0 || read_bytes == 0 {
                break;
            }
            on_chunk(&buf[..read_bytes as usize]);
        }
        unsafe {
            CloseHandle(handle);
        }
    })
}
#[cfg(test)]
#[path = "process_tests.rs"]
mod tests;
