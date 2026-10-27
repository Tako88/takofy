use takofy_model::TrackEntry;

use super::super::*;

pub(super) fn track(uri: &str) -> TrackEntry {
    TrackEntry {
        uri: uri.to_owned(),
        title: uri.to_owned(),
        artists: String::new(),
        album: None,
        album_uri: None,
        artist_uris: Vec::new(),
        artist_names: Vec::new(),
        added_at: None,
        duration_ms: None,
        named: true,
        cover: None,
    }
}

pub(super) fn track_by(uri: &str, artists: &str) -> TrackEntry {
    TrackEntry {
        artists: artists.to_owned(),
        ..track(uri)
    }
}

/// The upcoming entries as `(index, uri)` pairs, which is what every
/// `upcoming` test asserts on.
pub(super) fn upcoming(queue: &Queue) -> Vec<(usize, String)> {
    queue
        .upcoming()
        .map(|(index, entry)| (index, entry.uri.clone()))
        .collect()
}

pub(super) fn tracks(count: usize) -> Vec<TrackEntry> {
    (0..count).map(|i| track(&format!("t{i}"))).collect()
}

/// A queue started on `count` tracks, with the current entry at `index`.
pub(super) fn started(count: usize, index: usize) -> Queue {
    let tracks = tracks(count);
    let mut queue = Queue::default();
    let mut transport = Transport::default();
    queue.start(&tracks, index, None, &mut transport);
    queue
}

/// The whole queue as uris, the current entry included.
pub(super) fn order(queue: &Queue) -> Vec<String> {
    queue.tracks.iter().map(|entry| entry.uri.clone()).collect()
}

pub(super) fn uris(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

/// The queue's entries as their artists, which tells duplicates apart.
pub(super) fn artists(queue: &Queue) -> Vec<String> {
    queue
        .tracks
        .iter()
        .map(|entry| entry.artists.clone())
        .collect()
}

/// The rows the queue would show as picked.
pub(super) fn picked(queue: &Queue) -> Vec<usize> {
    (0..queue.tracks.len())
        .filter(|row| queue.selected(*row))
        .collect()
}

pub(super) fn play(uri: &str) -> Command {
    Command::Play {
        uri: uri.to_owned(),
    }
}

pub(super) fn transport_update(update: TransportUpdate) -> Update {
    Update::Transport(update)
}

/// The pipeline `drain` runs: the transport folds first, then the queue.
pub(super) fn feed(
    queue: &mut Queue,
    transport: &mut Transport,
    update: TransportUpdate,
) -> Option<Command> {
    let update = transport_update(update);
    transport.apply(&update);
    queue.apply(&update, transport)
}

/// The events a real load emits after a `Play`, with a failure at the end.
pub(super) fn skip(queue: &mut Queue, transport: &mut Transport) -> Option<Command> {
    feed(queue, transport, TransportUpdate::Loading);
    feed(queue, transport, TransportUpdate::Unavailable)
}

pub(super) fn clear_guard(queue: &mut Queue, transport: &mut Transport) {
    feed(queue, transport, TransportUpdate::Loading);
}

pub(super) fn end(queue: &mut Queue, transport: &mut Transport) -> Option<Command> {
    feed(queue, transport, TransportUpdate::Ended)
}

pub(super) fn seekable(queue: &mut Queue, transport: &mut Transport, position_ms: u32) {
    feed(
        queue,
        transport,
        TransportUpdate::Duration {
            duration_ms: 222_000,
        },
    );
    feed(
        queue,
        transport,
        TransportUpdate::Position {
            position_ms,
            paused: false,
        },
    );
}

/// A queue holding `count` rows with nothing current, so every row and
/// boundary is editable.
pub(super) fn armed(count: usize) -> Queue {
    let mut queue = Queue::default();
    queue.append(tracks(count));
    queue
}

/// Every non-empty pick set over `len` rows, each ascending.
pub(super) fn subsets(len: usize) -> Vec<Vec<usize>> {
    (1..(1usize << len) as u64)
        .map(|mask| {
            (0..len)
                .filter(|row| mask >> row & 1 == 1)
                .collect::<Vec<_>>()
        })
        .collect()
}

/// A hand-over landing on `rows` at `index`, playing, at the first
/// activation epoch.
pub(super) fn landing(rows: &[&str], index: usize) -> Landing {
    Landing {
        rows: rows.iter().map(|uri| (*uri).to_owned()).collect(),
        index,
        position_ms: 7_000,
        playing: true,
        context: Some("spotify:playlist:p".to_owned()),
        // A landing that names no mode, so the queue keeps the modes it is
        // in unless a test overrides one.
        shuffle: None,
        repeat_context: None,
        repeat_track: None,
        epoch: 1,
    }
}
