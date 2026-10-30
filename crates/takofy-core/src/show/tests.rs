use std::collections::HashMap;

use librespot::core::{SpotifyId, SpotifyUri};
use librespot::metadata::Show;
use librespot::protocol::metadata::Episode as EpisodeMessage;
use librespot::protocol::metadata::Show as ShowMessage;
use takofy_model::Episode;

use super::{detail, details, episode_uris};

/// A 16-byte gid, the only length `SpotifyId::from_raw` accepts.
fn gid(byte: u8) -> Vec<u8> {
    vec![byte; 16]
}

/// A protocol show whose episodes are named by their gid byte.
fn wire_show(name: &str, publisher: &str, episodes: &[u8]) -> ShowMessage {
    let mut show = ShowMessage::default();
    show.set_gid(gid(1));
    show.set_name(name.to_owned());
    show.set_publisher(publisher.to_owned());
    for byte in episodes {
        let mut episode = EpisodeMessage::default();
        episode.set_gid(gid(*byte));
        show.episode.push(episode);
    }
    show
}

/// The parsed show the wire's message converts to.
fn show(name: &str, publisher: &str, episodes: &[u8]) -> Show {
    Show::try_from(&wire_show(name, publisher, episodes)).expect("a valid show message")
}

/// The URI the wire's gid byte resolves to.
fn uri(byte: u8) -> String {
    SpotifyUri::Episode {
        id: SpotifyId::from_raw(&gid(byte)).expect("a 16-byte id"),
    }
    .to_uri()
    .expect("a uri")
}

#[test]
fn order_is_preserved() {
    let show = show("Record", "Band", &[10, 11, 12]);
    assert_eq!(
        episode_uris(&show),
        vec![uri(10), uri(11), uri(12)],
        "the page never sorts a show's episodes"
    );
}

#[test]
fn a_non_episode_entry_is_skipped() {
    let mut show = show("Record", "Band", &[10, 11]);
    show.episodes.push(SpotifyUri::Track {
        id: SpotifyId::from_raw(&gid(99)).expect("a 16-byte id"),
    });
    assert_eq!(
        episode_uris(&show),
        vec![uri(10), uri(11)],
        "a track in the episode list is not an episode"
    );
}

#[test]
fn an_empty_name_falls_back_to_the_uri() {
    let show = show("", "Band", &[]);
    let detail = detail("spotify:show:x", &show, Vec::new());
    assert_eq!(detail.show.name, "spotify:show:x");
    assert_eq!(detail.show.uri, "spotify:show:x");
}

#[test]
fn a_missing_cover_is_none() {
    let show = show("Record", "Band", &[]);
    assert_eq!(detail("spotify:show:x", &show, Vec::new()).show.cover, None);
}

#[test]
fn a_blank_publisher_is_an_empty_string_not_some_space() {
    let show = show("Record", "   ", &[]);
    assert_eq!(
        detail("spotify:show:x", &show, Vec::new()).show.publisher,
        ""
    );
}

/// A show URI, so a detail can be matched to the read that named it.
fn show_uri(name: &str) -> String {
    format!("spotify:show:{name}")
}

/// One labelled episode, as the pool's read answers it.
fn labelled(byte: u8, title: &str) -> (String, Episode) {
    (
        uri(byte),
        Episode {
            uri: uri(byte),
            title: title.to_owned(),
            show: String::new(),
            show_uri: None,
            description: None,
            published: None,
            duration_ms: Some(1_000),
            cover: None,
            named: true,
        },
    )
}

/// The episodes a detail carries should have been read, by their titles.
fn titles(detail: &takofy_model::ShowDetail) -> Vec<String> {
    detail
        .episodes
        .iter()
        .map(|episode| episode.title.clone())
        .collect()
}

#[test]
fn one_detail_per_show_in_the_order_read() {
    let reads = vec![
        (show_uri("a"), show("A", "Band", &[1, 2])),
        (show_uri("b"), show("B", "Band", &[3])),
    ];

    let read = details(&reads, &HashMap::new());

    assert_eq!(read.len(), 2, "one detail per show");
    assert_eq!(read[0].show.name, "A");
    assert_eq!(read[1].show.name, "B");
    assert_eq!(read[0].show.uri, show_uri("a"));
    assert_eq!(read[1].show.uri, show_uri("b"));
    assert_eq!(read[0].episodes.len(), 2);
    assert_eq!(read[1].episodes.len(), 1);
}

#[test]
fn each_show_keeps_its_own_order() {
    let reads = vec![(show_uri("a"), show("A", "Band", &[12, 10, 11]))];

    let read = details(&reads, &HashMap::new());

    assert_eq!(
        read[0]
            .episodes
            .iter()
            .map(|episode| episode.uri.clone())
            .collect::<Vec<_>>(),
        vec![uri(12), uri(10), uri(11)],
        "a pooled read never sorts a show's own order"
    );
}

#[test]
fn a_shared_episode_expands_for_both_shows() {
    let reads = vec![
        (show_uri("a"), show("A", "Band", &[7])),
        (show_uri("b"), show("B", "Band", &[7])),
    ];
    let found = HashMap::from([labelled(7, "Shared")]);

    let read = details(&reads, &found);

    assert_eq!(titles(&read[0]), vec!["Shared".to_owned()]);
    assert_eq!(titles(&read[1]), vec!["Shared".to_owned()]);
}

#[test]
fn an_unanswered_uri_falls_back_to_unnamed() {
    let reads = vec![(show_uri("a"), show("A", "Band", &[7]))];

    let read = details(&reads, &HashMap::new());

    let episode = &read[0].episodes[0];
    assert_eq!(episode.uri, uri(7));
    assert_eq!(episode.title, uri(7), "the URI stands in for the title");
    assert!(!episode.named);
    assert_eq!(episode.published, None);
    assert_eq!(episode.duration_ms, None);
}

#[test]
fn a_missing_run_across_two_shows_unnames_only_that_run() {
    let reads = vec![
        (show_uri("a"), show("A", "Band", &[1, 2, 3, 4])),
        (show_uri("b"), show("B", "Band", &[4, 5, 6, 7])),
    ];
    let found = HashMap::from([
        labelled(1, "One"),
        labelled(2, "Two"),
        labelled(3, "Three"),
        labelled(5, "Five"),
        labelled(6, "Six"),
        labelled(7, "Seven"),
    ]);

    let read = details(&reads, &found);

    assert_eq!(
        titles(&read[0]),
        vec![
            "One".to_owned(),
            "Two".to_owned(),
            "Three".to_owned(),
            uri(4)
        ]
    );
    assert_eq!(
        titles(&read[1]),
        vec![
            uri(4),
            "Five".to_owned(),
            "Six".to_owned(),
            "Seven".to_owned()
        ]
    );
    assert!(
        !read[0].episodes[3].named && !read[1].episodes[0].named,
        "the chunk that did not answer unnamed both shows' affected episodes"
    );
    assert!(
        read[0].episodes[..3].iter().all(|episode| episode.named)
            && read[1].episodes[1..].iter().all(|episode| episode.named),
        "the episodes outside the missing run kept their labels"
    );
}
