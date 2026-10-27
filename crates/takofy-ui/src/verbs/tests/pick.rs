use super::super::*;
use super::test_support::*;

#[test]
fn a_pick_of_tracks_keeps_the_rows_order_and_duplicates() {
    assert_eq!(pick(&tracks(&["spotify:track:a"])), Some(pick_of(&["a"])));
    assert_eq!(
        pick(&tracks(&[
            "spotify:track:a",
            "spotify:track:b",
            "spotify:track:c"
        ])),
        Some(pick_of(&["a", "b", "c"]))
    );
    assert_eq!(
        pick(&tracks(&["spotify:track:a", "spotify:track:a"])),
        Some(pick_of(&["a", "a"])),
        "two rows holding one track both travel"
    );
}

#[test]
fn a_pick_with_anything_but_tracks_is_not_a_pick() {
    assert_eq!(pick(&[]), None, "an empty pick");
    assert!(!pickable(&[]));
    for uri in ["spotify:episode:e", "spotify:local:A:B:C:1000"] {
        assert_eq!(pick(&tracks(&[uri])), None, "{uri}");
        assert_eq!(
            pick(&tracks(&["spotify:track:a", uri])),
            None,
            "a mixed pick is refused whole: {uri}"
        );
        assert_eq!(
            pick(&tracks(&[uri, "spotify:track:a"])),
            None,
            "{uri} in front"
        );
    }
}

#[test]
fn a_pick_is_only_tracks_and_nothing_else() {
    assert!(pickable(&tracks(&["spotify:track:a"])));
    assert!(pickable(&tracks(&["spotify:track:a", "spotify:track:b"])));
    assert!(!pickable(&tracks(&["spotify:episode:e"])));
}
