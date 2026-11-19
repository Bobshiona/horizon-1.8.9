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
    pub fn report(&self) {
        let count = self.count.min(self.samples.len());
        if count == 0 {
            return;
        }
        let mut sorted = self.samples;
        let sorted = &mut sorted[..count];
        sorted.sort_unstable();
        let percentile = |p| sorted[(count - 1) * p / 100];
        eprintln!(
            "FRAME_SUBMIT_US samples={count} p50={} p95={} p99={} max={}",
            percentile(50),
            percentile(95),
            percentile(99),
            sorted[count - 1]
        );
    }
}
