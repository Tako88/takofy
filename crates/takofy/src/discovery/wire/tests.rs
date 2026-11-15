//! The parsers pinned to synthetic bodies in the measured shapes.

use super::{
    AlbumHit, ArtistHit, WireError, albums, genre_table, has_error, related_artists,
    search_artists, top_titles,
};

const SEARCH: &str = include_str!("../fixtures/search_artist.json");
const TOP: &str = include_str!("../fixtures/top.json");
const RELATED: &str = include_str!("../fixtures/related.json");
const ALBUMS: &str = include_str!("../fixtures/albums.json");
const GENRE: &str = include_str!("../fixtures/genre.json");

const QUOTA_ERROR: &str = r#"{"error":{"type":"DataException","message":"no data","code":800}}"#;
const HTML: &str = "<html><body><h1>Service unavailable</h1></body></html>";

#[test]
fn the_search_body_names_its_artists() {
    assert_eq!(
        search_artists(SEARCH.as_bytes()),
        Ok(vec![
            ArtistHit {
                id: 1001,
                name: "Vela".to_owned()
            },
            ArtistHit {
                id: 1002,
                name: "Vela".to_owned()
            },
        ])
    );
}

#[test]
fn the_related_body_names_its_artists() {
    assert_eq!(
        related_artists(RELATED.as_bytes()),
        Ok(vec![
            ArtistHit {
                id: 2002,
                name: "Aphex Twin".to_owned()
            },
            ArtistHit {
                id: 2003,
                name: "Seefeel".to_owned()
            },
            ArtistHit {
                id: 2004,
                name: "Clark".to_owned()
            },
        ])
    );
}

#[test]
fn the_top_body_names_its_tracks() {
    assert_eq!(
        top_titles(TOP.as_bytes()),
        Ok(vec!["Signal Fire".to_owned(), "Night Watch".to_owned()])
    );
}

#[test]
fn the_album_body_keeps_the_genre_and_the_date() {
    assert_eq!(
        albums(ALBUMS.as_bytes()),
        Ok(vec![
            AlbumHit {
                title: "Night Watch".to_owned(),
                genre_id: Some(3),
                release_date: Some("2024-03-15".to_owned())
            },
            AlbumHit {
                title: "Day Break".to_owned(),
                genre_id: Some(3),
                release_date: Some("2022-00-00".to_owned())
            },
            AlbumHit {
                title: "Quiet".to_owned(),
                genre_id: Some(11),
                release_date: Some("2019-08-01".to_owned())
            },
            AlbumHit {
                title: "Collected".to_owned(),
                genre_id: Some(0),
                release_date: Some("2017-01-01".to_owned())
            },
        ])
    );
}

#[test]
fn the_genre_table_keeps_the_service_order() {
    let table = genre_table(GENRE.as_bytes()).expect("a synthetic table");
    assert_eq!(table.entries().len(), 22);
    assert_eq!(table.name_at(0), Some("All"));
    assert_eq!(table.name_at(2), Some("Rap/Hip Hop"));
    assert_eq!(table.index_of(10), Some(10));
    assert_eq!(table.index_of(99), None);
}

#[test]
fn an_error_object_is_not_an_empty_list() {
    for body in [QUOTA_ERROR, HTML] {
        assert_eq!(search_artists(body.as_bytes()), Err(WireError));
        assert_eq!(related_artists(body.as_bytes()), Err(WireError));
        assert_eq!(top_titles(body.as_bytes()), Err(WireError));
        assert_eq!(albums(body.as_bytes()), Err(WireError));
        assert_eq!(genre_table(body.as_bytes()), Err(WireError));
    }
}

#[test]
fn a_body_without_a_data_array_is_an_error() {
    let body = r#"{"next":null,"total":0}"#;
    assert_eq!(top_titles(body.as_bytes()), Err(WireError));
}

#[test]
fn an_empty_data_array_is_a_valid_empty_list() {
    let body = r#"{"data":[],"next":null,"total":0}"#;
    assert_eq!(search_artists(body.as_bytes()), Ok(Vec::new()));
    assert_eq!(related_artists(body.as_bytes()), Ok(Vec::new()));
    assert_eq!(top_titles(body.as_bytes()), Ok(Vec::new()));
    assert_eq!(albums(body.as_bytes()), Ok(Vec::new()));
    assert_eq!(
        genre_table(body.as_bytes()),
        Ok(super::GenreTable::default())
    );
}

#[test]
fn the_error_object_is_recognised_without_failing_the_json() {
    assert!(has_error(QUOTA_ERROR.as_bytes()));
    assert!(!has_error(TOP.as_bytes()));
    assert!(!has_error(HTML.as_bytes()));
}

#[test]
fn entries_with_a_missing_or_blank_field_are_skipped() {
    let body = r#"{"data":[
        {"id":1,"name":"Kept"},
        {"id":2},
        {"name":"No id"},
        {"id":3,"name":"   "},
        {"id":"four","name":"Wrong type"}
    ]}"#;
    assert_eq!(
        search_artists(body.as_bytes()),
        Ok(vec![ArtistHit {
            id: 1,
            name: "Kept".to_owned()
        }])
    );
}
