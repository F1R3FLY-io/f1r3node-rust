/// Share of the citable depth a released deploy may take, from release to
/// its verdict, before the queue counts the shard as congested.
const SETTLE_SHARE_OF_DEPTH: f64 = 0.5;

/// How many deploys the queue may have released and unsettled at once,
/// governed by how long the shard takes to settle them: additive increase,
/// multiplicative decrease, with slow start.
///
/// The signal is each released deploy's settle delay in heights, which does
/// not depend on how much the queue released, so the window cannot talk
/// itself up on a burst of verdicts or down into starving a healthy node.
/// Delay within `limit_heights` grows the window, but only while deploys are
/// held back for want of room: below the slow-start threshold by one per
/// timely verdict (doubling each round), above it by about one per round.
/// A late verdict, or a released deploy still unsettled past
/// `limit_heights`, halves it, at most once per round of `limit_heights`,
/// so one slow burst is one cut. Queues sharing the shard see the same
/// delays and converge on equal shares of it.
#[derive(Debug, Clone)]
pub struct ReleaseWindow {
    window: f64,
    slow_start_threshold: f64,
    ceiling: f64,
    limit_heights: f64,
    last_cut_tip: Option<i64>,
}

impl ReleaseWindow {
    /// A window for a shard of the given citable depth, never above
    /// `ceiling` released deploys. It starts at one deploy per height of
    /// the settle limit.
    pub fn new(citable_depth: i64, ceiling: usize) -> Self {
        let limit_heights = citable_depth.max(1) as f64 * SETTLE_SHARE_OF_DEPTH;
        let ceiling = ceiling.max(1) as f64;
        Self {
            window: limit_heights.ceil().min(ceiling),
            slow_start_threshold: ceiling,
            ceiling,
            limit_heights,
            last_cut_tip: None,
        }
    }

    /// Released deploys allowed at once; at least one, so a shard that has
    /// slowed is still probed.
    pub fn limit(&self) -> usize { (self.window.floor() as usize).max(1) }

    /// Heights a released deploy may take to settle before it counts as
    /// congestion.
    pub fn limit_heights(&self) -> f64 { self.limit_heights }

    /// A released deploy settled `delay` heights after release, observed at
    /// `tip`. `held_back` is whether deploys were waiting for room.
    pub fn on_verdict(&mut self, delay: i64, tip: i64, held_back: bool) {
        if delay as f64 > self.limit_heights {
            self.cut(tip);
        } else if held_back {
            let step = if self.window < self.slow_start_threshold {
                1.0
            } else {
                1.0 / self.window
            };
            self.window = (self.window + step).min(self.ceiling);
        }
    }

    /// A new tip; `oldest_unsettled_age` is how many heights the longest
    /// waiting released deploy has gone without a verdict.
    pub fn on_tip(&mut self, tip: i64, oldest_unsettled_age: Option<i64>) {
        if oldest_unsettled_age.is_some_and(|age| age as f64 > self.limit_heights) {
            self.cut(tip);
        }
    }

    fn cut(&mut self, tip: i64) {
        if self
            .last_cut_tip
            .is_some_and(|at| ((tip - at) as f64) < self.limit_heights)
        {
            return;
        }
        self.slow_start_threshold = (self.window / 2.0).max(1.0);
        self.window = self.slow_start_threshold;
        self.last_cut_tip = Some(tip);
        metrics::counter!("deploy_queue.release_cuts").increment(1);
    }

    pub fn window(&self) -> f64 { self.window }

    pub fn slow_start_threshold(&self) -> f64 { self.slow_start_threshold }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window() -> ReleaseWindow { ReleaseWindow::new(25, 256) }

    #[test]
    fn a_fresh_window_is_one_deploy_per_height_of_the_settle_limit() {
        let w = window();
        assert_eq!(w.limit(), 13);
        assert_eq!(w.limit_heights(), 12.5);
    }

    #[test]
    fn slow_start_doubles_the_window_each_round_of_timely_verdicts() {
        let mut w = window();
        for _ in 0..13 {
            w.on_verdict(4, 20, true);
        }
        assert_eq!(w.limit(), 26);
        for _ in 0..26 {
            w.on_verdict(4, 25, true);
        }
        assert_eq!(w.limit(), 52);
    }

    #[test]
    fn without_deploys_held_back_the_window_does_not_grow() {
        let mut w = window();
        for _ in 0..100 {
            w.on_verdict(4, 20, false);
        }
        assert_eq!(w.limit(), 13);
    }

    #[test]
    fn a_late_verdict_halves_the_window_once_per_round() {
        let mut w = window();
        for _ in 0..51 {
            w.on_verdict(4, 20, true);
        }
        assert_eq!(w.limit(), 64);
        w.on_verdict(13, 40, true);
        assert_eq!(w.limit(), 32);
        w.on_verdict(20, 50, true);
        assert_eq!(w.limit(), 32, "a second late verdict inside the round is the same congestion");
        w.on_verdict(20, 53, true);
        assert_eq!(w.limit(), 16, "a round later it is new congestion");
    }

    #[test]
    fn after_a_cut_growth_is_about_one_per_round() {
        let mut w = window();
        for _ in 0..51 {
            w.on_verdict(4, 20, true);
        }
        w.on_verdict(13, 40, true);
        assert_eq!(w.slow_start_threshold(), 32.0);
        for _ in 0..32 {
            w.on_verdict(4, 60, true);
        }
        assert_eq!(w.limit(), 32, "one window of verdicts adds about one deploy");
        assert!(w.window() > 32.9 && w.window() < 33.1, "{}", w.window());
    }

    #[test]
    fn an_overdue_released_deploy_cuts_without_any_verdict() {
        let mut w = window();
        w.on_tip(30, Some(12));
        assert_eq!(w.limit(), 13);
        w.on_tip(31, Some(13));
        assert_eq!(w.limit(), 6);
        w.on_tip(40, Some(22));
        assert_eq!(w.limit(), 6);
        w.on_tip(44, Some(26));
        assert_eq!(w.limit(), 3);
    }

    #[test]
    fn the_window_stays_between_one_and_the_ceiling() {
        let mut w = ReleaseWindow::new(25, 20);
        for _ in 0..1000 {
            w.on_verdict(1, 20, true);
        }
        assert_eq!(w.limit(), 20);
        for round in 0..20 {
            w.on_tip(100 + round * 13, Some(50));
        }
        assert_eq!(w.limit(), 1);
    }

    /// Two queues share a shard that settles `capacity` deploys a height.
    /// Every released deploy waits behind all released deploys, so both see
    /// the same delay; from very unequal starts they end near equal shares.
    #[test]
    fn queues_sharing_a_shard_converge_on_equal_shares() {
        let capacity = 20.0;
        let mut queues = [ReleaseWindow::new(25, 256), ReleaseWindow::new(25, 256)];
        for _ in 0..200 {
            queues[0].on_verdict(1, 0, true);
        }
        let mut shares = [0.0f64; 2];
        for tip in 1..4000i64 {
            let in_flight: f64 = queues.iter().map(|q| q.limit() as f64).sum();
            let delay = (in_flight / capacity).ceil() as i64;
            for (i, q) in queues.iter_mut().enumerate() {
                let settled = (q.limit() as f64 / delay.max(1) as f64).round() as usize;
                for _ in 0..settled {
                    q.on_verdict(delay, tip, true);
                }
                if tip > 3000 {
                    shares[i] += q.limit() as f64;
                }
            }
        }
        let ratio = shares[0] / shares[1];
        assert!((0.8..=1.25).contains(&ratio), "shares {:?}, ratio {}", shares, ratio);
    }
}
