use super::super::*;
use super::test_support::*;
use takofy_model::CoverUrl;

fn cover_set(tail: &str) -> CoverSet {
    CoverSet::single(cover_url(tail))
}

fn playlist_with_chosen_cover(uri: &str, name: &str, cover: CoverUrl) -> PlaylistEntry {
    covered(uri, name, PlaylistCover::chosen(CoverSet::single(cover)))
}

#[test]
fn a_tracks_cover_upgrades_an_entry_that_already_had_a_cover() {
    let first = cover_url("first");
    let context = cover_url("context");
    let mut library = Library::default();
    library.apply(&Update::Playlists(vec![playlist_with_cover(
        "a", "A", first,
    )]));
    library.apply(&Update::Tracks {
        playlist: "a".to_owned(),
        tracks: vec![track("t")],
        cover: Some(CoverSet::single(context.clone())),
        access: None,
    });

    assert_eq!(
        library.playlists,
        Load::Ready(vec![playlist_with_cover("a", "A", context)])
    );
}

#[test]
fn a_non_awaited_none_cover_leaves_the_entry_alone() {
    let first = cover_url("first");
    let mut library = Library::default();
    library.apply(&Update::Playlists(vec![playlist_with_cover(
        "a",
        "A",
        first.clone(),
    )]));
    library.apply(&Update::Tracks {
        playlist: "a".to_owned(),
        tracks: vec![track("t")],
        cover: None,
        access: None,
    });

    assert_eq!(
        library.playlists,
        Load::Ready(vec![playlist_with_cover("a", "A", first)]),
        "a read that answers no ask cannot say why a cover is missing, so it clears none"
    );
}

#[test]
fn a_tracks_cover_upgrades_the_update_playlist_not_the_selected_one() {
    let a_first = cover_url("a-first");
    let a_context = cover_url("a-context");
    let b_cover = cover_url("b");
    let mut library = Library::default();
    library.apply(&Update::Playlists(vec![
        playlist_with_cover("a", "A", a_first),
        playlist_with_cover("b", "B", b_cover.clone()),
    ]));
    library.select("b");
    library.apply(&Update::Tracks {
        playlist: "a".to_owned(),
        tracks: vec![track("t")],
        cover: Some(CoverSet::single(a_context.clone())),
        access: None,
    });

    assert_eq!(
        library.playlists,
        Load::Ready(vec![
            playlist_with_cover("a", "A", a_context),
            playlist_with_cover("b", "B", b_cover),
        ])
    );
    // A's reply must not fill B's tracks.
    assert_eq!(library.tracks, Load::Loading);
}

#[test]
fn a_cover_arriving_before_playlists_is_applied_after_them() {
    let context = cover_url("context");
    let mut library = Library::default();
    library.apply(&Update::Tracks {
        playlist: "a".to_owned(),
        tracks: Vec::new(),
        cover: Some(CoverSet::single(context.clone())),
        access: None,
    });
    library.apply(&Update::Playlists(vec![playlist("a", "A")]));

    assert_eq!(
        library.playlists,
        Load::Ready(vec![playlist_with_cover("a", "A", context)])
    );
}

#[test]
fn a_playlists_reload_keeps_a_recorded_upgrade() {
    let context = cover_url("context");
    let mut library = Library::default();
    library.apply(&Update::Playlists(vec![playlist("a", "A")]));
    library.apply(&Update::Tracks {
        playlist: "a".to_owned(),
        tracks: Vec::new(),
        cover: Some(CoverSet::single(context.clone())),
        access: None,
    });
    library.apply(&Update::Playlists(vec![playlist("a", "A")]));

    assert_eq!(
        library.playlists,
        Load::Ready(vec![playlist_with_cover("a", "A", context)])
    );
}

#[test]
fn a_removed_playlists_cover_is_pruned() {
    let context = cover_url("context");
    let mut library = Library::default();
    library.apply(&Update::Playlists(vec![playlist("a", "A")]));
    library.apply(&Update::Tracks {
        playlist: "a".to_owned(),
        tracks: Vec::new(),
        cover: Some(CoverSet::single(context)),
        access: None,
    });
    library.apply(&Update::Playlists(vec![playlist("b", "B")]));

    assert!(library.context_covers.is_empty(), "the key must be pruned");
    assert_eq!(library.playlists, Load::Ready(vec![playlist("b", "B")]));
}

/// The composed covers the sidebar row for `uri` holds.
fn entry_composed(library: &Library, uri: &str) -> Vec<CoverSet> {
    sidebar_entry(library, uri)
        .map(|entry| entry.composed.clone())
        .unwrap_or_default()
}

/// A playlist row with a derived cover and a mosaic composed from its head.
fn covered_and_composed(uri: &str, name: &str, cover: CoverUrl, tails: &[&str]) -> PlaylistEntry {
    PlaylistEntry {
        composed: tails.iter().map(|tail| cover_set(tail)).collect(),
        ..covered(uri, name, PlaylistCover::derived(CoverSet::single(cover)))
    }
}

#[test]
fn a_context_image_leaves_the_composed_covers_alone() {
    let first = cover_url("first");
    let context = cover_url("context");
    let mut library = Library {
        playlists: Load::Ready(vec![covered_and_composed(
            "a",
            "A",
            first,
            &["m0", "m1", "m2"],
        )]),
        ..Default::default()
    };

    library.apply(&Update::Tracks {
        playlist: "a".to_owned(),
        tracks: vec![track("t")],
        cover: Some(CoverSet::single(context.clone())),
        access: None,
    });

    assert_eq!(
        entry_cover(&library, "a"),
        Some(PlaylistCover::derived(CoverSet::single(context))),
        "the reply replaces the derived cover"
    );
    assert_eq!(
        entry_composed(&library, "a").len(),
        3,
        "the card keeps tiling what the band still tiles"
    );
}

#[test]
fn an_awaited_empty_reply_leaves_the_composed_covers_alone() {
    let first = cover_url("first");
    let mut library = Library {
        playlists: Load::Ready(vec![covered_and_composed("a", "A", first, &["m0", "m1"])]),
        ..Default::default()
    };
    library.cover_refresh.drop_for("a");
    library.select("a");

    library.apply(&Update::Tracks {
        playlist: "a".to_owned(),
        tracks: Vec::new(),
        cover: None,
        access: None,
    });

    assert_eq!(entry_cover(&library, "a"), None);
    assert_eq!(
        entry_composed(&library, "a").len(),
        2,
        "a playlist with no context picture still tiles its head"
    );
}

#[test]
fn a_cached_context_image_fills_the_row_and_the_mosaic_stays() {
    let first = cover_url("first");
    let context = cover_url("context");
    let mut library = Library {
        playlists: Load::Ready(vec![playlist_with_cover("a", "A", first.clone())]),
        ..Default::default()
    };
    library.apply(&Update::Tracks {
        playlist: "a".to_owned(),
        tracks: vec![track("t")],
        cover: Some(CoverSet::single(context.clone())),
        access: None,
    });

    library.apply(&Update::Playlists(vec![covered_and_composed(
        "a",
        "A",
        first,
        &["m0", "m1"],
    )]));

    assert_eq!(
        entry_cover(&library, "a"),
        Some(PlaylistCover::derived(CoverSet::single(context))),
        "the cached context image fills the row"
    );
    assert_eq!(
        entry_composed(&library, "a").len(),
        2,
        "the head still composes, so the card still tiles"
    );
}

#[test]
fn removing_the_first_track_drops_the_composed_covers() {
    let first = cover_url("first");
    let mut library = covered_page(&first, &["spotify:track:t0", "spotify:track:t1"]);
    if let Load::Ready(playlists) = &mut library.playlists {
        playlists[0].composed = vec![cover_set("m0"), cover_set("m1")];
    }
    assert!(
        library
            .remove_tracks("a", &[0], &moved(&["spotify:track:t0"]))
            .is_some()
    );

    library.apply(&Update::TracksRemoved {
        playlist: "a".to_owned(),
        revision: revision(0x02),
    });

    assert_eq!(entry_cover(&library, "a"), None);
    assert!(
        entry_composed(&library, "a").is_empty(),
        "a head edit drops the mosaic the old head composed"
    );
}

#[test]
fn a_non_awaited_some_cover_is_stored_as_derived() {
    let context = cover_url("context");
    let mut library = Library::default();
    library.apply(&Update::Playlists(vec![playlist("a", "A")]));
    library.apply(&Update::Tracks {
        playlist: "a".to_owned(),
        tracks: vec![track("t")],
        cover: Some(CoverSet::single(context.clone())),
        access: None,
    });

    assert_eq!(
        library.playlists,
        Load::Ready(vec![playlist_with_cover("a", "A", context)]),
        "a read that answers no ask still stores its cover, as derived"
    );
}

#[test]
fn a_non_awaited_some_cover_never_replaces_a_chosen_one() {
    let chosen = cover_url("chosen");
    let context = cover_url("context");
    let mut library = Library::default();
    library.apply(&Update::Playlists(vec![playlist_with_chosen_cover(
        "a",
        "A",
        chosen.clone(),
    )]));
    library.apply(&Update::Tracks {
        playlist: "a".to_owned(),
        tracks: vec![track("t")],
        cover: Some(CoverSet::single(context)),
        access: None,
    });

    assert_eq!(
        library.playlists,
        Load::Ready(vec![playlist_with_chosen_cover("a", "A", chosen)]),
        "a chosen picture is the user's own, so a read must not replace it"
    );
}

#[test]
fn an_awaited_some_cover_is_stored_as_derived() {
    let context = cover_url("context");
    let mut library = Library::default();
    library.apply(&Update::Playlists(vec![playlist("a", "A")]));
    // The row edit that drops a cover is what marks it due; seeding the ask
    // here keeps the reply rule under test on its own.
    library.cover_refresh.drop_for("a");
    library.select("a");
    library.apply(&Update::Tracks {
        playlist: "a".to_owned(),
        tracks: vec![track("t")],
        cover: Some(CoverSet::single(context.clone())),
        access: None,
    });

    assert!(
        !library.cover_refresh.answer("a"),
        "the reply must consume the ask, or an unawaited reply would look awaited"
    );
    assert_eq!(
        library.playlists,
        Load::Ready(vec![playlist_with_cover("a", "A", context)]),
        "a cover that answers an ask is stored as derived"
    );
}

#[test]
fn an_awaited_none_cover_clears_a_derived_cover_and_its_cache_entry() {
    let first = cover_url("first");
    let context = cover_url("context");
    let mut library = Library::default();
    library.apply(&Update::Playlists(vec![playlist_with_cover(
        "a", "A", first,
    )]));
    library.apply(&Update::Tracks {
        playlist: "a".to_owned(),
        tracks: vec![track("t")],
        cover: Some(CoverSet::single(context)),
        access: None,
    });
    assert!(
        library.context_covers.contains_key("a"),
        "the arrange step must cache a context image"
    );

    library.cover_refresh.drop_for("a");
    library.select("a");
    library.apply(&Update::Tracks {
        playlist: "a".to_owned(),
        tracks: Vec::new(),
        cover: None,
        access: None,
    });

    assert_eq!(
        library.playlists,
        Load::Ready(vec![playlist("a", "A")]),
        "the reply to an ask with no cover clears the derived one"
    );
    assert!(
        !library.context_covers.contains_key("a"),
        "the cached image goes with the cover it was drawn from"
    );
}

#[test]
fn an_awaited_none_cover_never_clears_a_chosen_one() {
    let chosen = cover_url("chosen");
    let mut library = Library::default();
    library.apply(&Update::Playlists(vec![playlist_with_chosen_cover(
        "a",
        "A",
        chosen.clone(),
    )]));
    library.cover_refresh.drop_for("a");
    library.select("a");
    library.apply(&Update::Tracks {
        playlist: "a".to_owned(),
        tracks: Vec::new(),
        cover: None,
        access: None,
    });

    assert!(
        !library.cover_refresh.answer("a"),
        "the ask was answered, so the chosen guard is what kept the cover"
    );
    assert_eq!(
        library.playlists,
        Load::Ready(vec![playlist_with_chosen_cover("a", "A", chosen)]),
        "a reply to an ask must not clear a chosen picture"
    );
}

#[test]
fn a_listing_reload_does_not_overlay_a_cached_cover_onto_a_chosen_entry() {
    let context = cover_url("context");
    let chosen = cover_url("chosen");
    let mut library = Library::default();
    library.apply(&Update::Playlists(vec![playlist("a", "A")]));
    library.apply(&Update::Tracks {
        playlist: "a".to_owned(),
        tracks: vec![track("t")],
        cover: Some(CoverSet::single(context)),
        access: None,
    });
    library.apply(&Update::Playlists(vec![playlist_with_chosen_cover(
        "a",
        "A",
        chosen.clone(),
    )]));

    assert_eq!(
        library.playlists,
        Load::Ready(vec![playlist_with_chosen_cover("a", "A", chosen)]),
        "a reload must not draw a cached derived image over a chosen picture"
    );
}

#[test]
fn a_removed_playlist_is_forgotten_by_cover_refresh() {
    let mut library = sidebar_library(&["a", "b"], 0x07, true, None);
    library.cover_refresh.drop_for("a");
    library.cover_refresh.asked("a");
    library.cover_refresh.drop_for("a");
    assert!(
        library
            .remove_from_library(&item(SidebarKind::Playlists, "a"))
            .is_some()
    );
    library.apply(&Update::PlaylistRemoved {
        playlist: "a".to_owned(),
        revision: revision(0x09),
    });

    assert!(
        library
            .cover_refresh
            .take_due(&names(&["a"]), None)
            .is_none(),
        "a removed playlist must not stay due for a cover"
    );
    assert!(
        !library.cover_refresh.answer("a"),
        "a removed playlist must not still be awaiting a cover"
    );
}

#[test]
fn a_removed_playlist_takes_its_cached_cover_with_it() {
    let context = cover_url("context");
    let mut library = sidebar_library(&["a", "b"], 0x07, true, None);
    library.apply(&Update::Tracks {
        playlist: "a".to_owned(),
        tracks: Vec::new(),
        cover: Some(CoverSet::single(context)),
        access: None,
    });
    assert!(
        library.context_covers.contains_key("a"),
        "arrange: the reply must cache a cover for the row"
    );

    assert!(
        library
            .remove_from_library(&item(SidebarKind::Playlists, "a"))
            .is_some()
    );
    library.apply(&Update::PlaylistRemoved {
        playlist: "a".to_owned(),
        revision: revision(0x09),
    });

    assert!(
        !library.context_covers.contains_key("a"),
        "a cached cover outlived the row it belonged to"
    );
}

#[test]
fn a_playlists_read_prunes_cover_refresh() {
    let mut library = Library::default();
    library.apply(&Update::Playlists(vec![
        playlist("a", "A"),
        playlist("b", "B"),
    ]));
    library.cover_refresh.drop_for("b");
    library.apply(&Update::Playlists(vec![playlist("a", "A")]));

    assert!(
        library
            .cover_refresh
            .take_due(&names(&["b"]), None)
            .is_none(),
        "a playlist the read no longer lists must not stay due"
    );
}

#[test]
fn a_due_playlist_selected_answers_its_own_cover() {
    let first = cover_url("first");
    let mut library = Library::default();
    library.apply(&Update::Playlists(vec![playlist_with_cover(
        "a", "A", first,
    )]));
    library.cover_refresh.drop_for("a");
    library.select("a");

    // Only the reply to an ask clears a cover, so this clear proves the
    // navigation's own read was adopted as the answer.
    library.apply(&Update::Tracks {
        playlist: "a".to_owned(),
        tracks: Vec::new(),
        cover: None,
        access: None,
    });

    assert_eq!(
        library.playlists,
        Load::Ready(vec![playlist("a", "A")]),
        "the read a navigation issued answers its own ask"
    );
}

#[test]
fn a_failed_read_releases_the_ask() {
    let first = cover_url("first");
    let mut library = Library::default();
    library.apply(&Update::Playlists(vec![playlist_with_cover(
        "a",
        "A",
        first.clone(),
    )]));
    library.cover_refresh.drop_for("a");
    library.select("a");
    library.apply(&Update::TracksFailed {
        playlist: "a".to_owned(),
        failure: Failure::Load,
    });
    library.apply(&Update::Tracks {
        playlist: "a".to_owned(),
        tracks: Vec::new(),
        cover: None,
        access: None,
    });

    assert_eq!(
        library.playlists,
        Load::Ready(vec![playlist_with_cover("a", "A", first)]),
        "the failure answered the ask, so the next reply is not awaited and clears nothing"
    );
}

#[test]
fn an_awaited_reply_while_the_listing_loads_still_fills_the_cache() {
    let old = cover_url("old");
    let context = cover_url("context");
    let mut library = Library::default();
    library.apply(&Update::Playlists(vec![playlist_with_cover(
        "a",
        "A",
        old.clone(),
    )]));
    library.cover_refresh.drop_for("a");
    library.select("a");
    library.reload();
    assert_eq!(library.playlists, Load::Loading);

    library.apply(&Update::Tracks {
        playlist: "a".to_owned(),
        tracks: vec![track("t")],
        cover: Some(CoverSet::single(context.clone())),
        access: None,
    });
    assert_eq!(
        library.context_covers.get("a"),
        Some(&CoverSet::single(context.clone())),
        "an awaited reply fills the cache even with no list to write to"
    );

    library.apply(&Update::Playlists(vec![playlist_with_cover("a", "A", old)]));

    assert_eq!(
        library.playlists,
        Load::Ready(vec![playlist_with_cover("a", "A", context)]),
        "the listing's own cover is older than the reply, so the cache fills the row"
    );
}

#[test]
fn removing_the_first_track_drops_the_cover_and_asks_for_it_again() {
    let first = cover_url("first");
    let mut library = covered_page(&first, &["spotify:track:t0", "spotify:track:t1"]);
    assert!(
        library
            .remove_tracks("a", &[0], &moved(&["spotify:track:t0"]))
            .is_some()
    );

    library.apply(&Update::TracksRemoved {
        playlist: "a".to_owned(),
        revision: revision(0x02),
    });

    assert_eq!(
        entry_cover(&library, "a"),
        None,
        "the cover was taken from a track the list no longer starts with"
    );
    assert_eq!(
        library.tracks_request(),
        Some(Command::LoadTracks {
            playlist: "a".to_owned()
        }),
        "a dropped cover is read again"
    );
}

#[test]
fn removing_a_later_row_keeps_the_cover_and_asks_nothing() {
    let first = cover_url("first");
    let mut library = covered_page(
        &first,
        &["spotify:track:t0", "spotify:track:t1", "spotify:track:t2"],
    );
    assert!(
        library
            .remove_tracks("a", &[1], &moved(&["spotify:track:t1"]))
            .is_some()
    );

    library.apply(&Update::TracksRemoved {
        playlist: "a".to_owned(),
        revision: revision(0x02),
    });

    assert_eq!(
        entry_cover(&library, "a"),
        Some(PlaylistCover::derived(CoverSet::single(first))),
        "the first track is still the one the cover came from"
    );
    assert_eq!(
        library.tracks_request(),
        None,
        "a cover that is still right must not be read again"
    );
}

#[test]
fn moving_a_row_to_the_front_drops_the_cover() {
    let first = cover_url("first");
    let mut library = covered_page(
        &first,
        &["spotify:track:t0", "spotify:track:t1", "spotify:track:t2"],
    );
    assert!(move_one(&mut library, "a", 2, 0, &track("spotify:track:t2")).is_some());

    library.apply(&Update::TrackMoved {
        playlist: "a".to_owned(),
        revision: revision(0x02),
    });

    assert_eq!(
        entry_cover(&library, "a"),
        None,
        "the list now starts with another track, so its cover is wrong"
    );
    assert_eq!(
        library.tracks_request(),
        Some(Command::LoadTracks {
            playlist: "a".to_owned()
        }),
        "a move that changes the first track drops and re-asks the cover"
    );
}

#[test]
fn a_move_that_keeps_the_first_track_keeps_the_cover() {
    let first = cover_url("first");
    let mut library = covered_page(
        &first,
        &["spotify:track:t0", "spotify:track:t1", "spotify:track:t2"],
    );
    assert!(move_one(&mut library, "a", 1, 3, &track("spotify:track:t1")).is_some());

    library.apply(&Update::TrackMoved {
        playlist: "a".to_owned(),
        revision: revision(0x02),
    });

    assert_eq!(
        entry_cover(&library, "a"),
        Some(PlaylistCover::derived(CoverSet::single(first))),
        "rows two and three swapped, so the first track the cover came from is unchanged"
    );
    assert_eq!(
        library.tracks_request(),
        None,
        "a move that keeps the first track asks for nothing"
    );
}

#[test]
fn an_episode_in_front_does_not_count_as_the_first_track_in_a_move() {
    let first = cover_url("first");
    let mut library = covered_page(
        &first,
        &["spotify:episode:e", "spotify:track:t0", "spotify:track:t1"],
    );
    assert!(move_one(&mut library, "a", 2, 1, &track("spotify:track:t1")).is_some());

    library.apply(&Update::TrackMoved {
        playlist: "a".to_owned(),
        revision: revision(0x02),
    });

    assert_eq!(
        entry_cover(&library, "a"),
        None,
        "a cover comes from the first track, so a move past an episode still changes it"
    );
    assert_eq!(
        library.tracks_request(),
        Some(Command::LoadTracks {
            playlist: "a".to_owned()
        }),
        "the first track changed, so the cover is read again"
    );
}

#[test]
fn an_add_to_an_empty_playlist_asks_for_its_cover() {
    let mut library = sidebar_library(&["a", "b"], 0x07, true, Some(0));
    // An add is made from another page, so the target's own rows are not on
    // screen to say which track came first.
    library.open(album("spotify:album:x"));
    assert!(library.add_tracks(0, &pick(&["spotify:track:x"])).is_some());

    library.apply(&Update::TrackAdded {
        playlist: "a".to_owned(),
        revision: revision(0x08),
    });

    assert_eq!(
        entry_cover(&library, "a"),
        None,
        "an empty list had no cover for the add to keep"
    );
    assert_eq!(
        library.tracks_request(),
        Some(Command::LoadTracks {
            playlist: "a".to_owned()
        }),
        "a list an add filled needs its first cover read"
    );
}

#[test]
fn an_add_to_a_playlist_with_a_cover_keeps_it() {
    let first = cover_url("first");
    let mut library = sidebar_library(&["a", "b"], 0x07, true, Some(2));
    library.playlists = Load::Ready(vec![
        with_access(
            playlist_with_cover("a", "A", first.clone()),
            0x07,
            true,
            Some(2),
        ),
        with_access(playlist("b", "B"), 0x07, true, Some(2)),
    ]);
    library.open(album("spotify:album:x"));
    assert!(library.add_tracks(0, &pick(&["spotify:track:x"])).is_some());

    library.apply(&Update::TrackAdded {
        playlist: "a".to_owned(),
        revision: revision(0x08),
    });

    assert_eq!(
        entry_cover(&library, "a"),
        Some(PlaylistCover::derived(CoverSet::single(first))),
        "an append names the end of the list, so it cannot change its first track"
    );
    assert_eq!(
        library.tracks_request(),
        None,
        "a cover an add kept is not read again"
    );
}

#[test]
fn a_chosen_cover_survives_add_remove_and_move_and_asks_nothing() {
    let chosen = cover_url("chosen");
    let mut library = page_with(
        "a",
        &["spotify:track:t0", "spotify:track:t1", "spotify:track:t2"],
        access(0x01, true),
    );
    library.playlists = Load::Ready(vec![with_access(
        playlist_with_chosen_cover("a", "A", chosen.clone()),
        0x07,
        true,
        Some(3),
    )]);

    assert!(
        library
            .remove_tracks("a", &[0], &moved(&["spotify:track:t0"]))
            .is_some()
    );
    library.apply(&Update::TracksRemoved {
        playlist: "a".to_owned(),
        revision: revision(0x02),
    });
    assert_eq!(
        entry_cover(&library, "a"),
        Some(PlaylistCover::chosen(CoverSet::single(chosen.clone()))),
        "a chosen picture is the user's own, so a removal cannot drop it"
    );

    assert!(move_one(&mut library, "a", 1, 0, &track("spotify:track:t2")).is_some());
    library.apply(&Update::TrackMoved {
        playlist: "a".to_owned(),
        revision: revision(0x03),
    });
    assert_eq!(
        entry_cover(&library, "a"),
        Some(PlaylistCover::chosen(CoverSet::single(chosen.clone()))),
        "a chosen picture is the user's own, so a move cannot drop it"
    );

    library.open(album("spotify:album:x"));
    assert!(library.add_tracks(0, &pick(&["spotify:track:y"])).is_some());
    library.apply(&Update::TrackAdded {
        playlist: "a".to_owned(),
        revision: revision(0x08),
    });
    assert_eq!(
        entry_cover(&library, "a"),
        Some(PlaylistCover::chosen(CoverSet::single(chosen))),
        "a chosen picture is the user's own, so an add cannot drop it"
    );
    assert_eq!(
        library.tracks_request(),
        None,
        "a chosen picture is never read over"
    );
}

#[test]
fn a_reply_with_no_pending_write_drops_nothing() {
    let first = cover_url("first");
    let mut library = covered_page(&first, &["spotify:track:t0", "spotify:track:t1"]);

    // No removal was begun, so this reply names no rows to compare.
    library.apply(&Update::TracksRemoved {
        playlist: "a".to_owned(),
        revision: revision(0x02),
    });

    assert_eq!(
        entry_cover(&library, "a"),
        Some(PlaylistCover::derived(CoverSet::single(first))),
        "a reply with no write pending cannot say a row changed"
    );
    assert_eq!(
        library.tracks_request(),
        None,
        "a reply that drops nothing asks for nothing"
    );
}

#[test]
fn a_reply_for_another_playlist_does_not_drop_a_pending_writes_cover() {
    let first = cover_url("first");
    let mut library = covered_page(&first, &["spotify:track:t0", "spotify:track:t1"]);
    assert!(
        library
            .remove_tracks("a", &[0], &moved(&["spotify:track:t0"]))
            .is_some()
    );

    // Neither reply has a write of its own on the list that holds the
    // cover, so neither can say a row of it changed.
    library.apply(&Update::TrackMoved {
        playlist: "b".to_owned(),
        revision: revision(0x09),
    });
    library.apply(&Update::TracksRemoved {
        playlist: "b".to_owned(),
        revision: revision(0x09),
    });

    assert_eq!(
        entry_cover(&library, "a"),
        Some(PlaylistCover::derived(CoverSet::single(first))),
        "a reply for another playlist cannot drop the pending write's cover"
    );
    assert!(
        library.pending.is_some(),
        "the write for a is still in flight"
    );
    assert_eq!(
        library.tracks_request(),
        None,
        "a reply that drops nothing asks for nothing"
    );
}

#[test]
fn a_move_with_no_write_pending_drops_nothing() {
    let first = cover_url("first");
    let mut library = covered_page(&first, &["spotify:track:t0", "spotify:track:t1"]);

    // No move was begun, so this reply names no rows to compare.
    library.apply(&Update::TrackMoved {
        playlist: "a".to_owned(),
        revision: revision(0x02),
    });

    assert_eq!(
        entry_cover(&library, "a"),
        Some(PlaylistCover::derived(CoverSet::single(first))),
        "a move reply with no write pending cannot say a row changed"
    );
    assert_eq!(
        library.tracks_request(),
        None,
        "a reply that drops nothing asks for nothing"
    );
}

#[test]
fn an_add_without_a_matching_pending_add_drops_nothing() {
    let mut library = sidebar_library(&["a", "b"], 0x07, true, Some(0));

    // The row shows no cover, so a hook placed before the guard would ask.
    library.apply(&Update::TrackAdded {
        playlist: "a".to_owned(),
        revision: revision(0x08),
    });

    assert_eq!(
        entry_cover(&library, "a"),
        None,
        "a reply naming no add of its own changes no cover"
    );
    assert_eq!(
        library.tracks_request(),
        None,
        "a reply with no add pending asks for nothing"
    );
}

#[test]
fn an_edit_on_the_open_page_asks_for_the_cover_without_touching_wanted_or_rows() {
    let first = cover_url("first");
    let mut library = covered_page(&first, &["spotify:track:t0", "spotify:track:t1"]);
    assert!(
        library
            .remove_tracks("a", &[0], &moved(&["spotify:track:t0"]))
            .is_some()
    );
    library.apply(&Update::TracksRemoved {
        playlist: "a".to_owned(),
        revision: revision(0x02),
    });

    assert_eq!(
        library.tracks_request(),
        Some(Command::LoadTracks {
            playlist: "a".to_owned()
        }),
        "the dropped cover is asked for"
    );
    assert_eq!(
        library.wanted.as_deref(),
        Some("a"),
        "a cover read for a listed playlist must not take the page's want"
    );
    assert_eq!(
        library.tracks,
        ready("a", &["spotify:track:t1"], access(0x02, true)),
        "a bare cover read must not blank the rows on screen"
    );
}

#[test]
fn an_add_while_the_page_is_idle_asks_once_and_marks_the_page_loading() {
    let first = cover_url("first");
    let mut library = page_dropped_by_an_add(Some(&first));
    assert_eq!(
        library.tracks,
        Load::Idle,
        "arrange: the add dropped the page"
    );

    assert_eq!(
        library.tracks_request(),
        Some(Command::LoadTracks {
            playlist: "a".to_owned()
        }),
        "a page a write left blank is asked for once"
    );
    assert_eq!(
        library.tracks,
        Load::Loading,
        "the ask marks the page loading, which is what stops a second one"
    );
    assert_eq!(
        library.tracks_request(),
        None,
        "a page already loading is not asked for twice"
    );
}

#[test]
fn an_add_while_the_page_is_loading_resets_it_and_the_re_ask_wins() {
    let first = cover_url("first");
    let mut library = page_dropped_by_an_add(Some(&first));
    assert!(library.tracks_request().is_some(), "arrange: the re-ask");

    // The read taken before the add, arriving after the re-ask was issued.
    library.apply(&Update::Tracks {
        playlist: "a".to_owned(),
        tracks: moved(&["spotify:track:t0", "spotify:track:t1"]),
        cover: Some(CoverSet::single(first.clone())),
        access: access(0x01, true),
    });
    assert_eq!(
        library.tracks,
        ready(
            "a",
            &["spotify:track:t0", "spotify:track:t1"],
            access(0x01, true)
        ),
        "arrange: the pre-add reply lands as the page first, race and all"
    );
    // The re-ask's own reply, which is last and therefore the page.
    library.apply(&Update::Tracks {
        playlist: "a".to_owned(),
        tracks: moved(&["spotify:track:t0", "spotify:track:t1", "spotify:track:x"]),
        cover: Some(CoverSet::single(first)),
        access: access(0x08, true),
    });

    assert_eq!(
        library.tracks,
        ready(
            "a",
            &["spotify:track:t0", "spotify:track:t1", "spotify:track:x"],
            access(0x08, true)
        ),
        "the reply to the newest read is the page, whatever arrived in between"
    );
}

#[test]
fn a_failed_page_is_never_asked_automatically() {
    let mut library = sidebar_library(&["a", "b"], 0x07, true, Some(2));
    library.select("a");
    library.apply(&Update::TracksFailed {
        playlist: "a".to_owned(),
        failure: Failure::Load,
    });

    assert_eq!(
        library.tracks_request(),
        None,
        "a failed page is left for its Retry, never read behind the user's back"
    );
    assert_eq!(
        library.tracks,
        Load::Failed(Failure::Load),
        "and the failure stands for the Retry to see"
    );
}

#[test]
fn a_failed_cover_re_ask_leaves_the_cover_dropped_and_does_not_ask_again() {
    let first = cover_url("first");
    let mut library = covered_page(&first, &["spotify:track:t0", "spotify:track:t1"]);
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

    library.apply(&Update::TracksFailed {
        playlist: "a".to_owned(),
        failure: Failure::Load,
    });

    assert_eq!(
        entry_cover(&library, "a"),
        None,
        "a failed re-read leaves the cover dropped rather than restoring a wrong one"
    );
    assert_eq!(
        library.tracks_request(),
        None,
        "a failed ask is not retried, so the hole cannot loop"
    );
    library.apply(&Update::Tracks {
        playlist: "a".to_owned(),
        tracks: moved(&["spotify:track:t1"]),
        cover: None,
        access: access(0x02, true),
    });
    assert_eq!(
        entry_cover(&library, "a"),
        None,
        "a later reply that answers no ask cannot say why the cover is missing"
    );
}

#[test]
fn a_removed_or_unlisted_playlist_is_never_asked() {
    let first = cover_url("first");
    let mut unlisted = covered_page(&first, &["spotify:track:t0", "spotify:track:t1"]);
    assert!(
        unlisted
            .remove_tracks("a", &[0], &moved(&["spotify:track:t0"]))
            .is_some()
    );
    unlisted.apply(&Update::TracksRemoved {
        playlist: "a".to_owned(),
        revision: revision(0x02),
    });
    unlisted.apply(&Update::Playlists(Vec::new()));

    assert_eq!(
        unlisted.tracks_request(),
        None,
        "a playlist the list no longer names is never asked for"
    );

    let mut removed = covered_page(&first, &["spotify:track:t0", "spotify:track:t1"]);
    removed.playlists = Load::Ready(vec![
        with_access(
            playlist_with_cover("a", "A", first.clone()),
            0x07,
            true,
            Some(2),
        ),
        with_access(playlist("b", "B"), 0x07, true, Some(1)),
    ]);
    removed.apply(&Update::RootlistAccess(rootlist_access(0x07, true)));
    assert!(
        removed
            .remove_tracks("a", &[0], &moved(&["spotify:track:t0"]))
            .is_some()
    );
    removed.apply(&Update::TracksRemoved {
        playlist: "a".to_owned(),
        revision: revision(0x02),
    });
    assert!(
        removed
            .remove_from_library(&item(SidebarKind::Playlists, "a"))
            .is_some()
    );
    removed.apply(&Update::PlaylistRemoved {
        playlist: "a".to_owned(),
        revision: revision(0x09),
    });

    assert_eq!(
        removed.tracks_request(),
        None,
        "a playlist the user removed is never asked for"
    );
}

#[test]
fn a_pre_edit_reply_arriving_after_the_edit_is_overwritten_by_the_re_ask() {
    let first = cover_url("first");
    let stale = cover_url("stale");
    let mut library = covered_page(
        &first,
        &["spotify:track:t0", "spotify:track:t1", "spotify:track:t2"],
    );
    assert!(
        library
            .remove_tracks("a", &[0], &moved(&["spotify:track:t0"]))
            .is_some()
    );
    library.apply(&Update::TracksRemoved {
        playlist: "a".to_owned(),
        revision: revision(0x02),
    });

    // The read taken before the removal answers no ask, so it is stored
    // with the image it was taken with.
    library.apply(&Update::Tracks {
        playlist: "a".to_owned(),
        tracks: moved(&["spotify:track:t0", "spotify:track:t1", "spotify:track:t2"]),
        cover: Some(CoverSet::single(stale.clone())),
        access: access(0x01, true),
    });
    assert_eq!(
        entry_cover(&library, "a"),
        Some(PlaylistCover::derived(CoverSet::single(stale))),
        "arrange: the pre-edit reply is stored before the re-ask is issued"
    );

    assert_eq!(
        library.tracks_request(),
        Some(Command::LoadTracks {
            playlist: "a".to_owned()
        }),
        "arrange: the dropped cover is asked for again"
    );

    // The re-read answers the ask and finds no cover at all, and only that
    // reply may say so.
    library.apply(&Update::Tracks {
        playlist: "a".to_owned(),
        tracks: moved(&["spotify:track:t1", "spotify:track:t2"]),
        cover: None,
        access: access(0x02, true),
    });

    assert_eq!(
        entry_cover(&library, "a"),
        None,
        "the awaited answer's `None` clears the stale cover the pre-edit reply stored"
    );
}

#[test]
fn two_quick_removals_end_with_the_last_answer() {
    let first = cover_url("first");
    let stale = cover_url("stale");
    let mut library = covered_page(
        &first,
        &["spotify:track:t0", "spotify:track:t1", "spotify:track:t2"],
    );
    assert!(
        library
            .remove_tracks("a", &[0], &moved(&["spotify:track:t0"]))
            .is_some()
    );
    library.apply(&Update::TracksRemoved {
        playlist: "a".to_owned(),
        revision: revision(0x02),
    });
    assert!(library.tracks_request().is_some(), "arrange: the first ask");

    assert!(
        library
            .remove_tracks("a", &[0], &moved(&["spotify:track:t1"]))
            .is_some()
    );
    library.apply(&Update::TracksRemoved {
        playlist: "a".to_owned(),
        revision: revision(0x03),
    });
    assert!(
        library.tracks_request().is_some(),
        "arrange: the second ask"
    );

    // The older ask's reply lands first, and the newer one has the last word.
    library.apply(&Update::Tracks {
        playlist: "a".to_owned(),
        tracks: moved(&["spotify:track:t1", "spotify:track:t2"]),
        cover: Some(CoverSet::single(stale)),
        access: access(0x02, true),
    });
    library.apply(&Update::Tracks {
        playlist: "a".to_owned(),
        tracks: moved(&["spotify:track:t2"]),
        cover: None,
        access: access(0x03, true),
    });

    assert_eq!(
        entry_cover(&library, "a"),
        None,
        "the last answer said the list has no cover, so none is drawn"
    );
}

#[test]
fn draining_tracks_request_terminates_with_several_due_playlists() {
    let first = cover_url("first");
    let mut library = covered_page(&first, &["spotify:track:t0", "spotify:track:t1"]);
    let mut more = match &library.playlists {
        Load::Ready(playlists) => playlists.clone(),
        Load::Idle | Load::Loading | Load::Failed(_) => Vec::new(),
    };
    more.push(with_access(playlist("b", "B"), 0x07, true, Some(1)));
    more.push(with_access(playlist("c", "C"), 0x07, true, Some(1)));
    library.playlists = Load::Ready(more);
    library.cover_refresh.drop_for("b");
    library.cover_refresh.drop_for("c");

    let mut asked = Vec::new();
    for _ in 0..8 {
        match library.tracks_request() {
            Some(command) => asked.push(command),
            None => break,
        }
    }

    assert_eq!(
        asked,
        vec![
            Command::LoadTracks {
                playlist: "b".to_owned()
            },
            Command::LoadTracks {
                playlist: "c".to_owned()
            },
        ],
        "each due playlist is asked for exactly once"
    );
    assert_eq!(
        library.tracks_request(),
        None,
        "the drain ends rather than spinning"
    );
}

#[test]
fn the_shell_and_tracks_request_never_both_ask_for_one_page() {
    let first = cover_url("first");
    let mut library = page_dropped_by_an_add(Some(&first));

    // The shell's own read goes first, and it marks the page loading.
    assert_eq!(
        library.open(Page::Playlist("a".to_owned())),
        Some(Command::LoadTracks {
            playlist: "a".to_owned()
        }),
        "arrange: the shell asks for the page"
    );

    assert_eq!(
        library.tracks_request(),
        None,
        "a page the shell already asked for must not be asked for twice"
    );
}

#[test]
fn a_created_playlist_keeps_its_cover_and_origin_through_a_later_add() {
    let cover = cover_url("cover");
    let new = "spotify:playlist:new";
    let mut library = naming_library(&["a"], "Mix");
    assert!(library.create_playlist().is_some());
    library.apply(&Update::PlaylistCreated {
        entry: with_access(
            covered(
                new,
                "Mix",
                PlaylistCover::derived(CoverSet::single(cover.clone())),
            ),
            0x08,
            true,
            Some(0),
        ),
        revision: revision(0x08),
    });
    assert_eq!(
        entry_cover(&library, new),
        Some(PlaylistCover::derived(CoverSet::single(cover.clone()))),
        "arrange: the created row carries the cover its reply named"
    );

    library.open(album("spotify:album:x"));
    assert!(library.add_tracks(0, &pick(&["spotify:track:x"])).is_some());
    library.apply(&Update::TrackAdded {
        playlist: new.to_owned(),
        revision: revision(0x09),
    });

    assert_eq!(
        entry_cover(&library, new),
        Some(PlaylistCover::derived(CoverSet::single(cover))),
        "an append cannot change which track a cover came from"
    );
    assert_eq!(
        library.tracks_request(),
        None,
        "so the new list's cover is not read again"
    );
}
