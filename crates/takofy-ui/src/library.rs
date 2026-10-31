//! The load state for the playlists and their tracks.
//!
//! The library is a reducer over worker updates: it decides what is on screen
//! and which load a reply belongs to, and it hands the view a command when the
//! user asks for one.
//!
//! This module keeps the library's state, the sidebar write ledger, the
//! permission checks, and the create, move, remove and rename methods. The
//! rest sits in the child modules: `nav` walks the page stack, `requests`
//! sends the reads, `saved` tracks what the library holds, `shelf_play` starts
//! a play from a shelf, and `writes` holds the writes in flight.

use std::collections::HashMap;
use std::ops::Range;
use std::time::Instant;

use takofy_model::{
    Account, AlbumDetail, ArtistDetail, CoverOrigin, CoverSet, FollowedArtist, FollowedShow,
    PlaylistAccess, PlaylistCover, PlaylistEntry, SavedAlbum, ShowDetail, TrackEntry,
};

use crate::cover_refresh::CoverRefresh;
use crate::discography::Filter;
use crate::follow::Follow;
use crate::home::Home;
use crate::library_filter::{RootFilter, RootKind};
use crate::likes::{self, Likes};
use crate::lyrics::LyricsPanel;
use crate::menu::{AddTarget, LibraryItem};
use crate::message::{Command, Failure, Update};
use crate::mosaic::Kept;
use crate::naming::{Naming, Renaming, Surface};
use crate::rail_filter::RailFilter;
use crate::releases::Releases;
use crate::removal;
use crate::reorder;
use crate::route::{History, Page};
use crate::search::Search;
use crate::show_order::{ShowOrder, default_order};
use crate::sidebar_kind::{SidebarFilter, SidebarKind};
use crate::track_filter::TrackFilter;

mod home;
mod nav;
mod radio;
mod requests;
mod restore;
mod saved;
mod shelf_play;
mod snapshot;
mod writes;

pub use home::HOME_HOLD;
pub use restore::Source;
use restore::{Sources, fail_list, live};
use shelf_play::PendingPlay;
pub use shelf_play::Start;
pub use snapshot::Snapshot;

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
    Entry(Option<&'a CoverSet>),
}

/// The band's single-cover source: the playlist entry alone, once its tracks
/// have settled.
///
/// The band never derives its single cover from the track list: the entry already
/// holds the picture or the first track's album cover, and a second derivation
/// could disagree with it. The four-cover mosaic is the deliberate exception: it
/// tiles the list's own album covers only when the entry names no chosen picture.
pub fn band_cover(entry: &PlaylistEntry, settled: bool) -> BandCover<'_> {
    if settled {
        BandCover::Entry(entry.cover_set())
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
    /// Whether the session is connected and the rootlist walked, so reads that
    /// need a session may be asked for.
    session_ready: bool,
    /// Where each list's rows came from, and what it is owed.
    sources: Sources,
    /// The account the session named, once a connect announced it.
    account: Option<Account>,
    /// The account whose copy the restored rows belong to.
    copy_of: Option<Account>,
    /// How many updates have changed what a saved copy would hold.
    generation: u64,
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
    /// The Home page's seed, and the labels the liked cards need.
    pub home: Home,
    /// When this Home visit started waiting for the dated feeds.
    held_since: Option<Instant>,
    /// Whether this visit's wait has been let go, so it does not arm again.
    hold_spent: bool,
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
    /// The covers the last ready playlist wanted for its mosaic, and which list
    /// they belonged to, so a reload keeps the mosaic it already holds.
    pub mosaic_kept: Option<Kept>,
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
    /// The library root's filter row. Opening a page or Home does not reset it:
    /// Back onto the root reloads the same list, so the choice is kept.
    pub root_filter: RootFilter,
    /// The sidebar's kind chips. Like `root_filter`, opening a page or Home
    /// does not reset them.
    pub sidebar_kind: SidebarFilter,
    /// The playlist page's filter. A page change clears it, because the query
    /// names one playlist's rows; a reload of the same playlist keeps it.
    pub track_filter: TrackFilter,
    /// The show page's episode order. It belongs to a show rather than to the
    /// page: `show_orders` remembers what each show was last left in, so Back
    /// onto a show draws it as it was and an unseen one starts on the wire's
    /// own order.
    pub show_order: ShowOrder,
    /// The order each show visited this session was left in.
    ///
    /// Session-scoped and unbounded: the choice belongs to the show, so every
    /// show the user sorts keeps its own for as long as the session lasts.
    show_orders: HashMap<String, ShowOrder>,
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
    context_covers: HashMap<String, CoverSet>,
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
    /// The play pressed on the shelf, until it starts or is dropped.
    play: Option<PendingPlay>,
    /// Why the last shelf play could not fetch its rows, until the next press or
    /// the next page change.
    ///
    /// Pressing play again is the retry, so the sentence stands alone.
    pub play_notice: Option<Failure>,
    /// The seed of the radio a menu started, until its read answers.
    ///
    /// It is not the shelf play: a radio works from any page, and a page change
    /// is no reason to drop one still in flight.
    radio: Option<String>,
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
    /// A radio the user asked for did not start.
    Radio,
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
    /// is left alone, and `LibraryReady` opens the list gates whenever it lands.
    pub fn apply(&mut self, update: &Update) {
        if update.changes_lists() {
            self.generation = self.generation.saturating_add(1);
        }
        match update {
            Update::LibraryReady => self.session_ready = true,
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
                self.sources.playlists = Source::Live;
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
                fail_list(&mut self.playlists, &mut self.sources.playlists, *failure);
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
                // A shelf play waiting on this playlist takes these rows, whatever
                // page is on screen. The cover upgrade below is keyed by the
                // update's own playlist, never the selected one, so a reply for A
                // cannot land on B.
                if self.wants(playlist) {
                    self.land(playlist, tracks.clone());
                }
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
                    self.clear_filter_when_empty();
                }
            }
            Update::TracksFailed { playlist, failure } => {
                self.fail_play(playlist, *failure);
                // A failed read still answers the ask, so the count keeps step
                // with the replies actually coming; otherwise a later ask never
                // reaches zero and an emptied playlist's `None` never clears its
                // cover.
                self.cover_refresh.answer(playlist);
                if self.wanted.as_deref() == Some(playlist.as_str()) {
                    self.tracks = Load::Failed(*failure);
                    // A failed page draws no rows and no field, so the query
                    // it carried would come back to a list nobody filtered.
                    self.track_filter.clear();
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
                // The rows are back, so a query that left with the last one
                // comes back with them.
                if let Some(query) = pending.query {
                    self.track_filter.edit(playlist).push_str(&query);
                }
            }
            // A reply lands only while its own load is in flight: a stale one is
            // discarded, and a second reply cannot replace the first. A shelf
            // play waiting on the same album takes it before that guard, because
            // the page's load may not be the ask it is waiting on.
            Update::Album(detail) => {
                if self.wants(&detail.uri) {
                    self.land(&detail.uri, detail.tracks.clone());
                }
                if self.wanted_album.as_deref() == Some(detail.uri.as_str())
                    && matches!(self.album, Load::Loading)
                {
                    self.album = Load::Ready(detail.clone());
                }
            }
            Update::AlbumFailed { album, failure } => {
                self.fail_play(album, *failure);
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
                    // The chip row is drawn only beside releases, so a query
                    // over an empty discography is one nobody can see or clear.
                    if detail.releases.is_empty() {
                        self.filter.clear_query();
                    }
                }
            }
            Update::ArtistFailed { artist, failure } => {
                if self.wanted_artist.as_deref() == Some(artist.as_str())
                    && matches!(self.artist, Load::Loading)
                {
                    self.artist = Load::Failed(*failure);
                    // A failed page draws no chip row to hold the query either.
                    self.filter.clear_query();
                }
            }
            // The show's reply is guarded exactly as the album's is. The show
            // is recorded on landing, so Back to it finds the order it was left
            // in; a reply for a show the page has left touches nothing drawn.
            Update::Show(detail) => {
                if self.wanted_show.as_deref() == Some(detail.show.uri.as_str())
                    && matches!(self.show, Load::Loading)
                {
                    self.show_orders
                        .entry(detail.show.uri.clone())
                        .or_insert_with(|| default_order(&detail.episodes));
                    self.show = Load::Ready(detail.clone());
                    // A show that has never been sorted opens on its own order,
                    // which is only knowable once its list lands.
                    self.adopt_show_order();
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
                self.sources.shows = Source::Live;
            }
            Update::FollowedShowsFailed(failure) => {
                fail_list(&mut self.followed, &mut self.sources.shows, *failure);
            }
            Update::FollowedArtists(artists) => {
                self.followed_artists = Load::Ready(artists.clone());
                self.sources.artists = Source::Live;
            }
            Update::FollowedArtistsFailed(failure) => {
                fail_list(
                    &mut self.followed_artists,
                    &mut self.sources.artists,
                    *failure,
                );
            }
            Update::SavedAlbums(albums) => {
                self.saved_albums = Load::Ready(albums.clone());
                self.sources.albums = Source::Live;
            }
            Update::SavedAlbumsFailed(failure) => {
                fail_list(&mut self.saved_albums, &mut self.sources.albums, *failure);
            }
            // The liked set is a whole set, not keyed by URI: the state keeps its
            // own epoch guard so a read that predates a flip is dropped.
            Update::LikedTracks(tracks) => {
                self.likes.apply(&likes::Event::Set(tracks.clone()));
            }
            Update::LikedTracksFailed(failure) => {
                self.likes.apply(&likes::Event::SetFailed(*failure));
            }
            Update::SavedLabelled { asked, named } => {
                self.home.apply_labels(asked, named);
            }
            Update::SavedLabelFailed { asked, failure } => {
                self.home.fail_labels(asked, *failure);
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
            // A Home answer lands on the shelf that asked for it, whatever page is shown.
            Update::Home(answer) => self.home.land(answer),
            Update::HomeFailed { ask, failure } => self.home.fail(ask, *failure),
            Update::Radio { seed, rows } => self.land_radio(seed, rows.clone()),
            Update::RadioFailed { seed } => self.fail_radio(seed),
            // The rows a radio's drift asked for go to the queue, which is the
            // only store that holds them.
            Update::RadioMore { .. } => {}
            Update::NeedsAuthorization
            | Update::SignInFailed(_)
            | Update::Devices(_)
            | Update::Remote(_)
            | Update::Transport(_)
            | Update::Cover { .. }
            | Update::CoverFailed { .. } => {}
            Update::Account(account) => self.learn_account(*account),
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
            Page::Home | Page::Album(_) | Page::Artist(_) | Page::Show(_) | Page::Search(_) => true,
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
        // A shelf play names a playlist too, and one that is gone can never
        // fetch its rows.
        if self
            .play
            .as_ref()
            .is_some_and(|play| play.kind == Some(SidebarKind::Playlists) && !present(&play.uri))
        {
            self.play = None;
        }
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
            && self.playlists_live()
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
            SidebarKind::Playlists => self.rootlist_removable() && self.playlists_live(),
            SidebarKind::Albums => live(&self.saved_albums, self.sources.albums),
            SidebarKind::Artists => live(&self.followed_artists, self.sources.artists),
            SidebarKind::Podcasts => live(&self.followed, self.sources.shows),
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
                if !live(&self.saved_albums, self.sources.albums) {
                    return None;
                }
                Command::SetSaved {
                    uri: item.uri.clone(),
                    saved: true,
                }
            }
            SidebarKind::Podcasts => {
                if !live(&self.followed, self.sources.shows) {
                    return None;
                }
                Command::SetSaved {
                    uri: item.uri.clone(),
                    saved: true,
                }
            }
            SidebarKind::Artists => {
                // A save is a write against a set, and a restored copy's list
                // may not carry one.
                if self.sources.artists != Source::Live {
                    return None;
                }
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

    /// Sets the order the show page draws its episodes in.
    pub fn set_show_order(&mut self, order: ShowOrder) {
        self.show_order = order;
        if let Some(Page::Show(uri)) = self.history.current() {
            self.show_orders.insert(uri.clone(), order);
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
            && self.playlists_live()
            && matches!(self.playlists, Load::Ready(_))
            && self.rootlist.is_some()
    }

    /// Whether `uri` may have its name rewritten now.
    ///
    /// The row's own access says the user may write the list; no rootlist
    /// revision is involved, because a rename names the playlist's own.
    pub fn can_rename(&self, uri: &str) -> bool {
        if self.write_in_flight() || !self.playlists_live() {
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

    /// Opens the head's new-playlist field, with the rail's query dropped.
    ///
    /// A created playlist the query does not match would land in the list
    /// already hidden, and the head keeps one field at a time: the create field
    /// and the filter field share the slot.
    pub fn begin_create(&mut self) {
        self.rail_filter.clear();
        self.naming.open();
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
            query: None,
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
    /// kept until the reply lands; a write that fails puts them back — and the
    /// query the last row took with it — and the page says so. Nothing is
    /// re-requested, so a removal can never leave a ready page waiting.
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
        let before = tracks.tracks.clone();
        self.tracks = Load::Ready(TracksFor {
            playlist: playlist.to_owned(),
            tracks: found.kept.clone(),
            access: Some(access.clone()),
            notice: None,
        });
        // Taking out the last row leaves nowhere to show the query, and no
        // reply for a removal carries a track list, so this is the only place
        // that can drop it. The write keeps the words, so a failed one can put
        // the query back beside the rows it restores.
        let query = self.clear_filter_when_empty();
        self.pending = Some(reorder::Pending {
            playlist: playlist.to_owned(),
            before,
            after: found.kept,
            query,
        });
        Some(Command::RemoveTracks {
            playlist: playlist.to_owned(),
            revision: access.revision,
            removals: found.removals,
        })
    }

    /// Drops the page's query when the page it filters has no rows to show, and
    /// hands back the text it took.
    ///
    /// The field is drawn beside rows, so an empty page can never show a query.
    /// Left to sit, it would come back and silently filter the next list that
    /// has rows.
    fn clear_filter_when_empty(&mut self) -> Option<String> {
        if matches!(&self.tracks, Load::Ready(tracks) if tracks.tracks.is_empty()) {
            let text = self.track_filter.take();
            return (!text.is_empty()).then_some(text);
        }
        None
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
mod tests;
