use super::Snapshot;
use takofy_model::{Account, FollowedArtist, FollowedShow, LikedTrack, PlaylistEntry, SavedAlbum};

fn account() -> Account {
    Account::from_wire("tako").expect("a name")
}

fn playlist() -> PlaylistEntry {
    PlaylistEntry {
        uri: "spotify:playlist:a".to_owned(),
        name: "A".to_owned(),
        named: true,
        cover: None,
        composed: Vec::new(),
        description: None,
        tracks: None,
        owner: None,
        access: None,
        saved_at: None,
    }
}

fn show() -> FollowedShow {
    FollowedShow {
        uri: "spotify:show:a".to_owned(),
        name: "A".to_owned(),
        named: true,
        publisher: None,
        episodes: None,
        cover: None,
    }
}

fn artist() -> FollowedArtist {
    FollowedArtist {
        uri: "spotify:artist:a".to_owned(),
        name: "A".to_owned(),
        named: true,
        portrait: None,
        releases: None,
    }
}

fn album() -> SavedAlbum {
    SavedAlbum {
        uri: "spotify:album:a".to_owned(),
        name: "A".to_owned(),
        named: true,
        artists: String::new(),
        artist_uris: Vec::new(),
        artist_names: Vec::new(),
        year: None,
        cover: None,
        tracks: None,
        saved_at: None,
    }
}

fn liked() -> LikedTrack {
    LikedTrack {
        uri: "spotify:track:a".to_owned(),
        saved_at: None,
    }
}

#[test]
fn a_new_copy_holds_no_list_and_has_no_rows() {
    let snapshot = Snapshot::new(account());
    assert_eq!(snapshot.account, account());
    assert_eq!(snapshot.playlists, None);
    assert_eq!(snapshot.shows, None);
    assert_eq!(snapshot.artists, None);
    assert_eq!(snapshot.albums, None);
    assert_eq!(snapshot.liked, None);
    assert!(!snapshot.has_rows());
}

#[test]
fn an_empty_list_is_not_a_row() {
    let mut snapshot = Snapshot::new(account());
    snapshot.playlists = Some(Vec::new());
    snapshot.shows = Some(Vec::new());
    snapshot.artists = Some(Vec::new());
    snapshot.albums = Some(Vec::new());
    snapshot.liked = Some(Vec::new());
    assert!(!snapshot.has_rows());
}

#[test]
fn a_row_in_any_list_gives_the_copy_something_to_write() {
    let mut snapshot = Snapshot::new(account());
    snapshot.playlists = Some(vec![playlist()]);
    assert!(snapshot.has_rows());

    let mut snapshot = Snapshot::new(account());
    snapshot.shows = Some(vec![show()]);
    assert!(snapshot.has_rows());

    let mut snapshot = Snapshot::new(account());
    snapshot.artists = Some(vec![artist()]);
    assert!(snapshot.has_rows());

    let mut snapshot = Snapshot::new(account());
    snapshot.albums = Some(vec![album()]);
    assert!(snapshot.has_rows());

    let mut snapshot = Snapshot::new(account());
    snapshot.liked = Some(vec![liked()]);
    assert!(snapshot.has_rows());
}
