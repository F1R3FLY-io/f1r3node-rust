//! The legacy storage charges of space operations (decision 45, Option P).
//!
//! In a legacy deploy, `ChargingRSpace` charges each host operation of an ordinary program
//! (`storage/charging_rspace.rs`). Before the operation it charges the storage of the record or
//! the continuation. After the operation it refunds the storage that a fire removes and charges
//! the event fees (`handle_result`). A space operation stages host-equivalent operations in a
//! space transaction, and the commit reaches the host store without `ChargingRSpace`. So the
//! agent charges each staged operation what `ChargingRSpace` charges for the equivalent host call.
//!
//! The commit runs the `before_observe` callback of each staged operation before the observer
//! calls of that operation. [`LegacyChain`] puts the charges of operation `i` into two places:
//!
//! * the callback of operation `i` first charges the post-charges of operation `i - 1`, then
//!   registers the introduction authority of operation `i`, then charges its pre-charge;
//! * [`LegacyChain::finish`] charges the post-charges of the last operation after the commit.
//!
//! So the legacy log of the deploy is the log of the equivalent host program: the pre-charge
//! and the post-charges of each operation, in order. A charge that runs out of phlo stops the
//! commit where the host program stops. A pre-charge stops before the observer calls of its
//! operation, and a post-charge stops after its operation. Replay stages the same operations
//! and runs the same callbacks, so it makes the same charges and stops at the same point.
//!
//! The chain is active only in a legacy deploy with a metered budget. A legacy deploy has no
//! host-work budget, so the encoded-length walks of the charges reserve nothing, as the walks of
//! `ChargingRSpace` reserve nothing.

use models::rhoapi::tagged_continuation::TaggedCont;
use models::rhoapi::{BindPattern, ListParWithRandom, Par, TaggedContinuation};
use rspace_plus_plus::rspace::errors::RSpaceError;
use rspace_plus_plus::rspace::internal::ConsumeCandidate;
use rspace_plus_plus::rspace::rspace_interface::RSpaceOperationSource;
use rspace_plus_plus::rspace::space_store_handle::{
    BeforeObserve, ConsumeOutcome, ProduceOutcome, SpaceConsume, SpaceProduce,
};

use crate::rust::interpreter::accounting::costs::{
    comm_event_storage_cost, event_storage_cost, storage_cost_consume, storage_cost_produce, Cost,
};
use crate::rust::interpreter::accounting::RuntimeBudget;
use crate::rust::interpreter::spaces::agent::ParSpaceTxn;
use crate::rust::interpreter::spaces::errors::SpaceError;

/// A staged produce over `Par`.
pub type ParSpaceProduce = SpaceProduce<Par, BindPattern, ListParWithRandom, TaggedContinuation>;

/// A staged consume over `Par`.
pub type ParSpaceConsume = SpaceConsume<Par, BindPattern, ListParWithRandom, TaggedContinuation>;

/// The legacy charges of one staged operation: the pre-charge that `ChargingRSpace` makes
/// before the host call, and the post-charges of its `handle_result` after the call.
#[derive(Clone, Debug, PartialEq)]
pub struct LegacyCharges {
    pub pre: Cost,
    pub post: Vec<Cost>,
}

/// The charges of a staged produce: the storage of the record before the call. After a store,
/// the event fee of one channel. After a fire, the charges of [`fire_post`].
pub fn produce_charges(op: &ParSpaceProduce) -> Result<LegacyCharges, SpaceError> {
    let pre = storage_cost_produce(&op.channel, &op.data);
    let post = match &op.outcome {
        ProduceOutcome::Store => vec![event_storage_cost(1)],
        ProduceOutcome::Fire {
            channels,
            continuation,
            data,
        } => fire_post(
            Fired {
                channels,
                patterns: &continuation.patterns,
                continuation: &continuation.continuation,
                persist: continuation.persist,
                candidates: data,
            },
            Trigger {
                id: &op.data.random_state,
                persist: op.persist,
                channels: 1,
            },
        )?,
    };
    Ok(LegacyCharges { pre, post })
}

/// The charges of a staged consume: the storage of the continuation before the call. After a
/// park, the event fee of its channels. After a fire, the charges of [`fire_post`], with the
/// consume as the trigger and as the fired continuation.
pub fn consume_charges(op: &ParSpaceConsume) -> Result<LegacyCharges, SpaceError> {
    let pre = storage_cost_consume(&op.channels, &op.patterns, &op.continuation);
    let channels = channel_count(op.channels.len())?;
    let post = match &op.outcome {
        ConsumeOutcome::Park => vec![event_storage_cost(channels)],
        ConsumeOutcome::Fire { data } => {
            let id = consume_id_bytes(&op.continuation)?;
            fire_post(
                Fired {
                    channels: &op.channels,
                    patterns: &op.patterns,
                    continuation: &op.continuation,
                    persist: op.persist,
                    candidates: data,
                },
                Trigger {
                    id: &id,
                    persist: op.persist,
                    channels,
                },
            )?
        }
    };
    Ok(LegacyCharges { pre, post })
}

/// The continuation that a fire binds, and the records that it binds, in channel order.
struct Fired<'a> {
    channels: &'a [Par],
    patterns: &'a [BindPattern],
    continuation: &'a TaggedContinuation,
    persist: bool,
    candidates: &'a [ConsumeCandidate<Par, ListParWithRandom>],
}

/// The operation that triggers a fire: its id bytes, its persist flag, and its channel count.
struct Trigger<'a> {
    id: &'a [u8],
    persist: bool,
    channels: i64,
}

/// `handle_result` of `ChargingRSpace` for a fire, in its order:
///
/// 1. The refund of the storage of the fired continuation, when it is not persistent or the
///    trigger has its id.
/// 2. The refund of the storage of each bound record that is not persistent or that the
///    trigger sent, on the channel that the continuation binds it on.
/// 3. The event fee of the trigger, when the trigger is not persistent.
/// 4. The COMM event fee of the continuation channels.
///
/// The persist flag of a candidate is the flag of the host's `wrap_result`, because the
/// receipt keeps that flag (decision 45, Rule E). The refund sum is checked, where
/// `ChargingRSpace` adds without a check.
fn fire_post(fired: Fired<'_>, trigger: Trigger<'_>) -> Result<Vec<Cost>, SpaceError> {
    let refund_for_consume = match !fired.persist || consume_id_is(fired.continuation, trigger.id)?
    {
        true => storage_cost_consume(fired.channels, fired.patterns, fired.continuation).value,
        false => 0,
    };
    let refund_for_produces = fired
        .candidates
        .iter()
        .zip(fired.channels)
        .filter(|(candidate, _)| {
            !candidate.datum.persist || candidate.removed_datum.random_state == trigger.id
        })
        .try_fold(0_i64, |total, (candidate, channel)| {
            total.checked_add(storage_cost_produce(channel, &candidate.removed_datum).value)
        })
        .ok_or(SpaceError::Inconsistent(
            "a legacy storage refund overflows i64",
        ))?;
    let mut post = Vec::with_capacity(4);
    post.push(Cost::create(-refund_for_consume, "consume storage refund"));
    post.push(Cost::create(
        -refund_for_produces,
        "produces storage refund",
    ));
    if !trigger.persist {
        post.push(event_storage_cost(trigger.channels));
    }
    post.push(comm_event_storage_cost(channel_count(
        fired.channels.len(),
    )?));
    Ok(post)
}

/// The id bytes of a continuation, as `ChargingRSpace` reads them: the random state of a body,
/// or the big-endian bytes of a body reference. A continuation without a body is
/// `Inconsistent`, where `ChargingRSpace` panics.
fn consume_id_bytes(continuation: &TaggedContinuation) -> Result<Vec<u8>, SpaceError> {
    match &continuation.tagged_cont {
        Some(TaggedCont::ParBody(body)) => Ok(body.random_state.clone()),
        Some(TaggedCont::ScalaBodyRef(reference)) => Ok(reference.to_be_bytes().to_vec()),
        None => Err(SpaceError::Inconsistent(
            "a staged continuation has no body",
        )),
    }
}

/// Whether the id bytes of `continuation` equal `id`, with no copy of the bytes.
fn consume_id_is(continuation: &TaggedContinuation, id: &[u8]) -> Result<bool, SpaceError> {
    match &continuation.tagged_cont {
        Some(TaggedCont::ParBody(body)) => Ok(body.random_state == id),
        Some(TaggedCont::ScalaBodyRef(reference)) => Ok(reference.to_be_bytes() == id),
        None => Err(SpaceError::Inconsistent(
            "a staged continuation has no body",
        )),
    }
}

fn channel_count(channels: usize) -> Result<i64, SpaceError> {
    i64::try_from(channels).map_err(|_| SpaceError::Inconsistent("a channel count exceeds i64"))
}

/// The legacy charges of the staged operations of one space transaction, in staging order.
///
/// Every staged operation of a space job goes through [`LegacyChain::produce`] or
/// [`LegacyChain::consume`]. Each call wraps the `before_observe` callback of the operation, so
/// the callback charges the carried post-charges of the operation before it, then registers the
/// introduction, then charges the pre-charge. [`LegacyChain::finish`] charges the post-charges
/// of the last operation after the commit.
pub struct LegacyChain {
    /// The budget of the deploy when the chain is active: a legacy deploy with a metered budget.
    budget: Option<RuntimeBudget>,
    /// The post-charges of the operation that the chain staged last.
    carry: Vec<Cost>,
}

impl LegacyChain {
    /// A chain over `budget`. It is active only in a legacy deploy with a metered budget.
    pub fn new(budget: &RuntimeBudget) -> LegacyChain {
        LegacyChain {
            budget: (budget.is_legacy() && !budget.is_unmetered()).then(|| budget.clone()),
            carry: Vec::new(),
        }
    }

    /// Whether the chain charges anything.
    pub fn is_active(&self) -> bool { self.budget.is_some() }

    /// Stages `op` in `txn`, with the legacy charges in its callback when the chain is active.
    pub fn produce(
        &mut self,
        txn: &mut ParSpaceTxn,
        mut op: ParSpaceProduce,
    ) -> Result<(), SpaceError> {
        if let Some(budget) = &self.budget {
            let charges = produce_charges(&op)?;
            op.before_observe =
                Some(self.attach(budget.clone(), charges, op.before_observe.take()));
        }
        txn.produce(op)?;
        Ok(())
    }

    /// Stages `op` in `txn`, with the legacy charges in its callback when the chain is active.
    pub fn consume(
        &mut self,
        txn: &mut ParSpaceTxn,
        mut op: ParSpaceConsume,
    ) -> Result<(), SpaceError> {
        if let Some(budget) = &self.budget {
            let charges = consume_charges(&op)?;
            op.before_observe =
                Some(self.attach(budget.clone(), charges, op.before_observe.take()));
        }
        txn.consume(op)?;
        Ok(())
    }

    /// The callback of an operation with `charges`: the carried post-charges, then
    /// `registration`, then the pre-charge. The post-charges of the operation become the carry.
    fn attach(
        &mut self,
        budget: RuntimeBudget,
        charges: LegacyCharges,
        registration: Option<BeforeObserve>,
    ) -> BeforeObserve {
        let LegacyCharges { pre, post } = charges;
        let carried = std::mem::replace(&mut self.carry, post);
        Box::new(move |source: RSpaceOperationSource<'_>| {
            for charge in carried {
                budget.charge_legacy(charge).map_err(RSpaceError::from)?;
            }
            if let Some(registration) = registration {
                registration(source)?;
            }
            budget.charge_legacy(pre).map_err(RSpaceError::from)
        })
    }

    /// Charges the post-charges of the last staged operation. The job calls it once, after the
    /// commit and before its outcome.
    pub fn finish(self) -> Result<(), SpaceError> {
        if let Some(budget) = self.budget {
            for charge in self.carry {
                budget
                    .charge_legacy(charge)
                    .map_err(|error| SpaceError::Host(RSpaceError::from(error)))?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::sync::Arc;

    use models::rhoapi::ParWithRandom;
    use models::rust::utils::{new_freevar_par, new_gstring_par};
    use rspace_plus_plus::rspace::internal::{Datum, WaitingContinuation};
    use rspace_plus_plus::rspace::rspace::RSpace;
    use rspace_plus_plus::rspace::rspace_interface::ISpace;
    use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
    use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
    use rspace_plus_plus::rspace::space_store_handle::SpaceStoreHandle;
    use rspace_plus_plus::rspace::trace::event::{Consume, Produce};

    use super::*;
    use crate::rust::interpreter::matcher::r#match::Matcher;

    type ParRSpace = RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>;

    fn channel(name: &str) -> Par { new_gstring_par(name.to_string(), Vec::new(), false) }

    fn record(value: &str, random: &[u8]) -> ListParWithRandom {
        ListParWithRandom {
            pars: vec![new_gstring_par(value.to_string(), Vec::new(), false)],
            random_state: random.to_vec(),
            cost_authority: None,
            cost_stack: None,
        }
    }

    fn continuation(random: &[u8]) -> TaggedContinuation {
        TaggedContinuation {
            tagged_cont: Some(TaggedCont::ParBody(ParWithRandom {
                body: Some(new_gstring_par("body".to_string(), Vec::new(), false)),
                random_state: random.to_vec(),
            })),
            guard: None,
            cost_authority: None,
        }
    }

    fn pattern() -> BindPattern {
        BindPattern {
            patterns: vec![new_freevar_par(0, Vec::new())],
            remainder: None,
            free_count: 1,
        }
    }

    fn candidate(
        on: &Par,
        removed: &ListParWithRandom,
        persist: bool,
        index: i32,
    ) -> ConsumeCandidate<Par, ListParWithRandom> {
        ConsumeCandidate {
            channel: on.clone(),
            datum: Datum {
                a: removed.clone(),
                persist,
                source: Produce::create(on, removed, persist),
            },
            removed_datum: removed.clone(),
            datum_index: index,
        }
    }

    fn waiter(
        channels: &[Par],
        persist: bool,
        random: &[u8],
    ) -> WaitingContinuation<BindPattern, TaggedContinuation> {
        let patterns = vec![pattern(); channels.len()];
        let continuation = continuation(random);
        WaitingContinuation {
            source: Consume::create(&channels.to_vec(), &patterns, &continuation, persist),
            patterns,
            continuation,
            persist,
            peeks: BTreeSet::new(),
        }
    }

    fn produce(
        on: &Par,
        data: ListParWithRandom,
        persist: bool,
        outcome: ProduceOutcome<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
    ) -> ParSpaceProduce {
        SpaceProduce {
            channel: on.clone(),
            data,
            persist,
            outcome,
            before_observe: None,
        }
    }

    fn consume(
        channels: &[Par],
        random: &[u8],
        persist: bool,
        outcome: ConsumeOutcome<Par, ListParWithRandom>,
    ) -> ParSpaceConsume {
        SpaceConsume {
            channels: channels.to_vec(),
            patterns: vec![pattern(); channels.len()],
            continuation: continuation(random),
            persist,
            peeks: BTreeSet::new(),
            outcome,
            before_observe: None,
        }
    }

    fn refund(value: i64, label: &'static str) -> Cost { Cost::create(-value, label) }

    // A store pays the storage of its record and the event fee of one channel. A park pays
    // the storage of its continuation and the event fee of its channels.
    #[test]
    fn legacy_charges_of_a_store_and_a_park() {
        let (a, b) = (channel("a"), channel("b"));
        let data = record("x", b"rx");
        let store = produce(&a, data.clone(), false, ProduceOutcome::Store);
        assert_eq!(
            produce_charges(&store),
            Ok(LegacyCharges {
                pre: storage_cost_produce(&a, &data),
                post: vec![event_storage_cost(1)],
            })
        );
        let park = consume(&[a.clone(), b.clone()], b"k", false, ConsumeOutcome::Park);
        assert_eq!(
            consume_charges(&park),
            Ok(LegacyCharges {
                pre: storage_cost_consume(&park.channels, &park.patterns, &park.continuation),
                post: vec![event_storage_cost(2)],
            })
        );
    }

    // A produce-side fire follows `handle_result`: the refund of a waiter that is not
    // persistent, the refunds of the incoming datum and of the stored records that are not
    // persistent or that the trigger sent, the event fee of a trigger that is not persistent,
    // and the COMM fee of the waiter channels. Each refund is one entry, also when it is zero.
    #[test]
    fn legacy_charges_of_a_produce_fire_follow_handle_result() {
        let (a, b, c) = (channel("a"), channel("b"), channel("c"));
        let data = record("x", b"rx");
        let stored_plain = record("y", b"ry");
        let stored_persistent = record("z", b"rz");
        let stored_by_trigger = record("w", b"rx");
        let channels = vec![a.clone(), b.clone(), c.clone(), b.clone()];
        let candidates = vec![
            candidate(&a, &data, false, -1),
            candidate(&b, &stored_plain, false, 0),
            candidate(&c, &stored_persistent, true, 0),
            candidate(&b, &stored_by_trigger, true, 1),
        ];
        let refunded = [
            storage_cost_produce(&a, &data).value,
            storage_cost_produce(&b, &stored_plain).value,
            storage_cost_produce(&b, &stored_by_trigger).value,
        ]
        .iter()
        .sum::<i64>();
        for (waiter_persist, waiter_random, consume_refunded) in [
            (false, &b"k"[..], true),
            (true, &b"k"[..], false),
            (true, &b"rx"[..], true),
        ] {
            for trigger_persist in [false, true] {
                let fired = waiter(&channels, waiter_persist, waiter_random);
                let consume_refund = match consume_refunded {
                    true => {
                        storage_cost_consume(&channels, &fired.patterns, &fired.continuation).value
                    }
                    false => 0,
                };
                let op = produce(&a, data.clone(), trigger_persist, ProduceOutcome::Fire {
                    channels: channels.clone(),
                    continuation: fired,
                    data: candidates.clone(),
                });
                let mut post = vec![
                    refund(consume_refund, "consume storage refund"),
                    refund(refunded, "produces storage refund"),
                ];
                if !trigger_persist {
                    post.push(event_storage_cost(1));
                }
                post.push(comm_event_storage_cost(4));
                assert_eq!(
                    produce_charges(&op),
                    Ok(LegacyCharges {
                        pre: storage_cost_produce(&a, &data),
                        post,
                    }),
                    "waiter persist {waiter_persist}, waiter id {waiter_random:?}, trigger persist {trigger_persist}"
                );
            }
        }
    }

    // A consume-side fire always refunds the storage of the consume. It refunds the records
    // that are not persistent or that carry the id of the consume, and it pays the event fee of
    // its channels when it is not persistent, and the COMM fee.
    #[test]
    fn legacy_charges_of_a_consume_fire_refund_the_consume() {
        let (a, b) = (channel("a"), channel("b"));
        let plain = record("x", b"rx");
        let persistent = record("y", b"ry");
        let by_consume = record("z", b"k");
        for persist in [false, true] {
            let op = consume(
                &[a.clone(), b.clone(), a.clone()],
                b"k",
                persist,
                ConsumeOutcome::Fire {
                    data: vec![
                        candidate(&a, &plain, false, 0),
                        candidate(&b, &persistent, true, 0),
                        candidate(&a, &by_consume, true, 1),
                    ],
                },
            );
            let stored = storage_cost_consume(&op.channels, &op.patterns, &op.continuation);
            let mut post = vec![
                refund(stored.value, "consume storage refund"),
                refund(
                    storage_cost_produce(&a, &plain).value
                        + storage_cost_produce(&a, &by_consume).value,
                    "produces storage refund",
                ),
            ];
            if !persist {
                post.push(event_storage_cost(3));
            }
            post.push(comm_event_storage_cost(3));
            assert_eq!(
                consume_charges(&op),
                Ok(LegacyCharges { pre: stored, post }),
                "consume persist {persist}"
            );
        }
    }

    // A continuation without a body cannot name its consume, so its charges fail closed where
    // `ChargingRSpace` panics.
    #[test]
    fn legacy_charges_of_a_continuation_without_a_body_fail_closed() {
        let a = channel("a");
        let mut op = consume(
            std::slice::from_ref(&a),
            b"k",
            false,
            ConsumeOutcome::Fire {
                data: vec![candidate(&a, &record("x", b"rx"), false, 0)],
            },
        );
        op.continuation.tagged_cont = None;
        assert_eq!(
            consume_charges(&op),
            Err(SpaceError::Inconsistent(
                "a staged continuation has no body"
            ))
        );
    }

    // The chain charges only a legacy deploy with a metered budget.
    #[test]
    fn legacy_chain_is_active_only_in_a_metered_legacy_deploy() {
        let budget = RuntimeBudget::new(Cost::create(1_000, "legacy chain test"));
        assert!(!LegacyChain::new(&budget).is_active());
        let _legacy = budget.enter_legacy_scope();
        assert!(LegacyChain::new(&budget).is_active());
        let unmetered = RuntimeBudget::unmetered();
        let _unmetered_legacy = unmetered.enter_legacy_scope();
        assert!(!LegacyChain::new(&unmetered).is_active());
    }

    async fn handle() -> (
        ParRSpace,
        SpaceStoreHandle<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
    ) {
        let mut stores = InMemoryStoreManager::new();
        let space = ParRSpace::create(
            stores.r_space_stores().await.expect("in-memory stores"),
            Arc::new(Box::new(Matcher)),
        )
        .expect("an in-memory space");
        (space.clone(), SpaceStoreHandle::new(space))
    }

    /// A store, a park and a consume-side fire of one transaction, with their charges.
    fn transaction(
        a: &Par,
        b: &Par,
    ) -> (
        Vec<ParSpaceProduce>,
        ParSpaceConsume,
        ParSpaceConsume,
        Vec<LegacyCharges>,
    ) {
        let data = record("x", b"rx");
        let store = produce(a, data.clone(), false, ProduceOutcome::Store);
        let park = consume(std::slice::from_ref(b), b"kb", false, ConsumeOutcome::Park);
        let fire = consume(
            std::slice::from_ref(a),
            b"ka",
            false,
            ConsumeOutcome::Fire {
                data: vec![candidate(a, &data, false, 0)],
            },
        );
        let charges = vec![
            produce_charges(&store).expect("store charges"),
            consume_charges(&park).expect("park charges"),
            consume_charges(&fire).expect("fire charges"),
        ];
        (vec![store], park, fire, charges)
    }

    fn host_order(charges: &[LegacyCharges]) -> Vec<Cost> {
        charges
            .iter()
            .flat_map(|charge| {
                std::iter::once(charge.pre.clone()).chain(charge.post.iter().cloned())
            })
            .collect()
    }

    // The callbacks charge each pre-charge before its operation and carry its post-charges to
    // the next callback. `finish` charges the last post-charges after the commit. So the legacy
    // log is the log of the host program: each pre-charge, then its post-charges.
    #[tokio::test]
    async fn legacy_chain_charges_in_host_order() {
        let (a, b) = (channel("a"), channel("b"));
        let (_space, store) = handle().await;
        let budget = RuntimeBudget::new(Cost::create(1_000_000, "legacy chain test"));
        let _legacy = budget.enter_legacy_scope();
        let (produces, park, fire, charges) = transaction(&a, &b);
        let mut chain = LegacyChain::new(&budget);
        let mut txn = store
            .begin(&[a.clone(), b.clone()])
            .await
            .expect("a transaction");
        for op in produces {
            chain.produce(&mut txn, op).expect("the store stages");
        }
        chain.consume(&mut txn, park).expect("the park stages");
        chain.consume(&mut txn, fire).expect("the fire stages");
        txn.commit().await.expect("the transaction commits");
        let expected = host_order(&charges);
        let tail = charges.last().expect("charges").post.len();
        assert_eq!(budget.get_log(), expected[..expected.len() - tail].to_vec());
        chain.finish().expect("the tail charges");
        assert_eq!(budget.get_log(), expected);
        let total: i64 = expected.iter().map(|cost| cost.value).sum();
        assert_eq!(budget.total_cost().value, total);
    }

    // A pre-charge that runs out of phlo stops the commit before its operation: the operations
    // before it stay applied, and the operation logs no event.
    #[tokio::test]
    async fn legacy_chain_pre_charge_stops_before_its_operation() {
        let (a, b) = (channel("a"), channel("b"));
        let (space, store) = handle().await;
        let (_, _, _, charges) = transaction(&a, &b);
        let first =
            charges[0].pre.value + charges[0].post.iter().map(|cost| cost.value).sum::<i64>();
        let budget = RuntimeBudget::new(Cost::create(
            first + charges[1].pre.value - 1,
            "legacy chain test",
        ));
        let _legacy = budget.enter_legacy_scope();
        let (produces, park, fire, _) = transaction(&a, &b);
        let mut chain = LegacyChain::new(&budget);
        let mut txn = store
            .begin(&[a.clone(), b.clone()])
            .await
            .expect("a transaction");
        for op in produces {
            chain.produce(&mut txn, op).expect("the store stages");
        }
        chain.consume(&mut txn, park).expect("the park stages");
        chain.consume(&mut txn, fire).expect("the fire stages");
        assert!(matches!(
            txn.commit().await,
            Err(
                rspace_plus_plus::rspace::space_store_handle::SpaceTxnError::Host(
                    RSpaceError::OutOfPhlogistons
                )
            )
        ));
        assert_eq!(
            space.get_data(&a).await.len(),
            1,
            "the store before the stop stays applied"
        );
        assert!(
            space
                .get_waiting_continuations(vec![b.clone()])
                .await
                .is_empty(),
            "the park never runs"
        );
    }

    // A tail that runs out of phlo fails after the full commit, as the post-charges of the last
    // host operation fail after that operation.
    #[tokio::test]
    async fn legacy_chain_tail_fails_after_the_commit() {
        let (a, b) = (channel("a"), channel("b"));
        let (space, store) = handle().await;
        let (_, _, _, charges) = transaction(&a, &b);
        let expected = host_order(&charges);
        let tail = charges.last().expect("charges").post.len();
        let before_tail: i64 = expected[..expected.len() - tail]
            .iter()
            .map(|cost| cost.value)
            .sum();
        let tail_sum: i64 = expected[expected.len() - tail..]
            .iter()
            .map(|cost| cost.value)
            .sum();
        assert!(
            tail_sum > 0,
            "the tail of a fire that is not persistent costs phlo"
        );
        let budget = RuntimeBudget::new(Cost::create(
            before_tail + tail_sum - 1,
            "legacy chain test",
        ));
        let _legacy = budget.enter_legacy_scope();
        let (produces, park, fire, _) = transaction(&a, &b);
        let mut chain = LegacyChain::new(&budget);
        let mut txn = store
            .begin(&[a.clone(), b.clone()])
            .await
            .expect("a transaction");
        for op in produces {
            chain.produce(&mut txn, op).expect("the store stages");
        }
        chain.consume(&mut txn, park).expect("the park stages");
        chain.consume(&mut txn, fire).expect("the fire stages");
        txn.commit().await.expect("the transaction commits");
        assert!(
            space.get_data(&a).await.is_empty(),
            "the fire removed the stored record"
        );
        assert_eq!(
            space.get_waiting_continuations(vec![b.clone()]).await.len(),
            1
        );
        assert_eq!(
            chain.finish(),
            Err(SpaceError::Host(RSpaceError::OutOfPhlogistons))
        );
    }
}
