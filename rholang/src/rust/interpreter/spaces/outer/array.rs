//! The Array outer store of the native spaces (P5d, F7, F8 and F11, P5 design v3 A.10).
//!
//! An Array space has `size` names, the `size` param of its config: a `GInt` in `1..=i64::MAX`
//! that the factory requires (F7). The optional `cyclic` param is a `GBool`, and its default is
//! `false`. The space names each collection by an index name `N(sid ‖ u64_be(i))`, and the
//! allocator of the space hands out the indices (`spaces::allocator`):
//!
//! * a fixed Array space allocates the indices `0..size` once each, in order, and an allocation
//!   that does not fit fails with `OutOfNames` and allocates nothing;
//! * a cyclic Array space allocates the indices in order modulo `size`, so it reuses each name
//!   after `size` allocations.
//!
//! A collection of an Array space has the key, the channels and the counter of the HashMap store
//! (A5), so the store delegates them to [`HashMapOuter`]. The disciplines, the counters and the
//! joins of a collection are the ones of the HashMap store. The store accepts only the index names
//! of its own space below its size, and every other channel fails with `NotAnIndexName` before any
//! staging (F8). The storage is lazy: a collection exists from its first operation, and nothing
//! is preallocated.
//!
//! The legacy array store kept its next index in memory, so a restart lost it, and its cyclic
//! allocation raced between parallel branches (D8). This store keeps the next index in the
//! allocator record `A(sid)`, which the creation stages and every allocation consumes and produces
//! again. So the allocator is consensus state, and two branches that allocate from one space race
//! on its record.

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{CostAuthority, ListParWithRandom, Par};

use super::hashmap::HashMapOuter;
use crate::rust::interpreter::reified::costs::ScanMeter;
use crate::rust::interpreter::spaces::agent::AllocOp;
use crate::rust::interpreter::spaces::allocator::{self, AllocationRule};
use crate::rust::interpreter::spaces::errors::SpaceError;
/// The config params of an Array space (F7): `size` holds its number of names, and `cyclic`
/// makes it wrap around (default `false`). The constructor schema of the factory owns the names.
pub use crate::rust::interpreter::spaces::factory::defaults::{CYCLIC_PARAM, SIZE_PARAM};
use crate::rust::interpreter::spaces::generic::{AllocCx, OuterStore};
use crate::rust::interpreter::spaces::namespace::{CollectionKey, NativeSpace};
use crate::rust::interpreter::spaces::record;
use crate::rust::interpreter::spaces::types::config::SpaceConfig;

// Disabled at the W2 wiring of ARRAY and FACTORY: the factory schema (`factory/defaults.rs`)
// defines the same two names, and one definition keeps the schema and the reader in step.
// /// The config param that holds the number of names of an Array space (F7).
// pub const SIZE_PARAM: &str = "size";
//
// /// The config param that makes an Array space cyclic (F7). Its default is `false`.
// pub const CYCLIC_PARAM: &str = "cyclic";

/// The Array outer store.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ArrayOuter;

/// The shape of an Array space, from the params of its config.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ArrayShape {
    /// The number of names of the space, in `1..=i64::MAX`.
    pub size: i64,
    /// True when the space reuses its names modulo `size`.
    pub cyclic: bool,
}

impl ArrayShape {
    /// Decodes the shape from the params of `config`. The factory validates the params when it
    /// creates the space, so a stored config without a positive `GInt` size, or with a `cyclic`
    /// param that is not a `GBool`, is `Inconsistent`. Each lookup of a param and each read of a
    /// value is reserved before it is made.
    pub fn of_config(config: &SpaceConfig, meter: &ScanMeter) -> Result<ArrayShape, SpaceError> {
        let size = match param(config, SIZE_PARAM, meter)? {
            Some(value) => positive_size(value, meter)?,
            None => {
                return Err(SpaceError::Inconsistent(
                    "the config of an Array space has no size",
                ))
            }
        };
        let cyclic = match param(config, CYCLIC_PARAM, meter)? {
            Some(value) => cyclic_flag(value, meter)?,
            None => false,
        };
        Ok(ArrayShape { size, cyclic })
    }

    /// The allocation rule of the shape: modulo `size` for a cyclic space, and at most `size`
    /// names for a fixed space.
    pub fn rule(self) -> AllocationRule {
        match self.cyclic {
            true => AllocationRule::Cyclic { size: self.size },
            false => AllocationRule::Fixed { size: self.size },
        }
    }
}

/// The value of the param `key` of `config`, or `None`. The lookup in the ordered map of the
/// params is reserved first.
fn param<'c>(
    config: &'c SpaceConfig,
    key: &str,
    meter: &ScanMeter,
) -> Result<Option<&'c Par>, SpaceError> {
    meter.ordered_lookup(config.params.len(), key.len())?;
    Ok(config.params.get(key))
}

/// The size in the param value `value`: exactly one `GInt` of at least 1, without a connective.
/// The read of the value is reserved first.
fn positive_size(value: &Par, meter: &ScanMeter) -> Result<i64, SpaceError> {
    meter.inspect(value)?;
    match (record::sole_expr(value), value.connective_used) {
        (Some(ExprInstance::GInt(size)), false) if *size >= 1 => Ok(*size),
        _ => Err(SpaceError::Inconsistent(
            "the size of an Array space is not a positive integer",
        )),
    }
}

/// The flag in the param value `value`: exactly one `GBool`, without a connective. The read of
/// the value is reserved first.
fn cyclic_flag(value: &Par, meter: &ScanMeter) -> Result<bool, SpaceError> {
    meter.inspect(value)?;
    match (record::sole_expr(value), value.connective_used) {
        (Some(ExprInstance::GBool(cyclic)), false) => Ok(*cyclic),
        _ => Err(SpaceError::Inconsistent(
            "the cyclic flag of an Array space is not a Boolean",
        )),
    }
}

impl OuterStore for ArrayOuter {
    const NAME: &'static str = "array";
    const ALLOCATOR: bool = true;

    /// The key of the HashMap store (A5).
    fn key(&self, channel: &Par) -> Result<CollectionKey, SpaceError> { HashMapOuter.key(channel) }

    /// Accepts `channel` only when it is an index name of this space with an index below the
    /// size of the space, and otherwise fails with `NotAnIndexName` (F8). The routed key is the
    /// key of `channel`, because routing computes it from the channel, so the store checks the
    /// channel only. The decode of the channel, the decode of the shape and the comparisons are
    /// reserved before they are made.
    fn check_key(
        &self,
        config: &SpaceConfig,
        channel: &Par,
        _key: &CollectionKey,
        meter: &ScanMeter,
    ) -> Result<(), SpaceError> {
        let index = allocator::index_of_name(config, channel, meter)?;
        let shape = ArrayShape::of_config(config, meter)?;
        allocator::check_below(index, shape.size, meter)
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

    /// Allocates the names of `op` with the rule of the shape of `config`: modulo the size for a
    /// cyclic space, and at most the size for a fixed space (`allocator::allocate`).
    fn allocate(
        &self,
        cx: &mut AllocCx<'_>,
        config: &SpaceConfig,
        op: &AllocOp,
    ) -> Result<Vec<u64>, SpaceError> {
        let rule = ArrayShape::of_config(config, cx.meter())?.rule();
        allocator::allocate(cx, rule, op)
    }

    /// An Array space allocates its names with an allocation job (`SpaceAgent::allocate`),
    /// which reads and writes its allocator record. This method touches no storage, so it cannot
    /// allocate an index, and a random-tail name would fail the key check of the space. So it
    /// fails closed with `Inconsistent`. The family agent and the handle method `gensym` never
    /// call it for an Array space.
    fn gensym(&self, _space: NativeSpace, _random: &Blake2b512Random) -> Result<Par, SpaceError> {
        Err(SpaceError::Inconsistent(
            "an Array space allocates its names with an allocation job",
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use models::rhoapi::{BindPattern, TaggedContinuation};
    use models::rust::host_work::{
        HostWorkDimension, HostWorkLimit, HostWorkLimits, HostWorkUsage,
    };
    use models::rust::space_factory_urn::{SpaceFactoryUrn, SpaceInner, SpaceOuter};
    use models::rust::utils::{new_freevar_var, new_gbool_par, new_gint_par, new_gstring_par};
    use rspace_plus_plus::rspace::rspace::RSpace;
    use rspace_plus_plus::rspace::rspace_interface::ISpace;
    use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
    use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
    use rspace_plus_plus::rspace::space_store_handle::SpaceStoreHandle;

    use super::*;
    use crate::rust::interpreter::accounting::costs::Cost;
    use crate::rust::interpreter::accounting::RuntimeBudget;
    use crate::rust::interpreter::host_work::HostWorkBudget;
    use crate::rust::interpreter::matcher::r#match::Matcher;
    use crate::rust::interpreter::spaces::agent::{
        Allocation, SpaceAgent, SpaceCreation, SpaceEnv,
    };
    use crate::rust::interpreter::spaces::disciplines::queue::QueueDiscipline;
    use crate::rust::interpreter::spaces::generic::{Discipline, GenericSpace};
    use crate::rust::interpreter::spaces::namespace::{self, Durability, NameTail, SID_LEN};
    use crate::rust::interpreter::spaces::types::qualifier::SpaceQualifier;

    type ParRSpace = RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>;
    type ArrayQueue = GenericSpace<ArrayOuter, QueueDiscipline>;

    const SPACE: NativeSpace = NativeSpace {
        sid: [6; SID_LEN],
        durability: Durability::Durable,
    };

    fn int(value: i64) -> Par { new_gint_par(value, Vec::new(), false) }

    fn string(value: &str) -> Par { new_gstring_par(value.to_string(), Vec::new(), false) }

    fn config_with(params: &[(&str, Par)]) -> SpaceConfig {
        let mut config = SpaceConfig::new(
            SPACE.sid,
            ArrayOuter::NAME,
            QueueDiscipline::NAME,
            SpaceQualifier::Default,
        );
        for (key, value) in params {
            config.params.insert(key.to_string(), value.clone());
        }
        config
    }

    fn array(size: i64, cyclic: bool) -> SpaceConfig {
        config_with(&[
            (SIZE_PARAM, int(size)),
            (CYCLIC_PARAM, new_gbool_par(cyclic, Vec::new(), false)),
        ])
    }

    fn unmetered() -> ScanMeter { ScanMeter::new(None) }

    fn check(config: &SpaceConfig, channel: &Par) -> Result<(), SpaceError> {
        let key = ArrayOuter.key(channel)?;
        ArrayOuter.check_key(config, channel, &key, &unmetered())
    }

    #[test]
    fn space_array_store_shares_the_hashmap_key_and_channels() {
        assert_eq!(ArrayOuter::NAME, SpaceOuter::Array.as_str());
        let channels = [
            allocator::index_name(SPACE, 0),
            allocator::index_name(SPACE, u64::MAX),
            string("a channel"),
        ];
        for channel in &channels {
            let key = ArrayOuter.key(channel).expect("a key");
            assert_eq!(Ok(key.clone()), HashMapOuter.key(channel));
            assert_eq!(
                ArrayOuter.data_channel(SPACE, &key),
                HashMapOuter.data_channel(SPACE, &key)
            );
            assert_eq!(
                ArrayOuter.counter_channel(SPACE, &key),
                HashMapOuter.counter_channel(SPACE, &key)
            );
        }
    }

    #[test]
    fn space_array_shape_decodes_the_size_and_the_cyclic_flag() {
        let meter = unmetered();
        assert_eq!(
            ArrayShape::of_config(&config_with(&[(SIZE_PARAM, int(3))]), &meter),
            Ok(ArrayShape {
                size: 3,
                cyclic: false
            })
        );
        assert_eq!(
            ArrayShape::of_config(&array(5, true), &meter).map(ArrayShape::rule),
            Ok(AllocationRule::Cyclic { size: 5 })
        );
        assert_eq!(
            ArrayShape::of_config(&array(i64::MAX, false), &meter).map(ArrayShape::rule),
            Ok(AllocationRule::Fixed { size: i64::MAX })
        );
        let no_size = SpaceError::Inconsistent("the config of an Array space has no size");
        let bad_size =
            SpaceError::Inconsistent("the size of an Array space is not a positive integer");
        let bad_flag =
            SpaceError::Inconsistent("the cyclic flag of an Array space is not a Boolean");
        let mut flagged = int(3);
        flagged.connective_used = true;
        let mut pair = int(3);
        pair.exprs.extend(int(4).exprs);
        for (params, expected) in [
            (vec![], no_size.clone()),
            (
                vec![(CYCLIC_PARAM, new_gbool_par(true, Vec::new(), false))],
                no_size,
            ),
            (vec![(SIZE_PARAM, int(0))], bad_size.clone()),
            (vec![(SIZE_PARAM, int(-4))], bad_size.clone()),
            (vec![(SIZE_PARAM, string("3"))], bad_size.clone()),
            (vec![(SIZE_PARAM, flagged)], bad_size.clone()),
            (vec![(SIZE_PARAM, pair)], bad_size.clone()),
            (vec![(SIZE_PARAM, Par::default())], bad_size),
            (
                vec![(SIZE_PARAM, int(3)), (CYCLIC_PARAM, int(1))],
                bad_flag.clone(),
            ),
            (
                vec![(SIZE_PARAM, int(3)), (CYCLIC_PARAM, Par::default())],
                bad_flag,
            ),
        ] {
            assert_eq!(
                ArrayShape::of_config(&config_with(&params), &meter),
                Err(expected),
                "{params:?}"
            );
        }
    }

    #[test]
    fn space_array_shape_decode_reserves_its_lookups_and_reads() {
        let host_work = HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(u64::MAX)));
        let meter = ScanMeter::new(Some(host_work.clone()));
        ArrayShape::of_config(&array(4, false), &meter).expect("a shape");
        assert!(host_work.usage(HostWorkDimension::VerificationOperations) > HostWorkUsage::ZERO);
        assert!(host_work.usage(HostWorkDimension::VerificationBytes) > HostWorkUsage::ZERO);
        let zero = HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(0)));
        assert_eq!(
            ArrayShape::of_config(&array(4, false), &ScanMeter::new(Some(zero))),
            Err(SpaceError::Host(
                rspace_plus_plus::rspace::errors::RSpaceError::HostWorkRejected
            ))
        );
    }

    #[test]
    fn space_array_check_key_accepts_its_index_names_below_its_size() {
        for cyclic in [false, true] {
            let config = array(3, cyclic);
            for index in 0..3 {
                assert_eq!(check(&config, &allocator::index_name(SPACE, index)), Ok(()));
            }
            let other = NativeSpace {
                sid: [7; SID_LEN],
                ..SPACE
            };
            for forged in [
                allocator::index_name(SPACE, 3),
                allocator::index_name(SPACE, u64::MAX),
                allocator::index_name(other, 0),
                namespace::gprivate(namespace::name_id(SPACE, NameTail::Random([6; 32]))),
                string("a channel"),
                int(0),
            ] {
                assert_eq!(
                    check(&config, &forged),
                    Err(SpaceError::NotAnIndexName),
                    "{forged:?}"
                );
            }
        }
        // A channel of the space under a corrupt config fails with the decode of the shape.
        assert_eq!(
            check(&config_with(&[]), &allocator::index_name(SPACE, 0)),
            Err(SpaceError::Inconsistent(
                "the config of an Array space has no size"
            ))
        );
    }

    #[test]
    fn space_array_gensym_fails_closed() {
        let random = Blake2b512Random::create_from_bytes(b"gensym call");
        assert_eq!(
            ArrayOuter.gensym(SPACE, &random),
            Err(SpaceError::Inconsistent(
                "an Array space allocates its names with an allocation job"
            ))
        );
        assert_eq!(
            ArrayQueue::default().gensym(SPACE, &random),
            Err(SpaceError::Inconsistent(
                "an Array space allocates its names with an allocation job"
            ))
        );
    }

    async fn space_env() -> (ParRSpace, SpaceEnv) {
        let mut stores = InMemoryStoreManager::new();
        let space = ParRSpace::create(
            stores.r_space_stores().await.expect("in-memory stores"),
            Arc::new(Box::new(Matcher)),
        )
        .expect("an in-memory space");
        let env = SpaceEnv {
            store: SpaceStoreHandle::new(space.clone()),
            budget: RuntimeBudget::new(Cost::create(1_000_000, "array store test")),
            meter: ScanMeter::new(None),
            volatile: Default::default(),
        };
        (space, env)
    }

    fn creation(config: &SpaceConfig) -> SpaceCreation {
        SpaceCreation {
            space: SPACE,
            config: config.to_par(),
            handle_pattern: BindPattern {
                patterns: Vec::new(),
                remainder: Some(new_freevar_var(0)),
                free_count: 1,
            },
            handle_continuation: TaggedContinuation::default(),
            random: Blake2b512Random::create_from_bytes(b"array factory call"),
            authority: CostAuthority::default(),
            family: SpaceFactoryUrn {
                outer: SpaceOuter::Array,
                inner: SpaceInner::Queue,
            },
        }
    }

    /// Runs the allocation of `count` names with the random state of the job `job`.
    async fn allocate(env: &SpaceEnv, job: u8, count: u64) -> Result<Vec<u64>, SpaceError> {
        let op = AllocOp {
            space: SPACE,
            count,
            random_state: Blake2b512Random::create_from_bytes(&[job]).to_bytes(),
            authority: CostAuthority::default(),
        };
        match ArrayQueue::default().allocate(env, op).await? {
            Allocation::Indices(prepared) => prepared.commit().await,
            Allocation::Baseline => panic!("an Array space has an allocator"),
        }
    }

    async fn allocator_value(space: &ParRSpace) -> Vec<i64> {
        space
            .get_data(&allocator::allocator_channel(SPACE))
            .await
            .iter()
            .map(|datum| record::counter_value(&datum.a).expect("an allocator record"))
            .collect()
    }

    #[tokio::test]
    async fn space_array_agent_allocates_fixed_and_cyclic_names() {
        let (space, env) = space_env().await;
        ArrayQueue::default()
            .prepare_create(&env, creation(&array(3, false)))
            .await
            .expect("the creation stages")
            .commit()
            .await
            .expect("the creation commits");
        assert_eq!(
            space.take_event_log().await.len(),
            4,
            "the config, the space counter, the allocator and the handle"
        );
        assert_eq!(allocator_value(&space).await, vec![0]);
        assert_eq!(allocate(&env, 1, 2).await, Ok(vec![0, 1]));
        assert_eq!(
            space.take_event_log().await.len(),
            3,
            "the take and the put"
        );
        assert_eq!(allocate(&env, 2, 2).await, Err(SpaceError::OutOfNames));
        assert!(
            space.take_event_log().await.is_empty(),
            "nothing is allocated"
        );
        assert_eq!(allocator_value(&space).await, vec![2]);
        assert_eq!(allocate(&env, 3, 1).await, Ok(vec![2]));
        assert_eq!(allocate(&env, 4, 1).await, Err(SpaceError::OutOfNames));
        assert_eq!(allocator_value(&space).await, vec![3]);

        let (space, env) = space_env().await;
        ArrayQueue::default()
            .prepare_create(&env, creation(&array(3, true)))
            .await
            .expect("the creation stages")
            .commit()
            .await
            .expect("the creation commits");
        assert_eq!(allocate(&env, 1, 2).await, Ok(vec![0, 1]));
        assert_eq!(allocate(&env, 2, 2).await, Ok(vec![2, 0]));
        assert_eq!(allocate(&env, 3, 4).await, Ok(vec![1, 2, 0, 1]));
        assert_eq!(allocator_value(&space).await, vec![2]);
    }
}
