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
mod tests;
