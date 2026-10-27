//! The sidebar: the library's four filters, its size and its list.
//!
//! Below 900 points this is not a column at all: the layout gives it no width
//! and the drawer holds it, which is why there is one form here and no rail.

use std::time::Instant;

use egui::{Layout, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, Ui, UiBuilder, Vec2};
use takofy_model::{
    CoverSet, FollowedArtist, FollowedShow, PLAYLIST_NAME_MAX, PlaylistEntry, SavedAlbum,
};

use crate::availability::Control;
use crate::cover::{Art, Cover, want_px};
use crate::format::{
    library_count, library_kind, media_type, playlist_detail, rail_nothing, root_empty_label,
    root_loading_label, sidebar_kind_label, sidebar_notice,
};
use crate::icon::Icon;
use crate::landing::Zone;
use crate::layout::{chips, section as section_layout};
use crate::library::{Library, Load};
use crate::library_filter::RootKind;
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
    // the rail: the slot runs from the row's start up to the create button.
    let slot = Rect::from_min_max(row.min, Pos2::new(create.left() - space::S2, row.bottom()));
    let mut changed = false;
    let field = if open {
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
            library.begin_create();
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
///
/// Both lines are the root's, so a kind reads the same in the column as it does
/// under the cards.
fn words(kind: SidebarKind) -> (&'static str, &'static str) {
    let root = match kind {
        SidebarKind::Playlists => RootKind::Playlists,
        SidebarKind::Artists => RootKind::Artists,
        SidebarKind::Albums => RootKind::Albums,
        SidebarKind::Podcasts => RootKind::Podcasts,
    };
    (root_loading_label(root), root_empty_label(root))
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
    draw: &mut Draw<'_>,
) -> Option<Action> {
    let (loading, empty) = words(ids.shelf);
    match view {
        ListView::Loading => {
            state(ui, rect, recipe::muted(loading, draw.tokens));
            None
        }
        ListView::Empty => {
            state(ui, rect, recipe::muted(empty, draw.tokens));
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
                        let want = want_px(extent::LIBRARY_ART, ui.pixels_per_point());
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
                            let cover = draw.art.look(item.cover(), want, draw.now);
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
                                    draw.art.look(item.cover(), want, draw.now),
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
    fn cover(&self) -> Option<&CoverSet>;
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

    fn cover(&self) -> Option<&CoverSet> {
        self.cover_set()
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

    fn cover(&self) -> Option<&CoverSet> {
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
        library_kind(media_type(RootKind::Artists), None)
    }

    fn cover(&self) -> Option<&CoverSet> {
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

    fn cover(&self) -> Option<&CoverSet> {
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
    let height = notice.map(|_| ui.fonts_mut(|fonts| fonts.row_height(&line)));
    let (rect, foot) = match height {
        Some(height) => {
            let (rows, foot) = section_layout::foot(rect, height);
            (rows, Some(foot))
        }
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
mod tests;
