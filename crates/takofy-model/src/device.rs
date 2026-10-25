//! Devices on the account, and the playback one of them is holding.
//!
//! Plain data: the cluster's parse fills it in, and the window only draws it.

use crate::TrackEntry;

/// A device signed in to the account.
///
/// `id` is the cluster's map key rather than the id inside the device's own
/// info: the cluster names the holder by that key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub kind: DeviceKind,
}

/// The kind of hardware a device is, reduced to the words a row can carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceKind {
    Computer,
    Phone,
    Tablet,
    Speaker,
    Tv,
    Car,
    Other,
}

/// The track another device is playing, and how far into it that device is.
///
/// `position_ms` is already advanced to the instant the push described, so the
/// window anchors its own counting from the moment it applies this.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemotePlayback {
    pub track: TrackEntry,
    pub state: RemoteState,
    pub position_ms: u32,
    /// `None` when the wire gave no positive length, which also makes a seek
    /// impossible to offer.
    pub duration_ms: Option<u32>,
    /// `false` when the server says this client may not control the device.
    pub steerable: bool,
}

/// What a remote device is doing, reduced from three wire flags to one answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteState {
    Playing,
    Paused,
    Loading,
}
