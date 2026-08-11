// See casper/src/main/scala/coop/rchain/casper/merging/DeployChainIndex.scala

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::sync::Arc;

use models::rust::block_hash::BlockHash;
use prost::bytes::Bytes;
use rspace_plus_plus::rspace::errors::HistoryError;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::history::history_repository::HistoryRepository;
use rspace_plus_plus::rspace::merger::event_log_index::EventLogIndex;
use rspace_plus_plus::rspace::merger::merging_logic::NumberChannelsDiff;
use rspace_plus_plus::rspace::merger::state_change::StateChange;
use shared::rust::hashable_set::HashableSet;

use super::deploy_index::DeployIndex;
use crate::rust::system_deploy::is_system_deploy_id;

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DeployIdWithCost {
    pub deploy_id: Bytes,
    pub cost: u64,
}

/** index of deploys depending on each other inside a single block (state transition) */
#[derive(Debug, Clone)]
pub struct DeployChainIndex {
    pub deploys_with_cost: HashableSet<DeployIdWithCost>,
    post_state_hash: Blake2b256Hash,
    pub user_event_log_index: EventLogIndex,
    pub system_event_log_index: EventLogIndex,
    pub event_log_index: EventLogIndex,
    pub state_changes: StateChange,
    pub effect_indices: BTreeSet<u32>,
    pub has_exact_state_witness: bool,
    pub exact_effect_changes: BTreeMap<u32, (StateChange, NumberChannelsDiff)>,
    // Source block identity. Allows the merge algorithm to identify chains
    // whose diffs were computed against a block that was subsequently rejected.
    pub source_block_hash: BlockHash,
    pub source_block_number: i64,
}

impl DeployChainIndex {
    pub fn new<C, P, A, K>(
        deploys: &HashableSet<DeployIndex>,
        pre_state_hash: &Blake2b256Hash,
        post_state_hash: &Blake2b256Hash,
        history_repository: Arc<Box<dyn HistoryRepository<C, P, A, K> + Send + Sync + 'static>>,
        source_block_hash: BlockHash,
        source_block_number: i64,
    ) -> Result<Self, HistoryError>
    where
        C: std::clone::Clone
            + serde::Serialize
            + for<'de> serde::Deserialize<'de>
            + Send
            + Sync
            + 'static,
        P: std::clone::Clone + for<'de> serde::Deserialize<'de> + Send + Sync + 'static,
        A: std::clone::Clone + for<'de> serde::Deserialize<'de> + Send + Sync + 'static,
        K: std::clone::Clone + for<'de> serde::Deserialize<'de> + Send + Sync + 'static,
    {
        let deploys_with_cost: HashSet<DeployIdWithCost> = deploys
            .0
            .iter()
            .map(|deploy| DeployIdWithCost {
                deploy_id: deploy.deploy_id.clone(),
                cost: deploy.cost,
            })
            .collect();

        let (user_event_log_index, system_event_log_index) = deploys.into_iter().try_fold(
            (EventLogIndex::empty(), EventLogIndex::empty()),
            |(user_acc, system_acc), deploy| -> Result<_, HistoryError> {
                if is_system_deploy_id(&deploy.deploy_id) {
                    Ok((
                        user_acc,
                        EventLogIndex::combine(&system_acc, &deploy.event_log_index)?,
                    ))
                } else {
                    Ok((
                        EventLogIndex::combine(&user_acc, &deploy.event_log_index)?,
                        system_acc,
                    ))
                }
            },
        )?;

        let event_log_index =
            EventLogIndex::combine(&user_event_log_index, &system_event_log_index)?;

        let effect_indices = deploys
            .0
            .iter()
            .map(|deploy| deploy.execution_index)
            .collect::<BTreeSet<_>>();
        let has_exact_state_witness = deploys
            .0
            .iter()
            .all(|deploy| deploy.state_changes.is_some());
        let exact_effect_changes = if has_exact_state_witness {
            let changes = deploys
                .0
                .iter()
                .map(|deploy| {
                    (
                        deploy.execution_index,
                        (
                            deploy
                                .state_changes
                                .clone()
                                .expect("exact state witness checked above")
                                .normalized(),
                            deploy.event_log_index.number_channels_data.clone(),
                        ),
                    )
                })
                .collect::<BTreeMap<_, _>>();
            if changes.len() != deploys.0.len() {
                return Err(HistoryError::MergeError(
                    "duplicate execution index in deploy chain".to_string(),
                ));
            }
            changes
        } else {
            BTreeMap::new()
        };
        let state_changes = if has_exact_state_witness {
            exact_effect_changes
                .values()
                .try_fold(StateChange::empty(), |acc, (change, _)| {
                    acc.additive_join(change.clone())
                })?
                .normalized()
        } else {
            if deploys
                .0
                .iter()
                .any(|deploy| deploy.state_changes.is_some())
            {
                return Err(HistoryError::MergeError(
                    "deploy chain mixes exact and legacy state changes".to_string(),
                ));
            }
            let pre_history_reader =
                history_repository.get_history_reader_struct(pre_state_hash)?;
            let post_history_reader =
                history_repository.get_history_reader_struct(post_state_hash)?;
            StateChange::new(pre_history_reader, post_history_reader, &event_log_index)?
        };

        Ok(Self {
            deploys_with_cost: HashableSet(deploys_with_cost),
            post_state_hash: post_state_hash.clone(),
            user_event_log_index,
            system_event_log_index,
            event_log_index,
            state_changes,
            effect_indices,
            has_exact_state_witness,
            exact_effect_changes,
            source_block_hash,
            source_block_number,
        })
    }

    /// Construct a DeployChainIndex directly from its parts (for testing).
    pub fn from_parts(
        deploys_with_cost: HashableSet<DeployIdWithCost>,
        post_state_hash: Blake2b256Hash,
        event_log_index: EventLogIndex,
        state_changes: StateChange,
        source_block_hash: BlockHash,
        source_block_number: i64,
    ) -> Self {
        let user_event_log_index = event_log_index;
        let system_event_log_index = EventLogIndex::empty();
        let event_log_index =
            EventLogIndex::combine(&user_event_log_index, &system_event_log_index)
                .expect("EventLogIndex::combine in DeployChainIndex::from_parts must not fail");
        DeployChainIndex {
            deploys_with_cost,
            post_state_hash,
            user_event_log_index,
            system_event_log_index,
            event_log_index,
            state_changes,
            effect_indices: BTreeSet::new(),
            has_exact_state_witness: false,
            exact_effect_changes: BTreeMap::new(),
            source_block_hash,
            source_block_number,
        }
    }
}

impl PartialEq for DeployChainIndex {
    fn eq(&self, other: &Self) -> bool {
        self.deploys_with_cost == other.deploys_with_cost
            && self.post_state_hash == other.post_state_hash
            && self.source_block_hash == other.source_block_hash
            && self.effect_indices == other.effect_indices
            && self.has_exact_state_witness == other.has_exact_state_witness
    }
}

impl std::hash::Hash for DeployChainIndex {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.deploys_with_cost.hash(state);
        self.post_state_hash.hash(state);
        self.source_block_hash.hash(state);
        self.effect_indices.hash(state);
        self.has_exact_state_witness.hash(state);
    }
}

impl Eq for DeployChainIndex {}

impl PartialOrd for DeployChainIndex {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> { Some(self.cmp(other)) }
}

impl Ord for DeployChainIndex {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // 1. PRIMARY: Highest total cost first (economic incentive)
        //    Higher-paying transactions get priority in conflict resolution
        let self_total_cost: u128 = self
            .deploys_with_cost
            .0
            .iter()
            .map(|d| u128::from(d.cost))
            .sum();
        let other_total_cost: u128 = other
            .deploys_with_cost
            .0
            .iter()
            .map(|d| u128::from(d.cost))
            .sum();

        let cost_cmp = self_total_cost.cmp(&other_total_cost).reverse(); // Higher cost first
        if cost_cmp != std::cmp::Ordering::Equal {
            return cost_cmp;
        }

        // 2. SECONDARY: Highest single deploy cost (prioritize high-value individual transactions)
        let self_max_cost = self
            .deploys_with_cost
            .0
            .iter()
            .map(|d| d.cost)
            .max()
            .unwrap_or(0);
        let other_max_cost = other
            .deploys_with_cost
            .0
            .iter()
            .map(|d| d.cost)
            .max()
            .unwrap_or(0);

        let max_cost_cmp = self_max_cost.cmp(&other_max_cost).reverse(); // Higher max cost first
        if max_cost_cmp != std::cmp::Ordering::Equal {
            return max_cost_cmp;
        }

        // 3. TERTIARY: Lexicographically smallest deploy signature (deterministic)
        //    This ensures consistent ordering across all nodes when costs are equal
        let self_min_deploy = self
            .deploys_with_cost
            .0
            .iter()
            .min_by(|a, b| a.deploy_id.cmp(&b.deploy_id));
        let other_min_deploy = other
            .deploys_with_cost
            .0
            .iter()
            .min_by(|a, b| a.deploy_id.cmp(&b.deploy_id));

        let signature_cmp = match (self_min_deploy, other_min_deploy) {
            (Some(self_deploy), Some(other_deploy)) => {
                self_deploy.deploy_id.cmp(&other_deploy.deploy_id)
            }
            (Some(_), None) => std::cmp::Ordering::Greater,
            (None, Some(_)) => std::cmp::Ordering::Less,
            (None, None) => std::cmp::Ordering::Equal,
        };

        if signature_cmp != std::cmp::Ordering::Equal {
            return signature_cmp;
        }

        // 4. QUATERNARY: Post-state hash. KEPT (not replaced) so the ordering on
        //    every input where the pre-existing 4-key comparator is already
        //    deterministic stays byte-identical — the fix is APPEND-ONLY, hence
        //    safe against live v0.4.16/master nodes under a rolling upgrade (it
        //    changes only the resolution of currently-non-deterministic 4-key
        //    ties, whose order is already node-dependent via the reseeded HashSet).
        let post_state_cmp = self.post_state_hash.cmp(&other.post_state_hash);
        if post_state_cmp != std::cmp::Ordering::Equal {
            return post_state_cmp;
        }

        let deploy_set_cmp = self.deploys_with_cost.cmp(&other.deploys_with_cost);
        if deploy_set_cmp != std::cmp::Ordering::Equal {
            return deploy_set_cmp;
        }

        let source_cmp = self.source_block_hash.cmp(&other.source_block_hash);
        if source_cmp != std::cmp::Ordering::Equal {
            return source_cmp;
        }

        let effect_cmp = self.effect_indices.cmp(&other.effect_indices);
        if effect_cmp != std::cmp::Ordering::Equal {
            return effect_cmp;
        }

        self.has_exact_state_witness
            .cmp(&other.has_exact_state_witness)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::hash_map::DefaultHasher;
    use std::collections::HashSet;
    use std::hash::{Hash, Hasher};

    use proptest::prelude::*;
    use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;

    use super::*;

    fn mk_index(deploys: &[(u8, u64)], post_state_seed: u8) -> DeployChainIndex {
        let deploys_with_cost: HashSet<DeployIdWithCost> = deploys
            .iter()
            .map(|(id, cost)| DeployIdWithCost {
                deploy_id: Bytes::from(vec![*id]),
                cost: *cost,
            })
            .collect();

        DeployChainIndex {
            deploys_with_cost: HashableSet(deploys_with_cost),
            post_state_hash: Blake2b256Hash::from_bytes(vec![post_state_seed; 32]),
            user_event_log_index: EventLogIndex::empty(),
            system_event_log_index: EventLogIndex::empty(),
            event_log_index: EventLogIndex::empty(),
            state_changes: StateChange::empty(),
            effect_indices: BTreeSet::new(),
            has_exact_state_witness: false,
            exact_effect_changes: BTreeMap::new(),
            source_block_hash: Bytes::from(vec![post_state_seed; 32]),
            source_block_number: 0,
        }
    }

    fn hash(index: &DeployChainIndex) -> u64 {
        let mut hasher = DefaultHasher::new();
        index.hash(&mut hasher);
        hasher.finish()
    }

    #[test]
    fn ordering_prefers_higher_total_cost() {
        let high_total = mk_index(&[(1, 10), (2, 1)], 1); // total = 11
        let low_total = mk_index(&[(1, 9), (2, 1)], 2); // total = 10

        assert_eq!(high_total.cmp(&low_total), std::cmp::Ordering::Less);
        assert_eq!(low_total.cmp(&high_total), std::cmp::Ordering::Greater);
    }

    #[test]
    fn ordering_tie_breaks_on_max_cost_then_signature() {
        // Same total (11), different max (7 vs 6)
        let max_seven = mk_index(&[(1, 7), (2, 4)], 1);
        let max_six = mk_index(&[(1, 6), (2, 5)], 2);
        assert_eq!(max_seven.cmp(&max_six), std::cmp::Ordering::Less);

        // Same total/max, tie-break by smallest deploy signature (2 < 3)
        let min_sig_two = mk_index(&[(2, 5), (9, 5)], 1);
        let min_sig_three = mk_index(&[(3, 5), (9, 5)], 2);
        assert_eq!(min_sig_two.cmp(&min_sig_three), std::cmp::Ordering::Less);
    }

    #[test]
    fn ordering_final_tie_breaks_on_post_state_hash() {
        let a = mk_index(&[(1, 5)], 0x01);
        let b = mk_index(&[(1, 5)], 0x02);

        assert_eq!(a.cmp(&b), std::cmp::Ordering::Less);
        assert_eq!(b.cmp(&a), std::cmp::Ordering::Greater);
    }

    #[test]
    fn source_distinguishes_identical_deploy_occurrences() {
        let a = mk_index(&[(1, 5)], 0x01);
        let mut b = a.clone();
        b.source_block_hash = Bytes::from(vec![0x02; 32]);

        assert_ne!(a, b);
        assert_ne!(a.cmp(&b), std::cmp::Ordering::Equal);
        assert_ne!(hash(&a), hash(&b));
    }

    #[test]
    fn total_cost_comparison_does_not_overflow() {
        let larger = mk_index(&[(1, u64::MAX), (2, u64::MAX)], 0x01);
        let smaller = mk_index(&[(1, u64::MAX), (2, 1)], 0x02);

        assert_eq!(larger.cmp(&smaller), std::cmp::Ordering::Less);
    }

    // Determinism regression (Finding B): two DISTINCT chains that tie on ALL FOUR
    // policy keys — Σcost (10), max single cost (5), min deploy_id ([1]), AND
    // post_state_hash (7) — but carry different deploy sets. Before the injective
    // 5th tie-break key, `cmp` returned `Equal` for these DISTINCT chains, so the
    // `min_by`/`sort` winner was decided by the reseeded `HashSet` iteration order —
    // a non-deterministic rejected-set that the recomputing validator rejects (fork).
    // The 5th key (the Eq set-order via `HashableSet<DeployIdWithCost>: Ord`) must
    // break the tie deterministically. (RED without the append; GREEN with it.)
    #[test]
    fn distinct_chains_tying_on_all_four_policy_keys_still_order_deterministically() {
        let a = mk_index(&[(1, 5), (3, 5)], 7);
        let b = mk_index(&[(1, 5), (4, 5)], 7);
        assert_ne!(
            a, b,
            "the two chains are genuinely distinct (different deploys)"
        );
        assert_ne!(
            a.cmp(&b),
            std::cmp::Ordering::Equal,
            "distinct chains must never tie under cmp (else min_by/sort is HashSet-order-dependent => fork)"
        );
        assert_eq!(
            a.cmp(&b),
            b.cmp(&a).reverse(),
            "cmp must be antisymmetric on the tie"
        );
    }

    #[test]
    fn distinct_chains_tying_on_policy_keys_still_order_deterministically() {
        let a = mk_index(&[(1, 10), (2, 10)], 0x01);
        let b = mk_index(&[(1, 10), (3, 10)], 0x01);

        assert_ne!(a, b);
        assert_ne!(a.cmp(&b), std::cmp::Ordering::Equal);
        assert_eq!(a.cmp(&b), b.cmp(&a).reverse());
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(400))]
        // `DeployChainIndex::cmp` is a STRICT TOTAL ORDER whose `Equal`-class is
        // contained in `Eq` (`cmp(a,b) == Equal => a == b`) — the property that makes
        // the merge's `min_by`/`sort` winner node-identical regardless of `HashSet`
        // reseeding (the no-fork guarantee; the Rust modality companion to the Rocq
        // `merge_algebra::KeepOneOrder` proof). Over arbitrary small deploy-chain sets:
        // irreflexivity, antisymmetry, transitivity, and injectivity of the Equal-class.
        #[test]
        fn cmp_is_strict_total_order_injective_on_equal(
            specs in prop::collection::vec(
                (prop::collection::vec((0u8..8, 1u64..6), 1..4), 0u8..4),
                2..6),
        ) {
            let chains: Vec<DeployChainIndex> =
                specs.iter().map(|(d, seed)| mk_index(d, *seed)).collect();
            use std::cmp::Ordering::{Equal, Greater};
            for a in &chains {
                prop_assert!(a.cmp(a) == Equal, "irreflexive: cmp(a,a) must be Equal");
                for b in &chains {
                    prop_assert!(a.cmp(b) == b.cmp(a).reverse(), "antisymmetric");
                    if a.cmp(b) == Equal {
                        prop_assert!(a == b, "cmp==Equal must imply Eq (no distinct ties => no fork)");
                    }
                    for c in &chains {
                        if a.cmp(b) != Greater && b.cmp(c) != Greater {
                            prop_assert!(a.cmp(c) != Greater, "transitive: a<=b<=c => a<=c");
                        }
                    }
                }
            }
        }
    }
}
