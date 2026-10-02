use crate::{
    action::{ACTIONS, allowed},
    registry::Entry,
    state::UiState,
};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
};
#[allow(clippy::many_single_char_names, clippy::too_many_lines)]
pub(super) fn draw(
    f: &mut Frame<'_>,
    s: &UiState,
    e: &[Entry],
    out: &str,
    scroll: u16,
    busy: bool,
    term: bool,
) {
    let narrow = f.area().width < 80;
    let r = Layout::default()
        .direction(Direction::Vertical)
        .constraints(if narrow {
            [
                Constraint::Length(3),
                Constraint::Percentage(60),
                Constraint::Length(5),
                Constraint::Min(4),
            ]
        } else {
            [
                Constraint::Length(3),
                Constraint::Percentage(45),
                Constraint::Length(6),
                Constraint::Min(4),
            ]
        })
        .split(f.area());
    f.render_widget(Paragraph::new("KurumiContainerd | j/k container | h/l action | Enter | r reload | PgUp/PgDn output | q quit").block(Block::default().borders(Borders::ALL)),r[0]);
    let c = Layout::default()
        .direction(if narrow {
            Direction::Vertical
        } else {
            Direction::Horizontal
        })
        .constraints(if narrow {
            [Constraint::Percentage(25), Constraint::Percentage(75)]
        } else {
            [Constraint::Percentage(42), Constraint::Percentage(58)]
        })
        .split(r[1]);
    let items: Vec<_> = e
        .iter()
        .map(|x| {
            ListItem::new(format!(
                "{}  {}",
                x.pointer.name,
                match &x.info {
                    Ok(i) if i.active => "running",
                    Ok(_) => "stopped",
                    Err(_) => "error",
                }
            ))
        })
        .collect();
    let mut ls = ListState::default().with_selected(Some(s.selected));
    f.render_stateful_widget(
        List::new(items)
            .block(Block::default().title("Containers").borders(Borders::ALL))
            .highlight_style(
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
        c[0],
        &mut ls,
    );
    if let Some(x) = e.get(s.selected) {
        let t = match &x.info {
            Ok(i) => std::iter::once(format!("Management: {}", x.pointer.name))
                .chain(std::iter::once(format!(
                    "Config: {}",
                    x.pointer.file.display()
                )))
                .chain(i.display_lines())
                .collect::<Vec<_>>()
                .join("\n"),
            Err(er) => format!("Config: {}\nError: {er:#}", x.pointer.file.display()),
        };
        f.render_widget(
            Paragraph::new(t)
                .block(Block::default().title("Details").borders(Borders::ALL))
                .wrap(Wrap { trim: false })
                .scroll((scroll, 0)),
            c[1],
        );
    }
    let a: Vec<_> = ACTIONS
        .iter()
        .enumerate()
        .map(|(i, k)| {
            Line::styled(
                format!(
                    "{} {}{}",
                    if i == s.action_index { ">" } else { " " },
                    k.label(),
                    e.get(s.selected)
                        .and_then(|x| allowed(*k, x, term).err())
                        .map_or(String::new(), |x| format!(" ({x})"))
                ),
                if i == s.action_index {
                    Style::default().fg(Color::Yellow)
                } else {
                    Style::default()
                },
            )
        })
        .collect();
    f.render_widget(
        Paragraph::new(a)
            .scroll((
                u16::try_from(
                    s.action_index
                        .saturating_sub(usize::from(r[2].height.saturating_sub(3))),
                )
                .unwrap_or(u16::MAX),
                0,
            ))
            .block(
                Block::default()
                    .title("Actions (h/l; Enter)")
                    .borders(Borders::ALL),
            ),
        r[2],
    );
    let t = if let Some(k) = s.open {
        format!(
            "{} | {}\n{}\n{}\n{}",
            k.label(),
            out,
            s.fields
                .iter()
                .enumerate()
                .map(|(i, v)| format!(
                    "{}: {}{}",
                    k.fields().get(i).copied().unwrap_or("Argument"),
                    v,
                    if i == s.field { "_" } else { "" }
                ))
                .collect::<Vec<_>>()
                .join("\n"),
            if k == crate::action::ActionKind::Install {
                format!(
                    "Force: {}{}",
                    s.force,
                    if s.field == s.fields.len() { "_" } else { "" }
                )
            } else {
                String::new()
            },
            if s.confirming {
                "Enter again to confirm / Esc cancel"
            } else {
                "Tab fields / Enter submit / Esc cancel"
            }
        )
    } else {
        format!("{}{}", if busy { "Running...\n" } else { "" }, out)
    };
    f.render_widget(
        Paragraph::new(t)
            .block(
                Block::default()
                    .title("Output / Input")
                    .borders(Borders::ALL),
            )
            .wrap(Wrap { trim: false })
            .scroll((
                if s.open.is_some() {
                    u16::try_from(
                        s.field
                            .saturating_sub(usize::from(r[3].height.saturating_sub(5))),
                    )
                    .unwrap_or(u16::MAX)
                } else {
                    scroll
                },
                0,
            )),
        r[3],
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::Entry;
    use kurumi_containerd_config::ConfigPointer;
    use kurumi_containerd_runtime::ContainerInfo;
    use ratatui::{Terminal, backend::TestBackend};
    use std::path::PathBuf;
    use uuid::Uuid;

    #[test]
    fn selected_action_and_form_error_are_visible() {
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        let mut state = UiState::new(vec![]);
        state.action_index = ACTIONS.len() - 1;
        state.open(crate::action::ActionKind::Install);
        terminal
            .draw(|frame| {
                draw(
                    frame,
                    &state,
                    &[],
                    "archive path is required",
                    40,
                    false,
                    false,
                );
            })
            .unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect();
        assert!(text.contains("Check host"));
        assert!(text.contains("archive path is required"));
        assert!(text.contains("Force: false"));
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
        let mut terminal = Terminal::new(TestBackend::new(60, 24)).unwrap();
        let state = UiState::new(vec!["small-screen".into()]);
        terminal
            .draw(|frame| draw(frame, &state, &[entry], "", 0, false, false))
            .unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect();
        assert!(text.contains("Rootfs:"));
        assert!(text.contains("UUID:"));
    }
}
