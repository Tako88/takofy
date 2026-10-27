use librespot::protocol::connect::Cluster;
use librespot::protocol::player::{PlayerState, ProvidedTrack};
use protobuf::MessageField;

pub(super) fn player(uri: &str, playing: bool, paused: bool, buffering: bool) -> PlayerState {
    PlayerState {
        track: MessageField::some(ProvidedTrack {
            uri: uri.to_owned(),
            ..Default::default()
        }),
        is_playing: playing,
        is_paused: paused,
        is_buffering: buffering,
        ..Default::default()
    }
}

pub(super) fn cluster_with_player(player: PlayerState) -> Cluster {
    Cluster {
        player_state: MessageField::some(player),
        ..Default::default()
    }
}
