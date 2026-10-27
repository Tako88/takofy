use super::{UriKind, parse};

fn raw(uri: &str) -> Option<(UriKind, &str, Option<&str>)> {
    parse(uri).map(|parsed| (parsed.kind, parsed.id, parsed.owner))
}

#[test]
fn every_kind_parses_with_the_remainder_as_its_id() {
    assert_eq!(
        raw("spotify:track:4uLU6hMCjMI75M1A2tKUQC"),
        Some((UriKind::Track, "4uLU6hMCjMI75M1A2tKUQC", None))
    );
    assert_eq!(
        raw("spotify:episode:1"),
        Some((UriKind::Episode, "1", None))
    );
    assert_eq!(
        raw("spotify:playlist:abc"),
        Some((UriKind::Playlist, "abc", None))
    );
    assert_eq!(
        raw("spotify:album:abc"),
        Some((UriKind::Album, "abc", None))
    );
    assert_eq!(
        raw("spotify:artist:abc"),
        Some((UriKind::Artist, "abc", None))
    );
    assert_eq!(raw("spotify:show:abc"), Some((UriKind::Show, "abc", None)));
}

#[test]
fn a_colon_in_the_id_is_part_of_the_id() {
    assert_eq!(
        raw("spotify:playlist:abc:extra"),
        Some((UriKind::Playlist, "abc:extra", None))
    );
    assert_eq!(
        raw("spotify:local:Artist:Album:Title:222"),
        Some((UriKind::Local, "Artist:Album:Title:222", None))
    );
}

#[test]
fn an_account_named_playlist_keeps_the_name_and_the_id() {
    assert_eq!(
        raw("spotify:user:alice:playlist:abc"),
        Some((UriKind::Playlist, "abc", Some("alice")))
    );
    assert_eq!(
        raw("spotify:user:alice:playlist:abc:extra"),
        Some((UriKind::Playlist, "abc:extra", Some("alice")))
    );
}

#[test]
fn a_kind_with_no_id_is_not_an_address() {
    for uri in [
        "spotify:track:",
        "spotify:playlist:",
        "spotify:user:alice:playlist:",
        "spotify:playlist",
    ] {
        assert_eq!(raw(uri), None, "must refuse {uri:?}");
    }
}

#[test]
fn an_address_naming_no_drawn_kind_is_not_an_address() {
    for uri in [
        "spotify:user:alice:collection",
        "spotify:user:alice:folder:x",
        "spotify:user:alice:folder:playlist:abc",
        "spotify:user::playlist:abc",
        "spotify:user:",
        "spotify:",
        "spotify:track_extra:abc",
    ] {
        assert_eq!(raw(uri), None, "must refuse {uri:?}");
    }
}

#[test]
fn a_foreign_scheme_is_not_an_address() {
    for uri in ["", "https://i.scdn.co/image/abc", "list:p0", "spotify"] {
        assert_eq!(raw(uri), None, "must refuse {uri:?}");
    }
}
