use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::num::NonZeroU64;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;
use std::time::SystemTime;

use proptest::prelude::*;
use tokio::sync::RwLock;

use super::checkpoint_wal::{SyntheticCheckpointSource, fixture as checkpoint_source_fixture};
use super::durable_publication::install_checkpoint_image;
use super::durable_recovery::{checkpoint_image_for_profile, limits};
use super::durable_rspace_authority::{
    Profile, Store, fixture, poll_checkpoint_with, poll_once, publication_limits, store_files,
};
use super::owner_incarnation::journal_bags;
use super::support::{
    PersistenceTrialDir, TEST_SEMANTIC_PROFILE_ID, TestSemanticProfile, empty_roots, encode,
    synthetic_ref, trial_scratch,
};
use super::trusted_successor_production::{PublicationEffects, observed_effects};
use crate::rspace::data_index::RSpaceDataIndex;
use crate::rspace::errors::{RSpaceError, RootError};
use crate::rspace::history::history_reader::HistoryReaderBase;
use crate::rspace::hot_store::{
    DurableHotStoreBootstrapLimits, DurableJournalBaseV1, HotStoreInstances, HotStoreState,
};
use crate::rspace::internal::{Datum, WaitingContinuation};
use crate::rspace::persistence::checkpoint::{
    CheckpointObjectInventoryV1, GenerationCheckpointDescriptorV1, OrderedObjectInventoryBuilderV1,
};
use crate::rspace::persistence::checksum::{CHECKSUM_FIELD_LEN, crc32c, decode_crc32c};
use crate::rspace::persistence::closure::RetainedUnionClosureReportV1;
use crate::rspace::persistence::difference::StateDifferenceLimits;
use crate::rspace::persistence::durable::journal_owner_hooks::{self, JournalOwnerHookPoint};
use crate::rspace::persistence::durable::{
    DurableAdmissionLimitsV1, DurableIoFailpointV1, DurableRSpaceAuthorityV1,
    DurableRSpaceTransactionError, DurableStateRootV1, DurableViewReaderV1,
    GenerationCheckpointPublicationHookPointV1, JournalPublicationPhaseV1, ResolvedDurableStateV1,
    ViewAttachmentRefusalV1, ViewBaseV1, ViewCheckpointLimitsV1, arm_durable_io_failpoint,
    arm_generation_checkpoint_publication_failpoint, armed_durable_io_failpoint, index_file_name,
    pack_file_name, recover_read_authority,
};
use crate::rspace::persistence::{
    CATALOG_IMAGE_LEN, COMMIT_DISCRIMINATOR_LEN_V1, CanonicalBlob, CanonicalBlobKind,
    CanonicalDagNode, CanonicalEncode, CanonicalFormatError, CatalogAuthorityTokenV1,
    CatalogSlotV1, CommitRecordKindV1, EndpointValue, GenerationLeaseKeyV1,
    GenerationLeaseRegistryV1, IndexGenerationV1, LegacyHistoryRoot, LogPackDecodeLimits,
    LogPackObjectDispositionV1, LogPackObjectHeaderV1, MutationBatchViewV1, MutationError,
    OBJECT_RECORD_HEADER_LEN_V1, ObjectDigest, ObjectRefV1, PREPARE_DISCRIMINATOR_LEN_V1, PackId,
    PhysicalDigest, PrepareRecordIdentityV1, PrepareRecordKindV1, RSpaceMutationBatchV1,
    RetainedStateV1, RetentionManifestV1, SemanticNamespace, SemanticProfile, SemanticProfileId,
    SemanticStateManifestV1, StateVersionV1, TRANSACTION_COMMIT_RECORD_LEN_V1,
    TRANSACTION_PREPARE_HEADER_LEN_V1, TransactionCommitViewV1, decode_commit_kind_v1,
    decode_prepare_kind_v1, preflight_prepare_record_v1,
};
use crate::rspace::rho_path::RhoPathV1;
use crate::rspace::serializers::serializers::{
    encode_continuation_identity, encode_continuation_source_into, encode_join_record,
};

type Owner = DurableRSpaceAuthorityV1<Profile>;
type Continuation = WaitingContinuation<String, String>;
type Refusal = Box<DurableRSpaceTransactionError>;
type StoreFiles = BTreeMap<String, Vec<u8>>;

const CHANNELS: [&str; 3] = ["a", "b", "c"];
const VALUES: [&str; 2] = ["v0", "v1"];
const BODIES: [&str; 2] = ["k0", "k1"];
const GROUPS: [&[&str]; 4] = [&["a"], &["a", "b"], &["b", "a"], &["c", "a", "b"]];
const UNSTORED_GROUPS: [&[&str]; 3] = [&["b"], &["a", "a"], &["a", "b", "c"]];
const SHARED_CHANNEL: &str = "z";
const GENERATED_SCENARIOS: u32 = 24;
const MAX_BATCHES: usize = 3;
const MAX_CHECKPOINTS: usize = 3;
const MAX_STEPS: usize = 6;
const BASE_SELECTORS: usize = 8;
const CATALOG_FILE: &str = "catalog.v1";
const INTERRUPTION: &str = "the owner task stops at a journal owner hook point";
pub(super) const DIRECTORY_FAILPOINTS: [DurableIoFailpointV1; 16] = [
    DurableIoFailpointV1::PhysicalWrite,
    DurableIoFailpointV1::PhysicalSync,
    DurableIoFailpointV1::CandidateWrite,
    DurableIoFailpointV1::CandidateSetLength,
    DurableIoFailpointV1::CandidateSync,
    DurableIoFailpointV1::BeforeReplace,
    DurableIoFailpointV1::AfterReplaceBeforeDirectorySync,
    DurableIoFailpointV1::DirectorySync,
    DurableIoFailpointV1::CatalogSlotWrite,
    DurableIoFailpointV1::CatalogSync,
    DurableIoFailpointV1::CatalogDirectorySync,
    DurableIoFailpointV1::ReclamationIntentCreate,
    DurableIoFailpointV1::ReclamationIntentSetLength,
    DurableIoFailpointV1::ReclamationIntentSync,
    DurableIoFailpointV1::ReclamationIntentBeforeRename,
    DurableIoFailpointV1::ReclamationIntentDirectorySync,
];

const fn directory_failpoint_index(failpoint: DurableIoFailpointV1) -> usize {
    match failpoint {
        DurableIoFailpointV1::PhysicalWrite => 0,
        DurableIoFailpointV1::PhysicalSync => 1,
        DurableIoFailpointV1::CandidateWrite => 2,
        DurableIoFailpointV1::CandidateSetLength => 3,
        DurableIoFailpointV1::CandidateSync => 4,
        DurableIoFailpointV1::BeforeReplace => 5,
        DurableIoFailpointV1::AfterReplaceBeforeDirectorySync => 6,
        DurableIoFailpointV1::DirectorySync => 7,
        DurableIoFailpointV1::CatalogSlotWrite => 8,
        DurableIoFailpointV1::CatalogSync => 9,
        DurableIoFailpointV1::CatalogDirectorySync => 10,
        DurableIoFailpointV1::ReclamationIntentCreate => 11,
        DurableIoFailpointV1::ReclamationIntentSetLength => 12,
        DurableIoFailpointV1::ReclamationIntentSync => 13,
        DurableIoFailpointV1::ReclamationIntentBeforeRename => 14,
        DurableIoFailpointV1::ReclamationIntentDirectorySync => 15,
    }
}

fn channel(name: &str) -> String { name.to_owned() }

fn group(names: &[&str]) -> Vec<String> { names.iter().copied().map(channel).collect() }

fn datum(channel_name: &str, value: &str) -> Datum<String> {
    Datum::create(&channel(channel_name), value.to_owned(), false)
}

pub(super) fn continuation(names: &[&str], body: &str) -> Continuation {
    let patterns: Vec<String> = names.iter().map(|name| format!("pattern-{name}")).collect();
    WaitingContinuation::create(&group(names), &patterns, &body.to_owned(), false, BTreeSet::new())
}

fn index_of(names: &[&str], value: &str) -> usize {
    names
        .iter()
        .position(|name| *name == value)
        .expect("an enumerated value belongs to the test universe")
}

fn stored_group<T: AsRef<str>>(channels: &[T]) -> usize {
    GROUPS
        .iter()
        .position(|names| {
            names.len() == channels.len() &&
                names
                    .iter()
                    .zip(channels)
                    .all(|(name, stored)| *name == stored.as_ref())
        })
        .expect("a stored channel group belongs to the test universe")
}

pub(super) fn panic_text(payload: &(dyn std::any::Any + Send)) -> &str {
    match (payload.downcast_ref::<&'static str>(), payload.downcast_ref::<String>()) {
        (Some(text), _) => text,
        (None, Some(text)) => text.as_str(),
        (None, None) => "a panic without a text payload",
    }
}

fn datum_path(channel_name: &str, value: &str) -> Vec<u8> {
    RSpaceDataIndex::datum_path(&channel(channel_name), &datum(channel_name, value))
}

fn continuation_path(names: &[&str], body: &str) -> Vec<u8> {
    RhoPathV1::collection_record_path(
        &RhoPathV1::continuation_prefix(&group(names)),
        &encode_continuation_identity(&continuation(names, body)),
    )
}

fn continuation_source(names: &[&str], body: &str) -> ObjectRefV1 {
    let mut payload = Vec::new();
    encode_continuation_source_into(&continuation(names, body), &mut payload);
    CanonicalBlob::new(CanonicalBlobKind::ContinuationSourceV1, payload)
        .expect("a continuation source is a canonical blob")
        .object_ref()
}

fn join_path(channel_name: &str, names: &[&str]) -> Vec<u8> {
    let mut path = RhoPathV1::join_prefix(&channel(channel_name));
    path.extend_from_slice(&encode_join_record(&group(names)));
    path
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Contents {
    data: [[u64; VALUES.len()]; CHANNELS.len()],
    continuations: [[bool; BODIES.len()]; GROUPS.len()],
    joins: BTreeSet<(usize, usize)>,
}

impl Contents {
    fn derive_joins(&mut self) {
        self.joins = GROUPS
            .iter()
            .enumerate()
            .filter(|&(group_index, _)| self.continuations[group_index].contains(&true))
            .flat_map(|(group_index, names)| {
                names
                    .iter()
                    .map(move |name| (index_of(&CHANNELS, name), group_index))
            })
            .collect();
    }

    fn building_steps(&self) -> Vec<Step> {
        let data_steps = usize::try_from(self.data.iter().flatten().sum::<u64>())
            .expect("a modelled datum count fits usize");
        let continuation_steps = self
            .continuations
            .iter()
            .flatten()
            .filter(|&&present| present)
            .count();
        let mut steps = Vec::with_capacity(data_steps + continuation_steps);
        for (channel_index, counts) in self.data.iter().enumerate() {
            for (value_index, &count) in counts.iter().enumerate() {
                for _ in 0..count {
                    steps.push(Step::Put {
                        channel_index,
                        value_index,
                    });
                }
            }
        }
        for (group_index, bodies) in self.continuations.iter().enumerate() {
            for (body_index, _) in bodies.iter().enumerate().filter(|&(_, &present)| present) {
                steps.push(Step::Install {
                    group_index,
                    body_index,
                });
            }
        }
        steps
    }
}

fn snapshot(reader: &dyn HistoryReaderBase<String, String, String, String>) -> Contents {
    let mut contents = Contents::default();
    for (channel_index, name) in CHANNELS.into_iter().enumerate() {
        for stored in reader.get_data_canonical(&channel(name)) {
            let value_index = index_of(&VALUES, &stored.a);
            assert_eq!(stored, datum(name, VALUES[value_index]), "a durable datum differs");
            contents.data[channel_index][value_index] += 1;
        }
        for joined in reader.get_joins_canonical(&channel(name)) {
            assert!(
                contents
                    .joins
                    .insert((channel_index, stored_group(&joined))),
                "a durable join was read twice: {name} {joined:?}"
            );
        }
    }
    for (group_index, names) in GROUPS.into_iter().enumerate() {
        for stored in reader.get_continuations_canonical(&group(names)) {
            let body_index = index_of(&BODIES, &stored.continuation);
            assert_eq!(stored, continuation(names, BODIES[body_index]), "a continuation differs");
            assert!(
                !contents.continuations[group_index][body_index],
                "a durable continuation was read twice: {stored:?}"
            );
            contents.continuations[group_index][body_index] = true;
        }
    }
    for names in UNSTORED_GROUPS {
        assert!(
            reader.get_continuations_canonical(&group(names)).is_empty(),
            "a continuation was read under the unstored group {names:?}"
        );
    }
    contents
}

fn put_value(hot: &Store, channel_index: usize, value_index: usize) {
    let name = CHANNELS[channel_index];
    hot.put_datum(&channel(name), datum(name, VALUES[value_index]));
}

fn take_value(hot: &Store, channel_index: usize, value_index: usize) {
    let key = channel(CHANNELS[channel_index]);
    let index = hot
        .get_data(&key)
        .iter()
        .position(|stored| stored.a.as_str() == VALUES[value_index])
        .expect("the datum to take is stored");
    hot.remove_datum(&key, i32::try_from(index).expect("a datum index fits i32"))
        .expect("take the stored datum");
}

fn install(hot: &Store, group_index: usize, body_index: usize) {
    let names = GROUPS[group_index];
    let channels = group(names);
    assert_eq!(
        hot.put_continuation(&channels, continuation(names, BODIES[body_index])),
        Some(true),
        "the hot store takes a continuation it does not hold"
    );
    for member in channels.iter().collect::<BTreeSet<_>>() {
        hot.put_join(member, &channels)
            .expect("the hot store takes the join");
    }
}

fn withdraw(hot: &Store, group_index: usize, body_index: usize) {
    let channels = group(GROUPS[group_index]);
    let index = hot
        .get_continuations(&channels)
        .iter()
        .position(|stored| stored.continuation.as_str() == BODIES[body_index])
        .expect("the continuation to withdraw is stored");
    hot.remove_continuation(
        &channels,
        i32::try_from(index).expect("a continuation index fits i32"),
    )
    .expect("withdraw the stored continuation");
    for member in channels.iter().collect::<BTreeSet<_>>() {
        hot.remove_join(member, &channels)
            .expect("drop the join once no continuation of its group remains");
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) enum Step {
    Put {
        channel_index: usize,
        value_index: usize,
    },
    Take {
        channel_index: usize,
        value_index: usize,
    },
    Install {
        group_index: usize,
        body_index: usize,
    },
    Withdraw {
        group_index: usize,
        body_index: usize,
    },
}

impl Step {
    pub(super) fn put(channel_name: &str, value: &str) -> Self {
        Self::Put {
            channel_index: index_of(&CHANNELS, channel_name),
            value_index: index_of(&VALUES, value),
        }
    }

    pub(super) fn take(channel_name: &str, value: &str) -> Self {
        Self::Take {
            channel_index: index_of(&CHANNELS, channel_name),
            value_index: index_of(&VALUES, value),
        }
    }

    pub(super) fn install(names: &[&str], body: &str) -> Self {
        Self::Install {
            group_index: stored_group(names),
            body_index: index_of(&BODIES, body),
        }
    }

    pub(super) fn withdraw(names: &[&str], body: &str) -> Self {
        Self::Withdraw {
            group_index: stored_group(names),
            body_index: index_of(&BODIES, body),
        }
    }
}

fn apply(hot: &Store, contents: &mut Contents, step: Step) {
    match step {
        Step::Put {
            channel_index,
            value_index,
        } => {
            put_value(hot, channel_index, value_index);
            contents.data[channel_index][value_index] += 1;
        }
        Step::Take {
            channel_index,
            value_index,
        } => match contents.data[channel_index][value_index] {
            0 => {}
            stored => {
                take_value(hot, channel_index, value_index);
                contents.data[channel_index][value_index] = stored - 1;
            }
        },
        Step::Install {
            group_index,
            body_index,
        } => match contents.continuations[group_index][body_index] {
            true => {}
            false => {
                install(hot, group_index, body_index);
                contents.continuations[group_index][body_index] = true;
            }
        },
        Step::Withdraw {
            group_index,
            body_index,
        } => match contents.continuations[group_index][body_index] {
            false => {}
            true => {
                withdraw(hot, group_index, body_index);
                contents.continuations[group_index][body_index] = false;
            }
        },
    }
    contents.derive_joins();
}

pub(super) fn apply_all(hot: &Store, contents: &mut Contents, steps: &[Step]) {
    for &step in steps {
        apply(hot, contents, step);
    }
}

pub(super) fn generated_steps() -> impl Strategy<Value = Step> {
    let datum_steps = (0..CHANNELS.len(), 0..VALUES.len());
    let continuation_steps = (0..GROUPS.len(), 0..BODIES.len());
    prop_oneof![
        datum_steps
            .clone()
            .prop_map(|(channel_index, value_index)| Step::Put {
                channel_index,
                value_index,
            }),
        datum_steps.prop_map(|(channel_index, value_index)| Step::Take {
            channel_index,
            value_index,
        }),
        continuation_steps
            .clone()
            .prop_map(|(group_index, body_index)| Step::Install {
                group_index,
                body_index,
            }),
        continuation_steps.prop_map(|(group_index, body_index)| Step::Withdraw {
            group_index,
            body_index,
        }),
    ]
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct DirectoryImage {
    directory_modified: SystemTime,
    entries: BTreeMap<String, (u64, SystemTime)>,
    store_files: StoreFiles,
}

pub(super) fn directory_image(root: &Path) -> DirectoryImage {
    let entries = fs::read_dir(root)
        .expect("list the store directory")
        .map(|entry| {
            let entry = entry.expect("read a store directory entry");
            let metadata = entry.metadata().expect("read a store entry's metadata");
            (
                entry
                    .file_name()
                    .into_string()
                    .expect("a store entry name is UTF-8"),
                (
                    metadata.len(),
                    metadata
                        .modified()
                        .expect("read a store entry's modification time"),
                ),
            )
        })
        .collect();
    DirectoryImage {
        directory_modified: fs::metadata(root)
            .expect("read the store directory's metadata")
            .modified()
            .expect("read the store directory's modification time"),
        entries,
        store_files: store_files(root),
    }
}

fn catalog_token(root: &Path) -> CatalogAuthorityTokenV1 {
    let image: [u8; CATALOG_IMAGE_LEN] = fs::read(root.join(CATALOG_FILE))
        .expect("read the catalog file")
        .try_into()
        .expect("the catalog file holds exactly two slots");
    CatalogAuthorityTokenV1::select(image)
        .expect("the catalog image decodes")
        .expect("the catalog selects a slot")
}

fn journal_is_empty(hot: &Store) -> bool {
    let pending = hot.pending_deltas().expect("read the hot store's journals");
    pending.data.is_some_and(|delta| delta.is_empty()) &&
        pending.continuations.is_some_and(|delta| delta.is_empty()) &&
        pending
            .continuation_content
            .is_some_and(|content| content.is_empty()) &&
        pending
            .continuation_source_bytes
            .is_some_and(|sources| sources.is_empty()) &&
        pending.joins.is_some_and(|delta| delta.is_empty())
}

pub(super) fn view_limits() -> ViewCheckpointLimitsV1 {
    ViewCheckpointLimitsV1 {
        publication: publication_limits(),
        difference: StateDifferenceLimits {
            max_object_reads: 1 << 20,
            max_object_read_bytes: 1 << 30,
            max_worklist: 1 << 12,
            max_path_bytes: 1 << 16,
        },
    }
}

pub(super) fn read_contents(owner: &Owner, root: DurableStateRootV1) -> Contents {
    let (resolved, handle) = owner
        .published_core()
        .read_handle(Some(root))
        .expect("a retained state takes a read handle");
    snapshot(&DurableViewReaderV1::new(handle, resolved.roots()))
}

fn lazy_store(
    owner: &Owner,
    root: DurableStateRootV1,
    registry: GenerationLeaseRegistryV1,
) -> (ResolvedDurableStateV1, Store) {
    let (resolved, handle) = owner
        .published_core()
        .read_handle(Some(root))
        .expect("a retained state takes a read handle");
    let hot = HotStoreInstances::create_from_durable_reader(
        Box::new(DurableViewReaderV1::new(handle, resolved.roots())),
        HotStoreState::default(),
        DurableJournalBaseV1 {
            manifest: resolved.manifest(),
            registry,
        },
    )
    .expect("an empty cache loads into a lazy store");
    (resolved, Arc::new(hot))
}

pub(super) struct View {
    pub(super) base: ViewBaseV1,
    pub(super) hot: Store,
    pub(super) contents: Contents,
}

pub(super) struct Trial {
    hot: Store,
    pub(super) gate: Arc<RwLock<()>>,
    pub(super) owner: Owner,
    genesis: ViewBaseV1,
    contents: Contents,
    bound_root: DurableStateRootV1,
    pub(super) states: Vec<(DurableStateRootV1, Contents)>,
    memoised: Vec<(DurableStateRootV1, Contents, ResolvedDurableStateV1)>,
    scratch: PersistenceTrialDir,
}

impl Trial {
    pub(super) fn open() -> Self {
        let (scratch, hot, gate, owner) = fixture();
        Self::over(scratch, hot, gate, owner)
    }

    pub(super) fn over(
        scratch: PersistenceTrialDir,
        hot: Store,
        gate: Arc<RwLock<()>>,
        owner: Owner,
    ) -> Self {
        let attachment = owner
            .attach(&hot)
            .expect("the bound store attaches to its fresh owner");
        let genesis = owner
            .published_core()
            .read_handle(None)
            .expect("the genesis state takes a read handle")
            .0;
        let mut states = Vec::with_capacity(MAX_BATCHES + MAX_CHECKPOINTS + 2);
        states.push((genesis.root(), Contents::default()));
        Self {
            hot,
            gate,
            owner,
            genesis: ViewBaseV1::new(attachment.incarnation(), genesis),
            contents: Contents::default(),
            bound_root: genesis.root(),
            states,
            memoised: Vec::with_capacity(MAX_CHECKPOINTS + 1),
            scratch,
        }
    }

    pub(super) fn path(&self) -> &Path { self.scratch.path() }

    pub(super) fn slot(&self) -> CatalogSlotV1 { catalog_token(self.path()).slot() }

    fn current_contents(&self) -> Contents {
        self.states
            .last()
            .expect("the genesis state is recorded")
            .1
            .clone()
    }

    fn base(&self, root: DurableStateRootV1) -> ViewBaseV1 {
        let state = self
            .owner
            .published_core()
            .read_handle(Some(root))
            .expect("a retained state takes a read handle")
            .0;
        ViewBaseV1::new(self.genesis.incarnation(), state)
    }

    pub(super) fn publish(&mut self, steps: &[Step]) -> DurableStateRootV1 {
        apply_all(&self.hot, &mut self.contents, steps);
        let state =
            poll_checkpoint_with(&mut self.owner, &self.gate, &self.hot, publication_limits())
                .expect("the bound store publishes while its state is current");
        let root = DurableStateRootV1::of_manifest(state.manifest());
        self.bound_root = root;
        self.states.push((root, self.contents.clone()));
        root
    }

    pub(super) fn view(&self, root: DurableStateRootV1, contents: Contents) -> View {
        let (state, hot) = lazy_store(&self.owner, root, self.owner.generation_leases());
        View {
            base: ViewBaseV1::new(self.genesis.incarnation(), state),
            hot,
            contents,
        }
    }

    fn view_of_state(&self, index: usize) -> View {
        let (root, contents) = self.states[index].clone();
        self.view(root, contents)
    }

    fn bound_view(&self) -> View {
        View {
            base: self.base(self.bound_root),
            hot: Arc::clone(&self.hot),
            contents: self.contents.clone(),
        }
    }

    pub(super) fn checkpoint_with(
        &mut self,
        base: ViewBaseV1,
        hot: &Store,
        limits: ViewCheckpointLimitsV1,
    ) -> Result<ResolvedDurableStateV1, Refusal> {
        let permit = Arc::clone(&self.gate)
            .try_write_owned()
            .expect("the view gate is free");
        poll_once(
            self.owner
                .checkpoint_view_from_base(permit, base, &self.gate, hot, limits),
        )
        .map_err(Box::new)
    }

    fn checkpoint(
        &mut self,
        base: ViewBaseV1,
        hot: &Store,
    ) -> Result<ResolvedDurableStateV1, Refusal> {
        self.checkpoint_with(base, hot, view_limits())
    }
}

struct Twin {
    base: DurableStateRootV1,
    successor: DurableStateRootV1,
    contents: Contents,
}

fn twin_checkpoint(base_contents: &Contents, journal: &[Step]) -> Twin {
    let mut twin = Trial::open();
    let base = twin.publish(&base_contents.building_steps());
    let successor = twin.publish(journal);
    Twin {
        base,
        successor,
        contents: twin.contents.clone(),
    }
}

fn recovered_from(files: &StoreFiles) -> (DurableStateRootV1, u64, Contents) {
    let scratch = trial_scratch("owner-cross-base-recovered-");
    for (name, bytes) in files {
        let path = scratch.path().join(name);
        fs::write(&path, bytes).expect("write a store file of the image");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
                .expect("restrict a store file of the image to its owner");
        }
    }
    let view = recover_read_authority::<Profile>(
        scratch.path(),
        limits(),
        LogPackDecodeLimits::UNBOUNDED,
        DurableAdmissionLimitsV1::UNBOUNDED,
    )
    .expect("the image recovers");
    let state = view.state();
    let resolved = view
        .resolve(DurableStateRootV1::of_manifest(state.manifest()))
        .expect("the recovered current state resolves");
    let handle = view
        .read_handle_at(&resolved)
        .expect("the recovered current state takes a read handle");
    let contents = snapshot(&DurableViewReaderV1::new(handle, resolved.roots()));
    (resolved.root(), state.generation(), contents)
}

#[derive(Clone, Debug)]
struct Scenario {
    batches: Vec<Vec<Step>>,
    checkpoints: Vec<(usize, Vec<Step>)>,
    closing: Vec<Step>,
}

fn seed_scenarios() -> [Scenario; 5] {
    [
        Scenario {
            batches: vec![vec![Step::put("a", "v0"), Step::install(&["a", "b"], "k0")], vec![
                Step::put("a", "v0"),
                Step::put("b", "v1"),
                Step::withdraw(&["a", "b"], "k0"),
                Step::install(&["c", "a", "b"], "k1"),
            ]],
            checkpoints: vec![
                (1, vec![
                    Step::take("a", "v0"),
                    Step::put("c", "v1"),
                    Step::install(&["a"], "k1"),
                    Step::withdraw(&["a", "b"], "k0"),
                    Step::install(&["a", "b"], "k1"),
                ]),
                (1, vec![Step::put("b", "v0")]),
                (0, vec![
                    Step::put("a", "v0"),
                    Step::put("b", "v0"),
                    Step::install(&["a", "b"], "k0"),
                ]),
                (2, Vec::new()),
                (4, vec![Step::take("b", "v0"), Step::install(&["b", "a"], "k1")]),
            ],
            closing: vec![Step::put("c", "v0"), Step::take("a", "v0")],
        },
        Scenario {
            batches: vec![vec![Step::put("a", "v0"), Step::install(&["a", "b"], "k0")], vec![
                Step::put("b", "v1"),
            ]],
            checkpoints: vec![
                (0, vec![Step::put("a", "v0"), Step::install(&["a", "b"], "k0")]),
                (2, vec![Step::put("c", "v0"), Step::put("c", "v0"), Step::take("b", "v1")]),
            ],
            closing: Vec::new(),
        },
        Scenario {
            batches: vec![Vec::new()],
            checkpoints: vec![(0, Vec::new())],
            closing: vec![Step::put("a", "v1")],
        },
        Scenario {
            batches: vec![vec![Step::put("a", "v0")]],
            checkpoints: vec![(0, vec![Step::put("b", "v0")]), (0, vec![Step::put("a", "v0")])],
            closing: vec![Step::put("c", "v1")],
        },
        Scenario {
            batches: vec![vec![Step::put("a", "v0")]],
            checkpoints: vec![
                (1, vec![Step::put("b", "v0")]),
                (1, vec![Step::put("c", "v1")]),
                (1, vec![Step::put("b", "v0")]),
            ],
            closing: Vec::new(),
        },
    ]
}

fn generated_scenarios() -> impl Strategy<Value = Scenario> {
    let journals = || prop::collection::vec(generated_steps(), 0..=MAX_STEPS);
    (
        prop::collection::vec(journals(), 1..=MAX_BATCHES),
        prop::collection::vec((0..BASE_SELECTORS, journals()), 1..=MAX_CHECKPOINTS),
        journals(),
    )
        .prop_map(|(batches, checkpoints, closing)| Scenario {
            batches,
            checkpoints,
            closing,
        })
}

#[derive(Clone, Copy, Debug, Default)]
struct Coverage {
    scenarios: u32,
    checkpoints: u32,
    cross_base_publications: u32,
    same_base_publications: u32,
    journals_without_effect_at_a_superseded_base: u32,
    successors_equal_to_the_current_state: u32,
    successors_equal_to_an_earlier_state: u32,
    bound_stores_published_from_a_superseded_state: u32,
    bound_stores_rebound_at_a_later_slot_of_their_state: u32,
    data_rebased: u32,
    data_cancelled: u32,
    data_restored: u32,
    continuations_cancelled: u32,
    continuations_restored_from_the_store: u32,
    continuations_removed: u32,
    continuations_staged_from_the_journal: u32,
    joins_cancelled: u32,
    joins_restored: u32,
    transitions_returned_from_the_memo: u32,
}

impl Coverage {
    fn merge(&mut self, other: Self) {
        let Self {
            scenarios,
            checkpoints,
            cross_base_publications,
            same_base_publications,
            journals_without_effect_at_a_superseded_base,
            successors_equal_to_the_current_state,
            successors_equal_to_an_earlier_state,
            bound_stores_published_from_a_superseded_state,
            bound_stores_rebound_at_a_later_slot_of_their_state,
            data_rebased,
            data_cancelled,
            data_restored,
            continuations_cancelled,
            continuations_restored_from_the_store,
            continuations_removed,
            continuations_staged_from_the_journal,
            joins_cancelled,
            joins_restored,
            transitions_returned_from_the_memo,
        } = other;
        self.scenarios += scenarios;
        self.checkpoints += checkpoints;
        self.cross_base_publications += cross_base_publications;
        self.same_base_publications += same_base_publications;
        self.journals_without_effect_at_a_superseded_base +=
            journals_without_effect_at_a_superseded_base;
        self.successors_equal_to_the_current_state += successors_equal_to_the_current_state;
        self.successors_equal_to_an_earlier_state += successors_equal_to_an_earlier_state;
        self.bound_stores_published_from_a_superseded_state +=
            bound_stores_published_from_a_superseded_state;
        self.bound_stores_rebound_at_a_later_slot_of_their_state +=
            bound_stores_rebound_at_a_later_slot_of_their_state;
        self.data_rebased += data_rebased;
        self.data_cancelled += data_cancelled;
        self.data_restored += data_restored;
        self.continuations_cancelled += continuations_cancelled;
        self.continuations_restored_from_the_store += continuations_restored_from_the_store;
        self.continuations_removed += continuations_removed;
        self.continuations_staged_from_the_journal += continuations_staged_from_the_journal;
        self.joins_cancelled += joins_cancelled;
        self.joins_restored += joins_restored;
        self.transitions_returned_from_the_memo += transitions_returned_from_the_memo;
    }

    fn assert_complete(&self) {
        let Self {
            scenarios,
            checkpoints,
            cross_base_publications,
            same_base_publications,
            journals_without_effect_at_a_superseded_base,
            successors_equal_to_the_current_state,
            successors_equal_to_an_earlier_state,
            bound_stores_published_from_a_superseded_state,
            bound_stores_rebound_at_a_later_slot_of_their_state,
            data_rebased,
            data_cancelled,
            data_restored,
            continuations_cancelled,
            continuations_restored_from_the_store,
            continuations_removed,
            continuations_staged_from_the_journal,
            joins_cancelled,
            joins_restored,
            transitions_returned_from_the_memo,
        } = *self;
        assert!(
            [
                scenarios,
                checkpoints,
                cross_base_publications,
                same_base_publications,
                journals_without_effect_at_a_superseded_base,
                successors_equal_to_the_current_state,
                successors_equal_to_an_earlier_state,
                bound_stores_published_from_a_superseded_state,
                bound_stores_rebound_at_a_later_slot_of_their_state,
                data_rebased,
                data_cancelled,
                data_restored,
                continuations_cancelled,
                continuations_restored_from_the_store,
                continuations_removed,
                continuations_staged_from_the_journal,
                joins_cancelled,
                joins_restored,
                transitions_returned_from_the_memo,
            ]
            .into_iter()
            .all(|count| count > 0),
            "a class of view checkpoint never occurred: {self:?}"
        );
    }
}

fn endpoint_classes(base: &Contents, expected: &Contents, current: &Contents) -> Coverage {
    let mut classes = Coverage::default();
    let counts = base
        .data
        .iter()
        .flatten()
        .zip(expected.data.iter().flatten())
        .zip(current.data.iter().flatten());
    for ((&before, &after), &stored) in counts {
        classes.data_rebased |= u32::from(before != after && stored != before && stored != after);
        classes.data_cancelled |= u32::from(before != after && stored == after);
        classes.data_restored |= u32::from(before == after && stored != before);
    }
    let bodies = base
        .continuations
        .iter()
        .flatten()
        .zip(expected.continuations.iter().flatten())
        .zip(current.continuations.iter().flatten());
    for ((&before, &after), &stored) in bodies {
        classes.continuations_cancelled |= u32::from(before != after && stored == after);
        classes.continuations_restored_from_the_store |= u32::from(before && after && !stored);
        classes.continuations_removed |= u32::from(!before && !after && stored);
        classes.continuations_staged_from_the_journal |= u32::from(!before && after && !stored);
    }
    for join in base
        .joins
        .iter()
        .chain(&expected.joins)
        .chain(&current.joins)
    {
        let (before, after, stored) = (
            base.joins.contains(join),
            expected.joins.contains(join),
            current.joins.contains(join),
        );
        classes.joins_cancelled |= u32::from(before != after && stored == after);
        classes.joins_restored |= u32::from(before == after && stored != before);
    }
    classes
}

fn check_checkpoint(
    trial: &mut Trial,
    view: &mut View,
    journal: &[Step],
    bound: bool,
) -> Result<Coverage, TestCaseError> {
    let base_contents = view.contents.clone();
    let base_root = view.base.state().root();
    let current_root = trial.owner.current_root();
    let current_contents = trial.current_contents();
    let before = trial.slot();
    apply_all(&view.hot, &mut view.contents, journal);
    let expected = view.contents.clone();
    let twin = twin_checkpoint(&base_contents, journal);
    prop_assert_eq!(
        (twin.base, &twin.contents),
        (base_root, &expected),
        "a second store built to the base's contents must reach the base's root and the model"
    );
    let journals = journal_bags(&view.hot);
    let binding = trial.owner.state(&view.hot).ok();
    let reached = trial.checkpoint(view.base, &view.hot).map_err(|refusal| {
        TestCaseError::fail(format!(
            "the owner refused a view checkpoint from a retained base: {refusal:?}"
        ))
    })?;
    let after = trial.slot();
    prop_assert_eq!(
        reached.root(),
        twin.successor,
        "the returned root must be the root the second store returns for the same base and journal"
    );
    prop_assert_eq!(
        &read_contents(&trial.owner, reached.root()),
        &expected,
        "the returned state must hold the base's contents with the journal applied"
    );
    let same_state = base_root == current_root;
    let memoised = trial
        .memoised
        .iter()
        .find(|(root, contents, _)| *root == base_root && *contents == expected)
        .map(|(_, _, first)| *first);
    let published = expected != base_contents && expected != current_contents && memoised.is_none();
    let mut coverage = Coverage {
        checkpoints: 1,
        ..Coverage::default()
    };
    match published {
        true => {
            prop_assert_eq!(
                (
                    after.state().generation(),
                    after.epoch(),
                    after.state().manifest(),
                    trial.owner.current_root(),
                    journal_is_empty(&view.hot),
                    trial.owner.state(&view.hot).ok(),
                ),
                (
                    before.state().generation() + 1,
                    before.epoch() + 1,
                    reached.manifest(),
                    reached.root(),
                    true,
                    Some(after.state()),
                ),
                "a publishing view checkpoint must advance the store by one generation to the \
                 returned state, acknowledge the journal and bind the store to the successor"
            );
            coverage.successors_equal_to_an_earlier_state =
                u32::from(trial.states.iter().any(|(root, _)| *root == reached.root()));
            trial.states.push((reached.root(), expected.clone()));
            trial.memoised.push((base_root, expected.clone(), reached));
            match same_state {
                true => {
                    coverage.same_base_publications = 1;
                    coverage.bound_stores_rebound_at_a_later_slot_of_their_state =
                        u32::from(bound && trial.owner.state(&trial.hot).is_ok());
                }
                false => {
                    coverage.merge(endpoint_classes(&base_contents, &expected, &current_contents));
                    coverage.cross_base_publications = 1;
                    coverage.bound_stores_published_from_a_superseded_state = u32::from(bound);
                }
            }
        }
        false => {
            prop_assert_eq!(
                (after, trial.owner.current_root(), journal_bags(&view.hot) == journals),
                (before, current_root, true),
                "a view checkpoint without effect must leave the catalog slot, the current state \
                 and the journal unchanged"
            );
            match (memoised, same_state, expected == base_contents) {
                (Some(first), _, _) => {
                    prop_assert_eq!(
                        (reached, trial.owner.state(&view.hot).ok()),
                        (first, match same_state {
                            true => Some(before.state()),
                            false => binding,
                        }),
                        "a memoised transition must return the state that its first publication \
                         returned and leave the view's store bound as it was, or bound to the \
                         current slot when its base is the current state"
                    );
                    coverage.transitions_returned_from_the_memo = 1;
                }
                (None, true, _) => {}
                (None, false, true) => {
                    prop_assert_eq!(reached, view.base.state());
                    coverage.journals_without_effect_at_a_superseded_base = 1;
                }
                (None, false, false) => {
                    prop_assert_eq!(reached.root(), current_root);
                    coverage.successors_equal_to_the_current_state = 1;
                    trial.memoised.push((base_root, expected.clone(), reached));
                }
            }
        }
    }
    for (root, contents) in &trial.states {
        prop_assert_eq!(
            &read_contents(&trial.owner, *root),
            contents,
            "every state published so far must still read its own contents"
        );
    }
    Ok(coverage)
}

fn check_scenario(scenario: &Scenario) -> Result<Coverage, TestCaseError> {
    let mut trial = Trial::open();
    let mut coverage = Coverage {
        scenarios: 1,
        ..Coverage::default()
    };
    for batch in &scenario.batches {
        trial.publish(batch);
    }
    let rebindable = trial.slot();
    for (selector, journal) in &scenario.checkpoints {
        let mut view = trial.view_of_state(selector % trial.states.len());
        coverage.merge(check_checkpoint(&mut trial, &mut view, journal, false)?);
    }
    let mut bound = trial.bound_view();
    let moved = trial.slot() != rebindable;
    let mut closing = check_checkpoint(&mut trial, &mut bound, &scenario.closing, true)?;
    closing.bound_stores_rebound_at_a_later_slot_of_their_state &= u32::from(moved);
    coverage.merge(closing);
    for (left_root, left_contents) in &trial.states {
        for (right_root, right_contents) in &trial.states {
            prop_assert_eq!(
                left_root == right_root,
                left_contents == right_contents,
                "a durable state root must name exactly one modelled content"
            );
        }
    }
    Ok(coverage)
}

#[test]
fn a_view_checkpoint_from_any_retained_base_returns_the_twin_stores_root() {
    let mut seeded = Coverage::default();
    for scenario in seed_scenarios() {
        seeded.merge(check_scenario(&scenario).expect("a seeded scenario satisfies XB-01"));
    }
    println!("owner cross-base checkpoint seeds: {seeded:?}");
    seeded.assert_complete();
    let generated = Cell::new(Coverage::default());
    prop::test_runner::TestRunner::new(ProptestConfig {
        cases: GENERATED_SCENARIOS,
        source_file: Some(file!()),
        ..ProptestConfig::default()
    })
    .run(&generated_scenarios(), |scenario| {
        let mut total = generated.get();
        total.merge(check_scenario(&scenario)?);
        generated.set(total);
        Ok(())
    })
    .expect("generated scenarios satisfy XB-01");
    assert!(generated.get().scenarios >= GENERATED_SCENARIOS);
    let mut merged = seeded;
    merged.merge(generated.get());
    println!("owner cross-base checkpoint: {merged:?}");
    merged.assert_complete();
}

fn offset(value: u64) -> usize { usize::try_from(value).expect("a store file offset fits usize") }

fn store_file<'files>(files: &'files StoreFiles, name: &str) -> &'files [u8] {
    files
        .get(name)
        .unwrap_or_else(|| panic!("the store holds no file named {name}"))
}

#[derive(Debug, PartialEq, Eq)]
struct RecordedBatch {
    data: BTreeMap<Vec<u8>, (u64, u64)>,
    continuations: BTreeMap<Vec<u8>, (Option<ObjectRefV1>, Option<ObjectRefV1>)>,
    joins: BTreeMap<Vec<u8>, (u64, u64)>,
}

struct RecordedPrepare {
    record_len: u64,
    sequence: u64,
    base: StateVersionV1,
    target: StateVersionV1,
    identity: PrepareRecordIdentityV1,
    batch: RecordedBatch,
    written: Vec<ObjectRefV1>,
}

fn recorded_prepare(record: &[u8]) -> RecordedPrepare {
    let header_len = offset(TRANSACTION_PREPARE_HEADER_LEN_V1);
    let preflight =
        preflight_prepare_record_v1(&record[..header_len], LogPackDecodeLimits::UNBOUNDED)
            .expect("the appended bytes begin with an authenticated incremental prepare header");
    let payload_end = record.len() - CHECKSUM_FIELD_LEN;
    let checksum = decode_crc32c(
        record[payload_end..]
            .try_into()
            .expect("a prepare record ends with its checksum field"),
    )
    .expect("the checksum field of the prepare record holds a CRC32C");
    assert_eq!(
        checksum,
        crc32c(&record[..payload_end]),
        "the checksum of the prepare record must cover the bytes before it"
    );
    let mutation_end = header_len + offset(preflight.mutation_len());
    let mutation = MutationBatchViewV1::parse(&record[header_len..mutation_end])
        .expect("the prepare record carries one canonical mutation batch");
    let batch = RecordedBatch {
        data: mutation
            .data()
            .map(|entry| {
                (entry.key.to_vec(), (entry.transition.before(), entry.transition.after()))
            })
            .collect(),
        continuations: mutation
            .continuations()
            .map(|entry| {
                (entry.key.to_vec(), (entry.transition.before(), entry.transition.after()))
            })
            .collect(),
        joins: mutation
            .joins()
            .map(|entry| {
                (entry.key.to_vec(), (entry.transition.before(), entry.transition.after()))
            })
            .collect(),
    };
    let mut written = Vec::with_capacity(offset(preflight.object_count()));
    let mut cursor = mutation_end;
    for _ in 0..preflight.object_count() {
        let body = cursor + offset(OBJECT_RECORD_HEADER_LEN_V1);
        let header = LogPackObjectHeaderV1::parse_exact(&record[cursor..body])
            .expect("an object record of the prepare record begins with its header");
        cursor = match header.disposition() {
            LogPackObjectDispositionV1::Data => {
                written.push(header.reference());
                body + offset(header.reference().encoded_len())
            }
            LogPackObjectDispositionV1::Reuse => body,
        };
    }
    assert_eq!(
        cursor, payload_end,
        "the object records of the prepare record must end at its checksum"
    );
    RecordedPrepare {
        record_len: preflight.record_len(),
        sequence: preflight.sequence().get(),
        base: preflight.base(),
        target: preflight.target(),
        identity: PrepareRecordIdentityV1::from_streamed(
            preflight,
            PhysicalDigest::of_bytes(&record[..payload_end]),
            checksum,
        ),
        batch,
        written,
    }
}

#[derive(Debug, PartialEq, Eq)]
struct AppendedBytes {
    pack: u64,
    index: u64,
}

struct CrossBasePublication {
    appended: AppendedBytes,
    previous_state_bytes: u64,
}

fn cross_base_publication(shared: usize) -> CrossBasePublication {
    let mut trial = Trial::open();
    for index in 0..shared {
        trial.hot.put_datum(
            &channel(SHARED_CHANNEL),
            Datum::create(&channel(SHARED_CHANNEL), format!("shared-{index:03}"), false),
        );
    }
    let base_root = trial.publish(&[Step::put("a", "v0"), Step::install(&["a", "b"], "k0")]);
    let previous_root = trial.publish(&[
        Step::put("a", "v0"),
        Step::put("b", "v1"),
        Step::withdraw(&["a", "b"], "k0"),
    ]);
    let mut view = trial.view_of_state(1);
    apply_all(&view.hot, &mut view.contents, &[Step::take("a", "v0"), Step::put("c", "v1")]);
    let bounded = |max_object_reads| ViewCheckpointLimitsV1 {
        publication: publication_limits(),
        difference: StateDifferenceLimits {
            max_object_reads,
            max_object_read_bytes: 1 << 16,
            max_worklist: 16,
            max_path_bytes: 1 << 12,
        },
    };
    let untouched = directory_image(trial.path());
    let starved = trial
        .checkpoint_with(view.base, &view.hot, bounded(1))
        .map_err(|refusal| *refusal);
    assert!(
        matches!(starved, Err(DurableRSpaceTransactionError::Rebase(_))) &&
            directory_image(trial.path()) == untouched &&
            matches!(trial.gate.try_write(), Ok(_)),
        "a difference walk limited to one object read must be refused before any write, with the \
         view gate returned: {starved:?}"
    );

    let files_before = store_files(trial.path());
    let before = catalog_token(trial.path());
    let (reached, effects) =
        observed_effects(|| trial.checkpoint_with(view.base, &view.hot, bounded(32)));
    let reached = reached.expect("the cross-base checkpoint publishes within 32 object reads");
    let files_after = store_files(trial.path());
    let after = catalog_token(trial.path());
    let (previous, successor) = (before.slot(), after.slot());

    assert_eq!(effects, PublicationEffects::trusted(), "one trusted incremental publication runs");
    assert!(
        files_before.keys().eq(files_after.keys()),
        "a view checkpoint must neither create nor remove a store file"
    );
    assert_eq!(
        (successor.active_pack(), successor.pack().index_generation()),
        (previous.active_pack(), previous.pack().index_generation()),
        "a view checkpoint appends to the active pack and its index"
    );
    let pack_name =
        pack_file_name(PackId::new(previous.active_pack()).expect("the active pack has an id"));
    let index_name = index_file_name(
        IndexGenerationV1::new(previous.pack().index_generation())
            .expect("the appointed index has a generation"),
    );
    let (pack_before, pack_after) =
        (store_file(&files_before, &pack_name), store_file(&files_after, &pack_name));
    let (index_before, index_after) =
        (store_file(&files_before, &index_name), store_file(&files_after, &index_name));
    assert_eq!(
        (
            pack_before.len(),
            pack_after.len(),
            index_before.len(),
            index_after.len(),
            pack_after.starts_with(pack_before),
            index_after.starts_with(index_before),
        ),
        (
            offset(previous.durable_pack_tail()),
            offset(successor.durable_pack_tail()),
            offset(previous.durable_index_tail()),
            offset(successor.durable_index_tail()),
            true,
            true,
        ),
        "the pack and the index are appended in place and end at the catalog's durable tails"
    );

    let commit_address = successor.commit_address();
    let appended = &pack_after[pack_before.len()..];
    let prepare_bytes = &pack_after[pack_before.len()..offset(commit_address.offset())];
    let commit_bytes = &pack_after[offset(commit_address.offset())..];
    let prepare = recorded_prepare(prepare_bytes);
    let commit = TransactionCommitViewV1::parse(commit_bytes)
        .expect("the appended bytes end with one incremental commit record");
    assert_eq!(
        (
            matches!(
                decode_prepare_kind_v1(&prepare_bytes[..offset(PREPARE_DISCRIMINATOR_LEN_V1)]),
                Ok(PrepareRecordKindV1::Incremental)
            ),
            matches!(
                decode_commit_kind_v1(&commit_bytes[..offset(COMMIT_DISCRIMINATOR_LEN_V1)]),
                Ok(CommitRecordKindV1::Incremental)
            ),
            appended.len(),
            commit_bytes.len(),
            commit_address.pack_id().get(),
            commit.verify_prepare_identity(prepare.identity).is_ok(),
            successor.verify_commit(commit).is_ok(),
            commit.prepare_address().offset(),
            commit.prepare_address().length(),
        ),
        (
            true,
            true,
            offset(prepare.record_len) + offset(TRANSACTION_COMMIT_RECORD_LEN_V1),
            offset(TRANSACTION_COMMIT_RECORD_LEN_V1),
            previous.active_pack(),
            true,
            true,
            previous.durable_pack_tail(),
            prepare.record_len,
        ),
        "the appended pack bytes are exactly one incremental prepare record and the commit record \
         that the successor's catalog slot names"
    );
    assert_eq!(
        (
            prepare.base,
            prepare.target,
            prepare.sequence,
            successor.commit_sequence().get(),
            successor.state().generation(),
            successor.epoch(),
            successor.state().manifest(),
            trial.owner.current_root(),
        ),
        (
            previous.state(),
            successor.state(),
            previous.commit_sequence().get() + 1,
            previous.commit_sequence().get() + 1,
            previous.state().generation() + 1,
            previous.epoch() + 1,
            reached.manifest(),
            reached.root(),
        ),
        "the record pair rewrites the previous current state into the returned successor, one \
         sequence number, one generation and one catalog epoch later"
    );
    assert!(
        after.image() ==
            &before
                .successor_image(successor)
                .expect("the successor slot follows the previous catalog image"),
        "the catalog changed by exactly one compare-and-swap of its inactive slot"
    );

    let restored_source = continuation_source(&["a", "b"], "k0");
    assert_eq!(
        prepare.batch,
        RecordedBatch {
            data: BTreeMap::from([
                (datum_path("a", "v0"), (2, 0)),
                (datum_path("b", "v1"), (1, 0)),
                (datum_path("c", "v1"), (0, 1)),
            ]),
            continuations: BTreeMap::from([(
                continuation_path(&["a", "b"], "k0"),
                (None, Some(restored_source))
            )]),
            joins: BTreeMap::from([
                (join_path("a", &["a", "b"]), (0, 1)),
                (join_path("b", &["a", "b"]), (0, 1)),
            ]),
        },
        "the recorded batch takes every before from the previous current state: the journal's \
         endpoints end at the journal's values and every other differing endpoint at the base's"
    );
    assert!(
        !prepare.written.contains(&restored_source),
        "the continuation source the base already stores must not be written again"
    );

    let shared_data = |root| {
        let (resolved, handle) = trial
            .owner
            .published_core()
            .read_handle(Some(root))
            .expect("a retained state takes a read handle");
        DurableViewReaderV1::new(handle, resolved.roots())
            .get_data_canonical(&channel(SHARED_CHANNEL))
            .len()
    };
    assert_eq!(
        (
            read_contents(&trial.owner, reached.root()),
            read_contents(&trial.owner, previous_root),
            read_contents(&trial.owner, base_root),
            [reached.root(), previous_root, base_root].map(shared_data),
        ),
        (view.contents.clone(), trial.states[2].1.clone(), trial.states[1].1.clone(), [shared; 3],),
        "the successor, the previous current state and the base all still resolve and read their \
         own contents and the data the three states share"
    );
    CrossBasePublication {
        appended: AppendedBytes {
            pack: successor.durable_pack_tail() - previous.durable_pack_tail(),
            index: successor.durable_index_tail() - previous.durable_index_tail(),
        },
        previous_state_bytes: previous.pack().current_state_canonical_bytes(),
    }
}

#[test]
fn a_cross_base_checkpoint_appends_one_record_pair_and_swaps_the_catalog_once() {
    let few = cross_base_publication(1);
    let many = cross_base_publication(96);
    assert!(
        many.previous_state_bytes > few.previous_state_bytes,
        "the second store's states must share more data than the first store's: {} and {} bytes",
        many.previous_state_bytes,
        few.previous_state_bytes
    );
    assert_eq!(
        many.appended, few.appended,
        "the bytes appended by a cross-base checkpoint must not grow with the endpoints the two \
         states share"
    );
}

#[test]
fn an_empty_journal_and_an_unchanged_current_state_write_and_sync_nothing() {
    assert!(
        DIRECTORY_FAILPOINTS
            .iter()
            .enumerate()
            .all(|(index, failpoint)| directory_failpoint_index(*failpoint) == index),
        "the failpoint list must hold every directory failpoint once"
    );
    let mut trial = Trial::open();
    trial.owner.disable_transition_memo_for_test();
    let retained_root = trial.publish(&[Step::put("a", "v0"), Step::install(&["b", "a"], "k1")]);
    let current_root = trial.publish(&[Step::put("c", "v1"), Step::take("a", "v0")]);
    let mut views = [
        trial.view_of_state(1),
        trial.view_of_state(0),
        trial.view_of_state(2),
        trial.view_of_state(1),
    ];
    apply_all(&views[1].hot, &mut views[1].contents, &[
        Step::put("c", "v1"),
        Step::install(&["b", "a"], "k1"),
    ]);
    apply_all(&views[3].hot, &mut views[3].contents, &[
        Step::put("b", "v0"),
        Step::take("b", "v0"),
        Step::install(&["a"], "k0"),
        Step::withdraw(&["a"], "k0"),
    ]);
    assert_eq!(
        views.each_ref().map(|view| view.contents.clone()),
        [1, 2, 2, 1].map(|index: usize| trial.states[index].1.clone()),
        "the four views end at their own base or at the current state"
    );
    let journals = views.each_ref().map(|view| journal_bags(&view.hot));
    let files = directory_image(trial.path());
    let slot = trial.slot();
    let outcomes = |trial: &mut Trial| {
        views.each_ref().map(|view| {
            trial
                .checkpoint(view.base, &view.hot)
                .map(|reached| reached.root())
                .map_err(|refusal| format!("{refusal:?}"))
        })
    };
    let baseline = outcomes(&mut trial);
    assert_eq!(
        baseline,
        [retained_root, current_root, current_root, retained_root].map(Ok),
        "an empty journal returns its base and a batch without effect returns the current state"
    );

    let mut observations = Vec::with_capacity(DIRECTORY_FAILPOINTS.len());
    for failpoint in DIRECTORY_FAILPOINTS {
        observations.push(std::thread::scope(|scope| {
            scope
                .spawn(|| {
                    arm_durable_io_failpoint(failpoint);
                    let observed = outcomes(&mut trial);
                    (failpoint, observed, armed_durable_io_failpoint())
                })
                .join()
                .expect("the failpoint thread completes")
        }));
    }
    for (failpoint, observed, still_armed) in &observations {
        assert_eq!(
            observed, &baseline,
            "a checkpoint changed its outcome with {failpoint:?} armed"
        );
        assert_eq!(
            *still_armed,
            Some(*failpoint),
            "a checkpoint without effect reached the {failpoint:?} failpoint and cleared it"
        );
    }
    assert_eq!(
        (
            directory_image(trial.path()) == files,
            trial.slot(),
            trial.owner.current_root(),
            views.each_ref().map(|view| journal_bags(&view.hot)) == journals,
            matches!(trial.gate.try_write(), Ok(_)),
        ),
        (true, slot, current_root, true, true),
        "checkpoints without effect must leave the store directory, the catalog slot, the current \
         state, the journals and the view gate as they were"
    );

    let mut publishing = trial.view_of_state(1);
    apply_all(&publishing.hot, &mut publishing.contents, &[Step::put("b", "v1")]);
    let (tripped, cleared) = std::thread::scope(|scope| {
        scope
            .spawn(|| {
                arm_durable_io_failpoint(DurableIoFailpointV1::PhysicalWrite);
                let refusal = trial
                    .checkpoint(publishing.base, &publishing.hot)
                    .err()
                    .map(|refusal| format!("{refusal:?}"));
                let cleared = armed_durable_io_failpoint().is_none();
                (refusal, cleared)
            })
            .join()
            .expect("the liveness control thread completes")
    });
    assert!(
        cleared &&
            tripped.as_deref().is_some_and(|refusal| {
                refusal.contains("injected durable I/O failure at PhysicalWrite")
            }),
        "a publishing view checkpoint in a failpoint thread must trip and clear PhysicalWrite: \
         {tripped:?} {cleared}"
    );
    let reconciled = trial
        .owner
        .reconcile(publication_limits())
        .expect("the owner reconciles the interrupted publication");
    let twin = twin_checkpoint(&trial.states[1].1, &[Step::put("b", "v1")]);
    let retried = trial
        .checkpoint(publishing.base, &publishing.hot)
        .expect("the interrupted view checkpoint publishes when it is retried");
    assert_eq!(
        (reconciled, retried.root(), trial.slot().state().generation()),
        (slot.state(), twin.successor, slot.state().generation() + 1),
        "the interrupted publication reconciles to the previous current state and its retry \
         publishes the successor of the view's base one generation later"
    );
}

#[derive(Debug, PartialEq, Eq)]
struct OwnerImage {
    root: DurableStateRootV1,
    generation: GenerationLeaseKeyV1,
    terminal: Option<CatalogSlotV1>,
    phase: JournalPublicationPhaseV1,
}

fn owner_image(owner: &Owner, observer: &Store) -> OwnerImage {
    OwnerImage {
        root: owner.current_root(),
        generation: owner.generation_key(),
        terminal: owner.pending_terminal(),
        phase: owner.phase(observer),
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Untouched {
    captured: bool,
    effects: PublicationEffects,
    directory: bool,
    journals: bool,
    owner: bool,
    permit_returned: bool,
}

fn untouched() -> Untouched {
    Untouched {
        captured: false,
        effects: PublicationEffects {
            trace: Vec::new(),
            cold_admissions: 0,
            index_constructions: 0,
        },
        directory: true,
        journals: true,
        owner: true,
        permit_returned: true,
    }
}

struct RefusedCall<'call> {
    owner: &'call mut Owner,
    directory: &'call Path,
    permits: &'call Arc<RwLock<()>>,
    gate: &'call Arc<RwLock<()>>,
    observer: &'call Store,
}

impl RefusedCall<'_> {
    fn refuse(
        &mut self,
        base: ViewBaseV1,
        hot: &Store,
    ) -> (Option<DurableRSpaceTransactionError>, Untouched) {
        let files = directory_image(self.directory);
        let journals = journal_bags(hot);
        let owner = owner_image(self.owner, self.observer);
        let captured = Rc::new(Cell::new(false));
        let hook = journal_owner_hooks::arm(JournalOwnerHookPoint::AfterCapture, {
            let captured = Rc::clone(&captured);
            move || captured.set(true)
        });
        let (result, effects) = observed_effects(|| {
            poll_once(
                self.owner.checkpoint_view_from_base(
                    Arc::clone(self.permits)
                        .try_write_owned()
                        .expect("the gate that lends the permit is free"),
                    base,
                    self.gate,
                    hot,
                    view_limits(),
                ),
            )
            .map_err(Box::new)
        });
        drop(hook);
        (result.err().map(|refusal| *refusal), Untouched {
            captured: captured.get(),
            effects,
            directory: directory_image(self.directory) == files,
            journals: journal_bags(hot) == journals,
            owner: owner_image(self.owner, self.observer) == owner,
            permit_returned: matches!(self.permits.try_write(), Ok(_)),
        })
    }

    fn refuse_without_reading_the_journal(
        &mut self,
        base: ViewBaseV1,
        hot: &Store,
        unreadable: &Store,
    ) -> (Option<DurableRSpaceTransactionError>, bool) {
        let (refusal, changes) = self.refuse(base, hot);
        let (blind_refusal, blind_changes) = self.refuse(base, unreadable);
        let same = format!("{refusal:?}") == format!("{blind_refusal:?}");
        (refusal, same && changes == untouched() && blind_changes == untouched())
    }
}

fn journal_conflict(refusal: Option<&DurableRSpaceTransactionError>) -> Option<&str> {
    match refusal {
        Some(DurableRSpaceTransactionError::Journal(RSpaceError::JournalConflict(text))) => {
            Some(text)
        }
        Some(_) | None => None,
    }
}

fn unreadable_store(owner: &Owner, root: DurableStateRootV1) -> Store {
    let (_, hot) = lazy_store(owner, root, GenerationLeaseRegistryV1::default());
    hot.put_datum(&channel("a"), datum("a", "v1"));
    hot
}

#[derive(Clone, Copy)]
struct ManifestShape {
    generation: u64,
    profile: SemanticProfileId,
    legacy_history_root: Option<LegacyHistoryRoot>,
}

fn encoded<T: CanonicalEncode>(value: &T) -> (ObjectRefV1, Vec<u8>) {
    let bytes = encode(value);
    let length = u64::try_from(bytes.len()).expect("a canonical encoding's length fits u64");
    let reference = ObjectRefV1::new(ObjectDigest::of_canonical_bytes(&bytes), length)
        .expect("a canonical encoding has a valid object reference");
    (reference, bytes)
}

fn checkpoint_source_of_empty_states(
    current: ManifestShape,
    historical: &[ManifestShape],
) -> (SyntheticCheckpointSource, Vec<ObjectRefV1>) {
    let empty = encoded(&CanonicalDagNode::empty());
    let roots = empty_roots(empty.0);
    let manifest_of = |shape: ManifestShape| {
        encoded(
            &SemanticStateManifestV1::new(1, shape.profile, roots, shape.legacy_history_root)
                .expect("a schema-one manifest is valid"),
        )
    };
    let closure_bytes = |manifest: ObjectRefV1| empty.0.encoded_len() + manifest.encoded_len();
    let mut manifests = Vec::with_capacity(historical.len() + 1);
    let mut retained = Vec::with_capacity(historical.len());
    for shape in historical {
        let manifest = manifest_of(*shape);
        retained.push(
            RetainedStateV1::new(
                StateVersionV1::new(shape.generation, manifest.0),
                closure_bytes(manifest.0),
            )
            .expect("a historical state claims its closure bytes"),
        );
        manifests.push(manifest);
    }
    let current_manifest = manifest_of(current);
    let current_state = StateVersionV1::new(current.generation, current_manifest.0);
    let current_bytes = closure_bytes(current_manifest.0);
    manifests.push(current_manifest);
    let retention = encoded(
        &RetentionManifestV1::new(
            RetainedStateV1::new(current_state, current_bytes)
                .expect("the current state claims its closure bytes"),
            retained,
        )
        .expect("the retention manifest is valid"),
    );
    let references = manifests
        .iter()
        .map(|(reference, _)| *reference)
        .collect::<Vec<_>>();
    let retained_union = RetainedUnionClosureReportV1 {
        distinct_objects: u64::try_from(manifests.len() + 1)
            .expect("the retained object count fits u64"),
        canonical_bytes: empty.0.encoded_len() +
            references
                .iter()
                .map(|reference| reference.encoded_len())
                .sum::<u64>(),
    };
    let retention_reference = retention.0;
    let mut objects = Vec::with_capacity(manifests.len() + 2);
    objects.push(empty);
    objects.extend(manifests);
    objects.push(retention);
    objects.sort_unstable_by_key(|(reference, _)| reference.digest());
    let data_bytes = objects
        .iter()
        .map(|(reference, _)| reference.encoded_len())
        .sum();
    let mut ordered = OrderedObjectInventoryBuilderV1::new(
        u64::try_from(objects.len()).expect("the object count fits u64"),
        data_bytes,
    );
    for (reference, _) in &objects {
        ordered
            .push(*reference)
            .expect("the objects are distinct and digest ordered");
    }
    let inventory = CheckpointObjectInventoryV1::from_test_ordered(
        retained_union,
        retention_reference,
        ordered.finish().expect("the inventory is complete"),
    )
    .expect("the inventory is the retained union plus its retention manifest");
    let mut source = checkpoint_source_fixture();
    source.descriptor = GenerationCheckpointDescriptorV1::from_test_inventory(
        current.profile,
        current_state,
        retention_reference,
        2,
        current_bytes,
        inventory,
    )
    .expect("the descriptor satisfies the V1 inventory rule");
    source.objects = objects;
    source.sort_objects_by_digest();
    (source, references)
}

struct EmptyStatesStore {
    hot: Store,
    gate: Arc<RwLock<()>>,
    owner: Owner,
    manifests: Vec<ObjectRefV1>,
    scratch: PersistenceTrialDir,
}

fn store_of_empty_states(current: ManifestShape, historical: &[ManifestShape]) -> EmptyStatesStore {
    let (source, manifests) = checkpoint_source_of_empty_states(current, historical);
    let scratch = trial_scratch("owner-cross-base-inheritance-");
    install_checkpoint_image(scratch.path(), &checkpoint_image_for_profile::<Profile>(source));
    let view = recover_read_authority::<Profile>(
        scratch.path(),
        limits(),
        LogPackDecodeLimits::UNBOUNDED,
        DurableAdmissionLimitsV1::UNBOUNDED,
    )
    .expect("the store of empty states recovers");
    let hot: Store = Arc::new(
        HotStoreInstances::create_from_durable(&view, DurableHotStoreBootstrapLimits::default())
            .expect("the hot store loads the empty current state"),
    );
    let owner = view.into_rspace_authority(&hot).unwrap_or_else(|failure| {
        panic!("the view of empty states hands over its authority: {:?}", failure.1)
    });
    EmptyStatesStore {
        hot,
        gate: Arc::new(RwLock::new(())),
        owner,
        manifests,
        scratch,
    }
}

#[test]
fn every_refusal_precedes_the_first_write_and_changes_nothing() {
    let mut trial = Trial::open();
    let retained_root = trial.publish(&[Step::put("a", "v0"), Step::install(&["a", "b"], "k0")]);
    trial.publish(&[Step::put("b", "v1"), Step::withdraw(&["a", "b"], "k0")]);
    let mut view = trial.view_of_state(1);
    apply_all(&view.hot, &mut view.contents, &[Step::take("a", "v0"), Step::put("c", "v0")]);
    let mut waiting = trial.view_of_state(0);
    apply_all(&waiting.hot, &mut waiting.contents, &[Step::put("b", "v0")]);
    let mut late = trial.view_of_state(1);
    apply_all(&late.hot, &mut late.contents, &[Step::put("c", "v1")]);
    let unreadable = unreadable_store(&trial.owner, retained_root);
    let foreign_gate = Arc::new(RwLock::new(()));
    let other_gate = Arc::new(RwLock::new(()));

    let control = RefusedCall {
        owner: &mut trial.owner,
        directory: trial.scratch.path(),
        permits: &trial.gate,
        gate: &trial.gate,
        observer: &trial.hot,
    }
    .refuse(view.base, &unreadable);
    assert_eq!(
        (journal_conflict(control.0.as_ref()), &control.1),
        (Some("durable journal belongs to another directory authority"), &untouched()),
        "a call that passes every refusal of step 1 must be refused by the journal of a store of \
         another lease registry, which shows that such a store's journal cannot be read: \
         {control:?}"
    );

    let foreign_permit = RefusedCall {
        owner: &mut trial.owner,
        directory: trial.scratch.path(),
        permits: &foreign_gate,
        gate: &trial.gate,
        observer: &trial.hot,
    }
    .refuse_without_reading_the_journal(view.base, &view.hot, &unreadable);
    assert!(
        matches!(foreign_permit, (Some(DurableRSpaceTransactionError::ForeignPermit), true)) &&
            matches!(trial.gate.try_write(), Ok(_)),
        "a permit of another gate must be refused before the journal is read, changing nothing: \
         {foreign_permit:?}"
    );

    let stranger = {
        let mut other = Trial::open();
        let root = other.publish(&[Step::put("c", "v1"), Step::put("c", "v1")]);
        other.base(root).state()
    };
    let unknown_base = RefusedCall {
        owner: &mut trial.owner,
        directory: trial.scratch.path(),
        permits: &trial.gate,
        gate: &trial.gate,
        observer: &trial.hot,
    }
    .refuse_without_reading_the_journal(
        ViewBaseV1::new(view.base.incarnation(), stranger),
        &view.hot,
        &unreadable,
    );
    assert!(
        matches!(
            &unknown_base,
            (Some(DurableRSpaceTransactionError::Root(RootError::UnknownRootError(text))), true)
                if *text == format!(
                    "durable state root {} is not a current or retained state of this store",
                    stranger.root()
                )
        ),
        "a base that this store does not retain must be refused as an unknown root before the \
         journal is read, changing nothing: {unknown_base:?}"
    );

    arm_generation_checkpoint_publication_failpoint(
        GenerationCheckpointPublicationHookPointV1::AfterCatalogSyncBeforeRecovery,
    );
    let uncertain = trial.checkpoint(view.base, &view.hot).is_err();
    let terminal = trial.owner.pending_terminal();
    let pending = RefusedCall {
        owner: &mut trial.owner,
        directory: trial.scratch.path(),
        permits: &other_gate,
        gate: &other_gate,
        observer: &trial.hot,
    }
    .refuse_without_reading_the_journal(waiting.base, &waiting.hot, &unreadable);
    let held = matches!(trial.gate.try_write(), Err(_));
    let reconciled = trial.owner.reconcile(publication_limits()).ok();
    let released = matches!(trial.gate.try_write(), Ok(_));
    let published = trial
        .checkpoint(waiting.base, &waiting.hot)
        .map(|_| trial.slot().state().generation())
        .ok();
    assert!(
        uncertain &&
            terminal.is_some() &&
            matches!(
                pending,
                (
                    Some(DurableRSpaceTransactionError::Unresolved(
                        JournalPublicationPhaseV1::RecoveryRequired
                    )),
                    true
                )
            ) &&
            held &&
            reconciled == terminal.map(|slot| slot.state()) &&
            released &&
            published == terminal.map(|slot| slot.state().generation() + 1),
        "a pending record must refuse another view's checkpoint as unresolved before the journal \
         is read, changing nothing; reconciliation then reaches the terminal and the refused view \
         publishes: {pending:?} {reconciled:?} {published:?}"
    );

    trial.owner.close().expect("the quiescent owner closes");
    let closed = RefusedCall {
        owner: &mut trial.owner,
        directory: trial.scratch.path(),
        permits: &trial.gate,
        gate: &trial.gate,
        observer: &trial.hot,
    }
    .refuse_without_reading_the_journal(late.base, &late.hot, &unreadable);
    assert!(
        matches!(closed, (Some(DurableRSpaceTransactionError::Closed), true)),
        "a closed owner must refuse before the journal is read, changing nothing: {closed:?}"
    );

    let reopened_view = recover_read_authority::<Profile>(
        trial.scratch.path(),
        limits(),
        LogPackDecodeLimits::UNBOUNDED,
        DurableAdmissionLimitsV1::UNBOUNDED,
    )
    .expect("the closed store recovers");
    let reopened_hot: Store = Arc::new(
        HotStoreInstances::create_from_durable(
            &reopened_view,
            DurableHotStoreBootstrapLimits::default(),
        )
        .expect("the hot store loads the recovered state"),
    );
    let mut reopened = reopened_view
        .into_rspace_authority(&reopened_hot)
        .unwrap_or_else(|failure| {
            panic!("the recovered view hands over its authority: {:?}", failure.1)
        });
    let reopened_gate = Arc::new(RwLock::new(()));
    let fresh = reopened
        .attach(&reopened_hot)
        .expect("the reopened owner's store attaches")
        .incarnation();
    let generation = trial.slot().state().generation();
    let stale = RefusedCall {
        owner: &mut reopened,
        directory: trial.scratch.path(),
        permits: &reopened_gate,
        gate: &reopened_gate,
        observer: &reopened_hot,
    }
    .refuse_without_reading_the_journal(late.base, &late.hot, &unreadable);
    let twin = twin_checkpoint(&trial.states[1].1, &[Step::put("c", "v1")]);
    let renamed = poll_once(
        reopened.checkpoint_view_from_base(
            Arc::clone(&reopened_gate)
                .try_write_owned()
                .expect("the reopened owner's view gate is free"),
            ViewBaseV1::new(fresh, late.base.state()),
            &reopened_gate,
            &late.hot,
            view_limits(),
        ),
    )
    .map(|reached| (reached.root(), trial.slot().state().generation()))
    .map_err(Box::new);
    assert!(
        matches!(
            &stale,
            (
                Some(DurableRSpaceTransactionError::Attachment(
                    ViewAttachmentRefusalV1::StaleIncarnation { attached, current }
                )),
                true
            ) if *attached == late.base.incarnation() && *current == fresh
        ) && matches!(&renamed, Ok(reached) if *reached == (twin.successor, generation + 1)),
        "a view of an earlier owner incarnation must be refused before the journal is read, \
         changing nothing, and the same journal must publish once its base names the reopened \
         owner: {stale:?} {renamed:?}"
    );

    let shape = |generation, profile, legacy_history_root| ManifestShape {
        generation,
        profile,
        legacy_history_root,
    };
    let legacy_root = LegacyHistoryRoot::from_array([0x33; 32]);
    let mut legacy = store_of_empty_states(shape(9, Profile::ID, Some(legacy_root)), &[shape(
        1,
        Profile::ID,
        None,
    )]);
    let retained = DurableStateRootV1::of_manifest(legacy.manifests[0]);
    let (retained_state, retained_hot) =
        lazy_store(&legacy.owner, retained, legacy.owner.generation_leases());
    retained_hot.put_datum(&channel("a"), datum("a", "v0"));
    let legacy_base = ViewBaseV1::new(
        legacy
            .owner
            .attach(&legacy.hot)
            .expect("the store of a legacy current state attaches")
            .incarnation(),
        retained_state,
    );
    let legacy_unreadable = unreadable_store(&legacy.owner, retained);
    let inherited_legacy_root = RefusedCall {
        owner: &mut legacy.owner,
        directory: legacy.scratch.path(),
        permits: &legacy.gate,
        gate: &legacy.gate,
        observer: &legacy.hot,
    }
    .refuse_without_reading_the_journal(legacy_base, &retained_hot, &legacy_unreadable);
    assert!(
        matches!(
            &inherited_legacy_root,
            (Some(DurableRSpaceTransactionError::Inheritance { current, base }), true)
                if current.legacy_history_root() == Some(legacy_root) &&
                    base.legacy_history_root().is_none() &&
                    current.profile_id() == base.profile_id() &&
                    current.schema_epoch() == base.schema_epoch()
        ),
        "a current state whose manifest carries a legacy-history root must refuse a base without \
         one before the journal is read, changing nothing: {inherited_legacy_root:?}"
    );

    let mut mixed = store_of_empty_states(shape(9, Profile::ID, None), &[shape(
        1,
        TEST_SEMANTIC_PROFILE_ID,
        None,
    )]);
    let other_profile_state = {
        let (source, _) =
            checkpoint_source_of_empty_states(shape(9, TEST_SEMANTIC_PROFILE_ID, None), &[]);
        let scratch = trial_scratch("owner-cross-base-profile-");
        install_checkpoint_image(
            scratch.path(),
            &checkpoint_image_for_profile::<TestSemanticProfile>(source),
        );
        let view = recover_read_authority::<TestSemanticProfile>(
            scratch.path(),
            limits(),
            LogPackDecodeLimits::UNBOUNDED,
            DurableAdmissionLimitsV1::UNBOUNDED,
        )
        .expect("the store of the other profile recovers");
        view.resolve(DurableStateRootV1::of_manifest(view.state().manifest()))
            .expect("the other profile's current state resolves in its own store")
    };
    mixed.hot.put_datum(&channel("a"), datum("a", "v0"));
    let mixed_base = ViewBaseV1::new(
        mixed
            .owner
            .attach(&mixed.hot)
            .expect("the store with a retained state of another profile attaches")
            .incarnation(),
        other_profile_state,
    );
    let mixed_unreadable = unreadable_store(&mixed.owner, mixed.owner.current_root());
    let inherited_profile = RefusedCall {
        owner: &mut mixed.owner,
        directory: mixed.scratch.path(),
        permits: &mixed.gate,
        gate: &mixed.gate,
        observer: &mixed.hot,
    }
    .refuse_without_reading_the_journal(mixed_base, &mixed.hot, &mixed_unreadable);
    assert!(
        other_profile_state.manifest() == mixed.manifests[0] &&
            matches!(
                &inherited_profile,
                (Some(DurableRSpaceTransactionError::Inheritance { current, base }), true)
                    if current.profile_id() == Profile::ID &&
                        base.profile_id() == TEST_SEMANTIC_PROFILE_ID &&
                        current.legacy_history_root().is_none() &&
                        base.legacy_history_root().is_none() &&
                        current.schema_epoch() == base.schema_epoch()
            ),
        "a retained base whose manifest names another profile than the current state must be \
         refused before the journal is read, changing nothing: {inherited_profile:?}"
    );
}

const OWNER_HOOK_POINTS: [JournalOwnerHookPoint; 6] = [
    JournalOwnerHookPoint::AfterCapture,
    JournalOwnerHookPoint::AfterPreparation,
    JournalOwnerHookPoint::BeforeSuccessorPin,
    JournalOwnerHookPoint::AfterSuccessorPin,
    JournalOwnerHookPoint::AfterAcknowledgement,
    JournalOwnerHookPoint::AfterPublishedCoreLoad,
];

const fn owner_hook_point_index(point: JournalOwnerHookPoint) -> usize {
    match point {
        JournalOwnerHookPoint::AfterCapture => 0,
        JournalOwnerHookPoint::AfterPreparation => 1,
        JournalOwnerHookPoint::BeforeSuccessorPin => 2,
        JournalOwnerHookPoint::AfterSuccessorPin => 3,
        JournalOwnerHookPoint::AfterAcknowledgement => 4,
        JournalOwnerHookPoint::AfterPublishedCoreLoad => 5,
    }
}

fn interruption_of<T: std::fmt::Debug>(run: impl FnOnce() -> T) -> String {
    match catch_unwind(AssertUnwindSafe(run)) {
        Ok(completed) => format!("completed with {completed:?}"),
        Err(payload) => panic_text(payload.as_ref()).to_owned(),
    }
}

#[test]
fn a_crash_at_every_owner_hook_point_recovers_the_old_state_or_the_successor() {
    assert!(
        OWNER_HOOK_POINTS
            .iter()
            .enumerate()
            .all(|(index, point)| owner_hook_point_index(*point) == index),
        "the hook point list must hold every journal owner hook point once"
    );
    let journal =
        [Step::take("a", "v0"), Step::put("c", "v0"), Step::install(&["c", "a", "b"], "k1")];
    for point in OWNER_HOOK_POINTS {
        let mut trial = Trial::open();
        trial.publish(&[Step::put("a", "v0"), Step::install(&["a", "b"], "k0")]);
        trial.publish(&[Step::put("b", "v1"), Step::withdraw(&["a", "b"], "k0")]);
        let mut view = trial.view_of_state(1);
        apply_all(&view.hot, &mut view.contents, &journal);
        let twin = twin_checkpoint(&trial.states[1].1, &journal);
        let generation = trial.slot().state().generation();
        let previous_image = (trial.owner.current_root(), generation, trial.current_contents());
        let successor_image = (twin.successor, generation + 1, twin.contents.clone());
        let previous_files = store_files(trial.path());
        let journals = journal_bags(&view.hot);
        let crash_files: Rc<RefCell<Option<StoreFiles>>> = Rc::new(RefCell::new(None));
        let hook = journal_owner_hooks::arm(point, {
            let crash_files = Rc::clone(&crash_files);
            let directory = trial.path().to_path_buf();
            move || {
                crash_files.replace(Some(store_files(&directory)));
                panic!("{INTERRUPTION}");
            }
        });
        let interruption = match point {
            JournalOwnerHookPoint::AfterCapture |
            JournalOwnerHookPoint::AfterPreparation |
            JournalOwnerHookPoint::BeforeSuccessorPin |
            JournalOwnerHookPoint::AfterSuccessorPin |
            JournalOwnerHookPoint::AfterAcknowledgement => {
                interruption_of(|| trial.checkpoint(view.base, &view.hot))
            }
            JournalOwnerHookPoint::AfterPublishedCoreLoad => {
                let completed = trial
                    .checkpoint(view.base, &view.hot)
                    .map(|reached| reached.root())
                    .map_err(|refusal| format!("{refusal:?}"));
                assert_eq!(
                    (completed, crash_files.borrow().is_none()),
                    (Ok(twin.successor), true),
                    "a view checkpoint must publish its successor without passing {point:?}"
                );
                interruption_of(|| {
                    trial
                        .owner
                        .published_core()
                        .read_handle(None)
                        .map(|(resolved, _)| resolved.root())
                })
            }
        };
        drop(hook);
        let (precedes_the_write, phase, held) = match point {
            JournalOwnerHookPoint::AfterCapture | JournalOwnerHookPoint::AfterPreparation => {
                (true, JournalPublicationPhaseV1::Captured, true)
            }
            JournalOwnerHookPoint::BeforeSuccessorPin |
            JournalOwnerHookPoint::AfterSuccessorPin |
            JournalOwnerHookPoint::AfterAcknowledgement => {
                (false, JournalPublicationPhaseV1::AwaitingAck, true)
            }
            JournalOwnerHookPoint::AfterPublishedCoreLoad => {
                (false, JournalPublicationPhaseV1::Ready, false)
            }
        };
        let recovered = match precedes_the_write {
            true => &previous_image,
            false => &successor_image,
        };
        let crashed = crash_files.borrow_mut().take();
        assert_eq!(
            (
                twin.base,
                interruption.as_str(),
                crashed.as_ref().map(recovered_from),
                crashed.as_ref().map(|files| *files == previous_files),
                matches!(trial.gate.try_write(), Err(_)),
                trial.owner.phase(&view.hot),
            ),
            (
                trial.states[1].0,
                INTERRUPTION,
                Some(recovered.clone()),
                Some(precedes_the_write),
                held,
                phase,
            ),
            "the store files copied when the owner task stops at {point:?} must recover the \
             previous current state, byte for byte the files before the call, or the exact \
             successor of the view's base one generation later, with the view gate still held by \
             a pending record"
        );

        let files = store_files(trial.path());
        let reconciled = trial
            .owner
            .reconcile(publication_limits())
            .map(|state| (DurableStateRootV1::of_manifest(state.manifest()), state.generation()))
            .map_err(|refusal| format!("{refusal:?}"));
        assert_eq!(
            (
                reconciled,
                store_files(trial.path()) == files,
                matches!(trial.gate.try_write(), Ok(_)),
                trial.owner.pending_terminal(),
                trial.owner.current_root(),
                journal_bags(&view.hot) == journals,
                journal_is_empty(&view.hot),
            ),
            (
                Ok((recovered.0, recovered.1)),
                true,
                true,
                None,
                recovered.0,
                precedes_the_write,
                !precedes_the_write,
            ),
            "reconciliation after a stop at {point:?} must complete the record at the state the \
             crash image recovers without writing, release the view gate, and leave the journal \
             intact before the write or acknowledged after it"
        );

        let settled = match precedes_the_write {
            true => trial
                .checkpoint(view.base, &view.hot)
                .map(|reached| reached.root())
                .map_err(|refusal| format!("{refusal:?}")),
            false => Ok(trial.owner.current_root()),
        };
        assert_eq!(
            (settled, trial.owner.current_root(), trial.slot().state().generation()),
            (Ok(twin.successor), twin.successor, generation + 1),
            "after a stop at {point:?} the store must settle on the exact successor of the view's \
             base one generation later, by a retry where the previous state was recovered"
        );
        let registry = trial.owner.generation_leases();
        assert_eq!(
            (
                read_contents(&trial.owner, twin.successor),
                journal_is_empty(&view.hot),
                view.hot
                    .validate_journal_authority(trial.slot(), &registry)
                    .is_ok(),
                trial
                    .states
                    .iter()
                    .all(|(root, contents)| read_contents(&trial.owner, *root) == *contents),
                matches!(trial.gate.try_write(), Ok(_)),
            ),
            (twin.contents.clone(), true, true, true, true),
            "after a stop at {point:?} the settled successor must hold the base with the journal \
             applied, with the journal acknowledged, the view's store bound to the successor, \
             every earlier state still readable and the view gate free"
        );
    }
}

fn bound_to(
    hot: &Store,
    registry: &GenerationLeaseRegistryV1,
    slots: [CatalogSlotV1; 2],
) -> [bool; 2] {
    slots.map(|slot| hot.validate_journal_authority(slot, registry).is_ok())
}

#[derive(Debug, PartialEq, Eq)]
struct CaptureBinding {
    reached: Result<DurableStateRootV1, String>,
    before: [bool; 2],
    at_capture: Option<[bool; 2]>,
    after: [bool; 2],
}

fn binding_through_a_checkpoint(
    trial: &mut Trial,
    base: ViewBaseV1,
    hot: &Store,
    own: CatalogSlotV1,
) -> CaptureBinding {
    let registry = trial.owner.generation_leases();
    let publication_base = trial.slot();
    let before = bound_to(hot, &registry, [publication_base, own]);
    let at_capture = Rc::new(Cell::new(None));
    let hook = journal_owner_hooks::arm(JournalOwnerHookPoint::AfterCapture, {
        let (hot, registry, at_capture) =
            (Arc::clone(hot), registry.clone(), Rc::clone(&at_capture));
        move || at_capture.set(Some(bound_to(&hot, &registry, [publication_base, own])))
    });
    let reached = trial
        .checkpoint(base, hot)
        .map(|reached| reached.root())
        .map_err(|refusal| format!("{refusal:?}"));
    drop(hook);
    CaptureBinding {
        reached,
        before,
        at_capture: at_capture.get(),
        after: bound_to(hot, &registry, [trial.slot(), own]),
    }
}

#[test]
fn a_base_equal_to_the_current_state_publishes_through_the_same_base_path() {
    let mut trial = Trial::open();
    trial.publish(&[Step::put("a", "v0")]);
    trial.publish(&[Step::put("b", "v1")]);
    let bound_slot = trial.slot();

    let mut retained = trial.view_of_state(1);
    apply_all(&retained.hot, &mut retained.contents, &[Step::put("c", "v0")]);
    let retained_twin = twin_checkpoint(&trial.states[1].1, &[Step::put("c", "v0")]);
    let cross_base =
        binding_through_a_checkpoint(&mut trial, retained.base, &retained.hot, bound_slot);

    let mut equal = trial.view(trial.owner.current_root(), retained.contents.clone());
    apply_all(&equal.hot, &mut equal.contents, &[Step::put("c", "v1")]);
    let equal_twin = twin_checkpoint(&retained.contents, &[Step::put("c", "v1")]);
    let equal_base_is_current = equal.base.state().manifest() == trial.slot().state().manifest();
    let same_base = binding_through_a_checkpoint(&mut trial, equal.base, &equal.hot, bound_slot);

    let mut bound = trial.bound_view();
    apply_all(&bound.hot, &mut bound.contents, &[Step::put("a", "v1")]);
    let bound_twin = twin_checkpoint(&trial.states[2].1, &[Step::put("a", "v1")]);
    let superseded = binding_through_a_checkpoint(&mut trial, bound.base, &bound.hot, bound_slot);

    assert!(
        equal_base_is_current && equal_twin.base == equal.base.state().root(),
        "the second view must name the state that is current when it is checkpointed"
    );
    assert_eq!(
        [cross_base, same_base, superseded],
        [
            CaptureBinding {
                reached: Ok(retained_twin.successor),
                before: [false, false],
                at_capture: Some([false, false]),
                after: [true, false],
            },
            CaptureBinding {
                reached: Ok(equal_twin.successor),
                before: [false, false],
                at_capture: Some([true, false]),
                after: [true, false],
            },
            CaptureBinding {
                reached: Ok(bound_twin.successor),
                before: [false, true],
                at_capture: Some([false, true]),
                after: [true, false],
            },
        ],
        "each row reports whether the view's store is bound to the catalog's current slot and to \
         the slot the bound store last published at: a detached store at a retained base and a \
         bound store at a superseded slot must be captured as they are, a detached store at the \
         current state must be bound to the current slot before its capture, and every \
         publication must leave its store bound to the successor"
    );

    let mut misplaced = trial.view_of_state(1);
    apply_all(&misplaced.hot, &mut misplaced.contents, &[Step::put("b", "v0")]);
    let mut current = trial.view(trial.owner.current_root(), bound.contents.clone());
    apply_all(&current.hot, &mut current.contents, &[Step::put("b", "v0")]);
    let refused_binding = RefusedCall {
        owner: &mut trial.owner,
        directory: trial.scratch.path(),
        permits: &trial.gate,
        gate: &trial.gate,
        observer: &trial.hot,
    }
    .refuse(current.base, &misplaced.hot);
    let refused_capture = RefusedCall {
        owner: &mut trial.owner,
        directory: trial.scratch.path(),
        permits: &trial.gate,
        gate: &trial.gate,
        observer: &trial.hot,
    }
    .refuse(misplaced.base, &current.hot);
    assert_eq!(
        (
            journal_conflict(refused_binding.0.as_ref()),
            &refused_binding.1,
            journal_conflict(refused_capture.0.as_ref()),
            &refused_capture.1,
        ),
        (
            Some("catalog slot names another state than the durable journal's base"),
            &untouched(),
            Some("durable journal was recorded against another state than the capture's origin"),
            &untouched(),
        ),
        "a journal recorded at another state than the named base must be refused with nothing \
         changed: by the binding when the base is the current state, which shows that such a base \
         takes the same-base path, and by the cross-base capture's origin comparison when the \
         base is a retained state: {refused_binding:?} {refused_capture:?}"
    );
}

type EndpointRecorder<V> = fn(&mut RSpaceMutationBatchV1, &[u8], V, V) -> Result<(), MutationError>;
type EndpointReader<V> = fn(&RSpaceMutationBatchV1, &[u8]) -> Option<(V, V)>;

struct EndpointDomain<V> {
    namespace: SemanticNamespace,
    path: Vec<u8>,
    spare: Vec<u8>,
    values: Vec<V>,
    endpoint: fn(V) -> EndpointValue,
    record: EndpointRecorder<V>,
    recorded: EndpointReader<V>,
    discontinuity: fn(V, V) -> MutationError,
}

impl<V: Copy + PartialEq + std::fmt::Debug> EndpointDomain<V> {
    fn preceded_cases(&self) -> usize {
        let mut journals = Vec::with_capacity(1 + self.values.len() * self.values.len());
        journals.push(None);
        for before in &self.values {
            for after in self.values.iter().filter(|after| *after != before) {
                journals.push(Some((*before, *after)));
            }
        }
        let mut cases = 0;
        for current in &self.values {
            for base in &self.values {
                for journal in &journals {
                    let mut batch = empty_batch();
                    if let Some((before, after)) = journal {
                        (self.record)(&mut batch, &self.path, *before, *after)
                            .expect("the batch records the journal's own transition");
                    }
                    let observed = batch
                        .precede_endpoint(
                            self.namespace,
                            &self.path,
                            (self.endpoint)(*current),
                            (self.endpoint)(*base),
                        )
                        .map(|()| (self.recorded)(&batch, &self.path));
                    let expected = match journal {
                        None => Ok((current != base).then_some((*current, *base))),
                        Some((before, after)) if before == base => {
                            Ok((current != after).then_some((*current, *after)))
                        }
                        Some((before, _)) => Err((self.discontinuity)(*base, *before)),
                    };
                    assert_eq!(
                        observed, expected,
                        "in {:?}, a difference endpoint from the current value {current:?} to the \
                         base's value {base:?} must precede the journal transition {journal:?} as \
                         one transition from the current value to the journal's after value, \
                         dropped when they are equal, and must be refused when the journal does \
                         not start at the base's value",
                        self.namespace
                    );
                    assert_eq!(
                        batch.precede_endpoint(
                            self.namespace,
                            &self.spare,
                            EndpointValue::Absent,
                            EndpointValue::Absent,
                        ),
                        expected
                            .map(|_| ())
                            .map_err(|_| MutationError::PoisonedBatch),
                        "a refused endpoint must poison the batch and an accepted one must not"
                    );
                    cases += 1;
                }
            }
        }
        cases
    }
}

fn empty_batch() -> RSpaceMutationBatchV1 {
    RSpaceMutationBatchV1::new(StateVersionV1::new(7, synthetic_ref(9)))
        .expect("a mutation batch opens on a state below the generation limit")
}

#[test]
fn a_difference_endpoint_precedes_a_journal_transition_exactly() {
    let count: fn(u64) -> EndpointValue =
        |count| NonZeroU64::new(count).map_or(EndpointValue::Absent, EndpointValue::Multiplicity);
    let count_discontinuity: fn(u64, u64) -> MutationError =
        |expected, actual| MutationError::DiscontinuousCount { expected, actual };
    let data = EndpointDomain {
        namespace: SemanticNamespace::DataBag,
        path: datum_path("a", "v0"),
        spare: datum_path("b", "v1"),
        values: vec![0, 1, 2, 3],
        endpoint: count,
        record: RSpaceMutationBatchV1::record_data,
        recorded: |batch, path| {
            batch
                .data_transition(path)
                .map(|transition| (transition.before(), transition.after()))
        },
        discontinuity: count_discontinuity,
    };
    let joins = EndpointDomain {
        namespace: SemanticNamespace::JoinBag,
        path: join_path("a", &["a"]),
        spare: join_path("b", &["b", "a"]),
        values: vec![0, 1, 2, 3],
        endpoint: count,
        record: RSpaceMutationBatchV1::record_join,
        recorded: |batch, path| {
            batch
                .join_transition(path)
                .map(|transition| (transition.before(), transition.after()))
        },
        discontinuity: count_discontinuity,
    };
    let continuations = EndpointDomain {
        namespace: SemanticNamespace::ContinuationMap,
        path: continuation_path(&["a"], "k0"),
        spare: continuation_path(&["a", "b"], "k1"),
        values: vec![None, Some(synthetic_ref(1)), Some(synthetic_ref(2))],
        endpoint: |content| content.map_or(EndpointValue::Absent, EndpointValue::Content),
        record: RSpaceMutationBatchV1::record_continuation,
        recorded: |batch, path| {
            batch
                .continuation_transition(path)
                .map(|transition| (transition.before(), transition.after()))
        },
        discontinuity: |expected, actual| MutationError::DiscontinuousContent { expected, actual },
    };
    assert_eq!(
        [data.preceded_cases(), joins.preceded_cases(), continuations.preceded_cases()],
        [4 * 4 * 13, 4 * 4 * 13, 3 * 3 * 7],
        "every pair of a current and a base value must be checked against no journal transition \
         and against every journal transition of the namespace"
    );

    let one = EndpointValue::Multiplicity(NonZeroU64::MIN);
    let content = EndpointValue::Content(synthetic_ref(1));
    let misfits = [
        (SemanticNamespace::DataBag, &data.path, [
            EndpointValue::ValueFree,
            EndpointValue::Unit,
            content,
        ]),
        (SemanticNamespace::ContinuationMap, &continuations.path, [
            EndpointValue::ValueFree,
            EndpointValue::Unit,
            one,
        ]),
        (SemanticNamespace::JoinBag, &joins.path, [
            EndpointValue::ValueFree,
            EndpointValue::Unit,
            content,
        ]),
    ];
    for (namespace, path, endpoints) in misfits {
        for misfit in endpoints {
            for (before, after) in
                [(misfit, EndpointValue::Absent), (EndpointValue::Absent, misfit)]
            {
                let mut batch = empty_batch();
                let refused = batch.precede_endpoint(namespace, path, before, after);
                let following = batch.precede_endpoint(
                    namespace,
                    path,
                    EndpointValue::Absent,
                    EndpointValue::Absent,
                );
                assert!(
                    matches!(
                        &refused,
                        Err(MutationError::Canonical(CanonicalFormatError::EndpointPolicy {
                            namespace: refused_namespace,
                            endpoint,
                        })) if *refused_namespace == namespace.name() && *endpoint == misfit.name()
                    ) && following == Err(MutationError::PoisonedBatch) &&
                        batch.is_empty(),
                    "an endpoint of a kind that {namespace:?} cannot hold must be refused by the \
                     endpoint policy, record nothing and poison the batch: {refused:?} \
                     {following:?}"
                );
            }
        }
    }
    let mut batch = empty_batch();
    let unkeyed =
        batch.precede_endpoint(SemanticNamespace::DataBag, &[], one, EndpointValue::Absent);
    assert!(
        matches!(
            &unkeyed,
            Err(MutationError::InvalidKey {
                namespace: SemanticNamespace::DataBag,
                ..
            })
        ) && batch.is_empty(),
        "an endpoint whose path is not a key of its namespace must be refused: {unkeyed:?}"
    );
}
