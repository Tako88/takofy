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
mod tests {
    use super::*;

    #[test]
    fn a_new_volume_is_full_and_owes_the_player_nothing() {
        let mut volume = Volume::default();
        assert_eq!(volume.level(), MAX);
        assert!(!volume.muted());
        assert_eq!(volume.fraction(), 1.0);
        assert_eq!(volume.take(), None, "the player already starts at full");
    }

    #[test]
    fn a_drag_moves_the_level_and_owes_one_command() {
        let mut volume = Volume::default();
        assert!(volume.set_fraction(0.5));
        assert_eq!(volume.level(), 50);
        assert_eq!(volume.take(), Some(50));
        assert_eq!(volume.take(), None, "one change is one command");
    }

    #[test]
    fn a_drag_that_lands_on_the_same_level_owes_nothing() {
        let mut volume = Volume::at(50);
        assert!(!volume.set_fraction(0.502), "a twitch is not a change");
        assert_eq!(volume.take(), None);
    }

    #[test]
    fn muting_silences_but_keeps_the_level() {
        let mut volume = Volume::at(70);
        assert!(volume.toggle_mute());
        assert_eq!(volume.effective(), 0);
        assert_eq!(volume.fraction(), 0.0, "the bar reads what is heard");
        assert_eq!(volume.level(), 70, "the level is kept behind the lid");
        assert_eq!(volume.take(), Some(0));

        assert!(volume.toggle_mute());
        assert_eq!(volume.effective(), 70);
        assert_eq!(volume.take(), Some(70), "unmuting restores the level");
    }

    #[test]
    fn a_drag_while_muted_unmutes() {
        let mut volume = Volume::muted_at(70);
        assert!(volume.set_fraction(0.4));
        assert!(!volume.muted());
        assert_eq!(volume.effective(), 40);
    }

    #[test]
    fn muting_at_silence_changes_nothing_and_asks_for_nothing() {
        let mut volume = Volume::at(0);
        assert!(!volume.toggle_mute(), "there is nothing to silence");
        assert!(!volume.muted(), "and the lid did not flip over silence");
        assert_eq!(volume.take(), None);
    }

    #[test]
    fn a_step_starts_from_what_is_heard() {
        let mut volume = Volume::muted_at(70);
        assert!(volume.step(i32::from(STEP)));
        assert_eq!(volume.level(), STEP, "one step up from silence");
        assert!(!volume.muted(), "and the bar is not lying any more");
    }

    #[test]
    fn the_slider_is_clamped_at_both_ends() {
        let mut volume = Volume::at(50);
        volume.set_fraction(-1.0);
        assert_eq!(volume.level(), 0);
        volume.set_fraction(4.0);
        assert_eq!(volume.level(), MAX);
    }

    #[test]
    fn a_step_is_clamped_and_moves_by_five() {
        let mut volume = Volume::at(50);
        volume.step(STEP as i32);
        assert_eq!(volume.level(), 55);
        volume.step(-100);
        assert_eq!(volume.level(), 0, "a big step stops at silence");
        volume.step(i32::MIN);
        assert_eq!(volume.level(), 0, "and does not wrap");
        volume.step(i32::MAX);
        assert_eq!(volume.level(), MAX);
    }

    #[test]
    fn restored_unmuted_owes_the_level_once() {
        let mut volume = Volume::restored(70, false);

        assert_eq!(volume.level(), 70);
        assert_eq!(volume.take(), Some(70), "the player must hear the level");
        assert_eq!(volume.take(), None, "one restore is one command");
    }

    #[test]
    fn restored_muted_owes_silence_and_keeps_the_level() {
        let mut volume = Volume::restored(70, true);

        assert_eq!(volume.take(), Some(0), "muted means silence is owed");
        assert_eq!(volume.level(), 70, "the level waits behind the lid");
        assert!(volume.muted());

        assert!(volume.toggle_mute());
        assert_eq!(volume.take(), Some(70), "unmuting restores the level");
    }

    #[test]
    fn restored_muted_at_zero_is_not_muted() {
        let mut volume = Volume::restored(0, true);

        assert!(!volume.muted(), "there is nothing to silence at zero");
        assert_eq!(volume.take(), Some(0), "the player is still told silence");
    }

    #[test]
    fn restored_clamps_the_level() {
        let volume = Volume::restored(250, false);

        assert_eq!(volume.level(), MAX, "nothing above full is a level");
    }

    #[test]
    fn set_fraction_zero_while_muted_unmutes_at_zero_and_owes_nothing() {
        let mut volume = Volume::muted_at(70);

        assert!(!volume.set_fraction(0.0), "the sound was already silent");
        assert_eq!(volume.level(), 0);
        assert!(!volume.muted(), "a zero level has no lid to lift");
        assert_eq!(volume.take(), None, "silence to silence is no command");
    }
}
