//! Spotify session, playback and library.
//!
//! Everything that talks to Spotify lives here, and nothing that draws does.
//! `takofy-ui` depends on this crate and never the other way round, so a view
//! cannot reach the network by accident.
//!
//! Empty for now; `#1` fills it in.

pub mod context;
pub mod credentials;
pub mod engine;
pub mod login;
pub mod model;
pub mod oauth;
pub mod rootlist;

mod remote;

pub use context::{ContextError, Page, page_from_proto, parse_page_json};
pub use engine::{Engine, EngineError, TrackPages};
pub use model::{PlaylistEntry, TrackEntry};
pub use rootlist::{RootlistError, RootlistPage, RootlistWire, parse_rootlist};
