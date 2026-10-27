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
        Icon::Play,
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
                heart.icon,
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
                heart.icon,
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
    /// The glyph itself, so the filled heart and the accent tone cannot disagree.
    icon: Icon,
}

/// The heart's three answers, in one place so the label and the glyph cannot
/// disagree about whether the context is in the library.
fn heart(saved: Option<bool>) -> Heart {
    match saved {
        Some(true) => Heart {
            label: "Remove from your library",
            live: true,
            lit: true,
            icon: Icon::HeartFilled,
        },
        Some(false) => Heart {
            label: "Save to your library",
            live: true,
            lit: false,
            icon: Icon::Heart,
        },
        None => Heart {
            label: "Save to your library",
            live: false,
            lit: false,
            icon: Icon::Heart,
        },
    }
}

#[cfg(test)]
mod tests;
