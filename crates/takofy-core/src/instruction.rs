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
mod tests {
    use librespot::core::dealer::protocol::{
        AddToQueueCommand, Command, GenericCommand, LoggingParams, PauseCommand, PlayCommand,
        PlayOptions, SeekToCommand, SetOptionsCommand, SetQueueCommand, SetValueCommand,
        SkipNextCommand, SkipTo, TransferCommand, TransferOptions, UpdateContextCommand,
    };
    use librespot::protocol::context::Context;
    use librespot::protocol::context_page::ContextPage;
    use librespot::protocol::context_player_options::ContextPlayerOptionOverrides;
    use librespot::protocol::context_track::ContextTrack;
    use librespot::protocol::playback::Playback;
    use librespot::protocol::player::{PlayOrigin, ProvidedTrack};
    use librespot::protocol::queue::Queue;
    use librespot::protocol::session::Session;
    use protobuf::MessageField;
    use takofy_model::Landing;

    use super::*;
    use crate::Resolution;

    fn logging() -> LoggingParams {
        LoggingParams {
            interaction_ids: None,
            device_identifier: None,
            command_initiated_time: None,
            page_instance_ids: None,
            command_id: None,
        }
    }

    fn play_options() -> PlayOptions {
        PlayOptions {
            skip_to: None,
            player_options_override: None,
            license: None,
            seek_to: None,
            always_play_something: None,
            audio_stream: None,
            initially_paused: None,
            prefetch_level: None,
            system_initiated: None,
        }
    }

    fn track(uri: &str) -> ContextTrack {
        ContextTrack {
            uri: Some(uri.to_owned()),
            ..Default::default()
        }
    }

    fn page(uris: &[&str]) -> ContextPage {
        ContextPage {
            tracks: uris.iter().map(|uri| track(uri)).collect(),
            ..Default::default()
        }
    }

    fn commands() -> Vec<(Command, Kind)> {
        vec![
            (
                Command::Transfer(TransferCommand {
                    data: None,
                    options: TransferOptions::default(),
                    from_device_identifier: String::new(),
                    logging_params: logging(),
                }),
                Kind::Transfer,
            ),
            (
                Command::Play(Box::new(PlayCommand {
                    context: Context::default(),
                    play_origin: PlayOrigin::default(),
                    options: play_options(),
                    logging_params: logging(),
                })),
                Kind::Play,
            ),
            (
                Command::Pause(PauseCommand {
                    logging_params: logging(),
                }),
                Kind::Pause,
            ),
            (
                Command::Resume(GenericCommand {
                    logging_params: logging(),
                }),
                Kind::Resume,
            ),
            (
                Command::SeekTo(SeekToCommand {
                    value: 0,
                    position: 0,
                    logging_params: logging(),
                }),
                Kind::SeekTo,
            ),
            (
                Command::SkipNext(SkipNextCommand {
                    track: None,
                    logging_params: logging(),
                }),
                Kind::SkipNext,
            ),
            (
                Command::SkipPrev(GenericCommand {
                    logging_params: logging(),
                }),
                Kind::SkipPrev,
            ),
            (
                Command::SetShufflingContext(SetValueCommand {
                    value: true,
                    logging_params: logging(),
                }),
                Kind::SetShufflingContext,
            ),
            (
                Command::SetRepeatingContext(SetValueCommand {
                    value: true,
                    logging_params: logging(),
                }),
                Kind::SetRepeatingContext,
            ),
            (
                Command::SetRepeatingTrack(SetValueCommand {
                    value: true,
                    logging_params: logging(),
                }),
                Kind::SetRepeatingTrack,
            ),
            (
                Command::SetOptions(SetOptionsCommand {
                    shuffling_context: None,
                    repeating_context: None,
                    repeating_track: None,
                    options: None,
                    logging_params: logging(),
                }),
                Kind::SetOptions,
            ),
            (
                Command::SetQueue(SetQueueCommand {
                    next_tracks: Vec::new(),
                    prev_tracks: Vec::new(),
                    queue_revision: String::new(),
                    logging_params: logging(),
                }),
                Kind::SetQueue,
            ),
            (
                Command::AddToQueue(AddToQueueCommand {
                    track: ProvidedTrack {
                        uri: "spotify:track:1".to_owned(),
                        ..Default::default()
                    },
                    logging_params: logging(),
                }),
                Kind::AddToQueue,
            ),
            (
                Command::UpdateContext(UpdateContextCommand {
                    context: Context::default(),
                    session_id: None,
                }),
                Kind::UpdateContext,
            ),
        ]
    }

    #[test]
    fn each_endpoint_has_its_kind() {
        let table = commands();
        assert_eq!(table.len(), 14, "the table covers every known endpoint");

        for (command, expected) in table {
            assert_eq!(kind_of(&command), expected, "{command}");
        }
    }

    #[test]
    fn an_unknown_endpoint_is_unknown() {
        let payload = serde_json::json!({"endpoint": "something_new", "value": 1});

        assert_eq!(kind_of(&Command::Unknown(payload)), Kind::Unknown);
    }

    #[test]
    fn kind_words_are_fixed_and_distinct() {
        let cases = [
            (Kind::Transfer, "transfer"),
            (Kind::Play, "play"),
            (Kind::Pause, "pause"),
            (Kind::Resume, "resume"),
            (Kind::SeekTo, "seek_to"),
            (Kind::SkipNext, "skip_next"),
            (Kind::SkipPrev, "skip_prev"),
            (Kind::SetShufflingContext, "set_shuffling_context"),
            (Kind::SetRepeatingContext, "set_repeating_context"),
            (Kind::SetRepeatingTrack, "set_repeating_track"),
            (Kind::SetOptions, "set_options"),
            (Kind::SetQueue, "set_queue"),
            (Kind::AddToQueue, "add_to_queue"),
            (Kind::UpdateContext, "update_context"),
            (Kind::Unknown, "unknown"),
        ];

        let mut words: Vec<&str> = Vec::new();
        for (kind, expected) in cases {
            assert_eq!(kind.word(), expected);
            assert!(
                !words.contains(&expected),
                "{expected} names more than one kind"
            );
            words.push(expected);
        }
    }

    #[test]
    fn context_kind_table() {
        let cases = [
            (
                Some("spotify:playlist:37i9dQZF1DXcBWIGoYBM5M"),
                ContextKind::Playlist,
            ),
            (
                Some("spotify:user:spotify:playlist:37i9dQZF1DXcBWIGoYBM5M"),
                ContextKind::Playlist,
            ),
            (
                Some("spotify:album:1DFixLWuPkv3KT3TnV35m3"),
                ContextKind::Album,
            ),
            (
                Some("spotify:show:5CfCWKI5pZ28U0uOJ0ZZAK"),
                ContextKind::Show,
            ),
            (
                Some("spotify:user:someone:collection"),
                ContextKind::Collection,
            ),
            (
                Some("spotify:artist:0OdUWJ0sBjDrqHygGUXeCF"),
                ContextKind::Artist,
            ),
            (
                Some("spotify:station:artist:0OdUWJ0sBjDrqHygGUXeCF"),
                ContextKind::Station,
            ),
            (Some("spotify:search:radiohead"), ContextKind::Search),
            (
                Some("spotify:local:artist:album:title:200"),
                ContextKind::Local,
            ),
            (
                // An artist's popular tracks: the phone's "Popular" list.
                Some(
                    "spotify:list:popular-release-segments-main-roles:artist_0OdUWJ0sBjDrqHygGUXeCF",
                ),
                ContextKind::List,
            ),
            (
                Some("spotify:track:4uLU6hMCjMI75M1A2tKUQC"),
                ContextKind::Other,
            ),
            (Some("https://example.test/list"), ContextKind::Other),
            (Some(""), ContextKind::None),
            (Some("-"), ContextKind::None),
            (None, ContextKind::None),
        ];

        for (uri, expected) in cases {
            assert_eq!(ContextKind::of_uri(uri), expected, "{uri:?}");
        }
    }

    #[test]
    fn every_flat_context_is_honoured_and_the_rest_are_not() {
        let cases = [
            (ContextKind::Playlist, true),
            (ContextKind::Album, true),
            (ContextKind::Show, true),
            (ContextKind::Collection, true),
            (ContextKind::List, true),
            (ContextKind::Artist, false),
            (ContextKind::Station, false),
            (ContextKind::Search, false),
            (ContextKind::Local, false),
            (ContextKind::Other, false),
            (ContextKind::None, false),
        ];

        for (kind, expected) in cases {
            assert_eq!(kind.honoured(), expected, "{kind:?}");
        }
    }

    /// The wire's ids are 22 base62 characters, so a fixture has to be one.
    const TRACK_1: &str = "spotify:track:4uLU6hMCjMI75M1A2tKUQ1";
    const TRACK_2: &str = "spotify:track:4uLU6hMCjMI75M1A2tKUQ2";
    const TRACK_3: &str = "spotify:track:4uLU6hMCjMI75M1A2tKUQ3";
    const TRACK_4: &str = "spotify:track:4uLU6hMCjMI75M1A2tKUQ4";
    const PLAYLIST: &str = "spotify:playlist:37i9dQZF1DXcBWIGoYBM5M";
    const ARTIST: &str = "spotify:artist:0OdUWJ0sBjDrqHygGUXeCF";
    const LOCAL: &str = "spotify:local:artist:album:title:200";

    /// The uris of `rows`, in the order given.
    fn uris(rows: &[&str]) -> Vec<String> {
        rows.iter().map(|uri| (*uri).to_owned()).collect()
    }

    fn request(command: Command) -> Request {
        Request {
            message_id: 1,
            sent_by_device_id: "phone".to_owned(),
            command,
        }
    }

    fn transfer(data: Option<TransferState>) -> Command {
        Command::Transfer(TransferCommand {
            data,
            options: TransferOptions::default(),
            from_device_identifier: String::new(),
            logging_params: logging(),
        })
    }

    /// A paused album whose current track sits inside its own page.
    fn state(context: &str) -> TransferState {
        TransferState {
            current_session: MessageField::some(Session {
                context: MessageField::some(Context {
                    uri: Some(context.to_owned()),
                    pages: vec![page(&[TRACK_1, TRACK_2])],
                    ..Default::default()
                }),
                ..Default::default()
            }),
            playback: MessageField::some(Playback {
                current_track: MessageField::some(track(TRACK_2)),
                is_paused: Some(true),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn a_transfer_with_data_is_a_plan() {
        let state = state(PLAYLIST);
        let expected = crate::transfer::plan(&state).expect("the fixture is playable");

        let instruction = instruction_of(&request(transfer(Some(state))));

        assert_eq!(instruction, Instruction::Transfer(expected.clone()));
        let Instruction::Transfer(plan) = instruction else {
            panic!("a playable transfer must decode into a plan");
        };
        assert_eq!(plan.current, TRACK_2);
        assert_eq!(plan.pages, vec![TRACK_1.to_owned(), TRACK_2.to_owned()]);
    }

    #[test]
    fn a_transfer_without_data_is_refused_no_data() {
        assert_eq!(
            instruction_of(&request(transfer(None))),
            Instruction::Refused(Refusal::NoData)
        );
    }

    #[test]
    fn an_unsupported_context_is_a_refusal() {
        assert_eq!(
            instruction_of(&request(transfer(Some(state(ARTIST))))),
            Instruction::Refused(Refusal::UnsupportedContext(ContextKind::Artist))
        );
    }

    #[test]
    fn every_known_endpoint_decodes_to_its_own_instruction() {
        let table = [
            (transfer(None), Instruction::Refused(Refusal::NoData)),
            (
                play(None, Vec::new(), play_options()),
                Instruction::Refused(Refusal::NoData),
            ),
            (
                Command::Pause(PauseCommand {
                    logging_params: logging(),
                }),
                Instruction::Order(Order::Pause),
            ),
            (
                Command::Resume(GenericCommand {
                    logging_params: logging(),
                }),
                Instruction::Order(Order::Resume),
            ),
            (
                Command::SeekTo(SeekToCommand {
                    value: 42_000,
                    position: 0,
                    logging_params: logging(),
                }),
                Instruction::Order(Order::Seek(42_000)),
            ),
            (
                Command::SkipNext(SkipNextCommand {
                    track: None,
                    logging_params: logging(),
                }),
                Instruction::Order(Order::Next(None)),
            ),
            (
                Command::SkipPrev(GenericCommand {
                    logging_params: logging(),
                }),
                Instruction::Order(Order::Prev),
            ),
            (
                Command::SetShufflingContext(SetValueCommand {
                    value: true,
                    logging_params: logging(),
                }),
                Instruction::Order(Order::SetShuffle(true)),
            ),
            (
                Command::SetRepeatingContext(SetValueCommand {
                    value: true,
                    logging_params: logging(),
                }),
                Instruction::Order(Order::SetOptions {
                    shuffling: None,
                    repeating_context: Some(true),
                    repeating_track: None,
                }),
            ),
            (
                Command::SetRepeatingTrack(SetValueCommand {
                    value: true,
                    logging_params: logging(),
                }),
                Instruction::Order(Order::SetOptions {
                    shuffling: None,
                    repeating_context: None,
                    repeating_track: Some(true),
                }),
            ),
            (
                Command::SetOptions(SetOptionsCommand {
                    shuffling_context: Some(true),
                    repeating_context: Some(false),
                    repeating_track: None,
                    options: None,
                    logging_params: logging(),
                }),
                Instruction::Order(Order::SetOptions {
                    shuffling: Some(true),
                    repeating_context: Some(false),
                    repeating_track: None,
                }),
            ),
            (
                Command::AddToQueue(AddToQueueCommand {
                    track: provided(TRACK_1),
                    logging_params: logging(),
                }),
                Instruction::Order(Order::AddToQueue(Some(TRACK_1.to_owned()))),
            ),
            (
                Command::SetQueue(SetQueueCommand {
                    next_tracks: vec![provided(TRACK_2)],
                    prev_tracks: vec![provided(TRACK_1)],
                    queue_revision: "rev-7".to_owned(),
                    logging_params: logging(),
                }),
                Instruction::Order(Order::SetQueue {
                    prev: vec![TRACK_1.to_owned()],
                    next: vec![TRACK_2.to_owned()],
                    revision: "rev-7".to_owned(),
                }),
            ),
            (
                Command::UpdateContext(UpdateContextCommand {
                    context: Context::default(),
                    session_id: None,
                }),
                Instruction::Ignored,
            ),
        ];

        assert_eq!(
            table.len(),
            commands().len(),
            "every endpoint in the kind table decodes here too"
        );
        for (command, expected) in table {
            assert_eq!(instruction_of(&request(command)), expected);
        }

        assert!(
            matches!(
                instruction_of(&request(play(None, vec![page(&[TRACK_1])], play_options()))),
                Instruction::Play(_)
            ),
            "a play that names rows decodes into the plan those rows land from"
        );

        let payload = serde_json::json!({"endpoint": "something_new"});
        assert_eq!(
            instruction_of(&request(Command::Unknown(payload))),
            Instruction::Other(Kind::Unknown),
            "an endpoint nobody knows stays unknown"
        );
    }

    /// One row the phone sends as a queue entry, with the uid and metadata that
    /// name nothing here.
    fn provided(uri: &str) -> ProvidedTrack {
        ProvidedTrack {
            uri: uri.to_owned(),
            uid: "a uid".to_owned(),
            metadata: std::collections::HashMap::from([("title".to_owned(), "a title".to_owned())]),
            ..Default::default()
        }
    }

    /// A play naming `context`, carrying `pages`.
    fn play(context: Option<&str>, pages: Vec<ContextPage>, options: PlayOptions) -> Command {
        Command::Play(Box::new(PlayCommand {
            context: Context {
                uri: context.map(str::to_owned),
                pages,
                ..Default::default()
            },
            play_origin: PlayOrigin::default(),
            options,
            logging_params: logging(),
        }))
    }

    fn play_plan(command: Command) -> TransferPlan {
        match instruction_of(&request(command)) {
            Instruction::Play(plan) => plan,
            other => panic!("a playable play must decode into a plan: {other:?}"),
        }
    }

    fn skip_to(uri: Option<&str>, index: Option<u32>, uid: Option<&str>) -> PlayOptions {
        PlayOptions {
            skip_to: Some(SkipTo {
                track_uid: uid.map(str::to_owned),
                track_uri: uri.map(str::to_owned),
                track_index: index,
            }),
            ..play_options()
        }
    }

    /// What one plan lands on, given the rows a context read answered.
    fn landed(plan: &TransferPlan, rows: &[&str]) -> Landing {
        let rows: Vec<String> = rows.iter().map(|row| (*row).to_owned()).collect();
        crate::transfer::land(plan, crate::transfer::Resolution::Resolved(rows), 0)
    }

    #[test]
    fn each_endpoint_decodes_to_its_order() {
        let table = [
            (
                Command::Pause(PauseCommand {
                    logging_params: logging(),
                }),
                Order::Pause,
            ),
            (
                Command::Resume(GenericCommand {
                    logging_params: logging(),
                }),
                Order::Resume,
            ),
            // The phone's position travels in `value`; `position` stays zero.
            (
                Command::SeekTo(SeekToCommand {
                    value: 42_000,
                    position: 0,
                    logging_params: logging(),
                }),
                Order::Seek(42_000),
            ),
            (
                Command::SkipNext(SkipNextCommand {
                    track: None,
                    logging_params: logging(),
                }),
                Order::Next(None),
            ),
            (
                Command::SkipNext(SkipNextCommand {
                    track: Some(provided(TRACK_1)),
                    logging_params: logging(),
                }),
                Order::Next(Some(TRACK_1.to_owned())),
            ),
            (
                Command::SkipPrev(GenericCommand {
                    logging_params: logging(),
                }),
                Order::Prev,
            ),
            (
                Command::SetShufflingContext(SetValueCommand {
                    value: true,
                    logging_params: logging(),
                }),
                Order::SetShuffle(true),
            ),
            (
                Command::SetShufflingContext(SetValueCommand {
                    value: false,
                    logging_params: logging(),
                }),
                Order::SetShuffle(false),
            ),
            (
                Command::SetRepeatingContext(SetValueCommand {
                    value: true,
                    logging_params: logging(),
                }),
                Order::SetOptions {
                    shuffling: None,
                    repeating_context: Some(true),
                    repeating_track: None,
                },
            ),
            (
                Command::SetRepeatingContext(SetValueCommand {
                    value: false,
                    logging_params: logging(),
                }),
                Order::SetOptions {
                    shuffling: None,
                    repeating_context: Some(false),
                    repeating_track: None,
                },
            ),
            (
                Command::SetRepeatingTrack(SetValueCommand {
                    value: true,
                    logging_params: logging(),
                }),
                Order::SetOptions {
                    shuffling: None,
                    repeating_context: None,
                    repeating_track: Some(true),
                },
            ),
            (
                Command::SetRepeatingTrack(SetValueCommand {
                    value: false,
                    logging_params: logging(),
                }),
                Order::SetOptions {
                    shuffling: None,
                    repeating_context: None,
                    repeating_track: Some(false),
                },
            ),
            (
                Command::SetOptions(SetOptionsCommand {
                    shuffling_context: Some(true),
                    repeating_context: Some(false),
                    repeating_track: None,
                    options: None,
                    logging_params: logging(),
                }),
                Order::SetOptions {
                    shuffling: Some(true),
                    repeating_context: Some(false),
                    repeating_track: None,
                },
            ),
            (
                Command::AddToQueue(AddToQueueCommand {
                    track: provided(TRACK_1),
                    logging_params: logging(),
                }),
                Order::AddToQueue(Some(TRACK_1.to_owned())),
            ),
            (
                Command::AddToQueue(AddToQueueCommand {
                    track: ProvidedTrack::default(),
                    logging_params: logging(),
                }),
                Order::AddToQueue(None),
            ),
            (
                Command::SetQueue(SetQueueCommand {
                    next_tracks: vec![provided(TRACK_2), provided(TRACK_3)],
                    prev_tracks: vec![provided(TRACK_1)],
                    queue_revision: "rev-7".to_owned(),
                    logging_params: logging(),
                }),
                Order::SetQueue {
                    prev: vec![TRACK_1.to_owned()],
                    next: vec![TRACK_2.to_owned(), TRACK_3.to_owned()],
                    revision: "rev-7".to_owned(),
                },
            ),
        ];

        for (command, expected) in table {
            assert_eq!(
                instruction_of(&request(command)),
                Instruction::Order(expected)
            );
        }
    }

    #[test]
    fn set_options_carries_only_the_fields_present() {
        let order = |command: Command| match instruction_of(&request(command)) {
            Instruction::Order(order) => order,
            other => panic!("{other:?}"),
        };

        assert_eq!(
            order(Command::SetOptions(SetOptionsCommand {
                shuffling_context: None,
                repeating_context: None,
                repeating_track: None,
                options: None,
                logging_params: logging(),
            })),
            Order::SetOptions {
                shuffling: None,
                repeating_context: None,
                repeating_track: None,
            },
            "a set_options that names nothing must not invent a value"
        );
        assert_eq!(
            order(Command::SetOptions(SetOptionsCommand {
                shuffling_context: None,
                repeating_context: None,
                repeating_track: Some(true),
                options: None,
                logging_params: logging(),
            })),
            Order::SetOptions {
                shuffling: None,
                repeating_context: None,
                repeating_track: Some(true),
            }
        );
    }

    #[test]
    fn update_context_decodes_to_ignored() {
        assert_eq!(
            instruction_of(&request(Command::UpdateContext(UpdateContextCommand {
                context: Context::default(),
                session_id: None,
            }))),
            Instruction::Ignored
        );
    }

    #[test]
    fn set_queue_decodes_both_lists_and_the_revision_string() {
        let instruction = instruction_of(&request(Command::SetQueue(SetQueueCommand {
            next_tracks: vec![provided(TRACK_2), provided(TRACK_3)],
            prev_tracks: vec![provided(TRACK_1)],
            queue_revision: "rev-7".to_owned(),
            logging_params: logging(),
        })));

        assert_eq!(
            instruction,
            Instruction::Order(Order::SetQueue {
                prev: vec![TRACK_1.to_owned()],
                next: vec![TRACK_2.to_owned(), TRACK_3.to_owned()],
                revision: "rev-7".to_owned(),
            }),
            "a window carries uris and the previous revision, nothing else"
        );
    }

    #[test]
    fn play_with_a_uri_resolves_it() {
        let plan = play_plan(play(
            Some(PLAYLIST),
            vec![page(&[TRACK_1, TRACK_2])],
            play_options(),
        ));

        assert_eq!(plan.resolve_uri(), Some(PLAYLIST));
        assert_eq!(plan.context.as_deref(), Some(PLAYLIST));
        assert_eq!(plan.current, TRACK_1, "a play with no skip_to starts it");
        assert_eq!(
            plan.pages.len(),
            2,
            "the phone's pages are kept for a read that fails"
        );
    }

    #[test]
    fn play_with_pages_only_uses_them() {
        for context in [None, Some(""), Some("-")] {
            let plan = play_plan(play(
                context,
                vec![page(&[TRACK_1, TRACK_2, TRACK_3])],
                play_options(),
            ));

            assert_eq!(plan.resolve_uri(), None, "{context:?}");
            assert_eq!(plan.context, None, "{context:?}");
            assert_eq!(
                plan.pages,
                vec![TRACK_1.to_owned(), TRACK_2.to_owned(), TRACK_3.to_owned()],
                "{context:?}"
            );
            assert_eq!(plan.current, TRACK_1, "{context:?}");
        }
    }

    #[test]
    fn play_with_neither_is_refused() {
        assert_eq!(
            instruction_of(&request(play(None, Vec::new(), play_options()))),
            Instruction::Refused(Refusal::NoData)
        );
    }

    #[test]
    fn play_skip_to_by_uri_lands_on_that_row() {
        let plan = play_plan(play(
            Some(PLAYLIST),
            Vec::new(),
            skip_to(Some(TRACK_3), Some(0), Some("uid-1")),
        ));

        let landing = landed(&plan, &[TRACK_1, TRACK_2, TRACK_3, TRACK_4]);
        assert_eq!(landing.index, 2, "the uri names the row, not the index");
        assert_eq!(landing.rows.len(), 4);
    }

    #[test]
    fn play_skip_to_by_index_lands_on_that_row_when_there_is_no_uri() {
        let plan = play_plan(play(
            Some(PLAYLIST),
            Vec::new(),
            skip_to(None, Some(3), Some("uid-4")),
        ));

        let landing = landed(&plan, &[TRACK_1, TRACK_2, TRACK_3, TRACK_4]);
        assert_eq!(landing.index, 3);
        assert_eq!(landing.rows[3], TRACK_4);
    }

    #[test]
    fn play_skip_to_with_no_fields_starts_the_context() {
        let plan = play_plan(play(Some(PLAYLIST), Vec::new(), skip_to(None, None, None)));

        let landing = landed(&plan, &[TRACK_1, TRACK_2, TRACK_3]);
        assert_eq!(landing.index, 0);
        assert_eq!(landing.rows[0], TRACK_1);
    }

    #[test]
    fn play_skip_to_an_index_past_the_context_lands_on_its_last_row() {
        let plan = play_plan(play(
            Some(PLAYLIST),
            Vec::new(),
            skip_to(None, Some(99), None),
        ));

        let landing = landed(&plan, &[TRACK_1, TRACK_2]);
        assert_eq!(
            landing.index, 1,
            "a position past the rows the read answered is the last one"
        );
        assert_eq!(landing.rows.len(), 2);
    }

    #[test]
    fn play_applies_seek_and_initially_paused() {
        let options = PlayOptions {
            seek_to: Some(42_000),
            initially_paused: Some(true),
            ..play_options()
        };
        let plan = play_plan(play(
            Some(PLAYLIST),
            vec![page(&[TRACK_1, TRACK_2])],
            options,
        ));

        assert_eq!(plan.position_ms, Some(42_000));
        assert!(plan.paused);
        let landing = landed(&plan, &[TRACK_1, TRACK_2]);
        assert_eq!(landing.position_ms, 42_000, "a seek is taken as it is");
        assert!(!landing.playing, "initially paused means it does not start");

        let playing = play_plan(play(Some(PLAYLIST), vec![page(&[TRACK_1])], play_options()));
        assert_eq!(playing.position_ms, None);
        assert!(!playing.paused, "a play that says nothing starts at once");
    }

    #[test]
    fn play_carries_the_options_override() {
        let options = PlayOptions {
            player_options_override: Some(ContextPlayerOptionOverrides {
                shuffling_context: Some(true),
                repeating_context: Some(false),
                repeating_track: Some(true),
                ..Default::default()
            }),
            ..play_options()
        };
        let plan = play_plan(play(None, vec![page(&[TRACK_1])], options));

        assert_eq!(plan.shuffle, Some(true));
        assert_eq!(plan.repeat_context, Some(false));
        assert_eq!(plan.repeat_track, Some(true));

        // A play that names no options names no mode, so the queue keeps the
        // ones it has rather than having them read back as off.
        let bare = play_plan(play(None, vec![page(&[TRACK_1])], play_options()));
        assert_eq!(bare.shuffle, None);
        assert_eq!(bare.repeat_context, None);
        assert_eq!(bare.repeat_track, None);
    }

    #[test]
    fn play_of_an_unsupported_context_is_refused() {
        assert_eq!(
            instruction_of(&request(play(
                Some(ARTIST),
                Vec::new(),
                skip_to(Some(TRACK_1), None, None),
            ))),
            Instruction::Refused(Refusal::UnsupportedContext(ContextKind::Artist))
        );
    }

    #[test]
    fn a_play_that_names_its_row_by_position_anchors_once_the_read_answers() {
        let plan = play_plan(play(
            Some(PLAYLIST),
            Vec::new(),
            skip_to(None, Some(1), None),
        ));

        assert_eq!(
            plan.anchor(&Resolution::Failed),
            None,
            "nothing was named by uri and the read brought no rows"
        );
        assert_eq!(
            plan.anchor(&Resolution::Resolved(Vec::new())),
            None,
            "an empty read is nothing to load either"
        );
        assert_eq!(
            plan.anchor(&Resolution::Resolved(vec![
                TRACK_1.to_owned(),
                TRACK_2.to_owned()
            ])),
            Some(TRACK_2.to_owned()),
            "the row the position names is the one to load"
        );
    }

    #[test]
    fn a_play_that_names_its_row_by_uri_anchors_without_a_read() {
        let plan = play_plan(play(
            Some(PLAYLIST),
            Vec::new(),
            skip_to(Some(TRACK_3), None, None),
        ));

        assert_eq!(
            plan.anchor(&Resolution::Failed),
            Some(TRACK_3.to_owned()),
            "a named row is the row even when the context read fails"
        );
    }

    #[test]
    fn a_play_of_a_local_or_unsupported_row_has_nothing_to_anchor_on() {
        let by_uri = play_plan(play(
            Some(PLAYLIST),
            Vec::new(),
            skip_to(Some(LOCAL), None, None),
        ));
        assert_eq!(
            by_uri.anchor(&Resolution::Resolved(uris(&[TRACK_1, TRACK_2]))),
            None,
            "a local file cannot be fetched, so a play of one is not a load"
        );

        let by_index = play_plan(play(
            Some(PLAYLIST),
            Vec::new(),
            skip_to(None, Some(1), None),
        ));
        assert_eq!(
            by_index.anchor(&Resolution::Resolved(uris(&[TRACK_1, LOCAL]))),
            None,
            "the row a position lands on is refused when that row is a local file"
        );
    }

    #[test]
    fn a_failed_read_resolves_the_named_position_against_the_phones_pages() {
        // The read brought nothing, so the pages the play carried are all there
        // is to find the row its position names.
        let plan = play_plan(play(
            Some(PLAYLIST),
            vec![page(&[TRACK_1, TRACK_2, TRACK_3])],
            skip_to(None, Some(1), None),
        ));

        assert_eq!(
            plan.anchor(&Resolution::Failed),
            Some(TRACK_2.to_owned()),
            "a failed read still has the pages the phone sent"
        );

        // Past the pages, the last row they carry is the closest the position
        // can mean; the row it names is not among them.
        let past = play_plan(play(
            Some(PLAYLIST),
            vec![page(&[TRACK_1, TRACK_2])],
            skip_to(None, Some(3), None),
        ));
        assert_eq!(
            past.anchor(&Resolution::Failed),
            Some(TRACK_2.to_owned()),
            "the position is resolved against the pages, not against nothing"
        );
    }

    #[test]
    fn a_play_with_a_track_uri_the_rows_lack_lands_on_that_row_alone() {
        // The phone named a row the context does not carry. It is still the row
        // it asked for, so it lands on that row alone rather than on a neighbour
        // or on nothing at all.
        let plan = play_plan(play(
            Some(PLAYLIST),
            Vec::new(),
            skip_to(Some(TRACK_4), None, None),
        ));

        assert_eq!(
            plan.anchor(&Resolution::Resolved(uris(&[TRACK_1, TRACK_2]))),
            Some(TRACK_4.to_owned())
        );
        let landing = landed(&plan, &[TRACK_1, TRACK_2]);
        assert_eq!(landing.rows, vec![TRACK_4.to_owned()]);
        assert_eq!(landing.index, 0);
    }

    #[test]
    fn a_queue_summary_counts_and_never_names() {
        let summary = summarise_queue(&SetQueueCommand {
            next_tracks: vec![provided(TRACK_1), provided(TRACK_2)],
            prev_tracks: vec![provided(TRACK_3)],
            queue_revision: "rev-7".to_owned(),
            logging_params: logging(),
        });
        let shown = summary.to_string();

        assert_eq!(summary.prev, 1);
        assert_eq!(summary.next, 2);
        assert!(shown.contains("prev 1 rows"), "{shown}");
        assert!(shown.contains("next 2 rows"), "{shown}");
        assert!(!shown.contains("spotify:"), "{shown}");
    }

    #[test]
    fn a_play_summary_names_presence_only() {
        let summary = summarise_play(&PlayCommand {
            context: Context {
                uri: Some(PLAYLIST.to_owned()),
                pages: vec![page(&[TRACK_1])],
                ..Default::default()
            },
            play_origin: PlayOrigin::default(),
            options: PlayOptions {
                seek_to: Some(42_000),
                ..skip_to(Some(TRACK_1), Some(0), Some("uid-1"))
            },
            logging_params: logging(),
        });
        let shown = summary.to_string();

        assert_eq!(summary.context, ContextKind::Playlist);
        assert_eq!((summary.pages, summary.tracks), (1, 1));
        assert!(summary.uri && summary.index && summary.uid);
        assert!(summary.seek && !summary.paused);
        assert!(shown.contains("skip uri named"), "{shown}");
        assert!(shown.contains("paused absent"), "{shown}");
        assert!(!shown.contains("spotify:"), "{shown}");
    }

    #[test]
    fn a_summary_counts_and_never_names() {
        let state = TransferState {
            current_session: MessageField::some(Session {
                context: MessageField::some(Context {
                    uri: Some("spotify:playlist:37i9dQZF1DXcBWIGoYBM5M".to_owned()),
                    pages: vec![
                        page(&["spotify:track:1", "spotify:track:2"]),
                        page(&["spotify:track:3"]),
                    ],
                    ..Default::default()
                }),
                ..Default::default()
            }),
            playback: MessageField::some(Playback {
                timestamp: Some(1_000),
                is_paused: Some(true),
                ..Default::default()
            }),
            queue: MessageField::some(Queue {
                tracks: vec![track("spotify:track:4")],
                is_playing_queue: Some(false),
                ..Default::default()
            }),
            ..Default::default()
        };

        let summary = summarise(&state, 2_500);
        let shown = summary.to_string();

        assert_eq!(summary.context, ContextKind::Playlist);
        assert_eq!(summary.pages, 2);
        assert_eq!(summary.tracks, 3);
        assert_eq!(summary.queue, 1);
        assert!(summary.paused);
        assert!(shown.contains("playlist"), "{shown}");
        assert!(shown.contains("pages 2"), "{shown}");
        assert!(shown.contains("tracks 3"), "{shown}");
        assert!(shown.contains("queue 1"), "{shown}");
        assert!(!shown.contains("spotify:"), "{shown}");
    }

    #[test]
    fn the_summary_reports_skew() {
        let state = TransferState {
            playback: MessageField::some(Playback {
                timestamp: Some(10_000),
                ..Default::default()
            }),
            ..Default::default()
        };

        assert_eq!(summarise(&state, 13_750).skew_ms, Some(3_750));
        assert_eq!(summarise(&state, 8_000).skew_ms, Some(-2_000));
        assert_eq!(summarise(&TransferState::default(), 13_750).skew_ms, None);
    }
}
