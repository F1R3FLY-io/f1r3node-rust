//! P9 step 2: N1, the pair of native play and native replay, and N9, two runs of one deploy set
//! (P9 design v2, sections 2, 4 and 7, amendments A1, A5 and A9).
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
//! | `n1_k_comm_replay_equal` | COMMs with more than one produce replay: a same-space join, a set retirement over `[D; n]` and a VectorDB list delivery. |
//! | `n1_trace_binding_rejects_swapped_events` | Negative control: the committed log with two adjacent events swapped fails the trace binding. |
//! | `n1_trace_binding_rejects_permuted_comm` | Negative control: the committed log with the produce list of a COMM permuted fails the trace binding. |
//! | `n9_run_twice_assert_canonical_equal` | Two fresh native runtimes give the same roots, outputs, logs, evidence bytes, phlo and host work for the 24 family programs and for the conformance kit of the reference agent. |
//!
//! Each program forwards each delivery to the public channel `rrs-native-out`, so the post-state
//! root witnesses the order of the deliveries. The costs of N1 are the phlo usage, the budget
//! usage, the authority events and the byte observations. N9 also compares the host work, play
//! against play, because native replay charges more host work than play (decision 79).
//!
//! The other rows of N1 are existing tests: the 24 family programs
//! (`reified_native_spaces.rs`, `space_native_replay_equal_*`), every VectorDB query shape
//! (`space_native_vectordb_replay_equal`), `temp` (`reified_temp_seq.rs`) and the `seq`
//! factories (`reified_hashset.rs`).

#[path = "support/mod.rs"]
mod support;

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{Expr, Par};
use rspace_plus_plus::rspace::trace::event::Event;
use support::family_programs::{program, space_program, Inner, Outer, JOIN};
use support::native::{
    assert_reproduces, certify, committed_evidence, host_work, replay_environment_from,
    run_twice_assert_canonical_equal, NativeChain, Played, AMPLE_LIMIT, OUT,
};
use support::session::error_codes;

/// The phlo limit of every deploy of the suite.
const LIMIT: u64 = AMPLE_LIMIT;

/// The reference agent: the example program of the repository, as the conformance kit runs it
/// (`reified_agent_conformance.rs`).
const REFERENCE_AGENT: &str = include_str!("../../examples/reified_rspaces/agent_hashmap_bag.rho");

/// The comment that starts the demo of the reference agent.
const DEMO_MARKER: &str = "  // Demo:";

/// The deploy that creates one instance of the reference agent and publishes it on
/// `@"kit-agent"`, as the conformance kit does.
fn reference_agent_creation() -> String {
    let (definitions, _) = REFERENCE_AGENT
        .split_once(DEMO_MARKER)
        .expect("the reference agent marks its demo");
    format!(
        "{definitions}  for (space <- agentHashmapBag!?(\"default\")) {{ @\"kit-agent\"!(*space) }}\n}}\n"
    )
}

/// The deploy that draws the names `x` and `y` from the agent space and publishes them on
/// `@"kit-names"`, as the conformance kit does.
const KIT_NAMES: &str = r#"new names in {
  for (space <<- @"kit-agent") { use space { new x, y in { names!([*x, *y]) } } } |
  for (@ns <- names) { @"kit-names"!(ns) }
}"#;

/// `body` with the names of the agent space bound to `x` and `y`.
fn with_kit_names(body: &str) -> String {
    format!("for (@[x, y] <<- @\"kit-names\") {{\n{body}\n}}")
}

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

/// COMMs with more than one produce: a same-space join, a set retirement over `[D; n]`, and a
/// VectorDB list delivery over `[D; m]`. Each program logs such a COMM, and native replay
/// reproduces each play.
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
        assert!(
            !sizes.is_empty(),
            "{label}: the log holds a COMM with more than one produce"
        );
    }
}

/// The trace binding of `played` over the log `log`: `Ok` when the binding accepts it.
async fn bind(played: &Played, log: Vec<Event>) -> Result<(), String> {
    let (recording, journal) = committed_evidence(played);
    replay_environment_from(
        played.history.clone(),
        log,
        LIMIT,
        &recording,
        journal,
        host_work(),
    )
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

/// Two fresh native runtimes give the same canonical run: the roots, the outputs, the event
/// logs, the evidence bytes, the phlo usage and the host work of each deploy. The deploy sets
/// are the 24 family programs on one chain, and the conformance kit of the reference agent.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn n9_run_twice_assert_canonical_equal() {
    let families: Vec<String> = Outer::ALL
        .into_iter()
        .flat_map(|outer| {
            Inner::ALL
                .into_iter()
                .map(move |inner| program(outer, inner))
        })
        .collect();
    let terms: Vec<&str> = families.iter().map(String::as_str).collect();
    run_twice_assert_canonical_equal("rrs-p9-n9-families", &terms, LIMIT).await;
    let kit = [
        reference_agent_creation(),
        KIT_NAMES.to_string(),
        with_kit_names("@x!(1, 2)"),
        with_kit_names(&format!("for (@a, @b <- @x) {{ @\"{OUT}\"!([a, b]) }}")),
        with_kit_names(&format!("for (@a <<- @y) {{ @\"{OUT}\"!(a) }}")),
        with_kit_names("@y!(3)"),
    ];
    let terms: Vec<&str> = kit.iter().map(String::as_str).collect();
    run_twice_assert_canonical_equal("rrs-p9-n9-kit", &terms, LIMIT).await;
}
