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
            self.remote.now_playing = self.labelled.get(uri).map(now_playing_of);
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
        self.remote.now_playing = Some(now_playing_of(entry));
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

/// What the bar and the pane draw for a labelled entry.
fn now_playing_of(entry: &TrackEntry) -> NowPlaying {
    NowPlaying {
        uri: entry.uri.clone(),
        title: entry.title.clone(),
        artists: entry.artists.clone(),
        cover: entry.cover.clone(),
    }
}

#[cfg(test)]
mod tests {
    use takofy_model::CoverUrl;

    use super::*;

    fn base() -> Instant {
        Instant::now()
    }

    fn device(id: &str, name: &str) -> Device {
        Device {
            id: id.to_owned(),
            name: name.to_owned(),
            kind: DeviceKind::Phone,
        }
    }

    fn phone() -> Device {
        device("a", "Phone")
    }

    fn laptop() -> Device {
        device("b", "Laptop")
    }

    fn entry(uri: &str) -> TrackEntry {
        TrackEntry {
            uri: uri.to_owned(),
            title: uri.to_owned(),
            artists: String::new(),
            album: None,
            album_uri: None,
            artist_uris: Vec::new(),
            artist_names: Vec::new(),
            added_at: None,
            duration_ms: None,
            named: false,
            cover: None,
        }
    }

    fn named_entry(uri: &str, title: &str) -> TrackEntry {
        TrackEntry {
            title: title.to_owned(),
            artists: "Vela".to_owned(),
            named: true,
            cover: CoverUrl::from_wire("https://i.scdn.co/image/abc"),
            ..entry(uri)
        }
    }

    fn playback(state: RemoteState, uri: &str, duration: Option<u32>) -> RemotePlayback {
        RemotePlayback {
            track: entry(uri),
            state,
            position_ms: 1_000,
            duration_ms: duration,
            steerable: true,
        }
    }

    fn roster(devices: Vec<Device>, holder: Option<&str>) -> Update {
        roster_of(devices, holder, false)
    }

    /// The same, saying whether this window is the device holding the sound.
    fn roster_of(devices: Vec<Device>, holder: Option<&str>, held_here: bool) -> Update {
        Update::Devices(DeviceUpdate::Roster {
            devices,
            holder: holder.map(str::to_owned),
            held_here,
        })
    }

    fn playing(playback: Option<RemotePlayback>) -> Update {
        Update::Devices(DeviceUpdate::Playing(playback))
    }

    fn failed(failure: Failure) -> Update {
        Update::Devices(DeviceUpdate::Failed(failure))
    }

    fn labelled(entry: TrackEntry) -> Update {
        Update::Devices(DeviceUpdate::Labelled(entry))
    }

    /// A watch that has answered with two devices, the phone holding nothing.
    fn watching(now: Instant) -> Devices {
        let mut devices = Devices::default();
        assert_eq!(devices.watch_request(true), Some(Command::WatchDevices));
        devices.apply(&roster(vec![phone(), laptop()], Some("a")), now);
        devices
    }

    /// The same, with the phone playing a track.
    fn remote(now: Instant) -> Devices {
        let mut devices = watching(now);
        devices.apply(
            &playing(Some(playback(
                RemoteState::Playing,
                "spotify:track:a",
                Some(10_000),
            ))),
            now,
        );
        devices
    }

    #[test]
    fn the_watch_is_asked_for_once_and_only_once_the_library_is_ready() {
        let mut devices = Devices::default();
        assert_eq!(devices.watch_request(false), None);
        assert_eq!(devices.button(), Button::Off, "a watch not asked is off");
        assert_eq!(devices.watch_request(true), Some(Command::WatchDevices));
        assert_eq!(devices.watch_request(true), None, "asked twice");
    }

    #[test]
    fn a_live_watch_is_never_asked_for_again() {
        let now = base();
        let mut devices = remote(now);
        assert_eq!(devices.watch_request(true), None);
    }

    #[test]
    fn a_failed_update_while_asked_fails_the_watch_and_leaves_the_button_ready() {
        let now = base();
        let mut devices = Devices::default();
        devices.watch_request(true);
        assert!(devices.apply(&failed(Failure::Devices), now));
        assert_eq!(devices.watch, Watch::Failed(Failure::Devices));
        assert_eq!(
            devices.button(),
            Button::Ready,
            "a failure must still let a press retry"
        );
    }

    #[test]
    fn an_unanswered_watch_fails_at_the_patience_deadline() {
        let now = base();
        let mut devices = Devices::default();
        devices.watch_request(true);
        assert!(!devices.expire(now), "the ask was just stamped");

        let just_before = now + WATCH_PATIENCE - Duration::from_millis(1);
        assert!(!devices.expire(just_before));
        assert_eq!(devices.button(), Button::Off);
        assert_eq!(devices.next_wake(now), Some(WATCH_PATIENCE));

        assert!(devices.expire(now + WATCH_PATIENCE));
        assert_eq!(devices.watch, Watch::Failed(Failure::Devices));
        assert_eq!(devices.button(), Button::Ready);
        assert_eq!(devices.next_wake(now + WATCH_PATIENCE), None);
    }

    #[test]
    fn a_press_in_a_failed_watch_asks_for_a_refresh_and_a_roster_recovers() {
        let now = base();
        let mut devices = Devices::default();
        devices.watch_request(true);
        devices.apply(&failed(Failure::Devices), now);

        devices.list_opened();
        assert_eq!(devices.refresh_request(), Some(Command::RefreshDevices));
        assert_eq!(devices.refresh_request(), None, "one refresh per open");

        assert!(devices.apply(&roster(vec![phone()], Some("a")), now));
        assert_eq!(devices.watch, Watch::Live);
        assert_eq!(devices.button(), Button::Lit);
    }

    #[test]
    fn a_roster_replaces_wholesale_and_an_identical_one_reports_unchanged() {
        let now = base();
        let mut devices = watching(now);
        assert!(
            !devices.apply(&roster(vec![phone(), laptop()], Some("a")), now),
            "an identical roster changed something"
        );

        assert!(devices.apply(&roster(vec![laptop()], Some("b")), now));
        let rows = devices.rows(&Transport::default(), false);
        let names: Vec<&str> = rows.iter().map(|row| row.device.name.as_str()).collect();
        assert_eq!(names, vec!["Laptop"]);
    }

    #[test]
    fn an_empty_roster_is_live_with_a_ready_button() {
        let now = base();
        let mut devices = Devices::default();
        devices.watch_request(true);
        assert!(devices.apply(&roster(Vec::new(), None), now));
        assert_eq!(devices.watch, Watch::Live);
        assert_eq!(devices.button(), Button::Ready);
        assert!(devices.rows(&Transport::default(), false).is_empty());
        assert_eq!(
            devices.notes(now),
            vec![NO_DEVICES.to_owned(), SWITCH_HINT.to_owned()]
        );
    }

    #[test]
    fn a_playing_update_without_a_holder_is_ignored() {
        let now = base();
        let mut devices = Devices::default();
        assert!(!devices.apply(
            &playing(Some(playback(
                RemoteState::Playing,
                "spotify:track:a",
                Some(10_000)
            ))),
            now
        ));
        assert_eq!(devices.playback, None);
        assert_eq!(devices.watch, Watch::Idle, "an ignored push answered");
        assert_eq!(devices.source(&Transport::default()), Source::Local);
    }

    #[test]
    fn a_playing_update_maps_the_remote_state_onto_the_display_transport() {
        let now = base();
        for (remote_state, state) in [
            (RemoteState::Playing, TransportState::Playing),
            (RemoteState::Paused, TransportState::Paused),
            (RemoteState::Loading, TransportState::Loading),
        ] {
            let mut devices = watching(now);
            devices.apply(
                &playing(Some(playback(
                    remote_state,
                    "spotify:track:a",
                    Some(10_000),
                ))),
                now,
            );
            assert_eq!(
                devices.remote.state, state,
                "{remote_state:?} mapped to the wrong state"
            );
        }
    }

    #[test]
    fn a_playing_update_anchors_the_position_and_the_length_at_the_push() {
        let now = base();
        let local = Transport::default();
        let mut devices = watching(now);
        // The preview a finished drag left, waiting for the push that confirms it.
        devices.remote.scrub = Some(42);

        assert!(devices.apply(
            &playing(Some(playback(
                RemoteState::Playing,
                "spotify:track:a",
                Some(10_000)
            ))),
            now
        ));

        assert_eq!(devices.shown_position(now), 1_000);
        assert_eq!(
            devices.shown_position(now + Duration::from_millis(500)),
            1_500
        );
        assert_eq!(devices.remote.duration_ms, Some(10_000));
        assert_eq!(devices.remote.scrub, None, "a push ends a stale preview");
        assert!(!devices.remote.dragging);
        assert_eq!(local, Transport::default(), "the local transport moved");
        assert_eq!(devices.source(&local), Source::Remote);
    }

    #[test]
    fn a_push_mid_drag_keeps_the_seek_the_release_asks_for() {
        let now = base();
        let mut devices = remote(now);
        devices.remote.scrub = Some(4_000);
        devices.remote.dragging = true;

        assert!(devices.apply(
            &playing(Some(playback(
                RemoteState::Playing,
                "spotify:track:a",
                Some(10_000)
            ))),
            now
        ));

        assert_eq!(
            devices.remote.scrub,
            Some(4_000),
            "a push dropped the pointer's preview"
        );
        assert!(devices.remote.dragging, "a push ended the drag");
        assert_eq!(
            devices.shown_position(now),
            4_000,
            "the bar draws the push over the drag"
        );
        assert_eq!(
            devices.remote.scrub_end(),
            Some(Command::Seek { position_ms: 4_000 }),
            "the release no longer asks for the place it landed on"
        );
    }

    #[test]
    fn a_playing_update_that_names_nothing_returns_the_transport_to_idle() {
        let now = base();
        let mut devices = remote(now);
        assert!(devices.apply(&playing(None), now));
        assert_eq!(devices.playback, None);
        assert_eq!(devices.remote.state, TransportState::Idle);
        assert_eq!(devices.remote.position_ms, 0);
        assert_eq!(devices.remote.duration_ms, None);
        assert_eq!(devices.source(&Transport::default()), Source::Local);
        assert!(
            !devices.apply(&playing(None), now),
            "nothing playing changed nothing"
        );
    }

    #[test]
    fn an_unnamed_holder_reads_as_another_device_and_asks_for_one_refresh() {
        let now = base();
        let mut devices = Devices::default();
        devices.apply(&roster(vec![phone()], Some("z")), now);
        assert_eq!(devices.caption(now), UNNAMED_HOLDER);
        assert_eq!(devices.refresh_request(), Some(Command::RefreshDevices));

        devices.apply(&roster(vec![phone()], Some("z")), now);
        assert_eq!(
            devices.refresh_request(),
            None,
            "the same unnamed holder was asked for twice"
        );

        devices.apply(&roster(vec![phone()], Some("y")), now);
        assert_eq!(
            devices.refresh_request(),
            Some(Command::RefreshDevices),
            "a different unnamed holder is a new question"
        );
    }

    #[test]
    fn a_roster_that_names_the_holder_ends_the_naming_ask() {
        let now = base();
        let mut devices = Devices::default();
        devices.apply(&roster(vec![phone()], Some("z")), now);
        assert_eq!(devices.refresh_request(), Some(Command::RefreshDevices));

        devices.apply(&roster(vec![device("z", "Tablet")], Some("z")), now);
        assert_eq!(devices.caption(now), "Tablet");
        assert_eq!(devices.refresh_request(), None);

        devices.apply(&roster(vec![phone()], Some("z")), now);
        assert_eq!(devices.caption(now), UNNAMED_HOLDER);
        assert_eq!(
            devices.refresh_request(),
            Some(Command::RefreshDevices),
            "a holder that dropped off the roster is unnamed again"
        );
    }

    #[test]
    fn a_label_is_asked_for_once_per_track_even_when_it_never_arrives() {
        let now = base();
        let mut devices = remote(now);
        assert_eq!(
            devices.label_request(),
            Some(Command::LabelRemote {
                track: "spotify:track:a".to_owned()
            })
        );
        assert_eq!(devices.label_request(), None, "the miss was not cached");

        let mut devices = watching(now);
        assert_eq!(devices.label_request(), None, "nothing playing was asked");

        devices.apply(
            &playing(Some(playback(RemoteState::Paused, "spotify:track:b", None))),
            now,
        );
        assert_eq!(
            devices.label_request(),
            Some(Command::LabelRemote {
                track: "spotify:track:b".to_owned()
            }),
            "a new track is a new question"
        );
    }

    #[test]
    fn a_label_names_the_track_and_its_cover() {
        let now = base();
        let mut devices = remote(now);
        assert!(devices.apply(&labelled(named_entry("spotify:track:a", "Song")), now));
        assert_eq!(
            devices.remote.now_playing,
            Some(NowPlaying {
                uri: "spotify:track:a".to_owned(),
                title: "Song".to_owned(),
                artists: "Vela".to_owned(),
                cover: CoverUrl::from_wire("https://i.scdn.co/image/abc"),
            })
        );
    }

    #[test]
    fn a_label_for_a_track_that_moved_on_is_ignored() {
        let now = base();
        let mut devices = remote(now);
        assert!(!devices.apply(&labelled(named_entry("spotify:track:b", "Other")), now));
        assert_eq!(devices.remote.now_playing, None);
    }

    #[test]
    fn a_track_that_comes_back_is_named_again_without_asking() {
        let now = base();
        let mut devices = remote(now);
        assert!(devices.apply(&labelled(named_entry("spotify:track:a", "Song")), now));

        // The phone moves on to a track no label has named.
        assert!(devices.apply(
            &playing(Some(playback(
                RemoteState::Playing,
                "spotify:track:b",
                Some(10_000)
            ))),
            now
        ));
        assert_eq!(devices.remote.now_playing, None);
        assert_eq!(
            devices.label_request(),
            Some(Command::LabelRemote {
                track: "spotify:track:b".to_owned()
            })
        );

        // And back to the one it named.
        assert!(devices.apply(
            &playing(Some(playback(
                RemoteState::Playing,
                "spotify:track:a",
                Some(10_000)
            ))),
            now
        ));
        assert_eq!(
            devices
                .remote
                .now_playing
                .as_ref()
                .map(|now| now.title.as_str()),
            Some("Song"),
            "a cached name did not come back with its track"
        );
        assert_eq!(
            devices.label_request(),
            None,
            "a cached name was asked again"
        );
    }

    #[test]
    fn stopping_and_starting_the_same_track_keeps_its_name() {
        let now = base();
        let mut devices = remote(now);
        assert!(devices.apply(&labelled(named_entry("spotify:track:a", "Song")), now));

        assert!(devices.apply(&playing(None), now));
        assert_eq!(
            devices.remote.now_playing, None,
            "the stop still wipes the bar"
        );

        assert!(devices.apply(
            &playing(Some(playback(
                RemoteState::Playing,
                "spotify:track:a",
                Some(10_000)
            ))),
            now
        ));
        assert_eq!(
            devices
                .remote
                .now_playing
                .as_ref()
                .map(|now| now.title.as_str()),
            Some("Song"),
            "the replay lost its name"
        );
        assert_eq!(devices.label_request(), None, "the replay was asked again");
    }

    #[test]
    fn a_push_that_already_names_the_track_asks_for_no_label() {
        let now = base();
        let mut devices = watching(now);
        let mut pushed = playback(RemoteState::Playing, "spotify:track:a", Some(10_000));
        pushed.track = named_entry("spotify:track:a", "Song");

        assert!(devices.apply(&playing(Some(pushed)), now));

        assert_eq!(
            devices
                .remote
                .now_playing
                .as_ref()
                .map(|now| now.title.as_str()),
            Some("Song")
        );
        assert_eq!(
            devices.label_request(),
            None,
            "a named push asked for a label"
        );
    }

    #[test]
    fn a_missing_label_leaves_the_title_unset_and_the_caption_drawn() {
        let now = base();
        let devices = remote(now);
        assert_eq!(devices.remote.now_playing, None);
        assert_eq!(devices.caption(now), "Phone");
    }

    #[test]
    fn the_source_rule_picks_the_remote_only_while_the_local_transport_is_silent() {
        let now = base();
        struct Case {
            holder: Option<&'static str>,
            playback: bool,
            local: TransportState,
            source: Source,
        }
        let cases = [
            Case {
                holder: Some("a"),
                playback: true,
                local: TransportState::Playing,
                source: Source::Local,
            },
            Case {
                holder: Some("a"),
                playback: true,
                local: TransportState::Loading,
                source: Source::Local,
            },
            Case {
                holder: Some("a"),
                playback: true,
                local: TransportState::Paused,
                source: Source::Remote,
            },
            Case {
                holder: Some("a"),
                playback: true,
                local: TransportState::Idle,
                source: Source::Remote,
            },
            Case {
                holder: Some("a"),
                playback: true,
                local: TransportState::Stopped,
                source: Source::Remote,
            },
            Case {
                holder: Some("a"),
                playback: true,
                local: TransportState::Failed(Failure::Other),
                source: Source::Remote,
            },
            Case {
                holder: Some("a"),
                playback: false,
                local: TransportState::Idle,
                source: Source::Local,
            },
            Case {
                holder: None,
                playback: true,
                local: TransportState::Idle,
                source: Source::Local,
            },
            Case {
                holder: None,
                playback: false,
                local: TransportState::Idle,
                source: Source::Local,
            },
        ];
        for case in cases {
            let mut devices = Devices::default();
            devices.apply(&roster(vec![phone()], case.holder), now);
            if case.playback {
                devices.apply(
                    &playing(Some(playback(
                        RemoteState::Playing,
                        "spotify:track:a",
                        Some(10_000),
                    ))),
                    now,
                );
            }
            let local = Transport {
                state: case.local,
                ..Transport::default()
            };
            assert_eq!(
                devices.source(&local),
                case.source,
                "holder {:?}, playback {}, local {:?}",
                case.holder,
                case.playback,
                case.local
            );
        }
    }

    #[test]
    fn a_holder_that_leaves_the_roster_flashes_that_it_went_offline() {
        let now = base();
        let mut devices = remote(now);
        assert!(devices.apply(&roster(vec![laptop()], None), now));

        assert_eq!(devices.caption(now), "Phone went offline");
        assert_eq!(
            devices.source(&Transport::default()),
            Source::Local,
            "the bar must fall back to local"
        );
        assert!(devices.expire(now + FLASH_FOR));
        assert_eq!(devices.caption(now + FLASH_FOR), "");
    }

    #[test]
    fn a_holder_that_stops_holding_but_stays_signed_in_clears_silently() {
        let now = base();
        let mut devices = remote(now);
        assert!(devices.apply(&roster(vec![phone(), laptop()], Some("b")), now));

        assert!(devices.flash.is_none(), "a hand-over is not an offline");
        assert_eq!(devices.caption(now), "Laptop");
    }

    #[test]
    fn a_later_flash_replaces_an_earlier_one() {
        let now = base();
        let mut devices = remote(now);
        devices.apply(&roster(vec![laptop()], Some("b")), now);
        assert_eq!(devices.caption(now), "Phone went offline");

        let later = now + Duration::from_secs(1);
        devices.apply(&roster(vec![device("c", "Tablet")], Some("c")), later);
        assert_eq!(devices.caption(later), "Laptop went offline");
    }

    #[test]
    fn steer_is_refused_without_a_holder_or_a_steerable_device() {
        let now = base();
        assert_eq!(Devices::default().steer(Steer::Pause, now), None);

        let mut devices = watching(now);
        assert_eq!(
            devices.steer(Steer::Pause, now),
            None,
            "nothing is known about what the holder plays"
        );

        let forbidden = RemotePlayback {
            steerable: false,
            ..playback(RemoteState::Playing, "spotify:track:a", Some(10_000))
        };
        devices.apply(&playing(Some(forbidden)), now);
        for steer in [
            Steer::Pause,
            Steer::Resume,
            Steer::SkipNext,
            Steer::SkipPrev,
            Steer::SeekTo(1_000),
        ] {
            assert_eq!(devices.steer(steer, now), None, "{steer:?} was steered");
        }
    }

    #[test]
    fn a_seek_is_refused_without_a_known_length() {
        let now = base();
        let mut devices = watching(now);
        devices.apply(
            &playing(Some(playback(
                RemoteState::Playing,
                "spotify:track:a",
                None,
            ))),
            now,
        );

        assert_eq!(devices.steer(Steer::SeekTo(1_000), now), None);
        assert_eq!(
            devices.steer(Steer::Pause, now),
            Some(Command::Steer {
                device: "a".to_owned(),
                steer: Steer::Pause,
            })
        );
    }

    #[test]
    fn steerable_names_a_known_holder_that_allows_remote_control() {
        let now = base();
        assert!(!Devices::default().steerable(), "nothing is known yet");

        let mut devices = watching(now);
        assert!(
            !devices.steerable(),
            "nothing is known about what the holder plays"
        );

        devices.apply(
            &playing(Some(playback(
                RemoteState::Playing,
                "spotify:track:a",
                Some(10_000),
            ))),
            now,
        );
        assert!(devices.steerable());

        devices.apply(&playing(None), now);
        assert!(
            !devices.steerable(),
            "a holder with no playback is not a transport"
        );
    }

    #[test]
    fn a_seek_is_allowed_while_the_holder_is_still_loading() {
        let now = base();
        let mut devices = watching(now);
        devices.apply(
            &playing(Some(playback(
                RemoteState::Loading,
                "spotify:track:a",
                Some(10_000),
            ))),
            now,
        );

        assert_eq!(
            devices.steer(Steer::SeekTo(4_000), now),
            Some(Command::Steer {
                device: "a".to_owned(),
                steer: Steer::SeekTo(4_000),
            }),
            "the bar enables a seek on the length, so the app must act on that"
        );
    }

    #[test]
    fn steer_names_the_holder_it_was_pressed_against() {
        let now = base();
        let mut devices = remote(now);
        let pressed = devices.steer(Steer::SkipNext, now);

        devices.apply(&roster(vec![phone(), laptop()], Some("b")), now);
        assert_eq!(
            pressed,
            Some(Command::Steer {
                device: "a".to_owned(),
                steer: Steer::SkipNext,
            }),
            "a holder change after the press must not retarget the command"
        );
    }

    #[test]
    fn a_steer_moves_the_bar_to_the_seeked_place() {
        let now = base();
        let mut devices = remote(now);
        assert!(devices.apply(
            &playing(Some(playback(
                RemoteState::Playing,
                "spotify:track:a",
                Some(10_000)
            ))),
            now
        ));
        assert_eq!(
            devices.steer(Steer::SeekTo(6_000), now),
            Some(Command::Steer {
                device: "a".to_owned(),
                steer: Steer::SeekTo(6_000),
            })
        );
        assert_eq!(devices.shown_position(now), 6_000);
        assert_eq!(
            devices.shown_position(now + Duration::from_millis(500)),
            6_500
        );
    }

    #[test]
    fn a_failed_steer_flashes_its_copy() {
        let now = base();
        let mut devices = remote(now);
        assert!(devices.apply(&Update::Devices(DeviceUpdate::SteerFailed), now));
        assert_eq!(devices.caption(now), failure_message(Failure::Steer));
        assert!(devices.expire(now + FLASH_FOR));
        assert_eq!(devices.caption(now + FLASH_FOR), "Phone");
    }

    #[test]
    fn a_failed_transfer_flashes_its_copy() {
        let now = base();
        let mut devices = remote(now);
        assert!(devices.apply(
            &Update::Devices(DeviceUpdate::TransferFailed(Failure::Transfer)),
            now
        ));
        assert_eq!(devices.caption(now), failure_message(Failure::Transfer));
    }

    #[test]
    fn holding_here_makes_the_source_local_even_when_the_local_transport_is_paused() {
        let now = base();
        let local = Transport {
            state: TransportState::Paused,
            ..Transport::default()
        };

        let mut devices = watching(now);
        devices.apply(
            &playing(Some(playback(
                RemoteState::Playing,
                "spotify:track:a",
                Some(10_000),
            ))),
            now,
        );
        assert_eq!(
            devices.source(&local),
            Source::Remote,
            "a still window that does not hold the sound shows the holder"
        );

        assert!(devices.apply(&roster_of(vec![phone(), laptop()], Some("a"), true), now));
        assert_eq!(
            devices.source(&local),
            Source::Local,
            "a landed transfer that settled paused is still ours"
        );
    }

    #[test]
    fn a_roster_that_only_changes_who_holds_is_applied() {
        let now = base();
        let mut devices = Devices::default();
        devices.watch_request(true);
        assert!(devices.apply(&roster_of(vec![phone(), laptop()], Some("a"), false), now));

        assert!(
            devices.apply(&roster_of(vec![phone(), laptop()], Some("a"), true), now),
            "a change of who holds the sound went unseen"
        );
    }

    #[test]
    fn a_failed_update_while_live_keeps_the_roster_and_the_playback() {
        let now = base();
        let mut devices = remote(now);
        assert!(!devices.apply(&failed(Failure::Devices), now));

        assert_eq!(devices.watch, Watch::Live);
        assert_eq!(devices.caption(now), "Phone");
        assert_eq!(devices.source(&Transport::default()), Source::Remote);
        assert_eq!(devices.button(), Button::Lit);
        assert!(devices.flash.is_none(), "a dead pump is not an event");
    }

    #[test]
    fn the_button_is_lit_only_while_a_holder_holds_the_sound() {
        let now = base();
        assert_eq!(Devices::default().button(), Button::Off);

        let mut devices = watching(now);
        assert_eq!(devices.button(), Button::Lit);

        devices.apply(&roster(vec![phone(), laptop()], None), now);
        assert_eq!(devices.button(), Button::Ready);
    }

    #[test]
    fn reading_the_source_twice_without_an_update_answers_the_same() {
        let now = base();
        let devices = remote(now);
        let local = Transport::default();
        assert_eq!(devices.source(&local), devices.source(&local));
        assert_eq!(devices.source(&local), Source::Remote);
    }

    #[test]
    fn the_rows_put_the_holder_first_then_by_name() {
        let now = base();
        let mut devices = Devices::default();
        devices.apply(
            &roster(
                vec![
                    device("z", "Zed"),
                    device("a", "Alpha"),
                    device("m", "Beta"),
                ],
                Some("z"),
            ),
            now,
        );
        let rows = devices.rows(&Transport::default(), false);
        let names: Vec<&str> = rows.iter().map(|row| row.device.name.as_str()).collect();
        assert_eq!(names, vec!["Zed", "Alpha", "Beta"]);
        let holders: Vec<bool> = rows.iter().map(|row| row.holder).collect();
        assert_eq!(holders, vec![true, false, false]);
    }

    #[test]
    fn a_row_is_pickable_while_this_window_holds_the_sound() {
        let now = base();
        let mut devices = Devices::default();
        devices.watch_request(true);
        devices.apply(&roster_of(vec![phone(), laptop()], None, true), now);

        let local = Transport::default();
        assert!(devices.can_pick(&local, true, false));
        let pickable: Vec<bool> = devices
            .rows(&local, false)
            .iter()
            .map(|row| row.pickable)
            .collect();
        assert_eq!(pickable, vec![true, true]);
    }

    #[test]
    fn a_row_is_pickable_while_this_window_plays_locally() {
        let now = base();
        // Nothing plays anywhere else, so the account reports no player at all.
        // This window is the one making sound, and that is enough to carry it.
        let devices = watching(now);
        let local = Transport {
            state: TransportState::Playing,
            ..Transport::default()
        };

        assert!(devices.can_pick(&local, false, true));
        let pickable: Vec<bool> = devices
            .rows(&local, true)
            .iter()
            .map(|row| row.pickable)
            .collect();
        assert_eq!(pickable, vec![true, true]);
    }

    #[test]
    fn a_row_is_not_pickable_without_a_current_row() {
        let now = base();
        // The transport plays but the queue holds no current entry, so there is
        // no state of this window's own to hand over.
        let devices = watching(now);
        let local = Transport {
            state: TransportState::Playing,
            ..Transport::default()
        };

        assert!(!devices.can_pick(&local, false, false));
        assert!(devices.rows(&local, false).iter().all(|row| !row.pickable));
    }

    #[test]
    fn a_remote_source_makes_no_row_pickable() {
        let now = base();
        let devices = remote(now);

        for state in [
            TransportState::Idle,
            TransportState::Paused,
            TransportState::Stopped,
        ] {
            let local = Transport {
                state,
                ..Transport::default()
            };
            assert!(!devices.can_pick(&local, false, true), "{state:?}");
            assert!(
                devices.rows(&local, true).iter().all(|row| !row.pickable),
                "{state:?}"
            );
        }
    }

    #[test]
    fn pick_returns_the_transfer_command_for_a_known_device() {
        let now = base();
        let mut devices = remote(now);

        assert_eq!(
            devices.pick("b", true),
            Some(Command::Transfer { to: "b".to_owned() })
        );
        assert_eq!(devices.pick_request(), None, "a sent pick was kept too");
    }

    #[test]
    fn pick_of_an_unknown_device_returns_nothing() {
        let now = base();
        let mut devices = remote(now);

        assert_eq!(devices.pick("z", true), None);
        assert_eq!(devices.pick("z", false), None);
        assert_eq!(
            devices.pick_request(),
            None,
            "an unknown device was kept for a later frame"
        );
    }

    #[test]
    fn a_pick_in_a_taken_frame_is_delayed_not_dropped() {
        let now = base();
        let mut devices = remote(now);

        assert_eq!(devices.pick("b", false), None, "a taken frame sent anyway");
        assert_eq!(
            devices.pick_request(),
            Some(Command::Transfer { to: "b".to_owned() })
        );
        assert_eq!(devices.pick_request(), None, "one pick was sent twice");
    }

    #[test]
    fn the_notes_name_the_failure_or_the_empty_list() {
        let now = base();
        let mut devices = Devices::default();
        devices.watch_request(true);
        devices.apply(&failed(Failure::DevicesOff), now);
        assert_eq!(
            devices.notes(now),
            vec![
                failure_message(Failure::DevicesOff).to_owned(),
                SWITCH_HINT.to_owned()
            ]
        );

        devices.apply(&roster(Vec::new(), None), now);
        assert_eq!(
            devices.notes(now),
            vec![NO_DEVICES.to_owned(), SWITCH_HINT.to_owned()]
        );
    }

    #[test]
    fn every_kind_has_a_word() {
        let labels = [
            DeviceKind::Computer,
            DeviceKind::Phone,
            DeviceKind::Tablet,
            DeviceKind::Speaker,
            DeviceKind::Tv,
            DeviceKind::Car,
            DeviceKind::Other,
        ]
        .map(kind_label);
        assert_eq!(
            labels,
            [
                "Computer", "Phone", "Tablet", "Speaker", "TV", "Car", "Other"
            ]
        );
    }

    #[test]
    fn the_next_wake_is_the_earliest_of_the_watch_and_the_flash() {
        let now = base();
        let mut devices = Devices::default();
        devices.watch_request(true);
        devices.expire(now);
        devices.apply(&Update::Devices(DeviceUpdate::SteerFailed), now);

        assert_eq!(devices.next_wake(now), Some(FLASH_FOR));
        assert!(devices.expire(now + FLASH_FOR));
        assert_eq!(
            devices.next_wake(now + FLASH_FOR),
            Some(WATCH_PATIENCE - FLASH_FOR)
        );
        assert_eq!(devices.button(), Button::Off);
    }

    #[test]
    fn the_play_pause_steer_follows_the_holders_state() {
        let now = base();
        let devices = Devices::default();
        assert_eq!(devices.toggle(), None, "nothing holds the sound yet");

        let mut devices = watching(now);
        assert_eq!(
            devices.toggle(),
            None,
            "a holder with no state toggles nothing"
        );

        for (state, steer) in [
            (RemoteState::Playing, Some(Steer::Pause)),
            (RemoteState::Loading, Some(Steer::Pause)),
            (RemoteState::Paused, Some(Steer::Resume)),
        ] {
            devices.apply(&playing(Some(playback(state, "t1", Some(180_000)))), now);
            assert_eq!(devices.toggle(), steer, "{state:?}");
        }
    }

    #[test]
    fn an_update_about_anything_else_is_ignored() {
        let now = base();
        let mut devices = Devices::default();
        assert!(!devices.apply(&Update::SignedIn, now));
        assert!(!devices.apply(
            &Update::Transport(crate::message::TransportUpdate::Loading),
            now
        ));
        assert_eq!(devices.watch, Watch::Idle);
    }
}
