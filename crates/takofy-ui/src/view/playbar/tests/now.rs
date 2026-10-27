use super::super::*;
use super::test_support::*;
use egui::Event;

/// The width `text` takes as one unclamped line in `font`.
fn natural_width(ctx: &egui::Context, text: &str, font: FontId) -> f32 {
    let mut width = 0.0;
    run(ctx, input(Vec::new()), |ui| {
        width = recipe::spaced(ui, text, font.clone(), Tokens::DARK.palette.text, 0.0)
            .size()
            .x;
    });
    width
}

/// A playing transport whose title is far wider than the now column.
fn wide_titled() -> Transport {
    let mut transport = named_playing();
    if let Some(now) = transport.now_playing.as_mut() {
        now.title = "A Signal That Decays Long After The Last Transmission Has Ended".to_owned();
    }
    transport
}

/// The fill a live control paints under the pointer, which is its own rect.
fn hover_fill(output: &egui::FullOutput) -> Option<Rect> {
    output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::Shape::Rect(shape) if (shape.rect.height() - extent::BUTTON).abs() < 0.01 => {
                Some(shape.rect)
            }
            _ => None,
        })
}

/// The now column's heart, read off the fill it paints when the pointer
/// rests on it. It is the column's only live control, so it is the only one
/// that paints a fill at all.
fn hovered_heart(
    ctx: &egui::Context,
    tokens: &Tokens,
    transport: &mut Transport,
    form: Playbar,
    column: Rect,
) -> Rect {
    let mut x = column.right() - 1.0;
    while x > column.left() {
        let at = Pos2::new(x, column.center().y);
        let _ = bar_frame(
            ctx,
            tokens,
            transport,
            form,
            Some(false),
            vec![Event::PointerMoved(at)],
        );
        let (output, _) = bar_frame(ctx, tokens, transport, form, Some(false), Vec::new());
        // The fill of the pass before lasts a frame as its hover eases out,
        // so only the fill the pointer is on says where the heart is.
        if let Some(rect) = hover_fill(&output).filter(|rect| rect.contains(at)) {
            return rect;
        }
        x -= 1.0;
    }
    panic!("the column paints no heart under the pointer");
}

#[test]
fn a_short_title_keeps_the_heart_and_add_button_beside_the_words() {
    let tokens = Tokens::DARK;
    let ctx = context(&tokens);
    let mut transport = named_playing();
    if let Some(now) = transport.now_playing.as_mut() {
        now.artists = "Wu".to_owned();
    }
    let (output, cols) = bar_frame(
        &ctx,
        &tokens,
        &mut transport,
        Playbar::Full,
        Some(false),
        Vec::new(),
    );
    let heart = hovered_heart(&ctx, &tokens, &mut transport, Playbar::Full, cols[0]);

    let title = painted_text(&output, "Signal Decay");
    let artist = painted_text(&output, "Wu");
    // The title is the wider line, so it is the one that sets the width.
    assert!(
        artist.right() <= title.right(),
        "the artist ends at {} past the title at {}",
        artist.right(),
        title.right()
    );
    let mut ellipsis = 0.0;
    run(&ctx, input(Vec::new()), |ui| {
        ellipsis = recipe::spaced(
            ui,
            "…",
            FontId::new(text::SM, fonts::sans(Weight::Medium)),
            tokens.palette.text,
            0.0,
        )
        .size()
        .x;
    });
    let gap = heart.left() - title.right();
    assert!(
        (gap - 12.0).abs() <= ellipsis,
        "the heart is {gap} from a title that fits, more than one glyph of slack"
    );
}

#[test]
fn a_long_title_elides_and_keeps_the_buttons_at_the_edge() {
    let tokens = Tokens::DARK;
    let ctx = context(&tokens);
    let mut transport = wide_titled();
    let title = transport
        .now_playing
        .as_ref()
        .map(|now| now.title.clone())
        .expect("the track is named");
    let (output, cols) = bar_frame(
        &ctx,
        &tokens,
        &mut transport,
        Playbar::Full,
        Some(false),
        Vec::new(),
    );
    let heart = hovered_heart(&ctx, &tokens, &mut transport, Playbar::Full, cols[0]);
    let controls_w = run_width(&NOW);

    assert!(
        (heart.left() - (cols[0].right() - controls_w)).abs() <= 1.0,
        "the controls start at {} rather than the column's edge less {controls_w}",
        heart.left()
    );
    let line = painted_text(&output, &title);
    assert!(
        line.right() <= heart.left() - 12.0 + 1e-3,
        "the title's galley ends at {} inside the heart's {}",
        line.right(),
        heart.left()
    );
}

#[test]
fn the_buttons_do_not_drift_when_the_artist_changes_under_an_elided_title() {
    let tokens = Tokens::DARK;
    let ctx = context(&tokens);
    let mut transport = wide_titled();
    let column = layout::playbar_for(heart_bar(), Playbar::Full).columns(heart_bar())[0];
    let narrow = hovered_heart(&ctx, &tokens, &mut transport, Playbar::Full, column);
    if let Some(now) = transport.now_playing.as_mut() {
        now.artists = "A Credit Long Enough To Run Far Past The Column It Is Drawn In".to_owned();
    }
    let wide = hovered_heart(&ctx, &tokens, &mut transport, Playbar::Full, column);
    assert_eq!(
        narrow, wide,
        "the controls moved when only the artist changed"
    );
}

/// A transport state is no line of the now column any more, so nothing there
/// may move when one state becomes another.
#[test]
fn changing_the_state_moves_nothing_in_the_now_column() {
    let tokens = Tokens::DARK;
    let ctx = context(&tokens);
    let mut transport = named_playing();
    let column = layout::playbar_for(heart_bar(), Playbar::Full).columns(heart_bar())[0];
    let (playing, _) = bar_frame(
        &ctx,
        &tokens,
        &mut transport,
        Playbar::Full,
        Some(false),
        Vec::new(),
    );
    let heart = hovered_heart(&ctx, &tokens, &mut transport, Playbar::Full, column);
    let words = painted_text(&playing, "Signal Decay");
    let credit = painted_text(&playing, "Nils Aker");
    for state in [
        TransportState::Loading,
        TransportState::Paused,
        TransportState::Unavailable,
        TransportState::Reconnecting,
        TransportState::Stopped,
        TransportState::Failed(crate::message::Failure::Load),
    ] {
        transport.state = state;
        let (output, _) = bar_frame(
            &ctx,
            &tokens,
            &mut transport,
            Playbar::Full,
            Some(false),
            Vec::new(),
        );
        assert_eq!(
            painted_text(&output, "Signal Decay"),
            words,
            "{state:?}: the state moved the title"
        );
        assert_eq!(
            painted_text(&output, "Nils Aker"),
            credit,
            "{state:?}: the state moved the artist"
        );
        assert_eq!(
            hovered_heart(&ctx, &tokens, &mut transport, Playbar::Full, column),
            heart,
            "{state:?}: the state moved the controls"
        );
    }
}

/// A short title with an artist wider than it: the wider line sets the
/// column's width, so the heart follows the artist.
#[test]
fn a_wide_artist_pushes_the_controls_right() {
    let tokens = Tokens::DARK;
    let ctx = context(&tokens);
    let credit = "Nils Aker And Vela Moreno";
    let mut transport = named_playing();
    if let Some(now) = transport.now_playing.as_mut() {
        now.artists = credit.to_owned();
    }
    let (output, cols) = bar_frame(
        &ctx,
        &tokens,
        &mut transport,
        Playbar::Full,
        Some(false),
        Vec::new(),
    );
    let heart = hovered_heart(&ctx, &tokens, &mut transport, Playbar::Full, cols[0]);
    let title = painted_text(&output, "Signal Decay");
    let artist = painted_text(&output, credit);
    let artist_w = natural_width(
        &ctx,
        credit,
        FontId::new(text::XXS, fonts::mono(MonoWeight::Regular)),
    );
    assert!(
        artist_w > title.width(),
        "the artist must be the wider line: {artist_w} against {}",
        title.width()
    );
    assert!(
        (artist.width() - artist_w).abs() < 0.01,
        "the artist elided to {} of {artist_w}, so it cannot set the width",
        artist.width()
    );
    let natural = heart.left() - title.left() - space::S3;
    assert!(
        (natural - artist_w).abs() < 0.01,
        "the controls sit at a natural width of {natural}, not the artist's {artist_w}"
    );
    let gap = heart.left() - artist.right();
    assert!(
        (gap - space::S3).abs() < 0.01,
        "the heart is {gap} from the artist, not the {} gap",
        space::S3
    );
    assert!(
        heart.left() > title.right() + space::S3,
        "the controls at {} did not move past the title ending at {}",
        heart.left(),
        title.right()
    );
}

/// The text face sets the width's shape, not the second line's font: a title
/// wider than its artist is what the controls follow.
#[test]
fn a_wide_title_still_sets_the_controls() {
    let tokens = Tokens::DARK;
    let ctx = context(&tokens);
    let heading = "A Signal That Decays";
    let mut transport = named_playing();
    if let Some(now) = transport.now_playing.as_mut() {
        now.title = heading.to_owned();
        now.artists = "Wu".to_owned();
    }
    let (output, cols) = bar_frame(
        &ctx,
        &tokens,
        &mut transport,
        Playbar::Full,
        Some(false),
        Vec::new(),
    );
    let heart = hovered_heart(&ctx, &tokens, &mut transport, Playbar::Full, cols[0]);
    let title = painted_text(&output, heading);
    let artist = painted_text(&output, "Wu");
    let title_w = natural_width(
        &ctx,
        heading,
        FontId::new(text::SM, fonts::sans(Weight::Medium)),
    );
    let artist_w = natural_width(
        &ctx,
        "Wu",
        FontId::new(text::XXS, fonts::mono(MonoWeight::Regular)),
    );
    assert!(
        title_w > artist_w,
        "the title must be the wider line: {title_w} against {artist_w}"
    );
    let natural = heart.left() - title.left() - space::S3;
    assert!(
        (natural - title_w).abs() < 0.01,
        "the controls sit at a natural width of {natural}, not the title's {title_w}"
    );
    assert!(
        artist.right() < title.right(),
        "the artist ends at {} past the title at {}",
        artist.right(),
        title.right()
    );
}

#[test]
fn no_state_word_is_painted() {
    let tokens = Tokens::DARK;
    let states = [
        TransportState::Idle,
        TransportState::Loading,
        TransportState::Playing,
        TransportState::Paused,
        TransportState::Unavailable,
        TransportState::Reconnecting,
        TransportState::Stopped,
        TransportState::Failed(crate::message::Failure::Load),
        TransportState::Failed(crate::message::Failure::Other),
    ];
    for state in states {
        let ctx = context(&tokens);
        let mut transport = named_playing();
        transport.state = state;
        let (output, _) = bar_frame(
            &ctx,
            &tokens,
            &mut transport,
            Playbar::Full,
            Some(false),
            Vec::new(),
        );
        let texts = text_inside(&output, heart_bar());
        for word in [
            "Loading",
            "Playing",
            "Paused",
            "Unavailable",
            "Reconnecting",
            "Stopped",
        ] {
            assert!(
                !texts.contains(&word.to_owned()),
                "{state:?}: the bar painted the state word {word:?}"
            );
        }
        for failure in [
            crate::message::Failure::Load,
            crate::message::Failure::Other,
        ] {
            let message = crate::signin::failure_message(failure);
            assert!(
                !texts.contains(&message.to_owned()),
                "{state:?}: the bar painted the failure {message:?}"
            );
        }
    }
}
