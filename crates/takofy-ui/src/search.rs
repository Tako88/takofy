//! The search field's state: the query, the menu's mark, and the two reads.
//!
//! This is a reducer, like [`crate::drawer::Drawer`]: the views draw what it
//! decides and never decide anything themselves. The rules it encodes are the
//! brief's R1–R3 and R12–R14, and each one has a test below.
//!
//! Two reads share one key space. The menu holds the answer it derives its
//! bands from; the page holds the whole answer. They are keyed by the same
//! collapsed query, so one reply can fill both, and a reply for a query the
//! user has typed past fills neither (R14).

use takofy_model::TrackEntry;

use crate::library::Load;
use crate::message::{Command, Failure, Update};
use crate::portraits::{Ask, Portrait, Portraits, Surface};
use crate::reveal::Reveal;
use crate::search_index::{Index, Kind, MENU, PAGE, Row, Shape};

/// The most tracks the search route answers. The page's note says "Top N" only
/// when an answer reaches it: a shorter answer hides nothing, so "Top" would
/// promise results that do not exist.
///
/// There is no constant to import, so this is kept in step by hand with
/// `Engine::search`'s doc in the core (`engine.rs`): the route answers twenty
/// and offers no paging.
pub const SEARCH_CAP: usize = 20;

/// Trims and joins runs of whitespace with one space, or `None` when blank.
///
/// It is the single key for both reads, the menu's heading and the floor row
/// (R6, R12), so "a  b" and "a b" are the same search everywhere.
pub fn collapse(raw: &str) -> Option<String> {
    let joined = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    if joined.is_empty() {
        None
    } else {
        Some(joined)
    }
}

/// The two lit tiers are one menu's, so the device lives beside the menu.
pub use crate::menu::Device;

/// One read: the key it is about, and what that key's answer is doing.
///
/// The key is stored rather than derived from the text, so an answer for a
/// superseded query can be told apart from the one on screen.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct Read {
    key: Option<String>,
    load: Load<Vec<TrackEntry>>,
    /// The shape derived from `load` when it is `Ready`.
    index: Option<Index>,
}

impl Read {
    /// Takes `load` as this read's answer, deriving its index when it is ready.
    fn settle(&mut self, load: Load<Vec<TrackEntry>>) {
        self.index = match &load {
            Load::Ready(tracks) => Some(Index::derive(tracks)),
            Load::Idle | Load::Loading | Load::Failed(_) => None,
        };
        self.load = load;
    }
}

/// Which of the two reads a request would take.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    Menu,
    Page,
}

/// What the field, the menu and the page need from one frame's keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct KeyInput {
    /// Arrow-down was pressed.
    pub down: bool,
    /// Arrow-up was pressed.
    pub up: bool,
    /// Enter was pressed and is not a repeat.
    pub enter: bool,
    /// Escape was pressed.
    pub escape: bool,
    /// The text edit reports that its text changed.
    pub typed: bool,
    /// The field itself was clicked.
    pub clicked_field: bool,
    /// Whether the field has the keyboard this frame.
    pub focused: bool,
}

/// What the field's keys asked the shell to do.
///
/// Only the actions that reach past the field are here: `Play` and
/// `OpenResults` are applied by the shell, and only when no other command took
/// this frame. `Close` says Escape cleared the query, which closed the menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Act {
    /// Nothing past the field's own state.
    #[default]
    None,
    /// Play the result at this index of the answer the menu came from (R16).
    Play(usize),
    /// Open the album at this index of the derived index.
    OpenAlbum(usize),
    /// Open the artist at this index of the derived index.
    OpenArtist(usize),
    /// Open the results page for the field's query (R1, R12).
    OpenResults,
    /// Escape cleared the query and closed the menu (R3 step two).
    Close,
}

/// The current answer with the bands the menu draws from it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listing<'a> {
    pub answer: &'a [TrackEntry],
    pub index: &'a Index,
    pub shape: Shape,
}

/// What the drop-down has to say, from the menu read alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuView<'a> {
    /// The current answer, its derived index, and the bands the menu draws.
    Bands(Listing<'a>),
    /// The answer was empty: one muted line, nothing to press (R13).
    Empty,
    /// The read failed: the same line with its own words (R13).
    Failed(Failure),
    /// Nothing true to say yet: idle, in flight, another query's answer, or
    /// blank text. There is no skeleton (R14).
    Nothing,
}

/// The results page's bands, and the whole answer they came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageListing<'a> {
    pub answer: &'a [TrackEntry],
    pub index: &'a Index,
    pub shape: Shape,
}

/// What the results page has to say, from the page read alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PageView<'a> {
    /// Nothing true to say yet: idle, in flight, or another query's answer.
    Waiting,
    /// The read failed.
    Failed(Failure),
    /// The answer was empty: the page's own two lines.
    Empty,
    /// The whole answer and the bands its blocks draw.
    Ready(PageListing<'a>),
}

/// The search field's state.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Search {
    /// The field's text, as typed. The text edit owns it; everything else reads
    /// its [`collapse`]d form.
    pub text: String,
    /// Whether the drop-down is drawn. Typing sets it; only the shell closes it,
    /// once per frame, after the field and the menu have both been drawn.
    open: bool,
    /// The marked row's ordinal in the walk, or `None` for the field (R1).
    mark: Option<usize>,
    /// The input that chose the mark, so the two highlight tiers never mix
    /// (R9).
    device: Device,
    /// The answer the menu derives its bands from.
    menu: Read,
    /// The whole answer the results page draws.
    page: Read,
    /// The key on the wire. One read at a time, so a burst of keystrokes cannot
    /// queue twenty requests.
    inflight: Option<String>,
    /// The artists' portraits both surfaces draw, and the read that fills them.
    portraits: Portraits,
    /// Whether a results page was open the last frame the shell asked. Set from
    /// [`Self::portraits_due`]'s `page` argument and cleared when it is `None`,
    /// so a reply that lands after the page is left is not taken for the page on
    /// screen.
    page_open: bool,
    /// How much of each block the page has been asked to show.
    reveal: Reveal,
}

impl Search {
    /// The query the field stands for, or `None` when it is blank.
    pub fn key(&self) -> Option<String> {
        collapse(&self.text)
    }

    /// Whether the drop-down is drawn.
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Which visit to the results page's query this is.
    ///
    /// It moves exactly when the page's reveal starts a query over (see
    /// [`Reveal::point`]), so a view keyed by it starts at the top on a fresh
    /// page and keeps its offset on a Retry or a re-entry of the same query.
    pub fn page_visit(&self) -> u32 {
        self.reveal.visit()
    }

    /// The marked row's ordinal in the walk, or `None` for the field.
    pub fn mark(&self) -> Option<usize> {
        self.mark
    }

    /// The current answer with its derived shape, or `None` when the menu read
    /// is not about the text as it stands.
    pub fn listing(&self) -> Option<Listing<'_>> {
        let key = collapse(&self.text)?;
        if self.menu.key.as_deref() != Some(key.as_str()) {
            return None;
        }
        let Load::Ready(answer) = &self.menu.load else {
            return None;
        };
        let index = self.menu.index.as_ref()?;
        Some(Listing {
            answer,
            index,
            shape: index.shape(MENU),
        })
    }

    /// The row the mark resolves to in the current shape, or `None` for the
    /// field and for a stale ordinal.
    pub fn marked(&self) -> Option<Row> {
        self.listing()?.shape.row(self.mark?)
    }

    /// The URI of derived album `index`, when the current answer has one.
    pub fn album_uri(&self, index: usize) -> Option<String> {
        self.listing()?
            .index
            .albums
            .get(index)
            .map(|album| album.uri.clone())
    }

    /// The URI of derived artist `index`, when the current answer has one.
    pub fn artist_uri(&self, index: usize) -> Option<String> {
        self.listing()?
            .index
            .artists
            .get(index)
            .map(|artist| artist.uri.clone())
    }

    /// What is known about `artist`'s portrait this frame.
    pub fn portrait(&self, artist: &str) -> Portrait<'_> {
        self.portraits.get(artist)
    }

    /// Which input last chose the mark.
    pub fn device(&self) -> Device {
        self.device
    }

    /// What the drop-down has to say this frame.
    pub fn menu_view(&self) -> MenuView<'_> {
        let Some(key) = collapse(&self.text) else {
            return MenuView::Nothing;
        };
        if self.menu.key.as_deref() != Some(key.as_str()) {
            return MenuView::Nothing;
        }
        match &self.menu.load {
            Load::Idle | Load::Loading => MenuView::Nothing,
            Load::Failed(failure) => MenuView::Failed(*failure),
            Load::Ready(tracks) if tracks.is_empty() => MenuView::Empty,
            Load::Ready(_) => match self.listing() {
                Some(listing) => MenuView::Bands(listing),
                None => MenuView::Nothing,
            },
        }
    }

    /// The whole answer the menu's query holds, or `None` when the menu has no
    /// answer for the text as it stands.
    ///
    /// [`Self::menu_view`] carries only the rows the menu draws; the queue is
    /// the whole answer the wire returned (R16), so the shell asks for it here
    /// rather than taking the menu's capped prefix.
    pub fn answer(&self) -> Option<&[TrackEntry]> {
        let key = collapse(&self.text)?;
        if self.menu.key.as_deref() != Some(key.as_str()) {
            return None;
        }
        match &self.menu.load {
            Load::Ready(tracks) => Some(tracks),
            Load::Idle | Load::Loading | Load::Failed(_) => None,
        }
    }

    /// The results page's load, when it is about `query`.
    ///
    /// `None` means the page is not holding this query's answer, so the view
    /// must not draw another query's tracks under this heading.
    pub fn page_answer(&self, query: &str) -> Option<&Load<Vec<TrackEntry>>> {
        if self.page.key.as_deref() == Some(query) {
            Some(&self.page.load)
        } else {
            None
        }
    }

    /// The results page's state for `query`, capped by what the reader has
    /// asked for.
    ///
    /// `None` means the page is not holding this query's answer, so the view
    /// must not draw another query's tracks under this heading. The bands come
    /// from the whole answer, never from the rows a block shows.
    pub fn page_view(&self, query: &str) -> Option<PageView<'_>> {
        let load = self.page_answer(query)?;
        Some(match load {
            Load::Idle | Load::Loading => PageView::Waiting,
            Load::Failed(failure) => PageView::Failed(*failure),
            Load::Ready(tracks) if tracks.is_empty() => PageView::Empty,
            Load::Ready(tracks) => {
                let index = self.page.index.as_ref()?;
                PageView::Ready(PageListing {
                    answer: tracks,
                    index,
                    shape: index.shape(self.reveal.caps(query, PAGE)),
                })
            }
        })
    }

    /// Takes `kind`'s reveal on the page `query`, if that page is the answer on
    /// screen. A late click on a page that has moved on changes nothing.
    ///
    /// Returns whether the reveal was taken, so the caller only moves the
    /// keyboard to a revealed row that actually exists.
    pub fn take_more(&mut self, query: &str, kind: Kind) -> bool {
        if self.page.key.as_deref() != Some(query) {
            return false;
        }
        if !matches!(self.page.load, Load::Ready(_)) {
            return false;
        }
        self.reveal.point(query);
        self.reveal.take(kind);
        true
    }

    /// Points the menu at the text as it now stands.
    ///
    /// An edit that moves the key drops the old answer, so a superseded reply
    /// can never be drawn (R14). A programmatic edit — the clear control — calls
    /// this too, because only the text edit itself reports `typed`.
    pub fn text_changed(&mut self) {
        let key = collapse(&self.text);
        Self::want(&mut self.menu, key);
    }

    /// Whether Escape has something to undo.
    ///
    /// The field consumes Escape only when this is true, so a drawer under an
    /// empty, closed search still gets its own Escape.
    pub fn escapes(&self) -> bool {
        self.mark.is_some() || self.open || !self.text.is_empty()
    }

    /// Routes one frame's keys and clicks while the field has the keyboard.
    ///
    /// The row count comes from [`Self::listing`], so the router works on what
    /// the current key draws. A frame that starts unfocused changes nothing
    /// (the field is not the one the user is typing into).
    ///
    /// The arrows only ever land on a row that exists:
    /// - With the menu closed and a query to re-open it on, Down and Up re-open
    ///   it and mark the first row (Down) or the last row (Up) in the same
    ///   keystroke. With the menu closed and a blank key they do nothing.
    /// - With the menu open and no rows to mark — idle, empty, failed — they do
    ///   nothing, so a mark is never a row that is not drawn.
    /// - Enter only acts on a row that is drawn: the menu must be open, and the
    ///   mark must resolve through the current shape.
    pub fn step(&mut self, input: KeyInput) -> Act {
        if !input.focused {
            return Act::None;
        }
        if input.typed || input.clicked_field {
            // R2: typing, and a click on the field, put the rule back on the
            // field and re-open the menu on the query it now stands for.
            self.mark = None;
            self.device = Device::Keyboard;
            self.text_changed();
            self.open = self.key().is_some();
        }
        // Read after the edit above: the frame that types has no rows yet.
        let rows = self.listing().map_or(0, |listing| listing.shape.len());
        if input.down {
            self.device = Device::Keyboard;
            if self.open {
                if rows > 0 {
                    self.mark = Some(match self.mark {
                        Some(index) => (index + 1).min(rows - 1),
                        None => 0,
                    });
                }
            } else if self.key().is_some() {
                // A closed menu is not drawn, so the arrow cannot move over it.
                // It re-opens on the query it stands for, and marks the row the
                // arrow would have landed on.
                self.open = true;
                self.mark = (rows > 0).then_some(0);
            }
        }
        if input.up {
            self.device = Device::Keyboard;
            if self.open {
                if rows > 0 {
                    self.mark = match self.mark {
                        None => Some(rows - 1),
                        // Up from the first row is the field: R1's mark lives on
                        // the rows and the field is where Enter opens the page.
                        Some(0) => None,
                        Some(index) => Some(index - 1),
                    };
                }
            } else if self.key().is_some() {
                self.open = true;
                self.mark = rows.checked_sub(1);
            }
        }
        if input.escape {
            if self.mark.is_some() {
                // R3 step one: back to the field, query kept, menu open.
                self.mark = None;
            } else if self.open || !self.text.is_empty() {
                // R3 step two: clear the query, which closes the menu.
                self.text.clear();
                self.text_changed();
                self.open = false;
                return Act::Close;
            }
        }
        if input.enter {
            // Only a drawn row can act: a closed menu draws nothing, and a
            // stale ordinal resolves to no row.
            let row = if self.open { self.marked() } else { None };
            match row {
                Some(Row::Track(index)) => return Act::Play(index),
                Some(Row::Album(index)) => return Act::OpenAlbum(index),
                Some(Row::Artist(index)) => return Act::OpenArtist(index),
                Some(Row::Floor) | None => {}
            }
            // The floor row, the field itself, and a mark left above a shrunken
            // list all open the page — if there is a query to open it with.
            if self.key().is_some() {
                return Act::OpenResults;
            }
        }
        Act::None
    }

    /// Closes the menu when the field has lost the keyboard and the pointer is
    /// on neither the field nor the menu.
    ///
    /// The shell is the only caller, once per frame, after both are drawn: the
    /// press that starts a row click may surrender the field's focus a frame
    /// before the release reports the click, and pointer geometry — not egui's
    /// surrender timing — is what keeps the menu alive across that frame.
    pub fn settle(&mut self, has_focus: bool, engaged: bool) {
        if !has_focus && !engaged {
            self.open = false;
            self.mark = None;
        }
    }

    /// Closes the menu after an action was applied, keeping the query.
    pub fn close(&mut self) {
        self.open = false;
        self.mark = None;
    }

    /// The pointer moved over the menu, so the pointer owns the highlight (R9).
    pub fn pointer_moved(&mut self) {
        self.device = Device::Pointer;
    }

    /// The read that is due now, if any.
    ///
    /// The menu asks while it is open; the page asks while it is the page on
    /// screen. Both need a key and an idle answer, and only one read may be on
    /// the wire at a time.
    fn due(&self, page: Option<&str>) -> Option<Target> {
        if self.inflight.is_some() {
            return None;
        }
        if self.open && self.menu.key.is_some() && self.menu.load == Load::Idle {
            return Some(Target::Menu);
        }
        self.due_page(page)
    }

    /// The page's read alone, without the menu's claim on the wire.
    ///
    /// [`Self::request_page`] is the one caller: a page control must be able to
    /// ask for the page's query while the menu is open and due.
    fn due_page(&self, page: Option<&str>) -> Option<Target> {
        if self.inflight.is_some() {
            return None;
        }
        let page = page?;
        if self.page.key.as_deref() == Some(page) && self.page.load == Load::Idle {
            Some(Target::Page)
        } else {
            None
        }
    }

    /// The search read this frame wants, if one is due (called from `logic`).
    ///
    /// `page` is the query of the open results page, passed in by the shell so
    /// this state never has to know what a page is.
    pub fn request(&mut self, page: Option<&str>) -> Option<Command> {
        let target = self.due(page)?;
        self.send(target)
    }

    /// The page's read, asked for even while the menu is the one `request`
    /// would take.
    ///
    /// Retry is the page's own control, so it asks for the page's query rather
    /// than whatever is due first; the page would otherwise stay `Idle` until a
    /// later frame. The wire and the sibling rule are the same as `request`'s.
    pub fn request_page(&mut self, page: Option<&str>) -> Option<Command> {
        let target = self.due_page(page)?;
        self.send(target)
    }

    /// Puts `target`'s read on the wire, unless its sibling already loads that
    /// key.
    fn send(&mut self, target: Target) -> Option<Command> {
        let (this, other) = match target {
            Target::Menu => (&mut self.menu, &self.page),
            Target::Page => (&mut self.page, &self.menu),
        };
        let key = this.key.clone()?;
        // The other read is already waiting on this key: wait with it instead of
        // putting a second identical read on the wire. The reply fills both.
        if matches!(other.load, Load::Loading) && other.key.as_deref() == Some(key.as_str()) {
            this.load = Load::Loading;
            return None;
        }
        this.load = Load::Loading;
        self.inflight = Some(key.clone());
        Some(Command::Search { query: key })
    }

    /// Whether [`Self::request`] or [`Self::request_portraits`] would send
    /// something now.
    ///
    /// It settles the portrait slot's bookkeeping (the key change and the
    /// artists on screen) but sends nothing; the window asks after a frame's
    /// edits and requests a repaint on true:
    /// `logic` runs before `ui`, so a read armed by that frame would otherwise
    /// wait for the next input event.
    pub fn wants(&mut self, page: Option<&str>) -> bool {
        if self.portraits_due(page).is_some() {
            return true;
        }
        let Some(target) = self.due(page) else {
            return false;
        };
        match target {
            Target::Menu => {
                !(matches!(self.page.load, Load::Loading) && self.page.key == self.menu.key)
            }
            Target::Page => {
                !(matches!(self.menu.load, Load::Loading) && self.menu.key == self.page.key)
            }
        }
    }

    /// Folds one worker reply into the reads.
    ///
    /// A reply fills every read waiting on its key, so the menu and the page can
    /// be answered by one read; anything else — another query, or a read that is
    /// not in flight — is dropped.
    pub fn apply(&mut self, update: &Update) {
        match update {
            Update::Search { query, tracks } => self.fill(query, &Load::Ready(tracks.clone())),
            Update::SearchFailed { query, failure } => self.fill(query, &Load::Failed(*failure)),
            Update::Portraits { .. } | Update::PortraitsFailed { .. } => {
                let page = self.page.key.clone().unwrap_or_default();
                self.portraits.set_shown(&self.portrait_windows(&page));
                let current = self.on_screen();
                self.portraits.apply(
                    update,
                    current
                        .as_ref()
                        .map(|(surface, key)| (*surface, key.as_str())),
                );
            }
            _ => {}
        }
    }

    /// The artists the menu's artist band is showing, in order.
    fn shown_artists(&self) -> Vec<String> {
        let Some(listing) = self.listing() else {
            return Vec::new();
        };
        Self::window(listing.index, &listing.shape)
    }

    /// The artists a shape's artist band shows, in order.
    fn window(index: &Index, shape: &Shape) -> Vec<String> {
        let Some(band) = shape.bands.iter().find(|band| band.kind == Kind::Artists) else {
            return Vec::new();
        };
        index
            .artists
            .iter()
            .take(band.shown)
            .map(|artist| artist.uri.clone())
            .collect()
    }

    /// The artists the page's artist band shows for `query`, in order.
    fn page_artists(&self, query: &str) -> Vec<String> {
        let Some(PageView::Ready(listing)) = self.page_view(query) else {
            return Vec::new();
        };
        Self::window(listing.index, &listing.shape)
    }

    /// The faces on screen this frame: the menu's key while it is open, else the
    /// page's.
    fn on_screen(&self) -> Option<(Surface, String)> {
        if self.open
            && let Some(key) = self.key()
        {
            return Some((Surface::Menu, key));
        }
        // The stored flag, not the page read's key: the read outlives the page
        // it was asked for, so a reply that lands after the page is left must
        // not be judged against it as if the page were still on screen.
        if !self.page_open {
            return None;
        }
        self.page.key.clone().map(|key| (Surface::Page, key))
    }

    /// The portrait read this frame would send, and the surface asking for it.
    ///
    /// The menu asks while it is open; the page asks while it is the page on
    /// screen and its answer is ready. The menu is never passed over for the
    /// page: an open menu with nothing due returns `None`.
    fn portraits_due(&mut self, page: Option<&str>) -> Option<Ask> {
        // The page is open exactly while the shell hands us its query. Recording
        // it here — where every ask already reads it — is what lets a reply
        // landed after the page is left be judged against nothing.
        self.page_open = page.is_some();
        let (surface, key, window) = if self.open && self.key().is_some() {
            let key = self.key()?;
            let window = self.shown_artists();
            (Surface::Menu, key, window)
        } else {
            let key = page?;
            let window = self.page_artists(key);
            (Surface::Page, key.to_owned(), window)
        };
        // The surface on screen owns the slot, so a changed one frees it for the
        // artists it is about to draw.
        self.portraits.note(Some((surface, key.as_str())));
        if window.is_empty() {
            return None;
        }
        self.portraits
            .set_shown(&self.portrait_windows(page.unwrap_or_default()));
        let refs: Vec<&str> = window.iter().map(String::as_str).collect();
        self.portraits.due(surface, &key, &refs)
    }

    /// Every artist either surface is showing, so a full cache keeps both.
    fn portrait_windows(&self, page: &str) -> Vec<String> {
        let mut artists = self.shown_artists();
        for artist in self.page_artists(page) {
            if !artists.contains(&artist) {
                artists.push(artist);
            }
        }
        artists
    }

    /// The portrait read this frame wants, if one is due.
    ///
    /// `page` is the query of the open results page, as [`Self::request`] takes
    /// it, so both surfaces ask through one slot.
    pub fn request_portraits(&mut self, page: Option<&str>) -> Option<Command> {
        let ask = self.portraits_due(page)?;
        Some(self.portraits.ask(ask))
    }

    /// Points the results page at `query`, seeding it from the menu when the
    /// menu already holds that answer.
    ///
    /// A failed menu is not retried here: Enter on a failed key is a deliberate
    /// action, so it re-asks exactly once. `Library::settle` is what keeps an
    /// answer that is already held and in flight.
    pub fn open_results(&mut self, query: &str) {
        self.reveal.point(query);
        if self.menu.key.as_deref() == Some(query)
            && let Load::Ready(tracks) = &self.menu.load
        {
            let mut page = Read {
                key: Some(query.to_owned()),
                ..Read::default()
            };
            page.settle(Load::Ready(tracks.clone()));
            self.page = page;
            return;
        }
        self.page = Read {
            key: Some(query.to_owned()),
            ..Read::default()
        };
    }

    /// Points the results page at `query` and drops whatever it held, for the
    /// page's own Retry.
    ///
    /// Unlike [`Self::open_results`] it never seeds from the menu: Retry is
    /// pressed on a failure, and the menu may hold a stale answer for the same
    /// key that must not be drawn again. The read is left `Idle`, so the next
    /// [`Self::request_page`] asks exactly once.
    pub fn retry_page(&mut self, query: &str) {
        self.reveal.point(query);
        self.page = Read {
            key: Some(query.to_owned()),
            ..Read::default()
        };
    }

    /// Points `read` at `key`, dropping its answer when the key moves.
    fn want(read: &mut Read, key: Option<String>) {
        if read.key != key {
            *read = Read {
                key,
                ..Read::default()
            };
        }
    }

    /// Fills every read waiting on `query`, and clears the wire marker when the
    /// echo is the key it was marked with.
    ///
    /// `Idle` counts as waiting, not only `Loading`: typing away and back puts
    /// the menu back to `Idle` on a key that is still on the wire, and Enter
    /// while the menu is loading leaves the page `Idle` on the same key. A
    /// matching reply is that read's answer in both cases. `Ready` and `Failed`
    /// are never overwritten by a late reply.
    fn fill(&mut self, query: &str, load: &Load<Vec<TrackEntry>>) {
        if self.inflight.as_deref() == Some(query) {
            self.inflight = None;
        }
        for read in [&mut self.menu, &mut self.page] {
            if read.key.as_deref() == Some(query) && matches!(read.load, Load::Loading | Load::Idle)
            {
                read.settle(load.clone());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(uri: &str) -> TrackEntry {
        TrackEntry {
            uri: uri.to_owned(),
            title: uri.to_owned(),
            artists: String::new(),
            album: None,
            album_uri: None,
            artist_uris: Vec::new(),
            artist_names: Vec::new(),
            added_at: None,
            duration_ms: None,
            named: true,
            cover: None,
        }
    }

    fn tracks(uris: &[&str]) -> Vec<TrackEntry> {
        uris.iter().map(|uri| track(uri)).collect()
    }

    fn reply(query: &str, uris: &[&str]) -> Update {
        Update::Search {
            query: query.to_owned(),
            tracks: tracks(uris),
        }
    }

    fn failed(query: &str) -> Update {
        Update::SearchFailed {
            query: query.to_owned(),
            failure: Failure::Search,
        }
    }

    fn search(query: &str) -> Command {
        Command::Search {
            query: query.to_owned(),
        }
    }

    fn rich(uri: &str, album: &str, artist: &str) -> TrackEntry {
        TrackEntry {
            uri: uri.to_owned(),
            title: uri.to_owned(),
            artists: artist.to_owned(),
            album: Some(album.to_owned()),
            album_uri: Some(format!("spotify:album:{album}")),
            artist_uris: vec![format!("spotify:artist:{artist}")],
            artist_names: vec![artist.to_owned()],
            added_at: None,
            duration_ms: None,
            named: true,
            cover: None,
        }
    }

    fn rich_reply(query: &str, count: usize) -> Update {
        Update::Search {
            query: query.to_owned(),
            tracks: (0..count)
                .map(|index| {
                    rich(
                        &format!("t{index}"),
                        &format!("a{index}"),
                        &format!("r{index}"),
                    )
                })
                .collect(),
        }
    }

    /// The field with an answer for its key, open as typing leaves it.
    fn answered_open(text: &str, uris: &[&str]) -> Search {
        let mut state = typed(text);
        state.apply(&reply(text, uris));
        state
    }

    fn banded() -> Search {
        let mut state = typed("abc");
        state.apply(&rich_reply("abc", 8));
        state
    }

    fn no_floor_reply(query: &str) -> Update {
        Update::Search {
            query: query.to_owned(),
            tracks: vec![
                rich("t0", "a0", "r0"),
                rich("t1", "a1", "r1"),
                rich("t2", "a1", "r1"),
            ],
        }
    }

    /// Types `text` into the field, as one frame of the real path would.
    fn type_text(state: &mut Search, text: &str) {
        state.text = text.to_owned();
        let input = KeyInput {
            typed: true,
            focused: true,
            ..KeyInput::default()
        };
        assert_eq!(
            state.step(input),
            Act::None,
            "typing only changes the field"
        );
    }

    /// Types `text` and hands back the state the field would be left in.
    fn typed(text: &str) -> Search {
        let mut state = Search::default();
        type_text(&mut state, text);
        state
    }

    /// A frame with the field focused and only `key` pressed.
    fn key(name: &str) -> KeyInput {
        KeyInput {
            down: name == "down",
            up: name == "up",
            enter: name == "enter",
            escape: name == "escape",
            typed: false,
            clicked_field: false,
            focused: true,
        }
    }

    #[test]
    fn collapse_trims_and_joins_runs_of_whitespace() {
        assert_eq!(
            collapse("  never   gonna  "),
            Some("never gonna".to_owned())
        );
        assert_eq!(collapse("never\tgonna"), Some("never gonna".to_owned()));
        assert_eq!(collapse(" a\nb "), Some("a b".to_owned()));
        assert_eq!(collapse("a"), Some("a".to_owned()));
        assert_eq!(collapse(""), None);
        assert_eq!(collapse("   "), None);
    }

    #[test]
    fn a_burst_of_keystrokes_sends_one_command() {
        let mut state = Search::default();
        type_text(&mut state, "a");
        assert_eq!(state.request(None), Some(search("a")));
        type_text(&mut state, "ab");
        type_text(&mut state, "abc");
        assert_eq!(
            state.request(None),
            None,
            "one read is on the wire at a time"
        );

        // The answer for the superseded "a" lands: it fills nothing, and the
        // next request is for the query the user has actually typed.
        state.apply(&reply("a", &["spotify:track:a"]));
        assert_eq!(
            state.menu_view(),
            MenuView::Nothing,
            "a stale answer is never drawn"
        );
        assert_eq!(state.request(None), Some(search("abc")));

        // An "ab" answer cannot fill "abc": the key is part of the guard.
        state.apply(&reply("ab", &["spotify:track:ab"]));
        assert_eq!(state.menu_view(), MenuView::Nothing);
        state.apply(&reply("abc", &["spotify:track:abc"]));
        let answer = tracks(&["spotify:track:abc"]);
        let listing = state.listing().expect("the current answer has a shape");
        assert_eq!(listing.answer, answer.as_slice());
        assert_eq!(listing.shape, Index::derive(&answer).shape(MENU));
    }

    #[test]
    fn a_reply_fills_both_reads_when_the_keys_match() {
        let mut state = typed("abc");
        state.open_results("abc");
        // The coalescing path: both reads are waiting on the same key.
        state.menu = Read {
            key: Some("abc".to_owned()),
            load: Load::Loading,
            index: None,
        };
        state.page = Read {
            key: Some("abc".to_owned()),
            load: Load::Loading,
            index: None,
        };
        state.inflight = Some("abc".to_owned());

        state.apply(&reply("abc", &["spotify:track:a"]));

        assert_eq!(
            state.menu.load,
            Load::Ready(tracks(&["spotify:track:a"])),
            "the menu takes the answer"
        );
        assert_eq!(
            state.page.load,
            Load::Ready(tracks(&["spotify:track:a"])),
            "and so does the page, from the one read"
        );
        assert_eq!(state.inflight, None, "the echo clears the wire marker");
    }

    #[test]
    fn typing_away_and_back_fills_the_reply_that_was_still_on_the_wire() {
        let mut state = typed("abc");
        assert_eq!(state.request(None), Some(search("abc")));
        // On to another key and back: `want` puts the menu back to `Idle` on
        // "abc", and the "abc" read is still the one on the wire.
        type_text(&mut state, "abcd");
        type_text(&mut state, "abc");
        assert_eq!(state.menu.load, Load::Idle);
        assert_eq!(state.inflight.as_deref(), Some("abc"));

        state.apply(&reply("abc", &["spotify:track:a"]));

        let answer = tracks(&["spotify:track:a"]);
        let listing = state.listing().expect("the current answer has a shape");
        assert_eq!(
            listing.answer,
            answer.as_slice(),
            "the reply fills the idle read on its own key"
        );
        assert_eq!(state.inflight, None, "the reply was used, not orphaned");
        assert_eq!(state.request(None), None, "and is not asked for again");
    }

    #[test]
    fn enter_while_the_menu_is_loading_still_fills_the_page() {
        let mut state = typed("abc");
        assert_eq!(state.request(None), Some(search("abc")));
        assert_eq!(state.step(key("enter")), Act::OpenResults);
        state.open_results("abc");
        assert_eq!(
            state.page_answer("abc"),
            Some(&Load::Idle),
            "a loading menu seeds nothing"
        );

        state.apply(&reply("abc", &["spotify:track:a"]));

        assert_eq!(
            state.page_answer("abc"),
            Some(&Load::Ready(tracks(&["spotify:track:a"]))),
            "the one read answers the page the menu had not answered yet"
        );
        assert_eq!(
            state.request(Some("abc")),
            None,
            "so the query is not fetched a second time"
        );
    }

    #[test]
    fn a_failure_clears_the_wire_marker_for_its_key() {
        let mut state = typed("abc");
        assert_eq!(state.request(None), Some(search("abc")));
        assert_eq!(state.inflight.as_deref(), Some("abc"));

        state.apply(&failed("abc"));

        assert_eq!(state.inflight, None, "the failed echo clears the wire");
        assert_eq!(state.menu_view(), MenuView::Failed(Failure::Search));
    }

    #[test]
    fn a_failure_for_a_superseded_key_leaves_the_newer_read_alone() {
        let mut state = typed("abcd");
        // The wire is already working on the key the user is on now.
        state.menu = Read {
            key: Some("abcd".to_owned()),
            load: Load::Loading,
            index: None,
        };
        state.inflight = Some("abcd".to_owned());

        state.apply(&failed("abc"));

        assert_eq!(
            state.menu,
            Read {
                key: Some("abcd".to_owned()),
                load: Load::Loading,
                index: None,
            },
            "a stale failure is not drawn and does not fail the new key"
        );
        assert_eq!(
            state.inflight.as_deref(),
            Some("abcd"),
            "the newer read is still on the wire"
        );
    }

    #[test]
    fn wants_agrees_with_request_over_the_four_load_pairs() {
        // (menu load, page load, the page holds the menu's key, a read goes out)
        let cases = [
            (Load::Idle, Load::Idle, true, true),
            (Load::Idle, Load::Idle, false, true),
            (Load::Idle, Load::Loading, true, false),
            (Load::Idle, Load::Loading, false, true),
            (Load::Loading, Load::Idle, true, false),
            (Load::Loading, Load::Idle, false, false),
            (Load::Loading, Load::Loading, true, false),
            (Load::Loading, Load::Loading, false, false),
        ];
        for (menu, page, same_key, due) in cases {
            let mut state = typed("abc");
            state.menu = Read {
                key: Some("abc".to_owned()),
                load: menu.clone(),
                index: None,
            };
            state.page = Read {
                key: Some(if same_key { "abc" } else { "xyz" }.to_owned()),
                load: page.clone(),
                index: None,
            };

            let case = format!("menu {menu:?}, page {page:?}, same key: {same_key}");
            assert_eq!(state.wants(Some("abc")), due, "wants: {case}");
            assert_eq!(
                state.request(Some("abc")),
                due.then(|| search("abc")),
                "request: {case}"
            );
        }
    }

    #[test]
    fn a_sibling_read_on_the_same_key_waits_instead_of_asking_again() {
        // Rule 3: the other read is loading the same key, so this one joins it
        // and nothing goes on the wire.
        let mut state = typed("abc");
        state.page = Read {
            key: Some("abc".to_owned()),
            load: Load::Loading,
            index: None,
        };

        assert_eq!(state.request(None), None);
        assert_eq!(state.menu.load, Load::Loading, "the menu waits with it");
        assert_eq!(state.inflight, None, "nothing was sent");
    }

    /// A page control asks for the page's query even while the menu is open and
    /// due: `request` would take the menu's read first, leaving the page idle.
    #[test]
    fn request_page_asks_for_the_page_even_when_the_menu_is_due() {
        let mut state = typed("abc");
        state.open_results("page");
        assert!(state.is_open(), "the menu is open on the typed key");

        assert_eq!(state.request_page(Some("page")), Some(search("page")));
        assert_eq!(state.page.load, Load::Loading);
        assert_eq!(state.menu.load, Load::Idle, "the menu was not the one sent");
        assert_eq!(state.request_page(Some("page")), None, "one read at a time");
    }

    /// The page-only path keeps the sibling rule: a menu already loading the
    /// page's key is waited on, not asked twice.
    #[test]
    fn request_page_waits_for_a_sibling_loading_its_key() {
        let mut state = typed("abc");
        state.open_results("abc");
        state.menu = Read {
            key: Some("abc".to_owned()),
            load: Load::Loading,
            index: None,
        };

        assert_eq!(state.request_page(Some("abc")), None);
        assert_eq!(state.page.load, Load::Loading, "the page waits with it");
        assert_eq!(state.inflight, None, "nothing was sent");
    }

    #[test]
    fn a_failure_is_not_retried_until_the_key_changes() {
        let mut state = typed("abc");
        assert_eq!(state.request(None), Some(search("abc")));
        state.apply(&failed("abc"));
        assert_eq!(state.menu_view(), MenuView::Failed(Failure::Search));

        assert_eq!(state.request(None), None, "a failed key is not re-asked");
        type_text(&mut state, "abcd");
        assert_eq!(
            state.request(None),
            Some(search("abcd")),
            "typing is the retry"
        );
    }

    #[test]
    fn empty_text_and_focus_alone_send_nothing() {
        let mut state = Search::default();
        let focused = KeyInput {
            focused: true,
            ..KeyInput::default()
        };
        assert_eq!(state.step(focused), Act::None);
        assert_eq!(state.request(None), None);
        assert!(!state.is_open(), "focus alone opens nothing (R6)");

        // Spaces are not a query either.
        type_text(&mut state, "   ");
        assert_eq!(state.request(None), None);
        assert!(!state.is_open());
    }

    #[test]
    fn a_ready_menu_seeds_the_page_without_a_command() {
        let mut state = typed("abc");
        assert_eq!(state.request(None), Some(search("abc")));
        state.apply(&reply("abc", &["spotify:track:a", "spotify:track:b"]));

        state.open_results("abc");
        assert_eq!(
            state.page_answer("abc"),
            Some(&Load::Ready(tracks(&[
                "spotify:track:a",
                "spotify:track:b"
            ])))
        );
        assert_eq!(
            state.request(Some("abc")),
            None,
            "seeded, so no second read"
        );
    }

    #[test]
    fn enter_after_a_failed_menu_asks_exactly_once() {
        let mut state = typed("abc");
        assert_eq!(state.request(None), Some(search("abc")));
        state.apply(&failed("abc"));

        state.open_results("abc");
        assert_eq!(state.page_answer("abc"), Some(&Load::Idle), "not seeded");
        assert_eq!(state.request(Some("abc")), Some(search("abc")));
        assert_eq!(
            state.request(Some("abc")),
            None,
            "the retry is the one read, not a loop"
        );
    }

    #[test]
    fn a_menu_answer_for_another_key_does_not_seed_the_page() {
        let mut state = typed("abc");
        assert_eq!(state.request(None), Some(search("abc")));
        state.apply(&reply("abc", &["spotify:track:a"]));
        // The user typed on, so the held answer is for a key nobody is on.
        type_text(&mut state, "abcd");

        state.open_results("abcd");
        assert_eq!(state.page_answer("abcd"), Some(&Load::Idle));
        assert_eq!(state.request(Some("abcd")), Some(search("abcd")));
    }

    #[test]
    fn retry_page_never_seeds_from_the_menu() {
        let mut state = typed("abc");
        assert_eq!(state.request(None), Some(search("abc")));
        state.apply(&reply("abc", &["spotify:track:a"]));
        // The menu still holds this key's answer while the page has failed.
        state.open_results("abc");
        state.retry_page("abc");
        assert_eq!(state.request(Some("abc")), Some(search("abc")));
        state.apply(&failed("abc"));
        assert_eq!(
            state.page_answer("abc"),
            Some(&Load::Failed(Failure::Search))
        );

        state.retry_page("abc");
        assert_eq!(
            state.page_answer("abc"),
            Some(&Load::Idle),
            "a retry on a failure must not draw the menu's old answer"
        );
        assert_eq!(state.request(Some("abc")), Some(search("abc")));
    }

    #[test]
    fn wants_is_true_exactly_when_request_would_send() {
        let mut state = Search::default();
        assert!(!state.wants(None));

        type_text(&mut state, "a");
        assert!(state.wants(None));
        assert_eq!(state.request(None), Some(search("a")));
        assert!(!state.wants(None), "already on the wire");

        state.apply(&reply("a", &["spotify:track:a"]));
        assert!(!state.wants(None), "answered");
        assert!(!state.wants(Some("a")), "the menu holds it and is not open");

        type_text(&mut state, "ab");
        assert!(state.wants(Some("ab")), "a page with no answer asks");
        assert_eq!(state.request(Some("ab")), Some(search("ab")));
    }

    #[test]
    fn down_marks_the_first_row_from_the_field() {
        let mut state = answered_open("abc", &["a", "b", "c"]);
        assert_eq!(state.mark(), None, "the menu opens unmarked (R1)");
        assert_eq!(state.step(key("down")), Act::None);
        assert_eq!(state.mark(), Some(0));
        assert_eq!(state.device(), Device::Keyboard);
    }

    #[test]
    fn down_clamps_at_the_last_row() {
        let mut state = answered_open("abc", &["a", "b", "c"]);
        for expected in [0, 1, 2, 2, 2] {
            state.step(key("down"));
            assert_eq!(state.mark(), Some(expected));
        }
        let mut with_floor = answered_open("abc", &["a", "b", "c", "d"]);
        for _ in 0..9 {
            with_floor.step(key("down"));
        }
        assert_eq!(with_floor.mark(), Some(3), "three rows and a floor");
    }

    #[test]
    fn up_from_the_first_row_returns_to_the_field() {
        let mut state = answered_open("abc", &["a", "b", "c"]);
        state.step(key("down"));
        assert_eq!(state.mark(), Some(0));
        assert_eq!(state.step(key("up")), Act::None);
        assert_eq!(state.mark(), None, "row 0 is one press from the field");
    }

    #[test]
    fn up_from_the_field_marks_the_last_row() {
        let mut state = answered_open("abc", &["a", "b", "c"]);
        state.step(key("up"));
        assert_eq!(state.mark(), Some(2));

        let mut with_floor = answered_open("abc", &["a", "b", "c", "d"]);
        with_floor.step(key("up"));
        assert_eq!(
            with_floor.mark(),
            Some(3),
            "the floor is drawn, so it is last"
        );
    }

    #[test]
    fn the_walk_crosses_tracks_then_albums_then_artists_then_the_floor() {
        let mut state = banded();
        let expected = [
            Row::Track(0),
            Row::Track(1),
            Row::Track(2),
            Row::Album(0),
            Row::Album(1),
            Row::Artist(0),
            Row::Artist(1),
            Row::Floor,
        ];
        for (ordinal, row) in expected.into_iter().enumerate() {
            state.step(key("down"));
            assert_eq!(state.mark(), Some(ordinal));
            assert_eq!(state.marked(), Some(row), "ordinal {ordinal}");
        }
        state.step(key("down"));
        assert_eq!(state.mark(), Some(7), "the floor is the last row");
    }

    #[test]
    fn without_a_floor_the_walk_ends_on_the_last_shown_row() {
        let mut state = typed("abc");
        state.apply(&no_floor_reply("abc"));
        for _ in 0..9 {
            state.step(key("down"));
        }
        assert_eq!(state.marked(), Some(Row::Artist(1)));
        assert_eq!(state.mark(), Some(6));
    }

    #[test]
    fn enter_on_a_track_plays_its_whole_answer_index() {
        let mut state = answered_open("abc", &["a", "b", "c"]);
        state.step(key("down"));
        state.step(key("down"));
        assert_eq!(state.step(key("enter")), Act::Play(1));
    }

    #[test]
    fn enter_on_an_album_and_an_artist_opens_the_derived_row() {
        let mut state = banded();
        for _ in 0..4 {
            state.step(key("down"));
        }
        assert_eq!(state.marked(), Some(Row::Album(0)));
        assert_eq!(state.step(key("enter")), Act::OpenAlbum(0));

        let mut state = banded();
        for _ in 0..6 {
            state.step(key("down"));
        }
        assert_eq!(state.marked(), Some(Row::Artist(0)));
        assert_eq!(state.step(key("enter")), Act::OpenArtist(0));
    }

    #[test]
    fn enter_on_the_floor_opens_the_results() {
        let mut state = answered_open("abc", &["a", "b", "c", "d"]);
        for _ in 0..6 {
            state.step(key("down"));
        }
        assert_eq!(state.marked(), Some(Row::Floor));
        assert_eq!(state.step(key("enter")), Act::OpenResults);
    }

    #[test]
    fn a_floor_does_not_exist_when_no_band_is_capped() {
        let mut state = typed("abc");
        state.apply(&no_floor_reply("abc"));
        for _ in 0..12 {
            state.step(key("down"));
        }
        assert_eq!(
            state.marked(),
            Some(Row::Artist(1)),
            "the walk never lands on a floor"
        );
        assert!(state.listing().is_some_and(|listing| !listing.shape.floor));
    }

    #[test]
    fn enter_on_a_stale_ordinal_opens_the_results() {
        let mut state = banded();
        state.step(key("down"));
        state.step(key("down"));
        state.step(key("down"));
        state.step(key("down"));
        assert_eq!(state.marked(), Some(Row::Album(0)));
        // The key moves and the new answer is too short for the old ordinal.
        type_text(&mut state, "abcd");
        state.apply(&rich_reply("abcd", 1));
        state.mark = Some(3);
        assert_eq!(state.mark(), Some(3));
        assert_eq!(state.marked(), None);
        assert_eq!(state.step(key("enter")), Act::OpenResults);
    }

    #[test]
    fn a_stale_key_counts_zero_rows_and_opens_nothing() {
        let mut state = answered_open("abc", &["a", "b", "c"]);
        state.step(key("down"));
        assert_eq!(state.mark(), Some(0));

        // The frame that types moves the key before the arrow is read.
        state.text = "abcd".to_owned();
        let typed_and_down = KeyInput {
            typed: true,
            down: true,
            focused: true,
            ..KeyInput::default()
        };
        assert_eq!(state.step(typed_and_down), Act::None);
        assert_eq!(state.mark(), None, "the stale answer contributes no rows");
    }

    #[test]
    fn album_and_artist_uris_resolve_only_for_the_current_key() {
        let mut state = banded();
        assert_eq!(state.album_uri(0).as_deref(), Some("spotify:album:a0"));
        assert_eq!(state.artist_uri(0).as_deref(), Some("spotify:artist:r0"));
        assert_eq!(state.album_uri(9), None, "out of range");
        assert_eq!(state.artist_uri(9), None, "out of range");

        type_text(&mut state, "abcd");
        assert_eq!(state.album_uri(0), None, "the key moved");
        assert_eq!(state.artist_uri(0), None, "the key moved");
    }

    #[test]
    fn down_in_the_frame_that_types_is_a_no_op() {
        let mut state = answered_open("abc", &["a", "b", "c"]);
        let frame = KeyInput {
            typed: true,
            down: true,
            focused: true,
            ..KeyInput::default()
        };
        state.text = "abcd".to_owned();
        assert_eq!(state.step(frame), Act::None);
        assert_eq!(state.mark(), None);
        assert_eq!(state.menu_view(), MenuView::Nothing);
    }

    /// The field with an answered menu, closed again as an applied action
    /// leaves it.
    fn answered_closed(text: &str, uris: &[&str]) -> Search {
        let mut state = answered_open(text, uris);
        state.close();
        assert!(!state.is_open());
        state
    }

    #[test]
    fn down_on_a_closed_menu_reopens_it_and_marks_the_first_row() {
        let mut state = answered_closed("abc", &["a", "b", "c"]);
        assert_eq!(state.step(key("down")), Act::None);
        assert!(state.is_open(), "the arrow re-opens the menu it acts on");
        assert_eq!(state.mark(), Some(0));
        assert_eq!(state.device(), Device::Keyboard);
    }

    #[test]
    fn up_on_a_closed_menu_reopens_it_and_marks_the_last_row() {
        let mut state = answered_closed("abc", &["a", "b", "c"]);
        assert_eq!(state.step(key("up")), Act::None);
        assert!(state.is_open());
        assert_eq!(state.mark(), Some(2));

        let mut with_floor = answered_closed("abc", &["a", "b", "c", "d"]);
        with_floor.step(key("up"));
        assert_eq!(with_floor.mark(), Some(3), "the floor is the last row");
    }

    #[test]
    fn an_arrow_on_a_closed_blank_field_does_nothing() {
        let mut blank = Search::default();
        assert_eq!(blank.step(key("down")), Act::None);
        assert_eq!(blank.step(key("up")), Act::None);
        assert!(!blank.is_open(), "a blank key re-opens nothing");
        assert_eq!(blank.mark(), None);

        // Whitespace is not a key either.
        let mut spaces = typed("   ");
        assert_eq!(spaces.step(key("down")), Act::None);
        assert_eq!(spaces.step(key("up")), Act::None);
        assert!(!spaces.is_open());
        assert_eq!(spaces.mark(), None);
    }

    #[test]
    fn an_arrow_marks_nothing_when_no_row_is_drawn() {
        // Open, and the answer is still on the wire: nothing to mark.
        let mut loading = typed("abc");
        assert_eq!(loading.request(None), Some(search("abc")));
        assert_eq!(loading.step(key("down")), Act::None);
        assert_eq!(loading.mark(), None);
        assert_eq!(loading.step(key("up")), Act::None);
        assert_eq!(loading.mark(), None);

        // Open and failed: nothing to mark.
        loading.apply(&failed("abc"));
        assert_eq!(loading.step(key("down")), Act::None);
        assert_eq!(loading.mark(), None);

        // Open and empty: nothing to mark.
        let mut empty = typed("abc");
        empty.apply(&reply("abc", &[]));
        assert_eq!(empty.step(key("down")), Act::None);
        assert_eq!(empty.mark(), None);

        // Closed with a query but no answer to draw: it re-opens, marks
        // nothing, because a mark must be a row that exists.
        let mut closed = typed("abc");
        closed.close();
        closed.step(key("down"));
        assert!(closed.is_open());
        assert_eq!(closed.mark(), None);
    }

    #[test]
    fn enter_after_close_is_the_fields_enter_not_a_play() {
        let mut state = answered_closed("abc", &["a", "b", "c"]);
        // A mark from before the close must not survive into an Enter.
        state.step(key("down"));
        assert_eq!(state.mark(), Some(0));
        state.close();
        assert_eq!(state.mark(), None);
        assert_eq!(state.step(key("enter")), Act::OpenResults);
    }

    #[test]
    fn enter_on_the_field_opens_the_results() {
        let mut state = answered_open("abc", &["a", "b", "c"]);
        assert_eq!(state.mark(), None);
        assert_eq!(state.step(key("enter")), Act::OpenResults);
    }

    #[test]
    fn enter_with_a_blank_key_does_nothing() {
        let mut state = typed("   ");
        assert_eq!(state.step(key("enter")), Act::None);
        // A mark left behind a blanked field opens nothing either.
        state.step(key("down"));
        assert_eq!(state.step(key("enter")), Act::None);
    }

    #[test]
    fn escape_from_a_row_keeps_the_query_and_the_menu() {
        let mut state = answered_open("abc", &["a", "b", "c"]);
        state.step(key("down"));
        assert_eq!(state.step(key("escape")), Act::None);
        assert_eq!(state.mark(), None);
        assert_eq!(state.text, "abc", "the query is kept (R3)");
        assert!(state.is_open(), "and the menu stays up");
        assert!(state.escapes(), "Escape still has something to undo");
    }

    #[test]
    fn escape_from_the_field_clears_the_query_and_closes_the_menu() {
        let mut state = typed("abc");
        assert_eq!(state.step(key("escape")), Act::Close);
        assert_eq!(state.text, "");
        assert!(!state.is_open());
        assert_eq!(state.mark(), None);
        assert!(!state.escapes(), "nothing left to undo");
    }

    #[test]
    fn escape_with_an_empty_closed_field_is_not_ours() {
        let mut state = Search::default();
        assert!(!state.escapes());
        assert_eq!(state.step(key("escape")), Act::None);
        assert_eq!(state.text, "");
        assert!(!state.is_open());
    }

    #[test]
    fn a_frame_that_starts_unfocused_does_nothing() {
        let mut state = answered_open("abc", &["a", "b", "c"]);
        state.step(key("down"));
        let blurred = KeyInput {
            focused: false,
            ..KeyInput::default()
        };
        assert_eq!(state.step(blurred), Act::None);
        assert_eq!(state.mark(), Some(0), "the mark is the shell's to clear");

        let blurred_escape = KeyInput {
            escape: true,
            focused: false,
            ..KeyInput::default()
        };
        assert_eq!(state.step(blurred_escape), Act::None);
        assert_eq!(state.text, "abc", "a blurred field does not eat keys");
        assert!(state.is_open());
    }

    #[test]
    fn typing_returns_the_rule_to_the_field() {
        let mut state = answered_open("abc", &["a", "b", "c"]);
        state.step(key("down"));
        assert_eq!(state.mark(), Some(0));
        type_text(&mut state, "abcd");
        assert_eq!(state.mark(), None, "R2: typing puts the rule back");
        assert!(state.is_open());
    }

    #[test]
    fn a_click_on_the_field_returns_the_rule_and_keeps_the_menu() {
        let mut state = answered_open("abc", &["a", "b", "c"]);
        state.pointer_moved();
        state.step(key("down"));
        assert_eq!(state.device(), Device::Keyboard);
        state.settle(false, false);
        assert!(!state.is_open(), "the menu closes when the field is left");

        let click = KeyInput {
            clicked_field: true,
            focused: true,
            ..KeyInput::default()
        };
        assert_eq!(state.step(click), Act::None);
        state.pointer_moved();
        assert!(
            state.is_open(),
            "a click re-opens it on the same query (R2)"
        );
        assert_eq!(state.mark(), None);
    }

    #[test]
    fn settle_closes_the_menu_only_when_nothing_is_engaged() {
        let mut state = typed("abc");
        state.settle(true, false);
        assert!(state.is_open(), "a focused field keeps the menu");

        state.settle(false, true);
        assert!(state.is_open(), "the pointer on the menu keeps it");

        state.settle(false, false);
        assert!(!state.is_open());
        assert_eq!(state.mark(), None);

        // A closed menu is not disturbed by a later settled frame.
        state.settle(false, false);
        assert!(!state.is_open());
    }

    #[test]
    fn closing_an_applied_action_keeps_the_query() {
        let mut state = answered_open("abc", &["a", "b", "c"]);
        state.step(key("down"));
        state.close();
        assert!(!state.is_open());
        assert_eq!(state.mark(), None);
        assert_eq!(state.text, "abc");
    }

    fn portrait_command(query: &str, artists: &[&str]) -> Command {
        Command::LoadPortraits {
            query: query.to_owned(),
            artists: artists.iter().map(|artist| (*artist).to_owned()).collect(),
        }
    }

    fn portrait(artist: &str, cover: Option<&str>) -> Update {
        Update::Portraits {
            query: "abc".to_owned(),
            portraits: vec![takofy_model::ArtistPortrait {
                artist: artist.to_owned(),
                portrait: cover.map(|tail| {
                    takofy_model::CoverUrl::from_wire(&format!("https://i.scdn.co/image/{tail}"))
                        .expect("a cover url")
                }),
            }],
        }
    }

    /// The page ready on `abc` with `count` distinct artists, and the menu
    /// closed, as landing on the page leaves it.
    fn paged(count: usize) -> Search {
        let mut state = typed("abc");
        state.open_results("abc");
        state.apply(&rich_reply("abc", count));
        state.close();
        state
    }

    fn artist(n: usize) -> String {
        format!("spotify:artist:r{n}")
    }

    fn uris(range: std::ops::Range<usize>) -> Vec<String> {
        range.map(artist).collect()
    }

    #[test]
    fn no_portraits_are_asked_for_while_the_menu_has_nothing_to_draw() {
        // Loading, not open, blank, and answering another key.
        let mut loading = typed("abc");
        assert_eq!(loading.request(None), Some(search("abc")));
        assert_eq!(loading.request_portraits(None), None);

        let mut closed = banded();
        closed.close();
        assert_eq!(closed.request_portraits(None), None);

        let mut blank = banded();
        blank.text.clear();
        blank.text_changed();
        assert_eq!(blank.request_portraits(None), None);

        let mut moved = banded();
        type_text(&mut moved, "abcd");
        assert_eq!(moved.request_portraits(None), None);
    }

    #[test]
    fn the_menu_asks_for_its_two_artists_in_band_order() {
        let mut state = banded();
        assert_eq!(
            state.request_portraits(None),
            Some(portrait_command(
                "abc",
                &["spotify:artist:r0", "spotify:artist:r1"]
            ))
        );
        assert_eq!(state.request_portraits(None), None, "one read at a time");
    }

    #[test]
    fn the_closed_menu_leaves_the_page_asking_for_its_ten() {
        let mut state = paged(25);
        assert!(!state.is_open());
        let expected: Vec<String> = uris(0..10);
        let expected: Vec<&str> = expected.iter().map(String::as_str).collect();
        assert_eq!(
            state.request_portraits(Some("abc")),
            Some(portrait_command("abc", &expected))
        );
    }

    #[test]
    fn an_open_menu_with_nothing_due_never_falls_through_to_the_page() {
        let mut state = banded();
        assert!(state.request_portraits(None).is_some());
        state.apply(&portrait("spotify:artist:r0", Some("0")));
        state.apply(&portrait("spotify:artist:r1", Some("1")));
        assert_eq!(
            state.request_portraits(Some("abc")),
            None,
            "the menu's two are answered, so no read is due"
        );
    }

    #[test]
    fn a_loading_failed_or_empty_page_asks_for_no_face() {
        let mut loading = typed("abc");
        loading.open_results("page");
        assert_eq!(loading.page_view("page"), Some(PageView::Waiting));
        assert_eq!(loading.request_portraits(Some("page")), None);

        let mut failed = typed("abc");
        failed.open_results("page");
        failed.apply(&Update::SearchFailed {
            query: "page".to_owned(),
            failure: Failure::Search,
        });
        assert_eq!(
            failed.page_view("page"),
            Some(PageView::Failed(Failure::Search))
        );
        assert_eq!(failed.request_portraits(Some("page")), None);

        let mut empty = typed("abc");
        empty.open_results("page");
        empty.apply(&reply("page", &[]));
        assert_eq!(empty.page_view("page"), Some(PageView::Empty));
        assert_eq!(empty.request_portraits(Some("page")), None);
    }

    #[test]
    fn a_newer_query_never_asks_for_the_previous_answers_artists() {
        let mut state = banded();
        assert!(state.request_portraits(None).is_some());
        state.apply(&portrait("spotify:artist:r0", Some("old")));
        type_text(&mut state, "abcd");
        state.apply(&Update::Search {
            query: "abcd".to_owned(),
            tracks: (0..8)
                .map(|index| {
                    rich(
                        &format!("u{index}"),
                        &format!("b{index}"),
                        &format!("s{index}"),
                    )
                })
                .collect(),
        });
        // The new answer's own first two artists, never the held answer's.
        assert_eq!(
            state.request_portraits(None),
            Some(portrait_command(
                "abcd",
                &["spotify:artist:s0", "spotify:artist:s1"]
            ))
        );
    }

    #[test]
    fn a_newer_page_query_never_asks_for_the_previous_answers_artists() {
        let mut state = paged(25);
        assert!(state.request_portraits(Some("abc")).is_some());
        state.apply(&Update::Search {
            query: "abc".to_owned(),
            tracks: Vec::new(),
        });
        // A page for another query is the one on screen; the old answer's faces
        // are not asked for.
        state.open_results("xy");
        assert_eq!(state.request_portraits(Some("xy")), None);
    }

    #[test]
    fn wants_is_true_exactly_when_request_portraits_would_send() {
        let mut state = banded();
        assert!(state.wants(None));
        assert!(state.request_portraits(None).is_some());
        assert!(!state.wants(None));
        assert_eq!(state.request_portraits(None), None);

        let mut page = paged(25);
        assert!(page.wants(Some("abc")));
        assert!(page.request_portraits(Some("abc")).is_some());
        assert!(!page.wants(Some("abc")));
        assert_eq!(page.request_portraits(Some("abc")), None);
    }

    #[test]
    fn an_artist_met_in_a_previous_query_is_known_and_asks_nothing() {
        let mut state = banded();
        assert!(state.request_portraits(None).is_some());
        state.apply(&portrait("spotify:artist:r0", Some("0")));
        state.apply(&portrait("spotify:artist:r1", Some("1")));
        type_text(&mut state, "abcd");
        state.apply(&rich_reply("abcd", 8));
        assert_eq!(
            state.portrait("spotify:artist:r0"),
            crate::portraits::Portrait::Known(
                &takofy_model::CoverUrl::from_wire("https://i.scdn.co/image/0")
                    .expect("a cover url")
            )
        );
        assert_eq!(
            state.request_portraits(None),
            None,
            "both rows the new answer shows are already held"
        );
    }

    #[test]
    fn a_portrait_failure_settles_the_rows_on_the_stand_in() {
        let mut state = banded();
        assert!(state.request_portraits(None).is_some());
        state.apply(&Update::PortraitsFailed {
            query: "abc".to_owned(),
        });
        assert_eq!(state.request_portraits(None), None, "a failed key is kept");
        for artist in ["spotify:artist:r0", "spotify:artist:r1"] {
            assert_eq!(state.portrait(artist), Portrait::Absent);
        }
    }

    #[test]
    fn a_page_ask_is_not_current_after_the_page_is_left() {
        let mut state = paged(25);
        assert!(state.request_portraits(Some("abc")).is_some());
        // The reader navigates to a playlist: no page is open this frame, and
        // the reply lands after it is gone.
        assert_eq!(state.request_portraits(None), None);
        state.apply(&Update::PortraitsFailed {
            query: "abc".to_owned(),
        });
        assert_eq!(
            state.portrait("spotify:artist:r0"),
            Portrait::Unread,
            "a failure that lands after the page is left must not mark its artists absent"
        );
    }

    #[test]
    fn a_menu_reply_that_lands_on_its_own_page_is_current() {
        let mut state = banded();
        assert_eq!(
            state.request_portraits(None),
            Some(portrait_command(
                "abc",
                &["spotify:artist:r0", "spotify:artist:r1"]
            ))
        );
        // Enter opens the page under the same key before the menu's reply
        // lands; the page's own ask waits for the read already on the wire, but
        // the frame that opens it is what makes the page the surface on screen.
        state.open_results("abc");
        state.close();
        assert_eq!(state.request_portraits(Some("abc")), None);
        state.apply(&Update::Portraits {
            query: "abc".to_owned(),
            portraits: vec![
                takofy_model::ArtistPortrait {
                    artist: "spotify:artist:r0".to_owned(),
                    portrait: Some(
                        takofy_model::CoverUrl::from_wire("https://i.scdn.co/image/0")
                            .expect("a cover url"),
                    ),
                },
                takofy_model::ArtistPortrait {
                    artist: "spotify:artist:r1".to_owned(),
                    portrait: Some(
                        takofy_model::CoverUrl::from_wire("https://i.scdn.co/image/1")
                            .expect("a cover url"),
                    ),
                },
            ],
        });
        assert_eq!(
            state.portrait("spotify:artist:r0"),
            Portrait::Known(
                &takofy_model::CoverUrl::from_wire("https://i.scdn.co/image/0")
                    .expect("a cover url")
            ),
            "the menu's faces are the faces the page draws"
        );
        let expected: Vec<String> = uris(2..8);
        let expected: Vec<&str> = expected.iter().map(String::as_str).collect();
        assert_eq!(
            state.request_portraits(Some("abc")),
            Some(portrait_command("abc", &expected)),
            "the page asks only for the artists the menu did not answer"
        );
    }

    #[test]
    fn the_menu_closing_hands_the_page_a_distinct_slot_in_the_next_frame() {
        let mut state = banded();
        state.open_results("abc");
        assert_eq!(
            state.request_portraits(Some("abc")),
            Some(portrait_command(
                "abc",
                &["spotify:artist:r0", "spotify:artist:r1"]
            )),
            "the menu wins the slot while it is open"
        );
        state.apply(&portrait("spotify:artist:r0", Some("0")));
        state.apply(&portrait("spotify:artist:r1", Some("1")));
        state.close();
        let expected: Vec<String> = uris(2..8);
        let expected: Vec<&str> = expected.iter().map(String::as_str).collect();
        assert_eq!(
            state.request_portraits(Some("abc")),
            Some(portrait_command("abc", &expected)),
            "the page asks under the same query for the rest"
        );
    }

    #[test]
    fn taking_the_artists_reveal_widens_the_page_window() {
        let mut state = paged(25);
        assert!(state.request_portraits(Some("abc")).is_some());
        state.apply(&Update::Portraits {
            query: "abc".to_owned(),
            portraits: uris(0..10)
                .into_iter()
                .map(|artist| takofy_model::ArtistPortrait {
                    artist,
                    portrait: None,
                })
                .collect(),
        });
        state.take_more("abc", Kind::Artists);
        let expected: Vec<String> = uris(10..20);
        let expected: Vec<&str> = expected.iter().map(String::as_str).collect();
        assert_eq!(
            state.request_portraits(Some("abc")),
            Some(portrait_command("abc", &expected)),
            "the next ten unknowns, in window order"
        );
    }

    // The page's own view.

    fn shares(state: &Search, query: &str) -> Vec<String> {
        match state.page_view(query) {
            Some(PageView::Ready(listing)) => listing
                .shape
                .bands
                .iter()
                .map(crate::search_index::Band::share)
                .collect(),
            other => panic!("expected a ready page, got {other:?}"),
        }
    }

    #[test]
    fn the_page_view_is_none_for_another_query() {
        let state = typed("abc");
        assert_eq!(state.page_view("xyz"), None);
    }

    #[test]
    fn the_page_view_follows_the_load() {
        let mut state = typed("abc");
        state.open_results("abc");
        assert_eq!(state.page_view("abc"), Some(PageView::Waiting));
        state.request_page(Some("abc"));
        assert_eq!(state.page_view("abc"), Some(PageView::Waiting));
        state.apply(&failed("abc"));
        assert_eq!(
            state.page_view("abc"),
            Some(PageView::Failed(Failure::Search))
        );
        state.retry_page("abc");
        state.apply(&reply("abc", &[]));
        assert_eq!(state.page_view("abc"), Some(PageView::Empty));

        let ready = paged(2);
        let Some(PageView::Ready(listing)) = ready.page_view("abc") else {
            panic!("a ready page");
        };
        assert_eq!(listing.answer.len(), 2);
        assert_eq!(listing.index.tracks, 2);
    }

    #[test]
    fn a_ready_page_is_capped_at_ten_of_each_kind_until_a_reveal_is_taken() {
        let mut state = paged(25);
        assert_eq!(shares(&state, "abc"), ["10 of 25", "10 of 25", "10 of 25"]);

        state.take_more("abc", Kind::Tracks);
        assert_eq!(shares(&state, "abc"), ["25", "10 of 25", "10 of 25"]);

        state.take_more("abc", Kind::Artists);
        assert_eq!(shares(&state, "abc"), ["25", "10 of 25", "25"]);
    }

    #[test]
    fn a_reveal_changes_one_block_and_leaves_the_others_whole() {
        let before = paged(25);
        let Some(PageView::Ready(before)) = before.page_view("abc") else {
            panic!("a ready page");
        };
        let albums = before.index.albums.clone();
        let artists = before.index.artists.clone();

        let mut after = paged(25);
        after.take_more("abc", Kind::Tracks);
        let Some(PageView::Ready(after)) = after.page_view("abc") else {
            panic!("a ready page");
        };
        assert_eq!(after.index.albums, albums);
        assert_eq!(after.index.artists, artists);
        assert_eq!(after.shape.bands[0].shown, 25);
        assert_eq!(
            (after.shape.bands[1].shown, after.shape.bands[2].shown),
            (10, 10)
        );
    }

    #[test]
    fn a_take_for_another_query_or_a_read_that_is_not_ready_does_nothing() {
        let mut state = paged(25);
        state.take_more("xy", Kind::Tracks);
        assert_eq!(shares(&state, "abc"), ["10 of 25", "10 of 25", "10 of 25"]);

        let mut waiting = typed("abc");
        waiting.open_results("abc");
        assert_eq!(waiting.page_view("abc"), Some(PageView::Waiting));
        waiting.take_more("abc", Kind::Tracks);
        waiting.apply(&rich_reply("abc", 25));
        assert_eq!(
            shares(&waiting, "abc"),
            ["10 of 25", "10 of 25", "10 of 25"]
        );
    }

    #[test]
    fn a_new_query_resets_the_reveal_and_the_same_one_keeps_it() {
        let mut state = paged(25);
        state.take_more("abc", Kind::Tracks);
        state.open_results("abc");
        assert_eq!(shares(&state, "abc")[0], "25");
        state.retry_page("abc");
        state.apply(&rich_reply("abc", 25));
        assert_eq!(shares(&state, "abc")[0], "25", "a retry keeps it");

        state.open_results("abcd");
        state.apply(&rich_reply("abcd", 25));
        assert_eq!(
            shares(&state, "abcd")[0],
            "10 of 25",
            "a new query starts over"
        );
        state.open_results("abc");
        state.apply(&rich_reply("abc", 25));
        assert_eq!(
            shares(&state, "abc")[0],
            "10 of 25",
            "and coming back through another query starts over too"
        );
    }

    #[test]
    fn a_page_visit_moves_for_a_new_query_and_keeps_a_reentry() {
        let mut state = paged(25);
        let first = state.page_visit();
        state.open_results("abc");
        assert_eq!(
            state.page_visit(),
            first,
            "the same query is the same visit, so the page keeps its offset"
        );
        state.retry_page("abc");
        assert_eq!(state.page_visit(), first, "a retry keeps its offset too");

        state.open_results("abcd");
        let second = state.page_visit();
        assert_ne!(second, first, "another query is another visit");
        state.open_results("abc");
        assert_ne!(
            state.page_visit(),
            first,
            "A→B→A is a fresh page, so it starts at the top"
        );
        assert_ne!(state.page_visit(), second);
    }

    #[test]
    fn clearing_the_text_closes_nothing_by_itself() {
        // The clear control edits the text programmatically: only the key moves
        // here, and the shell's `settle` is what closes the menu.
        let mut state = typed("abc");
        assert_eq!(state.request(None), Some(search("abc")));
        state.apply(&reply("abc", &["spotify:track:a"]));
        state.text.clear();
        state.text_changed();
        assert_eq!(state.menu_view(), MenuView::Nothing);
        assert_eq!(state.request(None), None, "a blank key asks for nothing");
        assert!(state.is_open(), "only `settle` or Escape closes the menu");
    }
}
