//! The episodes shelf's read, and the state the page draws it from.

use crate::home::Section;
use crate::home::episodes::List;
use crate::message::Command;

use super::super::Library;

impl Library {
    /// What the episodes shelf draws.
    pub(super) fn episodes_section(&self) -> Section<List<'_>> {
        Section::Absent
    }

    /// The read the episodes shelf owes, if one is due.
    pub(super) fn episodes_ask(&mut self) -> Option<Command> {
        None
    }
}
