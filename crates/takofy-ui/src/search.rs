//! The search field's state: the query, the menu's mark, and the two reads.
//!
//! This is a reducer, like [`crate::drawer::Drawer`]: the views draw what it
//! decides and never decide anything themselves. The rules it encodes are the
//! field's own: the mark starts on the field and lives on the rows, typing returns
//! it there, Escape is two steps, results start at the first character, the answer
//! ends in a floor row when a band hides something, a miss is one line, and no
//! read in flight draws a skeleton. Each one has a test beside this file.
//!
//! Two reads share one key space. The menu holds the answer it derives its
//! bands from; the page holds the whole answer. They are keyed by the same
//! collapsed query, so one reply can fill both, and a reply for a query the
//! user has typed past fills neither.

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
/// It is the single key for both reads, the menu's heading and the floor row,
/// so "a  b" and "a b" are the same search everywhere, and a blank key asks for
/// nothing: the first character is what starts a read.
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
    /// Play the result at this index of the whole answer, so the queue becomes
    /// the results and next walks them.
    Play(usize),
    /// Open the album at this index of the derived index.
    OpenAlbum(usize),
    /// Open the artist at this index of the derived index.
    OpenArtist(usize),
    /// Open the results page for the field's query: what Enter on the field asks
    /// for, and what the floor row asks for.
    OpenResults,
    /// Escape cleared the query and closed the menu, its second step.
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
    /// The answer was empty: one muted line, nothing to press.
    Empty,
    /// The read failed: the same line with its own words.
    Failed(Failure),
    /// Nothing true to say yet: idle, in flight, another query's answer, or
    /// blank text. There is no skeleton.
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
    /// The marked row's ordinal in the walk, or `None` for the field, which is
    /// where the menu opens.
    mark: Option<usize>,
    /// The input that chose the mark, so the two highlight tiers never mix: the
    /// last device wins.
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
    /// the whole answer the wire returned, so the shell asks for it here rather
    /// than taking the menu's capped prefix.
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
    /// can never be drawn. A programmatic edit — the clear control — calls
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
            // Typing, and a click on the field, put the rule back on the field
            // and re-open the menu on the query it now stands for.
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
                        // Up from the first row is the field: the mark lives on
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
                // Escape's first step: back to the field, query kept, menu open.
                self.mark = None;
            } else if self.open || !self.text.is_empty() {
                // Escape's second step: clear the query, which closes the menu.
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

    /// The pointer moved over the menu, so the pointer takes the highlight from
    /// the keyboard: the last device wins.
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
mod tests;
