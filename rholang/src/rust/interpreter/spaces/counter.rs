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
//! The bump copies only the counter record that it consumes, and it moves that copy into the fire.

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
// Disabled by decision 90 (decision 69): Option P is gone.
// use crate::rust::interpreter::reified::legacy_charges::LegacyChain;

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
// Changed by decision 124 (plan v4 C7.11, rule CR-33, ruling RADD2v2 6.6): the finish rows of a
// cut row bind the token with it (P1). The text before:
// static COUNTER_PATTERN: LazyLock<BindPattern> = LazyLock::new(counter_pattern);
pub(crate) static COUNTER_PATTERN: LazyLock<BindPattern> = LazyLock::new(counter_pattern);

/// An operation that a bump stages.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CounterOp {
    /// The consume of a counter record. It fires.
    Take,
    /// The produce of a counter record. It stores.
    Put,
}

// Retired by plan v4 errata (V4-19): `bump` reports each staged operation to its sink right
// after its stage call, so no code reads a list of the operations of a bump.
// /// The operations of the first bump of a collection: the take and the put of the space
// /// counter, and the put of the new collection counter.
// const OPEN: [CounterOp; 3] = [CounterOp::Take, CounterOp::Put, CounterOp::Put];
//
// /// The operations of every later bump: the take and the put of the collection counter.
// const STEP: [CounterOp; 2] = [CounterOp::Take, CounterOp::Put];

/// The bump of one collection operation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bump {
    /// The order of the operation: the value of the collection counter before the bump, or 0 for
    /// the first operation on the collection.
    pub ord: i64,
    // Retired by plan v4 errata (V4-19): the sink of `bump` lists each staged operation right
    // after its stage call, so no code reads this list.
    // /// The operations that the bump staged, in staging order.
    // pub staged: &'static [CounterOp],
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
    pub matcher: &'a SpaceMatcher,
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
    two: &'static str,
) -> Result<Option<Datum<ListParWithRandom>>, SpaceError> {
    match txn.data(channel)? {
        [] => Ok(None),
        [stored] => Ok(Some(stored.clone())),
        _ => Err(SpaceError::Inconsistent(two)),
    }
}

/// Stages the counter bump of one collection operation in `txn` and returns the order of the
/// operation. The lock keys of `txn` must cover both counter channels. Changed by decision 90
/// (decision 69): the parameter `legacy: &mut LegacyChain`, which charged each staged counter
/// operation in a legacy deploy (decision 45, Option P), is gone.
///
/// Added by plan v4 errata (V4-19, rule CR-6): the sink `staged` gets each operation right after
/// its stage call succeeds, and never before that call: take, put, put for the first operation
/// on a collection, and take, put for every later one. So when a record end of native replay
/// stops the bump, the staged record of the caller lists the operations that `txn` holds.
pub fn bump(
    txn: &mut ParSpaceTxn,
    channels: CounterChannels<'_>,
    context: &BumpContext<'_>,
    // Disabled by decision 90 (decision 69): Option P is gone.
    // legacy: &mut LegacyChain,
    // Added by plan v4 errata (V4-19): the staged record of the caller.
    staged: &mut impl FnMut(CounterOp),
) -> Result<Bump, SpaceError> {
    let collection = only_record(
        txn,
        channels.collection,
        "a collection has two counter records",
    )?;
    match collection {
        None => {
            let stored = only_record(txn, channels.space, "a space has two space counters")?
                .ok_or(SpaceError::Inconsistent("a space has no space counter"))?;
            let value = record::counter_value(&stored.a)?;
            let next = value.checked_add(1).ok_or(SpaceError::CounterOverflow)?;
            take(
                txn,
                channels.space,
                stored,
                value,
                RandomPurpose::SpaceCounter,
                context,
                // Disabled by decision 90 (decision 69): Option P is gone.
                // legacy,
            )?;
            // Added by plan v4 errata (V4-19): the take of the space counter staged.
            staged(CounterOp::Take);
            put(
                txn,
                channels.space,
                next,
                RandomPurpose::SpaceCounter,
                context,
                // Disabled by decision 90 (decision 69): Option P is gone.
                // legacy,
            )?;
            // Added by plan v4 errata (V4-19): the put of the space counter staged.
            staged(CounterOp::Put);
            put(
                txn,
                channels.collection,
                1,
                RandomPurpose::Counter,
                context,
                // Disabled by decision 90 (decision 69): Option P is gone.
                // legacy,
            )?;
            // Added by plan v4 errata (V4-19): the put of the collection counter staged.
            staged(CounterOp::Put);
            // Changed by plan v4 errata (V4-19): the sink listed the staged operations.
            // Ok(Bump {
            //     ord: 0,
            //     staged: &OPEN,
            // })
            Ok(Bump { ord: 0 })
        }
        Some(stored) => {
            // Changed by decision 124 (plan v4 C7.11, rule CR-33, ruling RADD2v2 6.6): the take
            // and the put come from `collection_bump`, which a finish row shares (P1). The bump
            // stages them in the order of the text before, and the sink gets each one right after
            // its stage call (V4-19). The text before:
            // ```
            // let value = record::counter_value(&stored.a)?;
            // let next = value.checked_add(1).ok_or(SpaceError::CounterOverflow)?;
            // take(
            //     txn,
            //     channels.collection,
            //     stored,
            //     value,
            //     RandomPurpose::Counter,
            //     context,
            //     legacy,
            // )?;
            // put(
            //     txn,
            //     channels.collection,
            //     next,
            //     RandomPurpose::Counter,
            //     context,
            //     legacy,
            // )?;
            // ```
            let value = record::counter_value(&stored.a)?;
            let matched = context.matcher.bind(&COUNTER_PATTERN, &stored.a);
            let (take_op, put_op) = collection_bump(
                channels.collection,
                stored,
                matched,
                context.random,
                context.authority,
            )?;
            txn.consume(take_op)?;
            // Added by plan v4 errata (V4-19): the take of the collection counter staged.
            staged(CounterOp::Take);
            txn.produce(put_op)?;
            // Added by plan v4 errata (V4-19): the put of the collection counter staged.
            staged(CounterOp::Put);
            // Changed by plan v4 errata (V4-19): the sink listed the staged operations.
            // Ok(Bump {
            //     ord: value,
            //     staged: &STEP,
            // })
            Ok(Bump { ord: value })
        }
    }
}

/// Consumes the only counter record of `channel`, at index 0 of its staged list. The record is
/// the bump's own copy, so it moves into the fire. The random states derive from `purpose`:
/// `split_byte(0)` for the continuation. The allocator of an Array or Vector space and the token
/// of a PathMap space take their records with it too (P5d, P5e).
pub(crate) fn take(
    txn: &mut ParSpaceTxn,
    channel: &Par,
    stored: Datum<ListParWithRandom>,
    value: i64,
    purpose: RandomPurpose,
    context: &BumpContext<'_>,
    // Disabled by decision 90 (decision 69): Option P is gone.
    // legacy: &mut LegacyChain,
) -> Result<(), SpaceError> {
    // Changed by decision 124 (plan v4 C7.11, rule CR-33, ruling RADD2v2 6.6): the request comes
    // from `take_request`, which a finish row shares (P1). The bind comes first and reads the
    // record in place, and `take_request` checks the persistence before the bind, so the errors
    // keep their order. The text before:
    // ```
    // if stored.persist {
    //     return Err(SpaceError::Inconsistent("a counter record is persistent"));
    // }
    // let matched =
    //     context
    //         .matcher
    //         .bind(&COUNTER_PATTERN, &stored.a)
    //         .ok_or(SpaceError::Inconsistent(
    //             "a counter record does not match the counter pattern",
    //         ))?;
    // let random = purpose.derive(context.random).split_byte(0);
    // let cost_authority = context.authority.continuation.cloned();
    // let continuation = counter_continuation(value, &random, cost_authority);
    // let mut channels = Vec::<Par>::with_capacity(1);
    // channels.push(channel.clone());
    // let mut patterns = Vec::<BindPattern>::with_capacity(1);
    // patterns.push((*COUNTER_PATTERN).clone());
    // let mut data = Vec::<ConsumeCandidate<Par, ListParWithRandom>>::with_capacity(1);
    // let Datum { a, persist, source } = stored;
    // data.push(ConsumeCandidate {
    //     channel: channel.clone(),
    //     datum: Datum {
    //         a: matched,
    //         persist,
    //         source,
    //     },
    //     removed_datum: a,
    //     datum_index: 0,
    // });
    // let op = SpaceConsume {
    //     channels,
    //     patterns,
    //     continuation,
    //     persist: false,
    //     peeks: BTreeSet::new(),
    //     outcome: ConsumeOutcome::Fire { data },
    //     before_observe: Some(record::register_introduction(
    //         context.authority.introduction,
    //     )),
    // };
    // legacy.consume(txn, op)
    // ```
    let matched = context.matcher.bind(&COUNTER_PATTERN, &stored.a);
    let op = take_request(
        channel,
        stored,
        matched,
        value,
        purpose,
        context.random,
        context.authority,
    )?;
    txn.consume(op)?;
    Ok(())
}

/// Stores the counter record `value` on `channel`, with the random state
/// `purpose.derive(..).split_byte(1)`. The allocator and the PathMap token use it too.
pub(crate) fn put(
    txn: &mut ParSpaceTxn,
    channel: &Par,
    value: i64,
    purpose: RandomPurpose,
    context: &BumpContext<'_>,
    // Disabled by decision 90 (decision 69): Option P is gone.
    // legacy: &mut LegacyChain,
) -> Result<(), SpaceError> {
    // Changed by decision 124 (plan v4 C7.11, rule CR-33, ruling RADD2v2 6.6): the request comes
    // from `put_request`, which a finish row shares (P1). The text before:
    // ```
    // let introduction = context.authority.introduction;
    // let cost_authority = record::cost_authority(introduction.authority());
    // let op = SpaceProduce {
    //     channel: channel.clone(),
    //     data: record::counter_record(
    //         value,
    //         &purpose.derive(context.random).split_byte(1),
    //         cost_authority,
    //     ),
    //     persist: false,
    //     outcome: ProduceOutcome::Store,
    //     before_observe: Some(record::register_introduction(introduction)),
    // };
    // legacy.produce(txn, op)
    // ```
    let op = put_request(channel, value, purpose, context.random, context.authority)?;
    txn.produce(op)?;
    Ok(())
}

/// Added by decision 124 (plan v4 C7.11, rule CR-33, ruling RADD2v2 6.6): the consume that takes
/// the only counter record `stored` of `channel` with the value `value`, as `take` stages it and
/// as a finish row of a cut row commits it (P1). `matched` is the bind of the counter pattern on
/// the record, and the random states derive from `purpose` and `random`: `split_byte(0)` for the
/// continuation. It stages nothing.
pub(crate) fn take_request(
    channel: &Par,
    stored: Datum<ListParWithRandom>,
    matched: Option<ListParWithRandom>,
    value: i64,
    purpose: RandomPurpose,
    random: &Blake2b512Random,
    authority: OperationAuthority<'_>,
) -> Result<SpaceConsume<Par, BindPattern, ListParWithRandom, TaggedContinuation>, SpaceError> {
    if stored.persist {
        return Err(SpaceError::Inconsistent("a counter record is persistent"));
    }
    let matched = matched.ok_or(SpaceError::Inconsistent(
        "a counter record does not match the counter pattern",
    ))?;
    let derived = purpose.derive(random).split_byte(0);
    // Changed by decision 124 (plan v2 C7.2, decision 99): `K(v)` takes the machinery seal, and
    // the payer introduction registers the take (CR-15). The text before:
    // let cost_authority = context.authority.continuation.cloned();
    let introduction = authority.introduction;
    let cost_authority = introduction.seal_of(authority.continuation)?;
    let payer = introduction.payer();
    let continuation = counter_continuation(value, &derived, cost_authority);
    let mut channels = Vec::<Par>::with_capacity(1);
    channels.push(channel.clone());
    let mut patterns = Vec::<BindPattern>::with_capacity(1);
    patterns.push((*COUNTER_PATTERN).clone());
    let mut data = Vec::<ConsumeCandidate<Par, ListParWithRandom>>::with_capacity(1);
    let Datum { a, persist, source } = stored;
    data.push(ConsumeCandidate {
        channel: channel.clone(),
        datum: Datum {
            a: matched,
            persist,
            source,
        },
        removed_datum: a,
        datum_index: 0,
    });
    Ok(SpaceConsume {
        channels,
        patterns,
        continuation,
        persist: false,
        peeks: BTreeSet::new(),
        outcome: ConsumeOutcome::Fire { data },
        // Changed by decision 124 (plan v2 C7.2): the payer introduction. The text before:
        // ```
        // before_observe: Some(record::register_introduction(
        //     context.authority.introduction,
        // )),
        // ```
        before_observe: Some(record::register_introduction(&payer)),
        // Added by decision 92: the random state of `K` derives from `BumpContext::random`, the
        // random state of the operation, so the take is the job's own.
        own: true,
        // Added by decision 123 (plan v4, V3-9): no row search.
        search: None,
    })
}

/// Added by decision 124 (plan v4 C7.11, rule CR-33, ruling RADD2v2 6.6): the produce that stores
/// the counter record `value` on `channel`, as `put` stages it and as a finish row of a cut row
/// commits it (P1). The random state of the record is `purpose.derive(random).split_byte(1)`.
/// It stages nothing.
pub(crate) fn put_request(
    channel: &Par,
    value: i64,
    purpose: RandomPurpose,
    random: &Blake2b512Random,
    authority: OperationAuthority<'_>,
) -> Result<SpaceProduce<Par, BindPattern, ListParWithRandom, TaggedContinuation>, SpaceError> {
    // Changed by decision 124 (plan v2 C7.2, decision 99): the record takes the machinery seal,
    // and the payer introduction registers the put (CR-15). The text before:
    // ```
    // let introduction = context.authority.introduction;
    // let cost_authority = record::cost_authority(introduction.authority());
    // ```
    let operation = authority.introduction;
    let sealed = record::cost_authority(operation.authority());
    let cost_authority = operation.seal_of(sealed.as_ref())?;
    let introduction = operation.payer();
    Ok(SpaceProduce {
        channel: channel.clone(),
        data: record::counter_record(value, &purpose.derive(random).split_byte(1), cost_authority),
        persist: false,
        outcome: ProduceOutcome::Store,
        // Changed by decision 124 (plan v2 C7.2): the payer introduction is owned here. The
        // text before:
        // before_observe: Some(record::register_introduction(introduction)),
        before_observe: Some(record::register_introduction(&introduction)),
        // Added by decision 92: the random state of the record derives from
        // `BumpContext::random`, the random state of the operation, so the put is the job's own.
        own: true,
        // Added by decision 123 (plan v4, V3-9): no row search.
        search: None,
    })
}

/// Added by decision 124 (plan v4 C7.11, rule CR-33, ruling RADD2v2 6.6): the take and the put
/// of the bump of the stored collection counter `stored` of `channel`, as the `Some` arm of
/// `bump` stages them and as the finish rows of a cut row commit them (P1). `matched` is the bind
/// of the counter pattern on the record. The value of the put is the stored value plus 1, and a
/// counter at `i64::MAX` fails with `CounterOverflow`. It stages nothing.
pub(crate) fn collection_bump(
    channel: &Par,
    stored: Datum<ListParWithRandom>,
    matched: Option<ListParWithRandom>,
    random: &Blake2b512Random,
    authority: OperationAuthority<'_>,
) -> Result<
    (
        SpaceConsume<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
        SpaceProduce<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
    ),
    SpaceError,
> {
    let value = record::counter_value(&stored.a)?;
    let next = value.checked_add(1).ok_or(SpaceError::CounterOverflow)?;
    let take = take_request(
        channel,
        stored,
        matched,
        value,
        RandomPurpose::Counter,
        random,
        authority,
    )?;
    let put = put_request(channel, next, RandomPurpose::Counter, random, authority)?;
    Ok((take, put))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use models::rhoapi::cost_signature::Value as CostSignatureValue;
    use models::rhoapi::{CostRegion, CostSignature};
    use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
    use rspace_plus_plus::rspace::hashing::stable_hash_provider;
    use rspace_plus_plus::rspace::rspace::RSpace;
    use rspace_plus_plus::rspace::rspace_interface::ISpace;
    use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
    use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
    // Changed by plan v4 errata (V4-19): UT-5 expects the record end of a scripted replay.
    // use rspace_plus_plus::rspace::space_store_handle::{SpaceOpReceipt, SpaceStoreHandle};
    use rspace_plus_plus::rspace::space_store_handle::{
        SpaceOpReceipt, SpaceStoreHandle, SpaceTxnError,
    };
    use rspace_plus_plus::rspace::trace::event::{Consume, Event, IOEvent, Produce};

    use super::*;
    // Added by decision 124 (plan v2 C7.2, decision 99): the expected rows take the machinery
    // seal.
    use crate::rust::interpreter::accounting::authority::system_residue_authority_with_caller;
    use crate::rust::interpreter::accounting::costs::Cost;
    use crate::rust::interpreter::accounting::RuntimeBudget;
    use crate::rust::interpreter::matcher::r#match::Matcher;
    use crate::rust::interpreter::spaces::agent::SpaceEnv;
    use crate::rust::interpreter::spaces::namespace::{
        self, CollectionKey, Durability, NativeSpace, SID_LEN,
    };
    use crate::rust::interpreter::spaces::record::Introduction;
    // Added by plan v4 errata (V4-19): UT-5 ends the record inside a bump.
    use crate::rust::interpreter::spaces::test_support::ScriptedReplay;

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
                // Added by decision 90 (DR-135): an unpriced region, as outside native
                // execution.
                price: 0,
                caller: None,
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
            volatile: Default::default(),
            origin: Default::default(),
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
                own: false,
                // Added by decision 123 (plan v4, V3-9): no row search.
                search: None,
            })
            .expect("the record stages");
        }
        txn.commit().await.expect("the records commit");
    }

    fn counter(value: i64, seed: u8) -> ListParWithRandom {
        record::counter_record(value, &Blake2b512Random::create_from_bytes(&[seed]), None)
    }

    // Changed by plan v4 errata (V4-19): the result also names the operations that the sink of
    // the bump collected, in staging order.
    // async fn run_bump(env: &SpaceEnv) -> Result<(Bump, ParSpaceTxn), SpaceError> {
    async fn run_bump(env: &SpaceEnv) -> Result<(Bump, Vec<CounterOp>, ParSpaceTxn), SpaceError> {
        let mut txn = env
            .store
            .begin(&[space_counter(), collection_counter()])
            .await?;
        let matcher = SpaceMatcher::new(env);
        let continuation = region(1);
        let introduction = Introduction::new(&env.budget, &region(2));
        let random = random();
        // Added by plan v4 errata (V4-19): a bump stages at most three operations.
        let mut staged = Vec::<CounterOp>::with_capacity(3);
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
            // Disabled by decision 90 (decision 69): Option P is gone.
            // &mut LegacyChain::new(&env.budget),
            // Added by plan v4 errata (V4-19): the sink collects each staged operation.
            &mut |kind| staged.push(kind),
        )?;
        // Changed by plan v4 errata (V4-19): the result names the collected operations.
        // Ok((bump, txn))
        Ok((bump, staged, txn))
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
        // Changed by plan v4 errata (V4-19): the sink of the bump lists its staged operations.
        // let (bump, mut txn) = run_bump(&env).await.expect("the bump stages");
        // assert_eq!(bump, Bump {
        //     ord: 0,
        //     staged: &[CounterOp::Take, CounterOp::Put, CounterOp::Put],
        // });
        let (bump, staged, mut txn) = run_bump(&env).await.expect("the bump stages");
        assert_eq!(bump, Bump { ord: 0 });
        assert_eq!(staged, vec![
            CounterOp::Take,
            CounterOp::Put,
            CounterOp::Put
        ]);
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
                // Changed by decision 124 (plan v2 C7.2, decision 99): the machinery seal. The
                // text before:
                // Some(region(1)),
                Some(
                    system_residue_authority_with_caller(&region(1), &env.budget.deploy_id(), None)
                        .expect("the machinery seal builds"),
                ),
            ),
            false,
        );
        let space_put = Produce::create(
            &space_counter(),
            &record::counter_record(
                6,
                &RandomPurpose::SpaceCounter.derive(&random()).split_byte(1),
                // Changed by decision 124 (plan v2 C7.2, decision 99): the machinery seal. The
                // text before:
                // Some(region(2)),
                Some(
                    system_residue_authority_with_caller(&region(2), &env.budget.deploy_id(), None)
                        .expect("the machinery seal builds"),
                ),
            ),
            false,
        );
        let collection_put = Produce::create(
            &collection_counter(),
            &record::counter_record(
                1,
                &RandomPurpose::Counter.derive(&random()).split_byte(1),
                // Changed by decision 124 (plan v2 C7.2, decision 99): the machinery seal. The
                // text before:
                // Some(region(2)),
                Some(
                    system_residue_authority_with_caller(&region(2), &env.budget.deploy_id(), None)
                        .expect("the machinery seal builds"),
                ),
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

    /// Added by decision 124 (plan v2 C7.2, decision 99, rule CR-15): the take and the puts of a
    /// bump register the payer introduction of the operation. The continuation of the take and
    /// the record of each put carry the machinery seal of an authority with a region.
    #[tokio::test]
    async fn counter_take_and_put_take_the_payer_introduction() {
        use rspace_plus_plus::rspace::rspace_interface::RSpaceOperationSource;

        let (space, env) = host().await;
        let _scope = env.budget.enter_comm_accounting_scope();
        store(&env, &space_counter(), vec![(counter(5, 0), false)]).await;
        space.take_event_log().await;
        let (_, _, mut txn) = run_bump(&env).await.expect("the bump stages");
        txn.commit().await.expect("the bump commits");
        let log = space.take_event_log().await;
        let [take, _, space_put, collection_put] = log.as_slice() else {
            panic!("the bump takes the space counter and puts both counters: {log:?}");
        };
        let payer = Introduction::new(&env.budget, &region(2)).payer();
        for event in [take, space_put, collection_put] {
            let source = match event {
                Event::IoEvent(IOEvent::Consume(consume)) => {
                    RSpaceOperationSource::Consume(consume)
                }
                Event::IoEvent(IOEvent::Produce(produce)) => {
                    RSpaceOperationSource::Produce(produce)
                }
                Event::Comm(_) => panic!("a take or a put is not a COMM: {event:?}"),
            };
            assert_eq!(
                record::register_introduction(&payer)(source),
                Ok(()),
                "{event:?}"
            );
        }
        let seal = |authority: &CostAuthority| {
            system_residue_authority_with_caller(authority, &env.budget.deploy_id(), None)
                .expect("the machinery seal builds")
        };
        let continuation = counter_continuation(
            5,
            &RandomPurpose::SpaceCounter.derive(&random()).split_byte(0),
            Some(seal(&region(1))),
        );
        let expected = Consume::create(
            &vec![space_counter()],
            &vec![counter_pattern()],
            &continuation,
            false,
        );
        assert_eq!(*take, Event::IoEvent(IOEvent::Consume(expected)));
        for channel in [space_counter(), collection_counter()] {
            let records = space.get_data(&channel).await;
            assert_eq!(records[0].a.cost_authority, Some(seal(&region(2))));
        }
    }

    #[tokio::test]
    async fn space_counter_bump_consumes_and_reproduces_the_collection_counter() {
        let (space, env) = host().await;
        store(&env, &space_counter(), vec![(counter(9, 0), false)]).await;
        store(&env, &collection_counter(), vec![(counter(3, 1), false)]).await;
        let stored = space.get_data(&collection_counter()).await[0].clone();
        space.take_event_log().await;
        // Changed by plan v4 errata (V4-19): the sink of the bump lists its staged operations.
        // let (bump, mut txn) = run_bump(&env).await.expect("the bump stages");
        // assert_eq!(bump, Bump {
        //     ord: 3,
        //     staged: &[CounterOp::Take, CounterOp::Put],
        // });
        let (bump, staged, mut txn) = run_bump(&env).await.expect("the bump stages");
        assert_eq!(bump, Bump { ord: 3 });
        assert_eq!(staged, vec![CounterOp::Take, CounterOp::Put]);
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
                // Changed by decision 124 (plan v2 C7.2, decision 99): the machinery seal. The
                // text before:
                // Some(region(1)),
                Some(
                    system_residue_authority_with_caller(&region(1), &env.budget.deploy_id(), None)
                        .expect("the machinery seal builds"),
                ),
            ),
            false,
        );
        assert_eq!(log[0], Event::IoEvent(IOEvent::Consume(take)));
    }

    /// Added by plan v4 errata (V4-19, rule CR-6): UT-5. The sink of a bump gets each staged
    /// operation right after its stage call succeeds, and never before that call. A scripted
    /// replay ends the record after `stages` stage calls. The commit then applies exactly the
    /// operations that the sink got, in staging order: a take fires and a put stores. `ord` is
    /// the order of a bump that completes, and `None` is a record end.
    #[tokio::test]
    async fn space_counter_bump_reports_each_staged_operation() {
        use CounterOp::{Put, Take};
        for (collection, stages, ord, reported, space_values, collection_values) in [
            // The first operation on a collection stages take, put, put.
            (None, 0, None, vec![], vec![5], vec![]),
            (None, 1, None, vec![Take], vec![], vec![]),
            (None, 2, None, vec![Take, Put], vec![6], vec![]),
            (None, 3, Some(0), vec![Take, Put, Put], vec![6], vec![1]),
            // A later operation stages take, put.
            (Some(3), 0, None, vec![], vec![5], vec![3]),
            (Some(3), 1, None, vec![Take], vec![5], vec![]),
            (Some(3), 2, Some(3), vec![Take, Put], vec![5], vec![4]),
        ] {
            let (space, env) = host().await;
            store(&env, &space_counter(), vec![(counter(5, 0), false)]).await;
            let records = match collection {
                Some(value) => vec![(counter(value, 1), false)],
                None => Vec::new(),
            };
            store(&env, &collection_counter(), records).await;
            let inner = env
                .store
                .begin(&[space_counter(), collection_counter()])
                .await
                .expect("a transaction");
            let mut txn: ParSpaceTxn = Box::new(ScriptedReplay {
                inner,
                comms: None,
                stages: Some(stages),
                follows: false,
                // Added by decision 102 (plan v4 C5.3): no per-source rows.
                rows: Vec::new(),
            });
            let matcher = SpaceMatcher::new(&env);
            let continuation = region(1);
            let introduction = Introduction::new(&env.budget, &region(2));
            let random = random();
            let mut staged = Vec::<CounterOp>::with_capacity(3);
            let bumped = bump(
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
                &mut |kind| staged.push(kind),
            );
            let case = format!("{collection:?} {stages}");
            let expected = match ord {
                Some(ord) => Ok(Bump { ord }),
                None => Err(SpaceError::Substrate(SpaceTxnError::RecordEnded)),
            };
            assert_eq!(bumped, expected, "{case}");
            assert_eq!(staged, reported, "{case}");
            let receipt = txn.commit().await.expect("the staged operations commit");
            let committed: Vec<CounterOp> = receipt
                .ops
                .iter()
                .map(|op| match op {
                    SpaceOpReceipt::Fired { .. } => Take,
                    SpaceOpReceipt::Stored { .. } => Put,
                    other => panic!("{case}: a bump only fires and stores: {other:?}"),
                })
                .collect();
            assert_eq!(committed, staged, "{case}");
            assert_eq!(
                values(&space, &space_counter()).await,
                space_values,
                "{case}"
            );
            assert_eq!(
                values(&space, &collection_counter()).await,
                collection_values,
                "{case}"
            );
        }
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
            // Changed by plan v4 errata (V4-19): `run_bump` also names the collected operations.
            // let error = run_bump(&env).await.map(|(bump, _)| bump);
            let error = run_bump(&env).await.map(|(bump, _, _)| bump);
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
}
