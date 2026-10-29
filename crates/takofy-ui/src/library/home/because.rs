//! The because shelf's read, and the state the page draws it from.

use crate::home::Section;
use crate::home::because::Row;
use crate::message::Command;

use super::super::Library;

impl Library {
    /// What the because shelf draws.
    pub(super) fn because_section(&self) -> Section<Row<'_>> {
        Section::Absent
    }

    /// The read the because shelf owes, if one is due.
    pub(super) fn because_ask(&mut self) -> Option<Command> {
        None
    }
}
