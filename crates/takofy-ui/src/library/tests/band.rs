use super::super::*; // the module under test
use super::*; // the slice's shared fixtures, from tests/mod.rs

#[test]
fn a_release_reply_reaches_the_release_store() {
    let mut library = Library::default();
    library.open(artist("spotify:artist:a"));
    library.apply(&Update::Artist(artist_detail("spotify:artist:a")));
    library.releases.look("spotify:album:r1");
    let _ = library.releases.requests();

    library.apply(&Update::ReleaseTracks {
        release: "spotify:album:r1".to_owned(),
        tracks: vec![track("t")],
    });

    assert!(matches!(
        library.releases.look("spotify:album:r1"),
        crate::releases::Labels::Ready(_)
    ));
}

#[test]
fn band_cover_waits_until_the_tracks_settle() {
    let cover = cover_url("context");
    let entry = playlist_with_cover("a", "A", cover.clone());
    assert_eq!(band_cover(&entry, false), BandCover::Pending);
    assert_eq!(
        band_cover(&entry, true),
        BandCover::Entry(Some(&CoverSet::single(cover)))
    );
}

#[test]
fn band_cover_is_missing_when_the_entry_names_none() {
    let entry = playlist("a", "A");
    assert_eq!(band_cover(&entry, true), BandCover::Entry(None));
}

#[test]
fn a_failed_context_still_settles_the_band_on_the_entry() {
    // The band's rule is the load's settle state, not the context's success:
    // a failed context settles onto whatever the entry already holds.
    let cover = cover_url("first-track");
    let entry = playlist_with_cover("a", "A", cover.clone());
    let mut library = Library::default();
    library.apply(&Update::Playlists(vec![entry]));
    library.select("a");
    library.apply(&Update::TracksFailed {
        playlist: "a".to_owned(),
        failure: Failure::Load,
    });
    let settled = !matches!(library.tracks, Load::Loading | Load::Idle);
    let entry = library.selected_entry().expect("the selected entry");
    assert_eq!(
        band_cover(entry, settled),
        BandCover::Entry(Some(&CoverSet::single(cover)))
    );
}

#[test]
fn band_cover_handles_an_empty_playlist() {
    let mut library = Library::default();
    library.select("a");
    library.apply(&Update::Tracks {
        playlist: "a".to_owned(),
        tracks: Vec::new(),
        cover: None,
        access: None,
    });
    assert_eq!(library.tracks, Load::Ready(tracks_for("a", Vec::new())));
    let settled = !matches!(library.tracks, Load::Loading | Load::Idle);
    // With no list loaded there is no entry to read; the band's caller draws
    // nothing. This pins the settled rule for the empty list itself.
    assert!(settled);
    assert!(library.selected_entry().is_none());
}

#[test]
fn the_band_waits_instead_of_drawing_the_dropped_cover_as_a_hole() {
    let first = cover_url("first");
    let mut library = page_dropped_by_an_add(Some(&first));
    assert!(
        library.tracks_request().is_some(),
        "arrange: the page is being read again"
    );

    let entry = library.selected_entry().expect("the selected entry");
    assert_eq!(
        band_cover(entry, settled(&library)),
        BandCover::Pending,
        "a cover being read waits, so the open band never draws a hole"
    );
}

#[test]
fn the_band_draws_the_cover_the_re_read_brought() {
    let first = cover_url("first");
    let mut library = page_dropped_by_an_add(Some(&first));
    assert!(library.tracks_request().is_some(), "arrange: the re-read");

    let fresh = cover_url("fresh");
    library.apply(&Update::Tracks {
        playlist: "a".to_owned(),
        tracks: moved(&["spotify:track:x"]),
        cover: Some(CoverSet::single(fresh.clone())),
        access: access(0x08, true),
    });

    let entry = library.selected_entry().expect("the selected entry");
    assert_eq!(
        band_cover(entry, settled(&library)),
        BandCover::Entry(Some(&CoverSet::single(fresh.clone()))),
        "once the read lands the band draws the cover it brought"
    );
}

#[test]
fn an_emptied_playlist_leaves_the_band_on_the_entry_with_no_cover() {
    let first = cover_url("first");
    let mut library = covered_page(&first, &["spotify:track:t0"]);
    assert!(
        library
            .remove_tracks("a", &[0], &moved(&["spotify:track:t0"]))
            .is_some()
    );
    library.apply(&Update::TracksRemoved {
        playlist: "a".to_owned(),
        revision: revision(0x02),
    });
    assert!(library.tracks_request().is_some(), "arrange: the re-ask");

    library.apply(&Update::Tracks {
        playlist: "a".to_owned(),
        tracks: Vec::new(),
        cover: None,
        access: access(0x02, true),
    });

    let entry = library.selected_entry().expect("the selected entry");
    assert_eq!(
        band_cover(entry, settled(&library)),
        BandCover::Entry(None),
        "an emptied playlist has no cover, and the band says so rather than waiting"
    );
}
