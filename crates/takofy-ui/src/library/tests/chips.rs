use super::super::*;
use super::test_support::*;
use crate::discography::Chip;
use crate::library_filter::RootKind;
use takofy_model::ReleaseKind;

#[test]
fn the_filter_starts_on_all() {
    assert_eq!(Library::default().filter.chosen(), Chip::All);
    assert_eq!(Library::default().filter.artist(), None);
}

#[test]
fn the_sidebar_kind_starts_on_playlists() {
    let mut library = Library::default();
    assert_eq!(library.sidebar_kind.chosen(), SidebarKind::Playlists);

    library.open(show("spotify:show:a"));
    assert_eq!(
        library.sidebar_kind.chosen(),
        SidebarKind::Playlists,
        "opening a page keeps the sidebar's choice"
    );

    library.back();
    assert_eq!(
        library.sidebar_kind.chosen(),
        SidebarKind::Playlists,
        "a Back keeps the sidebar's choice"
    );

    library.to_playlists();
    assert_eq!(
        library.sidebar_kind.chosen(),
        SidebarKind::Playlists,
        "going home keeps the sidebar's choice"
    );
}

#[test]
fn the_sidebar_kind_survives_a_trip_to_a_show_page_and_back() {
    let mut library = Library::default();
    assert!(library.sidebar_kind.choose(SidebarKind::Podcasts));

    library.open(show("spotify:show:a"));
    assert_eq!(library.sidebar_kind.chosen(), SidebarKind::Podcasts);

    library.back();
    assert_eq!(library.sidebar_kind.chosen(), SidebarKind::Podcasts);

    library.to_playlists();
    assert_eq!(library.sidebar_kind.chosen(), SidebarKind::Podcasts);
}

#[test]
fn the_playlist_highlight_clears_on_a_show_page_and_returns() {
    let mut library = Library::default();
    library.select("spotify:playlist:a");
    library.open(show("spotify:show:a"));
    assert_eq!(
        library.selected(),
        None,
        "the main panel is not showing a playlist"
    );

    library.back();
    assert_eq!(library.selected(), Some("spotify:playlist:a"));
}

#[test]
fn the_chosen_chip_survives_a_trip_to_a_show_page_and_back() {
    let mut library = Library::default();
    library.root_filter.choose(RootKind::Podcasts);
    assert_eq!(
        library.root_filter.chosen(),
        RootKind::Podcasts,
        "a live chip is pressed"
    );

    library.open(show("spotify:show:a"));
    assert_eq!(
        library.root_filter.chosen(),
        RootKind::Podcasts,
        "opening a page keeps the root's choice"
    );

    library.back();
    assert_eq!(
        library.root_filter.chosen(),
        RootKind::Podcasts,
        "a Back to the root redraws the chip the user left"
    );
}

#[test]
fn opening_another_artist_resets_the_filter() {
    let mut library = Library::default();
    library.open(artist("spotify:artist:a"));
    library.filter.choose(Chip::Kind(ReleaseKind::Ep));

    library.open(artist("spotify:artist:b"));

    assert_eq!(
        library.filter.chosen(),
        Chip::All,
        "another artist's discs are not the ones that were filtered"
    );
    assert_eq!(library.filter.artist(), Some("spotify:artist:b"));
}

#[test]
fn back_onto_the_same_artist_keeps_the_filter() {
    let mut library = Library::default();
    library.open(artist("spotify:artist:a"));
    library.apply(&Update::Artist(artist_detail("spotify:artist:a")));
    library.filter.choose(Chip::Kind(ReleaseKind::Ep));

    library.to_playlists();
    let _ = library.back();

    assert_eq!(
        library.filter.chosen(),
        Chip::Kind(ReleaseKind::Ep),
        "Back onto the same artist draws what the user left"
    );
}

#[test]
fn a_reload_of_the_same_artist_keeps_the_query_and_another_artist_drops_it() {
    let mut library = Library::default();
    library.open(artist("spotify:artist:a"));
    library.apply(&Update::Artist(artist_detail("spotify:artist:a")));
    library.filter.edit().push_str("  Midnight  ");

    library.to_playlists();
    let _ = library.back();
    assert_eq!(
        library.filter.needle().as_deref(),
        Some("midnight"),
        "a reload of the artist the query was typed on keeps it"
    );

    library.open(artist("spotify:artist:b"));
    assert_eq!(
        library.filter.needle(),
        None,
        "another artist's releases are not the ones the query named"
    );
    assert!(
        library.filter.edit().is_empty(),
        "nor does the field carry the words over"
    );
}

#[test]
fn retrying_the_same_artist_keeps_the_filter() {
    let mut library = Library::default();
    library.open(artist("spotify:artist:a"));
    library.apply(&Update::ArtistFailed {
        artist: "spotify:artist:a".to_owned(),
        failure: Failure::Load,
    });
    library.filter.choose(Chip::Kind(ReleaseKind::Single));

    assert_eq!(
        library.open(artist("spotify:artist:a")),
        Some(artist_command("spotify:artist:a")),
        "Retry re-issues the same load"
    );
    assert_eq!(library.filter.chosen(), Chip::Kind(ReleaseKind::Single));
}

#[test]
fn an_artist_reply_does_not_touch_the_filter() {
    let mut library = Library::default();
    library.open(artist("spotify:artist:a"));
    library.filter.choose(Chip::Kind(ReleaseKind::Compilation));

    library.apply(&Update::Artist(artist_detail("spotify:artist:a")));

    assert_eq!(
        library.filter.chosen(),
        Chip::Kind(ReleaseKind::Compilation),
        "a reply is not an artist change"
    );
}

#[test]
fn a_failed_artist_load_clears_the_discography_query_on_the_update() {
    let mut library = Library::default();
    library.open(artist("spotify:artist:a"));
    library.filter.edit().push_str("midnight");

    library.apply(&Update::ArtistFailed {
        artist: "spotify:artist:a".to_owned(),
        failure: Failure::Load,
    });

    assert_eq!(
        library.filter.needle(),
        None,
        "a failure that lands while another page is drawn still clears the query"
    );
}

#[test]
fn an_artist_with_no_releases_clears_the_discography_query_on_the_update() {
    let mut library = Library::default();
    library.open(artist("spotify:artist:a"));
    library.filter.edit().push_str("midnight");

    let mut detail = artist_detail("spotify:artist:a");
    detail.releases.clear();
    library.apply(&Update::Artist(detail));

    assert_eq!(
        library.filter.needle(),
        None,
        "an empty discography draws no chip row to hold the query"
    );
}

#[test]
fn to_playlists_keeps_the_filter() {
    let mut library = Library::default();
    library.open(artist("spotify:artist:a"));
    library.apply(&Update::Artist(artist_detail("spotify:artist:a")));
    library.filter.choose(Chip::Kind(ReleaseKind::Single));

    library.to_playlists();
    assert_eq!(
        library.filter.chosen(),
        Chip::Kind(ReleaseKind::Single),
        "home then Back reloads the same artist, so the choice is kept"
    );
    assert_eq!(library.filter.artist(), Some("spotify:artist:a"));

    let _ = library.back();
    assert_eq!(
        library.filter.chosen(),
        Chip::Kind(ReleaseKind::Single),
        "home then Back is still the same artist"
    );
}

#[test]
fn reopening_the_same_artist_keeps_the_filter() {
    let mut library = Library::default();
    library.open(artist("spotify:artist:a"));
    library.apply(&Update::Artist(artist_detail("spotify:artist:a")));
    library.filter.choose(Chip::Kind(ReleaseKind::Ep));
    library.to_playlists();

    assert_eq!(
        library.open(artist("spotify:artist:a")),
        Some(artist_command("spotify:artist:a")),
        "home cleared the want, so the page is loaded fresh rather than recalled"
    );
    assert_eq!(
        library.filter.chosen(),
        Chip::Kind(ReleaseKind::Ep),
        "opening the artist the choice was made for keeps it"
    );
}

#[test]
fn another_artists_ep_chip_does_not_keep_the_choice() {
    let mut library = Library::default();
    library.open(artist("spotify:artist:a"));
    library.apply(&Update::Artist(artist_detail("spotify:artist:a")));
    library.filter.choose(Chip::Kind(ReleaseKind::Ep));

    library.open(artist("spotify:artist:b"));

    let releases = vec![takofy_model::Release {
        kind: ReleaseKind::Ep,
        ..release("spotify:album:b-ep")
    }];
    let row = crate::discography::chips(&releases);
    assert!(
        row.contains(&Chip::Kind(ReleaseKind::Ep)),
        "B has an EP, so its row offers the chip the choice named: {row:?}"
    );
    assert_eq!(
        library.filter.effective(&row),
        Chip::All,
        "B still draws All pressed, because the choice belonged to A"
    );
}

#[test]
fn a_chip_change_clears_the_remove_sentence() {
    let mut library = sidebar_library(&["a"], 0x07, true, None);
    library.sidebar_notice = Some(SidebarWrite::Remove);

    assert!(library.choose_kind(SidebarKind::Podcasts));

    assert_eq!(library.sidebar_notice, None);
}
