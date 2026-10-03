use std::{io, path::PathBuf};

use anyhow::{Context, Result, bail};
use crossterm::{
    execute,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};

fn terminal_available(android: bool, graphical: bool, found: bool) -> bool {
    !android && graphical && found
}

pub(super) fn terminal_command() -> Result<Option<(PathBuf, &'static str)>> {
    let graphical =
        std::env::var_os("DISPLAY").is_some() || std::env::var_os("WAYLAND_DISPLAY").is_some();
    if !terminal_available(cfg!(target_os = "android"), graphical, true) {
        return Ok(None);
    }
    if let Some(name) = std::env::var_os("KURUMI_CONTAINERD_TERMINAL") {
        let path = which::which(&name)
            .with_context(|| format!("terminal {} not found", name.to_string_lossy()))?;
        let kind = match path.file_name().and_then(|value| value.to_str()) {
            Some("xterm") => "xterm",
            Some("kitty") => "kitty",
            Some("alacritty") => "alacritty",
            _ => bail!("KURUMI_CONTAINERD_TERMINAL must be xterm, kitty or alacritty"),
        };
        return Ok(Some((path, kind)));
    }
    Ok(["xterm", "kitty", "alacritty"]
        .into_iter()
        .find_map(|name| which::which(name).ok().map(|path| (path, name))))
}

pub(super) struct Screen;
impl Screen {
    pub(super) fn enter() -> Result<Self> {
        terminal::enable_raw_mode()?;
        if let Err(error) = execute!(io::stdout(), EnterAlternateScreen, crossterm::cursor::Hide) {
            terminal::disable_raw_mode()?;
            return Err(error.into());
        }
        Ok(Self)
    }
}
impl Drop for Screen {
    fn drop(&mut self) {
        let _ = terminal::disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen, crossterm::cursor::Show);
    }
}

#[cfg(test)]
mod tests;
