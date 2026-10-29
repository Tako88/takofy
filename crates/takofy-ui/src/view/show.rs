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
                                // The episode's own page is where it plays.
                                playable: true,
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
mod tests;
