//! A show as its page draws it.
//!
//! Pure: a parsed [`Show`] and the labels for its episode URIs in, a
//! [`ShowDetail`] out. The request itself is in `remote::read_show`.
//!
//! An episode's order is the show's own: the page keeps the wire's order and
//! never sorts, because a sort would break a serial show.

use std::collections::HashMap;

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

/// The labelled detail of every show read, in the order they were read.
///
/// One read names every episode of every show, so the labels arrive as one map
/// and each show expands its own order from it. A URI two shows share expands
/// for both, and a run of URIs that did not answer falls back to unnamed in both
/// shows it crosses.
pub(crate) fn details(
    shows: &[(String, Show)],
    found: &HashMap<String, Episode>,
) -> Vec<ShowDetail> {
    shows
        .iter()
        .map(|(uri, show)| detail(uri, show, expand_episodes(&episode_uris(show), found)))
        .collect()
}

/// One episode per input position, falling back to the URI when it was not
/// answered. The wire's order is the page's order, so this never sorts.
///
/// The position's own URI is what it draws: the answer is looked up by that URI,
/// and an answer that carried a different one cannot move the episode to another
/// position.
pub(crate) fn expand_episodes(uris: &[String], found: &HashMap<String, Episode>) -> Vec<Episode> {
    uris.iter()
        .map(|uri| match found.get(uri) {
            Some(episode) => Episode {
                uri: uri.clone(),
                ..episode.clone()
            },
            None => Episode {
                uri: uri.clone(),
                title: uri.clone(),
                show: String::new(),
                show_uri: None,
                description: None,
                published: None,
                duration_ms: None,
                cover: None,
                named: false,
            },
        })
        .collect()
}

#[cfg(test)]
mod tests;
