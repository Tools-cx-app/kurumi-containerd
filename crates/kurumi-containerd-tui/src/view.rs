use crate::{
    action::{ACTIONS, ActionKind, allowed},
    registry::Entry,
    state::{Focus, UiState},
};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
};

fn selection_style(selected: bool) -> Style {
    if selected {
        Style::default().add_modifier(Modifier::BOLD | Modifier::REVERSED)
    } else {
        Style::default()
    }
}

fn warning_color(warning: bool) -> Color {
    if warning { Color::Yellow } else { Color::Reset }
}

fn panel(title: &str, focused: bool) -> Block<'_> {
    Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().add_modifier(if focused {
            Modifier::BOLD
        } else {
            Modifier::DIM
        }))
}

fn unavailable(kind: ActionKind, entry: Option<&Entry>, terminal: bool) -> Option<String> {
    if kind == ActionKind::Check {
        None
    } else {
        entry.map_or_else(
            || Some("No container selected".into()),
            |entry| {
                allowed(kind, entry, terminal)
                    .err()
                    .map(|error| format!("{error:#}"))
            },
        )
    }
}

#[allow(clippy::too_many_lines)]
pub(super) fn draw(
    frame: &mut Frame<'_>,
    state: &UiState,
    entries: &[Entry],
    output: &str,
    busy: bool,
    terminal: bool,
) {
    let area = frame.area();
    if area.width < 36 || area.height < 10 {
        frame.render_widget(
            Paragraph::new("Resize terminal\nMinimum: 36 x 10\nq quit").wrap(Wrap { trim: false }),
            area,
        );
        return;
    }
    let narrow = area.width < 80;
    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Fill(3),
        Constraint::Length(4),
        Constraint::Fill(2),
        Constraint::Length(1),
    ])
    .split(area);
    let running = entries
        .iter()
        .filter(|entry| entry.info.as_ref().is_ok_and(|info| info.active))
        .count();
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                " KurumiContainerd ",
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::raw(format!(
                "{} containers | {running} running | {}",
                entries.len(),
                if busy { "Running..." } else { "Ready" }
            )),
        ])),
        rows[0],
    );
    let panes = Layout::default()
        .direction(if narrow {
            Direction::Vertical
        } else {
            Direction::Horizontal
        })
        .constraints(if narrow {
            [Constraint::Length(4), Constraint::Min(3)]
        } else {
            [Constraint::Percentage(32), Constraint::Percentage(68)]
        })
        .split(rows[1]);
    let items: Vec<_> = entries
        .iter()
        .map(|entry| {
            let (status, color) = match &entry.info {
                Ok(info) if info.active => ("running", Color::Green),
                Ok(_) => ("stopped", Color::Reset),
                Err(_) => ("error", Color::Red),
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!("{status:7} "), Style::default().fg(color)),
                Span::raw(&entry.pointer.name),
            ]))
        })
        .collect();
    if entries.is_empty() {
        frame.render_widget(
            Paragraph::new("No containers\nRegister with CLI; r reload")
                .block(panel("Containers", state.focus == Focus::Containers)),
            panes[0],
        );
    } else {
        frame.render_stateful_widget(
            List::new(items)
                .block(panel("Containers", state.focus == Focus::Containers))
                .highlight_symbol("> ")
                .highlight_style(selection_style(true)),
            panes[0],
            &mut ListState::default().with_selected(Some(state.selected)),
        );
    }
    let selected = entries.get(state.selected);
    let details = selected.map_or_else(
        || {
            "Select a container to view details.\nCheck host is available without a container."
                .into()
        },
        |entry| match &entry.info {
            Ok(info) => std::iter::once(format!("Management: {}", entry.pointer.name))
                .chain(std::iter::once(format!(
                    "Config: {}",
                    entry.pointer.file.display()
                )))
                .chain(info.display_lines())
                .collect::<Vec<_>>()
                .join("\n"),
            Err(error) => format!("Config: {}\nError: {error:#}", entry.pointer.file.display()),
        },
    );
    frame.render_widget(
        Paragraph::new(details)
            .block(panel("Details", state.focus == Focus::Details))
            .wrap(Wrap { trim: false })
            .scroll((state.details_scroll, 0)),
        panes[1],
    );

    // ponytail: a contiguous action window keeps selection visible without a second menu state.
    let current = state.action_index;
    let mut start = current;
    let mut used = ACTIONS[current].label().len() + 4;
    let width = usize::from(rows[2].width.saturating_sub(2));
    while start > 0 && used + ACTIONS[start - 1].label().len() + 4 <= width {
        start -= 1;
        used += ACTIONS[start].label().len() + 4;
    }
    let mut spans = Vec::new();
    used = 0;
    for (index, kind) in ACTIONS.iter().enumerate().skip(start) {
        let label = format!(
            " {}{} ",
            if index == current { "> " } else { "  " },
            kind.label()
        );
        if used + label.len() > width {
            break;
        }
        used += label.len();
        let style = if index == current {
            selection_style(true)
        } else if unavailable(*kind, selected, terminal).is_some() || busy {
            Style::default().add_modifier(Modifier::DIM)
        } else {
            Style::default()
        };
        spans.push(Span::styled(label, style));
    }
    let disabled = unavailable(ACTIONS[current], selected, terminal);
    let reason_style = if busy || disabled.is_some() {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    };
    let reason = if busy {
        "Operation in progress".into()
    } else {
        disabled.unwrap_or_else(|| "Enter to open | h/l actions".into())
    };
    frame.render_widget(
        Paragraph::new(vec![Line::from(spans), Line::styled(reason, reason_style)])
            .block(panel("Actions", false)),
        rows[2],
    );
    frame.render_widget(
        Paragraph::new(if output.is_empty() {
            "Command output appears here."
        } else {
            output
        })
        .block(panel(
            if busy {
                "Output | Running..."
            } else {
                "Output"
            },
            state.focus == Focus::Output,
        ))
        .wrap(Wrap { trim: false })
        .scroll((state.output_scroll, 0)),
        rows[3],
    );
    frame.render_widget(
        Paragraph::new(if narrow {
            "Tab focus | PgUp/Dn scroll | Enter | q"
        } else {
            "Tab focus | j/k containers | h/l actions | PgUp/PgDn scroll | r reload | q quit"
        })
        .style(Style::default().add_modifier(Modifier::DIM)),
        rows[4],
    );
    if let Some(kind) = state.open {
        draw_form(frame, state, kind, selected);
    }
}

fn draw_form(frame: &mut Frame<'_>, state: &UiState, kind: ActionKind, entry: Option<&Entry>) {
    let area = frame.area();
    let width = area.width.saturating_sub(4).min(76);
    let fields = state.fields.len() + usize::from(kind == ActionKind::Install);
    let height = u16::try_from(fields.saturating_add(8))
        .unwrap_or(u16::MAX)
        .min(area.height.saturating_sub(2));
    let modal = Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    );
    frame.render_widget(Clear, modal);
    let title = format!(
        " {}{} ",
        kind.label(),
        if state.confirming { " | Confirm" } else { "" }
    );
    let block = panel(&title, true);
    let inner = block.inner(modal);
    frame.render_widget(block, modal);
    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(2),
        Constraint::Length(1),
    ])
    .split(inner);
    frame.render_widget(
        Paragraph::new(format!(
            "Target: {}",
            entry.map_or("Host", |entry| entry.pointer.name.as_str())
        )),
        rows[0],
    );
    let mut lines: Vec<_> = state
        .fields
        .iter()
        .enumerate()
        .map(|(index, value)| {
            let label = kind
                .fields()
                .get(index)
                .map_or_else(|| format!("Argument {index}"), |label| (*label).into());
            let active = index == state.field;
            Line::styled(
                format!(
                    "{} {label}: {value}{}",
                    if active { ">" } else { " " },
                    if active { "_" } else { "" }
                ),
                selection_style(active),
            )
        })
        .collect();
    if kind == ActionKind::Install {
        lines.push(Line::styled(
            format!(
                "{} Force: {} (Space toggle)",
                if state.field == fields - 1 { ">" } else { " " },
                state.force
            ),
            selection_style(state.field == fields - 1).fg(warning_color(state.force)),
        ));
    }
    if lines.is_empty() {
        lines.push(Line::from(if state.confirming {
            "Confirm this operation on the target above."
        } else {
            "Press Enter to execute this operation."
        }));
    }
    let offset = state
        .field
        .saturating_sub(usize::from(rows[1].height.saturating_sub(1)));
    frame.render_widget(
        Paragraph::new(lines).scroll((u16::try_from(offset).unwrap_or(u16::MAX), 0)),
        rows[1],
    );
    frame.render_widget(
        Paragraph::new(&*state.form_error)
            .style(Style::default().fg(Color::Red))
            .wrap(Wrap { trim: false }),
        rows[2],
    );
    frame.render_widget(
        Paragraph::new(if state.confirming {
            "Enter confirm | Esc cancel"
        } else {
            "Tab/Shift+Tab fields | Enter | Esc"
        })
        .style(Style::default().fg(warning_color(state.confirming))),
        rows[3],
    );
}

#[cfg(test)]
mod tests {
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
}
