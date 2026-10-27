use super::super::*;
use super::test_support::*;
use egui::RawInput;

/// The title and the artist are one block, centred on the art: stacking
/// them from fixed offsets left the pair sitting low.
#[test]
fn the_now_text_is_centred_on_the_art() {
    let tokens = Tokens::DARK;
    let ctx = context(&tokens);
    let bar = Rect::from_min_size(Pos2::new(0.0, 100.0), Vec2::new(1200.0, extent::PLAYBAR_H));
    let mut transport = playing(0);
    transport.now_playing = Some(crate::player::NowPlaying {
        uri: "spotify:track:signal-decay".to_owned(),
        title: "Signal Decay".to_owned(),
        artists: "Nils Aker".to_owned(),
        artist_uri: Some("spotify:artist:nils-aker".to_owned()),
        ..Default::default()
    });

    let mut output = ctx.run_ui(
        RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1200.0, 300.0))),
            ..Default::default()
        },
        |ui| {
            let mut queue = Queue::default();
            let _ = show(
                ui,
                &mut transport,
                &mut queue,
                0,
                Playbar::Full,
                Bar::default(),
                None,
                bar,
                Cover::Missing,
                &tokens,
            );
        },
    );
    output.textures_delta.clear();

    let painted = |label: &str| {
        output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text) if text.galley.text() == label => {
                    Some(Rect::from_min_size(text.pos, text.galley.size()))
                }
                _ => None,
            })
    };
    let title = painted("Signal Decay").expect("the bar paints the title");
    let artist = painted("Nils Aker").expect("the bar paints the artist");
    let block = title.union(artist);
    let centre = bar.center().y;
    assert!(
        (block.center().y - centre).abs() < 0.01,
        "the title and artist block centres at {} rather than {centre}",
        block.center().y
    );
}

/// Every column centres on the bar, which is what `align-items: center` does
/// in the reference and what a row's cursor does not do on its own.
#[test]
fn every_column_sits_on_the_bars_centre() {
    let tokens = Tokens::DARK;
    let ctx = context(&tokens);
    let bar = Rect::from_min_size(Pos2::new(0.0, 100.0), Vec2::new(1200.0, extent::PLAYBAR_H));
    let mut transport = playing(0);

    let mut output = ctx.run_ui(
        RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1200.0, 300.0))),
            ..Default::default()
        },
        |ui| {
            let mut queue = Queue::default();
            let _ = show(
                ui,
                &mut transport,
                &mut queue,
                0,
                Playbar::Full,
                Bar::default(),
                None,
                bar,
                Cover::Missing,
                &tokens,
            );
        },
    );
    output.textures_delta.clear();

    let centre = bar.center().y;
    let boxes = |height: f32| {
        output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Rect(shape) if (shape.rect.height() - height).abs() < 0.01 => {
                    Some(shape.rect)
                }
                _ => None,
            })
            .collect::<Vec<_>>()
    };

    // The art is the now column's tallest box, so it holds the column's
    // height: its centre is the bar's.
    let art = boxes(extent::PLAYBAR_ART);
    assert!(!art.is_empty(), "the now column draws its art");
    assert!(
        (art[0].center().y - centre).abs() < 0.01,
        "the art at {:?} is not on the bar's centre {centre}",
        art[0].center()
    );

    // The transport and the seek row are one stack, centred together, so the
    // play button's top is half the stack above the bar's centre.
    let stack = layout::playbar::transport_stack();
    let play = boxes(extent::BUTTON_LG);
    assert!(!play.is_empty(), "the centre column draws the play button");
    assert!(
        (play[0].top() - (centre - stack / 2.0)).abs() < 0.01,
        "the transport stack starts at {} rather than {}",
        play[0].top(),
        centre - stack / 2.0
    );

    // The extras' volume is the one box there whose width is its own token,
    // so it identifies the run without guessing a coordinate.
    let volume = output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Rect(shape) if (shape.rect.width() - extent::VOLUME_W).abs() < 0.01 => {
                Some(shape.rect)
            }
            _ => None,
        })
        .next()
        .expect("the extras draw the volume");
    assert!(
        (volume.center().y - centre).abs() < 0.01,
        "the volume at {:?} is not on the bar's centre {centre}",
        volume.center()
    );
}

#[test]
fn an_idle_bar_centres_its_one_line_on_the_art() {
    let tokens = Tokens::DARK;
    let ctx = context(&tokens);
    let mut transport = Transport::default();
    assert_eq!(transport.state, TransportState::Idle);
    let (output, _) = bar_frame(
        &ctx,
        &tokens,
        &mut transport,
        Playbar::Full,
        Some(false),
        Vec::new(),
    );
    let line = painted_text(&output, "Nothing playing");
    let centre = heart_bar().center().y;
    assert!(
        (line.center().y - centre).abs() < 0.01,
        "the idle line centres at {} rather than {centre}",
        line.center().y
    );
}

#[test]
fn a_track_with_no_artist_centres_the_title_alone() {
    let tokens = Tokens::DARK;
    let ctx = context(&tokens);
    let mut transport = playing(0);
    transport.now_playing = Some(crate::player::NowPlaying {
        uri: "spotify:track:signal-decay".to_owned(),
        title: "Signal Decay".to_owned(),
        ..Default::default()
    });
    let (output, _) = bar_frame(
        &ctx,
        &tokens,
        &mut transport,
        Playbar::Full,
        Some(false),
        Vec::new(),
    );
    let title = painted_text(&output, "Signal Decay");
    let centre = heart_bar().center().y;
    assert!(
        (title.center().y - centre).abs() < 0.01,
        "the lone title centres at {} rather than {centre}",
        title.center().y
    );
}

/// The bar's height is its region's, and the second line keeps the font the
/// state word had: no state or artist makes the words taller than the art.
#[test]
fn the_playbar_is_not_taller_than_before() {
    let tokens = Tokens::DARK;
    let cases = [
        ("idle", false, Transport::default()),
        ("playing", false, linked_playing()),
        ("remote", true, linked_playing()),
    ];
    for (case, holds, mut transport) in cases {
        let ctx = context(&tokens);
        let holder = holds.then_some(Remote {
            line: "Living room",
            steerable: true,
        });
        let output = bar_of(&ctx, &tokens, &mut transport, holder);
        let now = layout::playbar_for(heart_bar(), Playbar::Full).columns(heart_bar())[0];
        let block = output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Text(shape) if now.contains(shape.pos) => {
                    Some(Rect::from_min_size(shape.pos, shape.galley.size()))
                }
                _ => None,
            })
            .reduce(|a, b| a.union(b))
            .expect("the bar paints its words");
        assert!(
            block.height() <= extent::PLAYBAR_ART,
            "{case}: the words take {} of the {} art",
            block.height(),
            extent::PLAYBAR_ART
        );
        assert!(
            block.top() >= heart_bar().top() && block.bottom() <= heart_bar().bottom(),
            "{case}: the words {block:?} leave the bar {:?}",
            heart_bar()
        );
    }
}
