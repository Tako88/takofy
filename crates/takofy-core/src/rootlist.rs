//! Rootlist parsing and paging.
//!
//! The rootlist body is assumed to be a binary [`SelectedListContent`]; the JSON
//! branch exists for the day that assumption is wrong.

use librespot::core::SpotifyUri;
use librespot::protocol::playlist4_external::{Item, SelectedListContent};
use protobuf::Message;
use protobuf_json_mapping::ParseOptions;
use takofy_model::{PlaylistAccess, Revision};

/// The wire format a rootlist body arrived in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootlistWire {
    Binary,
    Json,
}

/// One page of the rootlist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootlistPage {
    /// Playlist URIs kept from this page.
    pub playlists: Vec<String>,
    /// Raw item count, including folder markers and unparsable URIs.
    pub scanned: usize,
    /// `SelectedListContent.length`, when set and non-negative.
    pub total: Option<usize>,
    /// The list's revision, when the page named one.
    pub revision: Option<Revision>,
}

/// Why a rootlist read failed.
#[derive(Debug, thiserror::Error)]
pub enum RootlistError {
    /// The source itself failed; the message is the source's.
    #[error("{0}")]
    Source(String),
    /// The body could not be decoded. Only its size and wire kind are kept.
    #[error("could not parse a {wire:?} rootlist body of {len} bytes")]
    Parse { wire: RootlistWire, len: usize },
    /// More than [`MAX_ITEMS`] raw items were returned.
    #[error("rootlist has more than {MAX_ITEMS} items")]
    TooLarge,
}

/// Reads one page of the rootlist. Implemented over librespot in `remote`.
pub(crate) trait RootlistSource {
    async fn page(&self, from: usize, len: usize) -> Result<RootlistPage, RootlistError>;
}

/// Page size requested from the rootlist endpoint.
const PAGE_SIZE: usize = 120;
/// Hard cap on raw items, so a server that ignores `from` cannot loop forever.
const MAX_ITEMS: usize = 10_000;

/// Decodes a rootlist body and keeps its playlist URIs.
pub fn parse_rootlist(bytes: &[u8], wire: RootlistWire) -> Result<RootlistPage, RootlistError> {
    let len = bytes.len();
    let content = match wire {
        RootlistWire::Binary => SelectedListContent::parse_from_bytes(bytes)
            .map_err(|_| RootlistError::Parse { wire, len })?,
        RootlistWire::Json => {
            let json =
                std::str::from_utf8(bytes).map_err(|_| RootlistError::Parse { wire, len })?;
            let options = ParseOptions {
                ignore_unknown_fields: true,
                ..Default::default()
            };
            protobuf_json_mapping::parse_from_str_with_options::<SelectedListContent>(
                json, &options,
            )
            .map_err(|_| RootlistError::Parse { wire, len })?
        }
    };
    Ok(page_from_content(&content))
}

fn page_from_content(content: &SelectedListContent) -> RootlistPage {
    let items = &content.contents.items;
    let playlists = items.iter().filter_map(playlist_uri).collect();
    RootlistPage {
        playlists,
        scanned: items.len(),
        total: content
            .length
            .and_then(|length| usize::try_from(length).ok()),
        revision: content.revision.as_deref().and_then(Revision::from_wire),
    }
}

fn playlist_uri(item: &Item) -> Option<String> {
    let uri = item.uri.as_deref()?;
    if matches!(SpotifyUri::from_uri(uri), Ok(SpotifyUri::Playlist { .. })) {
        Some(uri.to_owned())
    } else {
        None
    }
}

/// A finished walk: the playlists it kept, and whether its indices are the
/// wire's own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Walked {
    pub playlists: Vec<String>,
    /// What the sidebar may write with, or `None` when the read named no
    /// revision.
    pub access: Option<PlaylistAccess>,
}

/// Walks the rootlist until `length` items are seen, a page comes back empty, or
/// the raw-item cap trips.
pub(crate) async fn fetch_all(source: &impl RootlistSource) -> Result<Walked, RootlistError> {
    let mut playlists = Vec::new();
    let mut from = 0;
    let mut scanned_total = 0usize;
    let mut revision: Option<Revision> = None;
    let mut first = true;
    let mut steady = true;
    let mut met_total = false;
    loop {
        let page = source.page(from, PAGE_SIZE).await?;
        if first {
            revision = page.revision;
            first = false;
        } else if page.revision != revision {
            steady = false;
        }
        if page.scanned == 0 {
            break;
        }
        scanned_total += page.scanned;
        playlists.extend(page.playlists);
        if let Some(total) = page.total
            && scanned_total >= total
        {
            met_total = total == scanned_total;
            break;
        }
        if scanned_total > MAX_ITEMS {
            return Err(RootlistError::TooLarge);
        }
        from += page.scanned;
    }
    // A dropped item, an unknown or unmet total, or a revision that moved under
    // the walk all make the row index stop being the wire's index.
    let access = revision.map(|revision| PlaylistAccess {
        revision,
        editable: steady && met_total && scanned_total == playlists.len(),
    });
    Ok(Walked { playlists, access })
}

#[cfg(test)]
mod tests;
