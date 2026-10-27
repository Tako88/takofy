//! A phone command decodes into the instruction it names, and a transfer into
//! the plan its own state supports.

use librespot::core::dealer::protocol::{
    AddToQueueCommand, Command, GenericCommand, PauseCommand, SeekToCommand, SetOptionsCommand,
    SetQueueCommand, SetValueCommand, SkipNextCommand, TransferCommand, TransferOptions,
    UpdateContextCommand,
};
use librespot::protocol::context::Context;
use librespot::protocol::playback::Playback;
use librespot::protocol::session::Session;
use librespot::protocol::transfer_state::TransferState;
use protobuf::MessageField;

use super::super::*;
use super::test_support::*;

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
