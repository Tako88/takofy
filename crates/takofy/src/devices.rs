//! One cluster view, as the fine-grained updates the window applies.

use std::sync::{Arc, Mutex};

use takofy_core::connect::ClusterView;
use takofy_model::Device;
use takofy_ui::message::{DeviceUpdate, Update};

/// The roster as it was last sent.
///
/// Kept beside the updates so a push that moves nothing but the position sends
/// no roster: the window replaces a roster wholesale, and an identical one would
/// only cost it a frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Roster {
    pub devices: Vec<Device>,
    pub holder: Option<String>,
    pub held_here: bool,
}

/// The roster the window last received, shared by every path that sends one.
#[derive(Clone, Default)]
pub(crate) struct SentRoster(Arc<Mutex<Option<Roster>>>);

impl SentRoster {
    /// One view as updates, comparing against what any path last sent.
    pub(crate) fn updates(&self, view: &ClusterView) -> Vec<Update> {
        let mut sent = match self.0.lock() {
            Ok(sent) => sent,
            Err(poisoned) => poisoned.into_inner(),
        };
        device_updates(view, &mut sent)
    }
}

/// One view as updates, with the roster only when it differs from `sent`.
///
/// A view with no device map says nothing about the roster, so the last one is
/// kept rather than blanked, and `Playing` always follows.
pub(crate) fn device_updates(view: &ClusterView, sent: &mut Option<Roster>) -> Vec<Update> {
    let mut updates = Vec::new();
    if let Some(devices) = &view.devices {
        let roster = Roster {
            devices: devices.clone(),
            holder: view.holder.clone(),
            held_here: view.held_here,
        };
        if sent.as_ref() != Some(&roster) {
            updates.push(Update::Devices(DeviceUpdate::Roster {
                devices: roster.devices.clone(),
                holder: roster.holder.clone(),
                held_here: roster.held_here,
            }));
            *sent = Some(roster);
        }
    }
    updates.push(Update::Devices(DeviceUpdate::Playing(
        view.playback.clone(),
    )));
    updates
}

#[cfg(test)]
mod tests;
