use super::{StationError, radio_rows, radio_uri};
use crate::context::PageTrack;
use takofy_model::home::{SeedKind, StationSeed};

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
fn a_seed_of_another_kind_names_no_station() {
    for kind in [SeedKind::Playlist, SeedKind::Artist, SeedKind::Album] {
        assert_eq!(
            radio_uri(&seed(kind, "spotify:track:abc")),
            None,
            "{kind:?} seeds no track radio"
        );
    }
}

#[test]
fn a_song_seed_naming_no_track_names_no_station() {
    assert_eq!(radio_uri(&seed(SeedKind::Song, "spotify:album:abc")), None);
    assert_eq!(radio_uri(&seed(SeedKind::Song, "spotify:track:")), None);
    assert_eq!(radio_uri(&seed(SeedKind::Song, "not a uri")), None);
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
