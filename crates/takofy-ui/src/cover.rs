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
pub const MAX_IN_FLIGHT: usize = 12;

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
    /// A first fetch or an upload is in progress.
    Loading,
    /// The box names no cover.
    Missing,
    /// The cover could not be fetched or decoded, and any retry is still running
    /// or pending.
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
    /// loading; a cover that has failed stays failed through its retries until
    /// an image arrives.
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
mod tests;
