// Copyright (C) 2026 Tools-cx-app
// SPDX-License-Identifier: GPL-v3-only

use super::*;

fn action(config: &SecurityConfig, syscall: libc::c_long, arg0: u32, arch: u32) -> u32 {
    let filter = seccomp_filter(config).unwrap();
    let mut accumulator = 0;
    let mut position = 0;
    loop {
        let instruction = &filter[position];
        match instruction.code {
            BPF_LD_W_ABS => {
                accumulator = match instruction.value {
                    SECCOMP_DATA_NR_OFFSET => u32::try_from(syscall).unwrap(),
                    SECCOMP_DATA_ARCH_OFFSET => arch,
                    SECCOMP_DATA_ARG0_OFFSET => arg0,
                    offset => panic!("unexpected seccomp load offset {offset}"),
                };
                position += 1;
            }
            BPF_JMP_JEQ_K | BPF_JMP_JSET_K => {
                let matches = if instruction.code == BPF_JMP_JEQ_K {
                    accumulator == instruction.value
                } else {
                    accumulator & instruction.value != 0
                };
                position += 1 + usize::from(if matches {
                    instruction.jump_true
                } else {
                    instruction.jump_false
                });
            }
            BPF_RET_K => return instruction.value,
            code => panic!("unexpected seccomp instruction {code}"),
        }
    }
}

#[test]
fn clone3_requires_user_namespace_opt_in_on_every_supported_architecture() {
    let mut config = SecurityConfig::default();
    assert_eq!(
        action(&config, syscall_clone3(), 0, audit_arch()),
        SECCOMP_ERRNO_NOT_IMPLEMENTED
    );
    config.allow_user_namespaces = true;
    assert_eq!(
        action(&config, syscall_clone3(), 0, audit_arch()),
        SECCOMP_ALLOW
    );
}

#[test]
fn user_namespace_flags_are_filtered_without_blocking_ordinary_process_creation() {
    let mut config = SecurityConfig::default();
    for syscall in [syscall_unshare(), syscall_clone()] {
        assert_eq!(
            action(&config, syscall, namespace_user_flag(), audit_arch()),
            SECCOMP_ERRNO_PERMISSION
        );
        assert_eq!(action(&config, syscall, 0, audit_arch()), SECCOMP_ALLOW);
    }
    config.allow_user_namespaces = true;
    for syscall in [syscall_unshare(), syscall_clone()] {
        assert_eq!(
            action(&config, syscall, namespace_user_flag(), audit_arch()),
            SECCOMP_ALLOW
        );
    }
}

#[test]
fn namespace_opt_in_preserves_architecture_and_host_syscall_restrictions() {
    for allow_user_namespaces in [false, true] {
        let config = SecurityConfig {
            allow_user_namespaces,
            ..SecurityConfig::default()
        };
        assert_eq!(action(&config, syscall_clone(), 0, 0), SECCOMP_KILL_PROCESS);
        for syscall in blocked_syscalls() {
            assert_eq!(
                action(&config, syscall, 0, audit_arch()),
                SECCOMP_ERRNO_PERMISSION
            );
        }
        assert_eq!(
            action(&config, syscall_socket(), ADDRESS_FAMILY_ALG, audit_arch()),
            SECCOMP_ERRNO_PERMISSION
        );
        assert_eq!(
            action(&config, syscall_socket(), 2, audit_arch()),
            SECCOMP_ALLOW
        );
    }
}
