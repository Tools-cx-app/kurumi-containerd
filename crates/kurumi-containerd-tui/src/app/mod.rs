use crate::{
    action::{ACTIONS, allowed},
    registry::{load_entries, refresh},
    state::UiState,
    terminal::{Screen, terminal_command},
    view::draw,
};
use anyhow::{Result, bail};
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::{Terminal, backend::CrosstermBackend};
use std::{
    fs::File,
    io::{self, IsTerminal, Read, Seek},
    path::Path,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

/// Runs the ratatui manager and delegates operations to the CLI executable.
///
/// # Errors
/// Returns an error when the terminal, registry, or child process cannot be accessed.
///
/// # Panics
/// Panics only if the child-process bookkeeping becomes inconsistent.
#[allow(clippy::too_many_lines)]
pub fn run(exe: &Path) -> Result<()> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        bail!("tui requires an interactive terminal");
    }
    let mut entries = load_entries()?;
    let mut state = UiState::new(entries.iter().map(|e| e.pointer.name.clone()).collect());
    let mut output = String::new();
    let mut child: Option<Child> = None;
    let mut output_file: Option<File> = None;
    let mut refreshed = Instant::now();
    let _screen = Screen::enter()?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    loop {
        if let Some(process) = child.as_mut()
            && process.try_wait()?.is_some()
        {
            let status = child.take().expect("child present").wait()?;
            let mut file = output_file.take().expect("output file present");
            file.rewind()?;
            let mut bytes = Vec::new();
            file.take(1024 * 1024).read_to_end(&mut bytes)?;
            output = format!("Exit: {status}\n{}", String::from_utf8_lossy(&bytes));
            state.output_scroll = 0;
            refresh(&mut entries);
        }
        if refreshed.elapsed() >= Duration::from_secs(3) && child.is_none() {
            refresh(&mut entries);
            refreshed = Instant::now();
        }
        let terminal_choice = terminal_command();
        let can_open = terminal_choice.as_ref().is_ok_and(Option::is_some);
        terminal.draw(|frame| {
            draw(frame, &state, &entries, &output, child.is_some(), can_open);
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
                &mut child,
                &mut output_file,
                exe,
                &entries,
                can_open,
            )?;
        } else {
            handle_screen(
                key.code,
                &mut state,
                &mut entries,
                &mut output,
                child.is_some(),
            );
            if (key.code == KeyCode::Char('q') || key.code == KeyCode::Esc) && child.is_none() {
                break;
            }
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn handle_form(
    code: KeyCode,
    state: &mut UiState,
    output: &mut String,
    child: &mut Option<Child>,
    file: &mut Option<File>,
    exe: &Path,
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
                let args = action.args(selected.map_or("", |entry| entry.pointer.name.as_str()));
                if action.needs_terminal(selected.is_some_and(|entry| entry.foreground)) {
                    if let Ok(Some((path, program))) = terminal_command() {
                        let mut cmd = Command::new(path);
                        match program {
                            "xterm" => {
                                cmd.args(["-hold", "-e"]);
                            }
                            "kitty" => {
                                cmd.args(["--hold", "--"]);
                            }
                            _ => {
                                cmd.args(["--hold", "-e"]);
                            }
                        }
                        let result = cmd
                            .arg(exe)
                            .args(args)
                            .stdin(Stdio::null())
                            .stdout(Stdio::null())
                            .stderr(Stdio::null())
                            .spawn();
                        *output = match result {
                            Ok(_) => "Terminal launched; results appear there".into(),
                            Err(e) => format!("terminal launch failed: {e}"),
                        };
                    } else {
                        *output = "No graphical terminal available".into();
                    }
                } else {
                    let f = tempfile::tempfile()?;
                    let err = f.try_clone()?;
                    let mut cmd = Command::new(exe);
                    cmd.args(args)
                        .stdin(Stdio::null())
                        .stdout(Stdio::from(f.try_clone()?))
                        .stderr(Stdio::from(err));
                    match cmd.spawn() {
                        Ok(process) => {
                            *child = Some(process);
                            *file = Some(f);
                            *output = format!("Running {}...", kind.label());
                        }
                        Err(error) => *output = format!("command launch failed: {error}"),
                    }
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
        KeyCode::Char('r') if !busy => match load_entries() {
            Ok(new) => {
                *entries = new;
                state.reload(entries.iter().map(|e| e.pointer.name.clone()).collect());
                *output = "Registry reloaded".into();
            }
            Err(e) => *output = format!("{e:#}"),
        },
        KeyCode::Enter if !busy => {
            let kind = ACTIONS[state.action_index];
            let can_open = terminal_command().as_ref().is_ok_and(Option::is_some);
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
mod tests;
