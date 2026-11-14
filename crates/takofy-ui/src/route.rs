//! The library's current page, and the history that moves between pages.
//!
//! A page is Home or one of the four kinds the main panel can show; `None` is
//! the library root, the playlists grid. Nothing here draws or fetches.

use takofy_model::uri::{UriKind, parse};

/// One page the main panel can show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Page {
    /// Home: the shelf of what was saved most recently.
    ///
    /// It carries no payload, because there is one Home and nothing names it.
    /// It is a page rather than the root, which is the playlists grid.
    Home,
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
    /// reads are keyed by. Home names no URI and answers the empty string, so
    /// nothing may read this to decide what a page is: the variant decides
    /// that, and Home has no address to read.
    pub fn uri(&self) -> &str {
        match self {
            Page::Home => "",
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
            Page::Home => "Home",
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

/// A destination text names: a page to open, or a playable URI to queue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Link {
    /// A page the text addresses.
    Page(Page),
    /// A track or episode, by URI, as the row to insert.
    Play(String),
}

/// The destination `text` names, or `None` when it names none.
///
/// Pure: it reads no state and asks no I/O. It never makes [`Page::Search`]
/// either, because a destination is a page and text that is not one stays a
/// query.
///
/// A bare `spotify:` uri is read as itself. A web address is rewritten to the
/// uri it stands for first, so one parse decides both. The older
/// `spotify:user:<name>:playlist:<id>` shape is canonicalised to the plain
/// playlist uri, because the name is not part of what the page addresses.
pub fn link(text: &str) -> Option<Link> {
    let text = text.trim();
    if text.is_empty() || text.contains(char::is_whitespace) {
        return None;
    }
    let uri = if text.starts_with("spotify:") {
        text.to_owned()
    } else {
        web_uri(text)?
    };

    let parsed = parse(&uri)?;
    if !plain_id(parsed.id) || parsed.owner.is_some_and(|owner| !plain_id(owner)) {
        return None;
    }
    let kind = parsed.kind;
    let canonical = match parsed.owner {
        Some(_) => format!("spotify:playlist:{}", parsed.id),
        None => uri,
    };

    match kind {
        UriKind::Playlist => Some(Link::Page(Page::Playlist(canonical))),
        UriKind::Album => Some(Link::Page(Page::Album(canonical))),
        UriKind::Artist => Some(Link::Page(Page::Artist(canonical))),
        UriKind::Show => Some(Link::Page(Page::Show(canonical))),
        UriKind::Track | UriKind::Episode => Some(Link::Play(canonical)),
        UriKind::Local => None,
    }
}

/// The `spotify:` uri a web address stands for, or `None` when it stands for
/// none.
///
/// Only the plain `<kind>/<id>` path and the older `user/<name>/playlist/<id>`
/// one are read. Everything else — a port, another host, an embed path, extra
/// segments — is refused rather than normalised.
fn web_uri(text: &str) -> Option<String> {
    let rest = strip_prefix_ignoring_case(text, HTTPS_SCHEME)
        .or_else(|| strip_prefix_ignoring_case(text, HTTP_SCHEME))?;
    let (host, path) = rest.split_once('/')?;
    let host = strip_prefix_ignoring_case(host, WWW).unwrap_or(host);
    if !host.eq_ignore_ascii_case(WEB_HOST) {
        return None;
    }

    let path = before_share_suffix(path);
    let path = path.strip_suffix('/').unwrap_or(path);
    let mut segments = path.split('/');
    let first = segments.next()?;
    let segments: Vec<&str> = if is_locale(first) {
        segments.collect()
    } else {
        std::iter::once(first).chain(segments).collect()
    };

    // A colon in any segment would splice this path into the colon-delimited
    // uri shape, so a path carrying one is not the address it looks like.
    if segments.iter().any(|segment| segment.contains(':')) {
        return None;
    }

    match segments.as_slice() {
        [kind, id] => Some(format!("spotify:{kind}:{id}")),
        [LEGACY_USER, name, LEGACY_PLAYLIST, id] => {
            Some(format!("spotify:user:{name}:playlist:{id}"))
        }
        _ => None,
    }
}

/// `text` without `prefix`, compared without regard to ASCII case.
fn strip_prefix_ignoring_case<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    let head = text.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &text[prefix.len()..])
}

/// The path up to its share suffix, if it carries one.
fn before_share_suffix(path: &str) -> &str {
    match path.find(['?', '#']) {
        Some(at) => &path[..at],
        None => path,
    }
}

/// Whether a path's first segment is a locale marker, `intl-` and a language.
fn is_locale(segment: &str) -> bool {
    segment.strip_prefix(LOCALE_PREFIX).is_some_and(|code| {
        code.len() == LOCALE_LEN && code.chars().all(|letter| letter.is_ascii_alphabetic())
    })
}

/// Whether `id` is a bare id, with nothing else in it.
///
/// The parse does not validate the id's shape, so the refusal is here: a real
/// Spotify id is base62, so a colon, a query, a fragment, a path, a space,
/// punctuation or non-ASCII text means the text is not an address. The length
/// is not checked, because a wrong-length id fails the same way any bad uri
/// does.
fn plain_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|character| character.is_ascii_alphanumeric())
}

/// The host the plain web address is served from.
const WEB_HOST: &str = "open.spotify.com";

/// The prefix a web address may carry in front of the bare host.
const WWW: &str = "www.";

const HTTPS_SCHEME: &str = "https://";
const HTTP_SCHEME: &str = "http://";

/// The prefix a locale-aware web address puts before its first segment.
const LOCALE_PREFIX: &str = "intl-";

/// How many letters a locale marker's language is.
const LOCALE_LEN: usize = 2;

/// The first segment of the older web playlist path.
const LEGACY_USER: &str = "user";

/// The third segment of the older web playlist path.
const LEGACY_PLAYLIST: &str = "playlist";

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
    /// A history that starts on `page`, with nothing behind it.
    ///
    /// The page a session opens on is not a move made from anywhere, so Back
    /// has nowhere to go until the first one. Every later move records its
    /// previous page as usual.
    pub fn starting_at(page: Page) -> History {
        History {
            current: Some(page),
            ..History::default()
        }
    }

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
