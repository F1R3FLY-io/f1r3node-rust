// Property tests of the Reified RSpaces storage substrate. Each property is
// the executable form of headline theorems of the Rocq models
// (EventCorrelation.v H1–H12 and H17–H23, VolatileSweep.v H13–H16) and of
// TLA+ invariants (CheckpointReplay.tla T1–T4 and T6, TxnLockCoverage T5);
// the mapping lives in formal/reified-rspaces-invariant-map.json. Runs are
// deterministic (fixed seed) and never write to the source tree (no failure
// persistence). Where a property allows it, the generators also produce
// groups that are not canonical (a permutation, or a repeat out of sorted
// order), which the substrate must reject with no effect and the host oracle
// skips, and sorted groups that repeat a channel, which the substrate
// accepts (FR-1').

#[path = "reified_space_store_handle/support.rs"]
mod support;

use std::cell::RefCell;
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::hash::{Hash, Hasher};
use std::pin::Pin;
use std::sync::Arc;

use futures::FutureExt;
use proptest::prelude::*;
use proptest::sample::{select, subsequence};
use proptest::test_runner::RngSeed;
use rspace_plus_plus::rspace::errors::RSpaceError;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::hashing::stable_hash_provider::{hash, hash_from_vec};
use rspace_plus_plus::rspace::internal::{Datum, WaitingContinuation};
use rspace_plus_plus::rspace::operation_context::{self, CausalPath, OperationOrder};
use rspace_plus_plus::rspace::rspace::RSpace;
use rspace_plus_plus::rspace::rspace_interface::{
    ISpace, RSpaceOperationCompletion, ReplayOperationDirective,
};
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use rspace_plus_plus::rspace::space_store_handle::{
    BoxedSpaceTxn, ConsumeOutcome, ProduceOutcome, SpaceConsume, SpaceOpReceipt, SpaceProduce,
    SpaceStoreHandle, SpaceTxnError, SweepReport, bind_candidate, incoming_candidate, sub_order,
};
use rspace_plus_plus::rspace::trace::event::{COMM, Consume, Event, IOEvent, Produce};
use serde::{Deserialize, Serialize};
use support::*;

const SEED: u64 = 0x0E0_5EED_2026_1004;

fn config() -> ProptestConfig {
    ProptestConfig {
        cases: 64,
        rng_seed: RngSeed::Fixed(SEED),
        failure_persistence: None,
        ..ProptestConfig::default()
    }
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("build a test runtime")
}

const CHANNELS: [&str; 3] = ["a", "b", "c"];
const VALUES: [&str; 2] = ["x", "y"];
const VOLATILE: [&str; 2] = ["v1", "v2"];
const DURABLE: [&str; 2] = ["a", "b"];
const REPEATING: [&str; 2] = ["a", "b"];
/// Probability that a generated group is not canonical: a permutation, or a
/// repeat out of sorted order.
const ODD: f64 = 0.15;
/// Probability that a canonical group repeats one of its channels (FR-1').
const REPEAT: f64 = 0.15;
/// Probability that a generated consume peeks, where the property allows
/// peeks.
const PEEK: f64 = 0.6;

fn pattern(values: &'static [&'static str]) -> impl Strategy<Value = Pat> {
    prop_oneof![
        2 => Just(Pat::Any),
        1 => select(values.to_vec()).prop_map(|value| Pat::Exact(s(value))),
    ]
}

fn produce_op(
    channels: &'static [&'static str],
    values: &'static [&'static str],
) -> impl Strategy<Value = Op> {
    (select(channels.to_vec()), select(values.to_vec()), proptest::bool::weighted(0.2))
        .prop_map(|(channel, value, persist)| Op::produce(channel, value, persist))
}

// A group of one or two distinct channels in canonical order. With
// probability `odd` it is not canonical instead (two channels reversed, or
// the smaller channel repeated after the larger one), and the substrate
// rejects it; that needs two channels. Otherwise, with probability `repeat`,
// one of its channels repeats in sorted position, which the substrate
// accepts (FR-1').
fn group(channels: Vec<String>, odd: f64, repeat: f64) -> impl Strategy<Value = Vec<String>> {
    let most = channels.len().min(2);
    (
        subsequence(channels.clone(), 1..=most),
        subsequence(channels, most..=most),
        proptest::bool::weighted(odd),
        proptest::bool::weighted(repeat),
        any::<bool>(),
        any::<usize>(),
    )
        .prop_map(|(joined, pair, odd, repeat, permute, pick)| {
            let mut pair = pair;
            pair.sort_by_key(hash);
            match (odd, pair.as_slice()) {
                (true, [smaller, larger]) => match permute {
                    true => vec![larger.clone(), smaller.clone()],
                    false => vec![smaller.clone(), larger.clone(), smaller.clone()],
                },
                _ => {
                    let mut group = joined;
                    group.sort_by_key(hash);
                    if repeat {
                        let index = pick % group.len();
                        let channel = group[index].clone();
                        group.insert(index, channel);
                    }
                    group
                }
            }
        })
}

fn consume_on(
    channels: Vec<String>,
    values: &'static [&'static str],
    peeks: bool,
    odd: f64,
    repeat: f64,
) -> impl Strategy<Value = Op> {
    (
        group(channels, odd, repeat),
        select(vec!["k1", "k2"]),
        proptest::bool::weighted(0.2),
        proptest::bool::weighted(if peeks { PEEK } else { 0.0 }),
    )
        .prop_flat_map(move |(joined, continuation, persist, peek)| {
            let binds = joined.len();
            (
                Just(joined),
                proptest::collection::vec(pattern(values), binds),
                Just(continuation),
                Just(persist),
                Just(peek),
            )
        })
        .prop_map(|(joined, patterns, continuation, persist, peek)| {
            let channels: Vec<&str> = joined.iter().map(String::as_str).collect();
            match peek {
                true => Op::peek(&channels, patterns, continuation),
                false => Op::consume(&channels, patterns, continuation, persist),
            }
        })
}

fn consume_op(
    channels: &'static [&'static str],
    values: &'static [&'static str],
    peeks: bool,
    odd: f64,
    repeat: f64,
) -> impl Strategy<Value = Op> {
    consume_on(names(channels), values, peeks, odd, repeat)
}

fn op(peeks: bool, odd: f64, repeat: f64) -> impl Strategy<Value = Op> {
    prop_oneof![produce_op(&CHANNELS, &VALUES), consume_op(&CHANNELS, &VALUES, peeks, odd, repeat)]
}

fn matches_value(pattern: &Pat, value: &str) -> bool {
    match pattern {
        Pat::Any => true,
        Pat::Exact(expected) => expected == value,
    }
}

// FR-1': a produce-side fire binds the incoming datum once, but the host
// binds a persistent incoming datum once per pattern of its channel that
// matches it, a fire that the substrate refuses. So the oracle makes a
// persistent produce non-persistent when an earlier consume could wait on a
// group where two patterns of the produced channel match the produced value.
fn binds_incoming_once(groups: Vec<Vec<Op>>) -> Vec<Vec<Op>> {
    let mut waiting: Vec<(Vec<String>, Vec<Pat>)> = Vec::new();
    let mut normalized = Vec::with_capacity(groups.len());
    for group in groups {
        let mut ops = Vec::with_capacity(group.len());
        for op in group {
            match op {
                Op::Produce {
                    channel,
                    data,
                    persist,
                } => {
                    let twice = waiting.iter().any(|(channels, patterns)| {
                        channels
                            .iter()
                            .zip(patterns)
                            .filter(|(joined, pattern)| {
                                **joined == channel && matches_value(pattern, &data)
                            })
                            .count() >
                            1
                    });
                    ops.push(Op::Produce {
                        channel,
                        data,
                        persist: persist && !twice,
                    });
                }
                consume => {
                    if let (
                        Op::Consume {
                            channels, patterns, ..
                        },
                        None,
                    ) = (&consume, consume.rejection())
                    {
                        waiting.push((channels.clone(), patterns.clone()));
                    }
                    ops.push(consume);
                }
            }
        }
        normalized.push(ops);
    }
    normalized
}

fn op_groups(peeks: bool, odd: f64) -> impl Strategy<Value = Vec<Vec<Op>>> {
    proptest::collection::vec(proptest::collection::vec(op(peeks, odd, REPEAT), 1..=4), 1..=5)
        .prop_map(binds_incoming_once)
}

// Groups for the peek property (H11, FR-10). Waiters outnumber sends two to
// one, and most waiters peek. So peek fires on one channel and on joins,
// re-offers that store or fire the next waiter, re-offers that fire a
// further peek waiter, and persistent sends that fire a peek waiter all
// occur over the fixed seed. Groups may be odd or repeat a channel, as in
// `op_groups`.
fn peek_op_groups() -> impl Strategy<Value = Vec<Vec<Op>>> {
    let op = prop_oneof![
        1 => produce_op(&CHANNELS, &VALUES),
        2 => consume_op(&CHANNELS, &VALUES, true, ODD, REPEAT),
    ];
    proptest::collection::vec(proptest::collection::vec(op, 1..=4), 1..=8)
        .prop_map(binds_incoming_once)
}

// Groups with no repeated channel, for the properties that replay through
// the host: host replay binds the records of a repeated channel in store
// order where play binds them in hash order, so it can compute another COMM
// than the log and fail its assertion (HEAD behavior, see
// `space_store_handle_host_replay_repeated_channel_control`).
fn host_replay_op_groups() -> impl Strategy<Value = Vec<Vec<Op>>> {
    proptest::collection::vec(proptest::collection::vec(op(false, ODD, 0.0), 1..=4), 1..=5)
}

// Volatile and durable channels never share a join group.
fn volatile_op_groups() -> impl Strategy<Value = Vec<Vec<Op>>> {
    let op = prop_oneof![
        produce_op(&VOLATILE, &VALUES),
        consume_op(&VOLATILE, &VALUES, false, ODD, REPEAT),
        produce_op(&DURABLE, &VALUES),
        consume_op(&DURABLE, &VALUES, false, ODD, REPEAT),
    ];
    proptest::collection::vec(proptest::collection::vec(op, 1..=4), 1..=5)
        .prop_map(binds_incoming_once)
}

// Few distinct produces, so identical produces repeat and counters grow.
fn repeating_op_groups() -> impl Strategy<Value = Vec<Vec<Op>>> {
    let op = prop_oneof![
        2 => produce_op(&["a", "b"], &["x"]),
        1 => consume_op(&["a", "b"], &["x"], false, ODD, REPEAT),
    ];
    proptest::collection::vec(proptest::collection::vec(op, 1..=4), 1..=5)
        .prop_map(binds_incoming_once)
}

// Groups over two channels that repeat a channel in sorted order with high
// probability, with a few groups that are not canonical mixed in (FR-1').
fn repeated_channel_op_groups() -> impl Strategy<Value = Vec<Vec<Op>>> {
    let op = prop_oneof![
        2 => produce_op(&REPEATING, &VALUES),
        2 => consume_op(&REPEATING, &VALUES, false, 0.1, 0.7),
    ];
    proptest::collection::vec(proptest::collection::vec(op, 1..=4), 1..=6)
        .prop_map(binds_incoming_once)
}

// A setup and an aborted group, normalized together as one run.
fn abort_case() -> impl Strategy<Value = (Vec<Vec<Op>>, Vec<Op>)> {
    (
        proptest::collection::vec(proptest::collection::vec(op(false, ODD, REPEAT), 1..=4), 0..=3),
        proptest::collection::vec(op(false, 0.4, REPEAT), 1..=4),
    )
        .prop_map(|(setup, aborted)| {
            let mut all = setup;
            all.push(aborted);
            let mut all = binds_incoming_once(all);
            let aborted = all.pop().unwrap_or_default();
            (all, aborted)
        })
}

fn assert_committed(receipts: Commits) -> Result<Vec<Receipt>, TestCaseError> {
    let mut committed = Vec::with_capacity(receipts.len());
    for receipt in receipts {
        match receipt {
            Ok(receipt) => committed.push(receipt),
            Err(error) => return Err(TestCaseError::fail(format!("commit failed: {:?}", error))),
        }
    }
    Ok(committed)
}

fn staged(plan: &Planned, txn: &mut Txn) -> Result<(), TestCaseError> {
    plan.stage_expected(txn).map_err(TestCaseError::fail)
}

fn op_channels(op: &Op) -> Vec<String> {
    match op {
        Op::Produce { channel, .. } => vec![channel.clone()],
        Op::Consume { channels, .. } => channels.clone(),
    }
}

fn fired_comms(receipts: &[Receipt]) -> Vec<Vec<u8>> {
    receipts
        .iter()
        .flat_map(|receipt| receipt.ops.iter())
        .filter_map(|op| match op {
            SpaceOpReceipt::Fired { comm, .. } => {
                Some(bincode::serialize(comm).expect("serialize a COMM"))
            }
            _ => None,
        })
        .collect()
}

fn comm_bytes(log: &[Event]) -> Vec<Vec<u8>> {
    comms_of(log)
        .iter()
        .map(|comm| bincode::serialize(comm).expect("serialize a COMM"))
        .collect()
}

// H1: committing staged operations yields the host's observer calls and events.
async fn play_trace_equals_host(groups: Vec<Vec<Op>>) -> Result<(), TestCaseError> {
    let host = play_space().await;
    let host_recorder = Recorder::new();
    observed(&host, &host_recorder);
    let txn = play_space().await;
    let txn_recorder = Recorder::new();
    observed(&txn, &txn_recorder);
    let (_, receipts) = run_guided(&host, &txn, &groups).await;
    assert_committed(receipts)?;
    prop_assert_eq!(txn_recorder.calls(), host_recorder.calls());
    prop_assert_eq!(
        event_bytes(&txn.take_event_log().await),
        event_bytes(&host.take_event_log().await)
    );
    Ok(())
}

// H2, T2: the committed state, and so the checkpoint root, equals the host's.
async fn play_root_equals_host(groups: Vec<Vec<Op>>) -> Result<(), TestCaseError> {
    let host = play_space().await;
    let txn = play_space().await;
    let (_, receipts) = run_guided(&host, &txn, &groups).await;
    assert_committed(receipts)?;
    prop_assert_eq!(logical_state(&txn), logical_state(&host));
    let host_checkpoint = checkpoint(&host).await;
    let txn_checkpoint = checkpoint(&txn).await;
    prop_assert_eq!(txn_checkpoint.root, host_checkpoint.root);
    prop_assert_eq!(event_bytes(&txn_checkpoint.log), event_bytes(&host_checkpoint.log));
    Ok(())
}

// H5, T1: replay pairs every COMM of the play log once and reaches the play
// root.
async fn replay_pairs_play_log(groups: Vec<Vec<Op>>) -> Result<(), TestCaseError> {
    let plans = plan_groups(&groups).await;
    let (play, replay) = play_and_replay().await;
    let start = checkpoint(&play).await;
    let played_receipts =
        assert_committed(run_plans(&SpaceStoreHandle::new(play.clone()), &plans).await)?;
    let played = checkpoint(&play).await;
    rig(&replay, &start.root, &played.log).await;
    let replayed_receipts =
        assert_committed(run_plans(&SpaceStoreHandle::new(replay.clone()), &plans).await)?;
    prop_assert_eq!(replayed_receipts, played_receipts);
    prop_assert!(replay_bindings(&replay).is_empty());
    match replay.create_checkpoint().await {
        Ok(replayed) => prop_assert_eq!(replayed.root, played.root),
        Err(error) => return Err(TestCaseError::fail(format!("replay checkpoint: {:?}", error))),
    }
    Ok(())
}

// T1: a host play log replays through transactions and a transaction play
// log replays through the host; both play logs are equal.
async fn cross_replay_with_host(groups: Vec<Vec<Op>>) -> Result<(), TestCaseError> {
    let plans = plan_groups(&groups).await;
    let ops = flatten(&groups);

    let (host_play, txn_replay) = play_and_replay().await;
    let host_start = checkpoint(&host_play).await;
    run_host_ops(&host_play, &ops).await;
    let host_played = checkpoint(&host_play).await;
    rig(&txn_replay, &host_start.root, &host_played.log).await;
    assert_committed(run_plans(&SpaceStoreHandle::new(txn_replay.clone()), &plans).await)?;
    match txn_replay.create_checkpoint().await {
        Ok(replayed) => prop_assert_eq!(replayed.root, host_played.root.clone()),
        Err(error) => return Err(TestCaseError::fail(format!("transaction replay: {:?}", error))),
    }

    let (txn_play, host_replay) = play_and_replay().await;
    let txn_start = checkpoint(&txn_play).await;
    assert_committed(run_plans(&SpaceStoreHandle::new(txn_play.clone()), &plans).await)?;
    let txn_played = checkpoint(&txn_play).await;
    rig(&host_replay, &txn_start.root, &txn_played.log).await;
    run_host_ops(&host_replay, &ops).await;
    match host_replay.create_checkpoint().await {
        Ok(replayed) => prop_assert_eq!(replayed.root, txn_played.root.clone()),
        Err(error) => return Err(TestCaseError::fail(format!("host replay: {:?}", error))),
    }
    prop_assert_eq!(event_bytes(&txn_played.log), event_bytes(&host_played.log));
    prop_assert_eq!(txn_played.root, host_played.root);
    Ok(())
}

// A fire whose COMM was never recorded: a fresh produced value, or a fresh
// continuation for a consume.
fn mutate(plan: &Planned) -> Planned {
    match plan.clone() {
        Planned::Produce {
            channel,
            persist,
            outcome:
                ProduceOutcome::Fire {
                    channels,
                    continuation,
                    data,
                },
            ..
        } => {
            let mutant = s("mutant");
            let data = data
                .into_iter()
                .map(|candidate| match candidate.datum_index {
                    -1 => incoming_candidate(
                        candidate.channel.clone(),
                        Produce::create(&channel, &mutant, persist),
                        mutant.clone(),
                        persist,
                        mutant.clone(),
                    ),
                    _ => candidate,
                })
                .collect();
            Planned::Produce {
                channel,
                data: mutant,
                persist,
                outcome: ProduceOutcome::Fire {
                    channels,
                    continuation,
                    data,
                },
            }
        }
        Planned::Consume {
            channels,
            patterns,
            persist,
            peeks,
            outcome: ConsumeOutcome::Fire { data },
            ..
        } => Planned::Consume {
            channels,
            patterns,
            continuation: s("mutant"),
            persist,
            peeks,
            outcome: ConsumeOutcome::Fire { data },
        },
        other => other,
    }
}

// The staged view through the transaction's read API.
fn txn_view(txn: &mut Txn, produces: &[Produce]) -> (ChannelView, BTreeMap<Produce, i32>) {
    let channels = universe();
    let mut groups: BTreeSet<Vec<String>> = channels.iter().map(|c| vec![c.clone()]).collect();
    let mut data = Vec::with_capacity(channels.len());
    let mut joins = Vec::with_capacity(channels.len());
    for channel in &channels {
        data.push(
            txn.data(channel)
                .expect("read staged data")
                .iter()
                .map(|record| bincode::serialize(record).expect("serialize a datum"))
                .collect(),
        );
        let channel_joins = txn.joins(channel).expect("read staged joins").to_vec();
        groups.extend(channel_joins.iter().cloned());
        joins.push(channel_joins);
    }
    let mut continuations = Vec::with_capacity(groups.len());
    for group in &groups {
        continuations.push(
            txn.continuations(group)
                .expect("read staged continuations")
                .iter()
                .map(|wk| bincode::serialize(wk).expect("serialize a continuation"))
                .collect(),
        );
    }
    let counters = txn
        .produce_counters(produces)
        .expect("read the staged counters");
    ((data, continuations, joins), counters)
}

fn plan_produces(plan: &Planned) -> Vec<Produce> {
    match plan {
        Planned::Produce {
            channel,
            data,
            persist,
            outcome,
        } => {
            let mut produces = vec![Produce::create(channel, data, *persist)];
            if let ProduceOutcome::Fire { data, .. } = outcome {
                produces.extend(data.iter().map(|candidate| candidate.datum.source.clone()));
            }
            produces
        }
        Planned::Consume {
            outcome: ConsumeOutcome::Fire { data },
            ..
        } => data
            .iter()
            .map(|candidate| candidate.datum.source.clone())
            .collect(),
        Planned::Consume { .. } | Planned::Rejected { .. } => Vec::new(),
    }
}

// The observer calls of an unpaired replay fire at commit, all for the
// source `source`: the start, the directive query, the introduction, the
// COMM call with `comm`, and the rejected finish (decision 36).
fn check_unpaired_calls(calls: &[Call], source: &[u8], comm: &COMM) -> Result<(), TestCaseError> {
    let comm_bytes = bincode::serialize(comm).expect("serialize a COMM");
    let (start, directive, introduction, observed, finish) = match calls {
        [
            Call::Start { source: start, .. },
            Call::Directive { source: directive },
            Call::Produce {
                source: introduction,
                ..
            } |
            Call::Consume {
                source: introduction,
                ..
            },
            Call::Comm { comm: observed, .. },
            Call::Finish {
                source: finish,
                completion: RSpaceOperationCompletion::Rejected,
            },
        ] => (start, directive, introduction, observed, finish),
        other => {
            return Err(TestCaseError::fail(format!(
                "the calls of the unpaired fire: {:?}",
                other
            )));
        }
    };
    for called in [start, directive, introduction, finish] {
        prop_assert_eq!(called.as_slice(), source);
    }
    prop_assert_eq!(observed, &comm_bytes);
    Ok(())
}

// H4, T3 (decision 36): a fire whose COMM the log does not hold stages as
// in play and stays unpaired. Staging it and a store after it leaves the
// bindings, the store and the observer untouched, and the staged view
// equals the view of play after the same operations. The commit applies the
// operations before the fire, makes the calls of the fire through its COMM
// call, and fails with the typed `ReplayDivergence`. The state, the bindings
// and the counters then equal those of a replay that commits only the
// operations before the fire, so nothing of the fire or after it is logged
// or applied. A new transaction with the recorded fire and the rest then
// reaches the play root.
async fn replay_divergence_typed_at_commit(groups: Vec<Vec<Op>>) -> Result<(), TestCaseError> {
    let plans = plan_groups(&groups).await;
    let flat: Vec<Planned> = plans.iter().flatten().cloned().collect();
    let target = flat.iter().position(Planned::is_fire);
    prop_assume!(target.is_some());
    let target = target.unwrap_or_default();

    let (play, replay) = play_and_replay().await;
    let start = checkpoint(&play).await;
    assert_committed(run_plans(&SpaceStoreHandle::new(play.clone()), &plans).await)?;
    let played = checkpoint(&play).await;
    let recorder = Recorder::new();
    observed(&replay, &recorder);
    rig(&replay, &start.root, &played.log).await;

    let mutant = mutate(&flat[target]);
    let after = Planned::Produce {
        channel: s("v2"),
        data: s("after"),
        persist: false,
        outcome: ProduceOutcome::Store,
    };
    let produces: Vec<Produce> = plan_produces(&flat[target])
        .into_iter()
        .chain(plan_produces(&mutant))
        .chain(plan_produces(&after))
        .collect();
    let handle = SpaceStoreHandle::new(replay.clone());
    let mut txn = handle.begin(&universe()).await.expect("begin");
    for plan in &flat[..target] {
        staged(plan, &mut txn)?;
    }
    let bindings = replay_bindings(&replay);
    let state = replay_state(&replay);
    let comm = match mutant.stage(&mut txn, None).map(|staged| staged.comm) {
        Ok(Some(comm)) => comm,
        other => {
            return Err(TestCaseError::fail(format!("the unpaired fire stages: {:?}", other)));
        }
    };
    staged(&after, &mut txn)?;
    prop_assert_eq!(replay_bindings(&replay), bindings);
    prop_assert_eq!(replay_state(&replay), state);
    prop_assert!(recorder.calls().is_empty());
    let twin = play_space().await;
    let twin_handle = SpaceStoreHandle::new(twin.clone());
    let mut twin_txn = twin_handle.begin(&universe()).await.expect("begin");
    for plan in flat[..target].iter().chain([&mutant, &after]) {
        staged(plan, &mut twin_txn)?;
    }
    prop_assert_eq!(txn_view(&mut txn, &produces), txn_view(&mut twin_txn, &produces));
    drop(twin_txn);
    prop_assert_eq!(
        txn.commit().await,
        Err(SpaceTxnError::ReplayDivergence(Box::new(comm.clone())))
    );

    let reference = replay_over(&play);
    let reference_recorder = Recorder::new();
    observed(&reference, &reference_recorder);
    rig(&reference, &start.root, &played.log).await;
    let reference_handle = SpaceStoreHandle::new(reference.clone());
    let mut reference_txn = reference_handle.begin(&universe()).await.expect("begin");
    for plan in &flat[..target] {
        staged(plan, &mut reference_txn)?;
    }
    if let Err(error) = reference_txn.commit().await {
        return Err(TestCaseError::fail(format!("reference prefix commit: {:?}", error)));
    }
    prop_assert_eq!(replay_state(&replay), replay_state(&reference));
    prop_assert_eq!(replay_bindings(&replay), replay_bindings(&reference));
    let prefix = reference_recorder.calls();
    let calls = recorder.calls();
    prop_assert!(calls.starts_with(&prefix));
    check_unpaired_calls(&calls[prefix.len()..], &source_of(&mutant), &comm)?;
    let mut probe = handle.begin(&universe()).await.expect("begin");
    let mut reference_probe = reference_handle.begin(&universe()).await.expect("begin");
    prop_assert_eq!(
        counters_of(&mut probe, &produces)?,
        counters_of(&mut reference_probe, &produces)?
    );
    drop(probe);
    drop(reference_probe);

    let mut txn = handle.begin(&universe()).await.expect("begin");
    for plan in &flat[target..] {
        staged(plan, &mut txn)?;
    }
    if let Err(error) = txn.commit().await {
        return Err(TestCaseError::fail(format!("commit after the divergence: {:?}", error)));
    }
    match replay.create_checkpoint().await {
        Ok(replayed) => prop_assert_eq!(replayed.root, played.root),
        Err(error) => return Err(TestCaseError::fail(format!("replay checkpoint: {:?}", error))),
    }
    Ok(())
}

// H6: what a transaction reads after staging equals the store after commit.
// The groups include peek fires and their re-offers (FR-10): a peek fire
// never puts its incoming datum into the view.
async fn view_equals_committed_store(groups: Vec<Vec<Op>>) -> Result<(), TestCaseError> {
    let plans = plan_groups(&groups).await;
    let space = play_space().await;
    let handle = SpaceStoreHandle::new(space.clone());
    for group in &plans {
        let mut txn = handle.begin(&universe()).await.expect("begin");
        for plan in group {
            staged(plan, &mut txn)?;
        }
        let (view, _) = txn_view(&mut txn, &[]);
        if let Err(error) = txn.commit().await {
            return Err(TestCaseError::fail(format!("commit: {:?}", error)));
        }
        let channels = universe();
        let mut keys: BTreeSet<Vec<String>> = channels.iter().map(|c| vec![c.clone()]).collect();
        for channel in &channels {
            keys.extend(space.get_joins(channel.clone()).await);
        }
        for joins in &view.2 {
            keys.extend(joins.iter().cloned());
        }
        let keys: Vec<Vec<String>> = keys.into_iter().collect();
        prop_assert_eq!(view, channel_view(&space, &channels, &keys).await);
    }
    Ok(())
}

async fn lockstep_holds(space: &Space) -> Result<(), TestCaseError> {
    for channel in universe() {
        for group in space.get_joins(channel.clone()).await {
            prop_assert!(group.contains(&channel));
            prop_assert!(
                !space
                    .get_waiting_continuations(group.clone())
                    .await
                    .is_empty()
            );
        }
    }
    for (group, waiting) in space.get_store().snapshot().continuations_flat() {
        if waiting.is_empty() {
            continue;
        }
        for channel in &group {
            prop_assert!(space.get_joins(channel.clone()).await.contains(&group));
        }
    }
    Ok(())
}

// H7: join entries and continuations change in lockstep, in the view and
// in the committed store.
async fn joins_lockstep(groups: Vec<Vec<Op>>) -> Result<(), TestCaseError> {
    let plans = plan_groups(&groups).await;
    let space = play_space().await;
    let handle = SpaceStoreHandle::new(space.clone());
    for group in &plans {
        let mut txn = handle.begin(&universe()).await.expect("begin");
        for plan in group {
            staged(plan, &mut txn)?;
            for channel in universe() {
                let joins = txn.joins(&channel).expect("read staged joins").to_vec();
                for join in joins {
                    prop_assert!(join.contains(&channel));
                    prop_assert!(!txn.continuations(&join).expect("read").is_empty());
                }
            }
        }
        if let Err(error) = txn.commit().await {
            return Err(TestCaseError::fail(format!("commit: {:?}", error)));
        }
        lockstep_holds(&space).await?;
    }
    Ok(())
}

fn is_volatile(op: &Op) -> bool {
    op_channels(op)
        .iter()
        .any(|channel| VOLATILE.contains(&channel.as_str()))
}

// H13–H15, T4: the sweep removes exactly the volatile records, continuations
// and joins, leaves the durable state, and does not move the root.
async fn sweep_exact_and_root_neutral(groups: Vec<Vec<Op>>) -> Result<(), TestCaseError> {
    let plans = plan_groups(&groups).await;
    let space = play_space().await;
    let handle = SpaceStoreHandle::new(space.clone());
    assert_committed(run_plans(&handle, &plans).await)?;
    let reference = play_space().await;
    let durable: Vec<Op> = flatten(&groups)
        .into_iter()
        .filter(|op| !is_volatile(op))
        .collect();
    run_host_ops(&reference, &durable).await;

    let volatile = names(&VOLATILE);
    let mut expected = SweepReport {
        channels: volatile.len(),
        ..SweepReport::default()
    };
    let mut swept_groups: BTreeSet<Vec<String>> =
        volatile.iter().map(|c| vec![c.clone()]).collect();
    for channel in &volatile {
        expected.data += space.get_data(channel).await.len();
        let joins = space.get_joins(channel.clone()).await;
        expected.joins += joins.len();
        swept_groups.extend(joins);
    }
    for group in &swept_groups {
        expected.continuations += space.get_waiting_continuations(group.clone()).await.len();
    }
    let report = handle.sweep_volatile(&volatile).await;
    prop_assert_eq!(report, Ok(expected));
    for channel in &volatile {
        prop_assert!(space.get_data(channel).await.is_empty());
        prop_assert!(space.get_joins(channel.clone()).await.is_empty());
    }
    for group in &swept_groups {
        prop_assert!(
            space
                .get_waiting_continuations(group.clone())
                .await
                .is_empty()
        );
    }
    let durable_channels = names(&DURABLE);
    let durable_groups = groups_of(&[&space, &reference]).await;
    prop_assert_eq!(
        channel_view(&space, &durable_channels, &durable_groups).await,
        channel_view(&reference, &durable_channels, &durable_groups).await
    );
    prop_assert_eq!(checkpoint(&space).await.root, checkpoint(&reference).await.root);
    Ok(())
}

// H8: sub-orders sort after their parent and before the next step, never
// equal a reduction order, and keep a transaction's events contiguous.
async fn sub_orders_contiguous(
    participant: Vec<(u64, u64)>,
    step: u64,
    count: usize,
    reductions: Vec<Vec<(u64, u64)>>,
) -> Result<(), TestCaseError> {
    let order = |segments: Vec<(u64, u64)>| OperationOrder {
        session: [5; 32],
        path: CausalPath::from(segments),
    };
    let extend = |segment: (u64, u64)| {
        let mut path = participant.clone();
        path.push(segment);
        order(path)
    };
    let parent = extend((step, 0));
    let next = extend((step + 1, 0));
    let split = extend((step + 1, 1));
    let subs: Vec<OperationOrder> = (0..count).map(|index| sub_order(&parent, index)).collect();
    prop_assert!(parent < subs[0]);
    prop_assert!(subs.windows(2).all(|pair| pair[0] < pair[1]));
    prop_assert!(subs.iter().all(|sub| *sub < next && *sub < split));
    for reduction in reductions {
        let reduction = order(reduction);
        prop_assert!(subs.iter().all(|sub| *sub != reduction));
    }

    let space = play_space().await;
    let handle = SpaceStoreHandle::new(space.clone());
    operation_context::scope(parent.clone(), async {
        run_host(&space, &Op::produce("b", "before", false))
            .await
            .expect("host op before");
        let mut txn = handle.begin(&universe()).await.expect("begin");
        for index in 0..count {
            txn.produce(SpaceProduce {
                channel: s("a"),
                data: index.to_string(),
                persist: false,
                outcome: ProduceOutcome::Store,
                before_observe: None,
            })
            .expect("stage");
        }
        txn.commit().await.expect("commit");
    })
    .await;
    operation_context::scope(next, async {
        run_host(&space, &Op::produce("b", "after", false))
            .await
            .expect("host op after");
    })
    .await;
    let mut expected = Vec::with_capacity(count + 2);
    expected.push(Produce::create(&s("b"), &s("before"), false));
    expected.extend((0..count).map(|index| Produce::create(&s("a"), &index.to_string(), false)));
    expected.push(Produce::create(&s("b"), &s("after"), false));
    let logged: Vec<Produce> = space
        .take_event_log()
        .await
        .into_iter()
        .filter_map(|event| match event {
            Event::IoEvent(IOEvent::Produce(produce)) => Some(produce),
            _ => None,
        })
        .collect();
    prop_assert_eq!(logged, expected);
    Ok(())
}

fn counters_of(
    txn: &mut Txn,
    produces: &[Produce],
) -> Result<BTreeMap<Produce, i32>, TestCaseError> {
    txn.produce_counters(produces)
        .map_err(|error| TestCaseError::fail(format!("read the counters: {:?}", error)))
}

// H9: dropping a transaction without commit leaves no events, observer
// calls, writes or counter changes, and releases the locks. The aborted
// operations include groups that are not canonical, which fail closed, and
// sorted groups that repeat a channel.
async fn abort_inert(setup: Vec<Vec<Op>>, aborted: Vec<Op>) -> Result<(), TestCaseError> {
    let mut all = setup.clone();
    all.push(aborted);
    let plans = plan_groups(&all).await;
    let space = play_space().await;
    let recorder = Recorder::new();
    observed(&space, &recorder);
    let handle = SpaceStoreHandle::new(space.clone());
    assert_committed(run_plans(&handle, &plans[..setup.len()]).await)?;
    space.take_event_log().await;
    let produces: Vec<Produce> = plans[setup.len()].iter().flat_map(plan_produces).collect();
    let mut probe = handle.begin(&universe()).await.expect("begin");
    let counters = counters_of(&mut probe, &produces)?;
    drop(probe);
    let calls = recorder.calls();
    let state = logical_state(&space);
    let keys = continuation_keys(&**space.get_store());

    {
        let mut txn = handle.begin(&universe()).await.expect("begin");
        for plan in &plans[setup.len()] {
            staged(plan, &mut txn)?;
        }
    }

    prop_assert_eq!(recorder.calls(), calls);
    prop_assert_eq!(logical_state(&space), state);
    prop_assert!(
        continuation_keys(&**space.get_store())
            .iter()
            .all(|key| group_rejection(key).is_none())
    );
    prop_assert!(keys.is_subset(&continuation_keys(&**space.get_store())));
    prop_assert!(space.take_event_log().await.is_empty());
    match handle.begin(&universe()).now_or_never() {
        Some(Ok(mut probe)) => prop_assert_eq!(counters_of(&mut probe, &produces)?, counters),
        _ => return Err(TestCaseError::fail("the dropped transaction kept its locks")),
    }
    Ok(())
}

type Effects = (
    Vec<Produce>,
    Vec<Produce>,
    Vec<WaitingContinuation<Pat, String>>,
    Vec<WaitingContinuation<Pat, String>>,
);

fn multiset_changes(before: &[Vec<u8>], after: &[Vec<u8>]) -> (Vec<Vec<u8>>, Vec<Vec<u8>>) {
    let mut remaining = before.to_vec();
    let mut added = Vec::new();
    for record in after {
        match remaining.iter().position(|existing| existing == record) {
            Some(position) => {
                remaining.swap_remove(position);
            }
            None => added.push(record.clone()),
        }
    }
    (added, remaining)
}

fn state_effects(before: &LogicalState, after: &LogicalState) -> Effects {
    let empty = Vec::new();
    let mut effects: Effects = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let channels: BTreeSet<&String> = before.data.keys().chain(after.data.keys()).collect();
    for channel in channels {
        let (added, removed) = multiset_changes(
            before.data.get(channel).unwrap_or(&empty),
            after.data.get(channel).unwrap_or(&empty),
        );
        let source = |bytes: &Vec<u8>| {
            bincode::deserialize::<Datum<String>>(bytes)
                .expect("decode a datum")
                .source
        };
        effects.0.extend(added.iter().map(source));
        effects.1.extend(removed.iter().map(source));
    }
    let groups: BTreeSet<&Vec<String>> = before
        .continuations
        .keys()
        .chain(after.continuations.keys())
        .collect();
    for group in groups {
        let (added, removed) = multiset_changes(
            before.continuations.get(group).unwrap_or(&empty),
            after.continuations.get(group).unwrap_or(&empty),
        );
        let decode = |bytes: &Vec<u8>| {
            bincode::deserialize::<WaitingContinuation<Pat, String>>(bytes)
                .expect("decode a continuation")
        };
        effects.2.extend(added.iter().map(decode));
        effects.3.extend(removed.iter().map(decode));
    }
    effects
}

// H10: every committed store change has its event, and every COMM follows
// its trigger event. The commit-time correlation check runs in every build
// profile as well.
async fn coverage_holds(groups: Vec<Vec<Op>>) -> Result<(), TestCaseError> {
    let plans = plan_groups(&groups).await;
    let space = play_space().await;
    let handle = SpaceStoreHandle::new(space.clone());
    for group in &plans {
        let before = logical_state(&space);
        let receipts = run_plans(&handle, std::slice::from_ref(group)).await;
        assert_committed(receipts)?;
        let events = space.take_event_log().await;
        let (put, removed, parked, unparked) = state_effects(&before, &logical_state(&space));
        let comms: Vec<COMM> = comms_of(&events);
        for source in &put {
            prop_assert!(
                events.iter().any(
                    |event| matches!(event, Event::IoEvent(IOEvent::Produce(p)) if p == source)
                )
            );
        }
        for source in &removed {
            prop_assert!(comms.iter().any(|comm| comm.produces.contains(source)));
        }
        for waiting in &parked {
            prop_assert!(events.iter().any(
                |event| matches!(event, Event::IoEvent(IOEvent::Consume(c)) if *c == waiting.source)
            ));
        }
        for waiting in &unparked {
            prop_assert!(comms.iter().any(|comm| comm.consume == waiting.source));
        }
        for (position, event) in events.iter().enumerate() {
            if let Event::Comm(comm) = event {
                let triggered = events.iter().take(position).any(|earlier| match earlier {
                    Event::IoEvent(IOEvent::Consume(consume)) => *consume == comm.consume,
                    Event::IoEvent(IOEvent::Produce(produce)) => comm.produces.contains(produce),
                    Event::Comm(_) => false,
                });
                prop_assert!(triggered, "a COMM precedes its trigger event");
            }
        }
    }
    Ok(())
}

// FR-10: the receipt of a peek fire reports a retained stored record as
// persistent and every other result with its own flag, so the incoming
// datum of a produce fire is never reported retained. A fire that does not
// peek reports every own flag. A rejected plan has no receipt.
fn check_receipt_flags(plans: &[Vec<Planned>], receipts: &[Receipt]) -> Result<(), TestCaseError> {
    prop_assert_eq!(plans.len(), receipts.len());
    for (group, receipt) in plans.iter().zip(receipts) {
        let committed: Vec<&Planned> = group.iter().filter(|plan| !plan.is_rejected()).collect();
        prop_assert_eq!(committed.len(), receipt.ops.len());
        for (plan, op) in committed.into_iter().zip(&receipt.ops) {
            let (candidates, retain) = match plan {
                Planned::Produce {
                    outcome:
                        ProduceOutcome::Fire {
                            continuation, data, ..
                        },
                    ..
                } => (data, !continuation.peeks.is_empty()),
                Planned::Consume {
                    peeks,
                    outcome: ConsumeOutcome::Fire { data },
                    ..
                } => (data, !peeks.is_empty()),
                _ => continue,
            };
            let expected: Vec<bool> = candidates
                .iter()
                .map(|candidate| (retain && candidate.datum_index >= 0) || candidate.datum.persist)
                .collect();
            match op {
                SpaceOpReceipt::Fired { results, .. } => {
                    let flags: Vec<bool> = results.iter().map(|result| result.persistent).collect();
                    prop_assert_eq!(flags, expected);
                }
                other => {
                    return Err(TestCaseError::fail(format!(
                        "a planned fire committed as {:?}",
                        other
                    )));
                }
            }
        }
    }
    Ok(())
}

// H11 (FR-10): a peek fire has the observer calls and events of HEAD's peek
// path, including the reducer's re-produce of a non-persistent incoming
// datum, which the agent stages as the next produce of the transaction
// (`run_guided`, `head_peek_path`). The space keeps the stored records of a
// peek fire; the oracle puts them back into the host without events, so the
// states are equal as multisets and the roots are equal. Only retained
// stored records are reported persistent. The same transactions, re-offers
// included, replay and report from the play log.
async fn peek_retention(groups: Vec<Vec<Op>>) -> Result<(), TestCaseError> {
    let host = play_space().await;
    let host_recorder = Recorder::new();
    observed(&host, &host_recorder);
    let txn = play_space().await;
    let txn_recorder = Recorder::new();
    observed(&txn, &txn_recorder);
    let start = checkpoint(&txn).await;
    let (plans, receipts) = run_guided(&host, &txn, &groups).await;
    let receipts = assert_committed(receipts)?;
    check_receipt_flags(&plans, &receipts)?;
    prop_assert_eq!(txn_recorder.calls(), host_recorder.calls());
    prop_assert_eq!(logical_state(&txn).sorted(), logical_state(&host).sorted());
    let host_checkpoint = checkpoint(&host).await;
    let txn_checkpoint = checkpoint(&txn).await;
    prop_assert_eq!(event_bytes(&txn_checkpoint.log), event_bytes(&host_checkpoint.log));
    prop_assert_eq!(txn_checkpoint.root.clone(), host_checkpoint.root);

    let replay = replay_over(&txn);
    rig(&replay, &start.root, &txn_checkpoint.log).await;
    let replayed =
        assert_committed(run_plans(&SpaceStoreHandle::new(replay.clone()), &plans).await)?;
    prop_assert_eq!(&replayed, &receipts);
    prop_assert!(replay_bindings(&replay).is_empty());
    prop_assert_eq!(checkpoint(&replay).await.root, txn_checkpoint.root.clone());

    let reporting = reporting_over(&txn);
    reporting
        .rig_and_reset(start.root.clone(), txn_checkpoint.log.clone())
        .await
        .expect("rig the reporting space");
    let reported =
        assert_committed(run_plans(&SpaceStoreHandle::new(reporting.clone()), &plans).await)?;
    prop_assert_eq!(reported, receipts);
    let reported_root = ISpace::create_checkpoint(&reporting)
        .await
        .expect("checkpoint the reporting space")
        .root;
    prop_assert_eq!(reported_root, txn_checkpoint.root);
    Ok(())
}

// H12: reporting replays exactly like replay and reports what the host
// reporting space reports.
async fn reporting_equals_replay(groups: Vec<Vec<Op>>) -> Result<(), TestCaseError> {
    let plans = plan_groups(&groups).await;
    let play = play_space().await;
    let start = checkpoint(&play).await;
    assert_committed(run_plans(&SpaceStoreHandle::new(play.clone()), &plans).await)?;
    let played = checkpoint(&play).await;
    let replay = replay_over(&play);
    let replay_recorder = Recorder::new();
    observed(&replay, &replay_recorder);
    let reporting = reporting_over(&play);
    let reporting_recorder = Recorder::new();
    observed(&reporting, &reporting_recorder);
    let by_host = reporting_over(&play);
    rig(&replay, &start.root, &played.log).await;
    for space in [&reporting, &by_host] {
        space
            .rig_and_reset(start.root.clone(), played.log.clone())
            .await
            .expect("rig a reporting space");
    }
    let replayed =
        assert_committed(run_plans(&SpaceStoreHandle::new(replay.clone()), &plans).await)?;
    let reported =
        assert_committed(run_plans(&SpaceStoreHandle::new(reporting.clone()), &plans).await)?;
    run_host_ops(&by_host, &flatten(&groups)).await;
    prop_assert_eq!(reported, replayed);
    prop_assert_eq!(reporting_recorder.calls(), replay_recorder.calls());
    prop_assert_eq!(
        reporting.get_report().expect("reporting report"),
        by_host.get_report().expect("host report")
    );
    let replay_root = checkpoint(&replay).await.root;
    let reporting_root = ISpace::create_checkpoint(&reporting)
        .await
        .expect("checkpoint the reporting space")
        .root;
    prop_assert_eq!(reporting_root, replay_root.clone());
    prop_assert_eq!(replay_root, played.root);
    Ok(())
}

// H16: the root leaf record count is the number of records in the leaf.
async fn root_leaf_count(groups: Vec<Vec<Op>>) -> Result<(), TestCaseError> {
    let plans = plan_groups(&groups).await;
    let (play, replay) = play_and_replay().await;
    let handle = SpaceStoreHandle::new(play.clone());
    assert_committed(run_plans(&handle, &plans).await)?;
    let persisted = checkpoint(&play).await;
    let history = play.get_history_repository();
    let reader = history
        .get_history_reader(&persisted.root)
        .expect("read the root");
    replay
        .reset(&persisted.root)
        .await
        .expect("reset the replay space");
    let replay_handle = SpaceStoreHandle::new(replay.clone());
    for channel in universe() {
        let count = handle.root_leaf_record_count(&channel);
        let leaf = reader
            .get_data_proj_binary(&hash(&channel))
            .expect("read the leaf")
            .len() as u64;
        prop_assert_eq!(count.clone(), Ok(leaf));
        prop_assert_eq!(play.get_data(&channel).await.len() as u64, leaf);
        prop_assert_eq!(replay_handle.root_leaf_record_count(&channel), count);
    }
    Ok(())
}

// H3: play counts the produce being fired, replay increments before
// matching; both give the same COMMs, which equal the host's.
async fn counter_timing(groups: Vec<Vec<Op>>) -> Result<(), TestCaseError> {
    let plans = plan_groups(&groups).await;
    let (play, replay) = play_and_replay().await;
    let start = checkpoint(&play).await;
    let played_receipts =
        assert_committed(run_plans(&SpaceStoreHandle::new(play.clone()), &plans).await)?;
    let played = checkpoint(&play).await;
    let host = play_space().await;
    run_host_ops(&host, &flatten(&groups)).await;
    let host_log = host.take_event_log().await;
    rig(&replay, &start.root, &played.log).await;
    let replayed_receipts =
        assert_committed(run_plans(&SpaceStoreHandle::new(replay.clone()), &plans).await)?;
    let played_comms = fired_comms(&played_receipts);
    prop_assert_eq!(fired_comms(&replayed_receipts), played_comms.clone());
    prop_assert_eq!(comm_bytes(&played.log), played_comms);
    prop_assert_eq!(comm_bytes(&host_log), comm_bytes(&played.log));
    Ok(())
}

// The trigger event of a staged fire: `Produce(source)` for a produce-side
// fire, `Consume(source)` for a consume-side fire.
fn trigger(plan: &Planned) -> Option<IOEvent> {
    match plan {
        Planned::Produce {
            channel,
            data,
            persist,
            outcome: ProduceOutcome::Fire { .. },
        } => Some(IOEvent::Produce(Produce::create(channel, data, *persist))),
        Planned::Consume {
            channels,
            patterns,
            continuation,
            persist,
            outcome: ConsumeOutcome::Fire { .. },
            ..
        } => Some(IOEvent::Consume(Consume::create(channels, patterns, continuation, *persist))),
        _ => None,
    }
}

// H4 (trigger-key pairing, decision 36): replay pairs a COMM only under its
// trigger event. With every copy of one fire's trigger event removed from
// the log, the first fire with that trigger stages unpaired, and every
// operation stages. Staging leaves the bindings, the store and the observer
// untouched. The commit fails with `ReplayDivergence` and the COMM of play
// after the calls of the fire. The state, the bindings, the counters and
// the calls before the fire equal those of a replay of the same log that
// commits only the operations before the fire, so nothing of the fire or
// after it is logged or applied. Against the complete log the same
// operations before the fire and the fire pair and commit.
async fn replay_requires_trigger_event(
    groups: Vec<Vec<Op>>,
    pick: usize,
) -> Result<(), TestCaseError> {
    let plans = plan_groups(&groups).await;
    let flat: Vec<Planned> = plans.iter().flatten().cloned().collect();
    let fires: Vec<usize> = flat
        .iter()
        .enumerate()
        .filter(|(_, plan)| plan.is_fire())
        .map(|(index, _)| index)
        .collect();
    prop_assume!(!fires.is_empty());
    let removed = trigger(&flat[fires[pick % fires.len()]]);
    prop_assume!(removed.is_some());
    let removed = removed.unwrap_or_else(|| IOEvent::Consume(Consume::default()));
    let target = flat
        .iter()
        .position(|plan| trigger(plan).as_ref() == Some(&removed))
        .unwrap_or_default();

    let (play, complete) = play_and_replay().await;
    let start = checkpoint(&play).await;
    assert_committed(run_plans(&SpaceStoreHandle::new(play.clone()), &plans).await)?;
    let played = checkpoint(&play).await;
    let forged = without_event(&played.log, &removed);
    prop_assert!(forged.len() < played.log.len());

    rig(&complete, &start.root, &played.log).await;
    let complete_handle = SpaceStoreHandle::new(complete.clone());
    let mut complete_txn = complete_handle.begin(&universe()).await.expect("begin");
    for plan in &flat[..target] {
        staged(plan, &mut complete_txn)?;
    }
    let comm = match flat[target]
        .stage(&mut complete_txn, None)
        .map(|staged| staged.comm)
    {
        Ok(Some(comm)) => comm,
        other => {
            return Err(TestCaseError::fail(format!("the complete log stages: {:?}", other)));
        }
    };
    if let Err(error) = complete_txn.commit().await {
        return Err(TestCaseError::fail(format!("the complete log pairs: {:?}", error)));
    }

    let reference = replay_over(&play);
    let reference_recorder = Recorder::new();
    observed(&reference, &reference_recorder);
    rig(&reference, &start.root, &forged).await;
    let reference_handle = SpaceStoreHandle::new(reference.clone());
    let mut reference_txn = reference_handle.begin(&universe()).await.expect("begin");
    for plan in &flat[..target] {
        staged(plan, &mut reference_txn)?;
    }
    if let Err(error) = reference_txn.commit().await {
        return Err(TestCaseError::fail(format!("reference prefix commit: {:?}", error)));
    }

    let forged_replay = replay_over(&play);
    let recorder = Recorder::new();
    observed(&forged_replay, &recorder);
    rig(&forged_replay, &start.root, &forged).await;
    let handle = SpaceStoreHandle::new(forged_replay.clone());
    let mut txn = handle.begin(&universe()).await.expect("begin");
    let bindings = replay_bindings(&forged_replay);
    let state = replay_state(&forged_replay);
    for plan in &flat {
        staged(plan, &mut txn)?;
    }
    prop_assert_eq!(replay_bindings(&forged_replay), bindings);
    prop_assert_eq!(replay_state(&forged_replay), state);
    prop_assert!(recorder.calls().is_empty());
    prop_assert_eq!(
        txn.commit().await,
        Err(SpaceTxnError::ReplayDivergence(Box::new(comm.clone())))
    );
    prop_assert_eq!(replay_state(&forged_replay), replay_state(&reference));
    prop_assert_eq!(replay_bindings(&forged_replay), replay_bindings(&reference));
    let prefix = reference_recorder.calls();
    let calls = recorder.calls();
    prop_assert!(calls.starts_with(&prefix));
    check_unpaired_calls(&calls[prefix.len()..], &source_of(&flat[target]), &comm)?;
    let produces = plan_produces(&flat[target]);
    let mut probe = handle.begin(&universe()).await.expect("begin");
    let mut reference_probe = reference_handle.begin(&universe()).await.expect("begin");
    prop_assert_eq!(
        counters_of(&mut probe, &produces)?,
        counters_of(&mut reference_probe, &produces)?
    );
    Ok(())
}

// H23, T6 (decision 36): a run that an observer call rejects replays to the
// outcome of play. Play rejects its n-th fallible call, as a deploy that
// runs out of phlo there, and keeps its log. Replay, rigged with that log,
// rejects its own n-th call: it fails with the error of play at the same
// operation, makes the calls of play plus its directive queries, reaches
// the state of play, and uses every recorded event. n ranges over every
// fallible call of the full run and one position past them.
async fn replay_reproduces_play_rejection(
    groups: Vec<Vec<Op>>,
    pick: usize,
) -> Result<(), TestCaseError> {
    let plans = plan_groups(&groups).await;
    let counted = play_space().await;
    let counter = Recorder::new();
    observed(&counted, &counter);
    assert_committed(run_plans(&SpaceStoreHandle::new(counted.clone()), &plans).await)?;
    let calls = counter.fallible_calls();
    let n = pick % (calls + 1);

    let (play, replay) = play_and_replay().await;
    let start = checkpoint(&play).await;
    let play_recorder = Recorder::rejecting_call(n);
    observed(&play, &play_recorder);
    let played = run_until_failure(&SpaceStoreHandle::new(play.clone()), &plans).await;
    let log = play.take_event_log().await;
    prop_assert_eq!(played.iter().any(Result::is_err), n < calls);

    let replay_recorder = Recorder::rejecting_call(n);
    observed(&replay, &replay_recorder);
    rig(&replay, &start.root, &log).await;
    let replayed = run_until_failure(&SpaceStoreHandle::new(replay.clone()), &plans).await;
    prop_assert_eq!(replayed, played);
    prop_assert_eq!(without_directives(replay_recorder.calls()), play_recorder.calls());
    prop_assert_eq!(replay_state(&replay), logical_state(&play));
    prop_assert!(replay_bindings(&replay).is_empty());
    Ok(())
}

// A staging probe on a group that is not canonical: a permutation, or a
// repeat out of sorted order.
#[derive(Clone, Debug)]
enum Probe {
    Read(Vec<String>),
    Park(Vec<String>),
    ConsumeFire(Vec<String>),
    ProduceFire(Vec<String>),
}

fn odd_group() -> impl Strategy<Value = Vec<String>> { group(names(&CHANNELS), 1.0, 0.0) }

fn probe() -> impl Strategy<Value = Probe> {
    prop_oneof![
        odd_group().prop_map(Probe::Read),
        odd_group().prop_map(Probe::Park),
        odd_group().prop_map(Probe::ConsumeFire),
        odd_group().prop_map(Probe::ProduceFire),
    ]
}

fn run_probe(txn: &mut Txn, probe: &Probe) -> (Result<(), SpaceTxnError>, Vec<String>) {
    let consume = |channels: &Vec<String>, outcome| SpaceConsume {
        channels: channels.clone(),
        patterns: vec![Pat::Any; channels.len()],
        continuation: s("probe"),
        persist: false,
        peeks: BTreeSet::new(),
        outcome,
        before_observe: None,
    };
    match probe {
        Probe::Read(group) => (txn.continuations(group).map(|_| ()), group.clone()),
        Probe::Park(group) => (
            txn.consume(consume(group, ConsumeOutcome::Park))
                .map(|_| ()),
            group.clone(),
        ),
        Probe::ConsumeFire(group) => (
            txn.consume(consume(group, ConsumeOutcome::Fire { data: Vec::new() }))
                .map(|_| ()),
            group.clone(),
        ),
        Probe::ProduceFire(group) => {
            let continuation = WaitingContinuation::create(
                group,
                &vec![Pat::Any; group.len()],
                &s("probe"),
                false,
                BTreeSet::new(),
            );
            let fire = SpaceProduce {
                channel: group[0].clone(),
                data: s("x"),
                persist: false,
                outcome: ProduceOutcome::Fire {
                    channels: group.clone(),
                    continuation,
                    data: Vec::new(),
                },
                before_observe: None,
            };
            (txn.produce(fire).map(|_| ()), group.clone())
        }
    }
}

// History leaves that the hot store's continuation keys address; two keys
// for one leaf would make the checkpoint refuse duplicate-key actions.
fn continuation_leaves(space: &Space) -> (usize, BTreeSet<Blake2b256Hash>) {
    let keys = continuation_keys(&**space.get_store());
    let leaves = keys.iter().map(hash_from_vec).collect();
    (keys.len(), leaves)
}

// H17, H22, H9: a group that is not canonical (not sorted by channel hash)
// fails closed before any store or cache effect: the hot store's keys are
// unchanged, so no two keys address one history leaf and the checkpoint
// accepts the store. After the
// transaction aborts or commits nothing, the root equals the reference. A
// non-canonical join that the host stored fails the sweep with no change.
async fn non_canonical_inputs_fail_closed(
    setup: Vec<Vec<Op>>,
    probes: Vec<Probe>,
    abort: bool,
    host_join: bool,
) -> Result<(), TestCaseError> {
    let plans = plan_groups(&setup).await;
    let space = play_space().await;
    let reference = play_space().await;
    for target in [&space, &reference] {
        assert_committed(run_plans(&SpaceStoreHandle::new(target.clone()), &plans).await)?;
    }
    let handle = SpaceStoreHandle::new(space.clone());
    let keys = continuation_keys(&**space.get_store());
    let state = logical_state(&space);
    let mut txn = handle.begin(&universe()).await.expect("begin");
    for probe in &probes {
        let (result, group) = run_probe(&mut txn, probe);
        prop_assert_eq!(result.err(), group_rejection(&group));
    }
    prop_assert_eq!(continuation_keys(&**space.get_store()), keys);
    prop_assert_eq!(logical_state(&space), state);
    let (key_count, leaves) = continuation_leaves(&space);
    prop_assert_eq!(key_count, leaves.len());
    match abort {
        true => drop(txn),
        false => match txn.commit().await {
            Ok(receipt) => prop_assert!(receipt.ops.is_empty()),
            Err(error) => return Err(TestCaseError::fail(format!("commit: {:?}", error))),
        },
    }
    prop_assert_eq!(checkpoint(&space).await.root, checkpoint(&reference).await.root);

    if host_join {
        let host_order = names(&["v2", "v1"]);
        for target in [&space, &reference] {
            target
                .consume(
                    host_order.clone(),
                    vec![Pat::Any, Pat::Any],
                    s("kv"),
                    false,
                    BTreeSet::new(),
                )
                .await
                .expect("the host parks on a non-canonical group");
        }
        let before = logical_state(&space);
        prop_assert_eq!(
            handle.sweep_volatile(&names(&["v1", "v2"])).await,
            Err(SpaceTxnError::NonCanonicalChannelOrder)
        );
        prop_assert_eq!(logical_state(&space), before);
        prop_assert_eq!(checkpoint(&space).await.root, checkpoint(&reference).await.root);
    }
    Ok(())
}

// A canonical read: data, a canonical group's continuations (the group may
// repeat a channel), or joins.
#[derive(Clone, Debug)]
enum Read {
    Data(String),
    Continuations(Vec<String>),
    Joins(String),
}

fn read() -> impl Strategy<Value = Read> {
    prop_oneof![
        select(universe()).prop_map(Read::Data),
        group(universe(), 0.0, 0.3).prop_map(Read::Continuations),
        select(universe()).prop_map(Read::Joins),
    ]
}

// H18: reads of canonical keys never change the checkpoint root, whether
// the key is in the hot store, only in history, or absent, and whether the
// transaction then aborts or commits nothing.
async fn reads_root_neutral(
    setup: Vec<Vec<Op>>,
    reads: Vec<Read>,
    persisted: bool,
    commit: bool,
) -> Result<(), TestCaseError> {
    let plans = plan_groups(&setup).await;
    let space = play_space().await;
    let reference = play_space().await;
    for target in [&space, &reference] {
        assert_committed(run_plans(&SpaceStoreHandle::new(target.clone()), &plans).await)?;
        if persisted {
            checkpoint(target).await;
        }
    }
    let handle = SpaceStoreHandle::new(space.clone());
    let mut txn = handle.begin(&universe()).await.expect("begin");
    for read in &reads {
        let result = match read {
            Read::Data(channel) => txn.data(channel).map(|_| ()),
            Read::Continuations(group) => txn.continuations(group).map(|_| ()),
            Read::Joins(channel) => txn.joins(channel).map(|_| ()),
        };
        prop_assert_eq!(result, Ok(()));
    }
    match commit {
        true => match txn.commit().await {
            Ok(receipt) => prop_assert!(receipt.ops.is_empty()),
            Err(error) => return Err(TestCaseError::fail(format!("commit: {:?}", error))),
        },
        false => drop(txn),
    }
    let (key_count, leaves) = continuation_leaves(&space);
    prop_assert_eq!(key_count, leaves.len());
    let (read_checkpoint, reference_checkpoint) =
        (checkpoint(&space).await, checkpoint(&reference).await);
    prop_assert_eq!(event_bytes(&read_checkpoint.log), event_bytes(&reference_checkpoint.log));
    prop_assert_eq!(read_checkpoint.root, reference_checkpoint.root);
    Ok(())
}

fn source_of(plan: &Planned) -> Vec<u8> {
    let event = match plan {
        Planned::Produce {
            channel,
            data,
            persist,
            ..
        } => IOEvent::Produce(Produce::create(channel, data, *persist)),
        Planned::Consume {
            channels,
            patterns,
            continuation,
            persist,
            ..
        } => IOEvent::Consume(Consume::create(channels, patterns, continuation, *persist)),
        Planned::Rejected { plan, .. } => return source_of(plan),
    };
    bincode::serialize(&event).expect("serialize the operation source")
}

// H19 (decision 19 D4): a native replay directive rejects the operation it
// is returned for with no effect of that operation: no event, no store,
// binding or counter change, and only the start, directive and rejected
// finish calls. The operations before it stay applied, as in the host
// program.
async fn replay_directive_fails_closed(
    groups: Vec<Vec<Op>>,
    pick: usize,
) -> Result<(), TestCaseError> {
    let plans = plan_groups(&groups).await;
    let applied: Vec<(usize, usize)> = plans
        .iter()
        .enumerate()
        .flat_map(|(group, plans)| {
            plans
                .iter()
                .enumerate()
                .filter(|(_, plan)| !plan.is_rejected())
                .map(move |(position, _)| (group, position))
        })
        .collect();
    prop_assume!(!applied.is_empty());
    let index = pick % applied.len();
    let (failing_group, failing_position) = applied[index];
    let failing = &plans[failing_group][failing_position];

    let play = play_space().await;
    let start = checkpoint(&play).await;
    assert_committed(run_plans(&SpaceStoreHandle::new(play.clone()), &plans).await)?;
    let played = checkpoint(&play).await;

    let directed = replay_over(&play);
    let directed_recorder = Recorder::with_directive_at(index, ReplayOperationDirective::Store);
    observed(&directed, &directed_recorder);
    rig(&directed, &start.root, &played.log).await;
    let directed_handle = SpaceStoreHandle::new(directed.clone());
    assert_committed(run_plans(&directed_handle, &plans[..failing_group]).await)?;
    let mut txn = directed_handle.begin(&universe()).await.expect("begin");
    for plan in &plans[failing_group] {
        staged(plan, &mut txn)?;
    }
    prop_assert_eq!(
        txn.commit().await.err(),
        Some(SpaceTxnError::NativeReplayDirectiveUnsupported)
    );

    let reference = replay_over(&play);
    let reference_recorder = Recorder::new();
    observed(&reference, &reference_recorder);
    rig(&reference, &start.root, &played.log).await;
    let reference_handle = SpaceStoreHandle::new(reference.clone());
    assert_committed(run_plans(&reference_handle, &plans[..failing_group]).await)?;
    let mut txn = reference_handle.begin(&universe()).await.expect("begin");
    for plan in &plans[failing_group][..failing_position] {
        staged(plan, &mut txn)?;
    }
    if let Err(error) = txn.commit().await {
        return Err(TestCaseError::fail(format!("reference prefix commit: {:?}", error)));
    }

    prop_assert_eq!(replay_state(&directed), replay_state(&reference));
    prop_assert_eq!(replay_bindings(&directed), replay_bindings(&reference));
    let source = source_of(failing);
    let mut expected = reference_recorder.calls();
    expected.push(Call::Start {
        source: source.clone(),
        channels: match failing {
            Planned::Produce { channel, .. } => vec![channel.clone()],
            Planned::Consume { channels, .. } => channels.clone(),
            Planned::Rejected { .. } => Vec::new(),
        },
        joins: match failing {
            Planned::Produce { channel, .. } => reference.get_joins(channel.clone()).await,
            _ => Vec::new(),
        },
    });
    expected.push(Call::Directive {
        source: source.clone(),
    });
    expected.push(Call::Finish {
        source,
        completion: RSpaceOperationCompletion::Rejected,
    });
    prop_assert_eq!(directed_recorder.calls(), expected);
    let produces = plan_produces(failing);
    let mut directed_probe = directed_handle.begin(&universe()).await.expect("begin");
    let mut reference_probe = reference_handle.begin(&universe()).await.expect("begin");
    prop_assert_eq!(
        counters_of(&mut directed_probe, &produces)?,
        counters_of(&mut reference_probe, &produces)?
    );
    Ok(())
}

// One commit attempt: an optional parent (a session step and a path) and
// the stores it stages.
#[derive(Clone, Debug)]
struct Attempt {
    parent: Option<(u8, u64)>,
    stores: Vec<(usize, usize)>,
}

fn attempts() -> impl Strategy<Value = Vec<Attempt>> {
    let attempt = (
        proptest::option::weighted(0.8, (0u8..2, 0u64..3)),
        proptest::collection::vec((0usize..3, 0usize..2), 1..=3),
    )
        .prop_map(|(parent, stores)| Attempt { parent, stores });
    proptest::collection::vec(attempt, 1..=8)
}

fn parent_order(session: u8, path: u64) -> OperationOrder {
    OperationOrder {
        session: [session; 32],
        path: CausalPath::from(vec![(path, 0)]),
    }
}

// H20: under one parent order at most one transaction commits; a second
// commit fails with `ParentOrderReused` and has no effect. The tracking
// resets with a new session, and commits without a parent are not tracked.
// So sub-orders are unique within a session, and the ordered log keeps each
// transaction's events together in commit order.
async fn parent_order_single_commit(attempts: Vec<Attempt>) -> Result<(), TestCaseError> {
    let space = play_space().await;
    let recorder = Recorder::new();
    observed(&space, &recorder);
    let handle = SpaceStoreHandle::new(space.clone());
    let mut session = 0u8;
    let mut model_session: Option<u8> = None;
    let mut used: BTreeSet<u64> = BTreeSet::new();
    let mut untracked: Vec<Event> = Vec::new();
    let mut tracked: BTreeMap<(u8, u64), Vec<Event>> = BTreeMap::new();
    for attempt in &attempts {
        let parent = attempt.parent.map(|(step, path)| {
            session += step;
            (session, path)
        });
        let stores: Vec<(String, String)> = attempt
            .stores
            .iter()
            .map(|(channel, value)| (s(CHANNELS[*channel]), s(VALUES[*value])))
            .collect();
        let calls = recorder.calls().len();
        let state = logical_state(&space);
        let run = async {
            let mut txn = handle.begin(&universe()).await.expect("begin");
            for (channel, value) in &stores {
                txn.produce(SpaceProduce {
                    channel: channel.clone(),
                    data: value.clone(),
                    persist: false,
                    outcome: ProduceOutcome::Store,
                    before_observe: None,
                })
                .expect("stage a store");
            }
            txn.commit().await
        };
        let result = match parent {
            Some((session, path)) => {
                operation_context::scope(parent_order(session, path), run).await
            }
            None => run.await,
        };
        let events: Vec<Event> = stores
            .iter()
            .map(|(channel, value)| {
                Event::IoEvent(IOEvent::Produce(Produce::create(channel, value, false)))
            })
            .collect();
        let admitted = match parent {
            None => true,
            Some((session, path)) => {
                if model_session != Some(session) {
                    model_session = Some(session);
                    used.clear();
                }
                used.insert(path)
            }
        };
        match (admitted, result) {
            (true, Ok(receipt)) => {
                prop_assert_eq!(receipt.ops.len(), stores.len());
                match parent {
                    Some(key) => tracked.entry(key).or_default().extend(events),
                    None => untracked.extend(events),
                }
            }
            (false, Err(SpaceTxnError::ParentOrderReused)) => {
                prop_assert_eq!(recorder.calls().len(), calls);
                prop_assert_eq!(logical_state(&space), state);
            }
            (admitted, result) => {
                return Err(TestCaseError::fail(format!(
                    "admitted {} but the commit gave {:?}",
                    admitted,
                    result.map(|receipt| receipt.ops.len())
                )));
            }
        }
    }
    let orders: Vec<OperationOrder> = recorder.orders().into_iter().flatten().collect();
    let distinct: BTreeSet<&OperationOrder> = orders.iter().collect();
    prop_assert_eq!(distinct.len(), orders.len());
    let mut expected = untracked;
    for events in tracked.into_values() {
        expected.extend(events);
    }
    prop_assert_eq!(event_bytes(&space.take_event_log().await), event_bytes(&expected));
    Ok(())
}

/// A channel with a field that `Ord` compares but `Eq`, `Hash` and serde
/// ignore, as `Par.locally_free` is.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct Tagged {
    name: String,
    #[serde(skip)]
    note: u8,
}

impl PartialEq for Tagged {
    fn eq(&self, other: &Self) -> bool { self.name == other.name }
}

impl Eq for Tagged {}

impl Hash for Tagged {
    fn hash<H: Hasher>(&self, state: &mut H) { self.name.hash(state); }
}

impl PartialOrd for Tagged {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> { Some(self.cmp(other)) }
}

impl Ord for Tagged {
    fn cmp(&self, other: &Self) -> Ordering {
        (&self.name, self.note).cmp(&(&other.name, other.note))
    }
}

type TaggedSpace = RSpace<Tagged, Pat, String, String>;
type TaggedHandle = SpaceStoreHandle<Tagged, Pat, String, String>;
type TaggedTxn = BoxedSpaceTxn<Tagged, Pat, String, String>;

const TAGGED_NAMES: [&str; 3] = ["a", "b", "c"];
const NOTES: u8 = 4;

fn tagged(name: &str, note: u8) -> Tagged {
    Tagged {
        name: s(name),
        note,
    }
}

// One operation on tagged channels; each channel reference carries a note.
#[derive(Clone, Debug)]
enum TaggedOp {
    Store {
        name: usize,
        note: u8,
        value: usize,
    },
    Park {
        name: usize,
        note: u8,
    },
    ParkJoin {
        first: usize,
        second: usize,
        notes: (u8, u8),
    },
    Fire {
        name: usize,
        note: u8,
        bind_note: u8,
    },
}

fn tagged_op() -> impl Strategy<Value = TaggedOp> {
    prop_oneof![
        (0usize..3, 0..NOTES, 0usize..2).prop_map(|(name, note, value)| TaggedOp::Store {
            name,
            note,
            value
        }),
        (0usize..3, 0..NOTES).prop_map(|(name, note)| TaggedOp::Park { name, note }),
        (0usize..3, 1usize..3, (0..NOTES, 0..NOTES)).prop_map(|(first, offset, notes)| {
            TaggedOp::ParkJoin {
                first,
                second: (first + offset) % 3,
                notes,
            }
        }),
        (0usize..3, 0..NOTES, 0..NOTES).prop_map(|(name, note, bind_note)| TaggedOp::Fire {
            name,
            note,
            bind_note
        }),
    ]
}

async fn tagged_space() -> TaggedSpace {
    let mut stores = InMemoryStoreManager::new();
    let store = stores
        .r_space_stores()
        .await
        .expect("create the in-memory RSpace stores");
    RSpace::create(store, Arc::new(Box::new(PatMatch))).expect("create the RSpace")
}

fn tagged_lock_keys() -> Vec<Tagged> {
    let mut keys: Vec<Tagged> = TAGGED_NAMES.iter().map(|name| tagged(name, 0)).collect();
    keys.push(tagged("sys", 0));
    keys
}

// Stages `op` with the notes `note_of` gives; `None` when the operation is
// skipped (a fire on an empty channel). Returns the staged identity.
fn stage_tagged(
    txn: &mut TaggedTxn,
    op: &TaggedOp,
    note_of: &dyn Fn(u8) -> u8,
) -> Result<Option<Vec<u8>>, SpaceTxnError> {
    let never = || Pat::Exact(s("never"));
    let staged = match op {
        TaggedOp::Store { name, note, value } => txn.produce(SpaceProduce {
            channel: tagged(TAGGED_NAMES[*name], note_of(*note)),
            data: s(VALUES[*value]),
            persist: false,
            outcome: ProduceOutcome::Store,
            before_observe: None,
        })?,
        TaggedOp::Park { name, note } => txn.consume(SpaceConsume {
            channels: vec![tagged(TAGGED_NAMES[*name], note_of(*note))],
            patterns: vec![never()],
            continuation: format!("k{}", name),
            persist: false,
            peeks: BTreeSet::new(),
            outcome: ConsumeOutcome::Park,
            before_observe: None,
        })?,
        TaggedOp::ParkJoin {
            first,
            second,
            notes,
        } => {
            let mut channels = vec![
                tagged(TAGGED_NAMES[*first], note_of(notes.0)),
                tagged(TAGGED_NAMES[*second], note_of(notes.1)),
            ];
            channels.sort_by_key(hash);
            txn.consume(SpaceConsume {
                channels,
                patterns: vec![never(), never()],
                continuation: format!("j{}{}", first.min(second), first.max(second)),
                persist: false,
                peeks: BTreeSet::new(),
                outcome: ConsumeOutcome::Park,
                before_observe: None,
            })?
        }
        TaggedOp::Fire {
            name,
            note,
            bind_note,
        } => {
            let records = txn.data(&tagged(TAGGED_NAMES[*name], note_of(*bind_note)))?;
            let Some(record) = records.first().cloned() else {
                return Ok(None);
            };
            let matched = record.a.clone();
            txn.consume(SpaceConsume {
                channels: vec![tagged(TAGGED_NAMES[*name], note_of(*note))],
                patterns: vec![Pat::Any],
                continuation: s("fire"),
                persist: false,
                peeks: BTreeSet::new(),
                outcome: ConsumeOutcome::Fire {
                    data: vec![bind_candidate(
                        tagged(TAGGED_NAMES[*name], note_of(*bind_note)),
                        &record,
                        matched,
                        0,
                    )],
                },
                before_observe: None,
            })?
        }
    };
    Ok(Some(
        bincode::serialize(&(format!("{:?}", staged.source), &staged.comm))
            .expect("serialize the staged identity"),
    ))
}

// The view of every name through the note `note`.
fn tagged_view(txn: &mut TaggedTxn, note: u8) -> Result<Vec<Vec<u8>>, SpaceTxnError> {
    let mut view = Vec::with_capacity(TAGGED_NAMES.len());
    for name in TAGGED_NAMES {
        let channel = tagged(name, note);
        let data = txn.data(&channel)?.to_vec();
        let continuations = txn.continuations(std::slice::from_ref(&channel))?.to_vec();
        let joins = txn.joins(&channel)?.to_vec();
        view.push(bincode::serialize(&(data, continuations, joins)).expect("serialize the view"));
    }
    Ok(view)
}

async fn tagged_store_view(space: &TaggedSpace, note: u8) -> Vec<Vec<u8>> {
    let mut view = Vec::with_capacity(TAGGED_NAMES.len());
    for name in TAGGED_NAMES {
        let channel = tagged(name, note);
        let data = space.get_data(&channel).await;
        let continuations = space.get_waiting_continuations(vec![channel.clone()]).await;
        let joins = space.get_joins(channel).await;
        view.push(bincode::serialize(&(data, continuations, joins)).expect("serialize the view"));
    }
    view
}

// H21: store identity is `stable_hash_provider::hash`, so channels that are
// equal but differ in an `Ord`-only field are one channel for every
// substrate decision. A run that names channels with arbitrary notes stages,
// reads, commits and checkpoints exactly like the run with every note zero;
// the staged view through any note equals the committed store through any
// note; and the installed guard holds through any note.
async fn store_identity_ignores_ord_only_fields(
    groups: Vec<Vec<TaggedOp>>,
) -> Result<(), TestCaseError> {
    let noted = tagged_space().await;
    let plain = tagged_space().await;
    for (space, note) in [(&noted, 1u8), (&plain, 0u8)] {
        space
            .install(vec![tagged("sys", note)], vec![Pat::Any], s("system"))
            .await
            .expect("install a system continuation");
    }
    let noted_handle: TaggedHandle = SpaceStoreHandle::new(noted.clone());
    let plain_handle: TaggedHandle = SpaceStoreHandle::new(plain.clone());
    let keys = tagged_lock_keys();
    let keep = |note: u8| note;
    let zero = |_: u8| 0u8;
    for group in &groups {
        let mut noted_txn = noted_handle.begin(&keys).await.expect("begin");
        let mut plain_txn = plain_handle.begin(&keys).await.expect("begin");
        for op in group {
            let noted_result = stage_tagged(&mut noted_txn, op, &keep);
            let plain_result = stage_tagged(&mut plain_txn, op, &zero);
            prop_assert_eq!(noted_result, plain_result);
            let plain_view = tagged_view(&mut plain_txn, 0);
            for note in 0..NOTES {
                prop_assert_eq!(&tagged_view(&mut noted_txn, note), &plain_view);
            }
            prop_assert_eq!(
                noted_txn.data(&tagged("sys", 3)).err(),
                Some(SpaceTxnError::InstalledChannel)
            );
        }
        let mut staged_views = Vec::with_capacity(usize::from(NOTES));
        for note in 0..NOTES {
            staged_views.push(tagged_view(&mut noted_txn, note));
        }
        let committed = noted_txn.commit().await.map(|receipt| receipt.ops.len());
        prop_assert_eq!(committed, plain_txn.commit().await.map(|receipt| receipt.ops.len()));
        let plain_store = tagged_store_view(&plain, 0).await;
        for note in 0..NOTES {
            let noted_store = tagged_store_view(&noted, note).await;
            prop_assert_eq!(&noted_store, &plain_store);
            for staged_view in &staged_views {
                prop_assert_eq!(staged_view, &Ok(noted_store.clone()));
            }
        }
    }
    let noted_checkpoint = noted.create_checkpoint().await;
    let plain_checkpoint = plain.create_checkpoint().await;
    match (noted_checkpoint, plain_checkpoint) {
        (Ok(noted_checkpoint), Ok(plain_checkpoint)) => {
            prop_assert_eq!(event_bytes(&noted_checkpoint.log), event_bytes(&plain_checkpoint.log));
            prop_assert_eq!(noted_checkpoint.root, plain_checkpoint.root);
        }
        (noted, plain) => {
            return Err(TestCaseError::fail(format!(
                "checkpoints: {:?} / {:?}",
                noted.map(|checkpoint| checkpoint.root),
                plain.map(|checkpoint| checkpoint.root)
            )));
        }
    }
    Ok(())
}

// A lock-coverage case: the covered channels `U` (the transaction's lock
// keys) and the rest `V`; setup operations within `U` or within `V`; the
// transaction's operations within `U`; probes that touch `V`; and host
// operations over the whole universe, run while the transaction is open.
#[derive(Clone, Debug)]
struct CoverageCase {
    covered: Vec<String>,
    setup: Vec<Vec<Op>>,
    plan: Vec<Op>,
    probes: Vec<Op>,
    host: Vec<Op>,
}

fn produce_on(channels: Vec<String>) -> impl Strategy<Value = Op> {
    (select(channels), select(VALUES.to_vec()), proptest::bool::weighted(0.2))
        .prop_map(|(channel, value, persist)| Op::produce(&channel, value, persist))
}

fn op_on(channels: Vec<String>) -> BoxedStrategy<Op> {
    prop_oneof![produce_on(channels.clone()), consume_on(channels, &VALUES, false, 0.0, 0.0)]
        .boxed()
}

// An operation that touches an uncovered channel: a store on it, or a park
// on a canonical group that contains it.
fn probe_on(covered: Vec<String>, uncovered: Vec<String>) -> BoxedStrategy<Op> {
    let store = produce_on(uncovered.clone());
    let park =
        (select(uncovered), proptest::option::of(select(covered))).prop_map(|(outside, inside)| {
            let mut group = vec![outside];
            group.extend(inside);
            group.sort_by_key(hash);
            let channels: Vec<&str> = group.iter().map(String::as_str).collect();
            Op::consume(&channels, vec![Pat::Exact(s("never")); channels.len()], "probe", false)
        });
    prop_oneof![store, park].boxed()
}

fn coverage_case() -> impl Strategy<Value = CoverageCase> {
    subsequence(universe(), 1..UNIVERSE.len())
        .prop_flat_map(|covered| {
            let uncovered: Vec<String> = universe()
                .into_iter()
                .filter(|channel| !covered.contains(channel))
                .collect();
            let setup_op = prop_oneof![op_on(covered.clone()), op_on(uncovered.clone())];
            (
                Just(covered.clone()),
                proptest::collection::vec(proptest::collection::vec(setup_op, 1..=3), 0..=3),
                proptest::collection::vec(op_on(covered.clone()), 1..=4),
                proptest::collection::vec(probe_on(covered, uncovered), 0..=3),
                proptest::collection::vec(op_on(universe()), 0..=4),
            )
        })
        .prop_map(|(covered, setup, plan, probes, host)| CoverageCase {
            covered,
            setup,
            plan,
            probes,
            host,
        })
}

type HostFuture<'a> = Pin<Box<dyn Future<Output = Result<HostOutcome, RSpaceError>> + 'a>>;

// T5 (lock coverage): an operation that touches a channel whose stripe the
// transaction does not hold fails with `UnlockedChannel` and no effect. Host
// operations that run while the transaction is open either complete at once
// (they touch only uncovered channels) or wait for the commit; the final
// state, event log and root equal the serial order: the operations that
// completed at once, the transaction, then the waiting operations in their
// completion order.
async fn rejects_uncovered_channels(case: CoverageCase) -> Result<(), TestCaseError> {
    let CoverageCase {
        covered,
        setup,
        plan,
        probes,
        host,
    } = case;
    let setup_ops = flatten(&setup);
    let planner_host = play_space().await;
    let planner_txn = play_space().await;
    let space = play_space().await;
    let reference = play_space().await;
    for target in [&planner_host, &planner_txn, &space, &reference] {
        run_host_ops(target, &setup_ops).await;
        target.take_event_log().await;
    }
    let (plans, receipts) = run_guided(&planner_host, &planner_txn, &[plan]).await;
    assert_committed(receipts)?;

    let handle = SpaceStoreHandle::new(space.clone());
    let mut txn = handle.begin(&covered).await.expect("begin");
    let state = logical_state(&space);
    for probe in &probes {
        prop_assert_eq!(
            probe.park().stage(&mut txn, None).err(),
            Some(SpaceTxnError::UnlockedChannel)
        );
    }
    prop_assert_eq!(logical_state(&space), state);
    for staged_plan in &plans[0] {
        staged(staged_plan, &mut txn)?;
    }
    let mut early: Vec<usize> = Vec::with_capacity(host.len());
    let mut waiting: Vec<(usize, HostFuture<'_>)> = Vec::with_capacity(host.len());
    for (index, op) in host.iter().enumerate() {
        let mut future: HostFuture<'_> = Box::pin(run_host(&space, op));
        match futures::poll!(&mut future) {
            std::task::Poll::Ready(Ok(_)) => early.push(index),
            std::task::Poll::Ready(Err(error)) => {
                return Err(TestCaseError::fail(format!("host operation: {:?}", error)));
            }
            std::task::Poll::Pending => waiting.push((index, future)),
        }
    }
    let covered_set: BTreeSet<&String> = covered.iter().collect();
    for index in &early {
        let touches_covered = op_channels(&host[*index])
            .iter()
            .any(|channel| covered_set.contains(channel));
        prop_assert!(!touches_covered, "a host operation on a covered channel did not wait");
    }
    if let Err(error) = txn.commit().await {
        return Err(TestCaseError::fail(format!("commit: {:?}", error)));
    }
    let completed: RefCell<Vec<usize>> = RefCell::new(Vec::with_capacity(waiting.len()));
    let results = futures::future::join_all(waiting.into_iter().map(|(index, future)| {
        let completed = &completed;
        async move {
            let result = future.await;
            completed.borrow_mut().push(index);
            result
        }
    }))
    .await;
    if let Some(error) = results.into_iter().find_map(Result::err) {
        return Err(TestCaseError::fail(format!("host operation: {:?}", error)));
    }

    for index in &early {
        run_host(&reference, &host[*index])
            .await
            .expect("an early host operation");
    }
    assert_committed(run_plans(&SpaceStoreHandle::new(reference.clone()), &plans).await)?;
    for index in completed.into_inner() {
        run_host(&reference, &host[index])
            .await
            .expect("a waiting host operation");
    }
    prop_assert_eq!(logical_state(&space), logical_state(&reference));
    let (concurrent, serial) = (checkpoint(&space).await, checkpoint(&reference).await);
    prop_assert_eq!(event_bytes(&concurrent.log), event_bytes(&serial.log));
    prop_assert_eq!(concurrent.root, serial.root);
    Ok(())
}

// FR-1': the stored records that a fire binds, by occurrence: exactly one
// incoming candidate in a produce-side fire, none in a consume-side fire,
// and a non-persistent record at most once.
fn binds_each_record_once(plan: &Planned) -> Result<(), TestCaseError> {
    let candidates = match plan {
        Planned::Produce {
            outcome: ProduceOutcome::Fire { data, .. },
            ..
        } => {
            let incoming = data
                .iter()
                .filter(|candidate| candidate.datum_index == -1)
                .count();
            prop_assert_eq!(incoming, 1);
            data
        }
        Planned::Consume {
            outcome: ConsumeOutcome::Fire { data },
            ..
        } => {
            prop_assert!(data.iter().all(|candidate| candidate.datum_index >= 0));
            data
        }
        _ => return Ok(()),
    };
    let mut bound: BTreeSet<(String, i32)> = BTreeSet::new();
    for candidate in candidates {
        if candidate.datum_index >= 0 && !candidate.datum.persist {
            prop_assert!(bound.insert((candidate.channel.clone(), candidate.datum_index)));
        }
    }
    Ok(())
}

// FR-1' with H1, H2 and H5: sorted groups that repeat a channel are
// accepted. Each occurrence binds its own record: a non-persistent record
// binds once per COMM, and a produce-side fire binds the incoming datum
// once. Staged from the host outcomes, such groups give the host's observer
// calls, events, state and root, and replay through transactions pairs every
// COMM and reaches the play root. Host replay is not an oracle here: it can
// fail on such a log (see `host_replay_op_groups`).
async fn repeated_channel_groups_equal_host(groups: Vec<Vec<Op>>) -> Result<(), TestCaseError> {
    let host = play_space().await;
    let host_recorder = Recorder::new();
    observed(&host, &host_recorder);
    let txn = play_space().await;
    let txn_recorder = Recorder::new();
    observed(&txn, &txn_recorder);
    let start = checkpoint(&txn).await;
    let (plans, receipts) = run_guided(&host, &txn, &groups).await;
    let played_receipts = assert_committed(receipts)?;
    for plan in plans.iter().flatten() {
        binds_each_record_once(plan)?;
    }
    prop_assert_eq!(txn_recorder.calls(), host_recorder.calls());
    prop_assert_eq!(logical_state(&txn), logical_state(&host));
    let host_played = checkpoint(&host).await;
    let played = checkpoint(&txn).await;
    prop_assert_eq!(event_bytes(&played.log), event_bytes(&host_played.log));
    prop_assert_eq!(played.root.clone(), host_played.root);

    let replay = replay_over(&txn);
    rig(&replay, &start.root, &played.log).await;
    let replayed =
        assert_committed(run_plans(&SpaceStoreHandle::new(replay.clone()), &plans).await)?;
    prop_assert_eq!(replayed, played_receipts);
    prop_assert!(replay_bindings(&replay).is_empty());
    match replay.create_checkpoint().await {
        Ok(replayed) => prop_assert_eq!(replayed.root, played.root),
        Err(error) => return Err(TestCaseError::fail(format!("replay checkpoint: {:?}", error))),
    }
    Ok(())
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn prop_space_txn_play_trace_equals_host(groups in op_groups(false, ODD)) {
        runtime().block_on(play_trace_equals_host(groups))?;
    }

    #[test]
    fn prop_space_txn_play_root_equals_host(groups in op_groups(false, ODD)) {
        runtime().block_on(play_root_equals_host(groups))?;
    }

    #[test]
    fn prop_space_txn_replay_pairs_play_log(groups in op_groups(false, ODD)) {
        runtime().block_on(replay_pairs_play_log(groups))?;
    }

    #[test]
    fn prop_space_txn_cross_replay_with_host(groups in host_replay_op_groups()) {
        runtime().block_on(cross_replay_with_host(groups))?;
    }

    #[test]
    fn prop_space_txn_replay_divergence_typed_at_commit(groups in op_groups(false, ODD)) {
        runtime().block_on(replay_divergence_typed_at_commit(groups))?;
    }

    #[test]
    fn prop_space_txn_view_equals_committed_store(groups in op_groups(true, ODD)) {
        runtime().block_on(view_equals_committed_store(groups))?;
    }

    #[test]
    fn prop_space_txn_joins_lockstep(groups in op_groups(false, ODD)) {
        runtime().block_on(joins_lockstep(groups))?;
    }

    #[test]
    fn prop_sweep_volatile_exact_and_root_neutral(groups in volatile_op_groups()) {
        runtime().block_on(sweep_exact_and_root_neutral(groups))?;
    }

    #[test]
    fn prop_sub_orders_contiguous_and_disjoint(
        participant in proptest::collection::vec((0u64..64, 0u64..2), 0..6),
        step in 0u64..1_000,
        count in 1usize..8,
        reductions in proptest::collection::vec(
            proptest::collection::vec((0u64..64, 0u64..2), 0..8),
            0..16,
        ),
    ) {
        runtime().block_on(sub_orders_contiguous(participant, step, count, reductions))?;
    }

    #[test]
    fn prop_space_txn_abort_inert((setup, aborted) in abort_case()) {
        runtime().block_on(abort_inert(setup, aborted))?;
    }

    #[test]
    fn prop_event_coverage_holds_for_generated_commits(groups in op_groups(false, ODD)) {
        runtime().block_on(coverage_holds(groups))?;
    }

    #[test]
    fn prop_space_txn_peek_retention(groups in peek_op_groups()) {
        runtime().block_on(peek_retention(groups))?;
    }

    #[test]
    fn prop_space_txn_reporting_equals_replay(groups in host_replay_op_groups()) {
        runtime().block_on(reporting_equals_replay(groups))?;
    }

    #[test]
    fn prop_root_leaf_record_count_matches_history(groups in op_groups(false, ODD)) {
        runtime().block_on(root_leaf_count(groups))?;
    }

    #[test]
    fn prop_counter_timing_play_equals_replay(groups in repeating_op_groups()) {
        runtime().block_on(counter_timing(groups))?;
    }

    #[test]
    fn prop_space_txn_replay_requires_trigger_event(
        groups in op_groups(false, ODD),
        pick in any::<usize>(),
    ) {
        runtime().block_on(replay_requires_trigger_event(groups, pick))?;
    }

    #[test]
    fn prop_space_txn_replay_reproduces_play_rejection(
        groups in op_groups(false, ODD),
        pick in any::<usize>(),
    ) {
        runtime().block_on(replay_reproduces_play_rejection(groups, pick))?;
    }

    #[test]
    fn prop_space_txn_non_canonical_inputs_fail_closed(
        setup in op_groups(false, ODD),
        probes in proptest::collection::vec(probe(), 1..=6),
        abort in any::<bool>(),
        host_join in any::<bool>(),
    ) {
        runtime().block_on(non_canonical_inputs_fail_closed(setup, probes, abort, host_join))?;
    }

    #[test]
    fn prop_space_txn_reads_root_neutral(
        setup in op_groups(false, ODD),
        reads in proptest::collection::vec(read(), 1..=8),
        persisted in any::<bool>(),
        commit in any::<bool>(),
    ) {
        runtime().block_on(reads_root_neutral(setup, reads, persisted, commit))?;
    }

    #[test]
    fn prop_space_txn_replay_directive_fails_closed(
        groups in op_groups(false, ODD),
        pick in any::<usize>(),
    ) {
        runtime().block_on(replay_directive_fails_closed(groups, pick))?;
    }

    #[test]
    fn prop_space_txn_parent_order_single_commit(attempts in attempts()) {
        runtime().block_on(parent_order_single_commit(attempts))?;
    }

    #[test]
    fn prop_space_txn_store_identity_ignores_ord_only_fields(
        groups in proptest::collection::vec(proptest::collection::vec(tagged_op(), 1..=5), 1..=4),
    ) {
        runtime().block_on(store_identity_ignores_ord_only_fields(groups))?;
    }

    #[test]
    fn prop_space_txn_rejects_uncovered_channels(case in coverage_case()) {
        runtime().block_on(rejects_uncovered_channels(case))?;
    }

    #[test]
    fn prop_space_txn_repeated_channel_groups_equal_host(groups in repeated_channel_op_groups()) {
        runtime().block_on(repeated_channel_groups_equal_host(groups))?;
    }
}
