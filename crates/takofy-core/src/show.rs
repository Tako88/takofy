//! A show as its page draws it.
//!
//! Pure: a parsed [`Show`] and the labels for its episode URIs in, a
//! [`ShowDetail`] out. The request itself is in `remote::read_show`.
//!
//! An episode's order is the show's own: the page keeps the wire's order and
//! never sorts, because a sort would break a serial show.

use librespot::core::SpotifyUri;
use librespot::metadata::Show;
use takofy_model::{Episode, ShowDetail};

use crate::cover::covers_in_images;

/// Why a show read failed.
#[derive(Debug, thiserror::Error)]
pub enum ShowError {
    /// The URI does not name a show, so there is nothing to ask for.
    #[error("not a show URI")]
    NotAShow,
    /// The request failed; the message is the source's.
    #[error("{0}")]
    Source(String),
}

/// The show's episode URIs, in its own order.
///
/// Only `SpotifyUri::Episode` positions are episodes, and one whose URI cannot be
/// written is left out rather than guessed at.
pub(crate) fn episode_uris(show: &Show) -> Vec<String> {
    show.episodes
        .iter()
        .filter(|uri| matches!(uri, SpotifyUri::Episode { .. }))
        .filter_map(|uri| uri.to_uri().ok())
        .collect()
}

/// The show's page data, from the show and its labelled episodes.
///
/// The name falls back to the URI exactly as a playlist's does, and a blank
/// publisher reads as no publisher rather than a line of spaces.
pub(crate) fn detail(uri: &str, show: &Show, episodes: Vec<Episode>) -> ShowDetail {
    let name = if show.name.is_empty() {
        uri.to_owned()
    } else {
        show.name.clone()
    };
    ShowDetail {
        show: takofy_model::Show {
            uri: uri.to_owned(),
            name,
            publisher: show.publisher.trim().to_owned(),
            description: show.description.clone(),
            cover: covers_in_images(&show.covers),
        },
        episodes,
    }
}

#[cfg(test)]
mod tests;
