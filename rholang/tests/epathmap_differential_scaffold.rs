//! EPathMap fix P0 — FUSED-VS-UNFUSED DIFFERENTIAL SCAFFOLD (test-only).
//!
//! The harness STRUCTURE that later phases' differentials plug into,
//! committed now so the parity methodology is fixed before any production
//! change exists. THE P2 CONSUMER: when P2 lands
//! `try_eval_fused_method_chain` (the method-chain view-fusion seam called
//! first in both dispatch arms), it extends [`QueryRunMode`] with
//! `FusedDisabled` / `Fused` variants — gated by a COMPILE-TIME
//! (`cfg`/feature) force-disable flag on the recognizer, never a runtime
//! flip (a runtime-flippable path is a node-divergence hazard under a latent
//! parity bug; plan amendment PM-5(3)) — and asserts
//! `observe(Fused, …) == observe(FusedDisabled, …)` field-for-field over
//! every chain shape and edge program below. P4's spliced event hashing
//! reuses the same observation (its `produce_hash` field is the
//! spliced-vs-direct gate).
//!
//! At P0 the only mode is [`QueryRunMode::TodayPath`]; the suite exercises
//! the harness by asserting the today-path observation is DETERMINISTIC
//! (two fresh runtimes, byte-identical observations) — the property every
//! later differential relies on to attribute any mismatch to the treatment
//! rather than to ambient nondeterminism.
//!
//! The programs mirror `epathmap_charge_trace_spec.rs` (kept self-contained
//! per the suite's house style of per-file fixtures; if you change a program
//! here, change its twin there).

use std::collections::HashMap;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{Expr, Par};
use prost::Message;
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::accounting::has_cost::HasCost;
use rholang::rust::interpreter::accounting::BillableKind;
use rholang::rust::interpreter::rho_runtime::{RhoRuntime, RhoRuntimeImpl};
use rholang::rust::interpreter::test_utils::resources::with_runtime;

// ─────────────────────────────────────────────────────────────────────────────
// Run modes
// ─────────────────────────────────────────────────────────────────────────────

/// Which evaluation path an observation runs. P0 ships only the today-path;
/// P2 adds `FusedDisabled` (recognizer force-disabled at compile time) and
/// `Fused` (the production fused path) — see the module docs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum QueryRunMode {
    /// The unmodified 84a0fbe4 evaluation path.
    TodayPath,
    // P2 (do not add before the seam exists):
    // FusedDisabled,
    // Fused,
}

// ─────────────────────────────────────────────────────────────────────────────
// The observation — every parity-relevant observable of one evaluation
// ─────────────────────────────────────────────────────────────────────────────

/// One channel's readback, at full byte fidelity.
#[derive(Clone, Debug, PartialEq, Eq)]
struct ChannelObservation {
    channel: String,
    /// PROST bytes of each datum Par (the value-domain observable).
    par_bytes: Vec<Vec<u8>>,
    /// The datum's `random_state` (deterministic under the fixed rand).
    random_state: Vec<Vec<u8>>,
    persist: Vec<bool>,
    /// The produce EVENT HASH of each datum (`Datum::source.hash`) — the
    /// consensus-side observable P4's spliced hashing must reproduce.
    produce_hash: Vec<Vec<u8>>,
}

/// Everything a fused-vs-unfused differential compares.
#[derive(Clone, Debug, PartialEq, Eq)]
struct QueryObservation {
    /// Interpreter errors, rendered `{:?}` (pins variants AND payloads —
    /// e.g. the exact `"Error: Multiple expressions given."` string).
    errors: Vec<String>,
    /// Consensus consumed total (`EvaluateResult::cost.value`).
    consumed: i64,
    /// The rendered canonical charge trace (kind, operation, weight, order —
    /// same rendering as `epathmap_charge_trace_spec.rs`).
    charge_trace: Vec<String>,
    /// Per-channel readbacks, in the caller-given channel order.
    channels: Vec<ChannelObservation>,
}

fn render_charge(kind: &BillableKind, weight: u64) -> String {
    match kind {
        BillableKind::Primitive(operation) => {
            let class = if operation.ends_with(" union cost") {
                "incr-prim"
            } else {
                "prim"
            };
            format!("{class}({operation})={weight}")
        }
        BillableKind::Comm => format!("comm={weight}"),
        BillableKind::Reduction => format!("reduction={weight}"),
        BillableKind::Substitution => format!("subst={weight}"),
    }
}

/// The fixed evaluation seed (`create_from_length` draws from
/// `rand::thread_rng()` and would make `random_state`/`produce_hash`
/// nondeterministic — the captured 84a0fbe4 truth this harness works around
/// by seeding explicitly).
fn fixed_rand() -> Blake2b512Random {
    Blake2b512Random::create_from_bytes(&[
        0xde, 0xad, 0xbe, 0xef, 0xca, 0xfe, 0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08,
        0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17,
        0x18, 0x19,
    ])
}

/// Run `program` on a fresh runtime under `mode` with `initial_phlo`, then
/// read back `channels`. THE differential entry point.
async fn observe(
    mode: QueryRunMode,
    prefix: &str,
    program: &str,
    channels: &[&str],
    initial_phlo: Cost,
) -> QueryObservation {
    // Mode dispatch: P2 inserts the force-disable toggle here (compile-time
    // gated); the today-path arm stays byte-identical.
    match mode {
        QueryRunMode::TodayPath => {}
    }

    let program = program.to_string();
    let channels: Vec<String> = channels.iter().map(|c| c.to_string()).collect();
    with_runtime(prefix, move |runtime: RhoRuntimeImpl| async move {
        let res = runtime
            .evaluate(&program, initial_phlo, HashMap::new(), fixed_rand())
            .await
            .expect("evaluate must not fail structurally");

        let charge_trace = runtime
            .cost()
            .get_canonical_event_log()
            .iter()
            .map(|event| render_charge(&event.kind, event.weight))
            .collect::<Vec<_>>();

        let mut channel_observations = Vec::with_capacity(channels.len());
        for channel_name in &channels {
            let channel = Par::default().with_exprs(vec![Expr {
                expr_instance: Some(ExprInstance::GString(channel_name.clone())),
            }]);
            let data = runtime.get_data(&channel).await;
            channel_observations.push(ChannelObservation {
                channel: channel_name.clone(),
                par_bytes: data
                    .iter()
                    .map(|datum| {
                        datum
                            .a
                            .pars
                            .iter()
                            .flat_map(|par| par.encode_to_vec())
                            .collect()
                    })
                    .collect(),
                random_state: data.iter().map(|datum| datum.a.random_state.clone()).collect(),
                persist: data.iter().map(|datum| datum.persist).collect(),
                produce_hash: data
                    .iter()
                    .map(|datum| datum.source.hash.bytes().to_vec())
                    .collect(),
            });
        }

        QueryObservation {
            errors: res.errors.iter().map(|error| format!("{error:?}")).collect(),
            consumed: res.cost.value,
            charge_trace,
            channels: channel_observations,
        }
    })
    .await
}

// ─────────────────────────────────────────────────────────────────────────────
// The differential program set (mirrors epathmap_charge_trace_spec.rs)
// ─────────────────────────────────────────────────────────────────────────────

const INDEX_MAP: &str = r#"{|
    ["t.deadbeef.Pair", "site0"],
    ["v", "site0", ("Pair",)],
    ["t.deadbeef.A", "site0", "Pair.0"],
    ["v", "site0", "Pair.0", ("A",)],
    ["t.deadbeef.B", "site0", "Pair.1"],
    ["v", "site0", "Pair.1", ("B",)]
|}"#;

fn e6a_program(result_channel: &str, chain: &str) -> String {
    format!(
        r#"
        @"e6a:idx:site0"!!({INDEX_MAP}) |
        for( @idx <- @"e6a:idx:site0" ) {{
            @"{result_channel}"!( {chain} )
        }}
        "#
    )
}

/// Every program the P2/P4 differentials must cover: the four E-6a chain
/// shapes plus the PM-4(d) edge programs (Nil-mid-chain in all four Nil
/// sources; Nil + wrong arity). Each row: (label, program, readback
/// channels).
fn differential_programs() -> Vec<(&'static str, String, Vec<&'static str>)> {
    vec![
        (
            "discovery",
            e6a_program(
                "e6a:sites:site0/Pair",
                r#"idx.readZipperAt(["t.deadbeef.Pair"]).getSubtrie()"#,
            ),
            vec!["e6a:sites:site0/Pair", "e6a:idx:site0"],
        ),
        (
            "tag-guard",
            e6a_program(
                "out",
                r#"idx.readZipperAt(["t.deadbeef.A", "site0", "Pair.0"]).pathExists()"#,
            ),
            vec!["out", "e6a:idx:site0"],
        ),
        (
            "sigma-exists",
            e6a_program(
                "out",
                r#"idx.readZipperAt(["v", "site0", "Pair.0"]).pathExists()"#,
            ),
            vec!["out", "e6a:idx:site0"],
        ),
        (
            "sigma-chain",
            e6a_program(
                "out",
                r#"idx.readZipperAt(["v", "site0", "Pair.0"]).descendFirst().getLeaf()"#,
            ),
            vec!["out", "e6a:idx:site0"],
        ),
        (
            "nil-getLeaf-no-value",
            r#"@"nil"!( {| ["a", "x"] |}.readZipperAt(["a"]).getLeaf().pathExists() )"#.to_string(),
            vec!["nil"],
        ),
        (
            "nil-descendFirst-no-children",
            r#"@"nil"!( {| ["a"] |}.readZipperAt(["a"]).descendFirst().getLeaf() )"#.to_string(),
            vec!["nil"],
        ),
        (
            "nil-ascendOne-at-root",
            r#"@"nil"!( {| ["a"] |}.readZipper().ascendOne().getLeaf() )"#.to_string(),
            vec!["nil"],
        ),
        (
            "nil-descendIndexedBranch-negative",
            r#"@"nil"!( {| ["a"] |}.readZipper().descendIndexedBranch(-1).getLeaf() )"#.to_string(),
            vec!["nil"],
        ),
        (
            "nil-wrong-arity",
            r#"@"nil"!( {| ["a", "x"] |}.readZipperAt(["a"]).getLeaf().getLeaf("extra") )"#
                .to_string(),
            vec!["nil"],
        ),
    ]
}

// ─────────────────────────────────────────────────────────────────────────────
// P0 exercise: the today-path observation is deterministic
// ─────────────────────────────────────────────────────────────────────────────

/// Two fresh runtimes, identical observations — errors, consumed, charge
/// trace, result bytes, random_state, persist flags, produce hashes. This is
/// the null-differential every later fused-vs-unfused comparison stands on.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn today_path_observations_are_deterministic() {
    for (label, program, channels) in differential_programs() {
        let first = observe(
            QueryRunMode::TodayPath,
            &format!("epm-p0-diff-a-{label}-"),
            &program,
            &channels,
            Cost::unsafe_max(),
        )
        .await;
        let second = observe(
            QueryRunMode::TodayPath,
            &format!("epm-p0-diff-b-{label}-"),
            &program,
            &channels,
            Cost::unsafe_max(),
        )
        .await;
        assert_eq!(
            first, second,
            "{label}: the today-path observation must be byte-deterministic"
        );
    }
}

/// The bounded-budget variant of the null differential — the k-axis the P2
/// exhaustion-at-index-k differential walks.
///
/// ★ CAPTURED SCHEDULE-DEPENDENCE (see
/// `epathmap_charge_trace_spec::budget_exhaustion_walks_comm_boundaries`):
/// at ks where parallel branches race for the last token, the losing branch
/// ABORTS and truncates the attempt multiset, so the diagnostic committed
/// charge trace — and, at k=1, even which side's produce reached the space —
/// is schedule-dependent. The deterministic projection at those ks is
/// (errors, consumed). The scaffold therefore asserts FULL observation
/// equality at k∈{0,3,4} (empty commit / complete runs) and the projection
/// at k∈{1,2} (k=2 has shown no variance but is classed conservatively with
/// k=1 pending a mechanism proof). P2/P4 exhaustion differentials inherit
/// exactly this contract.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn today_path_bounded_budget_observations_are_deterministic() {
    let (label, program, channels) = &differential_programs()[3]; // sigma-chain
    for k in 0..=4i64 {
        let first = observe(
            QueryRunMode::TodayPath,
            &format!("epm-p0-diffk-a-{label}-{k}-"),
            program,
            channels,
            Cost::create(k, "differential budget"),
        )
        .await;
        let second = observe(
            QueryRunMode::TodayPath,
            &format!("epm-p0-diffk-b-{label}-{k}-"),
            program,
            channels,
            Cost::create(k, "differential budget"),
        )
        .await;
        if matches!(k, 1 | 2) {
            assert_eq!(
                first.errors, second.errors,
                "{label} k={k}: exhaustion errors must be deterministic"
            );
            assert_eq!(
                first.consumed, second.consumed,
                "{label} k={k}: consumed total must be deterministic"
            );
        } else {
            assert_eq!(
                first, second,
                "{label} k={k}: bounded-budget observation must be byte-deterministic"
            );
        }
    }
}
