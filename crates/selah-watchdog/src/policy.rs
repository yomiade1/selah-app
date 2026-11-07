//! When to restart, when to back off, and when to enter safe mode.
use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// A child that sends no heartbeat for this long is treated as hung and restarted.
pub const HUNG_AFTER: Duration = Duration::from_millis(1500);
/// Time a new child gets before heartbeats are required.
pub const STARTUP_GRACE: Duration = Duration::from_secs(5);
/// SAFE-07: this many crashes (of any process) inside [`CRASH_WINDOW`] start safe mode.
pub const SAFE_MODE_CRASHES: usize = 3;
pub const CRASH_WINDOW: Duration = Duration::from_secs(60);
/// A single process that crashes this often inside the window is restarted with a delay.
pub const BACKOFF_AFTER: usize = 5;
pub const MAX_BACKOFF: Duration = Duration::from_secs(10);

/// Crash history across all children.
#[derive(Debug, Default)]
pub struct CrashLog {
    crashes: VecDeque<Instant>,
}

impl CrashLog {
    /// Record a crash. Returns true if this crash means safe mode should start now.
    pub fn record(&mut self, at: Instant) -> bool {
        self.crashes.push_back(at);
        while self
            .crashes
            .front()
            .is_some_and(|first| at.duration_since(*first) > CRASH_WINDOW)
        {
            self.crashes.pop_front();
        }
        self.crashes.len() >= SAFE_MODE_CRASHES
    }

    pub fn recent(&self) -> usize {
        self.crashes.len()
    }
}

/// How long to wait before restarting a process that has crashed `recent` times in the window.
/// The first restarts are immediate, so a crash is recovered well inside 2 seconds.
pub fn restart_delay(recent: usize) -> Duration {
    if recent < BACKOFF_AFTER {
        return Duration::ZERO;
    }
    let steps = (recent - BACKOFF_AFTER).min(5) as u32;
    (Duration::from_millis(500) * 2_u32.pow(steps)).min(MAX_BACKOFF)
}

/// Should a running child be treated as hung?
pub fn is_hung(started: Instant, last_heartbeat: Option<Instant>, now: Instant) -> bool {
    if now.duration_since(started) < STARTUP_GRACE {
        return false;
    }
    match last_heartbeat {
        Some(beat) => now.duration_since(beat) > HUNG_AFTER,
        None => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_crashes_in_sixty_seconds_start_safe_mode() {
        // safe: SAFE-07
        let start = Instant::now();
        let mut log = CrashLog::default();
        assert!(!log.record(start));
        assert!(!log.record(start + Duration::from_secs(20)));
        assert!(log.record(start + Duration::from_secs(59)));
    }

    #[test]
    fn crashes_spread_over_more_than_a_minute_do_not() {
        // safe: SAFE-07
        let start = Instant::now();
        let mut log = CrashLog::default();
        assert!(!log.record(start));
        assert!(!log.record(start + Duration::from_secs(40)));
        assert!(!log.record(start + Duration::from_secs(99)));
        assert_eq!(log.recent(), 2);
    }

    #[test]
    fn early_restarts_are_immediate_and_crash_loops_back_off() {
        assert_eq!(restart_delay(0), Duration::ZERO);
        assert_eq!(restart_delay(4), Duration::ZERO);
        assert_eq!(restart_delay(5), Duration::from_millis(500));
        assert_eq!(restart_delay(6), Duration::from_secs(1));
        assert_eq!(restart_delay(50), MAX_BACKOFF);
    }

    #[test]
    fn heartbeats_are_required_only_after_the_grace_period() {
        let start = Instant::now();
        assert!(!is_hung(start, None, start + Duration::from_secs(1)));
        assert!(is_hung(
            start,
            None,
            start + STARTUP_GRACE + Duration::from_millis(1)
        ));
        let beat = start + Duration::from_secs(10);
        assert!(!is_hung(
            start,
            Some(beat),
            beat + Duration::from_millis(1000)
        ));
        assert!(is_hung(
            start,
            Some(beat),
            beat + Duration::from_millis(1600)
        ));
    }
}
