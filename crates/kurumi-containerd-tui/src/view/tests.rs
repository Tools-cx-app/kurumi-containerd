use super::*;
use kurumi_containerd_config::ConfigPointer;
use kurumi_containerd_runtime::ContainerInfo;
use ratatui::{Terminal, backend::TestBackend};
use std::path::PathBuf;
use uuid::Uuid;

fn render(width: u16, height: u16, state: &UiState, entries: &[Entry]) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|frame| draw(frame, state, entries, "", false, false))
        .unwrap();
    terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(ratatui::buffer::Cell::symbol)
        .collect()
}

#[test]
fn responsive_empty_screen_keeps_selected_action_visible() {
    for (width, height) in [(100, 30), (60, 24), (80, 12)] {
        let mut state = UiState::new(vec![]);
        state.action_index = ACTIONS.len() - 1;
        let text = render(width, height, &state, &[]);
        assert!(text.contains("No containers"), "{width}x{height}: {text}");
        assert!(text.contains("Check host"));
        assert!(text.contains("Tab focus"));
    }
}

#[test]
fn tiny_screen_offers_resize_hint() {
    assert!(render(20, 5, &UiState::new(vec![]), &[]).contains("Resize terminal"));
}

#[test]
fn selected_action_and_form_error_are_visible() {
    let mut state = UiState::new(vec![]);
    state.action_index = ACTIONS.len() - 1;
    state.open(ActionKind::Install);
    state.form_error = "archive path is required".into();
    for (width, height) in [(100, 30), (60, 24), (80, 12)] {
        let text = render(width, height, &state, &[]);
        assert!(text.contains("archive path is required"));
        assert!(text.contains("Force: false"));
        assert!(text.contains("Target: Host"));
    }
}

#[test]
fn confirmation_identifies_target_container() {
    let entry = Entry {
        pointer: ConfigPointer {
            name: "production".into(),
            file: "container.toml".into(),
        },
        info: Err(anyhow::anyhow!("unavailable")),
        foreground: false,
        install_ready: true,
    };
    let mut state = UiState::new(vec!["production".into()]);
    state.open(ActionKind::Stop);
    state.confirming = true;
    let text = render(60, 24, &state, &[entry]);
    assert!(text.contains("Target: production"));
    assert!(text.contains("Enter confirm | Esc cancel"));
}

#[test]
fn narrow_details_wrap_long_container_info() {
    let entry = Entry {
        pointer: ConfigPointer {
            name: "small-screen".into(),
            file: PathBuf::from("/config/container.toml"),
        },
        info: Ok(ContainerInfo {
            name: "small-screen".into(),
            active: false,
            init_pid: None,
            monitor_pid: None,
            rootfs: PathBuf::from("/a/very/long/rootfs/path/that/must/wrap/on/small/screens"),
            uuid: Some(Uuid::nil()),
            init_system: None,
            generation: None,
            uptime_seconds: None,
            memory_kb: None,
            processes: None,
        }),
        foreground: false,
        install_ready: true,
    };
    let mut state = UiState::new(vec!["small-screen".into()]);
    state.details_scroll = 3;
    let text = render(60, 24, &state, &[entry]);
    assert!(text.contains("Rootfs:"));
}
