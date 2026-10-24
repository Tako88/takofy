//! What one incoming command asked for, in words this client can log.
//!
//! A command's payload can carry track uris and context uris, and the log is
//! read by whoever is watching the account. So the only shape that leaves this
//! module is a summary of counts and kinds, never a uri. It also decodes the
//! endpoint, so the rest of the crate asks one question instead of matching on
//! the wire's own enum.

use std::fmt;

use librespot::core::dealer::protocol::Command;
use librespot::protocol::transfer_state::TransferState;

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
        match self.skew_ms {
            Some(skew) => write!(f, "skew {skew} ms"),
            None => write!(f, "skew none"),
        }
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
