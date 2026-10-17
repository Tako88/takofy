//! Spotify session, playback and library.
//!
//! Everything that talks to Spotify lives here, and nothing that draws does.
//! `takofy-ui` depends on this crate and never the other way round, so a view
//! cannot reach the network by accident.
//!
//! Empty for now; `#1` fills it in.

pub mod credentials;
pub mod login;
pub mod oauth;
