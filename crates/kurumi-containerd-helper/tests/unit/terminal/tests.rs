// Copyright (C) 2026 Tools-cx-app
// SPDX-License-Identifier: GPL-v3-only

use std::{
    fs::File,
    io::Write as _,
    os::fd::{AsFd, AsRawFd},
};

use super::{PollEvents, poll, receive_fds, send_fds, set_nonblocking, socket_pair};

#[test]
fn polls_readable_and_writable_descriptors() {
    let (sender, receiver) = socket_pair().unwrap();
    let mut sender = File::from(sender);
    let interests = [
        (receiver.as_fd(), PollEvents::READABLE),
        (sender.as_fd(), PollEvents::WRITABLE),
    ];
    let [read, write] = poll(interests, 0).unwrap();
    assert!(read.is_empty());
    assert!(write.contains(PollEvents::WRITABLE));

    sender.write_all(b"data").unwrap();
    let [read] = poll([(receiver.as_fd(), PollEvents::READABLE)], 0).unwrap();
    assert!(read.contains(PollEvents::READABLE));
    let [ignored] = poll([(receiver.as_fd(), PollEvents::empty())], 0).unwrap();
    assert!(ignored.is_empty());
}

#[test]
fn polls_hangup_even_without_requested_events() {
    let (sender, receiver) = socket_pair().unwrap();
    drop(sender);
    let [events] = poll([(receiver.as_fd(), PollEvents::empty())], 0).unwrap();
    assert!(events.contains(PollEvents::HANGUP));
    assert_eq!(poll([], 0).unwrap(), []);
}

#[test]
fn transfers_descriptors_with_close_on_exec() {
    let (sender, receiver) = socket_pair().unwrap();
    let (transferred, _peer) = socket_pair().unwrap();

    send_fds(sender.as_fd(), &[transferred.as_raw_fd()]).unwrap();
    let descriptors = receive_fds(receiver.as_fd(), 1).unwrap();

    // SAFETY: the received descriptor remains owned and live for the call.
    let flags = unsafe { libc::fcntl(descriptors[0].as_raw_fd(), libc::F_GETFD) };
    assert_ne!(flags, -1);
    assert_ne!(flags & libc::FD_CLOEXEC, 0);
}

#[test]
fn enables_nonblocking_io() {
    let (sender, _) = socket_pair().unwrap();
    set_nonblocking(sender.as_fd()).unwrap();

    // SAFETY: sender owns a live descriptor.
    let flags = unsafe { libc::fcntl(sender.as_raw_fd(), libc::F_GETFL) };
    assert_ne!(flags & libc::O_NONBLOCK, 0);
}
