//! Spotify session, playback and library.
//!
//! Everything that talks to Spotify lives here, and nothing that draws does.
//! `takofy-ui` does not depend on this crate at all; the two share only
//! `takofy-model`, which has no network. So a view cannot reach the network,
//! even by accident.

pub mod context;
pub mod credentials;
pub mod engine;
pub mod login;
pub mod model;
pub mod oauth;
pub mod playback;
pub mod rootlist;

mod remote;

pub use context::{ContextError, Page, page_from_proto, parse_page_json};
pub use engine::{Engine, EngineError, TrackPages};
pub use model::{PlaylistEntry, TrackEntry};
pub use playback::{Outcome, Playback, PlaybackError, PlaybackEvent, drive, map_event};
pub use rootlist::{RootlistError, RootlistPage, RootlistWire, parse_rootlist};
