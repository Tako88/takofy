use super::LatestWriter;
use std::io;
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

const WAIT: Duration = Duration::from_secs(2);

type Log = Arc<Mutex<Vec<u32>>>;

fn recorder() -> (Log, impl Fn(&u32) -> io::Result<()> + Send + 'static) {
    let log: Log = Arc::new(Mutex::new(Vec::new()));
    let sink = {
        let log = Arc::clone(&log);
        move |value: &u32| {
            log.lock().expect("the sink log").push(*value);
            Ok(())
        }
    };
    (log, sink)
}

#[test]
fn a_burst_ends_with_the_last_value() {
    let (entered, started) = mpsc::channel();
    let (release, unblocked) = mpsc::channel::<()>();
    let log: Log = Arc::new(Mutex::new(Vec::new()));
    let first = std::cell::Cell::new(true);
    let sink = {
        let log = Arc::clone(&log);
        move |value: &u32| {
            log.lock().expect("the sink log").push(*value);
            if first.replace(false) {
                let _ = entered.send(());
                let _ = unblocked.recv();
            }
            Ok(())
        }
    };
    let writer = LatestWriter::start("latest-test", sink, WAIT).expect("a writer thread");
    writer.send(1);
    started
        .recv_timeout(Duration::from_secs(2))
        .expect("the sink takes its first call");
    for value in 2..=5 {
        writer.send(value);
    }
    release.send(()).expect("unblock the sink");
    drop(writer);
    let log = log.lock().expect("the sink log");
    assert!(
        log.len() < 5,
        "a burst coalesces into fewer writes: {} writes broke that",
        log.len()
    );
    assert_eq!(
        log.last(),
        Some(&5),
        "the last value is the one written: {log:?}"
    );
}

#[test]
fn drop_flushes_a_buffered_value() {
    let (log, sink) = recorder();
    let writer = LatestWriter::start("latest-test", sink, WAIT).expect("a writer thread");
    writer.send(33);
    drop(writer);
    assert_eq!(*log.lock().expect("the sink log"), vec![33]);
}

#[test]
fn a_writer_with_nothing_to_say_writes_nothing() {
    let (log, sink) = recorder();
    let writer = LatestWriter::start("latest-test", sink, WAIT).expect("a writer thread");
    drop(writer);
    assert!(log.lock().expect("the sink log").is_empty());
}

#[test]
fn a_stalled_sink_does_not_stall_drop() {
    let (entered, started) = mpsc::channel();
    let (release, unblocked) = mpsc::channel::<()>();
    let sink = move |_: &u32| {
        let _ = entered.send(());
        let _ = unblocked.recv();
        Ok(())
    };
    let writer = LatestWriter::start("latest-test", sink, Duration::from_millis(50))
        .expect("a writer thread");
    writer.send(40);
    started
        .recv_timeout(Duration::from_secs(2))
        .expect("the sink starts");
    let began = Instant::now();
    drop(writer);
    assert!(
        began.elapsed() < Duration::from_secs(2),
        "drop returns bounded: {:?}",
        began.elapsed()
    );
    release.send(()).expect("unblock the sink");
}

#[test]
fn a_panicking_sink_does_not_panic_drop() {
    let writer = LatestWriter::start("latest-test", |_: &u32| panic!("the sink panicked"), WAIT)
        .expect("a writer thread");
    writer.send(40);
    drop(writer);
}
