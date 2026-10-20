//! Spotify session, playback and library.
//!
//! Everything that talks to Spotify lives here, and nothing that draws does.
//! `takofy-ui` does not depend on this crate at all; the two share only
//! `takofy-model`, which has no network. So a view cannot reach the network,
//! even by accident.

pub mod album;
pub mod artist;
pub mod context;
pub mod credentials;
pub mod engine;
pub mod followed;
pub mod login;
pub mod oauth;
pub mod playback;
pub mod rootlist;
pub mod show;

mod collected;
mod collection;
mod cover;
mod remote;

pub use context::{ContextError, Page, PageTrack, page_from_proto, parse_page_json};
pub use engine::{Engine, EngineError, TrackPages};
pub use playback::{Outcome, Playback, PlaybackError, PlaybackEvent, drive, map_event};
pub use rootlist::{RootlistError, RootlistPage, RootlistWire, parse_rootlist};
