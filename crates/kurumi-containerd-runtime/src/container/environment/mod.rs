use std::{
    collections::BTreeMap, env, ffi::CString, fmt::Write, fs, os::unix::fs::symlink, path::Path,
};

use crate::{Result, error::ErrorContext as _};
use kurumi_containerd_config::AndroidConfig;

const DEFAULT_PATH: &str = "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";

const XDG_DEFAULTS: &[(&str, &str)] = &[
    ("XDG_CONFIG_HOME", "$HOME/.config"),
    ("XDG_CACHE_HOME", "$HOME/.cache"),
    ("XDG_DATA_HOME", "$HOME/.local/share"),
    ("XDG_STATE_HOME", "$HOME/.local/state"),
    ("XDG_DATA_DIRS", "/usr/local/share:/usr/share"),
    ("XDG_CONFIG_DIRS", "/etc/xdg"),
    ("XDG_RUNTIME_DIR", "/run/user/$(id -u)"),
];

pub(crate) fn login_environment_defaults(
    environment: &mut Vec<CString>,
    home: &str,
    uid: u32,
) -> Result<()> {
    for &(key, default) in XDG_DEFAULTS {
        let prefix = format!("{key}=");
        if !environment.iter().any(|entry| {
            entry.to_bytes().starts_with(prefix.as_bytes()) && entry.to_bytes().len() > prefix.len()
        }) {
            environment.retain(|entry| !entry.to_bytes().starts_with(prefix.as_bytes()));
            let value = default
                .replace("$HOME", home)
                .replace("$(id -u)", &uid.to_string());
            environment.push(variable(key, &value)?);
        }
    }
    Ok(())
}

pub(crate) fn container_environment(
    configured: &BTreeMap<String, String>,
    android: &AndroidConfig,
) -> Result<Vec<CString>> {
    let term = env::var("TERM")
        .ok()
        .filter(|value| !value.is_empty() && !value.contains('.') && !value.starts_with("bg"))
        .unwrap_or_else(|| "xterm-256color".to_owned());
    let mut environment = BTreeMap::from([
        ("PATH".to_owned(), DEFAULT_PATH.to_owned()),
        ("TERM".to_owned(), term),
        ("HOME".to_owned(), "/root".to_owned()),
        ("container".to_owned(), "kurumi-containerd".to_owned()),
        ("LANG".to_owned(), "en_US.UTF-8".to_owned()),
    ]);
    if android.termux_x11 {
        environment.insert("DISPLAY".to_owned(), ":5".to_owned());
    }
    if android.virgl {
        environment.insert("GALLIUM_DRIVER".to_owned(), "virpipe".to_owned());
    }
    if android.pulse_audio {
        environment.insert(
            "PULSE_SERVER".to_owned(),
            "unix:/tmp/.pulse-socket".to_owned(),
        );
    }
    environment.extend(configured.clone());
    environment
        .iter()
        .map(|(key, value)| variable(key, value))
        .collect()
}

pub(crate) fn session_environment(
    configured: &BTreeMap<String, String>,
    android: &AndroidConfig,
) -> Result<Vec<CString>> {
    let source = match fs::read_to_string("/etc/environment") {
        Ok(source) => source,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error).context("failed to read /etc/environment"),
    };
    session_environment_from(&source, configured, android)
}

fn session_environment_from(
    source: &str,
    configured: &BTreeMap<String, String>,
    android: &AndroidConfig,
) -> Result<Vec<CString>> {
    let mut environment = kurumi_containerd_config::parse_environment(source)?;
    environment.extend(configured.clone());
    container_environment(&environment, android)
}

pub(crate) fn write_profile_environment(
    configured: &BTreeMap<String, String>,
    android: &AndroidConfig,
) -> Result<()> {
    let mut environment = configured.clone();
    if android.termux_x11 {
        environment
            .entry("DISPLAY".to_owned())
            .or_insert(":5".to_owned());
    }
    if android.virgl {
        environment
            .entry("GALLIUM_DRIVER".to_owned())
            .or_insert("virpipe".to_owned());
    }
    if android.pulse_audio {
        environment
            .entry("PULSE_SERVER".to_owned())
            .or_insert("unix:/tmp/.pulse-socket".to_owned());
    }
    let contents = render_profile_environment(&environment);
    fs::write("/run/kurumi-containerd.env", contents)
        .context("failed to write /run/kurumi-containerd.env")?;
    if Path::new("/etc/profile.d").is_dir() {
        let link = Path::new("/etc/profile.d/kurumi_containerd_env.sh");
        match fs::remove_file(link) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error).context("failed to replace profile environment link"),
        }
        symlink("/run/kurumi-containerd.env", link)
            .context("failed to create profile environment link")?;
    }
    Ok(())
}

fn render_profile_environment(environment: &BTreeMap<String, String>) -> String {
    let mut contents = environment
        .iter()
        .fold(String::new(), |mut output, (key, value)| {
            writeln!(output, "export {key}='{}'", value.replace('\'', "'\\''"))
                .expect("writing to a String cannot fail");
            output
        });
    for &(key, default) in XDG_DEFAULTS {
        writeln!(contents, "export {key}=\"${{{key}:-{default}}}\"")
            .expect("writing to a String cannot fail");
    }
    contents
}

fn variable(key: &str, value: &str) -> Result<CString> {
    CString::new(format!("{key}={value}"))
        .with_context(|| format!("invalid NUL byte in environment variable {key}"))
}

#[cfg(test)]
mod tests;
