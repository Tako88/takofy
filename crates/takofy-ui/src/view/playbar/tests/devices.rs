use super::super::*;
use super::test_support::*;

#[test]
fn the_devices_run_names_the_button_and_lights_with_the_holder() {
    assert_eq!(devices_run(Button::Off).label, "Connect to a device");
    assert_eq!(devices_run(Button::Ready).label, "Connect to a device");
    assert!(!devices_run(Button::Off).lit);
    assert!(!devices_run(Button::Ready).lit);
    assert!(devices_run(Button::Lit).lit);
    for devices in [Button::Off, Button::Ready, Button::Lit] {
        let bar = Bar {
            devices,
            ..Bar::default()
        };
        let run = *extras_runs(Playbar::Full, bar, &Volume::default())
            .iter()
            .find(|run| run.control == Control::Devices)
            .expect("the full extras draw the devices button");
        assert_eq!(run.label, "Connect to a device");
        assert_eq!(run.lit, devices == Button::Lit, "{devices:?}");
    }
    assert!(
        extras_runs(Playbar::Reduced, Bar::default(), &Volume::default())
            .iter()
            .all(|run| run.control != Control::Devices),
        "the reduced extras have no devices button to draw"
    );
}

#[test]
fn the_devices_button_is_drawn_where_a_press_reaches_it() {
    let tokens = Tokens::DARK;
    let ctx = context(&tokens);
    let mut queue = queue_of(4);
    let form = Playbar::Full;
    let at = devices_button(heart_bar(), form).expect("the full bar draws it");
    let bar = Bar {
        devices: Button::Ready,
        ..Bar::default()
    };

    let replies = click_at(&ctx, &tokens, &mut queue, form, bar, at.center());

    assert!(
        replies.iter().any(|reply| reply.devices.is_some()),
        "the press at {at:?} did not reach the devices button: {replies:?}"
    );
    assert!(
        replies.iter().all(|reply| reply.command.is_none()),
        "opening a list is not a command: {replies:?}"
    );
    assert!(devices_button(heart_bar(), Playbar::Reduced).is_none());
}

#[test]
fn a_devices_button_with_no_list_reports_nothing() {
    let tokens = Tokens::DARK;
    let ctx = context(&tokens);
    let mut queue = queue_of(4);
    let at = devices_button(heart_bar(), Playbar::Full).expect("the full bar draws it");
    let replies = click_at(
        &ctx,
        &tokens,
        &mut queue,
        Playbar::Full,
        Bar::default(),
        at.center(),
    );
    assert!(
        replies.iter().all(|reply| reply.devices.is_none()),
        "a button with no list still opened one: {replies:?}"
    );
}

#[test]
fn a_lit_devices_button_paints_the_accent() {
    let tokens = Tokens::DARK;
    let ctx = context(&tokens);
    let mut queue = queue_of(4);
    let at = devices_button(heart_bar(), Playbar::Full).expect("the full bar draws it");
    for (devices, lit) in [
        (Button::Lit, true),
        (Button::Ready, false),
        (Button::Off, false),
    ] {
        let bar = Bar {
            devices,
            ..Bar::default()
        };
        let (_, output) = queue_frame(&ctx, &tokens, &mut queue, Playbar::Full, bar, Vec::new());
        assert_eq!(accent_inside(&output, &tokens, at), lit, "{devices:?}");
    }
}
