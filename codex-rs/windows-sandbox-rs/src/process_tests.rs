use super::batch_command_line;
use super::make_env_block;
use pretty_assertions::assert_eq;
use std::path::Path;
#[test]
fn empty_environment_and_batch_arguments_keep_windows_wire_semantics() {
    assert_eq!(make_env_block(&Default::default()), vec![0, 0]);
    let argv = ["probe", "%x%"].map(String::from);
    let command = batch_command_line(Path::new(r"C:\x.cmd"), &argv).unwrap();
    assert_eq!(
        command,
        r#"cmd.exe /e:ON /v:OFF /d /c ""C:\x.cmd" "%%cd:~,%%x%%cd:~,%%"""#
    );
}
