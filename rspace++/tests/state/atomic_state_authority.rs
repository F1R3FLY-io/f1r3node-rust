use std::collections::BTreeMap;
use std::sync::Arc;

use shared::rust::test_scratch;

use super::{AuthorityBackend, CURRENT_ROOT_KEY, RSpaceStateAuthority, verified_root_key};
use crate::rspace::errors::{HistoryError, RootError};
use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use crate::rspace::history::cold_store::{
    ColdLeafAddress, DataLeaf, PersistedData, PersistedDataKind, PersistedDataView,
};
use crate::rspace::history::history::{History, HistoryInstances};
use crate::rspace::history::history_action::{HistoryAction, InsertAction};
use crate::rspace::history::instances::radix_history::PreparedHistoryCheckpoint;
use crate::rspace::history::roots_store::CurrentRoot;
use crate::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;
use crate::rspace::state::closure_verifier::RecordValidator;

#[derive(Debug, PartialEq, Eq)]
struct MemorySnapshot {
    history: BTreeMap<Vec<u8>, Vec<u8>>,
    cold: BTreeMap<Vec<u8>, Vec<u8>>,
    roots: BTreeMap<Vec<u8>, Vec<u8>>,
}

fn memory_snapshot(authority: &RSpaceStateAuthority) -> MemorySnapshot {
    let AuthorityBackend::Memory(state) = &authority.backend else {
        panic!("test requires the in-memory authority");
    };
    let state = state.lock().expect("authority state lock");
    MemorySnapshot {
        history: state.history.clone(),
        cold: state.cold.clone(),
        roots: state.roots.clone(),
    }
}

fn validator() -> RecordValidator { Arc::new(|_, _| Ok(())) }

fn data_leaf(label: &[u8]) -> (ColdLeafAddress, Vec<u8>) {
    let record = bincode::serialize(&label.to_vec()).expect("record");
    let payload = bincode::serialize(&vec![record]).expect("record sequence");
    let bytes = PersistedData::Data(DataLeaf { bytes: payload }).into_legacy_bincode();
    let view = PersistedDataView::parse_canonical(&bytes)
        .expect("canonical data envelope")
        .validate_records(PersistedDataKind::Data, Some(1))
        .expect("one canonical record");
    let digest = Blake2b256Hash::new(view.leaf_encoding());
    (ColdLeafAddress::new(PersistedDataKind::Data, digest).expect("typed address"), bytes)
}

fn history_key(discriminator: u8) -> Vec<u8> {
    let mut key = vec![0];
    key.extend(std::iter::repeat_n(discriminator, 32));
    key
}

fn history_at_current(authority: &Arc<RSpaceStateAuthority>) -> Box<dyn History> {
    Box::new(
        HistoryInstances::create_authorized(
            authority.current_root().expect("current root"),
            Arc::clone(authority),
        )
        .expect("authorized history"),
    )
}

fn prepare_insert(
    history: &dyn History,
    discriminator: u8,
    address: &ColdLeafAddress,
) -> (Box<dyn History>, PreparedHistoryCheckpoint) {
    history
        .prepare_checkpoint(vec![HistoryAction::Insert(InsertAction {
            key: history_key(discriminator),
            hash: address.digest().clone(),
        })])
        .expect("prepare checkpoint")
}

fn assert_root_objects_visible(
    authority: &Arc<RSpaceStateAuthority>,
    root: &CurrentRoot,
    prepared: &PreparedHistoryCheckpoint,
    cold: &ColdLeafAddress,
) {
    assert_eq!(authority.current_verified_root().unwrap(), Some(root.clone()));
    assert!(authority.contains_verified_root(root.root()).unwrap());
    assert_eq!(
        authority
            .history_store()
            .get_one(&root.root().bytes())
            .unwrap()
            .as_deref(),
        prepared.root_encoding()
    );
    assert!(
        authority
            .cold_store()
            .get_one(&cold.encode())
            .unwrap()
            .is_some()
    );
}

#[test]
fn memory_checkpoint_publishes_all_namespaces_together() {
    let authority = RSpaceStateAuthority::in_memory();
    let history = history_at_current(&authority);
    let (cold, bytes) = data_leaf(b"atomic-success");
    let (_next, prepared) = prepare_insert(history.as_ref(), 1, &cold);

    let current = authority
        .commit_local_checkpoint(&prepared, vec![(cold.clone(), bytes)], &validator())
        .expect("atomic checkpoint");

    assert_root_objects_visible(&authority, &current, &prepared, &cold);
}

#[test]
fn authority_facets_reject_deletion_and_authorized_history_rejects_detached_commit() {
    let authority = RSpaceStateAuthority::in_memory();
    let history = history_at_current(&authority);
    let (cold, bytes) = data_leaf(b"immutable-authority");
    let (_next, prepared) = prepare_insert(history.as_ref(), 9, &cold);
    authority
        .commit_local_checkpoint(&prepared, vec![(cold.clone(), bytes)], &validator())
        .expect("seed atomic checkpoint");

    let deletion = authority
        .cold_store()
        .delete(vec![cold.encode()])
        .expect_err("authority-owned content cannot be deleted");
    assert!(deletion.to_string().contains("cannot be deleted"));

    let authorized = history_at_current(&authority);
    assert!(matches!(
        authorized.process(Vec::new()),
        Err(HistoryError::ActionError(message))
            if message.contains("atomic checkpoint authority")
    ));
}

#[test]
fn detached_history_rejects_a_missing_nonempty_root_without_panicking() {
    let missing = Blake2b256Hash::new(b"not-a-radix-root");
    let error = match HistoryInstances::create(missing, Arc::new(InMemoryKeyValueStore::new())) {
        Ok(_) => panic!("missing nonempty root was silently treated as empty"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        HistoryError::RadixTreeError(crate::rspace::errors::RadixTreeError::KeyNotFound(_))
    ));
}

#[test]
fn no_op_writes_nothing_and_does_not_advance_generation() {
    let authority = RSpaceStateAuthority::in_memory();
    let history = history_at_current(&authority);
    let before = memory_snapshot(&authority);
    let current_before = authority.current_verified_root().unwrap().unwrap();
    let (_next, prepared) = history.prepare_checkpoint(Vec::new()).expect("no-op plan");

    let observed = authority
        .commit_local_checkpoint(&prepared, Vec::new(), &validator())
        .expect("no-op checkpoint");

    assert_eq!(observed, current_before);
    assert_eq!(authority.current_verified_root().unwrap().unwrap(), current_before);
    assert_eq!(memory_snapshot(&authority), before);
}

#[test]
fn preparing_checkpoint_leaves_authority_byte_for_byte_unchanged() {
    let authority = RSpaceStateAuthority::in_memory();
    let history = history_at_current(&authority);
    let (cold, _bytes) = data_leaf(b"prepare-is-private");
    let before = memory_snapshot(&authority);
    let current_before = authority.current_verified_root().unwrap().unwrap();

    let (_next, _prepared) = prepare_insert(history.as_ref(), 14, &cold);

    assert_eq!(authority.current_verified_root().unwrap().unwrap(), current_before);
    assert_eq!(memory_snapshot(&authority), before);
}

#[test]
fn prepared_checkpoint_cannot_cross_an_authority_boundary() {
    let origin = RSpaceStateAuthority::in_memory();
    let foreign = RSpaceStateAuthority::in_memory();
    let history = history_at_current(&origin);
    let (cold, bytes) = data_leaf(b"foreign-authority");
    let (_next, prepared) = prepare_insert(history.as_ref(), 12, &cold);
    let foreign_before = memory_snapshot(&foreign);

    let error = foreign
        .commit_local_checkpoint(&prepared, vec![(cold, bytes)], &validator())
        .expect_err("a prepared checkpoint is authority-bound");

    assert!(matches!(
        error,
        HistoryError::ActionError(message) if message.contains("different RSpace authority")
    ));
    assert_eq!(memory_snapshot(&foreign), foreign_before);
}

#[test]
fn same_generation_plans_conflict_without_partial_visibility() {
    let authority = RSpaceStateAuthority::in_memory();
    let history = history_at_current(&authority);
    let (cold_a, bytes_a) = data_leaf(b"winner");
    let (cold_b, bytes_b) = data_leaf(b"stale");
    let (_next_a, plan_a) = prepare_insert(history.as_ref(), 2, &cold_a);
    let (_next_b, plan_b) = prepare_insert(history.as_ref(), 3, &cold_b);
    authority
        .commit_local_checkpoint(&plan_a, vec![(cold_a, bytes_a)], &validator())
        .expect("winner commits");
    let before_stale = memory_snapshot(&authority);

    let error = authority
        .commit_local_checkpoint(&plan_b, vec![(cold_b, bytes_b)], &validator())
        .expect_err("same-generation loser must conflict");

    assert!(matches!(error, HistoryError::RootError(RootError::RootCatalogConflict { .. })));
    assert_eq!(memory_snapshot(&authority), before_stale);
}

#[test]
fn target_equal_replay_requires_reactivation_and_reuses_exact_object_closure() {
    let authority = RSpaceStateAuthority::in_memory();
    let initial = authority.current_verified_root().unwrap().unwrap();
    let history = history_at_current(&authority);
    let (cold, bytes) = data_leaf(b"target-equal-record-replay");
    let (_play_history, play_plan) = prepare_insert(history.as_ref(), 15, &cold);
    let (_stale_replay_history, stale_replay_plan) = prepare_insert(history.as_ref(), 15, &cold);

    let play = authority
        .commit_local_checkpoint(&play_plan, vec![(cold.clone(), bytes.clone())], &validator())
        .expect("play checkpoint");
    assert_eq!(play_plan.target_root(), play.root());
    assert_eq!(stale_replay_plan.target_root(), play.root());
    let after_play = memory_snapshot(&authority);

    let error = authority
        .commit_local_checkpoint(
            &stale_replay_plan,
            vec![(cold.clone(), bytes.clone())],
            &validator(),
        )
        .expect_err("target equality must not authorize a stale replay base");
    assert!(matches!(error, HistoryError::RootError(RootError::RootCatalogConflict { .. })));
    assert_eq!(
        memory_snapshot(&authority),
        after_play,
        "RR-07: rejected skip-reset replay must preserve every authority namespace",
    );

    authority
        .activate_known(authority.known_verified_root(initial.root()).unwrap())
        .expect("reactivate the recorded base");
    let reset_snapshot = memory_snapshot(&authority);
    let fresh_history = history_at_current(&authority);
    let (_replay_history, replay_plan) = prepare_insert(fresh_history.as_ref(), 15, &cold);
    assert_eq!(
        memory_snapshot(&authority),
        reset_snapshot,
        "RR-10: fresh replay preparation must be byte-for-byte private",
    );
    let replay = authority
        .commit_local_checkpoint(&replay_plan, vec![(cold, bytes)], &validator())
        .expect("fresh replay checkpoint");
    assert_eq!(replay.root(), play.root());

    let mut after_replay = memory_snapshot(&authority);
    let mut play_closure = after_play;
    after_replay.roots.remove(CURRENT_ROOT_KEY);
    play_closure.roots.remove(CURRENT_ROOT_KEY);
    assert_eq!(
        after_replay, play_closure,
        "RR-12: replay must reuse the exact authenticated history/cold closure and verified-root \
         catalog",
    );
}

#[test]
fn aba_root_reactivation_does_not_revive_an_old_plan() {
    let authority = RSpaceStateAuthority::in_memory();
    let empty = authority.current_verified_root().unwrap().unwrap();
    let history = history_at_current(&authority);
    let (stale_cold, stale_bytes) = data_leaf(b"stale-after-aba");
    let (_stale_next, stale_plan) = prepare_insert(history.as_ref(), 4, &stale_cold);
    let (winner_cold, winner_bytes) = data_leaf(b"aba-winner");
    let (_winner_next, winner_plan) = prepare_insert(history.as_ref(), 5, &winner_cold);
    authority
        .commit_local_checkpoint(&winner_plan, vec![(winner_cold, winner_bytes)], &validator())
        .expect("advance away from empty");
    authority
        .activate_known(authority.known_verified_root(empty.root()).unwrap())
        .expect("return to same root at newer generation");
    let before_stale = memory_snapshot(&authority);

    let error = authority
        .commit_local_checkpoint(&stale_plan, vec![(stale_cold, stale_bytes)], &validator())
        .expect_err("ABA must be rejected by generation");

    assert!(matches!(error, HistoryError::RootError(RootError::RootCatalogConflict { .. })));
    assert_eq!(memory_snapshot(&authority), before_stale);
}

#[test]
fn missing_or_malformed_cold_leaf_leaves_exact_pre_state() {
    let authority = RSpaceStateAuthority::in_memory();
    let history = history_at_current(&authority);
    let (cold, _bytes) = data_leaf(b"must-be-present");
    let (_next, prepared) = prepare_insert(history.as_ref(), 6, &cold);
    let before = memory_snapshot(&authority);

    authority
        .commit_local_checkpoint(&prepared, Vec::new(), &validator())
        .expect_err("missing cold closure must fail");
    assert_eq!(memory_snapshot(&authority), before);

    authority
        .commit_local_checkpoint(
            &prepared,
            vec![(cold, b"not-a-canonical-leaf".to_vec())],
            &validator(),
        )
        .expect_err("malformed cold object must fail");
    assert_eq!(memory_snapshot(&authority), before);
}

#[test]
fn immutable_collision_leaves_exact_pre_state() {
    let authority = RSpaceStateAuthority::in_memory();
    let history = history_at_current(&authority);
    let (cold, bytes) = data_leaf(b"collision");
    let (_next, prepared) = prepare_insert(history.as_ref(), 7, &cold);
    let AuthorityBackend::Memory(state) = &authority.backend else {
        unreachable!()
    };
    {
        let mut state = state.lock().unwrap();
        state
            .history
            .insert(prepared.target_root().bytes(), b"conflicting-object".to_vec());
    }
    let before = memory_snapshot(&authority);

    authority
        .commit_local_checkpoint(&prepared, vec![(cold, bytes)], &validator())
        .expect_err("immutable history collision must fail");

    assert_eq!(memory_snapshot(&authority), before);
}

#[test]
fn same_key_same_value_is_idempotent_across_checkpoints() {
    let authority = RSpaceStateAuthority::in_memory();
    let first_history = history_at_current(&authority);
    let (cold, bytes) = data_leaf(b"shared-object");
    let (second_history, first) = prepare_insert(first_history.as_ref(), 8, &cold);
    authority
        .commit_local_checkpoint(&first, vec![(cold.clone(), bytes.clone())], &validator())
        .expect("first checkpoint");
    let (_third_history, second) = prepare_insert(second_history.as_ref(), 9, &cold);

    authority
        .commit_local_checkpoint(&second, vec![(cold.clone(), bytes)], &validator())
        .expect("same-key/same-value cold object is idempotent");

    assert!(
        authority
            .cold_store()
            .get_one(&cold.encode())
            .unwrap()
            .is_some()
    );
}

#[test]
fn lmdb_success_and_stale_conflict_match_memory_atomicity() {
    let unique = format!("rspace-authority-test-{}", std::process::id());
    let dir = std::env::temp_dir().join(unique);
    std::fs::create_dir_all(&dir).expect("create LMDB test directory");
    let authority =
        RSpaceStateAuthority::open_lmdb(&dir, "rspace", None, None, 16 * 1024 * 1024, 8)
            .expect("LMDB authority");
    authority
        .initialize_for_records(&validator())
        .expect("initialize LMDB authority");
    let history = history_at_current(&authority);
    let (winner_cold, winner_bytes) = data_leaf(b"lmdb-winner");
    let (loser_cold, loser_bytes) = data_leaf(b"lmdb-loser");
    let (_winner_next, winner) = prepare_insert(history.as_ref(), 10, &winner_cold);
    let (_loser_next, loser) = prepare_insert(history.as_ref(), 11, &loser_cold);
    let current = authority
        .commit_local_checkpoint(&winner, vec![(winner_cold.clone(), winner_bytes)], &validator())
        .expect("LMDB atomic checkpoint");
    assert_root_objects_visible(&authority, &current, &winner, &winner_cold);

    let history_before = authority.history_store().to_map().unwrap();
    let cold_before = authority.cold_store().to_map().unwrap();
    let current_before = authority.current_verified_root().unwrap();
    authority
        .commit_local_checkpoint(&loser, vec![(loser_cold, loser_bytes)], &validator())
        .expect_err("LMDB stale plan conflicts");
    assert_eq!(authority.history_store().to_map().unwrap(), history_before);
    assert_eq!(authority.cold_store().to_map().unwrap(), cold_before);
    assert_eq!(authority.current_verified_root().unwrap(), current_before);
}

#[test]
fn lmdb_reopen_preserves_exact_catalog_and_known_root_activation() {
    let scratch = test_scratch::acquire("rspace-authority-reopen-");
    let authority =
        RSpaceStateAuthority::open_lmdb(scratch.path(), "rspace", None, None, 16 * 1024 * 1024, 8)
            .expect("open first authority");
    authority
        .initialize_for_records(&validator())
        .expect("initialize first authority");
    let empty = authority.current_verified_root().unwrap().unwrap();
    let history = history_at_current(&authority);
    let (cold, bytes) = data_leaf(b"reopen-root");
    let (next_history, prepared) = prepare_insert(history.as_ref(), 13, &cold);
    let committed = authority
        .commit_local_checkpoint(&prepared, vec![(cold, bytes)], &validator())
        .expect("commit before reopen");
    drop(next_history);
    drop(prepared);
    drop(history);
    drop(authority);

    let reopened =
        RSpaceStateAuthority::open_lmdb(scratch.path(), "rspace", None, None, 16 * 1024 * 1024, 8)
            .expect("reopen authority");
    reopened
        .initialize_for_records(&validator())
        .expect("initialize reopened authority");
    assert_eq!(reopened.current_verified_root().unwrap(), Some(committed.clone()));
    assert!(reopened.contains_verified_root(committed.root()).unwrap());
    assert!(reopened.contains_verified_root(empty.root()).unwrap());

    let at_empty = reopened
        .activate_known(reopened.known_verified_root(empty.root()).unwrap())
        .expect("reset to the retained empty root");
    assert_eq!(at_empty.root(), empty.root());
    assert_eq!(at_empty.generation(), committed.generation() + 1);

    let restored = reopened
        .activate_known(reopened.known_verified_root(committed.root()).unwrap())
        .expect("reactivate the retained checkpoint root");
    assert_eq!(restored.root(), committed.root());
    assert_eq!(restored.generation(), at_empty.generation() + 1);
}

#[test]
fn lmdb_rejects_malformed_current_and_membership_records() {
    let current_scratch = test_scratch::acquire("rspace-authority-bad-current-");
    let current_authority = RSpaceStateAuthority::open_lmdb(
        current_scratch.path(),
        "rspace",
        None,
        None,
        16 * 1024 * 1024,
        8,
    )
    .expect("open current-record authority");
    current_authority
        .initialize_for_records(&validator())
        .expect("initialize current-record authority");
    let AuthorityBackend::Lmdb(current_state) = &current_authority.backend else {
        unreachable!()
    };
    let mut txn = current_state
        .env
        .write_txn()
        .expect("corrupt current write");
    current_state
        .roots
        .put(&mut txn, &CURRENT_ROOT_KEY.to_vec(), &b"malformed-current".to_vec())
        .expect("write malformed current");
    txn.commit().expect("commit malformed current");
    assert!(current_authority.current_verified_root().is_err());

    let member_scratch = test_scratch::acquire("rspace-authority-bad-membership-");
    let member_authority = RSpaceStateAuthority::open_lmdb(
        member_scratch.path(),
        "rspace",
        None,
        None,
        16 * 1024 * 1024,
        8,
    )
    .expect("open membership authority");
    member_authority
        .initialize_for_records(&validator())
        .expect("initialize membership authority");
    let current = member_authority.current_verified_root().unwrap().unwrap();
    let AuthorityBackend::Lmdb(member_state) = &member_authority.backend else {
        unreachable!()
    };
    let mut txn = member_state
        .env
        .write_txn()
        .expect("corrupt membership write");
    member_state
        .roots
        .put(&mut txn, &verified_root_key(current.root()), &b"malformed-membership".to_vec())
        .expect("write malformed membership");
    txn.commit().expect("commit malformed membership");
    assert!(
        member_authority
            .contains_verified_root(current.root())
            .is_err()
    );
}

type LegacyRepository = dyn crate::rspace::history::history_repository::HistoryRepository<String, String, String, String>
    + Send
    + Sync;
type LegacyAction = crate::rspace::hot_store_action::HotStoreAction<String, String, String, String>;
type LegacyPath = Vec<(Blake2b256Hash, Option<u8>)>;
type LegacyItems = Vec<(Blake2b256Hash, Vec<u8>)>;
type LegacyPage = (LegacyPath, LegacyPath, bool, LegacyItems, LegacyItems);

#[derive(Debug, PartialEq, Eq, serde::Serialize)]
struct LegacyMigrationSnapshot {
    current: (Vec<u8>, u64),
    catalog: BTreeMap<Vec<u8>, Vec<u8>>,
    history: BTreeMap<Vec<u8>, Vec<u8>>,
    cold: BTreeMap<Vec<u8>, Vec<u8>>,
    pages: Vec<(Vec<u8>, i32, Vec<LegacyPage>)>,
}

fn legacy_fixture_actions() -> (Vec<LegacyAction>, Vec<LegacyAction>) {
    use crate::rspace::hot_store_action::{
        HotStoreAction, InsertAction, InsertContinuations, InsertData, InsertJoins,
    };
    use crate::rspace::internal::{Datum, WaitingContinuation};

    let channels = vec!["legacy-fixture-a".to_owned(), "legacy-fixture-b".to_owned()];
    let datum = Datum::create(&channels[0], "alpha".to_owned(), false);
    let continuation = WaitingContinuation::create(
        &channels,
        &vec!["first".to_owned(), "second".to_owned()],
        &"body".to_owned(),
        true,
        std::collections::BTreeSet::from([0]),
    );
    let mut initial = vec![
        HotStoreAction::Insert(InsertAction::InsertData(InsertData {
            channel: channels[0].clone(),
            data: vec![datum.clone(), datum],
        })),
        HotStoreAction::Insert(InsertAction::InsertContinuations(InsertContinuations {
            channels: channels.clone(),
            continuations: vec![continuation],
        })),
    ];
    initial.extend(channels.iter().map(|channel| {
        HotStoreAction::Insert(InsertAction::InsertJoins(InsertJoins {
            channel: channel.clone(),
            joins: vec![channels.clone()],
        }))
    }));
    let suffix = vec![HotStoreAction::Insert(InsertAction::InsertData(InsertData {
        channel: channels[1].clone(),
        data: vec![Datum::create(&channels[1], "beta".to_owned(), true)],
    }))];
    (initial, suffix)
}

fn legacy_fixture_repository(authority: Arc<RSpaceStateAuthority>) -> Box<LegacyRepository> {
    crate::rspace::history::history_repository::HistoryRepositoryInstances::<
        String,
        String,
        String,
        String,
    >::repository(authority)
    .unwrap()
}

fn seed_legacy_fixture(
    authority: Arc<RSpaceStateAuthority>,
) -> (Box<LegacyRepository>, [Blake2b256Hash; 3]) {
    let repository = legacy_fixture_repository(authority);
    let empty = repository.root();
    let (initial, suffix) = legacy_fixture_actions();
    let rollback = repository.checkpoint(initial).unwrap();
    let rollback_root = rollback.root();
    let latest = rollback.checkpoint(suffix).unwrap();
    let latest_root = latest.root();
    (latest, [empty, rollback_root, latest_root])
}

fn legacy_fixture_pages(
    repository: &LegacyRepository,
    root: &Blake2b256Hash,
    take: i32,
) -> Vec<LegacyPage> {
    use crate::rspace::state::exporters::rspace_exporter_items::RSpaceExporterItems;

    let exporter = repository.exporter();
    let mut path = vec![(root.clone(), None)];
    let mut seen = std::collections::BTreeSet::new();
    let mut pages = Vec::new();
    loop {
        assert!(seen.insert(path.clone()), "legacy fixture cursor repeated before termination");
        let (history, cold) =
            RSpaceExporterItems::get_history_and_data(exporter.clone(), path.clone(), 0, take)
                .unwrap();
        assert_eq!(history.last_path, cold.last_path);
        let next = history.last_path;
        assert!(!next.is_empty());
        let terminal = next == path;
        pages.push((path, next.clone(), terminal, history.items, cold.items));
        if terminal {
            break;
        }
        assert_eq!(next.len(), 6);
        assert_eq!(next[0], (root.clone(), None));
        path = next;
    }
    pages
}

fn legacy_fixture_snapshot(
    authority: &Arc<RSpaceStateAuthority>,
    repository: &LegacyRepository,
    roots: &[Blake2b256Hash; 3],
) -> LegacyMigrationSnapshot {
    let current = authority.current_verified_root().unwrap().unwrap();
    let catalog: BTreeMap<_, _> = authority
        .roots_store_for_inspection()
        .to_map()
        .unwrap()
        .into_iter()
        .collect();
    let expected_members = roots
        .iter()
        .map(verified_root_key)
        .collect::<std::collections::BTreeSet<_>>();
    let actual_members = catalog
        .keys()
        .filter(|key| key.starts_with(super::VERIFIED_ROOT_KEY_PREFIX))
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(actual_members, expected_members);
    assert_eq!(catalog.len(), 4);
    for root in roots {
        assert!(authority.contains_verified_root(root).unwrap());
        assert_eq!(authority.known_verified_root(root).unwrap().root(), root);
    }
    assert_eq!(authority.current_verified_root().unwrap(), Some(current.clone()));
    let pages = roots
        .iter()
        .flat_map(|root| {
            [1, 2, 5].map(|take| (root.bytes(), take, legacy_fixture_pages(repository, root, take)))
        })
        .collect();
    LegacyMigrationSnapshot {
        current: (current.root().bytes(), current.generation()),
        catalog,
        history: authority
            .history_store()
            .to_map()
            .unwrap()
            .into_iter()
            .collect(),
        cold: authority
            .cold_store()
            .to_map()
            .unwrap()
            .into_iter()
            .collect(),
        pages,
    }
}

fn legacy_fixture_digest(value: &impl serde::Serialize) -> String {
    hex::encode(Blake2b256Hash::new(&bincode::serialize(value).unwrap()).bytes())
}

fn assert_legacy_fixture_reads(repository: &LegacyRepository, roots: &[Blake2b256Hash; 3]) {
    use crate::rspace::internal::{Datum, WaitingContinuation};
    use crate::rspace::serializers::serializers::{
        encode_continuations, encode_datums, encode_joins,
    };

    let channels = vec!["legacy-fixture-a".to_owned(), "legacy-fixture-b".to_owned()];
    let datum = Datum::create(&channels[0], "alpha".to_owned(), false);
    let beta = Datum::create(&channels[1], "beta".to_owned(), true);
    let continuation = WaitingContinuation::create(
        &channels,
        &vec!["first".to_owned(), "second".to_owned()],
        &"body".to_owned(),
        true,
        std::collections::BTreeSet::from([0]),
    );
    for (index, root) in roots.iter().enumerate() {
        let reader = repository.get_history_reader(root).unwrap();
        let alpha = if index == 0 {
            vec![]
        } else {
            vec![datum.clone(), datum.clone()]
        };
        let beta = if index == 2 {
            vec![beta.clone()]
        } else {
            vec![]
        };
        let continuations = if index == 0 {
            vec![]
        } else {
            vec![continuation.clone()]
        };
        let joins = if index == 0 {
            vec![]
        } else {
            vec![channels.clone()]
        };
        assert_eq!(encode_datums(&reader.base().get_data(&channels[0])), encode_datums(&alpha));
        assert_eq!(encode_datums(&reader.base().get_data(&channels[1])), encode_datums(&beta));
        assert_eq!(
            encode_continuations(&reader.base().get_continuations(&channels)),
            encode_continuations(&continuations)
        );
        for channel in &channels {
            assert_eq!(encode_joins(&reader.base().get_joins(channel)), encode_joins(&joins));
        }
    }
}

#[test]
fn legacy_lmdb_retained_fixture_survives_process_exit_and_frozen_root_replay() {
    use crate::rspace::state::import_session::{
        StateImportPage, StateImportPublicationMode, StateImportPublicationWitness, StateImportSpec,
    };

    const CHILD_DIRECTORY: &str = "RSPACE_LEGACY_MIGRATION_FIXTURE_CHILD_DIRECTORY";
    const CHILD_FINISHED: i32 = 73;
    if let Some(directory) = std::env::var_os(CHILD_DIRECTORY) {
        let authority = RSpaceStateAuthority::open_lmdb(
            std::path::Path::new(&directory),
            "rspace",
            None,
            None,
            16 * 1024 * 1024,
            8,
        )
        .unwrap();
        let (repository, roots) = seed_legacy_fixture(authority);
        assert_legacy_fixture_reads(repository.as_ref(), &roots);
        std::process::exit(CHILD_FINISHED);
    }

    let memory = RSpaceStateAuthority::in_memory();
    let (expected_repository, roots) = seed_legacy_fixture(memory.clone());
    assert_legacy_fixture_reads(expected_repository.as_ref(), &roots);
    let expected = legacy_fixture_snapshot(&memory, expected_repository.as_ref(), &roots);
    assert_eq!(expected.current, (roots[2].bytes(), 2));
    assert_ne!(roots[1], roots[2]);
    assert_eq!(
        roots
            .iter()
            .map(|root| hex::encode(root.bytes()))
            .collect::<Vec<_>>(),
        [
            "0e5751c026e543b2e8ab2eb06099daa1d1e5df47778f7787faab45cdf12fe3a8",
            "10842f8676716425a2fd9a60a4fd748b8225e4f6aa6c02578fc4fb79ded15c5d",
            "d430aa653eceb0c0df6e821ee8941bcb3f5e235584048a7279ebc48b49fe06b6",
        ]
    );
    for (observed, frozen) in [
        (
            legacy_fixture_digest(&expected.history),
            "96d2d98d20f9e5e7c77f0d826384360a6f7a8a02c03fed072106db7e9174eeb2",
        ),
        (
            legacy_fixture_digest(&expected.cold),
            "4cc917977696dcbc3c729117d3c85f69891e9244f2e0430614389bfdc6173c5c",
        ),
        (
            legacy_fixture_digest(&expected.catalog),
            "18957323e133547de68f8828cc9e7bc3c18612e6ef4f3795531e8d3152184097",
        ),
        (
            legacy_fixture_digest(&expected.pages),
            "c5bdca40adc5acc4d86117fb25309c16568265bef18441755189a536ddeaec12",
        ),
        (
            legacy_fixture_digest(&expected),
            "fafc11d0fb8accd3b1b3f3f2f26c07dc6aeb143aae9847dd502ef2f1a6f60afa",
        ),
    ] {
        assert_eq!(observed, frozen);
    }

    let scratch = test_scratch::acquire("rspace-legacy-migration-fixture-");
    let source = scratch.path().join("source");
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .arg("--exact")
        .arg("rspace::state::rspace_state_authority::atomic_state_authority_tests::legacy_lmdb_retained_fixture_survives_process_exit_and_frozen_root_replay")
        .arg("--nocapture")
        .env(CHILD_DIRECTORY, &source)
        .output().unwrap();
    assert_eq!(
        output.status.code(),
        Some(CHILD_FINISHED),
        "writer did not finish its committed fixture: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let reopened =
        RSpaceStateAuthority::open_lmdb(&source, "rspace", None, None, 16 * 1024 * 1024, 8)
            .unwrap();
    let repository = legacy_fixture_repository(reopened.clone());
    assert_legacy_fixture_reads(repository.as_ref(), &roots);
    assert_eq!(legacy_fixture_snapshot(&reopened, repository.as_ref(), &roots), expected);

    let destination = RSpaceStateAuthority::open_lmdb(
        &scratch.path().join("rollback"),
        "rspace",
        None,
        None,
        16 * 1024 * 1024,
        8,
    )
    .unwrap();
    let rollback_repository = legacy_fixture_repository(destination.clone());
    let spec = StateImportSpec::new(
        [roots[1].clone()],
        StateImportPublicationMode::Activate(roots[1].clone()),
    )
    .unwrap();
    let mut import = rollback_repository.importer().begin_import(spec).unwrap();
    for (start, next, terminal, history_items, data_items) in
        legacy_fixture_pages(repository.as_ref(), &roots[1], 2)
    {
        let receipt = import
            .validate_and_stage_page(StateImportPage {
                start_path: start,
                advertised_next_path: next.clone(),
                skip: 0,
                take: 2,
                history_items,
                data_items,
            })
            .unwrap();
        assert_eq!(receipt.next_path, next);
        assert_eq!(receipt.terminal, terminal);
    }
    let publication = import
        .seal()
        .unwrap()
        .publish(&StateImportPublicationWitness::new())
        .unwrap();
    assert_eq!(publication.verified_roots, vec![roots[1].clone()]);
    assert_eq!(publication.activation.unwrap().root, roots[1]);
    assert!(!destination.contains_verified_root(&roots[2]).unwrap());
    let restored = legacy_fixture_repository(destination.clone());
    assert_eq!(restored.root(), roots[1]);
    let rollback_current = destination.current_verified_root().unwrap().unwrap();
    assert_eq!(rollback_current.root(), &roots[1]);
    assert!(
        restored
            .get_history_reader(&roots[1])
            .unwrap()
            .base()
            .get_data(&"legacy-fixture-b".to_owned())
            .is_empty()
    );
    let replayed = restored.checkpoint(legacy_fixture_actions().1).unwrap();
    assert_eq!(replayed.root(), roots[2]);
    let replayed_current = destination.current_verified_root().unwrap().unwrap();
    assert_eq!(replayed_current.root(), &roots[2]);
    assert_eq!(replayed_current.generation(), rollback_current.generation() + 1);
    assert_eq!(legacy_fixture_repository(destination.clone()).root(), roots[2]);
    assert_legacy_fixture_reads(replayed.as_ref(), &roots);
    for root in &roots {
        for take in [1, 2, 5] {
            assert_eq!(
                legacy_fixture_pages(replayed.as_ref(), root, take),
                legacy_fixture_pages(repository.as_ref(), root, take)
            );
        }
    }
    assert_eq!(legacy_fixture_snapshot(&reopened, repository.as_ref(), &roots), expected);
}
