//! The parameters fixed at genesis that consensus rules read, taken from the
//! chain rather than from local configuration: two nodes running different
//! values would judge the same block differently.

use std::num::NonZeroU32;

use num_bigint::BigInt;
use shared::rust::token_amount::TokenAmount;

use crate::rust::economics::bucket_view::LeakyBucketParams;
use crate::rust::economics::reward::{RewardParams, PPM};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChainParameters {
    /// Parts per million in `[-1_000_000, 1_000_000]`; the finalized-floor
    /// oracle decides on this, never on a float.
    pub fault_tolerance_threshold_ppm: i64,
    /// The parent-spread validity bound; `i32::MAX` disables the check.
    pub max_parent_depth: i32,
    pub deploy_lifespan: i64,
    pub min_phlo_price: TokenAmount,
    pub epoch_length: i32,
    pub quarantine_length: i32,
    pub leaky_bucket: LeakyBucketParams,
}

impl ChainParameters {
    /// The parameters `value` answers by name, each checked against the range
    /// a node can run with. Genesis checks what it writes with this, and a
    /// node what it reads back, so genesis cannot write a value the node
    /// refuses.
    ///
    /// * a fault-tolerance ppm outside `[-1e6, 1e6]` breaks the hypothesis of
    ///   `FtExact.ft_decides_exact` — below it the oracle finalizes on any
    ///   stake, above it nothing ever finalizes — and the decision's
    ///   `debug_assert!` compiles out in release;
    /// * the parent-spread rule takes the depth as `i32`, and the narrowest
    ///   consumer of the lifespan, the expiry threshold, is `i32` too; a
    ///   window of zero inverts the expiry and repeat rules, and a negative
    ///   phlo floor rejects every deploy;
    /// * a zero leak never drains, a zero threshold never ejects, a zero
    ///   exponent makes every reward zero, and a zero recency reference
    ///   saturates the factor for every block.
    pub fn checked(value: impl Fn(&str) -> Result<BigInt, String>) -> Result<Self, String> {
        let in_range = |name: &str, low: i64, high: i64| -> Result<i64, String> {
            let found = value(name)?;
            i64::try_from(&found)
                .ok()
                .filter(|found| (low..=high).contains(found))
                .ok_or_else(|| format!("{name} out of range [{low}, {high}]: {found}"))
        };
        let positive = |name: &str, high: u32| -> Result<NonZeroU32, String> {
            let found = value(name)?;
            u32::try_from(&found)
                .ok()
                .filter(|positive| *positive <= high)
                .and_then(NonZeroU32::new)
                .ok_or_else(|| format!("{name} out of range [1, {high}]: {found}"))
        };
        let ppm = PPM as i64;
        let epoch_length = positive("epochLength", i32::MAX as u32)?;
        let basis = WindowBasis {
            max_parent_depth: in_range("maxParentDepth", 1, i32::MAX as i64)? as i32,
            deploy_lifespan: in_range("deployLifespan", 1, i32::MAX as i64)?,
            epoch_length: epoch_length.get() as i32,
        };
        Ok(Self {
            fault_tolerance_threshold_ppm: in_range("faultToleranceThresholdPpm", -ppm, ppm)?,
            max_parent_depth: basis.max_parent_depth,
            deploy_lifespan: basis.deploy_lifespan,
            min_phlo_price: TokenAmount::new(value("minPhloPrice")?)
                .map_err(|error| format!("minPhloPrice {error}"))?,
            epoch_length: basis.epoch_length,
            quarantine_length: in_range("quarantineLength", 1, i32::MAX as i64)? as i32,
            leaky_bucket: LeakyBucketParams {
                lambda_ppm: in_range("lambdaBasePpm", 1, ppm)? as u64,
                theta_eject_ppm: in_range("thetaEjectPpm", 1, ppm)? as u64,
                eject_observation_blocks: in_range("ejectObservationBlocks", 1, i64::MAX)? as u64,
                claim_deadline_blocks: basis.claim_deadline_blocks(),
                retention_window_blocks: basis.block_retention_window(),
                vote_list_bound: positive("voteListBound", u32::MAX)?,
                reward: RewardParams {
                    reward_rate_ppm: in_range("rewardRatePpm", 0, i64::MAX)? as u64,
                    p_breadth: in_range("breadthExponent", 1, u32::MAX as i64)? as u32,
                    y_ref_ppm: in_range("recencyReferencePpm", 1, i64::MAX)? as u64,
                    previous_reach: basis.previous_block_reach(),
                },
            },
        })
    }

    /// The threshold as a fraction, for display only.
    pub fn fault_tolerance_threshold(&self) -> f32 {
        (self.fault_tolerance_threshold_ppm as f64 / 1_000_000.0) as f32
    }

    pub fn window_basis(&self) -> WindowBasis {
        WindowBasis {
            max_parent_depth: self.max_parent_depth,
            deploy_lifespan: self.deploy_lifespan,
            epoch_length: self.epoch_length,
        }
    }
}

/// The values every window the chain derives rather than fixes is measured
/// from. Genesis writes the derived values into the PoS contract, and a node
/// derives the same ones from the values it reads back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindowBasis {
    pub max_parent_depth: i32,
    pub deploy_lifespan: i64,
    pub epoch_length: i32,
}

impl WindowBasis {
    /// One epoch. A member owes its vote in its next block, and nothing bounds
    /// how many blocks pass before that block, so the parent depth cannot size
    /// this.
    pub fn claim_deadline_blocks(&self) -> i64 { self.epoch_length as i64 }

    pub fn ejection_grace_blocks(&self) -> i64 { 2 * self.epoch_length as i64 }

    /// How many blocks below a block a node must hold to judge it: a parent
    /// may sit `max_parent_depth` below, and from there the expiry scan reads
    /// back the deploy lifespan, vote checks read back to a claim's opening,
    /// and slash checks read back to evidence from the current epoch.
    ///
    /// `None` when the parent-depth check is disabled: parents may then sit at
    /// any depth, and no finite window covers them.
    pub fn block_retention_window(&self) -> Option<i64> {
        if self.max_parent_depth == i32::MAX {
            return None;
        }
        let look_back = self
            .deploy_lifespan
            .max(self.claim_deadline_blocks())
            .max(self.epoch_length as i64);
        Some((self.max_parent_depth as i64).saturating_add(look_back))
    }

    /// How far below a block its sender's previous block counts for its
    /// reward: a parent depth and one further, which every node judging the
    /// block holds. `None` when the parent-depth check is disabled.
    pub fn previous_block_reach(&self) -> Option<i64> {
        (self.max_parent_depth != i32::MAX).then(|| self.max_parent_depth as i64 + 1)
    }

    /// How far below a judging block's floor dishonest-vote evidence counts the
    /// claimant's blocks. A node that can judge a block holds that block's
    /// floor state, so its anchor is at most a parent depth above the floor,
    /// and it holds the retention window below its anchor; each block counted
    /// also needs its parents, up to a parent depth and one further down.
    pub fn evidence_reach(&self) -> Option<i64> {
        self.block_retention_window()
            .map(|window| window - 2 * self.max_parent_depth as i64 - 1)
    }

    /// A validator that votes and then unbonds must stay slashable
    /// through the grace of any ejection it voted on and for as long as
    /// evidence against it can still be checked, with an epoch of margin.
    pub fn minimum_quarantine(&self) -> Option<i64> {
        self.block_retention_window().map(|window| {
            self.ejection_grace_blocks()
                .saturating_add(window)
                .saturating_add(self.epoch_length as i64)
        })
    }
}

#[cfg(any(test, feature = "test-utils"))]
impl ChainParameters {
    /// The values the shipped genesis carries, for fixtures that hold no chain
    /// state to read them from.
    pub fn shipped() -> Self {
        use crate::rust::genesis::contracts::proof_of_stake::ProofOfStake;

        let basis = WindowBasis {
            max_parent_depth: 15,
            deploy_lifespan: 50,
            epoch_length: 10_000,
        };
        Self {
            fault_tolerance_threshold_ppm: 670_000,
            max_parent_depth: basis.max_parent_depth,
            deploy_lifespan: basis.deploy_lifespan,
            min_phlo_price: 1.into(),
            epoch_length: basis.epoch_length,
            quarantine_length: 50_000,
            leaky_bucket: LeakyBucketParams {
                lambda_ppm: ProofOfStake::DEFAULT_LAMBDA_BASE_PPM as u64,
                theta_eject_ppm: ProofOfStake::DEFAULT_THETA_EJECT_PPM as u64,
                eject_observation_blocks: ProofOfStake::DEFAULT_EJECT_OBSERVATION_BLOCKS as u64,
                claim_deadline_blocks: basis.claim_deadline_blocks(),
                retention_window_blocks: basis.block_retention_window(),
                vote_list_bound: NonZeroU32::new(ProofOfStake::DEFAULT_VOTE_LIST_BOUND as u32)
                    .expect("the shipped vote list bound is positive"),
                reward: RewardParams {
                    reward_rate_ppm: ProofOfStake::DEFAULT_REWARD_RATE_PPM as u64,
                    p_breadth: ProofOfStake::DEFAULT_BREADTH_EXPONENT as u32,
                    y_ref_ppm: ProofOfStake::DEFAULT_RECENCY_REFERENCE_PPM as u64,
                    previous_reach: basis.previous_block_reach(),
                },
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn basis(max_parent_depth: i32, deploy_lifespan: i64, epoch_length: i32) -> WindowBasis {
        WindowBasis {
            max_parent_depth,
            deploy_lifespan,
            epoch_length,
        }
    }

    /// The parent reach and the look-back compose: the deepest block this node
    /// may be asked to judge sits a parent depth down, and its checks read
    /// further back from there.
    #[test]
    fn the_window_adds_the_parent_depth_to_the_longest_look_back() {
        assert_eq!(
            ChainParameters::shipped()
                .window_basis()
                .block_retention_window(),
            Some(10_015)
        );
    }

    /// Whichever look-back is longest governs: the lifespan, or the epoch,
    /// which is also the claim deadline.
    #[test]
    fn each_look_back_can_govern() {
        assert_eq!(basis(15, 500, 100).block_retention_window(), Some(515));
        assert_eq!(basis(15, 50, 700).block_retention_window(), Some(715));
    }

    #[test]
    fn a_disabled_depth_check_has_no_finite_window_or_quarantine() {
        let basis = basis(i32::MAX, 50, 10_000);
        assert_eq!(basis.block_retention_window(), None);
        assert_eq!(basis.minimum_quarantine(), None);
    }

    /// Genesis and a node's read both check through here, so a chain can carry
    /// no value a node refuses, and a node names the value it refuses.
    #[test]
    fn an_out_of_range_parameter_is_refused_by_name() {
        let error = ChainParameters::checked(|name| match name {
            "maxParentDepth" => Ok(0.into()),
            _ => Ok(1.into()),
        })
        .expect_err("a zero parent depth has no parent-spread rule");
        assert!(error.contains("maxParentDepth out of range"), "{error}");
    }

    #[test]
    fn the_derived_values_are_proportional_to_their_basis() {
        let shipped = ChainParameters::shipped().window_basis();
        assert_eq!(shipped.claim_deadline_blocks(), 10_000);
        assert_eq!(shipped.ejection_grace_blocks(), 20_000);
        assert_eq!(
            shipped.minimum_quarantine(),
            Some(20_000 + 10_015 + 10_000),
            "grace, then the retention window, then an epoch of margin"
        );
    }
}
