use std::cell::Cell;
use std::collections::{HashSet, VecDeque};

use proptest::prelude::*;

use super::durable_publication::production_trusted_crash_recovers_and_retries_every_phase;
use super::incremental_reachability_properties::{
    BUDGET_AXES, BudgetAxis, ComposedCase, ComposedDelta, ComposedEnvironment, ComposedEvidence,
    ComposedLimits, ComposedOutcome, ComposedPredecessor, CompositionVariant, DOMAINS, Domain,
    Ledger, Predecessor, PreparedEvidence, RetentionEntries, RetentionEntry, RootDelta, STEP_AXES,
    TrialCase, TrialEnvironment, TrialLimits, TrialOutcome, TrialRefusal, TrialResult, TrialUsage,
    TrialVariant, apply_delta, as_predecessor, cold_graph_oracle, cold_predecessor,
    history_environment, root_counts,
};
use super::trusted_successor_production::{
    production_bind_refuses_foreign_stale_and_replayed_receipts,
    production_binding_fixes_publication_mode_and_policy,
    production_fresh_checkpoint_measures_new_cold_demand,
    production_maxima_equal_cold_requirements,
    production_policy_accepts_exact_and_refuses_one_below,
    production_retention_migrates_once_and_matches_the_cold_seed,
    production_trials_refuse_in_domain_order,
    production_trusted_and_cold_publication_write_identical_bytes,
    production_trusted_index_equals_cold_recovery,
    production_trusted_publication_faults_recover_and_retry,
    production_trusted_publication_skips_cold_admission,
    production_trusted_successor_equals_cold_successor,
    production_union_budget_admits_scratch_above_final_size,
    production_validity_binds_the_exact_predecessor_and_receipt,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct SharedLedger([Predecessor; 3]);

impl SharedLedger {
    fn of(base: &ComposedPredecessor) -> Self {
        Self([base.current.clone(), base.retained.clone(), base.metadata.clone()])
    }

    pub(super) fn project(&self, domain: Domain) -> &Predecessor { &self.0[domain as usize] }

    fn projections(&self) -> ComposedPredecessor {
        ComposedPredecessor {
            current: self.project(Domain::Current).clone(),
            retained: self.project(Domain::Retained).clone(),
            metadata: self.project(Domain::Metadata).clone(),
        }
    }

    fn write_domain(&self, domain: Domain, evidence: &PreparedEvidence) -> Self {
        let mut written = self.clone();
        written.0[domain as usize] = as_predecessor(evidence);
        written
    }

    fn commit(&self, evidence: &ComposedEvidence) -> Self {
        DOMAINS.into_iter().fold(self.clone(), |shared, domain| {
            shared.write_domain(domain, evidence.domain(domain))
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TrustedVariant {
    Faithful,
    SkipRetainedTrial,
    SkipMetadataTrial,
    IgnoreSealedPolicy,
    PublishBeforeTrials,
}

fn skipped_trial(base: &Predecessor) -> TrialResult {
    TrialResult {
        retained: base.clone(),
        outcome: TrialOutcome::Prepared(PreparedEvidence {
            ledger: base.ledger.clone(),
            roots: base.roots.clone(),
            objects: base.objects,
            bytes: base.bytes,
            usage: TrialUsage::default(),
        }),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ProductionResult {
    Prepared(SharedLedger, ComposedEvidence),
    Refused(Domain, TrialRefusal),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum StateCeiling {
    StateObjects,
    StateBytes,
    RetainedStates,
    UnionObjects,
    UnionBytes,
    MetadataObjects,
    MetadataBytes,
    LogicalNodes,
    Endpoints,
    LogicalPathBytes,
    PayloadBytes,
}

const STATE_CEILINGS: [StateCeiling; 11] = [
    StateCeiling::StateObjects,
    StateCeiling::StateBytes,
    StateCeiling::RetainedStates,
    StateCeiling::UnionObjects,
    StateCeiling::UnionBytes,
    StateCeiling::MetadataObjects,
    StateCeiling::MetadataBytes,
    StateCeiling::LogicalNodes,
    StateCeiling::Endpoints,
    StateCeiling::LogicalPathBytes,
    StateCeiling::PayloadBytes,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PolicyAxis {
    State(StateCeiling),
    Work(Domain, BudgetAxis),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct TrustedPolicy {
    state: [u64; 11],
    work: [[u64; 7]; 3],
}

impl TrustedPolicy {
    pub(super) const GENEROUS: Self = Self {
        state: [64; 11],
        work: [[64; 7]; 3],
    };
    pub(super) const UNBOUNDED: Self = Self {
        state: [u64::MAX; 11],
        work: [[u64::MAX; 7]; 3],
    };
    const ZERO: Self = Self {
        state: [0; 11],
        work: [[0; 7]; 3],
    };

    fn ceiling(&self, ceiling: StateCeiling) -> u64 { self.state[ceiling as usize] }

    fn get(&self, axis: PolicyAxis) -> u64 {
        match axis {
            PolicyAxis::State(ceiling) => self.ceiling(ceiling),
            PolicyAxis::Work(domain, axis) => self.work[domain as usize][axis as usize],
        }
    }

    fn with(mut self, axis: PolicyAxis, value: u64) -> Self {
        match axis {
            PolicyAxis::State(ceiling) => self.state[ceiling as usize] = value,
            PolicyAxis::Work(domain, axis) => self.work[domain as usize][axis as usize] = value,
        }
        self
    }
}

pub(super) fn policy_axes() -> Vec<PolicyAxis> {
    let mut axes = Vec::with_capacity(STATE_CEILINGS.len() + DOMAINS.len() * STEP_AXES.len());
    axes.extend(STATE_CEILINGS.map(PolicyAxis::State));
    for domain in DOMAINS {
        axes.extend(STEP_AXES.map(|axis| PolicyAxis::Work(domain, axis)));
    }
    axes
}

fn final_ceilings(domain: Domain) -> [StateCeiling; 2] {
    match domain {
        Domain::Current => [StateCeiling::StateObjects, StateCeiling::StateBytes],
        Domain::Retained => [StateCeiling::UnionObjects, StateCeiling::UnionBytes],
        Domain::Metadata => [StateCeiling::MetadataObjects, StateCeiling::MetadataBytes],
    }
}

fn work_limits(work: [u64; 7], final_objects: u64, final_bytes: u64) -> TrialLimits {
    TrialLimits([
        work[0],
        work[1],
        work[2],
        work[3],
        work[4],
        work[5],
        work[6],
        final_objects,
        final_bytes,
    ])
}

fn policy_budget(policy: &TrustedPolicy) -> ComposedLimits {
    ComposedLimits(DOMAINS.map(|domain| {
        let [objects, bytes] = final_ceilings(domain);
        work_limits(policy.work[domain as usize], policy.ceiling(objects), policy.ceiling(bytes))
    }))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CeilingAxis {
    StateObjects,
    StateBytes,
    RetainedStates,
    LogicalNodes,
    Endpoints,
    LogicalPathBytes,
    PayloadBytes,
}

const CEILING_AXES: [CeilingAxis; 7] = [
    CeilingAxis::StateObjects,
    CeilingAxis::StateBytes,
    CeilingAxis::RetainedStates,
    CeilingAxis::LogicalNodes,
    CeilingAxis::Endpoints,
    CeilingAxis::LogicalPathBytes,
    CeilingAxis::PayloadBytes,
];

impl CeilingAxis {
    fn ceiling(self) -> StateCeiling {
        match self {
            Self::StateObjects => StateCeiling::StateObjects,
            Self::StateBytes => StateCeiling::StateBytes,
            Self::RetainedStates => StateCeiling::RetainedStates,
            Self::LogicalNodes => StateCeiling::LogicalNodes,
            Self::Endpoints => StateCeiling::Endpoints,
            Self::LogicalPathBytes => StateCeiling::LogicalPathBytes,
            Self::PayloadBytes => StateCeiling::PayloadBytes,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct ShapeSummary {
    logical_nodes: u64,
    endpoint_count: u64,
    logical_path_bytes: u64,
    payload_bytes: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SuccessorValues {
    maxima: (u64, u64),
    retained_states: u64,
    shape: ShapeSummary,
}

impl SuccessorValues {
    fn value(&self, axis: CeilingAxis) -> u64 {
        match axis {
            CeilingAxis::StateObjects => self.maxima.0,
            CeilingAxis::StateBytes => self.maxima.1,
            CeilingAxis::RetainedStates => self.retained_states,
            CeilingAxis::LogicalNodes => self.shape.logical_nodes,
            CeilingAxis::Endpoints => self.shape.endpoint_count,
            CeilingAxis::LogicalPathBytes => self.shape.logical_path_bytes,
            CeilingAxis::PayloadBytes => self.shape.payload_bytes,
        }
    }
}

fn first_ceiling_exceeded(policy: &TrustedPolicy, values: &SuccessorValues) -> Option<CeilingAxis> {
    CEILING_AXES
        .into_iter()
        .find(|&axis| policy.ceiling(axis.ceiling()) < values.value(axis))
}

fn exact_ceilings(values: &SuccessorValues, base: &TrustedPolicy) -> TrustedPolicy {
    CEILING_AXES.into_iter().fold(*base, |policy, axis| {
        policy.with(PolicyAxis::State(axis.ceiling()), values.value(axis))
    })
}

const INDEX_HEADER_LEN: u64 = 64;
const INDEX_RECORD_LEN: u64 = 72;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct IndexRecord {
    digest: usize,
    reference: usize,
    location: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct CommittedIndex {
    entries: Vec<IndexRecord>,
    count: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum IndexRefusal {
    DuplicateData,
    DigestReferenceConflict,
    RecordCountOverflow,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum IndexResult {
    Built(CommittedIndex),
    Refused(IndexRecord, IndexRefusal),
}

impl CommittedIndex {
    fn lookup(&self, digest: usize) -> Option<&IndexRecord> {
        self.entries.iter().find(|record| record.digest == digest)
    }

    fn validate_insert(&self, capacity: usize, record: &IndexRecord) -> Option<IndexRefusal> {
        match self.lookup(record.digest) {
            Some(existing) if existing.reference == record.reference => {
                Some(IndexRefusal::DuplicateData)
            }
            Some(_) => Some(IndexRefusal::DigestReferenceConflict),
            None if self.count < capacity => None,
            None => Some(IndexRefusal::RecordCountOverflow),
        }
    }

    fn insert_all(&self, capacity: usize, records: &[IndexRecord]) -> IndexResult {
        let mut index = CommittedIndex {
            entries: Vec::with_capacity(self.entries.len() + records.len()),
            count: self.count,
        };
        index.entries.extend_from_slice(&self.entries);
        for record in records {
            if let Some(why) = index.validate_insert(capacity, record) {
                return IndexResult::Refused(*record, why);
            }
            index.entries.push(*record);
            index.count += 1;
        }
        IndexResult::Built(index)
    }
}

fn cold_rebuild(capacity: usize, records: &[IndexRecord]) -> IndexResult {
    CommittedIndex::default().insert_all(capacity, records)
}

fn index_tail(count: usize) -> u64 {
    INDEX_HEADER_LEN +
        INDEX_RECORD_LEN * u64::try_from(count).expect("a generated index count fits u64")
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct AuthorityCore {
    pub(super) entries: RetentionEntries,
    pub(super) metadata_roots: Vec<usize>,
    pub(super) ledger: SharedLedger,
    retained_states: u64,
    maxima: (u64, u64),
    pub(super) shape: ShapeSummary,
    validity: usize,
    pub(super) profile: usize,
    pub(super) index_records: Vec<IndexRecord>,
    index: CommittedIndex,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct SealedReceipt {
    base: usize,
    target: usize,
    profile: usize,
    generation: usize,
    metadata: RootDelta,
    shape: ShapeSummary,
    index_plan: Vec<IndexRecord>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct PreparedTrustedSuccessor {
    entries: RetentionEntries,
    metadata_roots: Vec<usize>,
    ledger: SharedLedger,
    retained_states: u64,
    maxima: (u64, u64),
    shape: ShapeSummary,
    validity: usize,
    index_plan: Vec<IndexRecord>,
    policy: TrustedPolicy,
}

fn trusted_delta(core: &AuthorityCore, receipt: &SealedReceipt) -> ComposedDelta {
    ComposedDelta {
        current: RootDelta {
            added: vec![receipt.target],
            removed: vec![core.entries.current.manifest],
        },
        retained: RootDelta {
            added: vec![receipt.target],
            removed: Vec::new(),
        },
        metadata: receipt.metadata.clone(),
        pins: RootDelta::default(),
    }
}

fn pair_max(left: (u64, u64), right: (u64, u64)) -> (u64, u64) {
    (left.0.max(right.0), left.1.max(right.1))
}

fn final_inventory(evidence: &PreparedEvidence) -> (u64, u64) { (evidence.objects, evidence.bytes) }

fn successor_values(
    core: &AuthorityCore,
    receipt: &SealedReceipt,
    evidence: &ComposedEvidence,
) -> SuccessorValues {
    SuccessorValues {
        maxima: pair_max(core.maxima, final_inventory(&evidence.current)),
        retained_states: core
            .retained_states
            .checked_add(1)
            .expect("a generated retention history counts its states in u64"),
        shape: receipt.shape,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum BindRefusal {
    ForeignBase,
    ProfileMismatch,
    TrialRefusedAt(Domain, TrialRefusal),
    CeilingRefused(CeilingAxis),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum BindResult {
    Prepared(Box<PreparedTrustedSuccessor>),
    Refused(BindRefusal),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct ColdDemand {
    objects: u64,
    bytes: u64,
    references: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ColdAxis {
    Objects,
    Bytes,
    References,
}

const COLD_AXES: [ColdAxis; 3] = [ColdAxis::Objects, ColdAxis::Bytes, ColdAxis::References];

impl ColdDemand {
    pub(super) const LOOSE: Self = Self {
        objects: u64::MAX,
        bytes: u64::MAX,
        references: u64::MAX,
    };

    fn within(&self, limits: &ColdDemand) -> bool {
        self.objects <= limits.objects &&
            self.bytes <= limits.bytes &&
            self.references <= limits.references
    }

    fn get(&self, axis: ColdAxis) -> u64 {
        match axis {
            ColdAxis::Objects => self.objects,
            ColdAxis::Bytes => self.bytes,
            ColdAxis::References => self.references,
        }
    }

    fn with(mut self, axis: ColdAxis, value: u64) -> Self {
        match axis {
            ColdAxis::Objects => self.objects = value,
            ColdAxis::Bytes => self.bytes = value,
            ColdAxis::References => self.references = value,
        }
        self
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum SealedBinding {
    ColdChecked(ColdDemand),
    Trusted(Box<PreparedTrustedSuccessor>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PreflightRefusal {
    PublicationModeMismatch,
    TrustedPolicyMismatch,
    AdmissionPolicyMismatch,
}

fn preflight_trusted(
    variant: TrustedVariant,
    binding: &SealedBinding,
    policy: &TrustedPolicy,
) -> Option<PreflightRefusal> {
    match (binding, variant) {
        (SealedBinding::ColdChecked(_), _) => Some(PreflightRefusal::PublicationModeMismatch),
        (SealedBinding::Trusted(_), TrustedVariant::IgnoreSealedPolicy) => None,
        (SealedBinding::Trusted(prepared), _) if prepared.policy == *policy => None,
        (SealedBinding::Trusted(_), _) => Some(PreflightRefusal::TrustedPolicyMismatch),
    }
}

fn preflight_cold(binding: &SealedBinding, limits: &ColdDemand) -> Option<PreflightRefusal> {
    match binding {
        SealedBinding::Trusted(_) => Some(PreflightRefusal::PublicationModeMismatch),
        SealedBinding::ColdChecked(sealed) if sealed == limits => None,
        SealedBinding::ColdChecked(_) => Some(PreflightRefusal::AdmissionPolicyMismatch),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PublicationFault {
    NoFault,
    PinFault,
    CatalogMismatch,
    CatalogInterrupted,
    DirectorySyncFault,
    SidecarFault,
}

pub(super) const PUBLICATION_FAULTS: [PublicationFault; 6] = [
    PublicationFault::NoFault,
    PublicationFault::PinFault,
    PublicationFault::CatalogMismatch,
    PublicationFault::CatalogInterrupted,
    PublicationFault::DirectorySyncFault,
    PublicationFault::SidecarFault,
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum NotPublishedStage {
    Bind(BindRefusal),
    Preflight(PreflightRefusal),
    Assembly(IndexRecord, IndexRefusal),
    Pin,
    Catalog,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PublishedFailure {
    DirectorySyncFailed,
    SidecarResolutionFailed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum PublicationOutcome {
    TrustedReady(Box<AuthorityCore>),
    DefinitelyNotPublished(NotPublishedStage),
    PublishedFailure(PublishedFailure),
    CommitIndeterminate,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum ColdPublicationOutcome {
    ColdReady {
        core: Box<AuthorityCore>,
        evidence: ColdDemand,
    },
    DefinitelyNotPublished(NotPublishedStage),
    PublishedRecoveryRefused(IndexRecord, IndexRefusal),
    PublishedAdmissionRefused(ColdDemand),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SidecarState {
    Absent,
    Deferred,
    Reclaimable,
}

pub(super) const SIDECAR_STATES: [SidecarState; 3] =
    [SidecarState::Absent, SidecarState::Deferred, SidecarState::Reclaimable];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct SidecarResolution {
    pub(super) intent_kept: bool,
    pub(super) superseded_files_removed: bool,
}

pub(super) fn resolve_sidecar(state: SidecarState) -> SidecarResolution {
    match state {
        SidecarState::Absent => SidecarResolution {
            intent_kept: false,
            superseded_files_removed: false,
        },
        SidecarState::Deferred => SidecarResolution {
            intent_kept: true,
            superseded_files_removed: false,
        },
        SidecarState::Reclaimable => SidecarResolution {
            intent_kept: false,
            superseded_files_removed: true,
        },
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum PublicationEvent {
    ReceiptChecked,
    TrialRan(Domain),
    CeilingsChecked,
    PolicyPreflight,
    IndexAssembled,
    PackAppended(Vec<IndexRecord>),
    StorePinned,
    CatalogRefusedSwap,
    CatalogSwapped,
    CatalogDirectorySynced,
    SidecarResolved(SidecarResolution),
    PhysicalRecovery,
    ColdAdmission,
}

impl PublicationEvent {
    fn mutates(&self) -> bool {
        matches!(self, Self::PackAppended(_) | Self::CatalogSwapped | Self::SidecarResolved(_))
    }
}

fn bind_checks() -> Vec<PublicationEvent> {
    vec![
        PublicationEvent::ReceiptChecked,
        PublicationEvent::TrialRan(Domain::Current),
        PublicationEvent::TrialRan(Domain::Retained),
        PublicationEvent::TrialRan(Domain::Metadata),
        PublicationEvent::CeilingsChecked,
    ]
}

fn trials_through(refusing: Domain) -> Vec<PublicationEvent> {
    DOMAINS
        .into_iter()
        .take(refusing as usize + 1)
        .map(PublicationEvent::TrialRan)
        .collect()
}

fn bind_trace(result: &BindResult) -> Vec<PublicationEvent> {
    match result {
        BindResult::Refused(BindRefusal::ForeignBase | BindRefusal::ProfileMismatch) => {
            vec![PublicationEvent::ReceiptChecked]
        }
        BindResult::Refused(BindRefusal::TrialRefusedAt(refusing, _)) => {
            [vec![PublicationEvent::ReceiptChecked], trials_through(*refusing)].concat()
        }
        BindResult::Refused(BindRefusal::CeilingRefused(_)) | BindResult::Prepared(_) => {
            bind_checks()
        }
    }
}

fn durable_suffix(fault: PublicationFault, sidecar: SidecarResolution) -> Vec<PublicationEvent> {
    match fault {
        PublicationFault::NoFault => vec![
            PublicationEvent::StorePinned,
            PublicationEvent::CatalogSwapped,
            PublicationEvent::CatalogDirectorySynced,
            PublicationEvent::SidecarResolved(sidecar),
        ],
        PublicationFault::PinFault => Vec::new(),
        PublicationFault::CatalogMismatch => {
            vec![PublicationEvent::StorePinned, PublicationEvent::CatalogRefusedSwap]
        }
        PublicationFault::CatalogInterrupted | PublicationFault::DirectorySyncFault => {
            vec![PublicationEvent::StorePinned, PublicationEvent::CatalogSwapped]
        }
        PublicationFault::SidecarFault => vec![
            PublicationEvent::StorePinned,
            PublicationEvent::CatalogSwapped,
            PublicationEvent::CatalogDirectorySynced,
        ],
    }
}

fn assembled_core(
    core: &AuthorityCore,
    prepared: &PreparedTrustedSuccessor,
    index: CommittedIndex,
) -> AuthorityCore {
    AuthorityCore {
        entries: prepared.entries.clone(),
        metadata_roots: prepared.metadata_roots.clone(),
        ledger: prepared.ledger.clone(),
        retained_states: prepared.retained_states,
        maxima: prepared.maxima,
        shape: prepared.shape,
        validity: prepared.validity,
        profile: core.profile,
        index_records: [core.index_records.as_slice(), &prepared.index_plan].concat(),
        index,
    }
}

pub(super) fn fault_outcome(
    fault: PublicationFault,
    successor: AuthorityCore,
) -> PublicationOutcome {
    match fault {
        PublicationFault::NoFault => PublicationOutcome::TrustedReady(Box::new(successor)),
        PublicationFault::PinFault => {
            PublicationOutcome::DefinitelyNotPublished(NotPublishedStage::Pin)
        }
        PublicationFault::CatalogMismatch => {
            PublicationOutcome::DefinitelyNotPublished(NotPublishedStage::Catalog)
        }
        PublicationFault::CatalogInterrupted => PublicationOutcome::CommitIndeterminate,
        PublicationFault::DirectorySyncFault => {
            PublicationOutcome::PublishedFailure(PublishedFailure::DirectorySyncFailed)
        }
        PublicationFault::SidecarFault => {
            PublicationOutcome::PublishedFailure(PublishedFailure::SidecarResolutionFailed)
        }
    }
}

fn early_events(variant: TrustedVariant, plan: &[IndexRecord]) -> Vec<PublicationEvent> {
    match variant {
        TrustedVariant::PublishBeforeTrials => vec![
            PublicationEvent::PackAppended(plan.to_vec()),
            PublicationEvent::StorePinned,
            PublicationEvent::CatalogSwapped,
        ],
        TrustedVariant::Faithful |
        TrustedVariant::SkipRetainedTrial |
        TrustedVariant::SkipMetadataTrial |
        TrustedVariant::IgnoreSealedPolicy => Vec::new(),
    }
}

fn refused_before_mutation(outcome: &PublicationOutcome) -> bool {
    matches!(
        outcome,
        PublicationOutcome::DefinitelyNotPublished(
            NotPublishedStage::Bind(_) |
                NotPublishedStage::Preflight(_) |
                NotPublishedStage::Assembly(..)
        )
    )
}

fn trusted_safe_prefix() -> Vec<PublicationEvent> {
    [bind_checks(), vec![PublicationEvent::PolicyPreflight, PublicationEvent::IndexAssembled]]
        .concat()
}

#[derive(Clone, Debug)]
pub(super) struct TrustedWorld {
    pub(super) env: ComposedEnvironment,
    schedule: Vec<bool>,
    capacity: usize,
    sidecar: SidecarState,
}

impl TrustedWorld {
    fn environment(&self, domain: Domain) -> &TrialEnvironment {
        match domain {
            Domain::Current | Domain::Retained => &self.env.semantic,
            Domain::Metadata => &self.env.metadata,
        }
    }

    fn domain_trial(
        &self,
        budget: &ComposedLimits,
        shared: &SharedLedger,
        delta: &ComposedDelta,
        domain: Domain,
    ) -> TrialResult {
        let change = match domain {
            Domain::Current => &delta.current,
            Domain::Retained => &delta.retained,
            Domain::Metadata => &delta.metadata,
        };
        TrialCase {
            env: self.environment(domain).clone(),
            base: shared.project(domain).clone(),
            added: change.added.clone(),
            removed: change.removed.clone(),
            schedule: self.schedule.clone(),
        }
        .run(TrialVariant::Faithful, budget.get(domain))
    }

    fn production_order(
        &self,
        variant: TrustedVariant,
        budget: &ComposedLimits,
        shared: &SharedLedger,
        delta: &ComposedDelta,
    ) -> ProductionResult {
        let current = match self
            .domain_trial(budget, shared, delta, Domain::Current)
            .outcome
        {
            TrialOutcome::Prepared(evidence) => evidence,
            TrialOutcome::Refused(why) => return ProductionResult::Refused(Domain::Current, why),
        };
        let after_current = shared.write_domain(Domain::Current, &current);
        let retained = match variant {
            TrustedVariant::SkipRetainedTrial => {
                skipped_trial(after_current.project(Domain::Retained))
            }
            TrustedVariant::Faithful |
            TrustedVariant::SkipMetadataTrial |
            TrustedVariant::IgnoreSealedPolicy |
            TrustedVariant::PublishBeforeTrials => {
                self.domain_trial(budget, &after_current, delta, Domain::Retained)
            }
        };
        let retained = match retained.outcome {
            TrialOutcome::Prepared(evidence) => evidence,
            TrialOutcome::Refused(why) => return ProductionResult::Refused(Domain::Retained, why),
        };
        let after_semantic = after_current.write_domain(Domain::Retained, &retained);
        let metadata = match variant {
            TrustedVariant::SkipMetadataTrial => {
                skipped_trial(after_semantic.project(Domain::Metadata))
            }
            TrustedVariant::Faithful |
            TrustedVariant::SkipRetainedTrial |
            TrustedVariant::IgnoreSealedPolicy |
            TrustedVariant::PublishBeforeTrials => {
                self.domain_trial(budget, &after_semantic, delta, Domain::Metadata)
            }
        };
        let metadata = match metadata.outcome {
            TrialOutcome::Prepared(evidence) => evidence,
            TrialOutcome::Refused(why) => return ProductionResult::Refused(Domain::Metadata, why),
        };
        let evidence = ComposedEvidence {
            current,
            retained,
            metadata,
        };
        ProductionResult::Prepared(shared.commit(&evidence), evidence)
    }

    fn bind(
        &self,
        variant: TrustedVariant,
        core: &AuthorityCore,
        receipt: &SealedReceipt,
        policy: &TrustedPolicy,
    ) -> BindResult {
        match (receipt.base == core.validity, receipt.profile == core.profile) {
            (false, _) => return BindResult::Refused(BindRefusal::ForeignBase),
            (true, false) => return BindResult::Refused(BindRefusal::ProfileMismatch),
            (true, true) => {}
        }
        let (successor, evidence) = match self.production_order(
            variant,
            &policy_budget(policy),
            &core.ledger,
            &trusted_delta(core, receipt),
        ) {
            ProductionResult::Prepared(successor, evidence) => (successor, evidence),
            ProductionResult::Refused(refusing, why) => {
                return BindResult::Refused(BindRefusal::TrialRefusedAt(refusing, why));
            }
        };
        let values = successor_values(core, receipt, &evidence);
        if let Some(axis) = first_ceiling_exceeded(policy, &values) {
            return BindResult::Refused(BindRefusal::CeilingRefused(axis));
        }
        BindResult::Prepared(Box::new(PreparedTrustedSuccessor {
            entries: core.entries.commit(RetentionEntry {
                generation: receipt.generation,
                manifest: receipt.target,
            }),
            metadata_roots: apply_delta(&core.metadata_roots, &receipt.metadata)
                .expect("a generated receipt removes only present metadata roots"),
            ledger: successor,
            retained_states: values.retained_states,
            maxima: values.maxima,
            shape: receipt.shape,
            validity: receipt.target,
            index_plan: receipt.index_plan.clone(),
            policy: *policy,
        }))
    }

    fn publish_sealed(
        &self,
        variant: TrustedVariant,
        core: &AuthorityCore,
        binding: &SealedBinding,
        policy: &TrustedPolicy,
        fault: PublicationFault,
    ) -> (PublicationOutcome, Vec<PublicationEvent>) {
        if let Some(why) = preflight_trusted(variant, binding, policy) {
            return (
                PublicationOutcome::DefinitelyNotPublished(NotPublishedStage::Preflight(why)),
                vec![PublicationEvent::PolicyPreflight],
            );
        }
        let prepared = match binding {
            SealedBinding::ColdChecked(_) => {
                return (
                    PublicationOutcome::DefinitelyNotPublished(NotPublishedStage::Preflight(
                        PreflightRefusal::PublicationModeMismatch,
                    )),
                    vec![PublicationEvent::PolicyPreflight],
                );
            }
            SealedBinding::Trusted(prepared) => prepared,
        };
        match core.index.insert_all(self.capacity, &prepared.index_plan) {
            IndexResult::Refused(record, why) => (
                PublicationOutcome::DefinitelyNotPublished(NotPublishedStage::Assembly(
                    record, why,
                )),
                vec![PublicationEvent::PolicyPreflight, PublicationEvent::IndexAssembled],
            ),
            IndexResult::Built(index) => {
                let mut trace = vec![
                    PublicationEvent::PolicyPreflight,
                    PublicationEvent::IndexAssembled,
                    PublicationEvent::PackAppended(prepared.index_plan.clone()),
                ];
                trace.extend(durable_suffix(fault, resolve_sidecar(self.sidecar)));
                (fault_outcome(fault, assembled_core(core, prepared, index)), trace)
            }
        }
    }

    pub(super) fn publish(
        &self,
        variant: TrustedVariant,
        core: &AuthorityCore,
        receipt: &SealedReceipt,
        bind_policy: &TrustedPolicy,
        publication_policy: &TrustedPolicy,
        fault: PublicationFault,
    ) -> (PublicationOutcome, Vec<PublicationEvent>) {
        let mut trace = early_events(variant, &receipt.index_plan);
        match self.bind(variant, core, receipt, bind_policy) {
            BindResult::Refused(why) => {
                trace.extend(bind_trace(&BindResult::Refused(why)));
                (PublicationOutcome::DefinitelyNotPublished(NotPublishedStage::Bind(why)), trace)
            }
            BindResult::Prepared(prepared) => {
                trace.extend(bind_checks());
                let (outcome, sealed) = self.publish_sealed(
                    variant,
                    core,
                    &SealedBinding::Trusted(prepared),
                    publication_policy,
                    fault,
                );
                trace.extend(sealed);
                (outcome, trace)
            }
        }
    }

    fn publish_cold_checked(
        &self,
        core: &AuthorityCore,
        receipt: &SealedReceipt,
        sealed_policy: &TrustedPolicy,
        admission: &ColdDemand,
    ) -> (ColdPublicationOutcome, Vec<PublicationEvent>) {
        let prepared = match self.bind(TrustedVariant::Faithful, core, receipt, sealed_policy) {
            BindResult::Refused(why) => {
                return (
                    ColdPublicationOutcome::DefinitelyNotPublished(NotPublishedStage::Bind(why)),
                    bind_trace(&BindResult::Refused(why)),
                );
            }
            BindResult::Prepared(prepared) => prepared,
        };
        let mut trace = bind_checks();
        trace.push(PublicationEvent::PolicyPreflight);
        if let Some(why) = preflight_cold(&SealedBinding::ColdChecked(*admission), admission) {
            return (
                ColdPublicationOutcome::DefinitelyNotPublished(NotPublishedStage::Preflight(why)),
                trace,
            );
        }
        trace.extend([
            PublicationEvent::PackAppended(prepared.index_plan.clone()),
            PublicationEvent::CatalogSwapped,
            PublicationEvent::CatalogDirectorySynced,
            PublicationEvent::PhysicalRecovery,
            PublicationEvent::SidecarResolved(resolve_sidecar(self.sidecar)),
        ]);
        let recovered = match cold_admitted_core(
            self,
            prepared.entries.clone(),
            prepared.metadata_roots.clone(),
            prepared.shape,
            core.profile,
            [core.index_records.as_slice(), &prepared.index_plan].concat(),
        ) {
            Ok(recovered) => recovered,
            Err((record, why)) => {
                return (ColdPublicationOutcome::PublishedRecoveryRefused(record, why), trace);
            }
        };
        trace.push(PublicationEvent::ColdAdmission);
        let demand = measure_cold_demand(
            &self.env.semantic,
            &recovered.ledger.project(Domain::Current).ledger,
        );
        if demand.within(admission) {
            (
                ColdPublicationOutcome::ColdReady {
                    core: Box::new(recovered),
                    evidence: demand,
                },
                trace,
            )
        } else {
            (ColdPublicationOutcome::PublishedAdmissionRefused(demand), trace)
        }
    }
}

fn cold_counts_predecessor(env: &TrialEnvironment, roots: &[usize]) -> Predecessor {
    let ledger = cold_graph_oracle(&env.graph, roots);
    let (objects, bytes) = env.inventory(&ledger);
    Predecessor {
        ledger,
        roots: roots.to_vec(),
        objects,
        bytes,
    }
}

fn cold_state_inventories(env: &TrialEnvironment, entries: &RetentionEntries) -> Vec<(u64, u64)> {
    entries
        .manifests()
        .into_iter()
        .map(|manifest| {
            let state = cold_predecessor(env, &[manifest]);
            (state.objects, state.bytes)
        })
        .collect()
}

fn maxima_of(values: &[(u64, u64)]) -> (u64, u64) {
    values
        .iter()
        .fold((0, 0), |maxima, &value| pair_max(maxima, value))
}

fn per_state_preflight(values: &[(u64, u64)], objects_limit: u64, bytes_limit: u64) -> bool {
    values
        .iter()
        .all(|&(objects, bytes)| objects <= objects_limit && bytes <= bytes_limit)
}

pub(super) fn cold_admitted_core(
    world: &TrustedWorld,
    entries: RetentionEntries,
    metadata_roots: Vec<usize>,
    shape: ShapeSummary,
    profile: usize,
    index_records: Vec<IndexRecord>,
) -> Result<AuthorityCore, (IndexRecord, IndexRefusal)> {
    let index = match cold_rebuild(world.capacity, &index_records) {
        IndexResult::Built(index) => index,
        IndexResult::Refused(record, why) => return Err((record, why)),
    };
    let env = &world.env;
    let ledger = SharedLedger([
        cold_predecessor(&env.semantic, &[entries.current.manifest]),
        cold_predecessor(&env.semantic, &entries.manifests()),
        cold_counts_predecessor(&env.metadata, &metadata_roots),
    ]);
    Ok(AuthorityCore {
        retained_states: u64::try_from(entries.manifests().len())
            .expect("a generated retention history counts its states in u64"),
        maxima: maxima_of(&cold_state_inventories(&env.semantic, &entries)),
        validity: entries.current.manifest,
        entries,
        metadata_roots,
        ledger,
        shape,
        profile,
        index_records,
        index,
    })
}

fn state_references(env: &TrialEnvironment, ledger: &Ledger) -> u64 {
    ledger
        .active
        .iter()
        .zip(&env.graph)
        .filter(|(active, _)| **active)
        .map(|(_, children)| u64::try_from(children.len()).expect("a child list length fits u64"))
        .sum()
}

pub(super) fn measure_cold_demand(env: &TrialEnvironment, ledger: &Ledger) -> ColdDemand {
    let (objects, bytes) = env.inventory(ledger);
    ColdDemand {
        objects,
        bytes,
        references: state_references(env, ledger),
    }
}

fn summary_demand(objects: u64, bytes: u64) -> ColdDemand {
    ColdDemand {
        objects,
        bytes,
        references: objects.saturating_sub(1),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ColdPreflightVariant {
    MeasureColdDemand,
    ReusePredecessorDemand,
    TrustedSummaryDemand,
}

fn fresh_checkpoint_preflight(
    variant: ColdPreflightVariant,
    env: &TrialEnvironment,
    predecessor_demand: &ColdDemand,
    state: &Ledger,
    limits: &ColdDemand,
) -> bool {
    match variant {
        ColdPreflightVariant::MeasureColdDemand => measure_cold_demand(env, state).within(limits),
        ColdPreflightVariant::ReusePredecessorDemand => predecessor_demand.within(limits),
        ColdPreflightVariant::TrustedSummaryDemand => {
            let (objects, bytes) = env.inventory(state);
            summary_demand(objects, bytes).within(limits)
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum OwnerPhase {
    Ready,
    AwaitingAck(usize),
    RecoveryRequired,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum OwnerJournal {
    Empty,
    Captured,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct OwnerLimits {
    pub(super) cold: ColdDemand,
    pub(super) trusted: TrustedPolicy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum OwnerEvent {
    NoChange,
    TrustedPublished,
    AcknowledgedOnce(usize),
    ColdRecovered,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum OwnerOutcome {
    Accepted(usize),
    Refused(OwnerPhase),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct OwnerRequest {
    pub(super) current: usize,
    pub(super) pending: OwnerPhase,
    pub(super) journal: OwnerJournal,
    pub(super) limits: OwnerLimits,
    pub(super) recovery_demand: ColdDemand,
    pub(super) recovered: usize,
}

#[derive(Debug, Default)]
pub(super) struct OwnerProbe {
    pub(super) cold_checks: Cell<usize>,
    pub(super) publications: Cell<usize>,
}

impl OwnerProbe {
    fn cold_within(&self, demand: &ColdDemand, limits: &ColdDemand) -> bool {
        self.cold_checks.set(self.cold_checks.get() + 1);
        demand.within(limits)
    }
}

pub(super) fn owner_state(core: &AuthorityCore) -> usize { core.entries.current.generation }

fn owner_capture(
    request: &OwnerRequest,
    probe: &OwnerProbe,
    publish: &dyn Fn(&TrustedPolicy) -> PublicationOutcome,
) -> (OwnerOutcome, Vec<OwnerEvent>) {
    match request.journal {
        OwnerJournal::Empty => {
            (OwnerOutcome::Accepted(request.current), vec![OwnerEvent::NoChange])
        }
        OwnerJournal::Captured => {
            probe.publications.set(probe.publications.get() + 1);
            match publish(&request.limits.trusted) {
                PublicationOutcome::TrustedReady(successor) => {
                    let state = owner_state(&successor);
                    (OwnerOutcome::Accepted(state), vec![
                        OwnerEvent::TrustedPublished,
                        OwnerEvent::AcknowledgedOnce(state),
                    ])
                }
                PublicationOutcome::DefinitelyNotPublished(NotPublishedStage::Bind(_)) => {
                    (OwnerOutcome::Refused(OwnerPhase::Ready), Vec::new())
                }
                PublicationOutcome::DefinitelyNotPublished(
                    NotPublishedStage::Preflight(_) |
                    NotPublishedStage::Assembly(..) |
                    NotPublishedStage::Pin |
                    NotPublishedStage::Catalog,
                ) => (OwnerOutcome::Refused(OwnerPhase::RecoveryRequired), Vec::new()),
                PublicationOutcome::PublishedFailure(_) |
                PublicationOutcome::CommitIndeterminate => {
                    (OwnerOutcome::Refused(OwnerPhase::RecoveryRequired), vec![
                        OwnerEvent::TrustedPublished,
                    ])
                }
            }
        }
    }
}

pub(super) fn owner_reconcile(
    request: &OwnerRequest,
    probe: &OwnerProbe,
) -> (OwnerOutcome, Vec<OwnerEvent>) {
    match request.pending {
        OwnerPhase::Ready => (OwnerOutcome::Accepted(request.current), Vec::new()),
        OwnerPhase::AwaitingAck(successor) => {
            (OwnerOutcome::Accepted(successor), vec![OwnerEvent::AcknowledgedOnce(successor)])
        }
        OwnerPhase::RecoveryRequired => {
            if probe.cold_within(&request.recovery_demand, &request.limits.cold) {
                let mut events = Vec::with_capacity(2);
                events.push(OwnerEvent::ColdRecovered);
                if request.recovered != request.current {
                    events.push(OwnerEvent::AcknowledgedOnce(request.recovered));
                }
                (OwnerOutcome::Accepted(request.recovered), events)
            } else {
                (OwnerOutcome::Refused(OwnerPhase::RecoveryRequired), Vec::new())
            }
        }
    }
}

pub(super) fn owner_checkpoint(
    request: &OwnerRequest,
    probe: &OwnerProbe,
    publish: &dyn Fn(&TrustedPolicy) -> PublicationOutcome,
) -> (OwnerOutcome, Vec<OwnerEvent>) {
    match request.pending {
        OwnerPhase::Ready => owner_capture(request, probe, publish),
        OwnerPhase::AwaitingAck(_) | OwnerPhase::RecoveryRequired => {
            match owner_reconcile(request, probe) {
                (OwnerOutcome::Refused(next), events) => (OwnerOutcome::Refused(next), events),
                (OwnerOutcome::Accepted(state), events) if state == request.current => {
                    let (outcome, captured) = owner_capture(request, probe, publish);
                    (outcome, [events, captured].concat())
                }
                (accepted, events) => (accepted, events),
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum PublicationMode {
    Trusted,
    ColdChecked,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum CatalogImage {
    Old,
    New,
    Torn,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum RunPhase {
    Idle,
    Bound,
    Preflighted,
    IndexAssembled,
    Appended,
    Staged,
    CasVerified,
    SlotWriting,
    CatalogFileDurable,
    DirectoryDurable,
    Resolved,
    ColdRecovered,
    Ready,
    Refused,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum TrialStatus {
    Pending,
    Accepted,
    Refused,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum EvidenceKind {
    Absent,
    Measured,
    Fabricated,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum RunRefusal {
    TrustedPolicy,
    ColdLimits,
    ModeMismatch,
    PolicyMismatch,
    StaleAuthority,
    Assembly,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum RunFailure {
    Io,
    Stage,
    CasMismatch,
    SlotWrite,
    Directory,
    SidecarResolution,
    RereadMismatch,
    ColdRecovery,
    ColdAdmission,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum RunOutcome {
    Pending,
    DefinitelyNotPublished,
    Indeterminate,
    PublishedError,
    Ready,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct PublicationScenario {
    pub(super) sealed_mode: PublicationMode,
    pub(super) call_mode: PublicationMode,
    pub(super) exact_margin: bool,
    pub(super) sealed_call_policy: bool,
    pub(super) assembly_succeeds: bool,
    pub(super) loose_cold_limits: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct PublicationRun {
    pub(super) scenario: PublicationScenario,
    pub(super) phase: RunPhase,
    pub(super) trial: TrialStatus,
    pub(super) checked: bool,
    pub(super) mutated: bool,
    pub(super) slot_write_started: bool,
    pub(super) catalog: CatalogImage,
    pub(super) catalog_file_durable: bool,
    pub(super) directory_synced: bool,
    pub(super) resolved: bool,
    pub(super) recovered: bool,
    pub(super) cold_admitted: bool,
    pub(super) evidence: EvidenceKind,
    pub(super) adopted: Option<CatalogImage>,
    pub(super) refusal: Option<RunRefusal>,
    pub(super) failure: Option<RunFailure>,
    pub(super) outcome: RunOutcome,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RunAction {
    Bind,
    PolicyPreflight,
    AssembleSuccessorIndex,
    AppendCommit,
    Stage,
    CasVerify,
    SlotWrite,
    CompleteCatalogWrite,
    DirSync,
    ResolveSidecar,
    AdoptTrusted,
    ColdRecover,
    ColdAdmit,
    FailBeforeCas,
    FailStage,
    FailCatalogWrite(CatalogImage),
    FailDirSync,
    FailAfterDirectorySync,
    FailResolution,
    ExternalCatalogChange,
    DeferredCeilingBind,
    LateCeilingRefusal,
    AdoptWithFabricatedEvidence,
    PreflightIgnoringPolicy,
    AppendBeforeTrials,
    RecoverBeforeTrustedAdoption,
    AdoptBeforeDirectorySync,
    AdoptWithoutReread,
    ColdReadyWithoutAdmission,
}

pub(super) const BASE_RUN_ACTIONS: [RunAction; 22] = [
    RunAction::Bind,
    RunAction::PolicyPreflight,
    RunAction::AssembleSuccessorIndex,
    RunAction::AppendCommit,
    RunAction::Stage,
    RunAction::CasVerify,
    RunAction::SlotWrite,
    RunAction::CompleteCatalogWrite,
    RunAction::DirSync,
    RunAction::ResolveSidecar,
    RunAction::AdoptTrusted,
    RunAction::ColdRecover,
    RunAction::ColdAdmit,
    RunAction::FailBeforeCas,
    RunAction::FailStage,
    RunAction::FailCatalogWrite(CatalogImage::Old),
    RunAction::FailCatalogWrite(CatalogImage::Torn),
    RunAction::FailCatalogWrite(CatalogImage::New),
    RunAction::FailDirSync,
    RunAction::FailAfterDirectorySync,
    RunAction::FailResolution,
    RunAction::ExternalCatalogChange,
];

const TRUSTED_SUCCESS_ACTIONS: [RunAction; 11] = [
    RunAction::Bind,
    RunAction::PolicyPreflight,
    RunAction::AssembleSuccessorIndex,
    RunAction::AppendCommit,
    RunAction::Stage,
    RunAction::CasVerify,
    RunAction::SlotWrite,
    RunAction::CompleteCatalogWrite,
    RunAction::DirSync,
    RunAction::ResolveSidecar,
    RunAction::AdoptTrusted,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PublicationMutant {
    LateTrustedRefusal,
    FabricatedColdEvidence,
    ChangedPolicy,
    PublicationBeforeTrials,
    TrustedRecovery,
    EarlyAdoption,
    WrongCatalogAdoption,
    SkippedColdAdmission,
}

impl PublicationMutant {
    fn actions(self) -> &'static [RunAction] {
        match self {
            Self::LateTrustedRefusal => {
                &[RunAction::DeferredCeilingBind, RunAction::LateCeilingRefusal]
            }
            Self::FabricatedColdEvidence => &[RunAction::AdoptWithFabricatedEvidence],
            Self::ChangedPolicy => &[RunAction::PreflightIgnoringPolicy],
            Self::PublicationBeforeTrials => &[RunAction::AppendBeforeTrials],
            Self::TrustedRecovery => &[RunAction::RecoverBeforeTrustedAdoption],
            Self::EarlyAdoption => &[RunAction::AdoptBeforeDirectorySync],
            Self::WrongCatalogAdoption => &[RunAction::AdoptWithoutReread],
            Self::SkippedColdAdmission => &[RunAction::ColdReadyWithoutAdmission],
        }
    }
}

impl PublicationRun {
    pub(super) fn initial(scenario: PublicationScenario) -> Self {
        Self {
            scenario,
            phase: RunPhase::Idle,
            trial: TrialStatus::Pending,
            checked: false,
            mutated: false,
            slot_write_started: false,
            catalog: CatalogImage::Old,
            catalog_file_durable: false,
            directory_synced: false,
            resolved: false,
            recovered: false,
            cold_admitted: false,
            evidence: EvidenceKind::Absent,
            adopted: None,
            refusal: None,
            failure: None,
            outcome: RunOutcome::Pending,
        }
    }

    fn refuse(self, kind: RunRefusal) -> Self {
        Self {
            phase: RunPhase::Refused,
            refusal: Some(kind),
            outcome: RunOutcome::DefinitelyNotPublished,
            ..self
        }
    }

    pub(super) fn fail(self, kind: RunFailure, outcome: RunOutcome) -> Self {
        Self {
            phase: RunPhase::Failed,
            failure: Some(kind),
            outcome,
            ..self
        }
    }

    pub(super) fn adopt(self, image: CatalogImage) -> Self {
        Self {
            phase: RunPhase::Ready,
            adopted: Some(image),
            outcome: RunOutcome::Ready,
            ..self
        }
    }

    pub(super) fn step(self, action: RunAction) -> Option<Self> {
        let scenario = self.scenario;
        let trusted = scenario.call_mode == PublicationMode::Trusted;
        let cold = scenario.call_mode == PublicationMode::ColdChecked;
        let sealed_trusted = scenario.sealed_mode == PublicationMode::Trusted;
        let next = match action {
            RunAction::Bind if self.phase == RunPhase::Idle => {
                if scenario.exact_margin {
                    Self {
                        phase: RunPhase::Bound,
                        trial: TrialStatus::Accepted,
                        ..self
                    }
                } else {
                    Self {
                        phase: RunPhase::Refused,
                        trial: TrialStatus::Refused,
                        refusal: Some(if sealed_trusted {
                            RunRefusal::TrustedPolicy
                        } else {
                            RunRefusal::ColdLimits
                        }),
                        outcome: RunOutcome::DefinitelyNotPublished,
                        ..self
                    }
                }
            }
            RunAction::PolicyPreflight if self.phase == RunPhase::Bound => {
                if scenario.call_mode != scenario.sealed_mode {
                    self.refuse(RunRefusal::ModeMismatch)
                } else if trusted && !scenario.sealed_call_policy {
                    self.refuse(RunRefusal::PolicyMismatch)
                } else if self.catalog != CatalogImage::Old {
                    self.refuse(RunRefusal::StaleAuthority)
                } else {
                    Self {
                        phase: RunPhase::Preflighted,
                        checked: true,
                        ..self
                    }
                }
            }
            RunAction::AssembleSuccessorIndex if self.phase == RunPhase::Preflighted && trusted => {
                if scenario.assembly_succeeds {
                    Self {
                        phase: RunPhase::IndexAssembled,
                        ..self
                    }
                } else {
                    self.refuse(RunRefusal::Assembly)
                }
            }
            RunAction::AppendCommit
                if (self.phase == RunPhase::IndexAssembled && trusted) ||
                    (self.phase == RunPhase::Preflighted && cold) =>
            {
                Self {
                    phase: RunPhase::Appended,
                    mutated: true,
                    ..self
                }
            }
            RunAction::Stage if self.phase == RunPhase::Appended && trusted => Self {
                phase: RunPhase::Staged,
                ..self
            },
            RunAction::CasVerify
                if (self.phase == RunPhase::Staged && trusted) ||
                    (self.phase == RunPhase::Appended && cold) =>
            {
                if self.catalog == CatalogImage::Old {
                    Self {
                        phase: RunPhase::CasVerified,
                        ..self
                    }
                } else {
                    self.fail(RunFailure::CasMismatch, RunOutcome::DefinitelyNotPublished)
                }
            }
            RunAction::SlotWrite if self.phase == RunPhase::CasVerified => Self {
                phase: RunPhase::SlotWriting,
                slot_write_started: true,
                ..self
            },
            RunAction::CompleteCatalogWrite if self.phase == RunPhase::SlotWriting => Self {
                phase: RunPhase::CatalogFileDurable,
                catalog: CatalogImage::New,
                catalog_file_durable: true,
                ..self
            },
            RunAction::DirSync if self.phase == RunPhase::CatalogFileDurable => Self {
                phase: RunPhase::DirectoryDurable,
                directory_synced: true,
                ..self
            },
            RunAction::ResolveSidecar if self.phase == RunPhase::DirectoryDurable && trusted => {
                Self {
                    phase: RunPhase::Resolved,
                    resolved: true,
                    ..self
                }
            }
            RunAction::AdoptTrusted if self.phase == RunPhase::Resolved && trusted => {
                if self.catalog == CatalogImage::New {
                    self.adopt(CatalogImage::New)
                } else {
                    self.fail(RunFailure::RereadMismatch, RunOutcome::PublishedError)
                }
            }
            RunAction::ColdRecover if self.phase == RunPhase::DirectoryDurable && cold => {
                if self.catalog == CatalogImage::New {
                    Self {
                        phase: RunPhase::ColdRecovered,
                        recovered: true,
                        resolved: true,
                        ..self
                    }
                } else {
                    self.fail(RunFailure::ColdRecovery, RunOutcome::PublishedError)
                }
            }
            RunAction::ColdAdmit if self.phase == RunPhase::ColdRecovered => {
                if scenario.loose_cold_limits {
                    Self {
                        cold_admitted: true,
                        evidence: EvidenceKind::Measured,
                        ..self.adopt(self.catalog)
                    }
                } else {
                    self.fail(RunFailure::ColdAdmission, RunOutcome::PublishedError)
                }
            }
            RunAction::FailBeforeCas
                if matches!(
                    self.phase,
                    RunPhase::Preflighted |
                        RunPhase::IndexAssembled |
                        RunPhase::Appended |
                        RunPhase::Staged |
                        RunPhase::CasVerified
                ) =>
            {
                self.fail(RunFailure::Io, RunOutcome::DefinitelyNotPublished)
            }
            RunAction::FailStage if self.phase == RunPhase::Appended && trusted => {
                self.fail(RunFailure::Stage, RunOutcome::DefinitelyNotPublished)
            }
            RunAction::FailCatalogWrite(persisted)
                if self.phase == RunPhase::SlotWriting &&
                    matches!(
                        persisted,
                        CatalogImage::Old | CatalogImage::Torn | CatalogImage::New
                    ) =>
            {
                Self {
                    phase: RunPhase::Failed,
                    catalog: persisted,
                    failure: Some(RunFailure::SlotWrite),
                    outcome: RunOutcome::Indeterminate,
                    ..self
                }
            }
            RunAction::FailDirSync if self.phase == RunPhase::CatalogFileDurable => {
                self.fail(RunFailure::Directory, RunOutcome::PublishedError)
            }
            RunAction::FailAfterDirectorySync if self.phase == RunPhase::DirectoryDurable => {
                self.fail(RunFailure::Io, RunOutcome::PublishedError)
            }
            RunAction::FailResolution if self.phase == RunPhase::DirectoryDurable && trusted => {
                self.fail(RunFailure::SidecarResolution, RunOutcome::PublishedError)
            }
            RunAction::ExternalCatalogChange
                if matches!(
                    self.phase,
                    RunPhase::Idle |
                        RunPhase::Bound |
                        RunPhase::Preflighted |
                        RunPhase::IndexAssembled |
                        RunPhase::Appended |
                        RunPhase::Staged |
                        RunPhase::CatalogFileDurable |
                        RunPhase::DirectoryDurable |
                        RunPhase::Resolved
                ) && matches!(self.catalog, CatalogImage::Old | CatalogImage::New) =>
            {
                Self {
                    catalog: CatalogImage::Other,
                    ..self
                }
            }
            RunAction::DeferredCeilingBind
                if self.phase == RunPhase::Idle && sealed_trusted && !scenario.exact_margin =>
            {
                Self {
                    phase: RunPhase::Bound,
                    trial: TrialStatus::Accepted,
                    ..self
                }
            }
            RunAction::LateCeilingRefusal
                if self.phase == RunPhase::Appended && !scenario.exact_margin =>
            {
                self.refuse(RunRefusal::TrustedPolicy)
            }
            RunAction::AdoptWithFabricatedEvidence
                if self.phase == RunPhase::Resolved &&
                    trusted &&
                    self.catalog == CatalogImage::New =>
            {
                Self {
                    evidence: EvidenceKind::Fabricated,
                    ..self.adopt(CatalogImage::New)
                }
            }
            RunAction::PreflightIgnoringPolicy
                if self.phase == RunPhase::Bound &&
                    scenario.call_mode == scenario.sealed_mode &&
                    self.catalog == CatalogImage::Old =>
            {
                Self {
                    phase: RunPhase::Preflighted,
                    checked: true,
                    ..self
                }
            }
            RunAction::AppendBeforeTrials
                if self.phase == RunPhase::Idle && sealed_trusted && trusted =>
            {
                Self {
                    phase: RunPhase::Appended,
                    mutated: true,
                    ..self
                }
            }
            RunAction::RecoverBeforeTrustedAdoption
                if self.phase == RunPhase::Resolved && trusted && !self.recovered =>
            {
                Self {
                    recovered: true,
                    ..self
                }
            }
            RunAction::AdoptBeforeDirectorySync
                if self.phase == RunPhase::CatalogFileDurable &&
                    trusted &&
                    self.catalog == CatalogImage::New =>
            {
                self.adopt(CatalogImage::New)
            }
            RunAction::AdoptWithoutReread if self.phase == RunPhase::Resolved && trusted => {
                self.adopt(CatalogImage::New)
            }
            RunAction::ColdReadyWithoutAdmission if self.phase == RunPhase::ColdRecovered => {
                self.adopt(self.catalog)
            }
            _ => return None,
        };
        Some(next)
    }
}

fn trusted_call(run: &PublicationRun) -> bool { run.scenario.call_mode == PublicationMode::Trusted }

fn trusted_refusal_precedes_all_mutation(run: &PublicationRun) -> bool {
    (run.refusal.is_none() ||
        (!run.mutated &&
            !run.slot_write_started &&
            run.catalog != CatalogImage::New &&
            run.adopted.is_none() &&
            run.outcome == RunOutcome::DefinitelyNotPublished)) &&
        (!run.mutated || (run.trial == TrialStatus::Accepted && run.checked))
}

fn published_policy_is_sealed_policy(run: &PublicationRun) -> bool {
    !(run.mutated || run.outcome == RunOutcome::Ready) ||
        (run.scenario.call_mode == run.scenario.sealed_mode &&
            (!trusted_call(run) || run.scenario.sealed_call_policy))
}

fn trusted_successor_has_no_cold_evidence(run: &PublicationRun) -> bool {
    !trusted_call(run) || run.evidence == EvidenceKind::Absent
}

fn no_physical_recovery_on_trusted_success(run: &PublicationRun) -> bool {
    !(trusted_call(run) && run.outcome == RunOutcome::Ready) ||
        (!run.recovered && !run.cold_admitted)
}

fn cold_mode_success_includes_cold_admission(run: &PublicationRun) -> bool {
    !(!trusted_call(run) && run.outcome == RunOutcome::Ready) ||
        (run.recovered &&
            run.cold_admitted &&
            run.evidence == EvidenceKind::Measured &&
            run.scenario.loose_cold_limits)
}

fn adoption_follows_directory_sync_and_resolution(run: &PublicationRun) -> bool {
    run.adopted.is_none() || (run.catalog_file_durable && run.directory_synced && run.resolved)
}

pub(super) fn adopted_successor_is_exact_catalog(run: &PublicationRun) -> bool {
    run.adopted.is_none() ||
        (run.adopted == Some(CatalogImage::New) && run.catalog == CatalogImage::New)
}

pub(super) fn trusted_outcome_boundary_is_exact(run: &PublicationRun) -> bool {
    let terminal = matches!(run.phase, RunPhase::Ready | RunPhase::Refused | RunPhase::Failed);
    (run.outcome != RunOutcome::DefinitelyNotPublished ||
        (!run.slot_write_started && run.catalog != CatalogImage::New && run.adopted.is_none())) &&
        (run.outcome != RunOutcome::Indeterminate ||
            (run.slot_write_started && !run.catalog_file_durable && run.adopted.is_none())) &&
        (run.outcome != RunOutcome::PublishedError ||
            (run.catalog_file_durable && run.adopted.is_none())) &&
        ((run.outcome == RunOutcome::Ready) == run.adopted.is_some()) &&
        ((run.outcome == RunOutcome::Pending) == !terminal)
}

fn never_trusted_ready(run: &PublicationRun) -> bool {
    !(trusted_call(run) && run.outcome == RunOutcome::Ready && !run.scenario.loose_cold_limits)
}

fn never_cold_ready(run: &PublicationRun) -> bool {
    !(!trusted_call(run) && run.outcome == RunOutcome::Ready)
}

fn never_trusted_refusal(run: &PublicationRun) -> bool {
    run.refusal != Some(RunRefusal::TrustedPolicy)
}

fn never_published_resolution_failure(run: &PublicationRun) -> bool {
    !(run.outcome == RunOutcome::PublishedError &&
        run.failure == Some(RunFailure::SidecarResolution))
}

fn never_stage_failure_before_cas(run: &PublicationRun) -> bool {
    !(run.outcome == RunOutcome::DefinitelyNotPublished && run.failure == Some(RunFailure::Stage))
}

fn never_indeterminate(run: &PublicationRun) -> bool { run.outcome != RunOutcome::Indeterminate }

type RunPredicate = fn(&PublicationRun) -> bool;

const PUBLICATION_INVARIANTS: [RunPredicate; 8] = [
    trusted_refusal_precedes_all_mutation,
    published_policy_is_sealed_policy,
    trusted_successor_has_no_cold_evidence,
    no_physical_recovery_on_trusted_success,
    cold_mode_success_includes_cold_admission,
    adoption_follows_directory_sync_and_resolution,
    adopted_successor_is_exact_catalog,
    trusted_outcome_boundary_is_exact,
];

const BASE_REACHABLE_STATES: usize = 588;

fn publication_scenarios() -> Vec<PublicationScenario> {
    let modes = [PublicationMode::Trusted, PublicationMode::ColdChecked];
    let flags = [true, false];
    let mut scenarios = Vec::with_capacity(64);
    for sealed_mode in modes {
        for call_mode in modes {
            for exact_margin in flags {
                for sealed_call_policy in flags {
                    for assembly_succeeds in flags {
                        for loose_cold_limits in flags {
                            scenarios.push(PublicationScenario {
                                sealed_mode,
                                call_mode,
                                exact_margin,
                                sealed_call_policy,
                                assembly_succeeds,
                                loose_cold_limits,
                            });
                        }
                    }
                }
            }
        }
    }
    scenarios
}

fn explore_publication(mutant: Option<PublicationMutant>) -> Vec<PublicationRun> {
    let mut actions = BASE_RUN_ACTIONS.to_vec();
    if let Some(mutant) = mutant {
        actions.extend_from_slice(mutant.actions());
    }
    let mut seen = HashSet::with_capacity(1024);
    let mut frontier = VecDeque::with_capacity(1024);
    for scenario in publication_scenarios() {
        let run = PublicationRun::initial(scenario);
        if seen.insert(run) {
            frontier.push_back(run);
        }
    }
    let mut reachable = Vec::with_capacity(1024);
    while let Some(run) = frontier.pop_front() {
        reachable.push(run);
        frontier.extend(
            actions
                .iter()
                .filter_map(|&action| run.step(action))
                .filter(|next| seen.insert(*next)),
        );
    }
    reachable
}

fn assert_publication_model(
    invariants: &[RunPredicate],
    witnesses: &[RunPredicate],
    mutants: &[(PublicationMutant, RunPredicate)],
) {
    let reachable = explore_publication(None);
    assert_eq!(reachable.len(), BASE_REACHABLE_STATES, "the test model left the TLA+ state space");
    for (position, invariant) in invariants.iter().enumerate() {
        assert!(
            reachable.iter().all(invariant),
            "cited trusted-publication invariant {position} fails on a reachable state",
        );
    }
    for (position, witness) in witnesses.iter().enumerate() {
        assert!(
            reachable.iter().any(|run| !witness(run)),
            "cited trusted-publication witness {position} is unreachable",
        );
    }
    for (mutant, invariant) in mutants {
        assert!(
            explore_publication(Some(*mutant))
                .iter()
                .any(|run| !invariant(run)),
            "trusted-publication mutant {mutant:?} kept its target invariant",
        );
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum HookPhase {
    AfterPrepareWrite,
    AfterPrepareSync,
    AfterIndexWrite,
    AfterIndexSync,
    AfterCommitWrite,
    AfterCommitSync,
    AfterArtifactDirectorySync,
    BeforeCatalogCompareAndSwap,
    AfterCatalogCasVerifiedBeforeSlotWrite,
    AfterCatalogSlotWrite,
    AfterCatalogFileSyncBeforeDirectorySync,
    AfterCatalogDirectorySync,
    AfterCatalogSyncBeforeRecovery,
    AfterPhysicalRecovery,
    BeforeSemanticAdmission,
    AfterSemanticAdmission,
}

const COLD_HOOK_PHASES: [HookPhase; 16] = [
    HookPhase::AfterPrepareWrite,
    HookPhase::AfterPrepareSync,
    HookPhase::AfterIndexWrite,
    HookPhase::AfterIndexSync,
    HookPhase::AfterCommitWrite,
    HookPhase::AfterCommitSync,
    HookPhase::AfterArtifactDirectorySync,
    HookPhase::BeforeCatalogCompareAndSwap,
    HookPhase::AfterCatalogCasVerifiedBeforeSlotWrite,
    HookPhase::AfterCatalogSlotWrite,
    HookPhase::AfterCatalogFileSyncBeforeDirectorySync,
    HookPhase::AfterCatalogDirectorySync,
    HookPhase::AfterCatalogSyncBeforeRecovery,
    HookPhase::AfterPhysicalRecovery,
    HookPhase::BeforeSemanticAdmission,
    HookPhase::AfterSemanticAdmission,
];

const TRUSTED_HOOK_PHASE_COUNT: usize = 13;

pub(super) fn trusted_hook_phases() -> &'static [HookPhase] {
    &COLD_HOOK_PHASES[..TRUSTED_HOOK_PHASE_COUNT]
}

fn event_hooks(event: &PublicationEvent) -> &'static [HookPhase] {
    match event {
        PublicationEvent::PackAppended(_) => &COLD_HOOK_PHASES[..7],
        PublicationEvent::CatalogRefusedSwap => &COLD_HOOK_PHASES[7..8],
        PublicationEvent::CatalogSwapped => &COLD_HOOK_PHASES[7..11],
        PublicationEvent::CatalogDirectorySynced => &COLD_HOOK_PHASES[11..13],
        PublicationEvent::PhysicalRecovery => &COLD_HOOK_PHASES[13..14],
        PublicationEvent::ColdAdmission => &COLD_HOOK_PHASES[14..16],
        PublicationEvent::ReceiptChecked |
        PublicationEvent::TrialRan(_) |
        PublicationEvent::CeilingsChecked |
        PublicationEvent::PolicyPreflight |
        PublicationEvent::IndexAssembled |
        PublicationEvent::StorePinned |
        PublicationEvent::SidecarResolved(_) => &[],
    }
}

fn hook_trace(trace: &[PublicationEvent]) -> Vec<HookPhase> {
    trace.iter().flat_map(event_hooks).copied().collect()
}

pub(super) fn hook_failure_behaviors(hook: HookPhase) -> Vec<Vec<RunAction>> {
    let through =
        |count: usize, failure: RunAction| [&TRUSTED_SUCCESS_ACTIONS[..count], &[failure]].concat();
    match hook {
        HookPhase::AfterPrepareWrite |
        HookPhase::AfterPrepareSync |
        HookPhase::AfterIndexWrite |
        HookPhase::AfterIndexSync |
        HookPhase::AfterCommitWrite |
        HookPhase::AfterCommitSync |
        HookPhase::AfterArtifactDirectorySync => vec![through(4, RunAction::FailBeforeCas)],
        HookPhase::BeforeCatalogCompareAndSwap => vec![through(5, RunAction::FailBeforeCas)],
        HookPhase::AfterCatalogCasVerifiedBeforeSlotWrite => {
            vec![through(6, RunAction::FailBeforeCas)]
        }
        HookPhase::AfterCatalogSlotWrite => {
            [CatalogImage::Old, CatalogImage::Torn, CatalogImage::New]
                .map(|persisted| through(7, RunAction::FailCatalogWrite(persisted)))
                .to_vec()
        }
        HookPhase::AfterCatalogFileSyncBeforeDirectorySync => {
            vec![through(8, RunAction::FailDirSync)]
        }
        HookPhase::AfterCatalogDirectorySync | HookPhase::AfterCatalogSyncBeforeRecovery => {
            vec![through(9, RunAction::FailAfterDirectorySync)]
        }
        HookPhase::AfterPhysicalRecovery |
        HookPhase::BeforeSemanticAdmission |
        HookPhase::AfterSemanticAdmission => Vec::new(),
    }
}

fn production_failure_class(hook: HookPhase) -> RunOutcome {
    match hook {
        HookPhase::AfterPrepareWrite |
        HookPhase::AfterPrepareSync |
        HookPhase::AfterIndexWrite |
        HookPhase::AfterIndexSync |
        HookPhase::AfterCommitWrite |
        HookPhase::AfterCommitSync |
        HookPhase::AfterArtifactDirectorySync |
        HookPhase::BeforeCatalogCompareAndSwap |
        HookPhase::AfterCatalogCasVerifiedBeforeSlotWrite => RunOutcome::DefinitelyNotPublished,
        HookPhase::AfterCatalogSlotWrite => RunOutcome::Indeterminate,
        HookPhase::AfterCatalogFileSyncBeforeDirectorySync |
        HookPhase::AfterCatalogDirectorySync |
        HookPhase::AfterCatalogSyncBeforeRecovery |
        HookPhase::AfterPhysicalRecovery |
        HookPhase::BeforeSemanticAdmission |
        HookPhase::AfterSemanticAdmission => RunOutcome::PublishedError,
    }
}

fn replay(scenario: PublicationScenario, actions: &[RunAction]) -> Option<PublicationRun> {
    actions
        .iter()
        .try_fold(PublicationRun::initial(scenario), |run, &action| run.step(action))
}

fn trusted_scenario(
    exact_margin: bool,
    sealed_call_policy: bool,
    assembly_succeeds: bool,
) -> PublicationScenario {
    PublicationScenario {
        sealed_mode: PublicationMode::Trusted,
        call_mode: PublicationMode::Trusted,
        exact_margin,
        sealed_call_policy,
        assembly_succeeds,
        loose_cold_limits: true,
    }
}

fn outcome_class(outcome: &PublicationOutcome) -> RunOutcome {
    match outcome {
        PublicationOutcome::TrustedReady(_) => RunOutcome::Ready,
        PublicationOutcome::DefinitelyNotPublished(_) => RunOutcome::DefinitelyNotPublished,
        PublicationOutcome::PublishedFailure(_) => RunOutcome::PublishedError,
        PublicationOutcome::CommitIndeterminate => RunOutcome::Indeterminate,
    }
}

fn event_actions(
    event: &PublicationEvent,
    outcome: &PublicationOutcome,
    persisted: CatalogImage,
) -> Vec<RunAction> {
    match event {
        PublicationEvent::ReceiptChecked |
        PublicationEvent::TrialRan(_) |
        PublicationEvent::CeilingsChecked |
        PublicationEvent::PhysicalRecovery |
        PublicationEvent::ColdAdmission => Vec::new(),
        PublicationEvent::PolicyPreflight => vec![RunAction::PolicyPreflight],
        PublicationEvent::IndexAssembled => vec![RunAction::AssembleSuccessorIndex],
        PublicationEvent::PackAppended(_) => vec![RunAction::AppendCommit],
        PublicationEvent::StorePinned => vec![RunAction::Stage],
        PublicationEvent::CatalogRefusedSwap => {
            vec![RunAction::ExternalCatalogChange, RunAction::CasVerify]
        }
        PublicationEvent::CatalogSwapped => match outcome {
            PublicationOutcome::CommitIndeterminate => vec![
                RunAction::CasVerify,
                RunAction::SlotWrite,
                RunAction::FailCatalogWrite(persisted),
            ],
            PublicationOutcome::TrustedReady(_) |
            PublicationOutcome::DefinitelyNotPublished(_) |
            PublicationOutcome::PublishedFailure(_) => {
                vec![RunAction::CasVerify, RunAction::SlotWrite, RunAction::CompleteCatalogWrite]
            }
        },
        PublicationEvent::CatalogDirectorySynced => vec![RunAction::DirSync],
        PublicationEvent::SidecarResolved(_) => vec![RunAction::ResolveSidecar],
    }
}

pub(super) fn tla_behavior(
    trace: &[PublicationEvent],
    outcome: &PublicationOutcome,
    persisted: CatalogImage,
) -> Option<Vec<RunAction>> {
    let binds = |event: &PublicationEvent| {
        matches!(
            event,
            PublicationEvent::ReceiptChecked |
                PublicationEvent::TrialRan(_) |
                PublicationEvent::CeilingsChecked
        )
    };
    let bind_events = trace.iter().take_while(|event| binds(event)).count();
    let mut actions = Vec::with_capacity(trace.len() + 2);
    if bind_events > 0 {
        actions.push(RunAction::Bind);
    }
    for event in &trace[bind_events..] {
        if binds(event) ||
            matches!(event, PublicationEvent::PhysicalRecovery | PublicationEvent::ColdAdmission)
        {
            return None;
        }
        actions.extend(event_actions(event, outcome, persisted));
    }
    match outcome {
        PublicationOutcome::TrustedReady(_) => actions.push(RunAction::AdoptTrusted),
        PublicationOutcome::DefinitelyNotPublished(NotPublishedStage::Pin) => {
            actions.push(RunAction::FailStage)
        }
        PublicationOutcome::PublishedFailure(PublishedFailure::DirectorySyncFailed) => {
            actions.push(RunAction::FailDirSync)
        }
        PublicationOutcome::PublishedFailure(PublishedFailure::SidecarResolutionFailed) => {
            actions.push(RunAction::FailResolution)
        }
        PublicationOutcome::DefinitelyNotPublished(NotPublishedStage::Bind(
            BindRefusal::ForeignBase | BindRefusal::ProfileMismatch,
        )) => return None,
        PublicationOutcome::DefinitelyNotPublished(
            NotPublishedStage::Bind(_) |
            NotPublishedStage::Preflight(_) |
            NotPublishedStage::Assembly(..) |
            NotPublishedStage::Catalog,
        ) |
        PublicationOutcome::CommitIndeterminate => {}
    }
    Some(actions)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PackEntry {
    Prepare(usize),
    Record(IndexRecord),
    Commit(usize),
    TornTail,
}

pub(super) fn transaction_entries(receipt: &SealedReceipt) -> Vec<PackEntry> {
    let mut entries = Vec::with_capacity(receipt.index_plan.len() + 2);
    entries.push(PackEntry::Prepare(receipt.generation));
    entries.extend(receipt.index_plan.iter().copied().map(PackEntry::Record));
    entries.push(PackEntry::Commit(receipt.generation));
    entries
}

fn durable_window(hook: HookPhase, transaction_len: usize) -> (usize, usize) {
    match hook {
        HookPhase::AfterPrepareWrite => (0, 1),
        HookPhase::AfterPrepareSync => (1, 1),
        HookPhase::AfterIndexWrite => (1, transaction_len - 1),
        HookPhase::AfterIndexSync => (transaction_len - 1, transaction_len - 1),
        HookPhase::AfterCommitWrite => (transaction_len - 1, transaction_len),
        HookPhase::AfterCommitSync |
        HookPhase::AfterArtifactDirectorySync |
        HookPhase::BeforeCatalogCompareAndSwap |
        HookPhase::AfterCatalogCasVerifiedBeforeSlotWrite |
        HookPhase::AfterCatalogSlotWrite |
        HookPhase::AfterCatalogFileSyncBeforeDirectorySync |
        HookPhase::AfterCatalogDirectorySync |
        HookPhase::AfterCatalogSyncBeforeRecovery |
        HookPhase::AfterPhysicalRecovery |
        HookPhase::BeforeSemanticAdmission |
        HookPhase::AfterSemanticAdmission => (transaction_len, transaction_len),
    }
}

pub(super) fn catalog_images(hook: HookPhase) -> &'static [CatalogImage] {
    match hook {
        HookPhase::AfterPrepareWrite |
        HookPhase::AfterPrepareSync |
        HookPhase::AfterIndexWrite |
        HookPhase::AfterIndexSync |
        HookPhase::AfterCommitWrite |
        HookPhase::AfterCommitSync |
        HookPhase::AfterArtifactDirectorySync |
        HookPhase::BeforeCatalogCompareAndSwap |
        HookPhase::AfterCatalogCasVerifiedBeforeSlotWrite => &[CatalogImage::Old],
        HookPhase::AfterCatalogSlotWrite => {
            &[CatalogImage::Old, CatalogImage::Torn, CatalogImage::New]
        }
        HookPhase::AfterCatalogFileSyncBeforeDirectorySync |
        HookPhase::AfterCatalogDirectorySync |
        HookPhase::AfterCatalogSyncBeforeRecovery |
        HookPhase::AfterPhysicalRecovery |
        HookPhase::BeforeSemanticAdmission |
        HookPhase::AfterSemanticAdmission => &[CatalogImage::New],
    }
}

pub(super) fn interrupted_images(
    prefix: &[PackEntry],
    transaction: &[PackEntry],
    hook: HookPhase,
) -> Vec<(Vec<PackEntry>, CatalogImage)> {
    let (synced, written) = durable_window(hook, transaction.len());
    let catalogs = catalog_images(hook);
    let mut images = Vec::with_capacity(catalogs.len() * 2 * (written - synced + 1));
    for &catalog in catalogs {
        for kept in synced..=written {
            let image = [prefix, &transaction[..kept]].concat();
            if kept < written {
                let mut torn = image.clone();
                torn.push(PackEntry::TornTail);
                images.push((torn, catalog));
            }
            images.push((image, catalog));
        }
    }
    images
}

pub(super) fn recover_appointed(
    pack: &[PackEntry],
    catalog: CatalogImage,
    predecessor_entries: usize,
    successor_entries: usize,
) -> Vec<PackEntry> {
    match catalog {
        CatalogImage::New => pack[..successor_entries].to_vec(),
        CatalogImage::Old | CatalogImage::Torn | CatalogImage::Other => {
            pack[..predecessor_entries].to_vec()
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct StepChoice {
    manifest: usize,
    metadata_root: usize,
    shape: ShapeSummary,
    records: usize,
}

#[derive(Clone, Debug)]
pub(super) struct TrustedHistory {
    pub(super) world: TrustedWorld,
    pub(super) initial: AuthorityCore,
    pub(super) steps: Vec<StepChoice>,
}

#[derive(Clone, Debug)]
struct TrustedStep {
    predecessor: AuthorityCore,
    receipt: SealedReceipt,
    successor: AuthorityCore,
    trace: Vec<PublicationEvent>,
}

fn first_entries(manifest: usize) -> RetentionEntries {
    RetentionEntries {
        current: RetentionEntry {
            generation: 0,
            manifest,
        },
        historical: Vec::new(),
    }
}

pub(super) fn initial_records(count: usize) -> Vec<IndexRecord> {
    (0..count)
        .map(|offset| IndexRecord {
            digest: 10_000 + offset,
            reference: offset,
            location: offset,
        })
        .collect()
}

fn metadata_anchors(core: &AuthorityCore) -> Vec<usize> {
    core.metadata_roots
        .iter()
        .enumerate()
        .flat_map(|(vertex, &count)| std::iter::repeat_n(vertex, count))
        .collect()
}

pub(super) fn receipt_for(
    core: &AuthorityCore,
    generation: usize,
    choice: &StepChoice,
) -> SealedReceipt {
    SealedReceipt {
        base: core.validity,
        target: choice.manifest,
        profile: core.profile,
        generation,
        metadata: RootDelta {
            added: vec![choice.metadata_root],
            removed: metadata_anchors(core),
        },
        shape: choice.shape,
        index_plan: (0..choice.records)
            .map(|offset| {
                let digest = generation * 16 + offset;
                IndexRecord {
                    digest,
                    reference: digest + 1,
                    location: generation,
                }
            })
            .collect(),
    }
}

fn shape_summaries() -> impl Strategy<Value = ShapeSummary> {
    (0u64..6, 0u64..6, 0u64..24, 0u64..24).prop_map(
        |(logical_nodes, endpoint_count, logical_path_bytes, payload_bytes)| ShapeSummary {
            logical_nodes,
            endpoint_count,
            logical_path_bytes,
            payload_bytes,
        },
    )
}

pub(super) fn trusted_histories() -> impl Strategy<Value = TrustedHistory> {
    (1usize..8, 1usize..5).prop_flat_map(|(semantic_count, metadata_count)| {
        (
            prop::collection::vec(prop::collection::vec(any::<u8>(), 0..4), semantic_count),
            prop::collection::vec(0u64..5, semantic_count),
            prop::collection::vec(prop::collection::vec(any::<u8>(), 0..3), metadata_count),
            prop::collection::vec(1u64..5, metadata_count),
            (0..semantic_count, 0..metadata_count, shape_summaries(), 0usize..3, 0usize..3),
            prop::collection::vec(
                (0..semantic_count, 0..metadata_count, shape_summaries(), 0usize..4),
                1..5,
            ),
            prop::collection::vec(any::<bool>(), 1..9),
        )
            .prop_map(
                |(
                    raw_semantic,
                    semantic_bytes,
                    raw_metadata,
                    metadata_bytes,
                    (manifest, metadata_root, shape, profile, records),
                    steps,
                    schedule,
                )| {
                    let world = TrustedWorld {
                        env: ComposedEnvironment {
                            semantic: history_environment(raw_semantic, semantic_bytes),
                            metadata: history_environment(raw_metadata, metadata_bytes),
                        },
                        schedule,
                        capacity: 64,
                        sidecar: SidecarState::Absent,
                    };
                    let metadata_roots =
                        root_counts(world.env.metadata.graph.len(), &[metadata_root]);
                    let initial = cold_admitted_core(
                        &world,
                        first_entries(manifest),
                        metadata_roots,
                        shape,
                        profile,
                        initial_records(records),
                    )
                    .expect("initial index records have distinct digests below the capacity");
                    TrustedHistory {
                        world,
                        initial,
                        steps: steps
                            .into_iter()
                            .map(|(manifest, metadata_root, shape, records)| StepChoice {
                                manifest,
                                metadata_root,
                                shape,
                                records,
                            })
                            .collect(),
                    }
                },
            )
    })
}

fn run_trusted_history(history: &TrustedHistory) -> Result<Vec<TrustedStep>, TestCaseError> {
    let mut core = history.initial.clone();
    let mut steps = Vec::with_capacity(history.steps.len());
    for (generation, choice) in (1..).zip(&history.steps) {
        let receipt = receipt_for(&core, generation, choice);
        let (outcome, trace) = history.world.publish(
            TrustedVariant::Faithful,
            &core,
            &receipt,
            &TrustedPolicy::UNBOUNDED,
            &TrustedPolicy::UNBOUNDED,
            PublicationFault::NoFault,
        );
        let successor = match outcome {
            PublicationOutcome::TrustedReady(successor) => *successor,
            other => {
                return Err(TestCaseError::fail(format!(
                    "an unbounded faithful step did not publish: {other:?}"
                )));
            }
        };
        steps.push(TrustedStep {
            predecessor: core,
            receipt,
            successor: successor.clone(),
            trace,
        });
        core = successor;
    }
    Ok(steps)
}

fn production_evidence(
    world: &TrustedWorld,
    core: &AuthorityCore,
    receipt: &SealedReceipt,
) -> Result<ComposedEvidence, TestCaseError> {
    match world.production_order(
        TrustedVariant::Faithful,
        &ComposedLimits::UNBOUNDED,
        &core.ledger,
        &trusted_delta(core, receipt),
    ) {
        ProductionResult::Prepared(_, evidence) => Ok(evidence),
        ProductionResult::Refused(refusing, why) => Err(TestCaseError::fail(format!(
            "a generated step refused without limits at {refusing:?}: {why:?}"
        ))),
    }
}

fn exact_policy(
    core: &AuthorityCore,
    receipt: &SealedReceipt,
    evidence: &ComposedEvidence,
) -> TrustedPolicy {
    let mut policy =
        exact_ceilings(&successor_values(core, receipt, evidence), &TrustedPolicy::UNBOUNDED);
    for domain in DOMAINS {
        let measured = evidence.domain(domain);
        for axis in STEP_AXES {
            policy = policy.with(PolicyAxis::Work(domain, axis), measured.measured(axis));
        }
        if domain != Domain::Current {
            let [objects, bytes] = final_ceilings(domain);
            policy = policy
                .with(PolicyAxis::State(objects), measured.objects)
                .with(PolicyAxis::State(bytes), measured.bytes);
        }
    }
    policy
}

pub(super) fn expected_refusal(
    axis: PolicyAxis,
    current: (u64, u64),
    maxima: (u64, u64),
) -> BindRefusal {
    let refused_at = |domain, axis| BindRefusal::TrialRefusedAt(domain, TrialRefusal::Budget(axis));
    match axis {
        PolicyAxis::Work(domain, axis) => refused_at(domain, axis),
        PolicyAxis::State(StateCeiling::StateObjects) if current.0 == maxima.0 => {
            refused_at(Domain::Current, BudgetAxis::FinalObjects)
        }
        PolicyAxis::State(StateCeiling::StateObjects) => {
            BindRefusal::CeilingRefused(CeilingAxis::StateObjects)
        }
        PolicyAxis::State(StateCeiling::StateBytes) if current.1 == maxima.1 => {
            refused_at(Domain::Current, BudgetAxis::FinalBytes)
        }
        PolicyAxis::State(StateCeiling::StateBytes) => {
            BindRefusal::CeilingRefused(CeilingAxis::StateBytes)
        }
        PolicyAxis::State(StateCeiling::UnionObjects) => {
            refused_at(Domain::Retained, BudgetAxis::FinalObjects)
        }
        PolicyAxis::State(StateCeiling::UnionBytes) => {
            refused_at(Domain::Retained, BudgetAxis::FinalBytes)
        }
        PolicyAxis::State(StateCeiling::MetadataObjects) => {
            refused_at(Domain::Metadata, BudgetAxis::FinalObjects)
        }
        PolicyAxis::State(StateCeiling::MetadataBytes) => {
            refused_at(Domain::Metadata, BudgetAxis::FinalBytes)
        }
        PolicyAxis::State(StateCeiling::RetainedStates) => {
            BindRefusal::CeilingRefused(CeilingAxis::RetainedStates)
        }
        PolicyAxis::State(StateCeiling::LogicalNodes) => {
            BindRefusal::CeilingRefused(CeilingAxis::LogicalNodes)
        }
        PolicyAxis::State(StateCeiling::Endpoints) => {
            BindRefusal::CeilingRefused(CeilingAxis::Endpoints)
        }
        PolicyAxis::State(StateCeiling::LogicalPathBytes) => {
            BindRefusal::CeilingRefused(CeilingAxis::LogicalPathBytes)
        }
        PolicyAxis::State(StateCeiling::PayloadBytes) => {
            BindRefusal::CeilingRefused(CeilingAxis::PayloadBytes)
        }
    }
}

pub(super) fn assert_exact_authority(
    world: &TrustedWorld,
    core: &AuthorityCore,
) -> Result<(), TestCaseError> {
    let env = &world.env;
    prop_assert!(core.entries.current.manifest < env.semantic.graph.len());
    prop_assert_eq!(
        core.ledger.project(Domain::Current),
        &cold_predecessor(&env.semantic, &[core.entries.current.manifest])
    );
    prop_assert_eq!(
        core.ledger.project(Domain::Retained),
        &cold_predecessor(&env.semantic, &core.entries.manifests())
    );
    prop_assert_eq!(
        core.ledger.project(Domain::Metadata),
        &cold_counts_predecessor(&env.metadata, &core.metadata_roots)
    );
    prop_assert_eq!(
        core.retained_states,
        u64::try_from(core.entries.manifests().len()).expect("a state count fits u64")
    );
    prop_assert_eq!(core.maxima, maxima_of(&cold_state_inventories(&env.semantic, &core.entries)));
    prop_assert_eq!(core.validity, core.entries.current.manifest);
    prop_assert_eq!(
        cold_rebuild(world.capacity, &core.index_records),
        IndexResult::Built(core.index.clone())
    );
    Ok(())
}

pub(super) fn trusted_runner(
    cases: u32,
    source_file: &'static str,
) -> prop::test_runner::TestRunner {
    prop::test_runner::TestRunner::new(ProptestConfig {
        cases,
        source_file: Some(source_file),
        ..ProptestConfig::default()
    })
}

fn unit_environment(graph: Vec<Vec<usize>>) -> TrialEnvironment {
    let vertex_count = graph.len();
    TrialEnvironment {
        graph,
        object_bytes: vec![1; vertex_count],
        quoted_read: vec![0; vertex_count],
        consumed_read: vec![0; vertex_count],
    }
}

fn skip_fixture() -> (TrustedWorld, AuthorityCore, SealedReceipt) {
    let semantic = unit_environment(vec![vec![], vec![0], vec![], vec![]]);
    let mut metadata_graph = vec![Vec::new(); 12];
    metadata_graph[10] = vec![11];
    let metadata = unit_environment(metadata_graph);
    let base = ComposedPredecessor {
        current: cold_predecessor(&semantic, &[1]),
        retained: cold_predecessor(&semantic, &[1]),
        metadata: cold_predecessor(&metadata, &[9]),
    };
    let core = AuthorityCore {
        entries: first_entries(1),
        metadata_roots: root_counts(12, &[9]),
        ledger: SharedLedger::of(&base),
        retained_states: 1,
        maxima: (2, 2),
        shape: ShapeSummary::default(),
        validity: 1,
        profile: 0,
        index_records: Vec::new(),
        index: CommittedIndex::default(),
    };
    let receipt = SealedReceipt {
        base: 1,
        target: 2,
        profile: 0,
        generation: 1,
        metadata: RootDelta {
            added: vec![10],
            removed: vec![9],
        },
        shape: ShapeSummary::default(),
        index_plan: Vec::new(),
    };
    let world = TrustedWorld {
        env: ComposedEnvironment { semantic, metadata },
        schedule: vec![true],
        capacity: 8,
        sidecar: SidecarState::Absent,
    };
    (world, core, receipt)
}

fn tight_policy() -> TrustedPolicy {
    TrustedPolicy::GENEROUS.with(PolicyAxis::State(StateCeiling::UnionObjects), 2)
}

pub(super) fn mutation_free(trace: &[PublicationEvent]) -> bool {
    !trace.iter().any(PublicationEvent::mutates)
}

fn demand_fixture() -> TrialEnvironment {
    TrialEnvironment {
        graph: vec![vec![], vec![0, 0], vec![], vec![2]],
        object_bytes: vec![4, 11, 5, 10],
        quoted_read: vec![0; 4],
        consumed_read: vec![0; 4],
    }
}

#[test]
fn trusted_successor_matches_cold_admission_at_every_history_step() {
    assert_publication_model(&[adopted_successor_is_exact_catalog], &[], &[(
        PublicationMutant::WrongCatalogAdoption,
        adopted_successor_is_exact_catalog,
    )]);
    trusted_runner(256, file!())
        .run(&trusted_histories(), |history| {
            assert_exact_authority(&history.world, &history.initial)?;
            for step in run_trusted_history(&history)? {
                let TrustedStep {
                    predecessor,
                    receipt,
                    successor,
                    ..
                } = &step;
                assert_exact_authority(&history.world, successor)?;
                prop_assert_eq!(
                    &successor.entries,
                    &predecessor.entries.commit(RetentionEntry {
                        generation: receipt.generation,
                        manifest: receipt.target,
                    })
                );
                prop_assert_eq!(
                    Some(successor.metadata_roots.clone()),
                    apply_delta(&predecessor.metadata_roots, &receipt.metadata)
                );
                prop_assert_eq!(successor.validity, receipt.target);
                prop_assert_eq!(successor.profile, predecessor.profile);
                prop_assert_eq!(successor.retained_states, predecessor.retained_states + 1);
                prop_assert_eq!(
                    &successor.index_records,
                    &[predecessor.index_records.as_slice(), &receipt.index_plan].concat()
                );
                let cold = cold_admitted_core(
                    &history.world,
                    successor.entries.clone(),
                    successor.metadata_roots.clone(),
                    successor.shape,
                    successor.profile,
                    successor.index_records.clone(),
                )
                .map_err(|refusal| {
                    TestCaseError::fail(format!(
                        "cold admission refused the successor: {refusal:?}"
                    ))
                })?;
                prop_assert_eq!(successor, &cold);
            }
            Ok(())
        })
        .expect("generated trusted histories equal cold admission at every step");
    production_trusted_successor_equals_cold_successor(64);
}

fn index_records(max: usize) -> impl Strategy<Value = Vec<IndexRecord>> {
    prop::collection::vec(
        (0usize..6, 0usize..3, 0usize..1024).prop_map(|(digest, reference, location)| {
            IndexRecord {
                digest,
                reference,
                location,
            }
        }),
        0..max,
    )
}

fn index_outcome_slot(result: &IndexResult) -> usize {
    match result {
        IndexResult::Built(_) => 0,
        IndexResult::Refused(_, IndexRefusal::DuplicateData) => 1,
        IndexResult::Refused(_, IndexRefusal::DigestReferenceConflict) => 2,
        IndexResult::Refused(_, IndexRefusal::RecordCountOverflow) => 3,
    }
}

#[test]
fn trusted_successor_index_frontier_equals_cold_recovery() {
    let coverage = Cell::new([0usize; 4]);
    trusted_runner(256, file!())
        .run(&(index_records(6), index_records(6), 0usize..8), |(prefix, plan, capacity)| {
            let combined = [prefix.as_slice(), &plan].concat();
            let IndexResult::Built(index) = cold_rebuild(capacity, &prefix) else {
                return Ok(());
            };
            let appended = index.insert_all(capacity, &plan);
            prop_assert_eq!(&appended, &cold_rebuild(capacity, &combined));
            let mut seen = coverage.get();
            seen[index_outcome_slot(&appended)] += 1;
            coverage.set(seen);
            if let IndexResult::Built(successor) = appended {
                prop_assert_eq!(&successor.entries, &combined);
                prop_assert_eq!(successor.count, combined.len());
                prop_assert_eq!(
                    (index_tail(successor.count) - INDEX_HEADER_LEN) / INDEX_RECORD_LEN,
                    u64::try_from(successor.count).expect("an index count fits u64")
                );
            }
            Ok(())
        })
        .expect("appended indexes equal the cold rebuild of the appointed records");
    let seen = coverage.get();
    for (slot, name) in ["built", "duplicate", "conflict", "overflow"]
        .iter()
        .enumerate()
    {
        assert!(seen[slot] > 0, "no generated index append was {name}: {seen:?}");
    }

    trusted_runner(64, file!())
        .run(&trusted_histories(), |history| {
            let core = &history.initial;
            let base = receipt_for(core, 1, &history.steps[0]);
            let mut plans = Vec::with_capacity(4);
            if let Some(existing) = core.index_records.first() {
                plans.push(vec![*existing]);
                plans.push(vec![IndexRecord {
                    reference: existing.reference + 1,
                    ..*existing
                }]);
            }
            let repeated = IndexRecord {
                digest: 20_000,
                reference: 0,
                location: 0,
            };
            plans.push(vec![repeated, repeated]);
            plans.push(vec![repeated, IndexRecord {
                reference: 1,
                ..repeated
            }]);
            for plan in plans {
                let receipt = SealedReceipt {
                    index_plan: plan.clone(),
                    ..base.clone()
                };
                let (outcome, trace) = history.world.publish(
                    TrustedVariant::Faithful,
                    core,
                    &receipt,
                    &TrustedPolicy::UNBOUNDED,
                    &TrustedPolicy::UNBOUNDED,
                    PublicationFault::NoFault,
                );
                let records = [core.index_records.as_slice(), &plan].concat();
                let IndexResult::Refused(record, why) =
                    cold_rebuild(history.world.capacity, &records)
                else {
                    return Err(TestCaseError::fail("a colliding plan rebuilt cold"));
                };
                prop_assert_eq!(
                    outcome,
                    PublicationOutcome::DefinitelyNotPublished(NotPublishedStage::Assembly(
                        record, why
                    ))
                );
                prop_assert!(mutation_free(&trace));
            }
            if let Some(first) = base.index_plan.first() {
                let full = TrustedWorld {
                    capacity: core.index.count,
                    ..history.world.clone()
                };
                let (outcome, trace) = full.publish(
                    TrustedVariant::Faithful,
                    core,
                    &base,
                    &TrustedPolicy::UNBOUNDED,
                    &TrustedPolicy::UNBOUNDED,
                    PublicationFault::NoFault,
                );
                prop_assert_eq!(
                    outcome,
                    PublicationOutcome::DefinitelyNotPublished(NotPublishedStage::Assembly(
                        *first,
                        IndexRefusal::RecordCountOverflow
                    ))
                );
                prop_assert!(mutation_free(&trace));
            }
            Ok(())
        })
        .expect("index assembly refuses exactly where the cold rebuild refuses, before mutation");
    production_trusted_index_equals_cold_recovery(64);
}

#[test]
fn trusted_bind_runs_domains_in_order_and_keeps_the_predecessor_on_first_refusal() {
    let (world, core, receipt) = skip_fixture();
    let generous = TrustedPolicy::GENEROUS;
    let BindResult::Prepared(faithful) =
        world.bind(TrustedVariant::Faithful, &core, &receipt, &generous)
    else {
        panic!("the faithful skip fixture binds");
    };
    assert_eq!(faithful.ledger.project(Domain::Retained).objects, 3);
    assert_eq!(faithful.ledger.project(Domain::Metadata).objects, 2);
    let BindResult::Prepared(skip_retained) =
        world.bind(TrustedVariant::SkipRetainedTrial, &core, &receipt, &generous)
    else {
        panic!("the skipped retained trial binds");
    };
    assert_eq!(skip_retained.ledger.project(Domain::Retained).objects, 2);
    assert_ne!(
        skip_retained.ledger.project(Domain::Retained),
        &cold_predecessor(&world.env.semantic, &skip_retained.entries.manifests())
    );
    let BindResult::Prepared(skip_metadata) =
        world.bind(TrustedVariant::SkipMetadataTrial, &core, &receipt, &generous)
    else {
        panic!("the skipped metadata trial binds");
    };
    assert_eq!(skip_metadata.ledger.project(Domain::Metadata).objects, 1);
    assert_ne!(
        skip_metadata.ledger.project(Domain::Metadata),
        &cold_counts_predecessor(&world.env.metadata, &skip_metadata.metadata_roots)
    );

    let coverage = Cell::new([0usize; 5]);
    trusted_runner(128, file!())
        .run(&trusted_histories(), |history| {
            let world = &history.world;
            let mut seen = coverage.get();
            for step in run_trusted_history(&history)? {
                let core = &step.predecessor;
                let receipt = &step.receipt;
                let delta = trusted_delta(core, receipt);
                let composed = ComposedCase {
                    env: world.env.clone(),
                    base: core.ledger.projections(),
                    delta: delta.clone(),
                    schedule: world.schedule.clone(),
                }
                .run(CompositionVariant::Faithful, &ComposedLimits::UNBOUNDED);
                prop_assert_eq!(&composed.kept, &core.ledger.projections());
                let evidence = match (
                    world.production_order(
                        TrustedVariant::Faithful,
                        &ComposedLimits::UNBOUNDED,
                        &core.ledger,
                        &delta,
                    ),
                    composed.outcome,
                ) {
                    (
                        ProductionResult::Prepared(successor, evidence),
                        ComposedOutcome::Prepared(expected),
                    ) => {
                        prop_assert_eq!(&evidence, &expected);
                        prop_assert_eq!(&successor, &core.ledger.commit(&evidence));
                        prop_assert_eq!(successor.projections(), evidence.successor_base());
                        evidence
                    }
                    (production, composed) => {
                        return Err(TestCaseError::fail(format!(
                            "production order {production:?} diverged from composition \
                             {composed:?}"
                        )));
                    }
                };
                let written = core
                    .ledger
                    .write_domain(Domain::Current, &evidence.current)
                    .write_domain(Domain::Retained, &evidence.retained);
                prop_assert_eq!(
                    world.domain_trial(
                        &ComposedLimits::UNBOUNDED,
                        &written,
                        &delta,
                        Domain::Metadata
                    ),
                    world.domain_trial(
                        &ComposedLimits::UNBOUNDED,
                        &core.ledger,
                        &delta,
                        Domain::Metadata
                    )
                );
                let exact = ComposedLimits::exact(&evidence);
                for domain in DOMAINS {
                    for axis in BUDGET_AXES {
                        let measured = evidence.domain(domain).measured(axis);
                        if measured == 0 {
                            continue;
                        }
                        let lowered = exact.with(domain, axis, measured - 1);
                        let refused = ProductionResult::Refused(domain, TrialRefusal::Budget(axis));
                        prop_assert_eq!(
                            world.production_order(
                                TrustedVariant::Faithful,
                                &lowered,
                                &core.ledger,
                                &delta
                            ),
                            refused
                        );
                        let composed = ComposedCase {
                            env: world.env.clone(),
                            base: core.ledger.projections(),
                            delta: delta.clone(),
                            schedule: world.schedule.clone(),
                        }
                        .run(CompositionVariant::Faithful, &lowered);
                        prop_assert_eq!(&composed.kept, &core.ledger.projections());
                        prop_assert_eq!(
                            composed.outcome,
                            ComposedOutcome::Refused(domain, TrialRefusal::Budget(axis))
                        );
                    }
                }
                let policy = exact_policy(core, receipt, &evidence);
                for domain in DOMAINS {
                    for axis in STEP_AXES {
                        let measured = policy.get(PolicyAxis::Work(domain, axis));
                        if measured == 0 {
                            continue;
                        }
                        let lowered = policy.with(PolicyAxis::Work(domain, axis), measured - 1);
                        let before = core.clone();
                        let (outcome, trace) = world.publish(
                            TrustedVariant::Faithful,
                            core,
                            receipt,
                            &lowered,
                            &lowered,
                            PublicationFault::NoFault,
                        );
                        let why = BindRefusal::TrialRefusedAt(domain, TrialRefusal::Budget(axis));
                        prop_assert_eq!(
                            outcome,
                            PublicationOutcome::DefinitelyNotPublished(NotPublishedStage::Bind(
                                why
                            ))
                        );
                        prop_assert_eq!(
                            &trace,
                            &[vec![PublicationEvent::ReceiptChecked], trials_through(domain)]
                                .concat()
                        );
                        prop_assert_eq!(core, &before);
                        seen[domain as usize] += 1;
                    }
                }
                let cold_retained =
                    cold_predecessor(&world.env.semantic, &step.successor.entries.manifests());
                match world.bind(
                    TrustedVariant::SkipRetainedTrial,
                    core,
                    receipt,
                    &TrustedPolicy::UNBOUNDED,
                ) {
                    BindResult::Prepared(skipped)
                        if skipped.ledger.project(Domain::Retained) != &cold_retained =>
                    {
                        seen[3] += 1;
                    }
                    BindResult::Prepared(_) | BindResult::Refused(_) => {}
                }
                let cold_metadata =
                    cold_counts_predecessor(&world.env.metadata, &step.successor.metadata_roots);
                match world.bind(
                    TrustedVariant::SkipMetadataTrial,
                    core,
                    receipt,
                    &TrustedPolicy::UNBOUNDED,
                ) {
                    BindResult::Prepared(skipped)
                        if skipped.ledger.project(Domain::Metadata) != &cold_metadata =>
                    {
                        seen[4] += 1;
                    }
                    BindResult::Prepared(_) | BindResult::Refused(_) => {}
                }
            }
            coverage.set(seen);
            Ok(())
        })
        .expect("the production order is the composed trial and refuses first in domain order");
    let seen = coverage.get();
    for (slot, event) in [
        "current refusal",
        "retained refusal",
        "metadata refusal",
        "skipped retained trial divergence",
        "skipped metadata trial divergence",
    ]
    .iter()
    .enumerate()
    {
        assert!(seen[slot] > 0, "no generated history exposed a {event}: {seen:?}");
    }
    production_trials_refuse_in_domain_order(64);
}

#[test]
fn trusted_policy_accepts_exact_and_refuses_one_below_on_every_axis() {
    assert_publication_model(
        &[trusted_refusal_precedes_all_mutation],
        &[never_trusted_refusal],
        &[
            (PublicationMutant::LateTrustedRefusal, trusted_refusal_precedes_all_mutation),
            (PublicationMutant::PublicationBeforeTrials, trusted_refusal_precedes_all_mutation),
        ],
    );
    let ceiling_exact = SuccessorValues {
        maxima: (3, 7),
        retained_states: 2,
        shape: ShapeSummary {
            logical_nodes: 4,
            endpoint_count: 1,
            logical_path_bytes: 9,
            payload_bytes: 5,
        },
    };
    let exact = exact_ceilings(&ceiling_exact, &TrustedPolicy::ZERO);
    assert_eq!(first_ceiling_exceeded(&exact, &ceiling_exact), None);
    for axis in CEILING_AXES {
        let value = ceiling_exact.value(axis);
        let lowered = exact.with(PolicyAxis::State(axis.ceiling()), value - 1);
        assert_eq!(first_ceiling_exceeded(&lowered, &ceiling_exact), Some(axis));
    }
    let coverage = Cell::new([0usize; 2]);
    trusted_runner(64, file!())
        .run(&trusted_histories(), |history| {
            let world = &history.world;
            let mut seen = coverage.get();
            for step in run_trusted_history(&history)? {
                let core = &step.predecessor;
                let receipt = &step.receipt;
                let evidence = production_evidence(world, core, receipt)?;
                let values = successor_values(core, receipt, &evidence);
                let policy = exact_policy(core, receipt, &evidence);
                let BindResult::Prepared(prepared) =
                    world.bind(TrustedVariant::Faithful, core, receipt, &policy)
                else {
                    return Err(TestCaseError::fail(
                        "the exact policy refused its own measurement",
                    ));
                };
                prop_assert_eq!(&prepared.ledger, &step.successor.ledger);
                prop_assert_eq!(prepared.maxima, step.successor.maxima);
                prop_assert_eq!(prepared.policy, policy);
                for axis in policy_axes() {
                    let value = policy.get(axis);
                    if value == 0 {
                        continue;
                    }
                    let lowered = policy.with(axis, value - 1);
                    let why =
                        expected_refusal(axis, final_inventory(&evidence.current), values.maxima);
                    prop_assert_eq!(
                        world.bind(TrustedVariant::Faithful, core, receipt, &lowered),
                        BindResult::Refused(why)
                    );
                    let before = core.clone();
                    let (outcome, trace) = world.publish(
                        TrustedVariant::Faithful,
                        core,
                        receipt,
                        &lowered,
                        &lowered,
                        PublicationFault::NoFault,
                    );
                    prop_assert_eq!(
                        outcome,
                        PublicationOutcome::DefinitelyNotPublished(NotPublishedStage::Bind(why))
                    );
                    prop_assert!(mutation_free(&trace));
                    prop_assert_eq!(&trace, &bind_trace(&BindResult::Refused(why)));
                    prop_assert_eq!(core, &before);
                    match why {
                        BindRefusal::CeilingRefused(_) => seen[0] += 1,
                        BindRefusal::TrialRefusedAt(..) => seen[1] += 1,
                        BindRefusal::ForeignBase | BindRefusal::ProfileMismatch => {
                            return Err(TestCaseError::fail("a policy axis refused the receipt"));
                        }
                    }
                }
            }
            coverage.set(seen);
            Ok(())
        })
        .expect("every policy axis accepts its exact value and refuses one below before mutation");
    let seen = coverage.get();
    assert!(seen[0] > 0 && seen[1] > 0, "ceiling and trial refusals were not both exercised");
    production_policy_accepts_exact_and_refuses_one_below(64);
}

#[test]
fn trusted_policy_permits_budgeted_union_scratch_above_final_state_size() {
    let swap = TrialCase {
        env: unit_environment(vec![vec![], vec![]]),
        base: cold_predecessor(&unit_environment(vec![vec![], vec![]]), &[0]),
        added: vec![1],
        removed: vec![0],
        schedule: vec![true],
    };
    let swap_limits = TrialLimits([9; 9])
        .with(BudgetAxis::UnionObjects, 2)
        .with(BudgetAxis::FinalObjects, 1);
    let TrialOutcome::Prepared(swapped) = swap.run(TrialVariant::Faithful, &swap_limits).outcome
    else {
        panic!("the swap fixture prepares under the swap limits");
    };
    assert_eq!((swapped.usage.peak_objects, swapped.objects), (2, 1));
    assert_eq!(
        swap.run(TrialVariant::FinalOnUnion, &swap_limits).outcome,
        TrialOutcome::Refused(TrialRefusal::Budget(BudgetAxis::UnionObjects))
    );
    assert_eq!(
        swap.run(TrialVariant::Faithful, &swap_limits.with(BudgetAxis::FinalObjects, 0))
            .outcome,
        TrialOutcome::Refused(TrialRefusal::Budget(BudgetAxis::FinalObjects))
    );

    let coverage = Cell::new(0usize);
    trusted_runner(128, file!())
        .run(&trusted_histories(), |history| {
            let world = &history.world;
            for step in run_trusted_history(&history)? {
                let core = &step.predecessor;
                let receipt = &step.receipt;
                let delta = trusted_delta(core, receipt);
                let evidence = production_evidence(world, core, receipt)?;
                let policy = exact_policy(core, receipt, &evidence);
                let BindResult::Prepared(_) =
                    world.bind(TrustedVariant::Faithful, core, receipt, &policy)
                else {
                    return Err(TestCaseError::fail(
                        "the exact policy refused its own measurement",
                    ));
                };
                for domain in DOMAINS {
                    let measured = evidence.domain(domain);
                    let base = core.ledger.project(domain);
                    let change = match domain {
                        Domain::Current => &delta.current,
                        Domain::Retained => &delta.retained,
                        Domain::Metadata => &delta.metadata,
                    };
                    let mut union_roots = base.roots.clone();
                    for &vertex in &change.added {
                        union_roots[vertex] += 1;
                    }
                    let union = cold_counts_predecessor(world.environment(domain), &union_roots);
                    prop_assert_eq!(
                        (measured.usage.peak_objects, measured.usage.peak_bytes),
                        (union.objects, union.bytes)
                    );
                    for (union_axis, peak, final_value) in [
                        (BudgetAxis::UnionObjects, measured.usage.peak_objects, measured.objects),
                        (BudgetAxis::UnionBytes, measured.usage.peak_bytes, measured.bytes),
                    ] {
                        if peak <= final_value {
                            continue;
                        }
                        coverage.set(coverage.get() + 1);
                        let conflated =
                            policy.with(PolicyAxis::Work(domain, union_axis), final_value);
                        prop_assert_eq!(
                            world.bind(TrustedVariant::Faithful, core, receipt, &conflated),
                            BindResult::Refused(BindRefusal::TrialRefusedAt(
                                domain,
                                TrialRefusal::Budget(union_axis)
                            ))
                        );
                    }
                    let budget = policy_budget(&policy);
                    let limits = budget.get(domain);
                    let case = TrialCase {
                        env: world.environment(domain).clone(),
                        base: base.clone(),
                        added: change.added.clone(),
                        removed: change.removed.clone(),
                        schedule: world.schedule.clone(),
                    };
                    let conflated = case.run(TrialVariant::FinalOnUnion, limits).outcome;
                    match (
                        measured.usage.peak_objects > limits.get(BudgetAxis::FinalObjects),
                        measured.usage.peak_bytes > limits.get(BudgetAxis::FinalBytes),
                    ) {
                        (false, false) => prop_assert_eq!(
                            conflated,
                            case.run(TrialVariant::Faithful, limits).outcome
                        ),
                        (true, false) => prop_assert_eq!(
                            conflated,
                            TrialOutcome::Refused(TrialRefusal::Budget(BudgetAxis::UnionObjects))
                        ),
                        (false, true) => prop_assert_eq!(
                            conflated,
                            TrialOutcome::Refused(TrialRefusal::Budget(BudgetAxis::UnionBytes))
                        ),
                        (true, true) => prop_assert!(matches!(
                            conflated,
                            TrialOutcome::Refused(TrialRefusal::Budget(
                                BudgetAxis::UnionObjects | BudgetAxis::UnionBytes
                            ))
                        )),
                    }
                }
            }
            Ok(())
        })
        .expect("union scratch above the final state size is budgeted separately");
    assert!(coverage.get() > 0, "no generated step had a union peak above its final inventory");
    production_union_budget_admits_scratch_above_final_size(64);
}

#[test]
fn trusted_policy_and_mode_cannot_change_after_preparation() {
    assert_publication_model(&[published_policy_is_sealed_policy], &[], &[(
        PublicationMutant::ChangedPolicy,
        published_policy_is_sealed_policy,
    )]);
    let (world, core, receipt) = skip_fixture();
    let generous = TrustedPolicy::GENEROUS;
    let tight = tight_policy();
    let (outcome, trace) = world.publish(
        TrustedVariant::Faithful,
        &core,
        &receipt,
        &generous,
        &tight,
        PublicationFault::NoFault,
    );
    assert_eq!(
        outcome,
        PublicationOutcome::DefinitelyNotPublished(NotPublishedStage::Preflight(
            PreflightRefusal::TrustedPolicyMismatch
        ))
    );
    assert!(mutation_free(&trace));
    let (ignored, _) = world.publish(
        TrustedVariant::IgnoreSealedPolicy,
        &core,
        &receipt,
        &generous,
        &tight,
        PublicationFault::NoFault,
    );
    let PublicationOutcome::TrustedReady(successor) = ignored else {
        panic!("the policy-ignoring preflight publishes the generously bound successor");
    };
    assert!(
        tight.ceiling(StateCeiling::UnionObjects) <
            successor.ledger.project(Domain::Retained).objects
    );
    assert_eq!(
        world.bind(TrustedVariant::Faithful, &core, &receipt, &tight),
        BindResult::Refused(BindRefusal::TrialRefusedAt(
            Domain::Retained,
            TrialRefusal::Budget(BudgetAxis::FinalObjects)
        ))
    );

    trusted_runner(64, file!())
        .run(&trusted_histories(), |history| {
            let world = &history.world;
            for step in run_trusted_history(&history)? {
                let core = &step.predecessor;
                let receipt = &step.receipt;
                let evidence = production_evidence(world, core, receipt)?;
                let policy = exact_policy(core, receipt, &evidence);
                let BindResult::Prepared(prepared) =
                    world.bind(TrustedVariant::Faithful, core, receipt, &policy)
                else {
                    return Err(TestCaseError::fail(
                        "the exact policy refused its own measurement",
                    ));
                };
                let binding = SealedBinding::Trusted(prepared);
                prop_assert_eq!(
                    preflight_trusted(TrustedVariant::Faithful, &binding, &policy),
                    None
                );
                for axis in policy_axes() {
                    let value = policy.get(axis);
                    let mut changes = Vec::with_capacity(2);
                    changes.push(value + 1);
                    if value > 0 {
                        changes.push(value - 1);
                    }
                    for changed_value in changes {
                        let changed = policy.with(axis, changed_value);
                        prop_assert_eq!(
                            preflight_trusted(TrustedVariant::Faithful, &binding, &changed),
                            Some(PreflightRefusal::TrustedPolicyMismatch)
                        );
                        let (outcome, trace) = world.publish_sealed(
                            TrustedVariant::Faithful,
                            core,
                            &binding,
                            &changed,
                            PublicationFault::NoFault,
                        );
                        prop_assert_eq!(
                            outcome,
                            PublicationOutcome::DefinitelyNotPublished(
                                NotPublishedStage::Preflight(
                                    PreflightRefusal::TrustedPolicyMismatch
                                )
                            )
                        );
                        prop_assert_eq!(trace, vec![PublicationEvent::PolicyPreflight]);
                    }
                }
                let cold_state = &step.successor.ledger.project(Domain::Current).ledger;
                let limits = measure_cold_demand(&world.env.semantic, cold_state);
                let cold_binding = SealedBinding::ColdChecked(limits);
                prop_assert_eq!(
                    preflight_trusted(TrustedVariant::Faithful, &cold_binding, &policy),
                    Some(PreflightRefusal::PublicationModeMismatch)
                );
                prop_assert_eq!(
                    preflight_cold(&binding, &limits),
                    Some(PreflightRefusal::PublicationModeMismatch)
                );
                prop_assert_eq!(preflight_cold(&cold_binding, &limits), None);
                for axis in COLD_AXES {
                    prop_assert_eq!(
                        preflight_cold(&cold_binding, &limits.with(axis, limits.get(axis) + 1)),
                        Some(PreflightRefusal::AdmissionPolicyMismatch)
                    );
                }
                let (outcome, trace) = world.publish_sealed(
                    TrustedVariant::Faithful,
                    core,
                    &cold_binding,
                    &policy,
                    PublicationFault::NoFault,
                );
                prop_assert_eq!(
                    outcome,
                    PublicationOutcome::DefinitelyNotPublished(NotPublishedStage::Preflight(
                        PreflightRefusal::PublicationModeMismatch
                    ))
                );
                prop_assert!(mutation_free(&trace));
            }
            Ok(())
        })
        .expect("the sealed policy and mode bind every publication");
    production_binding_fixes_publication_mode_and_policy(64);
}

#[test]
fn trusted_binding_refuses_foreign_stale_and_resequenced_authority_before_mutation() {
    assert_publication_model(&[trusted_refusal_precedes_all_mutation], &[], &[(
        PublicationMutant::PublicationBeforeTrials,
        trusted_refusal_precedes_all_mutation,
    )]);
    let reachable = explore_publication(None);
    assert!(
        reachable
            .iter()
            .any(|run| run.refusal == Some(RunRefusal::StaleAuthority))
    );
    assert!(
        reachable
            .iter()
            .filter(|run| run.refusal == Some(RunRefusal::StaleAuthority))
            .all(|run| !run.mutated && run.catalog == CatalogImage::Other)
    );
    assert!(
        reachable
            .iter()
            .any(|run| run.failure == Some(RunFailure::RereadMismatch))
    );

    let (world, core, receipt) = skip_fixture();
    let tight = tight_policy();
    let refused = PublicationOutcome::DefinitelyNotPublished(NotPublishedStage::Bind(
        BindRefusal::TrialRefusedAt(
            Domain::Retained,
            TrialRefusal::Budget(BudgetAxis::FinalObjects),
        ),
    ));
    let (faithful, faithful_trace) = world.publish(
        TrustedVariant::Faithful,
        &core,
        &receipt,
        &tight,
        &tight,
        PublicationFault::NoFault,
    );
    assert_eq!(faithful, refused);
    assert!(mutation_free(&faithful_trace));
    let (early, early_trace) = world.publish(
        TrustedVariant::PublishBeforeTrials,
        &core,
        &receipt,
        &tight,
        &tight,
        PublicationFault::NoFault,
    );
    assert_eq!(early, refused);
    assert!(early_trace.contains(&PublicationEvent::CatalogSwapped));

    let coverage = Cell::new([0usize; 3]);
    trusted_runner(128, file!())
        .run(&trusted_histories(), |history| {
            let world = &history.world;
            let mut seen = coverage.get();
            let steps = run_trusted_history(&history)?;
            let mut tokens = Vec::with_capacity(steps.len() + 1);
            tokens.push(history.initial.validity);
            for (position, step) in steps.iter().enumerate() {
                let core = &step.predecessor;
                let foreign = world.env.semantic.graph.len() + position;
                let mut corrupted = vec![
                    (
                        SealedReceipt {
                            base: foreign,
                            ..step.receipt.clone()
                        },
                        BindRefusal::ForeignBase,
                    ),
                    (
                        SealedReceipt {
                            profile: core.profile + 1,
                            ..step.receipt.clone()
                        },
                        BindRefusal::ProfileMismatch,
                    ),
                ];
                if let Some(&stale) = tokens.iter().find(|&&token| token != core.validity) {
                    corrupted.push((
                        SealedReceipt {
                            base: stale,
                            ..step.receipt.clone()
                        },
                        BindRefusal::ForeignBase,
                    ));
                    seen[0] += 1;
                }
                if step.successor.validity != core.validity {
                    corrupted.push((step.receipt.clone(), BindRefusal::ForeignBase));
                    seen[1] += 1;
                }
                for (receipt, why) in corrupted {
                    let target = if receipt == step.receipt {
                        &step.successor
                    } else {
                        core
                    };
                    for fault in PUBLICATION_FAULTS {
                        let before = target.clone();
                        let (outcome, trace) = world.publish(
                            TrustedVariant::Faithful,
                            target,
                            &receipt,
                            &TrustedPolicy::UNBOUNDED,
                            &TrustedPolicy::UNBOUNDED,
                            fault,
                        );
                        prop_assert_eq!(
                            outcome,
                            PublicationOutcome::DefinitelyNotPublished(NotPublishedStage::Bind(
                                why
                            ))
                        );
                        prop_assert_eq!(trace, vec![PublicationEvent::ReceiptChecked]);
                        prop_assert_eq!(target, &before);
                    }
                }
                let evidence = production_evidence(world, core, &step.receipt)?;
                let exact = exact_policy(core, &step.receipt, &evidence);
                let axes = policy_axes();
                let lowered_axis = axes[position % axes.len()];
                let lowered = exact.with(lowered_axis, exact.get(lowered_axis).saturating_sub(1));
                for bind_policy in [TrustedPolicy::UNBOUNDED, exact, lowered] {
                    for publication_policy in [bind_policy, exact] {
                        for fault in PUBLICATION_FAULTS {
                            let (outcome, trace) = world.publish(
                                TrustedVariant::Faithful,
                                core,
                                &step.receipt,
                                &bind_policy,
                                &publication_policy,
                                fault,
                            );
                            if refused_before_mutation(&outcome) {
                                prop_assert!(mutation_free(&trace));
                            }
                            if let Some(first) = trace.iter().position(PublicationEvent::mutates) {
                                prop_assert!(trace[..first].starts_with(&trusted_safe_prefix()));
                                prop_assert_eq!(bind_policy, publication_policy);
                                seen[2] += 1;
                            }
                        }
                    }
                }
                tokens.push(step.successor.validity);
            }
            coverage.set(seen);
            Ok(())
        })
        .expect("foreign, stale and resequenced receipts are refused before any mutation");
    let seen = coverage.get();
    for (slot, event) in ["stale receipt", "replayed receipt", "mutating publication"]
        .iter()
        .enumerate()
    {
        assert!(seen[slot] > 0, "no generated history exercised a {event}: {seen:?}");
    }
    production_bind_refuses_foreign_stale_and_replayed_receipts(64);
}

#[test]
fn fresh_checkpoint_from_a_trusted_core_measures_new_cold_demand() {
    let env = demand_fixture();
    let shared_child = cold_predecessor(&env, &[1]).ledger;
    let single_child = cold_predecessor(&env, &[3]).ledger;
    assert_eq!(env.inventory(&shared_child), (2, 15));
    assert_eq!(env.inventory(&single_child), (2, 15));
    let shared_demand = measure_cold_demand(&env, &shared_child);
    let single_demand = measure_cold_demand(&env, &single_child);
    assert_eq!(shared_demand, ColdDemand {
        objects: 2,
        bytes: 15,
        references: 2
    });
    assert_eq!(single_demand, ColdDemand {
        objects: 2,
        bytes: 15,
        references: 1
    });
    assert_ne!(shared_demand, single_demand);
    let tight = ColdDemand {
        objects: 2,
        bytes: 15,
        references: 1,
    };
    use ColdPreflightVariant::{MeasureColdDemand, ReusePredecessorDemand, TrustedSummaryDemand};
    assert!(fresh_checkpoint_preflight(
        MeasureColdDemand,
        &env,
        &single_demand,
        &single_child,
        &tight
    ));
    assert!(fresh_checkpoint_preflight(
        ReusePredecessorDemand,
        &env,
        &single_demand,
        &shared_child,
        &tight
    ));
    assert!(!fresh_checkpoint_preflight(
        MeasureColdDemand,
        &env,
        &single_demand,
        &shared_child,
        &tight
    ));
    assert!(fresh_checkpoint_preflight(
        TrustedSummaryDemand,
        &env,
        &shared_demand,
        &shared_child,
        &tight
    ));
    assert!(!fresh_checkpoint_preflight(
        MeasureColdDemand,
        &env,
        &shared_demand,
        &shared_child,
        &tight
    ));
    assert!(fresh_checkpoint_preflight(
        MeasureColdDemand,
        &env,
        &shared_demand,
        &shared_child,
        &ColdDemand {
            references: 2,
            ..tight
        }
    ));

    let coverage = Cell::new([0usize; 2]);
    trusted_runner(256, file!())
        .run(&trusted_histories(), |history| {
            let env = &history.world.env.semantic;
            let mut seen = coverage.get();
            for step in run_trusted_history(&history)? {
                let previous =
                    measure_cold_demand(env, &step.predecessor.ledger.project(Domain::Current).ledger);
                let state = &step.successor.ledger.project(Domain::Current).ledger;
                let measured = measure_cold_demand(env, state);
                prop_assert_eq!(
                    measured,
                    measure_cold_demand(
                        env,
                        &cold_predecessor(env, &[step.successor.entries.current.manifest]).ledger
                    )
                );
                prop_assert!(fresh_checkpoint_preflight(MeasureColdDemand, env, &previous, state, &measured));
                for axis in COLD_AXES {
                    let value = measured.get(axis);
                    if value > 0 {
                        prop_assert!(!fresh_checkpoint_preflight(
                            MeasureColdDemand,
                            env,
                            &previous,
                            state,
                            &measured.with(axis, value - 1)
                        ));
                    }
                }
                if fresh_checkpoint_preflight(ReusePredecessorDemand, env, &previous, state, &previous) &&
                    !fresh_checkpoint_preflight(MeasureColdDemand, env, &previous, state, &previous)
                {
                    seen[0] += 1;
                }
                let (objects, bytes) = env.inventory(state);
                let summary = summary_demand(objects, bytes);
                if fresh_checkpoint_preflight(TrustedSummaryDemand, env, &previous, state, &summary) &&
                    !fresh_checkpoint_preflight(MeasureColdDemand, env, &previous, state, &summary)
                {
                    seen[1] += 1;
                }
            }
            coverage.set(seen);
            Ok(())
        })
        .expect("a fresh checkpoint measures the new state's cold demand exactly");
    let seen = coverage.get();
    assert!(seen[0] > 0, "no generated history exposed the reused-demand mutant: {seen:?}");
    assert!(seen[1] > 0, "no generated history exposed the summary-demand mutant: {seen:?}");
    production_fresh_checkpoint_measures_new_cold_demand(32);
}

#[test]
fn trusted_publication_performs_no_physical_recovery_or_cold_admission() {
    assert_publication_model(
        &[no_physical_recovery_on_trusted_success, trusted_successor_has_no_cold_evidence],
        &[never_trusted_ready],
        &[
            (PublicationMutant::TrustedRecovery, no_physical_recovery_on_trusted_success),
            (PublicationMutant::FabricatedColdEvidence, trusted_successor_has_no_cold_evidence),
        ],
    );
    trusted_runner(128, file!())
        .run(&trusted_histories(), |history| {
            let world = &history.world;
            for step in run_trusted_history(&history)? {
                prop_assert_eq!(hook_trace(&step.trace), trusted_hook_phases().to_vec());
                prop_assert!(!step.trace.iter().any(|event| matches!(
                    event,
                    PublicationEvent::PhysicalRecovery | PublicationEvent::ColdAdmission
                )));
                let limits = measure_cold_demand(
                    &world.env.semantic,
                    &step.successor.ledger.project(Domain::Current).ledger,
                );
                let (cold, cold_trace) = world.publish_cold_checked(
                    &step.predecessor,
                    &step.receipt,
                    &TrustedPolicy::UNBOUNDED,
                    &limits,
                );
                prop_assert_eq!(cold, ColdPublicationOutcome::ColdReady {
                    core: Box::new(step.successor.clone()),
                    evidence: limits,
                });
                prop_assert_eq!(hook_trace(&cold_trace), COLD_HOOK_PHASES.to_vec());
                for event in [PublicationEvent::PhysicalRecovery, PublicationEvent::ColdAdmission] {
                    prop_assert_eq!(cold_trace.iter().filter(|seen| **seen == event).count(), 1);
                }
            }
            Ok(())
        })
        .expect("trusted publication runs exactly the thirteen trusted phases");
    production_trusted_publication_skips_cold_admission(32);
}

#[test]
fn trusted_and_cold_checked_publication_write_identical_bytes() {
    assert_publication_model(&[adoption_follows_directory_sync_and_resolution], &[], &[(
        PublicationMutant::EarlyAdoption,
        adoption_follows_directory_sync_and_resolution,
    )]);
    trusted_runner(128, file!())
        .run(&trusted_histories(), |history| {
            for step in run_trusted_history(&history)? {
                let mut writes = Vec::with_capacity(SIDECAR_STATES.len());
                for sidecar in SIDECAR_STATES {
                    let world = TrustedWorld {
                        sidecar,
                        ..history.world.clone()
                    };
                    let (trusted, trusted_trace) = world.publish(
                        TrustedVariant::Faithful,
                        &step.predecessor,
                        &step.receipt,
                        &TrustedPolicy::UNBOUNDED,
                        &TrustedPolicy::UNBOUNDED,
                        PublicationFault::NoFault,
                    );
                    let (cold, cold_trace) = world.publish_cold_checked(
                        &step.predecessor,
                        &step.receipt,
                        &TrustedPolicy::UNBOUNDED,
                        &ColdDemand::LOOSE,
                    );
                    let trusted_writes = durable_writes(&trusted_trace);
                    prop_assert_eq!(&trusted_writes, &durable_writes(&cold_trace));
                    prop_assert_eq!(
                        trusted_writes.last(),
                        Some(&PublicationEvent::SidecarResolved(resolve_sidecar(sidecar)))
                    );
                    prop_assert_eq!(
                        trusted,
                        PublicationOutcome::TrustedReady(Box::new(step.successor.clone()))
                    );
                    let ColdPublicationOutcome::ColdReady { core, .. } = cold else {
                        return Err(TestCaseError::fail(
                            "cold-checked publication under loose limits refused",
                        ));
                    };
                    prop_assert_eq!(&*core, &step.successor);
                    writes.push(trusted_writes);
                }
                for (position, write) in writes.iter().enumerate() {
                    prop_assert!(writes[position + 1..].iter().all(|other| other != write));
                }
            }
            Ok(())
        })
        .expect("trusted and cold-checked publications write the same bytes for every sidecar");
    production_trusted_and_cold_publication_write_identical_bytes(16);
}

fn durable_writes(trace: &[PublicationEvent]) -> Vec<PublicationEvent> {
    trace
        .iter()
        .filter(|event| event.mutates())
        .cloned()
        .collect()
}

#[test]
fn trusted_incremental_publication_recovers_and_retries_every_phase() {
    assert_publication_model(
        &PUBLICATION_INVARIANTS,
        &[never_indeterminate, never_published_resolution_failure, never_stage_failure_before_cas],
        &[],
    );
    assert_eq!(trusted_hook_phases().len(), TRUSTED_HOOK_PHASE_COUNT);
    for &hook in trusted_hook_phases() {
        let behaviors = hook_failure_behaviors(hook);
        assert!(!behaviors.is_empty(), "trusted phase {hook:?} has no failure behavior");
        for behavior in behaviors {
            let run = replay(trusted_scenario(true, true, true), &behavior)
                .unwrap_or_else(|| panic!("failure at {hook:?} is not a model behavior"));
            assert_eq!(run.outcome, production_failure_class(hook), "{hook:?}");
            assert!(
                PUBLICATION_INVARIANTS
                    .iter()
                    .all(|invariant| invariant(&run))
            );
        }
    }
    let success = replay(trusted_scenario(true, true, true), &TRUSTED_SUCCESS_ACTIONS)
        .expect("the trusted success path is a model behavior");
    assert_eq!(success.outcome, RunOutcome::Ready);

    trusted_runner(64, file!())
        .run(&trusted_histories(), |history| {
            let world = &history.world;
            let mut pack = initial_records(history.initial.index_records.len())
                .into_iter()
                .map(PackEntry::Record)
                .collect::<Vec<_>>();
            for step in run_trusted_history(&history)? {
                let core = &step.predecessor;
                let receipt = &step.receipt;
                for fault in PUBLICATION_FAULTS {
                    let (outcome, trace) = world.publish(
                        TrustedVariant::Faithful,
                        core,
                        receipt,
                        &TrustedPolicy::UNBOUNDED,
                        &TrustedPolicy::UNBOUNDED,
                        fault,
                    );
                    for persisted in [CatalogImage::Old, CatalogImage::Torn, CatalogImage::New] {
                        let behavior =
                            tla_behavior(&trace, &outcome, persisted).ok_or_else(|| {
                                TestCaseError::fail("a faulted trace has no model behavior")
                            })?;
                        let run = replay(trusted_scenario(true, true, true), &behavior)
                            .ok_or_else(|| {
                                TestCaseError::fail(format!("{fault:?} left the model"))
                            })?;
                        prop_assert_eq!(run.outcome, outcome_class(&outcome));
                        prop_assert!(
                            PUBLICATION_INVARIANTS
                                .iter()
                                .all(|invariant| invariant(&run))
                        );
                    }
                }
                let evidence = production_evidence(world, core, receipt)?;
                let exact = exact_policy(core, receipt, &evidence);
                let changed = exact.with(
                    PolicyAxis::State(StateCeiling::Endpoints),
                    exact.get(PolicyAxis::State(StateCeiling::Endpoints)) + 1,
                );
                let (outcome, trace) = world.publish(
                    TrustedVariant::Faithful,
                    core,
                    receipt,
                    &exact,
                    &changed,
                    PublicationFault::NoFault,
                );
                let behavior = tla_behavior(&trace, &outcome, CatalogImage::Old)
                    .ok_or_else(|| TestCaseError::fail("a policy refusal has no model behavior"))?;
                let run = replay(trusted_scenario(true, false, true), &behavior)
                    .ok_or_else(|| TestCaseError::fail("a policy refusal left the model"))?;
                prop_assert_eq!(run.refusal, Some(RunRefusal::PolicyMismatch));

                let (retry, _) = world.publish(
                    TrustedVariant::Faithful,
                    core,
                    receipt,
                    &TrustedPolicy::UNBOUNDED,
                    &TrustedPolicy::UNBOUNDED,
                    PublicationFault::NoFault,
                );
                let PublicationOutcome::TrustedReady(retried_successor) = retry else {
                    return Err(TestCaseError::fail("the retry did not publish"));
                };
                prop_assert_eq!(&*retried_successor, &step.successor);
                let transaction = transaction_entries(receipt);
                let uninterrupted = [pack.as_slice(), &transaction].concat();
                for &hook in trusted_hook_phases() {
                    for (interrupted, catalog) in interrupted_images(&pack, &transaction, hook) {
                        let recovered = recover_appointed(
                            &interrupted,
                            catalog,
                            pack.len(),
                            uninterrupted.len(),
                        );
                        prop_assert_eq!(&recovered[..pack.len()], pack.as_slice());
                        let retried = match catalog {
                            CatalogImage::New => recovered,
                            CatalogImage::Old | CatalogImage::Torn | CatalogImage::Other => {
                                prop_assert_eq!(recovered.len(), pack.len());
                                [recovered.as_slice(), &transaction].concat()
                            }
                        };
                        prop_assert_eq!(&retried, &uninterrupted);
                    }
                }
                pack = uninterrupted;
            }
            Ok(())
        })
        .expect("every trusted phase recovers to the appointed prefix and retries exactly");
    production_trusted_publication_faults_recover_and_retry(16);
    production_trusted_crash_recovers_and_retries_every_phase();
}

#[test]
fn trusted_retained_state_maxima_equal_cold_requirements() {
    let env = TrialEnvironment {
        graph: vec![Vec::new(); 4],
        object_bytes: vec![0, 1, 2, 3],
        quoted_read: vec![0; 4],
        consumed_read: vec![0; 4],
    };
    let entry = |generation, manifest| RetentionEntry {
        generation,
        manifest,
    };
    let tied = RetentionEntries {
        current: entry(2, 1),
        historical: vec![entry(0, 3), entry(1, 3)],
    };
    let strict = RetentionEntries {
        current: entry(2, 1),
        historical: vec![entry(0, 3), entry(1, 2)],
    };
    let victim = entry(0, 3);
    let tied_maxima = maxima_of(&cold_state_inventories(&env, &tied));
    let strict_maxima = maxima_of(&cold_state_inventories(&env, &strict));
    assert_eq!((tied_maxima, strict_maxima), ((1, 3), (1, 3)));
    let victim_state = cold_predecessor(&env, &[victim.manifest]);
    assert_eq!((victim_state.objects, victim_state.bytes), (1, 3));
    assert!(tied.historical.contains(&victim) && strict.historical.contains(&victim));
    let tied_after = maxima_of(&cold_state_inventories(&env, &tied.evict(victim)));
    let strict_after = maxima_of(&cold_state_inventories(&env, &strict.evict(victim)));
    assert_eq!((tied_after, strict_after), ((1, 3), (1, 2)));
    assert_ne!(tied_after, strict_after);

    trusted_runner(256, file!())
        .run(&trusted_histories(), |history| {
            let env = &history.world.env.semantic;
            for step in run_trusted_history(&history)? {
                let successor = &step.successor;
                let values = cold_state_inventories(env, &successor.entries);
                prop_assert_eq!(successor.maxima, maxima_of(&values));
                let target = cold_predecessor(env, &[step.receipt.target]);
                prop_assert_eq!(
                    successor.maxima,
                    pair_max(step.predecessor.maxima, (target.objects, target.bytes))
                );
                let (objects, bytes) = successor.maxima;
                prop_assert!(per_state_preflight(&values, objects, bytes));
                if objects > 0 {
                    prop_assert!(!per_state_preflight(&values, objects - 1, bytes));
                }
                if bytes > 0 {
                    prop_assert!(!per_state_preflight(&values, objects, bytes - 1));
                }
            }
            Ok(())
        })
        .expect("trusted maxima are the cold per-state maxima while retention appends");
    production_maxima_equal_cold_requirements(64);
}

#[test]
fn trusted_validity_is_bound_to_the_exact_predecessor_and_receipt() {
    let coverage = Cell::new(0usize);
    trusted_runner(256, file!())
        .run(&trusted_histories(), |history| {
            let world = &history.world;
            for step in run_trusted_history(&history)? {
                let core = &step.predecessor;
                let receipt = &step.receipt;
                prop_assert_eq!(step.successor.validity, receipt.target);
                prop_assert_eq!(step.successor.profile, core.profile);
                for base in [core.validity + 1, core.validity + world.env.semantic.graph.len()] {
                    prop_assert_eq!(
                        world.bind(
                            TrustedVariant::Faithful,
                            core,
                            &SealedReceipt {
                                base,
                                ..receipt.clone()
                            },
                            &TrustedPolicy::UNBOUNDED
                        ),
                        BindResult::Refused(BindRefusal::ForeignBase)
                    );
                }
                for profile in [core.profile + 1, core.profile + 2] {
                    prop_assert_eq!(
                        world.bind(
                            TrustedVariant::Faithful,
                            core,
                            &SealedReceipt {
                                profile,
                                ..receipt.clone()
                            },
                            &TrustedPolicy::UNBOUNDED
                        ),
                        BindResult::Refused(BindRefusal::ProfileMismatch)
                    );
                }
                let next = receipt_for(&step.successor, receipt.generation + 1, &history.steps[0]);
                if next.base != core.validity {
                    coverage.set(coverage.get() + 1);
                    prop_assert_eq!(
                        world.bind(
                            TrustedVariant::Faithful,
                            core,
                            &next,
                            &TrustedPolicy::UNBOUNDED
                        ),
                        BindResult::Refused(BindRefusal::ForeignBase)
                    );
                }
            }
            Ok(())
        })
        .expect("a receipt binds only to the exact predecessor token and profile");
    assert!(
        coverage.get() > 0,
        "no generated history bound a successor's receipt to its predecessor"
    );
    production_validity_binds_the_exact_predecessor_and_receipt(64);
}

#[test]
fn trusted_successor_migrates_a_v1_source_once_and_matches_the_cold_seed() {
    let coverage = Cell::new([0usize; 2]);
    trusted_runner(256, file!())
        .run(&trusted_histories(), |history| {
            let world = &history.world;
            let metadata_count = world.env.metadata.graph.len();
            let initial = cold_admitted_core(
                world,
                history.initial.entries.clone(),
                vec![0; metadata_count],
                history.initial.shape,
                history.initial.profile,
                history.initial.index_records.clone(),
            )
            .map_err(|refusal| TestCaseError::fail(format!("{refusal:?}")))?;
            prop_assert_eq!(initial.ledger.project(Domain::Metadata).objects, 0);
            let v1 = TrustedHistory {
                initial,
                ..history.clone()
            };
            let mut seen = coverage.get();
            let steps = run_trusted_history(&v1)?;
            for (position, step) in steps.iter().enumerate() {
                let migration = step.receipt.metadata.removed.is_empty();
                prop_assert_eq!(migration, position == 0);
                let anchor = step.receipt.metadata.added[0];
                prop_assert_eq!(
                    &step.successor.metadata_roots,
                    &root_counts(metadata_count, &[anchor])
                );
                prop_assert_eq!(
                    step.successor.ledger.project(Domain::Metadata),
                    &cold_counts_predecessor(
                        &world.env.metadata,
                        &root_counts(metadata_count, &[anchor])
                    )
                );
                assert_exact_authority(world, &step.successor)?;
                if position == 0 {
                    let remigrated = SealedReceipt {
                        metadata: RootDelta {
                            added: step.receipt.metadata.added.clone(),
                            removed: Vec::new(),
                        },
                        ..receipt_for(&step.successor, step.receipt.generation + 1, &v1.steps[0])
                    };
                    if let BindResult::Prepared(twice) = world.bind(
                        TrustedVariant::Faithful,
                        &step.successor,
                        &remigrated,
                        &TrustedPolicy::UNBOUNDED,
                    ) {
                        let seed = root_counts(metadata_count, &[remigrated.metadata.added[0]]);
                        prop_assert_ne!(&twice.metadata_roots, &seed);
                        prop_assert_ne!(
                            twice.ledger.project(Domain::Metadata),
                            &cold_counts_predecessor(&world.env.metadata, &seed)
                        );
                        seen[0] += 1;
                    }
                }
                seen[1] += usize::from(!migration);
            }
            coverage.set(seen);
            Ok(())
        })
        .expect("a V1 source migrates once at its first successor and matches the cold seed");
    let seen = coverage.get();
    assert!(seen[0] > 0, "no generated history exposed a second migration: {seen:?}");
    assert!(seen[1] > 0, "no generated history extended a migrated source: {seen:?}");
    production_retention_migrates_once_and_matches_the_cold_seed(64);
}

fn tightened_cold_limits() -> impl Strategy<Value = ColdDemand> {
    prop_oneof![
        Just(ColdDemand::default()),
        (0..COLD_AXES.len()).prop_map(|axis| ColdDemand::LOOSE.with(COLD_AXES[axis], 0)),
    ]
}

fn owner_requests() -> impl Strategy<Value = (OwnerRequest, ColdDemand)> {
    (
        0usize..6,
        0usize..6,
        0usize..6,
        prop_oneof![Just(ColdDemand::default()), Just(ColdDemand::LOOSE)],
        prop_oneof![Just(TrustedPolicy::ZERO), Just(TrustedPolicy::GENEROUS)],
        (1u64..4, 1u64..4, 1u64..4),
        prop_oneof![Just(OwnerJournal::Empty), Just(OwnerJournal::Captured)],
        tightened_cold_limits(),
    )
        .prop_map(
            |(current, successor, recovered, cold, trusted, demand, journal, tightened)| {
                let (objects, bytes, references) = demand;
                (
                    OwnerRequest {
                        current,
                        pending: OwnerPhase::AwaitingAck(successor),
                        journal,
                        limits: OwnerLimits { cold, trusted },
                        recovery_demand: ColdDemand {
                            objects,
                            bytes,
                            references,
                        },
                        recovered,
                    },
                    tightened,
                )
            },
        )
}

fn refusing_publication(_: &TrustedPolicy) -> PublicationOutcome {
    PublicationOutcome::DefinitelyNotPublished(NotPublishedStage::Pin)
}

pub(super) fn owner_empty_checkpoint_model_property() {
    trusted_runner(256, file!())
        .run(&owner_requests(), |(request, tightened)| {
            let empty = OwnerRequest {
                pending: OwnerPhase::Ready,
                journal: OwnerJournal::Empty,
                ..request
            };
            let tight = OwnerRequest {
                limits: OwnerLimits {
                    cold: tightened,
                    ..empty.limits
                },
                ..empty
            };
            for candidate in [empty, tight] {
                let probe = OwnerProbe::default();
                prop_assert_eq!(
                    owner_checkpoint(&candidate, &probe, &refusing_publication),
                    (OwnerOutcome::Accepted(candidate.current), vec![OwnerEvent::NoChange])
                );
                prop_assert_eq!((probe.cold_checks.get(), probe.publications.get()), (0, 0));
            }
            Ok(())
        })
        .expect("an empty checkpoint consults no limit and publishes nothing");
}

pub(super) fn owner_awaiting_ack_retry_model_property() {
    let contrast = Cell::new(0usize);
    trusted_runner(256, file!())
        .run(&owner_requests(), |(request, tightened)| {
            let OwnerPhase::AwaitingAck(successor) = request.pending else {
                return Err(TestCaseError::fail("the generator yields awaiting owners"));
            };
            let tight = OwnerRequest {
                limits: OwnerLimits {
                    cold: tightened,
                    ..request.limits
                },
                ..request
            };
            let mut outcomes = Vec::with_capacity(2);
            for candidate in [request, tight] {
                let probe = OwnerProbe::default();
                prop_assert_eq!(
                    owner_reconcile(&candidate, &probe),
                    (OwnerOutcome::Accepted(successor), vec![OwnerEvent::AcknowledgedOnce(
                        successor
                    )])
                );
                let checkpoint = owner_checkpoint(&candidate, &probe, &refusing_publication);
                prop_assert_eq!(probe.cold_checks.get(), 0);
                prop_assert_eq!(
                    checkpoint
                        .1
                        .iter()
                        .filter(|event| matches!(event, OwnerEvent::AcknowledgedOnce(_)))
                        .count(),
                    1
                );
                outcomes.push(checkpoint);
            }
            prop_assert_eq!(&outcomes[0], &outcomes[1]);
            let recovery = OwnerRequest {
                pending: OwnerPhase::RecoveryRequired,
                ..request
            };
            let loose = OwnerRequest {
                limits: OwnerLimits {
                    cold: ColdDemand::LOOSE,
                    ..recovery.limits
                },
                ..recovery
            };
            let strict = OwnerRequest {
                limits: OwnerLimits {
                    cold: ColdDemand::default(),
                    ..recovery.limits
                },
                ..recovery
            };
            let loose_probe = OwnerProbe::default();
            let strict_probe = OwnerProbe::default();
            let loose_outcome = owner_reconcile(&loose, &loose_probe);
            let strict_outcome = owner_reconcile(&strict, &strict_probe);
            prop_assert_eq!(
                (loose_probe.cold_checks.get(), strict_probe.cold_checks.get()),
                (1, 1)
            );
            prop_assert_eq!(loose_outcome.0, OwnerOutcome::Accepted(recovery.recovered));
            prop_assert_eq!(strict_outcome.0, OwnerOutcome::Refused(OwnerPhase::RecoveryRequired));
            contrast.set(contrast.get() + 1);
            Ok(())
        })
        .expect("an awaiting retry acknowledges once without consulting the cold limits");
    assert!(contrast.get() > 0, "the recovery branch never consulted the cold limits");
}

pub(super) fn trusted_success_without_cold_admission_model_property() {
    assert_publication_model(
        &[cold_mode_success_includes_cold_admission],
        &[never_cold_ready, never_trusted_ready],
        &[(PublicationMutant::SkippedColdAdmission, cold_mode_success_includes_cold_admission)],
    );
    let coverage = Cell::new(0usize);
    trusted_runner(128, file!())
        .run(&trusted_histories(), |history| {
            let world = &history.world;
            for step in run_trusted_history(&history)? {
                let measured = measure_cold_demand(
                    &world.env.semantic,
                    &step.successor.ledger.project(Domain::Current).ledger,
                );
                for axis in COLD_AXES {
                    let value = measured.get(axis);
                    if value == 0 {
                        continue;
                    }
                    let tight = measured.with(axis, value - 1);
                    let (cold, cold_trace) = world.publish_cold_checked(
                        &step.predecessor,
                        &step.receipt,
                        &TrustedPolicy::UNBOUNDED,
                        &tight,
                    );
                    prop_assert_eq!(
                        cold,
                        ColdPublicationOutcome::PublishedAdmissionRefused(measured)
                    );
                    prop_assert!(cold_trace.contains(&PublicationEvent::ColdAdmission));
                    let (trusted, _) = world.publish(
                        TrustedVariant::Faithful,
                        &step.predecessor,
                        &step.receipt,
                        &TrustedPolicy::UNBOUNDED,
                        &TrustedPolicy::UNBOUNDED,
                        PublicationFault::NoFault,
                    );
                    prop_assert_eq!(
                        trusted,
                        PublicationOutcome::TrustedReady(Box::new(step.successor.clone()))
                    );
                    coverage.set(coverage.get() + 1);
                }
            }
            Ok(())
        })
        .expect("trusted success never implies cold admission under the incoming limits");
    assert!(coverage.get() > 0, "no generated step had a positive cold demand axis");
}

pub(super) fn owner_trusted_policy_refusal_model_property() {
    assert_publication_model(&[], &[never_trusted_refusal], &[]);
    let coverage = Cell::new(0usize);
    trusted_runner(64, file!())
        .run(&trusted_histories(), |history| {
            let world = &history.world;
            for step in run_trusted_history(&history)? {
                let core = &step.predecessor;
                let receipt = &step.receipt;
                let evidence = production_evidence(world, core, receipt)?;
                let exact = exact_policy(core, receipt, &evidence);
                for axis in policy_axes() {
                    let value = exact.get(axis);
                    if value == 0 {
                        continue;
                    }
                    let lowered = exact.with(axis, value - 1);
                    let request = OwnerRequest {
                        current: owner_state(core),
                        pending: OwnerPhase::Ready,
                        journal: OwnerJournal::Captured,
                        limits: OwnerLimits {
                            cold: ColdDemand::default(),
                            trusted: lowered,
                        },
                        recovery_demand: ColdDemand::default(),
                        recovered: owner_state(core),
                    };
                    let traces = std::cell::RefCell::new(Vec::with_capacity(1));
                    let publish = |policy: &TrustedPolicy| {
                        let (outcome, trace) = world.publish(
                            TrustedVariant::Faithful,
                            core,
                            receipt,
                            policy,
                            policy,
                            PublicationFault::NoFault,
                        );
                        traces.borrow_mut().push(trace);
                        outcome
                    };
                    let probe = OwnerProbe::default();
                    prop_assert_eq!(
                        owner_checkpoint(&request, &probe, &publish),
                        (OwnerOutcome::Refused(OwnerPhase::Ready), Vec::new())
                    );
                    prop_assert_eq!((probe.cold_checks.get(), probe.publications.get()), (0, 1));
                    prop_assert!(traces.borrow().iter().all(|trace| mutation_free(trace)));
                    coverage.set(coverage.get() + 1);
                }
            }
            Ok(())
        })
        .expect(
            "an owner's trusted policy refusal leaves its files, journals and authority unchanged",
        );
    assert!(coverage.get() > 0, "no generated step had a positive policy axis");
}
