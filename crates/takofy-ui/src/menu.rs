//! One menu's state: whether it is open, what it lists, and which row is lit.
//!
//! This is a reducer like [`crate::search::Search`] and [`crate::drawer::Drawer`]:
//! the views draw what it decides and decide nothing themselves. It imports no
//! I/O, so the rules the sheet states are pinned by tests alone.
//!
//! The rules it encodes: a pointer opens with nothing lit and a keyboard opens
//! with the first row lit; only one of the two tiers is ever lit; a verb that
//! cannot apply is absent rather than greyed; the destructive row is always
//! last; and closing hands the keyboard back to the button that opened it.

use egui::{Id, Pos2, Rect};
use takofy_model::{Revision, TrackEntry};

use crate::route::Page;
use crate::sidebar_kind::SidebarKind;
use crate::theme::extent;

/// Where a menu is drawn from.
///
/// A button opens the menu right-aligned beneath itself, which is what the `⋯`
/// and the keyboard do. A right-click opens it at the pointer instead: a menu
/// that moved to the row's `⋯` would describe a place the user did not click.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Anchor {
    Button(Rect),
    Pointer(Pos2),
}

/// Which input last chose a row.
///
/// The keyboard owns the 2pt rule, the surface wash and the action glyph; the
/// pointer owns the 4% wash and the tick, and never borrows the rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Device {
    /// The arrows and the keys: the tier that shows what Enter will fire.
    #[default]
    Keyboard,
    /// The mouse: a hint, not the target.
    Pointer,
}

/// Which of the two tiers one row is drawn in, if any.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    None,
    Pointer,
    Keyboard,
}

/// Whether a command is stated plainly or is the destructive one.
///
/// A destructive row is full ink behind a hairline and always last; there is no
/// third kind, because a verb that cannot apply is absent rather than greyed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Plain,
    Destructive,
}

/// What one row of a menu does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verb {
    AddToQueue,
    Save,
    Unsave,
    GoToAlbum,
    GoToArtist,
    CopyLink,
    AddToPlaylist,
    NewPlaylist,
    Remove,
    /// Removes the one row a library menu was opened on from the library.
    RemoveFromLibrary,
    /// Opens the name field on the one playlist a library menu was opened on.
    Rename,
    /// Puts the one item a menu was opened on into the library.
    SaveToLibrary,
    /// Follows the one artist a menu was opened on.
    Follow,
    /// Unfollows the one artist a menu was opened on.
    Unfollow,
}

/// The list a menu's rows came from, which decides its verbs and their labels.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Listing {
    /// Rows that are not a list the user may write: an album, an artist, a
    /// search, a release block, or a playlist that is not theirs.
    #[default]
    Page,
    /// An editable playlist page, which may both add and remove.
    Playlist { uri: String, list: Id },
    /// The queue's own rows, which may not add what is already there.
    Queue,
    /// The entry Now Playing is on, which is never removable.
    Current,
    /// One row of the sidebar, whose verbs act on that row alone.
    Library(LibraryItem),
    /// A playlist band, whose verbs act on the playlist it heads.
    Band(LibraryItem),
}

impl Listing {
    /// Whether this list offers a removal.
    pub fn removable(&self) -> bool {
        matches!(self, Listing::Playlist { .. } | Listing::Queue)
    }
}

/// One row of the sidebar, as a menu opened on it describes it.
///
/// A sidebar row is not a track: its identity is the kind it belongs to and the
/// URI that kind is listed by, and the list it lives in is read from the library
/// when a verb is applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryItem {
    pub kind: SidebarKind,
    pub uri: String,
    pub name: String,
}

/// A removal a list asked for by key: which list, the rows, and what they held.
#[derive(Debug, Clone, PartialEq)]
pub struct Cut {
    pub listing: Listing,
    pub rows: Vec<usize>,
    pub entries: Vec<TrackEntry>,
}

/// One command row: what it says and what it does.
///
/// `key` is a key hint and `submenu` says the row opens a list of its own rather
/// than firing a command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub label: String,
    pub kind: Kind,
    pub key: Option<&'static str>,
    pub submenu: bool,
    pub verb: Verb,
}

/// Which door opened the menu, and where it is anchored.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Door {
    pub anchor: Anchor,
    /// The widget the keyboard goes back to when the menu closes.
    pub opener: Id,
    pub device: Device,
}

/// Whether a menu may open at all.
///
/// A carried block and a write in flight both hold the lists still, so a menu
/// opened over either would act on rows that are about to move.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Gate {
    pub carrying: bool,
    pub writing: bool,
}

impl Gate {
    pub fn allows(&self) -> bool {
        !self.carrying && !self.writing
    }
}

/// One playlist the add submenu may offer, and what an add to it names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddTarget {
    pub uri: String,
    pub name: String,
    pub revision: Revision,
    /// The playlist's length, which is the index an append names.
    pub length: usize,
}

/// What a door asks the menu to show, taken as a snapshot of the rows it is about.
#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    pub door: Door,
    pub items: Vec<Item>,
    /// The playlists the add submenu may offer, in the sidebar's own order.
    pub playlists: Vec<AddTarget>,
    /// The rows the verbs act on, cloned at open time so a list that reloads
    /// underneath the menu cannot change what it does.
    pub targets: Vec<TrackEntry>,
    /// The rows those entries sit at, parallel to `targets`.
    pub rows: Vec<usize>,
    /// The list they belong to, which decides the verbs and their labels.
    pub listing: Listing,
    /// The page it was opened on, so a page change closes it.
    pub page: Option<Page>,
}

impl Request {
    /// Strips the new-playlist row when the window cannot offer it.
    ///
    /// Done at the one place a request is taken, rather than at each door: a
    /// menu opened in the rail has no sidebar to put the field in, and one
    /// opened before the playlists are ready has no list to insert into.
    pub fn allow_create(&mut self, allow: bool) {
        if !allow {
            self.items.retain(|item| item.verb != Verb::NewPlaylist);
        }
    }

    /// Takes the playlists the add submenu may offer, and strips the row when
    /// there are none: a submenu with nothing in it is worse than no row.
    pub fn allow_add(&mut self, targets: Vec<AddTarget>) {
        let offered = !targets.is_empty();
        self.playlists = targets;
        if !offered {
            self.items.retain(|item| item.verb != Verb::AddToPlaylist);
        }
    }
}

/// Why a menu closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    Escape,
    Tab,
    Outside,
    Page,
    /// The pointer chose something in the card.
    Pick,
}

/// What a close reports to the shell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Closed {
    /// The widget to hand the keyboard back to. `None` when the row the menu was
    /// about is gone, as it is on a page change.
    pub opener: Option<Id>,
    /// The verb the close chose, if it closed on a choice.
    pub chose: Option<Verb>,
}

/// One keyboard step over the rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Down,
    Up,
    First,
    Last,
}

/// An open menu, or a closed one that still remembers the snapshot and the list
/// its submenu chose.
#[derive(Debug, Clone, Default)]
pub struct Menu {
    open: bool,
    anchor: Option<Anchor>,
    opener: Option<Id>,
    device: Device,
    items: Vec<Item>,
    targets: Vec<TrackEntry>,
    rows: Vec<usize>,
    listing: Listing,
    page: Option<Page>,
    lit: Option<usize>,
    /// The lists the add submenu offers, in the sidebar's own order.
    playlists: Vec<AddTarget>,
    /// The row whose submenu is open, while it is.
    submenu: Option<usize>,
    /// The list row the submenu's keyboard is on, as a position in [`Menu::shown`].
    submenu_lit: Option<usize>,
    /// What the submenu's field holds. The lists are found by typing, because a
    /// user with thirty playlists does not scan a list eight rows long.
    filter: String,
    /// The list the submenu's last choice named. It survives the close, as the
    /// removal's snapshot does.
    chosen: Option<usize>,
    /// The clock of the first frame that drew this menu. A frame is not a pass:
    /// egui may run a frame twice, so the opening input is recognised by the
    /// clock rather than by a flag a first pass would clear before the second
    /// pass read it - which is what read the door's own click as a click
    /// outside.
    first_drawn: Option<f64>,
}

impl Menu {
    /// Opens on `request`, or refuses and changes nothing.
    ///
    /// A refused open leaves an already-open menu as it was: the frame's gate is
    /// about the moment, not about the menu that is already up.
    pub fn open(&mut self, request: Request, gate: Gate) -> bool {
        if !gate.allows() || request.items.is_empty() {
            return false;
        }
        let mut items = request.items;
        // Stable, so the plain commands keep the order they were built in.
        items.sort_by_key(|item| item.kind == Kind::Destructive);
        self.open = true;
        self.anchor = Some(request.door.anchor);
        self.opener = Some(request.door.opener);
        self.device = request.door.device;
        // The pointer lit nothing until it reaches a row; the keyboard says
        // where Enter will land.
        self.lit = match request.door.device {
            Device::Keyboard => Some(0),
            Device::Pointer => None,
        };
        self.items = items;
        self.targets = request.targets;
        self.rows = request.rows;
        self.listing = request.listing;
        self.page = request.page;
        self.playlists = request.playlists;
        self.submenu = None;
        self.chosen = None;
        self.first_drawn = None;
        true
    }

    /// Opens `row`'s submenu, when it has one and there is something to offer.
    ///
    /// A different row's submenu starts with an empty field: a filter typed for
    /// one list must not hide the next list's rows.
    pub fn open_submenu(&mut self, row: usize) -> bool {
        if !self.open || self.playlists.is_empty() {
            return false;
        }
        if !self.items.get(row).is_some_and(|item| item.submenu) {
            return false;
        }
        if self.submenu != Some(row) {
            self.submenu_lit = None;
            self.filter.clear();
        }
        self.submenu = Some(row);
        true
    }

    /// Closes the open submenu, leaving the menu itself up.
    pub fn close_submenu(&mut self) {
        self.submenu = None;
        self.submenu_lit = None;
        self.filter.clear();
    }

    /// The row whose submenu is open, while it is.
    pub fn submenu(&self) -> Option<usize> {
        self.submenu
    }

    /// The lists the add submenu offers.
    pub fn playlists(&self) -> &[AddTarget] {
        &self.playlists
    }

    /// What the submenu's field holds.
    pub fn filter(&self) -> &str {
        &self.filter
    }

    /// The submenu's field, for it to edit.
    pub fn filter_edit(&mut self) -> &mut String {
        &mut self.filter
    }

    /// The playlists the submenu shows: those whose name holds the typed text,
    /// in the sidebar's order, capped at the sheet's eight rows.
    ///
    /// The rows are indices into [`Menu::playlists`], so a choice names the list
    /// the filter did not have to be undone to reach.
    pub fn shown(&self) -> Vec<usize> {
        self.playlists
            .iter()
            .enumerate()
            .filter(|(_, target)| matches(&target.name, &self.filter))
            .map(|(index, _)| index)
            .take(extent::SUBMENU_ROWS)
            .collect()
    }

    /// Moves the submenu's lit row one step, clamping at the ends.
    pub fn step_submenu(&mut self, step: Step) {
        let last = match self.shown().len() {
            0 => return,
            count => count - 1,
        };
        self.device = Device::Keyboard;
        self.submenu_lit = Some(match step {
            Step::Down => self.submenu_lit.map_or(0, |row| (row + 1).min(last)),
            Step::Up => self.submenu_lit.map_or(last, |row| row.saturating_sub(1)),
            Step::First => 0,
            Step::Last => last,
        });
    }

    /// The submenu row the keyboard lit, as a position in [`Menu::shown`].
    ///
    /// A filter that shortened the list leaves the cursor past its end, which
    /// lights nothing until the keyboard moves again: a row that vanished must
    /// not fire because a different one took its place.
    pub fn submenu_lit(&self) -> Option<usize> {
        self.submenu_lit.filter(|row| *row < self.shown().len())
    }

    /// The pointer reached a submenu row, so the pointer owns that tier.
    pub fn submenu_pointer(&mut self, row: Option<usize>) {
        if !self.open || row.is_none() {
            return;
        }
        self.device = Device::Pointer;
        self.submenu_lit = row;
    }

    /// The tier one submenu row is drawn in.
    pub fn submenu_tier(&self, row: usize, hovered: Option<usize>) -> Tier {
        if !self.open || self.submenu.is_none() {
            return Tier::None;
        }
        match self.device {
            Device::Keyboard if self.submenu_lit() == Some(row) => Tier::Keyboard,
            Device::Pointer if hovered == Some(row) => Tier::Pointer,
            _ => Tier::None,
        }
    }

    /// Chooses the submenu row the keyboard lit.
    pub fn choose_submenu_lit(&mut self) -> Option<Closed> {
        let shown = self.shown();
        let target = *shown.get(self.submenu_lit()?)?;
        self.choose_target(target)
    }

    /// Chooses `index` of the open submenu's lists, and closes the menu.
    ///
    /// Refused without an open submenu, because a list nobody opened is a list
    /// nobody saw.
    pub fn choose_target(&mut self, index: usize) -> Option<Closed> {
        self.submenu?;
        if index >= self.playlists.len() {
            return None;
        }
        self.chosen = Some(index);
        self.close_with(Some(Verb::AddToPlaylist))
    }

    /// The list the submenu's last choice named, read after the close.
    pub fn chosen_target(&self) -> Option<&AddTarget> {
        self.chosen.and_then(|index| self.playlists.get(index))
    }

    /// Moves the lit row one step, clamping at the ends rather than wrapping.
    pub fn step(&mut self, step: Step) {
        if !self.open || self.items.is_empty() {
            return;
        }
        self.device = Device::Keyboard;
        let last = self.items.len() - 1;
        self.lit = Some(match step {
            Step::Down => self.lit.map_or(0, |row| (row + 1).min(last)),
            Step::Up => self.lit.map_or(last, |row| row.saturating_sub(1)),
            Step::First => 0,
            Step::Last => last,
        });
    }

    /// The pointer reached a row, so the pointer owns the tier from now on.
    ///
    /// A pointer that is on the card but not on a row takes nothing: the sheet
    /// lights nothing until the pointer reaches a row.
    pub fn pointer_over(&mut self, row: Option<usize>) {
        if !self.open || row.is_none() {
            return;
        }
        self.device = Device::Pointer;
        self.lit = None;
    }

    /// The tier one row is drawn in, given where the pointer is this frame.
    pub fn tier(&self, row: usize, hovered: Option<usize>) -> Tier {
        if !self.open {
            return Tier::None;
        }
        match self.device {
            Device::Keyboard if self.lit == Some(row) => Tier::Keyboard,
            Device::Pointer if hovered == Some(row) => Tier::Pointer,
            _ => Tier::None,
        }
    }

    /// Chooses `row` with the pointer, and closes.
    ///
    /// A row that opens a submenu is not a command, so it closes nothing.
    pub fn choose(&mut self, row: usize) -> Option<Closed> {
        let item = self.items.get(row)?;
        if item.submenu {
            return None;
        }
        self.close_with(Some(item.verb))
    }

    /// The row the keyboard lit, when the keyboard owns the tier.
    ///
    /// A pointer-opened menu has no current item, so nothing is lit for Enter.
    pub fn lit(&self) -> Option<usize> {
        self.lit.filter(|_| self.device == Device::Keyboard)
    }

    /// Chooses the row the keyboard lit, and closes. Enter fires nothing the user
    /// did not choose, so a pointer-opened menu has no current item.
    pub fn choose_lit(&mut self) -> Option<Closed> {
        let row = self.lit.filter(|_| self.device == Device::Keyboard)?;
        self.choose(row)
    }

    /// Chooses the destructive row, when the menu has one, and closes.
    pub fn choose_destructive(&mut self) -> Option<Closed> {
        let row = self
            .items
            .len()
            .checked_sub(1)
            .filter(|row| self.items[*row].kind == Kind::Destructive)?;
        self.choose(row)
    }

    /// Closes for `reason`, reporting what the shell has to undo.
    ///
    /// A page change loses the rows the menu described, so it is the one close
    /// that does not hand the keyboard back: the button is gone.
    pub fn close(&mut self, reason: Reason) -> Option<Closed> {
        let closed = self.close_with(None)?;
        let opener = if reason == Reason::Page {
            None
        } else {
            closed.opener
        };
        Some(Closed { opener, ..closed })
    }

    /// Closes when the page under the menu changed, because the rows it was
    /// opened on are gone.
    pub fn follow_page(&mut self, page: Option<&Page>) -> Option<Closed> {
        if self.open && self.page.as_ref() != page {
            return self.close(Reason::Page);
        }
        None
    }

    /// Whether `now` is still the frame the menu first drew on.
    ///
    /// Used to ignore the input that opened it: the click that is the door is
    /// also a click outside the menu, and an Enter on a focused `⋯` arrives in
    /// the same frame the rows appear.
    pub fn just_opened(&self, now: f64) -> bool {
        self.first_drawn.is_none_or(|drawn| now <= drawn)
    }

    /// Marks `now` as the frame the menu first drew on. Called only by a pass
    /// that really painted, since a sizing pass paints nothing.
    pub fn mark_drawn(&mut self, now: f64) {
        self.first_drawn.get_or_insert(now);
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn items(&self) -> &[Item] {
        &self.items
    }

    /// The rows the verbs act on. The snapshot survives the close, so the shell
    /// can apply the verb the close reported.
    pub fn targets(&self) -> &[TrackEntry] {
        &self.targets
    }

    /// The list the menu's rows came from, read after the close.
    pub fn listing(&self) -> &Listing {
        &self.listing
    }

    /// The sidebar row the menu was opened on, read after the close.
    pub fn library_item(&self) -> Option<&LibraryItem> {
        match &self.listing {
            Listing::Library(item) | Listing::Band(item) => Some(item),
            Listing::Page | Listing::Playlist { .. } | Listing::Queue | Listing::Current => None,
        }
    }

    /// The removal this menu's list would make, read after the close.
    ///
    /// A page's rows and the current entry are never removable, so only a
    /// playlist and the queue answer.
    pub fn cut(&self) -> Option<Cut> {
        if !self.listing.removable()
            || self.rows.is_empty()
            || self.rows.len() != self.targets.len()
        {
            return None;
        }
        Some(Cut {
            listing: self.listing.clone(),
            rows: self.rows.clone(),
            entries: self.targets.clone(),
        })
    }

    pub fn device(&self) -> Device {
        self.device
    }

    pub fn opener(&self) -> Option<Id> {
        self.opener
    }

    pub fn anchor(&self) -> Option<Anchor> {
        self.anchor
    }

    fn close_with(&mut self, chose: Option<Verb>) -> Option<Closed> {
        if !self.open {
            return None;
        }
        self.open = false;
        self.lit = None;
        self.submenu = None;
        self.submenu_lit = None;
        self.filter.clear();
        Some(Closed {
            opener: self.opener,
            chose,
        })
    }
}

/// Whether a list's name answers the submenu's field.
///
/// Case-insensitive and unanchored: typing `n` finds `Night Drive` and `Piano`.
/// An empty field matches every list, because a filter nobody typed filters
/// nothing.
pub fn matches(name: &str, filter: &str) -> bool {
    let filter = filter.trim();
    filter.is_empty() || name.to_lowercase().contains(&filter.to_lowercase())
}

#[cfg(test)]
mod tests;
