use std::collections::HashSet;

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{Expr, Par};
use rholang::rust::interpreter::accounting::has_cost::HasCost;
use rholang::rust::interpreter::errors::InterpreterError;
use rholang::rust::interpreter::interpreter::EvaluateResult;
use rholang::rust::interpreter::rho_runtime::{RhoRuntime, RhoRuntimeImpl};
use rholang::rust::interpreter::test_utils::resources::with_runtime;

async fn execute(
    runtime: &mut RhoRuntimeImpl,
    term: &str,
) -> Result<EvaluateResult, InterpreterError> {
    runtime.evaluate_with_term(term).await
}

async fn eval_ok(runtime: &mut RhoRuntimeImpl, term: &str) {
    let res = execute(runtime, term).await.unwrap();
    assert!(
        res.errors.is_empty(),
        "Expected success for: {}\nErrors: {:?}",
        term,
        res.errors
    );
}

async fn eval_err(runtime: &mut RhoRuntimeImpl, term: &str) {
    let res = execute(runtime, term).await.unwrap();
    assert!(
        !res.errors.is_empty(),
        "Expected error for: {}\nGot success",
        term
    );
}

async fn channel_data(runtime: &RhoRuntimeImpl, channel_expr: ExprInstance) -> HashSet<Par> {
    let ch = vec![models::par_from_default! {
        exprs: vec![Expr {
            expr_instance: Some(channel_expr),
        }],
        ..Default::default()
    }];
    runtime
        .get_hot_changes()
        .await
        .get(&ch)
        .map(|row| row.data.iter().flat_map(|d| d.a.pars.clone()).collect())
        .unwrap_or_default()
}

fn int_channel(n: i64) -> ExprInstance { ExprInstance::GInt(n) }

fn has_par_with_bool(data: &HashSet<Par>, expected: bool) -> bool {
    data.iter().any(|p| {
        p.exprs
            .iter()
            .any(|e| e.expr_instance == Some(ExprInstance::GBool(expected)))
    })
}

fn has_par_with_double(data: &HashSet<Par>, expected: f64) -> bool {
    let bits = expected.to_bits();
    data.iter().any(|p| {
        p.exprs
            .iter()
            .any(|e| e.expr_instance == Some(ExprInstance::GDouble(bits)))
    })
}

fn has_par_with_string(data: &HashSet<Par>, expected: &str) -> bool {
    data.iter().any(|p| {
        p.exprs
            .iter()
            .any(|e| e.expr_instance == Some(ExprInstance::GString(expected.to_string())))
    })
}

// --- Example file tests ---

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn numeric_types_example_evaluates_without_errors() {
    with_runtime("numeric-types-example-", |mut runtime| async move {
        let source = include_str!("../examples/numeric-types.rho");
        eval_ok(&mut runtime, source).await;
    })
    .await
}

// --- Float end-to-end ---

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn float_arithmetic_produces_correct_values() {
    with_runtime("float-arith-", |mut runtime| async move {
        eval_ok(
            &mut runtime,
            r#"@0!(1.5f64 + 2.25f64) | @1!(3.0f64 * 4.0f64) | @2!(10.0f64 / 4.0f64) | @3!(10.0f64 - 3.5f64)"#,
        )
        .await;

        let ch0 = channel_data(&runtime, int_channel(0)).await;
        let ch1 = channel_data(&runtime, int_channel(1)).await;
        let ch2 = channel_data(&runtime, int_channel(2)).await;
        let ch3 = channel_data(&runtime, int_channel(3)).await;
        assert!(has_par_with_double(&ch0, 3.75));
        assert!(has_par_with_double(&ch1, 12.0));
        assert!(has_par_with_double(&ch2, 2.5));
        assert!(has_par_with_double(&ch3, 6.5));
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn float_comparisons_produce_correct_booleans() {
    with_runtime("float-cmp-", |mut runtime| async move {
        eval_ok(
            &mut runtime,
            r#"@0!(1.0f64 < 2.0f64) | @1!(2.0f64 < 1.0f64) | @2!(1.0f64 <= 1.0f64) | @3!(2.0f64 > 1.0f64)"#,
        )
        .await;

        assert!(has_par_with_bool(&channel_data(&runtime, int_channel(0)).await, true));
        assert!(has_par_with_bool(&channel_data(&runtime, int_channel(1)).await, false));
        assert!(has_par_with_bool(&channel_data(&runtime, int_channel(2)).await, true));
        assert!(has_par_with_bool(&channel_data(&runtime, int_channel(3)).await, true));
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn float_division_by_zero_produces_ieee754_values() {
    with_runtime("float-divz-", |mut runtime| async move {
        eval_ok(&mut runtime, r#"@0!(1.0f64 / 0.0f64)"#).await;
        assert!(has_par_with_double(
            &channel_data(&runtime, int_channel(0)).await,
            f64::INFINITY
        ));

        eval_ok(&mut runtime, r#"@1!(-1.0f64 / 0.0f64)"#).await;
        assert!(has_par_with_double(
            &channel_data(&runtime, int_channel(1)).await,
            f64::NEG_INFINITY
        ));

        eval_ok(&mut runtime, r#"@2!(0.0f64 / 0.0f64)"#).await;
        let ch2 = channel_data(&runtime, int_channel(2)).await;
        let has_nan = ch2.iter().any(|p| {
            p.exprs.iter().any(|e| {
                matches!(&e.expr_instance, Some(ExprInstance::GDouble(bits)) if f64::from_bits(*bits).is_nan())
            })
        });
        assert!(has_nan, "0.0/0.0 should produce NaN");
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn float_nan_equality_follows_ieee754() {
    with_runtime("float-nan-", |mut runtime| async move {
        eval_ok(
            &mut runtime,
            r#"
            new x in {
                x!(0.0f64 / 0.0f64) |
                for (@nan <- x) {
                    @0!(nan == nan) |
                    @1!(nan != nan)
                }
            }
            "#,
        )
        .await;

        assert!(has_par_with_bool(
            &channel_data(&runtime, int_channel(0)).await,
            false
        ));
        assert!(has_par_with_bool(
            &channel_data(&runtime, int_channel(1)).await,
            true
        ));
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn float_nan_nested_in_list_follows_ieee754() {
    with_runtime("float-nan-nested-", |mut runtime| async move {
        eval_ok(
            &mut runtime,
            r#"
            new x in {
                x!(0.0f64 / 0.0f64) |
                for (@nan <- x) {
                    @0!([nan] == [nan]) |
                    @1!([nan] != [nan])
                }
            }
            "#,
        )
        .await;

        assert!(has_par_with_bool(
            &channel_data(&runtime, int_channel(0)).await,
            false
        ));
        assert!(has_par_with_bool(
            &channel_data(&runtime, int_channel(1)).await,
            true
        ));
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn float_nan_comparisons_return_false() {
    with_runtime("float-nan-cmp-", |mut runtime| async move {
        eval_ok(
            &mut runtime,
            r#"
            new x in {
                x!(0.0f64 / 0.0f64) |
                for (@nan <- x) {
                    @0!(nan < 1.0f64) |
                    @1!(nan > 1.0f64) |
                    @2!(nan <= 1.0f64) |
                    @3!(nan >= 1.0f64)
                }
            }
            "#,
        )
        .await;

        for i in 0..4 {
            assert!(
                has_par_with_bool(&channel_data(&runtime, int_channel(i)).await, false),
                "NaN comparison on channel {} should be false",
                i
            );
        }
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn float_modulo_by_zero_is_error() {
    with_runtime("float-modz-", |mut runtime| async move {
        eval_err(&mut runtime, r#"@0!(1.0f64 % 0.0f64)"#).await;
    })
    .await
}

// --- BigInt end-to-end ---

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn bigint_arithmetic_produces_correct_values() {
    with_runtime("bigint-arith-", |mut runtime| async move {
        eval_ok(
            &mut runtime,
            r#"@0!(100n + 200n) | @1!(7n * 13n) | @2!(100n / 3n) | @3!(100n % 3n) | @4!(50n - 30n)"#,
        )
        .await;

        let storage = rholang::rust::interpreter::storage::storage_printer::pretty_print(&runtime).await;
        assert!(storage.contains("300n"));
        assert!(storage.contains("91n"));
        assert!(storage.contains("33n"));
        assert!(storage.contains("1n"));
        assert!(storage.contains("20n"));
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn bigint_division_by_zero_is_error() {
    with_runtime("bigint-divz-", |mut runtime| async move {
        eval_err(&mut runtime, r#"@0!(1n / 0n)"#).await;
        eval_err(&mut runtime, r#"@0!(1n % 0n)"#).await;
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn bigint_comparisons_produce_correct_booleans() {
    with_runtime("bigint-cmp-", |mut runtime| async move {
        eval_ok(
            &mut runtime,
            r#"@0!(5n < 10n) | @1!(10n < 5n) | @2!(5n <= 5n) | @3!(10n > 5n) | @4!(5n >= 5n) | @5!(5n == 5n) | @6!(5n != 10n)"#,
        )
        .await;

        for i in [0, 2, 3, 4, 5, 6] {
            assert!(
                has_par_with_bool(&channel_data(&runtime, int_channel(i)).await, true),
                "channel {} should be true",
                i
            );
        }
        assert!(has_par_with_bool(&channel_data(&runtime, int_channel(1)).await, false));
    })
    .await
}

// --- BigRat end-to-end ---

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn bigrat_arithmetic_produces_correct_values() {
    with_runtime("bigrat-arith-", |mut runtime| async move {
        eval_ok(
            &mut runtime,
            r#"@0!(1r / 3r + 1r / 6r) | @1!(2r * 3r) | @2!(10r - 4r)"#,
        )
        .await;

        let storage =
            rholang::rust::interpreter::storage::storage_printer::pretty_print(&runtime).await;
        assert!(storage.contains("1/2r"));
        assert!(storage.contains("6/1r"));
        assert!(storage.contains("6/1r"));
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn bigrat_division_by_zero_is_error() {
    with_runtime("bigrat-divz-", |mut runtime| async move {
        eval_err(&mut runtime, r#"@0!(1r / 0r)"#).await;
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn bigrat_modulo_returns_zero() {
    with_runtime("bigrat-mod-", |mut runtime| async move {
        eval_ok(&mut runtime, r#"@0!(7r % 3r)"#).await;
        let storage =
            rholang::rust::interpreter::storage::storage_printer::pretty_print(&runtime).await;
        assert!(storage.contains("0/1r"));
    })
    .await
}

// --- FixedPoint end-to-end ---

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fixedpoint_arithmetic_produces_correct_values() {
    with_runtime("fp-arith-", |mut runtime| async move {
        eval_ok(
            &mut runtime,
            r#"@0!(1.50p2 + 2.25p2) | @1!(1.5p1 * 2.0p1) | @2!(10.00p2 - 3.25p2)"#,
        )
        .await;

        let storage =
            rholang::rust::interpreter::storage::storage_printer::pretty_print(&runtime).await;
        assert!(storage.contains("3.75p2"));
        assert!(storage.contains("3.0p1"));
        assert!(storage.contains("6.75p2"));
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fixedpoint_modulo_regression() {
    with_runtime("fp-mod-", |mut runtime| async move {
        eval_ok(
            &mut runtime,
            r#"@0!(1.50p2 % 1.00p2) | @1!(10.0p1 % 3.0p1)"#,
        )
        .await;

        let storage =
            rholang::rust::interpreter::storage::storage_printer::pretty_print(&runtime).await;
        assert!(storage.contains("0.50p2"));
        assert!(storage.contains("1.0p1"));
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fixedpoint_scale_mismatch_is_error() {
    with_runtime("fp-scale-", |mut runtime| async move {
        eval_err(&mut runtime, r#"@0!(1.5p1 + 2.50p2)"#).await;
        eval_err(&mut runtime, r#"@0!(1.5p1 - 2.50p2)"#).await;
        eval_err(&mut runtime, r#"@0!(1.5p1 * 2.50p2)"#).await;
        eval_err(&mut runtime, r#"@0!(1.5p1 / 2.50p2)"#).await;
        eval_err(&mut runtime, r#"@0!(1.5p1 % 2.50p2)"#).await;
        eval_err(&mut runtime, r#"@0!(1.5p1 < 2.50p2)"#).await;
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fixedpoint_division_by_zero_is_error() {
    with_runtime("fp-divz-", |mut runtime| async move {
        eval_err(&mut runtime, r#"@0!(1.5p1 / 0.0p1)"#).await;
        eval_err(&mut runtime, r#"@0!(1.5p1 % 0.0p1)"#).await;
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fixedpoint_comparisons_produce_correct_booleans() {
    with_runtime("fp-cmp-", |mut runtime| async move {
        eval_ok(
            &mut runtime,
            r#"@0!(1.5p1 < 2.0p1) | @1!(2.0p1 < 1.5p1) | @2!(1.5p1 == 1.5p1) | @3!(1.5p1 != 2.0p1)"#,
        )
        .await;

        assert!(has_par_with_bool(&channel_data(&runtime, int_channel(0)).await, true));
        assert!(has_par_with_bool(&channel_data(&runtime, int_channel(1)).await, false));
        assert!(has_par_with_bool(&channel_data(&runtime, int_channel(2)).await, true));
        assert!(has_par_with_bool(&channel_data(&runtime, int_channel(3)).await, true));
    })
    .await
}

// --- Cross-type errors ---

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cross_type_operations_are_errors() {
    with_runtime("cross-type-", |mut runtime| async move {
        eval_err(&mut runtime, r#"@0!(1n + 1r)"#).await;
        eval_err(&mut runtime, r#"@0!(1.0f64 + 1n)"#).await;
        eval_err(&mut runtime, r#"@0!(1.5p1 + 1n)"#).await;
        eval_err(&mut runtime, r#"@0!(1.0f64 < 1n)"#).await;
    })
    .await
}

// --- Channel-based numeric data flow ---

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn numeric_values_survive_channel_round_trip() {
    with_runtime("channel-rt-", |mut runtime| async move {
        eval_ok(
            &mut runtime,
            r#"
            new ch in {
                ch!(1.5f64) |
                for (@v <- ch) { @0!(v + 2.5f64) }
            }
            "#,
        )
        .await;

        assert!(has_par_with_double(
            &channel_data(&runtime, int_channel(0)).await,
            4.0
        ));
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn numeric_values_in_lists_and_tuples() {
    with_runtime("list-tuple-", |mut runtime| async move {
        eval_ok(
            &mut runtime,
            r#"@0!([1n, 2n, 3n]) | @1!((1.5f64, "label"))"#,
        )
        .await;

        let storage =
            rholang::rust::interpreter::storage::storage_printer::pretty_print(&runtime).await;
        assert!(storage.contains("1n"));
        assert!(storage.contains("2n"));
        assert!(storage.contains("3n"));
        assert!(storage.contains("1.5f64"));
    })
    .await
}

// --- Pattern matching with numeric types ---

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pattern_match_on_numeric_values() {
    with_runtime("pattern-match-", |mut runtime| async move {
        eval_ok(
            &mut runtime,
            r#"
            new ch in {
                ch!(42) |
                for (@42 <- ch) { @0!("matched") }
            }
            "#,
        )
        .await;

        assert!(has_par_with_string(
            &channel_data(&runtime, int_channel(0)).await,
            "matched"
        ));
    })
    .await
}

// ═══════════════════════════════════════════════════════════════════════════════════════════════
// ★★ Int overflow is CHECKED end-to-end — and what that costs the wrap-detect idiom
// ═══════════════════════════════════════════════════════════════════════════════════════════════

/// ★★ `Int` `+` and `-` REFUSE an unrepresentable result, from Rholang source.
///
/// The reducer-level cells live in `reduce_spec.rs`; this one measures the same ruling through the
/// whole pipeline — parse, normalize, reduce — because that is the surface a deployer writes
/// against. Ruled 2026-07-29: *"fix it — checked, with a clear error."*
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn int_addition_and_subtraction_refuse_an_unrepresentable_result() {
    with_runtime("int-overflow-checked-", |mut runtime| async move {
        // Both directions refuse.
        eval_err(&mut runtime, r#"@0!(9223372036854775807 + 1)"#).await;
        eval_err(&mut runtime, r#"@1!(-9223372036854775808 - 1)"#).await;

        // ── THE CONTROL: totals still compute, and compute the same values. ─────────────
        eval_ok(
            &mut runtime,
            r#"@2!(7 + 8) | @3!(10 - 3) | @4!(9223372036854775807 - 1)"#,
        )
        .await;
        assert!(channel_data(&runtime, int_channel(2))
            .await
            .iter()
            .any(|p| p
                .exprs
                .iter()
                .any(|e| e.expr_instance == Some(ExprInstance::GInt(15)))));
        assert!(channel_data(&runtime, int_channel(3))
            .await
            .iter()
            .any(|p| p
                .exprs
                .iter()
                .any(|e| e.expr_instance == Some(ExprInstance::GInt(7)))));
        assert!(channel_data(&runtime, int_channel(4))
            .await
            .iter()
            .any(|p| p
                .exprs
                .iter()
                .any(|e| e.expr_instance == Some(ExprInstance::GInt(9223372036854775806)))));
    })
    .await
}

/// ★★ THE MEASURED BLAST RADIUS — the "detect overflow by observing the wrap" idiom no longer
/// answers `false`; it RAISES.
///
/// `casper/src/main/resources/NonNegativeNumber.rho`'s `add` contract is written
///
/// ```text
///     if (v + x >= v) { …store v + x…; success!(true) }
///     else            { //overflow
///                       …store v…;     success!(false) }
/// ```
///
/// which only reaches its `else` branch **because `v + x` wrapped to a smaller number**. Under
/// checked addition the condition itself refuses, so the deploy aborts and `success` never
/// receives. `MakeMint.rho`'s `deposit` calls that same `add`.
///
/// ★ **RULED and APPLIED 2026-07-29.** The remedy is to test BEFORE adding, not after:
/// `if (x <= 9223372036854775807 - v)`, which is total for every non-negative `v`, `x`. It is now
/// what `NonNegativeNumber.rho` says. Contrary to what this comment previously claimed, the edit
/// does **not** move the contract's registry URI — `Registry.rho:592` derives it as
/// `build_uri(blake2b256(pubKeyBytes))` and signs only `(timestamp, deployerPubKey, version)`, so
/// neither the URI nor the insertion signature depends on the contract body. It does move the
/// genesis block hash and post-state hash, which was measured before the edit was made.
///
/// ⚠ **And contrary to what this comment previously claimed, the genesis suites do NOT exercise
/// either idiom.** Measured 2026-07-29 with `--no-capture`: of the 22 `RhoSpec`-driven genesis
/// specs, only `failing_result_collector_spec` collects any assertion at all — it is the only
/// test resource that performs no registry lookup. `non_negative_number_spec` and
/// `make_mint_spec` collect ZERO assertions and never report `has_finished`, and
/// `RhoSpec::run_tests` iterates only the assertions it received, so an empty collection is a
/// PASS. `test_fail_on_overflow` and `test_overflow_deposit` are written, and they never run.
/// That is why the behavioural pin lives here, in a test that drives the reducer directly.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_wrap_detect_overflow_idiom_now_raises_and_its_total_replacement_does_not() {
    with_runtime("wrap-detect-idiom-", |mut runtime| async move {
        // The idiom as the standard contracts write it, with `v = x = i64::MAX - 50`.
        eval_err(
            &mut runtime,
            r#"
            new v, x in {
                v!(9223372036854775757) | x!(9223372036854775757) |
                for (@vv <- v; @xx <- x) {
                    if (vv + xx >= vv) { @0!(true) } else { @0!(false) }
                }
            }
            "#,
        )
        .await;

        // ★ THE REPLACEMENT: guard BEFORE adding. Total for every non-negative pair, so it
        // answers `false` exactly where the wrap-detect idiom used to, and never raises.
        eval_ok(
            &mut runtime,
            r#"
            new v, x in {
                v!(9223372036854775757) | x!(9223372036854775757) |
                for (@vv <- v; @xx <- x) {
                    if (xx <= 9223372036854775807 - vv) { @1!(true) } else { @1!(false) }
                }
            }
            "#,
        )
        .await;
        assert!(
            has_par_with_bool(&channel_data(&runtime, int_channel(1)).await, false),
            "★ the total guard must answer `false` for the overflowing pair — same verdict the \
             wrap-detect idiom used to reach, without evaluating the overflowing sum",
        );

        // …and it still admits a pair that does NOT overflow.
        eval_ok(
            &mut runtime,
            r#"
            new v, x in {
                v!(10) | x!(32) |
                for (@vv <- v; @xx <- x) {
                    if (xx <= 9223372036854775807 - vv) { @2!(vv + xx) } else { @2!(-1) }
                }
            }
            "#,
        )
        .await;
        assert!(
            channel_data(&runtime, int_channel(2))
                .await
                .iter()
                .any(|p| p
                    .exprs
                    .iter()
                    .any(|e| e.expr_instance == Some(ExprInstance::GInt(42)))),
            "★ THE CONTROL: the total guard must not reject a sum that fits",
        );
    })
    .await
}

// ════════════════════════════════════════════════════════════════════════════════════════════════
// THE MECHANISM, and the genesis contract's guard shape
// ════════════════════════════════════════════════════════════════════════════════════════════════
//
// Everything above this line runs through `evaluate_with_term`, which takes a soft checkpoint and
// **reverts it whenever the evaluation reports an error** (`rho_runtime.rs:112-129`). That makes
// "the channel is empty after the failure" an assertion about the ROLLBACK, not about the failing
// reduction — it would hold whether or not the `else` branch had fired, so it is vacuous as a
// fails-open probe. The two cells below therefore inject the term with `RhoRuntime::inj`, which
// takes no checkpoint and reverts nothing, so what the store holds afterwards is what the
// reduction actually left there.

/// Normalize `term` and inject it with no soft checkpoint, so a failure leaves the tuplespace in
/// whatever state the failing reduction produced.
async fn inject(runtime: &RhoRuntimeImpl, term: &str) -> Result<(), InterpreterError> {
    let par = rholang::rust::interpreter::compiler::compiler::Compiler::source_to_adt(term)?;
    runtime
        .cost()
        .set(rholang::rust::interpreter::accounting::costs::Cost::unsafe_max());
    runtime
        .inj(
            par,
            rholang::rust::interpreter::env::Env::new(),
            crypto::rust::hash::blake2b512_random::Blake2b512Random::create_from_length(128),
        )
        .await
}

fn string_channel(name: &str) -> ExprInstance { ExprInstance::GString(name.to_string()) }

async fn int_on(runtime: &RhoRuntimeImpl, channel: &str) -> Vec<i64> {
    let mut found: Vec<i64> = channel_data(runtime, string_channel(channel))
        .await
        .iter()
        .flat_map(|p| p.exprs.iter())
        .filter_map(|e| match e.expr_instance {
            Some(ExprInstance::GInt(n)) => Some(n),
            _ => None,
        })
        .collect();
    found.sort_unstable();
    found
}

async fn bools_on(runtime: &RhoRuntimeImpl, channel: &str) -> Vec<bool> {
    let mut found: Vec<bool> = channel_data(runtime, string_channel(channel))
        .await
        .iter()
        .flat_map(|p| p.exprs.iter())
        .filter_map(|e| match e.expr_instance {
            Some(ExprInstance::GBool(b)) => Some(b),
            _ => None,
        })
        .collect();
    found.sort_unstable();
    found
}

/// ★★ **THE MECHANISM.** An arithmetic raise inside an `if` CONDITION is LOUD. It does **not**
/// silently select the `else` branch.
///
/// This is the cell that decides whether Rholang's `if` is fails-open or fails-closed on an
/// undecidable condition, and it is the reason `NonNegativeNumber.rho`'s old `if (v + x >= v)`
/// could not be left alone "because the genesis suite still passes".
///
/// The code path, named: `reduce.rs`'s [`eval_if`] evaluates the condition with
/// `self.eval_expr(&condition, env)?` — the `?` PROPAGATES a `combine_plus` refusal straight out
/// of `eval_if`, so no branch is ever selected. Only if the condition evaluates *successfully* to
/// a non-boolean does control reach the `match extract_bool(..)` arms, and even there the `None`
/// arm is `Err(InterpreterError::IfConditionTypeError { .. })` rather than a default branch. There
/// is no arm of `eval_if` that treats a failure as `false`.
///
/// ⚠ Watched RED three ways: if `combine_plus` returns to `wrapping_add` there is no error and
/// `expect_err` fails; if `eval_if` ever grows a fails-open arm, `@"wrapdetect"` holds `false` and
/// the emptiness assertion fails; if the refusal comes from somewhere other than the addition,
/// the message assertion fails.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_arithmetic_raise_in_an_if_condition_is_loud_and_selects_no_branch() {
    with_runtime("if-condition-raise-", |runtime| async move {
        // ★ THE NON-VACUITY FLOOR for the emptiness assertion at the end of this cell. An
        // `else`-branch write IS observable by `bools_on` — demonstrated on a condition that is
        // decidable and false — so "nothing arrived on `@\"wrapdetect\"`" below means the branch
        // did not run, not that the probe cannot see branches running.
        inject(
            &runtime,
            r#"if (1 > 2) { @"decidable"!(true) } else { @"decidable"!(false) }"#,
        )
        .await
        .expect("a decidable condition reduces");
        assert_eq!(
            bools_on(&runtime, "decidable").await,
            vec![false],
            "★ FLOOR: a taken `else` branch must be visible to this probe, or the emptiness \
             assertion below proves nothing",
        );

        let err = inject(
            &runtime,
            r#"
            @"v"!(9223372036854775757) | @"x"!(9223372036854775757) |
            for (@vv <- @"v"; @xx <- @"x") {
                if (vv + xx >= vv) { @"wrapdetect"!(true) } else { @"wrapdetect"!(false) }
            }
            "#,
        )
        .await
        .expect_err("★ checked `+` must refuse the overflowing sum in an `if` condition");

        let text = format!("{err}");
        assert!(
            text.contains("Arithmetic overflow in addition"),
            "★ the refusal must come from the ADDITION, naming its operands; got {text:?}",
        );
        assert!(
            text.contains("9223372036854775757"),
            "★ the refusal must pin the operands; got {text:?}",
        );

        // ★ THE FAILS-OPEN DISCRIMINATOR. If an unevaluable `if` condition selected the `else`
        // branch, `false` would be sitting on `@"wrapdetect"` right now.
        assert!(
            bools_on(&runtime, "wrapdetect").await.is_empty(),
            "★★ an error in an `if` CONDITION must select NO branch — Rholang's `if` is not \
             allowed to read an arithmetic failure as `false`",
        );
    })
    .await
}

/// ★★ **THE GENESIS CONTRACT'S GUARD, at the two values it exists to refuse.**
///
/// This reproduces the exact shape of `casper/src/main/resources/NonNegativeNumber.rho`'s `add` —
/// a `for` that CONSUMES the stored balance, a guard, and two branches of which the `else` one
/// must put the balance back — with the ruled total guard
/// `if (x <= 9223372036854775807 - v)` substituted for the old wrap detect. The store is a quoted
/// name rather than `@(*MergeableTag, *valueStore)` because the unforgeable pair is not reachable
/// from a test term; the guard, the consume and the restore are the contract's, verbatim.
///
/// The two refused values are the ones the (vacuous) genesis suites nominate:
///
/// | source | `v` | `x` | verdict |
/// |---|---|---|---|
/// | `NonNegativeNumberTest.rho:115` `test_fail_on_overflow` | `9223372036854775757` | same | refuse |
/// | `MakeMintTest.rho:190` `test_overflow_deposit` | `9223372036854775807` | `1` | refuse |
///
/// ⚠ Watched RED at the BOUNDARY, which is where an off-by-one in the rearrangement would hide:
/// `v = i64::MAX - 1, x = 1` must be ACCEPTED (the sum is exactly `i64::MAX`) and `x = 2` must be
/// refused. A guard written `x < MAX - v` passes every refusal case above and fails that one.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn nonnegativenumber_add_refuses_and_restores_the_balance_and_is_exact_at_the_boundary() {
    with_runtime("nnn-add-guard-", |runtime| async move {
        // (store, v, x, expected_success, expected_balance_after)
        let cases: [(&str, i64, i64, bool, i64); 5] = [
            // `test_fail_on_overflow`: v = x = i64::MAX - 50.
            (
                "a",
                9223372036854775757,
                9223372036854775757,
                false,
                9223372036854775757,
            ),
            // `test_overflow_deposit`: a full purse takes a deposit of 1.
            ("b", 9223372036854775807, 1, false, 9223372036854775807),
            // ★ THE BOUNDARY, accepted: the sum is exactly i64::MAX.
            ("c", 9223372036854775806, 1, true, 9223372036854775807),
            // ★ THE BOUNDARY, refused: one past.
            ("d", 9223372036854775806, 2, false, 9223372036854775806),
            // THE CONTROL: an ordinary sum still goes through.
            ("e", 10, 32, true, 42),
        ];

        for (tag, v, x, expect_success, expect_balance) in cases {
            let store = format!("store{tag}");
            let success = format!("success{tag}");
            let term = format!(
                r#"
                @"{store}"!({v}) |
                for (@v <- @"{store}") {{
                    if ({x} <= 9223372036854775807 - v) {{
                        @"{store}"!(v + {x}) | @"{success}"!(true)
                    }} else {{
                        @"{store}"!(v) | @"{success}"!(false)
                    }}
                }}
                "#
            );
            inject(&runtime, &term).await.unwrap_or_else(|err| {
                panic!("★ the total guard must never raise — v={v}, x={x}: {err}")
            });

            assert_eq!(
                bools_on(&runtime, &success).await,
                vec![expect_success],
                "★ v={v}, x={x}: `add` must report {expect_success}",
            );
            assert_eq!(
                int_on(&runtime, &store).await,
                vec![expect_balance],
                "★ v={v}, x={x}: the balance must be {expect_balance} — the `else` arm's job is \
                 to put the consumed balance BACK, and the `then` arm's is to store the sum",
            );
        }
    })
    .await
}
