//! The pictures a station answer names, as the head and the rows draw them.
//!
//! The wire names a picture two ways: one image id for a single cover, and a
//! `:`-separated list of ids for the four-up a playlist seeds. Both are read
//! here, so a station's head and a row's own cover come from one place.

use takofy_model::home::StationArt;
use takofy_model::{COMPOSED_MAX, CoverSet, CoverUrl};

/// The wire's single-picture prefix, whose id is a 20-byte file id in hex.
const IMAGE: &str = "spotify:image:";

/// The wire's composed-picture prefix, whose parts are image ids without it.
const MOSAIC: &str = "spotify:mosaic:";

/// The picture `uri` names, or `None` when no part of it is a real image id.
///
/// One valid part of a mosaic is a single picture: fewer than two tile nothing.
pub(super) fn art_of(uri: &str) -> Option<StationArt> {
    let Some(parts) = uri.strip_prefix(MOSAIC) else {
        return cover_of(uri).map(StationArt::Single);
    };
    let covers: Vec<CoverSet> = parts
        .split(':')
        .filter_map(cover_of_id)
        .take(COMPOSED_MAX)
        .collect();
    match covers.len() {
        0 => None,
        1 => covers.into_iter().next().map(StationArt::Single),
        _ => Some(StationArt::Mosaic(covers)),
    }
}

/// The one picture an image URI names, which is what a row's cover is.
pub(super) fn cover_of(uri: &str) -> Option<CoverSet> {
    cover_of_id(uri.strip_prefix(IMAGE)?)
}

/// One image id — the hex after the prefix — as a single-cover set.
fn cover_of_id(id: &str) -> Option<CoverSet> {
    CoverUrl::from_file_id(&decode(id)?).map(CoverSet::single)
}

/// The bytes a hex string spells, or `None` when it is not hex.
///
/// The length is the caller's business: a file id is twenty bytes, so anything
/// else is refused by [`CoverUrl::from_file_id`] rather than here.
fn decode(hex: &str) -> Option<Vec<u8>> {
    if hex.is_empty() || !hex.len().is_multiple_of(2) {
        return None;
    }
    let bytes = hex.as_bytes();
    let mut out = Vec::with_capacity(bytes.len() / 2);
    for pair in bytes.as_chunks::<2>().0 {
        let high = char::from(pair[0]).to_digit(16)?;
        let low = char::from(pair[1]).to_digit(16)?;
        out.push(u8::try_from(high * 16 + low).ok()?);
    }
    Some(out)
}

#[cfg(test)]
mod tests;
