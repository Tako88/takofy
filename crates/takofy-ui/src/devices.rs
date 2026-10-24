//! The account's devices, the one holding the sound, and that device's
//! transport.
//!
//! A reducer over the bridge's device updates. Time is handed in rather than
//! read, so the watch patience, a flash and the counting position are testable
//! without a clock. It imports no I/O: a press yields a [`Command`], and the
//! bridge is what sends it.

use std::collections::HashSet;
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
const SWITCH_HINT: &str = "Switch devices from Spotify on the device that is playing.";
/// Shown when the account signed in no other device.
const NO_DEVICES: &str = "No other devices are signed in.";

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
    playback: Option<RemotePlayback>,
    /// The holder's transport. Display only: the shell copies the local level
    /// into it, because volume is inert while remote.
    remote: Transport,
    watch: Watch,
    /// A one-line notice and the instant it stops being drawn.
    flash: Option<(String, Instant)>,
    /// Track URIs a label was asked for, hits and misses alike, so a label that
    /// never arrives is not asked for twice.
    labels: HashSet<String>,
    /// A refresh the user's own press asked for, taken once.
    wants_refresh: bool,
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
            DeviceUpdate::Roster { devices, holder } => {
                self.take_roster(devices.clone(), holder.clone(), now)
            }
            DeviceUpdate::Playing(playback) => self.take_playback(playback.clone(), now),
            DeviceUpdate::Labelled(entry) => self.take_label(entry),
            DeviceUpdate::Failed(failure) => self.take_failure(*failure),
            DeviceUpdate::SteerFailed => {
                self.flash = Some((failure_message(Failure::Steer).to_owned(), now + FLASH_FOR));
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
    /// A label that never arrives stays cached, so a track the account cannot
    /// name does not become a request loop.
    pub fn label_request(&mut self) -> Option<Command> {
        let uri = self.playback.as_ref()?.track.uri.clone();
        if uri.is_empty() || self.labels.contains(&uri) {
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
        self.labels.insert(uri.clone());
        Some(Command::LabelRemote { track: uri })
    }

    /// One transport press, as the command that steers the holder.
    ///
    /// The device is the holder as it was at the press, so an update that lands
    /// before the command is sent cannot send it somewhere else.
    pub fn steer(&mut self, steer: Steer, now: Instant) -> Option<Command> {
        let device = self.holder.clone()?;
        let (steerable, seekable) = self.playback.as_ref().map(|playback| {
            (
                playback.steerable,
                playback.duration_ms.is_some_and(|duration| duration > 0),
            )
        })?;
        if !steerable {
            return None;
        }
        if let Steer::SeekTo(position_ms) = steer {
            if !seekable {
                return None;
            }
            // The bar holds the new place until the push confirms it.
            self.remote.position_ms = position_ms;
            self.remote.scrub = None;
            self.anchored = Some(now);
        }
        Some(Command::Steer { device, steer })
    }

    /// Which transport the bar draws.
    pub fn source(&self, local: &Transport) -> Source {
        let holds = self.holder.is_some()
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

    /// The device rows: the holder first, then by name.
    pub fn rows(&self) -> Vec<Row> {
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
        devices
            .into_iter()
            .map(|device| {
                let holder = self.holder.as_deref() == Some(device.id.as_str());
                Row { device, holder }
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

    fn take_roster(&mut self, devices: Vec<Device>, holder: Option<String>, now: Instant) -> bool {
        let mut changed =
            self.roster.as_deref() != Some(devices.as_slice()) || self.holder != holder;
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
        // until the label read answers.
        if self
            .remote
            .now_playing
            .as_ref()
            .is_some_and(|now_playing| now_playing.uri != playback.track.uri)
        {
            self.remote.now_playing = None;
        }
        self.remote.position_ms = playback.position_ms;
        self.remote.state = state_of(playback.state);
        self.remote.duration_ms = playback.duration_ms;
        self.remote.scrub = None;
        self.remote.dragging = false;
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
        self.labels.insert(entry.uri.clone());
        self.remote.now_playing = Some(NowPlaying {
            uri: entry.uri.clone(),
            title: entry.title.clone(),
            artists: entry.artists.clone(),
            cover: entry.cover.clone(),
        });
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

fn state_of(state: RemoteState) -> TransportState {
    match state {
        RemoteState::Playing => TransportState::Playing,
        RemoteState::Paused => TransportState::Paused,
        RemoteState::Loading => TransportState::Loading,
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
        Update::Devices(DeviceUpdate::Roster {
            devices,
            holder: holder.map(str::to_owned),
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
        let rows = devices.rows();
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
        assert!(devices.rows().is_empty());
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
        devices.remote.scrub = Some(42);
        devices.remote.dragging = true;

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
        let rows = devices.rows();
        let names: Vec<&str> = rows.iter().map(|row| row.device.name.as_str()).collect();
        assert_eq!(names, vec!["Zed", "Alpha", "Beta"]);
        let holders: Vec<bool> = rows.iter().map(|row| row.holder).collect();
        assert_eq!(holders, vec![true, false, false]);
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
