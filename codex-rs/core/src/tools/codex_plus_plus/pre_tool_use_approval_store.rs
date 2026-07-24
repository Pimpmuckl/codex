use std::cell::Cell;
use std::future::Future;

tokio::task_local! {
    static EXACT_APPROVAL: Cell<bool>;
}

pub(crate) async fn scope<T>(approved: bool, future: impl Future<Output = T>) -> T {
    EXACT_APPROVAL.scope(Cell::new(approved), future).await
}

pub(crate) fn take() -> bool {
    EXACT_APPROVAL
        .try_with(|approved| approved.replace(false))
        .unwrap_or(false)
}

#[cfg(test)]
#[path = "pre_tool_use_approval_store_tests.rs"]
mod tests;
