//! JSON form of a node's validator liveness report. Validators are hex
//! public keys. Bucket levels, thresholds, overflow and shortfall are in
//! millionths of a bond unit, as decimal strings, since they exceed 64 bits.

use casper::rust::api::validator_liveness::ValidatorLiveness;
use casper::rust::economics::claims::ClaimTerms;
use models::rust::casper::protocol::casper_message::{Bond, ClaimFamily, FreezeInterval};
use models::rust::validator::Validator;
use serde::Serialize;
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ValidatorLivenessResponse {
    pub params: LivenessParams,
    /// This node's own view: it depends on which blocks the node accepted,
    /// and in what order, so nodes may disagree.
    pub local_view: LocalView,
    /// What this node would offer and vote in its next block; absent on a
    /// node without a validator identity.
    pub decision: Option<Decision>,
    /// What this node's last finalized block's state records.
    pub finalized_state: FinalizedState,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LivenessParams {
    pub lambda_ppm: u64,
    pub theta_eject_ppm: u64,
    pub eject_observation_blocks: u64,
    pub claim_deadline_blocks: i64,
    pub retention_window_blocks: Option<i64>,
    pub vote_list_bound: u32,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LocalView {
    pub reconciled_floor: i64,
    pub credited_from: i64,
    pub tip_height: i64,
    pub tip_committee: Vec<String>,
    pub buckets: Vec<Bucket>,
    pub open_ejection_claims: Vec<OpenEjectionClaim>,
    pub slashed: Vec<String>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Bucket {
    pub validator: String,
    pub capacity: u64,
    pub level: String,
    pub threshold: String,
    pub drained: bool,
    pub blocks_below_threshold: u64,
    /// Everything credited, before payments are taken off.
    pub overflow: String,
    /// Bond units paid out of the overflow since this view began crediting.
    pub paid: i64,
    pub ejected: bool,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OpenEjectionClaim {
    pub target: String,
    pub opened_at: i64,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Decision {
    pub ejection_offers: Vec<String>,
    pub drained: Vec<String>,
    pub shortfall: Vec<ValidatorAmount>,
    pub overflow: Vec<ValidatorAmount>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ValidatorAmount {
    pub validator: String,
    pub amount: String,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FinalizedState {
    pub block_hash: String,
    pub block_number: i64,
    pub bonds: Vec<Stake>,
    pub active_validators: Vec<String>,
    /// Stake withdrawn and still in quarantine.
    pub withdrawers: Vec<Stake>,
    pub claims: Vec<Claim>,
    pub ejected: Vec<String>,
    pub grace_ends: Vec<GraceEnd>,
    pub freezes: Vec<Freezes>,
    /// Overflow each validator has been paid in all.
    pub claimed: Vec<Stake>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Stake {
    pub validator: String,
    pub stake: i64,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Claim {
    pub family: String,
    pub target: String,
    pub opened_at: i64,
    pub proposer: String,
    pub electorate: Vec<Stake>,
    pub terms: Terms,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Terms {
    Eject,
    #[serde(rename_all = "camelCase")]
    Rejoin {
        topup: i64,
        grace_end: i64,
    },
    Overflow {
        amount: i64,
    },
}

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GraceEnd {
    pub validator: String,
    pub grace_end: i64,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Freezes {
    pub validator: String,
    pub intervals: Vec<Freeze>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Freeze {
    pub from: i64,
    pub rejoined_at: Option<i64>,
    pub topup: Option<i64>,
}

/// A report with one of everything: a drained bucket, an open ejection
/// claim, a rejoin claim, an ejected validator in grace, and a withdrawal in
/// quarantine.
#[cfg(test)]
pub(crate) fn sample() -> ValidatorLiveness {
    use std::collections::{HashMap, HashSet};
    use std::sync::Arc;

    use casper::rust::api::validator_liveness::FinalizedLiveness;
    use casper::rust::economics::bucket_view::{
        BucketSnapshot, LeakyBucketParams, LeakyBucketProposal, ViewSnapshot,
    };
    use casper::rust::economics::claims::ClaimRecord;
    use casper::rust::rholang::runtime::{Claimed, FloorState};
    use models::rust::casper::protocol::casper_message::{ClaimId, Rejoin};

    let v = |byte: u8| Validator::from(vec![byte; models::rust::validator::LENGTH]);
    let params = LeakyBucketParams::shipped();
    let bond = |byte: u8, stake: i64| Bond {
        validator: v(byte),
        stake,
    };
    ValidatorLiveness {
        params,
        local_view: ViewSnapshot {
            reconciled_floor: 40,
            credited_from: 0,
            tip_height: 44,
            tip_committee: vec![v(1), v(2)],
            buckets: vec![BucketSnapshot {
                validator: v(2),
                capacity: 1_000,
                level: 30_000_000_000_000_000_000,
                threshold: 300_000_000,
                drained: true,
                blocks_below_threshold: 61,
                overflow: 0,
                paid: 0,
                ejected: false,
            }],
            open_ejection_claims: vec![(v(2), 42)],
            slashed: vec![],
        },
        decision: Some(LeakyBucketProposal {
            params,
            ejection_offers: vec![v(2)],
            drained: HashSet::from([v(2)]),
            shortfall: HashMap::from([(v(2), 970_000_000)]),
            overflow: HashMap::from([(v(1), 5)]),
            accusations: vec![],
        }),
        finalized_state: FinalizedLiveness {
            block_hash: prost::bytes::Bytes::from(vec![0xAB; 32]),
            block_number: 40,
            state: Arc::new(FloorState {
                bonds: vec![bond(1, 1_000), bond(2, 1_000)],
                active_validators: vec![v(1), v(2)],
                withdrawers: vec![bond(4, 300)],
                claims: vec![ClaimRecord {
                    claim: ClaimId {
                        family: ClaimFamily::Rejoin,
                        target: v(3),
                        opened_at: 39,
                        proposer: v(3),
                    },
                    electorate: vec![bond(1, 1_000), bond(2, 1_000)],
                    terms: ClaimTerms::Rejoin {
                        topup: 20,
                        grace_end: 60,
                    },
                }],
                ejected: vec![v(3)],
                freezes: vec![(v(3), vec![
                    FreezeInterval {
                        from: 10,
                        rejoin: Some(Rejoin { at: 20, topup: 5 }),
                    },
                    FreezeInterval {
                        from: 30,
                        rejoin: None,
                    },
                ])],
                claimed: vec![Claimed {
                    validator: v(1),
                    total: 7,
                    payments: vec![],
                }],
            }),
            grace_ends: vec![(v(3), 60)],
        },
    }
}

fn key(validator: &Validator) -> String { hex::encode(validator) }

fn keys<'a>(validators: impl IntoIterator<Item = &'a Validator>) -> Vec<String> {
    let mut keys: Vec<String> = validators.into_iter().map(key).collect();
    keys.sort();
    keys
}

fn stakes(bonds: &[Bond]) -> Vec<Stake> {
    let mut stakes: Vec<Stake> = bonds
        .iter()
        .map(|bond| Stake {
            validator: key(&bond.validator),
            stake: bond.stake,
        })
        .collect();
    stakes.sort_by(|a, b| a.validator.cmp(&b.validator));
    stakes
}

fn amounts<'a>(
    amounts: impl IntoIterator<Item = (&'a Validator, &'a u128)>,
) -> Vec<ValidatorAmount> {
    let mut amounts: Vec<ValidatorAmount> = amounts
        .into_iter()
        .map(|(validator, amount)| ValidatorAmount {
            validator: key(validator),
            amount: amount.to_string(),
        })
        .collect();
    amounts.sort_by(|a, b| a.validator.cmp(&b.validator));
    amounts
}

fn family(family: ClaimFamily) -> String {
    match family {
        ClaimFamily::Eject => "eject",
        ClaimFamily::Rejoin => "rejoin",
        ClaimFamily::Overflow => "overflow",
    }
    .to_string()
}

fn freeze(interval: &FreezeInterval) -> Freeze {
    Freeze {
        from: interval.from,
        rejoined_at: interval.rejoin.map(|rejoin| rejoin.at),
        topup: interval.rejoin.map(|rejoin| rejoin.topup),
    }
}

impl From<&ValidatorLiveness> for ValidatorLivenessResponse {
    fn from(report: &ValidatorLiveness) -> Self {
        let params = &report.params;
        let view = &report.local_view;
        let finalized = &report.finalized_state;
        let state = &finalized.state;
        Self {
            params: LivenessParams {
                lambda_ppm: params.lambda_ppm,
                theta_eject_ppm: params.theta_eject_ppm,
                eject_observation_blocks: params.eject_observation_blocks,
                claim_deadline_blocks: params.claim_deadline_blocks,
                retention_window_blocks: params.retention_window_blocks,
                vote_list_bound: params.vote_list_bound.get(),
            },
            local_view: LocalView {
                reconciled_floor: view.reconciled_floor,
                credited_from: view.credited_from,
                tip_height: view.tip_height,
                tip_committee: keys(&view.tip_committee),
                buckets: view
                    .buckets
                    .iter()
                    .map(|bucket| Bucket {
                        validator: key(&bucket.validator),
                        capacity: bucket.capacity,
                        level: bucket.level.to_string(),
                        threshold: bucket.threshold.to_string(),
                        drained: bucket.drained,
                        blocks_below_threshold: bucket.blocks_below_threshold,
                        overflow: bucket.overflow.to_string(),
                        paid: bucket.paid,
                        ejected: bucket.ejected,
                    })
                    .collect(),
                open_ejection_claims: view
                    .open_ejection_claims
                    .iter()
                    .map(|(target, opened_at)| OpenEjectionClaim {
                        target: key(target),
                        opened_at: *opened_at,
                    })
                    .collect(),
                slashed: keys(&view.slashed),
            },
            decision: report.decision.as_ref().map(|decision| Decision {
                ejection_offers: decision.ejection_offers.iter().map(key).collect(),
                drained: keys(&decision.drained),
                shortfall: amounts(&decision.shortfall),
                overflow: amounts(&decision.overflow),
            }),
            finalized_state: FinalizedState {
                block_hash: hex::encode(&finalized.block_hash),
                block_number: finalized.block_number,
                bonds: stakes(&state.bonds),
                active_validators: keys(&state.active_validators),
                withdrawers: stakes(&state.withdrawers),
                claims: state
                    .claims
                    .iter()
                    .map(|record| Claim {
                        family: family(record.claim.family),
                        target: key(&record.claim.target),
                        opened_at: record.claim.opened_at,
                        proposer: key(&record.claim.proposer),
                        electorate: stakes(&record.electorate),
                        terms: match record.terms {
                            ClaimTerms::Eject => Terms::Eject,
                            ClaimTerms::Rejoin { topup, grace_end } => {
                                Terms::Rejoin { topup, grace_end }
                            }
                            ClaimTerms::Overflow { amount } => Terms::Overflow { amount },
                        },
                    })
                    .collect(),
                ejected: keys(&state.ejected),
                grace_ends: finalized
                    .grace_ends
                    .iter()
                    .map(|(validator, grace_end)| GraceEnd {
                        validator: key(validator),
                        grace_end: *grace_end,
                    })
                    .collect(),
                freezes: state
                    .freezes
                    .iter()
                    .map(|(validator, intervals)| Freezes {
                        validator: key(validator),
                        intervals: intervals.iter().map(freeze).collect(),
                    })
                    .collect(),
                claimed: state
                    .claimed
                    .iter()
                    .map(|claimed| Stake {
                        validator: key(&claimed.validator),
                        stake: claimed.total,
                    })
                    .collect(),
            },
        }
    }
}
