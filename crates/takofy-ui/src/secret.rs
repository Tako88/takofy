//! A pasted secret on its way out of the window.
//!
//! The key a listener pastes on the settings page is the one credential the
//! window ever holds, and it holds it only until the submit that sends it. The
//! wrapper has no `Display` and a redacting `Debug`, so a log line or a panic
//! that prints it says `Secret(..)` instead of the key.

use std::fmt;

/// A string that must never reach a log.
///
/// It is emptied by the submit that carries it out, which is what keeps it a
/// one-shot: the worker is the only holder after that.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    /// Wraps text a caller already has.
    pub fn from_text(text: impl Into<String>) -> Self {
        Self(text.into())
    }

    /// The text, for the field that draws and edits it.
    pub fn edit(&mut self) -> &mut String {
        &mut self.0
    }

    /// Whether there is nothing to send: whitespace alone is nothing.
    pub fn is_empty(&self) -> bool {
        self.0.trim().is_empty()
    }

    /// Moves the text out and leaves this one empty.
    pub fn take(&mut self) -> Self {
        Self(std::mem::take(&mut self.0))
    }

    /// The text itself, for the one caller that hands it to the keychain.
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(..)")
    }
}

#[cfg(test)]
mod tests;
