use egui::{Context, vec2};

use super::super::*;
use crate::cover::Cover;
use crate::player::Transport;

pub(super) fn context() -> Context {
    Context::default()
}

pub(super) fn entry(title: &str) -> TrackEntry {
    TrackEntry {
        uri: format!("u{title}"),
        title: title.to_owned(),
        artists: "a".to_owned(),
        album: None,
        album_uri: None,
        artist_uris: Vec::new(),
        artist_names: Vec::new(),
        added_at: None,
        duration_ms: Some(1_000),
        named: true,
        cover: None,
    }
}

pub(super) fn lifted() -> queue_row::Lifted {
    queue_row::Row {
        index: 0,
        title: "A Title",
        artist: "A Band",
        duration: None,
        cover: Cover::Missing,
        title_target: None,
        artist_target: None,
        look: Look::default(),
        refocus: false,
    }
    .lifted()
}

pub(super) fn ghost() -> Ghost {
    Ghost::Queue(lifted())
}

pub(super) fn carry(source: Source, rows: Vec<usize>) -> Carry {
    Carry {
        source,
        rows,
        entries: Vec::new(),
        size: vec2(100.0, 44.0),
        ghost: ghost(),
        playlist: None,
    }
}

pub(super) fn queue_of(count: usize, current: usize) -> Queue {
    let tracks: Vec<TrackEntry> = (0..count).map(|index| entry(&index.to_string())).collect();
    let mut queue = Queue::default();
    queue.start(&tracks, current, None, &mut Transport::default());
    queue
}
