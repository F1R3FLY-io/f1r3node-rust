use std::collections::VecDeque;
use std::time::Instant;

/// Weight of the newest sample in the wall-time-per-height average.
const SAMPLE_WEIGHT: f64 = 0.2;

/// One height span the queue learned something in.
#[derive(Debug, Clone, Copy)]
struct Sample {
    heights: f64,
    settled: u64,
    held_back: bool,
}

/// How fast the shard settles this queue's deploys, as the queue observes
/// it: its own released deploys that reach a verdict per height, and wall
/// time per height.
///
/// Settlement, not inclusion, is the capacity signal: a validator puts
/// everything in its pool into its next block, so inclusion follows whatever
/// the queue releases, while an overloaded shard shows up as slower heights
/// and verdicts falling behind.
///
/// Verdicts arrive in bursts, one per finality advance, so the rate is the
/// average over a window of recent heights, never a single height. Heights
/// with nothing unsettled say nothing and are left out. When the queue held
/// deploys back for want of room somewhere in the window, the window measures
/// capacity and the estimate follows it either way. When it gave the node
/// everything it had, the window measures only demand: the estimate can rise
/// to it, never fall. A fresh queue starts at one deploy per height, so it
/// probes the shard rather than assuming it is idle.
#[derive(Debug, Clone)]
pub struct Pace {
    window_heights: f64,
    samples: VecDeque<Sample>,
    settled_per_height: f64,
    secs_per_height: Option<f64>,
    last_tip: Option<(i64, Instant)>,
    settled_since_tip: u64,
    unsettled_since_tip: bool,
    held_back_since_tip: bool,
}

impl Pace {
    /// A pace averaging settlement over `window_heights` heights.
    pub fn new(window_heights: i64) -> Self {
        Self {
            window_heights: window_heights.max(1) as f64,
            samples: VecDeque::new(),
            settled_per_height: 1.0,
            secs_per_height: None,
            last_tip: None,
            settled_since_tip: 0,
            unsettled_since_tip: false,
            held_back_since_tip: false,
        }
    }

    /// Counts released deploys that reached a verdict.
    pub fn record_settled(&mut self, count: u64) { self.settled_since_tip += count; }

    /// Marks that released deploys were waiting for a verdict during the
    /// current height.
    pub fn record_unsettled(&mut self) { self.unsettled_since_tip = true; }

    /// Marks that the queue withheld deploys during the current height
    /// because its released deploys already filled the budget.
    pub fn record_held_back(&mut self) { self.held_back_since_tip = true; }

    pub fn on_tip(&mut self, tip: i64, now: Instant) {
        let Some((previous, at)) = self.last_tip else {
            self.last_tip = Some((tip, now));
            return;
        };
        if tip <= previous {
            return;
        }
        let heights = (tip - previous) as f64;
        let secs = now.duration_since(at).as_secs_f64() / heights;
        self.secs_per_height = Some(match self.secs_per_height {
            Some(average) => average * (1.0 - SAMPLE_WEIGHT) + secs * SAMPLE_WEIGHT,
            None => secs,
        });
        if self.unsettled_since_tip || self.held_back_since_tip || self.settled_since_tip > 0 {
            self.samples.push_back(Sample {
                heights,
                settled: self.settled_since_tip,
                held_back: self.held_back_since_tip,
            });
            self.trim_window();
            self.reestimate();
        }
        self.settled_since_tip = 0;
        self.unsettled_since_tip = false;
        self.held_back_since_tip = false;
        self.last_tip = Some((tip, now));
    }

    fn trim_window(&mut self) {
        let mut span: f64 = self.samples.iter().map(|s| s.heights).sum();
        while span > self.window_heights {
            let Some(oldest) = self.samples.front() else {
                break;
            };
            if span - oldest.heights < self.window_heights {
                break;
            }
            span -= oldest.heights;
            self.samples.pop_front();
        }
    }

    fn reestimate(&mut self) {
        let span: f64 = self.samples.iter().map(|s| s.heights).sum();
        let settled: u64 = self.samples.iter().map(|s| s.settled).sum();
        let average = settled as f64 / span;
        let held_back = self.samples.iter().any(|s| s.held_back);
        self.settled_per_height = if held_back {
            average
        } else {
            self.settled_per_height.max(average)
        };
    }

    /// Heights until `deploys` more of this queue's deploys settle. `None`
    /// when the observed rate is zero: the shard is settling nothing.
    pub fn heights_to_settle(&self, deploys: usize) -> Option<i64> {
        if deploys == 0 {
            return Some(0);
        }
        if self.settled_per_height <= 0.0 {
            return None;
        }
        Some((deploys as f64 / self.settled_per_height).ceil() as i64)
    }

    /// Deploys that settle in `heights` heights at the observed rate.
    pub fn deploys_settled_in(&self, heights: f64) -> usize {
        (self.settled_per_height * heights).ceil() as usize
    }

    /// Wall-clock seconds for `heights` heights, once a height has been
    /// timed.
    pub fn seconds_for(&self, heights: i64) -> Option<u64> {
        self.secs_per_height.map(|secs| {
            let millis = (secs * heights.max(1) as f64 * 1000.0).round();
            (millis / 1000.0).ceil() as u64
        })
    }

    pub fn settled_per_height(&self) -> f64 { self.settled_per_height }

    pub fn secs_per_height(&self) -> Option<f64> { self.secs_per_height }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn at(start: Instant, secs: u64) -> Instant { start + Duration::from_secs(secs) }

    #[test]
    fn a_fresh_queue_starts_at_one_deploy_per_height() {
        let pace = Pace::new(25);
        assert_eq!(pace.settled_per_height(), 1.0);
        assert_eq!(pace.heights_to_settle(0), Some(0));
        assert_eq!(pace.heights_to_settle(9), Some(9));
        assert_eq!(pace.deploys_settled_in(12.5), 13);
        assert_eq!(pace.seconds_for(3), None);
    }

    #[test]
    fn a_burst_of_verdicts_is_spread_over_the_window() {
        let start = Instant::now();
        let mut pace = Pace::new(10);
        pace.on_tip(0, start);
        // Demand of one deploy a height, settled in one burst of ten.
        for tip in 1..10 {
            pace.record_unsettled();
            pace.on_tip(tip, at(start, 3 * tip as u64));
        }
        pace.record_unsettled();
        pace.record_settled(10);
        pace.on_tip(10, at(start, 30));
        assert!(
            (pace.settled_per_height() - 1.0).abs() < 1e-9,
            "ten verdicts over ten heights is one a height, not ten; got {}",
            pace.settled_per_height()
        );
        assert_eq!(pace.secs_per_height().map(|s| s.round()), Some(3.0));
    }

    #[test]
    fn without_a_hold_back_the_estimate_only_rises() {
        let start = Instant::now();
        let mut pace = Pace::new(10);
        pace.on_tip(0, start);
        for tip in 1..=10 {
            pace.record_unsettled();
            pace.record_settled(3);
            pace.on_tip(tip, at(start, tip as u64));
        }
        assert!((pace.settled_per_height() - 3.0).abs() < 1e-9);

        // Demand falls to one a height: the queue gave the node everything.
        for tip in 11..=40 {
            pace.record_unsettled();
            pace.record_settled(1);
            pace.on_tip(tip, at(start, tip as u64));
        }
        assert!(
            (pace.settled_per_height() - 3.0).abs() < 1e-9,
            "light demand is not lower capacity"
        );
    }

    #[test]
    fn with_a_hold_back_the_estimate_follows_the_window_down() {
        let start = Instant::now();
        let mut pace = Pace::new(10);
        pace.on_tip(0, start);
        for tip in 1..=10 {
            pace.record_unsettled();
            pace.record_settled(4);
            pace.on_tip(tip, at(start, tip as u64));
        }
        assert!((pace.settled_per_height() - 4.0).abs() < 1e-9);

        for tip in 11..=20 {
            pace.record_unsettled();
            pace.record_held_back();
            pace.record_settled(2);
            pace.on_tip(tip, at(start, tip as u64));
        }
        assert!((pace.settled_per_height() - 2.0).abs() < 1e-9);
    }

    #[test]
    fn a_shard_that_settles_nothing_pushes_the_wait_out() {
        let start = Instant::now();
        let mut pace = Pace::new(25);
        pace.on_tip(1, start);
        for tip in 2..60 {
            pace.record_unsettled();
            pace.record_held_back();
            pace.on_tip(tip, at(start, tip as u64));
        }
        assert_eq!(pace.heights_to_settle(1), None);
        assert_eq!(pace.heights_to_settle(0), Some(0), "an empty queue still admits");
        assert_eq!(pace.deploys_settled_in(12.5), 0);
    }
}
