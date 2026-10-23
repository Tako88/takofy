//! The row of actions under a band: play, the context's name once the row pins,
//! and the tools that act on the whole context.
//!
//! The row is one component for every page that heads a list with a band. It
//! holds its resting height in the page's flow and pins to the top of the page's
//! scroll viewport, where it condenses, fills with the page's surface and wears a
//! hairline under it so the rows sliding beneath have an edge to stop at.
//!
//! The fill is opaque rather than the reference's 86% with a backdrop blur: egui
//! has no backdrop filter, and the row is drawn before the rows it would veil, so
//! a translucent fill would show the page behind the row rather than the rows'
//! ink. The rule carries the edge instead.

use egui::{FontId, Pos2, Rect, Sense, Ui, vec2};

use crate::availability::Control;
use crate::icon::Icon;
use crate::layout;
use crate::menu::Door;
use crate::theme::icon::Size;
use crate::theme::{Tokens, Weight, actionbar, fonts, stroke, text, tracking};

use super::{context_menu, controls, pin, text as recipe};

/// What the row acts on, and what it is called.
pub struct Bar<'a> {
    /// The context's name, painted beside the play tile only while the row pins.
    /// `None` leaves the room to the play tile and the tools.
    pub title: Option<&'a str>,
    /// Whether there is anything to play.
    pub playable: bool,
    /// Whether the reference gives this context a shuffle.
    pub shuffle: bool,
    /// Whether the "…" can act. `None` keeps it present and inert.
    pub more: Option<bool>,
    /// Whether this context is in the library, or `None` while no list can say.
    pub saved: Option<bool>,
}

/// What drawing the row reported.
pub struct Reply {
    pub played: bool,
    pub shuffled: bool,
    pub more_door: Option<Door>,
    /// The value the heart asks for, when it was pressed.
    pub saved: Option<bool>,
}

impl Reply {
    fn quiet() -> Reply {
        Reply {
            played: false,
            shuffled: false,
            more_door: None,
            saved: None,
        }
    }
}

/// The row's place in the page's flow.
///
/// It is the resting height whatever the row is doing: the page's scroll extent,
/// the scrollbar and the culling of the rows under it never change because the
/// row pinned.
pub(super) fn row_rect(ui: &mut Ui) -> Rect {
    ui.allocate_exact_size(
        vec2(ui.available_width(), actionbar::REST_H),
        Sense::hover(),
    )
    .0
}

/// Draws the row and reports what its controls did.
pub(super) fn show(ui: &mut Ui, bar: &Bar<'_>, tokens: &Tokens) -> Reply {
    let flow = row_rect(ui);
    let clip_top = ui.clip_rect().top();
    let (rect, stuck) = layout::actionbar::pin(flow, clip_top);
    if stuck {
        // Opaque, and painted before the widgets that land on it: the rows under
        // the row are drawn after it, so anything the row leaves empty would show
        // the ink of the rows sliding beneath.
        ui.painter().rect_filled(rect, 0.0, tokens.palette.surface);
        // The row is holding the rows under it, so it wears an edge to say so.
        // Inside the row's own height, which is the number anything sticky under
        // it clears.
        ui.painter().rect_filled(
            Rect::from_min_size(
                Pos2::new(rect.left(), rect.bottom() - stroke::HAIRLINE),
                vec2(rect.width(), stroke::HAIRLINE),
            ),
            0.0,
            tokens.palette.border,
        );
    }

    let placed = layout::actionbar::solve(rect, bar.shuffle);
    if stuck && let Some(title) = bar.title {
        paint_title(ui, rect, &placed, title, tokens);
    }

    let mut reply = Reply::quiet();
    // The name is part of the row only while it pins; at rest the play tile's
    // label still has to name the context, and that allocation is the only one.
    let named = bar.title.map(|title| format!("Play {title}"));
    let label = named.as_deref().unwrap_or("Play");
    let slot = controls::play_tile(
        ui,
        tokens,
        Control::HeroPlay,
        placed.play,
        label,
        bar.playable,
    );
    reply.played = slot.response.clicked();

    // The tools are drawn in the reference's DOM order, which is left to right
    // inside the row: shuffle, then the heart, then the menu.
    if let Some(rect) = placed.shuffle {
        let slot = controls::icon_button(
            ui,
            tokens,
            Control::HeroShuffle,
            rect,
            Icon::Shuffle,
            Size::MD,
            "Shuffle",
            true,
        );
        reply.shuffled = slot.response.clicked();
    }

    let heart = heart(bar.saved);
    let slot = if heart.lit {
        controls::icon_button_on(
            ui,
            tokens,
            Control::HeroLike,
            (),
            placed.heart,
            Icon::Heart,
            Size::MD,
            heart.label,
            heart.live,
        )
    } else {
        controls::icon_button(
            ui,
            tokens,
            Control::HeroLike,
            placed.heart,
            Icon::Heart,
            Size::MD,
            heart.label,
            heart.live,
        )
    };
    if slot.response.clicked() {
        reply.saved = bar.saved.map(|saved| !saved);
    }

    let live = bar.more.unwrap_or(false);
    let slot = controls::icon_button(
        ui,
        tokens,
        Control::HeroMore,
        placed.more,
        Icon::More,
        Size::MD,
        "More options",
        live,
    );
    reply.more_door = context_menu::button_door(ui, &slot, placed.more, slot.response.id);

    // The row says how far it reaches below the viewport's top, so a row that has
    // to be brought into view is asked for clear of it, and shrinks the clip so
    // nothing under it can be painted or hit there. Both are last: every widget
    // of the row is created before the clip moves under it.
    pin::set(ui.ctx(), if stuck { rect.bottom() - clip_top } else { 0.0 });
    if stuck {
        let clip = ui.clip_rect();
        ui.shrink_clip_rect(Rect::from_min_max(
            Pos2::new(clip.left(), rect.bottom()),
            clip.max,
        ));
    }
    reply
}

/// The context's name, at the row's leading edge after the play tile.
fn paint_title(
    ui: &Ui,
    bar: Rect,
    placed: &layout::actionbar::Placed,
    title: &str,
    tokens: &Tokens,
) {
    let Some(label_box) = layout::actionbar::title(bar, placed) else {
        return;
    };
    let line = recipe::line(
        ui,
        title,
        FontId::new(text::MD, fonts::sans(Weight::Bold)),
        tokens.palette.text,
        recipe::tracking_pts(text::MD, tracking::TIGHT),
        label_box.width(),
    );
    ui.painter().galley(
        Pos2::new(label_box.left(), label_box.center().y - line.size().y / 2.0),
        line,
        tokens.palette.text,
    );
}

/// How the row's heart reads, given what the library says about the context.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Heart {
    label: &'static str,
    /// Whether it can act: no list could say, so nothing is offered.
    live: bool,
    /// Whether the glyph is the filled, saved one.
    lit: bool,
}

/// The heart's three answers, in one place so the label and the glyph cannot
/// disagree about whether the context is in the library.
fn heart(saved: Option<bool>) -> Heart {
    match saved {
        Some(true) => Heart {
            label: "Remove from your library",
            live: true,
            lit: true,
        },
        Some(false) => Heart {
            label: "Save to your library",
            live: true,
            lit: false,
        },
        None => Heart {
            label: "Save to your library",
            live: false,
            lit: false,
        },
    }
}

#[cfg(test)]
mod tests {
    use egui::{Context, Event, Pos2, RawInput, Rect, Vec2};

    use super::*;
    use crate::theme::{Tokens, fonts, hero, style};
    use crate::view::test_support::click_events;

    const VIEWPORT: Vec2 = Vec2::new(900.0, 400.0);
    /// How much list there is above the row, so scrolling can pin it.
    const FILLER: f32 = 200.0;

    fn context() -> Context {
        let tokens = Tokens::DARK;
        let ctx = Context::default();
        ctx.set_fonts(fonts::definitions());
        ctx.set_global_style(style(&tokens));
        ctx
    }

    fn input(events: Vec<Event>) -> RawInput {
        RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, VIEWPORT)),
            events,
            ..Default::default()
        }
    }

    fn bar() -> Bar<'static> {
        Bar {
            title: Some("Late Night Drive"),
            playable: true,
            shuffle: true,
            more: Some(true),
            saved: Some(true),
        }
    }

    /// One frame of the row.
    struct Frame {
        reply: Reply,
        output: egui::FullOutput,
        /// The slot the row holds in the page's flow.
        flow: Rect,
    }

    // The row draws nothing else, so a probe that reads its place in the flow is
    // the layout's own answer rather than a re-derivation of the row's height.
    fn frame(ctx: &Context, offset: f32, events: Vec<Event>) -> Frame {
        let tokens = Tokens::DARK;
        let bar = bar();
        let mut reply = Reply::quiet();
        let mut flow = Rect::ZERO;
        let mut output = ctx.run_ui(input(events), |ui| {
            egui::ScrollArea::vertical()
                .id_salt("bar")
                .vertical_scroll_offset(offset)
                .show(ui, |ui| {
                    // The probe reads the slot off the cursor, so the gap the
                    // layout puts between two widgets would be counted as part
                    // of the row. The row places its own children absolutely and
                    // never reads it.
                    ui.spacing_mut().item_spacing.y = 0.0;
                    ui.add_space(FILLER);
                    let top = ui.cursor().min;
                    reply = show(ui, &bar, &tokens);
                    let bottom = ui.cursor().min.y;
                    flow =
                        Rect::from_min_size(top, Vec2::new(ui.available_width(), bottom - top.y));
                });
        });
        output.textures_delta.clear();
        Frame {
            reply,
            output,
            flow,
        }
    }

    /// The rects the frame filled with `fill`.
    fn fills(output: &egui::FullOutput, fill: egui::Color32) -> Vec<(usize, Rect)> {
        output
            .shapes
            .iter()
            .enumerate()
            .filter_map(|(at, clipped)| match &clipped.shape {
                egui::epaint::Shape::Rect(shape) if shape.fill == fill => Some((at, shape.rect)),
                _ => None,
            })
            .collect()
    }

    /// The place a text shape holds in the paint order, and its box.
    fn text_at(output: &egui::FullOutput, label: &str) -> Vec<(usize, Rect)> {
        output
            .shapes
            .iter()
            .enumerate()
            .filter_map(|(at, clipped)| match &clipped.shape {
                egui::epaint::Shape::Text(text) if text.galley.text() == label => {
                    Some((at, Rect::from_min_size(text.pos, text.galley.size())))
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_flow_slot_is_rest_height_stuck_or_not() {
        let ctx = context();
        let rested = frame(&ctx, 0.0, Vec::new());
        assert_eq!(
            rested.flow.height(),
            actionbar::REST_H,
            "the row holds its resting height in the flow: {:?}",
            rested.flow
        );
        assert_eq!(rested.flow.top(), FILLER);

        // Scrolled so the row's slot has gone past the top of the viewport.
        let pinned = frame(&ctx, FILLER + 50.0, Vec::new());
        assert!(pinned.flow.top() < 0.0, "the slot is above the clip");
        assert_eq!(
            pinned.flow.height(),
            actionbar::REST_H,
            "the slot keeps its resting height while the row is pinned"
        );
    }

    #[test]
    fn pinned_it_is_a_surface_fill_of_stuck_height_with_a_bottom_hairline() {
        let ctx = context();
        let tokens = Tokens::DARK;
        let pinned = frame(&ctx, FILLER + 50.0, Vec::new());

        let bands = fills(&pinned.output, tokens.palette.surface);
        let (_, band) = bands
            .iter()
            .find(|(_, rect)| (rect.top() - pinned.flow.top().max(0.0)).abs() < 0.01)
            .map(|(at, rect)| (*at, *rect))
            .unwrap_or_else(|| panic!("the pinned row paints no fill: {bands:?}"));
        assert!(
            (band.top() - 0.0).abs() < 0.01,
            "the row sits at the viewport's top: {band:?}"
        );
        assert_eq!(band.height(), actionbar::STUCK_H);
        assert_eq!(band.width(), pinned.flow.width());

        let line = fills(&pinned.output, tokens.palette.border)
            .into_iter()
            .find(|(_, rect)| {
                (rect.height() - stroke::HAIRLINE).abs() < 0.01
                    && (rect.bottom() - band.bottom()).abs() < 0.01
                    && (rect.width() - band.width()).abs() < 0.01
            })
            .map(|(_, rect)| rect);
        assert!(
            line.is_some(),
            "the pinned row wears a hairline across its bottom: {band:?}"
        );
    }

    #[test]
    fn at_rest_it_paints_no_fill_and_no_edge() {
        let ctx = context();
        let tokens = Tokens::DARK;
        let rested = frame(&ctx, 0.0, Vec::new());
        assert!(
            fills(&rested.output, tokens.palette.surface).is_empty(),
            "a row in its place veils nothing: {:?}",
            fills(&rested.output, tokens.palette.surface)
        );
        let edges: Vec<Rect> = fills(&rested.output, tokens.palette.border)
            .into_iter()
            .filter(|(_, rect)| (rect.bottom() - rested.flow.bottom()).abs() < 0.01)
            .map(|(_, rect)| rect)
            .collect();
        assert!(
            edges.is_empty(),
            "a row in its place wears no stuck edge: {edges:?}"
        );
        assert!(
            text_at(&rested.output, "Late Night Drive").is_empty(),
            "a row in its place paints no name: {:?}",
            text_at(&rested.output, "Late Night Drive")
        );
    }

    #[test]
    fn the_fill_is_painted_before_the_widgets() {
        let ctx = context();
        let tokens = Tokens::DARK;
        let pinned = frame(&ctx, FILLER + 50.0, Vec::new());
        let band_at = fills(&pinned.output, tokens.palette.surface)
            .first()
            .map(|(at, _)| *at)
            .expect("the pinned row fills its box");
        let play_at = fills(&pinned.output, tokens.palette.accent)
            .first()
            .map(|(at, _)| *at)
            .expect("the row paints its play tile");
        let title_at = text_at(&pinned.output, "Late Night Drive")
            .first()
            .map(|(at, _)| *at)
            .expect("the pinned row paints the context's name");
        assert!(band_at < play_at, "the fill is under the play tile");
        assert!(band_at < title_at, "the fill is under the name");
    }

    #[test]
    fn the_title_is_a_label_and_a_rename_never_moves_into_it() {
        let ctx = context();
        let offset = FILLER + 50.0;
        let pinned = frame(&ctx, offset, Vec::new());
        let (_, label) = text_at(&pinned.output, "Late Night Drive")
            .first()
            .copied()
            .expect("the pinned row paints the context's name");

        // A press on the name is a press on nothing: the row's name is a label,
        // so a rename in flight stays in the band's own field below it.
        let mut last = Reply::quiet();
        for event in click_events(label.center()) {
            last = frame(&ctx, offset, vec![event]).reply;
        }
        assert!(!last.played && !last.shuffled && last.saved.is_none() && last.more_door.is_none());
        assert_eq!(
            ctx.memory(|memory| memory.focused()),
            None,
            "the name takes no keyboard"
        );
    }

    #[test]
    fn the_bar_is_drawn_once_pinned() {
        let ctx = context();
        let tokens = Tokens::DARK;
        let pinned = frame(&ctx, FILLER + 50.0, Vec::new());
        assert_eq!(
            fills(&pinned.output, tokens.palette.surface).len(),
            1,
            "one row, one fill"
        );
        assert_eq!(
            text_at(&pinned.output, "Late Night Drive").len(),
            1,
            "one row, one name"
        );
        let tiles = fills(&pinned.output, tokens.palette.accent);
        assert_eq!(tiles.len(), 1, "one row, one play tile");
        assert_eq!(tiles[0].1.width(), hero::PLAY);
    }

    #[test]
    fn play_shuffle_heart_more_report_their_presses() {
        let ctx = context();
        let rested = frame(&ctx, 0.0, Vec::new());
        let placed = layout::actionbar::solve(rested.flow, true);
        let shuffle = placed.shuffle.expect("a playlist's row has a shuffle");

        let at = |target: Pos2| {
            let mut last = Reply::quiet();
            for event in click_events(target) {
                last = frame(&ctx, 0.0, vec![event]).reply;
            }
            last
        };

        assert!(at(placed.play.center()).played, "play reports its press");
        assert!(at(shuffle.center()).shuffled, "shuffle reports its press");
        assert_eq!(
            at(placed.heart.center()).saved,
            Some(false),
            "a saved context's heart asks to unsave it"
        );
        assert!(
            at(placed.more.center()).more_door.is_some(),
            "the menu reports its door"
        );
        assert!(
            !at(rested.flow.center()).played,
            "the row's middle is inert"
        );
    }

    #[test]
    fn the_heart_says_what_it_would_do_and_only_acts_on_a_known_answer() {
        let saved = heart(Some(true));
        assert_eq!(saved.label, "Remove from your library");
        assert!(saved.lit, "a saved context's heart is filled");
        assert!(saved.live);

        let unsaved = heart(Some(false));
        assert_eq!(unsaved.label, "Save to your library");
        assert!(!unsaved.lit);
        assert!(unsaved.live);

        let unknown = heart(None);
        assert!(
            !unknown.live,
            "no list could say, so the heart offers nothing"
        );
        assert!(!unknown.lit);
    }

    #[test]
    fn tab_order_is_play_shuffle_heart_more() {
        let ctx = context();
        let tokens = Tokens::DARK;
        let bar = bar();
        let mut expected = Vec::new();
        let mut output = ctx.run_ui(input(Vec::new()), |ui| {
            egui::ScrollArea::vertical().id_salt("bar").show(ui, |ui| {
                ui.add_space(FILLER);
                let _ = show(ui, &bar, &tokens);
                expected = vec![
                    controls::slot_id(ui, Control::HeroPlay, ()),
                    controls::slot_id(ui, Control::HeroShuffle, ()),
                    controls::slot_id(ui, Control::HeroLike, ()),
                    controls::slot_id(ui, Control::HeroMore, ()),
                ];
            });
        });
        output.textures_delta.clear();

        let [press, release] = crate::view::test_support::tab();
        let mut reached = Vec::new();
        for _ in 0..expected.len() {
            let _ = frame(&ctx, 0.0, vec![press.clone()]);
            reached.push(ctx.memory(|memory| memory.focused()));
            let _ = frame(&ctx, 0.0, vec![release.clone()]);
        }
        let reached: Vec<&str> = reached
            .iter()
            .map(|id| match *id {
                None => "nothing",
                Some(id) if id == expected[0] => "play",
                Some(id) if id == expected[1] => "shuffle",
                Some(id) if id == expected[2] => "heart",
                Some(id) if id == expected[3] => "more",
                Some(_) => "something else",
            })
            .collect();
        assert_eq!(reached, ["play", "shuffle", "heart", "more"]);
    }

    #[test]
    fn rows_never_paint_over_the_pinned_bar() {
        let ctx = context();
        let tokens = Tokens::DARK;
        let bar = bar();
        let mut output = ctx.run_ui(input(Vec::new()), |ui| {
            egui::ScrollArea::vertical()
                .id_salt("bar")
                .vertical_scroll_offset(FILLER + 50.0)
                .show(ui, |ui| {
                    ui.add_space(FILLER);
                    let _ = show(ui, &bar, &tokens);
                    // A row where the list would start, which the pinned row now
                    // covers.
                    ui.painter().text(
                        Pos2::new(0.0, 4.0),
                        egui::Align2::LEFT_TOP,
                        "a row",
                        FontId::default(),
                        tokens.palette.text,
                    );
                });
        });
        output.textures_delta.clear();
        let (_, painted) = text_at(&output, "a row")
            .first()
            .copied()
            .expect("the row is painted");
        let clipped = output
            .shapes
            .iter()
            .find(|clipped| {
                matches!(&clipped.shape, egui::epaint::Shape::Text(text) if text.galley.text() == "a row")
            })
            .map(|clipped| clipped.clip_rect)
            .expect("the row is painted");
        assert!(
            clipped.top() >= actionbar::STUCK_H - 0.01,
            "a row under the pinned row is clipped to its bottom: {clipped:?} for {painted:?}"
        );
    }

    #[test]
    fn a_press_in_the_pinned_strip_reaches_no_row() {
        let ctx = context();
        let tokens = Tokens::DARK;
        let bar = bar();
        let id = egui::Id::unique("row-under-the-bar");
        let at = Pos2::new(400.0, 20.0);
        let mut clicked = false;
        for events in [
            vec![Event::PointerMoved(at)],
            vec![crate::view::test_support::button(at, true)],
            vec![crate::view::test_support::button(at, false)],
        ] {
            let mut output = ctx.run_ui(input(events), |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("bar")
                    .vertical_scroll_offset(FILLER + 50.0)
                    .show(ui, |ui| {
                        ui.add_space(FILLER);
                        let _ = show(ui, &bar, &tokens);
                        let body = ui.interact(
                            Rect::from_min_size(
                                Pos2::ZERO,
                                Vec2::new(VIEWPORT.x, actionbar::STUCK_H),
                            ),
                            id,
                            Sense::click(),
                        );
                        clicked |= body.clicked();
                    });
            });
            output.textures_delta.clear();
        }
        assert!(!clicked, "a press inside the pinned strip reached a row");
    }
}
