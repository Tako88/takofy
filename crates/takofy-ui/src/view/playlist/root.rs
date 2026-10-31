//! The library root: the cards behind the chosen chip, and the state of the
//! two loads that fill them.

use std::time::Instant;

use egui::{Pos2, Rect, Ui, vec2};
use takofy_model::{CoverSet, Owner};

use crate::cover::{Art, Cover, want_px};
use crate::format::{
    copy_note, library_kind, library_note, media_size, media_type, root_empty_label,
    root_filter_label, root_loading_label,
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
                // The cards above are last session's, and the read that would
                // replace them failed. The grid stays drawn: retrying asks for
                // the lists again rather than blanking what the reader has.
                if library.copy_failed() {
                    ui.label(recipe::muted(copy_note(), tokens));
                    if ui.button("Retry").clicked() {
                        retry_playlists = true;
                    }
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
pub(in crate::view) struct MediaDraw<'a, 'b> {
    pub(in crate::view) art: &'a mut Art,
    pub(in crate::view) now: Instant,
    pub(in crate::view) tokens: &'a Tokens,
    pub(in crate::view) library: &'b Library,
    pub(in crate::view) select: &'b mut Option<String>,
    pub(in crate::view) open: &'b mut Option<Page>,
    /// The object whose play was pressed, and the list it belongs to.
    pub(in crate::view) play: &'b mut Option<(String, SidebarKind)>,
}

impl MediaDraw<'_, '_> {
    /// Records a press on an item's play. The want is taken after the scroll
    /// area, so the borrow of the items has ended by the time it is held.
    pub(in crate::view) fn want_play(&mut self, media: &MediaWords) {
        *self.play = Some((media.uri.clone(), media.shelf));
    }

    /// What a click on one item does: a playlist selects, the rest open their
    /// page.
    pub(in crate::view) fn activate(&mut self, item: &Item<'_>, uri: &str) {
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
    pub(in crate::view) fn media_of(
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
                media_type(RootKind::Artists),
                library_kind(media_type(RootKind::Artists), None),
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
pub(in crate::view) struct MediaWords {
    uri: String,
    name: String,
    kind: &'static str,
    pub(in crate::view) sub: String,
    size: String,
    round: bool,
    pub(in crate::view) shelf: SidebarKind,
    playable: bool,
    cover: Cover,
    pub(in crate::view) picture: hero::Picture,
}

impl MediaWords {
    pub(in crate::view) fn as_media(&self) -> media::Media<'_> {
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
/// total height, gaps included. An empty grid has no rows, so it asks for no
/// height and the list after it starts where the head left off.
pub(in crate::view) fn grid_rect(ui: &mut Ui, grid: &cards::Grid, count: usize) -> Rect {
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
mod tests;
