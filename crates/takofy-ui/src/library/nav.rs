//! Where the library is: the page stack, and the moves along it.

use crate::follow::Follow;
use crate::message::Command;
use crate::naming::Surface;
use crate::route::{History, Page};

use super::{Library, Load};

impl Library {
    /// The page on screen, or `None` for the library root.
    pub fn page(&self) -> Option<&Page> {
        self.history.current()
    }

    /// The query of the results page on screen, if one is.
    ///
    /// Search is the one page whose identity is a query rather than a URI, so
    /// the shell hands it to the search state instead of a page handle. It is
    /// `None` on every other page, including one whose text happens to spell a
    /// URL: the page variant decides.
    pub fn page_search_query(&self) -> Option<&str> {
        match self.history.current()? {
            Page::Search(query) => Some(query),
            Page::Home | Page::Playlist(_) | Page::Album(_) | Page::Artist(_) | Page::Show(_) => {
                None
            }
        }
    }

    /// Which playlist's tracks are on screen, if any.
    ///
    /// This is `None` on an album, artist, show or search page: the main panel
    /// is not showing a playlist, so the sidebar highlight clears and the entry
    /// and name helpers see no selection. A search whose query spells a playlist
    /// URI is still a search.
    pub fn selected(&self) -> Option<&str> {
        match self.history.current()? {
            Page::Playlist(uri) => Some(uri),
            Page::Home | Page::Album(_) | Page::Artist(_) | Page::Show(_) | Page::Search(_) => None,
        }
    }

    /// Whether Home is the page on screen.
    pub fn on_home(&self) -> bool {
        matches!(self.page(), Some(Page::Home))
    }

    /// Moves to a playlist and returns the command that loads it.
    ///
    /// Re-selecting the page already on screen reloads it without growing the
    /// history.
    pub fn select(&mut self, playlist: &str) -> Command {
        if self.history.go(Some(Page::Playlist(playlist.to_owned()))) {
            self.left_page();
        }
        self.load(playlist)
    }

    /// Moves to any page and returns the command that settles it.
    pub fn open(&mut self, page: Page) -> Option<Command> {
        if self.history.go(Some(page)) {
            self.left_page();
        }
        self.settle()
    }

    /// A page change ends the like sentence and any play pressed on the shelf:
    /// the row the play names is no longer on screen.
    pub(super) fn left_page(&mut self) {
        // The wait for the dated feeds belongs to one visit: reopening Home
        // arms its own.
        self.held_since = None;
        self.hold_spent = false;
        self.likes.clear_notice();
        self.track_filter.clear();
        self.play = None;
        self.play_notice = None;
        self.adopt_show_order();
        if self.renaming.surface() == Surface::Band {
            self.renaming.dismiss();
        }
    }

    /// Takes the order recorded for the page's show.
    ///
    /// A show the user has sorted draws as it was left, and one this session
    /// has not seen starts on the wire's own order.
    pub(super) fn adopt_show_order(&mut self) {
        if let Some(Page::Show(uri)) = self.history.current() {
            self.show_order = self.show_orders.get(uri).copied().unwrap_or_default();
        }
    }

    /// Moves to the playlists grid.
    ///
    /// The stale-reply guard is cleared so a late reply for the playlist just
    /// left is discarded. A settled list is kept for the page Back returns to;
    /// an in-flight or failed one is dropped, because nothing will ever settle
    /// it and the root must not sit on a spinner.
    pub fn to_playlists(&mut self) {
        if self.history.go(None) {
            self.left_page();
        }
        self.reset_details();
    }

    /// Moves to Home, drawing a new shelf only when this move changes the page.
    pub fn go_home(&mut self, now_ms: u64) {
        if self.history.go(Some(Page::Home)) {
            self.left_page();
            self.home.open(now_ms);
        }
        self.reset_details();
    }

    /// A library that opens on Home, with the first shelf already drawn.
    pub fn starting_home(now_ms: u64) -> Library {
        let mut library = Library {
            history: History::starting_at(Page::Home),
            ..Library::default()
        };
        library.home.open(now_ms);
        library
    }

    /// The page's detail loads, dropped along with the wants that name them.
    ///
    /// A settled tracks read is kept for the page Back returns to; the artist
    /// page's Follow state goes because it names a page no longer on screen.
    fn reset_details(&mut self) {
        self.wanted = None;
        self.wanted_album = None;
        self.wanted_artist = None;
        self.wanted_show = None;
        self.follow = Follow::default();
        if matches!(self.tracks, Load::Loading | Load::Failed(_)) {
            self.tracks = Load::Idle;
        }
        if matches!(self.album, Load::Loading | Load::Failed(_)) {
            self.album = Load::Idle;
        }
        if matches!(self.artist, Load::Loading | Load::Failed(_)) {
            self.artist = Load::Idle;
        }
        if matches!(self.show, Load::Loading | Load::Failed(_)) {
            self.show = Load::Idle;
        }
    }

    /// Moves back one page and returns the command that settles it.
    ///
    /// A Back with nowhere to go changes nothing, so it never reloads the page
    /// it is already on.
    pub fn back(&mut self) -> Option<Command> {
        if !self.history.back() {
            return None;
        }
        self.left_page();
        self.settle()
    }

    /// Moves forward one page and returns the command that settles it.
    ///
    /// A Forward with nowhere to go changes nothing, so it never reloads the
    /// page it is already on.
    pub fn forward(&mut self) -> Option<Command> {
        if !self.history.forward() {
            return None;
        }
        self.left_page();
        self.settle()
    }

    /// Whether there is a page to go back to.
    pub fn can_back(&self) -> bool {
        self.history.can_back()
    }

    /// Whether there is a page to go forward to.
    pub fn can_forward(&self) -> bool {
        self.history.can_forward()
    }
}
