use super::ApprovalRequestReasons;
use super::GuardianApprovalRequest;
use super::GuardianReviewContext;
use super::GuardianReviewOptions;
use super::review::run_synchronous_review;
use super::runtime::ReviewRuntime;
use crate::session::session::Session;
use codex_analytics::GuardianApprovalRequestSource;
use codex_protocol::approvals::GuardianReviewReason;
use codex_protocol::protocol::ReviewDecision;
use futures::future::BoxFuture;
use std::sync::Arc;

pub(crate) fn review(
    session: Arc<Session>,
    context: GuardianReviewContext,
    review_id: String,
    request: GuardianApprovalRequest,
) -> BoxFuture<'static, ReviewDecision> {
    // A hook's explicit ask requires a fresh review even under full access.
    // Erase the reviewer future to keep recursive session/auth futures bounded.
    Box::pin(run_synchronous_review(
        ReviewRuntime {
            session,
            context,
            review_id,
            request: request.into(),
            reasons: ApprovalRequestReasons::default(),
            options: GuardianReviewOptions {
                require_guardian: true,
                plugin_attribution_override: None,
                approval_request_source: GuardianApprovalRequestSource::MainTurn,
                external_cancel: None,
                require_synchronous_review: true,
            },
        },
        GuardianReviewReason::FreshRequired,
    ))
}
