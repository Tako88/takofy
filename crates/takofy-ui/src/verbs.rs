//! What a menu's verbs say, and what each one does with the rows it was opened on.
//!
//! Pure: a verb's label, its presence and its outcome are all decided from the
//! snapshot the menu took when it opened. Nothing here fetches, writes or draws.

use takofy_model::TrackEntry;

use crate::menu::{Item, Kind, Verb};
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

/// The rows' commands, in the sheet's order: queue, album, artist, link.
///
/// A verb that cannot apply is absent rather than greyed, and a pick only keeps
/// the verbs that are single-valued across it.
pub fn track_items(targets: &[TrackEntry], here: Option<&Page>) -> Vec<Item> {
    let mut items = Vec::new();
    if targets.is_empty() {
        return items;
    }

    items.push(item(Verb::AddToQueue, queue_label(targets)));

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
    }
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
    use crate::menu::{Item, Kind, Verb};
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
    fn one_track_gets_the_sheets_four_verbs_in_order() {
        let items = track_items(&[entry("spotify:track:a")], None);
        assert_eq!(
            labels(&items),
            ["Add to queue", "Go to album", "Go to artist", "Copy link"]
        );
        assert_eq!(
            verbs(&items),
            [
                Verb::AddToQueue,
                Verb::GoToAlbum,
                Verb::GoToArtist,
                Verb::CopyLink,
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
        let items = track_items(&targets, None);
        let labels = labels(&items);
        assert_eq!(labels[0], "Add 3 tracks to queue");
        assert_eq!(labels[2], "Go to artist", "a go-to never carries a count");
        assert_eq!(labels[3], "Copy 3 links");
    }

    #[test]
    fn a_pick_across_albums_omits_go_to_album_and_across_artists_go_to_artist() {
        let mut other_album = entry("spotify:track:b");
        other_album.album_uri = Some("spotify:album:z".to_owned());
        let items = track_items(&[entry("spotify:track:a"), other_album], None);
        assert!(!labels(&items).contains(&"Go to album"));

        let mut other_artist = entry("spotify:track:b");
        other_artist.artist_uris = vec!["spotify:artist:z".to_owned()];
        let items = track_items(&[entry("spotify:track:a"), other_artist], None);
        assert!(!labels(&items).contains(&"Go to artist"));
    }

    #[test]
    fn a_missing_album_or_artist_uri_omits_the_verb() {
        let mut bare = entry("spotify:track:a");
        bare.album_uri = None;
        bare.artist_uris = Vec::new();
        let items = track_items(&[bare], None);
        assert_eq!(labels(&items), ["Add to queue", "Copy link"]);
    }

    #[test]
    fn a_verb_that_leads_here_is_omitted() {
        let targets = [entry("spotify:track:a")];
        let album = track_items(&targets, Some(&Page::Album("spotify:album:a".to_owned())));
        assert!(!labels(&album).contains(&"Go to album"));
        assert!(labels(&album).contains(&"Go to artist"));

        let artist = track_items(&targets, Some(&Page::credit("spotify:artist:b")));
        assert!(!labels(&artist).contains(&"Go to artist"));
        assert!(labels(&artist).contains(&"Go to album"));
    }

    #[test]
    fn a_pick_with_one_linkable_track_says_copy_link() {
        let mut local = entry("spotify:local:A:B:C:1000");
        local.album_uri = None;
        local.artist_uris = Vec::new();
        let targets = [entry("spotify:track:a"), local];
        let items = track_items(&targets, None);
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
        let items = track_items(&[local], None);
        assert_eq!(labels(&items), ["Add to queue"]);
    }

    #[test]
    fn an_episode_pick_says_items() {
        let episode = entry("spotify:episode:e");
        let items = track_items(&[episode.clone(), episode], None);
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
}
