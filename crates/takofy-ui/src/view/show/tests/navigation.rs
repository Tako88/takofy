use egui::Rect;

use super::super::*;
use super::test_support::*;

/// The box an episode row's byline paints the show's name in.
fn byline(output: &egui::FullOutput) -> Rect {
    text_rect(output, "LATERAL").expect("the byline paints the show's name")
}

#[test]
fn an_episode_byline_click_opens_the_show_page() {
    let ctx = context();
    let mut library = library_with(detail(vec![episode(1), episode(2)]));
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    let first = frame(
        &ctx,
        Vec::new(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    )
    .1;
    let command = click(
        &ctx,
        byline(&first).center(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );

    assert_eq!(
        command, None,
        "the show is already open, so its own link answers nothing"
    );
    assert_eq!(
        library.page(),
        Some(&Page::Show(SHOW.to_owned())),
        "the byline is the show's link, not a press on the row"
    );
    assert!(
        ctx.memory(|memory| memory.focused()).is_none(),
        "a link takes no focus, so the row was never pressed"
    );
    library.back();
    assert_eq!(
        library.page(),
        None,
        "the click grew no history between the root and the show"
    );
}

#[test]
fn a_show_open_on_its_own_page_changes_no_history() {
    let mut library = library_with(detail(vec![episode(1)]));
    assert_eq!(library.page(), Some(&Page::Show(SHOW.to_owned())));

    assert_eq!(
        library.open(Page::Show(SHOW.to_owned())),
        None,
        "a settled page asks for nothing"
    );

    assert_eq!(
        library.page(),
        Some(&Page::Show(SHOW.to_owned())),
        "the page is still the show"
    );
    library.back();
    assert_eq!(
        library.page(),
        None,
        "one step back reaches the root, so the open pushed nothing"
    );
}
