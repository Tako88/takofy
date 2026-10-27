use super::super::wake::Waker;
use super::super::*;
use super::test_support::*;
use crate::cover::spawn_cover;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::channel;
use takofy_model::CoverImage;

#[test]
fn a_fetched_cover_decodes_and_reaches_the_window() {
    let runtime = runtime();
    let (tx, rx) = channel();
    let notifier = Notifier {
        tx,
        slot: WakeSlot::default(),
    };
    let url = CoverUrl::from_wire("https://i.scdn.co/image/abc").expect("a cover");
    let in_flight = Arc::new(Mutex::new(HashSet::new()));

    let update = runtime.block_on(async {
        let mut tasks = JoinSet::new();
        spawn_cover(
            &mut tasks,
            Arc::new(Semaphore::new(MAX_CONCURRENT_COVERS)),
            Arc::clone(&in_flight),
            url.clone(),
            async { Ok(include_bytes!("../../../tests/fixtures/cover.jpg").to_vec()) },
            |bytes| crate::decode::decode(&bytes),
            notifier,
        );
        let finished = tasks.join_next().await.expect("one task");
        finished.expect("the task does not panic");
        rx.recv().expect("update")
    });

    match update {
        Update::Cover { url: seen, image } => {
            assert_eq!(seen, url);
            assert_eq!((image.width, image.height), (16, 8));
        }
        other => panic!("expected a cover, got {other:?}"),
    }
    assert!(in_flight.lock().expect("lock").is_empty());
}

#[test]
fn a_failed_fetch_reaches_the_window_as_cover_failed() {
    let runtime = runtime();
    let (tx, rx) = channel();
    let notifier = Notifier {
        tx,
        slot: WakeSlot::default(),
    };
    let url = CoverUrl::from_wire("https://i.scdn.co/image/abc").expect("a cover");
    let in_flight = Arc::new(Mutex::new(HashSet::new()));

    let update = runtime.block_on(async {
        let mut tasks = JoinSet::new();
        spawn_cover(
            &mut tasks,
            Arc::new(Semaphore::new(MAX_CONCURRENT_COVERS)),
            Arc::clone(&in_flight),
            url.clone(),
            async { Err(crate::cover::CoverError::Fetch) },
            |_| Err(crate::cover::CoverError::Decode),
            notifier,
        );
        let finished = tasks.join_next().await.expect("one task");
        finished.expect("the task does not panic");
        rx.recv().expect("update")
    });

    assert_eq!(update, Update::CoverFailed { url });
    assert!(in_flight.lock().expect("lock").is_empty());
}

#[test]
fn a_fetch_with_no_session_or_client_is_answered_with_cover_failed() {
    let url = CoverUrl::from_wire("https://i.scdn.co/image/abc").expect("a cover");
    for (connected, client) in [
        (
            false,
            Some(reqwest::Client::builder().build().expect("client")),
        ),
        (true, None),
    ] {
        let (tx, rx) = channel();
        let notifier = Notifier {
            tx,
            slot: WakeSlot::default(),
        };
        let mut tasks = JoinSet::new();
        handle_fetch(
            connected,
            client.as_ref(),
            &mut tasks,
            Arc::new(Semaphore::new(MAX_CONCURRENT_COVERS)),
            Arc::new(Mutex::new(HashSet::new())),
            url.clone(),
            notifier,
        );
        assert!(tasks.is_empty(), "no fetch is spawned");
        assert_eq!(
            rx.try_recv().expect("one reply"),
            Update::CoverFailed { url: url.clone() }
        );
        assert!(rx.try_recv().is_err(), "exactly one reply");
    }
}

#[test]
fn a_fetch_before_sign_in_is_answered_with_cover_failed() {
    let store = Arc::new(MemoryStore::default());
    let source = Arc::new(FixedSource::default());
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::default());

    run_window(runtime(), host, store, source, |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        assert_eq!(
            ui_end.updates.recv().expect("resume update"),
            Update::NeedsAuthorization
        );
        let url = CoverUrl::from_wire("https://i.scdn.co/image/abc").expect("a cover");
        ui_end
            .commands
            .send(Command::Fetch { url: url.clone() })
            .expect("fetch");
        assert_eq!(
            ui_end.updates.recv().expect("cover reply"),
            Update::CoverFailed { url }
        );
    });
}

#[test]
fn a_failed_decode_reaches_the_window_as_cover_failed() {
    let runtime = runtime();
    let (tx, rx) = channel();
    let notifier = Notifier {
        tx,
        slot: WakeSlot::default(),
    };
    let url = CoverUrl::from_wire("https://i.scdn.co/image/abc").expect("a cover");
    let in_flight = Arc::new(Mutex::new(HashSet::new()));

    let update = runtime.block_on(async {
        let mut tasks = JoinSet::new();
        spawn_cover(
            &mut tasks,
            Arc::new(Semaphore::new(MAX_CONCURRENT_COVERS)),
            Arc::clone(&in_flight),
            url.clone(),
            async { Ok(vec![0u8; 16]) },
            |bytes| crate::decode::decode(&bytes),
            notifier,
        );
        let finished = tasks.join_next().await.expect("one task");
        finished.expect("the task does not panic");
        rx.recv().expect("update")
    });

    assert_eq!(update, Update::CoverFailed { url });
    assert!(in_flight.lock().expect("lock").is_empty());
}

#[test]
fn a_decode_that_outruns_its_budget_fails_instead_of_hanging() {
    let runtime = runtime();
    let (tx, rx) = channel();
    let notifier = Notifier {
        tx,
        slot: WakeSlot::default(),
    };
    let url = CoverUrl::from_wire("https://i.scdn.co/image/abc").expect("a cover");
    let in_flight = Arc::new(Mutex::new(HashSet::new()));

    let update = runtime.block_on(async {
        let mut tasks = JoinSet::new();
        crate::cover::spawn_cover_with_timeout(
            &mut tasks,
            Arc::new(Semaphore::new(MAX_CONCURRENT_COVERS)),
            Arc::clone(&in_flight),
            url.clone(),
            async { Ok(include_bytes!("../../../tests/fixtures/cover.jpg").to_vec()) },
            |_| {
                std::thread::sleep(Duration::from_millis(500));
                Ok(CoverImage::new(1, 1, vec![0, 0, 0, 255]).expect("one pixel"))
            },
            notifier,
            Duration::from_millis(20),
        );
        let finished = tasks.join_next().await.expect("one task");
        finished.expect("the task does not panic");
        rx.recv().expect("update")
    });

    assert_eq!(update, Update::CoverFailed { url });
    assert!(in_flight.lock().expect("lock").is_empty());
}

/// Records whether the in-flight set was already empty when the wake ran,
/// which is the moment the reply itself has just been queued.
struct InFlightProbe {
    in_flight: Arc<Mutex<HashSet<CoverUrl>>>,
    empty_at_wake: AtomicBool,
}

impl Waker for InFlightProbe {
    fn wake(&self) {
        let empty = self.in_flight.lock().expect("lock").is_empty();
        self.empty_at_wake.store(empty, Ordering::SeqCst);
    }
}

#[test]
fn a_retry_after_a_failed_fetch_is_not_deduped() {
    let runtime = runtime();
    let (tx, rx) = channel();
    let url = CoverUrl::from_wire("https://i.scdn.co/image/abc").expect("a cover");
    let in_flight = Arc::new(Mutex::new(HashSet::new()));
    let probe = Arc::new(InFlightProbe {
        in_flight: Arc::clone(&in_flight),
        empty_at_wake: AtomicBool::new(false),
    });
    let slot = WakeSlot::default();
    slot.install(probe.clone());
    let notifier = Notifier { tx, slot };
    let fetches = Arc::new(AtomicUsize::new(0));

    runtime.block_on(async {
        let mut tasks = JoinSet::new();
        let permits = Arc::new(Semaphore::new(MAX_CONCURRENT_COVERS));
        for _ in 0..2 {
            let counting = Arc::clone(&fetches);
            spawn_cover(
                &mut tasks,
                Arc::clone(&permits),
                Arc::clone(&in_flight),
                url.clone(),
                async move {
                    counting.fetch_add(1, Ordering::SeqCst);
                    Err(crate::cover::CoverError::Fetch)
                },
                |_| Err(crate::cover::CoverError::Decode),
                notifier.clone(),
            );
            assert_eq!(
                rx.recv().expect("failure"),
                Update::CoverFailed { url: url.clone() }
            );
        }
        while tasks.join_next().await.is_some() {}
    });

    // The second request is a real fetch: the first released its claim
    // before it announced the failure.
    assert_eq!(fetches.load(Ordering::SeqCst), 2);
    assert!(
        probe.empty_at_wake.load(Ordering::SeqCst),
        "the in-flight entry must be gone before the reply"
    );
    assert!(in_flight.lock().expect("lock").is_empty());
}

#[test]
fn a_finished_cover_task_is_reaped() {
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let mut tasks = JoinSet::new();
        tasks.spawn(async {});
        // One yield runs the ready task to completion, leaving its cell in
        // the set until something joins it.
        tokio::task::yield_now().await;
        assert_eq!(tasks.len(), 1);
        reap_finished_covers(&mut tasks);
        assert_eq!(tasks.len(), 0);
    });
}

#[test]
fn a_duplicate_fetch_for_an_in_flight_url_spawns_one_task() {
    let runtime = runtime();
    let (tx, _rx) = channel();
    let notifier = Notifier {
        tx,
        slot: WakeSlot::default(),
    };
    let url = CoverUrl::from_wire("https://i.scdn.co/image/abc").expect("a cover");
    let in_flight = Arc::new(Mutex::new(HashSet::new()));
    let fetches = Arc::new(AtomicUsize::new(0));

    runtime.block_on(async {
        let mut tasks = JoinSet::new();
        let permits = Arc::new(Semaphore::new(MAX_CONCURRENT_COVERS));
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        spawn_cover(
            &mut tasks,
            Arc::clone(&permits),
            Arc::clone(&in_flight),
            url.clone(),
            async move {
                let _ = started_tx.send(());
                std::future::pending().await
            },
            |_| Err(crate::cover::CoverError::Decode),
            notifier.clone(),
        );
        started_rx.await.expect("the first fetch started");

        // A second request for the same URL while it is in flight is
        // dropped rather than fetched again.
        let counting = Arc::clone(&fetches);
        spawn_cover(
            &mut tasks,
            Arc::clone(&permits),
            Arc::clone(&in_flight),
            url.clone(),
            async move {
                counting.fetch_add(1, Ordering::SeqCst);
                Ok(Vec::new())
            },
            |_| Err(crate::cover::CoverError::Decode),
            notifier.clone(),
        );
        assert_eq!(tasks.len(), 1);

        tasks.abort_all();
        while tasks.join_next().await.is_some() {}
    });

    assert_eq!(fetches.load(Ordering::SeqCst), 0);
    assert!(in_flight.lock().expect("lock").is_empty());
}

#[test]
fn a_fetch_after_completion_spawns_a_new_task() {
    let runtime = runtime();
    let (tx, _rx) = channel();
    let notifier = Notifier {
        tx,
        slot: WakeSlot::default(),
    };
    let url = CoverUrl::from_wire("https://i.scdn.co/image/abc").expect("a cover");
    let in_flight = Arc::new(Mutex::new(HashSet::new()));
    let fetches = Arc::new(AtomicUsize::new(0));

    runtime.block_on(async {
        let permits = Arc::new(Semaphore::new(MAX_CONCURRENT_COVERS));
        for _ in 0..2 {
            let mut tasks = JoinSet::new();
            let counting = Arc::clone(&fetches);
            spawn_cover(
                &mut tasks,
                Arc::clone(&permits),
                Arc::clone(&in_flight),
                url.clone(),
                async move {
                    counting.fetch_add(1, Ordering::SeqCst);
                    Ok(include_bytes!("../../../tests/fixtures/cover.jpg").to_vec())
                },
                |bytes| crate::decode::decode(&bytes),
                notifier.clone(),
            );
            while tasks.join_next().await.is_some() {}
        }
    });

    // Nothing completed is retained, so the second request is a real fetch.
    assert_eq!(fetches.load(Ordering::SeqCst), 2);
    assert!(in_flight.lock().expect("lock").is_empty());
}

#[test]
fn an_aborted_fetch_clears_the_in_flight_set_promptly() {
    let runtime = runtime();
    let (tx, _rx) = channel();
    let notifier = Notifier {
        tx,
        slot: WakeSlot::default(),
    };
    let url = CoverUrl::from_wire("https://i.scdn.co/image/abc").expect("a cover");
    let in_flight = Arc::new(Mutex::new(HashSet::new()));

    let started = std::time::Instant::now();
    runtime.block_on(async {
        let mut tasks = JoinSet::new();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        spawn_cover(
            &mut tasks,
            Arc::new(Semaphore::new(MAX_CONCURRENT_COVERS)),
            Arc::clone(&in_flight),
            url.clone(),
            async move {
                let _ = started_tx.send(());
                std::future::pending().await
            },
            |_| Err(crate::cover::CoverError::Decode),
            notifier,
        );
        started_rx.await.expect("the fetch started");
        tasks.abort_all();
        while tasks.join_next().await.is_some() {}
    });

    assert!(started.elapsed() < Duration::from_secs(1));
    assert!(in_flight.lock().expect("lock").is_empty());
}

#[test]
fn a_slow_decode_does_not_hold_up_the_runtime_drop() {
    let runtime = runtime();
    let (tx, _rx) = channel();
    let notifier = Notifier {
        tx,
        slot: WakeSlot::default(),
    };
    let url = CoverUrl::from_wire("https://i.scdn.co/image/abc").expect("a cover");
    let in_flight = Arc::new(Mutex::new(HashSet::new()));
    let (decode_started_tx, decode_started_rx) = channel();

    runtime.block_on(async {
        let mut tasks = JoinSet::new();
        spawn_cover(
            &mut tasks,
            Arc::new(Semaphore::new(MAX_CONCURRENT_COVERS)),
            Arc::clone(&in_flight),
            url.clone(),
            async { Ok(include_bytes!("../../../tests/fixtures/cover.jpg").to_vec()) },
            move |_bytes| {
                let _ = decode_started_tx.send(());
                std::thread::sleep(Duration::from_millis(400));
                Ok(CoverImage::new(1, 1, vec![0, 0, 0, 255]).expect("one pixel"))
            },
            notifier,
        );
        // The task is parked on its oneshot while the thread sleeps.
        decode_started_rx.recv().expect("the decode started");
        tasks.abort_all();
    });

    let dropped = std::time::Instant::now();
    drop(runtime);
    assert!(
        dropped.elapsed() < Duration::from_millis(300),
        "dropping the runtime must not join the detached decode thread"
    );
}
