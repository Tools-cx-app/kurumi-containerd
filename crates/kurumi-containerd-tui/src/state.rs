use anyhow::{Context, Result};

use crate::action::{Action, ActionKind};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Focus {
    Containers,
    Details,
    Output,
}

pub(super) struct UiState {
    names: Vec<String>,
    pub(super) selected: usize,
    pub(super) action_index: usize,
    pub(super) open: Option<ActionKind>,
    pub(super) fields: Vec<String>,
    pub(super) field: usize,
    pub(super) force: bool,
    pub(super) confirming: bool,
    pub(super) focus: Focus,
    pub(super) details_scroll: u16,
    pub(super) output_scroll: u16,
    pub(super) form_error: String,
}

impl UiState {
    pub(super) fn new(names: Vec<String>) -> Self {
        Self {
            names,
            selected: 0,
            action_index: 0,
            open: None,
            fields: Vec::new(),
            field: 0,
            force: false,
            confirming: false,
            focus: Focus::Containers,
            details_scroll: 0,
            output_scroll: 0,
            form_error: String::new(),
        }
    }
    fn selected_name(&self) -> Option<&str> {
        self.names.get(self.selected).map(String::as_str)
    }
    pub(super) fn reload(&mut self, names: Vec<String>) {
        let old = self.selected_name().map(str::to_owned);
        self.names = names;
        self.selected = old
            .and_then(|name| self.names.iter().position(|item| *item == name))
            .unwrap_or(0);
        self.details_scroll = 0;
    }
    pub(super) fn open(&mut self, kind: ActionKind) {
        self.form_error.clear();
        self.open = Some(kind);
        self.fields = vec![String::new(); kind.fields().len()];
        self.field = 0;
        self.force = false;
        self.confirming = false;
    }
    pub(super) fn cancel(&mut self) {
        self.open = None;
        self.confirming = false;
    }
    pub(super) fn next_field(&mut self) {
        if self.open == Some(ActionKind::Run) && self.field + 1 == self.fields.len() {
            self.fields.push(String::new());
        }
        let count = self.fields.len() + usize::from(self.open == Some(ActionKind::Install));
        self.field = if count == 0 {
            0
        } else {
            (self.field + 1) % count
        };
        self.confirming = false;
    }
    pub(super) fn previous_field(&mut self) {
        let count = self.fields.len() + usize::from(self.open == Some(ActionKind::Install));
        self.field = if count == 0 {
            0
        } else {
            (self.field + count - 1) % count
        };
        self.confirming = false;
    }
    pub(super) fn cycle_focus(&mut self, backwards: bool) {
        self.focus = match (self.focus, backwards) {
            (Focus::Containers, false) | (Focus::Output, true) => Focus::Details,
            (Focus::Details, false) | (Focus::Containers, true) => Focus::Output,
            _ => Focus::Containers,
        };
    }
    pub(super) fn scroll(&mut self, down: bool) {
        let offset = match self.focus {
            Focus::Details => &mut self.details_scroll,
            Focus::Output => &mut self.output_scroll,
            Focus::Containers => return,
        };
        *offset = if down {
            offset.saturating_add(5)
        } else {
            offset.saturating_sub(5)
        };
    }
    pub(super) fn toggle_force(&mut self) {
        if self.open == Some(ActionKind::Install) && self.field == self.fields.len() {
            self.force = !self.force;
            self.confirming = false;
            self.form_error.clear();
        }
    }
    pub(super) fn submit(&mut self) -> Result<Option<Action>> {
        let kind = self.open.context("no action selected")?;
        let action = kind.action(&self.fields, self.force)?;
        if (kind.confirm() || kind == ActionKind::Install && self.force) && !self.confirming {
            self.confirming = true;
            return Ok(None);
        }
        self.cancel();
        Ok(Some(action))
    }
}

#[cfg(test)]
mod tests {
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
}
