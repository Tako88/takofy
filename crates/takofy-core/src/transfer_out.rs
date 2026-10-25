//! The command that moves the sound to another device.
//!
//! The REST transfer endpoint only lets a device claim the sound for itself, so
//! a push to another device is refused there. What works is the dealer command
//! the phone itself sends this window: an `endpoint: transfer` request whose
//! data is the whole state to carry. The target then claims the sound with that
//! state instead of resolving the context on its own — which is why the state
//! carries the rows, not only the context's uri.

use std::sync::atomic::{AtomicU64, Ordering};

use librespot::protocol::context::Context;
use librespot::protocol::context_page::ContextPage;
use librespot::protocol::context_player_options::ContextPlayerOptions;
use librespot::protocol::context_track::ContextTrack;
use librespot::protocol::playback::Playback;
use librespot::protocol::session::Session as WireSession;
use librespot::protocol::transfer_state::TransferState;
use protobuf::{Message as ProtobufMessage, MessageField};
use takofy_model::{Report, Status, TrackEntry};

/// Why a state could not be built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveError {
    /// Nothing is loaded, so there is no row to carry.
    NoTrack,
}

/// The dealer wants a number per request; the account only echoes it back.
fn next_message_id() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// The body that asks `to` to take the sound, carrying this window's state.
///
/// One row per entry, in queue order, with a uid per row: the target matches its
/// current row by that uid, so the page and `current_uid` have to agree and
/// nothing else reads them.
pub(crate) fn body(report: &Report, own: &str, now_ms: i64) -> Result<String, MoveError> {
    let track = report.track.as_ref().ok_or(MoveError::NoTrack)?;
    let state = state_of(report, track, now_ms);
    let encoded = encode(&state.write_to_bytes().unwrap_or_default());
    Ok(format!(
        "{{\"message_id\":{},\"sent_by_device_id\":{},\"command\":{{\"endpoint\":\"transfer\",\"data\":\"{}\"}}}}",
        next_message_id(),
        json_string(own),
        encoded
    ))
}

fn state_of(report: &Report, track: &TrackEntry, now_ms: i64) -> TransferState {
    let options = report.options.unwrap_or_default();
    let mut rows: Vec<&TrackEntry> = report.prev.iter().collect();
    let current = rows.len();
    rows.push(track);
    rows.extend(report.next.iter());

    let tracks: Vec<ContextTrack> = rows
        .iter()
        .enumerate()
        .map(|(index, row)| ContextTrack {
            uri: Some(row.uri.clone()),
            uid: Some(uid_of(index)),
            ..Default::default()
        })
        .collect();
    let mut context = Context {
        uri: Some(report.context.clone().unwrap_or_default()),
        ..Default::default()
    };
    context.pages.push(ContextPage {
        tracks,
        ..Default::default()
    });
    let playing = matches!(report.status, Status::Playing | Status::Loading);

    TransferState {
        options: MessageField::some(ContextPlayerOptions {
            shuffling_context: Some(options.shuffle),
            repeating_context: Some(options.repeat_context),
            repeating_track: Some(options.repeat_track),
            playback_speed: Some(1.0),
            ..Default::default()
        }),
        playback: MessageField::some(Playback {
            timestamp: Some(now_ms),
            position_as_of_timestamp: Some(report.position_ms as i32),
            playback_speed: Some(1.0),
            is_paused: Some(!playing),
            current_track: MessageField::some(ContextTrack {
                uri: Some(track.uri.clone()),
                uid: Some(uid_of(current)),
                ..Default::default()
            }),
            ..Default::default()
        }),
        current_session: MessageField::some(WireSession {
            context: MessageField::some(context),
            current_uid: Some(uid_of(current)),
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn uid_of(index: usize) -> String {
    format!("row{index}")
}

/// A JSON string literal. The two characters that can appear in a device id are
/// escaped, so a hostile id cannot close the field it sits in.
fn json_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            character if character.is_control() => out.push(' '),
            character => out.push(character),
        }
    }
    out.push('"');
    out
}

/// Standard base64, because the dealer's transfer data is one string.
fn encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let second = chunk.get(1).copied();
        let third = chunk.get(2).copied();
        let bits = (u32::from(chunk[0]) << 16)
            | (u32::from(second.unwrap_or(0)) << 8)
            | u32::from(third.unwrap_or(0));
        out.push(ALPHABET[(bits >> 18) as usize & 63] as char);
        out.push(ALPHABET[(bits >> 12) as usize & 63] as char);
        out.push(match second {
            Some(_) => ALPHABET[(bits >> 6) as usize & 63] as char,
            None => '=',
        });
        out.push(match third {
            Some(_) => ALPHABET[bits as usize & 63] as char,
            None => '=',
        });
    }
    out
}

/// The inverse, for tests: nothing in production decodes this.
#[cfg(test)]
fn decode(text: &str) -> Vec<u8> {
    const ALPHABET: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::new();
    let cleaned: Vec<u8> = text.bytes().filter(|byte| *byte != b'=').collect();
    for chunk in cleaned.chunks(4) {
        let mut bits = 0u32;
        for (index, byte) in chunk.iter().enumerate() {
            let value = ALPHABET
                .bytes()
                .position(|candidate| candidate == *byte)
                .unwrap_or(0) as u32;
            bits |= value << (18 - 6 * index);
        }
        out.push((bits >> 16) as u8);
        if chunk.len() > 2 {
            out.push((bits >> 8) as u8);
        }
        if chunk.len() > 3 {
            out.push(bits as u8);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use takofy_model::Options;

    fn row(uri: &str) -> TrackEntry {
        TrackEntry {
            uri: uri.to_owned(),
            title: uri.to_owned(),
            artists: String::new(),
            album: None,
            album_uri: None,
            artist_uris: Vec::new(),
            duration_ms: None,
            cover: None,
            artist_names: Vec::new(),
            added_at: None,
            named: true,
        }
    }

    fn report() -> Report {
        Report {
            status: Status::Playing,
            track: Some(row("spotify:track:b")),
            prev: vec![row("spotify:track:a")],
            next: vec![row("spotify:track:c"), row("spotify:track:d")],
            context: Some("spotify:playlist:ours".to_owned()),
            queue_revision: String::new(),
            options: Some(Options {
                shuffle: true,
                repeat_context: false,
                repeat_track: true,
            }),
            position_ms: 4_200,
            duration_ms: Some(200_000),
        }
    }

    fn state(body: &str) -> TransferState {
        let data = body
            .split("\"data\":\"")
            .nth(1)
            .and_then(|rest| rest.split('"').next())
            .expect("the body carries data");
        TransferState::parse_from_bytes(&decode(data)).expect("the data is a transfer state")
    }

    #[test]
    fn the_body_asks_for_a_transfer_and_names_us() {
        let body = body(&report(), "our-device", 1_000).expect("a report with a row");
        assert!(body.starts_with("{\"message_id\":"), "{body}");
        assert!(
            body.contains("\"sent_by_device_id\":\"our-device\""),
            "{body}"
        );
        assert!(body.contains("\"endpoint\":\"transfer\""), "{body}");
        assert!(!body.contains('\n'), "a body is one line");
    }

    #[test]
    fn the_state_carries_every_row_around_the_current_one() {
        let state = state(&body(&report(), "own", 1_000).expect("a body"));
        let context = state.current_session.context.as_ref().expect("a context");
        assert_eq!(context.uri.as_deref(), Some("spotify:playlist:ours"));
        let tracks = &context.pages.first().expect("a page").tracks;
        let uris: Vec<&str> = tracks
            .iter()
            .filter_map(|track| track.uri.as_deref())
            .collect();
        assert_eq!(
            uris,
            vec![
                "spotify:track:a",
                "spotify:track:b",
                "spotify:track:c",
                "spotify:track:d"
            ]
        );

        // The uid the session names is the one the current row carries.
        let current_uid = state.current_session.current_uid.as_deref();
        assert_eq!(current_uid, Some("row1"));
        assert_eq!(
            tracks
                .iter()
                .find(|track| track.uid.as_deref() == current_uid)
                .and_then(|track| track.uri.as_deref()),
            Some("spotify:track:b")
        );
        assert_eq!(
            state.playback.current_track.uri.as_deref(),
            Some("spotify:track:b")
        );
    }

    #[test]
    fn the_state_carries_the_position_the_modes_and_whether_it_plays() {
        let state = state(&body(&report(), "own", 1_000).expect("a body"));
        let playback = state.playback.as_ref().expect("a playback");
        assert_eq!(playback.position_as_of_timestamp, Some(4_200));
        assert_eq!(playback.timestamp, Some(1_000));
        assert_eq!(playback.is_paused, Some(false));

        let options = state.options.as_ref().expect("options");
        assert_eq!(options.shuffling_context, Some(true));
        assert_eq!(options.repeating_context, Some(false));
        assert_eq!(options.repeating_track, Some(true));
    }

    #[test]
    fn a_paused_window_moves_a_paused_state() {
        let mut paused = report();
        paused.status = Status::Paused;
        let state = state(&body(&paused, "own", 1_000).expect("a body"));
        assert_eq!(
            state
                .playback
                .as_ref()
                .and_then(|playback| playback.is_paused),
            Some(true)
        );
    }

    #[test]
    fn a_report_with_no_row_cannot_move_anything() {
        let mut empty = report();
        empty.track = None;
        assert_eq!(body(&empty, "own", 1_000), Err(MoveError::NoTrack));
    }

    #[test]
    fn a_hostile_device_id_stays_inside_its_field() {
        let body = body(&report(), "ow\"n\\x", 1_000).expect("a body");
        assert!(
            body.contains("\"sent_by_device_id\":\"ow\\\"n\\\\x\""),
            "{body}"
        );
    }

    #[test]
    fn the_encoding_round_trips_and_matches_the_reference_vectors() {
        assert_eq!(encode(b""), "");
        assert_eq!(encode(b"f"), "Zg==");
        assert_eq!(encode(b"fo"), "Zm8=");
        assert_eq!(encode(b"foo"), "Zm9v");
        assert_eq!(encode(b"foob"), "Zm9vYg==");
        assert_eq!(encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(encode(b"foobar"), "Zm9vYmFy");
        for length in 0..40 {
            let bytes: Vec<u8> = (0..length).map(|value| (value * 37 % 256) as u8).collect();
            assert_eq!(decode(&encode(&bytes)), bytes);
        }
    }
}
