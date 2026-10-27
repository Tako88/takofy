use std::sync::Arc;
use std::time::Duration;

use takofy_model::{Report, RequestId, Status, TrackEntry};

use super::super::*;

use crate::presence::Presence;
use crate::registry::{Ack, Registry};
use crate::testing::{FakeResponder, FakeWire, Pump, recv, start_pump, view_of_event};
use crate::transfer::ContextSource;
use crate::watch::{Gate, Shared};

pub(super) const TRACK_1: &str = "spotify:track:4uLU6hMCjMI75M1A2tKUQ1";
pub(super) const TRACK_2: &str = "spotify:track:4uLU6hMCjMI75M1A2tKUQ2";
pub(super) const TRACK_3: &str = "spotify:track:4uLU6hMCjMI75M1A2tKUQ3";
pub(super) const PLAYLIST: &str = "spotify:playlist:37i9dQZF1DXcBWIGoYBM5M";

/// The phone's snapshot: five seconds in, two seconds old when it lands.
pub(super) const POSITION_MS: u32 = 5_000;
pub(super) const SNAPSHOT_MS: i64 = 1_000_000;
pub(super) const NOW_MS: i64 = SNAPSHOT_MS + 2_000;

/// The instant the account put on the accept's own reply.
pub(super) const REPLY_MS: i64 = 5_000;

pub(super) fn timing() -> Timing {
    Timing {
        id_wait: Duration::from_millis(20),
        withdraw: Duration::from_millis(50),
        gate_wait: Duration::from_millis(30),
        answer: Duration::from_millis(50),
        resolve: Duration::from_millis(20),
        put: Duration::from_millis(50),
        hand_over: Duration::from_millis(50),
        release: Duration::from_millis(20),
        grace: Duration::from_millis(50),
        debounce: Duration::from_millis(20),
    }
}

/// A wire that never cuts a held put, and a request that never expires.
pub(super) fn patient() -> Timing {
    Timing {
        put: Duration::from_secs(2),
        gate_wait: Duration::from_secs(2),
        answer: Duration::from_secs(30),
        ..timing()
    }
}

pub(super) fn plan() -> TransferPlan {
    TransferPlan {
        current: TRACK_2.to_owned(),
        from_queue: false,
        queue: Vec::new(),
        associated: None,
        pages: vec![TRACK_1.to_owned(), TRACK_2.to_owned(), TRACK_3.to_owned()],
        start_index: None,
        context: Some(PLAYLIST.to_owned()),
        source: ContextSource::Pages,
        position_ms: Some(POSITION_MS),
        timestamp_ms: Some(SNAPSHOT_MS),
        paused: false,
        shuffle: Some(false),
        repeat_context: Some(false),
        repeat_track: Some(false),
    }
}

pub(super) fn resolved() -> Resolution {
    Resolution::Resolved(vec![
        TRACK_1.to_owned(),
        TRACK_2.to_owned(),
        TRACK_3.to_owned(),
    ])
}

/// What this window is playing, as the window itself would publish it.
pub(super) fn report(status: Status) -> Report {
    Report {
        status,
        track: Some(TrackEntry {
            uri: TRACK_1.to_owned(),
            title: "one".to_owned(),
            artists: String::new(),
            album: None,
            album_uri: None,
            artist_uris: Vec::new(),
            artist_names: Vec::new(),
            added_at: None,
            duration_ms: None,
            named: true,
            cover: None,
        }),
        prev: Vec::new(),
        next: Vec::new(),
        context: Some(PLAYLIST.to_owned()),
        queue_revision: String::new(),
        options: None,
        position_ms: POSITION_MS,
        duration_ms: Some(200_000),
    }
}

pub(super) fn ack() -> Ack {
    Ack {
        message_id: 7,
        sender: "phone".to_owned(),
    }
}

/// Registers one request whose answer lands in the wire's own ordered log.
pub(super) fn pending(shared: &Shared, wire: &FakeWire, id: RequestId) {
    shared
        .registry
        .insert(id, ack(), Box::new(FakeResponder::new(wire.log.clone())));
}

/// One accept path's world: the wire, the gate holding it, and the bundle
/// every call reads.
pub(super) struct World {
    pub(super) wire: Arc<FakeWire>,
    pub(super) gate: Arc<Gate>,
    pub(super) shared: Arc<Shared>,
}

impl World {
    pub(super) fn new(timing: Timing) -> Self {
        let wire = Arc::new(FakeWire::default());
        wire.set_connection_id("connection-1");
        wire.set_now(NOW_MS);
        let presence = Arc::new(Presence::default());
        // Only a device that can play is ever handed the sound.
        presence.set_playable(true);
        let shared = Shared::detached(
            presence,
            Arc::new(crate::latest::Latest::default()),
            Arc::new(Registry::new(timing)),
        );
        Self {
            wire,
            gate: Arc::default(),
            shared,
        }
    }

    pub(super) fn pending(&self, id: RequestId) {
        pending(&self.shared, &self.wire, id);
    }

    pub(super) async fn accept(
        &self,
        id: RequestId,
        timing: Timing,
    ) -> Result<Landing, ConnectError> {
        accept(
            &*self.wire,
            &self.gate,
            &self.shared,
            id,
            &plan(),
            resolved(),
            timing,
        )
        .await
    }

    pub(super) async fn pick(&self, to: &str) -> Result<(), ConnectError> {
        self.pick_with(to, timing()).await
    }

    pub(super) async fn pick_with(&self, to: &str, timing: Timing) -> Result<(), ConnectError> {
        pick(&*self.wire, &self.gate, &self.shared, to, timing).await
    }
}
/// A pump whose stage is playable, with one held request already answered.
pub(super) async fn a_held_pump(timing: Timing) -> (Arc<FakeWire>, Arc<Gate>, Pump) {
    let wire = Arc::new(FakeWire::default());
    let gate = Arc::new(Gate::default());
    let mut pump = start_pump(Arc::clone(&wire), Arc::clone(&gate), timing);
    pump.shared.presence.set_playable(true);
    pump.ids.send(Ok("connection-1".to_owned())).expect("id");
    let _ = view_of_event(recv(&mut pump.events).await);
    (wire, gate, pump)
}
