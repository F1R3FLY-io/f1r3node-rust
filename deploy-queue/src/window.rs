/// Share of the citable depth a released deploy may take, from release to
/// its verdict, before it counts as late. It is also the length of a round.
const SETTLE_SHARE_OF_DEPTH: f64 = 0.5;

/// Why the window was halved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cut {
    /// Most of a round's verdicts were late.
    OverCapacity,
    /// A round in which a full window settled nothing.
    Stall,
}

impl Cut {
    pub fn name(self) -> &'static str {
        match self {
            Cut::OverCapacity => "over_capacity",
            Cut::Stall => "stall",
        }
    }
}

#[derive(Debug, Clone, Default)]
struct Round {
    start: i64,
    on_time: usize,
    late: usize,
    held_back: bool,
    settle_delays: Vec<i64>,
    inclusion_delays: Vec<i64>,
}

/// How many deploys the queue may have released and unsettled at once.
///
/// The shard is judged once per round, on what most released deploys saw:
/// a round whose verdicts were mostly later than the settle limit means the
/// typical deploy cannot settle in time, so the window halves; a round in
/// which a full window settled nothing (deploys held back, no verdict, a
/// deploy released before the round still waiting) is a stall and halves it
/// too. One deploy that keeps losing merges is one late verdict in
/// its round, never a cut by itself. Otherwise, if deploys were held back
/// for want of room and some settled in time, the window grows by one. It starts at one deploy per
/// height of a round, already near what a validator settles, so growth is
/// additive from the start: doubling would overshoot the shard by the time
/// a round's late verdicts could show it. Queues sharing the shard see the
/// same rounds and converge on equal shares of it.
#[derive(Debug, Clone)]
pub struct ReleaseWindow {
    window: f64,
    cuts: u64,
    ceiling: f64,
    round_heights: f64,
    round: Option<Round>,
    late_share: Option<f64>,
    settle_delay: Option<i64>,
    inclusion_delay: Option<i64>,
}

impl ReleaseWindow {
    /// A window for a shard of the given citable depth, never above
    /// `ceiling` released deploys, starting at one deploy per height of a
    /// round.
    pub fn new(citable_depth: i64, ceiling: usize) -> Self {
        let round_heights = citable_depth.max(1) as f64 * SETTLE_SHARE_OF_DEPTH;
        let ceiling = ceiling.max(1) as f64;
        Self {
            window: round_heights.ceil().min(ceiling),
            cuts: 0,
            ceiling,
            round_heights,
            round: None,
            late_share: None,
            settle_delay: None,
            inclusion_delay: None,
        }
    }

    /// Released deploys allowed at once; at least one, so a shard that has
    /// slowed is still probed.
    pub fn limit(&self) -> usize { (self.window.floor() as usize).max(1) }

    /// A released deploy settled `delay` heights after release.
    pub fn on_verdict(&mut self, delay: i64) {
        let late = delay as f64 > self.round_heights;
        if let Some(round) = self.round.as_mut() {
            if late {
                round.late += 1;
            } else {
                round.on_time += 1;
            }
            round.settle_delays.push(delay);
        }
    }

    /// A released deploy reached its first block `delay` heights after
    /// release.
    pub fn on_inclusion(&mut self, delay: i64) {
        if let Some(round) = self.round.as_mut() {
            round.inclusion_delays.push(delay);
        }
    }

    /// Deploys were waiting for room in the window.
    pub fn record_held_back(&mut self) {
        if let Some(round) = self.round.as_mut() {
            round.held_back = true;
        }
    }

    /// A new tip. `oldest_unsettled_release` is the release height of the
    /// longest-waiting released deploy. Ends the round, and judges it, once
    /// the tip reaches a round past its start.
    pub fn on_tip(&mut self, tip: i64, oldest_unsettled_release: Option<i64>) -> Option<Cut> {
        let Some(round) = self.round.as_ref() else {
            self.round = Some(Round {
                start: tip,
                ..Round::default()
            });
            return None;
        };
        if ((tip - round.start) as f64) < self.round_heights {
            return None;
        }
        let round = self.round.replace(Round {
            start: tip,
            ..Round::default()
        });
        let round = round.expect("a round was in progress");
        self.judge(round, oldest_unsettled_release)
    }

    fn judge(&mut self, mut round: Round, oldest_unsettled_release: Option<i64>) -> Option<Cut> {
        let verdicts = round.on_time + round.late;
        if verdicts > 0 {
            self.late_share = Some(round.late as f64 / verdicts as f64);
            self.settle_delay = median(&mut round.settle_delays);
        }
        if let Some(delay) = median(&mut round.inclusion_delays) {
            self.inclusion_delay = Some(delay);
        }
        let cut = if verdicts > 0 && round.late * 2 > verdicts {
            Some(Cut::OverCapacity)
        } else if verdicts == 0
            && round.held_back
            && oldest_unsettled_release.is_some_and(|at| at < round.start)
        {
            Some(Cut::Stall)
        } else {
            None
        };
        match cut {
            Some(_) => {
                self.window = (self.window / 2.0).max(1.0);
                self.cuts += 1;
            }
            None if round.held_back && round.on_time > 0 => {
                self.window = (self.window + 1.0).min(self.ceiling);
            }
            None => {}
        }
        cut
    }

    /// Deploys a height the window lets settle: the window over the median
    /// settle delay of the latest round that had verdicts.
    pub fn release_rate(&self) -> Option<f64> {
        self.settle_delay
            .map(|delay| self.window / delay.max(1) as f64)
    }

    /// Heights of window a deploy needs left to be worth releasing: the
    /// median inclusion delay of the latest round that saw inclusions, plus
    /// the block that carries it.
    pub fn release_headroom(&self) -> i64 { self.inclusion_delay.map_or(1, |delay| delay + 1) }

    pub fn window(&self) -> f64 { self.window }

    /// Times the window has been halved.
    pub fn cuts(&self) -> u64 { self.cuts }

    pub fn late_share(&self) -> Option<f64> { self.late_share }

    pub fn settle_delay(&self) -> Option<i64> { self.settle_delay }

    pub fn inclusion_delay(&self) -> Option<i64> { self.inclusion_delay }
}

fn median(values: &mut [i64]) -> Option<i64> {
    if values.is_empty() {
        return None;
    }
    values.sort_unstable();
    Some(values[values.len() / 2])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A window of depth 25 (rounds of 12.5 heights) whose first round
    /// starts at tip 0.
    fn started() -> ReleaseWindow {
        let mut w = ReleaseWindow::new(25, 256);
        w.on_tip(0, None);
        w
    }

    fn round(w: &mut ReleaseWindow, end_tip: i64, on_time: usize, late: usize, held_back: bool) -> Option<Cut> {
        for _ in 0..on_time {
            w.on_verdict(3);
        }
        for _ in 0..late {
            w.on_verdict(40);
        }
        if held_back {
            w.record_held_back();
        }
        w.on_tip(end_tip, None)
    }

    #[test]
    fn a_few_late_verdicts_do_not_cut_but_mostly_late_ones_do() {
        let mut w = started();
        assert_eq!(round(&mut w, 13, 12, 1, false), None, "one deploy that keeps losing is not congestion");
        assert_eq!(w.limit(), 13);
        assert_eq!(round(&mut w, 26, 4, 9, true), Some(Cut::OverCapacity));
        assert_eq!(w.limit(), 6);
        assert_eq!(w.late_share(), Some(9.0 / 13.0));
    }

    #[test]
    fn a_full_window_that_settles_nothing_for_a_round_is_a_stall() {
        let mut w = started();
        w.record_held_back();
        assert_eq!(w.on_tip(13, Some(0)), None, "released at the round's start is not older than it");
        w.record_held_back();
        assert_eq!(w.on_tip(26, Some(0)), Some(Cut::Stall));
        assert_eq!(w.limit(), 6, "a round without timely verdicts does not grow the window");
        assert_eq!(
            w.on_tip(39, Some(0)),
            None,
            "one old deploy waiting on an otherwise idle queue is not a stall"
        );
    }

    #[test]
    fn growth_is_one_deploy_per_held_back_round() {
        let mut w = started();
        round(&mut w, 13, 13, 0, true);
        assert_eq!(w.limit(), 14, "a round of timely verdicts adds one, however many");
        round(&mut w, 26, 14, 0, false);
        assert_eq!(w.limit(), 14, "without deploys held back demand, not capacity, is the limit");
        round(&mut w, 39, 14, 0, true);
        assert_eq!(w.limit(), 15);
        round(&mut w, 52, 5, 10, true);
        assert_eq!((w.limit(), w.cuts()), (7, 1));
        round(&mut w, 65, 7, 0, true);
        assert_eq!(w.limit(), 8);
        assert_eq!(w.release_rate(), Some(8.5 / 3.0));
    }

    /// Two queues share a shard that settles `capacity` deploys a height.
    /// Every released deploy waits behind all released deploys, so both see
    /// the same delays; from very unequal starts they end near equal shares.
    #[test]
    fn queues_sharing_a_shard_converge_on_equal_shares() {
        let capacity = 20.0;
        let mut queues = [ReleaseWindow::new(25, 256), ReleaseWindow::new(25, 256)];
        for q in queues.iter_mut() {
            q.on_tip(0, None);
        }
        queues[0].window = 200.0;
        let mut shares = [0.0f64; 2];
        for tip in 1..6000i64 {
            let in_flight: f64 = queues.iter().map(|q| q.limit() as f64).sum();
            let delay = (in_flight / capacity).ceil().max(1.0) as i64;
            for (i, q) in queues.iter_mut().enumerate() {
                let settled = (q.limit() as f64 / delay as f64).round() as usize;
                for _ in 0..settled {
                    q.on_verdict(delay);
                }
                q.record_held_back();
                q.on_tip(tip, None);
                if tip > 4000 {
                    shares[i] += q.limit() as f64;
                }
            }
        }
        let ratio = shares[0] / shares[1];
        assert!((0.8..=1.25).contains(&ratio), "shares {:?}, ratio {}", shares, ratio);
    }
}
