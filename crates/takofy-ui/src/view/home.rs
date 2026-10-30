//! Home: the greeting, and the five shelves under it.
//!
//! The page draws what the library decided and asks for nothing of its own: a
//! press is returned as the command or the want that follows from it. The
//! greeting is always drawn, so the page is never a bare title with nothing
//! under it.

use std::time::Instant;

use egui::Ui;
use takofy_model::TrackEntry;

use crate::cover::Art;
use crate::format::{
    HOME_BECAUSE_TITLE, HOME_COUNTED_TITLE, HOME_EPISODES_TITLE, HOME_STATIONS_TITLE,
};
use crate::home::{Fit, Presence, Section, Shelf, Slot, dated_draws, greeting};
use crate::library::Library;
use crate::message::Command;
use crate::route::Page;
use crate::sidebar_kind::SidebarKind;
use crate::theme::{Tokens, space};
use crate::view::playlist::root::{self, MediaDraw};

use super::text as recipe;
use super::{page_head, section};

mod because;
mod counted;
mod dated;
mod door;
mod episodes;
mod foot;
mod frame;
mod stations;

/// The shelves below the dated one, in the page's order.
///
/// It is `Slot::ORDER` less the dated shelf, which draws above them and is the
/// page's first band, so a shelf added to the page is drawn by default rather
/// than silently left out.
fn below() -> impl Iterator<Item = Slot> {
    Slot::ORDER.into_iter().filter(|slot| *slot != Slot::Dated)
}

/// What the page's presses asked for, collected until the frame's end.
///
/// The presses are held rather than acted on where they are drawn, because
/// drawing borrows the shelves and acting on them borrows the library.
#[derive(Debug, Default)]
pub(super) struct Pressed {
    /// The shelf whose Retry was pressed.
    pub(super) retry: Option<Slot>,
    /// The station pressed to play: its URI and the rows it plays.
    pub(super) station: Option<(String, Vec<TrackEntry>)>,
    /// The episodes a row's toggle pressed: the shelf from that row onwards.
    pub(super) episode: Option<Vec<TrackEntry>>,
    /// The day-one door asked the topbar's field for the keyboard.
    pub(super) focus_search: bool,
    /// The shelf whose foot door was pressed.
    pub(super) door: Option<Door>,
}

/// Where a shelf's foot door opens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Door {
    /// The library, at its first kind.
    Library,
    /// The library, narrowed to the shows that follow.
    Podcasts,
    /// One artist's page, which is where its neighbours are listed.
    Artist(String),
}

/// Draws Home and returns the command its controls asked for.
pub(super) fn home_view(
    ui: &mut Ui,
    library: &mut Library,
    art: &mut Art,
    now: Instant,
    now_ms: u64,
    local_hour: u8,
    tokens: &Tokens,
) -> Option<Command> {
    let mut select: Option<String> = None;
    let mut open: Option<Page> = None;
    let mut play: Option<(String, SidebarKind)> = None;
    let mut out = Pressed::default();

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            crate::view::inner(ui, |ui| {
                page_head::show(ui, greeting(local_hour), "", tokens);
                // The press that failed is the retry, so there is no button.
                if let Some(failure) = library.play_notice {
                    ui.label(recipe::muted(
                        crate::signin::failure_message(failure),
                        tokens,
                    ));
                }
                let fit = Fit {
                    columns: root::media_grid(ui).columns,
                };
                let shelves = library.home_sections(fit);
                let mut draw = MediaDraw {
                    art,
                    now,
                    tokens,
                    library: &*library,
                    select: &mut select,
                    open: &mut open,
                    play: &mut play,
                };

                // The dated shelf is the page's first band, so only a band that
                // follows another takes the rule above it.
                let mut drawn = false;
                if dated_draws(&shelves.dated, shelves.others()) {
                    dated::show(ui, &shelves.dated, now_ms, &mut draw, &mut out);
                    drawn = true;
                }
                // A failed rootlist is one failure and not five: the bands below
                // it would repeat what the shelf above already said.
                let gated = matches!(shelves.dated, Shelf::Unreadable(_));
                for slot in below() {
                    if gated {
                        continue;
                    }
                    match shelves.presence(slot) {
                        // A shelf with nothing to draw occupies no room.
                        Presence::Absent => continue,
                        Presence::Ready | Presence::Failed | Presence::Waiting => {}
                    }
                    if drawn {
                        section::divide(ui, tokens);
                    }
                    match slot {
                        // The dated shelf drew above, so this arm never runs.
                        Slot::Dated => {}
                        Slot::Stations => match &shelves.stations {
                            Section::Ready(band) => {
                                stations::show(ui, band, now_ms, tokens, &mut draw, &mut out);
                            }
                            Section::Failed(failure) => frame::failed(
                                ui,
                                tokens,
                                HOME_STATIONS_TITLE,
                                *failure,
                                slot,
                                &mut out,
                            ),
                            Section::Waiting => frame::waiting(
                                ui,
                                HOME_STATIONS_TITLE,
                                tokens,
                                frame::Band::Stations,
                            ),
                            Section::Absent => {}
                        },
                        Slot::Because => match &shelves.because {
                            Section::Ready(row) => {
                                because::show(ui, row, now_ms, tokens, &mut draw, &mut out);
                            }
                            Section::Failed(failure) => frame::failed(
                                ui,
                                tokens,
                                HOME_BECAUSE_TITLE,
                                *failure,
                                slot,
                                &mut out,
                            ),
                            Section::Waiting => {
                                frame::waiting(ui, HOME_BECAUSE_TITLE, tokens, frame::Band::Cards)
                            }
                            Section::Absent => {}
                        },
                        Slot::Counted => match &shelves.counted {
                            Section::Ready(tiles) => {
                                counted::show(ui, tiles, now_ms, tokens, &mut draw, &mut out);
                            }
                            Section::Failed(failure) => frame::failed(
                                ui,
                                tokens,
                                HOME_COUNTED_TITLE,
                                *failure,
                                slot,
                                &mut out,
                            ),
                            Section::Waiting => {
                                frame::waiting(ui, HOME_COUNTED_TITLE, tokens, frame::Band::Tiles)
                            }
                            Section::Absent => {}
                        },
                        Slot::Episodes => match &shelves.episodes {
                            Section::Ready(list) => {
                                episodes::show(ui, list, now_ms, tokens, &mut draw, &mut out);
                            }
                            Section::Failed(failure) => frame::failed(
                                ui,
                                tokens,
                                HOME_EPISODES_TITLE,
                                *failure,
                                slot,
                                &mut out,
                            ),
                            Section::Waiting => frame::waiting(
                                ui,
                                HOME_EPISODES_TITLE,
                                tokens,
                                frame::Band::Episodes,
                            ),
                            Section::Absent => {}
                        },
                    }
                    drawn = true;
                }
                // The page's foot, once there is a page above it to explain.
                if drawn {
                    ui.add_space(space::S6);
                    foot::show(ui, tokens);
                }
            });
        });

    if let Some((uri, kind)) = play
        && library.want_play(&uri, kind)
    {
        ui.ctx().request_repaint();
    }
    // A station's own URI is not a context this client can page, so the queue
    // gets its rows and no origin to publish.
    if let Some((_, rows)) = out.station.take()
        && library.want_play_rows(None, rows)
    {
        ui.ctx().request_repaint();
    }
    // The shelf's episodes come from different shows, so there is no context to
    // publish for them: the queue gets the rows and the playbar names the track.
    if let Some(rows) = out.episode.take()
        && library.want_play_rows(None, rows)
    {
        ui.ctx().request_repaint();
    }
    if out.focus_search {
        // The topbar draws the field and is drawn before this page, so the ask is
        // carried to it and the frame that moves the keyboard is asked for here.
        library.search.ask_focus();
        ui.ctx().request_repaint();
    }
    if let Some(door) = out.door.take() {
        // The library root is local state and asks for nothing; a shelf's door
        // that narrows it lands on the chip its words name.
        if let Door::Artist(uri) = door {
            return library.open(Page::Artist(uri));
        }
        library.to_playlists();
        if door == Door::Podcasts {
            library
                .root_filter
                .choose(crate::library_filter::RootKind::Podcasts);
        }
        return None;
    }
    if let Some(slot) = out.retry {
        return library.retry_home(slot);
    }
    if let Some(uri) = select {
        return Some(library.select(&uri));
    }
    if let Some(page) = open {
        return library.open(page);
    }
    None
}

#[cfg(test)]
mod tests;
