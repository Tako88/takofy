use super::super::*;
use super::test_support::*;

#[test]
fn the_failure_notice_draws_on_every_tab() {
    let line = "Could not remove that from your library. Try again.";
    for kind in sidebar_kind::ROW {
        let ctx = context();
        let mut library = four_kinds();
        assert!(library.choose_kind(kind));
        library.sidebar_notice = Some(crate::library::SidebarWrite::Remove);
        let mut art = Art::default();

        let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);

        assert!(painted(&output, line), "{kind:?} drew no notice");
    }
}

#[test]
fn a_query_with_no_survivor_says_nothing_matches_the_query() {
    let ctx = context();
    let mut library = named_playlists(&["Alpha", "Bravo"]);
    library.rail_filter.edit().push_str("  zzz  ");
    let mut art = Art::default();

    let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);

    assert_eq!(
        painted_rows(&output, &["Alpha", "Bravo"]),
        Vec::<String>::new(),
        "no row matches"
    );
    assert!(
        painted(&output, "Nothing matches “zzz”"),
        "the empty line names the trimmed query: {:?}",
        drawn_text(&output)
    );
}

#[test]
fn the_per_kind_sentence_stays_when_the_kind_alone_is_empty() {
    for (kind, words) in [
        (SidebarKind::Playlists, "No playlists yet."),
        (SidebarKind::Podcasts, "You don't follow any shows yet."),
        (SidebarKind::Artists, "You don't follow any artists yet."),
        (SidebarKind::Albums, "You haven't saved any albums yet."),
    ] {
        let ctx = context();
        let mut library = Library::default();
        library.playlists = Load::Ready(Vec::new());
        library.followed = Load::Ready(Vec::new());
        library.followed_artists = Load::Ready(Vec::new());
        library.saved_albums = Load::Ready(Vec::new());
        assert!(library.choose_kind(kind));
        library.rail_filter.edit().push_str("zzz");
        let mut art = Art::default();

        let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);

        assert!(
            painted(&output, words),
            "{kind:?} drew no per-kind sentence: {:?}",
            drawn_text(&output)
        );
        assert!(
            !painted(&output, "Nothing matches “zzz”"),
            "{kind:?} claimed a query emptied a kind that holds nothing"
        );
    }
}

#[test]
fn no_query_keeps_todays_sentence() {
    let ctx = context();
    let mut library = Library::default();
    library.playlists = Load::Ready(Vec::new());
    let mut art = Art::default();

    let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);

    assert!(painted(&output, "No playlists yet."));
}

#[test]
fn loading_and_failed_keep_their_words_with_a_query_present() {
    let ctx = context();
    let mut library = named_playlists(&["Alpha"]);
    library.rail_filter.edit().push_str("zzz");
    let mut art = Art::default();

    library.playlists = Load::Loading;
    let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);
    assert!(painted(&output, "Loading your playlists…"));
    assert!(!painted(&output, "Nothing matches “zzz”"));

    library.playlists = Load::Failed(Failure::Load);
    let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);
    assert!(painted(
        &output,
        "Could not load the library. Check your connection and try again."
    ));
    assert!(!painted(&output, "Nothing matches “zzz”"));
}

#[test]
fn a_long_query_wraps_inside_the_list() {
    let ctx = context();
    let mut library = named_playlists(&["Alpha"]);
    let long = "z".repeat(200);
    library.rail_filter.edit().push_str(&long);
    let mut art = Art::default();

    let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);

    let line = format!("Nothing matches “{long}”");
    // The painted rect is the union of the galley's rows, because the line is
    // centred: the shape's own `pos` is the centre, not the left edge.
    let painted = output.shapes.iter().fold(None, |union, clipped| {
        let egui::epaint::Shape::Text(text) = &clipped.shape else {
            return union;
        };
        if text.galley.text() != line {
            return union;
        }
        let block = text.galley.rows.iter().fold(Rect::NOTHING, |block, row| {
            block.union(row.rect().translate(text.pos.to_vec2()))
        });
        Some(union.map_or(block, |union: Rect| union.union(block)))
    });
    let painted = painted.expect("the empty line is drawn");
    let list = Rect::from_min_max(
        Pos2::new(space::S2, 0.0),
        Pos2::new(rail().width() - space::S2, rail().height()),
    );
    assert!(
        painted.left() >= list.left() - 0.01 && painted.right() <= list.right() + 0.01,
        "the line stays inside the list horizontally: {}..{} in {}..{}",
        painted.left(),
        painted.right(),
        list.left(),
        list.right()
    );
}
