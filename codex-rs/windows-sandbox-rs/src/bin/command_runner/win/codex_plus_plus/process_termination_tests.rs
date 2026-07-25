use std::sync::Arc;

use anyhow::anyhow;
use windows_sys::Win32::System::Threading::PROCESS_INFORMATION;

use super::super::super::OwnedWinHandle;
use super::ProcessHandles;
use super::ProcessTerminationTarget;

fn empty_process_information() -> PROCESS_INFORMATION {
    PROCESS_INFORMATION {
        hProcess: 0,
        hThread: 0,
        dwProcessId: 0,
        dwThreadId: 0,
    }
}

fn empty_process_handles() -> ProcessHandles {
    ProcessHandles::new(empty_process_information())
}

#[test]
fn restricted_termination_uses_assigned_job() {
    let target = ProcessTerminationTarget::from_assignment(
        empty_process_handles(),
        Ok(Arc::new(OwnedWinHandle::new(0))),
        None,
    );

    assert!(target.terminates_process_tree());
}

#[test]
fn restricted_termination_falls_back_to_process() {
    let target = ProcessTerminationTarget::from_assignment(
        empty_process_handles(),
        Err(anyhow!("nested job assignment rejected")),
        None,
    );

    assert!(!target.terminates_process_tree());
}

#[test]
fn required_job_assignment_failure_is_fatal() {
    let result = ProcessTerminationTarget::required(empty_process_information(), |_| {
        Err(anyhow!("job assignment rejected"))
    });

    assert!(result.is_err());
}
