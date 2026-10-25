//! The window's cover art: the cache, the uploads and the fetches in flight.
//!
//! The view never fetches. It `look`s the cover its box needs and draws the
//! [`Cover`] it gets back, and the app sends the [`Command::Fetch`]es this store
//! reports after the draw has said what it wanted. A frame with nothing pending returns no
//! commands, so an idle window does not spin; a frame with a failed cover
//! returns the time of its retry through [`Art::next_wake`], so the window wakes
//! itself exactly when the next attempt is due.

pub mod cache;
pub mod tint;

use std::collections::HashMap;
use std::time::{Duration, Instant};

use egui::{Color32, ColorImage, TextureHandle, TextureId};
use takofy_model::{CoverImage, CoverSet, CoverUrl};

use crate::message::{Command, Update};
use crate::theme::{self, TintParams, tint_color};
use cache::{Cache, Look};
use tint::{Mean, normalise};

/// The most fetches the window has outstanding at once.
pub const MAX_IN_FLIGHT: usize = 6;

/// The target of the one line an uploaded texture writes.
pub const COVER_LOG_TARGET: &str = "takofy_ui::cover";

/// The device pixels a box `points` wide covers at `pixels_per_point`.
///
/// A box always asks for at least one pixel, and a scale that is not finite
/// asks for no more than that: the pick has no meaningful answer otherwise.
pub fn want_px(points: f32, pixels_per_point: f32) -> u32 {
    let px = points * pixels_per_point;
    if !px.is_finite() || px < 1.0 {
        return 1;
    }
    px.ceil() as u32
}

/// What a cover box draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cover {
    /// The image is decoded and uploaded; this is its texture.
    Ready(TextureId),
    /// A fetch or upload is in progress, or a failed entry is waiting to retry.
    Loading,
    /// The box names no cover.
    Missing,
    /// The cover could not be fetched or decoded.
    Failed,
}

/// An uploaded cover: the texture and the mean of the pixels it came from.
///
/// The mean is kept rather than the finished colour, so another palette can
/// normalise the same pixels without fetching the cover again. The alpha floor
/// is fixed at upload, when the mean is computed; the rest of the palette's
/// normalisation happens per ask, from the stored mean.
struct Held {
    texture: TextureHandle,
    mean: Option<Mean>,
}

/// The decoded covers the window holds, and the fetch state that fills them.
#[derive(Default)]
pub struct Art {
    cache: Cache,
    /// URLs with a fetch sent and not yet answered, and when it was sent.
    in_flight: HashMap<CoverUrl, Instant>,
    /// Decoded images waiting to be uploaded, in arrival order.
    pending: Vec<(CoverUrl, CoverImage)>,
    /// Uploaded covers, keyed by the URL they came from.
    textures: HashMap<CoverUrl, Held>,
}

impl Art {
    /// An empty store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Marks the start of a frame.
    pub fn begin_frame(&mut self) {
        self.cache.begin_frame();
    }

    /// Records that `cover` is wanted this frame and reports what to draw.
    ///
    /// `None` is [`Cover::Missing`]. A URL that has never been looked at starts
    /// loading; a failure that is due to retry becomes loading again; a
    /// permanent failure stays failed.
    pub fn look(&mut self, cover: Option<&CoverSet>, want_px: u32, now: Instant) -> Cover {
        let Some(url) = cover.and_then(|set| set.pick(want_px)) else {
            return Cover::Missing;
        };
        let look = self.cache.look(url, now);
        self.reap();
        match look {
            Look::Ready => match self.textures.get(url) {
                Some(held) => Cover::Ready(held.texture.id()),
                // A cover that arrived this frame and has not been uploaded yet.
                None => Cover::Loading,
            },
            Look::Loading => Cover::Loading,
            Look::Failed => Cover::Failed,
        }
    }

    /// Draws the picked cover when its texture is already held, falling back to
    /// the largest held variant otherwise, and reports `None` when nothing is
    /// held.
    ///
    /// A held cover is still recorded as wanted, so eviction keeps it, but a URL
    /// with nothing to draw is never looked at: the band uses this while the
    /// tracks settle, to keep an already-held picture on screen without fetching
    /// a cover the context image may replace a moment later.
    pub fn look_held(
        &mut self,
        cover: Option<&CoverSet>,
        want_px: u32,
        now: Instant,
    ) -> Option<Cover> {
        let held = self.held_url(cover?, want_px)?;
        self.cache.look(held, now);
        self.reap();
        self.textures
            .get(held)
            .map(|held| Cover::Ready(held.texture.id()))
    }

    /// The URL whose pixels the box should draw for its tint: the exact pick
    /// when it is held, otherwise the nearest held size.
    fn held_url<'a>(&self, set: &'a CoverSet, want_px: u32) -> Option<&'a CoverUrl> {
        let sized = set
            .variants()
            .iter()
            .rev()
            .filter(|variant| variant.effective().is_some())
            .map(|variant| &variant.url);
        std::iter::once(set.pick(want_px)?)
            .chain(sized)
            .chain(set.variants().iter().rev().map(|variant| &variant.url))
            .find(|url| self.textures.contains_key(*url))
    }

    /// The tint of whatever size is held, preferring the box's own pick.
    ///
    /// A tint is a colour, not a resolution: the band colours itself as soon as
    /// any size exists, and barely moves when the larger one lands.
    pub fn tint_held(
        &self,
        cover: Option<&CoverSet>,
        want_px: u32,
        params: &TintParams,
    ) -> Option<Color32> {
        self.tint_of(self.held_url(cover?, want_px)?, params)
    }

    fn tint_of(&self, url: &CoverUrl, params: &TintParams) -> Option<Color32> {
        let mean = self.textures.get(url)?.mean?;
        Some(tint_color(normalise(mean, params)))
    }

    /// Stores an arrived cover's pixels, or records a failure.
    ///
    /// The pixels are uploaded by [`Art::upload`], which the app runs after the
    /// updates are drained and before the draw, so a cover that just arrived is
    /// ready in the same frame.
    ///
    /// An answer is only acted on when it belongs to an outstanding request. A
    /// worker that answers after the store already gave up on a fetch (it
    /// expired the request into a failure and may have retried) must not be
    /// counted twice: the URL is removed from the in-flight set, and the answer
    /// is dropped when it was not there.
    pub fn apply(&mut self, update: &Update, now: Instant) {
        match update {
            Update::Cover { url, image } => {
                if self.in_flight.remove(url).is_none() {
                    return;
                }
                self.pending.push((url.clone(), image.clone()));
                self.cache.insert(url, image.rgba.len());
            }
            Update::CoverFailed { url } => {
                if self.in_flight.remove(url).is_none() {
                    return;
                }
                self.cache.mark_failed(url, now);
            }
            _ => {}
        }
        self.reap();
    }

    /// The fetches to send after this frame's draw.
    ///
    /// At most [`MAX_IN_FLIGHT`] are outstanding in total. A fetch older than
    /// [`cache::IN_FLIGHT_TIMEOUT`] is expired into a failure, so a worker that
    /// never answers cannot hold a slot forever.
    pub fn requests(&mut self, now: Instant) -> Vec<Command> {
        let expired: Vec<CoverUrl> = self
            .in_flight
            .iter()
            .filter(|(_, started)| {
                now.saturating_duration_since(**started) >= cache::IN_FLIGHT_TIMEOUT
            })
            .map(|(url, _)| url.clone())
            .collect();
        for url in expired {
            self.in_flight.remove(&url);
            self.cache.mark_failed(&url, now);
        }

        let mut commands = Vec::new();
        for url in self.cache.wanted() {
            if self.in_flight.len() >= MAX_IN_FLIGHT {
                break;
            }
            if self.in_flight.contains_key(&url) {
                continue;
            }
            self.in_flight.insert(url.clone(), now);
            self.cache.mark_in_flight(&url, now);
            commands.push(Command::Fetch { url });
        }
        commands
    }

    /// Uploads every pending cover once, records its mean, and drops its pixels.
    pub fn upload(&mut self, ctx: &egui::Context, alpha_floor: u8) {
        for (url, image) in std::mem::take(&mut self.pending) {
            let mean = tint::mean(&image.rgba, alpha_floor);
            let pixels = ColorImage::from_rgba_unmultiplied(
                [image.width as usize, image.height as usize],
                &image.rgba,
            );
            let texture = ctx.load_texture(url.as_str(), pixels, theme::art::TEXTURE);
            // Dimensions only: a cover URL is a file id and stays out of the log.
            log::info!(
                target: COVER_LOG_TARGET,
                "cover {}x{}",
                image.width,
                image.height
            );
            self.textures.insert(url, Held { texture, mean });
        }
    }

    /// The soonest instant the store needs another frame.
    pub fn next_wake(&self, now: Instant) -> Option<Duration> {
        self.cache.next_wake(now)
    }

    /// The fetches sent and not yet answered, at most [`MAX_IN_FLIGHT`].
    pub fn outstanding(&self) -> usize {
        self.in_flight.len()
    }

    /// Frees the textures of entries the cache evicted.
    fn reap(&mut self) {
        for url in self.cache.take_removed() {
            self.textures.remove(&url);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Mutex, Once};

    use super::*;

    fn url(tail: &str) -> CoverUrl {
        CoverUrl::from_wire(&format!("https://i.scdn.co/image/{tail}")).expect("a cover url")
    }

    fn set(url: &CoverUrl) -> CoverSet {
        CoverSet::single(url.clone())
    }

    fn image() -> CoverImage {
        CoverImage::new(2, 2, vec![0; 16]).expect("a 2x2 image")
    }

    /// Answers the outstanding fetch for `url`, as a frame would: the URL is
    /// looked at, its fetch is sent, then the worker replies.
    fn cover(art: &mut Art, url: &CoverUrl, now: Instant) {
        let _ = art.look(Some(&set(url)), 64, now);
        let _ = art.requests(now);
        art.apply(
            &Update::Cover {
                url: url.clone(),
                image: image(),
            },
            now,
        );
    }

    fn base() -> Instant {
        Instant::now()
    }

    #[test]
    fn want_px_is_the_box_in_device_pixels() {
        assert_eq!(want_px(36.0, 2.0), 72);
        assert_eq!(want_px(44.0, 2.0), 88);
        assert_eq!(want_px(36.0, 1.0), 36);
        assert_eq!(want_px(36.5, 1.5), 55);
    }

    #[test]
    fn want_px_never_asks_for_less_than_one_pixel() {
        assert_eq!(want_px(0.0, 2.0), 1);
        assert_eq!(want_px(-10.0, 2.0), 1);
        assert_eq!(want_px(f32::NAN, 2.0), 1);
        assert_eq!(want_px(36.0, f32::NAN), 1);
        assert_eq!(want_px(f32::INFINITY, 2.0), 1);
    }

    /// The lines this module logged, so a test can read them.
    static LOGGED: Mutex<Vec<String>> = Mutex::new(Vec::new());

    struct Recorder;

    impl log::Log for Recorder {
        fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
            metadata.level() <= log::Level::Info
        }

        fn log(&self, record: &log::Record<'_>) {
            if record.target() != COVER_LOG_TARGET || !self.enabled(record.metadata()) {
                return;
            }
            LOGGED.lock().expect("lock").push(record.args().to_string());
        }

        fn flush(&self) {}
    }

    #[test]
    fn the_log_target_is_the_module_that_logs() {
        assert_eq!(
            module_path!().strip_suffix("::tests"),
            Some(COVER_LOG_TARGET),
            "the upload's target must be the module that logs it"
        );
    }

    /// One line per uploaded texture, carrying its size and never its URL.
    #[test]
    fn an_upload_logs_its_size_and_no_url() {
        static RECORDER: Recorder = Recorder;
        static INSTALL: Once = Once::new();
        INSTALL.call_once(|| {
            if log::set_logger(&RECORDER).is_ok() {
                log::set_max_level(log::LevelFilter::Info);
            }
        });
        LOGGED.lock().expect("lock").clear();

        let now = base();
        let mut art = Art::new();
        let url = url("secret");
        let _ = art.look(Some(&set(&url)), 64, now);
        let _ = art.requests(now);
        // A size no other test uploads, so this line is the one it read.
        let image = CoverImage::new(3, 5, vec![0; 60]).expect("a 3x5 image");
        art.apply(
            &Update::Cover {
                url: url.clone(),
                image,
            },
            now,
        );
        art.upload(&egui::Context::default(), TintParams::DARK.alpha_floor);

        let lines: Vec<String> = LOGGED
            .lock()
            .expect("lock")
            .iter()
            .filter(|line| line.starts_with("cover 3x5"))
            .cloned()
            .collect();
        assert_eq!(lines, vec!["cover 3x5".to_owned()], "one line per texture");
        assert!(!lines[0].contains("secret"), "a cover URL is a file id");
    }

    #[test]
    fn a_look_picks_the_size_its_box_asks_for() {
        let now = base();
        let mut art = Art::new();
        let small = url("small");
        let large = url("large");
        let set = CoverSet::from_sizes([
            (Some((64, 64)), small.clone()),
            (Some((640, 640)), large.clone()),
        ])
        .expect("a cover set");

        // A small box takes the small URL and sends only that fetch.
        assert_eq!(art.look(Some(&set), 72, now), Cover::Loading);
        let sent: Vec<CoverUrl> = art
            .requests(now)
            .into_iter()
            .filter_map(|command| match command {
                Command::Fetch { url } => Some(url),
                _ => None,
            })
            .collect();
        assert_eq!(sent, vec![small], "the small box fetches the small URL");

        // A large box picks the large URL, keyed separately.
        art.begin_frame();
        assert_eq!(art.look(Some(&set), 352, now), Cover::Loading);
        let sent: Vec<CoverUrl> = art
            .requests(now)
            .into_iter()
            .filter_map(|command| match command {
                Command::Fetch { url } => Some(url),
                _ => None,
            })
            .collect();
        assert_eq!(sent, vec![large], "the large box fetches the large URL");
    }

    #[test]
    fn look_held_falls_back_to_the_largest_held_variant() {
        let now = base();
        let mut art = Art::new();
        let small = url("small");
        let middle = url("middle");
        let large = url("large");
        let set = CoverSet::from_sizes([
            (Some((64, 64)), small.clone()),
            (Some((300, 300)), middle.clone()),
            (Some((640, 640)), large.clone()),
        ])
        .expect("a cover set");
        cover(&mut art, &small, now);
        art.upload(&egui::Context::default(), TintParams::DARK.alpha_floor);
        let held = match art.look(Some(&set), 72, now) {
            Cover::Ready(texture) => texture,
            other => panic!("the 64 should be held: {other:?}"),
        };

        assert_eq!(
            art.look_held(Some(&set), 72, now),
            Some(Cover::Ready(held)),
            "the picked size is held"
        );
        assert_eq!(
            art.look_held(Some(&set), 352, now),
            Some(Cover::Ready(held)),
            "the larger pick falls back to the held 64"
        );
        assert!(
            art.requests(now).is_empty(),
            "a held look never fetches what it cannot draw"
        );
    }

    #[test]
    fn look_held_answers_none_when_nothing_is_held() {
        let now = base();
        let mut art = Art::new();
        let set = CoverSet::from_sizes([
            (Some((64, 64)), url("small")),
            (Some((300, 300)), url("middle")),
            (Some((640, 640)), url("large")),
        ])
        .expect("a cover set");
        assert_eq!(art.look_held(Some(&set), 352, now), None);
        assert!(
            art.requests(now).is_empty(),
            "nothing held means nothing fetched"
        );
    }

    /// Holds `url` with a one-pixel cover of `rgba`.
    fn hold_pixel(art: &mut Art, url: &CoverUrl, rgba: [u8; 4], now: Instant) {
        let _ = art.look(Some(&set(url)), 64, now);
        let _ = art.requests(now);
        art.apply(
            &Update::Cover {
                url: url.clone(),
                image: CoverImage::new(1, 1, rgba.to_vec()).expect("a 1x1 image"),
            },
            now,
        );
        art.upload(&egui::Context::default(), TintParams::DARK.alpha_floor);
    }

    /// The tint of a pure red cover, from the reference table.
    fn red_tint() -> Color32 {
        theme::tint_color([116, 27, 27])
    }

    #[test]
    fn a_held_cover_answers_its_tint() {
        let now = base();
        let mut art = Art::new();
        let url = url("red");
        hold_pixel(&mut art, &url, [255, 0, 0, 255], now);

        assert_eq!(
            art.tint_held(Some(&set(&url)), 64, &TintParams::DARK),
            Some(red_tint())
        );
        assert_eq!(
            art.tint_held(Some(&set(&url)), 640, &TintParams::DARK),
            Some(red_tint()),
            "a set of one answers whatever size is asked for"
        );
    }

    #[test]
    fn a_cover_that_is_not_held_answers_no_tint() {
        let art = Art::new();
        assert_eq!(art.tint_held(None, 64, &TintParams::DARK), None);
        assert_eq!(
            art.tint_held(Some(&set(&url("loading"))), 64, &TintParams::DARK),
            None,
            "a loading cover has no pixels"
        );
        assert_eq!(
            art.tint_held(Some(&set(&url("missing"))), 64, &TintParams::DARK),
            None
        );
    }

    #[test]
    fn held_url_prefers_the_pick_and_falls_back_to_a_held_size() {
        let now = base();
        let mut art = Art::new();
        let small = url("small");
        let large = url("large");
        let set = CoverSet::from_sizes([
            (Some((64, 64)), small.clone()),
            (Some((640, 640)), large.clone()),
        ])
        .expect("a cover set");
        hold_pixel(&mut art, &small, [255, 0, 0, 255], now);

        assert_eq!(
            art.held_url(&set, 64),
            Some(&small),
            "the picked 64 is held"
        );
        assert_eq!(
            art.held_url(&set, 352),
            Some(&small),
            "the picked 640 is not held, so the 64 stands in"
        );
        assert_eq!(
            art.tint_held(Some(&set), 352, &TintParams::DARK),
            Some(red_tint()),
            "the band colours itself from the 64"
        );
    }

    #[test]
    fn an_all_transparent_cover_has_no_mean_and_no_tint() {
        let now = base();
        let mut art = Art::new();
        let url = url("clear");
        cover(&mut art, &url, now);
        art.upload(&egui::Context::default(), TintParams::DARK.alpha_floor);

        assert!(art.textures[&url].mean.is_none());
        assert_eq!(art.tint_held(Some(&set(&url)), 64, &TintParams::DARK), None);
    }

    #[test]
    fn an_evicted_cover_answers_no_tint() {
        let now = base();
        let mut art = Art::new();
        let url = url("evicted");
        hold_pixel(&mut art, &url, [255, 0, 0, 255], now);
        assert!(
            art.tint_held(Some(&set(&url)), 64, &TintParams::DARK)
                .is_some()
        );

        // Two quiet frames unpin the entry, then its own pixels overflow the
        // budget and evict it.
        art.begin_frame();
        art.begin_frame();
        art.cache.insert(&url, cache::MAX_BYTES + 1);
        art.reap();

        assert!(!art.textures.contains_key(&url), "the texture is freed");
        assert_eq!(art.tint_held(Some(&set(&url)), 64, &TintParams::DARK), None);
    }

    #[test]
    fn none_is_missing() {
        let mut art = Art::new();
        assert_eq!(art.look(None, 64, base()), Cover::Missing);
    }

    #[test]
    fn a_cover_arriving_after_loading_is_ready_on_the_next_look() {
        let now = base();
        let mut art = Art::new();
        let url = url("a");
        assert_eq!(art.look(Some(&set(&url)), 64, now), Cover::Loading);
        cover(&mut art, &url, now);
        assert_eq!(
            art.look(Some(&set(&url)), 64, now),
            Cover::Loading,
            "not uploaded yet"
        );
        art.upload(&egui::Context::default(), TintParams::DARK.alpha_floor);
        assert!(matches!(
            art.look(Some(&set(&url)), 64, now),
            Cover::Ready(_)
        ));
    }

    #[test]
    fn upload_happens_once_and_drops_the_pixels() {
        let now = base();
        let mut art = Art::new();
        let url = url("a");
        cover(&mut art, &url, now);
        let ctx = egui::Context::default();
        art.upload(&ctx, TintParams::DARK.alpha_floor);
        assert!(art.pending.is_empty(), "the buffer is dropped");
        let first = art.look(Some(&set(&url)), 64, now);
        art.upload(&ctx, TintParams::DARK.alpha_floor);
        assert_eq!(
            art.look(Some(&set(&url)), 64, now),
            first,
            "the texture is kept"
        );
        assert_eq!(art.textures.len(), 1);
    }

    #[test]
    fn a_failure_shows_as_failed() {
        let now = base();
        let mut art = Art::new();
        let url = url("a");
        // The URL is looked and its fetch sent before it fails, as the frame does.
        assert_eq!(art.look(Some(&set(&url)), 64, now), Cover::Loading);
        assert_eq!(art.requests(now).len(), 1);
        art.apply(&Update::CoverFailed { url: url.clone() }, now);
        assert_eq!(art.look(Some(&set(&url)), 64, now), Cover::Failed);
    }

    #[test]
    fn the_in_flight_cap_holds_at_six() {
        let now = base();
        let mut art = Art::new();
        let urls: Vec<CoverUrl> = (0..10).map(|i| url(&format!("u{i}"))).collect();
        for url in &urls {
            art.look(Some(&set(url)), 64, now);
        }
        assert_eq!(art.requests(now).len(), MAX_IN_FLIGHT);
        assert!(art.requests(now).is_empty(), "the cap is full");
        assert_eq!(art.in_flight.len(), MAX_IN_FLIGHT);

        // One answer frees one slot.
        cover(&mut art, &urls[0], now);
        assert_eq!(art.requests(now).len(), 1);
    }

    #[test]
    fn an_in_flight_timeout_becomes_a_failure() {
        let now = base();
        let mut art = Art::new();
        let url = url("a");
        art.look(Some(&set(&url)), 64, now);
        assert_eq!(art.requests(now).len(), 1);
        art.requests(now + cache::IN_FLIGHT_TIMEOUT);
        assert_eq!(
            art.look(Some(&set(&url)), 64, now + cache::IN_FLIGHT_TIMEOUT),
            Cover::Failed
        );
        assert!(art.in_flight.is_empty());
    }

    #[test]
    fn a_fetch_is_wanted_once_even_across_frames() {
        let now = base();
        let mut art = Art::new();
        let url = url("a");
        art.begin_frame();
        art.look(Some(&set(&url)), 64, now);
        assert_eq!(art.requests(now).len(), 1);
        art.begin_frame();
        art.look(Some(&set(&url)), 64, now);
        assert!(art.requests(now).is_empty());
    }

    #[test]
    fn requests_send_in_look_order_not_alphabetical() {
        let now = base();
        let mut art = Art::new();
        // The alphabetically last URL is drawn first, so it is fetched first.
        let drawn_first = url("z");
        let drawn_second = url("a");
        art.begin_frame();
        art.look(Some(&set(&drawn_first)), 64, now);
        art.look(Some(&set(&drawn_second)), 64, now);
        let sent: Vec<CoverUrl> = art
            .requests(now)
            .into_iter()
            .filter_map(|command| match command {
                Command::Fetch { url } => Some(url),
                _ => None,
            })
            .collect();
        assert_eq!(sent, vec![drawn_first, drawn_second]);
    }

    #[test]
    fn a_due_failure_retries_once_and_stops_after_three_attempts() {
        let now = base();
        let mut art = Art::new();
        let url = url("a");
        art.begin_frame();
        assert_eq!(art.look(Some(&set(&url)), 64, now), Cover::Loading);
        assert_eq!(art.requests(now).len(), 1, "the first attempt");
        art.apply(&Update::CoverFailed { url: url.clone() }, now);
        assert_eq!(art.look(Some(&set(&url)), 64, now), Cover::Failed);

        // Not due yet: the frame sends nothing.
        art.begin_frame();
        assert_eq!(
            art.look(Some(&set(&url)), 64, now + Duration::from_secs(4)),
            Cover::Failed
        );
        assert!(art.requests(now + Duration::from_secs(4)).is_empty());

        // Due at five seconds: exactly one fetch, the second attempt.
        art.begin_frame();
        assert_eq!(
            art.look(Some(&set(&url)), 64, now + Duration::from_secs(5)),
            Cover::Loading
        );
        assert_eq!(
            art.requests(now + Duration::from_secs(5)).len(),
            1,
            "the second attempt"
        );
        art.apply(
            &Update::CoverFailed { url: url.clone() },
            now + Duration::from_secs(5),
        );

        // Due at thirty-five seconds: the third and last fetch.
        art.begin_frame();
        assert_eq!(
            art.look(Some(&set(&url)), 64, now + Duration::from_secs(35)),
            Cover::Loading
        );
        assert_eq!(
            art.requests(now + Duration::from_secs(35)).len(),
            1,
            "the third attempt"
        );
        art.apply(
            &Update::CoverFailed { url: url.clone() },
            now + Duration::from_secs(35),
        );

        // The attempts are spent: it stays failed however long the frame waits.
        art.begin_frame();
        assert_eq!(
            art.look(Some(&set(&url)), 64, now + Duration::from_secs(1_000)),
            Cover::Failed
        );
        assert!(art.requests(now + Duration::from_secs(1_000)).is_empty());
    }

    #[test]
    fn a_late_failure_after_the_in_flight_expiry_is_ignored() {
        let now = base();
        let mut art = Art::new();
        let url = url("a");
        art.begin_frame();
        assert_eq!(art.look(Some(&set(&url)), 64, now), Cover::Loading);
        assert_eq!(art.requests(now).len(), 1);

        // The store gives up first and schedules the retry at the deadline.
        let expired = now + cache::IN_FLIGHT_TIMEOUT;
        assert!(art.requests(expired).is_empty());
        assert!(art.in_flight.is_empty());
        assert_eq!(art.look(Some(&set(&url)), 64, expired), Cover::Failed);

        // The worker's answer arrives late. It must not count as a second
        // failure, which would burn a retry.
        art.apply(&Update::CoverFailed { url: url.clone() }, expired);
        assert_eq!(art.look(Some(&set(&url)), 64, expired), Cover::Failed);

        // The first retry is still due five seconds after the expiry, and it is
        // the second attempt rather than the third.
        let retry = expired + Duration::from_secs(5);
        art.begin_frame();
        assert_eq!(art.look(Some(&set(&url)), 64, retry), Cover::Loading);
        assert_eq!(art.requests(retry).len(), 1);
        art.apply(&Update::CoverFailed { url: url.clone() }, retry);

        art.begin_frame();
        assert_eq!(
            art.look(Some(&set(&url)), 64, retry + Duration::from_secs(1)),
            Cover::Failed
        );
        assert_eq!(
            art.look(Some(&set(&url)), 64, retry + Duration::from_secs(31)),
            Cover::Loading,
            "the third attempt is still available"
        );
    }

    #[test]
    fn an_in_flight_url_is_never_evicted_and_is_answered_once() {
        let now = base();
        let mut art = Art::new();
        let flying = url("flying");
        art.begin_frame();
        art.look(Some(&set(&flying)), 64, now);
        assert_eq!(art.requests(now).len(), 1);

        // Look at it again on later frames while its fetch is still running:
        // no second fetch is sent, so the one answer settles it.
        for _ in 0..3 {
            art.begin_frame();
            assert_eq!(art.look(Some(&set(&flying)), 64, now), Cover::Loading);
            assert!(
                art.requests(now).is_empty(),
                "one fetch is outstanding for the URL"
            );
        }

        // The single answer is applied once and the URL is settled.
        art.apply(
            &Update::Cover {
                url: flying.clone(),
                image: image(),
            },
            now,
        );
        art.upload(&egui::Context::default(), TintParams::DARK.alpha_floor);
        assert!(matches!(
            art.look(Some(&set(&flying)), 64, now),
            Cover::Ready(_)
        ));
        assert!(art.in_flight.is_empty());
    }
}
