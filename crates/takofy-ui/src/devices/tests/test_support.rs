use super::super::*;

pub(super) fn base() -> Instant {
    Instant::now()
}

pub(super) fn device(id: &str, name: &str) -> Device {
    Device {
        id: id.to_owned(),
        name: name.to_owned(),
        kind: DeviceKind::Phone,
    }
}

pub(super) fn phone() -> Device {
    device("a", "Phone")
}

pub(super) fn laptop() -> Device {
    device("b", "Laptop")
}

pub(super) fn entry(uri: &str) -> TrackEntry {
    TrackEntry {
        uri: uri.to_owned(),
        title: uri.to_owned(),
        artists: String::new(),
        album: None,
        album_uri: None,
        artist_uris: Vec::new(),
        artist_names: Vec::new(),
        added_at: None,
        duration_ms: None,
        named: false,
        cover: None,
    }
}

pub(super) fn playback(state: RemoteState, uri: &str, duration: Option<u32>) -> RemotePlayback {
    RemotePlayback {
        track: entry(uri),
        state,
        position_ms: 1_000,
        duration_ms: duration,
        steerable: true,
    }
}

pub(super) fn roster(devices: Vec<Device>, holder: Option<&str>) -> Update {
    roster_of(devices, holder, false)
}

/// The same, saying whether this window is the device holding the sound.
pub(super) fn roster_of(devices: Vec<Device>, holder: Option<&str>, held_here: bool) -> Update {
    Update::Devices(DeviceUpdate::Roster {
        devices,
        holder: holder.map(str::to_owned),
        held_here,
    })
}

pub(super) fn playing(playback: Option<RemotePlayback>) -> Update {
    Update::Devices(DeviceUpdate::Playing(playback))
}

/// A watch that has answered with two devices, the phone holding nothing.
pub(super) fn watching(now: Instant) -> Devices {
    let mut devices = Devices::default();
    assert_eq!(devices.watch_request(true), Some(Command::WatchDevices));
    devices.apply(&roster(vec![phone(), laptop()], Some("a")), now);
    devices
}

/// The same, with the phone playing a track.
pub(super) fn remote(now: Instant) -> Devices {
    let mut devices = watching(now);
    devices.apply(
        &playing(Some(playback(
            RemoteState::Playing,
            "spotify:track:a",
            Some(10_000),
        ))),
        now,
    );
    devices
}
