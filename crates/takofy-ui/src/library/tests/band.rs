use super::super::*;
use super::test_support::*;
use takofy_model::{Owner, OwnerName};

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
    assert_eq!(band_cover(entry.cover_set(), false), BandCover::Pending);
    assert_eq!(
        band_cover(entry.cover_set(), true),
        BandCover::Source(Some(&CoverSet::single(cover)))
    );
}

#[test]
fn band_cover_is_missing_when_the_entry_names_none() {
    let entry = playlist("a", "A");
    assert_eq!(band_cover(entry.cover_set(), true), BandCover::Source(None));
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
        band_cover(entry.cover_set(), settled),
        BandCover::Source(Some(&CoverSet::single(cover)))
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
        head: None,
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
        band_cover(entry.cover_set(), settled(&library)),
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
        head: None,
    });

    let entry = library.selected_entry().expect("the selected entry");
    assert_eq!(
        band_cover(entry.cover_set(), settled(&library)),
        BandCover::Source(Some(&CoverSet::single(fresh.clone()))),
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
        head: None,
    });

    let entry = library.selected_entry().expect("the selected entry");
    assert_eq!(
        band_cover(entry.cover_set(), settled(&library)),
        BandCover::Source(None),
        "an emptied playlist has no cover, and the band says so rather than waiting"
    );
}

/// A page's own read naming a description, an owner and (when given) a picture.
fn full_head(name: &str, description: &str, owner: &str) -> PlaylistHead {
    PlaylistHead {
        name: name.to_owned(),
        description: Some(description.to_owned()),
        owner: Some(Owner::Named(OwnerName::from_wire(owner).expect("a name"))),
        picture: None,
    }
}

#[test]
fn an_unlisted_page_is_headed_by_its_own_read() {
    let library = headed_page(
        &["spotify:track:t0"],
        None,
        Some(full_head("Pasted", "Windows down", "alice")),
        None,
    );

    let source = library
        .band_source()
        .expect("a page the sidebar does not name is headed by its own read");
    assert_eq!(source.uri, "a");
    assert_eq!(source.name, "Pasted");
    assert_eq!(source.description.as_deref(), Some("Windows down"));
    assert_eq!(source.owner.as_ref().and_then(Owner::name), Some("alice"));
    assert_eq!(source.cover, None);
    assert_eq!(source.origin, None);
}

#[test]
fn a_named_entry_wins_over_the_page_head() {
    let mut library = headed_page(
        &["spotify:track:t0"],
        None,
        Some(full_head("From the page", "The page's line", "bob")),
        None,
    );
    let mut entry = playlist("a", "From the sidebar");
    entry.description = Some("The sidebar's line".to_owned());
    library.apply(&Update::Playlists(vec![entry]));

    let source = library.band_source().expect("the sidebar names it");
    assert_eq!(source.name, "From the sidebar");
    assert_eq!(
        source.description.as_deref(),
        Some("The sidebar's line"),
        "the two sources' fields are never mixed"
    );
    assert_eq!(
        source.owner, None,
        "the entry named no owner, so the head's must not fill the hole"
    );
}

#[test]
fn an_unnamed_entry_yields_to_the_head() {
    let picture = CoverSet::single(cover_url("picture"));
    let mut page = full_head("Pasted", "Windows down", "alice");
    page.picture = Some(picture.clone());
    let mut library = headed_page(&["spotify:track:t0"], None, Some(page), None);
    let mut entry = playlist("a", "a");
    entry.named = false;
    entry.description = Some("The URI-titled row's line".to_owned());
    library.apply(&Update::Playlists(vec![entry]));

    let source = library.band_source().expect("the head names it");
    assert_eq!(
        source.name, "Pasted",
        "an unnamed entry's title is the URI, so it yields entirely"
    );
    assert_eq!(source.description.as_deref(), Some("Windows down"));
    assert_eq!(source.owner.as_ref().and_then(Owner::name), Some("alice"));
    assert_eq!(source.cover, Some(picture));
}

#[test]
fn an_unnamed_entry_with_no_head_has_no_band_source() {
    let mut library = headed_page(&["spotify:track:t0"], None, None, None);
    let mut entry = playlist("a", "a");
    entry.named = false;
    library.apply(&Update::Playlists(vec![entry]));

    assert_eq!(
        library.band_source(),
        None,
        "a URI is not a name to draw above the rows"
    );
}

#[test]
fn a_page_with_no_head_has_no_band_source() {
    let library = headed_page(
        &["spotify:track:t0"],
        None,
        None,
        Some(CoverSet::single(cover_url("context"))),
    );
    assert_eq!(
        library.band_source(),
        None,
        "a read that named no name heads nothing, whatever cover it carried"
    );

    // A page whose own read failed never gets a head, and has no entry to
    // fall back on: it draws no band.
    let mut failed = Library::default();
    failed.select("a");
    failed.apply(&Update::TracksFailed {
        playlist: "a".to_owned(),
        failure: Failure::Load,
    });
    assert_eq!(failed.band_source(), None);
}

#[test]
fn a_head_for_another_page_names_nothing() {
    let mut library = Library::default();
    library.select("a");
    library.tracks = Load::Ready(TracksFor {
        playlist: "b".to_owned(),
        tracks: moved(&["spotify:track:t0"]),
        access: None,
        notice: None,
        head: Some(head("Another")),
        cover: None,
    });

    assert_eq!(
        library.band_source(),
        None,
        "another page's read heads nothing here"
    );
}

#[test]
fn an_unlisted_pages_cover_is_the_replys_cover() {
    let cover = CoverSet::single(cover_url("context"));
    let library = headed_page(
        &["spotify:track:t0"],
        None,
        Some(head("Pasted")),
        Some(cover.clone()),
    );

    let source = library.band_source().expect("the head names it");
    assert_eq!(source.cover, Some(cover));
    assert_eq!(
        source.origin, None,
        "a reply's cover is not a chosen picture"
    );
}

#[test]
fn a_chosen_picture_in_the_head_is_the_cover_and_forbids_a_mosaic() {
    let picture = CoverSet::single(cover_url("picture"));
    let mut page = head("Pasted");
    page.picture = Some(picture.clone());
    let library = headed_page(
        &["spotify:track:t0"],
        None,
        Some(page),
        Some(CoverSet::single(cover_url("context"))),
    );

    let source = library.band_source().expect("the head names it");
    assert_eq!(source.cover, Some(picture), "the user's own picture wins");
    assert_eq!(
        source.origin,
        Some(CoverOrigin::Chosen),
        "a chosen picture is never replaced by the mosaic"
    );
}

#[test]
fn a_head_without_a_picture_falls_back_to_the_replys_cover() {
    let cover = CoverSet::single(cover_url("context"));
    let library = headed_page(
        &["spotify:track:t0"],
        None,
        Some(full_head("Pasted", "Windows down", "alice")),
        Some(cover.clone()),
    );

    let source = library.band_source().expect("the head names it");
    assert_eq!(
        source.cover,
        Some(cover),
        "the head's own fields still stand"
    );
    assert_eq!(
        source.origin, None,
        "the reply's cover is derived, so the mosaic may tile it"
    );
}

#[test]
fn the_band_source_survives_a_playlists_read_that_lists_nothing() {
    let mut library = headed_page(
        &["spotify:track:t0"],
        None,
        Some(full_head("Pasted", "Windows down", "alice")),
        None,
    );

    library.apply(&Update::Playlists(Vec::new()));

    let source = library
        .band_source()
        .expect("a page the sidebar names nowhere is headed by its own read");
    assert_eq!(source.name, "Pasted");
    assert_eq!(source.description.as_deref(), Some("Windows down"));
    assert_eq!(source.owner.as_ref().and_then(Owner::name), Some("alice"));
}

#[test]
fn the_selected_name_falls_back_to_the_page_head() {
    let library = headed_page(&["spotify:track:t0"], None, Some(head("Pasted")), None);
    assert_eq!(library.selected_name(), Some("Pasted"));
}

#[test]
fn an_unnamed_entry_names_no_crumb_without_a_head() {
    let mut library = headed_page(&["spotify:track:t0"], None, None, None);
    let mut entry = playlist("a", "a");
    entry.named = false;
    library.apply(&Update::Playlists(vec![entry]));

    assert_eq!(
        library.selected_name(),
        None,
        "the breadcrumb draws no URI in place of a name"
    );
}
