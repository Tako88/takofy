//! Cover requests: the drains a session feeds, and the fetch tasks it answers.

use std::collections::HashSet;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use takofy_model::CoverUrl;
use takofy_ui::message::Update;
use tokio::sync::Semaphore;
use tokio::sync::mpsc::UnboundedSender;
use tokio::task::JoinSet;

use super::wake::Notifier;
use crate::cover::{fetch_cover, spawn_cover};
use crate::playhead::Playhead;

/// What the drains need to know about the session that feeds them.
#[derive(Clone)]
pub(super) struct Drains {
    /// Set when a session is retired, so a drain from the old one goes quiet
    /// even as its streams end.
    pub(super) retired: Arc<AtomicBool>,
    pub(super) playhead: Playhead,
    pub(super) signal: UnboundedSender<u64>,
}

impl Drains {
    pub(super) fn new(signal: UnboundedSender<u64>) -> Self {
        Self {
            retired: Arc::new(AtomicBool::new(false)),
            playhead: Playhead::new(),
            signal,
        }
    }
}

/// Removes finished fetch tasks so their cells do not accumulate over a
/// session, and logs a panic that would otherwise go unobserved.
pub(super) fn reap_finished_covers(tasks: &mut JoinSet<()>) {
    while let Some(joined) = tasks.try_join_next() {
        if let Err(error) = joined {
            log::warn!("cover task ended: {error}");
        }
    }
}

/// Answers one cover request. Every request ends in exactly one update: the
/// fetch is spawned, or a failure is sent straight back, so the window's
/// in-flight entry always resolves.
pub(super) fn handle_fetch(
    connected: bool,
    client: Option<&reqwest::Client>,
    tasks: &mut JoinSet<()>,
    permits: Arc<Semaphore>,
    in_flight: Arc<Mutex<HashSet<CoverUrl>>>,
    url: CoverUrl,
    notifier: Notifier,
) {
    let (true, Some(client)) = (connected, client) else {
        // A signed-out window has no cover to want, and a client that failed
        // to build can never serve one; both resolve the request now.
        notifier.send(Update::CoverFailed { url });
        return;
    };
    let client = client.clone();
    let requested = url.clone();
    let fetch = async move { fetch_cover(&client, &requested).await };
    spawn_cover(
        tasks,
        permits,
        in_flight,
        url,
        fetch,
        |bytes| crate::decode::decode(&bytes),
        notifier,
    );
}
