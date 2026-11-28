// Copyright (C) 2026 Tools-cx-app
// SPDX-License-Identifier: GPL-v3-only

use kurumi_containerd_runtime::ContainerInfo;
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
};

use super::{RED, panel};
use crate::registry::Entry;
use crate::state::{Focus, UiState};

pub(super) fn draw(frame: &mut Frame<'_>, area: Rect, state: &UiState, selected: Option<&Entry>) {
    let details: String = selected.map_or_else(
        || {
            "Select a container to view details.\nCheck host is available without a container."
                .into()
        },
        |entry| match &entry.info {
            Ok(info) => std::iter::once(format!("Config: {}", entry.config.path.display()))
                .chain(info_lines(info))
                .collect::<Vec<_>>()
                .join("\n"),
            Err(error) => format!("Config: {}\nError: {error:#}", entry.config.path.display()),
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
        area,
    );
}

fn info_lines(info: &ContainerInfo) -> Vec<String> {
    let mut lines = vec![format!("Name: {}", info.name)];
    lines.push(format!("Isolation: {}", info.isolation));
    if info.active {
        lines.extend([
            format!(
                "Active: {} for {}",
                if info.init_pid.is_some() {
                    "active (running)"
                } else {
                    "active (cleanup pending)"
                },
                format_duration(info.uptime_seconds.unwrap_or_default())
            ),
            format!(
                "Main PID: {} ({})",
                info.init_pid.unwrap_or_default(),
                info.init_system.unwrap_or_default()
            ),
            format!("Monitor PID: {}", info.monitor_pid.unwrap_or_default()),
            format!("Tasks: {}", info.processes.unwrap_or_default()),
            format!(
                "Memory: {}",
                format_memory(info.memory_kb.unwrap_or_default())
            ),
            format!("Generation: {}", info.generation.unwrap_or_default()),
        ]);
    } else {
        lines.push("Active: inactive (dead)".to_owned());
    }
    lines.extend([
        format!("Rootfs: {}", info.rootfs.display()),
        format!(
            "UUID: {}",
            info.uuid
                .map_or_else(|| "unassigned".to_owned(), |uuid| uuid.to_string())
        ),
    ]);
    lines
}

fn format_duration(seconds: u64) -> String {
    let days = seconds / 86_400;
    let hours = seconds % 86_400 / 3_600;
    let minutes = seconds % 3_600 / 60;
    let seconds = seconds % 60;
    if days > 0 {
        format!("{days}d {hours:02}h {minutes:02}m {seconds:02}s")
    } else if hours > 0 {
        format!("{hours}h {minutes:02}m {seconds:02}s")
    } else if minutes > 0 {
        format!("{minutes}m {seconds:02}s")
    } else {
        format!("{seconds}s")
    }
}

fn format_memory(kibibytes: u64) -> String {
    if kibibytes >= 1024 * 1024 {
        format_unit(kibibytes, 1024 * 1024, "GiB")
    } else if kibibytes >= 1024 {
        format_unit(kibibytes, 1024, "MiB")
    } else {
        format!("{kibibytes} KiB")
    }
}

fn format_unit(value: u64, unit: u64, suffix: &str) -> String {
    let whole = value / unit;
    let decimal = value % unit * 10 / unit;
    format!("{whole}.{decimal} {suffix}")
}

#[cfg(test)]
#[path = "../../tests/unit/view/details.rs"]
mod tests;
