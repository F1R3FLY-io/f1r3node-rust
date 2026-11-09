//! The stack discipline of the native spaces (decision 11, P5b, P5 design v3 A.8).
//!
//! A stack is head-only on both sides, as a queue is. The head of a side is its top: the entry
//! with the greatest `ord`. Only the top record and the top waiter interact, and a top that does
//! not match blocks the entries below it. Every operation bumps the collection counter
//! (decision 31), so a fresh element has the greatest `ord` of its side and always becomes its
//! top. Records and waiters therefore leave in LIFO order.
//!
//! A fresh element on top of its side can fire the top of the other side and so expose an older
//! pair below. The stack then settles: it fires the top pair while the two tops match
//! ([`settle`](crate::rust::interpreter::spaces::settle), F2). It settles before the plan of each
//! operation, after it, and in a settle job:
//!
//! ```text
//! STACK-OFFER(op, ord) = offer_head:                ▷ the head driver of generic.rs
//!   settle before the plan                          ▷ fires a pair that a settle job still owes
//!   if not at_head(Data, op.reissued_ord): store, settle after, stop   ▷ decision 43: LIFO place
//!   loop: W ← the top waiter, or store, settle after, stop
//!         if W does not match the datum (pattern and guard): store, settle after, stop
//!         fire; Done → settle after; Continue → next top waiter; Reissue → the operation
//! STACK-RECEIVE(op, ord) = receive_head: symmetric. After a peek fire of a non-persistent top
//!   record, the plain re-store puts the record back at its order (decision 45, Rule E).
//! ```
//!
//! Three rules follow from the head driver of `generic.rs` with this discipline:
//!
//! * A fresh element always reaches the top (`reaches_head` is true). A re-issued persistent
//!   element keeps its first-round `ord` (decision 43) and re-enters at its LIFO place: at the
//!   top when its side is empty or its `ord` is greater than the top (`precedes` is `>`), and
//!   otherwise stored or parked below the younger top, with no fire.
//! * A consume-side peek removes the top record and stores it again at the same `ord` before any
//!   settle step. The top pair is then the pair before the peek, so a peek never exposes a
//!   buried entry and never starts the settle.
//! * A peek receive can fire while waiters are queued, because a fresh receive reaches the top.
//!   The plain re-store is still exact: the restored record is the top record again, and the top
//!   waiter did not match it before the peek (INV).
//!
//! The order comes from the `ord` of the envelopes, never from the list order of the store: the
//! hot store inserts at the front, and a restart can load a list in another order. Two entries
//! with one `ord` order by their bincode bytes, the greater bytes on top, so the head is a
//! function of the entry set. On an equal order the scan encodes each entry at most once and
//! compares the bytes. Equal orders do not occur in a well-formed collection
//! (`StackDiscipline.v`), but the scan still gives one head for them.
//!
//! The legacy stack (`spaces/collections/basic.rs` and `continuations.rs` of the legacy branch)
//! popped the top of a vector and scanned every waiter, with no settle. Both are replaced.

use std::cmp::Ordering;

use models::rhoapi::Par;
// Added by decision 124 (plan v4 C7.11, ruling RADD 1.4): the tail read of a head row.
use rspace_plus_plus::rspace::space_store_handle::TailRead;
use serde::Serialize;

use crate::rust::interpreter::spaces::agent::{SettleOp, SpaceReissue};
use crate::rust::interpreter::spaces::errors::SpaceError;
// Changed by decision 124 (plan v4 C7.11, ruling RADD 1.4): the view of the stack reads the
// store through `StoreView` and composes its head reads. The text before:
// use crate::rust::interpreter::spaces::generic::{Cx, Discipline, Held, OpView, Ordered, Side};
use crate::rust::interpreter::spaces::generic::{
    datum_ord, head_waiter_of, Cx, Discipline, Held, OpView, Ordered, Side, StoreView, StoredMatch,
};
use crate::rust::interpreter::spaces::record;
use crate::rust::interpreter::spaces::settle::{
    self, Bound, BoundRecord, FirePlan, InFlight, SettleView,
};

/// The stack discipline.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct StackDiscipline;

impl Discipline for StackDiscipline {
    const NAME: &'static str = "stack";
    const USES_COUNTER: bool = true;
    const SEND_PARAMS: &'static [&'static str] = &[];
    /// Added by decision 124 (plan v4 C7.11, ruling RADD2 1.4): the stack settles (P5b).
    const SETTLES: bool = true;

    /// The entry with the greatest `(ord, bincode bytes)`, on both sides.
    fn head<T: Serialize>(
        &self,
        _side: Side,
        entries: &[Ordered<'_, T>],
    ) -> Result<Option<usize>, SpaceError> {
        let mut top: Option<Top<'_, '_, T>> = None;
        for (index, entry) in entries.iter().enumerate() {
            top = Some(match top {
                None => Top {
                    index,
                    entry,
                    bytes: None,
                },
                Some(best) => best.challenge(index, entry)?,
            });
        }
        Ok(top.map(|best| best.index))
    }

    /// A fresh element has the greatest order of its side, so it always reaches the top.
    fn reaches_head(&self, _side: Side, _queued: usize) -> bool { true }

    /// The greater order is on top, on both sides.
    fn precedes(&self, _side: Side, ord: i64, head: i64) -> Result<bool, SpaceError> {
        Ok(ord > head)
    }

    /// Fires the top pairs that the stack still owes before the plan of `op`. A settle that
    /// cannot complete re-issues `op` itself, with the order rule of decision 43.
    fn settle_before(
        &self,
        cx: &mut Cx<'_, '_>,
        op: OpView<'_>,
    ) -> Result<Option<SpaceReissue>, SpaceError> {
        settle::settle_before_plan(&StackView::new(self), cx, op)
    }

    /// Fires the top pairs that the plan of `op` exposed. A settle that cannot complete
    /// re-issues a settle job.
    fn settle_after(
        &self,
        cx: &mut Cx<'_, '_>,
        op: OpView<'_>,
    ) -> Result<Option<SpaceReissue>, SpaceError> {
        settle::settle_after_plan(&StackView::new(self), cx, op)
    }

    /// Fires the top pairs that a settle job continues. A settle that cannot complete re-issues
    /// the same job.
    fn settle_job(
        &self,
        cx: &mut Cx<'_, '_>,
        op: SettleOp,
    ) -> Result<Option<SpaceReissue>, SpaceError> {
        settle::settle_job(&StackView::new(self), cx, op)
    }

    /// Added by decision 124 (plan v4 C7.11, rule CR-31, ruling RADD 1.4): the first step of the
    /// settle of the stack after the unit of a deviating head row, over the store view `view`
    /// with the fire count `fired` (ADD A2.8). A deviating head row stores or parks, so nothing
    /// is in flight after its unit.
    fn tail_read<V: StoreView>(&self, view: &mut V, fired: usize) -> Result<TailRead, SpaceError> {
        settle::tail_read(&StackView::new(self), view, &InFlight::none(), fired)
    }
}

/// The settle order of a stack: the top pair of its collection.
#[derive(Clone, Copy, Debug)]
pub struct StackView<'d> {
    discipline: &'d StackDiscipline,
}

impl<'d> StackView<'d> {
    /// The settle order of the stack `discipline`.
    pub const fn new(discipline: &'d StackDiscipline) -> Self { StackView { discipline } }
}

impl SettleView for StackView<'_> {
    /// Added by decision 124 (plan v4 C7.11, ruling RADD 1.4): the settle offer of a stack
    /// builds its head search with the stack discipline.
    type Discipline = StackDiscipline;

    /// The top waiter and the top record, when the top waiter matches the payload of the top
    /// record and its guard admits the match (decision 37: a head-only collection tests only its
    /// top). A record in flight was the top record when the settle lifted it, and a settle stores
    /// no record while one is in flight, so the record in flight is still the top record. An
    /// empty side plans nothing and reads nothing.
    // Changed by decision 124 (plan v4 C7.11, ruling RADD 1.4): the reads of this fn go through
    // the view `view` in place of `cx`. The comment above a changed line keeps its text before.
    // fn next_fire(
    fn next_fire<V: StoreView>(
        &self,
        // cx: &mut Cx<'_, '_>,
        view: &mut V,
        in_flight: &InFlight,
    ) -> Result<Option<FirePlan>, SpaceError> {
        // if cx.side_len(Side::Waiters)? == 0 {
        if view.waiters()?.is_empty() {
            return Ok(None);
        }
        // if !in_flight.is_some() && cx.side_len(Side::Data)? == 0 {
        if !in_flight.is_some() && view.data()?.is_empty() {
            return Ok(None);
        }
        // let Some(waiter) = cx.head_waiter(self.discipline)? else {
        let Some(waiter) = head_waiter_of(self.discipline, view.waiters()?)? else {
            return Ok(None);
        };
        let [pattern] = waiter.patterns.as_slice() else {
            return Err(SpaceError::Inconsistent(
                "a waiting continuation of a collection has several binds",
            ));
        };
        let (record, payload) = match in_flight.record() {
            Some(lifted) => (
                BoundRecord::InFlight,
                record::unwrap_datum(&lifted.datum.a)?.datum,
            ),
            None => {
                // let Some((index, top)) = cx.head_datum(self.discipline)? else {
                let records = view.data()?;
                let head = Cx::scan_head(self.discipline, Side::Data, records, datum_ord)?;
                let Some((index, top)) = head
                    .and_then(|(index, _)| records.get(index).map(|datum| (index, datum.clone())))
                else {
                    return Ok(None);
                };
                let view_index = i32::try_from(index)
                    .map_err(|_| SpaceError::Inconsistent("a record index exceeds i32"))?;
                let payload = record::unwrap_datum(&top.a)?.datum;
                (
                    BoundRecord::Stored {
                        datum: top,
                        view_index,
                    },
                    payload,
                )
            }
        };
        // Added by decision 124 (plan v4 C7.8, rule CR-20): the top pair does not fire again
        // when the book holds it, so the stack owes no fire.
        let source = match (&record, in_flight.record()) {
            (BoundRecord::Stored { datum, .. }, _) => &datum.source,
            (BoundRecord::InFlight, Some(lifted)) => &lifted.datum.source,
            (BoundRecord::InFlight, None) => {
                return Err(SpaceError::Inconsistent(
                    "a settle plan binds the element in flight, but none is in flight",
                ))
            }
        };
        match view.booked(source, &waiter.source) {
            false => {}
            true => return Ok(None),
        }
        // The theory of the space checks a stored top record, and not the record in flight
        // (P7, TH-6).
        let held = match record {
            BoundRecord::Stored { .. } => Held::Stored,
            BoundRecord::InFlight => Held::InFlight,
        };
        // let Some(matched) = cx.matches(pattern, &payload, &waiter.continuation, held)? else {
        let Some(matched) = view.bind(pattern, &payload) else {
            return Ok(None);
        };
        let admitted = match held {
            Held::Stored => {
                view.admits_all(&waiter.continuation, std::slice::from_ref(&matched), &[
                    StoredMatch {
                        payload: &payload,
                        captures: &matched,
                    },
                ])?
            }
            Held::InFlight => {
                view.admits_all(&waiter.continuation, std::slice::from_ref(&matched), &[])?
            }
        };
        match admitted {
            true => {}
            false => return Ok(None),
        }
        let mut group = Vec::<Par>::with_capacity(1);
        // group.push(cx.data_channel().clone());
        group.push(view.data_channel()?.clone());
        let mut bound = Vec::<Bound>::with_capacity(1);
        bound.push(Bound { record, matched });
        Ok(Some(FirePlan {
            group,
            waiter,
            bound,
        }))
    }
}

// The top entry of a scan so far, with its bytes once a tie-break has encoded them.
struct Top<'e, 'a, T> {
    index: usize,
    entry: &'e Ordered<'a, T>,
    bytes: Option<Vec<u8>>,
}

impl<'e, 'a, T: Serialize> Top<'e, 'a, T> {
    // The higher of `self` and `entry`: a greater `ord`, or an equal `ord` and greater bytes. A
    // tie encodes `entry` and, once per scan, the top entry, and compares the bytes. On equal
    // bytes the earlier entry stays on top.
    fn challenge(self, index: usize, entry: &'e Ordered<'a, T>) -> Result<Self, SpaceError> {
        match entry.ord.cmp(&self.entry.ord) {
            Ordering::Greater => Ok(Top {
                index,
                entry,
                bytes: None,
            }),
            Ordering::Less => Ok(self),
            Ordering::Equal => {
                let top_bytes = match self.bytes {
                    Some(bytes) => bytes,
                    None => bytes(self.entry.entry)?,
                };
                let entry_bytes = bytes(entry.entry)?;
                match entry_bytes > top_bytes {
                    true => Ok(Top {
                        index,
                        entry,
                        bytes: Some(entry_bytes),
                    }),
                    false => Ok(Top {
                        index: self.index,
                        entry: self.entry,
                        bytes: Some(top_bytes),
                    }),
                }
            }
        }
    }
}

// The bincode bytes of a tie-break.
fn bytes<T: Serialize>(entry: &T) -> Result<Vec<u8>, SpaceError> {
    bincode::serialize(entry)
        .map_err(|_| SpaceError::Inconsistent("a collection entry is not serializable"))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use crypto::rust::hash::blake2b512_random::Blake2b512Random;
    use models::rhoapi::tagged_continuation::TaggedCont;
    use models::rhoapi::{
        BindPattern, CostAuthority, ListParWithRandom, ParWithRandom, TaggedContinuation,
    };
    use models::rust::space_factory_urn::{SpaceFactoryUrn, SpaceInner, SpaceOuter};
    use models::rust::utils::{new_freevar_var, new_gint_par, new_gstring_par};
    use rspace_plus_plus::rspace::hashing::stable_hash_provider;
    use rspace_plus_plus::rspace::rspace::RSpace;
    use rspace_plus_plus::rspace::rspace_interface::ISpace;
    use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
    use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
    use rspace_plus_plus::rspace::space_store_handle::SpaceStoreHandle;
    use rspace_plus_plus::rspace::trace::event::{Event, IOEvent};

    use super::*;
    use crate::rust::interpreter::accounting::costs::Cost;
    use crate::rust::interpreter::accounting::RuntimeBudget;
    use crate::rust::interpreter::matcher::r#match::Matcher;
    use crate::rust::interpreter::spaces::agent::{
        run_op, ConsumeOp, ProduceOp, SpaceAgent, SpaceCreation, SpaceEnv, SpaceOp, SpaceOutcome,
    };
    use crate::rust::interpreter::spaces::generic::{CollectionChannels, GenericSpace, OuterStore};
    use crate::rust::interpreter::spaces::namespace::{
        CollectionKey, Durability, NativeSpace, SID_LEN,
    };
    use crate::rust::interpreter::spaces::outer::hashmap::HashMapOuter;
    use crate::rust::interpreter::spaces::types::config::SpaceConfig;
    use crate::rust::interpreter::spaces::types::qualifier::SpaceQualifier;

    type Stack = GenericSpace<HashMapOuter, StackDiscipline>;
    type ParRSpace = RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>;

    const SPACE: NativeSpace = NativeSpace {
        sid: [5; SID_LEN],
        durability: Durability::Durable,
    };

    fn ordered(entries: &[(i64, String)]) -> Vec<Ordered<'_, String>> {
        entries
            .iter()
            .map(|(ord, entry)| Ordered { ord: *ord, entry })
            .collect()
    }

    fn top_of(entries: &[(i64, String)], side: Side) -> Option<(i64, String)> {
        StackDiscipline
            .head(side, &ordered(entries))
            .expect("strings serialize")
            .and_then(|index| entries.get(index).cloned())
    }

    fn permutations(entries: Vec<(i64, String)>) -> Vec<Vec<(i64, String)>> {
        match entries.len() {
            0 | 1 => vec![entries],
            _ => {
                let mut all = Vec::new();
                for index in 0..entries.len() {
                    let mut rest = entries.clone();
                    let first = rest.remove(index);
                    for mut tail in permutations(rest) {
                        tail.insert(0, first.clone());
                        all.push(tail);
                    }
                }
                all
            }
        }
    }

    #[test]
    fn space_stack_head_is_the_greatest_ord_on_both_sides() {
        let entries = vec![
            (4, "d".to_string()),
            (1, "a".to_string()),
            (7, "g".to_string()),
            (2, "b".to_string()),
        ];
        for side in [Side::Data, Side::Waiters] {
            assert_eq!(top_of(&entries, side), Some((7, "g".to_string())));
        }
    }

    #[test]
    fn space_stack_head_ignores_the_list_order() {
        let entries = vec![
            (7, "x".to_string()),
            (2, "y".to_string()),
            (9, "z".to_string()),
            (5, "w".to_string()),
        ];
        let orders = permutations(entries);
        assert_eq!(orders.len(), 24);
        for order in orders {
            assert_eq!(top_of(&order, Side::Data), Some((9, "z".to_string())));
        }
    }

    #[test]
    fn space_stack_equal_ords_order_by_their_bytes() {
        let entries = vec![
            (3, "b".to_string()),
            (3, "a".to_string()),
            (3, "c".to_string()),
            (2, "z".to_string()),
        ];
        let expected = entries
            .iter()
            .filter(|(ord, _)| *ord == 3)
            .max_by_key(|(_, entry)| bincode::serialize(entry).expect("strings serialize"))
            .cloned();
        assert_eq!(expected, Some((3, "c".to_string())));
        for order in permutations(entries) {
            assert_eq!(top_of(&order, Side::Waiters), expected);
        }
        let duplicated = vec![(1, "s".to_string()), (1, "s".to_string())];
        assert_eq!(
            StackDiscipline.head(Side::Data, &ordered(&duplicated)),
            Ok(Some(0)),
            "byte-identical entries keep the earlier index"
        );
    }

    #[test]
    fn space_stack_empty_side_has_no_head() {
        for side in [Side::Data, Side::Waiters] {
            assert_eq!(StackDiscipline.head::<String>(side, &[]), Ok(None));
        }
    }

    #[test]
    fn space_stack_fresh_elements_always_reach_the_top() {
        for side in [Side::Data, Side::Waiters] {
            for queued in [0, 1, 2, usize::MAX] {
                assert!(StackDiscipline.reaches_head(side, queued));
            }
        }
    }

    #[test]
    fn space_stack_precedes_is_a_greater_ord() {
        for side in [Side::Data, Side::Waiters] {
            assert_eq!(StackDiscipline.precedes(side, 5, 3), Ok(true));
            assert_eq!(StackDiscipline.precedes(side, 3, 5), Ok(false));
            assert_eq!(StackDiscipline.precedes(side, 4, 4), Ok(false));
        }
    }

    #[test]
    fn space_stack_discipline_uses_a_counter_without_params_or_capacity() {
        assert_eq!(StackDiscipline::NAME, "stack");
        assert_eq!(StackDiscipline::NAME, SpaceInner::Stack.as_str());
        assert!(StackDiscipline::USES_COUNTER);
        assert!(StackDiscipline::SEND_PARAMS.is_empty());
        assert!(StackDiscipline::QUERY_PARAMS.is_empty());
        assert_eq!(StackDiscipline::CAPACITY, None);
    }

    // ── Agent-level checks of the settle hooks ────────────────────────────────────────────────

    fn budget() -> RuntimeBudget { RuntimeBudget::new(Cost::create(1_000_000, "stack agent test")) }

    fn int(value: i64) -> Par { new_gint_par(value, Vec::new(), false) }

    fn string(value: &str) -> Par { new_gstring_par(value.to_string(), Vec::new(), false) }

    fn channel() -> Par { string("stack") }

    fn key() -> CollectionKey { CollectionKey::of_channel(&channel()).expect("a key") }

    fn channels() -> CollectionChannels { CollectionChannels::new(&HashMapOuter, SPACE, &key()) }

    fn named(name: &str) -> TaggedContinuation {
        TaggedContinuation {
            tagged_cont: Some(TaggedCont::ParBody(ParWithRandom {
                body: Some(string(name)),
                random_state: Blake2b512Random::create_from_bytes(name.as_bytes()).to_bytes(),
            })),
            guard: None,
            cost_authority: None,
        }
    }

    async fn fixture() -> (ParRSpace, SpaceEnv) {
        let mut stores = InMemoryStoreManager::new();
        let space = ParRSpace::create(
            stores.r_space_stores().await.expect("in-memory stores"),
            Arc::new(Box::new(Matcher)),
        )
        .expect("an in-memory space");
        let env = SpaceEnv {
            store: SpaceStoreHandle::new(space.clone()),
            budget: budget(),
            volatile: Default::default(),
            origin: Default::default(),
        };
        let config = SpaceConfig::new(
            SPACE.sid,
            HashMapOuter::NAME,
            StackDiscipline::NAME,
            SpaceQualifier::Default,
        )
        .to_par();
        Stack::default()
            .prepare_create(&env, SpaceCreation {
                space: SPACE,
                config,
                handle_pattern: BindPattern {
                    patterns: Vec::new(),
                    remainder: Some(new_freevar_var(0)),
                    free_count: 1,
                },
                handle_continuation: named("handle"),
                random: Blake2b512Random::create_from_bytes(b"stack factory call"),
                authority: CostAuthority::default(),
                family: SpaceFactoryUrn {
                    outer: SpaceOuter::HashMap,
                    inner: SpaceInner::Stack,
                },
            })
            .await
            .expect("the creation stages")
            .commit()
            .await
            .expect("the creation commits");
        space.take_event_log().await;
        (space, env)
    }

    fn send(value: i64) -> ProduceOp {
        ProduceOp {
            space: SPACE,
            key: key(),
            channel: channel(),
            data: ListParWithRandom {
                pars: vec![int(value)],
                random_state: Blake2b512Random::create_from_bytes(
                    format!("stack send {value}").as_bytes(),
                )
                .to_bytes(),
                cost_authority: None,
                cost_stack: None,
            },
            persistent: false,
            params: BTreeMap::new(),
            authority: CostAuthority::default(),
            reissued_ord: None,
        }
    }

    fn exactly(value: i64) -> BindPattern {
        BindPattern {
            patterns: vec![int(value)],
            remainder: None,
            free_count: 0,
        }
    }

    fn receive(name: &str, pattern: BindPattern) -> ConsumeOp {
        let body = match named(name).tagged_cont {
            Some(TaggedCont::ParBody(body)) => body,
            Some(TaggedCont::ScalaBodyRef(_)) | None => ParWithRandom::default(),
        };
        ConsumeOp {
            space: SPACE,
            key: key(),
            channel: channel(),
            pattern,
            body,
            persistent: false,
            peek: false,
            guard: None,
            query: None,
            authority: CostAuthority::default(),
            introduction_authority: CostAuthority::default(),
            reissued_ord: None,
        }
    }

    async fn run(env: &SpaceEnv, op: SpaceOp) -> SpaceOutcome {
        run_op(&Stack::default(), env, op)
            .await
            .expect("the operation commits")
    }

    fn name_of(
        hash: &rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash,
    ) -> &'static str {
        let ch = channels();
        match [ch.counter(), ch.data(), ch.space_counter()]
            .iter()
            .position(|channel| stable_hash_provider::hash(channel) == *hash)
        {
            Some(0) => "K",
            Some(1) => "D",
            Some(_) => "S",
            None => "?",
        }
    }

    fn labels(log: &[Event]) -> Vec<String> {
        log.iter()
            .map(|event| match event {
                Event::IoEvent(IOEvent::Produce(produce)) => {
                    format!("produce {}", name_of(&produce.channel_hash))
                }
                Event::IoEvent(IOEvent::Consume(consume)) => {
                    format!("consume {}", name_of(&consume.channel_hashes[0]))
                }
                Event::Comm(comm) => format!("comm {}", name_of(&comm.consume.channel_hashes[0])),
            })
            .collect()
    }

    /// The counterexample of v2 section 3.1: W2(a) and W(b) park, h(a) is stored under the top
    /// waiter W(b), and d(b) fires W(b). The tops h(a) and W2(a) then match, and the settle after
    /// the plan lifts h and offers it again to W2.
    #[tokio::test]
    async fn space_stack_settle_fires_the_exposed_top_pair() {
        let (space, env) = fixture().await;
        run(&env, SpaceOp::Consume(receive("w2", exactly(1)))).await;
        run(&env, SpaceOp::Consume(receive("w", exactly(2)))).await;
        assert_eq!(
            run(&env, SpaceOp::Produce(send(1))).await,
            SpaceOutcome::default(),
            "the top waiter blocks the datum"
        );
        space.take_event_log().await;
        let outcome = run(&env, SpaceOp::Produce(send(2))).await;
        assert_eq!(
            outcome.fires.len(),
            2,
            "the plan fires w, and the settle fires w2"
        );
        assert_eq!(outcome.reissue, None);
        assert_eq!(labels(&space.take_event_log().await), vec![
            "consume K",
            "comm K",
            "produce K",
            "produce D",
            "comm D",
            "consume D",
            "comm D",
            "produce D",
            "comm D",
        ]);
        assert!(space.get_data(channels().data()).await.is_empty());
        assert!(space
            .get_waiting_continuations(vec![channels().data().clone()])
            .await
            .is_empty());
    }

    #[tokio::test]
    async fn space_stack_settle_job_with_nothing_owed_only_bumps() {
        let (space, env) = fixture().await;
        run(&env, SpaceOp::Produce(send(1))).await;
        space.take_event_log().await;
        let settle_op = SettleOp {
            space: SPACE,
            key: key(),
            channel: channel(),
            random_state: vec![1, 2, 3],
            authority: CostAuthority::default(),
            introduction_authority: CostAuthority::default(),
            cascade: None,
        };
        assert_eq!(
            run(&env, SpaceOp::Settle(settle_op)).await,
            SpaceOutcome::default()
        );
        assert_eq!(labels(&space.take_event_log().await), vec![
            "consume K",
            "comm K",
            "produce K"
        ]);
        assert_eq!(space.get_data(channels().data()).await.len(), 1);
    }

    /// Added by decision 124 (plan v4 C7.8, rule CR-20): the stack owes no fire for a top pair
    /// that the book holds. W2(1) and W(2) park, h(1) is stored under the top waiter W, and the
    /// book holds the pair of h and W2. The send d(2) fires W, and the settle after the plan
    /// leaves the exposed top pair h and W2 in place.
    #[tokio::test]
    async fn stack_view_owes_no_fire_for_a_booked_top_pair() {
        let (space, env) = fixture().await;
        run(&env, SpaceOp::Consume(receive("w2", exactly(1)))).await;
        run(&env, SpaceOp::Consume(receive("w", exactly(2)))).await;
        run(&env, SpaceOp::Produce(send(1))).await;
        let record = match space.get_data(channels().data()).await.as_slice() {
            [h] => h.source.hash.clone(),
            other => panic!("h is stored alone, not {} records", other.len()),
        };
        let waiter = space
            .get_waiting_continuations(vec![channels().data().clone()])
            .await
            .into_iter()
            .find(|waiter| waiter.patterns.as_slice() == [exactly(1)])
            .expect("W2 waits")
            .source
            .hash;
        env.volatile.book_pairs([(record, waiter)]);
        let outcome = run(&env, SpaceOp::Produce(send(2))).await;
        assert_eq!(
            outcome.fires.len(),
            1,
            "the plan fires w, and the settle fires nothing"
        );
        assert_eq!(
            space.get_data(channels().data()).await.len(),
            1,
            "h stays stored"
        );
        assert_eq!(
            space
                .get_waiting_continuations(vec![channels().data().clone()])
                .await
                .len(),
            1,
            "W2 stays parked"
        );
    }
}
