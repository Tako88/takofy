//! One row of the Now Playing queue.
//!
//! The row is handed its data and its rect; it fetches nothing and decides no
//! width. What it decides is what a row looks like: the cover, the name over the
//! artist, the length, and a wash that follows the pointer.
//!
//! A row's identity is its absolute index in the queue, never its uri. Duplicate
//! adjacent entries are legal, so keying on the uri would give both rows one id,
//! one hover and one click, and the wrong occurrence would play. The row does not
//! carry a uri at all, which is what keeps that mistake out of reach.
//!
//! Its body is a click target and a drag source both. Enter and a double click
//! play the entry; a single click picks the row, and a drag lifts it, so scanning
//! the list with the pointer never starts playback.

use egui::{FontId, Id, Pos2, Rect, Ui, WidgetInfo};

use crate::cover::Cover;
use crate::layout::queue::row_parts;
use crate::theme::{Tokens, radius, space, text, tracking};

use super::rows::{self, Ghosting, Look};
use super::text as recipe;
use super::track_row::Click;
use super::{art, controls};

/// What one row draws.
pub struct Row<'a> {
    /// The entry's position in the queue, which is its identity.
    pub index: usize,
    pub title: &'a str,
    pub artist: &'a str,
    pub duration: Option<&'a str>,
    pub cover: Cover,
    /// Where the row stands in the queue's selection, and in a carry.
    pub look: Look,
    /// Whether this row is the one a just-finished edit landed on.
    pub refocus: bool,
}

/// The id a row's body is interacted with, so a caller can ask whether it holds
/// the keyboard.
pub fn id(ui: &Ui, index: usize) -> Id {
    ui.scope_id().with(("queue-row", index))
}

/// Draws one row into `rect` and reports what the pointer or the keyboard asked
/// for.
pub fn show(ui: &mut Ui, rect: Rect, row: &Row<'_>, tokens: &Tokens) -> Option<Click> {
    let body = rows::body(ui, rect, id(ui, row.index), row.look);
    let response = &body.response;
    response.widget_info(|| {
        WidgetInfo::labeled(
            egui::Role::Button,
            true,
            format!("{} by {}", row.title, row.artist),
        )
    });
    if row.refocus {
        // A row an edit landed on takes the keyboard with it, so Enter plays what
        // was just placed.
        response.request_focus();
    }

    let hover = rows::fade(
        ui,
        ui.scope_id().with(("queue-row-hover", row.index)),
        response.hovered() || response.has_focus(),
        row.look,
    );
    let before = rows::dim(ui, row.look);
    rows::wash(ui, rect, hover, row.look, tokens);
    // A ring on a carried row is noise: the pointer already holds it.
    if !row.look.carrying {
        controls::focus_ring(ui, response, rect, radius::SM, tokens);
    }
    paint(ui, rect, row, None, tokens);
    ui.set_opacity(before);

    // The row's own keyboard activation is answered before the pointer's, so a
    // click landing in the same frame can never swallow it.
    let keyboard = response.has_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
    if keyboard {
        return Some(Click::Play);
    }
    if response.double_clicked() {
        return Some(Click::Play);
    }
    if body.lift {
        return Some(Click::Lift);
    }
    body.press.map(Click::Select)
}

/// One row's own paint, kept so a carried row can be drawn away from its list.
#[derive(Clone, Debug, PartialEq)]
pub struct Lifted {
    title: String,
    artist: String,
    duration: Option<String>,
    cover: Cover,
}

impl Row<'_> {
    /// The row as it is drawn in flight: owned, and free of interaction.
    pub fn lifted(&self) -> Lifted {
        Lifted {
            title: self.title.to_owned(),
            artist: self.artist.to_owned(),
            duration: self.duration.map(str::to_owned),
            cover: self.cover,
        }
    }
}

impl Lifted {
    /// A ghost's own text, for a row whose list draws no queue row of its own.
    pub fn new(title: String, artist: String, duration: Option<String>, cover: Cover) -> Self {
        Self {
            title,
            artist,
            duration,
            cover,
        }
    }

    /// Draws the row into `rect` with no interaction of its own.
    pub fn paint(&self, ui: &Ui, rect: Rect, ghosting: Ghosting<'_>, tokens: &Tokens) {
        let row = Row {
            index: 0,
            title: &self.title,
            artist: &self.artist,
            duration: self.duration.as_deref(),
            cover: self.cover,
            look: Look::default(),
            refocus: false,
        };
        paint(ui, rect, &row, Some(ghosting), tokens);
    }
}

/// Draws the cover, the name over the artist, and the length.
///
/// `ghosting` is set when this is a carried block rather than a row in its list:
/// it brings the block's own count and running time, and the flat ink a refused
/// target asks for.
fn paint(ui: &Ui, rect: Rect, row: &Row<'_>, ghosting: Option<Ghosting<'_>>, tokens: &Tokens) {
    let flat = ghosting.is_some_and(|ghost| ghost.flat);
    let count = ghosting.and_then(|ghost| (ghost.count > 1).then_some(ghost.count));
    // A block's total stands in for its rows' lengths, and an unknown one stands
    // in for nothing rather than for the row under the pointer.
    let length = match ghosting {
        Some(ghost) if ghost.count > 1 => ghost.total,
        Some(ghost) => ghost.total.or(row.duration),
        None => row.duration,
    };
    let duration = length.map(|label| {
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
        stacked(ui, parts.text, row, count, flat, tokens);
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
}

/// The name over the artist, centred together as one block, with the count a
/// carried block stands for beside the name.
fn stacked(ui: &Ui, rect: Rect, row: &Row<'_>, count: Option<usize>, flat: bool, tokens: &Tokens) {
    let name = recipe::queue_name();
    let name_h = line_height(ui, &name);
    let artist = recipe::queue_artist();
    let artist_h = if row.artist.is_empty() {
        0.0
    } else {
        line_height(ui, &artist)
    };
    let top = rect.center().y - (name_h + artist_h) / 2.0;
    // The flat ink a refused target gets, so the ghost reads as set back from
    // what it is over.
    let title_ink = if flat {
        tokens.palette.text_subtle
    } else {
        tokens.palette.text
    };
    let secondary_ink = tokens.palette.text_subtle;

    let mut width = rect.width();
    let suffix = count.map(|count| {
        let galley = recipe::spaced(
            ui,
            &format!("×{count}"),
            recipe::track_mono(text::XXS),
            secondary_ink,
            recipe::tracking_pts(text::XXS, tracking::WIDE),
        );
        // The count takes its room from the name rather than from the column, so
        // the two never overlap.
        width = (rect.width() - galley.size().x - space::S2).max(0.0);
        galley
    });

    let line = recipe::line(ui, row.title, name, title_ink, 0.0, width);
    ui.painter()
        .galley(Pos2::new(rect.left(), top), line.clone(), title_ink);
    if let Some(suffix) = suffix {
        ui.painter().galley(
            Pos2::new(rect.left() + line.size().x + space::S2, top),
            suffix,
            secondary_ink,
        );
    }
    if artist_h > 0.0 {
        let line = recipe::line(ui, row.artist, artist, secondary_ink, 0.0, rect.width());
        ui.painter()
            .galley(Pos2::new(rect.left(), top + name_h), line, secondary_ink);
    }
}

/// The height of one line of `font`.
fn line_height(ui: &Ui, font: &FontId) -> f32 {
    ui.ctx().fonts_mut(|fonts| fonts.row_height(font))
}

#[cfg(test)]
mod tests {
    use egui::{Context, Event, Modifiers, Pos2, RawInput, Shape, Ui, Vec2, vec2};

    use super::*;
    use crate::selection::Press;
    use crate::theme::{fonts, style};
    use crate::view::test_support::tab;

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
            look: Look::default(),
            refocus: false,
        }
    }

    /// A press at `start`, a move `travel` points right, and the release.
    fn drag_events(start: Pos2, travel: f32) -> Vec<Event> {
        let end = start + Vec2::new(travel, 0.0);
        vec![
            Event::PointerMoved(start),
            crate::view::test_support::button(start, true),
            Event::PointerMoved(end),
            crate::view::test_support::button(end, false),
        ]
    }

    /// Clicks `centre` and reports what either row of a two-row harness answered,
    /// as `(first, second)`.
    fn click_both(
        ctx: &Context,
        centre: Pos2,
        left: &Row<'_>,
        right: &Row<'_>,
    ) -> (Option<Click>, Option<Click>) {
        let tokens = Tokens::DARK;
        let right_rect = Rect::from_min_size(
            Pos2::new(PROBE.left(), PROBE.bottom() + crate::layout::queue::ROW_H),
            PROBE.size(),
        );
        let mut clicked = (None, None);
        run(ctx, input(Vec::new()), |ui| {
            let _ = show(ui, PROBE, left, &tokens);
            let _ = show(ui, right_rect, right, &tokens);
        });
        for event in crate::view::test_support::click_events(centre) {
            let clicked = &mut clicked;
            run(ctx, input(vec![event]), |ui| {
                let first = show(ui, PROBE, left, &tokens);
                let second = show(ui, right_rect, right, &tokens);
                if first.is_some() || second.is_some() {
                    *clicked = (first, second);
                }
            });
        }
        clicked
    }

    /// Draws one row for a single pass and reports what it answered.
    fn answer(ctx: &Context, row: &Row<'_>, events: Vec<Event>) -> Option<Click> {
        let tokens = Tokens::DARK;
        let mut answer = None;
        run(ctx, input(events), |ui| {
            answer = show(ui, PROBE, row, &tokens);
        });
        answer
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
    fn a_single_click_on_the_body_selects_and_does_not_play() {
        let ctx = context();
        let (first, second) = click_both(
            &ctx,
            PROBE.center(),
            &row(0, "First", "A Band"),
            &row(1, "Second", "Another Band"),
        );
        assert_eq!(
            first,
            Some(Click::Select(Press::Replace)),
            "the clicked row must report a press"
        );
        assert_eq!(second, None, "the other row must not");
    }

    #[test]
    fn shift_and_ctrl_clicks_carry_their_press() {
        for (modifiers, press) in [
            (
                Modifiers {
                    shift: true,
                    ..Modifiers::NONE
                },
                Press::Extend,
            ),
            (
                Modifiers {
                    command: true,
                    ..Modifiers::NONE
                },
                Press::Toggle,
            ),
        ] {
            let ctx = context();
            let row = row(0, "A Title", "A Band");
            run(&ctx, input(Vec::new()), |ui| {
                let _ = show(ui, PROBE, &row, &Tokens::DARK);
            });

            let mut answer = None;
            let mut events = vec![Event::ModifiersChanged(modifiers)];
            events.extend(crate::view::test_support::click_events(PROBE.center()));
            for event in events {
                let answer = &mut answer;
                run(&ctx, input(vec![event]), |ui| {
                    if let Some(click) = show(ui, PROBE, &row, &Tokens::DARK) {
                        *answer = Some(click);
                    }
                });
            }
            assert_eq!(answer, Some(Click::Select(press)), "{modifiers:?}");
        }
    }

    #[test]
    fn enter_on_a_focused_row_plays_and_does_not_select() {
        let ctx = context();
        let row = row(0, "A Title", "A Band");
        // Warm up the focus traversal, then put the row in reach by Tab.
        run(&ctx, input(Vec::new()), |ui| {
            let _ = show(ui, PROBE, &row, &Tokens::DARK);
        });
        let mut expected = None;
        let mut focused = None;
        let [press, _] = tab();
        run(&ctx, input(vec![press]), |ui| {
            expected = Some(super::id(ui, row.index));
            let _ = show(ui, PROBE, &row, &Tokens::DARK);
            focused = ui.memory(|memory| memory.focused());
        });
        assert_eq!(focused, expected, "the Tab stop must reach the row");

        let answer = answer(
            &ctx,
            &row,
            vec![Event::Key {
                key: egui::Key::Enter,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Modifiers::NONE,
            }],
        );
        assert_eq!(answer, Some(Click::Play), "Enter plays the entry");
    }

    #[test]
    fn a_double_click_plays() {
        let ctx = context();
        let row = row(0, "A Title", "A Band");
        run(&ctx, input(Vec::new()), |ui| {
            let _ = show(ui, PROBE, &row, &Tokens::DARK);
        });
        let mut played = false;
        for _ in 0..2 {
            for event in crate::view::test_support::click_events(PROBE.center()) {
                let played = &mut played;
                run(&ctx, input(vec![event]), |ui| {
                    if show(ui, PROBE, &row, &Tokens::DARK) == Some(Click::Play) {
                        *played = true;
                    }
                });
            }
        }
        assert!(played, "a double click must play the entry");
    }

    #[test]
    fn a_three_point_twitch_between_clicks_does_not_lift_but_seven_points_does() {
        for (travel, lifts) in [(3.0, false), (7.0, true)] {
            let ctx = context();
            let row = row(0, "A Title", "A Band");
            run(&ctx, input(Vec::new()), |ui| {
                let _ = show(ui, PROBE, &row, &Tokens::DARK);
            });

            let mut lifted = false;
            let mut pressed = false;
            for event in drag_events(PROBE.center(), travel) {
                let lifted = &mut lifted;
                let pressed = &mut pressed;
                run(&ctx, input(vec![event]), |ui| {
                    match show(ui, PROBE, &row, &Tokens::DARK) {
                        Some(Click::Lift) => *lifted = true,
                        Some(Click::Select(_)) => *pressed = true,
                        _ => {}
                    }
                });
            }
            assert_eq!(lifted, lifts, "a {travel}-point move");
            assert_eq!(
                pressed, !lifts,
                "a {travel}-point move is a click only under the threshold"
            );
        }
    }

    #[test]
    fn a_lifted_row_reports_a_lift_once() {
        let ctx = context();
        let row = row(0, "A Title", "A Band");
        run(&ctx, input(Vec::new()), |ui| {
            let _ = show(ui, PROBE, &row, &Tokens::DARK);
        });

        let start = PROBE.center();
        let end = start + Vec2::new(10.0, 0.0);
        let mut lifts = 0;
        for events in [
            vec![Event::PointerMoved(start)],
            vec![crate::view::test_support::button(start, true)],
            vec![Event::PointerMoved(end)],
            vec![Event::PointerMoved(end + Vec2::new(2.0, 0.0))],
            vec![Event::PointerMoved(end + Vec2::new(4.0, 0.0))],
        ] {
            let lifts = &mut lifts;
            run(&ctx, input(events), |ui| {
                if show(ui, PROBE, &row, &Tokens::DARK) == Some(Click::Lift) {
                    *lifts += 1;
                }
            });
        }
        assert_eq!(lifts, 1, "a held drag lifts once, not every frame");
    }

    #[test]
    fn a_hovered_row_washes_with_row_hover() {
        let ctx = context();
        let tokens = Tokens::DARK;
        let row = row(0, "A Title", "A Band");
        let mut washed = false;
        for _ in 0..30 {
            let mut output = ctx.run_ui(input(vec![Event::PointerMoved(PROBE.center())]), |ui| {
                let _ = show(ui, PROBE, &row, &tokens);
            });
            output.textures_delta.clear();
            washed |= output.shapes.iter().any(|clipped| {
                matches!(&clipped.shape, Shape::Rect(shape) if shape.fill == tokens.palette.row_hover)
            });
        }
        assert!(washed, "a hovered row must paint the row hover wash");
    }

    #[test]
    fn the_row_does_not_reveal_or_fade_while_carrying() {
        let ctx = context();
        let tokens = Tokens::DARK;
        let row = Row {
            look: Look {
                carrying: true,
                ..Look::default()
            },
            ..row(0, "A Title", "A Band")
        };
        let mut washed = false;
        for _ in 0..30 {
            let mut output = ctx.run_ui(input(vec![Event::PointerMoved(PROBE.center())]), |ui| {
                let _ = show(ui, PROBE, &row, &tokens);
            });
            output.textures_delta.clear();
            washed |= output.shapes.iter().any(|clipped| {
                matches!(&clipped.shape, Shape::Rect(shape) if shape.fill == tokens.palette.row_hover)
            });
        }
        assert!(!washed, "nothing hovers while a block is carried");
    }

    #[test]
    fn the_lifted_row_paints_the_count_and_the_blocks_time() {
        let ctx = context();
        let tokens = Tokens::DARK;
        let row = Row {
            duration: Some("3:42"),
            ..row(0, "A Title", "A Band")
        };
        let lifted = row.lifted();
        let mut output = ctx.run_ui(input(Vec::new()), |ui| {
            lifted.paint(
                ui,
                PROBE,
                Ghosting {
                    count: 3,
                    total: Some("11:06"),
                    flat: false,
                },
                &tokens,
            );
        });
        output.textures_delta.clear();
        let texts = drawn_texts(&output);
        for label in ["A Title", "A Band", "×3", "11:06"] {
            assert!(
                texts.contains(&label),
                "the ghost must paint {label}: {texts:?}"
            );
        }
        assert!(
            !texts.contains(&"3:42"),
            "the block's time stands in for the row's: {texts:?}"
        );
    }

    #[test]
    fn a_block_with_no_total_paints_no_duration() {
        let ctx = context();
        let tokens = Tokens::DARK;
        let row = Row {
            duration: Some("3:42"),
            ..row(0, "A Title", "A Band")
        };
        let lifted = row.lifted();
        let mut output = ctx.run_ui(input(Vec::new()), |ui| {
            lifted.paint(
                ui,
                PROBE,
                Ghosting {
                    count: 3,
                    total: None,
                    flat: false,
                },
                &tokens,
            );
        });
        output.textures_delta.clear();
        let texts = drawn_texts(&output);

        assert!(
            !texts.contains(&"3:42"),
            "a block with no honest total paints none, not one row's length: {texts:?}"
        );
        assert!(texts.contains(&"×3"), "the block still says how many it is");
    }

    #[test]
    fn a_flat_ghost_paints_its_text_in_the_muted_ink() {
        let ctx = context();
        let tokens = Tokens::DARK;
        let lifted = row(0, "A Title", "A Band").lifted();
        let mut output = ctx.run_ui(input(Vec::new()), |ui| {
            lifted.paint(
                ui,
                PROBE,
                Ghosting {
                    count: 1,
                    total: None,
                    flat: true,
                },
                &tokens,
            );
        });
        output.textures_delta.clear();
        let title = output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                Shape::Text(text) if text.galley.text() == "A Title" => Some(text.fallback_color),
                _ => None,
            })
            .expect("the title is painted");
        assert_eq!(title, tokens.palette.text_subtle);
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
        assert!(first.is_some());
        assert_eq!(second, None);
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
