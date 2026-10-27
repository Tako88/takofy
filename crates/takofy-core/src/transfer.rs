//! What a hand-over asked this window to play, and where it lands.
//!
//! A transfer names a context, the phone's own pages, a queue and a current
//! track, and none of them is authoritative on its own. This file decides which
//! of them to use, in that order of cost, and turns the answer into the rows and
//! the row index the queue adopts.

use librespot::core::dealer::protocol::{PlayCommand, SkipTo};
use librespot::core::{SpotifyId, SpotifyUri};
use librespot::protocol::context::Context;
use librespot::protocol::context_track::ContextTrack;
use librespot::protocol::transfer_state::TransferState;
use takofy_model::Landing;

use crate::instruction::ContextKind;

/// Beyond this the phone's snapshot cannot belong to this playback.
///
/// Its state push is event-driven, so a snapshot seconds old is ordinary; five
/// minutes old is a different session's clock, and nothing is added.
pub(crate) const STATE_AGE_MAX: i64 = 300_000;

/// Where the rows to adopt come from.
///
/// The phone's pages cost no read and are what it was playing, so they win; a
/// context read is the fallback, and one track the last resort.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ContextSource {
    Pages,
    Resolve { uri: String },
    SingleTrack,
}

/// Why a transfer was refused before anything was put.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The payload carried nothing to play.
    NoData,
    /// A context this client does not play in full.
    UnsupportedContext(ContextKind),
    /// No current track to land on.
    NoTrack,
    /// The current track is not a track or an episode.
    NotPlayable,
}

impl Refusal {
    /// The word a log carries. Refusing a hand-over is ordinary, and which
    /// refusal it was is the only thing that says whether the phone's context or
    /// its track was the problem.
    pub fn word(&self) -> &'static str {
        match self {
            // The kind is the useful half: "artist" says why the context cannot
            // be adopted, where "unsupported context" leaves the reader looking.
            Self::UnsupportedContext(kind) => kind.word(),
            Self::NoData => "no data",
            Self::NoTrack => "no track",
            Self::NotPlayable => "not playable",
        }
    }
}

/// What a context read answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    Resolved(Vec<String>),
    Failed,
}

/// One hand-over, decoded into what the landing needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferPlan {
    /// The track the phone was playing.
    pub(crate) current: String,
    /// Whether the current track is the queue's head rather than the context's.
    pub(crate) from_queue: bool,
    /// The phone's queue, in its own order.
    pub(crate) queue: Vec<String>,
    /// The context row the queue was detached from.
    pub(crate) associated: Option<String>,
    /// The phone's inline pages, flattened in the order it sent them.
    pub(crate) pages: Vec<String>,
    /// The row's position in the resolved rows, when a play named its row by
    /// position rather than by uri.
    ///
    /// A transfer always lands by uri; only a play that names an index leaves
    /// the row itself to the context read.
    pub(crate) start_index: Option<usize>,
    /// The context the rows belong to, when the payload named a usable one.
    pub(crate) context: Option<String>,
    pub(crate) source: ContextSource,
    /// The phone's position when it took its snapshot, absent if it sent none.
    pub(crate) position_ms: Option<u32>,
    /// The phone's own instant for that snapshot, in unix milliseconds.
    pub(crate) timestamp_ms: Option<i64>,
    pub(crate) paused: bool,
    /// The modes the payload named, absent where it named none so the queue
    /// keeps the mode it has.
    pub(crate) shuffle: Option<bool>,
    pub(crate) repeat_context: Option<bool>,
    pub(crate) repeat_track: Option<bool>,
}

impl TransferPlan {
    /// The track the phone was playing.
    pub fn current(&self) -> &str {
        &self.current
    }

    /// The context to read, when the phone's own pages carry no rows.
    pub fn resolve_uri(&self) -> Option<&str> {
        match &self.source {
            ContextSource::Resolve { uri } => Some(uri),
            _ => None,
        }
    }

    /// The row this plan lands on, given what the context read answered.
    ///
    /// A play can name its row by position rather than by uri, so the row may
    /// only be known once the read has answered. `None` means the landing would
    /// sit on no row this window can load, which is a play to refuse rather than
    /// take the sound with and then fail.
    pub fn anchor(&self, resolution: &Resolution) -> Option<String> {
        let resolved = match resolution {
            Resolution::Resolved(uris) => uris.as_slice(),
            Resolution::Failed => self.pages.as_slice(),
        };
        let (rows, index) = rows_of(self, resolved);
        rows.get(index).filter(|row| playable(row)).cloned()
    }
}

/// The track a page row carries, found by the uid the phone says it was on.
///
/// The pages are the phone's own window, so this is a lookup rather than a read,
/// and it is the only way to land a transfer whose payload names no current
/// track.
fn row_of_uid(uid: Option<&str>, context: Option<&Context>) -> Option<String> {
    let uid = uid.filter(|uid| !uid.is_empty())?;
    context?
        .pages
        .iter()
        .flat_map(|page| page.tracks.iter())
        .find(|track| track.uid.as_deref() == Some(uid))
        .and_then(uri_of_track)
}

/// Reads one hand-over, or says why it cannot be played here.
pub(crate) fn plan(state: &TransferState) -> Result<TransferPlan, Refusal> {
    let playback = state.playback.as_ref();
    let session = state.current_session.as_ref();
    let queue = &state.queue;
    if playback.is_none() && session.is_none() && queue.tracks.is_empty() {
        return Err(Refusal::NoData);
    }

    let context = session.and_then(|session| session.context.as_ref());
    let context_uri = context.and_then(|context| context.uri.as_deref());
    let kind = ContextKind::of_uri(context_uri);
    if !kind.honoured() && kind != ContextKind::None {
        return Err(Refusal::UnsupportedContext(kind));
    }

    let from_queue = queue.is_playing_queue.unwrap_or_default();
    let current_track = if from_queue {
        queue.tracks.first()
    } else {
        playback.and_then(|playback| playback.current_track.as_ref())
    };
    // A real phone sends the row's uid and the pages, and sometimes no current
    // track at all, so the uid is what names the row it was playing.
    let current = match current_track.and_then(uri_of_track) {
        Some(uri) => uri,
        None => row_of_uid(
            session.and_then(|session| session.current_uid.as_deref()),
            context,
        )
        .ok_or(Refusal::NoTrack)?,
    };
    if !playable(&current) {
        return Err(Refusal::NotPlayable);
    }

    let pages: Vec<String> = context.map(pages_of).unwrap_or_default();
    let source = if kind.honoured() {
        if pages.is_empty() {
            ContextSource::Resolve {
                uri: context_uri.unwrap_or_default().to_owned(),
            }
        } else {
            ContextSource::Pages
        }
    } else if pages.is_empty() {
        ContextSource::SingleTrack
    } else {
        ContextSource::Pages
    };

    let options = state.options.as_ref();
    Ok(TransferPlan {
        current,
        from_queue,
        queue: queue.tracks.iter().filter_map(uri_of_track).collect(),
        associated: playback
            .and_then(|playback| playback.associated_current_track.as_ref())
            .and_then(uri_of_track),
        pages,
        start_index: None,
        context: if kind.honoured() {
            context_uri.map(str::to_owned)
        } else {
            None
        },
        source,
        position_ms: playback
            .and_then(|playback| playback.position_as_of_timestamp)
            .and_then(|position| u32::try_from(position).ok()),
        timestamp_ms: playback.and_then(|playback| playback.timestamp),
        paused: playback
            .and_then(|playback| playback.is_paused)
            .unwrap_or_default(),
        shuffle: options.and_then(|options| options.shuffling_context),
        repeat_context: options.and_then(|options| options.repeating_context),
        repeat_track: options.and_then(|options| options.repeating_track),
    })
}

/// Reads one play, or says why the rows it named cannot be played here.
///
/// A play names a context rather than a snapshot, so the context is what is
/// read; the phone's pages are kept because a failed read still has them. The
/// row it starts on comes from `skip_to`, and the last resort is the head of
/// the context.
pub(crate) fn plan_play(command: &PlayCommand) -> Result<TransferPlan, Refusal> {
    let context = &command.context;
    let context_uri = context.uri.as_deref();
    let kind = ContextKind::of_uri(context_uri);
    if !kind.honoured() && kind != ContextKind::None {
        return Err(Refusal::UnsupportedContext(kind));
    }
    let pages = pages_of(context);
    let source = if kind.honoured() {
        ContextSource::Resolve {
            uri: context_uri.unwrap_or_default().to_owned(),
        }
    } else if pages.is_empty() {
        return Err(Refusal::NoData);
    } else {
        ContextSource::Pages
    };

    let (current, start_index) = start_of(command.options.skip_to.as_ref(), &pages);
    let overrides = command.options.player_options_override.as_ref();
    Ok(TransferPlan {
        current,
        from_queue: false,
        queue: Vec::new(),
        associated: None,
        pages,
        start_index,
        context: if kind.honoured() {
            context_uri.map(str::to_owned)
        } else {
            None
        },
        source,
        position_ms: command.options.seek_to,
        // A play carries a position, not the instant it was taken at, so there
        // is no age to add to it.
        timestamp_ms: None,
        paused: command.options.initially_paused.unwrap_or_default(),
        shuffle: overrides.and_then(|options| options.shuffling_context),
        repeat_context: overrides.and_then(|options| options.repeating_context),
        repeat_track: overrides.and_then(|options| options.repeating_track),
    })
}

/// The row a play starts on, and its position when the phone named it that way.
///
/// The phone's uid names a row in its own pages, and the rows that reach the
/// landing carry uris, so a position is the only other way to say it. A play
/// that names nothing starts the context, which is its first row.
fn start_of(skip: Option<&SkipTo>, pages: &[String]) -> (String, Option<usize>) {
    if let Some(uri) = skip
        .and_then(|skip| skip.track_uri.as_deref())
        .filter(|uri| !uri.is_empty())
    {
        return (uri.to_owned(), None);
    }
    let index = skip
        .and_then(|skip| skip.track_index)
        .and_then(|index| usize::try_from(index).ok())
        .unwrap_or(0);
    // The row is only known once the context resolves, so a page carrying it is
    // the best this can say; the index is what the landing follows.
    (pages.get(index).cloned().unwrap_or_default(), Some(index))
}

/// The phone's inline pages, flattened in the order it sent them.
fn pages_of(context: &Context) -> Vec<String> {
    context
        .pages
        .iter()
        .flat_map(|page| page.tracks.iter())
        .filter_map(uri_of_track)
        .collect()
}

/// Turns a plan and what the context read answered into the rows to adopt.
///
/// A failed read does not discard the phone's inline pages: those cost nothing
/// and are what it was playing, so they are tried before the one track.
pub(crate) fn land(plan: &TransferPlan, resolution: Resolution, now_ms: i64) -> Landing {
    let resolved = match resolution {
        Resolution::Resolved(uris) => uris,
        Resolution::Failed => plan.pages.clone(),
    };
    let (rows, index) = rows_of(plan, &resolved);
    Landing {
        rows,
        index,
        position_ms: landing_position(plan.position_ms, plan.timestamp_ms, now_ms, plan.paused),
        playing: !plan.paused,
        context: plan.context.clone(),
        shuffle: plan.shuffle,
        repeat_context: plan.repeat_context,
        repeat_track: plan.repeat_track,
        epoch: 0,
    }
}

/// The position a track lands at, from the phone's snapshot.
///
/// A positive age is real elapsed music, because the phone pushes state on
/// events rather than on a tick. A negative age, or one past [`STATE_AGE_MAX`],
/// means the two clocks disagree and nothing is added: a clamped landing would
/// be a wrong landing.
pub(crate) fn landing_position(
    base: Option<u32>,
    timestamp: Option<i64>,
    now_ms: i64,
    paused: bool,
) -> u32 {
    let Some(base) = base else {
        return 0;
    };
    if paused {
        return base;
    }
    let Some(timestamp) = timestamp else {
        return base;
    };
    let elapsed = now_ms.saturating_sub(timestamp);
    if elapsed <= 0 || elapsed > STATE_AGE_MAX {
        return base;
    }
    base.saturating_add(u32::try_from(elapsed).unwrap_or(u32::MAX))
}

/// Builds the rows and the current row's index.
///
/// The phone's queue plays next, so it follows the current row; a queue that is
/// playing is the whole list, with the context resuming after the row it was
/// detached from.
fn rows_of(plan: &TransferPlan, resolved: &[String]) -> (Vec<String>, usize) {
    if plan.from_queue {
        let mut rows = plan.queue.clone();
        if let Some(associated) = plan.associated.as_deref()
            && let Some(at) = resolved.iter().position(|uri| uri == associated)
        {
            rows.extend_from_slice(&resolved[at + 1..]);
        }
        return (rows, 0);
    }
    // A position past the rows the read answered lands on the last one: the
    // list the phone named is shorter than the one that came back, and the last
    // row is the closest thing it meant.
    let found = match plan.start_index {
        Some(index) if !resolved.is_empty() => Some(index.min(resolved.len() - 1)),
        Some(_) => None,
        None => resolved.iter().position(|uri| *uri == plan.current),
    };
    let (mut rows, tail, index) = match found {
        Some(at) => (resolved[..=at].to_vec(), resolved[at + 1..].to_vec(), at),
        None => (vec![plan.current.clone()], Vec::new(), 0),
    };
    rows.extend(plan.queue.iter().cloned());
    rows.extend(tail);
    (rows, index)
}

/// A track's uri: the string the payload carries, or the id when it carries only
/// that.
///
/// A phone sends rows with a raw id and no uri at all, so reading the uri alone
/// makes every row of a real hand-over invisible. librespot's own receiver reads
/// the two the same way (`state/context.rs`).
fn uri_of_track(track: &ContextTrack) -> Option<String> {
    if let Some(uri) = uri_of(&track.uri) {
        return Some(uri);
    }
    let gid = track.gid.as_deref().filter(|gid| !gid.is_empty())?;
    let id = SpotifyId::from_raw(gid).ok()?;
    let uri = SpotifyUri::Track { id }.to_uri().ok()?;
    // A bare id has no kind of its own, and a context row is always a track.
    Some(uri.replace("unknown", "track"))
}

fn uri_of(uri: &Option<String>) -> Option<String> {
    uri.as_deref()
        .filter(|uri| !uri.is_empty())
        .map(str::to_owned)
}

/// Only a track or an episode can be loaded; a local file cannot be fetched.
fn playable(uri: &str) -> bool {
    matches!(
        SpotifyUri::from_uri(uri),
        Ok(SpotifyUri::Track { .. } | SpotifyUri::Episode { .. })
    )
}

#[cfg(test)]
mod tests;
