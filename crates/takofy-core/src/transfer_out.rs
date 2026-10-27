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
mod tests;
