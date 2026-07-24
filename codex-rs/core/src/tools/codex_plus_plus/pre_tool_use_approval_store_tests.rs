use super::scope;
use super::take;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn exact_approval_is_one_shot_and_task_scoped() {
    let (approved, unapproved) = tokio::join!(
        scope(true, async { (take(), take()) }),
        scope(false, async { take() }),
    );

    assert_eq!((approved, unapproved), ((true, false), false));
    assert!(!take());
}
