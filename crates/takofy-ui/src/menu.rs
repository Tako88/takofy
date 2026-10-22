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
use takofy_model::TrackEntry;

use crate::route::Page;

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
    Remove,
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
}

impl Listing {
    /// Whether this list offers a removal.
    pub fn removable(&self) -> bool {
        matches!(self, Listing::Playlist { .. } | Listing::Queue)
    }
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
/// `key` and `submenu` are the rest of the row grammar. No verb bound today
/// carries either, so both are `None`/`false` until a slice binds one.
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

/// What a door asks the menu to show, taken as a snapshot of the rows it is about.
#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    pub door: Door,
    pub items: Vec<Item>,
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

/// Why a menu closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    Escape,
    Tab,
    Outside,
    Page,
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

/// An open menu, or a closed one that still remembers nothing worth keeping.
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
        self.first_drawn = None;
        true
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
    pub fn choose(&mut self, row: usize) -> Option<Closed> {
        let verb = self.items.get(row)?.verb;
        self.close_with(Some(verb))
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

    pub fn lit(&self) -> Option<usize> {
        self.lit
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
        Some(Closed {
            opener: self.opener,
            chose,
        })
    }
}

#[cfg(test)]
mod tests {
    use egui::{Id, Pos2, Rect, vec2};
    use takofy_model::TrackEntry;

    use super::*;
    use crate::route::Page;

    fn opener() -> Id {
        Id::unique("row-more")
    }

    fn anchor() -> Anchor {
        Anchor::Button(Rect::from_min_size(Pos2::ZERO, vec2(28.0, 28.0)))
    }

    fn door(device: Device) -> Door {
        Door {
            anchor: anchor(),
            opener: opener(),
            device,
        }
    }

    fn item(index: usize) -> Item {
        let verbs = [
            Verb::AddToQueue,
            Verb::GoToAlbum,
            Verb::GoToArtist,
            Verb::CopyLink,
        ];
        Item {
            label: format!("Verb {index}"),
            kind: Kind::Plain,
            key: None,
            submenu: false,
            verb: verbs[index % verbs.len()],
        }
    }

    fn request(device: Device, rows: usize) -> Request {
        Request {
            door: door(device),
            items: (0..rows).map(item).collect(),
            targets: Vec::new(),
            rows: Vec::new(),
            listing: Listing::default(),
            page: None,
        }
    }

    fn playlist_listing() -> Listing {
        Listing::Playlist {
            uri: "spotify:playlist:a".to_owned(),
            list: Id::unique("playlist-tracks"),
        }
    }

    fn opened_on(request: Request) -> Menu {
        let mut menu = Menu::default();
        assert!(menu.open(request, Gate::default()), "the menu opened");
        menu
    }

    fn destructive_item(label: &str) -> Item {
        Item {
            label: label.to_owned(),
            kind: Kind::Destructive,
            key: Some("Del"),
            submenu: false,
            verb: Verb::Remove,
        }
    }

    fn opened(device: Device, rows: usize) -> Menu {
        let mut menu = Menu::default();
        assert!(
            menu.open(request(device, rows), Gate::default()),
            "the menu opened"
        );
        menu
    }

    fn entry(uri: &str) -> TrackEntry {
        TrackEntry {
            uri: uri.to_owned(),
            title: "A Title".to_owned(),
            artists: "A Band".to_owned(),
            album: Some("A Record".to_owned()),
            album_uri: Some("spotify:album:a".to_owned()),
            artist_uris: vec!["spotify:artist:b".to_owned()],
            added_at: None,
            duration_ms: None,
            named: true,
            cover: None,
        }
    }

    #[test]
    fn a_pointer_open_lights_nothing() {
        let menu = opened(Device::Pointer, 3);
        assert_eq!(menu.lit(), None, "the pointer lit a row it never reached");
        assert_eq!(menu.device(), Device::Pointer);
        for row in 0..3 {
            assert_eq!(menu.tier(row, None), Tier::None, "row {row}");
        }
    }

    #[test]
    fn a_keyboard_open_lights_the_first_row() {
        let menu = opened(Device::Keyboard, 3);
        assert_eq!(menu.lit(), Some(0));
        assert_eq!(menu.tier(0, None), Tier::Keyboard);
        assert_eq!(menu.tier(1, None), Tier::None);
    }

    #[test]
    fn opening_is_refused_while_carrying_or_writing() {
        for gate in [
            Gate {
                carrying: true,
                writing: false,
            },
            Gate {
                carrying: false,
                writing: true,
            },
        ] {
            let mut menu = Menu::default();
            assert!(!menu.open(request(Device::Pointer, 2), gate), "{gate:?}");
            assert!(!menu.is_open(), "{gate:?} opened the menu");
        }
        let mut menu = Menu::default();
        assert!(menu.open(request(Device::Pointer, 2), Gate::default()));
        assert!(menu.is_open());
    }

    #[test]
    fn an_empty_menu_does_not_open() {
        let mut menu = Menu::default();
        assert!(!menu.open(request(Device::Pointer, 0), Gate::default()));
        assert!(!menu.is_open());
    }

    #[test]
    fn down_from_nothing_lights_the_first_row_and_up_the_last() {
        let mut down = opened(Device::Pointer, 4);
        down.step(Step::Down);
        assert_eq!(down.lit(), Some(0));
        assert_eq!(down.device(), Device::Keyboard, "a step is the keyboard's");

        let mut up = opened(Device::Pointer, 4);
        up.step(Step::Up);
        assert_eq!(up.lit(), Some(3));
    }

    #[test]
    fn the_arrows_clamp_at_the_ends() {
        let mut menu = opened(Device::Keyboard, 3);
        for _ in 0..8 {
            menu.step(Step::Down);
        }
        assert_eq!(menu.lit(), Some(2), "down wrapped past the last row");
        for _ in 0..8 {
            menu.step(Step::Up);
        }
        assert_eq!(menu.lit(), Some(0), "up wrapped past the first row");
    }

    #[test]
    fn home_and_end_jump() {
        let mut menu = opened(Device::Pointer, 5);
        menu.step(Step::Last);
        assert_eq!(menu.lit(), Some(4));
        menu.step(Step::First);
        assert_eq!(menu.lit(), Some(0));
    }

    #[test]
    fn a_pointer_over_a_row_swaps_the_tier() {
        let mut menu = opened(Device::Keyboard, 4);
        assert_eq!(menu.lit(), Some(0));

        menu.pointer_over(Some(2));
        assert_eq!(
            menu.lit(),
            None,
            "the keyboard kept its row under the pointer"
        );
        assert_eq!(menu.device(), Device::Pointer);
        assert_eq!(menu.tier(2, Some(2)), Tier::Pointer);
        assert_eq!(menu.tier(0, Some(2)), Tier::None);
        assert_eq!(menu.tier(2, None), Tier::None);
    }

    #[test]
    fn only_one_tier_is_ever_lit() {
        for device in [Device::Keyboard, Device::Pointer] {
            for rows in 1..=4 {
                for pointer in [None, Some(0), Some(2)] {
                    let mut menu = opened(device, rows);
                    if pointer.is_some() {
                        menu.pointer_over(pointer);
                    }
                    for step in [None, Some(Step::Down), Some(Step::Up), Some(Step::Last)] {
                        let mut menu = menu.clone();
                        if let Some(step) = step {
                            menu.step(step);
                        }
                        let lit = (0..rows)
                            .filter(|row| menu.tier(*row, pointer) != Tier::None)
                            .count();
                        assert!(lit <= 1, "{lit} rows lit for {device:?} at {pointer:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn enter_chooses_only_a_row_the_keyboard_lit() {
        let mut pointer = opened(Device::Pointer, 3);
        assert_eq!(pointer.choose_lit(), None, "Enter fired a row nobody lit");
        assert!(
            pointer.is_open(),
            "the menu closed on a choice it never made"
        );

        let mut keyboard = opened(Device::Keyboard, 3);
        keyboard.step(Step::Down);
        assert_eq!(
            keyboard.choose_lit(),
            Some(Closed {
                opener: Some(opener()),
                chose: Some(Verb::GoToAlbum),
            })
        );
        assert!(!keyboard.is_open());
    }

    #[test]
    fn a_click_chooses_the_row_and_closes() {
        let mut menu = opened(Device::Pointer, 3);
        assert_eq!(
            menu.choose(1),
            Some(Closed {
                opener: Some(opener()),
                chose: Some(Verb::GoToAlbum),
            })
        );
        assert!(!menu.is_open());
    }

    #[test]
    fn choosing_a_row_that_is_not_there_does_nothing() {
        let mut menu = opened(Device::Pointer, 2);
        assert_eq!(menu.choose(2), None);
        assert!(menu.is_open(), "a miss closed the menu");
        assert_eq!(menu.choose(99), None);
        assert!(menu.is_open());
    }

    #[test]
    fn the_first_drawn_frame_is_the_opening_and_the_pass_after_it_is_not() {
        let mut menu = opened(Device::Pointer, 3);
        assert!(menu.just_opened(1.0), "nothing has drawn it yet");
        menu.mark_drawn(1.0);
        assert!(menu.just_opened(1.0), "a second pass of the same frame");
        assert!(
            !menu.just_opened(1.016),
            "a later frame is not the opening, or the door's click would dismiss it"
        );
    }

    #[test]
    fn escape_tab_outside_and_focus_all_return_the_opener() {
        for reason in [Reason::Escape, Reason::Tab, Reason::Outside] {
            let mut menu = opened(Device::Keyboard, 3);
            assert_eq!(
                menu.close(reason),
                Some(Closed {
                    opener: Some(opener()),
                    chose: None,
                }),
                "{reason:?}"
            );
            assert!(!menu.is_open(), "{reason:?}");
        }
    }

    #[test]
    fn a_page_change_closes_without_an_opener() {
        let mut menu = Menu::default();
        let mut request = request(Device::Pointer, 2);
        request.page = Some(Page::Playlist("a".to_owned()));
        assert!(menu.open(request, Gate::default()));

        assert_eq!(
            menu.follow_page(Some(&Page::Playlist("a".to_owned()))),
            None
        );
        assert!(menu.is_open(), "the page it opened on closed it");

        assert_eq!(
            menu.follow_page(Some(&Page::Playlist("b".to_owned()))),
            Some(Closed {
                opener: None,
                chose: None,
            }),
            "the row the menu was about is gone, so there is nothing to focus"
        );
        assert!(!menu.is_open());
    }

    #[test]
    fn reopening_replaces_the_menu() {
        let mut menu = opened(Device::Pointer, 2);
        let mut request = request(Device::Keyboard, 3);
        request.door.opener = Id::unique("another-more");
        assert!(menu.open(request, Gate::default()));

        assert_eq!(menu.items().len(), 3);
        assert_eq!(menu.opener(), Some(Id::unique("another-more")));
        assert_eq!(menu.lit(), Some(0));
    }

    #[test]
    fn the_destructive_row_is_always_last() {
        let mut menu = Menu::default();
        let mut request = request(Device::Pointer, 3);
        request.items[0].kind = Kind::Destructive;
        request.items[0].label = "Remove".to_owned();
        assert!(menu.open(request, Gate::default()));

        let kinds: Vec<Kind> = menu.items().iter().map(|item| item.kind).collect();
        assert_eq!(kinds, vec![Kind::Plain, Kind::Plain, Kind::Destructive]);
        assert_eq!(
            menu.items().last().map(|item| item.label.as_str()),
            Some("Remove")
        );
    }

    #[test]
    fn the_remove_row_is_drawn_last_and_lit_first_by_the_keyboard() {
        let mut request = request(Device::Keyboard, 2);
        request.items = vec![item(0), destructive_item("Remove from this playlist")];
        let menu = opened_on(request);

        assert_eq!(
            menu.items().last().map(|item| item.verb),
            Some(Verb::Remove),
            "the destructive row is not last after the sort"
        );
        assert_eq!(menu.lit(), Some(0), "the keyboard opens on the first row");
    }

    #[test]
    fn cut_is_built_from_the_snapshot_and_only_for_a_removing_listing() {
        let entries = vec![entry("spotify:track:a"), entry("spotify:track:b")];
        for listing in [
            Listing::Page,
            Listing::Current,
            Listing::Queue,
            playlist_listing(),
        ] {
            let mut request = request(Device::Pointer, 2);
            request.listing = listing.clone();
            request.rows = vec![1, 3];
            request.targets = entries.clone();
            let menu = opened_on(request);

            let removing = matches!(listing, Listing::Queue | Listing::Playlist { .. });
            match menu.cut() {
                Some(cut) => {
                    assert!(removing, "{listing:?} offered a removal");
                    assert_eq!(cut.listing, listing, "{listing:?}");
                    assert_eq!(cut.rows, vec![1, 3], "{listing:?}");
                    assert_eq!(cut.entries, entries, "{listing:?}");
                }
                None => assert!(!removing, "{listing:?} has no cut"),
            }
        }
    }

    #[test]
    fn cut_survives_the_close() {
        let mut request = request(Device::Keyboard, 2);
        request.listing = Listing::Queue;
        request.rows = vec![2];
        request.targets = vec![entry("spotify:track:a")];
        let mut menu = opened_on(request);

        let cut = menu.cut().expect("a removal");
        assert!(menu.close(Reason::Escape).is_some());
        assert_eq!(
            menu.cut(),
            Some(cut),
            "the shell reads the cut after the menu closed on it"
        );
    }

    #[test]
    fn choose_destructive_picks_the_last_row_only_if_destructive() {
        let mut request = request(Device::Keyboard, 2);
        request.items = vec![item(0), destructive_item("Remove from this playlist")];
        let mut menu = opened_on(request);
        assert_eq!(
            menu.choose_destructive(),
            Some(Closed {
                opener: Some(opener()),
                chose: Some(Verb::Remove),
            })
        );
        assert!(!menu.is_open());

        let mut plain = opened(Device::Keyboard, 2);
        assert_eq!(
            plain.choose_destructive(),
            None,
            "a menu with no removal chose one"
        );
        assert!(plain.is_open(), "a menu with no removal closed");
        assert_eq!(plain.lit(), Some(0), "the lit row moved");
    }
}
