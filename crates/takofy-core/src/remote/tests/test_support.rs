//! Fixtures shared by the remote module's tests.

use super::super::*;

use crate::context::PageTrack;
use librespot::core::{SpotifyId, SpotifyUri};
use librespot::protocol::metadata::image::Size;
use librespot::protocol::metadata::{Album, Artist, Image, ImageGroup, Track as TrackMessage};
use std::time::Duration;

pub(super) const TRACK_ID: &str = "4uLU6hMCjMI75M1A2tKUQC";
pub(super) const EPISODE_ID: &str = "6rqhFgbbKwnb9MLmUQDhG6";

pub(super) fn first_track() -> SpotifyUri {
    SpotifyUri::Track {
        id: SpotifyId::from_base62(TRACK_ID).expect("a track id"),
    }
}

pub(super) fn local() -> SpotifyUri {
    SpotifyUri::Local {
        artist: "Artist".to_owned(),
        album_title: "Album".to_owned(),
        track_title: "Title".to_owned(),
        duration: Duration::from_secs(3),
    }
}

pub(super) fn owned(uris: &[&str]) -> Vec<String> {
    uris.iter().map(|uri| (*uri).to_owned()).collect()
}

pub(super) fn items(uris: &[&str]) -> Vec<PageTrack> {
    uris.iter()
        .map(|uri| PageTrack {
            uri: (*uri).to_owned(),
            added_at: None,
        })
        .collect()
}

pub(super) fn named(pairs: &[(&str, &str, &str)]) -> Labels {
    pairs
        .iter()
        .map(|(uri, title, artists)| {
            (
                (*uri).to_owned(),
                TrackLabel {
                    title: (*title).to_owned(),
                    artists: (*artists).to_owned(),
                    album: None,
                    album_uri: None,
                    artist_uris: Vec::new(),
                    artist_names: Vec::new(),
                    duration_ms: None,
                    cover: None,
                },
            )
        })
        .collect()
}

pub(super) fn wire_track(album: Option<&str>, duration: Option<i32>) -> TrackMessage {
    TrackMessage {
        name: Some("One".to_owned()),
        artist: vec![Artist {
            name: Some("Band".to_owned()),
            ..Default::default()
        }],
        album: album
            .map(|name| Album {
                name: Some(name.to_owned()),
                ..Default::default()
            })
            .into(),
        duration,
        ..Default::default()
    }
}

/// A 16-byte gid, the only length `SpotifyId::from_raw` accepts.
pub(super) fn gid(byte: u8) -> Vec<u8> {
    vec![byte; 16]
}

pub(super) fn show_uri(byte: u8) -> String {
    SpotifyUri::Show {
        id: SpotifyId::from_raw(&gid(byte)).expect("a 16-byte id"),
    }
    .to_uri()
    .expect("a uri")
}

pub(super) fn grouped_cover(size: Size, file_id: &[u8]) -> ImageGroup {
    let mut image = Image::new();
    image.set_size(size);
    image.set_file_id(file_id.to_vec());
    ImageGroup {
        image: vec![image],
        ..Default::default()
    }
}
