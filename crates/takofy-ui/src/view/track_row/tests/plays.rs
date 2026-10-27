use super::super::*;
use super::test_support::*;
use crate::player::NowPlaying;
use crate::route::Page;
use crate::theme::Tokens;
use crate::view::test_support::click_events;

/// The columns a plays row uses: the artist kept, the album and date gone.
fn plays_columns() -> Columns {
    crate::layout::table::columns(900.0, crate::layout::Table::Full.columns().for_plays())
}

/// A plays row: the track's credit in its own column, no art box.
fn episode_row() -> Row<'static> {
    Row {
        uri: "spotify:episode:a",
        number: 1,
        title: "The Episode",
        artist: "Lateral with Tom Scott",
        // The episode names its show as its credit, so this cell opens the
        // show rather than an artist.
        artist_uri: Some("spotify:show:s"),
        album: None,
        album_uri: None,
        added: None,
        duration: None,
        state: State::Resting,
        cover: None,
        look: Look::default(),
        menu: false,
        liked: None,
    }
}

/// The click one sequence of passes reports for a pointer at `pos`, on a
/// plays-shaped row solved the way the album page solves it.
fn click_plays(ctx: &egui::Context, pos: Pos2) -> Option<Click> {
    let tokens = Tokens::DARK;
    let rect = body_rect();
    let columns = plays_columns();
    let mut output = ctx.run_ui(input_with(Vec::new()), |ui| {
        let _ = show(ui, rect, columns, &episode_row(), &tokens);
    });
    output.textures_delta.clear();

    let mut answers = Vec::new();
    for event in click_events(pos) {
        let mut answer = None;
        let mut output = ctx.run_ui(input_with(vec![event]), |ui| {
            answer = show(ui, rect, columns, &episode_row(), &tokens);
        });
        output.textures_delta.clear();
        answers.push(answer);
    }
    answers.pop().flatten()
}

/// Finding 4: `Loading` is a track on its way and an ended one has nothing
/// sounding, so neither may masquerade as a pause — the equaliser would
/// freeze on a stopped track and read as "held".
#[test]
fn a_loaded_row_is_only_paused_when_the_transport_is() {
    let uri = "spotify:track:a";
    let cases = [
        (TransportState::Idle, State::Stopped),
        (TransportState::Loading, State::Playing),
        (TransportState::Playing, State::Playing),
        (TransportState::Paused, State::Paused),
        (TransportState::Unavailable, State::Stopped),
        (TransportState::Stopped, State::Stopped),
        (TransportState::Reconnecting, State::Playing),
        (
            TransportState::Failed(crate::message::Failure::Other),
            State::Stopped,
        ),
    ];
    for (state, expected) in cases {
        let mut transport = Transport::default();
        transport.play(NowPlaying::named(uri, "A", "B"));
        transport.state = state;
        assert_eq!(state_of(uri, &transport), expected, "{state:?}");
        assert_eq!(
            state_of("spotify:track:b", &transport),
            State::Resting,
            "another row while {state:?}"
        );
    }
}

#[test]
fn a_credit_click_on_an_episode_row_opens_the_show() {
    let ctx = context();
    // The credit sits in its own artist column, so the click lands on its
    // painted words rather than on the name or the index column.
    let mut first = ctx.run_ui(input(), |ui| {
        let _ = show(
            ui,
            body_rect(),
            plays_columns(),
            &episode_row(),
            &Tokens::DARK,
        );
    });
    first.textures_delta.clear();
    let artist = text_rect(&first, "Lateral with Tom Scott").expect("the artist cell");
    assert_eq!(
        click_plays(&ctx, artist.center()),
        Some(Click::Open(Page::credit("spotify:show:s"))),
        "a click on the episode's credit opens the show page"
    );
}

#[test]
fn an_episode_rows_credit_node_says_open_show() {
    let ctx = context();
    ctx.enable_accesskit();
    let row = episode_row();
    let rect = body_rect();
    let columns = plays_columns();
    let tokens = Tokens::DARK;
    let mut first = ctx.run_ui(input(), |ui| {
        let _ = show(ui, rect, columns, &row, &tokens);
    });
    first.textures_delta.clear();
    let at = text_rect(&first, "Lateral with Tom Scott")
        .expect("the credit cell")
        .center();

    let mut answers = Vec::new();
    let mut labels = Vec::new();
    for event in click_events(at) {
        let mut answer = None;
        let mut output = ctx.run_ui(input_with(vec![event]), |ui| {
            answer = show(ui, rect, columns, &row, &tokens);
        });
        output.textures_delta.clear();
        answers.push(answer);
        if let Some(update) = output.platform_output.accesskit_update.as_ref() {
            for (_, node) in &update.nodes {
                if node.role() == egui::accesskit::Role::Link
                    && let Some(label) = node.label()
                {
                    labels.push(label.to_owned());
                }
            }
        }
    }

    // The credit's label names its page kind; the literal "artist" was wrong for a show.
    assert!(
        labels
            .iter()
            .any(|label| label == "Open show Lateral with Tom Scott"),
        "the credit node is labelled {labels:?}"
    );
    assert_eq!(
        answers.iter().flatten().last(),
        Some(&Click::Open(Page::Show("spotify:show:s".to_owned()))),
        "the credit opens the show page: {answers:?}"
    );
}

/// One frame of a plays-shaped row, with what it painted.
fn paint_plays(ctx: &egui::Context, row: &Row<'_>) -> egui::FullOutput {
    let tokens = Tokens::DARK;
    let mut output = ctx.run_ui(input(), |ui| {
        let _ = show(ui, body_rect(), plays_columns(), row, &tokens);
    });
    output.textures_delta.clear();
    output
}

/// The left of the episode's painted name, read back from a frame.
fn name_left(output: &egui::FullOutput) -> f32 {
    text_rect(output, "The Episode").expect("the name").left()
}

#[test]
fn a_row_with_no_art_box_starts_its_name_at_the_title_column() {
    let ctx = context();
    let output = paint_plays(&ctx, &episode_row());
    assert_eq!(name_left(&output), plays_columns().title.left());
}

#[test]
fn a_loading_art_box_pushes_the_name_past_the_box_and_its_gutter() {
    let ctx = context();
    let row = Row {
        cover: Some(Cover::Loading),
        ..episode_row()
    };
    let output = paint_plays(&ctx, &row);
    let expected =
        plays_columns().title.left() + crate::theme::extent::LIBRARY_ART + crate::theme::space::S3;
    assert_eq!(name_left(&output), expected);
}

#[test]
fn a_missing_cover_still_takes_its_box() {
    let ctx = context();
    let row = Row {
        cover: Some(Cover::Missing),
        ..episode_row()
    };
    let output = paint_plays(&ctx, &row);
    let expected =
        plays_columns().title.left() + crate::theme::extent::LIBRARY_ART + crate::theme::space::S3;
    assert_eq!(name_left(&output), expected);
}

#[test]
fn the_accessibility_label_names_whichever_credit_the_row_draws() {
    assert_eq!(label("T", "A", None), "T by A");
    assert_eq!(label("T", "A", Some("B")), "T by A", "the artist wins");
    assert_eq!(label("T", "", Some("B")), "T on B");
    assert_eq!(
        label("T", "", Some("")),
        "T",
        "an empty credit is no credit"
    );
    assert_eq!(label("T", "", None), "T");
}
