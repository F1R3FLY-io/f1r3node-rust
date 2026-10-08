//! The space agent of every family (P5 design v3, A.3.4).
//!
//! [`FamilyAgent`] is the [`SpaceAgent`] of the native spaces. A collection operation opens its
//! transaction over the channels of its collection and reads the config of its space once
//! (decision 42). The agent of the family that the config names then runs the operation in that
//! transaction ([`FamilySpace::run_with`]). The dispatch is static, as the agent factory of the
//! legacy branch was. A family that is not wired fails with `FamilyUnavailable`.
//!
//! [`Family`] is the one source of truth for the wired families. It gives the agent of each
//! family, its send params and its query params. The factory and the handle method `produce`
//! read the param schemas from it.
//!
//! The HashMap, Array and Vector stores key a collection by the canonical bytes of its channel
//! (A5), so the agent opens every collection with the channels of the HashMap store. A PathMap
//! operation opens the same first transaction to read the config. The PathMap store then runs it
//! in a new transaction over the lock keys of its prefix scope (`pathmap::join`).
//!
//! Each arm of the dispatch reserves the comparison of the config names with its family before
//! it compares them. So `hashmap:queue`, the first arm, reserves exactly the one comparison of
//! the P4 agent, and the host work of a queue operation does not change. The other arms follow
//! in ascending family index, then the PathMap arms of P5e, then the VectorDB arms of P6. So
//! the arm at dispatch position `i` reserves `i + 1` comparisons, and a later phase changes the
//! dispatch of no earlier family.

use async_trait::async_trait;
use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::Par;
use models::rust::space_factory_urn::{SpaceFactoryUrn, SpaceInner, SpaceOuter};

use super::agent::{
    AllocOp, Allocation, ConsumeOp, JoinOp, Prepared, ProduceOp, SettleOp, SpaceAgent,
    SpaceCreation, SpaceEnv, SpaceOp, SpaceOutcome,
};
use super::disciplines::bag::BagDiscipline;
use super::disciplines::cell::CellDiscipline;
use super::disciplines::priority_queue::PriorityQueueDiscipline;
use super::disciplines::queue::QueueDiscipline;
use super::disciplines::set::SetDiscipline;
use super::disciplines::stack::StackDiscipline;
use super::disciplines::vectordb::VectorDbDiscipline;
use super::errors::SpaceError;
use super::generic::{
    begin_allocation, begin_collection, begin_join, family_matches, family_unavailable,
    prepare_collection_op, read_config_map, CollectionTxn, Discipline, FamilySpace, GenericSpace,
    OuterStore,
};
use super::namespace::NativeSpace;
use super::outer::array::ArrayOuter;
use super::outer::hashmap::HashMapOuter;
use super::outer::pathmap::PathMapOuter;
use super::outer::vector::VectorOuter;
use super::types::config::SpaceConfig;
use crate::rust::interpreter::reified::costs::ScanMeter;

/// The agent of the family `hashmap:queue`.
static HASHMAP_QUEUE: GenericSpace<HashMapOuter, QueueDiscipline> =
    GenericSpace::new(HashMapOuter, QueueDiscipline);

/// The agent of the family `hashmap:bag`.
static HASHMAP_BAG: GenericSpace<HashMapOuter, BagDiscipline> =
    GenericSpace::new(HashMapOuter, BagDiscipline);

/// The agent of the family `hashmap:stack`.
static HASHMAP_STACK: GenericSpace<HashMapOuter, StackDiscipline> =
    GenericSpace::new(HashMapOuter, StackDiscipline);

/// The agent of the family `hashmap:set`.
static HASHMAP_SET: GenericSpace<HashMapOuter, SetDiscipline> =
    GenericSpace::new(HashMapOuter, SetDiscipline);

/// The agent of the family `hashmap:cell`.
static HASHMAP_CELL: GenericSpace<HashMapOuter, CellDiscipline> =
    GenericSpace::new(HashMapOuter, CellDiscipline);

/// The agent of the family `hashmap:priorityqueue`.
static HASHMAP_PRIORITY_QUEUE: GenericSpace<HashMapOuter, PriorityQueueDiscipline> =
    GenericSpace::new(HashMapOuter, PriorityQueueDiscipline);

/// The agent of the family `array:bag`.
static ARRAY_BAG: GenericSpace<ArrayOuter, BagDiscipline> =
    GenericSpace::new(ArrayOuter, BagDiscipline);

/// The agent of the family `array:queue`.
static ARRAY_QUEUE: GenericSpace<ArrayOuter, QueueDiscipline> =
    GenericSpace::new(ArrayOuter, QueueDiscipline);

/// The agent of the family `array:stack`.
static ARRAY_STACK: GenericSpace<ArrayOuter, StackDiscipline> =
    GenericSpace::new(ArrayOuter, StackDiscipline);

/// The agent of the family `array:set`.
static ARRAY_SET: GenericSpace<ArrayOuter, SetDiscipline> =
    GenericSpace::new(ArrayOuter, SetDiscipline);

/// The agent of the family `array:cell`.
static ARRAY_CELL: GenericSpace<ArrayOuter, CellDiscipline> =
    GenericSpace::new(ArrayOuter, CellDiscipline);

/// The agent of the family `array:priorityqueue`.
static ARRAY_PRIORITY_QUEUE: GenericSpace<ArrayOuter, PriorityQueueDiscipline> =
    GenericSpace::new(ArrayOuter, PriorityQueueDiscipline);

/// The agent of the family `vector:bag`.
static VECTOR_BAG: GenericSpace<VectorOuter, BagDiscipline> =
    GenericSpace::new(VectorOuter, BagDiscipline);

/// The agent of the family `vector:queue`.
static VECTOR_QUEUE: GenericSpace<VectorOuter, QueueDiscipline> =
    GenericSpace::new(VectorOuter, QueueDiscipline);

/// The agent of the family `vector:stack`.
static VECTOR_STACK: GenericSpace<VectorOuter, StackDiscipline> =
    GenericSpace::new(VectorOuter, StackDiscipline);

/// The agent of the family `vector:set`.
static VECTOR_SET: GenericSpace<VectorOuter, SetDiscipline> =
    GenericSpace::new(VectorOuter, SetDiscipline);

/// The agent of the family `vector:cell`.
static VECTOR_CELL: GenericSpace<VectorOuter, CellDiscipline> =
    GenericSpace::new(VectorOuter, CellDiscipline);

/// The agent of the family `vector:priorityqueue`.
static VECTOR_PRIORITY_QUEUE: GenericSpace<VectorOuter, PriorityQueueDiscipline> =
    GenericSpace::new(VectorOuter, PriorityQueueDiscipline);

/// The agent of the family `pathmap:bag` (P5e).
static PATHMAP_BAG: GenericSpace<PathMapOuter, BagDiscipline> =
    GenericSpace::new(PathMapOuter, BagDiscipline);

/// The agent of the family `pathmap:queue` (P5e).
static PATHMAP_QUEUE: GenericSpace<PathMapOuter, QueueDiscipline> =
    GenericSpace::new(PathMapOuter, QueueDiscipline);

/// The agent of the family `pathmap:stack` (P5e).
static PATHMAP_STACK: GenericSpace<PathMapOuter, StackDiscipline> =
    GenericSpace::new(PathMapOuter, StackDiscipline);

/// The agent of the family `pathmap:set` (P5e).
static PATHMAP_SET: GenericSpace<PathMapOuter, SetDiscipline> =
    GenericSpace::new(PathMapOuter, SetDiscipline);

/// The agent of the family `pathmap:cell` (P5e).
static PATHMAP_CELL: GenericSpace<PathMapOuter, CellDiscipline> =
    GenericSpace::new(PathMapOuter, CellDiscipline);

/// The agent of the family `pathmap:priorityqueue` (P5e).
static PATHMAP_PRIORITY_QUEUE: GenericSpace<PathMapOuter, PriorityQueueDiscipline> =
    GenericSpace::new(PathMapOuter, PriorityQueueDiscipline);

/// The agent of the family `hashmap:vectordb` (P6).
static HASHMAP_VECTORDB: GenericSpace<HashMapOuter, VectorDbDiscipline> =
    GenericSpace::new(HashMapOuter, VectorDbDiscipline);

/// The agent of the family `vector:vectordb` (P6).
static VECTOR_VECTORDB: GenericSpace<VectorOuter, VectorDbDiscipline> =
    GenericSpace::new(VectorOuter, VectorDbDiscipline);

/// A wired family of native spaces.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Family {
    /// `hashmap:queue` (P4).
    HashMapQueue,
    /// `hashmap:bag` (P5a).
    HashMapBag,
    /// `hashmap:stack` (P5b).
    HashMapStack,
    /// `hashmap:set` (P5a).
    HashMapSet,
    /// `hashmap:cell` (P5, F15).
    HashMapCell,
    /// `hashmap:priorityqueue` (P5b, decision 38).
    HashMapPriorityQueue,
    /// `array:bag` (P5d).
    ArrayBag,
    /// `array:queue` (P5d).
    ArrayQueue,
    /// `array:stack` (P5d).
    ArrayStack,
    /// `array:set` (P5d).
    ArraySet,
    /// `array:cell` (P5d).
    ArrayCell,
    /// `array:priorityqueue` (P5d).
    ArrayPriorityQueue,
    /// `vector:bag` (P5d).
    VectorBag,
    /// `vector:queue` (P5d).
    VectorQueue,
    /// `vector:stack` (P5d).
    VectorStack,
    /// `vector:set` (P5d).
    VectorSet,
    /// `vector:cell` (P5d).
    VectorCell,
    /// `vector:priorityqueue` (P5d).
    VectorPriorityQueue,
    /// `pathmap:bag` (P5e).
    PathMapBag,
    /// `pathmap:queue` (P5e).
    PathMapQueue,
    /// `pathmap:stack` (P5e).
    PathMapStack,
    /// `pathmap:set` (P5e).
    PathMapSet,
    /// `pathmap:cell` (P5e).
    PathMapCell,
    /// `pathmap:priorityqueue` (P5e).
    PathMapPriorityQueue,
    /// `hashmap:vectordb` (P6).
    HashMapVectorDb,
    /// `vector:vectordb` (P6).
    VectorVectorDb,
}

impl Family {
    /// The wired families, in dispatch order: `hashmap:queue` first, then ascending family index,
    /// then the PathMap families of P5e, and the VectorDB families of P6 last. So the dispatch of
    /// each family that a later phase found wired reserves the comparisons that it reserved
    /// before.
    pub const WIRED: [Family; 26] = [
        Family::HashMapQueue,
        Family::HashMapBag,
        Family::HashMapStack,
        Family::HashMapSet,
        Family::HashMapCell,
        Family::HashMapPriorityQueue,
        Family::ArrayBag,
        Family::ArrayQueue,
        Family::ArrayStack,
        Family::ArraySet,
        Family::ArrayCell,
        Family::ArrayPriorityQueue,
        Family::VectorBag,
        Family::VectorQueue,
        Family::VectorStack,
        Family::VectorSet,
        Family::VectorCell,
        Family::VectorPriorityQueue,
        Family::PathMapBag,
        Family::PathMapQueue,
        Family::PathMapStack,
        Family::PathMapSet,
        Family::PathMapCell,
        Family::PathMapPriorityQueue,
        Family::HashMapVectorDb,
        Family::VectorVectorDb,
    ];

    /// The family of the factory URN `urn`, or `None` when it is not wired.
    pub fn of_urn(urn: SpaceFactoryUrn) -> Option<Family> {
        match (urn.outer, urn.inner) {
            (SpaceOuter::HashMap, SpaceInner::Queue) => Some(Family::HashMapQueue),
            (SpaceOuter::HashMap, SpaceInner::Bag) => Some(Family::HashMapBag),
            (SpaceOuter::HashMap, SpaceInner::Stack) => Some(Family::HashMapStack),
            (SpaceOuter::HashMap, SpaceInner::Set) => Some(Family::HashMapSet),
            (SpaceOuter::HashMap, SpaceInner::Cell) => Some(Family::HashMapCell),
            (SpaceOuter::HashMap, SpaceInner::PriorityQueue) => Some(Family::HashMapPriorityQueue),
            (SpaceOuter::Array, SpaceInner::Bag) => Some(Family::ArrayBag),
            (SpaceOuter::Array, SpaceInner::Queue) => Some(Family::ArrayQueue),
            (SpaceOuter::Array, SpaceInner::Stack) => Some(Family::ArrayStack),
            (SpaceOuter::Array, SpaceInner::Set) => Some(Family::ArraySet),
            (SpaceOuter::Array, SpaceInner::Cell) => Some(Family::ArrayCell),
            (SpaceOuter::Array, SpaceInner::PriorityQueue) => Some(Family::ArrayPriorityQueue),
            (SpaceOuter::Vector, SpaceInner::Bag) => Some(Family::VectorBag),
            (SpaceOuter::Vector, SpaceInner::Queue) => Some(Family::VectorQueue),
            (SpaceOuter::Vector, SpaceInner::Stack) => Some(Family::VectorStack),
            (SpaceOuter::Vector, SpaceInner::Set) => Some(Family::VectorSet),
            (SpaceOuter::Vector, SpaceInner::Cell) => Some(Family::VectorCell),
            (SpaceOuter::Vector, SpaceInner::PriorityQueue) => Some(Family::VectorPriorityQueue),
            (SpaceOuter::PathMap, SpaceInner::Bag) => Some(Family::PathMapBag),
            (SpaceOuter::PathMap, SpaceInner::Queue) => Some(Family::PathMapQueue),
            (SpaceOuter::PathMap, SpaceInner::Stack) => Some(Family::PathMapStack),
            (SpaceOuter::PathMap, SpaceInner::Set) => Some(Family::PathMapSet),
            (SpaceOuter::PathMap, SpaceInner::Cell) => Some(Family::PathMapCell),
            (SpaceOuter::PathMap, SpaceInner::PriorityQueue) => Some(Family::PathMapPriorityQueue),
            (SpaceOuter::HashMap, SpaceInner::VectorDb) => Some(Family::HashMapVectorDb),
            (SpaceOuter::Vector, SpaceInner::VectorDb) => Some(Family::VectorVectorDb),
            // Every inner collection of the HashMap and Vector stores is wired since P6. The
            // PathMap and Array stores have no VectorDB family (design v3 4.2), and HashSet is
            // wired in P7 (F1).
            (SpaceOuter::PathMap | SpaceOuter::Array | SpaceOuter::HashSet, _) => None,
        }
    }

    /// The family that `config` names, or `None` when it is not wired. Each arm reserves the
    /// comparison of the config names with its family before it compares them.
    pub fn of_config(
        config: &SpaceConfig,
        meter: &ScanMeter,
    ) -> Result<Option<Family>, SpaceError> {
        for family in Family::WIRED {
            if family.is_named_by(config, meter)? {
                return Ok(Some(family));
            }
        }
        Ok(None)
    }

    /// True when `config` names this family. The comparison is reserved first.
    fn is_named_by(self, config: &SpaceConfig, meter: &ScanMeter) -> Result<bool, SpaceError> {
        match self {
            Family::HashMapQueue => family_matches::<HashMapOuter, QueueDiscipline>(config, meter),
            Family::HashMapBag => family_matches::<HashMapOuter, BagDiscipline>(config, meter),
            Family::HashMapStack => family_matches::<HashMapOuter, StackDiscipline>(config, meter),
            Family::HashMapSet => family_matches::<HashMapOuter, SetDiscipline>(config, meter),
            Family::HashMapCell => family_matches::<HashMapOuter, CellDiscipline>(config, meter),
            Family::HashMapPriorityQueue => {
                family_matches::<HashMapOuter, PriorityQueueDiscipline>(config, meter)
            }
            Family::ArrayBag => family_matches::<ArrayOuter, BagDiscipline>(config, meter),
            Family::ArrayQueue => family_matches::<ArrayOuter, QueueDiscipline>(config, meter),
            Family::ArrayStack => family_matches::<ArrayOuter, StackDiscipline>(config, meter),
            Family::ArraySet => family_matches::<ArrayOuter, SetDiscipline>(config, meter),
            Family::ArrayCell => family_matches::<ArrayOuter, CellDiscipline>(config, meter),
            Family::ArrayPriorityQueue => {
                family_matches::<ArrayOuter, PriorityQueueDiscipline>(config, meter)
            }
            Family::VectorBag => family_matches::<VectorOuter, BagDiscipline>(config, meter),
            Family::VectorQueue => family_matches::<VectorOuter, QueueDiscipline>(config, meter),
            Family::VectorStack => family_matches::<VectorOuter, StackDiscipline>(config, meter),
            Family::VectorSet => family_matches::<VectorOuter, SetDiscipline>(config, meter),
            Family::VectorCell => family_matches::<VectorOuter, CellDiscipline>(config, meter),
            Family::VectorPriorityQueue => {
                family_matches::<VectorOuter, PriorityQueueDiscipline>(config, meter)
            }
            Family::PathMapBag => family_matches::<PathMapOuter, BagDiscipline>(config, meter),
            Family::PathMapQueue => family_matches::<PathMapOuter, QueueDiscipline>(config, meter),
            Family::PathMapStack => family_matches::<PathMapOuter, StackDiscipline>(config, meter),
            Family::PathMapSet => family_matches::<PathMapOuter, SetDiscipline>(config, meter),
            Family::PathMapCell => family_matches::<PathMapOuter, CellDiscipline>(config, meter),
            Family::PathMapPriorityQueue => {
                family_matches::<PathMapOuter, PriorityQueueDiscipline>(config, meter)
            }
            Family::HashMapVectorDb => {
                family_matches::<HashMapOuter, VectorDbDiscipline>(config, meter)
            }
            Family::VectorVectorDb => {
                family_matches::<VectorOuter, VectorDbDiscipline>(config, meter)
            }
        }
    }

    /// The agent of this family.
    pub fn agent(self) -> &'static dyn FamilySpace {
        match self {
            Family::HashMapQueue => &HASHMAP_QUEUE,
            Family::HashMapBag => &HASHMAP_BAG,
            Family::HashMapStack => &HASHMAP_STACK,
            Family::HashMapSet => &HASHMAP_SET,
            Family::HashMapCell => &HASHMAP_CELL,
            Family::HashMapPriorityQueue => &HASHMAP_PRIORITY_QUEUE,
            Family::ArrayBag => &ARRAY_BAG,
            Family::ArrayQueue => &ARRAY_QUEUE,
            Family::ArrayStack => &ARRAY_STACK,
            Family::ArraySet => &ARRAY_SET,
            Family::ArrayCell => &ARRAY_CELL,
            Family::ArrayPriorityQueue => &ARRAY_PRIORITY_QUEUE,
            Family::VectorBag => &VECTOR_BAG,
            Family::VectorQueue => &VECTOR_QUEUE,
            Family::VectorStack => &VECTOR_STACK,
            Family::VectorSet => &VECTOR_SET,
            Family::VectorCell => &VECTOR_CELL,
            Family::VectorPriorityQueue => &VECTOR_PRIORITY_QUEUE,
            Family::PathMapBag => &PATHMAP_BAG,
            Family::PathMapQueue => &PATHMAP_QUEUE,
            Family::PathMapStack => &PATHMAP_STACK,
            Family::PathMapSet => &PATHMAP_SET,
            Family::PathMapCell => &PATHMAP_CELL,
            Family::PathMapPriorityQueue => &PATHMAP_PRIORITY_QUEUE,
            Family::HashMapVectorDb => &HASHMAP_VECTORDB,
            Family::VectorVectorDb => &VECTOR_VECTORDB,
        }
    }

    /// The named params of a send to a collection of this family: the schema of its discipline.
    pub fn send_params(self) -> &'static [&'static str] {
        match self {
            Family::HashMapQueue
            | Family::ArrayQueue
            | Family::VectorQueue
            | Family::PathMapQueue => QueueDiscipline::SEND_PARAMS,
            Family::HashMapBag | Family::ArrayBag | Family::VectorBag | Family::PathMapBag => {
                BagDiscipline::SEND_PARAMS
            }
            Family::HashMapStack
            | Family::ArrayStack
            | Family::VectorStack
            | Family::PathMapStack => StackDiscipline::SEND_PARAMS,
            Family::HashMapCell | Family::ArrayCell | Family::VectorCell | Family::PathMapCell => {
                CellDiscipline::SEND_PARAMS
            }
            Family::HashMapSet | Family::ArraySet | Family::VectorSet | Family::PathMapSet => {
                SetDiscipline::SEND_PARAMS
            }
            Family::HashMapPriorityQueue
            | Family::ArrayPriorityQueue
            | Family::VectorPriorityQueue
            | Family::PathMapPriorityQueue => PriorityQueueDiscipline::SEND_PARAMS,
            Family::HashMapVectorDb | Family::VectorVectorDb => VectorDbDiscipline::SEND_PARAMS,
        }
    }

    /// The named query params of a receive on a collection of this family.
    pub fn query_params(self) -> &'static [&'static str] {
        match self {
            Family::HashMapQueue
            | Family::ArrayQueue
            | Family::VectorQueue
            | Family::PathMapQueue => QueueDiscipline::QUERY_PARAMS,
            Family::HashMapBag | Family::ArrayBag | Family::VectorBag | Family::PathMapBag => {
                BagDiscipline::QUERY_PARAMS
            }
            Family::HashMapStack
            | Family::ArrayStack
            | Family::VectorStack
            | Family::PathMapStack => StackDiscipline::QUERY_PARAMS,
            Family::HashMapCell | Family::ArrayCell | Family::VectorCell | Family::PathMapCell => {
                CellDiscipline::QUERY_PARAMS
            }
            Family::HashMapSet | Family::ArraySet | Family::VectorSet | Family::PathMapSet => {
                SetDiscipline::QUERY_PARAMS
            }
            Family::HashMapPriorityQueue
            | Family::ArrayPriorityQueue
            | Family::VectorPriorityQueue
            | Family::PathMapPriorityQueue => PriorityQueueDiscipline::QUERY_PARAMS,
            Family::HashMapVectorDb | Family::VectorVectorDb => VectorDbDiscipline::QUERY_PARAMS,
        }
    }
}

/// The named params of a send to a collection of the family `urn`, or `None` when the family is
/// not wired. The queue takes none, so `priority` is an unknown param of a queue.
pub fn send_params(urn: SpaceFactoryUrn) -> Option<&'static [&'static str]> {
    Family::of_urn(urn).map(Family::send_params)
}

/// The named query params of a receive on a collection of the family `urn`, or `None` when the
/// family is not wired.
pub fn query_params(urn: SpaceFactoryUrn) -> Option<&'static [&'static str]> {
    Family::of_urn(urn).map(Family::query_params)
}

/// The space agent of every wired family.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct FamilyAgent;

/// Runs the collection operation `op` in the open transaction `opened` with the agent of the
/// family that the config names: the driver of the collection, or the join engine of P5c after
/// the escalation of its locks (`generic::prepare_collection_op`).
async fn dispatch(
    env: &SpaceEnv,
    opened: CollectionTxn,
    op: SpaceOp,
) -> Result<Prepared<SpaceOutcome>, SpaceError> {
    match Family::of_config(&opened.config, &env.meter)? {
        Some(family) => prepare_collection_op(family.agent(), env, opened, op).await,
        None => Err(family_unavailable(opened.config)),
    }
}

#[async_trait]
impl SpaceAgent for FamilyAgent {
    async fn prepare_produce(
        &self,
        env: &SpaceEnv,
        op: ProduceOp,
    ) -> Result<Prepared<SpaceOutcome>, SpaceError> {
        let opened = begin_collection(&HashMapOuter, env, op.space, &op.key, &op.channel).await?;
        dispatch(env, opened, SpaceOp::Produce(op)).await
    }

    async fn prepare_consume(
        &self,
        env: &SpaceEnv,
        op: ConsumeOp,
    ) -> Result<Prepared<SpaceOutcome>, SpaceError> {
        let opened = begin_collection(&HashMapOuter, env, op.space, &op.key, &op.channel).await?;
        dispatch(env, opened, SpaceOp::Consume(op)).await
    }

    async fn prepare_settle(
        &self,
        env: &SpaceEnv,
        op: SettleOp,
    ) -> Result<Prepared<SpaceOutcome>, SpaceError> {
        let opened = begin_collection(&HashMapOuter, env, op.space, &op.key, &op.channel).await?;
        dispatch(env, opened, SpaceOp::Settle(op)).await
    }

    /// The join locks the collections of its binds in the HashMap store, as every keyed
    /// operation does (A5).
    async fn prepare_join(
        &self,
        env: &SpaceEnv,
        op: JoinOp,
    ) -> Result<Prepared<SpaceOutcome>, SpaceError> {
        let opened = begin_join(&HashMapOuter, env, &op).await?;
        dispatch(env, opened, SpaceOp::Join(op)).await
    }

    async fn allocate(&self, env: &SpaceEnv, op: AllocOp) -> Result<Allocation, SpaceError> {
        let opened = begin_allocation(env, op.space).await?;
        match Family::of_config(&opened.config, &env.meter)? {
            Some(family) => family.agent().run_allocate(env, opened, &op),
            None => Err(family_unavailable(opened.config)),
        }
    }

    /// Sends the creation to the agent of the family of the factory. That agent decodes the new
    /// config and checks that it names the family, so a creation whose config names another
    /// family fails with `FamilyUnavailable`.
    async fn prepare_create(
        &self,
        env: &SpaceEnv,
        creation: SpaceCreation,
    ) -> Result<Prepared<()>, SpaceError> {
        match Family::of_urn(creation.family) {
            Some(family) => family.agent().prepare_create(env, creation).await,
            None => Err(SpaceError::FamilyUnavailable {
                outer: creation.family.outer.as_str().to_string(),
                inner: creation.family.inner.as_str().to_string(),
            }),
        }
    }

    async fn read_config(&self, env: &SpaceEnv, space: NativeSpace) -> Result<Par, SpaceError> {
        read_config_map(env, space).await
    }

    /// The random-tail name of the HashMap store. The PathMap store names its private subtrees
    /// the same way, and an Array or a Vector space allocates its index names with
    /// [`SpaceAgent::allocate`] instead (P5d).
    fn gensym(&self, space: NativeSpace, random: &Blake2b512Random) -> Result<Par, SpaceError> {
        HashMapOuter.gensym(space, random)
    }
}

#[cfg(test)]
mod tests {
    use models::rust::host_work::{
        HostWorkDimension, HostWorkLimit, HostWorkLimits, HostWorkUsage,
    };

    use super::*;
    use crate::rust::interpreter::host_work::HostWorkBudget;
    use crate::rust::interpreter::spaces::namespace::SID_LEN;
    use crate::rust::interpreter::spaces::types::qualifier::SpaceQualifier;

    fn config(outer: &str, inner: &str) -> SpaceConfig {
        SpaceConfig::new([3; SID_LEN], outer, inner, SpaceQualifier::Default)
    }

    /// The URN of the family `outer:inner`.
    fn urn(outer: SpaceOuter, inner: SpaceInner) -> SpaceFactoryUrn {
        SpaceFactoryUrn::all()
            .find(|urn| urn.outer == outer && urn.inner == inner)
            .expect("a factory family")
    }

    /// The wired families in dispatch order, with the names of their configs.
    const WIRED_NAMES: [(Family, &str, &str); 26] = [
        (Family::HashMapQueue, "hashmap", "queue"),
        (Family::HashMapBag, "hashmap", "bag"),
        (Family::HashMapStack, "hashmap", "stack"),
        (Family::HashMapSet, "hashmap", "set"),
        (Family::HashMapCell, "hashmap", "cell"),
        (Family::HashMapPriorityQueue, "hashmap", "priorityqueue"),
        (Family::ArrayBag, "array", "bag"),
        (Family::ArrayQueue, "array", "queue"),
        (Family::ArrayStack, "array", "stack"),
        (Family::ArraySet, "array", "set"),
        (Family::ArrayCell, "array", "cell"),
        (Family::ArrayPriorityQueue, "array", "priorityqueue"),
        (Family::VectorBag, "vector", "bag"),
        (Family::VectorQueue, "vector", "queue"),
        (Family::VectorStack, "vector", "stack"),
        (Family::VectorSet, "vector", "set"),
        (Family::VectorCell, "vector", "cell"),
        (Family::VectorPriorityQueue, "vector", "priorityqueue"),
        (Family::PathMapBag, "pathmap", "bag"),
        (Family::PathMapQueue, "pathmap", "queue"),
        (Family::PathMapStack, "pathmap", "stack"),
        (Family::PathMapSet, "pathmap", "set"),
        (Family::PathMapCell, "pathmap", "cell"),
        (Family::PathMapPriorityQueue, "pathmap", "priorityqueue"),
        (Family::HashMapVectorDb, "hashmap", "vectordb"),
        (Family::VectorVectorDb, "vector", "vectordb"),
    ];

    #[test]
    fn space_families_wire_exactly_the_listed_families() {
        assert_eq!(WIRED_NAMES.map(|(family, _, _)| family), Family::WIRED);
        let wired = WIRED_NAMES.map(|(family, outer, inner)| {
            let outer = SpaceOuter::parse(outer).expect("an outer store");
            let inner = SpaceInner::parse(inner).expect("an inner collection");
            (urn(outer, inner), family)
        });
        assert_eq!(wired[0].0, SpaceFactoryUrn::HASHMAP_QUEUE);
        for urn in SpaceFactoryUrn::all() {
            let family = wired
                .iter()
                .find(|(wired_urn, _)| *wired_urn == urn)
                .map(|(_, family)| *family);
            assert_eq!(Family::of_urn(urn), family, "{urn}");
            assert_eq!(send_params(urn), family.map(Family::send_params), "{urn}");
            assert_eq!(query_params(urn), family.map(Family::query_params), "{urn}");
        }
        for (family, send, query) in [
            (
                Family::HashMapQueue,
                QueueDiscipline::SEND_PARAMS,
                QueueDiscipline::QUERY_PARAMS,
            ),
            (
                Family::HashMapBag,
                BagDiscipline::SEND_PARAMS,
                BagDiscipline::QUERY_PARAMS,
            ),
            (
                Family::HashMapStack,
                StackDiscipline::SEND_PARAMS,
                StackDiscipline::QUERY_PARAMS,
            ),
            (
                Family::HashMapCell,
                CellDiscipline::SEND_PARAMS,
                CellDiscipline::QUERY_PARAMS,
            ),
            (
                Family::HashMapSet,
                SetDiscipline::SEND_PARAMS,
                SetDiscipline::QUERY_PARAMS,
            ),
            (
                Family::HashMapPriorityQueue,
                PriorityQueueDiscipline::SEND_PARAMS,
                PriorityQueueDiscipline::QUERY_PARAMS,
            ),
            (
                Family::ArrayBag,
                BagDiscipline::SEND_PARAMS,
                BagDiscipline::QUERY_PARAMS,
            ),
            (
                Family::ArrayQueue,
                QueueDiscipline::SEND_PARAMS,
                QueueDiscipline::QUERY_PARAMS,
            ),
            (
                Family::ArrayStack,
                StackDiscipline::SEND_PARAMS,
                StackDiscipline::QUERY_PARAMS,
            ),
            (
                Family::ArrayCell,
                CellDiscipline::SEND_PARAMS,
                CellDiscipline::QUERY_PARAMS,
            ),
            (
                Family::ArraySet,
                SetDiscipline::SEND_PARAMS,
                SetDiscipline::QUERY_PARAMS,
            ),
            (
                Family::ArrayPriorityQueue,
                PriorityQueueDiscipline::SEND_PARAMS,
                PriorityQueueDiscipline::QUERY_PARAMS,
            ),
            (
                Family::VectorBag,
                BagDiscipline::SEND_PARAMS,
                BagDiscipline::QUERY_PARAMS,
            ),
            (
                Family::VectorQueue,
                QueueDiscipline::SEND_PARAMS,
                QueueDiscipline::QUERY_PARAMS,
            ),
            (
                Family::VectorStack,
                StackDiscipline::SEND_PARAMS,
                StackDiscipline::QUERY_PARAMS,
            ),
            (
                Family::VectorCell,
                CellDiscipline::SEND_PARAMS,
                CellDiscipline::QUERY_PARAMS,
            ),
            (
                Family::VectorSet,
                SetDiscipline::SEND_PARAMS,
                SetDiscipline::QUERY_PARAMS,
            ),
            (
                Family::VectorPriorityQueue,
                PriorityQueueDiscipline::SEND_PARAMS,
                PriorityQueueDiscipline::QUERY_PARAMS,
            ),
            (
                Family::PathMapBag,
                BagDiscipline::SEND_PARAMS,
                BagDiscipline::QUERY_PARAMS,
            ),
            (
                Family::PathMapQueue,
                QueueDiscipline::SEND_PARAMS,
                QueueDiscipline::QUERY_PARAMS,
            ),
            (
                Family::PathMapStack,
                StackDiscipline::SEND_PARAMS,
                StackDiscipline::QUERY_PARAMS,
            ),
            (
                Family::PathMapCell,
                CellDiscipline::SEND_PARAMS,
                CellDiscipline::QUERY_PARAMS,
            ),
            (
                Family::PathMapSet,
                SetDiscipline::SEND_PARAMS,
                SetDiscipline::QUERY_PARAMS,
            ),
            (
                Family::PathMapPriorityQueue,
                PriorityQueueDiscipline::SEND_PARAMS,
                PriorityQueueDiscipline::QUERY_PARAMS,
            ),
            (
                Family::HashMapVectorDb,
                VectorDbDiscipline::SEND_PARAMS,
                VectorDbDiscipline::QUERY_PARAMS,
            ),
            (
                Family::VectorVectorDb,
                VectorDbDiscipline::SEND_PARAMS,
                VectorDbDiscipline::QUERY_PARAMS,
            ),
        ] {
            assert_eq!(family.send_params(), send, "{family:?}");
            assert_eq!(family.query_params(), query, "{family:?}");
        }
    }

    #[test]
    fn space_families_dispatch_by_config_reserves_one_comparison_per_arm() {
        let arm_bytes = |arms: usize| -> u64 {
            WIRED_NAMES[..arms]
                .iter()
                .map(|(_, outer, inner)| (outer.len() + inner.len()) as u64)
                .sum()
        };
        let unwired = [
            ("hashmap", "deque"),
            ("pathmap", "vectordb"),
            ("array", "vectordb"),
            ("hashset", "set"),
        ];
        let mut cases = Vec::with_capacity(WIRED_NAMES.len() + unwired.len());
        for (position, (family, outer, inner)) in WIRED_NAMES.into_iter().enumerate() {
            cases.push((outer, inner, Some(family), position + 1));
        }
        for (outer, inner) in unwired {
            cases.push((outer, inner, None, WIRED_NAMES.len()));
        }
        for (outer, inner, expected, arms) in cases {
            let host_work = HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(1_000)));
            let meter = ScanMeter::new(Some(host_work.clone()));
            assert_eq!(
                Family::of_config(&config(outer, inner), &meter),
                Ok(expected),
                "{outer}:{inner}"
            );
            // Each arm that the dispatch tries reserves one comparison, which reads the names of
            // the config and of the arm. The first arm costs what the P4 agent reserves.
            let config_bytes = (outer.len() + inner.len()) as u64;
            assert_eq!(
                host_work.usage(HostWorkDimension::VerificationOperations),
                HostWorkUsage::new(arms as u64),
                "{outer}:{inner}"
            );
            assert_eq!(
                host_work.usage(HostWorkDimension::VerificationBytes),
                HostWorkUsage::new(arms as u64 * config_bytes + arm_bytes(arms)),
                "{outer}:{inner}"
            );
        }
    }
}
