//! The allocator of the index names of the Array and Vector spaces (P5d, F7, F8 and F11, P5
//! design v3 A.10).
//!
//! An Array or a Vector space names its collections by index. The name with the index `i` is
//! `N(sid ‖ u64_be(i))` (`NameTail::Index`), and only this module and its callers make such names.
//! The allocator record `A(sid)` holds the next index `n` in the counter format of
//! `record::counter_record`. The creation of the space stages `A(sid) = 0` together with the
//! config, the space counter and the handle ([`creation_records`]). So two branches that allocate
//! from one space always consume one shared produce, and merge check #1 sees their race. An
//! allocator that a space made at its first allocation would let two branches allocate one index
//! with no shared produce (the Unsafe control `LazyAllocator` of `CounterConflict.tla`).
//!
//! An allocation of `k` names runs in one `Intent::Space` job over the lock keys `[C(sid), A(sid)]`
//! (`generic::begin_allocation`). The store of the space gives the [`AllocationRule`], and
//! [`allocate`] stages the allocation:
//!
//! ```text
//! ALLOC(rule, k):
//!   n ← the value of the only A(sid) record             ▷ decoded from a copy of the record
//!   fixed array of size s:  require n + k ≤ s, else OutOfNames     ▷ nothing is staged
//!                           indices n, n+1, …, n+k−1      next ← n + k
//!   cyclic array of size s: indices (n + i) mod s         next ← (n + k) mod s
//!   vector:                 require n + k ≤ i64::MAX, else OutOfNames
//!                           indices n, n+1, …, n+k−1      next ← n + k
//!   take A(sid) = n                                      ▷ counter::take, purpose Allocator
//!   put A(sid) = next                                    ▷ counter::put, purpose Allocator
//! ```
//!
//! The plan of an allocation is complete before the first staged operation. So an allocation
//! that fails with `OutOfNames` stages nothing, and the dropped transaction allocates no name. A
//! cyclic array reuses its names after `s` allocations, as the modulo rule of the FIPS says, so
//! two holders of one cyclic name share one collection. The take and the put use the random
//! states `RandomPurpose::Allocator.derive(r).split_byte(0)` and `.split_byte(1)` of the random
//! state `r` of the job. Their identities therefore differ for every job, also when a cyclic
//! allocator takes a value again (S6). Decision 43 does not apply, because an allocation never
//! re-issues, and Rule E does not apply, because an allocation never peeks.
//!
//! A collection of an Array or a Vector space has the key, the channels and the counter of the
//! HashMap store (A5). Its channel must be an index name of its own space ([`index_of_name`]),
//! and a fixed or cyclic Array space also requires an index below its size (F8). Every other
//! channel fails with `NotAnIndexName` before any staging.

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::{CostAuthority, ListParWithRandom, Par};

use super::agent::AllocOp;
use super::errors::SpaceError;
use super::generic::AllocCx;
use super::namespace::{self, Durability, NameTail, NativeSpace, RandomPurpose, ReifiedId};
use super::record;
use super::types::config::SpaceConfig;

/// How a store allocates its index names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AllocationRule {
    /// A fixed Array space with `size` names. It allocates each index once, in order, and it
    /// fails with `OutOfNames` when fewer than the requested names are free.
    Fixed {
        /// The number of names of the space, in `1..=i64::MAX`.
        size: i64,
    },
    /// A cyclic Array space with `size` names. It allocates the indices in order modulo
    /// `size`, so it never runs out of names and it reuses each name after `size` allocations.
    Cyclic {
        /// The number of names of the space, in `1..=i64::MAX`.
        size: i64,
    },
    /// A Vector space. It allocates each index once, in order, up to `i64::MAX` names.
    Unbounded,
}

/// The planned allocation of an [`AllocationRule`]: the indices in allocation order, and the
/// value that the allocator record takes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AllocationPlan {
    /// The allocated indices, in allocation order.
    pub indices: Vec<u64>,
    /// The next value of the allocator record.
    pub next: i64,
}

impl AllocationRule {
    /// Plans the allocation of `count` names from the allocator value `value`. The plan stages
    /// nothing. A fixed array fails with `OutOfNames` when `value + count` exceeds its size, and
    /// a vector fails with `OutOfNames` when `value + count` exceeds `i64::MAX`. An allocator
    /// value outside the range of the rule is `Inconsistent`.
    pub fn plan(self, value: i64, count: u64) -> Result<AllocationPlan, SpaceError> {
        let start = u64::try_from(value)
            .map_err(|_| SpaceError::Inconsistent("an allocator value is negative"))?;
        match self {
            AllocationRule::Fixed { size } => {
                if value > size {
                    return Err(SpaceError::Inconsistent(
                        "the allocator of an Array space exceeds its size",
                    ));
                }
                let next = match bounded_sum(value, count) {
                    Some(next) if next <= size => next,
                    Some(_) | None => return Err(SpaceError::OutOfNames),
                };
                Ok(AllocationPlan {
                    indices: sequential(start, count)?,
                    next,
                })
            }
            AllocationRule::Cyclic { size } => {
                if value >= size {
                    return Err(SpaceError::Inconsistent(
                        "the allocator of a cyclic Array space is not below its size",
                    ));
                }
                let modulus = u64::try_from(size).map_err(|_| {
                    SpaceError::Inconsistent("the size of an Array space is negative")
                })?;
                let next = (start + count % modulus) % modulus;
                Ok(AllocationPlan {
                    indices: cyclic(start, modulus, count)?,
                    next: i64::try_from(next).map_err(|_| {
                        SpaceError::Inconsistent("a cyclic allocator value exceeds i64")
                    })?,
                })
            }
            AllocationRule::Unbounded => {
                let next = bounded_sum(value, count).ok_or(SpaceError::OutOfNames)?;
                Ok(AllocationPlan {
                    indices: sequential(start, count)?,
                    next,
                })
            }
        }
    }
}

/// `value + count`, or `None` when the sum exceeds `i64::MAX`.
fn bounded_sum(value: i64, count: u64) -> Option<i64> {
    i64::try_from(count)
        .ok()
        .and_then(|count| value.checked_add(count))
}

/// The indices `start, start + 1, …, start + count − 1`. The caller has checked that the last
/// index fits in `i64`.
fn sequential(start: u64, count: u64) -> Result<Vec<u64>, SpaceError> {
    let mut indices = Vec::<u64>::with_capacity(buffer_length(count)?);
    indices.extend(start..start + count);
    Ok(indices)
}

/// The indices `(start + i) mod modulus` for `i` in `0..count`. `start` is below `modulus`.
fn cyclic(start: u64, modulus: u64, count: u64) -> Result<Vec<u64>, SpaceError> {
    let mut indices = Vec::<u64>::with_capacity(buffer_length(count)?);
    let mut index = start;
    for _ in 0..count {
        indices.push(index);
        index = match index + 1 {
            wrapped if wrapped == modulus => 0,
            next => next,
        };
    }
    Ok(indices)
}

/// The length of a buffer of `count` indices. A count that the address space cannot hold is
/// `Inconsistent`.
fn buffer_length(count: u64) -> Result<usize, SpaceError> {
    usize::try_from(count)
        .map_err(|_| SpaceError::Inconsistent("an allocation count exceeds the address space"))
}

/// The space that `config` describes: its id, and its durability. A `default` space is durable
/// consensus state, and a `temp` or `seq` space is ephemeral, as the factory makes them.
pub fn space_of(config: &SpaceConfig) -> NativeSpace {
    NativeSpace {
        sid: config.space_id,
        durability: match config.qualifier.is_persistent() {
            true => Durability::Durable,
            false => Durability::Ephemeral,
        },
    }
}

/// The index of `channel` when it is an index name of the space of `config`, or `NotAnIndexName`
/// (F8). A forged channel, a name with a random tail and a name of another space fail.
pub fn index_of_name(config: &SpaceConfig, channel: &Par) -> Result<u64, SpaceError> {
    let Some(ReifiedId::Name(space, NameTail::Index(index))) = namespace::reified_id_of(channel)
    else {
        return Err(SpaceError::NotAnIndexName);
    };
    match space == space_of(config) {
        true => Ok(index),
        false => Err(SpaceError::NotAnIndexName),
    }
}

/// Fails with `NotAnIndexName` unless `index` is below `size`.
pub fn check_below(index: u64, size: i64) -> Result<(), SpaceError> {
    match u64::try_from(size).is_ok_and(|size| index < size) {
        true => Ok(()),
        false => Err(SpaceError::NotAnIndexName),
    }
}

/// The index name `N(sid ‖ u64_be(index))` of `space`: the channel of the collection with the
/// index `index`.
pub fn index_name(space: NativeSpace, index: u64) -> Par {
    namespace::gprivate(namespace::name_id(space, NameTail::Index(index)))
}

/// The allocator channel `A(sid)` of `space`.
pub fn allocator_channel(space: NativeSpace) -> Par {
    namespace::gprivate(namespace::allocator_channel(space))
}

/// The records that the creation of an Array or a Vector space stages after the space counter: the
/// allocator `A(sid) = 0`. `random` is the random state of the factory call, and the record takes
/// `RandomPurpose::Allocator.derive(random).split_byte(1)`, as the put of an allocation does. The
/// record carries the cost authority `authority` of the caller, as the space counter does.
pub fn creation_records(
    space: NativeSpace,
    random: &Blake2b512Random,
    authority: &CostAuthority,
) -> Vec<(Par, ListParWithRandom)> {
    let mut records = Vec::<(Par, ListParWithRandom)>::with_capacity(1);
    let channel = allocator_channel(space);
    let record = record::counter_record(
        0,
        &RandomPurpose::Allocator.derive(random).split_byte(1),
        record::cost_authority(authority),
    );
    records.push((channel, record));
    records
}

/// Stages the allocation of `op.count` names with `rule` in `cx`, and returns the indices in
/// allocation order (`ALLOC`). It reads the only allocator record, plans the allocation, and
/// only then stages the take of the record and the put of its next value. So a failed plan
/// stages nothing. A space without an allocator record is `Inconsistent`.
pub fn allocate(
    cx: &mut AllocCx<'_>,
    rule: AllocationRule,
    op: &AllocOp,
) -> Result<Vec<u64>, SpaceError> {
    let stored = cx.allocator_record()?.ok_or(SpaceError::Inconsistent(
        "an Array or a Vector space has no allocator record",
    ))?;
    let value = record::counter_value(&stored.a)?;
    let AllocationPlan { indices, next } = rule.plan(value, op.count)?;
    cx.take(stored, value)?;
    cx.put(next)?;
    Ok(indices)
}

#[cfg(test)]
mod tests {
    use models::rhoapi::cost_signature::Value as CostSignatureValue;
    use models::rhoapi::expr::ExprInstance;
    use models::rhoapi::{CostRegion, CostSignature, Expr};
    use models::rust::utils::new_gstring_par;

    use super::*;
    use crate::rust::interpreter::spaces::namespace::SID_LEN;
    use crate::rust::interpreter::spaces::types::qualifier::SpaceQualifier;

    const SPACE: NativeSpace = NativeSpace {
        sid: [4; SID_LEN],
        durability: Durability::Durable,
    };

    fn plan(rule: AllocationRule, value: i64, count: u64) -> Result<AllocationPlan, SpaceError> {
        rule.plan(value, count)
    }

    fn planned(indices: Vec<u64>, next: i64) -> Result<AllocationPlan, SpaceError> {
        Ok(AllocationPlan { indices, next })
    }

    fn config(qualifier: SpaceQualifier) -> SpaceConfig {
        SpaceConfig::new(SPACE.sid, "array", "queue", qualifier)
    }

    fn region(byte: u8) -> CostAuthority {
        CostAuthority {
            regions: vec![CostRegion {
                instance_id: vec![byte; 32],
                signature: Some(CostSignature {
                    value: Some(CostSignatureValue::Ground(vec![byte])),
                }),
            }],
        }
    }

    #[test]
    fn space_allocator_fixed_array_allocates_in_order_up_to_its_size() {
        let fixed = AllocationRule::Fixed { size: 4 };
        assert_eq!(plan(fixed, 0, 3), planned(vec![0, 1, 2], 3));
        assert_eq!(plan(fixed, 3, 1), planned(vec![3], 4));
        assert_eq!(plan(fixed, 4, 0), planned(Vec::new(), 4));
        assert_eq!(plan(fixed, 0, 4), planned(vec![0, 1, 2, 3], 4));
        // An allocation that does not fit allocates nothing.
        assert_eq!(plan(fixed, 3, 2), Err(SpaceError::OutOfNames));
        assert_eq!(plan(fixed, 4, 1), Err(SpaceError::OutOfNames));
        assert_eq!(plan(fixed, 0, u64::MAX), Err(SpaceError::OutOfNames));
        let full = AllocationRule::Fixed { size: i64::MAX };
        let last = u64::try_from(i64::MAX - 1).expect("a positive index");
        assert_eq!(plan(full, i64::MAX - 1, 1), planned(vec![last], i64::MAX));
        assert_eq!(plan(full, i64::MAX, 1), Err(SpaceError::OutOfNames));
        // An allocator beyond the size is a corrupt record.
        assert_eq!(
            plan(fixed, 5, 0),
            Err(SpaceError::Inconsistent(
                "the allocator of an Array space exceeds its size"
            ))
        );
        assert_eq!(
            plan(fixed, -1, 0),
            Err(SpaceError::Inconsistent("an allocator value is negative"))
        );
    }

    #[test]
    fn space_allocator_cyclic_array_wraps_modulo_its_size() {
        let cyclic = AllocationRule::Cyclic { size: 3 };
        assert_eq!(plan(cyclic, 0, 2), planned(vec![0, 1], 2));
        assert_eq!(plan(cyclic, 2, 2), planned(vec![2, 0], 1));
        assert_eq!(plan(cyclic, 1, 7), planned(vec![1, 2, 0, 1, 2, 0, 1], 2));
        assert_eq!(plan(cyclic, 2, 0), planned(Vec::new(), 2));
        let single = AllocationRule::Cyclic { size: 1 };
        assert_eq!(plan(single, 0, 3), planned(vec![0, 0, 0], 0));
        // The greatest size wraps without an overflow of the index or of the next value.
        let widest = AllocationRule::Cyclic { size: i64::MAX };
        let last = u64::try_from(i64::MAX - 1).expect("a positive index");
        assert_eq!(plan(widest, i64::MAX - 1, 2), planned(vec![last, 0], 1));
        assert_eq!(
            plan(cyclic, 3, 1),
            Err(SpaceError::Inconsistent(
                "the allocator of a cyclic Array space is not below its size"
            ))
        );
    }

    #[test]
    fn space_allocator_vector_allocates_every_index_once() {
        let vector = AllocationRule::Unbounded;
        assert_eq!(plan(vector, 0, 1), planned(vec![0], 1));
        assert_eq!(plan(vector, 1, 3), planned(vec![1, 2, 3], 4));
        assert_eq!(plan(vector, 4, 0), planned(Vec::new(), 4));
        let last = u64::try_from(i64::MAX - 1).expect("a positive index");
        assert_eq!(
            plan(vector, i64::MAX - 2, 2),
            planned(vec![last - 1, last], i64::MAX)
        );
        assert_eq!(plan(vector, i64::MAX, 1), Err(SpaceError::OutOfNames));
        assert_eq!(plan(vector, 0, u64::MAX), Err(SpaceError::OutOfNames));
    }

    #[test]
    fn space_allocator_index_names_belong_to_their_space() {
        let durable = config(SpaceQualifier::Default);
        assert_eq!(space_of(&durable), SPACE);
        assert_eq!(
            space_of(&config(SpaceQualifier::Temp)).durability,
            Durability::Ephemeral
        );
        assert_eq!(
            space_of(&config(SpaceQualifier::Seq)).durability,
            Durability::Ephemeral
        );
        for index in [0, 1, 41, u64::MAX] {
            assert_eq!(
                index_of_name(&durable, &index_name(SPACE, index)),
                Ok(index)
            );
        }
        let other = NativeSpace {
            sid: [5; SID_LEN],
            ..SPACE
        };
        let ephemeral = NativeSpace {
            durability: Durability::Ephemeral,
            ..SPACE
        };
        let random_tail = namespace::gprivate(namespace::name_id(SPACE, NameTail::Random([4; 32])));
        let mut decorated = index_name(SPACE, 1);
        decorated.exprs.push(Expr {
            expr_instance: Some(ExprInstance::GInt(1)),
        });
        for forged in [
            new_gstring_par("a channel".to_string(), Vec::new(), false),
            index_name(other, 0),
            index_name(ephemeral, 0),
            random_tail,
            decorated,
            allocator_channel(SPACE),
            namespace::gprivate(namespace::handle_channel(SPACE)),
            Par::default(),
        ] {
            assert_eq!(
                index_of_name(&durable, &forged),
                Err(SpaceError::NotAnIndexName),
                "{forged:?}"
            );
        }
        // An ephemeral space accepts its own ephemeral names only.
        let temp = config(SpaceQualifier::Temp);
        assert_eq!(index_of_name(&temp, &index_name(ephemeral, 3)), Ok(3));
        assert_eq!(
            index_of_name(&temp, &index_name(SPACE, 3)),
            Err(SpaceError::NotAnIndexName)
        );
        assert_eq!(check_below(2, 3), Ok(()));
        assert_eq!(check_below(3, 3), Err(SpaceError::NotAnIndexName));
        assert_eq!(
            check_below(u64::MAX, i64::MAX),
            Err(SpaceError::NotAnIndexName)
        );
    }

    #[test]
    fn space_allocator_creation_stages_the_allocator_at_zero() {
        let random = Blake2b512Random::create_from_bytes(b"factory call");
        let authority = region(3);
        let records = creation_records(SPACE, &random, &authority);
        let [(channel, data)] = records.as_slice() else {
            panic!("one allocator record: {records:?}");
        };
        assert_eq!(*channel, allocator_channel(SPACE));
        assert_eq!(
            namespace::reified_id_of(channel),
            Some(ReifiedId::Allocator(SPACE))
        );
        assert_eq!(record::counter_value(data), Ok(0));
        assert_eq!(
            *data,
            record::counter_record(
                0,
                &RandomPurpose::Allocator.derive(&random).split_byte(1),
                Some(authority)
            )
        );
        // Without a region the record carries no authority, as the space counter.
        let records = creation_records(SPACE, &random, &CostAuthority::default());
        assert_eq!(records[0].1.cost_authority, None);
    }
}
