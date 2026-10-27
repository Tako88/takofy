//! The artist page's Follow state.
//!
//! The button reports what the server confirmed and nothing else: a press does
//! not flip the label, it sends the write, and the label moves when the answer
//! says so. While a write is in flight the button cannot be pressed again, and a
//! failure leaves the label where the server last left it.
//!
//! The page's probe and the write's own reply arrive as the same update, so a
//! write in flight is remembered by the value it asked for: only that value
//! ends the wait.

use crate::message::{Command, Failure, Update};

/// The artist the page is following, and what the server has said about it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Follow {
    /// The artist this state concerns; `None` before any artist page opened.
    artist: Option<String>,
    /// The last confirmed state. `None` means the probe has not answered, which
    /// reads as not followed and stays pressable: the write is idempotent.
    confirmed: Option<bool>,
    /// The value the in-flight write asked for. `None` means no write is in
    /// flight. The probe's answer and the write's answer both arrive as
    /// `Followed`, so the requested value is what says which one answered.
    pending: Option<bool>,
    /// The last write's failure, cleared when the write's own answer arrives.
    failed: Option<Failure>,
}

impl Follow {
    /// Points the state at an artist.
    ///
    /// Opening another artist resets everything, because a label confirmed for
    /// one artist says nothing about another. Re-opening the same artist keeps
    /// what is known, so a page that reloads does not flash `Follow` over a
    /// followed artist.
    pub fn open(&mut self, artist: &str) {
        if self.artist.as_deref() == Some(artist) {
            return;
        }
        *self = Self {
            artist: Some(artist.to_owned()),
            ..Self::default()
        };
    }

    /// The artist this state concerns.
    pub fn artist(&self) -> Option<&str> {
        self.artist.as_deref()
    }

    /// Whether the server has been heard to follow this artist, or `None` when
    /// the page's own probe did not answer.
    pub fn confirmed(&self) -> Option<bool> {
        self.confirmed
    }

    /// The label the button draws.
    pub fn label(&self) -> &'static str {
        if self.confirmed == Some(true) {
            "Following"
        } else {
            "Follow"
        }
    }

    /// Whether the button can be pressed at all.
    pub fn pressable(&self) -> bool {
        self.artist.is_some() && self.pending.is_none()
    }

    /// Whether a write of the button's is in flight.
    pub fn pending(&self) -> bool {
        self.pending.is_some()
    }

    /// The failure the button's hint shows, if the last write failed.
    pub fn failed(&self) -> Option<Failure> {
        self.failed
    }

    /// Presses the button: the write that asks the server for the opposite of
    /// what it last confirmed.
    ///
    /// Returns nothing while a write is in flight or before an artist page has
    /// opened, so a double press cannot send two writes.
    pub fn pressed(&mut self) -> Option<Command> {
        let artist = self.artist.clone()?;
        if self.pending.is_some() {
            return None;
        }
        let followed = !self.confirmed.unwrap_or(false);
        self.pending = Some(followed);
        Some(Command::SetFollow { artist, followed })
    }

    /// Folds one worker update into the state.
    ///
    /// Only an answer for the artist this state concerns is read. Every such
    /// answer moves the label; only the answer the in-flight write asked for
    /// ends the wait and clears the failure. A `Followed` that reports some
    /// other value is the page's own probe, which says nothing about the write.
    pub fn apply(&mut self, update: &Update) {
        match update {
            Update::Followed { artist, followed }
                if self.artist.as_deref() == Some(artist.as_str()) =>
            {
                self.confirmed = Some(*followed);
                if self.pending == Some(*followed) {
                    self.pending = None;
                    self.failed = None;
                }
            }
            Update::FollowFailed { artist, failure }
                if self.artist.as_deref() == Some(artist.as_str()) =>
            {
                self.pending = None;
                self.failed = Some(*failure);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests;
