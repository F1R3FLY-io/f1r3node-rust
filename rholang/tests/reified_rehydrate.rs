//! Rehydration and charging evidence of P7 (design v3, W6: rules RH-1 to RH-4, and the dangling
//! ids of W2: DG-1 to DG-4).
//!
//! Every deploy plays natively at the v6 limits and certifies through its committed evidence,
//! as proposer self-certification and validator replay do (`support::native`). A warm runtime is
//! the runtime that played the earlier blocks. A cold runtime is a restarted chain: a spawned
//! space over the same history with an empty hot store, and a new reducer reset to the root
//! (`NativeChain::restarted`). After each play, the hot store holds no volatile channel (the
//! residue check of VR-2). The process restart of three children is in
//! `reified_restart_rehydrate.rs`.
//!
//! A cost vector is the phlo usage of native play, its budget recording (usage, attempts and
//! journal rows), its authority events and byte observations, and the host work of the play in
//! each of the 16 dimensions.
//!
//! | Test | Property |
//! |---|---|
//! | `space_lazy_attach` | Block 1 creates a queue, a set, a priority queue, a VectorDB space and a PathMap space with records and waiters. A restarted runtime holds no space channel before its first operation, and block 2 (a handle call `config`, a take, a fire of the stored waiter and a store) gives on it the outcome and the cost vector of the warm runtime (RH-1). |
//! | `space_checkpoint_reset` | A reset to the root of block 1 restores the records, the waiters and the counters of block 1. Block 2 then reaches the root and the cost vector of its first play, and a restarted runtime reads the state of block 1 (RH-2). |
//! | `space_attach_cost` | A handle call `config` and a send on an existing queue have the pinned cost vectors on the warm and on a cold runtime, and the accounting observer receives the cost of each COMM: one journal COMM record for each COMM of the log, each linked to a budget observation of the kind `Comm` (RH-4). |
//! | `space_attach_warm_cold_cost_equal` | For each of the 24 families, block 2 over the state of the family program gives the same outcome and cost vector on the warm and on a cold runtime (RH-1, RH-4). |
//! | `prop_attach_warm_cold_cost_equal` | The same equality for generated families, stored records and block-2 operations (NE-2, `view_charge_cache_independent`). |
//! | `reducer_attach_costs` | A missing durable space (DG-2) fails each entry with one cost vector for any missing space id, on a fresh runtime and on a runtime that holds other spaces (DG-4). The handle calls `config` and `gensym` through a stored handle cost the same on the warm and on a cold runtime. |
//! | `reified_handle_persisted_restart_replay` | A handle and a name stored in block 1 serve three handle calls on a restarted runtime. The calls reach `HANDLE_OP` through the stored handle continuation, and native replay from the history of the root of block 1 reproduces the play. |
//! | `reducer_dangling_handle` | Every row of DG-1 and DG-2 fails with its code and names the space id (DG-3): the ids of a `temp` space of an ended deploy, and the ids of a durable space that does not exist. `getSpaceAgent` replies `DanglingHandle` with the space id, and `ann` replies `NotAVectorDbSpace`. Native replay reproduces each deploy. |
//! | `reducer_space_costs` | Native play of parallel space work on two spaces gives the same outcome and cost vector on a current-thread and on a multi-thread tokio runtime, and native replay reproduces each play. |
//! | `reified_factory_ephemeral_staging` | The factory stages the config, the space counter and the handle of a `temp` and of a `seq` space under lowercase KINDs, and of a `default` space under uppercase KINDs. The handle reply carries the KIND of its space, and only a `default` space leaves channels after its deploy. |
//! | `reified_factory_theory_argument` | A theory argument persists in the config. After a restart, the first operation resolves the stored identity: a `Nat` value is admitted, a string fails with `TheoryRefuted`, and the `config` reply shows the identity. A `temp` space stages its theory too. |

#[path = "support/mod.rs"]
mod support;

use std::collections::BTreeSet;

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::Par;
use models::rust::host_work::HostWorkUsages;
use proptest::prelude::{prop, prop_oneof, Just, Strategy};
use proptest::test_runner::TestCaseError;
use rholang::rust::interpreter::accounting::authority::AuthorityByteEventKind;
use rholang::rust::interpreter::accounting::byte_accounting::ByteCharge;
use rholang::rust::interpreter::accounting::{
    NativeBudgetRecording, NativeObservationLink, NativeOperationRecord,
};
use rholang::rust::interpreter::errors::InterpreterError;
use rholang::rust::interpreter::reified::vectordb_services::ServiceReplyCode;
use rholang::rust::interpreter::spaces::namespace::{
    self, Durability, NameTail, NativeSpace, ReifiedId, SpaceId, SID_LEN,
};
use rholang::rust::interpreter::spaces::theory::identity::TheoryIdentity;
use rspace_plus_plus::rspace::trace::event::Event;
use support::family_programs::{
    space_program, Inner, Outer, StoredFamily, CONFIG_CALL, JOIN, STORED_FAMILIES,
};
use support::native::{
    assert_reproduces, certify, footprint_channels, host_work, is_space_row, native_play_with,
    NativeChain, Played, AMPLE_LIMIT, OUT,
};
use support::props::{check, runtime_config, tokio_runtime};
use support::session::{comms_on, error_codes, injected, native, Injections};

// ─── Plays and cost vectors ────────────────────────────────────────────────────────────────────

/// A native play and the host work that it used.
type Run = (Played, HostWorkUsages);

/// No injected ids.
fn none() -> Injections { Injections::new() }

/// Plays `term` on `chain` with the random state of `label` and the injected ids `injections`,
/// checks that the hot store holds no volatile channel after it, and certifies it.
async fn play_certified(
    chain: &mut NativeChain,
    term: &str,
    label: &str,
    injections: &Injections,
) -> Run {
    let host = host_work();
    let played = chain
        .play_with(term, label, AMPLE_LIMIT, host.clone(), injections.clone())
        .await;
    assert!(
        played.residue.is_empty(),
        "{label}: the sweep leaves no volatile channel: {:?}",
        played.residue
    );
    let certification = certify(&played, term, AMPLE_LIMIT).await;
    assert_reproduces(label, &played, &certification);
    (played, host.usages())
}

/// Plays `term` like [`play_certified`] and checks that the deploy has no error.
async fn play_clean(chain: &mut NativeChain, term: &str, label: &str) -> Run {
    let run = play_certified(chain, term, label, &none()).await;
    assert!(
        run.0.result.errors.is_empty(),
        "{label}: the deploy fails: {:?}",
        run.0.result.errors
    );
    run
}

fn recording(played: &Played) -> &NativeBudgetRecording {
    played
        .result
        .native_budget_recording
        .as_ref()
        .expect("native play records its budget")
}

fn journal(played: &Played) -> &[NativeOperationRecord] {
    played
        .result
        .native_operation_recording
        .as_deref()
        .expect("native play records its operations")
}

/// Checks that the cold play `cold` equals the warm play `warm` from the same pre-state: the
/// outcome (errors, post-state root, event log, outputs) and the cost vector (RH-1, RH-4).
fn assert_same_play(label: &str, warm: &Run, cold: &Run) {
    let ((warm, warm_host), (cold, cold_host)) = (warm, cold);
    assert_eq!(cold.result.errors, warm.result.errors, "{label}: errors");
    assert_eq!(cold.root, warm.root, "{label}: post-state root");
    assert_eq!(cold.log, warm.log, "{label}: event log");
    assert_eq!(cold.outputs, warm.outputs, "{label}: outputs");
    assert_eq!(
        cold.result.native_phlo_usage, warm.result.native_phlo_usage,
        "{label}: phlo usage"
    );
    assert_eq!(
        recording(cold).used,
        recording(warm).used,
        "{label}: budget usage"
    );
    assert_eq!(
        recording(cold).attempts,
        recording(warm).attempts,
        "{label}: budget attempts"
    );
    assert_eq!(journal(cold), journal(warm), "{label}: journal rows");
    assert_eq!(
        cold.result.authority_events, warm.result.authority_events,
        "{label}: authority events"
    );
    assert_eq!(
        cold.result.byte_observations, warm.result.byte_observations,
        "{label}: byte observations"
    );
    assert_eq!(cold_host, warm_host, "{label}: host work");
}

/// The numeric cost vector of a play, which does not name channels: the phlo usage, the budget
/// usage, the grant, the kind and the measurement of each budget attempt, the number of journal
/// rows, and the host work.
#[derive(Debug, PartialEq, Eq)]
struct CostVector {
    phlo: Option<u64>,
    used: u64,
    attempts: Vec<(bool, AuthorityByteEventKind, Option<ByteCharge>)>,
    rows: usize,
    host: HostWorkUsages,
}

fn cost_vector(run: &Run) -> CostVector {
    let (played, host) = run;
    let recording = recording(played);
    CostVector {
        phlo: played.result.native_phlo_usage,
        used: recording.used,
        attempts: recording
            .attempts
            .iter()
            .map(|attempt| {
                (
                    attempt.granted,
                    attempt.observation.kind,
                    attempt.observation.measurement,
                )
            })
            .collect(),
        rows: journal(played).len(),
        host: *host,
    }
}

/// The host work of `run` in each dimension, in the order of `HostWorkDimension::ALL`.
fn host_array(run: &Run) -> [u64; 16] { run.1.into_array().map(|usage| usage.get()) }

// ─── Outputs ───────────────────────────────────────────────────────────────────────────────────

fn list(par: &Par) -> Option<&Vec<Par>> {
    match par.exprs.as_slice() {
        [expr] => match &expr.expr_instance {
            Some(ExprInstance::EListBody(list)) => Some(&list.ps),
            _ => None,
        },
        _ => None,
    }
}

fn string(par: &Par) -> Option<&str> {
    match par.exprs.as_slice() {
        [expr] => match &expr.expr_instance {
            Some(ExprInstance::GString(text)) => Some(text.as_str()),
            _ => None,
        },
        _ => None,
    }
}

/// The values that `played` holds on [`OUT`] under the owner `owner` and the label `what`: the
/// elements after the label of each pair `[owner, [what, …]]`.
fn delivered(played: &Played, owner: &str, what: &str) -> Vec<Vec<Par>> {
    played
        .out
        .iter()
        .filter_map(|pars| {
            let [pair] = pars.as_slice() else {
                return None;
            };
            let [first, inner] = list(pair)?.as_slice() else {
                return None;
            };
            (string(first)? == owner).then_some(())?;
            let (head, rest) = list(inner)?.split_first()?;
            (string(head)? == what).then(|| rest.to_vec())
        })
        .collect()
}

/// The entry `key` of the map `map`.
fn entry(map: &Par, key: &str) -> Option<Par> {
    match map.exprs.as_slice() {
        [expr] => match &expr.expr_instance {
            Some(ExprInstance::EMapBody(body)) => body.kvs.iter().find_map(|pair| {
                match pair.key.as_ref().and_then(string) == Some(key) {
                    true => pair.value.clone(),
                    false => None,
                }
            }),
            _ => None,
        },
        _ => None,
    }
}

/// The codes of `errors`, as `error_codes` gives them. A space error that the produce of a
/// system process reply meets on its route (R7) comes back inside the RSpace error of that
/// produce, so the code is the `<Code>` of the first `reified:<Code>:` of its Debug form.
fn codes(errors: &[InterpreterError]) -> Vec<String> {
    error_codes(errors)
        .into_iter()
        .map(|code| match code.split_once("reified:") {
            Some((_, rest)) => rest.split(':').next().unwrap_or(rest).to_string(),
            None => code,
        })
        .collect()
}

/// The Debug forms of `errors`.
fn messages(errors: &[InterpreterError]) -> Vec<String> {
    errors.iter().map(|error| format!("{error:?}")).collect()
}

// ─── Stored state ──────────────────────────────────────────────────────────────────────────────

/// The channels of the space rows of `played`: every channel that its space transactions lock,
/// decoded from the footprints of its journal rows.
fn space_footprint(played: &Played) -> BTreeSet<Par> {
    let recording = recording(played);
    journal(played)
        .iter()
        .filter(|row| is_space_row(recording, row))
        .flat_map(footprint_channels)
        .collect()
}

/// The state of the space channels `channels` that `chain` reads: for each channel, the Debug
/// forms of its data and of its waiting continuations, each sorted.
async fn snapshot(
    chain: &NativeChain,
    channels: &BTreeSet<Par>,
) -> Vec<(String, Vec<String>, Vec<String>)> {
    let mut state = Vec::with_capacity(channels.len());
    for channel in channels {
        let mut data: Vec<String> = chain
            .data(channel)
            .await
            .iter()
            .map(|datum| format!("{datum:?}"))
            .collect();
        data.sort_unstable();
        let mut waiters: Vec<String> = chain
            .waiters(channel)
            .await
            .iter()
            .map(|waiter| format!("{waiter:?}"))
            .collect();
        waiters.sort_unstable();
        state.push((format!("{channel:?}"), data, waiters));
    }
    state
}

/// Plays block 1 of every stored family on a new chain, and returns the chain and the root of
/// block 1.
async fn stored_chain() -> (
    NativeChain,
    rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash,
) {
    let mut chain = NativeChain::new().await;
    let mut root = None;
    for family in &STORED_FAMILIES {
        let (played, _) = play_clean(
            &mut chain,
            &family.creation(),
            &format!("create {}", family.label),
        )
        .await;
        root = Some(played.root);
    }
    (chain, root.expect("block 1 creates the stored families"))
}

// ─── RH-1: lazy attach ─────────────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_lazy_attach() {
    let (mut warm, root) = stored_chain().await;
    let mut cold = warm.restarted(&root).await;
    assert!(
        cold.space_channels(Durability::Durable).await.is_empty(),
        "a restarted runtime holds no space channel before its first operation"
    );
    for family in &STORED_FAMILIES {
        let label = format!("touch {}", family.label);
        let term = family.touching();
        let warm_run = play_clean(&mut warm, &term, &label).await;
        let cold_run = play_clean(&mut cold, &term, &label).await;
        for what in ["config", "take", "fired"] {
            assert_eq!(
                delivered(&cold_run.0, family.label, what).len(),
                1,
                "{label}: the cold runtime delivers {what}: {:?}",
                cold_run.0.outputs
            );
        }
        assert_same_play(&label, &warm_run, &cold_run);
    }
}

// ─── RH-2: checkpoint and reset ────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_checkpoint_reset() {
    for family in &STORED_FAMILIES {
        let label = family.label;
        let mut chain = NativeChain::new().await;
        let (created, _) = play_clean(&mut chain, &family.creation(), "create").await;
        let root = created.root.clone();
        // The space channels of block 2, from a play on a restarted runtime that the test then
        // drops.
        let mut probe = chain.restarted(&root).await;
        let (probed, _) = play_clean(&mut probe, &family.touching(), "touch").await;
        let channels = space_footprint(&probed);
        assert!(
            !channels.is_empty(),
            "{label}: block 2 locks space channels"
        );
        let before = snapshot(&chain, &channels).await;
        assert!(
            before.iter().any(|(_, data, _)| !data.is_empty())
                && before.iter().any(|(_, _, waiters)| !waiters.is_empty()),
            "{label}: block 1 leaves records and a waiter on the channels of block 2"
        );
        let first = play_clean(&mut chain, &family.touching(), "touch").await;
        assert_ne!(
            snapshot(&chain, &channels).await,
            before,
            "{label}: block 2 changes the space"
        );
        chain.reset(&root).await;
        assert_eq!(
            snapshot(&chain, &channels).await,
            before,
            "{label}: the reset restores the state of block 1"
        );
        let again = play_clean(&mut chain, &family.touching(), "touch").await;
        assert_same_play(&format!("{label}: block 2 after the reset"), &first, &again);
        let restarted = chain.restarted(&root).await;
        assert_eq!(
            snapshot(&restarted, &channels).await,
            before,
            "{label}: a restarted runtime reads the state of block 1"
        );
    }
}

// ─── RH-4: the attach cost ─────────────────────────────────────────────────────────────────────

/// The pinned cost vector of an attach: the phlo usage and the host work in the order of
/// `HostWorkDimension::ALL`.
struct PinnedCost {
    label: &'static str,
    body: &'static str,
    phlo: u64,
    host: [u64; 16],
}

/// The attach programs of `space_attach_cost` over the stored queue, with the cost vectors that
/// this build measures (P7 step 5).
const PINNED: [PinnedCost; 2] = [
    PinnedCost {
        label: "handle call config",
        body: CONFIG_CALL,
        phlo: 13_594,
        host: [
            1, 1_414, 9, 1_391, 15, 685, 39, 1_220, 34, 28, 0, 522_199, 0, 0, 182_890, 3_969_620,
        ],
    },
    PinnedCost {
        label: "send on a stored collection",
        body: "a!(4)",
        phlo: 7_909,
        host: [
            1, 848, 4, 459, 2, 4, 14, 285, 20, 16, 0, 472_265, 0, 0, 148_895, 4_226_954,
        ],
    },
];

/// Checks that the accounting observer receives the cost of each COMM of `played`: one journal
/// row with a COMM record for each COMM of the log, and each COMM record links to a budget
/// observation of the kind `Comm` (a granted attempt, or the retry of a persistent produce).
fn assert_comms_observed(label: &str, played: &Played) {
    let recording = recording(played);
    let comms = played
        .log
        .iter()
        .filter(|event| matches!(event, Event::Comm(_)))
        .count();
    let records: Vec<_> = journal(played)
        .iter()
        .filter_map(|row| row.comm.as_ref())
        .collect();
    assert!(comms > 0, "{label}: the deploy makes COMMs");
    assert_eq!(
        records.len(),
        comms,
        "{label}: one COMM record for each COMM"
    );
    for comm in records {
        let observation = match comm.observation {
            NativeObservationLink::Attempt(index) => {
                let attempt = &recording.attempts[index];
                assert!(attempt.granted, "{label}: the COMM attempt is granted");
                &attempt.observation
            }
            NativeObservationLink::Retry(index) => &recording.retries[index].observation,
        };
        assert_eq!(
            observation.kind,
            AuthorityByteEventKind::Comm,
            "{label}: the COMM record links to a COMM observation"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_attach_cost() {
    let family = &STORED_FAMILIES[0];
    let mut warm = NativeChain::new().await;
    let (created, _) = play_clean(&mut warm, &family.creation(), "create").await;
    let root = created.root;
    for pinned in &PINNED {
        let term = family.with_stash(pinned.body);
        warm.reset(&root).await;
        let warm_run = play_clean(&mut warm, &term, pinned.label).await;
        let mut cold = warm.restarted(&root).await;
        let cold_run = play_clean(&mut cold, &term, pinned.label).await;
        assert_same_play(pinned.label, &warm_run, &cold_run);
        assert_comms_observed(pinned.label, &cold_run.0);
        assert_eq!(
            (cold_run.0.result.native_phlo_usage, host_array(&cold_run)),
            (Some(pinned.phlo), pinned.host),
            "{}: the pinned cost vector",
            pinned.label
        );
    }
}

// ─── RH-1 and RH-4 over every family ───────────────────────────────────────────────────────────

/// Block 1 of the family `outer:inner`: the family program without its outer part, with the
/// space and the names `a` to `e` stored on the stash channel of the family.
fn family_creation(outer: Outer, inner: Inner) -> String {
    space_program(
        &format!("rho:space:{}:{}", outer.name(), inner.name()),
        &outer.ctor("default"),
        &format!(
            r#"@"{}"!(*space, *a, *b, *c, *d, *e) | {} | {}"#,
            family_stash(outer, inner),
            inner.part(),
            JOIN
        ),
    )
}

fn family_stash(outer: Outer, inner: Inner) -> String {
    format!("rrs-p7-family-{}-{}", outer.name(), inner.name())
}

/// Block 2 of the family `outer:inner`: the handle call `config`, a take with a store, and the
/// outer part of the family program, over the stored space and names.
fn family_touch(outer: Outer, inner: Inner) -> String {
    format!(
        r#"new out in {{
  for (space, a, b, c, d, e <<- @"{stash}") {{
    {CONFIG_CALL} |
    for (@x <- a) {{ out!(["again", x]) | a!(x + 10) }} |
    {part}
  }} |
  for (@value <= out) {{ @"{OUT}"!(value) }}
}}"#,
        stash = family_stash(outer, inner),
        part = outer.part(),
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_attach_warm_cold_cost_equal() {
    for outer in Outer::ALL {
        for inner in Inner::ALL {
            let label = format!("{}:{}", outer.name(), inner.name());
            let mut warm = NativeChain::new().await;
            let (created, _) = play_clean(
                &mut warm,
                &family_creation(outer, inner),
                &format!("{label} create"),
            )
            .await;
            let mut cold = warm.restarted(&created.root).await;
            let term = family_touch(outer, inner);
            let warm_run = play_clean(&mut warm, &term, &label).await;
            let cold_run = play_clean(&mut cold, &term, &label).await;
            assert_same_play(&label, &warm_run, &cold_run);
        }
    }
}

/// A block-2 operation of the property.
#[derive(Clone, Copy, Debug)]
enum Touch {
    Config,
    Gensym,
    Take,
    Peek,
    Send,
}

impl Touch {
    fn body(self) -> &'static str {
        match self {
            Touch::Config => CONFIG_CALL,
            Touch::Gensym => r#"for (n <- space!?("gensym")) { n!(1) }"#,
            Touch::Take => r#"for (@x <- a) { out!(["take", x]) }"#,
            Touch::Peek => r#"for (@x <<- a) { out!(["peek", x]) }"#,
            Touch::Send => "a!(7)",
        }
    }
}

/// A case of the property: a family, the records of `a` that block 1 stores, and the
/// operations of block 2.
#[derive(Clone, Debug)]
struct AttachCase {
    outer: Outer,
    inner: Inner,
    records: Vec<i64>,
    touches: Vec<Touch>,
}

fn attach_cases() -> impl Strategy<Value = AttachCase> {
    let outer = prop_oneof![
        Just(Outer::HashMap),
        Just(Outer::PathMap),
        Just(Outer::Array),
        Just(Outer::Vector)
    ];
    let inner = prop_oneof![
        Just(Inner::Bag),
        Just(Inner::Queue),
        Just(Inner::Stack),
        Just(Inner::Set),
        Just(Inner::Cell),
        Just(Inner::PriorityQueue)
    ];
    let touch = prop_oneof![
        Just(Touch::Config),
        Just(Touch::Gensym),
        Just(Touch::Take),
        Just(Touch::Peek),
        Just(Touch::Send)
    ];
    (
        outer,
        inner,
        prop::collection::vec(0_i64..20, 0..4),
        prop::collection::vec(touch, 1..4),
    )
        .prop_map(|(outer, inner, records, touches)| AttachCase {
            outer,
            inner,
            records,
            touches,
        })
}

/// Block 1 of a case: the space, its records on `a` (one at most in a cell), and the stash.
fn case_creation(case: &AttachCase) -> String {
    let limit = match case.inner {
        Inner::Cell => 1,
        _ => case.records.len(),
    };
    let mut body = r#"@"rrs-p7-case"!(*space, *a)"#.to_string();
    for value in case.records.iter().take(limit) {
        body.push_str(&format!(" | a!({value})"));
    }
    space_program(
        &format!("rho:space:{}:{}", case.outer.name(), case.inner.name()),
        &case.outer.ctor("default"),
        &body,
    )
}

/// Block 2 of a case: its operations in parallel over the stored space and `a`.
fn case_touch(case: &AttachCase) -> String {
    let body: Vec<&str> = case.touches.iter().map(|touch| touch.body()).collect();
    format!(
        r#"new out in {{
  for (space, a <<- @"rrs-p7-case") {{ {} }} |
  for (@value <= out) {{ @"{OUT}"!(value) }}
}}"#,
        body.join(" | ")
    )
}

#[test]
fn prop_attach_warm_cold_cost_equal() {
    check(runtime_config(), attach_cases(), |case| {
        tokio_runtime().block_on(async {
            let mut warm = NativeChain::new().await;
            let (created, _) =
                play_certified(&mut warm, &case_creation(&case), "create", &none()).await;
            proptest::prop_assert!(
                created.result.errors.is_empty(),
                "block 1 fails: {:?}",
                created.result.errors
            );
            let mut cold = warm.restarted(&created.root).await;
            let term = case_touch(&case);
            let warm_run = play_certified(&mut warm, &term, "touch", &none()).await;
            let cold_run = play_certified(&mut cold, &term, "touch", &none()).await;
            assert_same_play("touch", &warm_run, &cold_run);
            Ok::<(), TestCaseError>(())
        })
    });
}

// ─── Missing and restored handles ──────────────────────────────────────────────────────────────

/// A durable space that no factory created.
fn missing_space(byte: u8) -> NativeSpace {
    NativeSpace {
        sid: [byte; SID_LEN],
        durability: Durability::Durable,
    }
}

/// The injected handle `h` and name `n` of the missing durable space `space`.
fn missing_injections(space: NativeSpace) -> Injections {
    Injections::from([
        (
            injected("missing-handle"),
            namespace::gprivate(namespace::handle_channel(space)),
        ),
        (
            injected("missing-name"),
            namespace::gprivate(namespace::name_id(space, NameTail::Random([0x11; 32]))),
        ),
    ])
}

/// The binders of the injected ids of a missing space.
const MISSING: &str = "new h(`rho:rrs:test:missing-handle`), n(`rho:rrs:test:missing-name`) in";

/// The entries of DG-2 for a missing durable space: the body after [`MISSING`], and the code.
const DG2_ENTRIES: [(&str, &str, &str); 5] = [
    ("send on a name", "{ n!(1) }", "DanglingName"),
    (
        "receive on a name",
        "{ for (@v <- n) { Nil } }",
        "DanglingName",
    ),
    (
        "forged channel in use",
        r#"{ use h { @"rrs-p7-forged"!(1) } }"#,
        "DanglingHandle",
    ),
    (
        "new in the space",
        "{ new y : h in { Nil } }",
        "DanglingHandle",
    ),
    (
        "reply of a system process to a name (R7)",
        "{ new eq(`rho:assert:assertEqual`) in { eq!(*n, 1, 1) } }",
        "DanglingName",
    ),
];

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reducer_attach_costs() {
    // DG-4: one cost vector for each entry of a missing space, for any missing space id, on a
    // fresh runtime and on a runtime that holds the stored families.
    let (mut warm, root) = stored_chain().await;
    for (label, body, code) in DG2_ENTRIES {
        let term = format!("{MISSING} {body}");
        let mut vectors = Vec::with_capacity(4);
        for byte in [0x5A, 0xA5] {
            let injections = missing_injections(missing_space(byte));
            let mut fresh = NativeChain::new().await;
            for chain in [&mut fresh, &mut warm] {
                let run = play_certified(chain, &term, label, &injections).await;
                assert_eq!(
                    codes(&run.0.result.errors),
                    vec![code.to_string()],
                    "{label}"
                );
                vectors.push(cost_vector(&run));
            }
        }
        assert!(
            vectors.windows(2).all(|pair| pair[0] == pair[1]),
            "{label}: one cost vector for every missing space and runtime: {vectors:#?}"
        );
    }
    // Restored handles: the handle calls through a stored handle cost the same on the runtime
    // that created the space and on a restarted runtime.
    for family in &STORED_FAMILIES {
        for (method, body) in [
            ("config", CONFIG_CALL),
            ("gensym", r#"for (n <- space!?("gensym")) { n!(1) }"#),
        ] {
            let label = format!("{} {method}", family.label);
            let term = family.with_stash(body);
            warm.reset(&root).await;
            let warm_run = play_clean(&mut warm, &term, &label).await;
            let mut cold = warm.restarted(&root).await;
            let cold_run = play_clean(&mut cold, &term, &label).await;
            assert_same_play(&label, &warm_run, &cold_run);
        }
    }
}

// ─── The handle across a restart ───────────────────────────────────────────────────────────────

/// A deploy that creates a queue space and stores its handle and a name of it.
const SETUP: &str = r#"
new factory(`rho:space:hashmap:queue`) in {
  for (queue <- factory!?("default")) {
    @"rrs-p7-handle"!(*queue) |
    for (a <- queue!?("gensym")) { @"rrs-p7-name"!(*a) }
  }
}
"#;

/// Three handle calls after a restart. The handle continuation in the trie forwards each call.
const CALLS: &str = r#"
new out in {
  for (queue <<- @"rrs-p7-handle") { for (a <<- @"rrs-p7-name") {
    for (fresh <- queue!?("gensym")) {
      for (@config <- queue!?("config")) {
        for (_ <- queue!?("produce", *a, [7, 8])) {
          for (@x, @y <- a) { out!(["calls", [x, y, config]]) | fresh!(x) }
        }
      }
    }
  } } |
  for (@value <= out) { @"rrs-native-out"!(["handle", value]) }
}
"#;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reified_handle_persisted_restart_replay() {
    let mut chain = NativeChain::new().await;
    let (created, _) = play_clean(&mut chain, SETUP, "setup").await;
    let mut restarted = chain.restarted(&created.root).await;
    let (played, _) = play_clean(&mut restarted, CALLS, "calls").await;
    let handle_op = namespace::gprivate(namespace::op_channel(namespace::HANDLE_OP));
    assert_eq!(
        comms_on(&played.log, &handle_op),
        3,
        "the three handle calls reach HANDLE_OP through the stored handle continuation"
    );
    let calls = delivered(&played, "handle", "calls");
    let [call] = calls.as_slice() else {
        panic!("one result of the calls: {:?}", played.outputs);
    };
    let [values] = call.as_slice() else {
        panic!("the calls deliver one list: {call:?}");
    };
    let values = list(values).expect("the calls deliver a list");
    let [x, y, config] = values.as_slice() else {
        panic!("the list holds x, y and the config: {values:?}");
    };
    assert_eq!(
        (x, y),
        (
            &models::rust::utils::new_gint_par(7, Vec::new(), false),
            &models::rust::utils::new_gint_par(8, Vec::new(), false)
        )
    );
    assert_eq!(
        entry(config, "qualifier").as_ref().and_then(string),
        Some("default")
    );
}

// ─── DG-1 to DG-3: every row ───────────────────────────────────────────────────────────────────

/// The stash of the dead `temp` space and the channel of its parked continuation.
const DEAD_STASH: &str = "rrs-p7-dead";
const WAKE: &str = "rrs-p7-wake";
const REPLY: &str = "rrs-p7-reply";

/// A deploy that creates a `temp` queue, stores its handle and a name durably, parks a durable
/// continuation on [`WAKE`] that sends on the name, and stores a record on the name.
fn dead_creation() -> String {
    format!(
        r#"new factory(`rho:space:hashmap:queue`) in {{
  for (space <- factory!?("temp")) {{
    for (x <- space!?("gensym")) {{
      @"{DEAD_STASH}"!(*space, *x) | for (_ <- @"{WAKE}") {{ x!(99) }} | x!(0)
    }}
  }}
}}"#
    )
}

/// Plays the deploy `term` and checks that it fails with the one code `code` and names `sid`.
async fn expect_dangling(
    chain: &mut NativeChain,
    term: &str,
    label: &str,
    injections: &Injections,
    code: &str,
    sid: SpaceId,
) {
    let (played, _) = play_certified(chain, term, label, injections).await;
    assert_eq!(
        codes(&played.result.errors),
        vec![code.to_string()],
        "{label}"
    );
    let hex = hex::encode(sid);
    assert!(
        messages(&played.result.errors)
            .iter()
            .any(|message| message.contains(&hex)),
        "{label}: the error names the space id {hex}: {:?}",
        played.result.errors
    );
}

/// The reply that the chain stored on [`REPLY`] under the label `label`.
async fn reply(chain: &NativeChain, label: &str) -> Par {
    let replies = chain.values(REPLY).await;
    let found: Vec<&Par> = replies
        .iter()
        .filter_map(|pars| {
            let [pair] = pars.as_slice() else {
                return None;
            };
            let [first, value] = list(pair)?.as_slice() else {
                return None;
            };
            (string(first)? == label).then_some(value)
        })
        .collect();
    let [value] = found.as_slice() else {
        panic!("{label}: one reply: {replies:?}");
    };
    (*value).clone()
}

/// Checks that `reply` is the typed reply `DanglingHandle` of `getSpaceAgent` for `sid`.
fn assert_dangling_reply(label: &str, reply: &Par, sid: SpaceId) {
    let debug = format!("{reply:?}");
    assert!(
        debug.contains("rho:space:error") && debug.contains("DanglingHandle"),
        "{label}: {debug}"
    );
    let detail = sid.iter().map(|byte| byte.to_string()).collect::<Vec<_>>();
    assert!(
        debug.contains("spaceId") && debug.contains(&format!("[{}]", detail.join(", "))),
        "{label}: the reply names the space id: {debug}"
    );
}

/// Checks that `reply` is the reply `NotAVectorDbSpace` of `ann`.
fn assert_not_a_vectordb_reply(label: &str, reply: &Par) {
    let values = list(reply).expect("the reply of ann is a list");
    assert_eq!(
        values.get(1).and_then(string),
        Some(ServiceReplyCode::NotAVectorDbSpace.code()),
        "{label}: {reply:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reducer_dangling_handle() {
    let mut chain = NativeChain::new().await;
    play_clean(&mut chain, &dead_creation(), "create a temp space").await;
    let stash = chain.values(DEAD_STASH).await;
    let [entry] = stash.as_slice() else {
        panic!("one stash entry: {stash:?}");
    };
    let dead = native(entry.first().expect("the stash holds the handle"));
    assert_eq!(dead.durability, Durability::Ephemeral);
    let missing = missing_space(0x5A);
    let mut injections = missing_injections(missing);
    injections.insert(
        injected("dead-config"),
        namespace::gprivate(namespace::config_channel(dead)),
    );
    let peek = format!(r#"for (handle, name <<- @"{DEAD_STASH}")"#);
    let dead_rows: Vec<(&str, String, &str)> = vec![
        (
            "DG-1 handle call",
            format!(r#"{peek} {{ for (_ <- handle!?("config")) {{ Nil }} }}"#),
            "DanglingHandle",
        ),
        (
            "DG-1 send on a name",
            format!("{peek} {{ name!(2) }}"),
            "DanglingName",
        ),
        (
            "DG-1 receive on a name",
            format!("{peek} {{ for (@v <- name) {{ Nil }} }}"),
            "DanglingName",
        ),
        (
            "DG-1 join with a dead name in its second bind",
            format!(r#"{peek} {{ for (@x <- @"rrs-p7-host" & @v <- name) {{ Nil }} }}"#),
            "DanglingName",
        ),
        (
            "DG-1 another ephemeral id",
            "new c(`rho:rrs:test:dead-config`) in { c!(1) }".to_string(),
            "DanglingHandle",
        ),
        (
            "DG-1 use",
            format!("{peek} {{ use handle {{ Nil }} }}"),
            "DanglingHandle",
        ),
        (
            "DG-1 new in the space",
            format!("{peek} {{ new y : handle in {{ Nil }} }}"),
            "DanglingHandle",
        ),
        (
            "DG-1 parked continuation that fires into the dead space",
            format!(r#"@"{WAKE}"!(Nil)"#),
            "DanglingName",
        ),
        (
            "DG-2 reply of a system process to a dead name (R7)",
            format!("{peek} {{ new eq(`rho:assert:assertEqual`) in {{ eq!(*name, 1, 1) }} }}"),
            "DanglingName",
        ),
    ];
    for (label, term, code) in &dead_rows {
        expect_dangling(&mut chain, term, label, &injections, code, dead.sid).await;
    }
    for (label, body, code) in DG2_ENTRIES {
        let label = format!("DG-2 {label}");
        let term = format!("{MISSING} {body}");
        expect_dangling(&mut chain, &term, &label, &injections, code, missing.sid).await;
    }
    // The typed replies: `getSpaceAgent` answers `DanglingHandle` with the space id (DG-1 and
    // DG-2), and `ann` answers `NotAVectorDbSpace` (decision 67 order).
    let agent = |label: &str, binders: &str, space: &str| {
        format!(
            r#"{binders} {{ new gsa(`rho:space:getSpaceAgent`) in {{
  for (@reply <- gsa!?(*{space})) {{ @"{REPLY}"!(["{label}", reply]) }}
}} }}"#
        )
    };
    let ann = |label: &str, binders: &str, space: &str, name: &str| {
        format!(
            r#"{binders} {{ new ann(`rho:vectordb:ann`), r in {{
  ann!(*r, *{space}, *{name}, [1.0, 0.0], {{"k": 1}}) |
  for (@reply <- r) {{ @"{REPLY}"!(["{label}", reply]) }}
}} }}"#
        )
    };
    for (label, term) in [
        (
            "DG-1 getSpaceAgent",
            agent("DG-1 getSpaceAgent", &peek, "handle"),
        ),
        (
            "DG-2 getSpaceAgent",
            agent("DG-2 getSpaceAgent", MISSING, "h"),
        ),
        ("DG-1 ann", ann("DG-1 ann", &peek, "handle", "name")),
        ("DG-2 ann", ann("DG-2 ann", MISSING, "h", "n")),
    ] {
        let (played, _) = play_certified(&mut chain, &term, label, &injections).await;
        assert!(
            played.result.errors.is_empty(),
            "{label}: {:?}",
            played.result.errors
        );
    }
    assert_dangling_reply(
        "DG-1 getSpaceAgent",
        &reply(&chain, "DG-1 getSpaceAgent").await,
        dead.sid,
    );
    assert_dangling_reply(
        "DG-2 getSpaceAgent",
        &reply(&chain, "DG-2 getSpaceAgent").await,
        missing.sid,
    );
    assert_not_a_vectordb_reply("DG-1 ann", &reply(&chain, "DG-1 ann").await);
    assert_not_a_vectordb_reply("DG-2 ann", &reply(&chain, "DG-2 ann").await);
}

// ─── Child metering under two schedules ────────────────────────────────────────────────────────

/// Parallel space work on two spaces and a host channel.
const PARALLEL: &str = r#"
new queues(`rho:space:hashmap:queue`), bags(`rho:space:hashmap:bag`), out in {
  for (q <- queues!?("default"); s <- bags!?("default")) {
    for (a <- q!?("gensym"); b <- s!?("gensym")) {
      a!(1) | a!(2) | a!(3) | b!(4) | b!(5) | b!(6) |
      for (@x <- a) { out!(["queue", x]) } | for (@y <- a) { out!(["queue", y]) } |
      for (@z <- b) { out!(["bag", z]) } | for (@w <<- b) { out!(["peek", w]) } |
      for (@c <- q!?("config")) { out!(["config", 1]) } |
      for (n <- s!?("gensym")) { n!(8) | for (@v <- n) { out!(["name", v]) } } |
      @"rrs-p7-host"!(7) | for (@h <- @"rrs-p7-host") { out!(["host", h]) }
    }
  } |
  for (@value <= out) { @"rrs-native-out"!(value) }
}
"#;

/// Native play of [`PARALLEL`] on `runtime`, certified.
fn parallel_run(runtime: tokio::runtime::Runtime) -> Run {
    runtime.block_on(async {
        let host = host_work();
        let played = native_play_with(PARALLEL, AMPLE_LIMIT, host.clone()).await;
        assert!(
            played.result.errors.is_empty(),
            "{:?}",
            played.result.errors
        );
        let certification = certify(&played, PARALLEL, AMPLE_LIMIT).await;
        assert_reproduces("parallel space work", &played, &certification);
        (played, host.usages())
    })
}

#[test]
fn reducer_space_costs() {
    let current = parallel_run(
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build a current-thread tokio runtime"),
    );
    let multi = parallel_run(
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(4)
            .enable_all()
            .build()
            .expect("build a multi-thread tokio runtime"),
    );
    assert_eq!(current.0.outputs.len(), 7, "{:?}", current.0.outputs);
    assert_same_play("current-thread and multi-thread", &current, &multi);
}

// ─── The factory of P7 ─────────────────────────────────────────────────────────────────────────

const STAGED: &str = "rrs-p7-staged";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reified_factory_ephemeral_staging() {
    for (qualifier, durability, handle_kind) in [
        ("default", Durability::Durable, b'H'),
        ("temp", Durability::Ephemeral, b'h'),
        ("seq", Durability::Ephemeral, b'h'),
    ] {
        let term = format!(
            r#"new factory(`rho:space:hashmap:queue`) in {{
  for (space <- factory!?("{qualifier}")) {{ @"{STAGED}-{qualifier}"!(*space) }}
}}"#
        );
        let mut chain = NativeChain::new().await;
        let (played, _) = play_clean(&mut chain, &term, qualifier).await;
        let stash = chain.values(&format!("{STAGED}-{qualifier}")).await;
        let [entry] = stash.as_slice() else {
            panic!("{qualifier}: one handle: {stash:?}");
        };
        let space = native(entry.first().expect("the stash holds the handle"));
        assert_eq!(space.durability, durability, "{qualifier}");
        assert_eq!(
            namespace::handle_channel(space).get(1),
            Some(&handle_kind),
            "{qualifier}: the KIND of the handle"
        );
        // The staged rows of the creation touch the config, the space counter and the handle
        // of the space, each with the durability of the qualifier.
        let recording = recording(&played);
        let mut kinds = BTreeSet::new();
        for row in journal(&played) {
            if !is_space_row(recording, row) {
                continue;
            }
            for channel in footprint_channels(row) {
                let (kind, of) = match namespace::reified_id_of(&channel) {
                    Some(ReifiedId::Config(of)) => ("config", of),
                    Some(ReifiedId::SpaceCounter(of)) => ("space counter", of),
                    Some(ReifiedId::Handle(of)) => ("handle", of),
                    other => panic!("{qualifier}: a staged channel of the creation: {other:?}"),
                };
                assert_eq!(
                    of, space,
                    "{qualifier}: the staged {kind} belongs to the space"
                );
                kinds.insert(kind);
            }
        }
        assert_eq!(
            kinds,
            BTreeSet::from(["config", "handle", "space counter"]),
            "{qualifier}: the staged ids"
        );
        // Only a `default` space leaves its channels after its deploy.
        let left: BTreeSet<Vec<u8>> = played
            .durable
            .iter()
            .filter_map(|channel| namespace::sole_gprivate_id(channel).map(<[u8]>::to_vec))
            .collect();
        let expected: BTreeSet<Vec<u8>> = match durability {
            Durability::Durable => [
                namespace::config_channel(space),
                namespace::space_counter(space),
                namespace::handle_channel(space),
            ]
            .into_iter()
            .collect(),
            Durability::Ephemeral => BTreeSet::new(),
        };
        assert_eq!(left, expected, "{qualifier}: the channels after the deploy");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reified_factory_theory_argument() {
    // A theory argument persists in the config of a durable space, and the first operation
    // after a restart resolves the stored identity.
    let family = StoredFamily {
        label: "nat",
        urn: "rho:space:hashmap:queue",
        ctor: r#""default", free Nat()"#,
        state: "a!(1) | a!(2)",
        touch: "Nil",
    };
    let mut chain = NativeChain::new().await;
    let (created, _) = play_clean(&mut chain, &family.creation(), "create").await;
    let mut cold = chain.restarted(&created.root).await;
    let (admitted, _) = play_clean(
        &mut cold,
        &family.with_stash(r#"a!(5) | for (@x <- a) { out!(["take", x]) }"#),
        "admitted",
    )
    .await;
    assert_eq!(delivered(&admitted, "nat", "take"), vec![vec![
        models::rust::utils::new_gint_par(1, Vec::new(), false)
    ]]);
    let (refused, _) = play_certified(
        &mut cold,
        &family.with_stash(r#"a!("five")"#),
        "refused",
        &none(),
    )
    .await;
    assert_eq!(codes(&refused.result.errors), vec![
        "TheoryRefuted".to_string()
    ]);
    let (configured, _) = play_clean(&mut cold, &family.with_stash(CONFIG_CALL), "config").await;
    let configs = delivered(&configured, "nat", "config");
    let [config] = configs.as_slice() else {
        panic!("one config: {configs:?}");
    };
    let config = config.first().expect("the config map");
    assert_eq!(
        entry(config, "theory"),
        Some(TheoryIdentity::builtin("Nat").to_par()),
        "the config reply shows the stored identity"
    );
    // A `temp` space stages its theory too: a string is admitted, and a number is refused.
    let temp =
        |body: &str| space_program("rho:space:hashmap:queue", r#""temp", free String()"#, body);
    let mut chain = NativeChain::new().await;
    let (admitted, _) = play_clean(
        &mut chain,
        &temp(r#"a!("s") | for (@v <- a) { out!(["got", v]) }"#),
        "temp admitted",
    )
    .await;
    assert_eq!(admitted.outputs.len(), 1, "{:?}", admitted.outputs);
    let (refused, _) = play_certified(&mut chain, &temp("a!(1)"), "temp refused", &none()).await;
    assert_eq!(codes(&refused.result.errors), vec![
        "TheoryRefuted".to_string()
    ]);
}
