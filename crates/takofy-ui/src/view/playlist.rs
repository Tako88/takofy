//! The main pane's tracklist: the selected playlist's tracks, or the state of
//! their load.
//!
//! The header and the rows are solved from one width and one set of tokens, so the
//! columns line up by construction rather than because two pieces agree. The header
//! scrolls with the list, which is what the reference's tracklist does.

use std::time::Instant;

use egui::{Pos2, Rect, Sense, Ui, vec2};
use takofy_model::PlaylistEntry;

use crate::cover::{Art, Cover};
use crate::format::{
    added_label, byline, format_position, library_kind, library_note, media_size, media_type,
    plain_description, root_empty_label, root_filter_label, root_loading_label, total_duration_ms,
};
use crate::layout::{Table, cards, table as table_layout};
use crate::library::{BandCover, Library, Load, MediaLayout, band_cover};
use crate::library_filter::{self, Item, RootKind};
use crate::message::{Command, Failure};
use crate::player::Transport;
use crate::queue::Queue;
use crate::route::Page;
use crate::theme::{Tokens, extent, space};

use super::filter_row;
use super::hero;
use super::media;
use super::skeleton;
use super::text as recipe;
use super::track_list::{head, list_rect};
use super::track_row::{self, Click, Row, state_of};
use super::view_toggle;

/// The main pane: the selected playlist's tracks, or the state of their load.
#[expect(
    clippy::too_many_arguments,
    reason = "the pane, its table, the state it draws and the frame's clocks"
)]
pub fn tracks_view(
    ui: &mut Ui,
    table: Table,
    library: &mut Library,
    transport: &mut Transport,
    queue: &mut Queue,
    art: &mut Art,
    now: Instant,
    now_ms: u64,
    tokens: &Tokens,
) -> Option<Command> {
    ui.add_space(space::S3);

    // Dispatch on the page, not on the tracks: the root is the grid, and the
    // tracks may still hold a ready playlist that nothing draws.
    match main_page(library) {
        MainPage::Root => return super::inner(ui, |ui| idle(ui, library, art, now, tokens)),
        MainPage::Album => {
            return super::album::album_view(
                ui, table, library, transport, queue, art, now, tokens,
            );
        }
        MainPage::Artist => {
            return super::artist::artist_view(
                ui, table, library, transport, queue, art, now, tokens,
            );
        }
        MainPage::Show => {
            return super::show::show_view(ui, library, transport, queue, art, now, now_ms, tokens);
        }
        MainPage::Playlist => {}
    }

    // The band draws `Loading` until the selected playlist's tracks settle: it
    // must not fetch a first-track cover it is about to replace with the
    // context image.
    let settled = !matches!(library.tracks, Load::Loading);
    let mut command = None;
    let mut retry = false;
    // A link click is applied after the scroll area, because the matches below
    // borrow the tracks it has to move away from.
    let mut open: Option<Page> = None;
    egui::ScrollArea::vertical().show(ui, |ui| {
        // The band heads the playlist whenever the library can name the
        // selection; with no name there is nothing to say above the rows. It
        // takes the width and starts at the cursor, so the header and the rows
        // follow it down one scroll area.
        if let Some(entry) = library.selected_entry() {
            let cover = band_cover_for(entry, settled, art, now);
            let ready = match &library.tracks {
                Load::Ready(tracks) => Some(tracks),
                Load::Idle | Load::Loading | Load::Failed(_) => None,
            };
            let tracks = ready
                .filter(|tracks| !tracks.tracks.is_empty())
                .map(|tracks| (tracks.tracks.len(), total_duration_ms(&tracks.tracks)));
            // The owner and the description come from the listing; the count and
            // the total wait for the tracks. The description is owned here so the
            // band can borrow it for as long as it is drawn.
            let meta = byline(entry.owner.as_deref(), tracks);
            // The description is drawn as the wire sends it: whitespace is
            // collapsed, but markup it carries (entities, `spotify:` links)
            // draws literally.
            let description = entry
                .description
                .as_deref()
                .and_then(plain_description)
                .unwrap_or_default();
            let band = hero::Band {
                eyebrow: "Playlist",
                title: &entry.name,
                meta,
                description: &description,
                playable: tracks.is_some(),
                shuffle: true,
                cover,
            };
            if hero::show(ui, &band, tokens)
                && let Some(tracks) = ready
            {
                command = queue.start(&tracks.tracks, 0, transport);
            }
        }

        super::inner(ui, |ui| {
            let width = ui.available_width();
            match &library.tracks {
                Load::Idle => {}
                Load::Loading => {
                    // The listing already names how many tracks there are, so the
                    // page holds that many rows open rather than a screenful.
                    let count = skeleton::row_count(
                        library.selected_entry().and_then(|entry| entry.tracks),
                    );
                    skeleton::rows(ui, count, table.columns().for_playlist(true), tokens);
                }
                Load::Failed(failure) => {
                    ui.label(recipe::muted(
                        crate::signin::failure_message(*failure),
                        tokens,
                    ));
                    if ui.button("Retry").clicked() {
                        retry = true;
                    }
                }
                Load::Ready(tracks) if tracks.tracks.is_empty() => {
                    ui.label(recipe::muted("No tracks in this playlist.", tokens));
                }
                Load::Ready(tracks) => {
                    let columns = table_layout::columns(
                        width,
                        table
                            .columns()
                            .for_playlist(table_layout::is_playlist(&tracks.playlist)),
                    );
                    head(ui, columns, tokens);
                    let count = tracks.tracks.len();
                    // The whole list is reserved at once, so the scrollbar is right,
                    // and only the rows the viewport can show are laid out and given
                    // interaction: a thousand-track playlist paints the dozen rows on
                    // screen.
                    let list = list_rect(ui, count);
                    ui.allocate_rect(list, Sense::hover());
                    let visible = ui.clip_rect();

                    for (index, entry) in tracks.tracks.iter().enumerate() {
                        let rect = table_layout::row_rect(list, index, extent::ROW_H);
                        if !visible.intersects(rect) {
                            continue;
                        }
                        let duration = entry.duration_ms.map(format_position);
                        let added = entry.added_at.and_then(|added| added_label(added, now_ms));
                        let row = Row {
                            uri: &entry.uri,
                            number: index + 1,
                            title: &entry.title,
                            title_cell: track_row::TitleCell::ArtThenName,
                            artist: &entry.artists,
                            artist_uri: entry.artist_uris.first().map(String::as_str),
                            album: entry.album.as_deref(),
                            album_uri: entry.album_uri.as_deref(),
                            added: added.as_deref(),
                            duration: duration.as_deref(),
                            state: state_of(&entry.uri, transport),
                            cover: art.look(entry.cover.as_ref(), now),
                        };
                        match track_row::show(ui, rect, columns, &row, tokens) {
                            Some(Click::Play) => {
                                command = queue.start(&tracks.tracks, index, transport);
                            }
                            Some(Click::Open(page)) => open = Some(page),
                            None => {}
                        }
                    }
                }
            }
        });
    });

    if retry {
        let playlist = library.selected().map(str::to_owned);
        return playlist.map(|playlist| library.select(&playlist));
    }
    if let Some(page) = open {
        return library.open(page);
    }
    command
}

/// The cover the band draws this frame.
///
/// Once the tracks have settled this is the entry's own cover. While they load
/// it is `Pending`, but a texture the store already holds is kept on screen
/// rather than flashing the placeholder: a reload that re-selected the same
/// playlist shows the picture it already had, and only an entry with nothing
/// held draws `Loading`.
fn band_cover_for(entry: &PlaylistEntry, settled: bool, art: &mut Art, now: Instant) -> Cover {
    match band_cover(entry, settled) {
        BandCover::Pending => entry
            .cover
            .as_ref()
            .and_then(|url| art.look_held(url, now))
            .unwrap_or(Cover::Loading),
        BandCover::Entry(url) => art.look(url, now),
    }
}

/// What the page on screen asks the main pane to draw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MainPage {
    /// The library root: the playlists grid.
    Root,
    /// A playlist's tracklist.
    Playlist,
    /// An album's band and table.
    Album,
    /// An artist's hero, Popular list and Discography.
    Artist,
    /// A show's band and its episodes.
    Show,
}

/// Reads the pane's dispatch from the current page.
fn main_page(library: &Library) -> MainPage {
    match library.page() {
        None => MainPage::Root,
        Some(Page::Playlist(_)) => MainPage::Playlist,
        Some(Page::Album(_)) => MainPage::Album,
        Some(Page::Artist(_)) => MainPage::Artist,
        Some(Page::Show(_)) => MainPage::Show,
    }
}

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
/// A chip that draws a followed list says what its own read is doing; All and
/// Podcasts say it for the shows, which is the list they have always drawn.
fn note_for(library: &Library, chosen: RootKind) -> LoadNote {
    match chosen {
        RootKind::Playlists => LoadNote::None,
        RootKind::All | RootKind::Podcasts => index_note(FollowedRead::Shows, &library.followed),
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
fn idle(
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
    let mut retry_playlists = false;
    let mut retry: Option<FollowedRead> = None;

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            super::inner(ui, |ui| {
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
                page_head(ui, &note, tokens);

                let (picked, row) = filter_row::show_with(
                    ui,
                    tokens,
                    &library_filter::ROW,
                    library.root_filter.chosen(),
                    RootKind::control,
                    root_filter_label,
                    filter_row::RowSlot::Trailing(view_toggle::reserved()),
                );
                choose = picked;
                if let Some(layout) = view_toggle::show(ui, tokens, row, library.layout) {
                    relayout = Some(layout);
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
                                select: &mut select,
                                open: &mut open,
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

/// The page head: the title, the note at the far end and the rule under both.
///
/// The reference sets the title at `--text-2xl` in 800 and the note as a small
/// tracked mono line sitting on the title's baseline, then closes the head with
/// a hairline and one `S5` of air before the chips.
fn page_head(ui: &mut Ui, note: &str, tokens: &Tokens) {
    let width = ui.available_width();
    let font = recipe::page_title();
    let title_h = ui.ctx().fonts_mut(|fonts| fonts.row_height(&font));
    let height = space::S3 + title_h + space::S4;
    let (rect, _) = ui.allocate_exact_size(vec2(width, height), egui::Sense::hover());

    let top = rect.top() + space::S3;
    let title = recipe::line(
        ui,
        "Your Library",
        font,
        tokens.palette.text,
        recipe::tracking_pts(crate::theme::text::XXL, crate::theme::tracking::TIGHT),
        width,
    );
    ui.painter().galley(
        egui::Pos2::new(rect.left(), top),
        title,
        tokens.palette.text,
    );

    if !note.is_empty() {
        let line = recipe::spaced(
            ui,
            &note.to_uppercase(),
            egui::FontId::new(
                crate::theme::text::XXS,
                crate::theme::fonts::mono(crate::theme::MonoWeight::Medium),
            ),
            tokens.palette.text_subtle,
            recipe::tracking_pts(crate::theme::text::XXS, crate::theme::tracking::WIDE),
        );
        ui.painter().galley(
            egui::Pos2::new(rect.right() - line.size().x, top + title_h - line.size().y),
            line,
            tokens.palette.text_subtle,
        );
    }

    ui.painter().rect_filled(
        Rect::from_min_size(
            egui::Pos2::new(rect.left(), rect.bottom() - crate::theme::stroke::HAIRLINE),
            vec2(rect.width(), crate::theme::stroke::HAIRLINE),
        ),
        0.0,
        tokens.palette.border,
    );
    ui.add_space(space::S5);
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
        if !visible.intersects(rect) {
            continue;
        }
        let media = draw.media_of(item);
        if media::tile(ui, rect, &media.as_media(), draw.tokens) {
            draw.activate(item, &media.uri);
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
        if !visible.intersects(rect) {
            continue;
        }
        let media = draw.media_of(item);
        if media::row(ui, rect, &media.as_media(), draw.tokens) {
            draw.activate(item, &media.uri);
        }
    }
}

/// Where a click on one item goes, and the frame's drawing context.
struct MediaDraw<'a, 'b> {
    art: &'a mut Art,
    now: Instant,
    tokens: &'a Tokens,
    select: &'b mut Option<String>,
    open: &'b mut Option<Page>,
}

impl MediaDraw<'_, '_> {
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

    /// One item's words and cover, owned so a borrow of the item is not needed.
    fn media_of(&mut self, item: &Item<'_>) -> MediaWords {
        let (uri, name, kind, sub, size, round, cover) = match item {
            Item::Playlist(entry) => (
                &entry.uri,
                &entry.name,
                media_type(RootKind::Playlists),
                library_kind("Playlist", entry.owner.as_deref()),
                media_size(RootKind::Playlists, entry.tracks),
                false,
                entry.cover.as_ref(),
            ),
            Item::Show(show) => (
                &show.uri,
                &show.name,
                media_type(RootKind::Podcasts),
                library_kind("Podcast", show.publisher.as_deref()),
                media_size(RootKind::Podcasts, show.episodes),
                false,
                show.cover.as_ref(),
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
            ),
        };
        MediaWords {
            uri: uri.clone(),
            name: name.clone(),
            kind,
            sub,
            size,
            round,
            cover: self.art.look(cover, self.now),
        }
    }
}

/// One item's words and cover, owned so a borrow of the item is not needed.
struct MediaWords {
    uri: String,
    name: String,
    kind: &'static str,
    sub: String,
    size: String,
    round: bool,
    cover: Cover,
}

impl MediaWords {
    fn as_media(&self) -> media::Media<'_> {
        media::Media {
            uri: &self.uri,
            name: &self.name,
            kind: self.kind,
            sub: &self.sub,
            size: &self.size,
            round: self.round,
            cover: self.cover,
        }
    }
}

/// The solved media grid for the current width.
///
/// The reference's library grid is `minmax(11rem, 1fr)`, which is wider than the
/// grid the album page's releases use.
pub(super) fn media_grid(ui: &mut Ui) -> cards::Grid {
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
mod tests {
    use egui::{
        Context, Event, Modifiers, MouseWheelSource, MouseWheelUnit, Pos2, RawInput, TouchPhase,
        Vec2,
    };

    use super::*;
    use crate::library::Library;
    use crate::message::Update;
    use crate::theme::hero::PLAY;
    use crate::theme::{fonts, style};
    use crate::view::track_row::State;
    use takofy_model::{AddedAt, AlbumDetail, CoverImage, CoverUrl, PlaylistEntry, TrackEntry};

    /// The probe window: 900 by 400, the size every bare-row test runs in.
    const PROBE: Vec2 = Vec2::new(900.0, 400.0);
    /// The frame's wall clock in the tests: 2023-11-14T22:13:20Z.
    const NOW_MS: u64 = 1_700_000_000_000;
    /// One day in milliseconds.
    const DAY: u64 = 86_400_000;
    /// A probe window tall enough to hold the band, the header and two rows in
    /// full, so a click under the band is not clipped away by the viewport.
    const TALL: Vec2 = Vec2::new(900.0, 800.0);

    fn input(events: Vec<Event>) -> RawInput {
        sized_input(PROBE, events)
    }

    fn sized_input(size: Vec2, events: Vec<Event>) -> RawInput {
        RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)),
            events,
            ..Default::default()
        }
    }

    /// Runs one pass, consumes the font-atlas delta it may have produced, and
    /// hands back the frame for reading.
    fn run(ctx: &Context, input: RawInput, draw: impl FnMut(&mut Ui)) -> egui::FullOutput {
        let mut output = ctx.run_ui(input, draw);
        output.textures_delta.clear();
        output
    }

    /// Every galley the frame painted: its text, the top of its box and its
    /// height. The band's stack is only visible in these numbers.
    fn drawn_galleys(output: &egui::FullOutput) -> Vec<(String, f32, f32)> {
        output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Text(text) => Some((
                    text.galley.text().to_owned(),
                    text.pos.y,
                    text.galley.size().y,
                )),
                _ => None,
            })
            .collect()
    }

    /// The top of the galley whose text is exactly `label`.
    fn galley_top(drawn: &[(String, f32, f32)], label: &str) -> f32 {
        drawn
            .iter()
            .find(|(text, _, _)| text == label)
            .unwrap_or_else(|| panic!("{label:?} was not painted: {drawn:?}"))
            .1
    }

    /// The height of the galley whose text is exactly `label`.
    fn galley_height(drawn: &[(String, f32, f32)], label: &str) -> f32 {
        drawn
            .iter()
            .find(|(text, _, _)| text == label)
            .unwrap_or_else(|| panic!("{label:?} was not painted: {drawn:?}"))
            .2
    }

    /// The galley whose text is exactly `label`, from whatever painted shapes
    /// hold it.
    fn galley_of(output: &egui::FullOutput, label: &str) -> std::sync::Arc<egui::Galley> {
        output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Text(text) if text.galley.text() == label => {
                    Some(text.galley.clone())
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("{label:?} was not painted"))
    }

    /// The text the frame actually painted, in paint order.
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

    /// The row indices whose title the frame painted. A row that is laid out
    /// but clipped away paints nothing, so this is the row set the window shows.
    fn painted_rows(output: &egui::FullOutput) -> Vec<usize> {
        drawn_texts(output)
            .into_iter()
            .filter_map(|text| text.strip_prefix('t'))
            .filter_map(|tail| tail.parse::<usize>().ok())
            .collect()
    }

    /// The row a cover URL names, read from the tail the test's `track_with_cover`
    /// wrote into the URL itself.
    fn cover_row(url: &CoverUrl) -> usize {
        url.as_str()
            .rsplit('c')
            .next()
            .and_then(|tail| tail.parse().ok())
            .expect("a cover URL names its row")
    }

    fn track(uri: &str, title: &str) -> TrackEntry {
        TrackEntry {
            uri: uri.to_owned(),
            title: title.to_owned(),
            artists: String::new(),
            album: None,
            album_uri: None,
            artist_uris: Vec::new(),
            added_at: None,
            duration_ms: None,
            named: true,
            cover: None,
        }
    }

    /// A track whose album and first artist both name a page, so both cells link.
    fn linkable(uri: &str) -> TrackEntry {
        TrackEntry {
            artists: "Band".to_owned(),
            album: Some("Record".to_owned()),
            album_uri: Some("spotify:album:rec".to_owned()),
            artist_uris: vec!["spotify:artist:band".to_owned()],
            ..track(uri, "t0")
        }
    }

    /// One press of `key` with no modifiers.
    fn key(key: egui::Key) -> Event {
        Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        }
    }

    fn date(millis: u64) -> AddedAt {
        AddedAt::from_wire(&millis.to_string()).expect("a date above the floor")
    }

    fn dated(uri: &str, title: &str, added_at: Option<AddedAt>) -> TrackEntry {
        TrackEntry {
            added_at,
            ..track(uri, title)
        }
    }

    /// A library whose rows belong to `playlist`, with `tracks` loaded.
    fn library_selected(playlist: &str, tracks: Vec<TrackEntry>) -> Library {
        let mut library = Library::default();
        library.select(playlist);
        library.apply(&Update::Tracks {
            playlist: playlist.to_owned(),
            tracks,
            cover: None,
        });
        library
    }

    /// One frame of the tracklist at the test's fixed clock.
    fn tracklist_frame(ctx: &Context, table: Table, library: &mut Library) -> egui::FullOutput {
        let tokens = Tokens::DARK;
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        run(ctx, input(Vec::new()), |ui| {
            let _ = tracks_view(
                ui,
                table,
                library,
                &mut transport,
                &mut queue,
                &mut Art::default(),
                Instant::now(),
                NOW_MS,
                &tokens,
            );
        })
    }

    /// One frame of the tracklist in a window of `size`.
    fn tracklist_frame_in(ctx: &Context, size: Vec2, library: &mut Library) -> egui::FullOutput {
        let tokens = Tokens::DARK;
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        run(ctx, sized_input(size, Vec::new()), |ui| {
            let _ = tracks_view(
                ui,
                Table::Full,
                library,
                &mut transport,
                &mut queue,
                &mut Art::default(),
                Instant::now(),
                NOW_MS,
                &tokens,
            );
        })
    }

    /// Draws one frame of the tracklist and returns the text it painted.
    fn painted_lines(ctx: &Context, table: Table, library: &mut Library) -> Vec<String> {
        drawn_texts(&tracklist_frame(ctx, table, library))
            .into_iter()
            .map(str::to_owned)
            .collect()
    }

    /// The visible run of every painted line: an elided line is read as what it
    /// shows, not as the string it was given.
    fn painted_runs(ctx: &Context, table: Table, library: &mut Library) -> Vec<String> {
        tracklist_frame(ctx, table, library)
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Text(text) => text.galley.rows.first(),
                _ => None,
            })
            .map(|row| row.row.text().to_owned())
            .collect()
    }

    fn library_with(tracks: Vec<TrackEntry>) -> Library {
        let mut library = Library::default();
        library.select("spotify:playlist:a");
        library.apply(&Update::Tracks {
            playlist: "spotify:playlist:a".to_owned(),
            tracks,
            cover: None,
        });
        library
    }

    /// The same selection, with the playlist list ready so the library can name
    /// it. `selected_name` is the switch the hero hangs on.
    fn library_named(tracks: Vec<TrackEntry>, name: &str) -> Library {
        let mut library = library_with(tracks);
        library.apply(&Update::Playlists(vec![PlaylistEntry {
            uri: "spotify:playlist:a".to_owned(),
            name: name.to_owned(),
            named: true,
            cover: None,
            description: None,
            owner: None,
            tracks: None,
        }]));
        library
    }

    /// The same selection, with a playlist entry that carries a description and
    /// an owner, so the band has both to draw.
    fn library_detailed(
        tracks: Vec<TrackEntry>,
        name: &str,
        description: Option<&str>,
        owner: Option<&str>,
    ) -> Library {
        let mut library = library_with(tracks);
        library.apply(&Update::Playlists(vec![listed(name, description, owner)]));
        library
    }

    /// The selection listed but its tracks not asked for yet: the band draws
    /// before anything is ready.
    fn library_listed(name: &str, description: Option<&str>, owner: Option<&str>) -> Library {
        let mut library = Library::default();
        library.select("spotify:playlist:a");
        library.apply(&Update::Playlists(vec![listed(name, description, owner)]));
        library
    }

    fn listed(name: &str, description: Option<&str>, owner: Option<&str>) -> PlaylistEntry {
        PlaylistEntry {
            uri: "spotify:playlist:a".to_owned(),
            name: name.to_owned(),
            named: true,
            cover: None,
            description: description.map(str::to_owned),
            owner: owner.map(str::to_owned),
            tracks: None,
        }
    }

    /// `count` tracks, all `duration_ms` long, so the band's total is exact.
    fn timed_tracks(count: usize, duration_ms: u32) -> Vec<TrackEntry> {
        (0..count)
            .map(|index| TrackEntry {
                duration_ms: Some(duration_ms),
                ..track(&format!("t{index}"), &format!("t{index}"))
            })
            .collect()
    }

    fn playlist(uri: &str, name: &str) -> PlaylistEntry {
        PlaylistEntry {
            uri: uri.to_owned(),
            name: name.to_owned(),
            named: true,
            cover: None,
            description: None,
            owner: None,
            tracks: None,
        }
    }

    fn show(uri: &str, name: &str) -> takofy_model::FollowedShow {
        takofy_model::FollowedShow {
            uri: uri.to_owned(),
            name: name.to_owned(),
            named: true,
            publisher: None,
            cover: None,
            episodes: None,
        }
    }

    fn cover_url(tail: &str) -> CoverUrl {
        CoverUrl::from_wire(&format!("https://i.scdn.co/image/{tail}")).expect("a cover url")
    }

    fn playlist_with_cover(uri: &str, name: &str, cover: CoverUrl) -> PlaylistEntry {
        PlaylistEntry {
            uri: uri.to_owned(),
            name: name.to_owned(),
            named: true,
            cover: Some(cover),
            description: None,
            owner: None,
            tracks: None,
        }
    }

    fn track_with_cover(index: usize) -> TrackEntry {
        TrackEntry {
            uri: format!("t{index}"),
            title: format!("t{index}"),
            artists: String::new(),
            album: None,
            album_uri: None,
            artist_uris: Vec::new(),
            added_at: None,
            duration_ms: None,
            named: true,
            cover: Some(cover_url(&format!("c{index}"))),
        }
    }

    fn image() -> CoverImage {
        CoverImage::new(1, 1, vec![0; 4]).expect("a 1x1 image")
    }

    fn fetch_urls(commands: Vec<Command>) -> Vec<CoverUrl> {
        commands
            .into_iter()
            .filter_map(|command| match command {
                Command::Fetch { url } => Some(url),
                _ => None,
            })
            .collect()
    }

    /// A library whose playlists are ready and whose tracks are untouched: the
    /// idle state the grid fills.
    fn library_idle(playlists: Vec<PlaylistEntry>) -> Library {
        let mut library = Library::default();
        library.apply(&Update::Playlists(playlists));
        library
    }

    /// The rect the idle grid draws the `index`-th card into, solved the same
    /// way the view solves it: the leading space, then the grid at the current
    /// width. Nothing here is a coordinate copied out of the view.
    fn card_rect_for(ctx: &Context, index: usize) -> Rect {
        let tokens = Tokens::DARK;
        let mut found = None;
        run(ctx, input(Vec::new()), |ui| {
            ui.add_space(space::S3);
            egui::ScrollArea::vertical().show(ui, |ui| {
                crate::view::inner(ui, |ui| {
                    page_head(ui, &library_note(2), &tokens);
                    let _ = filter_row::show_with(
                        ui,
                        &tokens,
                        &library_filter::ROW,
                        RootKind::All,
                        RootKind::control,
                        root_filter_label,
                        filter_row::RowSlot::None,
                    );
                    let solved = media_grid(ui);
                    found = Some(solved.card_rect(index));
                });
            });
        });
        found.expect("the idle grid lays out")
    }

    /// The grid sits one gutter inside the pane, like every other page's content.
    #[test]
    fn the_grid_sits_one_gutter_inside_the_pane() {
        let ctx = context();
        let card = card_rect_for(&ctx, 0);
        assert!(
            (card.left() - space::GUTTER).abs() < 0.01,
            "the first card starts at {}, the pane's gutter is {}",
            card.left(),
            space::GUTTER
        );
    }

    /// A context with the app's fonts and style installed.
    fn context() -> Context {
        let tokens = Tokens::DARK;
        let ctx = Context::default();
        ctx.set_fonts(fonts::definitions());
        ctx.set_global_style(style(&tokens));
        ctx
    }

    /// The rect `tracks_view` draws the `index`-th row into, solved the same way
    /// the view solves it: the header, then the whole list reserved in one block,
    /// then `layout::table::row_rect` for the row.
    ///
    /// `Table::Full` is the width that gives every column room, so the row is
    /// solved with all seven tracks present. Nothing here is a coordinate copied
    /// out of the view: the header and the row pitch are the view's own solver.
    fn row_rect_for(ctx: &Context, count: usize, index: usize) -> Rect {
        let tokens = Tokens::DARK;
        let table = Table::Full;
        let mut found = None;
        run(ctx, input(Vec::new()), |ui| {
            ui.add_space(space::S3);
            egui::ScrollArea::vertical().show(ui, |ui| {
                crate::view::inner(ui, |ui| {
                    let width = ui.available_width();
                    let columns = table_layout::columns(width, table.columns());
                    head(ui, columns, &tokens);
                    let list = list_rect(ui, count);
                    found = Some(table_layout::row_rect(list, index, extent::ROW_H));
                });
            });
        });
        found.expect("the header and the list lay out")
    }

    /// The rect `tracks_view` draws the `index`-th row's `cell` into, solved the
    /// way the painter solves it: the row's own rect, then the columns at its
    /// origin.
    ///
    /// The cell is the whole column, padding included. Its label is painted at
    /// the left edge, so a point near the far edge is padding the cell must also
    /// take a click on.
    fn cell_rect_for(
        ctx: &Context,
        count: usize,
        index: usize,
        cell: fn(table_layout::Columns) -> Rect,
    ) -> Rect {
        let row = row_rect_for(ctx, count, index);
        cell(table_layout::columns(row.width(), Table::Full.columns()).at(row.min))
    }

    /// The rect `tracks_view` draws the `index`-th row into while the band is
    /// drawn, solved the same way the view solves it: the band and its action row
    /// first, then the header, then the whole list reserved in one block, then
    /// `layout::table::row_rect` for the row.
    ///
    /// The band pushes the rows down. Every other geometry test builds no named
    /// playlist, so the band never appears in them, and this is the only solve
    /// that sees its offset.
    fn row_rect_below_hero(
        ctx: &Context,
        screen: Vec2,
        count: usize,
        index: usize,
        title: &str,
    ) -> Rect {
        let tokens = Tokens::DARK;
        let table = Table::Full;
        let mut found = None;
        run(ctx, sized_input(screen, Vec::new()), |ui| {
            ui.add_space(space::S3);
            egui::ScrollArea::vertical().show(ui, |ui| {
                let band = hero::Band {
                    eyebrow: "Playlist",
                    title,
                    meta: Vec::new(),
                    description: "",
                    playable: count > 0,
                    shuffle: true,
                    cover: Cover::Missing,
                };
                let _ = hero::show(ui, &band, &tokens);
                crate::view::inner(ui, |ui| {
                    let width = ui.available_width();
                    let columns = table_layout::columns(width, table.columns());
                    head(ui, columns, &tokens);
                    let list = list_rect(ui, count);
                    found = Some(table_layout::row_rect(list, index, extent::ROW_H));
                });
            });
        });
        found.expect("the hero, header and list lay out")
    }

    /// The rect `tracks_view` draws the hero's play control into, solved the way
    /// the view solves it: the band takes the width and a fixed height, the action
    /// row a fixed height under it, and the tile sits at the row's left with the
    /// gutter. The band is drawn through the view's own painter to advance the
    /// layout, so only the action row's height is re-derived here.
    fn play_tile_rect_for(ctx: &Context, title: &str) -> Rect {
        let tokens = Tokens::DARK;
        let mut found = None;
        run(ctx, input(Vec::new()), |ui| {
            ui.add_space(space::S3);
            egui::ScrollArea::vertical().show(ui, |ui| {
                let width = ui.available_width();
                let top = ui.cursor().min;
                let band = hero::Band {
                    eyebrow: "Playlist",
                    title,
                    meta: Vec::new(),
                    description: "",
                    playable: true,
                    shuffle: true,
                    cover: Cover::Missing,
                };
                let _ = hero::show(ui, &band, &tokens);
                let bottom = ui.cursor().min.y;
                let height = PLAY + 2.0 * space::S4;
                let row =
                    Rect::from_min_size(Pos2::new(top.x, bottom - height), vec2(width, height));
                let centre = row.center().y;
                found = Some(Rect::from_min_size(
                    Pos2::new(row.left() + space::GUTTER, centre - PLAY / 2.0),
                    Vec2::splat(PLAY),
                ));
            });
        });
        found.expect("the hero and its action row lay out")
    }

    /// Clicks `centre` `times` times, one event per pass, and returns the command
    /// the view reported while they happened. `screen` is the probe window, so a
    /// test that needs the band and two whole rows can ask for a taller one.
    fn clicks(
        screen: Vec2,
        ctx: &Context,
        centre: Pos2,
        times: usize,
        library: &mut Library,
        transport: &mut Transport,
        queue: &mut Queue,
    ) -> Option<Command> {
        let tokens = Tokens::DARK;
        let mut command = None;
        // Warm up so hit-testing has a previous pass to read.
        run(ctx, sized_input(screen, Vec::new()), |ui| {
            let _ = tracks_view(
                ui,
                Table::Full,
                library,
                transport,
                queue,
                &mut Art::default(),
                Instant::now(),
                NOW_MS,
                &tokens,
            );
        });
        for _ in 0..times {
            for event in crate::view::test_support::click_events(centre) {
                let command = &mut command;
                let library = &mut *library;
                let transport = &mut *transport;
                let queue = &mut *queue;
                run(ctx, sized_input(screen, vec![event]), |ui| {
                    if let Some(next) = tracks_view(
                        ui,
                        Table::Full,
                        library,
                        transport,
                        queue,
                        &mut Art::default(),
                        Instant::now(),
                        NOW_MS,
                        &tokens,
                    ) {
                        *command = Some(next);
                    }
                });
            }
        }
        command
    }

    /// One click on `centre`, and the command the view returned while it happened.
    fn click(
        ctx: &Context,
        centre: Pos2,
        library: &mut Library,
        transport: &mut Transport,
        queue: &mut Queue,
    ) -> Option<Command> {
        clicks(PROBE, ctx, centre, 1, library, transport, queue)
    }

    /// The x the header paints the upper-cased `label` at, read back from the
    /// frame's text shapes. The header paints through the painter, so this is
    /// where a heading actually landed rather than a re-solve of its columns.
    fn heading_left(
        ctx: &Context,
        label: &str,
        library: &mut Library,
        transport: &mut Transport,
        queue: &mut Queue,
    ) -> f32 {
        let tokens = Tokens::DARK;
        let mut output = ctx.run_ui(input(Vec::new()), |ui| {
            let _ = tracks_view(
                ui,
                Table::Full,
                library,
                transport,
                queue,
                &mut Art::default(),
                Instant::now(),
                NOW_MS,
                &tokens,
            );
        });
        output.textures_delta.clear();
        let label = label.to_uppercase();
        output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Text(text) if text.galley.text() == label => Some(text.pos.x),
                _ => None,
            })
            .unwrap_or_else(|| panic!("the header painted {label:?}"))
    }

    /// One double click on `centre`, and the command the view returned while it
    /// happened.
    fn double_click(
        ctx: &Context,
        centre: Pos2,
        library: &mut Library,
        transport: &mut Transport,
        queue: &mut Queue,
    ) -> Option<Command> {
        clicks(PROBE, ctx, centre, 2, library, transport, queue)
    }

    #[test]
    fn a_double_click_on_a_row_starts_that_track_through_the_queue() {
        let ctx = context();
        let mut library = library_with(vec![track("a", "t0"), track("b", "t1")]);
        let mut transport = Transport::default();
        let mut queue = Queue::default();

        // The click is where the view draws row 0, not a probe's geometry.
        let centre = row_rect_for(&ctx, 2, 0).center();
        let command = double_click(&ctx, centre, &mut library, &mut transport, &mut queue);

        assert_eq!(
            command,
            Some(Command::Play {
                uri: "a".to_owned()
            })
        );
        assert!(
            queue.can_next(),
            "the double-clicked row must start the queue, not only the transport"
        );
        assert_eq!(
            transport.now_playing.as_ref().map(|now| now.uri.as_str()),
            Some("a")
        );
    }

    #[test]
    fn a_single_track_list_starts_its_only_track() {
        let ctx = context();
        let mut library = library_with(vec![track("a", "t0")]);
        let mut transport = Transport::default();
        let mut queue = Queue::default();

        let centre = row_rect_for(&ctx, 1, 0).center();
        let command = double_click(&ctx, centre, &mut library, &mut transport, &mut queue);

        assert_eq!(
            command,
            Some(Command::Play {
                uri: "a".to_owned()
            })
        );
        assert!(
            !queue.can_next(),
            "a one-track queue has nothing after its only entry"
        );
        assert_eq!(
            transport.now_playing.as_ref().map(|now| now.uri.as_str()),
            Some("a")
        );
    }

    #[test]
    fn a_row_with_no_album_added_or_duration_still_lays_out() {
        let tokens = Tokens::DARK;
        let ctx = context();
        // An album or an artist context carries neither an album name nor a
        // playlist's added date, and a track may have no length. Solving its
        // columns and double-clicking must still lay the row out and activate it.
        let columns = table_layout::columns(900.0, Table::Full.columns());
        let rect = table_layout::row_rect(
            Rect::from_min_size(Pos2::ZERO, vec2(900.0, extent::ROW_H)),
            0,
            extent::ROW_H,
        );
        let row = Row {
            uri: "a",
            number: 1,
            title: "t0",
            title_cell: track_row::TitleCell::ArtThenName,
            artist: "",
            artist_uri: None,
            album: None,
            album_uri: None,
            added: None,
            duration: None,
            state: State::Resting,
            cover: Cover::Missing,
        };

        run(&ctx, input(Vec::new()), |ui| {
            let _ = track_row::show(ui, rect, columns, &row, &tokens);
        });
        let mut clicked = false;
        for _ in 0..2 {
            for event in [
                Event::PointerMoved(rect.center()),
                crate::view::test_support::button(rect.center(), true),
                crate::view::test_support::button(rect.center(), false),
            ] {
                run(&ctx, input(vec![event]), |ui| {
                    clicked |= track_row::show(ui, rect, columns, &row, &tokens).is_some();
                });
            }
        }
        assert!(
            clicked,
            "a row with no album, added or duration must still lay out and activate"
        );
    }

    #[test]
    fn a_row_given_an_album_and_a_duration_draws_them() {
        let tokens = Tokens::DARK;
        let ctx = context();
        let columns = table_layout::columns(900.0, Table::Full.columns());
        let rect = table_layout::row_rect(
            Rect::from_min_size(Pos2::ZERO, vec2(900.0, extent::ROW_H)),
            0,
            extent::ROW_H,
        );
        let row = Row {
            uri: "a",
            number: 1,
            title: "t0",
            title_cell: track_row::TitleCell::ArtThenName,
            artist: "Band",
            artist_uri: None,
            album: Some("Record"),
            album_uri: None,
            added: None,
            duration: Some("3:42"),
            state: State::Resting,
            cover: Cover::Missing,
        };

        let mut output = ctx.run_ui(input(Vec::new()), |ui| {
            let _ = track_row::show(ui, rect, columns, &row, &tokens);
        });
        output.textures_delta.clear();
        let texts = drawn_texts(&output);
        assert!(
            texts.contains(&"Record"),
            "the album column must draw its label: {texts:?}"
        );
        assert!(
            texts.contains(&"3:42"),
            "the duration column must draw its label: {texts:?}"
        );
    }

    #[test]
    fn the_view_hands_a_tracks_album_and_length_to_its_row() {
        let tokens = Tokens::DARK;
        let ctx = context();
        let mut library = library_with(vec![TrackEntry {
            uri: "a".to_owned(),
            title: "t0".to_owned(),
            artists: "Band".to_owned(),
            album: Some("Record".to_owned()),
            album_uri: None,
            artist_uris: Vec::new(),
            added_at: None,
            duration_ms: Some(222_000),
            named: true,
            cover: None,
        }]);
        let mut transport = Transport::default();
        let mut queue = Queue::default();

        let mut output = ctx.run_ui(input(Vec::new()), |ui| {
            let _ = tracks_view(
                ui,
                Table::Full,
                &mut library,
                &mut transport,
                &mut queue,
                &mut Art::default(),
                Instant::now(),
                NOW_MS,
                &tokens,
            );
        });
        output.textures_delta.clear();
        let texts = drawn_texts(&output);
        assert!(
            texts.contains(&"Record"),
            "the row must be handed the track's album: {texts:?}"
        );
        assert!(
            texts.contains(&"3:42"),
            "the row must be handed the track's length as m:ss: {texts:?}"
        );
    }

    #[test]
    fn a_playlist_row_draws_its_added_label() {
        let ctx = context();
        let mut library = library_with(vec![dated("a", "t0", Some(date(NOW_MS - 4 * DAY)))]);

        let texts = painted_lines(&ctx, Table::Full, &mut library);
        assert!(
            texts.contains(&"ADDED".to_owned()),
            "the header must keep the added column: {texts:?}"
        );
        assert!(
            texts.contains(&"4 days ago".to_owned()),
            "the row must show when the track was added: {texts:?}"
        );
    }

    #[test]
    fn a_playlist_without_dates_keeps_its_rows_and_header() {
        // The column is a playlist's, not a date's: a playlist whose dates are
        // all absent still heads it and still draws every row.
        let ctx = context();
        let mut library = library_with(vec![dated("a", "t0", None), dated("b", "t1", None)]);

        let texts = painted_lines(&ctx, Table::Full, &mut library);
        assert!(
            texts.contains(&"ADDED".to_owned()),
            "the header must stay with no dates loaded: {texts:?}"
        );
        assert!(texts.contains(&"t0".to_owned()), "row 0 must draw");
        assert!(texts.contains(&"t1".to_owned()), "row 1 must draw");
        assert!(
            !texts
                .iter()
                .any(|text| text == "Today" || text.ends_with(" ago")),
            "an absent date must paint no cell: {texts:?}"
        );
    }

    #[test]
    fn a_future_date_draws_no_cell() {
        let ctx = context();
        let mut library = library_with(vec![dated("a", "t0", Some(date(NOW_MS + DAY)))]);

        let texts = painted_lines(&ctx, Table::Full, &mut library);
        assert!(texts.contains(&"t0".to_owned()), "the row must still draw");
        assert!(
            !texts
                .iter()
                .any(|text| text == "Today" || text.ends_with(" ago")),
            "a date after now is not an age: {texts:?}"
        );
    }

    #[test]
    fn a_context_that_is_not_a_playlist_draws_no_added_column() {
        let ctx = context();
        // Long enough to elide at the probe width in both contexts, so the
        // visible run is a measure of the column the title was given.
        let title = "The Extremely Long-Named Orchestra of the Northern Coast and Its Friends \
Plays the Music of the Twentieth Century for an Audience of Patient Listeners";
        let rows = || vec![dated("a", title, Some(date(NOW_MS - 2 * DAY)))];

        let mut playlist = library_selected("spotify:playlist:a", rows());
        let playlist_texts = painted_runs(&ctx, Table::Full, &mut playlist);

        let mut album = library_selected("spotify:album:a", rows());
        let album_texts = painted_runs(&ctx, Table::Full, &mut album);

        assert!(
            playlist_texts.contains(&"ADDED".to_owned()),
            "a playlist heads the date column: {playlist_texts:?}"
        );
        assert!(
            !album_texts.contains(&"ADDED".to_owned()),
            "an album must not head a date column: {album_texts:?}"
        );
        assert!(
            !album_texts
                .iter()
                .any(|text| text == "Today" || text.ends_with(" ago")),
            "an album's rows must show no date even when they carry one: {album_texts:?}"
        );

        // The dropped column's width goes to the title: the same title shows
        // more of itself in the album than in the playlist.
        let painted_title = |texts: &[String]| {
            texts
                .iter()
                .find(|text| text.starts_with("The Extremely"))
                .cloned()
                .expect("the row paints its title")
        };
        let playlist_title = painted_title(&playlist_texts);
        let album_title = painted_title(&album_texts);
        assert!(
            playlist_title.ends_with('…'),
            "the playlist's title must be squeezed: {playlist_title:?}"
        );
        assert!(
            album_title.chars().count() > playlist_title.chars().count(),
            "the album's title must take the freed width: {album_title:?} vs {playlist_title:?}"
        );
    }

    #[test]
    fn the_same_track_twice_shows_each_rows_own_date() {
        let ctx = context();
        let mut library = library_with(vec![
            dated("a", "t0", Some(date(NOW_MS - 4 * DAY))),
            dated("a", "t0", Some(date(NOW_MS - 14 * DAY))),
        ]);

        let texts = painted_lines(&ctx, Table::Full, &mut library);
        assert!(
            texts.contains(&"4 days ago".to_owned()),
            "the first row must show its own date: {texts:?}"
        );
        assert!(
            texts.contains(&"2 weeks ago".to_owned()),
            "the second row must show its own date: {texts:?}"
        );
    }

    #[test]
    fn the_second_of_two_identical_entries_starts_the_second() {
        // Two entries holding one uri: keyed by the uri alone, both rows would
        // share one egui id, so one click would drive both and the higher index
        // would win. The command's uri is `a` whichever row is taken, so the
        // queue's position is the only thing that tells the two apart: starting
        // the second leaves nothing after it, while the first would leave entry
        // two. `Queue::can_previous` reports only that the queue is non-empty, so
        // it cannot sharpen this; `can_next` is the sharp assertion.
        //
        // Index 1 also exercises `row_rect`'s pitch: the other tests only ever
        // click index 0.
        let ctx = context();
        let mut library = library_with(vec![track("a", "t0"), track("a", "t0")]);
        let mut transport = Transport::default();
        let mut queue = Queue::default();

        let first = row_rect_for(&ctx, 2, 0);
        let second = row_rect_for(&ctx, 2, 1);
        assert!(
            (second.top() - first.bottom()).abs() < 0.01,
            "the second row must start where the first ends: {first:?} then {second:?}"
        );
        let command = double_click(
            &ctx,
            second.center(),
            &mut library,
            &mut transport,
            &mut queue,
        );

        assert_eq!(
            command,
            Some(Command::Play {
                uri: "a".to_owned()
            })
        );
        assert!(
            !queue.can_next(),
            "the second of two identical entries was started, not the first"
        );
    }

    #[test]
    fn a_double_click_on_the_first_of_two_identical_entries_starts_the_first() {
        // The mirror of the test above. When the two rows shared one id, the
        // higher index overwrote the lower, so clicking the first row started
        // the second; this pins that the first is the one that answers, and it
        // fails against the uri-only key where the other test alone would not.
        let ctx = context();
        let mut library = library_with(vec![track("a", "t0"), track("a", "t0")]);
        let mut transport = Transport::default();
        let mut queue = Queue::default();

        let first = row_rect_for(&ctx, 2, 0).center();
        let command = double_click(&ctx, first, &mut library, &mut transport, &mut queue);

        assert_eq!(
            command,
            Some(Command::Play {
                uri: "a".to_owned()
            })
        );
        assert!(
            queue.can_next(),
            "the first of two identical entries was started, not the second"
        );
    }

    #[test]
    fn a_row_outside_the_viewport_takes_no_click() {
        let ctx = context();
        // Sixty rows at `extent::ROW_H` each put row forty far below the 400-point
        // probe window, so `ui.clip_rect()` cannot contain it and the view skips
        // laying it out. A shorter list would leave the clicked row on screen and
        // the test would prove nothing.
        let tracks: Vec<TrackEntry> = (0..60)
            .map(|i| track(&format!("t{i}"), &format!("t{i}")))
            .collect();
        let mut library = library_with(tracks);
        let mut transport = Transport::default();
        let mut queue = Queue::default();

        let offscreen = row_rect_for(&ctx, 60, 40);
        assert!(
            offscreen.top() > 400.0,
            "the culled row must sit below the probe's viewport: {offscreen:?}"
        );
        let command = double_click(
            &ctx,
            offscreen.center(),
            &mut library,
            &mut transport,
            &mut queue,
        );

        assert_eq!(command, None, "a culled row must not activate");
    }

    #[test]
    fn the_header_and_the_rows_share_a_column_origin() {
        // The header and the rows are solved from one width, so a heading's left
        // edge and its column's left edge are the same screen x. A padding change
        // on one side alone would slide them apart; this reads the header's own
        // painted position rather than re-solving it, so it can tell.
        let ctx = context();
        let mut library = library_with(vec![track("a", "t0")]);
        let mut transport = Transport::default();
        let mut queue = Queue::default();

        let row = row_rect_for(&ctx, 1, 0);
        let columns = table_layout::columns(row.width(), Table::Full.columns()).at(row.min);
        for (label, column) in [
            ("Title", columns.title),
            ("Artist", columns.artist),
            ("Album", columns.album),
            ("Added", columns.added),
        ] {
            let painted = heading_left(&ctx, label, &mut library, &mut transport, &mut queue);
            assert!(
                (painted - column.left()).abs() < 0.01,
                "the {label} heading is painted at {painted}, its column starts at {}",
                column.left()
            );
        }
    }

    /// The table sits one gutter inside the pane, so its first heading lines up
    /// under the band's own padding rather than rubbing the panel's edge.
    #[test]
    fn the_heading_sits_one_gutter_inside_the_pane() {
        let ctx = context();
        let mut library = library_with(vec![track("a", "t0")]);
        let mut transport = Transport::default();
        let mut queue = Queue::default();

        let painted = heading_left(&ctx, "Title", &mut library, &mut transport, &mut queue);
        let columns = table_layout::columns(PROBE.x - 2.0 * space::GUTTER, Table::Full.columns());
        assert!(
            (painted - (space::GUTTER + columns.title.left())).abs() < 0.01,
            "the title heading is painted at {painted}, one gutter plus its column is {}",
            space::GUTTER + columns.title.left()
        );
    }

    #[test]
    fn below_1000_the_playlist_heads_and_draws_the_artist_but_not_the_album() {
        // The reference drops the album at the same width it reduces the
        // playbar, from the header and from every row, and keeps the artist.
        let ctx = context();
        let mut library = library_with(vec![TrackEntry {
            uri: "a".to_owned(),
            title: "t0".to_owned(),
            artists: "Band".to_owned(),
            album: Some("Record".to_owned()),
            album_uri: None,
            artist_uris: Vec::new(),
            added_at: None,
            duration_ms: Some(222_000),
            named: true,
            cover: None,
        }]);
        let mut transport = Transport::default();
        let mut queue = Queue::default();

        let mut output = ctx.run_ui(input(Vec::new()), |ui| {
            let _ = tracks_view(
                ui,
                Table::Reduced,
                &mut library,
                &mut transport,
                &mut queue,
                &mut Art::default(),
                Instant::now(),
                NOW_MS,
                &Tokens::DARK,
            );
        });
        output.textures_delta.clear();
        let texts = drawn_texts(&output);
        assert!(
            texts.contains(&"ARTIST"),
            "the header must keep the artist heading: {texts:?}"
        );
        assert!(
            texts.contains(&"Band"),
            "the row must keep the artist: {texts:?}"
        );
        assert!(
            !texts.contains(&"ALBUM"),
            "the header must drop the album heading: {texts:?}"
        );
        assert!(
            !texts.contains(&"Record"),
            "the row must drop the album: {texts:?}"
        );
    }

    #[test]
    fn a_long_artist_elides_inside_its_column() {
        let tokens = Tokens::DARK;
        let ctx = context();
        let width = 700.0;
        let columns = table_layout::columns(width, Table::Reduced.columns());
        let rect = Rect::from_min_size(Pos2::ZERO, vec2(width, extent::ROW_H));
        let artist = "The Extremely Long-Named Orchestra of the Northern Coast and Its Friends";
        let row = Row {
            uri: "a",
            number: 1,
            title: "t0",
            title_cell: track_row::TitleCell::ArtThenName,
            artist,
            artist_uri: None,
            album: Some("Record"),
            album_uri: None,
            added: None,
            duration: Some("3:42"),
            state: State::Resting,
            cover: Cover::Missing,
        };

        let mut output = ctx.run_ui(input(Vec::new()), |ui| {
            let _ = track_row::show(ui, rect, columns, &row, &tokens);
        });
        output.textures_delta.clear();
        let galley = output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Text(text) if text.galley.text() == artist => {
                    Some(text.galley.clone())
                }
                _ => None,
            })
            .expect("the artist column paints its label");

        assert_eq!(galley.rows.len(), 1, "the artist must not wrap");
        assert!(galley.elided, "the artist must elide");
        assert!(
            galley.rows[0].row.text().ends_with('…'),
            "the elided artist must end in an ellipsis: {:?}",
            galley.rows[0].row.text()
        );
        assert!(
            galley.size().x <= columns.artist.width() + 0.01,
            "the artist is {} wide in a {} column",
            galley.size().x,
            columns.artist.width()
        );
    }

    #[test]
    fn an_empty_track_list_draws_without_panicking() {
        let tokens = Tokens::DARK;
        let ctx = context();
        let mut library = library_with(Vec::new());
        let mut transport = Transport::default();
        let mut queue = Queue::default();

        let mut command = None;
        run(&ctx, input(Vec::new()), |ui| {
            command = tracks_view(
                ui,
                Table::Full,
                &mut library,
                &mut transport,
                &mut queue,
                &mut Art::default(),
                Instant::now(),
                NOW_MS,
                &tokens,
            );
        });

        assert_eq!(command, None, "an empty list has nothing to activate");
    }

    #[test]
    fn the_hero_heads_a_named_playlist_and_not_an_unnamed_one() {
        let tokens = Tokens::DARK;
        let ctx = context();
        let mut transport = Transport::default();
        let mut queue = Queue::default();

        // A selected playlist whose name the library knows: the band is drawn.
        let mut named = library_named(vec![track("a", "t0")], "Evening");
        let mut output = ctx.run_ui(input(Vec::new()), |ui| {
            let _ = tracks_view(
                ui,
                Table::Full,
                &mut named,
                &mut transport,
                &mut queue,
                &mut Art::default(),
                Instant::now(),
                NOW_MS,
                &tokens,
            );
        });
        output.textures_delta.clear();
        let texts = drawn_texts(&output);
        assert!(
            texts.contains(&"PLAYLIST"),
            "the band's eyebrow must be drawn over a named playlist: {texts:?}"
        );
        assert!(
            texts.contains(&"Evening"),
            "the band must name the playlist: {texts:?}"
        );

        // The same selection with nothing in the library to name it: no band,
        // only the rows.
        let mut unnamed = library_with(vec![track("a", "t0")]);
        let mut output = ctx.run_ui(input(Vec::new()), |ui| {
            let _ = tracks_view(
                ui,
                Table::Full,
                &mut unnamed,
                &mut transport,
                &mut queue,
                &mut Art::default(),
                Instant::now(),
                NOW_MS,
                &tokens,
            );
        });
        output.textures_delta.clear();
        let texts = drawn_texts(&output);
        assert!(
            !texts.contains(&"PLAYLIST"),
            "no name means no band, only the rows: {texts:?}"
        );
    }

    #[test]
    fn the_hero_play_control_starts_the_first_track_through_the_queue() {
        let ctx = context();
        let mut library = library_named(vec![track("a", "t0"), track("b", "t1")], "Evening");
        let mut transport = Transport::default();
        let mut queue = Queue::default();

        // The click is where the view draws the control, not a probe's geometry.
        let play = play_tile_rect_for(&ctx, "Evening").center();
        let command = click(&ctx, play, &mut library, &mut transport, &mut queue);

        assert_eq!(
            command,
            Some(Command::Play {
                uri: "a".to_owned()
            })
        );
        assert!(
            queue.can_next(),
            "the hero's play must start the queue, not only the transport"
        );
        assert_eq!(
            transport.now_playing.as_ref().map(|now| now.uri.as_str()),
            Some("a")
        );
    }

    #[test]
    fn a_double_click_on_the_second_row_plays_it_while_the_band_is_drawn() {
        let ctx = context();
        let mut library = library_named(vec![track("a", "t0"), track("b", "t1")], "Evening");
        let mut transport = Transport::default();
        let mut queue = Queue::default();

        // The band pushes the rows down, so the only solve that sees its offset
        // is the one below it. Clicking the second row must start the second
        // track: if the offset were dropped, the same point would land on the
        // first.
        let second = row_rect_below_hero(&ctx, TALL, 2, 1, "Evening").center();
        let command = clicks(
            TALL,
            &ctx,
            second,
            2,
            &mut library,
            &mut transport,
            &mut queue,
        );

        assert_eq!(
            command,
            Some(Command::Play {
                uri: "b".to_owned()
            })
        );
        assert!(
            !queue.can_next(),
            "the second of two rows was started, not the first"
        );
        assert_eq!(
            transport.now_playing.as_ref().map(|now| now.uri.as_str()),
            Some("b")
        );
    }

    #[test]
    fn idle_view_reads_the_playlists_load() {
        // A list nobody has asked for yet and a list in flight are one state:
        // neither has anything to draw.
        assert_eq!(idle_view(&Library::default()), IdleView::Loading);

        let mut loading = Library::default();
        loading.apply(&Update::SignedIn);
        assert_eq!(idle_view(&loading), IdleView::Loading);
    }

    #[test]
    fn idle_view_failed_playlists_win_for_every_chip() {
        for kind in library_filter::ROW {
            let mut library = library_idle(Vec::new());
            library.root_filter.choose(kind);
            library.apply(&Update::PlaylistFailed(Failure::Load));
            assert_eq!(
                idle_view(&library),
                IdleView::Failed(Failure::Load),
                "{kind:?} must still report the page's failure"
            );
        }
    }

    #[test]
    fn idle_view_all_shows_playlists_and_shows() {
        let mut library = library_idle(vec![playlist("a", "A")]);
        library.apply(&Update::FollowedShows(vec![show("s", "S")]));
        let IdleView::Grid { items, note } = idle_view(&library) else {
            panic!("a ready root is a grid: {:?}", idle_view(&library));
        };
        assert_eq!(items.len(), 2, "playlists first, then shows");
        assert!(matches!(items[0], Item::Playlist(entry) if entry.uri == "a"));
        assert!(matches!(&items[1], Item::Show(entry) if entry.uri == "s"));
        assert_eq!(note, LoadNote::None);
    }

    #[test]
    fn idle_view_all_with_no_playlists_waits_on_the_followed_list() {
        let mut library = library_idle(Vec::new());
        let _ = library.followed_request();
        let IdleView::Grid { items, note } = idle_view(&library) else {
            panic!("the shows are still coming: {:?}", idle_view(&library));
        };
        assert!(items.is_empty());
        assert_eq!(note, LoadNote::Loading(FollowedRead::Shows));

        library.apply(&Update::FollowedShowsFailed(Failure::Followed));
        let IdleView::Grid { note, .. } = idle_view(&library) else {
            panic!("a failed list is its own note: {:?}", idle_view(&library));
        };
        assert_eq!(
            note,
            LoadNote::Failed(FollowedRead::Shows, Failure::Followed)
        );

        let mut settled = library_idle(Vec::new());
        settled.apply(&Update::FollowedShows(Vec::new()));
        assert_eq!(idle_view(&settled), IdleView::Empty(RootKind::All));
    }

    #[test]
    fn idle_view_playlists_hides_shows_and_their_loading() {
        let mut library = library_idle(vec![playlist("a", "A")]);
        library.root_filter.choose(RootKind::Playlists);
        // The followed list is in flight; the playlists chip does not wait on it.
        let _ = library.followed_request();
        let IdleView::Grid { items, note } = idle_view(&library) else {
            panic!("the playlists chip is a grid: {:?}", idle_view(&library));
        };
        assert_eq!(items.len(), 1);
        assert_eq!(note, LoadNote::None);
    }

    #[test]
    fn idle_view_podcasts_loading_failed_and_empty() {
        let mut library = library_idle(vec![playlist("a", "A")]);
        library.root_filter.choose(RootKind::Podcasts);

        let _ = library.followed_request();
        let IdleView::Grid { items, note } = idle_view(&library) else {
            panic!(
                "a loading followed list is a grid: {:?}",
                idle_view(&library)
            );
        };
        assert!(
            items.is_empty(),
            "the playlists chip is not the podcasts one"
        );
        assert_eq!(note, LoadNote::Loading(FollowedRead::Shows));

        library.apply(&Update::FollowedShowsFailed(Failure::Load));
        let IdleView::Grid { items, note } = idle_view(&library) else {
            panic!(
                "a failed followed list is a grid: {:?}",
                idle_view(&library)
            );
        };
        assert!(items.is_empty());
        assert_eq!(note, LoadNote::Failed(FollowedRead::Shows, Failure::Load));

        let mut empty = library_idle(vec![playlist("a", "A")]);
        empty.root_filter.choose(RootKind::Podcasts);
        empty.apply(&Update::FollowedShows(Vec::new()));
        assert_eq!(
            idle_view(&empty),
            IdleView::Empty(RootKind::Podcasts),
            "nothing to show is the chip's own empty line"
        );
    }

    #[test]
    fn a_click_on_a_card_loads_its_playlist() {
        let ctx = context();
        let mut library = library_idle(vec![playlist("a", "A"), playlist("b", "B")]);
        let mut transport = Transport::default();
        let mut queue = Queue::default();

        // The click is where the view draws the first card, not a probe's
        // geometry.
        let centre = card_rect_for(&ctx, 0).center();
        let command = click(&ctx, centre, &mut library, &mut transport, &mut queue);

        assert_eq!(
            command,
            Some(Command::LoadTracks {
                playlist: "a".to_owned()
            })
        );
        assert_eq!(library.selected(), Some("a"));
        assert_eq!(library.tracks, Load::Loading);
    }

    #[test]
    fn a_card_click_asks_for_tracks_once() {
        // The whole click sequence twice: the first click selects the playlist
        // and starts its load, and the second lands on the progress the view is
        // showing instead of a card. Counting the commands the view reports is
        // the assertion the single command in `a_click_on_a_card_loads_its_
        // playlist` cannot make.
        let ctx = context();
        let tokens = Tokens::DARK;
        let mut library = library_idle(vec![playlist("a", "A")]);
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let centre = card_rect_for(&ctx, 0).center();

        run(&ctx, input(Vec::new()), |ui| {
            let _ = tracks_view(
                ui,
                Table::Full,
                &mut library,
                &mut transport,
                &mut queue,
                &mut Art::default(),
                Instant::now(),
                NOW_MS,
                &tokens,
            );
        });

        let mut loads = 0;
        for _ in 0..2 {
            for event in crate::view::test_support::click_events(centre) {
                run(&ctx, input(vec![event]), |ui| {
                    if let Some(Command::LoadTracks { .. }) = tracks_view(
                        ui,
                        Table::Full,
                        &mut library,
                        &mut transport,
                        &mut queue,
                        &mut Art::default(),
                        Instant::now(),
                        NOW_MS,
                        &tokens,
                    ) {
                        loads += 1;
                    }
                });
            }
        }

        assert_eq!(loads, 1, "two clicks on the same card must load it once");
        assert_eq!(library.selected(), Some("a"));
    }

    #[test]
    fn the_tile_height_the_grid_solves_contains_the_last_line_the_tile_paints() {
        let ctx = context();
        let tokens = Tokens::DARK;
        // A realistic tile: the grid's own solve at the probe's width, so the
        // rect's height is `tile_w + media::extra_height`, the number the grid is
        // given.
        let rect = card_rect_for(&ctx, 0);
        let media = crate::view::media::Media {
            uri: "a",
            name: "Evening",
            kind: "Playlist",
            sub: "Playlist · Vela Moreno",
            size: "62 songs",
            round: false,
            cover: Cover::Missing,
        };
        let mut output = ctx.run_ui(input(Vec::new()), |ui| {
            let _ = crate::view::media::tile(ui, rect, &media, &tokens);
        });
        output.textures_delta.clear();

        let last = output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Text(text) => Some(text.pos.y + text.galley.size().y),
                _ => None,
            })
            .max_by(f32::total_cmp)
            .expect("the card paints a name line and a kind line");

        // The grid's card must contain the painter's lowest line, and it must
        // leave no more than the layout's own `S2` below it. The first half
        // catches an `extra_height` short enough to spill the line out of the
        // card; the second catches the 16 points of slack left when the padding
        // was counted against the art as well as against the card.
        assert!(
            last <= rect.bottom(),
            "the last painted line ends at {last}, past the card's {}",
            rect.bottom()
        );
        assert!(
            rect.bottom() - last <= space::S2 + 0.5,
            "the card leaves {} below its last line, more than the {} it lays out",
            rect.bottom() - last,
            space::S2
        );
    }

    #[test]
    fn to_playlists_returns_to_the_grid() {
        let mut library = library_idle(vec![playlist("a", "A")]);
        library.select("a");
        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: vec![track("t", "t")],
            cover: None,
        });
        library.to_playlists();

        let IdleView::Grid { items, .. } = idle_view(&library) else {
            panic!(
                "to_playlists must return to the grid: {:?}",
                idle_view(&library)
            );
        };
        assert_eq!(items.len(), 1);
        assert!(matches!(&items[0], Item::Playlist(entry) if entry.uri == "a"));
        assert!(
            matches!(library.tracks, Load::Ready(_)),
            "the grid draws while the tracks stay ready"
        );
    }

    #[test]
    fn the_root_draws_the_grid_while_the_tracks_stay_ready() {
        let ctx = context();
        let mut library = library_idle(vec![playlist("a", "A")]);
        library.select("a");
        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: vec![track("t0", "t0")],
            cover: None,
        });
        library.to_playlists();

        assert_eq!(library.page(), None);
        assert!(
            matches!(library.tracks, Load::Ready(_)),
            "the tracks stay ready behind the root"
        );

        let texts = painted_lines(&ctx, Table::Full, &mut library);

        assert!(
            texts.contains(&"A".to_owned()),
            "the grid must draw its card: {texts:?}"
        );
        assert!(
            !texts.contains(&"t0".to_owned()),
            "the ready tracklist must not draw at the root: {texts:?}"
        );
    }

    #[test]
    fn a_click_on_the_album_cell_opens_the_album() {
        let ctx = context();
        let mut library = library_with(vec![linkable("a")]);
        let mut transport = Transport::default();
        let mut queue = Queue::default();

        let cell = cell_rect_for(&ctx, 1, 0, |columns| columns.album);
        let command = click(
            &ctx,
            cell.center(),
            &mut library,
            &mut transport,
            &mut queue,
        );

        assert_eq!(
            library.page(),
            Some(&Page::Album("spotify:album:rec".to_owned())),
            "a single click on the album cell must open the album"
        );
        assert_eq!(
            command,
            Some(Command::LoadAlbum {
                album: "spotify:album:rec".to_owned()
            }),
            "opening an album asks the worker for it"
        );
        assert!(!queue.can_next(), "opening a page must not start the queue");
        assert_eq!(transport.now_playing, None, "opening a page must not play");
    }

    #[test]
    fn a_click_in_the_album_cells_padding_opens_the_album() {
        // The brief's link is the cell, not the painted text: a click past the end
        // of "Record" is still a click on the album.
        let ctx = context();
        let mut library = library_with(vec![linkable("a")]);
        let mut transport = Transport::default();
        let mut queue = Queue::default();

        let cell = cell_rect_for(&ctx, 1, 0, |columns| columns.album);
        let padding = Pos2::new(cell.right() - space::S3, cell.center().y);
        let command = click(&ctx, padding, &mut library, &mut transport, &mut queue);

        assert_eq!(
            library.page(),
            Some(&Page::Album("spotify:album:rec".to_owned())),
            "the whole album cell must take the click, padding included"
        );
        assert_eq!(
            command,
            Some(Command::LoadAlbum {
                album: "spotify:album:rec".to_owned()
            })
        );
        assert!(!queue.can_next());
    }

    #[test]
    fn a_click_on_the_artist_cell_opens_the_first_artist() {
        let ctx = context();
        let mut library = library_with(vec![linkable("a")]);
        let mut transport = Transport::default();
        let mut queue = Queue::default();

        let cell = cell_rect_for(&ctx, 1, 0, |columns| columns.artist);
        let command = click(
            &ctx,
            cell.center(),
            &mut library,
            &mut transport,
            &mut queue,
        );

        assert_eq!(
            library.page(),
            Some(&Page::Artist("spotify:artist:band".to_owned())),
            "a single click on the artist cell must open the first artist"
        );
        assert_eq!(
            command,
            Some(Command::LoadArtist {
                artist: "spotify:artist:band".to_owned()
            }),
            "opening an artist asks the worker for it"
        );
        assert!(!queue.can_next());
        assert_eq!(transport.now_playing, None);
    }

    #[test]
    fn a_click_in_the_artist_cells_padding_opens_the_first_artist() {
        let ctx = context();
        let mut library = library_with(vec![linkable("a")]);
        let mut transport = Transport::default();
        let mut queue = Queue::default();

        let cell = cell_rect_for(&ctx, 1, 0, |columns| columns.artist);
        let padding = Pos2::new(cell.right() - space::S3, cell.center().y);
        let command = click(&ctx, padding, &mut library, &mut transport, &mut queue);

        assert_eq!(
            library.page(),
            Some(&Page::Artist("spotify:artist:band".to_owned())),
            "the whole artist cell must take the click, padding included"
        );
        assert_eq!(
            command,
            Some(Command::LoadArtist {
                artist: "spotify:artist:band".to_owned()
            })
        );
        assert!(!queue.can_next());
    }

    #[test]
    fn a_link_cell_points_the_hand_and_brightens_its_label() {
        // A screenshot shows the colour but not the cursor the window reports, so
        // both of the hover's effects are pinned here.
        let ctx = context();
        let tokens = Tokens::DARK;
        let mut library = library_with(vec![linkable("a")]);
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let cell = cell_rect_for(&ctx, 1, 0, |columns| columns.album);

        let mut frame = |events: Vec<Event>| {
            let mut output = run(&ctx, input(events), |ui| {
                let _ = tracks_view(
                    ui,
                    Table::Full,
                    &mut library,
                    &mut transport,
                    &mut queue,
                    &mut Art::default(),
                    Instant::now(),
                    NOW_MS,
                    &tokens,
                );
            });
            output.textures_delta.clear();
            let color = output
                .shapes
                .iter()
                .find_map(|clipped| match &clipped.shape {
                    egui::epaint::Shape::Text(text) if text.galley.text() == "Record" => {
                        Some(text.galley.job.sections[0].format.color)
                    }
                    _ => None,
                })
                .expect("the album label is painted");
            (output.platform_output.cursor_icon, color)
        };

        // At rest the label is muted and the cursor is the window's own.
        let (rest_cursor, rest_color) = frame(Vec::new());
        assert_eq!(rest_color, tokens.palette.text_muted);
        assert_ne!(rest_cursor, egui::CursorIcon::PointingHand);

        // Resting on the cell points the hand and brightens the label to the
        // body colour, over the reveal's own time.
        for _ in 0..12 {
            frame(vec![Event::PointerMoved(cell.center())]);
        }
        let (hover_cursor, hover_color) = frame(vec![Event::PointerMoved(cell.center())]);
        assert_eq!(hover_cursor, egui::CursorIcon::PointingHand);
        assert_eq!(hover_color, tokens.palette.text);
    }

    #[test]
    fn the_row_keeps_its_hover_over_a_link_cell() {
        // A link cell's interact is created after the row's, and egui marks only
        // the top hit widget as hovered, so the row's own `hovered()` goes false
        // over the album and artist columns. The row must read where the pointer
        // is instead, or its highlight, its index play glyph and its like all
        // vanish there. The fill is the visible half of that; the glyph and the
        // like follow the same `revealed` value.
        let ctx = context();
        let tokens = Tokens::DARK;
        let mut library = library_with(vec![linkable("a")]);
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let row = row_rect_for(&ctx, 1, 0);
        let cell = cell_rect_for(&ctx, 1, 0, |columns| columns.album);

        // The frame's fill behind this row, read back from the shapes painted.
        let mut fill = |events: Vec<Event>| {
            let output = run(&ctx, input(events), |ui| {
                let _ = tracks_view(
                    ui,
                    Table::Full,
                    &mut library,
                    &mut transport,
                    &mut queue,
                    &mut Art::default(),
                    Instant::now(),
                    NOW_MS,
                    &tokens,
                );
            });
            output
                .shapes
                .iter()
                .find_map(|clipped| match &clipped.shape {
                    egui::epaint::Shape::Rect(shape) if shape.rect == row => Some(shape.fill),
                    _ => None,
                })
        };

        // With the pointer away the row paints no fill at all.
        assert_eq!(fill(Vec::new()), None, "a resting row paints no fill");

        // Resting on the album link must light the row to the same fill the
        // title would, over the reveal's own time.
        for _ in 0..12 {
            fill(vec![Event::PointerMoved(cell.center())]);
        }
        assert_eq!(
            fill(vec![Event::PointerMoved(cell.center())]),
            Some(tokens.palette.row_hover),
            "the row must keep its hover fill over the album link"
        );
    }

    #[test]
    fn a_double_click_in_the_album_cell_opens_it_without_playing() {
        let ctx = context();
        let mut library = library_with(vec![linkable("a")]);
        let mut transport = Transport::default();
        let mut queue = Queue::default();

        let cell = cell_rect_for(&ctx, 1, 0, |columns| columns.album);
        let command = double_click(
            &ctx,
            cell.center(),
            &mut library,
            &mut transport,
            &mut queue,
        );

        assert_eq!(
            library.page(),
            Some(&Page::Album("spotify:album:rec".to_owned())),
            "the second click lands on the album page, not on the row"
        );
        assert_eq!(
            command,
            Some(Command::LoadAlbum {
                album: "spotify:album:rec".to_owned()
            }),
            "the album page's load is the command, not a play"
        );
        assert!(!queue.can_next(), "a link cell must never start the queue");
        assert_eq!(transport.now_playing, None);
    }

    #[test]
    fn a_double_click_on_the_title_still_plays() {
        let ctx = context();
        let mut library = library_with(vec![linkable("a"), linkable("b")]);
        let mut transport = Transport::default();
        let mut queue = Queue::default();

        let cell = cell_rect_for(&ctx, 2, 0, |columns| columns.title);
        let command = double_click(
            &ctx,
            cell.center(),
            &mut library,
            &mut transport,
            &mut queue,
        );

        assert_eq!(
            command,
            Some(Command::Play {
                uri: "a".to_owned()
            })
        );
        assert_eq!(
            library.page(),
            Some(&Page::Playlist("spotify:playlist:a".to_owned()))
        );
        assert!(
            queue.can_next(),
            "the double-clicked row must start the queue"
        );
    }

    #[test]
    fn a_row_without_an_album_uri_keeps_a_plain_cell() {
        let ctx = context();
        let mut library = library_with(vec![TrackEntry {
            album_uri: None,
            ..linkable("a")
        }]);
        let mut transport = Transport::default();
        let mut queue = Queue::default();

        let cell = cell_rect_for(&ctx, 1, 0, |columns| columns.album);
        let command = click(
            &ctx,
            cell.center(),
            &mut library,
            &mut transport,
            &mut queue,
        );
        assert_eq!(
            command, None,
            "a cell with no URI must take no click of its own"
        );
        assert_eq!(
            library.page(),
            Some(&Page::Playlist("spotify:playlist:a".to_owned())),
            "a plain cell must not navigate"
        );

        // The cell still belongs to the row, so a double click there plays.
        let command = double_click(
            &ctx,
            cell.center(),
            &mut library,
            &mut transport,
            &mut queue,
        );
        assert_eq!(
            command,
            Some(Command::Play {
                uri: "a".to_owned()
            })
        );
    }

    #[test]
    fn an_album_with_no_displayed_name_is_not_a_link() {
        // The labelling can resolve a gid for an album the row never names. An
        // empty cell painted nothing, so it must offer no hit area either.
        let ctx = context();
        let mut library = library_with(vec![TrackEntry {
            album: Some(String::new()),
            ..linkable("a")
        }]);
        let mut transport = Transport::default();
        let mut queue = Queue::default();

        let cell = cell_rect_for(&ctx, 1, 0, |columns| columns.album);
        let command = click(
            &ctx,
            cell.center(),
            &mut library,
            &mut transport,
            &mut queue,
        );

        assert_eq!(command, None);
        assert_eq!(
            library.page(),
            Some(&Page::Playlist("spotify:playlist:a".to_owned())),
            "an unnamed album must not navigate"
        );
    }

    #[test]
    fn enter_and_space_play_with_the_pointer_on_a_link_cell() {
        // The pointer's hover gates only the pointer's double click. A keyboard
        // activation of the focused row is never taken by a link.
        for pressed in [egui::Key::Enter, egui::Key::Space] {
            let ctx = context();
            let tokens = Tokens::DARK;
            let mut library = library_with(vec![linkable("a")]);
            let mut transport = Transport::default();
            let mut queue = Queue::default();
            let cell = cell_rect_for(&ctx, 1, 0, |columns| columns.album);

            // Warm up so focus traversal and the hit test read a previous pass.
            for events in [Vec::new(), vec![key(egui::Key::Tab)]] {
                run(&ctx, input(events), |ui| {
                    let _ = tracks_view(
                        ui,
                        Table::Full,
                        &mut library,
                        &mut transport,
                        &mut queue,
                        &mut Art::default(),
                        Instant::now(),
                        NOW_MS,
                        &tokens,
                    );
                });
            }

            // Rest the pointer on the album cell, then press the key.
            let mut command = None;
            for events in [vec![Event::PointerMoved(cell.center())], vec![key(pressed)]] {
                let command = &mut command;
                let library = &mut library;
                let transport = &mut transport;
                let queue = &mut queue;
                run(&ctx, input(events), |ui| {
                    if let Some(next) = tracks_view(
                        ui,
                        Table::Full,
                        library,
                        transport,
                        queue,
                        &mut Art::default(),
                        Instant::now(),
                        NOW_MS,
                        &tokens,
                    ) {
                        *command = Some(next);
                    }
                });
            }

            assert_eq!(
                command,
                Some(Command::Play {
                    uri: "a".to_owned()
                }),
                "{pressed:?} on the focused row must play with the pointer on a link"
            );
            assert_eq!(
                library.page(),
                Some(&Page::Playlist("spotify:playlist:a".to_owned())),
                "{pressed:?} must not navigate"
            );
        }
    }

    /// The album's own page data for the view tests: one track, so a page can be
    /// told apart from the playlist behind it by its rows alone.
    fn album_page(uri: &str, name: &str) -> AlbumDetail {
        AlbumDetail {
            uri: uri.to_owned(),
            name: name.to_owned(),
            artists: "Band".to_owned(),
            year: Some(1997),
            cover: None,
            tracks: vec![TrackEntry {
                artists: "Band".to_owned(),
                ..track("spotify:track:t1", "Album Song")
            }],
        }
    }

    #[test]
    fn back_from_an_album_page_redraws_the_playlist() {
        let ctx = context();
        let mut library = library_with(vec![linkable("a")]);
        assert_eq!(
            library.open(Page::Album("spotify:album:rec".to_owned())),
            Some(Command::LoadAlbum {
                album: "spotify:album:rec".to_owned()
            })
        );
        library.apply(&Update::Album(album_page("spotify:album:rec", "Record")));
        let texts = painted_lines(&ctx, Table::Full, &mut library);
        assert!(
            texts.contains(&"Record".to_owned()),
            "the album page draws its own band: {texts:?}"
        );
        assert!(
            !texts.contains(&"t0".to_owned()),
            "the playlist behind it is not what is drawn: {texts:?}"
        );

        assert_eq!(
            library.back(),
            None,
            "a ready playlist with a matching wanted load needs no command"
        );

        let texts = painted_lines(&ctx, Table::Full, &mut library);
        assert!(
            texts.contains(&"t0".to_owned()),
            "Back must redraw the ready tracklist: {texts:?}"
        );
        assert!(
            !texts.contains(&"Album Song".to_owned()),
            "the album's rows are gone: {texts:?}"
        );
    }

    #[test]
    fn an_album_page_draws_the_album_view() {
        let ctx = context();
        let mut library = library_idle(vec![playlist("a", "A")]);
        library.select("a");
        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: vec![track("t0", "t0")],
            cover: None,
        });
        assert_eq!(
            library.open(Page::Album("spotify:album:x".to_owned())),
            Some(Command::LoadAlbum {
                album: "spotify:album:x".to_owned()
            })
        );
        library.apply(&Update::Album(album_page(
            "spotify:album:x",
            "Signal Decay",
        )));

        let texts = painted_lines(&ctx, Table::Full, &mut library);

        assert!(
            texts.contains(&"ALBUM".to_owned()),
            "the band's eyebrow: {texts:?}"
        );
        assert!(
            texts.contains(&"Signal Decay".to_owned()),
            "the band names the album: {texts:?}"
        );
        assert!(
            texts.contains(&"Album Song".to_owned()),
            "the album's table lists its own track: {texts:?}"
        );
        assert!(
            !texts.contains(&"t0".to_owned()),
            "the playlist behind the page is not what is drawn: {texts:?}"
        );
    }

    #[test]
    fn the_frame_loop_requests_only_visible_covers() {
        let ctx = context();
        let tokens = Tokens::DARK;
        // A thousand distinct covers in a short window: only the dozen rows the
        // viewport can show are ever laid out, even as the list is scrolled.
        let tracks: Vec<TrackEntry> = (0..1_000).map(track_with_cover).collect();
        let mut library = library_with(tracks);
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();
        let now = Instant::now();
        let mut requested: std::collections::HashMap<CoverUrl, usize> =
            std::collections::HashMap::new();
        let mut painted: std::collections::HashSet<usize> = std::collections::HashSet::new();
        let centre = Pos2::new(PROBE.x / 2.0, PROBE.y / 2.0);

        // Scroll from the top down. Every requested cover must belong to a row
        // the window painted in some frame, and the outstanding set must never
        // exceed the cap.
        for step in 0..240 {
            art.begin_frame();
            let mut events = vec![Event::PointerMoved(centre)];
            if step > 0 {
                events.push(Event::MouseWheel {
                    unit: MouseWheelUnit::Point,
                    delta: vec2(0.0, -extent::ROW_H),
                    phase: TouchPhase::Move,
                    source: MouseWheelSource::Unknown,
                    modifiers: Modifiers::NONE,
                });
            }
            let mut output = ctx.run_ui(input(events), |ui| {
                let _ = tracks_view(
                    ui,
                    Table::Full,
                    &mut library,
                    &mut transport,
                    &mut queue,
                    &mut art,
                    now,
                    NOW_MS,
                    &tokens,
                );
            });
            output.textures_delta.clear();
            painted.extend(painted_rows(&output));

            let commands = art.requests(now);
            assert!(
                art.outstanding() <= crate::cover::MAX_IN_FLIGHT,
                "{} fetches outstanding",
                art.outstanding()
            );
            for command in commands {
                if let Command::Fetch { url } = command {
                    requested.insert(url.clone(), cover_row(&url));
                    // Answer it, so the next frame can ask for the next row.
                    art.apply(
                        &Update::Cover {
                            url,
                            image: image(),
                        },
                        now,
                    );
                }
            }
        }

        assert!(!requested.is_empty(), "the visible rows must be fetched");
        assert!(
            painted.iter().copied().max().unwrap_or(0) > 50,
            "the frame loop must scroll through the list to exercise the cap"
        );
        for (url, row) in &requested {
            assert!(
                painted.contains(row),
                "{url:?} was requested for row {row}, which no frame ever painted"
            );
        }
    }

    #[test]
    fn a_grid_card_switches_to_the_context_image_when_it_arrives() {
        let ctx = context();
        let tokens = Tokens::DARK;
        let first = cover_url("first");
        let context_cover = cover_url("context");
        let mut library = library_idle(vec![playlist_with_cover("a", "A", first.clone())]);
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();
        let now = Instant::now();

        art.begin_frame();
        run(&ctx, input(Vec::new()), |ui| {
            let _ = tracks_view(
                ui,
                Table::Full,
                &mut library,
                &mut transport,
                &mut queue,
                &mut art,
                now,
                NOW_MS,
                &tokens,
            );
        });
        assert_eq!(fetch_urls(art.requests(now)), vec![first.clone()]);

        // A context image for an unselected playlist still upgrades its entry,
        // because the upgrade is keyed by the update's own playlist.
        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: Vec::new(),
            cover: Some(context_cover.clone()),
        });

        art.begin_frame();
        run(&ctx, input(Vec::new()), |ui| {
            let _ = tracks_view(
                ui,
                Table::Full,
                &mut library,
                &mut transport,
                &mut queue,
                &mut art,
                now,
                NOW_MS,
                &tokens,
            );
        });
        assert_eq!(fetch_urls(art.requests(now)), vec![context_cover.clone()]);
    }

    #[test]
    fn the_band_waits_for_the_context_image_and_then_shows_it() {
        let ctx = context();
        let tokens = Tokens::DARK;
        let first = cover_url("first");
        let context_cover = cover_url("context");
        let mut library = Library::default();
        library.apply(&Update::Playlists(vec![playlist_with_cover(
            "a",
            "Evening",
            first.clone(),
        )]));
        library.select("a");
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();
        let now = Instant::now();

        // While the tracks load, the band is drawn waiting and asks for nothing:
        // it must not fetch the first-track cover it is about to replace.
        art.begin_frame();
        let mut output = ctx.run_ui(input(Vec::new()), |ui| {
            let _ = tracks_view(
                ui,
                Table::Full,
                &mut library,
                &mut transport,
                &mut queue,
                &mut art,
                now,
                NOW_MS,
                &tokens,
            );
        });
        output.textures_delta.clear();
        assert!(
            drawn_texts(&output).contains(&"PLAYLIST"),
            "the band must head a loading playlist"
        );
        assert!(art.requests(now).is_empty(), "no cover yet");

        // The first page lands with the context image: the entry is upgraded and
        // the band wants it on the same frame.
        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: vec![track("t", "t")],
            cover: Some(context_cover.clone()),
        });
        art.begin_frame();
        run(&ctx, input(Vec::new()), |ui| {
            let _ = tracks_view(
                ui,
                Table::Full,
                &mut library,
                &mut transport,
                &mut queue,
                &mut art,
                now,
                NOW_MS,
                &tokens,
            );
        });
        assert_eq!(fetch_urls(art.requests(now)), vec![context_cover.clone()]);
    }

    #[test]
    fn the_band_keeps_an_already_held_cover_while_the_tracks_reload() {
        let ctx = context();
        let cover = cover_url("held");
        let entry = playlist_with_cover("a", "Evening", cover.clone());
        let now = Instant::now();
        let mut art = Art::default();

        // An earlier visit to the playlist fetched and held its cover.
        art.begin_frame();
        assert_eq!(art.look(Some(&cover), now), Cover::Loading);
        assert_eq!(fetch_urls(art.requests(now)), vec![cover.clone()]);
        art.apply(
            &Update::Cover {
                url: cover.clone(),
                image: image(),
            },
            now,
        );
        art.upload(&ctx);
        assert!(matches!(art.look(Some(&cover), now), Cover::Ready(_)));

        // Re-selecting the playlist puts the tracks back into `Loading`. The
        // band still draws the picture it already holds rather than flashing the
        // placeholder, and does not fetch it again.
        art.begin_frame();
        assert!(
            matches!(
                band_cover_for(&entry, false, &mut art, now),
                Cover::Ready(_)
            ),
            "the band keeps the cover it already holds"
        );
        assert!(
            fetch_urls(art.requests(now)).is_empty(),
            "a held cover is not fetched again"
        );

        // A playlist whose cover is not held still waits without fetching it.
        let other = playlist_with_cover("b", "Other", cover_url("unheld"));
        art.begin_frame();
        assert_eq!(band_cover_for(&other, false, &mut art, now), Cover::Loading);
        assert!(fetch_urls(art.requests(now)).is_empty());
    }

    /// The band's worded parts below: 62 tracks of 223_548 ms sum to 13_859_976
    /// ms, which the byline rounds to `3 hr 51 min`.
    const DESCRIPTION: &str = "Windows down, city behind you.";
    const META: &str = "Made by alice \u{b7} 62 songs \u{b7} 3 hr 51 min";
    /// Long enough that two rows of the band's column cannot hold it.
    const LONG_DESCRIPTION: &str = "Windows down, city behind you. Updated every \
        Sunday, and again whenever the mood takes me, with no plan and no promise, \
        only the long way home through the sodium light of the ring road, past the \
        depot and the all-night garage, until the city runs out.";

    /// A band whose description, owner and tracks are all known: the widest case
    /// the tests below read.
    fn library_full() -> Library {
        library_detailed(
            timed_tracks(62, 223_548),
            "Evening",
            Some(DESCRIPTION),
            Some("alice"),
        )
    }

    #[test]
    fn the_band_draws_the_description_and_the_owner() {
        let ctx = context();
        let mut library = library_full();
        let output = tracklist_frame(&ctx, Table::Full, &mut library);
        let texts = drawn_texts(&output);
        assert!(
            texts.contains(&DESCRIPTION),
            "the band must draw the description: {texts:?}"
        );
        assert!(
            texts.contains(&META),
            "the band must draw the owner ahead of the count and the total: {texts:?}"
        );
    }

    #[test]
    fn the_band_stacks_the_eyebrow_the_title_the_description_and_the_meta() {
        let ctx = context();
        let mut library = library_full();
        let output = tracklist_frame(&ctx, Table::Full, &mut library);
        let drawn = drawn_galleys(&output);
        let eyebrow = galley_top(&drawn, "PLAYLIST");
        let title = galley_top(&drawn, "Evening");
        let description = galley_top(&drawn, DESCRIPTION);
        let meta = galley_top(&drawn, META);
        assert!(
            eyebrow < title && title < description && description < meta,
            "the stack reads eyebrow, title, description, meta downwards: \
             {eyebrow} {title} {description} {meta}"
        );
    }

    #[test]
    fn a_playlist_without_a_description_or_owner_draws_neither_and_leaves_no_gap() {
        let ctx = context();
        let mut library = library_detailed(timed_tracks(2, 1_000), "Evening", None, None);
        let output = tracklist_frame(&ctx, Table::Full, &mut library);
        let texts = drawn_texts(&output);
        assert!(
            !texts.iter().any(|text| text.starts_with("Made by")),
            "no owner means no byline: {texts:?}"
        );
        assert!(
            !texts.contains(&DESCRIPTION),
            "no description means no description line: {texts:?}"
        );

        // With no description the title and the meta are one stack gap apart,
        // not two: an absent description must not leave its own space behind.
        let drawn = drawn_galleys(&output);
        let title_bottom = galley_top(&drawn, "Evening") + galley_height(&drawn, "Evening");
        let meta = galley_top(&drawn, "2 songs \u{b7} 0 min");
        assert!(
            (meta - title_bottom - space::S2).abs() < 0.01,
            "the gap between the title and the meta is {} with no description, \
             not the stack's own {}",
            meta - title_bottom,
            space::S2
        );
    }

    #[test]
    fn a_description_without_an_owner_or_tracks_draws_only_the_description() {
        let ctx = context();
        // The listing names a description but no owner, and the tracks are not
        // asked for: the band has one byline and no meta line at all.
        let mut library = library_listed("Evening", Some(DESCRIPTION), None);
        let output = tracklist_frame(&ctx, Table::Full, &mut library);
        let texts = drawn_texts(&output);
        assert!(
            texts.contains(&DESCRIPTION),
            "the description draws with no owner and no tracks: {texts:?}"
        );
        assert!(
            !texts.iter().any(|text| text.starts_with("Made by")),
            "no owner means no byline: {texts:?}"
        );
        assert!(
            !texts.iter().any(|text| text.contains("songs")),
            "no ready tracks means no count: {texts:?}"
        );
        assert!(
            !texts.iter().any(|text| text.contains('\u{b7}')),
            "the meta line is absent, not drawn empty: {texts:?}"
        );
        assert_eq!(
            texts.len(),
            3,
            "the band paints the eyebrow, the title and the description, and
             nothing where the meta would be: {texts:?}"
        );
    }

    #[test]
    fn the_band_names_the_owner_before_the_tracks_are_ready() {
        let ctx = context();
        let mut library = library_listed("Evening", None, Some("alice"));
        let output = tracklist_frame(&ctx, Table::Full, &mut library);
        let texts = drawn_texts(&output);
        assert!(
            texts.contains(&"Made by alice"),
            "the owner must show while the tracks load: {texts:?}"
        );
        assert!(
            !texts.iter().any(|text| text.contains("songs")),
            "the count waits for the tracks: {texts:?}"
        );
    }

    #[test]
    fn a_long_description_is_clamped_to_two_rows_and_elided() {
        let ctx = context();
        let mut library = library_detailed(
            timed_tracks(2, 1_000),
            "Evening",
            Some(LONG_DESCRIPTION),
            None,
        );
        let output = tracklist_frame(&ctx, Table::Full, &mut library);
        let galley = galley_of(&output, LONG_DESCRIPTION);
        assert_eq!(galley.rows.len(), 2, "two rows, no more");
        assert!(galley.elided, "a description that does not fit elides");
        let last = galley.rows[1].row.text();
        assert!(
            last.ends_with('\u{2026}'),
            "the elided last row must end in an ellipsis: {last:?}"
        );
    }

    #[test]
    fn a_narrow_band_elides_the_byline() {
        let ctx = context();
        let mut library = library_detailed(
            timed_tracks(62, 223_548),
            "Evening",
            None,
            Some("alice-the-very-long-username"),
        );
        let output = tracklist_frame_in(&ctx, Vec2::new(560.0, 800.0), &mut library);
        let galley = galley_of(
            &output,
            "Made by alice-the-very-long-username \u{b7} 62 songs \u{b7} 3 hr 51 min",
        );
        assert!(
            galley.elided,
            "a byline wider than the band elides: {:?}",
            galley.text()
        );
    }

    #[test]
    fn the_band_does_not_clip_the_eyebrow_at_its_tallest_content() {
        let ctx = context();
        let tokens = Tokens::DARK;
        let mut band_top = None;
        let output = run(
            &ctx,
            sized_input(Vec2::new(1600.0, 900.0), Vec::new()),
            |ui| {
                band_top = Some(ui.cursor().min.y);
                let band = hero::Band {
                    eyebrow: "Playlist",
                    title: "Late Night Drive",
                    meta: vec![
                        "Made by alice".to_owned(),
                        "62 songs".to_owned(),
                        "3 hr 51 min".to_owned(),
                    ],
                    description: LONG_DESCRIPTION,
                    playable: true,
                    shuffle: true,
                    cover: Cover::Missing,
                };
                let _ = hero::show(ui, &band, &tokens);
            },
        );
        let band_top = band_top.expect("the band starts at the cursor");
        let drawn = drawn_galleys(&output);
        let eyebrow = galley_top(&drawn, "PLAYLIST");
        assert!(
            eyebrow >= band_top,
            "the eyebrow's top {eyebrow} is above the band's top {band_top}: the \
             tallest content does not fit the band's height"
        );
    }
}

#[cfg(test)]
mod dispatch_tests {
    use super::*;

    #[test]
    fn main_page_maps_show_to_the_show_view() {
        let mut library = Library::default();
        assert_eq!(main_page(&library), MainPage::Root);
        let _ = library.open(Page::Playlist("spotify:playlist:a".to_owned()));
        assert_eq!(main_page(&library), MainPage::Playlist);
        let _ = library.open(Page::Album("spotify:album:a".to_owned()));
        assert_eq!(main_page(&library), MainPage::Album);
        let _ = library.open(Page::Artist("spotify:artist:a".to_owned()));
        assert_eq!(main_page(&library), MainPage::Artist);
        let _ = library.open(Page::Show("spotify:show:a".to_owned()));
        assert_eq!(
            main_page(&library),
            MainPage::Show,
            "a show page has its own body now"
        );
    }
}

#[cfg(test)]
mod root_tests {
    use std::time::Instant;

    use egui::{Context, Event, Pos2, RawInput, Rect, Vec2};
    use takofy_model::{CoverUrl, Episode, FollowedShow, PlaylistEntry};

    use super::*;
    use crate::message::{Failure, Update};
    use crate::theme::{extent, fonts, style};
    use crate::view::test_support::click_events;

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
            uri: uri.to_owned(),
            name: name.to_owned(),
            named: true,
            cover: None,
            description: None,
            owner: None,
            tracks: None,
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
            ui.add_space(space::S3);
            egui::ScrollArea::vertical().show(ui, |ui| {
                crate::view::inner(ui, |ui| {
                    page_head(ui, &library_note(count), &tokens);
                    let _ = filter_row::show_with(
                        ui,
                        &tokens,
                        &library_filter::ROW,
                        RootKind::All,
                        RootKind::control,
                        root_filter_label,
                        filter_row::RowSlot::None,
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
                cover: Some(
                    CoverUrl::from_wire(&format!("https://i.scdn.co/image/{index:0<40}"))
                        .expect("a cover url"),
                ),
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
}
