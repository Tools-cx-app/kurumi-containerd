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
mod tests {
    use std::os::unix::fs::MetadataExt as _;

    use kurumi_containerd_config::Config;
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn applies_defaults_and_user_overrides() {
        let mut configured = BTreeMap::new();
        configured.insert("LANG".to_owned(), "C.UTF-8".to_owned());
        configured.insert("APP_MODE".to_owned(), "test".to_owned());
        let environment = container_environment(&configured, &AndroidConfig::default()).unwrap();
        let environment = environment
            .iter()
            .map(|value| value.to_str().unwrap())
            .collect::<Vec<_>>();
        assert!(
            environment
                .contains(&"PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin")
        );
        assert!(environment.contains(&"container=kurumi-containerd"));
        assert!(environment.contains(&"LANG=C.UTF-8"));
        assert_eq!(
            environment
                .iter()
                .filter(|entry| entry.starts_with("LANG="))
                .count(),
            1
        );
        assert!(environment.contains(&"APP_MODE=test"));
    }

    #[test]
    fn quotes_profile_values_for_shell() {
        let mut configured = BTreeMap::new();
        configured.insert("VALUE".to_owned(), "it's safe".to_owned());
        let rendered = render_profile_environment(&configured);
        assert!(rendered.starts_with("export VALUE='it'\\''s safe'\n"));
    }

    #[test]
    fn profile_sets_user_specific_xdg_defaults_and_preserves_overrides() {
        let dir = tempdir().unwrap();
        let profile = dir.path().join("profile.sh");
        fs::write(&profile, render_profile_environment(&BTreeMap::new())).unwrap();
        for (home, configured) in [("/root", false), ("/home/developer", true)] {
            let mut command = std::process::Command::new("/bin/sh");
            command
                .env_clear()
                .env("PATH", DEFAULT_PATH)
                .env("HOME", home);
            if configured {
                command.env("XDG_CONFIG_HOME", "/custom/config");
            }
            let output = command
                .arg("-c")
                .arg(". \"$1\"; printf '%s\\n' \"$XDG_CONFIG_HOME\" \"$XDG_CACHE_HOME\" \"$XDG_DATA_HOME\" \"$XDG_STATE_HOME\" \"$XDG_DATA_DIRS\" \"$XDG_CONFIG_DIRS\" \"$XDG_RUNTIME_DIR\"")
                .arg("sh")
                .arg(&profile)
                .output()
                .unwrap();
            assert!(output.status.success());
            let uid = fs::metadata(dir.path()).unwrap();
            let config = if configured {
                "/custom/config".to_owned()
            } else {
                format!("{home}/.config")
            };
            assert_eq!(
                String::from_utf8(output.stdout).unwrap(),
                format!(
                    "{config}\n{home}/.cache\n{home}/.local/share\n{home}/.local/state\n/usr/local/share:/usr/share\n/etc/xdg\n/run/user/{}\n",
                    uid.uid()
                )
            );
        }
    }

    #[test]
    fn fallback_login_sets_xdg_defaults_for_the_selected_user() {
        let mut environment = vec![
            CString::new("XDG_CONFIG_HOME=/custom/config").unwrap(),
            CString::new("XDG_CACHE_HOME=").unwrap(),
        ];
        login_environment_defaults(&mut environment, "/home/developer", 1000).unwrap();
        let values = environment
            .iter()
            .map(|entry| entry.to_str().unwrap())
            .collect::<Vec<_>>();
        assert!(values.contains(&"XDG_CONFIG_HOME=/custom/config"));
        assert!(values.contains(&"XDG_CACHE_HOME=/home/developer/.cache"));
        assert!(values.contains(&"XDG_RUNTIME_DIR=/run/user/1000"));
        assert!(!values.contains(&"XDG_CACHE_HOME="));
    }

    #[test]
    fn configured_environment_overrides_session_environment() {
        let configured = BTreeMap::from([("LANG".to_owned(), "configured".to_owned())]);
        let environment = session_environment_from(
            "LANG=image\nIMAGE_ONLY=yes\n",
            &configured,
            &AndroidConfig::default(),
        )
        .unwrap();
        let environment = environment
            .iter()
            .map(|value| value.to_str().unwrap())
            .collect::<Vec<_>>();

        assert!(environment.contains(&"LANG=configured"));
        assert!(environment.contains(&"IMAGE_ONLY=yes"));
        assert!(!environment.contains(&"LANG=image"));
    }

    #[test]
    fn injects_environment_key_from_config() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("rootfs/sbin")).unwrap();
        fs::write(dir.path().join("rootfs/sbin/init"), "").unwrap();
        let path = dir.path().join("kurumi-containerd.toml");
        fs::write(
            &path,
            "[runtime]\n[container]\nname='test'\nrootfs='rootfs'\n[container.environment]\nMY_KEY='my-value'\n",
        )
        .unwrap();

        let config = Config::load(&path).unwrap();
        let environment =
            container_environment(&config.container.environment, &config.container.android)
                .unwrap();

        assert!(
            environment
                .iter()
                .any(|value| value.to_bytes() == b"MY_KEY=my-value")
        );
    }
}
