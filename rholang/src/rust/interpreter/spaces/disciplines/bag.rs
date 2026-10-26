//! The bag discipline of the native spaces (P5 design v3, A.7, family `hashmap:bag`).
//!
//! A bag is a multiset on both sides. Every record and every waiting continuation of a
//! collection can meet an incoming element, so nothing blocks. A receive takes the first record
//! in the canonical order of DR-75 whose payload its pattern binds, and it checks its guard once
//! on that record (decision 37). A send fires the first waiter in canonical order whose pattern
//! binds the datum and whose guard admits it. A guard refusal moves the send to the next waiter.
//! The canonical order sorts the entries by the hash of their source produce or consume, so the
//! choices depend on the entry set, not on the list order of the store (decision 41).
//!
//! A bag has no counter and no params. Every record and every parked continuation has the `ord`
//! 0, so two equal sends store two equal records, and a re-issued element keeps its record and
//! its identity (decision 43). A peek fire removes a non-persistent record, and the same
//! operation sends the record again (decision 45, Rule E). The plans are the scan driver of
//! [`super::scan`].
//!
//! The legacy bag kept its records in a list and removed the first match by list position
//! (`find_and_remove`, then `swap_remove`), so its choice depended on the insertion history. The
//! legacy waiter side scanned every continuation without a guard. Both are replaced.

use models::rhoapi::ListParWithRandom;
use serde::Serialize;

use super::queue::QueueDiscipline;
use super::scan::{self, ScanDiscipline};
use crate::rust::interpreter::spaces::agent::{ConsumeOp, ProduceOp, SpaceReissue};
use crate::rust::interpreter::spaces::errors::SpaceError;
use crate::rust::interpreter::spaces::generic::{Cx, Discipline, JoinWindow, Ordered, Side};

/// The bag discipline.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct BagDiscipline;

impl Discipline for BagDiscipline {
    const NAME: &'static str = "bag";
    const USES_COUNTER: bool = false;
    const SEND_PARAMS: &'static [&'static str] = &[];
    /// A join bind sees every stored record in the canonical order of DR-75 (P5c).
    const JOIN_WINDOW: JoinWindow = JoinWindow::Scan;

    /// The entry with the least `(ord, bincode bytes)`, as for the queue. Every entry of a bag
    /// has the `ord` 0, so it is the entry with the least bytes. Only the head driver uses a
    /// head, and a bag does not use the head driver: its plans are the scan driver.
    fn head<T: Serialize>(
        &self,
        side: Side,
        entries: &[Ordered<'_, T>],
    ) -> Result<Option<usize>, SpaceError> {
        QueueDiscipline.head(side, entries)
    }

    /// Every entry of a bag can meet an incoming element, so an incoming element always reaches
    /// the head. Only the head driver asks, and a bag does not use it.
    fn reaches_head(&self, _side: Side, _queued: usize) -> bool { true }

    /// The lesser order interacts first, as for the queue. Only the re-entry check of the head
    /// driver asks, and a bag does not use it.
    fn precedes(&self, side: Side, ord: i64, head: i64) -> Result<bool, SpaceError> {
        QueueDiscipline.precedes(side, ord, head)
    }

    /// OFFER-SCAN of the scan driver ([`scan::offer`]).
    fn offer(
        &self,
        cx: &mut Cx<'_, '_>,
        op: ProduceOp,
        ord: i64,
    ) -> Result<Option<SpaceReissue>, SpaceError> {
        scan::offer(self, cx, op, ord)
    }

    /// RECEIVE-SCAN of the scan driver ([`scan::receive`]), with RESTORE-SCAN after a peek fire.
    fn receive(
        &self,
        cx: &mut Cx<'_, '_>,
        op: ConsumeOp,
        ord: i64,
    ) -> Result<Option<SpaceReissue>, SpaceError> {
        scan::receive(self, cx, op, ord)
    }

    /// RESTORE-SCAN of the scan driver ([`scan::restore`]): the record is offered to the waiters
    /// again before it is stored, as the host sends a peeked datum again (decision 45, Rule E).
    fn restore(
        &self,
        cx: &mut Cx<'_, '_>,
        record: ListParWithRandom,
    ) -> Result<Option<SpaceReissue>, SpaceError> {
        scan::restore(self, cx, record)
    }
}

/// A bag has no classes: every record is a class of its own, so a receive binds one record and
/// a send retires nothing.
impl ScanDiscipline for BagDiscipline {}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use crypto::rust::hash::blake2b512_random::Blake2b512Random;
    use models::rhoapi::{BindPattern, CostAuthority, Par, TaggedContinuation};
    use models::rust::space_factory_urn::{SpaceFactoryUrn, SpaceInner, SpaceOuter};
    use models::rust::utils::{new_freevar_par, new_gint_par};
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
        run_op, SpaceAgent, SpaceCreation, SpaceEnv, SpaceOp, SpaceOutcome,
    };
    use crate::rust::interpreter::spaces::generic::{GenericSpace, OuterStore};
    use crate::rust::interpreter::spaces::namespace::{
        self, CollectionKey, Durability, NativeSpace, SID_LEN,
    };
    use crate::rust::interpreter::spaces::outer::hashmap::HashMapOuter;
    use crate::rust::interpreter::spaces::record;
    use crate::rust::interpreter::spaces::types::config::SpaceConfig;
    use crate::rust::interpreter::spaces::types::qualifier::SpaceQualifier;

    type Bag = GenericSpace<HashMapOuter, BagDiscipline>;
    type ParRSpace = RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>;

    const SPACE: NativeSpace = NativeSpace {
        sid: [11; SID_LEN],
        durability: Durability::Durable,
    };

    fn ordered(entries: &[(i64, String)]) -> Vec<Ordered<'_, String>> {
        entries
            .iter()
            .map(|(ord, entry)| Ordered { ord: *ord, entry })
            .collect()
    }

    fn int(value: i64) -> Par { new_gint_par(value, Vec::new(), false) }

    async fn fixture() -> (ParRSpace, SpaceEnv) {
        let mut stores = InMemoryStoreManager::new();
        let space = ParRSpace::create(
            stores.r_space_stores().await.expect("in-memory stores"),
            Arc::new(Box::new(Matcher)),
        )
        .expect("an in-memory space");
        let env = SpaceEnv {
            store: SpaceStoreHandle::new(space.clone()),
            budget: RuntimeBudget::new(Cost::create(1_000_000, "bag discipline test")),
            volatile: Default::default(),
            origin: Default::default(),
        };
        let config = SpaceConfig::new(
            SPACE.sid,
            HashMapOuter::NAME,
            BagDiscipline::NAME,
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
            random: Blake2b512Random::create_from_bytes(b"bag factory call"),
            authority: CostAuthority::default(),
            family: SpaceFactoryUrn {
                outer: SpaceOuter::HashMap,
                inner: SpaceInner::Bag,
            },
        };
        Bag::default()
            .prepare_create(&env, creation)
            .await
            .expect("the creation stages")
            .commit()
            .await
            .expect("the creation commits");
        space.take_event_log().await;
        (space, env)
    }

    fn channel() -> Par {
        models::rust::utils::new_gstring_par("bag".to_string(), Vec::new(), false)
    }

    fn key() -> CollectionKey { CollectionKey::of_channel(&channel()).expect("a key") }

    fn send(value: i64) -> ProduceOp {
        ProduceOp {
            space: SPACE,
            key: key(),
            channel: channel(),
            data: ListParWithRandom {
                pars: vec![int(value)],
                random_state: Blake2b512Random::create_from_bytes(
                    format!("send {value}").as_bytes(),
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

    fn receive(name: &str) -> ConsumeOp {
        ConsumeOp {
            space: SPACE,
            key: key(),
            channel: channel(),
            pattern: BindPattern {
                patterns: vec![new_freevar_par(0, Vec::new())],
                remainder: None,
                free_count: 1,
            },
            body: models::rhoapi::ParWithRandom {
                body: Some(models::rust::utils::new_gstring_par(
                    name.to_string(),
                    Vec::new(),
                    false,
                )),
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

    async fn run(env: &SpaceEnv, op: SpaceOp) -> SpaceOutcome {
        run_op(&Bag::default(), env, op)
            .await
            .expect("the operation commits")
    }

    #[test]
    fn space_bag_discipline_has_no_counter_params_query_or_capacity() {
        assert_eq!(BagDiscipline::NAME, "bag");
        assert!(!BagDiscipline::USES_COUNTER);
        assert!(BagDiscipline::SEND_PARAMS.is_empty());
        assert!(BagDiscipline::QUERY_PARAMS.is_empty());
        assert_eq!(BagDiscipline::CAPACITY, None);
    }

    #[test]
    fn space_bag_head_is_the_least_ord_and_bytes_on_both_sides() {
        let entries = vec![
            (0, "c".to_string()),
            (0, "a".to_string()),
            (0, "b".to_string()),
        ];
        let expected = entries
            .iter()
            .min_by_key(|(_, entry)| bincode::serialize(entry).expect("strings serialize"))
            .cloned();
        for side in [Side::Data, Side::Waiters] {
            let head = BagDiscipline
                .head(side, &ordered(&entries))
                .expect("strings serialize")
                .and_then(|index| entries.get(index).cloned());
            assert_eq!(head, expected);
            let mut reversed = entries.clone();
            reversed.reverse();
            let head = BagDiscipline
                .head(side, &ordered(&reversed))
                .expect("strings serialize")
                .and_then(|index| reversed.get(index).cloned());
            assert_eq!(head, expected, "the head ignores the list order");
        }
        assert_eq!(BagDiscipline.head::<String>(Side::Data, &[]), Ok(None));
    }

    #[test]
    fn space_bag_reaches_every_entry_and_orders_by_ord() {
        for side in [Side::Data, Side::Waiters] {
            for queued in [0, 1, usize::MAX] {
                assert!(BagDiscipline.reaches_head(side, queued));
            }
            assert_eq!(BagDiscipline.precedes(side, 0, 1), Ok(true));
            assert_eq!(BagDiscipline.precedes(side, 1, 0), Ok(false));
        }
    }

    #[test]
    fn space_bag_payloads_have_no_class() {
        assert_eq!(BagDiscipline.class_key(&[int(1)]), Ok(None));
    }

    #[tokio::test]
    async fn space_bag_agent_stores_with_ord_zero_and_bumps_no_counter() {
        let (space, env) = fixture().await;
        for value in [1, 1, 2] {
            assert_eq!(
                run(&env, SpaceOp::Produce(send(value))).await,
                SpaceOutcome::default()
            );
        }
        let data = namespace::gprivate(namespace::data_channel(SPACE, &key()));
        let mut stored: Vec<(i64, Vec<Par>)> = space
            .get_data(&data)
            .await
            .iter()
            .map(|datum| {
                let record = record::unwrap_datum(&datum.a).expect("a datum record");
                (record.ord, record.datum.pars)
            })
            .collect();
        stored.sort_by_key(|(_, pars)| format!("{pars:?}"));
        assert_eq!(stored, vec![
            (0, vec![int(1)]),
            (0, vec![int(1)]),
            (0, vec![int(2)])
        ]);
        let counter = namespace::gprivate(namespace::collection_counter(SPACE, &key()));
        assert!(space.get_data(&counter).await.is_empty());
        let outcome = run(&env, SpaceOp::Consume(receive("w"))).await;
        assert_eq!(outcome.fires.len(), 1);
        assert_eq!(space.get_data(&data).await.len(), 2);
    }
}
