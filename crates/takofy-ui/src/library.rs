//! The load state for the playlists and their tracks.
//!
//! The library is a reducer over worker updates: it decides what is on screen
//! and which load a reply belongs to, and it hands the view a command when the
//! user asks for one.

use std::collections::HashMap;
use std::ops::Range;

use takofy_model::{
    AlbumDetail, ArtistDetail, CoverOrigin, CoverUrl, FollowedArtist, FollowedShow, PlaylistAccess,
    PlaylistCover, PlaylistEntry, Revision, SavedAlbum, ShowDetail, TrackEntry,
};

use crate::cover_refresh::{CoverRefresh, first_track_uri};
use crate::discography::Filter;
use crate::follow::Follow;
use crate::library_filter::{RootFilter, RootKind};
use crate::likes::{self, Likes};
use crate::lyrics::LyricsPanel;
use crate::menu::{AddTarget, LibraryItem};
use crate::message::{Command, Failure, Update};
use crate::naming::{Naming, Renaming, Surface};
use crate::rail_filter::RailFilter;
use crate::releases::Releases;
use crate::removal;
use crate::reorder;
use crate::route::{History, Page};
use crate::search::Search;
use crate::sidebar_kind::{SidebarFilter, SidebarKind};
use crate::track_filter::TrackFilter;

/// How the library page draws its saved things.
///
/// The same items in either density, so the choice is one field and not two
/// lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum MediaLayout {
    /// A grid of tiles: art over a name and a subline.
    #[default]
    Grid,
    /// Rows with type and size columns, under a head.
    List,
}

/// What a loadable value is doing.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Load<T> {
    /// Nothing has been asked for.
    #[default]
    Idle,
    /// A request is in flight.
    Loading,
    /// The value is ready to draw.
    Ready(T),
    /// The request failed and may be retried.
    Failed(Failure),
}

/// The tracks of one playlist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TracksFor {
    pub playlist: String,
    pub tracks: Vec<TrackEntry>,
    /// What the page may change, when a read has carried the list's own state.
    pub access: Option<PlaylistAccess>,
    /// Why the last write for this page failed, until the next gesture or the
    /// next adopted read.
    ///
    /// The sentence is the whole affordance: "Try again" means drag again, so
    /// there is no Retry that could write twice for one gesture.
    pub notice: Option<Failure>,
}

/// What the playlist band draws for its cover.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BandCover<'a> {
    /// The selected playlist's tracks are still loading, so the band waits
    /// rather than fetching a first-track cover it may replace with the
    /// context image a moment later.
    Pending,
    /// The cover the playlist entry names, if any.
    Entry(Option<&'a CoverUrl>),
}

/// The band's cover source: the playlist entry alone, once its tracks have
/// settled.
///
/// The band never derives a cover from the track list: the entry already holds
/// the picture or the first track's album cover, and a second derivation could
/// disagree with it.
pub fn band_cover(entry: &PlaylistEntry, settled: bool) -> BandCover<'_> {
    if settled {
        BandCover::Entry(entry.cover_url())
    } else {
        BandCover::Pending
    }
}

/// The artist page's state, borrowed for one frame.
pub struct ArtistPage<'a> {
    /// The artist's load, so the page can draw its spinner or failure.
    pub load: &'a Load<ArtistDetail>,
    /// The loaded artist, only when it names the page on screen.
    pub current: Option<&'a ArtistDetail>,
    /// The Follow button's state.
    pub follow: &'a mut Follow,
    /// The discography's filter row.
    pub filter: &'a mut Filter,
    /// The per-release label store the discography fills in.
    pub releases: &'a mut Releases,
    /// The artists this one is heard with, once they are named.
    pub related: &'a Load<Vec<FollowedArtist>>,
    /// The liked-songs set, so a row's menu knows its library row.
    pub likes: &'a Likes,
}

/// The sidebar's playlists and the tracks the current page has loaded.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Library {
    pub playlists: Load<Vec<PlaylistEntry>>,
    /// The shows the user follows, as the library root draws them.
    pub followed: Load<Vec<FollowedShow>>,
    /// Whether the root draws its saved things as tiles or as rows.
    pub layout: MediaLayout,
    /// The artists the user follows, as the library root draws them.
    pub followed_artists: Load<Vec<FollowedArtist>>,
    /// The albums the user saved, as the library root draws them.
    pub saved_albums: Load<Vec<SavedAlbum>>,
    /// The liked-songs set, and the one write in flight against it.
    pub likes: Likes,
    /// The artists the open artist page is heard with, once they are named.
    pub related: Load<Vec<FollowedArtist>>,
    /// The artist the likeness list belongs to, so a stale reply is told apart.
    wanted_related: Option<String>,
    /// The panel under the library: its tab and the playing track's lyrics.
    pub lyrics: LyricsPanel,
    /// The topbar's search: the query, the menu's mark and its two reads.
    ///
    /// It is keyed by the query rather than by a page, so it survives every
    /// page change; only the layout root's own reset could drop it.
    pub search: Search,
    pub tracks: Load<TracksFor>,
    /// The album the album page draws.
    pub album: Load<AlbumDetail>,
    /// The show the show page draws.
    pub show: Load<ShowDetail>,
    /// The artist the artist page draws.
    pub artist: Load<ArtistDetail>,
    /// The artist page's Follow state.
    pub follow: Follow,
    /// The artist page's discography filter. Unlike `follow`, going home does
    /// not reset it: home then Back reloads the same artist, so the choice is
    /// kept, and opening a different artist is what resets it.
    pub filter: Filter,
    /// The per-release track labels the discography fills in as it scrolls.
    pub releases: Releases,
    /// The library root's filter row. Unlike `filter`, going home does not
    /// reset it: home then Back reloads the same root, so the choice is kept,
    /// and the root is the page the choice belongs to.
    pub root_filter: RootFilter,
    /// The sidebar's kind chips. Like `root_filter`, opening a page or going
    /// home does not reset it.
    pub sidebar_kind: SidebarFilter,
    /// The playlist page's filter. A page change clears it, because the query
    /// names one playlist's rows; a reload of the same playlist keeps it.
    pub track_filter: TrackFilter,
    /// The rail's name filter. A page change and a reload of any library list
    /// keep it, because the query names the rail's rows rather than a page's;
    /// only the field's clear control, its Escape, and the create field's
    /// opening clear it.
    pub rail_filter: RailFilter,
    /// The playlist `tracks` currently concerns; keeps a stale reply out.
    ///
    /// It is not the page: a reply that lands while the user is on an album
    /// page is kept, so Back finds it ready.
    wanted: Option<String>,
    /// The album `album` currently concerns; keeps a stale reply out.
    wanted_album: Option<String>,
    /// The show `show` currently concerns; keeps a stale reply out.
    wanted_show: Option<String>,
    /// The artist `artist` currently concerns; keeps a stale reply out.
    wanted_artist: Option<String>,
    /// Where the library is, and the pages Back and Forward return to.
    history: History,
    /// Context images, keyed by the playlist that owns them, so an upgrade that
    /// arrives before the playlists list is ready is not lost.
    context_covers: HashMap<String, CoverUrl>,
    /// Which derived covers a row edit has invalidated, and the asks in flight.
    ///
    /// A cover read says nothing about why the cover it carries is what it is,
    /// so a reply may only clear one when it answers an ask this holds.
    cover_refresh: CoverRefresh,
    /// The write in flight: its playlist, the rows before it and the rows after.
    ///
    /// While one is set, no read's order is adopted for that playlist. A read
    /// taken after the write reached the server can carry the new revision with
    /// the old order, so adopting on the revision would undo the drag.
    pending: Option<reorder::Pending>,
    /// The rootlist's revision, and whether the sidebar's rows are its indices.
    rootlist: Option<PlaylistAccess>,
    /// Whether the last playlists read was dropped while a write was pending.
    ///
    /// Its access has to be dropped with it: the access follows the list it
    /// belongs to, and a revision only means something beside that order.
    rootlist_dropped: bool,
    /// Whether a failed link owes the list a fresh read, because the link may
    /// have landed with only its reply lost.
    rootlist_stale: bool,
    /// The sidebar write in flight. It excludes the page's write and itself.
    sidebar_pending: Option<SidebarPending>,
    /// Why the last sidebar write failed, until a gesture or a read clears it.
    pub sidebar_notice: Option<SidebarWrite>,
    /// What is known about one album's or show's place in the library, beyond
    /// what the lists say.
    ///
    /// A write's echo and a probe's answer are the same fact, and both outlive
    /// the lists: a saved album is only listed once the albums read has run, and
    /// an add names no card at all.
    saved_known: Vec<(String, bool)>,
    /// The URI a saved-state probe is in flight for.
    saved_probe: Option<String>,
    /// The library head's new-playlist field.
    pub naming: Naming,
    /// The in-place rename field, on a sidebar row or a playlist band.
    pub renaming: Renaming,
    /// Whether the sidebar list should show its first row once.
    pub reveal_top: bool,
}

/// Which sidebar write is in flight, and what it would put back.
#[derive(Debug, Clone, PartialEq, Eq)]
enum SidebarPending {
    /// A rootlist move, holding the playlists as they were before the drop.
    Reorder { before: Vec<PlaylistEntry> },
    /// Tracks being appended to one playlist, and how many of them there are.
    Add { playlist: String, count: usize },
    /// A playlist being created and linked, which names no list yet.
    Create,
    /// One playlist being unlinked from the rootlist, and the row it named.
    Remove { playlist: String, index: usize },
    /// One album or show being written into its set, or out of it.
    Saved { uri: String, saved: bool },
    /// One playlist being renamed, which names no rootlist order.
    Rename { playlist: String },
    /// One playlist being put back on the rootlist, and the row it names.
    Link { playlist: String },
}

impl SidebarPending {
    /// Whether a playlists read taken now could disagree with this write.
    ///
    /// A collection removal writes no rootlist order, so a read may land beside
    /// it. Every other write either names the rootlist or is answered with the
    /// revision the read's access would replace, so both wait for it.
    fn holds_rootlist(&self) -> bool {
        !matches!(
            self,
            Self::Saved { .. } | Self::Rename { .. } | Self::Link { .. }
        )
    }
}

/// Why a sidebar write failed, as the row's list says it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarWrite {
    /// A playlist could not be moved.
    Reorder,
    /// A track could not be added to a playlist.
    Add,
    /// A playlist could not be created.
    Create,
    /// A playlist was created but could not be put on the rootlist.
    Link,
    /// A playlist was created but the pick it was made for did not land.
    Unfilled,
    /// A playlist was created but the rename that names it did not land.
    Unnamed,
    /// An album or show could not be saved into the library.
    Save,
    /// A thing could not be removed from the library.
    Remove,
    /// A playlist could not be renamed.
    Rename,
}

/// Whether a ready list of the library holds `uri`.
fn listed<T>(load: &Load<Vec<T>>, uri: &str, of: impl Fn(&T) -> &str) -> bool {
    matches!(load, Load::Ready(items) if items.iter().any(|item| of(item) == uri))
}

/// Whether a chip naming artists is the one the user is looking at.
///
/// Either surface counts: the root's chip row and the sidebar's are two ways to
/// ask for the same list.
fn artists_chosen(root: RootKind, sidebar: SidebarKind) -> bool {
    // All draws every kind, so it needs the lists each chip would have read.
    matches!(root, RootKind::All | RootKind::Artists) || sidebar == SidebarKind::Artists
}

/// Whether a chip naming albums is the one the user is looking at.
fn albums_chosen(root: RootKind, sidebar: SidebarKind) -> bool {
    matches!(root, RootKind::All | RootKind::Albums) || sidebar == SidebarKind::Albums
}

impl Library {
    /// Folds one worker update into the state.
    ///
    /// The sign-in and library updates are order-independent: `SignedIn` only
    /// starts a load that has not started, so a `Playlists` that arrives first
    /// is left alone.
    pub fn apply(&mut self, update: &Update) {
        match update {
            Update::SignedIn => {
                if self.playlists == Load::Idle {
                    self.playlists = Load::Loading;
                }
            }
            // A read's order is never adopted while a rootlist write is pending:
            // it may have been taken before the write reached the server, or
            // after, and the confirmation cannot tell the two apart. Its access
            // goes with it, and the dropped read owes a fresh one.
            Update::Playlists(_)
                if self
                    .sidebar_pending
                    .as_ref()
                    .is_some_and(SidebarPending::holds_rootlist) =>
            {
                self.rootlist_dropped = true;
                self.rootlist_stale = true;
            }
            Update::Playlists(playlists) => {
                let mut playlists = playlists.clone();
                for entry in &mut playlists {
                    // A cached context image is a derived cover, so it may fill a
                    // hole or replace another derived one, never a chosen picture.
                    let chosen = entry
                        .cover
                        .as_ref()
                        .is_some_and(|cover| cover.origin == CoverOrigin::Chosen);
                    if chosen {
                        continue;
                    }
                    if let Some(cover) = self.context_covers.get(&entry.uri) {
                        entry.cover = Some(PlaylistCover::derived(cover.clone()));
                    }
                }
                let present: Vec<String> =
                    playlists.iter().map(|entry| entry.uri.clone()).collect();
                self.context_covers.retain(|uri, _| present.contains(uri));
                let listed = |uri: &str| present.iter().any(|listed| listed == uri);
                self.prune_playlist_pages(&listed);
                // A playlist the read no longer lists is never asked for again.
                self.cover_refresh.retain(&present);
                self.playlists = Load::Ready(playlists);
                // A list is not movable until its own access arrives, so a read
                // can never be reordered on the previous read's revision.
                self.rootlist = None;
                self.rootlist_dropped = false;
                // A failed removal asked for this read, so its sentence has to
                // outlive the answer; a chip change or the next write clears it.
                if self.sidebar_notice != Some(SidebarWrite::Remove) {
                    self.sidebar_notice = None;
                }
            }
            Update::PlaylistFailed(failure) => {
                self.playlists = Load::Failed(*failure);
            }
            // Every sidebar write may replace the revision its read named, so an
            // access waits for the reply; the order it belongs to is held either
            // way, and a fresh read after the write is what keeps the column
            // movable.
            Update::RootlistAccess(access) => {
                if std::mem::take(&mut self.rootlist_dropped) {
                    self.rootlist_stale = true;
                    return;
                }
                if self
                    .sidebar_pending
                    .as_ref()
                    .is_some_and(SidebarPending::holds_rootlist)
                {
                    self.rootlist_stale = true;
                    return;
                }
                self.rootlist = Some(access.clone());
            }
            // The reply only replaces the held revision: the order already moved
            // at drop time, and reordering again here would move it twice.
            Update::PlaylistMoved { revision } => {
                if !matches!(self.sidebar_pending, Some(SidebarPending::Reorder { .. })) {
                    return;
                }
                self.sidebar_pending = None;
                if let Some(access) = &mut self.rootlist {
                    access.revision = revision.clone();
                }
                self.sidebar_notice = None;
            }
            // A failed move puts the rows back where they were, and says so.
            Update::PlaylistMoveFailed => {
                if !matches!(self.sidebar_pending, Some(SidebarPending::Reorder { .. })) {
                    return;
                }
                let Some(SidebarPending::Reorder { before }) = self.sidebar_pending.take() else {
                    return;
                };
                self.playlists = Load::Ready(before);
                self.sidebar_notice = Some(SidebarWrite::Reorder);
            }
            // A confirmed unlink drops the row and hands the next write its
            // revision.
            Update::PlaylistRemoved { playlist, revision } => {
                let (pending, index) = match &self.sidebar_pending {
                    Some(SidebarPending::Remove { playlist, index }) => (playlist.clone(), *index),
                    _ => return,
                };
                if pending != *playlist {
                    return;
                }
                self.sidebar_pending = None;
                self.sidebar_notice = None;
                // The list can move under the index the write named, and a
                // removal at a stale one would unlink a playlist the user never
                // picked. A list that no longer holds the row there is re-read
                // rather than guessed at.
                let named = match &self.playlists {
                    Load::Ready(playlists) => playlists
                        .get(index)
                        .is_some_and(|entry| entry.uri == *playlist),
                    Load::Idle | Load::Loading | Load::Failed(_) => false,
                };
                if !named {
                    self.rootlist = None;
                    self.rootlist_stale = true;
                    return;
                }
                if let Some(access) = &mut self.rootlist {
                    access.revision = revision.clone();
                }
                if let Load::Ready(playlists) = &mut self.playlists {
                    playlists.remove(index);
                }
                self.context_covers.remove(playlist);
                self.cover_refresh.forget(playlist);
                // The removed playlist may be the page on screen; the rule a
                // playlists read applies is the one that takes it home.
                let remaining: Vec<String> = match &self.playlists {
                    Load::Ready(playlists) => {
                        playlists.iter().map(|entry| entry.uri.clone()).collect()
                    }
                    Load::Idle | Load::Loading | Load::Failed(_) => Vec::new(),
                };
                self.prune_playlist_pages(&|uri| remaining.iter().any(|listed| listed == uri));
            }
            // The row stays and the held revision goes: the write may have
            // landed with only its reply lost, so the next read is what heals
            // it.
            Update::PlaylistLinked { playlist, revision } => {
                if !matches!(
                    &self.sidebar_pending,
                    Some(SidebarPending::Link { playlist: pending }) if pending == playlist
                ) {
                    return;
                }
                self.sidebar_pending = None;
                self.sidebar_notice = None;
                if let Some(access) = &mut self.rootlist {
                    access.revision = revision.clone();
                }
                // A link puts the row at the front, and the reply carries no row
                // to draw, so the list is re-read rather than invented.
                self.rootlist_stale = true;
            }
            Update::PlaylistLinkFailed { playlist } => {
                if !matches!(
                    &self.sidebar_pending,
                    Some(SidebarPending::Link { playlist: pending }) if pending == playlist
                ) {
                    return;
                }
                self.sidebar_pending = None;
                self.sidebar_notice = Some(SidebarWrite::Save);
            }
            Update::PlaylistRemoveFailed { playlist } => {
                let Some(SidebarPending::Remove {
                    playlist: pending, ..
                }) = &self.sidebar_pending
                else {
                    return;
                };
                if pending != playlist {
                    return;
                }
                self.sidebar_pending = None;
                self.sidebar_notice = Some(SidebarWrite::Remove);
                self.rootlist = None;
                self.rootlist_stale = true;
            }
            // The collection reply carries no body, so this echo is the whole
            // confirmation.
            Update::Saved { uri, saved } => self.end_saved(uri, *saved),
            // The probe answers for one URI, and its answer is worth keeping: the
            // page it was asked for stays on screen while the write follows it.
            Update::SavedState { uri, saved } => {
                if self.saved_probe.as_deref() != Some(uri.as_str()) {
                    return;
                }
                self.saved_probe = None;
                if let Some(saved) = saved {
                    self.remember_saved(uri, *saved);
                }
            }
            Update::SavedFailed { uri, saved } => {
                if !self.saved_pending(uri, *saved) {
                    return;
                }
                self.sidebar_pending = None;
                self.sidebar_notice = Some(if *saved {
                    SidebarWrite::Save
                } else {
                    SidebarWrite::Remove
                });
            }
            // Length and revision always move together, so an add never names a
            // revision whose length it has already counted.
            Update::TrackAdded { playlist, revision } => {
                let Some(SidebarPending::Add {
                    playlist: pending,
                    count,
                }) = &self.sidebar_pending
                else {
                    return;
                };
                if pending != playlist {
                    return;
                }
                let count = *count;
                self.sidebar_pending = None;
                // An add names the append index, so it cannot change the first
                // track of a list that already has one: a row showing a cover
                // keeps it. A row with no cover is the one the add may have
                // given a picture, and nothing else would ask for it.
                let uncovered = matches!(
                    &self.playlists,
                    Load::Ready(playlists)
                        if playlists
                            .iter()
                            .find(|entry| entry.uri == *playlist)
                            .is_some_and(|entry| entry.cover.is_none())
                );
                if let Load::Ready(playlists) = &mut self.playlists
                    && let Some(entry) = playlists.iter_mut().find(|entry| entry.uri == *playlist)
                {
                    if let Some(access) = &mut entry.access {
                        access.revision = revision.clone();
                    }
                    entry.tracks = entry.tracks.map(|held| held.saturating_add(count));
                }
                if uncovered {
                    self.drop_derived_cover(playlist);
                }
                // A page held for that playlist was read before the add, so it
                // shows the rows and names the revision of a list one track
                // shorter: dropping it makes the next visit reload. A read
                // already in flight still lands as the page if it answers
                // first, and names that older revision; a write started from
                // it is refused on the revision conflict.
                let held = match &self.tracks {
                    Load::Ready(tracks) => tracks.playlist == *playlist,
                    Load::Loading => self.wanted.as_deref() == Some(playlist.as_str()),
                    Load::Idle | Load::Failed(_) => false,
                };
                if held {
                    self.tracks = Load::Idle;
                    self.wanted = None;
                }
                self.sidebar_notice = None;
            }
            Update::AddFailed { playlist } => {
                if !matches!(
                    &self.sidebar_pending,
                    Some(SidebarPending::Add { playlist: pending, .. }) if pending == playlist
                ) {
                    return;
                }
                self.sidebar_pending = None;
                self.sidebar_notice = Some(SidebarWrite::Add);
            }
            // The row is drawn from the reply and never from the press: the URI
            // exists only once the create has answered, and a made-up one would
            // load a page that is not there.
            Update::PlaylistCreated { entry, revision } => {
                if !matches!(self.sidebar_pending, Some(SidebarPending::Create)) {
                    return;
                }
                self.sidebar_pending = None;
                self.naming.finished();
                self.sidebar_notice = None;
                if let Load::Ready(playlists) = &mut self.playlists
                    && !playlists.iter().any(|listed| listed.uri == entry.uri)
                {
                    playlists.insert(0, entry.clone());
                }
                // The link replaced the rootlist, so the revision it answered
                // with is the one the next create or move must name.
                if let Some(access) = &mut self.rootlist {
                    access.revision = revision.clone();
                }
                // The new row is the playlists' own, and it is at the top of a
                // list the user may have scrolled away from.
                self.sidebar_kind.choose(SidebarKind::Playlists);
                self.reveal_top = true;
            }
            // Nothing was created, so the text stays for a second attempt. The
            // POST may still have landed with only its reply lost, and the route
            // returns no id to check, so a retry can leave two playlists.
            Update::PlaylistCreateFailed => {
                if !matches!(self.sidebar_pending, Some(SidebarPending::Create)) {
                    return;
                }
                self.sidebar_pending = None;
                self.naming.failed();
                self.sidebar_notice = Some(SidebarWrite::Create);
            }
            // The playlist exists and cannot be created again, so the field goes
            // and the foot says what happened to the link.
            Update::PlaylistUnlinked => {
                if !matches!(self.sidebar_pending, Some(SidebarPending::Create)) {
                    return;
                }
                self.sidebar_pending = None;
                self.naming.finished();
                self.sidebar_notice = Some(SidebarWrite::Link);
                // The link may have landed with only its reply lost, so the held
                // revision names the rootlist as it was before it. Dropping it
                // refuses the next create or move until a read names a new one.
                self.rootlist = None;
                self.rootlist_stale = true;
            }
            // The list exists with nothing in it, and says so. A list the window
            // does not hold is ignored, as any other stale reply is.
            Update::PlaylistUnfilled { playlist } => {
                let Load::Ready(playlists) = &mut self.playlists else {
                    return;
                };
                let Some(entry) = playlists.iter_mut().find(|entry| entry.uri == *playlist) else {
                    return;
                };
                entry.tracks = Some(0);
                self.sidebar_notice = Some(SidebarWrite::Unfilled);
            }
            // The name the row shows is the typed one either way; the notice
            // says the server did not take it, so a reload is not a surprise.
            Update::PlaylistUnnamed { playlist } => {
                let Load::Ready(playlists) = &mut self.playlists else {
                    return;
                };
                if playlists.iter_mut().any(|entry| entry.uri == *playlist) {
                    self.sidebar_notice = Some(SidebarWrite::Unnamed);
                }
            }
            // A rename only counts for the row whose write is in flight; the
            // reply's revision replaces both copies of the holding.
            Update::PlaylistRenamed {
                playlist,
                name,
                revision,
            } => {
                let Some(SidebarPending::Rename { playlist: pending }) = &self.sidebar_pending
                else {
                    return;
                };
                if pending != playlist {
                    return;
                }
                self.sidebar_pending = None;
                self.sidebar_notice = None;
                if let Load::Ready(playlists) = &mut self.playlists
                    && let Some(entry) = playlists.iter_mut().find(|entry| entry.uri == *playlist)
                {
                    entry.name = name.clone();
                    entry.named = true;
                    if let Some(access) = &mut entry.access {
                        access.revision = revision.clone();
                    }
                }
                if let Load::Ready(tracks) = &mut self.tracks
                    && tracks.playlist == *playlist
                {
                    if let Some(access) = &mut tracks.access {
                        access.revision = revision.clone();
                    }
                    if tracks.notice == Some(Failure::Rename) {
                        tracks.notice = None;
                    }
                }
                if self.renaming.is_open() && self.renaming.uri() == playlist {
                    self.renaming.finished();
                }
            }
            // The field keeps its text so a retry is one press away, and the
            // foot says why; a band field also marks the page, because the foot
            // is invisible with the drawer closed.
            Update::PlaylistRenameFailed { playlist } => {
                let Some(SidebarPending::Rename { playlist: pending }) = &self.sidebar_pending
                else {
                    return;
                };
                if pending != playlist {
                    return;
                }
                self.sidebar_pending = None;
                self.sidebar_notice = Some(SidebarWrite::Rename);
                self.renaming.failed();
                if self.renaming.surface() == Surface::Band
                    && let Load::Ready(tracks) = &mut self.tracks
                    && tracks.playlist == *playlist
                {
                    tracks.notice = Some(Failure::Rename);
                }
            }
            Update::Tracks {
                playlist,
                tracks,
                cover,
                access,
            } => {
                // The cover upgrade is keyed by the update's own playlist, never
                // the selected one, so a reply for A cannot land on B.
                let authoritative = self.cover_refresh.answer(playlist);
                let entry = match &mut self.playlists {
                    Load::Ready(playlists) => {
                        playlists.iter_mut().find(|entry| entry.uri == *playlist)
                    }
                    Load::Idle | Load::Loading | Load::Failed(_) => None,
                };
                // A chosen picture is the user's own, so no read replaces or
                // clears it, and nothing cached may be drawn over it later.
                let chosen = entry.as_ref().is_some_and(|entry| {
                    entry
                        .cover
                        .as_ref()
                        .is_some_and(|cover| cover.origin == CoverOrigin::Chosen)
                });
                if !chosen {
                    match cover {
                        Some(cover) => {
                            self.context_covers.insert(playlist.clone(), cover.clone());
                            if let Some(entry) = entry {
                                entry.cover = Some(PlaylistCover::derived(cover.clone()));
                            }
                        }
                        // Only the reply to an ask may clear a cover: any other read
                        // says nothing about why the cover it lacks was dropped.
                        None if authoritative => {
                            self.context_covers.remove(playlist);
                            if let Some(entry) = entry {
                                entry.cover = None;
                            }
                        }
                        None => {}
                    }
                }
                if self.wanted.as_deref() == Some(playlist.as_str()) {
                    let pending = self
                        .pending
                        .as_ref()
                        .filter(|pending| pending.playlist == *playlist);
                    if let Some(pending) = pending {
                        let held = match &self.tracks {
                            Load::Ready(current) => current.access.clone(),
                            Load::Idle | Load::Loading | Load::Failed(_) => None,
                        };
                        self.tracks = Load::Ready(TracksFor {
                            playlist: playlist.clone(),
                            tracks: pending.after.clone(),
                            access: held.or_else(|| access.clone()),
                            notice: None,
                        });
                    } else {
                        self.tracks = Load::Ready(TracksFor {
                            playlist: playlist.clone(),
                            tracks: tracks.clone(),
                            access: access.clone(),
                            notice: None,
                        });
                    }
                }
            }
            Update::TracksFailed { playlist, failure } => {
                // A failed read still answers the ask, so the count keeps step
                // with the replies actually coming; otherwise a later ask never
                // reaches zero and an emptied playlist's `None` never clears its
                // cover.
                self.cover_refresh.answer(playlist);
                if self.wanted.as_deref() == Some(playlist.as_str()) {
                    self.tracks = Load::Failed(*failure);
                }
            }
            // The reply ends the write and names the revision the next one uses.
            Update::TrackMoved { playlist, revision } => {
                let (before, after) = self.pending_first_tracks(playlist);
                if !self.end_page_write(playlist, revision) {
                    return;
                }
                if before != after {
                    self.drop_derived_cover(playlist);
                }
            }
            // A removal's reply is a move's, plus the count it took out.
            Update::TracksRemoved { playlist, revision } => {
                let gone = self
                    .pending
                    .as_ref()
                    .filter(|pending| pending.playlist == *playlist)
                    .map_or(0, |pending| {
                        pending.before.len().saturating_sub(pending.after.len())
                    });
                // Read before the reply clears the write: these rows are the
                // only record of which track the list showed first.
                let (before, after) = self.pending_first_tracks(playlist);
                if !self.end_page_write(playlist, revision) {
                    return;
                }
                self.lower_page_count(playlist, gone);
                if before != after {
                    self.drop_derived_cover(playlist);
                }
            }
            // A failed write puts the rows back where they were, and says so.
            Update::MoveFailed { playlist, failure }
            | Update::RemoveFailed { playlist, failure } => {
                if !self.writing(playlist) {
                    return;
                }
                let Some(pending) = self.pending.take() else {
                    return;
                };
                // A page that is no longer on screen keeps its rows; Back reloads it.
                if self.selected() != Some(playlist.as_str()) {
                    return;
                }
                if let Load::Ready(tracks) = &mut self.tracks
                    && tracks.playlist == *playlist
                {
                    tracks.tracks = pending.before;
                    tracks.notice = Some(*failure);
                }
            }
            // A reply lands only while its own load is in flight: a stale one is
            // discarded, and a second reply cannot replace the first.
            Update::Album(detail) => {
                if self.wanted_album.as_deref() == Some(detail.uri.as_str())
                    && matches!(self.album, Load::Loading)
                {
                    self.album = Load::Ready(detail.clone());
                }
            }
            Update::AlbumFailed { album, failure } => {
                if self.wanted_album.as_deref() == Some(album.as_str())
                    && matches!(self.album, Load::Loading)
                {
                    self.album = Load::Failed(*failure);
                }
            }
            // The artist's reply is guarded the same way the album's is, and it
            // is the one thing that registers the releases the page may label.
            Update::Artist(detail) => {
                if self.wanted_artist.as_deref() == Some(detail.uri.as_str())
                    && matches!(self.artist, Load::Loading)
                {
                    self.releases.retain(&detail.releases);
                    self.artist = Load::Ready(detail.clone());
                }
            }
            Update::ArtistFailed { artist, failure } => {
                if self.wanted_artist.as_deref() == Some(artist.as_str())
                    && matches!(self.artist, Load::Loading)
                {
                    self.artist = Load::Failed(*failure);
                }
            }
            // The show's reply is guarded exactly as the album's is.
            Update::Show(detail) => {
                if self.wanted_show.as_deref() == Some(detail.show.uri.as_str())
                    && matches!(self.show, Load::Loading)
                {
                    self.show = Load::Ready(detail.clone());
                }
            }
            Update::ShowFailed { show, failure } => {
                if self.wanted_show.as_deref() == Some(show.as_str())
                    && matches!(self.show, Load::Loading)
                {
                    self.show = Load::Failed(*failure);
                }
            }
            // The followed list is a whole set, not keyed by URI: a reply is
            // accepted whenever it arrives, whatever page is on screen.
            Update::FollowedShows(shows) => {
                self.followed = Load::Ready(shows.clone());
            }
            Update::FollowedShowsFailed(failure) => {
                self.followed = Load::Failed(*failure);
            }
            Update::FollowedArtists(artists) => {
                self.followed_artists = Load::Ready(artists.clone());
            }
            Update::FollowedArtistsFailed(failure) => {
                self.followed_artists = Load::Failed(*failure);
            }
            Update::SavedAlbums(albums) => {
                self.saved_albums = Load::Ready(albums.clone());
            }
            Update::SavedAlbumsFailed(failure) => {
                self.saved_albums = Load::Failed(*failure);
            }
            // The liked set is a whole set, not keyed by URI: the state keeps its
            // own epoch guard so a read that predates a flip is dropped.
            Update::LikedTracks(tracks) => {
                self.likes.apply(&likes::Event::Set(tracks.clone()));
            }
            Update::LikedTracksFailed(failure) => {
                self.likes.apply(&likes::Event::SetFailed(*failure));
            }
            Update::Liked { tracks, liked } => {
                self.likes.apply(&likes::Event::Confirmed {
                    tracks: tracks.clone(),
                    liked: *liked,
                });
            }
            Update::LikeFailed { tracks, failure } => {
                self.likes.apply(&likes::Event::Failed {
                    tracks: tracks.clone(),
                    failure: *failure,
                });
            }
            // The likeness cards are keyed by the artist they belong to: the
            // page's own detail can be replaced while they are in flight.
            Update::Related { artist, cards } => {
                if self.wanted_related.as_deref() == Some(artist.as_str())
                    && matches!(self.related, Load::Loading)
                {
                    self.related = Load::Ready(cards.clone());
                }
            }
            Update::RelatedFailed { artist, failure } => {
                if self.wanted_related.as_deref() == Some(artist.as_str())
                    && matches!(self.related, Load::Loading)
                {
                    self.related = Load::Failed(*failure);
                }
            }
            Update::ReleaseTracks { .. } | Update::ReleaseTracksFailed { .. } => {
                self.releases.apply(update);
            }
            // An artist's removal rides the same unfollow the page's button
            // sends, so the reply is what takes its row out.
            Update::Followed {
                artist, followed, ..
            } => {
                self.follow.apply(update);
                // The sidebar row's own removal is ended here; a press on the
                // page or in a menu leaves nothing pending and is settled by the
                // answer alone.
                if !followed {
                    self.end_saved(artist, false);
                }
                self.settle_follow(artist, *followed);
            }
            Update::FollowFailed { artist, .. } => {
                self.follow.apply(update);
                if self.saved_pending(artist, false) {
                    self.sidebar_pending = None;
                    self.sidebar_notice = Some(SidebarWrite::Remove);
                }
            }
            // A lyrics reply is about a track, not a page, so it is forwarded
            // whatever page is on screen; the panel keeps its own guard.
            Update::Lyrics { .. } | Update::LyricsFailed { .. } => {
                self.lyrics.apply(update);
            }
            // A search reply is about a query, not a page, so it is forwarded
            // whatever page is on screen; the state keeps its own guard. The
            // same holds for the portraits a search answer draws.
            Update::Search { .. }
            | Update::SearchFailed { .. }
            | Update::Portraits { .. }
            | Update::PortraitsFailed { .. } => {
                self.search.apply(update);
            }
            Update::NeedsAuthorization
            | Update::SignInFailed(_)
            | Update::Transport(_)
            | Update::Cover { .. }
            | Update::CoverFailed { .. } => {}
        }
    }

    /// Drops every page and read for a playlist `present` no longer lists.
    ///
    /// Only playlist pages can be pruned; album, artist, show and search pages
    /// are not in the list and must survive it. A search's query may spell a
    /// playlist URI, so the variant is what decides, never the payload.
    fn prune_playlist_pages(&mut self, present: &dyn Fn(&str) -> bool) {
        if self.renaming.is_open() && !present(self.renaming.uri()) {
            self.renaming.force_close();
        }
        let page = self.history.current().cloned();
        self.history.retain(|page| match page {
            Page::Playlist(uri) => present(uri),
            Page::Album(_) | Page::Artist(_) | Page::Show(_) | Page::Search(_) => true,
        });
        if self.history.current().cloned() != page {
            self.left_page();
        }
        if self
            .wanted
            .as_deref()
            .is_some_and(|wanted| !present(wanted))
        {
            self.wanted = None;
            self.tracks = Load::Idle;
        }
        // A ready list names its playlist, so it is kept only while that
        // playlist exists. The root can leave one behind, with no wanted load to
        // prune it by.
        let ready = match &self.tracks {
            Load::Ready(tracks) => Some(tracks.playlist.clone()),
            Load::Idle | Load::Loading | Load::Failed(_) => None,
        };
        if ready.is_some_and(|playlist| !present(&playlist)) {
            self.tracks = Load::Idle;
        }
    }

    /// Whether a saved write of `uri` is in flight, in this direction.
    ///
    /// A page's own write asks this before sending: two writes for one saved
    /// thing would land in order and leave its row and its label disagreeing.
    pub fn saved_pending(&self, uri: &str, saved: bool) -> bool {
        matches!(
            &self.sidebar_pending,
            Some(SidebarPending::Saved { uri: pending, saved: pending_saved })
                if pending == uri && *pending_saved == saved
        )
    }

    /// Whether the library has `uri`, as the last read or the last write said.
    ///
    /// The write's echo outlives the list it would change: an add names no card,
    /// so the list cannot gain the row until its next read, and the heart would
    /// fall back off in between.
    pub fn is_saved(&self, uri: &str) -> Option<bool> {
        if let Some((_, saved)) = self.saved_known.iter().find(|(known, _)| known == uri) {
            return Some(*saved);
        }
        let kind = uri.strip_prefix("spotify:")?.split_once(':')?.0;
        let (here, ready) = match kind {
            "album" => (
                listed(&self.saved_albums, uri, |album| &album.uri),
                matches!(self.saved_albums, Load::Ready(_)),
            ),
            "show" => (
                listed(&self.followed, uri, |show| &show.uri),
                matches!(self.followed, Load::Ready(_)),
            ),
            "playlist" => (
                listed(&self.playlists, uri, |entry| &entry.uri),
                !matches!(self.playlists, Load::Idle | Load::Loading),
            ),
            // An artist's entry in the library is its follow, and the page's own
            // state is the only thing that knows it before the list is read.
            "artist" => (
                listed(&self.followed_artists, uri, |artist| &artist.uri)
                    || (self.follow.artist() == Some(uri) && self.follow.confirmed() == Some(true)),
                !matches!(self.followed_artists, Load::Idle),
            ),
            _ => return None,
        };
        if here {
            return Some(true);
        }
        ready.then_some(false)
    }

    /// Records one URI's place in the library, replacing an older answer.
    fn remember_saved(&mut self, uri: &str, saved: bool) {
        self.saved_known.retain(|(known, _)| known != uri);
        self.saved_known.push((uri.to_owned(), saved));
        // A page can only be one place at a time, so one answer is enough to keep.
        if self.saved_known.len() > 64 {
            self.saved_known.remove(0);
        }
    }

    /// Asks whether the page's own album or show is in the library.
    ///
    /// The lists cannot answer this: a saved album is listed only after the
    /// albums read has run, which happens when that chip is chosen. A band's
    /// heart would therefore be inert on every page opened from search or from
    /// another artist.
    pub fn saved_request(&mut self) -> Option<Command> {
        let uri = match self.page()? {
            crate::route::Page::Album(uri) | crate::route::Page::Show(uri) => uri.clone(),
            _ => return None,
        };
        if self.saved_known.iter().any(|(known, _)| *known == uri) {
            return None;
        }
        if self.saved_probe.as_deref() == Some(uri.as_str()) {
            return None;
        }
        self.saved_probe = Some(uri.clone());
        Some(Command::LoadSaved { uri })
    }

    /// Ends a saved write: a removal drops the row, an add remembers it.
    fn end_saved(&mut self, uri: &str, saved: bool) {
        if !self.saved_pending(uri, saved) {
            return;
        }
        self.sidebar_pending = None;
        self.sidebar_notice = None;
        self.remember_saved(uri, saved);
        if saved {
            // An add names no card, so the list cannot gain the row on its own.
            // Re-arming the read is what puts it in the sidebar; the next visit to
            // the chip that lists this kind draws it there.
            match uri
                .strip_prefix("spotify:")
                .and_then(|rest| rest.split_once(':'))
            {
                Some(("album", _)) => self.saved_albums = Load::Idle,
                Some(("show", _)) => self.followed = Load::Idle,
                _ => {}
            }
            return;
        }
        if let Load::Ready(albums) = &mut self.saved_albums {
            albums.retain(|album| album.uri != uri);
        }
        if let Load::Ready(shows) = &mut self.followed {
            shows.retain(|show| show.uri != uri);
        }
        if let Load::Ready(artists) = &mut self.followed_artists {
            artists.retain(|artist| artist.uri != uri);
        }
    }

    /// Ends the page write for `playlist` and holds `revision` for the next one.
    fn end_page_write(&mut self, playlist: &str, revision: &Revision) -> bool {
        if !self.writing(playlist) {
            return false;
        }
        self.pending = None;
        if let Load::Ready(tracks) = &mut self.tracks
            && tracks.playlist == *playlist
        {
            tracks.notice = None;
            if let Some(access) = &mut tracks.access {
                access.revision = revision.clone();
            }
        }
        if let Load::Ready(playlists) = &mut self.playlists
            && let Some(entry) = playlists.iter_mut().find(|entry| entry.uri == *playlist)
            && let Some(access) = &mut entry.access
        {
            access.revision = revision.clone();
        }
        true
    }

    /// Drops `playlist`'s derived cover and queues the read that replaces it.
    ///
    /// A chosen picture is the user's own, so no row edit invalidates it. The
    /// cached context image goes with the cover it was drawn from, and the ask
    /// is only queued: one drain turns it into a read.
    fn drop_derived_cover(&mut self, playlist: &str) {
        let entry = match &mut self.playlists {
            Load::Ready(playlists) => playlists.iter_mut().find(|entry| entry.uri == playlist),
            Load::Idle | Load::Loading | Load::Failed(_) => None,
        };
        if entry.as_ref().is_some_and(|entry| {
            entry
                .cover
                .as_ref()
                .is_some_and(|cover| cover.origin == CoverOrigin::Chosen)
        }) {
            return;
        }
        self.context_covers.remove(playlist);
        if let Some(entry) = entry {
            entry.cover = None;
        }
        self.cover_refresh.drop_for(playlist);
    }

    /// The first track of the page write in flight for `playlist`, before and
    /// after the write.
    ///
    /// Both are read before the reply ends the write, because that is when the
    /// rows it held are dropped. A reply with no write pending names no row, so
    /// it can change nothing.
    fn pending_first_tracks(&self, playlist: &str) -> (Option<String>, Option<String>) {
        let Some(pending) = self
            .pending
            .as_ref()
            .filter(|pending| pending.playlist == playlist)
        else {
            return (None, None);
        };
        (
            first_track_uri(&pending.before).map(str::to_owned),
            first_track_uri(&pending.after).map(str::to_owned),
        )
    }

    /// Brings the artist list in step with an answer about one artist.
    ///
    /// The list is a whole set read from the server, so an answer that disagrees
    /// with it is the only thing worth acting on: a row the server says is gone
    /// goes, and one it says should be there means the set is stale, which only a
    /// read can fix. An answer that agrees changes nothing, so a page's opening
    /// probe costs nothing.
    fn settle_follow(&mut self, artist: &str, followed: bool) {
        let Load::Ready(artists) = &self.followed_artists else {
            return;
        };
        let held = artists.iter().any(|held| held.uri == artist);
        match (followed, held) {
            (false, true) => {
                if let Load::Ready(artists) = &mut self.followed_artists {
                    artists.retain(|held| held.uri != artist);
                }
            }
            // A row the list does not hold needs its name and portrait, which
            // only the read brings.
            (true, false) => self.followed_artists = Load::Idle,
            _ => {}
        }
    }

    /// Lowers the sidebar's declared count for `playlist` by `gone` rows.
    fn lower_page_count(&mut self, playlist: &str, gone: usize) {
        if gone == 0 {
            return;
        }
        if let Load::Ready(playlists) = &mut self.playlists
            && let Some(entry) = playlists.iter_mut().find(|entry| entry.uri == playlist)
        {
            entry.tracks = entry.tracks.map(|count| count.saturating_sub(gone));
        }
    }

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
            Page::Playlist(_) | Page::Album(_) | Page::Artist(_) | Page::Show(_) => None,
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
            Page::Album(_) | Page::Artist(_) | Page::Show(_) | Page::Search(_) => None,
        }
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

    /// A page change ends the like sentence: it names the page the click was
    /// made on.
    fn left_page(&mut self) {
        self.likes.clear_notice();
        self.track_filter.clear();
        if self.renaming.surface() == Surface::Band {
            self.renaming.dismiss();
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

    /// Whether a write for this playlist is in flight.
    fn writing(&self, playlist: &str) -> bool {
        self.pending
            .as_ref()
            .is_some_and(|pending| pending.playlist == playlist)
    }

    /// Whether any write is in flight, on the page or in the sidebar.
    ///
    /// A menu opened over a write would act on rows that write is already
    /// replacing, so it waits for the reply.
    pub fn write_in_flight(&self) -> bool {
        self.pending.is_some() || self.sidebar_pending.is_some()
    }

    /// Whether a row may be lifted off the loaded page now.
    ///
    /// A page write in flight holds the page still: a drop would name a revision
    /// the write is already replacing. A sidebar write does not stop a lift,
    /// because the queue a page row can land in names no revision; the two
    /// writes' own methods refuse instead.
    ///
    /// A page the user cannot write still lifts, because a drop into the queue
    /// is not a write to that page.
    pub fn liftable(&self) -> bool {
        self.pending.is_none()
    }

    /// Whether the loaded track page may reorder its rows now.
    ///
    /// It needs a read that named an editable list, and no write in flight: a
    /// second one would name a revision the first is already replacing.
    pub fn movable(&self) -> bool {
        self.liftable()
            && self.sidebar_pending.is_none()
            && matches!(
                &self.tracks,
                Load::Ready(tracks)
                    if tracks.access.as_ref().is_some_and(|access| access.editable)
            )
    }

    /// Whether the loaded track page may take rows out now.
    ///
    /// A removal names the same revision a move does, so it waits for the same
    /// things.
    pub fn removable(&self) -> bool {
        self.movable()
    }

    /// Whether the sidebar's playlists may be reordered now.
    ///
    /// It needs a read that kept every item, read at a known revision, and no
    /// write in flight: a second write would name a revision the first is
    /// already replacing.
    pub fn rootlist_movable(&self) -> bool {
        self.liftable()
            && self.sidebar_pending.is_none()
            && matches!(self.playlists, Load::Ready(_))
            && self.rootlist.as_ref().is_some_and(|access| access.editable)
    }

    /// Whether a playlist row may be unlinked now.
    ///
    /// The unlink names the row's own wire index, so it needs what a move needs.
    pub fn rootlist_removable(&self) -> bool {
        self.rootlist_movable()
    }

    /// Whether a sidebar row of `kind` may be removed now.
    ///
    /// The library's state decides, not the row's: a playlist needs the editable
    /// rootlist its index names, a saved thing needs a ready list, and neither
    /// waits on a write already in flight.
    pub fn can_remove(&self, kind: SidebarKind) -> bool {
        if self.write_in_flight() {
            return false;
        }
        match kind {
            SidebarKind::Playlists => self.rootlist_removable(),
            SidebarKind::Albums => matches!(self.saved_albums, Load::Ready(_)),
            SidebarKind::Artists => matches!(self.followed_artists, Load::Ready(_)),
            SidebarKind::Podcasts => matches!(self.followed, Load::Ready(_)),
        }
    }

    /// Removes `item` from the library, and asks for the one write that does it.
    ///
    /// Writes a library item in, the mirror of [`Self::remove_from_library`].
    /// Applies the direction a heart asked for.
    ///
    /// A heart carries the value it was pressed into, so the library is told
    /// rather than left to infer it from a state that may have moved since the
    /// frame was drawn.
    pub fn set_saved(&mut self, item: &LibraryItem, saved: bool) -> Option<Command> {
        if saved {
            self.save_to_library(item)
        } else {
            self.remove_from_library(item)
        }
    }

    pub fn save_to_library(&mut self, item: &LibraryItem) -> Option<Command> {
        if self.is_saved(&item.uri) == Some(true) {
            return None;
        }
        let command = match item.kind {
            SidebarKind::Albums => {
                if !matches!(self.saved_albums, Load::Ready(_)) {
                    return None;
                }
                Command::SetSaved {
                    uri: item.uri.clone(),
                    saved: true,
                }
            }
            SidebarKind::Podcasts => {
                if !matches!(self.followed, Load::Ready(_)) {
                    return None;
                }
                Command::SetSaved {
                    uri: item.uri.clone(),
                    saved: true,
                }
            }
            SidebarKind::Artists => {
                // A page's own follow write is already in flight; a second one
                // for the same artist would race it.
                if self.follow.artist() == Some(item.uri.as_str()) && self.follow.pending() {
                    return None;
                }
                Command::SetFollow {
                    artist: item.uri.clone(),
                    followed: true,
                }
            }
            SidebarKind::Playlists => {
                let revision = self.rootlist.as_ref()?.revision.clone();
                Command::LinkPlaylist {
                    playlist: item.uri.clone(),
                    revision,
                }
            }
        };
        self.sidebar_pending = match item.kind {
            SidebarKind::Playlists => Some(SidebarPending::Link {
                playlist: item.uri.clone(),
            }),
            SidebarKind::Artists => None,
            SidebarKind::Albums | SidebarKind::Podcasts => Some(SidebarPending::Saved {
                uri: item.uri.clone(),
                saved: true,
            }),
        };
        self.sidebar_notice = None;
        Some(command)
    }

    /// Nothing leaves the sidebar here: the row goes when the reply confirms,
    /// because a removal that never landed has to come back.
    pub fn remove_from_library(&mut self, item: &LibraryItem) -> Option<Command> {
        if !self.can_remove(item.kind) {
            return None;
        }
        let index = match item.kind {
            SidebarKind::Playlists => {
                let Load::Ready(playlists) = &self.playlists else {
                    return None;
                };
                Some(playlists.iter().position(|entry| entry.uri == item.uri)?)
            }
            SidebarKind::Albums | SidebarKind::Artists | SidebarKind::Podcasts => None,
        };
        let command = match item.kind {
            SidebarKind::Playlists => Command::RemovePlaylist {
                playlist: item.uri.clone(),
                revision: self.rootlist.as_ref()?.revision.clone(),
                index: index?,
            },
            SidebarKind::Albums => {
                if !listed(&self.saved_albums, &item.uri, |album| &album.uri) {
                    return None;
                }
                Command::SetSaved {
                    uri: item.uri.clone(),
                    saved: false,
                }
            }
            SidebarKind::Podcasts => {
                if !listed(&self.followed, &item.uri, |show| &show.uri) {
                    return None;
                }
                Command::SetSaved {
                    uri: item.uri.clone(),
                    saved: false,
                }
            }
            SidebarKind::Artists => {
                // The list answers for a row it draws; a band's menu is about the
                // page's own artist, which the page's follow state knows first.
                if self.is_saved(&item.uri) != Some(true) {
                    return None;
                }
                // A page's own follow write is already in flight; a second one
                // for the same artist would race it.
                if self.follow.artist() == Some(item.uri.as_str()) && self.follow.pending() {
                    return None;
                }
                Command::SetFollow {
                    artist: item.uri.clone(),
                    followed: false,
                }
            }
        };
        self.sidebar_pending = match item.kind {
            SidebarKind::Playlists => Some(SidebarPending::Remove {
                playlist: item.uri.clone(),
                index: index?,
            }),
            SidebarKind::Albums | SidebarKind::Artists | SidebarKind::Podcasts => {
                Some(SidebarPending::Saved {
                    uri: item.uri.clone(),
                    saved: false,
                })
            }
        };
        self.sidebar_notice = None;
        Some(command)
    }

    /// Closes a row's rename field because the sidebar cannot draw its rows.
    ///
    /// The field lives in a row of the column; below the rail width the same
    /// sidebar is only drawn by the drawer, and a closed drawer leaves the field
    /// nowhere to draw.
    pub fn dismiss_row_rename(&mut self) {
        if self.renaming.surface() == Surface::Row {
            self.renaming.dismiss();
        }
    }

    /// Applies a rename field the user closed this frame.
    ///
    /// Escape, a blur and a chip change all close the field, and the row or band
    /// it was drawn in takes the keyboard back. The sentence a failed write left
    /// named that field, so it goes with it: nothing on screen is left asking
    /// for the retry it described.
    pub fn settle_rename(&mut self) {
        if !self.renaming.take_left() {
            return;
        }
        self.sidebar_notice = None;
        if let Load::Ready(tracks) = &mut self.tracks
            && tracks.notice == Some(Failure::Rename)
        {
            tracks.notice = None;
        }
    }

    /// Presses a sidebar chip, and clears the sentence a failed write left.
    pub fn choose_kind(&mut self, kind: SidebarKind) -> bool {
        let moved = self.sidebar_kind.chosen() != kind;
        let chose = self.sidebar_kind.choose(kind);
        if chose && moved {
            self.sidebar_notice = None;
            if self.renaming.surface() == Surface::Row {
                self.renaming.dismiss();
            }
        }
        chose
    }

    /// Whether `row` of the sidebar may take a track now.
    ///
    /// The open playlist is not a target: its rows are on screen and would not
    /// show the new track, and dropping one of its own rows on it would quietly
    /// duplicate it.
    pub fn addable(&self, row: usize) -> bool {
        if !self.liftable() || self.sidebar_pending.is_some() {
            return false;
        }
        let Load::Ready(playlists) = &self.playlists else {
            return false;
        };
        playlists
            .get(row)
            .is_some_and(|entry| entry.add_basis().is_some() && self.selected() != Some(&entry.uri))
    }

    /// Whether a new playlist may be created now.
    ///
    /// It needs a read of the playlists to insert into, a rootlist revision to
    /// link against, and no write in flight: one gesture is one write.
    ///
    /// Deliberately looser than [`Self::rootlist_movable`], which also needs an
    /// editable access: an access is not editable when the walk skipped a folder
    /// marker, and a create names wire index 0 whether or not items were
    /// dropped. A link that fails anyway takes the same path as any other.
    pub fn creatable(&self) -> bool {
        !self.write_in_flight()
            && matches!(self.playlists, Load::Ready(_))
            && self.rootlist.is_some()
    }

    /// Whether `uri` may have its name rewritten now.
    ///
    /// The row's own access says the user may write the list; no rootlist
    /// revision is involved, because a rename names the playlist's own.
    pub fn can_rename(&self, uri: &str) -> bool {
        if self.write_in_flight() {
            return false;
        }
        let Load::Ready(playlists) = &self.playlists else {
            return false;
        };
        playlists
            .iter()
            .find(|entry| entry.uri == uri)
            .is_some_and(|entry| entry.access.as_ref().is_some_and(|access| access.editable))
    }

    /// Opens the rename field on `uri`, prefilled with its current name.
    pub fn begin_rename(&mut self, uri: &str, surface: Surface) -> bool {
        if !self.can_rename(uri) || self.renaming.writing() {
            return false;
        }
        let Load::Ready(playlists) = &self.playlists else {
            return false;
        };
        let Some(entry) = playlists.iter().find(|entry| entry.uri == uri) else {
            return false;
        };
        self.renaming.open(uri, &entry.name, surface);
        true
    }

    /// Starts a rename for the name in the field, and asks for one write.
    ///
    /// A name the server already holds closes the field and sends nothing, so
    /// the wire never sees a no-op write. Only a row the listing calls named is
    /// the server's own: an unnamed row shows the name a create typed and its
    /// rename never reached, so sending the text it shows is the first write of
    /// that name rather than a no-op. A refusal changes nothing, so a held Enter
    /// sends once.
    pub fn rename_playlist(&mut self) -> Option<Command> {
        if !self.renaming.is_open() || self.renaming.writing() {
            return None;
        }
        let uri = self.renaming.uri().to_owned();
        if !self.can_rename(&uri) {
            return None;
        }
        let Load::Ready(playlists) = &self.playlists else {
            return None;
        };
        let (current, named) = playlists
            .iter()
            .find(|entry| entry.uri == uri)
            .map(|entry| (entry.name.clone(), entry.named))?;
        let name = self.renaming.begin_write()?;
        if named && name == current {
            self.renaming.finished();
            return None;
        }
        self.sidebar_pending = Some(SidebarPending::Rename {
            playlist: uri.clone(),
        });
        self.sidebar_notice = None;
        Some(Command::RenamePlaylist {
            playlist: uri,
            name,
        })
    }

    /// Starts a create for the name in the head's field, and asks for one write.
    ///
    /// The command names the trimmed name and the rootlist revision held now,
    /// and nothing is drawn until the reply: the URI exists only after the
    /// create answers. A refusal changes nothing, so a held Enter sends once.
    pub fn create_playlist(&mut self) -> Option<Command> {
        if !self.creatable() {
            return None;
        }
        let revision = self.rootlist.as_ref()?.revision.clone();
        let name = self.naming.begin_write()?;
        let tracks = self.naming.pick().to_vec();
        self.sidebar_pending = Some(SidebarPending::Create);
        self.sidebar_notice = None;
        Some(Command::CreatePlaylist {
            name,
            revision,
            tracks,
        })
    }

    /// Moves a dragged playlist row to the gap the pointer is over.
    ///
    /// The order moves at once and one write goes out on the rootlist's
    /// revision. `boundary` is the gap in the pre-move list, which is exactly
    /// the index the wire reads, so nothing is subtracted here.
    pub fn move_playlist(
        &mut self,
        from: usize,
        boundary: usize,
        carried: &str,
    ) -> Option<Command> {
        if !self.rootlist_movable() {
            return None;
        }
        let Load::Ready(playlists) = &self.playlists else {
            return None;
        };
        if playlists.get(from).is_none_or(|entry| entry.uri != carried) {
            return None;
        }
        let step = reorder::step(playlists.len(), from, boundary)?;
        let before = playlists.clone();
        let mut after = before.clone();
        reorder::apply(&mut after, step);
        let revision = self.rootlist.as_ref()?.revision.clone();
        self.sidebar_pending = Some(SidebarPending::Reorder { before });
        self.playlists = Load::Ready(after);
        self.sidebar_notice = None;
        Some(Command::MovePlaylist {
            revision,
            from: step.from,
            to: step.to,
        })
    }

    /// The playlists the add submenu may offer, in the sidebar's own order.
    ///
    /// Only the lists that can take a track, and never the page on screen. A
    /// write in flight empties it: one gesture is one write.
    pub fn add_targets(&self) -> Vec<AddTarget> {
        if self.write_in_flight() {
            return Vec::new();
        }
        let Load::Ready(playlists) = &self.playlists else {
            return Vec::new();
        };
        playlists
            .iter()
            .filter(|entry| self.selected() != Some(entry.uri.as_str()))
            .filter_map(|entry| {
                let (revision, length) = entry.add_basis()?;
                Some(AddTarget {
                    uri: entry.uri.clone(),
                    name: entry.name.clone(),
                    revision: revision.clone(),
                    length,
                })
            })
            .collect()
    }

    /// Adds `tracks` to the playlist `uri` names, when the sidebar holds it.
    pub fn add_pick(&mut self, uri: &str, tracks: &[String]) -> Option<Command> {
        let Load::Ready(playlists) = &self.playlists else {
            return None;
        };
        let row = playlists.iter().position(|entry| entry.uri == uri)?;
        self.add_tracks(row, tracks)
    }

    /// Appends `tracks` to `row`, and asks for one write.
    ///
    /// Nothing moves on screen: the count is confirmed by the reply, and a
    /// failure leaves it alone.
    pub fn add_tracks(&mut self, row: usize, tracks: &[String]) -> Option<Command> {
        self.begin_add(row, tracks)
    }

    /// Holds the row for one add of `tracks` and names the write it asks for.
    fn begin_add(&mut self, row: usize, tracks: &[String]) -> Option<Command> {
        if tracks.is_empty() || !self.addable(row) {
            return None;
        }
        let Load::Ready(playlists) = &self.playlists else {
            return None;
        };
        let entry = playlists.get(row)?;
        let (revision, from_index) = entry.add_basis()?;
        let command = Command::AddTracks {
            playlist: entry.uri.clone(),
            revision: revision.clone(),
            from_index,
            tracks: tracks.to_vec(),
        };
        self.sidebar_pending = Some(SidebarPending::Add {
            playlist: entry.uri.clone(),
            count: tracks.len(),
        });
        self.sidebar_notice = None;
        Some(command)
    }

    /// Moves the dragged rows to the gap the pointer is over, and asks for one
    /// write.
    ///
    /// It refuses unless that playlist is the page on screen, the page is
    /// editable, every row is still the one the drag carried, and no write is in
    /// flight: one gesture is one write. The new order is on screen at once and
    /// kept until the reply lands; the returned range is where the block went. A
    /// write that fails puts the rows back and the page says so — trying again
    /// means dragging again, not a second write for the same gesture.
    pub fn move_tracks(
        &mut self,
        playlist: &str,
        rows: &[usize],
        boundary: usize,
        carried: &[TrackEntry],
    ) -> Option<(Command, Range<usize>)> {
        let access = self.page_write(playlist, rows, carried)?;
        let Load::Ready(tracks) = &self.tracks else {
            return None;
        };
        let found = reorder::block(&tracks.tracks, rows, boundary)?;
        self.pending = Some(reorder::Pending {
            playlist: playlist.to_owned(),
            before: tracks.tracks.clone(),
            after: found.order.clone(),
        });
        self.tracks = Load::Ready(TracksFor {
            playlist: playlist.to_owned(),
            tracks: found.order,
            access: Some(access.clone()),
            notice: None,
        });
        Some((
            Command::MoveTrack {
                playlist: playlist.to_owned(),
                revision: access.revision,
                moves: found.moves,
            },
            found.landed..found.landed + rows.len(),
        ))
    }

    /// Takes the picked rows out of the page, and asks for one write.
    ///
    /// It runs the same guards a move does, because both name a revision and
    /// only one write may be in flight. The rows are gone on screen at once and
    /// kept until the reply lands; a write that fails puts them back and the
    /// page says so. Nothing is re-requested, so a removal can never leave a
    /// ready page waiting.
    pub fn remove_tracks(
        &mut self,
        playlist: &str,
        rows: &[usize],
        carried: &[TrackEntry],
    ) -> Option<Command> {
        let access = self.page_write(playlist, rows, carried)?;
        let Load::Ready(tracks) = &self.tracks else {
            return None;
        };
        let found = removal::plan(&tracks.tracks, rows)?;
        self.pending = Some(reorder::Pending {
            playlist: playlist.to_owned(),
            before: tracks.tracks.clone(),
            after: found.kept.clone(),
        });
        self.tracks = Load::Ready(TracksFor {
            playlist: playlist.to_owned(),
            tracks: found.kept,
            access: Some(access.clone()),
            notice: None,
        });
        Some(Command::RemoveTracks {
            playlist: playlist.to_owned(),
            revision: access.revision,
            removals: found.removals,
        })
    }

    /// What a page write may name now, when `rows` still hold `carried`.
    ///
    /// One write at a time, on the playlist on screen, with rows that are
    /// ascending and unchanged since the gesture picked them.
    fn page_write(
        &self,
        playlist: &str,
        rows: &[usize],
        carried: &[TrackEntry],
    ) -> Option<PlaylistAccess> {
        let Load::Ready(tracks) = &self.tracks else {
            return None;
        };
        if !self.liftable()
            || self.sidebar_pending.is_some()
            || self.selected() != Some(playlist)
            || tracks.playlist != playlist
        {
            return None;
        }
        let access = tracks.access.clone().filter(|access| access.editable)?;
        let ascending = !rows.is_empty() && rows.windows(2).all(|pair| pair[0] < pair[1]);
        let held = rows.len() == carried.len()
            && rows
                .iter()
                .zip(carried)
                .all(|(row, entry)| tracks.tracks.get(*row) == Some(entry));
        if !ascending || !held {
            return None;
        }
        Some(access)
    }

    /// Marks a playlist as wanted and returns the command that loads it.
    ///
    /// It never touches the history: Back and Forward move the page first and
    /// only reload what they land on.
    fn load(&mut self, playlist: &str) -> Command {
        self.wanted = Some(playlist.to_owned());
        self.tracks = Load::Loading;
        // A navigation to a playlist whose cover is due doubles as its ask: the
        // page read answers the cover too, so no second read is issued.
        self.cover_refresh.asked(playlist);
        Command::LoadTracks {
            playlist: playlist.to_owned(),
        }
    }

    /// Marks an album as wanted and returns the command that loads it.
    ///
    /// Like [`Self::load`] it never touches the history: Back and Forward move
    /// the page first and only reload what they land on.
    fn load_album(&mut self, album: &str) -> Command {
        self.wanted_album = Some(album.to_owned());
        self.album = Load::Loading;
        Command::LoadAlbum {
            album: album.to_owned(),
        }
    }

    /// Marks a show as wanted and returns the command that loads it.
    ///
    /// Like [`Self::load`] and [`Self::load_album`] it never touches the
    /// history: Back and Forward move the page first and only reload what they
    /// land on.
    fn load_show(&mut self, show: &str) -> Command {
        self.wanted_show = Some(show.to_owned());
        self.show = Load::Loading;
        Command::LoadShow {
            show: show.to_owned(),
        }
    }

    /// Points the artist page at `artist` and returns the command that loads it.
    ///
    /// Like [`Self::load`] it never touches the history. It also opens the
    /// Follow state and the discography filter at this artist, so a label
    /// confirmed for another one cannot be drawn over this page and another
    /// artist's filter choice is not carried into this one.
    fn load_artist(&mut self, artist: &str) -> Command {
        if self.wanted_artist.as_deref() != Some(artist) {
            // Another artist's likeness list is no use here.
            self.related = Load::Idle;
            self.wanted_related = None;
        }
        self.wanted_artist = Some(artist.to_owned());
        self.artist = Load::Loading;
        self.follow.open(artist);
        self.filter.open(artist);
        Command::LoadArtist {
            artist: artist.to_owned(),
        }
    }

    /// Returns the command the current page needs, if any.
    ///
    /// A page whose wanted load is in flight or ready is left alone; any other
    /// page of a loadable kind is reloaded through its own loader. A failed load
    /// therefore reloads, which is what makes the Retry control and a Back onto a
    /// failed page the same request. A page never matches another one's want, so
    /// Back onto one whose load was superseded asks again rather than showing
    /// nothing.
    fn settle(&mut self) -> Option<Command> {
        match self.history.current() {
            Some(Page::Playlist(playlist)) => {
                let playlist = playlist.clone();
                if self.wanted.as_deref() == Some(playlist.as_str())
                    && matches!(self.tracks, Load::Loading | Load::Ready(_))
                {
                    return None;
                }
                Some(self.load(&playlist))
            }
            Some(Page::Album(album)) => {
                let album = album.clone();
                if self.wanted_album.as_deref() == Some(album.as_str())
                    && matches!(self.album, Load::Loading | Load::Ready(_))
                {
                    return None;
                }
                Some(self.load_album(&album))
            }
            Some(Page::Artist(artist)) => {
                let artist = artist.clone();
                if self.wanted_artist.as_deref() == Some(artist.as_str())
                    && matches!(self.artist, Load::Loading | Load::Ready(_))
                {
                    return None;
                }
                Some(self.load_artist(&artist))
            }
            Some(Page::Show(show)) => {
                let show = show.clone();
                if self.wanted_show.as_deref() == Some(show.as_str())
                    && matches!(self.show, Load::Loading | Load::Ready(_))
                {
                    return None;
                }
                Some(self.load_show(&show))
            }
            // A search page is never fetched by `settle`: its read belongs to
            // the search state, and the shell asks for it from `logic` because
            // only the query, not the page, is what that read is about. A held
            // answer is what Back and Forward land on, so it is kept as it
            // stands; a dropped, idle or failed one is pointed at the page's
            // query again, which is what makes Back ask once. The page itself
            // never carries a load, so no command comes back from here.
            Some(Page::Search(query)) => {
                let query = query.clone();
                if matches!(
                    self.search.page_answer(&query),
                    Some(Load::Loading | Load::Ready(_))
                ) {
                    return None;
                }
                self.search.open_results(&query);
                None
            }
            _ => None,
        }
    }

    /// Marks the playlists loading again and returns the command that fetches
    /// them.
    ///
    /// The followed lists restart with them: a playlist retry is the root's
    /// Retry, so every half of the page asks again. A reply already in flight
    /// from before the retry still lands, and its list is valid — only a little
    /// older than the retry's.
    pub fn reload(&mut self) -> Command {
        self.playlists = Load::Loading;
        self.followed = Load::Idle;
        self.followed_artists = Load::Idle;
        self.saved_albums = Load::Idle;
        self.likes.reload();
        Command::ReloadLibrary
    }

    /// The library read a failed link asks for, if one is due.
    ///
    /// Want-driven like the followed lists: the update carries no command, so it
    /// is asked for after the drain, when the failed write is no longer pending
    /// and the read can therefore be adopted.
    pub fn rootlist_request(&mut self) -> Option<Command> {
        if self.write_in_flight() || !std::mem::take(&mut self.rootlist_stale) {
            return None;
        }
        Some(self.reload())
    }

    /// The command that reads a playlist's cover again, if one is due.
    ///
    /// A page whose playlist has no rows loaded is asked for itself: a write can
    /// leave the open page blank, and only the page's own read would ever fill
    /// it. That ask is the read the page needs anyway, so it goes through
    /// [`Self::load`] and doubles as the cover ask.
    ///
    /// A read for any other due playlist is a bare `LoadTracks`: its reply
    /// writes that playlist's cover only, because the rows are written for the
    /// wanted playlist alone. It therefore leaves the want and the page on
    /// screen exactly as they are. The page on screen is kept out of the asks
    /// only while its own read is in flight, because that reply answers its
    /// cover; a settled page has no reply coming, so its dropped cover must be
    /// asked for like any other.
    pub fn tracks_request(&mut self) -> Option<Command> {
        if let Some(Page::Playlist(playlist)) = self.history.current() {
            let playlist = playlist.clone();
            if listed(&self.playlists, &playlist, |entry| &entry.uri)
                && matches!(self.tracks, Load::Idle)
                && self.wanted.as_deref() != Some(playlist.as_str())
            {
                return Some(self.load(&playlist));
            }
        }
        let present: Vec<String> = match &self.playlists {
            Load::Ready(playlists) => playlists.iter().map(|entry| entry.uri.clone()).collect(),
            Load::Idle | Load::Loading | Load::Failed(_) => Vec::new(),
        };
        let keep = match (self.selected(), &self.tracks) {
            (Some(page), Load::Loading) if self.wanted.as_deref() == Some(page) => {
                Some(page.to_owned())
            }
            _ => None,
        };
        let playlist = self.cover_refresh.take_due(&present, keep.as_deref())?;
        Some(Command::LoadTracks { playlist })
    }

    /// The command that starts the likeness load, if one is due.
    ///
    /// The cards are the page's second read: they are asked for only once the
    /// artist's own detail has landed and named somebody, so the hero and the
    /// discography never wait on a dozen `Artist::get` calls. A page that names
    /// nobody asks for nothing.
    pub fn related_request(&mut self) -> Option<Command> {
        let Load::Ready(detail) = &self.artist else {
            return None;
        };
        let artist = detail.uri.clone();
        let uris = detail.related.clone();
        if uris.is_empty() {
            return None;
        }
        if matches!(self.related, Load::Loading)
            || (matches!(self.related, Load::Ready(_) | Load::Failed(_))
                && self.wanted_related.as_deref() == Some(artist.as_str()))
        {
            return None;
        }
        self.wanted_related = Some(artist.clone());
        self.related = Load::Loading;
        Some(Command::LoadRelated { artist, uris })
    }

    /// Marks the likeness list loading again, for the page's own Retry.
    pub fn reload_related(&mut self) -> Option<Command> {
        self.related = Load::Idle;
        self.related_request()
    }

    /// Marks the results page's read idle again and returns the command that
    /// fetches it, for the page's own Retry.
    ///
    /// The page is the only place that knows a Retry was pressed, and the read
    /// is want-driven, so this resets the read and asks for it in the same call
    /// rather than waiting a frame for `logic`'s next look. It asks for the
    /// page's own query even while the menu is open and due, which is what
    /// [`crate::search::Search::request_page`] is for.
    pub fn reload_search(&mut self) -> Option<Command> {
        let query = self.page_search_query().map(str::to_owned)?;
        self.search.retry_page(&query);
        self.search.request_page(Some(&query))
    }

    /// Marks the followed-shows list loading again and returns the command that
    /// fetches it.
    ///
    /// This is the list's own Retry, so it asks directly rather than going
    /// through the playlists.
    pub fn reload_followed(&mut self) -> Command {
        self.followed = Load::Loading;
        Command::LoadFollowedShows
    }

    /// The command that starts the followed-shows load, if one is due.
    ///
    /// The list is want-driven rather than chained onto the rootlist, so the
    /// worker's update stream is unchanged. It asks exactly when nothing has
    /// been asked for yet and the playlists can name a signed-in window; a
    /// window that never loads its playlists therefore never asks for shows.
    pub fn followed_request(&mut self) -> Option<Command> {
        if self.followed == Load::Idle && matches!(self.playlists, Load::Ready(_)) {
            self.followed = Load::Loading;
            Some(Command::LoadFollowedShows)
        } else {
            None
        }
    }

    /// Marks the followed-artists list loading again and returns the command
    /// that fetches it.
    ///
    /// This is the list's own Retry, so it asks directly rather than going
    /// through the playlists. A reply already in flight from before the retry
    /// still lands, and its list is valid — only a little older than the
    /// retry's.
    pub fn reload_followed_artists(&mut self) -> Command {
        self.followed_artists = Load::Loading;
        Command::LoadFollowedArtists
    }

    /// Marks the saved-albums list loading again and returns the command that
    /// fetches it.
    ///
    /// The followed-artists shape, one list over.
    pub fn reload_saved_albums(&mut self) -> Command {
        self.saved_albums = Load::Loading;
        Command::LoadSavedAlbums
    }

    /// The command that starts the followed-artists load, if one is due.
    ///
    /// Like the followed shows it is want-driven: it asks exactly when nothing
    /// has been asked for yet, the playlists can name a signed-in window, and a
    /// chip naming artists is the one the user is looking at.
    pub fn followed_artists_request(&mut self) -> Option<Command> {
        if self.followed_artists == Load::Idle
            && matches!(self.playlists, Load::Ready(_))
            && artists_chosen(self.root_filter.chosen(), self.sidebar_kind.chosen())
        {
            self.followed_artists = Load::Loading;
            Some(Command::LoadFollowedArtists)
        } else {
            None
        }
    }

    /// The command that starts the saved-albums load, if one is due.
    ///
    /// The followed-artists shape, one chip over.
    pub fn saved_albums_request(&mut self) -> Option<Command> {
        if self.saved_albums == Load::Idle
            && matches!(self.playlists, Load::Ready(_))
            && (albums_chosen(self.root_filter.chosen(), self.sidebar_kind.chosen())
                || self.on_artist_page())
        {
            self.saved_albums = Load::Loading;
            Some(Command::LoadSavedAlbums)
        } else {
            None
        }
    }

    /// Whether the page on screen draws a heart on releases it did not read.
    ///
    /// An artist page draws one per release, and only the saved-albums set can
    /// say what each one is.
    fn on_artist_page(&self) -> bool {
        matches!(self.page(), Some(crate::route::Page::Artist(_)))
    }

    /// The command that starts the liked-songs read, if one is due.
    ///
    /// It waits for the playlists, because a signed-out window has no set to
    /// read and no heart to draw; every later sweep is the set's own to ask for.
    pub fn likes_request(&mut self) -> Option<Command> {
        self.likes
            .request(matches!(self.playlists, Load::Ready(_)))
            .then_some(Command::LoadLikedTracks)
    }

    /// Flips the liked set and asks for the write, or changes nothing.
    ///
    /// The set mutates only when it has a change to send, so a refused or empty
    /// flip never leaves a write pending with nothing on the wire.
    pub fn set_liked(&mut self, tracks: Vec<String>, liked: bool) -> Option<Command> {
        let change = self.likes.set(&tracks, liked)?;
        Some(Command::SetLiked {
            tracks: change.tracks,
            liked: change.liked,
        })
    }

    /// The album the album page draws, once it is loaded and named by that page.
    ///
    /// The URI check is not decoration: a ready detail from an earlier page must
    /// not name a crumb for a page that is still loading something else.
    pub fn current_album(&self) -> Option<&AlbumDetail> {
        let Page::Album(uri) = self.history.current()? else {
            return None;
        };
        match &self.album {
            Load::Ready(detail) if detail.uri == *uri => Some(detail),
            Load::Idle | Load::Loading | Load::Ready(_) | Load::Failed(_) => None,
        }
    }

    /// The artist the artist page draws, once it is loaded and named by that
    /// page.
    ///
    /// The URI check is the album's: a ready detail from an earlier page must
    /// not name a crumb, or fill a hero, for a page loading something else.
    pub fn current_artist(&self) -> Option<&ArtistDetail> {
        let Page::Artist(uri) = self.history.current()? else {
            return None;
        };
        match &self.artist {
            Load::Ready(detail) if detail.uri == *uri => Some(detail),
            Load::Idle | Load::Loading | Load::Ready(_) | Load::Failed(_) => None,
        }
    }

    /// The show the show page draws, once it is loaded and named by that page.
    ///
    /// The URI check is the album's: a ready detail from an earlier page must
    /// not name a crumb, or fill a band, for a page loading something else.
    pub fn current_show(&self) -> Option<&ShowDetail> {
        let Page::Show(uri) = self.history.current()? else {
            return None;
        };
        match &self.show {
            Load::Ready(detail) if detail.show.uri == *uri => Some(detail),
            Load::Idle | Load::Loading | Load::Ready(_) | Load::Failed(_) => None,
        }
    }

    /// The artist page's four state pieces, borrowed for one frame.
    ///
    /// They are handed out together because the draw needs all four and the
    /// borrow checker will not let it take them one call at a time; the page's
    /// own URI is read from [`Self::page`] before this borrow starts.
    pub fn artist_page(&mut self) -> ArtistPage<'_> {
        let Self {
            artist,
            follow,
            filter,
            releases,
            related,
            likes,
            history,
            ..
        } = self;
        let current = match history.current() {
            Some(Page::Artist(uri)) => match &*artist {
                Load::Ready(detail) if detail.uri == *uri => Some(detail),
                Load::Idle | Load::Loading | Load::Ready(_) | Load::Failed(_) => None,
            },
            _ => None,
        };
        ArtistPage {
            load: artist,
            current,
            follow,
            filter,
            releases,
            related,
            likes,
        }
    }

    /// The selected playlist's name, once the list that names it is ready.
    pub fn selected_name(&self) -> Option<&str> {
        self.selected_entry().map(|entry| entry.name.as_str())
    }

    /// The selected playlist's entry, once the list that holds it is ready.
    pub fn selected_entry(&self) -> Option<&PlaylistEntry> {
        let selected = self.selected()?;
        match &self.playlists {
            Load::Ready(playlists) => playlists.iter().find(|entry| entry.uri == selected),
            Load::Idle | Load::Loading | Load::Failed(_) => None,
        }
    }

    /// How many episodes the show on this page lists, once the followed list
    /// that carries the count is ready.
    ///
    /// The count arrives with the listing rather than with the show's own detail,
    /// so a show page can hold its rows open while the detail is still in flight.
    pub fn selected_show_episodes(&self) -> Option<usize> {
        let Page::Show(uri) = self.history.current()? else {
            return None;
        };
        match &self.followed {
            Load::Ready(shows) => shows
                .iter()
                .find(|show| show.uri == *uri)
                .and_then(|show| show.episodes),
            Load::Idle | Load::Loading | Load::Failed(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use takofy_model::{
        AddedAt, AlbumDetail, ArtistDetail, ArtistPortrait, CoverUrl, Episode, FollowedShow, Move,
        PlaylistAccess, PlaylistCover, PlaylistEntry, ReleaseKind, Removal, Revision, Show,
        ShowDetail, TrackEntry, TrackLyrics,
    };

    use super::*;
    use crate::discography::Chip;
    #[test]
    fn a_show_page_holds_open_the_episode_count_its_listing_names() {
        let mut library = Library {
            followed: Load::Ready(vec![FollowedShow {
                uri: "spotify:show:a".to_owned(),
                name: "Field Notes".to_owned(),
                named: true,
                publisher: None,
                episodes: Some(12),
                cover: None,
            }]),
            ..Library::default()
        };
        library.open(Page::Show("spotify:show:a".to_owned()));
        assert_eq!(library.selected_show_episodes(), Some(12));

        library.open(Page::Show("spotify:show:b".to_owned()));
        assert_eq!(
            library.selected_show_episodes(),
            None,
            "another show's count is not this page's"
        );

        library.followed = Load::Loading;
        library.open(Page::Show("spotify:show:a".to_owned()));
        assert_eq!(library.selected_show_episodes(), None);
    }

    use crate::library_filter::RootKind;
    use crate::lyrics::Tab;
    use crate::message::TransportUpdate;
    use crate::player::NowPlaying;
    use crate::sidebar_kind::SidebarKind;

    fn playlist(uri: &str, name: &str) -> PlaylistEntry {
        PlaylistEntry {
            uri: uri.to_owned(),
            name: name.to_owned(),
            named: true,
            cover: None,
            description: None,
            owner: None,
            tracks: None,
            access: None,
        }
    }

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

    /// The URIs an add would carry, in the order the rows were picked.
    fn pick(uris: &[&str]) -> Vec<String> {
        uris.iter().map(|uri| (*uri).to_owned()).collect()
    }

    fn dated(uri: &str, wire: &str) -> TrackEntry {
        TrackEntry {
            added_at: AddedAt::from_wire(wire),
            ..track(uri)
        }
    }

    fn tracks_for(playlist: &str, tracks: Vec<TrackEntry>) -> TracksFor {
        TracksFor {
            playlist: playlist.to_owned(),
            tracks,
            access: None,
            notice: None,
        }
    }

    fn revision(byte: u8) -> Revision {
        Revision::from_wire(&[byte, 0x0a]).expect("a revision")
    }

    fn access(byte: u8, editable: bool) -> Option<PlaylistAccess> {
        Some(PlaylistAccess {
            revision: revision(byte),
            editable,
        })
    }

    /// A playlist page showing `uris`, with `access` as what it may change.
    fn page_with(playlist: &str, uris: &[&str], access: Option<PlaylistAccess>) -> Library {
        let mut library = Library::default();
        library.select(playlist);
        library.apply(&Update::Tracks {
            playlist: playlist.to_owned(),
            tracks: uris.iter().map(|uri| track(uri)).collect(),
            cover: None,
            access,
        });
        library
    }

    fn album(uri: &str) -> Page {
        Page::Album(uri.to_owned())
    }

    fn artist_command(uri: &str) -> Command {
        Command::LoadArtist {
            artist: uri.to_owned(),
        }
    }

    fn artist_detail(uri: &str) -> ArtistDetail {
        ArtistDetail {
            uri: uri.to_owned(),
            name: "Vela".to_owned(),
            portrait: None,
            top_tracks: vec![track("top")],
            releases: vec![release("spotify:album:r1")],
            related: Vec::new(),
        }
    }

    fn release(uri: &str) -> takofy_model::Release {
        takofy_model::Release {
            uri: uri.to_owned(),
            name: uri.to_owned(),
            kind: takofy_model::ReleaseKind::Album,
            year: None,
            cover: None,
            tracks: vec![uri.to_owned()],
        }
    }

    fn album_detail(uri: &str) -> AlbumDetail {
        AlbumDetail {
            uri: uri.to_owned(),
            name: "Record".to_owned(),
            artists: "Band".to_owned(),
            year: Some(1997),
            cover: None,
            tracks: vec![track("t")],
        }
    }

    fn artist(uri: &str) -> Page {
        Page::Artist(uri.to_owned())
    }

    fn show(uri: &str) -> Page {
        Page::Show(uri.to_owned())
    }

    fn show_command(uri: &str) -> Command {
        Command::LoadShow {
            show: uri.to_owned(),
        }
    }

    fn episode(uri: &str) -> Episode {
        Episode {
            uri: uri.to_owned(),
            title: uri.to_owned(),
            show: "Lateral".to_owned(),
            show_uri: Some("spotify:show:a".to_owned()),
            description: None,
            published: None,
            duration_ms: None,
            cover: None,
            named: true,
        }
    }

    fn show_detail(uri: &str) -> ShowDetail {
        ShowDetail {
            show: Show {
                uri: uri.to_owned(),
                name: "Show".to_owned(),
                publisher: "Lateral".to_owned(),
                description: String::new(),
                cover: None,
            },
            episodes: vec![episode("spotify:episode:e")],
        }
    }

    fn followed_show(uri: &str) -> FollowedShow {
        FollowedShow {
            uri: uri.to_owned(),
            name: uri.to_owned(),
            named: true,
            publisher: None,
            cover: None,
            episodes: None,
        }
    }

    fn related_artist(uri: &str, related: Vec<String>) -> ArtistDetail {
        ArtistDetail {
            related,
            ..artist_detail(uri)
        }
    }

    fn followed_artist(uri: &str) -> FollowedArtist {
        FollowedArtist {
            uri: uri.to_owned(),
            name: uri.to_owned(),
            named: true,
            portrait: None,
            releases: None,
        }
    }

    fn saved_album(uri: &str) -> SavedAlbum {
        SavedAlbum {
            uri: uri.to_owned(),
            name: uri.to_owned(),
            named: true,
            artists: String::new(),
            year: None,
            cover: None,
            tracks: None,
        }
    }

    fn cover_url(tail: &str) -> CoverUrl {
        CoverUrl::from_wire(&format!("https://i.scdn.co/image/{tail}")).expect("a cover url")
    }

    fn playlist_with_cover(uri: &str, name: &str, cover: CoverUrl) -> PlaylistEntry {
        covered(uri, name, PlaylistCover::derived(cover))
    }

    fn playlist_with_chosen_cover(uri: &str, name: &str, cover: CoverUrl) -> PlaylistEntry {
        covered(uri, name, PlaylistCover::chosen(cover))
    }

    fn covered(uri: &str, name: &str, cover: PlaylistCover) -> PlaylistEntry {
        PlaylistEntry {
            cover: Some(cover),
            ..playlist(uri, name)
        }
    }

    /// The cover the sidebar row for `uri` holds.
    fn entry_cover(library: &Library, uri: &str) -> Option<PlaylistCover> {
        sidebar_entry(library, uri).and_then(|entry| entry.cover)
    }

    /// A library whose open page is `a`, showing `uris`, with `cover` on `a`'s
    /// sidebar row.
    fn covered_page(cover: &CoverUrl, uris: &[&str]) -> Library {
        let mut library = page_with("a", uris, access(0x01, true));
        library.playlists = Load::Ready(vec![with_access(
            playlist_with_cover("a", "A", cover.clone()),
            0x07,
            true,
            Some(uris.len()),
        )]);
        library
    }

    /// Whether the band would treat the open page as settled this frame.
    fn settled(library: &Library) -> bool {
        !matches!(library.tracks, Load::Loading)
    }

    /// A library whose open page is `a` and whose rows were dropped by an add
    /// that landed while `a`'s own read was still in flight.
    ///
    /// The add is begun while another playlist is on screen, because the open
    /// playlist is not an add target.
    fn page_dropped_by_an_add(cover: Option<&CoverUrl>) -> Library {
        let mut library = sidebar_library(&["a", "b"], 0x07, true, Some(1));
        let listed = match cover {
            Some(cover) => playlist_with_cover("a", "A", cover.clone()),
            None => playlist("a", "A"),
        };
        library.playlists = Load::Ready(vec![
            with_access(listed, 0x07, true, Some(1)),
            with_access(playlist("b", "B"), 0x07, true, Some(1)),
        ]);
        library.select("b");
        assert!(
            library.add_tracks(0, &pick(&["spotify:track:x"])).is_some(),
            "arrange: the add must be in flight"
        );
        library.select("a");
        library.apply(&Update::TrackAdded {
            playlist: "a".to_owned(),
            revision: revision(0x08),
        });
        library
    }

    #[test]
    fn selected_is_none_before_a_click() {
        assert_eq!(Library::default().selected(), None);
    }

    #[test]
    fn signed_in_starts_the_playlists_loading() {
        let mut library = Library::default();
        library.apply(&Update::SignedIn);
        assert_eq!(library.playlists, Load::Loading);
    }

    #[test]
    fn signed_in_leaves_ready_playlists_alone() {
        let mut library = Library::default();
        library.apply(&Update::Playlists(vec![playlist("a", "A")]));
        library.apply(&Update::SignedIn);
        assert_eq!(library.playlists, Load::Ready(vec![playlist("a", "A")]));
    }

    #[test]
    fn playlists_become_ready_without_touching_the_tracks() {
        let mut library = Library::default();
        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: vec![track("t")],
            cover: None,
            access: None,
        });
        library.apply(&Update::Playlists(vec![playlist("a", "A")]));

        assert_eq!(library.playlists, Load::Ready(vec![playlist("a", "A")]));
        assert_eq!(library.tracks, Load::Idle);
    }

    #[test]
    fn a_playlist_failure_is_recorded() {
        let mut library = Library::default();
        library.apply(&Update::PlaylistFailed(Failure::Load));
        assert_eq!(library.playlists, Load::Failed(Failure::Load));
    }

    #[test]
    fn select_marks_the_load_and_returns_the_command() {
        let mut library = Library::default();
        let command = library.select("spotify:playlist:a");

        assert_eq!(
            command,
            Command::LoadTracks {
                playlist: "spotify:playlist:a".to_owned()
            }
        );
        assert_eq!(library.tracks, Load::Loading);
        assert_eq!(library.selected(), Some("spotify:playlist:a"));
        assert_eq!(
            library.page(),
            Some(&Page::Playlist("spotify:playlist:a".to_owned()))
        );
        assert_eq!(library.wanted.as_deref(), Some("spotify:playlist:a"));
    }

    #[test]
    fn select_of_the_current_playlist_reloads_without_growing_the_history() {
        let mut library = Library::default();
        library.select("a");

        let command = library.select("a");

        assert_eq!(
            command,
            Command::LoadTracks {
                playlist: "a".to_owned()
            }
        );
        assert_eq!(library.tracks, Load::Loading);
        assert_eq!(library.back(), None);
        assert_eq!(library.page(), None);
        assert!(
            !library.can_back(),
            "re-selecting the current page must add no history entry"
        );
    }

    #[test]
    fn selected_is_none_on_an_album_page() {
        let mut library = Library::default();
        library.apply(&Update::Playlists(vec![playlist("a", "A")]));
        library.select("a");

        library.open(album("album"));

        assert_eq!(library.page(), Some(&album("album")));
        assert_eq!(library.selected(), None, "the sidebar highlight clears");
        assert_eq!(library.selected_name(), None);
        assert_eq!(library.selected_entry(), None);
    }

    #[test]
    fn open_an_album_starts_its_load_and_keeps_the_tracks() {
        let mut library = Library::default();
        library.apply(&Update::Playlists(vec![playlist("a", "A")]));
        library.select("a");
        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: vec![track("t")],
            cover: None,
            access: None,
        });

        assert_eq!(
            library.open(album("spotify:album:rec")),
            Some(Command::LoadAlbum {
                album: "spotify:album:rec".to_owned()
            })
        );
        assert_eq!(library.page(), Some(&album("spotify:album:rec")));
        assert_eq!(library.album, Load::Loading);
        assert_eq!(
            library.tracks,
            Load::Ready(tracks_for("a", vec![track("t")])),
            "the playlist behind the album stays ready for Back"
        );
    }

    /// A lyrics reply is keyed by track, not by page, so it reaches the panel
    /// whatever page is on screen.
    #[test]
    fn a_lyrics_reply_reaches_the_panel_whatever_the_page() {
        let mut library = Library::default();
        library.open(album("spotify:album:a"));
        library.lyrics.select(Tab::Lyrics);
        let now = NowPlaying {
            uri: "spotify:track:a".to_owned(),
            title: "A track".to_owned(),
            artists: String::new(),
            cover: None,
        };
        assert_eq!(
            library.lyrics.request(Some(&now)),
            Some(Command::LoadLyrics {
                track: "spotify:track:a".to_owned(),
            })
        );

        let reply = TrackLyrics::default();
        library.apply(&Update::Lyrics {
            track: "spotify:track:a".to_owned(),
            lyrics: reply.clone(),
        });
        assert_eq!(library.lyrics.load(), &Load::Ready(reply));

        let next = NowPlaying {
            uri: "spotify:track:b".to_owned(),
            title: "Another track".to_owned(),
            artists: String::new(),
            cover: None,
        };
        library.lyrics.sync(Some(&next));
        assert_eq!(
            library.lyrics.request(Some(&next)),
            Some(Command::LoadLyrics {
                track: "spotify:track:b".to_owned(),
            })
        );

        library.apply(&Update::LyricsFailed {
            track: "spotify:track:b".to_owned(),
            failure: Failure::Lyrics,
        });
        assert_eq!(library.lyrics.load(), &Load::Failed(Failure::Lyrics));
    }

    /// The reply path: only the wanted album's answer lands, the first one wins,
    /// and a failure is recorded the same way a playlist's is.
    #[test]
    fn an_album_reply_lands_only_on_its_own_want() {
        let mut library = Library::default();
        library.open(album("spotify:album:a"));
        library.apply(&Update::Album(album_detail("spotify:album:a")));

        assert_eq!(library.album, Load::Ready(album_detail("spotify:album:a")));

        library.apply(&Update::Album(album_detail("spotify:album:b")));
        assert_eq!(
            library.album,
            Load::Ready(album_detail("spotify:album:a")),
            "a reply nobody asked for is discarded"
        );
    }

    #[test]
    fn a_stale_album_reply_is_discarded() {
        let mut library = Library::default();
        library.open(album("spotify:album:a"));
        library.open(album("spotify:album:b"));

        library.apply(&Update::Album(album_detail("spotify:album:a")));

        assert_eq!(library.album, Load::Loading, "A's reply is not B's");
        library.apply(&Update::Album(album_detail("spotify:album:b")));
        assert_eq!(library.album, Load::Ready(album_detail("spotify:album:b")));
    }

    #[test]
    fn a_stale_album_failure_is_discarded() {
        let mut library = Library::default();
        library.open(album("spotify:album:a"));
        library.open(album("spotify:album:b"));

        library.apply(&Update::AlbumFailed {
            album: "spotify:album:a".to_owned(),
            failure: Failure::Load,
        });
        assert_eq!(library.album, Load::Loading, "A's failure is not B's");

        library.apply(&Update::AlbumFailed {
            album: "spotify:album:b".to_owned(),
            failure: Failure::Load,
        });
        assert_eq!(library.album, Load::Failed(Failure::Load));
    }

    #[test]
    fn the_first_album_reply_wins() {
        let mut library = Library::default();
        library.open(album("spotify:album:a"));
        let first = AlbumDetail {
            name: "First".to_owned(),
            ..album_detail("spotify:album:a")
        };
        let second = AlbumDetail {
            name: "Second".to_owned(),
            ..album_detail("spotify:album:a")
        };

        library.apply(&Update::Album(first.clone()));
        library.apply(&Update::Album(second));

        assert_eq!(library.album, Load::Ready(first));
    }

    #[test]
    fn back_onto_a_ready_album_needs_no_command() {
        let mut library = Library::default();
        library.open(album("spotify:album:a"));
        library.apply(&Update::Album(album_detail("spotify:album:a")));
        assert_eq!(
            library.open(artist("spotify:artist:a")),
            Some(artist_command("spotify:artist:a")),
            "the artist page starts its own load"
        );

        assert_eq!(library.back(), None, "a ready album is already settled");
        assert_eq!(library.page(), Some(&album("spotify:album:a")));
    }

    #[test]
    fn back_onto_a_failed_album_reloads_it() {
        let mut library = Library::default();
        library.open(album("spotify:album:a"));
        library.apply(&Update::AlbumFailed {
            album: "spotify:album:a".to_owned(),
            failure: Failure::Load,
        });
        assert_eq!(
            library.open(artist("spotify:artist:a")),
            Some(artist_command("spotify:artist:a"))
        );

        assert_eq!(
            library.back(),
            Some(Command::LoadAlbum {
                album: "spotify:album:a".to_owned()
            }),
            "a failed album has nothing to draw, so Back asks again"
        );
        assert_eq!(library.album, Load::Loading);
    }

    #[test]
    fn reopening_a_failed_album_retries_it() {
        let mut library = Library::default();
        library.open(album("spotify:album:a"));
        library.apply(&Update::AlbumFailed {
            album: "spotify:album:a".to_owned(),
            failure: Failure::Load,
        });

        assert_eq!(
            library.open(album("spotify:album:a")),
            Some(Command::LoadAlbum {
                album: "spotify:album:a".to_owned()
            }),
            "the page's Retry is this call, so it must re-issue the load"
        );
        assert_eq!(library.album, Load::Loading);
    }

    #[test]
    fn to_playlists_clears_the_album_want_and_drops_an_unsettled_load() {
        let mut loading = Library::default();
        loading.open(album("spotify:album:a"));
        loading.to_playlists();
        assert_eq!(loading.album, Load::Idle, "a load nobody wants is dropped");
        assert_eq!(
            loading.open(album("spotify:album:a")),
            Some(Command::LoadAlbum {
                album: "spotify:album:a".to_owned()
            }),
            "the want was cleared, so the album is asked for again"
        );

        let mut failed = Library::default();
        failed.open(album("spotify:album:a"));
        failed.apply(&Update::AlbumFailed {
            album: "spotify:album:a".to_owned(),
            failure: Failure::Load,
        });
        failed.to_playlists();
        assert_eq!(failed.album, Load::Idle);

        let mut ready = Library::default();
        ready.open(album("spotify:album:a"));
        ready.apply(&Update::Album(album_detail("spotify:album:a")));
        ready.to_playlists();
        assert_eq!(
            ready.album,
            Load::Ready(album_detail("spotify:album:a")),
            "a settled album is kept for the page Back returns to"
        );
        assert_eq!(
            ready.open(album("spotify:album:a")),
            Some(Command::LoadAlbum {
                album: "spotify:album:a".to_owned()
            }),
            "the want is cleared even when the album stays ready"
        );
    }

    #[test]
    fn a_playlist_prune_leaves_the_album_alone() {
        let mut library = Library::default();
        library.select("a");
        library.open(album("spotify:album:a"));
        library.apply(&Update::Album(album_detail("spotify:album:a")));

        // The list no longer holds the playlist, so its page is pruned.
        library.apply(&Update::Playlists(vec![playlist("b", "B")]));

        assert_eq!(library.page(), Some(&album("spotify:album:a")));
        assert_eq!(
            library.album,
            Load::Ready(album_detail("spotify:album:a")),
            "a playlist prune never touches the album"
        );
        assert_eq!(
            library.current_album(),
            Some(&album_detail("spotify:album:a"))
        );
    }

    #[test]
    fn current_album_is_none_while_loading() {
        let mut library = Library::default();
        library.open(album("spotify:album:a"));
        assert_eq!(library.current_album(), None);
    }

    #[test]
    fn current_album_is_none_when_a_ready_detail_names_another_page() {
        let mut library = Library::default();
        library.open(album("spotify:album:b"));
        library.album = Load::Ready(album_detail("spotify:album:a"));

        assert_eq!(
            library.current_album(),
            None,
            "A's detail must not name B's page"
        );
    }

    #[test]
    fn current_album_is_some_when_it_names_the_page() {
        let mut library = Library::default();
        library.open(album("spotify:album:a"));
        library.apply(&Update::Album(album_detail("spotify:album:a")));

        assert_eq!(
            library.current_album(),
            Some(&album_detail("spotify:album:a"))
        );
    }

    #[test]
    fn open_a_show_starts_its_load_and_keeps_the_album() {
        let mut library = Library::default();
        library.open(album("spotify:album:a"));
        library.apply(&Update::Album(album_detail("spotify:album:a")));

        assert_eq!(
            library.open(show("spotify:show:s")),
            Some(show_command("spotify:show:s"))
        );
        assert_eq!(library.page(), Some(&show("spotify:show:s")));
        assert_eq!(library.show, Load::Loading);
        assert_eq!(
            library.album,
            Load::Ready(album_detail("spotify:album:a")),
            "the album behind the show stays ready for Back"
        );
    }

    #[test]
    fn a_show_reply_lands_only_on_its_own_want() {
        let mut library = Library::default();
        library.open(show("spotify:show:a"));
        library.apply(&Update::Show(show_detail("spotify:show:a")));

        assert_eq!(library.show, Load::Ready(show_detail("spotify:show:a")));

        library.apply(&Update::Show(show_detail("spotify:show:b")));
        assert_eq!(
            library.show,
            Load::Ready(show_detail("spotify:show:a")),
            "a reply nobody asked for is discarded"
        );
    }

    #[test]
    fn a_stale_show_reply_is_discarded() {
        let mut library = Library::default();
        library.open(show("spotify:show:a"));
        library.open(show("spotify:show:b"));

        library.apply(&Update::Show(show_detail("spotify:show:a")));

        assert_eq!(library.show, Load::Loading, "A's reply is not B's");
        library.apply(&Update::Show(show_detail("spotify:show:b")));
        assert_eq!(library.show, Load::Ready(show_detail("spotify:show:b")));
    }

    #[test]
    fn a_stale_show_failure_is_discarded() {
        let mut library = Library::default();
        library.open(show("spotify:show:a"));
        library.open(show("spotify:show:b"));

        library.apply(&Update::ShowFailed {
            show: "spotify:show:a".to_owned(),
            failure: Failure::Load,
        });
        assert_eq!(library.show, Load::Loading, "A's failure is not B's");

        library.apply(&Update::ShowFailed {
            show: "spotify:show:b".to_owned(),
            failure: Failure::Load,
        });
        assert_eq!(library.show, Load::Failed(Failure::Load));
    }

    #[test]
    fn the_first_show_reply_wins() {
        let mut library = Library::default();
        library.open(show("spotify:show:a"));
        let first = ShowDetail {
            show: Show {
                name: "First".to_owned(),
                ..show_detail("spotify:show:a").show
            },
            ..show_detail("spotify:show:a")
        };
        let second = ShowDetail {
            show: Show {
                name: "Second".to_owned(),
                ..show_detail("spotify:show:a").show
            },
            ..show_detail("spotify:show:a")
        };

        library.apply(&Update::Show(first.clone()));
        library.apply(&Update::Show(second));

        assert_eq!(library.show, Load::Ready(first));
    }

    #[test]
    fn back_onto_a_ready_show_needs_no_command() {
        let mut library = Library::default();
        library.open(show("spotify:show:a"));
        library.apply(&Update::Show(show_detail("spotify:show:a")));
        assert_eq!(
            library.open(album("spotify:album:a")),
            Some(Command::LoadAlbum {
                album: "spotify:album:a".to_owned()
            }),
            "the album page starts its own load"
        );

        assert_eq!(library.back(), None, "a ready show is already settled");
        assert_eq!(library.page(), Some(&show("spotify:show:a")));
    }

    #[test]
    fn back_onto_a_failed_show_reloads_it() {
        let mut library = Library::default();
        library.open(show("spotify:show:a"));
        library.apply(&Update::ShowFailed {
            show: "spotify:show:a".to_owned(),
            failure: Failure::Load,
        });
        assert_eq!(
            library.open(artist("spotify:artist:a")),
            Some(artist_command("spotify:artist:a"))
        );

        assert_eq!(
            library.back(),
            Some(show_command("spotify:show:a")),
            "a failed show has nothing to draw, so Back asks again"
        );
        assert_eq!(library.show, Load::Loading);
    }

    #[test]
    fn reopening_a_failed_show_retries_it() {
        let mut library = Library::default();
        library.open(show("spotify:show:a"));
        library.apply(&Update::ShowFailed {
            show: "spotify:show:a".to_owned(),
            failure: Failure::Load,
        });

        assert_eq!(
            library.open(show("spotify:show:a")),
            Some(show_command("spotify:show:a")),
            "the page's Retry is this call, so it must re-issue the load"
        );
        assert_eq!(library.show, Load::Loading);
    }

    /// Asks the search state for the read this frame wants, the way `app.rs`
    /// does when `logic` runs: the page's query is copied out first.
    fn search_request(library: &mut Library) -> Option<Command> {
        let page = library.page_search_query().map(str::to_owned);
        library.search.request(page.as_deref())
    }

    /// Types `text` into the field, as one frame of the real path would.
    fn type_search(library: &mut Library, text: &str) {
        library.search.text = text.to_owned();
        let input = crate::search::KeyInput {
            typed: true,
            focused: true,
            ..crate::search::KeyInput::default()
        };
        let _ = library.search.step(input);
    }

    fn search(query: &str) -> Page {
        Page::Search(query.to_owned())
    }

    fn search_reply(query: &str, tracks: Vec<TrackEntry>) -> Update {
        Update::Search {
            query: query.to_owned(),
            tracks,
        }
    }

    #[test]
    fn a_search_page_asks_for_its_answer_once_it_is_open() {
        let mut library = Library::default();
        assert_eq!(library.open(search("never gonna")), None);

        assert_eq!(
            search_request(&mut library),
            Some(Command::Search {
                query: "never gonna".to_owned()
            })
        );
        assert_eq!(library.page_search_query(), Some("never gonna"));
    }

    #[test]
    fn back_onto_a_held_search_page_does_not_ask_again() {
        let mut library = Library::default();
        library.open(search("never gonna"));
        library.apply(&search_reply("never gonna", vec![track("t")]));
        assert_eq!(
            library.open(album("spotify:album:a")),
            Some(Command::LoadAlbum {
                album: "spotify:album:a".to_owned()
            })
        );

        assert_eq!(library.back(), None, "the held answer is already settled");
        assert_eq!(library.page_search_query(), Some("never gonna"));
        assert_eq!(
            search_request(&mut library),
            None,
            "the page keeps the answer, so Back asks for nothing"
        );
    }

    #[test]
    fn back_onto_a_dropped_search_page_asks_again() {
        let mut library = Library::default();
        library.open(search("never gonna"));
        library.apply(&search_reply("never gonna", vec![track("t")]));
        library.open(album("spotify:album:a"));
        // A second query replaces the first, so the page no longer holds it.
        library.open(search("give you up"));

        // Back lands on the album; the second Back lands on the dropped search.
        assert_eq!(library.back(), None);
        assert_eq!(library.page(), Some(&album("spotify:album:a")));
        assert_eq!(library.back(), None);
        assert_eq!(library.page_search_query(), Some("never gonna"));
        assert_eq!(
            search_request(&mut library),
            Some(Command::Search {
                query: "never gonna".to_owned()
            }),
            "the dropped answer is asked for again"
        );
    }

    /// A failed page has nothing to draw, so Back onto it must ask again — the
    /// same request the page's Retry makes.
    #[test]
    fn back_onto_a_failed_search_page_asks_again() {
        let mut library = Library::default();
        library.open(search("never gonna"));
        library.apply(&Update::SearchFailed {
            query: "never gonna".to_owned(),
            failure: Failure::Search,
        });
        library.open(album("spotify:album:a"));

        assert_eq!(library.back(), None, "a search page never settles");
        assert_eq!(library.page_search_query(), Some("never gonna"));
        assert_eq!(
            search_request(&mut library),
            Some(Command::Search {
                query: "never gonna".to_owned()
            }),
            "a failed page is asked for again"
        );
        assert_eq!(
            library.search.page_answer("never gonna"),
            Some(&Load::Loading),
            "and the failure is no longer what the page draws"
        );
    }

    #[test]
    fn back_forward_between_search_pages_keeps_each_answer() {
        let mut library = Library::default();
        library.open(search("a"));
        library.apply(&search_reply("a", vec![track("ta")]));
        library.open(search("b"));
        library.apply(&search_reply("b", vec![track("tb")]));

        assert_eq!(library.back(), None);
        assert_eq!(
            library.search.page_answer("b"),
            None,
            "b's answer must not be drawn under a's heading"
        );
        assert_eq!(
            search_request(&mut library),
            Some(Command::Search {
                query: "a".to_owned()
            })
        );

        library.apply(&search_reply("a", vec![track("ta")]));
        assert_eq!(library.back(), None);
        assert_eq!(
            search_request(&mut library),
            None,
            "the root asks for nothing"
        );
        assert_eq!(library.forward(), None);
        assert_eq!(
            search_request(&mut library),
            None,
            "a's answer is still held"
        );
        assert_eq!(
            library.search.page_answer("a"),
            Some(&Load::Ready(vec![track("ta")]))
        );
    }

    #[test]
    fn a_playlist_reply_keeps_the_search_pages_in_the_history() {
        let mut library = Library::default();
        library.apply(&Update::Playlists(vec![playlist(
            "spotify:playlist:a",
            "Mix",
        )]));
        library.select("spotify:playlist:a");
        library.open(search("never gonna"));

        // The playlist leaves the library: its page is pruned, the search is not.
        library.apply(&Update::Playlists(Vec::new()));

        assert_eq!(library.page_search_query(), Some("never gonna"));
        assert!(library.can_back(), "the pruned playlist was behind it");
        library.back();
        assert_eq!(library.page(), None, "the pruned playlist is skipped");
    }

    #[test]
    fn a_search_query_that_spells_a_playlist_uri_is_not_that_playlist() {
        let query = "spotify:playlist:x";
        let mut library = Library::default();
        library.apply(&Update::Playlists(vec![playlist(query, "Mix")]));
        library.open(search(query));

        assert_eq!(library.page_search_query(), Some(query));
        assert_eq!(
            library.selected(),
            None,
            "the playlist highlight must not follow a search's text"
        );
        assert_eq!(library.wanted, None);
        assert_eq!(library.tracks, Load::Idle);

        // And a playlist reply cannot prune it: the variant decides, not the
        // text.
        library.apply(&Update::Playlists(Vec::new()));
        assert_eq!(library.page_search_query(), Some(query));
    }

    /// Retry is the page's own control, so it asks for the page's query even
    /// while the menu is open and idle on another key: `request` would take the
    /// menu's read and leave the page `Idle` until a later frame.
    #[test]
    fn reload_search_asks_for_the_page_while_the_menu_is_open_on_another_key() {
        let mut library = Library::default();
        library.open(search("never gonna"));
        library.apply(&Update::SearchFailed {
            query: "never gonna".to_owned(),
            failure: Failure::Search,
        });
        type_search(&mut library, "give you up");
        assert!(
            library.search.is_open(),
            "the menu is open on the typed key"
        );
        assert_eq!(
            library.search.menu_view(),
            crate::search::MenuView::Nothing,
            "and has nothing to draw: its read is idle"
        );

        assert_eq!(
            library.reload_search(),
            Some(Command::Search {
                query: "never gonna".to_owned()
            }),
            "the retry asks for the page, not for the menu's key"
        );
        assert_eq!(
            library.search.page_answer("never gonna"),
            Some(&Load::Loading)
        );
    }

    #[test]
    fn reload_search_sets_the_page_idle_and_asks_once() {
        let mut library = Library::default();
        library.open(search("never gonna"));
        library.apply(&Update::SearchFailed {
            query: "never gonna".to_owned(),
            failure: Failure::Search,
        });
        assert_eq!(
            library.search.page_answer("never gonna"),
            Some(&Load::Failed(Failure::Search))
        );

        assert_eq!(
            library.reload_search(),
            Some(Command::Search {
                query: "never gonna".to_owned()
            }),
            "the page's Retry is this call, so it asks in the same frame"
        );
        assert_eq!(
            library.search.page_answer("never gonna"),
            Some(&Load::Loading)
        );
        assert_eq!(library.reload_search(), None, "one read on the wire");

        // Retry is only the search page's: nowhere else has a page to reload.
        let mut library = Library::default();
        assert_eq!(library.reload_search(), None);
    }

    #[test]
    fn a_portrait_reply_does_not_disturb_the_search_reads() {
        let mut library = Library::default();
        library.open(search("never gonna"));
        library.apply(&search_reply("never gonna", vec![track("t")]));
        let menu = format!("{:?}", library.search.menu_view());
        let page = library.search.page_answer("never gonna").cloned();

        library.apply(&Update::Portraits {
            query: "never gonna".to_owned(),
            portraits: vec![ArtistPortrait {
                artist: "spotify:artist:a".to_owned(),
                portrait: CoverUrl::from_wire("https://i.scdn.co/image/abc"),
            }],
        });
        library.apply(&Update::PortraitsFailed {
            query: "never gonna".to_owned(),
        });

        assert_eq!(
            format!("{:?}", library.search.menu_view()),
            menu,
            "a portrait reply is not a page or a menu answer"
        );
        assert_eq!(library.search.page_answer("never gonna"), page.as_ref());
        assert_eq!(library.page_search_query(), Some("never gonna"));
    }

    #[test]
    fn to_playlists_clears_the_show_want_and_drops_an_unsettled_load() {
        let mut loading = Library::default();
        loading.open(show("spotify:show:a"));
        loading.to_playlists();
        assert_eq!(loading.show, Load::Idle, "a load nobody wants is dropped");
        assert_eq!(
            loading.open(show("spotify:show:a")),
            Some(show_command("spotify:show:a")),
            "the want was cleared, so the show is asked for again"
        );

        let mut failed = Library::default();
        failed.open(show("spotify:show:a"));
        failed.apply(&Update::ShowFailed {
            show: "spotify:show:a".to_owned(),
            failure: Failure::Load,
        });
        failed.to_playlists();
        assert_eq!(failed.show, Load::Idle);

        let mut ready = Library::default();
        ready.open(show("spotify:show:a"));
        ready.apply(&Update::Show(show_detail("spotify:show:a")));
        ready.to_playlists();
        assert_eq!(
            ready.show,
            Load::Ready(show_detail("spotify:show:a")),
            "a settled show is kept for the page Back returns to"
        );
        assert_eq!(
            ready.open(show("spotify:show:a")),
            Some(show_command("spotify:show:a")),
            "the want is cleared even when the show stays ready"
        );
    }

    #[test]
    fn a_playlist_prune_leaves_the_show_alone() {
        let mut library = Library::default();
        library.select("a");
        library.open(show("spotify:show:a"));
        library.apply(&Update::Show(show_detail("spotify:show:a")));

        // The list no longer holds the playlist, so its page is pruned.
        library.apply(&Update::Playlists(vec![playlist("b", "B")]));

        assert_eq!(library.page(), Some(&show("spotify:show:a")));
        assert_eq!(
            library.show,
            Load::Ready(show_detail("spotify:show:a")),
            "a playlist prune never touches the show"
        );
        assert_eq!(library.current_show(), Some(&show_detail("spotify:show:a")));
    }

    #[test]
    fn current_show_is_none_while_loading_or_for_another_uri() {
        let mut library = Library::default();
        library.open(show("spotify:show:b"));
        assert_eq!(library.current_show(), None, "loading names nothing");

        library.show = Load::Ready(show_detail("spotify:show:a"));
        assert_eq!(
            library.current_show(),
            None,
            "A's detail must not name B's page"
        );

        let mut loaded = Library::default();
        loaded.open(show("spotify:show:a"));
        loaded.apply(&Update::Show(show_detail("spotify:show:a")));
        assert_eq!(
            loaded.current_show(),
            Some(&show_detail("spotify:show:a")),
            "a detail that names the page is drawn"
        );
    }

    #[test]
    fn followed_starts_idle() {
        assert_eq!(Library::default().followed, Load::Idle);
    }

    #[test]
    fn followed_request_waits_for_ready_playlists() {
        let mut library = Library::default();
        assert_eq!(library.followed_request(), None, "nothing has loaded yet");

        library.apply(&Update::SignedIn);
        assert_eq!(library.followed_request(), None, "loading is not ready");

        library.apply(&Update::PlaylistFailed(Failure::Load));
        assert_eq!(
            library.followed_request(),
            None,
            "a failed library never names a window"
        );
        assert_eq!(library.followed, Load::Idle);

        let mut ready = Library::default();
        ready.apply(&Update::Playlists(Vec::new()));
        assert_eq!(ready.followed_request(), Some(Command::LoadFollowedShows));
        assert_eq!(ready.followed, Load::Loading);
    }

    #[test]
    fn followed_request_fires_once() {
        let mut library = Library::default();
        library.apply(&Update::Playlists(vec![playlist("a", "A")]));

        assert_eq!(library.followed_request(), Some(Command::LoadFollowedShows));
        assert_eq!(
            library.followed_request(),
            None,
            "an in-flight list is not asked for twice"
        );
    }

    #[test]
    fn followed_request_is_none_while_loading_ready_or_failed() {
        for state in [
            Load::Loading,
            Load::Ready(vec![followed_show("spotify:show:a")]),
            Load::Failed(Failure::Followed),
        ] {
            let mut library = Library::default();
            library.apply(&Update::Playlists(vec![playlist("a", "A")]));
            library.followed = state.clone();

            assert_eq!(library.followed_request(), None, "{state:?} asked again");
            assert_eq!(library.followed, state);
        }
    }

    #[test]
    fn a_followed_reply_is_recorded_whatever_the_page() {
        let shows = vec![followed_show("spotify:show:a")];
        let mut library = Library::default();
        library.apply(&Update::FollowedShows(shows.clone()));
        assert_eq!(library.followed, Load::Ready(shows.clone()));

        library.open(show("spotify:show:b"));
        library.apply(&Update::FollowedShows(shows.clone()));
        assert_eq!(
            library.followed,
            Load::Ready(shows),
            "a show page does not gate the list"
        );

        library.apply(&Update::FollowedShows(Vec::new()));
        assert_eq!(library.followed, Load::Ready(Vec::new()));
    }

    #[test]
    fn a_followed_failure_is_recorded() {
        let mut library = Library::default();
        library.apply(&Update::Playlists(vec![playlist("a", "A")]));
        library.apply(&Update::FollowedShowsFailed(Failure::Followed));

        assert_eq!(library.followed, Load::Failed(Failure::Followed));
        assert_eq!(
            library.playlists,
            Load::Ready(vec![playlist("a", "A")]),
            "the playlists beside it still draw"
        );
    }

    #[test]
    fn reload_resets_followed_so_it_refetches() {
        let mut library = Library::default();
        library.apply(&Update::Playlists(vec![playlist("a", "A")]));
        library.apply(&Update::FollowedShows(vec![followed_show(
            "spotify:show:a",
        )]));

        assert_eq!(library.reload(), Command::ReloadLibrary);
        assert_eq!(library.followed, Load::Idle, "a playlist retry re-arms it");
        assert_eq!(
            library.followed_request(),
            None,
            "the playlists are loading again, so the list waits"
        );

        library.apply(&Update::Playlists(vec![playlist("a", "A")]));
        assert_eq!(library.followed_request(), Some(Command::LoadFollowedShows));
    }

    #[test]
    fn reload_followed_marks_loading_and_returns_the_command() {
        let mut library = Library::default();
        library.apply(&Update::FollowedShowsFailed(Failure::Followed));

        assert_eq!(library.reload_followed(), Command::LoadFollowedShows);
        assert_eq!(library.followed, Load::Loading);

        let mut ready = Library::default();
        ready.apply(&Update::FollowedShows(Vec::new()));
        assert_eq!(ready.reload_followed(), Command::LoadFollowedShows);
        assert_eq!(ready.followed, Load::Loading);
    }

    #[test]
    fn reload_while_followed_is_loading_accepts_either_reply() {
        let mut library = Library::default();
        library.apply(&Update::Playlists(vec![playlist("a", "A")]));
        assert_eq!(library.followed_request(), Some(Command::LoadFollowedShows));
        assert_eq!(library.followed, Load::Loading);

        // A playlist retry while the first load is still in flight re-arms the
        // list, so a second load is issued and two replies are in the air.
        library.reload();
        assert_eq!(library.followed, Load::Idle);
        library.apply(&Update::Playlists(vec![playlist("a", "A")]));
        assert_eq!(
            library.followed_request(),
            Some(Command::LoadFollowedShows),
            "the second load is issued while the first is still in flight"
        );

        let first = vec![followed_show("spotify:show:a")];
        let second = vec![followed_show("spotify:show:b")];
        library.apply(&Update::FollowedShows(first.clone()));
        assert_eq!(library.followed, Load::Ready(first));
        library.apply(&Update::FollowedShows(second.clone()));
        assert_eq!(
            library.followed,
            Load::Ready(second),
            "the later whole-set reply wins and neither corrupts the state"
        );
    }

    /// The likeness cards are the artist page's second read: nothing is asked
    /// until the artist's own detail names somebody, and then it is asked once.
    #[test]
    fn the_related_request_waits_for_the_artist_and_fires_once() {
        let mut library = Library::default();
        assert_eq!(library.related_request(), None, "no artist page is open");

        // Ready with nobody to draw: no request, and no section.
        library.artist = Load::Ready(related_artist("spotify:artist:1", Vec::new()));
        assert_eq!(library.related_request(), None, "nobody was named");

        library.artist = Load::Ready(related_artist(
            "spotify:artist:1",
            vec!["spotify:artist:2".to_owned(), "spotify:artist:3".to_owned()],
        ));
        assert_eq!(
            library.related_request(),
            Some(Command::LoadRelated {
                artist: "spotify:artist:1".to_owned(),
                uris: vec!["spotify:artist:2".to_owned(), "spotify:artist:3".to_owned()],
            })
        );
        assert_eq!(library.related, Load::Loading);
        assert_eq!(library.related_request(), None, "the load is in flight");
    }

    #[test]
    fn a_related_reply_is_taken_only_for_the_artist_that_asked() {
        let mut library = Library {
            artist: Load::Ready(related_artist(
                "spotify:artist:1",
                vec!["spotify:artist:2".to_owned()],
            )),
            ..Library::default()
        };
        let _ = library.related_request();

        library.apply(&Update::Related {
            artist: "spotify:artist:9".to_owned(),
            cards: vec![followed_artist("spotify:artist:9")],
        });
        assert_eq!(library.related, Load::Loading, "another artist's cards");

        library.apply(&Update::Related {
            artist: "spotify:artist:1".to_owned(),
            cards: vec![followed_artist("spotify:artist:2")],
        });
        assert!(matches!(library.related, Load::Ready(_)));
    }

    #[test]
    fn another_artist_page_starts_a_fresh_likeness_list() {
        let mut library = Library {
            artist: Load::Ready(related_artist(
                "spotify:artist:1",
                vec!["spotify:artist:2".to_owned()],
            )),
            ..Library::default()
        };
        let _ = library.related_request();
        library.apply(&Update::Related {
            artist: "spotify:artist:1".to_owned(),
            cards: vec![followed_artist("spotify:artist:2")],
        });

        // Opening another artist drops the first one's row.
        let _ = library.open(Page::Artist("spotify:artist:5".to_owned()));
        assert_eq!(library.related, Load::Idle);
        library.artist = Load::Ready(related_artist(
            "spotify:artist:5",
            vec!["spotify:artist:6".to_owned()],
        ));
        assert!(matches!(
            library.related_request(),
            Some(Command::LoadRelated { .. })
        ));
    }

    #[test]
    fn the_library_draws_tiles_until_rows_are_chosen() {
        let mut library = Library::default();
        assert_eq!(library.layout, MediaLayout::Grid);

        library.layout = MediaLayout::List;
        // The choice survives a page, because the library outlives it.
        library.apply(&Update::SignedIn);
        let _ = library.select("spotify:playlist:a");
        assert_eq!(library.layout, MediaLayout::List);
        library.back();
        assert_eq!(library.layout, MediaLayout::List, "back keeps the density");
    }

    #[test]
    fn followed_artists_and_saved_albums_start_idle() {
        let library = Library::default();
        assert_eq!(library.followed_artists, Load::Idle);
        assert_eq!(library.saved_albums, Load::Idle);
    }

    #[test]
    fn the_artist_and_album_requests_stay_silent_until_the_playlists_are_ready() {
        let mut library = Library::default();
        assert_eq!(
            library.followed_artists_request(),
            None,
            "nothing has loaded yet"
        );
        assert_eq!(
            library.saved_albums_request(),
            None,
            "nothing has loaded yet"
        );

        library.apply(&Update::SignedIn);
        assert_eq!(
            library.followed_artists_request(),
            None,
            "loading is not ready"
        );
        assert_eq!(library.saved_albums_request(), None, "loading is not ready");

        library.apply(&Update::PlaylistFailed(Failure::Load));
        assert_eq!(
            library.followed_artists_request(),
            None,
            "a failed library never names a window"
        );
        assert_eq!(
            library.saved_albums_request(),
            None,
            "a failed library never names a window"
        );

        // All draws every kind, so a ready library under it asks for both.
        let mut ready = Library::default();
        ready.apply(&Update::Playlists(Vec::new()));
        assert_eq!(
            ready.followed_artists_request(),
            Some(Command::LoadFollowedArtists)
        );
        assert_eq!(ready.saved_albums_request(), Some(Command::LoadSavedAlbums));

        // A chip that draws only playlists names neither list, so both stay
        // idle until one is chosen.
        let mut narrowed = Library::default();
        narrowed.apply(&Update::Playlists(Vec::new()));
        narrowed.root_filter.choose(RootKind::Playlists);
        assert_eq!(narrowed.followed_artists_request(), None);
        assert_eq!(narrowed.saved_albums_request(), None);
        assert_eq!(narrowed.followed_artists, Load::Idle);
        assert_eq!(narrowed.saved_albums, Load::Idle);
    }

    /// The chip half of the two requests, tested where it can be reached: no
    /// chip naming artists or albums can be pressed until the controls act, so
    /// the condition is a function of the two chosen kinds.
    #[test]
    fn a_chip_naming_the_list_or_all_asks_for_it() {
        for root in crate::library_filter::ROW {
            for sidebar in crate::sidebar_kind::ROW {
                let all = root == RootKind::All;
                assert_eq!(
                    artists_chosen(root, sidebar),
                    all || root == RootKind::Artists || sidebar == SidebarKind::Artists,
                    "artists at {root:?} / {sidebar:?}"
                );
                assert_eq!(
                    albums_chosen(root, sidebar),
                    all || root == RootKind::Albums || sidebar == SidebarKind::Albums,
                    "albums at {root:?} / {sidebar:?}"
                );
            }
        }

        // Either surface names the list, so a root Artists chip beside a
        // sidebar Albums chip still asks for both lists.
        assert!(artists_chosen(RootKind::Artists, SidebarKind::Artists));
        assert!(artists_chosen(RootKind::Artists, SidebarKind::Albums));
        assert!(!artists_chosen(RootKind::Albums, SidebarKind::Albums));
        assert!(!albums_chosen(RootKind::Podcasts, SidebarKind::Playlists));
    }

    /// The state half of the two requests. The chip half cannot be exercised
    /// here, so this pins only that a list already loading, ready or failed is
    /// never asked for again.
    #[test]
    fn a_list_that_already_has_an_answer_is_never_asked_again() {
        let artist_states: [Load<Vec<FollowedArtist>>; 3] = [
            Load::Loading,
            Load::Ready(vec![followed_artist("spotify:artist:a")]),
            Load::Failed(Failure::FollowedArtists),
        ];
        for state in artist_states {
            let mut library = Library::default();
            library.apply(&Update::Playlists(vec![playlist("a", "A")]));
            library.followed_artists = state.clone();

            assert_eq!(
                library.followed_artists_request(),
                None,
                "{state:?} asked again"
            );
            assert_eq!(library.followed_artists, state);
        }

        let album_states: [Load<Vec<SavedAlbum>>; 3] = [
            Load::Loading,
            Load::Ready(vec![saved_album("spotify:album:a")]),
            Load::Failed(Failure::SavedAlbums),
        ];
        for state in album_states {
            let mut library = Library::default();
            library.apply(&Update::Playlists(vec![playlist("a", "A")]));
            library.saved_albums = state.clone();

            assert_eq!(
                library.saved_albums_request(),
                None,
                "{state:?} asked again"
            );
            assert_eq!(library.saved_albums, state);
        }
    }

    #[test]
    fn an_artist_list_reply_is_recorded_whatever_the_page() {
        let artists = vec![followed_artist("spotify:artist:a")];
        let mut library = Library::default();
        library.apply(&Update::FollowedArtists(artists.clone()));
        assert_eq!(library.followed_artists, Load::Ready(artists.clone()));

        library.open(show("spotify:show:b"));
        library.apply(&Update::FollowedArtists(artists.clone()));
        assert_eq!(
            library.followed_artists,
            Load::Ready(artists),
            "a show page does not gate the list"
        );

        library.apply(&Update::FollowedArtists(Vec::new()));
        assert_eq!(library.followed_artists, Load::Ready(Vec::new()));
    }

    #[test]
    fn a_saved_album_reply_is_recorded_whatever_the_page() {
        let albums = vec![saved_album("spotify:album:a")];
        let mut library = Library::default();
        library.apply(&Update::SavedAlbums(albums.clone()));
        assert_eq!(library.saved_albums, Load::Ready(albums.clone()));

        library.open(artist("spotify:artist:b"));
        library.apply(&Update::SavedAlbums(albums.clone()));
        assert_eq!(
            library.saved_albums,
            Load::Ready(albums),
            "an artist page does not gate the list"
        );

        library.apply(&Update::SavedAlbums(Vec::new()));
        assert_eq!(library.saved_albums, Load::Ready(Vec::new()));
    }

    #[test]
    fn an_artist_list_failure_is_recorded() {
        let mut library = Library::default();
        library.apply(&Update::Playlists(vec![playlist("a", "A")]));
        library.apply(&Update::FollowedArtistsFailed(Failure::FollowedArtists));

        assert_eq!(
            library.followed_artists,
            Load::Failed(Failure::FollowedArtists)
        );
        assert_eq!(
            library.playlists,
            Load::Ready(vec![playlist("a", "A")]),
            "the playlists beside it still draw"
        );
    }

    #[test]
    fn a_saved_album_failure_is_recorded() {
        let mut library = Library::default();
        library.apply(&Update::Playlists(vec![playlist("a", "A")]));
        library.apply(&Update::SavedAlbumsFailed(Failure::SavedAlbums));

        assert_eq!(library.saved_albums, Load::Failed(Failure::SavedAlbums));
        assert_eq!(
            library.playlists,
            Load::Ready(vec![playlist("a", "A")]),
            "the playlists beside it still draw"
        );
    }

    #[test]
    fn reload_rearms_the_artist_and_album_lists() {
        let mut library = Library::default();
        library.apply(&Update::Playlists(vec![playlist("a", "A")]));
        library.apply(&Update::FollowedArtists(vec![followed_artist(
            "spotify:artist:a",
        )]));
        library.apply(&Update::SavedAlbums(vec![saved_album("spotify:album:a")]));

        assert_eq!(library.reload(), Command::ReloadLibrary);
        assert_eq!(
            library.followed_artists,
            Load::Idle,
            "a playlist retry re-arms it"
        );
        assert_eq!(
            library.saved_albums,
            Load::Idle,
            "a playlist retry re-arms it"
        );
    }

    #[test]
    fn reload_followed_artists_clears_the_stale_state_and_returns_the_command() {
        let mut failed = Library::default();
        failed.apply(&Update::FollowedArtistsFailed(Failure::FollowedArtists));
        assert_eq!(
            failed.reload_followed_artists(),
            Command::LoadFollowedArtists
        );
        assert_eq!(failed.followed_artists, Load::Loading);

        let mut loading = Library {
            followed_artists: Load::Loading,
            ..Library::default()
        };
        assert_eq!(
            loading.reload_followed_artists(),
            Command::LoadFollowedArtists
        );
        assert_eq!(
            loading.followed_artists,
            Load::Loading,
            "a stale load is dropped and asked again"
        );

        let mut ready = Library::default();
        ready.apply(&Update::FollowedArtists(Vec::new()));
        assert_eq!(
            ready.reload_followed_artists(),
            Command::LoadFollowedArtists
        );
        assert_eq!(ready.followed_artists, Load::Loading);
    }

    #[test]
    fn reload_saved_albums_clears_the_stale_state_and_returns_the_command() {
        let mut failed = Library::default();
        failed.apply(&Update::SavedAlbumsFailed(Failure::SavedAlbums));
        assert_eq!(failed.reload_saved_albums(), Command::LoadSavedAlbums);
        assert_eq!(failed.saved_albums, Load::Loading);

        let mut loading = Library {
            saved_albums: Load::Loading,
            ..Library::default()
        };
        assert_eq!(loading.reload_saved_albums(), Command::LoadSavedAlbums);
        assert_eq!(
            loading.saved_albums,
            Load::Loading,
            "a stale load is dropped and asked again"
        );

        let mut ready = Library::default();
        ready.apply(&Update::SavedAlbums(Vec::new()));
        assert_eq!(ready.reload_saved_albums(), Command::LoadSavedAlbums);
        assert_eq!(ready.saved_albums, Load::Loading);
    }

    #[test]
    fn back_to_a_ready_playlist_returns_no_command() {
        let mut library = Library::default();
        library.select("a");
        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: vec![track("t")],
            cover: None,
            access: None,
        });
        library.open(album("album"));

        assert_eq!(library.back(), None);
        assert_eq!(library.page(), Some(&Page::Playlist("a".to_owned())));
        assert_eq!(
            library.tracks,
            Load::Ready(tracks_for("a", vec![track("t")]))
        );
    }

    #[test]
    fn back_to_a_failed_playlist_loads_it_again() {
        let mut library = Library::default();
        library.select("a");
        library.apply(&Update::TracksFailed {
            playlist: "a".to_owned(),
            failure: Failure::Load,
        });
        library.open(album("album"));

        assert_eq!(
            library.back(),
            Some(Command::LoadTracks {
                playlist: "a".to_owned()
            })
        );
        assert_eq!(library.tracks, Load::Loading);
        // The reload must not push the page it lands on.
        assert!(library.back().is_none(), "the root is the only entry left");
        assert!(!library.can_back());
    }

    #[test]
    fn back_to_an_idle_playlist_loads_it() {
        let mut library = Library::default();
        library.select("a");
        library.open(album("album"));
        library.tracks = Load::Idle;

        assert_eq!(
            library.back(),
            Some(Command::LoadTracks {
                playlist: "a".to_owned()
            })
        );
        assert_eq!(library.tracks, Load::Loading);
    }

    #[test]
    fn back_to_a_loading_playlist_returns_no_command() {
        let mut library = Library::default();
        library.select("a");
        library.open(album("album"));

        assert_eq!(library.back(), None);
        assert_eq!(library.tracks, Load::Loading);
    }

    #[test]
    fn back_from_the_root_loads_the_playlist() {
        let mut library = Library::default();
        library.select("a");
        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: vec![track("t")],
            cover: None,
            access: None,
        });
        library.to_playlists();

        assert_eq!(
            library.back(),
            Some(Command::LoadTracks {
                playlist: "a".to_owned()
            })
        );
        assert_eq!(library.page(), Some(&Page::Playlist("a".to_owned())));
        assert_eq!(library.tracks, Load::Loading);
    }

    #[test]
    fn a_reply_for_the_wanted_playlist_is_applied_on_an_album_page() {
        let mut library = Library::default();
        library.select("a");
        library.open(album("album"));

        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: vec![track("t")],
            cover: None,
            access: None,
        });

        assert_eq!(library.page(), Some(&album("album")));
        assert_eq!(
            library.tracks,
            Load::Ready(tracks_for("a", vec![track("t")]))
        );
    }

    #[test]
    fn can_back_and_can_forward_track_the_moves() {
        let mut library = Library::default();
        assert!(!library.can_back());
        assert!(!library.can_forward());

        library.select("a");
        assert!(library.can_back());
        assert!(!library.can_forward());

        assert_eq!(
            library.open(album("spotify:album:rec")),
            Some(Command::LoadAlbum {
                album: "spotify:album:rec".to_owned()
            })
        );
        assert_eq!(
            library.back(),
            None,
            "back onto the playlist already wanted needs no load"
        );
        assert!(library.can_forward());

        assert_eq!(
            library.forward(),
            None,
            "forward onto the album already wanted needs no load"
        );
        assert!(!library.can_forward());
        assert_eq!(library.page(), Some(&album("spotify:album:rec")));
    }

    #[test]
    fn a_prune_keeps_an_album_page_and_the_album_and_artist_entries() {
        let mut library = Library::default();
        library.apply(&Update::Playlists(vec![playlist("a", "A")]));
        library.select("a");
        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: vec![track("t")],
            cover: None,
            access: None,
        });
        library.open(album("album"));
        library.open(artist("artist"));
        library.back();

        library.apply(&Update::Playlists(vec![playlist("a", "A")]));

        assert_eq!(library.page(), Some(&album("album")));
        assert_eq!(
            library.tracks,
            Load::Ready(tracks_for("a", vec![track("t")]))
        );
        assert_eq!(library.back(), None);
        assert_eq!(library.page(), Some(&Page::Playlist("a".to_owned())));
        assert_eq!(library.forward(), None);
        assert_eq!(library.page(), Some(&album("album")));
        assert_eq!(library.forward(), None);
        assert_eq!(library.page(), Some(&artist("artist")));
    }

    #[test]
    fn a_prune_of_the_current_playlist_returns_to_the_root_without_duplicates() {
        let mut library = Library::default();
        library.apply(&Update::Playlists(vec![playlist("a", "A")]));
        library.select("a");
        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: vec![track("t")],
            cover: None,
            access: None,
        });

        library.apply(&Update::Playlists(vec![playlist("b", "B")]));

        assert_eq!(library.page(), None);
        assert_eq!(library.selected(), None);
        assert_eq!(library.tracks, Load::Idle);
        assert_eq!(library.wanted, None);
        assert!(
            !library.can_back(),
            "the root must not be duplicated behind the root"
        );
    }

    #[test]
    fn back_with_nowhere_to_go_does_not_reload() {
        let mut library = Library::default();
        library.select("a");
        library.apply(&Update::TracksFailed {
            playlist: "a".to_owned(),
            failure: Failure::Load,
        });

        assert_eq!(library.back(), None, "the root needs no command");
        assert_eq!(library.page(), None);
        assert_eq!(
            library.back(),
            None,
            "no page left behind the root, and no reload"
        );
        assert!(!library.can_back());
    }

    #[test]
    fn forward_with_nowhere_to_go_does_not_reload_a_failed_playlist() {
        let mut library = Library::default();
        library.select("a");
        library.apply(&Update::TracksFailed {
            playlist: "a".to_owned(),
            failure: Failure::Load,
        });

        assert!(!library.can_forward());
        assert_eq!(
            library.forward(),
            None,
            "a no-op Forward must not retry the page it is on"
        );
        assert_eq!(library.tracks, Load::Failed(Failure::Load));
        assert_eq!(library.page(), Some(&Page::Playlist("a".to_owned())));
    }

    #[test]
    fn a_prune_with_no_wanted_load_clears_ready_tracks() {
        let mut library = Library::default();
        library.apply(&Update::Playlists(vec![playlist("a", "A")]));
        library.select("a");
        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: vec![track("t")],
            cover: None,
            access: None,
        });
        library.to_playlists();
        assert_eq!(library.wanted, None);
        assert!(matches!(library.tracks, Load::Ready(_)));

        library.apply(&Update::Playlists(vec![playlist("b", "B")]));

        assert_eq!(
            library.tracks,
            Load::Idle,
            "the deleted playlist's ready tracks must be freed"
        );
    }

    #[test]
    fn to_playlists_while_loading_leaves_no_spinner_at_the_root() {
        let mut library = Library::default();
        library.apply(&Update::Playlists(vec![playlist("a", "A")]));
        library.select("a");
        assert_eq!(library.tracks, Load::Loading);

        library.to_playlists();

        assert_eq!(
            library.tracks,
            Load::Idle,
            "the root must not sit on a spinner nothing can settle"
        );
        assert_eq!(library.wanted, None);
    }

    #[test]
    fn to_playlists_drops_a_failed_load() {
        let mut library = Library::default();
        library.select("a");
        library.apply(&Update::TracksFailed {
            playlist: "a".to_owned(),
            failure: Failure::Load,
        });

        library.to_playlists();

        assert_eq!(library.tracks, Load::Idle);
        assert_eq!(library.wanted, None);
    }

    #[test]
    fn tracks_keep_their_added_at_through_apply() {
        let mut library = Library::default();
        library.select("a");
        let tracks = vec![
            dated("t0", "1700000000000"),
            track("t1"),
            dated("t2", "1792357200000"),
        ];
        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: tracks.clone(),
            cover: None,
            access: None,
        });

        assert_eq!(library.tracks, Load::Ready(tracks_for("a", tracks)));
    }

    #[test]
    fn a_reply_for_the_selected_playlist_is_applied() {
        let mut library = Library::default();
        library.select("a");
        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: vec![track("t")],
            cover: None,
            access: None,
        });

        assert_eq!(
            library.tracks,
            Load::Ready(tracks_for("a", vec![track("t")]))
        );
    }

    #[test]
    fn a_reply_for_a_stale_playlist_is_discarded() {
        let mut library = Library::default();
        library.select("a");
        library.select("b");
        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: vec![track("t")],
            cover: None,
            access: None,
        });

        assert_eq!(library.tracks, Load::Loading);
        assert_eq!(library.selected(), Some("b"));
    }

    #[test]
    fn a_failure_for_a_stale_playlist_is_discarded() {
        let mut library = Library::default();
        library.select("a");
        library.select("b");
        library.apply(&Update::TracksFailed {
            playlist: "a".to_owned(),
            failure: Failure::Load,
        });

        assert_eq!(library.tracks, Load::Loading);
    }

    #[test]
    fn a_failure_for_the_selected_playlist_is_recorded() {
        let mut library = Library::default();
        library.select("a");
        library.apply(&Update::TracksFailed {
            playlist: "a".to_owned(),
            failure: Failure::Load,
        });

        assert_eq!(library.tracks, Load::Failed(Failure::Load));
    }

    #[test]
    fn tracks_before_any_selection_are_discarded() {
        let mut library = Library::default();
        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: vec![track("t")],
            cover: None,
            access: None,
        });
        assert_eq!(library.tracks, Load::Idle);
    }

    /// The entry with what the listing read said about writing to it.
    fn with_access(
        entry: PlaylistEntry,
        byte: u8,
        editable: bool,
        tracks: Option<usize>,
    ) -> PlaylistEntry {
        PlaylistEntry {
            access: Some(PlaylistAccess {
                revision: revision(byte),
                editable,
            }),
            tracks,
            ..entry
        }
    }

    fn rootlist_access(byte: u8, editable: bool) -> PlaylistAccess {
        PlaylistAccess {
            revision: revision(byte),
            editable,
        }
    }

    /// A library whose sidebar holds `names`, each read at `byte`.
    fn sidebar_library(names: &[&str], byte: u8, editable: bool, tracks: Option<usize>) -> Library {
        let mut library = Library::default();
        let entries = names
            .iter()
            .map(|name| with_access(playlist(name, name), byte, editable, tracks))
            .collect();
        library.apply(&Update::Playlists(entries));
        library.apply(&Update::RootlistAccess(rootlist_access(byte, editable)));
        library
    }

    fn sidebar_order(library: &Library) -> Vec<String> {
        match &library.playlists {
            Load::Ready(playlists) => playlists.iter().map(|entry| entry.uri.clone()).collect(),
            Load::Idle | Load::Loading | Load::Failed(_) => Vec::new(),
        }
    }

    fn sidebar_entry(library: &Library, uri: &str) -> Option<PlaylistEntry> {
        match &library.playlists {
            Load::Ready(playlists) => playlists.iter().find(|entry| entry.uri == uri).cloned(),
            Load::Idle | Load::Loading | Load::Failed(_) => None,
        }
    }

    fn names(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn a_downward_drag_names_the_gap_after_its_own_row() {
        let mut library = sidebar_library(&["a", "b", "c", "d"], 0x07, true, None);

        let found = library.move_playlist(0, 2, "a");

        assert_eq!(
            found,
            Some(Command::MovePlaylist {
                revision: revision(0x07),
                from: 0,
                to: 2,
            })
        );
        assert_eq!(
            sidebar_order(&library),
            names(&["b", "a", "c", "d"]),
            "the gap after b is where a lands"
        );
    }

    #[test]
    fn a_downward_drag_to_the_very_end_names_the_gap_at_the_length() {
        let mut library = sidebar_library(&["a", "b", "c", "d"], 0x07, true, None);

        let found = library.move_playlist(1, 4, "b");

        assert_eq!(
            found,
            Some(Command::MovePlaylist {
                revision: revision(0x07),
                from: 1,
                to: 4,
            })
        );
        assert_eq!(sidebar_order(&library), names(&["a", "c", "d", "b"]));
    }

    #[test]
    fn an_upward_drag_names_the_gap_it_lands_in() {
        let mut library = sidebar_library(&["a", "b", "c", "d"], 0x07, true, None);

        let found = library.move_playlist(3, 0, "d");

        assert_eq!(
            found,
            Some(Command::MovePlaylist {
                revision: revision(0x07),
                from: 3,
                to: 0,
            })
        );
        assert_eq!(sidebar_order(&library), names(&["d", "a", "b", "c"]));
    }

    #[test]
    fn a_drag_beside_itself_sends_nothing() {
        let mut library = sidebar_library(&["a", "b", "c"], 0x07, true, None);

        for boundary in [1, 2] {
            assert_eq!(
                library.move_playlist(1, boundary, "b"),
                None,
                "the gap {boundary} is where b already is"
            );
        }
        assert_eq!(sidebar_order(&library), names(&["a", "b", "c"]));
        assert!(library.rootlist_movable(), "and nothing was written");
    }

    #[test]
    fn a_boundary_past_the_end_is_refused() {
        let mut library = sidebar_library(&["a", "b"], 0x07, true, None);
        assert_eq!(library.move_playlist(0, 3, "a"), None);
    }

    #[test]
    fn a_move_names_the_held_revision() {
        let mut library = sidebar_library(&["a", "b", "c"], 0x07, true, None);
        library.apply(&Update::RootlistAccess(rootlist_access(0x09, true)));

        assert_eq!(
            library.move_playlist(0, 2, "a"),
            Some(Command::MovePlaylist {
                revision: revision(0x09),
                from: 0,
                to: 2,
            })
        );
    }

    #[test]
    fn no_rootlist_access_refuses_a_move() {
        let mut library = Library::default();
        library.apply(&Update::Playlists(vec![
            playlist("a", "a"),
            playlist("b", "b"),
        ]));

        assert!(!library.rootlist_movable());
        assert_eq!(library.move_playlist(0, 2, "a"), None);
        assert_eq!(sidebar_order(&library), names(&["a", "b"]));
    }

    #[test]
    fn a_not_faithful_rootlist_refuses_a_move() {
        let mut library = sidebar_library(&["a", "b"], 0x07, false, None);

        assert!(!library.rootlist_movable());
        assert_eq!(library.move_playlist(0, 2, "a"), None);
    }

    #[test]
    fn a_playlist_that_is_not_where_the_drag_found_it_is_refused() {
        let mut library = sidebar_library(&["a", "b"], 0x07, true, None);

        assert_eq!(
            library.move_playlist(0, 2, "b"),
            None,
            "the row holds another playlist now"
        );
        assert_eq!(library.move_playlist(9, 0, "a"), None, "no such row");
        assert_eq!(sidebar_order(&library), names(&["a", "b"]));
    }

    #[test]
    fn a_move_waits_while_a_write_is_pending() {
        let mut library = sidebar_library(&["a", "b", "c"], 0x07, true, None);
        assert!(library.move_playlist(0, 2, "a").is_some());

        assert_eq!(library.move_playlist(0, 3, "b"), None);
        assert!(!library.rootlist_movable());
    }

    #[test]
    fn a_move_waits_while_a_page_move_is_pending() {
        let mut library = page_with("a", &["t0", "t1", "t2"], access(0x01, true));
        library.playlists = Load::Ready(vec![
            with_access(playlist("p0", "p0"), 0x07, true, None),
            with_access(playlist("p1", "p1"), 0x07, true, None),
        ]);
        library.apply(&Update::RootlistAccess(rootlist_access(0x07, true)));
        assert!(move_one(&mut library, "a", 0, 2, &track("t0")).is_some());

        assert_eq!(library.move_playlist(0, 2, "p0"), None);
    }

    #[test]
    fn a_page_move_waits_while_a_sidebar_write_is_pending() {
        let mut library = page_with("a", &["t0", "t1", "t2"], access(0x01, true));
        library.playlists = Load::Ready(vec![
            with_access(playlist("p0", "p0"), 0x07, true, None),
            with_access(playlist("p1", "p1"), 0x07, true, None),
        ]);
        library.apply(&Update::RootlistAccess(rootlist_access(0x07, true)));
        assert!(library.move_playlist(0, 2, "p0").is_some());

        assert_eq!(move_one(&mut library, "a", 0, 2, &track("t0")), None);
        assert!(!library.movable());
    }

    #[test]
    fn write_in_flight_is_any_pending_write() {
        let library = Library::default();
        assert!(!library.write_in_flight(), "an idle library has no write");

        let mut page = page_with("a", &["t0", "t1", "t2"], access(0x01, true));
        assert!(move_one(&mut page, "a", 0, 2, &track("t0")).is_some());
        assert!(page.write_in_flight(), "a page move is a write");

        let mut sidebar = sidebar_library(&["a", "b", "c"], 0x07, true, None);
        assert!(sidebar.move_playlist(0, 2, "a").is_some());
        assert!(sidebar.write_in_flight(), "a sidebar write is a write");
    }

    #[test]
    fn a_failed_like_reverts_and_the_healing_read_clears_the_sentence() {
        let mut library = Library::default();
        library.apply(&Update::Playlists(Vec::new()));
        assert_eq!(library.likes_request(), Some(Command::LoadLikedTracks));
        library.apply(&Update::LikedTracks(Vec::new()));

        let command = library
            .set_liked(vec!["spotify:track:a".to_owned()], true)
            .expect("a write");
        assert_eq!(
            command,
            Command::SetLiked {
                tracks: vec!["spotify:track:a".to_owned()],
                liked: true,
            }
        );
        assert_eq!(library.likes.state("spotify:track:a"), Some(true));
        assert!(!library.write_in_flight(), "a like is not a playlist write");

        library.apply(&Update::LikeFailed {
            tracks: vec!["spotify:track:a".to_owned()],
            failure: Failure::Save,
        });
        assert_eq!(library.likes.state("spotify:track:a"), Some(false));
        assert_eq!(library.likes.notice(), Some(Failure::Save));
        assert_eq!(library.likes_request(), Some(Command::LoadLikedTracks));

        library.apply(&Update::LikedTracks(Vec::new()));
        assert_eq!(library.likes.notice(), None);
    }

    #[test]
    fn a_like_confirmation_ends_the_write_and_leaves_the_heart_on() {
        let mut library = Library::default();
        library.apply(&Update::Playlists(Vec::new()));
        assert_eq!(library.likes_request(), Some(Command::LoadLikedTracks));
        library.apply(&Update::LikedTracks(Vec::new()));
        let track = "spotify:track:a".to_owned();
        assert!(
            library.set_liked(vec![track.clone()], true).is_some(),
            "arrange: the like must be in flight"
        );
        // A sweep asked for after the click is answered after it: it carries the
        // set from before the write, so adopting it would wipe the confirmed like.
        let _ = library.reload();
        library.apply(&Update::Playlists(Vec::new()));
        assert_eq!(
            library.likes_request(),
            Some(Command::LoadLikedTracks),
            "arrange: the sweep must be in flight"
        );

        library.apply(&Update::Liked {
            tracks: vec![track.clone()],
            liked: true,
        });
        library.apply(&Update::LikedTracks(Vec::new()));

        assert_eq!(
            library.likes.state(&track),
            Some(true),
            "a confirmed like outlives a read issued before its write landed"
        );
        assert!(
            library.set_liked(vec![track], false).is_some(),
            "the confirmation never ended the write, so the heart could not be flipped again"
        );
    }

    #[test]
    fn a_failure_sentence_does_not_survive_a_page_change() {
        let mut library = Library::default();
        library.apply(&Update::Playlists(Vec::new()));
        assert_eq!(library.likes_request(), Some(Command::LoadLikedTracks));
        library.apply(&Update::LikedTracks(Vec::new()));
        assert!(
            library
                .set_liked(vec!["spotify:track:a".to_owned()], true)
                .is_some()
        );
        library.apply(&Update::LikeFailed {
            tracks: vec!["spotify:track:a".to_owned()],
            failure: Failure::Save,
        });
        assert_eq!(library.likes.notice(), Some(Failure::Save));

        let _ = library.open(Page::Album("spotify:album:b".to_owned()));
        assert_eq!(
            library.likes.notice(),
            None,
            "the sentence names a page no longer on screen"
        );

        assert!(
            library
                .set_liked(vec!["spotify:track:a".to_owned()], true)
                .is_some()
        );
        library.apply(&Update::LikeFailed {
            tracks: vec!["spotify:track:a".to_owned()],
            failure: Failure::Save,
        });
        assert_eq!(library.likes.notice(), Some(Failure::Save));
        library.to_playlists();
        assert_eq!(
            library.likes.notice(),
            None,
            "the grid is not the page the like came from"
        );
    }

    #[test]
    fn a_loading_list_refuses_a_move() {
        let mut library = Library::default();
        library.apply(&Update::RootlistAccess(rootlist_access(0x07, true)));
        assert_eq!(library.move_playlist(0, 2, "a"), None);

        library.playlists = Load::Loading;
        assert!(!library.rootlist_movable());
    }

    #[test]
    fn the_reply_replaces_the_revision_and_reorders_nothing() {
        let mut library = sidebar_library(&["a", "b", "c"], 0x07, true, None);
        assert!(library.move_playlist(0, 2, "a").is_some());

        library.apply(&Update::PlaylistMoved {
            revision: revision(0x08),
        });

        assert_eq!(
            sidebar_order(&library),
            names(&["b", "a", "c"]),
            "the order already moved at drop time"
        );
        assert!(library.rootlist_movable(), "the write is answered");
        assert_eq!(
            library.move_playlist(0, 3, "b"),
            Some(Command::MovePlaylist {
                revision: revision(0x08),
                from: 0,
                to: 3,
            }),
            "the next move names the reply's revision"
        );
    }

    #[test]
    fn a_failed_move_restores_the_order_before_the_drop_and_says_so() {
        let mut library = sidebar_library(&["a", "b", "c"], 0x07, true, None);
        assert!(library.move_playlist(0, 2, "a").is_some());

        library.apply(&Update::PlaylistMoveFailed);

        assert_eq!(sidebar_order(&library), names(&["a", "b", "c"]));
        assert_eq!(library.sidebar_notice, Some(SidebarWrite::Reorder));
        assert!(
            library.rootlist_movable(),
            "the held revision is kept, so another drag may try again"
        );
        assert_eq!(
            library.move_playlist(0, 2, "a"),
            Some(Command::MovePlaylist {
                revision: revision(0x07),
                from: 0,
                to: 2,
            }),
            "the revision an unanswered write used is still the one held"
        );
    }

    #[test]
    fn a_playlists_read_while_a_move_is_pending_is_not_adopted() {
        let mut library = sidebar_library(&["a", "b", "c"], 0x07, true, None);
        assert!(library.move_playlist(0, 2, "a").is_some());

        library.apply(&Update::Playlists(vec![
            with_access(playlist("a", "a"), 0x07, true, None),
            with_access(playlist("b", "b"), 0x07, true, None),
            with_access(playlist("c", "c"), 0x07, true, None),
        ]));
        library.apply(&Update::RootlistAccess(rootlist_access(0x07, true)));

        assert_eq!(
            sidebar_order(&library),
            names(&["b", "a", "c"]),
            "a read taken before the drop would otherwise undo it"
        );
    }

    #[test]
    fn a_rootlist_access_read_while_a_move_is_pending_is_not_adopted() {
        let mut library = sidebar_library(&["a", "b", "c"], 0x07, true, None);
        assert!(library.move_playlist(0, 2, "a").is_some());

        library.apply(&Update::RootlistAccess(rootlist_access(0x09, true)));
        library.apply(&Update::PlaylistMoveFailed);

        assert_eq!(
            library.move_playlist(0, 2, "a"),
            Some(Command::MovePlaylist {
                revision: revision(0x07),
                from: 0,
                to: 2,
            }),
            "the access that arrived mid-write never became the held one"
        );
    }

    #[test]
    fn a_playlists_read_after_the_reply_is_adopted() {
        let mut library = sidebar_library(&["a", "b", "c"], 0x07, true, None);
        assert!(library.move_playlist(0, 2, "a").is_some());
        library.apply(&Update::PlaylistMoved {
            revision: revision(0x08),
        });

        library.apply(&Update::Playlists(vec![with_access(
            playlist("z", "z"),
            0x09,
            true,
            None,
        )]));

        assert_eq!(sidebar_order(&library), names(&["z"]));
    }

    #[test]
    fn a_playlists_read_clears_the_held_access_until_its_own_arrives() {
        let mut library = sidebar_library(&["a", "b"], 0x07, true, None);
        assert!(library.rootlist_movable());

        library.apply(&Update::Playlists(vec![
            with_access(playlist("a", "a"), 0x07, true, None),
            with_access(playlist("b", "b"), 0x07, true, None),
        ]));

        assert!(
            !library.rootlist_movable(),
            "the pair cannot straddle: the list waits for its own access"
        );

        library.apply(&Update::RootlistAccess(rootlist_access(0x09, true)));
        assert!(library.rootlist_movable());
    }

    #[test]
    fn an_adopted_read_clears_the_sidebar_notice() {
        let mut library = sidebar_library(&["a", "b", "c"], 0x07, true, None);
        assert!(library.move_playlist(0, 2, "a").is_some());
        library.apply(&Update::PlaylistMoveFailed);
        assert_eq!(library.sidebar_notice, Some(SidebarWrite::Reorder));

        library.apply(&Update::Playlists(vec![with_access(
            playlist("a", "a"),
            0x07,
            true,
            None,
        )]));

        assert_eq!(library.sidebar_notice, None);
    }

    #[test]
    fn a_new_gesture_clears_the_sidebar_notice_at_once() {
        let mut library = sidebar_library(&["a", "b", "c"], 0x07, true, Some(2));
        assert!(library.move_playlist(0, 2, "a").is_some());
        library.apply(&Update::PlaylistMoveFailed);
        assert_eq!(library.sidebar_notice, Some(SidebarWrite::Reorder));

        assert!(library.move_playlist(0, 2, "a").is_some());

        assert_eq!(library.sidebar_notice, None);
    }

    #[test]
    fn a_confirmed_write_after_a_failed_one_clears_the_sidebar_notice() {
        let mut library = sidebar_library(&["a", "b", "c"], 0x07, true, Some(2));
        assert!(library.move_playlist(0, 2, "a").is_some());
        library.apply(&Update::PlaylistMoveFailed);
        assert!(library.move_playlist(0, 2, "a").is_some());

        library.apply(&Update::PlaylistMoved {
            revision: revision(0x08),
        });

        assert_eq!(library.sidebar_notice, None);
    }

    #[test]
    fn an_add_asks_for_one_write_on_the_entrys_revision_and_length() {
        let mut library = sidebar_library(&["a", "b"], 0x07, true, Some(3));

        assert_eq!(
            library.add_tracks(1, &pick(&["spotify:track:x"])),
            Some(Command::AddTracks {
                playlist: "b".to_owned(),
                revision: revision(0x07),
                from_index: 3,
                tracks: pick(&["spotify:track:x"]),
            })
        );
    }

    #[test]
    fn an_add_names_the_pick_in_the_order_the_drag_took_it() {
        let mut library = sidebar_library(&["a", "b"], 0x07, true, Some(3));
        let tracks = pick(&["spotify:track:a", "spotify:track:b", "spotify:track:c"]);

        assert_eq!(
            library.add_tracks(1, &tracks),
            Some(Command::AddTracks {
                playlist: "b".to_owned(),
                revision: revision(0x07),
                from_index: 3,
                tracks: tracks.clone(),
            })
        );
    }

    #[test]
    fn an_add_of_three_counts_three() {
        let mut library = sidebar_library(&["a", "b"], 0x07, true, Some(3));
        let tracks = pick(&["spotify:track:a", "spotify:track:b", "spotify:track:c"]);
        assert!(library.add_tracks(1, &tracks).is_some());

        library.apply(&Update::TrackAdded {
            playlist: "b".to_owned(),
            revision: revision(0x08),
        });

        let entry = sidebar_entry(&library, "b").expect("still listed");
        assert_eq!(entry.tracks, Some(6), "three travelled, three were held");
    }

    #[test]
    fn a_failed_add_of_three_leaves_the_count_alone() {
        let mut library = sidebar_library(&["a", "b"], 0x07, true, Some(3));
        let tracks = pick(&["spotify:track:a", "spotify:track:b", "spotify:track:c"]);
        assert!(library.add_tracks(1, &tracks).is_some());

        library.apply(&Update::AddFailed {
            playlist: "b".to_owned(),
        });

        let entry = sidebar_entry(&library, "b").expect("still listed");
        assert_eq!(entry.tracks, Some(3));
        assert_eq!(library.sidebar_notice, Some(SidebarWrite::Add));
        assert!(library.addable(1), "the refusal is spent with the gesture");
    }

    #[test]
    fn an_add_changes_nothing_until_it_is_confirmed() {
        let mut library = sidebar_library(&["a", "b"], 0x07, true, Some(3));
        let before = sidebar_order(&library);

        assert!(library.add_tracks(1, &pick(&["spotify:track:x"])).is_some());

        assert_eq!(sidebar_order(&library), before);
        assert_eq!(
            sidebar_entry(&library, "b").and_then(|entry| entry.tracks),
            Some(3)
        );
        assert!(!library.addable(0), "the write is in flight");
    }

    #[test]
    fn a_confirmed_add_replaces_the_revision_and_counts_one_more() {
        let mut library = sidebar_library(&["a", "b"], 0x07, true, Some(3));
        assert!(library.add_tracks(1, &pick(&["spotify:track:x"])).is_some());

        library.apply(&Update::TrackAdded {
            playlist: "b".to_owned(),
            revision: revision(0x08),
        });

        let entry = sidebar_entry(&library, "b").expect("still listed");
        assert_eq!(entry.tracks, Some(4));
        assert_eq!(
            entry.access.map(|access| access.revision),
            Some(revision(0x08))
        );
        assert_eq!(library.sidebar_notice, None);
        assert!(library.addable(1), "the write is answered");
    }

    #[test]
    fn a_confirmed_add_drops_the_page_held_for_that_playlist() {
        // The add is made from an album page, so the playlist's own rows were
        // read before it and are still held behind it.
        let mut library = page_with("b", &["t0", "t1"], access(0x01, true));
        library.playlists = Load::Ready(vec![
            with_access(playlist("a", "a"), 0x07, true, Some(1)),
            with_access(playlist("b", "b"), 0x07, true, Some(2)),
        ]);
        library.apply(&Update::RootlistAccess(rootlist_access(0x07, true)));
        library.open(album("spotify:album:x"));
        assert!(library.add_tracks(1, &pick(&["spotify:track:x"])).is_some());

        library.apply(&Update::TrackAdded {
            playlist: "b".to_owned(),
            revision: revision(0x08),
        });

        assert_eq!(library.tracks, Load::Idle, "the page is one track short");
        assert_eq!(
            library.back(),
            Some(Command::LoadTracks {
                playlist: "b".to_owned()
            }),
            "Back asks for the page again"
        );
    }

    #[test]
    fn a_confirmed_add_keeps_the_page_held_for_another_playlist() {
        let mut library = page_with("a", &["t0", "t1"], access(0x01, true));
        library.playlists = Load::Ready(vec![
            with_access(playlist("a", "a"), 0x07, true, Some(1)),
            with_access(playlist("b", "b"), 0x07, true, Some(2)),
        ]);
        library.apply(&Update::RootlistAccess(rootlist_access(0x07, true)));
        library.open(album("spotify:album:x"));
        assert!(library.add_tracks(1, &pick(&["spotify:track:x"])).is_some());

        library.apply(&Update::TrackAdded {
            playlist: "b".to_owned(),
            revision: revision(0x08),
        });

        assert!(
            matches!(&library.tracks, Load::Ready(tracks) if tracks.playlist == "a"),
            "the add names another list"
        );
        assert_eq!(library.back(), None, "and that page is still ready");
    }

    #[test]
    fn the_next_add_builds_on_the_reply_and_the_new_count() {
        let mut library = sidebar_library(&["a", "b"], 0x07, true, Some(3));
        assert!(library.add_tracks(1, &pick(&["spotify:track:x"])).is_some());
        library.apply(&Update::TrackAdded {
            playlist: "b".to_owned(),
            revision: revision(0x08),
        });

        assert_eq!(
            library.add_tracks(1, &pick(&["spotify:track:y"])),
            Some(Command::AddTracks {
                playlist: "b".to_owned(),
                revision: revision(0x08),
                from_index: 4,
                tracks: pick(&["spotify:track:y"]),
            })
        );
    }

    #[test]
    fn a_failed_add_leaves_the_count_and_the_revision_and_says_so() {
        let mut library = sidebar_library(&["a", "b"], 0x07, true, Some(3));
        assert!(library.add_tracks(1, &pick(&["spotify:track:x"])).is_some());

        library.apply(&Update::AddFailed {
            playlist: "b".to_owned(),
        });

        let entry = sidebar_entry(&library, "b").expect("still listed");
        assert_eq!(entry.tracks, Some(3));
        assert_eq!(
            entry.access.map(|access| access.revision),
            Some(revision(0x07))
        );
        assert_eq!(library.sidebar_notice, Some(SidebarWrite::Add));
        assert!(library.addable(1));
    }

    #[test]
    fn a_list_that_can_not_be_edited_is_not_addable() {
        let library = sidebar_library(&["a", "b"], 0x07, false, Some(3));

        assert!(!library.addable(0));
        let mut library = library;
        assert_eq!(library.add_tracks(0, &pick(&["spotify:track:x"])), None);
    }

    #[test]
    fn a_playlist_with_no_revision_or_no_count_is_not_addable() {
        let mut library = Library::default();
        library.apply(&Update::Playlists(vec![
            with_access(playlist("a", "a"), 0x07, true, None),
            playlist("b", "b"),
        ]));
        library.apply(&Update::RootlistAccess(rootlist_access(0x07, true)));

        assert!(!library.addable(0), "no count");
        assert!(!library.addable(1), "no revision");
        assert_eq!(library.add_tracks(0, &pick(&["spotify:track:x"])), None);
    }

    #[test]
    fn the_open_playlist_is_not_addable() {
        let mut library = sidebar_library(&["a", "b"], 0x07, true, Some(3));
        library.select("a");
        assert!(!library.addable(0));
        assert!(library.addable(1));

        library.open(Page::Album("spotify:album:b".to_owned()));
        assert!(
            library.addable(0),
            "an album page leaves every playlist a target"
        );

        library.open(Page::Search("never gonna".to_owned()));
        assert!(library.addable(0));
    }

    #[test]
    fn an_empty_pick_is_not_a_write() {
        let mut library = sidebar_library(&["a", "b"], 0x07, true, Some(3));

        assert_eq!(library.add_tracks(0, &[]), None);
        assert!(library.addable(0), "a refused pick is no write at all");
    }

    #[test]
    fn an_add_waits_while_any_write_is_pending() {
        let mut library = page_with("a", &["t0", "t1"], access(0x01, true));
        library.playlists = Load::Ready(vec![
            with_access(playlist("p0", "p0"), 0x07, true, Some(1)),
            with_access(playlist("p1", "p1"), 0x07, true, Some(1)),
        ]);
        library.apply(&Update::RootlistAccess(rootlist_access(0x07, true)));
        assert!(move_one(&mut library, "a", 0, 2, &track("t0")).is_some());

        assert_eq!(library.add_tracks(1, &pick(&["spotify:track:x"])), None);

        let mut library = sidebar_library(&["a", "b"], 0x07, true, Some(1));
        assert!(library.move_playlist(0, 2, "a").is_some());
        assert_eq!(library.add_tracks(1, &pick(&["spotify:track:x"])), None);
    }

    #[test]
    fn the_add_targets_are_the_sidebars_order_without_the_open_page() {
        let mut library = sidebar_library(&["a", "b", "c"], 0x07, true, Some(2));
        library.select("b");

        let targets = library.add_targets();

        assert_eq!(
            targets
                .iter()
                .map(|row| row.uri.as_str())
                .collect::<Vec<_>>(),
            ["a", "c"],
            "the open page is no target, and the order is the sidebar's"
        );
        assert_eq!(targets[0].name, "a");
        assert_eq!(targets[0].revision, revision(0x07));
        assert_eq!(targets[0].length, 2);
    }

    #[test]
    fn an_add_target_needs_an_editable_list_with_a_revision_and_a_count() {
        let mut library = Library::default();
        library.apply(&Update::Playlists(vec![
            with_access(playlist("a", "a"), 0x07, true, Some(2)),
            with_access(playlist("b", "b"), 0x07, false, Some(2)),
            with_access(playlist("c", "c"), 0x07, true, None),
            playlist("d", "d"),
        ]));
        library.apply(&Update::RootlistAccess(rootlist_access(0x07, true)));

        assert_eq!(
            library
                .add_targets()
                .iter()
                .map(|row| row.uri.as_str())
                .collect::<Vec<_>>(),
            ["a"],
            "a foreign list is absent, never greyed"
        );
    }

    #[test]
    fn the_add_targets_are_empty_while_a_write_is_pending() {
        let mut library = sidebar_library(&["a", "b"], 0x07, true, Some(2));
        assert!(library.add_tracks(1, &pick(&["spotify:track:x"])).is_some());
        assert!(library.add_targets().is_empty(), "an add is in flight");

        let mut creating = sidebar_library(&["a", "b"], 0x07, true, Some(2));
        creating.naming.open();
        creating.naming.edit().push_str("Mix");
        assert!(creating.creatable());
        assert!(creating.create_playlist().is_some());
        assert!(creating.add_targets().is_empty(), "a create is in flight");
    }

    #[test]
    fn an_add_pick_names_the_held_list() {
        let mut library = sidebar_library(&["a", "b"], 0x07, true, Some(3));
        let tracks = pick(&["spotify:track:x", "spotify:track:y"]);

        assert_eq!(
            library.add_pick("b", &tracks),
            Some(Command::AddTracks {
                playlist: "b".to_owned(),
                revision: revision(0x07),
                from_index: 3,
                tracks: tracks.clone(),
            })
        );
    }

    #[test]
    fn an_add_pick_refuses_an_unknown_uri_the_open_page_and_a_pending_write() {
        let mut library = sidebar_library(&["a", "b"], 0x07, true, Some(3));
        let tracks = pick(&["spotify:track:x"]);

        assert_eq!(library.add_pick("missing", &tracks), None);
        assert!(library.addable(1), "an unknown URI is no write at all");

        library.select("b");
        assert_eq!(library.add_pick("b", &tracks), None, "the open page");
        library.open(Page::Search("query".to_owned()));

        assert!(library.add_pick("b", &tracks).is_some());
        assert_eq!(library.add_pick("a", &tracks), None, "a write is pending");
    }

    #[test]
    fn a_stale_reply_with_nothing_pending_is_ignored() {
        let mut library = sidebar_library(&["a", "b", "c"], 0x07, true, Some(2));
        library.apply(&Update::PlaylistMoved {
            revision: revision(0x08),
        });
        library.apply(&Update::PlaylistMoveFailed);
        library.apply(&Update::TrackAdded {
            playlist: "a".to_owned(),
            revision: revision(0x08),
        });
        library.apply(&Update::AddFailed {
            playlist: "a".to_owned(),
        });

        assert_eq!(sidebar_order(&library), names(&["a", "b", "c"]));
        assert_eq!(
            sidebar_entry(&library, "a").and_then(|entry| entry.tracks),
            Some(2)
        );
        assert_eq!(library.sidebar_notice, None);
        assert!(library.rootlist_movable());
    }

    #[test]
    fn a_reply_for_another_playlist_does_not_end_the_write() {
        let mut library = sidebar_library(&["a", "b"], 0x07, true, Some(2));
        assert!(library.add_tracks(1, &pick(&["spotify:track:x"])).is_some());

        library.apply(&Update::TrackAdded {
            playlist: "a".to_owned(),
            revision: revision(0x08),
        });
        library.apply(&Update::AddFailed {
            playlist: "a".to_owned(),
        });

        assert!(!library.addable(0), "the write for b is still in flight");
    }

    #[test]
    fn an_access_that_arrives_mid_add_waits_for_a_fresh_read() {
        let mut library = sidebar_library(&["a", "b"], 0x07, true, Some(3));
        // A read that lands just before the add drops the held access with the
        // order it brought, so the column is only movable again once its own
        // access arrives.
        library.apply(&Update::Playlists(vec![
            with_access(playlist("a", "a"), 0x07, true, Some(3)),
            with_access(playlist("b", "b"), 0x07, true, Some(3)),
        ]));
        assert!(!library.rootlist_movable(), "no access yet");
        assert!(library.add_tracks(1, &pick(&["spotify:track:x"])).is_some());

        library.apply(&Update::RootlistAccess(rootlist_access(0x09, true)));

        assert!(
            library.rootlist.is_none(),
            "an access adopted while a write is pending names a revision that write may replace"
        );
        assert_eq!(
            library.rootlist_request(),
            None,
            "a read asked for while the write it waits on is still in flight"
        );

        library.apply(&Update::TrackAdded {
            playlist: "b".to_owned(),
            revision: revision(0x08),
        });

        assert_eq!(
            library.rootlist_request(),
            Some(Command::ReloadLibrary),
            "the refused access asks for a read that can be adopted"
        );
        assert_eq!(library.rootlist_request(), None, "a second read");
    }

    #[test]
    fn an_access_whose_read_was_refused_is_refused_with_it() {
        let mut library = sidebar_library(&["a", "b"], 0x07, true, Some(3));
        // A read that drops the held access waits for its own to arrive.
        library.apply(&Update::Playlists(vec![
            with_access(playlist("a", "a"), 0x07, true, Some(3)),
            with_access(playlist("b", "b"), 0x07, true, Some(3)),
        ]));
        assert!(library.add_tracks(1, &pick(&["spotify:track:x"])).is_some());

        library.apply(&Update::Playlists(vec![
            with_access(playlist("b", "b"), 0x0a, true, Some(3)),
            with_access(playlist("a", "a"), 0x0a, true, Some(3)),
        ]));
        library.apply(&Update::RootlistAccess(rootlist_access(0x0a, true)));
        library.apply(&Update::TrackAdded {
            playlist: "b".to_owned(),
            revision: revision(0x08),
        });

        assert_eq!(
            sidebar_order(&library),
            names(&["a", "b"]),
            "the read's order was refused"
        );
        assert!(
            !library.rootlist_movable(),
            "and its revision is no use beside this one"
        );
    }

    #[test]
    fn a_read_while_an_add_is_pending_is_not_adopted() {
        let mut library = sidebar_library(&["a", "b"], 0x07, true, Some(3));
        assert!(library.add_tracks(1, &pick(&["spotify:track:x"])).is_some());

        library.apply(&Update::Playlists(vec![
            with_access(playlist("a", "a"), 0x07, true, Some(3)),
            with_access(playlist("b", "b"), 0x08, true, Some(4)),
        ]));

        assert_eq!(
            sidebar_entry(&library, "b").and_then(|entry| entry.tracks),
            Some(3),
            "a read taken after the server applied the add would count twice"
        );
        library.apply(&Update::TrackAdded {
            playlist: "b".to_owned(),
            revision: revision(0x09),
        });
        assert_eq!(
            sidebar_entry(&library, "b").and_then(|entry| entry.tracks),
            Some(4),
            "the confirmation counts from the count the drop named"
        );
    }

    #[test]
    fn a_page_move_reply_updates_the_entrys_revision() {
        let mut library = page_with("a", &["t0", "t1"], access(0x01, true));
        library.apply(&Update::Playlists(vec![with_access(
            playlist("a", "a"),
            0x01,
            true,
            Some(2),
        )]));
        library.apply(&Update::RootlistAccess(rootlist_access(0x07, true)));
        assert!(move_one(&mut library, "a", 0, 2, &track("t0")).is_some());

        library.apply(&Update::TrackMoved {
            playlist: "a".to_owned(),
            revision: revision(0x02),
        });

        library.open(Page::Album("spotify:album:b".to_owned()));
        assert_eq!(
            library.add_tracks(0, &pick(&["spotify:track:x"])),
            Some(Command::AddTracks {
                playlist: "a".to_owned(),
                revision: revision(0x02),
                from_index: 2,
                tracks: pick(&["spotify:track:x"]),
            }),
            "the add names the revision the move's reply named"
        );
    }

    /// A one-row drag, with the write's landed range reduced to its start.
    fn move_one(
        library: &mut Library,
        playlist: &str,
        from: usize,
        boundary: usize,
        carried: &TrackEntry,
    ) -> Option<(Command, usize)> {
        library
            .move_tracks(playlist, &[from], boundary, std::slice::from_ref(carried))
            .map(|(command, landed)| (command, landed.start))
    }

    fn moved(uris: &[&str]) -> Vec<TrackEntry> {
        uris.iter().map(|uri| track(uri)).collect()
    }

    fn ready(playlist: &str, uris: &[&str], access: Option<PlaylistAccess>) -> Load<TracksFor> {
        Load::Ready(TracksFor {
            playlist: playlist.to_owned(),
            tracks: moved(uris),
            access,
            notice: None,
        })
    }

    #[test]
    fn a_move_applies_at_once_and_asks_for_one_write_on_the_held_revision() {
        let mut library = page_with("a", &["t0", "t1", "t2"], access(0x01, true));

        let found = move_one(&mut library, "a", 0, 2, &track("t0"));

        assert_eq!(
            found,
            Some((
                Command::MoveTrack {
                    playlist: "a".to_owned(),
                    revision: revision(0x01),
                    moves: vec![Move {
                        from: 0,
                        length: 1,
                        to: 2,
                    }],
                },
                1,
            ))
        );
        assert_eq!(
            library.tracks,
            ready("a", &["t1", "t0", "t2"], access(0x01, true)),
            "the dragged order is on screen before the write is answered"
        );
        assert_eq!(
            library.pending,
            Some(reorder::Pending {
                playlist: "a".to_owned(),
                before: moved(&["t0", "t1", "t2"]),
                after: moved(&["t1", "t0", "t2"]),
            })
        );
    }

    #[test]
    fn a_move_selects_nothing_new_and_returns_where_the_row_landed() {
        let mut library = page_with("a", &["t0", "t1", "t2"], access(0x01, true));
        let page = library.page().cloned();

        let found = move_one(&mut library, "a", 2, 0, &track("t2"));

        assert_eq!(found.map(|(_, landed)| landed), Some(0));
        assert_eq!(library.page(), page.as_ref());
        assert_eq!(library.selected(), Some("a"));
    }

    #[test]
    fn a_read_only_page_refuses_a_move() {
        let mut library = page_with("a", &["t0", "t1"], access(0x01, false));

        assert!(!library.movable());
        assert_eq!(move_one(&mut library, "a", 0, 2, &track("t0")), None);
        assert_eq!(library.pending, None);
        assert_eq!(
            library.tracks,
            ready("a", &["t0", "t1"], access(0x01, false))
        );
    }

    #[test]
    fn a_page_with_no_access_refuses_a_move() {
        let mut library = page_with("a", &["t0", "t1"], None);

        assert!(!library.movable());
        assert_eq!(move_one(&mut library, "a", 0, 2, &track("t0")), None);
        assert_eq!(library.pending, None);
    }

    #[test]
    fn a_second_move_waits_for_the_first() {
        let mut library = page_with("a", &["t0", "t1", "t2"], access(0x01, true));
        assert!(move_one(&mut library, "a", 0, 2, &track("t0")).is_some());

        assert_eq!(
            move_one(&mut library, "a", 0, 2, &track("t1")),
            None,
            "one gesture is one write, so a second waits for the reply"
        );
        assert!(!library.movable());
    }

    #[test]
    fn a_move_whose_row_changed_under_the_drag_is_refused() {
        let mut library = page_with("a", &["t0", "t1"], access(0x01, true));

        assert_eq!(move_one(&mut library, "a", 0, 2, &track("gone")), None);
        assert_eq!(
            move_one(&mut library, "a", 9, 2, &track("t0")),
            None,
            "a row the list does not hold"
        );
        assert_eq!(library.pending, None);
    }

    #[test]
    fn a_block_move_applies_at_once_and_asks_for_one_write() {
        let mut library = page_with("a", &["t0", "t1", "t2", "t3"], access(0x01, true));

        let found = library.move_tracks("a", &[0, 2], 4, &moved(&["t0", "t2"]));

        assert_eq!(
            found,
            Some((
                Command::MoveTrack {
                    playlist: "a".to_owned(),
                    revision: revision(0x01),
                    moves: vec![
                        Move {
                            from: 0,
                            length: 1,
                            to: 4,
                        },
                        Move {
                            from: 1,
                            length: 1,
                            to: 4,
                        },
                    ],
                },
                2..4,
            )),
            "one command carries the whole chain"
        );
        assert_eq!(
            library.tracks,
            ready("a", &["t1", "t3", "t0", "t2"], access(0x01, true)),
            "the gathered block is on screen before the write is answered"
        );
        assert_eq!(
            library.pending,
            Some(reorder::Pending {
                playlist: "a".to_owned(),
                before: moved(&["t0", "t1", "t2", "t3"]),
                after: moved(&["t1", "t3", "t0", "t2"]),
            })
        );
    }

    #[test]
    fn a_block_whose_rows_changed_under_the_drag_is_refused() {
        let mut library = page_with("a", &["t0", "t1", "t2", "t3"], access(0x01, true));

        assert_eq!(
            library.move_tracks("a", &[0, 2], 4, &moved(&["t0", "gone"])),
            None,
            "a row is no longer the one the drag carried"
        );
        assert_eq!(
            library.move_tracks("a", &[0, 9], 4, &moved(&["t0", "gone"])),
            None,
            "a row the list does not hold"
        );
        assert_eq!(
            library.move_tracks("a", &[0, 2], 4, &moved(&["t0"])),
            None,
            "the entries no longer answer for the rows"
        );
        assert_eq!(library.pending, None);
    }

    #[test]
    fn a_block_that_is_not_ascending_is_refused() {
        let mut library = page_with("a", &["t0", "t1", "t2", "t3"], access(0x01, true));

        assert_eq!(
            library.move_tracks("a", &[2, 0], 4, &moved(&["t2", "t0"])),
            None
        );
        assert_eq!(
            library.move_tracks("a", &[0, 0], 4, &moved(&["t0", "t0"])),
            None,
            "one row is picked once"
        );
        assert_eq!(
            library.move_tracks("a", &[], 4, &[]),
            None,
            "nothing carried is nothing to move"
        );
        assert_eq!(library.pending, None);
    }

    #[test]
    fn a_read_only_page_refuses_a_block() {
        let mut library = page_with("a", &["t0", "t1", "t2"], access(0x01, false));

        assert_eq!(
            library.move_tracks("a", &[0, 2], 3, &moved(&["t0", "t2"])),
            None
        );
        assert_eq!(library.pending, None);
        assert_eq!(
            library.tracks,
            ready("a", &["t0", "t1", "t2"], access(0x01, false))
        );
    }

    #[test]
    fn a_block_that_would_not_move_writes_nothing() {
        let mut library = page_with("a", &["t0", "t1", "t2"], access(0x01, true));

        for boundary in 0..=2 {
            assert_eq!(
                library.move_tracks("a", &[0, 1], boundary, &moved(&["t0", "t1"])),
                None,
                "a contiguous block dropped on or inside itself at {boundary}"
            );
        }
        assert_eq!(library.pending, None);
        assert_eq!(
            library.tracks,
            ready("a", &["t0", "t1", "t2"], access(0x01, true))
        );
    }

    #[test]
    fn a_second_block_waits_for_the_first() {
        let mut library = page_with("a", &["t0", "t1", "t2", "t3"], access(0x01, true));
        assert!(
            library
                .move_tracks("a", &[0, 2], 4, &moved(&["t0", "t2"]))
                .is_some()
        );

        assert_eq!(
            library.move_tracks("a", &[0, 1], 4, &moved(&["t1", "t3"])),
            None,
            "one gesture is one write, so a second block waits for the reply"
        );
        assert!(!library.movable());
    }

    #[test]
    fn a_failed_block_restores_the_order_and_says_so() {
        let mut library = page_with("a", &["t0", "t1", "t2", "t3"], access(0x01, true));
        assert!(
            library
                .move_tracks("a", &[0, 2], 4, &moved(&["t0", "t2"]))
                .is_some()
        );

        library.apply(&Update::MoveFailed {
            playlist: "a".to_owned(),
            failure: Failure::Save,
        });

        assert_eq!(library.pending, None);
        assert_eq!(
            library.tracks,
            Load::Ready(TracksFor {
                playlist: "a".to_owned(),
                tracks: moved(&["t0", "t1", "t2", "t3"]),
                access: access(0x01, true),
                notice: Some(Failure::Save),
            })
        );
    }

    #[test]
    fn a_move_for_a_playlist_not_on_screen_is_refused() {
        let mut library = page_with("a", &["t0", "t1"], access(0x01, true));

        assert_eq!(move_one(&mut library, "b", 0, 2, &track("t0")), None);

        library.to_playlists();
        assert_eq!(
            move_one(&mut library, "a", 0, 2, &track("t0")),
            None,
            "the ready rows are kept for Back, not for a drop"
        );
        assert_eq!(library.pending, None);
    }

    #[test]
    fn a_drop_beside_itself_sends_nothing() {
        let mut library = page_with("a", &["t0", "t1", "t2"], access(0x01, true));

        assert_eq!(move_one(&mut library, "a", 1, 1, &track("t1")), None);
        assert_eq!(move_one(&mut library, "a", 1, 2, &track("t1")), None);
        assert_eq!(library.pending, None);
        assert_eq!(
            library.tracks,
            ready("a", &["t0", "t1", "t2"], access(0x01, true))
        );
    }

    #[test]
    fn the_reply_revision_replaces_the_held_one_and_ends_the_write() {
        let mut library = page_with("a", &["t0", "t1"], access(0x01, true));
        assert!(move_one(&mut library, "a", 0, 2, &track("t0")).is_some());

        library.apply(&Update::TrackMoved {
            playlist: "a".to_owned(),
            revision: revision(0x02),
        });

        assert_eq!(library.pending, None);
        assert_eq!(
            library.tracks,
            ready("a", &["t1", "t0"], access(0x02, true))
        );
    }

    #[test]
    fn the_next_move_is_based_on_the_reply_revision() {
        let mut library = page_with("a", &["t0", "t1", "t2"], access(0x01, true));
        assert!(move_one(&mut library, "a", 0, 2, &track("t0")).is_some());
        library.apply(&Update::TrackMoved {
            playlist: "a".to_owned(),
            revision: revision(0x02),
        });

        let found = move_one(&mut library, "a", 2, 0, &track("t2"));

        assert_eq!(
            found.map(|(command, _)| command),
            Some(Command::MoveTrack {
                playlist: "a".to_owned(),
                revision: revision(0x02),
                moves: vec![Move {
                    from: 2,
                    length: 1,
                    to: 0,
                }],
            })
        );
    }

    #[test]
    fn a_read_while_a_write_is_pending_is_not_adopted_even_with_an_equal_revision() {
        let mut library = page_with("a", &["t0", "t1"], access(0x01, true));
        assert!(move_one(&mut library, "a", 0, 2, &track("t0")).is_some());

        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: moved(&["t0", "t1"]),
            cover: None,
            access: access(0x01, true),
        });

        assert_eq!(
            library.tracks,
            ready("a", &["t1", "t0"], access(0x01, true)),
            "the server echoes the old order under the new revision"
        );
        assert!(library.pending.is_some(), "the write is still in flight");
    }

    #[test]
    fn a_read_while_a_write_is_pending_keeps_the_held_revision() {
        let mut library = page_with("a", &["t0", "t1"], access(0x01, true));
        assert!(move_one(&mut library, "a", 0, 2, &track("t0")).is_some());

        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: moved(&["t0", "t1"]),
            cover: None,
            access: access(0x02, true),
        });

        assert_eq!(
            library.tracks,
            ready("a", &["t1", "t0"], access(0x01, true)),
            "the held access is the one the write was sent on"
        );
    }

    #[test]
    fn a_read_onto_a_loading_page_while_a_write_is_pending_draws_the_moved_order() {
        let mut library = page_with("a", &["t0", "t1"], access(0x01, true));
        assert!(move_one(&mut library, "a", 0, 2, &track("t0")).is_some());
        library.select("a");
        assert_eq!(library.tracks, Load::Loading);

        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: moved(&["t0", "t1"]),
            cover: None,
            access: access(0x01, true),
        });

        assert_eq!(
            library.tracks,
            ready("a", &["t1", "t0"], access(0x01, true)),
            "the reload does not undo the drag it was asked for before"
        );
    }

    #[test]
    fn a_read_with_nothing_pending_is_adopted_even_with_an_equal_revision() {
        let mut library = page_with("a", &["t0", "t1"], access(0x01, true));

        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: moved(&["t1", "t0"]),
            cover: None,
            access: access(0x01, true),
        });

        assert_eq!(
            library.tracks,
            ready("a", &["t1", "t0"], access(0x01, true))
        );
    }

    #[test]
    fn a_read_after_the_reply_is_adopted() {
        let mut library = page_with("a", &["t0", "t1"], access(0x01, true));
        assert!(move_one(&mut library, "a", 0, 2, &track("t0")).is_some());
        library.apply(&Update::TrackMoved {
            playlist: "a".to_owned(),
            revision: revision(0x02),
        });

        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: moved(&["t0", "t1"]),
            cover: None,
            access: access(0x02, true),
        });

        assert_eq!(
            library.tracks,
            ready("a", &["t0", "t1"], access(0x02, true)),
            "nothing is pending, so every read is adopted"
        );
    }

    #[test]
    fn a_failed_write_reverts_to_the_order_before_it_and_says_save() {
        let mut library = page_with("a", &["t0", "t1"], access(0x01, true));
        assert!(move_one(&mut library, "a", 0, 2, &track("t0")).is_some());

        library.apply(&Update::MoveFailed {
            playlist: "a".to_owned(),
            failure: Failure::Save,
        });

        assert_eq!(library.pending, None);
        assert_eq!(
            library.tracks,
            Load::Ready(TracksFor {
                playlist: "a".to_owned(),
                tracks: moved(&["t0", "t1"]),
                access: access(0x01, true),
                notice: Some(Failure::Save),
            })
        );
    }

    #[test]
    fn a_failed_write_for_a_page_no_longer_shown_only_ends_the_write() {
        let mut library = page_with("a", &["t0", "t1"], access(0x01, true));
        assert!(move_one(&mut library, "a", 0, 2, &track("t0")).is_some());
        library.to_playlists();

        library.apply(&Update::MoveFailed {
            playlist: "a".to_owned(),
            failure: Failure::Save,
        });

        assert_eq!(library.pending, None);
        assert_eq!(
            library.tracks,
            Load::Ready(TracksFor {
                playlist: "a".to_owned(),
                tracks: moved(&["t1", "t0"]),
                access: access(0x01, true),
                notice: None,
            }),
            "nothing is on screen to put back, and Back reloads the list"
        );
    }

    #[test]
    fn an_adopted_read_clears_the_notice() {
        let mut library = page_with("a", &["t0", "t1"], access(0x01, true));
        assert!(move_one(&mut library, "a", 0, 2, &track("t0")).is_some());
        library.apply(&Update::MoveFailed {
            playlist: "a".to_owned(),
            failure: Failure::Save,
        });

        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: moved(&["t0", "t1"]),
            cover: None,
            access: access(0x01, true),
        });

        assert_eq!(
            library.tracks,
            ready("a", &["t0", "t1"], access(0x01, true))
        );
    }

    #[test]
    fn a_new_gesture_clears_the_notice_at_once() {
        let mut library = page_with("a", &["t0", "t1"], access(0x01, true));
        assert!(move_one(&mut library, "a", 0, 2, &track("t0")).is_some());
        library.apply(&Update::MoveFailed {
            playlist: "a".to_owned(),
            failure: Failure::Save,
        });

        assert!(move_one(&mut library, "a", 0, 2, &track("t0")).is_some());

        assert_eq!(
            library.tracks,
            ready("a", &["t1", "t0"], access(0x01, true)),
            "the sentence goes when the user tries again, not when the reply lands"
        );
    }

    #[test]
    fn a_successful_write_after_a_failed_one_clears_the_notice() {
        let mut library = page_with("a", &["t0", "t1"], access(0x01, true));
        assert!(move_one(&mut library, "a", 0, 2, &track("t0")).is_some());
        library.apply(&Update::MoveFailed {
            playlist: "a".to_owned(),
            failure: Failure::Save,
        });
        assert!(move_one(&mut library, "a", 0, 2, &track("t0")).is_some());

        library.apply(&Update::TrackMoved {
            playlist: "a".to_owned(),
            revision: revision(0x02),
        });

        assert_eq!(
            library.tracks,
            ready("a", &["t1", "t0"], access(0x02, true)),
            "a saved order must not still claim it could not be saved"
        );
    }

    #[test]
    fn a_reply_for_another_playlist_does_not_touch_the_page() {
        let mut library = page_with("a", &["t0", "t1"], access(0x01, true));
        assert!(move_one(&mut library, "a", 0, 2, &track("t0")).is_some());

        library.apply(&Update::TrackMoved {
            playlist: "b".to_owned(),
            revision: revision(0x09),
        });
        library.apply(&Update::MoveFailed {
            playlist: "b".to_owned(),
            failure: Failure::Save,
        });

        assert!(
            library.pending.is_some(),
            "the write for a is still in flight"
        );
        assert_eq!(
            library.tracks,
            ready("a", &["t1", "t0"], access(0x01, true))
        );
    }

    #[test]
    fn a_reply_while_the_page_is_loading_only_ends_the_write() {
        let mut library = page_with("a", &["t0", "t1"], access(0x01, true));
        assert!(move_one(&mut library, "a", 0, 2, &track("t0")).is_some());
        library.select("a");

        library.apply(&Update::TrackMoved {
            playlist: "a".to_owned(),
            revision: revision(0x02),
        });

        assert_eq!(library.pending, None);
        assert_eq!(library.tracks, Load::Loading);
    }

    #[test]
    fn a_reply_while_the_page_is_off_screen_only_ends_the_write() {
        let mut library = page_with("a", &["t0", "t1"], access(0x01, true));
        assert!(move_one(&mut library, "a", 0, 2, &track("t0")).is_some());
        library.to_playlists();

        library.apply(&Update::TrackMoved {
            playlist: "a".to_owned(),
            revision: revision(0x02),
        });

        assert_eq!(library.pending, None);
        assert_eq!(
            library.tracks,
            ready("a", &["t1", "t0"], access(0x02, true)),
            "the rows are kept for Back, which reloads them anyway"
        );
    }

    #[test]
    fn a_failed_write_while_the_page_is_loading_only_ends_the_write() {
        let mut library = page_with("a", &["t0", "t1"], access(0x01, true));
        assert!(move_one(&mut library, "a", 0, 2, &track("t0")).is_some());
        library.select("a");

        library.apply(&Update::MoveFailed {
            playlist: "a".to_owned(),
            failure: Failure::Save,
        });

        assert_eq!(library.pending, None);
        assert_eq!(library.tracks, Load::Loading);
    }

    #[test]
    fn a_tracks_failure_while_a_write_is_pending_keeps_the_write() {
        let mut library = page_with("a", &["t0", "t1"], access(0x01, true));
        assert!(move_one(&mut library, "a", 0, 2, &track("t0")).is_some());

        library.apply(&Update::TracksFailed {
            playlist: "a".to_owned(),
            failure: Failure::Load,
        });

        assert_eq!(library.tracks, Load::Failed(Failure::Load));
        assert!(
            library.pending.is_some(),
            "only the write's own reply ends it"
        );
    }

    #[test]
    fn movable_needs_an_editable_ready_page_and_no_write_in_flight() {
        assert!(!Library::default().movable(), "nothing is loaded");
        assert!(!page_with("a", &["t0"], None).movable(), "no access");
        assert!(!page_with("a", &["t0"], access(0x01, false)).movable());

        let mut library = page_with("a", &["t0", "t1"], access(0x01, true));
        assert!(library.movable());

        assert!(move_one(&mut library, "a", 0, 2, &track("t0")).is_some());
        assert!(!library.movable(), "a write is in flight");
    }

    #[test]
    fn reload_marks_the_playlists_loading_and_returns_the_command() {
        let mut library = Library::default();
        library.apply(&Update::Playlists(vec![playlist("a", "A")]));

        assert_eq!(library.reload(), Command::ReloadLibrary);
        assert_eq!(library.playlists, Load::Loading);
    }

    #[test]
    fn playlists_keep_the_tracks_when_the_selection_survives() {
        let mut library = Library::default();
        library.apply(&Update::Playlists(vec![playlist("a", "A")]));
        library.select("a");
        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: vec![track("t")],
            cover: None,
            access: None,
        });

        library.apply(&Update::Playlists(vec![
            playlist("a", "A"),
            playlist("b", "B"),
        ]));

        assert_eq!(
            library.tracks,
            Load::Ready(tracks_for("a", vec![track("t")]))
        );
        assert_eq!(library.selected(), Some("a"));
    }

    #[test]
    fn playlists_clear_the_tracks_when_the_selection_is_gone() {
        let mut library = Library::default();
        library.apply(&Update::Playlists(vec![playlist("a", "A")]));
        library.select("a");
        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: vec![track("t")],
            cover: None,
            access: None,
        });

        library.apply(&Update::Playlists(vec![playlist("b", "B")]));

        assert_eq!(library.tracks, Load::Idle);
        assert_eq!(library.selected(), None);
    }

    #[test]
    fn a_filter_is_cleared_when_the_page_changes() {
        let mut library = Library::default();
        library.select("a");
        library.track_filter.edit("a").push_str("midnight");

        library.select("b");
        assert!(
            library.track_filter.edit("a").is_empty(),
            "a new page clears the query"
        );

        library.track_filter.edit("b").push_str("midnight");
        library.back();
        assert!(
            library.track_filter.edit("b").is_empty(),
            "back clears the query"
        );

        library.track_filter.edit("b").push_str("midnight");
        library.forward();
        assert!(
            library.track_filter.edit("b").is_empty(),
            "forward clears the query"
        );

        library.track_filter.edit("b").push_str("midnight");
        library.to_playlists();
        assert!(
            library.track_filter.edit("b").is_empty(),
            "going home clears the query"
        );
    }

    #[test]
    fn a_page_change_keeps_the_rail_query() {
        let mut library = Library::default();
        library.apply(&Update::Playlists(vec![
            playlist("a", "A"),
            playlist("b", "B"),
        ]));
        library.select("a");
        library.rail_filter.edit().push_str("midnight");

        library.select("b");
        assert_eq!(
            library.rail_filter.query(),
            "midnight",
            "a new playlist keeps the rail's query"
        );

        library.open(Page::Album("spotify:album:a".to_owned()));
        assert_eq!(library.rail_filter.query(), "midnight", "a page keeps it");

        library.back();
        assert_eq!(library.rail_filter.query(), "midnight", "back keeps it");

        library.forward();
        assert_eq!(library.rail_filter.query(), "midnight", "forward keeps it");

        library.to_playlists();
        assert_eq!(
            library.rail_filter.query(),
            "midnight",
            "going home keeps it"
        );
    }

    #[test]
    fn a_playlists_reply_keeps_the_rail_query() {
        let mut library = Library::default();
        library.apply(&Update::Playlists(vec![playlist("a", "A")]));
        library.rail_filter.edit().push_str("midnight");

        library.apply(&Update::Playlists(vec![playlist("a", "A")]));

        assert_eq!(
            library.rail_filter.query(),
            "midnight",
            "a reload of any library list keeps the query"
        );
    }

    #[test]
    fn choosing_a_kind_keeps_the_rail_query() {
        let mut library = Library::default();
        library.rail_filter.edit().push_str("midnight");

        assert!(library.choose_kind(SidebarKind::Artists));

        assert_eq!(
            library.rail_filter.query(),
            "midnight",
            "the two axes are cumulative, so a chip keeps the query"
        );
    }

    #[test]
    fn a_page_change_still_clears_the_track_filter() {
        let mut library = Library::default();
        library.select("a");
        library.track_filter.edit("a").push_str("midnight");
        library.rail_filter.edit().push_str("midnight");

        library.select("b");

        assert!(
            library.track_filter.edit("a").is_empty(),
            "the page's query still goes with the page"
        );
        assert_eq!(
            library.rail_filter.query(),
            "midnight",
            "the rail's query is not the page's"
        );
    }

    #[test]
    fn reselecting_the_same_playlist_keeps_the_filter() {
        let mut library = Library::default();
        library.select("a");
        library.track_filter.edit("a").push_str("midnight");

        library.select("a");

        assert_eq!(
            library.track_filter.needle("a").as_deref(),
            Some("midnight"),
            "a reload of the same page is not a page change"
        );
    }

    #[test]
    fn a_tracks_reply_for_the_same_playlist_keeps_the_filter() {
        let mut library = Library::default();
        library.select("a");
        library.track_filter.edit("a").push_str("midnight");

        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: vec![track("t")],
            cover: None,
            access: None,
        });

        assert_eq!(
            library.track_filter.needle("a").as_deref(),
            Some("midnight")
        );
    }

    #[test]
    fn a_filter_does_not_change_movable_or_removable() {
        let mut library = page_with("a", &["t0", "t1"], access(0x01, true));
        assert!(library.movable() && library.removable());

        library.track_filter.edit("a").push_str("midnight");

        assert!(
            library.movable(),
            "the query is the view's gate, not this one"
        );
        assert!(library.removable());
    }

    #[test]
    fn to_playlists_clears_the_selection_but_keeps_the_tracks() {
        let mut library = Library::default();
        library.select("a");
        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: vec![track("t")],
            cover: None,
            access: None,
        });

        library.to_playlists();

        assert_eq!(library.selected(), None);
        assert_eq!(library.page(), None);
        assert!(library.can_back());
        assert_eq!(
            library.tracks,
            Load::Ready(tracks_for("a", vec![track("t")]))
        );
        assert_eq!(library.wanted, None, "the stale-reply guard is cleared");
    }

    #[test]
    fn a_reply_after_to_playlists_is_discarded() {
        let mut library = Library::default();
        library.select("a");
        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: vec![track("t0")],
            cover: None,
            access: None,
        });
        library.to_playlists();
        let settled = Load::Ready(tracks_for("a", vec![track("t0")]));

        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: vec![track("t1")],
            cover: None,
            access: None,
        });
        assert_eq!(library.tracks, settled, "the late tracks are discarded");

        library.apply(&Update::TracksFailed {
            playlist: "a".to_owned(),
            failure: Failure::Load,
        });
        assert_eq!(library.tracks, settled, "the late failure is discarded");

        assert_eq!(
            library.back(),
            Some(Command::LoadTracks {
                playlist: "a".to_owned()
            }),
            "going back into the playlist reloads it"
        );
    }

    #[test]
    fn the_selected_name_comes_from_a_ready_list() {
        let mut library = Library::default();
        library.apply(&Update::Playlists(vec![playlist("a", "A")]));
        library.select("a");
        assert_eq!(library.selected_name(), Some("A"));
    }

    #[test]
    fn the_selected_name_is_none_while_the_list_reloads() {
        let mut library = Library::default();
        library.apply(&Update::Playlists(vec![playlist("a", "A")]));
        library.select("a");
        library.reload();
        assert_eq!(library.selected(), Some("a"));
        assert_eq!(library.selected_name(), None);
    }

    #[test]
    fn transport_updates_are_ignored() {
        let mut library = Library::default();
        library.apply(&Update::Transport(TransportUpdate::Ended));
        assert_eq!(library.playlists, Load::Idle);
        assert_eq!(library.tracks, Load::Idle);
    }

    #[test]
    fn a_tracks_cover_upgrades_an_entry_that_already_had_a_cover() {
        let first = cover_url("first");
        let context = cover_url("context");
        let mut library = Library::default();
        library.apply(&Update::Playlists(vec![playlist_with_cover(
            "a", "A", first,
        )]));
        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: vec![track("t")],
            cover: Some(context.clone()),
            access: None,
        });

        assert_eq!(
            library.playlists,
            Load::Ready(vec![playlist_with_cover("a", "A", context)])
        );
    }

    #[test]
    fn a_non_awaited_none_cover_leaves_the_entry_alone() {
        let first = cover_url("first");
        let mut library = Library::default();
        library.apply(&Update::Playlists(vec![playlist_with_cover(
            "a",
            "A",
            first.clone(),
        )]));
        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: vec![track("t")],
            cover: None,
            access: None,
        });

        assert_eq!(
            library.playlists,
            Load::Ready(vec![playlist_with_cover("a", "A", first)]),
            "a read that answers no ask cannot say why a cover is missing, so it clears none"
        );
    }

    #[test]
    fn a_tracks_cover_upgrades_the_update_playlist_not_the_selected_one() {
        let a_first = cover_url("a-first");
        let a_context = cover_url("a-context");
        let b_cover = cover_url("b");
        let mut library = Library::default();
        library.apply(&Update::Playlists(vec![
            playlist_with_cover("a", "A", a_first),
            playlist_with_cover("b", "B", b_cover.clone()),
        ]));
        library.select("b");
        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: vec![track("t")],
            cover: Some(a_context.clone()),
            access: None,
        });

        assert_eq!(
            library.playlists,
            Load::Ready(vec![
                playlist_with_cover("a", "A", a_context),
                playlist_with_cover("b", "B", b_cover),
            ])
        );
        // A's reply must not fill B's tracks.
        assert_eq!(library.tracks, Load::Loading);
    }

    #[test]
    fn a_cover_arriving_before_playlists_is_applied_after_them() {
        let context = cover_url("context");
        let mut library = Library::default();
        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: Vec::new(),
            cover: Some(context.clone()),
            access: None,
        });
        library.apply(&Update::Playlists(vec![playlist("a", "A")]));

        assert_eq!(
            library.playlists,
            Load::Ready(vec![playlist_with_cover("a", "A", context)])
        );
    }

    #[test]
    fn a_playlists_reload_keeps_a_recorded_upgrade() {
        let context = cover_url("context");
        let mut library = Library::default();
        library.apply(&Update::Playlists(vec![playlist("a", "A")]));
        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: Vec::new(),
            cover: Some(context.clone()),
            access: None,
        });
        library.apply(&Update::Playlists(vec![playlist("a", "A")]));

        assert_eq!(
            library.playlists,
            Load::Ready(vec![playlist_with_cover("a", "A", context)])
        );
    }

    #[test]
    fn a_removed_playlists_cover_is_pruned() {
        let context = cover_url("context");
        let mut library = Library::default();
        library.apply(&Update::Playlists(vec![playlist("a", "A")]));
        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: Vec::new(),
            cover: Some(context),
            access: None,
        });
        library.apply(&Update::Playlists(vec![playlist("b", "B")]));

        assert!(library.context_covers.is_empty(), "the key must be pruned");
        assert_eq!(library.playlists, Load::Ready(vec![playlist("b", "B")]));
    }

    #[test]
    fn a_non_awaited_some_cover_is_stored_as_derived() {
        let context = cover_url("context");
        let mut library = Library::default();
        library.apply(&Update::Playlists(vec![playlist("a", "A")]));
        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: vec![track("t")],
            cover: Some(context.clone()),
            access: None,
        });

        assert_eq!(
            library.playlists,
            Load::Ready(vec![playlist_with_cover("a", "A", context)]),
            "a read that answers no ask still stores its cover, as derived"
        );
    }

    #[test]
    fn a_non_awaited_some_cover_never_replaces_a_chosen_one() {
        let chosen = cover_url("chosen");
        let context = cover_url("context");
        let mut library = Library::default();
        library.apply(&Update::Playlists(vec![playlist_with_chosen_cover(
            "a",
            "A",
            chosen.clone(),
        )]));
        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: vec![track("t")],
            cover: Some(context),
            access: None,
        });

        assert_eq!(
            library.playlists,
            Load::Ready(vec![playlist_with_chosen_cover("a", "A", chosen)]),
            "a chosen picture is the user's own, so a read must not replace it"
        );
    }

    #[test]
    fn an_awaited_some_cover_is_stored_as_derived() {
        let context = cover_url("context");
        let mut library = Library::default();
        library.apply(&Update::Playlists(vec![playlist("a", "A")]));
        // The row edit that drops a cover is what marks it due; seeding the ask
        // here keeps the reply rule under test on its own.
        library.cover_refresh.drop_for("a");
        library.select("a");
        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: vec![track("t")],
            cover: Some(context.clone()),
            access: None,
        });

        assert!(
            !library.cover_refresh.answer("a"),
            "the reply must consume the ask, or an unawaited reply would look awaited"
        );
        assert_eq!(
            library.playlists,
            Load::Ready(vec![playlist_with_cover("a", "A", context)]),
            "a cover that answers an ask is stored as derived"
        );
    }

    #[test]
    fn an_awaited_none_cover_clears_a_derived_cover_and_its_cache_entry() {
        let first = cover_url("first");
        let context = cover_url("context");
        let mut library = Library::default();
        library.apply(&Update::Playlists(vec![playlist_with_cover(
            "a", "A", first,
        )]));
        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: vec![track("t")],
            cover: Some(context),
            access: None,
        });
        assert!(
            library.context_covers.contains_key("a"),
            "the arrange step must cache a context image"
        );

        library.cover_refresh.drop_for("a");
        library.select("a");
        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: Vec::new(),
            cover: None,
            access: None,
        });

        assert_eq!(
            library.playlists,
            Load::Ready(vec![playlist("a", "A")]),
            "the reply to an ask with no cover clears the derived one"
        );
        assert!(
            !library.context_covers.contains_key("a"),
            "the cached image goes with the cover it was drawn from"
        );
    }

    #[test]
    fn an_awaited_none_cover_never_clears_a_chosen_one() {
        let chosen = cover_url("chosen");
        let mut library = Library::default();
        library.apply(&Update::Playlists(vec![playlist_with_chosen_cover(
            "a",
            "A",
            chosen.clone(),
        )]));
        library.cover_refresh.drop_for("a");
        library.select("a");
        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: Vec::new(),
            cover: None,
            access: None,
        });

        assert!(
            !library.cover_refresh.answer("a"),
            "the ask was answered, so the chosen guard is what kept the cover"
        );
        assert_eq!(
            library.playlists,
            Load::Ready(vec![playlist_with_chosen_cover("a", "A", chosen)]),
            "a reply to an ask must not clear a chosen picture"
        );
    }

    #[test]
    fn a_listing_reload_does_not_overlay_a_cached_cover_onto_a_chosen_entry() {
        let context = cover_url("context");
        let chosen = cover_url("chosen");
        let mut library = Library::default();
        library.apply(&Update::Playlists(vec![playlist("a", "A")]));
        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: vec![track("t")],
            cover: Some(context),
            access: None,
        });
        library.apply(&Update::Playlists(vec![playlist_with_chosen_cover(
            "a",
            "A",
            chosen.clone(),
        )]));

        assert_eq!(
            library.playlists,
            Load::Ready(vec![playlist_with_chosen_cover("a", "A", chosen)]),
            "a reload must not draw a cached derived image over a chosen picture"
        );
    }

    #[test]
    fn a_removed_playlist_is_forgotten_by_cover_refresh() {
        let mut library = sidebar_library(&["a", "b"], 0x07, true, None);
        library.cover_refresh.drop_for("a");
        library.cover_refresh.asked("a");
        library.cover_refresh.drop_for("a");
        assert!(
            library
                .remove_from_library(&item(SidebarKind::Playlists, "a"))
                .is_some()
        );
        library.apply(&Update::PlaylistRemoved {
            playlist: "a".to_owned(),
            revision: revision(0x09),
        });

        assert!(
            library
                .cover_refresh
                .take_due(&names(&["a"]), None)
                .is_none(),
            "a removed playlist must not stay due for a cover"
        );
        assert!(
            !library.cover_refresh.answer("a"),
            "a removed playlist must not still be awaiting a cover"
        );
    }

    #[test]
    fn a_removed_playlist_takes_its_cached_cover_with_it() {
        let context = cover_url("context");
        let mut library = sidebar_library(&["a", "b"], 0x07, true, None);
        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: Vec::new(),
            cover: Some(context),
            access: None,
        });
        assert!(
            library.context_covers.contains_key("a"),
            "arrange: the reply must cache a cover for the row"
        );

        assert!(
            library
                .remove_from_library(&item(SidebarKind::Playlists, "a"))
                .is_some()
        );
        library.apply(&Update::PlaylistRemoved {
            playlist: "a".to_owned(),
            revision: revision(0x09),
        });

        assert!(
            !library.context_covers.contains_key("a"),
            "a cached cover outlived the row it belonged to"
        );
    }

    #[test]
    fn a_playlists_read_prunes_cover_refresh() {
        let mut library = Library::default();
        library.apply(&Update::Playlists(vec![
            playlist("a", "A"),
            playlist("b", "B"),
        ]));
        library.cover_refresh.drop_for("b");
        library.apply(&Update::Playlists(vec![playlist("a", "A")]));

        assert!(
            library
                .cover_refresh
                .take_due(&names(&["b"]), None)
                .is_none(),
            "a playlist the read no longer lists must not stay due"
        );
    }

    #[test]
    fn a_due_playlist_selected_answers_its_own_cover() {
        let first = cover_url("first");
        let mut library = Library::default();
        library.apply(&Update::Playlists(vec![playlist_with_cover(
            "a", "A", first,
        )]));
        library.cover_refresh.drop_for("a");
        library.select("a");

        // Only the reply to an ask clears a cover, so this clear proves the
        // navigation's own read was adopted as the answer.
        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: Vec::new(),
            cover: None,
            access: None,
        });

        assert_eq!(
            library.playlists,
            Load::Ready(vec![playlist("a", "A")]),
            "the read a navigation issued answers its own ask"
        );
    }

    #[test]
    fn a_failed_read_releases_the_ask() {
        let first = cover_url("first");
        let mut library = Library::default();
        library.apply(&Update::Playlists(vec![playlist_with_cover(
            "a",
            "A",
            first.clone(),
        )]));
        library.cover_refresh.drop_for("a");
        library.select("a");
        library.apply(&Update::TracksFailed {
            playlist: "a".to_owned(),
            failure: Failure::Load,
        });
        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: Vec::new(),
            cover: None,
            access: None,
        });

        assert_eq!(
            library.playlists,
            Load::Ready(vec![playlist_with_cover("a", "A", first)]),
            "the failure answered the ask, so the next reply is not awaited and clears nothing"
        );
    }

    #[test]
    fn an_awaited_reply_while_the_listing_loads_still_fills_the_cache() {
        let old = cover_url("old");
        let context = cover_url("context");
        let mut library = Library::default();
        library.apply(&Update::Playlists(vec![playlist_with_cover(
            "a",
            "A",
            old.clone(),
        )]));
        library.cover_refresh.drop_for("a");
        library.select("a");
        library.reload();
        assert_eq!(library.playlists, Load::Loading);

        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: vec![track("t")],
            cover: Some(context.clone()),
            access: None,
        });
        assert_eq!(
            library.context_covers.get("a"),
            Some(&context),
            "an awaited reply fills the cache even with no list to write to"
        );

        library.apply(&Update::Playlists(vec![playlist_with_cover("a", "A", old)]));

        assert_eq!(
            library.playlists,
            Load::Ready(vec![playlist_with_cover("a", "A", context)]),
            "the listing's own cover is older than the reply, so the cache fills the row"
        );
    }

    #[test]
    fn removing_the_first_track_drops_the_cover_and_asks_for_it_again() {
        let first = cover_url("first");
        let mut library = covered_page(&first, &["spotify:track:t0", "spotify:track:t1"]);
        assert!(
            library
                .remove_tracks("a", &[0], &moved(&["spotify:track:t0"]))
                .is_some()
        );

        library.apply(&Update::TracksRemoved {
            playlist: "a".to_owned(),
            revision: revision(0x02),
        });

        assert_eq!(
            entry_cover(&library, "a"),
            None,
            "the cover was taken from a track the list no longer starts with"
        );
        assert_eq!(
            library.tracks_request(),
            Some(Command::LoadTracks {
                playlist: "a".to_owned()
            }),
            "a dropped cover is read again"
        );
    }

    #[test]
    fn removing_a_later_row_keeps_the_cover_and_asks_nothing() {
        let first = cover_url("first");
        let mut library = covered_page(
            &first,
            &["spotify:track:t0", "spotify:track:t1", "spotify:track:t2"],
        );
        assert!(
            library
                .remove_tracks("a", &[1], &moved(&["spotify:track:t1"]))
                .is_some()
        );

        library.apply(&Update::TracksRemoved {
            playlist: "a".to_owned(),
            revision: revision(0x02),
        });

        assert_eq!(
            entry_cover(&library, "a"),
            Some(PlaylistCover::derived(first)),
            "the first track is still the one the cover came from"
        );
        assert_eq!(
            library.tracks_request(),
            None,
            "a cover that is still right must not be read again"
        );
    }

    #[test]
    fn moving_a_row_to_the_front_drops_the_cover() {
        let first = cover_url("first");
        let mut library = covered_page(
            &first,
            &["spotify:track:t0", "spotify:track:t1", "spotify:track:t2"],
        );
        assert!(move_one(&mut library, "a", 2, 0, &track("spotify:track:t2")).is_some());

        library.apply(&Update::TrackMoved {
            playlist: "a".to_owned(),
            revision: revision(0x02),
        });

        assert_eq!(
            entry_cover(&library, "a"),
            None,
            "the list now starts with another track, so its cover is wrong"
        );
        assert_eq!(
            library.tracks_request(),
            Some(Command::LoadTracks {
                playlist: "a".to_owned()
            }),
            "a move that changes the first track drops and re-asks the cover"
        );
    }

    #[test]
    fn a_move_that_keeps_the_first_track_keeps_the_cover() {
        let first = cover_url("first");
        let mut library = covered_page(
            &first,
            &["spotify:track:t0", "spotify:track:t1", "spotify:track:t2"],
        );
        assert!(move_one(&mut library, "a", 1, 3, &track("spotify:track:t1")).is_some());

        library.apply(&Update::TrackMoved {
            playlist: "a".to_owned(),
            revision: revision(0x02),
        });

        assert_eq!(
            entry_cover(&library, "a"),
            Some(PlaylistCover::derived(first)),
            "rows two and three swapped, so the first track the cover came from is unchanged"
        );
        assert_eq!(
            library.tracks_request(),
            None,
            "a move that keeps the first track asks for nothing"
        );
    }

    #[test]
    fn an_episode_in_front_does_not_count_as_the_first_track_in_a_move() {
        let first = cover_url("first");
        let mut library = covered_page(
            &first,
            &["spotify:episode:e", "spotify:track:t0", "spotify:track:t1"],
        );
        assert!(move_one(&mut library, "a", 2, 1, &track("spotify:track:t1")).is_some());

        library.apply(&Update::TrackMoved {
            playlist: "a".to_owned(),
            revision: revision(0x02),
        });

        assert_eq!(
            entry_cover(&library, "a"),
            None,
            "a cover comes from the first track, so a move past an episode still changes it"
        );
        assert_eq!(
            library.tracks_request(),
            Some(Command::LoadTracks {
                playlist: "a".to_owned()
            }),
            "the first track changed, so the cover is read again"
        );
    }

    #[test]
    fn an_add_to_an_empty_playlist_asks_for_its_cover() {
        let mut library = sidebar_library(&["a", "b"], 0x07, true, Some(0));
        // An add is made from another page, so the target's own rows are not on
        // screen to say which track came first.
        library.open(album("spotify:album:x"));
        assert!(library.add_tracks(0, &pick(&["spotify:track:x"])).is_some());

        library.apply(&Update::TrackAdded {
            playlist: "a".to_owned(),
            revision: revision(0x08),
        });

        assert_eq!(
            entry_cover(&library, "a"),
            None,
            "an empty list had no cover for the add to keep"
        );
        assert_eq!(
            library.tracks_request(),
            Some(Command::LoadTracks {
                playlist: "a".to_owned()
            }),
            "a list an add filled needs its first cover read"
        );
    }

    #[test]
    fn an_add_to_a_playlist_with_a_cover_keeps_it() {
        let first = cover_url("first");
        let mut library = sidebar_library(&["a", "b"], 0x07, true, Some(2));
        library.playlists = Load::Ready(vec![
            with_access(
                playlist_with_cover("a", "A", first.clone()),
                0x07,
                true,
                Some(2),
            ),
            with_access(playlist("b", "B"), 0x07, true, Some(2)),
        ]);
        library.open(album("spotify:album:x"));
        assert!(library.add_tracks(0, &pick(&["spotify:track:x"])).is_some());

        library.apply(&Update::TrackAdded {
            playlist: "a".to_owned(),
            revision: revision(0x08),
        });

        assert_eq!(
            entry_cover(&library, "a"),
            Some(PlaylistCover::derived(first)),
            "an append names the end of the list, so it cannot change its first track"
        );
        assert_eq!(
            library.tracks_request(),
            None,
            "a cover an add kept is not read again"
        );
    }

    #[test]
    fn a_chosen_cover_survives_add_remove_and_move_and_asks_nothing() {
        let chosen = cover_url("chosen");
        let mut library = page_with(
            "a",
            &["spotify:track:t0", "spotify:track:t1", "spotify:track:t2"],
            access(0x01, true),
        );
        library.playlists = Load::Ready(vec![with_access(
            playlist_with_chosen_cover("a", "A", chosen.clone()),
            0x07,
            true,
            Some(3),
        )]);

        assert!(
            library
                .remove_tracks("a", &[0], &moved(&["spotify:track:t0"]))
                .is_some()
        );
        library.apply(&Update::TracksRemoved {
            playlist: "a".to_owned(),
            revision: revision(0x02),
        });
        assert_eq!(
            entry_cover(&library, "a"),
            Some(PlaylistCover::chosen(chosen.clone())),
            "a chosen picture is the user's own, so a removal cannot drop it"
        );

        assert!(move_one(&mut library, "a", 1, 0, &track("spotify:track:t2")).is_some());
        library.apply(&Update::TrackMoved {
            playlist: "a".to_owned(),
            revision: revision(0x03),
        });
        assert_eq!(
            entry_cover(&library, "a"),
            Some(PlaylistCover::chosen(chosen.clone())),
            "a chosen picture is the user's own, so a move cannot drop it"
        );

        library.open(album("spotify:album:x"));
        assert!(library.add_tracks(0, &pick(&["spotify:track:y"])).is_some());
        library.apply(&Update::TrackAdded {
            playlist: "a".to_owned(),
            revision: revision(0x08),
        });
        assert_eq!(
            entry_cover(&library, "a"),
            Some(PlaylistCover::chosen(chosen)),
            "a chosen picture is the user's own, so an add cannot drop it"
        );
        assert_eq!(
            library.tracks_request(),
            None,
            "a chosen picture is never read over"
        );
    }

    #[test]
    fn a_reply_with_no_pending_write_drops_nothing() {
        let first = cover_url("first");
        let mut library = covered_page(&first, &["spotify:track:t0", "spotify:track:t1"]);

        // No removal was begun, so this reply names no rows to compare.
        library.apply(&Update::TracksRemoved {
            playlist: "a".to_owned(),
            revision: revision(0x02),
        });

        assert_eq!(
            entry_cover(&library, "a"),
            Some(PlaylistCover::derived(first)),
            "a reply with no write pending cannot say a row changed"
        );
        assert_eq!(
            library.tracks_request(),
            None,
            "a reply that drops nothing asks for nothing"
        );
    }

    #[test]
    fn a_reply_for_another_playlist_does_not_drop_a_pending_writes_cover() {
        let first = cover_url("first");
        let mut library = covered_page(&first, &["spotify:track:t0", "spotify:track:t1"]);
        assert!(
            library
                .remove_tracks("a", &[0], &moved(&["spotify:track:t0"]))
                .is_some()
        );

        // Neither reply has a write of its own on the list that holds the
        // cover, so neither can say a row of it changed.
        library.apply(&Update::TrackMoved {
            playlist: "b".to_owned(),
            revision: revision(0x09),
        });
        library.apply(&Update::TracksRemoved {
            playlist: "b".to_owned(),
            revision: revision(0x09),
        });

        assert_eq!(
            entry_cover(&library, "a"),
            Some(PlaylistCover::derived(first)),
            "a reply for another playlist cannot drop the pending write's cover"
        );
        assert!(
            library.pending.is_some(),
            "the write for a is still in flight"
        );
        assert_eq!(
            library.tracks_request(),
            None,
            "a reply that drops nothing asks for nothing"
        );
    }

    #[test]
    fn a_move_with_no_write_pending_drops_nothing() {
        let first = cover_url("first");
        let mut library = covered_page(&first, &["spotify:track:t0", "spotify:track:t1"]);

        // No move was begun, so this reply names no rows to compare.
        library.apply(&Update::TrackMoved {
            playlist: "a".to_owned(),
            revision: revision(0x02),
        });

        assert_eq!(
            entry_cover(&library, "a"),
            Some(PlaylistCover::derived(first)),
            "a move reply with no write pending cannot say a row changed"
        );
        assert_eq!(
            library.tracks_request(),
            None,
            "a reply that drops nothing asks for nothing"
        );
    }

    #[test]
    fn an_add_without_a_matching_pending_add_drops_nothing() {
        let mut library = sidebar_library(&["a", "b"], 0x07, true, Some(0));

        // The row shows no cover, so a hook placed before the guard would ask.
        library.apply(&Update::TrackAdded {
            playlist: "a".to_owned(),
            revision: revision(0x08),
        });

        assert_eq!(
            entry_cover(&library, "a"),
            None,
            "a reply naming no add of its own changes no cover"
        );
        assert_eq!(
            library.tracks_request(),
            None,
            "a reply with no add pending asks for nothing"
        );
    }

    #[test]
    fn an_edit_on_the_open_page_asks_for_the_cover_without_touching_wanted_or_rows() {
        let first = cover_url("first");
        let mut library = covered_page(&first, &["spotify:track:t0", "spotify:track:t1"]);
        assert!(
            library
                .remove_tracks("a", &[0], &moved(&["spotify:track:t0"]))
                .is_some()
        );
        library.apply(&Update::TracksRemoved {
            playlist: "a".to_owned(),
            revision: revision(0x02),
        });

        assert_eq!(
            library.tracks_request(),
            Some(Command::LoadTracks {
                playlist: "a".to_owned()
            }),
            "the dropped cover is asked for"
        );
        assert_eq!(
            library.wanted.as_deref(),
            Some("a"),
            "a cover read for a listed playlist must not take the page's want"
        );
        assert_eq!(
            library.tracks,
            ready("a", &["spotify:track:t1"], access(0x02, true)),
            "a bare cover read must not blank the rows on screen"
        );
    }

    #[test]
    fn an_add_while_the_page_is_idle_asks_once_and_marks_the_page_loading() {
        let first = cover_url("first");
        let mut library = page_dropped_by_an_add(Some(&first));
        assert_eq!(
            library.tracks,
            Load::Idle,
            "arrange: the add dropped the page"
        );

        assert_eq!(
            library.tracks_request(),
            Some(Command::LoadTracks {
                playlist: "a".to_owned()
            }),
            "a page a write left blank is asked for once"
        );
        assert_eq!(
            library.tracks,
            Load::Loading,
            "the ask marks the page loading, which is what stops a second one"
        );
        assert_eq!(
            library.tracks_request(),
            None,
            "a page already loading is not asked for twice"
        );
    }

    #[test]
    fn an_add_while_the_page_is_loading_resets_it_and_the_re_ask_wins() {
        let first = cover_url("first");
        let mut library = page_dropped_by_an_add(Some(&first));
        assert!(library.tracks_request().is_some(), "arrange: the re-ask");

        // The read taken before the add, arriving after the re-ask was issued.
        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: moved(&["spotify:track:t0", "spotify:track:t1"]),
            cover: Some(first.clone()),
            access: access(0x01, true),
        });
        assert_eq!(
            library.tracks,
            ready(
                "a",
                &["spotify:track:t0", "spotify:track:t1"],
                access(0x01, true)
            ),
            "arrange: the pre-add reply lands as the page first, race and all"
        );
        // The re-ask's own reply, which is last and therefore the page.
        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: moved(&["spotify:track:t0", "spotify:track:t1", "spotify:track:x"]),
            cover: Some(first),
            access: access(0x08, true),
        });

        assert_eq!(
            library.tracks,
            ready(
                "a",
                &["spotify:track:t0", "spotify:track:t1", "spotify:track:x"],
                access(0x08, true)
            ),
            "the reply to the newest read is the page, whatever arrived in between"
        );
    }

    #[test]
    fn a_failed_page_is_never_asked_automatically() {
        let mut library = sidebar_library(&["a", "b"], 0x07, true, Some(2));
        library.select("a");
        library.apply(&Update::TracksFailed {
            playlist: "a".to_owned(),
            failure: Failure::Load,
        });

        assert_eq!(
            library.tracks_request(),
            None,
            "a failed page is left for its Retry, never read behind the user's back"
        );
        assert_eq!(
            library.tracks,
            Load::Failed(Failure::Load),
            "and the failure stands for the Retry to see"
        );
    }

    #[test]
    fn a_failed_cover_re_ask_leaves_the_cover_dropped_and_does_not_ask_again() {
        let first = cover_url("first");
        let mut library = covered_page(&first, &["spotify:track:t0", "spotify:track:t1"]);
        assert!(
            library
                .remove_tracks("a", &[0], &moved(&["spotify:track:t0"]))
                .is_some()
        );
        library.apply(&Update::TracksRemoved {
            playlist: "a".to_owned(),
            revision: revision(0x02),
        });
        assert!(library.tracks_request().is_some(), "arrange: the re-ask");

        library.apply(&Update::TracksFailed {
            playlist: "a".to_owned(),
            failure: Failure::Load,
        });

        assert_eq!(
            entry_cover(&library, "a"),
            None,
            "a failed re-read leaves the cover dropped rather than restoring a wrong one"
        );
        assert_eq!(
            library.tracks_request(),
            None,
            "a failed ask is not retried, so the hole cannot loop"
        );
        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: moved(&["spotify:track:t1"]),
            cover: None,
            access: access(0x02, true),
        });
        assert_eq!(
            entry_cover(&library, "a"),
            None,
            "a later reply that answers no ask cannot say why the cover is missing"
        );
    }

    #[test]
    fn a_removed_or_unlisted_playlist_is_never_asked() {
        let first = cover_url("first");
        let mut unlisted = covered_page(&first, &["spotify:track:t0", "spotify:track:t1"]);
        assert!(
            unlisted
                .remove_tracks("a", &[0], &moved(&["spotify:track:t0"]))
                .is_some()
        );
        unlisted.apply(&Update::TracksRemoved {
            playlist: "a".to_owned(),
            revision: revision(0x02),
        });
        unlisted.apply(&Update::Playlists(Vec::new()));

        assert_eq!(
            unlisted.tracks_request(),
            None,
            "a playlist the list no longer names is never asked for"
        );

        let mut removed = covered_page(&first, &["spotify:track:t0", "spotify:track:t1"]);
        removed.playlists = Load::Ready(vec![
            with_access(
                playlist_with_cover("a", "A", first.clone()),
                0x07,
                true,
                Some(2),
            ),
            with_access(playlist("b", "B"), 0x07, true, Some(1)),
        ]);
        removed.apply(&Update::RootlistAccess(rootlist_access(0x07, true)));
        assert!(
            removed
                .remove_tracks("a", &[0], &moved(&["spotify:track:t0"]))
                .is_some()
        );
        removed.apply(&Update::TracksRemoved {
            playlist: "a".to_owned(),
            revision: revision(0x02),
        });
        assert!(
            removed
                .remove_from_library(&item(SidebarKind::Playlists, "a"))
                .is_some()
        );
        removed.apply(&Update::PlaylistRemoved {
            playlist: "a".to_owned(),
            revision: revision(0x09),
        });

        assert_eq!(
            removed.tracks_request(),
            None,
            "a playlist the user removed is never asked for"
        );
    }

    #[test]
    fn a_pre_edit_reply_arriving_after_the_edit_is_overwritten_by_the_re_ask() {
        let first = cover_url("first");
        let stale = cover_url("stale");
        let mut library = covered_page(
            &first,
            &["spotify:track:t0", "spotify:track:t1", "spotify:track:t2"],
        );
        assert!(
            library
                .remove_tracks("a", &[0], &moved(&["spotify:track:t0"]))
                .is_some()
        );
        library.apply(&Update::TracksRemoved {
            playlist: "a".to_owned(),
            revision: revision(0x02),
        });

        // The read taken before the removal answers no ask, so it is stored
        // with the image it was taken with.
        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: moved(&["spotify:track:t0", "spotify:track:t1", "spotify:track:t2"]),
            cover: Some(stale.clone()),
            access: access(0x01, true),
        });
        assert_eq!(
            entry_cover(&library, "a"),
            Some(PlaylistCover::derived(stale)),
            "arrange: the pre-edit reply is stored before the re-ask is issued"
        );

        assert_eq!(
            library.tracks_request(),
            Some(Command::LoadTracks {
                playlist: "a".to_owned()
            }),
            "arrange: the dropped cover is asked for again"
        );

        // The re-read answers the ask and finds no cover at all, and only that
        // reply may say so.
        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: moved(&["spotify:track:t1", "spotify:track:t2"]),
            cover: None,
            access: access(0x02, true),
        });

        assert_eq!(
            entry_cover(&library, "a"),
            None,
            "the awaited answer's `None` clears the stale cover the pre-edit reply stored"
        );
    }

    #[test]
    fn two_quick_removals_end_with_the_last_answer() {
        let first = cover_url("first");
        let stale = cover_url("stale");
        let mut library = covered_page(
            &first,
            &["spotify:track:t0", "spotify:track:t1", "spotify:track:t2"],
        );
        assert!(
            library
                .remove_tracks("a", &[0], &moved(&["spotify:track:t0"]))
                .is_some()
        );
        library.apply(&Update::TracksRemoved {
            playlist: "a".to_owned(),
            revision: revision(0x02),
        });
        assert!(library.tracks_request().is_some(), "arrange: the first ask");

        assert!(
            library
                .remove_tracks("a", &[0], &moved(&["spotify:track:t1"]))
                .is_some()
        );
        library.apply(&Update::TracksRemoved {
            playlist: "a".to_owned(),
            revision: revision(0x03),
        });
        assert!(
            library.tracks_request().is_some(),
            "arrange: the second ask"
        );

        // The older ask's reply lands first, and the newer one has the last word.
        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: moved(&["spotify:track:t1", "spotify:track:t2"]),
            cover: Some(stale),
            access: access(0x02, true),
        });
        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: moved(&["spotify:track:t2"]),
            cover: None,
            access: access(0x03, true),
        });

        assert_eq!(
            entry_cover(&library, "a"),
            None,
            "the last answer said the list has no cover, so none is drawn"
        );
    }

    #[test]
    fn draining_tracks_request_terminates_with_several_due_playlists() {
        let first = cover_url("first");
        let mut library = covered_page(&first, &["spotify:track:t0", "spotify:track:t1"]);
        let mut more = match &library.playlists {
            Load::Ready(playlists) => playlists.clone(),
            Load::Idle | Load::Loading | Load::Failed(_) => Vec::new(),
        };
        more.push(with_access(playlist("b", "B"), 0x07, true, Some(1)));
        more.push(with_access(playlist("c", "C"), 0x07, true, Some(1)));
        library.playlists = Load::Ready(more);
        library.cover_refresh.drop_for("b");
        library.cover_refresh.drop_for("c");

        let mut asked = Vec::new();
        for _ in 0..8 {
            match library.tracks_request() {
                Some(command) => asked.push(command),
                None => break,
            }
        }

        assert_eq!(
            asked,
            vec![
                Command::LoadTracks {
                    playlist: "b".to_owned()
                },
                Command::LoadTracks {
                    playlist: "c".to_owned()
                },
            ],
            "each due playlist is asked for exactly once"
        );
        assert_eq!(
            library.tracks_request(),
            None,
            "the drain ends rather than spinning"
        );
    }

    #[test]
    fn the_shell_and_tracks_request_never_both_ask_for_one_page() {
        let first = cover_url("first");
        let mut library = page_dropped_by_an_add(Some(&first));

        // The shell's own read goes first, and it marks the page loading.
        assert_eq!(
            library.open(Page::Playlist("a".to_owned())),
            Some(Command::LoadTracks {
                playlist: "a".to_owned()
            }),
            "arrange: the shell asks for the page"
        );

        assert_eq!(
            library.tracks_request(),
            None,
            "a page the shell already asked for must not be asked for twice"
        );
    }

    #[test]
    fn a_created_playlist_keeps_its_cover_and_origin_through_a_later_add() {
        let cover = cover_url("cover");
        let new = "spotify:playlist:new";
        let mut library = naming_library(&["a"], "Mix");
        assert!(library.create_playlist().is_some());
        library.apply(&Update::PlaylistCreated {
            entry: with_access(
                covered(new, "Mix", PlaylistCover::derived(cover.clone())),
                0x08,
                true,
                Some(0),
            ),
            revision: revision(0x08),
        });
        assert_eq!(
            entry_cover(&library, new),
            Some(PlaylistCover::derived(cover.clone())),
            "arrange: the created row carries the cover its reply named"
        );

        library.open(album("spotify:album:x"));
        assert!(library.add_tracks(0, &pick(&["spotify:track:x"])).is_some());
        library.apply(&Update::TrackAdded {
            playlist: new.to_owned(),
            revision: revision(0x09),
        });

        assert_eq!(
            entry_cover(&library, new),
            Some(PlaylistCover::derived(cover)),
            "an append cannot change which track a cover came from"
        );
        assert_eq!(
            library.tracks_request(),
            None,
            "so the new list's cover is not read again"
        );
    }

    #[test]
    fn open_an_artist_starts_its_load_and_keeps_the_album() {
        let mut library = Library::default();
        library.open(album("spotify:album:a"));
        library.apply(&Update::Album(album_detail("spotify:album:a")));

        assert_eq!(
            library.open(artist("spotify:artist:a")),
            Some(artist_command("spotify:artist:a"))
        );
        assert_eq!(library.page(), Some(&artist("spotify:artist:a")));
        assert_eq!(library.artist, Load::Loading);
        assert_eq!(
            library.album,
            Load::Ready(album_detail("spotify:album:a")),
            "the album behind the artist stays ready for Back"
        );
    }

    #[test]
    fn an_artist_reply_lands_only_on_its_own_want_while_loading() {
        let mut library = Library::default();
        library.open(artist("spotify:artist:a"));
        library.apply(&Update::Artist(artist_detail("spotify:artist:a")));
        assert_eq!(
            library.artist,
            Load::Ready(artist_detail("spotify:artist:a"))
        );

        library.apply(&Update::Artist(artist_detail("spotify:artist:b")));
        assert_eq!(
            library.artist,
            Load::Ready(artist_detail("spotify:artist:a")),
            "a reply nobody asked for is discarded"
        );
    }

    #[test]
    fn a_stale_artist_reply_is_discarded() {
        let mut library = Library::default();
        library.open(artist("spotify:artist:a"));
        library.open(artist("spotify:artist:b"));

        library.apply(&Update::Artist(artist_detail("spotify:artist:a")));
        assert_eq!(library.artist, Load::Loading, "A's reply is not B's");

        library.apply(&Update::Artist(artist_detail("spotify:artist:b")));
        assert_eq!(
            library.artist,
            Load::Ready(artist_detail("spotify:artist:b"))
        );
    }

    #[test]
    fn a_stale_artist_failure_is_discarded() {
        let mut library = Library::default();
        library.open(artist("spotify:artist:a"));
        library.open(artist("spotify:artist:b"));

        library.apply(&Update::ArtistFailed {
            artist: "spotify:artist:a".to_owned(),
            failure: Failure::Load,
        });
        assert_eq!(library.artist, Load::Loading, "A's failure is not B's");

        library.apply(&Update::ArtistFailed {
            artist: "spotify:artist:b".to_owned(),
            failure: Failure::Load,
        });
        assert_eq!(library.artist, Load::Failed(Failure::Load));
    }

    #[test]
    fn the_first_artist_reply_wins() {
        let mut library = Library::default();
        library.open(artist("spotify:artist:a"));
        let first = ArtistDetail {
            name: "First".to_owned(),
            ..artist_detail("spotify:artist:a")
        };
        let second = ArtistDetail {
            name: "Second".to_owned(),
            ..artist_detail("spotify:artist:a")
        };

        library.apply(&Update::Artist(first.clone()));
        library.apply(&Update::Artist(second));

        assert_eq!(library.artist, Load::Ready(first));
    }

    #[test]
    fn back_onto_a_ready_artist_needs_no_command() {
        let mut library = Library::default();
        library.open(artist("spotify:artist:a"));
        library.apply(&Update::Artist(artist_detail("spotify:artist:a")));
        library.to_playlists();

        assert_eq!(library.back(), Some(artist_command("spotify:artist:a")));
        assert_eq!(library.artist, Load::Loading);
        library.apply(&Update::Artist(artist_detail("spotify:artist:a")));

        library.select("p");
        assert_eq!(library.back(), None, "a ready artist is already settled");
        assert_eq!(library.page(), Some(&artist("spotify:artist:a")));
    }

    #[test]
    fn back_onto_a_failed_artist_reloads_it() {
        let mut library = Library::default();
        library.open(artist("spotify:artist:a"));
        library.apply(&Update::ArtistFailed {
            artist: "spotify:artist:a".to_owned(),
            failure: Failure::Load,
        });
        library.select("p");

        assert_eq!(
            library.back(),
            Some(artist_command("spotify:artist:a")),
            "a failed artist has nothing to draw, so Back asks again"
        );
        assert_eq!(library.artist, Load::Loading);
    }

    #[test]
    fn reopening_a_failed_artist_retries_it() {
        let mut library = Library::default();
        library.open(artist("spotify:artist:a"));
        library.apply(&Update::ArtistFailed {
            artist: "spotify:artist:a".to_owned(),
            failure: Failure::Load,
        });

        assert_eq!(
            library.open(artist("spotify:artist:a")),
            Some(artist_command("spotify:artist:a")),
            "the page's Retry is this call, so it must re-issue the load"
        );
        assert_eq!(library.artist, Load::Loading);
    }

    #[test]
    fn the_filter_starts_on_all() {
        assert_eq!(Library::default().filter.chosen(), Chip::All);
        assert_eq!(Library::default().filter.artist(), None);
    }

    #[test]
    fn the_sidebar_kind_starts_on_playlists() {
        let mut library = Library::default();
        assert_eq!(library.sidebar_kind.chosen(), SidebarKind::Playlists);

        library.open(show("spotify:show:a"));
        assert_eq!(
            library.sidebar_kind.chosen(),
            SidebarKind::Playlists,
            "opening a page keeps the sidebar's choice"
        );

        library.back();
        assert_eq!(
            library.sidebar_kind.chosen(),
            SidebarKind::Playlists,
            "a Back keeps the sidebar's choice"
        );

        library.to_playlists();
        assert_eq!(
            library.sidebar_kind.chosen(),
            SidebarKind::Playlists,
            "going home keeps the sidebar's choice"
        );
    }

    #[test]
    fn the_sidebar_kind_survives_a_trip_to_a_show_page_and_back() {
        let mut library = Library::default();
        assert!(library.sidebar_kind.choose(SidebarKind::Podcasts));

        library.open(show("spotify:show:a"));
        assert_eq!(library.sidebar_kind.chosen(), SidebarKind::Podcasts);

        library.back();
        assert_eq!(library.sidebar_kind.chosen(), SidebarKind::Podcasts);

        library.to_playlists();
        assert_eq!(library.sidebar_kind.chosen(), SidebarKind::Podcasts);
    }

    #[test]
    fn the_playlist_highlight_clears_on_a_show_page_and_returns() {
        let mut library = Library::default();
        library.select("spotify:playlist:a");
        library.open(show("spotify:show:a"));
        assert_eq!(
            library.selected(),
            None,
            "the main panel is not showing a playlist"
        );

        library.back();
        assert_eq!(library.selected(), Some("spotify:playlist:a"));
    }

    #[test]
    fn the_chosen_chip_survives_a_trip_to_a_show_page_and_back() {
        let mut library = Library::default();
        library.root_filter.choose(RootKind::Podcasts);
        assert_eq!(
            library.root_filter.chosen(),
            RootKind::Podcasts,
            "a live chip is pressed"
        );

        library.open(show("spotify:show:a"));
        assert_eq!(
            library.root_filter.chosen(),
            RootKind::Podcasts,
            "opening a page keeps the root's choice"
        );

        library.back();
        assert_eq!(
            library.root_filter.chosen(),
            RootKind::Podcasts,
            "a Back to the root redraws the chip the user left"
        );
    }

    #[test]
    fn opening_another_artist_resets_the_filter() {
        let mut library = Library::default();
        library.open(artist("spotify:artist:a"));
        library.filter.choose(Chip::Kind(ReleaseKind::Ep));

        library.open(artist("spotify:artist:b"));

        assert_eq!(
            library.filter.chosen(),
            Chip::All,
            "another artist's discs are not the ones that were filtered"
        );
        assert_eq!(library.filter.artist(), Some("spotify:artist:b"));
    }

    #[test]
    fn back_onto_the_same_artist_keeps_the_filter() {
        let mut library = Library::default();
        library.open(artist("spotify:artist:a"));
        library.apply(&Update::Artist(artist_detail("spotify:artist:a")));
        library.filter.choose(Chip::Kind(ReleaseKind::Ep));

        library.to_playlists();
        let _ = library.back();

        assert_eq!(
            library.filter.chosen(),
            Chip::Kind(ReleaseKind::Ep),
            "Back onto the same artist draws what the user left"
        );
    }

    #[test]
    fn a_reload_of_the_same_artist_keeps_the_query_and_another_artist_drops_it() {
        let mut library = Library::default();
        library.open(artist("spotify:artist:a"));
        library.apply(&Update::Artist(artist_detail("spotify:artist:a")));
        library.filter.edit().push_str("  Midnight  ");

        library.to_playlists();
        let _ = library.back();
        assert_eq!(
            library.filter.needle().as_deref(),
            Some("midnight"),
            "a reload of the artist the query was typed on keeps it"
        );

        library.open(artist("spotify:artist:b"));
        assert_eq!(
            library.filter.needle(),
            None,
            "another artist's releases are not the ones the query named"
        );
        assert!(
            library.filter.edit().is_empty(),
            "nor does the field carry the words over"
        );
    }

    #[test]
    fn retrying_the_same_artist_keeps_the_filter() {
        let mut library = Library::default();
        library.open(artist("spotify:artist:a"));
        library.apply(&Update::ArtistFailed {
            artist: "spotify:artist:a".to_owned(),
            failure: Failure::Load,
        });
        library.filter.choose(Chip::Kind(ReleaseKind::Single));

        assert_eq!(
            library.open(artist("spotify:artist:a")),
            Some(artist_command("spotify:artist:a")),
            "Retry re-issues the same load"
        );
        assert_eq!(library.filter.chosen(), Chip::Kind(ReleaseKind::Single));
    }

    #[test]
    fn an_artist_reply_does_not_touch_the_filter() {
        let mut library = Library::default();
        library.open(artist("spotify:artist:a"));
        library.filter.choose(Chip::Kind(ReleaseKind::Compilation));

        library.apply(&Update::Artist(artist_detail("spotify:artist:a")));

        assert_eq!(
            library.filter.chosen(),
            Chip::Kind(ReleaseKind::Compilation),
            "a reply is not an artist change"
        );
    }

    #[test]
    fn to_playlists_keeps_the_filter() {
        let mut library = Library::default();
        library.open(artist("spotify:artist:a"));
        library.apply(&Update::Artist(artist_detail("spotify:artist:a")));
        library.filter.choose(Chip::Kind(ReleaseKind::Single));

        library.to_playlists();
        assert_eq!(
            library.filter.chosen(),
            Chip::Kind(ReleaseKind::Single),
            "home then Back reloads the same artist, so the choice is kept"
        );
        assert_eq!(library.filter.artist(), Some("spotify:artist:a"));

        let _ = library.back();
        assert_eq!(
            library.filter.chosen(),
            Chip::Kind(ReleaseKind::Single),
            "home then Back is still the same artist"
        );
    }

    #[test]
    fn reopening_the_same_artist_keeps_the_filter() {
        let mut library = Library::default();
        library.open(artist("spotify:artist:a"));
        library.apply(&Update::Artist(artist_detail("spotify:artist:a")));
        library.filter.choose(Chip::Kind(ReleaseKind::Ep));
        library.to_playlists();

        assert_eq!(
            library.open(artist("spotify:artist:a")),
            Some(artist_command("spotify:artist:a")),
            "home cleared the want, so the page is loaded fresh rather than recalled"
        );
        assert_eq!(
            library.filter.chosen(),
            Chip::Kind(ReleaseKind::Ep),
            "opening the artist the choice was made for keeps it"
        );
    }

    #[test]
    fn another_artists_ep_chip_does_not_keep_the_choice() {
        let mut library = Library::default();
        library.open(artist("spotify:artist:a"));
        library.apply(&Update::Artist(artist_detail("spotify:artist:a")));
        library.filter.choose(Chip::Kind(ReleaseKind::Ep));

        library.open(artist("spotify:artist:b"));

        let releases = vec![takofy_model::Release {
            kind: ReleaseKind::Ep,
            ..release("spotify:album:b-ep")
        }];
        let row = crate::discography::chips(&releases);
        assert!(
            row.contains(&Chip::Kind(ReleaseKind::Ep)),
            "B has an EP, so its row offers the chip the choice named: {row:?}"
        );
        assert_eq!(
            library.filter.effective(&row),
            Chip::All,
            "B still draws All pressed, because the choice belonged to A"
        );
    }

    #[test]
    fn artist_page_hands_out_the_filter() {
        let mut library = Library::default();
        library.open(artist("spotify:artist:a"));
        library.filter.choose(Chip::Kind(ReleaseKind::Single));
        {
            let page = library.artist_page();
            assert_eq!(page.filter.chosen(), Chip::Kind(ReleaseKind::Single));
            page.filter.choose(Chip::All);
        }
        assert_eq!(
            library.filter.chosen(),
            Chip::All,
            "the page borrows the library's own filter, not a copy"
        );
    }

    #[test]
    fn to_playlists_clears_the_artist_want_and_drops_an_unsettled_load() {
        let mut loading = Library::default();
        loading.open(artist("spotify:artist:a"));
        loading.to_playlists();
        assert_eq!(loading.artist, Load::Idle, "a load nobody wants is dropped");
        assert_eq!(
            loading.open(artist("spotify:artist:a")),
            Some(artist_command("spotify:artist:a")),
            "the want was cleared, so the artist is asked for again"
        );

        let mut failed = Library::default();
        failed.open(artist("spotify:artist:a"));
        failed.apply(&Update::ArtistFailed {
            artist: "spotify:artist:a".to_owned(),
            failure: Failure::Load,
        });
        failed.to_playlists();
        assert_eq!(failed.artist, Load::Idle);

        let mut ready = Library::default();
        ready.open(artist("spotify:artist:a"));
        ready.apply(&Update::Artist(artist_detail("spotify:artist:a")));
        ready.to_playlists();
        assert_eq!(
            ready.artist,
            Load::Ready(artist_detail("spotify:artist:a")),
            "a settled artist is kept for the page Back returns to"
        );
        assert_eq!(
            ready.open(artist("spotify:artist:a")),
            Some(artist_command("spotify:artist:a")),
            "the want is cleared even when the artist stays ready"
        );
    }

    #[test]
    fn to_playlists_resets_the_follow_state() {
        let mut library = Library::default();
        library.open(artist("spotify:artist:a"));
        library.follow.apply(&Update::Followed {
            artist: "spotify:artist:a".to_owned(),
            followed: true,
        });
        assert_eq!(library.follow.label(), "Following");

        library.to_playlists();

        assert_eq!(library.follow.label(), "Follow");
        assert_eq!(library.follow.artist(), None);
    }

    #[test]
    fn current_artist_is_none_while_loading_or_for_another_uri() {
        let mut library = Library::default();
        library.open(artist("spotify:artist:a"));
        assert_eq!(library.current_artist(), None);

        library.artist = Load::Ready(artist_detail("spotify:artist:b"));
        assert_eq!(
            library.current_artist(),
            None,
            "B's detail must not name A's page"
        );

        library.artist = Load::Ready(artist_detail("spotify:artist:a"));
        assert_eq!(
            library.current_artist(),
            Some(&artist_detail("spotify:artist:a"))
        );
    }

    #[test]
    fn a_playlist_prune_leaves_the_artist_alone() {
        let mut library = Library::default();
        library.apply(&Update::Playlists(vec![playlist("p", "P")]));
        library.select("p");
        library.open(artist("spotify:artist:a"));
        library.apply(&Update::Artist(artist_detail("spotify:artist:a")));

        library.apply(&Update::Playlists(vec![playlist("q", "Q")]));

        assert_eq!(library.page(), Some(&artist("spotify:artist:a")));
        assert_eq!(
            library.artist,
            Load::Ready(artist_detail("spotify:artist:a")),
            "a playlist prune never touches the artist"
        );
    }

    #[test]
    fn an_artist_reply_retains_only_its_own_releases() {
        let mut library = Library::default();
        library.open(artist("spotify:artist:a"));
        library.apply(&Update::Artist(artist_detail("spotify:artist:a")));

        // R1 is asked for, so its entry is in flight rather than idle: that is
        // what makes a dropped entry distinguishable from a retained one below.
        library.releases.look("spotify:album:r1");
        assert_eq!(
            library.releases.requests(),
            vec![Command::LoadReleaseTracks {
                release: "spotify:album:r1".to_owned(),
                tracks: vec!["spotify:album:r1".to_owned()],
            }]
        );

        // Coming back to the artist reloads it; the reply no longer names R1.
        library.open(artist("spotify:artist:b"));
        library.open(artist("spotify:artist:a"));
        let mut other = artist_detail("spotify:artist:a");
        other.releases = vec![release("spotify:album:r2")];
        library.apply(&Update::Artist(other));

        assert_eq!(
            library.releases.look("spotify:album:r1"),
            crate::releases::Labels::Idle,
            "a release the artist no longer names is forgotten"
        );
        library.releases.look("spotify:album:r2");
        assert_eq!(
            library.releases.requests(),
            vec![Command::LoadReleaseTracks {
                release: "spotify:album:r2".to_owned(),
                tracks: vec!["spotify:album:r2".to_owned()],
            }]
        );
    }

    #[test]
    fn a_release_reply_reaches_the_release_store() {
        let mut library = Library::default();
        library.open(artist("spotify:artist:a"));
        library.apply(&Update::Artist(artist_detail("spotify:artist:a")));
        library.releases.look("spotify:album:r1");
        let _ = library.releases.requests();

        library.apply(&Update::ReleaseTracks {
            release: "spotify:album:r1".to_owned(),
            tracks: vec![track("t")],
        });

        assert!(matches!(
            library.releases.look("spotify:album:r1"),
            crate::releases::Labels::Ready(_)
        ));
    }

    #[test]
    fn band_cover_waits_until_the_tracks_settle() {
        let cover = cover_url("context");
        let entry = playlist_with_cover("a", "A", cover.clone());
        assert_eq!(band_cover(&entry, false), BandCover::Pending);
        assert_eq!(band_cover(&entry, true), BandCover::Entry(Some(&cover)));
    }

    #[test]
    fn band_cover_is_missing_when_the_entry_names_none() {
        let entry = playlist("a", "A");
        assert_eq!(band_cover(&entry, true), BandCover::Entry(None));
    }

    #[test]
    fn a_failed_context_still_settles_the_band_on_the_entry() {
        // The band's rule is the load's settle state, not the context's success:
        // a failed context settles onto whatever the entry already holds.
        let cover = cover_url("first-track");
        let entry = playlist_with_cover("a", "A", cover.clone());
        let mut library = Library::default();
        library.apply(&Update::Playlists(vec![entry]));
        library.select("a");
        library.apply(&Update::TracksFailed {
            playlist: "a".to_owned(),
            failure: Failure::Load,
        });
        let settled = !matches!(library.tracks, Load::Loading | Load::Idle);
        let entry = library.selected_entry().expect("the selected entry");
        assert_eq!(band_cover(entry, settled), BandCover::Entry(Some(&cover)));
    }

    #[test]
    fn band_cover_handles_an_empty_playlist() {
        let mut library = Library::default();
        library.select("a");
        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: Vec::new(),
            cover: None,
            access: None,
        });
        assert_eq!(library.tracks, Load::Ready(tracks_for("a", Vec::new())));
        let settled = !matches!(library.tracks, Load::Loading | Load::Idle);
        // With no list loaded there is no entry to read; the band's caller draws
        // nothing. This pins the settled rule for the empty list itself.
        assert!(settled);
        assert!(library.selected_entry().is_none());
    }

    #[test]
    fn the_band_waits_instead_of_drawing_the_dropped_cover_as_a_hole() {
        let first = cover_url("first");
        let mut library = page_dropped_by_an_add(Some(&first));
        assert!(
            library.tracks_request().is_some(),
            "arrange: the page is being read again"
        );

        let entry = library.selected_entry().expect("the selected entry");
        assert_eq!(
            band_cover(entry, settled(&library)),
            BandCover::Pending,
            "a cover being read waits, so the open band never draws a hole"
        );
    }

    #[test]
    fn the_band_draws_the_cover_the_re_read_brought() {
        let first = cover_url("first");
        let mut library = page_dropped_by_an_add(Some(&first));
        assert!(library.tracks_request().is_some(), "arrange: the re-read");

        let fresh = cover_url("fresh");
        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: moved(&["spotify:track:x"]),
            cover: Some(fresh.clone()),
            access: access(0x08, true),
        });

        let entry = library.selected_entry().expect("the selected entry");
        assert_eq!(
            band_cover(entry, settled(&library)),
            BandCover::Entry(Some(&fresh)),
            "once the read lands the band draws the cover it brought"
        );
    }

    #[test]
    fn an_emptied_playlist_leaves_the_band_on_the_entry_with_no_cover() {
        let first = cover_url("first");
        let mut library = covered_page(&first, &["spotify:track:t0"]);
        assert!(
            library
                .remove_tracks("a", &[0], &moved(&["spotify:track:t0"]))
                .is_some()
        );
        library.apply(&Update::TracksRemoved {
            playlist: "a".to_owned(),
            revision: revision(0x02),
        });
        assert!(library.tracks_request().is_some(), "arrange: the re-ask");

        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: Vec::new(),
            cover: None,
            access: access(0x02, true),
        });

        let entry = library.selected_entry().expect("the selected entry");
        assert_eq!(
            band_cover(entry, settled(&library)),
            BandCover::Entry(None),
            "an emptied playlist has no cover, and the band says so rather than waiting"
        );
    }
    #[test]
    fn a_removal_issues_one_command_with_the_chain_and_the_held_revision() {
        let mut library = page_with("a", &["t0", "t1", "t2", "t3", "t4"], access(0x01, true));

        assert_eq!(
            library.remove_tracks("a", &[1, 3], &moved(&["t1", "t3"])),
            Some(Command::RemoveTracks {
                playlist: "a".to_owned(),
                revision: revision(0x01),
                removals: vec![
                    Removal { from: 3, length: 1 },
                    Removal { from: 1, length: 1 },
                ],
            })
        );
    }

    #[test]
    fn a_removal_applies_the_rows_at_once_and_records_pending() {
        let mut library = page_with("a", &["t0", "t1", "t2", "t3"], access(0x01, true));

        assert!(
            library
                .remove_tracks("a", &[1, 2], &moved(&["t1", "t2"]))
                .is_some()
        );
        assert_eq!(
            library.tracks,
            ready("a", &["t0", "t3"], access(0x01, true)),
            "the rows are gone before the write is answered"
        );
        assert_eq!(
            library.pending,
            Some(reorder::Pending {
                playlist: "a".to_owned(),
                before: moved(&["t0", "t1", "t2", "t3"]),
                after: moved(&["t0", "t3"]),
            })
        );
    }

    #[test]
    fn a_removal_waits_while_a_page_write_is_pending() {
        let mut library = page_with("a", &["t0", "t1", "t2"], access(0x01, true));
        assert!(library.remove_tracks("a", &[0], &moved(&["t0"])).is_some());

        assert_eq!(
            library.remove_tracks("a", &[0], &moved(&["t1"])),
            None,
            "one gesture is one write, so a second waits for the reply"
        );
        assert!(!library.removable());
    }

    #[test]
    fn a_removal_waits_while_a_sidebar_write_is_pending() {
        let mut library = page_with("a", &["t0", "t1", "t2"], access(0x01, true));
        library.playlists = Load::Ready(vec![
            with_access(playlist("a", "a"), 0x07, true, None),
            with_access(playlist("b", "b"), 0x07, true, None),
        ]);
        library.apply(&Update::RootlistAccess(rootlist_access(0x07, true)));
        assert!(library.move_playlist(0, 2, "a").is_some());

        assert_eq!(
            library.remove_tracks("a", &[0], &moved(&["t0"])),
            None,
            "a sidebar write holds every list still"
        );
    }

    #[test]
    fn a_removal_refuses_a_read_only_page() {
        let mut library = page_with("a", &["t0", "t1"], access(0x01, false));

        assert_eq!(library.remove_tracks("a", &[0], &moved(&["t0"])), None);
        assert!(!library.removable());
        assert_eq!(library.pending, None);
        assert_eq!(
            library.tracks,
            ready("a", &["t0", "t1"], access(0x01, false))
        );
    }

    #[test]
    fn a_removal_refuses_a_stale_pick() {
        let mut library = page_with("a", &["t0", "t1"], access(0x01, true));

        assert_eq!(
            library.remove_tracks("a", &[0], &moved(&["gone"])),
            None,
            "the row holds another entry now"
        );
        assert_eq!(
            library.remove_tracks("a", &[0, 1], &moved(&["t0"])),
            None,
            "the rows and the entries are not parallel"
        );
        assert_eq!(
            library.remove_tracks("a", &[9], &moved(&["gone"])),
            None,
            "a row the list does not hold"
        );
        assert_eq!(library.pending, None);
    }

    #[test]
    fn a_removal_refuses_an_unsorted_pick() {
        let mut library = page_with("a", &["t0", "t1"], access(0x01, true));

        assert_eq!(
            library.remove_tracks("a", &[1, 0], &moved(&["t1", "t0"])),
            None
        );
        assert_eq!(
            library.remove_tracks("a", &[0, 0], &moved(&["t0", "t0"])),
            None,
            "a repeated row is not a pick"
        );
        assert_eq!(library.pending, None);
    }

    #[test]
    fn the_reply_revision_ends_the_write_and_the_next_removal_uses_it() {
        let mut library = page_with("a", &["t0", "t1", "t2"], access(0x01, true));
        assert!(library.remove_tracks("a", &[1], &moved(&["t1"])).is_some());

        library.apply(&Update::TracksRemoved {
            playlist: "a".to_owned(),
            revision: revision(0x02),
        });

        assert_eq!(library.pending, None);
        assert_eq!(
            library.tracks,
            ready("a", &["t0", "t2"], access(0x02, true))
        );
        assert_eq!(
            library.remove_tracks("a", &[1], &moved(&["t2"])),
            Some(Command::RemoveTracks {
                playlist: "a".to_owned(),
                revision: revision(0x02),
                removals: vec![Removal { from: 1, length: 1 }],
            })
        );
    }

    #[test]
    fn the_reply_lowers_the_sidebar_count_by_the_rows_removed() {
        let mut library = page_with("a", &["t0", "t1", "t2", "t3"], access(0x01, true));
        library.playlists =
            Load::Ready(vec![with_access(playlist("a", "a"), 0x07, true, Some(12))]);
        assert!(
            library
                .remove_tracks("a", &[1, 2], &moved(&["t1", "t2"]))
                .is_some()
        );

        library.apply(&Update::TracksRemoved {
            playlist: "a".to_owned(),
            revision: revision(0x02),
        });

        assert_eq!(
            sidebar_entry(&library, "a").and_then(|entry| entry.tracks),
            Some(10)
        );
    }

    #[test]
    fn a_move_reply_leaves_the_count_alone() {
        let mut library = page_with("a", &["t0", "t1", "t2"], access(0x01, true));
        library.playlists =
            Load::Ready(vec![with_access(playlist("a", "a"), 0x07, true, Some(12))]);
        assert!(move_one(&mut library, "a", 0, 2, &track("t0")).is_some());

        library.apply(&Update::TrackMoved {
            playlist: "a".to_owned(),
            revision: revision(0x02),
        });

        assert_eq!(
            sidebar_entry(&library, "a").and_then(|entry| entry.tracks),
            Some(12),
            "a move changes the order, never the count"
        );
    }

    #[test]
    fn a_failed_removal_restores_the_rows_and_says_so() {
        let mut library = page_with("a", &["t0", "t1", "t2"], access(0x01, true));
        assert!(library.remove_tracks("a", &[1], &moved(&["t1"])).is_some());

        library.apply(&Update::RemoveFailed {
            playlist: "a".to_owned(),
            failure: Failure::Save,
        });

        assert_eq!(library.pending, None);
        assert_eq!(
            library.tracks,
            Load::Ready(TracksFor {
                playlist: "a".to_owned(),
                tracks: moved(&["t0", "t1", "t2"]),
                access: access(0x01, true),
                notice: Some(Failure::Save),
            })
        );
    }

    #[test]
    fn a_failed_removal_leaves_the_sidebar_count_alone() {
        let mut library = page_with("a", &["t0", "t1"], access(0x01, true));
        library.playlists = Load::Ready(vec![with_access(playlist("a", "a"), 0x07, true, Some(7))]);
        assert!(library.remove_tracks("a", &[1], &moved(&["t1"])).is_some());

        library.apply(&Update::RemoveFailed {
            playlist: "a".to_owned(),
            failure: Failure::Save,
        });

        assert_eq!(
            sidebar_entry(&library, "a").and_then(|entry| entry.tracks),
            Some(7)
        );
    }

    #[test]
    fn a_read_while_a_removal_is_pending_keeps_the_removed_order() {
        let mut library = page_with("a", &["t0", "t1", "t2"], access(0x01, true));
        assert!(library.remove_tracks("a", &[1], &moved(&["t1"])).is_some());

        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: moved(&["t0", "t1", "t2"]),
            cover: None,
            access: access(0x01, true),
        });

        assert_eq!(
            library.tracks,
            ready("a", &["t0", "t2"], access(0x01, true)),
            "a read issued before the write landed must not resurrect the rows"
        );
    }

    #[test]
    fn a_removal_never_drops_the_page() {
        let mut library = page_with("a", &["t0", "t1", "t2"], access(0x01, true));
        assert!(library.remove_tracks("a", &[1], &moved(&["t1"])).is_some());
        assert!(
            matches!(library.tracks, Load::Ready(_)),
            "the issue keeps the page ready, so nothing has to be re-requested"
        );

        library.apply(&Update::TracksRemoved {
            playlist: "a".to_owned(),
            revision: revision(0x02),
        });
        assert!(matches!(library.tracks, Load::Ready(_)));

        library.apply(&Update::RemoveFailed {
            playlist: "a".to_owned(),
            failure: Failure::Save,
        });
        assert!(matches!(library.tracks, Load::Ready(_)));
    }

    #[test]
    fn removable_matches_movable() {
        for (uris, access) in [
            (&["t0", "t1"][..], access(0x01, true)),
            (&["t0", "t1"][..], access(0x01, false)),
            (&["t0", "t1"][..], None),
        ] {
            let library = page_with("a", uris, access);
            assert_eq!(library.removable(), library.movable(), "{uris:?}");
        }

        let mut writing = page_with("a", &["t0", "t1"], access(0x01, true));
        assert!(writing.remove_tracks("a", &[0], &moved(&["t0"])).is_some());
        assert_eq!(writing.removable(), writing.movable());
        assert!(!writing.removable());
    }

    #[test]
    fn an_emptied_playlist_stays_ready_and_empty() {
        let mut library = page_with("a", &["t0", "t1"], access(0x01, true));

        assert!(
            library
                .remove_tracks("a", &[0, 1], &moved(&["t0", "t1"]))
                .is_some()
        );
        assert_eq!(library.tracks, ready("a", &[], access(0x01, true)));

        library.apply(&Update::TracksRemoved {
            playlist: "a".to_owned(),
            revision: revision(0x02),
        });
        assert_eq!(library.tracks, ready("a", &[], access(0x02, true)));
        assert_eq!(library.selected(), Some("a"));
        assert!(library.removable(), "an empty editable list still rewrites");
    }

    /// A sidebar holding `names`, with the head's field open on `name`.
    fn naming_library(names: &[&str], name: &str) -> Library {
        let mut library = sidebar_library(names, 0x07, true, Some(3));
        library.naming.open();
        library.naming.edit().push_str(name);
        library
    }

    /// The row a create's reply hands the sidebar.
    fn created(uri: &str, name: &str, access: Option<PlaylistAccess>) -> PlaylistEntry {
        PlaylistEntry {
            uri: uri.to_owned(),
            name: name.to_owned(),
            named: true,
            cover: None,
            description: None,
            tracks: Some(0),
            owner: None,
            access,
        }
    }

    #[test]
    fn creatable_needs_ready_playlists_a_rootlist_revision_and_no_write() {
        assert!(sidebar_library(&["a"], 0x07, true, None).creatable());

        for playlists in [Load::Idle, Load::Loading, Load::Failed(Failure::Load)] {
            let mut library = sidebar_library(&["a"], 0x07, true, None);
            library.playlists = playlists;
            assert!(
                !library.creatable(),
                "a list that is not ready to insert into"
            );
        }

        let mut no_rootlist = sidebar_library(&["a"], 0x07, true, None);
        no_rootlist.rootlist = None;
        assert!(!no_rootlist.creatable(), "no revision to link against");

        let mut writing = sidebar_library(&["a", "b"], 0x07, true, None);
        assert!(writing.move_playlist(0, 2, "a").is_some());
        assert!(!writing.creatable(), "a second write for one frame");
    }

    #[test]
    fn creatable_does_not_need_editable() {
        // The walk skips a folder marker and leaves the access uneditable, which
        // says nothing about whether index 0 of the rootlist can take an add.
        let library = sidebar_library(&["a"], 0x07, false, None);
        assert!(!library.rootlist_movable());
        assert!(library.creatable());
    }

    #[test]
    fn create_issues_one_command_naming_the_held_revision_and_the_trimmed_name() {
        let mut library = naming_library(&["a"], "  Night  Drive ");

        assert_eq!(
            library.create_playlist(),
            Some(Command::CreatePlaylist {
                name: "Night  Drive".to_owned(),
                revision: revision(0x07),
                tracks: Vec::new(),
            })
        );
        assert!(library.naming.writing());
        assert!(library.write_in_flight());
    }

    #[test]
    fn create_is_refused_when_naming_is_closed_blank_or_already_writing() {
        let mut closed = sidebar_library(&["a"], 0x07, true, None);
        assert_eq!(closed.create_playlist(), None, "a closed field wrote");

        let mut blank = naming_library(&["a"], "   ");
        assert_eq!(blank.create_playlist(), None);
        assert!(blank.naming.is_open(), "a blank name closed the field");
        assert!(!blank.write_in_flight());

        let mut writing = naming_library(&["a"], "Mix");
        assert!(writing.create_playlist().is_some());
        assert_eq!(writing.create_playlist(), None, "a second write");
    }

    #[test]
    fn create_waits_while_any_write_is_pending() {
        let mut library = naming_library(&["a", "b"], "Mix");
        assert!(library.add_tracks(1, &pick(&["spotify:track:x"])).is_some());
        assert_eq!(library.create_playlist(), None, "an add was in flight");
        assert!(!library.naming.writing());
    }

    #[test]
    fn a_create_names_the_pick_the_field_holds() {
        let mut library = naming_library(&["a"], "Mix");
        let tracks = pick(&["spotify:track:x", "spotify:track:y"]);
        library.naming.open_with(tracks.clone());

        assert_eq!(
            library.create_playlist(),
            Some(Command::CreatePlaylist {
                name: "Mix".to_owned(),
                revision: revision(0x07),
                tracks,
            })
        );
    }

    #[test]
    fn a_created_list_that_took_the_pick_counts_it() {
        let mut library = naming_library(&["a"], "Mix");
        library.naming.open_with(pick(&[
            "spotify:track:x",
            "spotify:track:y",
            "spotify:track:z",
        ]));
        assert!(library.create_playlist().is_some());
        let landed = PlaylistEntry {
            tracks: Some(3),
            ..created("spotify:playlist:new", "Mix", access(0x09, true))
        };

        library.apply(&Update::PlaylistCreated {
            entry: landed,
            revision: revision(0x08),
        });

        let entry = sidebar_entry(&library, "spotify:playlist:new").expect("the new row");
        assert_eq!(entry.tracks, Some(3), "the row counts what landed");
        assert_eq!(library.sidebar_notice, None);
    }

    #[test]
    fn a_created_list_that_lost_the_pick_says_so() {
        let mut library = naming_library(&["a"], "Mix");
        library
            .naming
            .open_with(pick(&["spotify:track:x", "spotify:track:y"]));
        assert!(library.create_playlist().is_some());
        library.apply(&Update::PlaylistCreated {
            entry: created("spotify:playlist:new", "Mix", access(0x09, true)),
            revision: revision(0x08),
        });

        library.apply(&Update::PlaylistUnfilled {
            playlist: "spotify:playlist:new".to_owned(),
        });

        let entry = sidebar_entry(&library, "spotify:playlist:new").expect("the new row");
        assert_eq!(entry.tracks, Some(0), "the row counts what landed");
        assert_eq!(library.sidebar_notice, Some(SidebarWrite::Unfilled));
        assert!(!library.naming.is_open(), "the field is spent");
        assert!(!library.write_in_flight());
    }

    #[test]
    fn a_created_list_that_lost_its_name_says_so() {
        let mut library = naming_library(&["a"], "Mix");
        assert!(library.create_playlist().is_some());
        library.apply(&Update::PlaylistCreated {
            entry: PlaylistEntry {
                // The rename never landed, so the row does not claim a name.
                named: false,
                ..created("spotify:playlist:new", "Mix", access(0x09, true))
            },
            revision: revision(0x08),
        });

        library.apply(&Update::PlaylistUnnamed {
            playlist: "spotify:playlist:new".to_owned(),
        });

        assert_eq!(library.sidebar_notice, Some(SidebarWrite::Unnamed));
        let entry = sidebar_entry(&library, "spotify:playlist:new").expect("the new row");
        assert_eq!(entry.name, "Mix", "the typed name stays for this session");
        assert!(!library.write_in_flight());
    }

    #[test]
    fn an_unnamed_notice_for_a_list_not_held_is_ignored() {
        let mut library = sidebar_library(&["a"], 0x07, true, Some(3));
        library.apply(&Update::PlaylistUnnamed {
            playlist: "spotify:playlist:absent".to_owned(),
        });
        assert_eq!(library.sidebar_notice, None);
    }

    #[test]
    fn an_unfilled_notice_for_a_list_not_held_is_ignored() {
        let mut library = sidebar_library(&["a"], 0x07, true, Some(3));

        library.apply(&Update::PlaylistUnfilled {
            playlist: "spotify:playlist:missing".to_owned(),
        });

        assert_eq!(library.sidebar_notice, None);
        assert_eq!(
            sidebar_entry(&library, "a").and_then(|entry| entry.tracks),
            Some(3)
        );
    }

    #[test]
    fn a_create_failure_keeps_the_pick_and_the_text() {
        let mut library = naming_library(&["a"], "Mix");
        let tracks = pick(&["spotify:track:x"]);
        library.naming.open_with(tracks.clone());
        assert!(library.create_playlist().is_some());

        library.apply(&Update::PlaylistCreateFailed);

        assert_eq!(library.naming.text(), "Mix");
        assert_eq!(library.naming.pick(), tracks, "a retry lands the same pick");
        assert!(library.naming.is_open());
        assert!(!library.naming.writing());
    }

    #[test]
    fn a_pending_create_blocks_moves_adds_and_menus() {
        let mut library = naming_library(&["a", "b"], "Mix");
        assert!(library.create_playlist().is_some());

        assert!(library.write_in_flight(), "the menu's gate stayed open");
        assert!(!library.rootlist_movable());
        assert_eq!(library.move_playlist(0, 1, "a"), None);
        assert_eq!(library.add_tracks(0, &pick(&["spotify:track:x"])), None);
        assert_eq!(sidebar_order(&library), names(&["a", "b"]), "a row moved");
    }

    #[test]
    fn a_confirmed_create_inserts_at_index_zero_and_keeps_the_rest_in_order() {
        let mut library = naming_library(&["a", "b"], "Mix");
        assert!(library.create_playlist().is_some());

        library.apply(&Update::PlaylistCreated {
            entry: created("spotify:playlist:new", "Mix", access(0x09, true)),
            revision: revision(0x08),
        });

        assert_eq!(
            sidebar_order(&library),
            names(&["spotify:playlist:new", "a", "b"])
        );
    }

    #[test]
    fn a_confirmed_create_on_an_empty_ready_list_is_the_only_row() {
        let mut library = naming_library(&[], "Mix");
        assert!(library.create_playlist().is_some());

        library.apply(&Update::PlaylistCreated {
            entry: created("spotify:playlist:new", "Mix", None),
            revision: revision(0x08),
        });

        assert_eq!(sidebar_order(&library), names(&["spotify:playlist:new"]));
    }

    #[test]
    fn a_confirmed_create_does_not_duplicate_a_uri_already_listed() {
        let mut library = naming_library(&["a"], "Mix");
        assert!(library.create_playlist().is_some());

        library.apply(&Update::PlaylistCreated {
            entry: created("a", "Mix", access(0x09, true)),
            revision: revision(0x08),
        });

        assert_eq!(sidebar_order(&library), names(&["a"]));
    }

    #[test]
    fn a_confirmed_create_holds_the_link_replys_revision_and_the_next_create_names_it() {
        let mut library = naming_library(&["a"], "Mix");
        assert!(library.create_playlist().is_some());
        library.apply(&Update::PlaylistCreated {
            entry: created("spotify:playlist:new", "Mix", None),
            revision: revision(0x08),
        });

        library.naming.open();
        library.naming.edit().push_str("Again");
        assert_eq!(
            library.create_playlist(),
            Some(Command::CreatePlaylist {
                name: "Again".to_owned(),
                revision: revision(0x08),
                tracks: Vec::new(),
            }),
            "the second create named the revision the first reply held"
        );
    }

    #[test]
    fn a_confirmed_create_closes_naming_clears_the_notice_and_sets_the_reveal() {
        let mut library = naming_library(&["a"], "Mix");
        assert!(library.create_playlist().is_some());
        library.sidebar_notice = Some(SidebarWrite::Reorder);

        library.apply(&Update::PlaylistCreated {
            entry: created("spotify:playlist:new", "Mix", None),
            revision: revision(0x08),
        });

        assert!(!library.naming.is_open());
        assert!(!library.naming.writing());
        assert_eq!(library.naming.text(), "");
        assert!(
            library.naming.wants_button(),
            "the keyboard did not go back"
        );
        assert_eq!(library.sidebar_notice, None);
        assert!(library.reveal_top);
        assert!(!library.write_in_flight());
    }

    #[test]
    fn a_confirmed_create_switches_the_chip_to_playlists() {
        let mut library = naming_library(&["a"], "Mix");
        assert!(library.sidebar_kind.choose(SidebarKind::Albums));
        assert!(library.create_playlist().is_some());

        library.apply(&Update::PlaylistCreated {
            entry: created("spotify:playlist:new", "Mix", None),
            revision: revision(0x08),
        });

        assert_eq!(library.sidebar_kind.chosen(), SidebarKind::Playlists);
    }

    #[test]
    fn a_confirmed_create_never_leaves_the_list_idle() {
        let mut library = naming_library(&["a"], "Mix");
        assert!(library.create_playlist().is_some());

        library.apply(&Update::PlaylistCreated {
            entry: created("spotify:playlist:new", "Mix", None),
            revision: revision(0x08),
        });

        assert!(
            matches!(library.playlists, Load::Ready(_)),
            "a created row must never leave the sidebar waiting"
        );
    }

    #[test]
    fn a_created_reply_with_no_ready_list_is_ignored() {
        let mut library = naming_library(&["a"], "Mix");
        assert!(library.create_playlist().is_some());
        library.playlists = Load::Loading;

        library.apply(&Update::PlaylistCreated {
            entry: created("spotify:playlist:new", "Mix", access(0x09, true)),
            revision: revision(0x08),
        });

        assert!(
            matches!(library.playlists, Load::Loading),
            "a read in flight was replaced by the created row's list"
        );
        assert!(!library.write_in_flight(), "the create stayed pending");
    }

    #[test]
    fn a_playlists_read_during_a_create_is_not_adopted_and_its_access_goes_with_it() {
        let mut library = naming_library(&["a", "b"], "Mix");
        assert!(library.create_playlist().is_some());

        library.apply(&Update::Playlists(vec![playlist("c", "c")]));
        library.apply(&Update::RootlistAccess(rootlist_access(0x0f, true)));

        assert_eq!(
            sidebar_order(&library),
            names(&["a", "b"]),
            "a read taken before the link landed replaced the list"
        );
        assert_eq!(
            library
                .rootlist
                .as_ref()
                .map(|access| access.revision.clone()),
            Some(revision(0x07)),
            "the dropped read's revision was adopted"
        );
    }

    #[test]
    fn a_rootlist_access_during_a_create_is_not_adopted() {
        let mut library = naming_library(&["a"], "Mix");
        assert!(library.create_playlist().is_some());

        library.apply(&Update::RootlistAccess(rootlist_access(0x0f, false)));

        let held = library.rootlist.clone().expect("the held access");
        assert_eq!(held.revision, revision(0x07));
        assert!(held.editable, "the access was replaced mid-create");
    }

    #[test]
    fn a_selected_playlist_stays_selected_after_the_insert() {
        let mut library = naming_library(&["a", "b"], "Mix");
        library.select("b");
        assert!(library.create_playlist().is_some());

        library.apply(&Update::PlaylistCreated {
            entry: created("spotify:playlist:new", "Mix", None),
            revision: revision(0x08),
        });

        assert_eq!(library.selected(), Some("b"), "the pick follows the URI");
    }

    #[test]
    fn a_make_failure_says_create_and_keeps_the_text() {
        let mut library = naming_library(&["a"], "Night Drive");
        assert!(library.create_playlist().is_some());

        library.apply(&Update::PlaylistCreateFailed);

        assert_eq!(library.sidebar_notice, Some(SidebarWrite::Create));
        assert!(library.naming.is_open());
        assert!(!library.naming.writing());
        assert!(library.naming.wants_focus(), "the retry had no keyboard");
        assert_eq!(library.naming.text(), "Night Drive");
        assert_eq!(sidebar_order(&library), names(&["a"]), "a row appeared");
        assert!(!library.write_in_flight());
    }

    #[test]
    fn a_link_failure_says_link_and_closes_naming() {
        let mut library = naming_library(&["a"], "Night Drive");
        assert!(library.create_playlist().is_some());

        library.apply(&Update::PlaylistUnlinked);

        assert_eq!(library.sidebar_notice, Some(SidebarWrite::Link));
        assert!(!library.naming.is_open());
        assert_eq!(library.naming.text(), "");
        assert_eq!(sidebar_order(&library), names(&["a"]), "a row appeared");
        assert!(!library.write_in_flight());
    }

    #[test]
    fn a_link_failure_asks_for_a_fresh_read() {
        let mut library = naming_library(&["a"], "Night Drive");
        assert!(library.create_playlist().is_some());

        library.apply(&Update::PlaylistUnlinked);

        assert!(
            library.rootlist.is_none(),
            "a revision from before the link is one the next write cannot name"
        );
        library.naming.open();
        library.naming.edit().push_str("Again");
        assert_eq!(library.create_playlist(), None, "a write on no revision");
        assert_eq!(library.rootlist_request(), Some(Command::ReloadLibrary));
        assert_eq!(library.rootlist_request(), None, "a second read");
    }

    #[test]
    fn a_stray_reply_with_nothing_pending_is_ignored() {
        let mut library = sidebar_library(&["a"], 0x07, true, None);

        library.apply(&Update::PlaylistCreated {
            entry: created("spotify:playlist:new", "Mix", None),
            revision: revision(0x08),
        });
        library.apply(&Update::PlaylistCreateFailed);
        library.apply(&Update::PlaylistUnlinked);

        assert_eq!(sidebar_order(&library), names(&["a"]));
        assert_eq!(library.sidebar_notice, None);
        assert_eq!(
            library
                .rootlist
                .as_ref()
                .map(|access| access.revision.clone()),
            Some(revision(0x07))
        );
    }

    #[test]
    fn a_new_gesture_clears_the_create_sentence() {
        let mut library = naming_library(&["a", "b"], "Mix");
        assert!(library.create_playlist().is_some());
        library.apply(&Update::PlaylistCreateFailed);
        assert_eq!(library.sidebar_notice, Some(SidebarWrite::Create));

        assert!(library.move_playlist(0, 2, "a").is_some());
        assert_eq!(library.sidebar_notice, None);
    }

    #[test]
    fn the_created_entry_is_addable_when_the_state_read_named_an_editable_access() {
        let mut library = naming_library(&[], "Mix");
        assert!(library.create_playlist().is_some());
        library.apply(&Update::PlaylistCreated {
            entry: created("spotify:playlist:new", "Mix", access(0x09, true)),
            revision: revision(0x08),
        });

        assert!(library.addable(0), "the new row cannot take a track");
        let entry = sidebar_entry(&library, "spotify:playlist:new").expect("the new row");
        assert_eq!(entry.tracks, Some(0));
        assert_eq!(entry.add_basis().map(|(_, count)| count), Some(0));
    }

    #[test]
    fn the_created_entry_is_not_addable_without_an_editable_access() {
        let mut library = naming_library(&[], "Mix");
        assert!(library.create_playlist().is_some());
        library.apply(&Update::PlaylistCreated {
            entry: created("spotify:playlist:new", "Mix", None),
            revision: revision(0x08),
        });

        assert!(!library.addable(0), "a row with no access took a track");
    }

    /// A sidebar row as a menu opened on it describes it.
    fn item(kind: SidebarKind, uri: &str) -> LibraryItem {
        LibraryItem {
            kind,
            uri: uri.to_owned(),
            name: uri.to_owned(),
        }
    }

    /// A library whose four lists are ready and hold one thing each.
    fn saved_library() -> Library {
        let mut library = Library::default();
        library.apply(&Update::Playlists(Vec::new()));
        library.apply(&Update::FollowedShows(vec![followed_show(
            "spotify:show:s",
        )]));
        library.apply(&Update::FollowedArtists(vec![followed_artist(
            "spotify:artist:a",
        )]));
        library.apply(&Update::SavedAlbums(vec![saved_album("spotify:album:r")]));
        library
    }

    #[test]
    fn a_playlist_removal_names_the_row_and_the_rootlist_revision() {
        let mut library = sidebar_library(&["a", "b"], 0x07, true, None);

        let command = library.remove_from_library(&item(SidebarKind::Playlists, "b"));

        assert_eq!(
            command,
            Some(Command::RemovePlaylist {
                playlist: "b".to_owned(),
                revision: revision(0x07),
                index: 1,
            })
        );
        assert_eq!(
            library.sidebar_pending,
            Some(SidebarPending::Remove {
                playlist: "b".to_owned(),
                index: 1,
            })
        );
    }

    #[test]
    fn a_playlist_removal_is_refused_without_leaving_a_pending() {
        let mut no_access = Library::default();
        no_access.apply(&Update::Playlists(vec![playlist("a", "A")]));
        assert_eq!(
            no_access.remove_from_library(&item(SidebarKind::Playlists, "a")),
            None,
            "a read with no revision offered a write"
        );
        assert!(!no_access.write_in_flight());

        let mut not_editable = sidebar_library(&["a"], 0x07, false, None);
        assert_eq!(
            not_editable.remove_from_library(&item(SidebarKind::Playlists, "a")),
            None,
            "a list the user cannot write offered a write"
        );
        assert!(!not_editable.write_in_flight());

        let mut writing = sidebar_library(&["a", "b"], 0x07, true, None);
        assert!(writing.move_playlist(0, 2, "a").is_some());
        assert_eq!(
            writing.remove_from_library(&item(SidebarKind::Playlists, "a")),
            None,
            "a second write went out beside the first"
        );

        let mut gone = sidebar_library(&["b"], 0x07, true, None);
        assert_eq!(
            gone.remove_from_library(&item(SidebarKind::Playlists, "a")),
            None,
            "a row the list does not hold was removed"
        );
        assert!(!gone.write_in_flight());

        let mut loading = sidebar_library(&["a"], 0x07, true, None);
        loading.playlists = Load::Loading;
        assert_eq!(
            loading.remove_from_library(&item(SidebarKind::Playlists, "a")),
            None,
            "a list that is not ready offered a write"
        );
        assert!(!loading.write_in_flight());
    }

    #[test]
    fn a_saved_removal_is_refused_until_its_own_list_is_ready() {
        for (kind, uri) in [
            (SidebarKind::Albums, "spotify:album:r"),
            (SidebarKind::Artists, "spotify:artist:a"),
            (SidebarKind::Podcasts, "spotify:show:s"),
        ] {
            let mut library = saved_library();
            assert!(
                library.remove_from_library(&item(kind, uri)).is_some(),
                "{kind:?} was not removed at all"
            );
            assert_eq!(
                library.remove_from_library(&item(kind, uri)),
                None,
                "{kind:?} sent a second write beside the first"
            );
        }

        let mut library = saved_library();
        library.saved_albums = Load::Idle;
        assert_eq!(
            library.remove_from_library(&item(SidebarKind::Albums, "spotify:album:r")),
            None
        );
        library.followed_artists = Load::Failed(Failure::FollowedArtists);
        assert_eq!(
            library.remove_from_library(&item(SidebarKind::Artists, "spotify:artist:a")),
            None
        );
        library.followed = Load::Loading;
        assert_eq!(
            library.remove_from_library(&item(SidebarKind::Podcasts, "spotify:show:s")),
            None
        );
        assert!(!library.write_in_flight());
    }

    #[test]
    fn an_artist_removal_is_refused_while_its_pages_follow_write_is_pending() {
        let mut library = saved_library();
        library.open(Page::Artist("spotify:artist:a".to_owned()));
        library.apply(&Update::Artist(artist_detail("spotify:artist:a")));
        library.apply(&Update::Followed {
            artist: "spotify:artist:a".to_owned(),
            followed: false,
        });
        assert_eq!(
            library.follow.pressed(),
            Some(Command::SetFollow {
                artist: "spotify:artist:a".to_owned(),
                followed: true,
            })
        );

        assert_eq!(
            library.remove_from_library(&item(SidebarKind::Artists, "spotify:artist:a")),
            None,
            "the removal raced the page's own follow write"
        );
        assert!(!library.write_in_flight());
    }

    #[test]
    fn a_confirmed_playlist_removal_drops_the_row_and_holds_the_reply_revision() {
        let mut library = sidebar_library(&["a", "b"], 0x07, true, None);
        assert!(
            library
                .remove_from_library(&item(SidebarKind::Playlists, "a"))
                .is_some()
        );
        assert_eq!(
            sidebar_order(&library),
            names(&["a", "b"]),
            "the row left before the write was confirmed"
        );

        library.apply(&Update::PlaylistRemoved {
            playlist: "a".to_owned(),
            revision: revision(0x09),
        });

        assert_eq!(sidebar_order(&library), names(&["b"]));
        assert!(!library.write_in_flight());
        assert_eq!(
            library.rootlist.as_ref().map(|held| held.revision.clone()),
            Some(revision(0x09)),
            "the next write must name the reply's revision"
        );
    }

    #[test]
    fn a_confirm_drops_one_row_of_a_duplicate_uri() {
        let mut library = sidebar_library(&["a", "a"], 0x07, true, None);

        assert!(
            library
                .remove_from_library(&item(SidebarKind::Playlists, "a"))
                .is_some()
        );
        library.apply(&Update::PlaylistRemoved {
            playlist: "a".to_owned(),
            revision: revision(0x09),
        });

        assert_eq!(
            sidebar_order(&library),
            names(&["a"]),
            "the confirm dropped every row naming that playlist"
        );
    }

    #[test]
    fn a_confirm_at_a_stale_index_rereads_instead_of_mutating_the_list() {
        let mut shifted = sidebar_library(&["a", "b"], 0x07, true, None);
        assert!(
            shifted
                .remove_from_library(&item(SidebarKind::Playlists, "b"))
                .is_some()
        );
        // The list no longer holds the row at the index the write named, as a
        // replacement list would leave it.
        shifted.playlists = Load::Ready(vec![with_access(playlist("a", "A"), 0x07, true, None)]);
        shifted.apply(&Update::PlaylistRemoved {
            playlist: "b".to_owned(),
            revision: revision(0x09),
        });

        assert_eq!(
            sidebar_order(&shifted),
            names(&["a"]),
            "a removal at a stale index dropped the wrong row"
        );
        assert!(
            shifted.rootlist.is_none(),
            "a stale index kept the held revision"
        );
        assert!(shifted.rootlist_stale, "the moved list owed a fresh read");
        assert_eq!(shifted.rootlist_request(), Some(Command::ReloadLibrary));

        let mut vanished = sidebar_library(&["a"], 0x07, true, None);
        assert!(
            vanished
                .remove_from_library(&item(SidebarKind::Playlists, "a"))
                .is_some()
        );
        vanished.playlists = Load::Ready(Vec::new());
        vanished.apply(&Update::PlaylistRemoved {
            playlist: "a".to_owned(),
            revision: revision(0x09),
        });

        assert_eq!(sidebar_order(&vanished), names(&[]));
        assert!(vanished.rootlist_stale, "a missing row owed a fresh read");
    }

    #[test]
    fn a_follow_up_removal_names_the_shifted_index() {
        let mut library = sidebar_library(&["a", "b", "c"], 0x07, true, None);
        assert!(
            library
                .remove_from_library(&item(SidebarKind::Playlists, "a"))
                .is_some()
        );
        library.apply(&Update::PlaylistRemoved {
            playlist: "a".to_owned(),
            revision: revision(0x09),
        });

        assert_eq!(
            library.remove_from_library(&item(SidebarKind::Playlists, "b")),
            Some(Command::RemovePlaylist {
                playlist: "b".to_owned(),
                revision: revision(0x09),
                index: 0,
            })
        );
    }

    #[test]
    fn removing_the_open_playlist_goes_home_while_an_album_page_survives() {
        let mut library = sidebar_library(&["a", "b"], 0x07, true, None);
        library.open(album("spotify:album:r"));
        library.select("a");
        assert_eq!(library.page(), Some(&Page::Playlist("a".to_owned())));

        assert!(
            library
                .remove_from_library(&item(SidebarKind::Playlists, "a"))
                .is_some()
        );
        library.apply(&Update::PlaylistRemoved {
            playlist: "a".to_owned(),
            revision: revision(0x09),
        });

        assert_eq!(
            library.page(),
            None,
            "the removed page stayed open instead of falling to the root"
        );
        let removed = Page::Playlist("a".to_owned());
        assert!(
            !library.history.pages().any(|page| *page == removed),
            "the removed playlist stayed in the history: {:?}",
            library.history
        );
        assert_eq!(library.wanted, None, "the removed playlist stayed wanted");
        assert!(
            library
                .history
                .pages()
                .any(|page| *page == Page::Album("spotify:album:r".to_owned())),
            "the album behind it was pruned with it"
        );
        assert!(library.can_back());
        library.back();
        assert_eq!(
            library.page(),
            Some(&Page::Album("spotify:album:r".to_owned())),
            "Back does not land on the album behind the removed page"
        );
    }

    #[test]
    fn a_failed_playlist_removal_keeps_the_row_and_asks_for_a_read() {
        let mut library = sidebar_library(&["a", "b"], 0x07, true, None);
        assert!(
            library
                .remove_from_library(&item(SidebarKind::Playlists, "a"))
                .is_some()
        );

        library.apply(&Update::PlaylistRemoveFailed {
            playlist: "a".to_owned(),
        });

        assert_eq!(sidebar_order(&library), names(&["a", "b"]));
        assert_eq!(library.sidebar_notice, Some(SidebarWrite::Remove));
        assert!(!library.write_in_flight());
        assert!(library.rootlist.is_none(), "the held revision was kept");
        assert_eq!(library.rootlist_request(), Some(Command::ReloadLibrary));

        // The read the failure asked for must not erase its own sentence.
        library.apply(&Update::Playlists(vec![
            playlist("a", "A"),
            playlist("b", "B"),
        ]));
        assert_eq!(sidebar_order(&library), names(&["a", "b"]));
        assert_eq!(
            library.sidebar_notice,
            Some(SidebarWrite::Remove),
            "the heal read cleared the notice it was asked for"
        );

        library.apply(&Update::RootlistAccess(rootlist_access(0x09, true)));
        assert!(
            library
                .remove_from_library(&item(SidebarKind::Playlists, "a"))
                .is_some()
        );
        assert_eq!(
            library.sidebar_notice, None,
            "the next write kept the old sentence"
        );
    }

    #[test]
    fn a_stale_removal_reply_is_ignored() {
        let mut library = sidebar_library(&["a", "b"], 0x07, true, None);
        assert!(
            library
                .remove_from_library(&item(SidebarKind::Playlists, "a"))
                .is_some()
        );

        library.apply(&Update::PlaylistRemoved {
            playlist: "b".to_owned(),
            revision: revision(0x09),
        });
        assert_eq!(sidebar_order(&library), names(&["a", "b"]));
        assert!(
            library.write_in_flight(),
            "another row's reply ended the write"
        );

        let mut idle = sidebar_library(&["a"], 0x07, true, None);
        idle.apply(&Update::PlaylistRemoved {
            playlist: "a".to_owned(),
            revision: revision(0x09),
        });
        assert_eq!(sidebar_order(&idle), names(&["a"]));
    }

    #[test]
    fn an_album_or_a_show_removal_is_confirmed_by_its_reply() {
        for (kind, uri, dropped) in [
            (SidebarKind::Albums, "spotify:album:r", 0),
            (SidebarKind::Podcasts, "spotify:show:s", 2),
        ] {
            let mut library = saved_library();
            assert_eq!(
                library.remove_from_library(&item(kind, uri)),
                Some(Command::SetSaved {
                    uri: uri.to_owned(),
                    saved: false,
                }),
                "{kind:?}"
            );
            assert_eq!(
                sidebar_order(&library),
                names(&[]),
                "{kind:?} changed its list before the write was confirmed"
            );

            library.apply(&Update::Saved {
                uri: uri.to_owned(),
                saved: false,
            });

            let left = match dropped {
                0 => match &library.saved_albums {
                    Load::Ready(albums) => albums.len(),
                    _ => usize::MAX,
                },
                _ => match &library.followed {
                    Load::Ready(shows) => shows.len(),
                    _ => usize::MAX,
                },
            };
            assert_eq!(left, 0, "{kind:?} kept its row");
            assert!(!library.write_in_flight());
            assert_eq!(library.sidebar_notice, None);
        }
    }

    #[test]
    fn a_save_asks_for_the_write_and_is_confirmed_by_its_echo() {
        let mut library = saved_library();
        // An album the library does not have: the page's band offers the heart
        // for it, and the list is ready, so the answer is a known "not saved".
        assert_eq!(library.is_saved("spotify:album:new"), Some(false));
        assert_eq!(
            library.save_to_library(&item(SidebarKind::Albums, "spotify:album:new")),
            Some(Command::SetSaved {
                uri: "spotify:album:new".to_owned(),
                saved: true,
            })
        );
        assert!(library.saved_pending("spotify:album:new", true));
        assert_eq!(
            library.is_saved("spotify:album:new"),
            Some(false),
            "the list has not changed yet"
        );

        library.apply(&Update::Saved {
            uri: "spotify:album:new".to_owned(),
            saved: true,
        });

        assert_eq!(
            library.is_saved("spotify:album:new"),
            Some(true),
            "the echo outlives the list an add cannot join"
        );
        assert!(!library.write_in_flight());
        assert_eq!(library.sidebar_notice, None);
    }

    /// A saved album reaches the sidebar by re-reading the list that holds it: an
    /// add names no card, so the list cannot gain the row on its own.
    /// An artist page draws a heart on each of its releases, and only the whole
    /// set can say what they are, so the page asks for it.
    /// A heart carries the value it was pressed into, so the library is told the
    /// direction instead of inferring it from a state that may have moved.
    #[test]
    fn a_heart_that_asks_to_save_writes_one_save() {
        let mut library = saved_library();
        let item = item(SidebarKind::Albums, "spotify:album:new");

        assert_eq!(
            library.set_saved(&item, true),
            Some(Command::SetSaved {
                uri: "spotify:album:new".to_owned(),
                saved: true,
            }),
            "an unsaved album's heart must save it"
        );
    }

    #[test]
    fn a_heart_that_asks_to_unsave_writes_one_removal() {
        let mut library = saved_library();
        let item = item(SidebarKind::Albums, "spotify:album:new");
        library.apply(&Update::SavedAlbums(vec![saved_album("spotify:album:new")]));
        assert_eq!(library.is_saved("spotify:album:new"), Some(true));

        assert_eq!(
            library.set_saved(&item, false),
            Some(Command::SetSaved {
                uri: "spotify:album:new".to_owned(),
                saved: false,
            }),
            "a saved album's heart asked to save it again"
        );
    }

    #[test]
    fn an_artist_page_wants_the_saved_albums() {
        let mut library = Library::default();
        library.apply(&Update::Playlists(Vec::new()));
        library.open(Page::Artist("spotify:artist:a".to_owned()));

        assert_eq!(
            library.saved_albums_request(),
            Some(Command::LoadSavedAlbums),
            "an artist page's hearts have nothing to read"
        );
    }

    #[test]
    fn a_playlist_page_does_not_want_the_saved_albums() {
        let mut library = Library::default();
        library.apply(&Update::Playlists(vec![playlist(
            "spotify:playlist:a",
            "Alpha",
        )]));
        library
            .root_filter
            .choose(crate::library_filter::RootKind::Playlists);
        library.open(Page::Playlist("spotify:playlist:a".to_owned()));

        assert_eq!(
            library.saved_albums_request(),
            None,
            "a page with no hearts on it asked for a set it does not draw"
        );
    }

    #[test]
    fn a_saved_album_re_arms_the_list_that_would_hold_it() {
        let mut library = saved_library();
        assert!(matches!(library.saved_albums, Load::Ready(_)));
        library.save_to_library(&item(SidebarKind::Albums, "spotify:album:new"));
        library.apply(&Update::Saved {
            uri: "spotify:album:new".to_owned(),
            saved: true,
        });

        assert_eq!(
            library.saved_albums,
            Load::Idle,
            "the list was not re-armed, so nothing would read it again"
        );
        assert_eq!(library.is_saved("spotify:album:new"), Some(true));
    }

    #[test]
    fn a_saved_show_re_arms_the_shows_read() {
        let mut library = saved_library();
        library.save_to_library(&item(SidebarKind::Podcasts, "spotify:show:new"));
        library.apply(&Update::Saved {
            uri: "spotify:show:new".to_owned(),
            saved: true,
        });
        assert_eq!(library.followed, Load::Idle);
    }

    #[test]
    fn a_followed_artist_re_arms_the_artist_read() {
        let mut library = Library::default();
        library.apply(&Update::FollowedArtists(Vec::new()));
        library.open(crate::route::Page::Artist("spotify:artist:new".to_owned()));
        library.apply(&Update::Artist(artist_detail("spotify:artist:new")));
        library.apply(&Update::Followed {
            artist: "spotify:artist:new".to_owned(),
            followed: false,
        });
        // The press is what makes the answer ours: the page's own probe carries
        // the same shape and must not re-arm anything.
        assert!(library.follow.pressed().is_some());

        library.apply(&Update::Followed {
            artist: "spotify:artist:new".to_owned(),
            followed: true,
        });
        assert_eq!(library.followed_artists, Load::Idle);
    }

    /// The page's own press has to reach the list the sidebar draws; the page's
    /// probe arrives in the same shape and must not.
    #[test]
    fn a_press_of_follow_asks_for_the_artist_list_again() {
        let mut library = Library::default();
        library.apply(&Update::Playlists(Vec::new()));
        library.apply(&Update::FollowedArtists(Vec::new()));
        library
            .sidebar_kind
            .choose(crate::sidebar_kind::SidebarKind::Artists);
        library.open(Page::Artist("spotify:artist:a".to_owned()));
        library.apply(&Update::Artist(artist_detail("spotify:artist:a")));
        library.apply(&Update::Followed {
            artist: "spotify:artist:a".to_owned(),
            followed: false,
        });
        assert!(library.follow.pressed().is_some(), "arrange: the press");

        library.apply(&Update::Followed {
            artist: "spotify:artist:a".to_owned(),
            followed: true,
        });

        assert_eq!(
            library.followed_artists_request(),
            Some(Command::LoadFollowedArtists),
            "a follow the user asked for must reach the list"
        );
    }

    /// An unfollow names a row the list already holds, so the row goes at once.
    #[test]
    fn a_press_of_unfollow_drops_the_artist_from_the_list() {
        let mut library = Library::default();
        library.apply(&Update::Playlists(Vec::new()));
        library.apply(&Update::FollowedArtists(vec![followed_artist(
            "spotify:artist:a",
        )]));
        library
            .sidebar_kind
            .choose(crate::sidebar_kind::SidebarKind::Artists);
        library.open(Page::Artist("spotify:artist:a".to_owned()));
        library.apply(&Update::Artist(artist_detail("spotify:artist:a")));
        library.apply(&Update::Followed {
            artist: "spotify:artist:a".to_owned(),
            followed: true,
        });
        assert_eq!(
            library.follow.pressed(),
            Some(Command::SetFollow {
                artist: "spotify:artist:a".to_owned(),
                followed: false,
            }),
            "arrange: the press unfollows"
        );

        library.apply(&Update::Followed {
            artist: "spotify:artist:a".to_owned(),
            followed: false,
        });

        match &library.followed_artists {
            Load::Ready(artists) => assert!(artists.is_empty(), "the row stayed"),
            other => panic!("the list was dropped: {other:?}"),
        }
    }

    /// An answer that agrees with the list is not a reason to read it again, so a
    /// page's opening probe costs nothing.
    #[test]
    fn a_probe_that_agrees_with_the_list_re_arms_nothing() {
        let mut library = Library::default();
        library.apply(&Update::FollowedArtists(vec![followed_artist(
            "spotify:artist:new",
        )]));
        library.open(crate::route::Page::Artist("spotify:artist:new".to_owned()));
        library.apply(&Update::Artist(artist_detail("spotify:artist:new")));

        library.apply(&Update::Followed {
            artist: "spotify:artist:new".to_owned(),
            followed: true,
        });

        assert!(
            matches!(library.followed_artists, Load::Ready(_)),
            "a list that already holds the artist was re-read for nothing"
        );
    }

    /// The opposite disagreement is a stale set, and only a read fixes it.
    #[test]
    fn a_probe_that_disagrees_asks_for_the_list() {
        let mut library = Library::default();
        library.apply(&Update::FollowedArtists(Vec::new()));
        library.open(crate::route::Page::Artist("spotify:artist:new".to_owned()));
        library.apply(&Update::Artist(artist_detail("spotify:artist:new")));

        library.apply(&Update::Followed {
            artist: "spotify:artist:new".to_owned(),
            followed: true,
        });

        assert_eq!(
            library.followed_artists,
            Load::Idle,
            "the server says followed and the list does not hold it"
        );
    }

    /// A band's `…` menu writes without opening the page's Follow state, so the
    /// answer is all there is to go on.
    #[test]
    fn a_menu_unfollow_with_nothing_pending_drops_the_row() {
        let mut library = Library::default();
        library.apply(&Update::FollowedArtists(vec![followed_artist(
            "spotify:artist:a",
        )]));

        library.apply(&Update::Followed {
            artist: "spotify:artist:a".to_owned(),
            followed: false,
        });

        match &library.followed_artists {
            Load::Ready(artists) => assert!(artists.is_empty(), "the row stayed"),
            other => panic!("the list was dropped: {other:?}"),
        }
    }

    #[test]
    fn a_menu_follow_with_nothing_pending_asks_for_the_list() {
        let mut library = Library::default();
        library.apply(&Update::FollowedArtists(Vec::new()));

        library.apply(&Update::Followed {
            artist: "spotify:artist:a".to_owned(),
            followed: true,
        });

        assert_eq!(library.followed_artists, Load::Idle);
    }

    #[test]
    fn a_probe_is_asked_for_when_no_list_can_answer() {
        let mut library = Library::default();
        // Nothing has been read, so no list can say whether this album is saved.
        library.open(crate::route::Page::Album("spotify:album:new".to_owned()));
        assert_eq!(library.is_saved("spotify:album:new"), None);

        assert_eq!(
            library.saved_request(),
            Some(Command::LoadSaved {
                uri: "spotify:album:new".to_owned(),
            })
        );
        assert_eq!(library.saved_request(), None, "one probe per page");

        library.apply(&Update::SavedState {
            uri: "spotify:album:new".to_owned(),
            saved: Some(true),
        });
        assert_eq!(library.is_saved("spotify:album:new"), Some(true));
        assert_eq!(library.saved_request(), None, "the answer ends the asking");
    }

    #[test]
    fn a_probe_that_does_not_answer_leaves_the_state_unknown() {
        let mut library = Library::default();
        library.open(crate::route::Page::Album("spotify:album:new".to_owned()));
        library.saved_request();
        library.apply(&Update::SavedState {
            uri: "spotify:album:new".to_owned(),
            saved: None,
        });

        assert_eq!(
            library.is_saved("spotify:album:new"),
            None,
            "an unanswered probe claims nothing"
        );
    }

    #[test]
    fn a_page_that_is_not_an_album_or_a_show_asks_nothing() {
        let mut library = Library::default();
        library.open(crate::route::Page::Playlist(
            "spotify:playlist:a".to_owned(),
        ));
        assert_eq!(library.saved_request(), None);
    }

    #[test]
    fn a_failed_save_keeps_the_heart_off_and_says_so() {
        let mut library = saved_library();
        library.save_to_library(&item(SidebarKind::Albums, "spotify:album:new"));
        library.apply(&Update::SavedFailed {
            uri: "spotify:album:new".to_owned(),
            saved: true,
        });

        assert_eq!(library.is_saved("spotify:album:new"), Some(false));
        assert_eq!(library.sidebar_notice, Some(SidebarWrite::Save));
        assert!(!library.write_in_flight());
    }

    #[test]
    fn an_item_the_library_has_is_not_saved_again() {
        let mut library = saved_library();
        assert_eq!(library.is_saved("spotify:album:r"), Some(true));
        assert_eq!(
            library.save_to_library(&item(SidebarKind::Albums, "spotify:album:r")),
            None
        );
        assert!(!library.write_in_flight());
    }

    #[test]
    fn a_playlist_is_put_back_on_the_rootlist_by_a_link() {
        let mut library = sidebar_library(&["a"], 0x07, true, None);
        // A playlist the sidebar does not list: the only way one is missing from
        // the library is that it was unlinked, and a band then offers the heart.
        assert_eq!(library.is_saved("spotify:playlist:gone"), Some(false));
        assert_eq!(
            library.save_to_library(&item(SidebarKind::Playlists, "gone")),
            Some(Command::LinkPlaylist {
                playlist: "gone".to_owned(),
                revision: revision(0x07),
            })
        );
        assert!(library.write_in_flight(), "the link holds the list still");

        library.apply(&Update::PlaylistLinked {
            playlist: "gone".to_owned(),
            revision: revision(0x08),
        });

        assert!(!library.write_in_flight());
        assert_eq!(library.sidebar_notice, None);
        assert_eq!(
            library
                .rootlist
                .as_ref()
                .map(|access| access.revision.clone()),
            Some(revision(0x08)),
            "the next write names the revision the link left"
        );
    }

    #[test]
    fn a_link_invents_no_row_and_asks_for_the_list() {
        let mut library = sidebar_library(&["a"], 0x07, true, None);
        assert!(
            library
                .save_to_library(&item(SidebarKind::Playlists, "gone"))
                .is_some()
        );

        library.apply(&Update::PlaylistLinked {
            playlist: "gone".to_owned(),
            revision: revision(0x08),
        });

        assert_eq!(
            sidebar_order(&library),
            names(&["a"]),
            "the reply names no row, so the sidebar cannot draw one from it"
        );
        assert_eq!(
            library.rootlist_request(),
            Some(Command::ReloadLibrary),
            "the linked row comes back by re-reading the list, not by inventing one"
        );
    }

    #[test]
    fn a_failed_link_says_so_and_leaves_the_list_alone() {
        let mut library = sidebar_library(&["a"], 0x07, true, None);
        library.save_to_library(&item(SidebarKind::Playlists, "gone"));
        library.apply(&Update::PlaylistLinkFailed {
            playlist: "gone".to_owned(),
        });

        assert_eq!(library.sidebar_notice, Some(SidebarWrite::Save));
        assert!(!library.write_in_flight());
        assert_eq!(
            library
                .rootlist
                .as_ref()
                .map(|access| access.revision.clone()),
            Some(revision(0x07)),
            "a failed link keeps the revision it had"
        );
    }

    #[test]
    fn a_failed_saved_removal_keeps_the_row_and_says_so() {
        let mut library = saved_library();
        assert!(
            library
                .remove_from_library(&item(SidebarKind::Albums, "spotify:album:r"))
                .is_some()
        );
        library.apply(&Update::SavedFailed {
            uri: "spotify:album:r".to_owned(),
            saved: false,
        });

        match &library.saved_albums {
            Load::Ready(albums) => assert_eq!(albums.len(), 1),
            other => panic!("the list was dropped: {other:?}"),
        }
        assert_eq!(library.sidebar_notice, Some(SidebarWrite::Remove));
        assert!(!library.write_in_flight());
    }

    #[test]
    fn an_artist_removal_rides_the_unfollow_and_resets_the_pages_label() {
        let mut library = saved_library();
        library.open(Page::Artist("spotify:artist:a".to_owned()));
        library.apply(&Update::Artist(artist_detail("spotify:artist:a")));
        library.apply(&Update::Followed {
            artist: "spotify:artist:a".to_owned(),
            followed: true,
        });
        assert_eq!(library.follow.label(), "Following");

        assert_eq!(
            library.remove_from_library(&item(SidebarKind::Artists, "spotify:artist:a")),
            Some(Command::SetFollow {
                artist: "spotify:artist:a".to_owned(),
                followed: false,
            })
        );
        library.apply(&Update::Followed {
            artist: "spotify:artist:a".to_owned(),
            followed: false,
        });

        match &library.followed_artists {
            Load::Ready(artists) => assert!(artists.is_empty(), "the row stayed"),
            other => panic!("the list was dropped: {other:?}"),
        }
        assert_eq!(library.follow.label(), "Follow");
        assert!(!library.write_in_flight());
    }

    #[test]
    fn a_failed_artist_removal_keeps_the_row_and_says_so() {
        let mut library = saved_library();
        assert!(
            library
                .remove_from_library(&item(SidebarKind::Artists, "spotify:artist:a"))
                .is_some()
        );
        library.apply(&Update::FollowFailed {
            artist: "spotify:artist:a".to_owned(),
            failure: Failure::Save,
        });

        match &library.followed_artists {
            Load::Ready(artists) => assert_eq!(artists.len(), 1),
            other => panic!("the list was dropped: {other:?}"),
        }
        assert_eq!(library.sidebar_notice, Some(SidebarWrite::Remove));
        assert!(!library.write_in_flight());
    }

    #[test]
    fn removing_the_last_row_leaves_an_empty_ready_list() {
        let mut library = sidebar_library(&["a"], 0x07, true, None);
        assert!(
            library
                .remove_from_library(&item(SidebarKind::Playlists, "a"))
                .is_some()
        );
        library.apply(&Update::PlaylistRemoved {
            playlist: "a".to_owned(),
            revision: revision(0x09),
        });

        assert_eq!(library.playlists, Load::Ready(Vec::new()));
    }

    #[test]
    fn a_playlists_read_lands_during_a_saved_removal_but_not_a_playlist_one() {
        let mut saved = saved_library();
        assert!(
            saved
                .remove_from_library(&item(SidebarKind::Albums, "spotify:album:r"))
                .is_some()
        );
        saved.apply(&Update::Playlists(vec![playlist("x", "X")]));
        assert_eq!(
            sidebar_order(&saved),
            names(&["x"]),
            "a collection removal dropped a playlists read"
        );

        let mut unlinking = sidebar_library(&["a"], 0x07, true, None);
        assert!(
            unlinking
                .remove_from_library(&item(SidebarKind::Playlists, "a"))
                .is_some()
        );
        unlinking.apply(&Update::Playlists(vec![playlist("x", "X")]));
        assert_eq!(sidebar_order(&unlinking), names(&["a"]));
        assert!(
            unlinking.rootlist_stale,
            "the dropped read owed a fresh one"
        );
    }

    #[test]
    fn can_remove_is_the_librarys_state_per_kind() {
        let library = saved_library();
        assert!(
            !library.can_remove(SidebarKind::Playlists),
            "a rootlist with no revision offered an unlink"
        );
        for kind in [
            SidebarKind::Albums,
            SidebarKind::Artists,
            SidebarKind::Podcasts,
        ] {
            assert!(library.can_remove(kind), "{kind:?} is ready and idle");
        }

        let editable = sidebar_library(&["a"], 0x07, true, None);
        assert!(editable.can_remove(SidebarKind::Playlists));
        assert!(!editable.can_remove(SidebarKind::Albums));

        let mut writing = sidebar_library(&["a", "b"], 0x07, true, None);
        assert!(writing.move_playlist(0, 2, "a").is_some());
        for kind in crate::sidebar_kind::ROW {
            assert!(!writing.can_remove(kind), "{kind:?} during a write");
        }
    }

    #[test]
    fn a_chip_change_clears_the_remove_sentence() {
        let mut library = sidebar_library(&["a"], 0x07, true, None);
        library.sidebar_notice = Some(SidebarWrite::Remove);

        assert!(library.choose_kind(SidebarKind::Podcasts));

        assert_eq!(library.sidebar_notice, None);
    }
    /// A sidebar holding one editable playlist, its page loaded.
    fn renaming_library() -> Library {
        let mut library = sidebar_library(&["a"], 0x07, true, None);
        library.select("a");
        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: vec![track("t0")],
            cover: None,
            access: access(0x07, true),
        });
        library
    }

    fn rename_pending(library: &mut Library, uri: &str, surface: crate::naming::Surface) {
        library.renaming.open(uri, uri, surface);
        library.sidebar_pending = Some(SidebarPending::Rename {
            playlist: uri.to_owned(),
        });
    }

    #[test]
    fn a_confirmed_rename_sets_the_name_named_and_both_revisions() {
        let mut library = renaming_library();
        rename_pending(&mut library, "a", crate::naming::Surface::Row);

        library.apply(&Update::PlaylistRenamed {
            playlist: "a".to_owned(),
            name: "New Name".to_owned(),
            revision: revision(0x09),
        });

        let entry = sidebar_entry(&library, "a").expect("the row");
        assert_eq!(entry.name, "New Name");
        assert!(entry.named);
        assert_eq!(
            entry.access.as_ref().map(|held| held.revision.clone()),
            Some(revision(0x09))
        );
        assert_eq!(
            match &library.tracks {
                Load::Ready(tracks) => tracks.access.as_ref().map(|held| held.revision.clone()),
                Load::Idle | Load::Loading | Load::Failed(_) => None,
            },
            Some(revision(0x09)),
            "the page's next move would name a stale base"
        );
        assert!(!library.renaming.is_open());
        assert!(!library.write_in_flight());
        assert_eq!(library.sidebar_notice, None);
    }

    #[test]
    fn a_rename_leaves_the_cover_the_band_and_the_row_already_showed() {
        let first = cover_url("first");
        let mut library = covered_page(&first, &["spotify:track:t0"]);
        rename_pending(&mut library, "a", crate::naming::Surface::Band);

        library.apply(&Update::PlaylistRenamed {
            playlist: "a".to_owned(),
            name: "New Name".to_owned(),
            revision: revision(0x09),
        });

        let entry = library.selected_entry().expect("the band's entry");
        assert_eq!(
            entry.name, "New Name",
            "the band titles the playlist from a name the rename did not write"
        );
        assert_eq!(
            band_cover(entry, settled(&library)),
            BandCover::Entry(Some(&first)),
            "a rename names the playlist, so it cannot change the picture beside the name"
        );
        assert_eq!(
            entry_cover(&library, "a"),
            Some(PlaylistCover::derived(first)),
            "the sidebar row and the library card keep the cover they showed"
        );
        assert_eq!(
            library.tracks_request(),
            None,
            "a rename marks no cover due, so it asks for no read of the page"
        );
    }

    #[test]
    fn a_named_false_playlist_can_be_renamed_and_the_confirm_names_it() {
        let mut library = renaming_library();
        if let Load::Ready(playlists) = &mut library.playlists {
            playlists[0].named = false;
        }
        rename_pending(&mut library, "a", crate::naming::Surface::Row);

        library.apply(&Update::PlaylistRenamed {
            playlist: "a".to_owned(),
            name: "Named".to_owned(),
            revision: revision(0x09),
        });

        let entry = sidebar_entry(&library, "a").expect("the row");
        assert_eq!(entry.name, "Named");
        assert!(entry.named);
    }

    #[test]
    fn a_rename_confirm_for_another_uri_changes_nothing() {
        let mut library = renaming_library();
        rename_pending(&mut library, "a", crate::naming::Surface::Row);

        library.apply(&Update::PlaylistRenamed {
            playlist: "b".to_owned(),
            name: "Other".to_owned(),
            revision: revision(0x09),
        });

        let entry = sidebar_entry(&library, "a").expect("the row");
        assert_eq!(entry.name, "a");
        assert!(library.renaming.is_open());
        assert!(library.write_in_flight());
        assert_eq!(
            entry.access.as_ref().map(|held| held.revision.clone()),
            Some(revision(0x07))
        );
    }

    #[test]
    fn a_rename_failure_keeps_the_text_and_says_so() {
        let mut library = renaming_library();
        rename_pending(&mut library, "a", crate::naming::Surface::Row);
        library.renaming.edit().clear();
        library.renaming.edit().push_str("Night Drive");

        library.apply(&Update::PlaylistRenameFailed {
            playlist: "a".to_owned(),
        });

        assert!(library.renaming.is_open());
        assert_eq!(library.renaming.text(), "Night Drive");
        assert!(library.renaming.wants_focus());
        assert!(!library.renaming.writing());
        assert_eq!(library.sidebar_notice, Some(SidebarWrite::Rename));
        assert!(!library.write_in_flight());
    }

    #[test]
    fn a_band_rename_failure_also_marks_the_loaded_page() {
        let mut library = renaming_library();
        rename_pending(&mut library, "a", crate::naming::Surface::Band);

        library.apply(&Update::PlaylistRenameFailed {
            playlist: "a".to_owned(),
        });

        assert_eq!(
            match &library.tracks {
                Load::Ready(tracks) => tracks.notice,
                Load::Idle | Load::Loading | Load::Failed(_) => None,
            },
            Some(Failure::Rename)
        );
        assert_eq!(library.sidebar_notice, Some(SidebarWrite::Rename));
    }

    #[test]
    fn a_row_rename_failure_leaves_the_page_notice_alone() {
        let mut library = renaming_library();
        rename_pending(&mut library, "a", crate::naming::Surface::Row);

        library.apply(&Update::PlaylistRenameFailed {
            playlist: "a".to_owned(),
        });

        assert_eq!(
            match &library.tracks {
                Load::Ready(tracks) => tracks.notice,
                Load::Idle | Load::Loading | Load::Failed(_) => None,
            },
            None
        );
    }

    #[test]
    fn a_rename_confirm_clears_only_its_own_page_notice() {
        let mut library = renaming_library();
        if let Load::Ready(tracks) = &mut library.tracks {
            tracks.notice = Some(Failure::Rename);
        }
        rename_pending(&mut library, "a", crate::naming::Surface::Band);

        library.apply(&Update::PlaylistRenamed {
            playlist: "a".to_owned(),
            name: "New".to_owned(),
            revision: revision(0x09),
        });
        assert_eq!(
            match &library.tracks {
                Load::Ready(tracks) => tracks.notice,
                Load::Idle | Load::Loading | Load::Failed(_) => None,
            },
            None
        );

        if let Load::Ready(tracks) = &mut library.tracks {
            tracks.notice = Some(Failure::Save);
        }
        rename_pending(&mut library, "a", crate::naming::Surface::Band);
        library.apply(&Update::PlaylistRenamed {
            playlist: "a".to_owned(),
            name: "Newer".to_owned(),
            revision: revision(0x0a),
        });
        assert_eq!(
            match &library.tracks {
                Load::Ready(tracks) => tracks.notice,
                Load::Idle | Load::Loading | Load::Failed(_) => None,
            },
            Some(Failure::Save),
            "an unrelated notice was cleared"
        );
    }

    #[test]
    fn a_playlists_read_during_a_rename_is_adopted() {
        let mut library = renaming_library();
        rename_pending(&mut library, "a", crate::naming::Surface::Row);

        library.apply(&Update::Playlists(vec![playlist("b", "B")]));

        match &library.playlists {
            Load::Ready(playlists) => {
                assert_eq!(playlists.len(), 1);
                assert_eq!(playlists[0].uri, "b");
            }
            other => panic!("the read was not adopted: {other:?}"),
        }
        assert!(library.write_in_flight(), "the rename went with it");
    }

    #[test]
    fn a_stale_tracks_read_for_another_playlist_leaves_the_revision_alone() {
        let mut library = renaming_library();
        rename_pending(&mut library, "a", crate::naming::Surface::Band);
        library.apply(&Update::PlaylistRenamed {
            playlist: "a".to_owned(),
            name: "New".to_owned(),
            revision: revision(0x09),
        });

        library.apply(&Update::Tracks {
            playlist: "b".to_owned(),
            tracks: vec![track("t1")],
            cover: None,
            access: access(0x05, true),
        });

        assert_eq!(
            match &library.tracks {
                Load::Ready(tracks) => tracks.access.as_ref().map(|held| held.revision.clone()),
                Load::Idle | Load::Loading | Load::Failed(_) => None,
            },
            Some(revision(0x09)),
            "another page's read replaced this page's base"
        );
    }

    #[test]
    fn can_rename_needs_a_ready_editable_list_and_no_write() {
        assert!(sidebar_library(&["a"], 0x07, true, None).can_rename("a"));

        assert!(
            !sidebar_library(&["a"], 0x07, false, None).can_rename("a"),
            "a list the user cannot write offered a rename"
        );
        assert!(
            !sidebar_library(&["a"], 0x07, true, None).can_rename("b"),
            "a row the list does not hold offered a rename"
        );
        let mut no_access = Library::default();
        no_access.apply(&Update::Playlists(vec![playlist("a", "A")]));
        assert!(
            !no_access.can_rename("a"),
            "a row with no access offered a rename"
        );
        let mut writing = sidebar_library(&["a", "b"], 0x07, true, None);
        assert!(writing.move_playlist(0, 2, "a").is_some());
        assert!(
            !writing.can_rename("a"),
            "a second write went out beside the first"
        );
        let mut loading = sidebar_library(&["a"], 0x07, true, None);
        loading.playlists = Load::Loading;
        assert!(!loading.can_rename("a"), "a list that is not ready");
    }

    #[test]
    fn begin_rename_fills_the_field_with_the_current_name() {
        let mut library = sidebar_library(&["a"], 0x07, true, None);

        assert!(library.begin_rename("a", crate::naming::Surface::Row));

        assert!(
            library
                .renaming
                .is_open_on("a", crate::naming::Surface::Row)
        );
        assert_eq!(library.renaming.text(), "a");
        assert!(library.renaming.wants_focus());
        assert!(!library.write_in_flight());
    }

    #[test]
    fn a_refused_rename_start_changes_nothing() {
        let mut library = sidebar_library(&["a", "b"], 0x07, true, None);
        assert!(library.move_playlist(0, 2, "a").is_some());

        assert!(!library.begin_rename("a", crate::naming::Surface::Row));
        assert!(!library.renaming.is_open());
    }

    #[test]
    fn rename_issues_one_command_with_the_trimmed_name() {
        let mut library = sidebar_library(&["a"], 0x07, true, None);
        assert!(library.begin_rename("a", crate::naming::Surface::Row));
        *library.renaming.edit() = "  Night  Drive ".to_owned();

        assert_eq!(
            library.rename_playlist(),
            Some(Command::RenamePlaylist {
                playlist: "a".to_owned(),
                name: "Night  Drive".to_owned(),
            })
        );
        assert!(library.renaming.writing());
        assert!(library.write_in_flight());
        assert_eq!(
            library.sidebar_pending,
            Some(SidebarPending::Rename {
                playlist: "a".to_owned(),
            })
        );
        assert_eq!(library.rename_playlist(), None, "a second write");
    }

    #[test]
    fn a_rename_to_the_same_name_sends_nothing_and_closes() {
        let mut library = sidebar_library(&["a"], 0x07, true, None);
        assert!(library.begin_rename("a", crate::naming::Surface::Band));
        *library.renaming.edit() = "  a  ".to_owned();

        assert_eq!(library.rename_playlist(), None);
        assert!(!library.renaming.is_open());
        assert!(!library.write_in_flight());
        assert!(library.renaming.wants_return());
    }

    #[test]
    fn a_rename_to_the_text_an_unnamed_row_shows_is_sent() {
        // A create whose own rename failed keeps the typed name in the row for
        // this session only: `named` is false, so the server has never heard it.
        // Opening Rename on that row prefills the text it shows, and Enter must
        // still reach the wire.
        let mut library = renaming_library();
        if let Load::Ready(playlists) = &mut library.playlists {
            playlists[0].named = false;
        }
        assert!(library.begin_rename("a", crate::naming::Surface::Row));
        assert_eq!(library.renaming.text(), "a", "the field prefilled nothing");

        assert_eq!(
            library.rename_playlist(),
            Some(Command::RenamePlaylist {
                playlist: "a".to_owned(),
                name: "a".to_owned(),
            }),
            "an unnamed row's own text was treated as the server's name"
        );
        assert!(library.renaming.writing());

        library.apply(&Update::PlaylistRenamed {
            playlist: "a".to_owned(),
            name: "a".to_owned(),
            revision: revision(0x09),
        });
        let entry = sidebar_entry(&library, "a").expect("the row");
        assert!(entry.named, "the sent name did not make the row named");
    }

    #[test]
    fn a_rename_enter_is_refused_while_a_page_write_holds_the_list() {
        let mut library = renaming_library();
        assert!(library.begin_rename("a", crate::naming::Surface::Band));
        *library.renaming.edit() = "New".to_owned();
        let first = match &library.tracks {
            Load::Ready(tracks) => tracks.tracks[0].clone(),
            Load::Idle | Load::Loading | Load::Failed(_) => panic!("the page is ready"),
        };
        assert!(library.remove_tracks("a", &[0], &[first]).is_some());

        assert_eq!(
            library.rename_playlist(),
            None,
            "a rename overran a write the page had already started"
        );
        assert!(library.renaming.is_open(), "a refusal closed the field");
        assert!(!library.renaming.writing());
        assert_eq!(library.renaming.text(), "New", "a refusal dropped the text");
    }

    #[test]
    fn a_rename_after_a_changed_name_sends_once() {
        let mut library = sidebar_library(&["a"], 0x07, true, None);
        assert!(library.begin_rename("a", crate::naming::Surface::Row));
        *library.renaming.edit() = "New".to_owned();

        assert!(library.rename_playlist().is_some());
        assert_eq!(library.rename_playlist(), None);
        assert!(library.renaming.writing());
    }

    #[test]
    fn a_page_change_closes_a_band_field_but_not_a_row_field_or_a_writing_one() {
        let mut band = renaming_library();
        assert!(band.begin_rename("a", crate::naming::Surface::Band));
        band.to_playlists();
        assert!(!band.renaming.is_open(), "a band field outlived its page");

        let mut row = renaming_library();
        assert!(row.begin_rename("a", crate::naming::Surface::Row));
        row.to_playlists();
        assert!(row.renaming.is_open(), "a row field outlived its page");

        let mut writing = renaming_library();
        assert!(writing.begin_rename("a", crate::naming::Surface::Band));
        *writing.renaming.edit() = "New".to_owned();
        assert!(writing.rename_playlist().is_some());
        writing.to_playlists();
        assert!(
            writing.renaming.is_open(),
            "a write in flight lost its field"
        );
    }

    #[test]
    fn a_removed_playlist_closes_its_rename_field() {
        let mut library = renaming_library();
        assert!(library.begin_rename("a", crate::naming::Surface::Row));

        library.apply(&Update::Playlists(Vec::new()));

        assert!(!library.renaming.is_open());
        assert!(!library.renaming.wants_return());
    }
    #[test]
    fn the_next_page_write_carries_the_rename_replys_revision() {
        let mut library = sidebar_library(&["a"], 0x07, true, None);
        library.select("a");
        library.apply(&Update::Tracks {
            playlist: "a".to_owned(),
            tracks: vec![track("t0"), track("t1")],
            cover: None,
            access: access(0x07, true),
        });
        rename_pending(&mut library, "a", crate::naming::Surface::Row);
        library.apply(&Update::PlaylistRenamed {
            playlist: "a".to_owned(),
            name: "New".to_owned(),
            revision: revision(0x09),
        });
        let first = match &library.tracks {
            Load::Ready(tracks) => tracks.tracks[0].clone(),
            Load::Idle | Load::Loading | Load::Failed(_) => panic!("the page is ready"),
        };

        let (command, _) = library
            .move_tracks("a", &[0], 2, &[first])
            .expect("the move is taken");

        assert_eq!(
            command,
            Command::MoveTrack {
                playlist: "a".to_owned(),
                revision: revision(0x09),
                moves: vec![Move {
                    from: 0,
                    length: 1,
                    to: 2,
                }],
            },
            "the next move named the pre-rename base"
        );
    }
}
