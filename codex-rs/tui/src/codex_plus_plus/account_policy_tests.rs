use super::*;
use pretty_assertions::assert_eq;

#[test]
fn failed_verification_applies_opt_outs_without_enabling_triggers() {
    let both = AutoRedeemResets::default();
    let expiry_only = AutoRedeemResets {
        weekly_exhausted_min_wait_hours: None,
        ..both
    };
    let exhaustion_only = AutoRedeemResets {
        before_expiry_minutes: None,
        ..both
    };
    for (current, requested, expected) in [
        (Some(both), Some(expiry_only), Some(expiry_only)),
        (Some(both), Some(exhaustion_only), Some(exhaustion_only)),
        (Some(expiry_only), Some(both), Some(expiry_only)),
        (None, Some(both), None),
        (Some(both), None, None),
    ] {
        assert_eq!(retained_auto_redeem_settings(current, requested), expected);
    }
}
