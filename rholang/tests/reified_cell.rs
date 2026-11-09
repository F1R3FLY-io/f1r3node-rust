//! The cell discipline of the native spaces (P5 design v3 A.7, F15) over the in-memory agent
//! environment.
//!
//! A cell holds at most one record and at most one waiting continuation. A store or a park on a
//! full side fails with `CellOccupied`, and the failed operation applies nothing. Every operation
//! bumps the counter of the collection (decision 31). A fresh element and a re-entered element
//! both meet the other side of the cell (decision 43, section 43.8 of its design). A consume-side
//! peek removes the record and stores it again (decision 45, Rule E).
//!
//! The tests run the agent of the family `hashmap:cell` directly, as
//! `GenericSpace<HashMapOuter, CellDiscipline>`, over an in-memory RSpace with play and replay.
//! The family dispatch and the factory of the family belong to the ENGINE and FACTORY batches.
//!
//! The properties check the headlines of `CellDiscipline.v`:
//!
//! * `prop_cell_capacity_one` (F11): a reference model of the cell plans predicts each round of
//!   the agent. Each side holds one entry at most.
//! * `prop_cell_restore_never_occupied`: a twin of the cell checks the capacity of the record
//!   slot before each re-store. The twin behaves as the cell on every script.
//! * `prop_cell_reentry_as_fresh`: a re-entered persistent element behaves as a fresh element
//!   with its first-round order.

#[path = "support/mod.rs"]
mod support;

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use models::rhoapi::{BindPattern, ListParWithRandom, Par, TaggedContinuation};
use models::rust::space_factory_urn::{SpaceFactoryUrn, SpaceInner, SpaceOuter};
use models::rust::utils::new_gbool_par;
use proptest::collection::vec;
use proptest::prelude::*;
use proptest::sample::Index;
use proptest::test_runner::TestCaseError;
use rholang::rust::interpreter::matcher::r#match::Matcher;
use rholang::rust::interpreter::merging::rholang_merging_logic::RholangMergingLogic;
use rholang::rust::interpreter::reified::costs::ReissueCause;
use rholang::rust::interpreter::spaces::agent::{
    ConsumeOp, JoinOp, ProduceOp, QueryParams, SpaceAgent, SpaceCreation, SpaceEnv, SpaceOp,
    SpaceOutcome, SpaceReissue,
};
use rholang::rust::interpreter::spaces::disciplines::cell::CellDiscipline;
use rholang::rust::interpreter::spaces::errors::SpaceError;
use rholang::rust::interpreter::spaces::generic::{
    Cx, Discipline, GenericSpace, Ordered, OuterStore, Side,
};
use rholang::rust::interpreter::spaces::namespace::{self, Durability, NativeSpace, SID_LEN};
use rholang::rust::interpreter::spaces::outer::hashmap::HashMapOuter;
use rholang::rust::interpreter::spaces::types::qualifier::SpaceQualifier;
// Retired by decision 90 (C6, step D): the retired replay test was the last user of `Checkpoint`.
// The text before:
// use rspace_plus_plus::rspace::checkpoint::Checkpoint;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::hashing::stable_hash_provider::hash;
use rspace_plus_plus::rspace::history::history_repository::HistoryRepository;
use rspace_plus_plus::rspace::merger::event_log_index::EventLogIndex;
use rspace_plus_plus::rspace::merger::merging_logic::{
    conflict_reason, conflicts, NumberChannelsDiff,
};
use rspace_plus_plus::rspace::merger::state_change::StateChange;
use rspace_plus_plus::rspace::merger::state_change_merger::compute_trie_actions;
use rspace_plus_plus::rspace::rspace_interface::ISpace;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use rspace_plus_plus::rspace::trace::event::{Event, IOEvent, Produce};
use serde::Serialize;
// Changed by decision 92: the checks of the anchor of a failed operation.
// use support::agent::{
//     any, budget, config_of, counter, counter_channel, create, creation, data_channel, env_over,
//     exactly, fired, int, play, play_and_replay, receive_on, records_on, run, send_on,
//     space_counter_channel, string, waiters_on, ParRSpace,
// };
use support::agent::{
    any, budget, config_of, counter, counter_channel, create, creation, data_channel, env_over,
    exactly, fired, int, logs_anchors_only, play, receive_on, records_on, run, send_on,
    space_counter_channel, string, waiters_on, ParRSpace,
};
use support::observed::{anchor_calls_only, observed};
use support::props::{check, pure_config, tokio_runtime};
use support::session::{config_channel, data_produces};

// ─── The cell under test ─────────────────────────────────────────────────────────────────────

/// The agent of the family `hashmap:cell`.
type Cell = GenericSpace<HashMapOuter, CellDiscipline>;

/// The space of every test.
const SPACE: NativeSpace = NativeSpace {
    sid: [0xC5; SID_LEN],
    durability: Durability::Durable,
};

/// The labels of the bump of an operation on an open collection. The bump takes the counter
/// record and puts the next one.
const BUMP: [&str; 3] = ["consume K", "comm K", "produce K"];

fn cell() -> Cell { Cell::default() }

/// The family of the factory of a cell space.
fn cell_family() -> SpaceFactoryUrn {
    SpaceFactoryUrn {
        outer: SpaceOuter::HashMap,
        inner: SpaceInner::Cell,
    }
}

/// The creation of the cell space `SPACE`.
fn cell_creation() -> SpaceCreation {
    creation(
        SPACE,
        config_of(
            SPACE,
            HashMapOuter::NAME,
            CellDiscipline::NAME,
            SpaceQualifier::Default,
        ),
        cell_family(),
    )
}

/// The channel of the cell collection of most tests.
fn channel() -> Par { string("cell") }

/// The data channel `D(sid,k)` of the collection on `channel()`.
fn data() -> Par { data_channel(SPACE, &channel()) }

/// The counter channel `S(sid,k)` of the collection on `channel()`.
fn counter_of_cell() -> Par { counter_channel(SPACE, &channel()) }

fn send(value: i64) -> SpaceOp { SpaceOp::Produce(send_on(SPACE, &channel(), value, false)) }

fn send_forever(value: i64) -> SpaceOp { SpaceOp::Produce(send_on(SPACE, &channel(), value, true)) }

fn receive_op(name: &str, pattern: BindPattern) -> ConsumeOp {
    receive_on(SPACE, &channel(), name, pattern)
}

fn receive(name: &str, pattern: BindPattern) -> SpaceOp {
    SpaceOp::Consume(receive_op(name, pattern))
}

fn receive_forever(name: &str, pattern: BindPattern) -> SpaceOp {
    SpaceOp::Consume(ConsumeOp {
        persistent: true,
        ..receive_op(name, pattern)
    })
}

fn peek(name: &str, pattern: BindPattern) -> SpaceOp {
    SpaceOp::Consume(ConsumeOp {
        peek: true,
        ..receive_op(name, pattern)
    })
}

/// The `where` guard `true` or `false`.
fn guard_of(admit: bool) -> Par { new_gbool_par(admit, Vec::new(), false) }

/// A receive whose `where` guard admits every match or refuses every match.
fn guarded(name: &str, pattern: BindPattern, admit: bool) -> SpaceOp {
    SpaceOp::Consume(ConsumeOp {
        guard: Some(guard_of(admit)),
        ..receive_op(name, pattern)
    })
}

/// `op` as a re-issue with the first-round order `ord` (decision 43).
fn with_reissued(op: SpaceOp, ord: i64) -> SpaceOp {
    match op {
        SpaceOp::Produce(produce) => SpaceOp::Produce(ProduceOp {
            reissued_ord: Some(ord),
            ..produce
        }),
        SpaceOp::Consume(consume) => SpaceOp::Consume(ConsumeOp {
            reissued_ord: Some(ord),
            ..consume
        }),
        SpaceOp::Settle(settle) => SpaceOp::Settle(settle),
        SpaceOp::Join(join) => SpaceOp::Join(JoinOp {
            reissued_ords: Some(vec![ord; join.binds.len()]),
            ..join
        }),
    }
}

/// The first-round order that a re-issued operation carries: the order of its first bind for a
/// join.
fn reissued_ord_of(op: &SpaceOp) -> Option<i64> {
    match op {
        SpaceOp::Produce(produce) => produce.reissued_ord,
        SpaceOp::Consume(consume) => consume.reissued_ord,
        SpaceOp::Settle(_) => None,
        SpaceOp::Join(join) => join
            .reissued_ords
            .as_ref()
            .and_then(|ords| ords.first().copied()),
    }
}

/// A play space with the cell space created by `agent`, and the environment of its jobs.
struct Fixture {
    host: ParRSpace,
    env: SpaceEnv,
}

async fn fixture_with(agent: &dyn SpaceAgent) -> Fixture {
    let host = play().await;
    let env = env_over(host.clone(), &budget());
    create(agent, &env, cell_creation()).await;
    host.take_event_log().await;
    Fixture { host, env }
}

async fn fixture() -> Fixture { fixture_with(&cell()).await }

/// The records of the cell as `(ord, payload, persistent)`, by `ord`.
async fn records(host: &ParRSpace) -> Vec<(i64, Vec<Par>, bool)> { records_on(host, &data()).await }

/// The waiting continuations of the cell as `(ord, persistent, peek)`, by `ord`.
async fn waiters(host: &ParRSpace) -> Vec<(i64, bool, bool)> { waiters_on(host, &data()).await }

/// The value of the counter of the cell, or nothing before its first operation.
async fn cell_counter(host: &ParRSpace) -> Vec<i64> { counter(host, &counter_of_cell()).await }

/// The label of the channel with the hash `channel_hash`. `C` is the config channel, `S` the space
/// counter and `H` the handle. `K` and `D` are the counter and the data channel of the cell.
fn label(channel_hash: &Blake2b256Hash) -> &'static str {
    let named: [(&'static str, Par); 5] = [
        ("C", config_channel(SPACE)),
        ("S", space_counter_channel(SPACE)),
        ("K", counter_of_cell()),
        ("D", data()),
        ("H", namespace::gprivate(namespace::handle_channel(SPACE))),
    ];
    named
        .iter()
        .find(|(_, channel)| hash(channel) == *channel_hash)
        .map(|(name, _)| *name)
        .unwrap_or("?")
}

/// The events of a log as `kind channel` labels. A COMM with peeks is marked `peek`.
fn labels(log: &[Event]) -> Vec<String> {
    log.iter()
        .map(|event| match event {
            Event::IoEvent(IOEvent::Produce(produce)) => {
                format!("produce {}", label(&produce.channel_hash))
            }
            Event::IoEvent(IOEvent::Consume(consume)) => {
                let names: Vec<&str> = consume.channel_hashes.iter().map(label).collect();
                format!("consume {}", names.join(","))
            }
            Event::Comm(comm) => {
                let names: Vec<&str> = comm.consume.channel_hashes.iter().map(label).collect();
                match comm.peeks.is_empty() {
                    true => format!("comm {}", names.join(",")),
                    false => format!("comm {} peek", names.join(",")),
                }
            }
        })
        .collect()
}

/// The labels of an operation on an open collection: the bump, then `rest`.
fn bumped(rest: &[&str]) -> Vec<String> {
    BUMP.iter()
        .chain(rest)
        .map(|label| label.to_string())
        .collect()
}

/// The body name and the bound values of a fire of the waiter `name`.
fn delivery(name: &str, values: &[i64]) -> (String, Vec<Par>) {
    (
        name.to_string(),
        values.iter().map(|value| int(*value)).collect(),
    )
}

/// Runs `op` with the cell agent and requires a commit.
async fn commit(env: &SpaceEnv, op: SpaceOp) -> SpaceOutcome {
    run(&cell(), env, op)
        .await
        .expect("the cell operation commits")
}

// ─── Twin disciplines (white-box checks and negative controls) ───────────────────────────────

/// A twin of the cell that checks the capacity of the record slot (`Cx::check_capacity`) before
/// each re-store of Rule E. It names the family `cell`, so it runs on the spaces of the cell. It
/// behaves as the cell exactly when every re-store meets an empty record slot.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct CheckedRestore;

impl Discipline for CheckedRestore {
    const NAME: &'static str = CellDiscipline::NAME;
    const USES_COUNTER: bool = CellDiscipline::USES_COUNTER;
    const SEND_PARAMS: &'static [&'static str] = CellDiscipline::SEND_PARAMS;
    const CAPACITY: Option<usize> = CellDiscipline::CAPACITY;

    fn head<T: Serialize>(
        &self,
        side: Side,
        entries: &[Ordered<'_, T>],
    ) -> Result<Option<usize>, SpaceError> {
        CellDiscipline.head(side, entries)
    }

    fn reaches_head(&self, side: Side, queued: usize) -> bool {
        CellDiscipline.reaches_head(side, queued)
    }

    fn precedes(&self, side: Side, ord: i64, head: i64) -> Result<bool, SpaceError> {
        CellDiscipline.precedes(side, ord, head)
    }

    fn restore(
        &self,
        cx: &mut Cx<'_, '_>,
        record: ListParWithRandom,
    ) -> Result<Option<SpaceReissue>, SpaceError> {
        cx.check_capacity::<Self>(Side::Data)?;
        cx.restore(record)?;
        Ok(None)
    }
}

/// The negative control of [`CheckedRestore`]: its re-store counts the record that the peek fire
/// removed (the Rocq mutant `cd-restore-counts-the-removed-record`). So it fails with
/// `CellOccupied` at every re-store.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct CountingRestore;

impl Discipline for CountingRestore {
    const NAME: &'static str = CellDiscipline::NAME;
    const USES_COUNTER: bool = CellDiscipline::USES_COUNTER;
    const SEND_PARAMS: &'static [&'static str] = CellDiscipline::SEND_PARAMS;
    const CAPACITY: Option<usize> = CellDiscipline::CAPACITY;

    fn head<T: Serialize>(
        &self,
        side: Side,
        entries: &[Ordered<'_, T>],
    ) -> Result<Option<usize>, SpaceError> {
        CellDiscipline.head(side, entries)
    }

    fn reaches_head(&self, side: Side, queued: usize) -> bool {
        CellDiscipline.reaches_head(side, queued)
    }

    fn precedes(&self, side: Side, ord: i64, head: i64) -> Result<bool, SpaceError> {
        CellDiscipline.precedes(side, ord, head)
    }

    fn restore(
        &self,
        cx: &mut Cx<'_, '_>,
        record: ListParWithRandom,
    ) -> Result<Option<SpaceReissue>, SpaceError> {
        let held = cx.side_len(Side::Data)? + 1;
        match Self::CAPACITY {
            Some(capacity) if held >= capacity => Err(SpaceError::CellOccupied),
            Some(_) | None => {
                cx.restore(record)?;
                Ok(None)
            }
        }
    }
}

/// The negative control of the re-entry rule. A re-entered element of this cell waits behind an
/// older head, by the rule of the queue (the Rocq mutant `cd-reentry-queue-rule`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct QueueRuleCell;

impl Discipline for QueueRuleCell {
    const NAME: &'static str = CellDiscipline::NAME;
    const USES_COUNTER: bool = CellDiscipline::USES_COUNTER;
    const SEND_PARAMS: &'static [&'static str] = CellDiscipline::SEND_PARAMS;
    const CAPACITY: Option<usize> = CellDiscipline::CAPACITY;

    fn head<T: Serialize>(
        &self,
        side: Side,
        entries: &[Ordered<'_, T>],
    ) -> Result<Option<usize>, SpaceError> {
        CellDiscipline.head(side, entries)
    }

    fn reaches_head(&self, side: Side, queued: usize) -> bool {
        CellDiscipline.reaches_head(side, queued)
    }

    fn precedes(&self, _side: Side, ord: i64, head: i64) -> Result<bool, SpaceError> {
        Ok(ord < head)
    }
}

// ─── Deterministic tests ─────────────────────────────────────────────────────────────────────

/// A receive on an empty cell parks with the order of its bump, and it opens the counter.
#[tokio::test]
async fn space_disciplines_cell_empty_read_parks() {
    let Fixture { host, env } = fixture().await;
    let outcome = commit(&env, receive("w", any())).await;
    assert_eq!(outcome, SpaceOutcome::default());
    assert_eq!(labels(&host.take_event_log().await), vec![
        "consume S",
        "comm S",
        "produce S",
        "produce K",
        "consume D"
    ]);
    assert_eq!(waiters(&host).await, vec![(0, false, false)]);
    assert!(records(&host).await.is_empty());
    assert_eq!(cell_counter(&host).await, vec![1]);
    assert_eq!(counter(&host, &space_counter_channel(SPACE)).await, vec![1]);
}

// Changed by decision 92: a failed job commits its anchor only, so the doc below replaces this
// one.
// /// A second send to a full cell fails with `CellOccupied`. The failed operation logs no event,
// /// leaves the record and the counter unchanged, and makes no legacy charge (decision 45).
/// A second send to a full cell fails with `CellOccupied`. The failed operation commits its anchor
/// only and then fails (decision 92). So it logs only the events of its anchor, leaves the record
/// and the counter unchanged, and makes only the observer calls of its anchor (decision 90,
/// decision 69). It made no legacy charge (decision 45) until DR-134 deleted the legacy meter.
#[tokio::test]
async fn space_disciplines_cell_second_produce_occupied() {
    let host = play().await;
    let budget = budget();
    // Changed by decision 90 (decision 69): the host records its observer calls, because DR-134
    // deleted the legacy meter and its scopes.
    // let _legacy = budget.enter_legacy_scope();
    let recorder = observed(&host);
    let env = env_over(host.clone(), &budget);
    create(&cell(), &env, cell_creation()).await;
    assert_eq!(commit(&env, send(1)).await, SpaceOutcome::default());
    host.take_event_log().await;
    // let charges = budget.get_log();
    // assert!(!charges.is_empty(), "the store makes its legacy charges");
    let calls = recorder.calls();
    assert!(!calls.is_empty(), "the store makes its observer calls");
    for op in [send(2), send_forever(3)] {
        assert_eq!(run(&cell(), &env, op).await, Err(SpaceError::CellOccupied));
    }
    // Changed by decision 92: each failed operation commits its anchor only.
    // assert!(host.take_event_log().await.is_empty());
    assert!(
        logs_anchors_only(&host.take_event_log().await, SPACE, 2),
        "each failed operation logs its anchor only"
    );
    assert_eq!(records(&host).await, vec![(0, vec![int(1)], false)]);
    assert_eq!(
        cell_counter(&host).await,
        vec![1],
        "the failed bumps never commit"
    );
    // assert_eq!(
    //     budget.get_log(),
    //     charges,
    //     "a failed operation makes no legacy charge"
    // );
    // Changed by decision 92: each failed operation makes the observer calls of its anchor.
    let after = recorder.calls();
    assert_eq!(&after[..calls.len()], calls.as_slice());
    assert!(
        anchor_calls_only(
            &after[calls.len()..],
            &config_channel(SPACE),
            &space_counter_channel(SPACE),
            2
        ),
        "a failed operation makes the observer calls of its anchor only"
    );
}

/// A second waiter fails with `CellOccupied` in the same way, for a plain, a peek and a
/// persistent receive. A send that fires the waiter frees the slot.
#[tokio::test]
async fn space_disciplines_cell_second_park_occupied() {
    let Fixture { host, env } = fixture().await;
    commit(&env, receive("w1", exactly(9))).await;
    host.take_event_log().await;
    for op in [
        receive("w2", any()),
        peek("w3", any()),
        receive_forever("w4", any()),
    ] {
        assert_eq!(run(&cell(), &env, op).await, Err(SpaceError::CellOccupied));
    }
    // Changed by decision 92: each failed operation commits its anchor only.
    // assert!(host.take_event_log().await.is_empty());
    assert!(
        logs_anchors_only(&host.take_event_log().await, SPACE, 3),
        "each failed operation logs its anchor only"
    );
    assert_eq!(waiters(&host).await, vec![(0, false, false)]);
    assert_eq!(cell_counter(&host).await, vec![1]);
    assert_eq!(fired(&commit(&env, send(9)).await), vec![delivery(
        "w1",
        &[]
    )]);
    assert!(waiters(&host).await.is_empty());
    assert_eq!(
        commit(&env, receive("w2", any())).await,
        SpaceOutcome::default()
    );
    assert_eq!(waiters(&host).await, vec![(2, false, false)]);
}

/// A receive takes the record and empties the cell, so a later send stores again.
#[tokio::test]
async fn space_disciplines_cell_consume_empties_cell() {
    let Fixture { host, env } = fixture().await;
    commit(&env, send(5)).await;
    host.take_event_log().await;
    let outcome = commit(&env, receive("t", any())).await;
    assert_eq!(fired(&outcome), vec![delivery("t", &[5])]);
    assert_eq!(outcome.reissue, None);
    assert_eq!(
        labels(&host.take_event_log().await),
        bumped(&["consume D", "comm D"])
    );
    assert!(records(&host).await.is_empty());
    assert!(waiters(&host).await.is_empty());
    assert_eq!(commit(&env, send(6)).await, SpaceOutcome::default());
    assert_eq!(records(&host).await, vec![(2, vec![int(6)], false)]);
}

/// A consume-side peek removes the record and stores it again with its identity (Rule E). The
/// fire left the cell empty, so the re-store is never `CellOccupied`. The re-stored record then
/// occupies the cell.
#[tokio::test]
async fn space_disciplines_cell_peek_restores_record_not_occupied() {
    let Fixture { host, env } = fixture().await;
    commit(&env, send(5)).await;
    let stored = data_produces(&host.take_event_log().await, &data());
    assert_eq!(stored.len(), 1);
    let peeked = run(&cell(), &env, peek("p", any()))
        .await
        .expect("the peek is never CellOccupied");
    assert_eq!(fired(&peeked), vec![delivery("p", &[5])]);
    assert!(
        !peeked.fires[0].results[0].persistent,
        "the result keeps the flag of the host"
    );
    assert_eq!(peeked.reissue, None);
    let log = host.take_event_log().await;
    assert_eq!(
        labels(&log),
        bumped(&["consume D", "comm D peek", "produce D"]),
        "the peek fire removes the record, and the re-store produces it again"
    );
    assert_eq!(
        data_produces(&log, &data()),
        stored,
        "the re-store has the identity of the first produce"
    );
    assert_eq!(records(&host).await, vec![(0, vec![int(5)], false)]);
    assert!(waiters(&host).await.is_empty());
    assert_eq!(
        run(&cell(), &env, send(6)).await,
        Err(SpaceError::CellOccupied)
    );
    // Changed by decision 92: the failed operation commits its anchor only.
    // assert!(host.take_event_log().await.is_empty());
    assert!(
        logs_anchors_only(&host.take_event_log().await, SPACE, 1),
        "the failed operation logs its anchor only"
    );
    assert_eq!(fired(&commit(&env, receive("t", any())).await), vec![
        delivery("t", &[5])
    ]);
    assert!(records(&host).await.is_empty());
}

/// A twin of the cell that checks the capacity before each re-store gives the same peek, so the
/// re-store meets an empty record slot. A twin whose re-store counts the removed record fails,
/// and its failed peek changes nothing.
#[tokio::test]
async fn space_disciplines_cell_restore_meets_an_empty_slot() {
    let Fixture { host, env } = fixture().await;
    commit(&env, send(5)).await;
    host.take_event_log().await;
    let checked = GenericSpace::<HashMapOuter, CheckedRestore>::default();
    let peeked = run(&checked, &env, peek("p", any()))
        .await
        .expect("the checked re-store meets an empty slot");
    assert_eq!(fired(&peeked), vec![delivery("p", &[5])]);
    assert_eq!(
        labels(&host.take_event_log().await),
        bumped(&["consume D", "comm D peek", "produce D"])
    );
    assert_eq!(records(&host).await, vec![(0, vec![int(5)], false)]);
    assert_eq!(cell_counter(&host).await, vec![2]);
    let counting = GenericSpace::<HashMapOuter, CountingRestore>::default();
    assert_eq!(
        run(&counting, &env, peek("q", any())).await,
        Err(SpaceError::CellOccupied),
        "negative control: a re-store that counts the removed record fails"
    );
    // Changed by decision 92: the failed peek commits its anchor only.
    // assert!(host.take_event_log().await.is_empty());
    assert!(
        logs_anchors_only(&host.take_event_log().await, SPACE, 1),
        "the failed peek logs its anchor only"
    );
    assert_eq!(records(&host).await, vec![(0, vec![int(5)], false)]);
    assert_eq!(cell_counter(&host).await, vec![2]);
}

/// A persistent send occupies the cell for good. Receives and peeks take its value, the record
/// stays without a re-store, and every later send fails with `CellOccupied`.
#[tokio::test]
async fn space_disciplines_cell_persistent_send_occupies_forever() {
    let Fixture { host, env } = fixture().await;
    commit(&env, send_forever(4)).await;
    assert_eq!(records(&host).await, vec![(0, vec![int(4)], true)]);
    host.take_event_log().await;
    for name in ["a", "b"] {
        assert_eq!(fired(&commit(&env, receive(name, any())).await), vec![
            delivery(name, &[4])
        ]);
        assert_eq!(
            labels(&host.take_event_log().await),
            bumped(&["consume D", "comm D"])
        );
    }
    let peeked = commit(&env, peek("p", any())).await;
    assert_eq!(fired(&peeked), vec![delivery("p", &[4])]);
    assert!(peeked.fires[0].results[0].persistent);
    assert_eq!(
        labels(&host.take_event_log().await),
        bumped(&["consume D", "comm D peek"]),
        "the fire keeps a persistent record, so nothing is stored again"
    );
    for op in [send(5), send_forever(6)] {
        assert_eq!(run(&cell(), &env, op).await, Err(SpaceError::CellOccupied));
    }
    assert_eq!(records(&host).await, vec![(0, vec![int(4)], true)]);
    assert_eq!(cell_counter(&host).await, vec![4]);
}

/// A persistent receive takes the record and then parks. It stays and takes each later send,
/// and a second receive finds the waiter slot full.
#[tokio::test]
async fn space_disciplines_cell_persistent_receive_parks_after_fire() {
    let Fixture { host, env } = fixture().await;
    commit(&env, send(1)).await;
    host.take_event_log().await;
    let outcome = commit(&env, receive_forever("r", any())).await;
    assert_eq!(fired(&outcome), vec![delivery("r", &[1])]);
    assert_eq!(outcome.reissue, None);
    assert_eq!(
        labels(&host.take_event_log().await),
        bumped(&["consume D", "comm D", "consume D"]),
        "the fire, and then the park"
    );
    assert!(records(&host).await.is_empty());
    assert_eq!(waiters(&host).await, vec![(1, true, false)]);
    for value in [2, 3] {
        assert_eq!(fired(&commit(&env, send(value)).await), vec![delivery(
            "r",
            &[value]
        )]);
        assert_eq!(waiters(&host).await, vec![(1, true, false)]);
        assert!(records(&host).await.is_empty());
    }
    assert_eq!(
        run(&cell(), &env, receive("x", any())).await,
        Err(SpaceError::CellOccupied)
    );
}

/// The current guard rule (decision 37) on a cell. On the produce side a refusal of the waiter
/// stores the datum, so a second datum then finds the record slot full. On the consume side the
/// guard is checked once on the record, and a refusal parks.
#[tokio::test]
async fn space_disciplines_cell_guard_refusal_then_occupied() {
    let Fixture { host, env } = fixture().await;
    commit(&env, guarded("g", any(), false)).await;
    assert_eq!(commit(&env, send(1)).await, SpaceOutcome::default());
    assert_eq!(records(&host).await, vec![(1, vec![int(1)], false)]);
    assert_eq!(waiters(&host).await, vec![(0, false, false)]);
    assert_eq!(
        run(&cell(), &env, send(2)).await,
        Err(SpaceError::CellOccupied)
    );
    assert_eq!(
        run(&cell(), &env, guarded("h", any(), false)).await,
        Err(SpaceError::CellOccupied),
        "the guard refuses the record, and the waiter slot is full"
    );
    // With a free waiter slot, the refused receive parks next to the record.
    let Fixture { host, env } = fixture().await;
    commit(&env, send(1)).await;
    assert_eq!(
        commit(&env, guarded("h", any(), false)).await,
        SpaceOutcome::default()
    );
    assert_eq!(records(&host).await, vec![(0, vec![int(1)], false)]);
    assert_eq!(waiters(&host).await, vec![(1, false, false)]);
    // Negative control: a guard that admits the match fires.
    let Fixture { host, env } = fixture().await;
    commit(&env, send(1)).await;
    assert_eq!(fired(&commit(&env, guarded("h", any(), true)).await), vec![
        delivery("h", &[1])
    ]);
    assert!(records(&host).await.is_empty());
}

/// A send that fires a peek waiter keeps the datum in flight (decision 30). The cell holds no
/// other waiter, so the datum is then stored, or the whole operation fails with `CellOccupied`
/// when the record slot is full.
#[tokio::test]
async fn space_disciplines_cell_produce_side_peek_waiter_reoffers_then_stores_or_occupied() {
    let Fixture { host, env } = fixture().await;
    commit(&env, peek("p", any())).await;
    host.take_event_log().await;
    let outcome = commit(&env, send(3)).await;
    assert_eq!(fired(&outcome), vec![delivery("p", &[3])]);
    assert!(
        !outcome.fires[0].results[0].persistent,
        "the incoming datum of a peek fire is not retained"
    );
    let log = host.take_event_log().await;
    assert_eq!(
        labels(&log),
        bumped(&["produce D", "comm D peek", "produce D"])
    );
    let produces = data_produces(&log, &data());
    assert_eq!(
        produces[0], produces[1],
        "the store has the identity of the send"
    );
    assert_eq!(records(&host).await, vec![(1, vec![int(3)], false)]);
    assert!(waiters(&host).await.is_empty());
    let Fixture { host, env } = fixture().await;
    commit(&env, send(7)).await;
    commit(&env, peek("q", exactly(3))).await;
    host.take_event_log().await;
    assert_eq!(
        run(&cell(), &env, send(3)).await,
        Err(SpaceError::CellOccupied),
        "the peek waiter fires, and then the record slot is full"
    );
    // Changed by decision 92: the failed operation commits its anchor only, and the discard
    // removes the staged fire of the peek waiter.
    // assert!(host.take_event_log().await.is_empty());
    assert!(
        logs_anchors_only(&host.take_event_log().await, SPACE, 1),
        "the failed operation logs its anchor only"
    );
    assert_eq!(waiters(&host).await, vec![(1, false, true)]);
    assert_eq!(records(&host).await, vec![(0, vec![int(7)], false)]);
}

/// Every operation that commits bumps the counter once (decision 31), also a peek and a fire
/// that changes no other state. A failed operation bumps nothing.
#[tokio::test]
async fn space_disciplines_cell_every_operation_bumps_counter() {
    let Fixture { host, env } = fixture().await;
    commit(&env, send(1)).await;
    assert_eq!(cell_counter(&host).await, vec![1]);
    host.take_event_log().await;
    let operations = [
        peek("p", any()),
        receive("t", any()),
        receive("w", exactly(4)),
        send(4),
        send(2),
    ];
    for (index, op) in operations.into_iter().enumerate() {
        commit(&env, op).await;
        let log = labels(&host.take_event_log().await);
        assert_eq!(&log[..3], &BUMP.map(str::to_string), "operation {index}");
        let expected = i64::try_from(index).expect("a small index") + 2;
        assert_eq!(cell_counter(&host).await, vec![expected]);
    }
    assert_eq!(
        run(&cell(), &env, send(3)).await,
        Err(SpaceError::CellOccupied)
    );
    // Changed by decision 92: the failed operation commits its anchor only, which bumps no
    // counter.
    // assert!(host.take_event_log().await.is_empty());
    assert!(
        logs_anchors_only(&host.take_event_log().await, SPACE, 1),
        "the failed operation logs its anchor only"
    );
    assert_eq!(cell_counter(&host).await, vec![6]);
    assert_eq!(counter(&host, &space_counter_channel(SPACE)).await, vec![1]);
}

/// A cell takes no send params and no query. Both fail before the bump.
#[tokio::test]
async fn space_disciplines_cell_takes_no_params_or_query() {
    let Fixture { host, env } = fixture().await;
    let with_param = SpaceOp::Produce(ProduceOp {
        params: BTreeMap::from([("priority".to_string(), int(1))]),
        ..send_on(SPACE, &channel(), 1, false)
    });
    assert_eq!(
        run(&cell(), &env, with_param).await,
        Err(SpaceError::UnknownParam {
            key: "priority".to_string(),
            suggestion: None,
        })
    );
    let queried = SpaceOp::Consume(ConsumeOp {
        query: Some(QueryParams::default()),
        ..receive_op("w", any())
    });
    assert_eq!(
        run(&cell(), &env, queried).await,
        Err(SpaceError::QuerySpecUnsupported)
    );
    assert!(host.take_event_log().await.is_empty());
    assert!(cell_counter(&host).await.is_empty());
    assert_eq!(counter(&host, &space_counter_channel(SPACE)).await, vec![0]);
}

/// The record, the waiter and the counter of a cell live in the history. So a process that
/// restarts at a checkpoint continues the cell. Both slots stay full, and the next operation takes
/// the next order.
#[tokio::test]
async fn space_disciplines_cell_checkpoint_restart() {
    let mut stores = InMemoryStoreManager::new();
    let host = ParRSpace::create(
        stores.r_space_stores().await.expect("in-memory stores"),
        Arc::new(Box::new(Matcher)),
    )
    .expect("an in-memory space");
    let env = env_over(host.clone(), &budget());
    create(&cell(), &env, cell_creation()).await;
    commit(&env, send(7)).await;
    commit(&env, receive("w", exactly(9))).await;
    let root = host.create_checkpoint().await.expect("a checkpoint").root;
    let restarted = ParRSpace::create(
        stores
            .r_space_stores()
            .await
            .expect("the same in-memory stores"),
        Arc::new(Box::new(Matcher)),
    )
    .expect("a restarted space");
    restarted
        .reset(&root)
        .await
        .expect("the restart resets to the checkpoint");
    let env = env_over(restarted.clone(), &budget());
    assert_eq!(records(&restarted).await, vec![(0, vec![int(7)], false)]);
    assert_eq!(waiters(&restarted).await, vec![(1, false, false)]);
    assert_eq!(cell_counter(&restarted).await, vec![2]);
    // The waiter refuses 8, so the send must be stored. The record refuses the pattern 5, so the
    // receive must park. Both slots are full.
    for op in [send(8), receive("x", exactly(5))] {
        assert_eq!(run(&cell(), &env, op).await, Err(SpaceError::CellOccupied));
    }
    assert_eq!(
        fired(&commit(&env, send(9)).await),
        vec![delivery("w", &[])]
    );
    assert_eq!(cell_counter(&restarted).await, vec![3]);
    assert_eq!(fired(&commit(&env, receive("y", any())).await), vec![
        delivery("y", &[7])
    ]);
    assert_eq!(cell_counter(&restarted).await, vec![4]);
    assert!(records(&restarted).await.is_empty());
    assert!(waiters(&restarted).await.is_empty());
}

// Retired by decision 90 (C6, step D): `space_disciplines_cell_replay_equal` and its replay script
// ran through the rigged replay modes, which retire (decision 125). Native replay: the family
// matrix `space_native_replay_equal_{hashmap,pathmap,array,vector}`. Its negative controls:
// `space_native_forged_predecessor_fails_closed`,
// `native_space_replay_rejects_a_tampered_candidate`,
// `native_space_replay_rejects_an_outcome_that_does_not_fit`. The text before:
// ```
// /// The script of the replay test: every kind of cell operation, two failures, and a divergent
// /// pair.
// fn replay_script() -> Vec<SpaceOp> {
//     vec![
//         send(1),
//         send(2),
//         peek("p", any()),
//         receive("a", any()),
//         peek("q", any()),
//         send(3),
//         receive("b", exactly(9)),
//         receive("c", any()),
//         receive("d", exactly(5)),
//         send(9),
//         send_forever(4),
//         receive_forever("r", any()),
//     ]
// }
//
// /// Runs the replay script. An operation that re-issues runs one more round.
// async fn run_replay_script(env: &SpaceEnv) -> Vec<Result<SpaceOutcome, SpaceError>> {
//     let mut results = Vec::new();
//     for op in replay_script() {
//         let result = run(&cell(), env, op).await;
//         let reissue = match &result {
//             Ok(SpaceOutcome {
//                 reissue: Some(reissue),
//                 ..
//             }) => Some(reissue.op.clone()),
//             Ok(_) | Err(_) => None,
//         };
//         results.push(result);
//         if let Some(op) = reissue {
//             results.push(run(&cell(), env, op).await);
//         }
//     }
//     results
// }
//
// /// Replay stages the transactions of play: equal outcomes and failures, every COMM paired, and
// /// the root of play. A replay without the log diverges.
// #[tokio::test]
// async fn space_disciplines_cell_replay_equal() {
//     let (play, replay) = play_and_replay().await;
//     let play_env = env_over(play.clone(), &budget());
//     create(&cell(), &play_env, cell_creation()).await;
//     let start = play
//         .create_checkpoint()
//         .await
//         .expect("a checkpoint after the creation")
//         .root;
//     let played = run_replay_script(&play_env).await;
//     assert_eq!(
//         played
//             .iter()
//             .filter(|result| **result == Err(SpaceError::CellOccupied))
//             .count(),
//         2,
//         "the script fails twice"
//     );
//     assert_eq!(
//         played
//             .iter()
//             .filter_map(|result| result.as_ref().ok())
//             .map(|outcome| outcome.fires.len())
//             .sum::<usize>(),
//         7
//     );
//     assert!(played.last().is_some_and(|result| result
//         .as_ref()
//         .is_ok_and(|outcome| outcome.reissue.is_some())));
//     let Checkpoint { root, log } = play.create_checkpoint().await.expect("a checkpoint");
//     replay
//         .rig_and_reset(start.clone(), log)
//         .await
//         .expect("the replay rigs the play log");
//     let replay_env = env_over(replay.clone(), &budget());
//     let replayed = run_replay_script(&replay_env).await;
//     assert_eq!(replayed, played);
//     replay
//         .check_replay_data()
//         .await
//         .expect("every recorded COMM is paired");
//     assert_eq!(
//         replay.create_checkpoint().await.expect("a checkpoint").root,
//         root
//     );
//     replay
//         .rig_and_reset(start, Vec::new())
//         .await
//         .expect("the replay rigs an empty log");
//     match run(&cell(), &replay_env, send(1)).await {
//         Err(error @ SpaceError::ReplayDivergence(_)) => {
//             assert_eq!(error.code(), "ReplayDivergence")
//         }
//         other => panic!("negative control: a log without the COMMs diverges: {other:?}"),
//     }
// }
// ```

/// A re-issued persistent send keeps its first-round order and identity, and it re-enters as a
/// fresh element (decision 43, section 43.8). It fires the persistent waiter again, although an
/// older record holds the record slot. Under the rule of the queue it would wait behind that
/// record and fail with `CellOccupied`.
#[tokio::test]
async fn space_disciplines_cell_reissue_reenters_as_fresh() {
    let Fixture { host, env } = fixture().await;
    commit(&env, receive_forever("w", exactly(1))).await;
    commit(&env, send(2)).await;
    host.take_event_log().await;
    let first = commit(&env, send_forever(1)).await;
    assert_eq!(fired(&first), vec![delivery("w", &[])]);
    let expected = SpaceReissue {
        op: with_reissued(send_forever(1), 2),
        cause: ReissueCause::DivergentPair,
    };
    assert_eq!(first.reissue.as_ref(), Some(&expected));
    let round_one = data_produces(&host.take_event_log().await, &data());
    assert_eq!(round_one.len(), 1);
    let second = commit(&env, expected.op.clone()).await;
    assert_eq!(
        fired(&second),
        vec![delivery("w", &[])],
        "the re-entry meets the waiter, as a fresh send does"
    );
    assert_eq!(second.reissue.as_ref(), Some(&expected));
    assert_eq!(
        data_produces(&host.take_event_log().await, &data()),
        round_one,
        "every round stages the same record, so the same identity"
    );
    assert_eq!(
        cell_counter(&host).await,
        vec![4],
        "each round bumps the counter"
    );
    assert_eq!(records(&host).await, vec![(1, vec![int(2)], false)]);
    assert_eq!(waiters(&host).await, vec![(0, true, false)]);
    let fresh = commit(&env, send_forever(1)).await;
    assert_eq!(fired(&fresh), vec![delivery("w", &[])]);
    assert_eq!(
        fresh.reissue.map(|reissue| reissue.op),
        Some(with_reissued(send_forever(1), 4))
    );
    host.take_event_log().await;
    let queue_rule = GenericSpace::<HashMapOuter, QueueRuleCell>::default();
    assert_eq!(
        run(&queue_rule, &env, expected.op).await,
        Err(SpaceError::CellOccupied),
        "negative control: under the rule of the queue the re-entry waits behind the record"
    );
    // Changed by decision 92: the failed operation commits its anchor only.
    // assert!(host.take_event_log().await.is_empty());
    assert!(
        logs_anchors_only(&host.take_event_log().await, SPACE, 1),
        "the failed operation logs its anchor only"
    );
}

// ─── Merges over the agent environment ───────────────────────────────────────────────────────

/// The history repository of an in-memory space.
type ParHistory = Arc<
    Box<
        dyn HistoryRepository<Par, BindPattern, ListParWithRandom, TaggedContinuation>
            + Send
            + Sync
            + 'static,
    >,
>;

/// One branch from a pre-state: the event log of its operations and its post-state root.
struct Branch {
    log: Vec<Event>,
    root: Blake2b256Hash,
}

/// Runs `ops` from the root `pre` and checkpoints.
async fn branch(host: &ParRSpace, env: &SpaceEnv, pre: &Blake2b256Hash, ops: &[SpaceOp]) -> Branch {
    host.reset(pre).await.expect("reset to the pre-state");
    host.take_event_log().await;
    for op in ops {
        commit(env, op.clone()).await;
    }
    let log = host.take_event_log().await;
    let root = host
        .create_checkpoint()
        .await
        .expect("a checkpoint of the branch")
        .root;
    Branch { log, root }
}

/// The index of an event log as `block_index.rs` makes it: the pre-state closures read the
/// history at `pre`, and no channel is a number channel.
fn pre_state_index(history: &ParHistory, pre: &Blake2b256Hash, log: &[Event]) -> EventLogIndex {
    let reader = history
        .get_history_reader(pre)
        .expect("the pre-state reader");
    let in_pre_state = |produce: &Produce| {
        reader
            .get_data(&produce.channel_hash)
            .is_ok_and(|data| data.iter().any(|datum| datum.source == *produce))
    };
    let touches_join = |produce: &Produce| {
        reader
            .get_joins(&produce.channel_hash)
            .is_ok_and(|joins| joins.iter().any(|join| join.len() > 1))
    };
    EventLogIndex::new(
        log.to_vec(),
        in_pre_state,
        touches_join,
        NumberChannelsDiff::new(),
    )
}

/// `log` without the events on the channels `counters`: the log of a cell without M-1.
fn without_counters(log: &[Event], counters: &[Blake2b256Hash]) -> Vec<Event> {
    log.iter()
        .filter(|event| match event {
            Event::IoEvent(IOEvent::Produce(produce)) => !counters.contains(&produce.channel_hash),
            Event::IoEvent(IOEvent::Consume(consume)) => !consume
                .channel_hashes
                .iter()
                .any(|channel| counters.contains(channel)),
            Event::Comm(comm) => !comm
                .consume
                .channel_hashes
                .iter()
                .any(|channel| counters.contains(channel)),
        })
        .cloned()
        .collect()
}

/// The merged state of branches that the merger accepts, as `dag_merger.rs` computes it. The
/// state changes of the branches are combined, turned into trie actions over the base, and
/// applied.
fn merged_root(
    history: &ParHistory,
    base: &Blake2b256Hash,
    branches: &[(&Blake2b256Hash, &EventLogIndex)],
) -> Blake2b256Hash {
    let mut combined = StateChange::empty();
    let mut writers: HashMap<Blake2b256Hash, usize> = HashMap::new();
    for (post, index) in branches {
        let change = StateChange::new(
            history
                .get_history_reader_struct(base)
                .expect("the base reader"),
            history
                .get_history_reader_struct(post)
                .expect("the post-state reader"),
            index,
        )
        .expect("the state change of a branch");
        for entry in change.datums_changes.iter() {
            if !entry.value().added.is_empty() {
                *writers.entry(entry.key().clone()).or_insert(0) += 1;
            }
        }
        combined = combined.combine(change);
    }
    let multi_writer: BTreeSet<Blake2b256Hash> = writers
        .into_iter()
        .filter(|(_, count)| *count > 1)
        .map(|(channel, _)| channel)
        .collect();
    let reader = history.get_history_reader(base).expect("the base reader");
    let actions = compute_trie_actions(&combined, &reader, &BTreeMap::new(), |hash, changes, _| {
        if !changes.added.is_empty() {
            let base_data = reader.get_data(hash)?;
            if !base_data.is_empty() || multi_writer.contains(hash) {
                let base_binary = reader.get_data_proj_binary(hash)?;
                RholangMergingLogic::check_single_value_cell_not_overfilled(
                    hash,
                    &base_data,
                    &base_binary,
                    changes,
                )?;
            }
        }
        Ok(None)
    })
    .expect("the trie actions of the merge");
    history
        .reset(base)
        .expect("reset the history to the base")
        .do_checkpoint(actions)
        .root()
}

/// Two branches that each send to one empty cell race on its counter (merge check #1), so the
/// merger keeps one of them. The same logs without the counter events merge, and the merged
/// cell then holds two records. So the counter keeps the capacity of a cell across merges.
#[tokio::test]
async fn space_disciplines_cell_branch_writes_race_on_the_counter() {
    let Fixture { host, env } = fixture().await;
    commit(&env, send(1)).await;
    commit(&env, receive("o", any())).await;
    let pre = host.create_checkpoint().await.expect("the pre-state").root;
    let left = branch(&host, &env, &pre, &[send(2)]).await;
    let right = branch(&host, &env, &pre, &[send(3)]).await;
    let history = host.get_history_repository();
    let (left_index, right_index) = (
        pre_state_index(&history, &pre, &left.log),
        pre_state_index(&history, &pre, &right.log),
    );
    let found: BTreeSet<Blake2b256Hash> =
        conflicts(&left_index, &right_index).0.into_iter().collect();
    assert_eq!(found, BTreeSet::from([hash(&counter_of_cell())]));
    let reason = conflict_reason(&left_index, &right_index);
    assert!(
        reason
            .as_deref()
            .is_some_and(|reason| reason.starts_with("racesForSameIOEvent")),
        "merge check #1 finds the race: {reason:?}"
    );
    let counters = [
        hash(&counter_of_cell()),
        hash(&space_counter_channel(SPACE)),
    ];
    let (left_bare, right_bare) = (
        pre_state_index(&history, &pre, &without_counters(&left.log, &counters)),
        pre_state_index(&history, &pre, &without_counters(&right.log, &counters)),
    );
    assert!(
        conflicts(&left_bare, &right_bare).0.is_empty(),
        "negative control: without the counters the branches merge"
    );
    let merged = merged_root(&history, &pre, &[
        (&left.root, &left_bare),
        (&right.root, &right_bare),
    ]);
    let merged_records = history
        .get_history_reader(&merged)
        .expect("the merged reader")
        .get_data(&hash(&data()))
        .expect("the merged records");
    assert_eq!(
        merged_records.len(),
        2,
        "without the counters a merge puts two records into one cell"
    );
}

/// Branches on two different open cells do not conflict, and their merge is the state of each
/// serial order.
#[tokio::test]
async fn space_disciplines_cell_disjoint_branches_merge_into_the_serial_state() {
    let Fixture { host, env } = fixture().await;
    let other = string("other cell");
    commit(&env, send(1)).await;
    commit(&env, SpaceOp::Produce(send_on(SPACE, &other, 8, false))).await;
    commit(
        &env,
        SpaceOp::Consume(receive_on(SPACE, &other, "o", any())),
    )
    .await;
    let pre = host.create_checkpoint().await.expect("the pre-state").root;
    let first = [receive("t", any())];
    let second = [SpaceOp::Produce(send_on(SPACE, &other, 2, false))];
    let left = branch(&host, &env, &pre, &first).await;
    let right = branch(&host, &env, &pre, &second).await;
    let history = host.get_history_repository();
    let (left_index, right_index) = (
        pre_state_index(&history, &pre, &left.log),
        pre_state_index(&history, &pre, &right.log),
    );
    assert!(conflicts(&left_index, &right_index).0.is_empty());
    let merged = merged_root(&history, &pre, &[
        (&left.root, &left_index),
        (&right.root, &right_index),
    ]);
    let serial = branch(&host, &env, &pre, &[first.clone(), second.clone()].concat()).await;
    let reversed = branch(&host, &env, &pre, &[second, first].concat()).await;
    assert_eq!(merged, serial.root);
    assert_eq!(merged, reversed.root);
}

// ─── The reference model of the cell plans (CellDiscipline.v) ────────────────────────────────

/// The pattern of a generated receive: any value, or one literal value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pat {
    Any,
    Exactly(i64),
}

impl Pat {
    fn bind_pattern(self) -> BindPattern {
        match self {
            Pat::Any => any(),
            Pat::Exactly(value) => exactly(value),
        }
    }

    /// The spatial match of the pattern against `value`.
    fn matches(self, value: i64) -> bool {
        match self {
            Pat::Any => true,
            Pat::Exactly(literal) => literal == value,
        }
    }

    /// The values that the pattern binds from `value`.
    fn bound(self, value: i64) -> Vec<Par> {
        match self {
            Pat::Any => vec![int(value)],
            Pat::Exactly(_) => Vec::new(),
        }
    }
}

/// One generated operation on the cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CellOp {
    Send {
        value: i64,
        persistent: bool,
    },
    Receive {
        pat: Pat,
        persistent: bool,
        peek: bool,
        /// The `where` guard: none, or a guard that admits or refuses every match.
        admit: Option<bool>,
    },
}

impl CellOp {
    /// The space operation of a fresh round, with the continuation name `name`.
    fn space_op(self, name: &str) -> SpaceOp {
        match self {
            CellOp::Send { value, persistent } => {
                SpaceOp::Produce(send_on(SPACE, &channel(), value, persistent))
            }
            CellOp::Receive {
                pat,
                persistent,
                peek,
                admit,
            } => SpaceOp::Consume(ConsumeOp {
                persistent,
                peek,
                guard: admit.map(guard_of),
                ..receive_op(name, pat.bind_pattern())
            }),
        }
    }

    /// The side that the element of the operation joins.
    fn side(self) -> Side {
        match self {
            CellOp::Send { .. } => Side::Data,
            CellOp::Receive { .. } => Side::Waiters,
        }
    }
}

/// A record of the model.
#[derive(Clone, Debug, PartialEq, Eq)]
struct ModelRecord {
    ord: i64,
    value: i64,
    persistent: bool,
}

/// A waiting continuation of the model.
#[derive(Clone, Debug, PartialEq, Eq)]
struct ModelWaiter {
    ord: i64,
    name: String,
    pat: Pat,
    persistent: bool,
    peek: bool,
    admit: Option<bool>,
}

/// The paths of the cell plans that a generated run meets. Each property requires each of its
/// paths at least once over its cases.
#[derive(Clone, Copy, Debug)]
enum Met {
    DataOccupied,
    WaiterOccupied,
    PeekRestore,
    ProduceSidePeek,
    Divergent,
    Reentry,
    GuardRefusal,
}

impl Met {
    const ALL: [Met; 7] = [
        Met::DataOccupied,
        Met::WaiterOccupied,
        Met::PeekRestore,
        Met::ProduceSidePeek,
        Met::Divergent,
        Met::Reentry,
        Met::GuardRefusal,
    ];
}

/// The result of one round in the model.
#[derive(Clone, Debug, PartialEq)]
enum Predicted {
    /// `CellOccupied`: the round stages nothing.
    Occupied,
    /// The fires of the round, and its re-issue as `(cause, first-round order)`.
    Planned {
        fires: Vec<(String, Vec<Par>)>,
        reissue: Option<(ReissueCause, Option<i64>)>,
    },
}

/// The cell plans of `CellDiscipline.v` over plain values: the head driver with
/// `reaches_head = true`, `precedes = true`, one slot on each side, and Rule E.
#[derive(Clone, Debug, Default)]
struct Model {
    records: Vec<ModelRecord>,
    waiters: Vec<ModelWaiter>,
    counter: Option<i64>,
}

impl Model {
    /// One round of `op`, whose continuation has the name `name`. Without `reissued` the round is
    /// fresh. Otherwise it is the re-entry with the first-round order `reissued`. Both meet the
    /// other side. A failed round leaves the model unchanged.
    fn round(
        &self,
        name: &str,
        op: CellOp,
        reissued: Option<i64>,
        met: &mut Vec<Met>,
    ) -> (Predicted, Model) {
        let bump = self.counter.unwrap_or(0);
        let ord = reissued.unwrap_or(bump);
        if reissued.is_some() {
            met.push(Met::Reentry);
        }
        let mut next = self.clone();
        next.counter = Some(bump + 1);
        let predicted = match op {
            CellOp::Send { value, persistent } => next.offer(
                ModelRecord {
                    ord,
                    value,
                    persistent,
                },
                met,
            ),
            CellOp::Receive {
                pat,
                persistent,
                peek,
                admit,
            } => next.receive(
                ModelWaiter {
                    ord,
                    name: name.to_string(),
                    pat,
                    persistent,
                    peek,
                    admit,
                },
                met,
            ),
        };
        match predicted {
            Predicted::Occupied => (Predicted::Occupied, self.clone()),
            planned => (planned, next),
        }
    }

    /// The produce plan of the datum `datum`.
    fn offer(&mut self, datum: ModelRecord, met: &mut Vec<Met>) -> Predicted {
        let mut fires = Vec::new();
        loop {
            let head = self
                .waiters
                .iter()
                .enumerate()
                .min_by_key(|(_, waiter)| waiter.ord)
                .map(|(index, _)| index);
            let Some(index) = head else {
                return self.store(datum, fires, met);
            };
            let waiter = self.waiters[index].clone();
            let spatial = waiter.pat.matches(datum.value);
            if !spatial || waiter.admit == Some(false) {
                if spatial {
                    met.push(Met::GuardRefusal);
                }
                return self.store(datum, fires, met);
            }
            fires.push((waiter.name.clone(), waiter.pat.bound(datum.value)));
            if waiter.peek {
                met.push(Met::ProduceSidePeek);
            }
            if !waiter.persistent {
                self.waiters.remove(index);
            }
            match (!datum.persistent && !waiter.peek, waiter.persistent) {
                (true, _) => {
                    return Predicted::Planned {
                        fires,
                        reissue: None,
                    }
                }
                (false, true) => {
                    met.push(Met::Divergent);
                    return Predicted::Planned {
                        fires,
                        reissue: Some((
                            ReissueCause::DivergentPair,
                            datum.persistent.then_some(datum.ord),
                        )),
                    };
                }
                (false, false) => {}
            }
        }
    }

    /// The store of `datum`, with the capacity check of the record slot.
    fn store(
        &mut self,
        datum: ModelRecord,
        fires: Vec<(String, Vec<Par>)>,
        met: &mut Vec<Met>,
    ) -> Predicted {
        if !self.records.is_empty() {
            met.push(Met::DataOccupied);
            return Predicted::Occupied;
        }
        self.records.push(datum);
        Predicted::Planned {
            fires,
            reissue: None,
        }
    }

    /// The consume plan of the continuation `waiter`, with the re-store of Rule E.
    fn receive(&mut self, waiter: ModelWaiter, met: &mut Vec<Met>) -> Predicted {
        let mut fires = Vec::new();
        loop {
            let head = self
                .records
                .iter()
                .enumerate()
                .min_by_key(|(_, record)| record.ord)
                .map(|(index, _)| index);
            let Some(index) = head else {
                return self.park(waiter, fires, met);
            };
            let record = self.records[index].clone();
            let spatial = waiter.pat.matches(record.value);
            if !spatial || waiter.admit == Some(false) {
                if spatial {
                    met.push(Met::GuardRefusal);
                }
                return self.park(waiter, fires, met);
            }
            fires.push((waiter.name.clone(), waiter.pat.bound(record.value)));
            if !record.persistent {
                self.records.remove(index);
                if waiter.peek {
                    met.push(Met::PeekRestore);
                    self.records.push(record.clone());
                }
            }
            match (!waiter.persistent, record.persistent || waiter.peek) {
                (true, _) => {
                    return Predicted::Planned {
                        fires,
                        reissue: None,
                    }
                }
                (false, true) => {
                    met.push(Met::Divergent);
                    return Predicted::Planned {
                        fires,
                        reissue: Some((
                            ReissueCause::DivergentPair,
                            waiter.persistent.then_some(waiter.ord),
                        )),
                    };
                }
                (false, false) => {}
            }
        }
    }

    /// The park of `waiter`, with the capacity check of the waiter slot.
    fn park(
        &mut self,
        waiter: ModelWaiter,
        fires: Vec<(String, Vec<Par>)>,
        met: &mut Vec<Met>,
    ) -> Predicted {
        if !self.waiters.is_empty() {
            met.push(Met::WaiterOccupied);
            return Predicted::Occupied;
        }
        self.waiters.push(waiter);
        Predicted::Planned {
            fires,
            reissue: None,
        }
    }

    fn records_view(&self) -> Vec<(i64, Vec<Par>, bool)> {
        let mut view: Vec<_> = self
            .records
            .iter()
            .map(|record| (record.ord, vec![int(record.value)], record.persistent))
            .collect();
        view.sort_by_key(|(ord, ..)| *ord);
        view
    }

    fn waiters_view(&self) -> Vec<(i64, bool, bool)> {
        let mut view: Vec<_> = self
            .waiters
            .iter()
            .map(|waiter| (waiter.ord, waiter.persistent, waiter.peek))
            .collect();
        view.sort_by_key(|(ord, ..)| *ord);
        view
    }

    fn counter_view(&self) -> Vec<i64> { self.counter.into_iter().collect() }
}

/// Checks the result of one round against the prediction of the model. The re-issue of a round
/// is the operation of the round with its first-round order.
fn check_round(
    result: &Result<SpaceOutcome, SpaceError>,
    predicted: &Predicted,
    op: &SpaceOp,
) -> Result<(), TestCaseError> {
    match (result, predicted) {
        (Err(SpaceError::CellOccupied), Predicted::Occupied) => Ok(()),
        (Ok(outcome), Predicted::Planned { fires, reissue }) => {
            prop_assert_eq!(&fired(outcome), fires);
            prop_assert!(
                outcome.fires.len() <= 1,
                "a cell operation fires at most once"
            );
            let actual = outcome
                .reissue
                .as_ref()
                .map(|reissue| (reissue.cause, reissued_ord_of(&reissue.op)));
            prop_assert_eq!(&actual, reissue);
            if let Some(reissue) = &outcome.reissue {
                let ord = reissued_ord_of(&reissue.op).ok_or_else(|| {
                    TestCaseError::fail("a re-issue of a cell carries its first-round order")
                })?;
                prop_assert_eq!(&reissue.op, &with_reissued(op.clone(), ord));
            }
            Ok(())
        }
        (actual, expected) => Err(TestCaseError::fail(format!(
            "the agent gives {actual:?}, and the model predicts {expected:?}"
        ))),
    }
}

/// Checks that the collection in `host` is the collection of the model, with one entry at most
/// on each side (F15).
async fn check_view(host: &ParRSpace, model: &Model) -> Result<(), TestCaseError> {
    let (stored, parked) = (records(host).await, waiters(host).await);
    prop_assert!(stored.len() <= 1, "the cell holds {} records", stored.len());
    prop_assert!(parked.len() <= 1, "the cell holds {} waiters", parked.len());
    prop_assert_eq!(stored, model.records_view());
    prop_assert_eq!(parked, model.waiters_view());
    prop_assert_eq!(cell_counter(host).await, model.counter_view());
    Ok(())
}

/// One generated step: an operation and the most re-issue rounds that follow it.
#[derive(Clone, Copy, Debug)]
struct Step {
    op: CellOp,
    rounds: usize,
}

fn arb_value() -> BoxedStrategy<i64> { (0i64..3).boxed() }

fn arb_pat() -> BoxedStrategy<Pat> {
    prop_oneof![1 => Just(Pat::Any), 1 => arb_value().prop_map(Pat::Exactly)].boxed()
}

fn arb_admit() -> BoxedStrategy<Option<bool>> {
    prop_oneof![4 => Just(None), 1 => Just(Some(true)), 1 => Just(Some(false))].boxed()
}

/// An operation: a send, persistent with a probability of about one third, or a receive that is
/// plain, persistent or a peek. A peek is never persistent (`CONSUME_ARROWS`).
fn arb_op() -> BoxedStrategy<CellOp> {
    prop_oneof![
        1 => (arb_value(), 0u8..3).prop_map(|(value, arrow)| CellOp::Send {
            value,
            persistent: arrow == 0,
        }),
        1 => (arb_pat(), 0u8..4, arb_admit()).prop_map(|(pat, arrow, admit)| CellOp::Receive {
            pat,
            persistent: arrow == 1,
            peek: arrow == 2,
            admit,
        }),
    ]
    .boxed()
}

/// A persistent operation: the element of a re-issue (decision 43).
fn arb_persistent_op() -> BoxedStrategy<CellOp> {
    prop_oneof![
        arb_value().prop_map(|value| CellOp::Send {
            value,
            persistent: true,
        }),
        (arb_pat(), arb_admit()).prop_map(|(pat, admit)| CellOp::Receive {
            pat,
            persistent: true,
            peek: false,
            admit,
        }),
    ]
    .boxed()
}

fn arb_step() -> BoxedStrategy<Step> {
    (arb_op(), 0usize..3)
        .prop_map(|(op, rounds)| Step { op, rounds })
        .boxed()
}

/// The re-issue that a result asks for.
fn reissue_of(result: &Result<SpaceOutcome, SpaceError>) -> Option<SpaceOp> {
    match result {
        Ok(SpaceOutcome {
            reissue: Some(reissue),
            ..
        }) => Some(reissue.op.clone()),
        Ok(_) | Err(_) => None,
    }
}

/// Runs `ops` in order with the cell agent and drops their re-issues.
async fn run_prefix(env: &SpaceEnv, ops: &[CellOp]) {
    for (index, op) in ops.iter().enumerate() {
        let _ = run(&cell(), env, op.space_op(&format!("pre{index}"))).await;
    }
}

// ─── Properties ──────────────────────────────────────────────────────────────────────────────

static CAPACITY_COVERAGE: [AtomicUsize; 7] = [const { AtomicUsize::new(0) }; 7];

/// `cell_capacity_one` (F11, `CellDiscipline.v`). The scripts are generated, with re-issue
/// rounds. A reference model of the cell plans predicts each round of the agent: the fires, the
/// re-issue, the `CellOccupied` failures and the collection after the round. The model fails a
/// round exactly when its store or its park meets a full slot. Each side holds one entry at most
/// after every round.
#[test]
fn prop_cell_capacity_one() {
    check(pure_config(), vec(arb_step(), 1..14), |steps| {
        tokio_runtime().block_on(capacity_one_case(steps))
    });
    for (kind, count) in Met::ALL.iter().zip(CAPACITY_COVERAGE.iter()) {
        assert!(
            count.load(Ordering::Relaxed) > 0,
            "the generated scripts never met {kind:?}"
        );
    }
}

async fn capacity_one_case(steps: Vec<Step>) -> Result<(), TestCaseError> {
    let Fixture { host, env } = fixture().await;
    let mut model = Model::default();
    let mut met = Vec::new();
    for (index, step) in steps.iter().enumerate() {
        let name = format!("w{index}");
        let mut op = step.op.space_op(&name);
        let mut reissued = None;
        for _ in 0..=step.rounds {
            let (predicted, next) = model.round(&name, step.op, reissued, &mut met);
            let result = run(&cell(), &env, op.clone()).await;
            check_round(&result, &predicted, &op)?;
            model = next;
            check_view(&host, &model).await?;
            match reissue_of(&result) {
                Some(again) => {
                    reissued = reissued_ord_of(&again);
                    op = again;
                }
                None => break,
            }
        }
    }
    for kind in met {
        CAPACITY_COVERAGE[kind as usize].fetch_add(1, Ordering::Relaxed);
    }
    Ok(())
}

static RESTORE_COVERAGE: [AtomicUsize; 2] = [const { AtomicUsize::new(0) }; 2];

/// `cell_restore_never_occupied` (`CellDiscipline.v`). A twin of the cell checks the capacity of
/// the record slot before each re-store. On every generated script, the twin gives the results
/// and the events of the cell. So every re-store of Rule E meets an empty record slot. The
/// negative control is a twin whose re-store counts the removed record. It fails at the first
/// re-store of a script.
#[test]
fn prop_cell_restore_never_occupied() {
    check(pure_config(), vec(arb_step(), 1..14), |steps| {
        tokio_runtime().block_on(restore_never_occupied_case(steps))
    });
    for (kind, count) in ["re-store", "rejected counting re-store"]
        .iter()
        .zip(RESTORE_COVERAGE.iter())
    {
        assert!(
            count.load(Ordering::Relaxed) > 0,
            "the generated scripts never met a {kind}"
        );
    }
}

async fn restore_never_occupied_case(steps: Vec<Step>) -> Result<(), TestCaseError> {
    let checked = GenericSpace::<HashMapOuter, CheckedRestore>::default();
    let counting = GenericSpace::<HashMapOuter, CountingRestore>::default();
    let plain = fixture().await;
    let twin = fixture_with(&checked).await;
    let control = fixture_with(&counting).await;
    let mut control_live = true;
    for (index, step) in steps.iter().enumerate() {
        let mut op = step.op.space_op(&format!("w{index}"));
        for _ in 0..=step.rounds {
            let result = run(&cell(), &plain.env, op.clone()).await;
            let log = plain.host.take_event_log().await;
            prop_assert_eq!(&run(&checked, &twin.env, op.clone()).await, &result);
            prop_assert_eq!(&twin.host.take_event_log().await, &log);
            let restored = matches!(&op, SpaceOp::Consume(consume) if consume.peek)
                && labels(&log).iter().any(|label| label == "produce D");
            if restored {
                RESTORE_COVERAGE[0].fetch_add(1, Ordering::Relaxed);
            }
            if control_live {
                let controlled = run(&counting, &control.env, op.clone()).await;
                control.host.take_event_log().await;
                match restored {
                    true => {
                        prop_assert_eq!(controlled, Err(SpaceError::CellOccupied));
                        RESTORE_COVERAGE[1].fetch_add(1, Ordering::Relaxed);
                        control_live = false;
                    }
                    false => prop_assert_eq!(&controlled, &result),
                }
            }
            match reissue_of(&result) {
                Some(again) => op = again,
                None => break,
            }
        }
    }
    prop_assert_eq!(records(&twin.host).await, records(&plain.host).await);
    prop_assert_eq!(waiters(&twin.host).await, waiters(&plain.host).await);
    prop_assert_eq!(
        cell_counter(&twin.host).await,
        cell_counter(&plain.host).await
    );
    Ok(())
}

static REENTRY_COVERAGE: [AtomicUsize; 3] = [const { AtomicUsize::new(0) }; 3];

/// `cell_reentry_as_fresh` (decision 43, section 43.8, `CellDiscipline.v`). After a generated
/// prefix, a persistent element re-enters with a first-round order `o`. It behaves as the same
/// element sent fresh. The fires, the failures and the re-issue cause are equal, and the
/// collections are equal when the order of the element is mapped. The order `o` lies below the
/// counter, and no entry of the side of the element holds it (the checks of `operation_ord` and
/// `Cx::at_head`). The negative control, the rule of the queue, differs in some case.
#[test]
fn prop_cell_reentry_as_fresh() {
    check(
        pure_config(),
        (
            vec(arb_op(), 1..10),
            arb_persistent_op(),
            proptest::arbitrary::any::<Index>(),
        ),
        |(prefix, element, pick)| {
            tokio_runtime().block_on(reentry_as_fresh_case(prefix, element, pick))
        },
    );
    for (kind, count) in [
        "re-entry that fires",
        "re-entry that fails with CellOccupied",
        "case where the rule of the queue differs",
    ]
    .iter()
    .zip(REENTRY_COVERAGE.iter())
    {
        assert!(
            count.load(Ordering::Relaxed) > 0,
            "the generated cases never met a {kind}"
        );
    }
}

/// The entries of `side` of the cell in `host`, as `(ord, the rest of the entry)`.
async fn side_entries(host: &ParRSpace, side: Side) -> Vec<(i64, String)> {
    match side {
        Side::Data => records(host)
            .await
            .into_iter()
            .map(|(ord, payload, persistent)| (ord, format!("{payload:?} {persistent}")))
            .collect(),
        Side::Waiters => waiters(host)
            .await
            .into_iter()
            .map(|(ord, persistent, peek)| (ord, format!("{persistent} {peek}")))
            .collect(),
    }
}

/// The other side of a cell.
fn other_side(side: Side) -> Side {
    match side {
        Side::Data => Side::Waiters,
        Side::Waiters => Side::Data,
    }
}

async fn reentry_as_fresh_case(
    prefix: Vec<CellOp>,
    element: CellOp,
    pick: Index,
) -> Result<(), TestCaseError> {
    let reentered = fixture().await;
    let fresh = fixture().await;
    let control = fixture().await;
    for fixture in [&reentered, &fresh, &control] {
        run_prefix(&fixture.env, &prefix).await;
    }
    let bump = match cell_counter(&fresh.host).await.as_slice() {
        [value] => *value,
        other => return Err(TestCaseError::fail(format!("the counter holds {other:?}"))),
    };
    let side = element.side();
    let held: BTreeSet<i64> = side_entries(&fresh.host, side)
        .await
        .into_iter()
        .map(|(ord, _)| ord)
        .collect();
    let orders: Vec<i64> = (0..bump).filter(|ord| !held.contains(ord)).collect();
    prop_assume!(!orders.is_empty());
    let first_round = orders[pick.index(orders.len())];
    let fresh_op = element.space_op("x");
    let reentry_op = with_reissued(fresh_op.clone(), first_round);
    let again = run(&cell(), &reentered.env, reentry_op.clone()).await;
    let new = run(&cell(), &fresh.env, fresh_op).await;
    let queue_rule = GenericSpace::<HashMapOuter, QueueRuleCell>::default();
    let controlled = run(&queue_rule, &control.env, reentry_op).await;
    match (&again, &new) {
        (Err(left), Err(right)) => {
            prop_assert_eq!(left, right);
            REENTRY_COVERAGE[1].fetch_add(1, Ordering::Relaxed);
        }
        (Ok(left), Ok(right)) => {
            prop_assert_eq!(fired(left), fired(right));
            prop_assert_eq!(
                left.reissue.as_ref().map(|reissue| reissue.cause),
                right.reissue.as_ref().map(|reissue| reissue.cause)
            );
            prop_assert!(left
                .reissue
                .as_ref()
                .is_none_or(|reissue| reissued_ord_of(&reissue.op) == Some(first_round)));
            prop_assert!(right
                .reissue
                .as_ref()
                .is_none_or(|reissue| reissued_ord_of(&reissue.op) == Some(bump)));
            if !left.fires.is_empty() {
                REENTRY_COVERAGE[0].fetch_add(1, Ordering::Relaxed);
            }
        }
        (left, right) => {
            return Err(TestCaseError::fail(format!(
                "the re-entry gives {left:?}, and the fresh element gives {right:?}"
            )))
        }
    }
    let mapped: Vec<(i64, String)> = side_entries(&reentered.host, side)
        .await
        .into_iter()
        .map(|(ord, rest)| match ord == first_round {
            true => (bump, rest),
            false => (ord, rest),
        })
        .collect();
    let mut expected = side_entries(&fresh.host, side).await;
    let mut mapped = mapped;
    expected.sort();
    mapped.sort();
    prop_assert_eq!(mapped, expected);
    prop_assert_eq!(
        side_entries(&reentered.host, other_side(side)).await,
        side_entries(&fresh.host, other_side(side)).await
    );
    prop_assert_eq!(
        cell_counter(&reentered.host).await,
        cell_counter(&fresh.host).await
    );
    let differs = match (&controlled, &new) {
        (Ok(left), Ok(right)) => fired(left) != fired(right),
        (Err(_), Err(_)) => false,
        (Ok(_), Err(_)) | (Err(_), Ok(_)) => true,
    };
    if differs {
        REENTRY_COVERAGE[2].fetch_add(1, Ordering::Relaxed);
    }
    Ok(())
}
