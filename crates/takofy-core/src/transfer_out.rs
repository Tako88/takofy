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

/// The most rows a move carries.
///
/// A playlist can hold thousands and the body is one POST; the target only needs
/// the rows around the current one, and it resolves the rest from the context.
pub(crate) const MOVE_MAX_ROWS: usize = 400;

/// One row of the resolved context: the track, and the account's own name for
/// that position in it.
///
/// The uid matters: the target matches its current row by uid against the
/// context it holds, so a uid this client invented names no row there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ContextRow {
    pub uri: String,
    pub uid: String,
}

/// Why a state could not be built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveError {
    /// Nothing is loaded, so there is no row to carry.
    NoTrack,
    /// The rows came from no page, so the target would be told to play nothing.
    NoContext,
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
pub(crate) fn body(
    report: &Report,
    own: &str,
    now_ms: i64,
    resolved: &[ContextRow],
) -> Result<String, MoveError> {
    let track = report.track.as_ref().ok_or(MoveError::NoTrack)?;
    // A target claims the sound with this state, so a state with no context asks
    // it to claim a sound it has nothing to play.
    let context = report
        .context
        .as_deref()
        .filter(|uri| !uri.is_empty())
        .ok_or(MoveError::NoContext)?;
    let state = state_of(report, track, context, now_ms, resolved);
    let encoded = encode(&state.write_to_bytes().unwrap_or_default());
    Ok(format!(
        "{{\"message_id\":{},\"sent_by_device_id\":{},\"command\":{{\"endpoint\":\"transfer\",\"data\":\"{}\"}}}}",
        next_message_id(),
        json_string(own),
        encoded
    ))
}

fn state_of(
    report: &Report,
    track: &TrackEntry,
    context_uri: &str,
    now_ms: i64,
    resolved: &[ContextRow],
) -> TransferState {
    let options = report.options.unwrap_or_default();
    let (rows, current) = match window(resolved, &track.uri) {
        Some((rows, current)) => (rows, current),
        // The context does not name the row — an edited queue, a shuffle this
        // client made. The near rows still give the target something to play,
        // though it will land at the context's own idea of the position.
        None => {
            let mut rows: Vec<ContextRow> = report
                .prev
                .iter()
                .chain(std::iter::once(track))
                .chain(report.next.iter())
                .enumerate()
                .map(|(index, row)| ContextRow {
                    uri: row.uri.clone(),
                    uid: uid_of(index),
                })
                .collect();
            rows.truncate(MOVE_MAX_ROWS);
            let current = report.prev.len().min(rows.len().saturating_sub(1));
            (rows, current)
        }
    };

    let tracks: Vec<ContextTrack> = rows
        .iter()
        .map(|row| ContextTrack {
            uri: Some(row.uri.clone()),
            uid: Some(row.uid.clone()),
            ..Default::default()
        })
        .collect();
    let current_track = tracks.get(current).cloned().unwrap_or(ContextTrack {
        uri: Some(track.uri.clone()),
        uid: Some(uid_of(current)),
        ..Default::default()
    });
    let mut context = Context {
        uri: Some(context_uri.to_owned()),
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
            current_track: MessageField::some(current_track),
            ..Default::default()
        }),
        current_session: MessageField::some(WireSession {
            context: MessageField::some(context),
            current_uid: rows.get(current).map(|row| row.uid.clone()),
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn uid_of(index: usize) -> String {
    format!("row{index}")
}

/// The rows to carry, and where the current one sits among them.
///
/// The window keeps the body bounded while still naming the current row; when
/// the context is short the window is the whole of it.
fn window(resolved: &[ContextRow], current_uri: &str) -> Option<(Vec<ContextRow>, usize)> {
    let at = resolved.iter().position(|row| row.uri == current_uri)?;
    if resolved.len() <= MOVE_MAX_ROWS {
        return Some((resolved.to_vec(), at));
    }
    let half = MOVE_MAX_ROWS / 2;
    let start = at.saturating_sub(half).min(resolved.len() - MOVE_MAX_ROWS);
    let rows = resolved[start..start + MOVE_MAX_ROWS].to_vec();
    Some((rows, at - start))
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

/// The state a body carries, for tests in other modules: nothing in production
/// reads a body back.
#[cfg(test)]
pub(crate) fn parse_body(body: &str) -> Option<TransferState> {
    let data = body
        .split("\"data\":\"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())?;
    TransferState::parse_from_bytes(&decode(data)).ok()
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
        let body = body(&report(), "our-device", 1_000, &[]).expect("a report with a row");
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
        let state = state(&body(&report(), "own", 1_000, &[]).expect("a body"));
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
        let state = state(&body(&report(), "own", 1_000, &[]).expect("a body"));
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
        let state = state(&body(&paused, "own", 1_000, &[]).expect("a body"));
        assert_eq!(
            state
                .playback
                .as_ref()
                .and_then(|playback| playback.is_paused),
            Some(true)
        );
    }

    fn resolved(uris: &[(&str, &str)]) -> Vec<ContextRow> {
        uris.iter()
            .map(|(uri, uid)| ContextRow {
                uri: (*uri).to_owned(),
                uid: (*uid).to_owned(),
            })
            .collect()
    }

    #[test]
    fn the_page_comes_from_the_resolved_context_and_names_the_row() {
        let rows = resolved(&[
            ("spotify:track:a", "server-a"),
            ("spotify:track:b", "server-b"),
            ("spotify:track:c", "server-c"),
        ]);
        let state = state(&body(&report(), "own", 1_000, &rows).expect("a report with a row"));
        let tracks = &state
            .current_session
            .context
            .as_ref()
            .expect("a context")
            .pages
            .first()
            .expect("a page")
            .tracks;
        assert_eq!(tracks.len(), 3, "the context's own order, not a window");
        assert_eq!(
            state.current_session.current_uid.as_deref(),
            Some("server-b"),
            "the row the sound is on, named the way the account names it"
        );
        assert_eq!(
            state.playback.current_track.uri.as_deref(),
            Some("spotify:track:b")
        );
        assert_eq!(
            state.playback.current_track.uid.as_deref(),
            Some("server-b")
        );
    }

    #[test]
    fn a_context_that_does_not_name_the_row_moves_the_near_rows() {
        let rows = resolved(&[
            ("spotify:track:a", "server-a"),
            ("spotify:track:d", "server-d"),
        ]);
        // The report is on a track this context does not hold.
        let state = state(&body(&report(), "own", 1_000, &rows).expect("a body"));
        let tracks = &state
            .current_session
            .context
            .as_ref()
            .expect("a context")
            .pages
            .first()
            .expect("a page")
            .tracks;
        assert_eq!(tracks.len(), 4, "the rows around the current one");
        assert_eq!(
            state.playback.current_track.uri.as_deref(),
            Some("spotify:track:b"),
            "the current row is still carried"
        );
    }

    #[test]
    fn a_long_context_is_windowed_around_the_row() {
        let mut uris: Vec<(String, String)> = (0..1_000)
            .map(|index| (format!("spotify:track:{index}"), format!("server-{index}")))
            .collect();
        let rows: Vec<ContextRow> = uris
            .drain(..)
            .map(|(uri, uid)| ContextRow { uri, uid })
            .collect();
        let mut long = report();
        long.track = Some(row("spotify:track:900"));
        let state = state(&body(&long, "own", 1_000, &rows).expect("a body"));
        let page = state
            .current_session
            .context
            .as_ref()
            .expect("a context")
            .pages
            .first()
            .expect("a page");
        assert_eq!(page.tracks.len(), MOVE_MAX_ROWS);
        assert_eq!(
            state.current_session.current_uid.as_deref(),
            Some("server-900")
        );
        assert!(
            page.tracks
                .iter()
                .any(|track| track.uid.as_deref() == Some("server-900")),
            "the current row is inside the window"
        );
    }

    #[test]
    fn a_report_with_no_row_cannot_move_anything() {
        let mut empty = report();
        empty.track = None;
        assert_eq!(body(&empty, "own", 1_000, &[]), Err(MoveError::NoTrack));
    }

    #[test]
    fn a_report_with_no_context_cannot_move_anything() {
        let mut empty = report();
        empty.context = None;
        assert_eq!(body(&empty, "own", 1_000, &[]), Err(MoveError::NoContext));
    }

    #[test]
    fn a_report_with_an_empty_context_cannot_move_anything() {
        let mut empty = report();
        empty.context = Some(String::new());
        assert_eq!(body(&empty, "own", 1_000, &[]), Err(MoveError::NoContext));
    }

    #[test]
    fn a_hostile_device_id_stays_inside_its_field() {
        let body = body(&report(), "ow\"n\\x", 1_000, &[]).expect("a body");
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
