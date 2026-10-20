//! The shared control slot: interaction, accessibility and the focus ring.
//!
//! A painted control draws itself, but every control passes through this one
//! seam first. The slot is where a waiting control stops being interactive: it
//! senses hover only, never joins the tab order, and announces itself disabled
//! with its label through `WidgetInfo`.
//!
//! The focus ring is drawn here too, because every control wants the same one
//! and only the slot knows the control has focus.

use std::hash::Hash;
use std::sync::Arc;

use egui::emath::GuiRounding;
use egui::text::LayoutJob;
use egui::{
    Color32, FontId, Galley, Rect, Response, Role, Sense, Stroke, StrokeKind, TextFormat, Ui, Vec2,
    WidgetInfo, pos2,
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
    let id = ui.scope_id().with((control, key));
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

    if ready && response.has_focus() {
        focus_ring(ui, rect, tokens);
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

/// The ring a focused control wears.
///
/// Shared, because a library row is focusable too and rows are not controls: they
/// are one per playlist, so they cannot come from the availability table.
pub fn focus_ring(ui: &Ui, rect: Rect, tokens: &Tokens) {
    ui.painter().rect_stroke(
        rect.expand(extent::FOCUS_OFFSET),
        radius::SM,
        Stroke::new(stroke::HAIRLINE, tokens.palette.accent),
        StrokeKind::Outside,
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
    let slot = slot_keyed(ui, control, key, Role::Button, rect, label, tokens, live);
    if let Some(fill) = wash(ui, tokens, slot.hover) {
        ui.painter().rect_filled(rect, radius::SM, fill);
    }
    icon::paint(
        ui.painter(),
        icon,
        centred(ui, rect, size.side),
        size,
        foreground(tokens, live, slot.hover),
    );
    slot
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
/// accent colour. Sound lives here and nowhere else.
pub fn play_tile(
    ui: &mut Ui,
    tokens: &Tokens,
    control: Control,
    rect: Rect,
    label: &str,
    live: bool,
) -> Slot {
    let slot = slot(ui, control, Role::Button, rect, label, tokens, live);
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
        Icon::Play,
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
mod tests {
    use super::*;
    use crate::theme::Tokens;
    use egui::{Context, Event, Modifiers, Pos2, RawInput, Rect, Ui, Vec2};

    fn input(events: Vec<Event>) -> RawInput {
        RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(400.0, 400.0))),
            events,
            ..Default::default()
        }
    }

    /// Runs one pass and consumes the font-atlas delta it may have produced.
    fn run(ctx: &Context, input: RawInput, draw: impl FnMut(&mut Ui)) {
        let mut output = ctx.run_ui(input, draw);
        output.textures_delta.clear();
    }

    fn cell(index: f32) -> Rect {
        Rect::from_min_size(Pos2::new(index * 40.0, 0.0), Vec2::new(28.0, 28.0))
    }

    #[test]
    fn a_waiting_control_is_not_interactive_and_not_focusable() {
        let ctx = Context::default();
        let mut sense = None;
        run(&ctx, input(Vec::new()), |ui| {
            let slot = slot(
                ui,
                Control::LibraryCreate,
                Role::Button,
                cell(0.0),
                "Create playlist or folder",
                &Tokens::DARK,
                true,
            );
            sense = Some(slot.response.sense);
        });
        let sense = sense.expect("the slot drew");
        assert!(!sense.senses_click());
        assert!(!sense.is_focusable());
    }

    #[test]
    fn a_ready_control_is_focusable() {
        let ctx = Context::default();
        let mut sense = None;
        run(&ctx, input(Vec::new()), |ui| {
            let slot = slot(
                ui,
                Control::DrawerToggle,
                Role::Button,
                cell(0.0),
                "Toggle",
                &Tokens::DARK,
                true,
            );
            sense = Some(slot.response.sense);
        });
        let sense = sense.expect("the slot drew");
        assert!(sense.senses_click());
        assert!(sense.is_focusable());
    }

    /// A control whose view exists can still be unable to act — nothing is queued,
    /// no track is loaded. It is offered exactly as a waiting one is: if it were
    /// not, it would take Tab and announce itself as enabled while doing nothing.
    #[test]
    fn a_momentarily_inert_control_is_offered_like_a_waiting_one() {
        let ctx = Context::default();
        let mut offered = None;
        run(&ctx, input(Vec::new()), |ui| {
            let live = slot(
                ui,
                Control::DrawerToggle,
                Role::Button,
                cell(0.0),
                "Toggle",
                &Tokens::DARK,
                true,
            );
            let inert = slot(
                ui,
                Control::DrawerToggle,
                Role::Button,
                cell(1.0),
                "Toggle",
                &Tokens::DARK,
                false,
            );
            offered = Some((live.live, inert.live, inert.response.sense));
        });
        let (live, inert, sense) = offered.expect("both slots drew");
        assert!(live, "a control that can act is live");
        assert!(!inert, "one that cannot is not");
        assert!(!sense.senses_click());
        assert!(!sense.is_focusable());
    }

    #[test]
    fn a_waiting_control_is_announced_disabled_with_its_label() {
        let ctx = Context::default();
        // AccessKit state exists only while the pass that built it is live.
        ctx.enable_accesskit();
        let mut node = None;
        run(&ctx, input(Vec::new()), |ui| {
            let slot = slot(
                ui,
                Control::LibraryCreate,
                Role::Button,
                cell(0.0),
                "Create playlist or folder",
                &Tokens::DARK,
                true,
            );
            assert!(ui.ctx().has_accesskit_node(slot.response.id));
            node = ui.ctx().accesskit_node_builder(slot.response.id, |node| {
                (node.is_disabled(), node.label().map(str::to_owned))
            });
        });
        let (disabled, label) = node.expect("the slot has an AccessKit node");
        assert!(disabled, "a waiting control must be announced disabled");
        assert_eq!(label.as_deref(), Some("Create playlist or folder"));
    }

    /// Which chip filters the list is a selection, so it has to reach the
    /// accessibility tree and not only the fill: the fill says nothing to a
    /// screen reader.
    #[test]
    fn a_pressed_chip_is_announced_selected() {
        let ctx = Context::default();
        // AccessKit state exists only while the pass that built it is live.
        ctx.enable_accesskit();
        let mut states = None;
        run(&ctx, input(Vec::new()), |ui| {
            let pressed = chip_in(
                ui,
                &Tokens::DARK,
                Control::FilterPlaylists,
                cell(0.0),
                "Playlists",
                true,
            );
            let idle = chip_in(
                ui,
                &Tokens::DARK,
                Control::FilterArtists,
                cell(1.0),
                "Artists",
                false,
            );
            let read = |id| {
                ui.ctx()
                    .accesskit_node_builder(id, |node| node.is_selected())
                    .expect("a chip has an AccessKit node")
            };
            states = Some((read(pressed.response.id), read(idle.response.id)));
        });
        let (pressed, idle) = states.expect("both chips drew");
        assert_eq!(
            pressed,
            Some(true),
            "the pressed chip is announced selected"
        );
        assert_eq!(
            idle,
            Some(false),
            "the chips that do not filter the list are announced not selected"
        );
    }

    #[test]
    fn tab_never_lands_on_a_waiting_control() {
        let ctx = Context::default();
        let controls = [
            (Control::DrawerToggle, "Toggle"),
            (Control::LibraryCreate, "Create playlist or folder"),
            (Control::TrailPlaylists, "Playlists"),
        ];
        let tab = Event::Key {
            key: egui::Key::Tab,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        };

        // Focus traversal reads the previous pass, and one Tab pass per widget.
        run(&ctx, input(Vec::new()), |_ui| {});
        let mut focused = Vec::new();
        for _ in 0..6 {
            let focused = &mut focused;
            run(&ctx, input(vec![tab.clone()]), |ui| {
                for (index, (control, label)) in controls.iter().enumerate() {
                    let slot = slot(
                        ui,
                        *control,
                        Role::Button,
                        cell(index as f32),
                        label,
                        &Tokens::DARK,
                        true,
                    );
                    if slot.response.has_focus() {
                        focused.push(*control);
                    }
                }
            });
        }

        assert!(
            !focused.contains(&Control::LibraryCreate),
            "a waiting control took focus: {focused:?}"
        );
        assert!(focused.contains(&Control::DrawerToggle));
        assert!(focused.contains(&Control::TrailPlaylists));
    }

    #[test]
    fn a_click_on_a_waiting_control_does_nothing() {
        let ctx = Context::default();
        let waiting = Rect::from_min_size(Pos2::new(100.0, 100.0), Vec2::new(28.0, 28.0));
        let ready = Rect::from_min_size(Pos2::new(200.0, 100.0), Vec2::new(28.0, 28.0));
        let mut clicked = (false, false);

        // A press and release need several passes to register.
        let tap = |ctx: &Context, at: Pos2, clicked: &mut (bool, bool)| {
            for event in crate::view::test_support::click_events(at) {
                run(ctx, input(vec![event]), |ui| {
                    let waiting_slot = slot(
                        ui,
                        Control::LibraryCreate,
                        Role::Button,
                        waiting,
                        "Create playlist or folder",
                        &Tokens::DARK,
                        true,
                    );
                    let ready_slot = slot(
                        ui,
                        Control::DrawerToggle,
                        Role::Button,
                        ready,
                        "Toggle",
                        &Tokens::DARK,
                        true,
                    );
                    clicked.0 = waiting_slot.response.clicked();
                    clicked.1 = ready_slot.response.clicked();
                });
            }
        };

        // Warm up so hit-testing has a previous pass to read.
        run(&ctx, input(Vec::new()), |_ui| {});
        tap(&ctx, waiting.center(), &mut clicked);
        assert!(!clicked.0, "a waiting control received a click");
        assert!(!clicked.1, "the waiting rect is not the ready one");

        tap(&ctx, ready.center(), &mut clicked);
        assert!(clicked.1, "the click harness never produced a click");
    }

    #[test]
    fn keyed_slots_on_one_control_are_distinct_per_key() {
        let ctx = Context::default();
        let keys = ["a", "b", "c", "d", "e", "f", "g", "h"];
        let mut ids = Vec::new();
        run(&ctx, input(Vec::new()), |ui| {
            for (index, key) in keys.iter().enumerate() {
                let slot = slot_keyed(
                    ui,
                    Control::TrackLike,
                    *key,
                    Role::Button,
                    cell(index as f32),
                    "Like",
                    &Tokens::DARK,
                    true,
                );
                ids.push(slot.response.id);
            }
        });
        assert_eq!(ids.len(), keys.len());
        for i in 0..ids.len() {
            for j in (i + 1)..ids.len() {
                assert!(
                    ids[i] != ids[j],
                    "keys {} and {} share an id",
                    keys[i],
                    keys[j]
                );
            }
        }
    }

    #[test]
    fn the_unit_key_reproduces_the_existing_slot_id() {
        let ctx = Context::default();
        let mut ids = (None, None, None);
        run(&ctx, input(Vec::new()), |ui| {
            let keyed = slot_keyed(
                ui,
                Control::TrackLike,
                (),
                Role::Button,
                cell(0.0),
                "Like",
                &Tokens::DARK,
                true,
            );
            let plain = slot(
                ui,
                Control::TrackLike,
                Role::Button,
                cell(1.0),
                "Like",
                &Tokens::DARK,
                true,
            );
            let raw = ui.scope_id().with(Control::TrackLike);
            ids = (Some(keyed.response.id), Some(plain.response.id), Some(raw));
        });
        let (keyed, plain, raw) = ids;
        assert_eq!(keyed, plain, "slot no longer delegates with the unit key");
        assert_eq!(keyed, raw, "the keyed id moved the existing control ids");
    }

    #[test]
    fn a_text_button_box_grows_with_its_label_and_pads_the_text() {
        let ctx = Context::default();
        let mut measured = None;
        run(&ctx, input(Vec::new()), |ui| {
            let short = text_button_size(ui, &Tokens::DARK, "Go");
            let long = text_button_size(ui, &Tokens::DARK, "Authorize");
            let bare = line(
                ui,
                "Authorize",
                text_button_font(),
                Tokens::DARK.palette.text,
                0.0,
            );
            measured = Some((short, long, bare.size().y));
        });
        let (short, long, bare) = measured.expect("the buttons measured");
        assert!(
            long.x > short.x,
            "a longer label must need a wider box: {long:?} vs {short:?}"
        );
        assert!(
            long.y > bare,
            "the box must be taller than its bare text: {long:?} vs {bare}"
        );
    }

    #[test]
    fn a_text_button_that_is_not_live_takes_no_click() {
        let ctx = Context::default();
        let rect = Rect::from_min_size(Pos2::new(100.0, 100.0), Vec2::new(120.0, 30.0));
        let mut clicked = false;

        // A press and release need several passes to register.
        let tap = |ctx: &Context, at: Pos2, clicked: &mut bool| {
            for event in crate::view::test_support::click_events(at) {
                run(ctx, input(vec![event]), |ui| {
                    let slot = text_button(
                        ui,
                        &Tokens::DARK,
                        Control::Authorize,
                        rect,
                        "Authorize",
                        false,
                    );
                    *clicked = slot.response.clicked();
                });
            }
        };

        // Warm up so hit-testing has a previous pass to read.
        run(&ctx, input(Vec::new()), |_ui| {});
        tap(&ctx, rect.center(), &mut clicked);
        assert!(!clicked, "a waiting text button received a click");
    }

    #[test]
    fn fraction_at_clamps_to_the_track() {
        assert_eq!(fraction_at(100.0, 100.0, 200.0), 0.0);
        assert_eq!(fraction_at(150.0, 100.0, 200.0), 0.25);
        assert_eq!(fraction_at(300.0, 100.0, 200.0), 1.0);
        assert_eq!(fraction_at(400.0, 100.0, 200.0), 1.0);
        assert_eq!(fraction_at(50.0, 100.0, 200.0), 0.0);
        assert_eq!(fraction_at(150.0, 100.0, 0.0), 0.0);
    }
}
