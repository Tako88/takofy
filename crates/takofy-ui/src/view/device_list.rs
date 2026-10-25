//! The device list: the account's other devices, drawn in the app's menu card.
//!
//! A row is information until this window has something to hand over: then it
//! takes a press and points the cursor, so the sound can be moved to that
//! device. Nothing else visible changes — no highlight, no mark — and the hint
//! keeps saying the same thing. What the card lists is
//! [`crate::devices::Devices`]'; whether it is open is
//! [`crate::device_list::DeviceList`]'s.
//!
//! The card closes on Escape — which is consumed, so nothing behind it reads the
//! key — and on a press outside it. A second press of the button that opened it
//! closes it in [`crate::device_list::DeviceList::press`], before this draws.

use std::time::Instant;

use egui::{Id, Key, Modifiers, Order, Rect, Role, Sense, Ui, WidgetInfo, pos2, vec2};

use crate::device_list::DeviceList;
use crate::devices::{Devices, Row, kind_label};
use crate::icon::{self, Icon};
use crate::layout::device_list as geometry;
use crate::layout::menu;
use crate::menu::Reason;
use crate::player::Transport;
use crate::theme::icon::Size;
use crate::theme::{Tokens, extent, space};
use crate::view::context_menu::{key_font, measure};
use crate::view::menu_box::{self, Card};
use crate::view::text as recipe;

/// What a screen reader hears the card say.
const LIST_LABEL: &str = "Devices";

/// What the view reads and what it may change.
pub struct Frame<'a> {
    pub list: &'a mut DeviceList,
    pub devices: &'a Devices,
    /// This window's own transport, which decides whether a press on a row can
    /// move the sound.
    pub local: &'a Transport,
    /// Whether that transport has a current queue entry, which is the state a
    /// press would hand over.
    pub has_row: bool,
    /// The clock the notes and the caption are read at.
    pub now: Instant,
    pub tokens: &'a Tokens,
}

/// What the card asked for this frame.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Outcome {
    /// The button the keyboard goes back to when the card closed on Escape.
    pub refocus: Option<Id>,
    /// The device a press picked, when the card offered the press.
    pub pick: Option<String>,
}

/// Draws the open card, reporting the button the keyboard goes back to when it
/// closed on Escape, and the device a press picked.
pub fn show(ui: &mut Ui, frame: Frame<'_>) -> Outcome {
    let Frame {
        list,
        devices,
        local,
        has_row,
        now,
        tokens,
    } = frame;
    let mut outcome = Outcome::default();
    if !list.is_open() {
        return outcome;
    }
    let Some(anchor) = list.anchor() else {
        return outcome;
    };

    // The list never scrolls: the holder and the first names win, and the rest
    // are found in Spotify until this card can do more than read.
    let rows: Vec<Row> = devices
        .rows(local, has_row)
        .into_iter()
        .take(extent::SUBMENU_ROWS)
        .collect();
    let notes = devices.notes(now);
    // The card is measured from its longest row, its muted lines included: a
    // note is a line of text on a row's own rhythm, so it is measured like one.
    let mut widths: Vec<menu::RowWidth> = rows
        .iter()
        .map(|row| menu::RowWidth {
            label: measure(ui, &row.device.name, recipe::track_secondary(), tokens)
                + Size::SM.side
                + space::S3,
            key: Some(measure(ui, kind_label(row.device.kind), key_font(), tokens)),
            chevron: false,
        })
        .collect();
    widths.extend(notes.iter().map(|note| menu::RowWidth {
        label: measure(ui, note, recipe::track_secondary(), tokens),
        key: None,
        chevron: false,
    }));
    let rect = menu::place(
        anchor,
        vec2(
            menu::width(&widths),
            menu::rows_height(rows.len() + notes.len()),
        ),
        ui.ctx().content_rect(),
    );

    let area_id = ui.make_persistent_id("device-list");
    let surface_id = area_id.with("surface");
    // The frame the card opens on is its own: the click that is the button is
    // also a click outside the card.
    let now = ui.input(|input| input.time);
    let fresh = list.just_opened(now);
    let area = egui::Area::new(area_id)
        .order(Order::Foreground)
        .fixed_pos(rect.min)
        .constrain(false)
        .fade_in(false)
        .show(ui.ctx(), |ui| {
            // The card is painted at an absolute rect rather than laid out, so
            // the area has to be told how big it is: without this its own rect
            // is empty and every click on the card reads as a click elsewhere.
            ui.expand_to_include_rect(rect);
            let card = Card {
                rect,
                id: surface_id,
                // A press picks a device; the card's own click is what closes it
                // on a press anywhere else, so it holds no keyboard and is not
                // a focus stop.
                sense: Sense::CLICK,
                tokens,
            };
            let (surface, picked) = menu_box::card(ui, card, |ui| {
                draw_rows(ui, &rows, &notes, rect, area_id, tokens)
            });
            surface.widget_info(|| WidgetInfo::labeled(Role::List, true, LIST_LABEL));
            picked
        });

    list.mark_drawn(now);
    outcome.pick = area.inner;
    if outcome.pick.is_some() {
        list.close(Reason::Pick);
        return outcome;
    }
    if fresh {
        return outcome;
    }
    let escape = ui.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Escape));
    if escape {
        outcome.refocus = list.close(Reason::Escape);
        return outcome;
    }
    if area.response.clicked_elsewhere() {
        list.close(Reason::Outside);
    }
    outcome
}

/// Draws the device rows, then the muted lines under them, reporting the device
/// a press picked.
fn draw_rows(
    ui: &mut Ui,
    rows: &[Row],
    notes: &[String],
    rect: Rect,
    id: Id,
    tokens: &Tokens,
) -> Option<String> {
    let mut picked = None;
    for (index, row) in rows.iter().enumerate() {
        let row_rect = menu::row_rect(rect, index);
        // A row that can move the sound takes the press; one that cannot stays
        // hover, so it still reads as information.
        let sense = if row.pickable {
            Sense::click()
        } else {
            Sense::hover()
        };
        let hit = ui.interact(row_rect, id.with(("device", index)), sense);
        let spoken = spoken(row);
        hit.widget_info(|| WidgetInfo::labeled(Role::ListItem, true, spoken.clone()));
        if row.pickable && hit.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if hit.clicked() {
            picked = Some(row.device.id.clone());
        }

        let kind = kind_label(row.device.kind);
        let parts = geometry::row_parts(row_rect, measure(ui, kind, key_font(), tokens));
        let glyph = if row.holder {
            tokens.palette.accent
        } else {
            tokens.palette.text_subtle
        };
        icon::paint(ui.painter(), Icon::Devices, parts.icon, Size::SM, glyph);

        let color = if row.holder {
            tokens.palette.text
        } else {
            tokens.palette.text_muted
        };
        if parts.label.width() > 0.0 {
            paint_line(ui, &row.device.name, parts.label, color);
        }
        if parts.trailing.width() > 0.0 {
            paint_key(ui, kind, parts.trailing, tokens.palette.text_subtle);
        }
    }

    for (index, note) in notes.iter().enumerate() {
        let row_rect = menu::row_rect(rect, rows.len() + index);
        let parts = menu::command_parts(row_rect, None, false);
        paint_line(ui, note, parts.label, tokens.palette.text_muted);
    }
    picked
}

/// What a row says to a screen reader: its name, its kind, and whether the sound
/// is coming out of it.
fn spoken(row: &Row) -> String {
    let playing = if row.holder { ", playing" } else { "" };
    format!(
        "{}, {}{playing}",
        row.device.name,
        kind_label(row.device.kind)
    )
}

/// One row's name, centred on its slot and elided to the slot's width.
fn paint_line(ui: &Ui, label: &str, slot: Rect, color: egui::Color32) {
    let galley = recipe::line(
        ui,
        label,
        recipe::track_secondary(),
        color,
        0.0,
        slot.width(),
    );
    ui.painter().galley(
        pos2(slot.left(), slot.center().y - galley.size().y / 2.0),
        galley,
        color,
    );
}

/// One row's kind word, drawn like a command row's key hint.
fn paint_key(ui: &Ui, label: &str, slot: Rect, color: egui::Color32) {
    let galley = recipe::line(ui, label, key_font(), color, 0.0, slot.width());
    ui.painter().galley(
        pos2(
            slot.right() - galley.size().x,
            slot.center().y - galley.size().y / 2.0,
        ),
        galley,
        color,
    );
}

#[cfg(test)]
mod tests {
    use egui::{Context, Event, Pos2, RawInput};

    use takofy_model::DeviceKind;

    use super::*;
    use crate::menu::Anchor;
    use crate::message::{DeviceUpdate, Update};
    use crate::theme::{fonts, style};
    use takofy_model::Device;

    fn context() -> Context {
        let ctx = Context::default();
        ctx.set_fonts(fonts::definitions());
        ctx.set_global_style(style(&Tokens::DARK));
        ctx.enable_accesskit();
        ctx
    }

    fn screen() -> Rect {
        Rect::from_min_size(Pos2::ZERO, vec2(1200.0, 760.0))
    }

    fn device(id: &str, name: &str, kind: DeviceKind) -> Device {
        Device {
            id: id.to_owned(),
            name: name.to_owned(),
            kind,
        }
    }

    /// A watch that has answered with the phone holding the sound and a laptop
    /// beside it.
    fn watched() -> Devices {
        let mut devices = Devices::default();
        let now = Instant::now();
        assert_eq!(
            devices.watch_request(true),
            Some(crate::message::Command::WatchDevices)
        );
        devices.apply(
            &Update::Devices(DeviceUpdate::Roster {
                devices: vec![
                    device("a", "Phone", DeviceKind::Phone),
                    device("b", "Laptop", DeviceKind::Computer),
                ],
                holder: Some("a".to_owned()),
                held_here: false,
            }),
            now,
        );
        devices
    }

    fn button() -> Rect {
        Rect::from_min_size(pos2(600.0, 700.0), vec2(28.0, 28.0))
    }

    fn anchor() -> Anchor {
        Anchor::Button(button())
    }

    fn input(events: Vec<Event>) -> RawInput {
        RawInput {
            screen_rect: Some(screen()),
            events,
            ..Default::default()
        }
    }

    /// One frame of the card, and what it reported.
    fn pass(
        ctx: &Context,
        list: &mut DeviceList,
        devices: &Devices,
        events: Vec<Event>,
    ) -> (Outcome, egui::FullOutput) {
        let tokens = Tokens::DARK;
        let mut outcome = Outcome::default();
        let mut output = ctx.run_ui(input(events), |ui| {
            outcome = show(
                ui,
                Frame {
                    list,
                    devices,
                    local: &idle(),
                    has_row: true,
                    now: Instant::now(),
                    tokens: &tokens,
                },
            );
        });
        output.textures_delta.clear();
        (outcome, output)
    }

    /// The second frame the card is drawn in: the first one only sizes the area,
    /// which egui draws invisible.
    fn settled(ctx: &Context, list: &mut DeviceList, devices: &Devices) -> egui::FullOutput {
        let _ = pass(ctx, list, devices, Vec::new());
        pass(ctx, list, devices, Vec::new()).1
    }

    /// The settled frame drawn with the pointer resting on `at`.
    fn hover_at(
        ctx: &Context,
        list: &mut DeviceList,
        devices: &Devices,
        at: Pos2,
    ) -> egui::FullOutput {
        let _ = pass(ctx, list, devices, Vec::new());
        pass(ctx, list, devices, vec![Event::PointerMoved(at)]).1
    }

    fn opened() -> (DeviceList, Id) {
        let mut list = DeviceList::default();
        let opener = Id::unique("devices");
        assert!(list.press(anchor(), opener), "the list opened");
        (list, opener)
    }

    /// A window with nothing playing: no row can act.
    fn idle() -> Transport {
        Transport::default()
    }

    /// The roster with nobody holding the sound, or with this window holding it:
    /// the rows and notes are the same either way, and only the press differs.
    fn roster(held_here: bool) -> Devices {
        let mut devices = Devices::default();
        assert_eq!(
            devices.watch_request(true),
            Some(crate::message::Command::WatchDevices)
        );
        devices.apply(
            &Update::Devices(DeviceUpdate::Roster {
                devices: vec![
                    device("a", "Phone", DeviceKind::Phone),
                    device("b", "Laptop", DeviceKind::Computer),
                ],
                holder: None,
                held_here,
            }),
            Instant::now(),
        );
        devices
    }

    /// Where a painted word sits, by its own galley.
    fn painted_text(output: &egui::FullOutput, text: &str) -> Rect {
        output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::Text(shape) if shape.galley.text() == text => {
                    Some(Rect::from_min_size(shape.pos, shape.galley.size()))
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("{text} is not painted"))
    }

    /// One click over the passes egui's hit test needs.
    fn click(ctx: &Context, list: &mut DeviceList, devices: &Devices, at: Pos2) -> Vec<Outcome> {
        let _ = pass(ctx, list, devices, Vec::new());
        let mut reported = vec![pass(ctx, list, devices, vec![Event::PointerMoved(at)]).0];
        for event in crate::view::test_support::click_events(at) {
            reported.push(pass(ctx, list, devices, vec![event]).0);
        }
        reported
    }

    #[test]
    fn a_click_on_a_row_leaves_the_card_open_and_asks_for_nothing() {
        let ctx = context();
        // Nobody holds the sound and this window plays nothing, so the rows are
        // still information.
        let devices = roster(false);
        let (mut list, _) = opened();
        let output = settled(&ctx, &mut list, &devices);
        let at = painted_text(&output, "Laptop").center();

        let reported = click(&ctx, &mut list, &devices, at);

        assert!(
            reported.iter().all(|outcome| outcome.pick.is_none()),
            "{reported:?}"
        );
        assert!(
            reported.iter().all(|outcome| outcome.refocus.is_none()),
            "{reported:?}"
        );
        assert!(list.is_open(), "a click on a row closed the card");
    }

    #[test]
    fn a_click_on_an_unpickable_row_does_nothing_and_keeps_the_card_open() {
        let ctx = context();
        let devices = roster(false);
        let (mut list, _) = opened();
        let output = settled(&ctx, &mut list, &devices);
        let at = painted_text(&output, "Laptop").center();
        assert!(
            devices.rows(&idle(), true).iter().all(|row| !row.pickable),
            "an inert card offered a press"
        );

        let reported = click(&ctx, &mut list, &devices, at);

        assert!(
            reported.iter().all(|outcome| outcome.pick.is_none()),
            "{reported:?}"
        );
        assert!(list.is_open(), "a click on an inert row closed the card");
    }

    #[test]
    fn a_click_on_a_pickable_row_picks_that_device_and_closes_the_card() {
        let ctx = context();
        let devices = roster(true);
        let (mut list, _) = opened();
        let output = settled(&ctx, &mut list, &devices);
        let at = painted_text(&output, "Laptop").center();

        let reported = click(&ctx, &mut list, &devices, at);

        let picked: Vec<&str> = reported
            .iter()
            .filter_map(|outcome| outcome.pick.as_deref())
            .collect();
        assert_eq!(picked, vec!["b"], "{reported:?}");
        assert!(
            reported.iter().all(|outcome| outcome.refocus.is_none()),
            "a pick handed the keyboard somewhere: {reported:?}"
        );
        assert!(!list.is_open(), "a pick left the card up");
    }

    #[test]
    fn pickable_rows_change_the_cursor_and_nothing_else() {
        let ctx = context();
        let (mut list, _) = opened();
        let output = settled(&ctx, &mut list, &roster(false));
        let at = painted_text(&output, "Laptop").center();

        let (mut inert, _) = opened();
        let rest = hover_at(&ctx, &mut inert, &roster(false), at);
        assert_ne!(
            rest.platform_output.cursor_icon,
            egui::CursorIcon::PointingHand,
            "an inert row pointed the hand"
        );

        let (mut live, _) = opened();
        let over = hover_at(&ctx, &mut live, &roster(true), at);
        assert_eq!(
            over.platform_output.cursor_icon,
            egui::CursorIcon::PointingHand,
            "a pickable row did not point the hand"
        );

        assert_eq!(
            rest.shapes, over.shapes,
            "a pickable row painted something an inert one did not"
        );
    }

    #[test]
    fn a_click_on_a_muted_line_leaves_the_card_open() {
        let ctx = context();
        let devices = watched();
        let (mut list, _) = opened();
        let output = settled(&ctx, &mut list, &devices);
        let now = Instant::now();
        let hint = devices
            .notes(now)
            .last()
            .cloned()
            .expect("the card always ends with its hint");
        let at = painted_text(&output, &hint).center();

        let closed = click(&ctx, &mut list, &devices, at);

        assert!(
            closed.iter().all(|outcome| outcome.pick.is_none()),
            "{closed:?}"
        );
        assert!(list.is_open(), "a click on a muted line closed the card");
    }

    #[test]
    fn escape_closes_the_card_and_returns_the_button() {
        let ctx = context();
        let devices = watched();
        let (mut list, opener) = opened();
        let _ = pass(&ctx, &mut list, &devices, Vec::new());

        let key = |pressed| Event::Key {
            key: Key::Escape,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: Modifiers::NONE,
        };
        let closed = pass(&ctx, &mut list, &devices, vec![key(true)]).0;
        let _ = pass(&ctx, &mut list, &devices, vec![key(false)]);

        assert_eq!(
            closed.refocus,
            Some(opener),
            "Escape did not hand the keyboard back"
        );
        assert!(!list.is_open());
    }

    #[test]
    fn a_press_outside_closes_the_card() {
        let ctx = context();
        let devices = watched();
        let (mut list, _) = opened();
        let closed = click(&ctx, &mut list, &devices, pos2(10.0, 10.0));

        assert!(
            closed.iter().all(|outcome| outcome.pick.is_none()),
            "{closed:?}"
        );
        assert!(!list.is_open(), "a press outside left the card up");
    }

    #[test]
    fn a_closed_list_draws_nothing() {
        let ctx = context();
        let devices = watched();
        let mut list = DeviceList::default();
        let (closed, output) = pass(&ctx, &mut list, &devices, Vec::new());
        assert_eq!(closed, Outcome::default());
        assert!(output.shapes.is_empty(), "a closed card painted something");
    }

    #[test]
    fn a_long_roster_shows_the_holder_and_then_the_first_names() {
        let ctx = context();
        let mut devices = Devices::default();
        let now = Instant::now();
        devices.watch_request(true);
        let roster: Vec<Device> = (1..=12)
            .map(|index| {
                device(
                    &format!("d{index}"),
                    &format!("Device {index:02}"),
                    DeviceKind::Computer,
                )
            })
            .collect();
        devices.apply(
            &Update::Devices(DeviceUpdate::Roster {
                devices: roster,
                holder: Some("d7".to_owned()),
                held_here: false,
            }),
            now,
        );
        let (mut list, _) = opened();
        let output = settled(&ctx, &mut list, &devices);
        let update = output
            .platform_output
            .accesskit_update
            .as_ref()
            .expect("the card announces itself");

        let mut rows: Vec<String> = update
            .nodes
            .iter()
            .filter(|(_, node)| node.role() == Role::ListItem)
            .filter_map(|(_, node)| node.label().map(str::to_owned))
            .collect();
        rows.sort();
        // The holder first, then by name: the seventh device leads and the ones
        // past the eighth are not on the card at all.
        let mut expected: Vec<String> = vec!["Device 07, Computer, playing".to_owned()];
        expected.extend(
            ["01", "02", "03", "04", "05", "06", "08"]
                .map(|name| format!("Device {name}, Computer")),
        );
        expected.sort();
        assert_eq!(rows, expected);
    }

    #[test]
    fn the_card_announces_a_list_and_its_rows_as_information() {
        let ctx = context();
        let devices = watched();
        let (mut list, _) = opened();
        let output = settled(&ctx, &mut list, &devices);
        let update = output
            .platform_output
            .accesskit_update
            .as_ref()
            .expect("the card announces itself");

        let role = |label: &str| {
            update
                .nodes
                .iter()
                .find(|(_, node)| node.label() == Some(label))
                .map(|(_, node)| node.role())
        };
        assert_eq!(role(LIST_LABEL), Some(Role::List));
        assert_eq!(role("Phone, Phone, playing"), Some(Role::ListItem));
        assert_eq!(role("Laptop, Computer"), Some(Role::ListItem));
    }

    #[test]
    fn everything_the_card_paints_is_inside_it() {
        let ctx = context();
        let devices = watched();
        let (mut list, _) = opened();
        let output = settled(&ctx, &mut list, &devices);

        // The card's own hairline is the only stroked rect the pass paints.
        let border: Vec<Rect> = output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Rect(shape)
                    if shape.stroke.color == Tokens::DARK.palette.border_strong =>
                {
                    Some(shape.rect)
                }
                _ => None,
            })
            .collect();
        assert_eq!(border.len(), 1, "the card paints one hairline");
        let card = border[0];
        assert!(screen().contains_rect(card), "the card left the window");
        // Above the button that opened it, because below would clip.
        assert!(card.bottom() < button().top(), "{card:?}");

        let words: Vec<(String, Rect)> = output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Text(shape) => Some((
                    shape.galley.text().to_owned(),
                    Rect::from_min_size(shape.pos, shape.galley.size()),
                )),
                _ => None,
            })
            .collect();
        assert_eq!(words.len(), 5, "two names, two kinds and the hint");
        for (word, at) in words {
            assert!(card.contains_rect(at), "{word} at {at:?} left {card:?}");
        }
    }

    /// The hint is the only line that says what to do instead, so it is painted
    /// whole: a card that elides it tells the reader nothing.
    #[test]
    fn the_hint_is_painted_whole() {
        let ctx = context();
        let devices = watched();
        let (mut list, _) = opened();
        let output = settled(&ctx, &mut list, &devices);

        let painted: Vec<String> = output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Text(shape) => Some(shape.galley.text().to_owned()),
                _ => None,
            })
            .collect();
        let hint = painted
            .iter()
            .find(|text| text.starts_with("Switch devices"))
            .expect("the card paints the hint");
        assert_eq!(hint, crate::devices::SWITCH_HINT, "the hint was elided");
        assert!(!hint.contains('\u{2026}'), "the hint was elided: {hint:?}");
    }

    /// The empty-roster line is short enough to be painted whole too.
    #[test]
    fn the_empty_note_is_painted_whole() {
        let ctx = context();
        let (mut list, _) = opened();
        let mut devices = Devices::default();
        assert_eq!(
            devices.watch_request(true),
            Some(crate::message::Command::WatchDevices)
        );
        devices.apply(
            &Update::Devices(DeviceUpdate::Roster {
                devices: Vec::new(),
                holder: None,
                held_here: false,
            }),
            Instant::now(),
        );
        let output = settled(&ctx, &mut list, &devices);

        let painted: Vec<String> = output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Text(shape) => Some(shape.galley.text().to_owned()),
                _ => None,
            })
            .collect();
        assert!(
            painted
                .iter()
                .any(|text| text == crate::devices::NO_DEVICES),
            "{painted:?}"
        );
    }
}
