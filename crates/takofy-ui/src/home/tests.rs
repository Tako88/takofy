use super::*;

fn named(uri: &str, album: Option<&str>) -> TrackEntry {
    TrackEntry {
        uri: uri.to_owned(),
        title: uri.to_owned(),
        artists: String::new(),
        album: None,
        album_uri: album.map(str::to_owned),
        artist_uris: Vec::new(),
        artist_names: Vec::new(),
        added_at: None,
        duration_ms: None,
        named: true,
        cover: None,
    }
}

fn uris(values: &[&str]) -> Vec<String> {
    values.iter().map(|uri| (*uri).to_owned()).collect()
}

#[test]
fn opening_home_twice_draws_two_seeds() {
    let mut home = Home::default();
    home.open(1_700_000_000_000);
    let first = home.seed();
    home.open(1_700_000_000_000);
    assert_ne!(first, home.seed(), "a second open is a second draw");
    assert_eq!(home.opens, 2);
}

#[test]
fn a_named_track_with_an_album_is_usable() {
    let mut home = Home::default();
    let track = named("spotify:track:a", Some("spotify:album:r"));
    home.apply_labels(&uris(&["spotify:track:a"]), std::slice::from_ref(&track));

    assert_eq!(home.label("spotify:track:a"), Some(&track));
    assert!(home.answered("spotify:track:a"));
}

#[test]
fn an_unnamed_track_or_one_with_no_album_is_unusable() {
    let mut home = Home::default();
    let mut unnamed = named("spotify:track:a", Some("spotify:album:r"));
    unnamed.named = false;
    let albumless = named("spotify:track:b", None);
    home.apply_labels(
        &uris(&["spotify:track:a", "spotify:track:b"]),
        &[unnamed, albumless],
    );

    assert_eq!(home.label("spotify:track:a"), None);
    assert_eq!(home.label("spotify:track:b"), None);
    assert!(home.answered("spotify:track:a"));
    assert!(home.answered("spotify:track:b"));
}

#[test]
fn a_track_the_read_did_not_name_is_unusable() {
    let mut home = Home::default();
    home.apply_labels(&uris(&["spotify:track:a"]), &[]);

    assert_eq!(home.label("spotify:track:a"), None);
    assert!(home.answered("spotify:track:a"));
}

#[test]
fn an_unasked_uri_is_absent() {
    let mut home = Home::default();
    home.apply_labels(
        &uris(&["spotify:track:a"]),
        &[named("spotify:track:b", Some("spotify:album:r"))],
    );

    assert!(!home.asked("spotify:track:b"));
    assert!(!home.answered("spotify:track:b"));
    assert_eq!(home.label("spotify:track:b"), None);
}

#[test]
fn a_sent_uri_is_asked_but_not_yet_answered() {
    let mut home = Home::default();
    home.mark_asked(&uris(&["spotify:track:a"]));

    assert!(home.asked("spotify:track:a"), "the read was sent for it");
    assert!(!home.answered("spotify:track:a"), "no reply has landed yet");
    assert_eq!(home.label("spotify:track:a"), None);

    home.apply_labels(
        &uris(&["spotify:track:a"]),
        &[named("spotify:track:a", Some("spotify:album:r"))],
    );
    assert!(home.answered("spotify:track:a"), "the reply landed");
}
