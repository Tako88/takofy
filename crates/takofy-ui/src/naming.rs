//! The library head's new-playlist field: whether it is open, what it holds and
//! whether a create is in flight.
//!
//! Pure: the head draws what this decides and asks it what a key means. The name
//! rules live here so the field and the core refuse the same text.

use takofy_model::PLAYLIST_NAME_MAX;

/// Why the field's text is not a name a playlist can have.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameError {
    /// Nothing but whitespace, so there is nothing to call the playlist.
    Empty,
    /// Longer than the server takes.
    TooLong,
}

/// The library head's name field.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Naming {
    open: bool,
    text: String,
    /// The tracks the create would land in the list it makes.
    pick: Vec<String>,
    writing: bool,
    /// The field should take the keyboard on the next frame it is drawn.
    wants_focus: bool,
    /// The `+` should take the keyboard on the next frame it is drawn.
    wants_button: bool,
}

impl Naming {
    /// Opens the field and asks it for the keyboard, with nothing to carry.
    ///
    /// A plain open drops a pick the last menu left: the list made from here is
    /// the one the user named, not the one a menu they have left was about.
    pub fn open(&mut self) {
        self.open_with(Vec::new());
    }

    /// Opens the field holding `pick`, so the create lands those tracks too.
    ///
    /// A create in flight is the one moment this is refused: the field is
    /// read-only then, so opening it would offer an edit nobody can make.
    pub fn open_with(&mut self, pick: Vec<String>) {
        if self.writing {
            return;
        }
        self.open = true;
        self.wants_focus = true;
        self.wants_button = false;
        self.pick = pick;
    }

    /// The tracks the create would land in the new list.
    pub fn pick(&self) -> &[String] {
        &self.pick
    }

    /// Whether the field is drawn in place of the count.
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// The text as typed.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The text, for the field to edit.
    pub fn edit(&mut self) -> &mut String {
        &mut self.text
    }

    /// Whether a create for this text is in flight.
    pub fn writing(&self) -> bool {
        self.writing
    }

    /// Whether the field should take the keyboard this frame.
    pub fn wants_focus(&self) -> bool {
        self.wants_focus
    }

    /// The field has the keyboard, so it need not ask again.
    pub fn focus_taken(&mut self) {
        self.wants_focus = false;
    }

    /// Whether the `+` should take the keyboard this frame.
    pub fn wants_button(&self) -> bool {
        self.wants_button
    }

    /// The `+` has taken the keyboard back.
    pub fn button_focus_taken(&mut self) {
        self.wants_button = false;
    }

    /// The name the text spells, trimmed, or why it is not one.
    ///
    /// Trimming keeps the inner spaces: a name is what the user typed, minus
    /// what they did not mean.
    pub fn name(&self) -> Result<String, NameError> {
        let name = self.text.trim();
        if name.is_empty() {
            return Err(NameError::Empty);
        }
        if name.chars().count() > PLAYLIST_NAME_MAX {
            return Err(NameError::TooLong);
        }
        Ok(name.to_owned())
    }

    /// Starts a create for the typed name, answering what to send.
    ///
    /// Refused while closed, while a create is in flight, and for text that is
    /// not a name. A refusal changes nothing, so a held Enter cannot send twice.
    pub fn begin_write(&mut self) -> Option<String> {
        if !self.open || self.writing {
            return None;
        }
        let name = self.name().ok()?;
        self.writing = true;
        self.wants_focus = false;
        Some(name)
    }

    /// Closes the field and clears it, handing the keyboard back to the `+`.
    ///
    /// A create in flight is the one moment this is refused: its reply still
    /// needs somewhere to land.
    pub fn cancel(&mut self) {
        if self.writing {
            return;
        }
        self.close();
        self.wants_button = true;
    }

    /// The create failed before anything existed, so the text stays for a retry.
    pub fn failed(&mut self) {
        self.writing = false;
        self.wants_focus = true;
    }

    /// The create is over: the playlist exists, so nothing is left to edit.
    pub fn finished(&mut self) {
        self.close();
        self.wants_button = true;
    }

    fn close(&mut self) {
        self.open = false;
        self.writing = false;
        self.text.clear();
        self.pick.clear();
        self.wants_focus = false;
    }
}

#[cfg(test)]
mod tests {
    use super::{NameError, Naming};
    use takofy_model::PLAYLIST_NAME_MAX;

    /// A naming field that is open with `text` in it.
    fn typing(text: &str) -> Naming {
        let mut naming = Naming::default();
        naming.open();
        naming.edit().push_str(text);
        naming
    }

    #[test]
    fn a_new_naming_is_closed_and_empty() {
        let naming = Naming::default();
        assert!(!naming.is_open());
        assert_eq!(naming.text(), "");
        assert!(!naming.writing());
        assert!(!naming.wants_focus());
        assert!(!naming.wants_button());
        assert_eq!(naming.name(), Err(NameError::Empty));
    }

    #[test]
    fn open_sets_wants_focus_and_is_idempotent() {
        let mut naming = Naming::default();
        naming.open();
        assert!(naming.is_open());
        assert!(naming.wants_focus());

        naming.focus_taken();
        assert!(!naming.wants_focus());
        naming.edit().push_str("Mix");
        naming.open();
        assert!(naming.is_open(), "a second open closed the field");
        assert!(
            naming.wants_focus(),
            "a second open did not ask for the keyboard"
        );
        assert_eq!(naming.text(), "Mix", "a second open cleared the text");
    }

    #[test]
    fn open_is_a_no_op_while_writing() {
        let mut naming = typing("Mix");
        assert_eq!(naming.begin_write(), Some("Mix".to_owned()));
        naming.cancel();
        assert!(naming.writing(), "a cancel ended a write in flight");
        naming.open();
        assert!(naming.writing());
        assert!(!naming.wants_focus(), "a write in flight took the keyboard");
    }

    #[test]
    fn name_trims_both_ends_and_keeps_inner_spaces() {
        assert_eq!(
            typing("  Night  Drive \t").name(),
            Ok("Night  Drive".to_owned())
        );
    }

    #[test]
    fn name_is_empty_for_blank_and_toolong_over_the_cap() {
        for blank in ["", " ", "\t\n"] {
            assert_eq!(typing(blank).name(), Err(NameError::Empty), "{blank:?}");
        }
        let mut naming = typing(&"a".repeat(PLAYLIST_NAME_MAX));
        assert!(naming.name().is_ok());
        naming.edit().push('a');
        assert_eq!(naming.name(), Err(NameError::TooLong));
    }

    #[test]
    fn cancel_closes_clears_and_is_refused_while_writing() {
        let mut naming = typing("Mix");
        naming.cancel();
        assert!(!naming.is_open());
        assert_eq!(naming.text(), "");
        assert!(naming.wants_button(), "the keyboard was not handed back");

        let mut writing = typing("Mix");
        assert!(writing.begin_write().is_some());
        writing.cancel();
        assert!(writing.is_open(), "a cancel closed a field mid-write");
        assert_eq!(writing.text(), "Mix", "a cancel dropped the text mid-write");
    }

    #[test]
    fn begin_write_requires_open_and_a_valid_name() {
        let mut closed = Naming::default();
        closed.edit().push_str("Mix");
        assert_eq!(closed.begin_write(), None, "a closed field wrote");

        let mut blank = typing("   ");
        assert_eq!(blank.begin_write(), None);
        assert!(!blank.writing());

        let mut naming = typing(" Mix ");
        assert_eq!(naming.begin_write(), Some("Mix".to_owned()));
        assert!(naming.writing());
        assert_eq!(naming.begin_write(), None, "a second write for one name");
    }

    #[test]
    fn a_make_failure_reopens_for_editing_with_the_text_kept() {
        let mut naming = typing("Night Drive");
        assert!(naming.begin_write().is_some());
        naming.failed();
        assert!(!naming.writing());
        assert!(naming.is_open(), "a failed create closed the field");
        assert!(naming.wants_focus(), "a failed create left no keyboard");
        assert_eq!(naming.text(), "Night Drive");
    }

    fn held() -> Vec<String> {
        vec!["spotify:track:a".to_owned(), "spotify:track:b".to_owned()]
    }

    #[test]
    fn open_with_keeps_the_pick_and_pick_reads_it() {
        let mut naming = Naming::default();
        naming.open_with(held());
        assert_eq!(naming.pick(), held());
        assert!(naming.is_open());
        assert!(naming.wants_focus());
    }

    #[test]
    fn a_plain_open_drops_a_held_pick() {
        let mut naming = Naming::default();
        naming.open_with(held());
        naming.open();
        assert!(naming.pick().is_empty(), "a plain open kept the last pick");
    }

    #[test]
    fn finished_and_cancel_drop_the_pick_but_failed_keeps_it() {
        let mut finished = Naming::default();
        finished.open_with(held());
        finished.edit().push_str("Mix");
        assert!(finished.begin_write().is_some());
        finished.finished();
        assert!(finished.pick().is_empty(), "a made list keeps no pick");

        let mut cancelled = Naming::default();
        cancelled.open_with(held());
        cancelled.cancel();
        assert!(
            cancelled.pick().is_empty(),
            "a cancelled field keeps no pick"
        );

        let mut failed = Naming::default();
        failed.open_with(held());
        failed.edit().push_str("Mix");
        assert!(failed.begin_write().is_some());
        failed.failed();
        assert_eq!(failed.pick(), held(), "a retry lands the same pick");
    }

    #[test]
    fn an_open_with_while_writing_leaves_it_empty() {
        let mut naming = typing("Mix");
        assert!(naming.begin_write().is_some());
        naming.open_with(held());
        assert!(
            naming.pick().is_empty(),
            "a write in flight took a new pick"
        );
        assert!(naming.writing());
        assert!(!naming.wants_focus());
    }

    #[test]
    fn a_link_failure_or_success_closes_and_clears() {
        // Both ends are `finished`: a success and a failed link leave the same
        // field, because the playlist exists either way.
        let mut naming = typing("Night Drive");
        assert!(naming.begin_write().is_some());
        naming.finished();
        assert!(!naming.is_open());
        assert!(!naming.writing());
        assert_eq!(naming.text(), "");
        assert!(naming.wants_button());
    }
}
