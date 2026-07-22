//! Native Windows console input ownership for the Codex++ TUI.

use std::pin::Pin;
use std::task::Context;
use std::task::Poll;

use tokio_stream::Stream;

pub(in crate::tui) struct OwnedEventStream {
    inner: Option<crossterm::event::EventStream>,
    #[cfg(windows)]
    recovery_drain: Option<RecoveryDrain>,
}

impl Default for OwnedEventStream {
    fn default() -> Self {
        #[cfg(windows)]
        if let Err(err) = ensure_native_windows_input_mode() {
            tracing::warn!(error = %err, "failed to restore native Windows terminal input mode");
        }
        Self {
            inner: Some(crossterm::event::EventStream::new()),
            #[cfg(windows)]
            recovery_drain: None,
        }
    }
}

impl Stream for OwnedEventStream {
    type Item = std::io::Result<crossterm::event::Event>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        loop {
            #[cfg(windows)]
            self.recover_runtime_drift();

            #[cfg(windows)]
            if self
                .recovery_drain
                .as_ref()
                .is_some_and(|drain| Instant::now() >= drain.deadline)
            {
                self.recovery_drain = None;
            }

            let result = Pin::new(
                self.inner
                    .get_or_insert_with(crossterm::event::EventStream::new),
            )
            .poll_next(cx);
            #[cfg(windows)]
            if let Some(drain) = self.recovery_drain.as_mut() {
                match &result {
                    Poll::Ready(Some(Ok(event))) => {
                        drain.remaining -= 1;
                        if drain.remaining == 0
                            || matches!(
                                event,
                                crossterm::event::Event::Key(event)
                                    if event.code == crossterm::event::KeyCode::Char(
                                        RECOVERY_DRAIN_SENTINEL
                                    )
                            )
                        {
                            self.recovery_drain = None;
                        }
                        continue;
                    }
                    Poll::Ready(Some(Err(_))) | Poll::Ready(None) => {
                        self.recovery_drain = None;
                    }
                    Poll::Pending => {}
                }
            }
            #[cfg(windows)]
            if result.is_ready() && self.recover_runtime_drift() {
                continue;
            }
            return result;
        }
    }
}

#[cfg(windows)]
impl OwnedEventStream {
    fn recover_runtime_drift(&mut self) -> bool {
        match ensure_native_windows_input_mode() {
            Ok(true) => {
                self.inner.take();
                super::super::flush_terminal_input_buffer();
                let drain_result = (|| {
                    let Some((handle, _)) = windows_console_input_mode()? else {
                        return Ok(());
                    };
                    let mut sentinel: INPUT_RECORD = unsafe { std::mem::zeroed() };
                    sentinel.EventType = KEY_EVENT as u16;
                    sentinel.Event.KeyEvent.bKeyDown = 1;
                    sentinel.Event.KeyEvent.wRepeatCount = 1;
                    sentinel.Event.KeyEvent.uChar.UnicodeChar = RECOVERY_DRAIN_SENTINEL as u16;
                    let mut written = 0;
                    if unsafe { WriteConsoleInputW(handle, &sentinel, 1, &mut written) } == 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                    if written != 1 {
                        return Err(std::io::Error::other("partial console input write"));
                    }
                    Ok(())
                })();
                if let Err(err) = drain_result {
                    tracing::warn!(error = %err, "failed to mark stale Windows terminal input");
                    self.recovery_drain = None;
                } else {
                    self.recovery_drain = Some(RecoveryDrain {
                        deadline: Instant::now() + RECOVERY_DRAIN_TIMEOUT,
                        remaining: RECOVERY_DRAIN_LIMIT,
                    });
                }
                self.inner = Some(crossterm::event::EventStream::new());
                true
            }
            Ok(false) => false,
            Err(err) => {
                tracing::warn!(error = %err, "failed to recover native Windows terminal input mode");
                false
            }
        }
    }
}

#[cfg(windows)]
use std::io::Result;
#[cfg(windows)]
use std::sync::OnceLock;
#[cfg(windows)]
use std::time::Duration;
#[cfg(windows)]
use std::time::Instant;
#[cfg(windows)]
use windows_sys::Win32::Foundation::HANDLE;
#[cfg(windows)]
use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
#[cfg(windows)]
use windows_sys::Win32::System::Console::ENABLE_VIRTUAL_TERMINAL_INPUT;
#[cfg(windows)]
use windows_sys::Win32::System::Console::GetConsoleMode;
#[cfg(windows)]
use windows_sys::Win32::System::Console::GetStdHandle;
#[cfg(windows)]
use windows_sys::Win32::System::Console::INPUT_RECORD;
#[cfg(windows)]
use windows_sys::Win32::System::Console::KEY_EVENT;
#[cfg(windows)]
use windows_sys::Win32::System::Console::STD_INPUT_HANDLE;
#[cfg(windows)]
use windows_sys::Win32::System::Console::SetConsoleMode;
#[cfg(windows)]
use windows_sys::Win32::System::Console::WriteConsoleInputW;

#[cfg(windows)]
static ORIGINAL_VIRTUAL_TERMINAL_INPUT: OnceLock<bool> = OnceLock::new();

#[cfg(windows)]
const RECOVERY_DRAIN_SENTINEL: char = '\u{e000}';
#[cfg(windows)]
const RECOVERY_DRAIN_LIMIT: u8 = 32;
#[cfg(windows)]
const RECOVERY_DRAIN_TIMEOUT: Duration = Duration::from_millis(50);

#[cfg(windows)]
struct RecoveryDrain {
    deadline: Instant,
    remaining: u8,
}

#[cfg(windows)]
pub(in crate::tui) fn ensure_native_windows_input_mode() -> Result<bool> {
    let Some((handle, mode)) = windows_console_input_mode()? else {
        return Ok(false);
    };
    ORIGINAL_VIRTUAL_TERMINAL_INPUT.get_or_init(|| mode & ENABLE_VIRTUAL_TERMINAL_INPUT != 0);

    let native_mode = mode & !ENABLE_VIRTUAL_TERMINAL_INPUT;
    if native_mode == mode {
        return Ok(false);
    }
    set_windows_console_input_mode(handle, native_mode)?;
    Ok(true)
}

#[cfg(windows)]
pub(in crate::tui) fn restore_native_windows_input_mode() -> Result<()> {
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

#[cfg(windows)]
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

#[cfg(windows)]
fn set_windows_console_input_mode(handle: HANDLE, mode: u32) -> Result<()> {
    if unsafe { SetConsoleMode(handle, mode) } == 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(all(test, windows))]
#[path = "native_windows_input_tests.rs"]
mod tests;
