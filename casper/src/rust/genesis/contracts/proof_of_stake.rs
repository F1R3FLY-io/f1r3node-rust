// See casper/src/main/scala/coop/rchain/casper/genesis/contracts/ProofOfStake.scala

use shared::rust::token_amount::TokenAmount;

use super::validator::Validator;
use crate::rust::chain_parameters::WindowBasis;

// TODO: Eliminate validators argument if unnecessary. - OLD
// TODO: eliminate the default for epochLength. Now it is used in order to minimise the impact of adding this parameter - OLD
// TODO: Remove hardcoded keys from standard deploys: https://rchain.atlassian.net/browse/RCHAIN-3321?atlOrigin=eyJpIjoiNDc0NjE4YzYxOTRkNDcyYjljZDdlOWMxYjE1NWUxNjIiLCJwIjoiaiJ9 - OLD
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct ProofOfStake {
    pub minimum_bond: TokenAmount,
    pub maximum_bond: TokenAmount,
    pub validators: Vec<Validator>,
    pub epoch_length: i32,
    pub quarantine_length: i32,
    pub number_of_active_validators: u32,
    pub fault_tolerance_threshold_ppm: i64,
    pub max_parent_depth: i32,
    pub deploy_lifespan: i64,
    pub min_phlo_price: TokenAmount,
    /// Reward rate per block, in ppm of the proposer's stake per committee
    /// member.
    pub reward_rate_ppm: i64,
    /// Fraction of its capacity a validator's bucket loses per block, in ppm.
    pub lambda_base_ppm: i64,
    /// Level below which a validator is a candidate for ejection, as a ppm
    /// fraction of its capacity.
    pub theta_eject_ppm: i64,
    /// Blocks a level must stay below the threshold before ejection is open.
    pub eject_observation_blocks: i64,
    /// Exponent of the reward's breadth factor.
    pub breadth_exponent: i64,
    /// Recency reference the reward's recency factor is capped against, in ppm.
    pub recency_reference_ppm: i64,
    /// Most votes one block may carry.
    pub vote_list_bound: i64,
    /// The largest overflow one claim may ask for, in minimum bonds.
    pub max_overflow_claim_minimum_bonds: i64,
    /// What a failed overflow claim costs its claimant, in ppm of its bond.
    pub failed_claim_fee_ppm: i64,
    pub pos_multi_sig_public_keys: Vec<String>,
    pub pos_multi_sig_quorum: u32,
}

/// The leaky-bucket parameters as configured for genesis, carried as one value
/// so the ceremony entry points take a single argument rather than six more.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LeakyBucketGenesis {
    pub reward_rate_ppm: i64,
    pub lambda_base_ppm: i64,
    pub theta_eject_ppm: i64,
    pub eject_observation_blocks: i64,
    pub breadth_exponent: i64,
    pub recency_reference_ppm: i64,
    pub vote_list_bound: i64,
    pub max_overflow_claim_minimum_bonds: i64,
    pub failed_claim_fee_ppm: i64,
}

impl Default for LeakyBucketGenesis {
    fn default() -> Self {
        Self {
            reward_rate_ppm: ProofOfStake::DEFAULT_REWARD_RATE_PPM,
            lambda_base_ppm: ProofOfStake::DEFAULT_LAMBDA_BASE_PPM,
            theta_eject_ppm: ProofOfStake::DEFAULT_THETA_EJECT_PPM,
            eject_observation_blocks: ProofOfStake::DEFAULT_EJECT_OBSERVATION_BLOCKS,
            breadth_exponent: ProofOfStake::DEFAULT_BREADTH_EXPONENT,
            recency_reference_ppm: ProofOfStake::DEFAULT_RECENCY_REFERENCE_PPM,
            vote_list_bound: ProofOfStake::DEFAULT_VOTE_LIST_BOUND,
            max_overflow_claim_minimum_bonds:
                ProofOfStake::DEFAULT_MAX_OVERFLOW_CLAIM_MINIMUM_BONDS,
            failed_claim_fee_ppm: ProofOfStake::DEFAULT_FAILED_CLAIM_FEE_PPM,
        }
    }
}

impl ProofOfStake {
    /// Shipped leaky-bucket genesis values, and the code fallbacks that must
    /// equal the keys in `defaults.conf`, where their derivation is given.
    pub const DEFAULT_REWARD_RATE_PPM: i64 = 630;
    pub const DEFAULT_LAMBDA_BASE_PPM: i64 = 140;
    pub const DEFAULT_THETA_EJECT_PPM: i64 = 900_000;
    pub const DEFAULT_EJECT_OBSERVATION_BLOCKS: i64 = 60;
    pub const DEFAULT_BREADTH_EXPONENT: i64 = 2;
    pub const DEFAULT_RECENCY_REFERENCE_PPM: i64 = 1_000_000;
    pub const DEFAULT_VOTE_LIST_BOUND: i64 = 32;
    /// One claim may ask for at most this many minimum bonds.
    pub const DEFAULT_MAX_OVERFLOW_CLAIM_MINIMUM_BONDS: i64 = 10;
    /// One per mil of the claimant's bond.
    pub const DEFAULT_FAILED_CLAIM_FEE_PPM: i64 = 1_000;

    /// The leaky-bucket parameters this genesis carries, gathered back into one
    /// value for callers that must reproduce them exactly.
    pub fn leaky_bucket(&self) -> LeakyBucketGenesis {
        LeakyBucketGenesis {
            reward_rate_ppm: self.reward_rate_ppm,
            lambda_base_ppm: self.lambda_base_ppm,
            theta_eject_ppm: self.theta_eject_ppm,
            eject_observation_blocks: self.eject_observation_blocks,
            breadth_exponent: self.breadth_exponent,
            recency_reference_ppm: self.recency_reference_ppm,
            vote_list_bound: self.vote_list_bound,
            max_overflow_claim_minimum_bonds: self.max_overflow_claim_minimum_bonds,
            failed_claim_fee_ppm: self.failed_claim_fee_ppm,
        }
    }

    /// The chain parameter this genesis writes under `name`, as the PoS
    /// contract's `getChainParameters` reports it back.
    pub fn chain_parameter(&self, name: &str) -> Result<num_bigint::BigInt, String> {
        Ok(match name {
            "faultToleranceThresholdPpm" => self.fault_tolerance_threshold_ppm.into(),
            "maxParentDepth" => self.max_parent_depth.into(),
            "deployLifespan" => self.deploy_lifespan.into(),
            "minPhloPrice" => self.min_phlo_price.to_bigint(),
            "epochLength" => self.epoch_length.into(),
            "quarantineLength" => self.quarantine_length.into(),
            "lambdaBasePpm" => self.lambda_base_ppm.into(),
            "thetaEjectPpm" => self.theta_eject_ppm.into(),
            "ejectObservationBlocks" => self.eject_observation_blocks.into(),
            "breadthExponent" => self.breadth_exponent.into(),
            "recencyReferencePpm" => self.recency_reference_ppm.into(),
            "voteListBound" => self.vote_list_bound.into(),
            "rewardRatePpm" => self.reward_rate_ppm.into(),
            other => return Err(format!("genesis writes no chain parameter {other}")),
        })
    }

    pub fn window_basis(&self) -> WindowBasis {
        WindowBasis {
            max_parent_depth: self.max_parent_depth,
            deploy_lifespan: self.deploy_lifespan,
            epoch_length: self.epoch_length,
        }
    }

    pub fn fault_tolerance_threshold_to_ppm(threshold: f32) -> i64 {
        ((threshold as f64) * 1_000_000.0).round() as i64
    }

    // TODO: Determine how the "initial bonds" map can simulate transferring stake into the PoS contract
    //       when this must be done during genesis, under the authority of the genesisPk, which calls the
    //       linear receive in PoS.rho - OLD
    pub fn initial_bonds(validators: &[Validator]) -> String {
        let mut sorted_validators = validators.to_vec();
        sorted_validators.sort_by(|a, b| a.pk.bytes.cmp(&b.pk.bytes));

        let map_entries = sorted_validators
            .iter()
            .map(|validator| {
                let pk_string = hex::encode(validator.pk.bytes.clone());
                format!(" \"{}\".hexToBytes() : {}n", pk_string, validator.stake)
            })
            .collect::<Vec<String>>()
            .join(", ");

        format!("{{{}}}", map_entries)
    }

    pub fn public_keys(pos_multi_sig_public_keys: &[String]) -> String {
        let indent_brackets = 12;
        let indent_keys = indent_brackets + 2;

        let pub_key_items = pos_multi_sig_public_keys
            .iter()
            .map(|pk| format!("{}\"{}\".hexToBytes()", " ".repeat(indent_keys), pk))
            .collect::<Vec<String>>()
            .join(",\n");

        format!("[\n{}\n{}]", pub_key_items, " ".repeat(indent_brackets))
    }
}
