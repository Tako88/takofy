//! The device watch: its state, arming it, and adopting the answer it brings.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use takofy_core::receive::Inbound;
use takofy_ui::message::Failure;
use tokio::sync::mpsc::UnboundedReceiver;

use super::covers::Drains;
use super::errors::ends_playability;
use super::wake::Notifier;
use super::{device_failure, log_device_error, spawn_device_drain};
use crate::devices::SentRoster;
use crate::session::{SessionError, SessionHost};

/// State the device watch keeps across commands: whether it was started, the
/// request stream it handed over, the roster the window was told, and whether
/// the pump has ended.
pub(super) struct Devices {
    pub(super) watching: bool,
    /// Whether the watch was running when its session was retired, so a rebuilt
    /// one starts it again.
    pub(super) rewatch: bool,
    pub(super) inbound: Option<UnboundedReceiver<Inbound>>,
    pub(super) sent: SentRoster,
    pub(super) dead: Arc<AtomicBool>,
}

impl Default for Devices {
    fn default() -> Self {
        Self {
            watching: false,
            rewatch: false,
            inbound: None,
            sent: SentRoster::default(),
            dead: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl Devices {
    /// Stops the watch this session owned: its request stream, and the
    /// end-of-stream notice its drain would otherwise send.
    ///
    /// A watch that was already due to be restarted stays due, so a session
    /// retired twice is still watched again by the rebuild.
    pub(super) fn retire(&mut self) {
        self.rewatch |= self.watching;
        self.watching = false;
        self.inbound = None;
        self.dead = Arc::new(AtomicBool::new(false));
    }
}

/// Readies the player and marks the watch as running, before the dealer call so
/// a racing request cannot start it twice.
pub(super) fn arm_watch<H: SessionHost>(host: &mut H, devices: &mut Devices) {
    // The audio path is readied before the device is announced, so the account
    // is told this window can play before anything here has played. Without it
    // the phone can only pick this device after a local play, which makes a
    // freshly opened window the one device it cannot choose. A machine with no
    // audio device is the honest exception: it announces a controller, which the
    // phone lists and greys out.
    match host.prepare_player() {
        Ok(()) => host.set_playable(true),
        Err(error) => {
            if ends_playability(&error) {
                host.set_playable(false);
            }
            log::warn!("device prepare failed: {}", error.kind());
        }
    }
    devices.watching = true;
    devices.dead.store(false, Ordering::SeqCst);
}

/// Adopts one watch's answer: its streams, or the failure the window is told.
pub(super) fn adopt_watch<H: SessionHost>(
    host: &mut H,
    notifier: &Notifier,
    devices: &mut Devices,
    drains: &Drains,
    result: Result<(), SessionError>,
) {
    match result {
        Err(error) => {
            log_device_error(&error);
            devices.watching = false;
            let failure = if error.devices_terminal() {
                Failure::DevicesOff
            } else {
                Failure::Devices
            };
            notifier.send(device_failure(failure));
        }
        Ok(()) => {
            devices.rewatch = false;
            if let Some(events) = host.take_devices() {
                spawn_device_drain(
                    notifier.clone(),
                    events,
                    devices.sent.clone(),
                    Arc::clone(&devices.dead),
                    Arc::clone(&drains.retired),
                );
            } else {
                // No stream to read, but the device was announced: keep
                // `watching` so the exit still withdraws it.
                notifier.send(device_failure(Failure::DevicesOff));
            }
            // Taken beside the events: the loop answers requests through it,
            // and a stream with nobody to read it would refuse them untold.
            devices.inbound = host.take_inbound();
        }
    }
}
