use super::batch_command_line;
use pretty_assertions::assert_eq;
use std::path::Path;

#[test]
fn batch_arguments_keep_windows_wire_semantics() {
    let argv = ["probe", "%x%", r"C:\dir\"].map(String::from);
    assert_eq!(
        batch_command_line(Path::new(r"C:\%x%\probe.cmd"), &argv).unwrap(),
        r#"cmd.exe /e:ON /v:OFF /d /c ""C:\%%cd:~,%%x%%cd:~,%%\probe.cmd" "%%cd:~,%%x%%cd:~,%%" "C:\dir\"""#
    );
    assert!(batch_command_line(Path::new("x.cmd"), &["x".into(), "\"&calc".into()]).is_err());
}
