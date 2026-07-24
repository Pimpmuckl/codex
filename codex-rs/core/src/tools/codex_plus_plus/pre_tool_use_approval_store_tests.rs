use super::scope;
use super::take;
use pretty_assertions::assert_eq;
use tokio::task::yield_now;

#[tokio::test]
async fn exact_approval_is_one_shot_and_task_scoped() {
    let (approved, unapproved) = tokio::join!(
        scope(/*approved*/ true, async {
            yield_now().await;
            (take(), take())
        }),
        scope(/*approved*/ false, async {
            yield_now().await;
            take()
        }),
    );

    assert_eq!((approved, unapproved), ((true, false), false));
    assert!(!take());
}
