//! The window's draw pass: the sign-in states, the library and the transport.
//!
//! The view reads the state machine and the tokens it is handed, and returns a
//! command when the user asks for one. It never transitions state itself.

mod actionbar;
mod album;
mod art;
mod artist;
mod artist_hero;
mod carry;
mod context_menu;
pub mod controls;
pub mod device_list;
mod discography_filter;
mod episode_row;
mod filter_count;
mod filter_field;
mod filter_row;
mod focus;
mod ghost;
mod heart;
mod hero;
pub(crate) mod keys;
mod list_more;
pub mod lyrics_pane;
pub mod media;
mod menu_box;
mod name_field;
pub mod now_playing;
mod page_head;
mod pin;
pub mod playbar;
pub mod playlist;
mod playlist_filter;
pub mod queue_pane;
pub mod queue_row;
mod release_block;
pub mod rows;
mod search_cards;
mod search_menu;
mod search_results;
mod section;
mod shell;
mod show;
pub mod sidebar;
mod sign_in;
mod skeleton;
#[cfg(test)]
pub(crate) mod test_support;
pub mod text;
pub mod topbar;
mod track_list;
mod track_row;
pub mod view_toggle;

use std::time::Instant;

use egui::Ui;

use crate::cover::Art;
use crate::device_list::DeviceList;
use crate::devices::Devices;
use crate::drawer::Drawer;
use crate::layout::PanePref;
use crate::library::Library;
use crate::menu::Menu;
use crate::message::Command;
use crate::player::Transport;
use crate::queue::Queue;
use crate::signin::SignIn;
use crate::theme::{Tokens, space};

/// The state the window draws from, borrowed for one pass.
pub struct Screen<'a> {
    pub state: &'a mut SignIn,
    pub library: &'a mut Library,
    pub transport: &'a mut Transport,
    pub queue: &'a mut Queue,
    /// The account's devices and the holder's transport.
    pub devices: &'a mut Devices,
    /// Whether the device list is open, and above which button.
    pub list: &'a mut DeviceList,
    pub drawer: &'a mut Drawer,
    /// What the user asked of the Now Playing column, carried so the shell can
    /// resolve it against the width and flip it on a press.
    pub pane: &'a mut PanePref,
    /// The context menu, owned by the window because it survives frames.
    pub menu: &'a mut Menu,
    pub position_ms: u32,
    /// The cover store every `look` goes through.
    pub art: &'a mut Art,
    /// The frame's clock, read once at the frame entry.
    pub now: Instant,
    /// The frame's wall clock in epoch milliseconds, read once at the frame
    /// entry.
    pub now_ms: u64,
}

/// Draws a page's content inside the reference's `.view__inner` gutter.
///
/// The band above it stays full-bleed and carries its own gutter padding, so
/// only what sits under a band is inset here.
pub(crate) fn inner<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(space::GUTTER as i8, 0))
        .show(ui, add)
        .inner
}

/// Draws the current state, returning a command when the user asks for one.
pub fn show(ui: &mut Ui, screen: &mut Screen<'_>, tokens: &Tokens) -> Option<Command> {
    // Before anything scrolls: the wheel moves the page in the frame it arrives.
    crate::scroll::instant(ui.ctx());
    // Before anything reads a key: whether focus is visible is this frame's.
    focus::begin_frame(ui.ctx());

    match *screen.state {
        // Starting, authorizing and a prompt are one card: with words when there
        // are words, with a spinner while the flow works.
        SignIn::Starting | SignIn::Authorizing | SignIn::NeedsAuthorization | SignIn::Failed(_) => {
            sign_in::show(ui, screen.state, tokens)
        }
        SignIn::SignedIn => shell::show(ui, screen, tokens),
    }
}
