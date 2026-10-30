// Copyright (C) 2026 Tools-cx-app
// SPDX-License-Identifier: GPL-v3-only

use crate::{
    action::{ACTIONS, allowed},
    backend::{Backend, CommandTask, Launch},
    state::UiState,
    terminal::Screen,
    view::draw,
};
use anyhow::{Result, bail};
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::{Terminal, backend::CrosstermBackend};
use std::{
    io::{self, IsTerminal},
    path::Path,
    time::{Duration, Instant},
};

/// Runs the ratatui manager and delegates operations to the CLI executable.
///
/// # Errors
/// Returns an error when the terminal, registry, or child process cannot be accessed.
///
/// # Panics
/// Panics only if an open form has no action.
#[allow(clippy::too_many_lines)]
pub fn run(exe: &Path) -> Result<()> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        bail!("tui requires an interactive terminal");
    }
    let backend = Backend::new(exe);
    let mut entries = backend.load_entries()?;
    let mut state = UiState::new(entries.iter().map(|e| e.pointer.name.clone()).collect());
    let mut output = String::new();
    let mut task: Option<CommandTask> = None;
    let mut refreshed = Instant::now();
    let _screen = Screen::enter()?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    loop {
        if let Some(command) = task.as_mut()
            && let Some(result) = command.poll()?
        {
            task = None;
            output = result;
            state.output_scroll = 0;
            backend.refresh(&mut entries);
        }
        if refreshed.elapsed() >= Duration::from_secs(3) && task.is_none() {
            backend.refresh(&mut entries);
            refreshed = Instant::now();
        }
        let can_open = backend.can_open_terminal();
        terminal.draw(|frame| {
            draw(frame, &state, &entries, &output, task.is_some(), can_open);
        })?;
        if !event::poll(Duration::from_millis(100))? {
            continue;
        }
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        if state.open.is_some() {
            handle_form(
                key.code,
                &mut state,
                &mut output,
                &mut task,
                &backend,
                &entries,
                can_open,
            )?;
        } else {
            handle_screen(
                key.code,
                &mut state,
                &mut entries,
                &mut output,
                task.is_some(),
                &backend,
            );
            if (key.code == KeyCode::Char('q') || key.code == KeyCode::Esc) && task.is_none() {
                break;
            }
        }
    }
    Ok(())
}

fn handle_form(
    code: KeyCode,
    state: &mut UiState,
    output: &mut String,
    task: &mut Option<CommandTask>,
    backend: &Backend<'_>,
    entries: &[crate::registry::Entry],
    can_open: bool,
) -> Result<()> {
    match code {
        KeyCode::Esc => state.cancel(),
        KeyCode::Tab => state.next_field(),
        KeyCode::BackTab => state.previous_field(),
        KeyCode::Char(' ') if state.field == state.fields.len() => state.toggle_force(),
        KeyCode::Backspace => {
            if let Some(v) = state.fields.get_mut(state.field) {
                v.pop();
            }
            state.confirming = false;
            state.form_error.clear();
        }
        KeyCode::Char(c) => {
            if let Some(v) = state.fields.get_mut(state.field) {
                v.push(c);
                state.confirming = false;
                state.form_error.clear();
            }
        }
        KeyCode::Enter => {
            let kind = state.open.expect("form is open");
            let selected = entries.get(state.selected);
            if kind != crate::action::ActionKind::Check {
                let Some(selected) = selected else {
                    state.form_error = "No container selected".into();
                    return Ok(());
                };
                if let Err(error) = allowed(kind, selected, can_open) {
                    state.form_error = format!("{error:#}");
                    return Ok(());
                }
            }
            let submitted = match state.submit() {
                Ok(action) => action,
                Err(error) => {
                    state.form_error = format!("{error:#}");
                    return Ok(());
                }
            };
            if let Some(action) = submitted {
                match backend.launch(&action, selected)? {
                    Launch::Terminal => {
                        *output = "Terminal launched; results appear there".into();
                    }
                    Launch::Captured(command) => {
                        *task = Some(command);
                        *output = format!("Running {}...", kind.label());
                    }
                    Launch::Failed(error) => *output = error,
                }
                state.output_scroll = 0;
                state.focus = crate::state::Focus::Output;
            }
        }
        _ => {}
    }
    Ok(())
}

fn handle_screen(
    code: KeyCode,
    state: &mut UiState,
    entries: &mut Vec<crate::registry::Entry>,
    output: &mut String,
    busy: bool,
    backend: &Backend<'_>,
) {
    let previous = state.selected;
    match code {
        KeyCode::Tab => state.cycle_focus(false),
        KeyCode::BackTab => state.cycle_focus(true),
        KeyCode::PageUp => state.scroll(false),
        KeyCode::PageDown => state.scroll(true),
        KeyCode::Up | KeyCode::Char('k') => state.selected = state.selected.saturating_sub(1),
        KeyCode::Down | KeyCode::Char('j') => {
            state.selected = (state.selected + 1).min(entries.len().saturating_sub(1));
        }
        KeyCode::Left | KeyCode::Char('h') => {
            state.action_index = state.action_index.saturating_sub(1);
        }
        KeyCode::Right | KeyCode::Char('l') => {
            state.action_index = (state.action_index + 1).min(ACTIONS.len() - 1);
        }
        KeyCode::Char('r') if !busy => match backend.load_entries() {
            Ok(new) => {
                *entries = new;
                state.reload(entries.iter().map(|e| e.pointer.name.clone()).collect());
                *output = "Registry reloaded".into();
            }
            Err(e) => *output = format!("{e:#}"),
        },
        KeyCode::Enter if !busy => {
            let kind = ACTIONS[state.action_index];
            let can_open = backend.can_open_terminal();
            let availability = if kind == crate::action::ActionKind::Check {
                Ok(())
            } else if let Some(entry) = entries.get(state.selected) {
                allowed(kind, entry, can_open)
            } else {
                Err(anyhow::anyhow!("No container selected"))
            };
            match availability {
                Ok(()) => state.open(kind),
                Err(error) => {
                    *output = format!("{error:#}");
                    state.output_scroll = 0;
                }
            }
        }
        _ => {}
    }
    if previous != state.selected {
        state.details_scroll = 0;
    }
}

#[cfg(test)]
#[path = "../../tests/unit/app/tests.rs"]
mod tests;
