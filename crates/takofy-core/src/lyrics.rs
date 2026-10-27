//! A track's lyrics, as the lyrics pane draws them.
//!
//! Pure: a parsed [`Lyrics`] in, a [`TrackLyrics`] out. The request itself is in
//! `remote::read_lyrics`.
//!
//! A line keeps the provider's order and is never sorted: an untimed line sits
//! where it was sent, so a lyric that is half-timed still reads as written.

use librespot::core::Error;
use librespot::core::error::ErrorKind;
use librespot::metadata::Lyrics;
use librespot::metadata::lyrics::SyncType;
use takofy_model::{LyricLine, TrackLyrics};

/// Why a lyrics read failed.
#[derive(Debug, thiserror::Error)]
pub enum LyricsError {
    /// The URI does not name a track, so there is nothing to ask for.
    #[error("not a track URI")]
    NotATrack,
    /// The request failed; the message is the source's.
    #[error("{0}")]
    Source(String),
}

/// A line time as the wire sends it: trimmed, decimal, and unsigned.
///
/// Anything that is not a plain count — empty, signed, non-numeric, or wider
/// than a `u32` — is no time at all rather than a guessed zero.
pub(crate) fn parse_time(raw: &str) -> Option<u32> {
    let raw = raw.trim();
    if raw.is_empty() || !raw.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    raw.parse().ok()
}

/// The pane's lyrics, from the provider's payload.
///
/// A line is only timed when both of its times parse; the `None` it keeps is
/// what stops it from ever becoming the current line.
pub(crate) fn track_lyrics(lyrics: &Lyrics) -> TrackLyrics {
    TrackLyrics {
        synced: matches!(lyrics.lyrics.sync_type, SyncType::LineSynced),
        lines: lyrics
            .lyrics
            .lines
            .iter()
            .map(|line| LyricLine {
                start_ms: parse_time(&line.start_time_ms),
                end_ms: parse_time(&line.end_time_ms),
                words: line.words.clone(),
            })
            .collect(),
    }
}

/// The lyrics request's answer, with "this track has none" as an empty success.
///
/// A track without lyrics answers 404, which librespot maps to `NotFound`; the
/// pane then says so instead of offering a retry. A payload this parser cannot
/// read at all is a different thing — it becomes an error, because a lyric that
/// exists but failed to parse is worth a Retry and one that does not exist is
/// not.
pub(crate) fn settled(result: Result<Lyrics, Error>) -> Result<TrackLyrics, LyricsError> {
    match result {
        Ok(lyrics) => Ok(track_lyrics(&lyrics)),
        Err(error) if error.kind == ErrorKind::NotFound => Ok(TrackLyrics::default()),
        Err(error) => Err(LyricsError::Source(error.to_string())),
    }
}

#[cfg(test)]
mod tests;
