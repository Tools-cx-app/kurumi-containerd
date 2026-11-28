// Copyright (C) 2026 Tools-cx-app
// SPDX-License-Identifier: GPL-v3-only

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Style},
    text::Line,
    widgets::{Clear, Paragraph, Wrap},
};

use super::{ACCENT, RED, SURFACE, TEXT, YELLOW, hint, panel, selection_style};
use crate::action::ActionKind;
use crate::registry::Entry;
use crate::state::UiState;

fn warning_color(warning: bool) -> Color {
    if warning { YELLOW } else { Color::Reset }
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

pub(super) fn draw(
    frame: &mut Frame<'_>,
    state: &UiState,
    kind: ActionKind,
    entry: Option<&Entry>,
) {
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
            entry.map_or("Host", |entry| entry.config.name.as_str())
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
