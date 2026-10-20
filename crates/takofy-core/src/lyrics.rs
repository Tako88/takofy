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
mod tests {
    use librespot::core::Error;
    use librespot::metadata::Lyrics;
    use librespot::metadata::lyrics::{Colors, Line, LyricsInner, SyncType};
    use takofy_model::{LyricLine, TrackLyrics};

    use super::{LyricsError, parse_time, settled, track_lyrics};

    /// A provider payload with the given sync type and `(start, end, words)` lines.
    fn lyrics_of(sync_type: SyncType, lines: &[(&str, &str, &str)]) -> Lyrics {
        Lyrics {
            colors: Colors {
                background: 0,
                highlight_text: 1,
                text: 2,
            },
            has_vocal_removal: false,
            lyrics: LyricsInner {
                is_dense_typeface: false,
                is_rtl_language: false,
                language: "en".to_owned(),
                lines: lines
                    .iter()
                    .map(|(start, end, words)| Line {
                        start_time_ms: (*start).to_owned(),
                        end_time_ms: (*end).to_owned(),
                        words: (*words).to_owned(),
                    })
                    .collect(),
                provider: "provider".to_owned(),
                provider_display_name: "Provider".to_owned(),
                provider_lyrics_id: "id".to_owned(),
                sync_lyrics_uri: "spotify:lyrics:1".to_owned(),
                sync_type,
            },
        }
    }

    /// A line the pane can compare without spelling out every field.
    fn line(start: Option<u32>, end: Option<u32>, words: &str) -> LyricLine {
        LyricLine {
            start_ms: start,
            end_ms: end,
            words: words.to_owned(),
        }
    }

    #[test]
    fn parse_time_reads_a_plain_millisecond_count() {
        assert_eq!(parse_time("0"), Some(0));
        assert_eq!(parse_time("12340"), Some(12_340));
        assert_eq!(parse_time("4294967295"), Some(u32::MAX));
    }

    #[test]
    fn parse_time_trims_surrounding_whitespace() {
        assert_eq!(parse_time("  12340\n"), Some(12_340));
        assert_eq!(parse_time("\t0 "), Some(0));
    }

    #[test]
    fn parse_time_rejects_empty_signed_and_non_numeric() {
        for raw in [
            "", "   ", "+12340", "-12340", "12.5", "1.2e4", "12ms", "abc",
        ] {
            assert_eq!(parse_time(raw), None, "must reject {raw:?}");
        }
    }

    #[test]
    fn parse_time_rejects_an_overflowing_count() {
        assert_eq!(parse_time("4294967296"), None);
        assert_eq!(parse_time("99999999999999999999"), None);
    }

    #[test]
    fn a_line_synced_lyric_is_synced_and_keeps_every_time() {
        let lyrics = lyrics_of(
            SyncType::LineSynced,
            &[("0", "1000", "first"), ("1000", "2000", "second")],
        );
        let parsed = track_lyrics(&lyrics);
        assert!(parsed.synced);
        assert_eq!(
            parsed.lines,
            vec![
                line(Some(0), Some(1000), "first"),
                line(Some(1000), Some(2000), "second"),
            ]
        );
    }

    #[test]
    fn an_unsynced_lyric_is_not_synced_and_carries_no_times() {
        let lyrics = lyrics_of(SyncType::Unsynced, &[("", "", "first"), ("", "", "second")]);
        let parsed = track_lyrics(&lyrics);
        assert!(!parsed.synced);
        assert_eq!(
            parsed.lines,
            vec![line(None, None, "first"), line(None, None, "second")]
        );
    }

    #[test]
    fn a_line_whose_times_do_not_parse_keeps_none_and_stays_in_wire_order() {
        let lyrics = lyrics_of(
            SyncType::LineSynced,
            &[
                ("bad", "1000", "first"),
                ("1000", "2000", "second"),
                ("-5", "", "third"),
            ],
        );
        let parsed = track_lyrics(&lyrics);
        assert_eq!(
            parsed.lines,
            vec![
                // Each time parses on its own: a bad start does not discard a good end.
                line(None, Some(1000), "first"),
                line(Some(1000), Some(2000), "second"),
                line(None, None, "third"),
            ],
            "an untimed line is not sorted away"
        );
    }

    #[test]
    fn an_empty_words_line_is_kept() {
        let lyrics = lyrics_of(
            SyncType::LineSynced,
            &[("0", "1000", "first"), ("1000", "2000", "")],
        );
        let parsed = track_lyrics(&lyrics);
        assert_eq!(parsed.lines.len(), 2);
        assert_eq!(parsed.lines[1].words, "");
    }

    #[test]
    fn an_empty_lyric_has_no_lines() {
        let lyrics = lyrics_of(SyncType::LineSynced, &[]);
        let parsed = track_lyrics(&lyrics);
        assert!(parsed.lines.is_empty());
        assert!(parsed.synced);
    }

    #[test]
    fn not_found_settles_to_empty_lyrics() {
        let error = Error::not_found(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "no lyrics for this track",
        ));
        assert_eq!(
            settled(Err(error)).expect("a not-found is empty lyrics"),
            TrackLyrics::default()
        );
    }

    #[test]
    fn another_error_settles_to_its_source() {
        let error = Error::internal("the provider fell over");
        assert!(
            matches!(settled(Err(error)), Err(LyricsError::Source(_))),
            "an internal failure is not a track without lyrics"
        );
    }

    #[test]
    fn a_payload_settles_to_its_lines() {
        let lyrics = lyrics_of(SyncType::Unsynced, &[("", "", "words")]);
        let parsed = settled(Ok(lyrics)).expect("a payload settles");
        assert_eq!(
            parsed,
            TrackLyrics {
                lines: vec![line(None, None, "words")],
                synced: false,
            }
        );
    }
}
