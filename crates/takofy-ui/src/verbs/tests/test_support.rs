use super::super::*;

pub(super) fn entry(uri: &str) -> TrackEntry {
    TrackEntry {
        uri: uri.to_owned(),
        title: format!("title of {uri}"),
        artists: "A Band".to_owned(),
        album: Some("A Record".to_owned()),
        album_uri: Some("spotify:album:a".to_owned()),
        artist_uris: vec!["spotify:artist:b".to_owned()],
        artist_names: vec!["A Band".to_owned()],
        added_at: None,
        duration_ms: None,
        named: true,
        cover: None,
    }
}

pub(super) fn labels(items: &[Item]) -> Vec<&str> {
    items.iter().map(|item| item.label.as_str()).collect()
}

pub(super) fn verbs(items: &[Item]) -> Vec<Verb> {
    items.iter().map(|item| item.verb).collect()
}

pub(super) fn playlist_listing() -> Listing {
    Listing::Playlist {
        uri: "spotify:playlist:a".to_owned(),
        list: egui::Id::unique("playlist-tracks"),
    }
}

pub(super) fn tracks(uris: &[&str]) -> Vec<TrackEntry> {
    uris.iter().map(|uri| entry(uri)).collect()
}

/// The URIs a pick holds, so a test names only their ids.
pub(super) fn pick_of(ids: &[&str]) -> Vec<String> {
    ids.iter().map(|id| format!("spotify:track:{id}")).collect()
}
