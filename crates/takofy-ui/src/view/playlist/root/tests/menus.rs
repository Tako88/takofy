use super::super::*;
use super::test_support::*;

use std::time::Instant;

use egui::{Context, Event};
use takofy_model::{PlaylistAccess, Revision};

use crate::layout::Table;
use crate::menu::{Anchor, Device, Kind, Listing, Request};
use crate::message::Update;
use crate::player::Transport;
use crate::queue::Queue;
use crate::view::playlist::tracks_view;
use crate::view::test_support::{click_events, secondary_click_events};

/// A rootlist, which a playlist row needs before it can be removed.
fn rootlist(editable: bool) -> PlaylistAccess {
    PlaylistAccess {
        revision: Revision::from_wire(b"root-1").expect("a revision"),
        editable,
    }
}

/// One click's passes, and the request the frame posted for it.
fn click_for_menu(
    ctx: &Context,
    events: [Event; 3],
    library: &mut Library,
    art: &mut Art,
) -> Option<Request> {
    let tokens = Tokens::DARK;
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut request = None;
    for event in events {
        let library = &mut *library;
        let mut output = ctx.run_ui(input(vec![event]), |ui| {
            let _ = tracks_view(
                ui,
                Table::Full,
                library,
                &mut transport,
                &mut queue,
                art,
                Instant::now(),
                NOW_MS,
                9,
                &tokens,
            );
        });
        output.textures_delta.clear();
        if let Some(posted) = context_menu::take_request(ctx) {
            request = Some(posted);
        }
    }
    request
}

/// The `⋯` of the root's one list row, pressed, and the request it posted.
fn press_the_more(ctx: &Context, library: &mut Library, art: &mut Art) -> Option<Request> {
    let more = media::columns(row_rect_for(ctx, 0)).more.center();
    click_for_menu(ctx, click_events(more), library, art)
}

/// Presses the `⋯` on the root's one list row and checks the menu it posted.
fn expect_row_menu(kind: SidebarKind, uri: &str, name: &str, library: &mut Library) {
    let ctx = context();
    let mut art = Art::default();
    library.layout = MediaLayout::List;
    let _ = frame(&ctx, library, &mut art);

    let request = press_the_more(&ctx, library, &mut art).expect("the `⋯` posted no request");
    assert_eq!(
        request.listing,
        Listing::Library(LibraryItem {
            kind,
            uri: uri.to_owned(),
            name: name.to_owned(),
        })
    );
    let labels: Vec<&str> = request
        .items
        .iter()
        .map(|item| item.label.as_str())
        .collect();
    let expected: Vec<&str> = if kind == SidebarKind::Podcasts {
        vec!["Copy link", "Remove from library"]
    } else {
        vec!["Copy link", "Start radio", "Remove from library"]
    };
    assert_eq!(labels, expected);
    assert!(
        !labels.contains(&"Rename"),
        "the root never offers a rename: {labels:?}"
    );
    assert_eq!(
        request.items.last().map(|item| item.kind),
        Some(Kind::Destructive),
        "the removal is the destructive row, drawn last"
    );
    assert_eq!(request.page, None, "the root posts no page");
    assert!(request.targets.is_empty() && request.rows.is_empty());
    assert!(
        matches!(request.door.anchor, Anchor::Button(_)),
        "the `⋯`'s own press hangs the menu off itself: {:?}",
        request.door.anchor
    );
    assert_eq!(request.door.device, Device::Pointer);
    assert!(library.selected().is_none(), "the press also selected");
    assert!(library.page().is_none(), "the press also opened a page");
}

#[test]
fn a_playlist_row_more_posts_the_sidebar_rows_menu() {
    let mut library = library(vec![playlist("spotify:playlist:a", "Evening")], vec![]);
    library.root_filter.choose(RootKind::Playlists);
    library.apply(&Update::RootlistAccess(rootlist(true)));
    expect_row_menu(
        SidebarKind::Playlists,
        "spotify:playlist:a",
        "Evening",
        &mut library,
    );
}

#[test]
fn a_podcast_row_more_posts_the_sidebar_rows_menu() {
    let mut library = library(vec![], vec![show("spotify:show:s", "Show One")]);
    library.root_filter.choose(RootKind::Podcasts);
    expect_row_menu(
        SidebarKind::Podcasts,
        "spotify:show:s",
        "Show One",
        &mut library,
    );
}

#[test]
fn an_artist_row_more_posts_the_sidebar_rows_menu() {
    let mut library = library(vec![], vec![]);
    library.root_filter.choose(RootKind::Artists);
    library.apply(&Update::FollowedArtists(vec![followed_artist(
        "spotify:artist:a",
        "Nils Aker",
    )]));
    expect_row_menu(
        SidebarKind::Artists,
        "spotify:artist:a",
        "Nils Aker",
        &mut library,
    );
}

#[test]
fn an_album_row_more_posts_the_sidebar_rows_menu() {
    let mut library = library(vec![], vec![]);
    library.root_filter.choose(RootKind::Albums);
    library.apply(&Update::SavedAlbums(vec![saved_album(
        "spotify:album:a",
        "Slow Tide",
    )]));
    expect_row_menu(
        SidebarKind::Albums,
        "spotify:album:a",
        "Slow Tide",
        &mut library,
    );
}

#[test]
fn a_playlist_row_the_user_cannot_write_offers_only_its_link() {
    let mut library = library(vec![playlist("spotify:playlist:a", "Evening")], vec![]);
    library.root_filter.choose(RootKind::Playlists);
    library.apply(&Update::RootlistAccess(rootlist(false)));
    let ctx = context();
    let mut art = Art::default();
    library.layout = MediaLayout::List;
    let _ = frame(&ctx, &mut library, &mut art);

    let request = press_the_more(&ctx, &mut library, &mut art).expect("the `⋯` posted no request");
    let labels: Vec<&str> = request
        .items
        .iter()
        .map(|item| item.label.as_str())
        .collect();
    assert_eq!(
        labels,
        vec!["Copy link", "Start radio"],
        "a row that cannot be removed keeps its link"
    );
}

#[test]
fn a_right_click_on_a_playlist_row_posts_the_same_request_at_the_pointer() {
    let mut library = library(vec![playlist("spotify:playlist:a", "Evening")], vec![]);
    library.root_filter.choose(RootKind::Playlists);
    library.apply(&Update::RootlistAccess(rootlist(true)));
    let ctx = context();
    let mut art = Art::default();
    library.layout = MediaLayout::List;
    let _ = frame(&ctx, &mut library, &mut art);

    let by_more = press_the_more(&ctx, &mut library, &mut art).expect("the `⋯` posted no request");
    let body = media::columns(row_rect_for(&ctx, 0)).body.center();
    let by_right = click_for_menu(&ctx, secondary_click_events(body), &mut library, &mut art)
        .expect("a right-click posted no request");

    assert_eq!(by_right.listing, by_more.listing);
    assert_eq!(by_right.items, by_more.items);
    assert_eq!(by_right.page, by_more.page);
    let Anchor::Pointer(at) = by_right.door.anchor else {
        panic!(
            "a right-click anchored to the `⋯`: {:?}",
            by_right.door.anchor
        );
    };
    assert!(
        (at - body).length() < 0.01,
        "a right-click at {body:?} opened at {at:?}"
    );
    assert_ne!(
        by_right.door.opener, by_more.door.opener,
        "the pointer door names the row, not the `⋯`"
    );
}

/// A tile has neither a row nor a `⋯`, so no press on it posts a menu: both
/// buttons do what a tile does, which is activate.
#[test]
fn a_grid_tile_posts_no_menu_on_a_click_or_a_right_click() {
    let ctx = context();
    let mut art = Art::default();
    let gridded = || {
        let mut library = library(vec![playlist("spotify:playlist:a", "Evening")], vec![]);
        library.root_filter.choose(RootKind::Playlists);
        library.apply(&Update::RootlistAccess(rootlist(true)));
        library.layout = MediaLayout::Grid;
        library
    };

    // The tile the grid solved, and the name it painted inside it: the press
    // lands on a drawn tile and not on the gap a row's `⋯` would fill.
    let mut library = gridded();
    let before = frame(&ctx, &mut library, &mut art);
    let card = card_rect_for(&ctx, 0, 1);
    let name = text_rect(&before, "Evening").expect("the grid paints the tile's name");
    assert!(
        card.contains_rect(name),
        "the tile's name {name:?} sits outside its card {card:?}"
    );

    let command = click(&ctx, card.center(), &mut library, &mut art);
    assert_eq!(
        command,
        Some(Command::LoadTracks {
            playlist: "spotify:playlist:a".to_owned()
        }),
        "a tile press selects its playlist, as it always did"
    );
    assert_eq!(library.selected(), Some("spotify:playlist:a"));
    assert_eq!(
        context_menu::take_request(&ctx),
        None,
        "a tile press posted a row menu"
    );

    // The right-click is its own press on a fresh library, and it must leave
    // the selection alone.
    let mut library = gridded();
    let _ = frame(&ctx, &mut library, &mut art);
    let request = click_for_menu(
        &ctx,
        secondary_click_events(card.center()),
        &mut library,
        &mut art,
    );
    assert_eq!(request, None, "a right-click on a tile posted a row menu");
    assert!(
        library.selected().is_none() && library.page().is_none(),
        "a right-click on a tile activated it"
    );
}
