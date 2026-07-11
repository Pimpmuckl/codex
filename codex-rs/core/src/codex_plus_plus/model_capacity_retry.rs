use std::time::Duration;

use crate::client::ModelClientSession;
use crate::session::session::Session;
use crate::session::turn_context::TurnContext;
use codex_protocol::error::CodexErr;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::SessionSource;
use codex_protocol::protocol::WarningEvent;
use tokio_util::sync::CancellationToken;
use tracing::warn;

const DELAY: Duration = Duration::from_secs(60);

pub(crate) fn applies_to_sampling(err: &CodexErr, session_source: &SessionSource) -> bool {
    matches!(err, CodexErr::ServerOverloaded)
        && !crate::guardian::is_guardian_reviewer_source(session_source)
}

pub(crate) async fn handle(
    retries: &mut u64,
    max_retries: u64,
    err: CodexErr,
    client_session: &mut ModelClientSession,
    sess: &Session,
    turn_context: &TurnContext,
    cancellation_token: &CancellationToken,
) -> Result<(), CodexErr> {
    if *retries >= max_retries {
        return Err(err);
    }

    *retries += 1;
    let retry_count = *retries;
    warn!(
        "model at capacity - retrying sampling request ({retry_count}/{max_retries} in {DELAY:?})...",
    );
    sess.send_event(
        turn_context,
        EventMsg::Warning(WarningEvent {
            message: format!(
                "The selected model is at capacity. Retrying in one minute ({retry_count}/{max_retries})."
            ),
        }),
    )
    .await;

    tokio::select! {
        _ = tokio::time::sleep(DELAY) => {
            client_session.reset_websocket_session();
            Ok(())
        },
        _ = cancellation_token.cancelled() => Err(CodexErr::TurnAborted),
    }
}
