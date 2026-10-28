use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::path::Path;
use std::rc::Rc;

use proptest::prelude::*;
use proptest::test_runner::TestRunner;

use super::checkpoint_wal::{SyntheticCheckpointSource, fixture, semantic_fixture};
use super::durable_publication::{
    FORMAL_PUBLICATION_PHASE_ORDER, directory_snapshot, install_checkpoint_image,
};
use super::durable_recovery::{checkpoint_image_from_source, limits};
use super::incremental_reachability_properties::{
    BudgetAxis, DOMAINS, Domain, STEP_AXES, TrialRefusal,
};
use super::support::{PersistenceTrialDir, TestSemanticProfile, trial_scratch};
use super::transaction_sealing::data_batch;
use super::trusted_successor_properties::{
    BindRefusal, CeilingAxis, PolicyAxis, SIDECAR_STATES, SidecarState, StateCeiling,
    expected_refusal, policy_axes, resolve_sidecar,
};
use crate::rspace::data_index::RSpaceDataIndex;
use crate::rspace::internal::Datum;
use crate::rspace::persistence::closure::ledger_trial::{
    LedgerTrialAxisV1, LedgerTrialLimitsV1, LedgerTrialRefusalV1, LedgerTrialUsageV1, RootDeltaV1,
    run_domain_trials, run_ledger_trial,
};
use crate::rspace::persistence::closure::{
    ClosureDomainV1, ClosureInventoryV1, StructuralClosureLedgerV1,
};
use crate::rspace::persistence::durable::{
    AdmittedCheckpointSourceV1, CommittedObjectIndexV1, DurableAdmissionErrorV1,
    DurableAdmissionLimitKindV1, DurableAdmissionLimitsV1, DurableAuthorityCoreV1,
    DurableDirectoryV1, DurableIndexedObjectStoreV1, DurableIoError, DurableIoFailpointV1,
    DurableObjectReadErrorV1, DurableStartupErrorV1, DurableTransactionPreparationError,
    DurableWriterLeaseV1, GenerationCheckpointPublicationErrorV1,
    GenerationCheckpointPublicationHookPointV1, GenerationCheckpointPublicationOutcomeV1,
    IncrementalPublicationErrorV1, ObjectIndexFileViewV1, PreparedTrustedSuccessorV1,
    SealedDurableTransactionV1, SemanticallyAdmittedDurableWriterV1, SuccessorBindingV1,
    TrustedCeilingAxisV1, TrustedStateCeilingsV1, TrustedSuccessorPolicyV1, TrustedTrialWorkV1,
    acquire_cold_checked, admit_physical_source, arm_durable_io_failpoint,
    arm_generation_checkpoint_publication_hook, cold_admissions, committed_index_constructions,
    index_file_name, pack_file_name, publish_incremental_from_lease_v1,
    publish_trusted_incremental_v1, recover_physical_from_lease_v1, reset_cold_admissions,
    reset_committed_index_constructions, start_generation_checkpoint_publication_trace,
    take_generation_checkpoint_publication_trace,
};
use crate::rspace::persistence::retention::v2_format::{RetentionFormat, RetentionReader};
use crate::rspace::persistence::semantic::CurrentShapeSummaryV1;
use crate::rspace::persistence::transaction::{SemanticValidityV1, ValidityOnlyV1};
use crate::rspace::persistence::{
    CATALOG_SLOT_LEN, CanonicalEncode, CatalogAuthorityTokenV1, CatalogSlotV1,
    DagTransactionLimits, GenerationLeaseKeyV1, GenerationLeaseV1, INDEX_HEADER_LEN_V1,
    INDEX_RECORD_LEN_V1, ImmutableObjectStore, IndexFormatError, IndexGenerationV1,
    LogPackDecodeLimits, LogPackSequence, MutationBatchViewV1, ObjectDigest, ObjectIndexRecordV1,
    ObjectRefV1, OpenDagTransaction, PackId, RSpaceMutationBatchV1, SealedDagTransaction,
    SealedObject, SemanticProfile, SemanticProfileId, StateVersionV1, StructuralValidationLimits,
    VerifiedDagState,
};

#[derive(Clone, Copy)]
pub(super) struct Scenario {
    pub(super) writer: RetentionFormat,
    pub(super) source: fn() -> SyntheticCheckpointSource,
    pub(super) foreign: fn() -> SyntheticCheckpointSource,
}

pub(super) const SCENARIOS: [Scenario; 4] = [
    Scenario {
        writer: RetentionFormat::V1,
        source: fixture,
        foreign: semantic_fixture,
    },
    Scenario {
        writer: RetentionFormat::V1,
        source: semantic_fixture,
        foreign: fixture,
    },
    Scenario {
        writer: RetentionFormat::V2,
        source: fixture,
        foreign: semantic_fixture,
    },
    Scenario {
        writer: RetentionFormat::V2,
        source: semantic_fixture,
        foreign: fixture,
    },
];

fn histories() -> impl Strategy<Value = Vec<(u8, u8)>> {
    prop::collection::vec((0u8..4, 0u8..5), 1..12)
}

fn production_runner(cases: u32) -> TestRunner {
    TestRunner::new(ProptestConfig {
        cases,
        source_file: Some(file!()),
        ..ProptestConfig::default()
    })
}

#[track_caller]
fn admit_writer(
    root: &Path,
    writer: RetentionFormat,
) -> SemanticallyAdmittedDurableWriterV1<TestSemanticProfile> {
    DurableDirectoryV1::open(root)
        .expect("open the durable trial directory")
        .with_retention_reader_for_test(RetentionReader::Dual)
        .with_retention_writer_for_test(writer)
        .recover_and_admit_v1::<TestSemanticProfile>(
            limits(),
            LogPackDecodeLimits::UNBOUNDED,
            DurableAdmissionLimitsV1::UNBOUNDED,
        )
        .expect("cold admission of the trial directory")
}

#[track_caller]
fn admit_parts(
    root: &Path,
    writer: RetentionFormat,
) -> (DurableWriterLeaseV1, AdmittedCheckpointSourceV1<TestSemanticProfile>) {
    admit_writer(root, writer).into_parts()
}

fn count(length: usize) -> u64 { u64::try_from(length).expect("a test collection length fits u64") }

fn step_mutation(
    state: StateVersionV1,
    counts: &[u8; 4],
    (identity, after): (u8, u8),
) -> Option<Vec<u8>> {
    match counts[usize::from(identity)] {
        before if before == after => None,
        before => {
            let channel = "inventory-channel".to_owned();
            let datum = Datum::create(&channel, format!("datum-{identity}"), false);
            let path = RSpaceDataIndex::datum_path(&channel, &datum);
            let mut batch = RSpaceMutationBatchV1::new(state)
                .expect("open a mutation batch at the admitted state");
            batch
                .record_data(&path, u64::from(before), u64::from(after))
                .expect("record the datum count transition");
            Some(
                batch
                    .plan()
                    .expect("plan the mutation batch")
                    .try_canonical_bytes()
                    .expect("encode the mutation batch"),
            )
        }
    }
}

struct PublicationTrace {
    active: bool,
}

impl PublicationTrace {
    fn start() -> Self {
        start_generation_checkpoint_publication_trace();
        Self { active: true }
    }

    fn finish(mut self) -> Vec<GenerationCheckpointPublicationHookPointV1> {
        self.active = false;
        take_generation_checkpoint_publication_trace()
    }
}

impl Drop for PublicationTrace {
    fn drop(&mut self) {
        if self.active {
            drop(take_generation_checkpoint_publication_trace());
        }
    }
}

pub(super) fn traced<T>(
    run: impl FnOnce() -> T,
) -> (T, Vec<GenerationCheckpointPublicationHookPointV1>) {
    let trace = PublicationTrace::start();
    let result = run();
    (result, trace.finish())
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct PublicationEffects {
    pub(super) trace: Vec<GenerationCheckpointPublicationHookPointV1>,
    pub(super) cold_admissions: u64,
    pub(super) index_constructions: u64,
}

impl PublicationEffects {
    pub(super) fn trusted() -> Self {
        Self {
            trace: FORMAL_PUBLICATION_PHASE_ORDER[2..=14].to_vec(),
            cold_admissions: 0,
            index_constructions: 0,
        }
    }

    fn cold() -> Self {
        Self {
            trace: FORMAL_PUBLICATION_PHASE_ORDER[2..].to_vec(),
            cold_admissions: 1,
            index_constructions: 1,
        }
    }
}

pub(super) fn observed_effects<T>(run: impl FnOnce() -> T) -> (T, PublicationEffects) {
    reset_cold_admissions();
    reset_committed_index_constructions();
    let (result, trace) = traced(run);
    (result, PublicationEffects {
        trace,
        cold_admissions: cold_admissions(),
        index_constructions: committed_index_constructions(),
    })
}

fn trusted_published(
    (published, effects): (
        Result<DurableAuthorityCoreV1<TestSemanticProfile>, IncrementalPublicationErrorV1>,
        PublicationEffects,
    ),
) -> Result<DurableAuthorityCoreV1<TestSemanticProfile>, TestCaseError> {
    let core = published.map_err(|error| {
        TestCaseError::fail(format!("the trusted publication failed: {error:?}"))
    })?;
    prop_assert_eq!(effects, PublicationEffects::trusted());
    Ok(core)
}

pub(super) fn preparation_refusal<T>(
    (published, trace): (
        Result<T, IncrementalPublicationErrorV1>,
        Vec<GenerationCheckpointPublicationHookPointV1>,
    ),
) -> Result<PreflightOutcome, TestCaseError> {
    prop_assert!(trace.is_empty(), "a preparation refusal ran publication phases {trace:?}");
    let error = published
        .err()
        .ok_or_else(|| TestCaseError::fail("the publication accepted a refused preparation"))?;
    prop_assert_eq!(
        error.publication_outcome(),
        GenerationCheckpointPublicationOutcomeV1::DefinitelyNotPublished
    );
    match error {
        IncrementalPublicationErrorV1::Preparation(refusal) => {
            Ok(preflight_outcome::<()>(Err(refusal)))
        }
        error => Err(TestCaseError::fail(format!(
            "the publication failed outside preparation: {error:?}"
        ))),
    }
}

struct PastState {
    version: StateVersionV1,
    validity: SemanticValidityV1,
    mutation: Vec<u8>,
}

struct MetadataDelta {
    added: [ObjectRefV1; 1],
    removed: Option<ObjectRefV1>,
}

struct MeasuredSuccessor {
    usage: [LedgerTrialUsageV1; 3],
    ledger: StructuralClosureLedgerV1,
}

#[derive(Debug, PartialEq, Eq)]
struct DomainObservation {
    objects: Vec<(ObjectRefV1, u64)>,
    roots: Vec<(ObjectDigest, u64)>,
    inventory: ClosureInventoryV1,
}

#[derive(Debug, PartialEq, Eq)]
struct UnchangedWitness {
    files: BTreeMap<String, Vec<u8>>,
    authority: CatalogAuthorityTokenV1,
    ledger: [DomainObservation; 3],
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct SuccessorObservation {
    version: StateVersionV1,
    validity: ObjectRefV1,
    retention: Vec<u8>,
    shape: CurrentShapeSummaryV1,
    retained_states: u64,
    maxima: ClosureInventoryV1,
    ledger: [DomainObservation; 3],
}

#[derive(Debug, PartialEq, Eq)]
struct SuccessorCeilings {
    maxima: ClosureInventoryV1,
    retained_states: u64,
    logical_nodes: u64,
    endpoints: u64,
    logical_path_bytes: u64,
    payload_bytes: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum BindOutcome {
    Prepared,
    AuthorityMismatch,
    NotWritable(RetentionFormat),
    Trial(ClosureDomainV1, LedgerTrialAxisV1),
    Ceiling(TrustedCeilingAxisV1, u64, u64),
    Other(String),
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum PreflightOutcome {
    Accepted,
    AuthorityMismatch,
    ModeMismatch,
    TrustedPolicyMismatch,
    AdmissionPolicyMismatch,
    Other(String),
}

struct SealedOverlay<'a> {
    sealed: &'a [SealedObject],
    durable: &'a DurableIndexedObjectStoreV1,
}

impl<'a> ImmutableObjectStore for SealedOverlay<'a> {
    type Error = DurableObjectReadErrorV1;
    type Object = Cow<'a, [u8]>;

    fn get_bounded(
        &self,
        digest: &ObjectDigest,
        max_encoded_len: u64,
    ) -> Result<Option<Self::Object>, Self::Error> {
        match self
            .sealed
            .binary_search_by_key(digest, |object| object.reference().digest())
        {
            Ok(position) => Ok(Some(Cow::Borrowed(self.sealed[position].canonical_bytes()))),
            Err(_) => Ok(self
                .durable
                .get_bounded(digest, max_encoded_len)?
                .map(|bytes| Cow::Owned(bytes.into_vec()))),
        }
    }
}

struct ProductionHistory {
    writer: RetentionFormat,
    directory: PersistenceTrialDir,
    lease: DurableWriterLeaseV1,
    source: AdmittedCheckpointSourceV1<TestSemanticProfile>,
    counts: [u8; 4],
    past: Vec<PastState>,
}

impl ProductionHistory {
    fn open(prefix: &str, writer: RetentionFormat, source: SyntheticCheckpointSource) -> Self {
        let directory = trial_scratch(prefix);
        install_checkpoint_image(directory.path(), &checkpoint_image_from_source(source));
        let (lease, source) = admit_parts(directory.path(), writer);
        Self::from_parts(writer, directory, lease, source)
    }

    fn from_parts(
        writer: RetentionFormat,
        directory: PersistenceTrialDir,
        lease: DurableWriterLeaseV1,
        source: AdmittedCheckpointSourceV1<TestSemanticProfile>,
    ) -> Self {
        Self {
            writer,
            directory,
            lease,
            source,
            counts: [0; 4],
            past: Vec::new(),
        }
    }

    fn state(&self) -> StateVersionV1 { self.source.authority().slot().state() }

    fn sequence(&self) -> LogPackSequence {
        self.source
            .authority()
            .slot()
            .commit_sequence()
            .successor()
            .expect("the admitted commit sequence has a successor")
    }

    fn mutation(&self, operation: (u8, u8)) -> Option<Vec<u8>> {
        step_mutation(self.state(), &self.counts, operation)
    }

    fn seal<'mutation>(
        &self,
        mutation: &'mutation [u8],
    ) -> SealedDagTransaction<'mutation, TestSemanticProfile> {
        OpenDagTransaction::new(
            self.source.core().verified_state(),
            MutationBatchViewV1::parse(mutation).expect("parse the step mutation"),
            DagTransactionLimits::UNBOUNDED,
        )
        .and_then(OpenDagTransaction::seal)
        .expect("seal the step mutation on the authority core")
    }

    fn bind<'mutation>(
        &self,
        mutation: &'mutation [u8],
        policy: TrustedSuccessorPolicyV1,
    ) -> Result<
        SealedDurableTransactionV1<'mutation, TestSemanticProfile>,
        DurableTransactionPreparationError,
    > {
        self.source
            .core()
            .bind_trusted_transaction(&self.lease, self.seal(mutation), policy)
    }

    fn bind_cold<'mutation>(
        &self,
        mutation: &'mutation [u8],
        limits: DurableAdmissionLimitsV1,
    ) -> SealedDurableTransactionV1<'mutation, TestSemanticProfile> {
        self.source
            .seal_transaction(
                &self.lease,
                MutationBatchViewV1::parse(mutation).expect("parse the step mutation"),
                DagTransactionLimits::UNBOUNDED,
                limits,
            )
            .expect("seal and bind the cold-checked successor")
    }

    fn bind_past<'a>(
        &'a self,
        past: &'a PastState,
    ) -> Result<
        SealedDurableTransactionV1<'a, TestSemanticProfile>,
        DurableTransactionPreparationError,
    > {
        let core = self.source.core();
        let state = VerifiedDagState::<_, TestSemanticProfile, ValidityOnlyV1>::from_validity(
            core.verified_state().object_store(),
            past.version,
            past.validity,
        )
        .expect("a retained state's validity token names its manifest");
        let sealed = OpenDagTransaction::new(
            state,
            MutationBatchViewV1::parse(&past.mutation).expect("parse a retained step mutation"),
            DagTransactionLimits::UNBOUNDED,
        )
        .and_then(OpenDagTransaction::seal)
        .expect("reseal a retained state's mutation on the current store");
        core.bind_trusted_transaction(&self.lease, sealed, TrustedSuccessorPolicyV1::UNBOUNDED)
    }

    fn adopt(
        &mut self,
        mutation: &[u8],
        (identity, after): (u8, u8),
        successor: AdmittedCheckpointSourceV1<TestSemanticProfile>,
    ) -> AdmittedCheckpointSourceV1<TestSemanticProfile> {
        let predecessor = std::mem::replace(&mut self.source, successor);
        self.past.push(PastState {
            version: predecessor.authority().slot().state(),
            validity: predecessor.core().validity(),
            mutation: mutation.to_vec(),
        });
        self.counts[usize::from(identity)] = after;
        predecessor
    }

    fn publish_observed(
        &mut self,
        mutation: &[u8],
        operation: (u8, u8),
    ) -> (AdmittedCheckpointSourceV1<TestSemanticProfile>, PublicationEffects) {
        let sealed = self.bind_cold(mutation, DurableAdmissionLimitsV1::UNBOUNDED);
        let prepared = self
            .source
            .prepare_transaction(
                &self.lease,
                &sealed,
                self.sequence(),
                LogPackDecodeLimits::UNBOUNDED,
            )
            .expect("prepare the cold-checked successor");
        let (successor, effects) = observed_effects(|| {
            publish_incremental_from_lease_v1(
                &mut self.lease,
                &self.source,
                &prepared,
                limits(),
                LogPackDecodeLimits::UNBOUNDED,
                DurableAdmissionLimitsV1::UNBOUNDED,
            )
        });
        let successor = successor.expect("publish the cold-checked successor");
        (self.adopt(mutation, operation, successor), effects)
    }

    fn publish(
        &mut self,
        mutation: &[u8],
        operation: (u8, u8),
    ) -> AdmittedCheckpointSourceV1<TestSemanticProfile> {
        self.publish_observed(mutation, operation).0
    }

    fn publish_trusted(
        &mut self,
        mutation: &[u8],
        operation: (u8, u8),
        policy: TrustedSuccessorPolicyV1,
    ) -> Result<DurableAuthorityCoreV1<TestSemanticProfile>, TestCaseError> {
        let bound = self.bind(mutation, policy).map_err(refused)?;
        let prepared = self
            .source
            .prepare_transaction(
                &self.lease,
                &bound,
                self.sequence(),
                LogPackDecodeLimits::UNBOUNDED,
            )
            .expect("prepare the trusted-bound successor");
        let published = trusted_published(observed_effects(|| {
            publish_trusted_incremental_v1(
                &mut self.lease,
                self.source.core(),
                &prepared,
                policy,
                |_| {},
            )
        }))?;
        self.adopt_trusted(mutation, operation, published)
    }

    fn adopt_trusted(
        &mut self,
        mutation: &[u8],
        operation: (u8, u8),
        published: DurableAuthorityCoreV1<TestSemanticProfile>,
    ) -> Result<DurableAuthorityCoreV1<TestSemanticProfile>, TestCaseError> {
        let readmitted = acquire_cold_checked(
            &mut self.lease,
            &published,
            limits(),
            LogPackDecodeLimits::UNBOUNDED,
            DurableAdmissionLimitsV1::UNBOUNDED,
        )
        .expect("cold re-admission of the trusted successor");
        prop_assert_eq!(core_observation(readmitted.core()), core_observation(&published));
        self.adopt(mutation, operation, readmitted);
        Ok(published)
    }

    fn witness(&self) -> UnchangedWitness {
        UnchangedWitness {
            files: directory_snapshot(self.directory.path()),
            authority: self.source.authority().clone(),
            ledger: ledger_observation(self.source.core().retained_closure()),
        }
    }

    fn overlay<'a>(
        &'a self,
        bound: &'a SealedDurableTransactionV1<'_, TestSemanticProfile>,
    ) -> SealedOverlay<'a> {
        SealedOverlay {
            sealed: bound.objects(),
            durable: self.source.core().verified_state().object_store(),
        }
    }

    fn metadata_delta(
        &self,
        bound: &SealedDurableTransactionV1<'_, TestSemanticProfile>,
    ) -> Option<MetadataDelta> {
        metadata_delta(self.source.core(), self.writer, bound)
    }

    fn measure(
        &self,
        bound: &SealedDurableTransactionV1<'_, TestSemanticProfile>,
    ) -> MeasuredSuccessor {
        measure_successor(self.source.core(), self.writer, bound)
    }
}

fn metadata_delta<P: SemanticProfile>(
    core: &DurableAuthorityCoreV1<P>,
    writer: RetentionFormat,
    bound: &SealedDurableTransactionV1<'_, P>,
) -> Option<MetadataDelta> {
    match (writer, core.retention_format()) {
        (RetentionFormat::V1, _) => None,
        (RetentionFormat::V2, predecessor) => Some(MetadataDelta {
            added: [bound
                .retention_manifest()
                .expect("a bound V2 successor seals its retention anchor")],
            removed: match predecessor {
                RetentionFormat::V1 => None,
                RetentionFormat::V2 => Some(core.authority().slot().pack().retention_manifest()),
            },
        }),
    }
}

fn measure_successor<P: SemanticProfile>(
    core: &DurableAuthorityCoreV1<P>,
    writer: RetentionFormat,
    bound: &SealedDurableTransactionV1<'_, P>,
) -> MeasuredSuccessor {
    let store = SealedOverlay {
        sealed: bound.objects(),
        durable: core.verified_state().object_store(),
    };
    let receipt = bound.receipt();
    let target = [receipt.target().manifest()];
    let base = [receipt.base().manifest()];
    let trials = run_domain_trials(
        &store,
        core.retained_closure(),
        RootDeltaV1 {
            added: &target,
            removed: &base,
        },
        RootDeltaV1 {
            added: &target,
            removed: &[],
        },
        [LedgerTrialLimitsV1::UNBOUNDED; 2],
    )
    .expect("the unbounded domain trials of a bound successor succeed");
    let [current, retained] = trials.usage;
    match metadata_delta(core, writer, bound) {
        None => MeasuredSuccessor {
            usage: [current, retained, LedgerTrialUsageV1::default()],
            ledger: trials.successor,
        },
        Some(delta) => {
            let metadata = run_ledger_trial(
                &store,
                &trials.successor,
                ClosureDomainV1::Metadata,
                &delta.added,
                delta.removed.as_slice(),
                LedgerTrialLimitsV1::UNBOUNDED,
            )
            .expect("the unbounded metadata trial of a bound successor succeeds");
            MeasuredSuccessor {
                usage: [current, retained, metadata.usage],
                ledger: metadata.successor,
            }
        }
    }
}

struct TrustedChain {
    directory: PersistenceTrialDir,
    lease: DurableWriterLeaseV1,
    core: DurableAuthorityCoreV1<TestSemanticProfile>,
    counts: [u8; 4],
}

impl TrustedChain {
    fn open(prefix: &str, writer: RetentionFormat, source: SyntheticCheckpointSource) -> Self {
        let directory = trial_scratch(prefix);
        install_checkpoint_image(directory.path(), &checkpoint_image_from_source(source));
        let (lease, source) = admit_parts(directory.path(), writer);
        Self::from_parts(directory, lease, source)
    }

    fn from_parts(
        directory: PersistenceTrialDir,
        lease: DurableWriterLeaseV1,
        source: AdmittedCheckpointSourceV1<TestSemanticProfile>,
    ) -> Self {
        Self {
            directory,
            lease,
            core: source.into_core(),
            counts: [0; 4],
        }
    }

    fn sequence(&self) -> LogPackSequence {
        self.core
            .authority()
            .slot()
            .commit_sequence()
            .successor()
            .expect("the published commit sequence has a successor")
    }

    fn mutation(&self, operation: (u8, u8)) -> Option<Vec<u8>> {
        step_mutation(self.core.authority().slot().state(), &self.counts, operation)
    }

    fn seal<'mutation>(
        &self,
        mutation: &'mutation [u8],
    ) -> SealedDagTransaction<'mutation, TestSemanticProfile> {
        OpenDagTransaction::new(
            self.core.verified_state(),
            MutationBatchViewV1::parse(mutation).expect("parse the step mutation"),
            DagTransactionLimits::UNBOUNDED,
        )
        .and_then(OpenDagTransaction::seal)
        .expect("seal the step mutation on the trusted core")
    }

    fn bind<'mutation>(
        &self,
        mutation: &'mutation [u8],
        policy: TrustedSuccessorPolicyV1,
    ) -> Result<
        SealedDurableTransactionV1<'mutation, TestSemanticProfile>,
        DurableTransactionPreparationError,
    > {
        self.core
            .bind_trusted_transaction(&self.lease, self.seal(mutation), policy)
    }

    fn publish(
        &mut self,
        mutation: &[u8],
        (identity, after): (u8, u8),
        policy: TrustedSuccessorPolicyV1,
    ) -> Result<PublicationEffects, TestCaseError> {
        let bound = self.bind(mutation, policy).map_err(refused)?;
        let prepared = self
            .core
            .prepare_transaction(
                &self.lease,
                &bound,
                self.sequence(),
                LogPackDecodeLimits::UNBOUNDED,
            )
            .expect("prepare the trusted-bound successor");
        let (published, effects) = observed_effects(|| {
            publish_trusted_incremental_v1(&mut self.lease, &self.core, &prepared, policy, |_| {})
        });
        self.core = published.map_err(|error| {
            TestCaseError::fail(format!("the trusted publication failed: {error:?}"))
        })?;
        self.counts[usize::from(identity)] = after;
        Ok(effects)
    }

    fn recover(&mut self, published: Option<(u8, u8)>) {
        let recovered = recover_physical_from_lease_v1(
            &mut self.lease,
            limits(),
            LogPackDecodeLimits::UNBOUNDED,
        )
        .expect("physical recovery after a publication fault");
        self.core = admit_physical_source::<TestSemanticProfile>(
            &self.lease,
            recovered,
            DurableAdmissionLimitsV1::UNBOUNDED,
        )
        .expect("cold admission after a publication fault")
        .into_core();
        if let Some((identity, after)) = published {
            self.counts[usize::from(identity)] = after;
        }
    }
}

const ADMISSION_AXES: [DurableAdmissionLimitKindV1; 18] = [
    DurableAdmissionLimitKindV1::PerStateDistinctObjects,
    DurableAdmissionLimitKindV1::PerStateCanonicalBytes,
    DurableAdmissionLimitKindV1::PerStateReferences,
    DurableAdmissionLimitKindV1::PerStateWorklist,
    DurableAdmissionLimitKindV1::RetainedStates,
    DurableAdmissionLimitKindV1::UnionObjects,
    DurableAdmissionLimitKindV1::UnionCanonicalBytes,
    DurableAdmissionLimitKindV1::AggregateRootVisits,
    DurableAdmissionLimitKindV1::AggregateReferences,
    DurableAdmissionLimitKindV1::ObjectReads,
    DurableAdmissionLimitKindV1::ObjectReadBytes,
    DurableAdmissionLimitKindV1::LogicalNodes,
    DurableAdmissionLimitKindV1::Endpoints,
    DurableAdmissionLimitKindV1::LogicalPathBytes,
    DurableAdmissionLimitKindV1::PayloadBytes,
    DurableAdmissionLimitKindV1::JoinLookups,
    DurableAdmissionLimitKindV1::JoinLookupSteps,
    DurableAdmissionLimitKindV1::SemanticWorklist,
];

fn admission_slot(
    limits: &mut DurableAdmissionLimitsV1,
    axis: DurableAdmissionLimitKindV1,
) -> &mut u64 {
    match axis {
        DurableAdmissionLimitKindV1::PerStateDistinctObjects => {
            &mut limits.per_state_structural.max_distinct_objects
        }
        DurableAdmissionLimitKindV1::PerStateCanonicalBytes => {
            &mut limits.per_state_structural.max_canonical_bytes
        }
        DurableAdmissionLimitKindV1::PerStateReferences => {
            &mut limits.per_state_structural.max_references
        }
        DurableAdmissionLimitKindV1::PerStateWorklist => {
            &mut limits.per_state_structural.max_worklist
        }
        DurableAdmissionLimitKindV1::RetainedStates => &mut limits.max_retained_states,
        DurableAdmissionLimitKindV1::UnionObjects => &mut limits.max_union_objects,
        DurableAdmissionLimitKindV1::UnionCanonicalBytes => &mut limits.max_union_canonical_bytes,
        DurableAdmissionLimitKindV1::AggregateRootVisits => &mut limits.max_aggregate_root_visits,
        DurableAdmissionLimitKindV1::AggregateReferences => &mut limits.max_aggregate_references,
        DurableAdmissionLimitKindV1::ObjectReads => &mut limits.max_object_reads,
        DurableAdmissionLimitKindV1::ObjectReadBytes => &mut limits.max_object_read_bytes,
        DurableAdmissionLimitKindV1::LogicalNodes => {
            &mut limits.current_semantics.max_logical_nodes
        }
        DurableAdmissionLimitKindV1::Endpoints => &mut limits.current_semantics.max_endpoints,
        DurableAdmissionLimitKindV1::LogicalPathBytes => {
            &mut limits.current_semantics.max_logical_path_bytes
        }
        DurableAdmissionLimitKindV1::PayloadBytes => {
            &mut limits.current_semantics.max_payload_bytes
        }
        DurableAdmissionLimitKindV1::JoinLookups => &mut limits.current_semantics.max_join_lookups,
        DurableAdmissionLimitKindV1::JoinLookupSteps => {
            &mut limits.current_semantics.max_join_lookup_steps
        }
        DurableAdmissionLimitKindV1::SemanticWorklist => &mut limits.current_semantics.max_worklist,
    }
}

fn admission_limits_with(
    mut limits: DurableAdmissionLimitsV1,
    axis: DurableAdmissionLimitKindV1,
    value: u64,
) -> DurableAdmissionLimitsV1 {
    *admission_slot(&mut limits, axis) = value;
    limits
}

fn preflight_refusal(
    source: &AdmittedCheckpointSourceV1<TestSemanticProfile>,
    limits: DurableAdmissionLimitsV1,
) -> Option<(DurableAdmissionLimitKindV1, u64, u64)> {
    source
        .preflight_checkpoint_admission(limits)
        .err()
        .map(|refusal| (refusal.resource, refusal.limit, refusal.attempted))
}

fn admission_demand(source: &AdmittedCheckpointSourceV1<TestSemanticProfile>) -> [u64; 18] {
    ADMISSION_AXES.map(|axis| {
        match preflight_refusal(
            source,
            admission_limits_with(DurableAdmissionLimitsV1::UNBOUNDED, axis, 0),
        ) {
            None => 0,
            Some((resource, 0, attempted)) if resource == axis => attempted,
            Some(refusal) => panic!("the zero {axis:?} probe refused {refusal:?}"),
        }
    })
}

fn demand_limits(demand: &[u64; 18]) -> DurableAdmissionLimitsV1 {
    ADMISSION_AXES
        .into_iter()
        .zip(demand)
        .fold(DurableAdmissionLimitsV1::UNBOUNDED, |limits, (axis, &value)| {
            admission_limits_with(limits, axis, value)
        })
}

fn positive_demand_axis(demand: &[u64; 18], rotation: usize) -> (DurableAdmissionLimitKindV1, u64) {
    let positive = ADMISSION_AXES
        .into_iter()
        .zip(demand.iter().copied())
        .filter(|&(_, value)| value > 0)
        .collect::<Vec<_>>();
    positive[rotation % positive.len()]
}

fn checkpointed_parts(
    prefix: &str,
    scenario: Scenario,
    sidecar: SidecarState,
) -> (
    PersistenceTrialDir,
    DurableWriterLeaseV1,
    AdmittedCheckpointSourceV1<TestSemanticProfile>,
    Option<GenerationLeaseV1>,
) {
    let directory = trial_scratch(prefix);
    install_checkpoint_image(directory.path(), &checkpoint_image_from_source((scenario.source)()));
    let writer = admit_writer(directory.path(), scenario.writer);
    let pin = match sidecar {
        SidecarState::Absent => None,
        SidecarState::Deferred | SidecarState::Reclaimable => Some(
            writer
                .generation_leases()
                .acquire(GenerationLeaseKeyV1::from_catalog(writer.catalog()))
                .expect("pin the generation the checkpoint supersedes"),
        ),
    };
    let (lease, source) = writer
        .publish_generation_checkpoint_v1(
            limits(),
            LogPackDecodeLimits::UNBOUNDED,
            DurableAdmissionLimitsV1::UNBOUNDED,
        )
        .expect("publish the fresh generation checkpoint")
        .into_parts();
    let pin = match sidecar {
        SidecarState::Deferred => pin,
        SidecarState::Absent | SidecarState::Reclaimable => {
            drop(pin);
            None
        }
    };
    (directory, lease, source, pin)
}

pub(super) struct TrustedPolicyEvidence {
    exact: TrustedSuccessorPolicyV1,
    current: ClosureInventoryV1,
    maxima: ClosureInventoryV1,
    target: StateVersionV1,
}

impl TrustedPolicyEvidence {
    pub(super) fn measure<P: SemanticProfile>(
        lease: &DurableWriterLeaseV1,
        core: &DurableAuthorityCoreV1<P>,
        sealed: SealedDagTransaction<'_, P>,
    ) -> Self {
        let bound = core
            .bind_trusted_transaction(lease, sealed, TrustedSuccessorPolicyV1::UNBOUNDED)
            .expect("the unbounded trusted bind of an admissible successor succeeds");
        let measured = measure_successor(core, lease.directory().retention_writer(), &bound);
        let ceilings = SuccessorCeilings::trusted(&bound);
        Self {
            exact: exact_policy(&measured, &ceilings),
            current: measured.ledger.inventory(ClosureDomainV1::Current),
            maxima: ceilings.maxima,
            target: bound.receipt().target(),
        }
    }

    pub(super) const fn exact(&self) -> TrustedSuccessorPolicyV1 { self.exact }

    pub(super) const fn target(&self) -> StateVersionV1 { self.target }

    pub(super) fn lowered(&self) -> Vec<(PolicyAxis, TrustedSuccessorPolicyV1, BindOutcome)> {
        policy_axes()
            .into_iter()
            .filter_map(|axis| {
                let value = policy_value(self.exact, axis);
                let lower = value.checked_sub(1)?;
                Some((
                    axis,
                    policy_with(self.exact, axis, lower),
                    refined(
                        expected_refusal(
                            axis,
                            (self.current.distinct_objects, self.current.canonical_bytes),
                            (self.maxima.distinct_objects, self.maxima.canonical_bytes),
                        ),
                        lower,
                        value,
                    ),
                ))
            })
            .collect()
    }
}

pub(super) const RECLAMATION_INTENT: &str = "reclaim.v1";

#[derive(Debug, PartialEq, Eq)]
struct ObservedResolution {
    intent_kept: bool,
    removed: Vec<String>,
}

fn observed_resolution(
    before: &BTreeMap<String, Vec<u8>>,
    after: &BTreeMap<String, Vec<u8>>,
) -> ObservedResolution {
    ObservedResolution {
        intent_kept: after.contains_key(RECLAMATION_INTENT),
        removed: before
            .keys()
            .filter(|name| !after.contains_key(*name))
            .cloned()
            .collect(),
    }
}

fn expected_resolution(state: SidecarState) -> ObservedResolution {
    let model = resolve_sidecar(state);
    ObservedResolution {
        intent_kept: model.intent_kept,
        removed: match model.superseded_files_removed {
            false => Vec::new(),
            true => {
                let mut names = vec![
                    index_file_name(IndexGenerationV1::FIRST),
                    pack_file_name(PackId::GENESIS),
                    RECLAMATION_INTENT.to_owned(),
                ];
                names.sort_unstable();
                names
            }
        },
    }
}

fn durable_index_records(root: &Path, slot: CatalogSlotV1) -> Vec<ObjectIndexRecordV1> {
    let generation = IndexGenerationV1::new(slot.pack().index_generation())
        .expect("the catalog appoints a nonzero index generation");
    let bytes = fs::read(root.join(index_file_name(generation))).expect("read the appointed index");
    ObjectIndexFileViewV1::parse_durable_prefix(&bytes, slot.pack().durable_index_tail())
        .expect("parse the appointed index prefix")
        .records()
        .collect::<Result<Vec<_>, _>>()
        .expect("decode the appointed index records")
}

fn index_tail(records: usize) -> u64 {
    count(records)
        .checked_mul(INDEX_RECORD_LEN_V1)
        .and_then(|bytes| bytes.checked_add(INDEX_HEADER_LEN_V1))
        .expect("a test index tail fits u64")
}

fn rebuilt_index(records: &[ObjectIndexRecordV1]) -> Result<u64, IndexFormatError> {
    let mut index = CommittedObjectIndexV1::new();
    for &record in records {
        index.insert(record)?;
    }
    Ok(index.record_count())
}

fn widened(record: ObjectIndexRecordV1) -> ObjectIndexRecordV1 {
    let reference = record.reference();
    ObjectIndexRecordV1::new(
        ObjectRefV1::new(
            reference.digest(),
            reference
                .encoded_len()
                .checked_add(1)
                .expect("a test object length has a successor"),
        )
        .expect("widen an indexed object reference"),
        record.location(),
    )
    .expect("index the widened reference at the same location")
}

fn index_plan(
    core: &DurableAuthorityCoreV1<TestSemanticProfile>,
    disk: &[ObjectIndexRecordV1],
    plan: &[ObjectIndexRecordV1],
) -> Result<Result<u64, IndexFormatError>, TestCaseError> {
    let combined = [disk, plan].concat();
    let trusted = core
        .trusted_successor_index(plan.iter().copied().map(Ok), index_tail(combined.len()))
        .map(|index| index.record_count());
    prop_assert_eq!(&trusted, &rebuilt_index(&combined));
    Ok(trusted)
}

fn domain_observation(
    ledger: &StructuralClosureLedgerV1,
    domain: ClosureDomainV1,
) -> DomainObservation {
    DomainObservation {
        objects: ledger
            .counted_objects()
            .into_iter()
            .filter_map(|(reference, _)| match ledger.count(domain, reference.digest()) {
                0 => None,
                count => Some((reference, count)),
            })
            .collect(),
        roots: ledger
            .root_multiplicities()
            .into_iter()
            .filter_map(|(digest, _)| match ledger.root_count(domain, digest) {
                0 => None,
                count => Some((digest, count)),
            })
            .collect(),
        inventory: ledger.inventory(domain),
    }
}

fn ledger_observation(ledger: &StructuralClosureLedgerV1) -> [DomainObservation; 3] {
    ClosureDomainV1::ALL.map(|domain| domain_observation(ledger, domain))
}

fn trusted_successor<'a, P: SemanticProfile>(
    bound: &'a SealedDurableTransactionV1<'_, P>,
) -> &'a PreparedTrustedSuccessorV1 {
    match bound.binding() {
        SuccessorBindingV1::Trusted { successor, .. } => successor,
        SuccessorBindingV1::ColdChecked { limits } => {
            panic!("a trusted bind carries a cold-checked binding with {limits:?}")
        }
    }
}

pub(super) fn trusted_observation<P: SemanticProfile>(
    bound: &SealedDurableTransactionV1<'_, P>,
) -> SuccessorObservation {
    let successor = trusted_successor(bound);
    SuccessorObservation {
        version: bound.receipt().target(),
        validity: successor.validity().manifest(),
        retention: successor.retention_manifest_bytes().to_vec(),
        shape: successor.shape().clone(),
        retained_states: successor.retained_states(),
        maxima: successor.retained_state_maxima(),
        ledger: ledger_observation(successor.retained_closure()),
    }
}

pub(super) fn core_observation<P: SemanticProfile>(
    core: &DurableAuthorityCoreV1<P>,
) -> SuccessorObservation {
    SuccessorObservation {
        version: core.authority().slot().state(),
        validity: core.validity().manifest(),
        retention: core.retention_manifest_bytes().to_vec(),
        shape: core.shape().clone(),
        retained_states: core.retained_states(),
        maxima: core.retained_state_maxima(),
        ledger: ledger_observation(core.retained_closure()),
    }
}

fn sealed_bytes(objects: &[SealedObject]) -> Vec<(ObjectRefV1, Vec<u8>)> {
    objects
        .iter()
        .map(|object| (object.reference(), object.canonical_bytes().to_vec()))
        .collect()
}

impl SuccessorCeilings {
    fn of(maxima: ClosureInventoryV1, retained_states: u64, shape: &CurrentShapeSummaryV1) -> Self {
        Self {
            maxima,
            retained_states,
            logical_nodes: shape.logical_nodes(),
            endpoints: shape.endpoints(),
            logical_path_bytes: shape
                .maximum_logical_path_bytes()
                .expect("an admitted shape summary's longest path fits u64"),
            payload_bytes: shape.source_payload_bytes(),
        }
    }

    fn trusted<P: SemanticProfile>(bound: &SealedDurableTransactionV1<'_, P>) -> Self {
        let successor = trusted_successor(bound);
        Self::of(successor.retained_state_maxima(), successor.retained_states(), successor.shape())
    }

    fn cold(source: &AdmittedCheckpointSourceV1<TestSemanticProfile>) -> Self {
        let core = source.core();
        Self::of(core.retained_state_maxima(), core.retained_states(), core.shape())
    }
}

const fn measured_work(usage: LedgerTrialUsageV1) -> TrustedTrialWorkV1 {
    TrustedTrialWorkV1 {
        count: usage.peak_count,
        union_objects: usage.peak_objects,
        union_bytes: usage.peak_bytes,
        scratch: usage.peak_scratch,
        references: usage.references,
        expansions: usage.expansions,
        quoted_bytes: usage.quoted_bytes,
    }
}

fn exact_policy(
    measured: &MeasuredSuccessor,
    ceilings: &SuccessorCeilings,
) -> TrustedSuccessorPolicyV1 {
    let retained = measured.ledger.inventory(ClosureDomainV1::Retained);
    let metadata = measured.ledger.inventory(ClosureDomainV1::Metadata);
    TrustedSuccessorPolicyV1 {
        state: TrustedStateCeilingsV1 {
            max_state_objects: ceilings.maxima.distinct_objects,
            max_state_canonical_bytes: ceilings.maxima.canonical_bytes,
            max_retained_states: ceilings.retained_states,
            max_union_objects: retained.distinct_objects,
            max_union_canonical_bytes: retained.canonical_bytes,
            max_metadata_objects: metadata.distinct_objects,
            max_metadata_canonical_bytes: metadata.canonical_bytes,
            max_logical_nodes: ceilings.logical_nodes,
            max_endpoints: ceilings.endpoints,
            max_logical_path_bytes: ceilings.logical_path_bytes,
            max_payload_bytes: ceilings.payload_bytes,
        },
        work: measured.usage.map(measured_work),
    }
}

const fn production_domain(domain: Domain) -> ClosureDomainV1 {
    match domain {
        Domain::Current => ClosureDomainV1::Current,
        Domain::Retained => ClosureDomainV1::Retained,
        Domain::Metadata => ClosureDomainV1::Metadata,
    }
}

const fn production_axis(axis: BudgetAxis) -> LedgerTrialAxisV1 {
    match axis {
        BudgetAxis::Count => LedgerTrialAxisV1::Count,
        BudgetAxis::UnionObjects => LedgerTrialAxisV1::UnionObjects,
        BudgetAxis::UnionBytes => LedgerTrialAxisV1::UnionBytes,
        BudgetAxis::Scratch => LedgerTrialAxisV1::Scratch,
        BudgetAxis::References => LedgerTrialAxisV1::References,
        BudgetAxis::Expansions => LedgerTrialAxisV1::Expansions,
        BudgetAxis::QuotedBytes => LedgerTrialAxisV1::QuotedBytes,
        BudgetAxis::FinalObjects => LedgerTrialAxisV1::FinalObjects,
        BudgetAxis::FinalBytes => LedgerTrialAxisV1::FinalBytes,
    }
}

const fn production_ceiling(axis: CeilingAxis) -> TrustedCeilingAxisV1 {
    match axis {
        CeilingAxis::StateObjects => TrustedCeilingAxisV1::StateObjects,
        CeilingAxis::StateBytes => TrustedCeilingAxisV1::StateBytes,
        CeilingAxis::RetainedStates => TrustedCeilingAxisV1::RetainedStates,
        CeilingAxis::LogicalNodes => TrustedCeilingAxisV1::LogicalNodes,
        CeilingAxis::Endpoints => TrustedCeilingAxisV1::Endpoints,
        CeilingAxis::LogicalPathBytes => TrustedCeilingAxisV1::LogicalPathBytes,
        CeilingAxis::PayloadBytes => TrustedCeilingAxisV1::PayloadBytes,
    }
}

fn policy_slot(policy: &mut TrustedSuccessorPolicyV1, axis: PolicyAxis) -> &mut u64 {
    match axis {
        PolicyAxis::State(StateCeiling::StateObjects) => &mut policy.state.max_state_objects,
        PolicyAxis::State(StateCeiling::StateBytes) => &mut policy.state.max_state_canonical_bytes,
        PolicyAxis::State(StateCeiling::RetainedStates) => &mut policy.state.max_retained_states,
        PolicyAxis::State(StateCeiling::UnionObjects) => &mut policy.state.max_union_objects,
        PolicyAxis::State(StateCeiling::UnionBytes) => &mut policy.state.max_union_canonical_bytes,
        PolicyAxis::State(StateCeiling::MetadataObjects) => &mut policy.state.max_metadata_objects,
        PolicyAxis::State(StateCeiling::MetadataBytes) => {
            &mut policy.state.max_metadata_canonical_bytes
        }
        PolicyAxis::State(StateCeiling::LogicalNodes) => &mut policy.state.max_logical_nodes,
        PolicyAxis::State(StateCeiling::Endpoints) => &mut policy.state.max_endpoints,
        PolicyAxis::State(StateCeiling::LogicalPathBytes) => {
            &mut policy.state.max_logical_path_bytes
        }
        PolicyAxis::State(StateCeiling::PayloadBytes) => &mut policy.state.max_payload_bytes,
        PolicyAxis::Work(domain, axis) => {
            let work = &mut policy.work[match domain {
                Domain::Current => 0,
                Domain::Retained => 1,
                Domain::Metadata => 2,
            }];
            match axis {
                BudgetAxis::Count => &mut work.count,
                BudgetAxis::UnionObjects => &mut work.union_objects,
                BudgetAxis::UnionBytes => &mut work.union_bytes,
                BudgetAxis::Scratch => &mut work.scratch,
                BudgetAxis::References => &mut work.references,
                BudgetAxis::Expansions => &mut work.expansions,
                BudgetAxis::QuotedBytes => &mut work.quoted_bytes,
                BudgetAxis::FinalObjects | BudgetAxis::FinalBytes => {
                    panic!("the {axis:?} budget of {domain:?} is a state ceiling, not a work axis")
                }
            }
        }
    }
}

fn policy_value(mut policy: TrustedSuccessorPolicyV1, axis: PolicyAxis) -> u64 {
    *policy_slot(&mut policy, axis)
}

pub(super) fn policy_with(
    mut policy: TrustedSuccessorPolicyV1,
    axis: PolicyAxis,
    value: u64,
) -> TrustedSuccessorPolicyV1 {
    *policy_slot(&mut policy, axis) = value;
    policy
}

fn lowered(policy: TrustedSuccessorPolicyV1, axis: PolicyAxis) -> TrustedSuccessorPolicyV1 {
    policy_with(
        policy,
        axis,
        policy_value(policy, axis)
            .checked_sub(1)
            .expect("only a positive policy axis is lowered"),
    )
}

fn refined(refusal: BindRefusal, limit: u64, attempted: u64) -> BindOutcome {
    match refusal {
        BindRefusal::ForeignBase | BindRefusal::ProfileMismatch => BindOutcome::AuthorityMismatch,
        BindRefusal::TrialRefusedAt(domain, TrialRefusal::Budget(axis)) => {
            BindOutcome::Trial(production_domain(domain), production_axis(axis))
        }
        BindRefusal::TrialRefusedAt(
            domain,
            refusal @ (TrialRefusal::UnauthorizedRemoval |
            TrialRefusal::ZeroCountDecrement |
            TrialRefusal::InventoryUnderflow |
            TrialRefusal::PotentialExhausted),
        ) => panic!("a lowered policy axis never refuses {domain:?} with {refusal:?}"),
        BindRefusal::CeilingRefused(axis) => {
            BindOutcome::Ceiling(production_ceiling(axis), limit, attempted)
        }
    }
}

pub(super) fn bind_outcome<T>(
    result: Result<T, DurableTransactionPreparationError>,
) -> BindOutcome {
    match result {
        Ok(_) => BindOutcome::Prepared,
        Err(DurableTransactionPreparationError::AuthorityMismatch) => {
            BindOutcome::AuthorityMismatch
        }
        Err(DurableTransactionPreparationError::RetentionFormatNotWritable(format)) => {
            BindOutcome::NotWritable(format)
        }
        Err(DurableTransactionPreparationError::TrustedTrial { domain, refusal }) => match *refusal
        {
            LedgerTrialRefusalV1::Budget(axis) => BindOutcome::Trial(domain, axis),
            refusal => BindOutcome::Other(format!("{domain:?} trial refusal {refusal:?}")),
        },
        Err(DurableTransactionPreparationError::TrustedCeiling {
            axis,
            limit,
            attempted,
        }) => BindOutcome::Ceiling(axis, limit, attempted),
        Err(error) => BindOutcome::Other(format!("{error:?}")),
    }
}

fn preflight_outcome<T>(result: Result<T, DurableTransactionPreparationError>) -> PreflightOutcome {
    match result {
        Ok(_) => PreflightOutcome::Accepted,
        Err(DurableTransactionPreparationError::AuthorityMismatch) => {
            PreflightOutcome::AuthorityMismatch
        }
        Err(DurableTransactionPreparationError::PublicationModeMismatch) => {
            PreflightOutcome::ModeMismatch
        }
        Err(DurableTransactionPreparationError::TrustedPolicyMismatch) => {
            PreflightOutcome::TrustedPolicyMismatch
        }
        Err(DurableTransactionPreparationError::AdmissionPolicyMismatch) => {
            PreflightOutcome::AdmissionPolicyMismatch
        }
        Err(error) => PreflightOutcome::Other(format!("{error:?}")),
    }
}

fn refused(error: DurableTransactionPreparationError) -> TestCaseError {
    TestCaseError::fail(format!("the trusted bind refused an admissible policy: {error:?}"))
}

fn forged_profile() -> SemanticProfileId {
    let mut bytes = *TestSemanticProfile::ID.as_bytes();
    bytes[0] ^= 0xff;
    SemanticProfileId::from_array(bytes)
}

fn changed_cold_limits() -> [DurableAdmissionLimitsV1; 3] {
    let unbounded = StructuralValidationLimits::unbounded();
    [
        StructuralValidationLimits {
            max_distinct_objects: u64::MAX - 1,
            ..unbounded
        },
        StructuralValidationLimits {
            max_canonical_bytes: u64::MAX - 1,
            ..unbounded
        },
        StructuralValidationLimits {
            max_references: u64::MAX - 1,
            ..unbounded
        },
    ]
    .map(|per_state_structural| DurableAdmissionLimitsV1 {
        per_state_structural,
        ..DurableAdmissionLimitsV1::UNBOUNDED
    })
}

fn per_state_limits(objects: u64, bytes: u64) -> DurableAdmissionLimitsV1 {
    DurableAdmissionLimitsV1 {
        per_state_structural: StructuralValidationLimits {
            max_distinct_objects: objects,
            max_canonical_bytes: bytes,
            ..StructuralValidationLimits::unbounded()
        },
        ..DurableAdmissionLimitsV1::UNBOUNDED
    }
}

fn run_histories(
    cases: u32,
    prefix: &'static str,
    property: &'static str,
    step: impl Fn(&mut ProductionHistory, &[u8], (u8, u8)) -> Result<(), TestCaseError>,
) {
    production_runner(cases)
        .run(&histories(), |operations| {
            for scenario in SCENARIOS {
                let mut history =
                    ProductionHistory::open(prefix, scenario.writer, (scenario.source)());
                for &operation in &operations {
                    if let Some(mutation) = history.mutation(operation) {
                        let published = history.past.len();
                        step(&mut history, &mutation, operation)?;
                        prop_assert_eq!(history.past.len(), published + 1);
                    }
                }
            }
            Ok(())
        })
        .expect(property);
}

pub(super) fn production_trusted_successor_equals_cold_successor(cases: u32) {
    run_histories(
        cases,
        "trusted-production-cold-",
        "the trusted successor equals the cold-checked successor at every history step",
        |history, mutation, operation| {
            let trusted = history
                .bind(mutation, TrustedSuccessorPolicyV1::UNBOUNDED)
                .map_err(refused)?;
            let cold = history.bind_cold(mutation, DurableAdmissionLimitsV1::UNBOUNDED);
            prop_assert_eq!(trusted.receipt(), cold.receipt());
            prop_assert_eq!(sealed_bytes(trusted.objects()), sealed_bytes(cold.objects()));
            prop_assert_eq!(trusted.retention_manifest(), cold.retention_manifest());
            prop_assert_eq!(trusted.successor_inventory(), cold.successor_inventory());
            let expected = trusted_observation(&trusted);
            history.publish(mutation, operation);
            prop_assert_eq!(expected, core_observation(history.source.core()));
            Ok(())
        },
    );
}

pub(super) fn production_trials_refuse_in_domain_order(cases: u32) {
    let coverage = Cell::new([0usize; 3]);
    run_histories(
        cases,
        "trusted-production-order-",
        "the trusted bind composes its trials in domain order and refuses at the first",
        |history, mutation, operation| {
            let bound = history
                .bind(mutation, TrustedSuccessorPolicyV1::UNBOUNDED)
                .map_err(refused)?;
            let measured = history.measure(&bound);
            prop_assert_eq!(
                ledger_observation(trusted_successor(&bound).retained_closure()),
                ledger_observation(&measured.ledger)
            );
            if let Some(delta) = history.metadata_delta(&bound) {
                let alone = run_ledger_trial(
                    &history.overlay(&bound),
                    history.source.core().retained_closure(),
                    ClosureDomainV1::Metadata,
                    &delta.added,
                    delta.removed.as_slice(),
                    LedgerTrialLimitsV1::UNBOUNDED,
                )
                .expect("the unbounded metadata trial on the predecessor ledger succeeds");
                prop_assert_eq!(alone.usage, measured.usage[2]);
                prop_assert_eq!(
                    domain_observation(&alone.successor, ClosureDomainV1::Metadata),
                    domain_observation(&measured.ledger, ClosureDomainV1::Metadata)
                );
            }
            let exact = exact_policy(&measured, &SuccessorCeilings::trusted(&bound));
            let witness = history.witness();
            let exactly_bound = history.bind(mutation, exact).map_err(refused)?;
            prop_assert_eq!(
                ledger_observation(trusted_successor(&exactly_bound).retained_closure()),
                ledger_observation(&measured.ledger)
            );
            let position = history.past.len();
            let lowered_axes = DOMAINS
                .into_iter()
                .filter_map(|domain| {
                    let positive = STEP_AXES
                        .into_iter()
                        .filter(|&axis| policy_value(exact, PolicyAxis::Work(domain, axis)) > 0)
                        .collect::<Vec<_>>();
                    match positive.len() {
                        0 => None,
                        count => Some((domain, positive[position % count])),
                    }
                })
                .collect::<Vec<_>>();
            let mut seen = coverage.get();
            for (first, &(domain, axis)) in lowered_axes.iter().enumerate() {
                seen[domain as usize] += 1;
                for last in first..lowered_axes.len() {
                    let policy = lowered_axes[first..=last]
                        .iter()
                        .fold(exact, |policy, &(domain, axis)| {
                            lowered(policy, PolicyAxis::Work(domain, axis))
                        });
                    prop_assert_eq!(
                        bind_outcome(history.bind(mutation, policy)),
                        BindOutcome::Trial(production_domain(domain), production_axis(axis))
                    );
                }
            }
            coverage.set(seen);
            prop_assert_eq!(history.witness(), witness);
            let published = history.publish_trusted(mutation, operation, exact)?;
            prop_assert_eq!(
                ledger_observation(published.retained_closure()),
                ledger_observation(&measured.ledger)
            );
            Ok(())
        },
    );
    let seen = coverage.get();
    for (domain, refusals) in DOMAINS.into_iter().zip(seen) {
        assert!(refusals > 0, "no production history refused a {domain:?} trial: {seen:?}");
    }
}

pub(super) fn production_policy_accepts_exact_and_refuses_one_below(cases: u32) {
    let coverage = Cell::new([0usize; 2]);
    run_histories(
        cases,
        "trusted-production-exact-",
        "every production policy axis accepts its exact value and refuses one below",
        |history, mutation, operation| {
            let bound = history
                .bind(mutation, TrustedSuccessorPolicyV1::UNBOUNDED)
                .map_err(refused)?;
            let measured = history.measure(&bound);
            let ceilings = SuccessorCeilings::trusted(&bound);
            let exact = exact_policy(&measured, &ceilings);
            let current = measured.ledger.inventory(ClosureDomainV1::Current);
            let witness = history.witness();
            prop_assert_eq!(bind_outcome(history.bind(mutation, exact)), BindOutcome::Prepared);
            let mut seen = coverage.get();
            for axis in policy_axes() {
                let value = policy_value(exact, axis);
                let Some(lower) = value.checked_sub(1) else {
                    continue;
                };
                let expected = refined(
                    expected_refusal(
                        axis,
                        (current.distinct_objects, current.canonical_bytes),
                        (ceilings.maxima.distinct_objects, ceilings.maxima.canonical_bytes),
                    ),
                    lower,
                    value,
                );
                match expected {
                    BindOutcome::Ceiling(..) => seen[0] += 1,
                    BindOutcome::Trial(..) => seen[1] += 1,
                    BindOutcome::Prepared |
                    BindOutcome::AuthorityMismatch |
                    BindOutcome::NotWritable(_) |
                    BindOutcome::Other(_) => {
                        return Err(TestCaseError::fail(format!(
                            "{axis:?} has no lowered refusal: {expected:?}"
                        )));
                    }
                }
                prop_assert_eq!(
                    bind_outcome(history.bind(mutation, policy_with(exact, axis, lower))),
                    expected
                );
            }
            coverage.set(seen);
            let exactly_bound = history.bind(mutation, exact).map_err(refused)?;
            let prepared = history
                .source
                .prepare_transaction(
                    &history.lease,
                    &exactly_bound,
                    history.sequence(),
                    LogPackDecodeLimits::UNBOUNDED,
                )
                .expect("prepare the exactly bound successor");
            for axis in policy_axes() {
                let Some(lower) = policy_value(exact, axis).checked_sub(1) else {
                    continue;
                };
                prop_assert_eq!(
                    preparation_refusal(traced(|| publish_trusted_incremental_v1(
                        &mut history.lease,
                        history.source.core(),
                        &prepared,
                        policy_with(exact, axis, lower),
                        |_| {},
                    )))?,
                    PreflightOutcome::TrustedPolicyMismatch
                );
            }
            prop_assert_eq!(history.witness(), witness);
            let published = trusted_published(observed_effects(|| {
                publish_trusted_incremental_v1(
                    &mut history.lease,
                    history.source.core(),
                    &prepared,
                    exact,
                    |_| {},
                )
            }))?;
            history.adopt_trusted(mutation, operation, published)?;
            prop_assert_eq!(&ceilings, &SuccessorCeilings::cold(&history.source));
            prop_assert_eq!(
                ledger_observation(&measured.ledger),
                ledger_observation(history.source.core().retained_closure())
            );
            Ok(())
        },
    );
    let seen = coverage.get();
    assert!(
        seen[0] > 0 && seen[1] > 0,
        "production ceiling and trial refusals were not both exercised: {seen:?}"
    );
}

pub(super) fn production_union_budget_admits_scratch_above_final_size(cases: u32) {
    let coverage = Cell::new(0usize);
    run_histories(
        cases,
        "trusted-production-union-",
        "a production union budget admits scratch above the final state size",
        |history, mutation, operation| {
            let bound = history
                .bind(mutation, TrustedSuccessorPolicyV1::UNBOUNDED)
                .map_err(refused)?;
            let measured = history.measure(&bound);
            let exact = exact_policy(&measured, &SuccessorCeilings::trusted(&bound));
            let witness = history.witness();
            prop_assert_eq!(bind_outcome(history.bind(mutation, exact)), BindOutcome::Prepared);
            for (domain, usage) in DOMAINS.into_iter().zip(measured.usage) {
                let inventory = measured.ledger.inventory(production_domain(domain));
                for (axis, peak, final_size) in [
                    (BudgetAxis::UnionObjects, usage.peak_objects, inventory.distinct_objects),
                    (BudgetAxis::UnionBytes, usage.peak_bytes, inventory.canonical_bytes),
                ] {
                    match peak.cmp(&final_size) {
                        std::cmp::Ordering::Less => {
                            return Err(TestCaseError::fail(format!(
                                "the {domain:?} {axis:?} peak {peak} is below its final size \
                                 {final_size}"
                            )));
                        }
                        std::cmp::Ordering::Equal => {}
                        std::cmp::Ordering::Greater => {
                            coverage.set(coverage.get() + 1);
                            let conflated =
                                policy_with(exact, PolicyAxis::Work(domain, axis), final_size);
                            prop_assert_eq!(
                                bind_outcome(history.bind(mutation, conflated)),
                                BindOutcome::Trial(
                                    production_domain(domain),
                                    production_axis(axis)
                                )
                            );
                        }
                    }
                }
            }
            prop_assert_eq!(history.witness(), witness);
            history.publish(mutation, operation);
            Ok(())
        },
    );
    assert!(coverage.get() > 0, "no production step had a union peak above its final inventory");
}

pub(super) fn production_binding_fixes_publication_mode_and_policy(cases: u32) {
    run_histories(
        cases,
        "trusted-production-mode-",
        "the sealed production binding fixes the publication mode and policy",
        |history, mutation, operation| {
            let bound = history
                .bind(mutation, TrustedSuccessorPolicyV1::UNBOUNDED)
                .map_err(refused)?;
            let exact = exact_policy(&history.measure(&bound), &SuccessorCeilings::trusted(&bound));
            let trusted = history.bind(mutation, exact).map_err(refused)?;
            let cold = history.bind_cold(mutation, DurableAdmissionLimitsV1::UNBOUNDED);
            let authority = history.source.authority().clone();
            let registry = history.lease.generation_leases();
            let witness = history.witness();
            let trusted_prepared = history
                .source
                .prepare_transaction(
                    &history.lease,
                    &trusted,
                    history.sequence(),
                    LogPackDecodeLimits::UNBOUNDED,
                )
                .expect("prepare the trusted-bound successor");
            let cold_prepared = history
                .source
                .prepare_transaction(
                    &history.lease,
                    &cold,
                    history.sequence(),
                    LogPackDecodeLimits::UNBOUNDED,
                )
                .expect("prepare the cold-checked successor");
            prop_assert_eq!(
                preflight_outcome(trusted_prepared.preflight_trusted(&authority, &registry, exact)),
                PreflightOutcome::Accepted
            );
            for axis in policy_axes() {
                let value = policy_value(exact, axis);
                for changed in [value.checked_add(1), value.checked_sub(1)]
                    .into_iter()
                    .flatten()
                {
                    prop_assert_eq!(
                        preflight_outcome(trusted_prepared.preflight_trusted(
                            &authority,
                            &registry,
                            policy_with(exact, axis, changed)
                        )),
                        PreflightOutcome::TrustedPolicyMismatch
                    );
                }
            }
            prop_assert_eq!(
                preflight_outcome(trusted_prepared.preflight(
                    &authority,
                    &registry,
                    DurableAdmissionLimitsV1::UNBOUNDED
                )),
                PreflightOutcome::ModeMismatch
            );
            prop_assert_eq!(
                preflight_outcome(cold_prepared.preflight_trusted(&authority, &registry, exact)),
                PreflightOutcome::ModeMismatch
            );
            prop_assert_eq!(
                preflight_outcome(cold_prepared.preflight(
                    &authority,
                    &registry,
                    DurableAdmissionLimitsV1::UNBOUNDED
                )),
                PreflightOutcome::Accepted
            );
            for changed in changed_cold_limits() {
                prop_assert_eq!(
                    preflight_outcome(cold_prepared.preflight(&authority, &registry, changed)),
                    PreflightOutcome::AdmissionPolicyMismatch
                );
            }
            match publish_incremental_from_lease_v1(
                &mut history.lease,
                &history.source,
                &trusted_prepared,
                limits(),
                LogPackDecodeLimits::UNBOUNDED,
                DurableAdmissionLimitsV1::UNBOUNDED,
            ) {
                Err(
                    error @ IncrementalPublicationErrorV1::Preparation(
                        DurableTransactionPreparationError::PublicationModeMismatch,
                    ),
                ) => prop_assert_eq!(
                    error.publication_outcome(),
                    GenerationCheckpointPublicationOutcomeV1::DefinitelyNotPublished
                ),
                Err(error) => {
                    return Err(TestCaseError::fail(format!(
                        "cold publication of a trusted binding failed otherwise: {error:?}"
                    )));
                }
                Ok(_) => {
                    return Err(TestCaseError::fail("cold publication accepted a trusted binding"));
                }
            }
            prop_assert_eq!(
                preparation_refusal(traced(|| publish_trusted_incremental_v1(
                    &mut history.lease,
                    history.source.core(),
                    &cold_prepared,
                    exact,
                    |_| {},
                )))?,
                PreflightOutcome::ModeMismatch
            );
            for axis in policy_axes() {
                let value = policy_value(exact, axis);
                for changed in [value.checked_add(1), value.checked_sub(1)]
                    .into_iter()
                    .flatten()
                {
                    prop_assert_eq!(
                        preparation_refusal(traced(|| publish_trusted_incremental_v1(
                            &mut history.lease,
                            history.source.core(),
                            &trusted_prepared,
                            policy_with(exact, axis, changed),
                            |_| {},
                        )))?,
                        PreflightOutcome::TrustedPolicyMismatch
                    );
                }
            }
            prop_assert_eq!(history.witness(), witness);
            let published = trusted_published(observed_effects(|| {
                publish_trusted_incremental_v1(
                    &mut history.lease,
                    history.source.core(),
                    &trusted_prepared,
                    exact,
                    |_| {},
                )
            }))?;
            history.adopt_trusted(mutation, operation, published)?;
            Ok(())
        },
    );
}

pub(super) fn production_bind_refuses_foreign_stale_and_replayed_receipts(cases: u32) {
    let coverage = Cell::new([0usize; 6]);
    production_runner(cases)
        .run(&histories(), |operations| {
            for scenario in SCENARIOS {
                let mut history = ProductionHistory::open(
                    "trusted-production-authority-",
                    scenario.writer,
                    (scenario.source)(),
                );
                let mut foreign = ProductionHistory::open(
                    "trusted-production-foreign-",
                    scenario.writer,
                    (scenario.foreign)(),
                );
                let foreign_mutation = data_batch(foreign.state(), "trusted-foreign-authority");
                for &operation in &operations {
                    let Some(mutation) = history.mutation(operation) else {
                        continue;
                    };
                    let mut seen = coverage.get();
                    let witness = history.witness();
                    let core = history.source.core();
                    prop_assert_ne!(foreign.state(), history.state());
                    prop_assert_eq!(
                        bind_outcome(core.bind_trusted_transaction(
                            &history.lease,
                            foreign.seal(&foreign_mutation),
                            TrustedSuccessorPolicyV1::UNBOUNDED
                        )),
                        BindOutcome::AuthorityMismatch
                    );
                    prop_assert_eq!(
                        bind_outcome(core.bind_trusted_transaction(
                            &foreign.lease,
                            history.seal(&mutation),
                            TrustedSuccessorPolicyV1::UNBOUNDED
                        )),
                        BindOutcome::AuthorityMismatch
                    );
                    seen[0] += 1;
                    seen[1] += 1;
                    if let Some(replayed) = history.past.last() {
                        prop_assert_eq!(
                            bind_outcome(history.bind_past(replayed)),
                            BindOutcome::AuthorityMismatch
                        );
                        seen[2] += 1;
                    }
                    if let [stale, _, ..] = history.past.as_slice() {
                        prop_assert_eq!(
                            bind_outcome(history.bind_past(stale)),
                            BindOutcome::AuthorityMismatch
                        );
                        seen[3] += 1;
                    }
                    prop_assert_eq!(&history.witness(), &witness);
                    let bound = history
                        .bind(&mutation, TrustedSuccessorPolicyV1::UNBOUNDED)
                        .map_err(refused)?;
                    let expected = trusted_observation(&bound);
                    let prepared = history
                        .source
                        .prepare_transaction(
                            &history.lease,
                            &bound,
                            history.sequence(),
                            LogPackDecodeLimits::UNBOUNDED,
                        )
                        .expect("prepare the trusted-bound successor");
                    let catalog = history.directory.path().join("catalog.v1");
                    let image = fs::read(&catalog).expect("read the predecessor catalog");
                    let inactive = CATALOG_SLOT_LEN
                        .checked_mul(history.source.authority().selection().inactive_slot_index())
                        .expect("a catalog slot offset fits usize");
                    let mut changed = image.clone();
                    changed[inactive] ^= 0xff;
                    fs::write(&catalog, &changed).expect("change the inactive catalog slot");
                    let (stale_catalog, trace) = traced(|| {
                        publish_trusted_incremental_v1(
                            &mut history.lease,
                            history.source.core(),
                            &prepared,
                            TrustedSuccessorPolicyV1::UNBOUNDED,
                            |_| {},
                        )
                    });
                    prop_assert!(trace.is_empty(), "a changed catalog ran publication phases");
                    match stale_catalog {
                        Err(error @ IncrementalPublicationErrorV1::CatalogCompareAndSwap) => {
                            prop_assert_eq!(
                                error.publication_outcome(),
                                GenerationCheckpointPublicationOutcomeV1::DefinitelyNotPublished
                            );
                        }
                        Err(error) => {
                            return Err(TestCaseError::fail(format!(
                                "a changed catalog failed outside its compare: {error:?}"
                            )));
                        }
                        Ok(_) => {
                            return Err(TestCaseError::fail(
                                "the trusted publication accepted a changed catalog",
                            ));
                        }
                    }
                    fs::write(&catalog, &image).expect("restore the predecessor catalog");
                    prop_assert_eq!(
                        preparation_refusal(traced(|| publish_trusted_incremental_v1(
                            &mut foreign.lease,
                            history.source.core(),
                            &prepared,
                            TrustedSuccessorPolicyV1::UNBOUNDED,
                            |_| {},
                        )))?,
                        PreflightOutcome::AuthorityMismatch
                    );
                    prop_assert_eq!(history.witness(), witness);
                    match history.past.len() % 2 {
                        0 => {
                            let published = trusted_published(observed_effects(|| {
                                publish_trusted_incremental_v1(
                                    &mut history.lease,
                                    history.source.core(),
                                    &prepared,
                                    TrustedSuccessorPolicyV1::UNBOUNDED,
                                    |_| {},
                                )
                            }))?;
                            prop_assert_eq!(core_observation(&published), expected);
                            history.adopt_trusted(&mutation, operation, published)?;
                            seen[4] += 1;
                        }
                        _ => {
                            let published_image = Rc::new(RefCell::new(None));
                            let stash = Rc::clone(&published_image);
                            let hook_catalog = catalog.clone();
                            arm_generation_checkpoint_publication_hook(
                                GenerationCheckpointPublicationHookPointV1::AfterCatalogSyncBeforeRecovery,
                                move || {
                                    *stash.borrow_mut() = Some(
                                        fs::read(&hook_catalog)
                                            .expect("read the published catalog"),
                                    );
                                    fs::write(&hook_catalog, &image)
                                        .expect("restore the predecessor catalog image");
                                },
                            );
                            let (published, trace) = traced(|| {
                                publish_trusted_incremental_v1(
                                    &mut history.lease,
                                    history.source.core(),
                                    &prepared,
                                    TrustedSuccessorPolicyV1::UNBOUNDED,
                                    |_| {},
                                )
                            });
                            prop_assert_eq!(trace, FORMAL_PUBLICATION_PHASE_ORDER[2..=14].to_vec());
                            let error = published.err().ok_or_else(|| {
                                TestCaseError::fail(
                                    "the trusted publication accepted a restored catalog",
                                )
                            })?;
                            prop_assert_eq!(
                                error.publication_outcome(),
                                GenerationCheckpointPublicationOutcomeV1::Published
                            );
                            match error {
                                IncrementalPublicationErrorV1::PublishedRecovery(startup) => {
                                    match *startup {
                                        DurableStartupErrorV1::Io(
                                            DurableIoError::CatalogAuthorityChanged {
                                                path: changed_root,
                                            },
                                        ) => prop_assert_eq!(
                                            changed_root.as_path(),
                                            history.directory.path()
                                        ),
                                        startup => {
                                            return Err(TestCaseError::fail(format!(
                                                "the post-swap catalog check failed otherwise: \
                                                 {startup:?}"
                                            )));
                                        }
                                    }
                                }
                                error => {
                                    return Err(TestCaseError::fail(format!(
                                        "a restored catalog failed outside its post-swap \
                                         check: {error:?}"
                                    )));
                                }
                            }
                            fs::write(
                                &catalog,
                                published_image
                                    .take()
                                    .expect("the post-swap catalog hook ran"),
                            )
                            .expect("restore the published catalog");
                            let recovered = recover_physical_from_lease_v1(
                                &mut history.lease,
                                limits(),
                                LogPackDecodeLimits::UNBOUNDED,
                            )
                            .expect("physical recovery of the published successor");
                            let successor = admit_physical_source::<TestSemanticProfile>(
                                &history.lease,
                                recovered,
                                DurableAdmissionLimitsV1::UNBOUNDED,
                            )
                            .expect("cold admission of the published successor");
                            prop_assert_eq!(core_observation(successor.core()), expected);
                            history.adopt(&mutation, operation, successor);
                            seen[5] += 1;
                        }
                    }
                    coverage.set(seen);
                }
            }
            Ok(())
        })
        .expect("foreign, stale and replayed receipts are refused before any durable mutation");
    let seen = coverage.get();
    for (slot, event) in [
        "foreign base",
        "foreign lease",
        "replayed receipt",
        "stale receipt",
        "trusted publication",
        "restored catalog",
    ]
    .iter()
    .enumerate()
    {
        assert!(seen[slot] > 0, "no production history exercised a {event}: {seen:?}");
    }
}

pub(super) fn production_maxima_equal_cold_requirements(cases: u32) {
    run_histories(
        cases,
        "trusted-production-maxima-",
        "trusted retained-state maxima equal the cold per-state requirements",
        |history, mutation, operation| {
            let bound = history
                .bind(mutation, TrustedSuccessorPolicyV1::UNBOUNDED)
                .map_err(refused)?;
            let maxima = trusted_successor(&bound).retained_state_maxima();
            let predecessor = history.source.core().retained_state_maxima();
            history.publish(mutation, operation);
            let successor = &history.source;
            let current = successor.report().current_structural;
            prop_assert_eq!(maxima, ClosureInventoryV1 {
                distinct_objects: predecessor.distinct_objects.max(current.distinct_objects),
                canonical_bytes: predecessor.canonical_bytes.max(current.canonical_bytes),
            });
            prop_assert_eq!(maxima, successor.core().retained_state_maxima());
            prop_assert_eq!(
                successor.preflight_checkpoint_admission(per_state_limits(
                    maxima.distinct_objects,
                    maxima.canonical_bytes
                )),
                Ok(())
            );
            let fewer_objects = maxima
                .distinct_objects
                .checked_sub(1)
                .expect("an admitted state holds at least its manifest");
            let fewer_bytes = maxima
                .canonical_bytes
                .checked_sub(1)
                .expect("an admitted state holds canonical bytes");
            for (limits, resource, limit, attempted) in [
                (
                    per_state_limits(fewer_objects, maxima.canonical_bytes),
                    DurableAdmissionLimitKindV1::PerStateDistinctObjects,
                    fewer_objects,
                    maxima.distinct_objects,
                ),
                (
                    per_state_limits(maxima.distinct_objects, fewer_bytes),
                    DurableAdmissionLimitKindV1::PerStateCanonicalBytes,
                    fewer_bytes,
                    maxima.canonical_bytes,
                ),
            ] {
                match successor.preflight_checkpoint_admission(limits) {
                    Err(refusal) => {
                        prop_assert_eq!(refusal.resource, resource);
                        prop_assert_eq!(refusal.limit, limit);
                        prop_assert_eq!(refusal.attempted, attempted);
                    }
                    Ok(()) => {
                        return Err(TestCaseError::fail(format!(
                            "the cold requirements admitted {resource:?} below the trusted maxima"
                        )));
                    }
                }
            }
            Ok(())
        },
    );
}

pub(super) fn production_validity_binds_the_exact_predecessor_and_receipt(cases: u32) {
    run_histories(
        cases,
        "trusted-production-validity-",
        "a production receipt binds only to its exact predecessor and profile",
        |history, mutation, operation| {
            let witness = history.witness();
            let forged = history
                .seal(mutation)
                .with_receipt_profile_for_test(forged_profile());
            prop_assert_eq!(
                bind_outcome(history.source.core().bind_trusted_transaction(
                    &history.lease,
                    forged,
                    TrustedSuccessorPolicyV1::UNBOUNDED
                )),
                BindOutcome::AuthorityMismatch
            );
            prop_assert_eq!(history.witness(), witness);
            let bound = history
                .bind(mutation, TrustedSuccessorPolicyV1::UNBOUNDED)
                .map_err(refused)?;
            let target = bound.receipt().target();
            let validity = trusted_successor(&bound).validity().manifest();
            prop_assert_eq!(validity, target.manifest());
            let predecessor = history.publish(mutation, operation);
            prop_assert_eq!(history.source.core().validity().manifest(), validity);
            let probe = data_batch(history.state(), "trusted-validity-probe");
            prop_assert_eq!(
                bind_outcome(predecessor.core().bind_trusted_transaction(
                    &history.lease,
                    history.seal(&probe),
                    TrustedSuccessorPolicyV1::UNBOUNDED
                )),
                BindOutcome::AuthorityMismatch
            );
            prop_assert_eq!(
                bind_outcome(history.bind(&probe, TrustedSuccessorPolicyV1::UNBOUNDED)),
                BindOutcome::Prepared
            );
            Ok(())
        },
    );
}

pub(super) fn production_retention_migrates_once_and_matches_the_cold_seed(cases: u32) {
    let coverage = Cell::new([0usize; 3]);
    production_runner(cases)
        .run(&histories(), |operations| {
            for scenario in SCENARIOS {
                let mut history = ProductionHistory::open(
                    "trusted-production-migration-",
                    scenario.writer,
                    (scenario.source)(),
                );
                for &operation in &operations {
                    let Some(mutation) = history.mutation(operation) else {
                        continue;
                    };
                    let mut seen = coverage.get();
                    let predecessor = history.source.core().retention_format();
                    let bound = history
                        .bind(&mutation, TrustedSuccessorPolicyV1::UNBOUNDED)
                        .map_err(refused)?;
                    let (_, _, metadata) = bound.successor_inventory();
                    let ledger = trusted_successor(&bound).retained_closure();
                    let expected = domain_observation(ledger, ClosureDomainV1::Metadata);
                    match scenario.writer {
                        RetentionFormat::V1 => {
                            prop_assert_eq!(metadata, None);
                            prop_assert_eq!(&expected, &DomainObservation {
                                objects: Vec::new(),
                                roots: Vec::new(),
                                inventory: ClosureInventoryV1::default(),
                            });
                        }
                        RetentionFormat::V2 => {
                            let anchor = bound
                                .retention_manifest()
                                .expect("a bound V2 successor seals its retention anchor");
                            prop_assert_eq!(metadata, Some(expected.inventory));
                            prop_assert_eq!(&expected.roots, &vec![(anchor.digest(), 1)]);
                            let migration = predecessor == RetentionFormat::V1;
                            prop_assert_eq!(migration, history.past.is_empty());
                            seen[usize::from(!migration)] += 1;
                        }
                    }
                    history.publish(&mutation, operation);
                    prop_assert_eq!(history.source.core().retention_format(), scenario.writer);
                    prop_assert_eq!(
                        domain_observation(
                            history.source.core().retained_closure(),
                            ClosureDomainV1::Metadata
                        ),
                        expected
                    );
                    coverage.set(seen);
                }
                if let (RetentionFormat::V2, false) = (scenario.writer, history.past.is_empty()) {
                    let ProductionHistory {
                        directory,
                        lease,
                        source,
                        ..
                    } = history;
                    drop((lease, source));
                    let (lease, source) = admit_parts(directory.path(), RetentionFormat::V1);
                    let files = directory_snapshot(directory.path());
                    let probe =
                        data_batch(source.authority().slot().state(), "trusted-v1-writer-probe");
                    let sealed = OpenDagTransaction::new(
                        source.core().verified_state(),
                        MutationBatchViewV1::parse(&probe).expect("parse the V1 writer probe"),
                        DagTransactionLimits::UNBOUNDED,
                    )
                    .and_then(OpenDagTransaction::seal)
                    .expect("seal the V1 writer probe");
                    prop_assert_eq!(
                        bind_outcome(source.core().bind_trusted_transaction(
                            &lease,
                            sealed,
                            TrustedSuccessorPolicyV1::UNBOUNDED
                        )),
                        BindOutcome::NotWritable(RetentionFormat::V2)
                    );
                    prop_assert_eq!(directory_snapshot(directory.path()), files);
                    let mut seen = coverage.get();
                    seen[2] += 1;
                    coverage.set(seen);
                }
            }
            Ok(())
        })
        .expect("a V1 source migrates once under the V2 writer and matches the cold seed");
    let seen = coverage.get();
    for (slot, event) in ["migration", "V2 successor", "V1 writer refusal"]
        .iter()
        .enumerate()
    {
        assert!(seen[slot] > 0, "no production history exercised a {event}: {seen:?}");
    }
}

pub(super) fn production_trusted_index_equals_cold_recovery(cases: u32) {
    run_histories(
        cases,
        "trusted-production-index-",
        "the trusted index equals the cold rebuild of the appointed records",
        |history, mutation, operation| {
            let core = history.source.core();
            let disk = durable_index_records(history.directory.path(), core.authority().slot());
            let predecessor = core.committed_index();
            prop_assert_eq!(predecessor.record_count(), count(disk.len()));
            for record in &disk {
                prop_assert_eq!(predecessor.get(record.reference().digest()), Some(*record));
            }
            let existing = *disk.first().expect("an admitted index holds its manifest");
            let probe = ObjectDigest::of_canonical_bytes(b"trusted-index-probe");
            prop_assert_eq!(predecessor.get(probe), None);
            let fresh = ObjectIndexRecordV1::new(
                ObjectRefV1::new(probe, existing.reference().encoded_len())
                    .expect("reference the index probe"),
                existing.location(),
            )
            .expect("index the probe at an indexed location");
            let appended = count(disk.len())
                .checked_add(1)
                .expect("a test index count has a successor");
            for (plan, expected) in [
                (vec![existing], Err(IndexFormatError::DuplicateData(existing.reference()))),
                (
                    vec![widened(existing)],
                    Err(IndexFormatError::DigestReferenceConflict {
                        digest: existing.reference().digest(),
                        existing: existing.reference(),
                        candidate: widened(existing).reference(),
                    }),
                ),
                (vec![fresh, fresh], Err(IndexFormatError::DuplicateData(fresh.reference()))),
                (
                    vec![fresh, widened(fresh)],
                    Err(IndexFormatError::DigestReferenceConflict {
                        digest: probe,
                        existing: fresh.reference(),
                        candidate: widened(fresh).reference(),
                    }),
                ),
                (vec![fresh], Ok(appended)),
            ] {
                prop_assert_eq!(index_plan(core, &disk, &plan)?, expected);
            }
            let tail = index_tail(disk.len().checked_add(1).expect("a test index length fits"));
            let wrong = tail
                .checked_add(INDEX_RECORD_LEN_V1)
                .expect("a test index tail has a successor record");
            prop_assert_eq!(
                core.trusted_successor_index([Ok(fresh)].into_iter(), wrong)
                    .map(|index| index.record_count()),
                Err(IndexFormatError::CanonicalStreamTail {
                    expected: tail,
                    appointed: wrong,
                })
            );
            let malformed = IndexFormatError::UnalignedDurableTail(1);
            prop_assert_eq!(
                core.trusted_successor_index(
                    [Err(malformed.clone())].into_iter(),
                    index_tail(disk.len())
                )
                .map(|index| index.record_count()),
                Err(malformed)
            );
            prop_assert_eq!(predecessor.record_count(), count(disk.len()));
            prop_assert_eq!(predecessor.get(probe), None);
            let published = history.publish_trusted(
                mutation,
                operation,
                TrustedSuccessorPolicyV1::UNBOUNDED,
            )?;
            let slot = published.authority().slot();
            let successor = durable_index_records(history.directory.path(), slot);
            prop_assert_eq!(successor.get(..disk.len()), Some(disk.as_slice()));
            prop_assert_eq!(index_tail(successor.len()), slot.pack().durable_index_tail());
            let readmitted = history.source.core().committed_index();
            prop_assert_eq!(published.committed_index().record_count(), count(successor.len()));
            prop_assert_eq!(readmitted.record_count(), count(successor.len()));
            for record in &successor {
                let digest = record.reference().digest();
                prop_assert_eq!(published.committed_index().get(digest), Some(*record));
                prop_assert_eq!(readmitted.get(digest), Some(*record));
            }
            Ok(())
        },
    );
}

pub(super) fn production_fresh_checkpoint_measures_new_cold_demand(cases: u32) {
    let coverage = Cell::new([0usize; 3]);
    production_runner(cases)
        .run(&histories(), |operations| {
            for (offset, scenario) in SCENARIOS.into_iter().enumerate() {
                let mut history = ProductionHistory::open(
                    "trusted-production-demand-",
                    scenario.writer,
                    (scenario.source)(),
                );
                let mut seen = coverage.get();
                for &operation in &operations {
                    let Some(mutation) = history.mutation(operation) else {
                        continue;
                    };
                    let previous = admission_demand(&history.source);
                    let trusted = history.publish_trusted(
                        &mutation,
                        operation,
                        TrustedSuccessorPolicyV1::UNBOUNDED,
                    )?;
                    let demand = admission_demand(&history.source);
                    let maxima = trusted.retained_state_maxima();
                    let union = trusted
                        .retained_closure()
                        .inventory(ClosureDomainV1::Retained);
                    let shape = trusted.shape();
                    prop_assert_eq!(
                        [
                            demand[0], demand[1], demand[4], demand[5], demand[6], demand[11],
                            demand[12], demand[13], demand[14],
                        ],
                        [
                            maxima.distinct_objects,
                            maxima.canonical_bytes,
                            trusted.retained_states(),
                            union.distinct_objects,
                            union.canonical_bytes,
                            shape.logical_nodes(),
                            shape.endpoints(),
                            shape
                                .maximum_logical_path_bytes()
                                .expect("an admitted shape summary's longest path fits u64"),
                            shape.source_payload_bytes(),
                        ]
                    );
                    let exact = demand_limits(&demand);
                    prop_assert_eq!(preflight_refusal(&history.source, exact), None);
                    for (axis, value) in ADMISSION_AXES.into_iter().zip(demand) {
                        let Some(lower) = value.checked_sub(1) else {
                            continue;
                        };
                        prop_assert_eq!(
                            preflight_refusal(
                                &history.source,
                                admission_limits_with(exact, axis, lower)
                            ),
                            Some((axis, lower, value))
                        );
                    }
                    let reused = ADMISSION_AXES[..4]
                        .iter()
                        .zip(previous)
                        .fold(exact, |limits, (&axis, value)| {
                            admission_limits_with(limits, axis, value)
                        });
                    let grown = ADMISSION_AXES[..4]
                        .iter()
                        .zip(previous.into_iter().zip(demand))
                        .find_map(|(&axis, (before, after))| {
                            (after > before).then_some((axis, before, after))
                        });
                    prop_assert_eq!(preflight_refusal(&history.source, reused), grown);
                    seen[0] += usize::from(grown.is_some());
                    let references = maxima
                        .distinct_objects
                        .checked_sub(1)
                        .expect("a retained state holds its manifest");
                    let summary = admission_limits_with(
                        exact,
                        DurableAdmissionLimitKindV1::PerStateReferences,
                        references,
                    );
                    let shared = (demand[2] > references).then_some((
                        DurableAdmissionLimitKindV1::PerStateReferences,
                        references,
                        demand[2],
                    ));
                    prop_assert_eq!(preflight_refusal(&history.source, summary), shared);
                    seen[1] += usize::from(shared.is_some());
                }
                match history.past.len() {
                    0 => {}
                    _ => {
                        let demand = admission_demand(&history.source);
                        let (axis, value) =
                            positive_demand_axis(&demand, operations.len() + offset);
                        let lower = value
                            .checked_sub(1)
                            .expect("a positive demand axis has a predecessor");
                        let lowered = admission_limits_with(demand_limits(&demand), axis, lower);
                        let ProductionHistory {
                            writer,
                            directory,
                            lease,
                            source,
                            ..
                        } = history;
                        let files = directory_snapshot(directory.path());
                        let checkpoint =
                            SemanticallyAdmittedDurableWriterV1::from_parts(lease, source)
                                .expect("reassemble the admitted writer");
                        let (refusal, trace) = traced(|| {
                            checkpoint.publish_generation_checkpoint_v1(
                                limits(),
                                LogPackDecodeLimits::UNBOUNDED,
                                lowered,
                            )
                        });
                        prop_assert!(
                            trace.is_empty(),
                            "an admission preflight refusal ran publication phases {trace:?}"
                        );
                        match refusal {
                            Err(GenerationCheckpointPublicationErrorV1::AdmissionPreflight(
                                refusal,
                            )) => prop_assert_eq!(
                                (refusal.resource, refusal.limit, refusal.attempted),
                                (axis, lower, value)
                            ),
                            Err(error) => {
                                return Err(TestCaseError::fail(format!(
                                    "the fresh checkpoint failed outside its admission preflight: \
                                     {error:?}"
                                )));
                            }
                            Ok(_) => {
                                return Err(TestCaseError::fail(
                                    "the fresh checkpoint admitted one below its cold demand",
                                ));
                            }
                        }
                        prop_assert_eq!(directory_snapshot(directory.path()), files);
                        let (lease, source) = admit_parts(directory.path(), writer);
                        prop_assert_eq!(admission_demand(&source), demand);
                        match SemanticallyAdmittedDurableWriterV1::from_parts(lease, source)
                            .expect("reassemble the restarted writer")
                            .publish_generation_checkpoint_v1(
                                limits(),
                                LogPackDecodeLimits::UNBOUNDED,
                                demand_limits(&demand),
                            ) {
                            Ok(published) => drop(published),
                            Err(error) => {
                                return Err(TestCaseError::fail(format!(
                                    "a fresh checkpoint refused its exact cold demand: {error:?}"
                                )));
                            }
                        }
                        seen[2] += 1;
                    }
                }
                coverage.set(seen);
            }
            Ok(())
        })
        .expect("a fresh checkpoint from a trusted core measures the new state's cold demand");
    let seen = coverage.get();
    for (slot, event) in ["reused-demand refusal", "summary-demand refusal", "fresh checkpoint"]
        .iter()
        .enumerate()
    {
        assert!(seen[slot] > 0, "no production history exercised a {event}: {seen:?}");
    }
}

pub(super) fn production_trusted_publication_skips_cold_admission(cases: u32) {
    production_runner(cases)
        .run(&histories(), |operations| {
            for scenario in SCENARIOS {
                let mut trusted = TrustedChain::open(
                    "trusted-production-skip-trusted-",
                    scenario.writer,
                    (scenario.source)(),
                );
                let mut cold = ProductionHistory::open(
                    "trusted-production-skip-cold-",
                    scenario.writer,
                    (scenario.source)(),
                );
                for &operation in &operations {
                    let mutation = trusted.mutation(operation);
                    prop_assert_eq!(&mutation, &cold.mutation(operation));
                    let Some(mutation) = mutation else {
                        continue;
                    };
                    prop_assert_eq!(
                        trusted.publish(
                            &mutation,
                            operation,
                            TrustedSuccessorPolicyV1::UNBOUNDED
                        )?,
                        PublicationEffects::trusted()
                    );
                    prop_assert_eq!(
                        cold.publish_observed(&mutation, operation).1,
                        PublicationEffects::cold()
                    );
                    prop_assert_eq!(
                        core_observation(&trusted.core),
                        core_observation(cold.source.core())
                    );
                }
            }
            Ok(())
        })
        .expect("a trusted publication runs no physical recovery, index rebuild or cold admission");
}

pub(super) fn production_trusted_and_cold_publication_write_identical_bytes(cases: u32) {
    let coverage = Cell::new([0usize; 3]);
    production_runner(cases)
        .run(&histories(), |operations| {
            for scenario in SCENARIOS {
                let mut resolutions = Vec::with_capacity(SIDECAR_STATES.len());
                for (slot, state) in SIDECAR_STATES.into_iter().enumerate() {
                    let (directory, lease, source, trusted_pin) =
                        checkpointed_parts("trusted-production-bytes-trusted-", scenario, state);
                    let mut trusted = TrustedChain::from_parts(directory, lease, source);
                    let (directory, lease, source, cold_pin) =
                        checkpointed_parts("trusted-production-bytes-cold-", scenario, state);
                    let mut cold =
                        ProductionHistory::from_parts(scenario.writer, directory, lease, source);
                    let before = directory_snapshot(trusted.directory.path());
                    prop_assert_eq!(
                        before.contains_key(RECLAMATION_INTENT),
                        matches!(state, SidecarState::Deferred | SidecarState::Reclaimable)
                    );
                    prop_assert_eq!(&before, &directory_snapshot(cold.directory.path()));
                    let mut resolution = None;
                    for &operation in &operations {
                        let mutation = trusted.mutation(operation);
                        prop_assert_eq!(&mutation, &cold.mutation(operation));
                        let Some(mutation) = mutation else {
                            continue;
                        };
                        prop_assert_eq!(
                            trusted.publish(
                                &mutation,
                                operation,
                                TrustedSuccessorPolicyV1::UNBOUNDED
                            )?,
                            PublicationEffects::trusted()
                        );
                        cold.publish(&mutation, operation);
                        let after = directory_snapshot(trusted.directory.path());
                        prop_assert_eq!(&after, &directory_snapshot(cold.directory.path()));
                        prop_assert_eq!(
                            core_observation(&trusted.core),
                            core_observation(cold.source.core())
                        );
                        resolution.get_or_insert_with(|| observed_resolution(&before, &after));
                    }
                    drop((trusted_pin, cold_pin));
                    if let Some(resolution) = resolution {
                        prop_assert_eq!(&resolution, &expected_resolution(state));
                        let mut seen = coverage.get();
                        seen[slot] += 1;
                        coverage.set(seen);
                        resolutions.push(resolution);
                    }
                }
                for (position, resolution) in resolutions.iter().enumerate() {
                    prop_assert!(
                        resolutions[position + 1..]
                            .iter()
                            .all(|other| other != resolution),
                        "two sidecar states resolved alike: {resolutions:?}"
                    );
                }
            }
            Ok(())
        })
        .expect("trusted and cold-checked publications write identical bytes for every sidecar");
    let seen = coverage.get();
    for (state, resolved) in SIDECAR_STATES.into_iter().zip(seen) {
        assert!(resolved > 0, "no production history resolved a {state:?} sidecar: {seen:?}");
    }
}

pub(super) fn production_trusted_publication_faults_recover_and_retry(cases: u32) {
    let coverage = Cell::new([0usize; 3]);
    production_runner(cases)
        .run(&histories(), |operations| {
            for (offset, scenario) in SCENARIOS.into_iter().enumerate() {
                let (directory, lease, source, faulted_pin) =
                    checkpointed_parts("trusted-production-fault-", scenario, SidecarState::Deferred);
                let mut faulted = TrustedChain::from_parts(directory, lease, source);
                let (directory, lease, source, reference_pin) = checkpointed_parts(
                    "trusted-production-fault-reference-",
                    scenario,
                    SidecarState::Deferred,
                );
                let mut reference = TrustedChain::from_parts(directory, lease, source);
                let mut seen = coverage.get();
                for (position, &operation) in operations.iter().enumerate() {
                    let mutation = faulted.mutation(operation);
                    prop_assert_eq!(&mutation, &reference.mutation(operation));
                    let Some(mutation) = mutation else {
                        continue;
                    };
                    prop_assert_eq!(
                        reference.publish(&mutation, operation, TrustedSuccessorPolicyV1::UNBOUNDED)?,
                        PublicationEffects::trusted()
                    );
                    let exact = TrustedPolicyEvidence::measure(
                        &faulted.lease,
                        &faulted.core,
                        faulted.seal(&mutation),
                    )
                    .exact();
                    let bound = faulted.bind(&mutation, exact).map_err(refused)?;
                    let expected = trusted_observation(&bound);
                    let prepared = faulted
                        .core
                        .prepare_transaction(
                            &faulted.lease,
                            &bound,
                            faulted.sequence(),
                            LogPackDecodeLimits::UNBOUNDED,
                        )
                        .expect("prepare the exactly bound successor");
                    let files = directory_snapshot(faulted.directory.path());
                    let endpoints = PolicyAxis::State(StateCeiling::Endpoints);
                    let changed = policy_with(
                        exact,
                        endpoints,
                        policy_value(exact, endpoints)
                            .checked_add(1)
                            .expect("an exact endpoint ceiling has a successor"),
                    );
                    prop_assert_eq!(
                        preparation_refusal(traced(|| publish_trusted_incremental_v1(
                            &mut faulted.lease,
                            &faulted.core,
                            &prepared,
                            changed,
                            |_| {},
                        )))?,
                        PreflightOutcome::TrustedPolicyMismatch
                    );
                    prop_assert_eq!(directory_snapshot(faulted.directory.path()), files);
                    match (position + offset) % 3 {
                        0 => {
                            let active = PackId::new(faulted.core.authority().slot().pack().active_pack())
                                .expect("the catalog appoints a nonzero active pack");
                            let pack = faulted.directory.path().join(pack_file_name(active));
                            arm_generation_checkpoint_publication_hook(
                                GenerationCheckpointPublicationHookPointV1::AfterArtifactDirectorySync,
                                move || {
                                    let file = OpenOptions::new()
                                        .write(true)
                                        .open(&pack)
                                        .expect("open the active pack");
                                    let length =
                                        file.metadata().expect("stat the active pack").len();
                                    file.set_len(
                                        length
                                            .checked_sub(1)
                                            .expect("a committed active pack is nonempty"),
                                    )
                                    .expect("truncate the active pack");
                                    file.sync_all().expect("sync the truncated active pack");
                                },
                            );
                            let (published, trace) = traced(|| {
                                publish_trusted_incremental_v1(
                                    &mut faulted.lease,
                                    &faulted.core,
                                    &prepared,
                                    exact,
                                    |_| {},
                                )
                            });
                            prop_assert_eq!(trace, FORMAL_PUBLICATION_PHASE_ORDER[2..=8].to_vec());
                            let error = published.err().ok_or_else(|| {
                                TestCaseError::fail("the trusted publication pinned a truncated pack")
                            })?;
                            prop_assert_eq!(
                                error.publication_outcome(),
                                GenerationCheckpointPublicationOutcomeV1::DefinitelyNotPublished
                            );
                            match error {
                                IncrementalPublicationErrorV1::TrustedSuccessorAssembly(
                                    assembly,
                                ) => match *assembly {
                                    DurableAdmissionErrorV1::PackTail {
                                        pack,
                                        authorized,
                                        available,
                                    } => {
                                        prop_assert_eq!(pack, active);
                                        prop_assert_eq!(available.checked_add(1), Some(authorized));
                                    }
                                    assembly => {
                                        return Err(TestCaseError::fail(format!(
                                            "the truncated pack failed assembly otherwise: \
                                             {assembly:?}"
                                        )));
                                    }
                                },
                                error => {
                                    return Err(TestCaseError::fail(format!(
                                        "the truncated pack failed outside assembly: {error:?}"
                                    )));
                                }
                            }
                            faulted.recover(None);
                            prop_assert_eq!(
                                faulted.publish(&mutation, operation, exact)?,
                                PublicationEffects::trusted()
                            );
                            seen[0] += 1;
                        }
                        1 => {
                            arm_durable_io_failpoint(DurableIoFailpointV1::CatalogDirectorySync);
                            let (published, trace) = traced(|| {
                                publish_trusted_incremental_v1(
                                    &mut faulted.lease,
                                    &faulted.core,
                                    &prepared,
                                    exact,
                                    |_| {},
                                )
                            });
                            prop_assert_eq!(trace, FORMAL_PUBLICATION_PHASE_ORDER[2..=12].to_vec());
                            let error = published.err().ok_or_else(|| {
                                TestCaseError::fail("the catalog directory sync fault was ignored")
                            })?;
                            prop_assert_eq!(
                                error.publication_outcome(),
                                GenerationCheckpointPublicationOutcomeV1::Published
                            );
                            prop_assert!(
                                matches!(
                                    error,
                                    IncrementalPublicationErrorV1::PublishedCatalogDirectory(_)
                                ),
                                "the catalog directory sync failed otherwise: {error:?}"
                            );
                            faulted.recover(Some(operation));
                            seen[1] += 1;
                        }
                        _ => {
                            let sidecar = faulted.directory.path().join(RECLAMATION_INTENT);
                            let intent =
                                fs::read(&sidecar).expect("read the deferred reclamation intent");
                            let mut corrupted = intent.clone();
                            *corrupted
                                .last_mut()
                                .expect("a reclamation intent is nonempty") ^= 0xff;
                            let hook_sidecar = sidecar.clone();
                            arm_generation_checkpoint_publication_hook(
                                GenerationCheckpointPublicationHookPointV1::AfterCatalogSyncBeforeRecovery,
                                move || {
                                    fs::write(&hook_sidecar, &corrupted)
                                        .expect("corrupt the reclamation intent");
                                },
                            );
                            let (published, trace) = traced(|| {
                                publish_trusted_incremental_v1(
                                    &mut faulted.lease,
                                    &faulted.core,
                                    &prepared,
                                    exact,
                                    |_| {},
                                )
                            });
                            prop_assert_eq!(trace, FORMAL_PUBLICATION_PHASE_ORDER[2..=14].to_vec());
                            let error = published.err().ok_or_else(|| {
                                TestCaseError::fail("a corrupted reclamation intent was resolved")
                            })?;
                            prop_assert_eq!(
                                error.publication_outcome(),
                                GenerationCheckpointPublicationOutcomeV1::Published
                            );
                            match error {
                                IncrementalPublicationErrorV1::PublishedRecovery(startup) => {
                                    match *startup {
                                        DurableStartupErrorV1::Io(DurableIoError::Io {
                                            source,
                                            ..
                                        }) if source.kind() == std::io::ErrorKind::InvalidData => {}
                                        startup => {
                                            return Err(TestCaseError::fail(format!(
                                                "the corrupted intent failed otherwise: \
                                                 {startup:?}"
                                            )));
                                        }
                                    }
                                }
                                error => {
                                    return Err(TestCaseError::fail(format!(
                                        "the corrupted intent failed outside resolution: \
                                         {error:?}"
                                    )));
                                }
                            }
                            fs::write(&sidecar, &intent).expect("restore the reclamation intent");
                            faulted.recover(Some(operation));
                            seen[2] += 1;
                        }
                    }
                    prop_assert_eq!(core_observation(&faulted.core), expected);
                    prop_assert_eq!(
                        core_observation(&faulted.core),
                        core_observation(&reference.core)
                    );
                    prop_assert_eq!(
                        directory_snapshot(faulted.directory.path()),
                        directory_snapshot(reference.directory.path())
                    );
                }
                coverage.set(seen);
                drop((faulted_pin, reference_pin));
            }
            Ok(())
        })
        .expect("a faulted trusted publication recovers to the reference successor and retries");
    let seen = coverage.get();
    for (slot, fault) in ["pin", "catalog directory sync", "reclamation intent"]
        .iter()
        .enumerate()
    {
        assert!(seen[slot] > 0, "no production history injected a {fault} fault: {seen:?}");
    }
}

pub(super) fn production_trusted_success_does_not_imply_cold_admission(cases: u32) {
    let coverage = Cell::new([0usize; 2]);
    production_runner(cases)
        .run(&histories(), |operations| {
            for (offset, scenario) in SCENARIOS.into_iter().enumerate() {
                let mut trusted = TrustedChain::open(
                    "trusted-production-imply-trusted-",
                    scenario.writer,
                    (scenario.source)(),
                );
                let mut cold = ProductionHistory::open(
                    "trusted-production-imply-cold-",
                    scenario.writer,
                    (scenario.source)(),
                );
                let mut seen = coverage.get();
                for (position, &operation) in operations.iter().enumerate() {
                    let mutation = trusted.mutation(operation);
                    prop_assert_eq!(&mutation, &cold.mutation(operation));
                    let Some(mutation) = mutation else {
                        continue;
                    };
                    prop_assert_eq!(
                        trusted.publish(
                            &mutation,
                            operation,
                            TrustedSuccessorPolicyV1::UNBOUNDED
                        )?,
                        PublicationEffects::trusted()
                    );
                    let demand = admission_demand(
                        &acquire_cold_checked(
                            &mut trusted.lease,
                            &trusted.core,
                            limits(),
                            LogPackDecodeLimits::UNBOUNDED,
                            DurableAdmissionLimitsV1::UNBOUNDED,
                        )
                        .expect("cold admission of the trusted successor"),
                    );
                    let (axis, value) = positive_demand_axis(&demand, position + offset);
                    let tight = admission_limits_with(
                        demand_limits(&demand),
                        axis,
                        value
                            .checked_sub(1)
                            .expect("a positive demand axis has a predecessor"),
                    );
                    match cold.source.seal_transaction(
                        &cold.lease,
                        MutationBatchViewV1::parse(&mutation).expect("parse the step mutation"),
                        DagTransactionLimits::UNBOUNDED,
                        tight,
                    ) {
                        Err(
                            DurableTransactionPreparationError::AdmissionPreflight(_) |
                            DurableTransactionPreparationError::Inventory(_) |
                            DurableTransactionPreparationError::RetentionWrite(_),
                        ) => {
                            seen[0] += 1;
                            cold.publish(&mutation, operation);
                        }
                        Err(error) => {
                            return Err(TestCaseError::fail(format!(
                                "the cold bind under the tightened limits failed otherwise: \
                                 {error:?}"
                            )));
                        }
                        Ok(sealed) => {
                            let prepared = cold
                                .source
                                .prepare_transaction(
                                    &cold.lease,
                                    &sealed,
                                    cold.sequence(),
                                    LogPackDecodeLimits::UNBOUNDED,
                                )
                                .expect("prepare the cold-checked successor");
                            let error = publish_incremental_from_lease_v1(
                                &mut cold.lease,
                                &cold.source,
                                &prepared,
                                limits(),
                                LogPackDecodeLimits::UNBOUNDED,
                                tight,
                            )
                            .err()
                            .ok_or_else(|| {
                                TestCaseError::fail(
                                    "cold admission accepted one below the successor's demand",
                                )
                            })?;
                            prop_assert_eq!(
                                error.publication_outcome(),
                                GenerationCheckpointPublicationOutcomeV1::Published
                            );
                            prop_assert!(
                                matches!(
                                    error,
                                    IncrementalPublicationErrorV1::PublishedAdmission(_)
                                ),
                                "the tightened cold publication failed otherwise: {error:?}"
                            );
                            let recovered = recover_physical_from_lease_v1(
                                &mut cold.lease,
                                limits(),
                                LogPackDecodeLimits::UNBOUNDED,
                            )
                            .expect("physical recovery of the published successor");
                            let successor = admit_physical_source::<TestSemanticProfile>(
                                &cold.lease,
                                recovered,
                                DurableAdmissionLimitsV1::UNBOUNDED,
                            )
                            .expect("cold admission of the published successor");
                            cold.adopt(&mutation, operation, successor);
                            seen[1] += 1;
                        }
                    }
                    prop_assert_eq!(
                        core_observation(&trusted.core),
                        core_observation(cold.source.core())
                    );
                }
                coverage.set(seen);
            }
            Ok(())
        })
        .expect("a trusted success never implies cold admission under the incoming limits");
    let seen = coverage.get();
    assert!(
        seen[0] > 0 && seen[1] > 0,
        "tightened cold limits were not refused at both the bind and the published admission: \
         {seen:?}"
    );
}
