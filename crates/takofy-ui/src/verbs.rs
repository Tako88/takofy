//! What a menu's verbs say, and what each one does with the rows it was opened on.
//!
//! Pure: a verb's label, its presence and its outcome are all decided from the
//! snapshot the menu took when it opened. Nothing here fetches, writes or draws.

use std::collections::HashSet;

use takofy_model::TrackEntry;
use takofy_model::home::{SeedKind, StationSeed};
use takofy_model::uri::{UriKind, parse};

use crate::likes::Standing;
use crate::menu::{Door, Item, Kind, LibraryItem, Listing, Request, Verb};
use crate::route::Page;
use crate::sidebar_kind::SidebarKind;

/// What applying a verb does, once the shell has the frame's command to spare.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The station this track, album, artist or playlist seeds.
    StartRadio { seed: StationSeed },
    /// These rows, in the order the menu took them.
    Queue(Vec<TrackEntry>),
    /// One URL per line, no trailing newline.
    Copy(String),
    /// The page a go-to opens.
    Open(Page),
    /// The list's rows leave it. Which list, and which rows, is the menu's cut.
    Remove,
    /// These tracks join the liked set, or leave it.
    Like { tracks: Vec<String>, liked: bool },
    /// The sidebar's name field opens, carrying these tracks into the new list.
    NewPlaylist { pick: Vec<String> },
    /// These tracks go to the playlist the submenu's chosen row names.
    AddToPlaylist { tracks: Vec<String> },
    /// The sidebar row leaves the library.
    Unlink(LibraryItem),
    /// The item enters the library: a link for a playlist, a collection write for
    /// an album or a show.
    Save(LibraryItem),
    /// An artist is followed or unfollowed.
    Follow { artist: String, followed: bool },
    /// The sidebar row's name field opens.
    Rename(LibraryItem),
}

/// The web address a URI stands for, or `None` when it has none.
///
/// Only the kinds a row or the library can be are mapped. A local file has no
/// page, and a web address needs one path segment, so an id carrying a colon
/// has none either.
pub fn spotify_link(uri: &str) -> Option<String> {
    let parsed = parse(uri)?;
    if parsed.id.contains(':') {
        return None;
    }
    let kind = match parsed.kind {
        UriKind::Track => "track",
        UriKind::Episode => "episode",
        UriKind::Playlist => "playlist",
        UriKind::Album => "album",
        UriKind::Artist => "artist",
        UriKind::Show => "show",
        UriKind::Local => return None,
    };
    Some(format!("https://open.spotify.com/{kind}/{}", parsed.id))
}

/// What a library row may do about its place in the library.
///
/// Both are absent rather than greyed, and both come from the library's own
/// state: a row the user cannot write offers neither.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LibraryRights {
    pub renamable: bool,
    pub removable: bool,
    /// Whether the item is in the library, or `None` when no list can say.
    ///
    /// A band offers the heart's verb from this, so a page whose kind the
    /// library never reads gets neither row.
    pub saved: Option<bool>,
}

/// The commands a sidebar row offers: its rename, its link, the library's write,
/// its radio, and its removal.
///
/// A command the library cannot write is absent rather than greyed, and the
/// removal is the destructive row, so it is drawn last. A sidebar row is one
/// thing rather than a pick, so nothing here carries a count.
pub fn library_items(row: &LibraryItem, rights: LibraryRights) -> Vec<Item> {
    let mut items = Vec::new();
    if rights.renamable {
        items.push(item(Verb::Rename, "Rename".to_owned()));
    }
    if spotify_link(&row.uri).is_some() {
        items.push(item(Verb::CopyLink, "Copy link".to_owned()));
    }
    if rights.saved == Some(false) {
        items.push(item(Verb::SaveToLibrary, "Save to your library".to_owned()));
    }
    if radio_seed(row).is_some() {
        items.push(item(Verb::StartRadio, "Start radio".to_owned()));
    }
    if rights.removable {
        items.push(Item {
            label: "Remove from library".to_owned(),
            kind: Kind::Destructive,
            key: Some("Del"),
            submenu: false,
            verb: Verb::RemoveFromLibrary,
        });
    }
    items
}

/// The seed a sidebar row's radio starts, or `None` when its URI names another
/// kind than the row's.
fn radio_seed(row: &LibraryItem) -> Option<StationSeed> {
    let (kind, named) = match row.kind {
        SidebarKind::Playlists => (SeedKind::Playlist, UriKind::Playlist),
        SidebarKind::Albums => (SeedKind::Album, UriKind::Album),
        SidebarKind::Artists => (SeedKind::Artist, UriKind::Artist),
        // No show seed is read on the wire.
        SidebarKind::Podcasts => return None,
    };
    let uri = parse(&row.uri)?;
    (uri.kind == named).then(|| StationSeed {
        kind,
        uri: row.uri.clone(),
    })
}

/// The request a row or a band posts for its own menu.
///
/// The listing carries the surface it came from: a row's menu acts on the
/// sidebar, a band's on the page it heads.
pub fn library_request(
    item: LibraryItem,
    door: Door,
    rights: LibraryRights,
    page: Option<Page>,
    surface: crate::naming::Surface,
) -> Request {
    let items = library_items(&item, rights);
    let listing = match surface {
        crate::naming::Surface::Row => Listing::Library(item),
        crate::naming::Surface::Band => Listing::Band(item),
    };
    Request {
        door,
        items,
        playlists: Vec::new(),
        targets: Vec::new(),
        rows: Vec::new(),
        listing,
        page,
    }
}

/// What a sidebar row's verb does with the row it was opened on.
///
/// `None` when the verb is not this row's. The removal carries the item, because
/// the list it belongs to and the revision it names are read from the library
/// when the verb is applied.
pub fn library_outcome(verb: Verb, row: &LibraryItem) -> Option<Outcome> {
    match verb {
        Verb::CopyLink => spotify_link(&row.uri).map(Outcome::Copy),
        Verb::RemoveFromLibrary => Some(Outcome::Unlink(row.clone())),
        Verb::SaveToLibrary => Some(Outcome::Save(row.clone())),
        Verb::Rename => Some(Outcome::Rename(row.clone())),
        Verb::Follow | Verb::Unfollow => Some(Outcome::Follow {
            artist: row.uri.clone(),
            followed: verb == Verb::Follow,
        }),
        _ => None,
    }
}

/// The request an artist's band posts for its own menu.
///
/// An artist's library entry is its follow, so the menu's rows are that write,
/// the radio and the link; nothing else in the vocabulary acts on an artist.
pub fn artist_request(
    artist: LibraryItem,
    door: Door,
    unfollowed: bool,
    page: Option<Page>,
) -> Request {
    let mut items = vec![item(
        if unfollowed {
            Verb::Follow
        } else {
            Verb::Unfollow
        },
        if unfollowed { "Follow" } else { "Unfollow" }.to_owned(),
    )];
    if radio_seed(&artist).is_some() {
        items.push(item(Verb::StartRadio, "Start radio".to_owned()));
    }
    if spotify_link(&artist.uri).is_some() {
        items.push(item(Verb::CopyLink, "Copy link".to_owned()));
    }
    Request {
        door,
        items,
        playlists: Vec::new(),
        targets: Vec::new(),
        rows: Vec::new(),
        listing: Listing::Band(artist),
        page,
    }
}

/// The heart's label and the menu row's, which say the action the state implies.
pub fn heart_label(liked: bool) -> &'static str {
    if liked {
        "Remove from your library"
    } else {
        "Save to your library"
    }
}

/// The rows' commands, in the sheet's order: queue, library, radio, album,
/// artist, link, remove.
///
/// A verb that cannot apply is absent rather than greyed, and a pick only keeps
/// the verbs that are single-valued across it. The library row is the pick's
/// standing made into a verb: a mixed pick offers only the constructive one,
/// because the destructive one would not apply to every row.
pub fn track_items(
    targets: &[TrackEntry],
    here: Option<&Page>,
    listing: &Listing,
    standing: Standing,
) -> Vec<Item> {
    let mut items = Vec::new();
    if targets.is_empty() {
        return items;
    }

    if matches!(listing, Listing::Page | Listing::Playlist { .. }) {
        items.push(item(Verb::AddToQueue, queue_label(targets)));
    }

    if let Some(item) = library_item(targets, standing) {
        items.push(item);
    }

    // A pick seeds a track's radio only when it is exactly one track.
    if let [only] = targets
        && only.is_track()
    {
        items.push(item(Verb::StartRadio, "Start radio".to_owned()));
    }

    if let Some(uri) = single(targets, |entry| entry.album_uri.as_deref())
        && here != Some(&Page::Album(uri.to_owned()))
    {
        items.push(item(Verb::GoToAlbum, "Go to album".to_owned()));
    }

    if let Some(uri) = single(targets, |entry| {
        entry.artist_uris.first().map(String::as_str)
    }) && here != Some(&Page::credit(uri))
    {
        items.push(item(Verb::GoToArtist, "Go to artist".to_owned()));
    }

    let links = links(targets);
    if !links.is_empty() {
        let label = if links.len() == 1 {
            "Copy link".to_owned()
        } else {
            format!("Copy {} links", links.len())
        };
        items.push(item(Verb::CopyLink, label));
    }

    // The submenu hangs off this row, so only a pick every playlist can take
    // gets one; a pick with an episode in it is refused whole.
    if let Some(tracks) = pick(targets) {
        items.push(Item {
            label: match tracks.len() {
                1 => "Add to playlist".to_owned(),
                many => format!("Add {many} tracks to playlist"),
            },
            kind: Kind::Plain,
            key: None,
            submenu: true,
            verb: Verb::AddToPlaylist,
        });
    }

    // The sheet's new-playlist row follows the link and sits above the band: the
    // one action a menu can always offer must not be buried. It acts on nothing
    // the menu holds, so a menu with no other row keeps none of its own.
    if !items.is_empty() {
        items.push(item(Verb::NewPlaylist, "New playlist…".to_owned()));
    }

    if let Some(label) = remove_label(targets, listing) {
        items.push(Item {
            label,
            kind: Kind::Destructive,
            key: Some("Del"),
            submenu: false,
            verb: Verb::Remove,
        });
    }

    items
}

/// What `verb` does with the snapshot it was offered on.
///
/// `None` when the verb cannot apply to these rows. The menu only offers verbs
/// that apply, so this is the guard against a caller that did not ask first.
pub fn outcome(verb: Verb, targets: &[TrackEntry]) -> Option<Outcome> {
    match verb {
        Verb::AddToQueue => (!targets.is_empty()).then(|| Outcome::Queue(targets.to_vec())),
        Verb::StartRadio => match targets {
            [only] if only.is_track() => Some(Outcome::StartRadio {
                seed: StationSeed {
                    kind: SeedKind::Song,
                    uri: only.uri.clone(),
                },
            }),
            _ => None,
        },
        Verb::CopyLink => {
            let links = links(targets);
            (!links.is_empty()).then(|| Outcome::Copy(links.join("\n")))
        }
        Verb::GoToAlbum => {
            let uri = single(targets, |entry| entry.album_uri.as_deref())?;
            Some(Outcome::Open(Page::Album(uri.to_owned())))
        }
        Verb::GoToArtist => {
            let uri = single(targets, |entry| {
                entry.artist_uris.first().map(String::as_str)
            })?;
            Some(Outcome::Open(Page::credit(uri)))
        }
        Verb::NewPlaylist => Some(Outcome::NewPlaylist {
            pick: pick(targets).unwrap_or_default(),
        }),
        Verb::AddToPlaylist => pick(targets).map(|tracks| Outcome::AddToPlaylist { tracks }),
        Verb::Remove => (!targets.is_empty()).then_some(Outcome::Remove),
        // A band's or a row's verbs act on the thing the menu was opened on, not
        // on a pick, so they have no meaning over track rows.
        Verb::RemoveFromLibrary
        | Verb::Rename
        | Verb::SaveToLibrary
        | Verb::Follow
        | Verb::Unfollow => None,
        Verb::Save => like_outcome(targets, true),
        Verb::Unsave => like_outcome(targets, false),
    }
}

/// The library row a pick's standing offers, or `None` when it offers none.
///
/// A mixed pick offers only Save: the count is the whole pick, but the write
/// filters to the rows it would change.
fn library_item(targets: &[TrackEntry], standing: Standing) -> Option<Item> {
    let verb = match standing {
        Standing::Unknown => return None,
        Standing::Unsaved | Standing::Mixed => Verb::Save,
        Standing::Saved => Verb::Unsave,
    };
    let label = match (targets.len(), verb) {
        (1, Verb::Save) => heart_label(false).to_owned(),
        (1, Verb::Unsave) => heart_label(true).to_owned(),
        (n, Verb::Save) => format!("Save {n} tracks to your library"),
        (n, Verb::Unsave) => format!("Remove {n} tracks from your library"),
        _ => return None,
    };
    Some(item(verb, label))
}

/// The write a save or an unsave asks for, naming every picked track once.
fn like_outcome(targets: &[TrackEntry], liked: bool) -> Option<Outcome> {
    if targets.is_empty() || !targets.iter().all(TrackEntry::is_track) {
        return None;
    }
    let mut seen = HashSet::new();
    let tracks: Vec<String> = targets
        .iter()
        .filter(|entry| seen.insert(entry.uri.clone()))
        .map(|entry| entry.uri.clone())
        .collect();
    Some(Outcome::Like { tracks, liked })
}

/// Whether a pick is one an add can take: every row a track, and at least one.
pub fn pickable(targets: &[TrackEntry]) -> bool {
    !targets.is_empty() && targets.iter().all(TrackEntry::is_track)
}

/// The tracks a pick would add, in the order the rows were taken.
///
/// Nothing is de-duplicated: two rows holding one track both travel, because the
/// pick is what the user dragged and not a set.
pub fn pick(targets: &[TrackEntry]) -> Option<Vec<String>> {
    pickable(targets).then(|| targets.iter().map(|entry| entry.uri.clone()).collect())
}

/// One plain command row.
fn item(verb: Verb, label: String) -> Item {
    Item {
        label,
        kind: Kind::Plain,
        key: None,
        submenu: false,
        verb,
    }
}

/// What a menu about these rows is called, for the one place a menu states its
/// subject rather than its commands: the accessibility tree.
///
/// A library row names itself; a pick names its count.
pub fn menu_label(targets: &[TrackEntry], listing: &Listing) -> String {
    if let Listing::Library(item) | Listing::Band(item) = listing {
        return item.name.clone();
    }
    match targets {
        [] => "Commands".to_owned(),
        [one] => one.title.clone(),
        many => format!("{} {}", many.len(), noun(many)),
    }
}

/// The destructive command's label, which names the list and the pick's count.
///
/// `None` for a list that cannot remove, and for a pick the menu would not open
/// on.
fn remove_label(targets: &[TrackEntry], listing: &Listing) -> Option<String> {
    match listing {
        Listing::Playlist { .. } => Some(match targets {
            [] => return None,
            [..] if targets.len() == 1 => "Remove from this playlist".to_owned(),
            many => format!("Remove {} {} from this playlist", many.len(), noun(many)),
        }),
        Listing::Queue => Some(match targets {
            [] => return None,
            [..] if targets.len() == 1 => "Remove from the queue".to_owned(),
            many => format!("Remove {} from the queue", many.len()),
        }),
        // A library row's removal row is built by `library_items`, because it
        // is one thing rather than a pick.
        Listing::Page | Listing::Current | Listing::Library(_) | Listing::Band(_) => None,
    }
}

/// The queue command's label, which carries the pick's count and its noun.
fn queue_label(targets: &[TrackEntry]) -> String {
    if targets.len() == 1 {
        return "Add to queue".to_owned();
    }
    format!("Add {} {} to queue", targets.len(), noun(targets))
}

/// The noun a count-bearing label uses: a playlist can hold episodes and local
/// files, so only a pick of nothing but tracks says tracks.
fn noun(targets: &[TrackEntry]) -> &'static str {
    if targets.iter().all(TrackEntry::is_track) {
        "tracks"
    } else {
        "items"
    }
}

/// The value every target agrees on, or `None` when one is missing or differs.
///
/// This is the sheet's pick rule: a verb that is not single-valued across the
/// pick is dropped, not decided by the row the menu opened on.
fn single<'a>(
    targets: &'a [TrackEntry],
    of: impl Fn(&'a TrackEntry) -> Option<&'a str>,
) -> Option<&'a str> {
    let mut found = None;
    for target in targets {
        let value = of(target)?;
        match found {
            None => found = Some(value),
            Some(seen) if seen != value => return None,
            Some(_) => {}
        }
    }
    found
}

/// Every target's web address, in the order the pick holds them.
fn links(targets: &[TrackEntry]) -> Vec<String> {
    targets
        .iter()
        .filter_map(|entry| spotify_link(&entry.uri))
        .collect()
}

#[cfg(test)]
mod tests;
