//! P9 step 2: N1, the pair of native play and native replay, and N9, the certified runs of one
//! deploy set (P9 design v2, sections 2, 4 and 7, amendments A1, A5 and A9, decisions 93 and 111).
// Changed by decision 93 (decision 111): N9 certifies each run. The old text follows.
// //! P9 step 2: N1, the pair of native play and native replay, and N9, two runs of one deploy set
// //! (P9 design v2, sections 2, 4 and 7, amendments A1, A5 and A9).
//!
//! Native replay keeps no event log (`native_session/space.rs`). Event equality of play and
//! replay holds by construction. The trace binding requires that the log of play is exactly the
//! events of the journal rows, in preorder (`checked_operations/trace.rs`). Replay authenticates
//! the footprint, the source and the ordered COMM of each operation, and the completeness check
//! requires every row. So `assert_reproduces` is the event check of N1. Two negative controls
//! show that the trace binding refuses a log in another order.
//!
//! | Test | Property |
//! |---|---|
//! | `n1_parked_wake_across_deploys_replay_equal` | Deploy 1 parks a waiter on a native space and a stored consume on the reference agent. Deploy 2 sends values that match neither, and deploy 3 wakes both. Each deploy certifies from its own pre-state. |
//! | `n1_failed_deploy_reverts_replay_equal` | A deploy fails after a committed space write. Native replay reproduces the failure. The block then continues from the pre-state, as Casper's settlement does after a user failure, and the next deploy sees the pre-state. |
//! | `n1_k_comm_replay_equal` | A same-space join logs a COMM with more than one produce. A set retirement and a VectorDB list delivery log a bind and a `K_retire` over one data channel, each with one produce (decision 101). Native replay reproduces each play. |
// Changed by decision 101 (C5.4): a set retirement and a list delivery are one bind and K_retire
// COMMs over `[D]`. The old row follows.
// //! | `n1_k_comm_replay_equal` | COMMs with more than one produce replay: a same-space join, a set retirement over `[D; n]` and a VectorDB list delivery. |
//! | `n1_trace_binding_rejects_swapped_events` | Negative control: the committed log with two adjacent events swapped fails the trace binding. |
//! | `n1_trace_binding_rejects_permuted_comm` | Negative control: the committed log with the produce list of a COMM permuted fails the trace binding. |
//! | `n9_run_twice_assert_canonical_equal` | The test makes three runs on each racing runtime. Each run plays the 24 family programs on one chain and the conformance kit of the reference agent on another. Each deploy logs events and certifies from its own pre-state. Each family program delivers an allowed label set, and the kit delivers its outputs in order (decisions 93 and 111). |
// Changed by decision 93 (decision 111): N9 certifies each run. The old row follows.
// //! | `n9_run_twice_assert_canonical_equal` | Two fresh native runtimes give the same roots, outputs, logs, evidence bytes and phlo for the 24 family programs and for the conformance kit of the reference agent. |
//!
//! Each program forwards each delivery to the public channel `rrs-native-out`, so the post-state
//! root witnesses the order of the deliveries. The costs of N1 are the phlo usage, the budget
//! usage, the authority events and the byte observations.
//!
//! The other rows of N1 are existing tests: the 24 family programs
//! (`reified_native_spaces.rs`, `space_native_replay_equal_*`), every VectorDB query shape
//! (`space_native_vectordb_replay_equal`), `temp` (`reified_temp_seq.rs`) and the `seq`
//! factories (`reified_hashset.rs`).

#[path = "support/mod.rs"]
mod support;

use std::collections::BTreeSet;

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{Expr, Par};
use rholang::rust::interpreter::spaces::namespace::{self, ReifiedId};
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::hashing::stable_hash_provider::hash;
// Changed by decision 101 (C5.4): the set and VectorDB rows read the produces of the COMMs over
// one data channel. The old import follows.
// use rspace_plus_plus::rspace::trace::event::Event;
use rspace_plus_plus::rspace::trace::event::{Event, Produce};
// Changed by decision 93 (decision 111): N9 checks each run against the allowed labels. The old
// import follows.
// use support::family_programs::{
//     kit_deploys, program, reference_agent_creation, space_program, with_kit_names, Inner, Outer,
//     JOIN, KIT_NAMES,
// };
use support::family_programs::{
    allowed_labels, kit_deploys, program, reference_agent_creation, space_program, with_kit_names,
    Inner, Outer, JOIN, KIT_NAMES,
};
// Changed by decision 93 (decision 111): N9 no longer compares two runs. The old import follows.
// use support::native::{
//     assert_reproduces, certify, committed_evidence, replay_environment_from,
//     run_twice_assert_canonical_equal, NativeChain, Played, AMPLE_LIMIT, OUT,
// };
// Changed by decision 101 (C5.4): the footprints of the journal rows name the data channels.
use support::native::{
    assert_reproduces, certify, committed_evidence, footprint_channels, replay_environment_from,
    NativeChain, Played, AMPLE_LIMIT, OUT,
};
use support::props::{racing_runtime, RACING_RUNS, RACING_WORKERS};
use support::session::error_codes;

/// The phlo limit of every deploy of the suite.
const LIMIT: u64 = AMPLE_LIMIT;

// The reference agent, its kit names and `with_kit_names` moved to `support::family_programs`
// (P9 step 3), so the P9 suites share them.

/// A send of `[label, value]` to the public channel [`OUT`].
fn out(label: &str, value: &str) -> String { format!("@\"{OUT}\"!([\"{label}\", {value}])") }

/// The one expression of `par`, when it has exactly one.
fn sole_expr(par: &Par) -> Option<&ExprInstance> {
    match par.exprs.as_slice() {
        [Expr {
            expr_instance: Some(instance),
        }] => Some(instance),
        _ => None,
    }
}

/// The label of a delivery `[label, …]` on [`OUT`].
fn label_of(value: &Par) -> Option<String> {
    let first = match sole_expr(value)? {
        ExprInstance::EListBody(list) => list.ps.first()?,
        _ => return None,
    };
    match sole_expr(first)? {
        ExprInstance::GString(label) => Some(label.clone()),
        _ => None,
    }
}

/// The labels of the deliveries on [`OUT`] after `played`, in bytewise order.
fn labels(played: &Played) -> Vec<String> {
    let mut labels: Vec<String> = played.out.iter().flatten().filter_map(label_of).collect();
    labels.sort();
    labels
}

/// The labels `expected`, in bytewise order.
fn expect(expected: &[&str]) -> Vec<String> {
    let mut labels: Vec<String> = expected.iter().map(|label| label.to_string()).collect();
    labels.sort();
    labels
}

/// Plays `term` as the next deploy of `chain`, checks that it succeeds and leaves no volatile
/// channel, and certifies it from its own pre-state.
async fn play_and_certify(chain: &mut NativeChain, term: &str, label: &str) -> Played {
    let played = chain.play(term, label, LIMIT).await;
    assert!(
        played.result.errors.is_empty(),
        "{label}: the deploy fails: {:?}",
        played.result.errors
    );
    assert!(
        played.residue.is_empty(),
        "{label}: the sweep leaves no volatile channel: {:?}",
        played.residue
    );
    let certification = certify(&played, term, LIMIT).await;
    assert_reproduces(label, &played, &certification);
    played
}

/// The number of produces of the COMMs of `log` that have more than one produce.
fn k_comm_sizes(log: &[Event]) -> Vec<usize> {
    log.iter()
        .filter_map(|event| match event {
            Event::Comm(comm) if comm.produces.len() > 1 => Some(comm.produces.len()),
            Event::Comm(_) | Event::IoEvent(_) => None,
        })
        .collect()
}

/// Added by decision 101 (C5.4): the COMMs of the log of `played` whose consume is on one data
/// channel of a collection, each as the hash of that channel and the produces of the COMM, in log
/// order. The data channels are the channels in the footprints of the journal rows that decode as
/// `ReifiedId::Data`.
fn data_comms(played: &Played) -> Vec<(Blake2b256Hash, Vec<Produce>)> {
    let journal = played
        .result
        .native_operation_recording
        .as_ref()
        .expect("native play records its operations");
    let data: BTreeSet<Blake2b256Hash> = journal
        .iter()
        .flat_map(footprint_channels)
        .filter(|channel| matches!(namespace::reified_id_of(channel), Some(ReifiedId::Data(..))))
        .map(|channel| hash(&channel))
        .collect();
    played
        .log
        .iter()
        .filter_map(|event| match event {
            Event::Comm(comm) => match comm.consume.channel_hashes.as_slice() {
                [channel] if data.contains(channel) => {
                    Some((channel.clone(), comm.produces.clone()))
                }
                _ => None,
            },
            Event::IoEvent(_) => None,
        })
        .collect()
}

// ─── N1 ────────────────────────────────────────────────────────────────────────────────────────

/// Deploy 1 parks a waiter for the value 2 on a name of a native bag space, and a stored consume
/// for the value 2 on a name of the reference agent. Deploy 2 sends the value 1 to both, which
/// matches neither, so both values stay stored. Deploy 3 sends the value 2 to both, which wakes
/// both. Each deploy of the block certifies from its own pre-state.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn n1_parked_wake_across_deploys_replay_equal() {
    let label = "rrs-p9-n1-wake";
    let mut chain = NativeChain::new().await;
    play_and_certify(
        &mut chain,
        &reference_agent_creation(),
        &format!("{label}-agent"),
    )
    .await;
    play_and_certify(&mut chain, KIT_NAMES, &format!("{label}-agent-names")).await;
    let native = r#"new factory(`rho:space:hashmap:bag`) in {
  for (s <- factory!?("default")) { for (n <- s!?("gensym")) { @"rrs-p9-n1-name"!(*n) } }
}"#;
    play_and_certify(&mut chain, native, &format!("{label}-native")).await;
    let both =
        |body: &str| with_kit_names(&format!("for (n <<- @\"rrs-p9-n1-name\") {{ {body} }}"));
    let park = both(&format!(
        "for (@2 <- n) {{ {} }} | for (@2 <- @x) {{ {} }}",
        out("native", "2"),
        out("agent", "2")
    ));
    let played = play_and_certify(&mut chain, &park, &format!("{label}-park")).await;
    assert_eq!(labels(&played), expect(&[]), "{label}: deploy 1 parks");
    let other = both("n!(1) | @x!(1)");
    let played = play_and_certify(&mut chain, &other, &format!("{label}-other")).await;
    assert_eq!(
        labels(&played),
        expect(&[]),
        "{label}: deploy 2 matches no waiter"
    );
    let wake = both("n!(2) | @x!(2)");
    let played = play_and_certify(&mut chain, &wake, &format!("{label}-wake")).await;
    assert_eq!(
        labels(&played),
        expect(&["agent", "native"]),
        "{label}: deploy 3 wakes both waiters"
    );
}

/// A deploy writes a record to a queue through the handle, and the write commits. Then it sends a
/// second value to a full cell, which aborts the deploy with `CellOccupied`. Native replay from
/// the pre-state reproduces the failure: the errors, a complete journal, the usage and the root
/// of play. After a user failure, Casper settles the deploy on the pre-state root
/// (`direct_wallet_funding/execution/replay.rs`, `settlement_root`), so the block continues from
/// the pre-state. The next deploy sees the pre-state: the queue holds no record, and the cell
/// holds its first value.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn n1_failed_deploy_reverts_replay_equal() {
    let label = "rrs-p9-n1-revert";
    let mut chain = NativeChain::new().await;
    let setup = r#"new fq(`rho:space:hashmap:queue`), fc(`rho:space:hashmap:cell`) in {
  for (Q <- fq!?("default"); C <- fc!?("default")) {
    for (q <- Q!?("gensym"); k <- C!?("gensym")) { @"rrs-p9-n1-revert"!(*Q, *q, *k) | k!(1) }
  }
}"#;
    play_and_certify(&mut chain, setup, &format!("{label}-setup")).await;
    let failing = r#"for (Q, q, k <<- @"rrs-p9-n1-revert") {
  for (_ <- Q!?("produce", *q, [5])) { k!(2) }
}"#;
    let played = chain
        .play(failing, &format!("{label}-failing"), LIMIT)
        .await;
    assert_eq!(
        error_codes(&played.result.errors),
        vec!["CellOccupied".to_string()],
        "{label}: the second value of the cell aborts the deploy"
    );
    assert_ne!(
        played.root, played.pre_root,
        "{label}: the deploy fails after a committed space write"
    );
    let certification = certify(&played, failing, LIMIT).await;
    assert_reproduces(&format!("{label}-failing"), &played, &certification);
    chain.reset(&played.pre_root).await;
    let next = format!(
        r#"for (Q, q, k <<- @"rrs-p9-n1-revert") {{
  for (@x <- q) {{ {} }} | for (@y <- k) {{ {} }}
}}"#,
        out("queue", "x"),
        out("cell", "y")
    );
    let played = play_and_certify(&mut chain, &next, &format!("{label}-next")).await;
    assert_eq!(
        labels(&played),
        expect(&["cell"]),
        "{label}: the next deploy sees the pre-state"
    );
}

/// A same-space join logs a COMM with more than one produce. A set retirement and a VectorDB list
/// delivery bind one member over one data channel and retire the other member with `K_retire`
/// over the same channel (decision 101, rule CR-13). So each of them logs two COMMs over that data
/// channel, each with one produce, and the two produces differ. Native replay reproduces each
/// play.
///
/// Changed by decision 101 (C5.4): a set retirement and a list delivery are one bind and
/// `K_retire` COMMs over `[D]`. The text before: "COMMs with more than one produce: a same-space
/// join, a set retirement over `[D; n]`, and a VectorDB list delivery over `[D; m]`. Each program
/// logs such a COMM, and native replay reproduces each play."
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn n1_k_comm_replay_equal() {
    let programs = [
        (
            "join",
            space_program("rho:space:hashmap:bag", r#""default""#, JOIN),
            "join",
        ),
        (
            "set retirement",
            space_program(
                "rho:space:hashmap:set",
                r#""default""#,
                r#"for (_ <- space!?("produce", *a, [1])) {
  for (_ <- space!?("produce", *a, [1])) { for (@1 <- a) { out!(["class", 1]) } }
}"#,
            ),
            "class",
        ),
        (
            "VectorDB list",
            space_program(
                "rho:space:hashmap:vectordb",
                r#""default"; capacity=16, dimensions=2"#,
                r#"for (_ <- space!?("produce", *a, [[1.0, 0.0]])) {
  for (_ <- space!?("produce", *a, [[0.6, 0.8]])) {
    for (@xs <- a ~ ([1.0, 0.0]; k=2)) { out!(["list", xs]) }
  }
}"#,
            ),
            "list",
        ),
    ];
    for (label, term, delivered) in programs {
        let mut chain = NativeChain::new().await;
        let played = play_and_certify(&mut chain, &term, label).await;
        assert_eq!(
            labels(&played),
            expect(&[delivered]),
            "{label}: the delivery"
        );
        let sizes = k_comm_sizes(&played.log);
        // Changed by decision 101 (C5.4): only the join logs a COMM with more than one produce.
        // The set retirement and the list delivery log a bind and a K_retire over one data
        // channel. The check before:
        // assert!(
        //     !sizes.is_empty(),
        //     "{label}: the log holds a COMM with more than one produce"
        // );
        match label {
            "join" => assert!(
                !sizes.is_empty(),
                "{label}: the log holds a COMM with more than one produce"
            ),
            _ => {
                assert!(
                    sizes.is_empty(),
                    "{label}: no COMM has more than one produce: {sizes:?}"
                );
                let comms = data_comms(&played);
                match comms.as_slice() {
                    [(bind_channel, bound), (retire_channel, retired)] => {
                        assert_eq!(
                            bind_channel, retire_channel,
                            "{label}: the bind and the K_retire are over one data channel"
                        );
                        match (bound.as_slice(), retired.as_slice()) {
                            ([bound], [retired]) => assert_ne!(
                                bound, retired,
                                "{label}: the bind and the K_retire take two members"
                            ),
                            other => panic!("{label}: each COMM has one produce: {other:?}"),
                        }
                    }
                    other => {
                        panic!("{label}: a bind and one K_retire over one data channel: {other:?}")
                    }
                }
            }
        }
    }
}

/// The trace binding of `played` over the log `log`: `Ok` when the binding accepts it.
async fn bind(played: &Played, log: Vec<Event>) -> Result<(), String> {
    let (recording, journal) = committed_evidence(played);
    replay_environment_from(played.history.clone(), log, LIMIT, &recording, journal)
        .await
        .map(|_| ())
}

/// The play of the join program, whose log holds a COMM with two produces.
async fn join_play() -> Played {
    let mut chain = NativeChain::new().await;
    let term = space_program("rho:space:hashmap:bag", r#""default""#, JOIN);
    play_and_certify(&mut chain, &term, "rrs-p9-n1-join").await
}

/// Negative control of the event check: the committed log of the join program with two adjacent
/// distinct events swapped fails the trace binding, and the log of play passes it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn n1_trace_binding_rejects_swapped_events() {
    let played = join_play().await;
    assert_eq!(bind(&played, played.log.clone()).await, Ok(()));
    let position = played
        .log
        .windows(2)
        .position(|pair| pair[0] != pair[1])
        .expect("the log holds two adjacent distinct events");
    let mut swapped = played.log.clone();
    swapped.swap(position, position + 1);
    let rejection = bind(&played, swapped)
        .await
        .expect_err("the trace binding rejects a swapped log");
    assert!(
        rejection.starts_with("the trace binding rejects the log"),
        "the rejection comes from the trace binding: {rejection}"
    );
}

/// Negative control of the event check: the committed log of the join program with the produce
/// list of its COMM in reverse order fails the trace binding.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn n1_trace_binding_rejects_permuted_comm() {
    let played = join_play().await;
    let mut permuted = played.log.clone();
    let comm = permuted
        .iter_mut()
        .find_map(|event| match event {
            Event::Comm(comm)
                if comm.produces.len() > 1 && comm.produces[0] != comm.produces[1] =>
            {
                Some(comm)
            }
            Event::Comm(_) | Event::IoEvent(_) => None,
        })
        .expect("the join logs a COMM with two distinct produces");
    comm.produces.reverse();
    let rejection = bind(&played, permuted)
        .await
        .expect_err("the trace binding rejects a permuted COMM");
    assert!(
        rejection.starts_with("the trace binding rejects the log"),
        "the rejection comes from the trace binding: {rejection}"
    );
}

// ─── N9 ────────────────────────────────────────────────────────────────────────────────────────

// Changed by decision 93 (decision 111): racing evaluation gives each run its own logs, roots and
// costs, so N9 certifies each run instead of comparing two runs. The old test follows.
//
// /// Two fresh native runtimes give the same canonical run: the roots, the outputs, the event
// /// logs, the evidence bytes and the phlo usage of each deploy. The deploy sets are the 24 family
// /// programs on one chain, and the conformance kit of the reference agent.
// #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
// async fn n9_run_twice_assert_canonical_equal() {
//     let families: Vec<String> = Outer::ALL
//         .into_iter()
//         .flat_map(|outer| {
//             Inner::ALL
//                 .into_iter()
//                 .map(move |inner| program(outer, inner))
//         })
//         .collect();
//     let terms: Vec<&str> = families.iter().map(String::as_str).collect();
//     run_twice_assert_canonical_equal("rrs-p9-n9-families", &terms, LIMIT).await;
//     let kit = kit_deploys();
//     let terms: Vec<&str> = kit.iter().map(String::as_str).collect();
//     run_twice_assert_canonical_equal("rrs-p9-n9-kit", &terms, LIMIT).await;
// }

/// N9 (decisions 93 and 111): three runs on each racing runtime. Each run plays the 24 family
/// programs on one chain and the conformance kit of the reference agent on another. Each deploy
/// logs events and certifies from its own pre-state. Each family program delivers a label set
/// that [`allowed_labels`] holds, and the kit delivers its outputs in order.
#[test]
fn n9_run_twice_assert_canonical_equal() {
    for workers in RACING_WORKERS {
        let runtime = racing_runtime(workers);
        for run in 0..RACING_RUNS {
            runtime.block_on(n9_run(&format!("{workers} workers, run {run}")));
        }
    }
}

/// Added by decision 93 (decision 111): the labels of `after` without one copy of each label of
/// `before`. The outputs of a chain accumulate, so these are the labels of the last deploy.
fn new_labels(before: &[String], after: &[String]) -> Vec<String> {
    let mut rest = after.to_vec();
    for label in before {
        if let Some(index) = rest.iter().position(|other| other == label) {
            rest.remove(index);
        }
    }
    rest
}

/// Added by decision 93 (decision 111): one run of N9.
async fn n9_run(run: &str) {
    let mut chain = NativeChain::new().await;
    let mut before = Vec::new();
    for (index, (outer, inner)) in Outer::ALL
        .into_iter()
        .flat_map(|outer| Inner::ALL.into_iter().map(move |inner| (outer, inner)))
        .enumerate()
    {
        let label = format!("rrs-p9-n9-families-{index}");
        let played = play_and_certify(&mut chain, &program(outer, inner), &label).await;
        assert!(
            !played.log.is_empty(),
            "{run}, {label}: the deploy logs events"
        );
        let after = labels(&played);
        let delivered = new_labels(&before, &after);
        assert!(
            allowed_labels(outer, inner, &delivered),
            "{run}, {label}: {}:{} delivers {delivered:?}",
            outer.name(),
            inner.name()
        );
        before = after;
    }
    let mut chain = NativeChain::new().await;
    let mut outputs = Vec::with_capacity(kit_deploys().len());
    for (index, term) in kit_deploys().iter().enumerate() {
        let label = format!("rrs-p9-n9-kit-{index}");
        let played = play_and_certify(&mut chain, term, &label).await;
        assert!(
            !played.log.is_empty(),
            "{run}, {label}: the deploy logs events"
        );
        outputs.push(played.out.len());
    }
    assert_eq!(
        outputs,
        [0, 0, 0, 1, 1, 2],
        "{run}: the kit delivers its outputs in order"
    );
}
