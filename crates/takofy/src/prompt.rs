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

#[cfg(test)]
mod tests;
