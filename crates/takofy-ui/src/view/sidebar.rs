//! The sidebar: the library's four filters, its size and its list.
//!
//! Below 900 points this is not a column at all: the layout gives it no width
//! and the drawer holds it, which is why there is one form here and no rail.

use std::time::Instant;

use egui::{Layout, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, Ui, UiBuilder, Vec2};
use takofy_model::{
    CoverUrl, FollowedArtist, FollowedShow, PLAYLIST_NAME_MAX, PlaylistEntry, SavedAlbum,
};

use crate::availability::Control;
use crate::cover::{Art, Cover};
use crate::format::{
    library_count, library_kind, playlist_detail, rail_nothing, sidebar_kind_label, sidebar_notice,
};
use crate::icon::Icon;
use crate::landing::Zone;
use crate::layout::chips;
use crate::library::{Library, Load};
use crate::menu::{Door, LibraryItem};
use crate::message::{Command, Failure};
use crate::naming::{Renaming, Surface};
use crate::rail_filter;
use crate::route::Page;
use crate::sidebar_kind::{self, SidebarKind};
use crate::signin::failure_message;
use crate::theme::icon::Size;
use crate::theme::{
    MonoWeight, Tokens, Weight, extent, fonts, radius, space, stroke, text, tracking,
};
use crate::track_filter::QUERY_MAX;

use super::art;
use super::carry::{self, Carry, ColumnOffer, Ghost, Source};
use super::context_menu;
use super::controls;
use super::filter_field;
use super::name_field;
use super::queue_row;
use super::rows::{self, Look};
use super::shell::rule;
use super::text as recipe;
use crate::verbs::{self, LibraryRights};

/// A library row: the art tile plus the padding above and below it.
pub(super) const ROW_H: f32 = extent::LIBRARY_ART + 2.0 * space::S2;

/// How many chips a row of the filter grid holds.
const CHIP_COLUMNS: usize = 2;

/// Draws the sidebar in the rail's drawer, returning a command if a row opened
/// a page.
///
/// Its rows do not lift: a write started here would leave a pending write that
/// no command ever answers, because the drawer form offers no drop target.
///
/// `free` is whether the frame still has room for this form's command. The
/// drawer draws after every region, so a command already taken this frame means
/// this one would be dropped after the row had already armed its load, and a
/// page armed with no request behind it spins until the app restarts. A row
/// picked in that frame is refused instead.
pub fn show(
    ui: &mut Ui,
    library: &mut Library,
    art: &mut Art,
    now: Instant,
    tokens: &Tokens,
    free: bool,
) -> Asked {
    draw(ui, library, art, now, tokens, false, free)
}

/// Draws the sidebar as the column, whose rows lift and offer themselves.
pub fn column(
    ui: &mut Ui,
    library: &mut Library,
    art: &mut Art,
    now: Instant,
    tokens: &Tokens,
) -> Option<Command> {
    draw(ui, library, art, now, tokens, true, true).command
}

fn draw(
    ui: &mut Ui,
    library: &mut Library,
    art: &mut Art,
    now: Instant,
    tokens: &Tokens,
    drag: bool,
    free: bool,
) -> Asked {
    let band = ui.available_rect_before_wrap();
    let y = filters(ui, library, tokens, band);
    let head = head(ui, library, tokens, band, y, drag);

    // The survivors are read from the same list the rows draw, after the field
    // has taken this frame's keystroke, clear or Escape.
    let kept = rail_kept(library);
    // A rename field on a row the query hides would keep asking for the
    // keyboard, and a keystroke that leaves its row drawn still moves the list
    // under it: either way the field goes.
    if head.changed
        || kept
            .as_deref()
            .is_some_and(|kept| rename_hidden(library, kept))
    {
        library.dismiss_row_rename();
    }
    // The count is one string in two places: the field's resting line, and a
    // polite status node. Sighted users see it whenever the field is empty; a
    // screen reader hears it when it changes, which is not every keystroke.
    let (kind_shown, total) = counts(library);
    let shown = kept.as_ref().map_or(kind_shown, Vec::len);
    announce(ui, shown, total, head.field);

    let list = Rect::from_min_size(
        Pos2::new(band.left() + space::S2, head.top),
        Vec2::new(band.width() - 2.0 * space::S2, band.bottom() - head.top),
    );
    rows(
        ui,
        list,
        library,
        art,
        now,
        tokens,
        drag,
        kept.as_deref(),
        free,
    )
}

/// The four filters, in a fixed 2x2 block at the top of the sidebar.
///
/// Pressing the pressed chip does nothing: the reference's exclusive group keeps
/// one chip lit, because a library with no filter and a library filtered to
/// nothing are not the same view.
fn filters(ui: &mut Ui, library: &mut Library, tokens: &Tokens, band: Rect) -> f32 {
    let width = band.width() - 2.0 * space::S4;
    let placed = chips::grid(
        sidebar_kind::ROW.len(),
        width,
        CHIP_COLUMNS,
        extent::BUTTON,
        space::S1,
    );
    let origin = Vec2::new(band.left() + space::S4, band.top() + space::S4);
    let mut pressed: Option<SidebarKind> = None;
    for (kind, rect) in sidebar_kind::ROW.iter().zip(placed.rects) {
        let slot = controls::chip_in(
            ui,
            tokens,
            kind.control(),
            rect.translate(origin),
            sidebar_kind_label(*kind),
            library.sidebar_kind.chosen() == *kind,
        );
        if slot.response.clicked() {
            pressed = Some(*kind);
        }
    }
    if let Some(kind) = pressed {
        library.choose_kind(kind);
    }

    origin.y + placed.height + space::S3
}

/// What the head leaves the rest of the frame.
struct Head {
    /// Where the list under it starts.
    top: f32,
    /// The filter field's widget id, when the field was drawn this frame.
    ///
    /// `None` while the create field holds the slot, so the announcement below
    /// never builds a node for a field that is not on screen.
    field: Option<egui::Id>,
    /// Whether this frame moved the query.
    changed: bool,
}

/// The rule, the name filter and the button that creates a playlist.
///
/// The field and the new playlist's name share one slot: the button swaps the
/// field for the create field rather than opening a second surface, and becomes
/// the cancel that closes it. The drawer's form discards its command, so the
/// button is not live there — a press that started a write would leave the
/// sidebar waiting for a reply nobody asked for. The filter itself is live in
/// the drawer: narrowing a list in memory writes nothing.
fn head(
    ui: &mut Ui,
    library: &mut Library,
    tokens: &Tokens,
    band: Rect,
    top: f32,
    drag: bool,
) -> Head {
    rule(
        ui,
        Rect::from_min_size(
            Pos2::new(band.left() + space::S1, top),
            Vec2::new(band.width() - 2.0 * space::S1, stroke::HAIRLINE),
        ),
        tokens.palette.border,
    );

    let row = Rect::from_min_size(
        Pos2::new(band.left() + space::S4, top + space::S2),
        Vec2::new(band.width() - 2.0 * space::S4, extent::BUTTON),
    );
    let open = drag && library.naming.is_open();
    let create = Rect::from_min_size(
        Pos2::new(
            row.right() - extent::BUTTON,
            row.center().y - extent::BUTTON / 2.0,
        ),
        Vec2::splat(extent::BUTTON),
    );

    // The field takes what the button leaves, which is the reference's line for
    // the rail: the count used to sit in this slot, and now the field does.
    let slot = Rect::from_min_max(row.min, Pos2::new(create.left() - space::S2, row.bottom()));
    let mut changed = false;
    let field = if open {
        // A created playlist would be hidden by a query that does not match the
        // name being typed. The create field and the filter field share one
        // slot, because the head row holds one field at a time, so opening the
        // create field clears the query rather than drawing two fields.
        library.rail_filter.clear();
        name_field(ui, slot, library, tokens);
        None
    } else {
        let (shown, total) = counts(library);
        let placeholder = library_count(shown, total);
        let id = ui.scope_id().with("library-filter");
        let edited = filter_field::show(
            ui,
            filter_field::Field {
                id,
                rect: slot,
                text: library.rail_filter.edit(),
                limit: QUERY_MAX,
                placeholder: &placeholder,
                label: "Filter your library",
                rule: filter_field::Rule::Resting,
            },
            tokens,
        );
        changed = edited.changed;
        Some(id)
    };

    let (icon, label, live) = if open {
        let writing = library.naming.writing();
        (Icon::Close, "Cancel new playlist", drag && !writing)
    } else {
        (Icon::Plus, "Create playlist", drag && library.creatable())
    };
    let slot = controls::icon_button(
        ui,
        tokens,
        Control::LibraryCreate,
        create,
        icon,
        Size::MD,
        label,
        live,
    );
    if slot.response.clicked() {
        if open {
            library.naming.cancel();
        } else {
            library.naming.open();
        }
    }
    // The keyboard comes back here after a cancel and after a create: the field
    // is gone, so nothing else could hold it.
    if library.naming.wants_button() {
        slot.response.request_focus();
        library.naming.button_focus_taken();
    }

    Head {
        top: row.bottom() + space::S2,
        field,
        changed,
    }
}

/// The new playlist's name, in the count's place.
///
/// Enter and Escape are taken before `TextEdit::show`, because a singleline edit
/// surrenders focus on Enter and would eat the key that submits. A submit is
/// posted for the shell rather than sent from here: the field cannot know
/// whether another region already took the frame's one command.
fn name_field(ui: &mut Ui, rect: Rect, library: &mut Library, tokens: &Tokens) {
    if rect.width() <= 0.0 {
        return;
    }
    ui.painter()
        .rect_filled(rect, radius::SM, tokens.palette.surface_raised);

    let writing = library.naming.writing();
    let wants_focus = library.naming.wants_focus();
    let id = ui.scope_id().with((Control::LibraryCreate, "name"));
    let out = {
        let text = library.naming.edit();
        name_field::edit(
            ui,
            name_field::Spec {
                id,
                rect,
                text,
                writing,
                wants_focus,
                font: egui::FontId::new(text::SM, fonts::sans(Weight::Regular)),
                hint: "New playlist name",
                inset: egui::Margin {
                    left: space::S2 as i8,
                    right: space::S2 as i8,
                    top: 0,
                    bottom: 0,
                },
                text_color: tokens.palette.text,
                weak_color: tokens.palette.text_subtle,
                cursor: Stroke::new(stroke::HAIRLINE, tokens.palette.text),
                align: egui::Align2::LEFT_CENTER,
                limit: PLAYLIST_NAME_MAX,
            },
        )
    };
    if out.focused {
        library.naming.focus_taken();
    }
    // A blank field left behind says nothing, so it goes; text does not, so a
    // click elsewhere never loses a name.
    if out.lost_focus && library.naming.text().trim().is_empty() {
        library.naming.cancel();
    }

    if out.enter {
        post_submit(ui.ctx());
    }
    if out.escape {
        library.naming.cancel();
    }

    // Border only: no glow, no glyph, and no width change, because the field's
    // own width is the row's.
    let border = if out.focused {
        tokens.palette.text
    } else {
        tokens.palette.border
    };
    ui.painter().rect_stroke(
        rect,
        radius::SM,
        Stroke::new(stroke::HAIRLINE, border),
        StrokeKind::Inside,
    );
}

fn submit_id() -> egui::Id {
    egui::Id::unique("library-name-submit")
}

/// Posts the head's submit for the shell to apply this frame.
fn post_submit(ctx: &egui::Context) {
    ctx.data_mut(|data| data.insert_temp(submit_id(), true));
}

/// The submit posted this frame, read once so it cannot outlive it.
pub(crate) fn take_submit(ctx: &egui::Context) -> bool {
    ctx.data_mut(|data| data.remove_temp::<bool>(submit_id()).unwrap_or(false))
}

/// Announces how many rows the rail shows, in the field's own resting words.
///
/// egui has no live region, so this is the smallest honest mechanism: a
/// `Role::Status` node the platform adapters treat as a polite live region, and
/// the same string on the field's own node as its placeholder. It is announced
/// when the node's name changes — a keystroke that leaves the count alone says
/// nothing — and when the node first appears. It is not an ARIA emulation.
fn announce(ui: &Ui, shown: usize, total: usize, field: Option<egui::Id>) {
    let text = library_count(shown, total);
    let ctx = ui.ctx();
    let _ = ctx.accesskit_node_builder(ui.scope_id().with("library-count"), |node| {
        node.set_role(egui::Role::Status);
        node.set_live(egui::accesskit::Live::Polite);
        node.set_label(text.as_str());
    });
    if let Some(id) = field {
        // Only when the field drew: a node for an id that is not on screen would
        // be a stray in the tree while the create field holds the slot.
        // Written after the edit, so it wins over the empty hint egui filled in.
        let _ = ctx.accesskit_node_builder(id, |node| node.set_placeholder(text.as_str()));
    }
}

/// How many things the sidebar lists, and how many the chosen filter shows.
fn counts(library: &Library) -> (usize, usize) {
    let playlists = match &library.playlists {
        Load::Ready(playlists) => playlists.len(),
        Load::Idle | Load::Loading | Load::Failed(_) => 0,
    };
    let shows = match &library.followed {
        Load::Ready(shows) => shows.len(),
        Load::Idle | Load::Loading | Load::Failed(_) => 0,
    };
    let artists = match &library.followed_artists {
        Load::Ready(artists) => artists.len(),
        Load::Idle | Load::Loading | Load::Failed(_) => 0,
    };
    let albums = match &library.saved_albums {
        Load::Ready(albums) => albums.len(),
        Load::Idle | Load::Loading | Load::Failed(_) => 0,
    };
    let shown = match library.sidebar_kind.chosen() {
        SidebarKind::Playlists => playlists,
        SidebarKind::Podcasts => shows,
        SidebarKind::Artists => artists,
        SidebarKind::Albums => albums,
    };
    (shown, playlists + shows + artists + albums)
}

/// The true rows the rail's query leaves, ascending; `None` when it leaves the
/// whole list.
///
/// It reads the same `Load::Ready` list the rows draw, so what the count says,
/// what the empty line would describe and what is painted are one set.
fn rail_kept(library: &Library) -> Option<Vec<usize>> {
    let needle = library.rail_filter.needle()?;
    Some(match library.sidebar_kind.chosen() {
        SidebarKind::Playlists => kept_in(&library.playlists, &needle),
        SidebarKind::Podcasts => kept_in(&library.followed, &needle),
        SidebarKind::Artists => kept_in(&library.followed_artists, &needle),
        SidebarKind::Albums => kept_in(&library.saved_albums, &needle),
    })
}

/// The ascending true rows of one list whose name contains `needle`.
fn kept_in<T: ListItem>(list: &Load<Vec<T>>, needle: &str) -> Vec<usize> {
    match list {
        Load::Ready(items) => rail_filter::keep(items.iter().map(ListItem::name), needle),
        Load::Idle | Load::Loading | Load::Failed(_) => Vec::new(),
    }
}

/// Whether an open row rename's row is not among the query's survivors.
///
/// A field on a hidden row would keep asking for the keyboard while there is
/// nothing on screen to type into.
fn rename_hidden(library: &Library, kept: &[usize]) -> bool {
    if !library.renaming.is_open() || library.renaming.surface() != Surface::Row {
        return false;
    }
    let uri = library.renaming.uri();
    let survives = match library.sidebar_kind.chosen() {
        SidebarKind::Playlists => kept_holds(&library.playlists, kept, uri),
        SidebarKind::Podcasts => kept_holds(&library.followed, kept, uri),
        SidebarKind::Artists => kept_holds(&library.followed_artists, kept, uri),
        SidebarKind::Albums => kept_holds(&library.saved_albums, kept, uri),
    };
    !survives
}

/// Whether `uri` is one of the rows `kept` names.
fn kept_holds<T: ListItem>(list: &Load<Vec<T>>, kept: &[usize], uri: &str) -> bool {
    match list {
        Load::Ready(items) => kept
            .iter()
            .any(|&row| items.get(row).is_some_and(|item| item.uri() == uri)),
        Load::Idle | Load::Loading | Load::Failed(_) => false,
    }
}

/// What a list draws: the items, or the state of the loads behind them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListView<'a, T> {
    /// The playlists or the list itself are in flight, or nothing has been
    /// asked for yet.
    Loading,
    /// A fetch failed; the retry names which load to restart.
    Failed(Failure, Retry),
    /// The user's library holds nothing of this kind.
    Empty,
    /// The items to draw, in the set's own order.
    List(&'a [T]),
}

/// Which load a retry restarts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Retry {
    /// The playlists; every followed list restarts with them.
    Playlists,
    /// The followed shows alone.
    Followed,
    /// The followed artists alone.
    FollowedArtists,
    /// The saved albums alone.
    SavedAlbums,
}

impl Retry {
    /// The command that restarts this load.
    fn go(self, library: &mut Library) -> Option<Command> {
        Some(match self {
            Self::Playlists => library.reload(),
            Self::Followed => library.reload_followed(),
            Self::FollowedArtists => library.reload_followed_artists(),
            Self::SavedAlbums => library.reload_saved_albums(),
        })
    }
}

/// Reads what a list draws from the playlists load and its own.
///
/// The playlists are checked first, because every followed request fires only
/// once the playlists are Ready (`followed_request` and its siblings), and a
/// retry of the playlists resets the followed lists to Idle.
fn list_view<'a, T>(
    playlists: &'a Load<Vec<PlaylistEntry>>,
    list: &'a Load<Vec<T>>,
    retry: Retry,
) -> ListView<'a, T> {
    match playlists {
        Load::Failed(failure) => ListView::Failed(*failure, Retry::Playlists),
        Load::Idle | Load::Loading => ListView::Loading,
        Load::Ready(_) => match list {
            Load::Idle | Load::Loading => ListView::Loading,
            Load::Failed(failure) => ListView::Failed(*failure, retry),
            Load::Ready(items) if items.is_empty() => ListView::Empty,
            Load::Ready(items) => ListView::List(items),
        },
    }
}

/// What the Playlists list draws.
fn playlists_view(library: &Library) -> ListView<'_, PlaylistEntry> {
    list_view(&library.playlists, &library.playlists, Retry::Playlists)
}

/// What the Podcasts list draws.
pub fn shows_view(library: &Library) -> ListView<'_, FollowedShow> {
    list_view(&library.playlists, &library.followed, Retry::Followed)
}

/// What the Artists list draws.
pub fn artists_view(library: &Library) -> ListView<'_, FollowedArtist> {
    list_view(
        &library.playlists,
        &library.followed_artists,
        Retry::FollowedArtists,
    )
}

/// What the Albums list draws.
pub fn albums_view(library: &Library) -> ListView<'_, SavedAlbum> {
    list_view(
        &library.playlists,
        &library.saved_albums,
        Retry::SavedAlbums,
    )
}

/// One row's identity: the scope its scroll area owns and the kind its widget
/// id carries, so two lists never share an id even when a uri is the same.
#[derive(Debug, Clone, Copy)]
struct Ids {
    scope: &'static str,
    kind: &'static str,
    /// The row's kind, which its menu carries.
    shelf: SidebarKind,
    /// Whether this kind is a list a drag can lift a row out of.
    lifts: bool,
}

impl Ids {
    fn id(self, ui: &Ui, uri: &str) -> egui::Id {
        ui.scope_id().with((self.kind, uri))
    }
}

/// The list's own words while it loads, or when there is nothing in it.
#[derive(Debug, Clone, Copy)]
struct Words {
    loading: &'static str,
    empty: &'static str,
}

/// What a press opens.
#[derive(Debug, Clone, Copy)]
enum Open {
    /// A playlist, which the transport has to select first.
    Playlist,
    /// A show, an artist or an album, each a route of its own.
    Page(fn(String) -> Page),
}

impl Open {
    fn go(self, library: &mut Library, uri: &str) -> Option<Command> {
        match self {
            Self::Playlist => Some(library.select(uri)),
            Self::Page(page) => library.open(page(uri.to_owned())),
        }
    }
}

/// The frame's drawing context, gathered so a list's own arguments stay few.
struct Draw<'a> {
    art: &'a mut Art,
    now: Instant,
    tokens: &'a Tokens,
    /// Whether a row may open its menu. The drawer's form posts nothing.
    menu: bool,
    /// Whether this kind's rows may be removed from the library.
    removable: bool,
    /// Whether the playlists list may rewrite a row's name now.
    renamable: bool,
    /// The page the menu records, so a page change closes it.
    here: Option<Page>,
    /// Whether a row may be dragged out of the column.
    drag: bool,
    /// Whether the column may lift a playlist row now.
    lift: bool,
    /// Whether the column offers its rows as a target.
    ///
    /// An add only needs the row's own playlist, so it is offered whenever the
    /// list is ready; the reorder it might mean instead is refused elsewhere.
    offer: bool,
    /// Whether this frame's list should show its first row.
    ///
    /// Taken once, so scrolling stays the user's on every other frame.
    reveal: bool,
    /// The open rename field, taken from the library for this frame's list.
    renaming: Renaming,
    /// The true rows the query left, ascending; `None` when it left every row.
    kept: Option<&'a [usize]>,
    /// The trimmed query itself, owned, for the line that names it.
    ///
    /// Owned because `Draw` is built from `&mut Library` and cannot keep a borrow
    /// of `rail_filter` for the whole frame.
    query: String,
}

/// What a press in a list asks the window to do.
enum Action {
    Open(String),
    Retry(Retry),
}

/// What the drawer's form asked the shell for.
///
/// A retry wants its command sent too, but it is not a pick: a list that failed
/// asks for itself on the frame it is drawn, and that must not read as the user
/// choosing a row and closing the drawer.
#[derive(Debug, Default)]
pub struct Asked {
    /// The command to send, when this form has one.
    pub command: Option<Command>,
    /// Whether a row was pressed this frame.
    pub picked: bool,
}

/// Draws one list of items, whatever they are.
///
/// The four kinds differ in three things only: what the row says under its name,
/// which page a press opens and what the list calls itself while it loads. Those
/// come in as arguments; the row, the scroll area and the states are shared.
fn list_rows<T: ListItem + Clone>(
    ui: &mut Ui,
    rect: Rect,
    view: ListView<'_, T>,
    current: Option<&str>,
    ids: Ids,
    words: Words,
    draw: &mut Draw<'_>,
) -> Option<Action> {
    match view {
        ListView::Loading => {
            state(ui, rect, recipe::muted(words.loading, draw.tokens));
            None
        }
        ListView::Empty => {
            state(ui, rect, recipe::muted(words.empty, draw.tokens));
            None
        }
        ListView::Failed(failure, retry) => {
            state(
                ui,
                rect,
                recipe::muted(failure_message(failure), draw.tokens),
            );
            Some(Action::Retry(retry))
        }
        ListView::List(items) => {
            let mut pressed: Option<String> = None;
            // The drawn slots and the true rows they stand for. A query narrows
            // `positions`; every write this frame makes names
            // `positions[index]`, because a slot's number is not the playlist's
            // index while the list is narrowed.
            let positions: Vec<usize> = match draw.kept {
                Some(kept) => kept
                    .iter()
                    .copied()
                    .filter(|&row| row < items.len())
                    .collect(),
                None => (0..items.len()).collect(),
            };
            let entries: Vec<T> = positions.iter().map(|&row| items[row].clone()).collect();
            // The query is what emptied the list, so it is named rather than the
            // kind's own sentence: only a blank list can truly say "nothing yet".
            if entries.is_empty() && !draw.query.is_empty() {
                let line = rail_nothing(&draw.query);
                state(ui, rect, recipe::muted(&line, draw.tokens));
                return None;
            }
            // A query that happened to match every row draws the whole list, so
            // its slots are still the true indices and it is not a narrowing.
            let narrowed = positions.len() != items.len();
            let mut lifted_carry: Option<Carry> = None;
            let mut opened: Option<(LibraryItem, Door, bool)> = None;
            // The first drawn row and the index it holds, so the offer describes
            // the rows wherever the scroll put them.
            let mut origin: Option<(usize, Rect)> = None;
            let mut edit_drawn = false;
            let lifted = match carry::carried(ui.ctx()) {
                Some(carried) if carried.source == Source::Playlist => carried.rows.clone(),
                _ => Vec::new(),
            };
            // A held block moves rows under the menu, so it opens none.
            let menu = draw.menu && !carry::carrying(ui.ctx());
            ui.scope_builder(
                UiBuilder::new()
                    .id_salt(ids.scope)
                    .max_rect(rect)
                    .layout(Layout::top_down(egui::Align::Min)),
                |ui| {
                    ui.spacing_mut().item_spacing.y = space::PX;
                    let mut area = egui::ScrollArea::vertical().auto_shrink([false, false]);
                    if draw.reveal {
                        // The only scroll-to the sidebar has: the offset is set on
                        // the frame the reveal is taken and left alone after it.
                        area = area.vertical_scroll_offset(0.0);
                    }
                    area.show_rows(ui, ROW_H, entries.len(), |ui, range| {
                        let start = range.start;
                        for index in range {
                            let Some(item) = entries.get(index) else {
                                continue;
                            };
                            let (_, place) =
                                ui.allocate_space(Vec2::new(ui.available_width(), ROW_H));
                            if origin.is_none() {
                                origin = Some((start, place));
                            }
                            let kind = item.kind();
                            let edit =
                                draw.drag && draw.renaming.is_open_on(item.uri(), Surface::Row);
                            edit_drawn |= edit;
                            let cover = draw.art.look(item.cover(), draw.now);
                            // The playlist this drawn slot stands for.
                            let true_row = positions[index];
                            let row = LibraryRow {
                                id: ids.id(ui, item.uri()),
                                name: item.name(),
                                kind: &kind,
                                cover,
                                selected: current == Some(item.uri()),
                                lifted: lifted.contains(&true_row),
                                edit,
                            };
                            let drawn = one(
                                ui,
                                place,
                                row,
                                draw.tokens,
                                if edit { false } else { draw.drag },
                                menu,
                                edit.then_some(&mut draw.renaming),
                            );
                            if draw.renaming.wants_return()
                                && draw.renaming.surface() == Surface::Row
                                && draw.renaming.uri() == item.uri()
                            {
                                drawn.response.request_focus();
                                draw.renaming.return_taken();
                            }
                            if drawn.clicked {
                                pressed = Some(item.uri().to_owned());
                            }
                            if let Some(door) = drawn.door {
                                opened = Some((item.menu_item(ids.shelf), door, item.renamable()));
                            }
                            // Only the playlists are a column a drag can lift;
                            // every other kind draws exactly as it always did.
                            if drawn.lift && ids.lifts && draw.lift && !narrowed {
                                let ghost = queue_row::Lifted::new(
                                    item.name().to_owned(),
                                    kind.clone(),
                                    None,
                                    draw.art.look(item.cover(), draw.now),
                                );
                                lifted_carry = Some(Carry {
                                    source: Source::Playlist,
                                    rows: vec![true_row],
                                    entries: Vec::new(),
                                    playlist: Some(item.uri().to_owned()),
                                    size: place.size(),
                                    ghost: Ghost::Queue(ghost),
                                });
                            }
                        }
                    });
                },
            );
            if !edit_drawn
                && draw.renaming.is_open()
                && draw.renaming.surface() == Surface::Row
                && !draw.renaming.writing()
            {
                draw.renaming.ask_focus();
            }
            if let Some(carry) = lifted_carry {
                carry::lift(ui.ctx(), carry);
            }
            if let Some((item, door, renamable)) = opened {
                open_menu(
                    ui.ctx(),
                    item,
                    door,
                    LibraryRights {
                        renamable: draw.renamable && renamable,
                        removable: draw.removable,
                        // A sidebar row is listed, so its own menu never offers a
                        // save: there is nothing to put back.
                        saved: Some(true),
                    },
                    draw.here.clone(),
                );
            }
            if ids.lifts
                && draw.offer
                && let Some((first, place)) = origin
            {
                let pitch = ROW_H + space::PX;
                // The rows box before the scroll moved it, so the offer describes
                // the same rows the painter drew.
                let list = Rect::from_min_size(
                    Pos2::new(place.left(), place.top() - first as f32 * pitch),
                    Vec2::new(place.width(), entries.len() as f32 * pitch),
                );
                // A slot's number is the playlist's index only while the drawn
                // set is the whole list, so the map is carried exactly when it
                // is not.
                let rows = narrowed.then_some(positions);
                carry::offer_column(
                    ui.ctx(),
                    ColumnOffer {
                        rows,
                        zone: Zone {
                            hit: rect,
                            list,
                            row_h: pitch,
                            first: 0,
                            count: entries.len(),
                        },
                        drawn: ROW_H,
                    },
                );
            }
            pressed.map(Action::Open)
        }
    }
}

/// The three things a list needs from what it lists.
trait ListItem {
    fn uri(&self) -> &str;
    fn name(&self) -> &str;
    /// What the row says under the name: the kind and who made it, or the kind
    /// alone when the wire named nobody.
    fn kind(&self) -> String;
    fn cover(&self) -> Option<&CoverUrl>;
    /// The row as a menu opened on it describes it.
    fn menu_item(&self, kind: SidebarKind) -> LibraryItem {
        LibraryItem {
            kind,
            uri: self.uri().to_owned(),
            name: self.name().to_owned(),
        }
    }
    /// Whether the listing says the user may rewrite this row's name.
    fn renamable(&self) -> bool {
        false
    }
}

impl ListItem for PlaylistEntry {
    fn uri(&self) -> &str {
        &self.uri
    }

    fn renamable(&self) -> bool {
        self.access.as_ref().is_some_and(|access| access.editable)
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn kind(&self) -> String {
        library_kind("Playlist", playlist_detail(self).as_deref())
    }

    fn cover(&self) -> Option<&CoverUrl> {
        self.cover_url()
    }
}

impl ListItem for FollowedShow {
    fn uri(&self) -> &str {
        &self.uri
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn kind(&self) -> String {
        library_kind("Podcast", self.publisher.as_deref())
    }

    fn cover(&self) -> Option<&CoverUrl> {
        self.cover.as_ref()
    }
}

impl ListItem for FollowedArtist {
    fn uri(&self) -> &str {
        &self.uri
    }

    fn name(&self) -> &str {
        &self.name
    }

    /// An artist's line is its kind alone: the reference puts a listener count
    /// there, and this client's wire does not carry one.
    fn kind(&self) -> String {
        "Artist".to_owned()
    }

    fn cover(&self) -> Option<&CoverUrl> {
        self.portrait.as_ref()
    }
}

impl ListItem for SavedAlbum {
    fn uri(&self) -> &str {
        &self.uri
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn kind(&self) -> String {
        library_kind(
            "Album",
            (!self.artists.is_empty()).then_some(self.artists.as_str()),
        )
    }

    fn cover(&self) -> Option<&CoverUrl> {
        self.cover.as_ref()
    }
}

/// A page kind, for asking which uri the open page names.
#[derive(Debug, Clone, Copy)]
enum PageKind {
    Show,
    Artist,
    Album,
}

/// The uri of the open page, when it is a page of `kind`.
///
/// The rows use it to draw themselves selected: `Library::selected` is about
/// playlists and is `None` on any of these pages.
fn page_uris(library: &Library, kind: PageKind) -> Option<&str> {
    match (library.page(), kind) {
        (Some(Page::Show(uri)), PageKind::Show) => Some(uri),
        (Some(Page::Artist(uri)), PageKind::Artist) => Some(uri),
        (Some(Page::Album(uri)), PageKind::Album) => Some(uri),
        _ => None,
    }
}

/// The list the chosen kind shows, or the state of the load that fetches it.
#[expect(
    clippy::too_many_arguments,
    reason = "the frame's own facts about one list, plus what the query left"
)]
fn rows(
    ui: &mut Ui,
    rect: Rect,
    library: &mut Library,
    art: &mut Art,
    now: Instant,
    tokens: &Tokens,
    drag: bool,
    kept: Option<&[usize]>,
    free: bool,
) -> Asked {
    if rect.height() < ROW_H {
        return Asked::default();
    }
    // The drawer draws the same column with no drag and no menu; a row field
    // there has no row to return to.
    if !drag {
        library.dismiss_row_rename();
    }

    // The failure line sits at the foot of every list, and the rows make room
    // for it rather than drawing over it.
    let notice = library.sidebar_notice.map(sidebar_notice);
    let line = egui::FontId::new(text::XXS, fonts::mono(MonoWeight::Regular));
    let foot = notice.map(|_| ui.fonts_mut(|fonts| fonts.row_height(&line)));
    let (rect, foot) = match foot {
        Some(height) => (
            Rect::from_min_max(
                rect.min,
                Pos2::new(rect.right(), (rect.bottom() - height).max(rect.top())),
            ),
            Some(Rect::from_min_max(
                Pos2::new(rect.left(), (rect.bottom() - height).max(rect.top())),
                rect.max,
            )),
        ),
        None => (rect, None),
    };

    let chosen = library.sidebar_kind.chosen();
    let renamable =
        drag && !library.write_in_flight() && matches!(library.playlists, Load::Ready(_));
    let renaming = std::mem::take(&mut library.renaming);
    let mut draw = Draw {
        art,
        now,
        tokens,
        menu: drag,
        removable: drag && library.can_remove(chosen),
        renamable,
        here: library.page().cloned(),
        drag,
        // A narrowed row's lift could only start a reorder, and a narrowed
        // column refuses every reorder: the rail already withholds a lift when
        // the order is not its to change, and this is the same vocabulary. A
        // query that matches every row narrows nothing, so its lift is kept.
        lift: drag && library.rootlist_movable(),
        offer: drag,
        reveal: std::mem::take(&mut library.reveal_top),
        renaming,
        kept,
        query: library.rail_filter.query().to_owned(),
    };
    let (action, open) = match library.sidebar_kind.chosen() {
        SidebarKind::Playlists => (
            list_rows(
                ui,
                rect,
                playlists_view(library),
                library.selected(),
                Ids {
                    scope: "library-list",
                    kind: "library-row",
                    shelf: SidebarKind::Playlists,
                    lifts: true,
                },
                Words {
                    loading: "Loading your playlists…",
                    empty: "No playlists yet.",
                },
                &mut draw,
            ),
            Open::Playlist,
        ),
        SidebarKind::Podcasts => (
            list_rows(
                ui,
                rect,
                shows_view(library),
                page_uris(library, PageKind::Show),
                Ids {
                    scope: "library-shows",
                    kind: "library-show",
                    shelf: SidebarKind::Podcasts,
                    lifts: false,
                },
                Words {
                    loading: "Loading your shows…",
                    empty: "No shows followed yet.",
                },
                &mut draw,
            ),
            Open::Page(Page::Show),
        ),
        SidebarKind::Artists => (
            list_rows(
                ui,
                rect,
                artists_view(library),
                page_uris(library, PageKind::Artist),
                Ids {
                    scope: "library-artists",
                    kind: "library-artist",
                    shelf: SidebarKind::Artists,
                    lifts: false,
                },
                Words {
                    loading: "Loading your artists…",
                    empty: "No artists followed yet.",
                },
                &mut draw,
            ),
            Open::Page(Page::Artist),
        ),
        SidebarKind::Albums => (
            list_rows(
                ui,
                rect,
                albums_view(library),
                page_uris(library, PageKind::Album),
                Ids {
                    scope: "library-albums",
                    kind: "library-album",
                    shelf: SidebarKind::Albums,
                    lifts: false,
                },
                Words {
                    loading: "Loading your albums…",
                    empty: "No albums saved yet.",
                },
                &mut draw,
            ),
            Open::Page(Page::Album),
        ),
    };

    library.renaming = draw.renaming;
    if let (Some(foot), Some(notice)) = (foot, notice) {
        ui.painter().text(
            Pos2::new(foot.left(), foot.center().y),
            egui::Align2::LEFT_CENTER,
            notice,
            line,
            tokens.palette.text_subtle,
        );
    }

    let picked = free && matches!(action, Some(Action::Open(_)));
    let command = match action {
        Some(Action::Open(uri)) if free => open.go(library, &uri),
        Some(Action::Retry(retry)) if free => retry.go(library),
        _ => None,
    };
    Asked { command, picked }
}

/// The row's click target and the chrome all four kinds of row share: a selected
/// row borrows the reference's language for the current navigation item — a
/// raised fill and a bone rule at the left edge — because the reference defines
/// no selected row.
fn row_chrome(
    ui: &mut Ui,
    rect: Rect,
    id: egui::Id,
    label: &str,
    look: Look,
    tokens: &Tokens,
    drag: bool,
) -> egui::Response {
    let sense = if drag {
        Sense::click_and_drag()
    } else {
        Sense::click()
    };
    let selected = look.selected;
    let response = ui.interact(rect, id, sense);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
    let _ = ui
        .ctx()
        .accesskit_node_builder(response.id, |node| node.set_selected(selected));

    if response.clicked() {
        // The keyboard follows the pointer: Tab continues from the row that was
        // picked.
        response.request_focus();
    }

    if selected {
        ui.painter()
            .rect_filled(rect, radius::SM, tokens.palette.surface_raised);
        ui.painter().rect_filled(
            Rect::from_min_size(rect.min, Vec2::new(stroke::RULE, rect.height())),
            radius::SM,
            tokens.palette.text,
        );
    } else if response.hovered() && !look.carrying {
        // The only fill in the column during a drag is the target's.
        ui.painter()
            .rect_filled(rect, radius::SM, tokens.palette.row_hover);
    }

    controls::focus_ring(ui, &response, rect, radius::SM, tokens);

    response
}

/// What one library row draws, whatever its kind.
struct LibraryRow<'a> {
    id: egui::Id,
    name: &'a str,
    kind: &'a str,
    cover: Cover,
    selected: bool,
    /// Whether the carried block names this row.
    lifted: bool,
    /// Whether this row's name is the open rename field.
    edit: bool,
}

/// What drawing one row reported.
struct Drawn {
    clicked: bool,
    /// Whether this frame began a drag out of the row.
    lift: bool,
    /// The door a right-click or the context-menu key opened, if one did.
    door: Option<Door>,
    response: egui::Response,
}

/// One library row: a placeholder tile, the name and the kind.
///
/// The tile stands in until covers are fetched.
/// The rect a row's rename field takes: both text lines, so the field's own
/// row height and caret fit instead of being clipped to the name line.
fn rename_field(name_x: f32, top: f32, width: f32, name_h: f32, kind_h: f32) -> Rect {
    Rect::from_min_size(Pos2::new(name_x, top), Vec2::new(width, name_h + kind_h))
}

fn one(
    ui: &mut Ui,
    rect: Rect,
    row: LibraryRow<'_>,
    tokens: &Tokens,
    drag: bool,
    menu: bool,
    renaming: Option<&mut Renaming>,
) -> Drawn {
    let LibraryRow {
        id,
        name,
        kind,
        cover,
        selected,
        lifted,
        edit,
    } = row;
    let look = Look {
        selected,
        lifted,
        carrying: carry::carrying(ui.ctx()),
    };
    let before = rows::dim(ui, look);
    let response = row_chrome(ui, rect, id, name, look, tokens, drag && !edit);
    let door = menu
        .then(|| pointer_door(ui, rect, id).or_else(|| key_door(ui, &response, rect)))
        .flatten();
    if menu {
        let open = context_menu::open_opener(ui.ctx()) == Some(id);
        let _ = ui.ctx().accesskit_node_builder(id, |node| {
            node.set_has_popup(egui::accesskit::HasPopup::Menu);
            node.set_expanded(open);
        });
    }

    let tile = Rect::from_min_size(
        Pos2::new(
            rect.left() + space::S2,
            rect.center().y - extent::LIBRARY_ART / 2.0,
        ),
        Vec2::splat(extent::LIBRARY_ART),
    );
    art::tile(
        ui.painter(),
        tile,
        radius::SM,
        tokens.palette.border,
        cover,
        tokens,
    );

    let name_x = tile.right() + space::S3;
    let name_width = (rect.right() - name_x - space::S2).max(0.0);
    let name_line = recipe::line(
        ui,
        name,
        egui::FontId::new(text::SM, fonts::sans(Weight::Medium)),
        tokens.palette.text,
        0.0,
        name_width,
    );
    let kind_line = recipe::line(
        ui,
        kind,
        egui::FontId::new(text::XXS, fonts::mono(MonoWeight::Regular)),
        tokens.palette.text_subtle,
        recipe::tracking_pts(text::XXS, tracking::SLIGHT),
        name_width,
    );
    let top = rect.center().y - (name_line.size().y + kind_line.size().y) / 2.0;
    let field = rename_field(
        name_x,
        top,
        name_width,
        name_line.size().y,
        kind_line.size().y,
    );
    if edit {
        ui.painter()
            .rect_filled(field, radius::SM, tokens.palette.surface_raised);
        if let Some(renaming) = renaming {
            let writing = renaming.writing();
            let wants_focus = renaming.wants_focus();
            let id = id.with("rename");
            let out = {
                let text = renaming.edit();
                name_field::edit(
                    ui,
                    name_field::Spec {
                        id,
                        rect: field,
                        text,
                        writing,
                        wants_focus,
                        font: egui::FontId::new(text::SM, fonts::sans(Weight::Medium)),
                        hint: "",
                        inset: egui::Margin {
                            left: space::S1 as i8,
                            right: space::S1 as i8,
                            top: 0,
                            bottom: 0,
                        },
                        text_color: tokens.palette.text,
                        weak_color: tokens.palette.text_subtle,
                        cursor: Stroke::new(stroke::HAIRLINE, tokens.palette.text),
                        align: egui::Align2::LEFT_CENTER,
                        limit: PLAYLIST_NAME_MAX,
                    },
                )
            };
            if out.focused {
                renaming.focus_taken();
            }
            if out.lost_focus && renaming.text().trim().is_empty() {
                renaming.dismiss();
            }
            if out.enter {
                name_field::post_submit(ui.ctx());
            }
            if out.escape {
                renaming.cancel();
            }
            let border = if out.focused {
                tokens.palette.text
            } else {
                tokens.palette.border
            };
            ui.painter().rect_stroke(
                field,
                radius::SM,
                Stroke::new(stroke::HAIRLINE, border),
                StrokeKind::Inside,
            );
        } else {
            ui.painter().galley(
                Pos2::new(name_x, top),
                name_line.clone(),
                tokens.palette.text,
            );
        }
    } else {
        ui.painter().galley(
            Pos2::new(name_x, top),
            name_line.clone(),
            tokens.palette.text,
        );
    }
    // The field covers the kind line, so drawing it would show through the box.
    if !edit {
        ui.painter().galley(
            Pos2::new(name_x, top + name_line.size().y),
            kind_line,
            tokens.palette.text_subtle,
        );
    }
    ui.set_opacity(before);

    // egui reports a focused row's Enter or Space as a click, repeats too, so a
    // held key would open the playlist once per frame. Only a fresh press is the
    // user's, as it is for the band's title.
    let clicked = response.clicked();
    let pointer = ui.input(|input| input.pointer.button_clicked(egui::PointerButton::Primary));
    let held = clicked && !pointer && context_menu::held_repeat(ui);
    Drawn {
        clicked: clicked && !held,
        lift: !edit && response.drag_started_by(egui::PointerButton::Primary),
        door,
        response,
    }
}

/// The pointer's door: a right-click anywhere on the row.
fn pointer_door(ui: &Ui, rect: Rect, id: egui::Id) -> Option<Door> {
    context_menu::pointer_door(ui, rect, id, rect)
}

/// The keyboard's door: the context-menu key or Shift+F10 on the focused row.
///
/// The row is the opener the keyboard goes back to, because a row opened this
/// way has no button of its own.
fn key_door(ui: &Ui, response: &egui::Response, rect: Rect) -> Option<Door> {
    if !response.has_focus() {
        return None;
    }
    context_menu::menu_key_pressed(ui).then_some(Door {
        anchor: crate::menu::Anchor::Button(rect),
        opener: response.id,
        device: crate::menu::Device::Keyboard,
    })
}

/// Posts the menu a sidebar row's door opened.
///
/// A library row is one thing rather than a pick, so the request carries no
/// targets and no rows: the listing names the row.
fn open_menu(
    ctx: &egui::Context,
    item: LibraryItem,
    door: Door,
    rights: LibraryRights,
    page: Option<Page>,
) {
    context_menu::post(
        ctx,
        verbs::library_request(item, door, rights, page, crate::naming::Surface::Row),
    );
}

/// A line of text where a list would be, for the empty and failed states.
fn state(ui: &mut Ui, rect: Rect, text: RichText) {
    ui.scope_builder(
        UiBuilder::new()
            .id_salt("library-state")
            .max_rect(rect)
            .layout(Layout::top_down(egui::Align::Center)),
        |ui| {
            ui.add_space(rect.height() / 4.0);
            ui.label(text);
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Color32, Event, Pos2, Vec2};
    use takofy_model::{FollowedShow, Owner, OwnerName, PlaylistEntry};

    use crate::menu::{Anchor, Device, LibraryItem, Listing};
    use crate::message::{Failure, Update};
    use crate::theme::{fonts, style};
    use crate::view::context_menu;
    use crate::view::test_support::{click_events, rings_in};

    /// The chip rect and label rect the real sidebar paints, per label.
    ///
    /// A chip's label is painted at the chip's top-left plus its padding, so
    /// this recovers both boxes from what the draw actually produced.
    fn painted_chips(width: f32) -> Vec<(&'static str, Rect)> {
        let tokens = Tokens::DARK;
        let ctx = egui::Context::default();
        ctx.set_fonts(fonts::definitions());
        ctx.set_global_style(style(&tokens));
        let mut library = Library::default();
        library.apply(&Update::Playlists(vec![PlaylistEntry {
            composed: Vec::new(),
            uri: "spotify:playlist:a".to_owned(),
            name: "A".to_owned(),
            named: true,
            cover: None,
            description: None,
            owner: None,
            tracks: None,
            access: None,
        }]));
        let mut art = Art::default();
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(width, 800.0))),
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| {
            let _ = show(ui, &mut library, &mut art, Instant::now(), &tokens, true).command;
        });
        output.textures_delta.clear();

        let mut chips = Vec::new();
        for label in ["PLAYLISTS", "ARTISTS", "ALBUMS", "PODCASTS"] {
            let Some((pos, size)) = output
                .shapes
                .iter()
                .find_map(|clipped| match &clipped.shape {
                    egui::epaint::Shape::Text(text) if text.galley.text() == label => {
                        Some((text.pos, text.galley.size()))
                    }
                    _ => None,
                })
            else {
                continue;
            };
            let rect = Rect::from_min_size(
                Pos2::new(pos.x - extent::CHIP_PAD_X, pos.y - extent::CHIP_PAD_Y),
                Vec2::new(
                    size.x + 2.0 * extent::CHIP_PAD_X,
                    size.y + 2.0 * extent::CHIP_PAD_Y,
                ),
            );
            let label: &'static str = match label {
                "PLAYLISTS" => "Playlists",
                "ARTISTS" => "Artists",
                "ALBUMS" => "Albums",
                _ => "Podcasts",
            };
            chips.push((label, rect));
        }
        chips
    }

    /// The four filters' labels, and where their chips are centred.
    ///
    /// The rect is rebuilt from the painted glyps, so it is the label's own box
    /// rather than the cell: a chip's label is centred in its cell, which is
    /// what this pins.
    #[test]
    fn the_drawn_chips_keep_their_characterised_positions() {
        // The sidebar is a fixed 256 points, in the column and in the drawer, so
        // these are the two widths the grid is ever solved at: the panel less its
        // hairline, and the panel itself.
        for width in [254.0, 256.0] {
            let chips = painted_chips(width);
            assert_eq!(chips.len(), sidebar_kind::ROW.len(), "band width {width}");

            let cell = (width - 2.0 * space::S4 - space::S1) / 2.0;
            for (index, (label, rect)) in chips.iter().enumerate() {
                assert_eq!(label, &sidebar_kind_label(sidebar_kind::ROW[index]));
                let column = index % CHIP_COLUMNS;
                let row = index / CHIP_COLUMNS;
                let centre_x = space::S4 + column as f32 * (cell + space::S1) + cell / 2.0;
                let centre_y =
                    space::S4 + row as f32 * (extent::BUTTON + space::S1) + extent::BUTTON / 2.0;
                assert!(
                    (rect.center().x - centre_x).abs() < 0.01
                        && (rect.center().y - centre_y).abs() < 0.01,
                    "band width {width}: {label} centred at {:?}, not ({centre_x}, {centre_y})",
                    rect.center()
                );
            }
        }
    }

    #[test]
    fn the_four_filters_sit_in_two_equal_columns() {
        let chips = painted_chips(254.0);
        assert_eq!(chips.len(), 4);
        let cell = (254.0 - 2.0 * space::S4 - space::S1) / 2.0;
        // Two chips share a row, and the columns are the same width, so the
        // block reads as a grid rather than as four buttons that happened to fit.
        assert!((chips[0].1.center().y - chips[1].1.center().y).abs() < 0.01);
        assert!((chips[2].1.center().y - chips[3].1.center().y).abs() < 0.01);
        assert!(chips[2].1.center().y > chips[0].1.center().y);
        for pair in [(0, 1), (2, 3)] {
            let gap = chips[pair.1].1.center().x - chips[pair.0].1.center().x;
            assert!(
                (gap - (cell + space::S1)).abs() < 0.01,
                "columns are one cell apart"
            );
        }
    }

    /// One show, the way the followed list carries it.
    fn followed_show(uri: &str, name: &str) -> FollowedShow {
        FollowedShow {
            uri: uri.to_owned(),
            name: name.to_owned(),
            named: true,
            publisher: None,
            cover: None,
            episodes: None,
        }
    }

    #[test]
    fn a_failed_playlist_load_wins_over_a_ready_followed_list() {
        let mut library = Library::default();
        library.playlists = Load::Failed(Failure::Load);
        library.followed = Load::Ready(vec![followed_show("spotify:show:a", "A")]);

        assert_eq!(
            shows_view(&library),
            ListView::Failed(Failure::Load, Retry::Playlists)
        );
    }

    #[test]
    fn an_unstarted_or_inflight_playlist_load_is_loading() {
        for playlists in [Load::Idle, Load::Loading] {
            let load = format!("{playlists:?}");
            let mut library = Library::default();
            library.playlists = playlists;
            assert_eq!(shows_view(&library), ListView::Loading, "{load}");
        }
    }

    #[test]
    fn a_reload_does_not_flash_the_empty_state() {
        // The retry reset the followed list to Idle while the playlists load.
        let mut library = Library::default();
        library.playlists = Load::Loading;
        library.followed = Load::Idle;
        assert_eq!(shows_view(&library), ListView::Loading);

        // One beat later the playlists have landed, but the list has not been
        // asked for yet.
        let mut library = Library::default();
        library.playlists = Load::Ready(Vec::new());
        library.followed = Load::Idle;
        assert_eq!(shows_view(&library), ListView::Loading);
    }

    #[test]
    fn an_inflight_followed_list_is_loading() {
        let mut library = Library::default();
        library.playlists = Load::Ready(Vec::new());
        library.followed = Load::Loading;
        assert_eq!(shows_view(&library), ListView::Loading);
    }

    #[test]
    fn a_failed_followed_list_carries_its_own_retry() {
        let mut library = Library::default();
        library.playlists = Load::Ready(Vec::new());
        library.followed = Load::Failed(Failure::Followed);

        assert_eq!(
            shows_view(&library),
            ListView::Failed(Failure::Followed, Retry::Followed)
        );
    }

    #[test]
    fn a_ready_followed_list_is_empty_when_it_follows_nothing() {
        let mut library = Library::default();
        library.playlists = Load::Ready(Vec::new());
        library.followed = Load::Ready(Vec::new());
        assert_eq!(shows_view(&library), ListView::Empty);
    }

    #[test]
    fn a_ready_followed_list_is_the_shows_to_draw() {
        let shows = vec![
            followed_show("spotify:show:a", "A"),
            followed_show("spotify:show:b", "B"),
        ];
        let mut library = Library::default();
        library.playlists = Load::Ready(Vec::new());
        library.followed = Load::Ready(shows.clone());

        assert_eq!(shows_view(&library), ListView::List(shows.as_slice()));
    }

    const PLAYLIST_IDS: Ids = Ids {
        scope: "library-list",
        kind: "library-row",
        shelf: SidebarKind::Playlists,
        lifts: true,
    };
    const SHOW_IDS: Ids = Ids {
        scope: "library-shows",
        kind: "library-show",
        shelf: SidebarKind::Podcasts,
        lifts: false,
    };

    fn followed_artist(uri: &str, name: &str) -> FollowedArtist {
        FollowedArtist {
            uri: uri.to_owned(),
            name: name.to_owned(),
            named: true,
            portrait: None,
            releases: None,
        }
    }

    fn saved_album(uri: &str, name: &str, artists: &str) -> SavedAlbum {
        SavedAlbum {
            uri: uri.to_owned(),
            name: name.to_owned(),
            named: true,
            artists: artists.to_owned(),
            year: None,
            cover: None,
            tracks: None,
        }
    }

    #[test]
    fn a_failed_artists_or_albums_load_carries_its_own_retry() {
        let mut artists = Library::default();
        artists.playlists = Load::Ready(Vec::new());
        artists.followed_artists = Load::Failed(Failure::FollowedArtists);
        assert_eq!(
            artists_view(&artists),
            ListView::Failed(Failure::FollowedArtists, Retry::FollowedArtists)
        );

        let mut albums = Library::default();
        albums.playlists = Load::Ready(Vec::new());
        albums.saved_albums = Load::Failed(Failure::SavedAlbums);
        assert_eq!(
            albums_view(&albums),
            ListView::Failed(Failure::SavedAlbums, Retry::SavedAlbums)
        );
    }

    #[test]
    fn an_unasked_artists_or_albums_list_is_loading_not_empty() {
        // Idle is "nobody has asked yet", which reads as loading: the empty
        // copy would claim the library holds nothing.
        let mut library = Library::default();
        library.playlists = Load::Ready(Vec::new());
        assert_eq!(artists_view(&library), ListView::Loading);
        assert_eq!(albums_view(&library), ListView::Loading);

        library.followed_artists = Load::Ready(Vec::new());
        library.saved_albums = Load::Ready(Vec::new());
        assert_eq!(artists_view(&library), ListView::Empty);
        assert_eq!(albums_view(&library), ListView::Empty);
    }

    #[test]
    fn the_size_counts_the_two_new_lists() {
        let mut library = Library::default();
        library.playlists = Load::Ready(vec![PlaylistEntry {
            composed: Vec::new(),
            uri: "spotify:playlist:a".to_owned(),
            name: "A".to_owned(),
            named: true,
            cover: None,
            description: None,
            owner: None,
            tracks: None,
            access: None,
        }]);
        library.followed = Load::Ready(vec![followed_show("spotify:show:a", "A")]);
        library.followed_artists = Load::Ready(vec![
            followed_artist("spotify:artist:a", "A"),
            followed_artist("spotify:artist:b", "B"),
        ]);
        library.saved_albums = Load::Ready(vec![saved_album("spotify:album:a", "A", "X")]);

        assert_eq!(counts(&library), (1, 5), "one shown of five saved");

        library.sidebar_kind.choose(SidebarKind::Artists);
        assert_eq!(counts(&library), (2, 5));
        library.sidebar_kind.choose(SidebarKind::Albums);
        assert_eq!(counts(&library), (1, 5));
    }

    #[test]
    fn an_artist_row_says_its_kind_and_an_album_row_names_its_artist() {
        assert_eq!(followed_artist("spotify:artist:a", "A").kind(), "Artist");
        assert_eq!(
            saved_album("spotify:album:a", "A", "Vela Moreno").kind(),
            "Album · Vela Moreno"
        );
        assert_eq!(
            saved_album("spotify:album:a", "A", "").kind(),
            "Album",
            "an album the wire named no artist for says only its kind"
        );
    }

    /// The context the sidebar draws into, the way the window builds it.
    fn context() -> egui::Context {
        let ctx = egui::Context::default();
        ctx.set_fonts(fonts::definitions());
        ctx.set_global_style(style(&Tokens::DARK));
        ctx
    }

    /// One frame of the full sidebar over the characterised band: `events` go
    /// into the pass, and the frame's output and command come back.
    fn run(
        ctx: &egui::Context,
        events: Vec<Event>,
        library: &mut Library,
        art: &mut Art,
    ) -> (egui::FullOutput, Option<Command>) {
        run_gated(ctx, events, library, art, true)
    }

    /// The same frame with the drawer's own room for a command: `free` false is
    /// what the shell passes when an earlier region has already taken the frame.
    fn run_gated(
        ctx: &egui::Context,
        events: Vec<Event>,
        library: &mut Library,
        art: &mut Art,
        free: bool,
    ) -> (egui::FullOutput, Option<Command>) {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(254.0, 800.0))),
            events,
            ..Default::default()
        };
        let tokens = Tokens::DARK;
        let mut command = None;
        let mut output = ctx.run_ui(input, |ui| {
            crate::view::focus::begin_frame(ui.ctx());
            if let Some(next) = show(ui, library, art, Instant::now(), &tokens, free).command {
                command = Some(next);
            }
        });
        output.textures_delta.clear();
        (output, command)
    }

    /// The three passes of one click at `pos`, one pass each; the last command
    /// any of them returned.
    fn click(
        ctx: &egui::Context,
        pos: Pos2,
        library: &mut Library,
        art: &mut Art,
    ) -> Option<Command> {
        let mut command = None;
        for event in click_events(pos) {
            let (_, next) = run(ctx, vec![event], library, art);
            if next.is_some() {
                command = next;
            }
        }
        command
    }

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

    /// The playlist list's one row, ready to draw.
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

    /// Where a label was painted, and how big it was.
    fn painted_text(output: &egui::FullOutput, label: &str) -> (Pos2, Vec2) {
        output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Text(text) if text.galley.text() == label => {
                    Some((text.pos, text.galley.size()))
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("{label:?} is drawn"))
    }

    /// One press of `key`, fresh or as a held key's auto-repeat.
    ///
    /// egui decides the repeat against the pass before it, so `repeat: true` only
    /// says "held" once the same key's press came in an earlier pass with no
    /// release between them.
    fn press(key: egui::Key, repeat: bool) -> Event {
        Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat,
            modifiers: egui::Modifiers::NONE,
        }
    }

    /// The rail's box on the frame's screen.
    fn rail() -> Rect {
        Rect::from_min_size(Pos2::ZERO, Vec2::new(254.0, 800.0))
    }

    #[test]
    fn a_clicked_rail_row_wears_no_ring_and_keeps_the_keyboard() {
        let ctx = context();
        let mut library = movable_playlists(&["A", "B"], 7, true, None);
        let mut art = Art::default();
        let (output, _) = run(&ctx, Vec::new(), &mut library, &mut art);
        let at = name_line(&output, "A");
        let _ = click(&ctx, at, &mut library, &mut art);

        let (output, _) = run(&ctx, Vec::new(), &mut library, &mut art);
        let focused = ctx
            .memory(|memory| memory.focused())
            .expect("the click took the keyboard");
        let row = ctx
            .read_response(focused)
            .expect("the row was laid out")
            .rect;
        assert!(
            row.contains(at),
            "the keyboard went to another row: {row:?}"
        );
        assert!(
            rings_in(&output).is_empty(),
            "a clicked rail row wears no ring: {:?}",
            rings_in(&output)
        );
    }

    #[test]
    fn tab_rings_the_rail_row_inside_it() {
        let ctx = context();
        let mut library = movable_playlists(&["A", "B"], 7, true, None);
        let mut art = Art::default();
        let (output, _) = run(&ctx, Vec::new(), &mut library, &mut art);
        let at = name_line(&output, "A");
        let _ = click(&ctx, at, &mut library, &mut art);

        let _ = run(
            &ctx,
            vec![press(egui::Key::Tab, false)],
            &mut library,
            &mut art,
        );
        let (output, _) = run(&ctx, Vec::new(), &mut library, &mut art);

        let focused = ctx
            .memory(|memory| memory.focused())
            .expect("Tab put the keyboard somewhere");
        let row = ctx
            .read_response(focused)
            .expect("the row was laid out")
            .rect;
        let rings = rings_in(&output);
        assert_eq!(rings.len(), 1, "Tab rings one rail row: {rings:?}");
        assert_eq!(rings[0], row, "the ring lies on the row's own edge");
        assert!(
            rail().contains_rect(row),
            "the ringed row left the rail: {row:?}"
        );
    }

    #[test]
    fn a_held_enter_on_the_focused_row_does_not_open_it_again() {
        let ctx = context();
        let mut library = movable_playlists(&["A", "B"], 7, true, None);
        let mut art = Art::default();
        let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);
        let at = name_line(&output, "A");
        let _ = click(&ctx, at, &mut library, &mut art);
        assert!(
            ctx.memory(|memory| memory.focused()).is_some(),
            "the row took no keyboard to hold Enter on"
        );

        // The first press is the user's, and opens the row once.
        let (_, fresh) = run_column(
            &ctx,
            vec![press(egui::Key::Enter, false)],
            &mut library,
            &mut art,
        );
        assert!(fresh.is_some(), "a first press of Enter opened nothing");

        // The repeats of the same held key are not presses of the user's.
        let (_, held) = run_column(
            &ctx,
            vec![press(egui::Key::Enter, true)],
            &mut library,
            &mut art,
        );
        assert_eq!(held, None, "a held Enter opened the row it just opened");
    }

    #[test]
    fn a_playlist_row_click_loads_its_tracks() {
        let ctx = context();
        let mut library = Library::default();
        library.apply(&Update::Playlists(vec![playlist(
            "spotify:playlist:a",
            "A list",
        )]));
        let mut art = Art::default();
        let (output, _) = run(&ctx, Vec::new(), &mut library, &mut art);

        let (name, name_size) = painted_text(&output, "A list");
        let (_, kind_size) = painted_text(&output, "Playlist");
        let command = click(
            &ctx,
            Pos2::new(name.x + 10.0, name.y + (name_size.y + kind_size.y) / 2.0),
            &mut library,
            &mut art,
        );

        assert_eq!(
            command,
            Some(Command::LoadTracks {
                playlist: "spotify:playlist:a".to_owned()
            })
        );
        assert_eq!(library.selected(), Some("spotify:playlist:a"));
    }

    /// The drawer draws after every region, so a frame an earlier region already
    /// took has no room for the drawer's command. The row must be refused
    /// *before* it arms its load: a page marked loading with no request behind it
    /// never recovers, because `settle` sees the want and the in-flight read and
    /// refuses to ask again.
    #[test]
    fn a_row_pick_is_refused_when_the_frame_cannot_take_its_command() {
        let ctx = context();
        let mut library = Library::default();
        library.apply(&Update::Playlists(vec![playlist(
            "spotify:playlist:a",
            "A list",
        )]));
        let mut art = Art::default();
        let (output, _) = run_gated(&ctx, Vec::new(), &mut library, &mut art, false);

        let (name, name_size) = painted_text(&output, "A list");
        let (_, kind_size) = painted_text(&output, "Playlist");
        let at = Pos2::new(name.x + 10.0, name.y + (name_size.y + kind_size.y) / 2.0);
        let mut command = None;
        for event in click_events(at) {
            let (_, next) = run_gated(&ctx, vec![event], &mut library, &mut art, false);
            if next.is_some() {
                command = next;
            }
        }

        assert_eq!(command, None, "a command with no room was produced");
        assert_eq!(library.selected(), None, "the refused row still selected");
        assert_eq!(library.page(), None, "the refused row opened a page");
    }

    #[test]
    fn a_playlist_name_starts_past_the_art_slot() {
        let ctx = context();
        let tokens = Tokens::DARK;
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(254.0, 800.0))),
                ..Default::default()
            },
            |ui| {
                let rect = Rect::from_min_size(Pos2::new(20.0, 0.0), Vec2::new(200.0, ROW_H));
                let row = LibraryRow {
                    id: PLAYLIST_IDS.id(ui, "spotify:playlist:a"),
                    name: "A list",
                    kind: "Playlist",
                    cover: Cover::Missing,
                    selected: false,
                    lifted: false,
                    edit: false,
                };
                one(ui, rect, row, &tokens, false, false, None);
            },
        );
        output.textures_delta.clear();

        let (name, _) = painted_text(&output, "A list");
        let (kind, _) = painted_text(&output, "Playlist");
        let expected = 20.0 + space::S2 + extent::LIBRARY_ART + space::S3;
        assert!((name.x - expected).abs() < 0.01, "name at {}", name.x);
        assert!((kind.x - expected).abs() < 0.01, "kind at {}", kind.x);
    }

    #[test]
    fn a_playlist_row_announces_its_selection() {
        let ctx = context();
        ctx.enable_accesskit();
        let tokens = Tokens::DARK;
        let uri = "spotify:playlist:a";
        let mut announced = None;
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(254.0, 800.0))),
                ..Default::default()
            },
            |ui| {
                let id = PLAYLIST_IDS.id(ui, uri);
                let rect = Rect::from_min_size(Pos2::new(0.0, 0.0), Vec2::new(200.0, ROW_H));
                let row = LibraryRow {
                    id,
                    name: "A list",
                    kind: "Playlist",
                    cover: Cover::Missing,
                    selected: true,
                    lifted: false,
                    edit: false,
                };
                one(ui, rect, row, &tokens, false, false, None);
                announced = ui
                    .ctx()
                    .accesskit_node_builder(id, |node| node.is_selected())
                    .expect("the row has an AccessKit node");
            },
        );
        output.textures_delta.clear();

        assert_eq!(announced, Some(true), "a selected row says so");
    }

    /// One frame of the column form: its rows may lift and offer themselves.
    fn run_column(
        ctx: &egui::Context,
        events: Vec<Event>,
        library: &mut Library,
        art: &mut Art,
    ) -> (egui::FullOutput, Option<Command>) {
        run_with(ctx, events, library, art, true)
    }

    /// One frame of either form, over the characterised band.
    fn run_with(
        ctx: &egui::Context,
        events: Vec<Event>,
        library: &mut Library,
        art: &mut Art,
        drag: bool,
    ) -> (egui::FullOutput, Option<Command>) {
        let frame = frame_of(ctx, events, library, art, drag);
        (frame.output, frame.command)
    }

    /// What one frame of the rail leaves behind.
    struct Frame {
        output: egui::FullOutput,
        command: Option<Command>,
    }

    /// One frame of either form, with the head's ids read out.
    fn frame_of(
        ctx: &egui::Context,
        events: Vec<Event>,
        library: &mut Library,
        art: &mut Art,
        drag: bool,
    ) -> Frame {
        frame_probing(ctx, events, library, art, drag, |_| ()).0
    }

    /// One frame of either form, read inside the pass so its AccessKit nodes are
    /// still standing.
    fn frame_probing<T>(
        ctx: &egui::Context,
        events: Vec<Event>,
        library: &mut Library,
        art: &mut Art,
        drag: bool,
        probe: impl FnOnce(&egui::Ui) -> T,
    ) -> (Frame, T) {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(254.0, 800.0))),
            events,
            ..Default::default()
        };
        let tokens = Tokens::DARK;
        let mut command = None;
        let mut probed = None;
        let mut probe = Some(probe);
        let mut output = ctx.run_ui(input, |ui| {
            crate::view::focus::begin_frame(ui.ctx());
            let next = if drag {
                column(ui, library, art, Instant::now(), &tokens)
            } else {
                show(ui, library, art, Instant::now(), &tokens, true).command
            };
            if let Some(next) = next {
                command = Some(next);
            }
            if let Some(probe) = probe.take() {
                probed = Some(probe(ui));
            }
        });
        output.textures_delta.clear();
        (
            Frame { output, command },
            probed.expect("the probe runs once"),
        )
    }

    /// Drags from a row's line towards the middle of the list, stopping with the
    /// button still down, and returns the last frame.
    fn hold_from(
        ctx: &egui::Context,
        from: Pos2,
        library: &mut Library,
        art: &mut Art,
        drag: bool,
    ) -> egui::FullOutput {
        let mut events = crate::view::test_support::drag_events(from, from + Vec2::new(0.0, 60.0));
        events.pop();
        let _ = run_with(ctx, Vec::new(), library, art, drag);
        let mut last = None;
        for event in events {
            let (output, _) = run_with(ctx, vec![event], library, art, drag);
            last = Some(output);
        }
        last.expect("a drag moves the pointer at least once")
    }

    /// A library whose sidebar holds the named playlists, movable at `byte`.
    fn movable_playlists(
        names: &[&str],
        byte: u8,
        editable: bool,
        tracks: Option<usize>,
    ) -> Library {
        let mut library = Library::default();
        let entries = names
            .iter()
            .map(|name| {
                let mut entry = playlist(&format!("spotify:playlist:{name}"), name);
                entry.tracks = tracks;
                entry.access = Some(takofy_model::PlaylistAccess {
                    revision: takofy_model::Revision::from_wire(&[byte]).expect("a revision"),
                    editable,
                });
                entry
            })
            .collect();
        library.apply(&Update::Playlists(entries));
        library.apply(&Update::RootlistAccess(takofy_model::PlaylistAccess {
            revision: takofy_model::Revision::from_wire(&[byte]).expect("a revision"),
            editable,
        }));
        library
    }

    fn name_line(output: &egui::FullOutput, label: &str) -> Pos2 {
        let (pos, size) = painted_text(output, label);
        Pos2::new(pos.x + 10.0, pos.y + size.y / 2.0)
    }

    /// The top of the row painted as `label`, from the two lines it draws.
    ///
    /// A row centres its name and kind in itself, so the name's own top plus
    /// half the pair, less half a row, is the box's top.
    fn row_top(output: &egui::FullOutput, label: &str, kind: &str) -> f32 {
        let (name, name_size) = painted_text(output, label);
        let (_, kind_size) = painted_text(output, kind);
        name.y + (name_size.y + kind_size.y) / 2.0 - ROW_H / 2.0
    }

    #[test]
    fn a_drag_on_a_playlist_row_lifts_it_when_the_rootlist_is_movable() {
        let ctx = context();
        let mut library = movable_playlists(&["A", "B"], 7, true, None);
        let mut art = Art::default();
        let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);
        let from = name_line(&output, "B");

        let _ = hold_from(&ctx, from, &mut library, &mut art, true);

        let carry = carry::carried(&ctx).expect("a payload");
        assert_eq!(carry.source, Source::Playlist);
        assert_eq!(carry.rows, vec![1]);
        assert_eq!(carry.playlist.as_deref(), Some("spotify:playlist:B"));
        assert!(carry.entries.is_empty(), "a playlist carries no entries");
    }

    #[test]
    fn a_drag_does_not_lift_when_the_rootlist_is_not_movable() {
        // No access, not editable, and a write in flight all refuse the lift.
        let mut cases: Vec<(&str, Library)> = Vec::new();

        let mut no_access = Library::default();
        no_access.apply(&Update::Playlists(vec![
            playlist("spotify:playlist:A", "A"),
            playlist("spotify:playlist:B", "B"),
        ]));
        cases.push(("no access", no_access));
        cases.push((
            "not editable",
            movable_playlists(&["A", "B"], 7, false, None),
        ));

        let mut pending = movable_playlists(&["A", "B"], 7, true, None);
        assert!(pending.move_playlist(0, 2, "spotify:playlist:A").is_some());
        cases.push(("write in flight", pending));

        for (why, mut library) in cases {
            let ctx = context();
            let mut art = Art::default();
            let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);
            let from = name_line(&output, "B");

            let _ = hold_from(&ctx, from, &mut library, &mut art, true);

            assert!(carry::carried(&ctx).is_none(), "{why} must not lift");
        }
    }

    #[test]
    fn the_non_drag_form_never_lifts() {
        let ctx = context();
        let mut library = movable_playlists(&["A", "B"], 7, true, None);
        let mut art = Art::default();
        let (output, _) = run(&ctx, Vec::new(), &mut library, &mut art);
        let from = name_line(&output, "B");

        let mut events = crate::view::test_support::drag_events(from, from + Vec2::new(0.0, 60.0));
        events.pop();
        for event in events {
            let _ = run(&ctx, vec![event], &mut library, &mut art);
        }

        assert!(
            carry::carried(&ctx).is_none(),
            "the drawer's form discards its command, so it never starts a write"
        );
        assert_eq!(carry::take_column_offer(&ctx), None);
    }

    #[test]
    fn other_kinds_never_lift_or_offer() {
        for kind in [
            SidebarKind::Podcasts,
            SidebarKind::Artists,
            SidebarKind::Albums,
        ] {
            let ctx = context();
            let mut library = movable_playlists(&["A"], 7, true, None);
            library.apply(&Update::FollowedShows(vec![followed_show(
                "spotify:show:s",
                "A show",
            )]));
            library.apply(&Update::FollowedArtists(vec![followed_artist(
                "spotify:artist:a",
                "An artist",
            )]));
            library.apply(&Update::SavedAlbums(vec![saved_album(
                "spotify:album:a",
                "An album",
                "Vela",
            )]));
            assert!(library.sidebar_kind.choose(kind));
            let mut art = Art::default();
            let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);
            let label = match kind {
                SidebarKind::Podcasts => "A show",
                SidebarKind::Artists => "An artist",
                SidebarKind::Albums => "An album",
                SidebarKind::Playlists => unreachable!(),
            };
            let from = name_line(&output, label);

            let _ = hold_from(&ctx, from, &mut library, &mut art, true);

            assert!(carry::carried(&ctx).is_none(), "{kind:?} must not lift");
            assert_eq!(
                carry::take_column_offer(&ctx),
                None,
                "{kind:?} is no drop target"
            );
        }
    }

    #[test]
    fn a_click_still_opens_the_playlist() {
        let ctx = context();
        let mut library = movable_playlists(&["A", "B"], 7, true, None);
        let mut art = Art::default();
        let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);
        let at = name_line(&output, "A");

        let mut command = None;
        for event in click_events(at) {
            let (_, next) = run_column(&ctx, vec![event], &mut library, &mut art);
            if next.is_some() {
                command = next;
            }
        }

        assert_eq!(
            command,
            Some(Command::LoadTracks {
                playlist: "spotify:playlist:A".to_owned()
            })
        );
    }

    #[test]
    fn the_playlists_list_offers_its_rows_at_the_pitch_it_drew() {
        let ctx = context();
        let mut library = movable_playlists(&["A", "B", "C"], 7, true, None);
        let mut art = Art::default();
        let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);

        let offer = carry::take_column_offer(&ctx).expect("the column offers itself");

        assert_eq!(offer.drawn, ROW_H);
        assert_eq!(offer.zone.row_h, ROW_H + space::PX);
        assert_eq!(offer.zone.count, 3);
        assert_eq!(offer.zone.first, 0);

        let pitch = ROW_H + space::PX;
        for (row, label) in ["A", "B", "C"].into_iter().enumerate() {
            let top = row_top(&output, label, "Playlist");
            assert!(
                (offer.zone.list.top() + row as f32 * pitch - top).abs() < 0.01,
                "row {row} starts {row} pitches below the list: {} vs {top}",
                offer.zone.list.top()
            );
        }
    }

    #[test]
    fn the_offers_origin_matches_a_drawn_row() {
        let ctx = context();
        let mut library = movable_playlists(&["A", "B", "C"], 7, true, None);
        let mut art = Art::default();
        let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);
        let offer = carry::take_column_offer(&ctx).expect("the column offers itself");

        // The offer's own formula is `painted top - first index * pitch`, which
        // is asserted here against what the painter drew rather than by driving
        // a scroll, so it holds at any offset.
        let painted = row_top(&output, "A", "Playlist");
        assert!((offer.zone.list.top() - painted).abs() < 0.01);
        assert!(
            offer.zone.list.width() > 0.0,
            "the rule a drop draws spans the rows"
        );
    }

    #[test]
    fn a_list_that_can_not_be_reordered_still_offers_its_rows() {
        let ctx = context();
        let mut library = Library::default();
        library.apply(&Update::Playlists(vec![
            playlist("spotify:playlist:A", "A"),
            playlist("spotify:playlist:B", "B"),
        ]));
        assert!(!library.rootlist_movable());
        let mut art = Art::default();

        let _ = run_column(&ctx, Vec::new(), &mut library, &mut art);

        let offer = carry::take_column_offer(&ctx).expect("the column offers its rows");
        assert_eq!(offer.zone.count, 2);
    }

    #[test]
    fn a_list_that_is_not_ready_offers_nothing() {
        let ctx = context();
        let mut library = Library::default();
        library.playlists = Load::Loading;
        let mut art = Art::default();

        let _ = run_column(&ctx, Vec::new(), &mut library, &mut art);

        assert_eq!(carry::take_column_offer(&ctx), None);
    }

    #[test]
    fn an_addable_row_reads_its_track_count_and_an_other_row_its_owner() {
        let library = movable_playlists(&["A"], 7, true, Some(4));
        let Load::Ready(playlists) = &library.playlists else {
            panic!("the list is ready");
        };
        assert_eq!(ListItem::kind(&playlists[0]), "Playlist · 4 tracks");

        let mut ours = playlist("spotify:playlist:O", "O");
        ours.owner = Some(Owner::You);
        assert_eq!(
            ListItem::kind(&ours),
            "Playlist",
            "our own list draws the kind word alone"
        );

        let unnamed = playlist("spotify:playlist:N", "N");
        assert_eq!(ListItem::kind(&unnamed), "Playlist");

        let mut named = playlist("spotify:playlist:V", "V");
        named.owner = Some(Owner::Named(
            OwnerName::from_wire("Vela Moreno").expect("a name"),
        ));
        assert_eq!(
            ListItem::kind(&named),
            "Playlist · Vela Moreno",
            "a playlist that cannot take an add keeps its owner line"
        );
    }

    #[test]
    fn the_failure_line_names_what_could_not_be_done() {
        let ctx = context();
        let mut library = movable_playlists(&["A", "B"], 7, true, None);
        library.sidebar_notice = Some(crate::library::SidebarWrite::Reorder);
        let mut art = Art::default();

        let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);

        assert!(
            painted(&output, "Could not move that playlist. Try again."),
            "the list says what could not be done"
        );
    }

    #[test]
    fn no_row_washes_while_a_block_is_carried() {
        let ctx = context();
        let mut library = movable_playlists(&["A", "B"], 7, true, None);
        let mut art = Art::default();
        let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);
        let at = name_line(&output, "B");
        let tokens = Tokens::DARK;

        // The pointer arrives over the row, and a block is carried from elsewhere.
        carry::lift(
            &ctx,
            Carry {
                source: Source::Queue,
                rows: vec![0],
                entries: Vec::new(),
                size: Vec2::new(100.0, ROW_H),
                ghost: Ghost::Queue(queue_row::Lifted::new(
                    "A Title".to_owned(),
                    String::new(),
                    None,
                    Cover::Missing,
                )),
                playlist: None,
            },
        );
        let mut washed = false;
        for _ in 0..3 {
            let (output, _) =
                run_column(&ctx, vec![Event::PointerMoved(at)], &mut library, &mut art);
            washed |= output.shapes.iter().any(|clipped| {
                matches!(&clipped.shape, egui::epaint::Shape::Rect(shape)
                    if shape.fill == tokens.palette.row_hover)
            });
        }

        assert!(!washed, "the only fill in the column is the target's");
    }

    /// A library with both lists ready, the way the window hands them over.
    fn library_with(playlists: Vec<PlaylistEntry>, shows: Vec<FollowedShow>) -> Library {
        let mut library = Library::default();
        library.apply(&Update::Playlists(playlists));
        library.apply(&Update::FollowedShows(shows));
        library
    }

    /// Whether a line of text was painted at all.
    fn painted(output: &egui::FullOutput, label: &str) -> bool {
        output.shapes.iter().any(|clipped| {
            matches!(&clipped.shape,
                egui::epaint::Shape::Text(text) if text.galley.text() == label)
        })
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

    #[test]
    fn a_show_row_asks_for_its_cover() {
        use takofy_model::CoverUrl;

        let ctx = context();
        let mut show = followed_show("spotify:show:s", "A show");
        let url = "https://i.scdn.co/image/0123456789abcdef0123456789abcdef01234567";
        let cover = CoverUrl::from_wire(url).expect("the CDN url is admitted");
        show.cover = Some(cover.clone());
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

    /// The fills the row chrome paints with the pointer at `at`.
    fn chrome_fills(at: Option<Pos2>) -> Vec<Color32> {
        let ctx = context();
        let tokens = Tokens::DARK;
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(254.0, ROW_H));
        let mut fills = Vec::new();
        for _ in 0..2 {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(254.0, 800.0))),
                    events: at.map(Event::PointerMoved).into_iter().collect(),
                    ..Default::default()
                },
                |ui| {
                    let id = egui::Id::unique("library-row");
                    let look = Look {
                        selected: false,
                        ..Look::default()
                    };
                    let _ = row_chrome(ui, rect, id, "A", look, &tokens, false);
                },
            );
            output.textures_delta.clear();
            fills = output
                .shapes
                .iter()
                .filter_map(|clipped| match &clipped.shape {
                    egui::epaint::Shape::Rect(shape) if shape.rect == rect => Some(shape.fill),
                    _ => None,
                })
                .collect();
        }
        fills
    }

    #[test]
    fn a_hovered_library_row_washes_with_row_hover() {
        let tokens = Tokens::DARK;
        assert_eq!(
            chrome_fills(Some(Pos2::new(120.0, ROW_H / 2.0))),
            vec![tokens.palette.row_hover],
            "a hovered library row washed with something other than the row hover"
        );
        assert!(
            chrome_fills(None).is_empty(),
            "a library row with the pointer away washed"
        );
    }

    /// A line painted as `text`, if the frame painted it.
    fn drawn_text(output: &egui::FullOutput) -> Vec<String> {
        output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Text(text) => Some(text.galley.text().to_owned()),
                _ => None,
            })
            .collect()
    }

    /// A library holding one row of each kind, with an editable rootlist.
    fn four_kinds() -> Library {
        let mut library = movable_playlists(&["A"], 7, true, None);
        library.apply(&Update::FollowedShows(vec![followed_show(
            "spotify:show:s",
            "A show",
        )]));
        library.apply(&Update::FollowedArtists(vec![followed_artist(
            "spotify:artist:a",
            "An artist",
        )]));
        library.apply(&Update::SavedAlbums(vec![saved_album(
            "spotify:album:r",
            "An album",
            "Vela",
        )]));
        library
    }

    /// One press of the context-menu key.
    fn menu_key() -> Event {
        Event::Key {
            key: egui::Key::Menu,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }
    }

    /// Right-clicks `at` in the column form, one pass per event.
    fn right_click(
        ctx: &egui::Context,
        at: Pos2,
        library: &mut Library,
        art: &mut Art,
    ) -> Option<Command> {
        let mut command = None;
        for event in crate::view::test_support::secondary_click_events(at) {
            let (_, next) = run_column(ctx, vec![event], library, art);
            if next.is_some() {
                command = next;
            }
        }
        command
    }

    #[test]
    fn a_right_click_on_each_kind_posts_its_library_request() {
        let cases = [
            (SidebarKind::Playlists, "A", "spotify:playlist:A", "A"),
            (SidebarKind::Podcasts, "A show", "spotify:show:s", "A show"),
            (
                SidebarKind::Artists,
                "An artist",
                "spotify:artist:a",
                "An artist",
            ),
            (
                SidebarKind::Albums,
                "An album",
                "spotify:album:r",
                "An album",
            ),
        ];
        for (kind, label, uri, name) in cases {
            let ctx = context();
            let mut library = four_kinds();
            assert!(library.choose_kind(kind));
            let mut art = Art::default();
            let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);
            let at = name_line(&output, label);

            let command = right_click(&ctx, at, &mut library, &mut art);

            let request = context_menu::take_request(&ctx)
                .unwrap_or_else(|| panic!("{kind:?} posted no menu"));
            assert_eq!(
                request.listing,
                Listing::Library(LibraryItem {
                    kind,
                    uri: uri.to_owned(),
                    name: name.to_owned(),
                })
            );
            let expected = if kind == SidebarKind::Playlists {
                vec!["Rename", "Copy link", "Remove from library"]
            } else {
                vec!["Copy link", "Remove from library"]
            };
            assert_eq!(
                request
                    .items
                    .iter()
                    .map(|item| item.label.as_str())
                    .collect::<Vec<_>>(),
                expected,
                "{kind:?}"
            );
            assert!(
                matches!(request.door.anchor, Anchor::Pointer(_)),
                "{kind:?}"
            );
            assert_eq!(request.door.device, Device::Pointer, "{kind:?}");
            assert!(request.targets.is_empty(), "{kind:?}");
            assert!(request.rows.is_empty(), "{kind:?}");
            assert_eq!(request.page, None, "{kind:?}");

            assert_eq!(command, None, "{kind:?} opened its page");
            assert!(carry::carried(&ctx).is_none(), "{kind:?} lifted its row");
        }
    }

    #[test]
    fn a_row_field_centres_its_text_in_the_box() {
        let ctx = context();
        let mut library = movable_playlists(&["A"], 7, true, None);
        let mut art = Art::default();
        let (plain, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);
        library
            .renaming
            .open("spotify:playlist:A", "A", crate::naming::Surface::Row);
        let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);

        let (pos, size) = painted_text(&output, "A");
        let middle = pos.y + size.y / 2.0;
        let row_middle = row_top(&plain, "A", "Playlist") + ROW_H / 2.0;
        assert!(
            (middle - row_middle).abs() < 1.0,
            "the field's text sits at {middle}, not the row's middle {row_middle}"
        );
    }

    #[test]
    fn a_rename_field_is_as_tall_as_both_of_the_row_lines() {
        let field = super::rename_field(10.0, 4.0, 120.0, 17.0, 13.0);
        assert_eq!(field.min, Pos2::new(10.0, 4.0));
        assert_eq!(field.width(), 120.0);
        assert_eq!(
            field.height(),
            30.0,
            "a field on the name line alone clips its own text"
        );
        assert!(
            field.height() > 17.0,
            "the field has no room past the name line"
        );
    }

    #[test]
    fn a_row_the_user_cannot_write_offers_no_rename() {
        for access in [
            Some(takofy_model::PlaylistAccess {
                revision: takofy_model::Revision::from_wire(&[7]).expect("a revision"),
                editable: false,
            }),
            None,
        ] {
            let ctx = context();
            let mut library = movable_playlists(&["A"], 7, true, None);
            if let Load::Ready(playlists) = &mut library.playlists {
                playlists[0].access = access;
            }
            let mut art = Art::default();
            let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);
            let at = name_line(&output, "A");
            let _ = right_click(&ctx, at, &mut library, &mut art);

            let request = context_menu::take_request(&ctx).expect("the row posted a menu");
            assert!(
                request
                    .items
                    .iter()
                    .all(|item| item.verb != crate::menu::Verb::Rename),
                "a row the listing does not call editable offered Rename"
            );
        }

        let ctx = context();
        let mut library = movable_playlists(&["A"], 7, true, None);
        if let Load::Ready(playlists) = &mut library.playlists {
            playlists[0].access = None;
        }
        let mut art = Art::default();
        let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);
        let at = name_line(&output, "A");
        let _ = right_click(&ctx, at, &mut library, &mut art);
        let request = context_menu::take_request(&ctx).expect("the row posted a menu");
        assert!(
            request
                .items
                .iter()
                .all(|item| item.verb != crate::menu::Verb::Rename),
            "a row with no access yet offered Rename"
        );

        if let Load::Ready(playlists) = &mut library.playlists {
            playlists[0].access = Some(takofy_model::PlaylistAccess {
                revision: takofy_model::Revision::from_wire(&[7]).expect("a revision"),
                editable: true,
            });
        }
        let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);
        let at = name_line(&output, "A");
        let _ = right_click(&ctx, at, &mut library, &mut art);
        let request = context_menu::take_request(&ctx).expect("the row posted a menu");
        assert!(
            request
                .items
                .iter()
                .any(|item| item.verb == crate::menu::Verb::Rename),
            "the arrived access still offered no Rename"
        );
    }

    #[test]
    fn the_two_rows_of_one_list_are_two_openers() {
        let ctx = context();
        let mut library = movable_playlists(&["A", "B"], 7, true, None);
        let mut art = Art::default();
        let mut openers = Vec::new();
        for label in ["A", "B"] {
            let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);
            let at = name_line(&output, label);
            let _ = right_click(&ctx, at, &mut library, &mut art);
            let request = context_menu::take_request(&ctx).expect("a menu");
            openers.push(request.door.opener);
        }

        assert_ne!(openers[0], openers[1], "both rows opened the same widget");
    }

    #[test]
    fn the_context_menu_key_opens_on_a_focused_row_only() {
        let ctx = context();
        let mut library = movable_playlists(&["A", "B"], 7, true, None);
        let mut art = Art::default();
        let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);
        let at = name_line(&output, "A");

        let _ = run_column(&ctx, vec![menu_key()], &mut library, &mut art);
        assert!(
            context_menu::take_request(&ctx).is_none(),
            "a key with no focused row opened a menu"
        );

        for event in click_events(at) {
            let _ = run_column(&ctx, vec![event], &mut library, &mut art);
        }
        let _ = run_column(&ctx, vec![menu_key()], &mut library, &mut art);

        let request = context_menu::take_request(&ctx).expect("the focused row opened its menu");
        assert!(matches!(request.door.anchor, Anchor::Button(_)));
        assert_eq!(request.door.device, Device::Keyboard);
        assert_eq!(
            request.listing,
            Listing::Library(LibraryItem {
                kind: SidebarKind::Playlists,
                uri: "spotify:playlist:A".to_owned(),
                name: "A".to_owned(),
            })
        );
    }

    #[test]
    fn a_carry_or_the_drawer_form_posts_no_menu() {
        // A block is held.
        let ctx = context();
        let mut library = movable_playlists(&["A"], 7, true, None);
        let mut art = Art::default();
        let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);
        let at = name_line(&output, "A");
        egui::DragAndDrop::set_payload(&ctx, ());
        let _ = right_click(&ctx, at, &mut library, &mut art);
        assert!(
            context_menu::take_request(&ctx).is_none(),
            "a held block opened a menu"
        );

        // The drawer's form opens no menu, so it posts nothing.
        let ctx = context();
        let mut library = movable_playlists(&["A"], 7, true, None);
        let (output, _) = run_with(&ctx, Vec::new(), &mut library, &mut art, false);
        let at = name_line(&output, "A");
        for event in crate::view::test_support::secondary_click_events(at) {
            let _ = run_with(&ctx, vec![event], &mut library, &mut art, false);
        }
        assert!(context_menu::take_request(&ctx).is_none());
    }

    #[test]
    fn a_playlist_row_that_cannot_be_unlinked_offers_only_its_link() {
        let ctx = context();
        let mut library = Library::default();
        library.apply(&Update::Playlists(vec![playlist(
            "spotify:playlist:A",
            "A",
        )]));
        assert!(!library.rootlist_removable());
        let mut art = Art::default();
        let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);
        let at = name_line(&output, "A");

        let _ = right_click(&ctx, at, &mut library, &mut art);

        let request = context_menu::take_request(&ctx).expect("a menu");
        assert_eq!(
            request
                .items
                .iter()
                .map(|item| item.label.as_str())
                .collect::<Vec<_>>(),
            ["Copy link"],
            "a row the library cannot write offered a removal"
        );
    }

    #[test]
    fn an_unowned_playlist_still_offers_its_removal() {
        let ctx = context();
        // The rootlist is the user's own, but this playlist's tracks are not
        // theirs to edit; the unlink names the rootlist row and not the tracks.
        let mut library = movable_playlists(&["A"], 7, false, None);
        library.apply(&Update::RootlistAccess(takofy_model::PlaylistAccess {
            revision: takofy_model::Revision::from_wire(&[7]).expect("a revision"),
            editable: true,
        }));
        assert!(library.rootlist_removable());
        let mut art = Art::default();
        let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);
        let at = name_line(&output, "A");

        let _ = right_click(&ctx, at, &mut library, &mut art);

        let request = context_menu::take_request(&ctx).expect("a menu");
        assert_eq!(
            request
                .items
                .iter()
                .map(|item| item.label.as_str())
                .collect::<Vec<_>>(),
            ["Copy link", "Remove from library"],
            "an unowned playlist was not offered its removal"
        );
    }

    // ---- the rail's filter field ----

    /// The hairlines a frame painted that could be the rail filter's underline:
    /// the field's own width, an ink that means focused or not.
    fn filter_hairlines(output: &egui::FullOutput) -> Vec<(Rect, Color32)> {
        let tokens = Tokens::DARK;
        output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Rect(shape)
                    if (shape.rect.height() - stroke::HAIRLINE).abs() < 0.01
                        && shape.rect.width() > 80.0
                        && shape.rect.width() < 200.0
                        && (shape.fill == tokens.palette.border
                            || shape.fill == tokens.palette.text) =>
                {
                    Some((shape.rect, shape.fill))
                }
                _ => None,
            })
            .collect()
    }

    /// The rail filter's box, read from the hairline under it.
    fn filter_field_of(output: &egui::FullOutput) -> Rect {
        let (hairline, _) = filter_hairlines(output)
            .into_iter()
            .next()
            .expect("the rail filter paints its bottom hairline");
        Rect::from_min_max(
            Pos2::new(hairline.left(), hairline.bottom() - extent::BUTTON),
            Pos2::new(hairline.right(), hairline.bottom()),
        )
    }

    /// A library whose playlists are the names, each ready and drawable.
    fn named_playlists(names: &[&str]) -> Library {
        let mut library = Library::default();
        library.apply(&Update::Playlists(
            names
                .iter()
                .map(|name| playlist(&format!("spotify:playlist:{name}"), name))
                .collect(),
        ));
        library
    }

    /// The names a frame painted, in the rows' own order.
    fn painted_rows(output: &egui::FullOutput, names: &[&str]) -> Vec<String> {
        let mut drawn: Vec<(f32, String)> = names
            .iter()
            .filter_map(|name| {
                output
                    .shapes
                    .iter()
                    .find_map(|clipped| match &clipped.shape {
                        egui::epaint::Shape::Text(text) if text.galley.text() == *name => {
                            Some((text.pos.y, (*name).to_owned()))
                        }
                        _ => None,
                    })
            })
            .collect();
        drawn.sort_by(|left, right| left.0.total_cmp(&right.0));
        drawn.into_iter().map(|(_, name)| name).collect()
    }

    /// The tiles a frame painted stepped back because their row is carried.
    fn lifted_tiles(output: &egui::FullOutput) -> Vec<Rect> {
        let tokens = Tokens::DARK;
        let dim = tokens
            .palette
            .surface_sunken
            .gamma_multiply(crate::theme::opacity::LIFTED);
        output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Rect(shape) if shape.fill == dim => Some(shape.rect),
                _ => None,
            })
            .collect()
    }

    /// Clicks the rail's field and types `text` into it, answering the frame the
    /// keystroke landed on.
    fn type_in_rail(
        ctx: &egui::Context,
        text: &str,
        library: &mut Library,
        art: &mut Art,
    ) -> egui::FullOutput {
        let (output, _) = run_column(ctx, Vec::new(), library, art);
        let field = filter_field_of(&output);
        for event in click_events(field.center()) {
            let _ = run_column(ctx, vec![event], library, art);
        }
        assert!(
            ctx.text_edit_focused(),
            "the click focused the rail's field"
        );
        let (output, _) = run_column(ctx, vec![Event::Text(text.to_owned())], library, art);
        output
    }

    #[test]
    fn typing_narrows_the_rows_in_the_same_frame() {
        let ctx = context();
        let mut library = named_playlists(&["Midnight City", "Outro", "Midnight Sun"]);
        let mut art = Art::default();

        let output = type_in_rail(&ctx, "midnight", &mut library, &mut art);

        assert_eq!(
            painted_rows(&output, &["Midnight City", "Outro", "Midnight Sun"]),
            ["Midnight City", "Midnight Sun"],
            "the keystroke frame already draws only the matches"
        );
        let pitch = ROW_H + space::PX;
        let first = row_top(&output, "Midnight City", "Playlist");
        let second = row_top(&output, "Midnight Sun", "Playlist");
        assert!(
            (second - first - pitch).abs() < 0.01,
            "the survivors close up into consecutive slots: {first} then {second}"
        );
    }

    #[test]
    fn the_two_axes_compose() {
        let ctx = context();
        // The matching playlist and the matching artist sit at different true
        // indices, so reading the wrong kind's list cannot pass.
        let mut library = named_playlists(&["Vela", "Artemis"]);
        library.apply(&Update::FollowedArtists(vec![
            followed_artist("spotify:artist:1", "Artemis"),
            followed_artist("spotify:artist:2", "Vela Moreno"),
        ]));
        assert!(library.choose_kind(SidebarKind::Artists));
        let mut art = Art::default();

        let output = type_in_rail(&ctx, "art", &mut library, &mut art);

        assert!(
            painted(&output, "Artemis"),
            "the chosen kind's matching row is drawn"
        );
        assert!(
            !painted(&output, "Vela Moreno"),
            "a row of the chosen kind that does not match is hidden"
        );
    }

    #[test]
    fn matching_is_case_insensitive_over_the_name_only() {
        let ctx = context();
        // The artist's kind line is "Artist", which a name-only match ignores.
        let mut library = named_playlists(&[]);
        library.apply(&Update::FollowedArtists(vec![followed_artist(
            "spotify:artist:a",
            "Vela",
        )]));
        assert!(library.choose_kind(SidebarKind::Artists));
        let mut art = Art::default();

        let output = type_in_rail(&ctx, "art", &mut library, &mut art);

        assert!(
            !painted(&output, "Vela"),
            "the kind line is not what the query matches"
        );

        // And a name matches whatever its case.
        let ctx = context();
        let mut library = named_playlists(&[]);
        library.apply(&Update::FollowedArtists(vec![followed_artist(
            "spotify:artist:a",
            "Artemis",
        )]));
        assert!(library.choose_kind(SidebarKind::Artists));
        library.rail_filter.edit().push_str("ARTE");
        let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);
        assert!(painted(&output, "Artemis"), "the query folds case");
    }

    #[test]
    fn a_blank_query_narrows_nothing() {
        let ctx = context();
        let mut library = named_playlists(&["Alpha", "Bravo"]);
        library.rail_filter.edit().push_str("   ");
        let mut art = Art::default();

        let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);

        assert_eq!(
            painted_rows(&output, &["Alpha", "Bravo"]),
            ["Alpha", "Bravo"],
            "whitespace is no filter"
        );
        assert_eq!(
            carry::take_column_offer(&ctx).and_then(|offer| offer.rows),
            None,
            "a blank query leaves the column whole"
        );
    }

    #[test]
    fn clearing_restores_every_row() {
        let ctx = context();
        let mut library = named_playlists(&["Midnight City", "Outro"]);
        let mut art = Art::default();
        let _ = type_in_rail(&ctx, "mid", &mut library, &mut art);

        library.rail_filter.clear();
        let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);

        assert_eq!(
            painted_rows(&output, &["Midnight City", "Outro"]),
            ["Midnight City", "Outro"],
            "the whole list is back"
        );
    }

    #[test]
    fn a_narrowed_list_offers_only_what_it_draws() {
        let ctx = context();
        let mut library =
            movable_playlists(&["Midnight City", "Outro", "Midnight Sun"], 7, true, None);
        library.rail_filter.edit().push_str("midnight");
        let mut art = Art::default();

        let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);

        let offer = carry::take_column_offer(&ctx).expect("the column offers itself");
        assert_eq!(offer.zone.count, 2, "only the drawn rows are offered");
        let first = row_top(&output, "Midnight City", "Playlist");
        assert!(
            (offer.zone.list.top() - first).abs() < 0.01,
            "the rows box starts at the first drawn row: {} vs {first}",
            offer.zone.list.top()
        );
    }

    #[test]
    fn a_narrowed_offer_carries_the_true_rows() {
        let ctx = context();
        let mut library =
            movable_playlists(&["Midnight City", "Outro", "Midnight Sun"], 7, true, None);
        library.rail_filter.edit().push_str("midnight");
        let mut art = Art::default();

        let _ = run_column(&ctx, Vec::new(), &mut library, &mut art);

        let offer = carry::take_column_offer(&ctx).expect("the column offers itself");
        assert_eq!(
            offer.rows,
            Some(vec![0, 2]),
            "the offer names the playlists the slots stand for"
        );
        assert_eq!(offer.source_row(1), Some(2));
    }

    #[test]
    fn an_unnarrowed_offer_carries_no_rows() {
        let ctx = context();
        let mut library = movable_playlists(&["Alpha", "Bravo"], 7, true, None);
        let mut art = Art::default();

        let _ = run_column(&ctx, Vec::new(), &mut library, &mut art);

        let offer = carry::take_column_offer(&ctx).expect("the column offers itself");
        assert_eq!(offer.rows, None, "a whole list needs no map");
    }

    #[test]
    fn a_narrowed_playlist_row_does_not_lift() {
        let ctx = context();
        let mut library = movable_playlists(&["Midnight City", "Outro"], 7, true, None);
        library.rail_filter.edit().push_str("midnight");
        let mut art = Art::default();
        let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);
        let from = name_line(&output, "Midnight City");

        let _ = hold_from(&ctx, from, &mut library, &mut art, true);

        assert!(
            carry::carried(&ctx).is_none(),
            "a narrowed rail row has no reorder it could land"
        );
    }

    #[test]
    fn an_unnarrowed_row_still_lifts_with_its_true_index() {
        let ctx = context();
        let mut library = movable_playlists(&["Alpha", "Bravo"], 7, true, None);
        let mut art = Art::default();
        let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);
        let from = name_line(&output, "Bravo");

        let _ = hold_from(&ctx, from, &mut library, &mut art, true);

        let carry = carry::carried(&ctx).expect("the unnarrowed row lifts");
        assert_eq!(carry.rows, vec![1], "the carry names the row's true index");
    }

    #[test]
    fn a_row_lifted_before_narrowing_carries_its_true_index_and_dims_the_right_row() {
        let ctx = context();
        let mut library = movable_playlists(&["Alpha", "Bravo", "Midnight Sun"], 7, true, None);
        let mut art = Art::default();
        let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);
        let from = name_line(&output, "Midnight Sun");

        let _ = hold_from(&ctx, from, &mut library, &mut art, true);
        let carry = carry::carried(&ctx).expect("the row lifts");
        assert_eq!(carry.rows, vec![2]);

        // The query leaves only the lifted row, which now draws in the first
        // slot: the dim follows the playlist, not the slot it moved to.
        library.rail_filter.edit().push_str("midnight");
        let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);

        assert_eq!(
            painted_rows(&output, &["Alpha", "Bravo", "Midnight Sun"]),
            ["Midnight Sun"],
            "only the lifted row survives the query"
        );
        assert_eq!(
            lifted_tiles(&output).len(),
            1,
            "the surviving row is the one stepped back: {:?}",
            lifted_tiles(&output)
        );
    }

    #[test]
    fn a_query_matching_nothing_draws_no_rows_and_offers_nothing() {
        let ctx = context();
        let mut library = movable_playlists(&["Alpha", "Bravo"], 7, true, None);
        library.rail_filter.edit().push_str("zzz");
        let mut art = Art::default();

        let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);

        assert_eq!(
            painted_rows(&output, &["Alpha", "Bravo"]),
            Vec::<String>::new(),
            "no row matches"
        );
        assert_eq!(
            carry::take_column_offer(&ctx),
            None,
            "an empty list offers no rows"
        );
    }

    #[test]
    fn the_query_survives_a_reload_and_a_loading_frame() {
        let ctx = context();
        let mut library = movable_playlists(&["Midnight City", "Outro"], 7, true, None);
        let mut art = Art::default();
        let _ = type_in_rail(&ctx, "mid", &mut library, &mut art);

        library.apply(&Update::Playlists(vec![
            playlist("spotify:playlist:Midnight City", "Midnight City"),
            playlist("spotify:playlist:Outro", "Outro"),
        ]));
        let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);
        assert_eq!(library.rail_filter.query(), "mid", "a reload keeps it");
        assert_eq!(
            painted_rows(&output, &["Midnight City", "Outro"]),
            ["Midnight City"],
            "and the narrowed rows come back with the reply"
        );

        library.playlists = Load::Loading;
        let _ = run_column(&ctx, Vec::new(), &mut library, &mut art);
        assert_eq!(
            library.rail_filter.query(),
            "mid",
            "a loading frame keeps the query too"
        );
        library.playlists = Load::Ready(vec![
            playlist("spotify:playlist:Midnight City", "Midnight City"),
            playlist("spotify:playlist:Outro", "Outro"),
        ]);
        let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);
        assert_eq!(
            painted_rows(&output, &["Midnight City", "Outro"]),
            ["Midnight City"],
            "the narrowed list is still narrowed after the load"
        );
    }

    #[test]
    fn changing_the_query_dismisses_a_row_rename() {
        let ctx = context();
        let mut library = movable_playlists(&["Alpha", "Bravo"], 7, true, None);
        library.renaming.open(
            "spotify:playlist:Bravo",
            "Bravo",
            crate::naming::Surface::Row,
        );
        let mut art = Art::default();
        let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);
        assert!(filter_hairlines(&output).len() == 1, "the field is drawn");

        let _ = type_in_rail(&ctx, "a", &mut library, &mut art);

        assert!(
            !library.renaming.is_open(),
            "a keystroke in the rail closed the rename it would hide"
        );
    }

    #[test]
    fn a_rename_on_a_row_the_query_hides_is_dismissed() {
        let ctx = context();
        let mut library = movable_playlists(&["Alpha", "Bravo"], 7, true, None);
        library.renaming.open(
            "spotify:playlist:Bravo",
            "Bravo",
            crate::naming::Surface::Row,
        );
        // The query is already there when the frame begins, so this is not a
        // keystroke: the hidden row is enough to close the field.
        library.rail_filter.edit().push_str("alpha");
        let mut art = Art::default();

        let _ = run_column(&ctx, Vec::new(), &mut library, &mut art);

        assert!(
            !library.renaming.is_open(),
            "the rename's row is not among the survivors, so the field goes"
        );
        assert_eq!(library.rail_filter.query(), "alpha");
    }

    /// The count's own words, read from its status node inside the pass.
    fn status_label(ctx: &egui::Context, library: &mut Library, art: &mut Art) -> Option<String> {
        let (_, label) = frame_probing(ctx, Vec::new(), library, art, true, |ui| {
            ui.ctx()
                .accesskit_node_builder(ui.scope_id().with("library-count"), |node| {
                    node.label().map(str::to_owned)
                })
                .flatten()
        });
        label
    }

    #[test]
    fn the_status_node_carries_the_count_politely() {
        let ctx = context();
        ctx.enable_accesskit();
        let mut library = movable_playlists(&["Alpha", "Bravo"], 7, true, None);
        let mut art = Art::default();

        let (_, read) = frame_probing(&ctx, Vec::new(), &mut library, &mut art, true, |ui| {
            ui.ctx()
                .accesskit_node_builder(ui.scope_id().with("library-count"), |node| {
                    (node.role(), node.live(), node.label().map(str::to_owned))
                })
                .expect("the count has a status node")
        });

        assert_eq!(read.0, egui::Role::Status);
        assert_eq!(read.1, Some(egui::accesskit::Live::Polite));
        assert_eq!(
            read.2.as_deref(),
            Some("2 saved"),
            "the rest string is the reference's own words"
        );
    }

    #[test]
    fn the_status_node_says_n_of_total_while_narrowed() {
        let ctx = context();
        ctx.enable_accesskit();
        let mut library = movable_playlists(&["Midnight City", "Outro", "Bravo"], 7, true, None);
        library.rail_filter.edit().push_str("mid");
        let mut art = Art::default();

        assert_eq!(
            status_label(&ctx, &mut library, &mut art).as_deref(),
            Some("1 of 3"),
            "the announced string counts both axes"
        );
    }

    #[test]
    fn the_status_node_follows_a_chip_change() {
        let ctx = context();
        ctx.enable_accesskit();
        let mut library = movable_playlists(&["Alpha", "Bravo"], 7, true, None);
        library.apply(&Update::FollowedArtists(vec![followed_artist(
            "spotify:artist:a",
            "An artist",
        )]));
        let mut art = Art::default();

        assert_eq!(
            status_label(&ctx, &mut library, &mut art).as_deref(),
            Some("2 of 3")
        );

        assert!(library.choose_kind(SidebarKind::Artists));
        assert_eq!(
            status_label(&ctx, &mut library, &mut art).as_deref(),
            Some("1 of 3"),
            "the chosen kind is what the count counts"
        );
    }

    #[test]
    fn the_fields_placeholder_property_matches_the_status_string() {
        let ctx = context();
        ctx.enable_accesskit();
        let mut library = movable_playlists(&["Midnight City", "Outro"], 7, true, None);
        let mut art = Art::default();

        // The field's own resting line, and the same string on its node.
        let read = |library: &mut Library, art: &mut Art| {
            frame_probing(&ctx, Vec::new(), library, art, true, |ui| {
                let count = ui
                    .ctx()
                    .accesskit_node_builder(ui.scope_id().with("library-count"), |node| {
                        node.label().map(str::to_owned)
                    });
                let field = ui
                    .ctx()
                    .accesskit_node_builder(ui.scope_id().with("library-filter"), |node| {
                        node.placeholder().map(str::to_owned)
                    });
                (count.flatten(), field.flatten())
            })
            .1
        };

        let (count, field) = read(&mut library, &mut art);
        assert_eq!(count.as_deref(), Some("2 saved"));
        assert_eq!(field, count, "the two are one string");

        library.rail_filter.edit().push_str("mid");
        let (count, field) = read(&mut library, &mut art);
        assert_eq!(count.as_deref(), Some("1 of 2"));
        assert_eq!(field, count, "the two are one string while narrowed too");
    }

    #[test]
    fn a_status_node_exists_while_the_create_field_is_open_but_the_field_gets_no_stray_node() {
        let ctx = context();
        ctx.enable_accesskit();
        let mut library = movable_playlists(&["Alpha", "Bravo"], 7, true, None);
        library.naming.open();
        let mut art = Art::default();

        let (_, (status, stray)) =
            frame_probing(&ctx, Vec::new(), &mut library, &mut art, true, |ui| {
                let status = ui
                    .ctx()
                    .accesskit_node_builder(ui.scope_id().with("library-count"), |node| {
                        node.label().map(str::to_owned)
                    })
                    .flatten();
                let stray = ui
                    .ctx()
                    .has_accesskit_node(ui.scope_id().with("library-filter"));
                (status, stray)
            });

        assert_eq!(
            status.as_deref(),
            Some("2 saved"),
            "the count is announced whether or not the field is drawn"
        );
        assert!(!stray, "the undrawn field was given a node of its own");
    }

    #[test]
    fn the_head_draws_the_resting_count_as_the_placeholder() {
        let ctx = context();
        let mut library = movable_playlists(&["A", "B"], 7, true, None);
        let mut art = Art::default();
        let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);

        let field = filter_field_of(&output);
        let (pos, _) = painted_text(&output, "2 SAVED");
        assert!(
            field.contains(pos),
            "the count is the field's resting line: {pos:?} in {field:?}"
        );
        assert!(
            pos.x >= field.left() + Size::SM.side,
            "the line starts past the magnifier's glyph: {}",
            pos.x
        );
    }

    #[test]
    fn the_placeholder_names_the_kind_count_when_a_chip_narrows() {
        let ctx = context();
        let mut library = movable_playlists(&["A", "B"], 7, true, None);
        library.apply(&Update::FollowedShows(vec![followed_show(
            "spotify:show:s",
            "A show",
        )]));
        library.apply(&Update::FollowedArtists(vec![followed_artist(
            "spotify:artist:a",
            "An artist",
        )]));
        assert_eq!(library.sidebar_kind.chosen(), SidebarKind::Playlists);
        let mut art = Art::default();
        let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);

        assert!(
            painted(&output, "2 OF 4"),
            "the resting line names what the chip shows: {:?}",
            drawn_text(&output)
        );
        assert!(
            !painted(&output, "4 SAVED"),
            "the total is not the whole story"
        );
    }

    #[test]
    fn the_old_count_line_is_gone() {
        let ctx = context();
        let mut library = movable_playlists(&["A", "B"], 7, true, None);
        let mut art = Art::default();
        let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);

        let texts = drawn_text(&output);
        assert!(
            !texts.contains(&"2 saved".to_owned()),
            "the head still draws a second copy of the count: {texts:?}"
        );
        assert!(texts.contains(&"2 SAVED".to_owned()));
    }

    #[test]
    fn the_placeholder_disappears_the_moment_text_exists() {
        let ctx = context();
        let mut library = movable_playlists(&["A", "B"], 7, true, None);
        let mut art = Art::default();
        let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);
        let field = filter_field_of(&output);

        for event in click_events(field.center()) {
            let _ = run_column(&ctx, vec![event], &mut library, &mut art);
        }
        assert!(
            ctx.text_edit_focused(),
            "the click focused the rail's field"
        );

        let (output, _) = run_column(
            &ctx,
            vec![Event::Text("a".to_owned())],
            &mut library,
            &mut art,
        );

        assert_eq!(library.rail_filter.query(), "a");
        assert!(
            !painted(&output, "2 SAVED"),
            "the placeholder is still painted over the text"
        );
    }

    #[test]
    fn the_field_takes_what_the_create_button_leaves() {
        let ctx = context();
        let mut library = movable_playlists(&["A", "B"], 7, true, None);
        let mut art = Art::default();
        let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);

        let field = filter_field_of(&output);
        let row_left = rail().left() + space::S4;
        let create_left = rail().right() - space::S4 - extent::BUTTON;

        assert!(
            (field.left() - row_left).abs() < 0.01,
            "the field starts at the row's left: {} vs {row_left}",
            field.left()
        );
        assert!(
            (field.right() + space::S2 - create_left).abs() < 0.01,
            "the field takes what the button leaves: {} vs {create_left}",
            field.right()
        );
        assert!(
            (field.height() - extent::BUTTON).abs() < 0.01,
            "the field is one button tall: {}",
            field.height()
        );
    }

    #[test]
    fn opening_the_create_field_swaps_the_filter_field_out_and_clears_the_query() {
        let ctx = context();
        let mut library = movable_playlists(&["A", "B"], 7, true, None);
        library.rail_filter.edit().push_str("zzz");
        let mut art = Art::default();
        let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);
        assert!(
            painted(&output, "zzz"),
            "the query is drawn before the create field opens"
        );

        library.naming.open();
        let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);

        assert!(
            library.rail_filter.query().is_empty(),
            "the create field's opening clears the query"
        );
        assert!(
            filter_hairlines(&output).is_empty(),
            "the filter field is not drawn beside the new playlist's name"
        );
    }

    #[test]
    fn closing_the_create_field_restores_the_filter_field() {
        let ctx = context();
        let mut library = movable_playlists(&["A", "B"], 7, true, None);
        let mut art = Art::default();
        library.naming.open();
        let (open, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);
        assert!(filter_hairlines(&open).is_empty(), "the field is out");

        library.naming.cancel();
        let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);

        assert_eq!(
            filter_hairlines(&output).len(),
            1,
            "the filter field came back"
        );
        assert!(painted(&output, "2 SAVED"), "and its resting line with it");
    }

    #[test]
    fn the_failure_notice_draws_on_every_tab() {
        let line = "Could not remove that from your library. Try again.";
        for kind in sidebar_kind::ROW {
            let ctx = context();
            let mut library = four_kinds();
            assert!(library.choose_kind(kind));
            library.sidebar_notice = Some(crate::library::SidebarWrite::Remove);
            let mut art = Art::default();

            let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);

            assert!(painted(&output, line), "{kind:?} drew no notice");
        }
    }

    #[test]
    fn a_query_with_no_survivor_says_nothing_matches_the_query() {
        let ctx = context();
        let mut library = named_playlists(&["Alpha", "Bravo"]);
        library.rail_filter.edit().push_str("  zzz  ");
        let mut art = Art::default();

        let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);

        assert_eq!(
            painted_rows(&output, &["Alpha", "Bravo"]),
            Vec::<String>::new(),
            "no row matches"
        );
        assert!(
            painted(&output, "Nothing matches “zzz”"),
            "the empty line names the trimmed query: {:?}",
            drawn_text(&output)
        );
    }

    #[test]
    fn the_per_kind_sentence_stays_when_the_kind_alone_is_empty() {
        for (kind, words) in [
            (SidebarKind::Playlists, "No playlists yet."),
            (SidebarKind::Podcasts, "No shows followed yet."),
            (SidebarKind::Artists, "No artists followed yet."),
            (SidebarKind::Albums, "No albums saved yet."),
        ] {
            let ctx = context();
            let mut library = Library::default();
            library.playlists = Load::Ready(Vec::new());
            library.followed = Load::Ready(Vec::new());
            library.followed_artists = Load::Ready(Vec::new());
            library.saved_albums = Load::Ready(Vec::new());
            assert!(library.choose_kind(kind));
            library.rail_filter.edit().push_str("zzz");
            let mut art = Art::default();

            let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);

            assert!(
                painted(&output, words),
                "{kind:?} drew no per-kind sentence: {:?}",
                drawn_text(&output)
            );
            assert!(
                !painted(&output, "Nothing matches “zzz”"),
                "{kind:?} claimed a query emptied a kind that holds nothing"
            );
        }
    }

    #[test]
    fn no_query_keeps_todays_sentence() {
        let ctx = context();
        let mut library = Library::default();
        library.playlists = Load::Ready(Vec::new());
        let mut art = Art::default();

        let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);

        assert!(painted(&output, "No playlists yet."));
    }

    #[test]
    fn loading_and_failed_keep_their_words_with_a_query_present() {
        let ctx = context();
        let mut library = named_playlists(&["Alpha"]);
        library.rail_filter.edit().push_str("zzz");
        let mut art = Art::default();

        library.playlists = Load::Loading;
        let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);
        assert!(painted(&output, "Loading your playlists…"));
        assert!(!painted(&output, "Nothing matches “zzz”"));

        library.playlists = Load::Failed(Failure::Load);
        let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);
        assert!(painted(
            &output,
            "Could not load the library. Check your connection and try again."
        ));
        assert!(!painted(&output, "Nothing matches “zzz”"));
    }

    #[test]
    fn a_long_query_wraps_inside_the_list() {
        let ctx = context();
        let mut library = named_playlists(&["Alpha"]);
        let long = "z".repeat(200);
        library.rail_filter.edit().push_str(&long);
        let mut art = Art::default();

        let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);

        let line = format!("Nothing matches “{long}”");
        // The painted rect is the union of the galley's rows, because the line is
        // centred: the shape's own `pos` is the centre, not the left edge.
        let painted = output.shapes.iter().fold(None, |union, clipped| {
            let egui::epaint::Shape::Text(text) = &clipped.shape else {
                return union;
            };
            if text.galley.text() != line {
                return union;
            }
            let block = text.galley.rows.iter().fold(Rect::NOTHING, |block, row| {
                block.union(row.rect().translate(text.pos.to_vec2()))
            });
            Some(union.map_or(block, |union: Rect| union.union(block)))
        });
        let painted = painted.expect("the empty line is drawn");
        let list = Rect::from_min_max(
            Pos2::new(space::S2, 0.0),
            Pos2::new(rail().width() - space::S2, rail().height()),
        );
        assert!(
            painted.left() >= list.left() - 0.01 && painted.right() <= list.right() + 0.01,
            "the line stays inside the list horizontally: {}..{} in {}..{}",
            painted.left(),
            painted.right(),
            list.left(),
            list.right()
        );
    }
}
