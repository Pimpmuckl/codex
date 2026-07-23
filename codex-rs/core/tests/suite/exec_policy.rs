#![allow(clippy::unwrap_used)]

use anyhow::Result;
use codex_config::test_support::CloudConfigBundleFixture;
use codex_exec_server::CreateDirectoryOptions;
use codex_features::Feature;
use codex_protocol::config_types::CollaborationMode;
use codex_protocol::config_types::ModeKind;
use codex_protocol::config_types::Settings;
use codex_protocol::models::PermissionProfile;
use codex_protocol::protocol::AskForApproval;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::GranularApprovalConfig;
use codex_protocol::protocol::Op;
use codex_protocol::protocol::ReviewDecision;
use codex_protocol::protocol::TurnEnvironmentSelections;
use codex_protocol::user_input::UserInput;
use core_test_support::TestTargetOs;
use core_test_support::hooks::trust_discovered_hooks;
use core_test_support::responses::ev_assistant_message;
use core_test_support::responses::ev_completed;
use core_test_support::responses::ev_function_call;
use core_test_support::responses::ev_response_created;
use core_test_support::responses::mount_sse_once;
use core_test_support::responses::mount_sse_sequence;
use core_test_support::responses::sse;
use core_test_support::responses::start_mock_server;
use core_test_support::skip_if_no_network;
use core_test_support::skip_if_target_windows;
use core_test_support::test_codex::local_selections;
use core_test_support::test_codex::test_codex;
use core_test_support::test_codex::turn_permission_fields;
use core_test_support::test_target_os;
use core_test_support::wait_for_event;
use serde_json::Value;
use serde_json::json;
use std::fs;
use std::path::Path;

const COMPLEX_FORCED_RM_COMMAND: &str = "for target in \"\"; do rm -rf \"$target\"; done";
const REVIEWED_WORKDIR: &str = "reviewed-workdir";

fn install_pre_tool_use_ask_hook(home: &Path) -> Result<()> {
    let script_path = home.join("pre_tool_use_ask.py");
    fs::write(
        &script_path,
        r#"import json, sys
json.load(sys.stdin)
print(json.dumps({"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"ask","permissionDecisionReason":"Review this exact shell command"}}))
"#,
    )?;
    let python = if cfg!(windows) { "python" } else { "python3" };
    let command = serde_json::to_string(&format!("{python} \"{}\"", script_path.display()))?;
    fs::write(
        home.join("hooks.json"),
        format!(
            r#"{{"hooks":{{"PreToolUse":[{{"matcher":"^Bash$","hooks":[{{"type":"command","command":{command}}}]}}]}}}}"#
        ),
    )?;
    Ok(())
}

fn shell_tool_sse(
    response_id: &str,
    call_id: &str,
    tool_name: &str,
    command: &str,
    environment_id: &str,
) -> String {
    let arguments = if tool_name == "exec_command" {
        json!({
            "cmd": command,
            "workdir": REVIEWED_WORKDIR,
            "environment_id": environment_id,
        })
    } else {
        json!({ "command": command })
    };
    sse(vec![
        ev_response_created(response_id),
        ev_function_call(call_id, tool_name, &arguments.to_string()),
        ev_completed(response_id),
    ])
}

fn guardian_review_sse(response_id: &str, outcome: &str) -> String {
    sse(vec![
        ev_response_created(response_id),
        ev_assistant_message(
            &format!("msg-{response_id}"),
            &json!({
                "risk_level": if outcome == "allow" { "low" } else { "high" },
                "user_authorization": if outcome == "allow" { "high" } else { "low" },
                "outcome": outcome,
                "rationale": "Reviewed the exact invocation."
            })
            .to_string(),
        ),
        ev_completed(response_id),
    ])
}

fn collaboration_mode_for_model(model: String) -> CollaborationMode {
    CollaborationMode {
        mode: ModeKind::Default,
        settings: Settings {
            model,
            reasoning_effort: None,
            developer_instructions: Some("exercise approvals in collaboration mode".to_string()),
        },
    }
}

async fn submit_user_turn(
    test: &core_test_support::test_codex::TestCodex,
    prompt: &str,
    approval_policy: AskForApproval,
    permission_profile: PermissionProfile,
    collaboration_mode: Option<CollaborationMode>,
) -> Result<()> {
    let session_model = test.session_configured.model.clone();
    let (sandbox_policy, permission_profile) =
        turn_permission_fields(permission_profile, test.config.cwd.as_path());
    let environments = TurnEnvironmentSelections::new(
        test.config.cwd.clone(),
        vec![test.executor_environment().selection().clone()],
    );
    test.codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: prompt.into(),
                text_elements: Vec::new(),
            }],
            final_output_json_schema: None,
            responsesapi_client_metadata: None,
            additional_context: Default::default(),
            thread_settings: codex_protocol::protocol::ThreadSettingsOverrides {
                environments: Some(environments),
                approval_policy: Some(approval_policy),
                sandbox_policy: Some(sandbox_policy),
                permission_profile,
                collaboration_mode: collaboration_mode.or({
                    Some(codex_protocol::config_types::CollaborationMode {
                        mode: codex_protocol::config_types::ModeKind::Default,
                        settings: codex_protocol::config_types::Settings {
                            model: session_model,
                            reasoning_effort: None,
                            developer_instructions: None,
                        },
                    })
                }),
                ..Default::default()
            },
        })
        .await?;
    Ok(())
}

fn assert_no_matched_rules_invariant(output_item: &Value) {
    let output = output_item
        .get("output")
        .and_then(Value::as_str)
        .expect("function call output should include a string output payload");
    assert!(
        !output.contains("invariant failed: matched_rules must be non-empty"),
        "unexpected invariant panic surfaced in output: {output}"
    );
}

async fn assert_pre_tool_use_ask_authorizes_only_reviewed_shell_tool(
    tool_name: &str,
    permission_profile: PermissionProfile,
) -> Result<()> {
    skip_if_no_network!(Ok(()));

    const APPROVED_MARKER: &str = "pretooluse-approved-marker";
    const DENIED_MARKER: &str = "pretooluse-denied-marker";
    const EXECUTION_LOG: &str = "pretooluse-executions";
    let approved_call_id = format!("pretooluse-ask-{tool_name}-approved");
    let denied_call_id = format!("pretooluse-ask-{tool_name}-denied");
    let unified_exec = tool_name == "exec_command";
    let approved_executes = matches!(&permission_profile, PermissionProfile::Disabled);
    let server = start_mock_server().await;
    let mut builder = test_codex()
        .with_model("test-gpt-5.1-codex")
        .with_windows_cmd_shell()
        .with_pre_build_hook(|home| {
            install_pre_tool_use_ask_hook(home).expect("install PreToolUse ask hook");
        })
        .with_config(move |config| {
            trust_discovered_hooks(config);
            if unified_exec {
                config
                    .features
                    .enable(Feature::UnifiedExec)
                    .expect("enable unified exec");
            }
        });
    let test = if unified_exec {
        builder.build_with_auto_env(&server).await?
    } else {
        builder.build(&server).await?
    };
    let selection = test.executor_environment().selection().clone();
    let cwd = if unified_exec {
        let cwd = selection.cwd.join(REVIEWED_WORKDIR)?;
        test.fs()
            .create_directory(
                &cwd,
                CreateDirectoryOptions { recursive: true },
                /*sandbox*/ None,
            )
            .await?;
        cwd
    } else {
        selection.cwd.clone()
    };
    let approved_marker = cwd.join(APPROVED_MARKER)?;
    let denied_marker = cwd.join(DENIED_MARKER)?;
    let execution_log = cwd.join(EXECUTION_LOG)?;
    let (approved_command, denied_command) = match test_target_os() {
        TestTargetOs::Linux | TestTargetOs::MacOs => (
            format!("printf 'executed\\n' >> {EXECUTION_LOG}; rm -f {APPROVED_MARKER}"),
            format!("printf 'denied\\n' >> {EXECUTION_LOG}; rm -f {DENIED_MARKER}"),
        ),
        TestTargetOs::Windows => (
            format!("echo executed>>{EXECUTION_LOG} & del /f /q {APPROVED_MARKER}"),
            format!("echo denied>>{EXECUTION_LOG} & del /f /q {DENIED_MARKER}"),
        ),
    };
    let responses = mount_sse_sequence(
        &server,
        vec![
            shell_tool_sse(
                "resp-approved-tool",
                &approved_call_id,
                tool_name,
                &approved_command,
                &selection.environment_id,
            ),
            guardian_review_sse("resp-approved-guardian", "allow"),
            shell_tool_sse(
                "resp-denied-tool",
                &denied_call_id,
                tool_name,
                &denied_command,
                &selection.environment_id,
            ),
            guardian_review_sse("resp-denied-guardian", "deny"),
            sse(vec![
                ev_response_created("resp-parent-done"),
                ev_assistant_message("msg-parent-done", "done"),
                ev_completed("resp-parent-done"),
            ]),
        ],
    )
    .await;
    for marker in [&approved_marker, &denied_marker] {
        test.fs()
            .write_file(marker, b"seed".to_vec(), /*sandbox*/ None)
            .await?;
    }

    submit_user_turn(
        &test,
        "review the requested shell commands",
        AskForApproval::Never,
        permission_profile,
        /*collaboration_mode*/ None,
    )
    .await?;
    wait_for_event(&test.codex, |event| {
        matches!(event, EventMsg::TurnComplete(_))
    })
    .await;

    let requests = responses.requests();
    let approved_output = requests
        .iter()
        .find_map(|request| request.function_call_output_text(&approved_call_id))
        .expect("approved invocation output");
    test.fs()
        .get_metadata(&denied_marker, /*sandbox*/ None)
        .await?;
    if approved_executes {
        assert!(
            test.fs()
                .get_metadata(&approved_marker, /*sandbox*/ None)
                .await
                .is_err(),
            "Guardian-approved dangerous command should execute: command={approved_command:?} marker={approved_marker} output={approved_output}"
        );
        let execution_log = test
            .fs()
            .read_file(&execution_log, /*sandbox*/ None)
            .await?;
        assert_eq!(String::from_utf8(execution_log)?.trim(), "executed");
    } else {
        test.fs()
            .get_metadata(&approved_marker, /*sandbox*/ None)
            .await?;
        assert!(approved_output.contains("rejected: blocked by policy"));
    }
    let guardian_requests = requests
        .iter()
        .filter(|request| {
            request.body_json()["client_metadata"]["x-openai-subagent"].as_str() == Some("guardian")
        })
        .collect::<Vec<_>>();
    assert_eq!(guardian_requests.len(), 2);
    assert!(guardian_requests[0].body_contains_text(&approved_command));
    assert!(guardian_requests[0].body_contains_text(&selection.environment_id));
    assert!(guardian_requests[0].body_contains_text(&cwd.to_string()));
    assert!(guardian_requests[1].body_contains_text(&denied_command));
    assert!(
        requests
            .iter()
            .any(|request| request.function_call_output_text(&denied_call_id).is_some()),
        "the denied invocation should return a blocked tool result"
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pre_tool_use_ask_authorizes_only_reviewed_unified_exec_under_yolo() -> Result<()> {
    assert_pre_tool_use_ask_authorizes_only_reviewed_shell_tool(
        "exec_command",
        PermissionProfile::Disabled,
    )
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pre_tool_use_ask_authorizes_only_reviewed_shell_command_under_yolo() -> Result<()> {
    assert_pre_tool_use_ask_authorizes_only_reviewed_shell_tool(
        "shell_command",
        PermissionProfile::Disabled,
    )
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pre_tool_use_ask_does_not_authorize_non_yolo_unified_exec() -> Result<()> {
    assert_pre_tool_use_ask_authorizes_only_reviewed_shell_tool(
        "exec_command",
        PermissionProfile::read_only(),
    )
    .await
}

#[tokio::test]
async fn startup_migrates_default_policy_and_honors_ignore_rules() -> Result<()> {
    const LEGACY_POLICY: &str = r#"prefix_rule(pattern=["rm"], decision="allow")
prefix_rule(pattern=["git", "status"], decision="allow")
"#;
    const MIGRATED_POLICY: &str = r#"prefix_rule(pattern=["git", "status"], decision="allow")
"#;
    const MIGRATION_MARKER_FILENAME: &str = ".sandbox_migration";

    let server = start_mock_server().await;
    let mut migrated_builder = test_codex().with_config(|config| {
        let policy_path = config.codex_home.join("rules/default.rules");
        fs::create_dir_all(policy_path.parent().expect("rules directory"))
            .expect("create rules directory");
        fs::write(policy_path, LEGACY_POLICY).expect("write legacy policy");
    });
    let migrated = migrated_builder.build_with_auto_env(&server).await?;
    let migrated_policy_path = migrated.codex_home_path().join("rules/default.rules");
    assert_eq!(fs::read_to_string(&migrated_policy_path)?, MIGRATED_POLICY);
    assert_eq!(
        fs::read_to_string(migrated.codex_home_path().join(MIGRATION_MARKER_FILENAME))?,
        "v1\n"
    );

    let mut ignored_builder = test_codex().with_config(|config| {
        let policy_path = config.codex_home.join("rules/default.rules");
        fs::create_dir_all(policy_path.parent().expect("rules directory"))
            .expect("create rules directory");
        fs::write(policy_path, LEGACY_POLICY).expect("write legacy policy");
        config.config_layer_stack = config
            .config_layer_stack
            .clone()
            .with_user_and_project_exec_policy_rules_ignored(
                /*ignore_user_and_project_exec_policy_rules*/ true,
            );
    });
    let ignored = ignored_builder.build_with_auto_env(&server).await?;
    let ignored_policy_path = ignored.codex_home_path().join("rules/default.rules");
    assert_eq!(fs::read_to_string(&ignored_policy_path)?, LEGACY_POLICY);
    assert!(
        !ignored
            .codex_home_path()
            .join(MIGRATION_MARKER_FILENAME)
            .exists()
    );

    Ok(())
}

#[tokio::test]
async fn granular_complex_forced_rm_denial_explains_why_the_command_was_rejected() -> Result<()> {
    skip_if_target_windows!(Ok(()), "uses a POSIX shell command fixture");

    let server = start_mock_server().await;
    let mut builder = test_codex();
    let test = builder.build_with_auto_env(&server).await?;
    let call_id = "forced-rm-denied";
    let args = json!({
        "command": COMPLEX_FORCED_RM_COMMAND,
        "timeout_ms": 1_000,
    });

    mount_sse_once(
        &server,
        sse(vec![
            ev_response_created("resp-forced-rm-1"),
            ev_function_call(call_id, "shell_command", &serde_json::to_string(&args)?),
            ev_completed("resp-forced-rm-1"),
        ]),
    )
    .await;
    let results_mock = mount_sse_once(
        &server,
        sse(vec![
            ev_assistant_message("msg-forced-rm-1", "done"),
            ev_completed("resp-forced-rm-2"),
        ]),
    )
    .await;

    submit_user_turn(
        &test,
        "run the forced rm loop",
        AskForApproval::Granular(GranularApprovalConfig {
            sandbox_approval: false,
            rules: true,
            skill_approval: true,
            request_permissions: true,
            mcp_elicitations: true,
        }),
        PermissionProfile::read_only(),
        /*collaboration_mode*/ None,
    )
    .await?;

    wait_for_event(&test.codex, |event| {
        matches!(event, EventMsg::TurnComplete(_))
    })
    .await;

    let output_item = results_mock.single_request().function_call_output(call_id);
    let output = output_item
        .get("output")
        .and_then(Value::as_str)
        .expect("function call output should include a string output payload");
    assert!(
        output.contains("rm -f style commands are not permitted. Use a safer approach"),
        "unexpected output: {output}"
    );

    Ok(())
}

#[tokio::test]
async fn granular_complex_forced_rm_requests_approval_when_allowed() -> Result<()> {
    skip_if_target_windows!(Ok(()), "uses a POSIX shell command fixture");

    let server = start_mock_server().await;
    let mut builder = test_codex();
    let test = builder.build_with_auto_env(&server).await?;
    let call_id = "forced-rm-approval";
    let args = json!({
        "command": COMPLEX_FORCED_RM_COMMAND,
        "timeout_ms": 1_000,
    });

    mount_sse_once(
        &server,
        sse(vec![
            ev_response_created("resp-forced-rm-approval-1"),
            ev_function_call(call_id, "shell_command", &serde_json::to_string(&args)?),
            ev_completed("resp-forced-rm-approval-1"),
        ]),
    )
    .await;
    mount_sse_once(
        &server,
        sse(vec![
            ev_assistant_message("msg-forced-rm-approval-1", "done"),
            ev_completed("resp-forced-rm-approval-2"),
        ]),
    )
    .await;

    submit_user_turn(
        &test,
        "run the forced rm loop",
        AskForApproval::Granular(GranularApprovalConfig {
            sandbox_approval: true,
            rules: true,
            skill_approval: true,
            request_permissions: true,
            mcp_elicitations: true,
        }),
        PermissionProfile::read_only(),
        /*collaboration_mode*/ None,
    )
    .await?;

    let approval_event = wait_for_event(&test.codex, |event| {
        matches!(
            event,
            EventMsg::ExecApprovalRequest(_) | EventMsg::TurnComplete(_)
        )
    })
    .await;
    let EventMsg::ExecApprovalRequest(approval) = approval_event else {
        panic!("expected forced rm to request approval before turn completion");
    };
    assert_eq!(
        approval.command.last().map(String::as_str),
        Some(COMPLEX_FORCED_RM_COMMAND)
    );

    test.codex
        .submit(Op::ExecApproval {
            id: approval.effective_approval_id(),
            turn_id: None,
            decision: ReviewDecision::denied("rejected by user"),
        })
        .await?;
    wait_for_event(&test.codex, |event| {
        matches!(event, EventMsg::TurnComplete(_))
    })
    .await;

    Ok(())
}

#[cfg(windows)]
#[tokio::test]
async fn unified_exec_disabled_windows_sandbox_rejects_managed_read_only_command() -> Result<()> {
    let server = start_mock_server().await;
    let mut builder = test_codex().with_config(|config| {
        config
            .features
            .enable(Feature::UnifiedExec)
            .expect("test config should allow feature update");
        config
            .features
            .disable(Feature::WindowsSandbox)
            .expect("test config should allow feature update");
        config
            .features
            .disable(Feature::WindowsSandboxElevated)
            .expect("test config should allow feature update");
        config.set_windows_sandbox_enabled(false);
        config.set_windows_elevated_sandbox_enabled(false);
    });
    let test = builder.build(&server).await?;
    let call_id = "unified-exec-disabled-windows-sandbox-read-only";
    let args = json!({
        "cmd": "cmd.exe /c dir",
        "yield_time_ms": 1_000,
    });

    mount_sse_once(
        &server,
        sse(vec![
            ev_response_created("resp-disabled-windows-sandbox-1"),
            ev_function_call(call_id, "exec_command", &serde_json::to_string(&args)?),
            ev_completed("resp-disabled-windows-sandbox-1"),
        ]),
    )
    .await;
    let results_mock = mount_sse_once(
        &server,
        sse(vec![
            ev_assistant_message("msg-disabled-windows-sandbox-1", "done"),
            ev_completed("resp-disabled-windows-sandbox-2"),
        ]),
    )
    .await;

    submit_user_turn(
        &test,
        "run unified exec with disabled Windows sandbox",
        AskForApproval::Never,
        PermissionProfile::read_only(),
        None,
    )
    .await?;

    wait_for_event(&test.codex, |event| {
        matches!(event, EventMsg::TurnComplete(_))
    })
    .await;

    let output_item = results_mock.single_request().function_call_output(call_id);
    let output = output_item
        .get("output")
        .and_then(Value::as_str)
        .expect("function call output should include a string output payload");
    assert!(
        output.contains("cmd.exe /c dir") && output.contains("rejected: blocked by policy"),
        "unexpected output: {output}",
    );

    Ok(())
}

#[tokio::test]
async fn execpolicy_blocks_shell_invocation() -> Result<()> {
    let mut builder = test_codex()
        .with_model("test-gpt-5.1-codex")
        .with_pre_build_hook(|home| {
            install_pre_tool_use_ask_hook(home).expect("install PreToolUse ask hook");
        })
        .with_config(|config| {
            trust_discovered_hooks(config);
            let policy_path = config.codex_home.join("rules").join("policy.rules");
            fs::create_dir_all(
                policy_path
                    .parent()
                    .expect("policy directory must have a parent"),
            )
            .expect("create policy directory");
            fs::write(
                &policy_path,
                r#"prefix_rule(pattern=["echo"], decision="forbidden")"#,
            )
            .expect("write policy file");
        });
    let server = start_mock_server().await;
    let test = builder.build(&server).await?;

    let call_id = "shell-forbidden";
    let args = json!({
        "command": "echo blocked",
        "timeout_ms": 1_000,
    });

    let responses = mount_sse_sequence(
        &server,
        vec![
            sse(vec![
                ev_response_created("resp-1"),
                ev_function_call(call_id, "shell_command", &serde_json::to_string(&args)?),
                ev_completed("resp-1"),
            ]),
            guardian_review_sse("resp-guardian", "allow"),
            sse(vec![
                ev_assistant_message("msg-1", "done"),
                ev_completed("resp-2"),
            ]),
        ],
    )
    .await;

    let session_model = test.session_configured.model.clone();
    let (sandbox_policy, permission_profile) =
        turn_permission_fields(PermissionProfile::Disabled, test.config.cwd.as_path());
    test.codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: "run shell command".into(),
                text_elements: Vec::new(),
            }],
            final_output_json_schema: None,
            responsesapi_client_metadata: None,
            additional_context: Default::default(),
            thread_settings: codex_protocol::protocol::ThreadSettingsOverrides {
                environments: Some(local_selections(test.config.cwd.clone())),
                approval_policy: Some(AskForApproval::Never),
                sandbox_policy: Some(sandbox_policy),
                permission_profile,
                collaboration_mode: Some(codex_protocol::config_types::CollaborationMode {
                    mode: codex_protocol::config_types::ModeKind::Default,
                    settings: codex_protocol::config_types::Settings {
                        model: session_model,
                        reasoning_effort: None,
                        developer_instructions: None,
                    },
                }),
                ..Default::default()
            },
        })
        .await?;

    let EventMsg::ExecCommandEnd(end) = wait_for_event(&test.codex, |event| {
        matches!(event, EventMsg::ExecCommandEnd(_))
    })
    .await
    else {
        unreachable!()
    };
    wait_for_event(&test.codex, |event| {
        matches!(event, EventMsg::TurnComplete(_))
    })
    .await;

    assert!(
        end.aggregated_output
            .contains("policy forbids commands starting with `echo`"),
        "unexpected output: {}",
        end.aggregated_output
    );
    assert_eq!(
        responses
            .requests()
            .iter()
            .filter(|request| {
                request.body_json()["client_metadata"]["x-openai-subagent"].as_str()
                    == Some("guardian")
            })
            .count(),
        1,
        "the explicit execpolicy rule should be evaluated after Guardian approval"
    );

    Ok(())
}

#[tokio::test]
async fn malformed_custom_rules_preserve_managed_forbidden_prefix() -> Result<()> {
    skip_if_target_windows!(
        Ok(()),
        "managed prefix fixture uses POSIX executable semantics"
    );

    let mut builder = test_codex()
        .with_cloud_config_bundle(
            CloudConfigBundleFixture::loader_with_enterprise_requirement(
                r#"
[rules]
prefix_rules = [
    { pattern = [{ token = "echo" }], decision = "forbidden" },
]
"#,
            ),
        )
        .with_config(|config| {
            config
                .features
                .enable(Feature::UnifiedExec)
                .expect("test config should allow feature update");
            let policy_path = config.codex_home.join("rules").join("broken.rules");
            fs::create_dir_all(
                policy_path
                    .parent()
                    .expect("policy directory must have a parent"),
            )
            .expect("create policy directory");
            fs::write(policy_path, "prefix_rule(").expect("write malformed policy file");
        });
    let server = start_mock_server().await;
    let test = builder.build_with_auto_env(&server).await?;
    let call_id = "managed-shell-forbidden";
    let args = json!({
        "cmd": "echo blocked",
        "yield_time_ms": 1_000,
    });

    mount_sse_once(
        &server,
        sse(vec![
            ev_response_created("resp-managed-1"),
            ev_function_call(call_id, "exec_command", &serde_json::to_string(&args)?),
            ev_completed("resp-managed-1"),
        ]),
    )
    .await;
    let results_mock = mount_sse_once(
        &server,
        sse(vec![
            ev_assistant_message("msg-managed-1", "done"),
            ev_completed("resp-managed-2"),
        ]),
    )
    .await;

    test.submit_turn_with_approval_and_permission_profile(
        "run shell command",
        AskForApproval::Never,
        PermissionProfile::Disabled,
    )
    .await?;

    let output_item = results_mock.single_request().function_call_output(call_id);
    let output = output_item
        .get("output")
        .and_then(Value::as_str)
        .expect("function call output should include a string output payload");
    assert!(
        output.contains("policy forbids commands starting with `echo`"),
        "unexpected output: {output}"
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shell_command_empty_script_with_collaboration_mode_does_not_panic() -> Result<()> {
    let server = start_mock_server().await;
    let mut builder = test_codex().with_model("gpt-5.2").with_config(|config| {
        config
            .features
            .enable(Feature::CollaborationModes)
            .expect("test config should allow feature update");
    });
    let test = builder.build(&server).await?;
    let call_id = "shell-empty-script-collab";
    let args = json!({
        "command": "",
        "timeout_ms": 1_000,
    });

    mount_sse_once(
        &server,
        sse(vec![
            ev_response_created("resp-empty-shell-1"),
            ev_function_call(call_id, "shell_command", &serde_json::to_string(&args)?),
            ev_completed("resp-empty-shell-1"),
        ]),
    )
    .await;
    let results_mock = mount_sse_once(
        &server,
        sse(vec![
            ev_assistant_message("msg-empty-shell-1", "done"),
            ev_completed("resp-empty-shell-2"),
        ]),
    )
    .await;

    let collaboration_mode = collaboration_mode_for_model(test.session_configured.model.clone());
    submit_user_turn(
        &test,
        "run an empty shell command",
        AskForApproval::OnRequest,
        PermissionProfile::Disabled,
        Some(collaboration_mode),
    )
    .await?;

    wait_for_event(&test.codex, |event| {
        matches!(event, EventMsg::TurnComplete(_))
    })
    .await;

    let output_item = results_mock.single_request().function_call_output(call_id);
    assert_no_matched_rules_invariant(&output_item);

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unified_exec_empty_script_with_collaboration_mode_does_not_panic() -> Result<()> {
    let server = start_mock_server().await;
    let mut builder = test_codex().with_model("gpt-5.2").with_config(|config| {
        config
            .features
            .enable(Feature::UnifiedExec)
            .expect("test config should allow feature update");
        config
            .features
            .enable(Feature::CollaborationModes)
            .expect("test config should allow feature update");
    });
    let test = builder.build(&server).await?;
    let call_id = "unified-exec-empty-script-collab";
    let args = json!({
        "cmd": "",
        "yield_time_ms": 1_000,
    });

    mount_sse_once(
        &server,
        sse(vec![
            ev_response_created("resp-empty-unified-1"),
            ev_function_call(call_id, "exec_command", &serde_json::to_string(&args)?),
            ev_completed("resp-empty-unified-1"),
        ]),
    )
    .await;
    let results_mock = mount_sse_once(
        &server,
        sse(vec![
            ev_assistant_message("msg-empty-unified-1", "done"),
            ev_completed("resp-empty-unified-2"),
        ]),
    )
    .await;

    let collaboration_mode = collaboration_mode_for_model(test.session_configured.model.clone());
    submit_user_turn(
        &test,
        "run empty unified exec command",
        AskForApproval::OnRequest,
        PermissionProfile::Disabled,
        Some(collaboration_mode),
    )
    .await?;

    wait_for_event(&test.codex, |event| {
        matches!(event, EventMsg::TurnComplete(_))
    })
    .await;

    let output_item = results_mock.single_request().function_call_output(call_id);
    assert_no_matched_rules_invariant(&output_item);

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shell_command_whitespace_script_with_collaboration_mode_does_not_panic() -> Result<()> {
    let server = start_mock_server().await;
    let mut builder = test_codex().with_model("gpt-5.2").with_config(|config| {
        config
            .features
            .enable(Feature::CollaborationModes)
            .expect("test config should allow feature update");
    });
    let test = builder.build(&server).await?;
    let call_id = "shell-whitespace-script-collab";
    let args = json!({
        "command": "  \n\t  ",
        "timeout_ms": 1_000,
    });

    mount_sse_once(
        &server,
        sse(vec![
            ev_response_created("resp-whitespace-shell-1"),
            ev_function_call(call_id, "shell_command", &serde_json::to_string(&args)?),
            ev_completed("resp-whitespace-shell-1"),
        ]),
    )
    .await;
    let results_mock = mount_sse_once(
        &server,
        sse(vec![
            ev_assistant_message("msg-whitespace-shell-1", "done"),
            ev_completed("resp-whitespace-shell-2"),
        ]),
    )
    .await;

    let collaboration_mode = collaboration_mode_for_model(test.session_configured.model.clone());
    submit_user_turn(
        &test,
        "run whitespace shell command",
        AskForApproval::OnRequest,
        PermissionProfile::Disabled,
        Some(collaboration_mode),
    )
    .await?;

    wait_for_event(&test.codex, |event| {
        matches!(event, EventMsg::TurnComplete(_))
    })
    .await;

    let output_item = results_mock.single_request().function_call_output(call_id);
    assert_no_matched_rules_invariant(&output_item);

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unified_exec_whitespace_script_with_collaboration_mode_does_not_panic() -> Result<()> {
    let server = start_mock_server().await;
    let mut builder = test_codex().with_model("gpt-5.2").with_config(|config| {
        config
            .features
            .enable(Feature::UnifiedExec)
            .expect("test config should allow feature update");
        config
            .features
            .enable(Feature::CollaborationModes)
            .expect("test config should allow feature update");
    });
    let test = builder.build(&server).await?;
    let call_id = "unified-exec-whitespace-script-collab";
    let args = json!({
        "cmd": " \n \t",
        "yield_time_ms": 1_000,
    });

    mount_sse_once(
        &server,
        sse(vec![
            ev_response_created("resp-whitespace-unified-1"),
            ev_function_call(call_id, "exec_command", &serde_json::to_string(&args)?),
            ev_completed("resp-whitespace-unified-1"),
        ]),
    )
    .await;
    let results_mock = mount_sse_once(
        &server,
        sse(vec![
            ev_assistant_message("msg-whitespace-unified-1", "done"),
            ev_completed("resp-whitespace-unified-2"),
        ]),
    )
    .await;

    let collaboration_mode = collaboration_mode_for_model(test.session_configured.model.clone());
    submit_user_turn(
        &test,
        "run whitespace unified exec command",
        AskForApproval::OnRequest,
        PermissionProfile::Disabled,
        Some(collaboration_mode),
    )
    .await?;

    wait_for_event(&test.codex, |event| {
        matches!(event, EventMsg::TurnComplete(_))
    })
    .await;

    let output_item = results_mock.single_request().function_call_output(call_id);
    assert_no_matched_rules_invariant(&output_item);

    Ok(())
}
