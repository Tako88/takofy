//! One row of the Now Playing queue.
//!
//! The row is handed its data and its rect; it fetches nothing and decides no
//! width. What it decides is what a row looks like: the cover, the name over the
//! artist, the length, and a hover wash that follows the pointer.
//!
//! A row's identity is its absolute index in the queue, never its uri. Duplicate
//! adjacent entries are legal, so keying on the uri would give both rows one id,
//! one hover and one click, and the wrong occurrence would play. The row does not
//! carry a uri at all, which is what keeps that mistake out of reach.

use egui::{FontId, Pos2, Rect, Sense, Ui, WidgetInfo};

use crate::cover::Cover;
use crate::layout::queue::row_parts;
use crate::theme::{Tokens, ease, motion, radius, text};

use super::art;
use super::controls;
use super::text as recipe;

/// What one row draws.
pub struct Row<'a> {
    /// The entry's position in the queue, which is its identity.
    pub index: usize,
    pub title: &'a str,
    pub artist: &'a str,
    pub duration: Option<&'a str>,
    pub cover: Cover,
}

/// Draws one row into `rect` and reports a click on it.
///
/// Keyboard activation is egui's own: a `Sense::click` widget reports a click for
/// Enter through `FAKE_PRIMARY_CLICKED`, so the row has no key branch of its own
/// to leave untested. A space never gets this far — the shell takes it for the
/// transport before anything draws — so a focused row cannot start a track.
pub fn show(ui: &mut Ui, rect: Rect, row: &Row<'_>, tokens: &Tokens) -> bool {
    let response = ui.interact(
        rect,
        ui.scope_id().with(("queue-row", row.index)),
        Sense::click(),
    );
    response.widget_info(|| {
        WidgetInfo::labeled(
            egui::Role::Button,
            true,
            format!("{} by {}", row.title, row.artist),
        )
    });
    if response.clicked() {
        // The keyboard follows the pointer: Tab continues from the row that was
        // picked.
        response.request_focus();
    }
    if response.has_focus() {
        // A row reachable by Tab has to come into view when it is reached, and
        // only a drawn row can hold focus.
        response.scroll_to_me(None);
    }

    let highlight = ui.ctx().animate_bool_with_time_and_easing(
        ui.scope_id().with(("queue-row-hover", row.index)),
        response.hovered() || response.has_focus(),
        motion::FAST,
        ease::out,
    );
    if highlight > 0.0 {
        ui.painter().rect_filled(
            rect,
            radius::SM,
            tokens.palette.surface_hover.gamma_multiply(highlight),
        );
    }
    if response.has_focus() {
        controls::focus_ring(ui, rect, tokens);
    }

    let duration = row.duration.map(|label| {
        recipe::line(
            ui,
            label,
            recipe::track_mono(text::XXS),
            tokens.palette.text_subtle,
            0.0,
            rect.width(),
        )
    });
    let duration_w = duration.as_ref().map_or(0.0, |line| line.size().x);
    let parts = row_parts(rect, duration_w);

    art::tile(
        ui.painter(),
        parts.art,
        radius::XS,
        tokens.palette.border,
        row.cover,
        tokens,
    );

    if parts.text.width() > 0.0 {
        stacked(ui, parts.text, row, tokens);
    }
    if let Some(line) = duration {
        ui.painter().galley(
            Pos2::new(
                parts.duration.right() - line.size().x,
                parts.duration.center().y - line.size().y / 2.0,
            ),
            line,
            tokens.palette.text_subtle,
        );
    }

    response.clicked()
}

/// The name over the artist, centred together as one block.
fn stacked(ui: &Ui, rect: Rect, row: &Row<'_>, tokens: &Tokens) {
    let name = recipe::queue_name();
    let name_h = line_height(ui, &name);
    let artist = recipe::queue_artist();
    let artist_h = if row.artist.is_empty() {
        0.0
    } else {
        line_height(ui, &artist)
    };
    let top = rect.center().y - (name_h + artist_h) / 2.0;

    let line = recipe::line(ui, row.title, name, tokens.palette.text, 0.0, rect.width());
    ui.painter()
        .galley(Pos2::new(rect.left(), top), line, tokens.palette.text);
    if artist_h > 0.0 {
        let line = recipe::line(
            ui,
            row.artist,
            artist,
            tokens.palette.text_subtle,
            0.0,
            rect.width(),
        );
        ui.painter().galley(
            Pos2::new(rect.left(), top + name_h),
            line,
            tokens.palette.text_subtle,
        );
    }
}

/// The height of one line of `font`.
fn line_height(ui: &Ui, font: &FontId) -> f32 {
    ui.ctx().fonts_mut(|fonts| fonts.row_height(font))
}

#[cfg(test)]
mod tests {
    use egui::{Context, Event, Pos2, RawInput, Shape, Ui, vec2};

    use super::*;
    use crate::theme::{fonts, style};

    const PROBE: Rect = Rect {
        min: Pos2::new(10.0, 10.0),
        max: Pos2::new(282.0, 58.0),
    };

    fn context() -> Context {
        let ctx = Context::default();
        ctx.set_fonts(fonts::definitions());
        ctx.set_global_style(style(&Tokens::DARK));
        ctx
    }

    fn input(events: Vec<Event>) -> RawInput {
        RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(400.0, 200.0))),
            events,
            ..Default::default()
        }
    }

    fn run(ctx: &Context, input: RawInput, draw: impl FnMut(&mut Ui)) {
        let mut output = ctx.run_ui(input, draw);
        output.textures_delta.clear();
    }

    fn drawn_texts(output: &egui::FullOutput) -> Vec<&str> {
        output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                Shape::Text(text) => Some(text.galley.text()),
                _ => None,
            })
            .collect()
    }

    fn row<'a>(index: usize, title: &'a str, artist: &'a str) -> Row<'a> {
        Row {
            index,
            title,
            artist,
            duration: None,
            cover: Cover::Missing,
        }
    }

    /// Clicks `centre` and reports whether either row of a two-row harness
    /// answered, as `(first, second)`.
    fn click_both(ctx: &Context, centre: Pos2, left: &Row<'_>, right: &Row<'_>) -> (bool, bool) {
        let tokens = Tokens::DARK;
        let right_rect = Rect::from_min_size(
            Pos2::new(PROBE.left(), PROBE.bottom() + crate::layout::queue::ROW_H),
            PROBE.size(),
        );
        let mut clicked = (false, false);
        run(ctx, input(Vec::new()), |ui| {
            let _ = show(ui, PROBE, left, &tokens);
            let _ = show(ui, right_rect, right, &tokens);
        });
        for event in crate::view::test_support::click_events(centre) {
            let clicked = &mut clicked;
            run(ctx, input(vec![event]), |ui| {
                let first = show(ui, PROBE, left, &tokens);
                let second = show(ui, right_rect, right, &tokens);
                if first || second {
                    *clicked = (first, second);
                }
            });
        }
        clicked
    }

    #[test]
    fn the_name_the_artist_and_the_duration_are_all_painted() {
        let ctx = context();
        let tokens = Tokens::DARK;
        let mut output = ctx.run_ui(input(Vec::new()), |ui| {
            let _ = show(
                ui,
                PROBE,
                &Row {
                    duration: Some("3:42"),
                    ..row(0, "A Title", "A Band")
                },
                &tokens,
            );
        });
        output.textures_delta.clear();
        let texts = drawn_texts(&output);
        for label in ["A Title", "A Band", "3:42"] {
            assert!(
                texts.contains(&label),
                "the row must paint {label}: {texts:?}"
            );
        }
    }

    #[test]
    fn a_row_with_no_duration_still_lays_out() {
        // The entry's length is optional metadata, and a row without one must
        // still draw its name rather than skip the row.
        let ctx = context();
        let tokens = Tokens::DARK;
        let mut output = ctx.run_ui(input(Vec::new()), |ui| {
            let _ = show(ui, PROBE, &row(0, "A Title", "A Band"), &tokens);
        });
        output.textures_delta.clear();
        assert!(drawn_texts(&output).contains(&"A Title"));
    }

    #[test]
    fn a_row_with_no_artist_paints_no_artist_line() {
        let ctx = context();
        let tokens = Tokens::DARK;
        let mut output = ctx.run_ui(input(Vec::new()), |ui| {
            let _ = show(ui, PROBE, &row(0, "A Title", ""), &tokens);
        });
        output.textures_delta.clear();
        let texts = drawn_texts(&output);
        assert!(texts.contains(&"A Title"), "{texts:?}");
        assert!(
            !texts.iter().any(|text| text.is_empty()),
            "an empty artist must paint nothing at all: {texts:?}"
        );
    }

    #[test]
    fn a_click_on_the_row_reports() {
        let ctx = context();
        let (first, second) = click_both(
            &ctx,
            PROBE.center(),
            &row(0, "First", "A Band"),
            &row(1, "Second", "Another Band"),
        );
        assert!(first, "the clicked row must report its click");
        assert!(!second, "the other row must not");
    }

    #[test]
    fn two_rows_with_identical_text_have_different_ids() {
        // Keyed by anything but the index — the uri, the text — the two rows
        // would share one id, and one click would drive both. Only the first
        // answering is what proves they did not.
        let ctx = context();
        let (first, second) = click_both(
            &ctx,
            PROBE.center(),
            &row(0, "Same", "Same"),
            &row(1, "Same", "Same"),
        );
        assert!(first);
        assert!(!second);
    }

    #[test]
    fn the_row_paints_nothing_into_a_zero_width_rect() {
        let tokens = Tokens::DARK;
        let ctx = context();
        let empty = Rect::from_min_size(Pos2::new(10.0, 10.0), vec2(0.0, 0.0));
        let mut output = ctx.run_ui(input(Vec::new()), |ui| {
            let _ = show(ui, empty, &row(0, "A Title", "A Band"), &tokens);
        });
        output.textures_delta.clear();
        assert!(
            !drawn_texts(&output).contains(&"A Title"),
            "a zero-width row must not lay a galley out"
        );
    }
}
