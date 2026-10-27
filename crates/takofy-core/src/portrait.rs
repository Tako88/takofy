//! Reading an artist's portrait from an `ARTIST_V4` answer.
//!
//! Pure: the wire's bytes in, a typed answer out, so the parse and the ordering
//! can be exercised without a session. The request itself is in `remote.rs`,
//! the one file that performs librespot I/O.

use std::collections::{HashMap, HashSet};

use librespot::core::{SpotifyId, SpotifyUri};
use librespot::protocol::metadata::Artist as ArtistMessage;
use protobuf::Message as _;
use takofy_model::{ArtistPortrait, CoverSet};

use crate::cover::covers_in_group;

/// One artist's answer, keyed by the artist URI the record itself names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Portrait {
    /// The artist URI, built from the record's own gid so the answer does not
    /// depend on how the server echoed the request. Falls back to the echoed
    /// URI, and is `None` when neither names one.
    pub artist: Option<String>,
    /// `None` when the record carries no portrait group.
    pub portrait: Option<CoverSet>,
}

/// The portrait group's image, or `None` when the record carries none.
pub fn portrait_of(message: &ArtistMessage) -> Option<CoverSet> {
    covers_in_group(message.portrait_group.as_ref())
}

/// The portrait an encoded artist answer carries, or `None` when the bytes are
/// not an artist message.
pub fn portrait_from(bytes: &[u8], echoed: &str) -> Option<Portrait> {
    let message = ArtistMessage::parse_from_bytes(bytes).ok()?;
    let portrait = portrait_of(&message);
    // A record that names itself by nothing and carries no portrait is not an
    // answer; caching it would show the fallback for a record never received.
    if message.gid.is_none() && portrait.is_none() {
        return None;
    }
    let artist = artist_uri(&message, echoed);
    Some(Portrait { artist, portrait })
}

/// The artist's own URI: its gid names it, so the echo is only a fallback.
fn artist_uri(message: &ArtistMessage, echoed: &str) -> Option<String> {
    message
        .gid
        .as_deref()
        .and_then(|gid| SpotifyId::from_raw(gid).ok())
        .map(|id| SpotifyUri::Artist { id })
        .and_then(|uri| uri.to_uri().ok())
        .or_else(|| Some(echoed.to_owned()).filter(|uri| !uri.is_empty()))
}

/// One portrait per requested artist, in the requested order.
///
/// A requested URI that no answer names is omitted, and every request is
/// answered once however often it was asked for.
pub fn portraits_of(
    requested: &[String],
    found: &HashMap<String, Portrait>,
) -> Vec<ArtistPortrait> {
    let answered: HashMap<&str, &Portrait> = found
        .values()
        .filter_map(|portrait| Some((portrait.artist.as_deref()?, portrait)))
        .collect();
    let mut seen = HashSet::new();
    requested
        .iter()
        .filter(|uri| seen.insert(uri.as_str()))
        .filter_map(|uri| {
            let portrait = answered.get(uri.as_str())?;
            Some(ArtistPortrait {
                artist: uri.clone(),
                portrait: portrait.portrait.clone(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests;
