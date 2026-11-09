// Reified RSpaces storage substrate (`SpaceStoreHandle` / `SpaceTxn`):
//   A — host parity of every operation shape (22 tests)
//   B — checkpoints, history read-through, and an LMDB round trip (5 tests)
// Changed by decision 90 (C6, step D): the rigged replay and reporting modes
// retired with their tests, so these counts no longer hold. The text before:
// ```
// //   C — replay lookup parity (11 tests), with decision 36: replay
// //       reproduces a play that an observer call rejects
// //   D — volatile sweep (7 tests)
// //   E — reporting delegation (3 tests)
// ```
//   C — replay lookup parity: retired by decision 90 (C6, step D), except
//       the host replay of the play log of a transaction
//   D — volatile sweep
//   E — reporting delegation: retired by decision 90 (C6, step D)
//   F — review regressions and rule coverage: canonical channel order,
//       store identity, trigger-key pairing, lock coverage, one commit per
//       parent order, nested begin, sweep bookkeeping, peeks in replay
//   G — round-2 rules: repeated channels in sorted groups (FR-1'), parent
//       tracking per root evaluation (FR-5'), checks that cost O(changed
//       entries) (FR-7'), and the sweep's nested-transaction guard (FR-9')
//   H — round-3 rule FR-10 with decision 45 (Rule E): a peek fire removes
//       its stored non-persistent records as the host does and never stores
//       the incoming datum, the agent stores each removed record again (the
//       re-store) and offers a non-persistent incoming datum again (the
//       re-offer) as next produces of the transaction, and every result of
//       a fire keeps the host's flag
// The parity oracle runs each operation on a host `RSpace` and stages the
// same outcome on a twin through a transaction, then compares observer
// calls, events, store contents and state roots. After a peek fire the
// oracle follows HEAD's peek path (`head_peek_path` in support.rs): HEAD's
// re-produce of each non-persistent result (`produce_peeks`) is the agent's
// next produce, the re-store of a removed record (decision 45, Rule E) or
// the re-offer of the incoming datum (FR-10). Every channel group is
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
// Retired by decision 90 (C6, step D): the tests of the rigged reporting mode
// were its only users (decision 125). The text before:
// use rspace_plus_plus::rspace::reporting_rspace::ReportingEvent;
use rspace_plus_plus::rspace::rspace::RSpace;
// Changed by decision 124 (plan of the batch B tests, B19, ruling RADD2v2
// 6.6 (f), U5): the funded observer of U5 implements the observer trait. The
// text before:
// use rspace_plus_plus::rspace::rspace_interface::{
//     ISpace, RSpaceOperationCompletion, RSpaceOperationSource,
//     ReplayOperationDirective,
// };
use rspace_plus_plus::rspace::rspace_interface::{
    ISpace, RSpaceAccountingObserver, RSpaceOperationCompletion, RSpaceOperationSource,
};
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use rspace_plus_plus::rspace::shared::lmdb_dir_store_manager::MB;
use rspace_plus_plus::rspace::shared::rspace_store_manager::mk_rspace_store_manager;
// Changed by decision 124 (plan of the batch B tests, B19, ruling RADD2v2
// 6.6 (f), U5): the scripted search of U5 names the parts of a row search.
// The text before:
// use rspace_plus_plus::rspace::space_store_handle::{
//     BeforeObserve, ConsumeOutcome, OpSource, ProduceOutcome, SpaceConsume,
//     SpaceMode, SpaceOpReceipt, SpaceProduce, SpaceStoreHandle,
//     SpaceTxnError, SweepReport, bind_candidate, incoming_candidate,
//     sub_order,
// };
use rspace_plus_plus::rspace::space_store_handle::{
    ActualFire, BeforeObserve, ConsumeOutcome, FinishRow, OpSource, ProduceOutcome, RowClaims,
    RowItemRule, RowLists, RowRun, RowSearch, RowTupleRequest, SpaceConsume, SpaceMode,
    SpaceOpReceipt, SpaceProduce, SpaceStoreHandle, SpaceTxnError, SweepReport, bind_candidate,
    incoming_candidate, sub_order,
};
// Changed by decision 124 (plan of the batch B tests, B19, ruling RADD2v2
// 6.6 (f), U5): the funded observer of U5 names `COMM`. The text before:
// use rspace_plus_plus::rspace::trace::event::{
//     Consume, Event, IOEvent, Produce,
// };
use rspace_plus_plus::rspace::trace::event::{COMM, Consume, Event, IOEvent, Produce};
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
        own: false,
        // Added by decision 123 (plan v4, V3-9): no row search.
        search: None,
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
        own: false,
        // Added by decision 123 (plan v4, V3-9): no row search.
        search: None,
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
        own: false,
        // Added by decision 123 (plan v4, V3-9): no row search.
        search: None,
    }
}

// A consume that peeks every bind and fires with `candidates`, which bind
// stored records only.
fn peek_consume_op(
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
        peeks: (0..channels.len() as i32).collect(),
        outcome: ConsumeOutcome::Fire { data: candidates },
        before_observe: None,
        own: false,
        // Added by decision 123 (plan v4, V3-9): no row search.
        search: None,
    }
}

// The produce counters of `sources` in the space behind `handle`, read in a
// transaction that stages nothing.
async fn counters_at(handle: &Handle, sources: &[Produce]) -> BTreeMap<Produce, i32> {
    let mut probe = handle.begin(&universe()).await.expect("begin a probe");
    probe
        .produce_counters(sources)
        .expect("read the produce counters")
}

fn single(groups: Vec<Op>) -> Vec<Vec<Op>> { vec![groups] }

// Retired by decision 90 (C6, step D): only the retired tests of the rigged
// replay modes called this helper (decision 125). The text before:
// ```text
// fn each(ops: Vec<Op>) -> Vec<Vec<Op>> { ops.into_iter().map(|op| vec![op]).collect() }
// ```

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

// HEAD's peek path (reduce.rs:776-811, 909-936 and `produce_peeks`,
// 981-1016): a peek fire removes its bound non-persistent records in the
// host, and the reducer produces each one again. The consume-side peek on `a`
// removes the stored x in the space as well, and the agent stores x again
// right after the fire (the re-store, decision 45, Rule E). The produce-side
// peek on `b` does not store `y`, and the agent offers `y` again as the next
// produce, which stores it (the re-offer, FR-10). Both re-produces have
// HEAD's events, observer calls and produce counters, and every result keeps
// the host's flag.
#[tokio::test]
async fn space_store_handle_peek_events_equal_host_and_restores() {
    let twin = twin().await;
    let groups = vec![
        vec![Op::produce("a", "x", false)],
        vec![Op::peek(&["a"], vec![Pat::Any], "k")],
        vec![Op::peek(&["b"], vec![Pat::Any], "k2")],
        vec![Op::produce("b", "y", false)],
    ];
    let (plans, receipts) = run_guided(&twin.host, &twin.txn, &groups).await;
    let receipts = receipts_ok(receipts);
    assert_eq!(plans.iter().map(Vec::len).collect::<Vec<_>>(), vec![1, 2, 1, 2]);
    let x = produce_source("a", "x", false);
    let y = produce_source("b", "y", false);
    match (&receipts[1].ops[..], &receipts[3].ops[..]) {
        (
            [
                SpaceOpReceipt::Fired {
                    results: peeked,
                    comm: consume_comm,
                    produce: None,
                    ..
                },
                SpaceOpReceipt::Stored { produce: restored },
            ],
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
            assert_eq!(peeked.len(), 1);
            assert!(!peeked[0].persistent);
            assert_eq!(consume_comm.peeks, BTreeSet::from([0]));
            assert_eq!(consume_comm.times_repeated, BTreeMap::from([(x.clone(), 1)]));
            assert_eq!(restored, &x);
            assert_eq!(produce_comm.peeks, BTreeSet::from([0]));
            assert_eq!(sent.len(), 1);
            assert!(!sent[0].persistent);
            assert_eq!(produce, &Some(y.clone()));
            assert_eq!(reoffered, &y);
        }
        other => {
            panic!("expected the peek fires with the re-store and the re-offer, got {:?}", other)
        }
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
    let sources = [x.clone(), y.clone()];
    let counters = counters_at(&SpaceStoreHandle::new(twin.txn.clone()), &sources).await;
    assert_eq!(counters, counters_at(&SpaceStoreHandle::new(twin.host.clone()), &sources).await);
    assert_eq!(counters, BTreeMap::from([(x, 2), (y, 2)]));
    let (_, txn) = assert_parity(&twin).await;
    assert_eq!(kinds(&txn.log), vec![
        "produce", "consume", "comm", "produce", "consume", "produce", "comm", "produce"
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
        own: false,
        // Added by decision 123 (plan v4, V3-9): no row search.
        search: None,
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
        own: false,
        // Added by decision 123 (plan v4, V3-9): no row search.
        search: None,
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
        own: false,
        // Added by decision 123 (plan v4, V3-9): no row search.
        search: None,
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

/// Map size of each LMDB environment of the round-trip test.
const LMDB_MAP_SIZE: usize = 64 * MB;

/// What a checkpoint persists, read through the substrate: the history root,
/// the universe through the `ISpace` getters and through a transaction view,
/// and the record count of each channel's leaf at the root.
#[derive(Debug, PartialEq, Eq)]
struct PersistedState {
    root: Blake2b256Hash,
    getters: ChannelView,
    txn_view: ChannelView,
    root_leaf_counts: Vec<u64>,
}

async fn persisted_state(space: &Space, handle: &Handle) -> PersistedState {
    let channels = universe();
    let groups = groups_of(&[space]).await;
    let getters = channel_view(space, &channels, &groups).await;
    let mut txn = handle
        .begin(&channels)
        .await
        .expect("begin a read transaction");
    let mut data = Vec::with_capacity(channels.len());
    let mut joins = Vec::with_capacity(channels.len());
    for channel in &channels {
        data.push(
            txn.data(channel)
                .expect("read data in the transaction view")
                .iter()
                .map(|record| bincode::serialize(record).expect("serialize a datum"))
                .collect(),
        );
        joins.push(
            txn.joins(channel)
                .expect("read joins in the transaction view")
                .to_vec(),
        );
    }
    let mut continuations = Vec::with_capacity(groups.len());
    for group in &groups {
        continuations.push(
            txn.continuations(group)
                .expect("read continuations in the transaction view")
                .iter()
                .map(|wk| bincode::serialize(wk).expect("serialize a continuation"))
                .collect(),
        );
    }
    drop(txn);
    let root_leaf_counts = channels
        .iter()
        .map(|channel| {
            handle
                .root_leaf_record_count(channel)
                .expect("count the root leaf")
        })
        .collect();
    PersistedState {
        root: space.get_history_repository().root(),
        getters,
        txn_view: (data, continuations, joins),
        root_leaf_counts,
    }
}

// A checkpoint of an LMDB-backed space survives the close of its directory:
// after every space, store and environment handle is dropped, a new store
// manager and a new space over the same directory, reset to the checkpoint
// root, read the same data, continuations, joins, root and root leaf record
// counts. The directory honors TMPDIR.
#[tokio::test]
async fn space_store_handle_lmdb_checkpoint_round_trip() {
    let directory = tempfile::Builder::new()
        .prefix("rrs-lmdb-checkpoint-")
        .tempdir()
        .expect("create the LMDB directory");
    let join = names(&["d", "c"]);

    let mut stores = mk_rspace_store_manager(directory.path().to_path_buf(), LMDB_MAP_SIZE);
    let space: Space = RSpace::create(
        stores
            .r_space_stores()
            .await
            .expect("open the LMDB RSpace stores"),
        Arc::new(Box::new(PatMatch)),
    )
    .expect("create the LMDB RSpace");
    let empty_root = space.get_history_repository().root();
    let handle = SpaceStoreHandle::new(space.clone());
    let mut txn = handle.begin(&universe()).await.expect("begin");
    txn.produce(store_op("a", "x", false))
        .expect("stage the datum");
    txn.consume(park_op(&["d", "c"], vec![Pat::Any, Pat::Any], "k", false, BTreeSet::new()))
        .expect("stage the join");
    txn.commit().await.expect("commit");
    let persisted = checkpoint(&space).await;
    assert_ne!(persisted.root, empty_root, "the checkpoint persists the commit");
    assert_eq!(space.get_data(&s("a")).await, vec![datum("a", "x", false)]);
    assert_eq!(space.get_waiting_continuations(join.clone()).await.len(), 1);
    for channel in ["c", "d"] {
        assert_eq!(space.get_joins(s(channel)).await, vec![join.clone()]);
    }
    assert_eq!(handle.root_leaf_record_count(&s("a")), Ok(1));
    let before = persisted_state(&space, &handle).await;
    let released = Arc::downgrade(&space.get_history_repository());
    drop(txn);
    drop(handle);
    drop(space);
    drop(stores);
    assert!(
        released.upgrade().is_none(),
        "dropping the space and its handle releases the history repository"
    );

    let mut stores = mk_rspace_store_manager(directory.path().to_path_buf(), LMDB_MAP_SIZE);
    let reopened: Space = RSpace::create(
        stores
            .r_space_stores()
            .await
            .expect("reopen the LMDB RSpace stores"),
        Arc::new(Box::new(PatMatch)),
    )
    .expect("create the reopened RSpace");
    assert_eq!(
        reopened.get_history_repository().root(),
        persisted.root,
        "the roots store keeps the checkpoint root"
    );
    reopened
        .reset(&persisted.root)
        .await
        .expect("reset to the checkpoint root");
    let handle = SpaceStoreHandle::new(reopened.clone());
    assert_eq!(persisted_state(&reopened, &handle).await, before);
    drop(handle);
    drop(reopened);
    drop(stores);
    directory.close().expect("remove the LMDB directory");
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

// Retired by decision 90 (C6, step D): the test drove the rigged replay or
// reporting mode of the store, which retires (decision 125). Native replay of
// the store is tested by `native_space_replay_rejects_a_tampered_candidate` and
// its siblings in `native_session/space.rs`, and by
// `space_native_replay_equal_*`. The text before:
// ```
// #[tokio::test]
// async fn space_store_handle_replay_pairs_play_log() {
//     let groups = replay_groups();
//     let plans = plan_groups(&groups).await;
//     let (play, replay) = play_and_replay().await;
//     let start = checkpoint(&play).await;
//     let played_receipts =
//         receipts_ok(run_plans(&SpaceStoreHandle::new(play.clone()), &plans).await);
//     let played = checkpoint(&play).await;
//     assert!(!comms_of(&played.log).is_empty());
//
//     rig(&replay, &start.root, &played.log).await;
//     let handle = SpaceStoreHandle::new(replay.clone());
//     assert_eq!(handle.mode(), SpaceMode::Replay);
//     let replayed_receipts = receipts_ok(run_plans(&handle, &plans).await);
//     assert_eq!(replayed_receipts, played_receipts);
//     assert!(replay_bindings(&replay).is_empty());
//     let replayed = checkpoint(&replay).await;
//     assert_eq!(replayed.root, played.root);
// }
// ```

// Retired by decision 90 (C6, step D): the test drove the rigged replay or
// reporting mode of the store, which retires (decision 125). Native replay of
// the store is tested by `native_space_replay_rejects_a_tampered_candidate` and
// its siblings in `native_session/space.rs`, and by
// `space_native_replay_equal_*`. The text before:
// ```
// #[tokio::test]
// async fn space_store_handle_replay_observer_calls_equal_host_replay() {
//     let groups = replay_groups();
//     let plans = plan_groups(&groups).await;
//     let (play, host_replay) = play_and_replay().await;
//     let start = checkpoint(&play).await;
//     run_host_ops(&play, &flatten(&groups)).await;
//     let played = checkpoint(&play).await;
//     let txn_replay = replay_over(&play);
//
//     let host_recorder = Recorder::new();
//     observed(&host_replay, &host_recorder);
//     rig(&host_replay, &start.root, &played.log).await;
//     run_host_ops(&host_replay, &flatten(&groups)).await;
//     let host_replayed = checkpoint(&host_replay).await;
//
//     let txn_recorder = Recorder::new();
//     observed(&txn_replay, &txn_recorder);
//     rig(&txn_replay, &start.root, &played.log).await;
//     receipts_ok(run_plans(&SpaceStoreHandle::new(txn_replay.clone()), &plans).await);
//     let txn_replayed = checkpoint(&txn_replay).await;
//
//     let calls = txn_recorder.calls();
//     assert!(
//         calls
//             .iter()
//             .any(|call| matches!(call, Call::Directive { .. }))
//     );
//     assert_eq!(calls, host_recorder.calls());
//     assert_eq!(txn_replayed.root, host_replayed.root);
//     assert_eq!(txn_replayed.root, played.root);
// }
// ```

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

// Retired by decision 90 (C6, step D): the test drove the rigged replay or
// reporting mode of the store, which retires (decision 125). Native replay of
// the store is tested by `native_space_replay_rejects_a_tampered_candidate` and
// its siblings in `native_session/space.rs`, and by
// `space_native_replay_equal_*`. The text before:
// ```
// #[tokio::test]
// async fn space_store_handle_txn_replay_accepts_host_play_log() {
//     let groups = replay_groups();
//     let plans = plan_groups(&groups).await;
//     let (play, replay) = play_and_replay().await;
//     let start = checkpoint(&play).await;
//     run_host_ops(&play, &flatten(&groups)).await;
//     let played = checkpoint(&play).await;
//     rig(&replay, &start.root, &played.log).await;
//     receipts_ok(run_plans(&SpaceStoreHandle::new(replay.clone()), &plans).await);
//     replay
//         .check_replay_data()
//         .await
//         .expect("the transaction replay pairs every COMM");
//     assert_eq!(checkpoint(&replay).await.root, played.root);
// }
// ```

// Retired by decision 90 (C6, step D): the test drove the rigged replay or
// reporting mode of the store, which retires (decision 125). Native replay of
// the store is tested by `native_space_replay_rejects_a_tampered_candidate` and
// its siblings in `native_session/space.rs`, and by
// `space_native_replay_equal_*`. The text before:
// ```text
// // H4 (decision 36): a replay fire whose COMM the log does not hold stages
// // as in play and stays unpaired. Staging makes no observer call and changes
// // no binding and no store entry, and the staged view shows the effects of
// // play. The commit applies the park before the fire, makes the observer
// // calls of the fire, and fails with the typed `ReplayDivergence`. The fire
// // and the store after it log, apply and unbind nothing, and the counter of
// // the fire rolls back. A new transaction with the recorded fire then
// // reaches the play root.
// #[tokio::test]
// async fn space_store_handle_replay_divergence_is_typed_at_commit() {
//     let groups =
//         each(vec![Op::consume(&["a"], vec![Pat::Any], "k", false), Op::produce("a", "x", false)]);
//     let plans = plan_groups(&groups).await;
//     let (play, replay) = play_and_replay().await;
//     let start = checkpoint(&play).await;
//     receipts_ok(run_plans(&SpaceStoreHandle::new(play.clone()), &plans).await);
//     let played = checkpoint(&play).await;
//     let recorder = Recorder::new();
//     observed(&replay, &recorder);
//     rig(&replay, &start.root, &played.log).await;
//
//     let handle = SpaceStoreHandle::new(replay.clone());
//     let mut txn = handle.begin(&universe()).await.expect("begin");
//     plans[0][0].stage(&mut txn, None).expect("stage the park");
//     let bindings = replay_bindings(&replay);
//     let state = replay_state(&replay);
//     let divergent = match &plans[1][0] {
//         Planned::Produce {
//             outcome:
//                 ProduceOutcome::Fire {
//                     channels,
//                     continuation,
//                     ..
//                 },
//             ..
//         } => SpaceProduce {
//             channel: s("a"),
//             data: s("y"),
//             persist: false,
//             outcome: ProduceOutcome::Fire {
//                 channels: channels.clone(),
//                 continuation: continuation.clone(),
//                 data: vec![incoming_candidate(
//                     s("a"),
//                     produce_source("a", "y", false),
//                     s("y"),
//                     false,
//                     s("y"),
//                 )],
//             },
//             before_observe: None,
//         },
//         other => panic!("expected a planned fire, got {:?}", other),
//     };
//     let y = produce_source("a", "y", false);
//     let comm = txn
//         .produce(divergent)
//         .expect("an unpaired fire stages as in play")
//         .comm
//         .expect("the fire has a COMM");
//     assert_eq!(comm.produces, vec![y.clone()]);
//     txn.produce(store_op("b", "w", false))
//         .expect("stage a store after the fire");
//     assert_eq!(replay_bindings(&replay), bindings);
//     assert_eq!(replay_state(&replay), state);
//     assert!(recorder.calls().is_empty());
//     assert!(txn.data(&s("a")).expect("read a").is_empty());
//     assert!(
//         txn.continuations(&names(&["a"]))
//             .expect("read a")
//             .is_empty()
//     );
//     assert_eq!(
//         txn.produce_counters(std::slice::from_ref(&y)),
//         Ok(BTreeMap::from([(y.clone(), 1)]))
//     );
//
//     assert_eq!(txn.commit().await, Err(SpaceTxnError::ReplayDivergence(Box::new(comm.clone()))));
//     let calls = recorder.calls();
//     assert_eq!(calls.iter().map(call_kind).collect::<Vec<_>>(), vec![
//         "start",
//         "directive",
//         "consume",
//         "finish",
//         "start",
//         "directive",
//         "produce",
//         "comm",
//         "finish"
//     ]);
//     match &calls[7..] {
//         [Call::Comm { comm: observed, .. }, Call::Finish { completion, .. }] => {
//             assert_eq!(*observed, bincode::serialize(&comm).expect("serialize a COMM"));
//             assert_eq!(*completion, RSpaceOperationCompletion::Rejected);
//         }
//         other => panic!("expected the COMM call and the rejected finish, got {:?}", other),
//     }
//     assert_eq!(replay_bindings(&replay), bindings);
//     assert_eq!(replay.get_waiting_continuations(names(&["a"])).await.len(), 1);
//     assert!(replay.get_data(&s("a")).await.is_empty());
//     assert!(replay.get_data(&s("b")).await.is_empty());
//     let mut probe = handle.begin(&universe()).await.expect("begin");
//     assert_eq!(probe.produce_counters(std::slice::from_ref(&y)), Ok(BTreeMap::from([(y, 0)])));
//     drop(probe);
//
//     let mut txn = handle.begin(&universe()).await.expect("begin");
//     plans[1][0]
//         .stage(&mut txn, None)
//         .expect("stage the recorded fire");
//     txn.commit().await.expect("commit");
//     assert!(replay_bindings(&replay).is_empty());
//     assert_eq!(checkpoint(&replay).await.root, played.root);
// }
// ```

// Retired by decision 90 (C6, step D): the test drove the rigged replay or
// reporting mode of the store, which retires (decision 125). Native replay of
// the store is tested by `native_space_replay_rejects_a_tampered_candidate` and
// its siblings in `native_session/space.rs`, and by
// `space_native_replay_equal_*`. The text before:
// ```text
// #[tokio::test]
// async fn space_store_handle_replay_unpaired_comm_fails_check_replay_data() {
//     let groups =
//         each(vec![Op::consume(&["a"], vec![Pat::Any], "k", false), Op::produce("a", "x", false)]);
//     let plans = plan_groups(&groups).await;
//     let (play, replay) = play_and_replay().await;
//     let start = checkpoint(&play).await;
//     receipts_ok(run_plans(&SpaceStoreHandle::new(play.clone()), &plans).await);
//     let played = checkpoint(&play).await;
//     rig(&replay, &start.root, &played.log).await;
//
//     let handle = SpaceStoreHandle::new(replay.clone());
//     let mut txn = handle.begin(&universe()).await.expect("begin");
//     plans[0][0].stage(&mut txn, None).expect("stage the park");
//     txn.produce(store_op("a", "x", false))
//         .expect("a store with a recorded binding stages, as in the host");
//     txn.commit().await.expect("commit");
//     assert!(matches!(replay.check_replay_data().await, Err(RSpaceError::UnusedCommEvent { .. })));
//     assert!(replay.create_checkpoint().await.is_err());
// }
// ```

// Retired by decision 90 (C6, step D): the test drove the rigged replay or
// reporting mode of the store, which retires (decision 125). Native replay of
// the store is tested by `native_space_replay_rejects_a_tampered_candidate` and
// its siblings in `native_session/space.rs`, and by
// `space_native_replay_equal_*`. The text before:
// ```
// #[tokio::test]
// async fn space_store_handle_replay_native_directive_fails_closed() {
//     let (play, replay) = play_and_replay().await;
//     let start = checkpoint(&play).await;
//     let recorder = Recorder::with_directive(ReplayOperationDirective::Store);
//     observed(&replay, &recorder);
//     rig(&replay, &start.root, &[]).await;
//     let handle = SpaceStoreHandle::new(replay.clone());
//
//     let mut txn = handle.begin(&universe()).await.expect("begin");
//     txn.produce(store_op("a", "x", false)).expect("stage");
//     assert_eq!(txn.commit().await, Err(SpaceTxnError::NativeReplayDirectiveUnsupported));
//     let mut txn = handle.begin(&universe()).await.expect("begin");
//     txn.consume(park_op(&["a"], vec![Pat::Any], "k", false, BTreeSet::new()))
//         .expect("stage");
//     assert_eq!(txn.commit().await, Err(SpaceTxnError::NativeReplayDirectiveUnsupported));
//
//     let kinds: Vec<&'static str> = recorder.calls().iter().map(call_kind).collect();
//     assert_eq!(kinds, vec!["start", "directive", "finish", "start", "directive", "finish"]);
//     assert!(recorder.calls().iter().all(|call| match call {
//         Call::Finish { completion, .. } => *completion == RSpaceOperationCompletion::Rejected,
//         _ => true,
//     }));
//     assert!(replay.get_data(&s("a")).await.is_empty());
//     assert!(
//         replay
//             .get_waiting_continuations(names(&["a"]))
//             .await
//             .is_empty()
//     );
// }
// ```

// Retired by decision 90 (C6, step D): the test drove the rigged replay or
// reporting mode of the store, which retires (decision 125). Native replay of
// the store is tested by `native_space_replay_rejects_a_tampered_candidate` and
// its siblings in `native_session/space.rs`, and by
// `space_native_replay_equal_*`. The text before:
// ```text
// #[tokio::test]
// async fn space_store_handle_replay_comm_rejection_rolls_back_counter() {
//     let groups =
//         each(vec![Op::consume(&["a"], vec![Pat::Any], "k", false), Op::produce("a", "x", false)]);
//     let plans = plan_groups(&groups).await;
//     let (play, txn_replay) = play_and_replay().await;
//     let start = checkpoint(&play).await;
//     receipts_ok(run_plans(&SpaceStoreHandle::new(play.clone()), &plans).await);
//     let played = checkpoint(&play).await;
//     let host_replay = replay_over(&play);
//     let txn_recorder = Recorder::rejecting_comm(0);
//     observed(&txn_replay, &txn_recorder);
//     let host_recorder = Recorder::rejecting_comm(0);
//     observed(&host_replay, &host_recorder);
//     rig(&txn_replay, &start.root, &played.log).await;
//     rig(&host_replay, &start.root, &played.log).await;
//
//     let ops = flatten(&groups);
//     run_host(&host_replay, &ops[0]).await.expect("host park");
//     assert!(matches!(run_host(&host_replay, &ops[1]).await, Err(RSpaceError::OutOfPhlogistons)));
//     let handle = SpaceStoreHandle::new(txn_replay.clone());
//     let mut txn = handle.begin(&universe()).await.expect("begin");
//     plans[0][0].stage(&mut txn, None).expect("stage the park");
//     plans[1][0].stage(&mut txn, None).expect("stage the fire");
//     assert_eq!(txn.commit().await, Err(SpaceTxnError::Host(RSpaceError::OutOfPhlogistons)));
//
//     let x = produce_source("a", "x", false);
//     for replay in [&host_replay, &txn_replay] {
//         let mut probe = SpaceStoreHandle::new(replay.clone())
//             .begin(&universe())
//             .await
//             .expect("begin");
//         assert_eq!(
//             probe.produce_counters(std::slice::from_ref(&x)),
//             Ok(BTreeMap::from([(x.clone(), 0)]))
//         );
//     }
//     assert_eq!(txn_recorder.calls(), host_recorder.calls());
//     assert_eq!(replay_state(&txn_replay), replay_state(&host_replay));
//     assert_eq!(replay_bindings(&txn_replay), replay_bindings(&host_replay));
// }
// ```

// Retired by decision 90 (C6, step D): the test drove the rigged replay or
// reporting mode of the store, which retires (decision 125). Native replay of
// the store is tested by `native_space_replay_rejects_a_tampered_candidate` and
// its siblings in `native_session/space.rs`, and by
// `space_native_replay_equal_*`. The text before:
// ```text
// #[tokio::test]
// async fn space_store_handle_replay_consult_admits_matches_host_rule() {
//     let groups = vec![
//         vec![
//             Op::produce("a", "x", false),
//             Op::produce("a", "x", false),
//             Op::consume(&["a"], vec![Pat::Any], "k", false),
//         ],
//         vec![Op::produce("a", "x", false), Op::consume(&["a"], vec![Pat::Any], "k", false)],
//         vec![Op::consume(&["b"], vec![Pat::Any], "k2", false), Op::produce("b", "y", false)],
//     ];
//     let plans = plan_groups(&groups).await;
//     let (play, replay) = play_and_replay().await;
//     let start = checkpoint(&play).await;
//     receipts_ok(run_plans(&SpaceStoreHandle::new(play.clone()), &plans).await);
//     let played = checkpoint(&play).await;
//     let host_replay = replay_over(&play);
//     rig(&host_replay, &start.root, &played.log).await;
//     run_host_ops(&host_replay, &flatten(&groups)).await;
//     host_replay
//         .check_replay_data()
//         .await
//         .expect("the host rule accepts the log");
//
//     let mut play_probe = SpaceStoreHandle::new(play.clone())
//         .begin(&universe())
//         .await
//         .expect("begin");
//     let k = consume_source(&["a"], &[Pat::Any], "k", false);
//     assert_eq!(play_probe.recorded_comms(RSpaceOperationSource::Consume(&k)), None);
//     drop(play_probe);
//
//     rig(&replay, &start.root, &played.log).await;
//     let handle = SpaceStoreHandle::new(replay.clone());
//     let mut txn = handle.begin(&universe()).await.expect("begin");
//     let recorded = txn
//         .recorded_comms(RSpaceOperationSource::Consume(&k))
//         .expect("replay lists recorded COMMs");
//     let x = produce_source("a", "x", false);
//     let times: Vec<i32> = recorded
//         .iter()
//         .map(|comm| *comm.times_repeated.get(&x).expect("counts x"))
//         .collect();
//     let mut sorted_times = times.clone();
//     sorted_times.sort();
//     assert_eq!(sorted_times, vec![2, 3]);
//     let comm_with = |count: i32| {
//         recorded
//             .iter()
//             .find(|comm| comm.times_repeated.get(&x) == Some(&count))
//             .expect("a recorded COMM with that count")
//             .clone()
//     };
//     let (two, three) = (comm_with(2), comm_with(3));
//     let record = datum("a", "x", false);
//
//     plans[0][0].stage(&mut txn, None).expect("stage");
//     plans[0][1].stage(&mut txn, None).expect("stage");
//     assert_eq!(txn.admits(&two, &record, false), Ok(true));
//     assert_eq!(txn.admits(&three, &record, false), Ok(false));
//     plans[0][2]
//         .stage(&mut txn, None)
//         .expect("stage the fire that pairs times 2");
//     assert_eq!(txn.recorded_comms(RSpaceOperationSource::Consume(&k)), Some(vec![three.clone()]));
//     plans[1][0].stage(&mut txn, None).expect("stage");
//     assert_eq!(txn.admits(&three, &record, false), Ok(true));
//     assert_eq!(txn.admits(&two, &record, false), Ok(false));
//     plans[1][1]
//         .stage(&mut txn, None)
//         .expect("stage the fire that pairs times 3");
//
//     plans[2][0].stage(&mut txn, None).expect("stage the park");
//     let y = produce_source("b", "y", false);
//     let produced = txn
//         .recorded_comms(RSpaceOperationSource::Produce(&y))
//         .expect("replay lists recorded COMMs");
//     assert_eq!(produced.len(), 1);
//     let incoming = datum("b", "y", false);
//     assert_eq!(txn.admits(&produced[0], &incoming, true), Ok(true));
//     assert_eq!(txn.admits(&produced[0], &incoming, false), Ok(false));
//     plans[2][1]
//         .stage(&mut txn, None)
//         .expect("stage the produce fire");
//     txn.commit().await.expect("commit");
//     assert_eq!(checkpoint(&replay).await.root, played.root);
// }
// ```

// Added by decision 102 (plan v4 C3b) and by plan v4 errata (V4-12): play
// follows no recorded row. `recorded_comms` gives None, the mark of a leading
// anchor changes nothing, and after a staging error `end_record` keeps the
// failure path of the caller.
#[tokio::test]
async fn space_store_handle_play_follows_no_recorded_row() {
    let plans = plan_groups(&[vec![Op::produce("a", "x", false)]]).await;
    let (play, _) = play_and_replay().await;
    let mut txn = SpaceStoreHandle::new(play.clone())
        .begin(&universe())
        .await
        .expect("begin");
    let x = produce_source("a", "x", false);
    assert_eq!(txn.recorded_comms(RSpaceOperationSource::Produce(&x)), None);
    plans[0][0].stage(&mut txn, None).expect("stage x");
    txn.mark_leading_anchor();
    assert!(!txn.end_record(), "play keeps the failure path");
    assert_eq!(txn.recorded_comms(RSpaceOperationSource::Produce(&x)), None);
    let receipt = txn.commit().await.expect("commit");
    assert!(matches!(receipt.ops.as_slice(), [SpaceOpReceipt::Stored { .. }]));
}

// Retired by decision 90 (C6, step D): the test drove the rigged replay or
// reporting mode of the store, which retires (decision 125). Native replay of
// the store is tested by `native_space_replay_rejects_a_tampered_candidate` and
// its siblings in `native_session/space.rs`, and by
// `space_native_replay_equal_*`. The text before:
// ```text
// #[tokio::test]
// async fn space_store_handle_replay_cross_channel_comm_pairs_once() {
//     let (play, replay) = play_and_replay().await;
//     let persistent_foreign = Datum {
//         a: s("m"),
//         persist: true,
//         source: produce_source("d", "m", true),
//     };
//     let linear_foreign = Datum {
//         a: s("n"),
//         persist: false,
//         source: produce_source("b", "n", false),
//     };
//     play.get_store()
//         .put_datum(&s("c"), persistent_foreign.clone());
//     play.get_store().put_datum(&s("a"), linear_foreign.clone());
//     run_host_ops(&play, &[Op::consume(&["d", "a"], vec![Pat::Any, Pat::Any], "k2", false)]).await;
//     let pre = checkpoint(&play).await;
//
//     let consume_fire = || {
//         fire_consume_op(&["c"], vec![Pat::Any], "k", vec![bind_candidate(
//             s("c"),
//             &persistent_foreign,
//             s("m"),
//             0,
//         )])
//     };
//     let joined = WaitingContinuation::create(
//         &names(&["d", "a"]),
//         &vec![Pat::Any, Pat::Any],
//         &s("k2"),
//         false,
//         BTreeSet::new(),
//     );
//     let produce_fire = || SpaceProduce {
//         channel: s("d"),
//         data: s("q"),
//         persist: false,
//         outcome: ProduceOutcome::Fire {
//             channels: names(&["d", "a"]),
//             continuation: joined.clone(),
//             data: vec![
//                 incoming_candidate(s("d"), produce_source("d", "q", false), s("q"), false, s("q")),
//                 bind_candidate(s("a"), &linear_foreign, s("n"), 0),
//             ],
//         },
//         before_observe: None,
//     };
//
//     let play_handle = SpaceStoreHandle::new(play.clone());
//     let mut txn = play_handle.begin(&universe()).await.expect("begin");
//     let consumed = txn.consume(consume_fire()).expect("stage the consume fire");
//     let produced = txn.produce(produce_fire()).expect("stage the produce fire");
//     txn.commit().await.expect("commit");
//     let block = checkpoint(&play).await;
//     let consumed = consumed.comm.expect("a COMM");
//     let produced = produced.comm.expect("a COMM");
//     assert_eq!(consumed.consume.channel_hashes, vec![hash(&s("c"))]);
//     assert_eq!(consumed.produces[0].channel_hash, hash(&s("d")));
//     assert!(
//         produced
//             .produces
//             .iter()
//             .any(|produce| produce.channel_hash == hash(&s("b")))
//     );
//
//     // Each COMM pairs once (decision 36): a second copy of the consume fire
//     // stages unpaired, and the commit fails at it with `ReplayDivergence`.
//     // The first fire stays applied and unbound; the copy and the produce
//     // fire after it apply and unbind nothing. A new transaction with the
//     // produce fire then reaches the block root.
//     rig(&replay, &pre.root, &block.log).await;
//     let bindings = replay_bindings(&replay);
//     assert_eq!(bindings.len(), 2, "one key per COMM: the logged consume and the logged produce");
//     let handle = SpaceStoreHandle::new(replay.clone());
//     let mut txn = handle.begin(&universe()).await.expect("begin");
//     txn.consume(consume_fire())
//         .expect("pairs under the consume key");
//     let k = consume_source(&["c"], &[Pat::Any], "k", false);
//     assert_eq!(txn.recorded_comms(RSpaceOperationSource::Consume(&k)), Some(Vec::new()));
//     let copy = txn
//         .consume(consume_fire())
//         .expect("the copy stages unpaired");
//     assert_eq!(copy.comm, Some(consumed.clone()));
//     txn.produce(produce_fire())
//         .expect("pairs under the produce key");
//     assert_eq!(
//         txn.commit().await,
//         Err(SpaceTxnError::ReplayDivergence(Box::new(consumed.clone())))
//     );
//     let produce_key = bincode::serialize(&IOEvent::Produce(produce_source("d", "q", false)))
//         .expect("serialize the produce key");
//     assert_eq!(
//         replay_bindings(&replay)
//             .into_iter()
//             .map(|(key, _)| key)
//             .collect::<Vec<_>>(),
//         vec![produce_key],
//         "the first fire unbinds its COMM, and the produce fire keeps its own"
//     );
//     assert_eq!(replay.get_data(&s("a")).await, vec![linear_foreign.clone()]);
//     assert_eq!(replay.get_waiting_continuations(names(&["d", "a"])).await, vec![joined.clone()]);
//     let mut txn = handle.begin(&universe()).await.expect("begin");
//     txn.produce(produce_fire())
//         .expect("pairs under the produce key");
//     txn.commit().await.expect("commit");
//     assert!(replay_bindings(&replay).is_empty());
//     assert_eq!(checkpoint(&replay).await.root, block.root);
//
//     // A COMM whose produce list repeats a produce comes from a consume on a
//     // group that repeats a channel and binds one persistent record twice.
//     // `rig` binds it once per occurrence under that produce key, and
//     // `recorded_comms` reports it once. A space stages the same sorted group
//     // (FR-1') and pairs the COMM once under its consume key.
//     let repeated = datum("b", "z", true);
//     let twice = || {
//         fire_consume_op(&["b", "b"], vec![Pat::Any, Pat::Any], "k4", vec![
//             bind_candidate(s("b"), &repeated, s("z"), 0),
//             bind_candidate(s("b"), &repeated, s("z"), 0),
//         ])
//     };
//     let mut txn = play_handle.begin(&universe()).await.expect("begin");
//     txn.produce(store_op("b", "z", true))
//         .expect("stage a store");
//     txn.commit().await.expect("commit");
//     let consumed_twice = play
//         .consume(names(&["b", "b"]), vec![Pat::Any, Pat::Any], s("k4"), false, BTreeSet::new())
//         .await
//         .expect("the host consumes on a repeated group");
//     assert!(consumed_twice.is_some());
//     let second = checkpoint(&play).await;
//     let doubled = comms_of(&second.log)
//         .pop()
//         .expect("the repeated COMM is logged");
//     assert_eq!(doubled.produces, vec![repeated.source.clone(), repeated.source.clone()]);
//
//     rig(&replay, &block.root, &second.log).await;
//     let mut txn = handle.begin(&universe()).await.expect("begin");
//     txn.produce(store_op("b", "z", true))
//         .expect("stage a store");
//     assert_eq!(
//         txn.recorded_comms(RSpaceOperationSource::Produce(&repeated.source)),
//         Some(vec![doubled.clone()])
//     );
//     let paired = txn
//         .consume(twice())
//         .expect("the space pairs the repeated COMM");
//     assert_eq!(paired.comm, Some(doubled.clone()));
//     let receipt = txn.commit().await.expect("commit");
//     match &receipt.ops[1] {
//         SpaceOpReceipt::Fired { results, .. } => {
//             assert_eq!(results.len(), 2);
//             assert!(results.iter().all(|result| result.persistent));
//         }
//         other => panic!("expected the repeated fire, got {:?}", other),
//     }
//     assert!(replay_bindings(&replay).is_empty());
//     assert_eq!(replay.get_data(&s("b")).await, vec![repeated.clone()]);
//     assert_eq!(checkpoint(&replay).await.root, second.root);
//
//     // The repeated COMM pairs once: a second copy of the repeated fire
//     // stages unpaired, and the commit fails at it after the store and the
//     // first fire, which stay applied.
//     rig(&replay, &block.root, &second.log).await;
//     let mut txn = handle.begin(&universe()).await.expect("begin");
//     txn.produce(store_op("b", "z", true))
//         .expect("stage a store");
//     txn.consume(twice())
//         .expect("the space pairs the repeated COMM");
//     assert_eq!(
//         txn.recorded_comms(RSpaceOperationSource::Produce(&repeated.source)),
//         Some(Vec::new())
//     );
//     let copy = txn.consume(twice()).expect("the copy stages unpaired");
//     assert_eq!(copy.comm, Some(doubled.clone()));
//     assert_eq!(txn.commit().await, Err(SpaceTxnError::ReplayDivergence(Box::new(doubled.clone()))));
//     assert!(replay_bindings(&replay).is_empty());
//     assert_eq!(replay.get_data(&s("b")).await, vec![repeated.clone()]);
//     assert_eq!(checkpoint(&replay).await.root, second.root);
//
//     // Control: host replay pairs the same COMM through its own consume.
//     let host_replay = replay_over(&play);
//     rig(&host_replay, &block.root, &second.log).await;
//     run_host_ops(&host_replay, &[Op::produce("b", "z", true)]).await;
//     host_replay
//         .consume(names(&["b", "b"]), vec![Pat::Any, Pat::Any], s("k4"), false, BTreeSet::new())
//         .await
//         .expect("the host replay pairs the repeated COMM");
//     assert!(replay_bindings(&host_replay).is_empty());
//     assert_eq!(checkpoint(&host_replay).await.root, second.root);
// }
// ```

// Retired by decision 90 (C6, step D): the test drove the rigged replay or
// reporting mode of the store, which retires (decision 125). Native replay of
// the store is tested by `native_space_replay_rejects_a_tampered_candidate` and
// its siblings in `native_session/space.rs`, and by
// `space_native_replay_equal_*`. The text before:
// ```text
// // Decision 36 (H23, T6): a deploy that runs out of phlo at any observer
// // call replays to the outcome of play. For each position n of a fallible
// // call, play rejects its n-th call and keeps its log, as Casper keeps the
// // log of a failed deploy. Replay, rigged with that log, rejects its own n-th
// // call. It stages what play stages, pairs the fires that play committed,
// // and stages a fire that play did not commit as unpaired, so the same call
// // rejects. Replay then fails with the error of play at the same operation,
// // makes the calls of play plus its directive queries, reaches the state of
// // play, and uses every recorded event. The last position is past every
// // call, so both runs commit everything.
// #[tokio::test]
// async fn space_store_handle_replay_reproduces_play_rejection() {
//     let mut groups = replay_groups();
//     groups.push(vec![
//         Op::peek(&["v1"], vec![Pat::Any], "kp"),
//         Op::produce("v1", "z", false),
//         Op::consume(&["v1"], vec![Pat::Any], "kv", false),
//     ]);
//     let plans = plan_groups(&groups).await;
//     let counted = play_space().await;
//     let counter = Recorder::new();
//     observed(&counted, &counter);
//     receipts_ok(run_plans(&SpaceStoreHandle::new(counted.clone()), &plans).await);
//     let calls = counter.fallible_calls();
//     let mut rejected_comms = 0usize;
//     for n in 0..=calls {
//         let (play, replay) = play_and_replay().await;
//         let start = checkpoint(&play).await;
//         let play_recorder = Recorder::rejecting_call(n);
//         observed(&play, &play_recorder);
//         let played = run_until_failure(&SpaceStoreHandle::new(play.clone()), &plans).await;
//         let log = play.take_event_log().await;
//         match n == calls {
//             true => assert!(played.iter().all(Result::is_ok), "n = {n}: play commits everything"),
//             false => assert_eq!(
//                 played.last(),
//                 Some(&Err(SpaceTxnError::Host(RSpaceError::OutOfPhlogistons))),
//                 "n = {n}: play fails at its n-th call"
//             ),
//         }
//         if matches!(fallible_call(&play_recorder.calls(), n), Some(Call::Comm { .. })) {
//             rejected_comms += 1;
//         }
//
//         let replay_recorder = Recorder::rejecting_call(n);
//         observed(&replay, &replay_recorder);
//         rig(&replay, &start.root, &log).await;
//         let replayed = run_until_failure(&SpaceStoreHandle::new(replay.clone()), &plans).await;
//         assert_eq!(replayed, played, "n = {n}: the outcome");
//         assert_eq!(
//             without_directives(replay_recorder.calls()),
//             play_recorder.calls(),
//             "n = {n}: the observer calls"
//         );
//         assert_eq!(replay_state(&replay), logical_state(&play), "n = {n}: the state");
//         assert!(replay_bindings(&replay).is_empty(), "n = {n}: replay uses every recorded event");
//     }
//     assert!(rejected_comms > 0, "the sweep rejects a COMM charge");
// }
// ```

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

// Retired by decision 90 (C6, step D): the test drove the rigged replay or
// reporting mode of the store, which retires (decision 125). Native replay of
// the store is tested by `native_space_replay_rejects_a_tampered_candidate` and
// its siblings in `native_session/space.rs`, and by
// `space_native_replay_equal_*`. The text before:
// ```
// #[tokio::test]
// async fn space_store_handle_sweep_play_and_replay_agree() {
//     let plans = plan_groups(&volatile_groups()).await;
//     let (play, replay) = play_and_replay().await;
//     let start = checkpoint(&play).await;
//     let play_handle = SpaceStoreHandle::new(play.clone());
//     receipts_ok(run_plans(&play_handle, &plans).await);
//     let played_report = play_handle
//         .sweep_volatile(&names(&["v1", "v2"]))
//         .await
//         .expect("sweep in play");
//     let played = checkpoint(&play).await;
//
//     rig(&replay, &start.root, &played.log).await;
//     let replay_handle = SpaceStoreHandle::new(replay.clone());
//     receipts_ok(run_plans(&replay_handle, &plans).await);
//     let replayed_report = replay_handle
//         .sweep_volatile(&names(&["v1", "v2"]))
//         .await
//         .expect("sweep in replay");
//     let replayed = checkpoint(&replay).await;
//     assert_eq!(replayed_report, played_report);
//     assert_eq!(replayed.root, played.root);
// }
// ```

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

// Retired by decision 90 (C6, step D): the test drove the rigged replay or
// reporting mode of the store, which retires (decision 125). Native replay of
// the store is tested by `native_space_replay_rejects_a_tampered_candidate` and
// its siblings in `native_session/space.rs`, and by
// `space_native_replay_equal_*`. The text before:
// ```
// #[tokio::test]
// async fn space_store_handle_reporting_pairs_like_replay() {
//     let groups = replay_groups();
//     let plans = plan_groups(&groups).await;
//     let play = play_space().await;
//     let start = checkpoint(&play).await;
//     receipts_ok(run_plans(&SpaceStoreHandle::new(play.clone()), &plans).await);
//     let played = checkpoint(&play).await;
//     let replay = replay_over(&play);
//     let reporting = reporting_over(&play);
//     rig(&replay, &start.root, &played.log).await;
//     reporting
//         .rig_and_reset(start.root.clone(), played.log.clone())
//         .await
//         .expect("rig the reporting space");
//
//     let replay_receipts =
//         receipts_ok(run_plans(&SpaceStoreHandle::new(replay.clone()), &plans).await);
//     let reporting_handle = SpaceStoreHandle::new(reporting.clone());
//     assert_eq!(reporting_handle.mode(), SpaceMode::Reporting);
//     let reporting_receipts = receipts_ok(run_plans(&reporting_handle, &plans).await);
//     assert_eq!(reporting_receipts, replay_receipts);
//     let replayed = checkpoint(&replay).await;
//     let reported = ISpace::create_checkpoint(&reporting)
//         .await
//         .expect("checkpoint the reporting space");
//     assert_eq!(reported.root, replayed.root);
//     assert_eq!(reported.root, played.root);
// }
// ```

// Retired by decision 90 (C6, step D): the test drove the rigged replay or
// reporting mode of the store, which retires (decision 125). Native replay of
// the store is tested by `native_space_replay_rejects_a_tampered_candidate` and
// its siblings in `native_session/space.rs`, and by
// `space_native_replay_equal_*`. The text before:
// ```
// #[tokio::test]
// async fn space_store_handle_reporting_records_txn_events_like_host() {
//     let groups = replay_groups();
//     let plans = plan_groups(&groups).await;
//     let play = play_space().await;
//     let start = checkpoint(&play).await;
//     receipts_ok(run_plans(&SpaceStoreHandle::new(play.clone()), &plans).await);
//     let played = checkpoint(&play).await;
//     let by_host = reporting_over(&play);
//     let by_txn = reporting_over(&play);
//     for reporting in [&by_host, &by_txn] {
//         reporting
//             .rig_and_reset(start.root.clone(), played.log.clone())
//             .await
//             .expect("rig the reporting space");
//     }
//     run_host_ops(&by_host, &flatten(&groups)).await;
//     receipts_ok(run_plans(&SpaceStoreHandle::new(by_txn.clone()), &plans).await);
//     let host_report = by_host.get_report().expect("host report");
//     let txn_report = by_txn.get_report().expect("transaction report");
//     assert!(!txn_report.is_empty());
//     assert!(
//         txn_report
//             .iter()
//             .flat_map(|batch| batch.events.iter())
//             .any(|event| matches!(event, ReportingEvent::ReportingComm(_)))
//     );
//     assert_eq!(txn_report, host_report);
// }
// ```

// Retired by decision 90 (C6, step D): the test drove the rigged replay or
// reporting mode of the store, which retires (decision 125). Native replay of
// the store is tested by `native_space_replay_rejects_a_tampered_candidate` and
// its siblings in `native_session/space.rs`, and by
// `space_native_replay_equal_*`. The text before:
// ```
// #[tokio::test]
// async fn space_store_handle_reporting_shares_inner_replay_state() {
//     let groups = replay_groups();
//     let plans = plan_groups(&groups).await;
//     let play = play_space().await;
//     let start = checkpoint(&play).await;
//     receipts_ok(run_plans(&SpaceStoreHandle::new(play.clone()), &plans).await);
//     let groups_view = groups_of(&[&play]).await;
//     let expected = channel_view(&play, &universe(), &groups_view).await;
//     let played = checkpoint(&play).await;
//     let reporting = reporting_over(&play);
//     reporting
//         .rig_and_reset(start.root.clone(), played.log.clone())
//         .await
//         .expect("rig the reporting space");
//
//     let handle = SpaceStoreHandle::new(reporting.clone());
//     receipts_ok(run_plans(&handle, &plans).await);
//     assert_eq!(channel_view(&reporting, &universe(), &groups_view).await, expected);
//     reporting
//         .check_replay_data()
//         .await
//         .expect("the transactions consumed the inner replay bindings");
//     let mut txn = handle.begin(&universe()).await.expect("begin");
//     assert_eq!(txn.mode(), SpaceMode::Reporting);
//     assert_eq!(txn.data(&s("a")).expect("read a").to_vec(), reporting.get_data(&s("a")).await);
// }
// ```

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
            own: false,
            // Added by decision 123 (plan v4, V3-9): no row search.
            search: None,
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

// Retired by decision 90 (C6, step D): the test drove the rigged replay or
// reporting mode of the store, which retires (decision 125). Native replay of
// the store is tested by `native_space_replay_rejects_a_tampered_candidate` and
// its siblings in `native_session/space.rs`, and by
// `space_native_replay_equal_*`. The text before:
// ```text
// // Review F-PAIR-1 and decision 36: replay pairs a COMM only under its
// // trigger key, as host replay does. A log without the Produce event of a
// // produce-side fire leaves that fire unpaired: it stages, and the commit
// // fails at it with `ReplayDivergence` after the observer calls of the fire.
// // The store staged before the fire stays applied; the fire and the store
// // after it apply, log and unbind nothing. Host replay rejects the same log.
// #[tokio::test]
// async fn space_store_handle_replay_requires_produce_trigger_event() {
//     let groups =
//         each(vec![Op::consume(&["a"], vec![Pat::Any], "k", false), Op::produce("a", "x", false)]);
//     let plans = plan_groups(&groups).await;
//     let (play, replay) = play_and_replay().await;
//     let start = checkpoint(&play).await;
//     receipts_ok(run_plans(&SpaceStoreHandle::new(play.clone()), &plans).await);
//     let played = checkpoint(&play).await;
//     let x = produce_source("a", "x", false);
//     let forged = without_event(&played.log, &IOEvent::Produce(x.clone()));
//     assert_eq!(forged.len() + 1, played.log.len());
//
//     let host_replay = replay_over(&play);
//     rig(&host_replay, &start.root, &forged).await;
//     run_host_ops(&host_replay, &flatten(&groups)).await;
//     assert!(matches!(
//         host_replay.check_replay_data().await,
//         Err(RSpaceError::UnusedCommEvent { .. })
//     ));
//
//     let recorder = Recorder::new();
//     observed(&replay, &recorder);
//     rig(&replay, &start.root, &forged).await;
//     let handle = SpaceStoreHandle::new(replay.clone());
//     receipts_ok(run_plans(&handle, &plans[..1]).await);
//     let calls = recorder.calls().len();
//     let bindings = replay_bindings(&replay);
//     let state = replay_state(&replay);
//     let mut txn = handle.begin(&universe()).await.expect("begin");
//     assert_eq!(txn.recorded_comms(RSpaceOperationSource::Produce(&x)), Some(Vec::new()));
//     txn.produce(store_op("b", "w", false))
//         .expect("stage a store before the fire");
//     let comm = plans[1][0]
//         .stage(&mut txn, None)
//         .expect("the fire stages unpaired")
//         .comm
//         .expect("the fire has a COMM");
//     assert_eq!(comm.produces, vec![x.clone()]);
//     txn.produce(store_op("c", "u", false))
//         .expect("stage a store after the fire");
//     assert_eq!(recorder.calls().len(), calls);
//     assert_eq!(txn.commit().await, Err(SpaceTxnError::ReplayDivergence(Box::new(comm))));
//     assert_unpaired_fire_calls(&recorder.calls()[calls..], "produce");
//     assert_eq!(replay_bindings(&replay), bindings);
//     assert_eq!(replay_state(&replay), with_record(state, "b", "w"));
//     assert!(matches!(replay.check_replay_data().await, Err(RSpaceError::UnusedCommEvent { .. })));
// }
// ```

// Retired by decision 90 (C6, step D): only the two trigger-event replay tests
// called this helper, and they retire with the rigged replay modes (decision
// 125). The text before:
// ```
// // The calls of a commit that applies a store and then fails at an unpaired
// // fire whose introduction is `introduction` ("produce" or "consume"): the
// // store's calls, then the fire's calls through its COMM call and a rejected
// // finish. The operation after the fire makes no call.
// fn assert_unpaired_fire_calls(calls: &[Call], introduction: &'static str) {
//     assert_eq!(calls.iter().map(call_kind).collect::<Vec<_>>(), vec![
//         "start",
//         "directive",
//         "produce",
//         "finish",
//         "start",
//         "directive",
//         introduction,
//         "comm",
//         "finish"
//     ]);
//     assert!(matches!(
//         calls.last(),
//         Some(Call::Finish {
//             completion: RSpaceOperationCompletion::Rejected,
//             ..
//         })
//     ));
// }
// ```

// Retired by decision 90 (C6, step D): only the two trigger-event replay tests
// called this helper, and they retire with the rigged replay modes (decision
// 125). The text before:
// ```
// // `state` with the non-persistent record `data` stored on `channel`.
// fn with_record(mut state: LogicalState, channel: &str, data: &str) -> LogicalState {
//     let record = bincode::serialize(&datum(channel, data, false)).expect("serialize a datum");
//     state.data.entry(s(channel)).or_default().insert(0, record);
//     state
// }
// ```

// Retired by decision 90 (C6, step D): the test drove the rigged replay or
// reporting mode of the store, which retires (decision 125). Native replay of
// the store is tested by `native_space_replay_rejects_a_tampered_candidate` and
// its siblings in `native_session/space.rs`, and by
// `space_native_replay_equal_*`. The text before:
// ```text
// // The consume-side form of F-PAIR-1 and decision 36: a log without the
// // Consume event of a consume-side fire leaves that fire unpaired, and the
// // commit fails at it with `ReplayDivergence`, with the same prefix effect.
// // Control (HEAD behavior): host replay still pairs this COMM, because `rig`
// // puts every COMM into `consume_replay_index` whether or not the log holds
// // its Consume event, and the narrowed consume path reads that index. The
// // substrate pairs only under the trigger key.
// #[tokio::test]
// async fn space_store_handle_replay_requires_consume_trigger_event() {
//     let groups =
//         each(vec![Op::produce("a", "x", false), Op::consume(&["a"], vec![Pat::Any], "k", false)]);
//     let plans = plan_groups(&groups).await;
//     let (play, replay) = play_and_replay().await;
//     let start = checkpoint(&play).await;
//     receipts_ok(run_plans(&SpaceStoreHandle::new(play.clone()), &plans).await);
//     let played = checkpoint(&play).await;
//     let k = consume_source(&["a"], &[Pat::Any], "k", false);
//     let forged = without_event(&played.log, &IOEvent::Consume(k.clone()));
//     assert_eq!(forged.len() + 1, played.log.len());
//
//     let host_replay = replay_over(&play);
//     rig(&host_replay, &start.root, &forged).await;
//     run_host_ops(&host_replay, &flatten(&groups)).await;
//     host_replay
//         .check_replay_data()
//         .await
//         .expect("control: host replay pairs through consume_replay_index");
//
//     let recorder = Recorder::new();
//     observed(&replay, &recorder);
//     rig(&replay, &start.root, &forged).await;
//     let handle = SpaceStoreHandle::new(replay.clone());
//     receipts_ok(run_plans(&handle, &plans[..1]).await);
//     let calls = recorder.calls().len();
//     let bindings = replay_bindings(&replay);
//     let state = replay_state(&replay);
//     let mut txn = handle.begin(&universe()).await.expect("begin");
//     assert_eq!(txn.recorded_comms(RSpaceOperationSource::Consume(&k)), Some(Vec::new()));
//     txn.produce(store_op("b", "w", false))
//         .expect("stage a store before the fire");
//     let comm = plans[1][0]
//         .stage(&mut txn, None)
//         .expect("the fire stages unpaired")
//         .comm
//         .expect("the fire has a COMM");
//     assert_eq!(comm.consume, k);
//     txn.produce(store_op("c", "u", false))
//         .expect("stage a store after the fire");
//     assert_eq!(recorder.calls().len(), calls);
//     assert_eq!(txn.commit().await, Err(SpaceTxnError::ReplayDivergence(Box::new(comm))));
//     assert_unpaired_fire_calls(&recorder.calls()[calls..], "consume");
//     assert_eq!(replay_bindings(&replay), bindings);
//     assert_eq!(replay_state(&replay), with_record(state, "b", "w"));
//     assert!(matches!(replay.check_replay_data().await, Err(RSpaceError::UnusedCommEvent { .. })));
// }
// ```

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
        own: false,
        // Added by decision 123 (plan v4, V3-9): no row search.
        search: None,
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

// Peek fires in replay and reporting: both pair the play log, remove the
// stored record of the consume-side peek as play does and store it again
// with the agent's re-store (decision 45, Rule E), leave the incoming datum
// of the produce-side peek unstored until its re-offer stores it (FR-10),
// and reach the play root.
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
        own: false,
        // Added by decision 123 (plan v4, V3-9): no row search.
        search: None,
    })
    .expect("stage a peek consume fire");
    assert!(txn.data(&s("a")).expect("read a").is_empty());
    txn.produce(store_op("a", "x", false))
        .expect("stage the re-store of x");
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
        own: false,
        // Added by decision 123 (plan v4, V3-9): no row search.
        search: None,
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

// The play log of the transactions equals the log of HEAD's program, whose
// reducer produces the peeked x and the incoming y again (`produce_peeks`).
#[tokio::test]
async fn space_store_handle_peek_fires_replay_and_report() {
    let play = play_space().await;
    // Changed by decision 90 (C6, step D): the root lost its reader with the replay
    // half, and the checkpoint stays because it empties the event log. The text
    // before:
    // let start = checkpoint(&play).await;
    checkpoint(&play).await;
    let played_receipts = receipts_ok(run_peek_fires(&SpaceStoreHandle::new(play.clone())).await);
    assert_eq!(play.get_data(&s("a")).await, vec![datum("a", "x", false)]);
    assert!(play.get_data(&s("b")).await.is_empty());
    match &played_receipts[0].ops[1..] {
        [
            SpaceOpReceipt::Fired {
                results: peeked,
                produce: None,
                ..
            },
            SpaceOpReceipt::Stored { produce: restored },
            SpaceOpReceipt::Parked { .. },
            SpaceOpReceipt::Fired { results, .. },
            SpaceOpReceipt::Stored { produce },
        ] => {
            assert_eq!(peeked.len(), 1);
            assert!(!peeked[0].persistent);
            assert_eq!(restored, &produce_source("a", "x", false));
            assert_eq!(results.len(), 1);
            assert!(!results[0].persistent);
            assert_eq!(produce, &produce_source("b", "y", false));
        }
        other => {
            panic!("expected the peek fires with the re-store and the re-offer, got {:?}", other)
        }
    }
    let played = checkpoint(&play).await;
    assert_eq!(comms_of(&played.log).len(), 3);
    assert_eq!(kinds(&played.log), vec![
        "produce", "consume", "comm", "produce", "consume", "produce", "comm", "produce",
        "consume", "comm"
    ]);
    let head = play_space().await;
    run_host_ops(&head, &[
        Op::produce("a", "x", false),
        Op::peek(&["a"], vec![Pat::Any], "k"),
        Op::produce("a", "x", false),
        Op::peek(&["b"], vec![Pat::Any], "k2"),
        Op::produce("b", "y", false),
        Op::produce("b", "y", false),
        Op::consume(&["b"], vec![Pat::Any], "k3", false),
    ])
    .await;
    let head_played = checkpoint(&head).await;
    assert_eq!(event_bytes(&played.log), event_bytes(&head_played.log));
    assert_eq!(played.root, head_played.root);

    // Retired by decision 90 (C6, step D): the replay half ran through the
    // rigged replay modes, which retire (decision 125). Native replay of the
    // store is tested by `native_space_replay_rejects_a_tampered_candidate` and
    // its siblings in `native_session/space.rs`, and by
    // `space_native_replay_equal_*`. The text before:
    // ```
    // let replay = replay_over(&play);
    // rig(&replay, &start.root, &played.log).await;
    // let replayed = receipts_ok(run_peek_fires(&SpaceStoreHandle::new(replay.clone())).await);
    // assert_eq!(replayed, played_receipts);
    // replay
    //     .check_replay_data()
    //     .await
    //     .expect("replay pairs every peek COMM");
    // assert_eq!(checkpoint(&replay).await.root, played.root);
    //
    // let reporting = reporting_over(&play);
    // reporting
    //     .rig_and_reset(start.root.clone(), played.log.clone())
    //     .await
    //     .expect("rig the reporting space");
    // let reported = receipts_ok(run_peek_fires(&SpaceStoreHandle::new(reporting.clone())).await);
    // assert_eq!(reported, played_receipts);
    // reporting
    //     .check_replay_data()
    //     .await
    //     .expect("reporting pairs every peek COMM");
    // let reported_comms = reporting
    //     .get_report()
    //     .expect("the report")
    //     .iter()
    //     .flat_map(|batch| batch.events.iter())
    //     .filter(|event| matches!(event, ReportingEvent::ReportingComm(_)))
    //     .count();
    // assert_eq!(reported_comms, 3);
    // let reported_root = ISpace::create_checkpoint(&reporting)
    //     .await
    //     .expect("checkpoint the reporting space")
    //     .root;
    // assert_eq!(reported_root, played.root);
    // ```
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
// Changed by decision 103 (decision 118, U3 (i) of decision 122): the fire
// that binds a non-persistent incoming datum twice fails closed. The
// substrate now accepts the fire of the control (CR-11), and the next test
// checks it. The text before: "Control (HEAD behavior): the host binds a
// persistent incoming datum once per pattern it matches, a fire that the
// substrate refuses."
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
        own: false,
        // Added by decision 123 (plan v4, V3-9): no row search.
        search: None,
    };
    // Changed by decision 103 (decision 118, U3 (i) of decision 122): the
    // error names the persistent case (CR-11).
    // assert_eq!(
    //     txn.produce(fire).err(),
    //     Some(SpaceTxnError::InvalidOperation(
    //         "a produce fire needs exactly one incoming candidate"
    //     ))
    // );
    assert_eq!(
        txn.produce(fire).err(),
        Some(SpaceTxnError::InvalidOperation(
            "a produce fire needs one incoming candidate, or more of a persistent datum in its \
             collection"
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

// Added by decision 103 (decision 118, U3 (i) of decision 122): a persistent
// incoming datum binds each position of its collection that its pattern
// matches, as the host binds it (CR-11). The substrate fire on [b, b] holds
// the incoming candidate twice, and both roles end in the same state, with
// the same observer calls, event log and root.
#[tokio::test]
async fn space_store_handle_persistent_incoming_binds_each_position_of_its_collection() {
    let twin = twin().await;
    let waiting = [Op::consume(&["b", "b"], vec![Pat::Any, Pat::Any], "k", false)];
    run_host_ops(&twin.host, &waiting).await;
    run_host_ops(&twin.txn, &waiting).await;
    let handle = SpaceStoreHandle::new(twin.txn.clone());
    let mut txn = handle.begin(&universe()).await.expect("begin");
    let continuation = txn.continuations(&names(&["b", "b"])).expect("read [b, b]")[0].clone();
    let incoming =
        || incoming_candidate(s("b"), produce_source("b", "x", true), s("x"), true, s("x"));
    let fire = SpaceProduce {
        channel: s("b"),
        data: s("x"),
        persist: true,
        outcome: ProduceOutcome::Fire {
            channels: names(&["b", "b"]),
            continuation,
            data: vec![incoming(), incoming()],
        },
        before_observe: None,
        own: false,
        // Added by decision 123 (plan v4, V3-9): no row search.
        search: None,
    };
    txn.produce(fire)
        .expect("a persistent incoming datum binds both positions");
    let receipt = txn.commit().await.expect("commit the fire");
    match receipt.ops.as_slice() {
        [
            SpaceOpReceipt::Fired {
                results, produce, ..
            },
        ] => {
            assert_eq!(results.len(), 2);
            assert!(
                results
                    .iter()
                    .all(|result| result.removed_datum == s("x") && result.persistent)
            );
            assert_eq!(produce, &Some(produce_source("b", "x", true)));
        }
        other => panic!("expected one produce fire on [b, b], got {:?}", other),
    }
    let (_, results, _) = twin
        .host
        .produce(s("b"), s("x"), true)
        .await
        .expect("the host produce")
        .expect("the host fires");
    assert_eq!(results.len(), 2);
    assert_eq!(logical_state(&twin.txn), logical_state(&twin.host));
    assert_parity(&twin).await;
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
            own: false,
            // Added by decision 123 (plan v4, V3-9): no row search.
            search: None,
        };
        assert_eq!(txn.produce(fire).err(), non_canonical);
        assert_eq!(txn.commit().await.expect("commit nothing").ops.len(), 0);
    }
    assert_eq!(continuation_keys(&**space.get_store()), keys);
    assert_eq!(logical_state(&space), logical_state(&reference));
    assert_eq!(checkpoint(&space).await.root, checkpoint(&reference).await.root);
}

// Host replay of a repeated channel (HEAD behavior since DR-129, E1): host
// play and host replay give the list of a channel that a join names twice one
// order by source hash, with the incoming datum first
// (`order_repeated_channels`). So host replay of the play log binds the
// recorded data, leaves no binding and reaches the play root, as the
// substrate does (FR-1').
// Changed by decision 89 (DR-129): before E1, host replay bound the records in
// store order and failed on this log, and this control expected the failure:
//
// // Control (HEAD behavior): host replay binds the records of a repeated
// // channel in store order, where host play binds them in hash order. So host
// // replay of a valid play log can compute another COMM than the log records
// // and fail its assertion. The substrate replays the same log, because the
// // agent stages the recorded bindings (FR-1').
// #[tokio::test]
// async fn space_store_handle_host_replay_repeated_channel_control() {
#[tokio::test]
async fn space_store_handle_host_replay_follows_a_repeated_channel() {
    let ops = vec![
        Op::produce("a", "y", false),
        Op::produce("a", "x", true),
        Op::consume(&["a", "a"], vec![Pat::Any, Pat::Any], "k1", false),
    ];
    // Changed by decision 90 (C6, step D): the replay through transactions
    // retires with the rigged replay modes (decision 125), so this test keeps
    // its host replay. The text before:
    // let plans = plan_groups(&each(ops.clone())).await;
    // let (play, replay) = play_and_replay().await;
    let play = play_space().await;
    let start = checkpoint(&play).await;
    run_host_ops(&play, &ops).await;
    let played = checkpoint(&play).await;
    let recorded = comms_of(&played.log);
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].produces, vec![
        produce_source("a", "y", false),
        produce_source("a", "x", true)
    ]);

    // Retired by decision 90 (C6, step D): the replay half ran through the rigged
    // replay modes, which retire (decision 125). Native replay of the store is
    // tested by `native_space_replay_rejects_a_tampered_candidate` and its siblings
    // in `native_session/space.rs`, and by `space_native_replay_equal_*`. The text
    // before:
    // ```
    // rig(&replay, &start.root, &played.log).await;
    // receipts_ok(run_plans(&SpaceStoreHandle::new(replay.clone()), &plans).await);
    // assert!(replay_bindings(&replay).is_empty());
    // assert_eq!(checkpoint(&replay).await.root, played.root);
    // ```

    let host_replay = replay_over(&play);
    rig(&host_replay, &start.root, &played.log).await;
    run_host_ops(&host_replay, &ops[..2]).await;
    // let refused = match AssertUnwindSafe(run_host(&host_replay, &ops[2]))
    //     .catch_unwind()
    //     .await
    // {
    //     Err(payload) => panic_text(payload),
    //     Ok(outcome) => panic!("expected the host replay to fail, got {:?}",
    // outcome.map(|_| ())), };
    // assert!(refused.contains("was not contained in the trace"), "{}", refused);
    run_host(&host_replay, &ops[2])
        .await
        .expect("host replay binds the recorded data of the repeated channel");
    assert!(replay_bindings(&host_replay).is_empty());
    assert_eq!(checkpoint(&host_replay).await.root, played.root);
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
// Changed by decision 90 (C6, step D): the replay half of this test retires
// with the rigged replay modes (decision 125). The text before:
// // commits under the same parent order, in play and in replay. Without the
// commits under the same parent order, in play. Without the
// scope reset, the second evaluation fails with `ParentOrderReused` and has
// no effect.
#[tokio::test]
async fn space_store_handle_evaluation_scope_resets_parent_orders() {
    let parent = OperationOrder {
        session: [6; 32],
        path: CausalPath::from(vec![(0, 0)]),
    };
    // Changed by decision 90 (C6, step D): the replay half of this test retires
    // with the rigged replay modes (decision 125), so the test keeps only its play
    // space. The text before:
    // ```
    // let (play, replay) = play_and_replay().await;
    // let start = checkpoint(&play).await;
    // rig(&replay, &start.root, &[]).await;
    // ```
    let play = play_space().await;
    let play_recorder = Recorder::new();
    observed(&play, &play_recorder);
    // Changed by decision 90 (C6, step D): the replay half of this test retires
    // with the rigged replay modes (decision 125). The text before:
    // ```
    // let replay_recorder = Recorder::new();
    // observed(&replay, &replay_recorder);
    // ```
    for (handle, recorder) in [
        (SpaceStoreHandle::new(play.clone()), &play_recorder),
        // Changed by decision 90 (C6, step D): the replay half of this test retires
        // with the rigged replay modes (decision 125). The text before:
        // ```
        // (SpaceStoreHandle::new(replay.clone()), &replay_recorder),
        // ```
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
    // Changed by decision 90 (C6, step D): the replay half of this test retires
    // with the rigged replay modes (decision 125). The text before:
    // assert_eq!(replay.get_data(&s("a")).await.len(), 2);
}

// FR-5': a reset or a revert replaces the host's hot store, and the first
// commit against the new store starts a new tracking scope. A second
// Changed by decision 90 (C6, step D): the replay half of this test retires
// with the rigged replay modes (decision 125). The text before:
// ```
// // evaluation of one deploy after a soft-checkpoint revert (play) or a reset
// // (replay) commits under the same parent order; without a store change, the
// // next evaluation under that order fails with no effect.
// ```
// evaluation of one deploy after a soft-checkpoint revert commits under the
// same parent order. Without a store change, the next evaluation under that
// order fails with no effect.
#[tokio::test]
async fn space_store_handle_store_reset_resets_parent_orders() {
    let parent = OperationOrder {
        session: [7; 32],
        path: CausalPath::from(vec![(1, 0)]),
    };
    // Changed by decision 90 (C6, step D): the replay half of this test retires
    // with the rigged replay modes (decision 125), so the test keeps only its play
    // space. The text before:
    // ```
    // let (play, replay) = play_and_replay().await;
    // let start = checkpoint(&play).await;
    // ```
    let play = play_space().await;

    let handle = SpaceStoreHandle::new(play.clone());
    let soft = play.create_soft_checkpoint().await;
    assert_eq!(evaluate(&handle, &parent, "x").await, Ok(1));
    play.revert_to_soft_checkpoint(soft)
        .await
        .expect("revert to the soft checkpoint");
    assert_eq!(evaluate(&handle, &parent, "x").await, Ok(1));
    assert_eq!(evaluate(&handle, &parent, "y").await, Err(SpaceTxnError::ParentOrderReused));
    assert_eq!(play.get_data(&s("a")).await, vec![datum("a", "x", false)]);

    // Retired by decision 90 (C6, step D): the replay half ran through the
    // rigged replay modes, which retire (decision 125). Native replay of the
    // store is tested by `native_space_replay_rejects_a_tampered_candidate` and
    // its siblings in `native_session/space.rs`, and by
    // `space_native_replay_equal_*`. The text before:
    // ```
    // rig(&replay, &start.root, &[]).await;
    // let handle = SpaceStoreHandle::new(replay.clone());
    // assert_eq!(evaluate(&handle, &parent, "x").await, Ok(1));
    // replay
    //     .reset(&start.root)
    //     .await
    //     .expect("reset the replay space");
    // assert_eq!(evaluate(&handle, &parent, "x").await, Ok(1));
    // assert_eq!(evaluate(&handle, &parent, "y").await, Err(SpaceTxnError::ParentOrderReused));
    // assert_eq!(replay.get_data(&s("a")).await, vec![datum("a", "x", false)]);
    // ```
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
        own: false,
        // Added by decision 123 (plan v4, V3-9): no row search.
        search: None,
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
        own: false,
        // Added by decision 123 (plan v4, V3-9): no row search.
        search: None,
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
        own: false,
        // Added by decision 123 (plan v4, V3-9): no row search.
        search: None,
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
            own: false,
            // Added by decision 123 (plan v4, V3-9): no row search.
            search: None,
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
    assert_eq!(
        one.commit_serializations, many.commit_serializations,
        "commit serializations: {costs:?}"
    );
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
        own: false,
        // Added by decision 123 (plan v4, V3-9): no row search.
        search: None,
    }
}

// The persistent flags of the results of a fire receipt; empty for a store
// or a park.
fn result_flags(receipt: &SpaceOpReceipt<String, Pat, String, String>) -> Vec<bool> {
    match receipt {
        SpaceOpReceipt::Fired { results, .. } => {
            results.iter().map(|result| result.persistent).collect()
        }
        // Changed by decision 123 (plan v4 C3d): a skipped row has no results.
        // The text before:
        // ```
        // SpaceOpReceipt::Stored { .. } | SpaceOpReceipt::Parked { .. } => Vec::new(),
        // ```
        SpaceOpReceipt::Stored { .. } |
        SpaceOpReceipt::Parked { .. } |
        SpaceOpReceipt::Skipped { .. } => Vec::new(),
    }
}

// A name for a peek waiter on `channel` that comes before the taking waiter
// `taking` in the canonical candidate order (candidate_order.rs, DR-75): by the
// hash of their source consume, then by the digest of their bincode bytes only
// when two source hashes are equal. The two waiters have different
// continuations, so their source hashes differ. Since DR-129 host play picks a
// waiter at random, so the tests of the peek re-offer draw host plays until the
// host fires this waiter first (`HOST_DRAWS`); replay keeps the canonical
// order.
fn peek_tried_first(channel: &str, taking: &str) -> String {
    let order = |waiting: &WaitingContinuation<Pat, String>| waiting.source.hash.clone();
    let taking = order(&waiter(&[channel], taking, false));
    (0..64)
        .map(|index| format!("peek-{}", index))
        .find(|name| order(&waiter(&[channel], name, true)) < taking)
        .expect("a peek waiter that the host tries first")
}

// The most host plays that a test of the peek re-offer draws before the host
// fires the peek waiter first (decision 89, DR-129). Each play picks one of two
// waiters, so all 64 draws miss with probability 2^-64.
const HOST_DRAWS: usize = 64;

// True when the host outcome `outcome` of a produce fires the continuation
// `continuation`.
fn fires_continuation(outcome: &HostOutcome, continuation: &str) -> bool {
    matches!(outcome, HostOutcome::Produced(Some((fired, _, _))) if fired.continuation == continuation)
}

// An introduction-authority registry with the rule of
// accounting/mod.rs:1041-1063: the first registration of an identity keeps
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

// Decision 45 (Rule E, applied everywhere) with FR-10 (P4 amendment A1): the
// produce-side fire of a peek join removes its stored non-persistent
// candidate e, as the host's `remove_matched_datum_and_join` does, and never
// stores its incoming datum r, not in the staged view and not in the
// Changed by decision 90 (C6, step D): the replay half of this test retires
// with the rigged replay modes (decision 125). The text before:
// ```
// // committed store, in play and in replay. The receipt keeps the host's flags.
// ```
// committed store, in play. The receipt keeps the host's flags.
// The state, the events and the root equal those of the host produce before
// the reducer's re-produces, which the agent stages as the next produces
// (see `space_store_handle_peek_fire_receipt_flags`). Without the re-store
// and the re-offer, neither record is in any store.
#[tokio::test]
async fn space_store_handle_produce_peek_join_removes_stored_and_leaves_incoming_unstored() {
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
            own: false,
            // Added by decision 123 (plan v4, V3-9): no row search.
            search: None,
        })
        .expect("stage the peek fire");
        assert!(txn.data(&s("b")).expect("read b").is_empty());
        assert!(txn.data(&s("v1")).expect("read v1").is_empty());
        assert!(
            txn.continuations(&names(&["b", "v1"]))
                .expect("read [b, v1]")
                .is_empty()
        );
        for channel in ["b", "v1"] {
            assert!(txn.joins(&s(channel)).expect("read the joins").is_empty());
        }
        vec![txn.commit().await]
    }

    let host = play_space().await;
    run_host_ops(&host, &[
        Op::produce("b", "e", false),
        Op::peek(&["b", "v1"], vec![Pat::Any, Pat::Any], "kbv"),
        Op::produce("v1", "r", false),
    ])
    .await;
    let play = play_space().await;
    // Changed by decision 90 (C6, step D): the root lost its reader with the replay
    // half, and the checkpoint stays because it empties the event log. The text
    // before:
    // let start = checkpoint(&play).await;
    checkpoint(&play).await;
    let played = receipts_ok(run(&SpaceStoreHandle::new(play.clone())).await);
    assert_eq!(played[0].ops.iter().map(result_flags).collect::<Vec<_>>(), vec![
        vec![],
        vec![],
        vec![false, false]
    ]);
    for channel in ["b", "v1"] {
        assert!(play.get_data(&s(channel)).await.is_empty());
        assert!(play.get_joins(s(channel)).await.is_empty());
    }
    assert_eq!(logical_state(&play), logical_state(&host));
    let log = checkpoint(&play).await;
    let head = checkpoint(&host).await;
    assert_eq!(kinds(&log.log), vec!["produce", "consume", "produce", "comm"]);
    assert_eq!(event_bytes(&log.log), event_bytes(&head.log));
    assert_eq!(log.root, head.root);

    // Retired by decision 90 (C6, step D): the replay half ran through the
    // rigged replay modes, which retire (decision 125). Native replay of the
    // store is tested by `native_space_replay_rejects_a_tampered_candidate` and
    // its siblings in `native_session/space.rs`, and by
    // `space_native_replay_equal_*`. The text before:
    // ```
    // let replay = replay_over(&play);
    // rig(&replay, &start.root, &log.log).await;
    // let replayed = receipts_ok(run(&SpaceStoreHandle::new(replay.clone())).await);
    // assert_eq!(replayed, played);
    // for channel in ["b", "v1"] {
    //     assert!(replay.get_data(&s(channel)).await.is_empty());
    // }
    // replay
    //     .check_replay_data()
    //     .await
    //     .expect("replay pairs the peek COMM");
    // assert_eq!(checkpoint(&replay).await.root, log.root);
    // ```
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
    // Changed by decision 89 (DR-129): host play picks one of the two waiters
    // at random, so the test draws twins until HEAD fires the peek waiter, the
    // play of FR-10.
    // let twin = twin().await;
    // run_host_ops(&twin.host, &parks).await;
    // let fired = run_host(&twin.host, &send).await.expect("the host produce");
    let mut drawn = None;
    for _ in 0..HOST_DRAWS {
        let twin = twin().await;
        run_host_ops(&twin.host, &parks).await;
        let fired = run_host(&twin.host, &send).await.expect("the host produce");
        if fires_continuation(&fired, &peeking) {
            drawn = Some((twin, fired));
            break;
        }
    }
    let (twin, fired) = drawn.expect("a host play that fires the peek waiter first");
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

// Changed by decision 90 (C6, step D): the replay and the reporting of the
// transactions retire with the rigged replay modes (decision 125). The text
// before:
// ```
// // FR-10 in replay and reporting: transactions of a peek fire and its
// // re-offer replay their play log, in replay and in reporting, with the play
// // receipts and root. While the peek fire is staged, the recorded COMMs of
// // the produce identity offer only the COMM of the re-offer. Host replay of
// // the same log with HEAD's operations (the re-produces included) reaches
// // the same root, and HEAD's play log replays through the same transactions.
// ```
// FR-10: transactions of a peek fire and its re-offer play the log and the root
// of HEAD's operations (the re-produces included), and host replay of that log
// with HEAD's operations reaches the same root.
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
    // Changed by decision 89 (DR-129): host play picks one of the two waiters
    // of `d` at random, so the test draws plans until the planner fires the
    // peek waiter first, the play of FR-10 with a re-offer (`[2, 2, 1, 2]`).
    // let plans = plan_groups(&groups).await;
    // assert_eq!(plans.iter().map(Vec::len).collect::<Vec<_>>(), vec![2, 2, 1, 2]);
    let mut drawn = None;
    for _ in 0..HOST_DRAWS {
        let plans = plan_groups(&groups).await;
        if plans.iter().map(Vec::len).collect::<Vec<_>>() == vec![2, 2, 1, 2] {
            drawn = Some(plans);
            break;
        }
    }
    let plans = drawn.expect("a planner play that fires the peek waiter first");
    // Changed by decision 90 (C6, step D): the replay through transactions retires
    // with the rigged replay modes (decision 125), so the test takes only a play
    // space. The text before:
    // let (play, replay) = play_and_replay().await;
    let play = play_space().await;
    let start = checkpoint(&play).await;
    // Changed by decision 90 (C6, step D): the receipts lose their readers with the
    // replay half, and the call stays because it plays the transactions. The text
    // before:
    // ```
    // let played_receipts =
    //     receipts_ok(run_plans(&SpaceStoreHandle::new(play.clone()), &plans).await);
    // ```
    receipts_ok(run_plans(&SpaceStoreHandle::new(play.clone()), &plans).await);
    assert_eq!(play.get_data(&s("a")).await, vec![datum("a", "e", false)]);
    let played = checkpoint(&play).await;
    let comms = comms_of(&played.log);
    assert_eq!(comms.len(), 3);
    assert_eq!(kinds(&played.log), vec![
        "consume", "consume", "produce", "comm", "produce", "comm", "consume", "produce", "comm",
        "produce"
    ]);

    // Retired by decision 90 (C6, step D): the replay half ran through the rigged
    // replay modes, which retire (decision 125). Native replay of the store is
    // tested by `native_space_replay_rejects_a_tampered_candidate` and its siblings
    // in `native_session/space.rs`, and by `space_native_replay_equal_*`. The text
    // before:
    // ```
    // rig(&replay, &start.root, &played.log).await;
    // let handle = SpaceStoreHandle::new(replay.clone());
    // let mut replayed = run_plans(&handle, &plans[..1]).await;
    // let d = produce_source("a", "d", false);
    // let mut txn = handle.begin(&universe()).await.expect("begin");
    // let recorded = |txn: &mut Txn| {
    //     txn.recorded_comms(RSpaceOperationSource::Produce(&d))
    //         .expect("recorded COMMs in replay")
    // };
    // assert_eq!(recorded(&mut txn).len(), 2);
    // plans[1][0]
    //     .stage_expected(&mut txn)
    //     .expect("stage the peek fire");
    // assert_eq!(recorded(&mut txn), vec![comms[1].clone()]);
    // plans[1][1]
    //     .stage_expected(&mut txn)
    //     .expect("stage the re-offer");
    // assert!(recorded(&mut txn).is_empty());
    // replayed.push(txn.commit().await);
    // replayed.extend(run_plans(&handle, &plans[2..]).await);
    // assert_eq!(receipts_ok(replayed), played_receipts);
    // replay
    //     .check_replay_data()
    //     .await
    //     .expect("replay pairs every COMM");
    // assert_eq!(checkpoint(&replay).await.root, played.root);
    //
    // let reporting = reporting_over(&play);
    // reporting
    //     .rig_and_reset(start.root.clone(), played.log.clone())
    //     .await
    //     .expect("rig the reporting space");
    // let reported = receipts_ok(run_plans(&SpaceStoreHandle::new(reporting.clone()), &plans).await);
    // assert_eq!(reported, played_receipts);
    // reporting
    //     .check_replay_data()
    //     .await
    //     .expect("reporting pairs every COMM");
    // let reported_comms = reporting
    //     .get_report()
    //     .expect("the report")
    //     .iter()
    //     .flat_map(|batch| batch.events.iter())
    //     .filter(|event| matches!(event, ReportingEvent::ReportingComm(_)))
    //     .count();
    // assert_eq!(reported_comms, 3);
    // let reported_root = ISpace::create_checkpoint(&reporting)
    //     .await
    //     .expect("checkpoint the reporting space")
    //     .root;
    // assert_eq!(reported_root, played.root);
    // ```

    let host_replay = replay_over(&play);
    rig(&host_replay, &start.root, &played.log).await;
    run_host_ops(&host_replay, &head_ops).await;
    host_replay
        .check_replay_data()
        .await
        .expect("host replay pairs every COMM of the transaction log");
    assert_eq!(checkpoint(&host_replay).await.root, played.root);

    // Changed by decision 89 (DR-129): HEAD's play of `head_ops` picks one of
    // the two waiters of `d` at random, so the test draws plays until HEAD fires
    // the peek waiter first. Only that play has the log of the transactions.
    // let (head_play, txn_replay) = play_and_replay().await;
    // let head_start = checkpoint(&head_play).await;
    // run_host_ops(&head_play, &head_ops).await;
    let mut drawn = None;
    for _ in 0..HOST_DRAWS {
        // Changed by decision 90 (C6, step D): the replay of the log of HEAD through
        // transactions retires with the rigged replay modes (decision 125), and the
        // checkpoint stays because it empties the event log. The text before:
        // ```
        // let (head_play, txn_replay) = play_and_replay().await;
        // let head_start = checkpoint(&head_play).await;
        // ```
        let head_play = play_space().await;
        checkpoint(&head_play).await;
        run_host_ops(&head_play, &head_ops[..2]).await;
        let fired = run_host(&head_play, &head_ops[2])
            .await
            .expect("HEAD's produce of d");
        if fires_continuation(&fired, &peeking) {
            run_host_ops(&head_play, &head_ops[3..]).await;
            // Changed by decision 90 (C6, step D): the draw keeps only the play space. The
            // text before:
            // drawn = Some((head_play, txn_replay, head_start));
            drawn = Some(head_play);
            break;
        }
    }
    // Changed by decision 90 (C6, step D): the draw keeps only the play space. The
    // text before:
    // ```
    // let (head_play, txn_replay, head_start) =
    //     drawn.expect("a HEAD play that fires the peek waiter first");
    // ```
    let head_play = drawn.expect("a HEAD play that fires the peek waiter first");
    let head_played = checkpoint(&head_play).await;
    assert_eq!(event_bytes(&head_played.log), event_bytes(&played.log));
    assert_eq!(head_played.root, played.root);
    // Retired by decision 90 (C6, step D): the replay half ran through the
    // rigged replay modes, which retire (decision 125). Native replay of the
    // store is tested by `native_space_replay_rejects_a_tampered_candidate` and
    // its siblings in `native_session/space.rs`, and by
    // `space_native_replay_equal_*`. The text before:
    // ```
    // rig(&txn_replay, &head_start.root, &head_played.log).await;
    // let txn_replayed =
    //     receipts_ok(run_plans(&SpaceStoreHandle::new(txn_replay.clone()), &plans).await);
    // assert_eq!(txn_replayed, played_receipts);
    // txn_replay
    //     .check_replay_data()
    //     .await
    //     .expect("transaction replay pairs every COMM of HEAD's log");
    // assert_eq!(checkpoint(&txn_replay).await.root, played.root);
    // ```
}

// Receipt flags under decision 45 (Rule E): every result of a fire keeps the
// flag of the host's `wrap_result`, also for a peek fire. A stored
// non-persistent record of a peek fire is reported non-persistent, because
// the fire removes it, and the agent stores it again (the re-store). The
// incoming datum of a produce-side peek fire keeps its own flag: false for a
// non-persistent send, which the agent offers again, and true for a
// persistent send. A fire that does not peek also reports the own flag of
// every record. Each re-store and re-offer is a store with the identity of
// its record. Staged from HEAD's outcomes and HEAD's re-produces, the space
// keeps host parity throughout.
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
    let receipts = receipts_ok(receipts);
    let flags: Vec<Vec<Vec<bool>>> = receipts
        .iter()
        .map(|receipt| receipt.ops.iter().map(result_flags).collect())
        .collect();
    assert_eq!(flags, vec![
        vec![vec![], vec![], vec![]],
        // A consume-side peek reports the removed x as non-persistent, and
        // its re-store stores x.
        vec![vec![false], vec![]],
        vec![vec![]],
        // A produce-side peek: the incoming q keeps false; its re-offer
        // stores q.
        vec![vec![false], vec![]],
        vec![vec![]],
        // A peek join reports the removed e and the incoming r as
        // non-persistent. In result order, the re-store stores e and the
        // re-offer stores r.
        vec![vec![false, false], vec![], vec![]],
        vec![vec![]],
        // A persistent send keeps its own flag, true, and is not stored.
        vec![vec![true]],
        // Fires that do not peek keep the own flags.
        vec![vec![true]],
        vec![vec![false]],
    ]);
    let stored_receipt = |channel: &str, data: &str| SpaceOpReceipt::Stored {
        produce: produce_source(channel, data, false),
    };
    assert_eq!(receipts[1].ops[1], stored_receipt("a", "x"));
    assert_eq!(receipts[3].ops[1], stored_receipt("d", "q"));
    assert_eq!(receipts[5].ops[1..], [stored_receipt("b", "e"), stored_receipt("v1", "r")]);
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

// HEAD's program for `space_store_handle_peek_restore_equals_head_reproduce`:
// a peek on x and a peek join on p and y, each one followed by the reducer's
// re-produce of the non-persistent record that it removed (`produce_peeks`),
// then a consume of x. The fire does not remove the persistent p, so the
// reducer does not produce p again.
fn head_restore_ops() -> Vec<Op> {
    vec![
        Op::produce("a", "x", false),
        Op::produce("a", "y", false),
        Op::produce("b", "p", true),
        Op::peek(&["a"], vec![Pat::Exact(s("x"))], "k1"),
        Op::produce("a", "x", false),
        Op::peek(&["b", "a"], vec![Pat::Any, Pat::Exact(s("y"))], "k2"),
        Op::produce("a", "y", false),
        Op::consume(&["a"], vec![Pat::Exact(s("x"))], "k3", false),
    ]
}

// The space program of the same deploy, in three transactions: the stores,
// then the two peek fires, each one followed by the agent's re-store of the
// record that it removed (decision 45, Rule E), then the consume of x. The
// staged view shows each removal and each re-store.
async fn run_peek_restores(handle: &Handle) -> Commits {
    let (x, y, p) = (datum("a", "x", false), datum("a", "y", false), datum("b", "p", true));
    let mut commits = Vec::with_capacity(3);
    let mut txn = handle.begin(&universe()).await.expect("begin");
    txn.produce(store_op("a", "x", false)).expect("stage a!x");
    txn.produce(store_op("a", "y", false)).expect("stage a!y");
    txn.produce(store_op("b", "p", true)).expect("stage b!!p");
    commits.push(txn.commit().await);

    let mut txn = handle.begin(&universe()).await.expect("begin");
    assert_eq!(txn.data(&s("a")).expect("read a"), &[y.clone(), x.clone()]);
    txn.consume(peek_consume_op(&["a"], vec![Pat::Exact(s("x"))], "k1", vec![bind_candidate(
        s("a"),
        &x,
        s("x"),
        1,
    )]))
    .expect("stage the peek on x");
    assert_eq!(txn.data(&s("a")).expect("read a"), std::slice::from_ref(&y));
    txn.produce(store_op("a", "x", false))
        .expect("stage the re-store of x");
    assert_eq!(txn.data(&s("a")).expect("read a"), &[x.clone(), y.clone()]);
    txn.consume(peek_consume_op(&["b", "a"], vec![Pat::Any, Pat::Exact(s("y"))], "k2", vec![
        bind_candidate(s("b"), &p, s("p"), 0),
        bind_candidate(s("a"), &y, s("y"), 1),
    ]))
    .expect("stage the peek join on p and y");
    assert_eq!(txn.data(&s("b")).expect("read b"), std::slice::from_ref(&p));
    assert_eq!(txn.data(&s("a")).expect("read a"), std::slice::from_ref(&x));
    txn.produce(store_op("a", "y", false))
        .expect("stage the re-store of y");
    commits.push(txn.commit().await);

    let mut txn = handle.begin(&universe()).await.expect("begin");
    assert_eq!(txn.data(&s("a")).expect("read a"), &[y.clone(), x.clone()]);
    txn.consume(fire_consume_op(&["a"], vec![Pat::Exact(s("x"))], "k3", vec![bind_candidate(
        s("a"),
        &x,
        s("x"),
        1,
    )]))
    .expect("stage the consume of x");
    commits.push(txn.commit().await);
    commits
}

// Decision 45 (Rule E), host parity of the re-store: HEAD's consume with a
// peek removes each bound non-persistent record, and the reducer produces it
// again (`produce_peeks`, reduce.rs:981-1016). The space stages the peek
// fire, which removes the record as the host COMM does, and right after it,
// in the same transaction, the agent's re-store: a store of the record's own
// data, with the record's identity. The peek join also binds the persistent
// Changed by decision 90 (C6, step D): the replay and the reporting of these
// transactions retire with the rigged replay modes (decision 125). The text
// before:
// ```
// // p, which its fire does not remove, so only y is stored again. In play,
// // replay and reporting, the space and HEAD have the same events in the same
// // order, the same observer calls, the same produce counters and the same
// // roots. The later consume of x counts both produces of x.
// ```
// p, which its fire does not remove, so only y is stored again. In play, the
// space and HEAD have the same events in the same order, the same observer
// calls, the same produce counters and the same roots. The later consume of x
// counts both produces of x.
#[tokio::test]
async fn space_store_handle_peek_restore_equals_head_reproduce() {
    let head_ops = head_restore_ops();
    let x = produce_source("a", "x", false);
    let y = produce_source("a", "y", false);
    let p = produce_source("b", "p", true);
    let sources = [x.clone(), y.clone(), p.clone()];
    let counters = BTreeMap::from([(x.clone(), 2), (y.clone(), 2), (p.clone(), 0)]);

    // Play: the transactions against HEAD's program.
    let twin = twin().await;
    // Changed by decision 90 (C6, step D): the root lost its reader with the replay
    // half, and the checkpoint stays because it empties the event log. The text
    // before:
    // let start = checkpoint(&twin.txn).await;
    checkpoint(&twin.txn).await;
    checkpoint(&twin.host).await;
    run_host_ops(&twin.host, &head_ops).await;
    let txn_handle = SpaceStoreHandle::new(twin.txn.clone());
    let played_receipts = receipts_ok(run_peek_restores(&txn_handle).await);
    let flags: Vec<Vec<Vec<bool>>> = played_receipts
        .iter()
        .map(|receipt| receipt.ops.iter().map(result_flags).collect())
        .collect();
    assert_eq!(flags, vec![
        vec![vec![], vec![], vec![]],
        vec![vec![false], vec![], vec![true, false], vec![]],
        vec![vec![false]],
    ]);
    match &played_receipts[1].ops[..] {
        [
            SpaceOpReceipt::Fired {
                comm: peek,
                produce: None,
                ..
            },
            SpaceOpReceipt::Stored {
                produce: restored_x,
            },
            SpaceOpReceipt::Fired {
                comm: join,
                produce: None,
                ..
            },
            SpaceOpReceipt::Stored {
                produce: restored_y,
            },
        ] => {
            assert_eq!(peek.peeks, BTreeSet::from([0]));
            assert_eq!(peek.times_repeated, BTreeMap::from([(x.clone(), 1)]));
            assert_eq!(restored_x, &x);
            assert_eq!(join.peeks, BTreeSet::from([0, 1]));
            assert_eq!(join.times_repeated, BTreeMap::from([(p.clone(), 0), (y.clone(), 1)]));
            assert_eq!(restored_y, &y);
        }
        other => panic!("expected each peek fire with its re-store, got {:?}", other),
    }
    match &played_receipts[2].ops[..] {
        [SpaceOpReceipt::Fired { comm, .. }] => {
            assert_eq!(comm.times_repeated, BTreeMap::from([(x.clone(), 2)]));
        }
        other => panic!("expected the consume of x, got {:?}", other),
    }
    assert_eq!(counters_at(&txn_handle, &sources).await, counters);
    assert_eq!(counters_at(&SpaceStoreHandle::new(twin.host.clone()), &sources).await, counters);
    assert_eq!(logical_state(&twin.txn), logical_state(&twin.host));
    // Changed by decision 90 (C6, step D): the log of HEAD lost its reader with the
    // replay half. The text before:
    // let (head_played, played) = assert_parity(&twin).await;
    let (_, played) = assert_parity(&twin).await;
    assert_eq!(kinds(&played.log), vec![
        "produce", "produce", "produce", "consume", "comm", "produce", "consume", "comm",
        "produce", "consume", "comm"
    ]);

    // Retired by decision 90 (C6, step D): the replay half ran through the
    // rigged replay modes, which retire (decision 125). Native replay of the
    // store is tested by `native_space_replay_rejects_a_tampered_candidate` and
    // its siblings in `native_session/space.rs`, and by
    // `space_native_replay_equal_*`. The text before:
    // ```
    // // Replay: the transactions pair the play log, and host replay of HEAD's
    // // program pairs HEAD's log, which is the same log.
    // let txn_replay = replay_over(&twin.txn);
    // let txn_replay_recorder = Recorder::new();
    // observed(&txn_replay, &txn_replay_recorder);
    // rig(&txn_replay, &start.root, &played.log).await;
    // let txn_replay_handle = SpaceStoreHandle::new(txn_replay.clone());
    // assert_eq!(receipts_ok(run_peek_restores(&txn_replay_handle).await), played_receipts);
    // let host_replay = replay_over(&twin.host);
    // let host_replay_recorder = Recorder::new();
    // observed(&host_replay, &host_replay_recorder);
    // rig(&host_replay, &start.root, &head_played.log).await;
    // run_host_ops(&host_replay, &head_ops).await;
    // let replay_calls = txn_replay_recorder.calls();
    // assert!(
    //     replay_calls
    //         .iter()
    //         .any(|call| matches!(call, Call::Directive { .. }))
    // );
    // assert_eq!(replay_calls, host_replay_recorder.calls());
    // assert_eq!(without_directives(replay_calls.clone()), twin.txn_recorder.calls());
    // assert_eq!(counters_at(&txn_replay_handle, &sources).await, counters);
    // assert_eq!(counters_at(&SpaceStoreHandle::new(host_replay.clone()), &sources).await, counters);
    // assert_eq!(replay_state(&txn_replay), replay_state(&host_replay));
    // for replay in [&txn_replay, &host_replay] {
    //     replay
    //         .check_replay_data()
    //         .await
    //         .expect("replay pairs every COMM");
    //     assert_eq!(checkpoint(replay).await.root, played.root);
    // }
    //
    // // Reporting: the transactions report what HEAD's program reports, event
    // // for event, with the calls of replay.
    // let reporting = reporting_over(&twin.txn);
    // let reporting_recorder = Recorder::new();
    // observed(&reporting, &reporting_recorder);
    // let by_host = reporting_over(&twin.host);
    // let by_host_recorder = Recorder::new();
    // observed(&by_host, &by_host_recorder);
    // reporting
    //     .rig_and_reset(start.root.clone(), played.log.clone())
    //     .await
    //     .expect("rig the reporting space");
    // by_host
    //     .rig_and_reset(start.root.clone(), head_played.log.clone())
    //     .await
    //     .expect("rig the host reporting space");
    // let reporting_handle = SpaceStoreHandle::new(reporting.clone());
    // assert_eq!(receipts_ok(run_peek_restores(&reporting_handle).await), played_receipts);
    // run_host_ops(&by_host, &head_ops).await;
    // let report = reporting.get_report().expect("the report");
    // assert!(!report.is_empty());
    // assert_eq!(report, by_host.get_report().expect("the host report"));
    // assert_eq!(reporting_recorder.calls(), by_host_recorder.calls());
    // assert_eq!(reporting_recorder.calls(), replay_calls);
    // assert_eq!(counters_at(&reporting_handle, &sources).await, counters);
    // assert_eq!(counters_at(&SpaceStoreHandle::new(by_host.clone()), &sources).await, counters);
    // for space in [&reporting, &by_host] {
    //     space
    //         .check_replay_data()
    //         .await
    //         .expect("reporting pairs every COMM");
    //     let root = ISpace::create_checkpoint(space)
    //         .await
    //         .expect("checkpoint the reporting space")
    //         .root;
    //     assert_eq!(root, played.root);
    // }
    // ```
}

// Negative control for decision 45 (Rule E): the substrate change and the
// agent's re-store must land together. If a peek fire kept its record (the
// retention that Rule E deletes) and the agent stored the record again
// anyway, the store would hold two byte-identical copies, because the hot
// store does not deduplicate data (`HotStore::put_datum` inserts at index 0,
// hot_store.rs:879-897). The test builds that wrong state through the host
// store API: x stays where a retaining fire left it, and the `put_datum` of
// the re-store adds a second copy. Two consumes then both take x, so x would
// be delivered twice, and the root differs from HEAD's. The other half fails
// too: a fire that removes x with no re-store after it loses x. The Rule E
// path, whose fire removes x before the re-store, ends with exactly one copy,
// with HEAD's state and root, so only the first of the two consumes takes x.
#[tokio::test]
async fn space_store_handle_restore_without_removal_duplicates_record() {
    let x = datum("a", "x", false);
    let x_bytes = bincode::serialize(&x).expect("serialize x");
    let copies = |records: &[Datum<String>]| {
        records
            .iter()
            .filter(|record| bincode::serialize(record).expect("serialize a datum") == x_bytes)
            .count()
    };

    let head = play_space().await;
    run_host_ops(&head, &[
        Op::produce("a", "x", false),
        Op::peek(&["a"], vec![Pat::Any], "k"),
        Op::produce("a", "x", false),
    ])
    .await;
    assert_eq!(copies(&head.get_data(&s("a")).await), 1);

    let peek_fire = || {
        peek_consume_op(&["a"], vec![Pat::Any], "k", vec![bind_candidate(s("a"), &x, s("x"), 0)])
    };
    let restored = play_space().await;
    let mut txn = SpaceStoreHandle::new(restored.clone())
        .begin(&universe())
        .await
        .expect("begin");
    txn.produce(store_op("a", "x", false)).expect("stage a!x");
    txn.consume(peek_fire()).expect("stage the peek fire");
    assert_eq!(copies(txn.data(&s("a")).expect("read a")), 0, "the peek fire removes x");
    txn.produce(store_op("a", "x", false))
        .expect("stage the re-store of x");
    assert_eq!(copies(txn.data(&s("a")).expect("read a")), 1);
    txn.commit()
        .await
        .expect("commit the fire and the re-store");
    assert_eq!(copies(&restored.get_data(&s("a")).await), 1, "Rule E leaves one copy");
    assert_eq!(logical_state(&restored), logical_state(&head));

    let removed_only = play_space().await;
    let mut txn = SpaceStoreHandle::new(removed_only.clone())
        .begin(&universe())
        .await
        .expect("begin");
    txn.produce(store_op("a", "x", false)).expect("stage a!x");
    txn.consume(peek_fire()).expect("stage the peek fire");
    txn.commit()
        .await
        .expect("commit the fire without a re-store");
    assert_eq!(copies(&removed_only.get_data(&s("a")).await), 0, "a removal alone loses x");

    let duplicated = play_space().await;
    run_host_ops(&duplicated, &[Op::produce("a", "x", false)]).await;
    duplicated.get_store().put_datum(&s("a"), x.clone());
    let records = duplicated.get_data(&s("a")).await;
    assert_eq!(records.len(), 2);
    assert_eq!(copies(&records), 2, "a re-store without the removal leaves two copies");

    let head_root = checkpoint(&head).await.root;
    assert_eq!(checkpoint(&restored).await.root, head_root);
    assert_ne!(checkpoint(&removed_only).await.root, head_root);
    assert_ne!(checkpoint(&duplicated).await.root, head_root);
    for (space, deliveries) in [(&head, 1), (&restored, 1), (&removed_only, 0), (&duplicated, 2)] {
        let mut taken = 0usize;
        for continuation in ["k1", "k2"] {
            let consume = Op::consume(&["a"], vec![Pat::Any], continuation, false);
            match run_host(space, &consume).await.expect("run a consume") {
                HostOutcome::Consumed(Some(_)) => taken += 1,
                HostOutcome::Consumed(None) | HostOutcome::Produced(_) => {}
            }
        }
        assert_eq!(taken, deliveries, "the consumes that take x");
    }
}

// Added by decision 124 (plan of the batch B tests, B19, ruling RADD2v2 6.6
// (f), U5): an observer of play with the owner checks on. So the commit runs
// the row search of a staged fire (C2v3 6.13 (b)).
struct Funded;

impl RSpaceAccountingObserver<String, Pat, String, String> for Funded {
    fn observe_produce(
        &self,
        _source: &Produce,
        _channel: &String,
        _data: &String,
        _persistent: bool,
    ) -> Result<(), RSpaceError> {
        Ok(())
    }

    fn observe_consume(
        &self,
        _source: &Consume,
        _channels: &[String],
        _patterns: &[Pat],
        _continuation: &String,
        _persistent: bool,
        _peeks: &BTreeSet<i32>,
    ) -> Result<(), RSpaceError> {
        Ok(())
    }

    fn observe_comm(
        &self,
        _comm: &COMM,
        _continuation: &String,
        _continuation_persistent: bool,
        _data: &[(&String, bool)],
    ) -> Result<(), RSpaceError> {
        Ok(())
    }

    fn owner_checks_active(&self) -> bool { true }
}

// Added by decision 124 (plan of the batch B tests, B19, ruling RADD2v2 6.6
// (f), U5): a scripted search of a produce of x on a that the staging fired on
// k1. Its one run finds k2, so the row deviates (C2v3 6.13 (d) step 6). The
// actual fire retires r on c. The claims of the unit name a, and the search
// hands out `finish` once.
struct FinishSearch {
    finish: Vec<FinishRow<String, Pat, String, String>>,
}

impl RowSearch<String, Pat, String, String> for FinishSearch {
    fn runs(&self) -> Vec<RowRun<String>> {
        vec![RowRun {
            channels: names(&["a"]),
            candidates: 0..1,
        }]
    }

    fn continuations(
        &mut self,
        _run: usize,
        lists: &RowLists<'_, String, Pat, String, String>,
    ) -> Result<Vec<(Arc<WaitingContinuation<Pat, String>>, i32)>, SpaceTxnError> {
        let k2 = consume_source(&["a"], &[Pat::Any], "k2", false);
        let mut waiters = lists.continuations(&names(&["a"]))?;
        let Some(at) = waiters.iter().position(|waiter| waiter.source == k2) else {
            return Ok(Vec::new());
        };
        let index = i32::try_from(at).expect("the index of k2 fits an i32");
        Ok(vec![(waiters.swap_remove(at), index)])
    }

    fn tuple(
        &mut self,
        _run: usize,
        _lists: &RowLists<'_, String, Pat, String, String>,
        _request: RowTupleRequest<'_, String, Pat, String, String>,
    ) -> Result<Option<Vec<Candidate>>, SpaceTxnError> {
        let x = incoming_candidate(s("a"), produce_source("a", "x", false), s("x"), false, s("x"));
        Ok(Some(vec![x]))
    }

    fn restore(&mut self) {}

    fn commit_check(
        &mut self,
        _request: &RowTupleRequest<'_, String, Pat, String, String>,
        _tuple: &[Candidate],
    ) -> Result<bool, SpaceTxnError> {
        Ok(true)
    }

    fn may_veto(&self, _matcher: &dyn Match<Pat, String, String>, _continuation: &String) -> bool {
        false
    }

    fn staged_retires(&self) -> usize { 0 }

    fn actual_fire(
        &mut self,
        _request: &RowTupleRequest<'_, String, Pat, String, String>,
        _tuple: &[Candidate],
    ) -> ActualFire<String, Pat, String, String> {
        let r = bind_candidate(s("c"), &datum("c", "r", false), s("r"), 0);
        ActualFire {
            retires: vec![fire_consume_op(&["c"], vec![Pat::Any], "taker", vec![r])],
            dispatched: None,
        }
    }

    fn item_failures(
        &self,
        _lists: &RowLists<'_, String, Pat, String, String>,
        _matcher: &dyn Match<Pat, String, String>,
        _rule: &dyn RowItemRule<String, String>,
    ) -> Result<Vec<(Blake2b256Hash, Blake2b256Hash)>, SpaceTxnError> {
        Ok(Vec::new())
    }

    fn actual_claims(&self) -> Option<RowClaims<String>> {
        Some(RowClaims {
            data: names(&["a"]),
            waiters: Vec::new(),
        })
    }

    fn finish_rows(&mut self) -> Vec<FinishRow<String, Pat, String, String>> {
        std::mem::take(&mut self.finish)
    }
}

// Added by decision 124 (plan of the batch B tests, B19, ruling RADD2v2 6.6
// (f), U5): play under owner funding over k1 and k2 on a, r on c and t on d.
// The commit holds the fire of x on a that the staging fired on k1, with the
// search `FinishSearch`, and a store on v1 after it.
async fn commit_with_finish(
    finish: Vec<FinishRow<String, Pat, String, String>>,
) -> (Space, Receipt) {
    let space = play_space().await;
    run_host_ops(&space, &[
        Op::consume(&["a"], vec![Pat::Any], "k1", false),
        Op::consume(&["a"], vec![Pat::Any], "k2", false),
        Op::produce("c", "r", false),
        Op::produce("d", "t", false),
    ])
    .await;
    space.set_accounting_observer(Some(Arc::new(Funded)));
    let k1 = consume_source(&["a"], &[Pat::Any], "k1", false);
    let waiters = space.get_waiting_continuations(names(&["a"])).await;
    let staged = waiters
        .iter()
        .find(|waiter| waiter.source == k1)
        .expect("k1 waits on a");
    let mut fire = fire_produce_op("a", "x", staged);
    fire.search = Some(Box::new(FinishSearch { finish }));
    let mut txn = SpaceStoreHandle::new(space.clone())
        .begin(&universe())
        .await
        .expect("begin");
    txn.produce(fire).expect("stage the fire of x");
    txn.produce(store_op("v1", "later", false))
        .expect("stage the store on v1");
    let receipt = txn.commit().await.expect("commit the unit");
    (space, receipt)
}

// Added by decision 124 (plan of the batch B tests, B19, ruling RADD2v2 6.6
// (f), U5): the unit loop of play commits the finish rows of a deviating row d
// after its group. Row d fires x on k2 in place of k1, its group retires r on
// c, and its finish rows store f on b and take t on d. The claims of the cut
// gain the channels of the finish rows. The store on v1 after the unit stays
// uncommitted. With no finish rows, the receipt holds row d and its retire
// only.
#[tokio::test]
async fn unit_loop_commits_the_finish_rows_after_the_group() {
    let t = bind_candidate(s("d"), &datum("d", "t", false), s("t"), 0);
    let take = fire_consume_op(&["d"], vec![Pat::Any], "taker", vec![t]);
    let finish = vec![FinishRow::Store(store_op("b", "f", false)), FinishRow::Take(take)];
    let (space, receipt) = commit_with_finish(finish).await;
    assert!(matches!(receipt.ops.as_slice(), [
        SpaceOpReceipt::Fired { .. },
        SpaceOpReceipt::Fired { .. },
        SpaceOpReceipt::Stored { .. },
        SpaceOpReceipt::Fired { .. },
    ]));
    let deviation = receipt.deviation.as_ref().expect("row d deviates");
    assert_eq!(deviation.index, 0);
    let claims = RowClaims {
        data: names(&["a", "b", "d"]),
        waiters: Vec::new(),
    };
    assert_eq!(deviation.claims, Some(claims));
    assert_eq!(space.get_data(&s("b")).await.len(), 1, "the store of f on b");
    assert!(space.get_data(&s("d")).await.is_empty(), "the take of t on d");
    assert!(space.get_data(&s("c")).await.is_empty(), "the retire of r on c");
    assert!(space.get_data(&s("v1")).await.is_empty(), "the store after the unit");
    let k1 = consume_source(&["a"], &[Pat::Any], "k1", false);
    let waiters = space.get_waiting_continuations(names(&["a"])).await;
    assert!(matches!(waiters.as_slice(), [waiter] if waiter.source == k1));

    let (space, receipt) = commit_with_finish(Vec::new()).await;
    assert!(matches!(receipt.ops.as_slice(), [
        SpaceOpReceipt::Fired { .. },
        SpaceOpReceipt::Fired { .. },
    ]));
    let deviation = receipt.deviation.as_ref().expect("row d deviates");
    assert_eq!(deviation.index, 0);
    let claims = RowClaims {
        data: names(&["a"]),
        waiters: Vec::new(),
    };
    assert_eq!(deviation.claims, Some(claims));
    assert!(space.get_data(&s("b")).await.is_empty(), "no store on b");
    assert_eq!(space.get_data(&s("d")).await.len(), 1, "t stays on d");
    assert!(space.get_data(&s("v1")).await.is_empty(), "the store after the unit");
}

// Added by decision 124 (T-C4-18, T-C4-25, T-C4-32, T-C4-33, T-C4-37,
// T-C4-38, T-P-1): the items of the scripted row search and of the scripted
// native replay of the tests below.
use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use rspace_plus_plus::rspace::internal::Install;
use rspace_plus_plus::rspace::replay_rspace::native_epoch::{
    NativeCandidateIdentity, NativeOperationEpoch, NativeOperationPublication,
    NativeOperationTicket, NativeOwnerFilter, NativeReplayBoundary, NativeReplayDecision,
    NativeReplayEpoch, NativeReplayOutcome, NativeReplayRestore, SpaceTransactionReadiness,
};
use rspace_plus_plus::rspace::replay_rspace::native_session::NativeReplaySession;
use rspace_plus_plus::rspace::replay_rspace::native_session::space::NativeSpaceHost;
use rspace_plus_plus::rspace::space_store_handle::{BoxedSpaceTxn, TailRead};

// Added by decision 124 (T-P-1, OP-TP1): the counts of a scripted row search:
// the records that its tuple searches walk, the item tests of its enumeration
// and its tail reads, with the records of the watched channel at each read.
#[derive(Default)]
struct SearchProbe {
    tuples: AtomicUsize,
    item_tests: AtomicUsize,
    tail_reads: AtomicUsize,
    tail_records: Mutex<Vec<usize>>,
}

// Added by decision 124 (T-C4-32, T-C4-33, T-C4-37, T-C4-38): a row search of
// a test. Its one run lists the waiters `listed` of its channels, by the store
// in that order, and then the waiters `extra`. A tuple search answers `tuple`,
// with `fire_on` set only for that continuation. The other fields are the
// parts of the row that a test scripts. With `window` set, each tuple search
// and the enumeration walk the records of that channel (T-P-1).
struct ScriptedRow<P: Clone> {
    channels: Vec<String>,
    listed: Vec<Consume>,
    extra: Vec<(Arc<WaitingContinuation<P, String>>, i32)>,
    tuple: Option<Vec<Candidate>>,
    fire_on: Option<String>,
    retires: Vec<SpaceConsume<String, P, String, String>>,
    staged_retires: usize,
    vetoing: Vec<String>,
    item_failed: Vec<(Blake2b256Hash, Blake2b256Hash)>,
    window: Option<String>,
    tail: TailRead,
    tail_channel: Option<String>,
    keeps_record: bool,
    unit: Vec<FinishRow<String, P, String, String>>,
    probe: Arc<SearchProbe>,
}

// Added by decision 124 (T-C4-32): a scripted row with no answers yet.
fn scripted_row<P: Clone>(channels: &[&str], listed: Vec<Consume>) -> ScriptedRow<P> {
    ScriptedRow {
        channels: names(channels),
        listed,
        extra: Vec::new(),
        tuple: None,
        fire_on: None,
        retires: Vec::new(),
        staged_retires: 0,
        vetoing: Vec::new(),
        item_failed: Vec::new(),
        window: None,
        tail: TailRead::Ends,
        tail_channel: None,
        keeps_record: true,
        unit: Vec::new(),
        probe: Arc::new(SearchProbe::default()),
    }
}

impl<P> RowSearch<String, P, String, String> for ScriptedRow<P>
where P: Clone + Send + Sync + 'static
{
    fn runs(&self) -> Vec<RowRun<String>> {
        vec![RowRun {
            channels: self.channels.clone(),
            candidates: 0..self.listed.len() + self.extra.len(),
        }]
    }

    fn continuations(
        &mut self,
        _run: usize,
        lists: &RowLists<'_, String, P, String, String>,
    ) -> Result<Vec<(Arc<WaitingContinuation<P, String>>, i32)>, SpaceTxnError> {
        let waiters = lists.continuations(&self.channels)?;
        let mut found = Vec::with_capacity(self.listed.len() + self.extra.len());
        for source in &self.listed {
            let Some(at) = waiters.iter().position(|waiter| waiter.source == *source) else {
                continue;
            };
            let index = i32::try_from(at).expect("the index of a waiter fits an i32");
            found.push((Arc::clone(&waiters[at]), index));
        }
        found.extend(self.extra.iter().cloned());
        Ok(found)
    }

    fn tuple(
        &mut self,
        _run: usize,
        lists: &RowLists<'_, String, P, String, String>,
        request: RowTupleRequest<'_, String, P, String, String>,
    ) -> Result<Option<Vec<Candidate>>, SpaceTxnError> {
        let built = match &self.window {
            None => 1,
            Some(channel) => lists.data(channel)?.len(),
        };
        self.probe.tuples.fetch_add(built, Ordering::Relaxed);
        let fires = match &self.fire_on {
            None => true,
            Some(continuation) => continuation == request.continuation,
        };
        match fires {
            false => Ok(None),
            true => Ok(self.tuple.clone()),
        }
    }

    fn restore(&mut self) {}

    fn commit_check(
        &mut self,
        _request: &RowTupleRequest<'_, String, P, String, String>,
        _tuple: &[Candidate],
    ) -> Result<bool, SpaceTxnError> {
        Ok(true)
    }

    fn may_veto(&self, _matcher: &dyn Match<P, String, String>, continuation: &String) -> bool {
        self.vetoing.contains(continuation)
    }

    fn staged_retires(&self) -> usize { self.staged_retires }

    fn actual_fire(
        &mut self,
        _request: &RowTupleRequest<'_, String, P, String, String>,
        _tuple: &[Candidate],
    ) -> ActualFire<String, P, String, String> {
        ActualFire {
            retires: std::mem::take(&mut self.retires),
            dispatched: None,
        }
    }

    fn item_failures(
        &self,
        lists: &RowLists<'_, String, P, String, String>,
        _matcher: &dyn Match<P, String, String>,
        rule: &dyn RowItemRule<String, String>,
    ) -> Result<Vec<(Blake2b256Hash, Blake2b256Hash)>, SpaceTxnError> {
        let Some(channel) = &self.window else {
            return Ok(self.item_failed.clone());
        };
        let waiters = lists.continuations(&self.channels)?;
        let records = lists.data(channel)?;
        let mut failed = Vec::with_capacity(waiters.len() * records.len());
        for waiter in &waiters {
            for record in &records {
                self.probe.item_tests.fetch_add(1, Ordering::Relaxed);
                let admitted = rule.admit_item(
                    &waiter.continuation,
                    waiter.persist,
                    self.channels.len(),
                    &record.a,
                    record.persist,
                )?;
                match admitted {
                    false => failed.push((record.source.hash.clone(), waiter.source.hash.clone())),
                    true => {}
                }
            }
        }
        Ok(failed)
    }

    fn tail_read(
        &mut self,
        lists: &RowLists<'_, String, P, String, String>,
        _matcher: &dyn Match<P, String, String>,
    ) -> Result<TailRead, SpaceTxnError> {
        self.probe.tail_reads.fetch_add(1, Ordering::Relaxed);
        match &self.tail_channel {
            None => {}
            Some(channel) => {
                let records = lists.data(channel)?.len();
                self.probe
                    .tail_records
                    .lock()
                    .expect("tail records lock")
                    .push(records);
            }
        }
        Ok(self.tail)
    }

    fn keeps_staged_record(&self) -> bool { self.keeps_record }

    fn store_unit(&mut self) -> Vec<FinishRow<String, P, String, String>> {
        std::mem::take(&mut self.unit)
    }
}

// Added by decision 124 (T-C4-25, T-C4-32, V3-12, V4-5): as `Funded`, but the
// COMM of the continuation `refused` is refused, so the machinery search of a
// retire of that continuation parks it inert.
struct FundedExcept {
    refused: String,
}

impl RSpaceAccountingObserver<String, Pat, String, String> for FundedExcept {
    fn observe_produce(
        &self,
        _source: &Produce,
        _channel: &String,
        _data: &String,
        _persistent: bool,
    ) -> Result<(), RSpaceError> {
        Ok(())
    }

    fn observe_consume(
        &self,
        _source: &Consume,
        _channels: &[String],
        _patterns: &[Pat],
        _continuation: &String,
        _persistent: bool,
        _peeks: &BTreeSet<i32>,
    ) -> Result<(), RSpaceError> {
        Ok(())
    }

    fn observe_comm(
        &self,
        _comm: &COMM,
        continuation: &String,
        _continuation_persistent: bool,
        _data: &[(&String, bool)],
    ) -> Result<(), RSpaceError> {
        match continuation == &self.refused {
            false => Ok(()),
            true => Err(RSpaceError::CommRefused),
        }
    }

    fn owner_checks_active(&self) -> bool { true }
}

// Added by decision 124 (T-C4-32, T-C4-33): a consume of `name` that parks on
// a.
fn parked(name: &str) -> Op { Op::consume(&["a"], vec![Pat::Any], name, false) }

// Added by decision 124 (T-C4-32, T-C4-33): play after the host operations
// `ops`, under owner funding.
async fn funded_space(ops: &[Op]) -> Space {
    let space = play_space().await;
    run_host_ops(&space, ops).await;
    space.set_accounting_observer(Some(Arc::new(Funded)));
    space
}

// Added by decision 124 (T-C4-25, T-C4-32): as `funded_space`, with the COMM
// of the continuation `refused` refused.
async fn funded_space_refusing(ops: &[Op], refused: &str) -> Space {
    let space = play_space().await;
    run_host_ops(&space, ops).await;
    space.set_accounting_observer(Some(Arc::new(FundedExcept {
        refused: s(refused),
    })));
    space
}

// Added by decision 124 (T-C4-32): the waiter `name` on a in the store of play.
async fn waiter_on_a(space: &Space, name: &str) -> WaitingContinuation<Pat, String> {
    let source = consume_source(&["a"], &[Pat::Any], name, false);
    let waiters = space.get_waiting_continuations(names(&["a"])).await;
    waiters
        .iter()
        .find(|waiter| waiter.source == source)
        .expect("the waiter waits on a")
        .clone()
}

// Added by decision 124 (T-C4-32): a transaction of play over the keys `keys`.
async fn begin_on(space: &Space, keys: &[String]) -> Txn {
    SpaceStoreHandle::new(space.clone())
        .begin(keys)
        .await
        .expect("begin")
}

// Added by decision 124 (T-C4-32): the candidate of the incoming x on a.
fn incoming_x() -> Candidate {
    incoming_candidate(s("a"), produce_source("a", "x", false), s("x"), false, s("x"))
}

// Added by decision 124 (T-C4-32): the candidate that binds the record `data`
// at index 0 of `channel`.
fn bound(channel: &str, data: &str) -> Candidate {
    bind_candidate(s(channel), &datum(channel, data, false), s(data), 0)
}

// Added by decision 124 (T-C4-32): the COMM of a recorded row with the
// candidates `candidates` of `consume`. Each produce counts once, as the first
// produce of its source does.
fn recorded_comm(candidates: &[Candidate], consume: Consume) -> COMM {
    COMM::new(candidates, consume, BTreeSet::new(), |produces| {
        produces
            .iter()
            .map(|produce| (produce.clone(), 1))
            .collect()
    })
}

// Added by decision 124 (T-C4-32): the retire of `data` on `channel` by
// `continuation`, with no search.
fn retire_of(
    channel: &str,
    data: &str,
    continuation: &str,
) -> SpaceConsume<String, Pat, String, String> {
    fire_consume_op(&[channel], vec![Pat::Any], continuation, vec![bound(channel, data)])
}

// Added by decision 124 (T-C4-25, T-C4-32, V4-5): a retire with its machinery
// search, which binds the record, so a refused COMM parks the retire inert.
fn machinery_retire(
    channel: &str,
    data: &str,
    continuation: &str,
) -> SpaceConsume<String, Pat, String, String> {
    let mut search = scripted_row::<Pat>(&[channel], Vec::new());
    search.tuple = Some(vec![bound(channel, data)]);
    let mut retire = retire_of(channel, data, continuation);
    retire.search = Some(Box::new(search));
    retire
}

// Added by decision 124 (T-C4-18, T-C4-38): a waiter of `channels`, listed by a
// search although the store may not hold it.
fn extra_waiter<P: Clone + Serialize>(
    channels: &[&str],
    pattern: P,
    name: &str,
) -> (Arc<WaitingContinuation<P, String>>, i32) {
    let patterns = vec![pattern; channels.len()];
    let waiter =
        WaitingContinuation::create(&names(channels), &patterns, &s(name), false, BTreeSet::new());
    (Arc::new(waiter), 1)
}

// Added by decision 124 (T-C4-32): the fire of x on a that the staging fired on
// the waiter k1, with the search `search`, staged in a transaction of play.
async fn staged_fire_of_k1(space: &Space, search: ScriptedRow<Pat>) -> Txn {
    let staged = waiter_on_a(space, "k1").await;
    let mut fire = fire_produce_op("a", "x", &staged);
    fire.search = Some(Box::new(search));
    let mut txn = begin_on(space, &universe()).await;
    txn.produce(fire).expect("stage the fire of x");
    txn
}

// Added by decision 124 (T-C4-18, T-C4-25, T-C4-37, T-C4-38): the matcher of
// the scripted native replay. A native session needs a pattern type with
// `ColdStoreDecode`, so these tests use `String` patterns, and a scripted
// search picks every tuple.
struct AnyPattern;

impl Match<String, String, String> for AnyPattern {
    fn get(&self, _pattern: &String, datum: &String) -> Option<String> { Some(datum.clone()) }
}

// Added by decision 124 (T-C4-18, T-C4-25, T-C4-37, T-C4-38): one recorded row
// of the scripted native replay: its outcome, the COMM of its candidate and
// the COMM that the row refused (R7).
#[derive(Clone)]
struct ReplayRow {
    outcome: NativeReplayOutcome,
    candidate: Option<COMM>,
    refusal: Option<COMM>,
}

impl ReplayRow {
    fn stored() -> Self {
        Self {
            outcome: NativeReplayOutcome::Stored,
            candidate: None,
            refusal: None,
        }
    }

    fn matched(candidate: Option<COMM>) -> Self {
        Self {
            outcome: NativeReplayOutcome::Matched,
            candidate,
            refusal: None,
        }
    }
}

// Added by decision 124 (T-C4-37, T-C4-25): the scripted answer of the lookup
// of the recorded rows of a chained transaction (CR-5, CR-6).
type RecordedRowScript = (usize, NativeReplayOutcome, Option<COMM>);

fn native_row_at(
    journal: usize,
    outcome: NativeReplayOutcome,
    comm: Option<COMM>,
) -> Option<RecordedRowScript> {
    Some((journal, outcome, comm))
}

// Added by decision 124 (T-C4-18, T-C4-25, T-C4-37, T-C4-38): a scripted epoch
// of native replay. Each operation begins the next row of `rows`, and each
// staged operation of a chained transaction looks up the next answer of
// `recorded`.
#[derive(Clone, Default)]
struct ReplayScript {
    rows: Arc<Mutex<VecDeque<ReplayRow>>>,
    recorded: Arc<Mutex<VecDeque<Option<RecordedRowScript>>>>,
}

impl ReplayScript {
    fn push(&self, row: ReplayRow) { self.rows.lock().expect("rows lock").push_back(row); }

    fn record(&self, rows: impl IntoIterator<Item = Option<RecordedRowScript>>) {
        self.recorded
            .lock()
            .expect("recorded rows lock")
            .extend(rows);
    }
}

struct ReplayBoundary;

struct ReplayRestore;

struct ReplayPublication;

impl NativeReplayEpoch for ReplayScript {
    type Boundary = ReplayBoundary;

    fn begin_boundary(&self) -> Result<ReplayBoundary, RSpaceError> { Ok(ReplayBoundary) }

    fn invalidate(&self) {}

    fn denial(&self) -> RSpaceError { RSpaceError::SignedLimitExhausted }
}

impl NativeReplayBoundary for ReplayBoundary {
    type Checkpoint = ();
    type Evidence = u64;
    type Restore = ReplayRestore;

    fn checkpoint(&self) -> Self::Checkpoint {}

    fn prepare_restore(self, _checkpoint: &()) -> Result<ReplayRestore, RSpaceError> {
        Ok(ReplayRestore)
    }

    fn check_complete(&self) -> Result<(), RSpaceError> { Ok(()) }

    fn completed_usage(&self) -> Result<u64, RSpaceError> { Ok(0) }

    fn completed_evidence(&self) -> Result<u64, RSpaceError> { Ok(0) }

    fn close(self) {}
}

impl NativeReplayRestore for ReplayRestore {
    fn publish(self) {}
}

impl NativeOperationPublication for ReplayPublication {
    fn publish(&mut self) {}
}

struct ReplayIdentity(Option<COMM>);

impl NativeCandidateIdentity for ReplayIdentity {
    fn matches_consume(&self, source: &Consume) -> bool {
        self.0.as_ref().is_some_and(|comm| comm.consume == *source)
    }

    fn matches_produce(&self, source: &Produce) -> bool {
        self.0
            .as_ref()
            .is_some_and(|comm| comm.produces.contains(source))
    }

    fn repetition(&self, source: &Produce) -> Option<i32> {
        self.0
            .as_ref()
            .and_then(|comm| comm.times_repeated.get(source).copied())
    }

    fn matches_comm(&self, source: &COMM) -> bool { self.0.as_ref() == Some(source) }
}

// Added by decision 124 (T-C4-18, T-C4-25, T-C4-37, T-C4-38): the ticket of a
// row of the scripted native replay. The row is under owner funding, so the
// search of the row runs, every item passes and a produce keeps its own datum
// (R5, R6, R7). `prepare` fails while a recorded refusal is not verified, as
// the check of the verified refusals of the real ticket does.
struct ReplayTicket {
    row: ReplayRow,
    identity: ReplayIdentity,
    refusal: ReplayIdentity,
    verified: usize,
}

impl NativeOwnerFilter<String, String, String, String> for ReplayTicket {
    fn admit_item(
        &self,
        _continuation: &String,
        _continuation_persistent: bool,
        _channel_count: usize,
        _datum: &String,
        _datum_persistent: bool,
    ) -> Result<bool, RSpaceError> {
        Ok(true)
    }

    fn names_refusal_for(&self, consume: &Consume) -> bool {
        self.row
            .refusal
            .as_ref()
            .is_some_and(|comm| comm.consume == *consume)
    }

    fn requires_own_datum(&self) -> bool { true }
}

impl NativeOperationTicket<String, String, String, String> for ReplayTicket {
    type Authority = ();
    type Publication = ReplayPublication;

    fn outcome(&self) -> NativeReplayOutcome { self.row.outcome }

    fn candidate_identity(&self) -> Option<&dyn NativeCandidateIdentity> {
        self.identity
            .0
            .as_ref()
            .map(|_| &self.identity as &dyn NativeCandidateIdentity)
    }

    fn authenticate_footprint(
        &mut self,
        _channels: &[String],
        _joins: &[Vec<String>],
    ) -> Result<(), RSpaceError> {
        Ok(())
    }

    fn observe_produce(
        &mut self,
        _source: &Produce,
        _channel: &String,
        _data: &String,
        _authority: &(),
    ) -> Result<NativeReplayDecision, RSpaceError> {
        Ok(NativeReplayDecision::Granted)
    }

    fn observe_consume(
        &mut self,
        _source: &Consume,
        _channels: &[String],
        _patterns: &[String],
        _continuation: &String,
        _peeks: &BTreeSet<i32>,
        _authority: &(),
    ) -> Result<NativeReplayDecision, RSpaceError> {
        Ok(NativeReplayDecision::Granted)
    }

    fn observe_comm(
        &mut self,
        source: &COMM,
        _continuation: &String,
        _persistent: bool,
        _data: &[Candidate],
    ) -> Result<NativeReplayDecision, RSpaceError> {
        match self.identity.matches_comm(source) {
            false => Err(RSpaceError::InterpreterError(
                "the fired COMM differs from the recorded candidate".to_owned(),
            )),
            true => Ok(NativeReplayDecision::Granted),
        }
    }

    fn returned_produce(&self, source: &Produce) -> Result<Produce, RSpaceError> {
        Ok(source.clone())
    }

    fn prepare(self, outcome: NativeReplayOutcome) -> Result<ReplayPublication, RSpaceError> {
        let verified = self.verified == usize::from(self.row.refusal.is_some());
        match (outcome == self.row.outcome, verified) {
            (false, _) => Err(RSpaceError::BugFoundError(format!(
                "prepared {outcome:?}, recorded {:?}",
                self.row.outcome
            ))),
            (true, false) => {
                Err(RSpaceError::InterpreterError("a recorded refusal is not verified".to_owned()))
            }
            (true, true) => Ok(ReplayPublication),
        }
    }

    fn owner_filter(&self) -> Option<&dyn NativeOwnerFilter<String, String, String, String>> {
        Some(self)
    }

    fn refusal_count(&self) -> usize { usize::from(self.row.refusal.is_some()) }

    fn refusal_identity(&self, index: usize) -> Option<&dyn NativeCandidateIdentity> {
        match (index, &self.refusal.0) {
            (0, Some(_)) => Some(&self.refusal),
            (_, _) => None,
        }
    }

    fn verify_refusal(
        &mut self,
        index: usize,
        comm: &COMM,
        _continuation: &String,
        _persistent: bool,
        _data: &[Candidate],
        _incoming: Option<&String>,
    ) -> Result<(), RSpaceError> {
        match index == 0 && self.refusal.matches_comm(comm) {
            false => {
                Err(RSpaceError::InterpreterError("a recorded refusal does not verify".to_owned()))
            }
            true => {
                self.verified += 1;
                Ok(())
            }
        }
    }
}

#[async_trait]
impl NativeOperationEpoch<String, String, String, String> for ReplayScript {
    type Ticket = ReplayTicket;

    async fn wait_ready(&self, _source: RSpaceOperationSource<'_>) -> Result<(), RSpaceError> {
        Ok(())
    }

    fn begin_operation(
        &self,
        _source: RSpaceOperationSource<'_>,
    ) -> Result<Option<ReplayTicket>, RSpaceError> {
        let row = self
            .rows
            .lock()
            .expect("rows lock")
            .pop_front()
            .ok_or_else(|| RSpaceError::BugFoundError("no recorded row".to_owned()))?;
        Ok(Some(ReplayTicket {
            identity: ReplayIdentity(row.candidate.clone()),
            refusal: ReplayIdentity(row.refusal.clone()),
            verified: 0,
            row,
        }))
    }

    fn space_transaction_ready(
        &self,
        _first: RSpaceOperationSource<'_>,
    ) -> Result<SpaceTransactionReadiness, RSpaceError> {
        Ok(SpaceTransactionReadiness::Ready)
    }

    fn space_recorded_row(
        &self,
        _source: RSpaceOperationSource<'_>,
        _previous: Option<usize>,
        _footprint_key: &String,
    ) -> Result<Option<RecordedRowScript>, RSpaceError> {
        Ok(self
            .recorded
            .lock()
            .expect("recorded rows lock")
            .pop_front()
            .flatten())
    }
}

type ScriptedSession = NativeReplaySession<String, String, String, String, ReplayScript>;
type ScriptedHandle = SpaceStoreHandle<String, String, String, String>;
type ReplayTxn = BoxedSpaceTxn<String, String, String, String>;
type ReplayProduce = SpaceProduce<String, String, String, String>;
type ReplayConsume = SpaceConsume<String, String, String, String>;

// Added by decision 124 (T-C4-18, T-C4-25, T-C4-37, T-C4-38): a scripted native
// replay session and its handle.
async fn replay_session(script: &ReplayScript) -> (Arc<ScriptedSession>, ScriptedHandle) {
    let mut stores = InMemoryStoreManager::new();
    let (play, _) = RSpace::<String, String, String, String>::create_with_replay(
        stores.r_space_stores().await.expect("in-memory stores"),
        Arc::new(Box::new(AnyPattern)),
    )
    .expect("an in-memory play and replay space");
    let session = Arc::new(
        ScriptedSession::new_with_installs(
            play.get_history_repository(),
            Arc::new(Box::new(AnyPattern)),
            script.clone(),
            Vec::<(Vec<String>, Install<String, String>)>::new(),
        )
        .expect("a native replay session"),
    );
    let host = NativeSpaceHost::new(
        Arc::clone(&session),
        |session: &ScriptedSession| session,
        Arc::new(|_source: RSpaceOperationSource<'_>| Ok(())),
        BTreeSet::new(),
    );
    (session, SpaceStoreHandle::new(host))
}

// Added by decision 124 (T-C4-37, T-C4-25): a transaction over `channels` and
// the footprint key S, whose rows form one chain (CR-5, CR-6).
async fn begin_chained(handle: &ScriptedHandle, channels: &[&str]) -> ReplayTxn {
    let mut keys = Vec::with_capacity(channels.len() + 1);
    keys.extend(channels.iter().map(|channel| s(channel)));
    keys.push(s("S"));
    let mut txn = handle.begin(&keys).await.expect("begin");
    txn.set_footprint_key(s("S")).expect("S is a lock key");
    txn
}

fn native_consume_source(channel: &str, continuation: &str) -> Consume {
    Consume::create(&names(&[channel]), &vec![s("_")], &s(continuation), false)
}

fn native_store_op(channel: &str, data: &str) -> ReplayProduce {
    SpaceProduce {
        channel: s(channel),
        data: s(data),
        persist: false,
        outcome: ProduceOutcome::Store,
        before_observe: None,
        own: false,
        search: None,
    }
}

fn native_park_op(channel: &str, continuation: &str) -> ReplayConsume {
    SpaceConsume {
        channels: names(&[channel]),
        patterns: vec![s("_")],
        continuation: s(continuation),
        persist: false,
        peeks: BTreeSet::new(),
        outcome: ConsumeOutcome::Park,
        before_observe: None,
        own: false,
        search: None,
    }
}

fn native_fire_op(
    channel: &str,
    data: &str,
    continuation: &WaitingContinuation<String, String>,
) -> ReplayProduce {
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
        own: false,
        search: None,
    }
}

fn native_take_op(channel: &str, data: &str, continuation: &str) -> ReplayConsume {
    SpaceConsume {
        channels: names(&[channel]),
        patterns: vec![s("_")],
        continuation: s(continuation),
        persist: false,
        peeks: BTreeSet::new(),
        outcome: ConsumeOutcome::Fire {
            data: vec![bound(channel, data)],
        },
        before_observe: None,
        own: false,
        search: None,
    }
}

// Added by decision 124 (T-C4-25, V4-5): as `machinery_retire`, for the
// `String` patterns of the scripted native replay.
fn native_take_retire(channel: &str, data: &str, continuation: &str) -> ReplayConsume {
    let mut search = scripted_row::<String>(&[channel], Vec::new());
    search.tuple = Some(vec![bound(channel, data)]);
    let mut retire = native_take_op(channel, data, continuation);
    retire.search = Some(Box::new(search));
    retire
}

// Added by decision 124 (T-C4-18, T-C4-37): parks `name` on `channel` through
// one recorded row.
async fn native_park(script: &ReplayScript, handle: &ScriptedHandle, channel: &str, name: &str) {
    script.push(ReplayRow::stored());
    let mut txn = handle.begin(&[s(channel)]).await.expect("begin");
    txn.consume(native_park_op(channel, name))
        .expect("stage the park");
    txn.commit().await.expect("commit the park");
}

// Added by decision 124 (T-C4-25, T-C4-37): stores `data` on `channel` through
// one recorded row.
async fn native_store(script: &ReplayScript, handle: &ScriptedHandle, channel: &str, data: &str) {
    script.push(ReplayRow::stored());
    let mut txn = handle.begin(&[s(channel)]).await.expect("begin");
    txn.produce(native_store_op(channel, data))
        .expect("stage the store");
    txn.commit().await.expect("commit the store");
}

// Added by decision 124 (T-C4-18, T-C4-25, T-C4-37): the waiter `name` of the
// group of `channel` in the view of the transaction.
fn native_waiter(
    txn: &mut ReplayTxn,
    channel: &str,
    name: &str,
) -> WaitingContinuation<String, String> {
    txn.continuations(&[s(channel)])
        .expect("read the waiters")
        .iter()
        .find(|waiter| waiter.continuation == name)
        .expect("the waiter waits")
        .clone()
}

// Added by decision 124 (ruling RADD 1.2, C2v3 6.11): the unit loop reads the
// tail of a deviating row d once, after the group of the row. The store of the
// read no longer holds the retired record, and the deviation carries the
// answer of the search.
#[tokio::test]
async fn unit_loop_reads_the_tail_once_after_the_group() {
    let space = funded_space(&[parked("k1"), parked("k2"), Op::produce("c", "r", false)]).await;
    let k2 = consume_source(&["a"], &[Pat::Any], "k2", false);
    let mut search = scripted_row::<Pat>(&["a"], vec![k2]);
    search.tuple = Some(vec![incoming_x()]);
    search.retires = vec![retire_of("c", "r", "taker")];
    search.tail = TailRead::Reissues;
    search.tail_channel = Some(s("c"));
    let probe = Arc::clone(&search.probe);
    let mut txn = staged_fire_of_k1(&space, search).await;
    let receipt = txn.commit().await.expect("commit the unit");
    assert!(matches!(receipt.ops.as_slice(), [
        SpaceOpReceipt::Fired { .. },
        SpaceOpReceipt::Fired { .. },
    ]));
    let deviation = receipt.deviation.as_ref().expect("row d deviates");
    assert_eq!(deviation.index, 0);
    assert_eq!(deviation.tail_read, TailRead::Reissues);
    assert_eq!(probe.tail_reads.load(Ordering::Relaxed), 1, "the tail is read once");
    let seen = probe
        .tail_records
        .lock()
        .expect("tail records lock")
        .clone();
    assert_eq!(seen, vec![0], "the read runs after the retire of r on c");
}

// Added by decision 124 (ruling RADD2 1.2, rules CR-31 and CR-33, J-STORE): a
// row that deviates to a store and whose search keeps no staged record writes
// nothing on its channel. The store unit commits after the row and before the
// tail read. A search with the default answers keeps the record of its row.
#[tokio::test]
async fn deviating_store_unit_commits_before_the_tail_read() {
    let space = funded_space(&[parked("k1")]).await;
    let k1 = consume_source(&["a"], &[Pat::Any], "k1", false);
    let mut search = scripted_row::<Pat>(&["a"], vec![k1.clone()]);
    search.keeps_record = false;
    search.unit = vec![FinishRow::Store(store_op("c", "r", false))];
    search.tail_channel = Some(s("c"));
    let probe = Arc::clone(&search.probe);
    let mut txn = staged_fire_of_k1(&space, search).await;
    let receipt = txn.commit().await.expect("commit the unit");
    assert!(matches!(receipt.ops.as_slice(), [
        SpaceOpReceipt::Stored { .. },
        SpaceOpReceipt::Stored { .. },
    ]));
    assert_eq!(receipt.deviation.as_ref().expect("row d deviates").index, 0);
    assert!(space.get_data(&s("a")).await.is_empty(), "no store keeps the staged record");
    assert_eq!(space.get_data(&s("c")).await.len(), 1, "the unit stores r on c");
    let seen = probe
        .tail_records
        .lock()
        .expect("tail records lock")
        .clone();
    assert_eq!(seen, vec![1], "the read runs after the store of r on c");

    let space = funded_space(&[parked("k1")]).await;
    let search = scripted_row::<Pat>(&["a"], vec![k1]);
    let mut txn = staged_fire_of_k1(&space, search).await;
    let receipt = txn.commit().await.expect("commit the row");
    assert!(matches!(receipt.ops.as_slice(), [SpaceOpReceipt::Stored { .. }]));
    assert_eq!(space.get_data(&s("a")).await.len(), 1, "the row keeps its record");
}

// Added by decision 124 (ruling RADD2 1.2, J-STORE): native replay commits the
// store unit of a deviating row from its own search, after the row and before
// its tail read, and the staged record stays out of the store.
#[tokio::test]
async fn replay_commits_the_store_unit_of_a_deviating_row() {
    let script = ReplayScript::default();
    let (_session, handle) = replay_session(&script).await;
    native_park(&script, &handle, "a", "k1").await;
    let mut search = scripted_row::<String>(&["a"], vec![native_consume_source("a", "k1")]);
    search.keeps_record = false;
    search.unit = vec![FinishRow::Store(native_store_op("c", "r"))];
    search.tail_channel = Some(s("c"));
    let probe = Arc::clone(&search.probe);
    let mut txn = handle.begin(&[s("a"), s("c")]).await.expect("begin");
    let waiter = native_waiter(&mut txn, "a", "k1");
    let mut fire = native_fire_op("a", "x", &waiter);
    fire.search = Some(Box::new(search));
    txn.produce(fire).expect("stage the fire of x");
    script.push(ReplayRow::stored());
    script.push(ReplayRow::stored());
    let receipt = txn.commit().await.expect("commit the unit of replay");
    assert!(matches!(receipt.ops.as_slice(), [
        SpaceOpReceipt::Stored { .. },
        SpaceOpReceipt::Stored { .. },
    ]));
    assert_eq!(receipt.deviation.as_ref().expect("row d deviates").index, 0);
    let seen = probe
        .tail_records
        .lock()
        .expect("tail records lock")
        .clone();
    assert_eq!(seen, vec![1], "the read runs after the store of r on c");
    let mut txn = handle.begin(&[s("a"), s("c")]).await.expect("begin");
    assert!(txn.data(&s("a")).expect("read a").is_empty(), "no store keeps the staged record");
    assert_eq!(txn.data(&s("c")).expect("read c").len(), 1, "the unit stores r on c");
}

// Added by decision 124 (ruling RADD2 1.2, rule P2, J-STORE): a staged row that
// follows the store unit of a deviating row belongs to the unit of the cut, so
// the commit passes and the row does not run twice.
#[tokio::test]
async fn replay_follows_the_store_unit_of_a_deviating_row() {
    let script = ReplayScript::default();
    let (_session, handle) = replay_session(&script).await;
    native_park(&script, &handle, "a", "k1").await;
    let mut search = scripted_row::<String>(&["a"], vec![native_consume_source("a", "k1")]);
    search.keeps_record = false;
    search.unit = vec![FinishRow::Store(native_store_op("c", "r"))];
    let mut txn = handle.begin(&[s("a"), s("c")]).await.expect("begin");
    let waiter = native_waiter(&mut txn, "a", "k1");
    let mut fire = native_fire_op("a", "x", &waiter);
    fire.search = Some(Box::new(search));
    txn.produce(fire).expect("stage the fire of x");
    txn.produce(native_store_op("c", "r"))
        .expect("stage the row that follows the unit");
    script.push(ReplayRow::stored());
    script.push(ReplayRow::stored());
    let receipt = txn.commit().await.expect("the row follows the unit");
    assert!(matches!(receipt.ops.as_slice(), [
        SpaceOpReceipt::Stored { .. },
        SpaceOpReceipt::Stored { .. },
    ]));
    assert_eq!(receipt.deviation.as_ref().expect("row d deviates").index, 0);
    let mut txn = handle.begin(&[s("a"), s("c")]).await.expect("begin");
    assert_eq!(txn.data(&s("c")).expect("read c").len(), 1, "the unit stores r once");
}

// Added by decision 124 (T-C4-32, V3-10, V4-5): every commit of a deviating
// unit passes the event check of its rows and gives the receipt of its rows:
// a deviating produce row (case 1), an actual class fire whose retires differ
// from the retires that the staging planned (case 2), and a refused retire
// inside a group, which the machinery search of the retire parks (case 3).
#[tokio::test]
async fn deviating_row_effects_match_its_events() {
    // Case 1: x fires k2 in place of k1.
    let space = funded_space(&[parked("k1"), parked("k2")]).await;
    let k2 = consume_source(&["a"], &[Pat::Any], "k2", false);
    let mut search = scripted_row::<Pat>(&["a"], vec![k2]);
    search.tuple = Some(vec![incoming_x()]);
    let mut txn = staged_fire_of_k1(&space, search).await;
    let receipt = txn.commit().await.expect("case 1 commits");
    assert!(matches!(receipt.ops.as_slice(), [SpaceOpReceipt::Fired { .. }]));
    let deviation = receipt.deviation.as_ref().expect("case 1 deviates");
    assert_eq!(deviation.index, 0);

    // Case 2: the staging planned the retire of r0 on c. The actual fire of k2
    // retires r1 on d and r2 on v1 instead, so r0 stays.
    let space = funded_space(&[
        parked("k1"),
        parked("k2"),
        Op::produce("c", "r0", false),
        Op::produce("d", "r1", false),
        Op::produce("v1", "r2", false),
    ])
    .await;
    let k2 = consume_source(&["a"], &[Pat::Any], "k2", false);
    let mut search = scripted_row::<Pat>(&["a"], vec![k2]);
    search.tuple = Some(vec![incoming_x()]);
    search.staged_retires = 1;
    search.retires = vec![retire_of("d", "r1", "retire1"), retire_of("v1", "r2", "retire2")];
    let mut txn = staged_fire_of_k1(&space, search).await;
    txn.consume(retire_of("c", "r0", "retire0"))
        .expect("stage the planned retire");
    let receipt = txn.commit().await.expect("case 2 commits");
    assert_eq!(receipt.ops.len(), 3, "row d and the two actual retires");
    assert_eq!(receipt.deviation.as_ref().expect("case 2 deviates").index, 0);
    assert_eq!(space.get_data(&s("c")).await.len(), 1, "the planned retire does not run");
    assert!(space.get_data(&s("d")).await.is_empty(), "r1 is retired");
    assert!(space.get_data(&s("v1")).await.is_empty(), "r2 is retired");

    // Case 3: x fires k1 as staged. The group retires r1 on c, whose COMM is
    // refused, so the retire parks inert, and r2 on d, which commits.
    let space = funded_space_refusing(
        &[parked("k1"), Op::produce("c", "r1", false), Op::produce("d", "r2", false)],
        "retire1",
    )
    .await;
    let k1 = consume_source(&["a"], &[Pat::Any], "k1", false);
    let mut search = scripted_row::<Pat>(&["a"], vec![k1]);
    search.tuple = Some(vec![incoming_x()]);
    search.staged_retires = 2;
    let mut txn = staged_fire_of_k1(&space, search).await;
    txn.consume(machinery_retire("c", "r1", "retire1"))
        .expect("stage the retire of r1");
    txn.consume(machinery_retire("d", "r2", "retire2"))
        .expect("stage the retire of r2");
    let receipt = txn.commit().await.expect("case 3 commits");
    assert!(matches!(receipt.ops.as_slice(), [
        SpaceOpReceipt::Fired { .. },
        SpaceOpReceipt::Parked { .. },
        SpaceOpReceipt::Fired { .. },
    ]));
    assert_eq!(receipt.deviation.as_ref().expect("case 3 deviates").index, 0);
    let r1 = datum("c", "r1", false).source.hash;
    let retire1 = consume_source(&["c"], &[Pat::Any], "retire1", false).hash;
    assert_eq!(receipt.refused, vec![(r1, retire1)]);
}

// Added by decision 124 (T-C4-33, V3-11, CR-36): a scripted row search reads a
// channel outside the keys of its transaction. The commit fails closed with
// `UnlockedChannel`, and no effect applies.
#[tokio::test]
async fn row_search_read_of_an_uncovered_channel_fails_closed() {
    let space = funded_space(&[parked("k1")]).await;
    let k1 = consume_source(&["a"], &[Pat::Any], "k1", false);
    let search = scripted_row::<Pat>(&["b"], vec![k1]);
    let staged = waiter_on_a(&space, "k1").await;
    let mut fire = fire_produce_op("a", "x", &staged);
    fire.search = Some(Box::new(search));
    let mut txn = begin_on(&space, &names(&["a"])).await;
    txn.produce(fire).expect("stage the fire of x");
    let error = txn.commit().await.expect_err("the read of b fails closed");
    assert_eq!(error, SpaceTxnError::UnlockedChannel);
    let waiters = space.get_waiting_continuations(names(&["a"])).await;
    assert_eq!(waiters.len(), 1, "k1 still waits on a");
    assert!(space.get_data(&s("a")).await.is_empty(), "no datum is stored");
}

// Added by decision 124 (T-C4-37, V4-17): the receipt lists a refused pair
// without a deviation. Case (a): a row that replay follows, with a recorded
// refusal of another waiter, is the named row of its record. Case (b): a staged
// park with a recorded refusal verifies it through its search and prepares.
// Without its search the park fails with the refusal error of its ticket
// (V4-17 (iv)).
#[tokio::test]
async fn replay_receipt_lists_refusals_without_a_deviation() {
    // Case (a): the row fires k2 after the recorded refusal of k1.
    let script = ReplayScript::default();
    let (_session, handle) = replay_session(&script).await;
    native_park(&script, &handle, "a", "k1").await;
    native_park(&script, &handle, "a", "k2").await;
    let k1 = native_consume_source("a", "k1");
    let k2 = native_consume_source("a", "k2");
    let comm_k1 = recorded_comm(&[incoming_x()], k1.clone());
    let comm_k2 = recorded_comm(&[incoming_x()], k2.clone());
    script.record([native_row_at(4, NativeReplayOutcome::Matched, Some(comm_k2.clone()))]);
    let mut search = scripted_row::<String>(&["a"], vec![k1.clone(), k2]);
    search.tuple = Some(vec![incoming_x()]);
    let mut txn = begin_chained(&handle, &["a"]).await;
    let waiter = native_waiter(&mut txn, "a", "k2");
    let mut fire = native_fire_op("a", "x", &waiter);
    fire.own = true;
    fire.search = Some(Box::new(search));
    txn.produce(fire).expect("stage the fire of k2");
    script.push(ReplayRow {
        outcome: NativeReplayOutcome::Matched,
        candidate: Some(comm_k2),
        refusal: Some(comm_k1),
    });
    let receipt = txn
        .commit()
        .await
        .expect("case (a): the refusal is verified");
    assert_eq!(receipt.deviation, None, "case (a): no deviation");
    let x = produce_source("a", "x", false).hash;
    assert_eq!(receipt.refused, vec![(x, k1.hash)]);

    // Case (b): the park of k on c refused its COMM with r.
    let refused_row = || ReplayRow {
        outcome: NativeReplayOutcome::Stored,
        candidate: None,
        refusal: Some(recorded_comm(&[bound("c", "r")], native_consume_source("c", "k"))),
    };
    let script = ReplayScript::default();
    let (_session, handle) = replay_session(&script).await;
    native_store(&script, &handle, "c", "r").await;
    let mut search = scripted_row::<String>(&["c"], Vec::new());
    search.tuple = Some(vec![bound("c", "r")]);
    let mut park = native_park_op("c", "k");
    park.search = Some(Box::new(search));
    let mut txn = handle.begin(&[s("c")]).await.expect("begin");
    txn.consume(park).expect("stage the park of k");
    script.push(refused_row());
    let receipt = txn
        .commit()
        .await
        .expect("case (b): the park verifies its refusal");
    assert!(matches!(receipt.ops.as_slice(), [SpaceOpReceipt::Parked { .. }]));
    assert_eq!(receipt.deviation, None, "case (b): no deviation");
    let r = produce_source("c", "r", false).hash;
    assert_eq!(receipt.refused, vec![(r, native_consume_source("c", "k").hash)]);

    // Case (b) without the search: the refusal is not verified, so the row
    // fails at its prepare.
    let script = ReplayScript::default();
    let (_session, handle) = replay_session(&script).await;
    native_store(&script, &handle, "c", "r").await;
    let mut txn = handle.begin(&[s("c")]).await.expect("begin");
    txn.consume(native_park_op("c", "k"))
        .expect("stage the park of k");
    script.push(refused_row());
    let error = txn
        .commit()
        .await
        .expect_err("case (b): an unverified refusal");
    let refusal = RSpaceError::InterpreterError("a recorded refusal is not verified".to_owned());
    assert_eq!(error, SpaceTxnError::Host(refusal));
}

// Added by decision 124 (T-C4-38, V4-18, L3, X7): `vetoing` of a deviating row
// comes from `RowSearch::may_veto`, equal in both roles. A two-bind vetoing
// waiter and a single-bind guarded waiter set it, and a waiter whose commit
// cannot veto leaves it unset.
#[tokio::test]
async fn deviating_row_sets_vetoing_from_row_search_may_veto() {
    let cases: [(&[&str], &str, bool); 3] =
        [(&["a", "b"], "joiner", true), (&["a"], "guarded", true), (&["a"], "plain", false)];
    for (channels, name, vetoes) in cases {
        let vetoing = match vetoes {
            false => Vec::new(),
            true => vec![s(name)],
        };

        // Play: no waiter matches, so the staged fire deviates to a store.
        let space = funded_space(&[parked("k1")]).await;
        let k1 = consume_source(&["a"], &[Pat::Any], "k1", false);
        let mut search = scripted_row::<Pat>(&["a"], vec![k1]);
        search.extra = vec![extra_waiter(channels, Pat::Any, name)];
        search.vetoing = vetoing.clone();
        let mut txn = staged_fire_of_k1(&space, search).await;
        let receipt = txn.commit().await.expect("commit the row of play");
        assert!(matches!(receipt.ops.as_slice(), [SpaceOpReceipt::Stored { .. }]));
        let played = receipt.deviation.expect("the row of play deviates").vetoing;

        // Replay: the recorded row stores, so the staged fire deviates.
        let script = ReplayScript::default();
        let (_session, handle) = replay_session(&script).await;
        native_park(&script, &handle, "a", "k1").await;
        let mut search = scripted_row::<String>(&["a"], vec![native_consume_source("a", "k1")]);
        search.extra = vec![extra_waiter(channels, s("_"), name)];
        search.vetoing = vetoing;
        let mut txn = handle.begin(&[s("a")]).await.expect("begin");
        let waiter = native_waiter(&mut txn, "a", "k1");
        let mut fire = native_fire_op("a", "x", &waiter);
        fire.search = Some(Box::new(search));
        txn.produce(fire).expect("stage the fire of x");
        script.push(ReplayRow::stored());
        let receipt = txn.commit().await.expect("commit the row of replay");
        assert!(matches!(receipt.ops.as_slice(), [SpaceOpReceipt::Stored { .. }]));
        let replayed = receipt
            .deviation
            .expect("the row of replay deviates")
            .vetoing;

        assert_eq!(played, vetoes, "play, waiter {name}");
        assert_eq!(replayed, vetoes, "replay, waiter {name}");
    }
}

// Added by decision 124 (rule CR-19 case 3, F22): a deviating store of a
// persistent record that the store holds leaves the one copy, in play and
// in replay. A deviating store of a record that the store does not hold
// stores it, and a plain store of a held record adds a copy, as the base does.
#[tokio::test]
async fn deviating_store_of_a_stored_persistent_record_keeps_one_copy() {
    for held in [false, true] {
        // Play: k1 waits for y, so the host parks it and the persistent x
        // stays stored. The search finds no tuple, so the fire deviates.
        let mut ops = Vec::with_capacity(2);
        ops.push(Op::consume(&["a"], vec![Pat::Exact(s("y"))], "k1", false));
        match held {
            true => ops.push(Op::produce("a", "x", true)),
            false => {}
        }
        let space = funded_space(&ops).await;
        let k1 = consume_source(&["a"], &[Pat::Exact(s("y"))], "k1", false);
        let waiter = space
            .get_waiting_continuations(names(&["a"]))
            .await
            .into_iter()
            .find(|waiter| waiter.source == k1)
            .expect("k1 waits on a");
        let search = scripted_row::<Pat>(&["a"], vec![k1]);
        let mut txn = begin_on(&space, &universe()).await;
        txn.produce(SpaceProduce {
            channel: s("a"),
            data: s("x"),
            persist: true,
            outcome: ProduceOutcome::Fire {
                channels: names(&["a"]),
                continuation: waiter,
                data: vec![incoming_candidate(
                    s("a"),
                    produce_source("a", "x", true),
                    s("x"),
                    true,
                    s("x"),
                )],
            },
            before_observe: None,
            own: false,
            search: Some(Box::new(search)),
        })
        .expect("stage the fire of x");
        let receipt = txn.commit().await.expect("commit the row of play");
        assert!(matches!(receipt.ops.as_slice(), [SpaceOpReceipt::Stored { .. }]));
        assert!(receipt.deviation.is_some(), "the row of play deviates, held {held}");
        assert_eq!(space.get_data(&s("a")).await.len(), 1, "play, held {held}");
        let mut txn = begin_on(&space, &universe()).await;
        txn.produce(store_op("a", "x", true))
            .expect("stage the plain store of x");
        txn.commit().await.expect("commit the plain store of x");
        assert_eq!(space.get_data(&s("a")).await.len(), 2, "play, plain, held {held}");

        // Replay: the recorded row stores, so the staged fire deviates.
        let script = ReplayScript::default();
        let (_session, handle) = replay_session(&script).await;
        native_park(&script, &handle, "a", "k1").await;
        match held {
            true => {
                script.push(ReplayRow::stored());
                let mut txn = handle.begin(&[s("a")]).await.expect("begin");
                txn.produce(SpaceProduce {
                    persist: true,
                    ..native_store_op("a", "x")
                })
                .expect("stage the store of x");
                txn.commit().await.expect("commit the store of x");
            }
            false => {}
        }
        let search = scripted_row::<String>(&["a"], vec![native_consume_source("a", "k1")]);
        let mut txn = handle.begin(&[s("a")]).await.expect("begin");
        let waiter = native_waiter(&mut txn, "a", "k1");
        txn.produce(SpaceProduce {
            channel: s("a"),
            data: s("x"),
            persist: true,
            outcome: ProduceOutcome::Fire {
                channels: names(&["a"]),
                continuation: waiter,
                data: vec![incoming_candidate(
                    s("a"),
                    produce_source("a", "x", true),
                    s("x"),
                    true,
                    s("x"),
                )],
            },
            before_observe: None,
            own: false,
            search: Some(Box::new(search)),
        })
        .expect("stage the fire of x");
        script.push(ReplayRow::stored());
        let receipt = txn.commit().await.expect("commit the row of replay");
        assert!(matches!(receipt.ops.as_slice(), [SpaceOpReceipt::Stored { .. }]));
        assert!(receipt.deviation.is_some(), "the row of replay deviates, held {held}");
        {
            let mut txn = handle.begin(&[s("a")]).await.expect("begin");
            assert_eq!(txn.data(&s("a")).expect("read a").len(), 1, "replay, held {held}");
        }
        script.push(ReplayRow::stored());
        let mut txn = handle.begin(&[s("a")]).await.expect("begin");
        txn.produce(SpaceProduce {
            persist: true,
            ..native_store_op("a", "x")
        })
        .expect("stage the plain store of x");
        txn.commit().await.expect("commit the plain store of x");
        let mut txn = handle.begin(&[s("a")]).await.expect("begin");
        assert_eq!(txn.data(&s("a")).expect("read a").len(), 2, "replay, plain, held {held}");
    }
}

// Added by decision 124 (T-C4-18, CR-26, V3-10): the alternative of a staged
// fire runs the checks of a staged fire: its COMM counts the incoming produce
// (i), replay computes the COMM of play (ii), a waiter that the store does not
// hold fails closed (iii), and so does a tuple that binds a record that is not
// in the store (iv).
#[tokio::test]
async fn alternative_runs_the_commit_checks_of_a_staged_fire() {
    // (i) Play: x was produced once before, so the alternative counts it twice.
    let space = funded_space(&[
        Op::produce("a", "x", false),
        Op::consume(&["a"], vec![Pat::Any], "taker", false),
        parked("k1"),
        parked("k2"),
    ])
    .await;
    let x = produce_source("a", "x", false);
    let handle = SpaceStoreHandle::new(space.clone());
    let before = counters_at(&handle, &[x.clone()]).await;
    let k2 = consume_source(&["a"], &[Pat::Any], "k2", false);
    let mut search = scripted_row::<Pat>(&["a"], vec![k2]);
    search.tuple = Some(vec![incoming_x()]);
    let mut txn = staged_fire_of_k1(&space, search).await;
    let receipt = txn.commit().await.expect("(i) commits");
    let SpaceOpReceipt::Fired { comm, .. } = &receipt.ops[0] else {
        panic!("(i): the alternative fires");
    };
    let expected = before.get(&x).copied().expect("(i): x has a counter") + 1;
    assert_eq!(comm.times_repeated.get(&x).copied(), Some(expected), "(i)");

    // (ii) Replay: the same alternative gives the recorded COMM.
    let script = ReplayScript::default();
    let (_session, replay) = replay_session(&script).await;
    native_park(&script, &replay, "a", "k1").await;
    native_park(&script, &replay, "a", "k2").await;
    let k2 = native_consume_source("a", "k2");
    let comm_k2 = recorded_comm(&[incoming_x()], k2.clone());
    let mut search = scripted_row::<String>(&["a"], vec![k2]);
    search.tuple = Some(vec![incoming_x()]);
    let mut txn = replay.begin(&[s("a")]).await.expect("begin");
    let waiter = native_waiter(&mut txn, "a", "k1");
    let mut fire = native_fire_op("a", "x", &waiter);
    fire.search = Some(Box::new(search));
    txn.produce(fire).expect("(ii): stage the fire of x");
    script.push(ReplayRow::matched(Some(comm_k2.clone())));
    let receipt = txn.commit().await.expect("(ii) commits");
    let SpaceOpReceipt::Fired { comm, .. } = &receipt.ops[0] else {
        panic!("(ii): the alternative fires");
    };
    assert_eq!(comm, &comm_k2, "(ii): replay computes the COMM of play");

    // (iii) A waiter that the store does not hold fails closed.
    let space = funded_space(&[parked("k1")]).await;
    let mut search = scripted_row::<Pat>(&["a"], Vec::new());
    search.extra = vec![extra_waiter(&["a"], Pat::Any, "k9")];
    search.tuple = Some(vec![incoming_x()]);
    let mut txn = staged_fire_of_k1(&space, search).await;
    let error = txn
        .commit()
        .await
        .expect_err("(iii): k9 is not in the store");
    assert_eq!(error, SpaceTxnError::NotInView("the waiting continuation"));
    let waiters = space.get_waiting_continuations(names(&["a"])).await;
    assert_eq!(waiters.len(), 1, "(iii): k1 still waits on a");
    assert!(space.get_data(&s("a")).await.is_empty(), "(iii): no effect");

    // (iv) A tuple that binds a record that is not in the store fails closed.
    let space = funded_space(&[
        parked("k1"),
        Op::consume(&["c", "a"], vec![Pat::Any, Pat::Any], "joined", false),
    ])
    .await;
    let joined = consume_source(&["c", "a"], &[Pat::Any, Pat::Any], "joined", false);
    let mut search = scripted_row::<Pat>(&["c", "a"], vec![joined]);
    search.tuple = Some(vec![bound("c", "ghost"), incoming_x()]);
    let mut txn = staged_fire_of_k1(&space, search).await;
    let error = txn
        .commit()
        .await
        .expect_err("(iv): the ghost record fails closed");
    assert_eq!(error, SpaceTxnError::NotInView("a stored candidate index"));
    let waiters = space.get_waiting_continuations(names(&["a"])).await;
    assert_eq!(waiters.len(), 1, "(iv): k1 still waits on a");
    assert!(space.get_data(&s("a")).await.is_empty(), "(iv): no effect");
}

// Added by decision 124 (C2v3 6.13 (d) step 5): under owner funding, a row
// search whose found COMM equals the staged COMM commits the staged fire, and
// the row does not deviate.
#[tokio::test]
async fn play_row_search_commits_c0_as_staged() {
    let space = funded_space(&[parked("k1")]).await;
    let k1 = consume_source(&["a"], &[Pat::Any], "k1", false);
    let mut search = scripted_row::<Pat>(&["a"], vec![k1]);
    search.tuple = Some(vec![incoming_x()]);
    let staged = waiter_on_a(&space, "k1").await;
    let mut fire = fire_produce_op("a", "x", &staged);
    fire.search = Some(Box::new(search));
    let mut txn = begin_on(&space, &universe()).await;
    let staged_op = txn.produce(fire).expect("stage the fire of x");
    let staged_comm = staged_op.comm.expect("a staged fire has a COMM");
    let receipt = txn.commit().await.expect("commit c0");
    let SpaceOpReceipt::Fired { comm, .. } = &receipt.ops[0] else {
        panic!("row c0 fires");
    };
    assert_eq!(comm, &staged_comm);
    assert_eq!(receipt.deviation, None, "the found COMM is c0");
    assert!(receipt.refused.is_empty());
}

// Added by decision 124 (C2v3 6.18 (g) step 4): replay of a row whose row
// search finds the COMM of the staged fire keeps c0, and the row does not
// deviate.
#[tokio::test]
async fn replay_row_search_keeps_c0() {
    let script = ReplayScript::default();
    let (_session, handle) = replay_session(&script).await;
    native_park(&script, &handle, "a", "k1").await;
    let mut search = scripted_row::<String>(&["a"], vec![native_consume_source("a", "k1")]);
    search.tuple = Some(vec![incoming_x()]);
    let mut txn = handle.begin(&[s("a")]).await.expect("begin");
    let waiter = native_waiter(&mut txn, "a", "k1");
    let mut fire = native_fire_op("a", "x", &waiter);
    fire.search = Some(Box::new(search));
    let staged = txn.produce(fire).expect("stage the fire of x");
    let staged_comm = staged.comm.expect("a staged fire has a COMM");
    script.push(ReplayRow::matched(Some(staged_comm.clone())));
    let receipt = txn.commit().await.expect("commit c0");
    let SpaceOpReceipt::Fired { comm, .. } = &receipt.ops[0] else {
        panic!("row c0 fires");
    };
    assert_eq!(comm, &staged_comm);
    assert_eq!(receipt.deviation, None, "the found COMM is c0");
}

// Added by decision 124 (OP-E211, C2v3 6.11): a replay stop at a record end
// carries the item pairs and `vetoing` of row d: a Stored row with a search
// can stop on its item failures.
#[tokio::test]
async fn record_end_stop_keeps_the_pairs_of_row_d() {
    let script = ReplayScript::default();
    let (_session, handle) = replay_session(&script).await;
    script.record([native_row_at(4, NativeReplayOutcome::Stored, None), None]);
    let pair = (Blake2b256Hash::new(b"record"), Blake2b256Hash::new(b"waiter"));
    let mut search = scripted_row::<String>(&["b"], Vec::new());
    search.extra = vec![extra_waiter(&["b"], s("_"), "guarded")];
    search.vetoing = vec![s("guarded")];
    search.item_failed = vec![pair.clone()];
    let mut store = native_store_op("a", "x");
    store.own = true;
    store.search = Some(Box::new(search));
    let mut txn = begin_chained(&handle, &["a", "b"]).await;
    txn.produce(store).expect("stage the own store of x");
    assert_eq!(txn.produce(native_store_op("a", "y")).err(), Some(SpaceTxnError::RecordEnded));
    script.push(ReplayRow::stored());
    let receipt = txn.commit().await.expect("the item failure gives CanStop");
    let cut = receipt.deviation.expect("the record end cuts the unit");
    assert_eq!(cut.index, 0);
    assert_eq!(cut.item_failed, vec![pair]);
    assert!(cut.vetoing, "the guarded waiter of row d can veto");
}

// Added by decision 124 (F19, C2v3 6.13 and 6.18): a Matched row books no item
// pair: a Matched alternative in play and a Matched row at a record end in
// replay give an empty `item_failed` and no veto.
#[tokio::test]
async fn matched_row_books_no_item_pair() {
    let pair = (Blake2b256Hash::new(b"record"), Blake2b256Hash::new(b"waiter"));

    // Play: x fires k2 in place of k1.
    let space = funded_space(&[parked("k1"), parked("k2")]).await;
    let k2 = consume_source(&["a"], &[Pat::Any], "k2", false);
    let mut search = scripted_row::<Pat>(&["a"], vec![k2]);
    search.tuple = Some(vec![incoming_x()]);
    search.item_failed = vec![pair.clone()];
    let mut txn = staged_fire_of_k1(&space, search).await;
    let receipt = txn.commit().await.expect("commit the alternative");
    let deviation = receipt.deviation.expect("the alternative deviates");
    assert!(deviation.item_failed.is_empty(), "play books no item pair");
    assert!(!deviation.vetoing, "play: a Matched row has no veto");

    // Replay: the own fire of the row matches c0 at a record end.
    let script = ReplayScript::default();
    let (_session, handle) = replay_session(&script).await;
    native_park(&script, &handle, "a", "k1").await;
    let k1 = native_consume_source("a", "k1");
    let comm = recorded_comm(&[incoming_x()], k1.clone());
    script.record([native_row_at(4, NativeReplayOutcome::Matched, Some(comm.clone())), None]);
    let mut search = scripted_row::<String>(&["a"], vec![k1]);
    search.tuple = Some(vec![incoming_x()]);
    search.item_failed = vec![pair];
    let mut txn = begin_chained(&handle, &["a"]).await;
    let waiter = native_waiter(&mut txn, "a", "k1");
    let mut fire = native_fire_op("a", "x", &waiter);
    fire.own = true;
    fire.search = Some(Box::new(search));
    txn.produce(fire).expect("stage the own fire of x");
    assert_eq!(txn.produce(native_store_op("a", "y")).err(), Some(SpaceTxnError::RecordEnded));
    script.push(ReplayRow::matched(Some(comm)));
    let receipt = txn.commit().await.expect("the item failure gives CanStop");
    let cut = receipt.deviation.expect("the record end cuts the unit");
    assert!(cut.item_failed.is_empty(), "replay books no item pair");
    assert!(!cut.vetoing, "replay: a Matched row has no veto");
}

// Added by decision 124 (T-C4-25, V3-12, V4-5): the retire group of an actual
// class fire commits. The first retire is refused and parks inert through its
// machinery search, the second commits, and replay accepts the unit by
// CanStop(U).
#[tokio::test]
async fn retire_group_of_the_actual_class_fire_commits() {
    // Play: x fires k2 in place of k1, and its group retires r1 and r2.
    let space = funded_space_refusing(
        &[parked("k1"), parked("k2"), Op::produce("c", "r1", false), Op::produce("d", "r2", false)],
        "retire1",
    )
    .await;
    let k2 = consume_source(&["a"], &[Pat::Any], "k2", false);
    let mut search = scripted_row::<Pat>(&["a"], vec![k2]);
    search.tuple = Some(vec![incoming_x()]);
    search.retires =
        vec![machinery_retire("c", "r1", "retire1"), machinery_retire("d", "r2", "retire2")];
    let mut txn = staged_fire_of_k1(&space, search).await;
    let receipt = txn.commit().await.expect("commit the unit of play");
    assert!(matches!(receipt.ops.as_slice(), [
        SpaceOpReceipt::Fired { .. },
        SpaceOpReceipt::Parked { .. },
        SpaceOpReceipt::Fired { .. },
    ]));
    assert_eq!(receipt.deviation.as_ref().expect("row d deviates").index, 0);
    let r1 = datum("c", "r1", false).source.hash;
    let retire1 = consume_source(&["c"], &[Pat::Any], "retire1", false);
    assert_eq!(receipt.refused, vec![(r1, retire1.hash.clone())]);
    assert_eq!(space.get_data(&s("c")).await.len(), 1, "the refused retire keeps r1");
    assert!(space.get_data(&s("d")).await.is_empty(), "the second retire commits");
    let waiters = space.get_waiting_continuations(names(&["c"])).await;
    assert!(
        matches!(waiters.as_slice(), [waiter] if waiter.source == retire1),
        "the refused retire waits on c"
    );

    // Replay: the record names k2. The retire of r1 parks with its recorded
    // refusal, the retire of r2 matches, and the record ends after the unit.
    let script = ReplayScript::default();
    let (_session, handle) = replay_session(&script).await;
    native_park(&script, &handle, "a", "k1").await;
    native_park(&script, &handle, "a", "k2").await;
    native_store(&script, &handle, "c", "r1").await;
    native_store(&script, &handle, "d", "r2").await;
    let k2 = native_consume_source("a", "k2");
    let comm_k2 = recorded_comm(&[incoming_x()], k2.clone());
    script.record([native_row_at(10, NativeReplayOutcome::Matched, Some(comm_k2.clone())), None]);
    let mut search = scripted_row::<String>(&["a"], vec![k2]);
    search.tuple = Some(vec![incoming_x()]);
    search.retires =
        vec![native_take_retire("c", "r1", "retire1"), native_take_retire("d", "r2", "retire2")];
    let mut txn = begin_chained(&handle, &["a", "c", "d"]).await;
    let waiter = native_waiter(&mut txn, "a", "k1");
    let mut fire = native_fire_op("a", "x", &waiter);
    fire.own = true;
    fire.search = Some(Box::new(search));
    txn.produce(fire).expect("stage the own fire of x");
    assert_eq!(txn.produce(native_store_op("a", "y")).err(), Some(SpaceTxnError::RecordEnded));
    let retire1 = native_consume_source("c", "retire1");
    script.push(ReplayRow::matched(Some(comm_k2)));
    script.push(ReplayRow {
        outcome: NativeReplayOutcome::Stored,
        candidate: None,
        refusal: Some(recorded_comm(&[bound("c", "r1")], retire1.clone())),
    });
    script.push(ReplayRow::matched(Some(recorded_comm(
        &[bound("d", "r2")],
        native_consume_source("d", "retire2"),
    ))));
    let receipt = txn
        .commit()
        .await
        .expect("the refused retire gives CanStop(U)");
    assert!(matches!(receipt.ops.as_slice(), [
        SpaceOpReceipt::Fired { .. },
        SpaceOpReceipt::Parked { .. },
        SpaceOpReceipt::Fired { .. },
    ]));
    assert_eq!(receipt.deviation.as_ref().expect("row d deviates").index, 0);
    let r1 = produce_source("c", "r1", false).hash;
    assert_eq!(receipt.refused, vec![(r1, retire1.hash)]);
}

// Added by decision 124 (T-P-1, OP-TP1, V3-15): the cost of a row search is
// bounded by the product of its window, with no clock. A search that matches
// nothing walks each record for each waiter, and so does its enumeration. A
// search that fires on the second waiter does not reach the third.
#[tokio::test]
async fn row_search_cost_is_bounded_by_the_window_product() {
    let host_ops = [
        parked("k1"),
        parked("k2"),
        parked("k3"),
        Op::produce("c", "r1", false),
        Op::produce("c", "r2", false),
    ];
    let listed = vec![
        consume_source(&["a"], &[Pat::Any], "k1", false),
        consume_source(&["a"], &[Pat::Any], "k2", false),
        consume_source(&["a"], &[Pat::Any], "k3", false),
    ];
    let waiters = listed.len();
    let records = 2;

    // Nothing matches, so the row stores x after the full walk.
    let space = funded_space(&host_ops).await;
    let mut search = scripted_row::<Pat>(&["a"], listed.clone());
    search.window = Some(s("c"));
    let probe = Arc::clone(&search.probe);
    let mut txn = staged_fire_of_k1(&space, search).await;
    let receipt = txn.commit().await.expect("commit the miss");
    assert!(matches!(receipt.ops.as_slice(), [SpaceOpReceipt::Stored { .. }]));
    assert_eq!(probe.tuples.load(Ordering::Relaxed), waiters * records, "the tuple bound");
    assert_eq!(probe.item_tests.load(Ordering::Relaxed), waiters * records, "the item bound");

    // The search fires on k2, so it does not reach k3.
    let space = funded_space(&host_ops).await;
    let mut search = scripted_row::<Pat>(&["a"], listed);
    search.window = Some(s("c"));
    search.fire_on = Some(s("k2"));
    search.tuple = Some(vec![incoming_x()]);
    let probe = Arc::clone(&search.probe);
    let mut txn = staged_fire_of_k1(&space, search).await;
    let receipt = txn.commit().await.expect("commit the alternative");
    assert!(matches!(receipt.ops.as_slice(), [SpaceOpReceipt::Fired { .. }]));
    let tuples = probe.tuples.load(Ordering::Relaxed);
    assert_eq!(tuples, 2 * records, "k3 is not tried");
    assert!(tuples <= waiters * records, "the tuple bound");
    assert_eq!(probe.item_tests.load(Ordering::Relaxed), 0, "a match walks no item");
}
