use std::collections::BTreeMap;

use codex_config::ConfigLayerStack;
use codex_config::UserMessageInbox;
use codex_protocol::codex_plus_plus::user_message_envelope;
use codex_protocol::codex_plus_plus::user_message_item_id;
use codex_protocol::items::AgentMessageContent;
use codex_protocol::items::AgentMessageItem;
use codex_protocol::items::TurnItem;
use codex_protocol::models::MessagePhase;
use codex_tools::JsonSchema;
use codex_tools::ResponsesApiTool;
use codex_tools::ToolName;
use codex_tools::ToolSpec;
use serde::Deserialize;

use crate::function_tool::FunctionCallError;
use crate::tools::context::FunctionToolOutput;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolPayload;
use crate::tools::context::boxed_tool_output;
use crate::tools::handlers::parse_arguments;
use crate::tools::registry::CoreToolRuntime;
use crate::tools::registry::ToolExecutor;

pub(crate) const LEAVE_USER_MESSAGE_TOOL_NAME: &str = "leave_user_message";
const MAX_MESSAGE_CHARS: usize = 2_000;
const SUCCESS_MESSAGE: &str = "Message left for the user.";

pub(crate) struct LeaveUserMessageHandler;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LeaveUserMessageArgs {
    message: String,
}

pub(crate) fn enabled(config_layer_stack: &ConfigLayerStack) -> bool {
    let setting = config_layer_stack
        .effective_config()
        .get("user_message_inbox")
        .cloned()
        .and_then(|value| value.try_into::<UserMessageInbox>().ok())
        .unwrap_or_default();
    setting == UserMessageInbox::Enabled
}

fn normalize_message(message: &str) -> Result<&str, String> {
    let message = message.trim();
    if message.is_empty() {
        return Err("`message` must not be empty.".to_string());
    }
    if message.chars().count() > MAX_MESSAGE_CHARS {
        return Err(format!(
            "`message` must be at most {MAX_MESSAGE_CHARS} characters."
        ));
    }
    Ok(message)
}

impl ToolExecutor<ToolInvocation> for LeaveUserMessageHandler {
    fn tool_name(&self) -> ToolName {
        ToolName::plain(LEAVE_USER_MESSAGE_TOOL_NAME)
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec::Function(ResponsesApiTool {
            name: LEAVE_USER_MESSAGE_TOOL_NAME.to_string(),
            description: "Leave a durable, non-blocking message for the user in this thread."
                .to_string(),
            strict: true,
            defer_loading: None,
            parameters: JsonSchema::object(
                BTreeMap::from([(
                    "message".to_string(),
                    JsonSchema::string(Some("The message to leave for the user.".to_string())),
                )]),
                Some(vec!["message".to_string()]),
                Some(false.into()),
            ),
            output_schema: None,
        })
    }

    fn handle(&self, invocation: ToolInvocation) -> codex_tools::ToolExecutorFuture<'_> {
        Box::pin(async move {
            let ToolInvocation {
                session,
                turn,
                call_id,
                payload,
                ..
            } = invocation;
            let ToolPayload::Function { arguments } = payload else {
                return Err(FunctionCallError::RespondToModel(format!(
                    "{LEAVE_USER_MESSAGE_TOOL_NAME} handler received unsupported payload"
                )));
            };
            let args: LeaveUserMessageArgs = parse_arguments(&arguments)?;
            let message =
                normalize_message(&args.message).map_err(FunctionCallError::RespondToModel)?;
            let item = TurnItem::AgentMessage(AgentMessageItem {
                id: user_message_item_id(&call_id),
                content: vec![AgentMessageContent::Text {
                    text: user_message_envelope(message),
                }],
                phase: Some(MessagePhase::Commentary),
                memory_citation: None,
            });

            session.emit_turn_item_started(turn.as_ref(), &item).await;
            session.emit_turn_item_completed(turn.as_ref(), item).await;

            Ok(boxed_tool_output(FunctionToolOutput::from_text(
                SUCCESS_MESSAGE.to_string(),
                Some(true),
            )))
        })
    }
}

impl CoreToolRuntime for LeaveUserMessageHandler {}

#[cfg(test)]
#[path = "user_message_inbox_tests.rs"]
mod tests;
