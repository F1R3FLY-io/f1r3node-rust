use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Debug;
use std::fs::{self, DirBuilder, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use proptest::prelude::*;
use proptest::test_runner::TestRunner;
use shared::rust::test_process::{
    ALLOW_EXPECTED_PROBE_CORE, ExpectedProbeCorePolicy, ProbeFailureKind, ProbeOutcome,
    prepare_expected_probe_core_dump, run_child_probe,
};

use super::checkpoint_wal::SyntheticCheckpointSource;
use super::durable_publication::{
    FORMAL_PUBLICATION_PHASE_ORDER, directory_snapshot, install_checkpoint_image,
};
use super::durable_recovery::{catalog_image, checkpoint_image_from_source, limits};
use super::path_exact_closure_properties::{
    CONTINUATION_SLOTS, DATA_SLOTS, ModelState, SlotEdit, Slots, continuation_slot, data_slot,
    install, model_state, staged_sources, transition_batch, transition_strategy,
};
use super::support::{PersistenceTrialDir, TestSemanticProfile, trial_scratch};
use super::trusted_successor_production::{
    BindOutcome, PreflightOutcome, PublicationEffects, RECLAMATION_INTENT, SCENARIOS, Scenario,
    SuccessorObservation, TrustedPolicyEvidence, bind_outcome, core_observation, observed_effects,
    policy_with, preparation_refusal, traced, trusted_observation,
};
use super::trusted_successor_properties::policy_axes;
use crate::rspace::persistence::closure::ClosureDomainV1;
use crate::rspace::persistence::closure::ledger_trial::LedgerTrialAxisV1;
use crate::rspace::persistence::durable::{
    DurableAdmissionErrorV1, DurableAdmissionLimitsV1, DurableAuthorityCoreV1, DurableDirectoryV1,
    DurableIndexedObjectStoreV1, DurableIoError, DurableIoFailpointV1, DurableStartupErrorV1,
    DurableWriterLeaseV1, GenerationCheckpointPublicationHookPointV1,
    GenerationCheckpointPublicationOutcomeV1, IncrementalPublicationErrorV1, ObjectIndexFileViewV1,
    SemanticallyAdmittedDurableWriterV1, TrustedSuccessorPolicyV1, acquire_cold_checked,
    admit_physical_source, arm_catalog_slot_write_prefix, arm_durable_io_failpoint,
    arm_generation_checkpoint_publication_failpoint, arm_generation_checkpoint_publication_hook,
    index_file_name, pack_file_name, publish_trusted_incremental_v1,
    recover_physical_from_lease_v1,
};
use crate::rspace::persistence::retention::v2_format::{RetentionFormat, RetentionReader};
use crate::rspace::persistence::retention::v2_reader::{
    AppointedRetention, admit_retention_history_v2,
};
use crate::rspace::persistence::transaction::{SemanticValidityV1, ValidityOnlyV1};
use crate::rspace::persistence::{
    CATALOG_IMAGE_LEN, CATALOG_SLOT_LEN, CanonicalObjectStore, CanonicalObjectView,
    CatalogAuthorityTokenV1, CatalogSelectionV1, CatalogSlotV1, DagTransactionLimits,
    EndpointValue, GenerationLeaseError, GenerationLeaseKeyV1, GenerationLeaseV1,
    ImmutableObjectStore, IndexGenerationV1, LogPackDecodeLimits, LogPackSequence,
    MutationBatchViewV1, ObjectDigest, ObjectRefV1, OpenDagTransaction, PackId, RetainedStateV1,
    SealedDagTransaction, SealedObject, SemanticNamespace, StateVersionV1, VerifiedDagState,
};

type Core = DurableAuthorityCoreV1<TestSemanticProfile>;
type HookPoint = GenerationCheckpointPublicationHookPointV1;
type Snapshot = BTreeMap<String, Vec<u8>>;
type Published = Result<Core, IncrementalPublicationErrorV1>;
type History = (Vec<RetainedStateV1>, RetainedStateV1);
type Retry<'a> = dyn Fn(&mut Writer) -> (Published, PublicationEffects) + 'a;
type Novel = Vec<(ObjectRefV1, Vec<u8>)>;

const ORDER: [HookPoint; 18] = FORMAL_PUBLICATION_PHASE_ORDER;
const CATALOG: &str = "catalog.v1";
const UNBOUNDED: TrustedSuccessorPolicyV1 = TrustedSuccessorPolicyV1::UNBOUNDED;
const FLIPPED_SLOT_BYTE: usize = 50;
pub(super) const TORN_PREFIXES: [usize; 14] =
    [0, 1, 2, 7, 8, 63, 64, 65, 127, 128, 255, 256, 334, 335];
const KILL_FAULTS: [FaultChoice; 4] =
    [FaultChoice::Hook(6), FaultChoice::Hook(11), FaultChoice::Hook(13), FaultChoice::Torn(200)];
pub(super) const IO_FAULTS: [IoFault; 5] = [
    IoFault::PhysicalWrite,
    IoFault::PhysicalSync,
    IoFault::CatalogSlotWrite,
    IoFault::CatalogSync,
    IoFault::CatalogDirectorySync,
];
const REBUILD_FAULTS: [RebuildFault; 6] = [
    RebuildFault::CandidateWrite,
    RebuildFault::CandidateSetLength,
    RebuildFault::CandidateSync,
    RebuildFault::BeforeReplace,
    RebuildFault::AfterReplaceBeforeDirectorySync,
    RebuildFault::DirectorySync,
];
const DECOY_POINTS: [DecoyPoint; 4] = [
    DecoyPoint::BeforeBase,
    DecoyPoint::BeforeReread,
    DecoyPoint::DuringRecovery,
    DecoyPoint::ForgedReread,
];
const KILL_CASE: &str = "RSPACE_TRUSTED_HISTORY_KILL_CASE";
const KILL_ROOT: &str = "RSPACE_TRUSTED_HISTORY_KILL_ROOT";
const KILL_CHILD: &str =
    "rspace::persistence::tests::trusted_history_production::trusted_history_kill_child";
const TICKER: usize = data_slot(2, 2);
const ZERO_SLOTS: Slots = Slots {
    data: [0; DATA_SLOTS],
    continuations: [0; CONTINUATION_SLOTS],
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum IoFault {
    PhysicalWrite,
    PhysicalSync,
    CatalogSlotWrite,
    CatalogSync,
    CatalogDirectorySync,
}

impl IoFault {
    pub(super) const fn failpoint(self) -> DurableIoFailpointV1 {
        match self {
            Self::PhysicalWrite => DurableIoFailpointV1::PhysicalWrite,
            Self::PhysicalSync => DurableIoFailpointV1::PhysicalSync,
            Self::CatalogSlotWrite => DurableIoFailpointV1::CatalogSlotWrite,
            Self::CatalogSync => DurableIoFailpointV1::CatalogSync,
            Self::CatalogDirectorySync => DurableIoFailpointV1::CatalogDirectorySync,
        }
    }

    pub(super) const fn index(self) -> usize { self as usize }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RebuildFault {
    CandidateWrite,
    CandidateSetLength,
    CandidateSync,
    BeforeReplace,
    AfterReplaceBeforeDirectorySync,
    DirectorySync,
}

impl RebuildFault {
    const fn failpoint(self) -> DurableIoFailpointV1 {
        match self {
            Self::CandidateWrite => DurableIoFailpointV1::CandidateWrite,
            Self::CandidateSetLength => DurableIoFailpointV1::CandidateSetLength,
            Self::CandidateSync => DurableIoFailpointV1::CandidateSync,
            Self::BeforeReplace => DurableIoFailpointV1::BeforeReplace,
            Self::AfterReplaceBeforeDirectorySync => {
                DurableIoFailpointV1::AfterReplaceBeforeDirectorySync
            }
            Self::DirectorySync => DurableIoFailpointV1::DirectorySync,
        }
    }

    const fn index(self) -> usize { self as usize }

    const fn leaves_candidate(self) -> bool {
        matches!(
            self,
            Self::CandidateWrite |
                Self::CandidateSetLength |
                Self::CandidateSync |
                Self::BeforeReplace
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FaultChoice {
    None,
    Hook(usize),
    Io(IoFault),
    Torn(usize),
    PackShort,
    SidecarCorrupt,
    CatalogReplaced { restart: bool },
    CatalogChangedAtCompare,
    RetryWithoutRecovery(usize),
    Rebuild(RebuildFault, usize),
    TruncateSync,
    RefusalsOverPackSuffix,
    RefusalsOverIndexSuffix,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DecoyPoint {
    BeforeBase,
    BeforeReread,
    DuringRecovery,
    ForgedReread,
}

impl DecoyPoint {
    const fn index(self) -> usize { self as usize }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RefusalKind {
    Lowered(usize),
    ChangedPolicy(usize),
    ModeMismatch,
    StaleAuthority,
}

impl RefusalKind {
    const fn index(self) -> usize {
        match self {
            Self::Lowered(_) => 0,
            Self::ChangedPolicy(_) => 1,
            Self::ModeMismatch => 2,
            Self::StaleAuthority => 3,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Interleave {
    None,
    Refusal(RefusalKind),
    AllRefusals,
    Pin,
    Unpin,
    PinOverflow,
    FreshCheckpoint,
    CrashMidReclamation { index_too: bool, then_restart: bool },
    Decoy(DecoyPoint),
    IdleReplace,
    Restart { keep_reader: bool },
    BindPast(usize),
}

#[derive(Clone, Debug)]
enum Transition {
    Jump(Slots),
    Edit(Vec<SlotEdit>),
    Revert(usize),
}

#[derive(Clone, Debug)]
struct HistoryStep {
    transition: Transition,
    fault: FaultChoice,
    interleave: Interleave,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum OutcomeClass {
    NotPublished,
    IndeterminateOld,
    IndeterminateNew,
    Published,
}

impl OutcomeClass {
    pub(super) const fn index(self) -> usize { self as usize }

    pub(super) const fn kept(self) -> bool {
        matches!(self, Self::NotPublished | Self::IndeterminateOld)
    }

    const fn restart_index(self) -> usize {
        match self {
            Self::NotPublished => 0,
            Self::IndeterminateOld => 1,
            Self::IndeterminateNew | Self::Published => 2,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Failure {
    Append,
    Io,
    CompareAndSwap,
    Indeterminate,
    PublishedDirectory,
    PublishedIo,
    InvalidData,
    AuthorityChanged,
    PackTail(PackId),
}

#[derive(Clone, Copy, Debug)]
enum CloneExpectation {
    Slot(CatalogSlotV1),
    InvalidData,
    Refused,
}

enum SweepItem {
    Step(HistoryStep),
    OldSuffix(Slots, Slots),
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum ParentKind {
    Manifest,
    Node,
    Leaf,
}

struct Adopted {
    slots: Slots,
    model: ModelState,
    version: StateVersionV1,
    validity: SemanticValidityV1,
}

struct Writer {
    lease: DurableWriterLeaseV1,
    core: Core,
}

struct Reader {
    directory: DurableDirectoryV1,
    lease: GenerationLeaseV1,
    state: StateVersionV1,
    packs: Vec<(String, Vec<u8>)>,
    index: (String, Vec<u8>),
}

struct Chain {
    root: PathBuf,
    writer: Option<Writer>,
    reader: Option<Reader>,
}

#[derive(Clone, Copy)]
struct Change<'a> {
    batch: &'a [u8],
    from: &'a ModelState,
    to: &'a ModelState,
}

struct StepPlan<'a> {
    to_slots: Slots,
    change: Change<'a>,
    fault: FaultChoice,
    interleave: Interleave,
    mid_restart: Option<bool>,
}

struct AdoptedStep {
    to_slots: Slots,
    from: ModelState,
    to: ModelState,
    manifest: ObjectRefV1,
    expected: SuccessorObservation,
    novel: Novel,
    pre_history: History,
    pre_states: u64,
}

struct Settlement<'a> {
    change: Change<'a>,
    fault: FaultChoice,
    interleave: Interleave,
    mid_restart: Option<bool>,
    class: Option<OutcomeClass>,
    pre_slot: CatalogSlotV1,
    parity: usize,
    pre_observation: &'a SuccessorObservation,
    expected: &'a SuccessorObservation,
    novel: &'a [(ObjectRefV1, Vec<u8>)],
    stash: Option<Vec<u8>>,
}

struct CrashRecord {
    key: GenerationLeaseKeyV1,
    files: Vec<String>,
    then_restart: bool,
}

struct Armed {
    decoy: Rc<RefCell<Option<Snapshot>>>,
    stash: Rc<RefCell<Option<Vec<u8>>>>,
    intent: Option<Vec<u8>>,
}

#[derive(Default)]
struct Walk {
    objects: BTreeMap<ObjectDigest, (ObjectRefV1, Arc<[u8]>)>,
    shared_nodes: usize,
    content_aliases: usize,
    same_target_siblings: usize,
}

struct ScriptBuilder {
    items: Vec<SweepItem>,
    next: usize,
}

struct Trial {
    scenario: Scenario,
    live: Chain,
    reference: Chain,
    moved: PathBuf,
    decoy: Option<Snapshot>,
    base: ModelState,
    adopted: Vec<Adopted>,
    retained_union: BTreeSet<ObjectDigest>,
    oracle: CanonicalObjectStore,
    initial: Snapshot,
    foreign: [u8; CATALOG_IMAGE_LEN],
    clones: usize,
    superseded: Option<CatalogSlotV1>,
    strict: bool,
    clone_every_step: bool,
    previous_faulted: bool,
    trusted_hooks: Option<Vec<HookPoint>>,
    directory: PersistenceTrialDir,
}

#[derive(Clone, Copy, Debug, Default)]
struct Coverage {
    hooks: [u32; 13],
    io: [u32; 5],
    truncate_sync: u32,
    torn: [[u32; 2]; 14],
    torn_classes: [u32; 3],
    pack_short: u32,
    sidecar_corrupt: u32,
    catalog_replaced: [u32; 2],
    idle_replace: u32,
    changed_at_compare: u32,
    retry_without_recovery: [u32; 7],
    rebuild: [u32; 6],
    outcomes: [u32; 4],
    stale_retry: [u32; 3],
    old_suffix: u32,
    refusal_with_pack_suffix: [u32; 4],
    refusal_with_index_suffix: [u32; 3],
    refusals: [u32; 4],
    lowered_classes: [u32; 3],
    reader_across_fault: u32,
    reader_across_restart: u32,
    fresh_checkpoint: u32,
    reclamation_deferred: u32,
    reclamation_completed: u32,
    crash_mid: [u32; 2],
    pin_overflow: u32,
    decoys: [u32; 4],
    restart_idle: u32,
    restart_mid: [u32; 3],
    bind_past: [u32; 2],
    revert: u32,
    shared_node: u32,
    content_alias: u32,
    replacement: u32,
    removal: u32,
    retained_exceeds_current: u32,
    same_target_siblings: u32,
    kills: [u32; 13],
}

pub(super) fn add<const N: usize>(total: &mut [u32; N], part: [u32; N]) {
    for (total, part) in total.iter_mut().zip(part) {
        *total = total.checked_add(part).expect("a coverage count fits u32");
    }
}

pub(super) fn add_one(total: &mut u32, part: u32) { add(std::array::from_mut(total), [part]); }

pub(super) fn require(name: &str, counts: &[u32]) {
    assert!(
        counts.iter().all(|count| *count > 0),
        "no trusted history covered every {name}: {counts:?}"
    );
}

impl Coverage {
    fn merge(&mut self, other: Self) {
        add(&mut self.hooks, other.hooks);
        add(&mut self.io, other.io);
        add_one(&mut self.truncate_sync, other.truncate_sync);
        for (total, part) in self.torn.iter_mut().zip(other.torn) {
            add(total, part);
        }
        add(&mut self.torn_classes, other.torn_classes);
        add_one(&mut self.pack_short, other.pack_short);
        add_one(&mut self.sidecar_corrupt, other.sidecar_corrupt);
        add(&mut self.catalog_replaced, other.catalog_replaced);
        add_one(&mut self.idle_replace, other.idle_replace);
        add_one(&mut self.changed_at_compare, other.changed_at_compare);
        add(&mut self.retry_without_recovery, other.retry_without_recovery);
        add(&mut self.rebuild, other.rebuild);
        add(&mut self.outcomes, other.outcomes);
        add(&mut self.stale_retry, other.stale_retry);
        add_one(&mut self.old_suffix, other.old_suffix);
        add(&mut self.refusal_with_pack_suffix, other.refusal_with_pack_suffix);
        add(&mut self.refusal_with_index_suffix, other.refusal_with_index_suffix);
        add(&mut self.refusals, other.refusals);
        add(&mut self.lowered_classes, other.lowered_classes);
        add_one(&mut self.reader_across_fault, other.reader_across_fault);
        add_one(&mut self.reader_across_restart, other.reader_across_restart);
        add_one(&mut self.fresh_checkpoint, other.fresh_checkpoint);
        add_one(&mut self.reclamation_deferred, other.reclamation_deferred);
        add_one(&mut self.reclamation_completed, other.reclamation_completed);
        add(&mut self.crash_mid, other.crash_mid);
        add_one(&mut self.pin_overflow, other.pin_overflow);
        add(&mut self.decoys, other.decoys);
        add_one(&mut self.restart_idle, other.restart_idle);
        add(&mut self.restart_mid, other.restart_mid);
        add(&mut self.bind_past, other.bind_past);
        add_one(&mut self.revert, other.revert);
        add_one(&mut self.shared_node, other.shared_node);
        add_one(&mut self.content_alias, other.content_alias);
        add_one(&mut self.replacement, other.replacement);
        add_one(&mut self.removal, other.removal);
        add_one(&mut self.retained_exceeds_current, other.retained_exceeds_current);
        add_one(&mut self.same_target_siblings, other.same_target_siblings);
        add(&mut self.kills, other.kills);
    }

    fn assert_complete(&self) {
        self.assert_authority();
        require("trusted hook", &self.hooks);
        require("I/O failpoint", &self.io);
        require("truncate sync", &[self.truncate_sync]);
        require("torn prefix and parity", self.torn.as_flattened());
        require("torn class", &self.torn_classes);
        require("short pack", &[self.pack_short]);
        require("catalog changed at the compare", &[self.changed_at_compare]);
        require("retry without recovery", &self.retry_without_recovery);
        require("index rebuild failpoint", &self.rebuild);
        require("stale retry", &self.stale_retry);
        require("old suffix", &[self.old_suffix]);
        require("refusal over a pack suffix", &self.refusal_with_pack_suffix);
        require("refusal over an index suffix", &self.refusal_with_index_suffix);
        require("refusal", &self.refusals);
        require("lowered class", &self.lowered_classes);
        require("mid-publication restart", &self.restart_mid);
        require("shared node", &[self.shared_node]);
        require("replacement", &[self.replacement]);
        require("removal", &[self.removal]);
        require("kill", &self.kills);
    }

    fn assert_authority(&self) {
        require("outcome class", &self.outcomes);
        require("reader across a fault", &[self.reader_across_fault]);
        require("reader across a restart", &[self.reader_across_restart]);
        require("fresh checkpoint", &[self.fresh_checkpoint]);
        require("deferred reclamation", &[self.reclamation_deferred]);
        require("completed reclamation", &[self.reclamation_completed]);
        require("crash mid-reclamation", &self.crash_mid);
        require("pin overflow", &[self.pin_overflow]);
        require("decoy", &self.decoys);
        require("replaced catalog", &self.catalog_replaced);
        require("idle replacement", &[self.idle_replace]);
        require("idle restart", &[self.restart_idle]);
        require("bind past", &self.bind_past);
        require("revert", &[self.revert]);
        require("corrupt sidecar", &[self.sidecar_corrupt]);
        require("content alias", &[self.content_alias]);
        require("retained closure above current", &[self.retained_exceeds_current]);
        assert_eq!(
            self.same_target_siblings, 0,
            "a canonical node reached one object through two edges"
        );
    }
}

pub(super) fn failure(context: &str, detail: impl Debug) -> TestCaseError {
    TestCaseError::fail(format!("{context}: {detail:?}"))
}

fn to_usize(value: u64) -> usize { usize::try_from(value).expect("a durable length fits usize") }

fn to_u64(value: usize) -> u64 { u64::try_from(value).expect("a file length fits u64") }

fn pack_id(value: u64) -> PackId { PackId::new(value).expect("a catalog names a nonzero pack") }

fn index_generation(value: u64) -> IndexGenerationV1 {
    IndexGenerationV1::new(value).expect("a catalog names a nonzero index generation")
}

pub(super) fn index_name(slot: CatalogSlotV1) -> String {
    index_file_name(index_generation(slot.pack().index_generation()))
}

pub(super) fn active_pack_name(slot: CatalogSlotV1) -> String {
    pack_file_name(pack_id(slot.active_pack()))
}

fn old_files(slot: CatalogSlotV1, index_too: bool) -> Vec<String> {
    let pack = slot.pack();
    let mut names = Vec::with_capacity(to_usize(pack.active_pack() - pack.first_live_pack()) + 2);
    names.extend(
        (pack.first_live_pack()..=pack.active_pack()).map(|id| pack_file_name(pack_id(id))),
    );
    if index_too {
        names.push(index_name(slot));
    }
    names
}

fn read_file(path: &Path) -> Vec<u8> { fs::read(path).expect("a trial file is readable") }

fn write_file(path: &Path, bytes: &[u8]) {
    fs::write(path, bytes).expect("a trial file is writable")
}

pub(super) fn file_length(path: &Path) -> u64 {
    fs::metadata(path).expect("a trial file has metadata").len()
}

fn file_exists(path: &Path) -> bool { path.try_exists().expect("a trial path is inspectable") }

pub(super) fn read_image(root: &Path) -> [u8; CATALOG_IMAGE_LEN] {
    read_file(&root.join(CATALOG))
        .try_into()
        .expect("the catalog holds exactly two slots")
}

pub(super) fn select(image: &[u8; CATALOG_IMAGE_LEN]) -> Result<CatalogSelectionV1, TestCaseError> {
    match CatalogSelectionV1::select(image) {
        Ok(Some(selection)) => Ok(selection),
        selection => Err(failure("the catalog image selects no slot", selection)),
    }
}

pub(super) fn appointed_prefix(root: &Path, slot: CatalogSlotV1) -> Vec<u8> {
    let mut bytes = read_file(&root.join(active_pack_name(slot)));
    let tail = to_usize(slot.durable_pack_tail());
    assert!(bytes.len() >= tail, "the active pack is shorter than its appointed tail");
    bytes.truncate(tail);
    bytes
}

const fn flip_offset(inactive: usize) -> usize { inactive * CATALOG_SLOT_LEN + FLIPPED_SLOT_BYTE }

fn flip_byte(path: &Path, offset: usize) {
    let mut bytes = read_file(path);
    bytes[offset] ^= 0x01;
    write_file(path, &bytes);
}

pub(super) fn torn_image(
    pre: &[u8; CATALOG_IMAGE_LEN],
    terminal: CatalogSlotV1,
    inactive: usize,
    prefix: usize,
) -> [u8; CATALOG_IMAGE_LEN] {
    let mut image = *pre;
    let offset = inactive * CATALOG_SLOT_LEN;
    image[offset..offset + prefix].copy_from_slice(&terminal.encode()[..prefix]);
    image
}

pub(super) fn make_directory(root: &Path) {
    DirBuilder::new()
        .mode(0o700)
        .create(root)
        .expect("a trial directory is created");
}

pub(super) fn copy_snapshot(files: &Snapshot, to: &Path) {
    make_directory(to);
    for (name, bytes) in files {
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(to.join(name))
            .and_then(|mut file| file.write_all(bytes))
            .expect("a copied trial file is written");
    }
}

pub(super) fn same_files(
    context: &str,
    left: &Snapshot,
    right: &Snapshot,
) -> Result<(), TestCaseError> {
    let differing = left
        .keys()
        .chain(right.keys())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter_map(|name| match (left.get(name), right.get(name)) {
            (Some(left), Some(right)) if left == right => None,
            (left, right) => Some((name.clone(), left.map(Vec::len), right.map(Vec::len))),
        })
        .collect::<Vec<_>>();
    prop_assert!(differing.is_empty(), "{context}: differing files {differing:?}");
    Ok(())
}

fn cold_admit(root: &Path, format: RetentionFormat) -> Result<Writer, DurableAdmissionErrorV1> {
    let (lease, source) = DurableDirectoryV1::open(root)
        .expect("a trial directory opens")
        .with_retention_reader_for_test(RetentionReader::Dual)
        .with_retention_writer_for_test(format)
        .recover_and_admit_v1::<TestSemanticProfile>(
            limits(),
            LogPackDecodeLimits::UNBOUNDED,
            DurableAdmissionLimitsV1::UNBOUNDED,
        )?
        .into_parts();
    Ok(Writer {
        lease,
        core: source.into_core(),
    })
}

fn recovered(writer: &mut Writer) -> Result<Core, DurableAdmissionErrorV1> {
    let physical = recover_physical_from_lease_v1(
        &mut writer.lease,
        limits(),
        LogPackDecodeLimits::UNBOUNDED,
    )?;
    Ok(admit_physical_source::<TestSemanticProfile>(
        &writer.lease,
        physical,
        DurableAdmissionLimitsV1::UNBOUNDED,
    )?
    .into_core())
}

fn successor_sequence(core: &Core) -> LogPackSequence {
    core.authority()
        .slot()
        .commit_sequence()
        .successor()
        .expect("the commit sequence has a successor")
}

fn seal_open<'mutation>(
    mut open: OpenDagTransaction<'_, 'mutation, DurableIndexedObjectStoreV1, TestSemanticProfile>,
    change: Change<'_>,
) -> SealedDagTransaction<'mutation, TestSemanticProfile> {
    for source in staged_sources(change.from, change.to) {
        open.stage_continuation_source(source)
            .expect("a model continuation source stages");
    }
    open.seal().expect("an admissible model transition seals")
}

fn seal_on<'mutation, E: Copy>(
    state: VerifiedDagState<'_, DurableIndexedObjectStoreV1, TestSemanticProfile, E>,
    change: Change<'mutation>,
) -> SealedDagTransaction<'mutation, TestSemanticProfile> {
    let open = OpenDagTransaction::new(
        state,
        MutationBatchViewV1::parse(change.batch).expect("a model batch parses"),
        DagTransactionLimits::UNBOUNDED,
    )
    .expect("a model batch opens on its base");
    seal_open(open, change)
}

fn attempt(
    writer: &mut Writer,
    change: Change<'_>,
    policy: TrustedSuccessorPolicyV1,
) -> (Published, PublicationEffects) {
    let sealed = seal_on(writer.core.verified_state(), change);
    let bound = writer
        .core
        .bind_trusted_transaction(&writer.lease, sealed, UNBOUNDED)
        .expect("the unbounded trusted bind of a model transition succeeds");
    let prepared = writer
        .core
        .prepare_transaction(
            &writer.lease,
            &bound,
            successor_sequence(&writer.core),
            LogPackDecodeLimits::UNBOUNDED,
        )
        .expect("a trusted bind prepares");
    observed_effects(|| {
        publish_trusted_incremental_v1(&mut writer.lease, &writer.core, &prepared, policy, |_| {})
    })
}

fn untraced() -> PublicationEffects {
    PublicationEffects {
        trace: Vec::new(),
        cold_admissions: 0,
        index_constructions: 0,
    }
}

fn effects_through(hook: usize) -> PublicationEffects {
    PublicationEffects {
        trace: phases(hook),
        cold_admissions: 0,
        index_constructions: 0,
    }
}

fn expect_compare_and_swap(
    (published, effects): (Published, PublicationEffects),
) -> Result<(), TestCaseError> {
    prop_assert_eq!(effects, untraced());
    match published {
        Err(error @ IncrementalPublicationErrorV1::CatalogCompareAndSwap) => {
            prop_assert_eq!(
                error.publication_outcome(),
                GenerationCheckpointPublicationOutcomeV1::DefinitelyNotPublished
            );
            Ok(())
        }
        published => Err(failure(
            "a stale publication was not refused at the compare",
            published.map(|_| ()),
        )),
    }
}

fn read_through(core: &Core, reference: ObjectRefV1) -> Result<Option<Box<[u8]>>, TestCaseError> {
    core.get_object_bounded(&reference.digest(), reference.encoded_len())
        .map_err(|error| failure("a durable object read failed", error))
}

fn novel_objects(core: &Core, objects: &[SealedObject]) -> Result<Novel, TestCaseError> {
    let mut novel = Vec::with_capacity(objects.len());
    for object in objects {
        let reference = object.reference();
        match read_through(core, reference)? {
            Some(_) => {}
            None => novel.push((reference, object.canonical_bytes().to_vec())),
        }
    }
    prop_assert!(!novel.is_empty(), "a successor writes at least its retention object");
    Ok(novel)
}

fn check_absent(core: &Core, novel: &[(ObjectRefV1, Vec<u8>)]) -> Result<(), TestCaseError> {
    for (reference, _) in novel {
        let read = read_through(core, *reference)?;
        prop_assert!(read.is_none(), "an unpublished object {reference:?} is readable");
    }
    Ok(())
}

fn check_present(core: &Core, novel: &[(ObjectRefV1, Vec<u8>)]) -> Result<(), TestCaseError> {
    for (reference, bytes) in novel {
        let read = read_through(core, *reference)?;
        prop_assert_eq!(read.as_deref(), Some(bytes.as_slice()), "object {:?}", reference);
    }
    Ok(())
}

fn check_readable(
    core: &Core,
    objects: &BTreeMap<ObjectDigest, (ObjectRefV1, Arc<[u8]>)>,
) -> Result<(), TestCaseError> {
    for (reference, bytes) in objects.values() {
        let read = read_through(core, *reference)?;
        prop_assert_eq!(read.as_deref(), Some(&**bytes), "object {:?}", reference);
    }
    Ok(())
}

fn oracle_object(store: &CanonicalObjectStore, reference: ObjectRefV1) -> Arc<[u8]> {
    store
        .get_bounded(&reference.digest(), reference.encoded_len())
        .expect("an oracle read succeeds")
        .expect("the oracle holds every installed object")
}

fn source_object(source: &SyntheticCheckpointSource, reference: ObjectRefV1) -> &[u8] {
    source
        .objects
        .iter()
        .find_map(|(candidate, bytes)| (*candidate == reference).then_some(bytes.as_slice()))
        .expect("the checkpoint source holds every object of its state")
}

fn base_model(source: &SyntheticCheckpointSource, manifest: ObjectRefV1) -> ModelState {
    let CanonicalObjectView::SemanticManifest(view) =
        CanonicalObjectView::parse(source_object(source, manifest))
            .expect("the checkpoint manifest parses")
    else {
        panic!("a checkpoint state names a semantic manifest");
    };
    let mut state = ModelState::default();
    for (namespace, root) in view.roots().iter() {
        let CanonicalObjectView::DagNode(node) =
            CanonicalObjectView::parse(source_object(source, root))
                .expect("a namespace root parses")
        else {
            panic!("a namespace root is a DAG node");
        };
        assert!(node.children().is_empty(), "a fixture namespace root is one leaf");
        match (namespace, node.endpoint()) {
            (_, EndpointValue::Absent) => {}
            (SemanticNamespace::DataBag, EndpointValue::Multiplicity(count)) => {
                state.data.insert(node.prefix().to_vec(), count.get());
            }
            (SemanticNamespace::JoinBag, EndpointValue::Multiplicity(count)) => {
                state.joins.insert(node.prefix().to_vec(), count.get());
            }
            (SemanticNamespace::ContinuationMap, EndpointValue::Content(content)) => {
                let CanonicalObjectView::Blob(blob) =
                    CanonicalObjectView::parse(source_object(source, content))
                        .expect("a continuation source parses")
                else {
                    panic!("a continuation endpoint names a blob");
                };
                state
                    .continuations
                    .insert(node.prefix().to_vec(), blob.payload().to_vec());
            }
            (namespace, endpoint) => panic!("a fixture root holds {namespace:?} {endpoint:?}"),
        }
    }
    state
}

fn full_model(base: &ModelState, slots: &Slots) -> ModelState {
    let mut state = model_state(slots);
    for (key, count) in &base.data {
        assert!(state.data.insert(key.clone(), *count).is_none(), "a slot datum is a base key");
    }
    for (key, source) in &base.continuations {
        assert!(
            state
                .continuations
                .insert(key.clone(), source.clone())
                .is_none(),
            "a slot continuation is a base key"
        );
    }
    for (key, count) in &base.joins {
        assert!(state.joins.insert(key.clone(), *count).is_none(), "a slot join is a base key");
    }
    state
}

fn walk(store: &CanonicalObjectStore, roots: &[ObjectRefV1]) -> Walk {
    let mut stack = Vec::with_capacity(roots.len());
    stack.extend(roots.iter().map(|root| (None, *root)));
    let mut parents = BTreeMap::<ObjectDigest, BTreeSet<(ParentKind, ObjectDigest)>>::new();
    let mut blobs = BTreeSet::new();
    let mut result = Walk::default();
    while let Some((parent, reference)) = stack.pop() {
        let digest = reference.digest();
        if let Some(parent) = parent {
            parents.entry(digest).or_default().insert(parent);
        }
        let std::collections::btree_map::Entry::Vacant(entry) = result.objects.entry(digest) else {
            continue;
        };
        let bytes = oracle_object(store, reference);
        match CanonicalObjectView::parse(&bytes).expect("an oracle object parses") {
            CanonicalObjectView::SemanticManifest(view) => {
                stack.extend(
                    view.roots()
                        .iter()
                        .map(|(_, root)| (Some((ParentKind::Manifest, digest)), root)),
                );
            }
            CanonicalObjectView::DagNode(node) => {
                let mut targets = BTreeSet::new();
                for child in node.children().iter() {
                    if !targets.insert(child.target.digest()) {
                        result.same_target_siblings += 1;
                    }
                    stack.push((Some((ParentKind::Node, digest)), child.target));
                }
                if let EndpointValue::Content(content) = node.endpoint() {
                    stack.push((Some((ParentKind::Leaf, digest)), content));
                }
            }
            CanonicalObjectView::Blob(_) => {
                blobs.insert(digest);
            }
            _ => panic!("a state closure holds only manifests, nodes and blobs"),
        }
        entry.insert((reference, bytes));
    }
    for (digest, edges) in &parents {
        let blob = blobs.contains(digest);
        assert!(
            edges
                .iter()
                .all(|(kind, _)| (*kind == ParentKind::Leaf) == blob),
            "a blob is reached only through a content endpoint"
        );
    }
    result.shared_nodes = parents
        .iter()
        .filter(|(digest, edges)| {
            !blobs.contains(*digest) &&
                edges
                    .iter()
                    .filter(|(kind, _)| *kind == ParentKind::Node)
                    .count() >=
                    2
        })
        .count();
    result.content_aliases = parents
        .iter()
        .filter(|(digest, edges)| blobs.contains(*digest) && edges.len() >= 2)
        .count();
    result
}

fn retained_history(core: &Core) -> History {
    match AppointedRetention::parse(core.retention_manifest_bytes(), RetentionReader::Dual)
        .expect("the appointed retention object parses")
    {
        AppointedRetention::V1(manifest) => (
            manifest
                .historical()
                .collect::<Result<Vec<_>, _>>()
                .expect("the V1 history decodes"),
            manifest.current(),
        ),
        AppointedRetention::V2(anchor) => (
            admit_retention_history_v2(core.verified_state().object_store(), anchor)
                .expect("the V2 history admits")
                .into_parts()
                .0,
            anchor.current(),
        ),
    }
}

fn shape_changes(from: &ModelState, to: &ModelState) -> (u32, u32) {
    fn changes<V: PartialEq>(from: &BTreeMap<Vec<u8>, V>, to: &BTreeMap<Vec<u8>, V>) -> (u32, u32) {
        from.iter()
            .fold((0, 0), |(replaced, removed), (key, value)| match to.get(key) {
                Some(next) if next != value => (replaced + 1, removed),
                Some(_) => (replaced, removed),
                None => (replaced, removed + 1),
            })
    }
    [
        changes(&from.data, &to.data),
        changes(&from.continuations, &to.continuations),
        changes(&from.joins, &to.joins),
    ]
    .into_iter()
    .fold((0, 0), |(replaced, removed), (more_replaced, more_removed)| {
        (replaced + more_replaced, removed + more_removed)
    })
}

fn sweep_slots(position: usize) -> Slots {
    let mut slots = ZERO_SLOTS;
    match position % 4 {
        0 => slots.data[..data_slot(2, 0)].copy_from_slice(&[1, 1, 2, 2, 2, 2]),
        1 => slots.data[..data_slot(2, 0)].copy_from_slice(&[2, 1, 2, 2, 1, 2]),
        2 => slots.continuations[continuation_slot(2, 0)..].copy_from_slice(&[2, 0, 1, 2, 0, 0]),
        _ => slots.continuations[continuation_slot(2, 0)..].copy_from_slice(&[2, 0, 2, 2, 0, 1]),
    }
    slots.data[TICKER] = u8::try_from(position % 3).expect("a ticker value fits u8");
    slots
}

pub(super) fn phases(through: usize) -> Vec<HookPoint> { ORDER[2..=through].to_vec() }

pub(super) const fn hook_failure(hook: usize) -> Failure {
    match hook {
        0..=5 => Failure::Append,
        6..=10 => Failure::Io,
        11 => Failure::Indeterminate,
        12 => Failure::PublishedDirectory,
        _ => Failure::PublishedIo,
    }
}

fn expectation(
    fault: FaultChoice,
    interleave: Interleave,
    active: PackId,
) -> (Vec<HookPoint>, Option<Failure>) {
    match fault {
        FaultChoice::None => (
            phases(14),
            matches!(interleave, Interleave::Decoy(DecoyPoint::ForgedReread))
                .then_some(Failure::AuthorityChanged),
        ),
        FaultChoice::Hook(hook) |
        FaultChoice::RetryWithoutRecovery(hook) |
        FaultChoice::Rebuild(_, hook) => (phases(hook), Some(hook_failure(hook))),
        FaultChoice::TruncateSync | FaultChoice::RefusalsOverPackSuffix => {
            (phases(3), Some(Failure::Append))
        }
        FaultChoice::RefusalsOverIndexSuffix => (phases(5), Some(Failure::Append)),
        FaultChoice::Io(IoFault::PhysicalWrite) => (phases(1), Some(Failure::Append)),
        FaultChoice::Io(IoFault::PhysicalSync) => (phases(2), Some(Failure::Append)),
        FaultChoice::Io(IoFault::CatalogSlotWrite) | FaultChoice::Torn(_) => {
            (phases(10), Some(Failure::Indeterminate))
        }
        FaultChoice::Io(IoFault::CatalogSync) => (phases(11), Some(Failure::Indeterminate)),
        FaultChoice::Io(IoFault::CatalogDirectorySync) => {
            (phases(12), Some(Failure::PublishedDirectory))
        }
        FaultChoice::PackShort => (phases(8), Some(Failure::PackTail(active))),
        FaultChoice::SidecarCorrupt => (phases(14), Some(Failure::InvalidData)),
        FaultChoice::CatalogReplaced { .. } => (phases(14), Some(Failure::AuthorityChanged)),
        FaultChoice::CatalogChangedAtCompare => (phases(9), Some(Failure::CompareAndSwap)),
    }
}

pub(super) fn check_failure(
    error: &IncrementalPublicationErrorV1,
    expected: Failure,
    lease_path: &Path,
) -> Result<(), TestCaseError> {
    let matched = match (expected, error) {
        (Failure::Append, IncrementalPublicationErrorV1::Append(_)) |
        (Failure::Io, IncrementalPublicationErrorV1::Io(DurableIoError::Io { .. })) |
        (Failure::CompareAndSwap, IncrementalPublicationErrorV1::CatalogCompareAndSwap) |
        (Failure::Indeterminate, IncrementalPublicationErrorV1::CatalogCommitIndeterminate(_)) |
        (
            Failure::PublishedDirectory,
            IncrementalPublicationErrorV1::PublishedCatalogDirectory(_),
        ) => true,
        (Failure::PublishedIo, IncrementalPublicationErrorV1::PublishedRecovery(startup)) => {
            matches!(&**startup, DurableStartupErrorV1::Io(DurableIoError::Io { .. }))
        }
        (Failure::InvalidData, IncrementalPublicationErrorV1::PublishedRecovery(startup)) => {
            matches!(
                &**startup,
                DurableStartupErrorV1::Io(DurableIoError::Io { source, .. })
                    if source.kind() == io::ErrorKind::InvalidData
            )
        }
        (Failure::AuthorityChanged, IncrementalPublicationErrorV1::PublishedRecovery(startup)) => {
            matches!(
                &**startup,
                DurableStartupErrorV1::Io(DurableIoError::CatalogAuthorityChanged { path })
                    if path.as_path() == lease_path
            )
        }
        (
            Failure::PackTail(active),
            IncrementalPublicationErrorV1::TrustedSuccessorAssembly(assembly),
        ) => matches!(
            &**assembly,
            DurableAdmissionErrorV1::PackTail { pack, authorized, available }
                if *pack == active && available.checked_add(1) == Some(*authorized)
        ),
        _ => false,
    };
    prop_assert!(matched, "expected {:?}, got {:?}", expected, error);
    Ok(())
}

pub(super) fn classify(
    outcome: GenerationCheckpointPublicationOutcomeV1,
    disk: &[u8; CATALOG_IMAGE_LEN],
    pre_slot: CatalogSlotV1,
    terminal: Option<CatalogSlotV1>,
) -> Result<OutcomeClass, TestCaseError> {
    match outcome {
        GenerationCheckpointPublicationOutcomeV1::DefinitelyNotPublished => {
            Ok(OutcomeClass::NotPublished)
        }
        GenerationCheckpointPublicationOutcomeV1::Published => Ok(OutcomeClass::Published),
        GenerationCheckpointPublicationOutcomeV1::Indeterminate => {
            let selected = select(disk)?.slot();
            match (selected == pre_slot, Some(selected) == terminal) {
                (true, _) => Ok(OutcomeClass::IndeterminateOld),
                (false, true) => Ok(OutcomeClass::IndeterminateNew),
                (false, false) => {
                    Err(failure("an indeterminate write selected a third slot", selected))
                }
            }
        }
    }
}

const fn recovers_plainly(fault: FaultChoice) -> bool {
    matches!(
        fault,
        FaultChoice::Hook(4..=14) |
            FaultChoice::Io(
                IoFault::CatalogSlotWrite | IoFault::CatalogSync | IoFault::CatalogDirectorySync
            ) |
            FaultChoice::Torn(_) |
            FaultChoice::PackShort
    )
}

fn lowered_class(outcome: &BindOutcome) -> Result<usize, TestCaseError> {
    match outcome {
        BindOutcome::Trial(_, LedgerTrialAxisV1::FinalObjects | LedgerTrialAxisV1::FinalBytes) => {
            Ok(1)
        }
        BindOutcome::Trial(..) => Ok(0),
        BindOutcome::Ceiling(..) => Ok(2),
        outcome => Err(failure("a lowered axis refused outside the trial and ceilings", outcome)),
    }
}

fn panic_text(payload: &(dyn std::any::Any + Send)) -> String {
    match (payload.downcast_ref::<&str>(), payload.downcast_ref::<String>()) {
        (Some(text), _) => (*text).to_owned(),
        (None, Some(text)) => text.clone(),
        (None, None) => "a panic without a text payload".to_owned(),
    }
}

pub(super) fn isolated<T: Send>(
    run: impl FnOnce() -> Result<T, TestCaseError> + Send,
) -> Result<T, TestCaseError> {
    std::thread::scope(|scope| {
        scope.spawn(run).join().unwrap_or_else(|payload| {
            Err(TestCaseError::fail(format!("a trial panicked: {}", panic_text(&*payload))))
        })
    })
}

fn runner(cases: u32) -> TestRunner {
    TestRunner::new(ProptestConfig {
        cases,
        source_file: Some(file!()),
        ..ProptestConfig::default()
    })
}

impl Chain {
    fn admit(root: PathBuf, format: RetentionFormat) -> Result<Self, TestCaseError> {
        let writer = cold_admit(&root, format)
            .map_err(|error| failure("cold admission of a fresh trial failed", error))?;
        Ok(Self {
            root,
            writer: Some(writer),
            reader: None,
        })
    }

    fn writer(&self) -> &Writer { self.writer.as_ref().expect("the chain holds its writer") }

    fn writer_mut(&mut self) -> &mut Writer {
        self.writer.as_mut().expect("the chain holds its writer")
    }

    fn core(&self) -> &Core { &self.writer().core }

    fn recover(&mut self) -> Result<(), TestCaseError> {
        let writer = self.writer_mut();
        writer.core =
            recovered(writer).map_err(|error| failure("held-lease recovery failed", error))?;
        Ok(())
    }

    fn restart(&mut self, keep_reader: bool, format: RetentionFormat) -> Result<(), TestCaseError> {
        self.writer = None;
        if !keep_reader {
            self.reader = None;
        }
        let writer = cold_admit(&self.root, format)
            .map_err(|error| failure("cold admission after a restart failed", error))?;
        self.writer = Some(writer);
        Ok(())
    }

    fn pin(&mut self) -> Result<(), TestCaseError> {
        prop_assert!(self.reader.is_none(), "a chain pins one reader at a time");
        let slot = self.core().authority().slot();
        self.reader = Some(Reader::pin(&self.root, slot)?);
        Ok(())
    }

    fn checkpoint(&mut self) -> Result<(), TestCaseError> {
        let Writer { mut lease, core } = self.writer.take().expect("the chain holds its writer");
        let source = acquire_cold_checked(
            &mut lease,
            &core,
            limits(),
            LogPackDecodeLimits::UNBOUNDED,
            DurableAdmissionLimitsV1::UNBOUNDED,
        )
        .map_err(|error| failure("the cold-checked acquire failed", error))?;
        drop(core);
        let (lease, source) = SemanticallyAdmittedDurableWriterV1::from_parts(lease, source)
            .expect("an admitted source rebinds its own lease")
            .publish_generation_checkpoint_v1(
                limits(),
                LogPackDecodeLimits::UNBOUNDED,
                DurableAdmissionLimitsV1::UNBOUNDED,
            )
            .map_err(|error| failure("a fresh checkpoint failed", error))?
            .into_parts();
        self.writer = Some(Writer {
            lease,
            core: source.into_core(),
        });
        Ok(())
    }
}

impl Reader {
    fn pin(root: &Path, slot: CatalogSlotV1) -> Result<Self, TestCaseError> {
        let directory = DurableDirectoryV1::open(root).expect("a trial directory opens");
        let lease = directory
            .generation_leases()
            .acquire(GenerationLeaseKeyV1::from_catalog(slot))
            .map_err(|error| failure("a reader pin was refused", error))?;
        let pack = slot.pack();
        let mut packs =
            Vec::with_capacity(to_usize(pack.active_pack() - pack.first_live_pack()) + 1);
        for id in pack.first_live_pack()..pack.active_pack() {
            let name = pack_file_name(pack_id(id));
            let bytes = read_file(&root.join(&name));
            packs.push((name, bytes));
        }
        packs.push((active_pack_name(slot), appointed_prefix(root, slot)));
        let name = index_name(slot);
        let mut index = read_file(&root.join(&name));
        prop_assert!(
            to_u64(index.len()) >= slot.durable_index_tail(),
            "the index is shorter than its appointed tail"
        );
        index.truncate(to_usize(slot.durable_index_tail()));
        Ok(Self {
            directory,
            lease,
            state: slot.state(),
            packs,
            index: (name, index),
        })
    }

    fn check(&self, root: &Path, oracle: &CanonicalObjectStore) -> Result<(), TestCaseError> {
        for (name, prefix) in self.packs.iter().chain(std::iter::once(&self.index)) {
            let bytes = read_file(&root.join(name));
            prop_assert!(bytes.starts_with(prefix), "the pinned file {} lost its prefix", name);
        }
        let (_, index) = &self.index;
        let view = ObjectIndexFileViewV1::parse_durable_prefix(index, to_u64(index.len()))
            .map_err(|error| failure("a pinned index prefix does not parse", error))?;
        let mut records = BTreeMap::new();
        for record in view.records() {
            let record =
                record.map_err(|error| failure("a pinned index record does not parse", error))?;
            records.insert(record.reference().digest(), record);
        }
        for (reference, bytes) in walk(oracle, &[self.state.manifest()]).objects.values() {
            let record = records
                .get(&reference.digest())
                .ok_or_else(|| failure("a pinned object has no index record", reference))?;
            prop_assert_eq!(record.reference(), *reference);
            let location = record.location();
            let pack_name = pack_file_name(location.pack_id());
            let (_, pack) = self
                .packs
                .iter()
                .find(|(name, _)| *name == pack_name)
                .ok_or_else(|| {
                    failure("a pinned object lies outside the pinned packs", location)
                })?;
            let start = to_usize(location.canonical_offset());
            let end = start + to_usize(reference.encoded_len());
            prop_assert_eq!(pack.get(start..end), Some(&**bytes), "pinned object {:?}", reference);
        }
        prop_assert_eq!(
            self.directory
                .generation_leases()
                .has_active(self.lease.key()),
            Ok(true)
        );
        Ok(())
    }
}

impl Trial {
    fn open(
        scenario: Scenario,
        steps: usize,
        clone_every_step: bool,
        strict: bool,
    ) -> Result<Self, TestCaseError> {
        let directory = trial_scratch("trusted-history-");
        let image = checkpoint_image_from_source((scenario.source)());
        let live_root = directory.path().join("live");
        let reference_root = directory.path().join("reference");
        for root in [&live_root, &reference_root] {
            make_directory(root);
            install_checkpoint_image(root, &image);
        }
        let live = Chain::admit(live_root, scenario.writer)?;
        let reference = Chain::admit(reference_root, scenario.writer)?;
        prop_assert_eq!(live.core().authority().slot(), image.catalog);
        prop_assert_eq!(reference.core().authority().slot(), image.catalog);
        let manifest = image.catalog.state().manifest();
        let base = base_model(&(scenario.source)(), manifest);
        let oracle = CanonicalObjectStore::new();
        prop_assert_eq!(install(&oracle, &base), manifest);
        let initial = directory_snapshot(&live.root);
        let foreign = catalog_image(checkpoint_image_from_source((scenario.foreign)()).catalog);
        let retained_union = walk(&oracle, &[manifest]).objects.into_keys().collect();
        let mut adopted = Vec::with_capacity(steps + 1);
        adopted.push(Adopted {
            slots: ZERO_SLOTS,
            model: base.clone(),
            version: live.core().authority().slot().state(),
            validity: live.core().validity(),
        });
        Ok(Self {
            scenario,
            live,
            reference,
            moved: directory.path().join("moved"),
            decoy: None,
            base,
            adopted,
            retained_union,
            oracle,
            initial,
            foreign,
            clones: 0,
            superseded: None,
            strict,
            clone_every_step,
            previous_faulted: false,
            trusted_hooks: None,
            directory,
        })
    }

    fn last(&self) -> &Adopted { self.adopted.last().expect("a trial adopts its base state") }

    fn live_path(&self) -> &Path {
        match self.decoy {
            Some(_) => &self.moved,
            None => &self.live.root,
        }
    }

    fn sidecar_exists(&self) -> bool { file_exists(&self.live_path().join(RECLAMATION_INTENT)) }

    fn normalize_before(
        &self,
        fault: FaultChoice,
        interleave: Interleave,
        empty: bool,
        to: &ModelState,
    ) -> (FaultChoice, Interleave) {
        let fault = match empty {
            true => FaultChoice::None,
            false => fault,
        };
        let sidecar = self.sidecar_exists();
        let reader = self.live.reader.is_some();
        let interleave = match interleave {
            Interleave::Refusal(_) |
            Interleave::AllRefusals |
            Interleave::Decoy(DecoyPoint::BeforeBase)
                if empty =>
            {
                Interleave::None
            }
            Interleave::Decoy(DecoyPoint::BeforeReread | DecoyPoint::ForgedReread)
                if empty || fault != FaultChoice::None =>
            {
                Interleave::None
            }
            Interleave::Decoy(DecoyPoint::ForgedReread) | Interleave::FreshCheckpoint
                if sidecar =>
            {
                Interleave::None
            }
            Interleave::Decoy(DecoyPoint::DuringRecovery)
                if matches!(
                    fault,
                    FaultChoice::CatalogReplaced { .. } |
                        FaultChoice::Rebuild(..) |
                        FaultChoice::TruncateSync |
                        FaultChoice::RefusalsOverPackSuffix |
                        FaultChoice::RefusalsOverIndexSuffix
                ) =>
            {
                Interleave::None
            }
            Interleave::CrashMidReclamation { .. } if !sidecar => Interleave::None,
            Interleave::Pin if reader => Interleave::None,
            Interleave::Unpin if !reader => Interleave::None,
            Interleave::Restart { keep_reader: true } if !reader => {
                Interleave::Restart { keep_reader: false }
            }
            Interleave::BindPast(past) => match self.adopted.len() {
                0 | 1 => Interleave::None,
                len => match past % (len - 1) {
                    index if self.adopted[index].model == *to => Interleave::None,
                    index => Interleave::BindPast(index),
                },
            },
            interleave => interleave,
        };
        (fault, interleave)
    }

    fn normalize_after(&self, fault: FaultChoice) -> FaultChoice {
        match (fault, self.sidecar_exists()) {
            (FaultChoice::SidecarCorrupt, false) | (FaultChoice::CatalogReplaced { .. }, true) => {
                FaultChoice::None
            }
            (fault, _) => fault,
        }
    }

    fn run_step(
        &mut self,
        step: &HistoryStep,
        coverage: &mut Coverage,
    ) -> Result<(), TestCaseError> {
        let mut seen = Coverage::default();
        let to_slots = match &step.transition {
            Transition::Jump(slots) => *slots,
            Transition::Edit(edits) => {
                let mut slots = self.last().slots;
                for edit in edits {
                    edit.apply(&mut slots);
                }
                slots
            }
            Transition::Revert(past) => self.adopted[past % self.adopted.len()].slots,
        };
        let from = self.last().model.clone();
        let to = full_model(&self.base, &to_slots);
        let base_version = self.live.core().authority().slot().state();
        let batch = transition_batch(base_version, &from, &to);
        let pending_at_start = self.sidecar_exists();
        let (fault, interleave) =
            self.normalize_before(step.fault, step.interleave, batch.is_none(), &to);
        let change = batch.as_deref().map(|batch| Change {
            batch,
            from: &from,
            to: &to,
        });
        let crash = self.run_interleave(interleave, change, &to, &mut seen)?;
        prop_assert_eq!(
            self.live.core().authority().slot().state(),
            base_version,
            "an interleave changed the state version"
        );
        let mut restarted = matches!(
            crash,
            Some(CrashRecord {
                then_restart: true,
                ..
            })
        );
        let mid_restart = match interleave {
            Interleave::Restart { keep_reader } if recovers_plainly(fault) => Some(keep_reader),
            Interleave::Restart { keep_reader } => {
                self.idle_restart(keep_reader, &mut seen)?;
                restarted = true;
                None
            }
            _ => None,
        };
        let fault = self.normalize_after(fault);
        if self.strict {
            prop_assert_eq!((fault, interleave), (step.fault, step.interleave));
        }
        let record = match change {
            None => {
                self.complete_without_publication(interleave, &mut seen)?;
                None
            }
            Some(change) => Some(self.publish_step(
                StepPlan {
                    to_slots,
                    change,
                    fault,
                    interleave,
                    mid_restart,
                },
                &mut seen,
                &mut restarted,
            )?),
        };
        self.finish_step(record, crash, pending_at_start, restarted, fault, &mut seen)?;
        coverage.merge(seen);
        Ok(())
    }

    fn run_interleave(
        &mut self,
        interleave: Interleave,
        change: Option<Change<'_>>,
        to: &ModelState,
        seen: &mut Coverage,
    ) -> Result<Option<CrashRecord>, TestCaseError> {
        match (interleave, change) {
            (Interleave::Refusal(kind), Some(change)) => {
                let classes = self.refuse(kind, change)?;
                add(&mut seen.lowered_classes, classes);
                seen.refusals[kind.index()] += 1;
            }
            (Interleave::AllRefusals, Some(change)) => {
                let before = directory_snapshot(&self.live.root);
                let (classes, count) = self.check_lowered(change, None)?;
                same_files(
                    "a lowered bind changed the store",
                    &before,
                    &directory_snapshot(&self.live.root),
                )?;
                add(&mut seen.lowered_classes, classes);
                seen.refusals[RefusalKind::Lowered(0).index()] += count;
                for kind in [
                    RefusalKind::ChangedPolicy(2),
                    RefusalKind::ModeMismatch,
                    RefusalKind::StaleAuthority,
                ] {
                    self.refuse(kind, change)?;
                    seen.refusals[kind.index()] += 1;
                }
            }
            (Interleave::Refusal(_) | Interleave::AllRefusals, None) => {
                return Err(TestCaseError::fail("a refusal needs a non-empty transition"));
            }
            (Interleave::Pin, _) => {
                self.live.pin()?;
                self.reference.pin()?;
            }
            (Interleave::Unpin, _) => {
                self.live.reader = None;
                self.reference.reader = None;
            }
            (Interleave::PinOverflow, _) => {
                self.pin_overflow()?;
                seen.pin_overflow += 1;
            }
            (Interleave::FreshCheckpoint, _) => self.fresh_checkpoint(seen)?,
            (
                Interleave::CrashMidReclamation {
                    index_too,
                    then_restart,
                },
                _,
            ) => {
                return self.crash_mid(index_too, then_restart).map(Some);
            }
            (Interleave::IdleReplace, change) => {
                self.idle_replace(change)?;
                seen.idle_replace += 1;
            }
            (Interleave::BindPast(past), _) => self.bind_past(past, to, seen)?,
            (Interleave::None | Interleave::Decoy(_) | Interleave::Restart { .. }, _) => {}
        }
        Ok(None)
    }

    fn refuse(&mut self, kind: RefusalKind, change: Change<'_>) -> Result<[u32; 3], TestCaseError> {
        let before = directory_snapshot(&self.live.root);
        let classes = match kind {
            RefusalKind::Lowered(index) => self.check_lowered(change, Some(index))?.0,
            RefusalKind::ChangedPolicy(index) => {
                let axes = policy_axes();
                let policy = policy_with(UNBOUNDED, axes[index % axes.len()], u64::MAX - 1);
                let (published, effects) = attempt(self.live.writer_mut(), change, policy);
                let PublicationEffects {
                    trace,
                    cold_admissions,
                    index_constructions,
                } = effects;
                prop_assert_eq!((cold_admissions, index_constructions), (0, 0));
                prop_assert_eq!(
                    preparation_refusal((published, trace))?,
                    PreflightOutcome::TrustedPolicyMismatch
                );
                [0; 3]
            }
            RefusalKind::ModeMismatch => {
                self.refuse_cold_binding(change)?;
                return Ok([0; 3]);
            }
            RefusalKind::StaleAuthority => {
                let catalog = self.live.root.join(CATALOG);
                let offset = flip_offset(
                    self.live
                        .core()
                        .authority()
                        .selection()
                        .inactive_slot_index(),
                );
                flip_byte(&catalog, offset);
                let attempted = attempt(self.live.writer_mut(), change, UNBOUNDED);
                flip_byte(&catalog, offset);
                expect_compare_and_swap(attempted)?;
                [0; 3]
            }
        };
        same_files("a refusal changed the store", &before, &directory_snapshot(&self.live.root))?;
        Ok(classes)
    }

    fn refuse_cold_binding(&mut self, change: Change<'_>) -> Result<(), TestCaseError> {
        let root = self.live.root.clone();
        let writer = self.live.writer_mut();
        let source = acquire_cold_checked(
            &mut writer.lease,
            &writer.core,
            limits(),
            LogPackDecodeLimits::UNBOUNDED,
            DurableAdmissionLimitsV1::UNBOUNDED,
        )
        .map_err(|error| failure("the cold-checked acquire failed", error))?;
        let before = directory_snapshot(&root);
        let open = source
            .open_transaction(
                MutationBatchViewV1::parse(change.batch).expect("a model batch parses"),
                DagTransactionLimits::UNBOUNDED,
            )
            .expect("a model batch opens on the cold-checked source");
        let cold = source
            .bind_durable_transaction(
                &writer.lease,
                seal_open(open, change),
                DurableAdmissionLimitsV1::UNBOUNDED,
            )
            .expect("a cold binding of a model transition succeeds");
        let prepared = source
            .prepare_transaction(
                &writer.lease,
                &cold,
                successor_sequence(source.core()),
                LogPackDecodeLimits::UNBOUNDED,
            )
            .expect("a cold binding prepares");
        let refused = preparation_refusal(traced(|| {
            publish_trusted_incremental_v1(
                &mut writer.lease,
                source.core(),
                &prepared,
                UNBOUNDED,
                |_| {},
            )
        }))?;
        prop_assert_eq!(refused, PreflightOutcome::ModeMismatch);
        same_files("a cold-bound refusal changed the store", &before, &directory_snapshot(&root))
    }

    fn check_lowered(
        &self,
        change: Change<'_>,
        select: Option<usize>,
    ) -> Result<([u32; 3], u32), TestCaseError> {
        let writer = self.live.writer();
        let lowered = TrustedPolicyEvidence::measure(
            &writer.lease,
            &writer.core,
            seal_on(writer.core.verified_state(), change),
        )
        .lowered();
        prop_assert!(!lowered.is_empty(), "a model transition has no lowered policy axis");
        let range = match select {
            Some(index) => {
                let index = index % lowered.len();
                index..index + 1
            }
            None => 0..lowered.len(),
        };
        let selected = &lowered[range];
        let mut classes = [0; 3];
        for (_, policy, expected) in selected {
            let outcome = bind_outcome(writer.core.bind_trusted_transaction(
                &writer.lease,
                seal_on(writer.core.verified_state(), change),
                *policy,
            ));
            prop_assert_eq!(&outcome, expected);
            classes[lowered_class(&outcome)?] += 1;
        }
        Ok((classes, u32::try_from(selected.len()).expect("a lowered axis count fits u32")))
    }

    fn pin_overflow(&self) -> Result<(), TestCaseError> {
        let before = directory_snapshot(&self.live.root);
        let key = GenerationLeaseKeyV1::from_catalog(self.live.core().authority().slot());
        let registry = self.live.writer().lease.generation_leases();
        let active = registry.has_active(key);
        let counter = registry.replace_lease_counter_for_test(u64::MAX);
        let refused = DurableDirectoryV1::open(&self.live.root)
            .expect("a trial directory opens")
            .generation_leases()
            .acquire(key)
            .err();
        prop_assert_eq!(registry.replace_lease_counter_for_test(counter), u64::MAX);
        prop_assert_eq!(refused, Some(GenerationLeaseError::CounterOverflow));
        prop_assert_eq!(registry.has_active(key), active);
        same_files("a refused pin changed the store", &before, &directory_snapshot(&self.live.root))
    }

    fn fresh_checkpoint(&mut self, seen: &mut Coverage) -> Result<(), TestCaseError> {
        let observation = core_observation(self.live.core());
        let superseded = self.live.core().authority().slot();
        self.live.checkpoint()?;
        self.reference.checkpoint()?;
        prop_assert_eq!(core_observation(self.live.core()), observation);
        self.superseded = Some(superseded);
        let present = old_files(superseded, true)
            .iter()
            .filter(|name| file_exists(&self.live.root.join(name)))
            .count();
        match (self.sidecar_exists(), self.live.reader.is_some()) {
            (true, true) => {
                prop_assert_eq!(present, old_files(superseded, true).len());
                seen.reclamation_deferred += 1;
            }
            (false, false) => prop_assert_eq!(present, 0),
            state => return Err(failure("a fresh checkpoint left a mixed reclamation", state)),
        }
        seen.fresh_checkpoint += 1;
        Ok(())
    }

    fn crash_mid(
        &mut self,
        index_too: bool,
        then_restart: bool,
    ) -> Result<CrashRecord, TestCaseError> {
        let superseded = self
            .superseded
            .ok_or_else(|| TestCaseError::fail("a pending reclamation has no superseded slot"))?;
        self.live.reader = None;
        self.reference.reader = None;
        for name in old_files(superseded, index_too) {
            fs::remove_file(self.live.root.join(name)).expect("a superseded file is removed");
        }
        prop_assert!(self.sidecar_exists(), "the crash removed the reclamation intent");
        if then_restart {
            self.live.restart(false, self.scenario.writer)?;
            self.reference.recover()?;
        }
        Ok(CrashRecord {
            key: GenerationLeaseKeyV1::from_catalog(superseded),
            files: old_files(superseded, true),
            then_restart,
        })
    }

    fn idle_replace(&mut self, change: Option<Change<'_>>) -> Result<(), TestCaseError> {
        let catalog = self.live.root.join(CATALOG);
        let saved = read_file(&catalog);
        write_file(&catalog, &self.foreign);
        let before = directory_snapshot(&self.live.root);
        if let Some(change) = change {
            expect_compare_and_swap(attempt(self.live.writer_mut(), change, UNBOUNDED))?;
        }
        prop_assert!(
            recovered(self.live.writer_mut()).is_err(),
            "held-lease recovery adopted a foreign catalog"
        );
        same_files(
            "a foreign catalog changed the store",
            &before,
            &directory_snapshot(&self.live.root),
        )?;
        write_file(&catalog, &saved);
        let observation = core_observation(self.live.core());
        self.live.recover()?;
        prop_assert_eq!(core_observation(self.live.core()), observation);
        self.reference.recover()
    }

    fn bind_past(
        &self,
        past: usize,
        to: &ModelState,
        seen: &mut Coverage,
    ) -> Result<(), TestCaseError> {
        let past = &self.adopted[past];
        let batch = transition_batch(past.version, &past.model, to)
            .ok_or_else(|| TestCaseError::fail("a past model equals the step target"))?;
        let writer = self.live.writer();
        let state = VerifiedDagState::<_, TestSemanticProfile, ValidityOnlyV1>::from_validity(
            writer.core.verified_state().object_store(),
            past.version,
            past.validity,
        )
        .expect("a retained state's validity names its manifest");
        let sealed = seal_on(state, Change {
            batch: &batch,
            from: &past.model,
            to,
        });
        prop_assert_eq!(
            bind_outcome(
                writer
                    .core
                    .bind_trusted_transaction(&writer.lease, sealed, UNBOUNDED)
            ),
            BindOutcome::AuthorityMismatch
        );
        seen.bind_past[usize::from(self.previous_faulted)] += 1;
        Ok(())
    }

    fn restart_both(&mut self, keep_reader: bool) -> Result<(), TestCaseError> {
        self.live.restart(keep_reader, self.scenario.writer)?;
        if !keep_reader {
            self.reference.reader = None;
        }
        self.reference.recover()
    }

    fn idle_restart(
        &mut self,
        keep_reader: bool,
        seen: &mut Coverage,
    ) -> Result<(), TestCaseError> {
        let observation = core_observation(self.live.core());
        self.restart_both(keep_reader)?;
        prop_assert_eq!(core_observation(self.live.core()), observation);
        seen.restart_idle += 1;
        Ok(())
    }

    fn place_decoy(&mut self, files: Snapshot) {
        fs::rename(&self.live.root, &self.moved).expect("the live directory moves aside");
        copy_snapshot(&files, &self.live.root);
        self.decoy = Some(files);
    }

    fn remove_decoy(&mut self) -> Result<(), TestCaseError> {
        let files = self.decoy.take().expect("a decoy is in place");
        same_files("a decoy was written", &files, &directory_snapshot(&self.live.root))?;
        fs::remove_dir_all(&self.live.root).expect("the decoy directory is removed");
        fs::rename(&self.moved, &self.live.root).expect("the live directory moves back");
        Ok(())
    }

    fn recover_under_decoy(
        &mut self,
        mirror: bool,
        seen: &mut Coverage,
    ) -> Result<(), TestCaseError> {
        let observation = core_observation(self.live.core());
        self.place_decoy(self.initial.clone());
        let recovery = self.live.recover();
        self.remove_decoy()?;
        recovery?;
        if mirror {
            prop_assert_eq!(core_observation(self.live.core()), observation);
            self.reference.recover()?;
        }
        seen.decoys[DecoyPoint::DuringRecovery.index()] += 1;
        Ok(())
    }

    fn complete_without_publication(
        &mut self,
        interleave: Interleave,
        seen: &mut Coverage,
    ) -> Result<(), TestCaseError> {
        match interleave {
            Interleave::CrashMidReclamation {
                then_restart: false,
                ..
            } => {
                self.live.recover()?;
                self.reference.recover()
            }
            Interleave::Decoy(DecoyPoint::DuringRecovery) => self.recover_under_decoy(true, seen),
            _ => Ok(()),
        }
    }
}

impl Trial {
    fn arm(
        &self,
        fault: FaultChoice,
        interleave: Interleave,
        pre: &CatalogAuthorityTokenV1,
    ) -> Armed {
        let root = self.live_path().to_path_buf();
        let decoy: Rc<RefCell<Option<Snapshot>>> = Rc::default();
        let stash: Rc<RefCell<Option<Vec<u8>>>> = Rc::default();
        let mut intent = None;
        match fault {
            FaultChoice::None => {}
            FaultChoice::Hook(hook) |
            FaultChoice::RetryWithoutRecovery(hook) |
            FaultChoice::Rebuild(_, hook) => {
                arm_generation_checkpoint_publication_failpoint(ORDER[hook])
            }
            FaultChoice::TruncateSync | FaultChoice::RefusalsOverPackSuffix => {
                arm_generation_checkpoint_publication_failpoint(ORDER[3]);
            }
            FaultChoice::RefusalsOverIndexSuffix => {
                arm_generation_checkpoint_publication_failpoint(ORDER[5]);
            }
            FaultChoice::Io(io) => arm_durable_io_failpoint(io.failpoint()),
            FaultChoice::Torn(prefix) => arm_catalog_slot_write_prefix(prefix),
            FaultChoice::PackShort => {
                let pack = root.join(active_pack_name(pre.slot()));
                arm_generation_checkpoint_publication_hook(ORDER[8], move || {
                    let file = OpenOptions::new()
                        .write(true)
                        .open(&pack)
                        .expect("the active pack opens for writing");
                    let length = file.metadata().expect("the active pack has metadata").len();
                    file.set_len(length.checked_sub(1).expect("the active pack is not empty"))
                        .expect("the active pack is shortened");
                    file.sync_all().expect("the shortened pack syncs");
                });
            }
            FaultChoice::SidecarCorrupt => {
                let sidecar = root.join(RECLAMATION_INTENT);
                let bytes = read_file(&sidecar);
                let mut corrupt = bytes.clone();
                *corrupt
                    .last_mut()
                    .expect("a reclamation intent is not empty") ^= 0xff;
                intent = Some(bytes);
                arm_generation_checkpoint_publication_hook(ORDER[14], move || {
                    write_file(&sidecar, &corrupt);
                });
            }
            FaultChoice::CatalogReplaced { .. } => {
                let catalog = root.join(CATALOG);
                let foreign = self.foreign;
                let stash = Rc::clone(&stash);
                arm_generation_checkpoint_publication_hook(ORDER[14], move || {
                    *stash.borrow_mut() = Some(read_file(&catalog));
                    write_file(&catalog, &foreign);
                });
            }
            FaultChoice::CatalogChangedAtCompare => {
                let catalog = root.join(CATALOG);
                let offset = flip_offset(pre.selection().inactive_slot_index());
                arm_generation_checkpoint_publication_hook(ORDER[9], move || {
                    flip_byte(&catalog, offset);
                });
            }
        }
        match interleave {
            Interleave::Decoy(DecoyPoint::BeforeReread) => {
                let live = self.live.root.clone();
                let moved = self.moved.clone();
                let initial = self.initial.clone();
                let decoy = Rc::clone(&decoy);
                arm_generation_checkpoint_publication_hook(ORDER[14], move || {
                    fs::rename(&live, &moved).expect("the live directory moves aside");
                    copy_snapshot(&initial, &live);
                    *decoy.borrow_mut() = Some(initial);
                });
            }
            Interleave::Decoy(DecoyPoint::ForgedReread) => {
                let live = self.live.root.clone();
                let moved = self.moved.clone();
                let image = *pre.image();
                let decoy = Rc::clone(&decoy);
                let stash = Rc::clone(&stash);
                arm_generation_checkpoint_publication_hook(ORDER[14], move || {
                    let catalog = live.join(CATALOG);
                    let successor = read_file(&catalog);
                    write_file(&catalog, &image);
                    fs::rename(&live, &moved).expect("the live directory moves aside");
                    let mut forged = directory_snapshot(&moved);
                    forged.insert(CATALOG.to_owned(), successor.clone());
                    copy_snapshot(&forged, &live);
                    *stash.borrow_mut() = Some(successor);
                    *decoy.borrow_mut() = Some(forged);
                });
            }
            _ => {}
        }
        Armed {
            decoy,
            stash,
            intent,
        }
    }

    fn publish_reference(&mut self, change: Change<'_>) -> Result<CatalogSlotV1, TestCaseError> {
        let writer = self.reference.writer_mut();
        let (published, effects) = attempt(writer, change, UNBOUNDED);
        prop_assert_eq!(effects, PublicationEffects::trusted());
        writer.core =
            published.map_err(|error| failure("the reference publication failed", error))?;
        Ok(writer.core.authority().slot())
    }

    fn publish_step(
        &mut self,
        plan: StepPlan<'_>,
        seen: &mut Coverage,
        restarted: &mut bool,
    ) -> Result<AdoptedStep, TestCaseError> {
        let StepPlan {
            to_slots,
            change,
            fault,
            interleave,
            mid_restart,
        } = plan;
        let manifest = install(&self.oracle, change.to);
        let reference_slot = self.publish_reference(change)?;
        let pre = self.live.core().authority().clone();
        let pre_slot = pre.slot();
        let parity = pre.selection().inactive_slot_index();
        let pre_observation = core_observation(self.live.core());
        let pre_prefix = appointed_prefix(&self.live.root, pre_slot);
        let pre_history = retained_history(self.live.core());
        let pre_states = self.live.core().retained_states();
        prop_assert_eq!(read_image(&self.live.root), *pre.image());
        prop_assert_eq!(
            file_length(&self.live.root.join(index_name(pre_slot))),
            pre_slot.durable_index_tail()
        );
        let writer = self.live.writer();
        let lease_path = writer.lease.directory().path().to_path_buf();
        let bound = writer
            .core
            .bind_trusted_transaction(
                &writer.lease,
                seal_on(writer.core.verified_state(), change),
                UNBOUNDED,
            )
            .expect("the unbounded trusted bind of a model transition succeeds");
        prop_assert_eq!(bound.receipt().target().manifest(), manifest);
        let expected = trusted_observation(&bound);
        let novel = novel_objects(&writer.core, bound.objects())?;
        let prepared = writer
            .core
            .prepare_transaction(
                &writer.lease,
                &bound,
                successor_sequence(&writer.core),
                LogPackDecodeLimits::UNBOUNDED,
            )
            .expect("a trusted bind prepares");
        if interleave == Interleave::Decoy(DecoyPoint::BeforeBase) {
            self.place_decoy(self.initial.clone());
        }
        let armed = self.arm(fault, interleave, &pre);
        let terminal = Cell::new(None);
        let writer = self.live.writer_mut();
        let (published, effects) = observed_effects(|| {
            publish_trusted_incremental_v1(
                &mut writer.lease,
                &writer.core,
                &prepared,
                UNBOUNDED,
                |slot| terminal.set(Some(slot)),
            )
        });
        if let Some(decoy) = armed.decoy.take() {
            self.decoy = Some(decoy);
        }
        let root = self.live_path().to_path_buf();
        let disk = read_image(&root);
        let captured = terminal.get();
        let PublicationEffects {
            trace,
            cold_admissions,
            index_constructions,
        } = effects;
        prop_assert_eq!(captured.is_some(), trace.contains(&ORDER[6]));
        if let Some(captured) = captured {
            prop_assert_eq!(captured, reference_slot);
        }
        let (expected_trace, expected_failure) =
            expectation(fault, interleave, pack_id(pre_slot.active_pack()));
        prop_assert_eq!(&trace, &expected_trace);
        prop_assert_eq!((cold_admissions, index_constructions), (0, 0));
        let class = match (published, expected_failure) {
            (Ok(core), None) => {
                self.live.writer_mut().core = core;
                None
            }
            (Err(error), Some(kind)) => {
                check_failure(&error, kind, &lease_path)?;
                Some(classify(error.publication_outcome(), &disk, pre_slot, captured)?)
            }
            (published, kind) => {
                return Err(failure(
                    "the publication differs from its expectation",
                    (published.map(|_| ()), kind),
                ));
            }
        };
        let torn = match fault {
            FaultChoice::Torn(prefix) => {
                let terminal = captured.ok_or_else(|| {
                    TestCaseError::fail("a torn catalog write captured no terminal slot")
                })?;
                prop_assert_eq!(disk, torn_image(pre.image(), terminal, parity, prefix));
                let class_index = match (disk != *pre.image(), class) {
                    (false, Some(OutcomeClass::IndeterminateOld)) => 0,
                    (true, Some(OutcomeClass::IndeterminateOld)) => 1,
                    (true, Some(OutcomeClass::IndeterminateNew)) => 2,
                    state => {
                        return Err(failure(
                            "a torn catalog write reached an impossible class",
                            state,
                        ));
                    }
                };
                Some((
                    TORN_PREFIXES
                        .iter()
                        .position(|candidate| *candidate == prefix),
                    class_index,
                ))
            }
            _ => None,
        };
        if !matches!(fault, FaultChoice::CatalogReplaced { .. }) {
            let selected = select(&disk)?.slot();
            let prefix = appointed_prefix(&root, selected);
            if selected == pre_slot {
                prop_assert_eq!(prefix, pre_prefix);
            } else {
                prop_assert_eq!(selected, reference_slot);
                prop_assert_eq!(prefix, appointed_prefix(&self.reference.root, reference_slot));
            }
        }
        let clone_expectation = match (fault, interleave, class) {
            (FaultChoice::SidecarCorrupt, ..) => CloneExpectation::InvalidData,
            (FaultChoice::CatalogReplaced { .. }, ..) => CloneExpectation::Refused,
            (_, Interleave::Decoy(DecoyPoint::ForgedReread), _) => CloneExpectation::Slot(pre_slot),
            (_, _, Some(class)) if class.kept() => CloneExpectation::Slot(pre_slot),
            _ => CloneExpectation::Slot(reference_slot),
        };
        let clone_observation = if self.clone_every_step || class.is_some() {
            self.check_clone(clone_expectation, &novel, pre_slot)?
        } else {
            None
        };
        if let Interleave::Decoy(
            point @ (DecoyPoint::BeforeBase | DecoyPoint::BeforeReread | DecoyPoint::ForgedReread),
        ) = interleave
        {
            self.remove_decoy()?;
            seen.decoys[point.index()] += 1;
        }
        if let Some(intent) = &armed.intent {
            write_file(&self.live.root.join(RECLAMATION_INTENT), intent);
        }
        let retry = |writer: &mut Writer| {
            observed_effects(|| {
                publish_trusted_incremental_v1(
                    &mut writer.lease,
                    &writer.core,
                    &prepared,
                    UNBOUNDED,
                    |_| {},
                )
            })
        };
        match (class, interleave) {
            (None | Some(OutcomeClass::NotPublished), _) |
            (_, Interleave::Decoy(DecoyPoint::ForgedReread)) => {}
            (Some(class), _) => self.stale_retry(&retry, class, &pre, seen)?,
        }
        let stash = armed.stash.take();
        let settled = self.settle(
            Settlement {
                change,
                fault,
                interleave,
                mid_restart,
                class,
                pre_slot,
                parity,
                pre_observation: &pre_observation,
                expected: &expected,
                novel: &novel,
                stash,
            },
            &retry,
            seen,
            restarted,
        )?;
        match (&clone_observation, interleave) {
            (None, _) | (Some(_), Interleave::Decoy(DecoyPoint::ForgedReread)) => {}
            (Some(observation), _) => prop_assert_eq!(observation, &settled),
        }
        match fault {
            FaultChoice::Hook(hook) => seen.hooks[hook - 2] += 1,
            FaultChoice::Io(io) => seen.io[io.index()] += 1,
            FaultChoice::PackShort => seen.pack_short += 1,
            FaultChoice::SidecarCorrupt => seen.sidecar_corrupt += 1,
            FaultChoice::CatalogReplaced { restart } => {
                seen.catalog_replaced[usize::from(restart)] += 1;
            }
            FaultChoice::CatalogChangedAtCompare => seen.changed_at_compare += 1,
            FaultChoice::RetryWithoutRecovery(hook) => seen.retry_without_recovery[hook - 4] += 1,
            FaultChoice::Rebuild(rebuild, _) => seen.rebuild[rebuild.index()] += 1,
            FaultChoice::TruncateSync => seen.truncate_sync += 1,
            FaultChoice::None |
            FaultChoice::Torn(_) |
            FaultChoice::RefusalsOverPackSuffix |
            FaultChoice::RefusalsOverIndexSuffix => {}
        }
        if let Some((position, class_index)) = torn {
            seen.torn_classes[class_index] += 1;
            if let Some(position) = position {
                seen.torn[position][parity] += 1;
            }
        }
        if let Some(class) = class {
            seen.outcomes[class.index()] += 1;
        }
        if self.trusted_hooks.is_none() && fault == FaultChoice::None && class.is_none() {
            self.trusted_hooks = Some(trace);
        }
        Ok(AdoptedStep {
            to_slots,
            from: change.from.clone(),
            to: change.to.clone(),
            manifest,
            expected,
            novel,
            pre_history,
            pre_states,
        })
    }

    fn check_clone(
        &mut self,
        expectation: CloneExpectation,
        novel: &[(ObjectRefV1, Vec<u8>)],
        pre_slot: CatalogSlotV1,
    ) -> Result<Option<SuccessorObservation>, TestCaseError> {
        let clone = self.directory.path().join(format!("clone-{}", self.clones));
        self.clones += 1;
        copy_snapshot(&directory_snapshot(self.live_path()), &clone);
        let observation = match (expectation, cold_admit(&clone, self.scenario.writer)) {
            (CloneExpectation::Slot(slot), Ok(writer)) => {
                prop_assert_eq!(writer.core.authority().slot(), slot);
                prop_assert!(
                    !writer
                        .lease
                        .generation_leases()
                        .shares_registry(&self.live.writer().lease.generation_leases()),
                    "a clone shares the live lease registry"
                );
                check_readable(
                    &writer.core,
                    &walk(&self.oracle, &[slot.state().manifest()]).objects,
                )?;
                if slot == pre_slot {
                    check_absent(&writer.core, novel)?;
                }
                Some(core_observation(&writer.core))
            }
            (
                CloneExpectation::InvalidData,
                Err(DurableAdmissionErrorV1::Startup(DurableStartupErrorV1::Io(
                    DurableIoError::Io { source, .. },
                ))),
            ) if source.kind() == io::ErrorKind::InvalidData => None,
            (CloneExpectation::Refused, Err(error)) => {
                println!("trusted history clone refused a replaced catalog: {error:?}");
                None
            }
            (expectation, admitted) => {
                return Err(failure(
                    "the clone admission differs from its expectation",
                    (expectation, admitted.map(|writer| writer.core.authority().slot())),
                ));
            }
        };
        fs::remove_dir_all(&clone).expect("a clone directory is removed");
        Ok(observation)
    }

    fn stale_retry(
        &mut self,
        retry: &Retry<'_>,
        class: OutcomeClass,
        pre: &CatalogAuthorityTokenV1,
        seen: &mut Coverage,
    ) -> Result<(), TestCaseError> {
        let changed = read_image(&self.live.root) != *pre.image();
        let before = directory_snapshot(&self.live.root);
        let (published, effects) = retry(self.live.writer_mut());
        let after = directory_snapshot(&self.live.root);
        if changed {
            expect_compare_and_swap((published, effects))?;
            same_files("a stale retry changed the store", &before, &after)?;
            seen.stale_retry[usize::from(class == OutcomeClass::IndeterminateOld)] += 1;
            return Ok(());
        }
        prop_assert_eq!(class, OutcomeClass::IndeterminateOld);
        prop_assert_eq!(effects, effects_through(3));
        match published {
            Err(error @ IncrementalPublicationErrorV1::Append(_)) => prop_assert_eq!(
                error.publication_outcome(),
                GenerationCheckpointPublicationOutcomeV1::DefinitelyNotPublished
            ),
            published => {
                return Err(failure(
                    "a stale retry over an unchanged catalog passed the index check",
                    published.map(|_| ()),
                ));
            }
        }
        let index = index_name(pre.slot());
        prop_assert_eq!(after.get(CATALOG), before.get(CATALOG));
        prop_assert_eq!(after.get(&index), before.get(&index));
        seen.stale_retry[2] += 1;
        Ok(())
    }
}

impl Trial {
    fn settle(
        &mut self,
        settlement: Settlement<'_>,
        retry: &Retry<'_>,
        seen: &mut Coverage,
        restarted: &mut bool,
    ) -> Result<SuccessorObservation, TestCaseError> {
        let Settlement {
            change,
            fault,
            interleave,
            mid_restart,
            class,
            pre_slot,
            parity,
            pre_observation,
            expected,
            novel,
            stash,
        } = settlement;
        let during = interleave == Interleave::Decoy(DecoyPoint::DuringRecovery);
        let Some(class) = class else {
            let observation = core_observation(self.live.core());
            if during {
                self.recover_under_decoy(true, seen)?;
            }
            return Ok(observation);
        };
        let tail = pre_slot.durable_pack_tail();
        let index_tail = pre_slot.durable_index_tail();
        let pack = self.live.root.join(active_pack_name(pre_slot));
        let index_file = index_name(pre_slot);
        let index = self.live.root.join(&index_file);
        let catalog = self.live.root.join(CATALOG);
        match fault {
            FaultChoice::None => {
                let successor = stash.ok_or_else(|| {
                    TestCaseError::fail("a forged reread stashed no successor catalog")
                })?;
                write_file(&catalog, &successor);
                self.live.recover()?;
                let observation = core_observation(self.live.core());
                prop_assert_eq!(&observation, expected);
                Ok(observation)
            }
            FaultChoice::Hook(2 | 3) |
            FaultChoice::Io(IoFault::PhysicalWrite | IoFault::PhysicalSync) => {
                check_absent(self.live.core(), novel)?;
                let observation = core_observation(self.live.core());
                prop_assert_eq!(&observation, pre_observation);
                self.retry_prepared(retry)?;
                if during {
                    self.recover_under_decoy(true, seen)?;
                }
                Ok(observation)
            }
            FaultChoice::TruncateSync => {
                arm_durable_io_failpoint(DurableIoFailpointV1::PhysicalSync);
                let (published, effects) = retry(self.live.writer_mut());
                prop_assert_eq!(effects, untraced());
                match published {
                    Err(error @ IncrementalPublicationErrorV1::Io(DurableIoError::Io { .. })) => {
                        prop_assert_eq!(
                            error.publication_outcome(),
                            GenerationCheckpointPublicationOutcomeV1::DefinitelyNotPublished
                        );
                    }
                    published => {
                        return Err(failure(
                            "a retry over a pack suffix did not fail at the truncate sync",
                            published.map(|_| ()),
                        ));
                    }
                }
                prop_assert_eq!(file_length(&pack), tail);
                let observation = core_observation(self.live.core());
                prop_assert_eq!(&observation, pre_observation);
                self.retry_prepared(retry)?;
                Ok(observation)
            }
            FaultChoice::RefusalsOverPackSuffix => {
                prop_assert!(file_length(&pack) > tail, "the faulted append left no pack suffix");
                for (position, kind) in [
                    RefusalKind::Lowered(0),
                    RefusalKind::ChangedPolicy(0),
                    RefusalKind::ModeMismatch,
                    RefusalKind::StaleAuthority,
                ]
                .into_iter()
                .enumerate()
                {
                    let classes = self.refuse(kind, change)?;
                    add(&mut seen.lowered_classes, classes);
                    seen.refusal_with_pack_suffix[position] += 1;
                }
                prop_assert!(file_length(&pack) > tail, "a refusal removed the pack suffix");
                check_absent(self.live.core(), novel)?;
                let observation = core_observation(self.live.core());
                prop_assert_eq!(&observation, pre_observation);
                self.retry_prepared(retry)?;
                Ok(observation)
            }
            FaultChoice::RefusalsOverIndexSuffix => {
                prop_assert!(file_length(&pack) > tail, "the faulted append left no pack suffix");
                prop_assert_ne!(file_length(&index), index_tail);
                for (position, kind) in [
                    RefusalKind::Lowered(1),
                    RefusalKind::ChangedPolicy(1),
                    RefusalKind::StaleAuthority,
                ]
                .into_iter()
                .enumerate()
                {
                    let classes = self.refuse(kind, change)?;
                    add(&mut seen.lowered_classes, classes);
                    seen.refusal_with_index_suffix[position] += 1;
                }
                prop_assert!(file_length(&pack) > tail, "a refusal removed the pack suffix");
                prop_assert_ne!(file_length(&index), index_tail);
                self.live.recover()?;
                let observation = core_observation(self.live.core());
                prop_assert_eq!(&observation, pre_observation);
                check_absent(self.live.core(), novel)?;
                self.retry_fresh(change)?;
                Ok(observation)
            }
            FaultChoice::RetryWithoutRecovery(_) => {
                let index_length = file_length(&index);
                let (published, effects) = retry(self.live.writer_mut());
                prop_assert_eq!(effects, effects_through(3));
                let error = match published {
                    Err(error @ IncrementalPublicationErrorV1::Append(_)) => error,
                    published => {
                        return Err(failure(
                            "a retry without recovery passed the index length check",
                            published.map(|_| ()),
                        ));
                    }
                };
                prop_assert_eq!(
                    error.publication_outcome(),
                    GenerationCheckpointPublicationOutcomeV1::DefinitelyNotPublished
                );
                let text = format!("{error:?}");
                prop_assert!(
                    text.contains(&format!("expected: Exact({index_tail})")),
                    "the index refusal names another expectation: {}",
                    text
                );
                prop_assert!(
                    text.contains(&format!("actual: {index_length}")),
                    "the index refusal names another length: {}",
                    text
                );
                prop_assert_eq!(file_length(&index), index_length);
                prop_assert!(file_length(&pack) > tail, "the refused retry left no pack suffix");
                check_absent(self.live.core(), novel)?;
                let observation =
                    self.settle_recovery(during, mid_restart, class, seen, restarted)?;
                prop_assert_eq!(&observation, pre_observation);
                self.retry_fresh(change)?;
                Ok(observation)
            }
            FaultChoice::Rebuild(rebuild, _) => {
                let before = directory_snapshot(&self.live.root);
                prop_assert_ne!(file_length(&index), index_tail);
                arm_durable_io_failpoint(rebuild.failpoint());
                match recover_physical_from_lease_v1(
                    &mut self.live.writer_mut().lease,
                    limits(),
                    LogPackDecodeLimits::UNBOUNDED,
                ) {
                    Err(DurableStartupErrorV1::Io(DurableIoError::Io { .. })) => {}
                    recovered => {
                        return Err(failure(
                            "an index rebuild failpoint did not fail recovery with an I/O error",
                            recovered.err(),
                        ));
                    }
                }
                let after = directory_snapshot(&self.live.root);
                let candidate = format!("{index_file}.rebuild");
                if rebuild.leaves_candidate() {
                    let written = after
                        .get(&candidate)
                        .ok_or_else(|| failure("a failed rebuild left no candidate", &candidate))?;
                    let mut expected_files = before.clone();
                    expected_files.insert(candidate.clone(), written.clone());
                    same_files(
                        "a failed rebuild changed more than its candidate",
                        &expected_files,
                        &after,
                    )?;
                } else {
                    prop_assert!(
                        !after.contains_key(&candidate),
                        "a replaced rebuild left its candidate"
                    );
                    let previous = before.get(&index_file).ok_or_else(|| {
                        failure("the index was missing before the rebuild", &index_file)
                    })?;
                    let rebuilt = after.get(&index_file).ok_or_else(|| {
                        failure("the index was missing after the rebuild", &index_file)
                    })?;
                    prop_assert_eq!(rebuilt.as_slice(), &previous[..to_usize(index_tail)]);
                    let mut others_before = before.clone();
                    let mut others_after = after.clone();
                    others_before.remove(&index_file);
                    others_after.remove(&index_file);
                    same_files(
                        "a replaced rebuild changed another file",
                        &others_before,
                        &others_after,
                    )?;
                }
                check_absent(self.live.core(), novel)?;
                self.live.recover()?;
                let observation = core_observation(self.live.core());
                prop_assert_eq!(&observation, pre_observation);
                self.retry_fresh(change)?;
                Ok(observation)
            }
            FaultChoice::CatalogReplaced { restart } => {
                let before = directory_snapshot(&self.live.root);
                prop_assert!(
                    recovered(self.live.writer_mut()).is_err(),
                    "held-lease recovery adopted a replaced catalog"
                );
                same_files(
                    "a refused recovery changed the store",
                    &before,
                    &directory_snapshot(&self.live.root),
                )?;
                if restart {
                    self.live.writer = None;
                    prop_assert!(
                        cold_admit(&self.live.root, self.scenario.writer).is_err(),
                        "cold admission adopted a replaced catalog"
                    );
                    same_files(
                        "a refused cold admission changed the store",
                        &before,
                        &directory_snapshot(&self.live.root),
                    )?;
                    *restarted = true;
                }
                let successor = stash.ok_or_else(|| {
                    TestCaseError::fail("a replaced catalog stashed no successor catalog")
                })?;
                write_file(&catalog, &successor);
                if restart {
                    let writer =
                        cold_admit(&self.live.root, self.scenario.writer).map_err(|error| {
                            failure("cold admission after restoring the catalog failed", error)
                        })?;
                    self.live.writer = Some(writer);
                } else {
                    self.live.recover()?;
                }
                let observation = core_observation(self.live.core());
                prop_assert_eq!(&observation, expected);
                Ok(observation)
            }
            FaultChoice::SidecarCorrupt => {
                let observation =
                    self.settle_recovery(during, mid_restart, class, seen, restarted)?;
                prop_assert_eq!(&observation, expected);
                if self.live.reader.is_some() {
                    prop_assert!(
                        self.sidecar_exists(),
                        "the reclamation intent was consumed under a pinned reader"
                    );
                }
                Ok(observation)
            }
            FaultChoice::CatalogChangedAtCompare => {
                flip_byte(&catalog, flip_offset(parity));
                let observation =
                    self.settle_recovery(during, mid_restart, class, seen, restarted)?;
                prop_assert_eq!(&observation, pre_observation);
                check_absent(self.live.core(), novel)?;
                self.retry_fresh(change)?;
                Ok(observation)
            }
            FaultChoice::Hook(_) |
            FaultChoice::Io(_) |
            FaultChoice::Torn(_) |
            FaultChoice::PackShort => {
                let observation =
                    self.settle_recovery(during, mid_restart, class, seen, restarted)?;
                if class.kept() {
                    prop_assert_eq!(&observation, pre_observation);
                    check_absent(self.live.core(), novel)?;
                    self.retry_fresh(change)?;
                } else {
                    prop_assert_eq!(&observation, expected);
                }
                Ok(observation)
            }
        }
    }

    fn settle_recovery(
        &mut self,
        during: bool,
        mid_restart: Option<bool>,
        class: OutcomeClass,
        seen: &mut Coverage,
        restarted: &mut bool,
    ) -> Result<SuccessorObservation, TestCaseError> {
        match (mid_restart, during) {
            (Some(keep_reader), _) => {
                self.restart_both(keep_reader)?;
                seen.restart_mid[class.restart_index()] += 1;
                *restarted = true;
            }
            (None, true) => self.recover_under_decoy(false, seen)?,
            (None, false) => self.live.recover()?,
        }
        Ok(core_observation(self.live.core()))
    }

    fn retry_prepared(&mut self, retry: &Retry<'_>) -> Result<(), TestCaseError> {
        let (published, effects) = retry(self.live.writer_mut());
        prop_assert_eq!(effects, PublicationEffects::trusted());
        self.live.writer_mut().core = published
            .map_err(|error| failure("the retry of the prepared successor failed", error))?;
        Ok(())
    }

    fn retry_fresh(&mut self, change: Change<'_>) -> Result<(), TestCaseError> {
        let writer = self.live.writer_mut();
        let (published, effects) = attempt(writer, change, UNBOUNDED);
        prop_assert_eq!(effects, PublicationEffects::trusted());
        writer.core = published.map_err(|error| failure("the rebound retry failed", error))?;
        Ok(())
    }

    fn finish_step(
        &mut self,
        record: Option<AdoptedStep>,
        crash: Option<CrashRecord>,
        pending_at_start: bool,
        restarted: bool,
        fault: FaultChoice,
        seen: &mut Coverage,
    ) -> Result<(), TestCaseError> {
        prop_assert!(self.decoy.is_none(), "a step ended with a decoy in place");
        prop_assert_eq!(&read_image(&self.live.root), self.live.core().authority().image());
        if let Some(record) = record {
            self.adopt(record, seen)?;
        }
        if let Some(reader) = &self.live.reader {
            reader.check(&self.live.root, &self.oracle)?;
            check_readable(
                self.live.core(),
                &walk(&self.oracle, &[reader.state.manifest()]).objects,
            )?;
            prop_assert!(
                reader
                    .directory
                    .generation_leases()
                    .shares_registry(&self.live.writer().lease.generation_leases()),
                "a pinned reader does not share the writer's lease registry"
            );
            if fault != FaultChoice::None {
                seen.reader_across_fault += 1;
            }
            if restarted {
                seen.reader_across_restart += 1;
            }
        }
        let contender = DurableDirectoryV1::open(&self.live.root)
            .expect("a trial directory opens")
            .try_acquire_writer();
        prop_assert!(
            matches!(contender, Err(DurableIoError::WriterLockContended { .. })),
            "a second writer was not refused: {:?}",
            contender.map(|_| ())
        );
        same_files(
            "the live chain diverged from the reference chain",
            &directory_snapshot(&self.live.root),
            &directory_snapshot(&self.reference.root),
        )?;
        prop_assert_eq!(
            core_observation(self.live.core()),
            core_observation(self.reference.core())
        );
        if let Some(crash) = crash {
            prop_assert!(!self.sidecar_exists(), "a crashed reclamation was not completed");
            for name in &crash.files {
                prop_assert!(
                    !file_exists(&self.live.root.join(name)),
                    "the reclaimed file {} survived",
                    name
                );
            }
            prop_assert_eq!(
                self.live
                    .writer()
                    .lease
                    .generation_leases()
                    .acquire(crash.key)
                    .err(),
                Some(GenerationLeaseError::Retired(crash.key))
            );
            seen.crash_mid[usize::from(crash.then_restart)] += 1;
        }
        if pending_at_start && !self.sidecar_exists() {
            seen.reclamation_completed += 1;
        }
        if restarted {
            self.check_domains()?;
        }
        self.previous_faulted = fault != FaultChoice::None;
        Ok(())
    }

    fn adopt(&mut self, record: AdoptedStep, seen: &mut Coverage) -> Result<(), TestCaseError> {
        let AdoptedStep {
            to_slots,
            from,
            to,
            manifest,
            expected,
            novel,
            pre_history,
            pre_states,
        } = record;
        let core = self.live.core();
        prop_assert_eq!(core_observation(core), expected);
        let state = core.authority().slot().state();
        prop_assert_eq!(state.manifest(), manifest);
        let current = walk(&self.oracle, &[manifest]);
        check_readable(core, &current.objects)?;
        check_present(core, &novel)?;
        let (historical, current_state) = retained_history(core);
        let (mut previous, previous_current) = pre_history;
        previous.push(previous_current);
        prop_assert_eq!(&historical, &previous);
        prop_assert_eq!(current_state.state(), state);
        prop_assert_eq!(core.retained_states(), to_u64(historical.len()) + 1);
        prop_assert_eq!(core.retained_states(), pre_states + 1);
        let ledger = core.retained_closure();
        let retained = ledger.inventory(ClosureDomainV1::Retained);
        let live = ledger.inventory(ClosureDomainV1::Current);
        prop_assert!(
            retained.distinct_objects > live.distinct_objects,
            "the retained closure {:?} does not exceed the current closure {:?}",
            retained,
            live
        );
        seen.retained_exceeds_current += 1;
        let (replacements, removals) = shape_changes(&from, &to);
        seen.replacement += replacements;
        seen.removal += removals;
        seen.shared_node += u32::from(current.shared_nodes > 0);
        seen.content_alias += u32::from(current.content_aliases > 0);
        seen.same_target_siblings +=
            u32::try_from(current.same_target_siblings).expect("a sibling count fits u32");
        prop_assert_eq!(current.same_target_siblings, 0);
        seen.revert += u32::from(self.adopted.iter().any(|past| past.model == to));
        let validity = core.validity();
        self.retained_union.extend(current.objects.into_keys());
        self.adopted.push(Adopted {
            slots: to_slots,
            model: to,
            version: state,
            validity,
        });
        Ok(())
    }

    fn check_domains(&self) -> Result<(), TestCaseError> {
        let core = self.live.core();
        let ledger = core.retained_closure();
        let objects = ledger.counted_objects();
        let mut retained = BTreeSet::new();
        let mut counted_current = BTreeSet::new();
        for (reference, _) in &objects {
            let digest = reference.digest();
            if ledger.count(ClosureDomainV1::Retained, digest) > 0 {
                retained.insert(digest);
            }
            if ledger.count(ClosureDomainV1::Current, digest) > 0 {
                counted_current.insert(digest);
            }
        }
        let walked = walk(&self.oracle, &[core.authority().slot().state().manifest()])
            .objects
            .into_keys()
            .collect::<BTreeSet<_>>();
        prop_assert!(
            retained == self.retained_union,
            "the retained domain differs from the adopted union: {} only counted, {} only adopted",
            retained.difference(&self.retained_union).count(),
            self.retained_union.difference(&retained).count()
        );
        prop_assert!(
            counted_current == walked,
            "the current domain differs from the current walk: {} only counted, {} only walked",
            counted_current.difference(&walked).count(),
            walked.difference(&counted_current).count()
        );
        Ok(())
    }

    fn old_suffix(
        &mut self,
        middle: Slots,
        target: Slots,
        coverage: &mut Coverage,
    ) -> Result<(), TestCaseError> {
        let mut seen = Coverage::default();
        let mut restarted = false;
        let pending_at_start = self.sidecar_exists();
        let from = self.last().model.clone();
        let middle_model = full_model(&self.base, &middle);
        let to = full_model(&self.base, &target);
        let base_version = self.live.core().authority().slot().state();
        let middle_batch = transition_batch(base_version, &from, &middle_model)
            .ok_or_else(|| TestCaseError::fail("an old-suffix middle state equals its base"))?;
        let batch = transition_batch(base_version, &from, &to)
            .ok_or_else(|| TestCaseError::fail("an old-suffix target equals its base"))?;
        let pre_observation = core_observation(self.live.core());
        let pre_image = *self.live.core().authority().image();
        let middle_change = Change {
            batch: &middle_batch,
            from: &from,
            to: &middle_model,
        };
        let writer = self.live.writer_mut();
        let bound = writer
            .core
            .bind_trusted_transaction(
                &writer.lease,
                seal_on(writer.core.verified_state(), middle_change),
                UNBOUNDED,
            )
            .expect("the unbounded trusted bind of a model transition succeeds");
        let abandoned = novel_objects(&writer.core, bound.objects())?;
        let prepared = writer
            .core
            .prepare_transaction(
                &writer.lease,
                &bound,
                successor_sequence(&writer.core),
                LogPackDecodeLimits::UNBOUNDED,
            )
            .expect("a trusted bind prepares");
        arm_durable_io_failpoint(DurableIoFailpointV1::CatalogSlotWrite);
        let (published, effects) = observed_effects(|| {
            publish_trusted_incremental_v1(
                &mut writer.lease,
                &writer.core,
                &prepared,
                UNBOUNDED,
                |_| {},
            )
        });
        prop_assert_eq!(effects, effects_through(10));
        match published {
            Err(error @ IncrementalPublicationErrorV1::CatalogCommitIndeterminate(_)) => {
                prop_assert_eq!(
                    error.publication_outcome(),
                    GenerationCheckpointPublicationOutcomeV1::Indeterminate
                );
            }
            published => {
                return Err(failure(
                    "an abandoned commit was not indeterminate at the slot write",
                    published.map(|_| ()),
                ));
            }
        }
        prop_assert_eq!(read_image(&self.live.root), pre_image);
        self.live.recover()?;
        prop_assert_eq!(core_observation(self.live.core()), pre_observation);
        check_absent(self.live.core(), &abandoned)?;
        let record = self.publish_step(
            StepPlan {
                to_slots: target,
                change: Change {
                    batch: &batch,
                    from: &from,
                    to: &to,
                },
                fault: FaultChoice::None,
                interleave: Interleave::None,
                mid_restart: None,
            },
            &mut seen,
            &mut restarted,
        )?;
        let fresh = record
            .novel
            .iter()
            .map(|(reference, _)| reference.digest())
            .collect::<BTreeSet<_>>();
        let orphans = abandoned
            .into_iter()
            .filter(|(reference, _)| !fresh.contains(&reference.digest()))
            .collect::<Vec<_>>();
        prop_assert!(!orphans.is_empty(), "the abandoned commit wrote no object the suffix lacks");
        check_absent(self.live.core(), &orphans)?;
        self.finish_step(
            Some(record),
            None,
            pending_at_start,
            restarted,
            FaultChoice::Io(IoFault::CatalogSlotWrite),
            &mut seen,
        )?;
        seen.old_suffix += 1;
        coverage.merge(seen);
        Ok(())
    }

    fn finish(&mut self, coverage: &mut Coverage) -> Result<(), TestCaseError> {
        let mut seen = Coverage::default();
        self.idle_restart(false, &mut seen)?;
        self.check_domains()?;
        same_files(
            "the live chain diverged from the reference chain",
            &directory_snapshot(&self.live.root),
            &directory_snapshot(&self.reference.root),
        )?;
        prop_assert_eq!(
            core_observation(self.live.core()),
            core_observation(self.reference.core())
        );
        coverage.merge(seen);
        Ok(())
    }
}

fn jump(position: usize, fault: FaultChoice, interleave: Interleave) -> HistoryStep {
    HistoryStep {
        transition: Transition::Jump(sweep_slots(position)),
        fault,
        interleave,
    }
}

fn seed_histories() -> [Vec<HistoryStep>; 3] {
    [
        vec![
            jump(0, FaultChoice::None, Interleave::Pin),
            jump(1, FaultChoice::Hook(6), Interleave::None),
            jump(2, FaultChoice::Hook(13), Interleave::Restart { keep_reader: true }),
            jump(3, FaultChoice::Io(IoFault::CatalogSlotWrite), Interleave::FreshCheckpoint),
            jump(4, FaultChoice::SidecarCorrupt, Interleave::None),
            jump(5, FaultChoice::None, Interleave::Unpin),
            jump(6, FaultChoice::Torn(335), Interleave::PinOverflow),
            HistoryStep {
                transition: Transition::Revert(0),
                fault: FaultChoice::None,
                interleave: Interleave::Restart { keep_reader: false },
            },
        ],
        vec![
            jump(0, FaultChoice::None, Interleave::Pin),
            jump(1, FaultChoice::None, Interleave::FreshCheckpoint),
            jump(2, FaultChoice::None, Interleave::CrashMidReclamation {
                index_too: true,
                then_restart: false,
            }),
            jump(3, FaultChoice::None, Interleave::Decoy(DecoyPoint::BeforeBase)),
            jump(4, FaultChoice::None, Interleave::Decoy(DecoyPoint::BeforeReread)),
            jump(5, FaultChoice::Hook(5), Interleave::Decoy(DecoyPoint::DuringRecovery)),
            jump(6, FaultChoice::None, Interleave::Decoy(DecoyPoint::ForgedReread)),
            jump(7, FaultChoice::CatalogReplaced { restart: true }, Interleave::None),
            jump(8, FaultChoice::None, Interleave::IdleReplace),
        ],
        vec![
            jump(0, FaultChoice::None, Interleave::Pin),
            jump(1, FaultChoice::None, Interleave::FreshCheckpoint),
            jump(2, FaultChoice::None, Interleave::CrashMidReclamation {
                index_too: false,
                then_restart: true,
            }),
            jump(3, FaultChoice::CatalogReplaced { restart: false }, Interleave::None),
            jump(4, FaultChoice::None, Interleave::BindPast(0)),
            jump(5, FaultChoice::Hook(12), Interleave::BindPast(1)),
        ],
    ]
}

fn run_history(
    scenario: Scenario,
    steps: &[HistoryStep],
    clone_every_step: bool,
    strict: bool,
) -> Result<Coverage, TestCaseError> {
    let mut trial = Trial::open(scenario, steps.len(), clone_every_step, strict)?;
    let mut coverage = Coverage::default();
    for step in steps {
        trial.run_step(step, &mut coverage)?;
    }
    trial.finish(&mut coverage)?;
    Ok(coverage)
}

pub(super) fn production_faulted_trusted_histories(cases: u32) {
    for scenario in [SCENARIOS[0], SCENARIOS[2]] {
        let mut merged = Coverage::default();
        for seed in seed_histories() {
            let seen = isolated(|| run_history(scenario, &seed, true, true))
                .expect("a seed trusted history passes");
            merged.merge(seen);
        }
        merged.assert_authority();
        let generated = Cell::new(Coverage::default());
        runner(cases)
            .run(&history_strategy(), |steps| {
                let seen = isolated(|| run_history(scenario, &steps, true, false))?;
                let mut total = generated.get();
                total.merge(seen);
                generated.set(total);
                Ok(())
            })
            .expect("generated faulted trusted histories pass");
        merged.merge(generated.get());
        println!("trusted history production [{:?}]: {merged:?}", scenario.writer);
        merged.assert_authority();
    }
}

fn fault_strategy() -> impl Strategy<Value = FaultChoice> {
    prop_oneof![
        6 => Just(FaultChoice::None),
        3 => (2_usize..=14).prop_map(FaultChoice::Hook),
        2 => prop::sample::select(IO_FAULTS.to_vec()).prop_map(FaultChoice::Io),
        2 => (0..CATALOG_SLOT_LEN).prop_map(FaultChoice::Torn),
        1 => Just(FaultChoice::PackShort),
        1 => Just(FaultChoice::SidecarCorrupt),
        1 => any::<bool>().prop_map(|restart| FaultChoice::CatalogReplaced { restart }),
        1 => Just(FaultChoice::CatalogChangedAtCompare),
        1 => (4_usize..=10).prop_map(FaultChoice::RetryWithoutRecovery),
        1 => (prop::sample::select(REBUILD_FAULTS.to_vec()), 4_usize..=10)
            .prop_map(|(rebuild, hook)| FaultChoice::Rebuild(rebuild, hook)),
    ]
}

fn refusal_strategy() -> impl Strategy<Value = RefusalKind> {
    prop_oneof![
        any::<usize>().prop_map(RefusalKind::Lowered),
        any::<usize>().prop_map(RefusalKind::ChangedPolicy),
        Just(RefusalKind::ModeMismatch),
        Just(RefusalKind::StaleAuthority),
    ]
}

fn interleave_strategy() -> impl Strategy<Value = Interleave> {
    prop_oneof![
        6 => Just(Interleave::None),
        2 => refusal_strategy().prop_map(Interleave::Refusal),
        1 => Just(Interleave::Pin),
        1 => Just(Interleave::Unpin),
        1 => Just(Interleave::PinOverflow),
        1 => Just(Interleave::FreshCheckpoint),
        1 => (any::<bool>(), any::<bool>()).prop_map(|(index_too, then_restart)| {
            Interleave::CrashMidReclamation {
                index_too,
                then_restart,
            }
        }),
        1 => prop::sample::select(DECOY_POINTS.to_vec()).prop_map(Interleave::Decoy),
        1 => Just(Interleave::IdleReplace),
        1 => any::<bool>().prop_map(|keep_reader| Interleave::Restart { keep_reader }),
        1 => any::<usize>().prop_map(Interleave::BindPast),
    ]
}

fn unit_strategy() -> impl Strategy<Value = Vec<HistoryStep>> {
    prop_oneof![
        4 => (
            transition_strategy(),
            fault_strategy(),
            interleave_strategy(),
            fault_strategy(),
            interleave_strategy(),
        )
            .prop_map(|((slots, edits), jump_fault, jump_interleave, edit_fault, edit_interleave)| {
                vec![
                    HistoryStep {
                        transition: Transition::Jump(slots),
                        fault: jump_fault,
                        interleave: jump_interleave,
                    },
                    HistoryStep {
                        transition: Transition::Edit(edits),
                        fault: edit_fault,
                        interleave: edit_interleave,
                    },
                ]
            }),
        1 => (any::<usize>(), fault_strategy(), interleave_strategy()).prop_map(
            |(past, fault, interleave)| {
                vec![HistoryStep {
                    transition: Transition::Revert(past),
                    fault,
                    interleave,
                }]
            }
        ),
    ]
}

fn history_strategy() -> impl Strategy<Value = Vec<HistoryStep>> {
    prop::collection::vec(unit_strategy(), 1..=5).prop_map(|units| {
        let mut steps = Vec::with_capacity(9);
        steps.extend(units.into_iter().flatten().take(9));
        steps
    })
}

impl ScriptBuilder {
    fn step(&mut self, fault: FaultChoice, interleave: Interleave) {
        self.items
            .push(SweepItem::Step(jump(self.next, fault, interleave)));
        self.next += 1;
    }

    fn revert(&mut self, past: usize, fault: FaultChoice, interleave: Interleave) {
        self.items.push(SweepItem::Step(HistoryStep {
            transition: Transition::Revert(past),
            fault,
            interleave,
        }));
    }

    fn old_suffix(&mut self) {
        self.items
            .push(SweepItem::OldSuffix(sweep_slots(self.next), sweep_slots(self.next + 1)));
        self.next += 2;
    }
}

fn sweep_script() -> Vec<SweepItem> {
    let mut script = ScriptBuilder {
        items: Vec::with_capacity(96),
        next: 0,
    };
    script.step(FaultChoice::None, Interleave::None);
    for hook in 2..=14 {
        script.step(FaultChoice::Hook(hook), Interleave::None);
    }
    for io in IO_FAULTS {
        script.step(FaultChoice::Io(io), Interleave::None);
    }
    script.step(FaultChoice::TruncateSync, Interleave::None);
    for prefix in TORN_PREFIXES {
        script.step(FaultChoice::Torn(prefix), Interleave::None);
        script.step(FaultChoice::Torn(prefix), Interleave::None);
    }
    script.step(FaultChoice::PackShort, Interleave::None);
    for hook in 4..=10 {
        script.step(FaultChoice::RetryWithoutRecovery(hook), Interleave::None);
    }
    for (offset, rebuild) in REBUILD_FAULTS.into_iter().enumerate() {
        script.step(FaultChoice::Rebuild(rebuild, 4 + offset), Interleave::None);
    }
    script.old_suffix();
    script.step(FaultChoice::RefusalsOverPackSuffix, Interleave::None);
    script.step(FaultChoice::RefusalsOverIndexSuffix, Interleave::None);
    script.step(FaultChoice::None, Interleave::AllRefusals);
    script.step(FaultChoice::None, Interleave::Pin);
    script.step(FaultChoice::Hook(6), Interleave::Restart { keep_reader: true });
    script.step(FaultChoice::Hook(11), Interleave::None);
    script.step(FaultChoice::None, Interleave::FreshCheckpoint);
    script.step(FaultChoice::SidecarCorrupt, Interleave::None);
    script.step(FaultChoice::None, Interleave::Unpin);
    for (index_too, then_restart) in [(false, false), (true, true)] {
        script.step(FaultChoice::None, Interleave::Pin);
        script.step(FaultChoice::None, Interleave::FreshCheckpoint);
        script.step(FaultChoice::None, Interleave::CrashMidReclamation {
            index_too,
            then_restart,
        });
    }
    script.step(FaultChoice::None, Interleave::PinOverflow);
    script.step(FaultChoice::CatalogReplaced { restart: false }, Interleave::None);
    script.step(FaultChoice::CatalogReplaced { restart: true }, Interleave::None);
    script.step(FaultChoice::None, Interleave::IdleReplace);
    script.step(FaultChoice::CatalogChangedAtCompare, Interleave::None);
    script.step(FaultChoice::None, Interleave::Decoy(DecoyPoint::BeforeBase));
    script.step(FaultChoice::None, Interleave::Decoy(DecoyPoint::BeforeReread));
    script.step(FaultChoice::Hook(5), Interleave::Decoy(DecoyPoint::DuringRecovery));
    script.step(FaultChoice::None, Interleave::Decoy(DecoyPoint::ForgedReread));
    let restart = Interleave::Restart { keep_reader: false };
    script.step(FaultChoice::None, restart);
    script.step(FaultChoice::Hook(5), restart);
    script.step(FaultChoice::Io(IoFault::CatalogSlotWrite), restart);
    script.step(FaultChoice::Hook(11), restart);
    script.step(FaultChoice::None, Interleave::BindPast(0));
    script.step(FaultChoice::Hook(7), Interleave::None);
    script.step(FaultChoice::None, Interleave::BindPast(1));
    script.revert(1, FaultChoice::None, Interleave::None);
    script.step(FaultChoice::None, Interleave::BindPast(1));
    script.items
}

fn sweep_scenario(scenario: Scenario) -> Result<(Coverage, Vec<HookPoint>), TestCaseError> {
    let script = sweep_script();
    let mut trial = Trial::open(scenario, script.len() + 1, false, true)?;
    let mut coverage = Coverage::default();
    for item in &script {
        match item {
            SweepItem::Step(step) => trial.run_step(step, &mut coverage)?,
            SweepItem::OldSuffix(middle, target) => {
                trial.old_suffix(*middle, *target, &mut coverage)?;
            }
        }
    }
    trial.finish(&mut coverage)?;
    let hooks = trial
        .trusted_hooks
        .take()
        .ok_or_else(|| TestCaseError::fail("the sweep recorded no clean trusted trace"))?;
    Ok((coverage, hooks))
}

pub(super) fn production_every_failpoint_at_later_steps() {
    let swept = std::thread::scope(|scope| {
        SCENARIOS
            .map(|scenario| scope.spawn(move || sweep_scenario(scenario)))
            .map(|handle| {
                handle.join().unwrap_or_else(|payload| {
                    Err(TestCaseError::fail(format!(
                        "a failpoint sweep panicked: {}",
                        panic_text(&*payload)
                    )))
                })
            })
    });
    let mut coverage = Coverage::default();
    let mut traces = Vec::with_capacity(SCENARIOS.len());
    for swept in swept {
        let (seen, trace) = swept.expect("every failpoint sweep passes");
        coverage.merge(seen);
        traces.push(trace);
    }
    let hooks = phases(14);
    assert!(
        traces.iter().all(|trace| *trace == hooks),
        "a clean trusted publication ran another hook order: {traces:?}"
    );
    add(&mut coverage.kills, run_kills(&hooks));
    println!("trusted history every failpoint: {coverage:?}");
    coverage.assert_complete();
}

fn run_kills(hooks: &[HookPoint]) -> [u32; 13] {
    let positions = hooks
        .iter()
        .map(|hook| {
            ORDER
                .iter()
                .position(|point| point == hook)
                .expect("a trusted hook lies on the publication order")
        })
        .collect::<Vec<_>>();
    let verified = std::thread::scope(|scope| {
        std::array::from_fn::<_, 4, _>(|scenario| {
            let assigned = positions
                .iter()
                .copied()
                .filter(|position| position % 4 == scenario)
                .collect::<Vec<_>>();
            scope.spawn(move || kill_scenario(scenario, &assigned))
        })
        .map(|handle| {
            handle.join().unwrap_or_else(|payload| {
                Err(TestCaseError::fail(format!(
                    "a kill parent panicked: {}",
                    panic_text(&*payload)
                )))
            })
        })
    });
    let mut kills = [0; 13];
    for verified in verified {
        for hook in verified.expect("every kill at a trusted hook recovers") {
            kills[hook - 2] += 1;
        }
    }
    kills
}

fn kill_scenario(index: usize, hooks: &[usize]) -> Result<Vec<usize>, TestCaseError> {
    let scenario = SCENARIOS[index];
    let mut trial = Trial::open(scenario, 3, false, true)?;
    let mut coverage = Coverage::default();
    trial.run_step(&jump(0, FaultChoice::None, Interleave::None), &mut coverage)?;
    trial.run_step(&jump(1, KILL_FAULTS[index], Interleave::None), &mut coverage)?;
    let settled_files = directory_snapshot(&trial.live.root);
    let settled_slot = trial.live.core().authority().slot();
    let settled = core_observation(trial.live.core());
    let settled_prefix = appointed_prefix(&trial.live.root, settled_slot);
    trial.live.writer = None;
    let from = trial.last().model.clone();
    let to = full_model(&trial.base, &sweep_slots(2));
    let batch = transition_batch(settled_slot.state(), &from, &to)
        .ok_or_else(|| TestCaseError::fail("the killed transition is empty"))?;
    let change = Change {
        batch: &batch,
        from: &from,
        to: &to,
    };
    trial.publish_reference(change)?;
    let successor_files = directory_snapshot(&trial.reference.root);
    let successor = core_observation(trial.reference.core());
    let executable = std::env::current_exe().expect("the test binary path is known");
    let mut roots = Vec::with_capacity(hooks.len());
    for hook in hooks {
        let root = trial.directory.path().join(format!("kill-{hook}"));
        make_directory(&root);
        copy_snapshot(&settled_files, &root.join("store"));
        roots.push(root);
    }
    let outcomes = std::thread::scope(|scope| {
        let mut probes = Vec::with_capacity(hooks.len());
        for (hook, root) in hooks.iter().zip(&roots) {
            let executable = &executable;
            probes.push(scope.spawn(move || {
                run_child_probe(
                    Command::new(executable)
                        .args(["--ignored", "--exact", KILL_CHILD, "--nocapture"])
                        .env(KILL_CASE, format!("{index}:{hook}"))
                        .env(KILL_ROOT, root)
                        .env_remove(ALLOW_EXPECTED_PROBE_CORE),
                    Duration::from_secs(30),
                )
            }));
        }
        probes
            .into_iter()
            .map(|probe| probe.join().expect("a kill probe thread completes"))
            .collect::<Vec<_>>()
    });
    let mut verified = Vec::with_capacity(hooks.len());
    for ((&hook, root), outcome) in hooks.iter().zip(&roots).zip(outcomes) {
        match outcome.map_err(|error| failure("a kill child did not run", error))? {
            ProbeOutcome::Failed(failed)
                if failed.kind == ProbeFailureKind::Signaled &&
                    failed.signal == Some(libc::SIGABRT) => {}
            outcome => {
                return Err(failure(
                    &format!("the kill child at hook {hook} did not abort"),
                    outcome,
                ));
            }
        }
        prop_assert_eq!(read_file(&root.join("marker")), format!("hook:{hook}").into_bytes());
        let store = root.join("store");
        let mut writer = cold_admit(&store, scenario.writer)
            .map_err(|error| failure("cold admission after a kill failed", error))?;
        if hook <= 10 {
            prop_assert_eq!(&core_observation(&writer.core), &settled);
            prop_assert_eq!(
                &appointed_prefix(&store, writer.core.authority().slot()),
                &settled_prefix
            );
            let (published, effects) = attempt(&mut writer, change, UNBOUNDED);
            prop_assert_eq!(effects, PublicationEffects::trusted());
            writer.core =
                published.map_err(|error| failure("the publication after a kill failed", error))?;
        }
        prop_assert_eq!(&core_observation(&writer.core), &successor);
        same_files(
            &format!("the store killed at hook {hook} diverged from the reference"),
            &directory_snapshot(&store),
            &successor_files,
        )?;
        drop(writer);
        let writer = cold_admit(&store, scenario.writer)
            .map_err(|error| failure("a second cold admission after a kill failed", error))?;
        prop_assert_eq!(&core_observation(&writer.core), &successor);
        verified.push(hook);
    }
    Ok(verified)
}

#[test]
#[ignore = "isolated trusted history kill child"]
fn trusted_history_kill_child() {
    let Ok(case) = std::env::var(KILL_CASE) else {
        return;
    };
    let (scenario, hook) = case
        .split_once(':')
        .expect("a kill case names a scenario and a hook");
    let scenario = SCENARIOS[scenario
        .parse::<usize>()
        .expect("a kill scenario is an index")];
    let hook = hook.parse::<usize>().expect("a kill hook is an index");
    assert!(
        matches!(
            prepare_expected_probe_core_dump(),
            ExpectedProbeCorePolicy::SuppressionInstalled |
                ExpectedProbeCorePolicy::ExplicitlyAllowed
        ),
        "the kill child cannot suppress its core dump"
    );
    let root = PathBuf::from(std::env::var_os(KILL_ROOT).expect("a kill child names its root"));
    let mut writer =
        cold_admit(&root.join("store"), scenario.writer).expect("the store admits before the kill");
    let manifest = checkpoint_image_from_source((scenario.source)())
        .catalog
        .state()
        .manifest();
    let base = base_model(&(scenario.source)(), manifest);
    let from = full_model(&base, &sweep_slots(1));
    let to = full_model(&base, &sweep_slots(2));
    let batch = transition_batch(writer.core.authority().slot().state(), &from, &to)
        .expect("the killed transition is not empty");
    let change = Change {
        batch: &batch,
        from: &from,
        to: &to,
    };
    let bound = writer
        .core
        .bind_trusted_transaction(
            &writer.lease,
            seal_on(writer.core.verified_state(), change),
            UNBOUNDED,
        )
        .expect("the unbounded trusted bind of the killed transition succeeds");
    let prepared = writer
        .core
        .prepare_transaction(
            &writer.lease,
            &bound,
            successor_sequence(&writer.core),
            LogPackDecodeLimits::UNBOUNDED,
        )
        .expect("the killed transition prepares");
    let marker = root.join("marker");
    arm_generation_checkpoint_publication_hook(ORDER[hook], move || {
        fs::write(&marker, format!("hook:{hook}")).expect("the kill marker is written");
        std::process::abort();
    });
    let published = publish_trusted_incremental_v1(
        &mut writer.lease,
        &writer.core,
        &prepared,
        UNBOUNDED,
        |_| {},
    );
    panic!("the kill child returned from its publication: {:?}", published.map(|_| ()));
}
