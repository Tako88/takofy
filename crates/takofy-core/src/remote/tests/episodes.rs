use super::super::*;
use super::test_support::*;

use librespot::core::{SpotifyId, SpotifyUri};
use librespot::protocol::metadata::image::Size;
use librespot::protocol::metadata::{
    Date as DateMessage, Episode as EpisodeMessage, Show as ShowMessage,
};
use std::collections::HashMap;
use takofy_model::{Episode, PublishedAt};

fn episode_uri(byte: u8) -> String {
    SpotifyUri::Episode {
        id: SpotifyId::from_raw(&gid(byte)).expect("a 16-byte id"),
    }
    .to_uri()
    .expect("a uri")
}

/// An episode message with every field the label reads set.
fn wire_episode(name: &str) -> EpisodeMessage {
    let mut episode = EpisodeMessage::default();
    episode.set_gid(gid(2));
    episode.set_name(name.to_owned());
    episode.set_description("A description".to_owned());
    episode.set_duration(2_773_461);
    let mut date = DateMessage::default();
    date.set_year(2026);
    date.set_month(10);
    date.set_day(16);
    episode.publish_time = Some(date).into();
    let mut show = ShowMessage::default();
    show.set_gid(gid(3));
    show.set_name("A Show".to_owned());
    episode.show = Some(show).into();
    episode
}

#[test]
fn partition_separates_episodes_from_tracks() {
    let track = format!("spotify:track:{TRACK_ID}");
    let episode = format!("spotify:episode:{EPISODE_ID}");
    let (tracks, episodes) = partition_by_kind(&owned(&[&track, &episode]));
    assert_eq!(tracks, vec![track]);
    assert_eq!(episodes, vec![episode]);
}

#[test]
fn a_local_or_unknown_uri_goes_with_the_tracks() {
    let local = local().to_uri().expect("a local uri");
    let (tracks, episodes) = partition_by_kind(&owned(&[&local, "not-a-uri", "spotify:episode:1"]));
    assert_eq!(tracks, vec![local, "not-a-uri".to_owned()]);
    assert_eq!(episodes, vec!["spotify:episode:1".to_owned()]);
}

#[test]
fn partition_keeps_order_within_each_group() {
    let uris = owned(&[
        "spotify:track:a",
        "spotify:episode:b",
        "spotify:track:c",
        "spotify:episode:d",
    ]);
    let (tracks, episodes) = partition_by_kind(&uris);
    assert_eq!(
        tracks,
        vec!["spotify:track:a".to_owned(), "spotify:track:c".to_owned()]
    );
    assert_eq!(
        episodes,
        vec![
            "spotify:episode:b".to_owned(),
            "spotify:episode:d".to_owned()
        ]
    );
}

#[test]
fn an_episode_uri_with_no_id_stays_in_the_episode_group() {
    // Pinned: the split reads only the prefix, so an empty id is still asked
    // for as an episode rather than falling back to the track batch.
    let (tracks, episodes) = partition_by_kind(&owned(&["spotify:episode:"]));
    assert!(tracks.is_empty());
    assert_eq!(episodes, vec!["spotify:episode:".to_owned()]);
}

#[test]
fn track_label_from_reads_an_encoded_track_and_refuses_garbage() {
    let bytes = wire_track(Some("Record"), Some(222_000))
        .write_to_bytes()
        .expect("encoded bytes");
    let label =
        track_label_from(&bytes, &format!("spotify:track:{TRACK_ID}")).expect("a named track");
    assert_eq!(label.title, "One");
    assert_eq!(label.artists, "Band");
    assert_eq!(label.duration_ms, Some(222_000));
    assert_eq!(track_label_from(&[0xff, 0xff], "spotify:track:a"), None);
}

#[test]
fn a_full_episode_message_maps_every_field() {
    let mut wire = wire_episode("An episode");
    wire.cover_image = Some(grouped_cover(Size::DEFAULT, &[7; 20])).into();
    let episode = episode_of(&wire, &episode_uri(9)).expect("a named episode");
    assert_eq!(
        episode.uri,
        episode_uri(9),
        "the echoed URI, not the message gid's"
    );
    assert_eq!(episode.title, "An episode");
    assert_eq!(episode.show, "A Show");
    assert_eq!(episode.show_uri.as_deref(), Some(show_uri(3).as_str()));
    assert_eq!(episode.description.as_deref(), Some("A description"));
    assert_eq!(
        episode.published.map(PublishedAt::as_millis),
        Some(1_792_108_800_000),
        "2026-10-16 at midnight UTC"
    );
    assert_eq!(episode.duration_ms, Some(2_773_461));
    assert_eq!(
        episode
            .cover
            .as_ref()
            .and_then(|cover| cover.variants().first())
            .map(|variant| variant.url.as_str()),
        Some("https://i.scdn.co/image/0707070707070707070707070707070707070707")
    );
    assert!(episode.named);
}

#[test]
fn a_missing_or_zero_publish_date_is_none() {
    let uri = episode_uri(2);
    let mut absent = wire_episode("An episode");
    absent.publish_time = None.into();
    assert_eq!(
        episode_of(&absent, &uri)
            .expect("a named episode")
            .published,
        None
    );

    let mut zero = wire_episode("An episode");
    zero.publish_time = Some(DateMessage::default()).into();
    assert_eq!(
        episode_of(&zero, &uri).expect("a named episode").published,
        None
    );

    let mut before_epoch = wire_episode("An episode");
    let mut date = DateMessage::default();
    date.set_year(1969);
    date.set_month(1);
    date.set_day(1);
    before_epoch.publish_time = Some(date).into();
    assert_eq!(
        episode_of(&before_epoch, &uri)
            .expect("a named episode")
            .published,
        None
    );
}

#[test]
fn a_year_only_publish_date_is_january_first_midnight() {
    let uri = episode_uri(2);
    let mut date = DateMessage::default();
    date.set_year(2026);
    assert_eq!(
        published_at(Some(&date)).map(PublishedAt::as_millis),
        Some(1_767_225_600_000),
        "no month or day means January 1 at 00:00 UTC"
    );

    let mut wire = wire_episode("An episode");
    wire.publish_time = Some(date).into();
    assert_eq!(
        episode_of(&wire, &uri)
            .expect("a named episode")
            .published
            .map(PublishedAt::as_millis),
        Some(1_767_225_600_000)
    );
}

#[test]
fn a_bad_show_gid_gives_no_show_uri() {
    let mut wire = wire_episode("An episode");
    let mut show = ShowMessage::default();
    show.set_gid(vec![1; 15]);
    show.set_name("A Show".to_owned());
    wire.show = Some(show).into();
    let episode = episode_of(&wire, &episode_uri(2)).expect("a named episode");
    assert_eq!(episode.show_uri, None);
    assert_eq!(episode.show, "A Show", "the name is still drawn");
}

#[test]
fn a_named_episode_with_a_bad_gid_keeps_the_requested_uri() {
    let mut wire = wire_episode("An episode");
    wire.set_gid(vec![1; 15]);
    let bytes = wire.write_to_bytes().expect("encoded bytes");
    let episode = episode_from(&bytes, &episode_uri(9)).expect("a named episode");
    assert_eq!(episode.uri, episode_uri(9), "the URI the answer echoed");
    assert_eq!(episode.title, "An episode", "the title is not lost");
    assert!(episode.named);

    let label = label_from_episode(&episode);
    assert_eq!(label.title, "An episode");
    assert_eq!(label.artists, "A Show");
    assert_eq!(
        label.artist_uris,
        vec![show_uri(3)],
        "the show gid is still all the link needs"
    );
}

#[test]
fn a_blank_show_name_gives_neither_a_name_nor_a_link() {
    let uri = episode_uri(2);
    let mut wire = wire_episode("An episode");
    let mut show = ShowMessage::default();
    show.set_gid(gid(3));
    show.set_name(" ".to_owned());
    wire.show = Some(show).into();

    let episode = episode_of(&wire, &uri).expect("a named episode");
    assert_eq!(episode.show, "", "a blank show name normalises to none");
    assert_eq!(
        episode.show_uri.as_deref(),
        Some(show_uri(3).as_str()),
        "the show URI is still resolvable"
    );

    let label = label_from_episode(&episode);
    assert_eq!(label.artists, "");
    assert!(
        label.artist_uris.is_empty(),
        "a row with no visible show name links nowhere"
    );
}

#[test]
fn a_negative_duration_is_none() {
    let uri = episode_uri(2);
    let mut wire = wire_episode("An episode");
    wire.set_duration(-1);
    assert_eq!(
        episode_of(&wire, &uri)
            .expect("a named episode")
            .duration_ms,
        None
    );

    let mut absent = wire_episode("An episode");
    absent.duration = None;
    assert_eq!(
        episode_of(&absent, &uri)
            .expect("a named episode")
            .duration_ms,
        None
    );
}

#[test]
fn an_empty_name_is_no_label() {
    let uri = episode_uri(2);
    let mut empty = wire_episode("");
    assert_eq!(episode_of(&empty, &uri), None);
    empty.name = None;
    assert_eq!(episode_of(&empty, &uri), None);
}

#[test]
fn episode_from_reads_encoded_bytes_and_refuses_garbage() {
    let bytes = wire_episode("An episode")
        .write_to_bytes()
        .expect("encoded bytes");
    let episode = episode_from(&bytes, &episode_uri(9)).expect("a named episode");
    assert_eq!(episode.uri, episode_uri(9));
    assert_eq!(episode.title, "An episode");
    assert_eq!(episode_from(&[0xff, 0xff], &episode_uri(9)), None);
}

#[test]
fn an_episode_becomes_a_track_label_with_the_show_as_its_credit() {
    let episode =
        episode_of(&wire_episode("An episode"), &episode_uri(2)).expect("a named episode");
    let label = label_from_episode(&episode);
    assert_eq!(label.title, "An episode");
    assert_eq!(label.artists, "A Show");
    assert_eq!(label.artist_uris, vec![show_uri(3)]);
    assert_eq!(label.album, None);
    assert_eq!(label.album_uri, None);
    assert_eq!(label.duration_ms, Some(2_773_461));
}

#[test]
fn expand_labels_over_a_track_and_an_episode_names_both() {
    let episode_uri = episode_uri(2);
    let mut found = HashMap::new();
    found.insert(
        episode_uri.clone(),
        episode_of(&wire_episode("An episode"), &episode_uri).expect("a named episode"),
    );
    let labels = merge_labels(named(&[("a", "One", "Band")]), episode_labels(found));
    let entries = expand_labels(&items(&["a", &episode_uri]), &labels);
    assert_eq!(entries[0].title, "One");
    assert_eq!(entries[0].artists, "Band");
    assert_eq!(entries[1].title, "An episode");
    assert_eq!(entries[1].artists, "A Show");
    assert_eq!(entries[1].artist_uris, vec![show_uri(3)]);
    assert!(entries.iter().all(|entry| entry.named));
}

#[test]
fn a_missing_episode_falls_back_to_its_uri_unnamed() {
    let uris = owned(&[&episode_uri(2), &episode_uri(4)]);
    let mut found = HashMap::new();
    found.insert(
        episode_uri(2),
        episode_of(&wire_episode("An episode"), &episode_uri(2)).expect("a named episode"),
    );
    let episodes = expand_episodes(&uris, &found);
    assert_eq!(episodes.len(), 2);
    assert_eq!(episodes[0].title, "An episode");
    assert!(episodes[0].named);
    assert_eq!(episodes[1].uri, episode_uri(4));
    assert_eq!(episodes[1].title, episode_uri(4));
    assert!(!episodes[1].named);
    assert!(episodes[1].show.is_empty());
    assert_eq!(episodes[1].published, None);
    assert_eq!(episodes[1].duration_ms, None);
}

#[test]
fn expand_episodes_draws_each_position_with_its_own_uri() {
    // The answer is found by the position's URI, so that URI is what the
    // position draws even if the value carried another one.
    let uri = episode_uri(2);
    let mut found = HashMap::new();
    found.insert(
        uri.clone(),
        Episode {
            uri: episode_uri(9),
            title: "An episode".to_owned(),
            show: "A Show".to_owned(),
            show_uri: Some(show_uri(3)),
            description: None,
            published: None,
            duration_ms: None,
            cover: None,
            named: true,
        },
    );
    let episodes = expand_episodes(&owned(&[&uri]), &found);
    assert_eq!(episodes[0].uri, uri);
    assert_eq!(episodes[0].title, "An episode");
}

#[test]
fn expand_episodes_keeps_one_episode_per_repeated_position() {
    let uri = episode_uri(2);
    let uris = vec![uri.clone(), episode_uri(4), uri.clone()];
    let mut found = HashMap::new();
    found.insert(
        uri.clone(),
        episode_of(&wire_episode("An episode"), &uri).expect("a named episode"),
    );
    let episodes = expand_episodes(&uris, &found);
    assert_eq!(episodes.len(), 3, "one episode per input position");
    assert_eq!(episodes[0].uri, uri);
    assert_eq!(episodes[2].uri, uri, "the repeat is cloned, not dropped");
    assert_eq!(episodes[0].title, "An episode");
    assert_eq!(episodes[2].title, "An episode");
    assert_eq!(episodes[1].uri, episode_uri(4));
    assert!(!episodes[1].named);
    assert!(episodes[0].named && episodes[2].named);
}

#[test]
fn a_failed_episode_batch_leaves_track_labels_intact() {
    // A failed batch arrives as no answers at all, like an honestly empty one.
    let merged = merge_labels(named(&[("a", "One", "Band")]), Labels::new());
    assert_eq!(merged.len(), 1);
    assert_eq!(
        merged.get("a").map(|label| label.title.as_str()),
        Some("One")
    );
}
