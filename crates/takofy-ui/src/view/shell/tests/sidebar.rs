use super::super::*;
use super::test_support::*;
use crate::cover::Art;
use crate::device_list::DeviceList;
use crate::devices::Devices;
use crate::library::{Library, Load, MediaLayout};
use crate::library_filter::RootKind;
use crate::menu::LibraryItem;
use crate::message::Update;
use crate::player::Transport;
use crate::queue::Queue;
use crate::route::Page;
use crate::sidebar_kind::SidebarKind;
use crate::signin::SignIn;
use crate::theme::{extent, fonts, style};
use std::time::Instant;
use takofy_model::{FollowedArtist, PlaylistEntry, SavedAlbum};

/// The sidebar's highlighted library rows, which wear a bone rule at the
/// left edge.
pub(super) fn selected_rows(output: &egui::FullOutput) -> usize {
    let tokens = Tokens::DARK;
    output
        .shapes
        .iter()
        .filter(|clipped| match &clipped.shape {
            egui::Shape::Rect(shape) => {
                shape.fill == tokens.palette.text
                    && (shape.rect.width() - stroke::RULE).abs() < 0.01
                    && shape.rect.height() > extent::ROW_H
                    && shape.rect.left() < extent::SIDEBAR_W
            }
            _ => false,
        })
        .count()
}

#[test]
fn the_playlists_crumb_returns_to_the_root_and_back_loads_the_playlist() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    ready_playlist(&mut window);

    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let crumb = text_in(&frame, "PLAYLISTS", topbar_rect(&window))
        .expect("the trail's Playlists crumb is painted");
    assert_eq!(
        window.click(&mut transport, &mut queue, crumb.center()),
        None
    );
    assert_eq!(window.library.page(), None, "the crumb returns to the root");

    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let back = topbar_buttons(&frame)[2];
    assert_eq!(
        window.click(&mut transport, &mut queue, back.center()),
        Some(Command::LoadTracks {
            playlist: PLAYLIST.to_owned()
        }),
        "the root cleared the wanted load, so Back reloads it"
    );
    assert_eq!(
        window.library.page(),
        Some(&Page::Playlist(PLAYLIST.to_owned()))
    );
}

#[test]
fn back_into_a_playlist_whose_tracks_were_cleared_loads_it() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    window.library.apply(&Update::Playlists(vec![PlaylistEntry {
        saved_at: None,
        composed: Vec::new(),
        uri: PLAYLIST.to_owned(),
        name: "Mix".to_owned(),
        named: true,
        cover: None,
        description: None,
        owner: None,
        tracks: None,
        access: None,
    }]));
    window.library.select(PLAYLIST);
    window.library.to_playlists();
    assert_eq!(window.library.tracks, Load::Idle);
    assert_eq!(window.library.page(), None);

    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let back = topbar_buttons(&frame)[2];
    assert_eq!(
        window.click(&mut transport, &mut queue, back.center()),
        Some(Command::LoadTracks {
            playlist: PLAYLIST.to_owned()
        })
    );
}

#[test]
fn the_sidebar_highlight_clears_on_an_album_page() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    ready_playlist(&mut window);

    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    assert_eq!(
        selected_rows(&frame),
        1,
        "the current playlist is highlighted"
    );

    window.library.open(Page::Album(ALBUM.to_owned()));
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    assert_eq!(
        selected_rows(&frame),
        0,
        "no playlist is current on an album page"
    );
}

/// A nav can settle a page: Back onto a playlist marks its load wanted and
/// starts it. Keeping an earlier command of the frame and dropping the nav's
/// would leave that load marked in flight with nothing fetching it, so the
/// page would spin for a reply nobody asked for. A losing nav must therefore
/// not touch the library at all.
///
/// Two activations in one frame need a keyboard and a pointer together, which
/// the harness above cannot stage: egui resolves one click position per
/// frame. So the collision is driven through the shell's nav step, which is
/// where the fix lives.
///
/// A nav from a side button or a browser key merges into the same call, so
/// this guard covers it too. The sidebar-command case is not duplicated with
/// a second shell frame: the shell keeps the region's command before the nav
/// is ever looked at.
#[test]
fn a_nav_that_loses_the_frame_leaves_the_library_alone() {
    let mut library = Library::default();
    stale_playlist_behind_an_album(&mut library);
    let page = library.page().cloned();
    let tracks = library.tracks.clone();

    let won = Command::LoadTracks {
        playlist: ALBUM.to_owned(),
    };
    let command = apply_nav(
        &mut library,
        Some(topbar::Nav::Back),
        Some(won.clone()),
        NOW_MS,
    );

    assert_eq!(command, Some(won), "the earlier command keeps the frame");
    assert_eq!(
        library.page(),
        page.as_ref(),
        "the losing nav must not move the page"
    );
    assert_eq!(
        library.tracks, tracks,
        "and must not mark a load that nothing would fetch"
    );
    assert_eq!(library.page(), Some(&Page::Album(ALBUM.to_owned())));
}

/// The same Back, with no other command in the frame, still settles the page
/// and reports the load it started.
#[test]
fn a_nav_applies_when_no_other_region_asked() {
    let mut library = Library::default();
    stale_playlist_behind_an_album(&mut library);

    assert_eq!(
        apply_nav(&mut library, Some(topbar::Nav::Back), None, NOW_MS),
        Some(Command::LoadTracks {
            playlist: PLAYLIST.to_owned()
        })
    );
    assert_eq!(library.page(), Some(&Page::Playlist(PLAYLIST.to_owned())));
    assert_eq!(library.tracks, Load::Loading);
}

/// A list crumb asks for the root showing its own chip, which is the whole
/// reason a crumb names a list rather than just "back".
#[test]
fn a_list_crumb_returns_to_the_root_on_its_own_chip() {
    let mut library = Library::default();
    library.select(PLAYLIST);
    library.root_filter.choose(RootKind::Albums);

    assert_eq!(
        apply_nav(
            &mut library,
            Some(topbar::Nav::Root(RootKind::Playlists)),
            None,
            NOW_MS,
        ),
        None
    );
    assert_eq!(library.page(), None, "the root is on screen");
    assert_eq!(library.root_filter.chosen(), RootKind::Playlists);
}

/// A crumb that names a page opens it.
#[test]
fn a_page_crumb_opens_the_page() {
    let mut library = Library::default();

    assert_eq!(
        apply_nav(
            &mut library,
            Some(topbar::Nav::Page(Page::Artist(ARTIST.to_owned()))),
            None,
            NOW_MS,
        ),
        Some(Command::LoadArtist {
            artist: ARTIST.to_owned()
        })
    );
    assert_eq!(library.page(), Some(&Page::Artist(ARTIST.to_owned())));
}

#[test]
fn copy_link_on_a_library_row_writes_its_url() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let uri = "spotify:playlist:4uLU6hMCjMI75M1A2tKUQC";
    sidebar_row(&mut window, uri, "Deep Focus");
    open_library_menu(
        &mut window,
        library_row(SidebarKind::Playlists, uri, "Deep Focus"),
        true,
    );
    settle_menu(&mut window, &mut transport, &mut queue);

    let (_, frame) = window.frame(
        &mut transport,
        &mut queue,
        vec![key(egui::Key::Enter, true)],
    );
    let copied = frame
        .platform_output
        .commands
        .iter()
        .find_map(|command| match command {
            egui::OutputCommand::CopyText(text) => Some(text.as_str()),
            _ => None,
        });
    assert_eq!(
        copied,
        Some("https://open.spotify.com/playlist/4uLU6hMCjMI75M1A2tKUQC"),
        "an empty target list is not what decides a library menu's verb"
    );
}

#[test]
fn a_library_rows_removal_issues_its_one_command() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let uri = "spotify:playlist:4uLU6hMCjMI75M1A2tKUQC";
    sidebar_row(&mut window, uri, "Deep Focus");
    open_library_menu(
        &mut window,
        library_row(SidebarKind::Playlists, uri, "Deep Focus"),
        true,
    );
    settle_menu(&mut window, &mut transport, &mut queue);

    let command = press(&mut window, &mut transport, &mut queue, egui::Key::Delete);

    assert_eq!(
        command,
        Some(Command::RemovePlaylist {
            playlist: uri.to_owned(),
            revision: revision("rev-1"),
            index: 0,
        })
    );
    assert!(window.library.write_in_flight());
}

#[test]
fn a_library_rows_removal_is_dropped_while_a_gate_holds() {
    let uri = "spotify:album:4uLU6hMCjMI75M1A2tKUQC";

    // The drawer covers the window.
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let row = library_row(SidebarKind::Albums, uri, "A Record");
    window.library.apply(&Update::SavedAlbums(vec![SavedAlbum {
        saved_at: None,
        uri: uri.to_owned(),
        name: "A Record".to_owned(),
        named: true,
        artists: String::new(),
        artist_uris: Vec::new(),
        artist_names: Vec::new(),
        year: None,
        cover: None,
        tracks: None,
    }]));
    open_library_menu(&mut window, row.clone(), true);
    settle_menu(&mut window, &mut transport, &mut queue);
    window.drawer = Drawer::Open;
    assert_eq!(
        press(&mut window, &mut transport, &mut queue, egui::Key::Delete),
        None,
        "a removal was applied under the drawer"
    );
    assert!(!window.library.write_in_flight());

    // A block is carried.
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    window.library.apply(&Update::SavedAlbums(vec![SavedAlbum {
        saved_at: None,
        uri: uri.to_owned(),
        name: "A Record".to_owned(),
        named: true,
        artists: String::new(),
        artist_uris: Vec::new(),
        artist_names: Vec::new(),
        year: None,
        cover: None,
        tracks: None,
    }]));
    open_library_menu(&mut window, row, true);
    settle_menu(&mut window, &mut transport, &mut queue);
    egui::DragAndDrop::set_payload(&window.ctx, ());
    assert_eq!(
        press(&mut window, &mut transport, &mut queue, egui::Key::Delete),
        None,
        "a removal was applied while a block was carried"
    );
    assert!(!window.library.write_in_flight());
}

#[test]
fn a_library_rows_removal_is_dropped_while_a_field_holds_the_keyboard() {
    let ctx = egui::Context::default();
    ctx.set_fonts(fonts::definitions());
    ctx.set_global_style(style(&Tokens::DARK));
    let uri = "spotify:album:4uLU6hMCjMI75M1A2tKUQC";
    let mut library = Library::default();
    library.apply(&Update::SavedAlbums(vec![SavedAlbum {
        saved_at: None,
        uri: uri.to_owned(),
        name: "A Record".to_owned(),
        named: true,
        artists: String::new(),
        artist_uris: Vec::new(),
        artist_names: Vec::new(),
        year: None,
        cover: None,
        tracks: None,
    }]));
    let row = library_row(SidebarKind::Albums, uri, "A Record");
    let mut state = SignIn::SignedIn;
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut drawer = Drawer::default();
    let mut menu = crate::menu::Menu::default();
    let mut art = Art::default();
    let mut devices = Devices::default();
    let mut list = DeviceList::default();
    let mut pane = layout::PanePref::default();
    let mut typed = String::new();
    let mut command = None;

    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(1400.0, 900.0),
            )),
            ..Default::default()
        },
        |ui| {
            let id = egui::Id::unique("field");
            egui::TextEdit::singleline(&mut typed).id(id).show(ui);
            ui.memory_mut(|memory| memory.request_focus(id));
            assert!(ui.ctx().text_edit_focused(), "the field holds the key");
            let mut screen = Screen {
                state: &mut state,
                library: &mut library,
                transport: &mut transport,
                queue: &mut queue,
                devices: &mut devices,
                list: &mut list,
                drawer: &mut drawer,
                menu: &mut menu,
                position_ms: 0,
                art: &mut art,
                now: Instant::now(),
                now_ms: 0,
                local_hour: 9,
                pane: &mut pane,
            };
            command = apply_remove_from_library(ui, &mut screen, &row);
        },
    );

    output.textures_delta.clear();

    assert_eq!(command, None, "a removal was applied over a typing field");
    assert!(!library.write_in_flight());
}

// ---- the library root's list rows ----

/// Draws the root in the list density on `kind`'s chip.
fn library_list(window: &mut Window, kind: RootKind) {
    window.library.layout = MediaLayout::List;
    window.library.root_filter.choose(kind);
}

/// Clicks the `⋯` on the root's list row painted as `title`.
fn click_root_more(
    window: &mut Window,
    transport: &mut Transport,
    queue: &mut Queue,
    title: &str,
) -> Option<Command> {
    let (_, frame) = window.frame(transport, queue, Vec::new());
    let more = more_button(&frame, view_rect(window), title);
    window.click(transport, queue, more)
}

/// Opens the menu the root's list row painted as `title` offers.
fn open_root_menu(window: &mut Window, transport: &mut Transport, queue: &mut Queue, title: &str) {
    let _ = click_root_more(window, transport, queue, title);
    settle_menu(window, transport, queue);
    assert!(window.menu.is_open(), "the row's `⋯` opened no menu");
}

#[test]
fn a_root_rows_more_opens_the_sidebar_rows_menu() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let uri = "spotify:playlist:a";
    sidebar_row(&mut window, uri, "Evening");
    library_list(&mut window, RootKind::Playlists);

    open_root_menu(&mut window, &mut transport, &mut queue, "Evening");

    assert_eq!(
        window.menu.listing(),
        &Listing::Library(library_row(SidebarKind::Playlists, uri, "Evening"))
    );
    let labels: Vec<&str> = window
        .menu
        .items()
        .iter()
        .map(|item| item.label.as_str())
        .collect();
    assert_eq!(labels, ["Copy link", "Start radio", "Remove from library"]);
    assert_eq!(
        window.menu.items().last().map(|item| item.kind),
        Some(crate::menu::Kind::Destructive),
        "the removal is the destructive row, drawn last"
    );
}

#[test]
fn a_root_rows_removal_issues_its_kinds_own_write() {
    // An editable playlist is unlinked from the rootlist.
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let uri = "spotify:playlist:a";
    sidebar_row(&mut window, uri, "Evening");
    library_list(&mut window, RootKind::Playlists);
    open_root_menu(&mut window, &mut transport, &mut queue, "Evening");

    assert_eq!(
        press(&mut window, &mut transport, &mut queue, egui::Key::Delete),
        Some(Command::RemovePlaylist {
            playlist: uri.to_owned(),
            revision: revision("rev-1"),
            index: 0,
        })
    );
    assert!(window.library.write_in_flight());

    // A saved album is put back instead.
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let uri = "spotify:album:r";
    window.library.apply(&Update::Playlists(Vec::new()));
    window.library.apply(&Update::SavedAlbums(vec![SavedAlbum {
        saved_at: None,
        uri: uri.to_owned(),
        name: "A Record".to_owned(),
        named: true,
        artists: String::new(),
        artist_uris: Vec::new(),
        artist_names: Vec::new(),
        year: None,
        cover: None,
        tracks: None,
    }]));
    library_list(&mut window, RootKind::Albums);
    open_root_menu(&mut window, &mut transport, &mut queue, "A Record");

    assert_eq!(
        press(&mut window, &mut transport, &mut queue, egui::Key::Delete),
        Some(Command::SetSaved {
            uri: uri.to_owned(),
            saved: false,
        })
    );
    assert!(window.library.write_in_flight(), "the album's write is out");
}

#[test]
fn a_root_rows_copy_link_writes_its_url() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let uri = "spotify:playlist:4uLU6hMCjMI75M1A2tKUQC";
    sidebar_row(&mut window, uri, "Deep Focus");
    library_list(&mut window, RootKind::Playlists);
    open_root_menu(&mut window, &mut transport, &mut queue, "Deep Focus");

    // The pointer opened the menu and lit nothing; one step down puts the
    // keyboard on the first row, the way a user reaching for the keys does.
    let _ = window.frame(
        &mut transport,
        &mut queue,
        vec![key(egui::Key::ArrowDown, true)],
    );
    let (_, frame) = window.frame(
        &mut transport,
        &mut queue,
        vec![key(egui::Key::Enter, true)],
    );

    let copied = frame
        .platform_output
        .commands
        .iter()
        .find_map(|command| match command {
            egui::OutputCommand::CopyText(text) => Some(text.as_str()),
            _ => None,
        });
    assert_eq!(
        copied,
        Some("https://open.spotify.com/playlist/4uLU6hMCjMI75M1A2tKUQC")
    );
    assert!(!window.menu.is_open());
}

#[test]
fn a_root_rows_more_opens_nothing_while_a_write_is_in_flight() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let uri = "spotify:playlist:a";
    sidebar_row(&mut window, uri, "Evening");
    library_list(&mut window, RootKind::Playlists);
    open_root_menu(&mut window, &mut transport, &mut queue, "Evening");
    assert_eq!(
        press(&mut window, &mut transport, &mut queue, egui::Key::Delete),
        Some(Command::RemovePlaylist {
            playlist: uri.to_owned(),
            revision: revision("rev-1"),
            index: 0,
        })
    );
    assert!(window.library.write_in_flight());

    let _ = click_root_more(&mut window, &mut transport, &mut queue, "Evening");
    settle_menu(&mut window, &mut transport, &mut queue);

    // The gate shuts the menu whole: there is no menu to ask what it omits.
    assert!(!window.menu.is_open());

    // The write's reply clears the gate and leaves the row where it was, so
    // the same point pressed again opens the row's menu. Only the pair
    // proves the gate is what shut it: the second press shows a live `⋯`
    // sits under this point in this setup, the first shows the write alone
    // holds it.
    window.library.apply(&Update::PlaylistRemoveFailed {
        playlist: uri.to_owned(),
    });
    assert!(!window.library.write_in_flight());

    let _ = click_root_more(&mut window, &mut transport, &mut queue, "Evening");
    settle_menu(&mut window, &mut transport, &mut queue);
    assert!(
        window.menu.is_open(),
        "the `⋯` opened nothing with no write in flight"
    );
}

#[test]
fn a_root_rows_refused_removal_is_a_dead_press() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    let uri = "spotify:artist:a";
    window.library.apply(&Update::Playlists(Vec::new()));
    window
        .library
        .apply(&Update::FollowedArtists(vec![FollowedArtist {
            uri: uri.to_owned(),
            name: "Vela Moreno".to_owned(),
            named: true,
            portrait: None,
            releases: None,
        }]));
    library_list(&mut window, RootKind::Artists);
    open_root_menu(&mut window, &mut transport, &mut queue, "Vela Moreno");
    assert_eq!(
        window.menu.items().last().map(|item| item.verb),
        Some(Verb::RemoveFromLibrary),
        "the row was stripped of a removal the library still allowed"
    );

    // The list empties under the open menu, so the row the menu names is no
    // longer saved and the write refuses.
    window.library.apply(&Update::FollowedArtists(Vec::new()));
    assert_eq!(
        window.library.is_saved(uri),
        Some(false),
        "the removal was refused for another reason"
    );

    assert_eq!(
        press(&mut window, &mut transport, &mut queue, egui::Key::Delete),
        None,
        "a refused removal produced a command"
    );
    assert!(!window.menu.is_open());
    assert!(!window.library.write_in_flight());
    assert!(
        matches!(&window.library.followed_artists, Load::Ready(artists) if artists.is_empty()),
        "a refused removal changed the list"
    );
}

#[test]
fn a_sidebar_rows_removal_asks_for_its_kinds_own_write() {
    for (kind, name) in [
        (SidebarKind::Playlists, "Night Drive"),
        (SidebarKind::Albums, "A Record"),
        (SidebarKind::Artists, "Vela Moreno"),
        (SidebarKind::Podcasts, "Field Notes"),
    ] {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        removable_rows(&mut window);
        assert!(window.library.choose_kind(kind));

        right_click_sidebar_row(&mut window, &mut transport, &mut queue, name);
        assert!(window.menu.is_open(), "{kind:?} opened no menu");
        settle_menu(&mut window, &mut transport, &mut queue);

        let command = press(&mut window, &mut transport, &mut queue, egui::Key::Delete);

        let expected = match kind {
            SidebarKind::Playlists => Command::RemovePlaylist {
                playlist: "playlist:Night Drive".to_owned(),
                revision: revision("root-1"),
                index: 0,
            },
            SidebarKind::Albums => Command::SetSaved {
                uri: "spotify:album:r".to_owned(),
                saved: false,
            },
            SidebarKind::Artists => Command::SetFollow {
                artist: "spotify:artist:a".to_owned(),
                followed: false,
            },
            SidebarKind::Podcasts => Command::SetSaved {
                uri: "spotify:show:s".to_owned(),
                saved: false,
            },
        };
        assert_eq!(command, Some(expected), "{kind:?}");
        assert!(window.library.write_in_flight(), "{kind:?}");
    }
}

#[test]
fn a_confirmed_removal_drops_the_row_and_its_open_page_goes_home() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    sidebar_playlists(
        &mut window,
        &[("Night Drive", None), ("Deep Focus", None)],
        Some(rootlist_access(true)),
    );
    let _ = window.library.select("playlist:Night Drive");
    assert!(window.library.page().is_some());

    right_click_sidebar_row(&mut window, &mut transport, &mut queue, "Night Drive");
    settle_menu(&mut window, &mut transport, &mut queue);
    let command = press(&mut window, &mut transport, &mut queue, egui::Key::Delete);
    assert!(
        matches!(command, Some(Command::RemovePlaylist { .. })),
        "{command:?}"
    );

    window.library.apply(&Update::PlaylistRemoved {
        playlist: "playlist:Night Drive".to_owned(),
        revision: revision("root-2"),
    });

    assert_eq!(window.library.page(), None, "the removed page stayed open");
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    assert!(
        painted_at(&frame, "Deep Focus").is_some(),
        "the other row went with it"
    );
    assert!(
        painted_at(&frame, "Night Drive").is_none(),
        "the removed row stayed in the sidebar"
    );
}

#[test]
fn a_confirmed_removal_lands_while_the_drawer_covers_the_column() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    sidebar_playlists(
        &mut window,
        &[("Night Drive", None), ("Deep Focus", None)],
        Some(rootlist_access(true)),
    );
    let _ = window.library.select("playlist:Night Drive");

    right_click_sidebar_row(&mut window, &mut transport, &mut queue, "Night Drive");
    settle_menu(&mut window, &mut transport, &mut queue);
    assert!(
        matches!(
            press(&mut window, &mut transport, &mut queue, egui::Key::Delete),
            Some(Command::RemovePlaylist { .. })
        ),
        "the removal never went out"
    );

    window.drawer = Drawer::Open;
    window.library.apply(&Update::PlaylistRemoved {
        playlist: "playlist:Night Drive".to_owned(),
        revision: revision("root-2"),
    });

    assert_eq!(
        window.library.page(),
        None,
        "the drawer dropped the removed page's way home"
    );
    match &window.library.playlists {
        Load::Ready(playlists) => assert!(
            playlists
                .iter()
                .all(|entry| entry.uri != "playlist:Night Drive"),
            "the removed row stayed in the list"
        ),
        other => panic!("the list was dropped: {other:?}"),
    }
}

#[test]
fn del_on_a_bare_focused_row_removes_nothing() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    sidebar_playlists(
        &mut window,
        &[("Night Drive", None)],
        Some(rootlist_access(true)),
    );
    let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
    let at = sidebar_line(&frame, &window, "Night Drive");
    for pressed in [true, false] {
        let _ = window.frame(
            &mut transport,
            &mut queue,
            vec![side(at, egui::PointerButton::Primary, pressed)],
        );
    }
    assert!(
        window.ctx.memory(|memory| memory.focused()).is_some(),
        "the click left the row without the keyboard"
    );

    let command = press(&mut window, &mut transport, &mut queue, egui::Key::Delete);

    assert_eq!(command, None, "a bare row unlinked a playlist");
    assert!(!window.library.write_in_flight());
}

#[test]
fn a_sidebar_row_opens_no_menu_while_a_write_is_in_flight() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    sidebar_playlists(
        &mut window,
        &[("Night Drive", None), ("Deep Focus", None)],
        Some(rootlist_access(true)),
    );
    assert!(
        window
            .library
            .move_playlist(0, 2, "playlist:Night Drive")
            .is_some()
    );

    right_click_sidebar_row(&mut window, &mut transport, &mut queue, "Deep Focus");

    assert!(!window.menu.is_open(), "a menu opened over a write");
}

#[test]
fn escape_closes_a_sidebar_rows_menu_back_to_its_row() {
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut window = Window::new(Vec2::new(1400.0, 900.0));
    sidebar_playlists(
        &mut window,
        &[("Night Drive", None)],
        Some(rootlist_access(true)),
    );
    right_click_sidebar_row(&mut window, &mut transport, &mut queue, "Night Drive");
    assert!(window.menu.is_open());
    let opener = window.menu.opener().expect("a door names its opener");
    settle_menu(&mut window, &mut transport, &mut queue);

    let _ = press(&mut window, &mut transport, &mut queue, egui::Key::Escape);

    assert!(!window.menu.is_open());
    assert_eq!(
        window.ctx.memory(|memory| memory.focused()),
        Some(opener),
        "the keyboard went somewhere other than the row"
    );
}

/// Opens the menu a sidebar row's door posts, without a pointer.
fn open_library_menu(window: &mut Window, row: LibraryItem, removable: bool) {
    let request = crate::menu::Request {
        door: crate::menu::Door {
            anchor: crate::menu::Anchor::Button(Rect::from_min_size(
                egui::Pos2::new(60.0, 300.0),
                Vec2::new(28.0, 28.0),
            )),
            opener: egui::Id::unique("library-row"),
            device: crate::menu::Device::Keyboard,
        },
        playlists: Vec::new(),
        items: crate::verbs::library_items(
            &row,
            crate::verbs::LibraryRights {
                renamable: false,
                removable,
                saved: Some(true),
            },
        ),
        targets: Vec::new(),
        rows: Vec::new(),
        listing: Listing::Library(row),
        page: window.library.page().cloned(),
    };
    assert!(
        window.menu.open(request, Gate::default()),
        "the menu opened"
    );
}
