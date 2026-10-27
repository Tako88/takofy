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
fn hover_at(ctx: &Context, list: &mut DeviceList, devices: &Devices, at: Pos2) -> egui::FullOutput {
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
        ["01", "02", "03", "04", "05", "06", "08"].map(|name| format!("Device {name}, Computer")),
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
