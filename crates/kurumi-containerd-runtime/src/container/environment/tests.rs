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
        environment.contains(&"PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin")
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
    configured.insert("VALUE", "it's safe");
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
        container_environment(&config.container.environment, &config.container.android).unwrap();

    assert!(
        environment
            .iter()
            .any(|value| value.to_bytes() == b"MY_KEY=my-value")
    );
}
