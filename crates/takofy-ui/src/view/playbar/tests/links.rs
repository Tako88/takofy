use super::super::*;
use super::test_support::*;
use crate::view::test_support::click_events;
use egui::Event;

/// The ink and box of the painted words.
fn painted_ink(output: &egui::FullOutput, text: &str) -> (egui::Color32, Rect) {
    output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::Shape::Text(shape) if shape.galley.text() == text => Some((
                shape.fallback_color,
                Rect::from_min_size(shape.pos, shape.galley.size()),
            )),
            _ => None,
        })
        .unwrap_or_else(|| panic!("{text} is not painted"))
}

/// Clicks `at` over the passes egui's hit test needs, reporting every reply.
fn link_click(
    ctx: &egui::Context,
    tokens: &Tokens,
    transport: &mut Transport,
    remote: Option<Remote<'_>>,
    at: Pos2,
) -> Vec<Reply> {
    let mut queue = Queue::default();
    let form = Playbar::Full;
    let _ = remote_frame(
        ctx,
        tokens,
        transport,
        &mut queue,
        form,
        Bar::default(),
        remote,
        Vec::new(),
    );
    let mut replies = vec![
        remote_frame(
            ctx,
            tokens,
            transport,
            &mut queue,
            form,
            Bar::default(),
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
                &mut queue,
                form,
                Bar::default(),
                remote,
                vec![event],
            )
            .0,
        );
    }
    replies
}

#[test]
fn the_title_links_to_the_album() {
    let tokens = Tokens::DARK;
    let ctx = context(&tokens);
    let mut transport = linked_playing();
    let output = bar_of(&ctx, &tokens, &mut transport, None);
    let at = painted_text(&output, "Signal Decay").center();
    let replies = link_click(&ctx, &tokens, &mut transport, None, at);
    assert!(
        replies
            .iter()
            .any(|reply| reply.open == Some(Page::Album(LINK_ALBUM.to_owned()))),
        "a click on the title opened nothing: {replies:?}"
    );
}

#[test]
fn the_artist_links_to_the_artist() {
    let tokens = Tokens::DARK;
    let ctx = context(&tokens);
    let mut transport = linked_playing();
    let output = bar_of(&ctx, &tokens, &mut transport, None);
    let at = painted_text(&output, "Nils Aker").center();
    let replies = link_click(&ctx, &tokens, &mut transport, None, at);
    assert!(
        replies
            .iter()
            .any(|reply| reply.open == Some(Page::credit(LINK_ARTIST))),
        "a click on the artist opened nothing: {replies:?}"
    );
}

#[test]
fn a_remote_title_still_links() {
    let tokens = Tokens::DARK;
    let ctx = context(&tokens);
    let mut transport = linked_playing();
    let holder = remote("Living room", true);
    let output = bar_of(&ctx, &tokens, &mut transport, holder);
    let at = painted_text(&output, "Signal Decay").center();
    let replies = link_click(&ctx, &tokens, &mut transport, holder, at);
    assert!(
        replies
            .iter()
            .any(|reply| reply.open == Some(Page::Album(LINK_ALBUM.to_owned()))),
        "a click on the holder's title opened nothing: {replies:?}"
    );
}

#[test]
fn no_uri_means_plain() {
    let tokens = Tokens::DARK;
    let ctx = context(&tokens);
    ctx.enable_accesskit();
    let mut transport = named_playing();
    let output = bar_of(&ctx, &tokens, &mut transport, None);
    assert!(
        link_labels(&output).is_empty(),
        "a track with no uris made a link node: {:?}",
        link_labels(&output)
    );
    for words in ["Signal Decay", "Nils Aker"] {
        let at = painted_text(&output, words).center();
        let replies = link_click(&ctx, &tokens, &mut transport, None, at);
        assert!(
            replies.iter().all(|reply| reply.open.is_none()),
            "a click on {words:?} opened a page: {replies:?}"
        );
    }
}

#[test]
fn only_the_painted_words_are_clickable() {
    let tokens = Tokens::DARK;
    let ctx = context(&tokens);
    let mut transport = linked_playing();
    let output = bar_of(&ctx, &tokens, &mut transport, None);
    let title = painted_text(&output, "Signal Decay");
    let artist = painted_text(&output, "Nils Aker");
    assert!(
        artist.right() <= title.right(),
        "the title must be the wider line, so the probe clears both"
    );
    // egui hands the nearest clickable widget a press within a few points of
    // its box, so the probe clears the artist's words by more than that: the
    // hit is the painted line, not the column it sits in.
    let past = Pos2::new(artist.right() + 6.0, artist.center().y);
    assert!(
        past.x < title.right() + space::S3,
        "the probe {past:?} must stay inside the text column"
    );
    let replies = link_click(&ctx, &tokens, &mut transport, None, past);
    assert!(
        replies.iter().all(|reply| reply.open.is_none()),
        "a click beside the words opened a page: {replies:?}"
    );
}

#[test]
fn hover_on_the_artist_brightens_and_underlines() {
    let tokens = Tokens::DARK;
    let ctx = context(&tokens);
    let mut transport = linked_playing();
    let output = bar_of(&ctx, &tokens, &mut transport, None);
    let at = painted_text(&output, "Nils Aker").center();
    let mut events = vec![Event::PointerMoved(at)];
    let mut settled = None;
    for _ in 0..60 {
        let (_, output) = remote_frame(
            &ctx,
            &tokens,
            &mut transport,
            &mut Queue::default(),
            Playbar::Full,
            Bar::default(),
            None,
            events,
        );
        settled = Some(output);
        events = Vec::new();
    }
    let output = settled.expect("the hover settles");
    let (ink, words) = painted_ink(&output, "Nils Aker");
    assert_eq!(
        ink, tokens.palette.text,
        "the hovered artist did not brighten"
    );
    let rules: Vec<Rect> = output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Rect(shape)
                if (shape.rect.height() - crate::theme::stroke::HAIRLINE).abs() < 0.01 =>
            {
                Some(shape.rect)
            }
            _ => None,
        })
        .collect();
    assert_eq!(rules.len(), 1, "the hovered artist painted {rules:?}");
    let rule = rules[0];
    assert!(
        (rule.width() - words.width()).abs() < 0.01,
        "the underline spans {} under {} wide words",
        rule.width(),
        words.width()
    );
    assert!(
        rule.center().y > words.center().y,
        "the underline sits above the artist's centre"
    );
}
