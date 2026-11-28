// Copyright (C) 2026 Tools-cx-app
// SPDX-License-Identifier: GPL-v3-only

use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use super::{ACCENT, CRUST, YELLOW, panel};
use crate::action::{ACTIONS, ActionKind, allowed};
use crate::registry::Entry;

fn unavailable(kind: ActionKind, entry: Option<&Entry>, terminal: bool) -> Option<String> {
    if kind.needs_entry() {
        entry.map_or_else(
            || Some("No container selected".into()),
            |entry| {
                allowed(kind, entry, terminal)
                    .err()
                    .map(|error| format!("{error:#}"))
            },
        )
    } else {
        None
    }
}

pub(super) fn draw(
    frame: &mut Frame<'_>,
    area: Rect,
    current: usize,
    selected: Option<&Entry>,
    busy: bool,
    terminal: bool,
) {
    // Keep the selected action visible when only a subset of the labels fits.
    let mut start = current;
    let mut used = ACTIONS[current].label().len() + 4;
    let width = usize::from(area.width.saturating_sub(4));
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
        area,
    );
}
