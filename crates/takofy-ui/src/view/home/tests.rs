use std::time::Instant;

use egui::{Context, Event, Pos2, RawInput, Rect, Vec2};
use takofy_model::{PlaylistEntry, SavedAlbum, SavedAt, TrackEntry};

use super::*;
use crate::layout::Table;
use crate::library::Library;
use crate::message::{Command, Failure, Update};
use crate::player::Transport;
use crate::queue::Queue;
use crate::sidebar_kind::SidebarKind;
use crate::theme::{Tokens, fonts, style};
use crate::view::playlist::tracks_view;
use crate::view::test_support::click_events;

/// The probe window: wide enough for four shelf cards and tall enough for the
/// head and one row of them.
const PROBE: Vec2 = Vec2::new(900.0, 600.0);
const NOW_MS: u64 = 1_700_000_000_000;
const LOCAL_HOUR: u8 = 9;

fn context() -> Context {
    let ctx = Context::default();
    ctx.set_fonts(fonts::definitions());
    ctx.set_global_style(style(&Tokens::DARK));
    ctx
}

fn input(events: Vec<Event>) -> RawInput {
    RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, PROBE)),
        events,
        ..Default::default()
    }
}

/// One pass through the whole pane, so the page is reached the way the window
/// reaches it.
fn run(
    ctx: &Context,
    events: Vec<Event>,
    library: &mut Library,
    art: &mut Art,
) -> (Option<Command>, egui::FullOutput) {
    let tokens = Tokens::DARK;
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut command = None;
    let mut output = ctx.run_ui(input(events), |ui| {
        if let Some(next) = tracks_view(
            ui,
            Table::Full,
            library,
            &mut transport,
            &mut queue,
            art,
            Instant::now(),
            NOW_MS,
            LOCAL_HOUR,
            &tokens,
        ) {
            command = Some(next);
        }
    });
    output.textures_delta.clear();
    (command, output)
}

/// One click, in the passes egui needs to report it, and what the page asked
/// for on the frame that reported it.
fn click(ctx: &Context, pos: Pos2, library: &mut Library, art: &mut Art) -> Option<Command> {
    let mut command = None;
    for event in click_events(pos) {
        let (next, _) = run(ctx, vec![event], library, art);
        command = command.or(next);
    }
    command
}

/// Where the galley whose text is exactly `label` was painted.
fn text_rect(output: &egui::FullOutput, label: &str) -> Option<Rect> {
    output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::epaint::Shape::Text(text) if text.galley.text() == label => {
                Some(Rect::from_min_size(text.pos, text.galley.size()))
            }
            _ => None,
        })
}

/// The text one frame actually painted, in paint order.
fn painted(output: &egui::FullOutput) -> Vec<&str> {
    output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::epaint::Shape::Text(text) => Some(text.galley.text()),
            _ => None,
        })
        .collect()
}

/// The rect the shelf's `index`-th card takes, solved the way the page solves
/// it: the page head, then the shelf's own head, then the grid.
fn card_rect_for(ctx: &Context, index: usize) -> Rect {
    let tokens = Tokens::DARK;
    let mut found = None;
    let mut output = ctx.run_ui(input(Vec::new()), |ui| {
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                crate::view::inner(ui, |ui| {
                    page_head::show(ui, greeting(LOCAL_HOUR), "", &tokens);
                    section::head(ui, HOME_SHELF_TITLE, "", &tokens);
                    found = Some(root::media_grid(ui).card_rect(index));
                });
            });
    });
    output.textures_delta.clear();
    found.expect("the shelf lays out")
}

fn date(ms: u64) -> Option<SavedAt> {
    SavedAt::from_millis(i64::try_from(ms).ok()?)
}

fn playlist(uri: &str, name: &str, ms: Option<u64>) -> PlaylistEntry {
    PlaylistEntry {
        saved_at: ms.and_then(date),
        composed: Vec::new(),
        uri: uri.to_owned(),
        name: name.to_owned(),
        named: true,
        cover: None,
        description: None,
        owner: None,
        tracks: None,
        access: None,
    }
}

fn track(uri: &str, album: &str) -> TrackEntry {
    TrackEntry {
        uri: uri.to_owned(),
        title: uri.to_owned(),
        artists: String::new(),
        album: None,
        album_uri: Some(album.to_owned()),
        artist_uris: Vec::new(),
        artist_names: Vec::new(),
        added_at: None,
        duration_ms: None,
        named: true,
        cover: None,
    }
}

fn album(uri: &str, name: &str, ms: Option<u64>) -> SavedAlbum {
    SavedAlbum {
        saved_at: ms.and_then(date),
        uri: uri.to_owned(),
        name: name.to_owned(),
        named: true,
        artists: "Nils Aker".to_owned(),
        year: None,
        cover: None,
        tracks: None,
    }
}

/// A Home library whose three reads have all answered with these lists.
fn settled(playlists: Vec<PlaylistEntry>, albums: Vec<SavedAlbum>) -> Library {
    let mut library = Library::starting_home(NOW_MS);
    library.apply(&Update::Playlists(playlists));
    assert_eq!(
        library.likes_request(),
        Some(Command::LoadLikedTracks),
        "arrange: the liked read must be in flight"
    );
    library.apply(&Update::LikedTracks(Vec::new()));
    library.apply(&Update::SavedAlbums(albums));
    library
}

/// One library per shelf the page can draw, so a rule can be checked on all of
/// them.
fn every_shelf() -> Vec<Library> {
    let unreadable = {
        let mut library = Library::starting_home(NOW_MS);
        library.apply(&Update::PlaylistFailed(Failure::Load));
        library
    };
    let incomplete = {
        let mut library = settled(Vec::new(), Vec::new());
        library.apply(&Update::SavedAlbumsFailed(Failure::SavedAlbums));
        library
    };
    let quiet = settled(vec![playlist("spotify:playlist:a", "A", None)], Vec::new());
    let cards = settled(
        vec![playlist("spotify:playlist:a", "Mix", Some(5_000))],
        Vec::new(),
    );
    vec![
        Library::starting_home(NOW_MS),
        unreadable,
        incomplete,
        settled(Vec::new(), Vec::new()),
        quiet,
        cards,
    ]
}

#[test]
fn a_playlist_card_press_loads_it() {
    let ctx = context();
    let mut library = settled(
        vec![playlist("spotify:playlist:a", "Mix", Some(5_000))],
        Vec::new(),
    );
    let mut art = Art::default();

    let (_, output) = run(&ctx, Vec::new(), &mut library, &mut art);
    let pos = text_rect(&output, "Mix")
        .expect("the card is drawn")
        .center();

    assert_eq!(
        click(&ctx, pos, &mut library, &mut art),
        Some(Command::LoadTracks {
            playlist: "spotify:playlist:a".to_owned(),
        })
    );
    assert_eq!(library.selected(), Some("spotify:playlist:a"));
}

#[test]
fn a_track_card_opens_the_album_its_label_named() {
    let ctx = context();
    // A dated liked song reaches the shelf only through a liked read and the
    // label the card draws its name from.
    let mut library = {
        let mut library = Library::starting_home(NOW_MS);
        library.apply(&Update::Playlists(Vec::new()));
        assert_eq!(library.likes_request(), Some(Command::LoadLikedTracks));
        library.apply(&Update::LikedTracks(vec![takofy_model::LikedTrack {
            uri: "spotify:track:t".to_owned(),
            saved_at: date(5_000),
        }]));
        library.apply(&Update::SavedAlbums(Vec::new()));
        library.apply(&Update::SavedLabelled {
            asked: vec!["spotify:track:t".to_owned()],
            named: vec![track("spotify:track:t", "spotify:album:r")],
        });
        library
    };
    let mut art = Art::default();

    let (_, output) = run(&ctx, Vec::new(), &mut library, &mut art);
    let pos = text_rect(&output, "spotify:track:t")
        .expect("the card is drawn")
        .center();

    assert_eq!(
        click(&ctx, pos, &mut library, &mut art),
        Some(Command::LoadAlbum {
            album: "spotify:album:r".to_owned(),
        })
    );
}

#[test]
fn a_press_on_a_cards_play_wants_the_play_and_selects_nothing() {
    let ctx = context();
    let mut library = settled(
        vec![playlist("spotify:playlist:a", "Mix", Some(5_000))],
        Vec::new(),
    );
    let mut art = Art::default();
    let card = card_rect_for(&ctx, 0);

    let command = click(
        &ctx,
        media::tile_play_rect(card).center(),
        &mut library,
        &mut art,
    );

    assert_eq!(command, None, "a play press took the frame's command");
    assert_eq!(
        library.selected(),
        None,
        "a play press selected the playlist"
    );
    assert_eq!(
        library.play_request(),
        Some(Command::LoadTracks {
            playlist: "spotify:playlist:a".to_owned(),
        }),
        "the press never became a want"
    );
}

#[test]
fn an_album_card_press_opens_its_page() {
    let ctx = context();
    let mut library = settled(
        Vec::new(),
        vec![album("spotify:album:r", "Record", Some(5_000))],
    );
    let mut art = Art::default();

    let (_, output) = run(&ctx, Vec::new(), &mut library, &mut art);
    let pos = text_rect(&output, "Record")
        .expect("the card is drawn")
        .center();

    assert_eq!(
        click(&ctx, pos, &mut library, &mut art),
        Some(Command::LoadAlbum {
            album: "spotify:album:r".to_owned(),
        })
    );
}

#[test]
fn a_failed_shelf_play_draws_the_load_sentence() {
    let ctx = context();
    let mut library = settled(
        vec![playlist("spotify:playlist:a", "Mix", Some(5_000))],
        Vec::new(),
    );
    let mut art = Art::default();
    assert!(library.want_play("spotify:playlist:a", SidebarKind::Playlists));
    assert!(library.play_request().is_some());
    library.apply(&Update::TracksFailed {
        playlist: "spotify:playlist:a".to_owned(),
        failure: Failure::Load,
    });
    assert!(matches!(library.home_shelf(4), Shelf::Cards(_)));

    let (_, output) = run(&ctx, Vec::new(), &mut library, &mut art);

    assert!(
        painted(&output).contains(&crate::signin::failure_message(Failure::Load)),
        "Home says why the play did not start"
    );
}

#[test]
fn the_day_one_door_asks_the_search_field_for_the_keyboard() {
    let ctx = context();
    let mut library = settled(Vec::new(), Vec::new());
    let mut art = Art::default();

    let (_, output) = run(&ctx, Vec::new(), &mut library, &mut art);
    let pos = text_rect(&output, HOME_DAY_ONE_DOOR)
        .expect("the door is drawn")
        .center();

    assert_eq!(click(&ctx, pos, &mut library, &mut art), None);
    assert!(
        library.search.take_focus_ask(),
        "the door is the field's only way in from this page"
    );
}

#[test]
fn retry_from_an_unreadable_library_reloads() {
    let ctx = context();
    let mut library = Library::starting_home(NOW_MS);
    library.apply(&Update::PlaylistFailed(Failure::Load));
    let mut art = Art::default();

    let (_, output) = run(&ctx, Vec::new(), &mut library, &mut art);
    let pos = text_rect(&output, "Retry")
        .expect("a failed read offers a retry")
        .center();

    assert_eq!(
        click(&ctx, pos, &mut library, &mut art),
        Some(Command::ReloadLibrary)
    );
}

#[test]
fn retry_from_an_incomplete_library_reloads() {
    let ctx = context();
    let mut library = settled(Vec::new(), Vec::new());
    library.apply(&Update::SavedAlbumsFailed(Failure::SavedAlbums));
    let mut art = Art::default();

    let (_, output) = run(&ctx, Vec::new(), &mut library, &mut art);
    let pos = text_rect(&output, "Retry")
        .expect("a failed read offers a retry")
        .center();

    assert_eq!(
        click(&ctx, pos, &mut library, &mut art),
        Some(Command::ReloadLibrary)
    );
}

/// The greeting is the page's floor: every shelf states something under it,
/// and none of them is the library root drawn in its place.
#[test]
fn every_shelf_draws_the_greeting_and_never_the_root() {
    let ctx = context();
    let mut art = Art::default();

    for mut library in every_shelf() {
        let (_, output) = run(&ctx, Vec::new(), &mut library, &mut art);
        let painted = painted(&output);
        assert!(
            painted.contains(&greeting(LOCAL_HOUR)),
            "the greeting is drawn on every shelf"
        );
        assert!(
            !painted.contains(&"Your Library"),
            "Home is not the library root"
        );
    }
}

#[test]
fn only_the_cards_draw_the_shelf_head() {
    let ctx = context();
    let mut art = Art::default();

    for mut library in every_shelf() {
        let cards = matches!(library.home_shelf(4), Shelf::Cards(_));
        let (_, output) = run(&ctx, Vec::new(), &mut library, &mut art);
        let headed = painted(&output).contains(&HOME_SHELF_TITLE);
        assert_eq!(headed, cards, "a shelf head is drawn with cards only");
    }
}
