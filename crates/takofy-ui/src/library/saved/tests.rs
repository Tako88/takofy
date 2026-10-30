use takofy_model::PlaylistEntry;

use super::*;

/// The older playlist shape, whose second segment is the owner rather than
/// the kind.
const ACCOUNT: &str = "spotify:user:someone:playlist:4lRh9XA5hFPyKVnThKW3mi";

fn playlist(uri: &str) -> PlaylistEntry {
    PlaylistEntry {
        saved_at: None,
        composed: Vec::new(),
        uri: uri.to_owned(),
        name: "A list".to_owned(),
        named: true,
        cover: None,
        description: None,
        owner: None,
        tracks: None,
        access: None,
    }
}

fn lib(playlists: Vec<PlaylistEntry>) -> Library {
    Library {
        playlists: Load::Ready(playlists),
        ..Library::default()
    }
}

#[test]
fn an_account_named_playlist_reads_as_saved_when_its_list_holds_it() {
    let library = lib(vec![playlist(ACCOUNT)]);

    assert_eq!(library.is_saved(ACCOUNT), Some(true));
}

#[test]
fn an_account_named_playlist_reads_as_unsaved_when_its_ready_list_lacks_it() {
    let library = lib(Vec::new());

    assert_eq!(library.is_saved(ACCOUNT), Some(false));
}

#[test]
fn a_saved_echo_for_an_account_named_playlist_is_remembered() {
    let mut library = lib(Vec::new());
    library.sidebar_pending = Some(SidebarPending::Saved {
        uri: ACCOUNT.to_owned(),
        saved: true,
    });

    library.end_saved(ACCOUNT, true);

    assert_eq!(library.sidebar_pending, None);
    assert_eq!(library.is_saved(ACCOUNT), Some(true));
}

#[test]
fn an_echo_with_no_write_in_flight_leaves_the_list_answer() {
    let mut library = lib(Vec::new());

    library.end_saved(ACCOUNT, true);

    assert_eq!(library.is_saved(ACCOUNT), Some(false));
}
