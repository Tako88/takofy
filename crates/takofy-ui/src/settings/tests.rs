use super::*;
use takofy_model::home::Day;

fn facts(program: Option<&str>, key: KeyState) -> Facts {
    Facts {
        program: program.map(str::to_owned),
        build: Freshness::Fresh,
        built: Some(Day::from_days(740_000)),
        expires: Some(Day::from_days(740_090)),
        key,
        looked_in_downloads: true,
    }
}

fn ready() -> Settings {
    let mut settings = Settings::default();
    settings.choose(SoundChoice::Soloist);
    settings.apply(&Update::Soloist(SoloistUpdate::Facts(facts(
        Some("/home/tako/soloist"),
        KeyState::Stored,
    ))));
    settings
}

fn fold(settings: &mut Settings, update: SoloistUpdate) {
    settings.apply(&Update::Soloist(update));
}

#[test]
fn choosing_soloist_reveals_its_setup() {
    let mut settings = Settings::default();
    assert!(!settings.panel());
    settings.choose(SoundChoice::Soloist);
    assert_eq!(settings.chosen(), SoundChoice::Soloist);
    assert!(settings.panel());
}

#[test]
fn the_built_in_engine_can_still_be_set_up_by_the_offer() {
    let mut settings = Settings::default();
    settings.reveal_setup();
    assert_eq!(settings.chosen(), SoundChoice::BuiltIn);
    assert!(settings.panel());
}

#[test]
fn an_empty_key_is_refused_and_nothing_is_sent() {
    let mut settings = Settings::default();
    settings.key_field_mut().push_str("   ");
    settings.submit_key();
    assert_eq!(settings.fault(), Some(SetupFault::KeyEmpty));
    assert!(!settings.pending());
}

#[test]
fn a_submitted_key_empties_the_field_and_the_outbox_carries_one() {
    let mut settings = Settings::default();
    settings.key_field_mut().push_str("hunter2");
    settings.submit_key();
    assert!(settings.key_field_mut().is_empty());
    assert_eq!(settings.submit_count(), 1);
    assert_eq!(
        settings.take_outbox(),
        vec![Command::Soloist(SoloistAsk::StoreKey(Secret::from_text(
            "hunter2"
        )))]
    );
    assert!(!format!("{:?}", settings).contains("hunter2"));
    assert!(!settings.pending());
}

#[test]
fn a_second_submit_is_salting_a_new_field() {
    let mut settings = Settings::default();
    settings.key_field_mut().push_str("one");
    settings.submit_key();
    settings.key_field_mut().push_str("two");
    settings.submit_key();
    assert_eq!(settings.submit_count(), 2);
    settings.take_outbox();
    assert!(!format!("{settings:#?}").contains("two"));
}

#[test]
fn the_visit_asks_for_one_scan_on_entry_and_none_while_staying() {
    let mut settings = Settings::default();
    settings.visit(true);
    assert_eq!(
        settings.take_outbox(),
        vec![Command::Soloist(SoloistAsk::Scan { path: None })]
    );
    assert!(settings.scanning());
    settings.visit(true);
    settings.visit(true);
    assert!(!settings.pending());
}

#[test]
fn a_scan_answers_and_the_pin_is_the_program_it_found() {
    let mut settings = Settings::default();
    settings.visit(true);
    fold(
        &mut settings,
        SoloistUpdate::Facts(facts(Some("/home/tako/soloist"), KeyState::Absent)),
    );
    assert!(!settings.scanning());
    assert_eq!(settings.path(), Some("/home/tako/soloist"));
}

#[test]
fn a_search_that_finds_nothing_keeps_the_pin_it_had() {
    let mut settings = ready();
    fold(
        &mut settings,
        SoloistUpdate::Facts(facts(None, KeyState::Absent)),
    );
    assert_eq!(settings.path(), Some("/home/tako/soloist"));
    assert_eq!(settings.status(), RowStatus::NotFound);
}

#[test]
fn a_chosen_soloist_scans_again_on_every_entry() {
    let mut settings = ready();
    settings.visit(true);
    settings.visit(false);
    settings.visit(true);
    let scan = Command::Soloist(SoloistAsk::Scan {
        path: Some("/home/tako/soloist".to_owned()),
    });
    assert_eq!(settings.take_outbox(), vec![scan.clone(), scan]);
}

#[test]
fn a_fault_and_a_faulty_scan_clear_the_wait() {
    let mut settings = Settings::default();
    settings.visit(true);
    fold(&mut settings, SoloistUpdate::Fault(SetupFault::NotSoloist));
    assert_eq!(settings.fault(), Some(SetupFault::NotSoloist));
    assert!(!settings.scanning());
}

#[test]
fn the_row_says_not_set_up_before_a_scan_and_unpaired_once_the_key_is_known() {
    let mut settings = Settings::default();
    assert_eq!(settings.status(), RowStatus::NotSetUp);
    fold(
        &mut settings,
        SoloistUpdate::Facts(facts(Some("/soloist"), KeyState::Stored)),
    );
    assert_eq!(settings.status(), RowStatus::Unpaired);
}

#[test]
fn a_missing_key_is_not_set_up_and_a_missing_program_is_not_found() {
    let mut settings = Settings::default();
    fold(
        &mut settings,
        SoloistUpdate::Facts(facts(Some("/soloist"), KeyState::Absent)),
    );
    assert_eq!(settings.status(), RowStatus::NotSetUp);
    fold(
        &mut settings,
        SoloistUpdate::Facts(facts(None, KeyState::Stored)),
    );
    assert_eq!(settings.status(), RowStatus::NotFound);
}

#[test]
fn the_daemons_ready_word_pairs_the_row() {
    let mut settings = ready();
    fold(&mut settings, SoloistUpdate::Daemon(DaemonState::Ready));
    assert_eq!(settings.paired(), Some(true));
    assert_eq!(settings.status(), RowStatus::ReadyPaired);
}

#[test]
fn the_daemons_expiry_wins_over_a_scan_that_reads_fresh() {
    let mut settings = ready();
    fold(&mut settings, SoloistUpdate::Daemon(DaemonState::Expired));
    assert_eq!(settings.status(), RowStatus::Expired);
}

#[test]
fn a_landing_survives_a_daemon_that_restarted_unpaired() {
    let mut settings = ready();
    fold(&mut settings, SoloistUpdate::Pairing(PairEnd::Landed));
    assert_eq!(settings.pairing(), Pairing::Idle);
    fold(&mut settings, SoloistUpdate::Daemon(DaemonState::Unpaired));
    assert_eq!(
        settings.paired(),
        Some(true),
        "the pairing landed this session"
    );
    assert_eq!(settings.status(), RowStatus::ReadyPaired);
    fold(&mut settings, SoloistUpdate::Daemon(DaemonState::Ready));
    fold(&mut settings, SoloistUpdate::Daemon(DaemonState::Unpaired));
    assert_eq!(
        settings.paired(),
        Some(false),
        "the daemon's word now speaks"
    );
    assert_eq!(settings.status(), RowStatus::Unpaired);
}

#[test]
fn a_expired_daemon_ends_the_session_pairing() {
    let mut settings = ready();
    fold(&mut settings, SoloistUpdate::Pairing(PairEnd::Landed));
    fold(&mut settings, SoloistUpdate::Daemon(DaemonState::Expired));
    assert_eq!(settings.paired(), None);
}

#[test]
fn a_landing_closes_the_panel_the_offer_opened() {
    let mut settings = Settings::default();
    settings.reveal_setup();
    assert!(settings.panel());
    fold(&mut settings, SoloistUpdate::Pairing(PairEnd::Landed));
    assert!(!settings.panel());
}

#[test]
fn a_failed_pairing_keeps_its_reason_and_a_stopped_one_goes_back() {
    let mut settings = ready();
    settings.start_pairing();
    assert_eq!(settings.pairing(), Pairing::Waiting);
    fold(
        &mut settings,
        SoloistUpdate::Pairing(PairEnd::Failed(PairFault::Busy)),
    );
    assert_eq!(settings.pairing(), Pairing::Failed(PairFault::Busy));
    fold(&mut settings, SoloistUpdate::Pairing(PairEnd::Stopped));
    assert_eq!(settings.pairing(), Pairing::Idle);
}

#[test]
fn the_daemons_ready_word_closes_a_waiting_pairing() {
    let mut settings = ready();
    settings.start_pairing();
    fold(&mut settings, SoloistUpdate::Daemon(DaemonState::Adopted));
    assert_eq!(settings.pairing(), Pairing::Idle);
}

#[test]
fn pairing_waits_for_a_program_a_key_and_its_own_turn() {
    let mut settings = Settings::default();
    assert!(!settings.pair_ready());
    settings.start_pairing();
    assert!(!settings.pending());

    let mut settings = ready();
    assert!(settings.pair_ready());
    settings.start_pairing();
    assert!(!settings.pair_ready(), "one pairing at a time");
    assert_eq!(
        settings.take_outbox(),
        vec![Command::Soloist(SoloistAsk::Pair)]
    );
}

#[test]
fn an_expired_build_cannot_pair() {
    let mut settings = Settings::default();
    let mut expired = facts(Some("/soloist"), KeyState::Stored);
    expired.build = Freshness::Expired;
    fold(&mut settings, SoloistUpdate::Facts(expired));
    assert!(!settings.pair_ready());
}

#[test]
fn stopping_a_pairing_sends_the_stop_only_while_one_waits() {
    let mut settings = ready();
    settings.stop_pairing();
    assert!(!settings.pending());
    settings.start_pairing();
    settings.take_outbox();
    settings.stop_pairing();
    assert_eq!(
        settings.take_outbox(),
        vec![Command::Soloist(SoloistAsk::StopPair)]
    );
}

#[test]
fn the_steps_are_done_in_the_order_they_must_be() {
    let mut settings = Settings::default();
    settings.choose(SoundChoice::Soloist);
    assert_eq!(settings.steps(), [Step::Active, Step::Ahead, Step::Ahead]);
    fold(
        &mut settings,
        SoloistUpdate::Facts(facts(Some("/soloist"), KeyState::Absent)),
    );
    assert_eq!(settings.steps(), [Step::Done, Step::Active, Step::Ahead]);
    fold(
        &mut settings,
        SoloistUpdate::Facts(facts(Some("/soloist"), KeyState::Stored)),
    );
    assert_eq!(settings.steps(), [Step::Done, Step::Done, Step::Active]);
    fold(&mut settings, SoloistUpdate::Daemon(DaemonState::Ready));
    assert_eq!(settings.steps(), [Step::Done, Step::Done, Step::Done]);
}

#[test]
fn the_expiry_block_speaks_only_when_the_build_is_close_or_past() {
    let mut settings = Settings::default();
    fold(
        &mut settings,
        SoloistUpdate::Facts(facts(Some("/soloist"), KeyState::Stored)),
    );
    assert!(!settings.expiry_block());
    let mut expiring = facts(Some("/soloist"), KeyState::Stored);
    expiring.build = Freshness::Expiring { days_left: 12 };
    fold(&mut settings, SoloistUpdate::Facts(expiring));
    assert!(settings.expiry_block());
    let mut expired = facts(Some("/soloist"), KeyState::Stored);
    expired.build = Freshness::Expired;
    fold(&mut settings, SoloistUpdate::Facts(expired));
    assert!(settings.expiry_block());
}

#[test]
fn the_restart_note_speaks_only_when_the_choice_is_not_what_runs() {
    let mut settings = Settings::default();
    assert!(!settings.restart_note());
    settings.choose(SoundChoice::Soloist);
    assert!(settings.restart_note());
    let prefs = Prefs::default().with_soloist(SoundChoice::Soloist, None, SOLOIST_NAME);
    let loaded = Settings::from_prefs(&prefs);
    assert_eq!(loaded.running(), SoundChoice::Soloist);
    assert!(!loaded.restart_note());
}

#[test]
fn a_session_reset_drops_a_waiting_pairing_and_a_scan() {
    let mut settings = ready();
    settings.start_pairing();
    settings.visit(true);
    settings.reset_session();
    assert_eq!(settings.pairing(), Pairing::Idle);
    assert!(!settings.scanning());
}

#[test]
fn the_outbox_is_handed_over_once() {
    let mut settings = Settings::default();
    settings.open(SoloistPage::Download);
    settings.open(SoloistPage::Key);
    assert_eq!(
        settings.take_outbox(),
        vec![
            Command::Soloist(SoloistAsk::Open(SoloistPage::Download)),
            Command::Soloist(SoloistAsk::Open(SoloistPage::Key)),
        ]
    );
    assert!(settings.take_outbox().is_empty());
}

#[test]
fn a_dropped_file_is_proposed_and_reads_as_the_field() {
    let mut settings = Settings::default();
    settings.drop_file("/home/tako/Downloads/soloist".to_owned());
    assert_eq!(settings.path_field(), "/home/tako/Downloads/soloist");
    assert_eq!(
        settings.take_outbox(),
        vec![Command::Soloist(SoloistAsk::Scan {
            path: Some("/home/tako/Downloads/soloist".to_owned())
        })]
    );
}

#[test]
fn an_empty_field_asks_for_the_auto_search() {
    let mut settings = Settings::default();
    settings.submit_path();
    settings.take_outbox();
    settings.path_field_mut().push_str("  /opt/soloist  ");
    settings.submit_path();
    assert_eq!(
        settings.take_outbox(),
        vec![Command::Soloist(SoloistAsk::Scan {
            path: Some("/opt/soloist".to_owned())
        })]
    );
}
