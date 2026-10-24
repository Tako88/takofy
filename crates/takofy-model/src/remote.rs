//! What the window and the engine exchange about the account's devices.
//!
//! Plain data: a request the engine is still working on is named by its id, and
//! the window has no way to answer one except through that id.

/// The handle the dealer gave one incoming command.
///
/// Only the engine mints one; the window keeps it so its answer can name the
/// request it belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RequestId(pub u64);
