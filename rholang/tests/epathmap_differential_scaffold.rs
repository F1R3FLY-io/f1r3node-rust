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
//! P2 STATUS: the seam exists (`try_eval_fused_method_chain`,
//! interpreter/fused_pathmap_chain.rs) and [`QueryRunMode`] now carries the
//! `FusedDisabled`/`Fused` variants under the `epathmap-fusion-differential`
//! feature — the PM-5(3) compile-time gate (`cargo test -p rholang
//! --features epathmap-fusion-differential --test
//! epathmap_differential_scaffold`). Without the feature this file compiles
//! to exactly the P0 suite: [`QueryRunMode::TodayPath`] — which POST-P2 IS
//! the fused production path — plus the two determinism tests, the property
//! every differential relies on to attribute any mismatch to the treatment
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

/// Which evaluation path an observation runs. `TodayPath` is whatever
/// production does (post-P2: the fused seam active); the P2 differential
/// modes exist only under the PM-5(3) compile-time feature gate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum QueryRunMode {
    /// The production evaluation path (post-P2: fusion active).
    TodayPath,
    /// The recognizer force-disabled via the test-only toggle — every chain
    /// takes the per-link fallback (the pre-P2 path, bit for bit).
    #[cfg(feature = "epathmap-fusion-differential")]
    FusedDisabled,
    /// The fused path, explicitly (identical to `TodayPath`; named for the
    /// differential's readability).
    #[cfg(feature = "epathmap-fusion-differential")]
    Fused,
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
    // Mode dispatch (P2): the force-disable toggle, compile-time gated per
    // PM-5(3). The guard restores the production default (fusion active) on
    // drop, even across panics.
    #[cfg(feature = "epathmap-fusion-differential")]
    let _toggle_guard = {
        use rholang::rust::interpreter::fused_pathmap_chain::fusion_test_support;
        struct ToggleGuard;
        impl Drop for ToggleGuard {
            fn drop(&mut self) {
                fusion_test_support::set_force_disabled(false);
            }
        }
        fusion_test_support::set_force_disabled(matches!(mode, QueryRunMode::FusedDisabled));
        ToggleGuard
    };
    #[cfg(not(feature = "epathmap-fusion-differential"))]
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
    // Under the P2 differential feature every test in this binary serializes
    // on the toggle lock (the toggle + hit counters are process-global).
    #[cfg(feature = "epathmap-fusion-differential")]
    let _serial = fused_differentials::serialize_toggle_tests();
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
    // Under the P2 differential feature every test in this binary serializes
    // on the toggle lock (the toggle + hit counters are process-global).
    #[cfg(feature = "epathmap-fusion-differential")]
    let _serial = fused_differentials::serialize_toggle_tests();
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

// ─────────────────────────────────────────────────────────────────────────────
// P2: the fused-vs-unfused differential suite (feature-gated per PM-5(3))
// ─────────────────────────────────────────────────────────────────────────────

/// The P2 differential proper: for every fusable shape and every edge, the
/// FUSED observation must equal the FORCE-DISABLED (per-link fallback)
/// observation field-for-field — result Par bytes, datum `random_state`,
/// persist flags, produce event hashes, interpreter errors (variant AND
/// payload strings), the consensus consumed total, and the full canonical
/// charge trace (reservation kind, operation, weight, ORDER). Each row also
/// pins its fusion-hit accounting: rows expected to fuse must record ≥1 hit
/// in Fused mode; rows expected to fall back must record EXACTLY 0; the
/// force-disabled run must always record 0.
#[cfg(feature = "epathmap-fusion-differential")]
mod fused_differentials {
    use std::sync::Mutex;

    use rholang::rust::interpreter::fused_pathmap_chain::fusion_test_support;

    use super::*;

    /// Serializes every test in this binary under the feature: the
    /// force-disable toggle and the hit counters are process-global, so a
    /// concurrently-running P0 determinism test (whose E-6a programs fuse)
    /// would smear another test's hit delta — and a `TodayPath` observation
    /// taken inside a force-disabled window, while byte-equivalent (that IS
    /// the differential claim), would turn a parity bug into flakiness
    /// instead of a clean failure. The P0 tests take this lock too
    /// (feature-gated; without the feature they run exactly as at P0).
    pub(super) static DIFFERENTIAL_LOCK: Mutex<()> = Mutex::new(());

    /// Lock, de-poisoned: each test asserts independently, so an earlier
    /// test's panic (which poisons the mutex) must not cascade into
    /// unrelated poison panics that mask the real failure.
    pub(super) fn serialize_toggle_tests() -> std::sync::MutexGuard<'static, ()> {
        DIFFERENTIAL_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Observe under `mode` and return the observation plus the fusion-hit
    /// DELTA across the whole run (runtime bootstrap included — bootstrap
    /// code contains no PathMap chains, and any future violation surfaces
    /// loudly as a nonzero delta on an expect-zero row).
    async fn observe_with_hits(
        mode: QueryRunMode,
        prefix: &str,
        program: &str,
        channels: &[&str],
        initial_phlo: Cost,
    ) -> (QueryObservation, u64) {
        let before = fusion_test_support::total_fusion_hits();
        let observation = observe(mode, prefix, program, channels, initial_phlo).await;
        let delta = fusion_test_support::total_fusion_hits() - before;
        (observation, delta)
    }

    /// One matrix row: label, program, readback channels, and whether the
    /// FUSED run must hit the fused evaluator (`expect_fusion`). Rows with
    /// `expect_fusion = false` prove the recognizer DECLINES (name gate,
    /// arity gate, spine break, eval_stable gate, non-map base) while the
    /// observations stay byte-identical through the fallback.
    ///
    /// NOTE on partial chains: a non-fusable OUTER link (wrong arity, write
    /// method, value-producer mid-chain) makes the WHOLE chain fall back, but
    /// the fallback's per-link recursion re-enters the seam on the INNER
    /// spine, which may legitimately fuse as its own shorter chain — those
    /// rows carry `expect_fusion = true` with the sub-chain documented.
    struct MatrixRow {
        label: &'static str,
        program: String,
        channels: Vec<&'static str>,
        expect_fusion: bool,
        /// Exact number of interpreter errors the run must produce (0 for
        /// success rows) — guards every row against passing VACUOUSLY on an
        /// unintended failure (e.g. a program that fails to parse would
        /// error identically in both modes and satisfy the equality check
        /// without exercising anything).
        expected_error_count: usize,
        /// Distinctive fragments that must appear in the rendered errors
        /// (`{:?}`) — pins the error FAMILY without over-pinning the full
        /// debug layout.
        error_fragments: Vec<&'static str>,
    }

    fn row(
        label: &'static str,
        program: impl Into<String>,
        channels: Vec<&'static str>,
        expect_fusion: bool,
    ) -> MatrixRow {
        MatrixRow {
            label,
            program: program.into(),
            channels,
            expect_fusion,
            expected_error_count: 0,
            error_fragments: Vec::new(),
        }
    }

    fn error_row(
        label: &'static str,
        program: impl Into<String>,
        channels: Vec<&'static str>,
        expect_fusion: bool,
        error_fragments: Vec<&'static str>,
    ) -> MatrixRow {
        MatrixRow {
            label,
            program: program.into(),
            channels,
            expect_fusion,
            expected_error_count: 1,
            error_fragments,
        }
    }

    /// THE MATRIX. Every fusable link appears in at least one fusing row;
    /// every recognizer-decline reason appears in at least one zero-hit row;
    /// all four PM-4(d) Nil sources and both in-fusion error families
    /// (`MethodNotDefined`, argument-extraction) are exercised.
    fn differential_matrix() -> Vec<MatrixRow> {
        let mut rows = Vec::with_capacity(32);

        // ── the four E-6a treatment shapes (var-map base) ────────────────
        for (label, program, channels) in differential_programs().into_iter().take(4) {
            rows.push(row(label, program, channels, true));
        }

        // ── singles: every value producer + both zipper creators ─────────
        rows.push(row(
            "single-readZipper-zipper-terminal",
            r#"@"out"!( {| ["a"], ["a", "b"] |}.readZipper() )"#,
            vec!["out"],
            true,
        ));
        rows.push(row(
            "single-readZipperAt-zipper-terminal",
            r#"@"out"!( {| ["a"], ["a", "b"] |}.readZipperAt(["a"]) )"#,
            vec!["out"],
            true,
        ));
        rows.push(row(
            "map-pathExists",
            r#"@"out"!( {| ["a"] |}.pathExists() )"#,
            vec!["out"],
            true,
        ));
        rows.push(row(
            // The raw-map ROOT getLeaf variant: an empty-list entry keys the
            // trie ROOT, so the root carries a value.
            "map-getLeaf-root-value",
            r#"@"out"!( {| [] |}.getLeaf() )"#,
            vec!["out"],
            true,
        ));
        rows.push(row(
            // Root has NO value: a terminal Nil datum.
            "map-getLeaf-root-nil",
            r#"@"out"!( {| ["a"] |}.getLeaf() )"#,
            vec!["out"],
            true,
        ));
        rows.push(row(
            "map-childCount",
            r#"@"out"!( {| ["a"], ["b"], ["a", "c"] |}.childCount() )"#,
            vec!["out"],
            true,
        ));
        rows.push(row(
            "map-atPath",
            r#"@"out"!( {| ["a", "x"] |}.atPath(["a", "x"]) )"#,
            vec!["out"],
            true,
        ));
        rows.push(row(
            "map-atPath-missing-path",
            r#"@"out"!( {| ["a", "x"] |}.atPath(["zzz"]) )"#,
            vec!["out"],
            true,
        ));
        rows.push(row(
            "map-getSubtrie-whole-map",
            r#"@"out"!( {| ["a"], ["b"] |}.getSubtrie() )"#,
            vec!["out"],
            true,
        ));

        // ── deep chains over the E-6a index ──────────────────────────────
        rows.push(row(
            // 7 links: readZipperAt → descendTo → ascendOne → reset →
            // descendFirst → toNextSibling → getSubtrie.
            "deep-chain-nav-mix",
            e6a_program(
                "out",
                r#"idx.readZipperAt(["t.deadbeef.Pair"]).descendTo(["site0"]).ascendOne().reset().descendFirst().toNextSibling().getSubtrie()"#,
            ),
            vec!["out", "e6a:idx:site0"],
            true,
        ));
        rows.push(row(
            "deep-chain-indexed-ascend",
            e6a_program(
                "out",
                r#"idx.readZipperAt(["v", "site0"]).descendIndexedBranch(0).ascend(1).pathExists()"#,
            ),
            vec!["out", "e6a:idx:site0"],
            true,
        ));
        rows.push(row(
            "deep-chain-prev-sibling",
            e6a_program(
                "out",
                r#"idx.readZipperAt([]).descendIndexedBranch(1).toPrevSibling().getSubtrie()"#,
            ),
            vec!["out", "e6a:idx:site0"],
            true,
        ));
        rows.push(row(
            "zipper-terminal-deep",
            e6a_program("out", r#"idx.readZipperAt(["v"]).descendTo(["site0"])"#),
            vec!["out", "e6a:idx:site0"],
            true,
        ));

        // ── edges: root path, missing path, terminal Nil ─────────────────
        rows.push(row(
            "root-path-pathExists",
            e6a_program("out", r#"idx.readZipperAt([]).pathExists()"#),
            vec!["out", "e6a:idx:site0"],
            true,
        ));
        rows.push(row(
            "missing-path-pathExists",
            r#"@"out"!( {| ["a"] |}.readZipperAt(["zzz"]).pathExists() )"#,
            vec!["out"],
            true,
        ));
        rows.push(row(
            // The EMPTY map literal: ps = [] is vacuously eval-stable; the
            // root does not exist (`!ps.is_empty()` = false).
            "empty-map-pathExists",
            r#"@"out"!( {| |}.pathExists() )"#,
            vec!["out"],
            true,
        ));
        rows.push(row(
            "empty-map-childCount",
            r#"@"out"!( {| |}.readZipper().childCount() )"#,
            vec!["out"],
            true,
        ));
        rows.push(row(
            "terminal-nil-ascendOne",
            r#"@"out"!( {| ["a"] |}.readZipper().ascendOne() )"#,
            vec!["out"],
            true,
        ));
        rows.push(row(
            // descendTo performs NO existence check: a phantom focus
            // materializes into the zipper's current_path byte-identically.
            "zipper-terminal-phantom-descendTo",
            r#"@"out"!( {| ["a"] |}.readZipper().descendTo(["zzz"]) )"#,
            vec!["out"],
            true,
        ));
        rows.push(error_row(
            // The sibling current-segment-not-found arm ("shouldn't happen"
            // ⇒ Nil): a phantom focus is not among its parent's actual
            // children, so toNextSibling yields Nil and the follow-on
            // getLeaf raises the Nil-mid-chain error — in-fusion.
            "sibling-of-phantom-focus-nil",
            r#"@"out"!( {| ["a"] |}.readZipperAt(["zzz"]).toNextSibling().getLeaf() )"#,
            vec!["out"],
            true,
            vec!["ReduceError", "Error: Multiple expressions given."],
        ));

        // ── the PM-4(d) Nil sources + follow-on link ─────────────────────
        // getLeaf-no-value is a MID-CHAIN value producer: the whole chain
        // falls back, but the fallback's inner spine
        // `readZipperAt(["a"]).getLeaf()` fuses as its own chain (hits ≥1).
        rows.push(error_row(
            "nil-getLeaf-no-value-then-pathExists",
            r#"@"nil"!( {| ["a", "x"] |}.readZipperAt(["a"]).getLeaf().pathExists() )"#,
            vec!["nil"],
            true,
            vec!["ReduceError", "Error: Multiple expressions given."],
        ));
        rows.push(error_row(
            "nil-descendFirst-no-children-then-getLeaf",
            r#"@"nil"!( {| ["a"] |}.readZipperAt(["a"]).descendFirst().getLeaf() )"#,
            vec!["nil"],
            true,
            vec!["ReduceError", "Error: Multiple expressions given."],
        ));
        rows.push(error_row(
            "nil-ascendOne-at-root-then-getLeaf",
            r#"@"nil"!( {| ["a"] |}.readZipper().ascendOne().getLeaf() )"#,
            vec!["nil"],
            true,
            vec!["ReduceError", "Error: Multiple expressions given."],
        ));
        rows.push(error_row(
            "nil-descendIndexedBranch-negative-then-getLeaf",
            r#"@"nil"!( {| ["a"] |}.readZipper().descendIndexedBranch(-1).getLeaf() )"#,
            vec!["nil"],
            true,
            vec!["ReduceError", "Error: Multiple expressions given."],
        ));

        // ── in-fusion error families beyond Nil ──────────────────────────
        rows.push(error_row(
            // MethodNotDefined("ascend (requires integer argument)",
            // "non-integer") AFTER the union constant — in-fusion.
            "ascend-non-integer-arg",
            r#"@"out"!( {| ["a"] |}.readZipper().ascend("x") )"#,
            vec!["out"],
            true,
            vec!["MethodNotDefined", "ascend (requires integer argument)", "non-integer"],
        ));
        rows.push(error_row(
            // MethodNotDefined("ascend (steps must be non-negative)",
            // "negative: -1") — in-fusion.
            "ascend-negative-arg",
            r#"@"out"!( {| ["a"] |}.readZipper().ascend(-1) )"#,
            vec!["out"],
            true,
            vec!["MethodNotDefined", "ascend (steps must be non-negative)", "negative: -1"],
        ));
        rows.push(error_row(
            // MethodNotDefined("descendTo", "pathmap") — a zipper-only link
            // on a map-mode view, in-fusion.
            "descendTo-on-map",
            r#"@"out"!( {| ["a"] |}.descendTo(["a"]) )"#,
            vec!["out"],
            true,
            vec!["MethodNotDefined", "descendTo", "pathmap"],
        ));
        rows.push(error_row(
            // MethodNotDefined("readZipper", "zipper") — a map-only link on
            // a zipper-mode view, in-fusion.
            "readZipper-on-zipper",
            r#"@"out"!( {| ["a"] |}.readZipper().readZipper() )"#,
            vec!["out"],
            true,
            vec!["MethodNotDefined", "readZipper", "zipper"],
        ));

        // ── var-zipper base (a zipper VALUE bound through the tuplespace) ─
        rows.push(row(
            "var-zipper-base",
            r#"
            @"z"!( {| ["a"], ["a", "b"] |}.readZipperAt(["a"]) ) |
            for( @z <- @"z" ) { @"out"!( z.descendFirst().getLeaf() ) }
            "#,
            vec!["out"],
            true,
        ));

        // ── recognizer-decline rows (expect ZERO hits) ───────────────────
        rows.push(error_row(
            // Wrong arity on the INNERMOST link: the outer chain declines on
            // the spine walk, and the inner seam declines on the arity gate,
            // so NOTHING fuses; the fallback raises the exact
            // MethodArgumentNumberMismatch { readZipper, 0, 1 }.
            "arity-mismatch-innermost",
            r#"@"out"!( {| ["a"] |}.readZipper("extra").pathExists() )"#,
            vec!["out"],
            false,
            vec!["MethodArgumentNumberMismatch", "readZipper"],
        ));
        rows.push(error_row(
            // Wrong arity on the OUTERMOST link: the whole chain declines,
            // but the inner spine `readZipperAt(["a"]).getLeaf()` fuses as
            // its own chain before the fallback raises
            // MethodArgumentNumberMismatch { getLeaf, 0, 1 }.
            "arity-mismatch-outermost",
            r#"@"nil"!( {| ["a", "x"] |}.readZipperAt(["a"]).getLeaf().getLeaf("extra") )"#,
            vec!["nil"],
            true,
            vec!["MethodArgumentNumberMismatch", "getLeaf"],
        ));
        rows.push(row(
            // A BoundVar inside the map literal: eval_stable = false (the
            // re-evaluation substitutes it and charges var_eval inside the
            // map re-eval) — the gate declines and the fallback preserves
            // those charges.
            "non-eval-stable-map-falls-back",
            r#"
            @"seed"!("s") |
            for( @x <- @"seed" ) { @"out"!( {| ["k", x] |}.pathExists() ) }
            "#,
            vec!["out"],
            false,
        ));
        rows.push(error_row(
            // An EVar base bound to a NON-map value: the recognizer declines
            // on the base shape; the fallback raises
            // MethodNotDefined { pathExists, "int" }.
            "evar-base-non-map",
            r#"
            @"seed"!(42) |
            for( @x <- @"seed" ) { @"out"!( x.pathExists() ) }
            "#,
            vec!["out"],
            false,
            vec!["MethodNotDefined", "pathExists", "int"],
        ));
        rows.push(row(
            // A WRITE method in the spine: write methods never fuse; the
            // inner literal base below writeZipperAt is not a chain at all,
            // so nothing fuses anywhere.
            "write-method-never-fuses",
            r#"@"out"!( {| ["a"] |}.writeZipperAt(["a"]).getLeaf() )"#,
            vec!["out"],
            false,
        ));

        // ── the expr-arm (guard) dispatch route — PM-4(a) ────────────────
        rows.push(row(
            "guard-pathExists-true-branch",
            r#"if( {| ["a"] |}.pathExists() ) { @"out"!("yes") } else { @"out"!("no") }"#,
            vec!["out"],
            true,
        ));
        rows.push(error_row(
            // A Nil chain result as an `if` CONDITION: eval_if evaluates the
            // condition via eval_expr (the PAR-arm seam), substitutes, then
            // extract_bool rejects the Nil par — IfConditionTypeError, in
            // both modes.
            "if-condition-nil-chain",
            r#"if( {| ["a"] |}.readZipper().ascendOne() ) { @"out"!("yes") } else { @"out"!("no") }"#,
            vec!["out"],
            true,
            vec!["IfConditionTypeError", "non-boolean process"],
        ));
        rows.push(error_row(
            // A Nil chain result as an EAnd CONJUNCT — the true expr-arm
            // route (eval_to_bool → eval_expr_to_expr → the seam): PM-4(a)'s
            // fused Some(par) goes through the SAME eval_single_expr
            // conversion today's arm applies to its result_par, raising the
            // identical "Error: Multiple expressions given.".
            "guard-and-conjunct-nil-chain",
            r#"@"out"!( true and {| ["a"] |}.readZipper().ascendOne() )"#,
            vec!["out"],
            true,
            vec!["ReduceError", "Error: Multiple expressions given."],
        ));
        rows.push(row(
            // The EAnd conjunct route (eval_to_bool → eval_expr_to_expr).
            "guard-and-conjunct",
            r#"@"out"!( true and {| ["a"] |}.pathExists() )"#,
            vec!["out"],
            true,
        ));

        rows
    }

    /// Run one row under both modes and assert the full differential
    /// contract. Returns the Fused-mode hit delta for shape accounting.
    async fn assert_row_differential(row: &MatrixRow) -> u64 {
        let (unfused, unfused_hits) = observe_with_hits(
            QueryRunMode::FusedDisabled,
            &format!("epm-p2-fd-{}-", row.label),
            &row.program,
            &row.channels,
            Cost::unsafe_max(),
        )
        .await;
        let (fused, fused_hits) = observe_with_hits(
            QueryRunMode::Fused,
            &format!("epm-p2-f-{}-", row.label),
            &row.program,
            &row.channels,
            Cost::unsafe_max(),
        )
        .await;

        assert_eq!(
            fused, unfused,
            "{}: the fused observation must equal the force-disabled observation \
             field-for-field (result bytes, random_state, persist, produce hashes, \
             errors, consumed, charge trace)",
            row.label
        );
        // Vacuousness guard: the row must have evaluated as INTENDED (a
        // program that failed some other way — e.g. at parse — would error
        // identically in both modes and satisfy the equality vacuously).
        assert_eq!(
            fused.errors.len(),
            row.expected_error_count,
            "{}: expected exactly {} interpreter error(s), got {:?}",
            row.label,
            row.expected_error_count,
            fused.errors
        );
        for fragment in &row.error_fragments {
            assert!(
                fused.errors.iter().any(|error| error.contains(fragment)),
                "{}: expected an error containing {fragment:?}, got {:?}",
                row.label,
                fused.errors
            );
        }
        assert_eq!(
            unfused_hits, 0,
            "{}: the force-disabled run must never enter the fused evaluator",
            row.label
        );
        if row.expect_fusion {
            assert!(
                fused_hits >= 1,
                "{}: expected the fused evaluator to own at least one chain \
                 (recognizer coverage regression?)",
                row.label
            );
        } else {
            assert_eq!(
                fused_hits, 0,
                "{}: expected the recognizer to DECLINE every chain in this program",
                row.label
            );
        }
        fused_hits
    }

    /// The matrix, plus the E-6a shape-key coverage assertion (the four
    /// treatment shapes must be owned by the fused evaluator under their
    /// expected shape keys).
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn fused_vs_unfused_differential_matrix() {
        let _serial = serialize_toggle_tests();

        fusion_test_support::reset_counters();
        for row in differential_matrix() {
            assert_row_differential(&row).await;
        }

        // E-6a shape coverage (instrumentation deliverable): the four
        // treatment shapes reduce to three distinct shape keys (tag-guard
        // and σ-existence share readZipperAt.pathExists).
        let shapes = fusion_test_support::fusion_hits_by_shape();
        for expected in [
            "var-map:readZipperAt.getSubtrie",
            "var-map:readZipperAt.pathExists",
            "var-map:readZipperAt.descendFirst.getLeaf",
        ] {
            assert!(
                shapes.get(expected).copied().unwrap_or(0) >= 1,
                "E-6a shape {expected} was not owned by the fused evaluator; shapes seen: {shapes:?}"
            );
        }
    }

    /// PM-5(1) CONTROL-NEUTRALITY FALSIFIER: a method-heavy program with
    /// ZERO PathMap methods must produce byte-identical observations with a
    /// fusion-hit count of EXACTLY 0 in both modes — the name gate proven
    /// (non-PathMap methods pay one string compare and nothing else).
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn control_neutrality_zero_pathmap_methods() {
        let _serial = serialize_toggle_tests();

        let program = r#"
            @"a"!( [1, 2, 3].nth(1) ) |
            @"b"!( "hello world".length() ) |
            @"c"!( [1, 2, 3].slice(0, 2) ) |
            @"d"!( "hex me".toUtf8Bytes() ) |
            @"e"!( Set(1, 2).union(Set(3)) ) |
            @"f"!( {"k": 1}.get("k") ) |
            @"g"!( [4, 5, 6].toSet().size() ) |
            @"h"!( ("abc" ++ "def").length() )
        "#;
        let channels = ["a", "b", "c", "d", "e", "f", "g", "h"];

        let (unfused, unfused_hits) = observe_with_hits(
            QueryRunMode::FusedDisabled,
            "epm-p2-control-fd-",
            program,
            &channels,
            Cost::unsafe_max(),
        )
        .await;
        let (fused, fused_hits) = observe_with_hits(
            QueryRunMode::Fused,
            "epm-p2-control-f-",
            program,
            &channels,
            Cost::unsafe_max(),
        )
        .await;

        assert!(
            fused.errors.is_empty(),
            "control program must evaluate cleanly: {:?}",
            fused.errors
        );
        assert_eq!(
            fused, unfused,
            "control: byte-identical results and charges with zero PathMap methods"
        );
        assert_eq!(fused_hits, 0, "control: the name gate must reject every method");
        assert_eq!(unfused_hits, 0, "control: force-disabled must never fuse");
    }

    /// The P2 budget-exhaustion-at-index-k differential over the
    /// DETERMINISTIC PROJECTION (the P0-captured contract, inherited
    /// verbatim): errors + consumed compare EXACTLY at every k; the
    /// diagnostic committed-rows count compares exactly at the
    /// schedule-independent ks (0: nothing commits; ≥3: the complete run)
    /// and is bounded to [consumed, full-trace] at the racing ks (1, 2),
    /// where parallel branches race for the last token and the losing
    /// branch's abort truncates the attempt multiset.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn fused_vs_unfused_exhaustion_at_index_k() {
        let _serial = serialize_toggle_tests();

        let (label, program, channels) = &differential_programs()[3]; // sigma-chain
        const FULL_TRACE_ROWS: usize = 17;

        for k in 0..=4i64 {
            let (unfused, _) = observe_with_hits(
                QueryRunMode::FusedDisabled,
                &format!("epm-p2-exh-fd-{label}-{k}-"),
                program,
                channels,
                Cost::create(k, "differential budget"),
            )
            .await;
            let (fused, fused_hits) = observe_with_hits(
                QueryRunMode::Fused,
                &format!("epm-p2-exh-f-{label}-{k}-"),
                program,
                channels,
                Cost::create(k, "differential budget"),
            )
            .await;

            // The deterministic projection: exact at every k.
            assert_eq!(
                fused.errors, unfused.errors,
                "{label} k={k}: exhaustion error must not move under fusion"
            );
            assert_eq!(
                fused.consumed, unfused.consumed,
                "{label} k={k}: consumed total must not move under fusion"
            );

            if matches!(k, 1 | 2) {
                // Racing ks: committed rows are schedule-dependent; both
                // paths must stay inside the captured envelope.
                for (side, obs) in [("fused", &fused), ("unfused", &unfused)] {
                    assert!(
                        (obs.consumed as usize..=FULL_TRACE_ROWS)
                            .contains(&obs.charge_trace.len()),
                        "{label} k={k} ({side}): committed rows {} outside the envelope",
                        obs.charge_trace.len()
                    );
                }
            } else {
                // Schedule-independent ks: the FULL observation must match.
                assert_eq!(
                    fused, unfused,
                    "{label} k={k}: full observation must match at a deterministic k"
                );
            }

            // The chain only evaluates once the RESULT send's comm commits:
            // eval_send reserves its comm BEFORE evaluating the send data
            // (reduce.rs:1086 precedes the data eval), so at k ≤ 2 the third
            // comm exhausts first and the chain never runs (0 hits); at
            // k ≥ 3 the full program commits and the fused path must own the
            // chain.
            if k >= 3 {
                assert!(
                    fused_hits >= 1,
                    "{label} k={k}: the σ-chain must fuse once the result send commits"
                );
            } else {
                assert_eq!(
                    fused_hits, 0,
                    "{label} k={k}: the chain must not evaluate before the result \
                     send's comm commits"
                );
            }
        }
    }
}
