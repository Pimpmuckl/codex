//! Cloud-hosted configuration data for Codex.
//!
//! This crate owns transport, caching, and refresh behavior for cloud-delivered
//! config data. Parsing and composition remain in `codex-config`.

mod backend;
mod bundle_loader;
mod cache;
mod codex_plus_plus;
mod metrics;
mod service;
mod validation;

pub use bundle_loader::cloud_config_bundle_loader;
pub use bundle_loader::cloud_config_bundle_loader_for_storage;
pub use codex_plus_plus::cloud_config_bundle_loader_for_selected_account;
pub use codex_plus_plus::stop_cloud_config_refresh_before_account_picker;
