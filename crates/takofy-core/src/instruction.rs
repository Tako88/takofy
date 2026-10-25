//! What one incoming command asked for, in words this client can log.
//!
//! A command's payload can carry track uris and context uris, and the log is
//! read by whoever is watching the account. So the only shape that reaches the
//! log is a summary of counts and kinds, never a uri: a decoded plan carries
//! uris because the queue has to adopt them, and it is never printed. It also
//! decodes the endpoint, so the rest of the crate asks one question instead of
//! matching on the wire's own enum.

use std::fmt;

use librespot::core::dealer::protocol::{Command, Request};
use librespot::protocol::transfer_state::TransferState;
use takofy_model::Order;

use crate::transfer::plan;
pub use crate::transfer::{Refusal, TransferPlan};

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
        )
    }

    fn word(self) -> &'static str {
        match self {
            ContextKind::Playlist => "playlist",
            ContextKind::Album => "album",
            ContextKind::Show => "show",
            ContextKind::Collection => "collection",
            ContextKind::Artist => "artist",
            ContextKind::Station => "station",
            ContextKind::Search => "search",
            ContextKind::Local => "local",
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
            if self.current_track {
                "named"
            } else {
                "absent"
            },
            if self.current_uid { "named" } else { "absent" },
        )?;
        match self.skew_ms {
            Some(skew) => write!(f, "skew {skew} ms"),
            None => write!(f, "skew none"),
        }
    }
}

/// What one incoming request asked this window to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Instruction {
    /// A hand-over this window can take, decoded into what the queue adopts.
    Transfer(TransferPlan),
    /// A hand-over it cannot take, and why, so nothing is put.
    Refused(Refusal),
    /// One transport action the window carries out and answers.
    Order(Order),
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
        other => Instruction::Other(kind_of(other)),
    }
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

#[cfg(test)]
mod tests {
    use librespot::core::dealer::protocol::{
        AddToQueueCommand, Command, GenericCommand, LoggingParams, PauseCommand, PlayCommand,
        PlayOptions, SeekToCommand, SetOptionsCommand, SetQueueCommand, SetValueCommand,
        SkipNextCommand, TransferCommand, TransferOptions, UpdateContextCommand,
    };
    use librespot::protocol::context::Context;
    use librespot::protocol::context_page::ContextPage;
    use librespot::protocol::context_track::ContextTrack;
    use librespot::protocol::playback::Playback;
    use librespot::protocol::player::{PlayOrigin, ProvidedTrack};
    use librespot::protocol::queue::Queue;
    use librespot::protocol::session::Session;
    use protobuf::MessageField;

    use super::*;

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
    fn only_playlist_album_show_and_collection_are_honoured() {
        let cases = [
            (ContextKind::Playlist, true),
            (ContextKind::Album, true),
            (ContextKind::Show, true),
            (ContextKind::Collection, true),
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
    const PLAYLIST: &str = "spotify:playlist:37i9dQZF1DXcBWIGoYBM5M";
    const ARTIST: &str = "spotify:artist:0OdUWJ0sBjDrqHygGUXeCF";

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
    fn other_endpoints_keep_their_kind() {
        for (command, expected) in commands() {
            if expected == Kind::Transfer || is_order(expected) {
                continue;
            }
            assert_eq!(
                instruction_of(&request(command)),
                Instruction::Other(expected),
                "{expected:?}"
            );
        }

        let payload = serde_json::json!({"endpoint": "something_new"});
        assert_eq!(
            instruction_of(&request(Command::Unknown(payload))),
            Instruction::Other(Kind::Unknown)
        );
    }

    /// The endpoints the window answers as orders.
    fn is_order(kind: Kind) -> bool {
        matches!(
            kind,
            Kind::Pause | Kind::Resume | Kind::SeekTo | Kind::SkipNext | Kind::SkipPrev
        )
    }

    #[test]
    fn each_transport_endpoint_decodes_to_its_order() {
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
                    track: Some(ProvidedTrack {
                        uri: TRACK_1.to_owned(),
                        ..Default::default()
                    }),
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
        ];

        for (command, expected) in table {
            assert_eq!(
                instruction_of(&request(command)),
                Instruction::Order(expected)
            );
        }
    }

    #[test]
    fn other_endpoints_are_not_orders() {
        let outside = [
            Kind::Play,
            Kind::SetOptions,
            Kind::SetQueue,
            Kind::AddToQueue,
            Kind::UpdateContext,
        ];
        let mut seen = Vec::new();
        for (command, kind) in commands() {
            if !outside.contains(&kind) {
                continue;
            }
            assert_eq!(
                instruction_of(&request(command)),
                Instruction::Other(kind),
                "{kind:?} is not an order"
            );
            seen.push(kind);
        }
        assert_eq!(seen, outside.to_vec(), "every endpoint outside the orders");

        let payload = serde_json::json!({"endpoint": "something_new"});
        assert_eq!(
            instruction_of(&request(Command::Unknown(payload))),
            Instruction::Other(Kind::Unknown)
        );
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
