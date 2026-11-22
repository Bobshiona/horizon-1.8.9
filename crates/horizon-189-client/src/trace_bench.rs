//! `--bench-trace`: statistics for the deterministic packet-trace benchmark.
//! The trace server marks phases with `BOBTRACE PHASE <name>` chat lines and
//! the end with `BOBTRACE END`; every frame and finished mesh in between is
//! counted against the current phase in fixed histograms, so measuring
//! allocates nothing per frame. The report is one `BOBTRACE_RESULT` line per
//! phase.
use std::time::Duration;

/// Microsecond buckets up to 100 ms, then one overflow bucket.
const BUCKETS: usize = 100_001;

pub struct Histogram {
    counts: Box<[u32]>,
    count: u64,
    sum: u64,
}

impl Default for Histogram {
    fn default() -> Self {
        Self {
            counts: vec![0; BUCKETS].into_boxed_slice(),
            count: 0,
            sum: 0,
        }
    }
}

impl Histogram {
    pub fn record(&mut self, time: Duration) {
        let micros = time.as_micros().min(u128::from(u64::MAX)) as u64;
        self.counts[(micros as usize).min(BUCKETS - 1)] += 1;
        self.count += 1;
        self.sum += micros;
    }

    pub fn count(&self) -> u64 {
        self.count
    }

    pub fn mean(&self) -> f64 {
        if self.count == 0 {
            0.0
        } else {
            self.sum as f64 / self.count as f64
        }
    }

    /// The smallest bucket at or above fraction `q` of the samples.
    pub fn quantile(&self, q: f64) -> u64 {
        if self.count == 0 {
            return 0;
        }
        let rank = ((self.count as f64 * q).ceil() as u64).clamp(1, self.count);
        let mut seen = 0u64;
        for (micros, &n) in self.counts.iter().enumerate() {
            seen += u64::from(n);
            if seen >= rank {
                return micros as u64;
            }
        }
        (BUCKETS - 1) as u64
    }

    /// Mean of the slowest `fraction` of samples, from the top down.
    pub fn slowest_mean(&self, fraction: f64) -> f64 {
        let want = ((self.count as f64 * fraction).ceil() as u64).max(1);
        let (mut taken, mut total) = (0u64, 0u64);
        for (micros, &n) in self.counts.iter().enumerate().rev() {
            if taken >= want {
                break;
            }
            let n = u64::from(n).min(want - taken);
            taken += n;
            total += n * micros as u64;
        }
        if taken == 0 {
            0.0
        } else {
            total as f64 / taken as f64
        }
    }
}

#[derive(Default)]
struct Phase {
    name: String,
    seconds: f64,
    interval: Histogram,
    cpu: Histogram,
    gpu: Histogram,
    draws: u64,
    meshes: Histogram,
    allocations: u64,
}

impl Phase {
    fn report(&self) -> String {
        let frames = self.interval.count();
        let fps = |micros: f64| if micros > 0.0 { 1e6 / micros } else { 0.0 };
        let per_frame = |total: u64| {
            if frames == 0 {
                0.0
            } else {
                total as f64 / frames as f64
            }
        };
        format!(
            "BOBTRACE_RESULT phase={} seconds={:.1} frames={frames} avg_fps={:.0} \
             low1_fps={:.0} p50_us={} p99_us={} p999_us={} cpu_p50_us={} cpu_p99_us={} \
             gpu_p50_us={} gpu_p99_us={} draws_per_frame={:.1} allocs_per_frame={:.1} \
             meshes={} mesh_p50_ms={:.1} mesh_p99_ms={:.1}",
            self.name,
            self.seconds,
            fps(self.interval.mean()),
            fps(self.interval.slowest_mean(0.01)),
            self.interval.quantile(0.50),
            self.interval.quantile(0.99),
            self.interval.quantile(0.999),
            self.cpu.quantile(0.50),
            self.cpu.quantile(0.99),
            self.gpu.quantile(0.50),
            self.gpu.quantile(0.99),
            per_frame(self.draws),
            per_frame(self.allocations),
            self.meshes.count(),
            self.meshes.quantile(0.50) as f64 / 1000.0,
            self.meshes.quantile(0.99) as f64 / 1000.0,
        )
    }
}

/// One frame's measurements.
pub struct Frame {
    pub interval: Duration,
    pub cpu: Duration,
    pub gpu: Duration,
    pub draws: u32,
    pub allocations: u64,
}

#[derive(Default)]
pub struct TraceBench {
    /// Allocations are counted (the counting allocator is installed).
    pub counting: bool,
    phases: Vec<Phase>,
    started: Option<std::time::Instant>,
    pub ended: bool,
    /// The main thread's allocation count at the last frame.
    allocations_seen: u64,
}

/// What a chat line from the trace server means.
#[derive(Debug, PartialEq, Eq)]
pub enum Marker<'a> {
    Phase(&'a str),
    End,
}

pub fn marker(text: &str) -> Option<Marker<'_>> {
    let rest = text.trim().strip_prefix("BOBTRACE ")?;
    if rest == "END" {
        return Some(Marker::End);
    }
    rest.strip_prefix("PHASE ")
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(Marker::Phase)
}

impl TraceBench {
    /// Counting when the allocator counts (`alloc-count`).
    pub fn new() -> Self {
        TraceBench {
            counting: horizon_threads::alloc_count::ON,
            allocations_seen: horizon_threads::alloc_count::here().allocs,
            ..TraceBench::default()
        }
    }

    /// Blocks the main thread allocated since the last call (0 when not counting).
    pub fn main_thread_allocations(&mut self) -> u64 {
        let now = horizon_threads::alloc_count::here().allocs;
        let since = now.wrapping_sub(self.allocations_seen);
        self.allocations_seen = now;
        since
    }

    /// A server chat line; true when it was one of the trace's markers.
    pub fn chat(&mut self, text: &str) -> bool {
        match marker(text) {
            Some(Marker::Phase(name)) => {
                self.close_phase();
                self.phases.push(Phase {
                    name: name.to_owned(),
                    ..Phase::default()
                });
                self.started = Some(std::time::Instant::now());
                true
            }
            Some(Marker::End) => {
                self.close_phase();
                self.ended = true;
                true
            }
            None => false,
        }
    }

    fn close_phase(&mut self) {
        if let (Some(phase), Some(started)) = (self.phases.last_mut(), self.started.take()) {
            phase.seconds = started.elapsed().as_secs_f64();
        }
    }

    fn current(&mut self) -> Option<&mut Phase> {
        self.started?;
        self.phases.last_mut()
    }

    pub fn frame(&mut self, frame: &Frame) {
        if let Some(phase) = self.current() {
            phase.interval.record(frame.interval);
            phase.cpu.record(frame.cpu);
            phase.gpu.record(frame.gpu);
            phase.draws += u64::from(frame.draws);
            phase.allocations += frame.allocations;
        }
    }

    /// A column mesh reached the renderer `latency` after it went dirty.
    pub fn mesh(&mut self, latency: Duration) {
        if let Some(phase) = self.current() {
            phase.meshes.record(latency);
        }
    }

    pub fn report(&self) -> Vec<String> {
        self.phases
            .iter()
            .map(|phase| {
                let line = phase.report();
                if self.counting {
                    line
                } else {
                    line.replace(
                        &format!(
                            "allocs_per_frame={:.1}",
                            phase.allocations as f64 / phase.interval.count().max(1) as f64
                        ),
                        "allocs_per_frame=n/a",
                    )
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(v: u64) -> Duration {
        Duration::from_micros(v * 1000)
    }

    #[test]
    fn quantiles_lows_and_overflow_are_exact() {
        let mut h = Histogram::default();
        for _ in 0..990 {
            h.record(ms(4));
        }
        for _ in 0..10 {
            h.record(ms(40));
        }
        assert_eq!(h.quantile(0.5), 4000);
        assert_eq!(h.quantile(0.99), 4000);
        assert_eq!(h.quantile(0.999), 40000);
        // 1% lows: the mean of the slowest 1% of frames, 40 ms here.
        assert_eq!(h.slowest_mean(0.01), 40000.0);
        assert!((h.mean() - 4360.0).abs() < 1e-9);
        // Beyond 100 ms lands in the overflow bucket, not out of bounds.
        h.record(Duration::from_secs(5));
        assert_eq!(h.quantile(1.0), 100_000);
        assert_eq!(Histogram::default().quantile(0.5), 0);
    }

    #[test]
    fn markers_split_frames_into_phases() {
        assert_eq!(marker("BOBTRACE PHASE lobby"), Some(Marker::Phase("lobby")));
        assert_eq!(marker("  BOBTRACE END "), Some(Marker::End));
        assert_eq!(marker("BOBTRACE PHASE "), None);
        assert_eq!(marker("hello"), None);
        let mut bench = TraceBench {
            counting: true,
            ..TraceBench::default()
        };
        let frame = |micros| Frame {
            interval: Duration::from_micros(micros),
            cpu: Duration::from_micros(micros / 2),
            gpu: Duration::from_micros(micros / 4),
            draws: 10,
            allocations: 2,
        };
        // Before the first marker nothing counts.
        bench.frame(&frame(1000));
        assert!(bench.chat("BOBTRACE PHASE lobby"));
        bench.frame(&frame(1000));
        bench.frame(&frame(3000));
        bench.mesh(ms(7));
        assert!(bench.chat("BOBTRACE PHASE flyover"));
        bench.frame(&frame(500));
        assert!(!bench.chat("<Steve> hi"));
        assert!(bench.chat("BOBTRACE END"));
        bench.frame(&frame(9999));
        assert!(bench.ended);
        let report = bench.report();
        assert_eq!(report.len(), 2);
        assert!(report[0].contains("phase=lobby") && report[0].contains("frames=2"));
        assert!(report[0].contains("avg_fps=500") && report[0].contains("draws_per_frame=10.0"));
        assert!(report[0].contains("meshes=1 mesh_p50_ms=7.0"));
        assert!(report[1].contains("phase=flyover") && report[1].contains("frames=1"));
        assert!(report[1].contains("allocs_per_frame=2.0"));
        bench.counting = false;
        assert!(bench.report()[1].contains("allocs_per_frame=n/a"));
    }

    #[test]
    fn main_thread_allocations_count_only_this_threads_blocks() {
        let mut bench = TraceBench::new();
        assert_eq!(bench.counting, horizon_threads::alloc_count::ON);
        let blocks: Vec<Box<u64>> = (0..50).map(Box::new).collect();
        // Another thread's allocations are not the frame's.
        std::thread::spawn(|| (0..500).map(Box::new).collect::<Vec<_>>().len())
            .join()
            .unwrap();
        let counted = bench.main_thread_allocations();
        if bench.counting {
            assert!((51..200).contains(&counted), "{counted}");
        } else {
            assert_eq!(counted, 0);
        }
        drop(blocks);
        assert!(bench.main_thread_allocations() < 50);
    }
}
