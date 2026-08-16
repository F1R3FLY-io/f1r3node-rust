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
                tracing::debug!(
                    target: "f1r3fly.merge.step",
                    step = "compute_trie_actions.CONT_DELETE",
                    consume_pointer = %hex::encode(history_pointer.clone().bytes()),
                    base_konts = init.len(),
                    "all base konts removed -> TrieDeleteConsume + RemoveJoin",
                );
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
                tracing::debug!(
                    target: "f1r3fly.merge.step",
                    step = "compute_trie_actions.CONT_UPDATE",
                    consume_pointer = %hex::encode(history_pointer.clone().bytes()),
                    base_konts = init.len(),
                    new_konts = new_val.len(),
                    "konts updated, consume present in base -> TrieInsertConsume, no join change",
                );
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
                    Ok(action)
                }
                None => make_trie_action(
                    history_pointer,
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
                ),
            }
        })
        .collect::<Result<Vec<_>, _>>()?;

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
    init_value: impl Fn(&Blake2b256Hash) -> Result<Vec<ByteVector>, HistoryError>,
    changes: &ChannelChange<ByteVector>,
    remove_action: impl Fn(&Blake2b256Hash) -> HotStoreTrieAction<C, P, A, K>,
    update_action: impl Fn(&Blake2b256Hash, Vec<ByteVector>) -> HotStoreTrieAction<C, P, A, K>,
) -> Result<HotStoreTrieAction<C, P, A, K>, HistoryError> {
    let init = init_value(history_pointer)?;

    let new_val = {
        // Use multiset diff: remove each item in 'removed' exactly once from 'init'
        let mut result = StateChange::multiset_diff(&init, &changes.removed);
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

    use super::*;
    use crate::rspace::history::history_reader::HistoryReaderBase;
    use crate::rspace::internal::{Datum, WaitingContinuation};

    #[derive(Default)]
    struct StubHistoryReaderBinary {
        data_map: HashMap<Blake2b256Hash, Vec<Vec<u8>>>,
        continuations_map: HashMap<Blake2b256Hash, Vec<Vec<u8>>>,
        joins_map: HashMap<Blake2b256Hash, Vec<Vec<u8>>>,
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
            key: &Blake2b256Hash,
        ) -> Result<Vec<Vec<u8>>, HistoryError> {
            Ok(self.continuations_map.get(key).cloned().unwrap_or_default())
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
                ..Default::default()
            });

        // Two sibling blocks both change A -> B on the same channel
        let branch_change = StateChange::from_parts(
            HashMap::from([(channel_hash.clone(), ChannelChange {
                added: vec![datum_b.clone()],
                removed: vec![datum_a.clone()],
            })]),
            HashMap::new(),
            HashMap::new(),
        );
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

    #[test]
    fn make_trie_action_distinguishes_delete_insert_and_noop() {
        let hash = Blake2b256Hash::from_bytes(vec![0x11; 32]);
        let existing = vec![0x22];
        let replacement = vec![0x33];

        let deleted: HotStoreTrieAction<(), (), (), ()> = make_trie_action(
            &hash,
            |_| Ok(vec![existing.clone()]),
            &ChannelChange {
                added: vec![],
                removed: vec![existing.clone()],
            },
            |hash| {
                HotStoreTrieAction::TrieDeleteAction(TrieDeleteAction::TrieDeleteProduce(
                    TrieDeleteProduce { hash: hash.clone() },
                ))
            },
            |hash, data| {
                HotStoreTrieAction::TrieInsertAction(TrieInsertAction::TrieInsertBinaryProduce(
                    TrieInsertBinaryProduce {
                        hash: hash.clone(),
                        data,
                    },
                ))
            },
        )
        .unwrap();
        assert!(matches!(
            deleted,
            HotStoreTrieAction::TrieDeleteAction(TrieDeleteAction::TrieDeleteProduce(_))
        ));

        let inserted: HotStoreTrieAction<(), (), (), ()> = make_trie_action(
            &hash,
            |_| Ok(vec![]),
            &ChannelChange {
                added: vec![replacement.clone()],
                removed: vec![],
            },
            |_| unreachable!(),
            |hash, data| {
                HotStoreTrieAction::TrieInsertAction(TrieInsertAction::TrieInsertBinaryProduce(
                    TrieInsertBinaryProduce {
                        hash: hash.clone(),
                        data,
                    },
                ))
            },
        )
        .unwrap();
        match inserted {
            HotStoreTrieAction::TrieInsertAction(TrieInsertAction::TrieInsertBinaryProduce(
                action,
            )) => assert_eq!(action.data, vec![replacement]),
            other => panic!("expected produce insertion, got {:?}", other),
        }

        let noop: Result<HotStoreTrieAction<(), (), (), ()>, _> = make_trie_action(
            &hash,
            |_| Ok(vec![existing.clone()]),
            &ChannelChange::empty(),
            |_| unreachable!(),
            |_, _| unreachable!(),
        );
        assert!(matches!(noop, Err(HistoryError::MergeError(_))));

        let empty_noop: Result<HotStoreTrieAction<(), (), (), ()>, _> = make_trie_action(
            &hash,
            |_| Ok(vec![]),
            &ChannelChange::empty(),
            |_| unreachable!(),
            |_, _| unreachable!(),
        );
        assert!(matches!(empty_noop, Err(HistoryError::MergeError(_))));
    }

    #[test]
    fn compute_trie_actions_inserts_new_consume_and_joins() {
        let channel_a = Blake2b256Hash::from_bytes(vec![0x01; 32]);
        let channel_b = Blake2b256Hash::from_bytes(vec![0x02; 32]);
        let consume_channels = vec![channel_b.clone(), channel_a.clone()];
        let continuation = vec![0x44];
        let join = vec![0x55];
        let changes = StateChange::from_parts(
            HashMap::new(),
            HashMap::from([(consume_channels.clone(), ChannelChange {
                added: vec![continuation.clone()],
                removed: vec![],
            })]),
            HashMap::from([(consume_channels.clone(), join.clone())]),
        );
        let reader: Box<dyn HistoryReader<Blake2b256Hash, (), (), (), ()>> =
            Box::new(StubHistoryReaderBinary::default());

        let actions =
            compute_trie_actions(&changes, &reader, &BTreeMap::new(), |_, _, _| Ok(None)).unwrap();

        assert_eq!(actions.len(), 3);
        match &actions[0] {
            HotStoreTrieAction::TrieInsertAction(TrieInsertAction::TrieInsertBinaryConsume(
                action,
            )) => assert_eq!(action.continuations, vec![continuation]),
            other => panic!("expected consume insertion, got {:?}", other),
        }
        let mut join_hashes = actions[1..]
            .iter()
            .map(|action| match action {
                HotStoreTrieAction::TrieInsertAction(TrieInsertAction::TrieInsertBinaryJoins(
                    action,
                )) => {
                    assert_eq!(action.joins, vec![join.clone()]);
                    action.hash.clone()
                }
                other => panic!("expected join insertion, got {:?}", other),
            })
            .collect::<Vec<_>>();
        let mut expected = vec![channel_a, channel_b];
        join_hashes.sort();
        expected.sort();
        assert_eq!(join_hashes, expected);
    }

    #[test]
    fn compute_trie_actions_updates_existing_consume_without_join_changes() {
        let channel = Blake2b256Hash::from_bytes(vec![0x03; 32]);
        let consume_channels = vec![channel];
        let pointer = stable_hash_provider::hash_from_hashes(&consume_channels);
        let old = vec![0x10];
        let new = vec![0x20];
        let changes = StateChange::from_parts(
            HashMap::new(),
            HashMap::from([(consume_channels, ChannelChange {
                added: vec![new.clone()],
                removed: vec![old.clone()],
            })]),
            HashMap::new(),
        );
        let reader: Box<dyn HistoryReader<Blake2b256Hash, (), (), (), ()>> =
            Box::new(StubHistoryReaderBinary {
                continuations_map: HashMap::from([(pointer, vec![old])]),
                ..Default::default()
            });

        let actions =
            compute_trie_actions(&changes, &reader, &BTreeMap::new(), |_, _, _| Ok(None)).unwrap();

        assert_eq!(actions.len(), 1);
        match &actions[0] {
            HotStoreTrieAction::TrieInsertAction(TrieInsertAction::TrieInsertBinaryConsume(
                action,
            )) => assert_eq!(action.continuations, vec![new]),
            other => panic!("expected consume update, got {:?}", other),
        }
    }

    #[test]
    fn compute_trie_actions_deletes_consumes_and_joins_together() {
        let channel = Blake2b256Hash::from_bytes(vec![0x04; 32]);
        let consume_channels = vec![channel.clone()];
        let pointer = stable_hash_provider::hash_from_hashes(&consume_channels);
        let continuation = vec![0x30];
        let join = vec![0x40];
        let changes = StateChange::from_parts(
            HashMap::new(),
            HashMap::from([(consume_channels.clone(), ChannelChange {
                added: vec![],
                removed: vec![continuation.clone()],
            })]),
            HashMap::from([(consume_channels, join.clone())]),
        );
        let reader: Box<dyn HistoryReader<Blake2b256Hash, (), (), (), ()>> =
            Box::new(StubHistoryReaderBinary {
                continuations_map: HashMap::from([(pointer, vec![continuation])]),
                joins_map: HashMap::from([(channel, vec![join])]),
                ..Default::default()
            });

        let actions =
            compute_trie_actions(&changes, &reader, &BTreeMap::new(), |_, _, _| Ok(None)).unwrap();

        assert_eq!(actions.len(), 2);
        assert!(matches!(
            actions[0],
            HotStoreTrieAction::TrieDeleteAction(TrieDeleteAction::TrieDeleteConsume(_))
        ));
        assert!(matches!(
            actions[1],
            HotStoreTrieAction::TrieDeleteAction(TrieDeleteAction::TrieDeleteJoins(_))
        ));
    }

    #[test]
    fn compute_trie_actions_rejects_empty_consume_changes_and_missing_join_data() {
        let channel = Blake2b256Hash::from_bytes(vec![0x05; 32]);
        let consume_channels = vec![channel];
        let reader: Box<dyn HistoryReader<Blake2b256Hash, (), (), (), ()>> =
            Box::new(StubHistoryReaderBinary::default());

        let noop = StateChange::from_parts(
            HashMap::new(),
            HashMap::from([(consume_channels.clone(), ChannelChange::empty())]),
            HashMap::new(),
        );
        let noop_result =
            compute_trie_actions(&noop, &reader, &BTreeMap::new(), |_, _, _| Ok(None));
        assert!(matches!(noop_result, Err(HistoryError::MergeError(_))));

        let missing_join = StateChange::from_parts(
            HashMap::new(),
            HashMap::from([(consume_channels, ChannelChange {
                added: vec![vec![0x50]],
                removed: vec![],
            })]),
            HashMap::new(),
        );
        let missing_join_result =
            compute_trie_actions(&missing_join, &reader, &BTreeMap::new(), |_, _, _| Ok(None));
        assert!(matches!(missing_join_result, Err(HistoryError::MergeError(_))));
    }

    #[test]
    fn compute_trie_actions_uses_number_channel_override_and_propagates_its_error() {
        let channel = Blake2b256Hash::from_bytes(vec![0x06; 32]);
        let changes = StateChange::from_parts(
            HashMap::from([(channel.clone(), ChannelChange {
                added: vec![vec![0x60]],
                removed: vec![],
            })]),
            HashMap::new(),
            HashMap::new(),
        );
        let reader: Box<dyn HistoryReader<Blake2b256Hash, (), (), (), ()>> =
            Box::new(StubHistoryReaderBinary::default());

        let actions = compute_trie_actions(&changes, &reader, &BTreeMap::new(), |hash, _, _| {
            Ok(Some(HotStoreTrieAction::TrieDeleteAction(TrieDeleteAction::TrieDeleteProduce(
                TrieDeleteProduce { hash: hash.clone() },
            ))))
        })
        .unwrap();
        assert_eq!(actions.len(), 1);
        assert!(matches!(
            actions[0],
            HotStoreTrieAction::TrieDeleteAction(TrieDeleteAction::TrieDeleteProduce(_))
        ));

        let error: Result<Vec<HotStoreTrieAction<(), (), (), ()>>, _> =
            compute_trie_actions(&changes, &reader, &BTreeMap::new(), |_, _, _| {
                Err(HistoryError::MergeError("override failed".to_string()))
            });
        assert!(matches!(error, Err(HistoryError::MergeError(_))));
    }

    #[test]
    fn compute_trie_actions_orders_datum_actions_by_channel_hash() {
        let low = Blake2b256Hash::from_bytes(vec![0x01; 32]);
        let high = Blake2b256Hash::from_bytes(vec![0xfe; 32]);
        let changes = StateChange::from_parts(
            HashMap::from([
                (high.clone(), ChannelChange {
                    added: vec![vec![0x70]],
                    removed: vec![],
                }),
                (low.clone(), ChannelChange {
                    added: vec![vec![0x80]],
                    removed: vec![],
                }),
            ]),
            HashMap::new(),
            HashMap::new(),
        );
        let reader: Box<dyn HistoryReader<Blake2b256Hash, (), (), (), ()>> =
            Box::new(StubHistoryReaderBinary::default());

        let actions =
            compute_trie_actions(&changes, &reader, &BTreeMap::new(), |_, _, _| Ok(None)).unwrap();
        let hashes = actions
            .iter()
            .map(|action| match action {
                HotStoreTrieAction::TrieInsertAction(
                    TrieInsertAction::TrieInsertBinaryProduce(action),
                ) => action.hash.clone(),
                other => panic!("expected produce insertion, got {:?}", other),
            })
            .collect::<Vec<_>>();
        assert_eq!(hashes, vec![low, high]);
    }
}
