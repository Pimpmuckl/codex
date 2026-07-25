use crate::ipc_framed::ChildConsoleMode;
use crate::winutil::argv_to_command_line;
use crate::winutil::to_wide;
use anyhow::Context;
use anyhow::Result;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::ffi::c_void;
use std::path::Path;
use std::ptr;
use windows_sys::Win32::Foundation::MAX_PATH;
use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW;
use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;
use windows_sys::Win32::System::Threading::CREATE_SUSPENDED;
use windows_sys::Win32::System::Threading::CREATE_UNICODE_ENVIRONMENT;
use windows_sys::Win32::System::Threading::CreateProcessW;
use windows_sys::Win32::System::Threading::EXTENDED_STARTUPINFO_PRESENT;
use windows_sys::Win32::System::Threading::PROCESS_INFORMATION;
use windows_sys::Win32::System::Threading::STARTUPINFOW;

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
        anyhow::bail!("system directory unavailable");
    }
    let mut path = system[..len].to_vec();
    path.extend(r"\cmd.exe".encode_utf16().chain([0]));
    Ok(path)
}

pub(crate) fn prepare_command(
    argv: &[String],
    cwd: &Path,
    env_map: &HashMap<String, String>,
) -> Result<(String, Option<Vec<u16>>)> {
    let request_path = env_map
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("PATH"))
        .map(|(_, value)| OsStr::new(value));
    let inherited_path = std::env::var_os("PATH");
    let resolved_program =
        which::which_in(&argv[0], request_path.or(inherited_path.as_deref()), cwd)
            .map(Some)
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
    let command_line = if is_batch {
        batch_command_line(
            resolved_program
                .as_deref()
                .context("resolved batch path missing")?,
            argv,
        )?
    } else {
        argv_to_command_line(argv)
    };
    let application_name = if is_batch {
        Some(command_prompt()?)
    } else if request_path.is_some() {
        resolved_program.as_ref().map(to_wide)
    } else {
        None
    };
    Ok((command_line, application_name))
}

/// # Safety
///
/// The supplied startup information and inherited handles must remain valid
/// for the duration of the call.
pub(crate) unsafe fn create_process_with_stdio(
    application_name: Option<&[u16]>,
    command_line: &mut [u16],
    environment: &[u16],
    cwd: &[u16],
    startup_info: &STARTUPINFOW,
    process_info: &mut PROCESS_INFORMATION,
    console_mode: ChildConsoleMode,
) -> (i32, u32) {
    let flags = CREATE_UNICODE_ENVIRONMENT
        | EXTENDED_STARTUPINFO_PRESENT
        | CREATE_SUSPENDED
        | match console_mode {
            ChildConsoleMode::Inherit => 0,
            ChildConsoleMode::NoWindow => CREATE_NO_WINDOW,
        };
    let ok = CreateProcessW(
        application_name.map_or(ptr::null(), <[u16]>::as_ptr),
        command_line.as_mut_ptr(),
        ptr::null_mut(),
        ptr::null_mut(),
        /*bInheritHandles*/ 1,
        flags,
        environment.as_ptr() as *mut c_void,
        cwd.as_ptr(),
        startup_info,
        process_info,
    );
    (ok, flags)
}

#[cfg(test)]
#[path = "current_user_process_tests.rs"]
mod tests;
