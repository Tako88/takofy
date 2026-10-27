use egui::Pos2;

use takofy_model::{AlbumDetail, TrackEntry};

use super::super::*;
use super::test_support::*;

#[test]
fn a_row_opens_its_own_track_artist_not_the_albums() {
    // A compilation or a feature: the row names a guest, and the album names
    // the record's artist. The credit draw and the link are the same cell.
    let ctx = context();
    let row_track = TrackEntry {
        artists: "Guest".to_owned(),
        artist_uris: vec!["spotify:artist:guest".to_owned()],
        artist_names: vec!["Guest".to_owned()],
        ..track("spotify:track:a", "One", "Guest", 222_000)
    };
    let mut library = library_with(detail(vec![row_track]));
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    let (_, output) = frame(
        &ctx,
        Vec::new(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    assert!(
        drawn_texts(&output).contains(&"Guest"),
        "the row draws the track's own artist"
    );
    let credit = text_rect(&output, "Guest").expect("the track's artist cell");
    let command = click(
        &ctx,
        credit.center(),
        1,
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );

    assert_eq!(
        library.page(),
        Some(&Page::Artist("spotify:artist:guest".to_owned())),
        "the row's credit opens the artist it names, not the album's"
    );
    assert_eq!(
        command,
        Some(Command::LoadArtist {
            artist: "spotify:artist:guest".to_owned()
        })
    );
    assert!(!queue.can_next(), "opening a page must not start the queue");
}

/// A row whose own credit names a guest, so the artist cell is live.
fn featured() -> TrackEntry {
    TrackEntry {
        artists: "Guest".to_owned(),
        artist_uris: vec!["spotify:artist:guest".to_owned()],
        artist_names: vec!["Guest".to_owned()],
        ..track("spotify:track:a", "One", "Guest", 222_000)
    }
}

/// The album's own artist, and the page a click on the band's credit asks
/// for.
const HERO_ARTIST: &str = "spotify:artist:nils";

/// An album whose band credit knows its page.
fn linked_detail() -> AlbumDetail {
    AlbumDetail {
        artist_uri: Some(HERO_ARTIST.to_owned()),
        ..detail(vec![featured()])
    }
}

/// Every label a frame gave a link node.
fn link_labels(output: &egui::FullOutput) -> Vec<String> {
    output
        .platform_output
        .accesskit_update
        .as_ref()
        .map(|update| {
            update
                .nodes
                .iter()
                .filter_map(|(_, node)| {
                    (node.role() == egui::accesskit::Role::Link)
                        .then(|| node.label().map(str::to_owned))
                        .flatten()
                })
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn the_hero_artist_opens_the_artist_page() {
    let ctx = context();
    let mut library = library_with(linked_detail());
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    let (_, output) = frame(
        &ctx,
        Vec::new(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    let credit = text_rect(&output, "Nils Aker").expect("the band's credit");
    let command = click(
        &ctx,
        credit.center(),
        1,
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );

    assert_eq!(
        library.page(),
        Some(&Page::Artist(HERO_ARTIST.to_owned())),
        "the band's artist opens the artist it names"
    );
    assert_eq!(
        command,
        Some(Command::LoadArtist {
            artist: HERO_ARTIST.to_owned()
        })
    );
    assert!(!queue.can_next(), "opening a page must not start the queue");
}

#[test]
fn an_album_with_no_artist_uri_has_no_link() {
    let ctx = context();
    ctx.enable_accesskit();
    // A row whose own credit knows no page, so the band's is the only
    // candidate for a link node.
    let mut library = library_with(detail(vec![track(
        "spotify:track:a",
        "One",
        "Guest",
        222_000,
    )]));
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    let (_, output) = frame(
        &ctx,
        Vec::new(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    let byline = drawn_texts(&output)
        .into_iter()
        .find(|text| text.starts_with("Nils Aker"))
        .expect("the band's credit");
    let credit = text_rect(&output, byline).expect("the byline is painted");
    assert!(
        link_labels(&output).is_empty(),
        "a credit with no uri registered {:?}",
        link_labels(&output)
    );
    let command = click(
        &ctx,
        credit.center(),
        1,
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    assert_eq!(command, None, "plain words asked for nothing");
    assert_eq!(
        library.page(),
        Some(&Page::Album(ALBUM.to_owned())),
        "the page stays where it was"
    );
}

#[test]
fn a_click_in_the_artist_cells_dead_zone_selects_the_row_and_does_not_open() {
    // The link is the painted words, not the cell. The cell is a tight share
    // of a wide pane, so most of it is past the words; a click there belongs
    // to the row, and a double click there plays it.
    let ctx = context();
    let mut library = library_with(detail(vec![featured()]));
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    let (_, artist_cell, row) = cells_for(&ctx, 0, 1);
    let dead = Pos2::new(artist_cell.right() - 2.0, row.center().y);

    // The premise the test rests on: the probe is in the cell but past the
    // painted words. Without this the test would pass just as well if the
    // credit grew to fill the cell and there were no dead zone at all.
    let (_, output) = frame(
        &ctx,
        Vec::new(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    let credit = text_rect(&output, "Guest").expect("the row's painted artist");
    assert!(
        dead.x > credit.right(),
        "the probe at x={} must be past the painted credit, which ends at x={}",
        dead.x,
        credit.right()
    );

    let command = click(
        &ctx,
        dead,
        1,
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );

    assert_eq!(
        library.page(),
        Some(&Page::Album(ALBUM.to_owned())),
        "a click past the credit must not navigate"
    );
    assert_eq!(command, None, "a click past the words is the row's own");
    assert!(
        !queue.can_next(),
        "a click in the artist cell's dead zone must not play the row"
    );

    // A fresh page, so the double click is not the third of a sequence.
    let ctx = context();
    let mut library = library_with(detail(vec![featured()]));
    let command = click(
        &ctx,
        dead,
        2,
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );

    assert_eq!(
        command,
        Some(Command::Play {
            uri: "spotify:track:a".to_owned()
        }),
        "a double click past the credit plays the row"
    );
    assert_eq!(
        library.page(),
        Some(&Page::Album(ALBUM.to_owned())),
        "playing the row stays on the album"
    );
}

#[test]
fn a_double_click_on_the_title_still_plays() {
    // The artist cell is a link, but the title cell is still the row's own:
    // a double click there plays, as it always did.
    let ctx = context();
    let mut library = library_with(detail(vec![featured()]));
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    let (title_cell, _, _) = cells_for(&ctx, 0, 1);
    let command = click(
        &ctx,
        title_cell.center(),
        2,
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );

    assert_eq!(
        command,
        Some(Command::Play {
            uri: "spotify:track:a".to_owned()
        }),
        "a double click on the title plays the row"
    );
    assert_eq!(
        library.page(),
        Some(&Page::Album(ALBUM.to_owned())),
        "playing a row stays on the album"
    );
}
