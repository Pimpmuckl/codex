use std::path::Path;
use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use codex_windows_sandbox::log_note;
use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::System::JobObjects::AssignProcessToJobObject;
use windows_sys::Win32::System::JobObjects::TerminateJobObject;
use windows_sys::Win32::System::Threading::PROCESS_INFORMATION;
use windows_sys::Win32::System::Threading::TerminateProcess;

use super::super::OwnedWinHandle;
use super::super::create_job_kill_on_close;

struct ProcessHandles {
    process: OwnedWinHandle,
    thread: OwnedWinHandle,
}

impl ProcessHandles {
    fn new(pi: PROCESS_INFORMATION) -> Self {
        Self {
            process: OwnedWinHandle::new(pi.hProcess),
            thread: OwnedWinHandle::new(pi.hThread),
        }
    }
}

pub(in super::super) struct ProcessTerminationTarget {
    handles: ProcessHandles,
    job: Option<Arc<OwnedWinHandle>>,
}

impl ProcessTerminationTarget {
    pub(in super::super) fn required(
        pi: PROCESS_INFORMATION,
        assign: impl FnOnce(HANDLE) -> Result<Arc<OwnedWinHandle>>,
    ) -> Result<Arc<Self>> {
        let handles = ProcessHandles::new(pi);
        let job = assign(handles.process.raw())?;
        Ok(Arc::new(Self {
            handles,
            job: Some(job),
        }))
    }

    pub(in super::super) fn restricted(
        pi: PROCESS_INFORMATION,
        log_dir: Option<&Path>,
    ) -> Arc<Self> {
        let handles = ProcessHandles::new(pi);
        let assignment = Self::try_assign(handles.process.raw());
        Arc::new(Self::from_assignment(handles, assignment, log_dir))
    }

    fn try_assign(process: HANDLE) -> Result<Arc<OwnedWinHandle>> {
        let job = unsafe { create_job_kill_on_close()? };
        if unsafe { AssignProcessToJobObject(job.raw(), process) } == 0 {
            return Err(std::io::Error::last_os_error()).context("AssignProcessToJobObject failed");
        }
        Ok(Arc::new(job))
    }

    fn from_assignment(
        handles: ProcessHandles,
        assignment: Result<Arc<OwnedWinHandle>>,
        log_dir: Option<&Path>,
    ) -> Self {
        match assignment {
            Ok(job) => Self {
                handles,
                job: Some(job),
            },
            Err(err) => {
                log_note(
                    &format!(
                        "runner job assignment unavailable; falling back to top-level termination: {err}"
                    ),
                    log_dir,
                );
                Self { handles, job: None }
            }
        }
    }

    pub(in super::super) fn process(&self) -> HANDLE {
        self.handles.process.raw()
    }

    pub(in super::super) fn thread(&self) -> HANDLE {
        self.handles.thread.raw()
    }

    pub(in super::super) fn terminates_process_tree(&self) -> bool {
        self.job.is_some()
    }

    pub(in super::super) fn terminate(&self) {
        unsafe {
            if let Some(job) = self.job.as_ref() {
                let _ = TerminateJobObject(job.raw(), 1);
            } else {
                let _ = TerminateProcess(self.process(), 1);
            }
        }
    }
}

impl Drop for ProcessTerminationTarget {
    fn drop(&mut self) {
        self.terminate();
    }
}

#[cfg(test)]
#[path = "process_termination_tests.rs"]
mod tests;
