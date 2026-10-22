//! The album page's body: the reference's album band and one album table.
//!
//! The band says which record this is, and the table lists its tracks in the
//! album's own order, numbered by their place in that one continuous list. No
//! per-row art is drawn: the band holds the record's one picture, so the title
//! cell is the name over its artist rather than cover-then-name.
//!
//! The page draws only what the library already has. `Load::Idle` has nothing to
//! say because the route asks for the album when it opens, so it draws nothing at
//! all rather than a second spinner. A ready record that does not name this page
//! is another album's, so it draws the spinner instead of that record's band.

use std::time::Instant;

use egui::{Id, Sense, Ui};

use crate::cover::{Art, Cover};
use crate::format::{album_meta, format_position, total_duration_ms};
use crate::layout::{Table, table as table_layout};
use crate::library::{Library, Load};
use crate::message::Command;
use crate::player::Transport;
use crate::queue::Queue;
use crate::route::Page;
use crate::scroll;
use crate::theme::{Tokens, extent};
use crate::view::text as recipe;

use super::carry::{self, Ghost};
use super::hero;
use super::skeleton;
use super::track_list::{head, list_rect};
use super::track_row::{self, Click, Row, TitleCell, state_of};

/// Draws the album page and returns the command its controls asked for.
#[expect(
    clippy::too_many_arguments,
    reason = "the pane, its table, the state it draws and the frame's cover store"
)]
pub fn album_view(
    ui: &mut Ui,
    table: Table,
    library: &mut Library,
    transport: &mut Transport,
    queue: &mut Queue,
    art: &mut Art,
    now: Instant,
    tokens: &Tokens,
) -> Option<Command> {
    // The page's own URI, so Retry asks for the album this page is on rather than
    // whichever one happens to be in the load state.
    let Some(Page::Album(uri)) = library.page() else {
        return None;
    };
    let uri = uri.clone();
    // The page a row's menu is about, so a verb that leads here is omitted and a
    // page change can close the menu.
    let here = Page::Album(uri.clone());
    let writing = library.write_in_flight();

    // No leading gap here: `tracks_view` adds the page's one gap before it
    // dispatches, and a second would start the album a step lower than every
    // other page.
    let mut command = None;
    let mut retry = false;
    // A link click is applied after the scroll area, because the matches below
    // borrow the album it has to move away from.
    let mut open: Option<Page> = None;

    // The scroll area is salted with the album's own URI, so the offset it keeps
    // is this album's and not the playlist page's that shares the pane.
    egui::ScrollArea::vertical()
        .id_salt(uri.as_str())
        .show(ui, |ui| {
            scroll::follow_drag(ui);
            match &library.album {
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
                    // A ready detail that does not name this page is another album's:
                    // the page keeps its loading state rather than painting that
                    // album's band and rows.
                    let Some(album) = library.current_album() else {
                        skeleton::rows(
                            ui,
                            skeleton::SCREENFUL,
                            table.columns().for_album(),
                            tokens,
                        );
                        return;
                    };
                    // The one cover lookup on this page: the rows carry none, so the
                    // band's picture is the only fetch a frame can ask for.
                    let cover = art.look(album.cover.as_ref(), now);
                    let meta = album_meta(
                        &album.artists,
                        album.year,
                        (album.tracks.len(), total_duration_ms(&album.tracks)),
                    );
                    let band = hero::Band {
                        eyebrow: "Album",
                        title: &album.name,
                        meta,
                        // An album's label and copyright are out of this slice, so
                        // there is no second byline to draw.
                        description: "",
                        playable: !album.tracks.is_empty(),
                        // The reference's album action bar has like and more, not
                        // shuffle.
                        shuffle: false,
                        cover,
                    };
                    if hero::show(ui, &band, tokens) && !album.tracks.is_empty() {
                        command = queue.start(&album.tracks, 0, transport);
                    }

                    super::inner(ui, |ui| {
                        let width = ui.available_width();
                        if album.tracks.is_empty() {
                            ui.label(recipe::muted("No tracks on this album.", tokens));
                            return;
                        }

                        // The album's own columns, solved once and handed to the head and
                        // every row, so the two cannot disagree about where a column is.
                        let columns = table_layout::columns(width, table.columns().for_album());
                        head(ui, columns, tokens);
                        let count = album.tracks.len();
                        // The whole list is reserved at once, so the scrollbar is right,
                        // and only the rows the viewport can show are laid out and given
                        // interaction.
                        let list = list_rect(ui, count);
                        ui.allocate_rect(list, Sense::hover());
                        let visible = ui.clip_rect();
                        let mut selection = carry::List::open(
                            ui.ctx(),
                            Id::unique(("album-tracks", album.uri.as_str())),
                            count,
                        );
                        let gate = selection.gate(writing);

                        for (index, entry) in album.tracks.iter().enumerate() {
                            let rect = table_layout::row_rect(list, index, extent::ROW_H);
                            if !visible.intersects(rect) {
                                continue;
                            }
                            let duration = entry.duration_ms.map(format_position);
                            let row = Row {
                                uri: &entry.uri,
                                // The number is the position in this one continuous list,
                                // which is what makes a multi-disc album read 1..N.
                                number: index + 1,
                                title: &entry.title,
                                title_cell: TitleCell::NameOverArtist,
                                artist: &entry.artists,
                                // The subline opens the artist it names, which on a
                                // compilation or a feature is not the album's.
                                artist_uri: entry.artist_uris.first().map(String::as_str),
                                // No album cell and no added date in an album's own table.
                                album: None,
                                album_uri: None,
                                added: None,
                                duration: duration.as_deref(),
                                state: state_of(&entry.uri, transport),
                                // The band already holds the record's one picture.
                                cover: Cover::Missing,
                                look: selection.look(index),
                                menu: gate.allows(),
                            };
                            // A row plays; its artist subline, when the album named
                            // a resolvable artist, opens that artist's page.
                            match track_row::show(ui, rect, columns, &row, tokens) {
                                Some(Click::Play) => {
                                    command = queue.start(&album.tracks, index, transport);
                                }
                                Some(Click::Open(page)) => open = Some(page),
                                Some(Click::Select(press)) => selection.press(index, press),
                                Some(Click::Menu(door)) => selection.menu(
                                    ui.ctx(),
                                    door,
                                    index,
                                    gate,
                                    Some(&here),
                                    |row| album.tracks.get(row).cloned(),
                                ),
                                Some(Click::Lift) => {
                                    let ghost = Ghost::Table(row.lifted(columns));
                                    selection.lift(index, rect.size(), ghost, |row| {
                                        album.tracks.get(row).cloned()
                                    });
                                }
                                None => {}
                            }
                        }
                        selection.finish(ui.ctx());
                    });
                }
            }
        });

    if retry {
        return library.open(Page::Album(uri));
    }
    if let Some(page) = open {
        return library.open(page);
    }
    command
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use egui::{Context, Event, Pos2, RawInput, Rect, Vec2};
    use takofy_model::{AlbumDetail, CoverUrl, PlaylistEntry, TrackEntry};

    use super::*;
    use crate::cover::Cover;
    use crate::layout::Table;
    use crate::layout::table as table_layout;
    use crate::message::{Failure, Update};
    use crate::route::Page;
    use crate::theme::hero::PLAY;
    use crate::theme::{Tokens, extent, fonts, space, style};
    use crate::view::test_support::click_events;
    use crate::view::{hero, track_list};

    const ALBUM: &str = "spotify:album:a";
    /// The album a stale ready detail names while the page is on [`ALBUM`].
    const OTHER_ALBUM: &str = "spotify:album:b";
    const PLAYLIST: &str = "spotify:playlist:a";
    /// The probe window: 900 by 800, tall enough for the band, the head and two
    /// rows in full.
    const PROBE: Vec2 = Vec2::new(900.0, 800.0);

    fn context() -> Context {
        let tokens = Tokens::DARK;
        let ctx = Context::default();
        ctx.set_fonts(fonts::definitions());
        ctx.set_global_style(style(&tokens));
        ctx
    }

    fn input(events: Vec<Event>) -> RawInput {
        RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, PROBE)),
            events,
            ..Default::default()
        }
    }

    fn run(ctx: &Context, input: RawInput, draw: impl FnMut(&mut egui::Ui)) -> egui::FullOutput {
        let mut output = ctx.run_ui(input, draw);
        output.textures_delta.clear();
        output
    }

    /// Every text the frame painted.
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

    /// The rect a painted line occupies, read back from the frame.
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

    fn detail(tracks: Vec<TrackEntry>) -> AlbumDetail {
        AlbumDetail {
            uri: ALBUM.to_owned(),
            name: "Signal Decay".to_owned(),
            artists: "Nils Aker".to_owned(),
            year: Some(1997),
            cover: None,
            tracks,
        }
    }

    /// A library on the album page with `detail` ready.
    fn library_with(detail: AlbumDetail) -> Library {
        let mut library = Library::default();
        library.open(Page::Album(ALBUM.to_owned()));
        library.apply(&Update::Album(detail));
        library
    }

    fn frame(
        ctx: &Context,
        events: Vec<Event>,
        library: &mut Library,
        transport: &mut Transport,
        queue: &mut Queue,
        art: &mut Art,
    ) -> (Option<Command>, egui::FullOutput) {
        let tokens = Tokens::DARK;
        let mut command = None;
        let output = run(ctx, input(events), |ui| {
            if let Some(next) = album_view(
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

    /// The rect the `index`-th row is drawn into, solved the way the page solves
    /// it: the band, then the head, then the whole list reserved in one block.
    fn row_rect_for(ctx: &Context, index: usize, count: usize) -> Rect {
        let tokens = Tokens::DARK;
        let mut found = None;
        run(ctx, input(Vec::new()), |ui| {
            ui.add_space(space::S3);
            let band = hero::Band {
                eyebrow: "Album",
                title: "Signal Decay",
                meta: Vec::new(),
                description: "",
                playable: true,
                shuffle: false,
                cover: Cover::Missing,
            };
            let _ = hero::show(ui, &band, &tokens);
            crate::view::inner(ui, |ui| {
                let columns =
                    table_layout::columns(ui.available_width(), Table::Full.columns().for_album());
                track_list::head(ui, columns, &tokens);
                let list = track_list::list_rect(ui, count);
                found = Some(table_layout::row_rect(list, index, extent::ROW_H));
            });
        });
        found.expect("the head and the list lay out")
    }

    /// The band's play tile, solved the way the page solves it.
    fn play_tile_rect_for(ctx: &Context) -> Rect {
        let tokens = Tokens::DARK;
        let mut found = None;
        run(ctx, input(Vec::new()), |ui| {
            ui.add_space(space::S3);
            egui::ScrollArea::vertical().show(ui, |ui| {
                let width = ui.available_width();
                let top = ui.cursor().min;
                let band = hero::Band {
                    eyebrow: "Album",
                    title: "Signal Decay",
                    meta: Vec::new(),
                    description: "",
                    playable: true,
                    shuffle: false,
                    cover: Cover::Missing,
                };
                let _ = hero::show(ui, &band, &tokens);
                let bottom = ui.cursor().min.y;
                let height = PLAY + 2.0 * space::S4;
                let row = Rect::from_min_size(
                    Pos2::new(top.x, bottom - height),
                    Vec2::new(width, height),
                );
                found = Some(Rect::from_min_size(
                    Pos2::new(row.left() + space::GUTTER, row.center().y - PLAY / 2.0),
                    Vec2::splat(PLAY),
                ));
            });
        });
        found.expect("the band and its action row lay out")
    }

    /// Clicks `pos` `times` times, one event per pass, and returns the last
    /// command the view reported.
    fn click(
        ctx: &Context,
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
                last = frame(ctx, vec![event], library, transport, queue, art).0;
            }
        }
        last
    }

    /// Whether `album`'s own list holds `index` as part of its selection.
    fn picked(ctx: &Context, album: &str, index: usize) -> bool {
        crate::view::carry::List::open(ctx, egui::Id::unique(("album-tracks", album)), 2)
            .look(index)
            .selected
    }

    #[test]
    fn a_right_click_posts_the_albums_menu() {
        let ctx = context();
        let mut library = library_with(detail(vec![
            track("spotify:track:a", "One", "Band", 222_000),
            track("spotify:track:b", "Two", "Band", 198_000),
        ]));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();
        let rect = row_rect_for(&ctx, 1, 2);

        for event in crate::view::test_support::secondary_click_events(rect.center()) {
            let _ = frame(
                &ctx,
                vec![event],
                &mut library,
                &mut transport,
                &mut queue,
                &mut art,
            );
        }
        let request = crate::view::context_menu::take_request(&ctx)
            .expect("a right-click on an album row posted a request");
        assert_eq!(request.targets.len(), 1);
        assert_eq!(request.targets[0].uri, "spotify:track:b");
        assert_eq!(request.page, Some(Page::Album(ALBUM.to_owned())));
        assert!(
            !request
                .items
                .iter()
                .any(|item| item.verb == crate::menu::Verb::GoToAlbum),
            "the album this page already is is not one of its verbs"
        );
    }

    #[test]
    fn a_single_click_selects_and_a_double_click_plays_it() {
        let tracks = || {
            vec![
                track("spotify:track:a", "One", "Band", 222_000),
                track("spotify:track:b", "Two", "Band", 198_000),
            ]
        };
        let ctx = context();
        let mut library = library_with(detail(tracks()));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let rect = row_rect_for(&ctx, 1, 2);
        let command = click(
            &ctx,
            rect.center(),
            1,
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );

        assert_eq!(command, None, "a single click plays nothing");
        assert!(transport.now_playing.is_none(), "and starts nothing");
        assert!(picked(&ctx, ALBUM, 1), "the row under the click is picked");
        assert!(!picked(&ctx, ALBUM, 0), "and only that row");

        // A fresh page for the double click: a third click inside the window is
        // a triple click, and that plays nothing.
        let ctx = context();
        let mut library = library_with(detail(tracks()));
        let mut transport = Transport::default();
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
                uri: "spotify:track:b".to_owned()
            }),
            "a double click starts the track"
        );
    }

    #[test]
    fn a_selection_stays_on_the_album_it_was_made_on() {
        let tracks = || {
            vec![
                track("spotify:track:a", "One", "Band", 222_000),
                track("spotify:track:b", "Two", "Band", 198_000),
            ]
        };
        let ctx = context();
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();
        let mut first = library_with(detail(tracks()));
        let rect = row_rect_for(&ctx, 1, 2);
        let _ = click(
            &ctx,
            rect.center(),
            1,
            &mut first,
            &mut transport,
            &mut queue,
            &mut art,
        );

        let mut second = Library::default();
        let _ = second.open(Page::Album(OTHER_ALBUM.to_owned()));
        second.apply(&Update::Album(AlbumDetail {
            uri: OTHER_ALBUM.to_owned(),
            ..detail(tracks())
        }));
        let _ = frame(
            &ctx,
            Vec::new(),
            &mut second,
            &mut transport,
            &mut queue,
            &mut art,
        );

        assert!(picked(&ctx, ALBUM, 1), "the first album keeps its pick");
        assert!(
            !picked(&ctx, OTHER_ALBUM, 1),
            "the second album does not show it against the same index"
        );
    }

    #[test]
    fn a_double_click_on_a_row_plays_that_track() {
        let ctx = context();
        let tracks = vec![
            track("spotify:track:a", "One", "Band", 222_000),
            track("spotify:track:b", "Two", "Band", 198_000),
        ];
        let mut library = library_with(detail(tracks));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

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
    fn the_bands_play_starts_the_first_track() {
        let ctx = context();
        let tracks = vec![
            track("spotify:track:a", "One", "Band", 222_000),
            track("spotify:track:b", "Two", "Band", 198_000),
        ];
        let mut library = library_with(detail(tracks));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let tile = play_tile_rect_for(&ctx);
        let command = click(
            &ctx,
            tile.center(),
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
            "the band plays the album from its first track"
        );
        assert!(
            queue.can_next(),
            "the whole album is queued, not just its first track"
        );
    }

    #[test]
    fn the_head_paints_the_number_and_the_title_but_no_artist_album_or_added() {
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
        for absent in ["ARTIST", "ADDED"] {
            assert!(
                !texts.contains(&absent),
                "an album table draws no {absent} heading: {texts:?}"
            );
        }
        // The band's eyebrow is the one `ALBUM` on this page; a column heading
        // would be a second.
        assert_eq!(
            texts.iter().filter(|text| **text == "ALBUM").count(),
            1,
            "an album table draws no ALBUM heading: {texts:?}"
        );
    }

    #[test]
    fn a_row_paints_its_title_and_its_artist_subline() {
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
        assert!(
            texts.contains(&"Nils Aker"),
            "the row's artist subline: {texts:?}"
        );

        // The subline sits under the name, not beside it.
        let name = text_rect(&output, "One").expect("the name");
        let artist = text_rect(&output, "Nils Aker").expect("the artist");
        assert!(
            artist.top() >= name.top(),
            "the artist is stacked under the name"
        );
        assert_eq!(
            artist.left(),
            name.left(),
            "both lines start at the same left"
        );
    }

    #[test]
    fn a_row_opens_its_own_track_artist_not_the_albums() {
        // A compilation or a feature: the row names a guest, and the album names
        // the record's artist. The subline draw and the link are the same name.
        let ctx = context();
        let row_track = TrackEntry {
            artists: "Guest".to_owned(),
            artist_uris: vec!["spotify:artist:guest".to_owned()],
            ..track("spotify:track:a", "One", "Guest", 222_000)
        };
        let mut library = library_with(detail(vec![row_track]));
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
            drawn_texts(&output).contains(&"Guest"),
            "the row draws the track's own artist"
        );
        let subline = text_rect(&output, "Guest").expect("the track artist subline");
        let command = click(
            &ctx,
            subline.center(),
            1,
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );

        assert_eq!(
            library.page(),
            Some(&Page::Artist("spotify:artist:guest".to_owned())),
            "the row's subline opens the artist it names, not the album's"
        );
        assert_eq!(
            command,
            Some(Command::LoadArtist {
                artist: "spotify:artist:guest".to_owned()
            })
        );
        assert!(!queue.can_next(), "opening a page must not start the queue");
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

    #[test]
    fn a_failure_draws_the_message_and_a_retry_that_asks_again() {
        let ctx = context();
        let mut library = Library::default();
        library.open(Page::Album(ALBUM.to_owned()));
        library.apply(&Update::AlbumFailed {
            album: ALBUM.to_owned(),
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
            1,
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        assert_eq!(
            command,
            Some(Command::LoadAlbum {
                album: ALBUM.to_owned()
            }),
            "Retry re-issues the same load"
        );
        assert_eq!(library.album, Load::Loading);
    }

    #[test]
    fn one_frame_asks_for_the_albums_cover_once() {
        let ctx = context();
        let mut library = library_with(AlbumDetail {
            cover: Some(CoverUrl::from_wire("https://i.scdn.co/image/abc").expect("a cover url")),
            tracks: vec![
                track("spotify:track:a", "One", "Band", 222_000),
                track("spotify:track:b", "Two", "Band", 198_000),
            ],
            ..detail(Vec::new())
        });
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();
        let now = Instant::now();

        art.begin_frame();
        let _ = frame(
            &ctx,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let requests = art.requests(now);

        assert_eq!(
            requests.len(),
            1,
            "the rows carry no art, so the band's cover is the one lookup: {requests:?}"
        );
        assert!(matches!(requests[0], Command::Fetch { .. }), "{requests:?}");
    }

    /// The band's height: the art plus its padding, or the band's own minimum.
    fn band_height() -> f32 {
        (crate::theme::hero::ART + 2.0 * space::S5).max(crate::theme::hero::MIN_H)
    }

    /// The top of the band a frame painted, read back from the frame: the band's
    /// wash is the one full-pane-width rect as tall as the band itself.
    fn band_top(output: &egui::FullOutput, width: f32) -> Option<f32> {
        output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Rect(rect)
                    if (rect.rect.width() - width).abs() < 0.5
                        && (rect.rect.height() - band_height()).abs() < 0.5 =>
                {
                    Some(rect.rect.top())
                }
                _ => None,
            })
    }

    /// The band's top as the shell's entry point draws it: `tracks_view` adds the
    /// page's one leading gap and dispatches, so this is the offset a viewer sees
    /// rather than one of the page's internal solves.
    fn pane_band_top(library: &mut Library) -> f32 {
        let ctx = context();
        let tokens = Tokens::DARK;
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();
        let output = run(&ctx, input(Vec::new()), |ui| {
            let _ = crate::view::playlist::tracks_view(
                ui,
                Table::Full,
                library,
                &mut transport,
                &mut queue,
                &mut art,
                Instant::now(),
                0,
                &tokens,
            );
        });
        band_top(&output, PROBE.x).expect("the pane paints a band")
    }

    /// A library on a ready playlist page, so the album page can be measured
    /// against the page every other page is laid out beside.
    fn playlist_library() -> Library {
        let mut library = Library::default();
        library.select(PLAYLIST);
        library.apply(&Update::Playlists(vec![PlaylistEntry {
            uri: PLAYLIST.to_owned(),
            name: "Mix".to_owned(),
            named: true,
            cover: None,
            description: None,
            owner: None,
            tracks: None,
            access: None,
        }]));
        library.apply(&Update::Tracks {
            playlist: PLAYLIST.to_owned(),
            tracks: vec![track("spotify:track:a", "One", "Band", 222_000)],
            cover: None,
            access: None,
        });
        library
    }

    /// The album's content sits one gutter inside the pane, so its table lines up
    /// under the band's own padding rather than rubbing the panel's edge.
    #[test]
    fn the_album_table_sits_one_gutter_inside_the_pane() {
        let ctx = context();
        let mut library = library_with(detail(vec![track(
            "spotify:track:a",
            "First",
            "Band",
            222_000,
        )]));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let (_, output) = frame(
            &ctx,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut Art::default(),
        );

        let heading = text_rect(&output, "TITLE").expect("the table head is painted");
        // The head paints the title column's heading, so the gutter shows up as
        // the difference between the painted left and the column solved at the
        // pane's width less the two gutters.
        let columns = table_layout::columns(
            PROBE.x - 2.0 * space::GUTTER,
            Table::Full.columns().for_album(),
        );
        assert!(
            (heading.left() - (space::GUTTER + columns.title.left())).abs() < 0.01,
            "the table head sits at {}, one gutter plus the title column is {}",
            heading.left(),
            space::GUTTER + columns.title.left()
        );
    }

    /// The album page starts where every other page starts. The leading gap is
    /// the pane's, added once by `tracks_view`; a page that adds its own sits a
    /// whole step lower, which no painted string can catch.
    #[test]
    fn the_album_page_starts_at_the_playlists_leading_gap() {
        let album = pane_band_top(&mut library_with(detail(vec![track(
            "spotify:track:a",
            "One",
            "Band",
            222_000,
        )])));
        let playlist = pane_band_top(&mut playlist_library());

        assert_eq!(
            album,
            space::S3,
            "the album band's top is the page's leading gap"
        );
        assert_eq!(
            album, playlist,
            "the album band's top {album} is not the playlist's {playlist}"
        );
    }

    /// A ready detail that does not name this page belongs to another album: the
    /// page keeps its loading state rather than painting that album's band and
    /// rows.
    #[test]
    fn a_ready_album_for_another_page_draws_the_loading_state() {
        let ctx = context();
        let mut library = Library::default();
        library.open(Page::Album(ALBUM.to_owned()));
        // The state the library guard exists for: a ready detail left from an
        // earlier page. `open` marks this page loading again, so it is staged
        // directly, as the library's own guard test does.
        library.album = Load::Ready(AlbumDetail {
            uri: OTHER_ALBUM.to_owned(),
            name: "Another Record".to_owned(),
            tracks: vec![track("spotify:track:z", "Elsewhere", "Other Band", 111_000)],
            ..detail(Vec::new())
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
            band_top(&output, PROBE.x).is_none(),
            "the page draws no band for an album it does not name: {texts:?}"
        );
        assert!(
            texts.is_empty(),
            "the page paints no words for another album: {texts:?}"
        );
        assert!(
            output.shapes.iter().any(|clipped| matches!(
                &clipped.shape,
                egui::epaint::Shape::Rect(rect) if rect.fill == Tokens::DARK.palette.skeleton_rule
            )),
            "the page holds its rows open instead of naming another album"
        );
    }
}
