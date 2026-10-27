//! The library root: the cards behind the chosen chip, and the state of the
//! two loads that fill them.

use std::time::Instant;

use egui::{Pos2, Rect, Ui, vec2};
use takofy_model::{CoverSet, Owner};

use crate::cover::{Art, Cover, want_px};
use crate::format::{
    library_kind, library_note, media_size, media_type, root_empty_label, root_filter_label,
    root_loading_label,
};
use crate::layout::{cards, table as table_layout};
use crate::library::{Library, Load, MediaLayout};
use crate::library_filter::{self, Item, RootKind};
use crate::menu::{Door, LibraryItem};
use crate::message::{Command, Failure};
use crate::mosaic;
use crate::naming::Surface;
use crate::route::Page;
use crate::sidebar_kind::SidebarKind;
use crate::theme::{Tokens, extent, space};
use crate::verbs::{self, LibraryRights};
use crate::view::{
    context_menu, filter_row, hero, media, page_head, skeleton, text as recipe, view_toggle,
};

/// What the root's main view shows: the cards for the chosen chip, or the state
/// of the load that fetches them.
///
/// The main view draws the cards whenever the page is the root, whatever the
/// tracks hold: after a trip to the root a ready playlist is kept for the page
/// Back returns to, and nothing draws it. It is dropped or reloaded when a
/// playlist page is next entered. The root reads both loads, because its cards
/// are playlists and shows, and it never competes with a tracklist for a
/// selection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdleView<'a> {
    /// The playlists are in flight, or have not been asked for yet.
    Loading,
    /// The playlist fetch failed; the failure is what the retry needs.
    Failed(Failure),
    /// The chosen chip has nothing to show.
    Empty(RootKind),
    /// These cards, and the state of the list read under them.
    Grid {
        items: Vec<Item<'a>>,
        note: LoadNote,
    },
}

/// What the root says about the list under its cards.
///
/// The list is a second read with its own failure: a failed playlist fetch is
/// the page's failure, while a failed followed read only costs that kind's
/// cards and gets one muted line and its own Retry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadNote {
    /// The list arrived, or the chosen chip draws no followed list.
    None,
    /// The list is in flight; the cards will appear when it lands.
    Loading(FollowedRead),
    /// The list failed; the note names the read to restart.
    Failed(FollowedRead, Failure),
}

/// The followed read a chip shows, and the only ones the root reads under its
/// cards. The playlists chip reads no followed list, so it has no value here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FollowedRead {
    /// The followed shows, under All and Podcasts.
    Shows,
    /// The followed artists.
    Artists,
    /// The saved albums.
    Albums,
}

impl FollowedRead {
    /// What the root says under its cards while this read is in flight.
    fn loading(self) -> &'static str {
        root_loading_label(match self {
            Self::Shows => RootKind::Podcasts,
            Self::Artists => RootKind::Artists,
            Self::Albums => RootKind::Albums,
        })
    }

    /// The command that asks for this read again.
    fn reload(self, library: &mut Library) -> Command {
        match self {
            Self::Shows => library.reload_followed(),
            Self::Artists => library.reload_followed_artists(),
            Self::Albums => library.reload_saved_albums(),
        }
    }
}

/// Reads the root view from both loads and the chosen chip.
///
/// The two unstaged states are one `Loading`: a list nobody has asked for yet
/// and a list being fetched both have nothing to draw, and the reference gives
/// both the same treatment. A failed playlist load wins over every chip: with no
/// session there is nothing to narrow.
pub fn idle_view(library: &Library) -> IdleView<'_> {
    match &library.playlists {
        Load::Idle | Load::Loading => IdleView::Loading,
        Load::Failed(failure) => IdleView::Failed(*failure),
        Load::Ready(playlists) => {
            let chosen = library.root_filter.chosen();
            let shows = slice(&library.followed);
            let artists = slice(&library.followed_artists);
            let albums = slice(&library.saved_albums);
            let items = library_filter::items(chosen, playlists, shows, artists, albums);
            let note = note_for(library, chosen);
            if items.is_empty() && note == LoadNote::None {
                IdleView::Empty(chosen)
            } else {
                IdleView::Grid { items, note }
            }
        }
    }
}

/// The items of a list that has arrived, or none while it has not.
fn slice<T>(load: &Load<Vec<T>>) -> &[T] {
    match load {
        Load::Ready(items) => items.as_slice(),
        Load::Idle | Load::Loading | Load::Failed(_) => &[],
    }
}

/// The note under the root's cards for the chosen chip.
///
/// A chip that draws one followed list says what that read is doing. All draws
/// every one of them, so it names the first that is still owed, in the order the
/// cards are drawn.
fn note_for(library: &Library, chosen: RootKind) -> LoadNote {
    match chosen {
        RootKind::Playlists => LoadNote::None,
        RootKind::All => [
            index_note(FollowedRead::Shows, &library.followed),
            index_note(FollowedRead::Artists, &library.followed_artists),
            index_note(FollowedRead::Albums, &library.saved_albums),
        ]
        .into_iter()
        .find(|note| *note != LoadNote::None)
        .unwrap_or(LoadNote::None),
        RootKind::Podcasts => index_note(FollowedRead::Shows, &library.followed),
        RootKind::Artists => index_note(FollowedRead::Artists, &library.followed_artists),
        RootKind::Albums => index_note(FollowedRead::Albums, &library.saved_albums),
    }
}

/// What one followed read is doing, named by the chip it belongs to.
fn index_note<T>(read: FollowedRead, load: &Load<Vec<T>>) -> LoadNote {
    match load {
        Load::Idle | Load::Loading => LoadNote::Loading(read),
        Load::Failed(failure) => LoadNote::Failed(read, *failure),
        Load::Ready(_) => LoadNote::None,
    }
}

/// Draws the idle main view and returns the command its controls asked for.
///
/// This is the whole root wiring in one place: the page head, the chip row and
/// the cards scroll as one column, the way the reference's view does. A pressed
/// chip is applied after the scroll area, because the matches inside borrow the
/// cards it has to change.
pub(super) fn idle(
    ui: &mut Ui,
    library: &mut Library,
    art: &mut Art,
    now: Instant,
    tokens: &Tokens,
) -> Option<Command> {
    let mut choose: Option<RootKind> = None;
    let mut relayout: Option<MediaLayout> = None;
    let mut select: Option<String> = None;
    let mut open: Option<Page> = None;
    let mut play: Option<(String, SidebarKind)> = None;
    let mut retry_playlists = false;
    let mut retry: Option<FollowedRead> = None;

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            crate::view::inner(ui, |ui| {
                let view = idle_view(library);
                let count = match &view {
                    IdleView::Grid { items, .. } => items.len(),
                    IdleView::Loading | IdleView::Failed(_) | IdleView::Empty(_) => 0,
                };
                let note = if count > 0 {
                    library_note(count)
                } else {
                    String::new()
                };
                page_head::show(ui, "Your Library", &note, tokens);

                let shown = filter_row::show_with(
                    ui,
                    tokens,
                    &library_filter::ROW,
                    library.root_filter.chosen(),
                    RootKind::control,
                    root_filter_label,
                    filter_row::RowSlot::Trailing(view_toggle::reserved()),
                );
                choose = shown.picked;
                if let Some(layout) = view_toggle::show(ui, tokens, shown.area, library.layout) {
                    relayout = Some(layout);
                }
                // Under the chips, where the page's own failures are said: the
                // press that failed is the retry, so there is no button.
                if let Some(failure) = library.play_notice {
                    ui.label(recipe::muted(
                        crate::signin::failure_message(failure),
                        tokens,
                    ));
                }

                match view {
                    IdleView::Loading => {
                        let solved = media_grid(ui);
                        let count = skeleton::SCREENFUL;
                        let list = grid_rect(ui, &solved, count);
                        ui.allocate_rect(list, egui::Sense::hover());
                        skeleton::tiles(ui, &solved, count, tokens);
                    }
                    IdleView::Failed(failure) => {
                        ui.label(recipe::muted(
                            crate::signin::failure_message(failure),
                            tokens,
                        ));
                        // The sidebar retries a failed playlist load with
                        // `reload`; the root offers the same recovery here.
                        if ui.button("Retry").clicked() {
                            retry_playlists = true;
                        }
                    }
                    IdleView::Empty(kind) => {
                        ui.label(recipe::muted(root_empty_label(kind), tokens));
                    }
                    IdleView::Grid { items, note } => {
                        grid(
                            ui,
                            &items,
                            library.layout,
                            &mut MediaDraw {
                                art,
                                now,
                                tokens,
                                library: &*library,
                                select: &mut select,
                                open: &mut open,
                                play: &mut play,
                            },
                        );
                        load_note(ui, note, tokens, &mut retry);
                    }
                }
            });
        });

    if let Some(layout) = relayout {
        library.layout = layout;
    }
    // The press is a want, not a command: the rows it needs are asked for by
    // `logic`, which runs before the next draw.
    if let Some((uri, kind)) = play
        && library.want_play(&uri, kind)
    {
        ui.ctx().request_repaint();
    }
    if let Some(kind) = choose {
        library.root_filter.choose(kind);
    }
    if retry_playlists {
        return Some(library.reload());
    }
    if let Some(read) = retry {
        return Some(read.reload(library));
    }
    if let Some(uri) = select {
        return select_playlist(library, &uri);
    }
    if let Some(page) = open {
        return library.open(page);
    }
    None
}

/// The saved things for the chosen chip, in the density the user chose.
///
/// One scroll area either way, so switching density keeps the place the reader
/// had reached. A tile selects its playlist or opens its page; a row does the
/// same, and its more button takes its own clicks.
fn grid(ui: &mut Ui, items: &[Item<'_>], layout: MediaLayout, draw: &mut MediaDraw<'_, '_>) {
    if layout == MediaLayout::List {
        return list(ui, items, draw);
    }

    let solved = media_grid(ui);
    let list = grid_rect(ui, &solved, items.len());
    // The whole grid is reserved at once, so the scrollbar is right, and only the
    // tiles the viewport can show are laid out and given interaction.
    ui.allocate_rect(list, egui::Sense::hover());
    let visible = ui.clip_rect();

    for (index, item) in items.iter().enumerate() {
        let rect = solved.card_rect(index);
        if !table_layout::on_screen(visible, rect) {
            continue;
        }
        let media = draw.media_of(item, layout, want_px(solved.card_w, ui.pixels_per_point()));
        match media::tile(ui, rect, &media.as_media(), draw.tokens) {
            Some(media::Press::Open) => draw.activate(item, &media.uri),
            Some(media::Press::Play) => draw.want_play(&media),
            Some(media::Press::Menu(_)) | None => {}
        }
    }
}

/// The same items as rows, under the columns' names.
fn list(ui: &mut Ui, items: &[Item<'_>], draw: &mut MediaDraw<'_, '_>) {
    let width = ui.available_width();
    let height = space::S2 + media::head_height(ui) + space::S2;
    let (head_rect, _) = ui.allocate_exact_size(vec2(width, height), egui::Sense::hover());
    media::head(ui, head_rect, draw.tokens);

    let top = ui.cursor().min.y;
    let rows = Rect::from_min_size(
        Pos2::new(ui.cursor().min.x, top),
        vec2(width, items.len() as f32 * extent::ROW_H),
    );
    ui.allocate_rect(rows, egui::Sense::hover());
    let visible = ui.clip_rect();
    for (index, item) in items.iter().enumerate() {
        let rect = Rect::from_min_size(
            Pos2::new(rows.left(), rows.top() + index as f32 * extent::ROW_H),
            vec2(width, extent::ROW_H),
        );
        if !table_layout::on_screen(visible, rect) {
            continue;
        }
        let media = draw.media_of(
            item,
            MediaLayout::List,
            want_px(extent::LIBRARY_ART, ui.pixels_per_point()),
        );
        let words = media.as_media();
        match media::row(ui, rect, &words, draw.tokens) {
            Some(media::Press::Open) => draw.activate(item, &media.uri),
            Some(media::Press::Play) => draw.want_play(&media),
            Some(media::Press::Menu(door)) => draw.post_menu(ui.ctx(), &media, door),
            None => {}
        }
    }
}

/// Where a click on one item goes, and the frame's drawing context.
pub(super) struct MediaDraw<'a, 'b> {
    pub(super) art: &'a mut Art,
    pub(super) now: Instant,
    pub(super) tokens: &'a Tokens,
    pub(super) library: &'b Library,
    pub(super) select: &'b mut Option<String>,
    pub(super) open: &'b mut Option<Page>,
    /// The object whose play was pressed, and the list it belongs to.
    pub(super) play: &'b mut Option<(String, SidebarKind)>,
}

impl MediaDraw<'_, '_> {
    /// Records a press on an item's play. The want is taken after the scroll
    /// area, so the borrow of the items has ended by the time it is held.
    fn want_play(&mut self, media: &MediaWords) {
        *self.play = Some((media.uri.clone(), media.shelf));
    }

    /// What a click on one item does: a playlist selects, the rest open their
    /// page.
    fn activate(&mut self, item: &Item<'_>, uri: &str) {
        match item {
            Item::Playlist(_) => *self.select = Some(uri.to_owned()),
            Item::Show(_) => *self.open = Some(Page::Show(uri.to_owned())),
            Item::Artist(_) => *self.open = Some(Page::Artist(uri.to_owned())),
            Item::Album(_) => *self.open = Some(Page::Album(uri.to_owned())),
        }
    }

    /// The menu a row's `⋯` asked for: the sidebar row's own verbs, built for
    /// the item it names.
    fn post_menu(&self, ctx: &egui::Context, media: &MediaWords, door: Door) {
        let item = LibraryItem {
            kind: media.shelf,
            uri: media.uri.clone(),
            name: media.name.clone(),
        };
        let rights = LibraryRights {
            // The name field lives in the sidebar, so the root offers no rename.
            renamable: false,
            removable: self.library.can_remove(media.shelf),
            // Every root row comes from a loaded list, so there is nothing to
            // put back.
            saved: Some(true),
        };
        // The page is `None` at the root, and is read anyway so the request stays
        // right if the root is ever drawn over a page.
        context_menu::post(
            ctx,
            verbs::library_request(
                item,
                door,
                rights,
                self.library.page().cloned(),
                Surface::Row,
            ),
        );
    }

    /// One item's words and picture, owned so a borrow of the item is not needed.
    ///
    /// Only the grid composes: a four-up at a row's art column is mush, so a
    /// list never looks its quadrants up.
    pub(super) fn media_of(
        &mut self,
        item: &Item<'_>,
        layout: MediaLayout,
        want: u32,
    ) -> MediaWords {
        let (uri, name, kind, sub, size, round, cover, shelf) = match item {
            Item::Playlist(entry) => (
                &entry.uri,
                &entry.name,
                media_type(RootKind::Playlists),
                library_kind("Playlist", entry.owner.as_ref().and_then(Owner::name)),
                media_size(RootKind::Playlists, entry.tracks),
                false,
                entry.cover_set(),
                SidebarKind::Playlists,
            ),
            Item::Show(show) => (
                &show.uri,
                &show.name,
                media_type(RootKind::Podcasts),
                library_kind("Podcast", show.publisher.as_deref()),
                media_size(RootKind::Podcasts, show.episodes),
                false,
                show.cover.as_ref(),
                SidebarKind::Podcasts,
            ),
            Item::Artist(artist) => (
                &artist.uri,
                &artist.name,
                "Artist",
                "Artist".to_owned(),
                media_size(
                    RootKind::Artists,
                    artist.releases.map(|count| count as usize),
                ),
                true,
                artist.portrait.as_ref(),
                SidebarKind::Artists,
            ),
            Item::Album(album) => (
                &album.uri,
                &album.name,
                media_type(RootKind::Albums),
                library_kind(
                    "Album",
                    (!album.artists.is_empty()).then_some(album.artists.as_str()),
                ),
                media_size(RootKind::Albums, album.tracks.map(|count| count as usize)),
                false,
                album.cover.as_ref(),
                SidebarKind::Albums,
            ),
        };
        let composed = match (layout, item) {
            (MediaLayout::Grid, Item::Playlist(entry)) => entry.composed.as_slice(),
            (MediaLayout::Grid, _) | (MediaLayout::List, _) => &[],
        };
        MediaWords {
            uri: uri.clone(),
            name: name.clone(),
            kind,
            sub,
            size,
            round,
            shelf,
            playable: Library::playable(shelf),
            cover: self.art.look(cover, want, self.now),
            picture: self.composed_picture(composed, want.div_ceil(2)),
        }
    }

    /// What a tile draws in its art box: the four covers it tiles, once every
    /// one is held, else the single picture the words already carry.
    fn composed_picture(&mut self, composed: &[CoverSet], want: u32) -> hero::Picture {
        let drawn: Vec<Cover> = composed
            .iter()
            .map(|cover| self.art.look(Some(cover), want, self.now))
            .collect();
        match mosaic::arrange(&drawn) {
            Some(covers) => hero::Picture::Mosaic(covers),
            None => hero::Picture::Cover,
        }
    }
}

/// One item's words and pictures, owned so a borrow of the item is not needed.
pub(super) struct MediaWords {
    uri: String,
    name: String,
    kind: &'static str,
    pub(super) sub: String,
    size: String,
    round: bool,
    pub(super) shelf: SidebarKind,
    playable: bool,
    cover: Cover,
    pub(super) picture: hero::Picture,
}

impl MediaWords {
    pub(super) fn as_media(&self) -> media::Media<'_> {
        media::Media {
            uri: &self.uri,
            name: &self.name,
            kind: self.kind,
            sub: &self.sub,
            size: &self.size,
            round: self.round,
            shelf: self.shelf,
            playable: self.playable,
            cover: self.cover,
            picture: self.picture,
        }
    }
}

/// The solved media grid for the current width.
///
/// The reference's library grid is `minmax(11rem, 1fr)`, which is wider than the
/// grid the album page's releases use.
pub(in crate::view) fn media_grid(ui: &mut Ui) -> cards::Grid {
    let area = Rect::from_min_size(ui.cursor().min, vec2(ui.available_width(), 0.0));
    cards::grid(
        area,
        extent::MEDIA_MIN_W,
        space::S3,
        media::extra_height(ui),
    )
}

/// The rect that holds every tile: the width the grid solved and the rows'
/// total height, gaps included.
fn grid_rect(ui: &mut Ui, grid: &cards::Grid, count: usize) -> Rect {
    let height = if count == 0 {
        0.0
    } else {
        let rows = grid.rows(count);
        rows as f32 * grid.card_rect(0).height() + (rows - 1) as f32 * space::S3
    };
    Rect::from_min_size(ui.cursor().min, vec2(ui.available_width(), height))
}

/// The note under the cards while the followed list is loading or failed.
///
/// The copy and the Retry are the same pair the playlists' failure draws, in the
/// same order: the words first, then the control that asks again.
fn load_note(ui: &mut Ui, note: LoadNote, tokens: &Tokens, retry: &mut Option<FollowedRead>) {
    match note {
        LoadNote::Loading(read) => {
            ui.add_space(space::S3);
            ui.label(recipe::muted(read.loading(), tokens));
        }
        LoadNote::Failed(read, failure) => {
            ui.add_space(space::S3);
            ui.label(recipe::muted(
                crate::signin::failure_message(failure),
                tokens,
            ));
            if ui.button("Retry").clicked() {
                *retry = Some(read);
            }
        }
        LoadNote::None => {}
    }
}

/// Starts a playlist's load from an idle-grid card click.
///
/// `Library::select` always starts a load and returns its command, and that is
/// all this does. The grid is drawn only at the root, so the card under a click
/// is never the playlist whose tracks are loading; there is nothing to suppress
/// here.
///
/// Going to the root is the one way the same playlist can be clicked again while
/// its first fetch is still coming back: it clears the stale-reply guard and the
/// in-flight load, and the library keeps no record of the request, so the second
/// click does start it again. The first reply is discarded as stale, so the
/// repeat costs a fetch and nothing else.
fn select_playlist(library: &mut Library, uri: &str) -> Option<Command> {
    Some(library.select(uri))
}

#[cfg(test)]
mod root_tests {
    use std::time::Instant;

    use egui::{Context, Event, Pos2, RawInput, Rect, Vec2};
    use takofy_model::{CoverUrl, Episode, FollowedShow, PlaylistAccess, PlaylistEntry, Revision};

    use super::*;
    use crate::layout::Table;
    use crate::menu::{Anchor, Device, Kind, Listing, Request};
    use crate::message::{Failure, Update};
    use crate::player::Transport;
    use crate::queue::Queue;
    use crate::theme::{extent, fonts, style};
    use crate::view::playlist::tracks_view;
    use crate::view::test_support::{click_events, secondary_click_events};

    const PROBE: Vec2 = Vec2::new(900.0, 800.0);
    const NOW_MS: u64 = 1_761_134_400_000;

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

    fn run(
        ctx: &Context,
        events: Vec<Event>,
        library: &mut Library,
        art: &mut Art,
    ) -> egui::FullOutput {
        let tokens = Tokens::DARK;
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut output = ctx.run_ui(input(events), |ui| {
            let _ = tracks_view(
                ui,
                Table::Full,
                library,
                &mut transport,
                &mut queue,
                art,
                Instant::now(),
                NOW_MS,
                &tokens,
            );
        });
        output.textures_delta.clear();
        output
    }

    fn frame(ctx: &Context, library: &mut Library, art: &mut Art) -> egui::FullOutput {
        run(ctx, Vec::new(), library, art)
    }

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

    fn playlist(uri: &str, name: &str) -> PlaylistEntry {
        PlaylistEntry {
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

    fn show(uri: &str, name: &str) -> FollowedShow {
        FollowedShow {
            uri: uri.to_owned(),
            name: name.to_owned(),
            named: true,
            publisher: None,
            cover: None,
            episodes: None,
        }
    }

    fn library(playlists: Vec<PlaylistEntry>, shows: Vec<FollowedShow>) -> Library {
        let mut library = Library::default();
        library.apply(&Update::Playlists(playlists));
        library.apply(&Update::FollowedShows(shows));
        library
    }

    fn click(ctx: &Context, pos: Pos2, library: &mut Library, art: &mut Art) -> Option<Command> {
        let tokens = Tokens::DARK;
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut command = None;
        for event in click_events(pos) {
            let command = &mut command;
            let library = &mut *library;
            let output = ctx.run_ui(input(vec![event]), |ui| {
                if let Some(next) = tracks_view(
                    ui,
                    Table::Full,
                    library,
                    &mut transport,
                    &mut queue,
                    art,
                    Instant::now(),
                    NOW_MS,
                    &tokens,
                ) {
                    *command = Some(next);
                }
            });
            let mut output = output;
            output.textures_delta.clear();
        }
        command
    }

    /// The rect a chip's label was painted into, plus its own padding.
    fn chip_rect(output: &egui::FullOutput, label: &str) -> Rect {
        let text = text_rect(output, &label.to_uppercase()).expect("the chip is drawn");
        text.expand2(Vec2::new(extent::CHIP_PAD_X, extent::CHIP_PAD_Y))
    }

    /// The rect the `index`-th card is drawn into, solved the way the view
    /// solves it: the head, the chip row, then the grid at the probe's width.
    fn card_rect_for(ctx: &Context, index: usize, count: usize) -> Rect {
        let tokens = Tokens::DARK;
        let mut found = None;
        let mut output = ctx.run_ui(input(Vec::new()), |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                crate::view::inner(ui, |ui| {
                    page_head::show(ui, "Your Library", &library_note(count), &tokens);
                    let _ = filter_row::show_with(
                        ui,
                        &tokens,
                        &library_filter::ROW,
                        RootKind::All,
                        RootKind::control,
                        root_filter_label,
                        filter_row::RowSlot::Trailing(0.0),
                    );
                    let solved = media_grid(ui);
                    found = Some(solved.card_rect(index));
                });
            });
        });
        output.textures_delta.clear();
        found.expect("the root lays out")
    }

    fn followed_artist(uri: &str, name: &str) -> takofy_model::FollowedArtist {
        takofy_model::FollowedArtist {
            uri: uri.to_owned(),
            name: name.to_owned(),
            named: true,
            portrait: None,
            releases: None,
        }
    }

    fn saved_album(uri: &str, name: &str) -> takofy_model::SavedAlbum {
        takofy_model::SavedAlbum {
            uri: uri.to_owned(),
            name: name.to_owned(),
            named: true,
            artists: "Nils Aker".to_owned(),
            year: None,
            cover: None,
            tracks: None,
        }
    }

    /// Each of the four kinds knows the shelf it belongs to, so a menu built on
    /// it never has to read the uri back.
    #[test]
    fn media_of_gives_each_item_its_shelf() {
        let mut art = Art::default();
        let tokens = Tokens::DARK;
        let mut select = None;
        let mut open = None;
        let mut play = None;
        let library = Library::default();
        let mut draw = MediaDraw {
            art: &mut art,
            now: Instant::now(),
            tokens: &tokens,
            library: &library,
            select: &mut select,
            open: &mut open,
            play: &mut play,
        };
        let playlist = playlist("spotify:playlist:a", "A");
        let show = show("spotify:show:a", "A");
        let artist = followed_artist("spotify:artist:a", "A");
        let album = saved_album("spotify:album:a", "A");
        let cases = [
            (Item::Playlist(&playlist), SidebarKind::Playlists),
            (Item::Show(&show), SidebarKind::Podcasts),
            (Item::Artist(&artist), SidebarKind::Artists),
            (Item::Album(&album), SidebarKind::Albums),
        ];
        for (item, shelf) in cases {
            assert_eq!(draw.media_of(&item, MediaLayout::Grid, 176).shelf, shelf);
        }
    }

    /// A rootlist, which a playlist row needs before it can be removed.
    fn rootlist(editable: bool) -> PlaylistAccess {
        PlaylistAccess {
            revision: Revision::from_wire(b"root-1").expect("a revision"),
            editable,
        }
    }

    /// The rect the `index`-th list row is drawn into, solved the way the view
    /// solves it: the head, the chips, then the rows.
    fn row_rect_for(ctx: &Context, index: usize) -> Rect {
        let tokens = Tokens::DARK;
        let mut found = None;
        let mut output = ctx.run_ui(input(Vec::new()), |ui| {
            ui.add_space(space::S3);
            crate::view::inner(ui, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        crate::view::inner(ui, |ui| {
                            page_head::show(ui, "Your Library", "", &tokens);
                            let _ = filter_row::show_with(
                                ui,
                                &tokens,
                                &library_filter::ROW,
                                RootKind::All,
                                RootKind::control,
                                root_filter_label,
                                filter_row::RowSlot::Trailing(view_toggle::reserved()),
                            );
                            let width = ui.available_width();
                            let height = space::S2 + media::head_height(ui) + space::S2;
                            let _ =
                                ui.allocate_exact_size(vec2(width, height), egui::Sense::hover());
                            let top = ui.cursor().min.y;
                            found = Some(Rect::from_min_size(
                                Pos2::new(ui.cursor().min.x, top + index as f32 * extent::ROW_H),
                                vec2(width, extent::ROW_H),
                            ));
                        });
                    });
            });
        });
        output.textures_delta.clear();
        found.expect("the root lays out")
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
        assert_eq!(labels, vec!["Copy link", "Remove from library"]);
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

        let request =
            press_the_more(&ctx, &mut library, &mut art).expect("the `⋯` posted no request");
        let labels: Vec<&str> = request
            .items
            .iter()
            .map(|item| item.label.as_str())
            .collect();
        assert_eq!(
            labels,
            vec!["Copy link"],
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

        let by_more =
            press_the_more(&ctx, &mut library, &mut art).expect("the `⋯` posted no request");
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

    #[test]
    fn idle_view_artists_ready_loading_failed_and_empty() {
        let mut root = library(vec![playlist("a", "A")], vec![]);
        root.root_filter.choose(RootKind::Artists);

        // Not asked for yet: loading, not the empty copy.
        assert_eq!(
            idle_view(&root),
            IdleView::Grid {
                items: Vec::new(),
                note: LoadNote::Loading(FollowedRead::Artists),
            }
        );

        root.apply(&Update::FollowedArtists(vec![
            followed_artist("spotify:artist:1", "One"),
            followed_artist("spotify:artist:2", "Two"),
        ]));
        let IdleView::Grid { items, note } = idle_view(&root) else {
            panic!("a ready artists list is a grid");
        };
        assert_eq!(items.len(), 2, "both artists draw as cards");
        assert!(matches!(items[0], Item::Artist(artist) if artist.uri.ends_with(":1")));
        assert_eq!(note, LoadNote::None);

        root.apply(&Update::FollowedArtistsFailed(Failure::FollowedArtists));
        assert_eq!(
            idle_view(&root),
            IdleView::Grid {
                items: Vec::new(),
                note: LoadNote::Failed(FollowedRead::Artists, Failure::FollowedArtists),
            }
        );

        let mut empty = library(vec![playlist("a", "A")], vec![]);
        empty.root_filter.choose(RootKind::Artists);
        empty.apply(&Update::FollowedArtists(Vec::new()));
        assert_eq!(
            idle_view(&empty),
            IdleView::Empty(RootKind::Artists),
            "nothing to show is the chip's own empty line"
        );
    }

    #[test]
    fn idle_view_albums_carry_their_own_load() {
        let mut library = library(vec![playlist("a", "A")], vec![]);
        library.root_filter.choose(RootKind::Albums);
        library.apply(&Update::SavedAlbums(vec![saved_album(
            "spotify:album:1",
            "Slow Tide",
        )]));

        let IdleView::Grid { items, note } = idle_view(&library) else {
            panic!("a ready albums list is a grid");
        };
        assert_eq!(items.len(), 1);
        assert_eq!(note, LoadNote::None);

        library.apply(&Update::SavedAlbumsFailed(Failure::SavedAlbums));
        let IdleView::Grid { note, .. } = idle_view(&library) else {
            panic!("a failed albums list keeps the grid");
        };
        assert_eq!(
            note,
            LoadNote::Failed(FollowedRead::Albums, Failure::SavedAlbums)
        );
    }

    /// The note's Retry asks the read it names, not the shows.
    #[test]
    fn the_artists_note_retries_the_artists() {
        let ctx = context();
        let mut library = library(vec![playlist("a", "A")], vec![]);
        library.root_filter.choose(RootKind::Artists);
        library.apply(&Update::FollowedArtistsFailed(Failure::FollowedArtists));
        let mut art = Art::default();

        let before = frame(&ctx, &mut library, &mut art);
        let retry = text_rect(&before, "Retry").expect("the note's retry");
        let command = click(&ctx, retry.center(), &mut library, &mut art);

        assert_eq!(command, Some(Command::LoadFollowedArtists));
        assert_eq!(library.followed_artists, Load::Loading);
    }

    #[test]
    fn a_show_card_click_opens_the_show_and_returns_load_show() {
        let ctx = context();
        let mut library = library(vec![playlist("a", "A")], vec![show("s", "Show One")]);
        let mut art = Art::default();
        let _ = frame(&ctx, &mut library, &mut art);

        let card = card_rect_for(&ctx, 1, 2);
        let command = click(&ctx, card.center(), &mut library, &mut art);

        assert_eq!(
            command,
            Some(Command::LoadShow {
                show: "s".to_owned()
            }),
            "a show card opens its page"
        );
        assert_eq!(library.page(), Some(&Page::Show("s".to_owned())));
    }

    #[test]
    fn a_playlist_card_click_still_selects() {
        let ctx = context();
        let mut library = library(vec![playlist("a", "A")], vec![show("s", "Show One")]);
        let mut art = Art::default();
        let _ = frame(&ctx, &mut library, &mut art);

        let card = card_rect_for(&ctx, 0, 2);
        let command = click(&ctx, card.center(), &mut library, &mut art);

        assert_eq!(
            command,
            Some(Command::LoadTracks {
                playlist: "a".to_owned()
            })
        );
    }

    #[test]
    fn choosing_podcasts_draws_only_show_cards() {
        let ctx = context();
        let mut library = library(vec![playlist("a", "List One")], vec![show("s", "Show One")]);
        let mut art = Art::default();

        let before = frame(&ctx, &mut library, &mut art);
        let chip = chip_rect(&before, "Podcasts");
        let _ = click(&ctx, chip.center(), &mut library, &mut art);

        let after = frame(&ctx, &mut library, &mut art);
        let texts = drawn_texts(&after);
        assert!(texts.contains(&"Show One"), "{texts:?}");
        assert!(
            !texts.contains(&"List One"),
            "the playlists card must be gone: {texts:?}"
        );
    }

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

    /// Pressing the Artists chip takes the choice, asks for the list once, and
    /// the next frame says it is loading rather than claiming the library is
    /// empty.
    #[test]
    fn the_artists_chip_asks_for_the_list_and_says_it_is_loading() {
        let ctx = context();
        let mut library = library(vec![playlist("a", "A")], vec![]);
        let mut art = Art::default();

        let before = frame(&ctx, &mut library, &mut art);
        let chip = chip_rect(&before, "Artists");
        let _ = click(&ctx, chip.center(), &mut library, &mut art);

        assert_eq!(library.root_filter.chosen(), RootKind::Artists);
        assert_eq!(
            library.followed_artists_request(),
            Some(Command::LoadFollowedArtists)
        );
        assert_eq!(library.followed_artists_request(), None, "asked once");

        let after = frame(&ctx, &mut library, &mut art);
        let texts = drawn_texts(&after);
        assert!(
            texts.contains(&"Loading your artists…"),
            "the root says what it is doing: {texts:?}"
        );
        assert!(
            !texts.contains(&"You don't follow any artists yet."),
            "and does not claim the list is empty: {texts:?}"
        );
    }

    #[test]
    fn the_followed_retry_returns_load_followed_shows() {
        let ctx = context();
        let mut library = library(vec![playlist("a", "A")], vec![]);
        library.apply(&Update::FollowedShowsFailed(Failure::Load));
        let mut art = Art::default();

        let before = frame(&ctx, &mut library, &mut art);
        let retry = text_rect(&before, "Retry").expect("the followed note's retry");
        let command = click(&ctx, retry.center(), &mut library, &mut art);

        assert_eq!(command, Some(Command::LoadFollowedShows));
    }

    #[test]
    fn the_page_head_note_counts_the_cards_shown() {
        let ctx = context();
        let mut library = library(
            vec![playlist("a", "A"), playlist("b", "B")],
            vec![show("s", "Show One")],
        );
        let mut art = Art::default();

        let output = frame(&ctx, &mut library, &mut art);
        let texts = drawn_texts(&output);
        assert!(
            texts.contains(&"3 SAVED"),
            "the note counts every card: {texts:?}"
        );

        library.root_filter.choose(RootKind::Podcasts);
        let output = frame(&ctx, &mut library, &mut art);
        let texts = drawn_texts(&output);
        assert!(
            texts.contains(&"1 SAVED"),
            "a narrowed chip counts only its own cards: {texts:?}"
        );
    }

    #[test]
    fn only_visible_cards_ask_for_covers() {
        let ctx = context();
        let shows: Vec<FollowedShow> = (0..40)
            .map(|index| FollowedShow {
                uri: format!("spotify:show:{index}"),
                name: format!("Show {index}"),
                named: true,
                publisher: None,
                cover: Some(CoverSet::single(
                    CoverUrl::from_wire(&format!("https://i.scdn.co/image/{index:0<40}"))
                        .expect("a cover url"),
                )),
                episodes: None,
            })
            .collect();
        let mut library = library(vec![], shows);
        let mut art = Art::default();
        let now = Instant::now();

        art.begin_frame();
        let _ = frame(&ctx, &mut library, &mut art);
        let requests = art.requests(now);
        assert!(!requests.is_empty(), "the visible cards must be fetched");
        assert!(
            requests.len() < 20,
            "only the cards the viewport shows are asked for: {}",
            requests.len()
        );
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

    #[test]
    fn a_press_on_a_cards_play_wants_the_play_and_selects_nothing() {
        let ctx = context();
        let mut art = Art::default();
        let mut library = library(vec![playlist("spotify:playlist:a", "Mix")], Vec::new());
        let card = card_rect_for(&ctx, 0, 1);

        let command = click(
            &ctx,
            media::tile_play_rect(card).center(),
            &mut library,
            &mut art,
        );

        assert_eq!(command, None, "a play press took the frame's command");
        assert_eq!(library.page(), None, "a play press opened the page");
        assert_eq!(library.selected(), None, "a play press selected the page");
        assert_eq!(
            library.play_request(),
            Some(Command::LoadTracks {
                playlist: "spotify:playlist:a".to_owned()
            }),
            "the press never became a want"
        );
    }

    #[test]
    fn a_press_on_a_rows_play_wants_the_play_and_opens_nothing() {
        let ctx = context();
        let mut art = Art::default();
        let mut library = library(vec![playlist("spotify:playlist:a", "Mix")], Vec::new());
        library.layout = MediaLayout::List;
        let row = row_rect_for(&ctx, 0);

        let command = click(
            &ctx,
            media::row_play_rect(row).center(),
            &mut library,
            &mut art,
        );

        assert_eq!(command, None, "a play press took the frame's command");
        assert_eq!(library.page(), None, "a play press opened the page");
        assert_eq!(library.selected(), None, "a play press selected the page");
        assert_eq!(
            library.play_request(),
            Some(Command::LoadTracks {
                playlist: "spotify:playlist:a".to_owned()
            }),
            "the press never became a want"
        );
    }

    #[test]
    fn a_failed_shelf_play_draws_the_load_sentence_under_the_chips() {
        let ctx = context();
        let mut art = Art::default();
        let mut library = library(vec![playlist("spotify:playlist:a", "Mix")], Vec::new());
        assert!(library.want_play("spotify:playlist:a", SidebarKind::Playlists));
        assert!(library.play_request().is_some());
        library.apply(&Update::TracksFailed {
            playlist: "spotify:playlist:a".to_owned(),
            failure: Failure::Load,
        });

        let output = frame(&ctx, &mut library, &mut art);
        let sentence = crate::signin::failure_message(Failure::Load);
        let line = text_rect(&output, sentence).expect("the failure sentence is drawn");
        let chips = text_rect(&output, "ALL").expect("the chips are drawn");
        let card = text_rect(&output, "Mix").expect("the card is drawn");

        assert!(
            line.top() >= chips.bottom(),
            "the sentence sits over the chips: {line:?} in {chips:?}"
        );
        assert!(
            line.bottom() <= card.top(),
            "the sentence sits under the cards: {line:?} in {card:?}"
        );
    }
}
