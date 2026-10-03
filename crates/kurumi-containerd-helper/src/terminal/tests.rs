use std::os::fd::{AsFd, AsRawFd};

use super::{receive_fds, send_fds, set_nonblocking, socket_pair};

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
