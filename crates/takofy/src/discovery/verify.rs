//! Whether two lists of titles name the same records, and which credited
//! artist a search hit belongs to.
//!
//! Normalising is deliberately narrow: it drops a trailing bracket or a
//! trailing suffix, lowercases and keeps alphanumerics. It does not fold
//! diacritics, so an accented title fails to match rather than matching a
//! different record.

use std::collections::HashSet;

use takofy_model::TrackEntry;

/// How many titles two lists must share once both are at least [`SMALL_LIST`].
pub(crate) const OVERLAP_MIN: usize = 2;
/// The smaller list at which [`OVERLAP_MIN`] replaces the single-title rule.
pub(crate) const SMALL_LIST: usize = 4;

/// A title reduced to the part two services can agree on.
pub(crate) fn normalise(title: &str) -> String {
    let mut text = title.trim();
    if let Some((head, _)) = text.rsplit_once(" - ") {
        text = head.trim_end();
    }
    if let Some(head) = strip_trailing_group(text) {
        text = head.trim_end();
    }
    text.chars()
        .flat_map(char::to_lowercase)
        .filter(|character| character.is_alphanumeric())
        .collect()
}

/// Whether the two lists share enough distinct titles.
pub(crate) fn overlaps(spotify: &[String], deezer: &[String]) -> bool {
    let smaller = spotify.len().min(deezer.len());
    let needed = if smaller < SMALL_LIST { 1 } else { OVERLAP_MIN };
    let left: HashSet<String> = spotify
        .iter()
        .map(|title| normalise(title))
        .filter(|title| !title.is_empty())
        .collect();
    let right: HashSet<String> = deezer
        .iter()
        .map(|title| normalise(title))
        .filter(|title| !title.is_empty())
        .collect();
    left.intersection(&right).count() >= needed
}

/// The index of the credited artist whose name matches, when the hit's title
/// also matches.
///
/// `artist_names` is zipped with `artist_uris`, so the caller takes the URI at
/// the returned index rather than looking a name up again.
pub(crate) fn confirm_hit(
    entry: &TrackEntry,
    wanted_title: &str,
    wanted_name: &str,
) -> Option<usize> {
    let title = normalise(wanted_title);
    if title.is_empty() || normalise(&entry.title) != title {
        return None;
    }
    let wanted = normalise(wanted_name);
    if wanted.is_empty() {
        return None;
    }
    entry
        .artist_names
        .iter()
        .zip(entry.artist_uris.iter())
        .position(|(name, _)| normalise(name) == wanted)
}

fn strip_trailing_group(text: &str) -> Option<&str> {
    let close = text.chars().last()?;
    let open = match close {
        ')' => '(',
        ']' => '[',
        '}' => '{',
        _ => return None,
    };
    let mut depth = 0usize;
    for (index, character) in text.char_indices().rev() {
        if character == close {
            depth += 1;
        } else if character == open {
            depth -= 1;
            if depth == 0 {
                return Some(&text[..index]);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests;
