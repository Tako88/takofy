//! The Now Playing panel: what the transport is on, and what is queued after it.
//!
//! The panel is handed everything it draws — the transport, the queue, the art
//! store and the frame's clock — and reports what its queue pane asked for. It
//! never reaches for any of them itself, and applying a jump, a press or a lift
//! is the shell's job, once every region has drawn.
//!
//! The tabs are the reference's pair and both are live: the queue is state the
//! window holds, and the lyrics pane asks for what it draws.

use std::time::Instant;

use egui::{FontId, Pos2, Rect, Ui, Vec2};
use takofy_model::TrackEntry;

use crate::availability::Control;
use crate::cover::{Art, want_px};
use crate::icon::Icon;
use crate::layout::panel::{self, Stack};
use crate::likes::Likes;
use crate::player::Transport;
use crate::queue::Queue;
use crate::theme::icon::Size;
use crate::theme::{Tokens, extent, radius, space, text, tracking};

use crate::lyrics::{LyricsPanel, Tab};
use crate::menu::{Anchor, Device, Door, Listing};
use crate::route::Page;
use crate::verbs;

use super::art;
use super::context_menu;
use super::controls;
use super::link::{self, Face, Target};
use super::lyrics_pane;
use super::queue_pane;
use super::text as recipe;

/// The tabs the reference draws, in its order.
const TABS: [(&str, Control); 2] = [
    ("Queue", Control::NowPlayingQueueTab),
    ("Lyrics", Control::NowPlayingLyricsTab),
];

/// What a frame of the panel asks the window to do.
///
/// Every one of these is the caller's to apply, after every region has drawn: a
/// frame that already carries a command must not also move the queue, or the
/// queue would name a track the transport was never asked to play.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Reply {
    /// What the queue pane asked for.
    pub queue: queue_pane::Reply,
    /// The page a click on the title or the artist asked for.
    pub open: Option<Page>,
    /// The tab a press asked for.
    pub tab: Option<Tab>,
    /// Whether the lyrics pane's Retry was pressed.
    pub retry: bool,
    /// The door the head's more button opened.
    pub menu: Option<Door>,
}

/// The frame's own facts, so the panel's signature stays readable.
pub struct Frame<'a> {
    pub art: &'a mut Art,
    pub now: Instant,
    /// The entry the pane draws, handed in by the shell: the one the head's
    /// more button acts on, or nothing when the drawn track is no entry here.
    pub current: Option<&'a TrackEntry>,
    pub tokens: &'a Tokens,
}

/// Draws the panel's contents inside its own rect.
pub fn show(
    ui: &mut Ui,
    transport: &Transport,
    queue: &Queue,
    lyrics: &LyricsPanel,
    likes: &Likes,
    position_ms: u32,
    frame: &mut Frame<'_>,
) -> Reply {
    let tokens = frame.tokens;
    let band = ui.available_rect_before_wrap();
    let meta_h = meta_height(ui);
    let tabs_h = tabs_height(ui, tokens);
    let stack = panel::stack_for(band, meta_h, tabs_h);
    // The panel asks for the size its own art box takes. While the playbar is
    // also on screen one track is fetched twice, which the cache's byte budget
    // pays for.
    let cover = frame.art.look(
        transport
            .now_playing
            .as_ref()
            .and_then(|now| now.cover.as_ref()),
        want_px(stack.art.width(), ui.pixels_per_point()),
        frame.now,
    );

    let menu = head(ui, stack, frame.current, likes, tokens);
    // Idle is the transport, never the cover: a track with no cover of its own
    // still has a box to frame.
    if transport.now_playing.is_none() {
        art::hole(ui.painter(), stack.art, radius::SM, tokens);
    } else {
        art::tile(
            ui.painter(),
            stack.art,
            radius::SM,
            tokens.palette.border_strong,
            cover,
            tokens,
        );
    }
    let open = meta(ui, stack.meta, transport, tokens);
    let tab = tabs(ui, stack.tabs, tokens, lyrics.showing());

    let showing = tab.unwrap_or(lyrics.showing());
    let retry = match showing {
        Tab::Queue => {
            return Reply {
                queue: queue_pane::show(ui, stack.pane, queue, frame.art, frame.now, tokens),
                open,
                tab,
                retry: false,
                menu,
            };
        }
        Tab::Lyrics => lyrics_pane::show(
            ui,
            stack.pane,
            lyrics,
            position_ms,
            transport.now_playing.as_ref().map(|now| now.uri.as_str()),
            tokens,
        ),
    };
    Reply {
        queue: queue_pane::Reply::default(),
        open,
        tab,
        retry,
        menu,
    }
}

/// The head: the panel's caption and the drawn entry's more button.
///
/// `current` is the entry the pane draws, handed in by the shell: this window's
/// current row, or the holder's track while the queue holds it. The button is
/// live only when that entry has something to do — a holder whose track is no
/// entry here leaves it waiting — and the menu it opens acts on that entry, so
/// it can never reach a track the pane is not drawing.
fn head(
    ui: &mut Ui,
    stack: Stack,
    current: Option<&TrackEntry>,
    likes: &Likes,
    tokens: &Tokens,
) -> Option<Door> {
    let caption = recipe::caption(ui, "Now playing", tokens);
    ui.painter().galley(
        Pos2::new(
            stack.head.left(),
            stack.head.center().y - caption.size().y / 2.0,
        ),
        caption,
        tokens.palette.text_subtle,
    );

    let more = Rect::from_min_size(
        Pos2::new(
            stack.head.right() - extent::BUTTON,
            stack.head.center().y - extent::BUTTON / 2.0,
        ),
        Vec2::splat(extent::BUTTON),
    );
    let live = current.is_some_and(|entry| {
        !verbs::track_items(
            std::slice::from_ref(entry),
            None,
            &Listing::Current,
            likes.standing(&[entry.uri.as_str()]),
        )
        .is_empty()
    });
    let id = controls::slot_id(ui, Control::NowPlayingMore, ());
    let open = context_menu::open_opener(ui.ctx()) == Some(id);
    let slot = controls::icon_button_open(
        ui,
        tokens,
        Control::NowPlayingMore,
        (),
        more,
        Icon::More,
        Size::MD,
        "More",
        live,
        open,
    );
    if let Some(door) = context_menu::button_door(ui, &slot, more, id) {
        return Some(door);
    }
    // The keyboard's door, taken only while the button holds it, so the key is
    // left alone everywhere else.
    if !slot.response.has_focus() {
        return None;
    }
    context_menu::menu_key_pressed(ui).then_some(Door {
        anchor: Anchor::Button(more),
        opener: id,
        device: Device::Keyboard,
    })
}

/// Title, artist and the transport's state, or the one line that says nothing is
/// playing.
///
/// The idle line sits where a title would, at the top of the reserved rect: the
/// same block with nothing in it. The title opens the album and the artist their
/// page, when the track names one; the block's heights do not change with them.
fn meta(ui: &Ui, rect: Rect, transport: &Transport, tokens: &Tokens) -> Option<Page> {
    if rect.width() <= 0.0 {
        return None;
    }
    let Some(now) = &transport.now_playing else {
        let line = recipe::line(
            ui,
            "Nothing playing",
            recipe::track_secondary(),
            tokens.palette.text_muted,
            recipe::tracking_pts(text::SM, tracking::SLIGHT),
            rect.width(),
        );
        ui.painter().galley(
            Pos2::new(rect.left(), rect.top()),
            line,
            tokens.palette.text_muted,
        );
        return None;
    };

    let title = link::Placed {
        pos: Pos2::new(rect.left(), rect.top()),
        galley: link::lay(
            ui,
            &now.title,
            &Face {
                font: recipe::panel_title(),
                tracking: recipe::tracking_pts(text::MD, tracking::TIGHT),
            },
            rect.width(),
        ),
    };
    let height = title.rect().height();
    let title = title.link(
        ui,
        ui.scope_id().with("now-playing-title"),
        rect,
        now.album_uri.as_deref().map(Target::Album),
        tokens.palette.text,
        tokens,
    );
    let mut top = rect.top() + height;
    let mut open = title.page;

    if !now.artists.is_empty() {
        let artist = link::Placed {
            pos: Pos2::new(rect.left(), top),
            galley: link::lay(
                ui,
                &now.artists,
                &Face {
                    font: recipe::track_secondary(),
                    tracking: 0.0,
                },
                rect.width(),
            ),
        };
        let shown = artist.link(
            ui,
            ui.scope_id().with("now-playing-artist"),
            rect,
            now.artist_uri.as_deref().map(Target::Credit),
            tokens.palette.text_muted,
            tokens,
        );
        open = open.or(shown.page);
    }
    // The artist line is reserved whether or not it holds an artist.
    top += line_height(ui, &recipe::track_secondary());

    let state = transport.state.label();
    if !state.is_empty() {
        let line = recipe::line(
            ui,
            state,
            recipe::track_mono(text::XXS),
            tokens.palette.text_subtle,
            recipe::tracking_pts(text::XXS, tracking::SLIGHT),
            rect.width(),
        );
        ui.painter().galley(
            Pos2::new(rect.left(), top + space::S1),
            line,
            tokens.palette.text_subtle,
        );
    }
    open
}

/// The tab pair, reporting the tab a press asked for.
fn tabs(ui: &mut Ui, rect: Rect, tokens: &Tokens, chosen: Tab) -> Option<Tab> {
    if rect.width() <= 0.0 {
        return None;
    }
    let mut pressed = None;
    let mut left = rect.left();
    for (index, (label, control)) in TABS.into_iter().enumerate() {
        let size = controls::chip_size(ui, tokens, label);
        let chip = Rect::from_min_size(
            Pos2::new(left, rect.top() + (rect.height() - size.y) / 2.0),
            size,
        );
        let tab = if index == 0 { Tab::Queue } else { Tab::Lyrics };
        let slot = controls::chip_in(ui, tokens, control, chip, label, chosen == tab);
        if slot.response.clicked() {
            pressed = Some(tab);
        }
        left += size.x + space::PX;
    }
    pressed
}

/// The reservation, from the pane's own line heights: every state names all three
/// lines, so the tabs and the art box are a function of the pane alone.
fn meta_height(ui: &Ui) -> f32 {
    panel::meta_reserved(
        line_height(ui, &recipe::panel_title()),
        line_height(ui, &recipe::track_secondary()),
        line_height(ui, &recipe::track_mono(text::XXS)),
    )
}

/// The height of the taller of the two chips.
fn tabs_height(ui: &Ui, tokens: &Tokens) -> f32 {
    TABS.into_iter()
        .map(|(label, _)| controls::chip_size(ui, tokens, label).y)
        .fold(0.0, f32::max)
}

fn line_height(ui: &Ui, font: &FontId) -> f32 {
    ui.ctx().fonts_mut(|fonts| fonts.row_height(font))
}

#[cfg(test)]
mod tests;
