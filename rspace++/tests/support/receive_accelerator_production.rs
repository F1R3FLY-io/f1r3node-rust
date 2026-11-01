use std::collections::{BTreeSet, HashMap};

use rspace_plus_plus::rspace::hashing::stable_hash_provider::StableHashSerialize;
use rspace_plus_plus::rspace::history::history_reader::HistoryReaderBase;
use rspace_plus_plus::rspace::hot_store::{HotStore, HotStoreInstances, HotStoreState};
use rspace_plus_plus::rspace::internal::{Datum, WaitingContinuation};
use rspace_plus_plus::rspace::receive_accelerator::{
    ReceiveAcceleratorClass, ReceiveAcceleratorHandle, ReceiveAnswer, ReceiveId,
    ReceiveKind as ProductionKind, ReceiveOccurrences, ReceiveTarget as ProductionTarget,
};
use rspace_plus_plus::rspace::rho_path::RhoPathEncode;
use rspace_plus_plus::rspace::serializers::cold_store_decode::{
    ColdStoreDecode, ColdStoreDecodeError,
};
use serde::Serialize;

use crate::reference::{
    AcceleratorClass, AcceleratorUnderTest, Answer, Fault, GroupKey, MaintenanceOp, Occurrence,
    ReceiveKind, ReceiveTarget, ReferenceWorld, Revision, StoreRevision, Token, group_key,
};

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize)]
pub struct RawChannel(pub Vec<u8>);

impl StableHashSerialize for RawChannel {}

impl RhoPathEncode for RawChannel {
    fn rho_path_structure(&self) -> Vec<u8> { self.0.clone() }

    fn rho_path_identity(&self) -> Vec<u8> { self.0.clone() }
}

impl ColdStoreDecode for RawChannel {
    fn cold_decode_prefix(bytes: &[u8]) -> Result<(Self, usize), ColdStoreDecodeError> {
        Ok((RawChannel(bytes.to_vec()), bytes.len()))
    }
}

pub type StringContinuation = WaitingContinuation<String, String>;
pub type RawStore = Box<dyn HotStore<RawChannel, String, String, String>>;
pub type RawState = HotStoreState<RawChannel, String, String, String>;
pub type RawOccurrences = ReceiveOccurrences<RawChannel, String, String>;
pub type RawTarget = ProductionTarget<RawChannel, String, String>;
pub type RawId = ReceiveId<RawChannel, String, String>;
type ProductionHandle = ReceiveAcceleratorHandle<RawChannel, String, String>;

struct EmptyHistory;

impl HistoryReaderBase<RawChannel, String, String, String> for EmptyHistory {
    fn get_data_canonical(&self, _channel: &RawChannel) -> Vec<Datum<String>> { Vec::new() }

    fn get_continuations_canonical(&self, _channels: &[RawChannel]) -> Vec<StringContinuation> {
        Vec::new()
    }

    fn get_joins_canonical(&self, _channel: &RawChannel) -> Vec<Vec<RawChannel>> { Vec::new() }
}

#[derive(Default)]
pub struct StubHistory {
    continuations: HashMap<Vec<RawChannel>, Vec<StringContinuation>>,
    joins: HashMap<RawChannel, Vec<Vec<RawChannel>>>,
}

impl StubHistory {
    pub fn put_continuations(
        &mut self,
        channels: Vec<RawChannel>,
        continuations: Vec<StringContinuation>,
    ) {
        self.continuations.insert(channels, continuations);
    }

    pub fn put_join(&mut self, channel: RawChannel, join: Vec<RawChannel>) {
        self.joins.entry(channel).or_default().push(join);
    }
}

impl HistoryReaderBase<RawChannel, String, String, String> for StubHistory {
    fn get_data_canonical(&self, _channel: &RawChannel) -> Vec<Datum<String>> { Vec::new() }

    fn get_continuations_canonical(&self, channels: &[RawChannel]) -> Vec<StringContinuation> {
        self.continuations
            .get(channels)
            .cloned()
            .unwrap_or_default()
    }

    fn get_joins_canonical(&self, channel: &RawChannel) -> Vec<Vec<RawChannel>> {
        self.joins.get(channel).cloned().unwrap_or_default()
    }
}

pub fn raw_hot_store() -> RawStore { raw_hot_store_from(RawState::default()) }

pub fn raw_hot_store_from(state: RawState) -> RawStore {
    HotStoreInstances::create_from_hs_and_hr(state, Box::new(EmptyHistory))
}

pub fn raw_hot_store_with_history(state: RawState, history: StubHistory) -> RawStore {
    HotStoreInstances::create_from_hs_and_hr(state, Box::new(history))
}

#[derive(Clone, Copy, Debug)]
pub enum Build {
    Published,
    Detached,
}

struct Pinned {
    handle: ProductionHandle,
    world: ReferenceWorld,
}

pub struct ProductionAccelerator<'a> {
    store: &'a dyn HotStore<RawChannel, String, String, String>,
    build: Build,
    pinned: Option<Pinned>,
}

impl<'a> ProductionAccelerator<'a> {
    pub fn new(store: &'a dyn HotStore<RawChannel, String, String, String>, build: Build) -> Self {
        Self {
            store,
            build,
            pinned: None,
        }
    }

    fn pinned_at(&self, world: &ReferenceWorld) -> Option<Pinned> {
        let handle = match self.build {
            Build::Published => self.store.receive_accelerator(),
            Build::Detached => self.store.detached_receive_accelerator(),
        };
        handle.map(|handle| Pinned {
            handle,
            world: world.clone(),
        })
    }

    pub fn occurrences(&self, channel: &RawChannel) -> ReceiveAnswer<RawChannel, String, String> {
        self.store.get_joins(channel);
        let held = self.pinned.as_ref().map(|pinned| &pinned.handle);
        self.store.receive_occurrences(held, channel)
    }
}

impl AcceleratorUnderTest for ProductionAccelerator<'_> {
    fn rebuild(&mut self, world: &ReferenceWorld) {
        if let Build::Published = self.build {
            self.store.rebuild_receive_accelerator();
        }
        self.pinned = self.pinned_at(world);
    }

    fn apply(
        &mut self,
        _operation: &MaintenanceOp,
        before: &ReferenceWorld,
        after: &ReferenceWorld,
    ) -> Result<(), Fault> {
        match &self.pinned {
            Some(pinned) if pinned.world == *before => {
                self.pinned = self.pinned_at(after);
                match self.pinned {
                    Some(_) => Ok(()),
                    None => Err(Fault::Unmaintainable(AcceleratorClass::Absent)),
                }
            }
            Some(pinned) => Err(Fault::Unmaintainable(unmaintainable_class(
                &pinned.world.revision,
                &before.revision,
            ))),
            None => Err(Fault::Unmaintainable(AcceleratorClass::Absent)),
        }
    }

    fn answer(&self, _world: &ReferenceWorld, channel: &[u8]) -> Answer {
        let channel = RawChannel(channel.to_vec());
        match self.occurrences(&channel) {
            ReceiveAnswer::Exact(occurrences) => {
                Answer::Exact(reference_occurrences(&channel, &occurrences))
            }
            ReceiveAnswer::Fallback(class) => Answer::Fallback(reference_class(class)),
        }
    }

    fn artifact_revisions(&self) -> Vec<Revision> {
        match &self.pinned {
            Some(pinned) => {
                match self.store.receive_revision().as_ref() == Some(pinned.handle.revision()) {
                    true => vec![pinned.world.revision.clone()],
                    false => vec![sentinel_revision()],
                }
            }
            None => Vec::new(),
        }
    }
}

fn unmaintainable_class(pinned: &Revision, current: &Revision) -> AcceleratorClass {
    match pinned.generation == current.generation {
        true => AcceleratorClass::Stale,
        false => AcceleratorClass::Foreign,
    }
}

fn sentinel_revision() -> Revision {
    Revision {
        generation: Token::fresh(),
        store: StoreRevision::default(),
        installed: Token::fresh(),
    }
}

pub fn ordered_targets(occurrences: &RawOccurrences) -> Vec<(GroupKey, Vec<ReceiveTarget>)> {
    let mut ordered = Vec::with_capacity(occurrences.groups().len());
    for group in occurrences.groups() {
        let key = group_key(group.channels());
        let mut targets = Vec::with_capacity(group.targets().len());
        targets.extend(
            group
                .targets()
                .iter()
                .map(|target| reference_target(&key, target)),
        );
        ordered.push((key, targets));
    }
    ordered
}

fn reference_occurrences(
    channel: &RawChannel,
    occurrences: &RawOccurrences,
) -> BTreeSet<Occurrence> {
    let image = channel.rho_path_identity();
    let mut mapped = BTreeSet::new();
    for group in occurrences.groups() {
        let key = group_key(group.channels());
        let mut positions = Vec::with_capacity(group.positions().len());
        positions.extend(group.positions().iter().map(|position| position.get()));
        for target in group.targets() {
            let occurrence = Occurrence {
                channel: image.clone(),
                group: key.clone(),
                positions: positions.clone(),
                target: reference_target(&key, target),
            };
            assert!(mapped.insert(occurrence), "a production answer names each occurrence once");
        }
    }
    mapped
}

pub fn reference_target(group: &GroupKey, target: &RawTarget) -> ReceiveTarget {
    ReceiveTarget {
        kind: reference_kind(target.kind()),
        group: group.clone(),
        identity: target.identity_bytes().into_owned(),
        source: target.source_bytes().into_owned(),
    }
}

pub fn reference_kind(kind: ProductionKind) -> ReceiveKind {
    match kind {
        ProductionKind::Installed => ReceiveKind::Installed,
        ProductionKind::Resting => ReceiveKind::Resting,
    }
}

fn reference_class(class: ReceiveAcceleratorClass) -> AcceleratorClass {
    match class {
        ReceiveAcceleratorClass::Absent => AcceleratorClass::Absent,
        ReceiveAcceleratorClass::Building => AcceleratorClass::Building,
        ReceiveAcceleratorClass::Foreign => AcceleratorClass::Foreign,
        ReceiveAcceleratorClass::Stale => AcceleratorClass::Stale,
        ReceiveAcceleratorClass::Incomplete => AcceleratorClass::Incomplete,
        ReceiveAcceleratorClass::Coherent => AcceleratorClass::Coherent,
    }
}
