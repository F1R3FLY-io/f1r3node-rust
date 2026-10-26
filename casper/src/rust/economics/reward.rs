use std::collections::{BTreeMap, HashSet};
use std::num::NonZeroU64;

use models::rust::casper::protocol::casper_message::Bond;
use models::rust::validator::Validator;

/// Fixed-point scale for every reward factor, and the scale of the reward
/// itself. The arithmetic is integer throughout because rewards are
/// recomputed on other nodes from the same public inputs and the results are
/// compared, so they must agree bit for bit — `f64::powf` carries no such
/// guarantee across platforms.
pub const PPM: u64 = 1_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RewardParams {
    /// The base reward is one epoch's emission over its blocks and over the
    /// block committee's members, in bond units.
    pub epoch_phlogiston: u64,
    pub epoch_length: NonZeroU64,
    /// Exponent of `f_breadth`. Zero makes the breadth factor identically
    /// zero, and therefore the reward zero — a misconfiguration that is
    /// reported as no reward rather than silently treated as one.
    pub p_breadth: u32,
    pub y_ref_ppm: u64,
}

/// What the reward needs from one parent: who authored it and how far below
/// the child it sits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParentFact {
    pub sender: Validator,
    pub block_number: i64,
}

/// The block's committee, each validator once with its stake.
pub type Committee = BTreeMap<Validator, i64>;

/// `bonds` as a committee.
pub fn committee_of(bonds: &[Bond]) -> Committee {
    bonds
        .iter()
        .map(|bond| (bond.validator.clone(), bond.stake))
        .collect()
}

/// The base reward scaled by how much of the committee the block builds on
/// (breadth) and how recent those parents are (recency). The committee is the
/// block's own.
///
/// Breadth is the share of the block's committee stake its parents were
/// authored by, summed over DISTINCT senders so citing one sender twice
/// cannot push the share past the whole committee. A parent whose sender is
/// absent from the committee adds nothing: the measure is committee stake
/// built upon, and a departed sender holds none of it.
///
/// The result is in units of `1 / PPM` of a bond unit. Both factors are
/// fractions, so a reward expressed in whole bond units would round to
/// nothing for every block that does not saturate both of them.
pub fn compute_reward(
    committee: &Committee,
    parents: &[ParentFact],
    block_number: i64,
    params: &RewardParams,
) -> u128 {
    // Summed wider than a stake: two bonds at the largest permitted value
    // already overflow `i64`.
    let total: i128 = committee.values().map(|stake| *stake as i128).sum();
    let members = committee.values().filter(|stake| **stake > 0).count() as u128;
    if total <= 0 || members == 0 {
        return 0;
    }

    let mut counted: HashSet<&Validator> = HashSet::new();
    let mut cited_stake: i128 = 0;
    let mut recency_ppm: u64 = 0;
    for parent in parents {
        if let Some(stake) = committee.get(&parent.sender) {
            if counted.insert(&parent.sender) {
                cited_stake += *stake as i128;
            }
        }
        let gap = block_number.saturating_sub(parent.block_number).max(0) as u64;
        recency_ppm = recency_ppm.saturating_add(PPM / (1 + gap));
    }

    let breadth_ppm = (cited_stake.max(0) as u128 * PPM as u128 / total as u128) as u64;
    let bill = f_breadth(breadth_ppm, params.p_breadth) as u128
        * f_recency(recency_ppm, params.y_ref_ppm) as u128
        * params.epoch_phlogiston as u128;
    bill / (PPM as u128 * params.epoch_length.get() as u128 * members)
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

    /// The shipped factors, with a base reward of one bond unit a block for a
    /// committee of `members`.
    fn one_a_block(members: u64) -> RewardParams {
        RewardParams {
            epoch_phlogiston: members,
            epoch_length: NonZeroU64::MIN,
            p_breadth: 2,
            y_ref_ppm: PPM,
        }
    }

    fn bonds(stakes: &[(u8, i64)]) -> Committee {
        stakes
            .iter()
            .map(|(byte, stake)| (validator(*byte), *stake))
            .collect()
    }

    fn parent(byte: u8, block_number: i64) -> ParentFact {
        ParentFact {
            sender: validator(byte),
            block_number,
        }
    }

    /// Citing only your own block pays strictly less than citing everyone,
    /// which is the whole point of the breadth factor. It holds only because
    /// breadth is a share of TOTAL committee stake: normalizing over the
    /// parent set instead would make it 1 for every block, including this
    /// one, and the factor would stop discriminating.
    #[test]
    fn breadth_pays_a_self_only_proposer_less_than_a_full_parent_set() {
        let committee = bonds(&[(1, 250), (2, 250), (3, 250), (4, 250)]);
        let params = one_a_block(4);

        let self_only = compute_reward(&committee, &[parent(1, 9)], 10, &params);
        let everyone = compute_reward(
            &committee,
            &[parent(1, 9), parent(2, 9), parent(3, 9), parent(4, 9)],
            10,
            &params,
        );

        // w_self = 1/4, p = 2 → breadth factor 1 − (3/4)² = 7/16, and one
        // parent one height back gives recency 1/2, which `y_ref = 1` does
        // not cap. Four parents sum to recency 2 and saturate it. At a base
        // reward of 1 those are 0.21875 and 1.0 of a bond unit.
        assert_eq!(self_only, 218_750);
        assert_eq!(everyone, PPM as u128);

        // And it rises with every additional share cited, not just at the
        // endpoints.
        let mut previous = 0;
        for cited in 1..=4u8 {
            let parents: Vec<ParentFact> = (1..=cited).map(|byte| parent(byte, 9)).collect();
            let reward = compute_reward(&committee, &parents, 10, &params);
            assert!(
                reward > previous,
                "citing {cited} of 4 paid {reward}, no more than citing one fewer \
                 ({previous})"
            );
            previous = reward;
        }
    }

    /// Recency is capped by `y_ref`, and a staler parent set earns no more
    /// than a fresher one.
    #[test]
    fn recency_is_monotone_in_freshness() {
        let committee = bonds(&[(1, 500), (2, 500)]);
        let params = RewardParams {
            y_ref_ppm: 4 * PPM,
            ..one_a_block(2)
        };
        let fresh = compute_reward(&committee, &[parent(1, 9), parent(2, 9)], 10, &params);
        let stale = compute_reward(&committee, &[parent(1, 1), parent(2, 1)], 10, &params);
        assert!(
            fresh > stale,
            "a fresher parent set must earn more: fresh {fresh}, stale {stale}"
        );
    }

    /// Isolates the breadth factor: a tiny `y_ref` saturates recency, so any
    /// difference between two parent sets is breadth alone.
    fn breadth_only_params() -> RewardParams {
        RewardParams {
            y_ref_ppm: 1,
            ..one_a_block(2)
        }
    }

    /// A parent authored by a validator that has left the committee adds no
    /// breadth — it holds none of the stake the measure is over.
    #[test]
    fn a_departed_parent_sender_contributes_no_breadth() {
        let committee = bonds(&[(1, 500), (2, 500)]);
        let params = breadth_only_params();
        let in_committee = compute_reward(&committee, &[parent(1, 9)], 10, &params);
        let with_departed = compute_reward(&committee, &[parent(1, 9), parent(9, 9)], 10, &params);
        assert_eq!(in_committee, 750_000, "breadth 1/2, p = 2 → 1 − (1/2)²");
        assert_eq!(in_committee, with_departed);
    }

    /// Two parents from the same sender are one share, so breadth cannot be
    /// inflated past the committee by citing a sender twice.
    #[test]
    fn a_repeated_parent_sender_counts_once() {
        let committee = bonds(&[(1, 500), (2, 500)]);
        let params = breadth_only_params();
        let once = compute_reward(&committee, &[parent(1, 9)], 10, &params);
        let twice = compute_reward(&committee, &[parent(1, 9), parent(1, 8)], 10, &params);
        assert_eq!(once, twice);
    }

    /// The base reward is the epoch's emission over its blocks and over the
    /// committee's members, so a block citing the whole committee earns
    /// `1_000_000 / 10_000 / 4` at the shipped emission and epoch, and twice
    /// that with half the members. A member without stake is no member.
    #[test]
    fn the_base_reward_is_the_epoch_emission_over_its_blocks_and_members() {
        let params = RewardParams {
            epoch_phlogiston: 1_000_000,
            epoch_length: NonZeroU64::new(10_000).expect("positive"),
            ..one_a_block(1)
        };
        let everyone = |stakes: &[(u8, i64)]| {
            let committee = bonds(stakes);
            let parents: Vec<ParentFact> =
                stakes.iter().map(|(byte, _)| parent(*byte, 9)).collect();
            compute_reward(&committee, &parents, 10, &params)
        };

        assert_eq!(
            everyone(&[(1, 250), (2, 250), (3, 250), (4, 250)]),
            25 * PPM as u128
        );
        assert_eq!(everyone(&[(1, 250), (2, 250)]), 50 * PPM as u128);
        assert_eq!(
            everyone(&[(1, 250), (2, 250), (3, 0)]),
            50 * PPM as u128,
            "a zero-stake key is not counted"
        );
        let committee = bonds(&[(1, 250), (2, 250), (3, 250), (4, 250)]);
        assert_eq!(
            compute_reward(&committee, &[parent(1, 9)], 10, &params),
            5_468_750,
            "a fraction of a bond unit is kept, not rounded away"
        );
    }

    /// A committee holds each validator once, so a bonds list naming one twice
    /// pays what its single entry pays.
    #[test]
    fn a_validator_listed_twice_is_one_member() {
        let bond = |byte: u8| Bond {
            validator: validator(byte),
            stake: 250,
        };
        let parents = [parent(1, 9), parent(2, 9)];
        let once = committee_of(&[bond(1), bond(2)]);
        let twice = committee_of(&[bond(1), bond(2), bond(2)]);
        assert_eq!(
            compute_reward(&twice, &parents, 10, &one_a_block(2)),
            compute_reward(&once, &parents, 10, &one_a_block(2))
        );
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

    /// Two bonds at the largest value genesis permits still form a committee
    /// whose whole stake a block can cite.
    #[test]
    fn a_committee_of_the_largest_bonds_pays_in_full() {
        let committee = bonds(&[(1, i64::MAX), (2, i64::MAX)]);
        let everyone = [parent(1, 9), parent(2, 9)];
        assert_eq!(
            compute_reward(&committee, &everyone, 10, &one_a_block(2)),
            PPM as u128
        );
    }

    #[test]
    fn an_empty_committee_earns_nothing() {
        let params = one_a_block(1);
        assert_eq!(
            compute_reward(&Committee::new(), &[parent(1, 9)], 10, &params),
            0
        );
        assert_eq!(
            compute_reward(&bonds(&[(1, 0)]), &[parent(1, 9)], 10, &params),
            0
        );
    }
}
