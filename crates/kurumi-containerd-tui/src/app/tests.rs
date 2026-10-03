use super::*;
use crate::state::UiState;

#[test]
fn screen_focus_scrolls_details_and_output_independently() {
    let mut state = UiState::new(Vec::new());
    let mut entries = Vec::new();
    let mut output = String::new();
    for key in [
        KeyCode::Tab,
        KeyCode::PageDown,
        KeyCode::Tab,
        KeyCode::PageDown,
        KeyCode::PageDown,
    ] {
        handle_screen(key, &mut state, &mut entries, &mut output, false);
    }
    assert_eq!(state.details_scroll, 5);
    assert_eq!(state.output_scroll, 10);
    handle_screen(
        KeyCode::BackTab,
        &mut state,
        &mut entries,
        &mut output,
        false,
    );
    handle_screen(
        KeyCode::PageUp,
        &mut state,
        &mut entries,
        &mut output,
        false,
    );
    assert_eq!(state.details_scroll, 0);
    assert_eq!(state.output_scroll, 10);
}

#[test]
fn empty_registry_blocks_container_actions_but_opens_check() {
    let mut state = UiState::new(Vec::new());
    let mut entries = Vec::new();
    let mut output = String::new();
    handle_screen(KeyCode::Enter, &mut state, &mut entries, &mut output, false);
    assert!(state.open.is_none());
    assert!(output.contains("No container selected"));
    state.action_index = ACTIONS.len() - 1;
    handle_screen(KeyCode::Enter, &mut state, &mut entries, &mut output, false);
    assert_eq!(state.open, Some(crate::action::ActionKind::Check));
    let mut child = None;
    let mut file = None;
    handle_form(
        KeyCode::Enter,
        &mut state,
        &mut output,
        &mut child,
        &mut file,
        Path::new("/bin/true"),
        &[],
        false,
    )
    .unwrap();
    assert!(child.as_mut().unwrap().wait().unwrap().success());
}

#[test]
fn failed_command_launch_is_reported_without_exiting_manager() {
    let mut state = UiState::new(Vec::new());
    state.open(crate::action::ActionKind::Check);
    let mut output = String::new();
    let mut child = None;
    let mut file = None;
    handle_form(
        KeyCode::Enter,
        &mut state,
        &mut output,
        &mut child,
        &mut file,
        Path::new("/nonexistent/kurumi-containerd"),
        &[],
        false,
    )
    .unwrap();
    assert!(output.contains("command launch failed"));
    assert!(child.is_none());
    assert!(state.open.is_none());
}

#[test]
fn form_preserves_spaces_and_recovers_from_missing_input() {
    let mut state = UiState::new(Vec::new());
    state.open(crate::action::ActionKind::Install);
    let mut output = String::new();
    let mut child = None;
    let mut file = None;
    let entry = crate::registry::Entry {
        pointer: kurumi_containerd_config::ConfigPointer {
            name: "test".into(),
            file: "container.toml".into(),
        },
        info: Err(anyhow::anyhow!("rootfs not installed")),
        foreground: false,
        install_ready: true,
    };

    for code in [KeyCode::Char(' '), KeyCode::Enter] {
        handle_form(
            code,
            &mut state,
            &mut output,
            &mut child,
            &mut file,
            Path::new("unused"),
            std::slice::from_ref(&entry),
            false,
        )
        .unwrap();
        assert_eq!(state.fields[0], " ");
    }
    assert!(state.form_error.contains("archive path is required"));
    assert!(state.open.is_some());
    assert!(child.is_none());
}
