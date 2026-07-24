use std::path::Path;

use codex_analytics::GuardianReviewedAction;
use codex_protocol::approvals::GuardianAssessmentAction;
use codex_protocol::approvals::GuardianCommandSource;
use codex_protocol::approvals::NetworkApprovalProtocol;
use codex_protocol::models::AdditionalPermissionProfile;
use codex_protocol::request_permissions::RequestPermissionProfile;
use codex_utils_absolute_path::AbsolutePathBuf;
use serde::Serialize;
use serde_json::Value;

use super::GUARDIAN_MAX_ACTION_STRING_TOKENS;
use super::GUARDIAN_MAX_ACTION_SUMMARY_TOKENS;
use super::GUARDIAN_MAX_ACTION_TOKENS;
use super::GUARDIAN_MAX_ASSESSMENT_INPUT_TOKENS;
use super::GUARDIAN_MAX_ASSESSMENT_REASON_TOKENS;
use super::prompt::guardian_truncate_text;

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum GuardianApprovalRequest {
    Shell {
        id: String,
        command: Vec<String>,
        cwd: AbsolutePathBuf,
        sandbox_permissions: crate::sandboxing::SandboxPermissions,
        additional_permissions: Option<AdditionalPermissionProfile>,
        justification: Option<String>,
    },
    ExecCommand {
        id: String,
        command: Vec<String>,
        cwd: AbsolutePathBuf,
        sandbox_permissions: crate::sandboxing::SandboxPermissions,
        additional_permissions: Option<AdditionalPermissionProfile>,
        justification: Option<String>,
        tty: bool,
    },
    #[cfg(unix)]
    Execve {
        id: String,
        source: GuardianCommandSource,
        program: String,
        argv: Vec<String>,
        cwd: AbsolutePathBuf,
        additional_permissions: Option<AdditionalPermissionProfile>,
    },
    ApplyPatch {
        id: String,
        cwd: AbsolutePathBuf,
        files: Vec<AbsolutePathBuf>,
        patch: String,
    },
    NetworkAccess {
        id: String,
        turn_id: String,
        target: String,
        host: String,
        protocol: NetworkApprovalProtocol,
        port: u16,
        trigger: Option<GuardianNetworkAccessTrigger>,
    },
    McpToolCall {
        id: String,
        server: String,
        tool_name: String,
        arguments: Option<Value>,
        connector_id: Option<String>,
        connector_name: Option<String>,
        connector_description: Option<String>,
        connected_account_email: Option<String>,
        tool_title: Option<String>,
        tool_description: Option<String>,
        annotations: Option<GuardianMcpAnnotations>,
    },
    PreToolUse {
        id: String,
        tool_name: String,
        tool_input: Value,
        execution_target: Option<Value>,
        reason: String,
        cwd: AbsolutePathBuf,
    },
    RequestPermissions {
        id: String,
        turn_id: String,
        reason: Option<String>,
        permissions: RequestPermissionProfile,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GuardianNetworkAccessTrigger {
    pub(crate) call_id: String,
    pub(crate) tool_name: String,
    pub(crate) command: Vec<String>,
    pub(crate) cwd: AbsolutePathBuf,
    pub(crate) sandbox_permissions: crate::sandboxing::SandboxPermissions,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) additional_permissions: Option<AdditionalPermissionProfile>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) justification: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) tty: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct GuardianMcpAnnotations {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) destructive_hint: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) open_world_hint: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) read_only_hint: Option<bool>,
}

#[derive(Serialize)]
struct CommandApprovalAction<'a> {
    tool: &'a str,
    command: &'a [String],
    cwd: &'a Path,
    sandbox_permissions: crate::sandboxing::SandboxPermissions,
    #[serde(skip_serializing_if = "Option::is_none")]
    additional_permissions: Option<&'a AdditionalPermissionProfile>,
    #[serde(skip_serializing_if = "Option::is_none")]
    justification: Option<&'a String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tty: Option<bool>,
}

#[cfg(unix)]
#[derive(Serialize)]
struct ExecveApprovalAction<'a> {
    tool: &'a str,
    program: &'a str,
    argv: &'a [String],
    cwd: &'a Path,
    #[serde(skip_serializing_if = "Option::is_none")]
    additional_permissions: Option<&'a AdditionalPermissionProfile>,
}

#[derive(Serialize)]
struct McpToolCallApprovalAction<'a> {
    tool: &'static str,
    server: &'a str,
    tool_name: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    arguments: Option<&'a Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    connector_id: Option<&'a String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    connector_name: Option<&'a String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    connector_description: Option<&'a String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    connected_account_email: Option<&'a String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_title: Option<&'a String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_description: Option<&'a String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    annotations: Option<&'a GuardianMcpAnnotations>,
}

#[derive(Serialize)]
struct PreToolUseApprovalAction<'a> {
    tool: &'static str,
    tool_name: &'a str,
    tool_input: &'a Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    execution_target: Option<&'a Value>,
    reason: &'a str,
    cwd: &'a Path,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct NetworkAccessApprovalAction<'a> {
    tool: &'static str,
    target: &'a str,
    host: &'a str,
    protocol: NetworkApprovalProtocol,
    port: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    trigger: Option<&'a GuardianNetworkAccessTrigger>,
}

#[derive(Serialize)]
struct RequestPermissionsApprovalAction<'a> {
    tool: &'static str,
    turn_id: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<&'a String>,
    permissions: &'a RequestPermissionProfile,
}

fn serialize_guardian_action(value: impl Serialize) -> serde_json::Result<Value> {
    serde_json::to_value(value)
}

fn serialize_command_guardian_action(
    tool: &'static str,
    command: &[String],
    cwd: &Path,
    sandbox_permissions: crate::sandboxing::SandboxPermissions,
    additional_permissions: Option<&AdditionalPermissionProfile>,
    justification: Option<&String>,
    tty: Option<bool>,
) -> serde_json::Result<Value> {
    serialize_guardian_action(CommandApprovalAction {
        tool,
        command,
        cwd,
        sandbox_permissions,
        additional_permissions,
        justification,
        tty,
    })
}

fn command_assessment_action(
    source: GuardianCommandSource,
    command: &[String],
    cwd: &AbsolutePathBuf,
) -> GuardianAssessmentAction {
    GuardianAssessmentAction::Command {
        source,
        command: codex_shell_command::parse_command::shlex_join(command),
        cwd: cwd.clone(),
    }
}

#[cfg(unix)]
fn guardian_command_source_tool_name(source: GuardianCommandSource) -> &'static str {
    match source {
        GuardianCommandSource::Shell => "shell",
        GuardianCommandSource::UnifiedExec => "exec_command",
    }
}

fn truncate_guardian_action_value(value: Value) -> (Value, bool) {
    match value {
        Value::String(text) => {
            let (text, truncated) =
                guardian_truncate_text(&text, GUARDIAN_MAX_ACTION_STRING_TOKENS);
            (Value::String(text), truncated)
        }
        Value::Array(values) => {
            let mut truncated = false;
            let values = values
                .into_iter()
                .map(|value| {
                    let (value, value_truncated) = truncate_guardian_action_value(value);
                    truncated |= value_truncated;
                    value
                })
                .collect::<Vec<_>>();
            (Value::Array(values), truncated)
        }
        Value::Object(values) => {
            let mut entries = values.into_iter().collect::<Vec<_>>();
            entries.sort_by(|(left, _), (right, _)| left.cmp(right));
            let mut truncated = false;
            let values = entries
                .into_iter()
                .map(|(key, value)| {
                    let (value, value_truncated) = truncate_guardian_action_value(value);
                    truncated |= value_truncated;
                    (key, value)
                })
                .collect();
            (Value::Object(values), truncated)
        }
        other => (other, false),
    }
}

fn bounded_guardian_assessment_input(value: &Value) -> Value {
    let (summary, truncated) =
        guardian_truncate_text(&value.to_string(), GUARDIAN_MAX_ASSESSMENT_INPUT_TOKENS);
    if truncated {
        Value::String(summary)
    } else {
        value.clone()
    }
}

fn bounded_guardian_pretty_assessment_input(value: &Value) -> Value {
    let value = bounded_guardian_assessment_input(value);
    let Ok(pretty) = serde_json::to_string_pretty(&value) else {
        return value;
    };
    let (summary, truncated) =
        guardian_truncate_text(&pretty, GUARDIAN_MAX_ASSESSMENT_INPUT_TOKENS);
    if truncated {
        Value::String(summary)
    } else {
        value
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FormattedGuardianAction {
    pub(crate) text: String,
    pub(crate) truncated: bool,
}

pub(crate) fn guardian_approval_request_to_json(
    action: &GuardianApprovalRequest,
) -> serde_json::Result<Value> {
    match action {
        GuardianApprovalRequest::Shell {
            id: _,
            command,
            cwd,
            sandbox_permissions,
            additional_permissions,
            justification,
        } => serialize_command_guardian_action(
            "shell",
            command,
            cwd,
            *sandbox_permissions,
            additional_permissions.as_ref(),
            justification.as_ref(),
            /*tty*/ None,
        ),
        GuardianApprovalRequest::ExecCommand {
            id: _,
            command,
            cwd,
            sandbox_permissions,
            additional_permissions,
            justification,
            tty,
        } => serialize_command_guardian_action(
            "exec_command",
            command,
            cwd,
            *sandbox_permissions,
            additional_permissions.as_ref(),
            justification.as_ref(),
            Some(*tty),
        ),
        #[cfg(unix)]
        GuardianApprovalRequest::Execve {
            id: _,
            source,
            program,
            argv,
            cwd,
            additional_permissions,
        } => serialize_guardian_action(ExecveApprovalAction {
            tool: guardian_command_source_tool_name(*source),
            program,
            argv,
            cwd,
            additional_permissions: additional_permissions.as_ref(),
        }),
        GuardianApprovalRequest::ApplyPatch {
            id: _,
            cwd,
            files,
            patch,
        } => Ok(serde_json::json!({
            "tool": "apply_patch",
            "cwd": cwd,
            "files": files,
            "patch": patch,
        })),
        GuardianApprovalRequest::NetworkAccess {
            id: _,
            turn_id: _,
            target,
            host,
            protocol,
            port,
            trigger,
        } => serialize_guardian_action(NetworkAccessApprovalAction {
            tool: "network_access",
            target,
            host,
            protocol: *protocol,
            port: *port,
            trigger: trigger.as_ref(),
        }),
        GuardianApprovalRequest::McpToolCall {
            id: _,
            server,
            tool_name,
            arguments,
            connector_id,
            connector_name,
            connector_description,
            connected_account_email,
            tool_title,
            tool_description,
            annotations,
        } => serialize_guardian_action(McpToolCallApprovalAction {
            tool: "mcp_tool_call",
            server,
            tool_name,
            arguments: arguments.as_ref(),
            connector_id: connector_id.as_ref(),
            connector_name: connector_name.as_ref(),
            connector_description: connector_description.as_ref(),
            connected_account_email: connected_account_email.as_ref(),
            tool_title: tool_title.as_ref(),
            tool_description: tool_description.as_ref(),
            annotations: annotations.as_ref(),
        }),
        GuardianApprovalRequest::PreToolUse {
            id: _,
            tool_name,
            tool_input,
            execution_target,
            reason,
            cwd,
        } => serialize_guardian_action(PreToolUseApprovalAction {
            tool: "pre_tool_use",
            tool_name,
            tool_input,
            execution_target: execution_target.as_ref(),
            reason,
            cwd,
        }),
        GuardianApprovalRequest::RequestPermissions {
            id: _,
            turn_id,
            reason,
            permissions,
        } => serialize_guardian_action(RequestPermissionsApprovalAction {
            tool: "request_permissions",
            turn_id,
            reason: reason.as_ref(),
            permissions,
        }),
    }
}

pub(crate) fn guardian_assessment_action(
    action: &GuardianApprovalRequest,
) -> GuardianAssessmentAction {
    match action {
        GuardianApprovalRequest::Shell { command, cwd, .. } => {
            command_assessment_action(GuardianCommandSource::Shell, command, cwd)
        }
        GuardianApprovalRequest::ExecCommand { command, cwd, .. } => {
            command_assessment_action(GuardianCommandSource::UnifiedExec, command, cwd)
        }
        #[cfg(unix)]
        GuardianApprovalRequest::Execve {
            source,
            program,
            argv,
            cwd,
            ..
        } => GuardianAssessmentAction::Execve {
            source: *source,
            program: program.clone(),
            argv: argv.clone(),
            cwd: cwd.clone(),
        },
        GuardianApprovalRequest::ApplyPatch { cwd, files, .. } => {
            GuardianAssessmentAction::ApplyPatch {
                cwd: cwd.clone(),
                files: files.clone(),
            }
        }
        GuardianApprovalRequest::NetworkAccess {
            id: _id,
            turn_id: _turn_id,
            target,
            host,
            protocol,
            port,
            trigger: _trigger,
        } => GuardianAssessmentAction::NetworkAccess {
            target: target.clone(),
            host: host.clone(),
            protocol: *protocol,
            port: *port,
        },
        GuardianApprovalRequest::McpToolCall {
            server,
            tool_name,
            connector_id,
            connector_name,
            tool_title,
            ..
        } => GuardianAssessmentAction::McpToolCall {
            server: server.clone(),
            tool_name: tool_name.clone(),
            connector_id: connector_id.clone(),
            connector_name: connector_name.clone(),
            tool_title: tool_title.clone(),
        },
        GuardianApprovalRequest::PreToolUse {
            tool_name,
            tool_input,
            reason,
            ..
        } => GuardianAssessmentAction::PreToolUse {
            tool_name: guardian_truncate_text(tool_name, GUARDIAN_MAX_ASSESSMENT_REASON_TOKENS).0,
            tool_input: bounded_guardian_assessment_input(tool_input),
            reason: guardian_truncate_text(reason, GUARDIAN_MAX_ASSESSMENT_REASON_TOKENS).0,
        },
        GuardianApprovalRequest::RequestPermissions {
            reason,
            permissions,
            ..
        } => GuardianAssessmentAction::RequestPermissions {
            reason: reason.clone(),
            permissions: permissions.clone(),
        },
    }
}

pub(crate) fn guardian_reviewed_action(
    request: &GuardianApprovalRequest,
) -> GuardianReviewedAction {
    match request {
        GuardianApprovalRequest::Shell {
            sandbox_permissions,
            additional_permissions,
            ..
        } => GuardianReviewedAction::Shell {
            sandbox_permissions: *sandbox_permissions,
            additional_permissions: additional_permissions.clone(),
        },
        GuardianApprovalRequest::ExecCommand {
            sandbox_permissions,
            additional_permissions,
            tty,
            ..
        } => GuardianReviewedAction::UnifiedExec {
            sandbox_permissions: *sandbox_permissions,
            additional_permissions: additional_permissions.clone(),
            tty: *tty,
        },
        #[cfg(unix)]
        GuardianApprovalRequest::Execve {
            source,
            program,
            additional_permissions,
            ..
        } => GuardianReviewedAction::Execve {
            source: *source,
            program: program.clone(),
            additional_permissions: additional_permissions.clone(),
        },
        GuardianApprovalRequest::ApplyPatch { .. } => GuardianReviewedAction::ApplyPatch {},
        GuardianApprovalRequest::NetworkAccess { protocol, port, .. } => {
            GuardianReviewedAction::NetworkAccess {
                protocol: *protocol,
                port: *port,
            }
        }
        GuardianApprovalRequest::McpToolCall {
            server,
            tool_name,
            connector_id,
            connector_name,
            tool_title,
            ..
        } => GuardianReviewedAction::McpToolCall {
            server: server.clone(),
            tool_name: tool_name.clone(),
            connector_id: connector_id.clone(),
            connector_name: connector_name.clone(),
            tool_title: tool_title.clone(),
        },
        GuardianApprovalRequest::PreToolUse { tool_name, .. } => {
            GuardianReviewedAction::PreToolUse {
                tool_name: tool_name.clone(),
            }
        }
        GuardianApprovalRequest::RequestPermissions { .. } => {
            GuardianReviewedAction::RequestPermissions {}
        }
    }
}

pub(crate) fn guardian_request_target_item_id(request: &GuardianApprovalRequest) -> Option<&str> {
    match request {
        GuardianApprovalRequest::Shell { id, .. }
        | GuardianApprovalRequest::ExecCommand { id, .. }
        | GuardianApprovalRequest::ApplyPatch { id, .. }
        | GuardianApprovalRequest::McpToolCall { id, .. }
        | GuardianApprovalRequest::RequestPermissions { id, .. } => Some(id),
        // PreToolUse targets the model tool call even though its ThreadItem is
        // emitted only after Guardian allows the handler to run.
        GuardianApprovalRequest::PreToolUse { id, .. } => Some(id),
        GuardianApprovalRequest::NetworkAccess { .. } => None,
        #[cfg(unix)]
        GuardianApprovalRequest::Execve { id, .. } => Some(id),
    }
}

pub(crate) fn guardian_request_turn_id<'a>(
    request: &'a GuardianApprovalRequest,
    default_turn_id: &'a str,
) -> &'a str {
    match request {
        GuardianApprovalRequest::NetworkAccess { turn_id, .. }
        | GuardianApprovalRequest::RequestPermissions { turn_id, .. } => turn_id,
        GuardianApprovalRequest::Shell { .. }
        | GuardianApprovalRequest::ExecCommand { .. }
        | GuardianApprovalRequest::ApplyPatch { .. }
        | GuardianApprovalRequest::McpToolCall { .. }
        | GuardianApprovalRequest::PreToolUse { .. } => default_turn_id,
        #[cfg(unix)]
        GuardianApprovalRequest::Execve { .. } => default_turn_id,
    }
}

pub(crate) fn format_guardian_action_pretty(
    action: &GuardianApprovalRequest,
) -> serde_json::Result<FormattedGuardianAction> {
    let value = guardian_approval_request_to_json(action)?;
    let (value, fields_truncated) = truncate_guardian_action_value(value);
    let text = serde_json::to_string_pretty(&value)?;
    let (_, action_truncated) = guardian_truncate_text(&text, GUARDIAN_MAX_ACTION_TOKENS);
    let text = if action_truncated {
        let summary = guardian_truncate_text(&text, GUARDIAN_MAX_ACTION_SUMMARY_TOKENS).0;
        serde_json::to_string_pretty(&serde_json::json!({
            "tool": value.get("tool"),
            "tool_name": value.get("tool_name").map(bounded_guardian_assessment_input),
            "tool_input": value.get("tool_input").map(bounded_guardian_pretty_assessment_input),
            "execution_target": value.get("execution_target").map(bounded_guardian_pretty_assessment_input),
            "reason": value.get("reason").map(bounded_guardian_assessment_input),
            "summary": summary,
        }))?
    } else {
        text
    };
    Ok(FormattedGuardianAction {
        text,
        truncated: fields_truncated || action_truncated,
    })
}
