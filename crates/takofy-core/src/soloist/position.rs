//! Local interpolation of Soloist's playback position.
//!
//! Only the monotonic receipt time is a clock. `timestamp_ms` orders anchors;
//! it is never compared with a wall clock, because the two clocks differ.

use tokio::time::Instant;

/// A position Soloist reported, with the speed it was moving at.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Anchor {
    pub position_ms: u32,
    pub timestamp_ms: i64,
    pub speed: f64,
}

/// The latest anchor, and when it reached us.
#[derive(Debug, Clone, Default)]
pub struct Clock {
    anchor: Option<Anchor>,
    received: Option<Instant>,
}

impl Clock {
    /// Takes an anchor unless a newer one is already held.
    pub fn anchor(&mut self, anchor: Anchor, received: Instant) {
        if let Some(current) = self.anchor
            && anchor.timestamp_ms < current.timestamp_ms
        {
            return;
        }
        self.anchor = Some(anchor);
        self.received = Some(received);
    }

    /// The position now, interpolated from the anchor and clamped to a `u32`.
    pub fn live_ms(&self, now: Instant) -> u32 {
        let (Some(anchor), Some(received)) = (self.anchor, self.received) else {
            return 0;
        };
        let elapsed = now.saturating_duration_since(received).as_secs_f64() * 1000.0;
        let live = f64::from(anchor.position_ms) + elapsed * anchor.speed.max(0.0);
        live.clamp(0.0, f64::from(u32::MAX)).round() as u32
    }

    /// Whether any anchor has arrived.
    pub fn is_anchored(&self) -> bool {
        self.anchor.is_some()
    }

    /// The anchor itself, for a caller that needs its speed.
    pub fn current(&self) -> Option<Anchor> {
        self.anchor
    }
}

#[cfg(test)]
mod tests;
