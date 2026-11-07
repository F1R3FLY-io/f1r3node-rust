use std::time::Instant;

/// Weight of the newest sample in each moving average.
const SAMPLE_WEIGHT: f64 = 0.2;

/// How fast the shard moves, as this queue observes it: deploys of its own
/// that reach a block per height, and wall time per height.
///
/// The inclusion rate starts at `max_user_deploys_per_block` (one block of
/// this validator's per height) and is only resampled over heights during
/// which the queue had deploys outstanding — an idle queue says nothing
/// about how fast the shard would include.
#[derive(Debug, Clone)]
pub struct Pace {
    deploys_per_height: f64,
    secs_per_height: Option<f64>,
    last_tip: Option<(i64, Instant)>,
    included_since_tip: u64,
    backlogged_since_tip: bool,
}

impl Pace {
    pub fn new(max_user_deploys_per_block: u32) -> Self {
        Self {
            deploys_per_height: f64::from(max_user_deploys_per_block),
            secs_per_height: None,
            last_tip: None,
            included_since_tip: 0,
            backlogged_since_tip: false,
        }
    }

    pub fn record_inclusions(&mut self, count: u64) { self.included_since_tip += count; }

    /// Marks that deploys were outstanding during the current height.
    pub fn record_backlog(&mut self) { self.backlogged_since_tip = true; }

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
        if self.backlogged_since_tip {
            let rate = self.included_since_tip as f64 / heights;
            self.deploys_per_height = blend(self.deploys_per_height, rate);
        }
        self.included_since_tip = 0;
        self.backlogged_since_tip = false;
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
    fn the_ceiling_paces_until_a_backlogged_height_is_observed() {
        let pace = Pace::new(4);
        assert_eq!(pace.heights_to_include(0), Some(0));
        assert_eq!(pace.heights_to_include(9), Some(3));
        assert_eq!(pace.seconds_for(3), None);
    }

    #[test]
    fn only_backlogged_heights_move_the_inclusion_rate() {
        let start = Instant::now();
        let mut pace = Pace::new(4);
        pace.on_tip(10, start);

        pace.on_tip(11, start + Duration::from_secs(6));
        assert_eq!(pace.deploys_per_height(), 4.0, "an idle height is no sample");
        assert_eq!(pace.secs_per_height(), Some(6.0));

        pace.record_backlog();
        pace.on_tip(12, start + Duration::from_secs(12));
        assert!((pace.deploys_per_height() - 3.2).abs() < 1e-9);

        pace.record_backlog();
        pace.record_inclusions(8);
        pace.on_tip(14, start + Duration::from_secs(24));
        assert!((pace.deploys_per_height() - 3.36).abs() < 1e-9);
        assert_eq!(pace.seconds_for(2), Some(12));
    }

    #[test]
    fn a_node_that_includes_nothing_pushes_the_drain_estimate_out() {
        let start = Instant::now();
        let mut pace = Pace::new(4);
        pace.on_tip(1, start);
        for tip in 2..200 {
            pace.record_backlog();
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
