use super::{StationError, radio_rows, radio_uri};
use crate::context::PageTrack;
use takofy_model::home::{SeedKind, StationSeed};
use takofy_model::uri::UriKind;

/// Each seed kind with the URI kind it names and that kind's segment.
const SEEDS: [(SeedKind, UriKind, &str); 4] = [
    (SeedKind::Song, UriKind::Track, "track"),
    (SeedKind::Playlist, UriKind::Playlist, "playlist"),
    (SeedKind::Album, UriKind::Album, "album"),
    (SeedKind::Artist, UriKind::Artist, "artist"),
];

fn seed(kind: SeedKind, uri: &str) -> StationSeed {
    StationSeed {
        kind,
        uri: uri.to_owned(),
    }
}

fn positions(uris: &[&str]) -> Vec<PageTrack> {
    uris.iter()
        .map(|uri| PageTrack {
            uri: (*uri).to_owned(),
            added_at: None,
        })
        .collect()
}

#[test]
fn a_song_seed_names_the_track_station() {
    assert_eq!(
        radio_uri(&seed(SeedKind::Song, "spotify:track:abc")),
        Some("spotify:station:track:abc".to_owned())
    );
}

#[test]
fn each_seed_names_its_own_station() {
    let cases = [
        (
            SeedKind::Song,
            "spotify:track:abc",
            "spotify:station:track:abc",
        ),
        (
            SeedKind::Playlist,
            "spotify:playlist:abc",
            "spotify:station:playlist:abc",
        ),
        (
            SeedKind::Album,
            "spotify:album:abc",
            "spotify:station:album:abc",
        ),
        (
            SeedKind::Artist,
            "spotify:artist:abc",
            "spotify:station:artist:abc",
        ),
        (
            SeedKind::Playlist,
            "spotify:user:u:playlist:abc",
            "spotify:station:playlist:abc",
        ),
    ];
    for (kind, uri, station) in cases {
        assert_eq!(
            radio_uri(&seed(kind, uri)),
            Some(station.to_owned()),
            "{kind:?} {uri}"
        );
    }
}

#[test]
fn a_seed_whose_uri_names_another_kind_names_no_station() {
    let mut pairs = 0;
    for (kind, own, _) in SEEDS {
        for (_, other, segment) in SEEDS {
            if other == own {
                continue;
            }
            pairs += 1;
            assert_eq!(
                radio_uri(&seed(kind, &format!("spotify:{segment}:abc"))),
                None,
                "{kind:?} seed with a {segment} uri"
            );
        }
    }
    assert_eq!(pairs, 12, "the matrix must cover every off-diagonal pair");

    for (kind, _, _) in SEEDS {
        for segment in ["show", "episode"] {
            assert_eq!(
                radio_uri(&seed(kind, &format!("spotify:{segment}:abc"))),
                None,
                "{kind:?} seed with a {segment} uri"
            );
        }
    }
}

#[test]
fn an_empty_or_unparseable_uri_names_no_station() {
    for (kind, _, _) in SEEDS {
        for uri in ["spotify:track:", "not a uri"] {
            assert_eq!(radio_uri(&seed(kind, uri)), None, "{kind:?} {uri}");
        }
    }
}

#[test]
fn an_id_carrying_a_colon_is_not_a_station_id() {
    for (kind, _, segment) in SEEDS {
        assert_eq!(
            radio_uri(&seed(kind, &format!("spotify:{segment}:a:b"))),
            None,
            "{kind:?} seed"
        );
    }
}

#[test]
fn radio_rows_keeps_the_station_order_and_its_repeats() {
    assert_eq!(
        radio_rows(positions(&["a", "b", "a"])).expect("rows"),
        vec!["a", "b", "a"]
    );
}

#[test]
fn radio_rows_keeps_the_seed_where_the_station_put_it() {
    let seed = "spotify:track:abc";
    assert_eq!(
        radio_rows(positions(&["spotify:track:x", seed])).expect("rows"),
        vec!["spotify:track:x", seed]
    );
}

#[test]
fn a_station_with_no_rows_is_not_a_station() {
    assert!(matches!(radio_rows(Vec::new()), Err(StationError::Empty)));
}
