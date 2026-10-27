use std::path::Path;

use super::checkpoint_wal::fixture;
use super::durable_publication::install_checkpoint_image;
use super::durable_recovery::{checkpoint_image_from_source, limits};
use super::support::{PersistenceTrialDir, TestSemanticProfile, trial_scratch};
use super::transaction_sealing::data_batch;
use crate::rspace::persistence::closure::{cold_closure_walks, reset_cold_closure_walks};
use crate::rspace::persistence::durable::{
    DurableAdmissionErrorV1, DurableAdmissionLimitKindV1, DurableAdmissionLimitsV1,
    DurableDirectoryV1, DurableIoError, DurableTransactionPreparationError, DurableWriterLeaseV1,
    SemanticallyAdmittedDurableWriterV1, acquire_cold_checked, cold_admissions,
    publish_incremental_from_lease_v1, reset_cold_admissions,
};
use crate::rspace::persistence::transaction::ValidityOnlyV1;
use crate::rspace::persistence::{
    CATALOG_IMAGE_LEN, CATALOG_SLOT_LEN, CanonicalFormatError, CatalogAuthorityTokenV1,
    DagTransactionLimits, LogPackDecodeLimits, MutationBatchViewV1, ObjectRefV1,
    OpenDagTransaction, SealedDagTransaction, StructuralValidationLimits, VerifiedDagState,
};

fn installed(prefix: &str) -> PersistenceTrialDir {
    let directory = trial_scratch(prefix);
    install_checkpoint_image(directory.path(), &checkpoint_image_from_source(fixture()));
    directory
}

#[track_caller]
fn admit(root: &Path) -> SemanticallyAdmittedDurableWriterV1<TestSemanticProfile> {
    DurableDirectoryV1::open(root)
        .expect("open the installed durable directory")
        .recover_and_admit_v1::<TestSemanticProfile>(
            limits(),
            LogPackDecodeLimits::UNBOUNDED,
            DurableAdmissionLimitsV1::UNBOUNDED,
        )
        .expect("cold admission of the installed checkpoint")
}

fn sealed_objects(
    sealed: &SealedDagTransaction<'_, TestSemanticProfile>,
) -> Vec<(ObjectRefV1, Vec<u8>)> {
    sealed
        .objects()
        .iter()
        .map(|object| (object.reference(), object.canonical_bytes().to_vec()))
        .collect()
}

#[track_caller]
fn assert_writer_held(lease: &DurableWriterLeaseV1) {
    let reader = lease
        .directory()
        .try_clone_reader()
        .expect("clone a reader of the leased directory");
    assert!(matches!(reader.try_acquire_writer(), Err(DurableIoError::WriterLockContended { .. })));
}

fn incremental_control(identity: &str) -> CatalogAuthorityTokenV1 {
    let directory = installed("authority-core-incremental-control-");
    let writer = admit(directory.path());
    let mutation = data_batch(writer.catalog().state(), identity);
    let sealed = writer
        .seal_transaction(
            MutationBatchViewV1::parse(&mutation).expect("parse the control batch"),
            DagTransactionLimits::UNBOUNDED,
            DurableAdmissionLimitsV1::UNBOUNDED,
        )
        .expect("seal the control transaction");
    let prepared = writer
        .prepare_transaction(
            &sealed,
            writer
                .catalog()
                .commit_sequence()
                .successor()
                .expect("the control commit sequence has a successor"),
            LogPackDecodeLimits::UNBOUNDED,
        )
        .expect("prepare the control transaction");
    let published = writer
        .publish_incremental_transaction_v1(
            &prepared,
            limits(),
            LogPackDecodeLimits::UNBOUNDED,
            DurableAdmissionLimitsV1::UNBOUNDED,
        )
        .expect("publish the control successor");
    published.authority().clone()
}

fn checkpoint_control() -> CatalogAuthorityTokenV1 {
    let directory = installed("authority-core-checkpoint-control-");
    let published = admit(directory.path())
        .publish_generation_checkpoint_v1(
            limits(),
            LogPackDecodeLimits::UNBOUNDED,
            DurableAdmissionLimitsV1::UNBOUNDED,
        )
        .expect("publish the control checkpoint");
    published.authority().clone()
}

#[test]
fn authority_core_projects_the_cold_source_without_cold_work() {
    let directory = installed("authority-core-projection-");
    reset_cold_admissions();
    reset_cold_closure_walks();
    let (_lease, source) = admit(directory.path()).into_parts();
    let report = source.report();
    assert_eq!(cold_admissions(), 1);
    assert_eq!(cold_closure_walks(), report.retained_states);

    let core = source.core();
    let cold = source.verified_state();
    let validity = core.verified_state();
    assert_eq!(validity.version(), cold.version());
    assert_eq!(core.authority().slot().state(), cold.version());
    assert_eq!(core.validity().manifest(), cold.version().manifest());
    assert!(std::ptr::eq(cold.object_store(), validity.object_store()));
    assert_eq!(core.authority(), source.authority());
    assert_eq!(core.shape(), source.shape());
    assert_eq!(core.retention_manifest_bytes(), source.retention_manifest_bytes());
    assert_eq!(core.retention_format(), source.retention_format());
    assert_eq!(core.retained_states(), report.retained_states);

    let maxima = core.retained_state_maxima();
    let at_maxima = DurableAdmissionLimitsV1 {
        per_state_structural: StructuralValidationLimits {
            max_distinct_objects: maxima.distinct_objects,
            max_canonical_bytes: maxima.canonical_bytes,
            ..StructuralValidationLimits::unbounded()
        },
        ..DurableAdmissionLimitsV1::UNBOUNDED
    };
    source
        .preflight_checkpoint_admission(at_maxima)
        .expect("the cold requirements fit the retained-state maxima");
    let fewer_objects = maxima
        .distinct_objects
        .checked_sub(1)
        .expect("an admitted state holds at least its manifest");
    let below_objects = DurableAdmissionLimitsV1 {
        per_state_structural: StructuralValidationLimits {
            max_distinct_objects: fewer_objects,
            ..at_maxima.per_state_structural
        },
        ..at_maxima
    };
    assert!(matches!(
        source.preflight_checkpoint_admission(below_objects),
        Err(refusal) if refusal.resource == DurableAdmissionLimitKindV1::PerStateDistinctObjects &&
            refusal.limit == fewer_objects &&
            refusal.attempted == maxima.distinct_objects
    ));
    let fewer_bytes = maxima
        .canonical_bytes
        .checked_sub(1)
        .expect("an admitted state holds canonical bytes");
    let below_bytes = DurableAdmissionLimitsV1 {
        per_state_structural: StructuralValidationLimits {
            max_canonical_bytes: fewer_bytes,
            ..at_maxima.per_state_structural
        },
        ..at_maxima
    };
    assert!(matches!(
        source.preflight_checkpoint_admission(below_bytes),
        Err(refusal) if refusal.resource == DurableAdmissionLimitKindV1::PerStateCanonicalBytes &&
            refusal.limit == fewer_bytes &&
            refusal.attempted == maxima.canonical_bytes
    ));

    let mutation = data_batch(cold.version(), "authority-core-projection");
    let through_source = source
        .open_transaction(
            MutationBatchViewV1::parse(&mutation).expect("parse the projection batch"),
            DagTransactionLimits::UNBOUNDED,
        )
        .and_then(OpenDagTransaction::seal)
        .expect("seal through the cold-checked source");
    let through_core = OpenDagTransaction::new(
        validity,
        MutationBatchViewV1::parse(&mutation).expect("parse the projection batch"),
        DagTransactionLimits::UNBOUNDED,
    )
    .and_then(OpenDagTransaction::seal)
    .expect("seal through the validity-only core state");
    assert_eq!(through_core.receipt(), through_source.receipt());
    assert_eq!(sealed_objects(&through_core), sealed_objects(&through_source));
    assert_eq!(cold_admissions(), 1);
    assert_eq!(cold_closure_walks(), report.retained_states);

    let state = cold.version();
    let authority = core.authority().clone();
    let core = source.into_core();
    assert_eq!(core.verified_state().version(), state);
    assert_eq!(core.authority(), &authority);
    assert_eq!(cold_admissions(), 1);
    assert_eq!(cold_closure_walks(), report.retained_states);
}

#[test]
fn semantic_validity_advances_only_along_its_own_sealed_receipt() {
    let directory = installed("authority-core-validity-");
    let (_lease, source) = admit(directory.path()).into_parts();
    let core = source.into_core();
    let base = core.verified_state().version();
    let mutation = data_batch(base, "authority-core-validity");
    let sealed = OpenDagTransaction::new(
        core.verified_state(),
        MutationBatchViewV1::parse(&mutation).expect("parse the validity batch"),
        DagTransactionLimits::UNBOUNDED,
    )
    .and_then(OpenDagTransaction::seal)
    .expect("seal through the validity-only core state");
    let receipt = sealed.receipt();
    assert_eq!(receipt.base(), base);
    assert_ne!(receipt.target().manifest(), base.manifest());

    let next = core
        .validity()
        .successor(&sealed)
        .expect("the receipt's base is the validated state");
    assert_eq!(next.manifest(), receipt.target().manifest());
    assert!(matches!(next.successor(&sealed), Err(CanonicalFormatError::SemanticCoherence { .. })));

    let store = core.verified_state().object_store();
    let rebound = VerifiedDagState::<_, TestSemanticProfile, ValidityOnlyV1>::from_validity(
        store,
        base,
        core.validity(),
    )
    .expect("the validity token names its own state");
    assert_eq!(rebound.version(), base);
    assert!(matches!(
        VerifiedDagState::<_, TestSemanticProfile, ValidityOnlyV1>::from_validity(
            store,
            receipt.target(),
            core.validity(),
        ),
        Err(CanonicalFormatError::SemanticCoherence { .. })
    ));
    assert!(matches!(
        VerifiedDagState::<_, TestSemanticProfile, ValidityOnlyV1>::from_validity(
            store, base, next,
        ),
        Err(CanonicalFormatError::SemanticCoherence { .. })
    ));
}

#[test]
fn cold_checked_acquisition_is_bound_to_the_exact_catalog_image() {
    let control = incremental_control("authority-core-acquire");
    let directory = installed("authority-core-acquire-");
    let (mut lease, source) = admit(directory.path()).into_parts();
    let stale = source.into_core();
    reset_cold_admissions();
    reset_cold_closure_walks();
    let current = acquire_cold_checked(
        &mut lease,
        &stale,
        limits(),
        LogPackDecodeLimits::UNBOUNDED,
        DurableAdmissionLimitsV1::UNBOUNDED,
    )
    .expect("acquire a cold-checked source for the unchanged catalog");
    assert_eq!(cold_admissions(), 1);
    assert_eq!(cold_closure_walks(), current.report().retained_states);
    assert_eq!(current.authority(), stale.authority());
    assert_eq!(current.retention_manifest_bytes(), stale.retention_manifest_bytes());
    assert_eq!(current.shape(), stale.shape());
    assert_eq!(current.verified_state().version(), stale.verified_state().version());

    let mutation = data_batch(current.verified_state().version(), "authority-core-acquire");
    let sealed = current
        .seal_transaction(
            &lease,
            MutationBatchViewV1::parse(&mutation).expect("parse the acquisition batch"),
            DagTransactionLimits::UNBOUNDED,
            DurableAdmissionLimitsV1::UNBOUNDED,
        )
        .expect("seal through the acquired source");
    let sequence = current
        .authority()
        .slot()
        .commit_sequence()
        .successor()
        .expect("the acquired commit sequence has a successor");
    let prepared = current
        .prepare_transaction(&lease, &sealed, sequence, LogPackDecodeLimits::UNBOUNDED)
        .expect("prepare through the acquired source");
    let successor = publish_incremental_from_lease_v1(
        &mut lease,
        &current,
        &prepared,
        limits(),
        LogPackDecodeLimits::UNBOUNDED,
        DurableAdmissionLimitsV1::UNBOUNDED,
    )
    .expect("publish through the acquired source");
    assert_eq!(cold_admissions(), 2);
    assert_eq!(successor.verified_state().version(), sealed.receipt().target());
    assert_eq!(successor.authority(), &control);

    assert!(matches!(
        acquire_cold_checked(
            &mut lease,
            &stale,
            limits(),
            LogPackDecodeLimits::UNBOUNDED,
            DurableAdmissionLimitsV1::UNBOUNDED,
        ),
        Err(DurableAdmissionErrorV1::AuthorityChanged)
    ));
    assert_writer_held(&lease);

    let successor_core = successor.into_core();
    let fresh = acquire_cold_checked(
        &mut lease,
        &successor_core,
        limits(),
        LogPackDecodeLimits::UNBOUNDED,
        DurableAdmissionLimitsV1::UNBOUNDED,
    )
    .expect("acquire a cold-checked source for the published successor");
    assert_eq!(fresh.authority(), successor_core.authority());

    let mut reslotted = [0u8; CATALOG_IMAGE_LEN];
    reslotted[..CATALOG_SLOT_LEN].copy_from_slice(&successor_core.authority().slot().encode());
    assert_ne!(&reslotted, successor_core.authority().image());
    let reselected = CatalogAuthorityTokenV1::select(reslotted)
        .expect("the re-slotted catalog image parses")
        .expect("the re-slotted catalog image selects a slot");
    assert_eq!(reselected.slot(), successor_core.authority().slot());
    std::fs::write(directory.path().join("catalog.v1"), reslotted)
        .expect("rewrite the catalog image with the same selected slot");
    assert!(matches!(
        acquire_cold_checked(
            &mut lease,
            &successor_core,
            limits(),
            LogPackDecodeLimits::UNBOUNDED,
            DurableAdmissionLimitsV1::UNBOUNDED,
        ),
        Err(DurableAdmissionErrorV1::AuthorityChanged)
    ));
    assert_writer_held(&lease);
}

#[test]
fn rebound_cold_checked_writer_publishes_the_same_fresh_generation() {
    let expected = checkpoint_control();
    let directory = installed("authority-core-rebind-");
    let foreign = installed("authority-core-rebind-foreign-");
    let (mut lease, source) = admit(directory.path()).into_parts();
    let (foreign_lease, foreign_source) = admit(foreign.path()).into_parts();
    assert_eq!(source.authority(), foreign_source.authority());
    drop(foreign_source);
    let state = source.verified_state().version();
    let core = source.into_core();

    let misbound = acquire_cold_checked(
        &mut lease,
        &core,
        limits(),
        LogPackDecodeLimits::UNBOUNDED,
        DurableAdmissionLimitsV1::UNBOUNDED,
    )
    .expect("acquire a cold-checked source for the unchanged catalog");
    assert!(matches!(
        SemanticallyAdmittedDurableWriterV1::from_parts(foreign_lease, misbound),
        Err(DurableTransactionPreparationError::AuthorityMismatch)
    ));

    let acquired = acquire_cold_checked(
        &mut lease,
        &core,
        limits(),
        LogPackDecodeLimits::UNBOUNDED,
        DurableAdmissionLimitsV1::UNBOUNDED,
    )
    .expect("reacquire a cold-checked source for the unchanged catalog");
    drop(core);
    let published = SemanticallyAdmittedDurableWriterV1::from_parts(lease, acquired)
        .expect("the acquired source is bound to its own lease")
        .publish_generation_checkpoint_v1(
            limits(),
            LogPackDecodeLimits::UNBOUNDED,
            DurableAdmissionLimitsV1::UNBOUNDED,
        )
        .expect("publish a fresh generation from the rebound writer");
    assert_eq!(published.verified_state().version(), state);
    assert_eq!(published.authority(), &expected);
}
