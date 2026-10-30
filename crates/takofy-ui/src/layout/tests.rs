use egui::{Pos2, vec2};

use super::*;

fn content(width: f32, height: f32) -> Rect {
    Rect::from_min_size(Pos2::ZERO, vec2(width, height))
}

fn solve(width: f32, height: f32) -> (Grid, Regions, Shell) {
    solve_with(width, height, PanePref::Auto)
}

fn solve_with(width: f32, height: f32, pane: PanePref) -> (Grid, Regions, Shell) {
    let content = content(width, height);
    let regions = regions_with(content.width(), pane);
    let shell = shell_for(&regions);
    (grid_for(content, &shell), regions, shell)
}

#[test]
fn each_threshold_is_inclusive_at_its_own_value_and_not_one_point_above() {
    assert_eq!(
        regions_for(NOWPLAYING_NARROW_MAX).now_playing,
        NowPlaying::Narrow
    );
    assert_eq!(
        regions_for(NOWPLAYING_NARROW_MAX + 0.1).now_playing,
        NowPlaying::Wide
    );

    assert_eq!(
        regions_for(NOWPLAYING_HIDDEN_MAX).now_playing,
        NowPlaying::Hidden
    );
    assert_eq!(
        regions_for(NOWPLAYING_HIDDEN_MAX + 0.1).now_playing,
        NowPlaying::Narrow
    );

    assert_eq!(regions_for(PLAYBAR_REDUCED_MAX).playbar, Playbar::Reduced);
    assert_eq!(
        regions_for(PLAYBAR_REDUCED_MAX + 0.1).playbar,
        Playbar::Full
    );

    assert_eq!(regions_for(RAIL_MAX).sidebar, Sidebar::Rail);
    assert_eq!(regions_for(RAIL_MAX + 0.1).sidebar, Sidebar::Full);

    assert_eq!(regions_for(TABLE_NARROW_MAX).table, Table::Narrow);
    assert_eq!(regions_for(TABLE_NARROW_MAX + 0.1).table, Table::Reduced);
    assert_eq!(regions_for(TABLE_REDUCED_MAX).table, Table::Reduced);
    assert_eq!(regions_for(TABLE_REDUCED_MAX + 0.1).table, Table::Full);
}

#[test]
fn a_narrow_table_keeps_the_title_the_artist_its_duration_and_its_controls() {
    let narrow = Table::Narrow.columns();
    assert!(!narrow.album && !narrow.added && !narrow.like);
    // A playlist is the one list whose artist cannot be inferred from the
    // list itself, so it is never dropped. The duration is what a listener
    // scrubs by, and the more button is the only way to reach a track's
    // actions, so neither is ever dropped either.
    assert!(narrow.artist);
    assert!(narrow.duration && narrow.actions);

    let full = Table::Full.columns();
    assert!(full.artist && full.album && full.added && full.like);
    assert!(full.duration && full.actions);
}

#[test]
fn the_album_goes_before_the_artist_below_1000() {
    let reduced = Table::Reduced.columns();
    assert!(!reduced.album);
    assert!(reduced.artist);
    // The date added and the like stay until the window is much narrower.
    assert!(reduced.added && reduced.like);
}

#[test]
fn regions_with_auto_equals_the_width_rule_at_every_width() {
    let edges = [
        NOWPLAYING_NARROW_MAX,
        NOWPLAYING_HIDDEN_MAX,
        PLAYBAR_REDUCED_MAX,
        RAIL_MAX,
        TABLE_REDUCED_MAX,
        TABLE_NARROW_MAX,
    ];
    let sweep = (WINDOW_MIN[0] as u32..=2000).map(|width| width as f32);
    let thresholds = edges
        .iter()
        .flat_map(|edge| [edge - 0.1, *edge, edge + 0.1]);
    for width in sweep.chain(thresholds) {
        assert_eq!(
            regions_with(width, PanePref::Auto),
            regions_for(width),
            "{width}"
        );
    }
}

#[test]
fn a_hidden_choice_hides_only_the_pane() {
    for width in [1120.1, 1200.0, 1240.0, 1360.0, 1360.1, 1440.0, 2000.0] {
        let shown = regions_for(width);
        let hidden = regions_with(width, PanePref::Hidden);
        assert_eq!(hidden.now_playing, NowPlaying::Hidden, "{width}");
        assert_eq!(hidden.sidebar, shown.sidebar, "{width}");
        assert_eq!(hidden.playbar, shown.playbar, "{width}");
        assert_eq!(hidden.table, shown.table, "{width}");
    }
}

#[test]
fn at_and_below_1120_the_choice_is_moot() {
    for width in [WINDOW_MIN[0], 900.0, 1000.0, 1120.0] {
        assert_eq!(
            regions_with(width, PanePref::Hidden),
            regions_with(width, PanePref::Auto),
            "{width}"
        );
    }
}

#[test]
fn the_choice_never_opens_a_pane_the_width_hides() {
    // The only choice a preference can express is "hidden"; there is no
    // variant that would force a pane onto a window with no room for it.
    for width in [WINDOW_MIN[0], 900.0, 1120.0] {
        assert_eq!(
            regions_with(width, PanePref::Hidden).now_playing,
            NowPlaying::Hidden,
            "{width}"
        );
    }
}

#[test]
fn pane_button_is_unavailable_at_and_below_1120_and_follows_the_choice_above() {
    for width in [WINDOW_MIN[0], 900.0, 1000.0, 1120.0] {
        for pane in [PanePref::Auto, PanePref::Hidden] {
            assert_eq!(
                pane_button(width, pane),
                PaneButton::Unavailable,
                "{width} {pane:?}"
            );
        }
    }
    assert_eq!(pane_button(1120.1, PanePref::Auto), PaneButton::Shown);
    assert_eq!(pane_button(1120.1, PanePref::Hidden), PaneButton::Hidden);
    assert_eq!(pane_button(1440.0, PanePref::Auto), PaneButton::Shown);
}

#[test]
fn press_flips_where_the_pane_exists_and_is_inert_where_it_does_not() {
    assert_eq!(PanePref::Auto.press(1240.0), PanePref::Hidden);
    assert_eq!(PanePref::Hidden.press(1240.0), PanePref::Auto);
    assert_eq!(PanePref::Auto.press(1120.1), PanePref::Hidden);

    for width in [WINDOW_MIN[0], 900.0, 1120.0] {
        for pane in [PanePref::Auto, PanePref::Hidden] {
            assert_eq!(pane.press(width), pane, "{width} {pane:?}");
        }
    }
}

#[test]
fn a_hidden_choice_survives_a_trip_below_the_threshold() {
    let hidden = PanePref::Auto.press(1240.0);
    assert_eq!(hidden, PanePref::Hidden);

    let narrow = hidden.press(800.0);
    assert_eq!(narrow, PanePref::Hidden, "a narrow press changed nothing");
    assert_eq!(
        regions_with(1240.0, narrow).now_playing,
        NowPlaying::Hidden,
        "the pane came back on the way up"
    );
}

#[test]
fn the_reduced_playbar_never_has_a_pane_to_toggle() {
    for width in (WINDOW_MIN[0] as u32)..=(PLAYBAR_REDUCED_MAX as u32) {
        let width = width as f32;
        assert_eq!(regions_for(width).playbar, Playbar::Reduced, "{width}");
        for pane in [PanePref::Auto, PanePref::Hidden] {
            assert_eq!(
                pane_button(width, pane),
                PaneButton::Unavailable,
                "{width} {pane:?}"
            );
        }
    }
}

#[test]
fn a_user_hidden_pane_hands_its_width_to_the_view() {
    let (shown, _, _) = solve(1240.0, 900.0);
    let (hidden, regions, shell) = solve_with(1240.0, 900.0, PanePref::Hidden);
    assert_eq!(regions.now_playing, NowPlaying::Hidden);
    assert!(hidden.now_playing.is_none());
    assert_eq!(hidden.view.right(), 1240.0 - shell.padding);
    assert!(hidden.view.width() > shown.view.width());
}

#[test]
fn the_regions_tile_the_inner_area_with_the_pane_hidden_by_choice() {
    let (grid, _, shell) = solve_with(1240.0, 900.0, PanePref::Hidden);
    let inner = content(1240.0, 900.0).shrink(shell.padding);

    let area = grid.sidebar.area() + grid.topbar.area() + grid.view.area() + grid.playbar.area();
    let column_gaps = shell.gap * grid.sidebar.height();
    let row_gap = shell.gap * inner.width();
    let tiled = area + column_gaps + row_gap;

    assert!(
        (tiled - inner.area()).abs() < 0.01,
        "regions cover {tiled}, inner area is {}",
        inner.area()
    );
}

#[test]
fn the_thresholds_are_evaluated_independently() {
    let regions = regions_for(800.0);
    assert_eq!(regions.sidebar, Sidebar::Rail);
    assert_eq!(regions.now_playing, NowPlaying::Hidden);
    assert_eq!(regions.playbar, Playbar::Reduced);
}

#[test]
fn now_playing_is_wide_above_1360_and_narrow_between_1121_and_1360() {
    assert_eq!(regions_for(1360.1).now_playing, NowPlaying::Wide);
    for width in [1120.1, 1300.0, 1360.0] {
        assert_eq!(
            regions_for(width).now_playing,
            NowPlaying::Narrow,
            "{width}"
        );
    }
}

#[test]
fn now_playing_is_hidden_at_and_below_1120() {
    for width in [WINDOW_MIN[0], 900.0, 1120.0] {
        assert_eq!(
            regions_for(width).now_playing,
            NowPlaying::Hidden,
            "{width}"
        );
    }
}

#[test]
fn the_sidebar_is_a_rail_at_and_below_900() {
    for width in [WINDOW_MIN[0], 800.0, 900.0] {
        assert_eq!(regions_for(width).sidebar, Sidebar::Rail, "{width}");
    }
    assert_eq!(regions_for(900.1).sidebar, Sidebar::Full);
}

#[test]
fn a_hidden_now_playing_has_no_width() {
    assert_eq!(shell_for(&regions_for(1120.0)).now_playing, None);
}

#[test]
fn the_visible_widths_are_the_extent_tokens() {
    let wide = shell_for(&regions_for(1440.0));
    assert_eq!(wide.sidebar, extent::SIDEBAR_W);
    assert_eq!(wide.now_playing, Some(extent::NOWPLAYING_W));
    assert_eq!(wide.playbar_h, extent::PLAYBAR_H);
    assert_eq!(wide.topbar_h, extent::TOPBAR_H);

    let narrow = shell_for(&regions_for(1200.0));
    assert_eq!(narrow.sidebar, extent::SIDEBAR_W);
    assert_eq!(narrow.now_playing, Some(extent::NOWPLAYING_NARROW_W));

    let drawer = shell_for(&regions_for(800.0));
    assert_eq!(drawer.sidebar, 0.0, "below 900 the sidebar is the drawer");
}

#[test]
fn the_minimum_window_is_the_narrowest_layout() {
    let regions = regions_for(WINDOW_MIN[0]);
    assert_eq!(regions.sidebar, Sidebar::Rail);
    assert_eq!(regions.now_playing, NowPlaying::Hidden);
    assert_eq!(regions.playbar, Playbar::Reduced);
}

#[test]
fn the_default_window_shows_the_now_playing_column() {
    assert_ne!(
        regions_for(WINDOW_DEFAULT[0]).now_playing,
        NowPlaying::Hidden
    );
}

#[test]
fn the_minimum_window_fits_inside_the_default() {
    for axis in 0..2 {
        assert!(
            WINDOW_MIN[axis] <= WINDOW_DEFAULT[axis],
            "minimum axis {axis} exceeds the default"
        );
    }
}

#[test]
fn the_outer_edge_is_the_padding_on_every_side() {
    let (grid, _, shell) = solve(1440.0, 900.0);
    assert_eq!(grid.sidebar.left(), shell.padding);
    assert_eq!(grid.sidebar.top(), shell.padding);
    assert_eq!(grid.playbar.right(), 1440.0 - shell.padding);
    assert_eq!(grid.playbar.bottom(), 900.0 - shell.padding);
    assert_eq!(grid.now_playing.unwrap().right(), 1440.0 - shell.padding);
}

#[test]
fn adjacent_columns_are_exactly_one_gap_apart() {
    let (grid, _, shell) = solve(1440.0, 900.0);
    assert_eq!(grid.sidebar.right() + shell.gap, grid.topbar.left());
    assert_eq!(
        grid.topbar.right() + shell.gap,
        grid.now_playing.unwrap().left()
    );

    // Below 900 there is no sidebar column, so the main pane starts at the
    // inner edge and no gap is left for a column that is not there.
    let (narrow, _, _) = solve(800.0, 720.0);
    assert_eq!(narrow.sidebar.width(), 0.0);
    assert_eq!(narrow.topbar.left(), narrow.sidebar.left());
    assert!(narrow.now_playing.is_none());
}

#[test]
fn the_playbar_spans_the_full_inner_width() {
    let (grid, _, shell) = solve(1440.0, 900.0);
    assert_eq!(grid.playbar.left(), shell.padding);
    assert_eq!(grid.playbar.right(), 1440.0 - shell.padding);
}

#[test]
fn the_panel_row_clears_the_playbar_by_one_gap() {
    let (grid, _, shell) = solve(1440.0, 900.0);
    assert_eq!(grid.sidebar.bottom() + shell.gap, grid.playbar.top());
    assert_eq!(
        grid.now_playing.unwrap().bottom() + shell.gap,
        grid.playbar.top()
    );
    assert_eq!(grid.view.bottom() + shell.gap, grid.playbar.top());
}

#[test]
fn the_view_is_what_the_other_columns_leave() {
    let (grid, _, shell) = solve(1440.0, 900.0);
    assert_eq!(grid.view.left(), grid.topbar.left());
    assert_eq!(
        grid.view.right(),
        grid.now_playing.unwrap().left() - shell.gap
    );
    assert_eq!(grid.view.top(), grid.topbar.bottom());
}

#[test]
fn a_hidden_now_playing_hands_its_width_to_the_view() {
    let (hidden, _, shell) = solve(1120.0, 900.0);
    assert!(hidden.now_playing.is_none());
    assert_eq!(hidden.view.right(), 1120.0 - shell.padding);

    let (narrow, _, _) = solve(1121.0, 900.0);
    assert!(narrow.now_playing.is_some());
    assert!(narrow.view.width() < hidden.view.width());
}

#[test]
fn the_narrow_shell_gives_the_sidebar_to_the_drawer() {
    let (full, _, _) = solve(1440.0, 900.0);
    let (narrow, regions, shell) = solve(800.0, 900.0);
    assert_eq!(narrow.sidebar.width(), 0.0);
    assert!(narrow.sidebar.width() < full.sidebar.width());
    assert_eq!(regions.sidebar, Sidebar::Rail);
    assert_eq!(narrow.sidebar.width(), shell.sidebar);
    // The main pane starts where the sidebar would have, because there is
    // nothing there to make room for.
    assert_eq!(narrow.topbar.left(), narrow.sidebar.left());
}

#[test]
fn the_topbar_sits_above_the_view_with_no_overlap() {
    let (grid, _, shell) = solve(1440.0, 900.0);
    assert_eq!(grid.topbar.bottom(), grid.view.top());
    assert_eq!(grid.topbar.top(), grid.view.top() - shell.topbar_h);
    assert_eq!(grid.topbar.height(), shell.topbar_h);
}

#[test]
fn no_rect_is_negative_or_degenerate_at_the_minimum_window() {
    let (grid, _, _) = solve(WINDOW_MIN[0], WINDOW_MIN[1]);
    // The sidebar is the drawer at this width, so it has no width of its
    // own to check: the drawer is where it is drawn.
    let mut rects = vec![grid.topbar, grid.view, grid.playbar, grid.drawer];
    if let Some(now_playing) = grid.now_playing {
        rects.push(now_playing);
    }
    for rect in rects {
        assert!(rect.width() > 0.0, "{rect:?} has no width");
        assert!(rect.height() > 0.0, "{rect:?} has no height");
    }
}

#[test]
fn the_drawer_starts_on_the_padding_corner_and_clears_the_playbar() {
    let (grid, _, shell) = solve(1440.0, 900.0);
    assert_eq!(grid.drawer.left(), shell.padding);
    assert_eq!(grid.drawer.top(), shell.padding);
    assert_eq!(grid.drawer.width(), extent::SIDEBAR_W);
    assert_eq!(grid.drawer.bottom(), grid.playbar.top() - space::S3);
}

#[test]
fn the_regions_tile_the_inner_area_without_gaps_or_overlaps() {
    let (grid, _, shell) = solve(1440.0, 900.0);
    let inner = content(1440.0, 900.0).shrink(shell.padding);

    let mut area = grid.sidebar.area() + grid.topbar.area() + grid.view.area();
    area += grid.playbar.area();
    if let Some(now_playing) = grid.now_playing {
        area += now_playing.area();
    }

    let columns = if grid.now_playing.is_some() { 3 } else { 2 };
    let column_gaps = shell.gap * grid.sidebar.height() * (columns - 1) as f32;
    let row_gap = shell.gap * inner.width();
    let tiled = area + column_gaps + row_gap;

    assert!(
        (tiled - inner.area()).abs() < 0.01,
        "regions cover {tiled}, inner area is {}",
        inner.area()
    );
}

fn assert_close(left: f32, right: f32) {
    assert!((left - right).abs() < 0.01, "{left} is not {right}");
}

fn playbar_at(width: f32) -> (Rect, Playbar) {
    let regions = regions_for(width);
    let shell = shell_for(&regions);
    let grid = grid_for(content(width, 900.0), &shell);
    (grid.playbar, regions.playbar)
}

fn playbar_tracks_width(rect: Rect) -> f32 {
    rect.width() - 2.0 * PlaybarCols::PADDING - 2.0 * PlaybarCols::GAP
}

fn topbar_rect(width: f32) -> Rect {
    Rect::from_min_size(Pos2::ZERO, vec2(width, extent::TOPBAR_H))
}

#[test]
fn the_reduced_extras_are_152_from_the_tokens() {
    assert_eq!(playbar_extras(Playbar::Reduced), 152.0);
}

#[test]
fn the_full_extras_are_216_from_the_tokens() {
    assert_eq!(playbar_extras(Playbar::Full), 216.0);
}

#[test]
fn the_reduced_playbar_fits_the_minimum_window_with_56_to_spare() {
    let (rect, form) = playbar_at(WINDOW_MIN[0]);
    assert_eq!(form, Playbar::Reduced);
    let columns = playbar_for(rect, form);
    let minimums = extent::PLAYBAR_NOW_MIN_REDUCED
        + extent::PLAYBAR_CENTRE_MIN_REDUCED
        + playbar_extras(Playbar::Reduced);
    let slack = playbar_tracks_width(rect) - minimums;
    assert_close(slack, 56.0);
    assert_close(
        columns.now + columns.centre + columns.extras,
        playbar_tracks_width(rect),
    );
}

#[test]
fn the_full_extras_fit_their_column_at_the_narrowest_full_width() {
    let (rect, form) = playbar_at(PLAYBAR_REDUCED_MAX + 0.1);
    assert_eq!(form, Playbar::Full);
    let columns = playbar_for(rect, form);
    assert!(columns.extras >= playbar_extras(Playbar::Full));
}

#[test]
fn every_column_meets_its_minimum_at_every_width_from_the_minimum_window_up() {
    for width in (WINDOW_MIN[0] as u32)..=2000 {
        let (rect, form) = playbar_at(width as f32);
        let columns = playbar_for(rect, form);
        let (now_min, centre_min, extras_min) = match form {
            Playbar::Full => (
                extent::PLAYBAR_NOW_MIN,
                extent::PLAYBAR_CENTRE_MIN,
                extent::PLAYBAR_EXTRAS_MIN,
            ),
            Playbar::Reduced => (
                extent::PLAYBAR_NOW_MIN_REDUCED,
                extent::PLAYBAR_CENTRE_MIN_REDUCED,
                playbar_extras(Playbar::Reduced),
            ),
        };
        assert!(columns.now >= now_min, "{width}: now {}", columns.now);
        assert!(
            columns.centre >= centre_min,
            "{width}: centre {}",
            columns.centre
        );
        assert!(
            columns.extras >= extras_min,
            "{width}: extras {}",
            columns.extras
        );
    }
}

#[test]
fn below_the_supported_width_columns_hold_their_minimums_and_overflow_right() {
    let width = WINDOW_MIN[0] - 100.0;
    let (rect, form) = playbar_at(width);
    assert_eq!(form, Playbar::Reduced);
    let columns = playbar_for(rect, form);
    assert_eq!(columns.now, extent::PLAYBAR_NOW_MIN_REDUCED);
    assert_eq!(columns.centre, extent::PLAYBAR_CENTRE_MIN_REDUCED);
    assert_eq!(columns.extras, playbar_extras(Playbar::Reduced));
    let used = columns.now
        + columns.centre
        + columns.extras
        + 2.0 * PlaybarCols::PADDING
        + 2.0 * PlaybarCols::GAP;
    assert!(used > rect.width());
}

#[test]
fn playbar_columns_are_one_gap_apart_and_inside_the_rect() {
    let (rect, form) = playbar_at(1440.0);
    let [now, centre, extras] = playbar_for(rect, form).columns(rect);
    assert_eq!(now.left(), rect.left() + PlaybarCols::PADDING);
    assert_close(now.right() + PlaybarCols::GAP, centre.left());
    assert_close(centre.right() + PlaybarCols::GAP, extras.left());
    assert!(extras.right() <= rect.right() - PlaybarCols::PADDING + 0.01);
    for column in [now, centre, extras] {
        assert_eq!(column.top(), rect.top());
        assert_eq!(column.bottom(), rect.bottom());
    }
}

#[test]
fn the_playbar_weights_follow_the_reference_at_1440_and_at_800() {
    let (full, form) = playbar_at(1440.0);
    let columns = playbar_for(full, form);
    let tracks = playbar_tracks_width(full);
    assert_close(columns.now, tracks / 4.0);
    assert_close(columns.centre, tracks / 2.0);
    assert_close(columns.extras, tracks / 4.0);

    let (reduced, form) = playbar_at(800.0);
    let columns = playbar_for(reduced, form);
    let flex = playbar_tracks_width(reduced) - playbar_extras(Playbar::Reduced);
    let unit = flex / 2.5;
    assert_close(columns.now, unit);
    assert_close(columns.centre, 1.5 * unit);
}

#[test]
fn the_separator_slot_is_the_mark_and_a_space_1_margin_each_side() {
    assert_eq!(HISTORY_RULE, stroke::HAIRLINE + 2.0 * space::S1);
}

#[test]
fn the_history_width_is_the_sum_of_its_parts() {
    // Four squares, the four gaps between them, and the rule's slot: the
    // hairline mark with the two `space-1` margins that stand in for its item.
    assert_eq!(
        history_width(),
        4.0 * extent::BUTTON + 4.0 * space::S1 + stroke::HAIRLINE + 2.0 * space::S1
    );
}

#[test]
fn the_placed_history_column_is_the_width_the_solver_reserved() {
    for width in [700.0, 1000.0, 1440.0] {
        let rect = topbar_rect(width);
        let sidebar = regions_for(width).sidebar;
        let places = topbar_for(rect, sidebar, 200.0).columns(rect);

        assert_eq!(places.history.width(), history_width(), "{width}");
        assert_close(
            places.search.left() - places.history.right(),
            TopbarCols::GAP,
        );
    }
}

#[test]
fn the_free_space_equals_the_field_the_trail_and_the_room_left_over() {
    // The solver's `free` has to be what the row actually has to spend: the
    // field, the trail, and the room the row leaves past its own gap. A solver
    // that subtracted a history width the row does not place would be off by
    // exactly that drift here, and by nothing else.
    for width in [700.0, 1000.0, 1440.0, 1800.0] {
        let rect = topbar_rect(width);
        let sidebar = regions_for(width).sidebar;
        let free = topbar_free(rect, sidebar);
        let columns = topbar_for(rect, sidebar, 200.0);
        let places = columns.columns(rect);

        let slack = places.trail.left() - places.search.right() - TopbarCols::GAP;
        assert_close(columns.search + columns.trail + slack, free);
    }
}

#[test]
fn the_search_field_is_capped_at_its_maximum() {
    let columns = topbar_for(topbar_rect(1440.0), Sidebar::Full, 100.0);
    assert_eq!(columns.search, extent::SEARCH_MAX_W);
}

#[test]
fn the_search_field_yields_to_the_trail_before_the_trail_truncates() {
    let rect = topbar_rect(1100.0);
    let free = topbar_free(rect, Sidebar::Full);

    let natural = free - 200.0;
    let columns = topbar_for(rect, Sidebar::Full, natural);
    assert_close(columns.search, 200.0);
    assert_close(columns.trail, natural);

    let natural = free - 50.0;
    let columns = topbar_for(rect, Sidebar::Full, natural);
    assert_eq!(columns.search, extent::SEARCH_MIN_W);
    assert!(columns.trail < natural);
    assert_close(columns.search + columns.trail, free);
}

#[test]
fn the_trail_never_overlaps_the_search_field() {
    for width in 640..=2000 {
        let rect = topbar_rect(width as f32);
        let sidebar = regions_for(width as f32).sidebar;
        let free = topbar_free(rect, sidebar);
        let columns = topbar_for(rect, sidebar, 300.0);
        assert!(
            columns.search + columns.trail <= free + 0.01,
            "{width}: search {} + trail {} beats {free}",
            columns.search,
            columns.trail
        );
    }
}

#[test]
fn the_toggle_exists_only_on_the_rail() {
    let full = topbar_for(topbar_rect(1440.0), Sidebar::Full, 200.0);
    let rail = topbar_for(topbar_rect(800.0), Sidebar::Rail, 200.0);
    assert_eq!(full.toggle, None);
    assert_eq!(rail.toggle, Some(extent::BUTTON));
    assert!(full.columns(topbar_rect(1440.0)).toggle.is_none());
    assert!(rail.columns(topbar_rect(800.0)).toggle.is_some());
}

#[test]
fn the_trail_is_flush_right_against_the_padding() {
    let rect = topbar_rect(1440.0);
    let columns = topbar_for(rect, Sidebar::Full, 200.0).columns(rect);
    assert_close(columns.trail.right(), rect.right() - TopbarCols::PADDING);
}
