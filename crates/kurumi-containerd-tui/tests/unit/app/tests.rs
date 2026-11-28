// Copyright (C) 2026 Tools-cx-app
// SPDX-License-Identifier: GPL-v3-only

use std::{fs, os::unix::fs::symlink};

use super::*;
use crate::state::UiState;

#[test]
fn screen_focus_scrolls_details_and_output_independently() {
    let mut state = UiState::new(Vec::new());
    let mut entries = Vec::new();
    let mut output = String::new();
    let backend = Backend::new(Path::new("/bin/true"));
    for key in [
        KeyCode::Tab,
        KeyCode::PageDown,
        KeyCode::Tab,
        KeyCode::PageDown,
        KeyCode::PageDown,
    ] {
        handle_screen(key, &mut state, &mut entries, &mut output, false, &backend);
    }
    assert_eq!(state.details_scroll, 5);
    assert_eq!(state.output_scroll, 10);
    handle_screen(
        KeyCode::BackTab,
        &mut state,
        &mut entries,
        &mut output,
        false,
        &backend,
    );
    handle_screen(
        KeyCode::PageUp,
        &mut state,
        &mut entries,
        &mut output,
        false,
        &backend,
    );
    assert_eq!(state.details_scroll, 0);
    assert_eq!(state.output_scroll, 10);
}

#[test]
fn empty_registry_blocks_container_actions_but_opens_check() {
    let mut state = UiState::new(Vec::new());
    let mut entries = Vec::new();
    let mut output = String::new();
    let backend = Backend::new(Path::new("/bin/true"));
    handle_screen(
        KeyCode::Enter,
        &mut state,
        &mut entries,
        &mut output,
        false,
        &backend,
    );
    assert!(state.open.is_none());
    assert!(output.contains("No container selected"));
    state.action_index = ACTIONS.len() - 1;
    handle_screen(
        KeyCode::Enter,
        &mut state,
        &mut entries,
        &mut output,
        false,
        &backend,
    );
    assert_eq!(state.open, Some(crate::action::ActionKind::Check));
    let mut child = None;
    handle_form(
        KeyCode::Enter,
        &mut state,
        &mut output,
        &mut child,
        &backend,
        &[],
        false,
    )
    .unwrap();
    let command = child.as_mut().unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(output) = command.poll().unwrap() {
            assert!(output.starts_with("Exit: exit status: 0\n"));
            break;
        }
        assert!(Instant::now() < deadline, "command did not finish");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn failed_command_launch_is_reported_without_exiting_manager() {
    let mut state = UiState::new(Vec::new());
    state.open(crate::action::ActionKind::Check);
    let mut output = String::new();
    let mut child = None;
    handle_form(
        KeyCode::Enter,
        &mut state,
        &mut output,
        &mut child,
        &Backend::new(Path::new("/nonexistent/kurumi-containerd")),
        &[],
        false,
    )
    .unwrap();
    assert!(output.contains("command launch failed"));
    assert!(child.is_none());
    assert!(state.open.is_none());
}

#[test]
fn first_container_can_be_inspected_and_imported_from_an_empty_screen() {
    use crate::action::ActionKind;

    let mut state = UiState::new(vec![]);
    let mut entries = vec![];
    let mut output = String::new();
    let backend = Backend::new(Path::new("/bin/echo"));
    for kind in [ActionKind::Inspect, ActionKind::Import] {
        state.action_index = ACTIONS.iter().position(|action| *action == kind).unwrap();
        handle_screen(
            KeyCode::Enter,
            &mut state,
            &mut entries,
            &mut output,
            false,
            &backend,
        );
        assert_eq!(state.open, Some(kind));
        state.fields[0] = "portable.kurumi".into();
        let mut task = None;
        for _ in 0..=usize::from(kind == ActionKind::Import) {
            handle_form(
                KeyCode::Enter,
                &mut state,
                &mut output,
                &mut task,
                &backend,
                &entries,
                false,
            )
            .unwrap();
        }
        let mut task = task.unwrap();
        assert_eq!(task.reload_registry, kind == ActionKind::Import);
        let deadline = Instant::now() + Duration::from_secs(5);
        while task.poll().unwrap().is_none() {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

#[test]
fn form_preserves_spaces_and_recovers_from_missing_input() {
    let mut state = UiState::new(Vec::new());
    state.open(crate::action::ActionKind::Install);
    let mut output = String::new();
    let mut child = None;
    let entry = crate::registry::Entry {
        config: kurumi_containerd_config::ConfigEntry {
            name: "test".into(),
            path: "config.json".into(),
        },
        info: Err(kurumi_containerd_error::RuntimeError::Message(
            "rootfs not installed".to_owned(),
        )),
        foreground: false,
        install_ready: true,
    };

    for code in [KeyCode::Char(' '), KeyCode::Enter] {
        handle_form(
            code,
            &mut state,
            &mut output,
            &mut child,
            &Backend::new(Path::new("unused")),
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

fn finish_service_command(page: &mut Services, backend: &Backend<'_>) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while page.busy() {
        assert!(Instant::now() < deadline);
        page.poll(backend);
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn captured_commands_target_selected_guest_and_refresh_after_confirmed_change() {
    let temp = tempfile::tempdir().unwrap();
    let cli = temp.path().join("cli");
    let fixture = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/services.sh");
    for tool in ["cli", "rc-service", "rc-status", "rc-update"] {
        symlink(fixture, temp.path().join(tool)).unwrap();
    }
    let backend = Backend::new(&cli);
    let mut page = Services::open("alpine", &backend);
    finish_service_command(&mut page, &backend);
    assert_eq!(page.rows.len(), 1, "{}", page.output);
    assert_eq!(page.rows[0].startup, "—");
    handle_services(KeyCode::Char('e'), &mut page, &backend);
    assert!(!page.busy());
    assert!(!temp.path().join("enabled").exists());
    handle_services(KeyCode::Esc, &mut page, &backend);
    assert!(page.confirm.is_none());
    handle_services(KeyCode::Char('e'), &mut page, &backend);
    let mut terminal = Terminal::new(ratatui::backend::TestBackend::new(60, 24)).unwrap();
    terminal.draw(|frame| draw_services(frame, &page)).unwrap();
    let screen = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(ratatui::buffer::Cell::symbol)
        .collect::<String>();
    assert!(screen.contains("Confirm enable sshd in alpine?"));
    handle_services(KeyCode::Enter, &mut page, &backend);
    finish_service_command(&mut page, &backend);
    assert_eq!(page.rows[0].startup, "default");
    assert!(page.output.contains("enable sshd (exit status: 0)"));
    fs::remove_file(temp.path().join("rc-update")).unwrap();
    handle_services(KeyCode::Char('r'), &mut page, &backend);
    finish_service_command(&mut page, &backend);
    assert!(page.rows.is_empty());
    assert!(page.output.contains("No systemd or OpenRC"));
}
