use crate::{
    action::{ACTIONS, ActionKind, allowed},
    registry::Entry,
    state::{Focus, UiState},
};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, BorderType, Borders, Clear, List, ListItem, ListState, Padding, Paragraph, Wrap,
    },
};

const ACCENT: Color = Color::Rgb(203, 166, 247);
const BORDER: Color = Color::Rgb(88, 91, 112);
const SURFACE: Color = Color::Rgb(49, 50, 68);
const TEXT: Color = Color::Rgb(205, 214, 244);
const CRUST: Color = Color::Rgb(17, 17, 27);
const GREEN: Color = Color::Rgb(166, 227, 161);
const YELLOW: Color = Color::Rgb(249, 226, 175);
const RED: Color = Color::Rgb(243, 139, 168);

fn selection_style(selected: bool) -> Style {
    if selected {
        Style::default().fg(TEXT).bg(SURFACE).bold()
    } else {
        Style::default()
    }
}

fn warning_color(warning: bool) -> Color {
    if warning { YELLOW } else { Color::Reset }
}

fn panel(title: &str, focused: bool) -> Block<'_> {
    Block::default()
        .title(Span::styled(
            format!(" {title} "),
            if focused {
                Style::default().fg(CRUST).bg(ACCENT).bold()
            } else {
                Style::default().fg(ACCENT).bold()
            },
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .padding(Padding::horizontal(1))
        .border_style(Style::default().fg(if focused { ACCENT } else { BORDER }))
}

fn badge(text: String, color: Color) -> Span<'static> {
    Span::styled(text, Style::default().fg(CRUST).bg(color).bold())
}

fn hint(text: &str) -> Line<'_> {
    let mut spans = Vec::new();
    for (index, part) in text.split(" | ").enumerate() {
        if index > 0 {
            spans.push(Span::styled(
                " | ",
                Style::default().add_modifier(Modifier::DIM),
            ));
        }
        let (key, description) = part.split_once(' ').unwrap_or((part, ""));
        spans.push(Span::styled(
            key,
            Style::default().fg(TEXT).bg(SURFACE).bold(),
        ));
        if !description.is_empty() {
            spans.push(Span::raw(format!(" {description}")));
        }
    }
    Line::from(spans)
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
    let spacious = area.height >= 24;
    let header_height = if spacious { 6 } else { 1 };
    let rows = Layout::vertical([
        Constraint::Length(header_height),
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
    let header = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).split(rows[0]);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" KurumiContainerd ", Style::default().fg(ACCENT).bold()),
            Span::raw(format!("{} containers | ", entries.len())),
            Span::styled(format!("{running} running"), Style::default().fg(GREEN)),
            Span::raw(" | "),
            badge(
                format!(" {} ", if busy { "Running..." } else { "Ready" }),
                if busy { YELLOW } else { GREEN },
            ),
        ])),
        header[0],
    );
    if spacious {
        let errors = entries.iter().filter(|entry| entry.info.is_err()).count();
        let cards = Layout::horizontal([Constraint::Fill(1); 4]).split(header[1]);
        for (index, (title, value, color)) in [
            ("Containers", entries.len().to_string(), ACCENT),
            ("Running", running.to_string(), GREEN),
            ("Errors", errors.to_string(), RED),
            (
                "Operation",
                if busy { "Busy" } else { "Ready" }.into(),
                if busy { YELLOW } else { GREEN },
            ),
        ]
        .into_iter()
        .enumerate()
        {
            frame.render_widget(
                Paragraph::new(vec![
                    Line::default(),
                    Line::styled(value, Style::default().fg(color).bold()),
                ])
                .alignment(Alignment::Center)
                .block(panel(title, false)),
                cards[index],
            );
        }
    }
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
                Ok(info) if info.active => ("running", GREEN),
                Ok(_) => ("stopped", Color::Reset),
                Err(_) => ("error", RED),
            };
            if spacious && !narrow {
                ListItem::new(vec![
                    Line::styled(&entry.pointer.name, Style::default().bold()),
                    Line::from(vec![
                        Span::styled(format!("● {status}"), Style::default().fg(color)),
                        Span::raw("  "),
                        Span::styled(
                            if entry.foreground {
                                "foreground"
                            } else {
                                "background"
                            },
                            Style::default().add_modifier(Modifier::DIM),
                        ),
                    ]),
                    Line::default(),
                ])
            } else {
                ListItem::new(Line::from(vec![
                    Span::styled(format!("{status:7} "), Style::default().fg(color).bold()),
                    Span::raw(&entry.pointer.name),
                ]))
            }
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
                .highlight_symbol("▍ ")
                .highlight_style(selection_style(true)),
            panes[0],
            &mut ListState::default().with_selected(Some(state.selected)),
        );
    }
    let selected = entries.get(state.selected);
    let details: String = selected.map_or_else(
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
    let details: Vec<_> = details
        .lines()
        .map(|line| {
            line.split_once(": ").map_or_else(
                || Line::raw(line.to_owned()),
                |(label, value)| {
                    Line::from(vec![
                        Span::styled(
                            format!("{:<13}", format!("{label}:")),
                            Style::default().add_modifier(Modifier::DIM),
                        ),
                        Span::styled(
                            value.to_owned(),
                            if label == "Error" {
                                Style::default().fg(RED)
                            } else {
                                Style::default()
                            },
                        ),
                    ])
                },
            )
        })
        .collect();
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
    let width = usize::from(rows[2].width.saturating_sub(4));
    while start > 0 && used + ACTIONS[start - 1].label().len() + 4 <= width {
        start -= 1;
        used += ACTIONS[start].label().len() + 4;
    }
    let mut spans = Vec::new();
    used = 0;
    for (index, kind) in ACTIONS.iter().enumerate().skip(start) {
        let label = format!(
            "{}{}{}",
            if index == current { "[ " } else { "  " },
            kind.label(),
            if index == current { " ]" } else { "  " },
        );
        if used + label.len() > width {
            break;
        }
        used += label.len();
        let disabled = unavailable(*kind, selected, terminal).is_some() || busy;
        let style = if index == current {
            Style::default()
                .fg(TEXT)
                .bg(SURFACE)
                .fg(CRUST)
                .bg(if disabled { YELLOW } else { ACCENT })
                .bold()
        } else if disabled {
            Style::default().add_modifier(Modifier::DIM)
        } else {
            Style::default()
        };
        spans.push(Span::styled(label, style));
    }
    let disabled = unavailable(ACTIONS[current], selected, terminal);
    let reason_style = if busy || disabled.is_some() {
        Style::default().fg(YELLOW)
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
            .block(panel("Actions · h/l select · Enter open", false)),
        rows[2],
    );
    frame.render_widget(
        Paragraph::new(if output.is_empty() {
            "Command output appears here."
        } else {
            output
        })
        .style(if output.is_empty() {
            Style::default().add_modifier(Modifier::DIM)
        } else {
            Style::default()
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
        Paragraph::new(hint(if narrow {
            "Tab focus | PgUp/Dn scroll | Enter | q"
        } else {
            "Tab focus | j/k containers | h/l actions | PgUp/PgDn scroll | r reload | q quit"
        })),
        rows[4],
    );
    if let Some(kind) = state.open {
        draw_form(frame, state, kind, selected);
    }
}

fn input_lines(label: &str, value: &str, active: bool, expanded: bool) -> Vec<Line<'static>> {
    if expanded {
        vec![
            Line::styled(
                format!("{label}:"),
                Style::default()
                    .fg(if active { ACCENT } else { Color::Reset })
                    .bold(),
            ),
            Line::styled(
                format!(
                    " {} {value}{}",
                    if active { "▍" } else { " " },
                    if active { "_" } else { "" }
                ),
                Style::default().fg(TEXT).bg(SURFACE),
            ),
            Line::default(),
        ]
    } else {
        vec![Line::styled(
            format!(
                "{} {label}: {value}{}",
                if active { ">" } else { " " },
                if active { "_" } else { "" }
            ),
            selection_style(active),
        )]
    }
}

fn draw_form(frame: &mut Frame<'_>, state: &UiState, kind: ActionKind, entry: Option<&Entry>) {
    let area = frame.area();
    let width = area.width.saturating_sub(4).min(76);
    let fields = state.fields.len() + usize::from(kind == ActionKind::Install);
    let field_height = if area.height >= 20 { 3 } else { 1 };
    let height = u16::try_from(fields.saturating_mul(field_height).saturating_add(8))
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
        "{}{}",
        kind.label(),
        if state.confirming { " | Confirm" } else { "" }
    );
    let block = panel(&title, true).border_style(Style::default().fg(if state.confirming {
        YELLOW
    } else {
        ACCENT
    }));
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
        ))
        .style(Style::default().fg(ACCENT).bold()),
        rows[0],
    );
    let mut lines: Vec<_> = state
        .fields
        .iter()
        .enumerate()
        .flat_map(|(index, value)| {
            let label = kind
                .fields()
                .get(index)
                .map_or_else(|| format!("Argument {index}"), |label| (*label).into());
            input_lines(&label, value, index == state.field, field_height == 3)
        })
        .collect();
    if kind == ActionKind::Install {
        lines.push(Line::styled(
            format!(
                "{} Force: {} (Space toggle)",
                if state.field == fields - 1 { ">" } else { " " },
                state.force
            ),
            selection_style(state.field == fields - 1).fg(if state.force {
                YELLOW
            } else if state.field == fields - 1 {
                TEXT
            } else {
                Color::Reset
            }),
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
        .saturating_mul(field_height)
        .saturating_sub(usize::from(rows[1].height.saturating_sub(1)));
    frame.render_widget(
        Paragraph::new(lines).scroll((u16::try_from(offset).unwrap_or(u16::MAX), 0)),
        rows[1],
    );
    frame.render_widget(
        Paragraph::new(&*state.form_error)
            .style(Style::default().fg(RED))
            .wrap(Wrap { trim: false }),
        rows[2],
    );
    frame.render_widget(
        Paragraph::new(hint(if state.confirming {
            "Enter confirm | Esc cancel"
        } else {
            "Tab/Shift+Tab fields | Enter | Esc"
        }))
        .style(Style::default().fg(warning_color(state.confirming))),
        rows[3],
    );
}

#[cfg(test)]
mod tests;
