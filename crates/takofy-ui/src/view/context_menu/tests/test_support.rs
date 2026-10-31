//! Fixtures and probes shared by the context menu's tests.

use super::super::*;
use crate::menu::{Anchor, Device, Door, Gate, Item, Listing, Menu, Request};
use crate::theme::{fonts, style};
use egui::{Event, Id, Key, Modifiers, Pos2, RawInput, Rect, Shape, pos2, vec2};
use takofy_model::TrackEntry;

pub(super) fn context() -> egui::Context {
    let ctx = egui::Context::default();
    ctx.set_fonts(fonts::definitions());
    ctx.set_global_style(style(&Tokens::DARK));
    ctx.enable_accesskit();
    ctx
}

pub(super) fn screen() -> Rect {
    Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))
}

pub(super) fn input(events: Vec<Event>) -> RawInput {
    RawInput {
        screen_rect: Some(screen()),
        events,
        ..Default::default()
    }
}

pub(super) fn anchor() -> Anchor {
    Anchor::Button(Rect::from_min_size(pos2(600.0, 100.0), vec2(28.0, 28.0)))
}

pub(super) fn opener() -> Id {
    Id::unique("row-more")
}

pub(super) fn door(device: Device) -> Door {
    Door {
        anchor: anchor(),
        opener: opener(),
        device,
    }
}

pub(super) fn track() -> TrackEntry {
    TrackEntry {
        uri: "spotify:track:a".to_owned(),
        title: "Harbour Wall".to_owned(),
        artists: "Slow Tide".to_owned(),
        album: Some("A Record".to_owned()),
        album_uri: Some("spotify:album:a".to_owned()),
        artist_uris: vec!["spotify:artist:b".to_owned()],
        artist_names: vec!["Slow Tide".to_owned()],
        added_at: None,
        duration_ms: None,
        named: true,
        cover: None,
    }
}

/// The verbs one track gets on a page, in the sheet's order.
pub(super) fn items() -> Vec<Item> {
    verbs::track_items(
        &[track()],
        None,
        &Listing::Page,
        crate::likes::Standing::Unknown,
    )
}

pub(super) fn request(device: Device, items: Vec<Item>) -> Request {
    Request {
        door: door(device),
        items,
        playlists: Vec::new(),
        targets: vec![track()],
        rows: vec![0],
        listing: Listing::Page,
        page: None,
    }
}

pub(super) fn opened(device: Device) -> Menu {
    let mut menu = Menu::default();
    assert!(menu.open(request(device, items()), Gate::default()));
    menu
}

pub(super) fn opened_with(device: Device, items: Vec<Item>) -> Menu {
    let mut menu = Menu::default();
    assert!(menu.open(request(device, items), Gate::default()));
    menu
}

/// What one frame drew, and what the menu reported.
pub(super) struct Drawn {
    pub(super) closed: Option<Closed>,
    pub(super) output: egui::FullOutput,
}

/// Draws one frame of the open menu, and runs `read` inside it, where the
/// widget ids and the AccessKit nodes are live.
pub(super) fn draw<R>(
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

pub(super) fn frame(ctx: &egui::Context, menu: &mut Menu, free: bool, events: Vec<Event>) -> Drawn {
    draw(ctx, menu, free, events, |_, _| ()).0
}

/// The card's rect, read back from the one raised-surface fill the frame
/// painted.
pub(super) fn card(output: &egui::FullOutput) -> Rect {
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
pub(super) fn fills(output: &egui::FullOutput) -> Vec<(Rect, egui::Color32)> {
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
pub(super) fn fills_of(output: &egui::FullOutput, color: egui::Color32) -> Vec<Rect> {
    fills(output)
        .into_iter()
        .filter(|(_, fill)| *fill == color)
        .map(|(rect, _)| rect)
        .collect()
}

/// The colour and text of every line one frame painted.
pub(super) fn texts(output: &egui::FullOutput) -> Vec<(String, egui::Color32)> {
    output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            Shape::Text(text) => Some((text.galley.text().to_owned(), text.fallback_color)),
            _ => None,
        })
        .collect()
}

pub(super) fn key_event(key: Key, modifiers: Modifiers) -> Event {
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
pub(super) fn repeat(key: Key) -> Event {
    Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: true,
        modifiers: Modifiers::NONE,
    }
}

pub(super) fn press(ctx: &egui::Context, menu: &mut Menu, key: Key) -> Drawn {
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
pub(super) fn warm(ctx: &egui::Context, menu: &mut Menu) {
    let _ = frame(ctx, menu, true, Vec::new());
    let _ = frame(ctx, menu, true, Vec::new());
}
