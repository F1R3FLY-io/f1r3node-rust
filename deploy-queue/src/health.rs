use std::collections::VecDeque;
use std::time::Instant;

use serde::Serialize;

use crate::error::Congestion;
use crate::events::Admission;

/// Weight of the newest sample in the wall-time-per-height average.
const SAMPLE_WEIGHT: f64 = 0.2;
/// Intake assumed before the validator has created a block the queue saw.
const UNMEASURED_INTAKE_PER_HEIGHT: f64 = 1.0;

/// One of the validator's own blocks, as its block-created event described it.
#[derive(Debug, Clone, Copy)]
struct OwnBlock {
    number: i64,
    /// Fresh deploys the block could take.
    capacity: usize,
}

/// What the queue knows about its validator and about the shard, from the
/// node's event stream, over the last `span` heights (the citable depth).
///
/// The validator is the only block producer that includes this queue's
/// deploys: deploys never gossip. Its own blocks say how many fresh deploys
/// each could take and whether that cap bound. A block that left deploys in
/// the pool at its cap took exactly its cap; one that took all it was offered
/// was limited by nothing but demand, so it could have taken up to the shard's
/// per-block ceiling.
///
/// The shard is every block the node adds, from any creator: how fast heights
/// come, and, through the validator's admission, whether the node is holding
/// deploys back because finality trails the tip.
#[derive(Debug, Clone)]
pub struct Health {
    span: i64,
    ceiling: usize,
    own_creator: Option<String>,
    /// Heights of recent blocks from any creator, with whether the validator
    /// created them.
    blocks: VecDeque<(i64, bool)>,
    own_blocks: VecDeque<OwnBlock>,
    latest_admission: Option<Admission>,
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
    /// The cap of its latest block and what set it.
    pub latest_cap: Option<usize>,
    pub latest_cap_source: Option<String>,
    /// Its latest block left fresh deploys in the pool.
    pub saturated: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ShardView {
    pub secs_per_height: Option<f64>,
    /// The node lowered its validator's cap because finality trails the tip
    /// or has stalled.
    pub backpressure: bool,
    /// Heights between the tip and the last finalized block when the
    /// validator last created a block.
    pub finality_lag: Option<i64>,
}

impl Health {
    /// Health over `span` heights for a shard whose blocks take at most
    /// `ceiling` user deploys.
    pub fn new(span: i64, ceiling: usize) -> Self {
        Self {
            span: span.max(1),
            ceiling: ceiling.max(1),
            own_creator: None,
            blocks: VecDeque::new(),
            own_blocks: VecDeque::new(),
            latest_admission: None,
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

    /// A block the validator created, before the node validated it.
    pub fn on_own_block(
        &mut self,
        number: i64,
        creator: &str,
        admission: Option<Admission>,
        now: Instant,
    ) {
        if self.own_creator.as_deref() != Some(creator) {
            self.own_creator = Some(creator.to_string());
        }
        if let Some(admission) = admission {
            let capacity = if admission.cap_hit || admission.byte_cap_hit {
                admission.cap
            } else {
                self.ceiling
            };
            self.own_blocks.push_back(OwnBlock { number, capacity });
            self.latest_admission = Some(admission);
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

    /// The validator took no deploys into a block over a full span while the
    /// shard advanced through it.
    pub fn validator_idle(&self) -> bool {
        let watched_a_span = match (self.first_tip, self.tip) {
            (Some(first), Some((tip, _))) => tip - first >= self.span,
            _ => false,
        };
        self.own_blocks.is_empty() && watched_a_span
    }

    /// Fresh deploys the validator takes per height: what its own blocks
    /// over the span could take, over the heights watched.
    pub fn intake_per_height(&self) -> f64 {
        if self.validator_idle() {
            return 0.0;
        }
        match (self.own_blocks.is_empty(), self.observed()) {
            (false, Some(observed)) => {
                self.own_blocks.iter().map(|b| b.capacity).sum::<usize>() as f64 / observed as f64
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

    /// Released deploys to keep waiting in the validator's pool: its cap
    /// while that cap binds, otherwise a full block.
    pub fn pool_target(&self) -> usize {
        match &self.latest_admission {
            Some(admission) if admission.cap_hit || admission.byte_cap_hit => admission.cap,
            _ => self.ceiling,
        }
    }

    /// Why deploys wait: the shard when the node is holding its validator
    /// back for finality, the validator otherwise.
    pub fn congestion(&self) -> Congestion {
        match &self.latest_admission {
            Some(admission) if admission.backpressure && !self.validator_idle() => {
                Congestion::Shard
            }
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
            latest_cap: self.latest_admission.as_ref().map(|a| a.cap),
            latest_cap_source: self.latest_admission.as_ref().map(|a| a.cap_source.clone()),
            saturated: self
                .latest_admission
                .as_ref()
                .is_some_and(|a| a.cap_hit || a.byte_cap_hit),
        }
    }

    pub fn shard_view(&self) -> ShardView {
        ShardView {
            secs_per_height: self.secs_per_height,
            backpressure: self
                .latest_admission
                .as_ref()
                .is_some_and(|a| a.backpressure),
            finality_lag: self.latest_admission.as_ref().map(|a| a.finality_lag),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn admission(cap: usize, cap_hit: bool, backpressure: bool) -> Option<Admission> {
        Some(Admission {
            cap,
            cap_hit,
            byte_cap_hit: false,
            cap_source: if backpressure { "hard" } else { "normal" }.to_string(),
            backpressure,
            finality_lag: if backpressure { 9 } else { 2 },
        })
    }

    #[test]
    fn a_validator_that_takes_all_it_is_offered_is_offered_a_full_block() {
        let start = Instant::now();
        let mut health = Health::new(10, 128);
        health.on_own_block(1, "v1", admission(4, false, false), start);
        assert_eq!(health.pool_target(), 128);

        health.on_own_block(2, "v1", admission(16, true, false), start);
        assert_eq!(
            health.pool_target(),
            16,
            "a binding cap holds the pool at the cap"
        );
    }

    #[test]
    fn intake_counts_what_the_validators_blocks_could_take_per_height() {
        let start = Instant::now();
        let mut health = Health::new(10, 128);
        // Four validators take turns; this one creates every fourth block,
        // each saturated at 8.
        for number in 1..=20 {
            let at = start + Duration::from_secs(3 * number as u64);
            if number % 4 == 0 {
                health.on_own_block(number, "v1", admission(8, true, false), at);
            }
            health.on_block(number, if number % 4 == 0 { "v1" } else { "peer" }, at);
        }
        assert_eq!(health.intake_per_height(), 8.0 * 3.0 / 10.0);
        assert_eq!(health.heights_per_own_block(), 4);
        assert_eq!(health.validator_view().block_share, Some(0.3));
        assert_eq!(health.secs_per_height().map(f64::round), Some(3.0));
    }

    #[test]
    fn a_validator_that_stops_proposing_while_the_shard_moves_takes_nothing() {
        let start = Instant::now();
        let mut health = Health::new(10, 128);
        health.on_own_block(1, "v1", admission(8, true, false), start);
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
        let mut health = Health::new(10, 128);
        health.on_own_block(1, "v1", admission(4, true, true), start);
        assert_eq!(health.congestion(), Congestion::Shard);
        assert!(health.shard_view().backpressure);

        health.on_own_block(2, "v1", admission(32, true, false), start);
        assert_eq!(
            health.congestion(),
            Congestion::Validator,
            "a binding cap without backpressure is the validator's own limit"
        );
    }
}
