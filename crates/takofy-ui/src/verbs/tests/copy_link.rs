use super::super::*;

#[test]
fn copy_link_maps_a_track_uri_to_its_web_url() {
    assert_eq!(
        spotify_link("spotify:track:abc").as_deref(),
        Some("https://open.spotify.com/track/abc")
    );
}

#[test]
fn copy_link_handles_episodes_and_refuses_local_and_malformed_uris() {
    assert_eq!(
        spotify_link("spotify:episode:xyz").as_deref(),
        Some("https://open.spotify.com/episode/xyz")
    );
    for refused in [
        "spotify:local:Artist:Album:Title:180000",
        "spotify:track:",
        "track:abc",
        "",
        "spotify:track:a:b",
        "spotify:user:alice",
    ] {
        assert_eq!(spotify_link(refused), None, "{refused}");
    }
}

#[test]
fn copy_link_maps_an_account_named_playlist_to_its_playlist_page() {
    assert_eq!(
        spotify_link("spotify:user:someone:playlist:4lRh9XA5hFPyKVnThKW3mi").as_deref(),
        Some("https://open.spotify.com/playlist/4lRh9XA5hFPyKVnThKW3mi")
    );
}

#[test]
fn copy_link_refuses_an_id_that_is_not_one_path_segment() {
    assert_eq!(spotify_link("spotify:playlist:abc:extra"), None);
}

#[test]
fn copy_link_maps_every_library_kind_to_its_web_url() {
    let id = "4uLU6hMCjMI75M1A2tKUQC";
    for kind in ["playlist", "album", "artist", "show"] {
        let uri = format!("spotify:{kind}:{id}");
        assert_eq!(
            spotify_link(&uri),
            Some(format!("https://open.spotify.com/{kind}/{id}")),
            "{uri}"
        );
    }
}
