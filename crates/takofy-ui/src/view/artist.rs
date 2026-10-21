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
//!
//! The filter row hides a block entirely rather than drawing it shorter: a block
//! the chosen chip does not match is skipped before its rect is solved, so it
//! reserves no space and asks for no labels and no cover.

use std::time::Instant;

use egui::{Id, Rect, Sense, Ui, Vec2, vec2};
use takofy_model::{FollowedArtist, Release};

use crate::cover::{Art, Cover};
use crate::format::{discography_meta, format_position};
use crate::layout::{Table, cards, table as table_layout};
use crate::library::{Library, Load};
use crate::message::Command;
use crate::player::Transport;
use crate::queue::Queue;
use crate::releases::Labels;
use crate::route::Page;
use crate::scroll;
use crate::theme::{Tokens, extent, space};
use crate::view::text as recipe;

use super::artist_hero::{self, ArtistHero};
use super::carry::{self, Ghost};
use super::filter_row;
use super::media;
use super::release_block::{self, Block};
use super::section;
use super::skeleton;
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
    // A card press names an artist; opening it needs the library, which the page
    // has borrowed for the whole draw, so it waits for the scroll area to close.
    let mut open: Option<String> = None;

    // The scroll offset is this artist's, not the album's or the playlist's that
    // share the pane.
    egui::ScrollArea::vertical()
        .id_salt(("artist", uri.as_str()))
        .show(ui, |ui| {
            scroll::follow_drag(ui);
            let page = library.artist_page();
            match page.load {
                Load::Idle => {}
                Load::Loading => {
                    skeleton::rows(ui, skeleton::SCREENFUL, table.columns().for_album(), tokens);
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
                        skeleton::rows(
                            ui,
                            skeleton::SCREENFUL,
                            table.columns().for_album(),
                            tokens,
                        );
                        return;
                    };

                    let hero = ArtistHero {
                        name: &artist.name,
                        portrait: art.look(artist.portrait.as_ref(), now),
                        playable: !artist.top_tracks.is_empty(),
                        follow: page.follow.label(),
                        follow_live: page.follow.pressable(),
                        follow_failed: page.follow.failed(),
                    };
                    let pressed = artist_hero::show(ui, &hero, tokens);
                    if pressed.followed {
                        command = page.follow.pressed();
                    }
                    if pressed.played {
                        command = queue.start(&artist.top_tracks, 0, transport);
                    }

                    super::inner(ui, |ui| {
                        let width = ui.available_width();
                        popular(
                            ui,
                            table,
                            width,
                            artist,
                            transport,
                            queue,
                            tokens,
                            &mut command,
                        );

                        let tracks: usize = artist
                            .releases
                            .iter()
                            .map(|release| release.tracks.len())
                            .sum();
                        section::head(
                            ui,
                            "Discography",
                            &discography_meta(artist.releases.len(), tracks),
                            tokens,
                        );
                        if artist.releases.is_empty() {
                            ui.label(recipe::muted("No releases.", tokens));
                        } else {
                            // The row the artist's own kinds allow, and the chip it
                            // is filtered by. The effective chip, never the raw
                            // choice: a choice whose kind this artist does not have
                            // would otherwise hide every block.
                            let row = crate::discography::chips(&artist.releases);
                            let active = page.filter.effective(&row);
                            if row.len() > 1
                                && let Some(picked) = filter_row::show(ui, tokens, &row, active)
                            {
                                page.filter.choose(picked);
                            }

                            for release in artist
                                .releases
                                .iter()
                                .filter(|release| active.matches(release.kind))
                            {
                                let on_screen = ui.clip_rect().intersects(block_rect(ui, release));
                                if !on_screen {
                                    let block = Block {
                                        release,
                                        labels: Labels::Idle,
                                        cover: Cover::Missing,
                                        on_screen: false,
                                        playable: false,
                                    };
                                    let _ =
                                        release_block::show(ui, table, &block, transport, tokens);
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
                                let clicked =
                                    release_block::show(ui, table, &block, transport, tokens);
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

                        if let Some(artist) = fans(ui, page.related, art, now, tokens) {
                            open = Some(artist);
                        }
                    });
                }
            }
        });

    if retry {
        return library.open(Page::Artist(uri));
    }
    if let Some(artist) = open {
        command = library.open(Page::Artist(artist));
    }
    command
}

/// The likeness row: the artists this one is heard with, as round cards.
///
/// The row is the page's second read, so it is absent until the cards land and
/// absent for good when the artist names nobody. A press opens that artist.
fn fans(
    ui: &mut Ui,
    related: &Load<Vec<FollowedArtist>>,
    art: &mut Art,
    now: Instant,
    tokens: &Tokens,
) -> Option<String> {
    let cards = match related {
        Load::Ready(cards) if !cards.is_empty() => cards,
        Load::Idle | Load::Loading | Load::Ready(_) | Load::Failed(_) => return None,
    };

    ui.add_space(space::S6);
    section::head(ui, "Fans also like", &cards.len().to_string(), tokens);

    let solved = cards::grid(
        Rect::from_min_size(ui.cursor().min, vec2(ui.available_width(), 0.0)),
        extent::MEDIA_MIN_W,
        space::S3,
        media::extra_height(ui),
    );
    let rows = solved.rows(cards.len());
    let height =
        rows as f32 * solved.card_rect(0).height() + (rows.saturating_sub(1)) as f32 * space::S3;
    let list = Rect::from_min_size(ui.cursor().min, vec2(ui.available_width(), height));
    ui.allocate_rect(list, egui::Sense::hover());
    let visible = ui.clip_rect();

    let mut pressed = None;
    for (index, card) in cards.iter().enumerate() {
        let rect = solved.card_rect(index);
        if !visible.intersects(rect) {
            continue;
        }
        let media = media::Media {
            uri: &card.uri,
            name: &card.name,
            kind: "Artist",
            sub: "Artist",
            size: "",
            round: true,
            cover: art.look(card.portrait.as_ref(), now),
        };
        if media::tile(ui, rect, &media, tokens) {
            pressed = Some(card.uri.clone());
        }
    }
    pressed
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
    let mut selection = carry::List::open(
        ui.ctx(),
        Id::unique(("artist-popular", artist.uri.as_str())),
        count,
    );

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
            look: selection.look(index),
        };
        match track_row::show(ui, rect, columns, &row, tokens) {
            Some(Click::Play) => {
                *command = queue.start(&artist.top_tracks, index, transport);
            }
            Some(Click::Select(press)) => selection.press(index, press),
            Some(Click::Lift) => selection.lift(
                index,
                entry.clone(),
                rect.size(),
                Ghost::Table(row.lifted(columns)),
            ),
            Some(Click::Open(_)) | None => {}
        }
    }
    selection.finish(ui.ctx());
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
    use takofy_model::{
        ArtistDetail, CoverUrl, Release, Release as _Release, ReleaseKind, TrackEntry,
    };

    use super::*;
    use crate::cover::Cover;
    use crate::discography::{Chip, chips};
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
        release_of_kind(uri, name, ReleaseKind::Album, tracks)
    }

    fn release_of_kind(uri: &str, name: &str, kind: ReleaseKind, tracks: &[&str]) -> Release {
        Release {
            uri: uri.to_owned(),
            name: name.to_owned(),
            kind,
            year: Some(2024),
            cover: None,
            tracks: tracks.iter().map(|track| (*track).to_owned()).collect(),
        }
    }

    /// The rect a chip occupies, recovered from the label it painted: a chip's
    /// label sits at the chip's top-left plus its padding.
    fn chip_rect(output: &egui::FullOutput, chip: Chip) -> Option<Rect> {
        let label = crate::format::filter_label(chip).to_uppercase();
        let text = text_rect(output, &label)?;
        Some(text.expand2(Vec2::new(extent::CHIP_PAD_X, extent::CHIP_PAD_Y)))
    }

    /// The pane every block of the three-release fixture fits in.
    fn tall() -> Vec2 {
        Vec2::new(900.0, 1500.0)
    }

    /// Two albums and one EP: two chips beside `All`, and three blocks.
    fn mixed() -> Vec<Release> {
        vec![
            release_of_kind(
                "spotify:album:r1",
                "Album One",
                ReleaseKind::Album,
                &["spotify:track:1"],
            ),
            release_of_kind(
                "spotify:ep:r2",
                "An EP",
                ReleaseKind::Ep,
                &["spotify:track:2"],
            ),
            release_of_kind(
                "spotify:album:r3",
                "Album Two",
                ReleaseKind::Album,
                &["spotify:track:3"],
            ),
        ]
    }

    /// One release of every kind, so the row has all five chips.
    fn every_kind() -> Vec<Release> {
        vec![
            release_of_kind("spotify:a", "An Album", ReleaseKind::Album, &["t1"]),
            release_of_kind("spotify:b", "An EP", ReleaseKind::Ep, &["t2"]),
            release_of_kind("spotify:c", "A Single", ReleaseKind::Single, &["t3"]),
            release_of_kind(
                "spotify:d",
                "A Compilation",
                ReleaseKind::Compilation,
                &["t4"],
            ),
        ]
    }

    /// The sizes the five chips need, as the row measures them.
    fn chip_sizes(ctx: &Context) -> Vec<Vec2> {
        let tokens = Tokens::DARK;
        let row = chips(&every_kind());
        let mut sizes = Vec::new();
        run(ctx, input(Vec2::new(900.0, 800.0), Vec::new()), |ui| {
            sizes = row
                .iter()
                .map(|chip| {
                    crate::view::controls::chip_size(
                        ui,
                        &tokens,
                        crate::format::filter_label(*chip),
                    )
                })
                .collect();
        });
        sizes
    }

    fn drawn(output: &egui::FullOutput, name: &str) -> bool {
        drawn_texts(output).contains(&name)
    }

    fn cover(tail: &str) -> CoverUrl {
        CoverUrl::from_wire(&format!("https://i.scdn.co/image/{tail}")).expect("a cover url")
    }

    const PAGE: Vec2 = Vec2::new(900.0, 1500.0);

    fn detail(top: Vec<TrackEntry>, releases: Vec<Release>) -> ArtistDetail {
        ArtistDetail {
            uri: ARTIST.to_owned(),
            name: "Vela Moreno".to_owned(),
            portrait: None,
            top_tracks: top,
            releases,
            related: Vec::new(),
        }
    }

    /// The likeness row draws under the discography, and a press opens that
    /// artist's page.
    #[test]
    fn the_likeness_row_draws_its_cards_and_a_press_opens_the_artist() {
        let ctx = context();
        let mut detail = detail(
            vec![track("spotify:track:t", "Top", "Vela", 200_000)],
            vec![release(
                "spotify:album:a",
                "Album One",
                &["spotify:track:t"],
            )],
        );
        detail.related = vec!["spotify:artist:other".to_owned()];
        let mut library = library_with(detail);
        // The cards are the page's second read, so the request comes first.
        let _ = library.related_request();
        library.apply(&Update::Related {
            artist: ARTIST.to_owned(),
            cards: vec![FollowedArtist {
                uri: "spotify:artist:other".to_owned(),
                name: "Other Artist".to_owned(),
                named: true,
                portrait: None,
                releases: None,
            }],
        });
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let (_, output) = frame(
            &ctx,
            PAGE,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let texts: Vec<&str> = drawn_texts(&output);
        assert!(texts.contains(&"Fans also like"), "{texts:?}");
        assert!(texts.contains(&"Other Artist"), "{texts:?}");

        let card = text_rect(&output, "Other Artist").expect("the card's name");
        let (command, _) = frame(
            &ctx,
            PAGE,
            crate::view::test_support::click_events(card.center()).to_vec(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        assert_eq!(
            command,
            Some(Command::LoadArtist {
                artist: "spotify:artist:other".to_owned()
            }),
            "a card opens that artist"
        );
    }

    /// An artist the wire names nobody for draws no section at all, and one with
    /// no releases still draws the row after its empty copy.
    #[test]
    fn the_likeness_row_is_absent_without_cards_and_survives_an_empty_discography() {
        let ctx = context();
        let mut bare = detail(vec![], vec![]);
        bare.related = vec!["spotify:artist:other".to_owned()];
        let mut library = library_with(bare);
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let (_, output) = frame(
            &ctx,
            PAGE,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        assert!(!drawn_texts(&output).contains(&"Fans also like"));

        let _ = library.related_request();
        library.apply(&Update::Related {
            artist: ARTIST.to_owned(),
            cards: vec![FollowedArtist {
                uri: "spotify:artist:other".to_owned(),
                name: "Other Artist".to_owned(),
                named: true,
                portrait: None,
                releases: None,
            }],
        });
        let (_, output) = frame(
            &ctx,
            PAGE,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let texts = drawn_texts(&output);
        assert!(texts.contains(&"No releases."), "{texts:?}");
        assert!(
            texts.contains(&"Fans also like"),
            "the row survives an empty discography: {texts:?}"
        );
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

    #[expect(
        clippy::too_many_arguments,
        reason = "one test click needs the frame's whole state"
    )]
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
            let (band, _) = ui.allocate_exact_size(
                vec2(ui.available_width(), hero::ARTIST_MIN_H),
                Sense::hover(),
            );
            let (row, _) = ui.allocate_exact_size(
                vec2(ui.available_width(), hero::PLAY + 2.0 * space::S4),
                Sense::hover(),
            );
            let play = Rect::from_min_size(
                Pos2::new(
                    row.left() + space::GUTTER,
                    row.center().y - hero::PLAY / 2.0,
                ),
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
            let _ = ui.allocate_exact_size(
                vec2(ui.available_width(), hero::ARTIST_MIN_H),
                Sense::hover(),
            );
            let _ = ui.allocate_exact_size(
                vec2(ui.available_width(), hero::PLAY + 2.0 * space::S4),
                Sense::hover(),
            );
            section::head(ui, "Popular", "", &tokens);
            let columns =
                table_layout::columns(ui.available_width(), Table::Full.columns().for_album());
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

        assert!(
            texts.contains(&"Vela Moreno"),
            "the hero's title: {texts:?}"
        );
        assert!(texts.contains(&"ARTIST"), "the hero's eyebrow: {texts:?}");
    }

    /// Whether `artist`'s popular list holds `index` as part of its selection.
    fn picked(ctx: &Context, artist: &str, index: usize) -> bool {
        crate::view::carry::List::open(ctx, egui::Id::unique(("artist-popular", artist)), 2)
            .look(index)
            .selected
    }

    /// Whether one release block's list holds `index` as part of its selection.
    fn release_picked(ctx: &Context, release: &str, index: usize) -> bool {
        crate::view::carry::List::open(ctx, egui::Id::unique(("release-tracks", release)), 1)
            .look(index)
            .selected
    }

    #[test]
    fn a_single_click_selects_and_a_double_click_plays_it() {
        let ctx = context();
        let size = Vec2::new(900.0, 800.0);
        let top = || {
            vec![
                track("spotify:track:a", "One", "Vela Moreno", 222_000),
                track("spotify:track:b", "Two", "Vela Moreno", 198_000),
            ]
        };
        let mut library = library_with(detail(top(), Vec::new()));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let rect = popular_row_rect(&ctx, size, 1, 2);
        let command = click(
            &ctx,
            size,
            rect.center(),
            1,
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );

        assert_eq!(command, None, "a single click plays nothing");
        assert!(transport.now_playing.is_none(), "and starts nothing");
        assert!(picked(&ctx, ARTIST, 1), "the row under the click is picked");
        assert!(!picked(&ctx, ARTIST, 0), "and only that row");

        // A fresh page for the double click: a third click inside the window is
        // a triple click, and that plays nothing.
        let ctx = context();
        let mut library = library_with(detail(top(), Vec::new()));
        let mut transport = Transport::default();
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
            "a double click starts the track"
        );
    }

    #[test]
    fn release_blocks_keep_their_own_selection_until_navigation() {
        let ctx = context();
        let mut library = library_with(detail(
            Vec::new(),
            vec![
                release("spotify:album:r1", "Album One", &["spotify:track:1"]),
                release("spotify:album:r2", "Album Two", &["spotify:track:2"]),
            ],
        ));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        // The blocks ask for their labels, then the answers land.
        let _ = frame(
            &ctx,
            tall(),
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        assert_eq!(
            library.releases.requests().len(),
            2,
            "both blocks ask for their labels"
        );
        for (release_uri, title) in [
            ("spotify:album:r1", "First One"),
            ("spotify:album:r2", "Second One"),
        ] {
            library.apply(&Update::ReleaseTracks {
                release: release_uri.to_owned(),
                tracks: vec![track(
                    &format!("spotify:track:{release_uri}"),
                    title,
                    "Vela Moreno",
                    200_000,
                )],
            });
        }

        let (_, output) = frame(
            &ctx,
            tall(),
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        assert!(drawn(&output, "First One"), "the first block's row");
        assert!(drawn(&output, "Second One"), "the second block's row");
        let row = text_rect(&output, "Second One").expect("the second block's row");

        let command = click(
            &ctx,
            tall(),
            row.center(),
            1,
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        assert_eq!(command, None, "a single click plays nothing");
        assert!(
            release_picked(&ctx, "spotify:album:r2", 0),
            "the block under the click picks its row"
        );
        assert!(
            !release_picked(&ctx, "spotify:album:r1", 0),
            "the other block keeps its own selection"
        );

        let _ = library.open(Page::Album("spotify:album:other".to_owned()));
        crate::view::carry::follow_page(&ctx, library.page());

        assert!(
            !release_picked(&ctx, "spotify:album:r1", 0),
            "navigation forgets the first block's selection"
        );
        assert!(
            !release_picked(&ctx, "spotify:album:r2", 0),
            "and the second's"
        );
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
    fn a_follow_failure_draws_its_copy_beside_the_confirmed_label() {
        let ctx = context();
        let size = Vec2::new(900.0, 800.0);
        let mut library = library_with(detail(Vec::new(), Vec::new()));
        library.apply(&Update::Followed {
            artist: ARTIST.to_owned(),
            followed: true,
        });
        let _ = library.follow.pressed();
        library.apply(&Update::FollowFailed {
            artist: ARTIST.to_owned(),
            failure: Failure::Save,
        });
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let (_, output) = frame(
            &ctx,
            size,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let texts = drawn_texts(&output);
        let copy = crate::signin::failure_message(Failure::Save);
        assert!(
            texts.contains(&copy),
            "the failed write's copy is drawn: {texts:?}"
        );
        assert!(
            texts.contains(&"Following"),
            "the label stays where the server last left it: {texts:?}"
        );

        // Beside the button, on its own centre line, not over the tools.
        let (_, follow) = hero_rects(&ctx, size);
        let hint = text_rect(&output, copy).expect("the failure's copy is painted");
        assert!(
            hint.left() > follow.left(),
            "the copy sits beside the button: {hint:?} against {follow:?}"
        );
        assert!(
            (hint.center().y - follow.center().y).abs() < 1.0,
            "the copy shares the button's centre line: {hint:?} against {follow:?}"
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

    /// A realistic discography: forty-eight releases, far more than fits. The
    /// frame must draw, and must ask only for the blocks it can show.
    #[test]
    fn a_long_discography_asks_only_for_what_is_on_screen() {
        let ctx = context();
        // 900 tall, not 800: the filter row takes its own height above the
        // blocks, and at 800 only one block fits, which would not exercise the
        // two-request cap.
        let size = Vec2::new(900.0, 900.0);
        let releases: Vec<Release> = (1..=48)
            .map(|index| {
                release(
                    &format!("spotify:album:r{index}"),
                    &format!("Release {index}"),
                    &["spotify:track:a", "spotify:track:b"],
                )
            })
            .collect();
        let mut library = library_with(detail(Vec::new(), releases));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let (_, output) = frame(
            &ctx,
            size,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );

        assert!(
            drawn_texts(&output).contains(&"Release 1"),
            "the first block's head is drawn"
        );
        let requests = library.releases.requests();
        assert_eq!(
            requests.len(),
            2,
            "the top of the discography is what fits, and the cap holds: {requests:?}"
        );
        for command in &requests {
            let Command::LoadReleaseTracks { release, .. } = command else {
                panic!("a label drain emits only label requests: {command:?}");
            };
            assert!(
                release == "spotify:album:r1" || release == "spotify:album:r2",
                "a block far below the fold was asked for: {release}"
            );
        }
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
            let list = Rect::from_min_size(
                pos2(top.x, list_top),
                vec2(ui.available_width(), 2.0 * extent::ROW_H),
            );
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
                    let out =
                        super::release_block::show(ui, Table::Full, &block, &transport, &tokens);
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

    /// A debug build paints egui's `Unaligned` marker inside any widget whose
    /// rect is off the rounding grid, which makes the chip row unreadable. The
    /// page must draw none.
    #[test]
    fn the_page_paints_no_unaligned_marker() {
        let ctx = context();
        let mut library = library_with(detail(
            vec![track(
                "spotify:track:1",
                "Slow Tide",
                "Vela Moreno",
                222_000,
            )],
            vec![
                release("spotify:album:r1", "Slow Tide", &["spotify:track:1"]),
                release("spotify:album:r2", "Kestrel", &["spotify:track:1"]),
            ],
        ));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let art = &mut Art::default();

        let (_, output) = frame(
            &ctx,
            Vec2::new(900.0, 800.0),
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            art,
        );

        assert!(
            !drawn_texts(&output).contains(&"Unaligned"),
            "the page paints an unaligned widget: {:?}",
            drawn_texts(&output)
        );
    }

    /// The sections sit one gutter inside the pane, so a release's art does not
    /// rub the panel's edge and the count does not touch the far one. The hero
    /// stays full-bleed above them.
    #[test]
    fn the_sections_sit_one_gutter_inside_the_pane() {
        let ctx = context();
        let mut library = library_with(detail(
            vec![track(
                "spotify:track:1",
                "Slow Tide",
                "Vela Moreno",
                222_000,
            )],
            vec![release(
                "spotify:album:r1",
                "Slow Tide",
                &["spotify:track:1"],
            )],
        ));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let art = &mut Art::default();

        let probe = Vec2::new(900.0, 800.0);
        let (_, output) = frame(
            &ctx,
            probe,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            art,
        );

        let popular = text_rect(&output, "Popular").expect("the section head is painted");
        assert!(
            (popular.left() - space::GUTTER).abs() < 0.01,
            "the section title sits at {}, the pane's gutter is {}",
            popular.left(),
            space::GUTTER
        );

        let aside = drawn_texts(&output)
            .into_iter()
            .find(|text| text.contains("RELEASE"))
            .expect("the discography counts its releases");
        let aside = text_rect(&output, aside).expect("the aside is painted");
        assert!(
            (aside.right() - (probe.x - space::GUTTER)).abs() < 0.01,
            "the aside ends at {}, the pane's far gutter starts at {}",
            aside.right(),
            probe.x - space::GUTTER
        );

        // The hero above them is still full-bleed: its title starts at the
        // gutter because the band pads itself, and its photograph spans the pane.
        let title = text_rect(&output, "Vela Moreno").expect("the hero title is painted");
        assert!(
            (title.left() - space::GUTTER).abs() < 0.01,
            "the hero title sits at {}",
            title.left()
        );
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
            output.shapes.iter().any(|clipped| matches!(
                &clipped.shape,
                egui::epaint::Shape::Rect(rect) if rect.fill == Tokens::DARK.palette.skeleton_rule
            )),
            "the page holds its rows open instead of naming another artist"
        );
    }

    #[test]
    fn the_row_has_all_plus_the_kinds_the_artist_has() {
        let ctx = context();
        let mut library = library_with(detail(Vec::new(), mixed()));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let (_, output) = frame(
            &ctx,
            tall(),
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let texts = drawn_texts(&output);

        assert!(texts.contains(&"ALL"), "{texts:?}");
        assert!(texts.contains(&"ALBUMS"), "{texts:?}");
        assert!(texts.contains(&"EPS"), "{texts:?}");
        assert!(!texts.contains(&"SINGLES"), "no single: {texts:?}");
        assert!(
            !texts.contains(&"COMPILATIONS"),
            "no compilation: {texts:?}"
        );
    }

    #[test]
    fn the_row_sits_between_the_head_and_the_first_block() {
        let ctx = context();
        let mut library = library_with(detail(Vec::new(), mixed()));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let (_, output) = frame(
            &ctx,
            tall(),
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );

        let head = text_rect(&output, "Discography").expect("the section head");
        let first_chip = chip_rect(&output, Chip::All).expect("the All chip");
        let first_block = text_rect(&output, "Album One").expect("the first block");
        assert!(
            head.bottom() < first_chip.top(),
            "the row is under the head: {head:?} against {first_chip:?}"
        );
        assert!(
            first_chip.bottom() < first_block.top(),
            "the row is above the first block: {first_chip:?} against {first_block:?}"
        );
    }

    #[test]
    fn all_is_pressed_on_first_draw() {
        let ctx = context();
        let releases = mixed();
        let mut library = library_with(detail(Vec::new(), releases.clone()));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let (_, output) = frame(
            &ctx,
            tall(),
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );

        assert_eq!(library.filter.chosen(), Chip::All);
        assert_eq!(
            library.filter.effective(&chips(&releases)),
            Chip::All,
            "the pressed chip is All, so every block draws"
        );
        for name in ["Album One", "An EP", "Album Two"] {
            assert!(drawn(&output, name), "{name} is drawn: {name}");
        }
    }

    #[test]
    fn opening_another_artist_draws_all_even_when_it_has_the_same_kind() {
        let ctx = context();
        let releases = mixed();
        let mut library = library_with(detail(Vec::new(), releases.clone()));
        library.filter.choose(Chip::Kind(ReleaseKind::Ep));
        let mut other = detail(Vec::new(), releases);
        other.uri = OTHER_ARTIST.to_owned();
        library.open(Page::Artist(other.uri.clone()));
        library.apply(&Update::Artist(other));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let (_, output) = frame(
            &ctx,
            tall(),
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );

        assert_eq!(
            library.filter.chosen(),
            Chip::All,
            "another artist's page starts on All"
        );
        assert!(
            drawn(&output, "EPS"),
            "the other artist has an EP chip too: {:?}",
            drawn_texts(&output)
        );
        for name in ["Album One", "An EP", "Album Two"] {
            assert!(
                drawn(&output, name),
                "All filters nothing on the new artist: {name}"
            );
        }
    }

    #[test]
    fn pressing_a_chip_filters_the_blocks_in_place() {
        let ctx = context();
        let size = tall();
        let mut library = library_with(detail(Vec::new(), mixed()));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let (_, before) = frame(
            &ctx,
            size,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let albums = chip_rect(&before, Chip::Kind(ReleaseKind::Album)).expect("the Albums chip");
        let page = library.page().cloned();
        let can_back = library.can_back();

        let command = click(
            &ctx,
            size,
            albums.center(),
            1,
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );

        assert_eq!(command, None, "a filter press asks for nothing");
        assert_eq!(library.page(), page.as_ref(), "it does not navigate");
        assert_eq!(library.can_back(), can_back, "it adds no history entry");
        assert!(
            matches!(library.artist, Load::Ready(_)),
            "the artist is not refetched"
        );
        assert_eq!(
            library.filter.chosen(),
            Chip::Kind(ReleaseKind::Album),
            "the pressed chip is the choice"
        );

        let (_, after) = frame(
            &ctx,
            size,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        assert!(drawn(&after, "Album One"), "an album block stays");
        assert!(drawn(&after, "Album Two"), "every album block stays");
        assert!(
            !drawn(&after, "An EP"),
            "the EP block draws nothing at all: {:?}",
            drawn_texts(&after)
        );
    }

    #[test]
    fn pressing_the_pressed_chip_does_nothing() {
        let ctx = context();
        let size = tall();
        let mut library = library_with(detail(Vec::new(), mixed()));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let (_, before) = frame(
            &ctx,
            size,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let albums = chip_rect(&before, Chip::Kind(ReleaseKind::Album)).expect("the Albums chip");
        let _ = click(
            &ctx,
            size,
            albums.center(),
            1,
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let _ = frame(
            &ctx,
            size,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );

        let _ = click(
            &ctx,
            size,
            albums.center(),
            1,
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let (_, after) = frame(
            &ctx,
            size,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );

        assert_eq!(
            library.filter.chosen(),
            Chip::Kind(ReleaseKind::Album),
            "the row is exclusive: a second press is not a way back out"
        );
        assert!(!drawn(&after, "An EP"), "the filter is still applied");
    }

    #[test]
    fn all_restores_every_block() {
        let ctx = context();
        let size = tall();
        let mut library = library_with(detail(Vec::new(), mixed()));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let (_, before) = frame(
            &ctx,
            size,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let albums = chip_rect(&before, Chip::Kind(ReleaseKind::Album)).expect("the Albums chip");
        let all = chip_rect(&before, Chip::All).expect("the All chip");
        let _ = click(
            &ctx,
            size,
            albums.center(),
            1,
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let _ = frame(
            &ctx,
            size,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );

        let _ = click(
            &ctx,
            size,
            all.center(),
            1,
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let (_, after) = frame(
            &ctx,
            size,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );

        assert_eq!(library.filter.chosen(), Chip::All);
        for name in ["Album One", "An EP", "Album Two"] {
            assert!(drawn(&after, name), "{name} is back");
        }
    }

    #[test]
    fn a_hidden_block_draws_nothing_and_reserves_nothing() {
        let ctx = context();
        let size = tall();
        let mut unfiltered = library_with(detail(Vec::new(), mixed()));
        let mut filtered = library_with(detail(Vec::new(), mixed()));
        filtered.filter.choose(Chip::Kind(ReleaseKind::Ep));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let (_, before) = frame(
            &ctx,
            size,
            Vec::new(),
            &mut unfiltered,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let (_, after) = frame(
            &ctx,
            size,
            Vec::new(),
            &mut filtered,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let texts = drawn_texts(&after);

        assert!(!texts.contains(&"Album One"), "{texts:?}");
        assert!(!texts.contains(&"Album Two"), "{texts:?}");

        let first_unfiltered = text_rect(&before, "Album One").expect("the first block");
        let first_filtered = text_rect(&after, "An EP").expect("the only block");
        assert!(
            (first_filtered.top() - first_unfiltered.top()).abs() < 0.01,
            "the hidden blocks reserved no space: {} against {}",
            first_filtered.top(),
            first_unfiltered.top()
        );
    }

    #[test]
    fn a_hidden_block_has_no_wants() {
        let ctx = context();
        let size = tall();
        let mut releases = mixed();
        releases[0].cover = Some(cover("album"));
        releases[1].cover = Some(cover("ep"));
        let now = Instant::now();

        // The control: unfiltered, both blocks are on screen and ask.
        let mut open = library_with(detail(Vec::new(), releases.clone()));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();
        art.begin_frame();
        let (_, output) = frame(
            &ctx,
            size,
            Vec::new(),
            &mut open,
            &mut transport,
            &mut queue,
            &mut art,
        );
        assert!(drawn(&output, "An EP"), "the EP block is on screen");
        assert!(drawn(&output, "Album One"), "the album block is on screen");
        let wanted_labels = open.releases.requests();
        let wanted_covers = art.requests(now);
        assert!(
            wanted_labels
                .iter()
                .any(|command| matches!(command, Command::LoadReleaseTracks { release, .. } if release == "spotify:album:r1")),
            "the control frame asks for the album's labels: {wanted_labels:?}"
        );
        assert!(
            wanted_covers
                .iter()
                .any(|command| matches!(command, Command::Fetch { url } if *url == cover("album"))),
            "the control frame asks for the album's cover: {wanted_covers:?}"
        );

        // The hidden album asks for neither.
        let mut hidden = library_with(detail(Vec::new(), releases));
        hidden.filter.choose(Chip::Kind(ReleaseKind::Ep));
        let mut art = Art::default();
        art.begin_frame();
        let _ = frame(
            &ctx,
            size,
            Vec::new(),
            &mut hidden,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let wanted_labels = hidden.releases.requests();
        let wanted_covers = art.requests(now);
        assert!(
            !wanted_labels.iter().any(|command| matches!(
                command,
                Command::LoadReleaseTracks { release, .. } if release == "spotify:album:r1"
            )),
            "a hidden block asks for no labels: {wanted_labels:?}"
        );
        assert!(
            !wanted_covers
                .iter()
                .any(|command| matches!(command, Command::Fetch { url } if *url == cover("album"))),
            "a hidden block fetches no cover: {wanted_covers:?}"
        );
        assert!(
            wanted_covers
                .iter()
                .any(|command| matches!(command, Command::Fetch { url } if *url == cover("ep"))),
            "the block the filter keeps still fetches its cover: {wanted_covers:?}"
        );
    }

    #[test]
    fn an_on_screen_matching_block_still_asks_for_labels() {
        let ctx = context();
        let size = tall();
        let mut library = library_with(detail(Vec::new(), mixed()));
        library.filter.choose(Chip::Kind(ReleaseKind::Ep));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let (_, output) = frame(
            &ctx,
            size,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );

        assert!(drawn(&output, "An EP"));
        assert_eq!(
            library.releases.requests(),
            vec![Command::LoadReleaseTracks {
                release: "spotify:ep:r2".to_owned(),
                tracks: vec!["spotify:track:2".to_owned()],
            }],
            "the block the filter keeps is still labelled"
        );
    }

    #[test]
    fn the_aside_keeps_the_artists_totals_while_filtered() {
        let ctx = context();
        let size = tall();
        let mut library = library_with(detail(Vec::new(), mixed()));
        library.filter.choose(Chip::Kind(ReleaseKind::Album));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let (_, output) = frame(
            &ctx,
            size,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );

        let aside = discography_meta(3, 3).to_uppercase();
        assert!(
            drawn(&output, &aside),
            "the aside describes the artist, not the filter: {aside}"
        );
    }

    #[test]
    fn a_choice_with_no_chip_after_a_reload_draws_all() {
        let ctx = context();
        let size = tall();
        let mut library = library_with(detail(Vec::new(), mixed()));
        library.filter.choose(Chip::Kind(ReleaseKind::Single));

        // A reload of the same artist: home, then Back, then the reply.
        library.to_playlists();
        let _ = library.back();
        library.apply(&Update::Artist(detail(Vec::new(), mixed())));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let (_, output) = frame(
            &ctx,
            size,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );

        for name in ["Album One", "An EP", "Album Two"] {
            assert!(
                drawn(&output, name),
                "{name} draws under the fallback to All"
            );
        }
        assert_eq!(
            library.filter.chosen(),
            Chip::Kind(ReleaseKind::Single),
            "the choice is remembered, not replaced"
        );
    }

    #[test]
    fn a_single_kind_artist_still_gets_all_and_that_kind() {
        let ctx = context();
        let mut library = library_with(detail(
            Vec::new(),
            vec![release("spotify:album:r1", "One", &["spotify:track:1"])],
        ));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let (_, output) = frame(
            &ctx,
            tall(),
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );

        assert!(drawn(&output, "ALL"), "{:?}", drawn_texts(&output));
        assert!(drawn(&output, "ALBUMS"), "{:?}", drawn_texts(&output));
        assert!(!drawn(&output, "EPS"), "{:?}", drawn_texts(&output));
    }

    #[test]
    fn an_artist_with_only_other_releases_draws_no_row() {
        let ctx = context();
        let mut library = library_with(detail(
            Vec::new(),
            vec![release_of_kind(
                "spotify:audiobook:r1",
                "A Book",
                ReleaseKind::Other,
                &["spotify:track:1"],
            )],
        ));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let (_, output) = frame(
            &ctx,
            tall(),
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );

        assert!(
            !drawn(&output, "ALL"),
            "a row of All alone filters nothing: {:?}",
            drawn_texts(&output)
        );
        assert!(
            drawn(&output, "A Book"),
            "the Other release stays reachable under All"
        );
    }

    #[test]
    fn no_releases_draws_no_row() {
        let ctx = context();
        let mut library = library_with(detail(Vec::new(), Vec::new()));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let (_, output) = frame(
            &ctx,
            tall(),
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let texts = drawn_texts(&output);

        assert!(texts.contains(&"No releases."), "{texts:?}");
        assert!(
            !texts.contains(&"ALL"),
            "there is nothing to filter: {texts:?}"
        );
    }

    #[test]
    fn the_chip_row_starts_at_the_gutter() {
        let ctx = context();
        let mut library = library_with(detail(Vec::new(), mixed()));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let (_, output) = frame(
            &ctx,
            tall(),
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );

        let all = chip_rect(&output, Chip::All).expect("the All chip");
        assert!(
            (all.left() - space::GUTTER).abs() < 0.01,
            "the row starts at {}, the gutter is {}",
            all.left(),
            space::GUTTER
        );
    }

    #[test]
    fn at_a_wide_pane_the_row_is_one_line() {
        let ctx = context();
        let mut library = library_with(detail(Vec::new(), every_kind()));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let sizes = chip_sizes(&ctx);
        let needed: f32 =
            sizes.iter().map(|size| size.x).sum::<f32>() + space::S1 * (sizes.len() - 1) as f32;
        let wide = needed + 2.0 * space::GUTTER + 32.0;

        let (_, output) = frame(
            &ctx,
            Vec2::new(wide, 1500.0),
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );

        let tops: Vec<f32> = [
            Chip::All,
            Chip::Kind(ReleaseKind::Album),
            Chip::Kind(ReleaseKind::Ep),
            Chip::Kind(ReleaseKind::Single),
            Chip::Kind(ReleaseKind::Compilation),
        ]
        .into_iter()
        .filter_map(|chip| chip_rect(&output, chip))
        .map(|rect| rect.top())
        .collect();
        assert_eq!(tops.len(), 5, "every chip is drawn");
        for top in &tops {
            assert!(
                (top - tops[0]).abs() < 0.01,
                "one line: {tops:?} at {wide} wide"
            );
        }
    }

    #[test]
    fn at_a_narrow_pane_the_row_wraps_inside_the_gutters() {
        let ctx = context();
        let mut library = library_with(detail(Vec::new(), every_kind()));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let sizes = chip_sizes(&ctx);
        let needed: f32 =
            sizes.iter().map(|size| size.x).sum::<f32>() + space::S1 * (sizes.len() - 1) as f32;
        let narrow = 2.0 * space::GUTTER + needed / 2.0;

        let (_, output) = frame(
            &ctx,
            Vec2::new(narrow, 1500.0),
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );

        let rects: Vec<Rect> = [
            Chip::All,
            Chip::Kind(ReleaseKind::Album),
            Chip::Kind(ReleaseKind::Ep),
            Chip::Kind(ReleaseKind::Single),
            Chip::Kind(ReleaseKind::Compilation),
        ]
        .into_iter()
        .filter_map(|chip| chip_rect(&output, chip))
        .collect();
        assert_eq!(rects.len(), 5, "every chip is drawn");
        let first = rects[0].top();
        assert!(
            rects.iter().any(|rect| (rect.top() - first).abs() > 0.01),
            "the row wraps at {narrow}: {rects:?}"
        );
        for rect in &rects {
            assert!(
                rect.left() >= space::GUTTER - 0.01,
                "inside the near gutter: {rect:?}"
            );
            assert!(
                rect.right() <= narrow - space::GUTTER + 0.01,
                "inside the far gutter: {rect:?}"
            );
        }
    }

    #[test]
    fn the_row_grows_with_its_wraps_and_pushes_the_blocks_down() {
        let ctx = context();
        let sizes = chip_sizes(&ctx);
        let needed: f32 =
            sizes.iter().map(|size| size.x).sum::<f32>() + space::S1 * (sizes.len() - 1) as f32;
        let wide = needed + 2.0 * space::GUTTER + 32.0;
        // One point short of fitting on one line: the row wraps a chip, and the
        // blocks keep a legible width.
        let narrow = 2.0 * space::GUTTER + needed - 1.0;
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let mut wide_library = library_with(detail(Vec::new(), every_kind()));
        let (_, wide_out) = frame(
            &ctx,
            Vec2::new(wide, 1500.0),
            Vec::new(),
            &mut wide_library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let mut narrow_library = library_with(detail(Vec::new(), every_kind()));
        let (_, narrow_out) = frame(
            &ctx,
            Vec2::new(narrow, 1500.0),
            Vec::new(),
            &mut narrow_library,
            &mut transport,
            &mut queue,
            &mut art,
        );

        let chips_on_narrow: Vec<Rect> = [
            Chip::All,
            Chip::Kind(ReleaseKind::Album),
            Chip::Kind(ReleaseKind::Ep),
            Chip::Kind(ReleaseKind::Single),
            Chip::Kind(ReleaseKind::Compilation),
        ]
        .into_iter()
        .filter_map(|chip| chip_rect(&narrow_out, chip))
        .collect();
        let mut tops: Vec<f32> = chips_on_narrow.iter().map(Rect::top).collect();
        tops.dedup_by(|a, b| (*a - *b).abs() < 0.01);
        let rows = tops.len();
        assert!(rows >= 2, "the narrow row wraps: {tops:?}");

        let chip_height = chips_on_narrow[0].height();
        let expected = (rows - 1) as f32 * (chip_height + space::S1);
        let wide_block = text_rect(&wide_out, "An Album").expect("the first block");
        let narrow_block = text_rect(&narrow_out, "An Album").expect("the first block");
        assert!(
            (narrow_block.top() - wide_block.top() - expected).abs() < 0.01,
            "the blocks move down by the wrapped rows: {} against {} for {rows} rows",
            narrow_block.top() - wide_block.top(),
            expected
        );
    }

    /// The row reserves its whole height, not just its first line: the first
    /// block sits below the row by the row's height plus the same gap the
    /// section head leaves above it.
    #[test]
    fn the_row_reserves_its_height_above_the_first_block() {
        let ctx = context();
        let size = Vec2::new(900.0, 1500.0);
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        // The same block with no row above it: an Other-only artist has one
        // chip, so the row is not drawn at all.
        let mut plain = library_with(detail(
            Vec::new(),
            vec![release_of_kind(
                "spotify:book:r1",
                "A Book",
                ReleaseKind::Other,
                &["spotify:track:1"],
            )],
        ));
        let (_, without) = frame(
            &ctx,
            size,
            Vec::new(),
            &mut plain,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let mut rowed = library_with(detail(
            Vec::new(),
            vec![release("spotify:album:r1", "A Book", &["spotify:track:1"])],
        ));
        let (_, with) = frame(
            &ctx,
            size,
            Vec::new(),
            &mut rowed,
            &mut transport,
            &mut queue,
            &mut art,
        );

        let plain_top = text_rect(&without, "A Book").expect("the block with no row");
        let rowed_top = text_rect(&with, "A Book").expect("the block under the row");
        let head = text_rect(&with, "Discography").expect("the section head");
        let chip = chip_rect(&with, Chip::All).expect("the All chip");
        let above_row = chip.top() - head.bottom();
        let above_block = rowed_top.top() - plain_top.top();

        assert!(
            (above_block - (chip.height() + above_row)).abs() < 0.01,
            "the row pushes the block down by {} and is {} plus the head's {above_row}",
            above_block,
            chip.height()
        );
    }

    #[test]
    fn the_wrapped_row_never_overlaps_the_first_block() {
        let ctx = context();
        let sizes = chip_sizes(&ctx);
        let needed: f32 =
            sizes.iter().map(|size| size.x).sum::<f32>() + space::S1 * (sizes.len() - 1) as f32;
        let narrow = 2.0 * space::GUTTER + needed - 1.0;
        let mut library = library_with(detail(Vec::new(), every_kind()));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let (_, output) = frame(
            &ctx,
            Vec2::new(narrow, 1500.0),
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );

        let first_block = text_rect(&output, "An Album").expect("the first block");
        for chip in [
            Chip::All,
            Chip::Kind(ReleaseKind::Album),
            Chip::Kind(ReleaseKind::Ep),
            Chip::Kind(ReleaseKind::Single),
            Chip::Kind(ReleaseKind::Compilation),
        ] {
            let rect = chip_rect(&output, chip).expect("every chip is drawn");
            assert!(
                rect.bottom() < first_block.top(),
                "{chip:?} ends at {}, the first block starts at {}",
                rect.bottom(),
                first_block.top()
            );
        }
    }
}
