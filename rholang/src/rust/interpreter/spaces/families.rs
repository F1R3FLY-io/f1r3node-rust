//! The space agent of every family (P5 design v3, A.3.4).
//!
//! [`FamilyAgent`] is the [`SpaceAgent`] of the native spaces. A collection operation opens its
//! transaction over the channels of its collection and reads the config of its space once. The
//! agent of the family that the config names then runs the operation in that transaction
// Changed by decision 127 (kind registry): the families are values of the registry. The text
// before:
// ```
// //! ([`FamilySpace::run_with`]). The dispatch is static, as the agent factory of the legacy branch
// //! was. A family that is not wired fails with `FamilyUnavailable`.
// ```
//! ([`FamilySpace::run_with`]). The registry of kinds holds the families, and a family that is
//! not registered fails with `FamilyUnavailable`.
//!
//! [`Family`] is the one source of truth for the wired families. It gives the agent of each
//! family, its send params and its query params. The factory and the handle method `produce`
//! read the param schemas from it.
// Changed by decisions 94 and 128 (kind routing): phase A opens the routed layout, and no
// operation runs in a new transaction. The text before:
// ```
// //! The HashMap, Array and Vector stores key a collection by the canonical bytes of its channel
// //! (A5), so the agent opens every collection with the channels of the HashMap store. A PathMap
// //! operation opens the same first transaction to read the config. The PathMap store then runs it
// //! in a new transaction over the lock keys of its prefix scope (`pathmap::join`).
// ```
//!
//! A store of a routed layout keys a collection by the canonical bytes of its channel (A5), so
//! the agent opens the collection with the channels of that layout.
// Changed by decision 127 (kind registry): a registered kind adds families with no edit here.
// The text before:
// ```
// //! Each arm of the dispatch compares the config names with its family. The first arm is
// //! `hashmap:queue`, the family of the P4 agent. The other arms follow in ascending family index,
// //! then the PathMap arms of P5e, then the VectorDB arms of P6, then the HashSet arms of P7. So the
// //! arm at dispatch position `i` makes `i + 1` comparisons, and a later phase changes the dispatch
// //! of no earlier family.
// ```
//!
//! The dispatch asks the registry for the family that the config names (`Family::of_config`).
//! The registration order keeps the dispatch of every earlier family unchanged.

// Added by decision 127 (kind registry).
use std::fmt;
use std::hash::{Hash, Hasher};

use async_trait::async_trait;
use crypto::rust::hash::blake2b512_random::Blake2b512Random;
// Added by decision 127 (kind registry).
use linkme::distributed_slice;
use models::rhoapi::Par;
// Changed by decision 127 (kind registry): a family carries the qualifiers of its store. The text
// before:
// use models::rust::space_factory_urn::{SpaceFactoryUrn, SpaceInner, SpaceOuter};
use models::rust::space_factory_urn::{SpaceFactoryUrn, SpaceInner, SpaceOuter, SpaceQualifier};

use super::agent::{
    AllocOp, Allocation, ConsumeOp, JoinOp, Prepared, ProduceOp, SettleOp, SpaceAgent,
    SpaceCreation, SpaceEnv, SpaceOp, SpaceOutcome,
};
use super::disciplines::bag::BagDiscipline;
use super::disciplines::cell::CellDiscipline;
use super::disciplines::priority_queue::PriorityQueueDiscipline;
use super::disciplines::queue::QueueDiscipline;
use super::disciplines::set::SetDiscipline;
// Added by decision 128 (kind routing).
use super::disciplines::similarity::QUERY_VECTOR_KEY;
use super::disciplines::stack::StackDiscipline;
use super::disciplines::vectordb::VectorDbDiscipline;
use super::errors::SpaceError;
// Added by decision 127 (kind registry).
use super::factory::defaults::{
    ParamCheck, ARRAY_PARAMS, ARRAY_PRIORITY_QUEUE_PARAMS, ARRAY_REQUIRED, NO_PARAMS,
    PRIORITY_QUEUE_PARAMS, VECTORDB_PARAMS, VECTORDB_REQUIRED,
};
// Changed by decision 127 (kind registry): a family compares its own names with a config, and
// it carries the join window of its discipline. The text before:
// ```
// use super::generic::{
//     begin_allocation, begin_collection, begin_join, family_matches, family_unavailable,
//     prepare_collection_op, read_config_map, CollectionTxn, Discipline, FamilySpace, GenericSpace,
//     OuterStore,
// };
// ```
use super::generic::{
    begin_allocation, begin_collection, begin_join, family_unavailable, prepare_collection_op,
    read_config_map, CollectionTxn, Discipline, FamilySpace, GenericSpace, JoinWindow, OuterStore,
};
use super::namespace::NativeSpace;
use super::outer::array::ArrayOuter;
use super::outer::hashmap::HashMapOuter;
use super::outer::hashset::HashSetOuter;
use super::outer::pathmap::PathMapOuter;
use super::outer::vector::VectorOuter;
use super::types::config::SpaceConfig;
// Added by decision 127 (kind registry).
use crate::rust::interpreter::reified::system_channels::{
    is_kind_op, SystemChannel, ARRAY_BAG_FACTORY, ARRAY_CELL_FACTORY, ARRAY_PRIORITY_QUEUE_FACTORY,
    ARRAY_QUEUE_FACTORY, ARRAY_SET_FACTORY, ARRAY_STACK_FACTORY, HASHMAP_BAG_FACTORY,
    HASHMAP_CELL_FACTORY, HASHMAP_PRIORITY_QUEUE_FACTORY, HASHMAP_QUEUE_FACTORY,
    HASHMAP_SET_FACTORY, HASHMAP_STACK_FACTORY, HASHMAP_VECTORDB_FACTORY, HASHSET_BAG_FACTORY,
    HASHSET_CELL_FACTORY, HASHSET_PRIORITY_QUEUE_FACTORY, HASHSET_QUEUE_FACTORY,
    HASHSET_SET_FACTORY, HASHSET_STACK_FACTORY, P4_SYSTEM_CHANNELS, P5_SYSTEM_CHANNELS,
    P6_PROCESSES, P6_SYSTEM_CHANNELS, P7_SYSTEM_CHANNELS, P8_PROCESSES, PATHMAP_BAG_FACTORY,
    PATHMAP_CELL_FACTORY, PATHMAP_PRIORITY_QUEUE_FACTORY, PATHMAP_QUEUE_FACTORY,
    PATHMAP_SET_FACTORY, PATHMAP_STACK_FACTORY, VECTOR_BAG_FACTORY, VECTOR_CELL_FACTORY,
    VECTOR_PRIORITY_QUEUE_FACTORY, VECTOR_QUEUE_FACTORY, VECTOR_SET_FACTORY, VECTOR_STACK_FACTORY,
    VECTOR_VECTORDB_FACTORY,
};
use crate::rust::interpreter::reified::system_processes::SpaceProcess;

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

/// The agent of the family `hashset:bag` (P7).
static HASHSET_BAG: GenericSpace<HashSetOuter, BagDiscipline> =
    GenericSpace::new(HashSetOuter, BagDiscipline);

/// The agent of the family `hashset:queue` (P7).
static HASHSET_QUEUE: GenericSpace<HashSetOuter, QueueDiscipline> =
    GenericSpace::new(HashSetOuter, QueueDiscipline);

/// The agent of the family `hashset:stack` (P7).
static HASHSET_STACK: GenericSpace<HashSetOuter, StackDiscipline> =
    GenericSpace::new(HashSetOuter, StackDiscipline);

/// The agent of the family `hashset:set` (P7).
static HASHSET_SET: GenericSpace<HashSetOuter, SetDiscipline> =
    GenericSpace::new(HashSetOuter, SetDiscipline);

/// The agent of the family `hashset:cell` (P7).
static HASHSET_CELL: GenericSpace<HashSetOuter, CellDiscipline> =
    GenericSpace::new(HashSetOuter, CellDiscipline);

/// The agent of the family `hashset:priorityqueue` (P7).
static HASHSET_PRIORITY_QUEUE: GenericSpace<HashSetOuter, PriorityQueueDiscipline> =
    GenericSpace::new(HashSetOuter, PriorityQueueDiscipline);

/// A wired family of native spaces.
// Changed by decision 127 (kind registry): a family is a value of the registry, so a registered
// kind adds families with no edit here. The built-in families are the constants of the old
// variant names. The text before:
// ```
// #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
// pub enum Family {
//     /// `hashmap:queue` (P4).
//     HashMapQueue,
//     /// `hashmap:bag` (P5a).
//     HashMapBag,
//     /// `hashmap:stack` (P5b).
//     HashMapStack,
//     /// `hashmap:set` (P5a).
//     HashMapSet,
//     /// `hashmap:cell` (P5, F15).
//     HashMapCell,
//     /// `hashmap:priorityqueue` (P5b, decision 38).
//     HashMapPriorityQueue,
//     /// `array:bag` (P5d).
//     ArrayBag,
//     /// `array:queue` (P5d).
//     ArrayQueue,
//     /// `array:stack` (P5d).
//     ArrayStack,
//     /// `array:set` (P5d).
//     ArraySet,
//     /// `array:cell` (P5d).
//     ArrayCell,
//     /// `array:priorityqueue` (P5d).
//     ArrayPriorityQueue,
//     /// `vector:bag` (P5d).
//     VectorBag,
//     /// `vector:queue` (P5d).
//     VectorQueue,
//     /// `vector:stack` (P5d).
//     VectorStack,
//     /// `vector:set` (P5d).
//     VectorSet,
//     /// `vector:cell` (P5d).
//     VectorCell,
//     /// `vector:priorityqueue` (P5d).
//     VectorPriorityQueue,
//     /// `pathmap:bag` (P5e).
//     PathMapBag,
//     /// `pathmap:queue` (P5e).
//     PathMapQueue,
//     /// `pathmap:stack` (P5e).
//     PathMapStack,
//     /// `pathmap:set` (P5e).
//     PathMapSet,
//     /// `pathmap:cell` (P5e).
//     PathMapCell,
//     /// `pathmap:priorityqueue` (P5e).
//     PathMapPriorityQueue,
//     /// `hashmap:vectordb` (P6).
//     HashMapVectorDb,
//     /// `vector:vectordb` (P6).
//     VectorVectorDb,
//     /// `hashset:bag` (P7).
//     HashSetBag,
//     /// `hashset:queue` (P7).
//     HashSetQueue,
//     /// `hashset:stack` (P7).
//     HashSetStack,
//     /// `hashset:set` (P7).
//     HashSetSet,
//     /// `hashset:cell` (P7).
//     HashSetCell,
//     /// `hashset:priorityqueue` (P7).
//     HashSetPriorityQueue,
// }
// ```
#[derive(Clone, Copy)]
pub struct Family {
    urn: SpaceFactoryUrn,
    factory: SystemChannel,
    agent: &'static dyn FamilySpace,
    send_params: &'static [&'static str],
    query_params: &'static [&'static str],
    constructor: &'static [&'static str],
    required: &'static [&'static str],
    qualifiers: &'static [SpaceQualifier],
    param_rule: fn(&str) -> Option<ParamCheck>,
    /// Added by decision 128 (kind routing): the join window of the discipline of the family.
    window: JoinWindow,
}

// Added by decision 127 (kind registry): a family is its URN, as the variant was.
impl PartialEq for Family {
    fn eq(&self, other: &Family) -> bool { self.urn == other.urn }
}

impl Eq for Family {}

impl Hash for Family {
    fn hash<H: Hasher>(&self, state: &mut H) { self.urn.hash(state) }
}

impl fmt::Debug for Family {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "Family({})", self.urn) }
}

// Added by decision 127 (kind registry): the constants keep the names of the old variants.
#[allow(non_upper_case_globals)]
impl Family {
    /// `hashmap:queue` (P4).
    pub const HashMapQueue: Family =
        Family::new(&HASHMAP_QUEUE, HASHMAP_QUEUE_FACTORY, NO_PARAMS, NO_PARAMS);
    /// `hashmap:bag` (P5a).
    pub const HashMapBag: Family =
        Family::new(&HASHMAP_BAG, HASHMAP_BAG_FACTORY, NO_PARAMS, NO_PARAMS);
    /// `hashmap:stack` (P5b).
    pub const HashMapStack: Family =
        Family::new(&HASHMAP_STACK, HASHMAP_STACK_FACTORY, NO_PARAMS, NO_PARAMS);
    /// `hashmap:set` (P5a).
    pub const HashMapSet: Family =
        Family::new(&HASHMAP_SET, HASHMAP_SET_FACTORY, NO_PARAMS, NO_PARAMS);
    /// `hashmap:cell` (P5, F15).
    pub const HashMapCell: Family =
        Family::new(&HASHMAP_CELL, HASHMAP_CELL_FACTORY, NO_PARAMS, NO_PARAMS);
    /// `hashmap:priorityqueue` (P5b, decision 38).
    pub const HashMapPriorityQueue: Family = Family::new(
        &HASHMAP_PRIORITY_QUEUE,
        HASHMAP_PRIORITY_QUEUE_FACTORY,
        PRIORITY_QUEUE_PARAMS,
        NO_PARAMS,
    );
    /// `array:bag` (P5d).
    pub const ArrayBag: Family =
        Family::new(&ARRAY_BAG, ARRAY_BAG_FACTORY, ARRAY_PARAMS, ARRAY_REQUIRED);
    /// `array:queue` (P5d).
    pub const ArrayQueue: Family = Family::new(
        &ARRAY_QUEUE,
        ARRAY_QUEUE_FACTORY,
        ARRAY_PARAMS,
        ARRAY_REQUIRED,
    );
    /// `array:stack` (P5d).
    pub const ArrayStack: Family = Family::new(
        &ARRAY_STACK,
        ARRAY_STACK_FACTORY,
        ARRAY_PARAMS,
        ARRAY_REQUIRED,
    );
    /// `array:set` (P5d).
    pub const ArraySet: Family =
        Family::new(&ARRAY_SET, ARRAY_SET_FACTORY, ARRAY_PARAMS, ARRAY_REQUIRED);
    /// `array:cell` (P5d).
    pub const ArrayCell: Family = Family::new(
        &ARRAY_CELL,
        ARRAY_CELL_FACTORY,
        ARRAY_PARAMS,
        ARRAY_REQUIRED,
    );
    /// `array:priorityqueue` (P5d).
    pub const ArrayPriorityQueue: Family = Family::new(
        &ARRAY_PRIORITY_QUEUE,
        ARRAY_PRIORITY_QUEUE_FACTORY,
        ARRAY_PRIORITY_QUEUE_PARAMS,
        ARRAY_REQUIRED,
    );
    /// `vector:bag` (P5d).
    pub const VectorBag: Family =
        Family::new(&VECTOR_BAG, VECTOR_BAG_FACTORY, NO_PARAMS, NO_PARAMS);
    /// `vector:queue` (P5d).
    pub const VectorQueue: Family =
        Family::new(&VECTOR_QUEUE, VECTOR_QUEUE_FACTORY, NO_PARAMS, NO_PARAMS);
    /// `vector:stack` (P5d).
    pub const VectorStack: Family =
        Family::new(&VECTOR_STACK, VECTOR_STACK_FACTORY, NO_PARAMS, NO_PARAMS);
    /// `vector:set` (P5d).
    pub const VectorSet: Family =
        Family::new(&VECTOR_SET, VECTOR_SET_FACTORY, NO_PARAMS, NO_PARAMS);
    /// `vector:cell` (P5d).
    pub const VectorCell: Family =
        Family::new(&VECTOR_CELL, VECTOR_CELL_FACTORY, NO_PARAMS, NO_PARAMS);
    /// `vector:priorityqueue` (P5d).
    pub const VectorPriorityQueue: Family = Family::new(
        &VECTOR_PRIORITY_QUEUE,
        VECTOR_PRIORITY_QUEUE_FACTORY,
        PRIORITY_QUEUE_PARAMS,
        NO_PARAMS,
    );
    /// `pathmap:bag` (P5e).
    pub const PathMapBag: Family =
        Family::new(&PATHMAP_BAG, PATHMAP_BAG_FACTORY, NO_PARAMS, NO_PARAMS);
    /// `pathmap:queue` (P5e).
    pub const PathMapQueue: Family =
        Family::new(&PATHMAP_QUEUE, PATHMAP_QUEUE_FACTORY, NO_PARAMS, NO_PARAMS);
    /// `pathmap:stack` (P5e).
    pub const PathMapStack: Family =
        Family::new(&PATHMAP_STACK, PATHMAP_STACK_FACTORY, NO_PARAMS, NO_PARAMS);
    /// `pathmap:set` (P5e).
    pub const PathMapSet: Family =
        Family::new(&PATHMAP_SET, PATHMAP_SET_FACTORY, NO_PARAMS, NO_PARAMS);
    /// `pathmap:cell` (P5e).
    pub const PathMapCell: Family =
        Family::new(&PATHMAP_CELL, PATHMAP_CELL_FACTORY, NO_PARAMS, NO_PARAMS);
    /// `pathmap:priorityqueue` (P5e).
    pub const PathMapPriorityQueue: Family = Family::new(
        &PATHMAP_PRIORITY_QUEUE,
        PATHMAP_PRIORITY_QUEUE_FACTORY,
        PRIORITY_QUEUE_PARAMS,
        NO_PARAMS,
    );
    /// `hashmap:vectordb` (P6).
    pub const HashMapVectorDb: Family = Family::new(
        &HASHMAP_VECTORDB,
        HASHMAP_VECTORDB_FACTORY,
        VECTORDB_PARAMS,
        VECTORDB_REQUIRED,
    );
    /// `vector:vectordb` (P6).
    pub const VectorVectorDb: Family = Family::new(
        &VECTOR_VECTORDB,
        VECTOR_VECTORDB_FACTORY,
        VECTORDB_PARAMS,
        VECTORDB_REQUIRED,
    );
    /// `hashset:bag` (P7).
    pub const HashSetBag: Family =
        Family::new(&HASHSET_BAG, HASHSET_BAG_FACTORY, NO_PARAMS, NO_PARAMS);
    /// `hashset:queue` (P7).
    pub const HashSetQueue: Family =
        Family::new(&HASHSET_QUEUE, HASHSET_QUEUE_FACTORY, NO_PARAMS, NO_PARAMS);
    /// `hashset:stack` (P7).
    pub const HashSetStack: Family =
        Family::new(&HASHSET_STACK, HASHSET_STACK_FACTORY, NO_PARAMS, NO_PARAMS);
    /// `hashset:set` (P7).
    pub const HashSetSet: Family =
        Family::new(&HASHSET_SET, HASHSET_SET_FACTORY, NO_PARAMS, NO_PARAMS);
    /// `hashset:cell` (P7).
    pub const HashSetCell: Family =
        Family::new(&HASHSET_CELL, HASHSET_CELL_FACTORY, NO_PARAMS, NO_PARAMS);
    /// `hashset:priorityqueue` (P7).
    pub const HashSetPriorityQueue: Family = Family::new(
        &HASHSET_PRIORITY_QUEUE,
        HASHSET_PRIORITY_QUEUE_FACTORY,
        PRIORITY_QUEUE_PARAMS,
        NO_PARAMS,
    );

    /// The wired families, in dispatch order: `hashmap:queue` first, then ascending family index,
    /// then the PathMap families of P5e, the VectorDB families of P6, and the HashSet families of
    /// P7 last. So the dispatch of each family that a later phase found wired makes the comparisons
    /// that it made before.
    pub const WIRED: [Family; 32] = [
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
        Family::HashSetBag,
        Family::HashSetQueue,
        Family::HashSetStack,
        Family::HashSetSet,
        Family::HashSetCell,
        Family::HashSetPriorityQueue,
    ];

    /// Added by decision 127 (kind registry): the family of `agent` with the factory `factory`,
    /// the constructor schema `constructor` and the required params `required`. The names, the
    /// send and query params, the qualifiers, the param rules and the join window come from `S`
    /// and `D`.
    pub const fn new<S: OuterStore, D: Discipline>(
        agent: &'static GenericSpace<S, D>,
        factory: SystemChannel,
        constructor: &'static [&'static str],
        required: &'static [&'static str],
    ) -> Family {
        Family {
            urn: SpaceFactoryUrn {
                outer: SpaceOuter::named(S::NAME),
                inner: SpaceInner::named(D::NAME),
            },
            factory,
            agent,
            send_params: D::SEND_PARAMS,
            query_params: D::QUERY_PARAMS,
            constructor,
            required,
            qualifiers: S::QUALIFIERS,
            param_rule: family_param_rule::<S, D>,
            // Added by decision 128 (kind routing): the window of the discipline.
            window: D::JOIN_WINDOW,
        }
    }

    /// The family of the factory URN `urn`, or `None` when it is not wired.
    // Changed by decision 127 (kind registry, ruling Q5): the registry holds the families, a
    // family holds its fields, and a lookup returns a reference to the registered family, so no
    // operation copies a family. The text before:
    // ```
    // pub fn of_urn(urn: SpaceFactoryUrn) -> Option<Family> {
    //     match (urn.outer, urn.inner) {
    //         (SpaceOuter::HashMap, SpaceInner::Queue) => Some(Family::HashMapQueue),
    //         (SpaceOuter::HashMap, SpaceInner::Bag) => Some(Family::HashMapBag),
    //         (SpaceOuter::HashMap, SpaceInner::Stack) => Some(Family::HashMapStack),
    //         (SpaceOuter::HashMap, SpaceInner::Set) => Some(Family::HashMapSet),
    //         (SpaceOuter::HashMap, SpaceInner::Cell) => Some(Family::HashMapCell),
    //         (SpaceOuter::HashMap, SpaceInner::PriorityQueue) => Some(Family::HashMapPriorityQueue),
    //         (SpaceOuter::Array, SpaceInner::Bag) => Some(Family::ArrayBag),
    //         (SpaceOuter::Array, SpaceInner::Queue) => Some(Family::ArrayQueue),
    //         (SpaceOuter::Array, SpaceInner::Stack) => Some(Family::ArrayStack),
    //         (SpaceOuter::Array, SpaceInner::Set) => Some(Family::ArraySet),
    //         (SpaceOuter::Array, SpaceInner::Cell) => Some(Family::ArrayCell),
    //         (SpaceOuter::Array, SpaceInner::PriorityQueue) => Some(Family::ArrayPriorityQueue),
    //         (SpaceOuter::Vector, SpaceInner::Bag) => Some(Family::VectorBag),
    //         (SpaceOuter::Vector, SpaceInner::Queue) => Some(Family::VectorQueue),
    //         (SpaceOuter::Vector, SpaceInner::Stack) => Some(Family::VectorStack),
    //         (SpaceOuter::Vector, SpaceInner::Set) => Some(Family::VectorSet),
    //         (SpaceOuter::Vector, SpaceInner::Cell) => Some(Family::VectorCell),
    //         (SpaceOuter::Vector, SpaceInner::PriorityQueue) => Some(Family::VectorPriorityQueue),
    //         (SpaceOuter::PathMap, SpaceInner::Bag) => Some(Family::PathMapBag),
    //         (SpaceOuter::PathMap, SpaceInner::Queue) => Some(Family::PathMapQueue),
    //         (SpaceOuter::PathMap, SpaceInner::Stack) => Some(Family::PathMapStack),
    //         (SpaceOuter::PathMap, SpaceInner::Set) => Some(Family::PathMapSet),
    //         (SpaceOuter::PathMap, SpaceInner::Cell) => Some(Family::PathMapCell),
    //         (SpaceOuter::PathMap, SpaceInner::PriorityQueue) => Some(Family::PathMapPriorityQueue),
    //         (SpaceOuter::HashMap, SpaceInner::VectorDb) => Some(Family::HashMapVectorDb),
    //         (SpaceOuter::Vector, SpaceInner::VectorDb) => Some(Family::VectorVectorDb),
    //         (SpaceOuter::HashSet, SpaceInner::Bag) => Some(Family::HashSetBag),
    //         (SpaceOuter::HashSet, SpaceInner::Queue) => Some(Family::HashSetQueue),
    //         (SpaceOuter::HashSet, SpaceInner::Stack) => Some(Family::HashSetStack),
    //         (SpaceOuter::HashSet, SpaceInner::Set) => Some(Family::HashSetSet),
    //         (SpaceOuter::HashSet, SpaceInner::Cell) => Some(Family::HashSetCell),
    //         (SpaceOuter::HashSet, SpaceInner::PriorityQueue) => Some(Family::HashSetPriorityQueue),
    //         // Changed by P7 step 3 (design v3 HS-7): the six HashSet families are wired. Every
    //         // pair that remains is not a family: the PathMap, Array and HashSet stores hold no
    //         // VectorDB collection (`SpaceFactoryUrn::is_supported_pair`).
    //         // // Every inner collection of the HashMap and Vector stores is wired since P6. The
    //         // // PathMap and Array stores have no VectorDB family (design v3 4.2), and HashSet is
    //         // // wired in P7 (F1).
    //         // (SpaceOuter::PathMap | SpaceOuter::Array | SpaceOuter::HashSet, _) => None,
    //         (
    //             SpaceOuter::PathMap | SpaceOuter::Array | SpaceOuter::HashSet,
    //             SpaceInner::VectorDb,
    //         ) => None,
    //     }
    // }
    // ```
    pub fn of_urn(urn: SpaceFactoryUrn) -> Option<&'static Family> {
        Family::registered().find(|family| family.urn == urn)
    }

    // Changed by decision 127 (kind registry): a lookup reads the registry. The text before:
    // ```
    // /// The family that `config` names, or `None` when it is not wired. Each arm compares the
    // /// config names with its family.
    // ```
    /// The family that `config` names, or `None` when it is not registered.
    // Changed by decision 127 (kind registry, ruling Q5): the registry holds the families, a
    // family holds its fields, and a lookup returns a reference to the registered family, so no
    // operation copies a family. The text before:
    // ```
    // pub fn of_config(config: &SpaceConfig) -> Option<Family> {
    //     Family::WIRED
    //         .into_iter()
    //         .find(|family| family.is_named_by(config))
    // }
    // ```
    pub fn of_config(config: &SpaceConfig) -> Option<&'static Family> {
        Family::registered().find(|family| family.is_named_by(config))
    }

    /// True when `config` names this family.
    // Changed by decision 127 (kind registry, ruling Q5): the registry holds the families, a
    // family holds its fields, and a lookup returns a reference to the registered family, so no
    // operation copies a family. The text before:
    // ```
    // fn is_named_by(self, config: &SpaceConfig) -> bool {
    //     match self {
    //         Family::HashMapQueue => family_matches::<HashMapOuter, QueueDiscipline>(config),
    //         Family::HashMapBag => family_matches::<HashMapOuter, BagDiscipline>(config),
    //         Family::HashMapStack => family_matches::<HashMapOuter, StackDiscipline>(config),
    //         Family::HashMapSet => family_matches::<HashMapOuter, SetDiscipline>(config),
    //         Family::HashMapCell => family_matches::<HashMapOuter, CellDiscipline>(config),
    //         Family::HashMapPriorityQueue => {
    //             family_matches::<HashMapOuter, PriorityQueueDiscipline>(config)
    //         }
    //         Family::ArrayBag => family_matches::<ArrayOuter, BagDiscipline>(config),
    //         Family::ArrayQueue => family_matches::<ArrayOuter, QueueDiscipline>(config),
    //         Family::ArrayStack => family_matches::<ArrayOuter, StackDiscipline>(config),
    //         Family::ArraySet => family_matches::<ArrayOuter, SetDiscipline>(config),
    //         Family::ArrayCell => family_matches::<ArrayOuter, CellDiscipline>(config),
    //         Family::ArrayPriorityQueue => {
    //             family_matches::<ArrayOuter, PriorityQueueDiscipline>(config)
    //         }
    //         Family::VectorBag => family_matches::<VectorOuter, BagDiscipline>(config),
    //         Family::VectorQueue => family_matches::<VectorOuter, QueueDiscipline>(config),
    //         Family::VectorStack => family_matches::<VectorOuter, StackDiscipline>(config),
    //         Family::VectorSet => family_matches::<VectorOuter, SetDiscipline>(config),
    //         Family::VectorCell => family_matches::<VectorOuter, CellDiscipline>(config),
    //         Family::VectorPriorityQueue => {
    //             family_matches::<VectorOuter, PriorityQueueDiscipline>(config)
    //         }
    //         Family::PathMapBag => family_matches::<PathMapOuter, BagDiscipline>(config),
    //         Family::PathMapQueue => family_matches::<PathMapOuter, QueueDiscipline>(config),
    //         Family::PathMapStack => family_matches::<PathMapOuter, StackDiscipline>(config),
    //         Family::PathMapSet => family_matches::<PathMapOuter, SetDiscipline>(config),
    //         Family::PathMapCell => family_matches::<PathMapOuter, CellDiscipline>(config),
    //         Family::PathMapPriorityQueue => {
    //             family_matches::<PathMapOuter, PriorityQueueDiscipline>(config)
    //         }
    //         Family::HashMapVectorDb => family_matches::<HashMapOuter, VectorDbDiscipline>(config),
    //         Family::VectorVectorDb => family_matches::<VectorOuter, VectorDbDiscipline>(config),
    //         Family::HashSetBag => family_matches::<HashSetOuter, BagDiscipline>(config),
    //         Family::HashSetQueue => family_matches::<HashSetOuter, QueueDiscipline>(config),
    //         Family::HashSetStack => family_matches::<HashSetOuter, StackDiscipline>(config),
    //         Family::HashSetSet => family_matches::<HashSetOuter, SetDiscipline>(config),
    //         Family::HashSetCell => family_matches::<HashSetOuter, CellDiscipline>(config),
    //         Family::HashSetPriorityQueue => {
    //             family_matches::<HashSetOuter, PriorityQueueDiscipline>(config)
    //         }
    //     }
    // }
    // ```
    fn is_named_by(&self, config: &SpaceConfig) -> bool {
        config.is_family(self.urn.outer.as_str(), self.urn.inner.as_str())
    }

    /// The agent of this family.
    // Changed by decision 127 (kind registry, ruling Q5): the registry holds the families, a
    // family holds its fields, and a lookup returns a reference to the registered family, so no
    // operation copies a family. The text before:
    // ```
    // pub fn agent(self) -> &'static dyn FamilySpace {
    //     match self {
    //         Family::HashMapQueue => &HASHMAP_QUEUE,
    //         Family::HashMapBag => &HASHMAP_BAG,
    //         Family::HashMapStack => &HASHMAP_STACK,
    //         Family::HashMapSet => &HASHMAP_SET,
    //         Family::HashMapCell => &HASHMAP_CELL,
    //         Family::HashMapPriorityQueue => &HASHMAP_PRIORITY_QUEUE,
    //         Family::ArrayBag => &ARRAY_BAG,
    //         Family::ArrayQueue => &ARRAY_QUEUE,
    //         Family::ArrayStack => &ARRAY_STACK,
    //         Family::ArraySet => &ARRAY_SET,
    //         Family::ArrayCell => &ARRAY_CELL,
    //         Family::ArrayPriorityQueue => &ARRAY_PRIORITY_QUEUE,
    //         Family::VectorBag => &VECTOR_BAG,
    //         Family::VectorQueue => &VECTOR_QUEUE,
    //         Family::VectorStack => &VECTOR_STACK,
    //         Family::VectorSet => &VECTOR_SET,
    //         Family::VectorCell => &VECTOR_CELL,
    //         Family::VectorPriorityQueue => &VECTOR_PRIORITY_QUEUE,
    //         Family::PathMapBag => &PATHMAP_BAG,
    //         Family::PathMapQueue => &PATHMAP_QUEUE,
    //         Family::PathMapStack => &PATHMAP_STACK,
    //         Family::PathMapSet => &PATHMAP_SET,
    //         Family::PathMapCell => &PATHMAP_CELL,
    //         Family::PathMapPriorityQueue => &PATHMAP_PRIORITY_QUEUE,
    //         Family::HashMapVectorDb => &HASHMAP_VECTORDB,
    //         Family::VectorVectorDb => &VECTOR_VECTORDB,
    //         Family::HashSetBag => &HASHSET_BAG,
    //         Family::HashSetQueue => &HASHSET_QUEUE,
    //         Family::HashSetStack => &HASHSET_STACK,
    //         Family::HashSetSet => &HASHSET_SET,
    //         Family::HashSetCell => &HASHSET_CELL,
    //         Family::HashSetPriorityQueue => &HASHSET_PRIORITY_QUEUE,
    //     }
    // }
    // ```
    pub fn agent(&self) -> &'static dyn FamilySpace { self.agent }

    /// The named params of a send to a collection of this family: the schema of its discipline.
    // Changed by decision 127 (kind registry, ruling Q5): the registry holds the families, a
    // family holds its fields, and a lookup returns a reference to the registered family, so no
    // operation copies a family. The text before:
    // ```
    // pub fn send_params(self) -> &'static [&'static str] {
    //     match self {
    //         Family::HashMapQueue
    //         | Family::ArrayQueue
    //         | Family::VectorQueue
    //         | Family::PathMapQueue
    //         | Family::HashSetQueue => QueueDiscipline::SEND_PARAMS,
    //         Family::HashMapBag
    //         | Family::ArrayBag
    //         | Family::VectorBag
    //         | Family::PathMapBag
    //         | Family::HashSetBag => BagDiscipline::SEND_PARAMS,
    //         Family::HashMapStack
    //         | Family::ArrayStack
    //         | Family::VectorStack
    //         | Family::PathMapStack
    //         | Family::HashSetStack => StackDiscipline::SEND_PARAMS,
    //         Family::HashMapCell
    //         | Family::ArrayCell
    //         | Family::VectorCell
    //         | Family::PathMapCell
    //         | Family::HashSetCell => CellDiscipline::SEND_PARAMS,
    //         Family::HashMapSet
    //         | Family::ArraySet
    //         | Family::VectorSet
    //         | Family::PathMapSet
    //         | Family::HashSetSet => SetDiscipline::SEND_PARAMS,
    //         Family::HashMapPriorityQueue
    //         | Family::ArrayPriorityQueue
    //         | Family::VectorPriorityQueue
    //         | Family::PathMapPriorityQueue
    //         | Family::HashSetPriorityQueue => PriorityQueueDiscipline::SEND_PARAMS,
    //         Family::HashMapVectorDb | Family::VectorVectorDb => VectorDbDiscipline::SEND_PARAMS,
    //     }
    // }
    // ```
    pub fn send_params(&self) -> &'static [&'static str] { self.send_params }

    /// The named query params of a receive on a collection of this family.
    // Changed by decision 127 (kind registry, ruling Q5): the registry holds the families, a
    // family holds its fields, and a lookup returns a reference to the registered family, so no
    // operation copies a family. The text before:
    // ```
    // pub fn query_params(self) -> &'static [&'static str] {
    //     match self {
    //         Family::HashMapQueue
    //         | Family::ArrayQueue
    //         | Family::VectorQueue
    //         | Family::PathMapQueue
    //         | Family::HashSetQueue => QueueDiscipline::QUERY_PARAMS,
    //         Family::HashMapBag
    //         | Family::ArrayBag
    //         | Family::VectorBag
    //         | Family::PathMapBag
    //         | Family::HashSetBag => BagDiscipline::QUERY_PARAMS,
    //         Family::HashMapStack
    //         | Family::ArrayStack
    //         | Family::VectorStack
    //         | Family::PathMapStack
    //         | Family::HashSetStack => StackDiscipline::QUERY_PARAMS,
    //         Family::HashMapCell
    //         | Family::ArrayCell
    //         | Family::VectorCell
    //         | Family::PathMapCell
    //         | Family::HashSetCell => CellDiscipline::QUERY_PARAMS,
    //         Family::HashMapSet
    //         | Family::ArraySet
    //         | Family::VectorSet
    //         | Family::PathMapSet
    //         | Family::HashSetSet => SetDiscipline::QUERY_PARAMS,
    //         Family::HashMapPriorityQueue
    //         | Family::ArrayPriorityQueue
    //         | Family::VectorPriorityQueue
    //         | Family::PathMapPriorityQueue
    //         | Family::HashSetPriorityQueue => PriorityQueueDiscipline::QUERY_PARAMS,
    //         Family::HashMapVectorDb | Family::VectorVectorDb => VectorDbDiscipline::QUERY_PARAMS,
    //     }
    // }
    // ```
    pub fn query_params(&self) -> &'static [&'static str] { self.query_params }

    /// Added by decision 127 (kind registry): the URN of this family.
    pub fn urn(&self) -> SpaceFactoryUrn { self.urn }

    /// Added by decision 127 (kind registry): the factory system channel of this family.
    pub fn factory(&self) -> SystemChannel { self.factory }

    /// Added by decision 127 (kind registry): the named params of the factory of this family, in
    /// bytewise order.
    pub fn constructor_schema(&self) -> &'static [&'static str] { self.constructor }

    /// Added by decision 127 (kind registry): the named params that the factory of this family
    /// requires.
    pub fn required_params(&self) -> &'static [&'static str] { self.required }

    /// Added by decision 127 (kind registry): true when a space of this family can have the
    /// qualifier `qualifier`.
    pub fn supports_qualifier(&self, qualifier: SpaceQualifier) -> bool {
        self.qualifiers.contains(&qualifier)
    }

    /// Added by decision 127 (kind registry): the value rule of the constructor param `key` that
    /// this family brings, or `None` when the built-in table holds the rule of the param.
    pub fn param_rule(&self, key: &str) -> Option<ParamCheck> { (self.param_rule)(key) }

    /// Added by decision 128 (kind routing): the join window of the discipline of the family.
    pub fn window(&self) -> JoinWindow { self.window }

    /// Added by decision 127 (kind registry, ruling D3): the built-in families in the order of
    /// `WIRED`, then the families of the registered kinds.
    pub fn registered() -> impl Iterator<Item = &'static Family> {
        BUILT_IN_FAMILIES.iter().chain(kind_families())
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

/// Added by decision 127 (kind registry): the value rule of the constructor param `key` of the
/// family of `S` and `D`: the rule of the discipline, then the rule of the store.
fn family_param_rule<S: OuterStore, D: Discipline>(key: &str) -> Option<ParamCheck> {
    D::param_rule(key).or_else(|| S::param_rule(key))
}

/// Added by decision 127 (kind registry): a kind of reified space: its families, its system
/// processes and the installer of its external services. A kind registers itself with one
/// `#[distributed_slice(SPACE_KINDS)]` static, as an FS handler registers in `FS_HANDLERS`.
pub struct SpaceKind {
    /// The families of the kind.
    pub families: &'static [Family],
    /// The system processes of the kind.
    pub processes: &'static [SpaceProcess],
    /// The installer of the external services of the kind. It takes `is_validator`: a
    /// validator installs the services of the build, and an observer installs the unavailable
    /// services (decision 109).
    pub services: Option<fn(bool)>,
}

/// Added by decision 127 (kind registry): the registered kinds. The built-in families are not
/// here (`BUILT_IN_FAMILIES`), so in production the slice holds the VectorDB services alone.
#[distributed_slice]
pub static SPACE_KINDS: [SpaceKind] = [..];

/// Added by decision 127 (kind registry, ruling D3): the built-in families, in the order of
/// `Family::WIRED`. They stay out of `SPACE_KINDS`, so the linker cannot drop them and the op
/// predicates never scan them.
static BUILT_IN_FAMILIES: &[Family] = &Family::WIRED;

/// Added by decision 127 (kind registry, ruling D3): the families of the registered kinds.
pub fn kind_families() -> impl Iterator<Item = &'static Family> {
    SPACE_KINDS.iter().flat_map(|kind| kind.families.iter())
}

/// Added by decision 127 (kind registry): the system processes of the registered kinds.
pub fn kind_processes() -> impl Iterator<Item = &'static SpaceProcess> {
    SPACE_KINDS.iter().flat_map(|kind| kind.processes.iter())
}

/// Added by decision 127 (kind registry): installs the external services of each registered
/// kind, once, at node start (decision 109).
pub fn install_space_services(is_validator: bool) {
    for install in SPACE_KINDS.iter().filter_map(|kind| kind.services) {
        install(is_validator);
    }
}

/// Added by decision 128 (kind routing): true for a key that the query map of a parked waiter
/// may hold: the query vector `q` or a query param of a registered family with the similarity
/// window. Without a plug-in these are the 8 keys of the VectorDB families.
pub fn is_policy_key(key: &str) -> bool {
    key == QUERY_VECTOR_KEY
        || Family::registered().any(|family| match family.window() {
            JoinWindow::Similarity => family.query_params().contains(&key),
            _ => false,
        })
}

/// Added by decision 127 (kind registry, ruling Q2): the check of the registry, in the form of
/// the base check of `CONSENSUS_FOLD`. It sorts the entries, checks them and names each broken
/// entry. The URNs of all system channels are unique. The ops of the kind channels are unique
/// and free (ruling D4), so their body refs are unique too. Each family has the factory of its
/// URN. The node runs it once at start (`setup_node_program`), and a broken registry stops the
/// start.
pub fn check_space_kinds() -> Result<(), String> {
    let mut channels: Vec<SystemChannel> = kind_families()
        .map(Family::factory)
        .chain(kind_processes().map(|process| process.channel))
        .collect();
    channels.sort_by_key(|channel| channel.op);
    let mut urns: Vec<&'static str> = P4_SYSTEM_CHANNELS
        .iter()
        .chain(&P5_SYSTEM_CHANNELS)
        .chain(&P6_SYSTEM_CHANNELS)
        .chain(&P6_PROCESSES)
        .chain(&P7_SYSTEM_CHANNELS)
        .chain(&P8_PROCESSES)
        .chain(&channels)
        .map(|channel| channel.urn)
        .collect();
    urns.sort_unstable();
    let mut broken = Vec::<String>::new();
    broken.extend(Family::registered().filter_map(|family| {
        let urn = family.urn().to_string();
        match urn == family.factory().urn {
            true => None,
            false => Some(format!(
                "the family {urn} has the factory {}",
                family.factory().urn
            )),
        }
    }));
    broken.extend(
        channels
            .iter()
            .filter_map(|channel| match is_kind_op(channel.op) {
                true => None,
                false => Some(format!(
                    "{} takes the reserved op {:#04x}",
                    channel.urn, channel.op
                )),
            }),
    );
    broken.extend(channels.windows(2).filter_map(|pair| match pair {
        [first, second] if first.op == second.op => Some(format!(
            "{} and {} share the op {:#04x}",
            first.urn, second.urn, first.op
        )),
        _ => None,
    }));
    broken.extend(urns.windows(2).filter_map(|pair| match pair {
        [first, second] if first == second => Some(format!("two channels have the URN {first}")),
        _ => None,
    }));
    match broken.is_empty() {
        true => Ok(()),
        false => Err(broken.join(", ")),
    }
}

/// Added by decision 128 (kind routing): the outer store of phase A of every keyed operation.
/// It has the routed layout of A5 and the random-tail names of P4, which are the defaults of
/// `OuterStore`. Phase A locks the channels of the routed key before the config names the family
/// (decision 94), so it cannot use the store of the family. No family has this store, so no
/// config names it, and nothing reads its name.
#[derive(Clone, Copy, Debug, Default)]
struct RoutedLayout;

impl OuterStore for RoutedLayout {
    const NAME: &'static str = "routed";
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
    match Family::of_config(&opened.config) {
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
        // Changed by decision 128 (kind routing): phase A opens the routed layout. The text before:
        // let opened = begin_collection(&HashMapOuter, env, op.space, &op.key, &op.channel).await?;
        let opened = begin_collection(&RoutedLayout, env, op.space, &op.key, &op.channel).await?;
        dispatch(env, opened, SpaceOp::Produce(op)).await
    }

    async fn prepare_consume(
        &self,
        env: &SpaceEnv,
        op: ConsumeOp,
    ) -> Result<Prepared<SpaceOutcome>, SpaceError> {
        // Changed by decision 128 (kind routing): phase A opens the routed layout. The text before:
        // let opened = begin_collection(&HashMapOuter, env, op.space, &op.key, &op.channel).await?;
        let opened = begin_collection(&RoutedLayout, env, op.space, &op.key, &op.channel).await?;
        dispatch(env, opened, SpaceOp::Consume(op)).await
    }

    async fn prepare_settle(
        &self,
        env: &SpaceEnv,
        op: SettleOp,
    ) -> Result<Prepared<SpaceOutcome>, SpaceError> {
        // Changed by decision 128 (kind routing): phase A opens the routed layout. The text before:
        // let opened = begin_collection(&HashMapOuter, env, op.space, &op.key, &op.channel).await?;
        let opened = begin_collection(&RoutedLayout, env, op.space, &op.key, &op.channel).await?;
        dispatch(env, opened, SpaceOp::Settle(op)).await
    }

    // Changed by decision 128 (kind routing): phase A opens the routed layout. The text before:
    // /// The join locks the collections of its binds in the HashMap store, as every keyed
    /// The join locks the collections of its binds in the routed layout, as every keyed
    /// operation does (A5).
    async fn prepare_join(
        &self,
        env: &SpaceEnv,
        op: JoinOp,
    ) -> Result<Prepared<SpaceOutcome>, SpaceError> {
        // Changed by decision 128 (kind routing): phase A opens the routed layout. The text before:
        // let opened = begin_join(&HashMapOuter, env, &op).await?;
        let opened = begin_join(&RoutedLayout, env, &op).await?;
        dispatch(env, opened, SpaceOp::Join(op)).await
    }

    async fn allocate(&self, env: &SpaceEnv, op: AllocOp) -> Result<Allocation, SpaceError> {
        let opened = begin_allocation(env, op.space).await?;
        match Family::of_config(&opened.config) {
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
        // Changed by decision 128 (kind routing): phase A opens the routed layout. The text before:
        // HashMapOuter.gensym(space, random)
        RoutedLayout.gensym(space, random)
    }
}

#[cfg(test)]
mod tests {

    // Added by decision 128 (kind routing): the tests of the routed layout and of the window
    // steps.
    use std::collections::BTreeMap;

    use models::rust::utils::new_gstring_par;

    use super::*;
    // Changed by decision 128 (kind routing): the routed layout test names a durable space. The
    // text before:
    // use crate::rust::interpreter::spaces::namespace::SID_LEN;
    use crate::rust::interpreter::spaces::namespace::{Durability, SID_LEN};
    use crate::rust::interpreter::spaces::record::WaiterPolicy;
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
    const WIRED_NAMES: [(Family, &str, &str); 32] = [
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
        (Family::HashSetBag, "hashset", "bag"),
        (Family::HashSetQueue, "hashset", "queue"),
        (Family::HashSetStack, "hashset", "stack"),
        (Family::HashSetSet, "hashset", "set"),
        (Family::HashSetCell, "hashset", "cell"),
        (Family::HashSetPriorityQueue, "hashset", "priorityqueue"),
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
            // Changed by decision 127 (kind registry, ruling Q5): a lookup returns a reference. The
            // text before:
            // ```
            // assert_eq!(Family::of_urn(urn), family, "{urn}");
            // assert_eq!(send_params(urn), family.map(Family::send_params), "{urn}");
            // assert_eq!(query_params(urn), family.map(Family::query_params), "{urn}");
            // ```
            assert_eq!(Family::of_urn(urn).copied(), family, "{urn}");
            assert_eq!(
                send_params(urn),
                family.as_ref().map(Family::send_params),
                "{urn}"
            );
            assert_eq!(
                query_params(urn),
                family.as_ref().map(Family::query_params),
                "{urn}"
            );
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
            (
                Family::HashSetBag,
                BagDiscipline::SEND_PARAMS,
                BagDiscipline::QUERY_PARAMS,
            ),
            (
                Family::HashSetQueue,
                QueueDiscipline::SEND_PARAMS,
                QueueDiscipline::QUERY_PARAMS,
            ),
            (
                Family::HashSetStack,
                StackDiscipline::SEND_PARAMS,
                StackDiscipline::QUERY_PARAMS,
            ),
            (
                Family::HashSetSet,
                SetDiscipline::SEND_PARAMS,
                SetDiscipline::QUERY_PARAMS,
            ),
            (
                Family::HashSetCell,
                CellDiscipline::SEND_PARAMS,
                CellDiscipline::QUERY_PARAMS,
            ),
            (
                Family::HashSetPriorityQueue,
                PriorityQueueDiscipline::SEND_PARAMS,
                PriorityQueueDiscipline::QUERY_PARAMS,
            ),
        ] {
            assert_eq!(family.send_params(), send, "{family:?}");
            assert_eq!(family.query_params(), query, "{family:?}");
        }
    }

    #[test]
    fn space_families_dispatch_by_config() {
        // Changed by P7 step 3 (design v3 HS-7): `hashset:set` is wired. The unsupported pair
        // `hashset:vectordb` takes its place.
        // let unwired = [
        //     ("hashmap", "deque"),
        //     ("pathmap", "vectordb"),
        //     ("array", "vectordb"),
        //     ("hashset", "set"),
        // ];
        let unwired = [
            ("hashmap", "deque"),
            ("pathmap", "vectordb"),
            ("array", "vectordb"),
            ("hashset", "vectordb"),
        ];
        let mut cases = Vec::with_capacity(WIRED_NAMES.len() + unwired.len());
        for (family, outer, inner) in WIRED_NAMES {
            cases.push((outer, inner, Some(family)));
        }
        for (outer, inner) in unwired {
            cases.push((outer, inner, None));
        }
        for (outer, inner, expected) in cases {
            assert_eq!(
                // Changed by decision 127 (kind registry, ruling Q5): a lookup returns a reference.
                // The text before:
                // Family::of_config(&config(outer, inner)),
                Family::of_config(&config(outer, inner)).copied(),
                expected,
                "{outer}:{inner}"
            );
        }
    }

    /// Added by decision 127 (kind registry, rulings D2 and D3): without a plug-in, the slice
    /// holds the VectorDB services alone, the registry is the wired families in order, the
    /// registry passes its check, and the registered URNs, ops and body refs are unique.
    #[test]
    fn space_kind_registry_holds_the_wired_families() {
        assert_eq!(SPACE_KINDS.len(), 1);
        assert_eq!(
            Family::registered().copied().collect::<Vec<_>>(),
            Family::WIRED.to_vec()
        );
        assert_eq!((kind_families().count(), kind_processes().count()), (0, 0));
        assert_eq!(check_space_kinds(), Ok(()));
        let mut urns = std::collections::BTreeSet::new();
        let mut ops = std::collections::BTreeSet::new();
        let mut bodies = std::collections::BTreeSet::new();
        let channels = Family::registered()
            .map(Family::factory)
            .chain(kind_processes().map(|process| process.channel));
        for channel in channels {
            assert!(urns.insert(channel.urn), "{}", channel.urn);
            assert!(ops.insert(channel.op), "{}", channel.urn);
            assert!(bodies.insert(channel.body_ref()), "{}", channel.urn);
        }
        for family in Family::registered() {
            assert_eq!(family.factory().urn, family.urn().to_string(), "{family:?}");
            assert_eq!(
                family.urn().family_index(),
                Some(family.factory().op),
                "{family:?}"
            );
            assert_eq!(
                matches!(family.window(), JoinWindow::Lanes),
                family.urn().inner.as_str() == "priorityqueue",
                "{family:?}"
            );
            assert_eq!(
                matches!(family.window(), JoinWindow::Similarity),
                family.urn().inner.as_str() == "vectordb",
                "{family:?}"
            );
            for qualifier in models::rust::space_factory_urn::SpaceQualifier::ALL {
                assert_eq!(
                    family.supports_qualifier(qualifier),
                    family.urn().supports_qualifier(qualifier),
                    "{family:?} {qualifier}"
                );
            }
            for key in family.constructor_schema() {
                assert!(family.param_rule(key).is_none(), "{family:?} {key}");
            }
        }
    }

    /// Added by decision 128 (kind routing): the policy keys of the registry are the 8 keys that
    /// `record::VDB_QUERY_KEYS` listed.
    #[test]
    fn space_policy_keys_are_the_query_keys_of_the_vectordb_families() {
        const VDB_QUERY_KEYS: [&str; 8] = [
            "ids",
            "k",
            "metric",
            "q",
            "radius",
            "results",
            "scores",
            "threshold",
        ];
        let mut keys: Vec<&str> = Family::registered()
            .flat_map(|family| family.query_params().iter().copied())
            .collect();
        keys.extend([
            "q",
            "priority",
            "score",
            "tensors",
            "precision",
            "embedding",
            "",
        ]);
        for key in keys {
            assert_eq!(is_policy_key(key), VDB_QUERY_KEYS.contains(&key), "{key}");
        }
    }

    /// Added by decision 128 (kind routing): phase A and the HashMap store keep one layout. The
    /// unit tests of the HashMap store pin the bytes of that layout.
    #[test]
    fn space_family_agent_opens_the_routed_layout() {
        let space = NativeSpace {
            sid: [5; SID_LEN],
            durability: Durability::Durable,
        };
        let channel = new_gstring_par("rrs-128-routed".to_string(), Vec::new(), false);
        let key = RoutedLayout
            .key(&channel)
            .expect("a string channel has a routed key");
        assert_eq!(HashMapOuter.key(&channel), Ok(key.clone()));
        assert_eq!(
            RoutedLayout.data_channel(space, &key),
            HashMapOuter.data_channel(space, &key)
        );
        assert_eq!(
            RoutedLayout.counter_channel(space, &key),
            HashMapOuter.counter_channel(space, &key)
        );
        let random = Blake2b512Random::create_from_bytes(&[1, 2, 8]);
        assert_eq!(
            RoutedLayout.gensym(space, &random),
            HashMapOuter.gensym(space, &random)
        );
    }

    /// Added by decision 128 (kind routing): a discipline without a window keeps the defaults of
    /// the window steps, and they fail closed.
    #[test]
    fn space_window_steps_fail_closed_by_default() {
        assert_eq!(
            QueueDiscipline::similarity_config(&config("hashmap", "queue")),
            Err(SpaceError::Inconsistent(
                "the discipline has no similarity window"
            ))
        );
        assert_eq!(
            QueueDiscipline::send_level(&BTreeMap::<String, Par>::new()),
            Err(SpaceError::Inconsistent(
                "the discipline has no lanes window"
            ))
        );
        assert_eq!(
            QueueDiscipline::receive_lane(&None),
            Err(SpaceError::Inconsistent(
                "the discipline has no lanes window"
            ))
        );
        assert_eq!(QueueDiscipline::policy_of_query(None), WaiterPolicy::None);
        assert_eq!(
            QueueDiscipline::list_pairs(&[], None),
            Err(SpaceError::Inconsistent(
                "the discipline has no similarity window"
            ))
        );
    }
}
