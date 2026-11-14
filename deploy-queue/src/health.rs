use std::collections::VecDeque;
use std::time::Instant;

use serde::Serialize;

use crate::error::Congestion;
use crate::events::Admission;

/// Weight of the newest sample in the wall-time-per-height average.
const SAMPLE_WEIGHT: f64 = 0.2;
/// Intake assumed before the validator has built a block from this queue's
/// deploys.
const UNMEASURED_INTAKE_PER_HEIGHT: f64 = 1.0;
/// The sample at `len / LOWER_QUARTILE` of the sorted samples is their lower
/// quartile.
const LOWER_QUARTILE: usize = 4;

/// One of the validator's own blocks. `could_take` is set only for a block
/// built while this queue had deploys in the validator's pool: one built from
/// an empty pool says nothing about what the validator takes.
#[derive(Debug, Clone, Copy)]
struct OwnBlock {
    number: i64,
    could_take: Option<usize>,
}

/// The validator's latest block built while this queue had deploys in its
/// pool.
#[derive(Debug, Clone)]
struct Measured {
    admission: Admission,
    /// This queue's deploys stayed in the pool after the block.
    saturated: bool,
}

/// What the queue knows about its validator and about the shard, from the
/// node's event stream, over the last `span` heights (the citable depth).
///
/// The validator is the only block producer that includes this queue's
/// deploys: deploys never gossip. Its block creator reports the cap it
/// applied to each block, so the queue keeps that many deploys in the pool
/// and no more: a deploy left in the pool has no deadline order and no
/// deployer share. A block that left this queue's deploys behind took what
/// it could; one that took all it was offered was limited by demand, so its
/// cap, not what it took, is what it could have taken. The validator's
/// intake is the average of what its blocks could take, times how often it
/// creates one.
///
/// The shard is every block the node adds, from any creator: how fast heights
/// come, and, through the validator's admission, whether the node is holding
/// deploys back because finality trails the tip.
#[derive(Debug, Clone)]
pub struct Health {
    span: i64,
    own_creator: Option<String>,
    /// Heights of recent blocks from any creator, with whether the validator
    /// created them.
    blocks: VecDeque<(i64, bool)>,
    own_blocks: VecDeque<OwnBlock>,
    measured: Option<Measured>,
    first_tip: Option<i64>,
    tip: Option<(i64, Instant)>,
    secs_per_height: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ValidatorView {
    /// Blocks the validator created over the span.
    pub own_blocks: usize,
    /// The validator's share of the blocks the node added over the span.
    pub block_share: Option<f64>,
    /// Fresh deploys the validator takes per height.
    pub intake_per_height: f64,
    /// Released deploys the queue keeps waiting in the validator's pool.
    pub pool_target: usize,
    /// The cap of its latest block built from this queue's deploys, and what
    /// set it.
    pub latest_cap: Option<usize>,
    pub latest_cap_source: Option<String>,
    /// That block left this queue's deploys in the pool.
    pub saturated: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ShardView {
    pub secs_per_height: Option<f64>,
    /// The node lowered its validator's cap because finality trails the tip
    /// or has stalled.
    pub backpressure: bool,
    /// Heights between the tip and the last finalized block when the
    /// validator last built a block from this queue's deploys.
    pub finality_lag: Option<i64>,
}

impl Health {
    /// Health over `span` heights.
    pub fn new(span: i64) -> Self {
        Self {
            span: span.max(1),
            own_creator: None,
            blocks: VecDeque::new(),
            own_blocks: VecDeque::new(),
            measured: None,
            first_tip: None,
            tip: None,
            secs_per_height: None,
        }
    }

    /// A block the node added, from any creator.
    pub fn on_block(&mut self, number: i64, creator: &str, now: Instant) {
        let own = self.own_creator.as_deref() == Some(creator);
        self.blocks.push_back((number, own));
        self.on_tip(number, now);
    }

    /// A block the validator created, before the node validated it, carrying
    /// `took` of this queue's deploys and leaving behind `left` that were in
    /// the pool before it was built.
    pub fn on_own_block(
        &mut self,
        number: i64,
        creator: &str,
        admission: Option<Admission>,
        took: usize,
        left: usize,
        now: Instant,
    ) {
        if self.own_creator.as_deref() != Some(creator) {
            self.own_creator = Some(creator.to_string());
        }
        let measured = match admission {
            Some(admission) if took + left > 0 => {
                let saturated = left > 0 || took >= admission.cap;
                Some(Measured {
                    admission,
                    saturated,
                })
            }
            _ => None,
        };
        self.own_blocks.push_back(OwnBlock {
            number,
            could_take: measured.as_ref().map(|m| {
                if m.saturated {
                    took
                } else {
                    m.admission.cap.max(took)
                }
            }),
        });
        if measured.is_some() {
            self.measured = measured;
        }
        self.on_tip(number, now);
    }

    fn on_tip(&mut self, number: i64, now: Instant) {
        self.first_tip.get_or_insert(number);
        match self.tip {
            Some((tip, at)) if number > tip => {
                let secs = now.duration_since(at).as_secs_f64() / (number - tip) as f64;
                self.secs_per_height = Some(match self.secs_per_height {
                    Some(average) => average * (1.0 - SAMPLE_WEIGHT) + secs * SAMPLE_WEIGHT,
                    None => secs,
                });
                self.tip = Some((number, now));
            }
            Some(_) => {}
            None => self.tip = Some((number, now)),
        }
        let oldest = self.tip.map_or(number, |(tip, _)| tip) - self.span;
        while self.blocks.front().is_some_and(|(n, _)| *n <= oldest) {
            self.blocks.pop_front();
        }
        while self.own_blocks.front().is_some_and(|b| b.number <= oldest) {
            self.own_blocks.pop_front();
        }
    }

    /// Heights the queue has watched, up to the span.
    fn observed(&self) -> Option<i64> {
        let (tip, _) = self.tip?;
        Some((tip - self.first_tip?).clamp(1, self.span))
    }

    /// The validator created no block over a full span while the shard
    /// advanced through it.
    pub fn validator_idle(&self) -> bool {
        let watched_a_span = match (self.first_tip, self.tip) {
            (Some(first), Some((tip, _))) => tip - first >= self.span,
            _ => false,
        };
        self.own_blocks.is_empty() && watched_a_span
    }

    /// Fresh deploys the validator takes per height: the lower quartile of
    /// what its blocks built from this queue's deploys could take, times how
    /// often it creates a block. A deploy admitted now waits for many blocks,
    /// and the node swings its cap with finality, so admission counts on the
    /// lower caps it has seen rather than their average.
    pub fn intake_per_height(&self) -> f64 {
        if self.validator_idle() {
            return 0.0;
        }
        let mut samples: Vec<usize> = self
            .own_blocks
            .iter()
            .filter_map(|b| b.could_take)
            .collect();
        samples.sort_unstable();
        match (samples.get(samples.len() / LOWER_QUARTILE), self.observed()) {
            (Some(per_block), Some(observed)) => {
                *per_block as f64 * self.own_blocks.len() as f64 / observed as f64
            }
            _ => UNMEASURED_INTAKE_PER_HEIGHT,
        }
    }

    /// Heights between the validator's blocks, at least one.
    pub fn heights_per_own_block(&self) -> i64 {
        match (self.own_blocks.len(), self.observed()) {
            (0, _) | (_, None) => 1,
            (count, Some(observed)) => (observed as f64 / count as f64).ceil().max(1.0) as i64,
        }
    }

    /// Released deploys to keep waiting in the validator's pool: the cap its
    /// block creator last applied to this queue's deploys, at least one so
    /// the validator always has work to report a cap for.
    pub fn pool_target(&self) -> usize {
        self.measured.as_ref().map_or(1, |m| m.admission.cap).max(1)
    }

    /// Why deploys wait: the shard when the node is holding its validator
    /// back for finality, the validator otherwise.
    pub fn congestion(&self) -> Congestion {
        match &self.measured {
            Some(m) if m.admission.backpressure && !self.validator_idle() => Congestion::Shard,
            _ => Congestion::Validator,
        }
    }

    pub fn secs_per_height(&self) -> Option<f64> { self.secs_per_height }

    /// Wall-clock seconds for `heights` heights, once a height has been timed.
    pub fn seconds_for(&self, heights: i64) -> Option<u64> {
        self.secs_per_height.map(|secs| {
            let millis = (secs * heights.max(1) as f64 * 1000.0).round();
            (millis / 1000.0).ceil() as u64
        })
    }

    pub fn validator_view(&self) -> ValidatorView {
        let own = self.blocks.iter().filter(|(_, own)| *own).count();
        ValidatorView {
            own_blocks: self.own_blocks.len(),
            block_share: (!self.blocks.is_empty()).then(|| own as f64 / self.blocks.len() as f64),
            intake_per_height: self.intake_per_height(),
            pool_target: self.pool_target(),
            latest_cap: self.measured.as_ref().map(|m| m.admission.cap),
            latest_cap_source: self
                .measured
                .as_ref()
                .map(|m| m.admission.cap_source.clone()),
            saturated: self.measured.as_ref().is_some_and(|m| m.saturated),
        }
    }

    pub fn shard_view(&self) -> ShardView {
        ShardView {
            secs_per_height: self.secs_per_height,
            backpressure: self
                .measured
                .as_ref()
                .is_some_and(|m| m.admission.backpressure),
            finality_lag: self.measured.as_ref().map(|m| m.admission.finality_lag),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn admission(cap: usize, backpressure: bool) -> Option<Admission> {
        Some(Admission {
            cap,
            cap_hit: false,
            byte_cap_hit: false,
            cap_source: if backpressure { "hard" } else { "normal" }.to_string(),
            backpressure,
            finality_lag: if backpressure { 9 } else { 2 },
        })
    }

    #[test]
    fn the_pool_holds_the_cap_the_block_creator_applies() {
        let start = Instant::now();
        let mut health = Health::new(10);
        assert_eq!(health.pool_target(), 1, "one deploy until a cap is known");

        // A cap that took all it was offered still bounds the pool: the node
        // can leave deploys behind without reporting its cap as hit.
        health.on_own_block(1, "v1", admission(16, false), 3, 0, start);
        assert_eq!(health.pool_target(), 16);
        assert!(!health.validator_view().saturated);

        health.on_own_block(2, "v1", admission(4, true), 4, 16, start);
        assert_eq!(health.pool_target(), 4);
        assert!(health.validator_view().saturated);
    }

    #[test]
    fn a_block_that_took_its_whole_cap_took_what_it_could() {
        let start = Instant::now();
        let mut health = Health::new(10);
        for number in 1..=10 {
            health.on_own_block(number, "v1", admission(8, false), 8, 0, start);
        }
        assert!(health.validator_view().saturated);
        assert_eq!(health.intake_per_height(), 8.0 * 10.0 / 9.0);
    }

    #[test]
    fn a_block_built_from_an_empty_pool_measures_nothing() {
        let start = Instant::now();
        let mut health = Health::new(10);
        health.on_own_block(1, "v1", admission(8, false), 2, 3, start);
        // A non-leader with nothing of this queue's in its pool reports a
        // cap of zero; that is no limit on what it would take.
        health.on_own_block(2, "v1", admission(0, false), 0, 0, start);
        assert_eq!(health.pool_target(), 8);
        assert_eq!(health.validator_view().latest_cap, Some(8));
    }

    #[test]
    fn intake_is_what_the_validators_blocks_took_per_height() {
        let start = Instant::now();
        let mut health = Health::new(10);
        // Four validators take turns; this one creates every fourth block,
        // each left deploys behind after taking 8.
        for number in 1..=20 {
            let at = start + Duration::from_secs(3 * number as u64);
            if number % 4 == 0 {
                health.on_own_block(number, "v1", admission(8, false), 8, 22, at);
            }
            health.on_block(number, if number % 4 == 0 { "v1" } else { "peer" }, at);
        }
        assert_eq!(health.intake_per_height(), 8.0 * 3.0 / 10.0);
        assert_eq!(health.heights_per_own_block(), 4);
        assert_eq!(health.validator_view().block_share, Some(0.3));
        assert_eq!(health.secs_per_height().map(f64::round), Some(3.0));
    }

    #[test]
    fn a_block_that_took_all_it_was_offered_could_have_taken_its_cap() {
        let start = Instant::now();
        let mut health = Health::new(10);
        // Light demand is not low capacity: each block took the two it was
        // offered under a cap of 16.
        for number in 1..=10 {
            health.on_own_block(number, "v1", admission(16, false), 2, 0, start);
        }
        assert_eq!(health.intake_per_height(), 16.0 * 10.0 / 9.0);

        // Once blocks leave deploys behind, they count what they took.
        for number in 11..=20 {
            health.on_own_block(number, "v1", admission(16, false), 4, 26, start);
        }
        assert_eq!(health.intake_per_height(), 4.0 * 10.0 / 10.0);
    }

    #[test]
    fn intake_counts_on_the_lower_caps_the_node_swings_to() {
        let start = Instant::now();
        let mut health = Health::new(20);
        // Backpressure swings the cap between 16 and 4: a third of the
        // blocks take 4. Their average, 12, over-promises a long wait.
        for number in 1..=21 {
            let cap = if number % 3 == 0 { 4 } else { 16 };
            health.on_own_block(number, "v1", admission(cap, false), cap, 10, start);
        }
        assert_eq!(health.intake_per_height(), 4.0 * 20.0 / 20.0);
    }

    #[test]
    fn a_validator_that_stops_proposing_while_the_shard_moves_takes_nothing() {
        let start = Instant::now();
        let mut health = Health::new(10);
        health.on_own_block(1, "v1", admission(8, false), 4, 0, start);
        for number in 1..=12 {
            health.on_block(number, "peer", start);
        }
        assert!(health.validator_idle());
        assert_eq!(health.intake_per_height(), 0.0);
        assert_eq!(health.congestion(), Congestion::Validator);
    }

    #[test]
    fn backpressure_from_the_node_names_the_shard() {
        let start = Instant::now();
        let mut health = Health::new(10);
        health.on_own_block(1, "v1", admission(4, true), 4, 5, start);
        assert_eq!(health.congestion(), Congestion::Shard);
        assert!(health.shard_view().backpressure);

        health.on_own_block(2, "v1", admission(32, false), 32, 8, start);
        assert_eq!(
            health.congestion(),
            Congestion::Validator,
            "a cap that binds without backpressure is the validator's own limit"
        );
    }
}
