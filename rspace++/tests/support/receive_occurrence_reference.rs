use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};
use std::hash::Hash;
use std::sync::Arc;

use rspace_plus_plus::rspace::hashing::stable_hash_provider::StableHashSerialize;
use rspace_plus_plus::rspace::hot_store::{ContinuationVersion, HotStore};
use rspace_plus_plus::rspace::internal::WaitingContinuation;
use rspace_plus_plus::rspace::ordered_receive::{
    ContinuationRecordView, OrderedReceiveView, PreparedContinuationRecord, RestingReceiveVisit,
};
use rspace_plus_plus::rspace::rho_path::{ReceiveChannelQuery, RhoPathEncode};

use self::adoption::{Adoption, persisted_accelerator_adoption};

#[path = "receive_accelerator_adoption.rs"]
pub mod adoption;

pub type ChannelImage = Vec<u8>;
pub type GroupKey = Vec<ChannelImage>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ReceiveKind {
    Installed,
    Resting,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ReceiveTarget {
    pub kind: ReceiveKind,
    pub group: GroupKey,
    pub identity: Vec<u8>,
    pub source: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Occurrence {
    pub channel: ChannelImage,
    pub group: GroupKey,
    pub positions: Vec<u64>,
    pub target: ReceiveTarget,
}

#[derive(Clone, Debug)]
pub struct Token(Arc<()>);

impl Token {
    pub fn fresh() -> Self { Self(Arc::new(())) }
}

impl PartialEq for Token {
    fn eq(&self, other: &Self) -> bool { Arc::ptr_eq(&self.0, &other.0) }
}

impl Eq for Token {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GroupVersion {
    Loaded(ContinuationVersion),
    Unloaded,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StoreRevision(pub BTreeMap<GroupKey, GroupVersion>);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Revision {
    pub generation: Token,
    pub store: StoreRevision,
    pub installed: Token,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReceiveId {
    pub revision: Revision,
    pub target: ReceiveTarget,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RestingRecord {
    pub order: u64,
    pub target: ReceiveTarget,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Observation {
    pub store: StoreRevision,
    pub installed: BTreeMap<GroupKey, ReceiveTarget>,
    pub resting: BTreeMap<GroupKey, Vec<RestingRecord>>,
    pub unloaded: BTreeSet<GroupKey>,
}

impl Observation {
    pub fn is_complete(&self) -> bool { self.unloaded.is_empty() }

    pub fn is_loaded(&self, group: &GroupKey) -> bool {
        matches!(self.store.0.get(group), Some(GroupVersion::Loaded(_)))
    }

    pub fn version(&self, group: &GroupKey) -> Option<&ContinuationVersion> {
        match self.store.0.get(group) {
            Some(GroupVersion::Loaded(version)) => Some(version),
            Some(GroupVersion::Unloaded) | None => None,
        }
    }

    pub fn targets(&self, group: &GroupKey) -> BTreeSet<ReceiveTarget> {
        let mut targets = BTreeSet::new();
        for record in self
            .resting
            .get(group)
            .map(Vec::as_slice)
            .unwrap_or_default()
        {
            targets.insert(record.target.clone());
        }
        targets.extend(self.installed.get(group).cloned());
        targets
    }

    pub fn ordered_targets(&self, group: &GroupKey) -> Vec<ReceiveTarget> {
        let resting = self
            .resting
            .get(group)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let installed = self.installed.get(group);
        let capacity = resting
            .len()
            .checked_add(usize::from(installed.is_some()))
            .expect("an ordered group length fits usize");
        let mut by_order: Vec<&RestingRecord> = resting.iter().collect();
        by_order.sort_by_key(|record| Reverse(record.order));
        let mut ordered = Vec::with_capacity(capacity);
        ordered.extend(installed.cloned());
        ordered.extend(by_order.into_iter().map(|record| record.target.clone()));
        ordered
    }

    pub fn support(&self) -> BTreeSet<GroupKey> {
        let mut support: BTreeSet<GroupKey> = self.installed.keys().cloned().collect();
        support.extend(
            self.resting
                .iter()
                .filter(|(_, records)| !records.is_empty())
                .map(|(group, _)| group.clone()),
        );
        support
    }

    pub fn contains_target(&self, target: &ReceiveTarget) -> bool {
        self.targets(&target.group).contains(target)
    }

    pub fn channel_images(&self) -> BTreeSet<ChannelImage> {
        self.store
            .0
            .keys()
            .flat_map(|group| group.iter().cloned())
            .collect()
    }
}

pub fn channel_image<C: RhoPathEncode>(channel: &C) -> ChannelImage { channel.rho_path_identity() }

pub fn group_key<C: RhoPathEncode>(channels: &[C]) -> GroupKey {
    channels
        .iter()
        .map(|channel| channel.rho_path_identity())
        .collect()
}

pub fn positions_in(group: &[ChannelImage], channel: &[u8]) -> Vec<u64> {
    group
        .iter()
        .enumerate()
        .filter(|(_, image)| image.as_slice() == channel)
        .map(|(index, _)| u64::try_from(index).expect("a receive position fits u64"))
        .collect()
}

pub fn capacity(group: &[ChannelImage], channel: &[u8]) -> usize {
    positions_in(group, channel).len()
}

pub fn distinct_channels(group: &[ChannelImage]) -> BTreeSet<ChannelImage> {
    group.iter().cloned().collect()
}

fn target_of(
    kind: ReceiveKind,
    group: &[ChannelImage],
    record: ContinuationRecordView<'_>,
) -> ReceiveTarget {
    ReceiveTarget {
        kind,
        group: group.to_vec(),
        identity: record.identity().to_vec(),
        source: record.source_suffix().to_vec(),
    }
}

pub fn target_of_continuation<P, K>(
    kind: ReceiveKind,
    group: &[ChannelImage],
    continuation: &WaitingContinuation<P, K>,
) -> ReceiveTarget
where
    P: Clone + StableHashSerialize,
    K: Clone + StableHashSerialize,
{
    let prepared = PreparedContinuationRecord::encode_waiting(continuation);
    target_of(kind, group, prepared.record_view())
}

pub fn occurrences_of(target: &ReceiveTarget) -> Vec<Occurrence> {
    let channels = distinct_channels(&target.group);
    let mut occurrences = Vec::with_capacity(channels.len());
    for channel in channels {
        let positions = positions_in(&target.group, &channel);
        occurrences.push(Occurrence {
            channel,
            group: target.group.clone(),
            positions,
            target: target.clone(),
        });
    }
    occurrences
}

fn check_view_positions(group: &[ChannelImage], view: &OrderedReceiveView<'_>) {
    for channel in distinct_channels(group) {
        let observed: Vec<u64> = view
            .positions(ReceiveChannelQuery::LegacyV1(&channel))
            .expect("a legacy continuation view answers a legacy channel query")
            .map(|position| position.get())
            .collect();
        assert_eq!(
            observed,
            positions_in(group, &channel),
            "the production view and the reference disagree on the positions of a channel"
        );
    }
}

pub fn observe<C, P, A, K>(
    store: &dyn HotStore<C, P, A, K>,
    universe: &[Vec<C>],
    hydrate: bool,
) -> Observation
where
    C: Clone + Eq + Hash + RhoPathEncode,
    P: Clone + StableHashSerialize,
    A: Clone,
    K: Clone + StableHashSerialize,
{
    let snapshot = store.snapshot();
    let mut domain: BTreeMap<GroupKey, Vec<C>> = BTreeMap::new();
    for channels in snapshot
        .continuations
        .keys()
        .chain(snapshot.installed_continuations.keys())
        .chain(universe.iter())
    {
        domain
            .entry(group_key(channels))
            .or_insert_with(|| channels.clone());
    }
    let mut observation = Observation::default();
    for (key, channels) in &domain {
        if hydrate {
            let _ = store.get_continuations_versioned(channels);
        }
        let mut records: Vec<RestingRecord> = Vec::new();
        let mut visited = 0usize;
        let visit = store.try_visit_resting_receive_views(channels, &mut |orders, admission| {
            let view = admission.expect("a stored continuation record admits its legacy view");
            check_view_positions(key, &view);
            let target = target_of(ReceiveKind::Resting, key, view.record());
            let order = match orders {
                [order] => *order,
                _ => panic!("a continuation record carries exactly one order"),
            };
            records.push(RestingRecord { order, target });
            visited = visited
                .checked_add(1)
                .expect("a visited record count fits usize");
        });
        match visit {
            RestingReceiveVisit::Visited(count) => {
                assert_eq!(count, visited, "the resting visit reports every record it visited");
                let (version, _) = store.get_continuations_versioned(channels);
                let version = version.expect("an in-memory hot store versions every loaded group");
                observation
                    .store
                    .0
                    .insert(key.clone(), GroupVersion::Loaded(version));
                observation.resting.insert(key.clone(), records);
            }
            RestingReceiveVisit::Unloaded => {
                observation
                    .store
                    .0
                    .insert(key.clone(), GroupVersion::Unloaded);
                observation.unloaded.insert(key.clone());
            }
        }
        if let Some(continuation) = snapshot.installed_continuations.get(channels) {
            let installed_target =
                target_of_continuation(ReceiveKind::Installed, key, continuation);
            observation.installed.insert(key.clone(), installed_target);
        }
    }
    observation
}

pub fn channel_groups(observation: &Observation, channel: &[u8]) -> BTreeSet<(GroupKey, Vec<u64>)> {
    observation
        .support()
        .into_iter()
        .filter_map(|group| {
            let positions = positions_in(&group, channel);
            match positions.is_empty() {
                true => None,
                false => Some((group, positions)),
            }
        })
        .collect()
}

pub fn derive(observation: &Observation, channel: &[u8]) -> BTreeSet<Occurrence> {
    let mut occurrences = BTreeSet::new();
    for (group, positions) in channel_groups(observation, channel) {
        for target in observation.targets(&group) {
            occurrences.insert(Occurrence {
                channel: channel.to_vec(),
                group: group.clone(),
                positions: positions.clone(),
                target,
            });
        }
    }
    occurrences
}

pub fn derive_all(observation: &Observation) -> BTreeMap<ChannelImage, BTreeSet<Occurrence>> {
    let mut relation: BTreeMap<ChannelImage, BTreeSet<Occurrence>> = BTreeMap::new();
    for group in observation.support() {
        let targets = observation.targets(&group);
        for channel in distinct_channels(&group) {
            let positions = positions_in(&group, &channel);
            let entry = relation.entry(channel.clone()).or_default();
            for target in &targets {
                entry.insert(Occurrence {
                    channel: channel.clone(),
                    group: group.clone(),
                    positions: positions.clone(),
                    target: target.clone(),
                });
            }
        }
    }
    relation
}

#[derive(Clone, Debug)]
pub struct Lineage {
    generation: Token,
    installed: Token,
    manifest: Vec<u8>,
}

impl Lineage {
    pub fn new(manifest: Vec<u8>) -> Self {
        Self {
            generation: Token::fresh(),
            installed: Token::fresh(),
            manifest,
        }
    }

    pub fn regenerate(&mut self) {
        self.generation = Token::fresh();
        self.installed = Token::fresh();
    }

    pub fn retarget(&mut self, manifest: Vec<u8>) {
        self.regenerate();
        self.manifest = manifest;
    }

    pub fn reinstall(&mut self) { self.installed = Token::fresh(); }

    pub fn manifest(&self) -> &[u8] { &self.manifest }

    pub fn world(&self, observation: Observation) -> ReferenceWorld {
        let revision = Revision {
            generation: self.generation.clone(),
            store: observation.store.clone(),
            installed: self.installed.clone(),
        };
        ReferenceWorld {
            observation,
            revision,
            manifest: self.manifest.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReferenceWorld {
    pub observation: Observation,
    pub revision: Revision,
    pub manifest: Vec<u8>,
}

pub fn mint_ids(revision: &Revision, occurrences: &BTreeSet<Occurrence>) -> Vec<ReceiveId> {
    let mut ids = Vec::with_capacity(occurrences.len());
    ids.extend(occurrences.iter().map(|occurrence| ReceiveId {
        revision: revision.clone(),
        target: occurrence.target.clone(),
    }));
    ids
}

pub fn resolve(world: &ReferenceWorld, id: &ReceiveId) -> Option<ReceiveTarget> {
    match id.revision == world.revision {
        true => world
            .observation
            .targets(&id.target.group)
            .into_iter()
            .find(|candidate| candidate == &id.target),
        false => None,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CommitOutcome {
    Committed(ReceiveTarget),
    Stale,
}

pub fn commit_candidate(world: &ReferenceWorld, id: &ReceiveId) -> CommitOutcome {
    let current = id.revision == world.revision;
    let present = world.observation.contains_target(&id.target);
    match (current, present) {
        (true, true) => CommitOutcome::Committed(id.target.clone()),
        (true, false) | (false, true) | (false, false) => CommitOutcome::Stale,
    }
}

pub struct BucketedTargets {
    hash: fn(&ReceiveTarget) -> u64,
    buckets: BTreeMap<u64, Vec<ReceiveTarget>>,
}

impl BucketedTargets {
    pub fn new(
        hash: fn(&ReceiveTarget) -> u64,
        targets: impl IntoIterator<Item = ReceiveTarget>,
    ) -> Self {
        let mut buckets: BTreeMap<u64, Vec<ReceiveTarget>> = BTreeMap::new();
        for target in targets {
            buckets.entry(hash(&target)).or_default().push(target);
        }
        Self { hash, buckets }
    }

    pub fn bucket_len(&self, target: &ReceiveTarget) -> usize {
        self.buckets.get(&(self.hash)(target)).map_or(0, Vec::len)
    }

    pub fn resolve(&self, wanted: &ReceiveTarget) -> Option<&ReceiveTarget> {
        let bucket = self.buckets.get(&(self.hash)(wanted))?;
        bucket.iter().find(|candidate| *candidate == wanted)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AcceleratorClass {
    Absent,
    Building,
    Foreign,
    Stale,
    Incomplete,
    Coherent,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Answer {
    Exact(BTreeSet<Occurrence>),
    Fallback(AcceleratorClass),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaultStage {
    Catalog,
    Occurrence,
    Automaton,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fault {
    Injected(FaultStage),
    Unmaintainable(AcceleratorClass),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MaintenanceOp {
    PutResting(ReceiveTarget),
    RemoveResting(ReceiveTarget),
    Install(ReceiveTarget),
    LoadHistory {
        group: GroupKey,
        targets: Vec<ReceiveTarget>,
    },
}

impl MaintenanceOp {
    pub fn group(&self) -> &GroupKey {
        match self {
            MaintenanceOp::PutResting(target) |
            MaintenanceOp::RemoveResting(target) |
            MaintenanceOp::Install(target) => &target.group,
            MaintenanceOp::LoadHistory { group, .. } => group,
        }
    }
}

pub trait AcceleratorUnderTest {
    fn rebuild(&mut self, world: &ReferenceWorld);

    fn apply(
        &mut self,
        operation: &MaintenanceOp,
        before: &ReferenceWorld,
        after: &ReferenceWorld,
    ) -> Result<(), Fault>;

    fn answer(&self, world: &ReferenceWorld, channel: &[u8]) -> Answer;

    fn artifact_revisions(&self) -> Vec<Revision>;
}

pub fn read<X: AcceleratorUnderTest + ?Sized>(
    accelerator: &X,
    world: &ReferenceWorld,
    complete: &Observation,
    channel: &[u8],
) -> BTreeSet<Occurrence> {
    assert!(complete.is_complete(), "a fallback derivation reads a fully hydrated observation");
    match accelerator.answer(world, channel) {
        Answer::Exact(occurrences) => occurrences,
        Answer::Fallback(_) => derive(complete, channel),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Published<T> {
    pub revision: Revision,
    pub value: T,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuiltAccelerator {
    pub manifest: Vec<u8>,
    pub revision: Revision,
    pub scope: BTreeSet<GroupKey>,
    pub catalog: Published<BTreeMap<GroupKey, BTreeSet<ReceiveTarget>>>,
    pub occurrences: Published<BTreeMap<ChannelImage, BTreeSet<Occurrence>>>,
    pub automaton: Published<BTreeMap<GroupKey, Vec<ReceiveTarget>>>,
}

fn compile_automaton(
    catalog: &BTreeMap<GroupKey, BTreeSet<ReceiveTarget>>,
) -> BTreeMap<GroupKey, Vec<ReceiveTarget>> {
    catalog
        .iter()
        .map(|(group, targets)| (group.clone(), targets.iter().cloned().collect()))
        .collect()
}

fn insert_occurrences(
    occurrences: &mut BTreeMap<ChannelImage, BTreeSet<Occurrence>>,
    target: &ReceiveTarget,
) {
    for occurrence in occurrences_of(target) {
        occurrences
            .entry(occurrence.channel.clone())
            .or_default()
            .insert(occurrence);
    }
}

fn remove_occurrences(
    occurrences: &mut BTreeMap<ChannelImage, BTreeSet<Occurrence>>,
    target: &ReceiveTarget,
) {
    for occurrence in occurrences_of(target) {
        let emptied = match occurrences.get_mut(&occurrence.channel) {
            Some(held) => {
                held.remove(&occurrence);
                held.is_empty()
            }
            None => false,
        };
        if emptied {
            occurrences.remove(&occurrence.channel);
        }
    }
}

fn remove_from_catalog(
    catalog: &mut BTreeMap<GroupKey, BTreeSet<ReceiveTarget>>,
    target: &ReceiveTarget,
) {
    let emptied = match catalog.get_mut(&target.group) {
        Some(held) => {
            held.remove(target);
            held.is_empty()
        }
        None => false,
    };
    if emptied {
        catalog.remove(&target.group);
    }
}

impl BuiltAccelerator {
    pub fn build(world: &ReferenceWorld) -> Self {
        let observation = &world.observation;
        let scope = observation
            .store
            .0
            .iter()
            .filter_map(|(group, version)| match version {
                GroupVersion::Loaded(_) => Some(group.clone()),
                GroupVersion::Unloaded => None,
            })
            .collect();
        let mut catalog = BTreeMap::new();
        for group in observation.support() {
            let targets = observation.targets(&group);
            catalog.insert(group, targets);
        }
        let mut occurrences = BTreeMap::new();
        for targets in catalog.values() {
            for target in targets {
                insert_occurrences(&mut occurrences, target);
            }
        }
        let automaton = compile_automaton(&catalog);
        Self {
            manifest: world.manifest.clone(),
            revision: world.revision.clone(),
            scope,
            catalog: Published {
                revision: world.revision.clone(),
                value: catalog,
            },
            occurrences: Published {
                revision: world.revision.clone(),
                value: occurrences,
            },
            automaton: Published {
                revision: world.revision.clone(),
                value: automaton,
            },
        }
    }

    pub fn lookup(&self, channel: &[u8]) -> BTreeSet<Occurrence> {
        self.occurrences
            .value
            .get(channel)
            .cloned()
            .unwrap_or_default()
    }

    fn representative(&self, candidate: &ReceiveTarget) -> Option<ReceiveTarget> {
        self.catalog
            .value
            .get(&candidate.group)?
            .iter()
            .find(|held| held.kind == candidate.kind && held.identity == candidate.identity)
            .cloned()
    }

    fn holds(&self, target: &ReceiveTarget) -> bool {
        self.catalog
            .value
            .get(&target.group)
            .is_some_and(|held| held.contains(target))
    }

    fn installed_targets(&self, group: &GroupKey) -> Vec<ReceiveTarget> {
        self.catalog
            .value
            .get(group)
            .map(|held| {
                held.iter()
                    .filter(|target| target.kind == ReceiveKind::Installed)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    fn delta(&self, operation: &MaintenanceOp) -> (Vec<ReceiveTarget>, Vec<ReceiveTarget>) {
        match operation {
            MaintenanceOp::PutResting(target) => match self.representative(target) {
                Some(_) => (Vec::new(), Vec::new()),
                None => (Vec::new(), vec![target.clone()]),
            },
            MaintenanceOp::RemoveResting(target) => match self.holds(target) {
                true => (vec![target.clone()], Vec::new()),
                false => (Vec::new(), Vec::new()),
            },
            MaintenanceOp::Install(target) => {
                (self.installed_targets(&target.group), vec![target.clone()])
            }
            MaintenanceOp::LoadHistory { targets, .. } => (Vec::new(), targets.clone()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AcceleratorPhase {
    Absent,
    Building,
    Built(Box<BuiltAccelerator>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PersistedAccelerator {
    pub bound_manifest: Vec<u8>,
    pub scope: BTreeSet<GroupKey>,
    pub catalog: BTreeMap<GroupKey, BTreeSet<ReceiveTarget>>,
    pub occurrences: BTreeMap<ChannelImage, BTreeSet<Occurrence>>,
    pub automaton: BTreeMap<GroupKey, Vec<ReceiveTarget>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReferenceAccelerator {
    phase: AcceleratorPhase,
    armed_fault: Option<FaultStage>,
}

impl Default for ReferenceAccelerator {
    fn default() -> Self { Self::new() }
}

impl ReferenceAccelerator {
    pub fn new() -> Self {
        Self {
            phase: AcceleratorPhase::Absent,
            armed_fault: None,
        }
    }

    pub fn phase(&self) -> &AcceleratorPhase { &self.phase }

    pub fn built(&self) -> Option<&BuiltAccelerator> {
        match &self.phase {
            AcceleratorPhase::Built(built) => Some(built),
            AcceleratorPhase::Absent | AcceleratorPhase::Building => None,
        }
    }

    pub fn begin_build(&mut self) { self.phase = AcceleratorPhase::Building; }

    pub fn arm_fault(&mut self, stage: FaultStage) { self.armed_fault = Some(stage); }

    fn check_fault(&mut self, stage: FaultStage) -> Result<(), Fault> {
        match self.armed_fault {
            Some(armed) if armed == stage => {
                self.armed_fault = None;
                Err(Fault::Injected(stage))
            }
            Some(_) | None => Ok(()),
        }
    }

    pub fn classify(&self, world: &ReferenceWorld) -> AcceleratorClass {
        let built = match &self.phase {
            AcceleratorPhase::Absent => return AcceleratorClass::Absent,
            AcceleratorPhase::Building => return AcceleratorClass::Building,
            AcceleratorPhase::Built(built) => built,
        };
        let foreign_generation = built.revision.generation != world.revision.generation;
        let foreign_manifest = built.manifest != world.manifest;
        let stale_store = built.revision.store != world.revision.store;
        let stale_installed = built.revision.installed != world.revision.installed;
        let uncovered = world.revision.store.0.iter().any(|(group, version)| {
            matches!(version, GroupVersion::Unloaded) || !built.scope.contains(group)
        });
        match (foreign_generation || foreign_manifest, stale_store || stale_installed, uncovered) {
            (true, _, _) => AcceleratorClass::Foreign,
            (false, true, _) => AcceleratorClass::Stale,
            (false, false, true) => AcceleratorClass::Incomplete,
            (false, false, false) => AcceleratorClass::Coherent,
        }
    }

    pub fn lookup(&self, channel: &[u8]) -> BTreeSet<Occurrence> {
        match &self.phase {
            AcceleratorPhase::Built(built) => built.lookup(channel),
            AcceleratorPhase::Absent | AcceleratorPhase::Building => BTreeSet::new(),
        }
    }

    pub fn persist(&self) -> Option<PersistedAccelerator> {
        self.built().map(|built| PersistedAccelerator {
            bound_manifest: built.manifest.clone(),
            scope: built.scope.clone(),
            catalog: built.catalog.value.clone(),
            occurrences: built.occurrences.value.clone(),
            automaton: built.automaton.value.clone(),
        })
    }

    pub fn adopt_or_rebuild(
        &mut self,
        persisted: &PersistedAccelerator,
        world: &ReferenceWorld,
    ) -> Adoption {
        let adoption = persisted_accelerator_adoption(&persisted.bound_manifest, &world.manifest);
        let built = match adoption {
            Adoption::Adopted => BuiltAccelerator {
                manifest: world.manifest.clone(),
                revision: world.revision.clone(),
                scope: persisted.scope.clone(),
                catalog: Published {
                    revision: world.revision.clone(),
                    value: persisted.catalog.clone(),
                },
                occurrences: Published {
                    revision: world.revision.clone(),
                    value: persisted.occurrences.clone(),
                },
                automaton: Published {
                    revision: world.revision.clone(),
                    value: persisted.automaton.clone(),
                },
            },
            Adoption::Foreign => BuiltAccelerator::build(world),
        };
        self.phase = AcceleratorPhase::Built(Box::new(built));
        adoption
    }

    fn maintain(
        &mut self,
        operation: &MaintenanceOp,
        before: &ReferenceWorld,
        after: &ReferenceWorld,
    ) -> Result<(), Fault> {
        let current = match &self.phase {
            AcceleratorPhase::Built(built)
                if built.revision == before.revision && built.manifest == before.manifest =>
            {
                built.clone()
            }
            AcceleratorPhase::Built(_) | AcceleratorPhase::Absent | AcceleratorPhase::Building => {
                return Err(Fault::Unmaintainable(self.classify(before)));
            }
        };
        let (removed, added) = current.delta(operation);
        let mut next = current;
        for target in &removed {
            remove_from_catalog(&mut next.catalog.value, target);
        }
        for target in &added {
            next.catalog
                .value
                .entry(target.group.clone())
                .or_default()
                .insert(target.clone());
        }
        next.catalog.revision = after.revision.clone();
        self.check_fault(FaultStage::Catalog)?;
        for target in &removed {
            remove_occurrences(&mut next.occurrences.value, target);
        }
        for target in &added {
            insert_occurrences(&mut next.occurrences.value, target);
        }
        next.occurrences.revision = after.revision.clone();
        self.check_fault(FaultStage::Occurrence)?;
        next.automaton.value = compile_automaton(&next.catalog.value);
        next.automaton.revision = after.revision.clone();
        self.check_fault(FaultStage::Automaton)?;
        next.scope.insert(operation.group().clone());
        next.revision = after.revision.clone();
        self.phase = AcceleratorPhase::Built(next);
        Ok(())
    }
}

impl AcceleratorUnderTest for ReferenceAccelerator {
    fn rebuild(&mut self, world: &ReferenceWorld) {
        self.phase = AcceleratorPhase::Built(Box::new(BuiltAccelerator::build(world)));
    }

    fn apply(
        &mut self,
        operation: &MaintenanceOp,
        before: &ReferenceWorld,
        after: &ReferenceWorld,
    ) -> Result<(), Fault> {
        self.maintain(operation, before, after)
    }

    fn answer(&self, world: &ReferenceWorld, channel: &[u8]) -> Answer {
        match self.classify(world) {
            AcceleratorClass::Coherent => Answer::Exact(self.lookup(channel)),
            AcceleratorClass::Absent => Answer::Fallback(AcceleratorClass::Absent),
            AcceleratorClass::Building => Answer::Fallback(AcceleratorClass::Building),
            AcceleratorClass::Foreign => Answer::Fallback(AcceleratorClass::Foreign),
            AcceleratorClass::Stale => Answer::Fallback(AcceleratorClass::Stale),
            AcceleratorClass::Incomplete => Answer::Fallback(AcceleratorClass::Incomplete),
        }
    }

    fn artifact_revisions(&self) -> Vec<Revision> {
        match &self.phase {
            AcceleratorPhase::Built(built) => vec![
                built.catalog.revision.clone(),
                built.occurrences.revision.clone(),
                built.automaton.revision.clone(),
            ],
            AcceleratorPhase::Absent | AcceleratorPhase::Building => Vec::new(),
        }
    }
}

#[derive(Clone, Debug)]
pub enum StoreStep<C, P: Clone, K: Clone> {
    Put {
        channels: Vec<C>,
        continuation: WaitingContinuation<P, K>,
    },
    Remove {
        channels: Vec<C>,
        index: i32,
    },
    Install {
        channels: Vec<C>,
        continuation: WaitingContinuation<P, K>,
    },
    Regenerate,
    Clear,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TraceReport {
    pub steps: usize,
    pub maintained: usize,
    pub regenerated: usize,
    pub refused: usize,
    pub probes: usize,
}

fn tally(count: usize) -> usize { count.checked_add(1).expect("a trace tally fits usize") }

pub fn check_trace<X, F, C, P, A, K>(
    make: F,
    store: &dyn HotStore<C, P, A, K>,
    universe: &[Vec<C>],
    probes: &[ChannelImage],
    steps: &[StoreStep<C, P, K>],
) -> TraceReport
where
    X: AcceleratorUnderTest,
    F: Fn() -> X,
    C: Clone + Eq + Hash + RhoPathEncode,
    P: Clone + StableHashSerialize,
    A: Clone,
    K: Clone + StableHashSerialize,
{
    let mut lineage = Lineage::new(Vec::new());
    let mut report = TraceReport::default();
    let mut maintained = make();
    maintained.rebuild(&lineage.world(observe(store, universe, true)));
    for step in steps {
        let before = lineage.world(observe(store, universe, true));
        let operation = match step {
            StoreStep::Put {
                channels,
                continuation,
            } => {
                let outcome = store.put_continuation(channels, continuation.clone());
                assert!(outcome.is_some(), "an in-memory put reports whether it inserted");
                Some(MaintenanceOp::PutResting(target_of_continuation(
                    ReceiveKind::Resting,
                    &group_key(channels),
                    continuation,
                )))
            }
            StoreStep::Remove { channels, index } => {
                let ordered = before.observation.ordered_targets(&group_key(channels));
                let named = usize::try_from(*index)
                    .ok()
                    .and_then(|position| ordered.get(position))
                    .cloned();
                match (store.remove_continuation(channels, *index), named) {
                    (Some(()), Some(target)) => Some(MaintenanceOp::RemoveResting(target)),
                    (Some(()), None) => {
                        panic!("production removed a continuation the reference does not order")
                    }
                    (None, _) => {
                        report.refused = tally(report.refused);
                        None
                    }
                }
            }
            StoreStep::Install {
                channels,
                continuation,
            } => {
                let outcome = store.install_continuation(channels, continuation.clone());
                assert_eq!(outcome, Some(()), "an in-memory install succeeds");
                lineage.reinstall();
                Some(MaintenanceOp::Install(target_of_continuation(
                    ReceiveKind::Installed,
                    &group_key(channels),
                    continuation,
                )))
            }
            StoreStep::Regenerate => {
                store.set_state(store.snapshot());
                lineage.regenerate();
                None
            }
            StoreStep::Clear => {
                store.clear();
                lineage.regenerate();
                None
            }
        };
        let after = lineage.world(observe(store, universe, true));
        match (operation, step) {
            (Some(operation), _) => {
                maintained
                    .apply(&operation, &before, &after)
                    .expect("a coherent accelerator maintains every successful update");
                report.maintained = tally(report.maintained);
            }
            (None, StoreStep::Regenerate | StoreStep::Clear) => {
                for probe in probes {
                    assert_eq!(
                        maintained.answer(&after, probe),
                        Answer::Fallback(AcceleratorClass::Foreign),
                        "a generation replacement makes the held accelerator foreign"
                    );
                }
                maintained.rebuild(&after);
                report.regenerated = tally(report.regenerated);
            }
            (None, _) => {
                assert_eq!(
                    after.observation, before.observation,
                    "a refused update leaves every version and target unchanged"
                );
            }
        }
        let mut rebuilt = make();
        rebuilt.rebuild(&after);
        let mut images = after.observation.channel_images();
        images.extend(probes.iter().cloned());
        for image in &images {
            let expected = Answer::Exact(derive(&after.observation, image));
            assert_eq!(
                maintained.answer(&after, image),
                expected,
                "the maintained accelerator answers the derivation"
            );
            assert_eq!(
                rebuilt.answer(&after, image),
                expected,
                "the rebuilt accelerator answers the derivation"
            );
            report.probes = tally(report.probes);
        }
        for revision in maintained.artifact_revisions() {
            assert_eq!(
                revision, after.revision,
                "every artifact is published at the current revision"
            );
        }
        report.steps = tally(report.steps);
    }
    report
}
