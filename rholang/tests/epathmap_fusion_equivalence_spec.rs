//! EPathMap fused-vs-fallback equivalence and COMM-accounting specification.
//!
//! The production `try_eval_fused_method_chain` optimization is checked
//! against the unfused per-link implementation from the same source. The
//! test-only [`QueryRunMode`] exposes
//! `FusedDisabled` / `Fused` variants — gated by a COMPILE-TIME
//! (`cfg`/feature) force-disable flag on the recognizer, never a runtime
//! flip (a runtime-flippable path is a node-divergence hazard under a latent
//! parity bug) — and asserts
//! `observe(Fused, …) == observe(FusedDisabled, …)` field-for-field over
//! every chain shape and edge program below.
//!
//! The seam exists in `interpreter/fused_pathmap_chain.rs`; the
//! `FusedDisabled`/`Fused` variants under the `epathmap-fusion-differential`
//! feature provide the compile-time gate (`cargo test -p rholang
//! --features epathmap-fusion-differential --test
//! epathmap_fusion_equivalence_spec`). Without the feature,
//! [`QueryRunMode::TodayPath`] exercises the fused production path and the
//! deterministic semantic/COMM-accounting assertions.
//!
//! Consensus accounting is ONE unit per committed COMM. Primitive,
//! substitution, and structural-reduction events have zero consensus cost.
//! Their canonical rows remain in [`QueryObservation::diagnostic_trace`] only
//! to prove that fusion preserves non-consensus diagnostics; this suite never
//! treats their operation names or weights as pinned metering values.

use std::collections::HashMap;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{ETuple, Expr, Par};
use models::rust::utils::{new_elist_par, new_gstring_par};
use prost::Message;
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::accounting::has_cost::HasCost;
use rholang::rust::interpreter::accounting::BillableKind;
use rholang::rust::interpreter::rho_runtime::{RhoRuntime, RhoRuntimeImpl};
use rholang::rust::interpreter::test_utils::resources::with_runtime;

// ─────────────────────────────────────────────────────────────────────────────
// Run modes
// ─────────────────────────────────────────────────────────────────────────────

/// Which evaluation path an observation runs. `TodayPath` is the production
/// path; differential modes exist only under the compile-time feature gate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum QueryRunMode {
    /// The production evaluation path (fusion active).
    TodayPath,
    /// The recognizer force-disabled via the test-only toggle — every chain
    /// takes the original per-link fallback, bit for bit.
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
    /// PROTOBUF bytes of each datum Par (the value-domain observable).
    par_bytes: Vec<Vec<u8>>,
    /// The datum's `random_state` (deterministic under the fixed rand).
    random_state: Vec<Vec<u8>>,
    persist: Vec<bool>,
    /// The produce EVENT HASH of each datum (`Datum::source.hash`).
    produce_hash: Vec<Vec<u8>>,
}

/// Everything a fused-vs-fallback differential compares.
#[derive(Clone, Debug, PartialEq, Eq)]
struct QueryObservation {
    /// Interpreter errors, rendered `{:?}` (pins variants AND payloads —
    /// e.g. the exact `"Error: Multiple expressions given."` string).
    errors: Vec<String>,
    /// Consensus consumed total (`EvaluateResult::cost.value`), equal to the
    /// committed COMM count.
    consumed: i64,
    /// Independently counted committed COMM events. This is the current
    /// consensus accounting model; diagnostic event weights are excluded.
    committed_comms: i64,
    /// Canonical non-consensus diagnostics. Dynamic fused-vs-fallback
    /// equality is useful, but these rows are not metering goldens.
    diagnostic_trace: Vec<String>,
    /// Per-channel readbacks, in the caller-given channel order.
    channels: Vec<ChannelObservation>,
}

fn render_diagnostic_event(kind: &BillableKind, weight: u64) -> String {
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
/// nondeterministic, so the suite seeds explicitly).
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
    // Compile-time-gated force-disable toggle. The guard restores the
    // production default (fusion active) on
    // drop, even across panics.
    #[cfg(feature = "epathmap-fusion-differential")]
    let _toggle_guard = {
        use rholang::rust::interpreter::fused_pathmap_chain::fusion_test_support;
        struct ToggleGuard;
        impl Drop for ToggleGuard {
            fn drop(&mut self) { fusion_test_support::set_force_disabled(false); }
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

        let canonical_events = runtime.cost().get_canonical_event_log();
        let committed_comms = canonical_events
            .iter()
            .filter(|event| matches!(event.kind, BillableKind::Comm))
            .count() as i64;
        let diagnostic_trace = canonical_events
            .iter()
            .map(|event| render_diagnostic_event(&event.kind, event.weight))
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
                random_state: data
                    .iter()
                    .map(|datum| datum.a.random_state.clone())
                    .collect(),
                persist: data.iter().map(|datum| datum.persist).collect(),
                produce_hash: data
                    .iter()
                    .map(|datum| datum.source.hash.bytes().to_vec())
                    .collect(),
            });
        }

        assert_eq!(
            res.cost.value, committed_comms,
            "consensus cost must equal the committed COMM count"
        );

        QueryObservation {
            errors: res
                .errors
                .iter()
                .map(|error| format!("{error:?}"))
                .collect(),
            consumed: res.cost.value,
            committed_comms,
            diagnostic_trace,
            channels: channel_observations,
        }
    })
    .await
}

// ─────────────────────────────────────────────────────────────────────────────
// Shared EPathMap query programs
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

/// Every program the differential must cover: the four E-6a chain
/// shapes plus the edge programs (Nil-mid-chain in all four Nil
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
// Production-path semantics, COMM accounting, and determinism
// ─────────────────────────────────────────────────────────────────────────────

fn gstring_par(value: &str) -> Par { new_gstring_par(value.to_string(), Vec::new(), false) }

fn ground_list(elements: Vec<Par>) -> Par {
    new_elist_par(elements, Vec::new(), false, None, Vec::new(), false)
}

fn ground_tuple1(inner: Par) -> Par {
    models::par_from_default! {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::ETupleBody(ETuple {
                ps: vec![inner],
                locally_free: Vec::new(),
                connective_used: false,
            })),
        }],
        ..Par::default()
    }
}

fn expected_sigma_entry() -> Par {
    ground_list(vec![
        gstring_par("v"),
        gstring_par("site0"),
        gstring_par("Pair.0"),
        ground_tuple1(gstring_par("A")),
    ])
}

fn single_observed_par(observation: &QueryObservation, channel_index: usize) -> Par {
    let channel = &observation.channels[channel_index];
    assert_eq!(
        channel.par_bytes.len(),
        1,
        "expected one datum at @{:?}",
        channel.channel
    );
    Par::decode(channel.par_bytes[0].as_slice()).expect("observed Par protobuf must decode")
}

/// Pin the intended production semantics independently of the dynamic
/// fused-vs-fallback comparison, and pin the current accounting rule: each
/// successful treatment shape consumes exactly its three COMMs while every
/// non-COMM diagnostic event contributes zero.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn today_path_has_expected_semantics_and_comm_accounting() {
    #[cfg(feature = "epathmap-fusion-differential")]
    let _serial = fused_differentials::serialize_toggle_tests();

    let programs = differential_programs();
    let mut observations = Vec::with_capacity(4);
    for (label, program, channels) in programs.iter().take(4) {
        let observation = observe(
            QueryRunMode::TodayPath,
            &format!("epm-current-{label}-"),
            program,
            channels,
            Cost::unsafe_max(),
        )
        .await;
        assert!(
            observation.errors.is_empty(),
            "{label}: unexpected errors: {:?}",
            observation.errors
        );
        assert_eq!(observation.consumed, 3, "{label}: expected three COMMs");
        assert_eq!(observation.committed_comms, 3);
        observations.push(observation);
    }

    let discovery = single_observed_par(&observations[0], 0);
    match discovery
        .exprs
        .first()
        .and_then(|expr| expr.expr_instance.as_ref())
    {
        Some(ExprInstance::EPathmapBody(map)) => {
            let entries = map.entry_trie().entries_owned();
            assert_eq!(entries.len(), 1);
            assert_eq!(
                entries[0],
                ground_list(vec![gstring_par("t.deadbeef.Pair"), gstring_par("site0")])
            );
        }
        other => panic!("expected an EPathMap discovery result, got {other:?}"),
    }

    for (label, observation) in [
        ("tag guard", &observations[1]),
        ("sigma existence", &observations[2]),
    ] {
        let value = single_observed_par(observation, 0);
        assert_eq!(
            value
                .exprs
                .first()
                .and_then(|expr| expr.expr_instance.clone()),
            Some(ExprInstance::GBool(true)),
            "{label} must hold"
        );
    }
    assert_eq!(
        single_observed_par(&observations[3], 0),
        expected_sigma_entry(),
        "getLeaf must return the original trie value losslessly"
    );

    for (label, program, channels) in programs.iter().skip(4).take(4) {
        let observation = observe(
            QueryRunMode::TodayPath,
            &format!("epm-current-{label}-"),
            program,
            channels,
            Cost::unsafe_max(),
        )
        .await;
        assert_eq!(observation.consumed, 1, "{label}: expected one send COMM");
        assert_eq!(observation.committed_comms, 1);
        assert_eq!(observation.errors.len(), 1);
        assert!(
            observation.errors[0].contains("ReduceError")
                && observation.errors[0].contains("Error: Multiple expressions given."),
            "{label}: wrong Nil-mid-chain error: {:?}",
            observation.errors
        );
    }

    let (label, program, channels) = &programs[8];
    let wrong_arity = observe(
        QueryRunMode::TodayPath,
        &format!("epm-current-{label}-"),
        program,
        channels,
        Cost::unsafe_max(),
    )
    .await;
    assert_eq!(wrong_arity.consumed, 1);
    assert_eq!(wrong_arity.committed_comms, 1);
    assert_eq!(wrong_arity.errors.len(), 1);
    assert!(
        wrong_arity.errors[0].contains("MethodArgumentNumberMismatch")
            && wrong_arity.errors[0].contains("getLeaf"),
        "wrong-arity ordering drifted: {:?}",
        wrong_arity.errors
    );
}

/// Two fresh runtimes, identical observations — errors, COMM consumption,
/// diagnostic trace, result bytes, random_state, persist flags, and produce
/// hashes. This is
/// the null-differential every later fused-vs-unfused comparison stands on.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn today_path_observations_are_deterministic() {
    // Under the differential feature every test in this binary serializes
    // on the toggle lock (the toggle + hit counters are process-global).
    #[cfg(feature = "epathmap-fusion-differential")]
    let _serial = fused_differentials::serialize_toggle_tests();
    for (label, program, channels) in differential_programs() {
        let first = observe(
            QueryRunMode::TodayPath,
            &format!("epm-current-diff-a-{label}-"),
            &program,
            &channels,
            Cost::unsafe_max(),
        )
        .await;
        let second = observe(
            QueryRunMode::TodayPath,
            &format!("epm-current-diff-b-{label}-"),
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

/// The bounded-budget variant of the null differential — the COMM-boundary
/// axis the fused-vs-fallback exhaustion differential walks.
///
/// At budgets where parallel branches race for the last token, the losing
/// branch aborts and can truncate the non-consensus diagnostic trace. The
/// deterministic consensus projection is `(errors, consumed,
/// committed_comms)`. The suite therefore asserts FULL observation
/// equality at k∈{0,3,4} (empty commit / complete runs) and the projection
/// at k∈{1,2}.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn today_path_bounded_budget_observations_are_deterministic() {
    // Under the differential feature every test in this binary serializes
    // on the toggle lock (the toggle + hit counters are process-global).
    #[cfg(feature = "epathmap-fusion-differential")]
    let _serial = fused_differentials::serialize_toggle_tests();
    let (label, program, channels) = &differential_programs()[3]; // sigma-chain
    let expected = [
        (0, true, 0),
        (1, true, 1),
        (2, true, 2),
        (3, false, 3),
        (4, false, 3),
    ];
    for (k, expect_oop, expected_comms) in expected {
        let first = observe(
            QueryRunMode::TodayPath,
            &format!("epm-current-diffk-a-{label}-{k}-"),
            program,
            channels,
            Cost::create(k, "differential budget"),
        )
        .await;
        let second = observe(
            QueryRunMode::TodayPath,
            &format!("epm-current-diffk-b-{label}-{k}-"),
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
            assert_eq!(
                first.committed_comms, second.committed_comms,
                "{label} k={k}: committed COMM count must be deterministic"
            );
        } else {
            assert_eq!(
                first, second,
                "{label} k={k}: bounded-budget observation must be byte-deterministic"
            );
        }
        assert_eq!(first.consumed, expected_comms, "{label} k={k}");
        assert_eq!(first.committed_comms, expected_comms, "{label} k={k}");
        assert_eq!(
            first
                .errors
                .iter()
                .any(|error| error.contains("OutOfPhlogistons")),
            expect_oop,
            "{label} k={k}: wrong COMM-boundary verdict"
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Fused-vs-unfused differential suite (compile-time feature gated)
// ─────────────────────────────────────────────────────────────────────────────

/// For every fusable shape and every edge, the
/// FUSED observation must equal the FORCE-DISABLED (per-link fallback)
/// observation field-for-field — result Par bytes, datum `random_state`,
/// persist flags, produce event hashes, interpreter errors (variant AND
/// payload strings), the consensus COMM total, and the full non-consensus
/// diagnostic trace. Each row also
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
    /// concurrently-running production-path test (whose E-6a programs fuse)
    /// would smear another test's hit delta — and a `TodayPath` observation
    /// taken inside a force-disabled window, while byte-equivalent (that IS
    /// the differential claim), would turn a parity bug into flakiness
    /// instead of a clean failure. Production-path tests take this lock too.
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
    /// all four Nil sources and both in-fusion error families
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

        // ── Nil sources + follow-on link ─────────────────────────────────
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
            vec![
                "MethodNotDefined",
                "ascend (requires integer argument)",
                "non-integer",
            ],
        ));
        rows.push(error_row(
            // MethodNotDefined("ascend (steps must be non-negative)",
            // "negative: -1") — in-fusion.
            "ascend-negative-arg",
            r#"@"out"!( {| ["a"] |}.readZipper().ascend(-1) )"#,
            vec!["out"],
            true,
            vec![
                "MethodNotDefined",
                "ascend (steps must be non-negative)",
                "negative: -1",
            ],
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

        // ── the expr-arm (guard) dispatch route ──────────────────────────
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
            // route (eval_to_bool → eval_expr_to_expr → the seam): the
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
            &format!("epm-fusion-fd-{}-", row.label),
            &row.program,
            &row.channels,
            Cost::unsafe_max(),
        )
        .await;
        let (fused, fused_hits) = observe_with_hits(
            QueryRunMode::Fused,
            &format!("epm-fusion-f-{}-", row.label),
            &row.program,
            &row.channels,
            Cost::unsafe_max(),
        )
        .await;

        assert_eq!(
            fused, unfused,
            "{}: the fused observation must equal the force-disabled observation \
             field-for-field (result bytes, random_state, persist, produce hashes, \
             errors, COMM consumption, diagnostic trace)",
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

    /// Control-neutrality falsifier: a method-heavy program with
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
            "epm-fusion-control-fd-",
            program,
            &channels,
            Cost::unsafe_max(),
        )
        .await;
        let (fused, fused_hits) = observe_with_hits(
            QueryRunMode::Fused,
            "epm-fusion-control-f-",
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
            "control: byte-identical observations with zero PathMap methods"
        );
        assert_eq!(
            fused_hits, 0,
            "control: the name gate must reject every method"
        );
        assert_eq!(unfused_hits, 0, "control: force-disabled must never fuse");
    }

    /// Budget-exhaustion differential over the deterministic consensus
    /// projection: errors, consumed units, and committed COMM count compare
    /// exactly at every boundary. Full observations, including diagnostics,
    /// compare only when the attempt multiset is schedule-independent.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn fused_vs_unfused_exhaustion_at_index_k() {
        let _serial = serialize_toggle_tests();

        let (label, program, channels) = &differential_programs()[3]; // sigma-chain
        for k in 0..=4i64 {
            let (unfused, _) = observe_with_hits(
                QueryRunMode::FusedDisabled,
                &format!("epm-fusion-exh-fd-{label}-{k}-"),
                program,
                channels,
                Cost::create(k, "differential budget"),
            )
            .await;
            let (fused, fused_hits) = observe_with_hits(
                QueryRunMode::Fused,
                &format!("epm-fusion-exh-f-{label}-{k}-"),
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
            assert_eq!(
                fused.committed_comms, unfused.committed_comms,
                "{label} k={k}: committed COMM count must not move under fusion"
            );

            if !matches!(k, 1 | 2) {
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
