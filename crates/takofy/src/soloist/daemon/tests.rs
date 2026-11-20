use std::time::{Duration, Instant};

use super::{ADOPT_WAIT, Action, BACKOFF_WINDOW, Daemon, Event, SPAWN_WAIT, State};

const NAME: &str = "Takofy Soloist";

fn at(base: Instant, ms: u64) -> Instant {
    base + Duration::from_millis(ms)
}

fn auth(logged_in: bool, name: &str) -> Event {
    Event::AuthState {
        logged_in,
        device_name: name.to_owned(),
    }
}

/// A machine that found the program and started it at `base`.
fn starting(base: Instant) -> Daemon {
    let mut daemon = Daemon::new(NAME);
    daemon.event(Event::Located, base);
    daemon.event(Event::Spawned, base);
    daemon
}

/// A machine whose own daemon answered as this window at `base`.
fn ready(base: Instant) -> Daemon {
    let mut daemon = starting(base);
    daemon.event(Event::PortRead, base);
    daemon.event(Event::Connected, base);
    daemon.event(auth(true, NAME), base);
    daemon
}

#[test]
fn a_program_that_is_not_found_is_missing_and_never_spawns() {
    let base = Instant::now();
    let mut daemon = Daemon::new(NAME);

    assert_eq!(daemon.state(), State::Missing);
    assert_eq!(daemon.action(base), Action::None);
    daemon.event(Event::Tick, at(base, 60_000));
    assert_eq!(daemon.state(), State::Missing);
}

#[test]
fn a_found_program_is_spawned_once() {
    let base = Instant::now();
    let mut daemon = Daemon::new(NAME);
    daemon.event(Event::Located, base);

    assert_eq!(daemon.state(), State::NotRunning);
    assert_eq!(daemon.action(base), Action::Spawn);

    daemon.event(Event::Spawned, base);
    assert_eq!(daemon.state(), State::Starting);
    assert_eq!(
        daemon.action(base),
        Action::Connect,
        "a second daemon would fight the first for the data directory"
    );
}

#[test]
fn a_daemon_that_opens_no_socket_settles_as_no_socket() {
    let base = Instant::now();
    let mut daemon = starting(base);

    daemon.event(Event::Tick, at(base, 9_999));
    assert_eq!(daemon.state(), State::Starting);

    daemon.event(Event::Tick, at(base, 10_000));
    assert_eq!(daemon.state(), State::NoSocket);
    assert_eq!(SPAWN_WAIT, Duration::from_secs(10));
}

#[test]
fn a_port_file_alone_is_not_ready() {
    let base = Instant::now();
    let mut daemon = starting(base);
    daemon.event(Event::PortRead, base);

    assert_eq!(daemon.state(), State::Starting);
    assert!(!daemon.state().playable());
}

#[test]
fn a_daemon_that_answers_as_this_window_is_ready() {
    let base = Instant::now();
    let daemon = ready(base);

    assert_eq!(daemon.state(), State::Ready);
    assert_eq!(daemon.action(base), Action::None);
    assert!(daemon.state().playable());
}

#[test]
fn a_daemon_that_is_not_signed_in_is_unpaired() {
    let base = Instant::now();
    let mut daemon = starting(base);
    daemon.event(Event::Connected, base);
    daemon.event(auth(false, NAME), base);

    assert_eq!(daemon.state(), State::Unpaired);
    assert_eq!(daemon.action(at(base, 1_000)), Action::None);
}

#[test]
fn a_daemon_that_signs_in_after_it_answers_becomes_ready() {
    let base = Instant::now();
    let mut daemon = starting(base);
    daemon.event(Event::Connected, base);
    // The socket opens before the session is restored, so the first frame
    // says no account and a later one says the account is there.
    daemon.event(auth(false, NAME), base);
    assert_eq!(daemon.state(), State::Unpaired);

    daemon.event(auth(true, NAME), at(base, 900));

    assert_eq!(daemon.state(), State::Ready);
    assert!(daemon.state().playable());
}

#[test]
fn a_borrowed_daemon_that_signs_in_late_is_adopted() {
    let base = Instant::now();
    let mut daemon = Daemon::new(NAME);
    daemon.event(Event::Located, base);
    daemon.event(Event::PortRead, base);
    daemon.event(Event::Connected, base);
    daemon.event(auth(false, NAME), base);

    daemon.event(auth(true, NAME), at(base, 400));

    assert_eq!(daemon.state(), State::Adopted);
}

#[test]
fn another_sounds_name_is_not_this_window() {
    let base = Instant::now();
    let mut daemon = starting(base);
    daemon.event(Event::Connected, base);
    daemon.event(auth(true, "Kitchen speaker"), base);

    assert_eq!(
        daemon.state(),
        State::NotRunning,
        "a stale port can belong to a stranger"
    );
    assert!(!daemon.state().playable());
}

#[test]
fn an_expired_exit_is_final() {
    let base = Instant::now();
    let mut daemon = starting(base);
    daemon.event(Event::Exited(10), at(base, 1_000));

    assert_eq!(daemon.state(), State::Expired);
    assert_eq!(daemon.action(at(base, 1_000)), Action::None);
    daemon.event(Event::Tick, at(base, 60_000));
    assert_eq!(daemon.state(), State::Expired, "no restart, ever");
}

#[test]
fn a_general_failure_concludes_nothing_until_the_socket_is_asked() {
    let base = Instant::now();
    let mut daemon = starting(base);
    daemon.event(Event::Exited(1), at(base, 1_000));

    assert_eq!(daemon.state(), State::Starting, "exit 1 is not a diagnosis");
    assert_eq!(
        daemon.action(at(base, 1_000)),
        Action::Connect,
        "no second process is started before the socket is asked"
    );

    daemon.event(Event::Connected, at(base, 1_100));
    daemon.event(auth(true, NAME), at(base, 1_200));

    assert_eq!(
        daemon.state(),
        State::Adopted,
        "the process we did not start is the daemon"
    );
    assert!(daemon.state().playable());
}

#[test]
fn a_general_failure_with_no_evidence_falls_back_to_not_running() {
    let base = Instant::now();
    let mut daemon = starting(base);
    daemon.event(Event::Exited(1), at(base, 1_000));

    daemon.event(Event::Tick, at(base, 1_000 + 1_999));
    assert_eq!(
        daemon.state(),
        State::Starting,
        "the evidence window is open"
    );

    daemon.event(Event::Tick, at(base, 1_000 + 2_000));
    assert_eq!(daemon.state(), State::NotRunning);
    assert_eq!(ADOPT_WAIT, Duration::from_secs(2));
    assert_eq!(
        daemon.action(at(base, 3_000)),
        Action::Spawn,
        "an ordinary failure is tried again, not given up on"
    );
}

#[test]
fn a_strangers_answer_is_no_evidence_of_adoption() {
    let base = Instant::now();
    let mut daemon = starting(base);
    daemon.event(Event::Exited(1), at(base, 1_000));
    daemon.event(Event::Connected, at(base, 1_100));
    daemon.event(auth(true, "Someone else"), at(base, 1_200));

    assert_eq!(daemon.state(), State::NotRunning);
}

#[test]
fn any_other_exit_restarts_with_backoff_and_then_settles() {
    let base = Instant::now();
    let mut daemon = starting(base);

    daemon.event(Event::Exited(2), at(base, 100));
    assert_eq!(daemon.state(), State::NotRunning);
    assert_eq!(daemon.action(at(base, 100)), Action::Spawn);

    daemon.event(Event::Spawned, at(base, 200));
    daemon.event(Event::Exited(0), at(base, 300));
    daemon.event(Event::Spawned, at(base, 400));
    daemon.event(Event::Exited(0), at(base, 500));

    assert_eq!(
        daemon.state(),
        State::NotRunning,
        "the third failure settles the machine"
    );
    assert_eq!(
        daemon.action(at(base, 600)),
        Action::None,
        "a failed loop does not restart itself forever"
    );
    assert_eq!(
        daemon.action(at(base, 100) + BACKOFF_WINDOW),
        Action::Spawn,
        "the window rolls over"
    );
}

#[test]
fn a_persons_request_tries_once_more() {
    let base = Instant::now();
    let mut daemon = starting(base);
    for ms in [100, 200, 300] {
        daemon.event(Event::Exited(0), at(base, ms));
        if ms < 300 {
            daemon.event(Event::Spawned, at(base, ms + 1));
        }
    }
    assert_eq!(daemon.action(at(base, 400)), Action::None);

    daemon.retry();
    assert_eq!(daemon.state(), State::NotRunning);
    assert_eq!(daemon.action(at(base, 400)), Action::Spawn);
}

#[test]
fn a_ready_daemon_is_never_spawned_again() {
    let base = Instant::now();
    let mut daemon = ready(base);

    daemon.retry();
    assert_eq!(daemon.state(), State::Ready);
    assert_eq!(daemon.action(base), Action::None);

    let mut adopted = starting(base);
    adopted.event(Event::Exited(1), at(base, 100));
    adopted.event(Event::Connected, at(base, 200));
    adopted.event(auth(true, NAME), at(base, 300));
    adopted.retry();
    assert_eq!(adopted.action(at(base, 300)), Action::None);
}

#[test]
fn an_expired_build_is_never_spawned() {
    let base = Instant::now();
    let mut daemon = Daemon::new(NAME);
    daemon.expire();
    daemon.event(Event::Located, base);

    assert_eq!(daemon.state(), State::Expired);
    assert_eq!(daemon.action(base), Action::None);
}

#[test]
fn every_settled_state_names_the_fault_it_answers_with() {
    use takofy_core::SoloistFault;

    assert_eq!(State::Missing.fault(), Some(SoloistFault::Missing));
    assert_eq!(State::NotRunning.fault(), Some(SoloistFault::NotRunning));
    assert_eq!(State::NoSocket.fault(), Some(SoloistFault::NoSocket));
    assert_eq!(State::Unpaired.fault(), Some(SoloistFault::Unpaired));
    assert_eq!(State::Expired.fault(), Some(SoloistFault::Expired));
    assert_eq!(State::Starting.fault(), None);
    assert_eq!(State::Ready.fault(), None);
    assert_eq!(State::Adopted.fault(), None);
}

#[test]
fn a_daemon_waiting_for_a_phone_is_not_killed() {
    // Being signed out is the state a first pairing starts in, and the phone
    // can only fix it while the process is alive.
    assert!(State::Unpaired.keeps_child());
    assert!(State::Starting.keeps_child());
    assert!(State::Ready.keeps_child());
    assert!(State::Adopted.keeps_child());
    assert!(!State::NotRunning.keeps_child());
    assert!(!State::NoSocket.keeps_child());
    assert!(!State::Expired.keeps_child());
    assert!(!State::Missing.keeps_child());
}
