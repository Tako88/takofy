//! The account's devices, the one holding the sound, and that device's
//! transport.
//!
//! A reducer over the bridge's device updates. Time is handed in rather than
//! read, so the watch patience, a flash and the counting position are testable
//! without a clock. It imports no I/O: a press yields a [`Command`], and the
//! bridge is what sends it.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use takofy_model::{Device, DeviceKind, RemotePlayback, RemoteState, TrackEntry};

use crate::message::{Command, DeviceUpdate, Failure, Steer, Update};
use crate::player::{NowPlaying, Transport, TransportState};
use crate::signin::failure_message;

/// How long a watch may go unanswered before the devices are reported as
/// unreachable, so a silent bridge cannot leave the button off for a session.
///
/// A patience, like the redraw interval: a value the interaction depends on,
/// not a design token.
pub const WATCH_PATIENCE: Duration = Duration::from_secs(30);

/// How long a one-line notice about a device stays on screen.
pub const FLASH_FOR: Duration = Duration::from_secs(4);

/// What the caption says about a holder the roster does not name.
const UNNAMED_HOLDER: &str = "another device";
/// The last line of the card, always shown: no row is a control.
pub(crate) const SWITCH_HINT: &str = "Switch devices on the device that is playing.";
/// Shown when the account signed in no other device.
pub(crate) const NO_DEVICES: &str = "No other devices are signed in.";

/// Which transport the bar draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Source {
    /// The window's own player.
    #[default]
    Local,
    /// The device that holds the sound.
    Remote,
}

/// How the devices button draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Button {
    /// There is no list to open yet: the watch has not answered.
    #[default]
    Off,
    /// The list can be opened.
    Ready,
    /// Another device holds the sound.
    Lit,
}

/// One device row of the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub device: Device,
    /// Whether this is the device holding the sound.
    pub holder: bool,
    /// Whether a press on this row can move the sound to it.
    pub pickable: bool,
}

/// How the watch for the account's devices stands.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
enum Watch {
    /// Nothing has been asked for.
    #[default]
    Idle,
    /// Asked and waiting for the first answer. The deadline is stamped by the
    /// first call that carries a clock, because a request carries none.
    Asked { since: Option<Instant> },
    /// The account has answered at least once.
    Live,
    /// No answer ever arrived, so a press may retry.
    Failed(Failure),
}

/// The account's devices as last reported, with the holder's transport.
#[derive(Debug, Clone, Default)]
pub struct Devices {
    /// `None` until a roster arrives. An empty roster is an answer of its own.
    roster: Option<Vec<Device>>,
    holder: Option<String>,
    /// Whether this window is the device holding the sound.
    held_here: bool,
    playback: Option<RemotePlayback>,
    /// The holder's transport. Display only: the shell copies the local level
    /// into it, because volume is inert while remote.
    remote: Transport,
    watch: Watch,
    /// A one-line notice and the instant it stops being drawn.
    flash: Option<(String, Instant)>,
    /// The entries a label named, by URI, so a track that comes back is drawn
    /// from the cache without asking again.
    labelled: HashMap<String, TrackEntry>,
    /// Track URIs a label was asked for and no answer has arrived for.
    asked: HashSet<String>,
    /// A refresh the user's own press asked for, taken once.
    wants_refresh: bool,
    /// The device a press picked, when that press landed in a frame whose one
    /// command was already taken.
    wants_pick: Option<String>,
    /// The unnamed holder a refresh was already asked for.
    named_ask: Option<String>,
    /// When the remote transport's position was anchored.
    anchored: Option<Instant>,
}

impl Devices {
    /// Folds one bridge update in, reporting whether anything drawn changed.
    pub fn apply(&mut self, update: &Update, now: Instant) -> bool {
        let Update::Devices(update) = update else {
            return false;
        };
        match update {
            DeviceUpdate::Roster {
                devices,
                holder,
                held_here,
            } => self.take_roster(devices.clone(), holder.clone(), *held_here, now),
            DeviceUpdate::Playing(playback) => self.take_playback(playback.clone(), now),
            DeviceUpdate::Labelled(entry) => self.take_label(entry),
            DeviceUpdate::Failed(failure) => self.take_failure(*failure),
            DeviceUpdate::SteerFailed => {
                self.flash = Some((failure_message(Failure::Steer).to_owned(), now + FLASH_FOR));
                true
            }
            DeviceUpdate::TransferFailed(failure) => {
                self.flash = Some((failure_message(*failure).to_owned(), now + FLASH_FOR));
                true
            }
        }
    }

    /// The watch request, taken once and only once the library is ready.
    pub fn watch_request(&mut self, ready: bool) -> Option<Command> {
        if !ready || self.watch != Watch::Idle {
            return None;
        }
        self.watch = Watch::Asked { since: None };
        Some(Command::WatchDevices)
    }

    /// The user opened the list: the announce's reply is the only way to learn
    /// about a device that signed in after launch.
    pub fn list_opened(&mut self) {
        // An open while the first answer is still coming is already covered by
        // that answer.
        if matches!(self.watch, Watch::Live | Watch::Failed(_)) {
            self.wants_refresh = true;
        }
    }

    /// The refresh an open asked for, taken once.
    pub fn refresh_request(&mut self) -> Option<Command> {
        if !self.wants_refresh {
            return None;
        }
        self.wants_refresh = false;
        Some(Command::RefreshDevices)
    }

    /// Asks the worker to name the track the holder is playing, once per URI.
    ///
    /// A label that never arrives stays asked, so a track the account cannot
    /// name does not become a request loop.
    pub fn label_request(&mut self) -> Option<Command> {
        let uri = self.playback.as_ref()?.track.uri.clone();
        if uri.is_empty() || self.asked.contains(&uri) || self.labelled.contains_key(&uri) {
            return None;
        }
        if self
            .remote
            .now_playing
            .as_ref()
            .is_some_and(|now_playing| now_playing.uri == uri)
        {
            return None;
        }
        self.asked.insert(uri.clone());
        Some(Command::LabelRemote { track: uri })
    }

    /// One transport press, as the command that steers the holder.
    ///
    /// The device is the holder as it was at the press, so an update that lands
    /// before the command is sent cannot send it somewhere else.
    pub fn steer(&mut self, steer: Steer, now: Instant) -> Option<Command> {
        let device = self.holder.clone()?;
        if !self.steerable() {
            return None;
        }
        if let Steer::SeekTo(position_ms) = steer {
            // The slider is enabled on the display transport's known length
            // alone, so a seek is refused on exactly that.
            if !self.remote.length_known() {
                return None;
            }
            // The bar holds the new place until the push confirms it.
            self.remote.position_ms = position_ms;
            self.remote.scrub = None;
            self.anchored = Some(now);
        }
        Some(Command::Steer { device, steer })
    }

    /// Whether the holder's transport can be steered at all: a known holder
    /// that lets remote control.
    pub fn steerable(&self) -> bool {
        self.holder.is_some()
            && self
                .playback
                .as_ref()
                .is_some_and(|playback| playback.steerable)
    }

    /// The play/pause steer for the holder, or nothing when its state is not
    /// one a toggle acts on. The bar's play tile and the Space key both ask
    /// here, so the rule has one home.
    pub fn toggle(&self) -> Option<Steer> {
        toggle_steer(self.remote.state)
    }

    /// Which transport the bar draws.
    ///
    /// A window that holds the sound draws its own transport however still it
    /// is: a hand-over that landed paused is still this window's track.
    pub fn source(&self, local: &Transport) -> Source {
        let holds = !self.held_here
            && self.holder.is_some()
            && self.playback.is_some()
            && !matches!(
                local.state,
                TransportState::Loading | TransportState::Playing
            );
        if holds { Source::Remote } else { Source::Local }
    }

    /// The holder's transport, for the shell to show and to seed with the local
    /// level.
    pub fn display(&mut self) -> &mut Transport {
        &mut self.remote
    }

    /// Where the remote track is now, counting from the push that anchored it.
    pub fn shown_position(&self, now: Instant) -> u32 {
        let Some(anchored) = self.anchored else {
            return self.remote.position_ms;
        };
        self.remote
            .shown_position(now.saturating_duration_since(anchored))
    }

    /// The playbar's line in remote mode: the holder, or what just happened to
    /// it.
    pub fn caption(&self, now: Instant) -> String {
        if let Some(text) = self.flash_live(now) {
            return text.to_owned();
        }
        let Some(holder) = &self.holder else {
            return String::new();
        };
        self.name_of(holder)
            .map_or_else(|| UNNAMED_HOLDER.to_owned(), str::to_owned)
    }

    /// Whether the sound can be moved from here: this window must have
    /// something to hand over.
    ///
    /// Holding it is enough. Playing here is enough too, because a hand-over
    /// that does not come from the sound's owner never lands: the pick makes
    /// this window active first, then hands over.
    ///
    /// `has_row` is what makes playing here enough: it is the queue's current
    /// entry, without which this window has no state of its own to publish and
    /// the account would be handed a body naming nothing. It is not this card's
    /// `playback`, which reports the device that is playing — `None` exactly
    /// when nothing plays anywhere else, and so false whenever this window is
    /// the only one making sound.
    pub fn can_pick(&self, local: &Transport, held_here: bool, has_row: bool) -> bool {
        held_here
            || (self.source(local) == Source::Local
                && matches!(
                    local.state,
                    TransportState::Playing | TransportState::Loading
                )
                && has_row)
    }

    /// A press on a row: the transfer that moves the sound there.
    ///
    /// Only a device the roster still names is a target. A press in a frame
    /// whose one command is already taken keeps its pick, so it is sent by a
    /// later frame rather than dropped.
    pub fn pick(&mut self, device: &str, free: bool) -> Option<Command> {
        let known = self
            .roster
            .as_ref()
            .is_some_and(|roster| roster.iter().any(|known| known.id == device));
        if !known {
            return None;
        }
        if !free {
            self.wants_pick = Some(device.to_owned());
            return None;
        }
        Some(Command::Transfer {
            to: device.to_owned(),
        })
    }

    /// The pick a frame could not send, taken once.
    pub fn pick_request(&mut self) -> Option<Command> {
        let to = self.wants_pick.take()?;
        Some(Command::Transfer { to })
    }

    /// The device rows: the holder first, then by name. Every row carries the
    /// one answer this window's own transport gives.
    pub fn rows(&self, local: &Transport, has_row: bool) -> Vec<Row> {
        let Some(roster) = &self.roster else {
            return Vec::new();
        };
        let mut devices = roster.clone();
        devices.sort_by(|a, b| (&a.name, &a.id).cmp(&(&b.name, &b.id)));
        if let Some(holder) = &self.holder
            && let Some(index) = devices.iter().position(|device| &device.id == holder)
        {
            let first = devices.remove(index);
            devices.insert(0, first);
        }
        let pickable = self.can_pick(local, self.held_here, has_row);
        devices
            .into_iter()
            .map(|device| {
                let holder = self.holder.as_deref() == Some(device.id.as_str());
                Row {
                    device,
                    holder,
                    pickable,
                }
            })
            .collect()
    }

    /// The card's muted lines, below its rows.
    pub fn notes(&self, now: Instant) -> Vec<String> {
        let mut notes = Vec::new();
        match &self.watch {
            Watch::Failed(failure) => notes.push(failure_message(*failure).to_owned()),
            Watch::Live if self.roster.as_ref().is_none_or(Vec::is_empty) => {
                notes.push(NO_DEVICES.to_owned());
            }
            _ => {}
        }
        if let Some(text) = self.flash_live(now) {
            notes.push(text.to_owned());
        }
        notes.push(SWITCH_HINT.to_owned());
        notes
    }

    /// How the devices button draws.
    pub fn button(&self) -> Button {
        match &self.watch {
            Watch::Idle | Watch::Asked { .. } => Button::Off,
            Watch::Live if self.holder.is_some() => Button::Lit,
            Watch::Live | Watch::Failed(_) => Button::Ready,
        }
    }

    /// How long the window may sleep before this module's own clocks need it.
    pub fn next_wake(&mut self, now: Instant) -> Option<Duration> {
        self.stamp_ask(now);
        let mut soonest = match &self.watch {
            Watch::Asked { since: Some(at) } => Some(*at + WATCH_PATIENCE),
            _ => None,
        };
        if let Some((_, until)) = &self.flash {
            soonest = Some(soonest.map_or(*until, |at| at.min(*until)));
        }
        soonest.map(|at| at.saturating_duration_since(now))
    }

    /// Ends what the clock ends: a watch that went unanswered, and a flash past
    /// its time.
    pub fn expire(&mut self, now: Instant) -> bool {
        self.stamp_ask(now);
        let overdue = matches!(
            &self.watch,
            Watch::Asked { since: Some(at) } if now.saturating_duration_since(*at) >= WATCH_PATIENCE
        );
        let mut changed = false;
        if overdue {
            self.watch = Watch::Failed(Failure::Devices);
            changed = true;
        }
        if let Some((_, until)) = &self.flash
            && *until <= now
        {
            self.flash = None;
            changed = true;
        }
        changed
    }

    fn take_roster(
        &mut self,
        devices: Vec<Device>,
        holder: Option<String>,
        held_here: bool,
        now: Instant,
    ) -> bool {
        let mut changed = self.roster.as_deref() != Some(devices.as_slice())
            || self.holder != holder
            || self.held_here != held_here;
        // A device that held the sound and is no longer on the roster is the one
        // that left; one that is still signed in merely stopped holding it.
        let vacated = if self.holder != holder {
            self.holder
                .clone()
                .filter(|old| !devices.iter().any(|device| &device.id == old))
        } else {
            None
        };
        if let Some(name) = vacated.and_then(|old| self.name_of(&old).map(str::to_owned)) {
            self.flash = Some((format!("{name} went offline"), now + FLASH_FOR));
            changed = true;
        }
        self.roster = Some(devices);
        self.holder = holder;
        self.held_here = held_here;
        self.review_naming();
        let answered = self.reach_answer();
        answered || changed
    }

    fn take_playback(&mut self, playback: Option<RemotePlayback>, now: Instant) -> bool {
        let Some(playback) = playback else {
            let changed = self.reach_answer()
                || self.playback.is_some()
                || self.remote.state != TransportState::Idle;
            self.playback = None;
            // The level the shell seeded each frame is the only field worth
            // keeping.
            let volume = self.remote.volume;
            self.remote = Transport {
                volume,
                ..Transport::default()
            };
            self.anchored = None;
            return changed;
        };
        // A playback is only ever drawn under the device the roster named.
        if self.holder.is_none() {
            return false;
        }
        // A push names the track only by URI, so a new one drops the old title
        // unless the label already named it. A URI that comes back — a replay
        // or a restart — is drawn again at once and asks for nothing.
        let uri = playback.track.uri.as_str();
        if playback.track.named {
            self.labelled
                .insert(playback.track.uri.clone(), playback.track.clone());
        }
        let already_drawn = self
            .remote
            .now_playing
            .as_ref()
            .is_some_and(|now_playing| now_playing.uri == uri);
        if playback.track.named || !already_drawn {
            self.remote.now_playing = self.labelled.get(uri).map(NowPlaying::from);
        }
        self.remote.position_ms = playback.position_ms;
        self.remote.state = state_of(playback.state);
        self.remote.duration_ms = playback.duration_ms;
        // A push mid-drag does not end the pointer's preview: the release still
        // asks for the place the drag landed on.
        if !self.remote.dragging {
            self.remote.scrub = None;
        }
        self.playback = Some(playback);
        self.anchored = Some(now);
        self.reach_answer();
        true
    }

    fn take_label(&mut self, entry: &TrackEntry) -> bool {
        // A label that lands after the track moved on names the wrong track.
        if self
            .playback
            .as_ref()
            .is_none_or(|playback| playback.track.uri != entry.uri)
        {
            return false;
        }
        self.labelled.insert(entry.uri.clone(), entry.clone());
        self.remote.now_playing = Some(NowPlaying::from(entry));
        true
    }

    fn take_failure(&mut self, failure: Failure) -> bool {
        // A roster already on screen outlives a dead pump: quiet staleness beats
        // a card that goes blank and a bar that drops back to local.
        if self.roster.is_some() {
            return false;
        }
        let changed = self.watch != Watch::Failed(failure);
        self.watch = Watch::Failed(failure);
        changed
    }

    /// Wants one refresh for a holder the roster cannot name, once per id.
    fn review_naming(&mut self) {
        let unnamed = self
            .holder
            .clone()
            .filter(|holder| self.name_of(holder).is_none());
        match unnamed {
            Some(id) if self.named_ask.as_deref() != Some(id.as_str()) => {
                self.named_ask = Some(id);
                self.wants_refresh = true;
            }
            Some(_) => {}
            None => self.named_ask = None,
        }
    }

    fn reach_answer(&mut self) -> bool {
        if self.watch == Watch::Live {
            return false;
        }
        self.watch = Watch::Live;
        true
    }

    fn name_of(&self, id: &str) -> Option<&str> {
        self.roster
            .as_ref()?
            .iter()
            .find(|device| device.id == id)
            .map(|device| device.name.as_str())
    }

    fn stamp_ask(&mut self, now: Instant) {
        if let Watch::Asked { since } = &mut self.watch
            && since.is_none()
        {
            *since = Some(now);
        }
    }

    fn flash_live(&self, now: Instant) -> Option<&str> {
        self.flash
            .as_ref()
            .filter(|(_, until)| now < *until)
            .map(|(text, _)| text.as_str())
    }
}

/// The word a row ends with, because the sprite has one devices glyph.
pub fn kind_label(kind: DeviceKind) -> &'static str {
    match kind {
        DeviceKind::Computer => "Computer",
        DeviceKind::Phone => "Phone",
        DeviceKind::Tablet => "Tablet",
        DeviceKind::Speaker => "Speaker",
        DeviceKind::Tv => "TV",
        DeviceKind::Car => "Car",
        DeviceKind::Other => "Other",
    }
}

/// The play/pause steer a transport state asks for: pause what is playing or
/// loading, resume what is paused, and nothing for a state that is neither.
pub fn toggle_steer(state: TransportState) -> Option<Steer> {
    match state {
        TransportState::Playing | TransportState::Loading => Some(Steer::Pause),
        TransportState::Paused => Some(Steer::Resume),
        _ => None,
    }
}

fn state_of(state: RemoteState) -> TransportState {
    match state {
        RemoteState::Playing => TransportState::Playing,
        RemoteState::Paused => TransportState::Paused,
        RemoteState::Loading => TransportState::Loading,
    }
}

#[cfg(test)]
mod tests;
