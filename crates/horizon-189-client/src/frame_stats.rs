//! Allocation-free rolling frame-submit wall times for bounded smoke runs.
use std::time::Duration;

pub struct FrameStats {
    samples: [u64; 1024],
    count: usize,
}
impl Default for FrameStats {
    fn default() -> Self {
        Self {
            samples: [0; 1024],
            count: 0,
        }
    }
}
impl FrameStats {
    pub fn record(&mut self, time: Duration) {
        self.samples[self.count % self.samples.len()] =
            time.as_micros().min(u128::from(u64::MAX)) as u64;
        self.count += 1;
    }
    /// Print `label` with percentiles in microseconds and how many samples
    /// took over twice the median (hitches, for frame intervals).
    pub fn report(&self, label: &str) {
        let count = self.count.min(self.samples.len());
        if count == 0 {
            return;
        }
        let mut sorted = self.samples;
        let sorted = &mut sorted[..count];
        sorted.sort_unstable();
        let percentile = |p| sorted[(count - 1) * p / 100];
        let median = percentile(50);
        let slow = sorted.iter().filter(|&&sample| sample > median * 2).count();
        eprintln!(
            "{label} samples={count} p50={median} p95={} p99={} max={} over_2x_median={slow}",
            percentile(95),
            percentile(99),
            sorted[count - 1]
        );
    }
}
