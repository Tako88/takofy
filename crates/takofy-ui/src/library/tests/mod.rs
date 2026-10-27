use takofy_model::{
    AddedAt, AlbumDetail, ArtistDetail, ArtistPortrait, CoverSet, CoverUrl, Episode, FollowedShow,
    Move, PlaylistAccess, PlaylistCover, PlaylistEntry, PublishedAt, ReleaseKind, Removal,
    Revision, Show, ShowDetail, TrackEntry, TrackLyrics,
};

use super::*;
use crate::discography::Chip;

use crate::library_filter::RootKind;
use crate::lyrics::Tab;
use crate::message::{DeviceUpdate, TransportUpdate};
use crate::player::NowPlaying;
use crate::sidebar_kind::SidebarKind;

fn playlist(uri: &str, name: &str) -> PlaylistEntry {
    PlaylistEntry {
        composed: Vec::new(),
        uri: uri.to_owned(),
        name: name.to_owned(),
        named: true,
        cover: None,
        description: None,
        owner: None,
        tracks: None,
        access: None,
    }
}

fn track(uri: &str) -> TrackEntry {
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

/// The URIs an add would carry, in the order the rows were picked.
fn pick(uris: &[&str]) -> Vec<String> {
    uris.iter().map(|uri| (*uri).to_owned()).collect()
}

fn tracks_for(playlist: &str, tracks: Vec<TrackEntry>) -> TracksFor {
    TracksFor {
        playlist: playlist.to_owned(),
        tracks,
        access: None,
        notice: None,
    }
}

fn revision(byte: u8) -> Revision {
    Revision::from_wire(&[byte, 0x0a]).expect("a revision")
}

fn access(byte: u8, editable: bool) -> Option<PlaylistAccess> {
    Some(PlaylistAccess {
        revision: revision(byte),
        editable,
    })
}

/// A playlist page showing `uris`, with `access` as what it may change.
fn page_with(playlist: &str, uris: &[&str], access: Option<PlaylistAccess>) -> Library {
    let mut library = Library::default();
    library.select(playlist);
    library.apply(&Update::Tracks {
        playlist: playlist.to_owned(),
        tracks: uris.iter().map(|uri| track(uri)).collect(),
        cover: None,
        access,
    });
    library
}

fn album(uri: &str) -> Page {
    Page::Album(uri.to_owned())
}

fn artist_command(uri: &str) -> Command {
    Command::LoadArtist {
        artist: uri.to_owned(),
    }
}

fn artist_detail(uri: &str) -> ArtistDetail {
    ArtistDetail {
        uri: uri.to_owned(),
        name: "Vela".to_owned(),
        portrait: None,
        top_tracks: vec![track("top")],
        releases: vec![release("spotify:album:r1")],
        related: Vec::new(),
    }
}

fn release(uri: &str) -> takofy_model::Release {
    takofy_model::Release {
        uri: uri.to_owned(),
        name: uri.to_owned(),
        kind: takofy_model::ReleaseKind::Album,
        year: None,
        cover: None,
        tracks: vec![uri.to_owned()],
    }
}

fn album_detail(uri: &str) -> AlbumDetail {
    AlbumDetail {
        uri: uri.to_owned(),
        name: "Record".to_owned(),
        artists: "Band".to_owned(),
        artist_uri: None,
        year: Some(1997),
        cover: None,
        tracks: vec![track("t")],
    }
}

fn artist(uri: &str) -> Page {
    Page::Artist(uri.to_owned())
}

fn show(uri: &str) -> Page {
    Page::Show(uri.to_owned())
}

fn episode(uri: &str) -> Episode {
    Episode {
        uri: uri.to_owned(),
        title: uri.to_owned(),
        show: "Lateral".to_owned(),
        show_uri: Some("spotify:show:a".to_owned()),
        description: None,
        published: None,
        duration_ms: None,
        cover: None,
        named: true,
    }
}

fn show_detail(uri: &str) -> ShowDetail {
    ShowDetail {
        show: Show {
            uri: uri.to_owned(),
            name: "Show".to_owned(),
            publisher: "Lateral".to_owned(),
            description: String::new(),
            cover: None,
        },
        episodes: vec![episode("spotify:episode:e")],
    }
}

fn followed_show(uri: &str) -> FollowedShow {
    FollowedShow {
        uri: uri.to_owned(),
        name: uri.to_owned(),
        named: true,
        publisher: None,
        cover: None,
        episodes: None,
    }
}

fn followed_artist(uri: &str) -> FollowedArtist {
    FollowedArtist {
        uri: uri.to_owned(),
        name: uri.to_owned(),
        named: true,
        portrait: None,
        releases: None,
    }
}

fn saved_album(uri: &str) -> SavedAlbum {
    SavedAlbum {
        uri: uri.to_owned(),
        name: uri.to_owned(),
        named: true,
        artists: String::new(),
        year: None,
        cover: None,
        tracks: None,
    }
}

fn cover_url(tail: &str) -> CoverUrl {
    CoverUrl::from_wire(&format!("https://i.scdn.co/image/{tail}")).expect("a cover url")
}

fn playlist_with_cover(uri: &str, name: &str, cover: CoverUrl) -> PlaylistEntry {
    covered(uri, name, PlaylistCover::derived(CoverSet::single(cover)))
}

fn covered(uri: &str, name: &str, cover: PlaylistCover) -> PlaylistEntry {
    PlaylistEntry {
        composed: Vec::new(),
        cover: Some(cover),
        ..playlist(uri, name)
    }
}

/// The cover the sidebar row for `uri` holds.
fn entry_cover(library: &Library, uri: &str) -> Option<PlaylistCover> {
    sidebar_entry(library, uri).and_then(|entry| entry.cover)
}

/// A library whose open page is `a`, showing `uris`, with `cover` on `a`'s
/// sidebar row.
fn covered_page(cover: &CoverUrl, uris: &[&str]) -> Library {
    let mut library = page_with("a", uris, access(0x01, true));
    library.playlists = Load::Ready(vec![with_access(
        playlist_with_cover("a", "A", cover.clone()),
        0x07,
        true,
        Some(uris.len()),
    )]);
    library
}

/// Whether the band would treat the open page as settled this frame.
fn settled(library: &Library) -> bool {
    !matches!(library.tracks, Load::Loading)
}

/// A library whose open page is `a` and whose rows were dropped by an add
/// that landed while `a`'s own read was still in flight.
///
/// The add is begun while another playlist is on screen, because the open
/// playlist is not an add target.
fn page_dropped_by_an_add(cover: Option<&CoverUrl>) -> Library {
    let mut library = sidebar_library(&["a", "b"], 0x07, true, Some(1));
    let listed = match cover {
        Some(cover) => playlist_with_cover("a", "A", cover.clone()),
        None => playlist("a", "A"),
    };
    library.playlists = Load::Ready(vec![
        with_access(listed, 0x07, true, Some(1)),
        with_access(playlist("b", "B"), 0x07, true, Some(1)),
    ]);
    library.select("b");
    assert!(
        library.add_tracks(0, &pick(&["spotify:track:x"])).is_some(),
        "arrange: the add must be in flight"
    );
    library.select("a");
    library.apply(&Update::TrackAdded {
        playlist: "a".to_owned(),
        revision: revision(0x08),
    });
    library
}

/// The entry with what the listing read said about writing to it.
fn with_access(
    entry: PlaylistEntry,
    byte: u8,
    editable: bool,
    tracks: Option<usize>,
) -> PlaylistEntry {
    PlaylistEntry {
        composed: Vec::new(),
        access: Some(PlaylistAccess {
            revision: revision(byte),
            editable,
        }),
        tracks,
        ..entry
    }
}

fn rootlist_access(byte: u8, editable: bool) -> PlaylistAccess {
    PlaylistAccess {
        revision: revision(byte),
        editable,
    }
}

/// A library whose sidebar holds `names`, each read at `byte`.
fn sidebar_library(names: &[&str], byte: u8, editable: bool, tracks: Option<usize>) -> Library {
    let mut library = Library::default();
    let entries = names
        .iter()
        .map(|name| with_access(playlist(name, name), byte, editable, tracks))
        .collect();
    library.apply(&Update::Playlists(entries));
    library.apply(&Update::RootlistAccess(rootlist_access(byte, editable)));
    library
}

fn sidebar_order(library: &Library) -> Vec<String> {
    match &library.playlists {
        Load::Ready(playlists) => playlists.iter().map(|entry| entry.uri.clone()).collect(),
        Load::Idle | Load::Loading | Load::Failed(_) => Vec::new(),
    }
}

fn sidebar_entry(library: &Library, uri: &str) -> Option<PlaylistEntry> {
    match &library.playlists {
        Load::Ready(playlists) => playlists.iter().find(|entry| entry.uri == uri).cloned(),
        Load::Idle | Load::Loading | Load::Failed(_) => None,
    }
}

fn names(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

/// A one-row drag, with the write's landed range reduced to its start.
fn move_one(
    library: &mut Library,
    playlist: &str,
    from: usize,
    boundary: usize,
    carried: &TrackEntry,
) -> Option<(Command, usize)> {
    library
        .move_tracks(playlist, &[from], boundary, std::slice::from_ref(carried))
        .map(|(command, landed)| (command, landed.start))
}

fn moved(uris: &[&str]) -> Vec<TrackEntry> {
    uris.iter().map(|uri| track(uri)).collect()
}

fn ready(playlist: &str, uris: &[&str], access: Option<PlaylistAccess>) -> Load<TracksFor> {
    Load::Ready(TracksFor {
        playlist: playlist.to_owned(),
        tracks: moved(uris),
        access,
        notice: None,
    })
}

/// A sidebar holding `names`, with the head's field open on `name`.
fn naming_library(names: &[&str], name: &str) -> Library {
    let mut library = sidebar_library(names, 0x07, true, Some(3));
    library.naming.open();
    library.naming.edit().push_str(name);
    library
}

/// The row a create's reply hands the sidebar.
fn created(uri: &str, name: &str, access: Option<PlaylistAccess>) -> PlaylistEntry {
    PlaylistEntry {
        composed: Vec::new(),
        uri: uri.to_owned(),
        name: name.to_owned(),
        named: true,
        cover: None,
        description: None,
        tracks: Some(0),
        owner: None,
        access,
    }
}

/// A sidebar row as a menu opened on it describes it.
fn item(kind: SidebarKind, uri: &str) -> LibraryItem {
    LibraryItem {
        kind,
        uri: uri.to_owned(),
        name: uri.to_owned(),
    }
}

/// A library whose four lists are ready and hold one thing each.
fn saved_library() -> Library {
    let mut library = Library::default();
    library.apply(&Update::Playlists(Vec::new()));
    library.apply(&Update::FollowedShows(vec![followed_show(
        "spotify:show:s",
    )]));
    library.apply(&Update::FollowedArtists(vec![followed_artist(
        "spotify:artist:a",
    )]));
    library.apply(&Update::SavedAlbums(vec![saved_album("spotify:album:r")]));
    library
}

fn rename_pending(library: &mut Library, uri: &str, surface: crate::naming::Surface) {
    library.renaming.open(uri, uri, surface);
    library.sidebar_pending = Some(SidebarPending::Rename {
        playlist: uri.to_owned(),
    });
}

mod add;
mod album;
mod artist;
mod band;
mod chips;
mod covers;
mod create;
mod filters;
mod followed;
mod history;
mod r#move;
mod remove;
mod rename;
mod reorder;
mod save;
mod search;
mod select;
mod shelf;
mod show;
mod writes;
