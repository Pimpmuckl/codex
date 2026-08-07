//! Codex++ app-server protocol behavior kept separate from upstream-owned projection code.

mod inbox_legacy_deduper;

pub(crate) use inbox_legacy_deduper::InboxLegacyDeduper;
