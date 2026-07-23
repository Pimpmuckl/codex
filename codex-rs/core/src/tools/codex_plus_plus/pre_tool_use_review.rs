use crate::guardian::GuardianApprovalRequest;
use crate::guardian::guardian_timeout_message;
use crate::guardian::new_guardian_review_id;
use crate::guardian::review_approval_request;
use crate::tools::context::ToolInvocation;
use crate::tools::registry::PreToolUsePayload;
use codex_protocol::protocol::ReviewDecision;
use futures::future::BoxFuture;

pub(crate) struct PreToolUseApproval;

pub(crate) fn review<'a>(
    invocation: &'a ToolInvocation,
    payload: &'a PreToolUsePayload,
    reason: String,
) -> BoxFuture<'a, Result<PreToolUseApproval, String>> {
    Box::pin(async move {
        let review_id = new_guardian_review_id();
        let decision = review_approval_request(
            &invocation.session,
            &invocation.turn,
            review_id,
            GuardianApprovalRequest::PreToolUse {
                id: invocation.call_id.clone(),
                tool_name: payload.tool_name.name().to_string(),
                tool_input: payload.tool_input.clone(),
                reason,
                #[allow(deprecated)]
                cwd: invocation.turn.cwd.clone(),
            },
            /*retry_reason*/ None,
        )
        .await;
        match decision {
            ReviewDecision::Approved
            | ReviewDecision::ApprovedForSession
            | ReviewDecision::ApprovedExecpolicyAmendment { .. }
            | ReviewDecision::NetworkPolicyAmendment { .. } => Ok(PreToolUseApproval),
            ReviewDecision::Denied { rejection } => Err(rejection),
            ReviewDecision::TimedOut => Err(guardian_timeout_message()),
            ReviewDecision::Abort => Err(
                "Automatic approval review was cancelled. The tool call was blocked.".to_string(),
            ),
        }
    })
}
