use super::super::*;

use crate::message::Update;
use crate::theme::{fonts, style};

/// The chip rect and label rect the real sidebar paints, per label.
///
/// A chip's label is painted at the chip's top-left plus its padding, so
/// this recovers both boxes from what the draw actually produced.
fn painted_chips(width: f32) -> Vec<(&'static str, Rect)> {
    let tokens = Tokens::DARK;
    let ctx = egui::Context::default();
    ctx.set_fonts(fonts::definitions());
    ctx.set_global_style(style(&tokens));
    let mut library = Library::default();
    library.apply(&Update::Playlists(vec![PlaylistEntry {
        composed: Vec::new(),
        uri: "spotify:playlist:a".to_owned(),
        name: "A".to_owned(),
        named: true,
        cover: None,
        description: None,
        owner: None,
        tracks: None,
        access: None,
    }]));
    let mut art = Art::default();
    let input = egui::RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(width, 800.0))),
        ..Default::default()
    };
    let mut output = ctx.run_ui(input, |ui| {
        let _ = show(ui, &mut library, &mut art, Instant::now(), &tokens, true).command;
    });
    output.textures_delta.clear();

    let mut chips = Vec::new();
    for label in ["PLAYLISTS", "ARTISTS", "ALBUMS", "PODCASTS"] {
        let Some((pos, size)) = output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Text(text) if text.galley.text() == label => {
                    Some((text.pos, text.galley.size()))
                }
                _ => None,
            })
        else {
            continue;
        };
        let rect = Rect::from_min_size(
            Pos2::new(pos.x - extent::CHIP_PAD_X, pos.y - extent::CHIP_PAD_Y),
            Vec2::new(
                size.x + 2.0 * extent::CHIP_PAD_X,
                size.y + 2.0 * extent::CHIP_PAD_Y,
            ),
        );
        let label: &'static str = match label {
            "PLAYLISTS" => "Playlists",
            "ARTISTS" => "Artists",
            "ALBUMS" => "Albums",
            _ => "Podcasts",
        };
        chips.push((label, rect));
    }
    chips
}

/// The four filters' labels, and where their chips are centred.
///
/// The rect is rebuilt from the painted glyps, so it is the label's own box
/// rather than the cell: a chip's label is centred in its cell, which is
/// what this pins.
#[test]
fn the_drawn_chips_keep_their_characterised_positions() {
    // The sidebar is a fixed 256 points, in the column and in the drawer, so
    // these are the two widths the grid is ever solved at: the panel less its
    // hairline, and the panel itself.
    for width in [254.0, 256.0] {
        let chips = painted_chips(width);
        assert_eq!(chips.len(), sidebar_kind::ROW.len(), "band width {width}");

        let cell = (width - 2.0 * space::S4 - space::S1) / 2.0;
        for (index, (label, rect)) in chips.iter().enumerate() {
            assert_eq!(label, &sidebar_kind_label(sidebar_kind::ROW[index]));
            let column = index % CHIP_COLUMNS;
            let row = index / CHIP_COLUMNS;
            let centre_x = space::S4 + column as f32 * (cell + space::S1) + cell / 2.0;
            let centre_y =
                space::S4 + row as f32 * (extent::BUTTON + space::S1) + extent::BUTTON / 2.0;
            assert!(
                (rect.center().x - centre_x).abs() < 0.01
                    && (rect.center().y - centre_y).abs() < 0.01,
                "band width {width}: {label} centred at {:?}, not ({centre_x}, {centre_y})",
                rect.center()
            );
        }
    }
}

#[test]
fn the_four_filters_sit_in_two_equal_columns() {
    let chips = painted_chips(254.0);
    assert_eq!(chips.len(), 4);
    let cell = (254.0 - 2.0 * space::S4 - space::S1) / 2.0;
    // Two chips share a row, and the columns are the same width, so the
    // block reads as a grid rather than as four buttons that happened to fit.
    assert!((chips[0].1.center().y - chips[1].1.center().y).abs() < 0.01);
    assert!((chips[2].1.center().y - chips[3].1.center().y).abs() < 0.01);
    assert!(chips[2].1.center().y > chips[0].1.center().y);
    for pair in [(0, 1), (2, 3)] {
        let gap = chips[pair.1].1.center().x - chips[pair.0].1.center().x;
        assert!(
            (gap - (cell + space::S1)).abs() < 0.01,
            "columns are one cell apart"
        );
    }
}
