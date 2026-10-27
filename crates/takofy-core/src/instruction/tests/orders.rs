//! A phone's controls decode into the order the queue follows.

use librespot::core::dealer::protocol::{
    AddToQueueCommand, Command, GenericCommand, PauseCommand, SeekToCommand, SetOptionsCommand,
    SetQueueCommand, SetValueCommand, SkipNextCommand, UpdateContextCommand,
};
use librespot::protocol::context::Context;
use librespot::protocol::player::ProvidedTrack;

use super::super::*;
use super::test_support::*;

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
