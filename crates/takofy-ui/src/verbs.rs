//! What a menu's verbs say, and what each one does with the rows it was opened on.
//!
//! Pure: a verb's label, its presence and its outcome are all decided from the
//! snapshot the menu took when it opened. Nothing here fetches, writes or draws.

use std::collections::HashSet;

use takofy_model::TrackEntry;

use crate::likes::Standing;
use crate::menu::{Item, Kind, Listing, Verb};
use crate::route::Page;

/// What applying a verb does, once the shell has the frame's command to spare.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
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
    /// The sidebar's name field opens; nothing is sent and nothing else moves.
    NewPlaylist,
    /// These tracks go to the playlist the submenu's chosen row names.
    AddToPlaylist { tracks: Vec<String> },
}

/// The web address a row's URI stands for, or `None` when it has none.
///
/// Only the two kinds a row can be are mapped. A local file has no page and a
/// malformed URI has no id, so neither yields a link.
pub fn spotify_link(uri: &str) -> Option<String> {
    let (kind, id) = uri.strip_prefix("spotify:")?.split_once(':')?;
    if id.is_empty() || id.contains(':') {
        return None;
    }
    let kind = match kind {
        "track" => "track",
        "episode" => "episode",
        _ => return None,
    };
    Some(format!("https://open.spotify.com/{kind}/{id}"))
}

/// The heart's label and the menu row's, which say the action the state implies.
pub fn heart_label(liked: bool) -> &'static str {
    if liked {
        "Remove from your library"
    } else {
        "Save to your library"
    }
}

/// The rows' commands, in the sheet's order: queue, library, album, artist,
/// link, remove.
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
        Verb::NewPlaylist => Some(Outcome::NewPlaylist),
        Verb::AddToPlaylist => pick(targets).map(|tracks| Outcome::AddToPlaylist { tracks }),
        Verb::Remove => (!targets.is_empty()).then_some(Outcome::Remove),
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
pub fn menu_label(targets: &[TrackEntry]) -> String {
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
        Listing::Page | Listing::Current => None,
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
mod tests {
    use super::*;
    use crate::menu::{Anchor, Device, Door, Item, Kind, Listing, Request, Verb};
    use crate::route::Page;
    use takofy_model::TrackEntry;

    fn entry(uri: &str) -> TrackEntry {
        TrackEntry {
            uri: uri.to_owned(),
            title: format!("title of {uri}"),
            artists: "A Band".to_owned(),
            album: Some("A Record".to_owned()),
            album_uri: Some("spotify:album:a".to_owned()),
            artist_uris: vec!["spotify:artist:b".to_owned()],
            added_at: None,
            duration_ms: None,
            named: true,
            cover: None,
        }
    }

    fn labels(items: &[Item]) -> Vec<&str> {
        items.iter().map(|item| item.label.as_str()).collect()
    }

    fn verbs(items: &[Item]) -> Vec<Verb> {
        items.iter().map(|item| item.verb).collect()
    }

    #[test]
    fn copy_link_maps_a_track_uri_to_its_web_url() {
        assert_eq!(
            spotify_link("spotify:track:abc").as_deref(),
            Some("https://open.spotify.com/track/abc")
        );
    }

    #[test]
    fn copy_link_handles_episodes_and_refuses_local_and_malformed_uris() {
        assert_eq!(
            spotify_link("spotify:episode:xyz").as_deref(),
            Some("https://open.spotify.com/episode/xyz")
        );
        for refused in [
            "spotify:local:Artist:Album:Title:180000",
            "spotify:track:",
            "track:abc",
            "",
            "spotify:track:a:b",
            "spotify:album:a",
        ] {
            assert_eq!(spotify_link(refused), None, "{refused}");
        }
    }

    #[test]
    fn one_track_gets_the_sheets_six_verbs_in_order() {
        let items = track_items(
            &[entry("spotify:track:a")],
            None,
            &Listing::Page,
            Standing::Unknown,
        );
        assert_eq!(
            labels(&items),
            [
                "Add to queue",
                "Go to album",
                "Go to artist",
                "Copy link",
                "Add to playlist",
                "New playlist…"
            ]
        );
        assert_eq!(
            verbs(&items),
            [
                Verb::AddToQueue,
                Verb::GoToAlbum,
                Verb::GoToArtist,
                Verb::CopyLink,
                Verb::AddToPlaylist,
                Verb::NewPlaylist,
            ]
        );
        assert!(items.iter().all(|item| item.kind == Kind::Plain));
    }

    #[test]
    fn a_pick_puts_its_count_in_the_labels() {
        let targets = [
            entry("spotify:track:a"),
            entry("spotify:track:b"),
            entry("spotify:track:c"),
        ];
        let items = track_items(&targets, None, &Listing::Page, Standing::Unknown);
        let labels = labels(&items);
        assert_eq!(labels[0], "Add 3 tracks to queue");
        assert_eq!(labels[2], "Go to artist", "a go-to never carries a count");
        assert_eq!(labels[3], "Copy 3 links");
    }

    #[test]
    fn a_pick_across_albums_omits_go_to_album_and_across_artists_go_to_artist() {
        let mut other_album = entry("spotify:track:b");
        other_album.album_uri = Some("spotify:album:z".to_owned());
        let items = track_items(
            &[entry("spotify:track:a"), other_album],
            None,
            &Listing::Page,
            Standing::Unknown,
        );
        assert!(!labels(&items).contains(&"Go to album"));

        let mut other_artist = entry("spotify:track:b");
        other_artist.artist_uris = vec!["spotify:artist:z".to_owned()];
        let items = track_items(
            &[entry("spotify:track:a"), other_artist],
            None,
            &Listing::Page,
            Standing::Unknown,
        );
        assert!(!labels(&items).contains(&"Go to artist"));
    }

    #[test]
    fn a_missing_album_or_artist_uri_omits_the_verb() {
        let mut bare = entry("spotify:track:a");
        bare.album_uri = None;
        bare.artist_uris = Vec::new();
        let items = track_items(&[bare], None, &Listing::Page, Standing::Unknown);
        assert_eq!(
            labels(&items),
            [
                "Add to queue",
                "Copy link",
                "Add to playlist",
                "New playlist…"
            ]
        );
    }

    #[test]
    fn a_verb_that_leads_here_is_omitted() {
        let targets = [entry("spotify:track:a")];
        let album = track_items(
            &targets,
            Some(&Page::Album("spotify:album:a".to_owned())),
            &Listing::Page,
            Standing::Unknown,
        );
        assert!(!labels(&album).contains(&"Go to album"));
        assert!(labels(&album).contains(&"Go to artist"));

        let artist = track_items(
            &targets,
            Some(&Page::credit("spotify:artist:b")),
            &Listing::Page,
            Standing::Unknown,
        );
        assert!(!labels(&artist).contains(&"Go to artist"));
        assert!(labels(&artist).contains(&"Go to album"));
    }

    #[test]
    fn a_pick_with_one_linkable_track_says_copy_link() {
        let mut local = entry("spotify:local:A:B:C:1000");
        local.album_uri = None;
        local.artist_uris = Vec::new();
        let targets = [entry("spotify:track:a"), local];
        let items = track_items(&targets, None, &Listing::Page, Standing::Unknown);
        let copy = items
            .iter()
            .find(|item| item.verb == Verb::CopyLink)
            .expect("one target is linkable");
        assert_eq!(copy.label, "Copy link");
        let Some(Outcome::Copy(text)) = outcome(Verb::CopyLink, &targets) else {
            panic!("copy link copies");
        };
        assert_eq!(text, "https://open.spotify.com/track/a");
        assert_eq!(text.lines().count(), 1);
    }

    #[test]
    fn a_local_only_pick_has_no_copy_link() {
        let mut local = entry("spotify:local:A:B:C:1000");
        local.album_uri = None;
        local.artist_uris = Vec::new();
        let items = track_items(&[local], None, &Listing::Page, Standing::Unknown);
        assert_eq!(labels(&items), ["Add to queue", "New playlist…"]);
    }

    #[test]
    fn an_episode_pick_says_items() {
        let episode = entry("spotify:episode:e");
        let items = track_items(
            &[episode.clone(), episode],
            None,
            &Listing::Page,
            Standing::Unknown,
        );
        assert_eq!(labels(&items)[0], "Add 2 items to queue");
    }

    #[test]
    fn outcomes_carry_what_the_menu_snapshot_held() {
        let targets = [
            entry("spotify:track:a"),
            entry("spotify:track:b"),
            entry("spotify:track:c"),
        ];
        assert_eq!(
            outcome(Verb::AddToQueue, &targets),
            Some(Outcome::Queue(targets.to_vec()))
        );

        let Some(Outcome::Copy(text)) = outcome(Verb::CopyLink, &targets) else {
            panic!("copy link copies");
        };
        assert_eq!(
            text,
            "https://open.spotify.com/track/a\nhttps://open.spotify.com/track/b\n\
             https://open.spotify.com/track/c"
        );
        assert!(!text.ends_with('\n'), "one URL per line, no trailing break");

        assert_eq!(
            outcome(Verb::GoToAlbum, &targets),
            Some(Outcome::Open(Page::Album("spotify:album:a".to_owned())))
        );
        assert_eq!(
            outcome(Verb::GoToArtist, &targets),
            Some(Outcome::Open(Page::Artist("spotify:artist:b".to_owned())))
        );
    }

    #[test]
    fn an_outcome_that_cannot_apply_is_none() {
        let mut bare = entry("spotify:track:a");
        bare.album_uri = None;
        bare.artist_uris = Vec::new();
        assert_eq!(outcome(Verb::GoToAlbum, &[bare.clone()]), None);
        assert_eq!(outcome(Verb::GoToArtist, &[bare]), None);
        assert_eq!(outcome(Verb::AddToQueue, &[]), None);
    }
    fn playlist_listing() -> Listing {
        Listing::Playlist {
            uri: "spotify:playlist:a".to_owned(),
            list: egui::Id::unique("playlist-tracks"),
        }
    }

    #[test]
    fn a_playlist_pick_labels_the_count() {
        let one = track_items(
            &[entry("spotify:track:a")],
            None,
            &playlist_listing(),
            Standing::Unknown,
        );
        assert_eq!(
            labels(&one),
            [
                "Add to queue",
                "Go to album",
                "Go to artist",
                "Copy link",
                "Add to playlist",
                "New playlist…",
                "Remove from this playlist",
            ]
        );

        let three = [
            entry("spotify:track:a"),
            entry("spotify:track:b"),
            entry("spotify:track:c"),
        ];
        let items = track_items(&three, None, &playlist_listing(), Standing::Unknown);
        assert!(
            labels(&items).contains(&"Remove 3 tracks from this playlist"),
            "{:?}",
            labels(&items)
        );
    }

    #[test]
    fn a_queue_pick_labels_the_count_without_a_noun() {
        let one = track_items(
            &[entry("spotify:track:a")],
            None,
            &Listing::Queue,
            Standing::Unknown,
        );
        assert_eq!(
            labels(&one),
            [
                "Go to album",
                "Go to artist",
                "Copy link",
                "Add to playlist",
                "New playlist…",
                "Remove from the queue"
            ]
        );

        let three = [
            entry("spotify:track:a"),
            entry("spotify:track:b"),
            entry("spotify:track:c"),
        ];
        let items = track_items(&three, None, &Listing::Queue, Standing::Unknown);
        assert_eq!(
            labels(&items).last(),
            Some(&"Remove 3 from the queue"),
            "the destructive row stays last"
        );
    }

    #[test]
    fn the_remove_row_is_last_destructive_and_carries_del() {
        let items = track_items(
            &[entry("spotify:track:a")],
            None,
            &playlist_listing(),
            Standing::Unknown,
        );
        let last = items.last().expect("a command");
        assert_eq!(last.verb, Verb::Remove);
        assert_eq!(last.kind, Kind::Destructive);
        assert_eq!(last.key, Some("Del"));
        assert!(!last.submenu);
        assert!(
            items[..items.len() - 1]
                .iter()
                .all(|item| item.kind == Kind::Plain),
            "a second command claimed to be destructive"
        );
    }

    #[test]
    fn a_page_listing_has_no_remove_row() {
        let items = track_items(
            &[entry("spotify:track:a")],
            None,
            &Listing::Page,
            Standing::Unknown,
        );
        assert!(!verbs(&items).contains(&Verb::Remove));
        assert!(verbs(&items).contains(&Verb::AddToQueue));
    }

    #[test]
    fn the_current_listing_has_neither_add_nor_remove() {
        let items = track_items(
            &[entry("spotify:track:a")],
            None,
            &Listing::Current,
            Standing::Unknown,
        );
        assert_eq!(
            verbs(&items),
            [
                Verb::GoToAlbum,
                Verb::GoToArtist,
                Verb::CopyLink,
                Verb::AddToPlaylist,
                Verb::NewPlaylist
            ]
        );
    }

    #[test]
    fn the_queue_listing_has_no_add_to_queue() {
        let items = track_items(
            &[entry("spotify:track:a")],
            None,
            &Listing::Queue,
            Standing::Unknown,
        );
        assert!(!verbs(&items).contains(&Verb::AddToQueue));
        assert_eq!(verbs(&items).last(), Some(&Verb::Remove));
    }

    #[test]
    fn no_other_item_carries_a_key_hint() {
        let targets = [entry("spotify:track:a")];
        for listing in [
            Listing::Page,
            Listing::Queue,
            Listing::Current,
            playlist_listing(),
        ] {
            for item in track_items(&targets, None, &listing, Standing::Unknown) {
                if item.verb == Verb::Remove {
                    assert_eq!(item.key, Some("Del"), "{}", item.label);
                } else {
                    assert_eq!(item.key, None, "{}", item.label);
                }
                assert_eq!(
                    item.submenu,
                    item.verb == Verb::AddToPlaylist,
                    "{}",
                    item.label
                );
            }
        }
    }

    #[test]
    fn a_local_current_entry_has_no_items() {
        let mut local = entry("spotify:local:A:B:C:1000");
        local.album_uri = None;
        local.artist_uris = Vec::new();
        assert!(track_items(&[local], None, &Listing::Current, Standing::Unknown).is_empty());
    }

    #[test]
    fn outcome_remove_is_a_unit_and_none_for_no_targets() {
        let targets = [entry("spotify:track:a")];
        assert_eq!(outcome(Verb::Remove, &targets), Some(Outcome::Remove));
        assert_eq!(outcome(Verb::Remove, &[]), None);
    }

    #[test]
    fn a_mixed_pick_says_items_and_counts_only_the_linkable_rows() {
        let mut local = entry("spotify:local:A:B:C:1000");
        local.album_uri = None;
        local.artist_uris = Vec::new();
        let targets = [entry("spotify:track:a"), entry("spotify:episode:e"), local];

        let items = track_items(&targets, None, &playlist_listing(), Standing::Unknown);
        assert!(
            labels(&items).contains(&"Remove 3 items from this playlist"),
            "{:?}",
            labels(&items)
        );
        let copy = items
            .iter()
            .find(|item| item.verb == Verb::CopyLink)
            .expect("two rows are linkable");
        assert_eq!(copy.label, "Copy 2 links");
    }

    fn tracks(uris: &[&str]) -> Vec<TrackEntry> {
        uris.iter().map(|uri| entry(uri)).collect()
    }

    fn library(items: &[Item]) -> Option<&Item> {
        items
            .iter()
            .find(|item| matches!(item.verb, Verb::Save | Verb::Unsave))
    }

    #[test]
    fn heart_label_names_the_action() {
        assert_eq!(heart_label(false), "Save to your library");
        assert_eq!(heart_label(true), "Remove from your library");
    }

    #[test]
    fn an_unsaved_track_gets_save_after_add_to_queue() {
        let items = track_items(
            &tracks(&["spotify:track:a"]),
            None,
            &Listing::Page,
            Standing::Unsaved,
        );
        assert_eq!(
            labels(&items),
            [
                "Add to queue",
                "Save to your library",
                "Go to album",
                "Go to artist",
                "Copy link",
                "Add to playlist",
                "New playlist…"
            ]
        );
        assert_eq!(
            verbs(&items),
            [
                Verb::AddToQueue,
                Verb::Save,
                Verb::GoToAlbum,
                Verb::GoToArtist,
                Verb::CopyLink,
                Verb::AddToPlaylist,
                Verb::NewPlaylist
            ]
        );
    }

    #[test]
    fn a_saved_track_gets_remove_from_your_library() {
        let items = track_items(
            &tracks(&["spotify:track:a"]),
            None,
            &Listing::Page,
            Standing::Saved,
        );
        let row = library(&items).expect("a library row");
        assert_eq!(row.label, "Remove from your library");
        assert_eq!(row.verb, Verb::Unsave);
    }

    #[test]
    fn a_pick_of_unsaved_says_save_n_tracks() {
        let items = track_items(
            &tracks(&["spotify:track:a", "spotify:track:b", "spotify:track:c"]),
            None,
            &Listing::Page,
            Standing::Unsaved,
        );
        assert_eq!(
            library(&items).expect("a library row").label,
            "Save 3 tracks to your library"
        );
    }

    #[test]
    fn a_pick_of_saved_says_remove_n_tracks_from_your_library() {
        let items = track_items(
            &tracks(&["spotify:track:a", "spotify:track:b", "spotify:track:c"]),
            None,
            &Listing::Page,
            Standing::Saved,
        );
        assert_eq!(
            library(&items).expect("a library row").label,
            "Remove 3 tracks from your library"
        );
    }

    #[test]
    fn a_mixed_pick_offers_save_n_tracks_and_never_remove() {
        let targets = tracks(&["spotify:track:a", "spotify:track:b", "spotify:track:c"]);
        let items = track_items(&targets, None, &Listing::Page, Standing::Mixed);
        assert_eq!(
            library(&items).expect("a library row").label,
            "Save 3 tracks to your library",
            "the count is the whole pick, because that is what the sheet draws"
        );
        assert!(
            items.iter().all(|item| item.verb != Verb::Unsave),
            "a mixed pick has no single remove"
        );
    }

    #[test]
    fn an_unknown_standing_has_no_library_row() {
        let items = track_items(
            &tracks(&["spotify:track:a"]),
            None,
            &Listing::Page,
            Standing::Unknown,
        );
        assert!(library(&items).is_none());
    }

    #[test]
    fn a_pick_with_an_episode_or_a_local_file_has_no_library_row() {
        for uri in ["spotify:episode:e", "spotify:local:A:B:C:1000"] {
            let items = track_items(
                &tracks(&["spotify:track:a", uri]),
                None,
                &Listing::Page,
                Standing::Unknown,
            );
            assert!(library(&items).is_none(), "{uri}");
        }
    }

    #[test]
    fn all_four_listings_carry_the_library_row() {
        let targets = tracks(&["spotify:track:a"]);
        for listing in [
            Listing::Page,
            Listing::Queue,
            Listing::Current,
            playlist_listing(),
        ] {
            let items = track_items(&targets, None, &listing, Standing::Unsaved);
            assert!(library(&items).is_some(), "{listing:?} has no library row");
        }
    }

    #[test]
    fn the_queue_listing_has_save_but_no_add_to_queue() {
        let items = track_items(
            &tracks(&["spotify:track:a"]),
            None,
            &Listing::Queue,
            Standing::Unsaved,
        );
        assert!(library(&items).is_some());
        assert!(items.iter().all(|item| item.verb != Verb::AddToQueue));
    }

    #[test]
    fn the_current_listing_with_a_saveable_track_has_a_non_empty_menu() {
        let items = track_items(
            &tracks(&["spotify:track:a"]),
            None,
            &Listing::Current,
            Standing::Unsaved,
        );
        assert!(!items.is_empty());
        assert!(library(&items).is_some());
    }

    #[test]
    fn the_library_row_is_plain_and_carries_no_key_hint() {
        for standing in [Standing::Unsaved, Standing::Saved, Standing::Mixed] {
            let items = track_items(
                &tracks(&["spotify:track:a", "spotify:track:b"]),
                None,
                &Listing::Page,
                standing,
            );
            let row = library(&items).expect("a library row");
            assert_eq!(row.kind, Kind::Plain, "{standing:?}");
            assert_eq!(row.key, None, "{standing:?}");
            assert!(!row.submenu, "{standing:?}");
        }
    }

    #[test]
    fn outcome_save_lists_every_track_uri_once_in_order() {
        let targets = tracks(&[
            "spotify:track:a",
            "spotify:track:b",
            "spotify:track:a",
            "spotify:track:c",
        ]);
        assert_eq!(
            outcome(Verb::Save, &targets),
            Some(Outcome::Like {
                tracks: vec![
                    "spotify:track:a".to_owned(),
                    "spotify:track:b".to_owned(),
                    "spotify:track:c".to_owned(),
                ],
                liked: true,
            })
        );
    }

    #[test]
    fn outcome_unsave_is_the_same_with_liked_false() {
        let targets = tracks(&["spotify:track:a", "spotify:track:b"]);
        assert_eq!(
            outcome(Verb::Unsave, &targets),
            Some(Outcome::Like {
                tracks: vec!["spotify:track:a".to_owned(), "spotify:track:b".to_owned()],
                liked: false,
            })
        );
    }

    #[test]
    fn outcome_save_is_none_for_a_pick_with_a_non_track() {
        assert_eq!(
            outcome(
                Verb::Save,
                &tracks(&["spotify:track:a", "spotify:episode:e"])
            ),
            None
        );
        assert_eq!(
            outcome(Verb::Save, &tracks(&["spotify:local:A:B:C:1000"])),
            None
        );
        assert_eq!(outcome(Verb::Save, &[]), None);
    }

    #[test]
    fn new_playlist_follows_copy_link_and_precedes_the_band() {
        let items = track_items(
            &tracks(&["spotify:track:a"]),
            None,
            &playlist_listing(),
            Standing::Unknown,
        );
        let order = verbs(&items);
        let at = |verb: Verb| {
            order
                .iter()
                .position(|found| *found == verb)
                .unwrap_or_else(|| panic!("{verb:?} is offered"))
        };
        assert_eq!(at(Verb::AddToPlaylist), at(Verb::CopyLink) + 1);
        assert_eq!(at(Verb::NewPlaylist), at(Verb::AddToPlaylist) + 1);
        assert_eq!(at(Verb::Remove), at(Verb::NewPlaylist) + 1);
    }

    #[test]
    fn new_playlist_is_offered_on_all_four_listings() {
        for listing in [
            Listing::Page,
            Listing::Queue,
            Listing::Current,
            playlist_listing(),
        ] {
            let items = track_items(
                &tracks(&["spotify:track:a"]),
                None,
                &listing,
                Standing::Unknown,
            );
            assert!(
                verbs(&items).contains(&Verb::NewPlaylist),
                "{listing:?} has no new-playlist row"
            );
        }
    }

    #[test]
    fn a_menu_with_no_other_item_gets_no_new_playlist_row() {
        let mut local = entry("spotify:local:A:B:C:1000");
        local.album_uri = None;
        local.artist_uris = Vec::new();
        let items = track_items(&[local], None, &Listing::Current, Standing::Unknown);
        assert!(
            items.is_empty(),
            "a row that acts on nothing got a menu of its own: {:?}",
            labels(&items)
        );
    }

    #[test]
    fn new_playlist_is_plain_with_no_key_and_no_chevron() {
        let items = track_items(
            &tracks(&["spotify:track:a"]),
            None,
            &Listing::Page,
            Standing::Unknown,
        );
        let row = items
            .iter()
            .find(|item| item.verb == Verb::NewPlaylist)
            .expect("a new-playlist row");
        assert_eq!(row.label, "New playlist…");
        assert_eq!(row.kind, Kind::Plain);
        assert_eq!(row.key, None);
        assert!(!row.submenu);
    }

    #[test]
    fn outcome_new_playlist_is_a_unit_with_no_command() {
        assert_eq!(outcome(Verb::NewPlaylist, &[]), Some(Outcome::NewPlaylist));
        assert_eq!(
            outcome(Verb::NewPlaylist, &tracks(&["spotify:track:a"])),
            Some(Outcome::NewPlaylist),
            "the row acts on nothing the menu holds"
        );
    }

    #[test]
    fn request_allow_create_false_strips_only_that_row() {
        let targets = tracks(&["spotify:track:a"]);
        let items = track_items(&targets, None, &Listing::Page, Standing::Unknown);
        let offered = items.len();
        let mut request = Request {
            door: Door {
                anchor: Anchor::Pointer(egui::Pos2::ZERO),
                opener: egui::Id::unique("row-more"),
                device: Device::Keyboard,
            },
            playlists: Vec::new(),
            items,
            targets,
            rows: vec![0],
            listing: Listing::Page,
            page: None,
        };

        request.allow_create(false);

        assert_eq!(request.items.len(), offered - 1);
        assert!(
            request
                .items
                .iter()
                .all(|item| item.verb != Verb::NewPlaylist)
        );
        assert!(
            request.items.iter().any(|item| item.verb == Verb::CopyLink),
            "the strip took another row with it"
        );

        request.allow_create(true);
        assert_eq!(
            request.items.len(),
            offered - 1,
            "allowing a stripped row put it back"
        );
    }

    #[test]
    fn a_pick_of_tracks_keeps_the_rows_order_and_duplicates() {
        assert_eq!(pick(&tracks(&["spotify:track:a"])), Some(pick_of(&["a"])));
        assert_eq!(
            pick(&tracks(&[
                "spotify:track:a",
                "spotify:track:b",
                "spotify:track:c"
            ])),
            Some(pick_of(&["a", "b", "c"]))
        );
        assert_eq!(
            pick(&tracks(&["spotify:track:a", "spotify:track:a"])),
            Some(pick_of(&["a", "a"])),
            "two rows holding one track both travel"
        );
    }

    #[test]
    fn a_pick_with_anything_but_tracks_is_not_a_pick() {
        assert_eq!(pick(&[]), None, "an empty pick");
        assert!(!pickable(&[]));
        for uri in ["spotify:episode:e", "spotify:local:A:B:C:1000"] {
            assert_eq!(pick(&tracks(&[uri])), None, "{uri}");
            assert_eq!(
                pick(&tracks(&["spotify:track:a", uri])),
                None,
                "a mixed pick is refused whole: {uri}"
            );
            assert_eq!(
                pick(&tracks(&[uri, "spotify:track:a"])),
                None,
                "{uri} in front"
            );
        }
    }

    #[test]
    fn a_pick_is_only_tracks_and_nothing_else() {
        assert!(pickable(&tracks(&["spotify:track:a"])));
        assert!(pickable(&tracks(&["spotify:track:a", "spotify:track:b"])));
        assert!(!pickable(&tracks(&["spotify:episode:e"])));
    }

    #[test]
    fn an_add_row_names_the_pick_and_opens_a_list() {
        for (uris, label) in [
            (&["spotify:track:a"][..], "Add to playlist"),
            (
                &["spotify:track:a", "spotify:track:b", "spotify:track:c"][..],
                "Add 3 tracks to playlist",
            ),
        ] {
            let items = track_items(&tracks(uris), None, &Listing::Page, Standing::Unknown);
            let row = items
                .iter()
                .find(|item| item.verb == Verb::AddToPlaylist)
                .expect("an add row");
            assert_eq!(row.label, label);
            assert_eq!(row.kind, Kind::Plain);
            assert_eq!(row.key, None);
            assert!(row.submenu, "the add row is not a command");
        }
    }

    #[test]
    fn an_add_row_is_absent_for_a_pick_with_no_tracks() {
        for listing in [
            Listing::Page,
            Listing::Queue,
            Listing::Current,
            playlist_listing(),
        ] {
            for uris in [
                &["spotify:episode:e"][..],
                &["spotify:local:A:B:C:1000"][..],
                &["spotify:track:a", "spotify:episode:e"][..],
            ] {
                let items = track_items(&tracks(uris), None, &listing, Standing::Unknown);
                assert!(
                    items.iter().all(|item| item.verb != Verb::AddToPlaylist),
                    "{listing:?} offered an add for {uris:?}"
                );
            }
        }
    }

    #[test]
    fn outcome_add_to_playlist_carries_the_pick_in_order() {
        let targets = tracks(&["spotify:track:a", "spotify:track:b", "spotify:track:c"]);
        assert_eq!(
            outcome(Verb::AddToPlaylist, &targets),
            Some(Outcome::AddToPlaylist {
                tracks: pick_of(&["a", "b", "c"]),
            })
        );
        assert_eq!(outcome(Verb::AddToPlaylist, &[]), None);
        assert_eq!(
            outcome(
                Verb::AddToPlaylist,
                &tracks(&["spotify:track:a", "spotify:episode:e"])
            ),
            None
        );
    }

    /// The URIs a pick holds, so a test names only their ids.
    fn pick_of(ids: &[&str]) -> Vec<String> {
        ids.iter().map(|id| format!("spotify:track:{id}")).collect()
    }
}
