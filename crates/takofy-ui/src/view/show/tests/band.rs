use egui::{Context, Rect};
use takofy_model::{CoverImage, CoverSet, CoverUrl};

use super::super::*;
use super::test_support::*;
use crate::cover::Cover;
use crate::message::Update;
use crate::theme::space;

/// The action row's play tile, solved the way the page solves it.
fn play_tile_rect_for(ctx: &Context) -> Rect {
    let tokens = Tokens::DARK;
    let mut found = None;
    run(ctx, input(Vec::new()), |ui| {
        egui::ScrollArea::vertical().show(ui, |ui| {
            let band = hero::Band {
                eyebrow: "Show",
                title: "Lateral",
                meta: show_meta("Lateral", "Tom Scott Media", 2),
                credit: None,
                description: "A show about the long way round.",
                cover: Cover::Missing,
                tint: None,
            };
            let _ = hero::show_with(ui, &band, &tokens, hero::Title::Plain);
            let row = actionbar::row_rect(ui);
            found = Some(
                crate::layout::actionbar::solve(
                    row,
                    Kind::Other,
                    crate::layout::actionbar::Parts {
                        shuffle: false,
                        field: false,
                        follow: None,
                    },
                )
                .play,
            );
        });
    });
    found.expect("the band and its action row lay out")
}

#[test]
fn the_band_plays_the_drawn_order() {
    let ctx = context();
    let mut library = library_with(detail(vec![dated(1, 20), dated(2, 10)]));
    library.set_show_order(ShowOrder::Newest);
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    let tile = play_tile_rect_for(&ctx);
    let command = click(
        &ctx,
        tile.center(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );

    assert_eq!(
        command,
        Some(Command::Play {
            uri: "spotify:episode:2".to_owned()
        }),
        "the band starts the show from what the page draws on top"
    );
    assert!(
        queue.can_next(),
        "the whole show is queued, not just its first episode"
    );
}

#[test]
fn starting_from_the_band_uses_index_zero() {
    let ctx = context();
    let mut library = library_with(detail(vec![episode(1), episode(2)]));
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    let tile = play_tile_rect_for(&ctx);
    let command = click(
        &ctx,
        tile.center(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );

    assert_eq!(
        command,
        Some(Command::Play {
            uri: "spotify:episode:1".to_owned()
        }),
        "the band plays the show from its first episode"
    );
    assert!(
        queue.can_next(),
        "the whole show is queued, not just its first episode"
    );
}

/// The fill of the band's wash: the one full-pane-width rect as tall as the
/// band itself.
fn band_fill(output: &egui::FullOutput) -> Option<egui::Color32> {
    let height = (crate::theme::hero::ART + 2.0 * space::S5).max(crate::theme::hero::MIN_H);
    output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::epaint::Shape::Rect(rect)
                if (rect.rect.width() - PROBE.x).abs() < 0.5
                    && (rect.rect.height() - height).abs() < 0.5 =>
            {
                Some(rect.fill)
            }
            _ => None,
        })
}

#[test]
fn a_held_cover_tints_the_band() {
    let ctx = context();
    let tokens = Tokens::DARK;
    let url = CoverUrl::from_wire("https://i.scdn.co/image/tint").expect("a cover url");
    let set = CoverSet::single(url.clone());
    let mut detail = detail(vec![episode(1)]);
    detail.show.cover = Some(set.clone());
    let mut library = library_with(detail);
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();
    let now = Instant::now();

    art.begin_frame();
    let _ = art.look(Some(&set), 176, now);
    let _ = art.requests(now);
    art.apply(
        &Update::Cover {
            url,
            image: CoverImage::new(1, 1, vec![255, 0, 0, 255]).expect("a 1x1 image"),
        },
        now,
    );
    art.upload(&ctx, tokens.palette.tint.alpha_floor);

    let (_, output) = frame(
        &ctx,
        Vec::new(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    assert_eq!(
        band_fill(&output),
        Some(crate::theme::tint_color([116, 27, 27])),
        "the show's own mean tints its band"
    );
}

#[test]
fn a_loading_cover_leaves_the_band_on_the_wash() {
    let ctx = context();
    let url = CoverUrl::from_wire("https://i.scdn.co/image/loading").expect("a cover url");
    let mut detail = detail(vec![episode(1)]);
    detail.show.cover = Some(CoverSet::single(url));
    let mut library = library_with(detail);
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();
    let now = Instant::now();

    art.begin_frame();
    let (_, output) = frame(
        &ctx,
        Vec::new(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    assert!(
        !art.requests(now).is_empty(),
        "the band asks for the cover it has not got"
    );
    assert_eq!(
        band_fill(&output),
        Some(Tokens::DARK.palette.hero_wash),
        "an unheld cover leaves the fallback in place"
    );
}

#[test]
fn the_band_cover_is_fetched_once() {
    let ctx = context();
    let mut detail = detail(vec![episode(1), episode(2)]);
    detail.show.cover = Some(CoverSet::single(
        CoverUrl::from_wire("https://i.scdn.co/image/abc").expect("a cover url"),
    ));
    let mut library = library_with(detail);
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();
    let now = Instant::now();

    art.begin_frame();
    let _ = frame(
        &ctx,
        Vec::new(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    let requests = art.requests(now);

    assert_eq!(
        requests.len(),
        1,
        "the rows carry no art, so the band's cover is the one lookup: {requests:?}"
    );
    assert!(matches!(requests[0], Command::Fetch { .. }), "{requests:?}");
}
