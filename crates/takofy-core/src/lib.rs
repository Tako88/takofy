//! Spotify session, playback and library.
//!
//! Everything that talks to Spotify lives here, and nothing that draws does.
//! The UI crate depends on this one; this one never depends on it, so a view
//! cannot reach the network even by accident.
//!
//! Phase 1 (`#1`) fills this in: OAuth login into the OS keyring, the rootlist
//! and paginated contexts out of `librespot_core::spclient`, and the player.
//! The crate exists now so that the dependency tree — which is where the risk
//! is — is already proven to build.
