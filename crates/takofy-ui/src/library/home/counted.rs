//! The counted shelf's read, and the state the page draws it from.

use crate::home::Section;
use crate::home::counted::Tiles;
use crate::message::Command;

use super::super::Library;

impl Library {
    /// What the counted shelf draws.
    pub(super) fn counted_section(&self) -> Section<Tiles<'_>> {
        Section::Absent
    }

    /// The read the counted shelf owes, if one is due.
    pub(super) fn counted_ask(&mut self) -> Option<Command> {
        None
    }
}
