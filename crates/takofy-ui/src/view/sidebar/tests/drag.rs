use super::super::*;
use super::test_support::*;

use egui::Event;

use takofy_model::{Owner, OwnerName};

use crate::message::Update;
use crate::view::test_support::click_events;

#[test]
fn a_drag_on_a_playlist_row_lifts_it_when_the_rootlist_is_movable() {
    let ctx = context();
    let mut library = movable_playlists(&["A", "B"], 7, true, None);
    let mut art = Art::default();
    let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);
    let from = name_line(&output, "B");

    let _ = hold_from(&ctx, from, &mut library, &mut art, true);

    let carry = carry::carried(&ctx).expect("a payload");
    assert_eq!(carry.source, Source::Playlist);
    assert_eq!(carry.rows, vec![1]);
    assert_eq!(carry.playlist.as_deref(), Some("spotify:playlist:B"));
    assert!(carry.entries.is_empty(), "a playlist carries no entries");
}

#[test]
fn a_drag_does_not_lift_when_the_rootlist_is_not_movable() {
    // No access, not editable, and a write in flight all refuse the lift.
    let mut cases: Vec<(&str, Library)> = Vec::new();

    let mut no_access = Library::default();
    no_access.apply(&Update::Playlists(vec![
        playlist("spotify:playlist:A", "A"),
        playlist("spotify:playlist:B", "B"),
    ]));
    cases.push(("no access", no_access));
    cases.push((
        "not editable",
        movable_playlists(&["A", "B"], 7, false, None),
    ));

    let mut pending = movable_playlists(&["A", "B"], 7, true, None);
    assert!(pending.move_playlist(0, 2, "spotify:playlist:A").is_some());
    cases.push(("write in flight", pending));

    for (why, mut library) in cases {
        let ctx = context();
        let mut art = Art::default();
        let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);
        let from = name_line(&output, "B");

        let _ = hold_from(&ctx, from, &mut library, &mut art, true);

        assert!(carry::carried(&ctx).is_none(), "{why} must not lift");
    }
}

#[test]
fn the_non_drag_form_never_lifts() {
    let ctx = context();
    let mut library = movable_playlists(&["A", "B"], 7, true, None);
    let mut art = Art::default();
    let (output, _) = run(&ctx, Vec::new(), &mut library, &mut art);
    let from = name_line(&output, "B");

    let mut events = crate::view::test_support::drag_events(from, from + Vec2::new(0.0, 60.0));
    events.pop();
    for event in events {
        let _ = run(&ctx, vec![event], &mut library, &mut art);
    }

    assert!(
        carry::carried(&ctx).is_none(),
        "the drawer's form discards its command, so it never starts a write"
    );
    assert_eq!(carry::take_column_offer(&ctx), None);
}

#[test]
fn other_kinds_never_lift_or_offer() {
    for kind in [
        SidebarKind::Podcasts,
        SidebarKind::Artists,
        SidebarKind::Albums,
    ] {
        let ctx = context();
        let mut library = movable_playlists(&["A"], 7, true, None);
        library.apply(&Update::FollowedShows(vec![followed_show(
            "spotify:show:s",
            "A show",
        )]));
        library.apply(&Update::FollowedArtists(vec![followed_artist(
            "spotify:artist:a",
            "An artist",
        )]));
        library.apply(&Update::SavedAlbums(vec![saved_album(
            "spotify:album:a",
            "An album",
            "Vela",
        )]));
        assert!(library.sidebar_kind.choose(kind));
        let mut art = Art::default();
        let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);
        let label = match kind {
            SidebarKind::Podcasts => "A show",
            SidebarKind::Artists => "An artist",
            SidebarKind::Albums => "An album",
            SidebarKind::Playlists => unreachable!(),
        };
        let from = name_line(&output, label);

        let _ = hold_from(&ctx, from, &mut library, &mut art, true);

        assert!(carry::carried(&ctx).is_none(), "{kind:?} must not lift");
        assert_eq!(
            carry::take_column_offer(&ctx),
            None,
            "{kind:?} is no drop target"
        );
    }
}

#[test]
fn a_click_still_opens_the_playlist() {
    let ctx = context();
    let mut library = movable_playlists(&["A", "B"], 7, true, None);
    let mut art = Art::default();
    let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);
    let at = name_line(&output, "A");

    let mut command = None;
    for event in click_events(at) {
        let (_, next) = run_column(&ctx, vec![event], &mut library, &mut art);
        if next.is_some() {
            command = next;
        }
    }

    assert_eq!(
        command,
        Some(Command::LoadTracks {
            playlist: "spotify:playlist:A".to_owned()
        })
    );
}

#[test]
fn the_playlists_list_offers_its_rows_at_the_pitch_it_drew() {
    let ctx = context();
    let mut library = movable_playlists(&["A", "B", "C"], 7, true, None);
    let mut art = Art::default();
    let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);

    let offer = carry::take_column_offer(&ctx).expect("the column offers itself");

    assert_eq!(offer.drawn, ROW_H);
    assert_eq!(offer.zone.row_h, ROW_H + space::PX);
    assert_eq!(offer.zone.count, 3);
    assert_eq!(offer.zone.first, 0);

    let pitch = ROW_H + space::PX;
    for (row, label) in ["A", "B", "C"].into_iter().enumerate() {
        let top = row_top(&output, label, "Playlist");
        assert!(
            (offer.zone.list.top() + row as f32 * pitch - top).abs() < 0.01,
            "row {row} starts {row} pitches below the list: {} vs {top}",
            offer.zone.list.top()
        );
    }
}

#[test]
fn the_offers_origin_matches_a_drawn_row() {
    let ctx = context();
    let mut library = movable_playlists(&["A", "B", "C"], 7, true, None);
    let mut art = Art::default();
    let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);
    let offer = carry::take_column_offer(&ctx).expect("the column offers itself");

    // The offer's own formula is `painted top - first index * pitch`, which
    // is asserted here against what the painter drew rather than by driving
    // a scroll, so it holds at any offset.
    let painted = row_top(&output, "A", "Playlist");
    assert!((offer.zone.list.top() - painted).abs() < 0.01);
    assert!(
        offer.zone.list.width() > 0.0,
        "the rule a drop draws spans the rows"
    );
}

#[test]
fn a_list_that_can_not_be_reordered_still_offers_its_rows() {
    let ctx = context();
    let mut library = Library::default();
    library.apply(&Update::Playlists(vec![
        playlist("spotify:playlist:A", "A"),
        playlist("spotify:playlist:B", "B"),
    ]));
    assert!(!library.rootlist_movable());
    let mut art = Art::default();

    let _ = run_column(&ctx, Vec::new(), &mut library, &mut art);

    let offer = carry::take_column_offer(&ctx).expect("the column offers its rows");
    assert_eq!(offer.zone.count, 2);
}

#[test]
fn a_list_that_is_not_ready_offers_nothing() {
    let ctx = context();
    let mut library = Library::default();
    library.playlists = Load::Loading;
    let mut art = Art::default();

    let _ = run_column(&ctx, Vec::new(), &mut library, &mut art);

    assert_eq!(carry::take_column_offer(&ctx), None);
}

#[test]
fn an_addable_row_reads_its_track_count_and_an_other_row_its_owner() {
    let library = movable_playlists(&["A"], 7, true, Some(4));
    let Load::Ready(playlists) = &library.playlists else {
        panic!("the list is ready");
    };
    assert_eq!(ListItem::kind(&playlists[0]), "Playlist · 4 tracks");

    let mut ours = playlist("spotify:playlist:O", "O");
    ours.owner = Some(Owner::You);
    assert_eq!(
        ListItem::kind(&ours),
        "Playlist",
        "our own list draws the kind word alone"
    );

    let unnamed = playlist("spotify:playlist:N", "N");
    assert_eq!(ListItem::kind(&unnamed), "Playlist");

    let mut named = playlist("spotify:playlist:V", "V");
    named.owner = Some(Owner::Named(
        OwnerName::from_wire("Vela Moreno").expect("a name"),
    ));
    assert_eq!(
        ListItem::kind(&named),
        "Playlist · Vela Moreno",
        "a playlist that cannot take an add keeps its owner line"
    );
}

#[test]
fn the_failure_line_names_what_could_not_be_done() {
    let ctx = context();
    let mut library = movable_playlists(&["A", "B"], 7, true, None);
    library.sidebar_notice = Some(crate::library::SidebarWrite::Reorder);
    let mut art = Art::default();

    let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);

    assert!(
        painted(&output, "Could not move that playlist. Try again."),
        "the list says what could not be done"
    );
}

#[test]
fn no_row_washes_while_a_block_is_carried() {
    let ctx = context();
    let mut library = movable_playlists(&["A", "B"], 7, true, None);
    let mut art = Art::default();
    let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);
    let at = name_line(&output, "B");
    let tokens = Tokens::DARK;

    // The pointer arrives over the row, and a block is carried from elsewhere.
    carry::lift(
        &ctx,
        Carry {
            source: Source::Queue,
            rows: vec![0],
            entries: Vec::new(),
            size: Vec2::new(100.0, ROW_H),
            ghost: Ghost::Queue(queue_row::Lifted::new(
                "A Title".to_owned(),
                String::new(),
                None,
                Cover::Missing,
            )),
            playlist: None,
        },
    );
    let mut washed = false;
    for _ in 0..3 {
        let (output, _) = run_column(&ctx, vec![Event::PointerMoved(at)], &mut library, &mut art);
        washed |= output.shapes.iter().any(|clipped| {
            matches!(&clipped.shape, egui::epaint::Shape::Rect(shape)
                if shape.fill == tokens.palette.row_hover)
        });
    }

    assert!(!washed, "the only fill in the column is the target's");
}
