use std::time::Instant;

/// Weight of the newest sample in each moving average.
const SAMPLE_WEIGHT: f64 = 0.2;

/// How fast the shard moves, as this queue observes it: deploys of its own
/// that reach a block per height, and wall time per height.
///
/// The inclusion rate estimates the node's capacity for this queue's
/// deploys. It starts at one deploy per height, or zero on a shard whose
/// blocks carry no user deploys, so a fresh queue probes the shard rather
/// than assuming it is idle. A height in which the queue held deploys back
/// for want of room measures capacity, and moves the estimate either way. A
/// height in which the queue gave the node everything it had measures only
/// demand: inclusion then can raise the estimate, never lower it. A height
/// with nothing outstanding says nothing.
#[derive(Debug, Clone)]
pub struct Pace {
    deploys_per_height: f64,
    secs_per_height: Option<f64>,
    last_tip: Option<(i64, Instant)>,
    included_since_tip: u64,
    outstanding_since_tip: bool,
    held_back_since_tip: bool,
}

impl Pace {
    pub fn new(max_user_deploys_per_block: u32) -> Self {
        Self {
            deploys_per_height: f64::from(max_user_deploys_per_block.min(1)),
            secs_per_height: None,
            last_tip: None,
            included_since_tip: 0,
            outstanding_since_tip: false,
            held_back_since_tip: false,
        }
    }

    pub fn record_inclusions(&mut self, count: u64) { self.included_since_tip += count; }

    /// Marks that deploys were in the node's pool during the current height.
    pub fn record_outstanding(&mut self) { self.outstanding_since_tip = true; }

    /// Marks that the queue withheld deploys during the current height
    /// because the node's pool held all it may.
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
            Some(average) => blend(average, secs),
            None => secs,
        });
        let rate = self.included_since_tip as f64 / heights;
        if self.held_back_since_tip
            || (self.outstanding_since_tip && rate > self.deploys_per_height)
        {
            self.deploys_per_height = blend(self.deploys_per_height, rate);
        }
        self.included_since_tip = 0;
        self.outstanding_since_tip = false;
        self.held_back_since_tip = false;
        self.last_tip = Some((tip, now));
    }

    /// Heights until `deploys` more of this queue's deploys reach blocks.
    /// `None` when the observed rate is zero: the node is including nothing.
    pub fn heights_to_include(&self, deploys: usize) -> Option<i64> {
        if deploys == 0 {
            return Some(0);
        }
        if self.deploys_per_height <= 0.0 {
            return None;
        }
        Some((deploys as f64 / self.deploys_per_height).ceil() as i64)
    }

    /// Wall-clock seconds for `heights` heights, once a height has been
    /// timed.
    pub fn seconds_for(&self, heights: i64) -> Option<u64> {
        self.secs_per_height.map(|secs| {
            let millis = (secs * heights.max(1) as f64 * 1000.0).round();
            (millis / 1000.0).ceil() as u64
        })
    }

    pub fn deploys_per_height(&self) -> f64 { self.deploys_per_height }

    pub fn secs_per_height(&self) -> Option<f64> { self.secs_per_height }
}

fn blend(average: f64, sample: f64) -> f64 {
    average * (1.0 - SAMPLE_WEIGHT) + sample * SAMPLE_WEIGHT
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn a_fresh_queue_starts_at_one_deploy_per_height() {
        let pace = Pace::new(128);
        assert_eq!(pace.deploys_per_height(), 1.0);
        assert_eq!(pace.heights_to_include(0), Some(0));
        assert_eq!(pace.heights_to_include(9), Some(9));
        assert_eq!(pace.seconds_for(3), None);
    }

    #[test]
    fn held_back_heights_move_the_estimate_either_way() {
        let start = Instant::now();
        let mut pace = Pace::new(128);
        pace.on_tip(10, start);

        pace.on_tip(11, start + Duration::from_secs(6));
        assert_eq!(pace.deploys_per_height(), 1.0, "an idle height is no sample");
        assert_eq!(pace.secs_per_height(), Some(6.0));

        pace.record_outstanding();
        pace.record_held_back();
        pace.record_inclusions(3);
        pace.on_tip(12, start + Duration::from_secs(12));
        assert!((pace.deploys_per_height() - 1.4).abs() < 1e-9);

        pace.record_outstanding();
        pace.record_held_back();
        pace.on_tip(14, start + Duration::from_secs(24));
        assert!((pace.deploys_per_height() - 1.12).abs() < 1e-9);
        assert_eq!(pace.seconds_for(2), Some(12));
    }

    #[test]
    fn light_demand_never_lowers_the_estimate() {
        let start = Instant::now();
        let mut pace = Pace::new(128);
        pace.on_tip(1, start);
        pace.record_outstanding();
        pace.record_held_back();
        pace.record_inclusions(5);
        pace.on_tip(2, start + Duration::from_secs(3));
        assert!((pace.deploys_per_height() - 1.8).abs() < 1e-9);

        // The queue gave the node all it had: one deploy a height.
        for tip in 3..50 {
            pace.record_outstanding();
            pace.record_inclusions(1);
            pace.on_tip(tip, start + Duration::from_secs(3 * tip as u64));
        }
        assert!((pace.deploys_per_height() - 1.8).abs() < 1e-9);

        pace.record_outstanding();
        pace.record_inclusions(6);
        pace.on_tip(50, start + Duration::from_secs(150));
        assert!(
            pace.deploys_per_height() > 1.8,
            "inclusion above the estimate is capacity even without a backlog"
        );
    }

    #[test]
    fn a_node_that_includes_nothing_pushes_the_drain_estimate_out() {
        let start = Instant::now();
        let mut pace = Pace::new(128);
        pace.on_tip(1, start);
        for tip in 2..200 {
            pace.record_outstanding();
            pace.record_held_back();
            pace.on_tip(tip, start + Duration::from_secs(tip as u64));
        }
        assert!(pace.heights_to_include(1).unwrap() > 1_000_000);
        assert_eq!(
            pace.heights_to_include(0),
            Some(0),
            "an empty queue still admits"
        );
    }

    #[test]
    fn a_shard_with_no_user_deploy_room_has_no_estimate() {
        assert_eq!(Pace::new(0).heights_to_include(1), None);
    }
}
