//! The library's current page, and the history that moves between pages.
//!
//! A page is one of the four kinds the main panel can show; `None` is the
//! library root, the playlists grid. Nothing here draws or fetches.

use takofy_model::uri::{UriKind, parse};

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
    /// The payload is not a URI: it is the collapsed query the field stands
    /// for. It can therefore look like one — a query of `spotify:playlist:x`
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
        if credit_is_show(uri) {
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

/// Whether a credit URI names a show rather than an artist.
///
/// The rule [`Page::credit`] opens a page by.
pub fn credit_is_show(uri: &str) -> bool {
    parse(uri).is_some_and(|parsed| parsed.kind == UriKind::Show)
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
mod tests;
