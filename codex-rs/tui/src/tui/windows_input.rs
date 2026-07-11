//! Windows console input-mode ownership for the TUI.

use std::io::Result;
use std::sync::OnceLock;

use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
use windows_sys::Win32::System::Console::ENABLE_VIRTUAL_TERMINAL_INPUT;
use windows_sys::Win32::System::Console::GetConsoleMode;
use windows_sys::Win32::System::Console::GetStdHandle;
use windows_sys::Win32::System::Console::STD_INPUT_HANDLE;
use windows_sys::Win32::System::Console::SetConsoleMode;

static ORIGINAL_VIRTUAL_TERMINAL_INPUT: OnceLock<bool> = OnceLock::new();

pub(super) fn ensure_native_windows_input_mode() -> Result<()> {
    let Some((handle, mode)) = windows_console_input_mode()? else {
        return Ok(());
    };
    ORIGINAL_VIRTUAL_TERMINAL_INPUT.get_or_init(|| mode & ENABLE_VIRTUAL_TERMINAL_INPUT != 0);

    let native_mode = mode & !ENABLE_VIRTUAL_TERMINAL_INPUT;
    if native_mode != mode {
        set_windows_console_input_mode(handle, native_mode)?;
    }

    Ok(())
}

pub(super) fn restore_native_windows_input_mode() -> Result<()> {
    let Some(originally_enabled) = ORIGINAL_VIRTUAL_TERMINAL_INPUT.get().copied() else {
        return Ok(());
    };
    let Some((handle, mode)) = windows_console_input_mode()? else {
        return Ok(());
    };

    let restored_mode = if originally_enabled {
        mode | ENABLE_VIRTUAL_TERMINAL_INPUT
    } else {
        mode & !ENABLE_VIRTUAL_TERMINAL_INPUT
    };
    if restored_mode != mode {
        set_windows_console_input_mode(handle, restored_mode)?;
    }

    Ok(())
}

fn windows_console_input_mode() -> Result<Option<(HANDLE, u32)>> {
    let handle = unsafe { GetStdHandle(STD_INPUT_HANDLE) };
    if handle == INVALID_HANDLE_VALUE || handle == 0 {
        return Ok(None);
    }

    let mut mode = 0;
    if unsafe { GetConsoleMode(handle, &mut mode) } == 0 {
        return Ok(None);
    }

    Ok(Some((handle, mode)))
}

fn set_windows_console_input_mode(handle: HANDLE, mode: u32) -> Result<()> {
    if unsafe { SetConsoleMode(handle, mode) } == 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}
