//! The row of actions under a band: play, the page's filter field, the context's
//! name once the row pins, and the tools that act on the whole context.
//!
//! The row is one component for every page that heads a list with a band. It
//! holds its resting height in the page's flow and pins to the top of the page's
//! scroll viewport, where it condenses, fills with the page's surface and wears a
//! hairline under it so the rows sliding beneath have an edge to stop at.
//!
//! A page whose one act on the whole context is a word rather than a saved state
//! puts that word in the heart's place, and its failure in the name's box: the
//! artist page follows, the reference gives it no like, and a failed write has
//! nowhere else on the fixed-height row to be said.
//!
//! The fill is opaque rather than the reference's 86% with a backdrop blur: egui
//! has no backdrop filter, and the row is drawn before the rows it would veil, so
//! a translucent fill would show the page behind the row rather than the rows'
//! ink. The rule carries the edge instead.
//!
//! The filter field is part of the row because the row is the only thing on the
//! page that stays still: a field left in the flow scrolls away exactly when the
//! list under it has grown long enough to need one.

use egui::{FontId, Pos2, Rect, Sense, Ui, vec2};

use crate::availability::Control;
use crate::format::filter_count_label;
use crate::icon::Icon;
use crate::layout;
use crate::layout::actionbar::Kind;
use crate::menu::Door;
use crate::message::Failure;
use crate::theme::icon::Size;
use crate::theme::{Tokens, Weight, actionbar, fonts, stroke, text, tracking};

use super::filter_field::Rule;
use super::{context_menu, controls, filter_count, pin, playlist_filter, text as recipe};

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
    /// Which page the row heads, and what its row leaves room for.
    pub kind: Kind,
    /// The page's own text control, in the heart's place.
    ///
    /// A page whose only act on the whole context is a word offers it here: the
    /// artist page follows rather than saving, and the two controls answer the
    /// same question, so the row carries one or the other.
    pub text: Option<Text<'a>>,
}

/// A text control the row carries where the heart would be.
#[derive(Clone, Copy, Debug)]
pub struct Text<'a> {
    /// The word the control draws: `Follow` or `Following`.
    pub label: &'a str,
    /// Whether it can act right now.
    pub live: bool,
    /// Why the last press failed. The message takes the name's box while it
    /// stands, because the row has no other line to put it on.
    pub failed: Option<Failure>,
}

/// What drawing the row reported.
pub struct Reply {
    pub played: bool,
    pub shuffled: bool,
    pub more_door: Option<Door>,
    /// The value the heart asks for, when it was pressed.
    pub saved: Option<bool>,
    /// Whether the page's own text control, if it has one, was pressed.
    pub followed: bool,
    /// The true rows the field's query leaves, when the field was drawn and the
    /// query is not empty. `None` means the whole list, including when the row
    /// had no room for a field: a query nobody can see never narrows the list.
    pub hits: Option<Vec<usize>>,
}

impl Reply {
    fn quiet() -> Reply {
        Reply {
            played: false,
            shuffled: false,
            more_door: None,
            saved: None,
            followed: false,
            hits: None,
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
pub(super) fn show(
    ui: &mut Ui,
    bar: &Bar<'_>,
    mut filter: Option<playlist_filter::Filter<'_>>,
    tokens: &Tokens,
) -> Reply {
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

    let placed = layout::actionbar::solve(
        rect,
        bar.kind,
        layout::actionbar::Parts {
            shuffle: bar.shuffle,
            field: filter.is_some(),
            follow: bar
                .text
                .map(|text| controls::text_button_size(ui, tokens, text.label)),
        },
    );

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

    // The field is drawn on the row's own canvas and takes the row's own rule:
    // a stuck row already says where it ends, so the field's underline would be a
    // second edge on the same line. It is created before the tools because the
    // reference's DOM order puts it there, and Tab walks the row in that order.
    let rule = if stuck {
        Rule::FocusOnly
    } else {
        Rule::Resting
    };
    if let (Some(filter), Some(field)) = (filter.as_mut(), placed.field) {
        reply.hits = playlist_filter::show(ui, filter, field, rule, tokens);
    }

    // The count follows the field, because the page has to know how many rows
    // the query left before it can say how many there are. Nothing placed before
    // this moves: the field and the tools are anchored, and the name elides.
    let mut count = None;
    if let (Some(hits), Some(filter)) = (&reply.hits, &filter) {
        let total = filter.entries.len();
        let words = filter_count_label(hits.len(), total);
        let label = filter_count::layout(ui, &words, tokens);
        count = layout::actionbar::count(&placed, bar.kind, label.size().x);
        if let Some(rect) = count {
            filter_count::paint(ui, rect, label, tokens);
        }
        filter_count::announce(ui, ui.scope_id().with("track-count"), &words);
    }

    // The page's text control stands between the field and the tools, which is
    // the reference's DOM order and so the row's Tab order too.
    if let (Some(text), Some(rect)) = (bar.text.as_ref(), placed.follow) {
        let slot = controls::text_button(ui, tokens, Control::Follow, rect, text.label, text.live);
        reply.followed = slot.response.clicked();
    }

    // The name is painted only while the row pins, and a failure takes the same
    // box in either state: it is the one thing the page has to say about the
    // control's last press, and the row is fixed at both heights, so a message
    // may not bring a second line. A box too narrow for the name holds no
    // message either.
    let name_box = layout::actionbar::title(rect, &placed, count, bar.kind, filter.is_some());
    if let Some(failure) = bar.text.as_ref().and_then(|text| text.failed) {
        if let Some(label_box) = name_box {
            paint_failure(ui, label_box, failure, tokens);
        } else {
            // A row too narrow for the name's box has nowhere to paint the
            // message, but the press must not become silent: the status node is
            // the one channel left.
            filter_count::announce(
                ui,
                ui.scope_id().with("bar-status"),
                crate::signin::failure_message(failure),
            );
        }
    } else if stuck
        && let Some(title) = bar.title
        && let Some(label_box) = name_box
    {
        paint_title(ui, label_box, title, tokens);
    }

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

    if let Some(rect) = placed.heart {
        let heart = heart(bar.saved);
        let slot = if heart.lit {
            controls::icon_button_on(
                ui,
                tokens,
                Control::HeroLike,
                (),
                rect,
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
                rect,
                Icon::Heart,
                Size::MD,
                heart.label,
                heart.live,
            )
        };
        if slot.response.clicked() {
            reply.saved = bar.saved.map(|saved| !saved);
        }
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
fn paint_title(ui: &Ui, label_box: Rect, title: &str, tokens: &Tokens) {
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

/// The last press's failure, in the row's name box.
///
/// Secondary weight and the muted tone, because it is an aside about a control
/// rather than the context's name.
fn paint_failure(ui: &Ui, label_box: Rect, failure: Failure, tokens: &Tokens) {
    let hint = recipe::line(
        ui,
        crate::signin::failure_message(failure),
        recipe::track_secondary(),
        tokens.palette.text_muted,
        0.0,
        label_box.width(),
    );
    ui.painter().galley(
        Pos2::new(label_box.left(), label_box.center().y - hint.size().y / 2.0),
        hint,
        tokens.palette.text_muted,
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
    use egui::{Context, Event, FontId, Pos2, RawInput, Rect, Vec2};
    use takofy_model::TrackEntry;

    use super::*;
    use crate::layout::Table;
    use crate::layout::actionbar::{Form, Parts, Placed, solve};
    use crate::theme::{Tokens, fonts, hero, style};
    use crate::track_filter::TrackFilter;
    use crate::view::test_support::click_events;

    const VIEWPORT: Vec2 = Vec2::new(900.0, 400.0);
    /// Wide enough that the rail stands aside, which is the only way the count
    /// is drawn at all.
    const WIDE: Vec2 = Vec2::new(1000.0, 400.0);
    /// How much list there is above the row, so scrolling can pin it.
    const FILLER: f32 = 200.0;
    /// The playlist the fixture's query belongs to.
    const PLAYLIST: &str = "spotify:playlist:a";

    fn context() -> Context {
        let tokens = Tokens::DARK;
        let ctx = Context::default();
        ctx.set_fonts(fonts::definitions());
        ctx.set_global_style(style(&tokens));
        ctx
    }

    fn input(size: Vec2, events: Vec<Event>) -> RawInput {
        RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)),
            events,
            ..Default::default()
        }
    }

    /// The kind a playlist page has in a window of `size`.
    fn kind(size: Vec2) -> Kind {
        Kind::Playlist(Form::at(size.x, Table::Full))
    }

    /// The row the tests draw, named and saved unless a test says otherwise.
    fn bar(kind: Kind) -> Bar<'static> {
        Bar {
            title: Some("Late Night Drive"),
            playable: true,
            shuffle: true,
            more: Some(true),
            saved: Some(true),
            kind,
            text: None,
        }
    }

    /// Three rows a query can narrow, and the query state that narrows them.
    struct Rows {
        state: TrackFilter,
        entries: Vec<TrackEntry>,
    }

    impl Rows {
        fn new() -> Rows {
            Rows {
                state: TrackFilter::default(),
                entries: (0..3)
                    .map(|index| TrackEntry {
                        uri: format!("t{index}"),
                        title: format!("t{index}"),
                        artists: if index == 1 {
                            "Midnight Artist".to_owned()
                        } else {
                            "Vela Moreno".to_owned()
                        },
                        album: None,
                        album_uri: None,
                        artist_uris: Vec::new(),
                        artist_names: Vec::new(),
                        added_at: None,
                        duration_ms: None,
                        named: true,
                        cover: None,
                    })
                    .collect(),
            }
        }

        fn narrowed() -> Rows {
            let mut rows = Rows::new();
            rows.state.edit(PLAYLIST).push_str("midnight");
            rows
        }

        fn filter(&mut self) -> playlist_filter::Filter<'_> {
            playlist_filter::Filter {
                state: &mut self.state,
                playlist: PLAYLIST,
                entries: &self.entries,
            }
        }
    }

    /// One frame of the row.
    struct Frame {
        reply: Reply,
        output: egui::FullOutput,
        /// The slot the row holds in the page's flow.
        flow: Rect,
    }

    impl Frame {
        /// The row's drawn box: the fill it paints when it pins, and its place in
        /// the flow when it did not.
        fn rect(&self) -> Rect {
            fills(&self.output, Tokens::DARK.palette.surface)
                .into_iter()
                .find(|(_, rect)| (rect.height() - actionbar::STUCK_H).abs() < 0.01)
                .map_or(self.flow, |(_, rect)| rect)
        }
    }

    fn frame_in(
        ctx: &Context,
        size: Vec2,
        offset: f32,
        events: Vec<Event>,
        filter: Option<playlist_filter::Filter<'_>>,
    ) -> Frame {
        frame_of(ctx, &bar(kind(size)), size, offset, events, filter)
    }

    fn frame(ctx: &Context, offset: f32, events: Vec<Event>) -> Frame {
        frame_in(ctx, VIEWPORT, offset, events, None)
    }

    /// One frame of a row of the caller's own making.
    fn frame_of(
        ctx: &Context,
        bar: &Bar<'_>,
        size: Vec2,
        offset: f32,
        events: Vec<Event>,
        filter: Option<playlist_filter::Filter<'_>>,
    ) -> Frame {
        frame_of_probed(ctx, bar, size, offset, events, filter, |_| ()).0
    }

    /// One frame of a row, plus whatever `probe` reads from the row's own `Ui`
    /// once every widget is drawn.
    fn frame_of_probed<R>(
        ctx: &Context,
        bar: &Bar<'_>,
        size: Vec2,
        offset: f32,
        events: Vec<Event>,
        mut filter: Option<playlist_filter::Filter<'_>>,
        probe: impl FnOnce(&egui::Ui) -> R,
    ) -> (Frame, R) {
        let tokens = Tokens::DARK;
        let mut reply = Reply::quiet();
        let mut flow = Rect::ZERO;
        let mut read = None;
        let mut probe = Some(probe);
        let mut output = ctx.run_ui(input(size, events), |ui| {
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
                    reply = show(ui, bar, filter.take(), &tokens);
                    let bottom = ui.cursor().min.y;
                    flow =
                        Rect::from_min_size(top, Vec2::new(ui.available_width(), bottom - top.y));
                    read = Some(probe.take().expect("the probe reads once")(ui));
                });
        });
        output.textures_delta.clear();
        (
            Frame {
                reply,
                output,
                flow,
            },
            read.expect("the row's frame ran"),
        )
    }

    /// One frame of a row, plus whatever `probe` reads from its `Ui`.
    fn frame_probed<R>(
        ctx: &Context,
        size: Vec2,
        offset: f32,
        events: Vec<Event>,
        filter: Option<playlist_filter::Filter<'_>>,
        probe: impl FnOnce(&egui::Ui) -> R,
    ) -> (Frame, R) {
        frame_of_probed(ctx, &bar(kind(size)), size, offset, events, filter, probe)
    }

    /// The row the artist page draws: a name, the tools, and the word the page
    /// owns in the heart's place.
    fn text_bar(kind: Kind) -> Bar<'static> {
        Bar {
            title: Some("Vela Moreno"),
            playable: true,
            shuffle: true,
            more: Some(true),
            saved: None,
            kind,
            text: Some(Text {
                label: "Follow",
                live: true,
                failed: None,
            }),
        }
    }

    /// The rects the row's controls registered, read back from the frame that
    /// drew them, so a test presses what the page actually laid out.
    struct Drawn {
        play: Rect,
        follow: Option<Rect>,
        shuffle: Option<Rect>,
        heart: Option<Rect>,
    }

    fn drawn(ctx: &Context, bar: &Bar<'_>, size: Vec2, offset: f32) -> Drawn {
        let (_, ids) = frame_of_probed(ctx, bar, size, offset, Vec::new(), None, |ui| {
            [
                Control::HeroPlay,
                Control::Follow,
                Control::HeroShuffle,
                Control::HeroLike,
                Control::HeroMore,
            ]
            .map(|control| (control, controls::slot_id(ui, control, ())))
        });
        let rect = |control: Control| {
            ids.iter()
                .find(|(each, _)| *each == control)
                .and_then(|(_, id)| ctx.read_response(*id))
                .map(|response| response.rect)
        };
        Drawn {
            play: rect(Control::HeroPlay).expect("the row draws its play tile"),
            follow: rect(Control::Follow),
            shuffle: rect(Control::HeroShuffle),
            heart: rect(Control::HeroLike),
        }
    }

    /// One press on a control, event by event, and the reply the last pass made.
    fn press(ctx: &Context, bar: &Bar<'_>, size: Vec2, offset: f32, at: Pos2) -> Reply {
        let mut last = Reply::quiet();
        for event in click_events(at) {
            last = frame_of(ctx, bar, size, offset, vec![event], None).reply;
        }
        last
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

    /// The one box a text shape with `label` was painted in.
    fn text_rect(output: &egui::FullOutput, label: &str) -> Option<Rect> {
        text_at(output, label).first().map(|(_, rect)| *rect)
    }

    /// The field's own bottom hairline, its box and the ink it carries.
    fn field_line(output: &egui::FullOutput) -> Option<(Rect, egui::Color32)> {
        let tokens = Tokens::DARK;
        output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Rect(shape)
                    if (shape.rect.height() - stroke::HAIRLINE).abs() < 0.01
                        && (shape.rect.width() - actionbar::FIELD_W).abs() < 0.01
                        && (shape.fill == tokens.palette.border
                            || shape.fill == tokens.palette.text) =>
                {
                    Some((shape.rect, shape.fill))
                }
                _ => None,
            })
    }

    /// The row's children, solved from the row's own drawn box.
    fn placed(frame: &Frame, size: Vec2) -> Placed {
        solve(
            frame.rect(),
            kind(size),
            Parts {
                shuffle: true,
                field: true,
                follow: None,
            },
        )
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
        let mut rows = Rows::narrowed();
        let pinned = frame_in(
            &ctx,
            VIEWPORT,
            FILLER + 50.0,
            Vec::new(),
            Some(rows.filter()),
        );
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
        let field_at = text_at(&pinned.output, "midnight")
            .first()
            .map(|(at, _)| *at)
            .expect("the field paints its query");
        assert!(band_at < play_at, "the fill is under the play tile");
        assert!(band_at < title_at, "the fill is under the name");
        assert!(band_at < field_at, "the fill is under the field's text");

        // And the placeholder too, on a row with no query in it.
        let mut quiet = Rows::new();
        let pinned = frame_in(
            &ctx,
            VIEWPORT,
            FILLER + 50.0,
            Vec::new(),
            Some(quiet.filter()),
        );
        let band_at = fills(&pinned.output, tokens.palette.surface)
            .first()
            .map(|(at, _)| *at)
            .expect("the pinned row fills its box");
        let placeholder_at = text_at(&pinned.output, "FILTER")
            .first()
            .map(|(at, _)| *at)
            .expect("the field paints its placeholder");
        assert!(
            band_at < placeholder_at,
            "the fill is under the field's placeholder"
        );
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
        let tiles = placed(&rested, VIEWPORT);
        let shuffle = tiles.shuffle.expect("a playlist's row has a shuffle");

        let at = |target: Pos2| {
            let mut last = Reply::quiet();
            for event in click_events(target) {
                last = frame(&ctx, 0.0, vec![event]).reply;
            }
            last
        };

        assert!(at(tiles.play.center()).played, "play reports its press");
        assert!(at(shuffle.center()).shuffled, "shuffle reports its press");
        assert_eq!(
            at(tiles.heart.expect("a wide row keeps its heart").center()).saved,
            Some(false),
            "a saved context's heart asks to unsave it"
        );
        assert!(
            at(tiles.more.center()).more_door.is_some(),
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
        let (_, expected) = frame_probed(&ctx, VIEWPORT, 0.0, Vec::new(), None, |ui| {
            vec![
                controls::slot_id(ui, Control::HeroPlay, ()),
                controls::slot_id(ui, Control::HeroShuffle, ()),
                controls::slot_id(ui, Control::HeroLike, ()),
                controls::slot_id(ui, Control::HeroMore, ()),
            ]
        });

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
    fn the_field_is_drawn_in_the_bar_between_play_and_shuffle_in_tab_order() {
        let ctx = context();
        let mut rows = Rows::narrowed();
        let (frame, expected) =
            frame_probed(&ctx, VIEWPORT, 0.0, Vec::new(), Some(rows.filter()), |ui| {
                vec![
                    controls::slot_id(ui, Control::HeroPlay, ()),
                    ui.scope_id().with("playlist-filter"),
                    controls::slot_id(ui, Control::HeroShuffle, ()),
                    controls::slot_id(ui, Control::HeroLike, ()),
                    controls::slot_id(ui, Control::HeroMore, ()),
                ]
            });

        // It is the row's own field, between the play tile and the tools.
        let tiles = placed(&frame, VIEWPORT);
        let field = tiles.field.expect("a wide row has a field");
        assert_eq!(field.width(), actionbar::FIELD_W);
        assert_eq!(field.height(), crate::theme::extent::BUTTON_LG);
        assert!(field.left() > tiles.play.right());
        assert!(field.right() <= tiles.shuffle.expect("shuffle").left());
        assert_eq!(
            text_at(&frame.output, "midnight").len(),
            1,
            "the row's one field paints the query"
        );
        assert_eq!(
            frame.reply.hits.as_deref(),
            Some(&[1][..]),
            "the drawn query is applied"
        );

        // Tab walks the row in the reference's DOM order: play, field, shuffle,
        // heart, more.
        let [press, release] = crate::view::test_support::tab();
        let mut reached = Vec::new();
        for _ in 0..expected.len() {
            let _ = frame_in(
                &ctx,
                VIEWPORT,
                0.0,
                vec![press.clone()],
                Some(rows.filter()),
            );
            reached.push(ctx.memory(|memory| memory.focused()));
            let _ = frame_in(
                &ctx,
                VIEWPORT,
                0.0,
                vec![release.clone()],
                Some(rows.filter()),
            );
        }
        let reached: Vec<&str> = reached
            .iter()
            .map(|id| match *id {
                None => "nothing",
                Some(id) if id == expected[0] => "play",
                Some(id) if id == expected[1] => "field",
                Some(id) if id == expected[2] => "shuffle",
                Some(id) if id == expected[3] => "heart",
                Some(id) if id == expected[4] => "more",
                Some(_) => "something else",
            })
            .collect();
        assert_eq!(
            reached,
            ["play", "field", "shuffle", "heart", "more"],
            "the field is a Tab stop between the play tile and the shuffle"
        );
    }

    #[test]
    fn a_stuck_unfocused_field_paints_no_hairline_and_the_bar_edge_stands_in() {
        let ctx = context();
        let tokens = Tokens::DARK;
        let mut rows = Rows::narrowed();

        // At rest the field is in the flow and wears its own line.
        let rested = frame_in(&ctx, VIEWPORT, 0.0, Vec::new(), Some(rows.filter()));
        let (line, ink) = field_line(&rested.output).expect("a resting field is underlined");
        assert_eq!(ink, tokens.palette.border);
        assert!(
            (line.bottom() - placed(&rested, VIEWPORT).field.expect("field").bottom()).abs() < 0.01,
            "the line is the field's bottom: {line:?}"
        );

        // Stuck and unfocused it paints none, and the row's own edge carries the
        // line instead.
        let pinned = frame_in(
            &ctx,
            VIEWPORT,
            FILLER + 50.0,
            Vec::new(),
            Some(rows.filter()),
        );
        assert_eq!(
            field_line(&pinned.output),
            None,
            "a stuck unfocused field paints no line of its own"
        );
        let bar = pinned.rect();
        let edge = fills(&pinned.output, tokens.palette.border)
            .into_iter()
            .find(|(_, rect)| {
                (rect.height() - stroke::HAIRLINE).abs() < 0.01
                    && (rect.bottom() - bar.bottom()).abs() < 0.01
                    && (rect.width() - bar.width()).abs() < 0.01
            })
            .map(|(_, rect)| rect)
            .expect("the stuck row wears its own edge");
        let field = placed(&pinned, VIEWPORT).field.expect("field");
        assert!(
            edge.width() > field.width(),
            "the edge is the row's, not the field's underline: {edge:?} against {field:?}"
        );

        // Under the keyboard the line returns, in full ink.
        for event in click_events(field.center()) {
            let _ = frame_in(
                &ctx,
                VIEWPORT,
                FILLER + 50.0,
                vec![event],
                Some(rows.filter()),
            );
        }
        let typed = frame_in(
            &ctx,
            VIEWPORT,
            FILLER + 50.0,
            Vec::new(),
            Some(rows.filter()),
        );
        assert_eq!(
            field_line(&typed.output).map(|(_, ink)| ink),
            Some(tokens.palette.text),
            "a focused field says so in full ink while the row is stuck"
        );
    }

    #[test]
    fn the_count_sits_one_tools_gap_left_of_the_field() {
        let ctx = context();
        let mut rows = Rows::narrowed();
        let frame = frame_in(&ctx, WIDE, 0.0, Vec::new(), Some(rows.filter()));
        assert_eq!(
            frame.reply.hits.as_deref(),
            Some(&[1][..]),
            "one of the three rows matches"
        );

        let field = placed(&frame, WIDE).field.expect("a wide row has a field");
        let count = text_rect(&frame.output, "1 OF 3 TRACKS").expect("the count is painted");
        assert!(
            (count.right() - (field.left() - actionbar::TOOLS_GAP)).abs() < 0.01,
            "the count sits one tools gap left of the field: {count:?} against {field:?}"
        );
        assert!(
            (count.center().y - field.center().y).abs() < 0.01,
            "the count is on the field's own line: {count:?} against {field:?}"
        );

        // With no query the count goes, and the field says so with its
        // placeholder instead.
        let mut quiet = Rows::new();
        let frame = frame_in(&ctx, WIDE, 0.0, Vec::new(), Some(quiet.filter()));
        assert_eq!(frame.reply.hits, None);
        assert!(text_rect(&frame.output, "FILTER").is_some());
        assert_eq!(
            text_at(&frame.output, "3 OF 3 TRACKS").len(),
            0,
            "an unfiltered list paints no count"
        );
    }

    #[test]
    fn the_status_node_is_polite_carries_the_unshortened_label_and_exists_only_while_filtering() {
        let ctx = context();
        ctx.enable_accesskit();
        let mut rows = Rows::narrowed();
        let (filtered, read) =
            frame_probed(&ctx, WIDE, 0.0, Vec::new(), Some(rows.filter()), |ui| {
                ui.ctx()
                    .accesskit_node_builder(ui.scope_id().with("track-count"), |node| {
                        (node.role(), node.live(), node.label().map(str::to_owned))
                    })
                    .expect("the count has a status node")
            });
        assert_eq!(read.0, egui::Role::Status);
        assert_eq!(read.1, Some(egui::accesskit::Live::Polite));
        assert_eq!(
            read.2.as_deref(),
            Some("1 of 3 tracks"),
            "the node says the count in the field's own words, not the painted glyphs"
        );
        assert!(
            text_rect(&filtered.output, "1 OF 3 TRACKS").is_some(),
            "and the painted count is the upper-cased one"
        );

        let mut quiet = Rows::new();
        let (_, stray) = frame_probed(&ctx, WIDE, 0.0, Vec::new(), Some(quiet.filter()), |ui| {
            ui.ctx()
                .has_accesskit_node(ui.scope_id().with("track-count"))
        });
        assert!(!stray, "an unfiltered list announces nothing");
    }

    #[test]
    fn a_bar_with_no_room_for_the_field_applies_no_query() {
        let ctx = context();
        // Narrow enough that the field has less than its floor between the play
        // tile and the tools, so it is not drawn at all.
        let size = Vec2::new(310.0, 400.0);
        let mut rows = Rows::narrowed();
        let frame = frame_in(&ctx, size, 0.0, Vec::new(), Some(rows.filter()));

        assert!(
            placed(&frame, size).field.is_none(),
            "the field is not drawn in a row this narrow"
        );
        assert_eq!(
            frame.reply.hits, None,
            "a query nobody can see never narrows the list"
        );
        assert!(
            text_at(&frame.output, "midnight").is_empty()
                && text_at(&frame.output, "FILTER").is_empty(),
            "nor is there a field to paint it in: {:?}",
            text_at(&frame.output, "midnight")
        );
    }

    #[test]
    fn the_playlist_bar_without_an_entry_draws_with_inert_heart_and_more() {
        let ctx = context();
        let mut rows = Rows::narrowed();
        // A playlist the library cannot name: nothing to call it, nothing to say
        // about the heart, and no menu to open.
        let unnamed = Bar {
            title: None,
            playable: true,
            shuffle: true,
            more: None,
            saved: None,
            kind: kind(VIEWPORT),
            text: None,
        };

        let rested = frame_of(
            &ctx,
            &unnamed,
            VIEWPORT,
            0.0,
            Vec::new(),
            Some(rows.filter()),
        );
        let tiles = placed(&rested, VIEWPORT);
        assert_eq!(
            text_at(&rested.output, "midnight").len(),
            1,
            "the field is drawn for a page with no name"
        );
        assert_eq!(
            rested.reply.hits.as_deref(),
            Some(&[1][..]),
            "and its query still narrows the list"
        );
        assert!(
            text_at(&rested.output, "Late Night Drive").is_empty(),
            "there is no name to paint"
        );
        let heart = tiles.heart.expect("the heart is present");
        let more = tiles.more;

        // Both are present and both are inert: no list could say what the
        // context's saved state is, and the menu has nothing to offer.
        let mut last = Reply::quiet();
        for event in click_events(heart.center()) {
            last = frame_of(
                &ctx,
                &unnamed,
                VIEWPORT,
                0.0,
                vec![event],
                Some(rows.filter()),
            )
            .reply;
        }
        assert_eq!(last.saved, None, "an unknown heart asks for nothing");
        assert!(!last.played, "a press on the heart is not a press on play");

        let mut last = Reply::quiet();
        for event in click_events(more.center()) {
            last = frame_of(
                &ctx,
                &unnamed,
                VIEWPORT,
                0.0,
                vec![event],
                Some(rows.filter()),
            )
            .reply;
        }
        assert!(last.more_door.is_none(), "an inert menu posts no door");
        assert!(!last.played, "a press on the menu is not a press on play");

        // The play tile still works on the tracks the page has.
        let mut last = Reply::quiet();
        for event in click_events(tiles.play.center()) {
            last = frame_of(
                &ctx,
                &unnamed,
                VIEWPORT,
                0.0,
                vec![event],
                Some(rows.filter()),
            )
            .reply;
        }
        assert!(last.played, "a page with no name still plays its tracks");
    }

    #[test]
    fn the_text_slot_reports_its_press() {
        let ctx = context();
        let text = text_bar(kind(VIEWPORT));
        let follow = drawn(&ctx, &text, VIEWPORT, 0.0)
            .follow
            .expect("the row draws the page's text control");

        let reply = press(&ctx, &text, VIEWPORT, 0.0, follow.center());
        assert!(reply.followed, "the text control reports its press");
        assert!(
            !reply.played && !reply.shuffled && reply.saved.is_none() && reply.more_door.is_none(),
            "a press on the word is a press on nothing else"
        );
    }

    #[test]
    fn a_slot_that_is_not_live_reports_nothing() {
        let ctx = context();
        let quiet = Bar {
            text: Some(Text {
                label: "Following",
                live: false,
                failed: None,
            }),
            ..text_bar(kind(VIEWPORT))
        };
        let follow = drawn(&ctx, &quiet, VIEWPORT, 0.0)
            .follow
            .expect("a waiting control is still drawn in its place");
        let reply = press(&ctx, &quiet, VIEWPORT, 0.0, follow.center());
        assert!(
            !reply.followed,
            "a control that cannot act reports nothing when pressed"
        );
    }

    #[test]
    fn the_slot_draws_the_pages_label() {
        let ctx = context();
        let following = Bar {
            text: Some(Text {
                label: "Following",
                live: true,
                failed: None,
            }),
            ..text_bar(kind(VIEWPORT))
        };
        let frame = frame_of(&ctx, &following, VIEWPORT, 0.0, Vec::new(), None);
        let follow = drawn(&ctx, &following, VIEWPORT, 0.0)
            .follow
            .expect("the row draws the page's text control");
        let label = text_rect(&frame.output, "Following").expect("the word is painted");
        assert!(
            follow.contains_rect(label),
            "the word sits inside the control's box: {label:?} against {follow:?}"
        );
        assert_eq!(
            text_rect(&frame.output, "Follow"),
            None,
            "the label is the page's, not a word the row chose"
        );
    }

    #[test]
    fn a_bar_with_a_slot_has_no_heart() {
        let ctx = context();
        let text = text_bar(kind(VIEWPORT));
        let boxes = drawn(&ctx, &text, VIEWPORT, 0.0);
        assert!(boxes.follow.is_some(), "the row draws the text control");
        assert!(
            boxes.heart.is_none(),
            "a page with a word in the heart's place has no heart: {:?}",
            boxes.heart
        );
        assert!(boxes.shuffle.is_some(), "the tools are still there");
    }

    #[test]
    fn tab_order_with_a_slot_is_play_follow_shuffle_more() {
        let ctx = context();
        let text = text_bar(kind(VIEWPORT));
        let (_, expected) = frame_of_probed(&ctx, &text, VIEWPORT, 0.0, Vec::new(), None, |ui| {
            vec![
                controls::slot_id(ui, Control::HeroPlay, ()),
                controls::slot_id(ui, Control::Follow, ()),
                controls::slot_id(ui, Control::HeroShuffle, ()),
                controls::slot_id(ui, Control::HeroMore, ()),
            ]
        });

        let [press, release] = crate::view::test_support::tab();
        let mut reached = Vec::new();
        for _ in 0..expected.len() {
            let _ = frame_of(&ctx, &text, VIEWPORT, 0.0, vec![press.clone()], None);
            reached.push(ctx.memory(|memory| memory.focused()));
            let _ = frame_of(&ctx, &text, VIEWPORT, 0.0, vec![release.clone()], None);
        }
        let reached: Vec<&str> = reached
            .iter()
            .map(|id| match *id {
                None => "nothing",
                Some(id) if id == expected[0] => "play",
                Some(id) if id == expected[1] => "follow",
                Some(id) if id == expected[2] => "shuffle",
                Some(id) if id == expected[3] => "more",
                Some(_) => "something else",
            })
            .collect();
        assert_eq!(
            reached,
            ["play", "follow", "shuffle", "more"],
            "the word is a Tab stop between the play tile and the tools"
        );
    }

    #[test]
    fn a_failure_at_rest_sits_between_play_and_the_slot() {
        let ctx = context();
        let failed = Bar {
            text: Some(Text {
                label: "Following",
                live: true,
                failed: Some(Failure::Save),
            }),
            ..text_bar(kind(VIEWPORT))
        };
        let frame = frame_of(&ctx, &failed, VIEWPORT, 0.0, Vec::new(), None);
        let boxes = drawn(&ctx, &failed, VIEWPORT, 0.0);
        let follow = boxes.follow.expect("the row draws the text control");

        let copy = crate::signin::failure_message(Failure::Save);
        let hint = text_rect(&frame.output, copy).expect("the failure's copy is painted");
        assert!(
            hint.left() >= boxes.play.right() + actionbar::GAP - 0.01,
            "the copy starts where the name's box starts: {hint:?} against {:?}",
            boxes.play
        );
        assert!(
            hint.right() <= follow.left() - actionbar::GAP + 0.01,
            "the copy stops short of the control: {hint:?} against {follow:?}"
        );
        assert!(
            (hint.center().y - follow.center().y).abs() < 1.0,
            "the copy shares the control's line: {hint:?} against {follow:?}"
        );
        assert!(
            text_rect(&frame.output, "Following").is_some(),
            "the label stays where the server last left it"
        );
        assert_eq!(
            frame.flow.height(),
            actionbar::REST_H,
            "a message brings no second line: the row keeps its resting height"
        );
    }

    #[test]
    fn a_failure_replaces_the_pinned_title_and_the_bar_stays_sixty() {
        let ctx = context();
        let failed = Bar {
            text: Some(Text {
                label: "Following",
                live: true,
                failed: Some(Failure::Save),
            }),
            ..text_bar(kind(VIEWPORT))
        };
        let clean = frame_of(
            &ctx,
            &text_bar(kind(VIEWPORT)),
            VIEWPORT,
            FILLER + 50.0,
            Vec::new(),
            None,
        );
        assert!(
            text_rect(&clean.output, "Vela Moreno").is_some(),
            "a pinned row with nothing to report paints the name"
        );

        let pinned = frame_of(&ctx, &failed, VIEWPORT, FILLER + 50.0, Vec::new(), None);
        let copy = crate::signin::failure_message(Failure::Save);
        let hint = text_rect(&pinned.output, copy).expect("the failure's copy is painted");
        assert!(
            text_rect(&pinned.output, "Vela Moreno").is_none(),
            "the message takes the name's box while it stands: {:?}",
            text_rect(&pinned.output, "Vela Moreno")
        );
        let bar = pinned.rect();
        assert_eq!(
            bar.height(),
            actionbar::STUCK_H,
            "the row is still the pinned height: {bar:?}"
        );
        let boxes = drawn(&ctx, &failed, VIEWPORT, FILLER + 50.0);
        assert!(
            hint.left() >= boxes.play.right() + actionbar::GAP - 0.01,
            "the copy sits in the name's box: {hint:?}"
        );
        assert!(
            hint.right() <= boxes.follow.expect("the slot").left() - actionbar::GAP + 0.01,
            "and in front of the control: {hint:?}"
        );
        assert!(
            (hint.center().y - bar.center().y).abs() < 0.01,
            "the copy is on the row's line: {hint:?} against {bar:?}"
        );
    }

    #[test]
    fn a_failure_with_no_box_paints_nothing() {
        let ctx = context();
        ctx.enable_accesskit();
        // Narrow enough that the row has no name's box between the play tile and
        // the text control.
        let size = Vec2::new(300.0, 400.0);
        let failed = Bar {
            text: Some(Text {
                label: "Following",
                live: true,
                failed: Some(Failure::Save),
            }),
            ..text_bar(kind(size))
        };
        let mut announced = None;
        let (frame, _) = frame_of_probed(&ctx, &failed, size, 0.0, Vec::new(), None, |ui| {
            announced = ui
                .ctx()
                .accesskit_node_builder(ui.scope_id().with("bar-status"), |node| {
                    (node.role(), node.live(), node.label().map(str::to_owned))
                });
        });
        let copy = crate::signin::failure_message(Failure::Save);
        assert!(
            text_rect(&frame.output, copy).is_none(),
            "a message with no box to sit in paints nothing: {:?}",
            text_rect(&frame.output, copy)
        );
        let announced = announced.expect("the failure still speaks through the status node");
        assert_eq!(announced.0, egui::Role::Status);
        assert_eq!(announced.1, Some(egui::accesskit::Live::Polite));
        assert_eq!(announced.2.as_deref(), Some(copy));
        assert!(
            text_rect(&frame.output, "Vela Moreno").is_none(),
            "and the name has no box either, so neither is drawn"
        );
        assert!(
            drawn(&ctx, &failed, size, 0.0).follow.is_some(),
            "the control is still drawn: only its message had nowhere to go"
        );
    }

    #[test]
    fn the_bar_alone_reports_its_own_reach() {
        let ctx = context();
        pin::clear(&ctx);
        let text = text_bar(kind(VIEWPORT));

        let _ = frame_of(&ctx, &text, VIEWPORT, 0.0, Vec::new(), None);
        assert_eq!(
            pin::inset(&ctx),
            0.0,
            "a row in its place reaches over nothing"
        );

        let _ = frame_of(&ctx, &text, VIEWPORT, FILLER + 50.0, Vec::new(), None);
        assert_eq!(
            pin::inset(&ctx),
            actionbar::STUCK_H,
            "a pinned row reports its own reach"
        );

        let _ = frame_of(&ctx, &text, VIEWPORT, FILLER + 50.0, Vec::new(), None);
        assert_eq!(
            pin::inset(&ctx),
            actionbar::STUCK_H,
            "the second frame reports the same reach, not twice it"
        );
    }

    #[test]
    fn rows_never_paint_over_the_pinned_bar() {
        let ctx = context();
        let tokens = Tokens::DARK;
        let bar = bar(kind(VIEWPORT));
        let mut output = ctx.run_ui(input(VIEWPORT, Vec::new()), |ui| {
            egui::ScrollArea::vertical()
                .id_salt("bar")
                .vertical_scroll_offset(FILLER + 50.0)
                .show(ui, |ui| {
                    ui.add_space(FILLER);
                    let _ = show(ui, &bar, None, &tokens);
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
        let bar = bar(kind(VIEWPORT));
        let id = egui::Id::unique("row-under-the-bar");
        let at = Pos2::new(400.0, 20.0);
        let mut clicked = false;
        for events in [
            vec![Event::PointerMoved(at)],
            vec![crate::view::test_support::button(at, true)],
            vec![crate::view::test_support::button(at, false)],
        ] {
            let mut output = ctx.run_ui(input(VIEWPORT, events), |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("bar")
                    .vertical_scroll_offset(FILLER + 50.0)
                    .show(ui, |ui| {
                        ui.add_space(FILLER);
                        let _ = show(ui, &bar, None, &tokens);
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
