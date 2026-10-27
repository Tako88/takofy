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
