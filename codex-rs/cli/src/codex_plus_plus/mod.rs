#![cfg_attr(debug_assertions, allow(dead_code))]

mod upstream_switch;

#[cfg(not(debug_assertions))]
pub(crate) use upstream_switch::run as run_upstream_switch;
