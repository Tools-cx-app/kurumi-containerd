use super::*;

#[test]
fn proxy_input_delegates_shutdown_and_preserves_input() {
    let calls = std::cell::Cell::new(0);
    let shutdown = || {
        calls.set(calls.get() + 1);
        Ok(())
    };
    assert_eq!(
        proxy_input(b"\x1b\x11rest", Some(&shutdown)).unwrap(),
        b"rest"
    );
    assert_eq!(
        proxy_input(b"ordinary", Some(&shutdown)).unwrap(),
        b"ordinary"
    );
    assert_eq!(proxy_input(b"\x1b", Some(&shutdown)).unwrap(), b"\x1b");
    assert_eq!(proxy_input(b"\x1b\x11", None).unwrap(), b"\x1b\x11");
    assert_eq!(calls.get(), 1);
    let failure = || Err(std::io::Error::other("shutdown failed").into());
    assert!(proxy_input(b"\x1b\x11", Some(&failure)).is_err());
}

#[cfg(target_os = "android")]
#[test]
fn uses_android_nobody_for_pty_broker() {
    assert_eq!(pty_broker_id(), 9_999);
}

#[cfg(not(target_os = "android"))]
#[test]
fn uses_linux_nobody_for_pty_broker() {
    assert_eq!(pty_broker_id(), 65_534);
}

#[test]
fn allocates_terminal_pair() {
    let console = Console::open().expect("PTY allocation should work");
    let slave = console.open_slave().expect("PTY slave should reopen");
    assert!(is_terminal(console.master.as_fd()).unwrap());
    assert!(is_terminal(slave.as_fd()).unwrap());
}

#[test]
fn transfers_terminal_descriptor() {
    let (sender, receiver) = socket_pair().unwrap();
    let console = Console::open().unwrap();
    send_fd(&sender, &console.master).unwrap();
    let transferred = receive_fd(&receiver).unwrap();
    assert!(is_terminal(transferred.as_fd()).unwrap());
}

#[test]
fn console_and_sync_use_first_valid_terminal_size() {
    let null = std::fs::File::open("/dev/null").unwrap();
    let first = open_pty(None).unwrap();
    let second = open_pty(Some(&WindowSize {
        rows: 31,
        columns: 97,
        x_pixels: 970,
        y_pixels: 310,
    }))
    .unwrap();
    let third = open_pty(Some(&WindowSize {
        rows: 42,
        columns: 120,
        ..WindowSize::default()
    }))
    .unwrap();
    let sources = [
        first.slave.as_fd(),
        second.slave.as_fd(),
        third.slave.as_fd(),
    ];
    let console = Console::open_from(&sources).unwrap();
    let size = terminal_size(console.master.as_fd()).unwrap();
    assert_eq!(
        (size.rows, size.columns, size.x_pixels, size.y_pixels),
        (31, 97, 970, 310)
    );

    sync_terminal_size(
        &[null.as_fd(), first.slave.as_fd(), third.slave.as_fd()],
        console.master.as_fd(),
    )
    .unwrap();
    let size = terminal_size(console.master.as_fd()).unwrap();
    assert_eq!((size.rows, size.columns), (42, 120));
    sync_terminal_size(&[null.as_fd()], console.master.as_fd()).unwrap();
    let size = terminal_size(console.master.as_fd()).unwrap();
    assert_eq!((size.rows, size.columns), (42, 120));
}

#[test]
fn console_defaults_when_source_has_no_valid_size() {
    let source = open_pty(None).unwrap();
    let null = std::fs::File::open("/dev/null").unwrap();
    for fd in [source.slave.as_fd(), null.as_fd()] {
        let console = Console::open_from(&[fd]).unwrap();
        let size = terminal_size(console.master.as_fd()).unwrap();
        assert_eq!((size.rows, size.columns), (24, 80));
    }
}

#[test]
fn sync_preserves_size_when_source_is_invalid() {
    let source = open_pty(None).unwrap();
    let target = open_pty(Some(&WindowSize {
        rows: 31,
        columns: 97,
        ..WindowSize::default()
    }))
    .unwrap();
    for (rows, columns) in [(0, 0), (0, 97), (31, 0)] {
        set_terminal_size(
            source.slave.as_fd(),
            &WindowSize {
                rows,
                columns,
                ..WindowSize::default()
            },
        )
        .unwrap();
        sync_terminal_size(&[source.slave.as_fd()], target.master.as_fd()).unwrap();
        let size = terminal_size(target.slave.as_fd()).unwrap();
        assert_eq!((size.rows, size.columns), (31, 97));
    }
}

#[test]
fn copies_source_terminal_settings() {
    let source = open_pty(None).unwrap();
    let mut settings = terminal_settings(source.slave.as_fd()).unwrap();
    make_raw(&mut settings);
    set_terminal_settings(source.slave.as_fd(), &settings).unwrap();

    let console = Console::open_from(&[source.slave.as_fd()]).unwrap();
    let slave = console.open_slave().unwrap();
    write(&console.master, b"x").unwrap();

    assert!(
        poll_terminal(slave.as_fd(), source.master.as_fd(), false, false)
            .unwrap()
            .0
    );
}
