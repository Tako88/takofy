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
//!
//! The name and the artist are links: a click on the words opens the page they
//! name and never plays, while a click beside them is the row's own. A carried
//! row and a ghost keep the words as plain text, because neither is a place to
//! navigate from.

use egui::{FontId, Id, Pos2, Rect, Ui, WidgetInfo};

use crate::cover::Cover;
use crate::layout::queue::row_parts;
use crate::route::Page;
use crate::theme::{Tokens, radius, space, text, tracking};

use super::link::{self, Face, Target};
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
    /// The album URI the name links to, when one is known.
    pub title_target: Option<&'a str>,
    /// The first artist's URI the artist links to, when one is known.
    pub artist_target: Option<&'a str>,
    /// Where the row stands in the queue's selection, and in a carry.
    pub look: Look,
    /// Whether this row is the one a just-finished edit landed on.
    pub refocus: bool,
}

/// What a row's two links reported.
#[derive(Debug, Default)]
struct Links {
    /// Whether the pointer is on either line's words.
    hovered: bool,
    /// The page a click on either line opened.
    page: Option<Page>,
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
    let links = paint(ui, rect, row, None, tokens);
    ui.set_opacity(before);

    // The row's own keyboard activation is answered before the pointer's, so a
    // click landing in the same frame can never swallow it.
    let keyboard = response.has_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
    if keyboard {
        return Some(Click::Play);
    }
    if let Some(page) = links.page {
        return Some(Click::Open(page));
    }
    // When a link wins the hit test the row's `double_clicked()` is already
    // false, because egui tells only the hit widget; the guard keeps a second
    // click on the words a navigation rather than a play whatever the hit test
    // does.
    if response.double_clicked() && !links.hovered {
        return Some(Click::Play);
    }
    if body.lift {
        return Some(Click::Lift);
    }
    if links.hovered {
        return None;
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
            title_target: None,
            artist_target: None,
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
fn paint(
    ui: &Ui,
    rect: Rect,
    row: &Row<'_>,
    ghosting: Option<Ghosting<'_>>,
    tokens: &Tokens,
) -> Links {
    let flat = ghosting.is_some_and(|ghost| ghost.flat);
    // A ghost and a row that a carry is holding are drawn, not navigated from.
    let live = ghosting.is_none() && !row.look.carrying;
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

    let links = if parts.text.width() > 0.0 {
        stacked(ui, parts.text, row, count, flat, live, tokens)
    } else {
        Links::default()
    };
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
    links
}

/// The name over the artist, centred together as one block, with the count a
/// carried block stands for beside the name.
///
/// `live` is whether the two lines are links; a ghost and a carried row paint
/// them as plain text.
fn stacked(
    ui: &Ui,
    rect: Rect,
    row: &Row<'_>,
    count: Option<usize>,
    flat: bool,
    live: bool,
    tokens: &Tokens,
) -> Links {
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

    let placed = link::Placed {
        pos: Pos2::new(rect.left(), top),
        galley: link::lay(
            ui,
            row.title,
            &Face {
                font: name,
                tracking: 0.0,
            },
            width,
        ),
    };
    let title_right = placed.rect().right();
    let shown = placed.link(
        ui,
        ui.scope_id().with(("queue-title", row.index)),
        rect,
        row.title_target.filter(|_| live).map(Target::Album),
        title_ink,
        tokens,
    );
    let mut links = Links {
        hovered: shown.hovered,
        page: shown.page,
    };
    if let Some(suffix) = suffix {
        ui.painter().galley(
            Pos2::new(title_right + space::S2, top),
            suffix,
            secondary_ink,
        );
    }
    if artist_h > 0.0 {
        let shown = link::Placed {
            pos: Pos2::new(rect.left(), top + name_h),
            galley: link::lay(
                ui,
                row.artist,
                &Face {
                    font: artist,
                    tracking: 0.0,
                },
                rect.width(),
            ),
        }
        .link(
            ui,
            ui.scope_id().with(("queue-artist", row.index)),
            rect,
            row.artist_target.filter(|_| live).map(Target::Credit),
            secondary_ink,
            tokens,
        );
        links.hovered |= shown.hovered;
        links.page = links.page.or(shown.page);
    }
    links
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

    const ALBUM_URI: &str = "spotify:album:a";
    const ARTIST_URI: &str = "spotify:artist:b";

    /// A row whose name and artist each name a page.
    fn linked<'a>(index: usize, title: &'a str, artist: &'a str) -> Row<'a> {
        Row {
            title_target: Some(ALBUM_URI),
            artist_target: Some(ARTIST_URI),
            ..row(index, title, artist)
        }
    }

    /// Draws one row for a single pass, returning what it answered and the frame.
    fn draw(ctx: &Context, row: &Row<'_>, events: Vec<Event>) -> (Option<Click>, egui::FullOutput) {
        let tokens = Tokens::DARK;
        let mut answer = None;
        let mut output = ctx.run_ui(input(events), |ui| {
            answer = show(ui, PROBE, row, &tokens);
        });
        output.textures_delta.clear();
        (answer, output)
    }

    /// The box the words of `label` were painted in.
    fn words(output: &egui::FullOutput, label: &str) -> Rect {
        output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                Shape::Text(text) if text.galley.text() == label => {
                    Some(Rect::from_min_size(text.pos, text.galley.size()))
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("{label} is painted"))
    }

    /// Every label a pass gave a link node.
    fn link_labels(output: &egui::FullOutput) -> Vec<String> {
        output
            .platform_output
            .accesskit_update
            .as_ref()
            .map(|update| {
                update
                    .nodes
                    .iter()
                    .filter_map(|(_, node)| {
                        (node.role() == egui::accesskit::Role::Link)
                            .then(|| node.label().map(str::to_owned))
                            .flatten()
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Feeds `events` one pass each after a warm-up, returning what the row
    /// answered in every pass.
    fn feed(ctx: &Context, row: &Row<'_>, events: Vec<Event>) -> Vec<Option<Click>> {
        let _ = draw(ctx, row, Vec::new());
        let mut answers = Vec::new();
        for event in events {
            let (answer, _) = draw(ctx, row, vec![event]);
            answers.push(answer);
        }
        answers
    }

    fn row<'a>(index: usize, title: &'a str, artist: &'a str) -> Row<'a> {
        Row {
            index,
            title,
            artist,
            duration: None,
            cover: Cover::Missing,
            title_target: None,
            artist_target: None,
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

    #[test]
    fn a_click_on_the_title_opens_the_album_and_does_not_select() {
        let ctx = context();
        let row = linked(0, "A Title", "A Band");
        let (_, frame) = draw(&ctx, &row, Vec::new());
        let at = words(&frame, "A Title").center();
        let answers = feed(
            &ctx,
            &row,
            crate::view::test_support::click_events(at).to_vec(),
        );
        assert_eq!(
            answers.iter().flatten().last(),
            Some(&Click::Open(Page::Album(ALBUM_URI.to_owned()))),
            "a click on the name must open the album: {answers:?}"
        );
        assert!(
            !answers
                .iter()
                .flatten()
                .any(|click| matches!(click, Click::Select(_) | Click::Play)),
            "the name is a link, not the row: {answers:?}"
        );
    }

    #[test]
    fn a_click_on_the_artist_opens_the_artist() {
        let ctx = context();
        let row = linked(0, "A Title", "A Band");
        let (_, frame) = draw(&ctx, &row, Vec::new());
        let at = words(&frame, "A Band").center();
        let answers = feed(
            &ctx,
            &row,
            crate::view::test_support::click_events(at).to_vec(),
        );
        assert_eq!(
            answers.iter().flatten().last(),
            Some(&Click::Open(Page::Artist(ARTIST_URI.to_owned()))),
            "a click on the artist must open the artist: {answers:?}"
        );
        assert!(
            !answers
                .iter()
                .flatten()
                .any(|click| matches!(click, Click::Select(_) | Click::Play)),
            "the artist is a link, not the row: {answers:?}"
        );
    }

    #[test]
    fn a_double_click_on_a_link_does_not_play() {
        let ctx = context();
        let row = linked(0, "A Title", "A Band");
        let (_, frame) = draw(&ctx, &row, Vec::new());
        let at = words(&frame, "A Title").center();
        let mut events = Vec::new();
        for _ in 0..2 {
            events.extend(crate::view::test_support::click_events(at));
        }
        let answers = feed(&ctx, &row, events);
        assert!(
            answers
                .iter()
                .flatten()
                .any(|click| matches!(click, Click::Open(_))),
            "the words never opened their page: {answers:?}"
        );
        assert!(
            !answers.iter().flatten().any(|click| *click == Click::Play),
            "a double click on the words played the entry: {answers:?}"
        );
    }

    #[test]
    fn a_click_beside_the_words_still_selects_and_a_double_click_there_plays() {
        let ctx = context();
        let row = linked(0, "A Title", "A Band");
        let (_, frame) = draw(&ctx, &row, Vec::new());
        let title = words(&frame, "A Title");
        let beside = Pos2::new(title.right() + 20.0, title.center().y);
        assert!(
            beside.x < PROBE.right(),
            "the probe must stay inside the row: {beside:?}"
        );

        let single = feed(
            &ctx,
            &row,
            crate::view::test_support::click_events(beside).to_vec(),
        );
        assert_eq!(
            single.iter().flatten().last(),
            Some(&Click::Select(Press::Replace)),
            "a click beside the words must pick the row: {single:?}"
        );

        let mut events = Vec::new();
        for _ in 0..2 {
            events.extend(crate::view::test_support::click_events(beside));
        }
        let double = feed(&ctx, &row, events);
        assert!(
            double.iter().flatten().any(|click| *click == Click::Play),
            "a double click beside the words must play: {double:?}"
        );
    }

    #[test]
    fn a_drag_started_on_a_link_still_lifts_the_row() {
        let ctx = context();
        let row = linked(0, "A Title", "A Band");
        let (_, frame) = draw(&ctx, &row, Vec::new());
        let at = words(&frame, "A Title").center();
        let answers = feed(&ctx, &row, drag_events(at, 20.0));
        assert!(
            answers.iter().flatten().any(|click| *click == Click::Lift),
            "a drag begun on the words must lift the row: {answers:?}"
        );
    }

    #[test]
    fn a_carried_row_has_no_links() {
        let ctx = context();
        ctx.enable_accesskit();
        let row = Row {
            look: Look {
                carrying: true,
                ..Look::default()
            },
            ..linked(0, "A Title", "A Band")
        };
        let (_, frame) = draw(&ctx, &row, Vec::new());
        assert!(
            link_labels(&frame).is_empty(),
            "a carried row drew a link node: {:?}",
            link_labels(&frame)
        );
        let at = words(&frame, "A Title").center();
        let answers = feed(
            &ctx,
            &row,
            crate::view::test_support::click_events(at).to_vec(),
        );
        assert!(
            !answers
                .iter()
                .flatten()
                .any(|click| matches!(click, Click::Open(_))),
            "a carried row opened a page: {answers:?}"
        );
    }

    #[test]
    fn a_row_without_uris_is_plain() {
        let ctx = context();
        ctx.enable_accesskit();
        let row = row(0, "A Title", "A Band");
        let (_, frame) = draw(&ctx, &row, Vec::new());
        assert!(
            link_labels(&frame).is_empty(),
            "a name with no page drew a link node: {:?}",
            link_labels(&frame)
        );
        let at = words(&frame, "A Title").center();
        let answers = feed(
            &ctx,
            &row,
            crate::view::test_support::click_events(at).to_vec(),
        );
        assert_eq!(
            answers.iter().flatten().last(),
            Some(&Click::Select(Press::Replace)),
            "plain words are the row's own click: {answers:?}"
        );
    }

    #[test]
    fn a_ghost_paints_no_link() {
        let ctx = context();
        ctx.enable_accesskit();
        let tokens = Tokens::DARK;
        let lifted = linked(0, "A Title", "A Band").lifted();
        let mut output = ctx.run_ui(input(vec![Event::PointerMoved(PROBE.center())]), |ui| {
            lifted.paint(
                ui,
                PROBE,
                Ghosting {
                    count: 1,
                    total: None,
                    flat: false,
                },
                &tokens,
            );
        });
        output.textures_delta.clear();
        assert!(
            link_labels(&output).is_empty(),
            "a ghost drew a link node: {:?}",
            link_labels(&output)
        );
    }
}
