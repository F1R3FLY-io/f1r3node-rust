//! The Vector outer store of the native spaces (P5d, F8 and F11, P5 design v3 A.10).
//!
//! A Vector space has no bound on its names. It names each collection by an index name
//! `N(sid ‖ u64_be(i))`, and its allocator hands out the indices `0, 1, 2, …` once each, in order
//! (`spaces::allocator`). The allocator record `A(sid)` holds the number of names that the space
//! has allocated, so the index of a name is the value of the counter when the name was allocated.
//! An allocation that would pass `i64::MAX` names fails with `OutOfNames` and allocates nothing.
//!
//! A collection of a Vector space has the key, the channels and the counter of the HashMap store
//! (A5), so the store delegates them to [`HashMapOuter`]. The store accepts only the index names
//! of its own space, and every other channel fails with `NotAnIndexName` before any staging (F8).
//! A Vector space takes no `size` param.
//!
//! The legacy vector store kept its next index in memory, so a restart lost it. This store keeps
//! it in the allocator record, which is consensus state, so two branches that allocate from one
//! space race on its record.

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::{CostAuthority, ListParWithRandom, Par};

use super::hashmap::HashMapOuter;
use crate::rust::interpreter::reified::costs::ScanMeter;
use crate::rust::interpreter::spaces::agent::AllocOp;
use crate::rust::interpreter::spaces::allocator::{self, AllocationRule};
use crate::rust::interpreter::spaces::errors::SpaceError;
use crate::rust::interpreter::spaces::generic::{AllocCx, OuterStore};
use crate::rust::interpreter::spaces::namespace::{CollectionKey, NativeSpace};
use crate::rust::interpreter::spaces::types::config::SpaceConfig;

/// The Vector outer store.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct VectorOuter;

impl OuterStore for VectorOuter {
    const NAME: &'static str = "vector";
    const ALLOCATOR: bool = true;

    /// The key of the HashMap store (A5).
    fn key(&self, channel: &Par) -> Result<CollectionKey, SpaceError> { HashMapOuter.key(channel) }

    /// Accepts `channel` only when it is an index name of this space, and otherwise fails with
    /// `NotAnIndexName` (F8). Every index is below the bound of a Vector space. The routed key is
    /// the key of `channel`, because routing computes it from the channel, so the store checks the
    /// channel only. The decode of the channel and the comparison of the spaces are reserved
    /// before they are made.
    fn check_key(
        &self,
        config: &SpaceConfig,
        channel: &Par,
        _key: &CollectionKey,
        meter: &ScanMeter,
    ) -> Result<(), SpaceError> {
        allocator::index_of_name(config, channel, meter).map(drop)
    }

    /// The data channel of the HashMap store (A5).
    fn data_channel(&self, space: NativeSpace, key: &CollectionKey) -> Par {
        HashMapOuter.data_channel(space, key)
    }

    /// The counter channel of the HashMap store (A5).
    fn counter_channel(&self, space: NativeSpace, key: &CollectionKey) -> Par {
        HashMapOuter.counter_channel(space, key)
    }

    /// The allocator `A(sid) = 0` (`allocator::creation_records`).
    fn creation_records(
        &self,
        space: NativeSpace,
        random: &Blake2b512Random,
        authority: &CostAuthority,
        meter: &ScanMeter,
    ) -> Result<Vec<(Par, ListParWithRandom)>, SpaceError> {
        allocator::creation_records(space, random, authority, meter)
    }

    /// Allocates the names of `op` in order, each once (`allocator::allocate` with
    /// [`AllocationRule::Unbounded`]).
    fn allocate(
        &self,
        cx: &mut AllocCx<'_>,
        _config: &SpaceConfig,
        op: &AllocOp,
    ) -> Result<Vec<u64>, SpaceError> {
        allocator::allocate(cx, AllocationRule::Unbounded, op)
    }

    /// A Vector space allocates its names with an allocation job (`SpaceAgent::allocate`),
    /// which reads and writes its allocator record. This method touches no storage, so it cannot
    /// allocate an index, and a random-tail name would fail the key check of the space. So it
    /// fails closed with `Inconsistent`. The family agent and the handle method `gensym` never
    /// call it for a Vector space.
    fn gensym(&self, _space: NativeSpace, _random: &Blake2b512Random) -> Result<Par, SpaceError> {
        Err(SpaceError::Inconsistent(
            "a Vector space allocates its names with an allocation job",
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use models::rhoapi::{BindPattern, TaggedContinuation};
    use models::rust::space_factory_urn::{SpaceFactoryUrn, SpaceInner, SpaceOuter};
    use models::rust::utils::{new_freevar_var, new_gint_par, new_gstring_par};
    use rspace_plus_plus::rspace::rspace::RSpace;
    use rspace_plus_plus::rspace::rspace_interface::ISpace;
    use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
    use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
    use rspace_plus_plus::rspace::space_store_handle::SpaceStoreHandle;

    use super::*;
    use crate::rust::interpreter::accounting::costs::Cost;
    use crate::rust::interpreter::accounting::RuntimeBudget;
    use crate::rust::interpreter::matcher::r#match::Matcher;
    use crate::rust::interpreter::spaces::agent::{
        Allocation, SpaceAgent, SpaceCreation, SpaceEnv,
    };
    use crate::rust::interpreter::spaces::disciplines::queue::QueueDiscipline;
    use crate::rust::interpreter::spaces::generic::{Discipline, GenericSpace};
    use crate::rust::interpreter::spaces::namespace::{self, Durability, NameTail, SID_LEN};
    use crate::rust::interpreter::spaces::record;
    use crate::rust::interpreter::spaces::types::qualifier::SpaceQualifier;

    type ParRSpace = RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>;
    type VectorQueue = GenericSpace<VectorOuter, QueueDiscipline>;

    const SPACE: NativeSpace = NativeSpace {
        sid: [9; SID_LEN],
        durability: Durability::Durable,
    };

    fn config() -> SpaceConfig {
        SpaceConfig::new(
            SPACE.sid,
            VectorOuter::NAME,
            QueueDiscipline::NAME,
            SpaceQualifier::Default,
        )
    }

    fn check(channel: &Par) -> Result<(), SpaceError> {
        let key = VectorOuter.key(channel)?;
        VectorOuter.check_key(&config(), channel, &key, &ScanMeter::new(None))
    }

    #[test]
    fn space_vector_store_shares_the_hashmap_key_and_channels() {
        assert_eq!(VectorOuter::NAME, SpaceOuter::Vector.as_str());
        for channel in [
            allocator::index_name(SPACE, 0),
            allocator::index_name(SPACE, u64::MAX),
            new_gstring_par("a channel".to_string(), Vec::new(), false),
        ] {
            let key = VectorOuter.key(&channel).expect("a key");
            assert_eq!(Ok(key.clone()), HashMapOuter.key(&channel));
            assert_eq!(
                VectorOuter.data_channel(SPACE, &key),
                HashMapOuter.data_channel(SPACE, &key)
            );
            assert_eq!(
                VectorOuter.counter_channel(SPACE, &key),
                HashMapOuter.counter_channel(SPACE, &key)
            );
        }
    }

    #[test]
    fn space_vector_check_key_accepts_every_index_name_of_its_space() {
        for index in [0, 1, 1 << 40, u64::MAX] {
            assert_eq!(check(&allocator::index_name(SPACE, index)), Ok(()));
        }
        let other = NativeSpace {
            sid: [8; SID_LEN],
            ..SPACE
        };
        for forged in [
            allocator::index_name(other, 0),
            namespace::gprivate(namespace::name_id(SPACE, NameTail::Random([9; 32]))),
            new_gstring_par("a channel".to_string(), Vec::new(), false),
            new_gint_par(0, Vec::new(), false),
        ] {
            assert_eq!(
                check(&forged),
                Err(SpaceError::NotAnIndexName),
                "{forged:?}"
            );
        }
    }

    #[test]
    fn space_vector_gensym_fails_closed() {
        let random = Blake2b512Random::create_from_bytes(b"gensym call");
        let closed = Err(SpaceError::Inconsistent(
            "a Vector space allocates its names with an allocation job",
        ));
        assert_eq!(VectorOuter.gensym(SPACE, &random), closed);
        assert_eq!(VectorQueue::default().gensym(SPACE, &random), closed);
    }

    #[tokio::test]
    async fn space_vector_agent_allocates_each_index_once() {
        let mut stores = InMemoryStoreManager::new();
        let space = ParRSpace::create(
            stores.r_space_stores().await.expect("in-memory stores"),
            Arc::new(Box::new(Matcher)),
        )
        .expect("an in-memory space");
        let env = SpaceEnv {
            store: SpaceStoreHandle::new(space.clone()),
            budget: RuntimeBudget::new(Cost::create(1_000_000, "vector store test")),
            meter: ScanMeter::new(None),
        };
        let creation = SpaceCreation {
            space: SPACE,
            config: config().to_par(),
            handle_pattern: BindPattern {
                patterns: Vec::new(),
                remainder: Some(new_freevar_var(0)),
                free_count: 1,
            },
            handle_continuation: TaggedContinuation::default(),
            random: Blake2b512Random::create_from_bytes(b"vector factory call"),
            authority: CostAuthority::default(),
            family: SpaceFactoryUrn {
                outer: SpaceOuter::Vector,
                inner: SpaceInner::Queue,
            },
        };
        VectorQueue::default()
            .prepare_create(&env, creation)
            .await
            .expect("the creation stages")
            .commit()
            .await
            .expect("the creation commits");
        let mut allocated = Vec::new();
        for (job, count) in [(1u8, 1u64), (2, 3), (3, 0), (4, 2)] {
            let op = AllocOp {
                space: SPACE,
                count,
                random_state: Blake2b512Random::create_from_bytes(&[job]).to_bytes(),
                authority: CostAuthority::default(),
            };
            let indices = match VectorQueue::default()
                .allocate(&env, op)
                .await
                .expect("the allocation stages")
            {
                Allocation::Indices(prepared) => {
                    prepared.commit().await.expect("the allocation commits")
                }
                Allocation::Baseline => panic!("a Vector space has an allocator"),
            };
            allocated.push(indices);
        }
        assert_eq!(allocated, vec![vec![0], vec![1, 2, 3], vec![], vec![4, 5]]);
        let values: Vec<i64> = space
            .get_data(&allocator::allocator_channel(SPACE))
            .await
            .iter()
            .map(|datum| record::counter_value(&datum.a).expect("an allocator record"))
            .collect();
        assert_eq!(values, vec![6]);
    }
}
