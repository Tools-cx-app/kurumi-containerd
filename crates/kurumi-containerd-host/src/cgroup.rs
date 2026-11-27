// Copyright (C) 2026 Tools-cx-app
// SPDX-License-Identifier: GPL-v3-only

use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    os::{
        fd::AsRawFd,
        unix::fs::{MetadataExt, OpenOptionsExt},
    },
    path::{Component, Path, PathBuf},
};
#[cfg(feature = "servicectl")]
use std::{
    collections::BTreeSet,
    io::Read,
    os::unix::fs::DirEntryExt,
    time::{Duration, Instant},
};

use kurumi_containerd_config::ResourceConfig;
use kurumi_containerd_helper::fs::{MountFlags, TMPFS_MAGIC, filesystem_type, mount, unmount};
#[cfg(feature = "servicectl")]
use kurumi_containerd_helper::{process::current_pid, signal::Signal};
use procfs::{
    ProcessCGroup,
    process::{MountInfo, Process},
};

use crate::{
    Result,
    error::{ErrorContext as _, ensure},
};
#[cfg(feature = "servicectl")]
use crate::process::ProcessHandle;

pub struct Cgroup {
    paths: Vec<PathBuf>,
    unified: bool,
    #[cfg(feature = "servicectl")]
    shared: Option<SharedCgroup>,
}

#[cfg(feature = "servicectl")]
struct SharedCgroup {
    directory: File,
    parent: File,
    name: String,
}

/// Open container cgroups captured from a running init process.
pub struct CgroupMembership {
    directories: Vec<File>,
}

impl CgroupMembership {
    /// Joins the captured hierarchies without changing their limits.
    ///
    /// # Errors
    /// Returns errors when creating an execution leaf or writing membership fails.
    pub fn attach(&self, pid: i32) -> Result<()> {
        for directory in &self.directories {
            let container = PathBuf::from(format!("/proc/self/fd/{}", directory.as_raw_fd()));
            attach_execution_leaf(&container, pid)?;
        }
        Ok(())
    }
}

impl Cgroup {
    /// Creates a fresh, dedicated cgroup v2 subtree, even without resource limits.
    ///
    /// # Errors
    /// Returns errors for missing v2, unsafe ownership, reused groups, or unavailable limits.
    #[cfg(feature = "servicectl")]
    pub fn create_shared(workdir: &Path, name: &str, resources: &ResourceConfig) -> Result<Self> {
        ensure!(
            !name.is_empty()
                && !name.contains('/')
                && matches!(
                    Path::new(name).components().next(),
                    Some(Component::Normal(_))
                ),
            "invalid shared cgroup name"
        );
        let root =
            cgroup2_root().context("shared-pid-ipc requires a mounted cgroup v2 hierarchy")?;
        let root = open_trusted_directory(&File::open("/")?, &root)?;
        ensure!(
            filesystem_type(&fd_path(&root))? == 0x6367_7270,
            "shared-pid-ipc requires cgroup v2"
        );
        let controllers = requested_controllers(resources, false);
        enable_controllers(&fd_path(&root), &controllers)?;
        let parent_path = fd_path(&root).join("kurumi-containerd");
        match fs::create_dir(&parent_path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.into()),
        }
        let parent = open_trusted_directory(&root, Path::new("kurumi-containerd"))?;
        enable_controllers(&fd_path(&parent), &controllers)?;
        fs::create_dir(fd_path(&parent).join(name)).context(
            "shared cgroup already exists or cannot be created; recover it before starting",
        )?;
        let directory = open_trusted_directory(&parent, Path::new(name))?;
        let path = fd_path(&directory);
        let cgroup = Self {
            paths: Vec::new(),
            unified: true,
            shared: Some(SharedCgroup {
                directory,
                parent,
                name: name.to_owned(),
            }),
        };
        write_limit(
            &path.join("memory.max"),
            resources.memory_bytes.map(|value| value.to_string()),
            "max",
        )?;
        write_limit(
            &path.join("pids.max"),
            resources.pids.map(|value| value.to_string()),
            "max",
        )?;
        write_limit(
            &path.join("cpu.max"),
            resources
                .cpu_quota
                .map(|quota| format!("{quota} {}", resources.cpu_period.unwrap_or(100_000))),
            "max 100000",
        )?;
        enable_controllers(&path, &controllers)?;
        let init = path.join("kurumi-init");
        fs::create_dir(&init)?;
        enable_controllers(&init, &controllers)?;
        let bootstrap = init.join("bootstrap");
        fs::create_dir(&bootstrap)?;
        enable_controllers(&bootstrap, &controllers)?;
        fs::create_dir_all(workdir)?;
        Ok(cgroup)
    }

    /// Captures the sole v2 container directory before its mount namespace exits.
    ///
    /// # Errors
    /// Returns errors when no sole v2 directory exists or its path is unsafe.
    #[cfg(feature = "servicectl")]
    pub fn directory(&self) -> Result<File> {
        if let Some(shared) = &self.shared {
            return Ok(shared.directory.try_clone()?);
        }
        ensure!(
            self.unified && self.paths.len() == 1,
            "no sole cgroup v2 directory"
        );
        open_trusted_directory(&File::open("/")?, &self.paths[0])
    }

    /// Resolves an absolute container-root membership in the monitor's mount view.
    /// The caller validates the monitor identity and exact container membership.
    ///
    /// # Errors
    /// Returns errors for unsafe paths, ownership, symlinks, or missing v2 mounts.
    #[cfg(feature = "servicectl")]
    pub fn capture_shared(monitor: &Process, membership: &str) -> Result<File> {
        let mounts = monitor.mountinfo()?;
        let path = shared_cgroup_path(membership, &mounts.0)?;
        let root = monitor.open_relative("root")?;
        let directory = open_trusted_directory(&root, &path)?;
        ensure!(
            filesystem_type(&fd_path(&directory))? == 0x6367_7270,
            "shared cgroup is not on cgroup v2"
        );
        Ok(directory)
    }

    /// Captures init's actual container cgroups through the monitor's mount view.
    ///
    /// The caller must validate both process identities around this capture.
    ///
    /// # Errors
    /// Returns errors for unreadable membership, unavailable mounts, or missing cgroups.
    pub fn capture_membership(
        name: &str,
        init: &Process,
        monitor: &Process,
    ) -> Result<CgroupMembership> {
        let groups = match init.cgroups() {
            Ok(groups) => groups,
            Err(procfs::ProcError::NotFound(_)) => {
                return Ok(CgroupMembership {
                    directories: Vec::new(),
                });
            }
            Err(error) => return Err(error.into()),
        };
        let mounts = monitor.mountinfo()?;
        let directories = execution_cgroup_paths(name, &groups.0, &mounts.0)?
            .into_iter()
            .map(|path| {
                let root = monitor.open_relative("root")?;
                open_trusted_directory(&root, &path)
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(CgroupMembership { directories })
    }

    /// Creates the configured cgroup hierarchy.
    ///
    /// # Errors
    /// Returns errors for unavailable controllers or failed cgroup configuration.
    pub fn create(
        workdir: &Path,
        name: &str,
        resources: &ResourceConfig,
        required: bool,
        bootstrap: bool,
    ) -> Result<Self> {
        if !cgroup_required(resources, required) {
            return Ok(Self {
                paths: Vec::new(),
                unified: false,
                #[cfg(feature = "servicectl")]
                shared: None,
            });
        }
        if let Some(root) = ensure_cgroup2_root(bootstrap)? {
            let controllers = requested_controllers(resources, false);
            enable_controllers(&root, &controllers)?;
            let parent = root.join("kurumi-containerd");
            fs::create_dir_all(&parent)?;
            enable_controllers(&parent, &controllers)?;
            let path = parent.join(name);
            fs::create_dir_all(&path)
                .with_context(|| format!("failed to create cgroup {}", path.display()))?;
            let cgroup = Self {
                paths: vec![path.clone()],
                unified: true,
                #[cfg(feature = "servicectl")]
                shared: None,
            };
            write_limit(
                &path.join("memory.max"),
                resources.memory_bytes.map(|value| value.to_string()),
                "max",
            )?;
            write_limit(
                &path.join("pids.max"),
                resources.pids.map(|value| value.to_string()),
                "max",
            )?;
            write_limit(
                &path.join("cpu.max"),
                resources
                    .cpu_quota
                    .map(|quota| format!("{quota} {}", resources.cpu_period.unwrap_or(100_000))),
                "max 100000",
            )?;
            fs::create_dir_all(workdir)?;
            return Ok(cgroup);
        }

        let roots = ensure_cgroup1_roots(resources, required, bootstrap)?;
        let mut cgroup = Self {
            paths: Vec::new(),
            unified: false,
            #[cfg(feature = "servicectl")]
            shared: None,
        };
        for (controller, root) in roots {
            let path = root.join("kurumi-containerd").join(name);
            fs::create_dir_all(&path)
                .with_context(|| format!("failed to create cgroup {}", path.display()))?;
            cgroup.paths.push(path.clone());
            match controller {
                Controller::Memory => {
                    if let Some(value) = resources.memory_bytes {
                        fs::write(path.join("memory.limit_in_bytes"), value.to_string())?;
                    }
                }
                Controller::Cpu => {
                    if let Some(quota) = resources.cpu_quota {
                        let period = resources.cpu_period.unwrap_or(100_000);
                        fs::write(path.join("cpu.cfs_quota_us"), quota.to_string())?;
                        fs::write(path.join("cpu.cfs_period_us"), period.to_string())?;
                    }
                }
                Controller::Pids => {
                    fs::write(
                        path.join("pids.max"),
                        resources
                            .pids
                            .map_or_else(|| "max".to_owned(), |value| value.to_string()),
                    )?;
                }
            }
        }
        fs::create_dir_all(workdir)?;
        Ok(cgroup)
    }

    /// Attaches a process to the configured cgroups.
    ///
    /// # Errors
    /// Returns errors when writing cgroup membership fails.
    pub fn attach(&self, pid: i32) -> Result<()> {
        #[cfg(feature = "servicectl")]
        if let Some(shared) = &self.shared {
            let leaf = open_directory(&shared.directory, Path::new("kurumi-init"))?;
            write_cgroup_file(&leaf, "cgroup.procs", &pid.to_string())?;
            return Ok(());
        }
        for path in &self.paths {
            fs::write(path.join("cgroup.procs"), pid.to_string())
                .with_context(|| format!("failed to attach PID {pid} to cgroup"))?;
        }
        Ok(())
    }

    /// Joins an existing container hierarchy without changing its limits.
    ///
    /// # Errors
    /// Returns errors if the hierarchy is missing or membership cannot be written.
    pub fn attach_existing(
        name: &str,
        resources: &ResourceConfig,
        required: bool,
        pid: i32,
    ) -> Result<()> {
        if !cgroup_required(resources, required) {
            return Ok(());
        }
        let roots = if let Some(root) = cgroup2_root() {
            vec![root]
        } else {
            ensure_cgroup1_roots(resources, required, false)?
                .into_iter()
                .map(|(_, root)| root)
                .collect()
        };
        for root in roots {
            let container = root.join("kurumi-containerd").join(name);
            ensure!(
                container.is_dir(),
                "container cgroup is missing: {}",
                container.display()
            );
            attach_execution_leaf(&container, pid)?;
        }
        Ok(())
    }

    /// Removes the owned cgroups.
    ///
    /// # Errors
    /// Returns errors when cgroup cleanup fails.
    pub fn remove(&mut self) -> Result<()> {
        #[cfg(feature = "servicectl")]
        if let Some(shared) = &self.shared {
            remove_cgroup_contents(&shared.directory)?;
            let path = fd_path(&shared.parent).join(&shared.name);
            let current = open_directory(&shared.parent, Path::new(&shared.name))?;
            let original = shared.directory.metadata()?;
            let current = current.metadata()?;
            ensure!(
                original.dev() == current.dev() && original.ino() == current.ino(),
                "shared cgroup directory was replaced"
            );
            fs::remove_dir(path)?;
            self.shared = None;
        }
        let mut first_error = None;
        self.paths.retain(|path| match remove_cgroup_tree(path) {
            Ok(()) => false,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
            Err(error) => {
                first_error.get_or_insert(error);
                true
            }
        });
        first_error.map_or(Ok(()), |error| Err(error.into()))
    }

    #[must_use]
    pub fn unified(&self) -> bool {
        self.unified
    }
}

/// Attaches the single-threaded boot process before its first exec.
///
/// # Errors
/// Returns errors when the captured cgroup's init leaf is unavailable.
#[cfg(feature = "servicectl")]
pub fn attach_shared_init(directory: &File, pid: i32) -> Result<()> {
    ensure!(pid > 0, "invalid shared init PID");
    let leaf = open_directory(directory, Path::new("kurumi-init"))?;
    write_cgroup_file(&leaf, "cgroup.procs", &pid.to_string())?;
    Ok(())
}

/// Moves the init process below the cgroup namespace root.
///
/// # Errors
/// Returns errors when the bootstrap child cannot accept the process.
#[cfg(feature = "servicectl")]
pub fn attach_shared_bootstrap(directory: &File, pid: i32) -> Result<()> {
    ensure!(pid > 0, "invalid shared init PID");
    let leaf = open_directory(directory, Path::new("kurumi-init/bootstrap"))?;
    write_cgroup_file(&leaf, "cgroup.procs", &pid.to_string())?;
    Ok(())
}

fn fd_path(directory: &File) -> PathBuf {
    PathBuf::from(format!("/proc/self/fd/{}", directory.as_raw_fd()))
}

fn open_directory(parent: &File, path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(fd_path(parent).join(path))
}

fn open_trusted_directory(root: &File, path: &Path) -> Result<File> {
    let mut directory = root.try_clone()?;
    let metadata = directory.metadata()?;
    ensure!(
        metadata.uid() == 0 && metadata.mode() & 0o022 == 0,
        "cgroup root must be root-owned and not writable by group or others"
    );
    for part in path.components() {
        match part {
            Component::RootDir => continue,
            Component::Normal(part) => directory = open_directory(&directory, Path::new(part))?,
            _ => {
                return Err(
                    io::Error::new(io::ErrorKind::InvalidInput, "unsafe cgroup path").into(),
                );
            }
        }
        let metadata = directory.metadata()?;
        ensure!(
            metadata.uid() == 0 && metadata.mode() & 0o022 == 0,
            "cgroup path must be root-owned and not writable by group or others"
        );
    }
    Ok(directory)
}

/// Removes an empty captured cgroup subtree, including its root directory.
/// Works after the mount namespace which supplied the handle has exited.
///
/// # Errors
/// Returns errors for live members, inaccessible directories, or replaced roots.
#[cfg(feature = "servicectl")]
pub fn remove_cgroup(directory: &File) -> Result<()> {
    let original = directory.metadata()?;
    if original.nlink() == 0 {
        return Ok(());
    }
    ensure!(
        cgroup_processes(directory)?.is_empty() && !cgroup_populated(directory)?,
        "refusing to remove a populated cgroup"
    );
    let parent = open_directory(directory, Path::new(".."))?;
    let parent_metadata = parent.metadata()?;
    ensure!(
        original.dev() == parent_metadata.dev() && original.ino() != parent_metadata.ino(),
        "refusing to remove a cgroup hierarchy root"
    );
    let mut name = None;
    for entry in fs::read_dir(fd_path(&parent))? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() || entry.ino() != original.ino() {
            continue;
        }
        let candidate = match open_directory(&parent, Path::new(&entry.file_name())) {
            Ok(candidate) => candidate,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        };
        let metadata = candidate.metadata()?;
        if metadata.dev() == original.dev() && metadata.ino() == original.ino() {
            name = Some(entry.file_name());
            break;
        }
    }
    let name = name.context("captured cgroup no longer has a matching parent entry")?;
    remove_cgroup_contents(directory)?;
    let current = open_directory(&parent, Path::new(&name))?.metadata()?;
    ensure!(
        original.dev() == current.dev() && original.ino() == current.ino(),
        "captured cgroup directory was replaced"
    );
    ensure!(
        cgroup_processes(directory)?.is_empty() && !cgroup_populated(directory)?,
        "cgroup became populated during removal"
    );
    fs::remove_dir(fd_path(&parent).join(name))?;
    Ok(())
}

#[cfg(feature = "servicectl")]
fn shared_cgroup_path(membership: &str, mounts: &[MountInfo]) -> Result<PathBuf> {
    let membership = Path::new(membership);
    ensure!(
        membership.is_absolute()
            && membership != Path::new("/")
            && membership
                .components()
                .all(|part| matches!(part, Component::RootDir | Component::Normal(_))),
        "unsafe shared cgroup membership"
    );
    let path = mounts
        .iter()
        .filter(|mount| mount.fs_type == "cgroup2")
        .find_map(|mount| {
            membership
                .strip_prefix(&mount.root)
                .ok()
                .map(|relative| mount.mount_point.join(relative))
        })
        .context("shared cgroup v2 mount is unavailable in monitor mount view")?;
    ensure!(
        path.is_absolute()
            && path
                .components()
                .all(|part| matches!(part, Component::RootDir | Component::Normal(_))),
        "unsafe cgroup mount path"
    );
    Ok(path)
}

#[cfg(feature = "servicectl")]
fn read_cgroup_file(directory: &File, name: &str) -> io::Result<String> {
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(fd_path(directory).join(name))?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;
    Ok(contents)
}

#[cfg(feature = "servicectl")]
fn write_cgroup_file(directory: &File, name: &str, contents: &str) -> io::Result<()> {
    OpenOptions::new()
        .write(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(fd_path(directory).join(name))?
        .write_all(contents.as_bytes())
}

#[cfg(feature = "servicectl")]
fn visit_cgroup_children(
    directory: &File,
    mut visit: impl FnMut(&File, &Path) -> io::Result<()>,
) -> io::Result<()> {
    let device = directory.metadata()?.dev();
    for entry in fs::read_dir(fd_path(directory))? {
        let entry = entry?;
        let kind = match entry.file_type() {
            Ok(kind) => kind,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        if !kind.is_dir() {
            continue;
        }
        let name = PathBuf::from(entry.file_name());
        let child = match open_directory(directory, &name) {
            Ok(child) => child,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        if child.metadata()?.dev() != device {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "cgroup descendant crosses a filesystem boundary",
            ));
        }
        match visit(&child, &name) {
            Err(error)
                if error.kind() == io::ErrorKind::NotFound && child.metadata()?.nlink() == 0 => {}
            result => result?,
        }
    }
    Ok(())
}

#[cfg(feature = "servicectl")]
fn collect_cgroup_processes(directory: &File, processes: &mut BTreeSet<i32>) -> io::Result<()> {
    let contents = match read_cgroup_file(directory, "cgroup.procs") {
        Ok(contents) => contents,
        Err(error)
            if error.kind() == io::ErrorKind::NotFound && directory.metadata()?.nlink() == 0 =>
        {
            return Ok(());
        }
        Err(error) => return Err(error),
    };
    for line in contents.lines() {
        let pid = line
            .parse::<i32>()
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        if pid <= 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid PID in cgroup.procs",
            ));
        }
        processes.insert(pid);
    }
    visit_cgroup_children(directory, |child, _| {
        collect_cgroup_processes(child, processes)
    })
}

/// Lists all live members of the captured subtree, including nested service groups.
///
/// # Errors
/// Returns errors for unreadable, malformed, or symlinked membership files.
#[cfg(feature = "servicectl")]
pub fn cgroup_processes(directory: &File) -> Result<Vec<i32>> {
    let mut processes = BTreeSet::new();
    collect_cgroup_processes(directory, &mut processes)?;
    Ok(processes.into_iter().collect())
}

#[cfg(feature = "servicectl")]
fn process_disappeared(error: &crate::RuntimeError) -> bool {
    let mut source: &(dyn std::error::Error + 'static) = error;
    loop {
        if let Some(error) = source.downcast_ref::<io::Error>() {
            return matches!(error.raw_os_error(), Some(libc::ESRCH | libc::ENOENT));
        }
        match source.source() {
            Some(next) => source = next,
            None => return false,
        }
    }
}

#[cfg(feature = "servicectl")]
fn cgroup_populated(directory: &File) -> io::Result<bool> {
    let contents = match read_cgroup_file(directory, "cgroup.events") {
        Ok(contents) => contents,
        Err(error)
            if error.kind() == io::ErrorKind::NotFound && directory.metadata()?.nlink() == 0 =>
        {
            return Ok(false);
        }
        Err(error) => return Err(error),
    };
    match contents
        .lines()
        .find_map(|line| line.strip_prefix("populated "))
    {
        Some("0") => Ok(false),
        Some("1") => Ok(true),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid populated value in cgroup.events",
        )),
    }
}

/// Kills and waits for an entire captured cgroup subtree to become empty.
/// The caller must remain outside the subtree and separately reap its children.
///
/// # Errors
/// Returns errors for own-process membership, access failures, or cleanup timeout.
#[cfg(feature = "servicectl")]
pub fn terminate_cgroup(directory: &File, timeout: Duration) -> Result<()> {
    let started = Instant::now();
    let mut use_kill = true;
    loop {
        let processes = cgroup_processes(directory)?;
        ensure!(
            !processes.contains(&current_pid()),
            "refusing to terminate cgroup containing the cleanup process"
        );
        if processes.is_empty() && !cgroup_populated(directory)? {
            return Ok(());
        }
        if use_kill {
            match write_cgroup_file(directory, "cgroup.kill", "1") {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => use_kill = false,
                Err(error) => return Err(error).context("failed to kill shared cgroup"),
            }
        }
        if !use_kill {
            for pid in processes {
                if started.elapsed() >= timeout {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "shared cgroup termination timed out while rescanning members",
                    )
                    .into());
                }
                let process = match ProcessHandle::open(pid) {
                    Ok(process) => process,
                    Err(error) if process_disappeared(&error) => continue,
                    Err(error) => return Err(error),
                };
                // Re-read after opening the pidfd so PID reuse cannot target a nonmember.
                let members = cgroup_processes(directory)?;
                ensure!(
                    !members.contains(&current_pid()),
                    "refusing to terminate cgroup containing the cleanup process"
                );
                if !members.contains(&pid) {
                    continue;
                }
                match process.send_signal(Signal::Kill) {
                    Ok(()) => {}
                    Err(error) if process_disappeared(&error) => {}
                    Err(error) => return Err(error),
                }
            }
        }
        if !cgroup_populated(directory)? && cgroup_processes(directory)?.is_empty() {
            return Ok(());
        }
        let remaining = timeout.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "shared cgroup remained populated after termination timeout",
            )
            .into());
        }
        std::thread::sleep(remaining.min(Duration::from_millis(10)));
    }
}

#[cfg(feature = "servicectl")]
fn remove_cgroup_contents(directory: &File) -> io::Result<()> {
    visit_cgroup_children(directory, |child, name| {
        remove_cgroup_contents(child)?;
        fs::remove_dir(fd_path(directory).join(name))
    })
}

fn execution_cgroup_paths(
    name: &str,
    groups: &[ProcessCGroup],
    mounts: &[MountInfo],
) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for group in groups {
        let membership = Path::new(&group.pathname);
        let Some(container) = membership.ancestors().find(|path| {
            path.file_name().is_some_and(|part| part == name)
                && path
                    .parent()
                    .is_some_and(|parent| parent.ends_with("kurumi-containerd"))
        }) else {
            continue;
        };
        ensure!(
            membership.is_absolute()
                && !membership
                    .components()
                    .any(|part| matches!(part, Component::ParentDir)),
            "container cgroup membership has an unsafe path"
        );
        let path = mounts
            .iter()
            .find_map(|mount| {
                let matches = if group.controllers.is_empty() {
                    mount.fs_type == "cgroup2"
                } else {
                    mount.fs_type == "cgroup"
                        && group
                            .controllers
                            .iter()
                            .all(|controller| mount.super_options.contains_key(controller))
                };
                matches
                    .then(|| {
                        container
                            .strip_prefix(&mount.root)
                            .ok()
                            .map(|relative| mount.mount_point.join(relative))
                    })
                    .flatten()
            })
            .with_context(|| {
                format!(
                    "container cgroup mount is unavailable: {}",
                    container.display()
                )
            })?;
        if !paths.contains(&path) {
            paths.push(path);
        }
    }
    Ok(paths)
}

fn attach_execution_leaf(container: &Path, pid: i32) -> Result<()> {
    // Systemd enables subtree controllers, so commands need a leaf cgroup.
    let directory = File::open(container)?;
    match fs::create_dir(fd_path(&directory).join("kurumi-exec")) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error.into()),
    }
    let leaf = open_directory(&directory, Path::new("kurumi-exec"))?;
    OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(fd_path(&leaf).join("cgroup.procs"))?
        .write_all(pid.to_string().as_bytes())
        .context("failed to attach command to container cgroup")
}

fn remove_cgroup_tree(path: &Path) -> std::io::Result<()> {
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            remove_cgroup_tree(&entry.path())?;
        }
    }
    fs::remove_dir(path)
}

fn write_limit(path: &Path, value: Option<String>, unlimited: &str) -> Result<()> {
    if let Some(value) = value {
        fs::write(path, value)?;
    } else if path.exists() {
        fs::write(path, unlimited)?;
    }
    Ok(())
}

fn cgroup_required(resources: &ResourceConfig, required: bool) -> bool {
    required
        || resources.memory_bytes.is_some()
        || resources.cpu_quota.is_some()
        || resources.pids.is_some()
}

fn ensure_cgroup2_root(bootstrap: bool) -> Result<Option<PathBuf>> {
    if let Some(root) = cgroup2_root() {
        return Ok(Some(root));
    }
    if !bootstrap {
        return Ok(None);
    }

    let root = PathBuf::from("/sys/fs/cgroup");
    fs::create_dir_all(&root)?;
    let filesystem = filesystem_type(&root)?;
    let mut mounted_tmpfs = false;
    if filesystem != TMPFS_MAGIC {
        mount(
            Some(Path::new("none")),
            &root,
            Some("tmpfs"),
            MountFlags::NOSUID | MountFlags::NODEV | MountFlags::NOEXEC,
            Some("mode=755,size=16M"),
        )
        .context("failed to mount cgroup tmpfs base")?;
        mounted_tmpfs = true;
    }
    if mount(
        Some(Path::new("none")),
        &root,
        Some("cgroup2"),
        MountFlags::NOSUID | MountFlags::NODEV | MountFlags::NOEXEC,
        None,
    )
    .is_err()
    {
        if mounted_tmpfs {
            let _ = unmount(&root, false);
        }
        return Ok(None);
    }
    Ok(cgroup2_root().filter(|path| path.join("cgroup.controllers").exists()))
}

fn cgroup2_root() -> Option<PathBuf> {
    procfs::process::Process::myself()
        .ok()?
        .mountinfo()
        .ok()?
        .0
        .into_iter()
        .find(|mount| mount.fs_type == "cgroup2")
        .map(|mount| mount.mount_point)
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
enum Controller {
    Memory,
    Cpu,
    Pids,
}

fn enable_controllers(path: &Path, controllers: &[Controller]) -> Result<()> {
    if controllers.is_empty() {
        return Ok(());
    }
    let available = fs::read_to_string(path.join("cgroup.controllers"))?;
    let enabled = fs::read_to_string(path.join("cgroup.subtree_control"))?;
    let mut changes = Vec::new();
    for controller in controllers {
        let name = match controller {
            Controller::Memory => "memory",
            Controller::Cpu => "cpu",
            Controller::Pids => "pids",
        };
        ensure!(
            available.split_whitespace().any(|value| value == name),
            "cgroup controller {name} is unavailable at {}",
            path.display()
        );
        if !enabled.split_whitespace().any(|value| value == name) {
            changes.push(format!("+{name}"));
        }
    }
    if !changes.is_empty() {
        fs::write(path.join("cgroup.subtree_control"), changes.join(" ")).with_context(|| {
            format!("failed to enable cgroup controllers at {}", path.display())
        })?;
    }
    Ok(())
}

fn ensure_cgroup1_roots(
    resources: &ResourceConfig,
    required: bool,
    bootstrap: bool,
) -> Result<Vec<(Controller, PathBuf)>> {
    let controllers = requested_controllers(resources, required);
    if controllers.is_empty() {
        return Ok(Vec::new());
    }
    let mut roots = cgroup1_roots()?;
    if controllers
        .iter()
        .all(|controller| roots.contains_key(controller))
    {
        return Ok(controllers
            .into_iter()
            .filter_map(|controller| roots.remove(&controller).map(|root| (controller, root)))
            .collect());
    }
    ensure!(bootstrap, "required cgroup controllers are not mounted");

    let root = PathBuf::from("/sys/fs/cgroup");
    fs::create_dir_all(&root)?;
    if filesystem_type(&root)? != TMPFS_MAGIC {
        mount(
            Some(Path::new("none")),
            &root,
            Some("tmpfs"),
            MountFlags::NOSUID | MountFlags::NODEV | MountFlags::NOEXEC,
            Some("mode=755,size=16M"),
        )
        .context("failed to mount cgroup tmpfs base")?;
    }
    let hierarchy = root.join("kurumi-containerd");
    fs::create_dir_all(&hierarchy)?;
    let options = if controllers.is_empty() {
        "none,name=kurumi-containerd".to_owned()
    } else {
        controllers
            .iter()
            .map(|controller| match controller {
                Controller::Memory => "memory",
                Controller::Cpu => "cpu",
                Controller::Pids => "pids",
            })
            .collect::<Vec<_>>()
            .join(",")
    };
    mount(
        Some(Path::new("none")),
        &hierarchy,
        Some("cgroup"),
        MountFlags::NOSUID | MountFlags::NODEV | MountFlags::NOEXEC,
        Some(options.as_str()),
    )
    .context("failed to mount cgroup v1 hierarchy")?;
    roots = cgroup1_roots()?;
    ensure!(
        controllers
            .iter()
            .all(|controller| roots.contains_key(controller)),
        "failed to mount required cgroup v1 controllers"
    );
    Ok(controllers
        .into_iter()
        .filter_map(|controller| roots.remove(&controller).map(|root| (controller, root)))
        .collect())
}

fn requested_controllers(resources: &ResourceConfig, required: bool) -> Vec<Controller> {
    let mut controllers = Vec::new();
    if resources.memory_bytes.is_some() {
        controllers.push(Controller::Memory);
    }
    if resources.cpu_quota.is_some() {
        controllers.push(Controller::Cpu);
    }
    if resources.pids.is_some() {
        controllers.push(Controller::Pids);
    }
    if required && controllers.is_empty() {
        // Systemd still needs a writable hierarchy even without limits.
        controllers.push(Controller::Pids);
    }
    controllers
}

fn cgroup1_roots() -> Result<std::collections::HashMap<Controller, PathBuf>> {
    let mountinfo = procfs::process::Process::myself()?.mountinfo()?;
    Ok(parse_cgroup1_roots(&mountinfo.0))
}

fn parse_cgroup1_roots(
    mountinfo: &[procfs::process::MountInfo],
) -> std::collections::HashMap<Controller, PathBuf> {
    let mut roots = std::collections::HashMap::new();
    for mount in mountinfo {
        if mount.fs_type != "cgroup" {
            continue;
        }
        for (name, controller) in [
            ("memory", Controller::Memory),
            ("cpu", Controller::Cpu),
            ("pids", Controller::Pids),
        ] {
            if mount.super_options.contains_key(name) {
                roots
                    .entry(controller)
                    .or_insert_with(|| mount.mount_point.clone());
            }
        }
    }
    roots
}

impl Drop for Cgroup {
    fn drop(&mut self) {
        let _ = self.remove();
    }
}

#[cfg(test)]
#[path = "../tests/unit/cgroup/tests.rs"]
mod tests;
