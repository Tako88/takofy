//! The library's current page, and the history that moves between pages.
//!
//! A page is one of the four kinds the main panel can show; `None` is the
//! library root, the playlists grid. Nothing here draws or fetches.

/// One page the main panel can show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Page {
    /// A playlist, by URI.
    Playlist(String),
    /// An album, by URI.
    Album(String),
    /// An artist, by URI.
    Artist(String),
    /// A show, by URI.
    Show(String),
    /// A search's results, by query.
    ///
    /// The payload is not a URI: it is the collapsed query the field stands for
    /// (R6). It can therefore look like one — a query of `spotify:playlist:x`
    /// is a legitimate thing to type — so every consumer decides by [`Page`]
    /// variant first and reads the payload second. Nothing may guess a page's
    /// kind from its payload's shape.
    Search(String),
}

impl Page {
    /// The page a row's credit link opens, from the row's resolved credit URI.
    ///
    /// An episode names its show as its first artist, so a show URI opens the
    /// show page; every other resolved credit is an artist, the only other
    /// credit a row carries. It is not asked for an album URI: a row's credit is
    /// an artist or a show, never an album.
    pub fn credit(uri: &str) -> Page {
        if uri.starts_with("spotify:show:") {
            Page::Show(uri.to_owned())
        } else {
            Page::Artist(uri.to_owned())
        }
    }

    /// The URI the page was opened with, or a search page's query.
    ///
    /// A search page's payload is a query and not a URI; it is returned here
    /// because it is the page's identity, the one string the history and the
    /// reads are keyed by.
    pub fn uri(&self) -> &str {
        match self {
            Page::Playlist(uri)
            | Page::Album(uri)
            | Page::Artist(uri)
            | Page::Show(uri)
            | Page::Search(uri) => uri,
        }
    }

    /// The page kind's name, as the crumb spells it.
    pub fn kind(&self) -> &'static str {
        match self {
            Page::Playlist(_) => "Playlist",
            Page::Album(_) => "Album",
            Page::Artist(_) => "Artist",
            Page::Show(_) => "Show",
            Page::Search(_) => "Search",
        }
    }
}

/// The oldest entry dropped when the history grows past this length.
const MAX: usize = 100;

/// Where the library is, and the pages Back and Forward return to.
///
/// `None` is the root. `back` and `forward` both hold `None` entries, because
/// leaving the root for a page is a move the history must be able to undo.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct History {
    current: Option<Page>,
    back: Vec<Option<Page>>,
    forward: Vec<Option<Page>>,
}

impl History {
    /// The page on screen, or `None` for the root.
    pub fn current(&self) -> Option<&Page> {
        self.current.as_ref()
    }

    /// Every page the history still holds, the one on screen included.
    ///
    /// A page can be dropped from a stack that is never walked again, so what a
    /// pruning rule leaves behind has to be readable without moving.
    pub fn pages(&self) -> impl Iterator<Item = &Page> + '_ {
        self.current
            .iter()
            .chain(self.back.iter().flatten())
            .chain(self.forward.iter().flatten())
    }

    /// Moves to `page`, recording the move.
    ///
    /// Moving to the page already on screen changes nothing and returns false.
    /// A new page clears `forward`, so Forward only retraces a Back.
    pub fn go(&mut self, page: Option<Page>) -> bool {
        if page == self.current {
            return false;
        }
        let previous = std::mem::replace(&mut self.current, page);
        self.back.push(previous);
        if self.back.len() > MAX {
            self.back.remove(0);
        }
        self.forward.clear();
        true
    }

    /// Moves to the previous page, if there is one.
    pub fn back(&mut self) -> bool {
        let Some(page) = self.back.pop() else {
            return false;
        };
        let previous = std::mem::replace(&mut self.current, page);
        self.forward.push(previous);
        true
    }

    /// Moves to the page Back left behind, if there is one.
    pub fn forward(&mut self) -> bool {
        let Some(page) = self.forward.pop() else {
            return false;
        };
        let previous = std::mem::replace(&mut self.current, page);
        self.back.push(previous);
        if self.back.len() > MAX {
            self.back.remove(0);
        }
        true
    }

    /// Whether there is a page to go back to.
    pub fn can_back(&self) -> bool {
        !self.back.is_empty()
    }

    /// Whether there is a page to go forward to.
    pub fn can_forward(&self) -> bool {
        !self.forward.is_empty()
    }

    /// Drops the pages `keep` rejects, then collapses duplicates.
    ///
    /// Only `Some` pages are judged: the root is always kept. The current page
    /// falls back to the root when it fails, and consecutive duplicates are
    /// collapsed, including a `back` entry equal to the current page, so Back
    /// never lands where you already are.
    pub fn retain(&mut self, mut keep: impl FnMut(&Page) -> bool) {
        if let Some(page) = &self.current
            && !keep(page)
        {
            self.current = None;
        }
        for list in [&mut self.back, &mut self.forward] {
            list.retain(|entry| match entry {
                Some(page) => keep(page),
                None => true,
            });
            list.dedup();
        }
        while self.back.last().is_some_and(|page| *page == self.current) {
            self.back.pop();
        }
        while self
            .forward
            .last()
            .is_some_and(|page| *page == self.current)
        {
            self.forward.pop();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn playlist(uri: &str) -> Option<Page> {
        Some(Page::Playlist(uri.to_owned()))
    }

    fn album(uri: &str) -> Option<Page> {
        Some(Page::Album(uri.to_owned()))
    }

    fn artist(uri: &str) -> Option<Page> {
        Some(Page::Artist(uri.to_owned()))
    }

    fn show(uri: &str) -> Option<Page> {
        Some(Page::Show(uri.to_owned()))
    }

    fn search(query: &str) -> Option<Page> {
        Some(Page::Search(query.to_owned()))
    }

    #[test]
    fn a_show_uri_credits_a_show_page() {
        assert_eq!(
            Page::credit("spotify:show:abc"),
            Page::Show("spotify:show:abc".to_owned())
        );
    }

    #[test]
    fn an_artist_uri_credits_an_artist_page() {
        assert_eq!(
            Page::credit("spotify:artist:abc"),
            Page::Artist("spotify:artist:abc".to_owned())
        );
    }

    #[test]
    fn anything_else_credits_an_artist_page() {
        assert_eq!(
            Page::credit("spotify:album:abc"),
            Page::Artist("spotify:album:abc".to_owned())
        );
    }

    #[test]
    fn go_records_the_previous_page_and_clears_forward() {
        let mut history = History::default();
        assert!(history.go(playlist("a")));
        assert!(history.go(playlist("b")));
        assert!(history.back());

        assert_eq!(history.current(), playlist("a").as_ref());
        assert!(history.can_forward());

        assert!(history.go(playlist("c")));
        assert_eq!(history.current(), playlist("c").as_ref());
        assert!(!history.can_forward(), "a new page clears forward");
    }

    #[test]
    fn go_to_the_current_page_is_a_no_op() {
        let mut history = History::default();
        history.go(playlist("a"));

        assert!(!history.go(playlist("a")));

        // The single move to `a` left only the root behind it.
        assert!(history.back());
        assert_eq!(history.current(), None);
        assert!(!history.can_back(), "the no-op must not grow back");
    }

    #[test]
    fn back_then_forward_round_trips() {
        let mut history = History::default();
        history.go(playlist("a"));
        history.go(album("b"));

        assert!(history.back());
        assert_eq!(history.current(), playlist("a").as_ref());
        assert!(history.forward());
        assert_eq!(history.current(), album("b").as_ref());
    }

    #[test]
    fn back_and_forward_on_an_empty_history_change_nothing() {
        let mut history = History::default();

        assert!(!history.back());
        assert_eq!(history.current(), None);
        assert!(!history.can_back());

        assert!(!history.forward());
        assert_eq!(history.current(), None);
        assert!(!history.can_forward());
    }

    #[test]
    fn a_new_page_after_back_clears_forward() {
        let mut history = History::default();
        history.go(playlist("a"));
        history.go(playlist("b"));
        history.back();

        history.go(playlist("c"));

        assert!(!history.can_forward());
        assert!(!history.forward());
    }

    #[test]
    fn the_cap_drops_the_oldest_entry() {
        let mut history = History::default();
        for index in 0..=MAX {
            history.go(playlist(&format!("p{index}")));
        }

        let mut steps = 0;
        while history.back() {
            steps += 1;
        }

        assert_eq!(steps, MAX, "the history is capped at {MAX}");
        // The oldest entry left is the first page pushed after the dropped
        // root, so the walk ends there rather than at the root.
        assert_eq!(history.current(), playlist("p0").as_ref());
    }

    #[test]
    fn retain_drops_a_rejected_page_from_back() {
        let mut history = History::default();
        history.go(playlist("a"));
        history.go(album("x"));
        history.go(playlist("b"));

        history.retain(|page| *page != Page::Album("x".to_owned()));

        assert_eq!(history.current(), playlist("b").as_ref());
        assert!(history.back());
        assert_eq!(history.current(), playlist("a").as_ref(), "x is skipped");
        assert!(history.back());
        assert_eq!(history.current(), None);
    }

    #[test]
    fn retain_drops_a_rejected_page_from_forward() {
        let mut history = History::default();
        history.go(playlist("a"));
        history.go(album("x"));
        history.back();

        history.retain(|page| *page != Page::Album("x".to_owned()));

        assert_eq!(history.current(), playlist("a").as_ref());
        assert!(!history.can_forward());
    }

    #[test]
    fn retain_falls_back_to_the_root_when_the_current_page_fails() {
        let mut history = History::default();
        history.go(playlist("a"));

        history.retain(|_| false);

        assert_eq!(history.current(), None);
        assert!(!history.can_back());
    }

    #[test]
    fn retain_collapses_consecutive_duplicates() {
        let mut history = History::default();
        history.go(album("a"));
        history.go(None);
        history.go(album("b"));

        history.retain(|page| *page != Page::Album("a".to_owned()));

        assert_eq!(history.current(), album("b").as_ref());
        assert!(history.back());
        assert_eq!(history.current(), None);
        assert!(
            !history.can_back(),
            "the two root entries must collapse into one"
        );
    }

    #[test]
    fn retain_collapses_a_back_entry_equal_to_the_current_page() {
        let mut history = History::default();
        history.go(album("a"));

        history.retain(|page| *page != Page::Album("a".to_owned()));

        assert_eq!(history.current(), None);
        assert!(!history.can_back(), "the root must not repeat the root");
    }

    #[test]
    fn retain_keeps_non_playlist_pages() {
        let mut history = History::default();
        history.go(album("album"));
        history.go(playlist("p"));
        history.go(artist("artist"));
        history.go(show("show"));

        history.retain(|page| !matches!(page, Page::Playlist(_)));

        assert_eq!(history.current(), show("show").as_ref());
        assert!(history.back());
        assert_eq!(history.current(), artist("artist").as_ref());
        assert!(history.back());
        assert_eq!(history.current(), album("album").as_ref());
        assert!(history.back());
        assert_eq!(history.current(), None);
    }

    #[test]
    fn retain_keeps_non_playlist_pages_in_forward() {
        let mut history = History::default();
        history.go(album("album"));
        history.go(artist("artist"));
        history.go(show("show"));
        history.back();
        history.back();
        assert_eq!(history.current(), album("album").as_ref());

        history.retain(|page| !matches!(page, Page::Playlist(_)));

        assert!(history.forward());
        assert_eq!(history.current(), artist("artist").as_ref());
        assert!(history.forward());
        assert_eq!(history.current(), show("show").as_ref());
    }

    #[test]
    fn a_page_names_its_uri_and_kind() {
        let cases = [
            (Page::Playlist("p".to_owned()), "p", "Playlist"),
            (Page::Album("a".to_owned()), "a", "Album"),
            (Page::Artist("r".to_owned()), "r", "Artist"),
            (Page::Show("s".to_owned()), "s", "Show"),
            (Page::Search("q".to_owned()), "q", "Search"),
        ];
        for (page, uri, kind) in cases {
            assert_eq!(page.uri(), uri);
            assert_eq!(page.kind(), kind);
        }
    }

    /// A query can be anything the field accepts, including something that
    /// spells a URI. Such a page is a search and must never be mistaken for the
    /// page that URI names: the payload is read, the kind is decided by the
    /// variant.
    #[test]
    fn a_search_query_that_spells_a_uri_is_still_a_search() {
        let page = Page::Search("spotify:playlist:x".to_owned());

        assert_eq!(page.kind(), "Search");
        assert_eq!(page.uri(), "spotify:playlist:x");
        assert_ne!(page, Page::Playlist("spotify:playlist:x".to_owned()));
    }

    /// The history is pruned of playlists that left the library, and a search
    /// whose query spells a playlist URI must survive that pruning: it is not a
    /// page the playlist list can name.
    #[test]
    fn retain_keeps_a_search_query_that_spells_a_playlist_uri() {
        let mut history = History::default();
        history.go(search("spotify:playlist:x"));

        history.retain(|page| !matches!(page, Page::Playlist(_)));

        assert_eq!(history.current(), search("spotify:playlist:x").as_ref());
    }

    #[test]
    fn the_same_query_twice_is_one_page() {
        let mut history = History::default();
        history.go(search("never gonna"));

        assert!(!history.go(search("never gonna")));
        assert!(history.go(search("never gonna give")));
    }
}
