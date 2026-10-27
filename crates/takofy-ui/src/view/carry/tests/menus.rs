use egui::{Rect, pos2, vec2};

use super::super::*;
use super::test_support::*;
use crate::player::Transport;

fn door() -> Door {
    Door {
        anchor: crate::menu::Anchor::Button(Rect::from_min_size(pos2(0.0, 0.0), vec2(28.0, 28.0))),
        opener: Id::unique("more"),
        device: crate::menu::Device::Pointer,
    }
}

fn entry_at(row: usize) -> Option<TrackEntry> {
    Some(entry(&row.to_string()))
}

fn opened(likes: &Likes) -> Open<'_> {
    Open {
        listing: Listing::Page,
        here: None,
        likes,
    }
}

fn uris(request: &Request) -> Vec<&str> {
    request
        .targets
        .iter()
        .map(|entry| entry.uri.as_str())
        .collect()
}

#[test]
fn menu_inside_a_pick_acts_on_the_pick_and_keeps_it() {
    let ctx = context();
    let id = Id::unique("list");
    let mut list = List::open(&ctx, id, 4);
    list.press(1, Press::Replace);
    list.press(3, Press::Toggle);
    list.menu(
        &ctx,
        door(),
        3,
        Gate::default(),
        opened(&Likes::default()),
        entry_at,
    );
    list.finish(&ctx);

    let request = context_menu::take_request(&ctx).expect("a request was posted");
    assert_eq!(
        uris(&request),
        vec!["u1", "u3"],
        "the whole pick, ascending"
    );
    let kept = List::open(&ctx, id, 4);
    assert!(
        kept.look(1).selected && kept.look(3).selected,
        "the pick was collapsed"
    );
}

#[test]
fn menu_outside_selects_first() {
    let ctx = context();
    let id = Id::unique("list");
    let mut list = List::open(&ctx, id, 4);
    list.press(1, Press::Replace);
    list.press(3, Press::Toggle);
    list.menu(
        &ctx,
        door(),
        2,
        Gate::default(),
        opened(&Likes::default()),
        entry_at,
    );
    list.finish(&ctx);

    let request = context_menu::take_request(&ctx).expect("a request was posted");
    assert_eq!(uris(&request), vec!["u2"], "the row under the menu, alone");
    let kept = List::open(&ctx, id, 4);
    assert!(kept.look(2).selected, "the row is picked");
    assert!(!kept.look(1).selected, "and nothing else is");
}

#[test]
fn menu_is_refused_while_carrying_or_writing_and_selects_nothing() {
    for gate in [
        Gate {
            carrying: true,
            writing: false,
        },
        Gate {
            carrying: false,
            writing: true,
        },
    ] {
        let ctx = context();
        let id = Id::unique("list");
        let mut list = List::open(&ctx, id, 4);
        list.press(1, Press::Replace);
        list.menu(&ctx, door(), 3, gate, opened(&Likes::default()), entry_at);
        list.finish(&ctx);

        assert_eq!(context_menu::take_request(&ctx), None, "{gate:?}");
        let kept = List::open(&ctx, id, 4);
        assert!(kept.look(1).selected, "{gate:?}");
        assert!(
            !kept.look(3).selected,
            "{gate:?}: the refused open picked a row"
        );
    }
}

#[test]
fn menu_refuses_a_pick_it_cannot_answer_for() {
    let ctx = context();
    let id = Id::unique("list");
    let mut list = List::open(&ctx, id, 4);
    list.press(1, Press::Replace);
    list.press(3, Press::Toggle);
    list.menu(
        &ctx,
        door(),
        3,
        Gate::default(),
        opened(&Likes::default()),
        |row| (row != 3).then(|| entry(&row.to_string())),
    );
    list.finish(&ctx);

    assert_eq!(context_menu::take_request(&ctx), None);
    let kept = List::open(&ctx, id, 4);
    assert!(
        kept.look(1).selected && kept.look(3).selected,
        "a pick refused whole still changed the pick"
    );
}

#[test]
fn a_posted_request_is_taken_once() {
    let ctx = context();
    let request = Request {
        door: door(),
        playlists: Vec::new(),
        items: crate::verbs::track_items(&[entry("0")], None, &Listing::Page, Standing::Unknown),
        targets: vec![entry("0")],
        rows: vec![0],
        listing: Listing::Page,
        page: Some(Page::Playlist("spotify:playlist:a".to_owned())),
    };
    context_menu::post(&ctx, request.clone());
    assert_eq!(context_menu::take_request(&ctx), Some(request));
    assert_eq!(
        context_menu::take_request(&ctx),
        None,
        "a request is read once"
    );
}

#[test]
fn request_rows_and_targets_stay_parallel() {
    let ctx = context();
    let id = Id::unique("list");
    let mut list = List::open(&ctx, id, 6);
    list.press(1, Press::Replace);
    list.press(3, Press::Toggle);
    list.press(4, Press::Toggle);
    list.menu(
        &ctx,
        door(),
        3,
        Gate::default(),
        Open {
            listing: Listing::Queue,
            here: None,
            likes: &Likes::default(),
        },
        entry_at,
    );
    list.finish(&ctx);

    let request = context_menu::take_request(&ctx).expect("a request was posted");
    assert_eq!(request.rows, vec![1, 3, 4]);
    assert_eq!(uris(&request), vec!["u1", "u3", "u4"]);
    assert_eq!(request.listing, Listing::Queue);
}

#[test]
fn queue_menu_posts_a_queue_request_and_keeps_the_pick() {
    let ctx = context();
    let mut queue = queue_of(6, 0);
    queue.press(2, Press::Replace);
    queue.press(4, Press::Toggle);

    queue_menu(
        &mut queue,
        &ctx,
        door(),
        2,
        Gate::default(),
        &Likes::default(),
        None,
    );

    let request = context_menu::take_request(&ctx).expect("a request was posted");
    assert_eq!(request.rows, vec![2, 4]);
    assert_eq!(uris(&request), vec!["u2", "u4"]);
    assert_eq!(request.listing, Listing::Queue);
    assert_eq!(
        request.items.last().map(|item| item.verb),
        Some(crate::menu::Verb::Remove)
    );
    assert!(
        request
            .items
            .iter()
            .all(|item| item.verb != crate::menu::Verb::AddToQueue),
        "the queue offered to add what is already there"
    );
    assert_eq!(
        queue.acting_rows(None),
        vec![2, 4],
        "the pick was collapsed"
    );
}

#[test]
fn queue_menu_is_refused_by_the_gate_and_changes_no_pick() {
    for gate in [
        Gate {
            carrying: true,
            writing: false,
        },
        Gate {
            carrying: false,
            writing: true,
        },
    ] {
        let ctx = context();
        let mut queue = queue_of(6, 0);
        queue.press(2, Press::Replace);

        queue_menu(&mut queue, &ctx, door(), 4, gate, &Likes::default(), None);

        assert_eq!(context_menu::take_request(&ctx), None, "{gate:?}");
        assert_eq!(queue.acting_rows(None), vec![2], "{gate:?}");
    }
}

#[test]
fn queue_menu_on_the_current_entry_posts_nothing() {
    let ctx = context();
    let mut queue = queue_of(4, 1);

    queue_menu(
        &mut queue,
        &ctx,
        door(),
        1,
        Gate::default(),
        &Likes::default(),
        None,
    );

    assert_eq!(context_menu::take_request(&ctx), None);
}

#[test]
fn queue_menu_on_a_pick_the_queue_has_passed_posts_nothing() {
    let ctx = context();
    let mut queue = queue_of(6, 1);
    queue.press(2, Press::Replace);
    let _ = queue.next(&mut Transport::default());

    queue_menu(
        &mut queue,
        &ctx,
        door(),
        2,
        Gate::default(),
        &Likes::default(),
        None,
    );

    assert_eq!(
        context_menu::take_request(&ctx),
        None,
        "a pick the queue has passed was offered"
    );
}

#[test]
fn queue_cut_posts_the_pick_for_the_queue() {
    let ctx = context();
    let mut queue = queue_of(5, 0);
    queue.press(1, Press::Replace);
    queue.press(3, Press::Toggle);

    queue_cut(&mut queue, &ctx, 3, Gate::default());

    let cut = context_menu::take_cut(&ctx).expect("a cut was posted");
    assert_eq!(cut.listing, Listing::Queue);
    assert_eq!(cut.rows, vec![1, 3]);
    assert_eq!(cut.entries.len(), 2);
}

#[test]
fn queue_cut_is_refused_by_the_gate() {
    let ctx = context();
    let mut queue = queue_of(4, 0);
    queue.press(1, Press::Replace);

    queue_cut(
        &mut queue,
        &ctx,
        2,
        Gate {
            carrying: false,
            writing: true,
        },
    );

    assert_eq!(context_menu::take_cut(&ctx), None);
    assert_eq!(queue.acting_rows(None), vec![1], "the pick changed");
}

/// A track with a page to open and a link to copy, which is an entry the
/// menu has verbs for.
fn named_entry(uri: &str) -> TrackEntry {
    TrackEntry {
        uri: uri.to_owned(),
        album_uri: Some("spotify:album:a".to_owned()),
        artist_uris: vec!["spotify:artist:b".to_owned()],
        artist_names: vec!["b".to_owned()],
        ..entry("named")
    }
}

#[test]
fn current_menu_posts_the_playing_entry_with_no_removal() {
    let ctx = context();
    let mut transport = Transport::default();
    let named = named_entry("spotify:track:a");
    let mut queue = Queue::default();
    queue.start(std::slice::from_ref(&named), 0, None, &mut transport);

    current_menu(
        queue.current(),
        &ctx,
        door(),
        Gate::default(),
        &Likes::default(),
        None,
    );

    let request = context_menu::take_request(&ctx).expect("a request was posted");
    assert_eq!(request.listing, Listing::Current);
    assert_eq!(request.targets, vec![named]);
    assert_eq!(request.rows.len(), request.targets.len(), "not parallel");
    assert!(
        request.items.iter().all(|item| {
            item.verb != crate::menu::Verb::AddToQueue && item.verb != crate::menu::Verb::Remove
        }),
        "the current entry offered a list command"
    );
    assert!(
        request
            .items
            .iter()
            .any(|item| item.verb == crate::menu::Verb::GoToAlbum)
    );
}

#[test]
fn current_menu_with_no_current_entry_or_no_verbs_posts_nothing() {
    let ctx = context();
    current_menu(None, &ctx, door(), Gate::default(), &Likes::default(), None);
    assert_eq!(context_menu::take_request(&ctx), None, "an empty queue");

    let mut transport = Transport::default();
    let mut queue = Queue::default();
    queue.start(&[entry("0")], 0, None, &mut transport);
    current_menu(
        queue.current(),
        &ctx,
        door(),
        Gate::default(),
        &Likes::default(),
        None,
    );
    assert_eq!(
        context_menu::take_request(&ctx),
        None,
        "an entry with no verbs"
    );
}

#[test]
fn current_menu_is_refused_by_the_gate() {
    let ctx = context();
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    queue.start(&[named_entry("spotify:track:a")], 0, None, &mut transport);

    current_menu(
        queue.current(),
        &ctx,
        door(),
        Gate {
            carrying: true,
            writing: false,
        },
        &Likes::default(),
        None,
    );

    assert_eq!(context_menu::take_request(&ctx), None);
}
