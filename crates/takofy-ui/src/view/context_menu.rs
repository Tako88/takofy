//! The context menu: a track row's commands, drawn in the app's one menu box.
//!
//! The card and the two lit tiers are [`super::menu_box`]'s; this view owns the
//! command-row grammar — a label, an optional key hint and an optional chevron —
//! and the keyboard. The state is [`crate::menu::Menu`]: the view reads what it
//! decided and reports what the user chose, so the shell applies a choice only
//! when it has the frame's command to spare.
//!
//! The card is the menu's one focus stop. It takes the keyboard when the menu
//! opens, locks Tab, the arrows and Escape while it holds it, and hands the
//! keyboard back to the opener when it closes — so the focus never falls to the
//! page and the user never has to find their way back.

use egui::{
    Event, EventFilter, FontId, Id, Key, Modifiers, Order, Rect, Sense, Stroke, StrokeKind, Ui,
    WidgetInfo, pos2, vec2,
};

use crate::icon::{self, Icon};
use crate::layout::menu as geometry;
use crate::menu::{
    Anchor, Closed, Cut, Device, Door, Item, Kind, Menu, Reason, Request, Step, Tier,
};
use crate::theme::icon::Size;
use crate::theme::{MonoWeight, Tokens, Weight, fonts, radius, space, stroke, text, tracking};
use crate::verbs;

use super::controls::Slot;
use super::keys;
use super::menu_box::{self, Card, clicked_on, paint_highlight, paint_marker, row_corners};
use super::text as recipe;

/// What the keyboard may do while the menu holds it.
///
/// Without this egui turns Tab, the arrows and Escape into focus moves before any
/// widget runs, so the menu would lose the keyboard to the page on the first Tab
/// and Escape would clear its focus instead of closing it.
const FILTER: EventFilter = EventFilter {
    tab: true,
    horizontal_arrows: true,
    vertical_arrows: true,
    escape: true,
};

/// What the view needs from the frame it is drawn in.
pub struct Frame<'a> {
    pub menu: &'a mut Menu,
    /// Whether the frame's command is still unclaimed, so a choice can be
    /// applied. A choice made while another region took the frame's command is
    /// not offered, and its gesture is spent: the user must make it again.
    pub free: bool,
    pub tokens: &'a Tokens,
}

/// The request a page posted for this frame, read once so it cannot outlive it.
pub fn take_request(ctx: &egui::Context) -> Option<Request> {
    ctx.data_mut(|data| {
        let request = data.get_temp::<Request>(request_id());
        data.remove::<Request>(request_id());
        request
    })
}

/// Posts the request the shell opens the menu on this frame.
///
/// It is one frame's value, taken by the shell after the page has drawn, so a
/// page that does not draw posts nothing.
pub fn post(ctx: &egui::Context, request: Request) {
    ctx.data_mut(|data| data.insert_temp(request_id(), request));
}

/// Whether the open menu holds the keyboard, which a region reads before it
/// takes `Del`: a consumed event is gone, and the menu's own `Del` would never
/// fire.
pub fn is_open(ctx: &egui::Context) -> bool {
    open_opener(ctx).is_some()
}

/// Posts the cut a list read on `Del` for the shell to apply this frame.
pub fn post_cut(ctx: &egui::Context, cut: Cut) {
    ctx.data_mut(|data| data.insert_temp(cut_id(), cut));
}

/// The cut a list posted for this frame, read once so it cannot outlive it.
pub fn take_cut(ctx: &egui::Context) -> Option<Cut> {
    ctx.data_mut(|data| {
        let cut = data.get_temp::<Cut>(cut_id());
        data.remove::<Cut>(cut_id());
        cut
    })
}

/// Takes this frame's Delete presses and reports whether one was a fresh press.
///
/// The auto-repeats behind a held key are consumed with it, so a held `Del`
/// removes once, and a modified `Del` is left to whoever else may want it.
pub fn take_delete(ui: &Ui) -> bool {
    let mut owned = false;
    let mut fresh = false;
    ui.input(|input| {
        for event in &input.events {
            if let Some(repeat) = delete_key(event) {
                owned = true;
                fresh |= !repeat;
            }
        }
    });
    if !owned {
        return false;
    }
    ui.input_mut(|input| input.events.retain(|event| delete_key(event).is_none()));
    fresh
}

/// `Some(repeat)` when this event is a Delete the lists own.
///
/// `consume_key` would admit a `Del` held with a modifier, so the modifiers are
/// read here instead.
fn delete_key(event: &Event) -> Option<bool> {
    match event {
        Event::Key {
            key: Key::Delete,
            pressed: true,
            repeat,
            modifiers,
            ..
        } if modifiers.is_none() => Some(*repeat),
        _ => None,
    }
}

/// The widget the open menu was opened from, if one is open.
///
/// A row draws before the menu does, so this is how the `⋯` that opened it
/// learns that its menu is up.
pub fn open_opener(ctx: &egui::Context) -> Option<Id> {
    ctx.data(|data| data.get_temp::<Id>(open_opener_id()))
}

/// The id a posted request lives under.
fn request_id() -> Id {
    Id::unique("context-menu-request")
}

/// The id the open menu's opener lives under.
fn open_opener_id() -> Id {
    Id::unique("context-menu-opener")
}

/// The id a posted cut lives under.
fn cut_id() -> Id {
    Id::unique("context-menu-cut")
}

/// Records — or forgets — the widget whose menu is up.
pub(super) fn note_open(ctx: &egui::Context, opener: Option<Id>) {
    ctx.data_mut(|data| match opener {
        Some(opener) => {
            data.insert_temp(open_opener_id(), opener);
        }
        None => data.remove::<Id>(open_opener_id()),
    });
}

/// Draws the open menu, and reports what closed it.
pub fn show(ui: &mut Ui, frame: Frame<'_>) -> Option<Closed> {
    let Frame { menu, free, tokens } = frame;
    if !menu.is_open() {
        // The button that says the menu is open learns it from here, so a closed
        // menu has to forget it.
        note_open(ui.ctx(), None);
        return None;
    }
    let anchor = menu.anchor()?;
    note_open(ui.ctx(), menu.opener());

    // The frame the menu opens is its own: the Enter that opened it must not
    // also fire the row it lights, and the click that opened it must not
    // dismiss it.
    // A frame is not a pass: egui can run the opening frame twice, so the input
    // that opened the menu is recognised by the clock, not by a flag that the
    // first pass would clear before the second pass read it.
    let now = ui.input(|input| input.time);
    let fresh = menu.just_opened(now);

    let widths: Vec<geometry::RowWidth> = menu
        .items()
        .iter()
        .map(|item| geometry::RowWidth {
            label: measure(ui, &item.label, recipe::track_secondary(), tokens),
            key: item.key.map(|key| measure(ui, key, key_font(), tokens)),
            chevron: item.submenu,
        })
        .collect();
    let size = vec2(
        geometry::width(&widths),
        geometry::rows_height(menu.items().len()),
    );
    let rect = geometry::place(anchor, size, ui.ctx().content_rect());

    let area_id = ui.make_persistent_id("context-menu");
    let surface_id = area_id.with("surface");
    // The pointer reached a row, so the pointer owns the tier from this frame on.
    // A pointer merely resting where the menu opens is not the last device: a
    // keyboard open would otherwise lose its lit row to a pointer that never moved.
    let hovered = pointer_row(ui, rect, menu.items().len());
    if ui.input(|input| input.pointer.is_moving()) {
        menu.pointer_over(hovered);
    }
    // A submenu follows the pointer along the rows: a row that opens a list
    // opens it, and any other row closes the one already up. This one is not
    // gated on the pointer moving, because entering the window has no delta and
    // the pointer would then have to wiggle before the list appeared.
    if let Some(row) = hovered {
        if menu.items().get(row).is_some_and(|item| item.submenu) {
            menu.open_submenu(row);
        } else if menu.submenu().is_some_and(|open| open != row) {
            menu.close_submenu();
        }
    }

    // Enter is consumed before anything draws, so the submenu's field never sees
    // a singleline edit's Enter and never gives up the keyboard on the key that
    // chooses a list. A repeat reaches neither the `⋯` nor the drawer, and only
    // a fresh press chooses.
    let enter = ui.input_mut(consume_enter);

    let mut chosen = None;
    let mut target = None;
    let mut sizing = false;
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
            sizing = ui.is_sizing_pass();
            if !sizing {
                // A sizing pass cannot take focus, so the keyboard is asked for
                // on every frame that can hold it: a context menu is modal until
                // it is dismissed, and a focus taken elsewhere must not leave it
                // drawing keys nobody will read.
                // The submenu's field is the keyboard's home while a list is
                // open, so the user types the moment it appears; otherwise the
                // card holds it.
                let focus = match menu.submenu() {
                    Some(row) => area_id.with(("submenu", row)).with("field"),
                    None => surface_id,
                };
                ui.memory_mut(|memory| memory.request_focus(focus));
            }
            let card = Card {
                rect,
                id: surface_id,
                // The card is the menu's one focus stop; the rows are not.
                sense: Sense::click(),
                tokens,
            };
            // The card clips to its own rect, so the clip is saved before it and
            // restored for the submenu: a card drawn after it would otherwise be
            // cut to the card's box.
            let clip = ui.clip_rect();
            let (surface, clicked) =
                menu_box::card(ui, card, |ui| draw_rows(ui, menu, rect, area_id, tokens));
            surface.widget_info(|| {
                WidgetInfo::labeled(
                    egui::Role::Menu,
                    true,
                    verbs::menu_label(menu.targets(), menu.listing()),
                )
            });
            chosen = clicked;
            if !sizing {
                ui.set_clip_rect(clip);
                target = draw_submenu(ui, menu, rect, area_id, tokens);
            }
        });

    if !sizing {
        menu.mark_drawn(now);
    }

    // A list chosen in the submenu closes the whole menu, so it is reported
    // before the parent's own rows are read.
    if let Some(closed) = target {
        focus_back(ui, closed);
        return Some(closed);
    }

    if !fresh {
        // Escape is read and consumed before anything else, so the focus path
        // below never sees it and the drawer - which reads it without consuming
        // - never gets one the menu closed on.
        let (escape, tab, down, up, first, last, left) = ui.input_mut(|input| {
            (
                input.consume_key(Modifiers::NONE, Key::Escape),
                input.consume_key(Modifiers::NONE, Key::Tab),
                input.consume_key(Modifiers::NONE, Key::ArrowDown),
                input.consume_key(Modifiers::NONE, Key::ArrowUp),
                input.consume_key(Modifiers::NONE, Key::Home),
                input.consume_key(Modifiers::NONE, Key::End),
                input.consume_key(Modifiers::NONE, Key::ArrowLeft),
            )
        });
        // The submenu takes the keys while it is up: Escape and Left step back to
        // the parent rather than closing the whole menu, and the arrows walk its
        // own rows. Tab still closes everything, as it does above.
        let submenu = menu.submenu().is_some();
        if escape {
            if submenu {
                menu.close_submenu();
            } else {
                return close(ui, menu, Reason::Escape);
            }
        }
        if tab {
            return close(ui, menu, Reason::Tab);
        }
        if left && submenu {
            menu.close_submenu();
        }
        let step = if down {
            Some(Step::Down)
        } else if up {
            Some(Step::Up)
        } else if first {
            Some(Step::First)
        } else if last {
            Some(Step::Last)
        } else {
            None
        };
        if let Some(step) = step {
            if submenu {
                menu.step_submenu(step);
            } else {
                menu.step(step);
            }
        }
        // Del is the destructive row's key, so the hint tells the truth while
        // the menu is open: a menu without one takes the key and does nothing.
        if free
            && take_delete(ui)
            && let Some(closed) = menu.choose_destructive()
        {
            focus_back(ui, closed);
            return Some(closed);
        }
        if enter
            && free
            && let Some(closed) = choose(menu)
        {
            focus_back(ui, closed);
            return Some(closed);
        }
    }

    // A click anywhere else dismisses the menu, but not on the frame a request
    // opened it: that click was the door.
    if !fresh && area.response.clicked_elsewhere() {
        return close(ui, menu, Reason::Outside);
    }
    // A press or a click on the card surrenders the card's own focus: egui
    // surrenders on press, and the row under the pointer is the widget being
    // clicked, not the card. The menu takes the keyboard back for that gesture,
    // so the click it belongs to still lands on the row.
    let engaged = ui
        .ctx()
        .pointer_hover_pos()
        .is_some_and(|pos| rect.contains(pos))
        && ui.input(|input| input.pointer.any_down() || input.pointer.any_click());
    if engaged && !ui.memory(|memory| memory.has_focus(surface_id)) {
        ui.memory_mut(|memory| memory.request_focus(surface_id));
    }
    ui.memory_mut(|memory| memory.set_focus_lock_filter(surface_id, FILTER));

    if free
        && let Some(row) = chosen
        && let Some(closed) = menu.choose(row)
    {
        focus_back(ui, closed);
        return Some(closed);
    }
    None
}

/// Chooses what Enter would fire: the submenu's lit list when one is up, the
/// parent's lit row otherwise.
fn choose(menu: &mut Menu) -> Option<Closed> {
    if menu.submenu().is_some() {
        return menu.choose_submenu_lit();
    }
    // Enter on the row that opens a list opens the list: the list is what Enter
    // would fire if it fired anything, and a row that did nothing under Enter
    // would be the one row of a menu the keyboard could not reach.
    if let Some(row) = menu.lit()
        && menu.items().get(row).is_some_and(|item| item.submenu)
    {
        menu.open_submenu(row);
        return None;
    }
    menu.choose_lit()
}

/// Consumes this frame's Enter presses and reports whether one was a fresh press.
///
/// egui counts a held key's auto-repeat as a press, so a held Enter would fire a
/// row; the event itself is read instead.
fn consume_enter(input: &mut egui::InputState) -> bool {
    let mut fresh = false;
    input.events.retain(|event| {
        let Event::Key {
            key: Key::Enter,
            pressed: true,
            repeat,
            ..
        } = event
        else {
            return true;
        };
        fresh |= !repeat;
        false
    });
    fresh
}

/// The door a right-click on `rect` opens, or `None` when it is not one.
///
/// The pointer is read rather than the widget's own `secondary_clicked()`,
/// because some rows interact later and win the hit test: a right-click over a
/// link cell is still the row's. The rect test is the layer and clip test, so a
/// right-click under another layer, or outside the scroll's clip, opens nothing.
pub fn pointer_door(ui: &Ui, rect: Rect, opener: Id, anchor: Rect) -> Option<Door> {
    let secondary = ui.input(|input| input.pointer.secondary_clicked());
    let over = ui
        .ctx()
        .rect_contains_pointer(ui.layer_id(), rect.intersect(ui.clip_rect()));
    // At the cursor, not at the `⋯`: the click named a place. Falls back to the
    // `⋯` when the pointer has already gone.
    let at = ui
        .input(|input| input.pointer.interact_pos())
        .map_or(Anchor::Button(anchor), Anchor::Pointer);
    (secondary && over).then_some(Door {
        anchor: at,
        opener,
        device: Device::Pointer,
    })
}

/// Takes this frame's context-menu key, the menu key or Shift+F10, and says
/// whether it was pressed.
///
/// The caller decides whether a row holds the keyboard, because only it knows
/// which id the keyboard is on.
pub fn menu_key_pressed(ui: &Ui) -> bool {
    ui.input_mut(|input| {
        // Shift is required for F10, so a plain F10 is not this door.
        input.consume_key(Modifiers::SHIFT, Key::F10)
            || input.consume_key(Modifiers::NONE, Key::Menu)
    })
}

/// The door a press on `slot` opens, or `None` when the press is not one.
///
/// A press on the `⋯` whose own menu is up closes it rather than posting a fresh
/// request, and a held key's repeats are not presses of the user's. A press also
/// takes the keyboard, so Enter and the focus ring stay on the button.
pub fn button_door(ui: &Ui, slot: &Slot, rect: Rect, id: Id) -> Option<Door> {
    let clicked = slot.response.clicked();
    if clicked {
        slot.response.request_focus();
    }
    // A press on a focused `⋯` is also Enter or Space, which egui reports as a
    // click without a pointer, so the device decides what the tiers mean.
    let pointer = ui.input(|input| input.pointer.button_clicked(egui::PointerButton::Primary));
    let device = if pointer {
        Device::Pointer
    } else {
        Device::Keyboard
    };
    let toggle = clicked && pointer && open_opener(ui.ctx()) == Some(id);
    let held = clicked && !pointer && held_repeat(ui);
    (clicked && !toggle && !held).then_some(Door {
        anchor: Anchor::Button(rect),
        opener: id,
        device,
    })
}

/// Consumes this frame's Enter and Space repeats and says whether they were all
/// a held key's.
///
/// egui reports a focused button's Enter or Space as a click and counts the
/// repeats too, so a held key would reopen the menu it just closed.
pub fn held_repeat(ui: &Ui) -> bool {
    let mut fresh = false;
    let mut repeated = false;
    ui.input_mut(|input| {
        input.events.retain(|event| {
            let Event::Key {
                key: Key::Enter | Key::Space,
                pressed: true,
                repeat,
                ..
            } = event
            else {
                return true;
            };
            fresh |= !repeat;
            repeated |= *repeat;
            !*repeat
        })
    });
    repeated && !fresh
}

/// Draws the command rows, and reports the one a click chose.
fn draw_rows(ui: &mut Ui, menu: &Menu, rect: Rect, id: Id, tokens: &Tokens) -> Option<usize> {
    let pointer = pointer_row(ui, rect, menu.items().len());
    let count = menu.items().len();
    let mut clicked = None;

    for (index, item) in menu.items().iter().enumerate() {
        let row = geometry::row_rect(rect, index);
        let hit = ui.interact(row, id.with(("row", index)), Sense::CLICK);
        hit.widget_info(|| WidgetInfo::labeled(egui::Role::MenuItem, true, item.label.clone()));

        let corners = row_corners(index == 0, index + 1 == count);
        let tier = menu.tier(index, pointer);
        paint_highlight(ui, tokens, row, corners, tier);
        if tier == Tier::Keyboard {
            paint_marker(ui, row, corners, tokens.palette.text);
        }
        if item.kind == Kind::Destructive {
            // The verb is the whole signal: a hairline above and full ink.
            ui.painter().rect_filled(
                Rect::from_min_size(row.min, vec2(row.width(), stroke::HAIRLINE)),
                radius::NONE,
                tokens.palette.border,
            );
        }
        paint_row(ui, tokens, item, row, tier);

        if hit.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if clicked_on(ui, &hit, row) {
            clicked = Some(index);
        }
    }
    clicked
}

/// The submenu a row opens: a field to find a list by, and the lists that match.
///
/// It is the same card as the menu it hangs off, beside it at that row's top and
/// flipped to the other side when the window's right edge would cut it. The
/// sheet shows eight rows: more lists than that are found by typing, so the card
/// never scrolls and the field is never decorative.
///
/// Returns the close a list took, which ends the whole menu.
fn draw_submenu(
    ui: &mut Ui,
    menu: &mut Menu,
    parent: Rect,
    area_id: Id,
    tokens: &Tokens,
) -> Option<Closed> {
    let row = menu.submenu()?;
    let names: Vec<String> = menu
        .shown()
        .into_iter()
        .map(|index| menu.playlists()[index].name.clone())
        .collect();
    let widths: Vec<geometry::RowWidth> = names
        .iter()
        .map(|name| geometry::RowWidth {
            label: measure(ui, name, recipe::track_secondary(), tokens),
            key: None,
            chevron: false,
        })
        .collect();
    let size = geometry::submenu_size(names.len().max(1), geometry::submenu_width(&widths));
    let rect = geometry::submenu_place(
        parent,
        geometry::row_rect(parent, row),
        size,
        ui.ctx().content_rect(),
    );
    ui.expand_to_include_rect(rect);

    let id = area_id.with(("submenu", row));
    let field_rect = geometry::submenu_field(rect);
    let field_id = id.with("field");
    let mut chosen = None;

    let card = Card {
        rect,
        id,
        // The field is the submenu's one focus stop, not the card.
        sense: Sense::CLICK,
        tokens,
    };
    menu_box::card(ui, card, |ui| {
        ui.painter()
            .rect_filled(field_rect, radius::SM, tokens.palette.surface);
        let focused = draw_filter(ui, menu, field_rect, field_id, tokens);
        ui.painter().rect_stroke(
            field_rect,
            radius::SM,
            Stroke::new(
                stroke::HAIRLINE,
                if focused {
                    tokens.palette.border_strong
                } else {
                    tokens.palette.border
                },
            ),
            StrokeKind::Inside,
        );
        ui.painter().rect_filled(
            geometry::submenu_rule(rect),
            radius::NONE,
            tokens.palette.border,
        );

        if names.is_empty() {
            // An empty result is one muted line, on the same rhythm as a row:
            // the card is never a hole with a field on top of it.
            let row = geometry::submenu_row(rect, 0);
            let parts = geometry::command_parts(row, None, false);
            let galley = recipe::line(
                ui,
                "No playlists match.",
                recipe::track_secondary(),
                tokens.palette.text_muted,
                0.0,
                parts.label.width(),
            );
            ui.painter().galley(
                pos2(parts.label.left(), row.center().y - galley.size().y / 2.0),
                galley,
                tokens.palette.text_muted,
            );
            return;
        }

        let hovered = pointer_row_in(ui, rect, names.len());
        if ui.input(|input| input.pointer.is_moving()) {
            menu.submenu_pointer(hovered);
        }
        for (position, name) in names.iter().enumerate() {
            let row = geometry::submenu_row(rect, position);
            let hit = ui.interact(row, id.with(("list", position)), Sense::CLICK);
            hit.widget_info(|| WidgetInfo::labeled(egui::Role::MenuItem, true, name.clone()));

            let corners = row_corners(position == 0, position + 1 == names.len());
            let tier = menu.submenu_tier(position, hovered);
            paint_highlight(ui, tokens, row, corners, tier);
            if tier == Tier::Keyboard {
                paint_marker(ui, row, corners, tokens.palette.text);
            }

            let parts = geometry::command_parts(row, None, false);
            let color = if tier == Tier::None {
                tokens.palette.text_muted
            } else {
                tokens.palette.text
            };
            let galley = recipe::line(
                ui,
                name,
                recipe::track_secondary(),
                color,
                0.0,
                parts.label.width(),
            );
            ui.painter().galley(
                pos2(parts.label.left(), row.center().y - galley.size().y / 2.0),
                galley,
                color,
            );

            if hit.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if clicked_on(ui, &hit, row) {
                let list = menu.shown()[position];
                chosen = menu.choose_target(list);
            }
        }
    });
    chosen
}

/// The submenu's find field, and whether it holds the keyboard.
fn draw_filter(ui: &mut Ui, menu: &mut Menu, rect: Rect, id: Id, tokens: &Tokens) -> bool {
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .id_salt(id.with("text"))
            .max_rect(rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    child.set_clip_rect(rect);
    // The hint is the palette's subtle tier, as the search field's is: egui's own
    // hint colour is a step too bright for a placeholder.
    child.visuals_mut().weak_text_color = Some(tokens.palette.text_subtle);
    child.visuals_mut().text_cursor.stroke = Stroke::new(stroke::HAIRLINE, tokens.palette.text);

    let output = egui::TextEdit::singleline(menu.filter_edit())
        .id(id)
        .frame(
            egui::Frame::NONE
                .inner_margin(egui::Margin::symmetric(space::S2 as i8, space::S1 as i8)),
        )
        .min_size(rect.size())
        .desired_width(rect.width())
        .align(egui::Align2::LEFT_CENTER)
        .font(FontId::new(text::SM, fonts::sans(Weight::Regular)))
        .text_color(tokens.palette.text)
        .event_filter(keys::FIELD_FILTER)
        .hint_text(egui::RichText::new("Find a playlist").color(tokens.palette.text_subtle))
        .show(&mut child);
    output.response.has_focus()
}

/// The submenu row the pointer is on, if it is on one.
fn pointer_row_in(ui: &Ui, card: Rect, rows: usize) -> Option<usize> {
    let pos = ui.ctx().pointer_hover_pos()?;
    (0..rows).find(|row| geometry::submenu_row(card, *row).contains(pos))
}

/// One command row's text and trailing slot.
fn paint_row(ui: &Ui, tokens: &Tokens, item: &Item, row: Rect, tier: Tier) {
    let lit = tier != Tier::None;
    let key = item.key.map(|key| measure(ui, key, key_font(), tokens));
    let parts = geometry::command_parts(row, key, item.submenu);

    // Commands rest one tier below the ink, so the destructive one can be the
    // only row written in full bone without a colour the palette does not have.
    let color = if lit || item.kind == Kind::Destructive {
        tokens.palette.text
    } else {
        tokens.palette.text_muted
    };
    if parts.label.width() > 0.0 {
        let galley = recipe::line(
            ui,
            &item.label,
            recipe::track_secondary(),
            color,
            0.0,
            parts.label.width(),
        );
        ui.painter().galley(
            pos2(
                parts.label.left(),
                parts.label.center().y - galley.size().y / 2.0,
            ),
            galley,
            color,
        );
    }

    // A hint only exists for a key that is really bound, and on a lit row it
    // steps up one tier with the label.
    if let (Some(key), Some(slot)) = (item.key, parts.key) {
        let key_color = if lit {
            tokens.palette.text_muted
        } else {
            tokens.palette.text_subtle
        };
        let galley = recipe::line(
            ui,
            key,
            key_font(),
            key_color,
            recipe::tracking_pts(text::XXS, tracking::WIDE),
            slot.width(),
        );
        ui.painter().galley(
            pos2(slot.left(), slot.center().y - galley.size().y / 2.0),
            galley,
            key_color,
        );
    }

    if let Some(slot) = parts.chevron {
        let color = if lit {
            tokens.palette.text_muted
        } else {
            tokens.palette.text_subtle
        };
        icon::paint(ui.painter(), Icon::ChevronRight, slot, Size::SM, color);
    }
}

/// The row the pointer is on, if it is on one.
fn pointer_row(ui: &Ui, rect: Rect, rows: usize) -> Option<usize> {
    let pos = ui.ctx().pointer_hover_pos()?;
    (0..rows).find(|row| geometry::row_rect(rect, *row).contains(pos))
}

/// A label's own width, measured in the font it is drawn in.
pub(super) fn measure(ui: &Ui, label: &str, font: FontId, tokens: &Tokens) -> f32 {
    recipe::line(ui, label, font, tokens.palette.text, 0.0, f32::INFINITY)
        .size()
        .x
}

/// The face a key hint is set in: mono caps at the smallest step, tracked apart.
pub(super) fn key_font() -> FontId {
    FontId::new(text::XXS, fonts::mono(MonoWeight::Medium))
}

fn close(ui: &Ui, menu: &mut Menu, reason: Reason) -> Option<Closed> {
    let closed = menu.close(reason)?;
    // A page change has no button to give the keyboard back to.
    if closed.opener.is_some() {
        focus_back(ui, closed);
    }
    Some(closed)
}

/// Hands the keyboard back to the widget that opened the menu.
fn focus_back(ui: &Ui, closed: Closed) {
    if let Some(opener) = closed.opener {
        ui.memory_mut(|memory| memory.request_focus(opener));
    }
}

#[cfg(test)]
mod tests;
