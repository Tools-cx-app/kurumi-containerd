// Copyright (C) 2026 Tools-cx-app
// SPDX-License-Identifier: GPL-v3-only

use std::{
    fmt::Write as _,
    io::{self, IsTerminal},
    path::Path,
    time::{Duration, Instant},
};

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use kurumi_containerd_error::{Result, RuntimeError, bail};
use ratatui::{Terminal, backend::CrosstermBackend};

use crate::{
    action::{ACTIONS, allowed},
    backend::{Backend, CommandTask, Launch},
    services::{Manager, Operation, Services},
    state::UiState,
    terminal::Screen,
    view::{draw, draw_services},
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
    let mut state = UiState::new(entries.iter().map(|e| e.config.name.clone()).collect());
    let mut output = String::new();
    let mut services: Option<Services> = None;
    let mut task: Option<CommandTask> = None;
    let mut refreshed = Instant::now();
    let _screen = Screen::enter()?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    loop {
        if let Some(command) = task.as_mut()
            && let Some(result) = command.poll()?
        {
            let reload_registry = command.reload_registry;
            task = None;
            output = result;
            state.output_scroll = 0;
            if reload_registry {
                match backend.load_entries() {
                    Ok(loaded) => {
                        entries = loaded;
                        state.reload(
                            entries
                                .iter()
                                .map(|entry| entry.config.name.clone())
                                .collect(),
                        );
                    }
                    Err(error) => {
                        let _ = write!(output, "\nRegistry refresh failed: {error:#}");
                    }
                }
            } else {
                backend.refresh(&mut entries);
            }
        }
        if refreshed.elapsed() >= Duration::from_secs(3) && task.is_none() && services.is_none() {
            backend.refresh(&mut entries);
            refreshed = Instant::now();
        }
        if let Some(page) = &mut services {
            page.poll(&backend);
            terminal.draw(|frame| draw_services(frame, page))?;
        } else {
            terminal.draw(|frame| {
                draw(
                    frame,
                    &state,
                    &entries,
                    &output,
                    task.is_some(),
                    backend.can_open_terminal(),
                );
            })?;
        }
        if !event::poll(Duration::from_millis(100))? {
            continue;
        }
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        if let Some(page) = &mut services {
            if handle_services(key.code, page, &backend) {
                services = None;
            }
        } else if state.open.is_some() {
            handle_form(
                key.code,
                &mut state,
                &mut output,
                &mut task,
                &backend,
                &entries,
                backend.can_open_terminal(),
            )?;
        } else if key.code == KeyCode::Char('s') && task.is_none() {
            match entries.get(state.selected) {
                Some(entry)
                    if entry
                        .info
                        .as_ref()
                        .is_ok_and(|info| info.active && info.init_pid.is_some()) =>
                {
                    services = Some(Services::open(&entry.config.name, &backend));
                }
                _ => {
                    output = "Select a running container to manage its services".into();
                    state.output_scroll = 0;
                }
            }
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

fn handle_services(code: KeyCode, page: &mut Services, backend: &Backend<'_>) -> bool {
    if page.busy() {
        return false;
    }
    let operation = if let Some(pending) = page.confirm {
        match code {
            KeyCode::Enter => Some(pending),
            KeyCode::Esc => {
                page.confirm = None;
                None
            }
            _ => None,
        }
    } else {
        match code {
            KeyCode::Esc | KeyCode::Char('q') => return true,
            KeyCode::Up | KeyCode::Char('k') => {
                page.selected = page.selected.saturating_sub(1);
                None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                page.selected = (page.selected + 1).min(page.rows.len().saturating_sub(1));
                None
            }
            KeyCode::PageUp => {
                page.scroll = page.scroll.saturating_sub(5);
                None
            }
            KeyCode::PageDown => {
                page.scroll = page.scroll.saturating_add(5);
                None
            }
            KeyCode::Char('r') => {
                page.output.clear();
                page.refresh(backend);
                None
            }
            KeyCode::Enter => Some(Operation::Status),
            KeyCode::Char('s') => Some(Operation::Start),
            KeyCode::Char('x') => Some(Operation::Stop),
            KeyCode::Char('R') => Some(Operation::Restart),
            KeyCode::Char('e') => Some(Operation::Enable),
            KeyCode::Char('d') => Some(Operation::Disable),
            KeyCode::Char('J') if page.manager == Some(Manager::Systemd) => {
                Some(Operation::Journal)
            }
            _ => None,
        }
    };
    if let Some(operation) = operation
        && let Err(error) = page.request(operation, backend)
    {
        page.output = format!("{error:#}");
    }
    false
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
            if kind.needs_entry() {
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
                state.reload(entries.iter().map(|e| e.config.name.clone()).collect());
                *output = "Registry reloaded".into();
            }
            Err(e) => *output = format!("{e:#}"),
        },
        KeyCode::Enter if !busy => {
            let kind = ACTIONS[state.action_index];
            let can_open = backend.can_open_terminal();
            let availability = if !kind.needs_entry() {
                Ok(())
            } else if let Some(entry) = entries.get(state.selected) {
                allowed(kind, entry, can_open)
            } else {
                Err(RuntimeError::Message("No container selected".to_owned()))
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
#[path = "../tests/unit/app/tests.rs"]
mod tests;
