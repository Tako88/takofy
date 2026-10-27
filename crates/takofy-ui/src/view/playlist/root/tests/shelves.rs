use super::super::*;
use super::test_support::*;

use std::time::Instant;

use crate::library_filter::Item;
use crate::message::Update;

/// Each of the four kinds knows the shelf it belongs to, so a menu built on
/// it never has to read the uri back.
#[test]
fn media_of_gives_each_item_its_shelf() {
    let mut art = Art::default();
    let tokens = Tokens::DARK;
    let mut select = None;
    let mut open = None;
    let mut play = None;
    let library = Library::default();
    let mut draw = MediaDraw {
        art: &mut art,
        now: Instant::now(),
        tokens: &tokens,
        library: &library,
        select: &mut select,
        open: &mut open,
        play: &mut play,
    };
    let playlist = playlist("spotify:playlist:a", "A");
    let show = show("spotify:show:a", "A");
    let artist = followed_artist("spotify:artist:a", "A");
    let album = saved_album("spotify:album:a", "A");
    let cases = [
        (Item::Playlist(&playlist), SidebarKind::Playlists),
        (Item::Show(&show), SidebarKind::Podcasts),
        (Item::Artist(&artist), SidebarKind::Artists),
        (Item::Album(&album), SidebarKind::Albums),
    ];
    for (item, shelf) in cases {
        assert_eq!(draw.media_of(&item, MediaLayout::Grid, 176).shelf, shelf);
    }
}

#[test]
fn idle_view_artists_ready_loading_failed_and_empty() {
    let mut root = library(vec![playlist("a", "A")], vec![]);
    root.root_filter.choose(RootKind::Artists);

    // Not asked for yet: loading, not the empty copy.
    assert_eq!(
        idle_view(&root),
        IdleView::Grid {
            items: Vec::new(),
            note: LoadNote::Loading(FollowedRead::Artists),
        }
    );

    root.apply(&Update::FollowedArtists(vec![
        followed_artist("spotify:artist:1", "One"),
        followed_artist("spotify:artist:2", "Two"),
    ]));
    let IdleView::Grid { items, note } = idle_view(&root) else {
        panic!("a ready artists list is a grid");
    };
    assert_eq!(items.len(), 2, "both artists draw as cards");
    assert!(matches!(items[0], Item::Artist(artist) if artist.uri.ends_with(":1")));
    assert_eq!(note, LoadNote::None);

    root.apply(&Update::FollowedArtistsFailed(Failure::FollowedArtists));
    assert_eq!(
        idle_view(&root),
        IdleView::Grid {
            items: Vec::new(),
            note: LoadNote::Failed(FollowedRead::Artists, Failure::FollowedArtists),
        }
    );

    let mut empty = library(vec![playlist("a", "A")], vec![]);
    empty.root_filter.choose(RootKind::Artists);
    empty.apply(&Update::FollowedArtists(Vec::new()));
    assert_eq!(
        idle_view(&empty),
        IdleView::Empty(RootKind::Artists),
        "nothing to show is the chip's own empty line"
    );
}

#[test]
fn idle_view_albums_carry_their_own_load() {
    let mut library = library(vec![playlist("a", "A")], vec![]);
    library.root_filter.choose(RootKind::Albums);
    library.apply(&Update::SavedAlbums(vec![saved_album(
        "spotify:album:1",
        "Slow Tide",
    )]));

    let IdleView::Grid { items, note } = idle_view(&library) else {
        panic!("a ready albums list is a grid");
    };
    assert_eq!(items.len(), 1);
    assert_eq!(note, LoadNote::None);

    library.apply(&Update::SavedAlbumsFailed(Failure::SavedAlbums));
    let IdleView::Grid { note, .. } = idle_view(&library) else {
        panic!("a failed albums list keeps the grid");
    };
    assert_eq!(
        note,
        LoadNote::Failed(FollowedRead::Albums, Failure::SavedAlbums)
    );
}

/// The note's Retry asks the read it names, not the shows.
#[test]
fn the_artists_note_retries_the_artists() {
    let ctx = context();
    let mut library = library(vec![playlist("a", "A")], vec![]);
    library.root_filter.choose(RootKind::Artists);
    library.apply(&Update::FollowedArtistsFailed(Failure::FollowedArtists));
    let mut art = Art::default();

    let before = frame(&ctx, &mut library, &mut art);
    let retry = text_rect(&before, "Retry").expect("the note's retry");
    let command = click(&ctx, retry.center(), &mut library, &mut art);

    assert_eq!(command, Some(Command::LoadFollowedArtists));
    assert_eq!(library.followed_artists, Load::Loading);
}

/// Pressing the Artists chip takes the choice, asks for the list once, and
/// the next frame says it is loading rather than claiming the library is
/// empty.
#[test]
fn the_artists_chip_asks_for_the_list_and_says_it_is_loading() {
    let ctx = context();
    let mut library = library(vec![playlist("a", "A")], vec![]);
    let mut art = Art::default();

    let before = frame(&ctx, &mut library, &mut art);
    let chip = chip_rect(&before, "Artists");
    let _ = click(&ctx, chip.center(), &mut library, &mut art);

    assert_eq!(library.root_filter.chosen(), RootKind::Artists);
    assert_eq!(
        library.followed_artists_request(),
        Some(Command::LoadFollowedArtists)
    );
    assert_eq!(library.followed_artists_request(), None, "asked once");

    let after = frame(&ctx, &mut library, &mut art);
    let texts = drawn_texts(&after);
    assert!(
        texts.contains(&"Loading your artists…"),
        "the root says what it is doing: {texts:?}"
    );
    assert!(
        !texts.contains(&"You don't follow any artists yet."),
        "and does not claim the list is empty: {texts:?}"
    );
}

#[test]
fn the_followed_retry_returns_load_followed_shows() {
    let ctx = context();
    let mut library = library(vec![playlist("a", "A")], vec![]);
    library.apply(&Update::FollowedShowsFailed(Failure::Load));
    let mut art = Art::default();

    let before = frame(&ctx, &mut library, &mut art);
    let retry = text_rect(&before, "Retry").expect("the followed note's retry");
    let command = click(&ctx, retry.center(), &mut library, &mut art);

    assert_eq!(command, Some(Command::LoadFollowedShows));
}
