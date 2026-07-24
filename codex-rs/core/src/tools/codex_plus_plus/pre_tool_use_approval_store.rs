use super::pre_tool_use_review::PreToolUseApprovalReceipt;
use super::pre_tool_use_review::PreToolUseExecutionTarget;
use crate::tools::context::ToolInvocation;
use std::collections::HashSet;
use std::sync::Mutex;

#[derive(Default)]
pub(crate) struct PreToolUseApprovalStore(Mutex<HashSet<String>>);

impl PreToolUseApprovalStore {
    pub(crate) fn record_if_authorized(
        &self,
        invocation: &ToolInvocation,
        receipt: &PreToolUseApprovalReceipt,
        execution_target: Option<&PreToolUseExecutionTarget>,
    ) -> bool {
        receipt.authorizes(&invocation.call_id, &invocation.payload, execution_target)
            && self
                .0
                .lock()
                .is_ok_and(|mut approvals| approvals.insert(invocation.call_id.clone()))
    }

    pub(crate) fn take(&self, call_id: &str) -> bool {
        self.0
            .lock()
            .is_ok_and(|mut approvals| approvals.remove(call_id))
    }
}
