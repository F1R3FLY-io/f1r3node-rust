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

use std::collections::BTreeSet;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::tagged_continuation::TaggedCont;
use models::rhoapi::{
    BindPattern, CostAuthority, ListParWithRandom, Par, ParWithRandom, TaggedContinuation,
};
use models::rust::utils::{new_freevar_par, new_gint_par};
use rspace_plus_plus::rspace::internal::Datum;
use rspace_plus_plus::rspace::space_store_handle::{
    bind_candidate, ConsumeOutcome, ProduceOutcome, SpaceConsume, SpaceProduce,
};

use super::agent::ParSpaceTxn;
use super::errors::SpaceError;
use super::matching::SpaceMatcher;
use super::namespace::RandomPurpose;
use super::record::{self, OperationAuthority};
use crate::rust::interpreter::accounting::RuntimeBudget;

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
    TaggedContinuation {
        tagged_cont: Some(TaggedCont::ParBody(ParWithRandom {
            body: Some(new_gint_par(value, Vec::new(), false)),
            random_state: random.to_bytes(),
        })),
        guard: None,
        cost_authority,
    }
}

/// An operation that a bump stages.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CounterOp {
    /// The consume of a counter record. It fires.
    Take,
    /// The produce of a counter record. It stores.
    Put,
}

/// The bump of one collection operation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bump {
    /// The order of the operation: the value of the collection counter before the bump, or 0 for
    /// the first operation on the collection.
    pub ord: i64,
    /// The operations that the bump staged, in staging order.
    pub staged: Vec<CounterOp>,
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
    /// The matcher of the job.
    pub matcher: &'a SpaceMatcher<'a>,
    /// The random state of the operation.
    pub random: &'a Blake2b512Random,
    /// The authorities of the operation (P4 design B.7).
    pub authority: &'a OperationAuthority,
    /// The phlo budget of the deploy.
    pub budget: &'a RuntimeBudget,
}

/// Stages the counter bump of one collection operation in `txn` and returns the order of the
/// operation. The lock keys of `txn` must cover both counter channels.
pub fn bump(
    txn: &mut ParSpaceTxn,
    channels: CounterChannels<'_>,
    context: &BumpContext<'_>,
) -> Result<Bump, SpaceError> {
    let collection = txn.data(channels.collection)?.to_vec();
    match collection.as_slice() {
        [] => {
            let space = txn.data(channels.space)?.to_vec();
            let stored = match space.as_slice() {
                [stored] => stored,
                [] => return Err(SpaceError::Inconsistent("a space has no space counter")),
                _ => return Err(SpaceError::Inconsistent("a space has two space counters")),
            };
            let value = record::counter_value(&stored.a)?;
            let next = value.checked_add(1).ok_or(SpaceError::CounterOverflow)?;
            take(
                txn,
                channels.space,
                stored,
                value,
                RandomPurpose::SpaceCounter,
                context,
            )?;
            put(
                txn,
                channels.space,
                next,
                RandomPurpose::SpaceCounter,
                context,
            )?;
            put(txn, channels.collection, 1, RandomPurpose::Counter, context)?;
            Ok(Bump {
                ord: 0,
                staged: vec![CounterOp::Take, CounterOp::Put, CounterOp::Put],
            })
        }
        [stored] => {
            let value = record::counter_value(&stored.a)?;
            let next = value.checked_add(1).ok_or(SpaceError::CounterOverflow)?;
            take(
                txn,
                channels.collection,
                stored,
                value,
                RandomPurpose::Counter,
                context,
            )?;
            put(
                txn,
                channels.collection,
                next,
                RandomPurpose::Counter,
                context,
            )?;
            Ok(Bump {
                ord: value,
                staged: vec![CounterOp::Take, CounterOp::Put],
            })
        }
        _ => Err(SpaceError::Inconsistent(
            "a collection has two counter records",
        )),
    }
}

// Consumes the only counter record of `channel`, at index 0 of its staged list.
fn take(
    txn: &mut ParSpaceTxn,
    channel: &Par,
    stored: &Datum<ListParWithRandom>,
    value: i64,
    purpose: RandomPurpose,
    context: &BumpContext<'_>,
) -> Result<(), SpaceError> {
    if stored.persist {
        return Err(SpaceError::Inconsistent("a counter record is persistent"));
    }
    let pattern = counter_pattern();
    let matched = context
        .matcher
        .bind(&pattern, &stored.a)?
        .ok_or(SpaceError::Inconsistent(
            "a counter record does not match the counter pattern",
        ))?;
    let continuation = counter_continuation(
        value,
        &purpose.derive(context.random).split_byte(0),
        context.authority.continuation.clone(),
    );
    txn.consume(SpaceConsume {
        channels: vec![channel.clone()],
        patterns: vec![pattern],
        continuation,
        persist: false,
        peeks: BTreeSet::new(),
        outcome: ConsumeOutcome::Fire {
            data: vec![bind_candidate(channel.clone(), stored, matched, 0)],
        },
        before_observe: Some(record::register_introduction(
            context.budget,
            &context.authority.introduction,
        )),
    })?;
    Ok(())
}

// Stores the counter record `value` on `channel`.
fn put(
    txn: &mut ParSpaceTxn,
    channel: &Par,
    value: i64,
    purpose: RandomPurpose,
    context: &BumpContext<'_>,
) -> Result<(), SpaceError> {
    txn.produce(SpaceProduce {
        channel: channel.clone(),
        data: record::counter_record(
            value,
            &purpose.derive(context.random).split_byte(1),
            record::cost_authority(&context.authority.introduction),
        ),
        persist: false,
        outcome: ProduceOutcome::Store,
        before_observe: Some(record::register_introduction(
            context.budget,
            &context.authority.introduction,
        )),
    })?;
    Ok(())
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
    use crate::rust::interpreter::host_work::HostWorkBudget;
    use crate::rust::interpreter::matcher::r#match::Matcher;
    use crate::rust::interpreter::reified::costs::ScanMeter;
    use crate::rust::interpreter::spaces::agent::SpaceEnv;
    use crate::rust::interpreter::spaces::namespace::{
        self, CollectionKey, Durability, NativeSpace, SID_LEN,
    };

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

    fn authority() -> OperationAuthority {
        OperationAuthority {
            continuation: Some(region(1)),
            introduction: region(2),
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
        let authority = authority();
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
                authority: &authority,
                budget: &env.budget,
            },
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
            staged: vec![CounterOp::Take, CounterOp::Put, CounterOp::Put],
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
            staged: vec![CounterOp::Take, CounterOp::Put],
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

    #[tokio::test]
    async fn space_counter_bump_matches_with_the_metered_matcher() {
        let (space, mut env) = host().await;
        // One match of a counter record scans about 1.2 million state bytes in the matcher's
        // backing model, so the budget covers a few matches.
        let host_work =
            HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(100_000_000)));
        env.meter = ScanMeter::new(Some(host_work.clone()));
        store(&env, &space_counter(), vec![(counter(0, 0), false)]).await;
        let (_, mut txn) = run_bump(&env).await.expect("the bump stages");
        txn.commit().await.expect("the bump commits");
        assert_eq!(
            host_work.usage(HostWorkDimension::SearchCandidates),
            HostWorkUsage::new(1)
        );
        assert!(host_work.usage(HostWorkDimension::SearchStateBytes) > HostWorkUsage::ZERO);
        assert_eq!(values(&space, &collection_counter()).await, vec![1]);
        let rejecting = HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(0)));
        env.meter = ScanMeter::new(Some(rejecting));
        assert_eq!(
            run_bump(&env).await.map(|(bump, _)| bump),
            Err(SpaceError::Host(RSpaceError::HostWorkRejected))
        );
    }
}
