//! The artist page's body: the photograph hero, Popular, and the Discography.
//!
//! The page draws only what the library already has. `Load::Idle` has nothing to
//! say because the route asks for the artist when it opens, so it draws nothing
//! rather than a second spinner. A ready detail that does not name this page is
//! another artist's, so it draws the spinner instead of that artist's hero.
//!
//! The discography's tracklists are labelled only while their block is on
//! screen: the block's rect is solved before it is drawn, and only an on-screen
//! block asks the release store for its labels. The store's requests are drained
//! by the app after the draw, because a want cannot be dropped the way a command
//! can.

use std::time::Instant;

use egui::{Rect, Sense, Ui, Vec2};
use takofy_model::Release;

use crate::cover::{Art, Cover};
use crate::format::{discography_meta, format_position};
use crate::layout::{Table, table as table_layout};
use crate::library::{Library, Load};
use crate::message::Command;
use crate::player::Transport;
use crate::queue::Queue;
use crate::releases::Labels;
use crate::route::Page;
use crate::theme::{Tokens, extent};
use crate::view::text as recipe;

use super::artist_hero::{self, ArtistHero};
use super::release_block::{self, Block};
use super::section;
use super::track_list::{head, list_rect};
use super::track_row::{self, Click, Row, TitleCell, state_of};

/// Draws the artist page and returns the command its controls asked for.
#[expect(
    clippy::too_many_arguments,
    reason = "the pane, its table, the state it draws and the frame's stores"
)]
pub fn artist_view(
    ui: &mut Ui,
    table: Table,
    library: &mut Library,
    transport: &mut Transport,
    queue: &mut Queue,
    art: &mut Art,
    now: Instant,
    tokens: &Tokens,
) -> Option<Command> {
    // The page's own URI, so Retry asks for the artist this page is on rather
    // than whichever one happens to be in the load state.
    let Some(Page::Artist(uri)) = library.page() else {
        return None;
    };
    let uri = uri.clone();

    let mut command = None;
    let mut retry = false;

    // The scroll offset is this artist's, not the album's or the playlist's that
    // share the pane.
    egui::ScrollArea::vertical()
        .id_salt(("artist", uri.as_str()))
        .show(ui, |ui| {
            let width = ui.available_width();
            let page = library.artist_page();
            match page.load {
                Load::Idle => {}
                Load::Loading => {
                    ui.spinner();
                }
                Load::Failed(failure) => {
                    ui.label(recipe::muted(
                        crate::signin::failure_message(*failure),
                        tokens,
                    ));
                    if ui.button("Retry").clicked() {
                        retry = true;
                    }
                }
                Load::Ready(_) => {
                    // A ready detail that does not name this page is another
                    // artist's: the page keeps its loading state rather than
                    // painting that artist's hero and discography.
                    let Some(artist) = page.current else {
                        ui.spinner();
                        return;
                    };

                    let hero = ArtistHero {
                        name: &artist.name,
                        portrait: art.look(artist.portrait.as_ref(), now),
                        playable: !artist.top_tracks.is_empty(),
                        follow: page.follow.label(),
                        follow_live: page.follow.pressable(),
                    };
                    let pressed = artist_hero::show(ui, &hero, tokens);
                    if pressed.followed {
                        command = page.follow.pressed();
                    }
                    if pressed.played {
                        command = queue.start(&artist.top_tracks, 0, transport);
                    }

                    popular(ui, table, width, artist, transport, queue, tokens, &mut command);

                    let tracks: usize = artist.releases.iter().map(|release| release.tracks.len()).sum();
                    section::head(
                        ui,
                        "Discography",
                        &discography_meta(artist.releases.len(), tracks),
                        tokens,
                    );
                    if artist.releases.is_empty() {
                        ui.label(recipe::muted("No releases.", tokens));
                        return;
                    }
                    for release in &artist.releases {
                        let on_screen = ui.clip_rect().intersects(block_rect(ui, release));
                        if !on_screen {
                            let block = Block {
                                release,
                                labels: Labels::Idle,
                                cover: Cover::Missing,
                                on_screen: false,
                                playable: false,
                            };
                            let _ = release_block::show(ui, table, &block, transport, tokens);
                            continue;
                        }
                        let labels = page.releases.look(&release.uri);
                        let cover = art.look(release.cover.as_ref(), now);
                        let block = Block {
                            release,
                            labels,
                            cover,
                            on_screen: true,
                            playable: matches!(labels, Labels::Ready(_)),
                        };
                        let clicked = release_block::show(ui, table, &block, transport, tokens);
                        let ready = match labels {
                            Labels::Ready(tracks) => Some(tracks),
                            Labels::Idle | Labels::Loading | Labels::Failed(_) => None,
                        };
                        if clicked.played
                            && let Some(tracks) = ready
                        {
                            command = queue.start(tracks, 0, transport);
                        }
                        if let (Some(index), Some(tracks)) = (clicked.row, ready) {
                            command = queue.start(tracks, index, transport);
                        }
                    }
                }
            }
        });

    if retry {
        return library.open(Page::Artist(uri));
    }
    command
}

/// The Popular section: the artist's top tracks as the album's own table rows.
///
/// No `Plays` column: the wire carries no play counts. A row is the name over
/// its artist, numbered by its place in the list, and a double click starts the
/// whole top list from there.
#[expect(
    clippy::too_many_arguments,
    reason = "the section needs its table, width, state and frame's stores"
)]
fn popular(
    ui: &mut Ui,
    table: Table,
    width: f32,
    artist: &takofy_model::ArtistDetail,
    transport: &mut Transport,
    queue: &mut Queue,
    tokens: &Tokens,
    command: &mut Option<Command>,
) {
    section::head(ui, "Popular", "", tokens);
    if artist.top_tracks.is_empty() {
        ui.label(recipe::muted("No popular tracks.", tokens));
        return;
    }

    let columns = table_layout::columns(width, table.columns().for_album());
    head(ui, columns, tokens);
    let count = artist.top_tracks.len();
    let list = list_rect(ui, count);
    ui.allocate_rect(list, Sense::hover());
    let visible = ui.clip_rect();

    for (index, entry) in artist.top_tracks.iter().enumerate() {
        let rect = table_layout::row_rect(list, index, extent::ROW_H);
        if !visible.intersects(rect) {
            continue;
        }
        let duration = entry.duration_ms.map(format_position);
        let row = Row {
            uri: &entry.uri,
            number: index + 1,
            title: &entry.title,
            title_cell: TitleCell::NameOverArtist,
            artist: &entry.artists,
            // The artist page's own subline is not a link to itself.
            artist_uri: None,
            album: None,
            album_uri: None,
            added: None,
            duration: duration.as_deref(),
            state: state_of(&entry.uri, transport),
            cover: Cover::Missing,
        };
        match track_row::show(ui, rect, columns, &row, tokens) {
            Some(Click::Play) => {
                *command = queue.start(&artist.top_tracks, index, transport);
            }
            Some(Click::Open(_)) | None => {}
        }
    }
}

/// The rect a release block will occupy, solved before it is drawn so its
/// visibility decides whether its labels are wanted.
fn block_rect(ui: &Ui, release: &Release) -> Rect {
    let top = ui.cursor().min;
    let height = release_block::height(release.tracks.len());
    Rect::from_min_size(top, Vec2::new(ui.available_width(), height))
}

#[cfg(test)]
mod tests {
    use egui::{Context, Event, Pos2, RawInput, Rect, Vec2, pos2, vec2};
    use takofy_model::{ArtistDetail, Release, ReleaseKind, Release as _Release, TrackEntry};

    use super::*;
    use crate::cover::Cover;
    use crate::library::Library;
    use crate::message::{Failure, Update};
    use crate::releases::Labels;
    use crate::theme::{Tokens, extent, fonts, hero, space, stroke, style};
    use crate::view::release_block::Block;
    use crate::view::test_support::click_events;

    const ARTIST: &str = "spotify:artist:a";
    /// The artist a stale ready detail names while the page is on [`ARTIST`].
    const OTHER_ARTIST: &str = "spotify:artist:b";

    fn context() -> Context {
        let tokens = Tokens::DARK;
        let ctx = Context::default();
        ctx.set_fonts(fonts::definitions());
        ctx.set_global_style(style(&tokens));
        ctx
    }

    fn input(size: Vec2, events: Vec<Event>) -> RawInput {
        RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)),
            events,
            ..Default::default()
        }
    }

    fn run(ctx: &Context, input: RawInput, draw: impl FnMut(&mut egui::Ui)) -> egui::FullOutput {
        let mut output = ctx.run_ui(input, draw);
        output.textures_delta.clear();
        output
    }

    fn drawn_texts(output: &egui::FullOutput) -> Vec<&str> {
        output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Text(text) => Some(text.galley.text()),
                _ => None,
            })
            .collect()
    }

    fn text_rect(output: &egui::FullOutput, label: &str) -> Option<Rect> {
        output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Text(text) if text.galley.text() == label => {
                    Some(Rect::from_min_size(text.pos, text.galley.size()))
                }
                _ => None,
            })
    }

    fn track(uri: &str, title: &str, artists: &str, duration_ms: u32) -> TrackEntry {
        TrackEntry {
            uri: uri.to_owned(),
            title: title.to_owned(),
            artists: artists.to_owned(),
            album: None,
            album_uri: None,
            artist_uris: Vec::new(),
            added_at: None,
            duration_ms: Some(duration_ms),
            named: true,
            cover: None,
        }
    }

    fn release(uri: &str, name: &str, tracks: &[&str]) -> Release {
        Release {
            uri: uri.to_owned(),
            name: name.to_owned(),
            kind: ReleaseKind::Album,
            year: Some(2024),
            cover: None,
            tracks: tracks.iter().map(|track| (*track).to_owned()).collect(),
        }
    }

    fn detail(top: Vec<TrackEntry>, releases: Vec<Release>) -> ArtistDetail {
        ArtistDetail {
            uri: ARTIST.to_owned(),
            name: "Vela Moreno".to_owned(),
            portrait: None,
            top_tracks: top,
            releases,
        }
    }

    fn library_with(detail: ArtistDetail) -> Library {
        let mut library = Library::default();
        library.open(Page::Artist(detail.uri.clone()));
        library.apply(&Update::Artist(detail));
        library
    }

    fn frame(
        ctx: &Context,
        size: Vec2,
        events: Vec<Event>,
        library: &mut Library,
        transport: &mut Transport,
        queue: &mut Queue,
        art: &mut Art,
    ) -> (Option<Command>, egui::FullOutput) {
        let tokens = Tokens::DARK;
        let mut command = None;
        let output = run(ctx, input(size, events), |ui| {
            ui.add_space(space::S3);
            if let Some(next) = artist_view(
                ui,
                Table::Full,
                library,
                transport,
                queue,
                art,
                Instant::now(),
                &tokens,
            ) {
                command = Some(next);
            }
        });
        (command, output)
    }

    fn click(
        ctx: &Context,
        size: Vec2,
        pos: Pos2,
        times: usize,
        library: &mut Library,
        transport: &mut Transport,
        queue: &mut Queue,
        art: &mut Art,
    ) -> Option<Command> {
        let mut last = None;
        for _ in 0..times {
            for event in click_events(pos) {
                last = frame(ctx, size, vec![event], library, transport, queue, art).0;
            }
        }
        last
    }

    /// The hero's play tile and its Follow button, solved as the hero lays them
    /// out: the band first, then the action row under it.
    fn hero_rects(ctx: &Context, size: Vec2) -> (Rect, Rect) {
        let tokens = Tokens::DARK;
        let mut found = None;
        run(ctx, input(size, Vec::new()), |ui| {
            ui.add_space(space::S3);
            let (band, _) =
                ui.allocate_exact_size(vec2(ui.available_width(), hero::ARTIST_MIN_H), Sense::hover());
            let (row, _) = ui.allocate_exact_size(
                vec2(ui.available_width(), hero::PLAY + 2.0 * space::S4),
                Sense::hover(),
            );
            let play = Rect::from_min_size(
                Pos2::new(row.left() + space::GUTTER, row.center().y - hero::PLAY / 2.0),
                Vec2::splat(hero::PLAY),
            );
            let size = crate::view::controls::text_button_size(ui, &tokens, "Follow");
            let follow = Rect::from_min_size(
                Pos2::new(play.right() + space::S4, row.center().y - size.y / 2.0),
                size,
            );
            let _ = band;
            found = Some((play, follow));
        });
        found.expect("the hero and its action row lay out")
    }

    /// The rect the Popular table's `index`-th row occupies, solved the way the
    /// page solves it.
    fn popular_row_rect(ctx: &Context, size: Vec2, index: usize, count: usize) -> Rect {
        let tokens = Tokens::DARK;
        let mut found = None;
        run(ctx, input(size, Vec::new()), |ui| {
            ui.add_space(space::S3);
            let _ =
                ui.allocate_exact_size(vec2(ui.available_width(), hero::ARTIST_MIN_H), Sense::hover());
            let _ = ui.allocate_exact_size(
                vec2(ui.available_width(), hero::PLAY + 2.0 * space::S4),
                Sense::hover(),
            );
            section::head(ui, "Popular", "", &tokens);
            let columns = table_layout::columns(ui.available_width(), Table::Full.columns().for_album());
            head(ui, columns, &tokens);
            let list = list_rect(ui, count);
            found = Some(table_layout::row_rect(list, index, extent::ROW_H));
        });
        found.expect("the popular table lays out")
    }

    #[test]
    fn the_hero_draws_the_artists_name() {
        let ctx = context();
        let mut library = library_with(detail(Vec::new(), Vec::new()));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let (_, output) = frame(
            &ctx,
            Vec2::new(900.0, 800.0),
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let texts = drawn_texts(&output);

        assert!(texts.contains(&"Vela Moreno"), "the hero's title: {texts:?}");
        assert!(texts.contains(&"ARTIST"), "the hero's eyebrow: {texts:?}");
    }

    #[test]
    fn a_double_click_on_a_popular_row_plays_that_track() {
        let ctx = context();
        let size = Vec2::new(900.0, 800.0);
        let top = vec![
            track("spotify:track:a", "One", "Vela Moreno", 222_000),
            track("spotify:track:b", "Two", "Vela Moreno", 198_000),
        ];
        let mut library = library_with(detail(top, Vec::new()));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let rect = popular_row_rect(&ctx, size, 1, 2);
        let command = click(
            &ctx,
            size,
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
                uri: "spotify:track:b".to_owned()
            }),
            "the row under the double click is what plays"
        );
        assert_eq!(
            transport.now_playing.as_ref().map(|now| now.uri.as_str()),
            Some("spotify:track:b")
        );
    }

    #[test]
    fn the_heros_play_starts_the_top_list() {
        let ctx = context();
        let size = Vec2::new(900.0, 800.0);
        let top = vec![
            track("spotify:track:a", "One", "Vela Moreno", 222_000),
            track("spotify:track:b", "Two", "Vela Moreno", 198_000),
        ];
        let mut library = library_with(detail(top, Vec::new()));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let (play, _) = hero_rects(&ctx, size);
        let command = click(
            &ctx,
            size,
            play.center(),
            1,
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );

        assert_eq!(
            command,
            Some(Command::Play {
                uri: "spotify:track:a".to_owned()
            }),
            "the hero plays the artist from the top of the list"
        );
        assert!(
            queue.can_next(),
            "the whole top list is queued, not just its first track"
        );
    }

    #[test]
    fn the_follow_press_asks_for_the_negated_state() {
        let ctx = context();
        let size = Vec2::new(900.0, 800.0);
        let mut library = library_with(detail(
            vec![track("spotify:track:a", "One", "Vela Moreno", 222_000)],
            Vec::new(),
        ));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let (_, follow) = hero_rects(&ctx, size);
        let command = click(
            &ctx,
            size,
            follow.center(),
            1,
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );

        assert_eq!(
            command,
            Some(Command::SetFollow {
                artist: ARTIST.to_owned(),
                followed: true,
            }),
            "an unconfirmed artist is followed, not unfollowed"
        );
        assert_eq!(
            library.follow.label(),
            "Follow",
            "the press alone must not flip the label"
        );
    }

    #[test]
    fn the_follow_press_targets_unfollow_once_the_server_confirmed() {
        let ctx = context();
        let size = Vec2::new(900.0, 800.0);
        let mut library = library_with(detail(Vec::new(), Vec::new()));
        library.apply(&Update::Followed {
            artist: ARTIST.to_owned(),
            followed: true,
        });
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let (_, follow) = hero_rects(&ctx, size);
        let command = click(
            &ctx,
            size,
            follow.center(),
            1,
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );

        assert_eq!(
            command,
            Some(Command::SetFollow {
                artist: ARTIST.to_owned(),
                followed: false,
            })
        );
    }

    #[test]
    fn an_off_screen_block_asks_for_no_labels() {
        let ctx = context();
        let size = Vec2::new(900.0, 600.0);
        let mut library = library_with(detail(
            Vec::new(),
            vec![
                release("spotify:album:r1", "One", &["spotify:track:1"]),
                release("spotify:album:r2", "Two", &["spotify:track:2"]),
                release("spotify:album:r3", "Three", &["spotify:track:3"]),
            ],
        ));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let _ = frame(
            &ctx,
            size,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );

        for absent in ["spotify:album:r2", "spotify:album:r3"] {
            assert_eq!(
                library.releases.look(absent),
                Labels::Idle,
                "{absent} is off screen and must not be wanted"
            );
        }
    }

    /// One frame asks for the on-screen block's labels and nothing else: the
    /// want set is filled by the draw and drained once, afterwards.
    #[test]
    fn one_frame_asks_for_exactly_the_on_screen_labels() {
        let ctx = context();
        let size = Vec2::new(900.0, 600.0);
        let mut library = library_with(detail(
            Vec::new(),
            vec![
                release("spotify:album:r1", "One", &["spotify:track:1"]),
                release("spotify:album:r2", "Two", &["spotify:track:2"]),
                release("spotify:album:r3", "Three", &["spotify:track:3"]),
            ],
        ));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let _ = frame(
            &ctx,
            size,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let requests = library.releases.requests();

        assert_eq!(
            requests,
            vec![Command::LoadReleaseTracks {
                release: "spotify:album:r1".to_owned(),
                tracks: vec!["spotify:track:1".to_owned()],
            }],
            "exactly the block that is on screen is asked for: {requests:?}"
        );
        assert!(
            library.releases.requests().is_empty(),
            "the drain clears the want set, so one frame is one request"
        );
    }

    #[test]
    fn a_release_rows_double_click_plays_that_track() {
        let ctx = context();
        let tokens = Tokens::DARK;
        let size = Vec2::new(900.0, 800.0);
        let release = _Release {
            uri: "spotify:album:r1".to_owned(),
            name: "One".to_owned(),
            kind: ReleaseKind::Album,
            year: Some(2024),
            cover: None,
            tracks: vec!["spotify:track:1".to_owned(), "spotify:track:2".to_owned()],
        };
        let labels = vec![
            track("spotify:track:1", "One", "", 222_000),
            track("spotify:track:2", "Two", "", 198_000),
        ];
        let mut transport = Transport::default();
        let mut clicked = None;

        // The second row's rect inside a block drawn at the top of the pane.
        let mut row_rect = None;
        run(&ctx, input(size, Vec::new()), |ui| {
            let top = ui.cursor().min;
            let list_top = top.y + stroke::HAIRLINE + space::S5 + extent::RELEASE_ART + space::S4;
            let list = Rect::from_min_size(pos2(top.x, list_top), vec2(ui.available_width(), 2.0 * extent::ROW_H));
            row_rect = Some(table_layout::row_rect(list, 1, extent::ROW_H));
        });
        let row_rect = row_rect.expect("the block's body lays out");

        for _ in 0..2 {
            for event in click_events(row_rect.center()) {
                run(&ctx, input(size, vec![event]), |ui| {
                    let block = Block {
                        release: &release,
                        labels: Labels::Ready(&labels),
                        cover: Cover::Missing,
                        on_screen: true,
                        playable: true,
                    };
                    let out = super::release_block::show(ui, Table::Full, &block, &transport, &tokens);
                    clicked = Some(out);
                });
            }
        }

        assert_eq!(
            clicked.and_then(|clicked| clicked.row),
            Some(1),
            "the row under the double click is what plays"
        );
        let _ = &mut transport;
    }

    #[test]
    fn an_empty_artist_draws_both_empty_copies() {
        let ctx = context();
        let mut library = library_with(detail(Vec::new(), Vec::new()));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let (_, output) = frame(
            &ctx,
            Vec2::new(900.0, 800.0),
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let texts = drawn_texts(&output);

        assert!(
            texts.contains(&"No popular tracks."),
            "the Popular empty copy: {texts:?}"
        );
        assert!(
            texts.contains(&"No releases."),
            "the Discography empty copy: {texts:?}"
        );
        assert!(texts.contains(&"Popular"), "the section head: {texts:?}");
        assert!(texts.contains(&"Discography"), "{texts:?}");
    }

    #[test]
    fn a_failure_draws_the_message_and_a_retry_that_asks_again() {
        let ctx = context();
        let mut library = Library::default();
        library.open(Page::Artist(ARTIST.to_owned()));
        library.apply(&Update::ArtistFailed {
            artist: ARTIST.to_owned(),
            failure: Failure::Load,
        });
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let (_, output) = frame(
            &ctx,
            Vec2::new(900.0, 800.0),
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
            Vec2::new(900.0, 800.0),
            retry.center(),
            1,
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        assert_eq!(
            command,
            Some(Command::LoadArtist {
                artist: ARTIST.to_owned()
            }),
            "Retry re-issues the same load"
        );
        assert_eq!(library.artist, Load::Loading);
    }

    #[test]
    fn a_ready_artist_for_another_page_draws_the_loading_state() {
        let ctx = context();
        let mut library = Library::default();
        library.open(Page::Artist(ARTIST.to_owned()));
        // The state the library guard exists for: a ready detail left from an
        // earlier page.
        library.artist = Load::Ready(ArtistDetail {
            uri: OTHER_ARTIST.to_owned(),
            name: "Another Artist".to_owned(),
            ..detail(Vec::new(), Vec::new())
        });
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let (_, output) = frame(
            &ctx,
            Vec2::new(900.0, 800.0),
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let texts = drawn_texts(&output);

        assert!(
            texts.is_empty(),
            "the page paints no words for another artist: {texts:?}"
        );
        assert!(
            output
                .shapes
                .iter()
                .any(|clipped| matches!(clipped.shape, egui::epaint::Shape::Path(_))),
            "the loading state's spinner arc is painted"
        );
    }
}
