use std::collections::{BTreeMap, HashSet};

use block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation;
use models::rust::casper::protocol::casper_message::{Bond, Justification};
use models::rust::validator::Validator;
use num_bigint::BigInt;
use shared::rust::store::key_value_store::MissingBlockContext;

use crate::rust::errors::CasperError;

/// Fixed-point scale for every reward factor, and the scale of the reward
/// itself. The arithmetic is integer throughout because rewards are
/// recomputed on other nodes from the same public inputs and the results are
/// compared, so they must agree bit for bit — `f64::powf` carries no such
/// guarantee across platforms.
pub const PPM: u64 = 1_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RewardParams {
    /// The reward rate, in ppm of the proposer's stake per block: a block
    /// credits this for every committee member, since each member proposes
    /// about one block in that many.
    pub reward_rate_ppm: u64,
    /// Exponent of `f_breadth`. Zero makes the breadth factor identically
    /// zero, and therefore the reward zero — a misconfiguration that is
    /// reported as no reward rather than silently treated as one.
    pub p_breadth: u32,
    pub y_ref_ppm: u64,
    /// How far below a block its sender's previous block may sit and still
    /// count as previous: one past the parent depth, which every node judging
    /// the block holds. `None` when the parent depth is unbounded.
    pub previous_reach: Option<i64>,
}

/// What the reward needs from one parent: who authored it and how far below
/// the child it sits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParentFact {
    pub sender: Validator,
    pub block_number: i64,
}

/// The block's committee, each validator once with its stake.
pub type Committee = BTreeMap<Validator, BigInt>;

/// `bonds` as a committee.
pub fn committee_of(bonds: &[Bond]) -> Committee {
    bonds
        .iter()
        .map(|bond| (bond.validator.clone(), bond.stake.to_bigint()))
        .collect()
}

/// The validators other than `sender` whose latest block `justifications`
/// names differently from `previous_own`, the justifications of `sender`'s
/// previous block: the members whose new work the block acknowledges. With no
/// previous block, every other justified validator is new.
pub fn fresh_validators(
    sender: &Validator,
    justifications: &[Justification],
    previous_own: Option<&[Justification]>,
) -> Vec<Validator> {
    let before: BTreeMap<&Validator, &prost::bytes::Bytes> = previous_own
        .into_iter()
        .flatten()
        .map(|justification| (&justification.validator, &justification.latest_block_hash))
        .collect();
    let mut fresh: Vec<Validator> = justifications
        .iter()
        .filter(|justification| {
            justification.validator != *sender
                && before.get(&justification.validator) != Some(&&justification.latest_block_hash)
        })
        .map(|justification| justification.validator.clone())
        .collect();
    fresh.sort();
    fresh.dedup();
    fresh
}

/// `fresh_validators` for block `block_number` by `sender` justifying
/// `justifications`, its previous block found through the DAG. A
/// justification of `sender` naming a block `sender` did not sign — genesis,
/// for a member that has not proposed — is no previous block, nor is one more
/// than `previous_reach` below: nodes need not hold it, and every node must
/// reach the same reward. A previous block this node does not hold is
/// `BlockNotHeld`, an absence to fetch, never a fault of the block.
pub fn fresh_in(
    dag: &KeyValueDagRepresentation,
    sender: &Validator,
    justifications: &[Justification],
    block_number: i64,
    previous_reach: Option<i64>,
) -> Result<Vec<Validator>, CasperError> {
    let previous_own = match justifications
        .iter()
        .find(|justification| justification.validator == *sender)
    {
        Some(justification) => {
            let previous = dag
                .lookup(&justification.latest_block_hash)?
                .ok_or_else(|| {
                    CasperError::BlockNotHeld(
                        justification.latest_block_hash.clone(),
                        MissingBlockContext::new("fresh_in"),
                    )
                })?;
            Some(previous).filter(|previous| {
                previous.sender == *sender
                    && previous_reach.is_none_or(|reach| {
                        previous.block_number >= block_number.saturating_sub(reach)
                    })
            })
        }
        None => None,
    };
    Ok(fresh_validators(
        sender,
        justifications,
        previous_own
            .as_ref()
            .map(|previous| previous.justifications.as_slice()),
    ))
}

/// The reward `sender`'s block earns: a share of the sender's stake scaled by
/// how much of the committee's new work the block acknowledges (breadth) and
/// how recent its parents are (recency). The committee is the block's own.
///
/// Breadth is the committee stake of the `fresh` validators, those whose new
/// blocks this block acknowledges since its sender's previous one, over the
/// committee's total. It depends on what the sender has seen, not on who
/// proposed just before. A fresh validator absent from the committee adds
/// nothing.
///
/// The reward is proportional to the sender's stake because the drain is: a
/// member proposing its share of blocks keeps the same level whatever its
/// bond.
///
/// The result is in units of `1 / PPM` of a bond unit. Both factors are
/// fractions, so a reward expressed in whole bond units would round to
/// nothing for every block that does not saturate both of them.
pub fn compute_reward(
    committee: &Committee,
    sender: &Validator,
    fresh: &[Validator],
    parents: &[ParentFact],
    block_number: i64,
    params: &RewardParams,
) -> BigInt {
    let zero = BigInt::from(0);
    let total: BigInt = committee.values().sum();
    let members = committee.values().filter(|stake| **stake > zero).count();
    // A sender the committee does not seat has no stake to credit.
    let Some(own_stake) = committee.get(sender).filter(|stake| **stake > zero) else {
        return zero;
    };
    if total <= zero || members == 0 {
        return zero;
    }

    let mut counted: HashSet<&Validator> = HashSet::new();
    let mut fresh_stake = BigInt::from(0);
    for validator in fresh {
        if validator == sender {
            continue;
        }
        if let Some(stake) = committee.get(validator) {
            if counted.insert(validator) && *stake > zero {
                fresh_stake += stake;
            }
        }
    }
    let mut recency_ppm: u64 = 0;
    for parent in parents {
        let gap = block_number.saturating_sub(parent.block_number).max(0) as u64;
        recency_ppm = recency_ppm.saturating_add(PPM / (1 + gap));
    }

    let breadth_ppm = u64::try_from(fresh_stake * PPM / &total)
        .expect("the fresh stake is part of the committee's total, so breadth is at most one");
    let factor_ppm = f_breadth(breadth_ppm, params.p_breadth) as u128
        * f_recency(recency_ppm, params.y_ref_ppm) as u128
        / PPM as u128;
    BigInt::from(factor_ppm) * params.reward_rate_ppm * members * own_stake / PPM
}

/// `1 − (1 − x)^p`, on the ppm scale. The power is taken by repeated
/// squaring, so its cost grows with the exponent's bit length, not its size.
fn f_breadth(breadth_ppm: u64, p_breadth: u32) -> u64 {
    let scale = PPM as u128;
    let mut base = PPM.saturating_sub(breadth_ppm) as u128;
    let mut power = scale;
    let mut exponent = p_breadth;
    while exponent > 0 {
        if exponent & 1 == 1 {
            power = power * base / scale;
        }
        exponent >>= 1;
        if exponent > 0 {
            base = base * base / scale;
        }
    }
    PPM.saturating_sub(power as u64)
}

/// `min(1, y / y_ref)`, on the ppm scale. A zero reference is the limit of
/// that ratio, so the factor saturates: that is the value the formula gives,
/// not a default standing in for a missing one.
fn f_recency(recency_ppm: u64, y_ref_ppm: u64) -> u64 {
    if y_ref_ppm == 0 {
        return PPM;
    }
    ((recency_ppm as u128 * PPM as u128 / y_ref_ppm as u128).min(PPM as u128)) as u64
}

#[cfg(test)]
mod tests {
    use prost::bytes::Bytes;

    use super::*;

    fn validator(byte: u8) -> Validator { Bytes::from(vec![byte; models::rust::validator::LENGTH]) }

    /// The shipped factors, at a rate of `rate_ppm` of the proposer's stake per
    /// member per block.
    fn at_rate(rate_ppm: u64) -> RewardParams {
        RewardParams {
            reward_rate_ppm: rate_ppm,
            p_breadth: 2,
            y_ref_ppm: PPM,
            previous_reach: Some(16),
        }
    }

    fn bonds(stakes: &[(u8, i64)]) -> Committee {
        stakes
            .iter()
            .map(|(byte, stake)| (validator(*byte), BigInt::from(*stake)))
            .collect()
    }

    fn zero() -> BigInt { BigInt::from(0) }

    fn parent(byte: u8, block_number: i64) -> ParentFact {
        ParentFact {
            sender: validator(byte),
            block_number,
        }
    }

    fn justification(byte: u8, hash: u8) -> Justification {
        Justification {
            validator: validator(byte),
            latest_block_hash: Bytes::from(vec![hash; 32]),
        }
    }

    /// Two proposers acknowledging the same new work earn in proportion to
    /// their stakes, because each drains in proportion to its stake.
    #[test]
    fn the_reward_is_proportional_to_the_proposers_stake() {
        let committee = bonds(&[(1, 100), (2, 1_000), (3, 5_000)]);
        let params = at_rate(630);
        let fresh = [validator(3)];
        let parents = [parent(3, 9)];
        let light = compute_reward(&committee, &validator(1), &fresh, &parents, 10, &params);
        let heavy = compute_reward(&committee, &validator(2), &fresh, &parents, 10, &params);
        assert!(light > zero());
        assert_eq!(heavy, light * 10);
    }

    /// Breadth measures the new work a block acknowledges, so the reward does
    /// not depend on whose block happens to be its parent.
    #[test]
    fn the_reward_does_not_depend_on_who_proposed_just_before() {
        let committee = bonds(&[(1, 100), (2, 500), (3, 1_000), (4, 10_000)]);
        let params = at_rate(630);
        let others = [validator(1), validator(3), validator(4)];
        let after_light = compute_reward(
            &committee,
            &validator(2),
            &others,
            &[parent(1, 9)],
            10,
            &params,
        );
        let after_heavy = compute_reward(
            &committee,
            &validator(2),
            &others,
            &[parent(4, 9)],
            10,
            &params,
        );
        assert_eq!(after_light, after_heavy);
    }

    /// A block acknowledging every other member's new work earns
    /// `f_breadth · f_recency · rate · members · stake`: committee 4 × 250,
    /// breadth 3/4 so `1 − (1/4)² = 0.9375`, three parents one height back
    /// saturate recency, and 630 ppm × 4 × 250 = 0.63 bond units.
    #[test]
    fn a_full_acknowledgement_earns_the_rate_times_members_times_stake() {
        let committee = bonds(&[(1, 250), (2, 250), (3, 250), (4, 250)]);
        let others = [validator(2), validator(3), validator(4)];
        let parents = [parent(2, 9), parent(3, 9), parent(4, 9)];
        assert_eq!(
            compute_reward(
                &committee,
                &validator(1),
                &others,
                &parents,
                10,
                &at_rate(630)
            ),
            BigInt::from(590_625)
        );
    }

    /// Acknowledging no new work pays nothing, and each additional member's
    /// acknowledged work pays more.
    #[test]
    fn breadth_rises_with_each_member_acknowledged() {
        let committee = bonds(&[(1, 250), (2, 250), (3, 250), (4, 250)]);
        let params = at_rate(630);
        let parents = [parent(1, 9)];
        assert_eq!(
            compute_reward(&committee, &validator(1), &[], &parents, 10, &params),
            zero()
        );
        let mut previous = zero();
        for acknowledged in 2..=4u8 {
            let fresh: Vec<Validator> = (2..=acknowledged).map(validator).collect();
            let reward = compute_reward(&committee, &validator(1), &fresh, &parents, 10, &params);
            assert!(reward > previous, "{acknowledged}: {reward} <= {previous}");
            previous = reward;
        }
    }

    /// The sender's own new block is not breadth, a member absent from the
    /// committee adds nothing, and a member named twice counts once.
    #[test]
    fn breadth_counts_each_other_committee_member_once() {
        let committee = bonds(&[(1, 500), (2, 500)]);
        let params = at_rate(630);
        let parents = [parent(2, 9)];
        let base = compute_reward(
            &committee,
            &validator(1),
            &[validator(2)],
            &parents,
            10,
            &params,
        );
        for fresh in [
            vec![validator(2), validator(1)],
            vec![validator(2), validator(9)],
            vec![validator(2), validator(2)],
        ] {
            assert_eq!(
                compute_reward(&committee, &validator(1), &fresh, &parents, 10, &params),
                base
            );
        }
    }

    /// Recency is capped by `y_ref`, and a staler parent set earns no more
    /// than a fresher one.
    #[test]
    fn recency_is_monotone_in_freshness() {
        let committee = bonds(&[(1, 500), (2, 500)]);
        let params = RewardParams {
            y_ref_ppm: 4 * PPM,
            ..at_rate(630)
        };
        let fresh = [validator(2)];
        let recent = compute_reward(
            &committee,
            &validator(1),
            &fresh,
            &[parent(1, 9), parent(2, 9)],
            10,
            &params,
        );
        let stale = compute_reward(
            &committee,
            &validator(1),
            &fresh,
            &[parent(1, 1), parent(2, 1)],
            10,
            &params,
        );
        assert!(recent > stale, "recent {recent}, stale {stale}");
    }

    /// A sender the committee does not seat earns nothing.
    #[test]
    fn a_sender_outside_the_committee_earns_nothing() {
        let committee = bonds(&[(1, 500), (2, 0)]);
        let params = at_rate(630);
        let parents = [parent(1, 9)];
        for sender in [validator(2), validator(9)] {
            assert_eq!(
                compute_reward(&committee, &sender, &[validator(1)], &parents, 10, &params),
                zero()
            );
        }
    }

    /// A committee holds each validator once, so a bonds list naming one twice
    /// pays what its single entry pays.
    #[test]
    fn a_validator_listed_twice_is_one_member() {
        let bond = |byte: u8| Bond {
            validator: validator(byte),
            stake: 250.into(),
        };
        let parents = [parent(1, 9), parent(2, 9)];
        let fresh = [validator(2)];
        let once = committee_of(&[bond(1), bond(2)]);
        let twice = committee_of(&[bond(1), bond(2), bond(2)]);
        assert_eq!(
            compute_reward(&twice, &validator(1), &fresh, &parents, 10, &at_rate(630)),
            compute_reward(&once, &validator(1), &fresh, &parents, 10, &at_rate(630))
        );
    }

    /// A previous block within reach decides what is new; one beyond it is no
    /// previous block, so every node, whether it holds that block or not,
    /// counts every other justified member as new.
    #[tokio::test]
    async fn a_previous_block_beyond_reach_is_no_previous_block() {
        use block_storage::rust::dag::block_dag_key_value_storage::{
            BlockDagKeyValueStorage, InsertMode,
        };
        use models::rust::block_implicits::get_random_block;
        use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;

        let mut kvm = InMemoryStoreManager::new();
        let dag_storage = BlockDagKeyValueStorage::new(&mut kvm)
            .await
            .expect("dag storage");
        let block = |number: i64, sender: u8, justifications: Vec<Justification>| {
            get_random_block(
                Some(number),
                Some(number as i32),
                None,
                None,
                Some(validator(sender)),
                None,
                Some(number),
                Some(Vec::new()),
                Some(justifications),
                Some(Vec::new()),
                Some(Vec::new()),
                None,
                Some("test".to_string()),
                None,
            )
        };
        let genesis = block(0, 0, Vec::new());
        let w_block = block(1, 2, Vec::new());
        let previous = block(2, 1, vec![Justification {
            validator: validator(2),
            latest_block_hash: w_block.block_hash.clone(),
        }]);
        dag_storage
            .insert(&genesis, InsertMode::Approved)
            .expect("insert");
        dag_storage
            .insert(&w_block, InsertMode::Normal)
            .expect("insert");
        dag_storage
            .insert(&previous, InsertMode::Normal)
            .expect("insert");
        let dag = dag_storage.get_representation().expect("dag");

        let same_w = vec![
            Justification {
                validator: validator(1),
                latest_block_hash: previous.block_hash.clone(),
            },
            Justification {
                validator: validator(2),
                latest_block_hash: w_block.block_hash.clone(),
            },
        ];
        let fresh_at =
            |number: i64| fresh_in(&dag, &validator(1), &same_w, number, Some(16)).expect("held");
        assert_eq!(
            fresh_at(18),
            Vec::<Validator>::new(),
            "16 below: nothing new"
        );
        assert_eq!(
            fresh_at(19),
            vec![validator(2)],
            "17 below: no previous block"
        );

        let unheld = vec![Justification {
            validator: validator(1),
            latest_block_hash: Bytes::from(vec![0xAB; 32]),
        }];
        assert!(
            matches!(
                fresh_in(&dag, &validator(1), &unheld, 3, Some(16)),
                Err(CasperError::BlockNotHeld(..))
            ),
            "an unheld previous block is an absence to fetch, not a fault"
        );
    }

    /// The members whose justification moved since the sender's previous block
    /// are fresh; the sender itself never is; with no previous block every
    /// other justified member is.
    #[test]
    fn fresh_validators_are_the_others_whose_latest_block_moved() {
        let previous = [
            justification(1, 10),
            justification(2, 20),
            justification(3, 30),
        ];
        let now = [
            justification(1, 11),
            justification(2, 20),
            justification(3, 31),
            justification(4, 40),
        ];
        assert_eq!(
            fresh_validators(&validator(1), &now, Some(&previous)),
            vec![validator(3), validator(4)]
        );
        assert_eq!(fresh_validators(&validator(1), &now, None), vec![
            validator(2),
            validator(3),
            validator(4)
        ]);
    }

    /// `f_breadth` by one multiplication per unit of the exponent.
    fn f_breadth_by_steps(breadth_ppm: u64, p_breadth: u32) -> u64 {
        let complement = PPM.saturating_sub(breadth_ppm) as u128;
        let mut power = PPM as u128;
        for _ in 0..p_breadth {
            power = power * complement / PPM as u128;
        }
        PPM.saturating_sub(power as u64)
    }

    /// The largest exponent the chain accepts is as cheap as a small one.
    #[test]
    fn the_breadth_factor_is_cheap_at_any_exponent() {
        let started = std::time::Instant::now();
        for breadth in [0, 1, 500_000, 999_999, PPM] {
            std::hint::black_box(f_breadth(
                std::hint::black_box(breadth),
                std::hint::black_box(u32::MAX),
            ));
        }
        assert!(started.elapsed() < std::time::Duration::from_millis(50));
    }

    /// At the shipped exponent and below, the factor is exactly the one taken
    /// step by step; above, squaring truncates at different points and moves
    /// it by no more than a part per million per multiplication.
    #[test]
    fn the_breadth_factor_matches_the_step_by_step_power() {
        for breadth in (0..=PPM).step_by(12_345).chain([1, 999_999, PPM]) {
            for p in [1u32, 2] {
                assert_eq!(f_breadth(breadth, p), f_breadth_by_steps(breadth, p));
            }
            for p in 3u32..=64 {
                let (fast, slow) = (f_breadth(breadth, p), f_breadth_by_steps(breadth, p));
                assert!(
                    fast.abs_diff(slow) <= p as u64,
                    "breadth {breadth}, p {p}: {fast} against {slow}"
                );
            }
        }
    }

    /// Two bonds at the largest token amount are credited exactly: breadth 1/2
    /// gives 0.75, recency saturates, and 630 ppm × 2 members makes the reward
    /// 945 ppm of the stake per bond unit's `1 / PPM`.
    #[test]
    fn a_committee_of_the_largest_bonds_is_credited_exactly() {
        let largest = shared::rust::token_amount::TokenAmount::max().into_bigint();
        let committee: Committee = [
            (validator(1), largest.clone()),
            (validator(2), largest.clone()),
        ]
        .into_iter()
        .collect();
        let parents = [parent(1, 9), parent(2, 9)];
        assert_eq!(
            compute_reward(
                &committee,
                &validator(1),
                &[validator(2)],
                &parents,
                10,
                &at_rate(630)
            ),
            largest * 945
        );
    }

    #[test]
    fn an_empty_committee_earns_nothing() {
        let params = at_rate(630);
        assert_eq!(
            compute_reward(
                &Committee::new(),
                &validator(1),
                &[],
                &[parent(1, 9)],
                10,
                &params
            ),
            zero()
        );
    }
}
