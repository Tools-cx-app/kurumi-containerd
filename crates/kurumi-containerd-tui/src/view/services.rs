// Copyright (C) 2026 Tools-cx-app
// SPDX-License-Identifier: GPL-v3-only

use ratatui::{
    Frame,
    layout::{Constraint, Layout},
    widgets::{List, ListItem, ListState, Paragraph, Wrap},
};

use super::{hint, panel, selection_style};
use crate::services::{Manager, Services};

pub(crate) fn draw_services(frame: &mut Frame<'_>, page: &Services) {
    let area = frame.area();
    if area.width < 36 || area.height < 10 {
        frame.render_widget(Paragraph::new("Resize terminal to at least 36 x 10"), area);
        return;
    }
    let keys = if page.confirm.is_some() {
        "Enter confirm | Esc cancel".to_owned()
    } else if page.busy() {
        "Running container command…".to_owned()
    } else {
        let journal = if page.manager == Some(Manager::Systemd) {
            " | J journal"
        } else {
            ""
        };
        format!(
            "↑/↓ select | Enter status | s start | x stop | R restart | e enable | d disable{journal} | r refresh | PgUp/Dn output | Esc back"
        )
    };
    let mut lines = vec![String::new()];
    for part in keys.split(" | ") {
        let line = lines.last_mut().expect("at least one hint line");
        if line.is_empty() {
            line.push_str(part);
        } else if line.len() + part.len() + 3 <= usize::from(area.width) {
            line.push_str(" | ");
            line.push_str(part);
        } else {
            lines.push(part.to_owned());
        }
    }
    let height = u16::try_from(lines.len()).unwrap_or(u16::MAX);
    let hints = Paragraph::new(lines.iter().map(|line| hint(line)).collect::<Vec<_>>());
    let rows = Layout::vertical([
        Constraint::Fill(2),
        Constraint::Fill(1),
        Constraint::Length(height),
    ])
    .split(area);
    let title = format!(
        "Services · {} · {}",
        page.container,
        page.manager.map_or(
            if page.busy() {
                "detecting"
            } else {
                "unavailable"
            },
            Manager::label
        )
    );
    let block = panel(&title, true);
    if page.rows.is_empty() {
        let text = if page.busy() {
            "Loading services…"
        } else if page.manager.is_some() {
            "No services found"
        } else {
            "Service list unavailable"
        };
        frame.render_widget(Paragraph::new(text).block(block), rows[0]);
    } else {
        let items = page.rows.iter().map(|service| {
            ListItem::new(format!(
                "{:<11} {:<9} {}",
                service.state, service.startup, service.name
            ))
        });
        frame.render_stateful_widget(
            List::new(items)
                .block(block)
                .highlight_symbol("▍ ")
                .highlight_style(selection_style(true)),
            rows[0],
            &mut ListState::default().with_selected(Some(page.selected)),
        );
    }
    let message = page
        .confirm
        .zip(page.rows.get(page.selected))
        .map(|(op, service)| {
            format!(
                "Confirm {} {} in {}?",
                op.verb(),
                service.name,
                page.container
            )
        });
    frame.render_widget(
        Paragraph::new(message.as_deref().unwrap_or(&page.output))
            .block(panel("Output", false))
            .wrap(Wrap { trim: false })
            .scroll((if message.is_some() { 0 } else { page.scroll }, 0)),
        rows[1],
    );
    frame.render_widget(hints, rows[2]);
}
