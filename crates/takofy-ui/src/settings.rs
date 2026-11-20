//! The settings page's state: the engine choice, the Soloist setup, and the
//! autoplay switch the queue holds.
//!
//! The page never draws and never sends: an intent changes this state or pushes
//! a command onto the outbox, and the shell hands the outbox on once a frame. A
//! command per widget would be dropped by the shell's one-command frame, so the
//! page collects and the shell drains.

use crate::message::{
    Command, DaemonState, Facts, Freshness, KeyState, PairEnd, PairFault, SetupFault, SoloistAsk,
    SoloistPage, SoloistUpdate, Update,
};
use crate::prefs::{Prefs, SOLOIST_NAME, SoundChoice};
use crate::secret::Secret;

/// The word an engine row reads at a glance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowStatus {
    /// A pairing is in flight.
    Pairing,
    /// The build is past its expiry.
    Expired,
    /// The search found no program.
    NotFound,
    /// A step is missing: nothing scanned yet, or no key stored.
    NotSetUp,
    /// The program and the key are there, and the daemon is not signed in.
    Unpaired,
    /// The daemon answered as this window's own device.
    ReadyPaired,
}

/// Where the pairing step stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Pairing {
    /// Not started, stopped, or finished.
    #[default]
    Idle,
    /// Waiting for the phone.
    Waiting,
    /// Tried and could not finish.
    Failed(PairFault),
}

/// What the setup block draws for one step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Finished: one line.
    Done,
    /// The next thing to do: the whole step.
    Active,
    /// Blocked on an earlier step: the whole step, faded.
    Ahead,
}

/// The settings page's state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// The engine the running session plays through.
    running: SoundChoice,
    /// The engine the preferences name for the next start.
    chosen: SoundChoice,
    /// The pinned program, as the last scan named it.
    path: Option<String>,
    /// The name the daemon announces.
    name: String,
    /// What the last scan found, once one has answered.
    facts: Option<Facts>,
    /// Where the daemon is, once it has said.
    daemon: Option<DaemonState>,
    /// Whether a scan is in flight.
    scanning: bool,
    /// The pairing step.
    pairing: Pairing,
    /// Whether a pairing landed in this session, which outlives a daemon that
    /// restarted before it answered for the pairing.
    paired_this_session: bool,
    /// Why the last setup action failed, until the next attempt.
    fault: Option<SetupFault>,
    /// The program field's text.
    path_field: String,
    /// The key field, emptied by the submit that sends it.
    key_field: Secret,
    /// How many keys this visit has submitted, which salts the field's id so a
    /// submit discards the text egui kept for the old one.
    submit_count: u32,
    /// Whether the setup block is shown although Soloist is not chosen.
    setup_open: bool,
    /// Commands the page asked for, waiting for the shell's drain.
    outbox: Vec<Command>,
    /// Whether the page was on screen last frame, so entering is told apart
    /// from staying.
    was_on_page: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            running: SoundChoice::default(),
            chosen: SoundChoice::default(),
            path: None,
            name: SOLOIST_NAME.to_owned(),
            facts: None,
            daemon: None,
            scanning: false,
            pairing: Pairing::default(),
            paired_this_session: false,
            fault: None,
            path_field: String::new(),
            key_field: Secret::default(),
            submit_count: 0,
            setup_open: false,
            outbox: Vec::new(),
            was_on_page: false,
        }
    }
}

impl Settings {
    /// The page's starting state, from the preferences a session loaded.
    pub fn from_prefs(prefs: &Prefs) -> Self {
        Self {
            running: prefs.engine(),
            chosen: prefs.engine(),
            path: prefs.soloist_path().map(str::to_owned),
            name: prefs.soloist_name().to_owned(),
            ..Self::default()
        }
    }

    /// Folds one worker update in.
    ///
    /// Only a Soloist update moves it; the caller forwards the whole stream,
    /// because a reply is about the setup rather than about the page on screen.
    pub fn apply(&mut self, update: &Update) {
        let Update::Soloist(update) = update else {
            return;
        };
        match update {
            SoloistUpdate::Facts(facts) => {
                self.scanning = false;
                // A program that was found becomes the pin; a search that found
                // nothing leaves the old pin alone, so a typo names itself
                // instead of quietly forgetting where the program was.
                if let Some(program) = &facts.program {
                    self.path = Some(program.clone());
                }
                self.facts = Some(facts.clone());
            }
            SoloistUpdate::Daemon(state) => {
                self.daemon = Some(*state);
                match state {
                    // The daemon's own word now speaks for the pairing, so the
                    // session's bridge is no longer needed.
                    DaemonState::Ready | DaemonState::Adopted => {
                        self.paired_this_session = false;
                        if self.pairing == Pairing::Waiting {
                            self.pairing = Pairing::Idle;
                        }
                    }
                    DaemonState::Expired => self.paired_this_session = false,
                    _ => {}
                }
            }
            SoloistUpdate::Pairing(end) => match end {
                PairEnd::Waiting => {
                    self.pairing = Pairing::Waiting;
                    self.paired_this_session = false;
                }
                PairEnd::Landed => {
                    self.pairing = Pairing::Idle;
                    self.paired_this_session = true;
                    self.setup_open = false;
                }
                PairEnd::Failed(fault) => self.pairing = Pairing::Failed(*fault),
                PairEnd::Stopped => self.pairing = Pairing::Idle,
            },
            SoloistUpdate::Fault(fault) => {
                self.fault = Some(*fault);
                self.scanning = false;
            }
        }
    }

    /// Forgets a pairing and a scan that belonged to the session that just
    /// ended, so the next one starts from what it can actually see.
    pub fn reset_session(&mut self) {
        self.pairing = Pairing::Idle;
        self.scanning = false;
    }

    /// Draws the setup block, whatever the engine choice is.
    pub fn reveal_setup(&mut self) {
        self.setup_open = true;
    }

    /// Chooses the engine the next start plays through.
    ///
    /// Choosing Soloist reveals its setup, because a listener who picks it is
    /// about to be told what it still needs.
    pub fn choose(&mut self, choice: SoundChoice) {
        self.chosen = choice;
        if choice == SoundChoice::Soloist {
            self.setup_open = true;
        }
    }

    /// Asks for the program, from the field's text.
    ///
    /// An empty field means auto-search; anything else is a proposal that
    /// resolves to the pin or reports that nothing is there.
    pub fn submit_path(&mut self) {
        let text = self.path_field.trim();
        let path = if text.is_empty() {
            None
        } else {
            Some(text.to_owned())
        };
        self.ask_scan(path);
    }

    /// Looks again for the program, with the field as it stands.
    pub fn rescan(&mut self) {
        self.submit_path();
    }

    /// Takes a file the listener dropped on the window as the program.
    pub fn drop_file(&mut self, path: String) {
        self.path_field = path.clone();
        self.ask_scan(Some(path));
    }

    /// Sends the pasted key to the keychain, and empties the field.
    ///
    /// Nothing is sent for an empty field, and the field is only emptied by the
    /// submit that carries the key out.
    pub fn submit_key(&mut self) {
        if self.key_field.is_empty() {
            self.fault = Some(SetupFault::KeyEmpty);
            return;
        }
        self.fault = None;
        let key = self.key_field.take();
        self.submit_count = self.submit_count.saturating_add(1);
        self.outbox
            .push(Command::Soloist(SoloistAsk::StoreKey(key)));
    }

    /// Starts the pairing step, if there is something to pair with.
    pub fn start_pairing(&mut self) {
        if !self.pair_ready() {
            return;
        }
        self.fault = None;
        self.pairing = Pairing::Waiting;
        self.outbox.push(Command::Soloist(SoloistAsk::Pair));
    }

    /// Stops the pairing step that is waiting.
    pub fn stop_pairing(&mut self) {
        if self.pairing == Pairing::Waiting {
            self.outbox.push(Command::Soloist(SoloistAsk::StopPair));
        }
    }

    /// Opens one of the setup pages in the browser.
    pub fn open(&mut self, page: SoloistPage) {
        self.outbox.push(Command::Soloist(SoloistAsk::Open(page)));
    }

    /// Notes that the page was entered or left, and asks for a scan on entry.
    ///
    /// A visit is the retry the page has: it looks once when it opens, and looks
    /// again only when the listener asks or leaves and comes back.
    pub fn visit(&mut self, on_page: bool) {
        let entered = on_page && !self.was_on_page;
        self.was_on_page = on_page;
        if !entered {
            return;
        }
        if self.facts.is_none() || self.chosen == SoundChoice::Soloist {
            let path = self.path.clone();
            self.ask_scan(path);
        }
    }

    /// Hands the commands the page asked for to the shell, once.
    pub fn take_outbox(&mut self) -> Vec<Command> {
        std::mem::take(&mut self.outbox)
    }

    /// Whether a command is waiting for the shell.
    pub fn pending(&self) -> bool {
        !self.outbox.is_empty()
    }

    fn ask_scan(&mut self, path: Option<String>) {
        self.fault = None;
        self.scanning = true;
        self.outbox
            .push(Command::Soloist(SoloistAsk::Scan { path }));
    }

    /// Whether the daemon answered as this window's own paired device.
    ///
    /// `None` is unknown, and reads as not paired. A pairing that landed in
    /// this session stays paired until the daemon says it is ready or expired:
    /// the machine restarts after pairing and has not answered for it yet.
    pub fn paired(&self) -> Option<bool> {
        if self.paired_this_session {
            return Some(true);
        }
        match self.daemon {
            Some(DaemonState::Ready | DaemonState::Adopted) => Some(true),
            Some(DaemonState::Unpaired) => Some(false),
            _ => None,
        }
    }

    /// The word the chosen engine's row carries.
    pub fn status(&self) -> RowStatus {
        if self.pairing == Pairing::Waiting {
            return RowStatus::Pairing;
        }
        if self.daemon == Some(DaemonState::Expired) {
            return RowStatus::Expired;
        }
        let Some(facts) = &self.facts else {
            return RowStatus::NotSetUp;
        };
        if facts.build == Freshness::Expired {
            return RowStatus::Expired;
        }
        if facts.program.is_none() {
            return RowStatus::NotFound;
        }
        if facts.key != KeyState::Stored {
            return RowStatus::NotSetUp;
        }
        match self.paired() {
            Some(true) => RowStatus::ReadyPaired,
            Some(false) | None => RowStatus::Unpaired,
        }
    }

    /// The three setup steps, in the order they must be done.
    pub fn steps(&self) -> [Step; 3] {
        let done = [
            self.playable_program(),
            self.key_stored(),
            self.paired() == Some(true),
        ];
        let mut steps = [Step::Done; 3];
        if let Some(next) = done.iter().position(|done| !done) {
            for (slot, step) in steps.iter_mut().enumerate() {
                *step = match slot.cmp(&next) {
                    std::cmp::Ordering::Less => Step::Done,
                    std::cmp::Ordering::Equal => Step::Active,
                    std::cmp::Ordering::Greater => Step::Ahead,
                };
            }
        }
        steps
    }

    /// Whether the pairing step can be started now.
    ///
    /// It needs a program that may run, a stored key, and no pairing already
    /// waiting.
    pub fn pair_ready(&self) -> bool {
        self.playable_program() && self.key_stored() && self.pairing != Pairing::Waiting
    }

    /// Whether the expiry block has something to say.
    pub fn expiry_block(&self) -> bool {
        matches!(
            &self.facts,
            Some(facts)
                if matches!(facts.build, Freshness::Expiring { .. } | Freshness::Expired)
        )
    }

    /// Whether the page says the choice takes effect at the next start.
    pub fn restart_note(&self) -> bool {
        self.chosen != self.running
    }

    /// Whether the setup block is drawn.
    pub fn panel(&self) -> bool {
        self.chosen == SoundChoice::Soloist || self.setup_open
    }

    fn playable_program(&self) -> bool {
        matches!(
            &self.facts,
            Some(facts) if facts.program.is_some() && facts.build != Freshness::Expired
        )
    }

    fn key_stored(&self) -> bool {
        matches!(&self.facts, Some(facts) if facts.key == KeyState::Stored)
    }

    pub fn running(&self) -> SoundChoice {
        self.running
    }

    pub fn chosen(&self) -> SoundChoice {
        self.chosen
    }

    pub fn path(&self) -> Option<&str> {
        self.path.as_deref()
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn facts(&self) -> Option<&Facts> {
        self.facts.as_ref()
    }

    pub fn daemon(&self) -> Option<DaemonState> {
        self.daemon
    }

    pub fn scanning(&self) -> bool {
        self.scanning
    }

    pub fn pairing(&self) -> Pairing {
        self.pairing
    }

    pub fn fault(&self) -> Option<SetupFault> {
        self.fault
    }

    pub fn path_field(&self) -> &str {
        &self.path_field
    }

    pub fn path_field_mut(&mut self) -> &mut String {
        &mut self.path_field
    }

    pub fn key_field_mut(&mut self) -> &mut String {
        self.key_field.edit()
    }

    /// Whether the key field holds anything worth sending.
    pub fn key_field_empty(&self) -> bool {
        self.key_field.is_empty()
    }

    pub fn submit_count(&self) -> u32 {
        self.submit_count
    }
}

#[cfg(test)]
mod tests;
