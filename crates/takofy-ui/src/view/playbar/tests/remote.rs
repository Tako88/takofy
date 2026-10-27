use super::super::*;
use super::test_support::*;
use crate::view::test_support::click_events;
use egui::Event;

/// Clicks `at` while remote, over the passes egui's hit test needs.
fn remote_click(
    ctx: &egui::Context,
    tokens: &Tokens,
    transport: &mut Transport,
    queue: &mut Queue,
    bar: Bar,
    remote: Option<Remote<'_>>,
    at: Pos2,
) -> Vec<Reply> {
    let form = Playbar::Full;
    let _ = remote_frame(ctx, tokens, transport, queue, form, bar, remote, Vec::new());
    let mut replies = vec![
        remote_frame(
            ctx,
            tokens,
            transport,
            queue,
            form,
            bar,
            remote,
            vec![Event::PointerMoved(at)],
        )
        .0,
    ];
    for event in click_events(at) {
        replies.push(
            remote_frame(
                ctx,
                tokens,
                transport,
                queue,
                form,
                bar,
                remote,
                vec![event],
            )
            .0,
        );
    }
    replies
}

/// Where the play tile is drawn, solved here from the theme's own constants.
fn play_slot(centre: Rect) -> Rect {
    let stack = extent::BUTTON_LG + space::S1 + extent::SLIDER_H;
    Rect::from_min_size(
        Pos2::new(
            centre.center().x - extent::BUTTON_LG / 2.0,
            centre.center().y - stack / 2.0,
        ),
        Vec2::splat(extent::BUTTON_LG),
    )
}

#[test]
fn a_remote_bar_steers_from_an_empty_local_queue() {
    let tokens = Tokens::DARK;
    let ctx = context(&tokens);
    ctx.enable_accesskit();
    let form = Playbar::Full;
    let mut queue = Queue::default();
    assert!(
        !queue.can_previous() && !queue.can_next() && !queue.can_toggle(),
        "the local queue is empty"
    );
    let mut transport = playing(0);
    let centre = layout::playbar_for(heart_bar(), form).columns(heart_bar())[1];
    let playing = remote("Phone", true);

    for (control, at, steer) in [
        (
            Control::Previous,
            transport_slot(centre, Control::Previous).center(),
            Steer::SkipPrev,
        ),
        (
            Control::Next,
            transport_slot(centre, Control::Next).center(),
            Steer::SkipNext,
        ),
        (Control::PlayPause, play_slot(centre).center(), Steer::Pause),
    ] {
        let replies = remote_click(
            &ctx,
            &tokens,
            &mut transport,
            &mut queue,
            Bar::default(),
            playing,
            at,
        );
        assert!(
            replies.iter().any(|reply| reply.steer == Some(steer)),
            "{control:?}: the press did not steer {steer:?}: {replies:?}"
        );
        assert!(
            replies.iter().all(|reply| reply.command.is_none()),
            "{control:?}: a remote press reached the local player: {replies:?}"
        );
        assert!(
            replies
                .iter()
                .all(|reply| !reply.pane && !reply.fullscreen && reply.devices.is_none()),
            "{control:?}: a remote press pressed something else: {replies:?}"
        );
    }
    assert!(
        queue.current().is_none() && !queue.armed(),
        "a remote press moved the local queue"
    );
}

/// The colours a control's glyph is painted in, read from the shapes inside
/// its own rect.
fn glyph_colours(output: &egui::FullOutput, rect: Rect) -> Vec<egui::Color32> {
    output
        .shapes
        .iter()
        .filter(|clipped| rect.contains(clipped.shape.visual_bounding_rect().center()))
        .filter_map(|clipped| shape_colour(&clipped.shape))
        .collect()
}

/// Whether `control` is drawn as something that can act this frame.
fn drawn_live(
    ctx: &egui::Context,
    tokens: &Tokens,
    queue: &mut Queue,
    transport: &mut Transport,
    bar: Bar,
    remote: Option<Remote<'_>>,
    at: Rect,
) -> bool {
    let (_, output) = remote_frame(
        ctx,
        tokens,
        transport,
        queue,
        Playbar::Full,
        bar,
        remote,
        Vec::new(),
    );
    let colours = glyph_colours(&output, at);
    assert!(!colours.is_empty(), "nothing is painted at {at:?}");
    for colour in &colours {
        if *colour == tokens.palette.text_disabled {
            return false;
        }
    }
    true
}

#[test]
fn a_remote_press_never_moves_the_local_queue() {
    let tokens = Tokens::DARK;
    let ctx = context(&tokens);
    let mut queue = queue_of(4);
    assert_eq!(queue.first_upcoming(), 1, "the queue is on its first track");
    let mut transport = playing(0);
    let centre = layout::playbar_for(heart_bar(), Playbar::Full).columns(heart_bar())[1];
    let at = transport_slot(centre, Control::Next).center();

    let replies = remote_click(
        &ctx,
        &tokens,
        &mut transport,
        &mut queue,
        Bar::default(),
        remote("Phone", true),
        at,
    );

    assert!(
        replies
            .iter()
            .any(|reply| reply.steer == Some(Steer::SkipNext)),
        "the remote skip did not steer: {replies:?}"
    );
    assert_eq!(
        queue.first_upcoming(),
        1,
        "a remote skip moved the local queue"
    );
}

#[test]
fn a_remote_bar_draws_its_transport_live_with_an_empty_queue() {
    let tokens = Tokens::DARK;
    let ctx = context(&tokens);
    ctx.enable_accesskit();
    let mut queue = Queue::default();
    let mut transport = playing(0);
    let centre = layout::playbar_for(heart_bar(), Playbar::Full).columns(heart_bar())[1];

    for control in [Control::Previous, Control::Next] {
        assert!(
            drawn_live(
                &ctx,
                &tokens,
                &mut queue,
                &mut transport,
                Bar::default(),
                remote("Phone", true),
                transport_slot(centre, control),
            ),
            "{control:?} is drawn inert while the phone holds the sound"
        );
    }
    let (_, output) = remote_frame(
        &ctx,
        &tokens,
        &mut transport,
        &mut queue,
        Playbar::Full,
        Bar::default(),
        remote("Phone", true),
        Vec::new(),
    );
    assert!(
        !announced_disabled(&output, "Play"),
        "the play tile is drawn inert while the phone holds the sound"
    );
    for label in ["Shuffle off", "Repeat off", "Mute", "Volume", "Queue"] {
        assert!(
            announced_disabled(&output, label),
            "{label} is drawn live while another device holds the sound"
        );
    }
}

#[test]
fn a_remote_bar_that_cannot_steer_announces_its_transport_disabled() {
    let tokens = Tokens::DARK;
    let ctx = context(&tokens);
    ctx.enable_accesskit();
    let mut queue = Queue::default();
    let mut transport = playing(0);
    let (_, output) = remote_frame(
        &ctx,
        &tokens,
        &mut transport,
        &mut queue,
        Playbar::Full,
        Bar::default(),
        remote("Phone", false),
        Vec::new(),
    );
    for label in ["Play", "Seek", "Mute", "Volume", "Shuffle off", "Queue"] {
        assert!(
            announced_disabled(&output, label),
            "{label} is drawn live while the holder refuses control"
        );
    }
    let centre = layout::playbar_for(heart_bar(), Playbar::Full).columns(heart_bar())[1];
    for control in [Control::Previous, Control::Next] {
        assert!(
            !drawn_live(
                &ctx,
                &tokens,
                &mut queue,
                &mut transport,
                Bar::default(),
                remote("Phone", false),
                transport_slot(centre, control),
            ),
            "{control:?} is drawn live while the holder refuses control"
        );
    }

    for at in [
        transport_slot(
            layout::playbar_for(heart_bar(), Playbar::Full).columns(heart_bar())[1],
            Control::Previous,
        )
        .center(),
        play_slot(layout::playbar_for(heart_bar(), Playbar::Full).columns(heart_bar())[1]).center(),
    ] {
        let replies = remote_click(
            &ctx,
            &tokens,
            &mut transport,
            &mut queue,
            Bar::default(),
            remote("Phone", false),
            at,
        );
        assert!(
            replies
                .iter()
                .all(|reply| reply.steer.is_none() && reply.command.is_none()),
            "a holder that cannot be steered was steered: {replies:?}"
        );
    }
}

#[test]
fn a_remote_toggle_pauses_what_plays_and_resumes_what_is_paused() {
    let tokens = Tokens::DARK;
    let ctx = context(&tokens);
    let form = Playbar::Full;
    let centre = layout::playbar_for(heart_bar(), form).columns(heart_bar())[1];
    let at = play_slot(centre).center();

    for (state, steer) in [
        (TransportState::Playing, Steer::Pause),
        (TransportState::Loading, Steer::Pause),
        (TransportState::Paused, Steer::Resume),
    ] {
        let mut queue = Queue::default();
        let mut transport = Transport {
            state,
            duration_ms: Some(222_000),
            ..Transport::default()
        };
        let replies = remote_click(
            &ctx,
            &tokens,
            &mut transport,
            &mut queue,
            Bar::default(),
            remote("Phone", true),
            at,
        );
        assert!(
            replies.iter().any(|reply| reply.steer == Some(steer)),
            "{state:?} did not steer {steer:?}: {replies:?}"
        );
    }
}

#[test]
fn a_remote_scrub_seeks_the_holder() {
    let tokens = Tokens::DARK;
    let ctx = context(&tokens);
    let mut queue = Queue::default();
    let mut transport = playing(0);
    let centre = layout::playbar_for(heart_bar(), Playbar::Full).columns(heart_bar())[1];
    let (_, output) = remote_frame(
        &ctx,
        &tokens,
        &mut transport,
        &mut queue,
        Playbar::Full,
        Bar::default(),
        remote("Phone", true),
        Vec::new(),
    );
    let track = painted_track(&output, centre);
    let at = Pos2::new(track.center().x, track.center().y);

    let replies = remote_click(
        &ctx,
        &tokens,
        &mut transport,
        &mut queue,
        Bar::default(),
        remote("Phone", true),
        at,
    );

    assert!(
        replies
            .iter()
            .any(|reply| matches!(reply.steer, Some(Steer::SeekTo(ms)) if ms == 111_000)),
        "the scrub did not seek the phone: {replies:?}"
    );
    assert!(
        replies.iter().all(|reply| reply.command.is_none()),
        "a remote scrub reached the local player: {replies:?}"
    );
}

#[test]
fn a_remote_scrub_seeks_a_holder_that_is_still_loading() {
    // The slider is enabled on the holder's known length, so a scrub must
    // act on exactly that: a load is not a state where the bar goes dead.
    let tokens = Tokens::DARK;
    let ctx = context(&tokens);
    ctx.enable_accesskit();
    let mut queue = Queue::default();
    let mut transport = Transport {
        state: TransportState::Loading,
        duration_ms: Some(222_000),
        ..Transport::default()
    };
    let centre = layout::playbar_for(heart_bar(), Playbar::Full).columns(heart_bar())[1];
    let (_, output) = remote_frame(
        &ctx,
        &tokens,
        &mut transport,
        &mut queue,
        Playbar::Full,
        Bar::default(),
        remote("Phone", true),
        Vec::new(),
    );
    assert!(
        !announced_disabled(&output, "Seek"),
        "the slider is inert while the holder loads"
    );
    let track = painted_track(&output, centre);
    let at = Pos2::new(track.center().x, track.center().y);

    let replies = remote_click(
        &ctx,
        &tokens,
        &mut transport,
        &mut queue,
        Bar::default(),
        remote("Phone", true),
        at,
    );

    assert!(
        replies
            .iter()
            .any(|reply| matches!(reply.steer, Some(Steer::SeekTo(ms)) if ms == 111_000)),
        "a live slider yielded nothing: {replies:?}"
    );
}

#[test]
fn a_remote_bar_draws_the_caption_in_place_of_the_artist() {
    let tokens = Tokens::DARK;
    let ctx = context(&tokens);
    ctx.enable_accesskit();
    let mut queue = Queue::default();
    let mut transport = linked_playing();
    let (_, output) = remote_frame(
        &ctx,
        &tokens,
        &mut transport,
        &mut queue,
        Playbar::Full,
        Bar::default(),
        remote("Living room", true),
        Vec::new(),
    );
    let texts = text_inside(&output, heart_bar());
    assert!(
        texts.contains(&"Living room".to_owned()),
        "the caption is not drawn"
    );
    assert!(
        !texts.contains(&"Nils Aker".to_owned()),
        "the track's artist is drawn beside the caption"
    );
    assert_eq!(
        link_labels(&output),
        vec!["Open album Signal Decay".to_owned()],
        "the caption took a link of its own, or the title lost one"
    );
}

#[test]
fn steer_of_maps_only_the_scrubbers_output() {
    assert_eq!(
        steer_of(&Command::Seek {
            position_ms: 42_000
        }),
        Some(Steer::SeekTo(42_000))
    );
    for command in [
        Command::Play {
            uri: "t1".to_owned(),
        },
        Command::Pause,
        Command::Resume,
        Command::Stop,
        Command::Volume { percent: 50 },
    ] {
        assert_eq!(steer_of(&command), None, "{command:?}");
    }
}
