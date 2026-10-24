//! One cluster view, as the fine-grained updates the window applies.

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
        };
        if sent.as_ref() != Some(&roster) {
            updates.push(Update::Devices(DeviceUpdate::Roster {
                devices: roster.devices.clone(),
                holder: roster.holder.clone(),
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
mod tests {
    use takofy_model::{DeviceKind, RemotePlayback, RemoteState, TrackEntry};

    use super::*;

    fn device(id: &str) -> Device {
        Device {
            id: id.to_owned(),
            name: format!("Device {id}"),
            kind: DeviceKind::Phone,
        }
    }

    fn track() -> TrackEntry {
        TrackEntry {
            uri: "spotify:track:a".to_owned(),
            title: "A".to_owned(),
            artists: "Vela".to_owned(),
            album: None,
            album_uri: None,
            artist_uris: Vec::new(),
            artist_names: Vec::new(),
            added_at: None,
            duration_ms: Some(1_000),
            named: true,
            cover: None,
        }
    }

    fn playback(state: RemoteState) -> RemotePlayback {
        RemotePlayback {
            track: track(),
            state,
            position_ms: 10,
            duration_ms: Some(1_000),
            steerable: true,
        }
    }

    fn view(
        devices: Option<Vec<Device>>,
        holder: Option<&str>,
        playback: Option<RemotePlayback>,
    ) -> ClusterView {
        ClusterView {
            devices,
            holder: holder.map(str::to_owned),
            playback,
        }
    }

    #[test]
    fn a_view_is_a_roster_then_the_playback() {
        let view = view(
            Some(vec![device("a")]),
            Some("a"),
            Some(playback(RemoteState::Playing)),
        );
        let updates = device_updates(&view, &mut None);

        assert_eq!(
            updates,
            vec![
                Update::Devices(DeviceUpdate::Roster {
                    devices: vec![device("a")],
                    holder: Some("a".to_owned()),
                }),
                Update::Devices(DeviceUpdate::Playing(Some(playback(RemoteState::Playing)))),
            ]
        );
    }

    #[test]
    fn an_unchanged_roster_is_not_sent_again_but_the_playback_is() {
        let mut sent = None;
        let first = view(Some(vec![device("a")]), Some("a"), None);
        assert_eq!(device_updates(&first, &mut sent).len(), 2);

        let second = view(
            Some(vec![device("a")]),
            Some("a"),
            Some(playback(RemoteState::Paused)),
        );
        assert_eq!(
            device_updates(&second, &mut sent),
            vec![Update::Devices(DeviceUpdate::Playing(Some(playback(
                RemoteState::Paused
            ))))],
            "a volume-only push sends no roster"
        );
    }

    #[test]
    fn a_view_without_a_device_map_keeps_the_roster() {
        let mut sent = None;
        let first = view(Some(vec![device("a")]), None, None);
        let _ = device_updates(&first, &mut sent);

        let second = view(None, None, None);
        assert_eq!(
            device_updates(&second, &mut sent),
            vec![Update::Devices(DeviceUpdate::Playing(None))]
        );
        assert!(sent.is_some(), "the last roster is kept");
    }

    #[test]
    fn a_holder_change_alone_replaces_the_roster() {
        let mut sent = None;
        let first = view(Some(vec![device("a"), device("b")]), Some("a"), None);
        let _ = device_updates(&first, &mut sent);

        let second = view(Some(vec![device("a"), device("b")]), Some("b"), None);
        assert_eq!(
            device_updates(&second, &mut sent),
            vec![
                Update::Devices(DeviceUpdate::Roster {
                    devices: vec![device("a"), device("b")],
                    holder: Some("b".to_owned()),
                }),
                Update::Devices(DeviceUpdate::Playing(None)),
            ]
        );
    }
}
