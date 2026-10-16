use std::{error::Error as _, fs, path::Path};

use super::*;
use crate::Config;

#[test]
fn selects_named_entry_and_rejects_ambiguous_lists() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.json");
    fs::write(
        &path,
        r#"[{"name":"one","file":"one.toml"},{"name":"two","file":"two.toml"}]"#,
    )
    .unwrap();
    let pointers = ConfigPointer::load(&path).unwrap();
    assert!(ConfigPointer::select(&pointers, None).is_err());
    assert!(ConfigPointer::select(&pointers, Some("missing")).is_err());
    assert_eq!(
        ConfigPointer::select(&pointers, Some("two")).unwrap().file,
        dir.path().join("two.toml")
    );
    for source in [
        "[]",
        r#"{"name":"one","file":"one.toml"}"#,
        r#"[{"name":"one","file":"one.toml"},{"name":"one","file":"two.toml"}]"#,
    ] {
        fs::write(&path, source).unwrap();
        assert!(ConfigPointer::load(&path).is_err());
    }
}

#[test]
fn resolves_pointer_without_overriding_toml_identity() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.json");
    fs::create_dir(dir.path().join("configs")).unwrap();
    fs::write(
        dir.path().join("configs/container.toml"),
        "[runtime]\n[container]\nname='runtime-name'\nrootfs='rootfs'\n",
    )
    .unwrap();
    fs::write(
        &path,
        r#"[{"name":"Display name","file":"configs/container.toml"}]"#,
    )
    .unwrap();
    let pointers = ConfigPointer::load(&path).unwrap();
    let pointer = ConfigPointer::select(&pointers, None).unwrap();
    assert_eq!(pointer.name, "Display name");
    assert_eq!(pointer.file, dir.path().join("configs/container.toml"));
    let config = Config::load_for_install(&pointer.file).unwrap();
    assert_eq!(config.container.name, "runtime-name");
    assert_eq!(
        config.container.rootfs,
        Some(dir.path().join("configs/rootfs"))
    );
    fs::write(
        &path,
        r#"[{"name":" name ","file":"/absolute/config.toml"}]"#,
    )
    .unwrap();
    let pointers = ConfigPointer::load(&path).unwrap();
    let pointer = ConfigPointer::select(&pointers, Some(" name ")).unwrap();
    assert_eq!(pointer.name, " name ");
    assert_eq!(pointer.file, Path::new("/absolute/config.toml"));
}

#[test]
fn rejects_invalid_pointer_fields() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.json");
    for source in [
        "{}",
        "null",
        "[]",
        r#"{"name":"n"}"#,
        r#"{"file":"f"}"#,
        r#"{"name":1,"file":"f"}"#,
        r#"{"name":"n","file":null}"#,
        r#"{"name":"n","file":"f","extra":true}"#,
        r#"{"name":"","file":"f"}"#,
        r#"{"name":"  ","file":"f"}"#,
        r#"{"name":"n","file":""}"#,
        r#"{"name":"n","file":"\t"}"#,
    ] {
        fs::write(&path, format!("[{source}]")).unwrap();
        let error = ConfigPointer::load(&path).unwrap_err();
        assert!(
            error.to_string().contains(&path.display().to_string()),
            "{error}"
        );
    }
    fs::write(&path, "{").unwrap();
    assert!(ConfigPointer::load(&path).unwrap_err().source().is_some());
    fs::remove_file(&path).unwrap();
    assert!(ConfigPointer::load(&path).unwrap_err().source().is_some());
}

#[test]
fn requires_absolute_nonempty_home() {
    for home in [None, Some("".into()), Some("relative".into())] {
        assert!(path_from_home(home).is_err());
    }
    assert_eq!(
        path_from_home(Some("/home/user".into())).unwrap(),
        Path::new("/home/user/.kurumi-containerd/config.json")
    );
}
