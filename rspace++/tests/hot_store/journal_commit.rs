use super::*;
use crate::rspace::history::history_reader::HistoryReaderBase;
use crate::rspace::internal::{AcceptedDatumChanges, Datum, WaitingContinuation};
use crate::rspace::persistence::{CountTransition, ObjectDigest, ObjectRefV1, StateVersionV1};

struct EmptyHistory;

impl HistoryReaderBase<String, String, String, String> for EmptyHistory {
    fn get_data_canonical(&self, _key: &String) -> Vec<Datum<String>> { Vec::new() }

    fn get_continuations_canonical(
        &self,
        _key: &[String],
    ) -> Vec<WaitingContinuation<String, String>> {
        Vec::new()
    }

    fn get_joins_canonical(&self, _key: &String) -> Vec<Vec<String>> { Vec::new() }
}

fn datum(value: &str) -> Datum<String> {
    Datum::create(&"channel".to_owned(), value.to_owned(), false)
}

fn base_version() -> StateVersionV1 {
    StateVersionV1::new(
        0,
        ObjectRefV1::new(ObjectDigest::of_canonical_bytes(&[0; 32]), 32).unwrap(),
    )
}

fn populated_store() -> Box<dyn HotStore<String, String, String, String>> {
    let store =
        HotStoreInstances::create_from_hs_and_hr(HotStoreState::default(), Box::new(EmptyHistory));
    populate(store.as_ref());
    store
}

fn populate(store: &dyn HotStore<String, String, String, String>) {
    let channels = vec!["channel".to_owned(), "channel".to_owned()];
    let continuation = WaitingContinuation::create(
        &channels,
        &vec!["first".to_owned(), "second".to_owned()],
        &"body".to_owned(),
        false,
        BTreeSet::new(),
    );
    store.put_datum(&channels[0], datum("value"));
    assert_eq!(store.put_continuation(&channels, continuation), Some(true));
    store.put_join(&channels[0], &channels).unwrap();
}

fn durable_catalog() -> CatalogSlotV1 {
    use crate::rspace::persistence::{
        CatalogPackStateV1, INDEX_HEADER_LEN_V1, LogPackAddress, LogPackSequence,
        PACK_HEADER_LEN_V1, PackId, PhysicalDigest, SemanticProfileId, StoreId,
        TRANSACTION_COMMIT_RECORD_LEN_V1,
    };
    let address = LogPackAddress::from_pack(
        PackId::GENESIS,
        PACK_HEADER_LEN_V1,
        TRANSACTION_COMMIT_RECORD_LEN_V1,
    )
    .unwrap();
    let pack = CatalogPackStateV1::new(
        1,
        1,
        address.end(),
        1,
        INDEX_HEADER_LEN_V1,
        base_version().manifest(),
        32,
        32,
    )
    .unwrap();
    CatalogSlotV1::new(
        StoreId::from_array([7; 16]),
        1,
        base_version(),
        SemanticProfileId::from_array(*b"RSpace.Test.V1\0\0"),
        PhysicalDigest::of_bytes(b"base"),
        LogPackSequence::FIRST,
        address,
        pack,
    )
    .unwrap()
}

fn next_catalog(base: CatalogSlotV1) -> CatalogSlotV1 {
    use crate::rspace::persistence::{
        CatalogPackStateV1, LogPackAddress, PackId, PhysicalDigest,
        TRANSACTION_COMMIT_RECORD_LEN_V1,
    };
    let generation = base.state().generation() + 1;
    let reference =
        ObjectRefV1::new(ObjectDigest::of_canonical_bytes(&generation.to_le_bytes()), 32).unwrap();
    let address = LogPackAddress::from_pack(
        PackId::GENESIS,
        base.pack().durable_pack_tail() + 64,
        TRANSACTION_COMMIT_RECORD_LEN_V1,
    )
    .unwrap();
    let pack = CatalogPackStateV1::new(
        1,
        1,
        address.end(),
        1,
        base.pack().durable_index_tail(),
        reference,
        32,
        32,
    )
    .unwrap();
    base.successor(
        base.state(),
        StateVersionV1::new(generation, reference),
        PhysicalDigest::of_bytes(&generation.to_le_bytes()),
        base.commit_sequence().successor().unwrap(),
        address,
        pack,
    )
    .unwrap()
}

fn durable_populated_store() -> (InMemHotStore<String, String, String, String>, CatalogSlotV1) {
    let base = durable_catalog();
    let mut state = InMemHotStoreState::default();
    state.durable_journal =
        Some(Box::new(DurableJournalStateV1::new(base, GenerationLeaseRegistryV1::default())));
    let store = InMemHotStore {
        state: std::sync::RwLock::new(state),
        history_cache: std::sync::RwLock::new(HistoryStoreCache::default()),
        history_reader_base: Box::new(EmptyHistory),
    };
    populate(&store);
    (store, base)
}

fn assert_pending_empty(pending: PendingDeltas) {
    assert!(pending.data.unwrap().is_empty());
    assert!(pending.continuations.unwrap().is_empty());
    assert!(pending.continuation_content.unwrap().is_empty());
    assert!(pending.continuation_source_bytes.unwrap().is_empty());
    assert!(pending.joins.unwrap().is_empty());
}

#[test]
fn durable_acknowledgement_is_exact_atomic_and_idempotent() {
    let (store, base) = durable_populated_store();
    let captured = store.capture_pending_journal(base).unwrap();
    let before = store.snapshot();
    let target = next_catalog(base);
    assert!(
        store
            .commit_pending_deltas(&captured.observation.pending)
            .is_err()
    );
    assert_eq!(
        store
            .acknowledge_pending_journal(&captured, target, target)
            .unwrap(),
        HotJournalAcknowledgementV1::Committed
    );
    assert_pending_empty(store.pending_deltas().unwrap());
    let after = store.snapshot();
    assert_eq!(before.data, after.data);
    assert_eq!(before.continuations, after.continuations);
    assert_eq!(before.joins, after.joins);
    assert_eq!(before.installed_continuations, after.installed_continuations);
    assert_eq!(before.installed_joins, after.installed_joins);
    let current = store
        .state
        .read()
        .unwrap()
        .durable_journal
        .as_ref()
        .unwrap()
        .version
        .clone();
    for _ in 0..4 {
        assert_eq!(
            store
                .acknowledge_pending_journal(&captured, target, target)
                .unwrap(),
            HotJournalAcknowledgementV1::AlreadyCommitted
        );
        assert!(
            current.matches(
                &store
                    .state
                    .read()
                    .unwrap()
                    .durable_journal
                    .as_ref()
                    .unwrap()
                    .version
            )
        );
    }
    assert!(store.capture_pending_journal(base).is_err());
    assert!(store.capture_pending_journal(target).is_ok());
}

#[test]
fn durable_acknowledgement_rejects_foreign_receipts_and_every_changed_domain() {
    let (store, base) = durable_populated_store();
    let (foreign, same_base) = durable_populated_store();
    assert_eq!(base, same_base);
    let captured = store.capture_pending_journal(base).unwrap();
    let foreign_receipt = foreign.capture_pending_journal(base).unwrap();
    let target = next_catalog(base);
    assert_same_pending(&captured.observation.pending, &foreign_receipt.observation.pending);
    assert!(
        store
            .acknowledge_pending_journal(&foreign_receipt, target, target)
            .is_err()
    );
    for omitted in [false, true] {
        for domain in 0..5 {
            let mut wrong = store.capture_pending_journal(base).unwrap();
            match domain {
                0 => wrong.observation.pending.data = (!omitted).then(BagDelta::new),
                1 => wrong.observation.pending.continuations = (!omitted).then(BagDelta::new),
                2 => wrong.observation.pending.continuation_content = (!omitted).then(PathMap::new),
                3 => {
                    wrong.observation.pending.continuation_source_bytes =
                        (!omitted).then(PathMap::new)
                }
                4 => wrong.observation.pending.joins = (!omitted).then(BagDelta::new),
                _ => unreachable!(),
            }
            assert!(
                store
                    .acknowledge_pending_journal(&wrong, target, target)
                    .is_err()
            );
            assert_same_pending(&captured.observation.pending, &store.pending_deltas().unwrap());
        }
    }
    let competing = store.capture_pending_journal(base).unwrap();
    store
        .acknowledge_pending_journal(&captured, target, target)
        .unwrap();
    assert!(
        store
            .acknowledge_pending_journal(&competing, target, target)
            .is_err()
    );
    assert!(
        store
            .acknowledge_pending_journal(&foreign_receipt, target, target)
            .is_err()
    );
    assert_pending_empty(store.pending_deltas().unwrap());
}

#[test]
fn durable_acknowledgement_rejects_wrong_terminal_before_and_after_commit() {
    use crate::rspace::persistence::{
        CatalogPackStateV1, INDEX_RECORD_LEN_V1, PhysicalDigest, SemanticProfileId, StoreId,
    };
    let (store, base) = durable_populated_store();
    let captured = store.capture_pending_journal(base).unwrap();
    let target = next_catalog(base);
    for acknowledged in [false, true] {
        if acknowledged {
            store
                .acknowledge_pending_journal(&captured, target, target)
                .unwrap();
        }
        let pending = store.pending_deltas().unwrap();
        for field in 0..6 {
            let mut pack = target.pack();
            if field == 5 {
                pack = CatalogPackStateV1::new(
                    pack.first_live_pack(),
                    pack.active_pack(),
                    pack.durable_pack_tail(),
                    pack.index_generation(),
                    pack.durable_index_tail() + INDEX_RECORD_LEN_V1,
                    pack.retention_manifest(),
                    pack.current_state_canonical_bytes(),
                    pack.retained_canonical_bytes(),
                )
                .unwrap();
            }
            let wrong = CatalogSlotV1::new(
                if field == 0 {
                    StoreId::from_array([8; 16])
                } else {
                    target.store_id()
                },
                if field == 1 {
                    target.epoch() + 1
                } else {
                    target.epoch()
                },
                target.state(),
                if field == 2 {
                    SemanticProfileId::from_array([9; 16])
                } else {
                    target.profile_id()
                },
                if field == 3 {
                    PhysicalDigest::of_bytes(b"other")
                } else {
                    target.commit_digest()
                },
                if field == 4 {
                    target.commit_sequence().successor().unwrap()
                } else {
                    target.commit_sequence()
                },
                target.commit_address(),
                pack,
            )
            .unwrap();
            assert!(
                store
                    .acknowledge_pending_journal(&captured, target, wrong)
                    .is_err()
            );
            assert!(
                store
                    .acknowledge_pending_journal(&captured, wrong, target)
                    .is_err()
            );
            assert_same_pending(&pending, &store.pending_deltas().unwrap());
        }
    }
}

#[test]
fn durable_acknowledgement_cannot_reuse_identical_journals_after_restore_or_mutation() {
    let (store, base) = durable_populated_store();
    let captured = store.capture_pending_journal(base).unwrap();
    let pending = captured.observation.pending.clone();
    store
        .restore_pending_deltas(
            pending.data.unwrap(),
            pending.continuations.unwrap(),
            pending.continuation_content.unwrap(),
            pending.continuation_source_bytes.unwrap(),
            pending.joins.unwrap(),
        )
        .unwrap();
    assert_same_pending(&captured.observation.pending, &store.pending_deltas().unwrap());
    let target = next_catalog(base);
    assert!(
        store
            .acknowledge_pending_journal(&captured, target, target)
            .is_err()
    );
    let fresh = store.capture_pending_journal(base).unwrap();
    store
        .acknowledge_pending_journal(&fresh, target, target)
        .unwrap();
    let added = datum("temporary");
    let path = RSpaceDataIndex::datum_path(&"channel".to_owned(), &added);
    store.put_datum(&"channel".to_owned(), added);
    store.remove_datum_path(&path).unwrap();
    assert_pending_empty(store.pending_deltas().unwrap());
    assert!(
        store
            .acknowledge_pending_journal(&fresh, target, target)
            .is_err()
    );
}

#[test]
fn durable_base_is_invalidated_by_clear_or_state_replacement() {
    for replace in [false, true] {
        let (store, base) = durable_populated_store();
        let captured = store.capture_pending_journal(base).unwrap();
        if replace {
            store.set_state(store.snapshot());
        } else {
            store.clear();
        }
        assert!(store.capture_pending_journal(base).is_err());
        let target = next_catalog(base);
        assert!(
            store
                .acknowledge_pending_journal(&captured, target, target)
                .is_err()
        );
    }
}

#[test]
fn every_durable_mutator_invalidates_retained_journal_versions() {
    for operation in 0..12 {
        let (store, base) = durable_populated_store();
        let channel = "channel".to_owned();
        let channels = vec![channel.clone(), channel.clone()];
        if operation == 9 {
            store.remove_continuation(&channels, 0).unwrap();
        }
        let captured = store.capture_pending_journal(base).unwrap();
        let path = RSpaceDataIndex::datum_path(&channel, &datum("value"));
        let mut removal = BagDelta::new();
        removal
            .record_removal(&path, crate::rspace::bag::Multiplicity::ONE)
            .unwrap();
        match operation {
            0 => store.put_datum(&channel, datum("new")),
            1 => store.remove_datum(&channel, 0).unwrap(),
            2 => store.remove_datum_path(&path).unwrap(),
            3 => store.apply_datum_delta(&removal).unwrap(),
            4 | 5 => {
                let continuation = WaitingContinuation::create(
                    &channels,
                    &vec!["first".to_owned(), "second".to_owned()],
                    &"new body".to_owned(),
                    false,
                    BTreeSet::new(),
                );
                if operation == 4 {
                    assert_eq!(store.put_continuation(&channels, continuation), Some(true));
                } else {
                    store.install_continuation(&channels, continuation).unwrap();
                }
            }
            6 => store.remove_continuation(&channels, 0).unwrap(),
            7 => store.put_join(&channel, &[channel.clone()]).unwrap(),
            8 => store.install_join(&channel, &channels).unwrap(),
            9 => store.remove_join(&channel, &channels).unwrap(),
            10 | 11 => {
                let continuation = if operation == 11 {
                    let (version, records) = store.get_continuations_versioned(&channels);
                    let selected = &records[0];
                    Some(StoredContinuationCommit {
                        address: ContinuationAddress::new(version, 0),
                        source: selected.source.clone(),
                        persist: selected.persist,
                        peeks: selected.peeks.clone(),
                    })
                } else {
                    None
                };
                let outcome = store
                    .apply_accepted_match(AcceptedMatch {
                        channels: channels.clone(),
                        continuation,
                        datum_delta: AcceptedDatumChanges::from_removals(if operation == 10 {
                            removal
                        } else {
                            BagDelta::new()
                        })
                        .unwrap(),
                        required_persistent_paths: Vec::new(),
                        join_channels: vec![channel.clone()],
                    })
                    .unwrap();
                assert!(matches!(outcome, AcceptedMatchOutcome::Committed { .. }));
            }
            _ => unreachable!(),
        }
        let current = store.capture_pending_journal(base).unwrap();
        assert!(!captured.version.matches(&current.version), "operation {operation}");
        let target = next_catalog(base);
        assert!(
            store
                .acknowledge_pending_journal(&captured, target, target)
                .is_err()
        );
        assert_same_pending(&current.observation.pending, &store.pending_deltas().unwrap());
    }
}

#[test]
fn durable_reads_noops_and_refused_mutations_preserve_the_capture() {
    let (store, base) = durable_populated_store();
    let channel = "channel".to_owned();
    let channels = vec![channel.clone(), channel.clone()];
    let captured = store.capture_pending_journal(base).unwrap();
    let continuation = store.get_continuations(&channels)[0].clone();
    assert_eq!(store.put_continuation(&channels, continuation), Some(false));
    store.put_join(&channel, &channels).unwrap();
    store.get_data(&"cold data".to_owned());
    store.get_candidate_pool(&"cold pool".to_owned());
    store.get_continuations(&["cold continuation".to_owned()]);
    store.get_joins(&"cold join".to_owned());
    assert!(store.remove_datum(&channel, 99).is_err());
    let absent = RSpaceDataIndex::datum_path(&channel, &datum("absent"));
    assert!(store.remove_datum_path(&absent).is_err());
    assert!(store.remove_continuation(&channels, 99).is_none());
    store.apply_datum_delta(&BagDelta::new()).unwrap();
    let mut impossible = BagDelta::new();
    impossible
        .record_removal(&absent, crate::rspace::bag::Multiplicity::ONE)
        .unwrap();
    assert!(store.apply_datum_delta(&impossible).is_err());
    let outcome = store
        .apply_accepted_match(AcceptedMatch {
            channels,
            continuation: None,
            datum_delta: AcceptedDatumChanges::from_removals(impossible).unwrap(),
            required_persistent_paths: Vec::new(),
            join_channels: Vec::new(),
        })
        .unwrap();
    assert!(matches!(outcome, AcceptedMatchOutcome::Stale));
    let current = store.capture_pending_journal(base).unwrap();
    assert!(captured.version.matches(&current.version));
    assert_same_pending(&captured.observation.pending, &current.observation.pending);
    let target = next_catalog(base);
    store
        .acknowledge_pending_journal(&captured, target, target)
        .unwrap();
}

#[test]
fn an_empty_durable_journal_cannot_fabricate_a_successor() {
    let (store, base) = durable_populated_store();
    let captured = store.capture_pending_journal(base).unwrap();
    let target = next_catalog(base);
    store
        .acknowledge_pending_journal(&captured, target, target)
        .unwrap();
    let empty = store.capture_pending_journal(target).unwrap();
    let fabricated = next_catalog(target);
    assert!(
        store
            .acknowledge_pending_journal(&empty, fabricated, fabricated)
            .is_err()
    );
    assert!(store.capture_pending_journal(target).is_ok());
    assert_pending_empty(store.pending_deltas().unwrap());
    assert_eq!(
        store
            .acknowledge_pending_journal(&captured, target, target)
            .unwrap(),
        HotJournalAcknowledgementV1::AlreadyCommitted
    );
}

#[test]
fn journal_version_rollover_retains_fresh_identity_while_old_receipts_live() {
    let mut current = JournalVersionV1::new();
    current.revision = u64::MAX;
    let old = current.clone();
    current = current.next();
    assert_eq!(current.revision, 0);
    assert!(!Arc::ptr_eq(&old.identity, &current.identity));
    assert!(!old.matches(&current));
    let next = current.next();
    assert_eq!(next.revision, 1);
    assert!(Arc::ptr_eq(&current.identity, &next.identity));
}

proptest! {
    #[test]
    fn durable_journal_checkpoint_histories_preserve_live_state_and_reject_old_receipts(
        restores in proptest::collection::vec(any::<bool>(), 1..24), near_rollover in any::<bool>(),
    ) {
        let (store, mut base) = durable_populated_store();
        if near_rollover {
            store.state.write().unwrap().durable_journal.as_mut().unwrap().version.revision = u64::MAX - 1;
        }
        let mut held = Vec::new();
        for restore in restores {
            store.put_datum(&"channel".to_owned(), datum("history"));
            for (receipt, terminal) in &held {
                prop_assert!(store.acknowledge_pending_journal(receipt, *terminal, *terminal).is_err());
            }
            if restore {
                let stale = store.capture_pending_journal(base).unwrap();
                let pending = store.pending_deltas().unwrap();
                store.restore_pending_deltas(pending.data.unwrap(), pending.continuations.unwrap(), pending.continuation_content.unwrap(), pending.continuation_source_bytes.unwrap(), pending.joins.unwrap()).unwrap();
                let target = next_catalog(base);
                prop_assert!(store.acknowledge_pending_journal(&stale, target, target).is_err());
                held.push((stale, target));
            }
            let receipt = store.capture_pending_journal(base).unwrap();
            let before = store.snapshot();
            prop_assert!(store.get_data(&"unrelated cold channel".to_owned()).is_empty());
            let target = next_catalog(base);
            prop_assert_eq!(store.acknowledge_pending_journal(&receipt, target, target).unwrap(), HotJournalAcknowledgementV1::Committed);
            prop_assert_eq!(store.acknowledge_pending_journal(&receipt, target, target).unwrap(), HotJournalAcknowledgementV1::AlreadyCommitted);
            assert_pending_empty(store.pending_deltas().unwrap());
            let after = store.snapshot();
            prop_assert_eq!(before.data.get("channel"), after.data.get("channel"));
            prop_assert_eq!(before.continuations, after.continuations);
            prop_assert_eq!(before.joins, after.joins);
            held.push((receipt, target));
            base = target;
        }
    }

    #[test]
    fn journal_version_histories_never_reuse_a_retained_version(start in any::<u64>(), steps in 1usize..128) {
        let mut current = JournalVersionV1::new();
        current.revision = start;
        let mut held = vec![current.clone()];
        for _ in 0..steps {
            current = current.next();
            prop_assert!(held.iter().all(|prior| !current.matches(prior)));
            held.push(current.clone());
        }
    }
}

fn assert_same_pending(left: &PendingDeltas, right: &PendingDeltas) {
    assert_eq!(left.data, right.data);
    assert_eq!(left.continuations, right.continuations);
    assert_eq!(left.joins, right.joins);
    assert!(
        left.continuation_content.as_ref().unwrap().iter().eq(right
            .continuation_content
            .as_ref()
            .unwrap()
            .iter())
    );
    assert!(
        left.continuation_source_bytes
            .as_ref()
            .unwrap()
            .iter()
            .eq(right.continuation_source_bytes.as_ref().unwrap().iter())
    );
}

#[test]
fn acknowledgement_clears_five_populated_journals_and_preserves_live_records() {
    let store = populated_store();
    let pending = store.pending_deltas().unwrap();
    assert!(!pending.data.as_ref().unwrap().is_empty());
    assert!(!pending.continuations.as_ref().unwrap().is_empty());
    assert!(!pending.continuation_content.as_ref().unwrap().is_empty());
    assert!(
        !pending
            .continuation_source_bytes
            .as_ref()
            .unwrap()
            .is_empty()
    );
    assert!(!pending.joins.as_ref().unwrap().is_empty());
    let before = store.snapshot();
    store.commit_pending_deltas(&pending).unwrap();
    let cleared = store.pending_deltas().unwrap();
    assert!(cleared.data.unwrap().is_empty());
    assert!(cleared.continuations.unwrap().is_empty());
    assert!(cleared.continuation_content.unwrap().is_empty());
    assert!(cleared.continuation_source_bytes.unwrap().is_empty());
    assert!(cleared.joins.unwrap().is_empty());
    let after = store.snapshot();
    assert_eq!(before.data, after.data);
    assert_eq!(before.continuations, after.continuations);
    assert_eq!(before.joins, after.joins);
    assert_eq!(before.installed_continuations, after.installed_continuations);
    assert_eq!(before.installed_joins, after.installed_joins);
}

#[test]
fn mismatch_or_missing_capability_in_any_journal_rejects_without_partial_clear() {
    let store = populated_store();
    let pending = store.pending_deltas().unwrap();
    for omitted in [false, true] {
        for namespace in 0..5 {
            let mut wrong = pending.clone();
            match namespace {
                0 => wrong.data = (!omitted).then(BagDelta::new),
                1 => wrong.continuations = (!omitted).then(BagDelta::new),
                2 => wrong.continuation_content = (!omitted).then(PathMap::new),
                3 => wrong.continuation_source_bytes = (!omitted).then(PathMap::new),
                4 => wrong.joins = (!omitted).then(BagDelta::new),
                _ => unreachable!(),
            }
            assert!(matches!(
                store.commit_pending_deltas(&wrong),
                Err(RSpaceError::JournalConflict(_))
            ));
            assert_same_pending(&pending, &store.pending_deltas().unwrap());
        }
    }
}

#[test]
fn join_journal_uses_native_key_and_repeated_channel_positions() {
    use crate::rspace::serializers::serializers::encode_join_record;
    let store = populated_store();
    let channels = vec!["channel".to_owned(), "channel".to_owned()];
    store.put_join(&channels[0], &channels).unwrap();
    let mut native_path = RhoPathV1::join_prefix(&channels[0]);
    let canonical = encode_join_record(&channels);
    native_path.extend_from_slice(&canonical);
    let observation = store.pending_mutation_observation().unwrap();
    let joins = observation.pending.joins.as_ref().unwrap();
    assert_eq!(joins.additions().count(&native_path), 1);
    assert_eq!(observation.join_counts.get(&native_path), Some(&1));
    assert_eq!(RhoPathV1::parse_join(&native_path).unwrap().join_identity(), canonical);
    let batch = mutation_batch_from_observation(base_version(), &observation).unwrap();
    assert_eq!(batch.join_transition(&native_path), Some(CountTransition::new(0, 1).unwrap()));
    store.commit_pending_deltas(&observation.pending).unwrap();
    store.remove_continuation(&channels, 0).unwrap();
    store.remove_join(&channels[0], &channels).unwrap();
    let removal = store.pending_mutation_observation().unwrap();
    assert_eq!(
        removal
            .pending
            .joins
            .as_ref()
            .unwrap()
            .removals()
            .count(&native_path),
        1
    );
    assert_eq!(removal.join_counts.get(&native_path), Some(&0));
    let batch = mutation_batch_from_observation(base_version(), &removal).unwrap();
    assert_eq!(batch.join_transition(&native_path), Some(CountTransition::new(1, 0).unwrap()));
}

#[test]
fn absent_count_evidence_is_not_a_captured_zero() {
    let store = populated_store();
    store
        .commit_pending_deltas(&store.pending_deltas().unwrap())
        .unwrap();
    store.remove_datum(&"channel".to_owned(), 0).unwrap();
    let mut observation = store.pending_mutation_observation().unwrap();
    assert!(mutation_batch_from_observation(base_version(), &observation).is_ok());
    observation.data_counts = PathMap::new();
    assert!(mutation_batch_from_observation(base_version(), &observation).is_err());
}

proptest! {
    #[test]
    fn journal_count_inversion_matches_live_multiplicities(
        before in 0usize..16, after in 0usize..16,
    ) {
        let mut state = HotStoreState::default();
        if before != 0 {
            state.data.insert("channel".to_owned(), vec![datum("value"); before]);
        }
        let store = HotStoreInstances::create_from_hs_and_hr(state, Box::new(EmptyHistory));
        let path = RSpaceDataIndex::datum_path(&"channel".to_owned(), &datum("value"));
        for _ in before..after { store.put_datum(&"channel".to_owned(), datum("value")); }
        for _ in after..before { store.remove_datum(&"channel".to_owned(), 0).unwrap(); }
        let observation = store.pending_mutation_observation().unwrap();
        if before == after {
            prop_assert!(matches!(mutation_batch_from_observation(base_version(), &observation),
                Err(MutationError::EmptyBatch)));
        } else {
            prop_assert_eq!(observation.data_counts.get(&path), Some(&(after as u64)));
            let batch = mutation_batch_from_observation(base_version(), &observation).unwrap();
            prop_assert_eq!(batch.data_transition(&path),
                Some(crate::rspace::persistence::CountTransition::new(before as u64, after as u64).unwrap()));
        }
    }
}

#[test]
fn committed_journal_clears_only_the_exact_revision() {
    let store = HotStoreInstances::create_from_hs_and_hr::<String, String, String, String>(
        HotStoreState::default(),
        Box::new(EmptyHistory),
    );
    // The first write populates a cold channel from history; the second write
    // exercises the resident PathMap mutation/journal path.
    store.put_datum(&"channel".to_owned(), datum("seed"));
    store.put_datum(&"channel".to_owned(), datum("value"));
    let snapshot = store.pending_deltas().expect("journal snapshot");
    let before = store.snapshot();

    store
        .commit_pending_deltas(&snapshot)
        .expect("matching journal commits");

    assert!(store.pending_deltas().unwrap().data.unwrap().is_empty());
    assert!(
        store
            .pending_deltas()
            .unwrap()
            .continuations
            .unwrap()
            .is_empty()
    );
    assert!(store.pending_deltas().unwrap().joins.unwrap().is_empty());
    let after = store.snapshot();
    assert_eq!(after.data, before.data, "data contents are unchanged");
    assert_eq!(after.continuations, before.continuations, "continuations unchanged");
    assert_eq!(after.joins, before.joins, "joins unchanged");
    assert_eq!(
        after.installed_continuations, before.installed_continuations,
        "installed continuations unchanged"
    );
    assert_eq!(after.installed_joins, before.installed_joins, "installed joins unchanged");
}

#[test]
fn stale_journal_cannot_clear_a_newer_revision() {
    let store = HotStoreInstances::create_from_hs_and_hr::<String, String, String, String>(
        HotStoreState::default(),
        Box::new(EmptyHistory),
    );
    store.put_datum(&"channel".to_owned(), datum("seed"));
    store.put_datum(&"channel".to_owned(), datum("old"));
    let stale = store.pending_deltas().expect("journal snapshot");
    store.put_datum(&"channel".to_owned(), datum("new"));

    let error = store
        .commit_pending_deltas(&stale)
        .expect_err("stale journal must be rejected");
    assert!(matches!(error, RSpaceError::JournalConflict(_)));
    assert!(!store.pending_deltas().unwrap().data.unwrap().is_empty());
}

#[test]
fn first_write_to_a_cold_channel_is_journaled() {
    let store = HotStoreInstances::create_from_hs_and_hr::<String, String, String, String>(
        HotStoreState::default(),
        Box::new(EmptyHistory),
    );
    store.put_datum(&"channel".to_owned(), datum("first"));
    let pending = store.pending_deltas().expect("journal snapshot");
    assert_eq!(pending.data.as_ref().unwrap().additions().distinct_len(), 1);
    assert!(pending.data.as_ref().unwrap().removals().is_empty());
}

#[test]
fn mutation_adapter_reads_counts_from_resident_pathmaps() {
    let store = HotStoreInstances::create_from_hs_and_hr::<String, String, String, String>(
        HotStoreState::default(),
        Box::new(EmptyHistory),
    );
    store.put_datum(&"channel".to_owned(), datum("seed"));
    store.put_datum(&"channel".to_owned(), datum("value"));
    let observation = store
        .pending_mutation_observation()
        .expect("coherent journal/count observation");
    let path = observation
        .pending
        .data
        .as_ref()
        .unwrap()
        .additions()
        .iter()
        .next()
        .expect("one data path")
        .0;
    assert_eq!(store.data_count_at_path(&path).unwrap(), 1);
    assert_eq!(store.join_count_at_path(&path).unwrap(), 0);

    let base_bytes = [0_u8; 32];
    let base = StateVersionV1::new(
        0,
        ObjectRefV1::new(ObjectDigest::of_canonical_bytes(&base_bytes), base_bytes.len() as u64)
            .unwrap(),
    );
    assert_eq!(observation.data_counts.get(&path), Some(&1));
    let batch = mutation_batch_from_observation(base, &observation)
        .expect("resident lookup adapter builds an exact batch");
    assert_eq!(
        batch.data_transition(&path),
        Some(CountTransition::new(0, 1).unwrap()),
        "the pending addition is reconstructed from the current resident count"
    );
}
