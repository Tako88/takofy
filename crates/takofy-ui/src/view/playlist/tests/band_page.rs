use super::super::*;
use super::test_support::*;
use crate::message::Update;
use egui::Context;
use takofy_model::{Owner, OwnerName, PlaylistHead};

/// A page whose own read named it, described it, and credited its owner.
fn page_head(name: &str) -> PlaylistHead {
    PlaylistHead {
        name: name.to_owned(),
        description: Some("Windows down, city behind you.".to_owned()),
        owner: Some(Owner::Named(OwnerName::from_wire("alice").expect("a name"))),
        picture: None,
    }
}

fn frame(ctx: &Context, library: &mut Library) -> egui::FullOutput {
    tracklist_frame(ctx, Table::Full, library)
}

#[test]
fn an_unlisted_page_with_a_head_draws_its_band() {
    let ctx = context();
    let mut library = library_headed(vec![track("a", "t0")], page_head("Pasted Win"));
    let output = frame(&ctx, &mut library);
    let texts = drawn_texts(&output);

    assert!(
        texts.contains(&"PLAYLIST"),
        "a page the sidebar does not name still heads itself: {texts:?}"
    );
    assert!(texts.contains(&"Pasted Win"), "{texts:?}");
    assert!(
        texts.contains(&"Windows down, city behind you."),
        "the description comes from the page's own read: {texts:?}"
    );
    assert!(
        texts.iter().any(|text| text.starts_with("Made by alice")),
        "and so does the owner: {texts:?}"
    );
}

#[test]
fn an_unlisted_page_with_no_head_draws_no_band() {
    let ctx = context();
    let mut library = library_with(vec![track("a", "t0")]);
    let output = frame(&ctx, &mut library);
    let texts = drawn_texts(&output);

    assert!(
        !texts.contains(&"PLAYLIST"),
        "nothing names the page, so nothing heads it: {texts:?}"
    );
    assert!(texts.contains(&"t0"), "the rows still draw: {texts:?}");
}

#[test]
fn a_named_entry_overrides_the_head_title() {
    let ctx = context();
    let mut library = library_headed(vec![track("a", "t0")], page_head("From the page"));
    library.apply(&Update::Playlists(vec![playlist(
        "spotify:playlist:a",
        "From the sidebar",
    )]));

    let output = frame(&ctx, &mut library);
    let texts = drawn_texts(&output);
    assert!(texts.contains(&"From the sidebar"), "{texts:?}");
    assert!(
        !texts.contains(&"From the page"),
        "the sidebar's name is the one drawn: {texts:?}"
    );
}

#[test]
fn an_unnamed_entry_draws_the_heads_title_not_its_uri() {
    let ctx = context();
    let mut library = library_headed(vec![track("a", "t0")], page_head("Pasted Win"));
    let mut entry = playlist("spotify:playlist:a", "spotify:playlist:a");
    entry.named = false;
    library.apply(&Update::Playlists(vec![entry]));

    let output = frame(&ctx, &mut library);
    let texts = drawn_texts(&output);
    assert!(
        texts.contains(&"Pasted Win"),
        "the head names the page: {texts:?}"
    );
    assert!(
        !texts.contains(&"spotify:playlist:a"),
        "an address is not a name to draw: {texts:?}"
    );
}

#[test]
fn an_unlisted_pages_menu_offers_neither_rename_nor_remove() {
    let ctx = context();
    let mut library = library_pasted(page_head("Pasted Win"));
    let at = band_more_point(&ctx, &mut library, "Pasted Win");
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    assert_eq!(
        click(&ctx, at, &mut library, &mut transport, &mut queue),
        None
    );

    let request = crate::view::context_menu::take_request(&ctx).expect("the band posted a menu");
    assert_eq!(
        request.listing,
        crate::menu::Listing::Band(LibraryItem {
            kind: SidebarKind::Playlists,
            uri: "spotify:playlist:a".to_owned(),
            name: "Pasted Win".to_owned(),
        }),
        "the menu acts on the page's own read"
    );
    let verbs: Vec<crate::menu::Verb> = request.items.iter().map(|item| item.verb).collect();
    assert!(
        !verbs.contains(&crate::menu::Verb::Rename),
        "no sidebar row carries the access a rename needs: {verbs:?}"
    );
    assert!(
        !verbs.contains(&crate::menu::Verb::RemoveFromLibrary),
        "nothing is listed to unlink: {verbs:?}"
    );
    assert!(
        verbs.contains(&crate::menu::Verb::SaveToLibrary),
        "and the link is what the menu offers instead: {verbs:?}"
    );
}
