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
mod tests;
