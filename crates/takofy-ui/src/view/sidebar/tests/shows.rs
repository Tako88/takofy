use super::super::*;
use super::test_support::*;

use takofy_model::CoverUrl;

use crate::message::Update;

const SHOW_IDS: Ids = Ids {
    scope: "library-shows",
    kind: "library-show",
    shelf: SidebarKind::Podcasts,
    lifts: false,
};

/// The rect the chip whose label was painted as `label` was drawn into:
/// the label text rect, expanded by the chip's padding.
fn chip_rect(output: &egui::FullOutput, label: &str) -> Rect {
    let (pos, size) = output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::epaint::Shape::Text(text) if text.galley.text() == label => {
                Some((text.pos, text.galley.size()))
            }
            _ => None,
        })
        .expect("the chip is drawn");
    Rect::from_min_size(pos, size).expand2(Vec2::new(extent::CHIP_PAD_X, extent::CHIP_PAD_Y))
}

/// A library with both lists ready, the way the window hands them over.
fn library_with(playlists: Vec<PlaylistEntry>, shows: Vec<FollowedShow>) -> Library {
    let mut library = Library::default();
    library.apply(&Update::Playlists(playlists));
    library.apply(&Update::FollowedShows(shows));
    library
}

#[test]
fn the_podcasts_chip_lists_the_shows_and_a_row_opens_the_show() {
    let ctx = context();
    let mut library = library_with(
        vec![playlist("spotify:playlist:a", "A list")],
        vec![followed_show("spotify:show:s", "A show")],
    );
    let mut art = Art::default();
    let (output, _) = run(&ctx, Vec::new(), &mut library, &mut art);

    // This click is the harness's proof that its rects hit the drawn chip:
    // a switch is observable, unlike re-pressing the choice already made.
    let command = click(
        &ctx,
        chip_rect(&output, "PODCASTS").center(),
        &mut library,
        &mut art,
    );
    assert_eq!(library.sidebar_kind.chosen(), SidebarKind::Podcasts);
    assert_eq!(command, None, "choosing a kind loads nothing by itself");

    let (output, _) = run(&ctx, Vec::new(), &mut library, &mut art);
    let (name, size) = painted_text(&output, "A show");
    assert!(!painted(&output, "A list"), "one kind is drawn at a time");

    let command = click(
        &ctx,
        Pos2::new(name.x + 10.0, name.y + size.y / 2.0),
        &mut library,
        &mut art,
    );
    assert_eq!(
        command,
        Some(Command::LoadShow {
            show: "spotify:show:s".to_owned()
        })
    );
    assert_eq!(
        library.page(),
        Some(&Page::Show("spotify:show:s".to_owned()))
    );
}

#[test]
fn a_show_name_lines_up_with_a_playlist_name() {
    let ctx = context();
    let shows = vec![followed_show("spotify:show:s", "A show")];
    let playlists = vec![playlist("spotify:playlist:a", "A list")];
    let mut art = Art::default();

    let mut library = library_with(playlists.clone(), shows.clone());
    let (output, _) = run(&ctx, Vec::new(), &mut library, &mut art);
    let (name, _) = painted_text(&output, "A list");
    let playlist_x = name.x;

    let mut library = library_with(playlists, shows);
    assert!(library.sidebar_kind.choose(SidebarKind::Podcasts));
    let (output, _) = run(&ctx, Vec::new(), &mut library, &mut art);
    let (name, _) = painted_text(&output, "A show");

    assert!(
        (name.x - playlist_x).abs() < 0.01,
        "{} vs {playlist_x}",
        name.x
    );
    assert!(
        (playlist_x - (space::S2 + space::S2 + extent::LIBRARY_ART + space::S3)).abs() < 0.01,
        "a name starts past the art slot: {playlist_x}"
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
fn a_show_row_asks_for_the_small_size_at_two_x() {
    let ctx = context();
    ctx.set_pixels_per_point(2.0);
    let mut show = followed_show("spotify:show:s", "A show");
    let (set, small, _, large) = sized("show");
    show.cover = Some(set);
    let mut library = library_with(Vec::new(), vec![show]);
    assert!(library.sidebar_kind.choose(SidebarKind::Podcasts));
    let mut art = Art::default();
    let now = Instant::now();

    let _ = run(&ctx, Vec::new(), &mut library, &mut art);
    let requested = art.requests(now);

    assert!(
        requested
            .iter()
            .any(|command| matches!(command, Command::Fetch { url } if *url == small)),
        "a 72px row takes the 64: {requested:?}"
    );
    assert!(
        !requested
            .iter()
            .any(|command| matches!(command, Command::Fetch { url } if *url == large)),
        "a row must not fetch the 640: {requested:?}"
    );
}

#[test]
fn a_show_row_asks_for_its_cover() {
    use takofy_model::{CoverSet, CoverUrl};

    let ctx = context();
    let mut show = followed_show("spotify:show:s", "A show");
    let url = "https://i.scdn.co/image/0123456789abcdef0123456789abcdef01234567";
    let cover = CoverUrl::from_wire(url).expect("the CDN url is admitted");
    show.cover = Some(CoverSet::single(cover.clone()));
    let mut library = library_with(Vec::new(), vec![show]);
    assert!(library.sidebar_kind.choose(SidebarKind::Podcasts));
    let mut art = Art::default();
    let now = Instant::now();

    let _ = run(&ctx, Vec::new(), &mut library, &mut art);
    let requested = art.requests(now);
    assert!(
        requested
            .iter()
            .any(|command| matches!(command, Command::Fetch { url: asked } if *asked == cover)),
        "the row looks its cover up through the art store"
    );
}

#[test]
fn a_show_row_names_its_kind() {
    let ctx = context();
    let mut library = library_with(Vec::new(), vec![followed_show("spotify:show:s", "A show")]);
    assert!(library.sidebar_kind.choose(SidebarKind::Podcasts));
    let mut art = Art::default();

    let (output, _) = run(&ctx, Vec::new(), &mut library, &mut art);
    assert!(painted(&output, "Podcast"), "the row says what it is");
}

#[test]
fn the_open_show_is_drawn_selected() {
    let ctx = context();
    let mut library = library_with(
        Vec::new(),
        vec![
            followed_show("spotify:show:a", "A show"),
            followed_show("spotify:show:b", "B show"),
        ],
    );
    assert!(library.sidebar_kind.choose(SidebarKind::Podcasts));
    library.open(Page::Show("spotify:show:b".to_owned()));
    let mut art = Art::default();

    let (output, _) = run(&ctx, Vec::new(), &mut library, &mut art);
    let (name, size) = painted_text(&output, "B show");
    let (_, kind) = painted_text(&output, "Podcast");
    let row = Rect::from_min_size(
        Pos2::new(space::S2, name.y + (size.y + kind.y) / 2.0 - ROW_H / 2.0),
        Vec2::new(254.0 - 2.0 * space::S2, ROW_H),
    );
    let selected = output.shapes.iter().any(|clipped| {
        matches!(&clipped.shape, egui::epaint::Shape::Rect(rect)
            if rect.rect == row && rect.fill == Tokens::DARK.palette.surface_raised)
    });
    assert!(selected, "the open show's row carries the selected fill");
}

#[test]
fn a_failed_playlist_load_retries_from_the_podcasts_list() {
    let ctx = context();
    let mut library = Library::default();
    assert!(library.sidebar_kind.choose(SidebarKind::Podcasts));
    library.playlists = Load::Failed(Failure::Load);
    let mut art = Art::default();

    let (_, command) = run(&ctx, Vec::new(), &mut library, &mut art);
    assert_eq!(command, Some(Command::ReloadLibrary));

    let (_, command) = run(&ctx, Vec::new(), &mut library, &mut art);
    assert_eq!(command, None, "the retry starts the load, so it asks once");
}

#[test]
fn a_failed_followed_load_retries_itself() {
    let ctx = context();
    let mut library = Library::default();
    assert!(library.sidebar_kind.choose(SidebarKind::Podcasts));
    library.playlists = Load::Ready(Vec::new());
    library.followed = Load::Failed(Failure::Followed);
    let mut art = Art::default();

    let (_, command) = run(&ctx, Vec::new(), &mut library, &mut art);
    assert_eq!(command, Some(Command::LoadFollowedShows));

    let (_, command) = run(&ctx, Vec::new(), &mut library, &mut art);
    assert_eq!(command, None, "the retry starts the load, so it asks once");
}

#[test]
fn a_show_and_a_playlist_row_never_share_an_id() {
    let ctx = context();
    let mut checked = false;
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(254.0, 800.0))),
            ..Default::default()
        },
        |ui| {
            assert_ne!(SHOW_IDS.id(ui, "x"), PLAYLIST_IDS.id(ui, "x"));
            checked = true;
        },
    );
    output.textures_delta.clear();
    assert!(checked);
}

#[test]
fn clicking_the_playlists_chip_keeps_the_choice() {
    let ctx = context();
    let mut library = Library::default();
    let mut art = Art::default();
    let (output, _) = run(&ctx, Vec::new(), &mut library, &mut art);

    let command = click(
        &ctx,
        chip_rect(&output, "PLAYLISTS").center(),
        &mut library,
        &mut art,
    );

    assert_eq!(
        library.sidebar_kind.chosen(),
        SidebarKind::Playlists,
        "the pressed chip is still the chosen kind"
    );
    assert_eq!(command, None, "pressing the choice asks for nothing");
}

/// Pressing the Artists chip takes the choice and, one frame later, asks for
/// the list. The second press asks for nothing: the load is already in
/// flight.
#[test]
fn pressing_the_artists_chip_asks_for_the_list_once() {
    let ctx = context();
    let mut library = Library::default();
    library.apply(&Update::LibraryReady);
    library.apply(&Update::Playlists(Vec::new()));
    let mut art = Art::default();
    let (output, _) = run(&ctx, Vec::new(), &mut library, &mut art);

    let chip = chip_rect(&output, "ARTISTS").center();
    let _ = click(&ctx, chip, &mut library, &mut art);
    assert_eq!(library.sidebar_kind.chosen(), SidebarKind::Artists);

    let first = library.followed_artists_request();
    assert_eq!(first, Some(Command::LoadFollowedArtists));
    assert_eq!(library.followed_artists_request(), None, "asked once");

    // The frame also draws the list's own loading copy while it waits.
    let (after, _) = run(&ctx, Vec::new(), &mut library, &mut art);
    let texts = drawn_text(&after);
    assert!(
        texts.contains(&"Loading your artists…".to_owned()),
        "{texts:?}"
    );
}
