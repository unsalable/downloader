//! Speed and ETA smoothing.
//!
//! A raw "bytes since last tick" figure swings wildly -- TCP delivers in bursts,
//! and at a 100 ms sampling interval the number visibly flickers. Everything the
//! user sees comes from an exponential moving average over fixed-length windows,
//! which settles quickly on a change of pace without jittering on a steady one.

use std::time::{Duration, Instant};

/// How much weight a new sample carries. Low enough to be steady, high enough
/// that throttling or a resumed connection shows up within about a second.
const ALPHA: f64 = 0.22;

/// Samples shorter than this are folded into the next one, so a burst of small
/// chunks cannot produce an absurd instantaneous rate.
const WINDOW: Duration = Duration::from_millis(400);

pub struct SpeedTracker {
    smoothed_bps: f64,
    window_bytes: u64,
    window_start: Instant,
    started: Instant,
    total_bytes: u64,
    /// Bytes already on disk when a resumed download started -- excluded from
    /// the rate so resuming does not report an instant gigabyte per second.
    baseline_bytes: u64,
}

impl SpeedTracker {
    pub fn new(baseline_bytes: u64) -> Self {
        let now = Instant::now();
        Self {
            smoothed_bps: 0.0,
            window_bytes: 0,
            window_start: now,
            started: now,
            total_bytes: baseline_bytes,
            baseline_bytes,
        }
    }

    /// Record freshly received bytes. Returns true when a window closed, which
    /// is the signal to emit a progress update.
    pub fn record(&mut self, bytes: u64) -> bool {
        self.total_bytes += bytes;
        self.window_bytes += bytes;

        let elapsed = self.window_start.elapsed();
        if elapsed < WINDOW {
            return false;
        }

        let instant_bps = self.window_bytes as f64 / elapsed.as_secs_f64();
        self.smoothed_bps = if self.smoothed_bps <= 0.0 {
            instant_bps
        } else {
            ALPHA * instant_bps + (1.0 - ALPHA) * self.smoothed_bps
        };

        self.window_bytes = 0;
        self.window_start = Instant::now();
        true
    }

    /// Called when a transfer stalls, so the readout decays toward zero rather
    /// than freezing at the last healthy rate.
    pub fn decay(&mut self) {
        if self.window_start.elapsed() >= WINDOW * 2 {
            self.smoothed_bps *= 0.5;
            self.window_start = Instant::now();
            self.window_bytes = 0;
        }
    }

    pub fn speed_bps(&self) -> f64 {
        if self.smoothed_bps < 1.0 {
            0.0
        } else {
            self.smoothed_bps
        }
    }

    pub fn received(&self) -> u64 {
        self.total_bytes
    }

    /// Average over the whole transfer. Used for the ETA once enough of the
    /// file has arrived, because it is steadier than the moving average.
    pub fn average_bps(&self) -> f64 {
        let elapsed = self.started.elapsed().as_secs_f64();
        if elapsed < 0.25 {
            return 0.0;
        }
        (self.total_bytes.saturating_sub(self.baseline_bytes)) as f64 / elapsed
    }

    pub fn eta_sec(&self, total: Option<u64>) -> Option<f64> {
        let total = total?;
        if total <= self.total_bytes {
            return Some(0.0);
        }
        let remaining = (total - self.total_bytes) as f64;

        // Blend the moving average with the overall average, weighting the
        // overall one more as the download progresses. Early on the moving
        // average is the only signal; later it is the noisier of the two.
        let progress = (self.total_bytes as f64 / total as f64).clamp(0.0, 1.0);
        let moving = self.speed_bps();
        let overall = self.average_bps();

        let rate = if overall <= 0.0 {
            moving
        } else if moving <= 0.0 {
            overall
        } else {
            moving * (1.0 - progress * 0.6) + overall * (progress * 0.6)
        };

        (rate > 1.0).then(|| remaining / rate)
    }

    pub fn percent(&self, total: Option<u64>) -> Option<f64> {
        let total = total?;
        if total == 0 {
            return None;
        }
        Some((self.total_bytes as f64 / total as f64 * 100.0).clamp(0.0, 100.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_window_closes_before_the_interval_elapses() {
        let mut tracker = SpeedTracker::new(0);
        assert!(!tracker.record(1024));
        assert_eq!(tracker.received(), 1024);
        assert_eq!(tracker.speed_bps(), 0.0);
    }

    #[test]
    fn a_closed_window_produces_a_rate() {
        let mut tracker = SpeedTracker::new(0);
        std::thread::sleep(WINDOW + Duration::from_millis(20));
        assert!(tracker.record(1_000_000));
        assert!(tracker.speed_bps() > 0.0);
    }

    #[test]
    fn resumed_bytes_are_excluded_from_the_average() {
        let tracker = SpeedTracker::new(5_000_000);
        assert_eq!(tracker.received(), 5_000_000);
        // Nothing new has arrived, so the overall rate must stay at zero rather
        // than crediting the pre-existing bytes to this session.
        assert_eq!(tracker.average_bps(), 0.0);
    }

    #[test]
    fn percent_needs_a_known_total() {
        let mut tracker = SpeedTracker::new(0);
        tracker.record(50);
        assert_eq!(tracker.percent(None), None);
        assert_eq!(tracker.percent(Some(200)), Some(25.0));
        assert_eq!(tracker.percent(Some(0)), None);
    }

    #[test]
    fn percent_is_clamped_when_a_source_understates_its_size() {
        let mut tracker = SpeedTracker::new(0);
        tracker.record(300);
        assert_eq!(tracker.percent(Some(200)), Some(100.0));
    }

    #[test]
    fn eta_is_none_without_a_total_or_a_rate() {
        let mut tracker = SpeedTracker::new(0);
        tracker.record(10);
        assert_eq!(tracker.eta_sec(None), None);
        assert_eq!(tracker.eta_sec(Some(1_000_000)), None);
    }

    #[test]
    fn eta_is_zero_once_the_total_is_reached() {
        let mut tracker = SpeedTracker::new(0);
        tracker.record(1000);
        assert_eq!(tracker.eta_sec(Some(1000)), Some(0.0));
    }

    #[test]
    fn a_stall_decays_the_reported_rate() {
        let mut tracker = SpeedTracker::new(0);
        std::thread::sleep(WINDOW + Duration::from_millis(20));
        tracker.record(1_000_000);
        let before = tracker.speed_bps();
        assert!(before > 0.0);

        std::thread::sleep(WINDOW * 2 + Duration::from_millis(20));
        tracker.decay();
        assert!(tracker.speed_bps() < before);
    }
}
