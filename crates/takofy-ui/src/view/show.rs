//! The show page's body: the band that says which show this is, then its episodes.
//!
//! The band holds the show's one picture and the description; every row under it
//! is an episode with its own play toggle, its description and its publish date.
//! No per-row art is drawn: the reference puts the toggle where a track row puts
//! its cover, so the band is the only place this page asks the art store for
//! anything.
//!
//! The page draws only what the library already has. `Load::Idle` has nothing to
//! say because the route asks for the show when it opens, so it draws nothing at
//! all rather than a second spinner. A ready show that does not name this page is
//! another show's, so it draws the spinner instead of that show's band.

use std::time::Instant;

use egui::{Sense, Ui};
use takofy_model::Episode;

use crate::cover::Art;
use crate::format::{episode_meta, plain_description, show_meta, total_label};
use crate::layout::{episode as episode_layout, table as table_layout};
use crate::library::{Library, Load};
use crate::message::Command;
use crate::player::Transport;
use crate::queue::Queue;
use crate::route::Page;
use crate::theme::Tokens;

use super::episode_row;
use super::hero;
use super::rows::Look;
use super::section;
use super::skeleton;
use super::text as recipe;
use super::track_list::list_rect_with;
use super::track_row::{Click, state_of};

/// Draws the show page and returns the command its controls asked for.
#[expect(
    clippy::too_many_arguments,
    reason = "the pane, the state it draws and the frame's cover store and clocks"
)]
pub fn show_view(
    ui: &mut Ui,
    library: &mut Library,
    transport: &mut Transport,
    queue: &mut Queue,
    art: &mut Art,
    now: Instant,
    now_ms: u64,
    tokens: &Tokens,
) -> Option<Command> {
    // The page's own URI, so Retry asks for the show this page is on rather than
    // whichever one happens to be in the load state.
    let Some(Page::Show(uri)) = library.page() else {
        return None;
    };
    let uri = uri.clone();

    let mut command = None;
    let mut retry = false;

    // The scroll offset is this show's, not the album's or the playlist's that
    // share the pane.
    egui::ScrollArea::vertical()
        .id_salt(("show", uri.as_str()))
        .show(ui, |ui| {
            match &library.show {
                Load::Idle => {}
                Load::Loading => {
                    let count = skeleton::row_count(library.selected_show_episodes());
                    skeleton::episode_rows(ui, count, tokens);
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
                    // A ready show that does not name this page is another
                    // show's: the page keeps its loading state rather than
                    // painting that show's band and rows.
                    let Some(show) = library.current_show() else {
                        ui.spinner();
                        return;
                    };
                    let detail = show;
                    // The one cover lookup on this page: the rows carry none.
                    let cover = art.look(detail.show.cover.as_ref(), now);
                    let meta = show_meta(
                        &detail.show.name,
                        &detail.show.publisher,
                        detail.episodes.len(),
                    );
                    let description =
                        plain_description(&detail.show.description).unwrap_or_default();
                    let band = hero::Band {
                        eyebrow: "Show",
                        title: &detail.show.name,
                        meta,
                        description: &description,
                        playable: !detail.episodes.is_empty(),
                        // The reference's show band has no shuffle: episodes are a
                        // list to work through, not a queue to stir.
                        shuffle: false,
                        cover,
                    };
                    if hero::show(ui, &band, tokens) && !detail.episodes.is_empty() {
                        command = queue.start(&entries(detail), 0, transport);
                    }

                    super::inner(ui, |ui| {
                        section::head(ui, "Episodes", "", tokens);
                        if detail.episodes.is_empty() {
                            ui.label(recipe::muted("No episodes.", tokens));
                            return;
                        }

                        let count = detail.episodes.len();
                        // The whole list is reserved at once, so the scrollbar is
                        // right, and only the rows the viewport can show are laid
                        // out and given interaction.
                        let list = list_rect_with(ui, count, episode_layout::ROW_H);
                        ui.allocate_rect(list, Sense::hover());
                        let visible = ui.clip_rect();

                        for (index, episode) in detail.episodes.iter().enumerate() {
                            let rect = table_layout::row_rect(list, index, episode_layout::ROW_H);
                            if !visible.intersects(rect) {
                                continue;
                            }
                            let duration = episode.duration_ms.and_then(total_label);
                            let meta = episode_meta(&detail.show.name, episode.published, now_ms)
                                .join(" · ");
                            let description =
                                episode.description.as_deref().and_then(plain_description);
                            let row = episode_row::Row {
                                uri: &episode.uri,
                                index,
                                title: &episode.title,
                                description: description.as_deref(),
                                meta: &meta,
                                duration: duration.as_deref(),
                                state: state_of(&episode.uri, transport),
                                look: Look::default(),
                                refocus: false,
                            };
                            if episode_row::show(ui, rect, &row, tokens) == Some(Click::Play) {
                                command = queue.start(&entries(detail), index, transport);
                            }
                        }
                    });
                }
            }
        });

    if retry {
        return library.open(Page::Show(uri));
    }
    command
}

/// The show's episodes as the queue wants them.
///
/// Built at click time only: the row list is the page's own order, and turning
/// every episode into an entry up front would copy the whole show each frame.
fn entries(detail: &takofy_model::ShowDetail) -> Vec<takofy_model::TrackEntry> {
    detail.episodes.iter().map(Episode::entry).collect()
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use egui::{
        Context, Event, Modifiers, MouseWheelSource, MouseWheelUnit, Pos2, RawInput, Rect,
        TouchPhase, Vec2, vec2,
    };

    use takofy_model::{CoverUrl, Episode, PublishedAt, Show, ShowDetail};

    use super::*;
    use crate::cover::Cover;
    use crate::message::{Failure, Update};
    use crate::theme::hero as hero_tokens;
    use crate::theme::{extent, fonts, space, style};
    use crate::view::test_support::click_events;

    const SHOW: &str = "spotify:show:a";
    const OTHER_SHOW: &str = "spotify:show:b";
    const NOW_MS: u64 = 1_761_134_400_000;
    const DAY_MS: u64 = 86_400_000;
    /// The probe window: tall enough for the band, the head and about three rows.
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

    fn episode(n: usize) -> Episode {
        Episode {
            uri: format!("spotify:episode:{n}"),
            title: format!("Episode {n}"),
            show: "Lateral".to_owned(),
            show_uri: Some(SHOW.to_owned()),
            description: Some(format!("The long way round, part {n}.")),
            published: PublishedAt::from_millis((NOW_MS - 2 * DAY_MS) as i64),
            duration_ms: Some(2_773_461),
            cover: None,
            named: true,
        }
    }

    fn detail(episodes: Vec<Episode>) -> ShowDetail {
        ShowDetail {
            show: Show {
                uri: SHOW.to_owned(),
                name: "Lateral".to_owned(),
                publisher: "Tom Scott Media".to_owned(),
                description: "A show about the long way round.".to_owned(),
                cover: None,
            },
            episodes,
        }
    }

    fn library_with(detail: ShowDetail) -> Library {
        let mut library = Library::default();
        let _ = library.open(Page::Show(SHOW.to_owned()));
        library.apply(&Update::Show(detail));
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
            if let Some(next) = show_view(
                ui,
                library,
                transport,
                queue,
                art,
                Instant::now(),
                NOW_MS,
                &tokens,
            ) {
                command = Some(next);
            }
        });
        (command, output)
    }

    /// Clicks `pos` once, one event per pass, and returns the last command.
    fn click(
        ctx: &Context,
        pos: Pos2,
        library: &mut Library,
        transport: &mut Transport,
        queue: &mut Queue,
        art: &mut Art,
    ) -> Option<Command> {
        let mut last = None;
        for event in click_events(pos) {
            last = frame(ctx, vec![event], library, transport, queue, art).0;
        }
        last
    }

    /// The centre of one row's play toggle, read from where its title was painted.
    fn toggle_centre(output: &egui::FullOutput, title: &str) -> Pos2 {
        let title = text_rect(output, title).expect("the row's title is painted");
        Pos2::new(
            title.left() - space::S4 - extent::EPISODE_TOGGLE / 2.0,
            title.top() + extent::EPISODE_TOGGLE / 2.0,
        )
    }

    /// The band's play tile, solved the way the page solves it.
    fn play_tile_rect_for(ctx: &Context) -> Rect {
        let tokens = Tokens::DARK;
        let mut found = None;
        run(ctx, input(Vec::new()), |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                let width = ui.available_width();
                let top = ui.cursor().min;
                let band = hero::Band {
                    eyebrow: "Show",
                    title: "Lateral",
                    meta: show_meta("Lateral", "Tom Scott Media", 2),
                    description: "A show about the long way round.",
                    playable: true,
                    shuffle: false,
                    cover: Cover::Missing,
                };
                let _ = hero::show(ui, &band, &tokens);
                let bottom = ui.cursor().min.y;
                let height = hero_tokens::PLAY + 2.0 * space::S4;
                let row = Rect::from_min_size(
                    Pos2::new(top.x, bottom - height),
                    Vec2::new(width, height),
                );
                found = Some(Rect::from_min_size(
                    Pos2::new(
                        row.left() + space::GUTTER,
                        row.center().y - hero_tokens::PLAY / 2.0,
                    ),
                    Vec2::splat(hero_tokens::PLAY),
                ));
            });
        });
        found.expect("the band and its action row lay out")
    }

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

    #[test]
    fn a_row_toggle_click_returns_play_for_that_episode() {
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
        let centre = toggle_centre(&output, "Episode 2");
        let command = click(
            &ctx,
            centre,
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );

        assert_eq!(
            command,
            Some(Command::Play {
                uri: "spotify:episode:2".to_owned()
            }),
            "the toggle under the click is what plays"
        );
    }

    #[test]
    fn a_culled_row_draws_nothing() {
        let ctx = context();
        let episodes = (1..=20).map(episode).collect();
        let mut library = library_with(detail(episodes));
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
        assert!(texts.contains(&"Episode 1"), "{texts:?}");
        assert!(
            !texts.contains(&"Episode 20"),
            "a row below the fold must not be laid out: {texts:?}"
        );
    }

    #[test]
    fn the_last_episode_of_a_long_show_can_be_scrolled_to() {
        // A show is routinely hundreds of rows tall. The scroll area learns its
        // height from the list's rect, so a list reserved at the wrong row pitch
        // stops short of its own end.
        let ctx = context();
        let episodes = (1..=252).map(episode).collect();
        let mut library = library_with(detail(episodes));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let _ = frame(
            &ctx,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let mut saw_last = false;
        for _ in 0..400 {
            let (_, output) = frame(
                &ctx,
                vec![
                    Event::PointerMoved(Pos2::new(300.0, 300.0)),
                    Event::MouseWheel {
                        unit: MouseWheelUnit::Point,
                        delta: vec2(0.0, -extent::EPISODE_ROW_H),
                        phase: TouchPhase::Move,
                        source: MouseWheelSource::Unknown,
                        modifiers: Modifiers::NONE,
                    },
                ],
                &mut library,
                &mut transport,
                &mut queue,
                &mut art,
            );
            if drawn_texts(&output).contains(&"Episode 252") {
                saw_last = true;
                break;
            }
        }
        assert!(saw_last, "the last episode must be reachable by scrolling");
    }

    #[test]
    fn starting_from_the_band_uses_index_zero() {
        let ctx = context();
        let mut library = library_with(detail(vec![episode(1), episode(2)]));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let tile = play_tile_rect_for(&ctx);
        let command = click(
            &ctx,
            tile.center(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );

        assert_eq!(
            command,
            Some(Command::Play {
                uri: "spotify:episode:1".to_owned()
            }),
            "the band plays the show from its first episode"
        );
        assert!(
            queue.can_next(),
            "the whole show is queued, not just its first episode"
        );
    }

    #[test]
    fn the_band_cover_is_fetched_once() {
        let ctx = context();
        let mut detail = detail(vec![episode(1), episode(2)]);
        detail.show.cover =
            Some(CoverUrl::from_wire("https://i.scdn.co/image/abc").expect("a cover url"));
        let mut library = library_with(detail);
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
}
