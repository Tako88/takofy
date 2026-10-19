//! Selection prompts: parsing a choice and formatting a list line.

use takofy_model::{PlaylistEntry, TrackEntry};

/// A parsed answer to a numbered prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Choice {
    Pick(usize),
    Next,
    Quit,
    Invalid,
}

/// Parses a 1-based choice against `len` options.
pub(crate) fn parse_choice(input: &str, len: usize) -> Choice {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Choice::Quit;
    }
    if trimmed.eq_ignore_ascii_case("n") {
        return Choice::Next;
    }
    if trimmed.eq_ignore_ascii_case("q") {
        return Choice::Quit;
    }
    match trimmed.parse::<usize>() {
        Ok(number) if (1..=len).contains(&number) => Choice::Pick(number),
        _ => Choice::Invalid,
    }
}

pub(crate) fn format_playlist_line(index: usize, entry: &PlaylistEntry) -> String {
    format!("{index}. {} - {}", entry.name, entry.uri)
}

pub(crate) fn format_track_line(index: usize, track: &TrackEntry) -> String {
    format!(
        "{index}. {} - {} ({})",
        track.title, track.artists, track.uri
    )
}

/// Reads one trimmed line from stdin, or `None` at EOF or on error.
pub(crate) async fn read_line() -> Option<String> {
    tokio::task::spawn_blocking(|| {
        let mut line = String::new();
        match std::io::stdin().read_line(&mut line) {
            Ok(0) => None,
            Ok(_) => Some(line.trim().to_owned()),
            Err(_) => None,
        }
    })
    .await
    .ok()
    .flatten()
}

#[cfg(test)]
mod tests {
    use super::{Choice, format_playlist_line, format_track_line, parse_choice};
    use takofy_model::{PlaylistEntry, TrackEntry};

    #[test]
    fn in_range_numbers_pick() {
        assert_eq!(parse_choice("1", 3), Choice::Pick(1));
        assert_eq!(parse_choice("3", 3), Choice::Pick(3));
    }

    #[test]
    fn out_of_range_numbers_are_invalid() {
        assert_eq!(parse_choice("4", 3), Choice::Invalid);
        assert_eq!(parse_choice("999", 3), Choice::Invalid);
    }

    #[test]
    fn zero_is_invalid() {
        assert_eq!(parse_choice("0", 3), Choice::Invalid);
    }

    #[test]
    fn surrounding_whitespace_is_trimmed() {
        assert_eq!(parse_choice("  2\t", 3), Choice::Pick(2));
    }

    #[test]
    fn whitespace_only_quits() {
        assert_eq!(parse_choice("  \t ", 3), Choice::Quit);
    }

    #[test]
    fn next_is_case_insensitive() {
        assert_eq!(parse_choice("n", 3), Choice::Next);
        assert_eq!(parse_choice("N", 3), Choice::Next);
    }

    #[test]
    fn quit_is_case_insensitive() {
        assert_eq!(parse_choice("q", 3), Choice::Quit);
        assert_eq!(parse_choice("Q", 3), Choice::Quit);
    }

    #[test]
    fn empty_line_quits() {
        assert_eq!(parse_choice("", 3), Choice::Quit);
    }

    #[test]
    fn non_numeric_input_is_invalid() {
        assert_eq!(parse_choice("abc", 3), Choice::Invalid);
        assert_eq!(parse_choice("1x", 3), Choice::Invalid);
    }

    #[test]
    fn playlist_line_names_the_playlist() {
        let entry = PlaylistEntry {
            uri: "spotify:playlist:x".to_owned(),
            name: "Mix".to_owned(),
            named: true,
            cover: None,
        };
        assert_eq!(
            format_playlist_line(2, &entry),
            "2. Mix - spotify:playlist:x"
        );
    }

    #[test]
    fn track_line_names_the_track() {
        let track = TrackEntry {
            uri: "spotify:track:y".to_owned(),
            title: "Song".to_owned(),
            artists: "Band".to_owned(),
            album: None,
            duration_ms: None,
            named: true,
            cover: None,
        };
        assert_eq!(
            format_track_line(1, &track),
            "1. Song - Band (spotify:track:y)"
        );
    }
}
