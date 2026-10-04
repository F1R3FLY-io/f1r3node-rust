// Reified RSpaces storage substrate (`SpaceStoreHandle` / `SpaceTxn`):
//   A — host parity of every operation shape (22 tests)
//   B — checkpoints and history read-through (4 tests)
//   C — replay lookup parity (10 tests)
//   D — volatile sweep (7 tests)
//   E — reporting delegation (3 tests)
//   F — review regressions and rule coverage: canonical channel order,
//       store identity, trigger-key pairing, lock coverage, one commit per
//       parent order, nested begin, sweep bookkeeping, peeks in replay
//   G — round-2 rules: repeated channels in sorted groups (FR-1'), parent
//       tracking per root evaluation (FR-5'), checks that cost O(changed
//       entries) (FR-7'), and the sweep's nested-transaction guard (FR-9')
//   H — round-3 rule FR-10: a peek fire does not store the incoming datum,
//       the agent offers it again as the next produce of the transaction,
//       and only retained stored records are reported persistent
// The parity oracle runs each operation on a host `RSpace` and stages the
// same outcome on a twin through a transaction, then compares observer
// calls, events, store contents and state roots. After a peek fire the
// oracle follows HEAD's peek path (`head_peek_path` in support.rs): the
// space keeps the stored records, and HEAD's re-produce of the incoming
// datum is the agent's re-offer (FR-10). Every channel group is
// canonical: sorted (non-decreasing) by channel hash, which in the test
// universe is `b < d < c < a < v1 < v2`. A sorted group may repeat a
// channel.

#[path = "reified_space_store_handle/support.rs"]
mod support;

use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::panic::AssertUnwindSafe;
use std::sync::{Arc, Mutex};

use futures::FutureExt;
use rspace_plus_plus::rspace::checkpoint::Checkpoint;
use rspace_plus_plus::rspace::errors::RSpaceError;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::hashing::stable_hash_provider::{hash, hash_from_vec};
use rspace_plus_plus::rspace::internal::{Datum, WaitingContinuation};
use rspace_plus_plus::rspace::r#match::Match;
use rspace_plus_plus::rspace::operation_context::{self, CausalPath, OperationOrder};
use rspace_plus_plus::rspace::reporting_rspace::ReportingEvent;
use rspace_plus_plus::rspace::rspace::RSpace;
use rspace_plus_plus::rspace::rspace_interface::{
    ISpace, RSpaceOperationCompletion, RSpaceOperationSource, ReplayOperationDirective,
};
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use rspace_plus_plus::rspace::space_store_handle::{
    BeforeObserve, ConsumeOutcome, OpSource, ProduceOutcome, SpaceConsume, SpaceMode,
    SpaceOpReceipt, SpaceProduce, SpaceStoreHandle, SpaceTxnError, SweepReport, bind_candidate,
    incoming_candidate, sub_order,
};
use rspace_plus_plus::rspace::trace::event::{Consume, Event, IOEvent, Produce};
use serde::{Deserialize, Serialize};
use support::*;

struct Twin {
    host: Space,
    host_recorder: Arc<Recorder>,
    txn: Space,
    txn_recorder: Arc<Recorder>,
}

async fn twin() -> Twin {
    let host = play_space().await;
    let host_recorder = Recorder::new();
    observed(&host, &host_recorder);
    let txn = play_space().await;
    let txn_recorder = Recorder::new();
    observed(&txn, &txn_recorder);
    Twin {
        host,
        host_recorder,
        txn,
        txn_recorder,
    }
}

/// Host parity: equal observer calls, equal contents through the `ISpace`
/// getters, and equal event logs and roots at a checkpoint.
async fn assert_parity(twin: &Twin) -> (Checkpoint, Checkpoint) {
    assert_eq!(twin.txn_recorder.calls(), twin.host_recorder.calls(), "observer calls");
    let groups = groups_of(&[&twin.host, &twin.txn]).await;
    assert_eq!(
        channel_view(&twin.txn, &universe(), &groups).await,
        channel_view(&twin.host, &universe(), &groups).await,
        "space contents"
    );
    let host = checkpoint(&twin.host).await;
    let txn = checkpoint(&twin.txn).await;
    assert_eq!(event_bytes(&txn.log), event_bytes(&host.log), "event log");
    assert_eq!(txn.root, host.root, "state root");
    (host, txn)
}

fn kinds(log: &[Event]) -> Vec<&'static str> {
    log.iter()
        .map(|event| match event {
            Event::IoEvent(IOEvent::Produce(_)) => "produce",
            Event::IoEvent(IOEvent::Consume(_)) => "consume",
            Event::Comm(_) => "comm",
        })
        .collect()
}

fn call_kind(call: &Call) -> &'static str {
    match call {
        Call::BeforeObserve { .. } => "before",
        Call::Directive { .. } => "directive",
        Call::Start { .. } => "start",
        Call::Produce { .. } => "produce",
        Call::Consume { .. } => "consume",
        Call::Comm { .. } => "comm",
        Call::Finish { .. } => "finish",
    }
}

fn produce_source(channel: &str, data: &str, persist: bool) -> Produce {
    Produce::create(&s(channel), &s(data), persist)
}

fn consume_source(
    channels: &[&str],
    patterns: &[Pat],
    continuation: &str,
    persist: bool,
) -> Consume {
    Consume::create(&names(channels), &patterns.to_vec(), &s(continuation), persist)
}

fn datum(channel: &str, data: &str, persist: bool) -> Datum<String> {
    Datum {
        a: s(data),
        persist,
        source: produce_source(channel, data, persist),
    }
}

fn store_op(channel: &str, data: &str, persist: bool) -> SpaceProduce<String, Pat, String, String> {
    SpaceProduce {
        channel: s(channel),
        data: s(data),
        persist,
        outcome: ProduceOutcome::Store,
        before_observe: None,
    }
}

fn park_op(
    channels: &[&str],
    patterns: Vec<Pat>,
    continuation: &str,
    persist: bool,
    peeks: BTreeSet<i32>,
) -> SpaceConsume<String, Pat, String, String> {
    SpaceConsume {
        channels: names(channels),
        patterns,
        continuation: s(continuation),
        persist,
        peeks,
        outcome: ConsumeOutcome::Park,
        before_observe: None,
    }
}

fn fire_consume_op(
    channels: &[&str],
    patterns: Vec<Pat>,
    continuation: &str,
    candidates: Vec<Candidate>,
) -> SpaceConsume<String, Pat, String, String> {
    SpaceConsume {
        channels: names(channels),
        patterns,
        continuation: s(continuation),
        persist: false,
        peeks: BTreeSet::new(),
        outcome: ConsumeOutcome::Fire { data: candidates },
        before_observe: None,
    }
}

fn single(groups: Vec<Op>) -> Vec<Vec<Op>> { vec![groups] }

fn each(ops: Vec<Op>) -> Vec<Vec<Op>> { ops.into_iter().map(|op| vec![op]).collect() }

// ---------------------------------------------------------------- Group A

#[tokio::test]
async fn space_store_handle_parity_produce_store() {
    let twin = twin().await;
    let groups = single(vec![
        Op::produce("a", "x", false),
        Op::produce("a", "x", false),
        Op::produce("b", "y", true),
    ]);
    let (_, receipts) = run_guided(&twin.host, &twin.txn, &groups).await;
    let receipts = receipts_ok(receipts);
    assert_eq!(receipts[0].ops, vec![
        SpaceOpReceipt::Stored {
            produce: produce_source("a", "x", false)
        },
        SpaceOpReceipt::Stored {
            produce: produce_source("a", "x", false)
        },
        SpaceOpReceipt::Stored {
            produce: produce_source("b", "y", true)
        },
    ]);
    assert_eq!(logical_state(&twin.txn), logical_state(&twin.host));
    let (_, txn) = assert_parity(&twin).await;
    assert_eq!(kinds(&txn.log), vec!["produce", "produce", "produce"]);
}

#[tokio::test]
async fn space_store_handle_parity_consume_park() {
    let twin = twin().await;
    // One channel order per channel set: the hot store keys continuations
    // by caller order but history keys sort them, so `[a, b]` and `[b, a]`
    // in one hot store fail the host's own checkpoint. The canonical order
    // of {a, b} is [b, a].
    let groups = single(vec![
        Op::consume(&["b", "a"], vec![Pat::Any, Pat::Any], "k", false),
        Op::consume(&["a"], vec![Pat::Exact(s("x"))], "k2", true),
        Op::consume(&["c", "a"], vec![Pat::Any, Pat::Any], "k3", false),
    ]);
    let (_, receipts) = run_guided(&twin.host, &twin.txn, &groups).await;
    let receipts = receipts_ok(receipts);
    assert!(
        receipts[0]
            .ops
            .iter()
            .all(|op| matches!(op, SpaceOpReceipt::Parked { .. }))
    );
    assert_eq!(logical_state(&twin.txn), logical_state(&twin.host));
    assert_eq!(twin.txn.get_joins(s("a")).await, vec![
        names(&["c", "a"]),
        names(&["a"]),
        names(&["b", "a"])
    ]);
    let (_, txn) = assert_parity(&twin).await;
    assert_eq!(kinds(&txn.log), vec!["consume", "consume", "consume"]);
}

#[tokio::test]
async fn space_store_handle_parity_consume_fire() {
    let twin = twin().await;
    let groups =
        vec![vec![Op::produce("a", "x", false), Op::produce("b", "y", false)], vec![Op::consume(
            &["b", "a"],
            vec![Pat::Any, Pat::Any],
            "k",
            false,
        )]];
    let (_, receipts) = run_guided(&twin.host, &twin.txn, &groups).await;
    let receipts = receipts_ok(receipts);
    match &receipts[1].ops[0] {
        SpaceOpReceipt::Fired {
            comm,
            results,
            produce,
            continuation,
        } => {
            assert_eq!(produce, &None);
            assert_eq!(results.len(), 2);
            assert_eq!(continuation.channels, names(&["b", "a"]));
            assert_eq!(comm.produces.len(), 2);
        }
        other => panic!("expected a consume-side fire, got {:?}", other),
    }
    assert_eq!(logical_state(&twin.txn), logical_state(&twin.host));
    let (_, txn) = assert_parity(&twin).await;
    assert_eq!(kinds(&txn.log), vec!["produce", "produce", "consume", "comm"]);
}

#[tokio::test]
async fn space_store_handle_parity_produce_fire() {
    let twin = twin().await;
    let groups = vec![vec![Op::consume(&["a"], vec![Pat::Any], "k", false)], vec![Op::produce(
        "a", "x", false,
    )]];
    let (_, receipts) = run_guided(&twin.host, &twin.txn, &groups).await;
    let receipts = receipts_ok(receipts);
    match &receipts[1].ops[0] {
        SpaceOpReceipt::Fired {
            produce, results, ..
        } => {
            assert_eq!(produce, &Some(produce_source("a", "x", false)));
            assert_eq!(results[0].matched_datum, s("x"));
        }
        other => panic!("expected a produce-side fire, got {:?}", other),
    }
    assert!(twin.txn.get_joins(s("a")).await.is_empty());
    assert!(
        twin.txn
            .get_waiting_continuations(names(&["a"]))
            .await
            .is_empty()
    );
    assert_eq!(logical_state(&twin.txn), logical_state(&twin.host));
    let (_, txn) = assert_parity(&twin).await;
    assert_eq!(kinds(&txn.log), vec!["consume", "produce", "comm"]);
}

#[tokio::test]
async fn space_store_handle_parity_join_produce_fire() {
    let twin = twin().await;
    let groups = single(vec![
        Op::consume(&["b", "a"], vec![Pat::Any, Pat::Exact(s("x"))], "k", false),
        Op::produce("b", "z", false),
        Op::produce("a", "x", false),
    ]);
    let (plans, receipts) = run_guided(&twin.host, &twin.txn, &groups).await;
    let receipts = receipts_ok(receipts);
    match &plans[0][2] {
        Planned::Produce {
            outcome: ProduceOutcome::Fire { data, .. },
            ..
        } => {
            let indices: Vec<i32> = data.iter().map(|candidate| candidate.datum_index).collect();
            assert_eq!(indices, vec![0, -1]);
        }
        other => panic!("expected a planned join fire, got {:?}", other),
    }
    assert!(matches!(receipts[0].ops[2], SpaceOpReceipt::Fired { .. }));
    for channel in ["a", "b"] {
        assert!(twin.txn.get_joins(s(channel)).await.is_empty());
        assert!(twin.txn.get_data(&s(channel)).await.is_empty());
    }
    assert_eq!(logical_state(&twin.txn), logical_state(&twin.host));
    assert_parity(&twin).await;
}

#[tokio::test]
async fn space_store_handle_parity_persistent_continuation_keeps_join() {
    let twin = twin().await;
    let groups = vec![vec![Op::consume(&["a"], vec![Pat::Any], "k", true)], vec![
        Op::produce("a", "x", false),
        Op::produce("a", "y", false),
    ]];
    let (_, receipts) = run_guided(&twin.host, &twin.txn, &groups).await;
    let receipts = receipts_ok(receipts);
    assert!(
        receipts[1]
            .ops
            .iter()
            .all(|op| matches!(op, SpaceOpReceipt::Fired { .. }))
    );
    assert_eq!(twin.txn.get_joins(s("a")).await, vec![names(&["a"])]);
    assert_eq!(
        twin.txn
            .get_waiting_continuations(names(&["a"]))
            .await
            .len(),
        1
    );
    assert_eq!(logical_state(&twin.txn), logical_state(&twin.host));
    assert_parity(&twin).await;
}

#[tokio::test]
async fn space_store_handle_parity_persistent_datum_retained() {
    let twin = twin().await;
    let groups = vec![vec![Op::produce("a", "x", true)], vec![
        Op::consume(&["a"], vec![Pat::Any], "k", false),
        Op::consume(&["a"], vec![Pat::Any], "k2", false),
    ]];
    let (_, receipts) = run_guided(&twin.host, &twin.txn, &groups).await;
    let receipts = receipts_ok(receipts);
    for op in &receipts[1].ops {
        match op {
            SpaceOpReceipt::Fired { comm, results, .. } => {
                assert!(results[0].persistent);
                assert_eq!(comm.times_repeated.get(&produce_source("a", "x", true)), Some(&0));
            }
            other => panic!("expected a fire, got {:?}", other),
        }
    }
    assert_eq!(twin.txn.get_data(&s("a")).await, vec![datum("a", "x", true)]);
    assert_eq!(logical_state(&twin.txn), logical_state(&twin.host));
    assert_parity(&twin).await;
}

#[tokio::test]
async fn space_store_handle_parity_identical_produces_times_repeated() {
    let twin = twin().await;
    let consume = || Op::consume(&["a"], vec![Pat::Any], "k", false);
    let groups = vec![
        vec![Op::produce("a", "x", false), consume()],
        vec![Op::produce("a", "x", false), consume()],
        vec![Op::produce("a", "x", false), Op::produce("a", "x", false), consume(), consume()],
    ];
    run_guided(&twin.host, &twin.txn, &groups).await;
    let (_, txn) = assert_parity(&twin).await;
    let source = produce_source("a", "x", false);
    let times: Vec<i32> = comms_of(&txn.log)
        .iter()
        .map(|comm| {
            *comm
                .times_repeated
                .get(&source)
                .expect("the COMM counts its produce")
        })
        .collect();
    assert_eq!(times, vec![1, 2, 4, 4]);
}

#[tokio::test]
async fn space_store_handle_parity_counter_bump_shape() {
    let twin = twin().await;
    let groups = vec![vec![Op::produce("c", "0", false)], vec![
        Op::consume(&["c"], vec![Pat::Exact(s("0"))], "bump", false),
        Op::produce("c", "1", false),
    ]];
    let (_, receipts) = run_guided(&twin.host, &twin.txn, &groups).await;
    let receipts = receipts_ok(receipts);
    assert!(matches!(receipts[1].ops[0], SpaceOpReceipt::Fired { .. }));
    assert!(matches!(receipts[1].ops[1], SpaceOpReceipt::Stored { .. }));
    assert_eq!(twin.txn.get_data(&s("c")).await, vec![datum("c", "1", false)]);
    let (_, txn) = assert_parity(&twin).await;
    assert_eq!(kinds(&txn.log), vec!["produce", "consume", "comm", "produce"]);
}

#[tokio::test]
async fn space_store_handle_parity_continuation_dedup() {
    let twin = twin().await;
    let groups = single(vec![
        Op::consume(&["a"], vec![Pat::Any], "k", false),
        Op::consume(&["a"], vec![Pat::Any], "k", false),
    ]);
    run_guided(&twin.host, &twin.txn, &groups).await;
    assert_eq!(
        twin.txn
            .get_waiting_continuations(names(&["a"]))
            .await
            .len(),
        1
    );
    assert_eq!(logical_state(&twin.txn), logical_state(&twin.host));
    let (_, txn) = assert_parity(&twin).await;
    assert_eq!(kinds(&txn.log), vec!["consume", "consume"]);
}

#[tokio::test]
async fn space_store_handle_parity_observer_rejection_leaves_prefix() {
    let ops = vec![
        Op::consume(&["a"], vec![Pat::Any], "k1", false),
        Op::produce("a", "x", false),
        Op::consume(&["b"], vec![Pat::Any], "k2", false),
        Op::produce("b", "y", false),
        Op::produce("c", "z", false),
    ];
    let plans = plan_groups(&single(ops.clone())).await;
    let host = play_space().await;
    let host_recorder = Recorder::rejecting_comm(1);
    observed(&host, &host_recorder);
    let txn = play_space().await;
    let txn_recorder = Recorder::rejecting_comm(1);
    observed(&txn, &txn_recorder);

    for op in &ops[..3] {
        run_host(&host, op).await.expect("run the host prefix");
    }
    assert!(matches!(run_host(&host, &ops[3]).await, Err(RSpaceError::OutOfPhlogistons)));

    let handle = SpaceStoreHandle::new(txn.clone());
    let mut space_txn = handle.begin(&universe()).await.expect("begin");
    for plan in &plans[0] {
        plan.stage(&mut space_txn, None).expect("stage");
    }
    assert_eq!(space_txn.commit().await, Err(SpaceTxnError::Host(RSpaceError::OutOfPhlogistons)));
    assert!(txn.get_data(&s("c")).await.is_empty());
    let finishes: Vec<RSpaceOperationCompletion> = txn_recorder
        .calls()
        .into_iter()
        .filter_map(|call| match call {
            Call::Finish { completion, .. } => Some(completion),
            _ => None,
        })
        .collect();
    assert_eq!(finishes, vec![
        RSpaceOperationCompletion::Stored,
        RSpaceOperationCompletion::Matched,
        RSpaceOperationCompletion::Stored,
        RSpaceOperationCompletion::Rejected,
    ]);
    assert_parity(&Twin {
        host,
        host_recorder,
        txn,
        txn_recorder,
    })
    .await;
}

#[tokio::test]
async fn space_store_handle_parity_before_observe_precedes_start() {
    let groups =
        single(vec![Op::produce("a", "x", false), Op::consume(&["a"], vec![Pat::Any], "k", false)]);
    let plans = plan_groups(&groups).await;
    let host = play_space().await;
    let host_recorder = Recorder::new();
    observed(&host, &host_recorder);
    run_host_ops(&host, &flatten(&groups)).await;
    let txn = play_space().await;
    let txn_recorder = Recorder::new();
    observed(&txn, &txn_recorder);
    let handle = SpaceStoreHandle::new(txn.clone());

    let mut space_txn = handle.begin(&universe()).await.expect("begin");
    for plan in &plans[0] {
        plan.stage(&mut space_txn, Some(txn_recorder.before_observe(Ok(()))))
            .expect("stage");
    }
    space_txn.commit().await.expect("commit");
    let calls = txn_recorder.calls();
    assert_eq!(calls.iter().map(call_kind).collect::<Vec<_>>(), vec![
        "before", "start", "produce", "finish", "before", "start", "consume", "comm", "finish"
    ]);
    for pair in calls.windows(2) {
        if let [Call::BeforeObserve { source }, next] = pair {
            match next {
                Call::Start { source: start, .. } => assert_eq!(start, source),
                other => panic!("a callback must precede the start, got {:?}", other),
            }
        }
    }
    let without_callbacks: Vec<Call> = calls
        .into_iter()
        .filter(|call| !matches!(call, Call::BeforeObserve { .. }))
        .collect();
    assert_eq!(without_callbacks, host_recorder.calls());

    let before = txn_recorder.calls().len();
    let mut space_txn = handle.begin(&universe()).await.expect("begin");
    let mut kept = store_op("b", "w", false);
    kept.before_observe = Some(txn_recorder.before_observe(Ok(())));
    space_txn.produce(kept).expect("stage the kept produce");
    let mut refused = store_op("b", "y", false);
    refused.before_observe = Some(txn_recorder.before_observe(Err(RSpaceError::OutOfPhlogistons)));
    space_txn
        .produce(refused)
        .expect("stage the refused produce");
    assert_eq!(space_txn.commit().await, Err(SpaceTxnError::Host(RSpaceError::OutOfPhlogistons)));
    let refused_calls: Vec<&'static str> = txn_recorder.calls()[before..]
        .iter()
        .map(call_kind)
        .collect();
    assert_eq!(refused_calls, vec!["before", "start", "produce", "finish", "before"]);
    assert_eq!(txn.get_data(&s("b")).await, vec![datum("b", "w", false)]);
}

#[tokio::test]
async fn space_store_handle_parity_sub_orders_distinct_and_flatten_in_sequence() {
    let order = |segments: Vec<(u64, u64)>| OperationOrder {
        session: [9; 32],
        path: CausalPath::from(segments),
    };
    let parent = order(vec![(4, 1), (0, 0), (2, 0)]);
    let next = order(vec![(4, 1), (0, 0), (3, 0)]);
    let before_op = Op::produce("b", "first", false);
    let group = vec![Op::consume(&["a"], vec![Pat::Any], "k", false), Op::produce("a", "x", false)];
    let after_op = Op::produce("b", "last", false);
    let plans = plan_groups(&[vec![before_op.clone()], group.clone()]).await;
    let twin = twin().await;

    operation_context::scope(parent.clone(), async {
        run_host(&twin.host, &before_op)
            .await
            .expect("host op before");
        run_host_ops(&twin.host, &group).await;
    })
    .await;
    operation_context::scope(next.clone(), async {
        run_host(&twin.host, &after_op)
            .await
            .expect("host op after");
    })
    .await;

    let handle = SpaceStoreHandle::new(twin.txn.clone());
    operation_context::scope(parent.clone(), async {
        run_host(&twin.txn, &before_op)
            .await
            .expect("host op before");
        let mut space_txn = handle.begin(&universe()).await.expect("begin");
        for plan in &plans[1] {
            plan.stage(&mut space_txn, None).expect("stage");
        }
        space_txn.commit().await.expect("commit");
    })
    .await;
    operation_context::scope(next.clone(), async {
        run_host(&twin.txn, &after_op).await.expect("host op after");
    })
    .await;

    let orders: Vec<OperationOrder> = twin
        .txn_recorder
        .orders()
        .into_iter()
        .map(|order| order.expect("every operation runs under an order"))
        .collect();
    assert_eq!(orders, vec![
        parent.clone(),
        sub_order(&parent, 0),
        sub_order(&parent, 1),
        next.clone()
    ]);
    assert!(orders.windows(2).all(|pair| pair[0] < pair[1]));
    assert_parity(&twin).await;
}

// HEAD's peek path (reduce.rs:776-803, 973-1008): a peek fire removes the
// bound records in the host, and the reducer produces them again. The space
// keeps the stored record of the consume-side peek on `a` (the oracle puts
// it back into the host without events), and the produce-side peek on `b`
// does not store `y`: the agent offers `y` again as the next produce, which
// stores it with HEAD's events and observer calls (FR-10).
#[tokio::test]
async fn space_store_handle_peek_events_equal_host_and_retains() {
    let twin = twin().await;
    let groups = vec![
        vec![Op::produce("a", "x", false)],
        vec![Op::peek(&["a"], vec![Pat::Any], "k")],
        vec![Op::peek(&["b"], vec![Pat::Any], "k2")],
        vec![Op::produce("b", "y", false)],
    ];
    let (plans, receipts) = run_guided(&twin.host, &twin.txn, &groups).await;
    let receipts = receipts_ok(receipts);
    assert_eq!(plans[3].len(), 2);
    match (&receipts[1].ops[0], &receipts[3].ops[..]) {
        (
            SpaceOpReceipt::Fired {
                results: kept,
                comm: consume_comm,
                ..
            },
            [
                SpaceOpReceipt::Fired {
                    results: sent,
                    comm: produce_comm,
                    produce,
                    ..
                },
                SpaceOpReceipt::Stored { produce: reoffered },
            ],
        ) => {
            assert!(kept.iter().all(|result| result.persistent));
            assert_eq!(consume_comm.peeks, BTreeSet::from([0]));
            assert_eq!(produce_comm.peeks, BTreeSet::from([0]));
            assert_eq!(sent.len(), 1);
            assert!(!sent[0].persistent);
            assert_eq!(produce, &Some(produce_source("b", "y", false)));
            assert_eq!(reoffered, &produce_source("b", "y", false));
        }
        other => panic!("expected the peek fires and the re-offer, got {:?}", other),
    }
    let state = logical_state(&twin.txn);
    assert_eq!(state, logical_state(&twin.host));
    assert_eq!(
        state.data,
        BTreeMap::from([
            (s("a"), vec![bincode::serialize(&datum("a", "x", false)).expect("bytes")]),
            (s("b"), vec![bincode::serialize(&datum("b", "y", false)).expect("bytes")]),
        ])
    );
    assert!(state.continuations.is_empty());
    assert!(state.joins.is_empty());
    let (_, txn) = assert_parity(&twin).await;
    assert_eq!(kinds(&txn.log), vec![
        "produce", "consume", "comm", "consume", "produce", "comm", "produce"
    ]);
}

#[tokio::test]
async fn space_store_handle_rejects_mixed_arrow_join() {
    let space = play_space().await;
    let handle = SpaceStoreHandle::new(space.clone());
    let mut txn = handle.begin(&universe()).await.expect("begin");
    let partial = BTreeSet::from([0]);
    assert_eq!(
        txn.consume(park_op(&["b", "a"], vec![Pat::Any, Pat::Any], "k", false, partial.clone()))
            .err(),
        Some(SpaceTxnError::MixedArrowJoin {
            binds: 2,
            peeks: partial,
        })
    );
    assert_eq!(txn.commit().await.expect("commit").ops, vec![]);

    space
        .consume(names(&["b", "a"]), vec![Pat::Any, Pat::Any], s("k2"), false, BTreeSet::from([1]))
        .await
        .expect("park a host continuation with one peek");
    let waiting = space.get_waiting_continuations(names(&["b", "a"])).await;
    let mut txn = handle.begin(&universe()).await.expect("begin");
    let fire = SpaceProduce {
        channel: s("a"),
        data: s("x"),
        persist: false,
        outcome: ProduceOutcome::Fire {
            channels: names(&["b", "a"]),
            continuation: waiting[0].clone(),
            data: vec![incoming_candidate(
                s("a"),
                produce_source("a", "x", false),
                s("x"),
                false,
                s("x"),
            )],
        },
        before_observe: None,
    };
    assert_eq!(
        txn.produce(fire).err(),
        Some(SpaceTxnError::MixedArrowJoin {
            binds: 2,
            peeks: BTreeSet::from([1]),
        })
    );
    assert_eq!(txn.commit().await.expect("commit").ops, vec![]);
    assert_eq!(kinds(&space.take_event_log().await), vec!["consume"]);
}

#[tokio::test]
async fn space_store_handle_rejects_candidate_not_in_view() {
    let space = play_space().await;
    run_host_ops(&space, &[
        Op::produce("a", "x", false),
        Op::consume(&["b"], vec![Pat::Any], "k", false),
    ])
    .await;
    let handle = SpaceStoreHandle::new(space.clone());
    let mut txn = handle.begin(&universe()).await.expect("begin");
    let stored = txn.data(&s("a")).expect("read a")[0].clone();

    let absent = datum("a", "zzz", false);
    let wrong_record = fire_consume_op(&["a"], vec![Pat::Any], "k1", vec![bind_candidate(
        s("a"),
        &absent,
        s("zzz"),
        0,
    )]);
    assert_eq!(
        txn.consume(wrong_record).err(),
        Some(SpaceTxnError::NotInView("a stored candidate record"))
    );
    let wrong_index = fire_consume_op(&["a"], vec![Pat::Any], "k1", vec![bind_candidate(
        s("a"),
        &stored,
        s("x"),
        5,
    )]);
    assert_eq!(
        txn.consume(wrong_index).err(),
        Some(SpaceTxnError::NotInView("a stored candidate index"))
    );
    // A sorted group may repeat its channel (FR-1'), but each occurrence
    // binds its own record: a non-persistent record binds once per COMM.
    let twice = fire_consume_op(&["a", "a"], vec![Pat::Any, Pat::Any], "k1", vec![
        bind_candidate(s("a"), &stored, s("x"), 0),
        bind_candidate(s("a"), &stored, s("x"), 0),
    ]);
    assert_eq!(
        txn.consume(twice).err(),
        Some(SpaceTxnError::NotInView("a non-persistent record bound twice"))
    );
    let foreign = WaitingContinuation::create(
        &names(&["b"]),
        &vec![Pat::Any],
        &s("other"),
        false,
        BTreeSet::new(),
    );
    let wrong_continuation = SpaceProduce {
        channel: s("b"),
        data: s("y"),
        persist: false,
        outcome: ProduceOutcome::Fire {
            channels: names(&["b"]),
            continuation: foreign,
            data: vec![incoming_candidate(
                s("b"),
                produce_source("b", "y", false),
                s("y"),
                false,
                s("y"),
            )],
        },
        before_observe: None,
    };
    assert_eq!(
        txn.produce(wrong_continuation).err(),
        Some(SpaceTxnError::NotInView("the waiting continuation"))
    );

    let valid = fire_consume_op(&["a"], vec![Pat::Any], "k1", vec![bind_candidate(
        s("a"),
        &stored,
        s("x"),
        0,
    )]);
    txn.consume(valid)
        .expect("a valid fire after the rejections");
    let receipt = txn.commit().await.expect("commit");
    assert_eq!(receipt.ops.len(), 1);
    assert!(space.get_data(&s("a")).await.is_empty());
}

#[tokio::test]
async fn space_store_handle_rejects_installed_channel() {
    let space = play_space().await;
    space
        .install(names(&["sys"]), vec![Pat::Any], s("system"))
        .await
        .expect("install a system continuation");
    let handle = SpaceStoreHandle::new(space.clone());
    let mut lock_keys = universe();
    lock_keys.push(s("sys"));
    let mut txn = handle.begin(&lock_keys).await.expect("begin");
    assert_eq!(txn.data(&s("sys")).err(), Some(SpaceTxnError::InstalledChannel));
    assert_eq!(txn.continuations(&names(&["sys"])).err(), Some(SpaceTxnError::InstalledChannel));
    assert_eq!(txn.joins(&s("sys")).err(), Some(SpaceTxnError::InstalledChannel));
    assert_eq!(
        txn.produce(store_op("sys", "x", false)).err(),
        Some(SpaceTxnError::InstalledChannel)
    );
    assert_eq!(
        txn.consume(park_op(&["sys", "a"], vec![Pat::Any, Pat::Any], "k", false, BTreeSet::new()))
            .err(),
        Some(SpaceTxnError::InstalledChannel)
    );
    assert_eq!(txn.commit().await.expect("commit").ops, vec![]);
    assert_eq!(handle.sweep_volatile(&names(&["sys"])).await, Err(SpaceTxnError::InstalledChannel));
    assert_eq!(space.get_waiting_continuations(names(&["sys"])).await.len(), 1);
}

#[tokio::test]
async fn space_store_handle_abort_is_inert() {
    let setup = vec![Op::produce("a", "x", false), Op::consume(&["b"], vec![Pat::Any], "k", false)];
    let reference = play_space().await;
    run_host_ops(&reference, &setup).await;
    let space = play_space().await;
    let recorder = Recorder::new();
    observed(&space, &recorder);
    run_host_ops(&space, &setup).await;
    let handle = SpaceStoreHandle::new(space.clone());
    let source = produce_source("a", "x", false);
    let counter = |txn: &mut Txn| {
        txn.produce_counters(std::slice::from_ref(&source))
            .expect("read the counters")
    };
    let mut probe = handle.begin(&universe()).await.expect("begin");
    let counters_before = counter(&mut probe);
    drop(probe);
    let calls_before = recorder.calls();
    let state_before = logical_state(&space);

    {
        let mut txn = handle.begin(&universe()).await.expect("begin");
        txn.produce(store_op("a", "y", false))
            .expect("stage a store");
        let stored = txn.data(&s("a")).expect("read a")[1].clone();
        txn.consume(fire_consume_op(&["a"], vec![Pat::Any], "k2", vec![bind_candidate(
            s("a"),
            &stored,
            s("x"),
            1,
        )]))
        .expect("stage a fire");
        txn.consume(park_op(&["c"], vec![Pat::Any], "k3", false, BTreeSet::new()))
            .expect("stage a park");
        assert_eq!(counter(&mut txn).get(&source), Some(&1));
    }

    let mut probe = handle
        .begin(&universe())
        .now_or_never()
        .expect("the dropped transaction released its locks")
        .expect("begin");
    assert_eq!(counter(&mut probe), counters_before);
    drop(probe);
    assert_eq!(recorder.calls(), calls_before);
    assert_eq!(logical_state(&space), state_before);
    assert_eq!(kinds(&space.take_event_log().await), vec!["produce", "consume"]);
    assert_eq!(checkpoint(&space).await.root, checkpoint(&reference).await.root);
}

#[tokio::test]
async fn space_store_handle_view_reads_staged_effects() {
    let space = play_space().await;
    run_host_ops(&space, &[Op::produce("a", "x", false)]).await;
    let handle = SpaceStoreHandle::new(space.clone());
    let mut txn = handle.begin(&universe()).await.expect("begin");
    assert_eq!(txn.mode(), SpaceMode::Play);
    assert_eq!(txn.data(&s("a")).expect("read a"), &[datum("a", "x", false)]);

    txn.produce(store_op("a", "y", false))
        .expect("stage a store");
    assert_eq!(txn.data(&s("a")).expect("read a"), &[
        datum("a", "y", false),
        datum("a", "x", false)
    ]);
    let y = produce_source("a", "y", false);
    let x = produce_source("a", "x", false);
    assert_eq!(
        txn.produce_counters(&[x.clone(), y.clone()]),
        Ok(BTreeMap::from([(x.clone(), 1), (y.clone(), 1)]))
    );

    txn.consume(park_op(&["b"], vec![Pat::Any], "k", false, BTreeSet::new()))
        .expect("stage a park");
    let waiting = WaitingContinuation::create(
        &names(&["b"]),
        &vec![Pat::Any],
        &s("k"),
        false,
        BTreeSet::new(),
    );
    assert_eq!(txn.continuations(&names(&["b"])).expect("read b"), std::slice::from_ref(&waiting));
    assert_eq!(txn.joins(&s("b")).expect("read b joins"), &[names(&["b"])]);

    txn.consume(fire_consume_op(&["a"], vec![Pat::Any], "k2", vec![bind_candidate(
        s("a"),
        &datum("a", "x", false),
        s("x"),
        1,
    )]))
    .expect("stage a consume fire");
    assert_eq!(txn.data(&s("a")).expect("read a"), &[datum("a", "y", false)]);

    let fire = SpaceProduce {
        channel: s("b"),
        data: s("z"),
        persist: false,
        outcome: ProduceOutcome::Fire {
            channels: names(&["b"]),
            continuation: waiting,
            data: vec![incoming_candidate(
                s("b"),
                produce_source("b", "z", false),
                s("z"),
                false,
                s("z"),
            )],
        },
        before_observe: None,
    };
    let staged = txn.produce(fire).expect("stage a produce fire");
    assert!(staged.comm.is_some());
    assert!(
        txn.continuations(&names(&["b"]))
            .expect("read b")
            .is_empty()
    );
    assert!(txn.joins(&s("b")).expect("read b joins").is_empty());
    assert!(txn.data(&s("b")).expect("read b").is_empty());

    txn.commit().await.expect("commit");
    assert_eq!(space.get_data(&s("a")).await, vec![datum("a", "y", false)]);
    assert!(space.get_data(&s("b")).await.is_empty());
    assert!(space.get_joins(s("b")).await.is_empty());
    assert!(
        space
            .get_waiting_continuations(names(&["b"]))
            .await
            .is_empty()
    );
}

#[tokio::test]
async fn space_store_handle_commit_twice_is_finished() {
    let space = play_space().await;
    let handle = SpaceStoreHandle::new(space.clone());
    let mut txn = handle.begin(&universe()).await.expect("begin");
    txn.produce(store_op("a", "x", false)).expect("stage");
    assert_eq!(txn.commit().await.expect("commit").ops.len(), 1);
    assert_eq!(txn.commit().await, Err(SpaceTxnError::Finished));
    assert_eq!(txn.data(&s("a")).err(), Some(SpaceTxnError::Finished));
    assert_eq!(txn.joins(&s("a")).err(), Some(SpaceTxnError::Finished));
    assert_eq!(txn.produce(store_op("a", "y", false)).err(), Some(SpaceTxnError::Finished));
    assert_eq!(
        txn.consume(park_op(&["a"], vec![Pat::Any], "k", false, BTreeSet::new()))
            .err(),
        Some(SpaceTxnError::Finished)
    );
    assert_eq!(space.get_data(&s("a")).await, vec![datum("a", "x", false)]);
    assert_eq!(
        txn.produce_counters(&[produce_source("a", "x", false)]),
        Ok(BTreeMap::from([(produce_source("a", "x", false), 1)]))
    );
}

#[tokio::test]
async fn space_store_handle_store_swap_fails_closed() {
    let space = play_space().await;
    let recorder = Recorder::new();
    observed(&space, &recorder);
    let handle = SpaceStoreHandle::new(space.clone());
    let mut txn = handle.begin(&universe()).await.expect("begin");
    txn.produce(store_op("a", "x", false)).expect("stage");
    let soft = space.create_soft_checkpoint().await;
    space
        .revert_to_soft_checkpoint(soft)
        .await
        .expect("swap the hot store");
    assert_eq!(txn.commit().await, Err(SpaceTxnError::StoreSwapped));
    assert!(recorder.calls().is_empty());
    assert!(space.get_data(&s("a")).await.is_empty());
    assert!(space.take_event_log().await.is_empty());
}

// Transactions of different tasks on the same lock keys serialize; a second
// `begin` in the same task fails closed instead (see the nested-begin test).
#[tokio::test]
async fn space_store_handle_begin_serializes_same_lock_keys() {
    let space = play_space().await;
    let handle = SpaceStoreHandle::new(space.clone());
    let keys = names(&["a"]);

    let first = handle.begin(&keys).await.expect("begin the first");
    let waiting_handle = handle.clone();
    let waiting_keys = keys.clone();
    let second = tokio::spawn(async move {
        let mut txn = waiting_handle
            .begin(&waiting_keys)
            .await
            .expect("begin the second");
        txn.produce(store_op("a", "x", false)).expect("stage");
        txn.commit().await.map(|receipt| receipt.ops.len())
    });
    tokio::task::yield_now().await;
    assert!(!second.is_finished());
    drop(first);
    assert_eq!(second.await.expect("join the second"), Ok(1));

    let third_handle = handle.clone();
    let third_keys = keys.clone();
    let mut held = handle.begin(&keys).await.expect("begin again");
    let third =
        tokio::spawn(async move { third_handle.begin(&third_keys).await.map(|txn| txn.mode()) });
    tokio::task::yield_now().await;
    assert!(!third.is_finished());
    held.produce(store_op("a", "y", false)).expect("stage");
    held.commit().await.expect("commit releases the locks");
    assert_eq!(third.await.expect("join the third"), Ok(SpaceMode::Play));
    assert_eq!(
        handle.begin(&[]).await.err(),
        Some(SpaceTxnError::InvalidOperation("lock keys must not be empty"))
    );
    assert_eq!(space.get_data(&s("a")).await.len(), 2);
}

// ---------------------------------------------------------------- Group B

#[tokio::test]
async fn space_store_handle_checkpoint_root_and_log_equal_host() {
    let twin = twin().await;
    let first = vec![
        vec![
            Op::produce("a", "x", false),
            Op::consume(&["b", "a"], vec![Pat::Any, Pat::Any], "k", false),
        ],
        vec![Op::produce("b", "y", false)],
        vec![
            Op::consume(&["c"], vec![Pat::Any], "k2", true),
            Op::produce("c", "z", false),
            Op::produce("c", "w", true),
        ],
    ];
    run_guided(&twin.host, &twin.txn, &first).await;
    let (host, txn) = assert_parity(&twin).await;
    assert_eq!(kinds(&txn.log).len(), kinds(&host.log).len());
    assert_ne!(txn.root, checkpoint(&play_space().await).await.root);

    let second = vec![vec![Op::consume(&["a"], vec![Pat::Any], "k3", false)], vec![
        Op::produce("a", "q", false),
        Op::produce("c", "u", false),
    ]];
    run_guided(&twin.host, &twin.txn, &second).await;
    assert_parity(&twin).await;
}

#[tokio::test]
async fn space_store_handle_history_read_through_after_reset() {
    let twin = twin().await;
    let setup = vec![
        Op::produce("a", "x", false),
        Op::produce("a", "y", false),
        Op::consume(&["b"], vec![Pat::Any], "k", false),
        Op::consume(&["d", "c"], vec![Pat::Any, Pat::Any], "k2", false),
    ];
    let mut roots = Vec::with_capacity(2);
    for space in [&twin.host, &twin.txn] {
        run_host_ops(space, &setup).await;
        let persisted = checkpoint(space).await;
        space
            .reset(&persisted.root)
            .await
            .expect("reset to the root");
        roots.push(persisted.root);
    }
    assert_eq!(roots[0], roots[1]);

    let handle = SpaceStoreHandle::new(twin.txn.clone());
    let mut txn = handle.begin(&universe()).await.expect("begin");
    assert_eq!(txn.data(&s("a")).expect("read a").len(), 2);
    assert_eq!(txn.continuations(&names(&["b"])).expect("read b").len(), 1);
    assert_eq!(txn.joins(&s("c")).expect("read c joins"), &[names(&["d", "c"])]);
    drop(txn);

    let groups = vec![
        vec![Op::produce("b", "z", false), Op::consume(&["a"], vec![Pat::Any], "k3", false)],
        vec![Op::produce("c", "u", false), Op::produce("d", "w", false)],
    ];
    let (_, receipts) = run_guided(&twin.host, &twin.txn, &groups).await;
    let receipts = receipts_ok(receipts);
    assert!(matches!(receipts[0].ops[0], SpaceOpReceipt::Fired { .. }));
    assert!(matches!(receipts[0].ops[1], SpaceOpReceipt::Fired { .. }));
    assert!(matches!(receipts[1].ops[1], SpaceOpReceipt::Fired { .. }));
    assert_parity(&twin).await;
}

#[tokio::test]
async fn space_store_handle_soft_checkpoint_revert_discards_commit() {
    let twin = twin().await;
    let setup = vec![Op::produce("a", "x", false)];
    run_host_ops(&twin.host, &setup).await;
    run_host_ops(&twin.txn, &setup).await;
    let host_soft = twin.host.create_soft_checkpoint().await;
    let txn_soft = twin.txn.create_soft_checkpoint().await;
    let groups = single(vec![
        Op::produce("a", "y", false),
        Op::consume(&["a"], vec![Pat::Exact(s("x"))], "k", false),
    ]);
    let (_, receipts) = run_guided(&twin.host, &twin.txn, &groups).await;
    receipts_ok(receipts);
    assert_eq!(twin.txn.get_data(&s("a")).await, vec![datum("a", "y", false)]);

    twin.host
        .revert_to_soft_checkpoint(host_soft)
        .await
        .expect("revert the host");
    twin.txn
        .revert_to_soft_checkpoint(txn_soft)
        .await
        .expect("revert the transaction space");
    assert_eq!(twin.txn.get_data(&s("a")).await, vec![datum("a", "x", false)]);
    let handle = SpaceStoreHandle::new(twin.txn.clone());
    let mut probe = handle.begin(&universe()).await.expect("begin");
    let x = produce_source("a", "x", false);
    let y = produce_source("a", "y", false);
    assert_eq!(
        probe.produce_counters(&[x.clone(), y.clone()]),
        Ok(BTreeMap::from([(x, 1), (y, 0)]))
    );
    drop(probe);
    let (_, txn) = assert_parity(&twin).await;
    assert_eq!(kinds(&txn.log), vec!["produce"]);
}

#[tokio::test]
async fn space_store_handle_root_leaf_record_count_reads_root() {
    let space = play_space().await;
    let handle = SpaceStoreHandle::new(space.clone());
    assert_eq!(handle.root_leaf_record_count(&s("a")), Ok(0));
    run_host_ops(&space, &[
        Op::produce("a", "x", false),
        Op::produce("a", "y", false),
        Op::produce("a", "z", true),
    ])
    .await;
    assert_eq!(handle.root_leaf_record_count(&s("a")), Ok(0));
    let first = checkpoint(&space).await;
    assert_eq!(handle.root_leaf_record_count(&s("a")), Ok(3));
    let history = space.get_history_repository();
    let reader = history
        .get_history_reader(&first.root)
        .expect("read the root");
    assert_eq!(
        reader
            .get_data_proj_binary(&hash(&s("a")))
            .expect("read the leaf")
            .len(),
        3
    );

    let mut txn = handle.begin(&universe()).await.expect("begin");
    txn.produce(store_op("a", "w", false)).expect("stage");
    txn.commit().await.expect("commit");
    assert_eq!(handle.root_leaf_record_count(&s("a")), Ok(3));
    checkpoint(&space).await;
    assert_eq!(handle.root_leaf_record_count(&s("a")), Ok(4));
    assert_eq!(handle.root_leaf_record_count(&s("b")), Ok(0));
}

// ---------------------------------------------------------------- Group C

fn replay_groups() -> Vec<Vec<Op>> {
    vec![
        vec![Op::consume(&["a"], vec![Pat::Any], "k", false), Op::produce("b", "y", false)],
        vec![Op::produce("a", "x", false), Op::consume(&["b"], vec![Pat::Any], "k2", false)],
        vec![
            Op::consume(&["d", "c"], vec![Pat::Exact(s("w")), Pat::Any], "k3", true),
            Op::produce("d", "w", false),
            Op::produce("c", "u", false),
            Op::produce("d", "w", false),
        ],
        vec![Op::produce("a", "x", false), Op::produce("a", "x", false)],
        vec![Op::consume(&["a"], vec![Pat::Any], "k", false)],
    ]
}

#[tokio::test]
async fn space_store_handle_replay_pairs_play_log() {
    let groups = replay_groups();
    let plans = plan_groups(&groups).await;
    let (play, replay) = play_and_replay().await;
    let start = checkpoint(&play).await;
    let played_receipts =
        receipts_ok(run_plans(&SpaceStoreHandle::new(play.clone()), &plans).await);
    let played = checkpoint(&play).await;
    assert!(!comms_of(&played.log).is_empty());

    rig(&replay, &start.root, &played.log).await;
    let handle = SpaceStoreHandle::new(replay.clone());
    assert_eq!(handle.mode(), SpaceMode::Replay);
    let replayed_receipts = receipts_ok(run_plans(&handle, &plans).await);
    assert_eq!(replayed_receipts, played_receipts);
    assert!(replay_bindings(&replay).is_empty());
    let replayed = checkpoint(&replay).await;
    assert_eq!(replayed.root, played.root);
}

#[tokio::test]
async fn space_store_handle_replay_observer_calls_equal_host_replay() {
    let groups = replay_groups();
    let plans = plan_groups(&groups).await;
    let (play, host_replay) = play_and_replay().await;
    let start = checkpoint(&play).await;
    run_host_ops(&play, &flatten(&groups)).await;
    let played = checkpoint(&play).await;
    let txn_replay = replay_over(&play);

    let host_recorder = Recorder::new();
    observed(&host_replay, &host_recorder);
    rig(&host_replay, &start.root, &played.log).await;
    run_host_ops(&host_replay, &flatten(&groups)).await;
    let host_replayed = checkpoint(&host_replay).await;

    let txn_recorder = Recorder::new();
    observed(&txn_replay, &txn_recorder);
    rig(&txn_replay, &start.root, &played.log).await;
    receipts_ok(run_plans(&SpaceStoreHandle::new(txn_replay.clone()), &plans).await);
    let txn_replayed = checkpoint(&txn_replay).await;

    let calls = txn_recorder.calls();
    assert!(
        calls
            .iter()
            .any(|call| matches!(call, Call::Directive { .. }))
    );
    assert_eq!(calls, host_recorder.calls());
    assert_eq!(txn_replayed.root, host_replayed.root);
    assert_eq!(txn_replayed.root, played.root);
}

#[tokio::test]
async fn space_store_handle_host_replay_accepts_txn_play_log() {
    let groups = replay_groups();
    let plans = plan_groups(&groups).await;
    let (play, replay) = play_and_replay().await;
    let start = checkpoint(&play).await;
    receipts_ok(run_plans(&SpaceStoreHandle::new(play.clone()), &plans).await);
    let played = checkpoint(&play).await;
    rig(&replay, &start.root, &played.log).await;
    run_host_ops(&replay, &flatten(&groups)).await;
    replay
        .check_replay_data()
        .await
        .expect("the host replay pairs every COMM");
    assert_eq!(checkpoint(&replay).await.root, played.root);
}

#[tokio::test]
async fn space_store_handle_txn_replay_accepts_host_play_log() {
    let groups = replay_groups();
    let plans = plan_groups(&groups).await;
    let (play, replay) = play_and_replay().await;
    let start = checkpoint(&play).await;
    run_host_ops(&play, &flatten(&groups)).await;
    let played = checkpoint(&play).await;
    rig(&replay, &start.root, &played.log).await;
    receipts_ok(run_plans(&SpaceStoreHandle::new(replay.clone()), &plans).await);
    replay
        .check_replay_data()
        .await
        .expect("the transaction replay pairs every COMM");
    assert_eq!(checkpoint(&replay).await.root, played.root);
}

#[tokio::test]
async fn space_store_handle_replay_divergence_is_typed_and_inert() {
    let groups =
        each(vec![Op::consume(&["a"], vec![Pat::Any], "k", false), Op::produce("a", "x", false)]);
    let plans = plan_groups(&groups).await;
    let (play, replay) = play_and_replay().await;
    let start = checkpoint(&play).await;
    receipts_ok(run_plans(&SpaceStoreHandle::new(play.clone()), &plans).await);
    let played = checkpoint(&play).await;
    let recorder = Recorder::new();
    observed(&replay, &recorder);
    rig(&replay, &start.root, &played.log).await;

    let handle = SpaceStoreHandle::new(replay.clone());
    let mut txn = handle.begin(&universe()).await.expect("begin");
    plans[0][0].stage(&mut txn, None).expect("stage the park");
    let bindings = replay_bindings(&replay);
    let state = replay_state(&replay);
    let divergent = match &plans[1][0] {
        Planned::Produce {
            outcome:
                ProduceOutcome::Fire {
                    channels,
                    continuation,
                    ..
                },
            ..
        } => SpaceProduce {
            channel: s("a"),
            data: s("y"),
            persist: false,
            outcome: ProduceOutcome::Fire {
                channels: channels.clone(),
                continuation: continuation.clone(),
                data: vec![incoming_candidate(
                    s("a"),
                    produce_source("a", "y", false),
                    s("y"),
                    false,
                    s("y"),
                )],
            },
            before_observe: None,
        },
        other => panic!("expected a planned fire, got {:?}", other),
    };
    match txn.produce(divergent) {
        Err(SpaceTxnError::ReplayDivergence(comm)) => {
            assert_eq!(comm.produces, vec![produce_source("a", "y", false)]);
        }
        other => panic!("expected a replay divergence, got {:?}", other),
    }
    assert_eq!(replay_bindings(&replay), bindings);
    assert_eq!(replay_state(&replay), state);
    assert!(recorder.calls().is_empty());
    assert!(txn.data(&s("a")).expect("read a").is_empty());
    assert_eq!(txn.continuations(&names(&["a"])).expect("read a").len(), 1);
    let y = produce_source("a", "y", false);
    assert_eq!(txn.produce_counters(std::slice::from_ref(&y)), Ok(BTreeMap::from([(y, 0)])));

    plans[1][0]
        .stage(&mut txn, None)
        .expect("stage the recorded fire");
    txn.commit().await.expect("commit");
    assert_eq!(checkpoint(&replay).await.root, played.root);
}

#[tokio::test]
async fn space_store_handle_replay_unpaired_comm_fails_check_replay_data() {
    let groups =
        each(vec![Op::consume(&["a"], vec![Pat::Any], "k", false), Op::produce("a", "x", false)]);
    let plans = plan_groups(&groups).await;
    let (play, replay) = play_and_replay().await;
    let start = checkpoint(&play).await;
    receipts_ok(run_plans(&SpaceStoreHandle::new(play.clone()), &plans).await);
    let played = checkpoint(&play).await;
    rig(&replay, &start.root, &played.log).await;

    let handle = SpaceStoreHandle::new(replay.clone());
    let mut txn = handle.begin(&universe()).await.expect("begin");
    plans[0][0].stage(&mut txn, None).expect("stage the park");
    txn.produce(store_op("a", "x", false))
        .expect("a store with a recorded binding stages, as in the host");
    txn.commit().await.expect("commit");
    assert!(matches!(replay.check_replay_data().await, Err(RSpaceError::UnusedCommEvent { .. })));
    assert!(replay.create_checkpoint().await.is_err());
}

#[tokio::test]
async fn space_store_handle_replay_native_directive_fails_closed() {
    let (play, replay) = play_and_replay().await;
    let start = checkpoint(&play).await;
    let recorder = Recorder::with_directive(ReplayOperationDirective::Store);
    observed(&replay, &recorder);
    rig(&replay, &start.root, &[]).await;
    let handle = SpaceStoreHandle::new(replay.clone());

    let mut txn = handle.begin(&universe()).await.expect("begin");
    txn.produce(store_op("a", "x", false)).expect("stage");
    assert_eq!(txn.commit().await, Err(SpaceTxnError::NativeReplayDirectiveUnsupported));
    let mut txn = handle.begin(&universe()).await.expect("begin");
    txn.consume(park_op(&["a"], vec![Pat::Any], "k", false, BTreeSet::new()))
        .expect("stage");
    assert_eq!(txn.commit().await, Err(SpaceTxnError::NativeReplayDirectiveUnsupported));

    let kinds: Vec<&'static str> = recorder.calls().iter().map(call_kind).collect();
    assert_eq!(kinds, vec!["start", "directive", "finish", "start", "directive", "finish"]);
    assert!(recorder.calls().iter().all(|call| match call {
        Call::Finish { completion, .. } => *completion == RSpaceOperationCompletion::Rejected,
        _ => true,
    }));
    assert!(replay.get_data(&s("a")).await.is_empty());
    assert!(
        replay
            .get_waiting_continuations(names(&["a"]))
            .await
            .is_empty()
    );
}

#[tokio::test]
async fn space_store_handle_replay_comm_rejection_rolls_back_counter() {
    let groups =
        each(vec![Op::consume(&["a"], vec![Pat::Any], "k", false), Op::produce("a", "x", false)]);
    let plans = plan_groups(&groups).await;
    let (play, txn_replay) = play_and_replay().await;
    let start = checkpoint(&play).await;
    receipts_ok(run_plans(&SpaceStoreHandle::new(play.clone()), &plans).await);
    let played = checkpoint(&play).await;
    let host_replay = replay_over(&play);
    let txn_recorder = Recorder::rejecting_comm(0);
    observed(&txn_replay, &txn_recorder);
    let host_recorder = Recorder::rejecting_comm(0);
    observed(&host_replay, &host_recorder);
    rig(&txn_replay, &start.root, &played.log).await;
    rig(&host_replay, &start.root, &played.log).await;

    let ops = flatten(&groups);
    run_host(&host_replay, &ops[0]).await.expect("host park");
    assert!(matches!(run_host(&host_replay, &ops[1]).await, Err(RSpaceError::OutOfPhlogistons)));
    let handle = SpaceStoreHandle::new(txn_replay.clone());
    let mut txn = handle.begin(&universe()).await.expect("begin");
    plans[0][0].stage(&mut txn, None).expect("stage the park");
    plans[1][0].stage(&mut txn, None).expect("stage the fire");
    assert_eq!(txn.commit().await, Err(SpaceTxnError::Host(RSpaceError::OutOfPhlogistons)));

    let x = produce_source("a", "x", false);
    for replay in [&host_replay, &txn_replay] {
        let mut probe = SpaceStoreHandle::new(replay.clone())
            .begin(&universe())
            .await
            .expect("begin");
        assert_eq!(
            probe.produce_counters(std::slice::from_ref(&x)),
            Ok(BTreeMap::from([(x.clone(), 0)]))
        );
    }
    assert_eq!(txn_recorder.calls(), host_recorder.calls());
    assert_eq!(replay_state(&txn_replay), replay_state(&host_replay));
    assert_eq!(replay_bindings(&txn_replay), replay_bindings(&host_replay));
}

#[tokio::test]
async fn space_store_handle_replay_consult_admits_matches_host_rule() {
    let groups = vec![
        vec![
            Op::produce("a", "x", false),
            Op::produce("a", "x", false),
            Op::consume(&["a"], vec![Pat::Any], "k", false),
        ],
        vec![Op::produce("a", "x", false), Op::consume(&["a"], vec![Pat::Any], "k", false)],
        vec![Op::consume(&["b"], vec![Pat::Any], "k2", false), Op::produce("b", "y", false)],
    ];
    let plans = plan_groups(&groups).await;
    let (play, replay) = play_and_replay().await;
    let start = checkpoint(&play).await;
    receipts_ok(run_plans(&SpaceStoreHandle::new(play.clone()), &plans).await);
    let played = checkpoint(&play).await;
    let host_replay = replay_over(&play);
    rig(&host_replay, &start.root, &played.log).await;
    run_host_ops(&host_replay, &flatten(&groups)).await;
    host_replay
        .check_replay_data()
        .await
        .expect("the host rule accepts the log");

    let mut play_probe = SpaceStoreHandle::new(play.clone())
        .begin(&universe())
        .await
        .expect("begin");
    let k = consume_source(&["a"], &[Pat::Any], "k", false);
    assert_eq!(play_probe.recorded_comms(RSpaceOperationSource::Consume(&k)), None);
    drop(play_probe);

    rig(&replay, &start.root, &played.log).await;
    let handle = SpaceStoreHandle::new(replay.clone());
    let mut txn = handle.begin(&universe()).await.expect("begin");
    let recorded = txn
        .recorded_comms(RSpaceOperationSource::Consume(&k))
        .expect("replay lists recorded COMMs");
    let x = produce_source("a", "x", false);
    let times: Vec<i32> = recorded
        .iter()
        .map(|comm| *comm.times_repeated.get(&x).expect("counts x"))
        .collect();
    let mut sorted_times = times.clone();
    sorted_times.sort();
    assert_eq!(sorted_times, vec![2, 3]);
    let comm_with = |count: i32| {
        recorded
            .iter()
            .find(|comm| comm.times_repeated.get(&x) == Some(&count))
            .expect("a recorded COMM with that count")
            .clone()
    };
    let (two, three) = (comm_with(2), comm_with(3));
    let record = datum("a", "x", false);

    plans[0][0].stage(&mut txn, None).expect("stage");
    plans[0][1].stage(&mut txn, None).expect("stage");
    assert_eq!(txn.admits(&two, &record, false), Ok(true));
    assert_eq!(txn.admits(&three, &record, false), Ok(false));
    plans[0][2]
        .stage(&mut txn, None)
        .expect("stage the fire that pairs times 2");
    assert_eq!(txn.recorded_comms(RSpaceOperationSource::Consume(&k)), Some(vec![three.clone()]));
    plans[1][0].stage(&mut txn, None).expect("stage");
    assert_eq!(txn.admits(&three, &record, false), Ok(true));
    assert_eq!(txn.admits(&two, &record, false), Ok(false));
    plans[1][1]
        .stage(&mut txn, None)
        .expect("stage the fire that pairs times 3");

    plans[2][0].stage(&mut txn, None).expect("stage the park");
    let y = produce_source("b", "y", false);
    let produced = txn
        .recorded_comms(RSpaceOperationSource::Produce(&y))
        .expect("replay lists recorded COMMs");
    assert_eq!(produced.len(), 1);
    let incoming = datum("b", "y", false);
    assert_eq!(txn.admits(&produced[0], &incoming, true), Ok(true));
    assert_eq!(txn.admits(&produced[0], &incoming, false), Ok(false));
    plans[2][1]
        .stage(&mut txn, None)
        .expect("stage the produce fire");
    txn.commit().await.expect("commit");
    assert_eq!(checkpoint(&replay).await.root, played.root);
}

#[tokio::test]
async fn space_store_handle_replay_cross_channel_comm_pairs_once() {
    let (play, replay) = play_and_replay().await;
    let persistent_foreign = Datum {
        a: s("m"),
        persist: true,
        source: produce_source("d", "m", true),
    };
    let linear_foreign = Datum {
        a: s("n"),
        persist: false,
        source: produce_source("b", "n", false),
    };
    play.get_store()
        .put_datum(&s("c"), persistent_foreign.clone());
    play.get_store().put_datum(&s("a"), linear_foreign.clone());
    run_host_ops(&play, &[Op::consume(&["d", "a"], vec![Pat::Any, Pat::Any], "k2", false)]).await;
    let pre = checkpoint(&play).await;

    let consume_fire = || {
        fire_consume_op(&["c"], vec![Pat::Any], "k", vec![bind_candidate(
            s("c"),
            &persistent_foreign,
            s("m"),
            0,
        )])
    };
    let joined = WaitingContinuation::create(
        &names(&["d", "a"]),
        &vec![Pat::Any, Pat::Any],
        &s("k2"),
        false,
        BTreeSet::new(),
    );
    let produce_fire = || SpaceProduce {
        channel: s("d"),
        data: s("q"),
        persist: false,
        outcome: ProduceOutcome::Fire {
            channels: names(&["d", "a"]),
            continuation: joined.clone(),
            data: vec![
                incoming_candidate(s("d"), produce_source("d", "q", false), s("q"), false, s("q")),
                bind_candidate(s("a"), &linear_foreign, s("n"), 0),
            ],
        },
        before_observe: None,
    };

    let play_handle = SpaceStoreHandle::new(play.clone());
    let mut txn = play_handle.begin(&universe()).await.expect("begin");
    let consumed = txn.consume(consume_fire()).expect("stage the consume fire");
    let produced = txn.produce(produce_fire()).expect("stage the produce fire");
    txn.commit().await.expect("commit");
    let block = checkpoint(&play).await;
    let consumed = consumed.comm.expect("a COMM");
    let produced = produced.comm.expect("a COMM");
    assert_eq!(consumed.consume.channel_hashes, vec![hash(&s("c"))]);
    assert_eq!(consumed.produces[0].channel_hash, hash(&s("d")));
    assert!(
        produced
            .produces
            .iter()
            .any(|produce| produce.channel_hash == hash(&s("b")))
    );

    rig(&replay, &pre.root, &block.log).await;
    let bindings = replay_bindings(&replay);
    assert_eq!(bindings.len(), 2, "one key per COMM: the logged consume and the logged produce");
    let handle = SpaceStoreHandle::new(replay.clone());
    let mut txn = handle.begin(&universe()).await.expect("begin");
    txn.consume(consume_fire())
        .expect("pairs under the consume key");
    match txn.consume(consume_fire()) {
        Err(SpaceTxnError::ReplayDivergence(comm)) => assert_eq!(*comm, consumed),
        other => panic!("the COMM must pair once, got {:?}", other),
    }
    txn.produce(produce_fire())
        .expect("pairs under the produce key");
    txn.commit().await.expect("commit");
    assert!(replay_bindings(&replay).is_empty());
    assert_eq!(checkpoint(&replay).await.root, block.root);

    // A COMM whose produce list repeats a produce comes from a consume on a
    // group that repeats a channel and binds one persistent record twice.
    // `rig` binds it once per occurrence under that produce key, and
    // `recorded_comms` reports it once. A space stages the same sorted group
    // (FR-1') and pairs the COMM once under its consume key.
    let repeated = datum("b", "z", true);
    let twice = || {
        fire_consume_op(&["b", "b"], vec![Pat::Any, Pat::Any], "k4", vec![
            bind_candidate(s("b"), &repeated, s("z"), 0),
            bind_candidate(s("b"), &repeated, s("z"), 0),
        ])
    };
    let mut txn = play_handle.begin(&universe()).await.expect("begin");
    txn.produce(store_op("b", "z", true))
        .expect("stage a store");
    txn.commit().await.expect("commit");
    let consumed_twice = play
        .consume(names(&["b", "b"]), vec![Pat::Any, Pat::Any], s("k4"), false, BTreeSet::new())
        .await
        .expect("the host consumes on a repeated group");
    assert!(consumed_twice.is_some());
    let second = checkpoint(&play).await;
    let doubled = comms_of(&second.log)
        .pop()
        .expect("the repeated COMM is logged");
    assert_eq!(doubled.produces, vec![repeated.source.clone(), repeated.source.clone()]);

    rig(&replay, &block.root, &second.log).await;
    let mut txn = handle.begin(&universe()).await.expect("begin");
    txn.produce(store_op("b", "z", true))
        .expect("stage a store");
    assert_eq!(
        txn.recorded_comms(RSpaceOperationSource::Produce(&repeated.source)),
        Some(vec![doubled.clone()])
    );
    let paired = txn
        .consume(twice())
        .expect("the space pairs the repeated COMM");
    assert_eq!(paired.comm, Some(doubled.clone()));
    match txn.consume(twice()) {
        Err(SpaceTxnError::ReplayDivergence(comm)) => assert_eq!(*comm, doubled),
        other => panic!("the repeated COMM must pair once, got {:?}", other),
    }
    let receipt = txn.commit().await.expect("commit");
    match &receipt.ops[1] {
        SpaceOpReceipt::Fired { results, .. } => {
            assert_eq!(results.len(), 2);
            assert!(results.iter().all(|result| result.persistent));
        }
        other => panic!("expected the repeated fire, got {:?}", other),
    }
    assert!(replay_bindings(&replay).is_empty());
    assert_eq!(replay.get_data(&s("b")).await, vec![repeated.clone()]);
    assert_eq!(checkpoint(&replay).await.root, second.root);

    // Control: host replay pairs the same COMM through its own consume.
    let host_replay = replay_over(&play);
    rig(&host_replay, &block.root, &second.log).await;
    run_host_ops(&host_replay, &[Op::produce("b", "z", true)]).await;
    host_replay
        .consume(names(&["b", "b"]), vec![Pat::Any, Pat::Any], s("k4"), false, BTreeSet::new())
        .await
        .expect("the host replay pairs the repeated COMM");
    assert!(replay_bindings(&host_replay).is_empty());
    assert_eq!(checkpoint(&host_replay).await.root, second.root);
}

// ---------------------------------------------------------------- Group D

fn volatile_groups() -> Vec<Vec<Op>> {
    vec![
        vec![
            Op::produce("v1", "x", false),
            Op::consume(&["v1", "v2"], vec![Pat::Exact(s("never")), Pat::Any], "k", false),
            Op::produce("a", "durable", false),
        ],
        vec![
            Op::consume(&["v2"], vec![Pat::Exact(s("never"))], "k2", true),
            Op::produce("v2", "y", false),
            Op::consume(&["d"], vec![Pat::Any], "k3", false),
        ],
    ]
}

fn durable_ops() -> Vec<Op> {
    vec![Op::produce("a", "durable", false), Op::consume(&["d"], vec![Pat::Any], "k3", false)]
}

#[tokio::test]
async fn space_store_handle_sweep_removes_data_continuations_and_joins() {
    let space = play_space().await;
    let plans = plan_groups(&volatile_groups()).await;
    let handle = SpaceStoreHandle::new(space.clone());
    receipts_ok(run_plans(&handle, &plans).await);
    assert_eq!(space.get_joins(s("v2")).await.len(), 2);
    let report = handle
        .sweep_volatile(&names(&["v1", "v2"]))
        .await
        .expect("sweep");
    assert_eq!(report, SweepReport {
        channels: 2,
        data: 2,
        continuations: 2,
        joins: 3,
    });
    for channel in ["v1", "v2"] {
        assert!(space.get_data(&s(channel)).await.is_empty());
        assert!(space.get_joins(s(channel)).await.is_empty());
    }
    for group in [names(&["v1", "v2"]), names(&["v2"]), names(&["v1"])] {
        assert!(space.get_waiting_continuations(group).await.is_empty());
    }
    assert_eq!(space.get_data(&s("a")).await.len(), 1);
}

#[tokio::test]
async fn space_store_handle_sweep_is_root_neutral() {
    let reference = play_space().await;
    run_host_ops(&reference, &durable_ops()).await;
    let space = play_space().await;
    let plans = plan_groups(&volatile_groups()).await;
    let handle = SpaceStoreHandle::new(space.clone());
    receipts_ok(run_plans(&handle, &plans).await);
    handle
        .sweep_volatile(&names(&["v1", "v2"]))
        .await
        .expect("sweep");
    let swept = checkpoint(&space).await;
    let expected = checkpoint(&reference).await;
    assert_eq!(swept.root, expected.root);
    assert!(kinds(&swept.log).len() > kinds(&expected.log).len());
}

#[tokio::test]
async fn space_store_handle_sweep_leaves_durable_channels_untouched() {
    let space = play_space().await;
    let plans = plan_groups(&volatile_groups()).await;
    let handle = SpaceStoreHandle::new(space.clone());
    receipts_ok(run_plans(&handle, &plans).await);
    let durable = names(&["a", "b", "c", "d"]);
    let durable_groups = vec![names(&["d"]), names(&["a"])];
    let before = channel_view(&space, &durable, &durable_groups).await;
    handle
        .sweep_volatile(&names(&["v1", "v2"]))
        .await
        .expect("sweep");
    assert_eq!(channel_view(&space, &durable, &durable_groups).await, before);
    assert_eq!(space.get_joins(s("d")).await, vec![names(&["d"])]);
}

#[tokio::test]
async fn space_store_handle_sweep_rejects_mixed_join_group() {
    let space = play_space().await;
    let handle = SpaceStoreHandle::new(space.clone());
    let mut txn = handle.begin(&universe()).await.expect("begin");
    txn.consume(park_op(&["d", "v1"], vec![Pat::Any, Pat::Any], "k", false, BTreeSet::new()))
        .expect("stage a join across the boundary");
    txn.commit().await.expect("commit");
    let state = logical_state(&space);
    assert_eq!(
        handle.sweep_volatile(&names(&["v1"])).await,
        Err(SpaceTxnError::SweepCrossesDurable)
    );
    assert_eq!(logical_state(&space), state);

    run_host_ops(&space, &[Op::produce("v2", "persisted", false)]).await;
    checkpoint(&space).await;
    assert_eq!(
        handle.sweep_volatile(&names(&["v2"])).await,
        Err(SpaceTxnError::SweepTouchesPersisted)
    );
    assert_eq!(space.get_data(&s("v2")).await.len(), 1);
}

#[tokio::test]
async fn space_store_handle_sweep_play_and_replay_agree() {
    let plans = plan_groups(&volatile_groups()).await;
    let (play, replay) = play_and_replay().await;
    let start = checkpoint(&play).await;
    let play_handle = SpaceStoreHandle::new(play.clone());
    receipts_ok(run_plans(&play_handle, &plans).await);
    let played_report = play_handle
        .sweep_volatile(&names(&["v1", "v2"]))
        .await
        .expect("sweep in play");
    let played = checkpoint(&play).await;

    rig(&replay, &start.root, &played.log).await;
    let replay_handle = SpaceStoreHandle::new(replay.clone());
    receipts_ok(run_plans(&replay_handle, &plans).await);
    let replayed_report = replay_handle
        .sweep_volatile(&names(&["v1", "v2"]))
        .await
        .expect("sweep in replay");
    let replayed = checkpoint(&replay).await;
    assert_eq!(replayed_report, played_report);
    assert_eq!(replayed.root, played.root);
}

#[tokio::test]
async fn space_store_handle_sweep_emits_no_events_or_observations() {
    let space = play_space().await;
    let recorder = Recorder::new();
    observed(&space, &recorder);
    let plans = plan_groups(&volatile_groups()).await;
    let handle = SpaceStoreHandle::new(space.clone());
    receipts_ok(run_plans(&handle, &plans).await);
    let played = space.take_event_log().await;
    assert!(!played.is_empty());
    let calls = recorder.calls();
    let report = handle
        .sweep_volatile(&names(&["v1", "v2"]))
        .await
        .expect("sweep");
    assert!(report.data > 0);
    assert_eq!(recorder.calls(), calls);
    assert!(space.take_event_log().await.is_empty());
}

#[tokio::test]
async fn space_store_handle_remove_all_continuations_keeps_joins_control() {
    let space = play_space().await;
    run_host_ops(&space, &[Op::consume(&["v1", "v2"], vec![Pat::Any, Pat::Any], "k", false)]).await;
    space
        .remove_all_continuations(names(&["v1", "v2"]))
        .await
        .expect("remove the continuations");
    assert!(
        space
            .get_waiting_continuations(names(&["v1", "v2"]))
            .await
            .is_empty()
    );
    assert_eq!(space.get_joins(s("v1")).await, vec![names(&["v1", "v2"])]);
    assert_eq!(space.get_joins(s("v2")).await, vec![names(&["v1", "v2"])]);

    let report = SpaceStoreHandle::new(space.clone())
        .sweep_volatile(&names(&["v1", "v2"]))
        .await
        .expect("sweep the residue");
    assert_eq!(report.joins, 2);
    assert_eq!(report.continuations, 0);
    assert!(space.get_joins(s("v1")).await.is_empty());
    assert!(space.get_joins(s("v2")).await.is_empty());
}

// ---------------------------------------------------------------- Group E

#[tokio::test]
async fn space_store_handle_reporting_pairs_like_replay() {
    let groups = replay_groups();
    let plans = plan_groups(&groups).await;
    let play = play_space().await;
    let start = checkpoint(&play).await;
    receipts_ok(run_plans(&SpaceStoreHandle::new(play.clone()), &plans).await);
    let played = checkpoint(&play).await;
    let replay = replay_over(&play);
    let reporting = reporting_over(&play);
    rig(&replay, &start.root, &played.log).await;
    reporting
        .rig_and_reset(start.root.clone(), played.log.clone())
        .await
        .expect("rig the reporting space");

    let replay_receipts =
        receipts_ok(run_plans(&SpaceStoreHandle::new(replay.clone()), &plans).await);
    let reporting_handle = SpaceStoreHandle::new(reporting.clone());
    assert_eq!(reporting_handle.mode(), SpaceMode::Reporting);
    let reporting_receipts = receipts_ok(run_plans(&reporting_handle, &plans).await);
    assert_eq!(reporting_receipts, replay_receipts);
    let replayed = checkpoint(&replay).await;
    let reported = ISpace::create_checkpoint(&reporting)
        .await
        .expect("checkpoint the reporting space");
    assert_eq!(reported.root, replayed.root);
    assert_eq!(reported.root, played.root);
}

#[tokio::test]
async fn space_store_handle_reporting_records_txn_events_like_host() {
    let groups = replay_groups();
    let plans = plan_groups(&groups).await;
    let play = play_space().await;
    let start = checkpoint(&play).await;
    receipts_ok(run_plans(&SpaceStoreHandle::new(play.clone()), &plans).await);
    let played = checkpoint(&play).await;
    let by_host = reporting_over(&play);
    let by_txn = reporting_over(&play);
    for reporting in [&by_host, &by_txn] {
        reporting
            .rig_and_reset(start.root.clone(), played.log.clone())
            .await
            .expect("rig the reporting space");
    }
    run_host_ops(&by_host, &flatten(&groups)).await;
    receipts_ok(run_plans(&SpaceStoreHandle::new(by_txn.clone()), &plans).await);
    let host_report = by_host.get_report().expect("host report");
    let txn_report = by_txn.get_report().expect("transaction report");
    assert!(!txn_report.is_empty());
    assert!(
        txn_report
            .iter()
            .flat_map(|batch| batch.events.iter())
            .any(|event| matches!(event, ReportingEvent::ReportingComm(_)))
    );
    assert_eq!(txn_report, host_report);
}

#[tokio::test]
async fn space_store_handle_reporting_shares_inner_replay_state() {
    let groups = replay_groups();
    let plans = plan_groups(&groups).await;
    let play = play_space().await;
    let start = checkpoint(&play).await;
    receipts_ok(run_plans(&SpaceStoreHandle::new(play.clone()), &plans).await);
    let groups_view = groups_of(&[&play]).await;
    let expected = channel_view(&play, &universe(), &groups_view).await;
    let played = checkpoint(&play).await;
    let reporting = reporting_over(&play);
    reporting
        .rig_and_reset(start.root.clone(), played.log.clone())
        .await
        .expect("rig the reporting space");

    let handle = SpaceStoreHandle::new(reporting.clone());
    receipts_ok(run_plans(&handle, &plans).await);
    assert_eq!(channel_view(&reporting, &universe(), &groups_view).await, expected);
    reporting
        .check_replay_data()
        .await
        .expect("the transactions consumed the inner replay bindings");
    let mut txn = handle.begin(&universe()).await.expect("begin");
    assert_eq!(txn.mode(), SpaceMode::Reporting);
    assert_eq!(txn.data(&s("a")).expect("read a").to_vec(), reporting.get_data(&s("a")).await);
}

// ---------------------------------------------------------------- Group F

fn permuted(group: &[String]) -> Vec<String> {
    let mut permuted = group.to_vec();
    permuted.reverse();
    permuted
}

fn produced(channel: &str, data: &str) -> Event {
    Event::IoEvent(IOEvent::Produce(produce_source(channel, data, false)))
}

// Review F-ORDER-2: a transaction that only reads the permuted key of a
// stored group leaves the checkpoint intact. The read fails closed before
// any hot-store access, so no entry appears under the permuted key.
#[tokio::test]
async fn space_store_handle_permuted_read_keeps_checkpoint() {
    let group = canonical(&["a", "b"]);
    let reference = play_space().await;
    let space = play_space().await;
    for target in [&reference, &space] {
        target
            .consume(group.clone(), vec![Pat::Any, Pat::Any], s("k"), false, BTreeSet::new())
            .await
            .expect("park on the canonical group");
    }
    let handle = SpaceStoreHandle::new(space.clone());
    let keys = continuation_keys(&**space.get_store());
    {
        let mut txn = handle.begin(&universe()).await.expect("begin");
        assert_eq!(
            txn.continuations(&permuted(&group)).err(),
            Some(SpaceTxnError::NonCanonicalChannelOrder)
        );
        let unsorted_repeat = vec![group[0].clone(), group[1].clone(), group[0].clone()];
        assert_eq!(
            txn.continuations(&unsorted_repeat).err(),
            Some(SpaceTxnError::NonCanonicalChannelOrder)
        );
        assert_eq!(
            txn.continuations(&group)
                .expect("read the canonical group")
                .len(),
            1
        );
    }
    assert_eq!(continuation_keys(&**space.get_store()), keys);
    assert_eq!(logical_state(&space), logical_state(&reference));
    assert_eq!(checkpoint(&space).await.root, checkpoint(&reference).await.root);
}

// Review F-ORDER-3: an aborted transaction that stages operations on the
// permuted key of a stored group leaves the checkpoint intact.
#[tokio::test]
async fn space_store_handle_permuted_stage_keeps_checkpoint() {
    let group = canonical(&["a", "b"]);
    let reversed = permuted(&group);
    let reference = play_space().await;
    let space = play_space().await;
    for target in [&reference, &space] {
        run_host_ops(target, &[
            Op::produce("a", "x", false),
            Op::consume(&["b", "a"], vec![Pat::Any, Pat::Exact(s("never"))], "k", false),
        ])
        .await;
    }
    let handle = SpaceStoreHandle::new(space.clone());
    let keys = continuation_keys(&**space.get_store());
    {
        let mut txn = handle.begin(&universe()).await.expect("begin");
        let stored = txn.data(&s("a")).expect("read a")[0].clone();
        let non_canonical = Some(SpaceTxnError::NonCanonicalChannelOrder);
        assert_eq!(
            txn.consume(park_op(
                &["a", "b"],
                vec![Pat::Any, Pat::Any],
                "k2",
                false,
                BTreeSet::new()
            ))
            .err(),
            non_canonical
        );
        assert_eq!(
            txn.consume(fire_consume_op(&["a", "b"], vec![Pat::Any, Pat::Any], "k3", vec![
                bind_candidate(s("a"), &stored, s("x"), 0),
                bind_candidate(s("b"), &stored, s("x"), 0),
            ]))
            .err(),
            non_canonical
        );
        let waiting = txn.continuations(&group).expect("read the canonical group")[0].clone();
        let fire = SpaceProduce {
            channel: s("b"),
            data: s("y"),
            persist: false,
            outcome: ProduceOutcome::Fire {
                channels: reversed.clone(),
                continuation: waiting,
                data: vec![
                    bind_candidate(s("a"), &stored, s("x"), 0),
                    incoming_candidate(
                        s("b"),
                        produce_source("b", "y", false),
                        s("y"),
                        false,
                        s("y"),
                    ),
                ],
            },
            before_observe: None,
        };
        assert_eq!(txn.produce(fire).err(), non_canonical);
        txn.consume(park_op(&["b", "a"], vec![Pat::Any, Pat::Any], "k4", false, BTreeSet::new()))
            .expect("the transaction stays usable");
    }
    assert_eq!(continuation_keys(&**space.get_store()), keys);
    assert_eq!(logical_state(&space), logical_state(&reference));
    assert_eq!(checkpoint(&space).await.root, checkpoint(&reference).await.root);
}

// Review F-ORDER-4: a park on the permuted order of a staged group is
// rejected, so the sweep removes one group and the checkpoint returns to the
// empty root.
#[tokio::test]
async fn space_store_handle_permuted_park_rejected_then_sweep_keeps_checkpoint() {
    let empty_root = checkpoint(&play_space().await).await.root;
    let space = play_space().await;
    let handle = SpaceStoreHandle::new(space.clone());
    let mut txn = handle.begin(&universe()).await.expect("begin");
    txn.consume(park_op(&["v1", "v2"], vec![Pat::Any, Pat::Any], "k1", false, BTreeSet::new()))
        .expect("stage [v1, v2]");
    assert_eq!(
        txn.consume(park_op(&["v2", "v1"], vec![Pat::Any, Pat::Any], "k2", false, BTreeSet::new()))
            .err(),
        Some(SpaceTxnError::NonCanonicalChannelOrder)
    );
    assert_eq!(txn.commit().await.expect("commit").ops.len(), 1);
    assert_eq!(
        handle.sweep_volatile(&names(&["v1", "v2"])).await,
        Ok(SweepReport {
            channels: 2,
            data: 0,
            continuations: 1,
            joins: 2,
        })
    );
    let keys = continuation_keys(&**space.get_store());
    assert!(keys.contains(&names(&["v1", "v2"])));
    assert!(keys.iter().all(|key| group_rejection(key).is_none()));
    assert_eq!(checkpoint(&space).await.root, empty_root);
}

// A group that ordinary code stored in a non-canonical order fails closed in
// every substrate read, stage and sweep that meets it, with no mutation; the
// store keeps one key per channel set, so the checkpoint is intact.
#[tokio::test]
async fn space_store_handle_non_canonical_stored_join_fails_closed() {
    let host_order = permuted(&canonical(&["a", "b"]));
    let reference = play_space().await;
    let space = play_space().await;
    for target in [&reference, &space] {
        target
            .consume(host_order.clone(), vec![Pat::Any, Pat::Any], s("k"), false, BTreeSet::new())
            .await
            .expect("the host parks on a non-canonical group");
    }
    let handle = SpaceStoreHandle::new(space.clone());
    let non_canonical = Some(SpaceTxnError::NonCanonicalChannelOrder);
    {
        let mut txn = handle.begin(&universe()).await.expect("begin");
        assert_eq!(txn.joins(&s("a")).err(), non_canonical);
        assert_eq!(txn.joins(&s("b")).err(), non_canonical);
        assert!(txn.data(&s("a")).expect("read a").is_empty());
        assert_eq!(txn.produce(store_op("a", "x", false)).err(), non_canonical);
        assert_eq!(
            txn.consume(park_op(&["a"], vec![Pat::Any], "k2", false, BTreeSet::new()))
                .err(),
            non_canonical
        );
        assert_eq!(txn.commit().await.expect("commit nothing").ops.len(), 0);
    }
    assert_eq!(
        handle.sweep_volatile(&names(&["a", "b"])).await,
        Err(SpaceTxnError::NonCanonicalChannelOrder)
    );
    assert_eq!(logical_state(&space), logical_state(&reference));
    assert_eq!(checkpoint(&space).await.root, checkpoint(&reference).await.root);
}

// Control (HEAD behavior, review F-ORDER-1): the host refuses a checkpoint
// whose hot store holds one channel set under two orders, the duplicate-key
// check of radix_history.rs. Canonical order keeps the substrate from
// creating such a store.
#[tokio::test]
async fn space_store_handle_host_permuted_keys_break_checkpoint_control() {
    let space = play_space().await;
    let group = canonical(&["a", "b"]);
    space
        .consume(group.clone(), vec![Pat::Any, Pat::Any], s("k"), false, BTreeSet::new())
        .await
        .expect("park on the canonical group");
    assert!(
        space
            .get_waiting_continuations(permuted(&group))
            .await
            .is_empty()
    );
    let refused = match AssertUnwindSafe(space.create_checkpoint())
        .catch_unwind()
        .await
    {
        Err(payload) => panic_text(payload),
        Ok(Err(error)) => format!("{:?}", error),
        Ok(Ok(checkpoint)) => panic!("expected a refused checkpoint, got {:?}", checkpoint.root),
    };
    assert!(refused.contains("duplicate actions on one key"), "{}", refused);
}

// Review F-PAIR-1: replay pairs a COMM only under its trigger key, as host
// replay does. A log without the Produce event of a produce-side fire fails
// that fire with `ReplayDivergence` and no effect; host replay rejects the
// same log.
#[tokio::test]
async fn space_store_handle_replay_requires_produce_trigger_event() {
    let groups =
        each(vec![Op::consume(&["a"], vec![Pat::Any], "k", false), Op::produce("a", "x", false)]);
    let plans = plan_groups(&groups).await;
    let (play, replay) = play_and_replay().await;
    let start = checkpoint(&play).await;
    receipts_ok(run_plans(&SpaceStoreHandle::new(play.clone()), &plans).await);
    let played = checkpoint(&play).await;
    let x = produce_source("a", "x", false);
    let forged = without_event(&played.log, &IOEvent::Produce(x.clone()));
    assert_eq!(forged.len() + 1, played.log.len());

    let host_replay = replay_over(&play);
    rig(&host_replay, &start.root, &forged).await;
    run_host_ops(&host_replay, &flatten(&groups)).await;
    assert!(matches!(
        host_replay.check_replay_data().await,
        Err(RSpaceError::UnusedCommEvent { .. })
    ));

    let recorder = Recorder::new();
    observed(&replay, &recorder);
    rig(&replay, &start.root, &forged).await;
    let handle = SpaceStoreHandle::new(replay.clone());
    receipts_ok(run_plans(&handle, &plans[..1]).await);
    let calls = recorder.calls();
    let bindings = replay_bindings(&replay);
    let state = replay_state(&replay);
    let mut txn = handle.begin(&universe()).await.expect("begin");
    assert_eq!(txn.recorded_comms(RSpaceOperationSource::Produce(&x)), Some(Vec::new()));
    match plans[1][0].stage(&mut txn, None) {
        Err(SpaceTxnError::ReplayDivergence(comm)) => assert_eq!(comm.produces, vec![x.clone()]),
        other => panic!("expected a replay divergence, got {:?}", other),
    }
    assert_eq!(txn.commit().await.expect("commit nothing").ops.len(), 0);
    assert_eq!(recorder.calls(), calls);
    assert_eq!(replay_bindings(&replay), bindings);
    assert_eq!(replay_state(&replay), state);
    assert!(matches!(replay.check_replay_data().await, Err(RSpaceError::UnusedCommEvent { .. })));
}

// The consume-side form of F-PAIR-1: a log without the Consume event of a
// consume-side fire fails that fire with `ReplayDivergence` and no effect.
// Control (HEAD behavior): host replay still pairs this COMM, because `rig`
// puts every COMM into `consume_replay_index` whether or not the log holds
// its Consume event, and the narrowed consume path reads that index. The
// substrate pairs only under the trigger key.
#[tokio::test]
async fn space_store_handle_replay_requires_consume_trigger_event() {
    let groups =
        each(vec![Op::produce("a", "x", false), Op::consume(&["a"], vec![Pat::Any], "k", false)]);
    let plans = plan_groups(&groups).await;
    let (play, replay) = play_and_replay().await;
    let start = checkpoint(&play).await;
    receipts_ok(run_plans(&SpaceStoreHandle::new(play.clone()), &plans).await);
    let played = checkpoint(&play).await;
    let k = consume_source(&["a"], &[Pat::Any], "k", false);
    let forged = without_event(&played.log, &IOEvent::Consume(k.clone()));
    assert_eq!(forged.len() + 1, played.log.len());

    let host_replay = replay_over(&play);
    rig(&host_replay, &start.root, &forged).await;
    run_host_ops(&host_replay, &flatten(&groups)).await;
    host_replay
        .check_replay_data()
        .await
        .expect("control: host replay pairs through consume_replay_index");

    rig(&replay, &start.root, &forged).await;
    let handle = SpaceStoreHandle::new(replay.clone());
    receipts_ok(run_plans(&handle, &plans[..1]).await);
    let bindings = replay_bindings(&replay);
    let state = replay_state(&replay);
    let mut txn = handle.begin(&universe()).await.expect("begin");
    assert_eq!(txn.recorded_comms(RSpaceOperationSource::Consume(&k)), Some(Vec::new()));
    match plans[1][0].stage(&mut txn, None) {
        Err(SpaceTxnError::ReplayDivergence(comm)) => assert_eq!(comm.consume, k),
        other => panic!("expected a replay divergence, got {:?}", other),
    }
    assert_eq!(txn.commit().await.expect("commit nothing").ops.len(), 0);
    assert_eq!(replay_bindings(&replay), bindings);
    assert_eq!(replay_state(&replay), state);
    assert!(matches!(replay.check_replay_data().await, Err(RSpaceError::UnusedCommEvent { .. })));
}

// Review F-SUB-1: under one parent order at most one transaction commits. A
// second commit fails with `ParentOrderReused` before any effect, so the
// sub-orders stay unique and the ordered log keeps commit order. A new
// session resets the tracking; commits without an operation context are
// not tracked and emit no sub-orders.
#[tokio::test]
async fn space_store_handle_parent_order_admits_one_commit() {
    let space = play_space().await;
    let recorder = Recorder::new();
    observed(&space, &recorder);
    let handle = SpaceStoreHandle::new(space.clone());
    let parent = OperationOrder {
        session: [3; 32],
        path: CausalPath::from(vec![(0, 0)]),
    };
    let second = operation_context::scope(parent.clone(), async {
        let mut first = handle.begin(&universe()).await.expect("begin the first");
        first.produce(store_op("a", "x", false)).expect("stage a!x");
        first.produce(store_op("a", "y", false)).expect("stage a!y");
        first.commit().await.expect("commit the first");
        let mut second = handle.begin(&universe()).await.expect("begin the second");
        second
            .produce(store_op("b", "z", false))
            .expect("stage b!z");
        second.commit().await
    })
    .await;
    assert_eq!(second.err(), Some(SpaceTxnError::ParentOrderReused));
    assert!(space.get_data(&s("b")).await.is_empty());
    assert_eq!(recorder.orders(), vec![Some(sub_order(&parent, 0)), Some(sub_order(&parent, 1))]);
    assert_eq!(recorder.calls().len(), 6);
    assert_eq!(
        event_bytes(&space.take_event_log().await),
        event_bytes(&[produced("a", "x"), produced("a", "y")])
    );

    let next = OperationOrder {
        session: [4; 32],
        path: parent.path.clone(),
    };
    operation_context::scope(next.clone(), async {
        let mut txn = handle.begin(&universe()).await.expect("begin");
        txn.produce(store_op("b", "z", false)).expect("stage b!z");
        txn.commit()
            .await
            .expect("a new session resets the tracking");
    })
    .await;
    for value in ["u", "w"] {
        let mut txn = handle.begin(&universe()).await.expect("begin");
        txn.produce(store_op("c", value, false)).expect("stage");
        txn.commit()
            .await
            .expect("a commit without a parent is not tracked");
    }
    assert_eq!(recorder.orders()[2..].to_vec(), vec![Some(sub_order(&next, 0)), None, None]);
    assert_eq!(space.get_data(&s("c")).await.len(), 2);
}

// Review F-LOCK-1: a staged operation whose channel stripe the transaction
// does not hold fails with `UnlockedChannel` before any hot-store access,
// so a concurrent host produce cannot slip between staging and commit.
#[tokio::test]
async fn space_store_handle_uncovered_channel_fails_closed() {
    let reference = play_space().await;
    run_host_ops(&reference, &[Op::produce("a", "x", false)]).await;
    let space = play_space().await;
    let handle = SpaceStoreHandle::new(space.clone());
    let mut rejected = false;
    for anchor in ["anchor-0", "anchor-1", "anchor-2", "anchor-3"] {
        let mut txn = handle.begin(&[s(anchor)]).await.expect("begin");
        match txn.consume(park_op(&["a"], vec![Pat::Any], "k", false, BTreeSet::new())) {
            Err(SpaceTxnError::UnlockedChannel) => {
                let produced = space
                    .produce(s("a"), s("x"), false)
                    .now_or_never()
                    .expect("the host produce does not wait for the transaction")
                    .expect("the host produce");
                assert!(produced.is_none());
                assert_eq!(txn.commit().await.expect("commit nothing").ops.len(), 0);
                rejected = true;
                break;
            }
            Ok(_) => continue,
            Err(error) => panic!("unexpected staging error {:?}", error),
        }
    }
    assert!(rejected, "an uncovered channel must fail closed");
    assert!(continuation_keys(&**space.get_store()).is_empty());
    assert_eq!(logical_state(&space), logical_state(&reference));
    assert_eq!(checkpoint(&space).await.root, checkpoint(&reference).await.root);
}

// Lock coverage: every channel a staged operation touches must have its
// stripe held, in the phase the host uses for it: the produce channel, the
// channels of its join groups, the fired group, and the consume channels.
// Reads are not staged and need no coverage.
#[tokio::test]
async fn space_store_handle_lock_coverage_covers_touched_channels() {
    let space = play_space().await;
    let join = canonical(&["a", "b"]);
    run_host_ops(&space, &[
        Op::consume(&["b", "a"], vec![Pat::Exact(s("never")), Pat::Any], "k", false),
        Op::produce("c", "z", false),
    ])
    .await;
    let handle = SpaceStoreHandle::new(space.clone());
    let unlocked = Some(SpaceTxnError::UnlockedChannel);
    let state = logical_state(&space);

    let mut txn = handle.begin(&names(&["b"])).await.expect("begin with b");
    assert_eq!(txn.produce(store_op("a", "x", false)).err(), unlocked);
    assert_eq!(txn.produce(store_op("b", "y", false)).err(), unlocked);
    assert_eq!(
        txn.consume(park_op(&["c"], vec![Pat::Any], "k2", false, BTreeSet::new()))
            .err(),
        unlocked
    );
    assert_eq!(
        txn.consume(park_op(&["b", "a"], vec![Pat::Any, Pat::Any], "k3", false, BTreeSet::new()))
            .err(),
        unlocked
    );
    let z = txn.data(&s("c")).expect("reads need no coverage")[0].clone();
    assert_eq!(
        txn.consume(fire_consume_op(&["c"], vec![Pat::Any], "k4", vec![bind_candidate(
            s("c"),
            &z,
            s("z"),
            0
        )]))
        .err(),
        unlocked
    );
    let waiting = txn.continuations(&join).expect("reads need no coverage")[0].clone();
    let fire = SpaceProduce {
        channel: s("b"),
        data: s("y"),
        persist: false,
        outcome: ProduceOutcome::Fire {
            channels: join.clone(),
            continuation: waiting,
            data: vec![incoming_candidate(
                s("b"),
                produce_source("b", "y", false),
                s("y"),
                false,
                s("y"),
            )],
        },
        before_observe: None,
    };
    assert_eq!(txn.produce(fire).err(), unlocked);
    assert!(
        txn.joins(&s("a"))
            .expect("reads need no coverage")
            .contains(&join)
    );
    assert_eq!(txn.commit().await.expect("commit nothing").ops.len(), 0);

    let mut txn = handle.begin(&names(&["a"])).await.expect("begin with a");
    assert_eq!(txn.produce(store_op("a", "x", false)).err(), unlocked);
    drop(txn);
    assert_eq!(logical_state(&space), state);

    let mut txn = handle
        .begin(&names(&["a", "b"]))
        .await
        .expect("begin with a and b");
    txn.produce(store_op("a", "x", false))
        .expect("the channel and its join partner are covered");
    txn.consume(park_op(
        &["b", "a"],
        vec![Pat::Exact(s("never")), Pat::Any],
        "k5",
        false,
        BTreeSet::new(),
    ))
    .expect("both consume channels are covered");
    assert_eq!(txn.commit().await.expect("commit").ops.len(), 2);
}

// With the channel covered, a host produce on it waits for the commit, and
// the final state equals the serial order: the transaction's park, then the
// host produce, which fires it.
#[tokio::test]
async fn space_store_handle_host_produce_waits_for_commit() {
    let space = play_space().await;
    let handle = SpaceStoreHandle::new(space.clone());
    let mut txn = handle.begin(&names(&["a"])).await.expect("begin");
    txn.consume(park_op(&["a"], vec![Pat::Any], "k", false, BTreeSet::new()))
        .expect("stage the park");
    let mut host_produce = Box::pin(space.produce(s("a"), s("x"), false));
    assert!(futures::poll!(&mut host_produce).is_pending());
    tokio::task::yield_now().await;
    assert!(futures::poll!(&mut host_produce).is_pending());
    txn.commit().await.expect("commit");
    let fired = host_produce.await.expect("the host produce");
    assert!(fired.is_some());

    let serial = play_space().await;
    run_host_ops(&serial, &[
        Op::consume(&["a"], vec![Pat::Any], "k", false),
        Op::produce("a", "x", false),
    ])
    .await;
    assert_eq!(logical_state(&space), logical_state(&serial));
    let (concurrent, serial) = (checkpoint(&space).await, checkpoint(&serial).await);
    assert_eq!(event_bytes(&concurrent.log), event_bytes(&serial.log));
    assert_eq!(concurrent.root, serial.root);
}

// A second `begin` in a task that holds an open transaction of the handle
// fails closed at once, before it takes any lock, so it cannot deadlock.
// Another task waits for the locks instead. Commit, a commit error, a failed
// begin and drop all end the registration.
#[tokio::test]
async fn space_store_handle_nested_begin_fails_closed() {
    let space = play_space().await;
    let handle = SpaceStoreHandle::new(space.clone());
    let keys = names(&["a"]);

    let mut first = handle.begin(&keys).await.expect("begin");
    for nested_keys in [names(&["a"]), names(&["b"])] {
        assert!(matches!(
            handle.begin(&nested_keys).now_or_never(),
            Some(Err(SpaceTxnError::NestedTransaction))
        ));
        assert!(matches!(
            handle.clone().begin(&nested_keys).now_or_never(),
            Some(Err(SpaceTxnError::NestedTransaction))
        ));
    }
    let waiting_handle = handle.clone();
    let waiting = tokio::spawn(async move {
        waiting_handle
            .begin(&names(&["a"]))
            .await
            .map(|txn| txn.mode())
    });
    tokio::task::yield_now().await;
    assert!(!waiting.is_finished());
    first.produce(store_op("a", "x", false)).expect("stage");
    first.commit().await.expect("commit ends the registration");
    assert_eq!(waiting.await.expect("join the other task"), Ok(SpaceMode::Play));

    let txn = handle.begin(&keys).await.expect("begin after the commit");
    drop(txn);
    let mut txn = handle.begin(&keys).await.expect("begin after the drop");
    txn.produce(store_op("a", "y", false)).expect("stage");
    let soft = space.create_soft_checkpoint().await;
    space
        .revert_to_soft_checkpoint(soft)
        .await
        .expect("swap the hot store");
    assert_eq!(txn.commit().await.err(), Some(SpaceTxnError::StoreSwapped));
    assert!(handle.begin(&[]).await.is_err());
    let txn = handle.begin(&keys).await.expect("begin after the errors");
    assert_eq!(txn.mode(), SpaceMode::Play);
}

// Peek fires in replay and reporting: both pair the play log, keep the
// stored record of the consume-side peek as play does, leave the incoming
// datum of the produce-side peek unstored until its re-offer stores it
// (FR-10), and reach the play root.
async fn run_peek_fires(handle: &Handle) -> Commits {
    let peek = BTreeSet::from([0]);
    let k2 =
        WaitingContinuation::create(&names(&["b"]), &vec![Pat::Any], &s("k2"), false, peek.clone());
    let mut commits = Vec::with_capacity(2);
    let mut txn = handle.begin(&universe()).await.expect("begin");
    txn.produce(store_op("a", "x", false)).expect("stage a!x");
    let stored = txn.data(&s("a")).expect("read a")[0].clone();
    txn.consume(SpaceConsume {
        channels: names(&["a"]),
        patterns: vec![Pat::Any],
        continuation: s("k"),
        persist: false,
        peeks: peek.clone(),
        outcome: ConsumeOutcome::Fire {
            data: vec![bind_candidate(s("a"), &stored, s("x"), 0)],
        },
        before_observe: None,
    })
    .expect("stage a peek consume fire");
    txn.consume(park_op(&["b"], vec![Pat::Any], "k2", false, peek.clone()))
        .expect("stage a peek park");
    txn.produce(SpaceProduce {
        channel: s("b"),
        data: s("y"),
        persist: false,
        outcome: ProduceOutcome::Fire {
            channels: names(&["b"]),
            continuation: k2,
            data: vec![incoming_candidate(
                s("b"),
                produce_source("b", "y", false),
                s("y"),
                false,
                s("y"),
            )],
        },
        before_observe: None,
    })
    .expect("stage a peek produce fire");
    assert!(txn.data(&s("b")).expect("read b").is_empty());
    txn.produce(store_op("b", "y", false))
        .expect("stage the re-offer of y, which no waiter takes");
    commits.push(txn.commit().await);
    let mut txn = handle.begin(&universe()).await.expect("begin");
    let reoffered = txn.data(&s("b")).expect("read b")[0].clone();
    txn.consume(fire_consume_op(&["b"], vec![Pat::Any], "k3", vec![bind_candidate(
        s("b"),
        &reoffered,
        s("y"),
        0,
    )]))
    .expect("consume the re-offered record");
    commits.push(txn.commit().await);
    commits
}

#[tokio::test]
async fn space_store_handle_peek_fires_replay_and_report() {
    let play = play_space().await;
    let start = checkpoint(&play).await;
    let played_receipts = receipts_ok(run_peek_fires(&SpaceStoreHandle::new(play.clone())).await);
    assert_eq!(play.get_data(&s("a")).await, vec![datum("a", "x", false)]);
    assert!(play.get_data(&s("b")).await.is_empty());
    match &played_receipts[0].ops[3..] {
        [SpaceOpReceipt::Fired { results, .. }, SpaceOpReceipt::Stored { produce }] => {
            assert_eq!(results.len(), 1);
            assert!(!results[0].persistent);
            assert_eq!(produce, &produce_source("b", "y", false));
        }
        other => panic!("expected the peek produce fire and its re-offer, got {:?}", other),
    }
    let played = checkpoint(&play).await;
    assert_eq!(comms_of(&played.log).len(), 3);
    assert_eq!(kinds(&played.log), vec![
        "produce", "consume", "comm", "consume", "produce", "comm", "produce", "consume", "comm"
    ]);

    let replay = replay_over(&play);
    rig(&replay, &start.root, &played.log).await;
    let replayed = receipts_ok(run_peek_fires(&SpaceStoreHandle::new(replay.clone())).await);
    assert_eq!(replayed, played_receipts);
    replay
        .check_replay_data()
        .await
        .expect("replay pairs every peek COMM");
    assert_eq!(checkpoint(&replay).await.root, played.root);

    let reporting = reporting_over(&play);
    reporting
        .rig_and_reset(start.root.clone(), played.log.clone())
        .await
        .expect("rig the reporting space");
    let reported = receipts_ok(run_peek_fires(&SpaceStoreHandle::new(reporting.clone())).await);
    assert_eq!(reported, played_receipts);
    reporting
        .check_replay_data()
        .await
        .expect("reporting pairs every peek COMM");
    let reported_comms = reporting
        .get_report()
        .expect("the report")
        .iter()
        .flat_map(|batch| batch.events.iter())
        .filter(|event| matches!(event, ReportingEvent::ReportingComm(_)))
        .count();
    assert_eq!(reported_comms, 3);
    let reported_root = ISpace::create_checkpoint(&reporting)
        .await
        .expect("checkpoint the reporting space")
        .root;
    assert_eq!(reported_root, played.root);
}

// The sweep refuses a channel whose join leaf, or whose continuation leaf,
// is at the history root, each one on its own, with no change to the store.
#[tokio::test]
async fn space_store_handle_sweep_rejects_persisted_join_and_continuation_leaves() {
    let space = play_space().await;
    let waiting = WaitingContinuation::create(
        &names(&["v1"]),
        &vec![Pat::Any],
        &s("k"),
        false,
        BTreeSet::new(),
    );
    space.get_store().put_continuation(&names(&["v1"]), waiting);
    space.get_store().put_join(&s("v2"), &names(&["v2"]));
    let root = checkpoint(&space).await.root;
    let reader = space
        .get_history_repository()
        .get_history_reader(&root)
        .expect("read the root");
    let leaf_present = |bytes: Vec<Vec<u8>>| !bytes.is_empty();
    let continuation_leaf = |channel: &str| {
        reader
            .get_continuations_proj_binary(&hash_from_vec(&names(&[channel])))
            .expect("read a continuation leaf")
    };
    let join_leaf = |channel: &str| {
        reader
            .get_joins_proj_binary(&hash(&s(channel)))
            .expect("read a join leaf")
    };
    let data_leaf = |channel: &str| {
        reader
            .get_data_proj_binary(&hash(&s(channel)))
            .expect("read a data leaf")
    };
    assert!(leaf_present(continuation_leaf("v1")));
    assert!(!leaf_present(join_leaf("v1")) && !leaf_present(data_leaf("v1")));
    assert!(leaf_present(join_leaf("v2")));
    assert!(!leaf_present(continuation_leaf("v2")) && !leaf_present(data_leaf("v2")));

    let handle = SpaceStoreHandle::new(space.clone());
    let groups = vec![names(&["v1"]), names(&["v2"])];
    let before = channel_view(&space, &names(&["v1", "v2"]), &groups).await;
    for channel in ["v1", "v2"] {
        assert_eq!(
            handle.sweep_volatile(&names(&[channel])).await,
            Err(SpaceTxnError::SweepTouchesPersisted)
        );
    }
    assert_eq!(channel_view(&space, &names(&["v1", "v2"]), &groups).await, before);
    assert_eq!(checkpoint(&space).await.root, root);
}

// The sweep report counts what the sweep removed: distinct channels, data
// records, continuations, and the join entries actually removed.
#[tokio::test]
async fn space_store_handle_sweep_report_counts_removed_entries() {
    let space = play_space().await;
    let handle = SpaceStoreHandle::new(space.clone());
    let never = || Pat::Exact(s("never"));
    let mut txn = handle.begin(&universe()).await.expect("begin");
    txn.consume(park_op(&["v1", "v2"], vec![never(), never()], "k1", false, BTreeSet::new()))
        .expect("park k1");
    txn.consume(park_op(&["v1"], vec![never()], "k2", false, BTreeSet::new()))
        .expect("park k2");
    txn.consume(park_op(&["v1"], vec![never()], "k3", true, BTreeSet::new()))
        .expect("park k3");
    for (channel, value) in [("v1", "x"), ("v2", "y"), ("v2", "y")] {
        txn.produce(store_op(channel, value, false))
            .expect("store a record");
    }
    txn.commit().await.expect("commit");

    let volatile = names(&["v1", "v2"]);
    let mut expected = SweepReport {
        channels: volatile.len(),
        ..SweepReport::default()
    };
    let mut groups: BTreeSet<Vec<String>> = BTreeSet::new();
    for channel in &volatile {
        expected.data += space.get_data(channel).await.len();
        let joins = space.get_joins(channel.clone()).await;
        expected.joins += joins.len();
        groups.extend(joins);
        groups.insert(vec![channel.clone()]);
    }
    for group in &groups {
        expected.continuations += space.get_waiting_continuations(group.clone()).await.len();
    }
    assert_eq!(expected, SweepReport {
        channels: 2,
        data: 3,
        continuations: 3,
        joins: 3,
    });
    assert_eq!(handle.sweep_volatile(&names(&["v1", "v2", "v1"])).await, Ok(expected));
    for channel in &volatile {
        assert!(space.get_data(channel).await.is_empty());
        assert!(space.get_joins(channel.clone()).await.is_empty());
    }
    assert_eq!(
        handle.sweep_volatile(&volatile).await,
        Ok(SweepReport {
            channels: 2,
            ..SweepReport::default()
        })
    );
}

// ---------------------------------------------------------------- Group G

// FR-1': a consume on a sorted group that repeats a channel binds one record
// per occurrence. Staged from the host outcomes, such consumes give the
// host's observer calls, events, contents and root. Two non-persistent
// occurrences bind two distinct records; a persistent record binds twice.
#[tokio::test]
async fn space_store_handle_repeated_group_consume_fire_binds_distinct_records() {
    let twin = twin().await;
    let exact = |value: &str| Pat::Exact(s(value));
    let groups = vec![
        vec![
            Op::produce("b", "x", false),
            Op::produce("b", "y", false),
            Op::produce("b", "x", false),
        ],
        vec![Op::consume(&["b", "b"], vec![Pat::Any, Pat::Any], "k", false)],
        vec![
            Op::produce("b", "p", true),
            Op::consume(&["b", "b", "a"], vec![exact("p"), exact("p"), Pat::Any], "k2", false),
            Op::produce("a", "w", false),
        ],
    ];
    let (plans, receipts) = run_guided(&twin.host, &twin.txn, &groups).await;
    let receipts = receipts_ok(receipts);
    match (&plans[1][0], &receipts[1].ops[0]) {
        (
            Planned::Consume {
                outcome: ConsumeOutcome::Fire { data },
                ..
            },
            SpaceOpReceipt::Fired { results, comm, .. },
        ) => {
            assert_eq!(data.len(), 2);
            assert_ne!(data[0].datum_index, data[1].datum_index);
            assert!(data.iter().all(|candidate| !candidate.datum.persist));
            assert_eq!(results.len(), 2);
            assert_eq!(comm.produces.len(), 2);
        }
        other => panic!("expected a consume fire on [b, b], got {:?}", other),
    }
    let kept = twin.txn.get_data(&s("b")).await;
    assert_eq!(kept.len(), 2);
    assert!(kept.contains(&datum("b", "p", true)));
    assert_eq!(kept.iter().filter(|record| !record.persist).count(), 1);
    match (&plans[2][2], &receipts[2].ops[2]) {
        (
            Planned::Produce {
                outcome: ProduceOutcome::Fire { data, .. },
                ..
            },
            SpaceOpReceipt::Fired { comm, .. },
        ) => {
            let p = produce_source("b", "p", true);
            assert_eq!(data.len(), 3);
            assert_eq!(data[0].datum_index, data[1].datum_index);
            assert!(data[0].datum.persist && data[1].datum.persist);
            assert_eq!(
                data.iter()
                    .filter(|candidate| candidate.datum_index == -1)
                    .count(),
                1
            );
            assert_eq!(
                comm.produces
                    .iter()
                    .filter(|produce| **produce == p)
                    .count(),
                2
            );
        }
        other => panic!("expected a produce fire on [b, b, a], got {:?}", other),
    }
    assert_eq!(logical_state(&twin.txn), logical_state(&twin.host));
    assert_parity(&twin).await;
}

// FR-1': a produce-side fire on a sorted group that repeats the produced
// channel binds the incoming datum exactly once; the other occurrence binds
// a stored record. A fire that binds the incoming datum twice fails closed.
// Control (HEAD behavior): the host binds a persistent incoming datum once
// per pattern it matches, a fire that the substrate refuses.
#[tokio::test]
async fn space_store_handle_repeated_group_produce_fire_binds_incoming_once() {
    let twin = twin().await;
    let groups = vec![
        vec![Op::consume(&["b", "b"], vec![Pat::Any, Pat::Any], "k", false)],
        vec![Op::produce("b", "y", false)],
        vec![Op::produce("b", "x", false)],
    ];
    let (plans, receipts) = run_guided(&twin.host, &twin.txn, &groups).await;
    let receipts = receipts_ok(receipts);
    assert!(matches!(receipts[1].ops[0], SpaceOpReceipt::Stored { .. }));
    match (&plans[2][0], &receipts[2].ops[0]) {
        (
            Planned::Produce {
                outcome: ProduceOutcome::Fire { data, .. },
                ..
            },
            SpaceOpReceipt::Fired {
                results, produce, ..
            },
        ) => {
            assert_eq!(data.len(), 2);
            assert_eq!(
                data.iter()
                    .filter(|candidate| candidate.datum_index == -1)
                    .count(),
                1
            );
            assert_eq!(
                data.iter()
                    .filter(|candidate| candidate.datum_index == 0)
                    .count(),
                1
            );
            assert_eq!(results.len(), 2);
            assert_eq!(produce, &Some(produce_source("b", "x", false)));
        }
        other => panic!("expected a produce fire on [b, b], got {:?}", other),
    }
    assert!(twin.txn.get_data(&s("b")).await.is_empty());
    assert_eq!(logical_state(&twin.txn), logical_state(&twin.host));
    assert_parity(&twin).await;

    let space = play_space().await;
    run_host_ops(&space, &[Op::consume(&["b", "b"], vec![Pat::Any, Pat::Any], "k", false)]).await;
    let state = logical_state(&space);
    let handle = SpaceStoreHandle::new(space.clone());
    let mut txn = handle.begin(&universe()).await.expect("begin");
    let waiting = txn.continuations(&names(&["b", "b"])).expect("read [b, b]")[0].clone();
    let incoming =
        || incoming_candidate(s("b"), produce_source("b", "x", false), s("x"), false, s("x"));
    let fire = SpaceProduce {
        channel: s("b"),
        data: s("x"),
        persist: false,
        outcome: ProduceOutcome::Fire {
            channels: names(&["b", "b"]),
            continuation: waiting,
            data: vec![incoming(), incoming()],
        },
        before_observe: None,
    };
    assert_eq!(
        txn.produce(fire).err(),
        Some(SpaceTxnError::InvalidOperation(
            "a produce fire needs exactly one incoming candidate"
        ))
    );
    assert_eq!(txn.commit().await.expect("commit nothing").ops.len(), 0);
    assert_eq!(logical_state(&space), state);

    let (_, results, _) = space
        .produce(s("b"), s("x"), true)
        .await
        .expect("the host produce")
        .expect("the host fires");
    assert_eq!(results.len(), 2);
    assert!(
        results
            .iter()
            .all(|result| result.removed_datum == s("x") && result.persistent)
    );
}

// FR-1': a repeat out of sorted order is not canonical. Reads and stages of
// such a group fail closed before any hot-store access, so no key appears
// and the checkpoint keeps the reference root.
#[tokio::test]
async fn space_store_handle_unsorted_repeat_fails_closed() {
    let setup = vec![Op::produce("b", "x", false), Op::produce("a", "y", false)];
    let reference = play_space().await;
    let space = play_space().await;
    for target in [&reference, &space] {
        run_host_ops(target, &setup).await;
    }
    let unsorted = names(&["b", "a", "b"]);
    let non_canonical = Some(SpaceTxnError::NonCanonicalChannelOrder);
    assert_eq!(group_rejection(&unsorted), non_canonical);
    let handle = SpaceStoreHandle::new(space.clone());
    let keys = continuation_keys(&**space.get_store());
    {
        let mut txn = handle.begin(&universe()).await.expect("begin");
        assert_eq!(txn.continuations(&unsorted).err(), non_canonical);
        let any = vec![Pat::Any; 3];
        assert_eq!(
            txn.consume(park_op(&["b", "a", "b"], any.clone(), "k", false, BTreeSet::new()))
                .err(),
            non_canonical
        );
        let x = txn.data(&s("b")).expect("read b")[0].clone();
        let y = txn.data(&s("a")).expect("read a")[0].clone();
        assert_eq!(
            txn.consume(fire_consume_op(&["b", "a", "b"], any.clone(), "k", vec![
                bind_candidate(s("b"), &x, s("x"), 0),
                bind_candidate(s("a"), &y, s("y"), 0),
                bind_candidate(s("b"), &x, s("x"), 0),
            ]))
            .err(),
            non_canonical
        );
        let waiting = WaitingContinuation::create(&unsorted, &any, &s("k"), false, BTreeSet::new());
        let fire = SpaceProduce {
            channel: s("a"),
            data: s("z"),
            persist: false,
            outcome: ProduceOutcome::Fire {
                channels: unsorted.clone(),
                continuation: waiting,
                data: vec![
                    bind_candidate(s("b"), &x, s("x"), 0),
                    incoming_candidate(
                        s("a"),
                        produce_source("a", "z", false),
                        s("z"),
                        false,
                        s("z"),
                    ),
                    bind_candidate(s("b"), &x, s("x"), 0),
                ],
            },
            before_observe: None,
        };
        assert_eq!(txn.produce(fire).err(), non_canonical);
        assert_eq!(txn.commit().await.expect("commit nothing").ops.len(), 0);
    }
    assert_eq!(continuation_keys(&**space.get_store()), keys);
    assert_eq!(logical_state(&space), logical_state(&reference));
    assert_eq!(checkpoint(&space).await.root, checkpoint(&reference).await.root);
}

// Control (HEAD behavior): host replay binds the records of a repeated
// channel in store order, where host play binds them in hash order. So host
// replay of a valid play log can compute another COMM than the log records
// and fail its assertion. The substrate replays the same log, because the
// agent stages the recorded bindings (FR-1').
#[tokio::test]
async fn space_store_handle_host_replay_repeated_channel_control() {
    let ops = vec![
        Op::produce("a", "y", false),
        Op::produce("a", "x", true),
        Op::consume(&["a", "a"], vec![Pat::Any, Pat::Any], "k1", false),
    ];
    let plans = plan_groups(&each(ops.clone())).await;
    let (play, replay) = play_and_replay().await;
    let start = checkpoint(&play).await;
    run_host_ops(&play, &ops).await;
    let played = checkpoint(&play).await;
    let recorded = comms_of(&played.log);
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].produces, vec![
        produce_source("a", "y", false),
        produce_source("a", "x", true)
    ]);

    rig(&replay, &start.root, &played.log).await;
    receipts_ok(run_plans(&SpaceStoreHandle::new(replay.clone()), &plans).await);
    assert!(replay_bindings(&replay).is_empty());
    assert_eq!(checkpoint(&replay).await.root, played.root);

    let host_replay = replay_over(&play);
    rig(&host_replay, &start.root, &played.log).await;
    run_host_ops(&host_replay, &ops[..2]).await;
    let refused = match AssertUnwindSafe(run_host(&host_replay, &ops[2]))
        .catch_unwind()
        .await
    {
        Err(payload) => panic_text(payload),
        Ok(outcome) => panic!("expected the host replay to fail, got {:?}", outcome.map(|_| ())),
    };
    assert!(refused.contains("was not contained in the trace"), "{}", refused);
}

// One evaluation of a deploy: a transaction under `parent` that stores
// `a!value`.
async fn evaluate(
    handle: &Handle,
    parent: &OperationOrder,
    value: &str,
) -> Result<usize, SpaceTxnError> {
    operation_context::scope(parent.clone(), async {
        let mut txn = handle.begin(&universe()).await.expect("begin");
        txn.produce(store_op("a", value, false))
            .expect("stage a store");
        txn.commit().await.map(|receipt| receipt.ops.len())
    })
    .await
}

// FR-5': parent tracking is scoped to one root evaluation. After
// `begin_evaluation_scope`, a second evaluation of one deploy on one handle
// commits under the same parent order, in play and in replay. Without the
// scope reset, the second evaluation fails with `ParentOrderReused` and has
// no effect.
#[tokio::test]
async fn space_store_handle_evaluation_scope_resets_parent_orders() {
    let parent = OperationOrder {
        session: [6; 32],
        path: CausalPath::from(vec![(0, 0)]),
    };
    let (play, replay) = play_and_replay().await;
    let start = checkpoint(&play).await;
    rig(&replay, &start.root, &[]).await;
    let play_recorder = Recorder::new();
    observed(&play, &play_recorder);
    let replay_recorder = Recorder::new();
    observed(&replay, &replay_recorder);
    for (handle, recorder) in [
        (SpaceStoreHandle::new(play.clone()), &play_recorder),
        (SpaceStoreHandle::new(replay.clone()), &replay_recorder),
    ] {
        assert_eq!(evaluate(&handle, &parent, "x").await, Ok(1));
        assert_eq!(evaluate(&handle, &parent, "x").await, Err(SpaceTxnError::ParentOrderReused));
        handle.begin_evaluation_scope();
        assert_eq!(evaluate(&handle, &parent, "x").await, Ok(1));
        assert_eq!(recorder.orders(), vec![
            Some(sub_order(&parent, 0)),
            Some(sub_order(&parent, 0))
        ]);
    }
    assert_eq!(play.get_data(&s("a")).await.len(), 2);
    assert_eq!(replay.get_data(&s("a")).await.len(), 2);
}

// FR-5': a reset or a revert replaces the host's hot store, and the first
// commit against the new store starts a new tracking scope. A second
// evaluation of one deploy after a soft-checkpoint revert (play) or a reset
// (replay) commits under the same parent order; without a store change, the
// next evaluation under that order fails with no effect.
#[tokio::test]
async fn space_store_handle_store_reset_resets_parent_orders() {
    let parent = OperationOrder {
        session: [7; 32],
        path: CausalPath::from(vec![(1, 0)]),
    };
    let (play, replay) = play_and_replay().await;
    let start = checkpoint(&play).await;

    let handle = SpaceStoreHandle::new(play.clone());
    let soft = play.create_soft_checkpoint().await;
    assert_eq!(evaluate(&handle, &parent, "x").await, Ok(1));
    play.revert_to_soft_checkpoint(soft)
        .await
        .expect("revert to the soft checkpoint");
    assert_eq!(evaluate(&handle, &parent, "x").await, Ok(1));
    assert_eq!(evaluate(&handle, &parent, "y").await, Err(SpaceTxnError::ParentOrderReused));
    assert_eq!(play.get_data(&s("a")).await, vec![datum("a", "x", false)]);

    rig(&replay, &start.root, &[]).await;
    let handle = SpaceStoreHandle::new(replay.clone());
    assert_eq!(evaluate(&handle, &parent, "x").await, Ok(1));
    replay
        .reset(&start.root)
        .await
        .expect("reset the replay space");
    assert_eq!(evaluate(&handle, &parent, "x").await, Ok(1));
    assert_eq!(evaluate(&handle, &parent, "y").await, Err(SpaceTxnError::ParentOrderReused));
    assert_eq!(replay.get_data(&s("a")).await, vec![datum("a", "x", false)]);
}

// FR-9': a sweep inside an open transaction of the same task fails closed
// at once with `NestedTransaction`, before it takes any lock, instead of
// waiting for the transaction's own stripes. A sweep in another task waits
// for the commit, and after the commit the same task sweeps.
#[tokio::test]
async fn space_store_handle_sweep_in_open_transaction_fails_closed() {
    let space = play_space().await;
    let handle = SpaceStoreHandle::new(space.clone());
    let volatile = names(&["v1", "v2"]);
    let never = || Pat::Exact(s("never"));
    let mut txn = handle.begin(&volatile).await.expect("begin");
    txn.consume(park_op(&["v1", "v2"], vec![never(), never()], "k", false, BTreeSet::new()))
        .expect("park on [v1, v2]");
    txn.commit().await.expect("commit");
    let state = logical_state(&space);

    let mut open = handle.begin(&volatile).await.expect("begin");
    for channels in [volatile.clone(), names(&["a"])] {
        assert_eq!(
            handle.sweep_volatile(&channels).now_or_never(),
            Some(Err(SpaceTxnError::NestedTransaction))
        );
        assert_eq!(
            handle.clone().sweep_volatile(&channels).now_or_never(),
            Some(Err(SpaceTxnError::NestedTransaction))
        );
    }
    assert_eq!(logical_state(&space), state);
    let other_handle = handle.clone();
    let other_channels = volatile.clone();
    let other = tokio::spawn(async move { other_handle.sweep_volatile(&other_channels).await });
    tokio::task::yield_now().await;
    assert!(!other.is_finished());
    open.produce(store_op("v1", "x", false))
        .expect("stage a store");
    open.commit().await.expect("commit releases the locks");
    assert_eq!(
        other.await.expect("join the other task"),
        Ok(SweepReport {
            channels: 2,
            data: 1,
            continuations: 1,
            joins: 2,
        })
    );
    assert_eq!(
        handle.sweep_volatile(&volatile).await,
        Ok(SweepReport {
            channels: 2,
            ..SweepReport::default()
        })
    );
}

// FR-7': the commit-time state check runs in every build profile and checks
// the entries that an operation changes. A store write that bypasses the
// transaction's locks and moves a record or a continuation that an
// operation removes fails the commit with `Inconsistent` before any effect
// of that operation, in debug and in release builds alike.
#[tokio::test]
async fn space_store_handle_commit_checks_run_in_every_profile() {
    let space = play_space().await;
    let recorder = Recorder::new();
    observed(&space, &recorder);
    run_host_ops(&space, &[
        Op::produce("a", "x", false),
        Op::consume(&["b"], vec![Pat::Any], "k", false),
    ])
    .await;
    space.take_event_log().await;
    let calls = recorder.calls().len();
    let handle = SpaceStoreHandle::new(space.clone());
    let inconsistent =
        Some(SpaceTxnError::Inconsistent("the committed state differs from the staged view"));

    let mut txn = handle.begin(&universe()).await.expect("begin");
    let stored = txn.data(&s("a")).expect("read a")[0].clone();
    txn.consume(fire_consume_op(&["a"], vec![Pat::Any], "k2", vec![bind_candidate(
        s("a"),
        &stored,
        s("x"),
        0,
    )]))
    .expect("stage a consume fire");
    space.get_store().put_datum(&s("a"), datum("a", "y", false));
    assert_eq!(txn.commit().await.err(), inconsistent);
    assert_eq!(space.get_data(&s("a")).await, vec![datum("a", "y", false), datum("a", "x", false)]);
    assert_eq!(recorder.calls().len(), calls);
    assert!(space.take_event_log().await.is_empty());

    let waiting = space.get_waiting_continuations(names(&["b"])).await;
    let mut txn = handle.begin(&universe()).await.expect("begin");
    txn.produce(SpaceProduce {
        channel: s("b"),
        data: s("z"),
        persist: false,
        outcome: ProduceOutcome::Fire {
            channels: names(&["b"]),
            continuation: waiting[0].clone(),
            data: vec![incoming_candidate(
                s("b"),
                produce_source("b", "z", false),
                s("z"),
                false,
                s("z"),
            )],
        },
        before_observe: None,
    })
    .expect("stage a produce fire");
    let other = WaitingContinuation::create(
        &names(&["b"]),
        &vec![Pat::Exact(s("never"))],
        &s("k3"),
        false,
        BTreeSet::new(),
    );
    assert_eq!(
        space
            .get_store()
            .put_continuation(&names(&["b"]), other.clone()),
        Some(true)
    );
    assert_eq!(txn.commit().await.err(), inconsistent);
    assert_eq!(space.get_waiting_continuations(names(&["b"])).await, vec![
        other,
        waiting[0].clone()
    ]);
    assert!(space.get_data(&s("b")).await.is_empty());
    assert_eq!(recorder.calls().len(), calls);
    assert!(space.take_event_log().await.is_empty());
}

thread_local! {
    static RECORD_COPIES: Cell<usize> = const { Cell::new(0) };
    static RECORD_SERIALIZATIONS: Cell<usize> = const { Cell::new(0) };
}

fn record_copies() -> usize { RECORD_COPIES.with(Cell::get) }

fn record_serializations() -> usize { RECORD_SERIALIZATIONS.with(Cell::get) }

/// A datum whose clones and serializations are counted, so a test can
/// measure what the commit checks copy. `record_copies` counts both, and
/// `record_serializations` counts the serializations only.
#[derive(Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(transparent)]
struct Counted(String);

impl Clone for Counted {
    fn clone(&self) -> Self {
        RECORD_COPIES.with(|copies| copies.set(copies.get() + 1));
        Counted(self.0.clone())
    }
}

impl Serialize for Counted {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        RECORD_COPIES.with(|copies| copies.set(copies.get() + 1));
        RECORD_SERIALIZATIONS.with(|serializations| serializations.set(serializations.get() + 1));
        self.0.serialize(serializer)
    }
}

struct CountedMatch;

impl Match<Pat, Counted, String> for CountedMatch {
    fn get(&self, pattern: &Pat, datum: &Counted) -> Option<Counted> {
        match pattern {
            Pat::Any => Some(datum.clone()),
            Pat::Exact(expected) => (*expected == datum.0).then(|| datum.clone()),
        }
    }
}

async fn counted_space() -> RSpace<String, Pat, Counted, String> {
    let mut stores = InMemoryStoreManager::new();
    let store = stores
        .r_space_stores()
        .await
        .expect("create the in-memory RSpace stores");
    RSpace::create(store, Arc::new(Box::new(CountedMatch))).expect("create the RSpace")
}

// FR-7': the always-on checks cost O(changed entries). After the view has
// read a channel once, staging and committing one more store on it clone
// and serialize as many records when the channel holds 1 record as when it
// holds 32: no check copies or serializes a whole list.
#[tokio::test]
async fn space_store_handle_commit_checks_cost_changed_entries() {
    let store = |index: usize| SpaceProduce {
        channel: s("a"),
        data: Counted(format!("value-{}", index)),
        persist: false,
        outcome: ProduceOutcome::Store,
        before_observe: None,
    };
    let mut copies = Vec::with_capacity(2);
    for existing in [1usize, 32] {
        let space = counted_space().await;
        let handle = SpaceStoreHandle::new(space.clone());
        let mut txn = handle.begin(&universe()).await.expect("begin");
        for index in 0..existing {
            txn.produce(store(index)).expect("stage a store");
        }
        txn.commit().await.expect("commit the stored list");
        let mut txn = handle.begin(&universe()).await.expect("begin");
        assert_eq!(txn.data(&s("a")).expect("read a").len(), existing);
        let before_staging = record_copies();
        txn.produce(store(existing)).expect("stage one more store");
        let staging = record_copies() - before_staging;
        let before_commit = record_copies();
        assert_eq!(txn.commit().await.expect("commit").ops.len(), 1);
        let commit = record_copies() - before_commit;
        assert_eq!(space.get_data(&s("a")).await.len(), existing + 1);
        copies.push((staging, commit));
    }
    assert_eq!(copies[0], copies[1]);
}

// One measurement of `space_store_handle_fire_checks_cost_changed_entries`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FireCost {
    list_read: usize,
    staging: usize,
    commit_clones: usize,
    commit_serializations: usize,
}

// FR-7': the checks of a fire also cost O(changed entries). A consume fire
// binds the newest of n stored records on its channel. Staging copies as
// many records for n = 1 as for n = 32, and the commit serializes as many
// records: no check serializes or compares a whole list. To apply the
// fire, the commit reads the bound channel's list once, because the host
// removes a record by its list index, as the host consume reads the list
// for its match. So the commit's clones grow by exactly one list read.
#[tokio::test]
async fn space_store_handle_fire_checks_cost_changed_entries() {
    let store = |index: usize| SpaceProduce {
        channel: s("a"),
        data: Counted(format!("value-{}", index)),
        persist: false,
        outcome: ProduceOutcome::Store,
        before_observe: None,
    };
    let mut costs = Vec::with_capacity(2);
    for existing in [1usize, 32] {
        let space = counted_space().await;
        let handle = SpaceStoreHandle::new(space.clone());
        let mut txn = handle.begin(&universe()).await.expect("begin");
        for index in 0..existing {
            txn.produce(store(index)).expect("stage a store");
        }
        txn.commit().await.expect("commit the stored list");
        let before_read = record_copies();
        assert_eq!(space.get_data(&s("a")).await.len(), existing);
        let list_read = record_copies() - before_read;
        let mut txn = handle.begin(&universe()).await.expect("begin");
        let view = txn.data(&s("a")).expect("read a");
        assert_eq!(view.len(), existing);
        let newest = view[0].clone();
        let fire = SpaceConsume {
            channels: names(&["a"]),
            patterns: vec![Pat::Any],
            continuation: s("k"),
            persist: false,
            peeks: BTreeSet::new(),
            outcome: ConsumeOutcome::Fire {
                data: vec![bind_candidate(s("a"), &newest, newest.a.clone(), 0)],
            },
            before_observe: None,
        };
        let before_staging = record_copies();
        txn.consume(fire).expect("stage the consume fire");
        let staging = record_copies() - before_staging;
        let (before_clones, before_serializations) = (record_copies(), record_serializations());
        assert_eq!(txn.commit().await.expect("commit").ops.len(), 1);
        let commit_serializations = record_serializations() - before_serializations;
        let commit_clones = record_copies() - before_clones - commit_serializations;
        assert_eq!(space.get_data(&s("a")).await.len(), existing - 1);
        costs.push(FireCost {
            list_read,
            staging,
            commit_clones,
            commit_serializations,
        });
    }
    let (one, many) = (costs[0], costs[1]);
    assert_eq!(one.staging, many.staging, "staging copies: {costs:?}");
    assert_eq!(one.commit_serializations, many.commit_serializations, "commit serializations: {costs:?}");
    assert_eq!(
        many.commit_clones - one.commit_clones,
        many.list_read - one.list_read,
        "commit clones beyond one list read: {costs:?}"
    );
}

// ---------------------------------------------------------------- Group H

// A waiter on `channels` with one `Pat::Any` per channel. A peek waiter
// peeks every bind.
fn waiter(channels: &[&str], continuation: &str, peek: bool) -> WaitingContinuation<Pat, String> {
    let peeks = match peek {
        true => (0..channels.len() as i32).collect(),
        false => BTreeSet::new(),
    };
    WaitingContinuation::create(
        &names(channels),
        &vec![Pat::Any; channels.len()],
        &s(continuation),
        false,
        peeks,
    )
}

// A non-persistent produce of `data` on `channel` that fires the
// single-channel `continuation` with the incoming datum.
fn fire_produce_op(
    channel: &str,
    data: &str,
    continuation: &WaitingContinuation<Pat, String>,
) -> SpaceProduce<String, Pat, String, String> {
    SpaceProduce {
        channel: s(channel),
        data: s(data),
        persist: false,
        outcome: ProduceOutcome::Fire {
            channels: names(&[channel]),
            continuation: continuation.clone(),
            data: vec![incoming_candidate(
                s(channel),
                produce_source(channel, data, false),
                s(data),
                false,
                s(data),
            )],
        },
        before_observe: None,
    }
}

// The persistent flags of the results of a fire receipt; empty for a store
// or a park.
fn result_flags(receipt: &SpaceOpReceipt<String, Pat, String, String>) -> Vec<bool> {
    match receipt {
        SpaceOpReceipt::Fired { results, .. } => {
            results.iter().map(|result| result.persistent).collect()
        }
        SpaceOpReceipt::Stored { .. } | SpaceOpReceipt::Parked { .. } => Vec::new(),
    }
}

// A name for a peek waiter on `channel` that the host tries before the
// taking waiter `taking`. The host tries the waiting continuations of a
// channel group in the order of `deterministic_candidates`
// (space_matcher.rs): by the Blake2b-256 hash of their bincode bytes.
fn peek_tried_first(channel: &str, taking: &str) -> String {
    let order = |waiting: &WaitingContinuation<Pat, String>| {
        Blake2b256Hash::new(&bincode::serialize(waiting).expect("serialize a waiter"))
    };
    let taking = order(&waiter(&[channel], taking, false));
    (0..64)
        .map(|index| format!("peek-{}", index))
        .find(|name| order(&waiter(&[channel], name, true)) < taking)
        .expect("a peek waiter that the host tries first")
}

// An introduction-authority registry with the rule of
// accounting/mod.rs:959-976: the first registration of an identity keeps
// its authority, and a later registration of that identity passes only with
// an equal authority. The registry records every callback in `recorder`.
#[derive(Default)]
struct AuthorityRegistry {
    entries: Mutex<BTreeMap<Vec<u8>, String>>,
}

impl AuthorityRegistry {
    fn register(self: &Arc<Self>, recorder: &Arc<Recorder>, authority: &str) -> BeforeObserve {
        let registry = Arc::clone(self);
        let record = recorder.before_observe(Ok(()));
        let authority = authority.to_string();
        Box::new(move |source: RSpaceOperationSource<'_>| {
            record(source)?;
            let identity = source_bytes(source);
            let mut entries = registry.entries.lock().expect("registry lock");
            match entries.get(&identity) {
                Some(registered) if *registered == authority => Ok(()),
                Some(_) => {
                    Err(RSpaceError::InterpreterError("event identity conflict".to_string()))
                }
                None => {
                    entries.insert(identity, authority);
                    Ok(())
                }
            }
        })
    }

    fn entries(&self) -> BTreeMap<Vec<u8>, String> {
        self.entries.lock().expect("registry lock").clone()
    }
}

// FR-10 (P4 amendment A1): a produce-side peek fire never stores its
// incoming datum, not in the staged view and not in the committed store, in
// play and in replay. The retained stored record of the fired join stays
// where it was. Without a re-offer, the datum is in no store.
#[tokio::test]
async fn space_store_handle_retained_produce_fire_leaves_incoming_unstored() {
    async fn run(handle: &Handle) -> Commits {
        let joined = waiter(&["b", "v1"], "kbv", true);
        let mut txn = handle.begin(&universe()).await.expect("begin");
        txn.produce(store_op("b", "e", false)).expect("stage b!e");
        txn.consume(park_op(
            &["b", "v1"],
            vec![Pat::Any, Pat::Any],
            "kbv",
            false,
            BTreeSet::from([0, 1]),
        ))
        .expect("park the peek join");
        let stored = txn.data(&s("b")).expect("read b")[0].clone();
        txn.produce(SpaceProduce {
            channel: s("v1"),
            data: s("r"),
            persist: false,
            outcome: ProduceOutcome::Fire {
                channels: names(&["b", "v1"]),
                continuation: joined,
                data: vec![
                    bind_candidate(s("b"), &stored, s("e"), 0),
                    incoming_candidate(
                        s("v1"),
                        produce_source("v1", "r", false),
                        s("r"),
                        false,
                        s("r"),
                    ),
                ],
            },
            before_observe: None,
        })
        .expect("stage the peek fire");
        assert_eq!(txn.data(&s("b")).expect("read b"), &[datum("b", "e", false)]);
        assert!(txn.data(&s("v1")).expect("read v1").is_empty());
        assert!(
            txn.continuations(&names(&["b", "v1"]))
                .expect("read [b, v1]")
                .is_empty()
        );
        vec![txn.commit().await]
    }

    let play = play_space().await;
    let start = checkpoint(&play).await;
    let played = receipts_ok(run(&SpaceStoreHandle::new(play.clone())).await);
    assert_eq!(played[0].ops.iter().map(result_flags).collect::<Vec<_>>(), vec![
        vec![],
        vec![],
        vec![true, false]
    ]);
    assert_eq!(play.get_data(&s("b")).await, vec![datum("b", "e", false)]);
    assert!(play.get_data(&s("v1")).await.is_empty());
    for channel in ["b", "v1"] {
        assert!(play.get_joins(s(channel)).await.is_empty());
    }
    let log = checkpoint(&play).await;
    assert_eq!(kinds(&log.log), vec!["produce", "consume", "produce", "comm"]);

    let replay = replay_over(&play);
    rig(&replay, &start.root, &log.log).await;
    let replayed = receipts_ok(run(&SpaceStoreHandle::new(replay.clone())).await);
    assert_eq!(replayed, played);
    assert_eq!(replay.get_data(&s("b")).await, vec![datum("b", "e", false)]);
    assert!(replay.get_data(&s("v1")).await.is_empty());
    replay
        .check_replay_data()
        .await
        .expect("replay pairs the peek COMM");
    assert_eq!(checkpoint(&replay).await.root, log.root);
}

// FR-10, the queue shape: a peek waiter W1, then a taking waiter W2, then a
// send of d. W1 fires, and the fire does not store d, not even in the
// staged view. The agent offers d again as the next produce of the
// transaction, with the same identity, and the re-offer fires W2. Nothing
// is stored, and the counter of d steps once per produce.
#[tokio::test]
async fn space_store_handle_peek_reoffer_fires_next_waiter() {
    let space = play_space().await;
    run_host_ops(&space, &[
        Op::peek(&["a"], vec![Pat::Any], "w1"),
        Op::consume(&["a"], vec![Pat::Any], "w2", false),
    ])
    .await;
    let w1 = waiter(&["a"], "w1", true);
    let w2 = waiter(&["a"], "w2", false);
    let d = produce_source("a", "d", false);
    let handle = SpaceStoreHandle::new(space.clone());
    let mut txn = handle.begin(&universe()).await.expect("begin");
    assert_eq!(txn.continuations(&names(&["a"])).expect("read [a]"), &[w2.clone(), w1.clone()]);

    let fire = txn
        .produce(fire_produce_op("a", "d", &w1))
        .expect("stage the peek fire");
    assert!(txn.data(&s("a")).expect("read a").is_empty());
    assert_eq!(txn.continuations(&names(&["a"])).expect("read [a]"), std::slice::from_ref(&w2));
    assert_eq!(txn.joins(&s("a")).expect("read the joins of a"), &[names(&["a"])]);
    let reoffer = txn
        .produce(fire_produce_op("a", "d", &w2))
        .expect("stage the re-offer");
    assert_eq!(fire.source, OpSource::Produce(d.clone()));
    assert_eq!(reoffer.source, fire.source);
    assert_eq!(reoffer.index, fire.index + 1);
    assert!(txn.data(&s("a")).expect("read a").is_empty());
    assert!(
        txn.continuations(&names(&["a"]))
            .expect("read [a]")
            .is_empty()
    );
    assert!(txn.joins(&s("a")).expect("read the joins of a").is_empty());
    assert_eq!(
        txn.produce_counters(std::slice::from_ref(&d)),
        Ok(BTreeMap::from([(d.clone(), 2)]))
    );
    let (Some(peek_comm), Some(take_comm)) = (fire.comm, reoffer.comm) else {
        panic!("both produces fire");
    };
    assert_eq!(peek_comm.consume, w1.source);
    assert_eq!(peek_comm.peeks, BTreeSet::from([0]));
    assert_eq!(peek_comm.times_repeated, BTreeMap::from([(d.clone(), 1)]));
    assert_eq!(take_comm.consume, w2.source);
    assert!(take_comm.peeks.is_empty());
    assert_eq!(take_comm.times_repeated, BTreeMap::from([(d.clone(), 2)]));

    let receipt = txn.commit().await.expect("commit");
    match &receipt.ops[..] {
        [
            SpaceOpReceipt::Fired {
                comm: first,
                produce: first_produce,
                ..
            },
            SpaceOpReceipt::Fired {
                comm: second,
                produce: second_produce,
                ..
            },
        ] => {
            assert_eq!((first, second), (&peek_comm, &take_comm));
            assert_eq!(first_produce, &Some(d.clone()));
            assert_eq!(second_produce, &Some(d.clone()));
        }
        other => panic!("expected two fires, got {:?}", other),
    }
    assert_eq!(receipt.ops.iter().map(result_flags).collect::<Vec<_>>(), vec![vec![false], vec![
        false
    ]]);
    assert!(space.get_data(&s("a")).await.is_empty());
    assert!(
        space
            .get_waiting_continuations(names(&["a"]))
            .await
            .is_empty()
    );
    assert!(space.get_joins(s("a")).await.is_empty());
    let mut probe = handle.begin(&universe()).await.expect("begin");
    assert_eq!(
        probe.produce_counters(std::slice::from_ref(&d)),
        Ok(BTreeMap::from([(d.clone(), 2)]))
    );
    drop(probe);
    assert_eq!(space.take_event_log().await, vec![
        Event::IoEvent(IOEvent::Consume(w1.source.clone())),
        Event::IoEvent(IOEvent::Consume(w2.source.clone())),
        produced("a", "d"),
        Event::Comm(peek_comm),
        produced("a", "d"),
        Event::Comm(take_comm),
    ]);
}

// FR-10: with no second waiter, the re-offer stores d. The space then holds
// one copy of d, as HEAD does after its re-produce, with HEAD's observer
// calls, events and root. The counter of d stepped once per produce: a
// later consume records 2.
#[tokio::test]
async fn space_store_handle_peek_reoffer_stores_without_waiter() {
    let twin = twin().await;
    let d = produce_source("a", "d", false);
    let groups =
        vec![vec![Op::peek(&["a"], vec![Pat::Any], "w1")], vec![Op::produce("a", "d", false)]];
    let (plans, receipts) = run_guided(&twin.host, &twin.txn, &groups).await;
    match &plans[1][..] {
        [
            Planned::Produce {
                outcome: ProduceOutcome::Fire { continuation, .. },
                ..
            },
            Planned::Produce {
                channel,
                data,
                persist: false,
                outcome: ProduceOutcome::Store,
            },
        ] => {
            assert_eq!(continuation, &waiter(&["a"], "w1", true));
            assert_eq!((channel.as_str(), data.as_str()), ("a", "d"));
        }
        other => panic!("expected the peek fire and the re-offer, got {:?}", other),
    }
    let receipts = receipts_ok(receipts);
    match &receipts[1].ops[..] {
        [fire @ SpaceOpReceipt::Fired { comm, .. }, SpaceOpReceipt::Stored { produce }] => {
            assert_eq!(result_flags(fire), vec![false]);
            assert_eq!(comm.times_repeated, BTreeMap::from([(d.clone(), 1)]));
            assert_eq!(produce, &d);
        }
        other => panic!("expected the peek fire and the stored re-offer, got {:?}", other),
    }
    assert_eq!(twin.txn.get_data(&s("a")).await, vec![datum("a", "d", false)]);
    assert!(
        twin.txn
            .get_waiting_continuations(names(&["a"]))
            .await
            .is_empty()
    );
    assert_eq!(logical_state(&twin.txn), logical_state(&twin.host));

    let (_, consumed) = run_guided(&twin.host, &twin.txn, &[vec![Op::consume(
        &["a"],
        vec![Pat::Any],
        "w3",
        false,
    )]])
    .await;
    match &receipts_ok(consumed)[0].ops[..] {
        [SpaceOpReceipt::Fired { comm, .. }] => {
            assert_eq!(comm.times_repeated, BTreeMap::from([(d.clone(), 2)]));
        }
        other => panic!("expected the consume of the re-offered d, got {:?}", other),
    }
    let (_, txn) = assert_parity(&twin).await;
    assert_eq!(kinds(&txn.log), vec!["consume", "produce", "comm", "produce", "consume", "comm"]);
}

// FR-10, host parity of the re-offer: HEAD's produce fires the peek waiter,
// and the reducer produces d again (`produce_peeks`), which fires the taking
// waiter. The space stages both host outcomes in one transaction. Observer
// calls, events, contents and root equal HEAD's. Each produce runs its
// introduction callback with the one produce identity immediately before
// its start, and the registry accepts the second registration of that
// identity with an equal authority.
#[tokio::test]
async fn space_store_handle_peek_reoffer_equals_head_reproduce() {
    let peeking = peek_tried_first("a", "take");
    let parks = vec![
        Op::peek(&["a"], vec![Pat::Any], &peeking),
        Op::consume(&["a"], vec![Pat::Any], "take", false),
    ];
    let send = Op::produce("a", "d", false);
    let d = produce_source("a", "d", false);
    let twin = twin().await;
    run_host_ops(&twin.host, &parks).await;
    let fired = run_host(&twin.host, &send).await.expect("the host produce");
    let reproduced = run_host(&twin.host, &send)
        .await
        .expect("HEAD's re-produce");
    match (&fired, &reproduced) {
        (
            HostOutcome::Produced(Some((first, _, _))),
            HostOutcome::Produced(Some((second, _, _))),
        ) => {
            assert!(first.peek);
            assert_eq!(first.continuation, peeking);
            assert!(!second.peek);
            assert_eq!(second.continuation, "take");
        }
        other => {
            panic!("expected HEAD to fire the peek waiter, then the taking waiter, got {:?}", other)
        }
    }

    run_host_ops(&twin.txn, &parks).await;
    let registry = Arc::new(AuthorityRegistry::default());
    let handle = SpaceStoreHandle::new(twin.txn.clone());
    let mut txn = handle.begin(&universe()).await.expect("begin");
    for outcome in [&fired, &reproduced] {
        let plan = plan_from_host(&mut txn, &send, outcome);
        plan.stage(&mut txn, Some(registry.register(&twin.txn_recorder, "alpha")))
            .expect("stage the fire and the re-offer");
    }
    let receipt = txn.commit().await.expect("commit");
    assert_eq!(receipt.ops.iter().map(result_flags).collect::<Vec<_>>(), vec![vec![false], vec![
        false
    ]]);

    let identity = source_bytes(RSpaceOperationSource::Produce(&d));
    assert_eq!(registry.entries(), BTreeMap::from([(identity.clone(), s("alpha"))]));
    let calls = twin.txn_recorder.calls();
    let callbacks: Vec<usize> = calls
        .iter()
        .enumerate()
        .filter(|(_, call)| matches!(call, Call::BeforeObserve { .. }))
        .map(|(index, _)| index)
        .collect();
    assert_eq!(callbacks.len(), 2);
    for index in callbacks {
        assert_eq!(calls[index], Call::BeforeObserve {
            source: identity.clone()
        });
        match &calls[index + 1] {
            Call::Start { source, .. } => assert_eq!(source, &identity),
            other => panic!("a callback must precede the start, got {:?}", other),
        }
    }
    let without_callbacks: Vec<Call> = calls
        .into_iter()
        .filter(|call| !matches!(call, Call::BeforeObserve { .. }))
        .collect();
    assert_eq!(without_callbacks, twin.host_recorder.calls());

    let state = logical_state(&twin.txn);
    assert_eq!(state, logical_state(&twin.host));
    assert!(state.data.is_empty() && state.continuations.is_empty() && state.joins.is_empty());
    let groups = groups_of(&[&twin.host, &twin.txn]).await;
    assert_eq!(
        channel_view(&twin.txn, &universe(), &groups).await,
        channel_view(&twin.host, &universe(), &groups).await
    );
    let host = checkpoint(&twin.host).await;
    let txn = checkpoint(&twin.txn).await;
    assert_eq!(event_bytes(&txn.log), event_bytes(&host.log));
    assert_eq!(kinds(&txn.log), vec!["consume", "consume", "produce", "comm", "produce", "comm"]);
    assert_eq!(txn.root, host.root);
}

// FR-10: the re-offer registers its introduction authority under the
// identity that the peek fire registered. The registry refuses another
// authority for that identity, so the commit fails at the re-offer before
// its start, and the peek fire stays applied, as when HEAD's re-produce
// fails its registration (prefix semantics, decision 19 D6). The datum is
// then in no store.
#[tokio::test]
async fn space_store_handle_peek_reoffer_authority_conflict_fails_at_reoffer() {
    let space = play_space().await;
    let recorder = Recorder::new();
    observed(&space, &recorder);
    run_host_ops(&space, &[Op::peek(&["a"], vec![Pat::Any], "w1")]).await;
    let registry = Arc::new(AuthorityRegistry::default());
    let handle = SpaceStoreHandle::new(space.clone());
    let before = recorder.calls().len();
    let mut txn = handle.begin(&universe()).await.expect("begin");
    let mut fire = fire_produce_op("a", "d", &waiter(&["a"], "w1", true));
    fire.before_observe = Some(registry.register(&recorder, "alpha"));
    txn.produce(fire).expect("stage the peek fire");
    let mut reoffer = store_op("a", "d", false);
    reoffer.before_observe = Some(registry.register(&recorder, "beta"));
    txn.produce(reoffer).expect("stage the re-offer");
    assert_eq!(
        txn.commit().await,
        Err(SpaceTxnError::Host(RSpaceError::InterpreterError(
            "event identity conflict".to_string()
        )))
    );
    let calls: Vec<&'static str> = recorder.calls()[before..].iter().map(call_kind).collect();
    assert_eq!(calls, vec!["before", "start", "produce", "comm", "finish", "before"]);
    assert_eq!(registry.entries().len(), 1);
    assert!(space.get_data(&s("a")).await.is_empty());
    assert!(
        space
            .get_waiting_continuations(names(&["a"]))
            .await
            .is_empty()
    );
    assert_eq!(kinds(&space.take_event_log().await), vec!["consume", "produce", "comm"]);
}

// FR-10 in replay and reporting: transactions of a peek fire and its
// re-offer replay their play log, in replay and in reporting, with the play
// receipts and root. While the peek fire is staged, the recorded COMMs of
// the produce identity offer only the COMM of the re-offer. Host replay of
// the same log with HEAD's operations (the re-produces included) reaches
// the same root, and HEAD's play log replays through the same transactions.
#[tokio::test]
async fn space_store_handle_peek_reoffer_replay_and_report() {
    let peeking = peek_tried_first("a", "take");
    let peek = Op::peek(&["a"], vec![Pat::Any], &peeking);
    let take = Op::consume(&["a"], vec![Pat::Any], "take", false);
    let groups = vec![
        vec![peek.clone(), take.clone()],
        vec![Op::produce("a", "d", false)],
        vec![peek.clone()],
        vec![Op::produce("a", "e", false)],
    ];
    let head_ops = vec![
        peek.clone(),
        take,
        Op::produce("a", "d", false),
        Op::produce("a", "d", false),
        peek,
        Op::produce("a", "e", false),
        Op::produce("a", "e", false),
    ];
    let plans = plan_groups(&groups).await;
    assert_eq!(plans.iter().map(Vec::len).collect::<Vec<_>>(), vec![2, 2, 1, 2]);
    let (play, replay) = play_and_replay().await;
    let start = checkpoint(&play).await;
    let played_receipts =
        receipts_ok(run_plans(&SpaceStoreHandle::new(play.clone()), &plans).await);
    assert_eq!(play.get_data(&s("a")).await, vec![datum("a", "e", false)]);
    let played = checkpoint(&play).await;
    let comms = comms_of(&played.log);
    assert_eq!(comms.len(), 3);
    assert_eq!(kinds(&played.log), vec![
        "consume", "consume", "produce", "comm", "produce", "comm", "consume", "produce", "comm",
        "produce"
    ]);

    rig(&replay, &start.root, &played.log).await;
    let handle = SpaceStoreHandle::new(replay.clone());
    let mut replayed = run_plans(&handle, &plans[..1]).await;
    let d = produce_source("a", "d", false);
    let mut txn = handle.begin(&universe()).await.expect("begin");
    let recorded = |txn: &mut Txn| {
        txn.recorded_comms(RSpaceOperationSource::Produce(&d))
            .expect("recorded COMMs in replay")
    };
    assert_eq!(recorded(&mut txn).len(), 2);
    plans[1][0]
        .stage_expected(&mut txn)
        .expect("stage the peek fire");
    assert_eq!(recorded(&mut txn), vec![comms[1].clone()]);
    plans[1][1]
        .stage_expected(&mut txn)
        .expect("stage the re-offer");
    assert!(recorded(&mut txn).is_empty());
    replayed.push(txn.commit().await);
    replayed.extend(run_plans(&handle, &plans[2..]).await);
    assert_eq!(receipts_ok(replayed), played_receipts);
    replay
        .check_replay_data()
        .await
        .expect("replay pairs every COMM");
    assert_eq!(checkpoint(&replay).await.root, played.root);

    let reporting = reporting_over(&play);
    reporting
        .rig_and_reset(start.root.clone(), played.log.clone())
        .await
        .expect("rig the reporting space");
    let reported = receipts_ok(run_plans(&SpaceStoreHandle::new(reporting.clone()), &plans).await);
    assert_eq!(reported, played_receipts);
    reporting
        .check_replay_data()
        .await
        .expect("reporting pairs every COMM");
    let reported_comms = reporting
        .get_report()
        .expect("the report")
        .iter()
        .flat_map(|batch| batch.events.iter())
        .filter(|event| matches!(event, ReportingEvent::ReportingComm(_)))
        .count();
    assert_eq!(reported_comms, 3);
    let reported_root = ISpace::create_checkpoint(&reporting)
        .await
        .expect("checkpoint the reporting space")
        .root;
    assert_eq!(reported_root, played.root);

    let host_replay = replay_over(&play);
    rig(&host_replay, &start.root, &played.log).await;
    run_host_ops(&host_replay, &head_ops).await;
    host_replay
        .check_replay_data()
        .await
        .expect("host replay pairs every COMM of the transaction log");
    assert_eq!(checkpoint(&host_replay).await.root, played.root);

    let (head_play, txn_replay) = play_and_replay().await;
    let head_start = checkpoint(&head_play).await;
    run_host_ops(&head_play, &head_ops).await;
    let head_played = checkpoint(&head_play).await;
    assert_eq!(event_bytes(&head_played.log), event_bytes(&played.log));
    assert_eq!(head_played.root, played.root);
    rig(&txn_replay, &head_start.root, &head_played.log).await;
    let txn_replayed =
        receipts_ok(run_plans(&SpaceStoreHandle::new(txn_replay.clone()), &plans).await);
    assert_eq!(txn_replayed, played_receipts);
    txn_replay
        .check_replay_data()
        .await
        .expect("transaction replay pairs every COMM of HEAD's log");
    assert_eq!(checkpoint(&txn_replay).await.root, played.root);
}

// FR-10 receipt flags: a peek fire reports its retained stored records as
// persistent. The incoming datum of a produce-side peek fire keeps its own
// flag: false for a non-persistent send, which the agent offers again, and
// true for a persistent send. A fire that does not peek reports the own
// flag of every record. Staged from HEAD's outcomes, the space keeps host
// parity throughout.
#[tokio::test]
async fn space_store_handle_peek_fire_receipt_flags() {
    let twin = twin().await;
    let groups = vec![
        vec![
            Op::produce("a", "x", false),
            Op::produce("b", "e", false),
            Op::produce("c", "p", true),
        ],
        vec![Op::peek(&["a"], vec![Pat::Any], "ka")],
        vec![Op::peek(&["d"], vec![Pat::Any], "kd")],
        vec![Op::produce("d", "q", false)],
        vec![Op::peek(&["b", "v1"], vec![Pat::Any, Pat::Any], "kbv")],
        vec![Op::produce("v1", "r", false)],
        vec![Op::peek(&["v2"], vec![Pat::Any], "kv")],
        vec![Op::produce("v2", "s", true)],
        vec![Op::consume(&["c"], vec![Pat::Any], "kc", false)],
        vec![Op::consume(&["d"], vec![Pat::Any], "kd2", false)],
    ];
    let (_, receipts) = run_guided(&twin.host, &twin.txn, &groups).await;
    let flags: Vec<Vec<Vec<bool>>> = receipts_ok(receipts)
        .iter()
        .map(|receipt| receipt.ops.iter().map(result_flags).collect())
        .collect();
    assert_eq!(flags, vec![
        vec![vec![], vec![], vec![]],
        // A consume-side peek retains the stored x.
        vec![vec![true]],
        vec![vec![]],
        // A produce-side peek: the incoming q keeps false; its re-offer
        // stores q.
        vec![vec![false], vec![]],
        vec![vec![]],
        // A peek join: the stored e is retained, the incoming r keeps
        // false; its re-offer stores r.
        vec![vec![true, false], vec![]],
        vec![vec![]],
        // A persistent send keeps its own flag, true, and is not stored.
        vec![vec![true]],
        // Fires that do not peek keep the own flags.
        vec![vec![true]],
        vec![vec![false]],
    ]);
    let state = logical_state(&twin.txn);
    assert_eq!(state, logical_state(&twin.host));
    let stored = |channel: &str, data: &str, persist: bool| {
        (s(channel), vec![bincode::serialize(&datum(channel, data, persist)).expect("bytes")])
    };
    assert_eq!(
        state.data,
        BTreeMap::from([
            stored("a", "x", false),
            stored("b", "e", false),
            stored("c", "p", true),
            stored("v1", "r", false),
        ])
    );
    assert!(state.continuations.is_empty());
    assert!(state.joins.is_empty());
    assert_parity(&twin).await;
}
