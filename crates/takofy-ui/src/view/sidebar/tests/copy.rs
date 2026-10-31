use super::super::*;
use super::test_support::*;

use takofy_model::Account;

use crate::library::Snapshot;
use crate::message::Update;

fn account() -> Account {
    Account::from_wire("dana").expect("an account")
}

/// A sidebar drawn from last session's copy, whose playlists refresh failed.
fn stale(snapshot: Snapshot) -> Library {
    let mut library = Library::default();
    library.restore(snapshot);
    library.apply(&Update::Account(account()));
    library.apply(&Update::LibraryReady);
    library.apply(&Update::PlaylistFailed(Failure::Load));
    library
}

fn copy_of_playlists(names: &[(&str, &str)]) -> Snapshot {
    let mut snapshot = Snapshot::new(account());
    snapshot.playlists = Some(
        names
            .iter()
            .map(|(uri, name)| playlist(uri, name))
            .collect(),
    );
    snapshot
}

#[test]
fn the_foot_says_the_library_is_last_time_s_after_a_failed_refresh() {
    let ctx = context();
    let mut library = stale(copy_of_playlists(&[("spotify:playlist:a", "A")]));
    let mut art = Art::default();

    let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);

    assert!(
        painted(&output, copy_note()),
        "the foot says the rows are old: {:?}",
        drawn_text(&output)
    );
    assert_eq!(
        painted_rows(&output, &["A"]),
        vec!["A".to_owned()],
        "the rows it describes are still drawn"
    );
}

#[test]
fn a_write_notice_outranks_the_copy_note() {
    let ctx = context();
    let mut library = stale(copy_of_playlists(&[("spotify:playlist:a", "A")]));
    library.sidebar_notice = Some(crate::library::SidebarWrite::Remove);
    let mut art = Art::default();

    let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);

    assert!(painted(
        &output,
        sidebar_notice(crate::library::SidebarWrite::Remove)
    ));
    assert!(
        !painted(&output, copy_note()),
        "one foot line, and the write's is the newer news"
    );
}

#[test]
fn a_live_library_draws_no_foot() {
    let ctx = context();
    let mut library = named_playlists(&["Alpha"]);
    let mut art = Art::default();

    let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);

    assert!(!painted(&output, copy_note()));
}

#[test]
fn a_copy_draws_its_rows_in_every_list() {
    let mut snapshot = Snapshot::new(account());
    snapshot.playlists = Some(vec![playlist("spotify:playlist:a", "A playlist")]);
    snapshot.shows = Some(vec![followed_show("spotify:show:s", "A show")]);
    snapshot.artists = Some(vec![followed_artist("spotify:artist:a", "An artist")]);
    snapshot.albums = Some(vec![saved_album("spotify:album:r", "An album", "Vela")]);
    let mut library = Library::default();
    library.restore(snapshot);
    library.apply(&Update::Account(account()));

    for (kind, name) in [
        (SidebarKind::Playlists, "A playlist"),
        (SidebarKind::Podcasts, "A show"),
        (SidebarKind::Artists, "An artist"),
        (SidebarKind::Albums, "An album"),
    ] {
        let ctx = context();
        assert!(library.choose_kind(kind));
        let mut art = Art::default();

        let (output, _) = run_column(&ctx, Vec::new(), &mut library, &mut art);

        assert_eq!(
            painted_rows(&output, &[name]),
            vec![name.to_owned()],
            "{kind:?} did not draw the copy's row"
        );
    }
}
