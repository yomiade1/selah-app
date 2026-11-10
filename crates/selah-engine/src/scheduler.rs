//! Vsync-locked monotonic frame scheduler (M3.1).
use std::time::{Duration, Instant};

use crate::engine::FRAME_PERIOD;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SchedulerStats {
    pub scheduled_frames: u64,
    pub missed_vsyncs: u64,
    pub max_jitter_ns: u64,
}

/// Pure integer grid step helper for deterministic testing and vsync alignment.
pub fn advance_grid(next_tick_ns: u64, actual_ns: u64, period_ns: u64) -> (u64, u64, u64) {
    let period_ns = period_ns.max(1);
    let target_ns = if next_tick_ns == 0 {
        actual_ns
    } else {
        next_tick_ns
    };
    let jitter = actual_ns.abs_diff(target_ns);
    if actual_ns >= target_ns {
        let overrun = actual_ns - target_ns;
        let missed = overrun / period_ns;
        let next = target_ns + (missed + 1) * period_ns;
        (next, missed, jitter)
    } else {
        (target_ns + period_ns, 0, jitter)
    }
}

/// Tracks vsync-locked frame cadence against an output refresh period.
pub struct FrameScheduler {
    epoch: Instant,
    period_ns: u64,
    next_tick_ns: u64,
    stats: SchedulerStats,
}

impl FrameScheduler {
    pub fn new(epoch: Instant, period: Duration) -> Self {
        Self {
            epoch,
            period_ns: period.as_nanos().max(1) as u64,
            next_tick_ns: 0,
            stats: SchedulerStats::default(),
        }
    }

    pub fn with_default_period(epoch: Instant) -> Self {
        Self::new(epoch, FRAME_PERIOD)
    }

    pub fn stats(&self) -> SchedulerStats {
        self.stats
    }

    /// How long to sleep until the next vsync grid slot when animating.
    pub fn wait_timeout(&self, now: Instant, animating: bool) -> Option<Duration> {
        if !animating {
            return None;
        }
        let elapsed_ns = now.saturating_duration_since(self.epoch).as_nanos() as u64;
        if self.next_tick_ns <= elapsed_ns {
            Some(Duration::ZERO)
        } else {
            Some(Duration::from_nanos(self.next_tick_ns - elapsed_ns))
        }
    }

    /// Record a presented frame on the grid and advance to the next vsync slot without drift.
    pub fn record_present(&mut self, at: Instant) {
        let actual_ns = at.saturating_duration_since(self.epoch).as_nanos() as u64;
        let (next, missed, jitter) = advance_grid(self.next_tick_ns, actual_ns, self.period_ns);
        self.next_tick_ns = next;
        self.stats.scheduled_frames += 1;
        self.stats.missed_vsyncs += missed;
        self.stats.max_jitter_ns = self.stats.max_jitter_ns.max(jitter);
    }

    /// Advance the grid past a skipped idle tick (e.g. a video tick with no new decoded frame)
    /// without counting a scheduled frame or missed vsync.
    pub fn advance_idle(&mut self, at: Instant) {
        let actual_ns = at.saturating_duration_since(self.epoch).as_nanos() as u64;
        let (next, _, _) = advance_grid(self.next_tick_ns, actual_ns, self.period_ns);
        self.next_tick_ns = next;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_advances_without_drift_and_counts_missed_vsyncs() {
        let period = FRAME_PERIOD.as_nanos() as u64;
        let (next1, missed1, _) = advance_grid(period, period + 200_000, period);
        assert_eq!(missed1, 0);
        assert_eq!(next1, period * 2);

        // Jumping past one full vsync period counts 1 missed frame and re-locks to period * 4.
        let (next2, missed2, _) = advance_grid(next1, period * 3 + 500_000, period);
        assert_eq!(missed2, 1);
        assert_eq!(next2, period * 4);
    }

    #[test]
    fn idle_scheduler_does_not_poll_while_animating_returns_remaining_wait() {
        let epoch = Instant::now();
        let mut sched = FrameScheduler::new(epoch, Duration::from_millis(16));
        assert_eq!(sched.wait_timeout(epoch, false), None);
        sched.record_present(epoch);
        let wait = sched
            .wait_timeout(epoch + Duration::from_millis(4), true)
            .expect("animating wait");
        assert_eq!(wait, Duration::from_millis(12));
        assert_eq!(sched.stats().scheduled_frames, 1);
    }
}
