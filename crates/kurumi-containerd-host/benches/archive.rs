// Copyright (C) 2026 Tools-cx-app
// SPDX-License-Identifier: GPL-v3-only

use std::{
    fs::{self, File},
    hint::black_box,
    io::copy,
    path::{Path, PathBuf},
};

use criterion::{BatchSize, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use flate2::{Compression, write::GzEncoder};
use kurumi_containerd_host::rootfs::Rootfs;
use tempfile::tempdir;

const BACKUP_FORMATS: [&str; 4] = ["tar", "xz", "zst", "zip"];
const INSTALL_FORMATS: [&str; 5] = ["tar", "tar.gz", "xz", "zst", "zip"];

fn create_corpus(root: &Path, count: usize, file_size: usize) -> Vec<(PathBuf, Vec<u8>)> {
    const TEXT: &[u8] = b"kurumi-containerd rootfs benchmark payload\n";

    let mut files = vec![(PathBuf::from("sbin/init"), b"benchmark init\n".to_vec())];
    for index in 0..count {
        let mut state = u32::try_from(index + 1).unwrap();
        let data = (0..file_size)
            .map(|offset| {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                // Mix text and deterministic noise to avoid an all-zero compression workload.
                if offset % 4096 < 2048 {
                    TEXT[offset % TEXT.len()]
                } else {
                    state.to_le_bytes()[3]
                }
            })
            .collect();
        files.push((
            PathBuf::from(format!("usr/share/bench/{index:04}.dat")),
            data,
        ));
    }
    for (relative, data) in &files {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, data).unwrap();
    }
    files
}

fn archive_operations(criterion: &mut Criterion) {
    for (workload, count, file_size) in [
        ("many_small_files", 256, 4096),
        ("large_files", 4, 1024 * 1024),
    ] {
        let directory = tempdir().unwrap();
        let source_path = directory.path().join("source");
        let expected = create_corpus(&source_path, count, file_size);
        let source = Rootfs::from_paths(Some(&source_path), None, directory.path());
        for extension in BACKUP_FORMATS {
            source
                .backup(&directory.path().join(format!("rootfs.{extension}")))
                .expect("create archive fixture through the production backup path");
        }
        let mut encoder = GzEncoder::new(
            File::create(directory.path().join("rootfs.tar.gz")).unwrap(),
            Compression::default(),
        );
        copy(
            &mut File::open(directory.path().join("rootfs.tar")).unwrap(),
            &mut encoder,
        )
        .unwrap();
        encoder.finish().unwrap();

        let mut group = criterion.benchmark_group(format!("rootfs/{workload}"));
        group.sample_size(10);
        group.throughput(Throughput::Bytes(
            expected.iter().map(|(_, data)| data.len() as u64).sum(),
        ));

        for extension in INSTALL_FORMATS {
            let archive = directory.path().join(format!("rootfs.{extension}"));
            let restored = tempdir().unwrap();
            let target = restored.path().join("rootfs");
            Rootfs::from_paths(Some(&target), None, restored.path())
                .install(&archive, None, false, |_| Ok(()))
                .expect("verify archive fixture installs successfully");
            for (relative, data) in &expected {
                assert_eq!(&fs::read(target.join(relative)).unwrap(), data);
            }

            group.bench_with_input(
                BenchmarkId::new("install", extension),
                &archive,
                |bencher, archive| {
                    bencher.iter_batched_ref(
                        || {
                            let parent = tempdir().unwrap();
                            let rootfs = Rootfs::from_paths(
                                Some(&parent.path().join("rootfs")),
                                None,
                                parent.path(),
                            );
                            (parent, rootfs)
                        },
                        |(_, rootfs)| {
                            rootfs
                                .install(black_box(archive), None, false, |_| Ok(()))
                                .expect("install archive");
                        },
                        BatchSize::PerIteration,
                    );
                },
            );
        }

        for extension in BACKUP_FORMATS {
            group.bench_function(BenchmarkId::new("backup", extension), |bencher| {
                bencher.iter_batched_ref(
                    || {
                        let parent = tempdir().unwrap();
                        let output = parent.path().join(format!("rootfs.{extension}"));
                        (parent, output)
                    },
                    |(_, output)| source.backup(black_box(output)).expect("back up rootfs"),
                    BatchSize::PerIteration,
                );
            });
        }
        group.finish();
    }
}

criterion_group!(benches, archive_operations);
criterion_main!(benches);
