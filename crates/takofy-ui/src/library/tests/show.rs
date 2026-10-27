use super::super::*;
use super::test_support::*;
use takofy_model::Show;

fn show_command(uri: &str) -> Command {
    Command::LoadShow {
        show: uri.to_owned(),
    }
}

#[test]
fn a_show_page_holds_open_the_episode_count_its_listing_names() {
    let mut library = Library {
        followed: Load::Ready(vec![FollowedShow {
            uri: "spotify:show:a".to_owned(),
            name: "Field Notes".to_owned(),
            named: true,
            publisher: None,
            episodes: Some(12),
            cover: None,
        }]),
        ..Library::default()
    };
    library.open(Page::Show("spotify:show:a".to_owned()));
    assert_eq!(library.selected_show_episodes(), Some(12));

    library.open(Page::Show("spotify:show:b".to_owned()));
    assert_eq!(
        library.selected_show_episodes(),
        None,
        "another show's count is not this page's"
    );

    library.followed = Load::Loading;
    library.open(Page::Show("spotify:show:a".to_owned()));
    assert_eq!(library.selected_show_episodes(), None);
}

#[test]
fn open_a_show_starts_its_load_and_keeps_the_album() {
    let mut library = Library::default();
    library.open(album("spotify:album:a"));
    library.apply(&Update::Album(album_detail("spotify:album:a")));

    assert_eq!(
        library.open(show("spotify:show:s")),
        Some(show_command("spotify:show:s"))
    );
    assert_eq!(library.page(), Some(&show("spotify:show:s")));
    assert_eq!(library.show, Load::Loading);
    assert_eq!(
        library.album,
        Load::Ready(album_detail("spotify:album:a")),
        "the album behind the show stays ready for Back"
    );
}

#[test]
fn a_show_reply_lands_only_on_its_own_want() {
    let mut library = Library::default();
    library.open(show("spotify:show:a"));
    library.apply(&Update::Show(show_detail("spotify:show:a")));

    assert_eq!(library.show, Load::Ready(show_detail("spotify:show:a")));

    library.apply(&Update::Show(show_detail("spotify:show:b")));
    assert_eq!(
        library.show,
        Load::Ready(show_detail("spotify:show:a")),
        "a reply nobody asked for is discarded"
    );
}

#[test]
fn a_stale_show_reply_is_discarded() {
    let mut library = Library::default();
    library.open(show("spotify:show:a"));
    library.open(show("spotify:show:b"));

    library.apply(&Update::Show(show_detail("spotify:show:a")));

    assert_eq!(library.show, Load::Loading, "A's reply is not B's");
    library.apply(&Update::Show(show_detail("spotify:show:b")));
    assert_eq!(library.show, Load::Ready(show_detail("spotify:show:b")));
}

#[test]
fn a_stale_show_failure_is_discarded() {
    let mut library = Library::default();
    library.open(show("spotify:show:a"));
    library.open(show("spotify:show:b"));

    library.apply(&Update::ShowFailed {
        show: "spotify:show:a".to_owned(),
        failure: Failure::Load,
    });
    assert_eq!(library.show, Load::Loading, "A's failure is not B's");

    library.apply(&Update::ShowFailed {
        show: "spotify:show:b".to_owned(),
        failure: Failure::Load,
    });
    assert_eq!(library.show, Load::Failed(Failure::Load));
}

#[test]
fn the_first_show_reply_wins() {
    let mut library = Library::default();
    library.open(show("spotify:show:a"));
    let first = ShowDetail {
        show: Show {
            name: "First".to_owned(),
            ..show_detail("spotify:show:a").show
        },
        ..show_detail("spotify:show:a")
    };
    let second = ShowDetail {
        show: Show {
            name: "Second".to_owned(),
            ..show_detail("spotify:show:a").show
        },
        ..show_detail("spotify:show:a")
    };

    library.apply(&Update::Show(first.clone()));
    library.apply(&Update::Show(second));

    assert_eq!(library.show, Load::Ready(first));
}

#[test]
fn back_onto_a_ready_show_needs_no_command() {
    let mut library = Library::default();
    library.open(show("spotify:show:a"));
    library.apply(&Update::Show(show_detail("spotify:show:a")));
    assert_eq!(
        library.open(album("spotify:album:a")),
        Some(Command::LoadAlbum {
            album: "spotify:album:a".to_owned()
        }),
        "the album page starts its own load"
    );

    assert_eq!(library.back(), None, "a ready show is already settled");
    assert_eq!(library.page(), Some(&show("spotify:show:a")));
}

#[test]
fn back_onto_a_failed_show_reloads_it() {
    let mut library = Library::default();
    library.open(show("spotify:show:a"));
    library.apply(&Update::ShowFailed {
        show: "spotify:show:a".to_owned(),
        failure: Failure::Load,
    });
    assert_eq!(
        library.open(artist("spotify:artist:a")),
        Some(artist_command("spotify:artist:a"))
    );

    assert_eq!(
        library.back(),
        Some(show_command("spotify:show:a")),
        "a failed show has nothing to draw, so Back asks again"
    );
    assert_eq!(library.show, Load::Loading);
}

#[test]
fn reopening_a_failed_show_retries_it() {
    let mut library = Library::default();
    library.open(show("spotify:show:a"));
    library.apply(&Update::ShowFailed {
        show: "spotify:show:a".to_owned(),
        failure: Failure::Load,
    });

    assert_eq!(
        library.open(show("spotify:show:a")),
        Some(show_command("spotify:show:a")),
        "the page's Retry is this call, so it must re-issue the load"
    );
    assert_eq!(library.show, Load::Loading);
}

/// Asks the search state for the read this frame wants, the way `app.rs`
/// does when `logic` runs: the page's query is copied out first.

#[test]
fn to_playlists_clears_the_show_want_and_drops_an_unsettled_load() {
    let mut loading = Library::default();
    loading.open(show("spotify:show:a"));
    loading.to_playlists();
    assert_eq!(loading.show, Load::Idle, "a load nobody wants is dropped");
    assert_eq!(
        loading.open(show("spotify:show:a")),
        Some(show_command("spotify:show:a")),
        "the want was cleared, so the show is asked for again"
    );

    let mut failed = Library::default();
    failed.open(show("spotify:show:a"));
    failed.apply(&Update::ShowFailed {
        show: "spotify:show:a".to_owned(),
        failure: Failure::Load,
    });
    failed.to_playlists();
    assert_eq!(failed.show, Load::Idle);

    let mut ready = Library::default();
    ready.open(show("spotify:show:a"));
    ready.apply(&Update::Show(show_detail("spotify:show:a")));
    ready.to_playlists();
    assert_eq!(
        ready.show,
        Load::Ready(show_detail("spotify:show:a")),
        "a settled show is kept for the page Back returns to"
    );
    assert_eq!(
        ready.open(show("spotify:show:a")),
        Some(show_command("spotify:show:a")),
        "the want is cleared even when the show stays ready"
    );
}

#[test]
fn a_playlist_prune_leaves_the_show_alone() {
    let mut library = Library::default();
    library.select("a");
    library.open(show("spotify:show:a"));
    library.apply(&Update::Show(show_detail("spotify:show:a")));

    // The list no longer holds the playlist, so its page is pruned.
    library.apply(&Update::Playlists(vec![playlist("b", "B")]));

    assert_eq!(library.page(), Some(&show("spotify:show:a")));
    assert_eq!(
        library.show,
        Load::Ready(show_detail("spotify:show:a")),
        "a playlist prune never touches the show"
    );
    assert_eq!(library.current_show(), Some(&show_detail("spotify:show:a")));
}

#[test]
fn current_show_is_none_while_loading_or_for_another_uri() {
    let mut library = Library::default();
    library.open(show("spotify:show:b"));
    assert_eq!(library.current_show(), None, "loading names nothing");

    library.show = Load::Ready(show_detail("spotify:show:a"));
    assert_eq!(
        library.current_show(),
        None,
        "A's detail must not name B's page"
    );

    let mut loaded = Library::default();
    loaded.open(show("spotify:show:a"));
    loaded.apply(&Update::Show(show_detail("spotify:show:a")));
    assert_eq!(
        loaded.current_show(),
        Some(&show_detail("spotify:show:a")),
        "a detail that names the page is drawn"
    );
}
