use egui::{Context, Event, Pos2, Rect, Vec2, epaint::TextShape};

use super::*;
use crate::route::Page;
use crate::view::test_support::click_events;

/// The artist the band's credit names, and the page a click asks for.
const ARTIST: &str = "spotify:artist:nils";
const PROBE: Vec2 = Vec2::new(1200.0, 400.0);

fn context() -> Context {
    let tokens = Tokens::DARK;
    let ctx = Context::default();
    ctx.set_fonts(crate::theme::fonts::definitions());
    ctx.set_global_style(crate::theme::style(&tokens));
    ctx
}

fn input(events: Vec<Event>) -> egui::RawInput {
    egui::RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, PROBE)),
        events,
        ..Default::default()
    }
}

/// A band with the album's shape and `credit` over `meta`.
fn band<'a>(credit: Option<Credit<'a>>, meta: Vec<String>) -> Band<'a> {
    Band {
        eyebrow: "Album",
        title: "Signal Decay",
        meta,
        description: "",
        cover: crate::cover::Cover::Missing,
        tint: None,
        credit,
    }
}

/// A band whose credit names `ARTIST`.
fn linked<'a>(name: &'a str) -> Band<'a> {
    band(
        Credit::new(name, Some(Target::Credit(ARTIST))),
        vec!["1997".to_owned(), "12 tracks".to_owned()],
    )
}

fn draw(ctx: &Context, band: &Band<'_>, events: Vec<Event>) -> (Reply, egui::FullOutput) {
    let tokens = Tokens::DARK;
    let mut reply = None;
    let mut output = ctx.run_ui(input(events), |ui| {
        reply = Some(show_with(ui, band, &tokens, Title::Plain));
    });
    output.textures_delta.clear();
    (reply.expect("the band is drawn"), output)
}

/// One click on `at`, returning the release pass's reply.
fn click(ctx: &Context, band: &Band<'_>, at: Pos2) -> Reply {
    let mut last = None;
    for event in click_events(at) {
        last = Some(draw(ctx, band, vec![event]).0);
    }
    last.expect("the click runs")
}

/// The painted line with `label`, as the frame drew it.
fn painted<'a>(output: &'a egui::FullOutput, label: &str) -> Option<&'a TextShape> {
    output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::epaint::Shape::Text(text) if text.galley.text() == label => Some(text),
            _ => None,
        })
}

/// The box the painted line occupies.
fn rect_of(text: &TextShape) -> Rect {
    Rect::from_min_size(text.pos, text.galley.size())
}

/// Every text the frame painted.
fn drawn_texts(output: &egui::FullOutput) -> Vec<&str> {
    output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::epaint::Shape::Text(text) => Some(text.galley.text()),
            _ => None,
        })
        .collect()
}

/// Every label a pass gave a link node.
fn link_labels(output: &egui::FullOutput) -> Vec<String> {
    output
        .platform_output
        .accesskit_update
        .as_ref()
        .map(|update| {
            update
                .nodes
                .iter()
                .filter_map(|(_, node)| {
                    (node.role() == egui::accesskit::Role::Link)
                        .then(|| node.label().map(str::to_owned))
                        .flatten()
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The rect the band paints its wash into.
fn band_rect(output: &egui::FullOutput) -> Rect {
    let tokens = Tokens::DARK;
    output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::epaint::Shape::Rect(rect) if rect.fill == tokens.palette.hero_wash => {
                Some(rect.rect)
            }
            _ => None,
        })
        .expect("the band paints its own wash")
}

/// The column the band's words are laid out in, solved the way the band
/// solves it from the wash it paints.
fn column(output: &egui::FullOutput) -> Rect {
    let band = band_rect(output);
    let left = band.left() + space::GUTTER + hero::ART + space::S5;
    Rect::from_min_max(
        Pos2::new(left, band.top()),
        Pos2::new(band.right() - space::GUTTER, band.bottom()),
    )
}

#[test]
fn a_band_with_a_plain_title_reports_nothing() {
    let ctx = context();
    let (reply, _) = draw(&ctx, &band(None, Vec::new()), Vec::new());
    assert!(
        !reply.rename_clicked,
        "an untouched frame asked for nothing"
    );
    assert_eq!(reply.title_id, None, "a painted title carries no id");
    assert_eq!(reply.open, None, "a plain band opens nothing");
}

/// The rects the frame filled with `color`, in paint order.
fn fills(output: &egui::FullOutput, color: egui::Color32) -> Vec<Rect> {
    output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::epaint::Shape::Rect(shape) if shape.fill == color => Some(shape.rect),
            _ => None,
        })
        .collect()
}

fn band_marker_ink() -> egui::Color32 {
    Tokens::DARK.palette.border
}
#[test]
fn a_tinted_band_paints_its_tint_then_the_hairline() {
    let ctx = context();
    let tint = crate::theme::tint_color([116, 27, 27]);
    let mut band = band(None, Vec::new());
    band.tint = Some(tint);
    let (_, output) = draw(&ctx, &band, Vec::new());

    let tinted = fills(&output, tint);
    assert_eq!(tinted.len(), 1, "one wash over the whole band");
    assert_eq!(tinted[0].width(), PROBE.x, "the tint spans the band");
    assert_eq!(
        tinted[0],
        Rect::from_min_size(
            Pos2::ZERO,
            Vec2::new(PROBE.x, (hero::ART + 2.0 * space::S5).max(hero::MIN_H))
        ),
        "the tint covers the band and nothing else"
    );
    assert!(
        fills(&output, Tokens::DARK.palette.hero_wash).is_empty(),
        "a tinted band does not also paint the fallback"
    );

    let tinted_at = output
        .shapes
        .iter()
        .position(|clipped| {
            matches!(&clipped.shape, egui::epaint::Shape::Rect(shape) if shape.fill == tint)
        })
        .expect("the tint is painted");
    let hairline_at = output
        .shapes
        .iter()
        .position(|clipped| {
            matches!(&clipped.shape, egui::epaint::Shape::Rect(shape)
                if shape.fill == band_marker_ink()
                    && (shape.rect.height() - stroke::HAIRLINE).abs() < 0.01
                    && (shape.rect.bottom() - tinted[0].bottom()).abs() < 0.01)
        })
        .expect("the band closes with a hairline");
    assert!(
        tinted_at < hairline_at,
        "the tint is painted under the rule"
    );
}

#[test]
fn an_untinted_band_paints_the_wash() {
    let ctx = context();
    let tokens = Tokens::DARK;
    let [_, _, _, alpha] = tokens.palette.hero_wash.to_srgba_unmultiplied();
    assert_eq!(alpha, 255, "the fallback is opaque");
    let (_, output) = draw(&ctx, &band(None, Vec::new()), Vec::new());
    assert_eq!(
        fills(&output, tokens.palette.hero_wash),
        vec![band_rect(&output)]
    );
}

#[test]
fn a_blank_credit_is_none() {
    assert!(
        Credit::new("  ", Some(Target::Credit(ARTIST))).is_none(),
        "a blank name is no credit, whatever page it would open"
    );
    assert_eq!(
        Credit::new("  Nils Aker ", None).map(|credit| credit.name),
        Some("Nils Aker"),
        "a credit is its trimmed name"
    );
}

#[test]
fn the_credit_is_a_link_followed_by_plain_facts() {
    let tokens = Tokens::DARK;
    let ctx = context();
    let band = linked("Nils Aker");
    let (reply, output) = draw(&ctx, &band, Vec::new());
    assert_eq!(reply.open, None, "a drawn frame asked for no page");

    let credit = painted(&output, "Nils Aker").expect("the credit is painted");
    let facts = painted(&output, " · 1997 · 12 tracks").expect("the facts are painted");
    let credit = rect_of(credit);
    let facts = rect_of(facts);
    assert!(
        (facts.left() - credit.right()).abs() < 0.5,
        "the facts start at {} past the credit ending at {}",
        facts.left(),
        credit.right()
    );
    assert!(
        (facts.center().y - credit.center().y).abs() < 0.5,
        "the facts sit on the credit's line, not under it"
    );
    assert_eq!(
        painted(&output, " · 1997 · 12 tracks").map(|text| text.fallback_color),
        Some(tokens.palette.text_muted),
        "the facts are plain byline ink"
    );

    let reply = click(&ctx, &band, credit.center());
    assert_eq!(
        reply.open,
        Some(Page::Artist(ARTIST.to_owned())),
        "the credit is a link to the artist it names"
    );
    let reply = click(&ctx, &band, facts.center());
    assert_eq!(reply.open, None, "the facts are plain, not a link");
}

#[test]
fn a_credit_without_a_target_is_painted_as_one_line() {
    let tokens = Tokens::DARK;
    let ctx = context();
    let band = band(
        Credit::new("Nils Aker", None),
        vec!["1997".to_owned(), "12 tracks".to_owned()],
    );
    let (reply, output) = draw(&ctx, &band, Vec::new());
    assert_eq!(reply.open, None);
    assert!(
        drawn_texts(&output).contains(&"Nils Aker · 1997 · 12 tracks"),
        "an unknown page reads as one plain line: {:?}",
        drawn_texts(&output)
    );
    assert_eq!(
        drawn_texts(&output).len(),
        3,
        "one byline, one title and one eyebrow: {:?}",
        drawn_texts(&output)
    );
    let line = rect_of(painted(&output, "Nils Aker · 1997 · 12 tracks").expect("the byline"));
    assert_eq!(
        painted(&output, "Nils Aker · 1997 · 12 tracks").map(|text| text.fallback_color),
        Some(tokens.palette.text_muted),
        "the plain byline is byline ink"
    );
    assert_eq!(
        click(&ctx, &band, line.center()).open,
        None,
        "plain words open nothing"
    );
}

#[test]
fn a_long_credit_elides_and_hides_the_facts() {
    let ctx = context();
    let name = "Nils Aker And The Long Ensemble ".repeat(10);
    let band = linked(name.trim());
    let (_, output) = draw(&ctx, &band, Vec::new());
    let credit = painted(&output, name.trim()).expect("the credit is painted");
    assert!(credit.galley.elided, "a credit past the column must elide");
    let credit = rect_of(credit);
    assert!(
        credit.right() <= column(&output).right() + 0.5,
        "the elided credit ends at {} past the column {}",
        credit.right(),
        column(&output).right()
    );
    assert!(
        !drawn_texts(&output)
            .iter()
            .any(|text| text.starts_with(" · ")),
        "the facts were painted after a credit that filled the column: {:?}",
        drawn_texts(&output)
    );
}

#[test]
fn the_facts_never_overflow_the_column() {
    let ctx = context();
    let facts = vec!["38 min 58 sec".to_owned(); 20];
    let band = band(
        Credit::new("Nils Aker", Some(Target::Credit(ARTIST))),
        facts.clone(),
    );
    let (_, output) = draw(&ctx, &band, Vec::new());
    let label = format!(" · {}", facts.join(" · "));
    let words = painted(&output, &label).expect("the facts are painted");
    assert!(words.galley.elided, "long facts must elide, not overflow");
    let words = rect_of(words);
    let column = column(&output);
    assert!(
        words.right() <= column.right() + 0.5,
        "the facts end at {} past the column {}",
        words.right(),
        column.right()
    );
    assert!(
        words.left() > column.left(),
        "the facts start at the column's own edge"
    );
}

#[test]
fn the_credit_is_painted_in_the_muted_ink() {
    let tokens = Tokens::DARK;
    let ctx = context();
    let band = linked("Nils Aker");
    let (_, output) = draw(&ctx, &band, Vec::new());
    let credit = painted(&output, "Nils Aker").expect("the credit is painted");
    assert_eq!(
        credit.fallback_color, tokens.palette.text_muted,
        "a resting credit is byline ink"
    );
    assert_ne!(
        credit.fallback_color, tokens.palette.text,
        "and only the hover lifts it to full ink"
    );
}

#[test]
fn a_linked_band_is_as_tall_as_a_plain_one() {
    let ctx = context();
    let facts = vec!["1997".to_owned(), "12 tracks".to_owned()];
    let shape = |band: &Band<'_>| {
        let (_, output) = draw(&ctx, band, Vec::new());
        let title = painted(&output, band.title).expect("the title is painted");
        (title.pos, band_rect(&output))
    };
    let linked = shape(&linked("Nils Aker"));
    let plain = shape(&band(Credit::new("Nils Aker", None), facts));
    assert_eq!(
        linked.0, plain.0,
        "a link must not push the title up or down"
    );
    assert_eq!(
        linked.1, plain.1,
        "a link must not change the band's height"
    );
}

#[test]
fn the_facts_are_dropped_when_the_room_left_is_a_sliver() {
    let ctx = context();
    let mut sliver = None;
    // Each letter narrows what the facts would get. The last credit that
    // still fits therefore leaves less than one letter of room.
    for pad in 0..200 {
        let name = format!("Nils Aker{}", "n".repeat(pad));
        let band = linked(&name);
        let (_, output) = draw(&ctx, &band, Vec::new());
        let credit = painted(&output, &name).expect("the credit is painted");
        if credit.galley.elided {
            break;
        }
        let room = column(&output).right() - rect_of(credit).right();
        assert!(room >= 0.0, "the credit ends past the column");
        if room > 0.0 && sliver.as_ref().is_none_or(|(best, _)| room < *best) {
            sliver = Some((room, output));
        }
    }
    let (room, output) = sliver.expect("a credit fits the column without filling it");
    let texts = drawn_texts(&output);
    assert!(
        room < 10.0,
        "the sliver must be narrower than a fact, not {room} points"
    );
    assert!(
        !texts.iter().any(|text| text.starts_with(" · ")),
        "a credit leaving {room} points painted the facts anyway: {texts:?}"
    );
}

#[test]
fn a_credit_on_its_own_page_is_still_a_link() {
    // The same band heads the artist page this credit names. Nothing here
    // knows which page it is on, so the words stay a link and the open is
    // reported for the page to no-op on.
    let ctx = context();
    ctx.enable_accesskit();
    let band = linked("Nils Aker");
    let (_, output) = draw(&ctx, &band, Vec::new());
    assert_eq!(
        link_labels(&output),
        vec!["Open artist Nils Aker".to_owned()],
        "a credit on its own page still registers a link"
    );
    let credit = rect_of(painted(&output, "Nils Aker").expect("the credit"));
    assert_eq!(
        click(&ctx, &band, credit.center()).open,
        Some(Page::Artist(ARTIST.to_owned()))
    );
}
