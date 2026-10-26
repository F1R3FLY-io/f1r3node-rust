use std::collections::BTreeSet;
use std::fs;
use std::sync::Arc;

use proptest::prelude::*;
use tokio::sync::RwLock;

use super::checkpoint_wal::fixture_for_profile;
use super::durable_publication::install_checkpoint_image;
use super::durable_recovery::{checkpoint_image_for_profile, limits};
use super::retention_v2_format_properties::migrate_checkpoint_source_to_v2;
use super::support::{PersistenceTrialDir, trial_scratch};
use crate::rspace::hot_store::{DurableHotStoreBootstrapLimits, HotStore, HotStoreInstances};
use crate::rspace::internal::{Datum, WaitingContinuation};
use crate::rspace::persistence::durable::{
    DurableAdmissionLimitsV1, DurableDirectoryV1, DurableRSpaceAuthorityV1,
    DurableRSpacePublicationLimitsV1, DurableRSpaceTransactionError,
    DurableTransactionPreparationError, GenerationCheckpointPublicationHookPointV1,
    JournalPublicationPhaseV1, arm_generation_checkpoint_publication_failpoint,
    arm_generation_checkpoint_publication_hook, recover_read_authority,
};
use crate::rspace::persistence::retention::v2_format::RetentionFormat;
use crate::rspace::persistence::{
    DagTransactionLimits, GenerationLeaseKeyV1, GenerationLeaseRegistryV1, LogPackDecodeLimits,
    RSpaceSemanticProfile, SemanticProfile, StateVersionV1,
};

type Profile = RSpaceSemanticProfile<String, String, String, String>;
type Store = Arc<Box<dyn HotStore<String, String, String, String>>>;

fn poll_checkpoint(owner: &mut DurableRSpaceAuthorityV1<Profile>) {
    use std::future::Future;
    use std::task::{Context, Poll, Waker};
    let mut checkpoint = std::pin::pin!(owner.checkpoint(publication_limits()));
    assert!(matches!(
        checkpoint
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop())),
        Poll::Ready(_)
    ));
}

fn assert_authority_unwind_child(case: &str, expected_diagnostic: &str) {
    let scratch = shared::rust::test_scratch::acquire("rspace-durable-owner-unwind-");
    let marker = scratch.path().join("verified");
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--ignored",
            "--exact",
            "rspace::persistence::tests::durable_rspace_authority::authority_unwind_child",
            "--nocapture",
        ])
        .env("RSPACE_DURABLE_OWNER_UNWIND_CASE", case)
        .env("RSPACE_DURABLE_OWNER_UNWIND_MARKER", &marker)
        .env_remove(shared::rust::test_process::ALLOW_EXPECTED_PROBE_CORE);
    let outcome = shared::rust::test_process::run_child_probe(
        &mut command,
        std::time::Duration::from_secs(30),
    )
    .expect("run the durable owner unwind child");
    match outcome {
        shared::rust::test_process::ProbeOutcome::Failed(failure) => {
            assert_eq!(failure.kind, shared::rust::test_process::ProbeFailureKind::Panicked);
            assert!(failure.stderr.contains(expected_diagnostic), "{}", failure.stderr);
        }
        shared::rust::test_process::ProbeOutcome::Passed => {
            panic!("the durable owner unwind child did not fail: {case}")
        }
    }
    assert_eq!(fs::read(marker).unwrap(), case.as_bytes());
}

fn authority_unwind_marker() -> std::path::PathBuf {
    std::env::var_os("RSPACE_DURABLE_OWNER_UNWIND_MARKER")
        .expect("durable owner unwind marker")
        .into()
}

#[test]
#[ignore = "isolated durable owner unwind child"]
fn authority_unwind_child() {
    let Some(case) = std::env::var_os("RSPACE_DURABLE_OWNER_UNWIND_CASE") else {
        return;
    };
    let case = case.to_str().expect("durable owner unwind case is UTF-8");
    let mut parts = case.split(':');
    match parts.next().unwrap() {
        "boundary" => {
            let index = parts.next().unwrap().parse::<usize>().unwrap();
            assert!(parts.next().is_none());
            run_owner_boundary_unwind_child(index, case);
        }
        "publication" => {
            assert!(parts.next().is_none());
            run_owner_publication_unwind_child(case);
        }
        "invalid-base" => {
            let replace = parts.next().unwrap().parse::<bool>().unwrap();
            assert!(parts.next().is_none());
            run_invalid_base_unwind_child(replace, case);
        }
        "ack-conflict" => {
            assert!(parts.next().is_none());
            run_ack_conflict_unwind_child(case);
        }
        _ => panic!("unknown durable owner unwind case: {case}"),
    }
}

#[test]
fn owner_capture_preparation_and_acknowledgement_unwinds_retain_all_required_capabilities() {
    for index in 0..owner_boundary_points().len() {
        assert_authority_unwind_child(
            &format!("boundary:{index}"),
            "journal owner boundary unwind",
        );
    }
}

fn owner_boundary_points()
-> [crate::rspace::persistence::durable::journal_owner_hooks::JournalOwnerHookPoint; 5] {
    use crate::rspace::persistence::durable::journal_owner_hooks::JournalOwnerHookPoint;
    [
        JournalOwnerHookPoint::AfterCapture,
        JournalOwnerHookPoint::AfterPreparation,
        JournalOwnerHookPoint::BeforeSuccessorPin,
        JournalOwnerHookPoint::AfterSuccessorPin,
        JournalOwnerHookPoint::AfterAcknowledgement,
    ]
}

fn run_owner_boundary_unwind_child(index: usize, case: &str) {
    use crate::rspace::persistence::durable::journal_owner_hooks::{self, JournalOwnerHookPoint};
    struct VerifyOnDrop {
        scratch: PersistenceTrialDir,
        hot: Store,
        gate: Arc<RwLock<()>>,
        owner: DurableRSpaceAuthorityV1<Profile>,
        old: StateVersionV1,
        old_key: GenerationLeaseKeyV1,
        new_key: GenerationLeaseKeyV1,
        registry: GenerationLeaseRegistryV1,
        observed: Arc<std::sync::atomic::AtomicBool>,
        point: JournalOwnerHookPoint,
        case: String,
        marker: std::path::PathBuf,
    }
    impl Drop for VerifyOnDrop {
        fn drop(&mut self) {
            assert!(self.observed.load(std::sync::atomic::Ordering::Relaxed));
            assert!(self.gate.try_read().is_err());
            assert!(self.registry.has_active(self.old_key).unwrap());
            assert!(
                DurableDirectoryV1::open(self.scratch.path())
                    .unwrap()
                    .try_acquire_writer()
                    .is_err()
            );
            let files = store_files(self.scratch.path());
            let recovered = self.owner.reconcile(publication_limits()).unwrap();
            if matches!(
                self.point,
                JournalOwnerHookPoint::AfterCapture | JournalOwnerHookPoint::AfterPreparation
            ) {
                assert_eq!(recovered, self.old);
                assert!(!self.hot.pending_deltas().unwrap().data.unwrap().is_empty());
            } else {
                assert_eq!(recovered.generation(), self.old.generation() + 1);
                assert_empty(&self.hot);
                assert!(!self.registry.has_active(self.old_key).unwrap());
                assert!(self.registry.has_active(self.new_key).unwrap());
            }
            assert_eq!(store_files(self.scratch.path()), files);
            assert!(self.gate.try_read().is_ok());
            fs::write(&self.marker, self.case.as_bytes()).unwrap();
        }
    }
    let point = owner_boundary_points()[index];
    let (scratch, hot, gate, owner) = fixture();
    populate(&hot);
    let old = owner.state().unwrap();
    let old_key = owner.generation_key();
    let new_key = GenerationLeaseKeyV1::new(
        old_key.epoch() + 1,
        old_key.first_live_pack(),
        old_key.active_pack(),
        old_key.index_generation(),
    );
    let registry = owner.generation_leases();
    let observed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut verify = VerifyOnDrop {
        scratch,
        hot,
        gate,
        owner,
        old,
        old_key,
        new_key,
        registry,
        observed,
        point,
        case: case.to_owned(),
        marker: authority_unwind_marker(),
    };
    let seen = Arc::clone(&verify.observed);
    let hook_hot = Arc::clone(&verify.hot);
    let hook_gate = Arc::clone(&verify.gate);
    let hook_registry = verify.registry.clone();
    let _hook = journal_owner_hooks::arm(point, move || {
        seen.store(true, std::sync::atomic::Ordering::Relaxed);
        assert!(hook_gate.try_read().is_err());
        assert!(hook_registry.has_active(old_key).unwrap());
        let pinned = matches!(
            point,
            JournalOwnerHookPoint::AfterSuccessorPin | JournalOwnerHookPoint::AfterAcknowledgement
        );
        assert_eq!(hook_registry.has_active(new_key).unwrap(), pinned);
        if point == JournalOwnerHookPoint::AfterAcknowledgement {
            assert_empty(&hook_hot);
        } else {
            assert!(!hook_hot.pending_deltas().unwrap().data.unwrap().is_empty());
        }
        panic!("journal owner boundary unwind");
    });
    poll_checkpoint(&mut verify.owner);
}

fn store_files(root: &std::path::Path) -> std::collections::BTreeMap<String, Vec<u8>> {
    fs::read_dir(root)
        .unwrap()
        .map(Result::unwrap)
        .filter_map(|entry| {
            let name = entry.file_name().into_string().unwrap();
            (name == "catalog.v1" || name.starts_with("pack-") || name.starts_with("index-"))
                .then(|| (name, fs::read(entry.path()).unwrap()))
        })
        .collect()
}

fn replace_store_files(
    root: &std::path::Path,
    files: &std::collections::BTreeMap<String, Vec<u8>>,
) {
    for (name, bytes) in files {
        let path = root.join(name);
        fs::write(&path, bytes).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
        }
    }
}

#[tokio::test]
async fn owner_recovery_rejects_foreign_third_and_same_state_different_terminals() {
    use crate::rspace::persistence::durable::DurableRSpaceTransactionError;
    for variant in 0..3 {
        let (scratch, hot, gate, mut owner) = fixture();
        populate(&hot);
        arm_generation_checkpoint_publication_failpoint(
            GenerationCheckpointPublicationHookPointV1::AfterCatalogSyncBeforeRecovery,
        );
        assert!(owner.checkpoint(publication_limits()).await.is_err());
        let expected = owner.pending_terminal().unwrap();
        let original = store_files(scratch.path());
        let (other_dir, mut other_hot, other_gate, mut other) = fixture();
        if variant == 2 {
            drop(other);
            let writer = DurableDirectoryV1::open(other_dir.path())
                .unwrap()
                .recover_and_admit_v1::<Profile>(
                    limits(),
                    LogPackDecodeLimits::UNBOUNDED,
                    DurableAdmissionLimitsV1::UNBOUNDED,
                )
                .unwrap();
            let writer = writer
                .publish_generation_checkpoint_v1(
                    limits(),
                    LogPackDecodeLimits::UNBOUNDED,
                    DurableAdmissionLimitsV1::UNBOUNDED,
                )
                .unwrap();
            drop(writer);
            let view = recover_read_authority::<Profile>(
                other_dir.path(),
                limits(),
                LogPackDecodeLimits::UNBOUNDED,
                DurableAdmissionLimitsV1::UNBOUNDED,
            )
            .unwrap();
            other_hot = Arc::new(
                HotStoreInstances::create_from_durable(
                    &view,
                    DurableHotStoreBootstrapLimits::default(),
                )
                .unwrap(),
            );
            other = view
                .into_rspace_authority(Arc::clone(&other_hot), other_gate)
                .unwrap_or_else(|failure| panic!("{:?}", failure.1));
        }
        populate(&other_hot);
        if variant == 0 {
            other_hot.put_datum(
                &"channel".to_owned(),
                Datum::create(&"channel".to_owned(), "different".to_owned(), false),
            );
        }
        other.checkpoint(publication_limits()).await.unwrap();
        if variant == 1 {
            other_hot.put_datum(
                &"channel".to_owned(),
                Datum::create(&"channel".to_owned(), "third".to_owned(), false),
            );
            other.checkpoint(publication_limits()).await.unwrap();
        }
        if variant == 2 {
            assert_eq!(
                other.state().unwrap(),
                expected.state(),
                "same semantic target, different physical terminal"
            );
            assert_ne!(other.generation_key(), GenerationLeaseKeyV1::from_catalog(expected));
        }
        let alternative = store_files(other_dir.path());
        replace_store_files(scratch.path(), &alternative);
        assert!(
            matches!(
                owner.reconcile(publication_limits()),
                Err(DurableRSpaceTransactionError::UnexpectedTerminal { .. })
            ),
            "variant {variant}"
        );
        assert_eq!(owner.phase(), JournalPublicationPhaseV1::RecoveryRequired);
        assert!(gate.try_read().is_err());
        assert_eq!(owner.pending_terminal(), Some(expected));
        assert!(!hot.pending_deltas().unwrap().data.unwrap().is_empty());
        assert!(
            owner
                .generation_leases()
                .has_active(owner.generation_key())
                .unwrap()
        );
        replace_store_files(scratch.path(), &original);
        assert_eq!(owner.reconcile(publication_limits()).unwrap(), expected.state());
        assert_empty(&hot);
        assert!(gate.try_read().is_ok());
    }
}

#[tokio::test]
async fn damaged_selected_publication_stays_nonready_until_the_same_files_are_repaired() {
    let (scratch, hot, gate, mut owner) = fixture();
    populate(&hot);
    arm_generation_checkpoint_publication_failpoint(
        GenerationCheckpointPublicationHookPointV1::AfterCatalogSyncBeforeRecovery,
    );
    assert!(owner.checkpoint(publication_limits()).await.is_err());
    let terminal = owner.pending_terminal().unwrap();
    let original = store_files(scratch.path());
    let mut damaged = original.clone();
    let pack = damaged
        .iter_mut()
        .find(|(name, _)| name.starts_with("pack-"))
        .unwrap()
        .1;
    *pack.last_mut().unwrap() ^= 1;
    replace_store_files(scratch.path(), &damaged);
    for _ in 0..3 {
        assert!(owner.reconcile(publication_limits()).is_err());
        assert_eq!(owner.phase(), JournalPublicationPhaseV1::RecoveryRequired);
        assert!(gate.try_read().is_err());
        assert_eq!(owner.pending_terminal(), Some(terminal));
        assert!(!hot.pending_deltas().unwrap().data.unwrap().is_empty());
        assert!(
            DurableDirectoryV1::open(scratch.path())
                .unwrap()
                .try_acquire_writer()
                .is_err()
        );
        assert_eq!(store_files(scratch.path()), damaged);
    }
    replace_store_files(scratch.path(), &original);
    assert_eq!(owner.reconcile(publication_limits()).unwrap(), terminal.state());
    assert_empty(&hot);
    assert_eq!(store_files(scratch.path()), original);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    #[test]
    fn owner_checkpoint_histories_preserve_multiplicities_across_retry_and_cold_restart(
        history in proptest::collection::vec((0usize..5, any::<bool>(), any::<bool>()), 1..9),
    ) {
        let runtime = tokio::runtime::Builder::new_current_thread().build().unwrap();
        runtime.block_on(async {
            let (scratch, mut hot, gate, mut owner) = fixture();
            populate(&hot);
            let channel = "channel".to_owned();
            for (step, (count, interrupt, published)) in history.into_iter().enumerate() {
                let before = hot.get_data(&channel).len();
                for _ in before..count { hot.put_datum(&channel, Datum::create(&channel, "datum".to_owned(), false)); }
                for _ in count..before { hot.remove_datum(&channel, 0).unwrap(); }
                let old = owner.state().unwrap();
                let changed = step == 0 || before != count;
                if interrupt && changed {
                    arm_generation_checkpoint_publication_failpoint(if published {
                        GenerationCheckpointPublicationHookPointV1::AfterCatalogSyncBeforeRecovery
                    } else { GenerationCheckpointPublicationHookPointV1::AfterCommitSync });
                    assert!(owner.checkpoint(publication_limits()).await.is_err());
                    assert!(gate.try_read().is_err());
                }
                let next = owner.checkpoint(publication_limits()).await.unwrap();
                assert_eq!(next.generation(), old.generation() + u64::from(changed));
                assert_eq!(hot.get_data(&channel).len(), count);
                assert_empty(&hot);
                assert!(gate.try_read().is_ok());
                drop(owner);
                let view = recover_read_authority::<Profile>(scratch.path(), limits(), LogPackDecodeLimits::UNBOUNDED, DurableAdmissionLimitsV1::UNBOUNDED).unwrap();
                assert_eq!(view.state(), next);
                hot = Arc::new(HotStoreInstances::create_from_durable(&view, DurableHotStoreBootstrapLimits::default()).unwrap());
                assert_eq!(hot.get_data(&channel).len(), count);
                assert_empty(&hot);
                owner = view.into_rspace_authority(Arc::clone(&hot), Arc::clone(&gate)).unwrap_or_else(|failure| panic!("{:?}", failure.1));
            }
        });
    }
}

#[test]
fn successor_pin_failure_retains_the_admitted_successor_old_pin_and_permit_for_retry() {
    use crate::rspace::persistence::durable::DurableRSpaceTransactionError;
    let (scratch, hot, gate, mut owner) = fixture();
    populate(&hot);
    let registry = owner.generation_leases();
    let old_key = owner.generation_key();
    let counter = registry.replace_lease_counter_for_test(u64::MAX);
    use std::future::Future;
    use std::task::{Context, Poll, Waker};
    {
        let mut checkpoint = std::pin::pin!(owner.checkpoint(publication_limits()));
        assert!(matches!(
            checkpoint
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop())),
            Poll::Ready(Err(DurableRSpaceTransactionError::Lease(
                crate::rspace::persistence::GenerationLeaseError::CounterOverflow
            )))
        ));
    }
    assert_eq!(owner.phase(), JournalPublicationPhaseV1::AwaitingAck);
    assert!(registry.has_active(old_key).unwrap());
    assert!(gate.try_read().is_err());
    assert!(!hot.pending_deltas().unwrap().data.unwrap().is_empty());
    let terminal = owner.pending_terminal().unwrap();
    let files = store_files(scratch.path());
    registry.replace_lease_counter_for_test(counter);
    assert_eq!(owner.reconcile(publication_limits()).unwrap(), terminal.state());
    assert_eq!(store_files(scratch.path()), files);
    assert_empty(&hot);
    assert!(gate.try_read().is_ok());
    assert!(!registry.has_active(old_key).unwrap());
    assert!(
        registry
            .has_active(GenerationLeaseKeyV1::from_catalog(terminal))
            .unwrap()
    );
}

#[tokio::test]
async fn a_stale_capture_is_rejected_before_publication_even_when_its_journal_bytes_match() {
    use crate::rspace::persistence::durable::journal_owner_hooks::{self, JournalOwnerHookPoint};
    for point in [JournalOwnerHookPoint::AfterCapture, JournalOwnerHookPoint::AfterPreparation] {
        let (scratch, hot, gate, mut owner) = fixture();
        populate(&hot);
        let old = owner.state().unwrap();
        let files = store_files(scratch.path());
        let changed = Arc::clone(&hot);
        let _hook = journal_owner_hooks::arm(point, move || {
            let pending = changed.pending_deltas().unwrap();
            changed
                .restore_pending_deltas(
                    pending.data.unwrap(),
                    pending.continuations.unwrap(),
                    pending.continuation_content.unwrap(),
                    pending.continuation_source_bytes.unwrap(),
                    pending.joins.unwrap(),
                )
                .unwrap();
        });
        assert!(owner.checkpoint(publication_limits()).await.is_err());
        assert_eq!(owner.state().unwrap(), old);
        assert_eq!(store_files(scratch.path()), files);
        assert!(!hot.pending_deltas().unwrap().data.unwrap().is_empty());
        assert!(gate.try_read().is_ok());
        owner.checkpoint(publication_limits()).await.unwrap();
        assert_empty(&hot);
    }
}

fn publication_limits() -> DurableRSpacePublicationLimitsV1 {
    DurableRSpacePublicationLimitsV1 {
        transaction: DagTransactionLimits::UNBOUNDED,
        recovery: limits(),
        wal: LogPackDecodeLimits::UNBOUNDED,
        admission: DurableAdmissionLimitsV1::UNBOUNDED,
    }
}

fn fixture() -> (PersistenceTrialDir, Store, Arc<RwLock<()>>, DurableRSpaceAuthorityV1<Profile>) {
    let scratch = trial_scratch("journal-owner-");
    let image = checkpoint_image_for_profile::<Profile>(fixture_for_profile(Profile::ID));
    install_checkpoint_image(scratch.path(), &image);
    let view = recover_read_authority::<Profile>(
        scratch.path(),
        limits(),
        LogPackDecodeLimits::UNBOUNDED,
        DurableAdmissionLimitsV1::UNBOUNDED,
    )
    .unwrap();
    let hot = Arc::new(
        HotStoreInstances::create_from_durable(&view, DurableHotStoreBootstrapLimits::default())
            .unwrap(),
    );
    let gate = Arc::new(RwLock::new(()));
    let owner = view
        .into_rspace_authority(Arc::clone(&hot), Arc::clone(&gate))
        .unwrap_or_else(|failure| panic!("{:?}", failure.1));
    (scratch, hot, gate, owner)
}

fn populate(hot: &Store) {
    let channels = vec!["channel".to_owned(), "channel".to_owned()];
    hot.put_datum(&channels[0], Datum::create(&channels[0], "datum".to_owned(), false));
    let continuation = WaitingContinuation::create(
        &channels,
        &vec!["a".to_owned(), "b".to_owned()],
        &"body".to_owned(),
        false,
        BTreeSet::new(),
    );
    assert_eq!(hot.put_continuation(&channels, continuation), Some(true));
    hot.put_join(&channels[0], &channels).unwrap();
}

fn assert_empty(hot: &Store) {
    let pending = hot.pending_deltas().unwrap();
    assert!(pending.data.unwrap().is_empty());
    assert!(pending.continuations.unwrap().is_empty());
    assert!(pending.continuation_content.unwrap().is_empty());
    assert!(pending.continuation_source_bytes.unwrap().is_empty());
    assert!(pending.joins.unwrap().is_empty());
}

#[tokio::test]
async fn owner_checkpoint_clears_all_journals_after_authenticated_publication_and_reopens() {
    let (scratch, hot, gate, mut owner) = fixture();
    let old = owner.state().unwrap();
    let registry = owner.generation_leases();
    let old_pin = owner.generation_key();
    populate(&hot);
    let before = hot.snapshot();
    let next = owner.checkpoint(publication_limits()).await.unwrap();
    assert_eq!(next.generation(), old.generation() + 1);
    assert_eq!(owner.phase(), JournalPublicationPhaseV1::Ready);
    assert!(gate.try_read().is_ok());
    assert_empty(&hot);
    assert_eq!(before.data, hot.snapshot().data);
    assert_eq!(before.continuations, hot.snapshot().continuations);
    assert_eq!(before.joins, hot.snapshot().joins);
    assert!(!registry.has_active(old_pin).unwrap());
    assert!(registry.has_active(owner.generation_key()).unwrap());
    assert!(
        DurableDirectoryV1::open(scratch.path())
            .unwrap()
            .try_acquire_writer()
            .is_err()
    );
    drop(owner);
    let reopened = recover_read_authority::<Profile>(
        scratch.path(),
        limits(),
        LogPackDecodeLimits::UNBOUNDED,
        DurableAdmissionLimitsV1::UNBOUNDED,
    )
    .unwrap();
    assert_eq!(reopened.state(), next);
    let cold = HotStoreInstances::create_from_durable(
        &reopened,
        DurableHotStoreBootstrapLimits::default(),
    )
    .unwrap();
    assert_eq!(before.data, cold.snapshot().data);
    assert_eq!(before.continuations, cold.snapshot().continuations);
    assert_eq!(before.joins, cold.snapshot().joins);
    assert!(cold.pending_deltas().unwrap().data.unwrap().is_empty());
}

#[tokio::test]
async fn owner_noop_and_preparation_refusal_leave_files_journals_and_authority_unchanged() {
    let (scratch, hot, gate, mut owner) = fixture();
    let old = owner.state().unwrap();
    let catalog = fs::read(scratch.path().join("catalog.v1")).unwrap();
    assert_eq!(owner.checkpoint(publication_limits()).await.unwrap(), old);
    assert_empty(&hot);
    populate(&hot);
    let pending = hot.pending_deltas().unwrap();
    let mut refused = publication_limits();
    refused.wal.max_objects = 0;
    assert!(owner.checkpoint(refused).await.is_err());
    assert_eq!(owner.state().unwrap(), old);
    assert_eq!(fs::read(scratch.path().join("catalog.v1")).unwrap(), catalog);
    assert_eq!(pending.data, hot.pending_deltas().unwrap().data);
    assert!(gate.try_read().is_ok());
    assert!(
        DurableDirectoryV1::open(scratch.path())
            .unwrap()
            .try_acquire_writer()
            .is_err()
    );
    owner.checkpoint(publication_limits()).await.unwrap();
    assert_empty(&hot);
}

#[tokio::test]
async fn owner_uncertain_old_and_new_publications_retain_the_gate_and_original_writer() {
    for point in [
        GenerationCheckpointPublicationHookPointV1::AfterPrepareWrite,
        GenerationCheckpointPublicationHookPointV1::AfterCommitSync,
        GenerationCheckpointPublicationHookPointV1::AfterCatalogSlotWrite,
        GenerationCheckpointPublicationHookPointV1::AfterCatalogSyncBeforeRecovery,
    ] {
        let (scratch, hot, gate, mut owner) = fixture();
        populate(&hot);
        let old = owner.state().unwrap();
        let registry = owner.generation_leases();
        let old_pin = owner.generation_key();
        arm_generation_checkpoint_publication_failpoint(point);
        assert!(owner.checkpoint(publication_limits()).await.is_err(), "{point:?}");
        assert_eq!(owner.phase(), JournalPublicationPhaseV1::RecoveryRequired);
        assert!(owner.state().is_err());
        assert!(gate.try_read().is_err());
        assert!(registry.has_active(old_pin).unwrap());
        assert!(
            DurableDirectoryV1::open(scratch.path())
                .unwrap()
                .try_acquire_writer()
                .is_err()
        );
        assert!(!hot.pending_deltas().unwrap().data.unwrap().is_empty());
        let terminal = owner.pending_terminal();
        let recovered = owner.reconcile(publication_limits()).unwrap();
        assert!(gate.try_read().is_ok());
        if matches!(
            point,
            GenerationCheckpointPublicationHookPointV1::AfterPrepareWrite |
                GenerationCheckpointPublicationHookPointV1::AfterCommitSync
        ) {
            assert_eq!(recovered, old);
            assert!(!hot.pending_deltas().unwrap().data.unwrap().is_empty());
            owner.checkpoint(publication_limits()).await.unwrap();
        } else {
            assert_eq!(recovered, terminal.unwrap().state());
            assert_empty(&hot);
            assert_eq!(owner.checkpoint(publication_limits()).await.unwrap(), recovered);
        }
        assert_empty(&hot);
        assert_eq!(owner.state().unwrap().generation(), old.generation() + 1);
    }
}

#[test]
fn owner_unwind_keeps_the_expected_terminal_and_recovers_without_republication() {
    assert_authority_unwind_child("publication", "owner publication unwind");
}

fn run_owner_publication_unwind_child(case: &str) {
    struct VerifyOnDrop {
        scratch: PersistenceTrialDir,
        hot: Store,
        gate: Arc<RwLock<()>>,
        owner: DurableRSpaceAuthorityV1<Profile>,
        case: String,
        marker: std::path::PathBuf,
    }
    impl Drop for VerifyOnDrop {
        fn drop(&mut self) {
            assert!(self.gate.try_read().is_err());
            assert_eq!(self.owner.phase(), JournalPublicationPhaseV1::RecoveryRequired);
            let terminal = self.owner.pending_terminal().unwrap();
            let catalog = fs::read(self.scratch.path().join("catalog.v1")).unwrap();
            assert_eq!(self.owner.reconcile(publication_limits()).unwrap(), terminal.state());
            assert_eq!(fs::read(self.scratch.path().join("catalog.v1")).unwrap(), catalog);
            assert_empty(&self.hot);
            assert!(self.gate.try_read().is_ok());
            assert!(
                self.owner
                    .generation_leases()
                    .has_active(GenerationLeaseKeyV1::from_catalog(terminal))
                    .unwrap()
            );
            fs::write(&self.marker, self.case.as_bytes()).unwrap();
        }
    }
    let (scratch, hot, gate, owner) = fixture();
    populate(&hot);
    let mut verify = VerifyOnDrop {
        scratch,
        hot,
        gate,
        owner,
        case: case.to_owned(),
        marker: authority_unwind_marker(),
    };
    arm_generation_checkpoint_publication_hook(
        GenerationCheckpointPublicationHookPointV1::AfterCatalogSyncBeforeRecovery,
        || panic!("owner publication unwind"),
    );
    poll_checkpoint(&mut verify.owner);
}

#[tokio::test]
async fn cancelling_a_checkpoint_waiting_for_the_exact_gate_does_not_capture_or_publish() {
    use std::future::Future;
    use std::task::{Context, Poll, Waker};
    let (scratch, hot, gate, mut owner) = fixture();
    populate(&hot);
    let old = owner.state().unwrap();
    let catalog = fs::read(scratch.path().join("catalog.v1")).unwrap();
    let held = gate.read().await;
    {
        let mut checkpoint = std::pin::pin!(owner.checkpoint(publication_limits()));
        assert!(matches!(
            checkpoint
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop())),
            Poll::Pending
        ));
    }
    assert_eq!(owner.state().unwrap(), old);
    assert_eq!(owner.phase(), JournalPublicationPhaseV1::Ready);
    assert_eq!(fs::read(scratch.path().join("catalog.v1")).unwrap(), catalog);
    assert!(!hot.pending_deltas().unwrap().data.unwrap().is_empty());
    drop(held);
    owner.checkpoint(publication_limits()).await.unwrap();
    assert_empty(&hot);
}

#[tokio::test]
async fn invalidated_base_cannot_release_a_refused_capture_as_ready() {
    use crate::rspace::persistence::durable::journal_owner_hooks::{self, JournalOwnerHookPoint};
    for point in [JournalOwnerHookPoint::AfterCapture, JournalOwnerHookPoint::AfterPreparation] {
        for replace in [false, true] {
            let (scratch, hot, gate, mut owner) = fixture();
            populate(&hot);
            let files = store_files(scratch.path());
            let changed = Arc::clone(&hot);
            let _hook = journal_owner_hooks::arm(point, move || {
                if replace {
                    changed.set_state(changed.snapshot());
                } else {
                    changed.clear();
                }
            });
            assert!(owner.checkpoint(publication_limits()).await.is_err());
            assert!(owner.state().is_err(), "invalid hot base reported Ready at {point:?}");
            assert!(gate.try_read().is_err());
            assert!(owner.reconcile(publication_limits()).is_err());
            assert!(gate.try_read().is_err());
            assert_eq!(store_files(scratch.path()), files);
            assert!(
                DurableDirectoryV1::open(scratch.path())
                    .unwrap()
                    .try_acquire_writer()
                    .is_err()
            );
        }
    }
}

#[test]
fn invalidated_base_cannot_release_old_recovery_or_report_ready() {
    for capture_unwind in [false, true] {
        for replace in [false, true] {
            if capture_unwind {
                assert_authority_unwind_child(&format!("invalid-base:{replace}"), "capture unwind");
                continue;
            }
            let (scratch, hot, gate, mut owner) = fixture();
            populate(&hot);
            arm_generation_checkpoint_publication_failpoint(
                GenerationCheckpointPublicationHookPointV1::AfterCommitSync,
            );
            poll_checkpoint(&mut owner);
            let files = store_files(scratch.path());
            if replace {
                hot.set_state(hot.snapshot());
            } else {
                hot.clear();
            }
            for _ in 0..3 {
                assert!(
                    owner.reconcile(publication_limits()).is_err(),
                    "invalid hot base accepted after exact old recovery"
                );
                assert!(owner.state().is_err());
                assert!(gate.try_read().is_err());
                assert_eq!(store_files(scratch.path()), files);
                assert!(
                    owner
                        .generation_leases()
                        .has_active(owner.generation_key())
                        .unwrap()
                );
            }
        }
    }
    for replace in [false, true] {
        let (_scratch, hot, _gate, owner) = fixture();
        if replace {
            hot.set_state(hot.snapshot());
        } else {
            hot.clear();
        }
        assert!(owner.state().is_err());
        assert_ne!(owner.phase(), JournalPublicationPhaseV1::Ready);
    }
}

fn run_invalid_base_unwind_child(replace: bool, case: &str) {
    use crate::rspace::persistence::durable::journal_owner_hooks::{self, JournalOwnerHookPoint};
    struct VerifyOnDrop {
        scratch: PersistenceTrialDir,
        hot: Store,
        gate: Arc<RwLock<()>>,
        owner: DurableRSpaceAuthorityV1<Profile>,
        replace: bool,
        case: String,
        marker: std::path::PathBuf,
    }
    impl Drop for VerifyOnDrop {
        fn drop(&mut self) {
            let files = store_files(self.scratch.path());
            if self.replace {
                self.hot.set_state(self.hot.snapshot());
            } else {
                self.hot.clear();
            }
            for _ in 0..3 {
                assert!(
                    self.owner.reconcile(publication_limits()).is_err(),
                    "invalid hot base accepted after exact old recovery"
                );
                assert!(self.owner.state().is_err());
                assert!(self.gate.try_read().is_err());
                assert_eq!(store_files(self.scratch.path()), files);
                assert!(
                    self.owner
                        .generation_leases()
                        .has_active(self.owner.generation_key())
                        .unwrap()
                );
            }
            fs::write(&self.marker, self.case.as_bytes()).unwrap();
        }
    }
    let (scratch, hot, gate, owner) = fixture();
    populate(&hot);
    let mut verify = VerifyOnDrop {
        scratch,
        hot,
        gate,
        owner,
        replace,
        case: case.to_owned(),
        marker: authority_unwind_marker(),
    };
    let _hook =
        journal_owner_hooks::arm(JournalOwnerHookPoint::AfterCapture, || panic!("capture unwind"));
    poll_checkpoint(&mut verify.owner);
}

fn assert_journals_unchanged(hot: &Store, before: &crate::rspace::hot_store::PendingDeltas) {
    let after = hot.pending_deltas().unwrap();
    assert_eq!(before.data, after.data);
    assert_eq!(before.continuations, after.continuations);
    assert_eq!(before.joins, after.joins);
    assert!(
        before
            .continuation_content
            .as_ref()
            .unwrap()
            .iter()
            .eq(after.continuation_content.as_ref().unwrap().iter())
    );
    assert!(
        before
            .continuation_source_bytes
            .as_ref()
            .unwrap()
            .iter()
            .eq(after.continuation_source_bytes.as_ref().unwrap().iter())
    );
}

/// A journal owner over an admitted V2 checkpoint refuses its checkpoint when
/// it binds the successor, before any durable mutation, and stays ready with
/// its journals and operation gate intact.
pub(super) fn assert_v2_owner_checkpoint_refuses_before_durable_mutation() {
    use std::future::Future;
    use std::task::{Context, Poll, Waker};
    let scratch = trial_scratch("journal-owner-v2-");
    let source = migrate_checkpoint_source_to_v2(&fixture_for_profile(Profile::ID));
    install_checkpoint_image(scratch.path(), &checkpoint_image_for_profile::<Profile>(source));
    let view = recover_read_authority::<Profile>(
        scratch.path(),
        limits(),
        LogPackDecodeLimits::UNBOUNDED,
        DurableAdmissionLimitsV1::UNBOUNDED,
    )
    .expect("the dual reader adopts a V2 checkpoint");
    let hot: Store = Arc::new(
        HotStoreInstances::create_from_durable(&view, DurableHotStoreBootstrapLimits::default())
            .expect("a V2 checkpoint bootstraps the hot store"),
    );
    let gate = Arc::new(RwLock::new(()));
    let mut owner = view
        .into_rspace_authority(Arc::clone(&hot), Arc::clone(&gate))
        .unwrap_or_else(|failure| panic!("{:?}", failure.1));
    let old = owner.state().expect("a fresh owner is ready");
    populate(&hot);
    let files = store_files(scratch.path());
    let before = hot
        .pending_deltas()
        .expect("the populated journals are readable");
    let result = {
        let mut checkpoint = std::pin::pin!(owner.checkpoint(publication_limits()));
        match checkpoint
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
        {
            Poll::Ready(result) => result,
            Poll::Pending => panic!("an uncontended owner checkpoint completes in one poll"),
        }
    };
    assert!(
        matches!(
            result,
            Err(DurableRSpaceTransactionError::Preparation(
                DurableTransactionPreparationError::RetentionFormatNotWritable(RetentionFormat::V2)
            ))
        ),
        "{result:?}"
    );
    assert_eq!(
        owner
            .state()
            .expect("a refused successor leaves the owner ready"),
        old
    );
    assert_eq!(owner.phase(), JournalPublicationPhaseV1::Ready);
    assert_eq!(store_files(scratch.path()), files);
    assert_journals_unchanged(&hot, &before);
    assert!(gate.try_read().is_ok(), "the refused checkpoint released the operation gate");
}

#[test]
fn acknowledgement_conflict_retains_both_pins_and_never_republishes() {
    use crate::rspace::persistence::durable::journal_owner_hooks::{self, JournalOwnerHookPoint};
    for point in
        [JournalOwnerHookPoint::AfterSuccessorPin, JournalOwnerHookPoint::AfterAcknowledgement]
    {
        let (scratch, hot, gate, mut owner) = fixture();
        populate(&hot);
        let old_key = owner.generation_key();
        let changed = Arc::clone(&hot);
        let _hook = journal_owner_hooks::arm(point, move || {
            let pending = changed.pending_deltas().unwrap();
            changed
                .restore_pending_deltas(
                    pending.data.unwrap(),
                    pending.continuations.unwrap(),
                    pending.continuation_content.unwrap(),
                    pending.continuation_source_bytes.unwrap(),
                    pending.joins.unwrap(),
                )
                .unwrap();
        });
        poll_checkpoint(&mut owner);
        let terminal = owner.pending_terminal().unwrap();
        let files = store_files(scratch.path());
        let pending = hot.pending_deltas().unwrap();
        for _ in 0..3 {
            assert!(owner.reconcile(publication_limits()).is_err());
            assert_eq!(owner.phase(), JournalPublicationPhaseV1::AwaitingAck);
            assert!(gate.try_read().is_err());
            assert!(owner.generation_leases().has_active(old_key).unwrap());
            assert!(
                owner
                    .generation_leases()
                    .has_active(GenerationLeaseKeyV1::from_catalog(terminal))
                    .unwrap()
            );
            assert_eq!(store_files(scratch.path()), files);
            assert_journals_unchanged(&hot, &pending);
        }
    }
    assert_authority_unwind_child("ack-conflict", "acknowledgement completion unwind");
}

fn run_ack_conflict_unwind_child(case: &str) {
    use crate::rspace::persistence::durable::journal_owner_hooks::{self, JournalOwnerHookPoint};
    struct VerifyOnDrop {
        scratch: PersistenceTrialDir,
        hot: Store,
        gate: Arc<RwLock<()>>,
        owner: DurableRSpaceAuthorityV1<Profile>,
        old_key: GenerationLeaseKeyV1,
        case: String,
        marker: std::path::PathBuf,
    }
    impl Drop for VerifyOnDrop {
        fn drop(&mut self) {
            let terminal = self.owner.pending_terminal().unwrap();
            let files = store_files(self.scratch.path());
            let pending = self.hot.pending_deltas().unwrap();
            for _ in 0..3 {
                assert!(self.owner.reconcile(publication_limits()).is_err());
                assert_eq!(self.owner.phase(), JournalPublicationPhaseV1::AwaitingAck);
                assert!(self.gate.try_read().is_err());
                assert!(
                    self.owner
                        .generation_leases()
                        .has_active(self.old_key)
                        .unwrap()
                );
                assert!(
                    self.owner
                        .generation_leases()
                        .has_active(GenerationLeaseKeyV1::from_catalog(terminal))
                        .unwrap()
                );
                assert_eq!(store_files(self.scratch.path()), files);
                assert_journals_unchanged(&self.hot, &pending);
            }
            fs::write(&self.marker, self.case.as_bytes()).unwrap();
        }
    }
    let (scratch, hot, gate, owner) = fixture();
    populate(&hot);
    let old_key = owner.generation_key();
    let mut verify = VerifyOnDrop {
        scratch,
        hot,
        gate,
        owner,
        old_key,
        case: case.to_owned(),
        marker: authority_unwind_marker(),
    };
    let changed = Arc::clone(&verify.hot);
    let _hook = journal_owner_hooks::arm(JournalOwnerHookPoint::AfterAcknowledgement, move || {
        let pending = changed.pending_deltas().unwrap();
        changed
            .restore_pending_deltas(
                pending.data.unwrap(),
                pending.continuations.unwrap(),
                pending.continuation_content.unwrap(),
                pending.continuation_source_bytes.unwrap(),
                pending.joins.unwrap(),
            )
            .unwrap();
        panic!("acknowledgement completion unwind");
    });
    poll_checkpoint(&mut verify.owner);
}

#[tokio::test]
async fn cold_restart_before_acknowledgement_selects_old_or_new_without_delta_replay() {
    for published in [false, true] {
        let (scratch, hot, gate, mut owner) = fixture();
        let old = owner.state().unwrap();
        populate(&hot);
        arm_generation_checkpoint_publication_failpoint(if published {
            GenerationCheckpointPublicationHookPointV1::AfterCatalogSyncBeforeRecovery
        } else {
            GenerationCheckpointPublicationHookPointV1::AfterCommitSync
        });
        assert!(owner.checkpoint(publication_limits()).await.is_err());
        let expected = owner.pending_terminal().unwrap();
        assert!(!hot.pending_deltas().unwrap().data.unwrap().is_empty());
        drop(owner);
        drop(hot);
        assert!(gate.try_read().is_ok());
        let view = recover_read_authority::<Profile>(
            scratch.path(),
            limits(),
            LogPackDecodeLimits::UNBOUNDED,
            DurableAdmissionLimitsV1::UNBOUNDED,
        )
        .unwrap();
        assert_eq!(view.state(), if published { expected.state() } else { old });
        let cold = Arc::new(
            HotStoreInstances::create_from_durable(
                &view,
                DurableHotStoreBootstrapLimits::default(),
            )
            .unwrap(),
        );
        assert_empty(&cold);
        let channel = "channel".to_owned();
        assert_eq!(cold.get_data(&channel).len(), usize::from(published));
        assert_eq!(
            cold.get_continuations(&[channel.clone(), channel.clone()])
                .len(),
            usize::from(published)
        );
        assert_eq!(cold.get_joins(&channel).len(), usize::from(published));
        let mut reopened = view
            .into_rspace_authority(Arc::clone(&cold), gate)
            .unwrap_or_else(|failure| panic!("{:?}", failure.1));
        cold.put_datum(&channel, Datum::create(&channel, "datum".to_owned(), false));
        let next = reopened.checkpoint(publication_limits()).await.unwrap();
        assert_eq!(next.generation(), old.generation() + u64::from(published) + 1);
        assert_eq!(cold.get_data(&channel).len(), usize::from(published) + 1);
        assert_empty(&cold);
    }
}

#[tokio::test]
async fn constructor_refuses_foreign_or_invalidated_hot_authority_without_losing_its_view() {
    for invalidated in [false, true] {
        let (scratch, _old_hot, gate, owner) = fixture();
        let (other_scratch, other_hot, _other_gate, other_owner) = fixture();
        drop(owner);
        let view = recover_read_authority::<Profile>(
            scratch.path(),
            limits(),
            LogPackDecodeLimits::UNBOUNDED,
            DurableAdmissionLimitsV1::UNBOUNDED,
        )
        .unwrap();
        assert_eq!(view.state(), other_owner.state().unwrap());
        assert_eq!(store_files(scratch.path()), store_files(other_scratch.path()));
        let pin = GenerationLeaseKeyV1::from_catalog(view.catalog());
        let registry = view.generation_leases();
        let correct = Arc::new(
            HotStoreInstances::create_from_durable(
                &view,
                DurableHotStoreBootstrapLimits::default(),
            )
            .unwrap(),
        );
        let invalid = if invalidated {
            let hot = Arc::new(
                HotStoreInstances::create_from_durable(
                    &view,
                    DurableHotStoreBootstrapLimits::default(),
                )
                .unwrap(),
            );
            hot.clear();
            hot
        } else {
            Arc::clone(&other_hot)
        };
        let failure = view
            .into_rspace_authority(invalid, Arc::clone(&gate))
            .err()
            .expect("crossed authority must be refused");
        let (view, _) = *failure;
        assert!(registry.shares_registry(&view.generation_leases()));
        assert!(registry.has_active(pin).unwrap());
        assert!(
            DurableDirectoryV1::open(scratch.path())
                .unwrap()
                .try_acquire_writer()
                .is_err()
        );
        assert_eq!(store_files(scratch.path()), store_files(other_scratch.path()));
        let mut owner = view
            .into_rspace_authority(Arc::clone(&correct), Arc::clone(&gate))
            .unwrap_or_else(|failure| panic!("{:?}", failure.1));
        populate(&correct);
        let next = owner.checkpoint(publication_limits()).await.unwrap();
        assert_eq!(next.generation(), other_owner.state().unwrap().generation() + 1);
        assert_empty(&correct);
        assert_empty(&other_hot);
    }
}
