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

use egui::{Id, Rect, Sense, Ui, Vec2};
use takofy_model::Episode;

use crate::availability::Control;
use crate::cover::{Art, want_px};
use crate::format::{episode_meta, plain_description, show_meta, total_label};
use crate::layout::actionbar::Kind;
use crate::layout::{episode as episode_layout, table as table_layout};
use crate::library::{Library, Load};
use crate::menu::LibraryItem;
use crate::message::Command;
use crate::player::Transport;
use crate::queue::Queue;
use crate::route::Page;
use crate::scroll;
use crate::show_order::{ShowOrder, ordered};
use crate::sidebar_kind::SidebarKind;
use crate::theme::Tokens;

use crate::verbs;

use super::actionbar;
use super::carry::{self, Ghost};
use super::context_menu;
use super::controls;
use super::episode_row;
use super::hero;
use super::pin;
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
    // A click on an episode's byline asks for the show's page, which the page it
    // is already on refuses; the ask waits for the borrow of the library to end.
    let mut opened: Option<Page> = None;
    // The band's heart asks for the library, which the show is borrowed from.
    // The value the heart asked for, and the name the write needs.
    let mut save: Option<(String, bool)> = None;
    // The order a chip asked for, applied once the draw's borrow of the library
    // ends.
    let mut order: Option<ShowOrder> = None;

    // The scroll offset is this show's, not the album's or the playlist's that
    // share the pane.
    egui::ScrollArea::vertical()
        .id_salt(("show", uri.as_str()))
        .show(ui, |ui| {
            scroll::follow_drag(ui);
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
                    // One ordered view per frame: the rows, the queue and a lift
                    // all read the order the page draws.
                    let view = ordered(&detail.episodes, library.show_order);
                    // The one cover lookup on this page: the rows carry none.
                    let want = want_px(crate::theme::hero::ART, ui.pixels_per_point());
                    let cover = art.look(detail.show.cover.as_ref(), want, now);
                    let tint =
                        art.tint_held(detail.show.cover.as_ref(), want, &tokens.palette.tint);
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
                        credit: None,
                        description: &description,
                        cover,
                        tint,
                    };
                    let _ = hero::show_with(ui, &band, tokens, hero::Title::Plain);
                    let saved = library.is_saved(&uri);
                    let acted = actionbar::show(
                        ui,
                        &actionbar::Bar {
                            title: Some(&detail.show.name),
                            playable: !detail.episodes.is_empty(),
                            // The reference's show band has no shuffle: episodes
                            // are a list to work through, not a queue to stir.
                            shuffle: false,
                            more: Some(true),
                            saved,
                            kind: Kind::Other,
                            text: None,
                        },
                        None,
                        tokens,
                    );
                    if let Some(door) = acted.more_door {
                        let item = LibraryItem {
                            kind: SidebarKind::Podcasts,
                            uri: uri.clone(),
                            name: detail.show.name.clone(),
                        };
                        context_menu::post(
                            ui.ctx(),
                            verbs::library_request(
                                item,
                                door,
                                verbs::LibraryRights {
                                    renamable: false,
                                    removable: saved == Some(true),
                                    saved,
                                },
                                Some(Page::Show(uri.clone())),
                                crate::naming::Surface::Band,
                            ),
                        );
                    }
                    if acted.played && !detail.episodes.is_empty() {
                        command = queue.start(
                            &entries(&view),
                            0,
                            Some(detail.show.uri.as_str()),
                            transport,
                        );
                    }
                    if let Some(asked) = acted.saved {
                        // The library is borrowed for the show for the whole draw,
                        // so the write is asked for once the borrow ends.
                        save = Some((detail.show.name.clone(), asked));
                    }

                    super::inner(ui, |ui| {
                        let size = order_chip_size(ui, tokens, library.show_order);
                        let head = section::head_chips(ui, "Episodes", size, tokens);
                        if detail.episodes.is_empty() {
                            ui.label(recipe::muted("No episodes.", tokens));
                            return;
                        }

                        let count = view.len();
                        // The whole list is reserved at once, so the scrollbar is
                        // right, and only the rows the viewport can show are laid
                        // out and given interaction.
                        let list = list_rect_with(ui, count, episode_layout::ROW_H);
                        ui.allocate_rect(list, Sense::hover());
                        let visible = ui.clip_rect();
                        let mut selection = carry::List::open(
                            ui.ctx(),
                            Id::unique(("show-episodes", detail.show.uri.as_str())),
                            count,
                        );

                        if draw_order_chip(ui, tokens, library.show_order, head) {
                            // A pick left behind would point at a row that now
                            // holds a different episode.
                            selection.restrict(|_| false);
                            order = Some(library.show_order.flip());
                            // This frame draws the old order: the new one lands
                            // once the draw's borrow of the library ends, so the
                            // next frame is asked for here.
                            ui.ctx().request_repaint();
                        }

                        // The id the rows draw under, so a focused row is read from
                        // the same `Ui` that drew them, and the box it holds, read
                        // before the clip culls it.
                        let focused_now = ui.memory(|memory| memory.focused());
                        let mut reveal = None;

                        for (index, episode) in view.iter().enumerate() {
                            let rect = table_layout::row_rect(list, index, episode_layout::ROW_H);
                            if !table_layout::on_screen(visible, rect) {
                                // A row the cull skipped draws nothing, so it has
                                // no reveal of its own; one that holds the keyboard
                                // is asked for once the loop is done.
                                if focused_now
                                    == Some(episode_row::id(ui, (episode.uri.as_str(), index)))
                                {
                                    reveal = Some(rect);
                                }
                                continue;
                            }
                            let duration = episode.duration_ms.and_then(total_label);
                            let meta = episode_meta(&detail.show.name, episode.published, now_ms);
                            let description =
                                episode.description.as_deref().and_then(plain_description);
                            let row = episode_row::Row {
                                uri: &episode.uri,
                                index,
                                title: &episode.title,
                                description: description.as_deref(),
                                show: meta.show.as_deref().unwrap_or_default(),
                                show_uri: Some(detail.show.uri.as_str()),
                                age: meta.age.as_deref().unwrap_or_default(),
                                duration: duration.as_deref(),
                                state: state_of(episode.uri.as_str(), transport),
                                look: selection.look(index),
                                refocus: false,
                            };
                            match episode_row::show(ui, rect, &row, tokens) {
                                Some(Click::Play) => {
                                    command = queue.start(
                                        &entries(&view),
                                        index,
                                        Some(detail.show.uri.as_str()),
                                        transport,
                                    );
                                }
                                Some(Click::Select(press)) => selection.press(index, press),
                                Some(Click::Lift) => {
                                    let ghost = Ghost::Queue(row.lifted());
                                    selection.lift(index, rect.size(), ghost, |row| {
                                        view.get(row).map(|episode| episode.entry())
                                    });
                                }
                                Some(Click::Open(page)) => opened = Some(page),
                                Some(Click::Menu(_)) | Some(Click::Like(_)) | None => {}
                            }
                        }
                        // A focused row the cull skipped is asked for again through
                        // the seam, which shifts the request clear of the action row
                        // pinned above the list; the row's own reveal knows nothing
                        // of it and would leave the row behind it.
                        if let Some(row) = reveal
                            && row.top() < ui.clip_rect().top()
                        {
                            pin::reveal(ui, row);
                        }
                        selection.finish(ui.ctx());
                    });
                }
            }
        });

    if let Some(order) = order {
        library.set_show_order(order);
    }
    if retry {
        return library.open(Page::Show(uri));
    }
    if command.is_none()
        && let Some(page) = opened
    {
        return library.open(page);
    }
    if let Some((name, asked)) = save {
        let item = LibraryItem {
            kind: SidebarKind::Podcasts,
            uri: uri.clone(),
            name,
        };
        if command.is_none() {
            command = library.set_saved(&item, asked);
        }
    }
    command
}

/// The show's episodes as the queue wants them.
///
/// Built at click time only: turning every episode into an entry up front would
/// copy the whole show each frame. The rows are the frame's ordered view, so the
/// queue starts and plays what the page draws.
fn entries(episodes: &[&Episode]) -> Vec<takofy_model::TrackEntry> {
    episodes.iter().map(|episode| episode.entry()).collect()
}

/// The order chip's label, which names the order the page is drawn in.
///
/// The chip states the order rather than offering one, so a show that opens on
/// its own order says so, and a press is the only thing that changes it.
fn order_label(order: ShowOrder) -> &'static str {
    match order {
        ShowOrder::Newest => "Newest first",
        ShowOrder::Oldest => "Oldest first",
    }
}

/// The box the order chip takes, which is what the head reserves for it.
fn order_chip_size(ui: &Ui, tokens: &Tokens, order: ShowOrder) -> Vec2 {
    controls::chip_size(ui, tokens, order_label(order))
}

/// Draws the order chip in the box the head reserved, and reports a press.
fn draw_order_chip(ui: &mut Ui, tokens: &Tokens, order: ShowOrder, rect: Rect) -> bool {
    let label = order_label(order);
    controls::chip_in_keyed(ui, tokens, Control::EpisodeOrder, (), rect, label, false)
        .response
        .clicked()
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use egui::{
        Context, Event, Modifiers, MouseWheelSource, MouseWheelUnit, Pos2, RawInput, Rect,
        TouchPhase, Vec2,
    };

    use takofy_model::{CoverImage, CoverSet, CoverUrl, Episode, PublishedAt, Show, ShowDetail};

    use super::*;
    use crate::cover::Cover;
    use crate::message::{Failure, Update};
    use crate::theme::{extent, fonts, space, stroke, style};
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

    /// The bottom edge of the row whose title is `label`, read from the hairline
    /// every row closes itself with.
    fn row_bottom(output: &egui::FullOutput, label: &str) -> Option<f32> {
        let tokens = Tokens::DARK;
        let title = text_rect(output, label)?.top();
        output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Rect(shape)
                    if shape.fill == tokens.palette.border
                        && (shape.rect.height() - stroke::HAIRLINE).abs() < 0.01
                        && shape.rect.top() >= title =>
                {
                    Some(shape.rect.bottom())
                }
                _ => None,
            })
            .reduce(f32::min)
    }

    /// The page's scroll offset, read through the id the page salts it with.
    fn show_offset(ctx: &Context) -> Option<f32> {
        let mut id = None;
        run(ctx, input(Vec::new()), |ui| {
            // The area wraps its salt once before salting the frame's own ui with
            // it, so the probe hands over the same wrapped salt.
            id = Some(ui.make_persistent_id(egui::IdSalt::new(("show", SHOW))));
        });
        egui::scroll_area::State::load(ctx, id?).map(|state| state.clamped_offset().y)
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

    /// An episode published `days_ago` days before the probe's clock, so a
    /// reordering has dates to read.
    fn dated(n: usize, days_ago: u64) -> Episode {
        Episode {
            published: PublishedAt::from_millis((NOW_MS - days_ago * DAY_MS) as i64),
            ..episode(n)
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
            // A notch lands in the frame it arrives, as the window's own draw
            // pass makes it: egui's spread of one notch over several frames
            // would still be moving the list while a probe reads a row's place.
            crate::scroll::instant(ui.ctx());
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

    /// The action row's box, read from the surface it fills once it pins.
    fn bar_rect(output: &egui::FullOutput) -> Option<Rect> {
        let tokens = Tokens::DARK;
        output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Rect(shape)
                    if shape.fill == tokens.palette.surface
                        && (shape.rect.height() - crate::theme::actionbar::STUCK_H).abs()
                            < 0.01 =>
                {
                    Some(shape.rect)
                }
                _ => None,
            })
    }

    /// One frame with the wheel moved by `points` at the foot of the list, which
    /// is how the page scrolls.
    fn wheel(
        ctx: &Context,
        points: f32,
        library: &mut Library,
        transport: &mut Transport,
        queue: &mut Queue,
        art: &mut Art,
    ) -> egui::FullOutput {
        let at = Pos2::new(PROBE.x / 2.0, PROBE.y - extent::EPISODE_ROW_H);
        frame(
            ctx,
            vec![
                Event::PointerMoved(at),
                Event::MouseWheel {
                    unit: MouseWheelUnit::Point,
                    delta: Vec2::new(0.0, points),
                    phase: TouchPhase::Move,
                    source: MouseWheelSource::Unknown,
                    modifiers: Modifiers::NONE,
                },
            ],
            library,
            transport,
            queue,
            art,
        )
        .1
    }

    /// Wheels until the action row pins, and answers that frame.
    fn pinned_frame(
        ctx: &Context,
        library: &mut Library,
        transport: &mut Transport,
        queue: &mut Queue,
        art: &mut Art,
    ) -> egui::FullOutput {
        let mut output = frame(ctx, Vec::new(), library, transport, queue, art).1;
        for _ in 0..240 {
            if bar_rect(&output).is_some() {
                return output;
            }
            output = wheel(ctx, -extent::EPISODE_ROW_H, library, transport, queue, art);
        }
        panic!("the action row never pinned");
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

    /// The box a chip occupies, recovered from the label it painted: a chip's
    /// label sits at the chip's top-left plus its padding.
    fn chip_rect(output: &egui::FullOutput, label: &str) -> Option<Rect> {
        let text = text_rect(output, &label.to_uppercase())?;
        Some(text.expand2(Vec2::new(extent::CHIP_PAD_X, extent::CHIP_PAD_Y)))
    }

    /// How long the page asked the host to wait before the next frame.
    fn repaint_delay(output: &egui::FullOutput) -> std::time::Duration {
        output
            .viewport_output
            .get(&egui::ViewportId::ROOT)
            .map_or(std::time::Duration::MAX, |viewport| viewport.repaint_delay)
    }

    /// Whether the page's list holds `row` as a pick.
    fn picked(ctx: &Context, count: usize, row: usize) -> bool {
        crate::view::carry::List::open(ctx, egui::Id::unique(("show-episodes", SHOW)), count)
            .look(row)
            .selected
    }

    /// The action row's play tile, solved the way the page solves it.
    fn play_tile_rect_for(ctx: &Context) -> Rect {
        let tokens = Tokens::DARK;
        let mut found = None;
        run(ctx, input(Vec::new()), |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                let band = hero::Band {
                    eyebrow: "Show",
                    title: "Lateral",
                    meta: show_meta("Lateral", "Tom Scott Media", 2),
                    credit: None,
                    description: "A show about the long way round.",
                    cover: Cover::Missing,
                    tint: None,
                };
                let _ = hero::show_with(ui, &band, &tokens, hero::Title::Plain);
                let row = actionbar::row_rect(ui);
                found = Some(
                    crate::layout::actionbar::solve(
                        row,
                        Kind::Other,
                        crate::layout::actionbar::Parts {
                            shuffle: false,
                            field: false,
                            follow: None,
                        },
                    )
                    .play,
                );
            });
        });
        found.expect("the band and its action row lay out")
    }

    /// A show with more episodes than one screen holds.
    fn long_detail() -> ShowDetail {
        detail((0..12).map(episode).collect())
    }

    #[test]
    fn newest_first_puts_the_last_episode_first_and_keeps_every_row() {
        let ctx = context();
        // The wire lists the oldest first, so newest-first has to turn it round.
        let mut library = library_with(detail(vec![dated(1, 30), dated(2, 20), dated(3, 10)]));
        library.set_show_order(ShowOrder::Newest);
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
        let top = |label: &str| {
            text_rect(&output, label)
                .unwrap_or_else(|| panic!("{label} is drawn"))
                .top()
        };
        assert!(
            top("Episode 3") < top("Episode 2") && top("Episode 2") < top("Episode 1"),
            "the newest episode leads and the other two follow it"
        );

        let texts = drawn_texts(&output);
        for number in 1..=3 {
            let label = format!("Episode {number}");
            assert_eq!(
                texts.iter().filter(|text| **text == label).count(),
                1,
                "every episode is drawn once: {texts:?}"
            );
        }
    }

    #[test]
    fn a_click_on_a_sorted_row_plays_that_episode() {
        let ctx = context();
        // Newest first, Episode 1 is the list's second row while it is the
        // wire's first: a draw over the wire's order would play Episode 2.
        let mut library = library_with(detail(vec![dated(1, 20), dated(2, 10)]));
        library.set_show_order(ShowOrder::Newest);
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
        let sorted = text_rect(&output, "Episode 1").expect("the sorted row is drawn");
        let leading = text_rect(&output, "Episode 2").expect("the newest row is drawn");
        assert!(
            leading.top() < sorted.top(),
            "the sorted row is not the first one"
        );

        let command = click(
            &ctx,
            toggle_centre(&output, "Episode 1"),
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
            "the toggle under the click plays the row the page drew there"
        );
    }

    #[test]
    fn a_lift_on_a_sorted_row_carries_that_episode() {
        let ctx = context();
        let mut library = library_with(detail(vec![dated(1, 20), dated(2, 10)]));
        library.set_show_order(ShowOrder::Newest);
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
        let row = text_rect(&output, "Episode 1").expect("the sorted row is drawn");
        let mut events = crate::view::test_support::drag_events(
            row.center(),
            row.center() - Vec2::new(0.0, extent::EPISODE_ROW_H),
        );
        // Stop with the button still down: the lift is what this reads.
        events.pop();
        let mut carried = None;
        for event in events {
            let _ = frame(
                &ctx,
                vec![event],
                &mut library,
                &mut transport,
                &mut queue,
                &mut art,
            );
            carried = carried.or_else(|| crate::view::carry::carried(&ctx));
        }
        let carried = carried.expect("the drag lifts the row");
        assert_eq!(
            carried.rows,
            vec![1],
            "the sorted row is the one that travels"
        );
        let uris: Vec<&str> = carried
            .entries
            .iter()
            .map(|entry| entry.uri.as_str())
            .collect();
        assert_eq!(
            uris,
            ["spotify:episode:1"],
            "the carried entry is the row's own episode"
        );
    }

    #[test]
    fn the_band_plays_the_drawn_order() {
        let ctx = context();
        let mut library = library_with(detail(vec![dated(1, 20), dated(2, 10)]));
        library.set_show_order(ShowOrder::Newest);
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
                uri: "spotify:episode:2".to_owned()
            }),
            "the band starts the show from what the page draws on top"
        );
        assert!(
            queue.can_next(),
            "the whole show is queued, not just its first episode"
        );
    }

    #[test]
    fn a_sort_clears_the_picks_so_the_highlight_does_not_move() {
        let ctx = context();
        let mut library = library_with(detail(vec![dated(1, 20), dated(2, 10)]));
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
        let row = text_rect(&output, "Episode 1").expect("the wire's first row");
        let command = click(
            &ctx,
            row.center(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        assert_eq!(command, None, "a press picks a row and plays nothing");
        assert!(picked(&ctx, 2, 0), "the press picks the row it landed on");

        let chip = chip_rect(&output, "Oldest first").expect("the chip naming the order drawn");
        let command = click(
            &ctx,
            chip.center(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        assert_eq!(command, None, "a sort is no command");
        assert_eq!(
            library.show_order,
            ShowOrder::Newest,
            "the chip reordered the page"
        );
        assert!(
            !picked(&ctx, 2, 0),
            "the pick on the row that moved is gone"
        );
        assert!(!picked(&ctx, 2, 1), "and no other row took it");
    }

    #[test]
    fn a_sort_keeps_the_scroll_area_inside_its_bounds() {
        let ctx = context();
        const EPISODES: usize = 12;
        // Episode 1 is the newest, so oldest-first reverses the wire's order.
        let episodes = (1..=EPISODES).map(|n| dated(n, n as u64 * 10)).collect();
        let mut library = library_with(detail(episodes));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let (_, first) = frame(
            &ctx,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let list_bottom = row_bottom(&first, "Episode 1").expect("the first row's edge")
            + (EPISODES - 1) as f32 * extent::EPISODE_ROW_H;
        let max_scroll = list_bottom - PROBE.y;

        let mut offset = show_offset(&ctx).expect("the page keeps its scroll offset");
        let mut settled = false;
        for _ in 0..400 {
            let _ = wheel(
                &ctx,
                -extent::EPISODE_ROW_H,
                &mut library,
                &mut transport,
                &mut queue,
                &mut art,
            );
            let next = show_offset(&ctx).expect("the page keeps its scroll offset");
            if (next - offset).abs() < 0.01 {
                offset = next;
                settled = true;
                break;
            }
            offset = next;
        }
        assert!(settled, "the scroll must come to rest");
        assert!(
            (offset - max_scroll).abs() < 0.5,
            "the scroll rests at {offset}, not the list's end {max_scroll}"
        );

        library.set_show_order(ShowOrder::Oldest);
        let mut output = frame(
            &ctx,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        )
        .1;
        for _ in 0..4 {
            output = frame(
                &ctx,
                Vec::new(),
                &mut library,
                &mut transport,
                &mut queue,
                &mut art,
            )
            .1;
        }

        let after = show_offset(&ctx).expect("the page keeps its scroll offset");
        assert!(
            (0.0..=max_scroll + 0.5).contains(&after),
            "the sorted page keeps {after} inside the list, whose end is {max_scroll}"
        );
        assert!(
            text_rect(&output, "Episode 1").is_some(),
            "the newest episode ends an oldest-first list, and the end is what the bottom shows"
        );
    }

    #[test]
    fn a_chip_click_reorders_without_a_command() {
        let ctx = context();
        // The wire lists the oldest first, so the page opens naming that order.
        let mut library = library_with(detail(vec![dated(1, 20), dated(2, 10)]));
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
        let oldest = chip_rect(&output, "Oldest first").expect("the chip naming the order drawn");
        let command = click(
            &ctx,
            oldest.center(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        assert_eq!(command, None, "a sort is page state, not a command");
        assert_eq!(
            library.show_order,
            ShowOrder::Newest,
            "the press turned the list round"
        );

        let (_, output) = frame(
            &ctx,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let leading = text_rect(&output, "Episode 2").expect("the newest row");
        let trailing = text_rect(&output, "Episode 1").expect("the oldest row");
        assert!(
            leading.top() < trailing.top(),
            "the page draws the order the chip asked for"
        );
        let named = chip_rect(&output, "Newest first").expect("the chip renamed itself");
        assert!(
            chip_rect(&output, "Oldest first").is_none(),
            "one chip names one order"
        );

        // The chip is the way back: pressing the order it names gives the other.
        let command = click(
            &ctx,
            named.center(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        assert_eq!(command, None, "a sort is page state, not a command");
        assert_eq!(
            library.show_order,
            ShowOrder::Oldest,
            "a second press turns it back"
        );
    }

    #[test]
    fn a_chip_press_draws_the_new_order_without_a_second_event() {
        let ctx = context();
        // The wire lists the newest first, so oldest-first has to turn it round.
        let mut library = library_with(detail(vec![dated(2, 10), dated(1, 20)]));
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
        let chip = chip_rect(&output, "Newest first").expect("the chip naming the order drawn");

        // The press in full, then nothing at all: the chip's order lands after
        // this frame's draw, so that frame has to be asked for by name.
        let mut command = None;
        let mut pressed = None;
        for event in click_events(chip.center()) {
            let (asked, drawn) = frame(
                &ctx,
                vec![event],
                &mut library,
                &mut transport,
                &mut queue,
                &mut art,
            );
            command = command.or(asked);
            pressed = Some(drawn);
        }
        assert_eq!(command, None, "a sort is page state, not a command");
        let pressed = pressed.expect("the press drew a frame");
        let drew = |output: &egui::FullOutput, label: &str| {
            text_rect(output, label)
                .unwrap_or_else(|| panic!("{label} is drawn"))
                .top()
        };
        assert!(
            drew(&pressed, "Episode 2") < drew(&pressed, "Episode 1"),
            "the press frame itself still draws the order it started with"
        );
        assert_eq!(
            repaint_delay(&pressed),
            std::time::Duration::ZERO,
            "the press asks for the frame that paints the order it took"
        );

        let (_, after) = frame(
            &ctx,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        assert!(
            drew(&after, "Episode 1") < drew(&after, "Episode 2"),
            "the frame after the press draws the oldest episode first"
        );
    }

    #[test]
    fn pressing_the_chip_reverses_the_list_and_renames_itself() {
        let ctx = context();
        // The wire lists the newest first, so the page opens naming that order
        // and one press has to lead with the episode at the far end of it.
        let mut library = library_with(detail(vec![dated(3, 10), dated(2, 20), dated(1, 30)]));
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
        let chip = chip_rect(&output, "Newest first").expect("the chip naming the order drawn");
        let command = click(
            &ctx,
            chip.center(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        assert_eq!(command, None, "a sort is page state, not a command");

        let (_, output) = frame(
            &ctx,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let leading = text_rect(&output, "Episode 1").expect("the oldest row");
        let trailing = text_rect(&output, "Episode 3").expect("the newest row");
        assert!(
            leading.top() < trailing.top(),
            "the drawn first row is the episode at the newest one's far end"
        );
        assert!(
            chip_rect(&output, "Oldest first").is_some(),
            "the chip now names the order it turned the page to"
        );
        assert!(
            chip_rect(&output, "Newest first").is_none(),
            "and no longer names the one it left"
        );
    }

    #[test]
    fn the_show_bar_pins_and_rows_clear_it() {
        let ctx = context();
        let mut library = library_with(long_detail());
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
        let pinned = pinned_frame(&ctx, &mut library, &mut transport, &mut queue, &mut art);
        let bar = bar_rect(&pinned).expect("the action row is pinned");
        assert!(
            (bar.top() - 0.0).abs() < 0.01,
            "the row sits at the viewport's top edge: {bar:?}"
        );

        let mut painted = 0;
        for number in 1..=12 {
            let Some(title) = text_rect(&pinned, &format!("Episode {number}")) else {
                continue;
            };
            painted += 1;
            assert!(
                title.top() >= bar.bottom() - 0.01,
                "episode {number} paints at {title:?}, inside the pinned row {bar:?}"
            );
        }
        assert!(painted > 0, "the list is drawn under the pinned row");
    }

    #[test]
    fn the_show_bar_title_is_the_pages_name() {
        let ctx = context();
        let mut library = library_with(long_detail());
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
        let pinned = pinned_frame(&ctx, &mut library, &mut transport, &mut queue, &mut art);
        let bar = bar_rect(&pinned).expect("the action row is pinned");
        let in_bar: Vec<Rect> = pinned
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Text(text) if text.galley.text() == "Lateral" => {
                    Some(Rect::from_min_size(text.pos, text.galley.size()))
                }
                _ => None,
            })
            .filter(|rect| rect.top() >= bar.top() - 0.01 && rect.bottom() <= bar.bottom() + 0.01)
            .collect();
        assert_eq!(
            in_bar.len(),
            1,
            "the pinned row paints the show's name once: {in_bar:?}"
        );
        assert!(
            in_bar[0].left() > bar.left() + crate::theme::hero::PLAY,
            "the name stands beside the play tile: {:?}",
            in_bar[0]
        );
    }

    #[test]
    fn a_focused_row_partly_under_the_bar_is_scrolled_clear_of_it() {
        let ctx = context();
        let mut library = library_with(long_detail());
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
        let pinned = pinned_frame(&ctx, &mut library, &mut transport, &mut queue, &mut art);
        let bar = bar_rect(&pinned).expect("the action row is pinned");

        // The topmost episode the list draws, so a press on it lands on the row
        // and not in the strip the pinned row owns.
        let title = text_rect(&pinned, "Episode 1").expect("the first episode is drawn");
        let _ = click(
            &ctx,
            Pos2::new(title.left() + 2.0, title.center().y),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let focused = ctx
            .memory(|memory| memory.focused())
            .expect("the row took the keyboard");

        // Walk the list up until the row's top is inside the strip the pinned
        // row covers, then let what the reveal asked for settle.
        let mut covered = false;
        for _ in 0..80 {
            let row = ctx.read_response(focused).expect("the row is drawn").rect;
            if row.top() < bar.bottom() - 0.5 {
                covered = true;
                break;
            }
            let _ = wheel(
                &ctx,
                -6.0,
                &mut library,
                &mut transport,
                &mut queue,
                &mut art,
            );
        }
        assert!(covered, "the row never slid under the pinned row");

        let mut output = frame(
            &ctx,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        )
        .1;
        let mut settled = false;
        for _ in 0..240 {
            if let Some(row) = ctx.read_response(focused)
                && row.rect.top() >= bar.bottom() - 0.01
            {
                settled = true;
                break;
            }
            output = frame(
                &ctx,
                Vec::new(),
                &mut library,
                &mut transport,
                &mut queue,
                &mut art,
            )
            .1;
        }
        assert_eq!(
            ctx.memory(|memory| memory.focused()),
            Some(focused),
            "the row keeps the keyboard"
        );
        assert!(
            settled,
            "the focused row never came out from under the pinned row"
        );
        let bar = bar_rect(&output).expect("the row is still pinned");
        let row = ctx
            .read_response(focused)
            .expect("the row is drawn again")
            .rect;
        assert!(
            row.top() >= bar.bottom() - 0.01,
            "the focused row rests below the pinned row: {row:?} against {bar:?}"
        );
    }

    #[test]
    fn a_focused_row_wholly_under_the_bar_is_still_brought_out() {
        let ctx = context();
        let mut library = library_with(long_detail());
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
        let pinned = pinned_frame(&ctx, &mut library, &mut transport, &mut queue, &mut art);
        assert!(
            bar_rect(&pinned).is_some(),
            "the action row is pinned before the press"
        );

        // The topmost episode the list draws, so a press on its title lands on
        // the row and not in the strip the pinned row owns.
        let title = text_rect(&pinned, "Episode 1").expect("the first episode is drawn");
        let _ = click(
            &ctx,
            Pos2::new(title.left() + 2.0, title.center().y),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let focused = ctx
            .memory(|memory| memory.focused())
            .expect("the row took the keyboard");
        // The press lands on whichever row was under the pointer when the click
        // resolved, so the focused row is read from the frame it was drawn in.
        let after = frame(
            &ctx,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        )
        .1;
        let rect = ctx
            .read_response(focused)
            .expect("the focused row is drawn")
            .rect;
        let label = drawn_texts(&after)
            .into_iter()
            .find(|text| {
                text.starts_with("Episode ")
                    && text_rect(&after, text).is_some_and(|box_| rect.contains_rect(box_))
            })
            .map(str::to_owned)
            .expect("the focused row paints its title");

        // One wheel jump of at least the pinned row and a whole row, and enough
        // to leave this row wholly above the strip, so the frame after it draws
        // nothing at the focused row: the cull skips it and `rows::body`, which
        // is a drawn row's own reveal, never runs.
        let strip = bar_rect(&after).expect("the row is pinned").bottom();
        let jump =
            (rect.bottom() - strip).max(crate::theme::actionbar::STUCK_H) + extent::EPISODE_ROW_H;
        let mut output = wheel(
            &ctx,
            -jump,
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );

        let mut culled = false;
        let mut settled = false;
        for _ in 0..240 {
            let strip = bar_rect(&output).expect("the row is still pinned").bottom();
            if text_rect(&output, &label).is_none() {
                culled = true;
            } else if culled
                && ctx
                    .read_response(focused)
                    .is_some_and(|row| row.rect.top() >= strip - 0.01)
            {
                settled = true;
                break;
            }
            output = frame(
                &ctx,
                Vec::new(),
                &mut library,
                &mut transport,
                &mut queue,
                &mut art,
            )
            .1;
        }
        assert!(
            culled,
            "the jump never put the focused row wholly under the pinned row"
        );
        assert!(
            settled,
            "the focused row never came out from under the pinned row"
        );
        let bar = bar_rect(&output).expect("the row is still pinned");
        let row = ctx
            .read_response(focused)
            .expect("the row is drawn again")
            .rect;
        assert!(
            row.top() >= bar.bottom() - 0.01,
            "the focused row rests below the pinned row: {row:?} against {bar:?}"
        );
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
    fn a_single_click_selects_and_a_double_click_plays_it() {
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
        let row = text_rect(&output, "Episode 2").expect("the row's title");

        let command = click(
            &ctx,
            row.center(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );

        assert_eq!(command, None, "a single click plays nothing");
        assert!(transport.now_playing.is_none(), "and starts nothing");
        assert!(
            crate::view::carry::List::open(&ctx, egui::Id::unique(("show-episodes", SHOW)), 2)
                .look(1)
                .selected,
            "the row under the click is picked"
        );
        assert!(
            !crate::view::carry::List::open(&ctx, egui::Id::unique(("show-episodes", SHOW)), 2)
                .look(0)
                .selected,
            "and only that row"
        );

        let mut command = None;
        // A fresh page for the double click: a third click inside the window is
        // a triple click, and that plays nothing.
        let ctx = context();
        let mut library = library_with(detail(vec![episode(1), episode(2)]));
        let mut transport = Transport::default();
        for _ in 0..2 {
            for event in click_events(row.center()) {
                command = frame(
                    &ctx,
                    vec![event],
                    &mut library,
                    &mut transport,
                    &mut queue,
                    &mut art,
                )
                .0;
            }
        }

        assert_eq!(
            command,
            Some(Command::Play {
                uri: "spotify:episode:2".to_owned()
            }),
            "a double click plays the episode"
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
        const EPISODES: usize = 252;
        let episodes = (1..=EPISODES).map(episode).collect();
        let mut library = library_with(detail(episodes));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();

        let (_, first) = frame(
            &ctx,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        let texts = drawn_texts(&first);
        assert!(texts.contains(&"Episode 1"), "{texts:?}");
        assert!(
            !texts.contains(&"Episode 252"),
            "a row below the fold must not be drawn before scrolling: {texts:?}"
        );

        // The list's height, read off the first row's own edge and the pitch the
        // page reserves rows at, so the bound does not come from the scroll state.
        let list_bottom = row_bottom(&first, "Episode 1").expect("the first row's edge")
            + (EPISODES - 1) as f32 * extent::EPISODE_ROW_H;
        let max_scroll = list_bottom - PROBE.y;

        let mut saw_last = false;
        for _ in 0..400 {
            let output = wheel(
                &ctx,
                -extent::EPISODE_ROW_H,
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

        // The wheel keeps moving until the area stops: the end of the list must
        // be the end of the area, not where the viewport stopped drawing.
        let mut offset = show_offset(&ctx).expect("the page keeps its scroll offset");
        let mut settled = false;
        for _ in 0..50 {
            let _ = wheel(
                &ctx,
                -extent::EPISODE_ROW_H,
                &mut library,
                &mut transport,
                &mut queue,
                &mut art,
            );
            let next = show_offset(&ctx).expect("the page keeps its scroll offset");
            if (next - offset).abs() < 0.01 {
                offset = next;
                settled = true;
                break;
            }
            offset = next;
        }
        assert!(settled, "the scroll must come to rest");
        assert!(offset > 0.0, "the page must have scrolled: {offset}");
        assert!(
            (offset - max_scroll).abs() < 0.5,
            "the scroll rests at {offset}, not the list's end {max_scroll}"
        );
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

    /// The fill of the band's wash: the one full-pane-width rect as tall as the
    /// band itself.
    fn band_fill(output: &egui::FullOutput) -> Option<egui::Color32> {
        let height = (crate::theme::hero::ART + 2.0 * space::S5).max(crate::theme::hero::MIN_H);
        output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Rect(rect)
                    if (rect.rect.width() - PROBE.x).abs() < 0.5
                        && (rect.rect.height() - height).abs() < 0.5 =>
                {
                    Some(rect.fill)
                }
                _ => None,
            })
    }

    #[test]
    fn a_held_cover_tints_the_band() {
        let ctx = context();
        let tokens = Tokens::DARK;
        let url = CoverUrl::from_wire("https://i.scdn.co/image/tint").expect("a cover url");
        let set = CoverSet::single(url.clone());
        let mut detail = detail(vec![episode(1)]);
        detail.show.cover = Some(set.clone());
        let mut library = library_with(detail);
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();
        let now = Instant::now();

        art.begin_frame();
        let _ = art.look(Some(&set), 176, now);
        let _ = art.requests(now);
        art.apply(
            &Update::Cover {
                url,
                image: CoverImage::new(1, 1, vec![255, 0, 0, 255]).expect("a 1x1 image"),
            },
            now,
        );
        art.upload(&ctx, tokens.palette.tint.alpha_floor);

        let (_, output) = frame(
            &ctx,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        assert_eq!(
            band_fill(&output),
            Some(crate::theme::tint_color([116, 27, 27])),
            "the show's own mean tints its band"
        );
    }

    #[test]
    fn a_loading_cover_leaves_the_band_on_the_wash() {
        let ctx = context();
        let url = CoverUrl::from_wire("https://i.scdn.co/image/loading").expect("a cover url");
        let mut detail = detail(vec![episode(1)]);
        detail.show.cover = Some(CoverSet::single(url));
        let mut library = library_with(detail);
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut art = Art::default();
        let now = Instant::now();

        art.begin_frame();
        let (_, output) = frame(
            &ctx,
            Vec::new(),
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
        );
        assert!(
            !art.requests(now).is_empty(),
            "the band asks for the cover it has not got"
        );
        assert_eq!(
            band_fill(&output),
            Some(Tokens::DARK.palette.hero_wash),
            "an unheld cover leaves the fallback in place"
        );
    }

    #[test]
    fn the_band_cover_is_fetched_once() {
        let ctx = context();
        let mut detail = detail(vec![episode(1), episode(2)]);
        detail.show.cover = Some(CoverSet::single(
            CoverUrl::from_wire("https://i.scdn.co/image/abc").expect("a cover url"),
        ));
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
}
