use super::super::*;
use super::test_support::*;

use egui::Pos2;
use takofy_model::Episode;

use crate::message::Update;

/// The two densities draw the same items and differ in what they add: the
/// list has the columns' head and the type and size cells, the grid has
/// neither.
#[test]
fn the_list_density_adds_the_head_and_the_two_cells() {
    let ctx = context();
    let mut library = library(vec![playlist("a", "Evening")], vec![]);
    library.layout = MediaLayout::List;
    let mut art = Art::default();

    let listed = frame(&ctx, &mut library, &mut art);
    let texts = drawn_texts(&listed);
    for heading in ["TITLE", "TYPE", "SIZE"] {
        assert!(
            texts.contains(&heading),
            "the list density draws the {heading} head: {texts:?}"
        );
    }
    assert!(
        texts.contains(&"PLAYLIST"),
        "a row carries its type: {texts:?}"
    );

    library.layout = MediaLayout::Grid;
    let gridded = frame(&ctx, &mut library, &mut art);
    let texts = drawn_texts(&gridded);
    assert!(!texts.contains(&"TITLE"), "the grid has no head: {texts:?}");
    assert!(
        texts.iter().any(|text| text.contains("Playlist")),
        "the tile still says what the thing is: {texts:?}"
    );
}

/// A press on the toggle moves the library's density, and the frame after it
/// draws the other one.
#[test]
fn pressing_the_list_toggle_draws_rows_next_frame() {
    let ctx = context();
    let mut library = library(vec![playlist("a", "Evening")], vec![]);
    let mut art = Art::default();

    let tokens = Tokens::DARK;
    let before = frame(&ctx, &mut library, &mut art);
    assert!(
        !drawn_texts(&before).contains(&"TITLE"),
        "the grid has no head to press"
    );

    let grid_cell = pressed_layout_cell(&before, &tokens);
    let list_cell = Pos2::new(
        grid_cell.right() + extent::BUTTON / 2.0,
        grid_cell.center().y,
    );
    let _ = click(&ctx, list_cell, &mut library, &mut art);

    assert_eq!(
        library.layout,
        MediaLayout::List,
        "the press moved the density"
    );
    let after = frame(&ctx, &mut library, &mut art);
    assert!(
        drawn_texts(&after).contains(&"TITLE"),
        "the next frame draws the list"
    );
}

/// The pressed density cell, found by its own chrome: a button-sized square
/// bordered in the text colour, which nothing else in the root draws.
fn pressed_layout_cell(output: &egui::FullOutput, tokens: &Tokens) -> Rect {
    output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::Shape::Rect(shape)
                if (shape.rect.width() - extent::BUTTON).abs() < 0.01
                    && (shape.stroke.width - crate::theme::stroke::HAIRLINE).abs() < 0.01
                    && shape.stroke.color == tokens.palette.text =>
            {
                Some(shape.rect)
            }
            _ => None,
        })
        .expect("the chosen density is bordered in the text colour")
}

/// A press on a row does what the card did: the playlist is selected.
#[test]
fn a_click_on_a_row_selects_its_playlist() {
    let ctx = context();
    let mut library = library(vec![playlist("a", "Evening")], vec![]);
    library.layout = MediaLayout::List;
    let mut art = Art::default();

    let before = frame(&ctx, &mut library, &mut art);
    // The type cell exists only in the list density, so a click there proves
    // the row drew and took the press.
    let kind = text_rect(&before, "PLAYLIST").expect("the row paints its type");
    let command = click(&ctx, kind.center(), &mut library, &mut art);

    assert_eq!(
        command,
        Some(Command::LoadTracks {
            playlist: "a".to_owned()
        }),
        "a row selects its playlist, as the card did"
    );
    assert_eq!(library.selected(), Some("a"));
}

#[test]
fn a_playlist_row_for_an_episode_draws_its_title_not_its_uri() {
    let ctx = context();
    let episode = Episode {
        uri: "spotify:episode:abc".to_owned(),
        title: "210: A dry labyrinth".to_owned(),
        show: "Lateral with Tom Scott".to_owned(),
        show_uri: Some("spotify:show:lateral".to_owned()),
        description: None,
        published: None,
        duration_ms: Some(2_773_461),
        cover: None,
        named: true,
    };
    let mut library = Library::default();
    library.apply(&Update::Playlists(vec![playlist("a", "A")]));
    library.select("a");
    library.apply(&Update::Tracks {
        playlist: "a".to_owned(),
        tracks: vec![episode.entry()],
        cover: None,
        access: None,
    });
    let mut art = Art::default();

    let output = frame(&ctx, &mut library, &mut art);
    let texts = drawn_texts(&output);
    assert!(
        texts.contains(&"210: A dry labyrinth"),
        "the row names the episode: {texts:?}"
    );
    assert!(
        !texts
            .iter()
            .any(|text| text.starts_with("spotify:episode:")),
        "the raw URI must not be drawn: {texts:?}"
    );
}
