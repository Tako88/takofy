//! Fixtures and probes shared by the shell's tests.

use super::super::*;
use crate::cover::Art;
use crate::device_list::DeviceList;
use crate::devices::Devices;
use crate::library::{Library, Load};
use crate::menu::LibraryItem;
use crate::message::Update;
use crate::player::{NowPlaying, Transport};
use crate::queue::Queue;
use crate::route::Page;
use crate::sidebar_kind::SidebarKind;
use crate::signin::SignIn;
use crate::theme::{extent, fonts, space, style};
use std::time::Instant;
use takofy_model::{
    CoverSet, CoverUrl, Device, DeviceKind, FollowedArtist, FollowedShow, PlaylistAccess,
    PlaylistEntry, Revision, SavedAlbum, TrackEntry,
};

pub(super) fn cover_url(tail: &str) -> CoverUrl {
    CoverUrl::from_wire(&format!("https://i.scdn.co/image/{tail}")).expect("a cover url")
}

pub(super) fn cover(tail: &str) -> CoverSet {
    CoverSet::single(cover_url(tail))
}

pub(super) fn playing(cover: CoverSet) -> Transport {
    Transport {
        now_playing: Some(NowPlaying {
            uri: "spotify:track:x".to_owned(),
            title: "A Title".to_owned(),
            artists: "A Band".to_owned(),
            cover: Some(cover),
            ..Default::default()
        }),
        ..Transport::default()
    }
}

pub(super) fn track(index: usize, cover: Option<CoverSet>) -> TrackEntry {
    TrackEntry {
        uri: format!("t{index}"),
        title: format!("t{index}"),
        artists: String::new(),
        album: None,
        album_uri: None,
        artist_uris: Vec::new(),
        artist_names: Vec::new(),
        added_at: None,
        duration_ms: None,
        named: true,
        cover,
    }
}

/// The window's own frame, kept across passes because egui's hit test reads
/// the previous one.
pub(super) struct Window {
    pub(super) ctx: egui::Context,
    pub(super) size: Vec2,
    pub(super) state: SignIn,
    pub(super) library: Library,
    pub(super) devices: Devices,
    pub(super) list: DeviceList,
    pub(super) drawer: Drawer,
    pub(super) menu: crate::menu::Menu,
    pub(super) art: Art,
    pub(super) now: Instant,
    /// The modifiers every frame reports as held. egui has no field for them
    /// on `RawInput`: a frame is told through `Event::ModifiersChanged`.
    pub(super) held: egui::Modifiers,
    /// Whether the frame reports the window as holding the OS keyboard.
    pub(super) unfocused: bool,
    /// What the user asked of the Now Playing column.
    pub(super) pane: layout::PanePref,
    /// What the window reports about itself, or `None` while it cannot say.
    pub(super) fullscreen: Option<bool>,
}

impl Window {
    pub(super) fn new(size: Vec2) -> Self {
        let tokens = Tokens::DARK;
        let ctx = egui::Context::default();
        ctx.set_fonts(fonts::definitions());
        ctx.set_global_style(style(&tokens));
        Self {
            ctx,
            size,
            state: SignIn::SignedIn,
            library: Library::default(),
            devices: Devices::default(),
            list: DeviceList::default(),
            drawer: Drawer::default(),
            menu: crate::menu::Menu::default(),
            art: Art::default(),
            now: Instant::now(),
            held: egui::Modifiers::NONE,
            unfocused: false,
            pane: layout::PanePref::default(),
            fullscreen: Some(false),
        }
    }

    /// Draws one frame, returning the command the shell reported and the
    /// frame's output.
    pub(super) fn frame(
        &mut self,
        transport: &mut Transport,
        queue: &mut Queue,
        events: Vec<egui::Event>,
    ) -> (Option<Command>, egui::FullOutput) {
        let tokens = Tokens::DARK;
        let mut events = events;
        events.insert(0, egui::Event::ModifiersChanged(self.held));
        let mut viewports = egui::ViewportIdMap::default();
        viewports.insert(
            egui::ViewportId::ROOT,
            egui::ViewportInfo {
                fullscreen: self.fullscreen,
                ..Default::default()
            },
        );
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, self.size)),
            viewports,
            events,
            focused: !self.unfocused,
            ..Default::default()
        };
        self.art.begin_frame();
        let mut command = None;
        let mut screen = Screen {
            state: &mut self.state,
            library: &mut self.library,
            transport,
            queue,
            devices: &mut self.devices,
            list: &mut self.list,
            drawer: &mut self.drawer,
            menu: &mut self.menu,
            position_ms: 0,
            art: &mut self.art,
            now: self.now,
            now_ms: 0,
            local_hour: 9,
            pane: &mut self.pane,
        };
        let mut output = self.ctx.run_ui(input, |ui| {
            command = show(ui, &mut screen, &tokens);
        });
        output.textures_delta.clear();
        (command, output)
    }

    /// The covers the last frame's draw asked for.
    pub(super) fn requests(&mut self) -> Vec<CoverUrl> {
        self.art
            .requests(self.now)
            .into_iter()
            .filter_map(|command| match command {
                Command::Fetch { url } => Some(url),
                _ => None,
            })
            .collect()
    }

    /// Double clicks `at` over the passes egui's hit test needs, returning
    /// the command the shell reported. A track row plays on a double click,
    /// the way every other table's rows do.
    pub(super) fn double_click(
        &mut self,
        transport: &mut Transport,
        queue: &mut Queue,
        at: egui::Pos2,
    ) -> Option<Command> {
        let _ = self.frame(transport, queue, Vec::new());
        let mut command = None;
        for _ in 0..2 {
            for event in crate::view::test_support::click_events(at) {
                if let Some(next) = self.frame(transport, queue, vec![event]).0 {
                    command = Some(next);
                }
            }
        }
        command
    }

    /// Clicks `at` over the passes egui's hit test needs, returning the
    /// viewport commands the passes sent.
    pub(super) fn click_viewport(
        &mut self,
        transport: &mut Transport,
        queue: &mut Queue,
        at: egui::Pos2,
    ) -> Vec<egui::ViewportCommand> {
        let _ = self.frame(transport, queue, Vec::new());
        let mut commands = Vec::new();
        for event in crate::view::test_support::click_events(at) {
            let (_, frame) = self.frame(transport, queue, vec![event]);
            commands.extend(viewport_commands(&frame));
        }
        commands
    }

    /// Clicks `at` over the passes egui's hit test needs, returning the
    /// command the shell reported.
    pub(super) fn click(
        &mut self,
        transport: &mut Transport,
        queue: &mut Queue,
        at: egui::Pos2,
    ) -> Option<Command> {
        let _ = self.frame(transport, queue, Vec::new());
        let mut command = None;
        for event in crate::view::test_support::click_events(at) {
            if let Some(next) = self.frame(transport, queue, vec![event]).0 {
                command = Some(next);
            }
        }
        command
    }

    /// Drags from `from` to `to`, release included, returning the command the
    /// shell reported.
    pub(super) fn drag(
        &mut self,
        transport: &mut Transport,
        queue: &mut Queue,
        from: egui::Pos2,
        to: egui::Pos2,
    ) -> Option<Command> {
        let last = self.hold_drag(transport, queue, from, to);
        self.frame(transport, queue, vec![self.release(to)])
            .0
            .or(last.0)
    }

    /// Drags from `from` to `to` and stops there with the button still down,
    /// returning the last frame the pointer moved in.
    pub(super) fn hold_drag(
        &mut self,
        transport: &mut Transport,
        queue: &mut Queue,
        from: egui::Pos2,
        to: egui::Pos2,
    ) -> (Option<Command>, egui::FullOutput) {
        let mut events = crate::view::test_support::drag_events(from, to);
        events.pop();
        let _ = self.frame(transport, queue, Vec::new());
        let mut last = None;
        let mut command = None;
        for event in events {
            let (next, output) = self.frame(transport, queue, vec![event]);
            command = command.or(next);
            last = Some(output);
        }
        (
            command,
            last.expect("a drag moves the pointer at least once"),
        )
    }

    /// The release event for a drag that ends at `to`, carrying the held
    /// modifiers the way a real release does.
    pub(super) fn release(&self, to: egui::Pos2) -> egui::Event {
        egui::Event::PointerButton {
            pos: to,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: self.held,
        }
    }
}

/// Where a label was painted, so a click lands on a row the shell drew
/// rather than on a coordinate a test guessed.
pub(super) fn painted_at(output: &egui::FullOutput, label: &str) -> Option<egui::Pos2> {
    output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::Shape::Text(text) if text.galley.text() == label => Some(text.pos),
            _ => None,
        })
}

/// The rect the first text shape with `label` inside `area` was painted in.
pub(super) fn text_in(output: &egui::FullOutput, label: &str, area: Rect) -> Option<Rect> {
    output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::Shape::Text(text) if text.galley.text() == label && area.contains(text.pos) => {
                Some(Rect::from_min_size(text.pos, text.galley.size()))
            }
            _ => None,
        })
}

/// The topbar's rect, solved the same way the shell solves it.
/// The bar spans the main column, which the pane widens; `sidebar_rect` is width-only.
pub(super) fn topbar_rect(window: &Window) -> Rect {
    let content = Rect::from_min_size(egui::Pos2::ZERO, window.size);
    let regions = layout::regions_with(content.width(), window.pane);
    let shell = layout::shell_for(&regions);
    layout::grid_for(content, &shell).topbar
}

/// The drawer's box, solved the same way the shell solves it.
pub(super) fn drawer_rect(window: &Window) -> Rect {
    let content = Rect::from_min_size(egui::Pos2::ZERO, window.size);
    let regions = layout::regions_with(content.width(), window.pane);
    let shell = layout::shell_for(&regions);
    layout::grid_for(content, &shell).drawer
}

/// The view's rect, solved the same way the shell solves it. A same-frame
/// destination is read from the text the view paints here, never from the
/// sidebar, which draws before the nav is applied.
pub(super) fn view_rect(window: &Window) -> Rect {
    let content = Rect::from_min_size(egui::Pos2::ZERO, window.size);
    let regions = layout::regions_with(content.width(), window.pane);
    let shell = layout::shell_for(&regions);
    layout::grid_for(content, &shell).view
}

/// One press or release of `key` with no modifiers.
pub(super) fn key(key: egui::Key, pressed: bool) -> egui::Event {
    key_with_repeat(key, pressed, false)
}

/// One press of `key` as a held key's auto-repeat, never a release.
pub(super) fn key_repeat(key: egui::Key) -> egui::Event {
    key_with_repeat(key, true, true)
}

/// `event` as one that says it holds `modifiers`.
pub(super) fn holding(mut event: egui::Event, modifiers: egui::Modifiers) -> egui::Event {
    if let egui::Event::Key {
        modifiers: held, ..
    } = &mut event
    {
        *held = modifiers;
    }
    event
}

/// One press of `key` holding `modifiers`, then its release.
///
/// `consume_key` reads the event's own modifiers, not the frame's, so a chord
/// has to say what it holds.
pub(super) fn chord(
    window: &mut Window,
    transport: &mut Transport,
    queue: &mut Queue,
    pressed: egui::Key,
    modifiers: egui::Modifiers,
) -> Option<Command> {
    window.held = modifiers;
    let press = holding(key(pressed, true), modifiers);
    let (command, _) = window.frame(transport, queue, vec![press]);
    let (_, _) = window.frame(transport, queue, vec![key(pressed, false)]);
    window.held = egui::Modifiers::NONE;
    command
}

fn key_with_repeat(key: egui::Key, pressed: bool, repeat: bool) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed,
        repeat,
        modifiers: egui::Modifiers::NONE,
    }
}

/// One press or release of `button` at `pos`.
///
/// The side buttons are read globally, so most tests pass `Pos2::ZERO`: a
/// `Primary` press there only arms a click on whatever sits top-left, which
/// is benign without a release. A frame that also clicks passes the click's
/// own position, because that is where the press would arrive on real
/// hardware. `test_support::button` is `Primary`-only, so it is not used
/// here.
pub(super) fn side(pos: egui::Pos2, button: egui::PointerButton, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button,
        pressed,
        modifiers: egui::Modifiers::NONE,
    }
}

/// The topbar's round buttons, left to right: Home, back, forward. Found from
/// the buttons' painted frames rather than from coordinates a test guessed.
pub(super) fn topbar_buttons(output: &egui::FullOutput) -> Vec<Rect> {
    // Only the topbar's band: the library's density toggle draws the same
    // 28-point squares further down the window.
    let band = extent::TOPBAR_H;
    let mut rects = output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Rect(shape)
                if shape.stroke.width > 0.0
                    && (shape.rect.width() - extent::BUTTON).abs() < 0.01
                    && (shape.rect.height() - extent::BUTTON).abs() < 0.01
                    && shape.rect.bottom() <= band =>
            {
                Some(shape.rect)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    rects.sort_by(|left, right| left.left().total_cmp(&right.left()));
    rects
}

/// The search field's painted box, found by its height rather than by a
/// coordinate a test guessed.
pub(super) fn search_field(output: &egui::FullOutput) -> Option<Rect> {
    output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::Shape::Rect(shape)
                if (shape.rect.height() - extent::SEARCH_H).abs() < 0.01
                    && shape.rect.width() > 100.0 =>
            {
                Some(shape.rect)
            }
            _ => None,
        })
}

pub(super) const PLAYLIST: &str = "spotify:playlist:a";
pub(super) const ARTIST: &str = "spotify:artist:a";
pub(super) const ALBUM: &str = "spotify:album:b";
pub(super) const ALBUM_B: &str = "spotify:album:c";

/// Puts one named, ready playlist on screen.
pub(super) fn ready_playlist(window: &mut Window) {
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
    window.library.apply(&Update::Tracks {
        playlist: PLAYLIST.to_owned(),
        tracks: Vec::new(),
        cover: None,
        access: None,
    });
}

/// A track a menu's verbs can all act on: a real track URI, an album and an
/// artist.
pub(super) fn menu_track(index: usize) -> TrackEntry {
    TrackEntry {
        uri: format!("spotify:track:{index}"),
        title: format!("t{index}"),
        artists: "A Band".to_owned(),
        album: Some("A Record".to_owned()),
        album_uri: Some(ALBUM.to_owned()),
        artist_uris: vec!["spotify:artist:b".to_owned()],
        artist_names: vec!["A Band".to_owned()],
        added_at: None,
        duration_ms: None,
        named: true,
        cover: None,
    }
}

/// Opens the context menu on these rows, as a row's door would.
pub(super) fn open_menu(
    window: &mut Window,
    verb: Verb,
    targets: Vec<TrackEntry>,
    listing: Listing,
    rows: Vec<usize>,
) {
    let uris: Vec<&str> = targets.iter().map(|entry| entry.uri.as_str()).collect();
    let standing = window.library.likes.standing(&uris);
    let items: Vec<_> = crate::verbs::track_items(&targets, None, &listing, standing)
        .into_iter()
        .filter(|item| item.verb == verb)
        .collect();
    assert!(!items.is_empty(), "the verb is offered for these rows");
    let request = crate::menu::Request {
        door: crate::menu::Door {
            anchor: crate::menu::Anchor::Button(Rect::from_min_size(
                egui::Pos2::new(600.0, 300.0),
                Vec2::new(28.0, 28.0),
            )),
            opener: egui::Id::unique("row-more"),
            device: crate::menu::Device::Keyboard,
        },
        playlists: Vec::new(),
        items,
        targets,
        rows,
        listing,
        // The page it is opened on, as a door posts it, so a page change
        // still closes it.
        page: window.library.page().cloned(),
    };
    assert!(
        window.menu.open(request, Gate::default()),
        "the menu opened"
    );
}

/// The page's own entries at `rows`, which is what a request acts on.
pub(super) fn page_entries(window: &Window, rows: &[usize]) -> Vec<TrackEntry> {
    match &window.library.tracks {
        Load::Ready(tracks) => rows
            .iter()
            .filter_map(|row| tracks.tracks.get(*row).cloned())
            .collect(),
        Load::Idle | Load::Loading | Load::Failed(_) => Vec::new(),
    }
}

/// Clicks the playlist page's row titled `title`, which picks and focuses
/// it.
pub(super) fn click_page_row(
    window: &mut Window,
    transport: &mut Transport,
    queue: &mut Queue,
    title: &str,
) -> Option<Command> {
    let (_, frame) = window.frame(transport, queue, Vec::new());
    let at = row_line(&frame, view_rect(window), title);
    window.click(transport, queue, at)
}

/// The Now Playing head's more button, solved the way the panel solves it.
pub(super) fn now_playing_more(window: &Window) -> egui::Pos2 {
    let pane = pane_rect_when_shown(window);
    let head = Rect::from_min_size(
        pane.min + Vec2::splat(space::S3),
        Vec2::new(pane.width() - 2.0 * space::S3, extent::BUTTON),
    );
    egui::Pos2::new(head.right() - extent::BUTTON / 2.0, head.center().y)
}

/// The playlist filter's field, read from the hairline it paints under
/// itself. The head's own rule is wider than the field's 15rem cap.
pub(super) fn page_filter_field(output: &egui::FullOutput, view: Rect) -> Option<Rect> {
    let tokens = Tokens::DARK;
    output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Rect(shape)
                if view.contains_rect(shape.rect)
                    && (shape.rect.height() - stroke::HAIRLINE).abs() < 0.01
                    && shape.rect.width() > 80.0
                    && shape.rect.width() <= 240.5
                    && (shape.fill == tokens.palette.border
                        || shape.fill == tokens.palette.text) =>
            {
                Some(shape.rect)
            }
            _ => None,
        })
        .min_by(|left, right| left.top().total_cmp(&right.top()))
}

/// The two frames an `Area` takes to arrive, so the menu's opening frame is
/// behind us.
pub(super) fn settle_menu(window: &mut Window, transport: &mut Transport, queue: &mut Queue) {
    for _ in 0..2 {
        let _ = window.frame(transport, queue, Vec::new());
    }
}

/// The `⋯` of the row titled `title`, found as the rightmost glyph on that
/// row's line: the like sits left of it, and the band's own `⋯` is a row
/// higher.
pub(super) fn more_button(output: &egui::FullOutput, view: Rect, title: &str) -> egui::Pos2 {
    let line = row_line(output, view, title).y;
    output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Path(_) => Some(clipped.shape.visual_bounding_rect()),
            _ => None,
        })
        .filter(|rect| {
            view.contains(rect.center()) && (rect.center().y - line).abs() < extent::ROW_H
        })
        .max_by(|left, right| left.center().x.total_cmp(&right.center().x))
        .map(|rect| rect.center())
        .expect("the row paints a `⋯`")
}

/// Makes the window's liked set these tracks, as a landed read would.
pub(super) fn liked(window: &mut Window, tracks: &[&str]) {
    assert!(window.library.likes.request(true));
    window.library.apply(&Update::LikedTracks(
        tracks
            .iter()
            .map(|uri| takofy_model::LikedTrack {
                uri: (*uri).to_owned(),
                saved_at: None,
            })
            .collect(),
    ));
}

/// The head's create button, from the band the sidebar solves.
///
/// The chips are a 2x2 grid of `extent::BUTTON` cells with `space::S1`
/// between them, under `space::S4` of padding, and the head row is `space::S2`
/// below them. The button is one `extent::BUTTON` square at the row's right.
pub(super) fn head_create(band: Rect) -> Rect {
    let chips = 2.0 * extent::BUTTON + space::S1;
    let row = Rect::from_min_size(
        egui::pos2(
            band.left() + space::S4,
            band.top() + space::S4 + chips + space::S3 + space::S2,
        ),
        Vec2::new(band.width() - 2.0 * space::S4, extent::BUTTON),
    );
    Rect::from_min_size(
        egui::pos2(row.right() - extent::BUTTON, row.top()),
        Vec2::splat(extent::BUTTON),
    )
}

/// The head's button centre, from the band the sidebar solves.
pub(super) fn head_plus(band: Rect) -> egui::Pos2 {
    head_create(band).center()
}

pub(super) fn sidebar_plus(window: &Window) -> egui::Pos2 {
    head_plus(sidebar_rect(window))
}

/// A sidebar whose playlists and rootlist are ready to create one.
pub(super) fn creatable_sidebar(window: &mut Window) {
    sidebar_playlists(
        window,
        &[("a", Some(0)), ("b", Some(0))],
        Some(rootlist_access(true)),
    );
}

/// Opens the head's field with the `+`, and waits for the keyboard to follow.
pub(super) fn open_naming(window: &mut Window, transport: &mut Transport, queue: &mut Queue) {
    let _ = window.click(transport, queue, sidebar_plus(window));
    assert!(window.library.naming.is_open(), "the `+` opened no field");
    // The field is drawn on the frame after the press, and takes the keyboard
    // on that frame.
    let _ = window.frame(transport, queue, Vec::new());
    assert!(
        window.ctx.text_edit_focused(),
        "the keyboard did not follow the field"
    );
    // One more frame, so the field's own key filter is the stored one: egui
    // stores a filter only for a widget that had focus last frame.
    let _ = window.frame(transport, queue, Vec::new());
}

/// Types into whatever holds the keyboard.
pub(super) fn type_text(
    window: &mut Window,
    transport: &mut Transport,
    queue: &mut Queue,
    text: &str,
) {
    let _ = window.frame(transport, queue, vec![egui::Event::Text(text.to_owned())]);
}

/// The row a create's reply hands the sidebar.
pub(super) fn new_playlist(name: &str, access: Option<PlaylistAccess>) -> PlaylistEntry {
    PlaylistEntry {
        saved_at: None,
        composed: Vec::new(),
        uri: "spotify:playlist:new".to_owned(),
        name: name.to_owned(),
        named: true,
        cover: None,
        description: None,
        tracks: Some(0),
        owner: None,
        access,
    }
}

/// Puts a ready search page on screen, answered with `count` labelled
/// tracks. The page is opened through `library.open`, which is the only way
/// in while the field cannot navigate yet.
pub(super) fn answered(window: &mut Window, query: &str, count: usize) {
    let _ = window.library.open(Page::Search(query.to_owned()));
    window.library.apply(&Update::Search {
        query: query.to_owned(),
        tracks: (0..count).map(|index| track(index, None)).collect(),
    });
}

/// Takes the reveal row labelled `label`, which the page applies itself.
pub(super) fn reveal(
    window: &mut Window,
    transport: &mut Transport,
    queue: &mut Queue,
    label: &str,
) {
    let (_, frame) = window.frame(transport, queue, Vec::new());
    let at = painted_at(&frame, label).unwrap_or_else(|| panic!("{label:?} is painted"))
        + Vec2::new(4.0, 4.0);
    assert_eq!(
        window.click(transport, queue, at),
        None,
        "a reveal asks for nothing"
    );
}

/// Asks the search state for the read a frame wants, the way `logic` does.
pub(super) fn search_read(window: &mut Window) -> Option<Command> {
    let page = window.library.page_search_query().map(str::to_owned);
    window.library.search.request(page.as_deref())
}

/// A window whose sidebar holds one row the user may unlink.
pub(super) fn sidebar_row(window: &mut Window, uri: &str, name: &str) {
    window.library.apply(&Update::Playlists(vec![PlaylistEntry {
        saved_at: None,
        composed: Vec::new(),
        uri: uri.to_owned(),
        name: name.to_owned(),
        named: true,
        cover: None,
        description: None,
        owner: None,
        tracks: None,
        access: page_access(true),
    }]));
    window
        .library
        .apply(&Update::RootlistAccess(PlaylistAccess {
            revision: revision("rev-1"),
            editable: true,
        }));
}

pub(super) fn library_row(kind: SidebarKind, uri: &str, name: &str) -> LibraryItem {
    LibraryItem {
        kind,
        uri: uri.to_owned(),
        name: name.to_owned(),
    }
}

pub(super) fn device(id: &str, name: &str, kind: DeviceKind) -> Device {
    Device {
        id: id.to_owned(),
        name: name.to_owned(),
        kind,
    }
}

/// The playbar's own rect and the form it was drawn in.
pub(super) fn playbar_parts(window: &Window) -> (Rect, Playbar) {
    let content = Rect::from_min_size(egui::Pos2::ZERO, window.size);
    let regions = layout::regions_with(content.width(), window.pane);
    let shell = layout::shell_for(&regions);
    let bar = layout::grid_for(content, &shell)
        .playbar
        .shrink(stroke::HAIRLINE);
    (bar, regions.playbar)
}

pub(super) fn carried_rows(list: egui::Id, rows: Vec<usize>) -> Carry {
    use crate::cover::Cover;
    use crate::view::queue_row;

    Carry {
        source: Source::List(list),
        rows,
        entries: Vec::new(),
        size: Vec2::new(10.0, 20.0),
        ghost: carry::Ghost::Queue(
            queue_row::Row {
                index: 0,
                title: "A Title",
                artist: "A Band",
                duration: None,
                cover: Cover::Missing,
                title_target: None,
                artist_target: None,
                look: crate::view::rows::Look::default(),
                refocus: false,
            }
            .lifted(),
        ),
        playlist: None,
    }
}

/// One queue row's pitch.
pub(super) const QUEUE_ROW_H: f32 = crate::layout::queue::ROW_H;

/// A queue of `count` tracks, started on the first.
pub(super) fn queue_of(count: usize, transport: &mut Transport) -> Queue {
    let tracks: Vec<TrackEntry> = (0..count).map(|index| track(index, None)).collect();
    let mut queue = Queue::default();
    queue.start(&tracks, 0, None, transport);
    queue
}

/// The sidebar column's content box: the panel less its hairline.
pub(super) fn sidebar_rect(window: &Window) -> Rect {
    let content = Rect::from_min_size(egui::Pos2::ZERO, window.size);
    let regions = layout::regions_for(content.width());
    let shell = layout::shell_for(&regions);
    layout::grid_for(content, &shell)
        .sidebar
        .shrink(stroke::HAIRLINE)
}

/// The rows the sidebar holds, in the order it holds them.
pub(super) fn sidebar_order(window: &Window) -> Vec<String> {
    match &window.library.playlists {
        Load::Ready(entries) => entries.iter().map(|entry| entry.name.clone()).collect(),
        Load::Idle | Load::Loading | Load::Failed(_) => Vec::new(),
    }
}

/// Puts `rows` in the sidebar, each read at `access`.
///
/// The list is written directly so a page already on screen survives it: a
/// `Playlists` update prunes a page whose playlist is not in it.
pub(super) fn sidebar_playlists(
    window: &mut Window,
    rows: &[(&str, Option<usize>)],
    access: Option<PlaylistAccess>,
) {
    window.library.playlists = Load::Ready(
        rows.iter()
            .map(|(name, tracks)| PlaylistEntry {
                saved_at: None,
                composed: Vec::new(),
                uri: format!("playlist:{name}"),
                name: (*name).to_owned(),
                named: true,
                cover: None,
                description: None,
                owner: None,
                tracks: *tracks,
                access: access.clone(),
            })
            .collect(),
    );
    if let Some(access) = access {
        window.library.apply(&Update::RootlistAccess(access));
    }
}

pub(super) fn rootlist_access(editable: bool) -> PlaylistAccess {
    PlaylistAccess {
        revision: revision("root-1"),
        editable,
    }
}

/// A page whose rows are real track URIs, so one can be added to a playlist.
pub(super) fn page_of_tracks(window: &mut Window, uris: &[&str]) {
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
    window.library.apply(&Update::Tracks {
        playlist: PLAYLIST.to_owned(),
        tracks: uris
            .iter()
            .map(|uri| TrackEntry {
                uri: (*uri).to_owned(),
                title: (*uri).to_owned(),
                ..track(0, None)
            })
            .collect(),
        cover: None,
        access: page_access(true),
    });
}

pub(super) const TRACK_A: &str = "spotify:track:4uLU6hMCjMI75M1A2tKUQC";

/// The top of the sidebar row painted as `title`, from the two lines it
/// draws. A row centres its name and kind in itself, and that offset is the
/// same for every row, so one row's lines measure all of them.
pub(super) fn sidebar_row_top(
    output: &egui::FullOutput,
    area: Rect,
    title: &str,
    kind: &str,
) -> f32 {
    let name =
        text_in(output, title, area).unwrap_or_else(|| panic!("{title} is painted in the sidebar"));
    let kind =
        text_in(output, kind, area).unwrap_or_else(|| panic!("{kind} is painted in the sidebar"));
    let inset = sidebar::ROW_H / 2.0 - (name.height() + kind.height()) / 2.0;
    name.top() - inset
}

/// A point at the gap `at` of the sidebar's rows: 0 is above the first row,
/// `titles.len()` below the last.
pub(super) fn sidebar_boundary(
    window: &Window,
    output: &egui::FullOutput,
    titles: &[&str],
    kind: &str,
    at: usize,
) -> egui::Pos2 {
    let area = sidebar_rect(window);
    let pitch = sidebar::ROW_H + space::PX;
    let last = titles.len() - 1;
    let y = if at > last {
        sidebar_row_top(output, area, titles[last], kind) + pitch
    } else {
        sidebar_row_top(output, area, titles[at], kind)
    };
    egui::pos2(area.left() + 100.0, y)
}

/// The box a sidebar row's fill covers, from the row's own painted line.
pub(super) fn sidebar_row_rect(
    window: &Window,
    output: &egui::FullOutput,
    title: &str,
    kind: &str,
) -> Rect {
    let area = sidebar_rect(window);
    let top = sidebar_row_top(output, area, title, kind);
    Rect::from_min_size(
        egui::pos2(area.left() + space::S2, top),
        Vec2::new(area.width() - 2.0 * space::S2, sidebar::ROW_H),
    )
}

/// Every entry in the queue, in order, wherever the index has got to.
pub(super) fn entries(queue: &Queue) -> Vec<String> {
    let count = queue.first_upcoming() + queue.upcoming().len();
    queue
        .entries_at(&(0..count).collect::<Vec<_>>())
        .into_iter()
        .map(|entry| entry.uri)
        .collect()
}

/// The Now Playing column's content box, for a window whose pane is shown.
/// The queue pane takes its width and ends at its bottom edge; its top is
/// the panel's head, well above the pane.
pub(super) fn pane_rect_when_shown(window: &Window) -> Rect {
    let content = Rect::from_min_size(egui::Pos2::ZERO, window.size);
    let regions = layout::regions_with(content.width(), window.pane);
    let shell = layout::shell_for(&regions);
    layout::grid_for(content, &shell)
        .now_playing
        .expect("the Now Playing column is on screen")
        .shrink(stroke::HAIRLINE)
}

/// The playbar's queue button, solved the way the shell solves it. A test
/// aims the pointer with it; what the button draws is read from the frame.
pub(super) fn queue_button_at(window: &Window) -> Rect {
    let content = Rect::from_min_size(egui::Pos2::ZERO, window.size);
    let regions = layout::regions_with(content.width(), window.pane);
    let shell = layout::shell_for(&regions);
    let bar = layout::grid_for(content, &shell)
        .playbar
        .shrink(stroke::HAIRLINE);
    playbar::queue_button(bar, regions.playbar)
}

/// The viewport commands a frame sent the window.
pub(super) fn viewport_commands(output: &egui::FullOutput) -> Vec<egui::ViewportCommand> {
    output
        .viewport_output
        .get(&egui::ViewportId::ROOT)
        .map(|viewport| viewport.commands.clone())
        .unwrap_or_default()
}

/// The rect the queue button's claim filled, if it drew one. The whole
/// playbar is scanned, and it holds no view row that could wear the same
/// fill.
pub(super) fn claimed_fill(output: &egui::FullOutput, window: &Window) -> Option<Rect> {
    let tokens = Tokens::DARK;
    let content = Rect::from_min_size(egui::Pos2::ZERO, window.size);
    let regions = layout::regions_with(content.width(), window.pane);
    let shell = layout::shell_for(&regions);
    let bar = layout::grid_for(content, &shell).playbar;
    output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::Shape::Rect(shape)
                if shape.fill == tokens.palette.row_selected && bar.contains_rect(shape.rect) =>
            {
                Some(shape.rect)
            }
            _ => None,
        })
}

/// A point inside the row painted as `title` in `area`, on its own line.
pub(super) fn row_line(output: &egui::FullOutput, area: Rect, title: &str) -> egui::Pos2 {
    let at = text_in(output, title, area)
        .unwrap_or_else(|| panic!("{title} is not painted inside {area:?}"))
        .min;
    egui::pos2(at.x + 4.0, at.y)
}

/// A point `pitches` queue rows below `at`. A row's line sits about a tenth of
/// a pitch into its row, so `-0.25` is the boundary before it and `0.75` the
/// boundary after it.
pub(super) fn below(at: egui::Pos2, pitches: f32) -> egui::Pos2 {
    egui::pos2(at.x, at.y + QUEUE_ROW_H * pitches)
}

/// The fill painted where the ghost starts: the pointer, offset.
pub(super) fn ghost_at(output: &egui::FullOutput, pointer: egui::Pos2) -> Option<Color32> {
    let at = pointer + Vec2::new(space::S3, space::S4);
    output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::Shape::Rect(shape) if (shape.rect.min - at).length() < 0.5 => Some(shape.fill),
            _ => None,
        })
}

/// The boxes this frame cast a shadow from.
pub(super) fn shadows(output: &egui::FullOutput) -> Vec<Rect> {
    output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Rect(shape) if shape.blur_width > 0.0 => Some(shape.rect),
            _ => None,
        })
        .collect()
}

/// The rule that says where a carried block lands, if one was drawn.
pub(super) fn marker(output: &egui::FullOutput) -> Option<Rect> {
    let tokens = Tokens::DARK;
    output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::Shape::Rect(shape)
                if shape.fill == tokens.palette.mark
                    && (shape.rect.height() - stroke::RULE).abs() < 0.01 =>
            {
                Some(shape.rect)
            }
            _ => None,
        })
}

/// The box a queue row's focus ring was drawn around, if one was.
pub(super) fn ringed_row(output: &egui::FullOutput) -> Option<Rect> {
    let tokens = Tokens::DARK;
    crate::view::test_support::rings(output, &tokens)
        .into_iter()
        .find(|ring| (ring.height() - QUEUE_ROW_H).abs() < 0.6)
}

/// A playlist page with one row per title, so a track can be dragged out of
/// it. The uris are the page's own, so they cannot be mistaken for the
/// queue's. The page is editable, the way a list the user owns is.
pub(super) fn playlist_page(window: &mut Window, titles: &[&str]) {
    playlist_page_with(window, titles, page_access(true));
}

/// The same page, read at a revision the user may not write.
pub(super) fn read_only_playlist_page(window: &mut Window, titles: &[&str]) {
    playlist_page_with(window, titles, page_access(false));
}

/// A revision the pages in these tests were read at.
pub(super) fn revision(tag: &str) -> Revision {
    Revision::from_wire(tag.as_bytes()).expect("a non-empty revision")
}

/// What a page may change, at a revision of its own.
pub(super) fn page_access(editable: bool) -> Option<PlaylistAccess> {
    Some(PlaylistAccess {
        revision: revision("rev-1"),
        editable,
    })
}

/// The rows the page holds, in the order it holds them.
pub(super) fn page_rows(window: &Window) -> Vec<String> {
    match &window.library.tracks {
        Load::Ready(tracks) => tracks
            .tracks
            .iter()
            .map(|entry| entry.uri.clone())
            .collect(),
        Load::Idle | Load::Loading | Load::Failed(_) => Vec::new(),
    }
}

/// A point at the boundary `at` of the page's rows, from the lines it
/// painted: 0 is above the first row, `count` below the last. A row's title
/// sits in the upper half of its row, so a row's own line is the gap it
/// opens above itself.
pub(super) fn page_boundary(
    output: &egui::FullOutput,
    area: Rect,
    titles: &[&str],
    at: usize,
) -> egui::Pos2 {
    let last = titles.len() - 1;
    let line = row_line(output, area, titles[at.min(last)]);
    let y = if at > last { line.y + 24.0 } else { line.y };
    egui::pos2(line.x, y)
}

/// Clicks the queue row painted as `title` inside `area`.
pub(super) fn clicked_row(
    window: &mut Window,
    queue: &mut Queue,
    transport: &mut Transport,
    area: Rect,
    title: &str,
) {
    let (_, frame) = window.frame(transport, queue, Vec::new());
    let at = row_line(&frame, area, title);
    let _ = window.click(transport, queue, at);
}

/// The field's painted box this frame.
pub(super) fn field_box(window: &mut Window, transport: &mut Transport, queue: &mut Queue) -> Rect {
    let (_, frame) = window.frame(transport, queue, Vec::new());
    search_field(&frame).expect("the topbar paints the search field")
}

/// Clicks the field and types `query` into it, leaving the read unsent so a
/// test can assert what it asks for.
pub(super) fn type_query(
    window: &mut Window,
    transport: &mut Transport,
    queue: &mut Queue,
    query: &str,
) {
    let at = field_box(window, transport, queue).center();
    let _ = window.click(transport, queue, at);
    let _ = window.frame(transport, queue, vec![egui::Event::Text(query.to_owned())]);
}

/// The menu's frame for a given number of rows, solved the way the view
/// solves it.
pub(super) fn menu_box(window: &Window, field: Rect, height: f32) -> Rect {
    let content = Rect::from_min_size(egui::Pos2::ZERO, window.size);
    layout::search_menu::menu_rect(field, content, height)
}

/// Types `query`, sends and answers its read with `count` tracks, and leaves
/// the menu drawn. Returns the menu's frame and that frame's output.
pub(super) fn answered_menu(
    window: &mut Window,
    transport: &mut Transport,
    queue: &mut Queue,
    query: &str,
    count: usize,
) -> (Rect, egui::FullOutput) {
    type_query(window, transport, queue, query);
    let key = crate::search::collapse(query).expect("a key");
    assert_eq!(
        search_read(window),
        Some(Command::Search { query: key.clone() }),
        "typing arms one read"
    );
    window.library.apply(&Update::Search {
        query: key,
        tracks: (0..count).map(|index| track(index, None)).collect(),
    });
    // The first frame an `Area` is shown is its sizing pass: egui draws
    // nothing and marks it invisible, then asks for the next frame. The
    // second frame is the menu the reader sees.
    let _ = window.frame(transport, queue, Vec::new());
    let (_, frame) = window.frame(transport, queue, Vec::new());
    let field = search_field(&frame).expect("the topbar paints the field");
    let (rows, floor) = window
        .library
        .search
        .listing()
        .map(|listing| (listing.shape.rows_per_band(), listing.shape.floor))
        .expect("the answered menu has a shape");
    let height = layout::search_menu::bands_height(&rows, floor);
    (menu_box(window, field, height), frame)
}

/// One keyboard press, with a release behind it so the next press is a new
/// one rather than an auto-repeat.
pub(super) fn press(
    window: &mut Window,
    transport: &mut Transport,
    queue: &mut Queue,
    code: egui::Key,
) -> Option<Command> {
    let command = window.frame(transport, queue, vec![key(code, true)]).0;
    let _ = window.frame(transport, queue, vec![key(code, false)]);
    command
}

/// A history that holds a stale playlist behind an album, so Back has to
/// start the playlist's load again.
pub(super) fn stale_playlist_behind_an_album(library: &mut Library) {
    library.apply(&Update::Playlists(vec![PlaylistEntry {
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
    library.select(PLAYLIST);
    library.apply(&Update::TracksFailed {
        playlist: PLAYLIST.to_owned(),
        failure: crate::message::Failure::Load,
    });
    library.open(Page::Album(ALBUM.to_owned()));
}

/// Queues two tracks, so the transport has something to pause.
pub(super) fn two_tracks(transport: &mut Transport) -> Queue {
    let tracks = vec![track(0, None), track(1, None)];
    let mut queue = Queue::default();
    queue.start(&tracks, 0, None, transport);
    queue
}

/// One press and its release, so the next press reads as a new one: egui
/// marks a press with no release behind it as auto-repeat.
pub(super) fn tap(
    window: &mut Window,
    transport: &mut Transport,
    queue: &mut Queue,
    event: egui::Event,
) -> (Option<Command>, egui::FullOutput) {
    let frame = window.frame(transport, queue, vec![event]);
    let _ = window.frame(transport, queue, vec![key(egui::Key::Space, false)]);
    frame
}

/// A point inside the sidebar row painted as `name`.
pub(super) fn sidebar_line(output: &egui::FullOutput, window: &Window, name: &str) -> egui::Pos2 {
    let rect = text_in(output, name, sidebar_rect(window))
        .unwrap_or_else(|| panic!("{name:?} is painted in the sidebar"));
    egui::Pos2::new(rect.left() + 4.0, rect.center().y)
}

/// Right-clicks the sidebar row painted as `name`, the way a mouse does.
pub(super) fn right_click_sidebar_row(
    window: &mut Window,
    transport: &mut Transport,
    queue: &mut Queue,
    name: &str,
) {
    let (_, frame) = window.frame(transport, queue, Vec::new());
    let at = sidebar_line(&frame, window, name);
    for event in [
        egui::Event::PointerMoved(at),
        side(at, egui::PointerButton::Secondary, true),
        side(at, egui::PointerButton::Secondary, false),
    ] {
        let _ = window.frame(transport, queue, vec![event]);
    }
}

/// A sidebar holding one row of each kind, each one the user may remove.
pub(super) fn removable_rows(window: &mut Window) {
    sidebar_playlists(
        window,
        &[("Night Drive", None)],
        Some(rootlist_access(true)),
    );
    window
        .library
        .apply(&Update::FollowedShows(vec![FollowedShow {
            uri: "spotify:show:s".to_owned(),
            name: "Field Notes".to_owned(),
            named: true,
            publisher: None,
            cover: None,
            episodes: None,
        }]));
    window
        .library
        .apply(&Update::FollowedArtists(vec![FollowedArtist {
            uri: "spotify:artist:a".to_owned(),
            name: "Vela Moreno".to_owned(),
            named: true,
            portrait: None,
            releases: None,
        }]));
    window.library.apply(&Update::SavedAlbums(vec![SavedAlbum {
        saved_at: None,
        uri: "spotify:album:r".to_owned(),
        name: "A Record".to_owned(),
        named: true,
        artists: String::new(),
        year: None,
        cover: None,
        tracks: None,
    }]));
}

pub(super) fn playlist_page_with(
    window: &mut Window,
    titles: &[&str],
    access: Option<PlaylistAccess>,
) {
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
    window.library.apply(&Update::Tracks {
        playlist: PLAYLIST.to_owned(),
        tracks: titles
            .iter()
            .map(|title| TrackEntry {
                uri: format!("list:{title}"),
                title: (*title).to_owned(),
                ..track(0, None)
            })
            .collect(),
        cover: None,
        access,
    });
}
