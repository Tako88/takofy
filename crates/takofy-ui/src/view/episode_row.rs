//! One episode row: the play toggle, the name over its description, and the length.
//!
//! The row is handed its data and its rect; it fetches nothing and decides no
//! width. The reference draws an episode as a small square toggle where a track
//! row puts its art, then a text column that wraps to two rows, then the length.
//!
//! A row's identity is its URI and its position in the list. A show cannot list
//! one episode twice today, but the position costs nothing and keeps the row's
//! contract the same as every other list's.

use egui::emath::GuiRounding;
use egui::{FontId, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2, WidgetInfo};

use crate::icon::{self, Icon};
use crate::layout::episode::row_parts;
use crate::theme::icon::Size;
use crate::theme::{
    MonoWeight, Tokens, ease, extent, fonts, motion, radius, space, stroke, text, tracking,
};

use super::controls;
use super::text as recipe;
use super::track_row::{Click, State};

/// What one row draws.
pub struct Row<'a> {
    pub uri: &'a str,
    /// The episode's position in the list, half of its identity.
    pub index: usize,
    pub title: &'a str,
    /// The episode's description, already collapsed to one line; absent draws none.
    pub description: Option<&'a str>,
    /// The byline under the description: the show, then when it was published.
    pub meta: &'a str,
    pub duration: Option<&'a str>,
    pub state: State,
}

/// Draws one row into `rect` and reports a play request.
///
/// The toggle asks for one episode; a double click anywhere on the row asks for
/// the same. A single click only takes focus, so scanning a list does not start
/// playback by accident.
pub fn show(ui: &mut Ui, rect: Rect, row: &Row<'_>, tokens: &Tokens) -> Option<Click> {
    if rect.width() <= 0.0 || rect.height() <= 0.0 {
        return None;
    }
    let key = (row.uri, row.index);
    let response = ui.interact(
        rect,
        ui.scope_id().with(("episode-row", key)),
        Sense::click(),
    );
    response.widget_info(|| WidgetInfo::labeled(egui::Role::Button, true, spoken(row)));
    if response.has_focus() {
        // A row reached by Tab has to come into view, and only a drawn row can
        // hold focus.
        response.scroll_to_me(None);
    }

    let highlight = ui.ctx().animate_bool_with_time_and_easing(
        ui.scope_id().with(("episode-row-hover", key)),
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
    // The reference closes every row with a hairline rather than drawing a box.
    ui.painter().rect_filled(
        Rect::from_min_size(
            Pos2::new(rect.left(), rect.bottom() - stroke::HAIRLINE),
            Vec2::new(rect.width(), stroke::HAIRLINE),
        ),
        0.0,
        tokens.palette.border,
    );

    let duration = row.duration.map(|label| {
        recipe::spaced(
            ui,
            &label.to_uppercase(),
            FontId::new(text::XXS, fonts::mono(MonoWeight::Medium)),
            tokens.palette.text_subtle,
            recipe::tracking_pts(text::XXS, tracking::WIDE),
        )
    });
    let duration_w = duration.as_ref().map_or(0.0, |line| line.size().x);
    let parts = row_parts(rect, duration_w);

    let played = toggle(ui, parts.toggle, row, key, tokens);

    let text_width = parts.body.width().min(measure(ui));
    if text_width > 0.0 {
        text_block(ui, parts.body, row, text_width, tokens);
    }
    if let Some(line) = duration {
        ui.painter().galley(
            Pos2::new(parts.duration.right() - line.size().x, parts.duration.top()),
            line,
            tokens.palette.text_subtle,
        );
    }

    // Keyboard activation is answered before the pointer's, so a click landing in
    // the same frame can never swallow it.
    let keyboard = response.has_focus()
        && ui.input(|input| {
            input.key_pressed(egui::Key::Enter) || input.key_pressed(egui::Key::Space)
        });
    if keyboard || played || response.double_clicked() {
        return Some(Click::Play);
    }
    None
}

/// The bordered square that starts one episode.
fn toggle(ui: &mut Ui, rect: Rect, row: &Row<'_>, key: (&str, usize), tokens: &Tokens) -> bool {
    if rect.width() <= 0.0 || rect.height() <= 0.0 {
        return false;
    }
    let response = ui.interact(
        rect,
        ui.scope_id().with(("episode-toggle", key)),
        Sense::click(),
    );
    response.widget_info(|| {
        let label = if row.state == State::Playing {
            format!("Pause {}", row.title)
        } else {
            format!("Play {}", row.title)
        };
        WidgetInfo::labeled(egui::Role::Button, true, label)
    });
    if response.has_focus() {
        response.scroll_to_me(None);
    }

    let hover = ui.ctx().animate_bool_with_time_and_easing(
        ui.scope_id().with(("episode-toggle-hover", key)),
        response.hovered() || response.has_focus(),
        motion::FAST,
        ease::out,
    );
    let current = row.state != State::Resting;
    let border = if current {
        tokens.palette.accent
    } else if hover > 0.0 {
        tokens.palette.text
    } else {
        tokens.palette.border_strong
    };
    ui.painter().rect_stroke(
        rect,
        radius::SM,
        Stroke::new(stroke::HAIRLINE, border),
        StrokeKind::Inside,
    );
    let glyph = if row.state == State::Playing {
        Icon::Pause
    } else {
        Icon::Play
    };
    let color = if current || hover > 0.0 {
        tokens.palette.accent
    } else {
        tokens.palette.text
    };
    icon::paint(
        ui.painter(),
        glyph,
        Rect::from_center_size(rect.center(), Vec2::splat(Size::BASE.side))
            .round_to_pixels(ui.pixels_per_point()),
        Size::BASE,
        color,
    );
    response.clicked()
}

/// The title, the clamped description and the byline, stacked from the top.
fn text_block(ui: &Ui, rect: Rect, row: &Row<'_>, width: f32, tokens: &Tokens) {
    let mut top = rect.top();
    let title = recipe::line(
        ui,
        row.title,
        recipe::episode_title(),
        tokens.palette.text,
        0.0,
        width,
    );
    ui.painter().galley(
        Pos2::new(rect.left(), top),
        title.clone(),
        tokens.palette.text,
    );
    top += title.size().y;

    if let Some(description) = row.description.filter(|text| !text.is_empty()) {
        let galley = recipe::clamped(
            ui,
            description,
            recipe::episode_text(),
            tokens.palette.text_muted,
            0.0,
            width,
            2,
        );
        top += space::S2;
        ui.painter().galley(
            Pos2::new(rect.left(), top),
            galley.clone(),
            tokens.palette.text_muted,
        );
        top += galley.size().y;
    }

    if !row.meta.is_empty() {
        let line = recipe::spaced(
            ui,
            &row.meta.to_uppercase(),
            FontId::new(text::XXS, fonts::mono(MonoWeight::Medium)),
            tokens.palette.text_subtle,
            recipe::tracking_pts(text::XXS, tracking::WIDE),
        );
        top += space::S2;
        ui.painter().galley(
            Pos2::new(rect.left(), top),
            line,
            tokens.palette.text_subtle,
        );
    }
}

/// The measure a description is allowed, `EPISODE_TEXT_CH` characters wide.
fn measure(ui: &Ui) -> f32 {
    let font = recipe::episode_text();
    let width = ui.ctx().fonts_mut(|fonts| fonts.glyph_width(&font, '0'));
    width * extent::EPISODE_TEXT_CH
}

/// What a screen reader says for the whole row.
fn spoken(row: &Row<'_>) -> String {
    if row.meta.is_empty() {
        row.title.to_owned()
    } else {
        format!("{}, {}", row.title, row.meta)
    }
}

#[cfg(test)]
mod tests {
    use egui::{Context, Event, Pos2, RawInput, Shape, Ui, vec2};

    use super::*;
    use crate::theme::{fonts, style};

    const PROBE: Rect = Rect {
        min: Pos2::new(10.0, 10.0),
        max: Pos2::new(610.0, 130.0),
    };

    fn context() -> Context {
        let ctx = Context::default();
        ctx.set_fonts(fonts::definitions());
        ctx.set_global_style(style(&Tokens::DARK));
        ctx
    }

    fn input(events: Vec<Event>) -> RawInput {
        RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(900.0, 400.0))),
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

    fn row<'a>(title: &'a str) -> Row<'a> {
        Row {
            uri: "spotify:episode:a",
            index: 0,
            title,
            description: None,
            meta: "",
            duration: None,
            state: State::Resting,
        }
    }

    fn key(key: egui::Key) -> Event {
        Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }
    }

    #[test]
    fn a_toggle_click_returns_play() {
        let ctx = context();
        let tokens = Tokens::DARK;
        let row = row("A Title");
        let centre = PROBE.min
            + vec2(
                space::S3 + extent::EPISODE_TOGGLE / 2.0,
                space::S4 + extent::EPISODE_TOGGLE / 2.0,
            );
        let mut played = false;
        for event in crate::view::test_support::click_events(centre) {
            let played = &mut played;
            run(&ctx, input(vec![event]), |ui| {
                if show(ui, PROBE, &row, &tokens) == Some(Click::Play) {
                    *played = true;
                }
            });
        }
        assert!(played, "the toggle must ask to play");
    }

    #[test]
    fn keyboard_activation_returns_play() {
        for pressed in [egui::Key::Enter, egui::Key::Space] {
            let ctx = context();
            let tokens = Tokens::DARK;
            let row = row("A Title");
            // Warm up the focus traversal, then Tab onto the row.
            for events in [Vec::new(), vec![key(egui::Key::Tab)]] {
                run(&ctx, input(events), |ui| {
                    let _ = show(ui, PROBE, &row, &tokens);
                });
            }
            let mut played = false;
            run(&ctx, input(vec![key(pressed)]), |ui| {
                played = show(ui, PROBE, &row, &tokens) == Some(Click::Play);
            });
            assert!(played, "{pressed:?} on the focused row must play");
        }
    }

    #[test]
    fn a_single_click_on_the_text_does_not_play() {
        // Scanning the list with the pointer must not start an episode: only the
        // toggle and a double click do.
        let ctx = context();
        let tokens = Tokens::DARK;
        let row = row("A Title");
        let mut played = false;
        for event in crate::view::test_support::click_events(Pos2::new(300.0, 40.0)) {
            let played = &mut played;
            run(&ctx, input(vec![event]), |ui| {
                if show(ui, PROBE, &row, &tokens) == Some(Click::Play) {
                    *played = true;
                }
            });
        }
        assert!(!played, "one click on the row is not a play");
    }

    #[test]
    fn the_title_the_description_and_the_meta_are_painted() {
        let ctx = context();
        let tokens = Tokens::DARK;
        let row = Row {
            description: Some("A long way home."),
            meta: "Field Notes · 3 days ago",
            duration: Some("46 min"),
            ..row("A Title")
        };
        let mut output = ctx.run_ui(input(Vec::new()), |ui| {
            let _ = show(ui, PROBE, &row, &tokens);
        });
        output.textures_delta.clear();
        let texts = drawn_texts(&output);
        for label in ["A Title", "A long way home."] {
            assert!(
                texts.contains(&label),
                "the row must paint {label}: {texts:?}"
            );
        }
        assert!(
            texts.iter().any(|text| text.contains("FIELD NOTES")),
            "the meta line is upper-cased: {texts:?}"
        );
        assert!(
            texts.iter().any(|text| text.contains("46 MIN")),
            "the duration is upper-cased: {texts:?}"
        );
    }
}
