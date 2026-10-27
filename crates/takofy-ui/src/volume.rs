//! The playbar's volume: what the slider shows, and what the player is told.
//!
//! Muting is a lid on the level, not a level of its own: the slider keeps where
//! it was, so bringing the sound back needs no memory of its own, and a drag
//! while muted is a drag that unmutes — a bar that moves without sounding would
//! be lying about what it controls.

/// The loudest level the slider can name.
pub const MAX: u16 = 100;

/// How far one arrow key moves the level.
pub const STEP: u16 = 5;

/// The level, whether it is muted, and whether the player has heard about it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Volume {
    level: u16,
    muted: bool,
    /// The level the player has not been told yet. A frame takes it once, so a
    /// drag sends one command per change rather than one per drawn frame.
    pending: Option<u16>,
}

impl Default for Volume {
    fn default() -> Self {
        Self::at(MAX)
    }
}

impl Volume {
    /// A volume at `level`, with nothing owed to the player.
    ///
    /// The player starts at full on its own, so this seeds only what the slider
    /// shows: no command is sent and the speakers stay where they were.
    pub fn at(level: u16) -> Self {
        Self {
            level: level.min(MAX),
            muted: false,
            pending: None,
        }
    }

    /// A volume remembered from a preferences file, owing the player what it
    /// should sound at now.
    ///
    /// A remembered mute at zero is not a mute: there is nothing to silence and
    /// no level waiting behind a lid.
    pub fn restored(level: u16, muted: bool) -> Self {
        let mut volume = if muted && level > 0 {
            Self::muted_at(level)
        } else {
            Self::at(level)
        };
        volume.pending = Some(volume.effective());
        volume
    }

    /// A seeded volume that is also muted, for a remembered mute.
    pub fn muted_at(level: u16) -> Self {
        let mut volume = Self::at(level);
        volume.muted = true;
        volume
    }

    pub fn level(&self) -> u16 {
        self.level
    }

    pub fn muted(&self) -> bool {
        self.muted
    }

    /// What is actually heard: nothing while muted.
    pub fn effective(&self) -> u16 {
        if self.muted { 0 } else { self.level }
    }

    /// The slider's position, which follows what is heard: a muted bar reads
    /// empty, and the level it will come back to is kept behind it.
    pub fn fraction(&self) -> f32 {
        f32::from(self.effective()) / f32::from(MAX)
    }

    /// Sets the level from a slider position, and unmutes: the pointer asked for
    /// sound.
    pub fn set_fraction(&mut self, fraction: f32) -> bool {
        let level = (fraction.clamp(0.0, 1.0) * f32::from(MAX)).round() as u16;
        self.aim(level, false)
    }

    /// Moves the level by `by` percent from what is heard, clamped, and unmutes.
    ///
    /// The bar is a picture of the sound, so a step starts from where it is
    /// drawn: muted, one step right is one step up from silence rather than from
    /// the level waiting behind the lid.
    pub fn step(&mut self, by: i32) -> bool {
        let level = i32::from(self.effective())
            .saturating_add(by)
            .clamp(0, i32::from(MAX)) as u16;
        self.aim(level, false)
    }

    /// Silences without forgetting the level; pressing again brings it back.
    ///
    /// At zero there is nothing to silence, so the lid does not flip: a lit
    /// button over an already silent player would offer an action with no sound
    /// behind it.
    pub fn toggle_mute(&mut self) -> bool {
        if self.level == 0 {
            return false;
        }
        self.aim(self.level, !self.muted)
    }

    /// The level the player should be told, once.
    pub fn take(&mut self) -> Option<u16> {
        self.pending.take()
    }

    /// Sets the level and the lid, and records what the player still owes.
    fn aim(&mut self, level: u16, muted: bool) -> bool {
        let before = self.effective();
        self.level = level.min(MAX);
        self.muted = muted;
        let after = self.effective();
        if after == before {
            return false;
        }
        self.pending = Some(after);
        true
    }
}

#[cfg(test)]
mod tests;
