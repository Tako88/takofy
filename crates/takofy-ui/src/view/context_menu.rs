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
    Event, EventFilter, FontId, Id, Key, Modifiers, Order, Rect, Sense, Ui, WidgetInfo, pos2, vec2,
};

use crate::icon::{self, Icon};
use crate::layout::menu as geometry;
use crate::menu::{Closed, Cut, Item, Kind, Menu, Reason, Request, Step, Tier};
use crate::theme::icon::Size;
use crate::theme::{MonoWeight, Tokens, fonts, radius, stroke, text, tracking};
use crate::verbs;

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

    let mut chosen = None;
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
                ui.memory_mut(|memory| memory.request_focus(surface_id));
            }
            let card = Card {
                rect,
                id: surface_id,
                // The card is the menu's one focus stop; the rows are not.
                sense: Sense::click(),
                tokens,
            };
            let (surface, clicked) =
                menu_box::card(ui, card, |ui| draw_rows(ui, menu, rect, area_id, tokens));
            surface.widget_info(|| {
                WidgetInfo::labeled(egui::Role::Menu, true, verbs::menu_label(menu.targets()))
            });
            chosen = clicked;
        });

    if !sizing {
        menu.mark_drawn(now);
    }

    // Enter is consumed on every frame, so a repeat reaches neither the `⋯` nor
    // the drawer, and only a fresh press chooses.
    let enter = ui.input_mut(consume_enter);

    if !fresh {
        // Escape is read and consumed before anything else, so the focus path
        // below never sees it and the drawer - which reads it without consuming
        // - never gets one the menu closed on.
        let (escape, tab, down, up, first, last) = ui.input_mut(|input| {
            (
                input.consume_key(Modifiers::NONE, Key::Escape),
                input.consume_key(Modifiers::NONE, Key::Tab),
                input.consume_key(Modifiers::NONE, Key::ArrowDown),
                input.consume_key(Modifiers::NONE, Key::ArrowUp),
                input.consume_key(Modifiers::NONE, Key::Home),
                input.consume_key(Modifiers::NONE, Key::End),
            )
        });
        if escape {
            return close(ui, menu, Reason::Escape);
        }
        if tab {
            return close(ui, menu, Reason::Tab);
        }
        if down {
            menu.step(Step::Down);
        }
        if up {
            menu.step(Step::Up);
        }
        if first {
            menu.step(Step::First);
        }
        if last {
            menu.step(Step::Last);
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
            && let Some(closed) = menu.choose_lit()
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
fn measure(ui: &Ui, label: &str, font: FontId, tokens: &Tokens) -> f32 {
    recipe::line(ui, label, font, tokens.palette.text, 0.0, f32::INFINITY)
        .size()
        .x
}

/// The face a key hint is set in: mono caps at the smallest step, tracked apart.
fn key_font() -> FontId {
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
mod tests {
    use egui::{Event, Id, Key, Modifiers, Pos2, RawInput, Rect, Shape, pos2, vec2};
    use takofy_model::TrackEntry;

    use super::*;
    use crate::layout::search_menu::row_rect;
    use crate::menu::{Anchor, Device, Door, Gate, Item, Kind, Listing, Menu, Request, Verb};
    use crate::theme::{extent, fonts, style};
    use crate::verbs;
    use crate::view::test_support::{button, click_events};

    fn context() -> egui::Context {
        let ctx = egui::Context::default();
        ctx.set_fonts(fonts::definitions());
        ctx.set_global_style(style(&Tokens::DARK));
        ctx.enable_accesskit();
        ctx
    }

    fn screen() -> Rect {
        Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))
    }

    fn input(events: Vec<Event>) -> RawInput {
        RawInput {
            screen_rect: Some(screen()),
            events,
            ..Default::default()
        }
    }

    fn anchor() -> Anchor {
        Anchor::Button(Rect::from_min_size(pos2(600.0, 100.0), vec2(28.0, 28.0)))
    }

    fn opener() -> Id {
        Id::unique("row-more")
    }

    fn door(device: Device) -> Door {
        Door {
            anchor: anchor(),
            opener: opener(),
            device,
        }
    }

    fn track() -> TrackEntry {
        TrackEntry {
            uri: "spotify:track:a".to_owned(),
            title: "Harbour Wall".to_owned(),
            artists: "Slow Tide".to_owned(),
            album: Some("A Record".to_owned()),
            album_uri: Some("spotify:album:a".to_owned()),
            artist_uris: vec!["spotify:artist:b".to_owned()],
            added_at: None,
            duration_ms: None,
            named: true,
            cover: None,
        }
    }

    /// The four verbs one track gets on a page, in the sheet's order.
    fn items() -> Vec<Item> {
        verbs::track_items(&[track()], None, &Listing::Page)
    }

    fn request(device: Device, items: Vec<Item>) -> Request {
        Request {
            door: door(device),
            items,
            targets: vec![track()],
            rows: vec![0],
            listing: Listing::Page,
            page: None,
        }
    }

    fn opened(device: Device) -> Menu {
        let mut menu = Menu::default();
        assert!(menu.open(request(device, items()), Gate::default()));
        menu
    }

    fn opened_with(device: Device, items: Vec<Item>) -> Menu {
        let mut menu = Menu::default();
        assert!(menu.open(request(device, items), Gate::default()));
        menu
    }

    /// What one frame drew, and what the menu reported.
    struct Drawn {
        closed: Option<Closed>,
        output: egui::FullOutput,
    }

    /// Draws one frame of the open menu, and runs `read` inside it, where the
    /// widget ids and the AccessKit nodes are live.
    fn draw<R>(
        ctx: &egui::Context,
        menu: &mut Menu,
        free: bool,
        events: Vec<Event>,
        read: impl FnOnce(&egui::Ui, Id) -> R,
    ) -> (Drawn, R) {
        let tokens = Tokens::DARK;
        let mut closed = None;
        let mut read = Some(read);
        let mut answer = None;
        let mut output = ctx.run_ui(input(events), |ui| {
            closed = show(
                ui,
                Frame {
                    menu,
                    free,
                    tokens: &tokens,
                },
            );
            let area = ui.make_persistent_id("context-menu");
            if let Some(read) = read.take() {
                answer = Some(read(ui, area));
            }
        });
        output.textures_delta.clear();
        (
            Drawn { closed, output },
            answer.expect("the frame ran its read"),
        )
    }

    fn frame(ctx: &egui::Context, menu: &mut Menu, free: bool, events: Vec<Event>) -> Drawn {
        draw(ctx, menu, free, events, |_, _| ()).0
    }

    /// The card's rect, read back from the one raised-surface fill the frame
    /// painted.
    fn card(output: &egui::FullOutput) -> Rect {
        output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                Shape::Rect(shape) if shape.fill == Tokens::DARK.palette.surface_raised => {
                    Some(shape.rect)
                }
                _ => None,
            })
            .expect("the menu painted its card")
    }

    /// Every fill one frame painted, with the rect it filled.
    fn fills(output: &egui::FullOutput) -> Vec<(Rect, egui::Color32)> {
        output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                Shape::Rect(shape) => Some((shape.rect, shape.fill)),
                _ => None,
            })
            .collect()
    }

    /// The fills of `color`, as rects.
    fn fills_of(output: &egui::FullOutput, color: egui::Color32) -> Vec<Rect> {
        fills(output)
            .into_iter()
            .filter(|(_, fill)| *fill == color)
            .map(|(rect, _)| rect)
            .collect()
    }

    /// Whether a glyph, and not a line of text, was painted into `rect`.
    fn glyph_into(output: &egui::FullOutput, rect: Rect) -> bool {
        output.shapes.iter().any(|clipped| {
            matches!(clipped.shape, Shape::Path(_))
                && clipped.shape.visual_bounding_rect().intersects(rect)
        })
    }

    /// The colour and text of every line one frame painted.
    fn texts(output: &egui::FullOutput) -> Vec<(String, egui::Color32)> {
        output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                Shape::Text(text) => Some((text.galley.text().to_owned(), text.fallback_color)),
                _ => None,
            })
            .collect()
    }

    /// A search state with one answer, open the way a click on the field opens
    /// it.
    fn answered_search() -> crate::search::Search {
        let mut search = crate::search::Search::default();
        search.text = "a".to_owned();
        search.text_changed();
        search.apply(&crate::message::Update::Search {
            query: "a".to_owned(),
            tracks: vec![track()],
        });
        let _ = search.step(
            crate::search::KeyInput {
                focused: true,
                clicked_field: true,
                ..crate::search::KeyInput::default()
            },
            1,
            false,
        );
        assert!(search.is_open(), "the search menu is drawn");
        search
    }

    fn field() -> Rect {
        Rect::from_min_size(pos2(40.0, 12.0), vec2(320.0, extent::SEARCH_H))
    }

    /// Draws one frame of the search drop-down, returning the frame that paints.
    fn search_frame(ctx: &egui::Context, search: &mut crate::search::Search) -> egui::FullOutput {
        let tokens = Tokens::DARK;
        let mut art = crate::cover::Art::default();
        let mut output = None;
        for pass in 0..2 {
            art.begin_frame();
            let mut drawn = ctx.run_ui(input(Vec::new()), |ui| {
                let _ = crate::view::search_menu::show(
                    ui,
                    field(),
                    search,
                    &crate::player::Transport::default(),
                    &mut art,
                    std::time::Instant::now(),
                    &tokens,
                );
            });
            drawn.textures_delta.clear();
            if pass == 1 {
                output = Some(drawn);
            }
        }
        output.expect("two passes")
    }

    /// How many Delete presses the frame still has after the menu read it.
    fn deletes(ui: &egui::Ui) -> usize {
        ui.input(|input| {
            input
                .events
                .iter()
                .filter(|event| {
                    matches!(
                        event,
                        Event::Key {
                            key: Key::Delete,
                            pressed: true,
                            ..
                        }
                    )
                })
                .count()
        })
    }

    fn destructive_item(label: &str) -> Item {
        Item {
            label: label.to_owned(),
            kind: Kind::Destructive,
            key: Some("Del"),
            submenu: false,
            verb: Verb::Remove,
        }
    }

    /// The four page verbs, plus the destructive one last.
    fn removable() -> Vec<Item> {
        let mut items = items();
        items.push(destructive_item("Remove from this playlist"));
        items
    }

    fn key_event(key: Key, modifiers: Modifiers) -> Event {
        Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        }
    }

    /// One press with no release before it, which egui marks as a held key's
    /// auto-repeat from its own `keys_down`.
    fn repeat(key: Key) -> Event {
        Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: true,
            modifiers: Modifiers::NONE,
        }
    }

    fn press(ctx: &egui::Context, menu: &mut Menu, key: Key) -> Drawn {
        let pressed = frame(ctx, menu, true, vec![key_event(key, Modifiers::NONE)]);
        // A release as well, so a held key is not read twice by the next frame.
        frame(
            ctx,
            menu,
            true,
            vec![Event::Key {
                key,
                physical_key: None,
                pressed: false,
                repeat: false,
                modifiers: Modifiers::NONE,
            }],
        );
        pressed
    }

    /// Draws the two frames an `Area` takes to arrive: its first is a sizing
    /// pass that paints nothing, and its second is the menu's opening frame,
    /// which ignores the frame's keys.
    fn warm(ctx: &egui::Context, menu: &mut Menu) {
        let _ = frame(ctx, menu, true, Vec::new());
        let _ = frame(ctx, menu, true, Vec::new());
    }

    #[test]
    fn the_box_is_the_search_menus_box() {
        let ctx = context();
        let mut menu = opened(Device::Pointer);
        warm(&ctx, &mut menu);
        let (drawn, _) = draw(&ctx, &mut menu, true, Vec::new(), |_, _| ());
        let search_output = search_frame(&ctx, &mut answered_search());

        let tokens = Tokens::DARK;
        let box_of = |output: &egui::FullOutput| {
            let rects: Vec<&egui::epaint::RectShape> = output
                .shapes
                .iter()
                .filter_map(|clipped| match &clipped.shape {
                    Shape::Rect(shape) => Some(shape),
                    _ => None,
                })
                .collect();
            let shadow = rects
                .iter()
                .find(|shape| shape.blur_width > 0.0)
                .expect("a shadow");
            let fill = rects
                .iter()
                .find(|shape| shape.fill == tokens.palette.surface_raised)
                .expect("a fill");
            let hairline = rects
                .iter()
                .find(|shape| shape.stroke.color == tokens.palette.border_strong)
                .expect("a hairline");
            (
                shadow.blur_width,
                shadow.fill,
                fill.corner_radius,
                hairline.corner_radius,
                hairline.stroke.width,
            )
        };
        assert_eq!(
            box_of(&drawn.output),
            box_of(&search_output),
            "the two menus are not one box"
        );
    }

    #[test]
    fn the_menu_draws_above_the_search_drop_down() {
        // The shell draws the drop-down first and the context menu after it, so
        // both hang in the foreground layer and this one is painted last.
        let ctx = context();
        let tokens = Tokens::DARK;
        let mut search = answered_search();
        let mut menu = opened(Device::Pointer);
        let mut art = crate::cover::Art::default();
        let mut output = None;
        for pass in 0..2 {
            art.begin_frame();
            let mut drawn = ctx.run_ui(input(Vec::new()), |ui| {
                let _ = crate::view::search_menu::show(
                    ui,
                    field(),
                    &mut search,
                    &crate::player::Transport::default(),
                    &mut art,
                    std::time::Instant::now(),
                    &tokens,
                );
                let _ = show(
                    ui,
                    Frame {
                        menu: &mut menu,
                        free: true,
                        tokens: &tokens,
                    },
                );
            });
            drawn.textures_delta.clear();
            if pass == 1 {
                output = Some(drawn);
            }
        }
        let output = output.expect("two passes");

        let cards: Vec<(usize, Rect)> = output
            .shapes
            .iter()
            .enumerate()
            .filter_map(|(index, clipped)| match &clipped.shape {
                Shape::Rect(shape) if shape.fill == tokens.palette.surface_raised => {
                    Some((index, shape.rect))
                }
                _ => None,
            })
            .collect();
        assert_eq!(cards.len(), 2, "both menus are drawn: {cards:?}");
        let drop_down = cards
            .iter()
            .min_by(|left, right| left.1.top().total_cmp(&right.1.top()))
            .expect("a drop-down");
        let context = cards
            .iter()
            .max_by(|left, right| left.1.top().total_cmp(&right.1.top()))
            .expect("a context menu");
        assert!(
            drop_down.0 < context.0,
            "the context menu is painted under the drop-down"
        );
    }

    #[test]
    fn a_pointer_open_paints_no_rule_and_no_wash() {
        let ctx = context();
        let mut menu = opened(Device::Pointer);
        warm(&ctx, &mut menu);
        let (drawn, _) = draw(&ctx, &mut menu, true, Vec::new(), |_, _| ());
        let tokens = Tokens::DARK;
        for wash in [tokens.palette.row_selected, tokens.palette.row_hover] {
            assert!(
                fills_of(&drawn.output, wash).is_empty(),
                "a pointer open painted a tier: {wash:?}"
            );
        }
        let rule = fills_of(&drawn.output, tokens.palette.text)
            .into_iter()
            .filter(|rect| rect.width() == crate::theme::stroke::RULE);
        assert_eq!(
            rule.count(),
            0,
            "a pointer open painted the keyboard's rule"
        );
    }

    #[test]
    fn a_keyboard_open_paints_the_rule_and_the_selected_wash_on_row_0_only() {
        let ctx = context();
        let mut menu = opened(Device::Keyboard);
        warm(&ctx, &mut menu);
        let (drawn, _) = draw(&ctx, &mut menu, true, Vec::new(), |_, _| ());
        let tokens = Tokens::DARK;
        let card = card(&drawn.output);

        let washes = fills_of(&drawn.output, tokens.palette.row_selected);
        assert_eq!(washes, vec![row_rect(card, 0)], "the wash is not on row 0");

        let rules: Vec<Rect> = fills_of(&drawn.output, tokens.palette.text)
            .into_iter()
            .filter(|rect| rect.width() == crate::theme::stroke::RULE)
            .collect();
        assert_eq!(rules.len(), 1, "the keyboard's rule is drawn once");
        assert_eq!(rules[0].top(), row_rect(card, 0).top());
        assert_eq!(rules[0].height(), extent::ROW_H);
        assert!(
            fills_of(&drawn.output, tokens.palette.row_hover).is_empty(),
            "a keyboard open painted the pointer's wash"
        );
    }

    #[test]
    fn the_pointer_tier_paints_wash_and_tick_only_under_the_pointer() {
        let ctx = context();
        let mut menu = opened(Device::Pointer);
        warm(&ctx, &mut menu);
        let (drawn, _) = draw(&ctx, &mut menu, true, Vec::new(), |_, _| ());
        let card = card(&drawn.output);
        let row = row_rect(card, 1);
        let tokens = Tokens::DARK;

        // The pointer arrives, and the frame after it draws the tier.
        frame(
            &ctx,
            &mut menu,
            true,
            vec![Event::PointerMoved(row.center())],
        );
        let drawn = frame(
            &ctx,
            &mut menu,
            true,
            vec![Event::PointerMoved(row.center())],
        );

        assert_eq!(
            fills_of(&drawn.output, tokens.palette.row_hover),
            vec![row],
            "the pointer's wash is not on the row under the pointer"
        );
        let ticks: Vec<Rect> = fills_of(&drawn.output, tokens.palette.text_muted)
            .into_iter()
            .filter(|rect| rect.width() == crate::theme::stroke::RULE)
            .collect();
        assert_eq!(ticks.len(), 1, "the pointer's tier is one tick");
        assert_eq!(ticks[0].center().y, row.center().y);
        assert!(
            fills_of(&drawn.output, tokens.palette.row_selected).is_empty(),
            "the pointer's tier borrowed the keyboard's wash"
        );
    }

    #[test]
    fn rows_are_named_menu_items_and_the_card_is_a_menu() {
        let ctx = context();
        let mut menu = opened(Device::Pointer);
        warm(&ctx, &mut menu);
        let (_, nodes) = draw(&ctx, &mut menu, true, Vec::new(), |ui, area| {
            let surface = ui
                .ctx()
                .accesskit_node_builder(area.with("surface"), |node| {
                    (node.role(), node.label().map(str::to_owned))
                })
                .expect("the card is a node");
            let rows: Vec<_> = (0..4)
                .map(|index| {
                    ui.ctx()
                        .accesskit_node_builder(area.with(("row", index)), |node| {
                            (node.role(), node.label().map(str::to_owned))
                        })
                        .expect("a row is a node")
                })
                .collect();
            (surface, rows)
        });

        assert_eq!(nodes.0.0, egui::Role::Menu);
        assert_eq!(nodes.0.1.as_deref(), Some("Harbour Wall"));
        let expected = ["Add to queue", "Go to album", "Go to artist", "Copy link"];
        assert_eq!(nodes.1.len(), expected.len());
        for ((role, label), name) in nodes.1.into_iter().zip(expected) {
            assert_eq!(role, egui::Role::MenuItem);
            assert_eq!(label.as_deref(), Some(name));
        }
    }

    #[test]
    fn a_destructive_row_is_full_ink_with_a_hairline_above_it() {
        let ctx = context();
        let items = vec![
            Item {
                label: "New playlist…".to_owned(),
                kind: Kind::Plain,
                key: None,
                submenu: false,
                verb: Verb::AddToQueue,
            },
            Item {
                label: "Remove from this playlist".to_owned(),
                kind: Kind::Destructive,
                key: None,
                submenu: false,
                verb: Verb::CopyLink,
            },
        ];
        let mut menu = opened_with(Device::Pointer, items);
        warm(&ctx, &mut menu);
        let (drawn, _) = draw(&ctx, &mut menu, true, Vec::new(), |_, _| ());
        let tokens = Tokens::DARK;
        let card = card(&drawn.output);
        let destructive = row_rect(card, 1);

        let (_, color) = texts(&drawn.output)
            .into_iter()
            .find(|(text, _)| text == "Remove from this playlist")
            .expect("the destructive label is painted");
        assert_eq!(color, tokens.palette.text, "the command is not full ink");

        let (_, plain) = texts(&drawn.output)
            .into_iter()
            .find(|(text, _)| text == "New playlist…")
            .expect("the plain label is painted");
        assert_eq!(
            plain, tokens.palette.text_muted,
            "a resting command is not one tier below the ink"
        );

        let hairline = Rect::from_min_size(
            destructive.min,
            vec2(destructive.width(), crate::theme::stroke::HAIRLINE),
        );
        assert!(
            fills_of(&drawn.output, tokens.palette.border).contains(&hairline),
            "the destructive row has no hairline above it"
        );
    }

    #[test]
    fn a_key_hint_draws_only_when_the_item_has_one() {
        let ctx = context();
        let hinted = vec![Item {
            label: "Remove from this playlist".to_owned(),
            kind: Kind::Destructive,
            key: Some("Del"),
            submenu: false,
            verb: Verb::CopyLink,
        }];
        let mut menu = opened_with(Device::Pointer, hinted);
        warm(&ctx, &mut menu);
        let (drawn, _) = draw(&ctx, &mut menu, true, Vec::new(), |_, _| ());
        assert!(
            texts(&drawn.output).iter().any(|(text, _)| text == "Del"),
            "the key hint is not drawn"
        );

        let bare = vec![Item {
            label: "Remove from this playlist".to_owned(),
            kind: Kind::Destructive,
            key: None,
            submenu: false,
            verb: Verb::CopyLink,
        }];
        let mut menu = opened_with(Device::Pointer, bare);
        warm(&ctx, &mut menu);
        let (drawn, _) = draw(&ctx, &mut menu, true, Vec::new(), |_, _| ());
        assert!(
            !texts(&drawn.output).iter().any(|(text, _)| text == "Del"),
            "a hint for an unbound key was drawn"
        );
    }

    #[test]
    fn a_chevron_draws_only_for_a_submenu_item() {
        let ctx = context();
        let submenu = vec![Item {
            label: "Add to playlist".to_owned(),
            kind: Kind::Plain,
            key: None,
            submenu: true,
            verb: Verb::AddToQueue,
        }];
        let mut menu = opened_with(Device::Pointer, submenu);
        warm(&ctx, &mut menu);
        let (drawn, _) = draw(&ctx, &mut menu, true, Vec::new(), |_, _| ());
        let card = card(&drawn.output);
        let row = row_rect(card, 0);
        let chevron = Rect::from_min_size(
            pos2(
                row.right() - crate::theme::space::S3 - crate::theme::icon::Size::SM.side,
                row.center().y - crate::theme::icon::Size::SM.side / 2.0,
            ),
            vec2(
                crate::theme::icon::Size::SM.side,
                crate::theme::icon::Size::SM.side,
            ),
        );
        assert!(
            glyph_into(&drawn.output, chevron),
            "the submenu row drew no chevron"
        );

        let plain = vec![Item {
            label: "Add to playlist".to_owned(),
            kind: Kind::Plain,
            key: None,
            submenu: false,
            verb: Verb::AddToQueue,
        }];
        let mut menu = opened_with(Device::Pointer, plain);
        warm(&ctx, &mut menu);
        let (drawn, _) = draw(&ctx, &mut menu, true, Vec::new(), |_, _| ());
        assert!(
            !glyph_into(&drawn.output, chevron),
            "a row with no submenu drew a chevron"
        );
    }

    #[test]
    fn a_long_label_elides_on_one_line() {
        let ctx = context();
        let long = vec![Item {
            label: "Add all twelve tracks from this record to the end of the queue".to_owned(),
            kind: Kind::Plain,
            key: None,
            submenu: false,
            verb: Verb::AddToQueue,
        }];
        let mut menu = opened_with(Device::Pointer, long);
        warm(&ctx, &mut menu);
        let (drawn, _) = draw(&ctx, &mut menu, true, Vec::new(), |_, _| ());

        let galley = drawn
            .output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                Shape::Text(text) => Some(text.galley.clone()),
                _ => None,
            })
            .expect("the label is painted");
        assert!(galley.elided, "the long label is not elided");
        assert_eq!(galley.rows.len(), 1, "the label wrapped onto a second line");
        assert_eq!(
            card(&drawn.output).width(),
            extent::MENU_MAX_W,
            "a label longer than the cap widened the menu past it"
        );
    }

    #[test]
    fn down_lights_the_next_row() {
        let ctx = context();
        let mut menu = opened(Device::Keyboard);
        warm(&ctx, &mut menu);
        press(&ctx, &mut menu, Key::ArrowDown);
        assert_eq!(menu.lit(), Some(1));

        let drawn = frame(&ctx, &mut menu, true, Vec::new());
        let card = card(&drawn.output);
        assert_eq!(
            fills_of(&drawn.output, Tokens::DARK.palette.row_selected),
            vec![row_rect(card, 1)],
            "the wash did not follow the lit row"
        );
    }

    #[test]
    fn enter_chooses_it() {
        let ctx = context();
        let mut menu = opened(Device::Keyboard);
        warm(&ctx, &mut menu);
        press(&ctx, &mut menu, Key::ArrowDown);
        let (drawn, focused) = draw(
            &ctx,
            &mut menu,
            true,
            vec![key_event(Key::Enter, Modifiers::NONE)],
            |ui, _| ui.memory(|memory| memory.has_focus(opener())),
        );
        assert_eq!(
            drawn.closed,
            Some(Closed {
                opener: Some(opener()),
                chose: Some(Verb::GoToAlbum),
            })
        );
        assert!(!menu.is_open());
        assert!(focused, "the keyboard did not go back to the button");
    }

    #[test]
    fn the_enter_that_opens_leaves_the_menu_open() {
        let ctx = context();
        // The first open sizes the area. The second is the one a user makes, and
        // egui fakes a primary click for Enter, so the same keystroke that
        // opened the menu must not fire row 0.
        let mut menu = opened(Device::Keyboard);
        warm(&ctx, &mut menu);
        let _ = menu.close(Reason::Escape);

        let mut menu = opened(Device::Keyboard);
        // The opening Enter comes with its release: egui marks a press with no
        // release between it and the one before as an auto-repeat.
        let drawn = press(&ctx, &mut menu, Key::Enter);
        assert_eq!(drawn.closed, None, "the opening Enter fired a verb");
        assert!(menu.is_open(), "the opening Enter closed the menu");

        // The next Enter is the user's, and fires.
        let drawn = press(&ctx, &mut menu, Key::Enter);
        assert_eq!(
            drawn.closed.map(|closed| closed.chose),
            Some(Some(Verb::AddToQueue))
        );
    }

    #[test]
    fn a_repeated_enter_chooses_nothing_and_leaks_nowhere() {
        let ctx = context();
        // The user opens the menu with Enter and holds it: egui marks every
        // press with no release between it and the one before as a repeat.
        let mut menu = opened(Device::Keyboard);
        warm(&ctx, &mut menu);
        let _ = menu.close(Reason::Escape);

        let mut menu = opened(Device::Keyboard);
        let opening = frame(
            &ctx,
            &mut menu,
            true,
            vec![key_event(Key::Enter, Modifiers::NONE)],
        );
        assert_eq!(opening.closed, None, "the opening Enter fired a row");

        let (drawn, leaked) = draw(&ctx, &mut menu, true, vec![repeat(Key::Enter)], |ui, _| {
            ui.input(|input| input.key_pressed(Key::Enter))
        });
        assert_eq!(drawn.closed, None, "a held Enter fired a row");
        assert!(menu.is_open(), "a held Enter closed the menu");
        assert!(!leaked, "the repeat reached the `⋯` or the drawer");
    }

    #[test]
    fn escape_closes_and_the_drawer_never_sees_it() {
        let ctx = context();
        let mut menu = opened(Device::Keyboard);
        warm(&ctx, &mut menu);

        let mut escaped = None;
        let (drawn, focused) = draw(
            &ctx,
            &mut menu,
            true,
            vec![key_event(Key::Escape, Modifiers::NONE)],
            |ui, _| {
                escaped = Some(ui.input(|input| input.key_pressed(Key::Escape)));
                ui.memory(|memory| memory.has_focus(opener()))
            },
        );
        assert_eq!(
            drawn.closed,
            Some(Closed {
                opener: Some(opener()),
                chose: None,
            })
        );
        assert_eq!(escaped, Some(false), "the Escape reached the drawer");
        assert!(focused, "Escape did not return the keyboard");
    }

    #[test]
    fn tab_closes_and_focus_returns_to_the_opener() {
        let ctx = context();
        let mut menu = opened(Device::Keyboard);
        warm(&ctx, &mut menu);
        // One more frame, so the focus lock filter is in place.
        frame(&ctx, &mut menu, true, Vec::new());

        let (drawn, focused) = draw(
            &ctx,
            &mut menu,
            true,
            vec![key_event(Key::Tab, Modifiers::NONE)],
            |ui, _| ui.memory(|memory| memory.has_focus(opener())),
        );
        assert_eq!(
            drawn.closed,
            Some(Closed {
                opener: Some(opener()),
                chose: None,
            })
        );
        assert!(focused, "Tab did not return the keyboard");
    }

    #[test]
    fn an_outside_click_closes_and_returns_the_opener() {
        let ctx = context();
        let mut menu = opened(Device::Pointer);
        warm(&ctx, &mut menu);
        let away = pos2(20.0, 560.0);
        frame(&ctx, &mut menu, true, vec![Event::PointerMoved(away)]);
        let mut closed = None;
        let mut focused = false;
        for event in click_events(away) {
            let (drawn, has) = draw(&ctx, &mut menu, true, vec![event], |ui, _| {
                ui.memory(|memory| memory.has_focus(opener()))
            });
            closed = drawn.closed;
            if closed.is_some() {
                focused = has;
            }
        }
        assert_eq!(
            closed,
            Some(Closed {
                opener: Some(opener()),
                chose: None,
            })
        );
        assert!(focused, "the outside click did not return the keyboard");
    }

    #[test]
    fn choosing_a_row_returns_the_opener() {
        let ctx = context();
        let mut menu = opened(Device::Pointer);
        warm(&ctx, &mut menu);
        let (drawn, _) = draw(&ctx, &mut menu, true, Vec::new(), |_, _| ());
        let card = card(&drawn.output);
        let row = row_rect(card, 0);
        frame(
            &ctx,
            &mut menu,
            true,
            vec![Event::PointerMoved(row.center())],
        );
        let mut closed = None;
        let mut focused = false;
        for event in click_events(row.center()) {
            let (drawn, has) = draw(&ctx, &mut menu, true, vec![event], |ui, _| {
                ui.memory(|memory| memory.has_focus(opener()))
            });
            closed = drawn.closed;
            if closed.is_some() {
                focused = has;
            }
        }
        assert_eq!(
            closed.map(|closed| (closed.opener, closed.chose)),
            Some((Some(opener()), Some(Verb::AddToQueue)))
        );
        assert!(focused, "the choice did not return the keyboard");
    }

    #[test]
    fn a_press_on_one_row_and_a_release_on_another_chooses_nothing() {
        let ctx = context();
        let mut menu = opened(Device::Pointer);
        warm(&ctx, &mut menu);
        let (drawn, _) = draw(&ctx, &mut menu, true, Vec::new(), |_, _| ());
        let card = card(&drawn.output);
        let from = row_rect(card, 0).center();
        let to = row_rect(card, 1).center();

        let mut closed = None;
        for event in [
            Event::PointerMoved(from),
            button(from, true),
            Event::PointerMoved(to),
            button(to, false),
        ] {
            closed = frame(&ctx, &mut menu, true, vec![event]).closed;
        }
        assert_eq!(closed, None, "a press on one row chose the one released on");
        assert!(menu.is_open());
    }

    #[test]
    fn an_outside_click_closes_but_not_on_the_frame_a_request_opened_it() {
        let ctx = context();
        // The first open sizes the area; the second is the one a door opens, and
        // its release is the click that opened it.
        let mut menu = opened(Device::Pointer);
        warm(&ctx, &mut menu);
        let _ = menu.close(Reason::Escape);

        let mut menu = opened(Device::Pointer);
        let away = pos2(20.0, 560.0);
        let drawn = frame(
            &ctx,
            &mut menu,
            true,
            vec![Event::PointerMoved(away), button(away, false)],
        );
        assert_eq!(drawn.closed, None, "the opening click dismissed the menu");
        assert!(menu.is_open());

        let mut closed = None;
        for event in click_events(away) {
            closed = frame(&ctx, &mut menu, true, vec![event]).closed;
        }
        assert!(closed.is_some(), "an outside click did not close the menu");
        assert!(!menu.is_open());
    }

    #[test]
    fn a_choice_is_not_offered_while_the_frame_has_no_command_to_spare() {
        let ctx = context();
        let mut menu = opened(Device::Keyboard);
        warm(&ctx, &mut menu);
        press(&ctx, &mut menu, Key::ArrowDown);
        let drawn = frame(
            &ctx,
            &mut menu,
            false,
            vec![key_event(Key::Enter, Modifiers::NONE)],
        );
        assert_eq!(drawn.closed, None, "a choice was offered and dropped");
        assert!(menu.is_open(), "a dropped choice closed the menu");
    }

    #[test]
    fn the_menu_stays_inside_a_640_by_480_window_from_every_corner() {
        let window = Rect::from_min_size(Pos2::ZERO, vec2(640.0, 480.0));
        let corners = [
            pos2(0.0, 0.0),
            pos2(window.width() - 28.0, 0.0),
            pos2(0.0, window.height() - 28.0),
            pos2(window.width() - 28.0, window.height() - 28.0),
        ];
        for corner in corners {
            let ctx = egui::Context::default();
            ctx.set_fonts(fonts::definitions());
            ctx.set_global_style(style(&Tokens::DARK));
            let mut menu = Menu::default();
            assert!(menu.open(
                Request {
                    door: Door {
                        anchor: Anchor::Button(Rect::from_min_size(corner, vec2(28.0, 28.0))),
                        opener: opener(),
                        device: Device::Pointer,
                    },
                    items: items(),
                    targets: vec![track()],
                    rows: vec![0],
                    listing: Listing::Page,
                    page: None,
                },
                Gate::default()
            ));
            let tokens = Tokens::DARK;
            // Two passes: an `Area`'s first frame is its sizing pass.
            let mut output = None;
            for pass in 0..2 {
                let mut drawn = ctx.run_ui(
                    RawInput {
                        screen_rect: Some(window),
                        ..Default::default()
                    },
                    |ui| {
                        let _ = show(
                            ui,
                            Frame {
                                menu: &mut menu,
                                free: true,
                                tokens: &tokens,
                            },
                        );
                    },
                );
                drawn.textures_delta.clear();
                if pass == 1 {
                    output = Some(drawn);
                }
            }
            let output = output.expect("two passes");
            let card = card(&output);
            assert!(
                card.left() >= window.left() - 0.01
                    && card.right() <= window.right() + 0.01
                    && card.top() >= window.top() - 0.01
                    && card.bottom() <= window.bottom() + 0.01,
                "the menu at {corner:?} left the window: {card:?}"
            );
        }
    }

    #[test]
    fn the_card_swallows_clicks_and_is_the_one_focus_stop() {
        let ctx = context();
        let mut menu = opened(Device::Pointer);
        warm(&ctx, &mut menu);
        let (_, (surface, rows)) = draw(&ctx, &mut menu, true, Vec::new(), |ui, area| {
            let surface = ui
                .ctx()
                .read_response(area.with("surface"))
                .expect("the card is a widget");
            let rows: Vec<egui::Response> = (0..4)
                .map(|index| {
                    ui.ctx()
                        .read_response(area.with(("row", index)))
                        .expect("a row is a widget")
                })
                .collect();
            (surface, rows)
        });

        assert!(surface.sense.senses_click(), "the card takes the click");
        assert!(surface.sense.is_focusable(), "the card is the focus stop");
        for row in &rows {
            assert!(!row.sense.is_focusable(), "a row joined the tab order");
        }
    }

    #[test]
    fn a_closed_menu_draws_nothing() {
        let ctx = context();
        let mut menu = Menu::default();
        let (drawn, _) = draw(&ctx, &mut menu, true, Vec::new(), |_, _| ());
        assert_eq!(drawn.closed, None);
        assert!(drawn.output.shapes.is_empty(), "a closed menu painted");
    }
    #[test]
    fn delete_chooses_the_destructive_row_and_takes_the_key() {
        let ctx = context();
        let mut menu = opened_with(Device::Pointer, removable());
        warm(&ctx, &mut menu);
        let (drawn, left) = draw(
            &ctx,
            &mut menu,
            true,
            vec![key_event(Key::Delete, Modifiers::NONE)],
            |ui, _| deletes(ui),
        );
        assert_eq!(
            drawn.closed.and_then(|closed| closed.chose),
            Some(Verb::Remove),
            "Del did not choose the destructive row"
        );
        assert_eq!(left, 0, "the Del reached the region behind the menu");
        assert!(!menu.is_open());
    }

    #[test]
    fn delete_in_a_menu_without_a_removal_does_nothing() {
        let ctx = context();
        let mut menu = opened(Device::Pointer);
        warm(&ctx, &mut menu);
        let (drawn, left) = draw(
            &ctx,
            &mut menu,
            true,
            vec![key_event(Key::Delete, Modifiers::NONE)],
            |ui, _| deletes(ui),
        );
        assert_eq!(drawn.closed, None, "a menu with no removal chose one");
        assert!(menu.is_open(), "a menu with no removal closed on Del");
        assert_eq!(left, 0, "the menu left the key for the page behind it");
    }

    #[test]
    fn a_modified_delete_is_not_the_menus_and_a_repeat_chooses_nothing() {
        let ctx = context();
        let mut menu = opened_with(Device::Pointer, removable());
        warm(&ctx, &mut menu);
        let shifted = Event::Key {
            key: Key::Delete,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::SHIFT,
        };
        let (drawn, left) = draw(&ctx, &mut menu, true, vec![shifted], |ui, _| deletes(ui));
        assert_eq!(drawn.closed, None, "Shift+Del chose a row");
        assert!(menu.is_open());
        assert_eq!(left, 1, "a modified Del was taken from its own owner");

        let mut menu = opened_with(Device::Pointer, removable());
        warm(&ctx, &mut menu);
        let (drawn, left) = draw(&ctx, &mut menu, true, vec![repeat(Key::Delete)], |ui, _| {
            deletes(ui)
        });
        assert_eq!(drawn.closed, None, "a held Del chose twice");
        assert!(menu.is_open(), "a held Del closed the menu");
        assert_eq!(left, 0, "a held Del leaked past the menu");
    }
    #[test]
    fn a_posted_cut_is_taken_once() {
        let ctx = context();
        let cut = Cut {
            listing: Listing::Queue,
            rows: vec![1],
            entries: vec![track()],
        };
        post_cut(&ctx, cut.clone());
        assert_eq!(take_cut(&ctx), Some(cut));
        assert_eq!(take_cut(&ctx), None, "a cut is read once");
    }

    /// One frame that reads `is_open` before it draws the menu, the order the
    /// shell runs in.
    fn seen_before_the_draw(ctx: &egui::Context, menu: &mut Menu) -> bool {
        let tokens = Tokens::DARK;
        let mut seen = false;
        let mut drawn = ctx.run_ui(input(Vec::new()), |ui| {
            seen = is_open(ui.ctx());
            let _ = show(
                ui,
                Frame {
                    menu,
                    free: true,
                    tokens: &tokens,
                },
            );
        });
        drawn.textures_delta.clear();
        seen
    }

    #[test]
    fn an_open_menu_says_so_before_it_draws() {
        let ctx = context();
        let mut menu = opened(Device::Pointer);
        warm(&ctx, &mut menu);
        assert!(
            seen_before_the_draw(&ctx, &mut menu),
            "a page drew behind an open menu it could not see"
        );

        let _ = menu.close(Reason::Escape);
        let _ = seen_before_the_draw(&ctx, &mut menu);
        assert!(
            !seen_before_the_draw(&ctx, &mut menu),
            "the frame after the close still reported an open menu"
        );
    }
}
