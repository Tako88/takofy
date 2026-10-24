//! The search field's state: the query, the menu's mark, and the two reads.
//!
//! This is a reducer, like [`crate::drawer::Drawer`]: the views draw what it
//! decides and never decide anything themselves. The rules it encodes are the
//! brief's R1–R3 and R12–R14, and each one has a test below.
//!
//! Two reads share one key space. The menu holds the first five tracks the
//! wire answered; the page holds the whole answer. They are keyed by the same
//! collapsed query, so one reply can fill both, and a reply for a query the
//! user has typed past fills neither (R14).

use takofy_model::TrackEntry;

use crate::library::Load;
use crate::message::{Command, Failure, Update};

/// How many tracks the drop-down shows; the wire answers twenty at most (R12).
const SHOWN: usize = 5;

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
    /// Open the results page for the field's query (R1, R12).
    OpenResults,
    /// Escape cleared the query and closed the menu (R3 step two).
    Close,
}

/// What the drop-down has to say, from the menu read alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuView<'a> {
    /// The first [`SHOWN`] tracks, the total the wire answered, and whether a
    /// floor row is drawn (R12).
    Rows {
        tracks: &'a [TrackEntry],
        total: usize,
        floor: bool,
    },
    /// The answer was empty: one muted line, nothing to press (R13).
    Empty,
    /// The read failed: the same line with its own words (R13).
    Failed(Failure),
    /// Nothing true to say yet: idle, in flight, another query's answer, or
    /// blank text. There is no skeleton (R14).
    Nothing,
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
    /// The marked row, `Some(shown)` for the floor row, `None` for the field
    /// (R1).
    mark: Option<usize>,
    /// The input that chose the mark, so the two highlight tiers never mix
    /// (R9).
    device: Device,
    /// The first five results the menu draws.
    menu: Read,
    /// The whole answer the results page draws.
    page: Read,
    /// The key on the wire. One read at a time, so a burst of keystrokes cannot
    /// queue twenty requests.
    inflight: Option<String>,
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

    /// The marked row: `None` for the field, `Some(5)` for the floor row.
    pub fn mark(&self) -> Option<usize> {
        self.mark
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
            Load::Ready(tracks) => MenuView::Rows {
                tracks: &tracks[..tracks.len().min(SHOWN)],
                total: tracks.len(),
                floor: tracks.len() > SHOWN,
            },
        }
    }

    /// The whole answer the menu's query holds, or `None` when the menu has no
    /// answer for the text as it stands.
    ///
    /// [`Self::menu_view`] carries only the five rows the menu draws; the queue
    /// is the whole answer the wire returned (R16), so the shell asks for it
    /// here rather than taking the menu's first five.
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

    /// Points the menu at the text as it now stands.
    ///
    /// An edit that moves the key drops the old answer, so a superseded reply
    /// can never be drawn (R14). A programmatic edit — the clear control — calls
    /// this too, because only the text edit itself reports `typed`.
    pub fn text_changed(&mut self) {
        Self::want(&mut self.menu, collapse(&self.text));
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
    /// `shown` and `floor` come from [`Self::menu_view`], so the router works on
    /// what is drawn. A frame that starts unfocused changes nothing (the field
    /// is not the one the user is typing into).
    ///
    /// The arrows only ever land on a row that exists:
    /// - With the menu closed and a query to re-open it on, Down and Up re-open
    ///   it and mark the first row (Down) or the last row (Up) in the same
    ///   keystroke. With the menu closed and a blank key they do nothing.
    /// - With the menu open and no rows to mark — idle, empty, failed — they do
    ///   nothing, so a mark is never a row that is not drawn.
    /// - Enter only plays a row that is drawn: the menu must be open, the mark
    ///   must name a row, and that row must be inside `shown`.
    pub fn step(&mut self, input: KeyInput, shown: usize, floor: bool) -> Act {
        if !input.focused {
            return Act::None;
        }
        let rows = shown + usize::from(floor);
        if input.typed || input.clicked_field {
            // R2: typing, and a click on the field, put the rule back on the
            // field and re-open the menu on the query it now stands for.
            self.mark = None;
            self.device = Device::Keyboard;
            self.text_changed();
            self.open = self.key().is_some();
        }
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
            // Only a drawn row can be played: a closed menu draws nothing, and
            // the floor row opens the page instead.
            if self.open
                && let Some(index) = self.mark
                && index < shown
            {
                return Act::Play(index);
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

    /// Whether [`Self::request`] would send something, without changing
    /// anything.
    ///
    /// The window asks after a frame's edits and requests a repaint on true:
    /// `logic` runs before `ui`, so a read armed by that frame would otherwise
    /// wait for the next input event.
    pub fn wants(&self, page: Option<&str>) -> bool {
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
            _ => {}
        }
    }

    /// Points the results page at `query`, seeding it from the menu when the
    /// menu already holds that answer.
    ///
    /// A failed menu is not retried here: Enter on a failed key is a deliberate
    /// action, so it re-asks exactly once. `Library::settle` is what keeps an
    /// answer that is already held and in flight.
    pub fn open_results(&mut self, query: &str) {
        if self.menu.key.as_deref() == Some(query)
            && let Load::Ready(tracks) = &self.menu.load
        {
            self.page = Read {
                key: Some(query.to_owned()),
                load: Load::Ready(tracks.clone()),
            };
            return;
        }
        self.page = Read {
            key: Some(query.to_owned()),
            load: Load::Idle,
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
        self.page = Read {
            key: Some(query.to_owned()),
            load: Load::Idle,
        };
    }

    /// Points `read` at `key`, dropping its answer when the key moves.
    fn want(read: &mut Read, key: Option<String>) {
        if read.key != key {
            *read = Read {
                key,
                load: Load::Idle,
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
                read.load = load.clone();
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

    /// Types `text` into the field, as one frame of the real path would.
    fn type_text(state: &mut Search, text: &str) {
        state.text = text.to_owned();
        let input = KeyInput {
            typed: true,
            focused: true,
            ..KeyInput::default()
        };
        assert_eq!(
            state.step(input, 0, false),
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
        assert_eq!(
            state.menu_view(),
            MenuView::Rows {
                tracks: &answer,
                total: 1,
                floor: false,
            }
        );
    }

    #[test]
    fn a_reply_fills_both_reads_when_the_keys_match() {
        let mut state = typed("abc");
        state.open_results("abc");
        // The coalescing path: both reads are waiting on the same key.
        state.menu = Read {
            key: Some("abc".to_owned()),
            load: Load::Loading,
        };
        state.page = Read {
            key: Some("abc".to_owned()),
            load: Load::Loading,
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
        assert_eq!(
            state.menu_view(),
            MenuView::Rows {
                tracks: &answer,
                total: 1,
                floor: false,
            },
            "the reply fills the idle read on its own key"
        );
        assert_eq!(state.inflight, None, "the reply was used, not orphaned");
        assert_eq!(state.request(None), None, "and is not asked for again");
    }

    #[test]
    fn enter_while_the_menu_is_loading_still_fills_the_page() {
        let mut state = typed("abc");
        assert_eq!(state.request(None), Some(search("abc")));
        assert_eq!(state.step(key("enter"), 0, false), Act::OpenResults);
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
        };
        state.inflight = Some("abcd".to_owned());

        state.apply(&failed("abc"));

        assert_eq!(
            state.menu,
            Read {
                key: Some("abcd".to_owned()),
                load: Load::Loading,
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
            };
            state.page = Read {
                key: Some(if same_key { "abc" } else { "xyz" }.to_owned()),
                load: page.clone(),
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
        assert_eq!(state.step(focused, 0, false), Act::None);
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
        let mut state = typed("abc");
        assert_eq!(state.mark(), None, "the menu opens unmarked (R1)");
        assert_eq!(state.step(key("down"), 5, false), Act::None);
        assert_eq!(state.mark(), Some(0));
        assert_eq!(state.device(), Device::Keyboard);
    }

    #[test]
    fn down_clamps_at_the_last_row() {
        let mut state = typed("abc");
        for expected in [0, 1, 2, 3, 4, 4, 4] {
            state.step(key("down"), 5, false);
            assert_eq!(state.mark(), Some(expected));
        }
        let mut with_floor = typed("abc");
        for _ in 0..9 {
            with_floor.step(key("down"), 5, true);
        }
        assert_eq!(with_floor.mark(), Some(5), "the floor is the last row");
    }

    #[test]
    fn up_from_the_first_row_returns_to_the_field() {
        let mut state = typed("abc");
        state.step(key("down"), 5, false);
        assert_eq!(state.mark(), Some(0));
        assert_eq!(state.step(key("up"), 5, false), Act::None);
        assert_eq!(state.mark(), None, "row 0 is one press from the field");
    }

    #[test]
    fn up_from_the_field_marks_the_last_row() {
        let mut state = typed("abc");
        state.step(key("up"), 5, false);
        assert_eq!(state.mark(), Some(4));

        let mut with_floor = typed("abc");
        with_floor.step(key("up"), 5, true);
        assert_eq!(
            with_floor.mark(),
            Some(5),
            "the floor is drawn, so it is last"
        );
    }

    /// The field with an answered menu, closed again as an applied action
    /// leaves it.
    fn answered_closed(text: &str, uris: &[&str]) -> Search {
        let mut state = typed(text);
        assert_eq!(state.request(None), Some(search(text)));
        state.apply(&reply(text, uris));
        state.close();
        assert!(!state.is_open());
        state
    }

    #[test]
    fn down_on_a_closed_menu_reopens_it_and_marks_the_first_row() {
        let mut state = answered_closed("abc", &["a", "b", "c"]);
        assert_eq!(state.step(key("down"), 3, false), Act::None);
        assert!(state.is_open(), "the arrow re-opens the menu it acts on");
        assert_eq!(state.mark(), Some(0));
        assert_eq!(state.device(), Device::Keyboard);
    }

    #[test]
    fn up_on_a_closed_menu_reopens_it_and_marks_the_last_row() {
        let mut state = answered_closed("abc", &["a", "b", "c"]);
        assert_eq!(state.step(key("up"), 3, false), Act::None);
        assert!(state.is_open());
        assert_eq!(state.mark(), Some(2));

        let mut with_floor = answered_closed("abc", &["a", "b", "c"]);
        with_floor.step(key("up"), 3, true);
        assert_eq!(with_floor.mark(), Some(3), "the floor is the last row");
    }

    #[test]
    fn an_arrow_on_a_closed_blank_field_does_nothing() {
        let mut blank = Search::default();
        assert_eq!(blank.step(key("down"), 0, false), Act::None);
        assert_eq!(blank.step(key("up"), 0, false), Act::None);
        assert!(!blank.is_open(), "a blank key re-opens nothing");
        assert_eq!(blank.mark(), None);

        // Whitespace is not a key either.
        let mut spaces = typed("   ");
        assert_eq!(spaces.step(key("down"), 0, false), Act::None);
        assert_eq!(spaces.step(key("up"), 0, false), Act::None);
        assert!(!spaces.is_open());
        assert_eq!(spaces.mark(), None);
    }

    #[test]
    fn an_arrow_marks_nothing_when_no_row_is_drawn() {
        // Open, and the answer is still on the wire: nothing to mark.
        let mut loading = typed("abc");
        assert_eq!(loading.request(None), Some(search("abc")));
        assert_eq!(loading.step(key("down"), 0, false), Act::None);
        assert_eq!(loading.mark(), None);
        assert_eq!(loading.step(key("up"), 0, false), Act::None);
        assert_eq!(loading.mark(), None);

        // Open and failed: nothing to mark.
        loading.apply(&failed("abc"));
        assert_eq!(loading.step(key("down"), 0, false), Act::None);
        assert_eq!(loading.mark(), None);

        // Open and empty: nothing to mark.
        let mut empty = typed("abc");
        empty.apply(&reply("abc", &[]));
        assert_eq!(empty.step(key("down"), 0, false), Act::None);
        assert_eq!(empty.mark(), None);

        // Closed with a query but no answer to draw: it re-opens, marks
        // nothing, because a mark must be a row that exists.
        let mut closed = typed("abc");
        closed.close();
        closed.step(key("down"), 0, false);
        assert!(closed.is_open());
        assert_eq!(closed.mark(), None);
    }

    #[test]
    fn enter_after_close_is_the_fields_enter_not_a_play() {
        let mut state = answered_closed("abc", &["a", "b", "c"]);
        // A mark from before the close must not survive into an Enter.
        state.step(key("down"), 3, false);
        assert_eq!(state.mark(), Some(0));
        state.close();
        assert_eq!(state.mark(), None);
        assert_eq!(state.step(key("enter"), 3, false), Act::OpenResults);
    }

    #[test]
    fn the_five_versus_six_boundary_counts_the_floor() {
        let mut state = typed("abc");
        for _ in 0..8 {
            state.step(key("down"), SHOWN, true);
        }
        assert_eq!(state.mark(), Some(5), "five rows and a floor: 0..=5");

        let mut no_floor = typed("abc");
        for _ in 0..8 {
            no_floor.step(key("down"), SHOWN, false);
        }
        assert_eq!(no_floor.mark(), Some(4), "five rows: 0..=4");
    }

    #[test]
    fn enter_on_a_row_plays_it() {
        let mut state = typed("abc");
        state.step(key("down"), 5, false);
        state.step(key("down"), 5, false);
        assert_eq!(state.step(key("enter"), 5, false), Act::Play(1));
    }

    #[test]
    fn enter_on_the_floor_opens_the_results() {
        let mut state = typed("abc");
        for _ in 0..6 {
            state.step(key("down"), 5, true);
        }
        assert_eq!(state.mark(), Some(5));
        assert_eq!(state.step(key("enter"), 5, true), Act::OpenResults);
    }

    #[test]
    fn enter_on_the_field_opens_the_results() {
        let mut state = typed("abc");
        assert_eq!(state.mark(), None);
        assert_eq!(state.step(key("enter"), 5, false), Act::OpenResults);
    }

    #[test]
    fn enter_with_a_blank_key_does_nothing() {
        let mut state = typed("   ");
        assert_eq!(state.step(key("enter"), 0, false), Act::None);
        // A mark left behind a blanked field opens nothing either.
        state.step(key("down"), 0, false);
        assert_eq!(state.step(key("enter"), 0, false), Act::None);
    }

    #[test]
    fn escape_from_a_row_keeps_the_query_and_the_menu() {
        let mut state = typed("abc");
        state.step(key("down"), 5, false);
        assert_eq!(state.step(key("escape"), 5, false), Act::None);
        assert_eq!(state.mark(), None);
        assert_eq!(state.text, "abc", "the query is kept (R3)");
        assert!(state.is_open(), "and the menu stays up");
        assert!(state.escapes(), "Escape still has something to undo");
    }

    #[test]
    fn escape_from_the_field_clears_the_query_and_closes_the_menu() {
        let mut state = typed("abc");
        assert_eq!(state.step(key("escape"), 5, false), Act::Close);
        assert_eq!(state.text, "");
        assert!(!state.is_open());
        assert_eq!(state.mark(), None);
        assert!(!state.escapes(), "nothing left to undo");
    }

    #[test]
    fn escape_with_an_empty_closed_field_is_not_ours() {
        let mut state = Search::default();
        assert!(!state.escapes());
        assert_eq!(state.step(key("escape"), 0, false), Act::None);
        assert_eq!(state.text, "");
        assert!(!state.is_open());
    }

    #[test]
    fn a_frame_that_starts_unfocused_does_nothing() {
        let mut state = typed("abc");
        state.step(key("down"), 5, false);
        let blurred = KeyInput {
            focused: false,
            ..KeyInput::default()
        };
        assert_eq!(state.step(blurred, 5, false), Act::None);
        assert_eq!(state.mark(), Some(0), "the mark is the shell's to clear");

        let blurred_escape = KeyInput {
            escape: true,
            focused: false,
            ..KeyInput::default()
        };
        assert_eq!(state.step(blurred_escape, 5, false), Act::None);
        assert_eq!(state.text, "abc", "a blurred field does not eat keys");
        assert!(state.is_open());
    }

    #[test]
    fn typing_returns_the_rule_to_the_field() {
        let mut state = typed("abc");
        state.step(key("down"), 5, false);
        assert_eq!(state.mark(), Some(0));
        type_text(&mut state, "abcd");
        assert_eq!(state.mark(), None, "R2: typing puts the rule back");
        assert!(state.is_open());
    }

    #[test]
    fn a_click_on_the_field_returns_the_rule_and_keeps_the_menu() {
        let mut state = typed("abc");
        state.pointer_moved();
        state.step(key("down"), 5, false);
        assert_eq!(state.device(), Device::Keyboard);
        state.settle(false, false);
        assert!(!state.is_open(), "the menu closes when the field is left");

        let click = KeyInput {
            clicked_field: true,
            focused: true,
            ..KeyInput::default()
        };
        assert_eq!(state.step(click, 5, false), Act::None);
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
        let mut state = typed("abc");
        state.step(key("down"), 5, false);
        state.close();
        assert!(!state.is_open());
        assert_eq!(state.mark(), None);
        assert_eq!(state.text, "abc");
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
