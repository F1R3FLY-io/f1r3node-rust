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
