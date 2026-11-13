// Copyright (C) 2026 Tools-cx-app
// SPDX-License-Identifier: GPL-v3-only

use std::{fmt::Write as _, fs, hint::black_box, os::unix::fs::symlink, path::Path};

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use kurumi_containerd_config::{Config, parse_environment, resolve_container_path};

const CONFIGS: [(&str, &str); 2] = [
    (
        "minimal",
        r#"[{"runtime": {}, "container": {"name": "benchmark", "rootfs": "rootfs"}}]"#,
    ),
    ("example", include_str!("../../../config.example.json")),
];

fn parse_json(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("config/parse");
    for (name, source) in CONFIGS {
        group.throughput(Throughput::Bytes(source.len() as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(name),
            source,
            |bencher, source| {
                bencher.iter(|| {
                    serde_json::from_str::<Vec<Config>>(black_box(source))
                        .expect("valid JSON fixture")
                });
            },
        );
    }
    group.finish();
}

fn parse_environment_file(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("environment/parse");
    for count in [8, 64, 512] {
        let mut source = String::from("# benchmark environment\n\n");
        for index in 0..count {
            writeln!(source, "export BENCH_{index}=\"value {index}=data\"").unwrap();
        }
        let environment = parse_environment(&source).expect("valid environment fixture");
        assert_eq!(environment.len(), count);
        assert_eq!(environment["BENCH_0"], "value 0=data");

        group.throughput(Throughput::Bytes(source.len() as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(count),
            &source,
            |bencher, source| {
                bencher.iter(|| {
                    parse_environment(black_box(source)).expect("valid environment fixture")
                });
            },
        );
    }
    group.finish();
}

fn configuration_io(criterion: &mut Criterion) {
    let directory = tempfile::tempdir().expect("create benchmark directory");
    let rootfs = directory.path().join("rootfs");
    fs::create_dir_all(rootfs.join("usr/sbin")).unwrap();
    fs::write(rootfs.join("usr/sbin/init"), "init").unwrap();
    symlink("/usr/sbin", rootfs.join("sbin")).unwrap();
    symlink("usr/sbin", rootfs.join("relative-sbin")).unwrap();
    fs::create_dir(directory.path().join("shared")).unwrap();

    let mut group = criterion.benchmark_group("config/load");
    for (name, source) in CONFIGS {
        let path = directory.path().join(format!("{name}.json"));
        fs::write(&path, source).unwrap();
        let config = Config::load(&path).expect("valid configuration fixture");
        assert_eq!(
            config.container.rootfs,
            Some(rootfs.canonicalize().unwrap())
        );
        assert!(config.container.uuid.is_none());

        group.bench_with_input(BenchmarkId::from_parameter(name), &path, |bencher, path| {
            bencher.iter(|| Config::load(black_box(path)).expect("valid configuration fixture"));
        });
        assert_eq!(fs::read_to_string(&path).unwrap(), source);
    }
    group.finish();

    let mut group = criterion.benchmark_group("path/resolve");
    for (name, path) in [
        ("direct", Path::new("/usr/sbin/init")),
        ("absolute_symlink", Path::new("/sbin/init")),
        ("relative_symlink", Path::new("/relative-sbin/init")),
    ] {
        assert_eq!(
            resolve_container_path(&rootfs, path).unwrap(),
            rootfs.join("usr/sbin/init")
        );
        group.bench_with_input(BenchmarkId::from_parameter(name), path, |bencher, path| {
            bencher.iter(|| {
                resolve_container_path(black_box(&rootfs), black_box(path))
                    .expect("valid container path fixture")
            });
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    parse_json,
    parse_environment_file,
    configuration_io
);
criterion_main!(benches);
