// Copyright (C) 2026 Tools-cx-app
// SPDX-License-Identifier: GPL-v3-only

mod actions;
mod details;
mod form;
mod services;

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, List, ListItem, ListState, Padding, Paragraph, Wrap},
};

use crate::registry::Entry;
use crate::state::{Focus, UiState};

pub(super) use services::draw_services;

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
    let [header, body, actions, output_area, footer] = Layout::vertical([
        Constraint::Length(header_height),
        Constraint::Fill(3),
        Constraint::Length(4),
        Constraint::Fill(2),
        Constraint::Length(1),
    ])
    .areas(area);
    let [containers, details] = Layout::default()
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
        .areas(body);
    let selected = entries.get(state.selected);
    draw_header(frame, header, entries, busy, spacious);
    draw_containers(frame, containers, state, entries, spacious && !narrow);
    details::draw(frame, details, state, selected);
    actions::draw(frame, actions, state.action_index, selected, busy, terminal);
    draw_output(frame, output_area, state, output, busy);
    frame.render_widget(
        Paragraph::new(hint(if narrow {
            "s services | Tab focus | Enter | q"
        } else {
            "s services | Tab focus | j/k containers | h/l actions | PgUp/Dn | r reload | q"
        })),
        footer,
    );
    if let Some(kind) = state.open {
        form::draw(frame, state, kind, selected);
    }
}

fn draw_header(frame: &mut Frame<'_>, area: Rect, entries: &[Entry], busy: bool, spacious: bool) {
    let running = entries
        .iter()
        .filter(|entry| entry.info.as_ref().is_ok_and(|info| info.active))
        .count();
    let header = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).split(area);
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
}

fn draw_containers(
    frame: &mut Frame<'_>,
    area: Rect,
    state: &UiState,
    entries: &[Entry],
    expanded: bool,
) {
    let items: Vec<_> = entries
        .iter()
        .map(|entry| {
            let (status, color) = match &entry.info {
                Ok(info) if info.active => ("running", GREEN),
                Ok(_) => ("stopped", Color::Reset),
                Err(_) => ("error", RED),
            };
            if expanded {
                ListItem::new(vec![
                    Line::styled(&entry.config.name, Style::default().bold()),
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
                    Span::raw(&entry.config.name),
                ]))
            }
        })
        .collect();
    if entries.is_empty() {
        frame.render_widget(
            Paragraph::new("No containers\nSelect Import package to add one")
                .block(panel("Containers", state.focus == Focus::Containers)),
            area,
        );
    } else {
        frame.render_stateful_widget(
            List::new(items)
                .block(panel("Containers", state.focus == Focus::Containers))
                .highlight_symbol("▍ ")
                .highlight_style(selection_style(true)),
            area,
            &mut ListState::default().with_selected(Some(state.selected)),
        );
    }
}

fn draw_output(frame: &mut Frame<'_>, area: Rect, state: &UiState, output: &str, busy: bool) {
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
        area,
    );
}

#[cfg(test)]
#[path = "../tests/unit/view/tests.rs"]
mod tests;
