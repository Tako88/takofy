//! The row heart's one-frame intent.
//!
//! A heart click flips the row on the spot, but the write is the shell's: the
//! row posts what it wants and the shell applies it after every region has
//! drawn, only when the frame's one command is still free. A dropped intent
//! changes nothing, so a heart can never show a flip nobody sent.

use egui::{Context, Id};

/// One row's heart, as the shell applies it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ask {
    /// The tracks the click named, in the pick's order.
    pub tracks: Vec<String>,
    /// What the heart asks for.
    pub liked: bool,
}

fn ask_id() -> Id {
    Id::unique("heart-ask")
}

/// Posts the ask the shell applies this frame.
pub fn post(ctx: &Context, ask: Ask) {
    ctx.data_mut(|data| data.insert_temp(ask_id(), ask));
}

/// The ask posted this frame, read once so it cannot outlive it.
pub fn take(ctx: &Context) -> Option<Ask> {
    ctx.data_mut(|data| {
        let ask = data.get_temp::<Ask>(ask_id());
        data.remove::<Ask>(ask_id());
        ask
    })
}
