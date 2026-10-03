use super::*;

#[test]
fn validates_signal_numbers_and_flags() {
    assert_eq!(SignalNumber::from(Signal::Kill).raw(), libc::SIGKILL);
    assert_eq!(SignalNumber::new(0), Some(SignalNumber::NONE));
    assert_eq!(SignalNumber::new(-1), None);
    assert_eq!(SignalNumber::new(libc::SIGRTMAX() + 1), None);
    assert_eq!(
        SignalNumber::realtime(3).unwrap().raw(),
        libc::SIGRTMIN() + 3
    );
    assert_eq!(SignalActionFlags::RESTART.bits(), libc::SA_RESTART);
}
