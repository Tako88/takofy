//! The shared types, re-exported so `crate::model` resolves.
//!
//! The definitions live in `takofy-model`, which holds no network and no I/O;
//! see that crate for why they are not here.

pub use takofy_model::{PlaylistEntry, TrackEntry};
