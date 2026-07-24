use crate::sandboxing::SandboxPermissions;
use crate::tools::sandboxing::ExecApprovalRequirement;
use codex_execpolicy::Decision;
use codex_execpolicy::Evaluation;
use codex_execpolicy::RuleMatch;
use codex_protocol::models::PermissionProfile;
use codex_protocol::protocol::AskForApproval;
use codex_shell_command::is_dangerous_command::DangerousCommandMatch;

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum GuardianExecApprovalRequirement {
    CurrentPolicy(ExecApprovalRequirement),
    PreToolUseApproved,
}

pub(crate) struct GuardianExecApprovalContext<'a> {
    pub(crate) exact_pre_tool_use_approval: bool,
    pub(crate) approval_policy: AskForApproval,
    pub(crate) permission_profile: &'a PermissionProfile,
    pub(crate) sandbox_permissions: SandboxPermissions,
    pub(crate) evaluation: &'a Evaluation,
    pub(crate) dangerous_command_match: Option<DangerousCommandMatch>,
}

pub(crate) fn classify_guardian_exec_approval(
    context: GuardianExecApprovalContext<'_>,
    current_requirement: ExecApprovalRequirement,
) -> GuardianExecApprovalRequirement {
    let has_explicit_blocking_rule = context.evaluation.matched_rules.iter().any(|rule_match| {
        matches!(rule_match, RuleMatch::PrefixRuleMatch { .. })
            && rule_match.decision() != Decision::Allow
    });
    let approved_dangerous_heuristic = context.exact_pre_tool_use_approval
        && context.approval_policy == AskForApproval::Never
        && matches!(context.permission_profile, PermissionProfile::Disabled)
        && context.sandbox_permissions == SandboxPermissions::UseDefault
        && matches!(
            context.evaluation.decision,
            Decision::Prompt | Decision::Forbidden
        )
        && context.dangerous_command_match.is_some()
        && !has_explicit_blocking_rule;

    if approved_dangerous_heuristic {
        GuardianExecApprovalRequirement::PreToolUseApproved
    } else {
        GuardianExecApprovalRequirement::CurrentPolicy(current_requirement)
    }
}

#[cfg(test)]
#[path = "guardian_exec_approval_tests.rs"]
mod tests;
