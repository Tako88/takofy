//! The episodes shelf: the newest episodes of the shows you follow.
//!
//! The head names the shelf, and the four rows are the app's own episode rows in
//! their compact form: the toggle, the title, the show and how long ago it was
//! published, and how long it runs. The description belongs to the show page, not
//! to a shelf that is only offering the newest of them.
//!
//! A row's toggle plays the shelf from that row: the episodes here come from
//! different shows, so there is no one context to hand the queue, and pressing
//! the newest is how a reader starts listening through them.

use egui::{Id, Sense, Ui};

use crate::format::{HOME_EPISODES_TITLE, episode_meta, total_label};
use crate::home::episodes::List;
use crate::layout::{episode as episode_layout, table as table_layout};
use crate::theme::Tokens;
use crate::view::carry::{self, Ghost};
use crate::view::episode_row;
use crate::view::playlist::root::MediaDraw;
use crate::view::section;
use crate::view::track_list::list_rect_with;
use crate::view::track_row::{Click, State};

use super::Pressed;

/// Draws the shelf, and nothing until it has something to draw.
pub(super) fn show(
    ui: &mut Ui,
    list: &List<'_>,
    now_ms: u64,
    tokens: &Tokens,
    draw: &mut MediaDraw<'_, '_>,
    out: &mut Pressed,
) {
    if list.rows.is_empty() {
        return;
    }
    section::head(ui, HOME_EPISODES_TITLE, "", tokens);

    let count = list.rows.len();
    let list_rect = list_rect_with(ui, count, episode_layout::ROW_H);
    ui.allocate_rect(list_rect, Sense::hover());
    let visible = ui.clip_rect();
    let mut selection = carry::List::open(ui.ctx(), Id::unique("home-episodes"), count);

    for (index, episode) in list.rows.iter().enumerate() {
        let rect = table_layout::row_rect(list_rect, index, episode_layout::ROW_H);
        if !table_layout::on_screen(visible, rect) {
            continue;
        }
        let meta = episode_meta(&episode.show, episode.published, now_ms);
        let duration = episode.duration_ms.and_then(total_label);
        let words = episode_row::Row {
            uri: &episode.uri,
            index,
            title: &episode.title,
            description: None,
            show: meta.show.as_deref().unwrap_or_default(),
            show_uri: episode.show_uri.as_deref(),
            age: meta.age.as_deref().unwrap_or_default(),
            duration: duration.as_deref(),
            playable: true,
            state: State::Resting,
            look: selection.look(index),
            refocus: false,
        };
        match episode_row::show(ui, rect, &words, tokens) {
            Some(Click::Select(press)) => selection.press(index, press),
            Some(Click::Lift) => {
                let ghost = Ghost::Queue(words.lifted());
                selection.lift(index, rect.size(), ghost, |at| {
                    list.rows.get(at).map(|episode| episode.entry())
                });
            }
            Some(Click::Play) => {
                out.episode = Some(list.rows[index..].iter().map(|row| row.entry()).collect());
            }
            Some(Click::Open(page)) => *draw.open = Some(page),
            // The menu and the like belong to the page the row opens.
            Some(Click::Menu(_)) | Some(Click::Like(_)) | None => {}
        }
    }
    selection.finish(ui.ctx());
}

#[cfg(test)]
mod tests;
