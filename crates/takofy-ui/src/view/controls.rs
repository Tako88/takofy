//! The shared control slot: interaction, accessibility and the focus ring.
//!
//! A painted control draws itself, but every control passes through this one
//! seam first. The slot is where a waiting control stops being interactive: it
//! senses hover only, never joins the tab order, and announces itself disabled
//! with its label through `WidgetInfo`.
//!
//! The focus ring is drawn here too, because every control wants the same one
//! and only the slot knows the control has focus.

use std::sync::Arc;

use egui::emath::GuiRounding;
use egui::text::LayoutJob;
use egui::{
    Align2, Color32, FontId, Galley, Rect, Response, Role, Sense, Stroke, StrokeKind, TextFormat,
    Ui, Vec2, WidgetInfo, pos2,
};

use crate::availability::{Availability, Control, availability};
use crate::icon::{self, Icon};
use crate::layout::Sidebar;
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
pub fn slot(
    ui: &mut Ui,
    control: Control,
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
    let id = ui.scope_id().with(control);
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
    let slot = slot(ui, control, Role::Button, rect, label, tokens, live);
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
) -> Slot {
    let slot = slot(ui, control, Role::Button, rect, label, tokens, live);
    let live = slot.live;
    let border = if live && slot.hover > 0.0 {
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

/// A navigation entry: icon and label in the sidebar, icon alone in the rail.
///
/// The two forms differ only in the label and the padding, which is why they are
/// one control taking the region model rather than two controls.
pub fn nav_item(
    ui: &mut Ui,
    tokens: &Tokens,
    control: Control,
    rect: Rect,
    form: Sidebar,
    icon: Icon,
    label: &str,
) -> Slot {
    let slot = slot(ui, control, Role::Button, rect, label, tokens, true);
    if let Some(fill) = wash(ui, tokens, slot.hover) {
        ui.painter().rect_filled(rect, radius::SM, fill);
    }

    let color = foreground(tokens, slot.live, slot.hover);
    let glyph = match form {
        Sidebar::Rail => centred(ui, rect, Size::LG.side),
        Sidebar::Full => {
            let left = rect.left() + space::S3;
            Rect::from_min_size(
                pos2(left, rect.center().y - Size::LG.side / 2.0),
                Vec2::splat(Size::LG.side),
            )
            .round_to_pixels(ui.pixels_per_point())
        }
    };
    icon::paint(ui.painter(), icon, glyph, Size::LG, color);

    if form == Sidebar::Full {
        ui.painter().text(
            pos2(glyph.right() + space::S3, rect.center().y),
            Align2::LEFT_CENTER,
            label,
            FontId::new(text::BASE, fonts::sans(Weight::Medium)),
            color,
        );
    }

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

/// A chip drawn into a rect the caller chose.
pub fn chip_in(
    ui: &mut Ui,
    tokens: &Tokens,
    control: Control,
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
    let slot = slot(ui, control, Role::Button, rect, label, tokens, true);

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
    ui.painter().galley(
        rect.min + Vec2::new(extent::CHIP_PAD_X, extent::CHIP_PAD_Y),
        galley,
        color,
    );

    slot
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
    use egui::{Context, Event, Modifiers, PointerButton, Pos2, RawInput, Rect, Ui, Vec2};

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

    fn button(pos: Pos2, pressed: bool) -> Event {
        Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        }
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
                Control::NavHome,
                Role::Button,
                cell(0.0),
                "Home",
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
                Control::NavHome,
                Role::Button,
                cell(0.0),
                "Home",
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
        assert_eq!(label.as_deref(), Some("Home"));
    }

    #[test]
    fn tab_never_lands_on_a_waiting_control() {
        let ctx = Context::default();
        let controls = [
            (Control::DrawerToggle, "Toggle"),
            (Control::NavHome, "Home"),
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
            !focused.contains(&Control::NavHome),
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
            for event in [Event::PointerMoved(at), button(at, true), button(at, false)] {
                run(ctx, input(vec![event]), |ui| {
                    let waiting_slot = slot(
                        ui,
                        Control::NavHome,
                        Role::Button,
                        waiting,
                        "Home",
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
    fn fraction_at_clamps_to_the_track() {
        assert_eq!(fraction_at(100.0, 100.0, 200.0), 0.0);
        assert_eq!(fraction_at(150.0, 100.0, 200.0), 0.25);
        assert_eq!(fraction_at(300.0, 100.0, 200.0), 1.0);
        assert_eq!(fraction_at(400.0, 100.0, 200.0), 1.0);
        assert_eq!(fraction_at(50.0, 100.0, 200.0), 0.0);
        assert_eq!(fraction_at(150.0, 100.0, 0.0), 0.0);
    }
}
