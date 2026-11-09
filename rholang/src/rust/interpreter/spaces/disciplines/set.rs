//! The set discipline of the native spaces (P5 design v3, A.7, family `hashmap:set`).
//!
//! A set is a bag whose records form classes. Two records are members of one class when their
//! payloads have equal set keys (F5). So equal values form one class, whatever their random
//! states, their cost authorities and their cost stacks. The store keeps every record. Only the
//! view of a set removes the duplicates: a receive takes a whole class, and a fire of a send
//! removes the stored members of its class.
//!
//! The plans are the scan driver of [`super::scan`] with the class key of this module:
//!
//! * A receive takes the first record in its pick order whose payload its pattern binds. Play
//!   shuffles the canonical order of DR-75, and replay puts first the records that the recorded
//!   row names (rules CR-1 and CR-4). Every member of the class of that record binds the pattern
//!   too. The receive checks its guard once, on the first member of the class in DR-75 order
//!   (decision 37, decision 41, decision 119 G1). The members have equal payloads, so each
//!   member gives the same verdict.
//! * A receive that does not peek fires one consume over `[D]` with the first member of the
//!   class in DR-75 order, and the outcome dispatches it (decision 119 G1, decision 101). Then
//!   RETIRE-CLASS retires each other member that is not persistent, in DR-75 order, with a
//!   single-channel `K_retire` COMM. So a class with a persistent member stays in the set.
//! * A send fires the first waiter in its pick order whose pattern binds the datum and whose
//!   guard admits it (rules CR-1 and CR-4). After a fire of a waiter that does not peek, the
//!   send retires the stored members of its class that are not persistent. Each retire is a
//!   single-channel `K_retire` COMM (decision 37). Only a guarded waiter that parked while a
//!   stored member passes its guard causes this case (readiness review S3).
//! * A peek binds the record of its pick alone on `[D]`. Its fire removes that record when it
//!   is not persistent, and RESTORE-SCAN sends the record again (decision 45, Rule E). A peek
//!   retires nothing. A fire of the second send on a waiter that does not peek retires the class.
//!
//! Changed by decision 102 (rules CR-1, CR-4, CR-12 and CR-13, decision 119 G1, decision 101).
//! The text before:
//!
//! ```text
//! view of a set removes the duplicates: a receive binds a whole class, and a fire of a send
//! removes the stored members of its class.
//!
//! * A receive takes the first record in the canonical order of DR-75 whose payload its pattern
//!   binds. It checks its guard once, on that record (decision 37, decision 41). Every member of
//!   the class of that record binds the pattern too. So the first record is the first member of
//!   its class in canonical order, the representative of the class.
//! * A receive that does not peek fires one consume over `[D; n]` with the `n` stored members of
//!   the class, the representative first. The outcome dispatches the result of the
//!   representative only (F3, F20). The fire removes every member that is not persistent, so a
//!   class with a persistent member stays in the set.
//! * A send fires the first waiter in canonical order whose pattern binds the datum and whose
//!   guard admits it. [...]
//! * A peek binds the representative alone on `[D]`. Its fire removes a representative that is
//!   not persistent, and RESTORE-SCAN sends the record again (decision 45, Rule E). [...]
//! ```
//!
//! A set has no counter and no params. Every record and every parked continuation has the `ord`
//! 0, so a re-issued element keeps its record and its identity (decision 43). Waiters are equal
//! only when they are byte-identical (F14). Two receives differ in their random states, so they
//! stay two waiters.
//!
//! **The set key (F5).** The key of a payload `p_1 … p_n` frames the count and the length of
//! each element:
//!
//! ```text
//! set_key(p_1 … p_n) = u64_be(n) ‖ u64_be(len_1) ‖ b_1 ‖ … ‖ u64_be(len_n) ‖ b_n
//!          where b_i = bincode(ParSortMatcher::sort_match(p_i).term), len_i = |b_i|
//! ```
//!
//! The framing makes the key injective on lists of element encodings. So two payloads have equal
//! keys exactly when their sorted elements are equal (`SetDiscipline.v`, `set_key_injective`).
//! bincode writes `locally_free` as empty bytes, and `Par` equality also ignores it. A failed
//! sort or encode is a typed error before any staging, never an empty key.
//!
//! **Key comparison.** The driver compares two keys by their hashes first. It compares the full
//! keys only on equal hashes (DR-90, `scan::same_class`).
//!
//! The legacy set removed duplicates at write time by a 64-bit `DefaultHasher` of the debug text
//! of a value. Its consume removed one member. Both are replaced: the store keeps every record,
//! the key compares full bytes, and a consume retires the whole class.

use std::mem::size_of;

use models::rhoapi::{ListParWithRandom, Par};
use models::rust::rholang::sorter::par_sort_matcher::ParSortMatcher;
use models::rust::rholang::sorter::sortable::Sortable;
use serde::Serialize;

use super::queue::QueueDiscipline;
use super::scan::{self, ScanDiscipline};
use crate::rust::interpreter::spaces::agent::{ConsumeOp, ProduceOp, SpaceReissue};
use crate::rust::interpreter::spaces::errors::SpaceError;
use crate::rust::interpreter::spaces::generic::{Cx, Discipline, JoinWindow, Ordered, Side};

/// The bytes of one length field of a set key: a big-endian `u64`.
pub const LENGTH_BYTES: usize = size_of::<u64>();

/// The set discipline.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct SetDiscipline;

impl Discipline for SetDiscipline {
    const NAME: &'static str = "set";
    const USES_COUNTER: bool = false;
    const SEND_PARAMS: &'static [&'static str] = &[];
    /// A join bind sees one representative for each class, and it claims its whole class (P5c).
    const JOIN_WINDOW: JoinWindow = JoinWindow::Classes;
    /// Added by decision 102 (plan v4 C4.3, rule CR-1): a set picks at random in play.
    const RANDOM_PICKS: bool = true;

    /// The entry with the least `(ord, bincode bytes)`, as for the queue and the bag. Every
    /// entry of a set has the `ord` 0. Only the head driver uses a head. A set does not use the
    /// head driver, because its plans are the scan driver.
    fn head<T: Serialize>(
        &self,
        side: Side,
        entries: &[Ordered<'_, T>],
    ) -> Result<Option<usize>, SpaceError> {
        QueueDiscipline.head(side, entries)
    }

    /// Every entry of a set can meet an incoming element. Only the head driver asks, and a set
    /// does not use it.
    fn reaches_head(&self, _side: Side, _queued: usize) -> bool { true }

    /// The lesser order interacts first, as for the queue. Only the re-entry check of the head
    /// driver asks, and a set does not use it.
    fn precedes(&self, side: Side, ord: i64, head: i64) -> Result<bool, SpaceError> {
        QueueDiscipline.precedes(side, ord, head)
    }

    /// OFFER-SCAN of the scan driver ([`scan::offer`]). RETIRE-CLASS follows a fire of a waiter
    /// that does not peek.
    fn offer(
        &self,
        cx: &mut Cx<'_, '_>,
        op: ProduceOp,
        ord: i64,
    ) -> Result<Option<SpaceReissue>, SpaceError> {
        scan::offer(self, cx, op, ord)
    }

    /// RECEIVE-SCAN of the scan driver ([`scan::receive`]). It fires one bind over `[D]` with the
    /// first member of the class in DR-75 order, and RETIRE-CLASS retires the other members. A
    /// peek fires the record of its pick alone, and RESTORE-SCAN follows. Changed by decision
    /// 102 (decision 119 G1, decision 101). The text before: "It fires over `[D; n]` for a
    /// class, or over the representative alone for a peek."
    fn receive(
        &self,
        cx: &mut Cx<'_, '_>,
        op: ConsumeOp,
        ord: i64,
    ) -> Result<Option<SpaceReissue>, SpaceError> {
        scan::receive(self, cx, op, ord)
    }

    /// RESTORE-SCAN of the scan driver ([`scan::restore`]). The record is offered to the waiters
    /// again before it is stored. The host sends a peeked datum again in the same way (decision
    /// 45, Rule E).
    fn restore(
        &self,
        cx: &mut Cx<'_, '_>,
        record: ListParWithRandom,
    ) -> Result<Option<SpaceReissue>, SpaceError> {
        scan::restore(self, cx, record)
    }

    /// The class of a payload in a join window: its F5 set key, as for the scan driver
    /// ([`ScanDiscipline::class_key`]).
    fn join_class_key(&self, payload: &[Par]) -> Result<Option<Vec<u8>>, SpaceError> {
        ScanDiscipline::class_key(self, payload)
    }
}

/// Every payload of a set has a class: the payloads with equal set keys.
impl ScanDiscipline for SetDiscipline {
    /// The F5 set key of the payload ([`set_key`]).
    fn class_key(&self, payload: &[Par]) -> Result<Option<Vec<u8>>, SpaceError> {
        Ok(Some(set_key(payload)?))
    }
}

/// The F5 set key of the payload `payload` (P5 design v3, A.7). The key holds the count of the
/// elements, then the length and the canonical bytes of each element. Each count and each length
/// is a big-endian `u64`.
///
/// The key covers the payload without its envelope. The random state, the cost authority and the
/// cost stack of a record lie outside its payload, so they play no part. The canonical bytes of
/// each element are computed first ([`canonical_bytes`]). Then the key buffer of the exact
/// length is allocated and filled.
pub fn set_key(payload: &[Par]) -> Result<Vec<u8>, SpaceError> {
    let mut elements = Vec::<Vec<u8>>::with_capacity(payload.len());
    let mut length = LENGTH_BYTES;
    for element in payload {
        let bytes = canonical_bytes(element)?;
        length += LENGTH_BYTES + bytes.len();
        elements.push(bytes);
    }
    let mut key = Vec::<u8>::with_capacity(length);
    key.extend_from_slice(&u64_be(payload.len())?);
    for bytes in &elements {
        key.extend_from_slice(&u64_be(bytes.len())?);
        key.extend_from_slice(bytes);
    }
    Ok(key)
}

/// The canonical bytes of one element `element` of a payload: the bincode bytes of its sorted
/// `Par`. An encode that fails is `Inconsistent`, as for the tie-break encode of the queue.
pub fn canonical_bytes(element: &Par) -> Result<Vec<u8>, SpaceError> {
    let sorted = sorted_par(element);
    bincode::serialize(&sorted)
        .map_err(|_| SpaceError::Inconsistent("a payload element of a set does not encode"))
}

/// The sorted `Par` of `element`, sorted as the host sorts.
fn sorted_par(element: &Par) -> Par { ParSortMatcher::sort_match(element).term }

/// The big-endian bytes of `length` as a `u64`. A length above `u64::MAX` is `Inconsistent`.
fn u64_be(length: usize) -> Result<[u8; LENGTH_BYTES], SpaceError> {
    Ok(u64::try_from(length)
        .map_err(|_| SpaceError::Inconsistent("a length of a set key exceeds u64"))?
        .to_be_bytes())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use crypto::rust::hash::blake2b512_random::Blake2b512Random;
    use models::rhoapi::expr::ExprInstance;
    use models::rhoapi::{BindPattern, CostAuthority, EList, Expr, TaggedContinuation};
    use models::rust::space_factory_urn::{SpaceFactoryUrn, SpaceInner, SpaceOuter};
    use models::rust::utils::{new_freevar_par, new_gint_par, new_gstring_par};
    use rspace_plus_plus::rspace::rspace::RSpace;
    use rspace_plus_plus::rspace::rspace_interface::ISpace;
    use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
    use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
    use rspace_plus_plus::rspace::space_store_handle::SpaceStoreHandle;
    // Changed by decision 102 (decision 119 G1): the test names the DR-75 order of the members.
    // use rspace_plus_plus::rspace::trace::event::{Event, IOEvent};
    use rspace_plus_plus::rspace::trace::event::{Event, IOEvent, Produce};

    use super::*;
    use crate::rust::interpreter::accounting::costs::Cost;
    use crate::rust::interpreter::accounting::RuntimeBudget;
    use crate::rust::interpreter::matcher::r#match::Matcher;
    use crate::rust::interpreter::spaces::agent::{
        run_op, SpaceAgent, SpaceCreation, SpaceEnv, SpaceOp, SpaceOutcome,
    };
    use crate::rust::interpreter::spaces::disciplines::bag::BagDiscipline;
    use crate::rust::interpreter::spaces::generic::{GenericSpace, OuterStore};
    use crate::rust::interpreter::spaces::namespace::{
        self, CollectionKey, Durability, NativeSpace, SID_LEN,
    };
    use crate::rust::interpreter::spaces::outer::hashmap::HashMapOuter;
    use crate::rust::interpreter::spaces::record;
    use crate::rust::interpreter::spaces::types::config::SpaceConfig;
    use crate::rust::interpreter::spaces::types::qualifier::SpaceQualifier;

    type Set = GenericSpace<HashMapOuter, SetDiscipline>;
    type ParRSpace = RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>;

    const SPACE: NativeSpace = NativeSpace {
        sid: [17; SID_LEN],
        durability: Durability::Durable,
    };

    fn int(value: i64) -> Par { new_gint_par(value, Vec::new(), false) }

    fn string(value: &str) -> Par { new_gstring_par(value.to_string(), Vec::new(), false) }

    /// A `Par` with the expressions `values`, in this order.
    fn exprs(values: &[i64]) -> Par {
        Par {
            exprs: values
                .iter()
                .map(|value| Expr {
                    expr_instance: Some(ExprInstance::GInt(*value)),
                })
                .collect(),
            ..Par::default()
        }
    }

    /// The bincode bytes of the sorted `element`, as the host sorts it.
    fn sorted_bytes(element: &Par) -> Vec<u8> {
        bincode::serialize(&ParSortMatcher::sort_match(element).term).expect("a Par serializes")
    }

    /// A list nested `depth` levels deep around the integer 0.
    fn nested(depth: usize) -> Par {
        let mut par = int(0);
        for _ in 0..depth {
            par = Par {
                exprs: vec![Expr {
                    expr_instance: Some(ExprInstance::EListBody(EList {
                        ps: vec![par],
                        ..EList::default()
                    })),
                }],
                ..Par::default()
            };
        }
        par
    }

    #[test]
    fn space_set_discipline_has_no_counter_params_query_or_capacity() {
        assert_eq!(SetDiscipline::NAME, "set");
        assert!(!SetDiscipline::USES_COUNTER);
        assert!(SetDiscipline::SEND_PARAMS.is_empty());
        assert!(SetDiscipline::QUERY_PARAMS.is_empty());
        assert_eq!(SetDiscipline::CAPACITY, None);
        let entries = [(0, "b".to_string()), (0, "a".to_string())];
        let ordered: Vec<Ordered<'_, String>> = entries
            .iter()
            .map(|(ord, entry)| Ordered { ord: *ord, entry })
            .collect();
        for side in [Side::Data, Side::Waiters] {
            assert_eq!(
                SetDiscipline.head(side, &ordered),
                BagDiscipline.head(side, &ordered),
                "the head rule of the bag"
            );
            for queued in [0, 1, usize::MAX] {
                assert!(SetDiscipline.reaches_head(side, queued));
            }
            assert_eq!(SetDiscipline.precedes(side, 0, 1), Ok(true));
            assert_eq!(SetDiscipline.precedes(side, 1, 0), Ok(false));
        }
    }

    #[test]
    fn space_set_key_frames_the_count_and_each_length() {
        let payload = [int(7), string("seven")];
        let first = sorted_bytes(&payload[0]);
        let second = sorted_bytes(&payload[1]);
        let mut expected = Vec::new();
        expected.extend_from_slice(&2u64.to_be_bytes());
        expected.extend_from_slice(&(first.len() as u64).to_be_bytes());
        expected.extend_from_slice(&first);
        expected.extend_from_slice(&(second.len() as u64).to_be_bytes());
        expected.extend_from_slice(&second);
        assert_eq!(set_key(&payload), Ok(expected));
        assert_eq!(
            set_key(&[]),
            Ok(0u64.to_be_bytes().to_vec()),
            "an empty payload has the key of the count 0"
        );
        assert_eq!(
            SetDiscipline.class_key(&payload),
            set_key(&payload).map(Some),
            "every payload of a set has a class"
        );
    }

    #[test]
    fn space_set_key_sorts_each_element_and_keeps_the_element_order() {
        assert_ne!(exprs(&[1, 2]), exprs(&[2, 1]), "two orders of one Par");
        assert_eq!(
            set_key(&[exprs(&[1, 2])]),
            set_key(&[exprs(&[2, 1])]),
            "the key of an element is the key of its sorted Par"
        );
        assert_ne!(
            set_key(&[int(1), int(2)]),
            set_key(&[int(2), int(1)]),
            "the elements of a payload keep their positions"
        );
        assert_ne!(
            set_key(&[int(1)]),
            set_key(&[int(1), int(1)]),
            "the count separates the arities"
        );
        let mut free = int(3);
        free.locally_free = vec![0b101];
        assert_eq!(
            set_key(&[free]),
            set_key(&[int(3)]),
            "bincode writes locally_free as empty bytes"
        );
    }

    /// The encode and the drop of a deep term recurse once for each level. So the test runs on a
    /// thread with a large stack, as the reducer runs on a large stack.
    #[test]
    fn space_set_key_sorts_a_deep_term() {
        std::thread::Builder::new()
            .stack_size(256 * 1024 * 1024)
            .spawn(|| {
                let deep = [nested(260)];
                let key = set_key(&deep).expect("the host sorts any depth");
                assert_eq!(&key[..LENGTH_BYTES], &1u64.to_be_bytes());
                assert_eq!(
                    set_key(&deep).as_ref(),
                    Ok(&key),
                    "the key of a deep term is deterministic"
                );
                assert_ne!(
                    set_key(&[nested(8)]).as_ref(),
                    Ok(&key),
                    "two depths give two keys"
                );
            })
            .expect("a test thread starts")
            .join()
            .expect("the deep-term test passes");
    }

    fn channel() -> Par { string("set") }

    fn key() -> CollectionKey { CollectionKey::of_channel(&channel()).expect("a key") }

    fn data() -> Par { namespace::gprivate(namespace::data_channel(SPACE, &key())) }

    async fn fixture() -> (ParRSpace, SpaceEnv) {
        let mut stores = InMemoryStoreManager::new();
        let space = ParRSpace::create(
            stores.r_space_stores().await.expect("in-memory stores"),
            Arc::new(Box::new(Matcher)),
        )
        .expect("an in-memory space");
        let env = SpaceEnv {
            store: SpaceStoreHandle::new(space.clone()),
            budget: RuntimeBudget::new(Cost::create(1_000_000, "set discipline test")),
            volatile: Default::default(),
            origin: Default::default(),
        };
        let config = SpaceConfig::new(
            SPACE.sid,
            HashMapOuter::NAME,
            SetDiscipline::NAME,
            SpaceQualifier::Default,
        )
        .to_par();
        let creation = SpaceCreation {
            space: SPACE,
            config,
            handle_pattern: BindPattern {
                patterns: Vec::new(),
                remainder: Some(models::rust::utils::new_freevar_var(0)),
                free_count: 1,
            },
            handle_continuation: TaggedContinuation::default(),
            random: Blake2b512Random::create_from_bytes(b"set factory call"),
            authority: CostAuthority::default(),
            family: SpaceFactoryUrn {
                outer: SpaceOuter::HashMap,
                inner: SpaceInner::Set,
            },
        };
        Set::default()
            .prepare_create(&env, creation)
            .await
            .expect("the creation stages")
            .commit()
            .await
            .expect("the creation commits");
        space.take_event_log().await;
        (space, env)
    }

    fn send(value: i64, salt: u64) -> ProduceOp {
        ProduceOp {
            space: SPACE,
            key: key(),
            channel: channel(),
            data: ListParWithRandom {
                pars: vec![int(value)],
                random_state: Blake2b512Random::create_from_bytes(
                    format!("set send {value} {salt}").as_bytes(),
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

    fn receive(name: &str, pattern: BindPattern) -> ConsumeOp {
        ConsumeOp {
            space: SPACE,
            key: key(),
            channel: channel(),
            pattern,
            body: models::rhoapi::ParWithRandom {
                body: Some(string(name)),
                random_state: Blake2b512Random::create_from_bytes(name.as_bytes()).to_bytes(),
            },
            persistent: false,
            peek: false,
            guard: None,
            query: None,
            authority: CostAuthority::default(),
            introduction_authority: CostAuthority::default(),
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

    fn any() -> BindPattern {
        BindPattern {
            patterns: vec![new_freevar_par(0, Vec::new())],
            remainder: None,
            free_count: 1,
        }
    }

    async fn run(env: &SpaceEnv, op: SpaceOp) -> SpaceOutcome {
        run_op(&Set::default(), env, op)
            .await
            .expect("the operation commits")
    }

    /// The values of the records of the collection with their orders, sorted.
    async fn stored(space: &ParRSpace) -> Vec<(i64, Vec<Par>)> {
        let mut stored: Vec<(i64, Vec<Par>)> = space
            .get_data(&data())
            .await
            .iter()
            .map(|datum| {
                let record = record::unwrap_datum(&datum.a).expect("a datum record");
                (record.ord, record.datum.pars)
            })
            .collect();
        stored.sort_by_key(|(_, pars)| format!("{pars:?}"));
        stored
    }

    #[tokio::test]
    async fn space_set_agent_stores_every_record_and_a_receive_takes_the_class() {
        let (space, env) = fixture().await;
        for (value, salt) in [(1, 1), (1, 2), (2, 1)] {
            assert_eq!(
                run(&env, SpaceOp::Produce(send(value, salt))).await,
                SpaceOutcome::default()
            );
        }
        assert_eq!(stored(&space).await, vec![
            (0, vec![int(1)]),
            (0, vec![int(1)]),
            (0, vec![int(2)])
        ]);
        let counter = namespace::gprivate(namespace::collection_counter(SPACE, &key()));
        assert!(space.get_data(&counter).await.is_empty(), "no counter");
        space.take_event_log().await;
        let outcome = run(&env, SpaceOp::Consume(receive("k", exactly(1)))).await;
        let [fire] = outcome.fires.as_slice() else {
            panic!("one fire: {outcome:?}");
        };
        // Changed by decision 102 (decision 119 G1, decision 101): the fire binds the first
        // member of the class in DR-75 order, and one `K_retire` row retires the other member.
        // The text before:
        // assert_eq!(
        //     fire.results.len(),
        //     1,
        //     "the representative alone is dispatched"
        // );
        let [result] = fire.results.as_slice() else {
            panic!("the fire binds one member: {fire:?}");
        };
        let first = [send(1, 1), send(1, 2)]
            .iter()
            .map(|op| record::wrap_datum(0, &op.data))
            .min_by_key(|record| Produce::create(&data(), record, false).hash)
            .expect("two members");
        assert_eq!(
            result.removed_datum, first,
            "the first member in DR-75 order is dispatched"
        );
        let log = space.take_event_log().await;
        let widths: Vec<usize> = log
            .iter()
            .filter_map(|event| match event {
                Event::IoEvent(IOEvent::Consume(consume)) => Some(consume.channel_hashes.len()),
                Event::Comm(comm) => Some(comm.produces.len()),
                Event::IoEvent(IOEvent::Produce(_)) => None,
            })
            .collect();
        // Changed by decision 102 (decision 101). The text before:
        // assert_eq!(widths, vec![2, 2], "one consume and one COMM over [D; 2]");
        assert_eq!(
            widths,
            vec![1, 1, 1, 1],
            "the fire over [D], then one K_retire row over [D]"
        );
        assert_eq!(stored(&space).await, vec![(0, vec![int(2)])]);
        let outcome = run(&env, SpaceOp::Consume(receive("w", any()))).await;
        assert_eq!(outcome.fires.len(), 1);
        assert!(stored(&space).await.is_empty());
    }
}
