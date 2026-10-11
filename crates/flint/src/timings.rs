//! The timing log: the last 100 shows, from request to first frame, with
//! their percentiles. This is how the 50 ms budget (DESIGN.md,
//! "Performance budget") is measured on real hardware: every show is
//! logged, and `flint status --timings` prints the recent ones.

use std::collections::VecDeque;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use flint_protocol::client::{TimingSample, Timings};

/// How many samples are kept.
pub const KEPT: usize = 100;
/// The budget for a show, hotkey to presented frame.
pub const BUDGET: Duration = Duration::from_millis(50);

/// The recent shows.
#[derive(Debug, Default)]
pub struct Recorder {
    samples: VecDeque<TimingSample>,
}

impl Recorder {
    /// Records a show and returns the log line for it.
    pub fn record(&mut self, source: &str, elapsed: Duration) -> String {
        if self.samples.len() == KEPT {
            self.samples.pop_front();
        }
        let ms = elapsed.as_secs_f64() * 1000.0;
        let at_unix_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as u64);
        self.samples.push_back(TimingSample {
            source: source.to_owned(),
            ms,
            at_unix_ms,
        });
        let over = if elapsed > BUDGET {
            format!(" (over the {} ms budget)", BUDGET.as_millis())
        } else {
            String::new()
        };
        format!("show via {source}: {ms:.1} ms to first frame{over}")
    }

    /// The samples and their percentiles, for `flint status --timings`.
    pub fn report(&self) -> Timings {
        let mut sorted: Vec<f64> = self.samples.iter().map(|s| s.ms).collect();
        sorted.sort_by(f64::total_cmp);
        Timings {
            samples: self.samples.iter().cloned().collect(),
            p50_ms: percentile(&sorted, 50),
            p95_ms: percentile(&sorted, 95),
        }
    }
}

/// The nearest-rank percentile of sorted values: the smallest value with
/// at least `p`% of the values at or below it.
fn percentile(sorted: &[f64], p: usize) -> Option<f64> {
    if sorted.is_empty() {
        return None;
    }
    let rank = (p * sorted.len()).div_ceil(100).max(1);
    Some(sorted[rank - 1])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentiles_by_nearest_rank() {
        let values: Vec<f64> = (1..=20).map(f64::from).collect();
        assert_eq!(percentile(&values, 50), Some(10.0));
        assert_eq!(percentile(&values, 95), Some(19.0));
        assert_eq!(percentile(&[7.0], 95), Some(7.0));
        assert_eq!(percentile(&[], 50), None);
    }

    #[test]
    fn only_the_last_hundred_are_kept() {
        let mut r = Recorder::default();
        for i in 0..150 {
            r.record("hotkey", Duration::from_millis(i));
        }
        let report = r.report();
        assert_eq!(report.samples.len(), KEPT);
        assert_eq!(report.samples[0].ms, 50.0);
        assert_eq!(report.p95_ms, Some(144.0));
    }

    #[test]
    fn the_log_line_says_when_the_budget_is_missed() {
        let mut r = Recorder::default();
        assert_eq!(
            r.record("hotkey", Duration::from_micros(12_340)),
            "show via hotkey: 12.3 ms to first frame"
        );
        assert!(r
            .record("toggle", Duration::from_millis(80))
            .ends_with("(over the 50 ms budget)"));
    }
}
