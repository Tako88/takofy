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

/// The name the text spells, trimmed, or why it is not one.
///
/// Trimming keeps the inner spaces: a name is what the user typed, minus what
/// they did not mean.
pub fn check_name(text: &str) -> Result<String, NameError> {
    let name = text.trim();
    if name.is_empty() {
        return Err(NameError::Empty);
    }
    if name.chars().count() > PLAYLIST_NAME_MAX {
        return Err(NameError::TooLong);
    }
    Ok(name.to_owned())
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
    /// Opens the field with nothing to carry, dropping any pick a menu left.
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
        check_name(&self.text)
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

/// Where an open rename field draws.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Surface {
    /// In a sidebar row's own name slot.
    #[default]
    Row,
    /// In place of a playlist band's title.
    Band,
}

/// A rename of one existing playlist, in whichever surface opened it.
///
/// The name rules are the create field's, so the field and the core refuse the
/// same text. A rename in flight is the one moment the target may not change.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Renaming {
    open: bool,
    uri: String,
    surface: Surface,
    text: String,
    writing: bool,
    wants_focus: bool,
    wants_return: bool,
    /// The user closed the field, so anything the sidebar or the page said about
    /// the write it carried is about a field that is gone.
    left: bool,
}

impl Renaming {
    /// Opens the field on `uri`, prefilled with the name it goes by now.
    pub fn open(&mut self, uri: &str, current: &str, surface: Surface) {
        if self.writing {
            return;
        }
        self.open = true;
        self.uri = uri.to_owned();
        self.surface = surface;
        self.text = current.to_owned();
        self.wants_focus = true;
        self.wants_return = false;
        self.left = false;
    }

    /// Whether a field is drawn.
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// The playlist the field would rename.
    pub fn uri(&self) -> &str {
        &self.uri
    }

    /// Where the field draws.
    pub fn surface(&self) -> Surface {
        self.surface
    }

    /// Whether the field is open on exactly this playlist and surface.
    pub fn is_open_on(&self, uri: &str, surface: Surface) -> bool {
        self.is_open() && self.uri == uri && self.surface == surface
    }

    /// The text as typed.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The text, for the field to edit.
    pub fn edit(&mut self) -> &mut String {
        &mut self.text
    }

    /// Whether a rename for this text is in flight.
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

    /// Whether the surface that opened the field should take the keyboard back.
    pub fn wants_return(&self) -> bool {
        self.wants_return
    }

    /// The surface has taken the keyboard back.
    pub fn return_taken(&mut self) {
        self.wants_return = false;
        self.uri.clear();
    }

    /// Whether the user closed the field, clearing the flag.
    ///
    /// The owner reads it after drawing: a cancel, a blur and a dismissed field
    /// are all the user leaving, and the notices beside the field go with it.
    pub fn take_left(&mut self) -> bool {
        std::mem::take(&mut self.left)
    }

    /// The field is open but its row is not on screen; ask again when it is.
    pub fn ask_focus(&mut self) {
        if self.is_open() {
            self.wants_focus = true;
        }
    }

    /// The name the text spells, trimmed, or why it is not one.
    pub fn name(&self) -> Result<String, NameError> {
        check_name(&self.text)
    }

    /// Starts the rename for the typed name, answering what to send.
    ///
    /// Refused while closed, while a write is in flight, and for text that is
    /// not a name. A refusal changes nothing, so a held Enter cannot send twice.
    pub fn begin_write(&mut self) -> Option<String> {
        if !self.is_open() || self.writing {
            return None;
        }
        let name = self.name().ok()?;
        self.writing = true;
        self.wants_focus = false;
        Some(name)
    }

    /// Closes the field and hands the keyboard back to the surface.
    ///
    /// A write in flight is the one moment this is refused: its reply still
    /// needs somewhere to land.
    pub fn cancel(&mut self) {
        if self.writing {
            return;
        }
        self.left |= self.open;
        self.close();
        self.wants_return = true;
    }

    /// Closes the field without a return: a blur or a page change takes the
    /// surface away.
    pub fn dismiss(&mut self) {
        if self.writing {
            return;
        }
        self.left |= self.open;
        self.close();
        self.uri.clear();
        self.wants_return = false;
    }

    /// The rename failed, so the text stays for a retry.
    ///
    /// A field that is no longer open has nothing to retry in: its reply lost the
    /// race with the page or the drawer that took the field away, and asking for
    /// the keyboard would take it from whatever the user is on now.
    pub fn failed(&mut self) {
        if !self.is_open() {
            return;
        }
        self.writing = false;
        self.wants_focus = true;
    }

    /// The rename is over, so nothing is left to edit.
    pub fn finished(&mut self) {
        self.close();
        self.wants_return = true;
    }

    /// Closes the field because its playlist is gone.
    pub fn force_close(&mut self) {
        self.close();
        self.uri.clear();
        self.wants_return = false;
    }

    /// Closes the field but keeps its target, so a cancel or a success can hand
    /// the keyboard back to the row or title that opened it.
    fn close(&mut self) {
        self.open = false;
        self.text.clear();
        self.writing = false;
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

#[cfg(test)]
mod renaming_tests {
    use super::{NameError, Renaming, Surface};
    use takofy_model::PLAYLIST_NAME_MAX;

    const URI: &str = "spotify:playlist:a";

    fn typing(text: &str) -> Renaming {
        let mut renaming = Renaming::default();
        renaming.open(URI, "Old Name", Surface::Row);
        *renaming.edit() = text.to_owned();
        renaming
    }

    #[test]
    fn a_new_rename_is_closed_and_empty() {
        let renaming = Renaming::default();
        assert!(!renaming.is_open());
        assert_eq!(renaming.uri(), "");
        assert_eq!(renaming.text(), "");
        assert!(!renaming.writing());
        assert!(!renaming.wants_focus());
        assert!(!renaming.wants_return());
        assert_eq!(renaming.name(), Err(NameError::Empty));
    }

    #[test]
    fn open_prefills_the_current_name_and_asks_for_focus() {
        let mut renaming = Renaming::default();
        renaming.open(URI, "Night Drive", Surface::Band);
        assert!(renaming.is_open());
        assert_eq!(renaming.uri(), URI);
        assert_eq!(renaming.surface(), Surface::Band);
        assert_eq!(renaming.text(), "Night Drive");
        assert!(renaming.wants_focus());
        assert!(!renaming.wants_return());
        assert!(renaming.is_open_on(URI, Surface::Band));
        assert!(!renaming.is_open_on(URI, Surface::Row));
        assert!(!renaming.is_open_on("spotify:playlist:b", Surface::Band));
    }

    #[test]
    fn open_is_refused_while_writing() {
        let mut renaming = typing("Mix");
        assert!(renaming.begin_write().is_some());
        renaming.open("spotify:playlist:b", "Other", Surface::Band);
        assert_eq!(renaming.uri(), URI);
        assert_eq!(renaming.text(), "Mix");
        assert!(renaming.writing());
    }

    #[test]
    fn name_uses_the_same_rules_as_the_create() {
        assert_eq!(
            typing("  Night  Drive \t").name(),
            Ok("Night  Drive".to_owned())
        );
        for blank in ["", " ", "\t\n"] {
            assert_eq!(typing(blank).name(), Err(NameError::Empty), "{blank:?}");
        }
        let mut long = typing(&"a".repeat(PLAYLIST_NAME_MAX));
        assert!(long.name().is_ok());
        long.edit().push('a');
        assert_eq!(long.name(), Err(NameError::TooLong));
    }

    #[test]
    fn begin_write_requires_open_and_a_valid_name_and_runs_once() {
        let mut closed = Renaming::default();
        closed.edit().push_str("Mix");
        assert_eq!(closed.begin_write(), None);

        let mut blank = typing("   ");
        assert_eq!(blank.begin_write(), None);
        assert!(!blank.writing());

        let mut renaming = typing(" Mix ");
        assert_eq!(renaming.begin_write(), Some("Mix".to_owned()));
        assert!(renaming.writing());
        assert!(!renaming.wants_focus());
        assert_eq!(renaming.begin_write(), None, "a second write for one name");
    }

    #[test]
    fn cancel_closes_and_returns_the_keyboard_but_not_while_writing() {
        let mut renaming = typing("Mix");
        renaming.cancel();
        assert!(!renaming.is_open());
        assert_eq!(renaming.text(), "");
        assert!(renaming.wants_return());

        let mut writing = typing("Mix");
        assert!(writing.begin_write().is_some());
        writing.cancel();
        assert!(writing.is_open(), "a cancel closed a field mid-write");
        assert_eq!(writing.text(), "Mix");
        assert!(!writing.wants_return());
    }

    #[test]
    fn dismiss_closes_without_returning_the_keyboard_and_is_refused_while_writing() {
        let mut renaming = typing("Mix");
        renaming.dismiss();
        assert!(!renaming.is_open());
        assert_eq!(renaming.text(), "");
        assert!(!renaming.wants_return());

        let mut writing = typing("Mix");
        assert!(writing.begin_write().is_some());
        writing.dismiss();
        assert!(writing.is_open(), "a dismiss closed a field mid-write");
    }

    #[test]
    fn failed_keeps_the_text_and_asks_for_focus_again() {
        let mut renaming = typing("Night Drive");
        assert!(renaming.begin_write().is_some());
        renaming.failed();
        assert!(!renaming.writing());
        assert!(renaming.is_open());
        assert!(renaming.wants_focus());
        assert_eq!(renaming.text(), "Night Drive");
    }

    #[test]
    fn finished_closes_clears_and_returns_the_keyboard() {
        let mut renaming = typing("Night Drive");
        assert!(renaming.begin_write().is_some());
        renaming.finished();
        assert!(!renaming.is_open());
        assert!(!renaming.writing());
        assert_eq!(renaming.text(), "");
        assert!(renaming.wants_return());
    }

    #[test]
    fn focus_and_return_are_taken_once() {
        let mut renaming = typing("Mix");
        assert!(renaming.wants_focus());
        renaming.focus_taken();
        assert!(!renaming.wants_focus());

        renaming.cancel();
        assert!(renaming.wants_return());
        renaming.return_taken();
        assert!(!renaming.wants_return());
    }

    #[test]
    fn a_closed_field_that_fails_asks_for_nothing() {
        let mut renaming = Renaming::default();
        renaming.failed();

        assert!(!renaming.is_open());
        assert!(
            !renaming.wants_focus(),
            "a field nobody drew took the keyboard"
        );
    }

    #[test]
    fn a_cancel_or_a_dismiss_is_the_user_leaving() {
        let mut cancelled = typing("Mix");
        assert!(!cancelled.take_left());
        cancelled.cancel();
        assert!(cancelled.take_left());
        assert!(!cancelled.take_left(), "the leave was read twice");

        let mut dismissed = typing("Mix");
        dismissed.dismiss();
        assert!(dismissed.take_left());

        let mut finished = typing("Mix");
        assert!(finished.begin_write().is_some());
        finished.finished();
        assert!(
            !finished.take_left(),
            "a confirmed rename is the reply's, not the user's leaving"
        );
    }

    #[test]
    fn force_close_forgets_the_target_and_returns_nothing() {
        let mut renaming = typing("Mix");
        renaming.force_close();
        assert!(!renaming.is_open());
        assert_eq!(renaming.uri(), "");
        assert!(!renaming.wants_return());
    }
    #[test]
    fn a_return_keeps_the_target_until_the_keyboard_is_back() {
        let mut cancelled = typing("Mix");
        cancelled.cancel();
        assert_eq!(cancelled.uri(), URI, "a cancel forgot where to return to");
        cancelled.return_taken();
        assert_eq!(cancelled.uri(), "");

        let mut finished = typing("Mix");
        assert!(finished.begin_write().is_some());
        finished.finished();
        assert_eq!(finished.uri(), URI, "a success forgot where to return to");
        finished.return_taken();
        assert_eq!(finished.uri(), "");
    }
}
