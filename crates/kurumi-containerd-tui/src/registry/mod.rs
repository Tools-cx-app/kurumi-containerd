use anyhow::Result;
use kurumi_containerd_config::{Config, ConfigPointer};
use kurumi_containerd_runtime::{ContainerInfo, Runtime};

pub(super) struct Entry {
    pub(super) pointer: ConfigPointer,
    pub(super) info: Result<ContainerInfo>,
    pub(super) foreground: bool,
    pub(super) install_ready: bool,
}

pub(super) fn load_entries() -> Result<Vec<Entry>> {
    Ok(ConfigPointer::load_home()?
        .into_iter()
        .map(|pointer| {
            let install_ready = Config::load_for_install(&pointer.file).is_ok();
            let (info, foreground) = match Config::load(&pointer.file) {
                Ok(config) => {
                    let foreground = config.container.foreground;
                    (
                        Runtime::new(config)
                            .and_then(|runtime| runtime.info())
                            .map_err(anyhow::Error::from),
                        foreground,
                    )
                }
                Err(error) => (Err(error.into()), false),
            };
            Entry {
                pointer,
                info,
                foreground,
                install_ready,
            }
        })
        .collect())
}

pub(super) fn refresh(entries: &mut [Entry]) {
    for entry in entries {
        entry.install_ready = Config::load_for_install(&entry.pointer.file).is_ok();
        match Config::load(&entry.pointer.file) {
            Ok(config) => {
                entry.foreground = config.container.foreground;
                entry.info = Runtime::new(config)
                    .and_then(|runtime| runtime.info())
                    .map_err(Into::into);
            }
            Err(error) => entry.info = Err(error.into()),
        }
    }
}
