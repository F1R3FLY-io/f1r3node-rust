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
//! (A5), so the agent opens every collection with the channels of the HashMap store.
//!
//! Each arm of the dispatch reserves the comparison of the config names with its family before
//! it compares them. So `hashmap:queue`, the first arm, reserves exactly the one comparison of
//! the P4 agent, and the host work of a queue operation does not change. The other arms follow
//! in ascending family index, so the arm at dispatch position `i` reserves `i + 1` comparisons.

use async_trait::async_trait;
use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::Par;
use models::rust::space_factory_urn::{SpaceFactoryUrn, SpaceInner, SpaceOuter};

use super::agent::{
    AllocOp, Allocation, ConsumeOp, Prepared, ProduceOp, SettleOp, SpaceAgent, SpaceCreation,
    SpaceEnv, SpaceOp, SpaceOutcome,
};
use super::disciplines::bag::BagDiscipline;
use super::disciplines::cell::CellDiscipline;
use super::disciplines::queue::QueueDiscipline;
use super::disciplines::stack::StackDiscipline;
use super::errors::SpaceError;
use super::generic::{
    begin_allocation, begin_collection, family_matches, family_unavailable, read_config_map,
    CollectionTxn, Discipline, FamilySpace, GenericSpace, OuterStore,
};
use super::namespace::NativeSpace;
use super::outer::array::ArrayOuter;
use super::outer::hashmap::HashMapOuter;
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

/// The agent of the family `hashmap:cell`.
static HASHMAP_CELL: GenericSpace<HashMapOuter, CellDiscipline> =
    GenericSpace::new(HashMapOuter, CellDiscipline);

/// The agent of the family `array:queue`.
static ARRAY_QUEUE: GenericSpace<ArrayOuter, QueueDiscipline> =
    GenericSpace::new(ArrayOuter, QueueDiscipline);

/// The agent of the family `vector:queue`.
static VECTOR_QUEUE: GenericSpace<VectorOuter, QueueDiscipline> =
    GenericSpace::new(VectorOuter, QueueDiscipline);

/// A wired family of native spaces.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Family {
    /// `hashmap:queue` (P4).
    HashMapQueue,
    /// `hashmap:bag` (P5a).
    HashMapBag,
    /// `hashmap:stack` (P5b).
    HashMapStack,
    /// `hashmap:cell` (P5, F15).
    HashMapCell,
    /// `array:queue` (P5d).
    ArrayQueue,
    /// `vector:queue` (P5d).
    VectorQueue,
}

impl Family {
    /// The wired families, in dispatch order: `hashmap:queue` first, then ascending family index.
    pub const WIRED: [Family; 6] = [
        Family::HashMapQueue,
        Family::HashMapBag,
        Family::HashMapStack,
        Family::HashMapCell,
        Family::ArrayQueue,
        Family::VectorQueue,
    ];

    /// The family of the factory URN `urn`, or `None` when it is not wired.
    pub fn of_urn(urn: SpaceFactoryUrn) -> Option<Family> {
        match (urn.outer, urn.inner) {
            (SpaceOuter::HashMap, SpaceInner::Queue) => Some(Family::HashMapQueue),
            (SpaceOuter::HashMap, SpaceInner::Bag) => Some(Family::HashMapBag),
            (SpaceOuter::HashMap, SpaceInner::Stack) => Some(Family::HashMapStack),
            (SpaceOuter::HashMap, SpaceInner::Cell) => Some(Family::HashMapCell),
            (SpaceOuter::Array, SpaceInner::Queue) => Some(Family::ArrayQueue),
            (SpaceOuter::Vector, SpaceInner::Queue) => Some(Family::VectorQueue),
            (
                SpaceOuter::HashMap
                | SpaceOuter::PathMap
                | SpaceOuter::Array
                | SpaceOuter::Vector
                | SpaceOuter::HashSet,
                _,
            ) => None,
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
            Family::HashMapCell => family_matches::<HashMapOuter, CellDiscipline>(config, meter),
            Family::ArrayQueue => family_matches::<ArrayOuter, QueueDiscipline>(config, meter),
            Family::VectorQueue => family_matches::<VectorOuter, QueueDiscipline>(config, meter),
        }
    }

    /// The agent of this family.
    pub fn agent(self) -> &'static dyn FamilySpace {
        match self {
            Family::HashMapQueue => &HASHMAP_QUEUE,
            Family::HashMapBag => &HASHMAP_BAG,
            Family::HashMapStack => &HASHMAP_STACK,
            Family::HashMapCell => &HASHMAP_CELL,
            Family::ArrayQueue => &ARRAY_QUEUE,
            Family::VectorQueue => &VECTOR_QUEUE,
        }
    }

    /// The named params of a send to a collection of this family: the schema of its discipline.
    pub fn send_params(self) -> &'static [&'static str] {
        match self {
            Family::HashMapQueue | Family::ArrayQueue | Family::VectorQueue => {
                QueueDiscipline::SEND_PARAMS
            }
            Family::HashMapBag => BagDiscipline::SEND_PARAMS,
            Family::HashMapStack => StackDiscipline::SEND_PARAMS,
            Family::HashMapCell => CellDiscipline::SEND_PARAMS,
        }
    }

    /// The named query params of a receive on a collection of this family.
    pub fn query_params(self) -> &'static [&'static str] {
        match self {
            Family::HashMapQueue | Family::ArrayQueue | Family::VectorQueue => {
                QueueDiscipline::QUERY_PARAMS
            }
            Family::HashMapBag => BagDiscipline::QUERY_PARAMS,
            Family::HashMapStack => StackDiscipline::QUERY_PARAMS,
            Family::HashMapCell => CellDiscipline::QUERY_PARAMS,
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
/// family that the config names.
fn dispatch(
    env: &SpaceEnv,
    opened: CollectionTxn,
    op: SpaceOp,
) -> Result<Prepared<SpaceOutcome>, SpaceError> {
    match Family::of_config(&opened.config, &env.meter)? {
        Some(family) => family.agent().run_with(env, opened, op),
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
        dispatch(env, opened, SpaceOp::Produce(op))
    }

    async fn prepare_consume(
        &self,
        env: &SpaceEnv,
        op: ConsumeOp,
    ) -> Result<Prepared<SpaceOutcome>, SpaceError> {
        let opened = begin_collection(&HashMapOuter, env, op.space, &op.key, &op.channel).await?;
        dispatch(env, opened, SpaceOp::Consume(op))
    }

    async fn prepare_settle(
        &self,
        env: &SpaceEnv,
        op: SettleOp,
    ) -> Result<Prepared<SpaceOutcome>, SpaceError> {
        let opened = begin_collection(&HashMapOuter, env, op.space, &op.key, &op.channel).await?;
        dispatch(env, opened, SpaceOp::Settle(op))
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
    const WIRED_NAMES: [(Family, &str, &str); 6] = [
        (Family::HashMapQueue, "hashmap", "queue"),
        (Family::HashMapBag, "hashmap", "bag"),
        (Family::HashMapStack, "hashmap", "stack"),
        (Family::HashMapCell, "hashmap", "cell"),
        (Family::ArrayQueue, "array", "queue"),
        (Family::VectorQueue, "vector", "queue"),
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
                Family::ArrayQueue,
                QueueDiscipline::SEND_PARAMS,
                QueueDiscipline::QUERY_PARAMS,
            ),
            (
                Family::VectorQueue,
                QueueDiscipline::SEND_PARAMS,
                QueueDiscipline::QUERY_PARAMS,
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
            ("pathmap", "queue"),
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
