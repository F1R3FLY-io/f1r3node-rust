use std::borrow::Cow;
use std::cell::Cell;
use std::collections::BTreeMap;
use std::path::Path;

use proptest::prelude::*;
use proptest::test_runner::TestRunner;

use super::checkpoint_wal::{SyntheticCheckpointSource, fixture, semantic_fixture};
use super::durable_publication::{directory_snapshot, install_checkpoint_image};
use super::durable_recovery::{checkpoint_image_from_source, limits};
use super::incremental_reachability_properties::{
    BudgetAxis, DOMAINS, Domain, STEP_AXES, TrialRefusal,
};
use super::support::{PersistenceTrialDir, TestSemanticProfile, trial_scratch};
use super::transaction_sealing::data_batch;
use super::trusted_successor_properties::{
    BindRefusal, CeilingAxis, PolicyAxis, StateCeiling, expected_refusal, policy_axes,
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
    AdmittedCheckpointSourceV1, DurableAdmissionLimitKindV1, DurableAdmissionLimitsV1,
    DurableDirectoryV1, DurableIndexedObjectStoreV1, DurableObjectReadErrorV1,
    DurableTransactionPreparationError, DurableWriterLeaseV1,
    GenerationCheckpointPublicationOutcomeV1, IncrementalPublicationErrorV1,
    PreparedTrustedSuccessorV1, SealedDurableTransactionV1, SuccessorBindingV1,
    TrustedCeilingAxisV1, TrustedStateCeilingsV1, TrustedSuccessorPolicyV1, TrustedTrialWorkV1,
    publish_incremental_from_lease_v1,
};
use crate::rspace::persistence::retention::v2_format::{RetentionFormat, RetentionReader};
use crate::rspace::persistence::semantic::CurrentShapeSummaryV1;
use crate::rspace::persistence::transaction::{SemanticValidityV1, ValidityOnlyV1};
use crate::rspace::persistence::{
    CanonicalEncode, CatalogAuthorityTokenV1, DagTransactionLimits, ImmutableObjectStore,
    LogPackDecodeLimits, LogPackSequence, MutationBatchViewV1, ObjectDigest, ObjectRefV1,
    OpenDagTransaction, RSpaceMutationBatchV1, SealedDagTransaction, SealedObject, SemanticProfile,
    SemanticProfileId, StateVersionV1, StructuralValidationLimits, VerifiedDagState,
};

#[derive(Clone, Copy)]
struct Scenario {
    writer: RetentionFormat,
    source: fn() -> SyntheticCheckpointSource,
    foreign: fn() -> SyntheticCheckpointSource,
}

const SCENARIOS: [Scenario; 4] = [
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
fn admit_parts(
    root: &Path,
    writer: RetentionFormat,
) -> (DurableWriterLeaseV1, AdmittedCheckpointSourceV1<TestSemanticProfile>) {
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
        .into_parts()
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
struct SuccessorObservation {
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
enum BindOutcome {
    Prepared,
    AuthorityMismatch,
    NotWritable(RetentionFormat),
    Trial(ClosureDomainV1, LedgerTrialAxisV1),
    Ceiling(TrustedCeilingAxisV1, u64, u64),
    Other(String),
}

#[derive(Debug, PartialEq, Eq)]
enum PreflightOutcome {
    Accepted,
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

    fn mutation(&self, (identity, after): (u8, u8)) -> Option<Vec<u8>> {
        match self.counts[usize::from(identity)] {
            before if before == after => None,
            before => {
                let channel = "inventory-channel".to_owned();
                let datum = Datum::create(&channel, format!("datum-{identity}"), false);
                let path = RSpaceDataIndex::datum_path(&channel, &datum);
                let mut batch = RSpaceMutationBatchV1::new(self.state())
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

    fn publish(
        &mut self,
        mutation: &[u8],
        (identity, after): (u8, u8),
    ) -> AdmittedCheckpointSourceV1<TestSemanticProfile> {
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
        let successor = publish_incremental_from_lease_v1(
            &mut self.lease,
            &self.source,
            &prepared,
            limits(),
            LogPackDecodeLimits::UNBOUNDED,
            DurableAdmissionLimitsV1::UNBOUNDED,
        )
        .expect("publish the cold-checked successor");
        let predecessor = std::mem::replace(&mut self.source, successor);
        self.past.push(PastState {
            version: predecessor.authority().slot().state(),
            validity: predecessor.core().validity(),
            mutation: mutation.to_vec(),
        });
        self.counts[usize::from(identity)] = after;
        predecessor
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
        match (self.writer, self.source.core().retention_format()) {
            (RetentionFormat::V1, _) => None,
            (RetentionFormat::V2, predecessor) => Some(MetadataDelta {
                added: [bound
                    .retention_manifest()
                    .expect("a bound V2 successor seals its retention anchor")],
                removed: match predecessor {
                    RetentionFormat::V1 => None,
                    RetentionFormat::V2 => {
                        Some(self.source.authority().slot().pack().retention_manifest())
                    }
                },
            }),
        }
    }

    fn measure(
        &self,
        bound: &SealedDurableTransactionV1<'_, TestSemanticProfile>,
    ) -> MeasuredSuccessor {
        let store = self.overlay(bound);
        let receipt = bound.receipt();
        let target = [receipt.target().manifest()];
        let base = [receipt.base().manifest()];
        let trials = run_domain_trials(
            &store,
            self.source.core().retained_closure(),
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
        match self.metadata_delta(bound) {
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

fn trusted_successor<'a>(
    bound: &'a SealedDurableTransactionV1<'_, TestSemanticProfile>,
) -> &'a PreparedTrustedSuccessorV1 {
    match bound.binding() {
        SuccessorBindingV1::Trusted { successor, .. } => successor,
        SuccessorBindingV1::ColdChecked { limits } => {
            panic!("a trusted bind carries a cold-checked binding with {limits:?}")
        }
    }
}

fn trusted_observation(
    bound: &SealedDurableTransactionV1<'_, TestSemanticProfile>,
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

fn cold_observation(
    source: &AdmittedCheckpointSourceV1<TestSemanticProfile>,
) -> SuccessorObservation {
    let core = source.core();
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

    fn trusted(bound: &SealedDurableTransactionV1<'_, TestSemanticProfile>) -> Self {
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

fn policy_with(
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

fn bind_outcome<T>(result: Result<T, DurableTransactionPreparationError>) -> BindOutcome {
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

fn preflight_outcome(result: Result<(), DurableTransactionPreparationError>) -> PreflightOutcome {
    match result {
        Ok(()) => PreflightOutcome::Accepted,
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
    TestCaseError::fail(format!("the trusted bind refused an unbounded policy: {error:?}"))
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
            prop_assert_eq!(expected, cold_observation(&history.source));
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
            history.publish(mutation, operation);
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
            prop_assert_eq!(history.witness(), witness);
            history.publish(mutation, operation);
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
            prop_assert_eq!(history.witness(), witness);
            history.publish(mutation, operation);
            Ok(())
        },
    );
}

pub(super) fn production_bind_refuses_foreign_stale_and_replayed_receipts(cases: u32) {
    let coverage = Cell::new([0usize; 4]);
    production_runner(cases)
        .run(&histories(), |operations| {
            for scenario in SCENARIOS {
                let mut history = ProductionHistory::open(
                    "trusted-production-authority-",
                    scenario.writer,
                    (scenario.source)(),
                );
                let foreign = ProductionHistory::open(
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
                    prop_assert_eq!(history.witness(), witness);
                    coverage.set(seen);
                    history.publish(&mutation, operation);
                }
            }
            Ok(())
        })
        .expect("foreign, stale and replayed receipts are refused before any durable mutation");
    let seen = coverage.get();
    for (slot, event) in ["foreign base", "foreign lease", "replayed receipt", "stale receipt"]
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
