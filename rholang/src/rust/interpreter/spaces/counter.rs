//! The counters of the ordered collections (M-1, decision 31, P4 design B.3).
//!
//! Every operation on an ordered collection, peeks and fires that change no state included,
//! consumes the counter `S(sid,k)` of its collection and produces it again with the next value,
//! in its own transaction. The value before the bump is the `ord` of the operation. Two branches
//! that use one collection consume one counter produce, so merge check #1 sees their race, and
//! the operations of one block on one collection form one chain.
//!
//! The bump stages two operations:
//!
//! | # | Operation | Outcome | Events |
//! |---|-----------|---------|--------|
//! | 0 | consume `S(sid,k)` with the counter pattern and `K(v)` | fire with the counter record `v` | `Consume`, `COMM` |
//! | 1 | produce `S(sid,k) = v + 1` | store | `Produce` |
//!
//! The first operation on a collection finds no counter record. It consumes the space counter
//! `S(sid) = m` instead, produces `S(sid) = m + 1`, and produces `S(sid,k) = 1`. Its `ord` is 0.
//! So two branches that each open a collection of one space race on `S(sid)` (A25).
//!
//! The continuation `K(v)` has the body `GInt(v)`, so its identity is new for each value, also
//! when a re-issued operation has the same random state. The random states derive from the
//! random state of the operation (P4 design C.11): `Counter` and `split_byte(0)` for `K`, and
//! `split_byte(1)` for the new record, or `SpaceCounter` for the space counter.
//!
//! A collection with two counter records, a persistent counter record, or a missing space
//! counter is `Inconsistent`. A counter at `i64::MAX` fails with `CounterOverflow`.
//!
//! The bump reserves its reads and copies on the meter of the job before it makes them
//! (decision 42): it copies only the counter record that it consumes, and it moves that copy
//! into the fire. The pattern and the continuation are built from templates, whose reserved
//! copies cover their allocations. As in the reducer of `feature/cost-accounted-rho`, a split of
//! a random state is not charged (`util::evaluation_random`), and its encode is
//! (`ScanMeter::random_bytes`).

use std::collections::BTreeSet;
use std::sync::LazyLock;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::tagged_continuation::TaggedCont;
use models::rhoapi::{
    BindPattern, CostAuthority, ListParWithRandom, Par, ParWithRandom, TaggedContinuation,
};
use models::rust::utils::{new_freevar_par, new_gint_par};
use rspace_plus_plus::rspace::internal::{ConsumeCandidate, Datum};
use rspace_plus_plus::rspace::space_store_handle::{
    ConsumeOutcome, ProduceOutcome, SpaceConsume, SpaceProduce,
};

use super::agent::ParSpaceTxn;
use super::errors::SpaceError;
use super::matching::SpaceMatcher;
use super::namespace::RandomPurpose;
use super::record::{self, OperationAuthority};
use crate::rust::interpreter::reified::legacy_charges::LegacyChain;

/// The pattern that consumes a counter record: two free variables, one for each par.
pub fn counter_pattern() -> BindPattern {
    BindPattern {
        patterns: vec![
            new_freevar_par(0, Vec::new()),
            new_freevar_par(1, Vec::new()),
        ],
        remainder: None,
        free_count: 2,
    }
}

/// The continuation that consumes the counter record with the value `value`.
pub fn counter_continuation(
    value: i64,
    random: &Blake2b512Random,
    cost_authority: Option<CostAuthority>,
) -> TaggedContinuation {
    continuation_with(value, random.to_bytes(), cost_authority)
}

/// The counter continuation with the value `value`, the random state bytes `random_state` and
/// the cost authority `cost_authority`.
fn continuation_with(
    value: i64,
    random_state: Vec<u8>,
    cost_authority: Option<CostAuthority>,
) -> TaggedContinuation {
    TaggedContinuation {
        tagged_cont: Some(TaggedCont::ParBody(ParWithRandom {
            body: Some(new_gint_par(value, Vec::new(), false)),
            random_state,
        })),
        guard: None,
        cost_authority,
    }
}

/// The template of the counter pattern.
static COUNTER_PATTERN: LazyLock<BindPattern> = LazyLock::new(counter_pattern);

/// The template of a counter continuation: its body, without random state bytes and without a
/// cost authority. The value is an inline integer, so every counter continuation has its shape.
static COUNTER_CONTINUATION: LazyLock<TaggedContinuation> =
    LazyLock::new(|| continuation_with(0, Vec::new(), None));

/// An operation that a bump stages.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CounterOp {
    /// The consume of a counter record. It fires.
    Take,
    /// The produce of a counter record. It stores.
    Put,
}

/// The operations of the first bump of a collection: the take and the put of the space
/// counter, and the put of the new collection counter.
const OPEN: [CounterOp; 3] = [CounterOp::Take, CounterOp::Put, CounterOp::Put];

/// The operations of every later bump: the take and the put of the collection counter.
const STEP: [CounterOp; 2] = [CounterOp::Take, CounterOp::Put];

/// The bump of one collection operation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bump {
    /// The order of the operation: the value of the collection counter before the bump, or 0 for
    /// the first operation on the collection.
    pub ord: i64,
    /// The operations that the bump staged, in staging order.
    pub staged: &'static [CounterOp],
}

/// The counter channels of one collection.
#[derive(Clone, Copy, Debug)]
pub struct CounterChannels<'a> {
    /// The space counter `S(sid)`.
    pub space: &'a Par,
    /// The collection counter `S(sid,k)`.
    pub collection: &'a Par,
}

/// What the staging of a bump needs from its operation.
pub struct BumpContext<'a> {
    /// The matcher of the job, with its meter.
    pub matcher: &'a SpaceMatcher<'a>,
    /// The random state of the operation.
    pub random: &'a Blake2b512Random,
    /// The authorities of the operation (P4 design B.7).
    pub authority: OperationAuthority<'a>,
}

/// The only counter record of `channel` in the staged view, copied, or `None`. Two records are
/// `Inconsistent` with the message `two`.
fn only_record(
    txn: &mut ParSpaceTxn,
    channel: &Par,
    context: &BumpContext<'_>,
    two: &'static str,
) -> Result<Option<Datum<ListParWithRandom>>, SpaceError> {
    match txn.data(channel)? {
        [] => Ok(None),
        [stored] => Ok(Some(context.matcher.meter().copy(stored)?)),
        _ => Err(SpaceError::Inconsistent(two)),
    }
}

/// Stages the counter bump of one collection operation in `txn` and returns the order of the
/// operation. The lock keys of `txn` must cover both counter channels. Each staged counter
/// operation goes through `legacy`, which charges it in a legacy deploy (decision 45, Option P).
pub fn bump(
    txn: &mut ParSpaceTxn,
    channels: CounterChannels<'_>,
    context: &BumpContext<'_>,
    legacy: &mut LegacyChain,
) -> Result<Bump, SpaceError> {
    let meter = context.matcher.meter();
    let collection = only_record(
        txn,
        channels.collection,
        context,
        "a collection has two counter records",
    )?;
    match collection {
        None => {
            let stored = only_record(
                txn,
                channels.space,
                context,
                "a space has two space counters",
            )?
            .ok_or(SpaceError::Inconsistent("a space has no space counter"))?;
            let value = record::counter_value_metered(&stored.a, meter)?;
            let next = value.checked_add(1).ok_or(SpaceError::CounterOverflow)?;
            take(
                txn,
                channels.space,
                stored,
                value,
                RandomPurpose::SpaceCounter,
                context,
                legacy,
            )?;
            put(
                txn,
                channels.space,
                next,
                RandomPurpose::SpaceCounter,
                context,
                legacy,
            )?;
            put(
                txn,
                channels.collection,
                1,
                RandomPurpose::Counter,
                context,
                legacy,
            )?;
            Ok(Bump {
                ord: 0,
                staged: &OPEN,
            })
        }
        Some(stored) => {
            let value = record::counter_value_metered(&stored.a, meter)?;
            let next = value.checked_add(1).ok_or(SpaceError::CounterOverflow)?;
            take(
                txn,
                channels.collection,
                stored,
                value,
                RandomPurpose::Counter,
                context,
                legacy,
            )?;
            put(
                txn,
                channels.collection,
                next,
                RandomPurpose::Counter,
                context,
                legacy,
            )?;
            Ok(Bump {
                ord: value,
                staged: &STEP,
            })
        }
    }
}

// Consumes the only counter record of `channel`, at index 0 of its staged list. The record is
// the bump's own copy, so it moves into the fire.
fn take(
    txn: &mut ParSpaceTxn,
    channel: &Par,
    stored: Datum<ListParWithRandom>,
    value: i64,
    purpose: RandomPurpose,
    context: &BumpContext<'_>,
    legacy: &mut LegacyChain,
) -> Result<(), SpaceError> {
    if stored.persist {
        return Err(SpaceError::Inconsistent("a counter record is persistent"));
    }
    let meter = context.matcher.meter();
    let matched =
        context
            .matcher
            .bind(&COUNTER_PATTERN, &stored.a)?
            .ok_or(SpaceError::Inconsistent(
                "a counter record does not match the counter pattern",
            ))?;
    let random = purpose.derive(context.random).split_byte(0);
    meter.reserve_copy(&*COUNTER_CONTINUATION)?;
    meter.random_bytes(&random)?;
    let cost_authority = context
        .authority
        .continuation
        .map(|authority| meter.copy(authority))
        .transpose()?;
    let continuation = counter_continuation(value, &random, cost_authority);
    let mut channels = meter.buffer::<Par>(1)?;
    channels.push(meter.copy(channel)?);
    let mut patterns = meter.buffer::<BindPattern>(1)?;
    patterns.push(meter.copy(&*COUNTER_PATTERN)?);
    let mut data = meter.buffer::<ConsumeCandidate<Par, ListParWithRandom>>(1)?;
    let Datum { a, persist, source } = stored;
    data.push(ConsumeCandidate {
        channel: meter.copy(channel)?,
        datum: Datum {
            a: matched,
            persist,
            source,
        },
        removed_datum: a,
        datum_index: 0,
    });
    let op = SpaceConsume {
        channels,
        patterns,
        continuation,
        persist: false,
        peeks: BTreeSet::new(),
        outcome: ConsumeOutcome::Fire { data },
        before_observe: Some(record::register_introduction(
            context.authority.introduction,
            meter,
        )?),
    };
    legacy.consume(txn, op)
}

// Stores the counter record `value` on `channel`.
fn put(
    txn: &mut ParSpaceTxn,
    channel: &Par,
    value: i64,
    purpose: RandomPurpose,
    context: &BumpContext<'_>,
    legacy: &mut LegacyChain,
) -> Result<(), SpaceError> {
    let meter = context.matcher.meter();
    let introduction = context.authority.introduction;
    let cost_authority = record::cost_authority_metered(introduction.authority(), meter)?;
    let op = SpaceProduce {
        channel: meter.copy(channel)?,
        data: record::counter_record_metered(
            value,
            &purpose.derive(context.random).split_byte(1),
            cost_authority,
            meter,
        )?,
        persist: false,
        outcome: ProduceOutcome::Store,
        before_observe: Some(record::register_introduction(introduction, meter)?),
    };
    legacy.produce(txn, op)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use models::rhoapi::cost_signature::Value as CostSignatureValue;
    use models::rhoapi::{CostRegion, CostSignature};
    use models::rust::host_work::{
        HostWorkDimension, HostWorkLimit, HostWorkLimits, HostWorkUsage,
    };
    use rspace_plus_plus::rspace::errors::RSpaceError;
    use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
    use rspace_plus_plus::rspace::hashing::stable_hash_provider;
    use rspace_plus_plus::rspace::rspace::RSpace;
    use rspace_plus_plus::rspace::rspace_interface::ISpace;
    use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
    use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
    use rspace_plus_plus::rspace::space_store_handle::{SpaceOpReceipt, SpaceStoreHandle};
    use rspace_plus_plus::rspace::trace::event::{Consume, Event, IOEvent, Produce};

    use super::*;
    use crate::rust::interpreter::accounting::costs::Cost;
    use crate::rust::interpreter::accounting::RuntimeBudget;
    use crate::rust::interpreter::host_work::HostWorkBudget;
    use crate::rust::interpreter::matcher::r#match::Matcher;
    use crate::rust::interpreter::reified::costs::ScanMeter;
    use crate::rust::interpreter::spaces::agent::SpaceEnv;
    use crate::rust::interpreter::spaces::namespace::{
        self, CollectionKey, Durability, NativeSpace, SID_LEN,
    };
    use crate::rust::interpreter::spaces::record::Introduction;

    type ParRSpace = RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>;

    const SPACE: NativeSpace = NativeSpace {
        sid: [2; SID_LEN],
        durability: Durability::Durable,
    };

    fn space_counter() -> Par { namespace::gprivate(namespace::space_counter(SPACE)) }

    fn collection_counter() -> Par {
        let key = CollectionKey::from_bytes(vec![1, 2, 3]).expect("a key");
        namespace::gprivate(namespace::collection_counter(SPACE, &key))
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

    fn random() -> Blake2b512Random { Blake2b512Random::create_from_bytes(b"counter operation") }

    async fn host() -> (ParRSpace, SpaceEnv) {
        let mut stores = InMemoryStoreManager::new();
        let space = ParRSpace::create(
            stores.r_space_stores().await.expect("in-memory stores"),
            Arc::new(Box::new(Matcher)),
        )
        .expect("an in-memory space");
        let env = SpaceEnv {
            store: SpaceStoreHandle::new(space.clone()),
            budget: RuntimeBudget::new(Cost::create(1_000, "space counter test")),
            meter: ScanMeter::new(None),
        };
        (space, env)
    }

    // Stores `records` on `channel` in one committed transaction.
    async fn store(env: &SpaceEnv, channel: &Par, records: Vec<(ListParWithRandom, bool)>) {
        let mut txn = env
            .store
            .begin(std::slice::from_ref(channel))
            .await
            .expect("a transaction");
        for (data, persist) in records {
            txn.produce(SpaceProduce {
                channel: channel.clone(),
                data,
                persist,
                outcome: ProduceOutcome::Store,
                before_observe: None,
            })
            .expect("the record stages");
        }
        txn.commit().await.expect("the records commit");
    }

    fn counter(value: i64, seed: u8) -> ListParWithRandom {
        record::counter_record(value, &Blake2b512Random::create_from_bytes(&[seed]), None)
    }

    async fn run_bump(env: &SpaceEnv) -> Result<(Bump, ParSpaceTxn), SpaceError> {
        let mut txn = env
            .store
            .begin(&[space_counter(), collection_counter()])
            .await?;
        let matcher = SpaceMatcher::new(env);
        let continuation = region(1);
        let introduction = Introduction::new(&env.budget, &region(2), &env.meter)?;
        let random = random();
        let bump = bump(
            &mut txn,
            CounterChannels {
                space: &space_counter(),
                collection: &collection_counter(),
            },
            &BumpContext {
                matcher: &matcher,
                random: &random,
                authority: OperationAuthority {
                    continuation: Some(&continuation),
                    introduction: &introduction,
                },
            },
            &mut LegacyChain::new(&env.budget),
        )?;
        Ok((bump, txn))
    }

    async fn values(space: &ParRSpace, channel: &Par) -> Vec<i64> {
        space
            .get_data(channel)
            .await
            .iter()
            .map(|datum| record::counter_value(&datum.a).expect("a counter record"))
            .collect()
    }

    fn channel_of(hash: &Blake2b256Hash) -> &'static str {
        match (
            *hash == stable_hash_provider::hash(&space_counter()),
            *hash == stable_hash_provider::hash(&collection_counter()),
        ) {
            (true, _) => "S",
            (false, true) => "K",
            (false, false) => "?",
        }
    }

    fn labels(log: &[Event]) -> Vec<String> {
        log.iter()
            .map(|event| match event {
                Event::IoEvent(IOEvent::Produce(produce)) => {
                    format!("produce {}", channel_of(&produce.channel_hash))
                }
                Event::IoEvent(IOEvent::Consume(consume)) => {
                    format!("consume {}", channel_of(&consume.channel_hashes[0]))
                }
                Event::Comm(comm) => {
                    format!("comm {}", channel_of(&comm.consume.channel_hashes[0]))
                }
            })
            .collect()
    }

    #[tokio::test]
    async fn space_counter_first_operation_bumps_the_space_counter() {
        let (space, env) = host().await;
        store(&env, &space_counter(), vec![(counter(5, 0), false)]).await;
        space.take_event_log().await;
        let (bump, mut txn) = run_bump(&env).await.expect("the bump stages");
        assert_eq!(bump, Bump {
            ord: 0,
            staged: &[CounterOp::Take, CounterOp::Put, CounterOp::Put],
        });
        let receipt = txn.commit().await.expect("the bump commits");
        assert!(matches!(receipt.ops.as_slice(), [
            SpaceOpReceipt::Fired { .. },
            SpaceOpReceipt::Stored { .. },
            SpaceOpReceipt::Stored { .. }
        ]));
        assert_eq!(values(&space, &space_counter()).await, vec![6]);
        assert_eq!(values(&space, &collection_counter()).await, vec![1]);
        let log = space.take_event_log().await;
        assert_eq!(labels(&log), vec![
            "consume S",
            "comm S",
            "produce S",
            "produce K"
        ]);
        let take = Consume::create(
            &vec![space_counter()],
            &vec![counter_pattern()],
            &counter_continuation(
                5,
                &RandomPurpose::SpaceCounter.derive(&random()).split_byte(0),
                Some(region(1)),
            ),
            false,
        );
        let space_put = Produce::create(
            &space_counter(),
            &record::counter_record(
                6,
                &RandomPurpose::SpaceCounter.derive(&random()).split_byte(1),
                Some(region(2)),
            ),
            false,
        );
        let collection_put = Produce::create(
            &collection_counter(),
            &record::counter_record(
                1,
                &RandomPurpose::Counter.derive(&random()).split_byte(1),
                Some(region(2)),
            ),
            false,
        );
        assert_eq!(log[0], Event::IoEvent(IOEvent::Consume(take)));
        assert_eq!(log[2], Event::IoEvent(IOEvent::Produce(space_put)));
        assert_eq!(log[3], Event::IoEvent(IOEvent::Produce(collection_put)));
        match &log[1] {
            Event::Comm(comm) => {
                assert!(comm.peeks.is_empty(), "a bump never peeks");
                assert_eq!(comm.produces.len(), 1);
            }
            other => panic!("the bump fires its counter: {other:?}"),
        }
    }

    #[tokio::test]
    async fn space_counter_bump_consumes_and_reproduces_the_collection_counter() {
        let (space, env) = host().await;
        store(&env, &space_counter(), vec![(counter(9, 0), false)]).await;
        store(&env, &collection_counter(), vec![(counter(3, 1), false)]).await;
        let stored = space.get_data(&collection_counter()).await[0].clone();
        space.take_event_log().await;
        let (bump, mut txn) = run_bump(&env).await.expect("the bump stages");
        assert_eq!(bump, Bump {
            ord: 3,
            staged: &[CounterOp::Take, CounterOp::Put],
        });
        txn.commit().await.expect("the bump commits");
        assert_eq!(values(&space, &collection_counter()).await, vec![4]);
        assert_eq!(
            values(&space, &space_counter()).await,
            vec![9],
            "only a first operation bumps the space counter"
        );
        let log = space.take_event_log().await;
        assert_eq!(labels(&log), vec!["consume K", "comm K", "produce K"]);
        match &log[1] {
            Event::Comm(comm) => assert_eq!(comm.produces, vec![stored.source]),
            other => panic!("the bump fires the stored counter: {other:?}"),
        }
        let take = Consume::create(
            &vec![collection_counter()],
            &vec![counter_pattern()],
            &counter_continuation(
                3,
                &RandomPurpose::Counter.derive(&random()).split_byte(0),
                Some(region(1)),
            ),
            false,
        );
        assert_eq!(log[0], Event::IoEvent(IOEvent::Consume(take)));
    }

    #[test]
    fn space_counter_continuation_identity_is_new_for_each_value() {
        let random = random();
        let identity = |value| {
            Consume::create(
                &vec![collection_counter()],
                &vec![counter_pattern()],
                &counter_continuation(value, &random, None),
                false,
            )
        };
        assert_ne!(identity(3), identity(4));
        assert_eq!(identity(3), identity(3));
        assert_eq!(counter_pattern().free_count, 2);
        assert_eq!(counter_pattern().patterns.len(), 2);
    }

    #[tokio::test]
    async fn space_counter_rejects_corrupt_counters() {
        let datum_record = record::wrap_datum(0, &counter(1, 9));
        let cases: Vec<(
            Vec<(ListParWithRandom, bool)>,
            Vec<(ListParWithRandom, bool)>,
            SpaceError,
        )> = vec![
            (
                vec![(counter(1, 0), false)],
                vec![(counter(1, 1), false), (counter(2, 2), false)],
                SpaceError::Inconsistent("a collection has two counter records"),
            ),
            (
                vec![(counter(1, 0), false)],
                vec![(counter(1, 1), true)],
                SpaceError::Inconsistent("a counter record is persistent"),
            ),
            (
                vec![(counter(1, 0), false)],
                vec![(counter(i64::MAX, 1), false)],
                SpaceError::CounterOverflow,
            ),
            (
                vec![(counter(1, 0), false)],
                vec![(datum_record, false)],
                SpaceError::Inconsistent("a malformed counter record"),
            ),
            (
                Vec::new(),
                Vec::new(),
                SpaceError::Inconsistent("a space has no space counter"),
            ),
            (
                vec![(counter(1, 0), false), (counter(1, 1), false)],
                Vec::new(),
                SpaceError::Inconsistent("a space has two space counters"),
            ),
            (
                vec![(counter(i64::MAX, 0), false)],
                Vec::new(),
                SpaceError::CounterOverflow,
            ),
        ];
        for (space_records, collection_records, expected) in cases {
            let (space, env) = host().await;
            store(&env, &space_counter(), space_records).await;
            store(&env, &collection_counter(), collection_records).await;
            let before = (
                record_count(&space, &space_counter()).await,
                record_count(&space, &collection_counter()).await,
            );
            space.take_event_log().await;
            let error = run_bump(&env).await.map(|(bump, _)| bump);
            assert_eq!(error, Err(expected.clone()));
            assert!(space.take_event_log().await.is_empty(), "{expected:?}");
            assert_eq!(
                (
                    record_count(&space, &space_counter()).await,
                    record_count(&space, &collection_counter()).await,
                ),
                before
            );
        }
    }

    async fn record_count(space: &ParRSpace, channel: &Par) -> usize {
        space.get_data(channel).await.len()
    }

    /// The dimensions that a space job charges (decision 42).
    const CHARGED: [HostWorkDimension; 3] = [
        HostWorkDimension::VerificationOperations,
        HostWorkDimension::VerificationBytes,
        HostWorkDimension::SearchStateBytes,
    ];

    /// A host-work budget with the limit `limit` in every dimension.
    fn uniform(limit: u64) -> HostWorkBudget {
        HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(limit)))
    }

    /// Stages and commits one first bump with the meter `meter`, and returns the bump result.
    async fn bump_with(meter: ScanMeter) -> (ParRSpace, Result<Bump, SpaceError>) {
        let (space, mut env) = host().await;
        env.meter = meter;
        store(&env, &space_counter(), vec![(counter(0, 0), false)]).await;
        space.take_event_log().await;
        let result = match run_bump(&env).await {
            Ok((bump, mut txn)) => {
                txn.commit().await.expect("the bump commits");
                Ok(bump)
            }
            Err(error) => Err(error),
        };
        (space, result)
    }

    #[tokio::test]
    async fn space_counter_bump_reserves_its_work_on_the_branch_dimensions() {
        let host_work = uniform(100_000_000);
        let (space, result) = bump_with(ScanMeter::new(Some(host_work.clone()))).await;
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(
            host_work.usage(HostWorkDimension::SearchCandidates),
            HostWorkUsage::ZERO,
            "the branch charges candidates only in its funding search"
        );
        for dimension in CHARGED {
            assert!(
                host_work.usage(dimension) > HostWorkUsage::ZERO,
                "{dimension:?}"
            );
        }
        assert_eq!(values(&space, &collection_counter()).await, vec![1]);
        let (_, rejected) = bump_with(ScanMeter::new(Some(uniform(0)))).await;
        assert_eq!(
            rejected,
            Err(SpaceError::Host(RSpaceError::HostWorkRejected))
        );
    }

    #[tokio::test]
    async fn space_counter_bump_cut_budget_rejects_without_effect() {
        let measured = uniform(100_000_000);
        let (_, result) = bump_with(ScanMeter::new(Some(measured.clone()))).await;
        assert!(result.is_ok(), "{result:?}");
        let mut exact = HostWorkLimits::uniform(HostWorkLimit::new(u64::MAX));
        for dimension in CHARGED {
            exact.set(
                dimension,
                HostWorkLimit::new(measured.usage(dimension).get()),
            );
        }
        let (_, accepted) = bump_with(ScanMeter::new(Some(HostWorkBudget::new(exact)))).await;
        assert!(accepted.is_ok(), "the exact usage suffices: {accepted:?}");
        for dimension in CHARGED {
            let mut cut = exact;
            cut.set(
                dimension,
                HostWorkLimit::new(measured.usage(dimension).get() - 1),
            );
            let (space, rejected) = bump_with(ScanMeter::new(Some(HostWorkBudget::new(cut)))).await;
            assert_eq!(
                rejected,
                Err(SpaceError::Host(RSpaceError::HostWorkRejected)),
                "{dimension:?}"
            );
            assert!(space.take_event_log().await.is_empty(), "{dimension:?}");
            assert_eq!(values(&space, &space_counter()).await, vec![0]);
            assert!(values(&space, &collection_counter()).await.is_empty());
        }
    }
}
