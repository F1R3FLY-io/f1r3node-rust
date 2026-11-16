use std::any::type_name;
use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Duration;

use super::durable_publication::directory_snapshot;
use super::durable_recovery::limits;
use super::durable_rspace_authority::{
    Profile, fixture, poll_checkpoint_with, populate, publication_limits,
};
use super::owner_cross_base_checkpoint::view_limits;
use super::support::trial_scratch;
use crate::rspace::errors::{
    AuthorityBackendMismatch, HistoryError, HistoryRepositoryError, RootError,
};
use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use crate::rspace::history::history::{History, HistoryInstances};
use crate::rspace::history::history_repository::{HistoryRepository, HistoryRepositoryInstances};
use crate::rspace::history::instances::radix_history::RadixHistory;
use crate::rspace::hot_store::HotStore;
use crate::rspace::persistence::durable::{
    DurableAdmissionErrorV1, DurableAdmissionLimitsV1, DurableIoError, DurableStartupErrorV1,
    DurableStoreGenesisStepV1, arm_durable_store_genesis_failpoint, index_file_name,
    pack_file_name, start_durable_store_genesis_trace, take_durable_store_genesis_trace,
};
use crate::rspace::persistence::{
    DurableRSpacePublicationLimitsV1, DurableStateRootV1, IndexGenerationV1, LogPackDecodeLimits,
    PackId, RSpaceSemanticProfile, ViewCheckpointLimitsV1,
};
use crate::rspace::rspace::{RSpace, RSpaceStore};
use crate::rspace::state::closure_verifier::RecordValidator;
use crate::rspace::state::import_session::{
    StateImportError, StateImportPublicationMode, StateImportPublicationWitness, StateImportSpec,
};
use crate::rspace::state::rspace_state_authority::{
    AuthorityRouteV1, DurableBackendErrorV1, DurableBackendV1, DurableOwnerSlotV1,
    RSpaceStateAuthority,
};

const CATALOG_FILE_NAME: &str = "catalog.v1";

type ByteChannelProfile = RSpaceSemanticProfile<Vec<u8>, String, String, String>;
type StringRepository =
    Box<dyn HistoryRepository<String, String, String, String> + Send + Sync + 'static>;
type StringHotStore = Box<dyn HotStore<String, String, String, String>>;
type ByteChannelRepository =
    Box<dyn HistoryRepository<Vec<u8>, String, String, String> + Send + Sync + 'static>;
type ByteChannelHotStore = Box<dyn HotStore<Vec<u8>, String, String, String>>;

fn durable_authority(root: &Path) -> Arc<RSpaceStateAuthority> {
    RSpaceStateAuthority::durable(root, ViewCheckpointLimitsV1 {
        publication: DurableRSpacePublicationLimitsV1 {
            recovery: limits(),
            wal: LogPackDecodeLimits::UNBOUNDED,
            admission: DurableAdmissionLimitsV1::UNBOUNDED,
            ..publication_limits()
        },
        difference: view_limits().difference,
    })
}

fn open(
    authority: &Arc<RSpaceStateAuthority>,
) -> Result<(StringRepository, StringHotStore), HistoryRepositoryError> {
    RSpace::<String, String, String, String>::create_history_repo(RSpaceStore::new(Arc::clone(
        authority,
    )))
}

fn open_with_byte_channels(
    authority: &Arc<RSpaceStateAuthority>,
) -> Result<(ByteChannelRepository, ByteChannelHotStore), HistoryRepositoryError> {
    RSpace::<Vec<u8>, String, String, String>::create_history_repo(RSpaceStore::new(Arc::clone(
        authority,
    )))
}

fn backend(authority: &RSpaceStateAuthority) -> &DurableBackendV1 {
    match authority.route() {
        AuthorityRouteV1::Durable(backend) => backend,
        AuthorityRouteV1::Legacy => panic!("the trial authority uses the durable backend"),
    }
}

fn owner_slot(authority: &RSpaceStateAuthority) -> Arc<DurableOwnerSlotV1<Profile>> {
    backend(authority)
        .owner_slot::<String, String, String, String>()
        .expect("the durable owner slot serves the string-channel profile")
}

fn backend_refusal(refusal: &HistoryRepositoryError) -> &DurableBackendErrorV1 {
    match refusal {
        HistoryRepositoryError::DurableBackend(error) => error
            .downcast_ref::<DurableBackendErrorV1>()
            .expect("a durable backend refusal carries the backend's typed error"),
        other => panic!("create_history_repo refused outside the durable backend: {other:?}"),
    }
}

fn store_genesis_root() -> Blake2b256Hash {
    Blake2b256Hash::from(DurableStateRootV1::store_genesis::<Profile>())
}

fn without_genesis<T>(action: impl FnOnce() -> T) -> T {
    start_durable_store_genesis_trace();
    let outcome = action();
    assert_eq!(
        take_durable_store_genesis_trace(),
        Vec::new(),
        "the durable backend ran store genesis over a directory that holds store files"
    );
    outcome
}

fn traced_backend_genesis_points() -> Vec<DurableStoreGenesisStepV1> {
    let scratch = trial_scratch("durable-backend-genesis-trace-");
    let authority = durable_authority(scratch.path());
    start_durable_store_genesis_trace();
    let opened = open(&authority);
    let points = take_durable_store_genesis_trace();
    opened.expect("the traced durable backend creates and opens the empty store");
    points
}

fn assert_profile_mismatch(refusal: &DurableBackendErrorV1, expected: &str) {
    match refusal {
        DurableBackendErrorV1::ProfileMismatch { requested } => assert_eq!(*requested, expected),
        other => panic!("a second semantic profile was refused with {other:?}"),
    }
}

fn assert_legacy_methods_refused(
    durable: &Arc<RSpaceStateAuthority>,
    legacy: &Arc<RSpaceStateAuthority>,
) {
    let validator: RecordValidator = Arc::new(|_, _| Ok(()));
    let empty = RadixHistory::empty_root_node_hash();
    let roots_refusal = RootError::BackendMismatch(AuthorityBackendMismatch::Roots);
    assert!(matches!(durable.history_store(), Err(AuthorityBackendMismatch::History)));
    assert!(matches!(durable.cold_store(), Err(AuthorityBackendMismatch::Cold)));
    assert!(matches!(durable.roots_store_for_inspection(), Err(AuthorityBackendMismatch::Roots)));
    assert_eq!(durable.initialize_for_records(&validator), Err(roots_refusal.clone()));
    assert!(matches!(durable.current_verified_root(), Err(error) if error == roots_refusal));
    assert_eq!(durable.current_root(), Err(roots_refusal.clone()));
    assert_eq!(durable.contains_verified_root(&empty), Err(roots_refusal.clone()));
    assert!(matches!(durable.known_verified_root(&empty), Err(error) if error == roots_refusal));
    let known = legacy
        .known_verified_root(&empty)
        .expect("the legacy authority knows its empty root");
    assert!(matches!(durable.activate_known(known), Err(error) if error == roots_refusal));
    let history = HistoryInstances::create_authorized(
        legacy
            .current_root()
            .expect("the legacy authority has a current root"),
        Arc::clone(legacy),
    )
    .expect("the legacy authority authorizes its history");
    let (_next, prepared) = history
        .prepare_checkpoint(Vec::new())
        .expect("the legacy history prepares an empty checkpoint");
    assert!(matches!(
        durable.commit_local_checkpoint(&prepared, Vec::new(), &validator),
        Err(HistoryError::BackendMismatch(AuthorityBackendMismatch::History))
    ));
    let spec = || {
        StateImportSpec::new([empty.clone()], StateImportPublicationMode::PreserveCurrent)
            .expect("the empty root is an import target")
    };
    assert!(matches!(
        durable.begin_import(spec()),
        Err(StateImportError::BackendMismatch(AuthorityBackendMismatch::History))
    ));
    let start = vec![(empty.clone(), None)];
    let mut staged = legacy
        .begin_import(spec())
        .expect("the legacy authority stages an import");
    assert!(matches!(
        durable.accept_import_page(&mut staged, &start, Vec::new(), true),
        Err(StateImportError::BackendMismatch(AuthorityBackendMismatch::History))
    ));
    assert!(matches!(
        durable.seal_import(staged, &validator),
        Err(StateImportError::BackendMismatch(AuthorityBackendMismatch::History))
    ));
    let mut staged = legacy
        .begin_import(spec())
        .expect("the legacy authority stages a second import");
    legacy
        .accept_import_page(&mut staged, &start, Vec::new(), true)
        .expect("the legacy authority accepts the terminal page");
    let sealed = legacy
        .seal_import(staged, &validator)
        .expect("the legacy authority seals the empty root");
    assert!(matches!(
        durable.publish_import(sealed, &StateImportPublicationWitness::new()),
        Err(StateImportError::BackendMismatch(AuthorityBackendMismatch::Roots))
    ));
    assert!(matches!(
        HistoryRepositoryInstances::<String, String, String, String>::repository(Arc::clone(
            durable
        )),
        Err(HistoryRepositoryError::RootError(RootError::BackendMismatch(
            AuthorityBackendMismatch::Roots
        )))
    ));
}

async fn contend_for_the_owner(
    slot: Arc<DurableOwnerSlotV1<Profile>>,
) -> (DurableStateRootV1, DurableStateRootV1) {
    let (held_sender, held_receiver) = tokio::sync::oneshot::channel::<()>();
    let (release_sender, release_receiver) = tokio::sync::oneshot::channel::<()>();
    let (waiting_sender, waiting_receiver) = tokio::sync::oneshot::channel::<()>();
    let holder_slot = Arc::clone(&slot);
    let holder = tokio::task::spawn_local(async move {
        let guard = holder_slot.owner().lock().await;
        held_sender
            .send(())
            .expect("the test awaits the held owner");
        release_receiver
            .await
            .expect("the test releases the held owner");
        guard.current_root()
    });
    held_receiver
        .await
        .expect("the holder takes the owner mutex");
    let waiter_slot = Arc::clone(&slot);
    let waiter = tokio::task::spawn_local(async move {
        waiting_sender
            .send(())
            .expect("the test awaits the waiting task");
        waiter_slot.owner().lock().await.current_root()
    });
    waiting_receiver
        .await
        .expect("the waiter starts waiting for the owner mutex");
    release_sender
        .send(())
        .expect("the holder awaits its release");
    let held = holder.await.expect("the holder completes");
    let waited = waiter.await.expect("the waiter completes");
    (held, waited)
}

#[test]
fn create_history_repo_calls_share_one_durable_owner_slot() {
    let scratch = trial_scratch("durable-backend-shared-owner-");
    let authority = durable_authority(scratch.path());
    let (first_repository, first_hot) =
        open(&authority).expect("the first call opens the durable owner slot");
    let (second_repository, second_hot) =
        open(&authority).expect("the second call reuses the durable owner slot");
    let first_slot = owner_slot(&authority);
    let second_slot = owner_slot(&authority);
    assert!(Arc::ptr_eq(&first_slot, &second_slot));
    assert!(std::ptr::eq(first_slot.owner(), second_slot.owner()));
    assert_eq!(
        Arc::strong_count(&first_slot),
        3,
        "only the backend and the two handles hold the owner slot"
    );
    let current = Blake2b256Hash::from(first_slot.published_core().current_root());
    assert_eq!(current, store_genesis_root());
    assert_eq!(first_repository.root(), current);
    assert_eq!(second_repository.root(), current);
    let channel = "channel".to_owned();
    assert!(first_hot.get_data(&channel).is_empty());
    assert!(second_hot.get_data(&channel).is_empty());
}

#[test]
fn a_second_semantic_profile_is_refused_by_the_owner_slot() {
    let string_first = trial_scratch("durable-backend-string-profile-first-");
    let authority = durable_authority(string_first.path());
    let (repository, _hot) =
        open(&authority).expect("the string-channel profile opens the owner slot");
    let installed = owner_slot(&authority);
    let before = directory_snapshot(string_first.path());
    let refused = open_with_byte_channels(&authority)
        .err()
        .expect("the byte-channel profile is refused");
    assert_profile_mismatch(backend_refusal(&refused), type_name::<ByteChannelProfile>());
    match backend(&authority).owner_slot::<Vec<u8>, String, String, String>() {
        Err(refusal) => assert_profile_mismatch(&refusal, type_name::<ByteChannelProfile>()),
        Ok(_) => panic!("the owner slot served a second semantic profile"),
    }
    assert_eq!(
        directory_snapshot(string_first.path()),
        before,
        "a refused profile changed the store"
    );
    let (again, _again_hot) =
        open(&authority).expect("the installed string-channel profile is still served");
    assert!(Arc::ptr_eq(&installed, &owner_slot(&authority)));
    assert_eq!(again.root(), repository.root());

    let bytes_first = trial_scratch("durable-backend-byte-profile-first-");
    let authority = durable_authority(bytes_first.path());
    let (repository, _hot) =
        open_with_byte_channels(&authority).expect("the byte-channel profile opens the owner slot");
    let before = directory_snapshot(bytes_first.path());
    let refused = open(&authority)
        .err()
        .expect("the string-channel profile is refused");
    assert_profile_mismatch(backend_refusal(&refused), type_name::<Profile>());
    match backend(&authority).owner_slot::<String, String, String, String>() {
        Err(refusal) => assert_profile_mismatch(&refusal, type_name::<Profile>()),
        Ok(_) => panic!("the owner slot served a second semantic profile"),
    }
    assert_eq!(
        directory_snapshot(bytes_first.path()),
        before,
        "a refused profile changed the store"
    );
    let (again, _again_hot) = open_with_byte_channels(&authority)
        .expect("the installed byte-channel profile is still served");
    assert_eq!(again.root(), repository.root());
}

#[test]
fn legacy_only_authority_methods_refuse_the_durable_backend() {
    let scratch = trial_scratch("durable-backend-legacy-methods-");
    let authority = durable_authority(scratch.path());
    let legacy = RSpaceStateAuthority::in_memory();
    assert_legacy_methods_refused(&authority, &legacy);
    assert!(
        std::fs::read_dir(scratch.path())
            .expect("list the durable trial directory")
            .next()
            .is_none(),
        "a refused legacy method touched the store directory"
    );
    let (repository, _hot) =
        open(&authority).expect("the durable backend opens the empty directory");
    let before = directory_snapshot(scratch.path());
    assert_legacy_methods_refused(&authority, &legacy);
    assert_eq!(
        directory_snapshot(scratch.path()),
        before,
        "a refused legacy method changed the store"
    );
    assert_eq!(repository.root(), store_genesis_root());
}

#[test]
fn only_an_empty_directory_becomes_a_store_at_the_store_genesis_root() {
    let scratch = trial_scratch("durable-backend-genesis-");
    let created = durable_authority(scratch.path());
    let (repository, hot) = open(&created).expect("an empty directory becomes a durable store");
    assert_eq!(repository.root(), store_genesis_root());
    assert_eq!(repository.empty_root(), store_genesis_root());
    assert!(hot.get_data(&"channel".to_owned()).is_empty());
    assert_eq!(
        directory_snapshot(scratch.path())
            .into_keys()
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([
            CATALOG_FILE_NAME.to_owned(),
            pack_file_name(PackId::GENESIS),
            index_file_name(IndexGenerationV1::FIRST),
        ])
    );
    drop((repository, hot, created));
    let reopened = durable_authority(scratch.path());
    let (repository, _hot) =
        without_genesis(|| open(&reopened)).expect("the created store reopens without genesis");
    assert_eq!(repository.root(), store_genesis_root());

    let points = traced_backend_genesis_points();
    let slot_write = points
        .iter()
        .position(|point| *point == DurableStoreGenesisStepV1::WriteCatalogSlot)
        .expect("the backend's genesis writes the catalog slot");
    let mut created_points = Vec::with_capacity(points.len());
    let mut refused_points = Vec::with_capacity(points.len());
    let mut opened_points = Vec::with_capacity(points.len());
    for (position, point) in points.iter().copied().enumerate() {
        let scratch = trial_scratch("durable-backend-genesis-crash-");
        let authority = durable_authority(scratch.path());
        arm_durable_store_genesis_failpoint(point);
        let crashed_open = open(&authority)
            .err()
            .expect("a crash inside store genesis refuses the open");
        assert!(
            matches!(backend_refusal(&crashed_open), DurableBackendErrorV1::Genesis(_)),
            "the crash at {point:?} was refused with {crashed_open:?}"
        );
        let crashed = directory_snapshot(scratch.path());
        if crashed.is_empty() {
            let (repository, _hot) =
                open(&authority).expect("a directory without store files becomes a store");
            assert_eq!(repository.root(), store_genesis_root());
            created_points.push(point);
            continue;
        }
        match without_genesis(|| open(&authority)) {
            Ok((repository, _hot)) => {
                assert!(
                    position > slot_write,
                    "the crash at {point:?} left a store that opens before its catalog slot was \
                     written"
                );
                assert_eq!(repository.root(), store_genesis_root());
                opened_points.push(point);
            }
            Err(refused) => {
                assert!(
                    position <= slot_write,
                    "the crash at {point:?} left a written catalog slot that is refused: \
                     {refused:?}"
                );
                assert!(
                    matches!(backend_refusal(&refused), DurableBackendErrorV1::Recovery(_)),
                    "the partial store left by the crash at {point:?} was refused with {refused:?}"
                );
                assert_eq!(
                    directory_snapshot(scratch.path()),
                    crashed,
                    "the refused partial store left by the crash at {point:?} was changed"
                );
                refused_points.push(point);
            }
        }
    }
    assert_eq!(created_points, points[..1]);
    assert!(!refused_points.is_empty(), "some crash point leaves a partial store");
    assert!(!opened_points.is_empty(), "some crash point lies after the catalog slot write");
}

#[test]
fn a_task_waiting_for_the_owner_mutex_yields_its_thread() {
    let scratch = trial_scratch("durable-backend-owner-mutex-");
    let authority = durable_authority(scratch.path());
    let (repository, _hot) = open(&authority).expect("the durable backend opens");
    let slot = owner_slot(&authority);
    let expected = slot.published_core().current_root();
    assert_eq!(Blake2b256Hash::from(expected), repository.root());
    let (finished_sender, finished_receiver) = mpsc::channel();
    let runtime_thread = std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("build a current-thread runtime");
        let tasks = tokio::task::LocalSet::new();
        let roots = tasks.block_on(&runtime, contend_for_the_owner(slot));
        finished_sender
            .send(roots)
            .expect("the test thread awaits the contended roots");
    });
    match finished_receiver.recv_timeout(Duration::from_secs(30)) {
        Ok((held, waited)) => {
            runtime_thread.join().expect("the runtime thread finishes");
            assert_eq!(held, expected);
            assert_eq!(waited, expected);
        }
        Err(RecvTimeoutError::Timeout) => {
            panic!("a task waiting for the owner mutex blocked the runtime thread for 30 s")
        }
        Err(RecvTimeoutError::Disconnected) => match runtime_thread.join() {
            Err(panic) => std::panic::resume_unwind(panic),
            Ok(()) => panic!("the runtime thread ended without reporting the contended roots"),
        },
    }
}

#[test]
fn a_second_authority_over_a_held_store_is_refused_by_the_writer_lease() {
    let scratch = trial_scratch("durable-backend-writer-lease-");
    let first = durable_authority(scratch.path());
    let (repository, hot) = open(&first).expect("the first authority opens the empty directory");
    let before = directory_snapshot(scratch.path());
    let second = durable_authority(scratch.path());
    let refused = without_genesis(|| open(&second))
        .err()
        .expect("a second authority over a held store is refused");
    match backend_refusal(&refused) {
        DurableBackendErrorV1::Recovery(refusal) => assert!(
            matches!(
                **refusal,
                DurableAdmissionErrorV1::Startup(DurableStartupErrorV1::Io(
                    DurableIoError::WriterLockContended { .. }
                ))
            ),
            "the second authority was refused with {refusal:?}"
        ),
        other => panic!("the second authority was refused with {other:?}"),
    }
    assert_eq!(
        directory_snapshot(scratch.path()),
        before,
        "a refused second authority changed the store"
    );
    drop(first);
    let (reopened, _reopened_hot) = without_genesis(|| open(&second))
        .expect("the second authority opens once the first authority is dropped");
    assert_eq!(reopened.root(), repository.root());
    assert!(hot.get_data(&"channel".to_owned()).is_empty());
}

#[test]
fn a_published_store_reopens_at_its_published_root() {
    let (scratch, hot, gate, mut owner) = fixture();
    populate(&hot);
    let published = poll_checkpoint_with(&mut owner, &gate, &hot, publication_limits())
        .expect("the owner publishes the populated journal");
    drop((owner, hot, gate));
    let authority = durable_authority(scratch.path());
    let (repository, reopened) =
        without_genesis(|| open(&authority)).expect("the published store reopens");
    assert_eq!(
        repository.root(),
        Blake2b256Hash::from(DurableStateRootV1::of_manifest(published.manifest()))
    );
    let channel = "channel".to_owned();
    let group = vec![channel.clone(), channel.clone()];
    assert_eq!(
        reopened
            .get_data(&channel)
            .iter()
            .map(|datum| datum.a.as_str())
            .collect::<Vec<_>>(),
        ["datum"]
    );
    assert_eq!(
        reopened
            .get_continuations(&group)
            .iter()
            .map(|waiting| waiting.continuation.as_str())
            .collect::<Vec<_>>(),
        ["body"]
    );
    assert_eq!(reopened.get_joins(&channel), vec![group]);
}
