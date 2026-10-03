use super::*;

#[test]
fn install_uses_selected_management_name_and_exact_arguments() {
    let action = Action::Install {
        archive: "rootfs.tar".into(),
        size: Some("8G".into()),
        force: true,
    };
    assert_eq!(
        action.args("debian"),
        vec![
            "--name",
            "debian",
            "install",
            "rootfs.tar",
            "--size",
            "8G",
            "--force"
        ]
        .into_iter()
        .map(OsString::from)
        .collect::<Vec<_>>()
    );
}

#[test]
fn host_check_does_not_require_a_container() {
    assert_eq!(Action::Check.args("debian"), [OsString::from("check")]);
}

#[test]
fn configured_foreground_start_uses_separate_terminal() {
    assert!(Action::Start(false).needs_terminal(true));
    assert!(!Action::Start(false).needs_terminal(false));
}
