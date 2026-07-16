//! Codex++ TUI capabilities kept separate from upstream-owned orchestration.

mod account_policy;
#[cfg(any(not(debug_assertions), test))]
mod lag_warning;
mod model_capacity_retry;
mod release_status;
mod startup_accounts;
mod user_message_inbox;
mod weekly_window_scheduler;
mod welcome;

pub(crate) use account_policy::persist_settings;
#[cfg(not(debug_assertions))]
pub(crate) use lag_warning::lag_warning;
pub(crate) use model_capacity_retry::status_details as model_capacity_retry_status_details;
#[cfg(any(not(debug_assertions), test))]
pub(crate) use release_status::dismiss_version;
#[cfg(any(not(debug_assertions), test))]
pub(crate) use release_status::read_release_status;
#[cfg(not(debug_assertions))]
pub(crate) use release_status::refresh_release_status;
#[cfg(any(not(debug_assertions), test))]
pub(crate) use release_status::release_status_filepath;
pub(super) use startup_accounts::StartupAccountSelection;
pub(super) use startup_accounts::run_startup_account_picker;
pub(crate) use user_message_inbox::UserMessageInboxState;
pub(crate) use user_message_inbox::enabled as user_message_inbox_enabled;
pub(crate) use user_message_inbox::recognize as recognize_user_message;
pub(crate) use weekly_window_scheduler::WeeklyWindowScheduler;
pub(crate) use weekly_window_scheduler::WeeklyWindowStatus;
pub(crate) use welcome::WELCOME_TIP;
pub(crate) use welcome::replace_upstream_app_promo;
pub(crate) use welcome::welcome_help_lines;
