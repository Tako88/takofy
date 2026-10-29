//! The stations shelf's read, and the state the page draws it from.

use crate::home::Section;
use crate::home::stations::Band;
use crate::message::Command;

use super::super::Library;

impl Library {
    /// What the stations shelf draws.
    pub(super) fn stations_section(&self) -> Section<Band<'_>> {
        Section::Absent
    }

    /// The read the stations shelf owes, if one is due.
    pub(super) fn stations_ask(&mut self) -> Option<Command> {
        None
    }
}
