use crate::guardian::GuardianApprovalRequest;
use crate::guardian::guardian_rejection_message;
use crate::guardian::guardian_timeout_message;
use crate::guardian::new_guardian_review_id;
use crate::guardian::review_approval_request;
use crate::tools::context::ToolInvocation;
use crate::tools::registry::PreToolUsePayload;
use codex_protocol::config_types::ApprovalsReviewer;
use codex_protocol::protocol::ReviewDecision;
use futures::future::BoxFuture;

pub(crate) fn review<'a>(
    invocation: &'a ToolInvocation,
    payload: &'a PreToolUsePayload,
    reason: String,
    approvals_reviewer: ApprovalsReviewer,
) -> BoxFuture<'a, Result<(), String>> {
    Box::pin(async move {
        let strict = invocation
            .session
            .strict_auto_review_enabled_for_turn()
            .await;
        if approvals_reviewer != ApprovalsReviewer::AutoReview && !strict {
            return Err(
                "PreToolUse hook requested Guardian review, but auto review is disabled."
                    .to_string(),
            );
        }

        let review_id = new_guardian_review_id();
        let decision = review_approval_request(
            &invocation.session,
            &invocation.turn,
            review_id.clone(),
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
            | ReviewDecision::NetworkPolicyAmendment { .. } => Ok(()),
            ReviewDecision::Denied => {
                Err(guardian_rejection_message(&invocation.session, &review_id).await)
            }
            ReviewDecision::TimedOut => Err(guardian_timeout_message()),
            ReviewDecision::Abort => Err(
                "Automatic approval review was cancelled. The tool call was blocked.".to_string(),
            ),
        }
    })
}
