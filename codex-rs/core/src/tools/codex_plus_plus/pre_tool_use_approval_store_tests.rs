use super::ExactPreToolUseApproval;
use super::scope;
use super::take;
use pretty_assertions::assert_eq;
use tokio::task::yield_now;

#[tokio::test]
async fn exact_approval_is_one_shot_and_task_scoped() {
    let approval = ExactPreToolUseApproval {
        execution_target: None,
    };
    let (approved, unapproved) = tokio::join!(
        scope(Some(approval.clone()), async {
            yield_now().await;
            (take(), take())
        }),
        scope(/*approval*/ None, async {
            yield_now().await;
            take()
        }),
    );

    assert_eq!((approved, unapproved), ((Some(approval), None), None));
    assert_eq!(take(), None);
}
