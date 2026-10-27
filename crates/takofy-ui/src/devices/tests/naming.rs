use takofy_model::{CoverSet, CoverUrl};

use super::super::*;
use super::test_support::*;

fn named_entry(uri: &str, title: &str) -> TrackEntry {
    TrackEntry {
        title: title.to_owned(),
        artists: "Vela".to_owned(),
        named: true,
        cover: Some(CoverSet::single(
            CoverUrl::from_wire("https://i.scdn.co/image/abc").expect("a cover url"),
        )),
        ..entry(uri)
    }
}

fn labelled(entry: TrackEntry) -> Update {
    Update::Devices(DeviceUpdate::Labelled(entry))
}

#[test]
fn an_unnamed_holder_reads_as_another_device_and_asks_for_one_refresh() {
    let now = base();
    let mut devices = Devices::default();
    devices.apply(&roster(vec![phone()], Some("z")), now);
    assert_eq!(devices.caption(now), UNNAMED_HOLDER);
    assert_eq!(devices.refresh_request(), Some(Command::RefreshDevices));

    devices.apply(&roster(vec![phone()], Some("z")), now);
    assert_eq!(
        devices.refresh_request(),
        None,
        "the same unnamed holder was asked for twice"
    );

    devices.apply(&roster(vec![phone()], Some("y")), now);
    assert_eq!(
        devices.refresh_request(),
        Some(Command::RefreshDevices),
        "a different unnamed holder is a new question"
    );
}

#[test]
fn a_roster_that_names_the_holder_ends_the_naming_ask() {
    let now = base();
    let mut devices = Devices::default();
    devices.apply(&roster(vec![phone()], Some("z")), now);
    assert_eq!(devices.refresh_request(), Some(Command::RefreshDevices));

    devices.apply(&roster(vec![device("z", "Tablet")], Some("z")), now);
    assert_eq!(devices.caption(now), "Tablet");
    assert_eq!(devices.refresh_request(), None);

    devices.apply(&roster(vec![phone()], Some("z")), now);
    assert_eq!(devices.caption(now), UNNAMED_HOLDER);
    assert_eq!(
        devices.refresh_request(),
        Some(Command::RefreshDevices),
        "a holder that dropped off the roster is unnamed again"
    );
}

#[test]
fn a_label_is_asked_for_once_per_track_even_when_it_never_arrives() {
    let now = base();
    let mut devices = remote(now);
    assert_eq!(
        devices.label_request(),
        Some(Command::LabelRemote {
            track: "spotify:track:a".to_owned()
        })
    );
    assert_eq!(devices.label_request(), None, "the miss was not cached");

    let mut devices = watching(now);
    assert_eq!(devices.label_request(), None, "nothing playing was asked");

    devices.apply(
        &playing(Some(playback(RemoteState::Paused, "spotify:track:b", None))),
        now,
    );
    assert_eq!(
        devices.label_request(),
        Some(Command::LabelRemote {
            track: "spotify:track:b".to_owned()
        }),
        "a new track is a new question"
    );
}

#[test]
fn a_label_names_the_track_and_its_cover() {
    let now = base();
    let mut devices = remote(now);
    assert!(devices.apply(&labelled(named_entry("spotify:track:a", "Song")), now));
    assert_eq!(
        devices.remote.now_playing,
        Some(NowPlaying {
            uri: "spotify:track:a".to_owned(),
            title: "Song".to_owned(),
            artists: "Vela".to_owned(),
            album_uri: None,
            artist_uri: None,
            cover: Some(CoverSet::single(
                CoverUrl::from_wire("https://i.scdn.co/image/abc").expect("a cover url"),
            )),
        })
    );
}

#[test]
fn a_label_names_the_album_and_first_artist_uris() {
    let now = base();
    let mut devices = remote(now);
    let mut entry = named_entry("spotify:track:a", "Song");
    entry.album_uri = Some("spotify:album:one".to_owned());
    entry.artist_uris = vec![
        "spotify:artist:first".to_owned(),
        "spotify:artist:second".to_owned(),
    ];

    assert!(devices.apply(&labelled(entry), now));

    let now = devices.remote.now_playing.expect("the label named it");
    assert_eq!(now.album_uri.as_deref(), Some("spotify:album:one"));
    assert_eq!(now.artist_uri.as_deref(), Some("spotify:artist:first"));
}

#[test]
fn a_label_for_a_track_that_moved_on_is_ignored() {
    let now = base();
    let mut devices = remote(now);
    assert!(!devices.apply(&labelled(named_entry("spotify:track:b", "Other")), now));
    assert_eq!(devices.remote.now_playing, None);
}

#[test]
fn a_track_that_comes_back_is_named_again_without_asking() {
    let now = base();
    let mut devices = remote(now);
    assert!(devices.apply(&labelled(named_entry("spotify:track:a", "Song")), now));

    // The phone moves on to a track no label has named.
    assert!(devices.apply(
        &playing(Some(playback(
            RemoteState::Playing,
            "spotify:track:b",
            Some(10_000)
        ))),
        now
    ));
    assert_eq!(devices.remote.now_playing, None);
    assert_eq!(
        devices.label_request(),
        Some(Command::LabelRemote {
            track: "spotify:track:b".to_owned()
        })
    );

    // And back to the one it named.
    assert!(devices.apply(
        &playing(Some(playback(
            RemoteState::Playing,
            "spotify:track:a",
            Some(10_000)
        ))),
        now
    ));
    assert_eq!(
        devices
            .remote
            .now_playing
            .as_ref()
            .map(|now| now.title.as_str()),
        Some("Song"),
        "a cached name did not come back with its track"
    );
    assert_eq!(
        devices.label_request(),
        None,
        "a cached name was asked again"
    );
}

#[test]
fn stopping_and_starting_the_same_track_keeps_its_name() {
    let now = base();
    let mut devices = remote(now);
    assert!(devices.apply(&labelled(named_entry("spotify:track:a", "Song")), now));

    assert!(devices.apply(&playing(None), now));
    assert_eq!(
        devices.remote.now_playing, None,
        "the stop still wipes the bar"
    );

    assert!(devices.apply(
        &playing(Some(playback(
            RemoteState::Playing,
            "spotify:track:a",
            Some(10_000)
        ))),
        now
    ));
    assert_eq!(
        devices
            .remote
            .now_playing
            .as_ref()
            .map(|now| now.title.as_str()),
        Some("Song"),
        "the replay lost its name"
    );
    assert_eq!(devices.label_request(), None, "the replay was asked again");
}

#[test]
fn a_push_that_already_names_the_track_asks_for_no_label() {
    let now = base();
    let mut devices = watching(now);
    let mut pushed = playback(RemoteState::Playing, "spotify:track:a", Some(10_000));
    pushed.track = named_entry("spotify:track:a", "Song");

    assert!(devices.apply(&playing(Some(pushed)), now));

    assert_eq!(
        devices
            .remote
            .now_playing
            .as_ref()
            .map(|now| now.title.as_str()),
        Some("Song")
    );
    assert_eq!(
        devices.label_request(),
        None,
        "a named push asked for a label"
    );
}

#[test]
fn a_missing_label_leaves_the_title_unset_and_the_caption_drawn() {
    let now = base();
    let devices = remote(now);
    assert_eq!(devices.remote.now_playing, None);
    assert_eq!(devices.caption(now), "Phone");
}
