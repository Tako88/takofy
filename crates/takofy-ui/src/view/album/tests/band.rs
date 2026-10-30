use egui::{Context, Pos2};

use takofy_model::{AlbumDetail, CoverImage, CoverSet, CoverUrl};

use super::super::*;
use super::test_support::*;
use crate::cover::Cover;
use crate::message::Update;
use crate::theme::space;

/// A library on the album page with `detail` ready.
fn saved_album(uri: &str) -> takofy_model::SavedAlbum {
    takofy_model::SavedAlbum {
        saved_at: None,
        uri: uri.to_owned(),
        name: "Signal Decay".to_owned(),
        named: true,
        artists: "Band".to_owned(),
        artist_uris: Vec::new(),
        artist_names: Vec::new(),
        year: Some(2024),
        cover: None,
        tracks: Some(2),
    }
}

/// The action row heart's centre, solved by the row itself.
fn band_heart_point(ctx: &Context) -> Pos2 {
    let tokens = Tokens::DARK;
    let mut found = None;
    run(ctx, input(Vec::new()), |ui| {
        ui.add_space(space::S3);
        egui::ScrollArea::vertical().show(ui, |ui| {
            let band = hero::Band {
                eyebrow: "Album",
                title: "Signal Decay",
                meta: Vec::new(),
                credit: None,
                description: "",
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
                .heart
                .expect("an album's row keeps its heart")
                .center(),
            );
        });
    });
    found.expect("the band and its action row lay out")
}

/// A heart carries the value it was pressed into, so a saved album's heart
/// takes it out of the library rather than asking to save it again.
#[test]
fn a_saved_albums_band_heart_asks_to_unsave() {
    let ctx = context();
    let mut library = library_with(detail(vec![track(
        "spotify:track:a",
        "One",
        "Band",
        222_000,
    )]));
    library.apply(&Update::SavedAlbums(vec![saved_album("spotify:album:a")]));
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();
    let point = band_heart_point(&ctx);

    let _ = frame(
        &ctx,
        Vec::new(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    let _ = frame(
        &ctx,
        vec![egui::Event::PointerMoved(point)],
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    let command = click(
        &ctx,
        point,
        1,
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );

    assert_eq!(
        command,
        Some(Command::SetSaved {
            uri: "spotify:album:a".to_owned(),
            saved: false,
        }),
        "a saved album's heart asked to save it again"
    );
}

/// A cover offering 64, 300 and 640 squares, with their URLs.
fn sized(tail: &str) -> (CoverSet, CoverUrl, CoverUrl, CoverUrl) {
    let small =
        CoverUrl::from_wire(&format!("https://i.scdn.co/image/{tail}-64")).expect("a cover url");
    let middle =
        CoverUrl::from_wire(&format!("https://i.scdn.co/image/{tail}-300")).expect("a cover url");
    let large =
        CoverUrl::from_wire(&format!("https://i.scdn.co/image/{tail}-640")).expect("a cover url");
    let set = CoverSet::from_sizes([
        (Some((64, 64)), small.clone()),
        (Some((300, 300)), middle.clone()),
        (Some((640, 640)), large.clone()),
    ])
    .expect("a cover set");
    (set, small, middle, large)
}

#[test]
fn the_band_asks_for_the_large_size_at_two_x() {
    let ctx = context();
    ctx.set_pixels_per_point(2.0);
    let (set, small, _, large) = sized("album");
    let mut library = library_with(AlbumDetail {
        cover: Some(set),
        ..detail(Vec::new())
    });
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
    let fetched: Vec<CoverUrl> = requests
        .into_iter()
        .filter_map(|command| match command {
            Command::Fetch { url } => Some(url),
            _ => None,
        })
        .collect();

    assert!(
        fetched.contains(&large),
        "a 352px band takes the 640: {fetched:?}"
    );
    assert!(
        !fetched.contains(&small),
        "a band must not fetch the 64: {fetched:?}"
    );
}

#[test]
fn a_held_cover_tints_the_band() {
    let ctx = context();
    let tokens = Tokens::DARK;
    let url = CoverUrl::from_wire("https://i.scdn.co/image/tint").expect("a cover url");
    let set = CoverSet::single(url.clone());
    let mut library = library_with(AlbumDetail {
        cover: Some(set.clone()),
        ..detail(Vec::new())
    });
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
        "the album's own mean tints its band"
    );
}

#[test]
fn a_loading_cover_leaves_the_band_on_the_wash() {
    let ctx = context();
    let url = CoverUrl::from_wire("https://i.scdn.co/image/loading").expect("a cover url");
    let mut library = library_with(AlbumDetail {
        cover: Some(CoverSet::single(url)),
        ..detail(Vec::new())
    });
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
fn one_frame_asks_for_the_albums_cover_once() {
    let ctx = context();
    let mut library = library_with(AlbumDetail {
        cover: Some(CoverSet::single(
            CoverUrl::from_wire("https://i.scdn.co/image/abc").expect("a cover url"),
        )),
        tracks: vec![
            track("spotify:track:a", "One", "Band", 222_000),
            track("spotify:track:b", "Two", "Band", 198_000),
        ],
        ..detail(Vec::new())
    });
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

/// The fill of the band's wash: the one full-pane-width rect as tall as the
/// band itself.
fn band_fill(output: &egui::FullOutput) -> Option<egui::Color32> {
    output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::epaint::Shape::Rect(rect)
                if (rect.rect.width() - PROBE.x).abs() < 0.5
                    && (rect.rect.height() - band_height()).abs() < 0.5 =>
            {
                Some(rect.fill)
            }
            _ => None,
        })
}
