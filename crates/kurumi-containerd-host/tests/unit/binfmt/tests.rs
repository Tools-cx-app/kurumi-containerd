// Copyright (C) 2026 Tools-cx-app
// SPDX-License-Identifier: GPL-v3-only

use super::*;

fn matches(arch: BinfmtArch, header: &[u8]) -> bool {
    let (magic, mask) = elf::signature(arch);
    header.len() >= magic.len()
        && header
            .iter()
            .zip(magic)
            .zip(mask)
            .all(|((&actual, &expected), &mask)| actual & mask == expected & mask)
}

#[test]
fn elf_matches_executable_and_pie_but_rejects_other_abis() {
    let mut header = *b"\x7fELF\x02\x01\x01\0\0\0\0\0\0\0\0\0\x02\0\xb7\0";
    assert!(matches(BinfmtArch::Aarch64, &header));
    header[16] = 3;
    header[7] = 3;
    assert!(matches(BinfmtArch::Aarch64, &header));
    assert!(!matches(BinfmtArch::Arm, &header));
    assert!(!matches(BinfmtArch::X86_64, &header));
    header[5] = 2;
    assert!(!matches(BinfmtArch::Aarch64, &header));
}

#[test]
fn mips_o32_and_n32_are_distinct_in_both_byte_orders() {
    for (o32, n32, flag) in [
        (BinfmtArch::Mips, BinfmtArch::Mipsn32, 39),
        (BinfmtArch::Mipsel, BinfmtArch::Mipsn32el, 36),
    ] {
        let mut header = elf::signature(o32).0.to_vec();
        assert!(matches(o32, &header));
        assert!(!matches(n32, &header));
        header[flag] |= 0x20;
        assert!(matches(n32, &header));
        assert!(!matches(o32, &header));
    }
}

#[test]
fn registration_encodes_nuls_and_pins_interpreter_without_preserving_argv0() {
    let config = BinfmtConfig {
        arch: BinfmtArch::Aarch64,
        interpreter: PathBuf::from("/usr/bin/qemu-aarch64-static"),
    };
    let value = registration("kurumi-test", &config).unwrap();
    assert!(value.starts_with(":kurumi-test:M:0:\\x7f\\x45\\x4c\\x46"));
    assert!(!value.contains('\0'));
    assert!(value.ends_with(":/usr/bin/qemu-aarch64-static:OCF"));
    let mut config = config;
    config.interpreter = PathBuf::from(format!("/{}", "x".repeat(1920)));
    assert!(registration("kurumi-test", &config).is_err());
}

fn native_elf(program_type: u32) -> Vec<u8> {
    let arch: BinfmtArch = serde_json::from_value(std::env::consts::ARCH.into()).unwrap();
    let magic = elf::signature(arch).0;
    let mut bytes = vec![0; 128];
    bytes[..magic.len()].copy_from_slice(magic);
    let little = bytes[5] == 1;
    let (offset_at, size_at, count_at, size) = if bytes[4] == 2 {
        (32, 54, 56, 56_u16)
    } else {
        (28, 42, 44, 32_u16)
    };
    if bytes[4] == 2 {
        bytes[offset_at..offset_at + 8].copy_from_slice(&if little {
            64_u64.to_le_bytes()
        } else {
            64_u64.to_be_bytes()
        });
    } else {
        bytes[offset_at..offset_at + 4].copy_from_slice(&if little {
            64_u32.to_le_bytes()
        } else {
            64_u32.to_be_bytes()
        });
    }
    bytes[size_at..size_at + 2].copy_from_slice(&if little {
        size.to_le_bytes()
    } else {
        size.to_be_bytes()
    });
    bytes[count_at..count_at + 2].copy_from_slice(&if little {
        1_u16.to_le_bytes()
    } else {
        1_u16.to_be_bytes()
    });
    bytes[64..68].copy_from_slice(&if little {
        program_type.to_le_bytes()
    } else {
        program_type.to_be_bytes()
    });
    bytes
}

#[test]
fn interpreter_validation_rejects_dynamic_foreign_and_truncated_elf() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("interpreter");
    let mut bytes = native_elf(1);
    fs::write(&path, &bytes).unwrap();
    validate_interpreter(&path).unwrap();
    fs::write(&path, native_elf(3)).unwrap();
    assert!(
        validate_interpreter(&path)
            .unwrap_err()
            .to_string()
            .contains("statically linked")
    );
    bytes[18] ^= 0xff;
    fs::write(&path, &bytes).unwrap();
    assert!(
        validate_interpreter(&path)
            .unwrap_err()
            .to_string()
            .contains("host-native")
    );
    fs::write(&path, &native_elf(1)[..80]).unwrap();
    assert!(
        validate_interpreter(&path)
            .unwrap_err()
            .to_string()
            .contains("program header")
    );
    fs::write(&path, b"#!/bin/sh\n").unwrap();
    assert!(validate_interpreter(&path).is_err());
}
