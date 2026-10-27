use super::super::*;
use super::test_support::*;

#[test]
fn the_head_paints_the_number_title_and_artist_but_no_album_or_added() {
    let ctx = context();
    let mut library = library_with(detail(vec![track(
        "spotify:track:a",
        "One",
        "Band",
        222_000,
    )]));
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

    assert!(texts.contains(&"#"), "the index heading: {texts:?}");
    assert!(texts.contains(&"TITLE"), "the title heading: {texts:?}");
    assert!(texts.contains(&"ARTIST"), "the artist heading: {texts:?}");
    assert!(
        !texts.contains(&"ADDED"),
        "an album table draws no ADDED heading: {texts:?}"
    );
    // The band's eyebrow is the one `ALBUM` on this page; a column heading
    // would be a second.
    assert_eq!(
        texts.iter().filter(|text| **text == "ALBUM").count(),
        1,
        "an album table draws no ALBUM heading: {texts:?}"
    );
}

#[test]
fn a_row_paints_its_artist_in_its_own_column() {
    let ctx = context();
    let mut library = library_with(detail(vec![track(
        "spotify:track:a",
        "One",
        "Nils Aker",
        222_000,
    )]));
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

    assert!(texts.contains(&"One"), "the row's name: {texts:?}");
    assert!(texts.contains(&"Nils Aker"), "the row's artist: {texts:?}");

    // The credit has a column of its own, beside the name rather than under
    // it, and the two lines are level with each other.
    let (title_cell, artist_cell, _) = cells_for(&ctx, 0, 1);
    let name = text_rect(&output, "One").expect("the name");
    let artist = text_rect(&output, "Nils Aker").expect("the artist");
    assert_eq!(
        name.left(),
        title_cell.left(),
        "the name starts the title column"
    );
    assert_eq!(
        artist.left(),
        artist_cell.left(),
        "the artist starts its own column"
    );
    assert!(
        (artist.center().y - name.center().y).abs() < 0.5,
        "the artist sits level with the name, not under it"
    );
}

#[test]
fn a_two_disc_album_is_one_list_numbered_from_one() {
    // The model has no disc field: the album's own order is already flattened
    // and a number is a position. Two discs therefore paint 1 and 2, and no
    // disc heading exists to paint.
    let ctx = context();
    let mut library = library_with(detail(vec![
        track("spotify:track:a", "One", "Band", 222_000),
        track("spotify:track:b", "Two", "Band", 198_000),
    ]));
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

    assert!(texts.contains(&"1"), "{texts:?}");
    assert!(texts.contains(&"2"), "{texts:?}");
    assert!(
        !texts.iter().any(|text| text.starts_with("Disc")),
        "a multi-disc album draws no disc heading: {texts:?}"
    );
}

#[test]
fn an_album_with_a_duplicate_track_uri_keeps_both_rows_and_plays_the_one_clicked() {
    let ctx = context();
    let uri = "spotify:track:a";
    let mut library = library_with(detail(vec![
        track(uri, "One", "Band", 222_000),
        track(uri, "One", "Band", 222_000),
    ]));
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
    let names = drawn_texts(&output)
        .into_iter()
        .filter(|text| *text == "One")
        .count();
    assert_eq!(names, 2, "both positions draw their own row");

    // The second of two identical positions: nothing follows it in the queue.
    let rect = row_rect_for(&ctx, 1, 2);
    let command = click(
        &ctx,
        rect.center(),
        2,
        &mut library,
        &mut transport,
        &mut queue,
        &mut art,
    );
    assert_eq!(
        command,
        Some(Command::Play {
            uri: uri.to_owned()
        })
    );
    assert!(
        !queue.can_next(),
        "the second position was started, not the first"
    );
}

#[test]
fn an_empty_album_draws_the_band_and_the_empty_copy() {
    let ctx = context();
    let mut library = library_with(detail(Vec::new()));
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

    assert!(texts.contains(&"ALBUM"), "the band's eyebrow: {texts:?}");
    assert!(
        texts.contains(&"Signal Decay"),
        "the band's title: {texts:?}"
    );
    assert!(
        texts.contains(&"No tracks on this album."),
        "the empty copy: {texts:?}"
    );
    assert!(
        !texts.contains(&"TITLE"),
        "an album with no tracks draws no table head: {texts:?}"
    );
}
