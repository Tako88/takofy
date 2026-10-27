use super::super::*;
use super::test_support::*;
use crate::message::{Failure, Update};

const OTHER_SHOW: &str = "spotify:show:b";

#[test]
fn loading_draws_a_spinner_and_no_band() {
    let ctx = context();
    let mut library = Library::default();
    let _ = library.open(Page::Show(SHOW.to_owned()));
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    let (_, output) = frame(
        &ctx,
        Vec::new(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    assert!(
        !drawn_texts(&output).contains(&"Lateral"),
        "a loading show draws no band: {:?}",
        drawn_texts(&output)
    );
}

#[test]
fn failed_draws_the_failure_and_a_retry_that_returns_load_show() {
    let ctx = context();
    let mut library = Library::default();
    let _ = library.open(Page::Show(SHOW.to_owned()));
    library.apply(&Update::ShowFailed {
        show: SHOW.to_owned(),
        failure: Failure::Load,
    });
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    let (_, output) = frame(
        &ctx,
        Vec::new(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    let texts = drawn_texts(&output);
    assert!(
        texts.contains(&crate::signin::failure_message(Failure::Load)),
        "the failure's own copy: {texts:?}"
    );

    let retry = text_rect(&output, "Retry").expect("a retry control");
    let command = click(
        &ctx,
        retry.center(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    assert_eq!(
        command,
        Some(Command::LoadShow {
            show: SHOW.to_owned()
        }),
        "Retry re-issues this page's own load"
    );
}

#[test]
fn a_ready_detail_for_another_uri_draws_no_band() {
    let ctx = context();
    let mut library = Library::default();
    let _ = library.open(Page::Show(SHOW.to_owned()));
    let mut other = detail(vec![episode(1)]);
    other.show.uri = OTHER_SHOW.to_owned();
    other.show.name = "Another Show".to_owned();
    library.show = Load::Ready(other);
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    let (_, output) = frame(
        &ctx,
        Vec::new(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    let texts = drawn_texts(&output);
    assert!(
        !texts.contains(&"Another Show"),
        "another show's band must not be painted: {texts:?}"
    );
}

#[test]
fn a_ready_show_draws_name_publisher_and_episode_titles() {
    let ctx = context();
    let mut library = library_with(detail(vec![episode(1), episode(2)]));
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();

    let (_, output) = frame(
        &ctx,
        Vec::new(),
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    let texts = drawn_texts(&output);
    for label in [
        "Lateral",
        "2 episodes · Tom Scott Media",
        "Episode 1",
        "Episode 2",
    ] {
        assert!(
            texts.contains(&label),
            "the page must paint {label}: {texts:?}"
        );
    }
}
