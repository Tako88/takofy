//! What one incoming command asked for, in words this client can log.
//!
//! A command's payload can carry track uris and context uris, and the log is
//! read by whoever is watching the account. So the only shape that reaches the
//! log is a summary of counts and kinds, never a uri: a decoded plan carries
//! uris because the queue has to adopt them, and it is never printed. It also
//! decodes the endpoint, so the rest of the crate asks one question instead of
//! matching on the wire's own enum.

use std::fmt;

use librespot::core::dealer::protocol::{Command, PlayCommand, Request, SetQueueCommand};
use librespot::protocol::transfer_state::TransferState;
use takofy_model::Order;

pub use crate::transfer::{Refusal, TransferPlan};
use crate::transfer::{plan, plan_play};

/// The wire's way of saying a transfer has no context.
const NO_CONTEXT: &str = "-";

/// One command endpoint, reduced to the set this client knows how to talk about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Transfer,
    Play,
    Pause,
    Resume,
    SeekTo,
    SkipNext,
    SkipPrev,
    SetShufflingContext,
    SetRepeatingContext,
    SetRepeatingTrack,
    SetOptions,
    SetQueue,
    AddToQueue,
    UpdateContext,
    Unknown,
}

impl Kind {
    /// The fixed word for this endpoint, for a log line.
    pub fn word(self) -> &'static str {
        match self {
            Kind::Transfer => "transfer",
            Kind::Play => "play",
            Kind::Pause => "pause",
            Kind::Resume => "resume",
            Kind::SeekTo => "seek_to",
            Kind::SkipNext => "skip_next",
            Kind::SkipPrev => "skip_prev",
            Kind::SetShufflingContext => "set_shuffling_context",
            Kind::SetRepeatingContext => "set_repeating_context",
            Kind::SetRepeatingTrack => "set_repeating_track",
            Kind::SetOptions => "set_options",
            Kind::SetQueue => "set_queue",
            Kind::AddToQueue => "add_to_queue",
            Kind::UpdateContext => "update_context",
            Kind::Unknown => "unknown",
        }
    }
}

/// What a context uri names, as far as deciding whether it can be played.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextKind {
    Playlist,
    Album,
    Show,
    Collection,
    Artist,
    Station,
    Search,
    Local,
    /// A curated list, such as an artist's popular tracks. Flat, and it resolves
    /// to its rows like a playlist does.
    List,
    Other,
    None,
}

impl ContextKind {
    /// Reads a uri that may be absent, empty, or the wire's "no context" dash.
    pub fn of_uri(uri: Option<&str>) -> ContextKind {
        let Some(uri) = uri else {
            return ContextKind::None;
        };
        if uri.is_empty() || uri == NO_CONTEXT {
            return ContextKind::None;
        }
        let Some((scheme, rest)) = uri.split_once(':') else {
            return ContextKind::Other;
        };
        if scheme != "spotify" {
            return ContextKind::Other;
        }
        match rest.split(':').next() {
            Some("playlist") => ContextKind::Playlist,
            Some("album") => ContextKind::Album,
            Some("show") => ContextKind::Show,
            Some("artist") => ContextKind::Artist,
            Some("station") => ContextKind::Station,
            Some("search") => ContextKind::Search,
            Some("local") => ContextKind::Local,
            Some("list") => ContextKind::List,
            Some("user") => user_context(rest),
            _ => ContextKind::Other,
        }
    }

    /// Whether a transfer of this context can be played in full here.
    ///
    /// The four kinds whose rows this client can resolve and adopt; every other
    /// kind is refused rather than downgraded to one track.
    pub fn honoured(self) -> bool {
        matches!(
            self,
            ContextKind::Playlist
                | ContextKind::Album
                | ContextKind::Show
                | ContextKind::Collection
                | ContextKind::List
        )
    }

    pub(crate) fn word(self) -> &'static str {
        match self {
            ContextKind::Playlist => "playlist",
            ContextKind::Album => "album",
            ContextKind::Show => "show",
            ContextKind::Collection => "collection",
            ContextKind::Artist => "artist",
            ContextKind::Station => "station",
            ContextKind::Search => "search",
            ContextKind::Local => "local",
            ContextKind::List => "list",
            ContextKind::Other => "other",
            ContextKind::None => "none",
        }
    }
}

/// `spotify:user:<name>:<kind>:<id>` — the user prefix hides the real kind.
fn user_context(rest: &str) -> ContextKind {
    match rest.split(':').nth(2) {
        Some("playlist") => ContextKind::Playlist,
        Some("collection") => ContextKind::Collection,
        _ => ContextKind::Other,
    }
}

/// The shape of one transfer's payload, with nothing in it that names a track.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferSummary {
    pub context: ContextKind,
    pub pages: usize,
    pub tracks: usize,
    pub queue: usize,
    pub paused: bool,
    /// Whether the payload named a current track, and whether it named the row
    /// by uid. A hand-over can arrive with neither, and which one was missing is
    /// what says whether the row is findable.
    pub current_track: bool,
    pub current_uid: bool,
    /// `now − playback.timestamp`; absent when the payload carried no instant.
    pub skew_ms: Option<i64>,
}

impl fmt::Display for TransferSummary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "context {}, pages {}, tracks {}, queue {}, paused {}, ",
            self.context.word(),
            self.pages,
            self.tracks,
            self.queue,
            self.paused,
        )?;
        write!(
            f,
            "track {}, uid {}, ",
            named(self.current_track),
            named(self.current_uid),
        )?;
        match self.skew_ms {
            Some(skew) => write!(f, "skew {skew} ms"),
            None => write!(f, "skew none"),
        }
    }
}

/// One payload's presence as a log may carry it.
fn named(present: bool) -> &'static str {
    if present { "named" } else { "absent" }
}

/// The counts a `set_queue` may log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueueSummary {
    pub prev: usize,
    pub next: usize,
}

impl fmt::Display for QueueSummary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "prev {} rows, next {} rows", self.prev, self.next)
    }
}

/// What a `play` named: counts, and which fields its `skip_to` carried.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaySummary {
    pub context: ContextKind,
    pub pages: usize,
    pub tracks: usize,
    /// Whether `skip_to` named the row by uri, by index, and by uid.
    pub uri: bool,
    pub index: bool,
    pub uid: bool,
    pub seek: bool,
    pub paused: bool,
}

impl fmt::Display for PlaySummary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "context {}, pages {}, tracks {}, skip uri {}, index {}, uid {}, ",
            self.context.word(),
            self.pages,
            self.tracks,
            named(self.uri),
            named(self.index),
            named(self.uid),
        )?;
        write!(
            f,
            "seek {}, paused {}",
            named(self.seek),
            named(self.paused)
        )
    }
}

/// What one incoming request asked this window to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Instruction {
    /// A hand-over this window can take, decoded into what the queue adopts.
    Transfer(TransferPlan),
    /// A phone-side play, decoded into the same plan a hand-over makes so both
    /// land through one path.
    Play(TransferPlan),
    /// A hand-over it cannot take, and why, so nothing is put.
    Refused(Refusal),
    /// One transport action the window carries out and answers.
    Order(Order),
    /// An endpoint that names no action and no row: there is nothing to read and
    /// nothing to change.
    Ignored,
    /// Any other endpoint: this window answers it without acting on it.
    Other(Kind),
}

/// Decodes one request into a plan, a refusal, or just its endpoint.
pub fn instruction_of(request: &Request) -> Instruction {
    match &request.command {
        Command::Transfer(transfer) => match transfer.data.as_ref() {
            Some(state) => match plan(state) {
                Ok(plan) => Instruction::Transfer(plan),
                Err(refusal) => Instruction::Refused(refusal),
            },
            None => Instruction::Refused(Refusal::NoData),
        },
        Command::Play(play) => match plan_play(play) {
            Ok(plan) => Instruction::Play(plan),
            Err(refusal) => Instruction::Refused(refusal),
        },
        Command::Pause(_) => Instruction::Order(Order::Pause),
        Command::Resume(_) => Instruction::Order(Order::Resume),
        // The phone's position travels in `value`; `position` is never filled.
        Command::SeekTo(seek) => Instruction::Order(Order::Seek(seek.value)),
        Command::SkipNext(next) => Instruction::Order(Order::Next(
            next.track
                .as_ref()
                .map(|track| track.uri.clone())
                .filter(|uri| !uri.is_empty()),
        )),
        Command::SkipPrev(_) => Instruction::Order(Order::Prev),
        Command::SetShufflingContext(value) => Instruction::Order(Order::SetShuffle(value.value)),
        // The phone's repeat is one three-state switch, so an endpoint that names
        // one half of it says nothing about the other: the mode it leaves unnamed
        // is kept.
        Command::SetRepeatingContext(value) => Instruction::Order(Order::SetOptions {
            shuffling: None,
            repeating_context: Some(value.value),
            repeating_track: None,
        }),
        Command::SetRepeatingTrack(value) => Instruction::Order(Order::SetOptions {
            shuffling: None,
            repeating_context: None,
            repeating_track: Some(value.value),
        }),
        Command::SetOptions(options) => Instruction::Order(Order::SetOptions {
            shuffling: options.shuffling_context,
            repeating_context: options.repeating_context,
            repeating_track: options.repeating_track,
        }),
        Command::AddToQueue(add) => Instruction::Order(Order::AddToQueue(
            Some(add.track.uri.clone()).filter(|uri| !uri.is_empty()),
        )),
        Command::SetQueue(queue) => Instruction::Order(Order::SetQueue {
            prev: uris_of(&queue.prev_tracks),
            next: uris_of(&queue.next_tracks),
            revision: queue.queue_revision.clone(),
        }),
        // A context names where playback lives, not what to change here: the
        // window's own rows are the queue, whatever the phone is browsing.
        Command::UpdateContext(_) => Instruction::Ignored,
        other => Instruction::Other(kind_of(other)),
    }
}

/// The uris a queue window's list carries, in its own order.
fn uris_of(tracks: &[librespot::protocol::player::ProvidedTrack]) -> Vec<String> {
    tracks.iter().map(|track| track.uri.clone()).collect()
}

/// Which endpoint one command names.
pub fn kind_of(command: &Command) -> Kind {
    match command {
        Command::Transfer(_) => Kind::Transfer,
        Command::Play(_) => Kind::Play,
        Command::Pause(_) => Kind::Pause,
        Command::Resume(_) => Kind::Resume,
        Command::SeekTo(_) => Kind::SeekTo,
        Command::SkipNext(_) => Kind::SkipNext,
        Command::SkipPrev(_) => Kind::SkipPrev,
        Command::SetShufflingContext(_) => Kind::SetShufflingContext,
        Command::SetRepeatingContext(_) => Kind::SetRepeatingContext,
        Command::SetRepeatingTrack(_) => Kind::SetRepeatingTrack,
        Command::SetOptions(_) => Kind::SetOptions,
        Command::SetQueue(_) => Kind::SetQueue,
        Command::AddToQueue(_) => Kind::AddToQueue,
        Command::UpdateContext(_) => Kind::UpdateContext,
        Command::Unknown(_) => Kind::Unknown,
    }
}

/// Reduces a transfer's payload to counts, kinds and the clock gap.
pub fn summarise(state: &TransferState, now_ms: i64) -> TransferSummary {
    let context = state.current_session.context.as_ref();
    let playback = state.playback.as_ref();
    TransferSummary {
        context: ContextKind::of_uri(context.and_then(|context| context.uri.as_deref())),
        pages: context.map_or(0, |context| context.pages.len()),
        tracks: context.map_or(0, |context| {
            context.pages.iter().map(|page| page.tracks.len()).sum()
        }),
        queue: state.queue.tracks.len(),
        paused: playback.is_some_and(|playback| playback.is_paused.unwrap_or_default()),
        current_track: playback.is_some_and(|playback| playback.current_track.is_some()),
        current_uid: state
            .current_session
            .current_uid
            .as_deref()
            .is_some_and(|uid| !uid.is_empty()),
        skew_ms: playback
            .and_then(|playback| playback.timestamp)
            .map(|timestamp| now_ms.saturating_sub(timestamp)),
    }
}

/// Reduces a `set_queue` window to the length of each of its lists.
pub fn summarise_queue(command: &SetQueueCommand) -> QueueSummary {
    QueueSummary {
        prev: command.prev_tracks.len(),
        next: command.next_tracks.len(),
    }
}

/// Reduces a `play` to what it named, never to a uri.
pub fn summarise_play(command: &PlayCommand) -> PlaySummary {
    let context = &command.context;
    let skip = command.options.skip_to.as_ref();
    PlaySummary {
        context: ContextKind::of_uri(context.uri.as_deref()),
        pages: context.pages.len(),
        tracks: context.pages.iter().map(|page| page.tracks.len()).sum(),
        uri: skip.is_some_and(|skip| skip.track_uri.is_some()),
        index: skip.is_some_and(|skip| skip.track_index.is_some()),
        uid: skip.is_some_and(|skip| skip.track_uid.is_some()),
        seek: command.options.seek_to.is_some(),
        paused: command.options.initially_paused.is_some(),
    }
}

#[cfg(test)]
mod tests;
