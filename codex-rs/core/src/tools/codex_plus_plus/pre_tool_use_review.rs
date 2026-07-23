use crate::guardian::GuardianApprovalRequest;
use crate::guardian::guardian_timeout_message;
use crate::guardian::new_guardian_review_id;
use crate::guardian::review_approval_request;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolPayload;
use crate::tools::registry::PreToolUsePayload;
use codex_protocol::protocol::ReviewDecision;
use codex_protocol::protocol::TurnEnvironmentSelection;
use futures::future::BoxFuture;
use serde_json::json;

pub(crate) type PreToolUseExecutionTarget = TurnEnvironmentSelection;

pub(crate) struct PreToolUseApproval {
    call_id: String,
    payload: ToolPayload,
    execution_target: Option<PreToolUseExecutionTarget>,
}

impl PreToolUseApproval {
    pub(crate) fn authorizes(
        &self,
        invocation: &ToolInvocation,
        execution_target: Option<&PreToolUseExecutionTarget>,
    ) -> bool {
        self.call_id == invocation.call_id
            && self.payload == invocation.payload
            && self.execution_target.as_ref() == execution_target
    }
}

pub(crate) fn review<'a>(
    invocation: &'a ToolInvocation,
    payload: &'a PreToolUsePayload,
    reason: String,
) -> BoxFuture<'a, Result<PreToolUseApproval, String>> {
    Box::pin(async move {
        let review_id = new_guardian_review_id();
        let tool_input = payload.execution_target.as_ref().map_or_else(
            || payload.tool_input.clone(),
            |target| {
                json!({
                    "hook_input": payload.tool_input,
                    "execution_target": {
                        "environment_id": target.environment_id,
                        "cwd": target.cwd,
                    },
                })
            },
        );
        let decision = review_approval_request(
            &invocation.session,
            &invocation.turn,
            review_id,
            GuardianApprovalRequest::PreToolUse {
                id: invocation.call_id.clone(),
                tool_name: payload.tool_name.name().to_string(),
                tool_input,
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
            | ReviewDecision::NetworkPolicyAmendment { .. } => Ok(PreToolUseApproval {
                call_id: invocation.call_id.clone(),
                payload: invocation.payload.clone(),
                execution_target: payload.execution_target.clone(),
            }),
            ReviewDecision::Denied { rejection } => Err(rejection),
            ReviewDecision::TimedOut => Err(guardian_timeout_message()),
            ReviewDecision::Abort => Err(
                "Automatic approval review was cancelled. The tool call was blocked.".to_string(),
            ),
        }
    })
}
