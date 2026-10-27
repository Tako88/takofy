use egui::{Event, Modifiers, PointerButton, Pos2, RawInput, Rect, pos2, vec2};

use super::super::*;
use super::test_support::*;

#[test]
fn any_payload_counts_as_carrying() {
    // The gates ask whether a block is held, not whose it is, so a test that
    // only needs them held shut may set any payload type at all.
    let ctx = context();
    assert!(!carrying(&ctx), "an empty context carries nothing");
    egui::DragAndDrop::set_payload(&ctx, ());
    assert!(carrying(&ctx), "a foreign payload is still a carry");
    assert!(
        carried(&ctx).is_none(),
        "a foreign payload is no block of ours"
    );
    egui::DragAndDrop::clear_payload(&ctx);
    assert!(!carrying(&ctx));
}

#[test]
fn a_pick_of_tracks_travels_whole_and_in_order() {
    let first = TrackEntry {
        uri: "spotify:track:4uLU6hMCjMI75M1A2tKUQC".to_owned(),
        ..entry("a")
    };
    let second = TrackEntry {
        uri: "spotify:track:4uLU6hMCjMI75M1A2tKUQD".to_owned(),
        ..first.clone()
    };
    let block = Carry {
        entries: vec![first.clone(), second.clone()],
        rows: vec![1, 2],
        ..carry(Source::Queue, Vec::new())
    };

    assert_eq!(
        block.pick(),
        Some(vec![first.uri.clone(), second.uri.clone()])
    );
    assert!(block.addable());
}

#[test]
fn a_playlist_an_episode_and_a_local_file_are_not_picks() {
    let named = TrackEntry {
        uri: "spotify:track:4uLU6hMCjMI75M1A2tKUQC".to_owned(),
        ..entry("a")
    };

    let playlist = Carry {
        rows: vec![1],
        playlist: Some("spotify:playlist:a".to_owned()),
        ..carry(Source::Playlist, Vec::new())
    };
    assert_eq!(playlist.pick(), None, "a playlist is no pick");
    assert!(!playlist.addable());

    for (uri, why) in [
        ("spotify:episode:1", "an episode is no track"),
        ("spotify:local:A:B:C:1000", "a local file is no track"),
    ] {
        let carry = Carry {
            rows: vec![1],
            entries: vec![TrackEntry {
                uri: uri.to_owned(),
                ..named.clone()
            }],
            ..carry(Source::Queue, Vec::new())
        };
        assert_eq!(carry.pick(), None, "{why}");
        assert!(!carry.addable(), "{why}");
    }

    let no_entries = carry(Source::Queue, vec![3]);
    assert_eq!(no_entries.pick(), None, "the rows and the entries disagree");
}

fn input(events: Vec<Event>) -> RawInput {
    RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(400.0, 400.0))),
        events,
        ..Default::default()
    }
}

#[test]
fn from_queue_carries_the_selected_block_or_just_the_lifted_row() {
    let mut queue = queue_of(6, 0);
    queue.press(2, Press::Replace);
    queue.press(4, Press::Toggle);
    let block = Lift {
        index: 4,
        size: vec2(272.0, 57.0),
        ghost: lifted(),
    };
    let carry = from_queue(&mut queue, block).expect("a carry");
    assert_eq!(carry.source, Source::Queue);
    assert_eq!(carry.rows, vec![2, 4]);
    assert_eq!(carry.entries.len(), 2);
    assert_eq!(carry.size, vec2(272.0, 57.0));

    // Lifting a row that was not picked travels alone.
    let mut queue = queue_of(6, 0);
    let carry = from_queue(
        &mut queue,
        Lift {
            index: 3,
            size: vec2(272.0, 57.0),
            ghost: lifted(),
        },
    )
    .expect("a carry");
    assert_eq!(carry.rows, vec![3]);
    assert_eq!(carry.entries.len(), 1);

    // The current entry is never lifted.
    let mut queue = queue_of(6, 2);
    assert_eq!(
        from_queue(
            &mut queue,
            Lift {
                index: 2,
                size: vec2(272.0, 57.0),
                ghost: lifted(),
            },
        ),
        None
    );
}

#[test]
fn nothing_is_carried_after_a_release() {
    let ctx = context();
    lift(&ctx, carry(Source::Queue, vec![1]));
    assert!(carrying(&ctx));

    // egui clears the payload at the end of the pass that sees the release.
    let at = pos2(10.0, 10.0);
    for event in [
        Event::PointerMoved(at),
        Event::PointerButton {
            pos: at,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        },
        Event::PointerButton {
            pos: at,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        },
    ] {
        let mut output = ctx.run_ui(input(vec![event]), |_ui| {});
        output.textures_delta.clear();
    }

    assert!(!carrying(&ctx));
    assert!(carried(&ctx).is_none());
}
