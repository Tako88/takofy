//! Which name, description, owner and cover head a playlist page.
//!
//! The sidebar entry for the page names it when that entry has a real name. A
//! page the sidebar does not name, or names only by its URI, is headed by the
//! page's own read. Fields are never mixed from the two: an unnamed entry's
//! title is the URI, so it must yield entirely to the page.

use takofy_model::{CoverOrigin, CoverSet, Owner, PlaylistEntry, PlaylistHead};

use crate::sidebar_kind::SidebarKind;
use crate::verbs::LibraryRights;

use super::{Library, Load, TracksFor, listed};

/// Where the band's fields come from.
enum BandFrom<'a> {
    /// The sidebar entry the list read named.
    Entry(&'a PlaylistEntry),
    /// The page's own read, with the page it belongs to.
    Head(&'a PlaylistHead, &'a TracksFor),
}

/// What the band draws, owned so the view can borrow the library mutably beside
/// it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BandSource {
    pub uri: String,
    pub name: String,
    pub description: Option<String>,
    pub owner: Option<Owner>,
    pub cover: Option<CoverSet>,
    /// Where the cover came from, when the source names a cover at all.
    pub origin: Option<CoverOrigin>,
}

impl BandSource {
    /// The source a named entry gives, its fields read off the entry.
    fn of(entry: &PlaylistEntry) -> Self {
        Self {
            uri: entry.uri.clone(),
            name: entry.name.clone(),
            description: entry.description.clone(),
            owner: entry.owner.clone(),
            cover: entry.cover_set().cloned(),
            origin: entry.cover.as_ref().map(|cover| cover.origin),
        }
    }

    /// The source the page's own read gives, its picture ahead of the reply's
    /// cover.
    fn headed(head: &PlaylistHead, tracks: &TracksFor) -> Self {
        Self {
            uri: tracks.playlist.clone(),
            name: head.name.clone(),
            description: head.description.clone(),
            owner: head.owner.clone(),
            cover: head.picture.clone().or_else(|| tracks.cover.clone()),
            origin: head.picture.as_ref().map(|_| CoverOrigin::Chosen),
        }
    }
}

impl Library {
    /// The one rule for what heads a playlist page.
    ///
    /// A sidebar entry with a real name wins entirely. Otherwise the page's own
    /// read heads it, when that read is the one on screen. Otherwise the page
    /// has nothing to say above its rows.
    fn band_from(&self) -> Option<BandFrom<'_>> {
        if let Some(entry) = self.selected_entry()
            && entry.named
        {
            return Some(BandFrom::Entry(entry));
        }
        let selected = self.selected()?;
        let Load::Ready(tracks) = &self.tracks else {
            return None;
        };
        if tracks.playlist != selected {
            return None;
        }
        Some(BandFrom::Head(tracks.head.as_ref()?, tracks))
    }

    /// The selected playlist's name, from whichever source heads the page.
    pub fn selected_name(&self) -> Option<&str> {
        match self.band_from()? {
            BandFrom::Entry(entry) => Some(entry.name.as_str()),
            BandFrom::Head(head, _) => Some(head.name.as_str()),
        }
    }

    /// What the selected page's band draws, when something heads it.
    pub fn band_source(&self) -> Option<BandSource> {
        Some(match self.band_from()? {
            BandFrom::Entry(entry) => BandSource::of(entry),
            BandFrom::Head(head, tracks) => BandSource::headed(head, tracks),
        })
    }

    /// Whether the band's heart may act, and in which state.
    ///
    /// An unsaved page is only offered a live heart once a rootlist revision
    /// exists to link against, and never while a write is in flight: the press
    /// would carry a revision the write is already replacing.
    pub fn band_saved(&self, uri: &str) -> Option<bool> {
        match self.is_saved(uri)? {
            true => Some(true),
            false => (self.rootlist.is_some() && !self.write_in_flight()).then_some(false),
        }
    }

    /// What a band's rename field opens on.
    ///
    /// The band draws whichever name heads the page, so that is what its field
    /// holds. A band asked for before its page is on screen falls back to a row
    /// that names the list — never to an unnamed row's URI, which is a stand-in
    /// for a name the band never draws. A page nothing names opens on nothing.
    pub(super) fn band_name(&self, uri: &str) -> Option<String> {
        if let Some(source) = self.band_source()
            && source.uri == uri
        {
            return Some(source.name);
        }
        match &self.playlists {
            Load::Ready(playlists) => playlists
                .iter()
                .find(|entry| entry.uri == uri && entry.named)
                .map(|entry| entry.name.clone()),
            Load::Idle | Load::Loading | Load::Failed(_) => None,
        }
    }

    /// What the band's menu and heart offer for `uri`.
    pub fn band_rights(&self, uri: &str) -> LibraryRights {
        LibraryRights {
            renamable: self.can_rename(uri),
            removable: listed(&self.playlists, uri, |entry| &entry.uri)
                && self.can_remove(SidebarKind::Playlists),
            saved: self.band_saved(uri),
        }
    }
}
