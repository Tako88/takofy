//! Spotify session, playback and library.
//!
//! Everything that talks to Spotify lives here, and nothing that draws does.
//! `takofy-ui` does not depend on this crate at all; the two share only
//! `takofy-model`, which has no network. So a view cannot reach the network,
//! even by accident.

pub mod album;
pub mod artist;
pub mod connect;
pub mod context;
pub mod credentials;
pub mod engine;
pub mod followed;
pub mod hold;
pub mod instruction;
pub mod login;
pub mod lyrics;
pub mod oauth;
pub mod playback;
pub mod playlist_edit;
pub mod receive;
pub mod rootlist;
pub mod show;

mod collected;
mod collection;
mod cover;
mod latest;
mod loss;
mod playstate;
mod portrait;
mod presence;
mod publish;
mod registry;
mod remote;
mod search;
#[cfg(test)]
mod testing;
mod transfer;
mod transfer_out;
mod watch;
mod wire;

pub use context::{ContextError, Page, PageTrack, page_from_proto, parse_page_json};
pub use engine::{Engine, EngineError, PlaylistPage, TrackPages};
pub use playback::{Outcome, Playback, PlaybackError, PlaybackEvent, drive, map_event};
pub use rootlist::{RootlistError, RootlistPage, RootlistWire, parse_rootlist};
pub use transfer::{Resolution, TransferPlan};
pub use watch::{ORDER_MARGIN, Watching};
