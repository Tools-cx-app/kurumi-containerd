use super::*;

#[test]
fn reverse_form_navigation_wraps_and_clears_confirmation() {
    let mut state = UiState::new(vec![]);
    state.open(ActionKind::Install);
    state.confirming = true;
    state.previous_field();
    assert_eq!(state.field, 2);
    assert!(!state.confirming);
    state.previous_field();
    assert_eq!(state.field, 1);
}

#[test]
fn reload_preserves_selected_name() {
    let mut state = UiState::new(vec!["one".into(), "two".into()]);
    state.selected = 1;
    state.reload(vec!["two".into(), "three".into()]);
    assert_eq!(state.selected_name(), Some("two"));
}

#[test]
fn cancelled_form_cannot_dispatch() {
    let mut state = UiState::new(vec!["one".into()]);
    state.open(ActionKind::Install);
    state.cancel();
    assert!(state.submit().is_err());
}

#[test]
fn required_inputs_are_checked_before_dispatch() {
    let mut state = UiState::new(vec!["one".into()]);
    state.open(ActionKind::Install);
    assert!(state.submit().is_err());
    state.open(ActionKind::Run);
    assert!(state.submit().is_err());
}

#[test]
fn destructive_actions_require_confirmation() {
    let mut state = UiState::new(vec!["one".into()]);
    for kind in [ActionKind::Stop, ActionKind::Restart, ActionKind::Scan] {
        state.open(kind);
        assert!(state.submit().unwrap().is_none());
        assert!(state.submit().unwrap().is_some());
    }
    state.open(ActionKind::Install);
    state.fields[0] = "rootfs.tar".into();
    state.force = true;
    assert!(state.submit().unwrap().is_none());
    assert!(state.submit().unwrap().is_some());
}

#[test]
fn tab_on_action_without_fields_keeps_selection_valid() {
    let mut state = UiState::new(vec!["one".into()]);
    state.open(ActionKind::Check);
    state.next_field();
    assert_eq!(state.field, 0);
    assert_eq!(state.submit().unwrap(), Some(Action::Check));
}

#[test]
fn install_force_field_toggles_only_after_inputs() {
    let mut state = UiState::new(vec!["one".into()]);
    state.open(ActionKind::Install);
    state.next_field();
    state.next_field();
    state.toggle_force();
    assert!(state.force);
    state.next_field();
    state.toggle_force();
    assert!(state.force);
}
