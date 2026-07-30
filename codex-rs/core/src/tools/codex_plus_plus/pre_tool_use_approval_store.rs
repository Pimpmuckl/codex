use std::cell::RefCell;
use std::future::Future;

use super::pre_tool_use_review::PreToolUseExecutionTarget;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ExactPreToolUseApproval {
    pub(crate) execution_target: Option<PreToolUseExecutionTarget>,
}

tokio::task_local! {
    static EXACT_APPROVAL: RefCell<Option<ExactPreToolUseApproval>>;
}

pub(crate) async fn scope<T>(
    approval: Option<ExactPreToolUseApproval>,
    future: impl Future<Output = T>,
) -> T {
    EXACT_APPROVAL.scope(RefCell::new(approval), future).await
}

pub(crate) fn take() -> Option<ExactPreToolUseApproval> {
    EXACT_APPROVAL
        .try_with(|approval| approval.borrow_mut().take())
        .unwrap_or(None)
}

#[cfg(test)]
#[path = "pre_tool_use_approval_store_tests.rs"]
mod tests;
