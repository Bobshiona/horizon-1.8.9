//! Just-in-time frame pacing for vsync. With Fifo and two drawables (one on
//! screen, one being drawn), a frame presented before a vsync is shown at it,
//! and only then does the drawable it replaces come free: the next frame
//! can't be drawn before then. Drawing whenever a timer fires shows input
//! anywhere from a frame's cost to a refresh old; starting each frame a short
//! budget before the vsync after the one its predecessor is shown at shows
//! it about a budget old, every frame.
//!
//! Nothing reports vsync times to safe Rust, so the pacer learns them: an
//! acquire that blocks ended at a vsync. While pacing works no acquire
//! blocks, so every so often a sync frame starts at once to block and
//! re-measure, and a frame that missed its vsync makes the next acquire
//! block, which both re-measures and widens the budget.

use std::time::{Duration, Instant};

/// An acquire that waited this long ended at a vsync.
const BLOCKED: Duration = Duration::from_micros(500);
/// Budget limits, and the start before the frame cost is known.
const MIN_BUDGET: Duration = Duration::from_micros(2500);
const START_BUDGET: Duration = Duration::from_millis(4);
/// Re-measure the vsync phase this often.
const SYNC_EVERY: u32 = 120;
/// Shrink the budget after this many frames without a miss.
const SHRINK_AFTER: u32 = 240;

#[derive(Debug)]
pub struct Pacer {
    period: Duration,
    /// A measured vsync; the others are whole periods from it.
    vsync: Option<Instant>,
    /// How long before its vsync a frame starts.
    budget: Duration,
    /// Slowest recent frame (sample to present), decaying.
    cost: Duration,
    /// How late frames start after the time asked for (the timer and a run
    /// loop spin), decaying: frames are asked for this much earlier.
    late: Duration,
    clean: u32,
    since_sync: u32,
    /// This frame starts at once to block on purpose.
    syncing: bool,
    /// When the last frame was presented, its own cost and the lead it was
    /// given: a miss after a frame slower than its lead was that frame's
    /// fault, not the budget's.
    last_presented: Option<Instant>,
    last_cost: Duration,
    last_lead: Duration,
    pub misses: u64,
}

/// One presented frame, for the pacer.
#[derive(Clone, Copy, Debug)]
pub struct Frame {
    /// When the pacer asked the frame to start, if it did.
    pub planned: Option<Instant>,
    /// When the frame read input (the camera pose).
    pub sampled: Instant,
    /// The acquire's wait, and when it returned.
    pub waited: Duration,
    pub acquired: Instant,
    /// When the frame was presented.
    pub presented: Instant,
}

impl Pacer {
    pub fn new(period: Duration) -> Self {
        Self {
            period,
            vsync: None,
            budget: START_BUDGET,
            cost: Duration::ZERO,
            late: Duration::ZERO,
            clean: 0,
            since_sync: 0,
            syncing: true,
            last_presented: None,
            last_cost: Duration::ZERO,
            last_lead: Duration::MAX,
            misses: 0,
        }
    }

    pub fn set_period(&mut self, period: Duration) {
        if period != self.period {
            self.period = period;
            self.vsync = None;
            self.syncing = true;
        }
    }

    pub fn budget(&self) -> Duration {
        self.budget
    }

    /// The first vsync strictly after `at`, if the phase is known.
    pub fn vsync_after(&self, at: Instant) -> Option<Instant> {
        let vsync = self.vsync?;
        let period = self.period.as_nanos().max(1);
        let next = if at < vsync {
            let behind = (vsync - at).as_nanos();
            vsync - Duration::from_nanos(((behind - 1) / period * period) as u64)
        } else {
            let ahead = (at - vsync).as_nanos();
            vsync + Duration::from_nanos(((ahead / period + 1) * period) as u64)
        };
        Some(next)
    }

    /// Learn from a presented frame. Returns how old its input was when it
    /// reached the screen, by the vsync model, once the phase is known.
    pub fn presented(&mut self, frame: Frame) -> Option<Duration> {
        // The lead this frame was started with, before it changes anything.
        let given = self.budget + self.late;
        let blocked = frame.waited >= BLOCKED;
        if blocked {
            if !self.syncing && self.vsync.is_some() {
                // A paced frame found its drawable still on screen: the
                // previous frame missed its vsync. Only a cheap frame that
                // still missed says the budget is short (GPU or compositor
                // time); a frame that overran its own lead (loading, a
                // hitch) would have missed anyway.
                self.misses += 1;
                self.clean = 0;
                if self.last_cost < self.last_lead {
                    self.budget = (self.budget + Duration::from_millis(1)).min(self.period * 3 / 4);
                }
            }
            self.vsync = Some(frame.acquired);
        } else {
            self.clean += 1;
            if self.clean >= SHRINK_AFTER {
                self.clean = 0;
                let floor = (self.cost + Duration::from_millis(1)).max(MIN_BUDGET);
                self.budget = self
                    .budget
                    .saturating_sub(Duration::from_micros(250))
                    .max(floor);
            }
        }
        // The frame's own work, not the wait for its drawable.
        let cost = frame
            .presented
            .saturating_duration_since(frame.sampled)
            .saturating_sub(frame.waited);
        self.cost = if cost > self.cost {
            cost
        } else {
            // Decay toward lighter frames over about a second.
            self.cost - (self.cost - cost) / 64
        };
        if let Some(planned) = frame.planned {
            let late = frame
                .sampled
                .saturating_duration_since(planned)
                .min(self.period / 4);
            self.late = if late > self.late {
                late
            } else {
                self.late - (self.late - late) / 64
            };
        }
        // Never start so late that the frame's own cost overruns the vsync.
        let floor = (self.cost + Duration::from_millis(1)).max(MIN_BUDGET);
        if self.budget < floor {
            self.budget = floor.min(self.period * 3 / 4);
        }
        self.last_presented = Some(frame.presented);
        self.last_cost = cost;
        self.last_lead = frame.planned.map_or(Duration::MAX, |_| given);
        self.syncing = false;
        self.since_sync += 1;
        if self.since_sync >= SYNC_EVERY {
            self.since_sync = 0;
            self.syncing = true;
        }
        self.vsync_after(frame.presented)
            .map(|shown| shown.saturating_duration_since(frame.sampled))
    }

    /// When the next frame should start: a budget before the vsync after
    /// the one the last frame is shown at (its drawable frees then), or now
    /// for a sync frame or before the phase is known.
    pub fn next_start(&self, now: Instant) -> Instant {
        if self.syncing {
            return now;
        }
        let Some(mut target) = self.vsync_after(self.last_presented.unwrap_or(now)) else {
            return now;
        };
        target += self.period;
        let lead = self.budget + self.late;
        while target.saturating_duration_since(now) < lead {
            target += self.period;
        }
        target - lead
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const P: Duration = Duration::from_micros(16_667);

    fn frame(sampled: Instant, waited: Duration, presented_after: Duration) -> Frame {
        let acquired = sampled + waited;
        Frame {
            planned: None,
            sampled,
            waited,
            acquired,
            presented: acquired + presented_after,
        }
    }

    #[test]
    fn vsyncs_are_whole_periods_from_the_measured_one() {
        let t = Instant::now() + Duration::from_secs(1);
        let mut pacer = Pacer::new(P);
        assert_eq!(pacer.vsync_after(t), None);
        assert_eq!(pacer.next_start(t), t, "unknown phase: start now");
        pacer.vsync = Some(t);
        assert_eq!(pacer.vsync_after(t), Some(t + P));
        assert_eq!(pacer.vsync_after(t + P / 2), Some(t + P));
        assert_eq!(pacer.vsync_after(t - P / 2), Some(t));
        assert_eq!(pacer.vsync_after(t - P), Some(t));
        assert_eq!(pacer.vsync_after(t - P * 3 / 2), Some(t - P));
    }

    #[test]
    fn a_blocking_sync_frame_locks_the_phase_then_frames_start_a_budget_early() {
        let t0 = Instant::now() + Duration::from_secs(1);
        let mut pacer = Pacer::new(P);
        // The sync frame blocks 6 ms: the vsync is at t0 + 6 ms, and the
        // frame (sampled before blocking) is shown a refresh later.
        let age = pacer
            .presented(frame(
                t0,
                Duration::from_millis(6),
                Duration::from_millis(1),
            ))
            .unwrap();
        let vsync = t0 + Duration::from_millis(6);
        assert_eq!(pacer.vsync, Some(vsync));
        assert_eq!(age, Duration::from_millis(6) + P);
        assert_eq!(pacer.misses, 0, "sync frames block on purpose");
        // The sync frame is shown at vsync + P, when the drawable the next
        // frame needs comes free: that frame starts a budget before vsync + 2P.
        let now = vsync + Duration::from_millis(1);
        let start = pacer.next_start(now);
        assert_eq!(start, vsync + P * 2 - pacer.budget());
        // It doesn't block and is shown at that vsync: a budget old.
        let age = pacer
            .presented(frame(start, Duration::ZERO, Duration::from_millis(1)))
            .unwrap();
        assert_eq!(age, pacer.budget());
        assert!(age < Duration::from_millis(5));
    }

    #[test]
    fn a_miss_widens_the_budget_and_clean_frames_narrow_it_to_the_cost() {
        let t0 = Instant::now() + Duration::from_secs(1);
        let mut pacer = Pacer::new(P);
        pacer.presented(frame(
            t0,
            Duration::from_millis(3),
            Duration::from_millis(1),
        ));
        let before = pacer.budget();
        // A paced frame that blocks means the previous one missed.
        pacer.presented(frame(
            t0 + P,
            Duration::from_millis(2),
            Duration::from_millis(1),
        ));
        assert_eq!(pacer.misses, 1);
        assert_eq!(pacer.budget(), before + Duration::from_millis(1));
        // Clean frames costing 1 ms bring it down, but not below the floor.
        for i in 0..5000 {
            pacer.presented(frame(
                t0 + P * (i + 2),
                Duration::ZERO,
                Duration::from_millis(1),
            ));
        }
        assert_eq!(pacer.budget(), MIN_BUDGET);
        // Every SYNC_EVERY frames one starts at once to re-measure.
        let mut syncs = 0;
        for i in 0..(SYNC_EVERY * 3) {
            if pacer.next_start(t0) == t0 {
                syncs += 1;
            }
            pacer.presented(frame(t0 + P * i, Duration::ZERO, Duration::from_millis(1)));
        }
        assert_eq!(syncs, 3);
        // Frames that start 2 ms after they're asked for are asked for 2 ms
        // earlier.
        let mut late = frame(t0, Duration::ZERO, Duration::from_millis(1));
        late.planned = Some(t0 - Duration::from_millis(2));
        pacer.presented(late);
        let start = pacer.next_start(t0);
        let target = pacer.vsync_after(late.presented).unwrap() + P;
        assert_eq!(start, target - pacer.budget() - Duration::from_millis(2));
        // A miss after a frame that overran its own lead is that frame's,
        // not the budget's.
        let before = pacer.budget();
        let mut slow = frame(t0, Duration::ZERO, P);
        slow.planned = Some(t0);
        pacer.presented(slow);
        let after_slow = pacer.budget();
        pacer.presented(frame(
            t0 + P,
            Duration::from_millis(2),
            Duration::from_millis(1),
        ));
        assert_eq!(
            pacer.budget(),
            after_slow,
            "slow frame's miss leaves the budget"
        );
        assert!(after_slow >= before);
        // A heavy frame raises the floor at once.
        pacer.presented(frame(t0, Duration::ZERO, Duration::from_millis(6)));
        assert!(pacer.budget() >= Duration::from_millis(7));
    }
}
