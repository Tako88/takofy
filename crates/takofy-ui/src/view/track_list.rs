//! The tracklist's header and the rect its rows are laid out in.
//!
//! The playlist and the album draw the same table from the same solver, so the
//! header lives here rather than in either page: a head and its rows can never
//! disagree about where a column is when both are handed one `Columns`.
//!
//! The header mirrors the row: the number sits in the same right-justified box
//! the row draws it in, and a column the row does not carry arrives zero-width,
//! so it paints no heading either. That is what makes one `head` serve both a
//! playlist's five headings and an album's two.

use egui::emath::GuiRounding;
use egui::{FontId, Pos2, Rect, Ui, vec2};

use crate::icon;
use crate::layout::table as table_layout;
use crate::theme::icon::Size;
use crate::theme::{Tokens, extent, space, stroke, text, tracking};

use super::text as recipe;

/// The rect the rows are laid out in: the list's width, and the height of every
/// row, starting where the cursor is — which is under the header.
///
/// The view and its test both call this, so a row's place and a test's click
/// cannot drift apart.
pub(crate) fn list_rect(ui: &mut Ui, count: usize) -> Rect {
    Rect::from_min_size(
        ui.cursor().min,
        vec2(ui.available_width(), extent::ROW_H * count as f32),
    )
}

/// The column headings, in the same columns as the rows beneath them.
pub(crate) fn head(ui: &mut Ui, columns: table_layout::Columns, tokens: &Tokens) {
    let font = recipe::track_mono(text::XXS);
    let line = ui.ctx().fonts_mut(|fonts| fonts.row_height(&font));
    let band = ui
        .allocate_space(vec2(ui.available_width(), line + space::S2))
        .1;
    let columns = columns.at(Pos2::new(band.left(), band.top()));
    let cell = |rect: Rect| Rect::from_x_y_ranges(rect.x_range(), band.y_range());

    // The header mirrors the row: the number sits in the same right-justified
    // box the row draws it in.
    heading(
        ui,
        table_layout::number_box(cell(columns.index)),
        "#",
        Align::Centre,
        &font,
        tokens,
    );
    heading(ui, cell(columns.title), "Title", Align::Left, &font, tokens);
    heading(
        ui,
        cell(columns.artist),
        "Artist",
        Align::Left,
        &font,
        tokens,
    );
    heading(ui, cell(columns.album), "Album", Align::Left, &font, tokens);
    heading(ui, cell(columns.added), "Added", Align::Left, &font, tokens);

    // The duration column is headed by a clock rather than by a word, and the like
    // and the actions columns carry no heading: a label over a glyph repeats it.
    // A block-level glyph ignores text alignment, so the clock is placed at the
    // column's right edge explicitly, where the row's duration ends.
    let clock = cell(columns.duration);
    if clock.width() > 0.0 {
        let side = Size::SM.side;
        icon::paint(
            ui.painter(),
            icon::Icon::Clock,
            Rect::from_min_size(
                Pos2::new(clock.right() - side, clock.center().y - side / 2.0),
                vec2(side, side),
            )
            .round_to_pixels(ui.pixels_per_point()),
            Size::SM,
            tokens.palette.text_subtle,
        );
    }

    ui.painter().rect_filled(
        Rect::from_min_size(
            Pos2::new(band.left(), band.bottom()),
            vec2(band.width(), stroke::HAIRLINE),
        ),
        0.0,
        tokens.palette.border,
    );
    ui.add_space(space::S1);
}

/// Where a heading sits across its column.
#[derive(Clone, Copy)]
enum Align {
    Left,
    Centre,
}

/// One heading, upper-cased and tracked apart, in its column.
fn heading(ui: &Ui, rect: Rect, label: &str, align: Align, font: &FontId, tokens: &Tokens) {
    if rect.width() <= 0.0 || label.is_empty() {
        return;
    }
    let galley = recipe::line(
        ui,
        &label.to_uppercase(),
        font.clone(),
        tokens.palette.text_subtle,
        recipe::tracking_pts(text::XXS, tracking::CAPS),
        rect.width(),
    );
    let x = match align {
        Align::Left => rect.left(),
        Align::Centre => rect.center().x - galley.size().x / 2.0,
    };
    let y = rect.center().y - galley.size().y / 2.0;
    ui.painter()
        .galley(Pos2::new(x, y), galley, tokens.palette.text_subtle);
}
