use super::*;
use crate::layout::{Table, WINDOW_MIN};

fn all() -> Present {
    Present {
        shape: Shape::Playlist,
        artist: true,
        album: true,
        added: true,
        like: true,
        duration: true,
        actions: true,
    }
}

/// A plays row: the track's own artist in a tight column, no album, and no
/// membership date.
fn plays() -> Present {
    all().for_plays()
}

/// A slim row: no credit column at all, as a release block draws it.
fn slim() -> Present {
    all().for_slim()
}

/// Every shape, at the full width table.
fn shapes() -> [Present; 4] {
    [all(), plays(), all().for_plays_album(), slim()]
}

/// The gutter a shape pays between its columns.
fn gutter(present: Present) -> f32 {
    if present.shape == Shape::Playlist {
        space::S3
    } else {
        space::S4
    }
}

fn rects(columns: Columns) -> [(&'static str, Rect); 8] {
    [
        ("index", columns.index),
        ("title", columns.title),
        ("artist", columns.artist),
        ("album", columns.album),
        ("added", columns.added),
        ("like", columns.like),
        ("duration", columns.duration),
        ("actions", columns.actions),
    ]
}

fn assert_close(left: f32, right: f32) {
    assert!((left - right).abs() < 0.01, "{left} is not {right}");
}

#[test]
fn only_a_playlist_uri_is_a_playlist() {
    for uri in [
        "spotify:playlist:abc",
        "spotify:user:u:playlist:abc",
        "spotify:playlist:abc:extra",
    ] {
        assert!(is_playlist(uri), "must accept {uri}");
    }
    for uri in [
        "spotify:album:abc",
        "spotify:artist:playlist-fan",
        "spotify:track:abc",
        "spotify:local:Artist:Album:Title:222",
        "spotify:user:u:collection",
        "spotify:user:someone:folder:abc",
        "spotify:playlist",
        "spotify:playlist:",
        "spotify:user:u:playlist",
        "spotify:user:u:playlist:",
        "",
        "https://i.scdn.co/image/abc",
    ] {
        assert!(!is_playlist(uri), "must reject {uri}");
    }
}

#[test]
fn only_a_playlist_presents_the_date_added() {
    for table in [Table::Full, Table::Reduced, Table::Narrow] {
        let present = table.columns();
        assert_eq!(
            present.for_playlist(true),
            present,
            "{table:?} must keep its own columns for a playlist"
        );
        assert_eq!(
            present.for_playlist(false),
            Present {
                added: false,
                ..present
            },
            "{table:?} must drop only the date for a non-playlist"
        );
    }
}

#[test]
fn the_columns_sum_to_the_available_width_when_it_is_large_enough() {
    let width = 1000.0;
    for present in shapes() {
        let columns = columns(width, present);
        let count = rects(columns)
            .iter()
            .filter(|(_, rect)| rect.width() > 0.0)
            .count();
        let sum = columns.index.width()
            + columns.title.width()
            + columns.artist.width()
            + columns.album.width()
            + columns.added.width()
            + columns.like.width()
            + columns.duration.width()
            + columns.actions.width()
            + (count - 1) as f32 * gutter(present);
        assert_close(sum, width - 2.0 * space::S3);
    }
}

#[test]
fn an_absent_column_takes_no_width_and_no_gap() {
    let width = 1000.0;
    let hidden = columns(width, slim());
    assert_eq!(hidden.artist.width(), 0.0);
    assert_eq!(hidden.album.width(), 0.0);
    assert_eq!(hidden.added.width(), 0.0);

    // A slim row keeps the title, the like, the duration and the actions,
    // with `space-4` between them.
    let sum = hidden.index.width()
        + hidden.title.width()
        + hidden.like.width()
        + hidden.duration.width()
        + hidden.actions.width()
        + 4.0 * space::S4;
    assert_close(sum, width - 2.0 * space::S3);

    // The title takes the dropped columns' share of the growth.
    assert!(hidden.title.width() > columns(width, all()).title.width());
}

#[test]
fn a_plays_table_keeps_the_artist_and_clears_album_and_added() {
    let width = 1000.0;
    let present = all().for_plays();
    assert_eq!(present.shape, Shape::Plays);
    assert!(present.artist, "a plays row names the track's artist");
    assert!(!present.album, "a plays row names no album");
    assert!(!present.added, "a plays row has no membership date");

    let columns = columns(width, present);
    assert!(columns.artist.width() > 0.0);
    assert_eq!(columns.album.width(), 0.0);
    assert_eq!(columns.added.width(), 0.0);

    // The controls the plays row keeps: the like, the length, the more.
    assert!(columns.like.width() > 0.0, "the plays row keeps the like");
    assert!(columns.duration.width() > 0.0);
    assert!(columns.actions.width() > 0.0);

    // The title starts one space-4 after the index, as a slim row's does.
    assert_close(columns.title.left(), columns.index.right() + space::S4);

    // Adjacent present columns are space-4 apart, not the playlist's tighter
    // space-3: with the album dropped, the artist and the like are neighbours.
    assert_close(columns.like.left(), columns.artist.right() + space::S4);

    // Six present columns tile the inner width with five space-4 gutters.
    let sum = columns.index.width()
        + columns.title.width()
        + columns.artist.width()
        + columns.like.width()
        + columns.duration.width()
        + columns.actions.width()
        + 5.0 * space::S4;
    assert_close(sum, width - 2.0 * space::S3);
}

#[test]
fn the_plays_artist_takes_055_of_the_titles_share() {
    for width in [700.0, 900.0, 1200.0] {
        let columns = columns(width, plays());
        assert_close(
            columns.artist.width() / columns.title.width(),
            grow::CREDIT_PLAYS,
        );
    }
}

#[test]
fn the_plays_album_takes_the_same_share_in_the_other_cell() {
    for width in [700.0, 900.0, 1200.0] {
        let columns = columns(width, all().for_plays_album());
        assert!(
            columns.artist.width() == 0.0,
            "the artist column is dropped, not merely empty: {columns:?}"
        );
        assert_close(
            columns.album.width() / columns.title.width(),
            grow::CREDIT_PLAYS,
        );
    }
}

#[test]
fn a_slim_table_has_no_artist_album_or_added() {
    let width = 1000.0;
    let present = all().for_slim();
    assert_eq!(present.shape, Shape::Slim);
    assert!(!present.artist, "a slim row names no artist column");
    assert!(!present.album, "a slim row names no album column");
    assert!(!present.added, "a slim row has no membership date");

    let columns = columns(width, present);
    assert_eq!(columns.artist.width(), 0.0);
    assert_eq!(columns.album.width(), 0.0);
    assert_eq!(columns.added.width(), 0.0);

    // The title is the only grower left, so its width is exactly what the
    // fixed columns and the four gutters leave: this pins `grow::TITLE` as
    // the slim row's weight rather than a second flexible column.
    let fixed = extent::TRACK_INDEX_W
        + extent::TRACK_LIKE_W
        + extent::TRACK_DURATION_W
        + extent::TRACK_ACTIONS_W;
    assert_close(
        columns.title.width(),
        width - 2.0 * space::S3 - fixed - 4.0 * space::S4,
    );
}

#[test]
fn only_the_widest_slim_row_keeps_the_like() {
    for table in [Table::Full, Table::Reduced, Table::Narrow] {
        let present = table.columns().for_slim();
        assert!(
            !present.artist && !present.album && !present.added,
            "{table:?} must clear the same three columns"
        );
        assert_eq!(
            present.like,
            table != Table::Narrow,
            "{table:?} keeps the like exactly until its breakpoint drops it"
        );
        assert!(
            present.duration && present.actions,
            "{table:?} always keeps the length and the more"
        );
    }
}

#[test]
fn only_the_widest_plays_row_keeps_the_like() {
    for table in [Table::Full, Table::Reduced, Table::Narrow] {
        let present = table.columns().for_plays();
        assert!(
            present.artist && !present.album && !present.added,
            "{table:?} must keep the artist and clear the other two"
        );
        assert_eq!(
            present.like,
            table != Table::Narrow,
            "{table:?} keeps the like exactly until its breakpoint drops it"
        );
        assert!(
            present.duration && present.actions,
            "{table:?} always keeps the length and the more"
        );
    }
}

#[test]
fn only_a_playlist_and_a_plays_row_carry_the_artist_column() {
    let width = 900.0;
    let playlist = columns(width, all());
    let plays = columns(width, plays());
    let slim = columns(width, slim());

    assert!(playlist.artist.width() > 0.0);
    assert!(plays.artist.width() > 0.0);
    assert_eq!(slim.artist.width(), 0.0);

    // Only the playlist names the album.
    assert!(playlist.album.width() > 0.0);
    assert_eq!(plays.album.width(), 0.0);
    assert_eq!(slim.album.width(), 0.0);

    // The playlist's absent album leaves no gap behind: the artist starts
    // one gutter after the title whether or not the album is dropped.
    let reduced = columns(
        width,
        Present {
            album: false,
            ..all()
        },
    );
    assert_close(reduced.artist.left(), reduced.title.right() + space::S3);

    // Each shape keeps its own gutter.
    assert_close(playlist.artist.left(), playlist.title.right() + space::S3);
    assert_close(plays.artist.left(), plays.title.right() + space::S4);
    assert_close(slim.like.left(), slim.title.right() + space::S4);
}

#[test]
fn the_title_starts_after_the_index_and_its_gutter() {
    for present in shapes() {
        let columns = columns(900.0, present);
        assert_close(
            columns.title.left(),
            columns.index.right() + gutter(present),
        );
    }
}

#[test]
fn the_playlist_columns_have_not_moved() {
    // The rects the playlist tables solved to before the row shapes were
    // named, at a pane 1000 points wide: naming the shape must not move a
    // single playlist column.
    let expected: [(Table, [(f32, f32); 8]); 3] = [
        (
            Table::Full,
            [
                (12.0, 36.0),
                (60.0, 281.14285),
                (353.14285, 187.42856),
                (552.5714, 187.42859),
                (752.0, 88.0),
                (852.0, 28.0),
                (892.0, 56.0),
                (960.0, 28.0),
            ],
        ),
        (
            Table::Reduced,
            [
                (12.0, 36.0),
                (60.0, 400.8),
                (472.8, 267.2),
                (752.0, 0.0),
                (752.0, 88.0),
                (852.0, 28.0),
                (892.0, 56.0),
                (960.0, 28.0),
            ],
        ),
        (
            Table::Narrow,
            [
                (12.0, 36.0),
                (60.0, 484.80005),
                (556.80005, 323.19995),
                (892.0, 0.0),
                (892.0, 0.0),
                (892.0, 0.0),
                (892.0, 56.0),
                (960.0, 28.0),
            ],
        ),
    ];

    for (table, expected) in expected {
        let solved = columns(1000.0, table.columns());
        for ((name, rect), (left, width)) in rects(solved).into_iter().zip(expected) {
            assert!(
                (rect.left() - left).abs() < 0.01,
                "{table:?} {name} left: {} is not {left}",
                rect.left()
            );
            assert!(
                (rect.width() - width).abs() < 0.01,
                "{table:?} {name} width: {} is not {width}",
                rect.width()
            );
        }
    }
}

#[test]
fn the_number_box_sits_at_the_right_of_the_index_column() {
    let columns = columns(900.0, all());
    let box_rect = number_box(columns.index);

    assert_eq!(box_rect.width(), extent::TRACK_NUMBER_W);
    assert_eq!(box_rect.height(), extent::TRACK_NUMBER_W);
    assert_close(box_rect.right(), columns.index.right());
    assert_close(box_rect.center().y, columns.index.center().y);

    // A column narrower than the box never lets it spill.
    let thin = Rect::from_min_size(pos2(10.0, 0.0), vec2(10.0, 10.0));
    let box_rect = number_box(thin);
    assert_eq!(box_rect.width(), 10.0);
    assert_close(box_rect.right(), thin.right());
}

#[test]
fn a_rows_columns_never_overlap_at_its_narrowest_width() {
    // The narrowest content area the shell hands the tracklist at WINDOW_MIN,
    // then every width up to a full playlist. A playlist keeps the artist by
    // dropping the album; below 760 it also drops the date and the like. A
    // plays row drops the album and the date at every width.
    let narrowest = WINDOW_MIN[0];
    for present in [
        Present {
            album: false,
            ..all()
        },
        Present {
            album: false,
            added: false,
            like: false,
            ..all()
        },
        all().for_plays(),
        Table::Narrow.columns().for_plays(),
        all().for_plays_album(),
        Table::Narrow.columns().for_plays_album(),
    ] {
        for width in [narrowest, 400.0, 700.0, 900.0, 1440.0] {
            let columns = columns(width, present);
            let mut previous: Option<(&str, Rect)> = None;
            for (name, rect) in rects(columns) {
                if rect.width() <= 0.0 {
                    continue;
                }
                if let Some((before, left)) = previous {
                    assert!(
                        rect.left() >= left.right() - 0.01,
                        "{name} at {width} starts at {} before {before} ends at {}",
                        rect.left(),
                        left.right()
                    );
                }
                previous = Some((name, rect));
            }
        }
    }
}

#[test]
fn every_column_stays_inside_the_inner_rect_at_the_minimum_window() {
    // The narrowest content area the shell can hand the tracklist at
    // WINDOW_MIN, then widths below it, for every shape.
    for width in [
        WINDOW_MIN[0] - extent::SIDEBAR_W,
        WINDOW_MIN[0],
        200.0,
        100.0,
        24.0,
        0.0,
    ] {
        for present in shapes() {
            let columns = columns(width, present);
            let inner = inner_rect(width);
            for (name, rect) in rects(columns) {
                assert!(
                    rect.left() >= inner.left() - 0.01,
                    "{name} at {width}: {rect:?} spills left of {inner:?}"
                );
                assert!(
                    rect.right() <= inner.right() + 0.01,
                    "{name} at {width}: {rect:?} spills right of {inner:?}"
                );
                assert!(
                    rect.top() >= inner.top() - 0.01,
                    "{name} at {width}: {rect:?} spills above {inner:?}"
                );
                assert!(
                    rect.bottom() <= inner.bottom() + 0.01,
                    "{name} at {width}: {rect:?} spills below {inner:?}"
                );
                assert!(rect.width() >= 0.0, "{name} at {width} is negative");
                assert!(rect.height() >= 0.0, "{name} at {width} is negative");
            }
        }
    }
}

#[test]
fn a_zero_or_non_finite_width_yields_zero_width_rects() {
    for width in [0.0, -5.0, f32::NAN, f32::INFINITY] {
        let columns = columns(width, all());
        for (name, rect) in rects(columns) {
            assert_eq!(rect.width(), 0.0, "{name} at {width}: {rect:?}");
            assert!(rect.height() >= 0.0, "{name} at {width}: {rect:?}");
        }
    }
}

#[test]
fn title_parts_splits_at_the_art_and_the_gutter() {
    let title = Rect::from_min_size(pos2(0.0, 0.0), vec2(400.0, extent::LIBRARY_ART));
    let parts = title_parts(title, true);
    assert_eq!(parts.art.width(), extent::LIBRARY_ART);
    assert_eq!(parts.art.height(), title.height());
    assert_eq!(parts.text.left(), extent::LIBRARY_ART + space::S3);
    assert_eq!(parts.text.right(), title.right());
}

#[test]
fn title_parts_without_art_hands_the_whole_title_to_the_text() {
    let title = Rect::from_min_size(pos2(10.0, 0.0), vec2(400.0, extent::LIBRARY_ART));
    let parts = title_parts(title, false);
    assert_eq!(parts.art.width(), 0.0);
    assert_eq!(parts.art.min, title.min);
    assert_eq!(parts.art.height(), title.height());
    assert_eq!(parts.text, title);
}

#[test]
fn title_parts_never_goes_negative() {
    for art in [true, false] {
        for width in [
            0.0,
            1.0,
            extent::LIBRARY_ART,
            extent::LIBRARY_ART + space::S3,
            extent::LIBRARY_ART + space::S3 + 100.0,
        ] {
            let title = Rect::from_min_size(pos2(10.0, 0.0), vec2(width, extent::LIBRARY_ART));
            let parts = title_parts(title, art);
            assert!(parts.art.width() >= 0.0, "{width} art={art}");
            assert!(parts.text.width() >= 0.0, "{width} art={art}");
            assert!(
                parts.art.right() <= title.right() + 0.01,
                "{width} art={art}"
            );
            assert!(
                parts.text.right() <= title.right() + 0.01,
                "{width} art={art}"
            );
        }
    }
}

#[test]
fn rows_tile_with_no_gap() {
    let list = Rect::from_min_size(pos2(4.0, 8.0), vec2(300.0, 200.0));
    let row_h = extent::ROW_H;
    let first = row_rect(list, 0, row_h);
    let second = row_rect(list, 1, row_h);
    let third = row_rect(list, 2, row_h);

    assert_eq!(first.min, list.min);
    assert_eq!(first.width(), list.width());
    assert_eq!(first.height(), row_h);
    assert_eq!(first.bottom(), second.top());
    assert_eq!(second.bottom(), third.top());

    assert_eq!(row_rect(list, 0, 0.0).height(), 0.0);
    assert_eq!(row_rect(list, 0, f32::NAN).height(), 0.0);
    assert_eq!(row_rect(list, 0, -10.0).height(), 0.0);
}

fn list() -> Rect {
    Rect::from_min_size(pos2(10.0, 100.0), vec2(300.0, 176.0))
}

#[test]
fn the_boundary_in_a_rows_top_half_is_its_own_index_and_the_bottom_half_the_next() {
    let list = list();
    let row_h = extent::ROW_H;
    assert_eq!(boundary_at(list, list.top() + 1.0, row_h, 4), 0);
    assert_eq!(
        boundary_at(list, list.top() + row_h / 2.0 - 1.0, row_h, 4),
        0
    );
    assert_eq!(
        boundary_at(list, list.top() + row_h / 2.0 + 1.0, row_h, 4),
        1
    );
    assert_eq!(boundary_at(list, list.top() + row_h + 1.0, row_h, 4), 1);
    assert_eq!(
        boundary_at(list, list.top() + 2.0 * row_h - 1.0, row_h, 4),
        2
    );
}

#[test]
fn above_the_list_is_the_first_boundary() {
    let list = list();
    assert_eq!(boundary_at(list, list.top() - 50.0, extent::ROW_H, 4), 0);
}

#[test]
fn below_the_list_is_the_count() {
    let list = list();
    assert_eq!(boundary_at(list, list.bottom() + 50.0, extent::ROW_H, 4), 4);
}

#[test]
fn a_zero_or_non_finite_pitch_is_the_first_boundary() {
    let list = list();
    let y = list.top() + 100.0;
    assert_eq!(boundary_at(list, y, 0.0, 4), 0);
    assert_eq!(boundary_at(list, y, -1.0, 4), 0);
    assert_eq!(boundary_at(list, y, f32::NAN, 4), 0);
    assert_eq!(boundary_at(list, f32::NAN, extent::ROW_H, 4), 0);
}

#[test]
fn a_rect_that_only_shares_an_edge_is_not_on_screen() {
    let visible = Rect::from_min_size(pos2(0.0, 0.0), vec2(100.0, 100.0));
    let below = Rect::from_min_size(pos2(10.0, visible.bottom()), vec2(20.0, 20.0));
    let above = Rect::from_min_size(pos2(10.0, -20.0), vec2(20.0, 20.0));
    let beside = Rect::from_min_size(pos2(visible.right(), 10.0), vec2(20.0, 20.0));

    assert!(
        !on_screen(visible, below),
        "a rect sharing the viewport's bottom edge is not on screen"
    );
    assert!(
        !on_screen(visible, above),
        "a rect sharing the viewport's top edge is not on screen"
    );
    assert!(
        !on_screen(visible, beside),
        "a rect sharing the viewport's side edge is not on screen"
    );

    let overlap = Rect::from_min_size(pos2(10.0, visible.bottom() - 1.0), vec2(20.0, 20.0));
    let inside = Rect::from_min_size(pos2(10.0, 10.0), vec2(20.0, 20.0));
    assert!(
        on_screen(visible, overlap),
        "a rect one point over the fold is on screen"
    );
    assert!(
        on_screen(visible, inside),
        "a rect inside the viewport is on screen"
    );
}

#[test]
fn boundary_at_inverts_row_rect() {
    let list = list();
    let row_h = extent::ROW_H;
    for index in 0..4 {
        let rect = row_rect(list, index, row_h);
        assert_eq!(boundary_at(list, rect.top() + 1.0, row_h, 4), index);
        assert_eq!(boundary_at(list, rect.center().y, row_h, 4), index + 1);
    }
}
