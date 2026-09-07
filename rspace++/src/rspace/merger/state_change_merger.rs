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
        target: "f1r3fly.merge.step",
        step = "compute_trie_actions.ENTER",
        cont_changes = changes.cont_changes.len(),
        datums_changes = changes.datums_changes.len(),
        joins = changes.consume_channels_to_join_serialized_map.len(),
        mergeable_chs = mergeable_chs.len(),
        "computing trie actions for merged state change",
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
            // A fully-netted change — a continuation installed by one chain
            // and COMM-fired by a dependent chain in the same merge, both
            // sides cancelled by `ChannelChange::cancel_common` — is a
            // legitimate no-op: the consume pointer ends exactly at base.
            // No trie action, no join action. Only an entry that still
            // CLAIMS content and produces no delta is incoherence (the
            // `init == new_val` error below).
            if channel_change.added.is_empty() && channel_change.removed.is_empty() {
                tracing::debug!(
                    target: "f1r3fly.merge.step",
                    step = "compute_trie_actions.CONT_NETTED_NOOP",
                    n_channels = consume_channels.len(),
                    "fully-netted cont change (install+fire cancelled) -> no-op",
                );
                return Ok(None);
            }
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

            if tracing::enabled!(target: "f1r3fly.merge.step", tracing::Level::DEBUG) {
                let removed_bytes: usize = channel_change.removed.iter().map(|k| k.len()).sum();
                let added_bytes: usize = channel_change.added.iter().map(|k| k.len()).sum();
                tracing::debug!(
                    target: "f1r3fly.merge.step",
                    step = "compute_trie_actions.CONT_CHANGE",
                    consume_pointer = %hex::encode(history_pointer.clone().bytes()),
                    n_channels = consume_channels.len(),
                    base_konts = init.len(),
                    removed_konts = channel_change.removed.len(),
                    removed_bytes,
                    added_konts = channel_change.added.len(),
                    added_bytes,
                    new_konts = new_val.len(),
                    "cont change: applying removed/added to base konts",
                );
            }

            if init == new_val {
                tracing::debug!(
                    target: "f1r3fly.merge.step",
                    step = "compute_trie_actions.CONT_NOOP_ERR",
                    consume_pointer = %hex::encode(history_pointer.clone().bytes()),
                    "empty consume change (init == new_val) -> merge error",
                );
                Err(HistoryError::MergeError(
                    "Merging logic error: empty consume change when computing trie action."
                        .to_string(),
                ))
            } else if init.is_empty() {
                // No konts were in base state and some are added - insert konts and add join.
                tracing::debug!(
                    target: "f1r3fly.merge.step",
                    step = "compute_trie_actions.CONT_INSERT",
                    consume_pointer = %hex::encode(history_pointer.clone().bytes()),
                    new_konts = new_val.len(),
                    "base empty -> TrieInsertConsume + AddJoin",
                );
                Ok(Some(ConsumeAndJoinActions {
                    consume_action: HotStoreTrieAction::TrieInsertAction(
                        TrieInsertAction::TrieInsertBinaryConsume(TrieInsertBinaryConsume {
                            hash: history_pointer,
                            continuations: new_val,
                        }),
                    ),
                    join_action: Some(JoinActionKind::AddJoin(consume_channels.clone())),
                }))
            } else if new_val.is_empty() {
                // All konts present in base are removed - remove consume, remove join.
                tracing::debug!(
                    target: "f1r3fly.merge.step",
                    step = "compute_trie_actions.CONT_DELETE",
                    consume_pointer = %hex::encode(history_pointer.clone().bytes()),
                    base_konts = init.len(),
                    "all base konts removed -> TrieDeleteConsume + RemoveJoin",
                );
                Ok(Some(ConsumeAndJoinActions {
                    consume_action: HotStoreTrieAction::TrieDeleteAction(
                        TrieDeleteAction::TrieDeleteConsume(TrieDeleteConsume {
                            hash: history_pointer,
                        }),
                    ),
                    join_action: Some(JoinActionKind::RemoveJoin(consume_channels.clone())),
                }))
            } else {
                // Konts were updated but consume is present in base state - update konts, no
                // joins.
                tracing::debug!(
                    target: "f1r3fly.merge.step",
                    step = "compute_trie_actions.CONT_UPDATE",
                    consume_pointer = %hex::encode(history_pointer.clone().bytes()),
                    base_konts = init.len(),
                    new_konts = new_val.len(),
                    "konts updated, consume present in base -> TrieInsertConsume, no join change",
                );
                Ok(Some(ConsumeAndJoinActions {
                    consume_action: HotStoreTrieAction::TrieInsertAction(
                        TrieInsertAction::TrieInsertBinaryConsume(TrieInsertBinaryConsume {
                            hash: history_pointer,
                            continuations: new_val,
                        }),
                    ),
                    join_action: None,
                }))
            }
        })
        .collect::<Result<Vec<Option<ConsumeAndJoinActions<C, P, A, K>>>, HistoryError>>()?
        .into_iter()
        .flatten()
        .collect();

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
            if tracing::enabled!(target: "f1r3fly.merge.step", tracing::Level::DEBUG) {
                let removed_bytes: usize = changes.removed.iter().map(|d| d.len()).sum();
                let added_bytes: usize = changes.added.iter().map(|d| d.len()).sum();
                tracing::debug!(
                    target: "f1r3fly.merge.step",
                    step = "compute_trie_actions.DATUM_CHANGE",
                    channel = %hex::encode(history_pointer.clone().bytes()),
                    removed = changes.removed.len(),
                    removed_bytes,
                    added = changes.added.len(),
                    added_bytes,
                    "datum change for channel -> produce trie action",
                );
            }

            // Number-channel (mergeable) override path, if any.
            let override_action = handle_channel_change(history_pointer, changes, mergeable_chs)?;
            match override_action {
                Some(action) => {
                    tracing::debug!(
                        target: "f1r3fly.merge.step",
                        step = "compute_trie_actions.NUMBER_CHANNEL",
                        channel = %hex::encode(history_pointer.clone().bytes()),
                        "handle_channel_change produced number-channel trie action (override)",
                    );
                    Ok(Some(action))
                }
                // A fully-netted change (a produce and its dependent consume
                // cancelled by `ChannelChange::cancel_common`) is a
                // legitimate no-op: the channel ends exactly at base. The
                // guard sits AFTER the override — mergeable channels derive
                // their action from the fold's diff, not from added/removed.
                None if changes.added.is_empty() && changes.removed.is_empty() => {
                    tracing::debug!(
                        target: "f1r3fly.merge.step",
                        step = "compute_trie_actions.DATUM_NETTED_NOOP",
                        channel = %hex::encode(history_pointer.clone().bytes()),
                        "fully-netted datum change (produce+consume cancelled) -> no-op",
                    );
                    Ok(None)
                }
                None => make_trie_action(
                    history_pointer,
                    "datum",
                    |hash| base_reader.get_data_proj_binary(hash),
                    changes,
                    |hash| {
                        HotStoreTrieAction::TrieDeleteAction(TrieDeleteAction::TrieDeleteProduce(
                            TrieDeleteProduce { hash: hash.clone() },
                        ))
                    },
                    |hash, data| {
                        HotStoreTrieAction::TrieInsertAction(
                            TrieInsertAction::TrieInsertBinaryProduce(TrieInsertBinaryProduce {
                                hash: hash.clone(),
                                data,
                            }),
                        )
                    },
                )
                .map(Some),
            }
        })
        .collect::<Result<Vec<Option<_>>, _>>()?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();

    // Process joins changes
    let joins_channels_to_body_map = &changes.consume_channels_to_join_serialized_map;
    let mut joins_changes = std::collections::HashMap::new();

    tracing::debug!(
        target: "f1r3fly.merge.step",
        step = "compute_trie_actions.JOINS_ENTER",
        consume_actions = consume_with_join_actions.len(),
        "collecting join changes from consume actions",
    );

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

    tracing::debug!(
        target: "f1r3fly.merge.step",
        step = "compute_trie_actions.JOINS_COLLECTED",
        join_channels = joins_changes.len(),
        "join changes collected per channel",
    );

    // Sort joins changes by history pointer for deterministic ordering
    let mut joins_changes_sorted: Vec<_> = joins_changes.into_iter().collect();
    joins_changes_sorted.sort_by_key(|(history_pointer, _)| history_pointer.clone());

    let joins_trie_actions = joins_changes_sorted
        .iter()
        .map(|(history_pointer, changes)| {
            if tracing::enabled!(target: "f1r3fly.merge.step", tracing::Level::DEBUG) {
                tracing::debug!(
                    target: "f1r3fly.merge.step",
                    step = "compute_trie_actions.JOIN_CHANGE",
                    channel = %hex::encode(history_pointer.clone().bytes()),
                    removed = changes.removed.len(),
                    added = changes.added.len(),
                    "join change for channel -> join trie action",
                );
            }
            make_trie_action(
                history_pointer,
                "joins",
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
            )
        })
        .collect::<Result<Vec<_>, _>>()?;

    // Combine all trie actions
    let n_produce = produce_trie_actions.len();
    let n_consume = consume_trie_actions.len();
    let n_joins = joins_trie_actions.len();
    let mut result = Vec::new();
    result.extend(produce_trie_actions);
    result.extend(consume_trie_actions);
    result.extend(joins_trie_actions);

    tracing::debug!(
        target: "f1r3fly.merge.step",
        step = "compute_trie_actions.EXIT",
        produce_actions = n_produce,
        consume_actions = n_consume,
        join_actions = n_joins,
        total_actions = result.len(),
        "trie actions computed",
    );

    Ok(result)
}

fn make_trie_action<C: Clone, P: Clone, A: Clone, K: Clone>(
    history_pointer: &Blake2b256Hash,
    // Which fold this action belongs to ("datum" | "joins"). The incoherence
    // error below is otherwise indistinguishable between the two call sites,
    // and they fail for different reasons: datum removals are checked against
    // the base by the availability splitter, joins removals are checked by
    // nothing. Naming the kind is the difference between knowing which guard
    // failed and re-running the whole hunt to find out.
    kind: &'static str,
    init_value: impl Fn(&Blake2b256Hash) -> Result<Vec<ByteVector>, HistoryError>,
    changes: &ChannelChange<ByteVector>,
    remove_action: impl Fn(&Blake2b256Hash) -> HotStoreTrieAction<C, P, A, K>,
    update_action: impl Fn(&Blake2b256Hash, Vec<ByteVector>) -> HotStoreTrieAction<C, P, A, K>,
) -> Result<HotStoreTrieAction<C, P, A, K>, HistoryError> {
    let init = init_value(history_pointer)?;

    // Removing an item the base does not hold is record incoherence — the
    // availability splitters keep only chains whose consumes are available
    // at the base, and `ChannelChange::combine` nets producer→consumer
    // pairs before this point, so a residual absent-removal means the
    // applied diffs disagree with the base they claim to extend. Hard
    // error, never a silent no-op: a silent no-op is how stale diffs
    // "apply" cleanly while the resulting state diverges from the record.
    let new_val = {
        let mut remove_counts: std::collections::HashMap<&Vec<u8>, usize> =
            std::collections::HashMap::new();
        for item in &changes.removed {
            *remove_counts.entry(item).or_insert(0) += 1;
        }
        let mut result: Vec<ByteVector> = Vec::with_capacity(init.len());
        for item in &init {
            match remove_counts.get_mut(item) {
                Some(count) if *count > 0 => *count -= 1,
                _ => result.push(item.clone()),
            }
        }
        let unmatched: usize = remove_counts.values().sum();
        if unmatched > 0 {
            // Digest the mismatch itself: which values the base holds versus
            // which the diff wants gone. Without this the message says only
            // that counts disagree, and identifying the stale value costs a
            // debug-level re-run of the whole shard.
            let digest = |items: &[ByteVector]| -> Vec<String> {
                items
                    .iter()
                    .take(4)
                    .map(|b| hex::encode(&b[..8.min(b.len())]))
                    .collect()
            };
            return Err(HistoryError::MergeError(format!(
                "channel {} [{}]: {} removed item(s) absent from the base (base holds {}, diff \
                 removes {}) — applied diffs are incoherent with the base state; base={:?} \
                 removed={:?}",
                hex::encode(history_pointer.clone().bytes()),
                kind,
                unmatched,
                init.len(),
                changes.removed.len(),
                digest(&init),
                digest(&changes.removed),
            )));
        }
        result.extend(changes.added.clone());
        result
    };

    if tracing::enabled!(target: "f1r3fly.merge.step", tracing::Level::DEBUG) {
        let base_bytes: usize = init.iter().map(|d| d.len()).sum();
        let removed_bytes: usize = changes.removed.iter().map(|d| d.len()).sum();
        let added_bytes: usize = changes.added.iter().map(|d| d.len()).sum();
        let new_bytes: usize = new_val.iter().map(|d| d.len()).sum();
        tracing::debug!(
            target: "f1r3fly.merge.step",
            step = "make_trie_action.ENTER",
            channel = %hex::encode(history_pointer.clone().bytes()),
            base_items = init.len(),
            base_bytes,
            removed = changes.removed.len(),
            removed_bytes,
            added = changes.added.len(),
            added_bytes,
            new_items = new_val.len(),
            new_bytes,
            "applied removed/added to base value",
        );
    }

    if new_val.is_empty() && !init.is_empty() {
        // Case 1: All items present in base are removed - remove action
        tracing::debug!(
            target: "f1r3fly.merge.step",
            step = "make_trie_action.DELETE",
            channel = %hex::encode(history_pointer.clone().bytes()),
            base_items = init.len(),
            "result EMPTY (channel emptied) -> TrieDelete",
        );
        Ok(remove_action(history_pointer))
    } else if init != new_val {
        // Case 2: Items were updated - update action
        tracing::debug!(
            target: "f1r3fly.merge.step",
            step = "make_trie_action.INSERT",
            channel = %hex::encode(history_pointer.clone().bytes()),
            new_items = new_val.len(),
            "result NON-EMPTY (channel retained) -> TrieInsert",
        );
        Ok(update_action(history_pointer, new_val))
    } else {
        // Case 3: Error case - no changes
        tracing::debug!(
            target: "f1r3fly.merge.step",
            step = "make_trie_action.NOOP_ERR",
            channel = %hex::encode(history_pointer.clone().bytes()),
            "init == new_val (no change) -> merge error",
        );
        Err(HistoryError::MergeError(
            "Merging logic error: empty channel change for produce or join when computing trie \
             action."
                .to_string(),
        ))
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

    /// Removing a datum that is NOT in the base is record incoherence, and
    /// it must be a hard error, never a silent no-op. Upstream layers
    /// guarantee the shape cannot occur legitimately: the availability
    /// splitters keep only chains whose consumes are available at the base,
    /// and `ChannelChange::combine` nets producer→consumer pairs within one
    /// application before this point. A silent no-op here is how stale diffs
    /// apply "cleanly" onto a base without the work they assumed — the
    /// applied state diverges from the record while every check passes.
    #[test]
    fn removal_absent_from_base_is_an_error_not_a_noop() {
        let phantom_removed: Vec<u8> = vec![0xdd; 32];
        let datum_new: Vec<u8> = vec![0xee; 32];
        let channel_hash = Blake2b256Hash::from_bytes(vec![0x02; 32]);

        // Base holds NOTHING on the channel.
        let base_reader: Box<dyn HistoryReader<Blake2b256Hash, (), (), (), ()>> =
            Box::new(StubHistoryReaderBinary {
                data_map: HashMap::new(),
            });

        let datums_changes = DashMap::new();
        datums_changes.insert(channel_hash, ChannelChange {
            added: vec![datum_new],
            removed: vec![phantom_removed],
        });
        let changes = StateChange {
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

        let result = compute_trie_actions(&changes, &base_reader, &mergeable_chs, no_override);

        assert!(
            result.is_err(),
            "a datum removal absent from the base must hard-error (record incoherence), not \
             silently no-op"
        );

        // The message must be actionable on its own. This error deterministically
        // wedges every propose over the same scope, so a reader who has only this
        // line must still learn WHICH fold failed and WHAT disagreed — recovering
        // that from a debug-level re-run costs gigabytes per minute.
        let message = result.unwrap_err().to_string();
        assert!(
            message.contains("[datum]"),
            "the failing fold must be named — 'datum' and 'joins' share this error text and are \
             checked by different guards: {message}"
        );
        assert!(
            message.contains("removed=") && message.contains("base="),
            "the message must digest base-vs-removed values, not just counts: {message}"
        );
    }

    /// A continuation INSTALLED by one chain and COMM-FIRED by a dependent
    /// chain in the same merge scope nets to an EMPTY cont change
    /// (`ChannelChange::cancel_common`) whose map entry survives the
    /// combine. That fully-netted entry is a legitimate no-op — the
    /// consume pointer ends exactly at base — and must produce no trie
    /// action and no join action, never the "empty consume change"
    /// merging-logic error, which kills every propose whose scope spans an
    /// install+fire pair.
    #[test]
    fn netted_install_fire_cont_change_is_a_noop_not_an_error() {
        let kont: Vec<u8> = vec![0xcc; 48];
        let channel = Blake2b256Hash::from_bytes(vec![0x03; 32]);

        let install = StateChange {
            datums_changes: DashMap::new(),
            cont_changes: {
                let m = DashMap::new();
                m.insert(vec![channel.clone()], ChannelChange {
                    added: vec![kont.clone()],
                    removed: vec![],
                });
                m
            },
            consume_channels_to_join_serialized_map: DashMap::new(),
        };
        let fire = StateChange {
            datums_changes: DashMap::new(),
            cont_changes: {
                let m = DashMap::new();
                m.insert(vec![channel.clone()], ChannelChange {
                    added: vec![],
                    removed: vec![kont],
                });
                m
            },
            consume_channels_to_join_serialized_map: DashMap::new(),
        };
        let combined = install.combine(fire);

        // Base holds no continuations anywhere.
        let base_reader: Box<dyn HistoryReader<Blake2b256Hash, (), (), (), ()>> =
            Box::new(StubHistoryReaderBinary {
                data_map: HashMap::new(),
            });
        let mergeable_chs: NumberChannelsDiff = BTreeMap::new();
        let no_override =
            |_: &Blake2b256Hash,
             _: &ChannelChange<Vec<u8>>,
             _: &NumberChannelsDiff|
             -> Result<Option<HotStoreTrieAction<(), (), (), ()>>, HistoryError> {
                Ok(None)
            };

        let actions = compute_trie_actions(&combined, &base_reader, &mergeable_chs, no_override)
            .expect("a fully-netted cont change is a no-op, not a merge error");
        assert!(actions.is_empty(), "no trie action may be emitted for a netted install+fire pair");
    }

    /// The datum-side twin: a seed's produce and its dependent consume in
    /// one merge scope net to an empty datum change; the channel ends
    /// exactly at base and must produce no trie action, never the "empty
    /// channel change for produce or join" error.
    #[test]
    fn netted_produce_consume_datum_change_is_a_noop_not_an_error() {
        let datum: Vec<u8> = vec![0xab; 32];
        let channel = Blake2b256Hash::from_bytes(vec![0x04; 32]);

        let seed = StateChange {
            datums_changes: {
                let m = DashMap::new();
                m.insert(channel.clone(), ChannelChange {
                    added: vec![datum.clone()],
                    removed: vec![],
                });
                m
            },
            cont_changes: DashMap::new(),
            consume_channels_to_join_serialized_map: DashMap::new(),
        };
        let consume = StateChange {
            datums_changes: {
                let m = DashMap::new();
                m.insert(channel.clone(), ChannelChange {
                    added: vec![],
                    removed: vec![datum],
                });
                m
            },
            cont_changes: DashMap::new(),
            consume_channels_to_join_serialized_map: DashMap::new(),
        };
        let combined = seed.combine(consume);

        let base_reader: Box<dyn HistoryReader<Blake2b256Hash, (), (), (), ()>> =
            Box::new(StubHistoryReaderBinary {
                data_map: HashMap::new(),
            });
        let mergeable_chs: NumberChannelsDiff = BTreeMap::new();
        let no_override =
            |_: &Blake2b256Hash,
             _: &ChannelChange<Vec<u8>>,
             _: &NumberChannelsDiff|
             -> Result<Option<HotStoreTrieAction<(), (), (), ()>>, HistoryError> {
                Ok(None)
            };

        let actions = compute_trie_actions(&combined, &base_reader, &mergeable_chs, no_override)
            .expect("a fully-netted datum change is a no-op, not a merge error");
        assert!(
            actions.is_empty(),
            "no trie action may be emitted for a netted produce+consume pair"
        );
    }
}

#[cfg(test)]
mod branch_tests {
    use std::collections::{BTreeMap, HashMap};

    use dashmap::DashMap;

    use super::*;
    use crate::rspace::history::history_reader::HistoryReaderBase;
    use crate::rspace::internal::{Datum, WaitingContinuation};
    use crate::rspace::merger::merging_logic::MergeType;

    struct MapHistoryReader {
        data_map: HashMap<Blake2b256Hash, Vec<Vec<u8>>>,
        cont_map: HashMap<Blake2b256Hash, Vec<Vec<u8>>>,
        joins_map: HashMap<Blake2b256Hash, Vec<Vec<u8>>>,
    }

    struct EmptyBase;

    impl HistoryReaderBase<(), (), (), ()> for EmptyBase {
        fn get_data_proj(&self, _key: &()) -> Vec<Datum<()>> { vec![] }

        fn get_continuations_proj(&self, _key: &Vec<()>) -> Vec<WaitingContinuation<(), ()>> {
            vec![]
        }

        fn get_joins_proj(&self, _key: &()) -> Vec<Vec<()>> { vec![] }
    }

    impl HistoryReader<Blake2b256Hash, (), (), (), ()> for MapHistoryReader {
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
            key: &Blake2b256Hash,
        ) -> Result<Vec<Vec<u8>>, HistoryError> {
            Ok(self.cont_map.get(key).cloned().unwrap_or_default())
        }

        fn get_joins_proj(&self, _key: &Blake2b256Hash) -> Result<Vec<Vec<()>>, HistoryError> {
            Ok(vec![])
        }

        fn get_joins_proj_binary(
            &self,
            key: &Blake2b256Hash,
        ) -> Result<Vec<Vec<u8>>, HistoryError> {
            Ok(self.joins_map.get(key).cloned().unwrap_or_default())
        }

        fn base(&self) -> Box<dyn HistoryReaderBase<(), (), (), ()>> { Box::new(EmptyBase) }

        fn get_data_proj_generic(&self, _key: &()) -> Vec<Datum<()>> { vec![] }

        fn get_continuations_proj_generic(
            &self,
            _key: &Vec<()>,
        ) -> Vec<WaitingContinuation<(), ()>> {
            vec![]
        }

        fn get_joins_proj_generic(&self, _key: &()) -> Vec<Vec<()>> { vec![] }
    }

    fn mk_hash(byte: u8) -> Blake2b256Hash { Blake2b256Hash::from_bytes(vec![byte; 32]) }

    fn reader(
        data_map: HashMap<Blake2b256Hash, Vec<Vec<u8>>>,
        cont_map: HashMap<Blake2b256Hash, Vec<Vec<u8>>>,
        joins_map: HashMap<Blake2b256Hash, Vec<Vec<u8>>>,
    ) -> Box<dyn HistoryReader<Blake2b256Hash, (), (), (), ()>> {
        Box::new(MapHistoryReader {
            data_map,
            cont_map,
            joins_map,
        })
    }

    fn cont_state_change(
        channel: &Blake2b256Hash,
        added: Vec<Vec<u8>>,
        removed: Vec<Vec<u8>>,
        join_value: Option<Vec<u8>>,
    ) -> StateChange {
        let cont_changes = DashMap::new();
        cont_changes.insert(vec![channel.clone()], ChannelChange { added, removed });
        let joins = DashMap::new();
        if let Some(value) = join_value {
            joins.insert(vec![channel.clone()], value);
        }
        StateChange {
            datums_changes: DashMap::new(),
            cont_changes,
            consume_channels_to_join_serialized_map: joins,
        }
    }

    fn datum_state_change(
        channel: &Blake2b256Hash,
        added: Vec<Vec<u8>>,
        removed: Vec<Vec<u8>>,
    ) -> StateChange {
        let datums_changes = DashMap::new();
        datums_changes.insert(channel.clone(), ChannelChange { added, removed });
        StateChange {
            datums_changes,
            cont_changes: DashMap::new(),
            consume_channels_to_join_serialized_map: DashMap::new(),
        }
    }

    fn no_override(
        _: &Blake2b256Hash,
        _: &ChannelChange<Vec<u8>>,
        _: &NumberChannelsDiff,
    ) -> Result<Option<HotStoreTrieAction<(), (), (), ()>>, HistoryError> {
        Ok(None)
    }

    #[test]
    fn cont_added_on_empty_base_inserts_consume_and_adds_join() {
        let channel = mk_hash(0x10);
        let pointer = stable_hash_provider::hash_from_hashes(&vec![channel.clone()]);
        let kont = vec![0xc1; 16];
        let join_bytes = vec![0x1a; 8];

        let changes =
            cont_state_change(&channel, vec![kont.clone()], vec![], Some(join_bytes.clone()));
        let base = reader(HashMap::new(), HashMap::new(), HashMap::new());

        let actions = compute_trie_actions(&changes, &base, &BTreeMap::new(), no_override).unwrap();

        assert_eq!(actions.len(), 2);
        match &actions[0] {
            HotStoreTrieAction::TrieInsertAction(TrieInsertAction::TrieInsertBinaryConsume(
                insert,
            )) => {
                assert_eq!(insert.hash, pointer);
                assert_eq!(insert.continuations, vec![kont]);
            }
            other => panic!("expected TrieInsertBinaryConsume, got {:?}", other),
        }
        match &actions[1] {
            HotStoreTrieAction::TrieInsertAction(TrieInsertAction::TrieInsertBinaryJoins(
                insert,
            )) => {
                assert_eq!(insert.hash, channel);
                assert_eq!(insert.joins, vec![join_bytes]);
            }
            other => panic!("expected TrieInsertBinaryJoins, got {:?}", other),
        }
    }

    #[test]
    fn all_base_konts_removed_deletes_consume_and_removes_join() {
        let channel = mk_hash(0x11);
        let pointer = stable_hash_provider::hash_from_hashes(&vec![channel.clone()]);
        let kont = vec![0xc2; 16];
        let join_bytes = vec![0x2a; 8];

        let changes =
            cont_state_change(&channel, vec![], vec![kont.clone()], Some(join_bytes.clone()));
        let base = reader(
            HashMap::new(),
            HashMap::from([(pointer.clone(), vec![kont])]),
            HashMap::from([(channel.clone(), vec![join_bytes])]),
        );

        let actions = compute_trie_actions(&changes, &base, &BTreeMap::new(), no_override).unwrap();

        assert_eq!(actions.len(), 2);
        assert!(matches!(
            &actions[0],
            HotStoreTrieAction::TrieDeleteAction(TrieDeleteAction::TrieDeleteConsume(
                TrieDeleteConsume { hash },
            )) if *hash == pointer
        ));
        assert!(matches!(
            &actions[1],
            HotStoreTrieAction::TrieDeleteAction(TrieDeleteAction::TrieDeleteJoins(
                TrieDeleteJoins { hash },
            )) if *hash == channel
        ));
    }

    #[test]
    fn removed_join_keeps_remaining_base_joins_as_insert() {
        let channel = mk_hash(0x12);
        let pointer = stable_hash_provider::hash_from_hashes(&vec![channel.clone()]);
        let kont = vec![0xc3; 16];
        let removed_join = vec![0x3a; 8];
        let surviving_join = vec![0x3b; 8];

        let changes =
            cont_state_change(&channel, vec![], vec![kont.clone()], Some(removed_join.clone()));
        let base = reader(
            HashMap::new(),
            HashMap::from([(pointer, vec![kont])]),
            HashMap::from([(channel.clone(), vec![removed_join, surviving_join.clone()])]),
        );

        let actions = compute_trie_actions(&changes, &base, &BTreeMap::new(), no_override).unwrap();

        assert_eq!(actions.len(), 2);
        match &actions[1] {
            HotStoreTrieAction::TrieInsertAction(TrieInsertAction::TrieInsertBinaryJoins(
                insert,
            )) => {
                assert_eq!(insert.hash, channel);
                assert_eq!(insert.joins, vec![surviving_join]);
            }
            other => panic!("expected TrieInsertBinaryJoins, got {:?}", other),
        }
    }

    #[test]
    fn kont_update_with_consume_still_present_emits_insert_without_join_change() {
        let channel = mk_hash(0x13);
        let pointer = stable_hash_provider::hash_from_hashes(&vec![channel.clone()]);
        let old_kont = vec![0xc4; 16];
        let new_kont = vec![0xc5; 16];

        let changes =
            cont_state_change(&channel, vec![new_kont.clone()], vec![old_kont.clone()], None);
        let base = reader(
            HashMap::new(),
            HashMap::from([(pointer.clone(), vec![old_kont])]),
            HashMap::new(),
        );

        let actions = compute_trie_actions(&changes, &base, &BTreeMap::new(), no_override).unwrap();

        assert_eq!(actions.len(), 1);
        match &actions[0] {
            HotStoreTrieAction::TrieInsertAction(TrieInsertAction::TrieInsertBinaryConsume(
                insert,
            )) => {
                assert_eq!(insert.hash, pointer);
                assert_eq!(insert.continuations, vec![new_kont]);
            }
            other => panic!("expected TrieInsertBinaryConsume, got {:?}", other),
        }
    }

    #[test]
    fn cont_change_that_claims_content_but_produces_no_delta_is_an_error() {
        let channel = mk_hash(0x14);
        let pointer = stable_hash_provider::hash_from_hashes(&vec![channel.clone()]);
        let kont = vec![0xc6; 16];

        let changes = cont_state_change(&channel, vec![kont.clone()], vec![kont.clone()], None);
        let base = reader(HashMap::new(), HashMap::from([(pointer, vec![kont])]), HashMap::new());

        let result = compute_trie_actions(&changes, &base, &BTreeMap::new(), no_override);
        let message = result.unwrap_err().to_string();
        assert!(message.contains("empty consume change"), "{message}");
    }

    #[test]
    fn missing_serialized_join_for_join_action_is_an_error() {
        let channel = mk_hash(0x15);
        let kont = vec![0xc7; 16];

        let changes = cont_state_change(&channel, vec![kont], vec![], None);
        let base = reader(HashMap::new(), HashMap::new(), HashMap::new());

        let result = compute_trie_actions(&changes, &base, &BTreeMap::new(), no_override);
        let message = result.unwrap_err().to_string();
        assert!(message.contains("No ByteVector value for join"), "{message}");
    }

    #[test]
    fn all_base_datums_removed_deletes_produce() {
        let channel = mk_hash(0x16);
        let datum = vec![0xd1; 16];

        let changes = datum_state_change(&channel, vec![], vec![datum.clone()]);
        let base =
            reader(HashMap::from([(channel.clone(), vec![datum])]), HashMap::new(), HashMap::new());

        let actions = compute_trie_actions(&changes, &base, &BTreeMap::new(), no_override).unwrap();

        assert_eq!(actions.len(), 1);
        assert!(matches!(
            &actions[0],
            HotStoreTrieAction::TrieDeleteAction(TrieDeleteAction::TrieDeleteProduce(
                TrieDeleteProduce { hash },
            )) if *hash == channel
        ));
    }

    #[test]
    fn datum_change_that_claims_content_but_produces_no_delta_is_an_error() {
        let channel = mk_hash(0x17);
        let datum = vec![0xd2; 16];

        let changes = datum_state_change(&channel, vec![datum.clone()], vec![datum.clone()]);
        let base =
            reader(HashMap::from([(channel.clone(), vec![datum])]), HashMap::new(), HashMap::new());

        let result = compute_trie_actions(&changes, &base, &BTreeMap::new(), no_override);
        let message = result.unwrap_err().to_string();
        assert!(message.contains("empty channel change for produce or join"), "{message}");
    }

    #[test]
    fn number_channel_override_replaces_datum_fold() {
        let channel = mk_hash(0x18);
        let override_data = vec![vec![0x99; 8]];

        let changes = datum_state_change(&channel, vec![], vec![]);
        let base = reader(HashMap::new(), HashMap::new(), HashMap::new());
        let mut mergeable_chs: NumberChannelsDiff = BTreeMap::new();
        mergeable_chs.insert(channel.clone(), (1, MergeType::IntegerAdd));

        let expected_data = override_data.clone();
        let with_override =
            move |hash: &Blake2b256Hash,
                  _: &ChannelChange<Vec<u8>>,
                  chs: &NumberChannelsDiff|
                  -> Result<Option<HotStoreTrieAction<(), (), (), ()>>, HistoryError> {
                assert!(chs.contains_key(hash));
                Ok(Some(HotStoreTrieAction::TrieInsertAction(
                    TrieInsertAction::TrieInsertBinaryProduce(TrieInsertBinaryProduce {
                        hash: hash.clone(),
                        data: expected_data.clone(),
                    }),
                )))
            };

        let actions = compute_trie_actions(&changes, &base, &mergeable_chs, with_override).unwrap();

        assert_eq!(actions.len(), 1);
        match &actions[0] {
            HotStoreTrieAction::TrieInsertAction(TrieInsertAction::TrieInsertBinaryProduce(
                insert,
            )) => {
                assert_eq!(insert.hash, channel);
                assert_eq!(insert.data, override_data);
            }
            other => panic!("expected override TrieInsertBinaryProduce, got {:?}", other),
        }
    }
}
