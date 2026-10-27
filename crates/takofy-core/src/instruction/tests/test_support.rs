//! Fixtures the instruction tests share: the wire's own command shapes, the
//! contexts and rows they name, and what a plan lands on.

use librespot::core::dealer::protocol::{
    AddToQueueCommand, Command, GenericCommand, LoggingParams, PauseCommand, PlayCommand,
    PlayOptions, Request, SeekToCommand, SetOptionsCommand, SetQueueCommand, SetValueCommand,
    SkipNextCommand, SkipTo, TransferCommand, TransferOptions, UpdateContextCommand,
};
use librespot::protocol::context::Context;
use librespot::protocol::context_page::ContextPage;
use librespot::protocol::context_track::ContextTrack;
use librespot::protocol::player::{PlayOrigin, ProvidedTrack};
use takofy_model::Landing;

use super::super::*;

pub(super) fn logging() -> LoggingParams {
    LoggingParams {
        interaction_ids: None,
        device_identifier: None,
        command_initiated_time: None,
        page_instance_ids: None,
        command_id: None,
    }
}

pub(super) fn play_options() -> PlayOptions {
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

pub(super) fn track(uri: &str) -> ContextTrack {
    ContextTrack {
        uri: Some(uri.to_owned()),
        ..Default::default()
    }
}

pub(super) fn page(uris: &[&str]) -> ContextPage {
    ContextPage {
        tracks: uris.iter().map(|uri| track(uri)).collect(),
        ..Default::default()
    }
}

pub(super) fn commands() -> Vec<(Command, Kind)> {
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

/// The wire's ids are 22 base62 characters, so a fixture has to be one.
pub(super) const TRACK_1: &str = "spotify:track:4uLU6hMCjMI75M1A2tKUQ1";
pub(super) const TRACK_2: &str = "spotify:track:4uLU6hMCjMI75M1A2tKUQ2";
pub(super) const TRACK_3: &str = "spotify:track:4uLU6hMCjMI75M1A2tKUQ3";
pub(super) const TRACK_4: &str = "spotify:track:4uLU6hMCjMI75M1A2tKUQ4";
pub(super) const PLAYLIST: &str = "spotify:playlist:37i9dQZF1DXcBWIGoYBM5M";
pub(super) const ARTIST: &str = "spotify:artist:0OdUWJ0sBjDrqHygGUXeCF";
pub(super) const LOCAL: &str = "spotify:local:artist:album:title:200";

pub(super) fn request(command: Command) -> Request {
    Request {
        message_id: 1,
        sent_by_device_id: "phone".to_owned(),
        command,
    }
}

/// One row the phone sends as a queue entry, with the uid and metadata that
/// name nothing here.
pub(super) fn provided(uri: &str) -> ProvidedTrack {
    ProvidedTrack {
        uri: uri.to_owned(),
        uid: "a uid".to_owned(),
        metadata: std::collections::HashMap::from([("title".to_owned(), "a title".to_owned())]),
        ..Default::default()
    }
}

/// A play naming `context`, carrying `pages`.
pub(super) fn play(
    context: Option<&str>,
    pages: Vec<ContextPage>,
    options: PlayOptions,
) -> Command {
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

pub(super) fn play_plan(command: Command) -> TransferPlan {
    match instruction_of(&request(command)) {
        Instruction::Play(plan) => plan,
        other => panic!("a playable play must decode into a plan: {other:?}"),
    }
}

pub(super) fn skip_to(uri: Option<&str>, index: Option<u32>, uid: Option<&str>) -> PlayOptions {
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
pub(super) fn landed(plan: &TransferPlan, rows: &[&str]) -> Landing {
    let rows: Vec<String> = rows.iter().map(|row| (*row).to_owned()).collect();
    crate::transfer::land(plan, crate::transfer::Resolution::Resolved(rows), 0)
}
