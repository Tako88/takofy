use super::super::*;
use super::test_support::*;

use egui::Event;

use crate::message::Update;
use crate::view::test_support::click_events;

// ---- the rail's filter field ----

/// The tiles a frame painted stepped back because their row is carried.
fn lifted_tiles(output: &egui::FullOutput) -> Vec<Rect> {
    let tokens = Tokens::DARK;
    let dim = tokens
        .palette
        .surface_sunken
        .gamma_multiply(crate::theme::opacity::LIFTED);
    output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::epaint::Shape::Rect(shape) if shape.fill == dim => Some(shape.rect),
            _ => None,
        })
        .collect()
}

/// Clicks the rail's field and types `text` into it, answering the frame the
/// keystroke landed on.
fn type_in_rail(
    ctx: &egui::Context,
    text: &str,
    library: &mut Library,
    art: &mut Art,
) -> egui::FullOutput {
    let (output, _) = run_column(ctx, Vec::new(), library, art);
    let field = filter_field_of(&output);
    for event in click_events(field.center()) {
        let _ = run_column(ctx, vec![event], library, art);
    }
    assert!(
        ctx.text_edit_focused(),
        "the click focused the rail's field"
    );
    let (output, _) = run_column(ctx, vec![Event::Text(text.to_owned())], library, art);
    output
}

#[test]
fn typing_narrows_the_rows_in_the_same_frame() {
    let ctx = context();
    let mut library = named_playlists(&["Midnight City", "Outro", "Midnight Sun"]);
    let mut art = Art::default();

    let output = type_in_rail(&ctx, "midnight", &mut library, &mut art);

    assert_eq!(
        painted_rows(&output, &["Midnight City", "Outro", "Midnight Sun"]),
        ["Midnight City", "Midnight Sun"],
        "the keystroke frame already draws only the matches"
    );
    let pitch = ROW_H + space::PX;
    let first = row_top(&output, "Midnight City", "Playlist");
    let second = row_top(&output, "Midnight Sun", "Playlist");
    assert!(
        (second - first - pitch).abs() < 0.01,
        "the survivors close up into consecutive slots: {first} then {second}"
    );
}

#[test]
fn the_two_axes_compose() {
    let ctx = context();
    // The matching playlist and the matching artist sit at different true
    // indices, so reading the wrong kind's list cannot pass.
    let mut library = named_playlists(&["Vela", "Artemis"]);
    library.apply(&Update::FollowedArtists(vec![
        followed_artist("spotify:artist:1", "Artemis"),
        followed_artist("spotify:artist:2", "Vela Moreno"),
    ]));
    assert!(library.choose_kind(SidebarKind::Artists));
    let mut art = Art::default();

    let output = type_in_rail(&ctx, "art", &mut library, &mut art);

    assert!(
        painted(&output, "Artemis"),
        "the chosen kind's matching row is drawn"
    );
    assert!(
        !painted(&output, "Vela Moreno"),
        "a row of the chosen kind that does not match is hidden"
    );
}

#[test]
fn matching_is_case_insensitive_over_the_name_only() {
    let ctx = context();
    // The artist's kind line is "Artist", which a name-only match ignores.
    let mut library = named_playlists(&[]);
    library.apply(&Update::FollowedArtists(vec![followed_artist(
        "spotify:artist:a",
        "Vela",
    )]));
    assert!(library.choose_kind(SidebarKind::Artists));
    let mut art = Art::default();

    let output = type_in_rail(&ctx, "art", &mut library, &mut art);

    assert!(
        !painted(&output, "Vela"),
        "the kind line is not what the query matches"
    );

    // And a name matches whatever its case.
    let ctx = context();
    let mut library = named_playlists(&[]);
    library.apply(&Update::FollowedArtists(vec![followed_artist(
        "spotify:artist:a",
        "Artemis",
    )]));
    assert!(library.choose_kind(SidebarKind::Artists));
    library.rail_filter.edit().push_str("ARTE");
    let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);
    assert!(painted(&output, "Artemis"), "the query folds case");
}

#[test]
fn a_blank_query_narrows_nothing() {
    let ctx = context();
    let mut library = named_playlists(&["Alpha", "Bravo"]);
    library.rail_filter.edit().push_str("   ");
    let mut art = Art::default();

    let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);

    assert_eq!(
        painted_rows(&output, &["Alpha", "Bravo"]),
        ["Alpha", "Bravo"],
        "whitespace is no filter"
    );
    assert_eq!(
        carry::take_column_offer(&ctx).and_then(|offer| offer.rows),
        None,
        "a blank query leaves the column whole"
    );
}

#[test]
fn clearing_restores_every_row() {
    let ctx = context();
    let mut library = named_playlists(&["Midnight City", "Outro"]);
    let mut art = Art::default();
    let _ = type_in_rail(&ctx, "mid", &mut library, &mut art);

    library.rail_filter.clear();
    let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);

    assert_eq!(
        painted_rows(&output, &["Midnight City", "Outro"]),
        ["Midnight City", "Outro"],
        "the whole list is back"
    );
}

#[test]
fn a_narrowed_list_offers_only_what_it_draws() {
    let ctx = context();
    let mut library = movable_playlists(&["Midnight City", "Outro", "Midnight Sun"], 7, true, None);
    library.rail_filter.edit().push_str("midnight");
    let mut art = Art::default();

    let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);

    let offer = carry::take_column_offer(&ctx).expect("the column offers itself");
    assert_eq!(offer.zone.count, 2, "only the drawn rows are offered");
    let first = row_top(&output, "Midnight City", "Playlist");
    assert!(
        (offer.zone.list.top() - first).abs() < 0.01,
        "the rows box starts at the first drawn row: {} vs {first}",
        offer.zone.list.top()
    );
}

#[test]
fn a_narrowed_offer_carries_the_true_rows() {
    let ctx = context();
    let mut library = movable_playlists(&["Midnight City", "Outro", "Midnight Sun"], 7, true, None);
    library.rail_filter.edit().push_str("midnight");
    let mut art = Art::default();

    let _ = run_column(&ctx, Vec::new(), &mut library, &mut art);

    let offer = carry::take_column_offer(&ctx).expect("the column offers itself");
    assert_eq!(
        offer.rows,
        Some(vec![0, 2]),
        "the offer names the playlists the slots stand for"
    );
    assert_eq!(offer.source_row(1), Some(2));
}

#[test]
fn an_unnarrowed_offer_carries_no_rows() {
    let ctx = context();
    let mut library = movable_playlists(&["Alpha", "Bravo"], 7, true, None);
    let mut art = Art::default();

    let _ = run_column(&ctx, Vec::new(), &mut library, &mut art);

    let offer = carry::take_column_offer(&ctx).expect("the column offers itself");
    assert_eq!(offer.rows, None, "a whole list needs no map");
}

#[test]
fn a_narrowed_playlist_row_does_not_lift() {
    let ctx = context();
    let mut library = movable_playlists(&["Midnight City", "Outro"], 7, true, None);
    library.rail_filter.edit().push_str("midnight");
    let mut art = Art::default();
    let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);
    let from = name_line(&output, "Midnight City");

    let _ = hold_from(&ctx, from, &mut library, &mut art, true);

    assert!(
        carry::carried(&ctx).is_none(),
        "a narrowed rail row has no reorder it could land"
    );
}

#[test]
fn an_unnarrowed_row_still_lifts_with_its_true_index() {
    let ctx = context();
    let mut library = movable_playlists(&["Alpha", "Bravo"], 7, true, None);
    let mut art = Art::default();
    let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);
    let from = name_line(&output, "Bravo");

    let _ = hold_from(&ctx, from, &mut library, &mut art, true);

    let carry = carry::carried(&ctx).expect("the unnarrowed row lifts");
    assert_eq!(carry.rows, vec![1], "the carry names the row's true index");
}

#[test]
fn a_row_lifted_before_narrowing_carries_its_true_index_and_dims_the_right_row() {
    let ctx = context();
    let mut library = movable_playlists(&["Alpha", "Bravo", "Midnight Sun"], 7, true, None);
    let mut art = Art::default();
    let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);
    let from = name_line(&output, "Midnight Sun");

    let _ = hold_from(&ctx, from, &mut library, &mut art, true);
    let carry = carry::carried(&ctx).expect("the row lifts");
    assert_eq!(carry.rows, vec![2]);

    // The query leaves only the lifted row, which now draws in the first
    // slot: the dim follows the playlist, not the slot it moved to.
    library.rail_filter.edit().push_str("midnight");
    let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);

    assert_eq!(
        painted_rows(&output, &["Alpha", "Bravo", "Midnight Sun"]),
        ["Midnight Sun"],
        "only the lifted row survives the query"
    );
    assert_eq!(
        lifted_tiles(&output).len(),
        1,
        "the surviving row is the one stepped back: {:?}",
        lifted_tiles(&output)
    );
}

#[test]
fn a_query_matching_nothing_draws_no_rows_and_offers_nothing() {
    let ctx = context();
    let mut library = movable_playlists(&["Alpha", "Bravo"], 7, true, None);
    library.rail_filter.edit().push_str("zzz");
    let mut art = Art::default();

    let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);

    assert_eq!(
        painted_rows(&output, &["Alpha", "Bravo"]),
        Vec::<String>::new(),
        "no row matches"
    );
    assert_eq!(
        carry::take_column_offer(&ctx),
        None,
        "an empty list offers no rows"
    );
}

#[test]
fn the_query_survives_a_reload_and_a_loading_frame() {
    let ctx = context();
    let mut library = movable_playlists(&["Midnight City", "Outro"], 7, true, None);
    let mut art = Art::default();
    let _ = type_in_rail(&ctx, "mid", &mut library, &mut art);

    library.apply(&Update::Playlists(vec![
        playlist("spotify:playlist:Midnight City", "Midnight City"),
        playlist("spotify:playlist:Outro", "Outro"),
    ]));
    let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);
    assert_eq!(library.rail_filter.query(), "mid", "a reload keeps it");
    assert_eq!(
        painted_rows(&output, &["Midnight City", "Outro"]),
        ["Midnight City"],
        "and the narrowed rows come back with the reply"
    );

    library.playlists = Load::Loading;
    let _ = run_column(&ctx, Vec::new(), &mut library, &mut art);
    assert_eq!(
        library.rail_filter.query(),
        "mid",
        "a loading frame keeps the query too"
    );
    library.playlists = Load::Ready(vec![
        playlist("spotify:playlist:Midnight City", "Midnight City"),
        playlist("spotify:playlist:Outro", "Outro"),
    ]);
    let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);
    assert_eq!(
        painted_rows(&output, &["Midnight City", "Outro"]),
        ["Midnight City"],
        "the narrowed list is still narrowed after the load"
    );
}

#[test]
fn changing_the_query_dismisses_a_row_rename() {
    let ctx = context();
    let mut library = movable_playlists(&["Alpha", "Bravo"], 7, true, None);
    library.renaming.open(
        "spotify:playlist:Bravo",
        "Bravo",
        crate::naming::Surface::Row,
    );
    let mut art = Art::default();
    let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);
    assert!(filter_hairlines(&output).len() == 1, "the field is drawn");

    let _ = type_in_rail(&ctx, "a", &mut library, &mut art);

    assert!(
        !library.renaming.is_open(),
        "a keystroke in the rail closed the rename it would hide"
    );
}

#[test]
fn a_rename_on_a_row_the_query_hides_is_dismissed() {
    let ctx = context();
    let mut library = movable_playlists(&["Alpha", "Bravo"], 7, true, None);
    library.renaming.open(
        "spotify:playlist:Bravo",
        "Bravo",
        crate::naming::Surface::Row,
    );
    // The query is already there when the frame begins, so this is not a
    // keystroke: the hidden row is enough to close the field.
    library.rail_filter.edit().push_str("alpha");
    let mut art = Art::default();

    let _ = run_column(&ctx, Vec::new(), &mut library, &mut art);

    assert!(
        !library.renaming.is_open(),
        "the rename's row is not among the survivors, so the field goes"
    );
    assert_eq!(library.rail_filter.query(), "alpha");
}
