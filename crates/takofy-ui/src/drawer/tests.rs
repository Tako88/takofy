use super::*;

fn on_rail() -> Input {
    Input {
        rail: true,
        ..Input::default()
    }
}

#[test]
fn toggle_opens_and_closes_on_the_rail() {
    let open = Drawer::Closed.step(Input {
        toggle: true,
        ..on_rail()
    });
    assert_eq!(open, Drawer::Open);
    assert_eq!(
        open.step(Input {
            toggle: true,
            ..on_rail()
        }),
        Drawer::Closed
    );
}

#[test]
fn escape_closes() {
    assert_eq!(
        Drawer::Open.step(Input {
            escape: true,
            ..on_rail()
        }),
        Drawer::Closed
    );
}

#[test]
fn a_click_outside_closes() {
    assert_eq!(
        Drawer::Open.step(Input {
            clicked_outside: true,
            ..on_rail()
        }),
        Drawer::Closed
    );
}

#[test]
fn toggle_beats_an_outside_click_in_the_same_frame() {
    assert_eq!(
        Drawer::Closed.step(Input {
            toggle: true,
            clicked_outside: true,
            ..on_rail()
        }),
        Drawer::Open
    );
}

#[test]
fn picking_a_playlist_closes_the_drawer() {
    assert_eq!(
        Drawer::Open.step(Input {
            selected: true,
            ..on_rail()
        }),
        Drawer::Closed
    );
}

#[test]
fn widening_past_the_rail_closes() {
    assert_eq!(
        Drawer::Open.step(Input {
            rail: false,
            ..Input::default()
        }),
        Drawer::Closed
    );
}

#[test]
fn narrowing_again_does_not_reopen() {
    let widened = Drawer::Open.step(Input {
        rail: false,
        ..Input::default()
    });
    assert_eq!(widened, Drawer::Closed);
    assert_eq!(widened.step(on_rail()), Drawer::Closed);
}

#[test]
fn off_the_rail_nothing_opens() {
    assert_eq!(
        Drawer::Closed.step(Input {
            rail: false,
            toggle: true,
            ..Input::default()
        }),
        Drawer::Closed
    );
}
