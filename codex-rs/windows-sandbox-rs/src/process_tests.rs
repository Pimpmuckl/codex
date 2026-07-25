use super::make_env_block;
use pretty_assertions::assert_eq;
#[test]
fn empty_environment_keeps_windows_wire_semantics() {
    assert_eq!(make_env_block(&Default::default()), vec![0, 0]);
}
