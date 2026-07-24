use super::GuardianExecApprovalRequirement::CurrentPolicy;
use super::GuardianExecApprovalRequirement::PreToolUseApproved;
use super::*;
use codex_protocol::protocol::GranularApprovalConfig;
use pretty_assertions::assert_eq;

struct Case {
    exact_pre_tool_use_approval: bool,
    policy: AskForApproval,
    profile: PermissionProfile,
    sandbox: SandboxPermissions,
    decision: Decision,
    explicit_rule: Option<Decision>,
    dangerous_command_match: Option<DangerousCommandMatch>,
}

fn approved_case() -> Case {
    Case {
        exact_pre_tool_use_approval: true,
        policy: AskForApproval::Never,
        profile: PermissionProfile::Disabled,
        sandbox: SandboxPermissions::UseDefault,
        decision: Decision::Forbidden,
        explicit_rule: None,
        dangerous_command_match: Some(DangerousCommandMatch::Other),
    }
}

fn classify(case: Case) -> GuardianExecApprovalRequirement {
    let mut matched_rules = vec![RuleMatch::HeuristicsRuleMatch {
        command: vec!["dangerous".to_string()],
        decision: case.decision,
    }];
    if let Some(decision) = case.explicit_rule {
        matched_rules.push(RuleMatch::PrefixRuleMatch {
            matched_prefix: vec!["dangerous".to_string()],
            decision,
            resolved_program: None,
            justification: None,
        });
    }
    let evaluation = Evaluation {
        decision: case.decision,
        matched_rules,
    };
    classify_guardian_exec_approval(
        GuardianExecApprovalContext {
            exact_pre_tool_use_approval: case.exact_pre_tool_use_approval,
            approval_policy: case.policy,
            permission_profile: &case.profile,
            sandbox_permissions: case.sandbox,
            evaluation: &evaluation,
            dangerous_command_match: case.dangerous_command_match,
        },
        current_requirement(),
    )
}

fn current_requirement() -> ExecApprovalRequirement {
    ExecApprovalRequirement::Forbidden {
        reason: "current policy".to_string(),
    }
}

fn assert_current_policy(case: Case) {
    assert_eq!(classify(case), CurrentPolicy(current_requirement()));
}

#[test]
fn exact_review_classifies_dangerous_heuristic_prompt_and_forbidden() {
    let mut prompt = approved_case();
    prompt.decision = Decision::Prompt;
    assert_eq!(classify(prompt), PreToolUseApproved);
    assert_eq!(classify(approved_case()), PreToolUseApproved);
}

#[test]
fn exact_review_preserves_hard_boundaries() {
    let mut absent = approved_case();
    absent.exact_pre_tool_use_approval = false;
    assert_current_policy(absent);
    for policy in [
        AskForApproval::OnRequest,
        AskForApproval::UnlessTrusted,
        AskForApproval::Granular(GranularApprovalConfig {
            sandbox_approval: true,
            rules: true,
            skill_approval: true,
            request_permissions: true,
            mcp_elicitations: true,
        }),
    ] {
        let mut case = approved_case();
        case.policy = policy;
        assert_current_policy(case);
    }
    let mut managed_profile = approved_case();
    managed_profile.profile = PermissionProfile::read_only();
    assert_current_policy(managed_profile);
    let mut sandbox_request = approved_case();
    sandbox_request.sandbox = SandboxPermissions::RequireEscalated;
    assert_current_policy(sandbox_request);
    let mut sandbox_prompt = approved_case();
    sandbox_prompt.decision = Decision::Prompt;
    sandbox_prompt.dangerous_command_match = None;
    assert_current_policy(sandbox_prompt);
    for decision in [Decision::Prompt, Decision::Forbidden] {
        let mut case = approved_case();
        case.explicit_rule = Some(decision);
        assert_current_policy(case);
    }
}
