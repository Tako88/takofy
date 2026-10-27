//! The shared control slot: interaction, accessibility and the focus ring.
//!
//! A painted control draws itself, but every control passes through this one
//! seam first. The slot is where a waiting control stops being interactive: it
//! senses hover only, never joins the tab order, and announces itself disabled
//! with its label through `WidgetInfo`.
//!
//! The focus ring is drawn here too, because every control and row wants the
//! same one. It shows only after keyboard input, so a clicked control leaves no
//! ring behind it.

use std::hash::Hash;
use std::sync::Arc;

use egui::emath::GuiRounding;
use egui::text::LayoutJob;
use egui::{
    Color32, CornerRadius, FontId, Galley, Rect, Response, Role, Sense, Stroke, StrokeKind,
    TextFormat, Ui, Vec2, WidgetInfo, pos2,
};

use crate::availability::{Availability, Control, availability};
use crate::icon::{self, Icon};
use crate::theme::ease;
use crate::theme::icon::Size;
use crate::theme::{
    MonoWeight, Tokens, Weight, extent, fonts, motion, radius, space, stroke, text, tracking,
};

/// What a slot hands back.
pub struct Slot {
    pub response: Response,
    /// 0 at rest, 1 fully hovered, eased. Always 0 while waiting.
    pub hover: f32,
    /// Whether the control can act: its view exists *and* the moment allows it.
    pub live: bool,
}

/// Wires one control's rect to its interaction and accessibility.
///
/// `role` is a parameter because the search field is a `Role::SearchInput` and
/// not a button. `tokens` is needed for the focus ring's accent, which is not a
/// field of egui's `Visuals`.
///
/// The id is `(control, ())`, which hashes the same as the control alone, so a
/// control that appears once keeps the id it always had.
pub fn slot(
    ui: &mut Ui,
    control: Control,
    role: Role,
    rect: Rect,
    label: &str,
    tokens: &Tokens,
    live: bool,
) -> Slot {
    slot_keyed(ui, control, (), role, rect, label, tokens, live)
}

/// The id a keyed control's slot is registered under.
///
/// A caller that has to read a control's state before it draws — the row's `⋯`
/// asks whether its menu is the one open — needs the id first, and this is the
/// one place the formula lives.
pub fn slot_id(ui: &Ui, control: Control, key: impl Hash + std::fmt::Debug) -> egui::Id {
    ui.scope_id().with((control, key))
}

/// Wires one control's rect to its interaction and accessibility, keyed.
///
/// A control that appears once can use `slot`. One that appears once per row —
/// a like or a more — must pass a key, because `availability` and the id would
/// otherwise be identical for every row and every row would share one hover,
/// one focus node and one AccessKit node.
///
/// `role` is a parameter because the search field is a `Role::SearchInput` and
/// not a button. `tokens` is needed for the focus ring's accent, which is not a
/// field of egui's `Visuals`.
#[expect(
    clippy::too_many_arguments,
    reason = "a control needs its identity, place, label and state"
)]
pub fn slot_keyed(
    ui: &mut Ui,
    control: Control,
    key: impl Hash + std::fmt::Debug,
    role: Role,
    rect: Rect,
    label: &str,
    tokens: &Tokens,
    live: bool,
) -> Slot {
    // Two different facts meet here. `availability` is about the build and never
    // changes; `live` is about the moment — nothing is queued, no track is
    // loaded. A control that cannot act right now is offered exactly as a waiting
    // one is: unfocusable, announced disabled, and drawn with the same tone.
    let ready = availability(control) == Availability::Ready && live;
    let id = slot_id(ui, control, key);
    let sense = if ready {
        // Draggable as well as clickable: the seek bar is scrubbed, and a press and
        // release without movement is still a click.
        Sense::click_and_drag()
    } else {
        Sense::hover()
    };
    let response = ui.interact(rect, id, sense);

    response.widget_info(|| WidgetInfo::labeled(role, ready, label));

    let hover = if ready {
        ui.ctx().animate_bool_with_time_and_easing(
            id.with("hover"),
            response.hovered(),
            motion::FAST,
            ease::out,
        )
    } else {
        0.0
    };

    if ready {
        focus_ring(
            ui,
            &response,
            rect.expand(extent::FOCUS_OFFSET + stroke::HAIRLINE),
            radius::SM,
            tokens,
        );
    }

    if ready && response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    Slot {
        response,
        hover,
        live: ready,
    }
}

// ---------------------------------------------------------------------------
// The painted controls
//
// Every control takes a rect and paints itself from tokens. A waiting control
// keeps its frame and drops to `text_disabled`, because it has to read as
// something that is present and not yet usable rather than something faded out.
// ---------------------------------------------------------------------------

/// Reserves a box in the current layout and returns its rect.
///
/// A control paints into a rect it is handed, so a region and the gallery place
/// it the same way. This is the only thing here that moves the layout cursor.
fn place(ui: &mut Ui, size: Vec2) -> Rect {
    ui.allocate_space(size).1
}

/// A square of `side` centred in `rect`, snapped so a hairline glyph lands on
/// whole pixels.
fn centred(ui: &Ui, rect: Rect, side: f32) -> Rect {
    Rect::from_center_size(rect.center(), Vec2::splat(side)).round_to_pixels(ui.pixels_per_point())
}

/// The hover wash: the panel a control sits on, moved toward the hover tone.
///
/// Nothing is drawn at rest, so a control on the playbar does not stamp a
/// panel-coloured patch onto the window behind it.
fn wash(ui: &Ui, tokens: &Tokens, mix: f32) -> Option<Color32> {
    (mix > 0.0).then(|| {
        ui.visuals()
            .panel_fill
            .lerp_to_gamma(tokens.palette.surface_hover, mix)
    })
}

/// A control's foreground: muted at rest, `text` under the pointer, and the
/// disabled tone while it is waiting. Waiting never brightens.
fn foreground(tokens: &Tokens, ready: bool, mix: f32) -> Color32 {
    if ready {
        tokens
            .palette
            .text_muted
            .lerp_to_gamma(tokens.palette.text, mix)
    } else {
        tokens.palette.text_disabled
    }
}

/// The ring a keyboard-focused row, tile or control wears.
///
/// Inside `rect`, so it lies on the widget's own edge and a scroll area or
/// region clip cuts it with the widget rather than over its neighbours. `radius`
/// is the widget's own corner, so the ring follows the shape it rings.
pub fn focus_ring(ui: &Ui, response: &Response, rect: Rect, radius: CornerRadius, tokens: &Tokens) {
    if !super::focus::visible(ui.ctx(), response.id) {
        return;
    }
    ui.painter().rect_stroke(
        rect,
        radius,
        Stroke::new(stroke::HAIRLINE, tokens.palette.accent),
        StrokeKind::Inside,
    );
}

/// One line of text, laid out with the reference's letter spacing.
fn line(ui: &Ui, label: &str, font: FontId, color: Color32, spacing: f32) -> Arc<Galley> {
    let format = TextFormat {
        font_id: font,
        color,
        extra_letter_spacing: spacing,
        ..Default::default()
    };
    let mut job = LayoutJob::single_section(label.to_owned(), format);
    job.wrap.max_rows = 1;
    ui.painter().layout_job(job)
}

/// An icon-only button.
/// A control's identity, its place, its glyph, its size, its label and whether it
/// can act right now are six separate facts about one button; grouping them would
/// invent a type that has no other use.
#[expect(clippy::too_many_arguments, reason = "six facts about one button")]
pub fn icon_button(
    ui: &mut Ui,
    tokens: &Tokens,
    control: Control,
    rect: Rect,
    icon: Icon,
    size: Size,
    label: &str,
    live: bool,
) -> Slot {
    icon_button_keyed(ui, tokens, control, (), rect, icon, size, label, live)
}

/// An icon button that appears once per row.
///
/// A row's like and more are one control type repeated down a list, so they have
/// to carry a key or every row would share one hover, one focus node and one
/// AccessKit node. It paints exactly what `icon_button` paints.
#[expect(
    clippy::too_many_arguments,
    reason = "the same facts as a button, plus which row it belongs to"
)]
pub fn icon_button_keyed(
    ui: &mut Ui,
    tokens: &Tokens,
    control: Control,
    key: impl Hash + std::fmt::Debug,
    rect: Rect,
    icon: Icon,
    size: Size,
    label: &str,
    live: bool,
) -> Slot {
    icon_button_painted(
        ui,
        tokens,
        control,
        key,
        rect,
        icon,
        size,
        label,
        live,
        Lit::Rest,
    )
}

/// An icon button whose own state is on: the glyph takes the accent tone, the
/// one signal colour the palette has.
#[expect(
    clippy::too_many_arguments,
    reason = "the same facts as a button, plus the state its glyph carries"
)]
pub fn icon_button_on(
    ui: &mut Ui,
    tokens: &Tokens,
    control: Control,
    key: impl Hash + std::fmt::Debug,
    rect: Rect,
    icon: Icon,
    size: Size,
    label: &str,
    live: bool,
) -> Slot {
    icon_button_painted(
        ui,
        tokens,
        control,
        key,
        rect,
        icon,
        size,
        label,
        live,
        Lit::On,
    )
}

/// The button behind both, so the two entry points paint one thing.
#[expect(clippy::too_many_arguments, reason = "six facts about one button")]
fn icon_button_painted(
    ui: &mut Ui,
    tokens: &Tokens,
    control: Control,
    key: impl Hash + std::fmt::Debug,
    rect: Rect,
    icon: Icon,
    size: Size,
    label: &str,
    live: bool,
    lit: Lit,
) -> Slot {
    let slot = slot_keyed(ui, control, key, Role::Button, rect, label, tokens, live);
    paint_icon_button(ui, tokens, rect, icon, size, &slot, lit);
    slot
}

/// An icon button a carried block hangs over: the selection's 16% fill and a lit
/// glyph, because a drop claims it the way a pick claims a row.
///
/// It takes no `live`: the claim makes the button inert, so it never competes
/// with a click or a focus ring.
// a11y: always Waiting and announced disabled; the drop is the only live thing here.
pub fn icon_button_claimed(
    ui: &mut Ui,
    tokens: &Tokens,
    control: Control,
    rect: Rect,
    icon: Icon,
    size: Size,
    label: &str,
) -> Slot {
    let slot = slot(ui, control, Role::Button, rect, label, tokens, false);
    paint_icon_button(ui, tokens, rect, icon, size, &slot, Lit::Claimed);
    slot
}

/// An icon button that says the menu it opened is still up: full ink on the
/// hover's fill, which is the only thing on the row that marks the menu's state.
///
/// A row's `⋯` is one per row, so it carries a key like every other repeated
/// control.
#[expect(
    clippy::too_many_arguments,
    reason = "the same facts as a button, plus whether its menu is open"
)]
pub fn icon_button_open(
    ui: &mut Ui,
    tokens: &Tokens,
    control: Control,
    key: impl Hash + std::fmt::Debug,
    rect: Rect,
    icon: Icon,
    size: Size,
    label: &str,
    live: bool,
    open: bool,
) -> Slot {
    let slot = slot_keyed(ui, control, key, Role::Button, rect, label, tokens, live);
    let lit = if open { Lit::Open } else { Lit::Rest };
    paint_icon_button(ui, tokens, rect, icon, size, &slot, lit);
    slot
}

/// How an icon button is lit, if at all.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Lit {
    /// Nothing is claimed and no menu is open: the hover wash alone.
    Rest,
    /// A carried block hangs over it.
    Claimed,
    /// The menu this button opened is still up.
    Open,
    /// The button's own state is on.
    On,
}

/// An icon button's body: the wash, or the claim's or the open state's fill, and
/// the glyph over it.
fn paint_icon_button(
    ui: &mut Ui,
    tokens: &Tokens,
    rect: Rect,
    icon: Icon,
    size: Size,
    slot: &Slot,
    lit: Lit,
) {
    let color = match lit {
        Lit::Claimed => {
            ui.painter()
                .rect_filled(rect, radius::SM, tokens.palette.row_selected);
            tokens.palette.text
        }
        Lit::Open => {
            ui.painter()
                .rect_filled(rect, radius::SM, tokens.palette.surface_hover);
            tokens.palette.text
        }
        Lit::Rest => {
            if let Some(fill) = wash(ui, tokens, slot.hover) {
                ui.painter().rect_filled(rect, radius::SM, fill);
            }
            foreground(tokens, slot.live, slot.hover)
        }
        Lit::On => {
            if let Some(fill) = wash(ui, tokens, slot.hover) {
                ui.painter().rect_filled(rect, radius::SM, fill);
            }
            tokens.palette.accent
        }
    };
    icon::paint(
        ui.painter(),
        icon,
        centred(ui, rect, size.side),
        size,
        color,
    );
}

/// A bordered button: the history pair, and the play tile.
///
/// The frame stays put while the control is waiting, which is the whole point of
/// drawing it at all.
/// A control's identity, its place, its glyph, its size, its label and whether it
/// can act right now are six separate facts about one button; grouping them would
/// invent a type that has no other use.
#[expect(clippy::too_many_arguments, reason = "six facts about one button")]
pub fn round_button(
    ui: &mut Ui,
    tokens: &Tokens,
    control: Control,
    rect: Rect,
    icon: Icon,
    size: Size,
    label: &str,
    live: bool,
    current: bool,
) -> Slot {
    let slot = slot(ui, control, Role::Button, rect, label, tokens, live);
    let live = slot.live;
    // A current button is the reference's `aria-current`: a stronger hairline
    // and a full-strength glyph, so the cluster says where you are.
    let border = if current || (live && slot.hover > 0.0) {
        tokens.palette.border_strong
    } else {
        tokens.palette.border
    };
    if let Some(fill) = wash(ui, tokens, slot.hover) {
        ui.painter().rect_filled(rect, radius::SM, fill);
    }
    ui.painter().rect_stroke(
        rect,
        radius::SM,
        Stroke::new(stroke::HAIRLINE, border),
        StrokeKind::Inside,
    );
    icon::paint(
        ui.painter(),
        icon,
        centred(ui, rect, size.side),
        size,
        foreground(tokens, live, slot.hover),
    );
    slot
}

/// A square segmented tab. The selected one is filled and bordered in `text`;
/// the rest keep a hairline.
pub fn chip(ui: &mut Ui, tokens: &Tokens, control: Control, label: &str, selected: bool) -> Slot {
    let rect = place(ui, chip_size(ui, tokens, label));
    chip_in(ui, tokens, control, rect, label, selected)
}

/// The box a chip's label needs.
///
/// The reference wraps its chips, so a caller that wraps them has to know the
/// sizes before it decides where the lines break.
pub fn chip_size(ui: &Ui, tokens: &Tokens, label: &str) -> Vec2 {
    let galley = line(
        ui,
        &label.to_uppercase(),
        FontId::new(text::XXS, fonts::mono(MonoWeight::Medium)),
        tokens.palette.text_muted,
        crate::view::text::tracking_pts(text::XXS, tracking::WIDE),
    );
    Vec2::new(
        galley.size().x + 2.0 * extent::CHIP_PAD_X,
        galley.size().y + 2.0 * extent::CHIP_PAD_Y,
    )
}

/// A bordered button carrying a word rather than a glyph: the reference's
/// `.button`, which is the sign-in card's one control.
///
/// The frame is `border_strong` at rest and the text colour under the pointer,
/// which is the reference's own hover for a button. A waiting one keeps the frame
/// and drops to the disabled tone, like every other control here.
pub fn text_button(
    ui: &mut Ui,
    tokens: &Tokens,
    control: Control,
    rect: Rect,
    label: &str,
    live: bool,
) -> Slot {
    let slot = slot(ui, control, Role::Button, rect, label, tokens, live);
    let ready = slot.live;
    // The reference's `.button` sets its label to the text colour at rest; the
    // pointer changes only its frame and its wash. A waiting one keeps the frame
    // and drops to the disabled tone, like every other control here.
    let color = if ready {
        tokens.palette.text
    } else {
        tokens.palette.text_disabled
    };
    let border = if ready && slot.hover > 0.0 {
        tokens.palette.text
    } else {
        tokens.palette.border_strong
    };

    if let Some(fill) = wash(ui, tokens, slot.hover) {
        ui.painter().rect_filled(rect, radius::SM, fill);
    }
    ui.painter().rect_stroke(
        rect,
        radius::SM,
        Stroke::new(stroke::HAIRLINE, border),
        StrokeKind::Inside,
    );
    let galley = line(ui, label, text_button_font(), color, 0.0);
    ui.painter().galley(
        pos2(
            rect.center().x - galley.size().x / 2.0,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        color,
    );
    slot
}

/// The box a text button's label needs.
pub fn text_button_size(ui: &Ui, tokens: &Tokens, label: &str) -> Vec2 {
    let galley = line(ui, label, text_button_font(), tokens.palette.text, 0.0);
    Vec2::new(
        galley.size().x + 2.0 * space::S3,
        galley.size().y + 2.0 * extent::BUTTON_PAD_Y,
    )
}

/// The face a button's label is set in: `--text-sm` at weight 600.
fn text_button_font() -> FontId {
    FontId::new(text::SM, fonts::sans(Weight::Semibold))
}

/// The one loud element in the interface: a solid accent tile.
///
/// The reference draws it for the hero's own play control, for a hovered card and
/// for a hovered row's index, and it is the only thing here that fills with the
/// accent colour. Sound lives here and nowhere else. The glyph is the caller's
/// because the playbar's tile is the same tile saying pause.
pub fn play_tile(
    ui: &mut Ui,
    tokens: &Tokens,
    control: Control,
    rect: Rect,
    glyph: Icon,
    label: &str,
    live: bool,
) -> Slot {
    play_tile_keyed(ui, tokens, control, (), rect, glyph, label, live)
}

/// The accent tile, keyed so one control can appear once per card or row.
///
/// The library's play is one control over every item, so it has to carry the
/// item's own key: without one, every card would share one hover, one focus node
/// and one AccessKit node.
#[expect(
    clippy::too_many_arguments,
    reason = "the same facts as a tile, plus which item it belongs to"
)]
pub fn play_tile_keyed(
    ui: &mut Ui,
    tokens: &Tokens,
    control: Control,
    key: impl Hash + std::fmt::Debug,
    rect: Rect,
    glyph: Icon,
    label: &str,
    live: bool,
) -> Slot {
    let slot = slot_keyed(ui, control, key, Role::Button, rect, label, tokens, live);
    let ready = slot.live;
    let fill = if !ready {
        tokens.palette.text_disabled
    } else if slot.hover > 0.0 {
        tokens
            .palette
            .accent
            .lerp_to_gamma(tokens.palette.accent_hover, slot.hover)
    } else {
        tokens.palette.accent
    };
    ui.painter().rect_filled(rect, radius::SM, fill);
    icon::paint(
        ui.painter(),
        glyph,
        centred(ui, rect, Size::MD.side),
        Size::MD,
        tokens.palette.text_inverse,
    );
    slot
}

/// A chip drawn into a rect the caller chose, keyed so one control can appear
/// several times on a page without its chips sharing hover, focus or an
/// AccessKit node.
pub fn chip_in_keyed(
    ui: &mut Ui,
    tokens: &Tokens,
    control: Control,
    key: impl std::hash::Hash + std::fmt::Debug,
    rect: Rect,
    label: &str,
    selected: bool,
) -> Slot {
    let live = availability(control) == Availability::Ready;
    let color = if live {
        tokens.palette.text_muted
    } else {
        tokens.palette.text_disabled
    };
    let galley = line(
        ui,
        &label.to_uppercase(),
        FontId::new(text::XXS, fonts::mono(MonoWeight::Medium)),
        color,
        crate::view::text::tracking_pts(text::XXS, tracking::WIDE),
    );
    let slot = slot_keyed(ui, control, key, Role::Button, rect, label, tokens, true);

    let border = if selected {
        tokens.palette.text
    } else if live && slot.hover > 0.0 {
        tokens.palette.border_strong
    } else {
        tokens.palette.border
    };
    if selected {
        ui.painter()
            .rect_filled(rect, radius::SM, tokens.palette.pressed);
    }
    ui.painter().rect_stroke(
        rect,
        radius::SM,
        Stroke::new(stroke::HAIRLINE, border),
        StrokeKind::Inside,
    );
    // The label is centred in the chip: the filter rows size each chip to its
    // label, so this only shows where the caller hands a wider cell, like the
    // sidebar's grid.
    let at = pos2(
        rect.left() + ((rect.width() - galley.size().x) / 2.0).max(extent::CHIP_PAD_X),
        rect.center().y - galley.size().y / 2.0,
    );
    ui.painter().galley(at, galley, color);
    let _ = ui
        .ctx()
        .accesskit_node_builder(slot.response.id, |node| node.set_selected(selected));

    slot
}

/// A chip drawn into a rect the caller chose.
pub fn chip_in(
    ui: &mut Ui,
    tokens: &Tokens,
    control: Control,
    rect: Rect,
    label: &str,
    selected: bool,
) -> Slot {
    chip_in_keyed(ui, tokens, control, (), rect, label, selected)
}

/// A slider. `None` draws the track alone, which is what a bar with no known
/// duration has to show rather than a position it cannot know.
pub fn slider(
    ui: &mut Ui,
    tokens: &Tokens,
    control: Control,
    rect: Rect,
    fraction: Option<f32>,
    label: &str,
    live: bool,
) -> Slot {
    let slot = slot(ui, control, Role::Slider, rect, label, tokens, live);
    let track = Rect::from_center_size(
        rect.center(),
        Vec2::new(rect.width(), extent::SLIDER_TRACK_H),
    );
    ui.painter()
        .rect_filled(track, radius::PILL, tokens.palette.border_strong);

    if let Some(fraction) = fraction {
        let width = track.width() * fraction.clamp(0.0, 1.0);
        ui.painter().rect_filled(
            Rect::from_min_size(track.min, Vec2::new(width, track.height())),
            radius::PILL,
            tokens.palette.text,
        );

        if slot.live && (slot.hover > 0.0 || slot.response.has_focus()) {
            let thumb = Rect::from_center_size(
                pos2(track.left() + width, rect.center().y),
                Vec2::new(extent::SLIDER_THUMB_W, extent::SLIDER_H),
            );
            ui.painter()
                .rect_filled(thumb, radius::PILL, tokens.palette.accent);
        }
    }

    slot
}

/// The fraction across a track at `pointer_x`, clamped to `[0, 1]`. A zero
/// width has no interior, so it reads as the start.
pub fn fraction_at(pointer_x: f32, left: f32, width: f32) -> f32 {
    if width <= 0.0 {
        return 0.0;
    }
    ((pointer_x - left) / width).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests;
