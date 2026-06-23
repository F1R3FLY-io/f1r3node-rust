// See rspace/src/main/scala/coop/rchain/rspace/merger/StateChangeMerger.scala

use shared::rust::ByteVector;

use super::channel_change::ChannelChange;
use super::merging_logic::NumberChannelsDiff;
use super::state_change::StateChange;
use crate::rspace::errors::HistoryError;
use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use crate::rspace::hashing::stable_hash_provider;
use crate::rspace::history::history_reader::HistoryReader;
use crate::rspace::hot_store_trie_action::{
    HotStoreTrieAction, TrieDeleteAction, TrieDeleteConsume, TrieDeleteJoins, TrieDeleteProduce,
    TrieInsertAction, TrieInsertBinaryConsume, TrieInsertBinaryJoins, TrieInsertBinaryProduce,
};

/**
 * This classes are used to compute joins.
 * Consume value pointer that stores continuations on some channel is
 * identified by channels involved in. Therefore when no continuations on
 * some consume is left and the whole consume ponter is removed -
 * no joins with corresponding seq of channels exist in tuple space. So join
 * should be removed.
 */
pub enum JoinActionKind {
    AddJoin(Vec<Blake2b256Hash>),
    RemoveJoin(Vec<Blake2b256Hash>),
}

pub struct ConsumeAndJoinActions<C: Clone, P: Clone, A: Clone, K: Clone> {
    consume_action: HotStoreTrieAction<C, P, A, K>,
    join_action: Option<JoinActionKind>,
}

pub fn compute_trie_actions<C: Clone, P: Clone, A: Clone, K: Clone>(
    changes: &StateChange,
    base_reader: &Box<dyn HistoryReader<Blake2b256Hash, C, P, A, K>>,
    mergeable_chs: &NumberChannelsDiff,
    handle_channel_change: impl Fn(
        &Blake2b256Hash,
        &ChannelChange<Vec<u8>>,
        &NumberChannelsDiff,
    ) -> Result<Option<HotStoreTrieAction<C, P, A, K>>, HistoryError>,
) -> Result<Vec<HotStoreTrieAction<C, P, A, K>>, HistoryError> {
    tracing::debug!(
        target: "f1r3fly.merge.dag",
        datums_channels = changes.datums_changes.len(),
        cont_channels = changes.cont_changes.len(),
        joins_channels = changes.consume_channels_to_join_serialized_map.len(),
        mergeable_chs = mergeable_chs.len(),
        "compute-trie-actions entry"
    );

    // Sort continuation changes by hash of consume channels for deterministic
    // ordering
    let mut cont_changes_sorted: Vec<_> = changes
        .cont_changes
        .iter()
        .map(|ref_multi| (ref_multi.key().clone(), ref_multi.value().clone()))
        .collect();
    cont_changes_sorted.sort_by_key(|(consume_channels, _)| {
        stable_hash_provider::hash_from_hashes(consume_channels)
    });

    let consume_with_join_actions: Vec<ConsumeAndJoinActions<C, P, A, K>> = cont_changes_sorted
        .iter()
        .map(|(consume_channels, channel_change)| {
            // Use hash_from_hashes to match EXEC path's hash_from_vec behavior:
            // The EXEC path uses hash_from_vec(&channels) which serializes each channel,
            // hashes each, sorts, concatenates, and hashes again.
            // Since consume_channels here is already Vec<Blake2b256Hash>, we use
            // hash_from_hashes which sorts, concatenates, and hashes - matching
            // the EXEC behavior.
            let history_pointer = stable_hash_provider::hash_from_hashes(consume_channels);
            let init = base_reader.get_continuations_proj_binary(&history_pointer)?;

            let new_val = {
                // Use multiset diff: remove each item in 'removed' exactly once from 'init'
                let mut result = StateChange::multiset_diff(&init, &channel_change.removed);
                result.extend(channel_change.added.clone());
                result
            };

            if init == new_val {
                Err(HistoryError::MergeError(
                    "Merging logic error: empty consume change when computing trie action."
                        .to_string(),
                ))
            } else if init.is_empty() {
                // No konts were in base state and some are added - insert konts and add join.
                Ok(ConsumeAndJoinActions {
                    consume_action: HotStoreTrieAction::TrieInsertAction(
                        TrieInsertAction::TrieInsertBinaryConsume(TrieInsertBinaryConsume {
                            hash: history_pointer,
                            continuations: new_val,
                        }),
                    ),
                    join_action: Some(JoinActionKind::AddJoin(consume_channels.clone())),
                })
            } else if new_val.is_empty() {
                // All konts present in base are removed - remove consume, remove join.
                Ok(ConsumeAndJoinActions {
                    consume_action: HotStoreTrieAction::TrieDeleteAction(
                        TrieDeleteAction::TrieDeleteConsume(TrieDeleteConsume {
                            hash: history_pointer,
                        }),
                    ),
                    join_action: Some(JoinActionKind::RemoveJoin(consume_channels.clone())),
                })
            } else {
                // Konts were updated but consume is present in base state - update konts, no
                // joins.
                Ok(ConsumeAndJoinActions {
                    consume_action: HotStoreTrieAction::TrieInsertAction(
                        TrieInsertAction::TrieInsertBinaryConsume(TrieInsertBinaryConsume {
                            hash: history_pointer,
                            continuations: new_val,
                        }),
                    ),
                    join_action: None,
                })
            }
        })
        .collect::<Result<Vec<ConsumeAndJoinActions<C, P, A, K>>, HistoryError>>()?;

    let consume_trie_actions = consume_with_join_actions
        .iter()
        .map(|consume_and_join_action| consume_and_join_action.consume_action.clone())
        .collect::<Vec<_>>();

    // Sort datum changes by history pointer for deterministic ordering
    let mut datums_changes_sorted: Vec<_> = changes
        .datums_changes
        .iter()
        .map(|ref_multi| (ref_multi.key().clone(), ref_multi.value().clone()))
        .collect();
    datums_changes_sorted.sort_by_key(|(history_pointer, _)| history_pointer.clone());

    let produce_trie_actions = datums_changes_sorted
        .iter()
        .map(|(history_pointer, changes)| {
            handle_channel_change(history_pointer, changes, mergeable_chs).and_then(|action| {
                action.map(Ok).unwrap_or_else(|| {
                    make_trie_action(
                        history_pointer,
                        |hash| base_reader.get_data_proj_binary(hash),
                        changes,
                        |hash| {
                            HotStoreTrieAction::TrieDeleteAction(
                                TrieDeleteAction::TrieDeleteProduce(TrieDeleteProduce {
                                    hash: hash.clone(),
                                }),
                            )
                        },
                        |hash, data| {
                            HotStoreTrieAction::TrieInsertAction(
                                TrieInsertAction::TrieInsertBinaryProduce(
                                    TrieInsertBinaryProduce {
                                        hash: hash.clone(),
                                        data,
                                    },
                                ),
                            )
                        },
                        // datums path: enforce the single-value-cell stale-consume backstop
                        true,
                    )
                })
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    // Process joins changes
    let joins_channels_to_body_map = &changes.consume_channels_to_join_serialized_map;
    let mut joins_changes = std::collections::HashMap::new();

    // Collect join changes from consume actions
    for consume_and_join_action in &consume_with_join_actions {
        if let Some(join_action) = &consume_and_join_action.join_action {
            let join_channels = match join_action {
                JoinActionKind::AddJoin(chs) => chs,
                JoinActionKind::RemoveJoin(chs) => chs,
            };

            // Get the serialized join data for these channels
            if let Some(join_data) = joins_channels_to_body_map.get(join_channels) {
                // Update the joins_changes for each channel
                for channel in join_channels {
                    let current_val = joins_changes
                        .entry(channel.clone())
                        .or_insert_with(ChannelChange::empty);

                    match join_action {
                        JoinActionKind::AddJoin(_) => {
                            current_val.added.push(join_data.clone());
                        }
                        JoinActionKind::RemoveJoin(_) => {
                            current_val.removed.push(join_data.clone());
                        }
                    }
                }
            } else {
                return Err(HistoryError::MergeError(
                    "No ByteVector value for join found when merging when computing trie action."
                        .to_string(),
                ));
            }
        }
    }

    // Sort joins changes by history pointer for deterministic ordering
    let mut joins_changes_sorted: Vec<_> = joins_changes.into_iter().collect();
    joins_changes_sorted.sort_by_key(|(history_pointer, _)| history_pointer.clone());

    let joins_trie_actions = joins_changes_sorted
        .iter()
        .map(|(history_pointer, changes)| {
            make_trie_action(
                history_pointer,
                |hash| base_reader.get_joins_proj_binary(hash),
                changes,
                |hash| {
                    HotStoreTrieAction::TrieDeleteAction(TrieDeleteAction::TrieDeleteJoins(
                        TrieDeleteJoins { hash: hash.clone() },
                    ))
                },
                |hash, joins| {
                    HotStoreTrieAction::TrieInsertAction(TrieInsertAction::TrieInsertBinaryJoins(
                        TrieInsertBinaryJoins {
                            hash: hash.clone(),
                            joins,
                        },
                    ))
                },
                // joins path: the stale-consume backstop is single-value-cell
                // specific (datums); joins remove/add legitimately track consume
                // lifecycle, so it does not apply here.
                false,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;

    // Combine all trie actions
    let mut result = Vec::new();
    result.extend(produce_trie_actions);
    result.extend(consume_trie_actions);
    result.extend(joins_trie_actions);

    Ok(result)
}

fn make_trie_action<C: Clone, P: Clone, A: Clone, K: Clone>(
    history_pointer: &Blake2b256Hash,
    init_value: impl Fn(&Blake2b256Hash) -> Result<Vec<ByteVector>, HistoryError>,
    changes: &ChannelChange<ByteVector>,
    remove_action: impl Fn(&Blake2b256Hash) -> HotStoreTrieAction<C, P, A, K>,
    update_action: impl Fn(&Blake2b256Hash, Vec<ByteVector>) -> HotStoreTrieAction<C, P, A, K>,
    reject_stale_removes: bool,
) -> Result<HotStoreTrieAction<C, P, A, K>, HistoryError> {
    let init = init_value(history_pointer)?;

    // Single-value-cell stale-consume backstop (datums path only). A `removed`
    // datum that is neither present in the base nor re-added by this combined
    // change is a stale consume: a chain whose diff was computed against a base
    // it did not execute on (a sibling already rewrote the cell). Base-first
    // composition would silently leave the un-removed base value alongside the
    // new one — a single-value cell going multi-value. Such chains are rejected
    // upstream in DagMerger; reaching here means one slipped past, so fail the
    // merge loudly rather than corrupt the cell. The `!added.contains` clause
    // exempts an in-branch produced-then-consumed datum (it lives in both
    // `added` and `removed` after ChannelChange::combine).
    if reject_stale_removes {
        if let Some(stale) = changes
            .removed
            .iter()
            .find(|d| !init.contains(d) && !changes.added.contains(d))
        {
            return Err(HistoryError::MergeError(format!(
                "stale-consume on channel {}: removed datum {} absent from base and from added; \
                 chain rebased onto a divergent base (single-value-cell race) — must be rejected \
                 upstream, not composed",
                hex::encode(history_pointer.bytes()),
                hex::encode(&stale[..std::cmp::min(8, stale.len())]),
            )));
        }
    }

    // Compose the channel's new value. The two multiset orderings differ only
    // when a `removed` datum was PRODUCED within the merge set (not in the base):
    //
    //   datums path  (init ++ added) -- removed  — pool form. A datum a chain
    //     produces and a serialized successor consumes cancels, so a kept linear
    //     write path on a single-value cell collapses to one value. Concurrent
    //     (fork) writers are serialized upstream in DagMerger — one path kept,
    //     the rest rejected to recovery — so only a single path reaches here.
    //   joins path   (init -- removed) ++ added  — base-first, unchanged. Join
    //     add/remove track consume lifecycle against the base, not in-set
    //     produce/consume cancellation.
    //
    // The two forms are identical whenever `removed` is a sub-multiset of `init`
    // (the common case), so this changes behavior only for the in-set-produce
    // case the datums path needs. The stale-consume backstop above still rejects
    // a `removed` datum absent from BOTH init and added.
    let new_val = if reject_stale_removes {
        let mut pooled = init.clone();
        pooled.extend(changes.added.clone());
        StateChange::multiset_diff(&pooled, &changes.removed)
    } else {
        let mut result = StateChange::multiset_diff(&init, &changes.removed);
        result.extend(changes.added.clone());
        result
    };

    tracing::trace!(
        target: "f1r3fly.merge.dag",
        channel = %hex::encode(history_pointer.bytes()),
        init_len = init.len(),
        added_len = changes.added.len(),
        removed_len = changes.removed.len(),
        new_val_len = new_val.len(),
        "make-trie-action"
    );
    if new_val.len() > 1
        && tracing::enabled!(target: "f1r3fly.rspace.multidatum", tracing::Level::DEBUG)
    {
        let init_hashes: Vec<String> = init.iter().map(hex::encode).collect();
        let added_hashes: Vec<String> = changes.added.iter().map(hex::encode).collect();
        let removed_hashes: Vec<String> = changes.removed.iter().map(hex::encode).collect();
        let new_val_hashes: Vec<String> = new_val.iter().map(hex::encode).collect();
        tracing::debug!(
            target: "f1r3fly.rspace.multidatum",
            site = "merge",
            channel = %hex::encode(history_pointer.bytes()),
            new_val_len = new_val.len(),
            init_hashes = ?init_hashes,
            added_hashes = ?added_hashes,
            removed_hashes = ?removed_hashes,
            new_val_hashes = ?new_val_hashes,
            "merge produced multi-datum channel value"
        );
    }

    if new_val.is_empty() && !init.is_empty() {
        // Case 1: All items present in base are removed - remove action
        Ok(remove_action(history_pointer))
    } else if init != new_val {
        // Case 2: Items were updated - update action
        Ok(update_action(history_pointer, new_val))
    } else {
        // Case 3: the composed changes cancel out exactly (a chained write
        // sequence that returns the channel to its base value). Emit an
        // idempotent update — writing the identical value is a no-op on the
        // trie — rather than failing the whole merge.
        tracing::debug!(
            target: "f1r3fly.merge.dag",
            channel = %hex::encode(history_pointer.bytes()),
            "channel changes cancel to the base value; idempotent update"
        );
        Ok(update_action(history_pointer, new_val))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, HashMap};

    use dashmap::DashMap;

    use super::*;
    use crate::rspace::history::history_reader::HistoryReaderBase;
    use crate::rspace::internal::{Datum, WaitingContinuation};

    struct StubHistoryReaderBinary {
        data_map: HashMap<Blake2b256Hash, Vec<Vec<u8>>>,
    }

    struct EmptyHistoryReaderBase;

    impl HistoryReaderBase<(), (), (), ()> for EmptyHistoryReaderBase {
        fn get_data_proj(&self, _key: &()) -> Vec<Datum<()>> { vec![] }

        fn get_continuations_proj(&self, _key: &Vec<()>) -> Vec<WaitingContinuation<(), ()>> {
            vec![]
        }

        fn get_joins_proj(&self, _key: &()) -> Vec<Vec<()>> { vec![] }
    }

    impl HistoryReader<Blake2b256Hash, (), (), (), ()> for StubHistoryReaderBinary {
        fn root(&self) -> Blake2b256Hash { Blake2b256Hash::from_bytes(vec![0xff; 32]) }

        fn get_data_proj(&self, _key: &Blake2b256Hash) -> Result<Vec<Datum<()>>, HistoryError> {
            Ok(vec![])
        }

        fn get_data_proj_binary(&self, key: &Blake2b256Hash) -> Result<Vec<Vec<u8>>, HistoryError> {
            Ok(self.data_map.get(key).cloned().unwrap_or_default())
        }

        fn get_continuations_proj(
            &self,
            _key: &Blake2b256Hash,
        ) -> Result<Vec<WaitingContinuation<(), ()>>, HistoryError> {
            Ok(vec![])
        }

        fn get_continuations_proj_binary(
            &self,
            _key: &Blake2b256Hash,
        ) -> Result<Vec<Vec<u8>>, HistoryError> {
            Ok(vec![])
        }

        fn get_joins_proj(&self, _key: &Blake2b256Hash) -> Result<Vec<Vec<()>>, HistoryError> {
            Ok(vec![])
        }

        fn get_joins_proj_binary(
            &self,
            _key: &Blake2b256Hash,
        ) -> Result<Vec<Vec<u8>>, HistoryError> {
            Ok(vec![])
        }

        fn base(&self) -> Box<dyn HistoryReaderBase<(), (), (), ()>> {
            Box::new(EmptyHistoryReaderBase)
        }

        fn get_data_proj_generic(&self, _key: &()) -> Vec<Datum<()>> { vec![] }

        fn get_continuations_proj_generic(
            &self,
            _key: &Vec<()>,
        ) -> Vec<WaitingContinuation<(), ()>> {
            vec![]
        }

        fn get_joins_proj_generic(&self, _key: &()) -> Vec<Vec<()>> { vec![] }
    }

    /// Reproduces the ChannelChange.combine() duplication bug end-to-end
    /// through compute_trie_actions.
    #[test]
    fn compute_trie_actions_should_not_duplicate_data_when_merging_identical_sibling_changes() {
        let datum_a: Vec<u8> = vec![0xaa; 32];
        let datum_b: Vec<u8> = vec![0xbb; 32];
        let channel_hash = Blake2b256Hash::from_bytes(vec![0x01; 32]);

        let base_reader: Box<dyn HistoryReader<Blake2b256Hash, (), (), (), ()>> =
            Box::new(StubHistoryReaderBinary {
                data_map: HashMap::from([(channel_hash.clone(), vec![datum_a.clone()])]),
            });

        // Two sibling blocks both change A -> B on the same channel
        let datums_changes = DashMap::new();
        datums_changes.insert(channel_hash.clone(), ChannelChange {
            added: vec![datum_b.clone()],
            removed: vec![datum_a.clone()],
        });
        let branch_change = StateChange {
            datums_changes,
            cont_changes: DashMap::new(),
            consume_channels_to_join_serialized_map: DashMap::new(),
        };
        let combined = branch_change.clone().combine(branch_change);

        let mergeable_chs: NumberChannelsDiff = BTreeMap::new();
        let no_override =
            |_: &Blake2b256Hash,
             _: &ChannelChange<Vec<u8>>,
             _: &NumberChannelsDiff|
             -> Result<Option<HotStoreTrieAction<(), (), (), ()>>, HistoryError> {
                Ok(None)
            };

        let actions = compute_trie_actions(&combined, &base_reader, &mergeable_chs, no_override)
            .expect("compute_trie_actions should succeed");

        assert_eq!(actions.len(), 1, "expected exactly one trie action");
        match &actions[0] {
            HotStoreTrieAction::TrieInsertAction(TrieInsertAction::TrieInsertBinaryProduce(
                insert,
            )) => {
                assert_eq!(insert.hash, channel_hash);
                assert_eq!(insert.data, vec![datum_b]);
            }
            other => panic!("expected TrieInsertBinaryProduce, got {:?}", other),
        }
    }

    /// RED reproduction of the cross-branch re-base stale-consume verified in
    /// run 8686406b. A surviving chain's `removed` datum O is neither in the
    /// base (I, the main parent's close-block value) nor produced by any
    /// sibling chain, so base-first composition `(init -- removed) ++
    /// added` leaves `[I, A]`: a single-value cell silently goes
    /// multi-value. The merge must surface this as an error rather than
    /// corrupt the cell. (The stale chain is rejected upstream in
    /// dag_merger; this asserts the defense-in-depth backstop so a missed
    /// stale fails loudly instead of composing to garbage.)
    #[test]
    fn compute_trie_actions_rejects_stale_removed_on_single_value_cell() {
        let datum_i: Vec<u8> = vec![0x11; 32]; // base value (winner, already in base)
        let datum_o: Vec<u8> = vec![0x00; 32]; // value the stale chain consumed (lost the race)
        let datum_a: Vec<u8> = vec![0xaa; 32]; // stale chain's new value
        let channel_hash = Blake2b256Hash::from_bytes(vec![0x01; 32]);

        let base_reader: Box<dyn HistoryReader<Blake2b256Hash, (), (), (), ()>> =
            Box::new(StubHistoryReaderBinary {
                data_map: HashMap::from([(channel_hash.clone(), vec![datum_i.clone()])]),
            });

        // One chain: removed=[O], added=[A], with O absent from base=[I] and from
        // added. This is the exact verified shape (removed != init, added != init,
        // new_val distinct).
        let datums_changes = DashMap::new();
        datums_changes.insert(channel_hash.clone(), ChannelChange {
            added: vec![datum_a.clone()],
            removed: vec![datum_o.clone()],
        });
        let change = StateChange {
            datums_changes,
            cont_changes: DashMap::new(),
            consume_channels_to_join_serialized_map: DashMap::new(),
        };

        let mergeable_chs: NumberChannelsDiff = BTreeMap::new(); // non-foldable channel
        let no_override =
            |_: &Blake2b256Hash,
             _: &ChannelChange<Vec<u8>>,
             _: &NumberChannelsDiff|
             -> Result<Option<HotStoreTrieAction<(), (), (), ()>>, HistoryError> {
                Ok(None)
            };

        let result = compute_trie_actions(&change, &base_reader, &mergeable_chs, no_override);

        assert!(
            result.is_err(),
            "stale-consume (a removed datum absent from base AND from added) must surface as a \
             merge error, not silently compose a single-value cell to multi-value; got Ok with \
             {:?} action(s)",
            result.as_ref().map(|a| a.len()),
        );
    }

    /// Negative guard against over-rejection: a normal single-value rewrite —
    /// `removed` equals the base value — must NOT be flagged as stale. This is
    /// the common, correct case (consume the current cell value, produce the
    /// next) and must keep composing to a single value.
    #[test]
    fn compute_trie_actions_allows_normal_rewrite_removing_base_value() {
        let datum_base: Vec<u8> = vec![0x22; 32];
        let datum_next: Vec<u8> = vec![0x33; 32];
        let channel_hash = Blake2b256Hash::from_bytes(vec![0x02; 32]);

        let base_reader: Box<dyn HistoryReader<Blake2b256Hash, (), (), (), ()>> =
            Box::new(StubHistoryReaderBinary {
                data_map: HashMap::from([(channel_hash.clone(), vec![datum_base.clone()])]),
            });

        let datums_changes = DashMap::new();
        datums_changes.insert(channel_hash.clone(), ChannelChange {
            added: vec![datum_next.clone()],
            removed: vec![datum_base.clone()], // removed == base: legitimate rewrite
        });
        let change = StateChange {
            datums_changes,
            cont_changes: DashMap::new(),
            consume_channels_to_join_serialized_map: DashMap::new(),
        };

        let mergeable_chs: NumberChannelsDiff = BTreeMap::new();
        let no_override =
            |_: &Blake2b256Hash,
             _: &ChannelChange<Vec<u8>>,
             _: &NumberChannelsDiff|
             -> Result<Option<HotStoreTrieAction<(), (), (), ()>>, HistoryError> {
                Ok(None)
            };

        let actions = compute_trie_actions(&change, &base_reader, &mergeable_chs, no_override)
            .expect("a normal rewrite (removed == base) must not be rejected as stale");
        assert_eq!(actions.len(), 1, "expected exactly one trie action");
        match &actions[0] {
            HotStoreTrieAction::TrieInsertAction(TrieInsertAction::TrieInsertBinaryProduce(
                insert,
            )) => {
                assert_eq!(insert.data, vec![datum_next], "must compose to the single next value");
            }
            other => panic!("expected TrieInsertBinaryProduce, got {:?}", other),
        }
    }
}
