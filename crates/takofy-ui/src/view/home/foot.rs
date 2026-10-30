//! Home's foot: the air the page ends in.
//!
//! The reference closes the page with a rule, a ledger of what it read and a
//! sentence saying where its shelves came from. None of that survives here: the
//! ledger was three counts, and the sentence explains a machine to the person
//! reading it. What is left of the foot is the room under the last shelf, so the
//! page does not end against the edge of its own scroll.

use egui::Ui;

use crate::theme::space;

/// The room Home ends in, under its last shelf.
pub(super) fn show(ui: &mut Ui) {
    ui.add_space(space::S6);
}
