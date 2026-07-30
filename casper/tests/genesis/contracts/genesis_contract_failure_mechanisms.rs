//! ★★ **The mechanism of every genesis suite that goes red now that the suites RUN.**
//!
//! Repairing `RhoSpec::get_results` (`92a6f36c`) made sixteen suites execute for the first time.
//! Twelve pass. This file is the measurement for the ones that do not: each cell drives the
//! **same** genesis-scoped runtime and the **same** `get_results` the suites use, against a
//! synthetic one-test suite that reproduces the failure, and pins the outcome.
//!
//! ⚠ **These cells pin BEHAVIOUR, not approval.** Where a cell records that a blessed contract
//! raises, the pin exists so the defect cannot be lost and so a future fix has something to turn
//! green. It is not a statement that raising is correct.
//!
//! # ⚠ Why every probe goes through `get_results` and not through a bare `inj`
//!
//! **A Rholang program that blocks is not an error.** `runtime.inj(term, …)` returns `Ok(())` for a
//! program that reduced to a *stuck* normal form just as readily as for one that finished its work,
//! so `assert!(eval(code).is_ok())` is a **vacuous** control: it passes when the code under test
//! never ran. Measured the hard way — the first version of this file asserted exactly that, and its
//! "control: the purse-log round trip is clean" was passing on a program that had gone quiet. That
//! is the same defect as the one this whole work item is about, reproduced inside its own probe.
//!
//! A synthetic **RhoSpec** suite fixes it, because `RhoSpecContract.rho` supplies a liveness
//! signal: `testSuiteCompleted` fires only after `ListOps.foreach` has walked the registered list,
//! and each `assert` is recorded by name. [`SuiteOutcome`] therefore reports *reached the end*
//! separately from *did not raise*, and no cell here is allowed to conclude anything from the
//! second alone.
//!
//! # Why probes and not a bisected `.rho`
//!
//! `InterpreterError::ReduceError` carries no source position — `"Error: Multiple expressions
//! given."` names neither the term nor the file. Narrowing a 14-test fixture by editing it would
//! mutate the corpus under measurement; narrowing by *reconstruction* leaves the corpus alone and
//! produces a permanent, self-contained statement of the mechanism.

use std::collections::{BTreeSet, HashMap};
use std::time::Duration;

use casper::rust::helper::test_result_collector::TestResultCollector;
use rholang::rust::build::compile_rholang_source::CompiledRholangSource;
use rholang::rust::interpreter::errors::InterpreterError;

use crate::helper::rho_spec::get_results;
use crate::util::genesis_builder::GenesisBuilder;

/// How long a probe suite gets. Each probe is a handful of COMM events once genesis is built, so a
/// suite still running after this has blocked rather than slowed down.
const PROBE_TIMEOUT: Duration = Duration::from_secs(60);

/// What a probe suite did, with *raised* and *reached the end* reported separately.
#[derive(Debug)]
enum SuiteOutcome {
    /// The suite ran to `testSuiteCompleted` and reported these test names.
    Completed { reported: BTreeSet<String> },
    /// The suite reduced to a normal form without completing: something blocked.
    /// `reported` is what it managed to assert before going quiet.
    Blocked { reported: BTreeSet<String> },
    /// The suite raised.
    Raised(InterpreterError),
}

impl SuiteOutcome {
    fn is_completed(&self) -> bool { matches!(self, SuiteOutcome::Completed { .. }) }

    fn raise_text(&self) -> Option<String> {
        match self {
            SuiteOutcome::Raised(e) => Some(format!("{e:?}")),
            _ => None,
        }
    }
}

/// Drive a synthetic RhoSpec suite through the harness's own `get_results`.
///
/// Deliberately NOT `RhoSpec::run_tests`: the non-vacuity floor would turn a probe's deliberately
/// partial suite into a panic, and what a probe wants is the outcome as a value.
async fn run_suite(source: &str) -> SuiteOutcome {
    let compiled = match CompiledRholangSource::new(
        source.to_string(),
        HashMap::new(),
        "<probe suite>".to_string(),
    ) {
        Ok(compiled) => compiled,
        Err(e) => {
            return SuiteOutcome::Raised(InterpreterError::BugFoundError(format!(
                "probe source must compile: {e:?}"
            )))
        }
    };

    let collector = std::sync::Arc::new(TestResultCollector::new());
    match get_results(
        &compiled,
        &[],
        PROBE_TIMEOUT,
        GenesisBuilder::build_genesis_parameters_with_defaults(None, None),
        collector,
    )
    .await
    {
        Err(e) => SuiteOutcome::Raised(e),
        Ok(result) => {
            let reported = result.assertions.keys().cloned().collect();
            match result.has_finished {
                true => SuiteOutcome::Completed { reported },
                false => SuiteOutcome::Blocked { reported },
            }
        }
    }
}

/// A one-test RhoSpec suite: `setup` and `body` spliced into the standard preamble.
///
/// `rho:id:zphjg…` is `RhoSpecContract.rho`'s registered URI (its own header table, row 8).
fn one_test_suite(test_name: &str, setup: &str, body: &str) -> String {
    format!(
        r#"
new rl(`rho:registry:lookup`), RhoSpecCh, setup, probeTest in {{
  rl!(`rho:id:zphjgsfy13h1k85isc8rtwtgt3t9zzt5pjd5ihykfmyapfc4wt3x5h`, *RhoSpecCh) |
  for(@(_, RhoSpec) <- RhoSpecCh) {{
    @RhoSpec!("testSuite", *setup, [("{test_name}", *probeTest)])
  }} |
  contract setup(_, retCh) = {{
{setup}
  }} |
  contract probeTest(rhoSpec, setupResult, ackCh) = {{
{body}
  }}
}}
"#
    )
}

/// The name every probe registers, so `Completed { reported }` is checkable against one constant.
const PROBE_TEST: &str = "the probe";

fn only(name: &str) -> BTreeSet<String> {
    let mut set = BTreeSet::new();
    set.insert(name.to_string());
    set
}

// ════════════════════════════════════════════════════════════════════════════════════════════════
// (a) `MakeMint.rho`'s LOGGING path raises — `make_mint_spec`, `test_log_set`
// ════════════════════════════════════════════════════════════════════════════════════════════════

/// ★★ **`MakeMintTest.rho` ITSELF, one registered test at a time — `test_log_set` raises.**
///
/// `make_mint_spec` dies inside `test_log_set` ("be able to set a log channel and receive
/// notifications") with `ReduceError("Error: parallel or non expression found where expression
/// expected.")`, before its first assertion.
///
/// Every logging site in `MakeMint.rho` is guarded by `if (Nil != *logCh)` — lines 61, 71, 84, 144,
/// 159, 170, 181 — and the default is `logStore!(Nil)` at line 34. So with logging off, none of that
/// code is evaluated: `test_deposit` and `test_split` drive the same `deposit`/`decr` contracts to a
/// pass while taking the other branch. **`MakeMint.rho`'s logging branch has never been evaluated
/// by any test in this repository**, because `test_log_set` is the only test that calls `setLog` and
/// it has never run.
///
/// ⚠ **Why this cell runs the fixture instead of a reconstruction, and what that cost.** Two
/// hand-built reconstructions of `test_log_set` were measured and **neither reproduced**: setting
/// logs on both purses, depositing, and reading both log entries completes cleanly when the mint
/// arrives through a `match` on the setup result rather than through the fixture's
/// `contract test_log_set(rhoSpec, @(mintA, _), ackCh)` formal. So "the logging branch raises" is
/// **not** a sufficient statement of the mechanism — some further interaction with the fixture's own
/// shape is required, and a cell that claimed otherwise would be asserting more than was measured.
///
/// What *is* reproducible is the fixture, run one registered test at a time. The list is narrowed by
/// [`with_single_registered_test`], which rewrites the `testSuite` list in the loaded source and
/// then **verifies the rewrite with the extractor that the non-vacuity floor uses** — so the
/// narrowing is checked, not assumed. `test_deposit` is the control: same file, same setup, same
/// `deposit` contract, log left at its `Nil` default.
#[tokio::test]
async fn makemint_test_log_set_raises_where_test_deposit_passes() {
    let fixture = crate::util::rholang::test_rho_loader::load_test_rho("MakeMintTest.rho")
        .expect("MakeMintTest.rho must be loadable");

    const LOG_SET: &str = "be able to set a log channel and receive notifications";
    const DEPOSIT: &str = "Deposit should work as expected";

    let control_source = with_single_registered_test(&fixture, DEPOSIT);
    let subject_source = with_single_registered_test(&fixture, LOG_SET);

    let control = run_suite(&control_source).await;
    println!("control  ({DEPOSIT}): {control:?}");
    assert!(
        control.is_completed(),
        "★ CONTROL: `{DEPOSIT}` alone must COMPLETE — it exercises the same `deposit` contract with \
         logging at its default. If it does not, the failure is not specific to the log channel. \
         Got {control:?}",
    );
    assert_eq!(
        match &control {
            SuiteOutcome::Completed { reported } => reported.clone(),
            other => panic!("checked above: {other:?}"),
        },
        only(DEPOSIT),
        "★ CONTROL: exactly the narrowed test must have reported",
    );

    let subject = run_suite(&subject_source).await;
    println!("subject  ({LOG_SET}): {subject:?}");
    let raise = subject.raise_text().unwrap_or_else(|| {
        panic!(
            "★★ MEASURED 2026-07-29: `MakeMintTest.rho`'s `{LOG_SET}` RAISES on its own, while \
             `{DEPOSIT}` from the same file completes. If it no longer raises the defect has been \
             fixed — delete this cell and let `make_mint_spec` be the guard, since the fixture \
             covers it directly. Got {subject:?}"
        )
    });
    assert!(
        raise.contains("parallel or non expression found where expression expected"),
        "★ the failure must still be the expression-position one `make_mint_spec` reports; a \
         DIFFERENT error means the mechanism moved and this cell's analysis needs redoing. \
         Got {raise}",
    );
}

/// Rewrite a fixture's `testSuite` registration list down to the single entry naming `keep`.
///
/// ★ The rewrite is **verified, not assumed**: the result is put through
/// [`crate::helper::rho_spec_suite_manifest::registered_test_names`] — the same extractor the
/// non-vacuity floor derives its expectations from — and the extracted set must be exactly
/// `{keep}`. A narrowing that silently kept the whole list, or dropped everything, cannot pass.
fn with_single_registered_test(source: &str, keep: &str) -> String {
    use crate::helper::rho_spec_suite_manifest::registered_test_names;

    let selector = source
        .find(r#""testSuite""#)
        .expect("a RhoSpec fixture sends \"testSuite\"");
    let open = selector
        + source[selector..]
            .find('[')
            .expect("the registration list follows the selector");

    let bytes = source.as_bytes();
    let mut depth = 0usize;
    let mut close = None;
    for (offset, byte) in bytes[open..].iter().enumerate() {
        match byte {
            b'[' => depth += 1,
            b']' => {
                depth -= 1;
                if depth == 0 {
                    close = Some(open + offset);
                    break;
                }
            }
            _ => {}
        }
    }
    let close = close.expect("the registration list's bracket is balanced");

    // The `(name, *body)` entry naming `keep`, taken verbatim from the list so the body's
    // identifier does not have to be guessed.
    let list = &source[open..=close];
    let entry_start = list
        .find(&format!("(\"{keep}\""))
        .unwrap_or_else(|| panic!("the fixture must register {keep:?}; list was {list}"));
    let entry_end = entry_start
        + list[entry_start..]
            .find(')')
            .expect("a registration entry is a closed tuple");
    let entry = &list[entry_start..=entry_end];

    let narrowed = format!("{}[{entry}]{}", &source[..open], &source[close + 1..]);

    let extracted = registered_test_names(&narrowed)
        .expect("the narrowed fixture must still normalize");
    assert_eq!(
        extracted,
        only(keep),
        "★ FLOOR for the narrowing: after rewriting the list the fixture must register EXACTLY \
         {keep:?}. Anything else means the rewrite missed, and the measurement below would be of \
         the wrong program.",
    );

    narrowed
}

// ════════════════════════════════════════════════════════════════════════════════════════════════
// (b) `TreeHashMapTest.rho`'s `test_update_after_delete` expects `Nil + 1` to be silent
// ════════════════════════════════════════════════════════════════════════════════════════════════

/// ★★ **`Nil + 1` RAISES; it does not "yield no result".**
///
/// `TreeHashMapTest.rho:385` (`test_update_after_delete`) carries the comment
///
/// > With the same `val + 1` update body that yields no result on Nil input, get must return Nil
///
/// and that premise is false. `combine_plus`
/// (`rholang/src/rust/interpreter/reduce.rs:3492`) dispatches on a pair of `Expr`s, and its
/// operands are produced by the coercion that rejects a `Par` holding **no** expression. `Nil` is
/// exactly that `Par`, and the arm it falls into is the catch-all that reports `"Error: Multiple
/// expressions given."` — a message that names the wrong condition for an operand that is empty
/// rather than plural, which is why the suite's failure was hard to place.
///
/// The consequence for the fixture is not "the update is a no-op": a raise inside `inj` aborts the
/// **whole** program, so `tree_hash_map_spec` dies at that test and the twelve tests registered
/// after it never run.
#[tokio::test]
async fn adding_to_nil_raises_rather_than_yielding_no_result() {
    const SETUP: &str = r#"    retCh!(Nil)"#;

    // The control: the same shape with an Int, so the refusal is attributable to the `Nil`.
    let on_int = one_test_suite(
        PROBE_TEST,
        SETUP,
        r#"    new ch in {
      ch!(41) |
      for (@val <- ch) {
        rhoSpec!("assert", (42, "==", val + 1), "41 + 1 is 42", *ackCh)
      }
    }"#,
    );

    let on_nil = one_test_suite(
        PROBE_TEST,
        SETUP,
        r#"    new ch in {
      ch!(Nil) |
      for (@val <- ch) {
        rhoSpec!("assert", (Nil, "==", val + 1), "Nil + 1 is unreachable", *ackCh)
      }
    }"#,
    );

    let int_outcome = run_suite(&on_int).await;
    assert!(
        int_outcome.is_completed(),
        "★ CONTROL: `41 + 1` must complete and assert; got {int_outcome:?}",
    );
    assert_eq!(
        match &int_outcome {
            SuiteOutcome::Completed { reported } => reported.clone(),
            other => panic!("checked above: {other:?}"),
        },
        only(PROBE_TEST),
        "★ CONTROL: the assertion must be recorded under the registered name",
    );

    let nil_outcome = run_suite(&on_nil).await;
    let raise = nil_outcome.raise_text().unwrap_or_else(|| {
        panic!(
            "★★ `Nil + 1` must RAISE — `TreeHashMapTest.rho:385`'s premise is that it is silent. \
             Got {nil_outcome:?}"
        )
    });
    assert!(
        raise.contains("Multiple expressions given"),
        "★ `Nil + 1` reports the catch-all arm of the operand coercion, which spells an EMPTY \
         operand as a plural one. Got {raise}",
    );
}

// ════════════════════════════════════════════════════════════════════════════════════════════════
// (a) `PoSTest.rho`'s `closeBlock` caller went stale when `PoS.rhox` grew two parameters
// ════════════════════════════════════════════════════════════════════════════════════════════════

/// ★★ **An ARITY MISMATCH: `PoSTest.rho:704` sends 3 arguments to a 5-argument contract.**
///
/// ```text
/// PoSTest.rho:704   @PoS!("closeBlock", sysAuthToken, *ackCh0)
/// PoS.rhox:1093     contract PoS(@"closeBlock", @sysAuthToken, mintList, feeConvertList, ackCh)
/// ```
///
/// A Rholang send whose arity does not match any contract for that channel never matches: it rests
/// as data, forever, with no error. `PoSTest.rho`'s local `contract closeBlock(ackCh)` therefore
/// never answers, so every one of its callers — lines 120, 227, 289, 297, 330, 448, 467, 532, 546,
/// 679 — blocks. `pos_spec` reports the first suite's assertions and then goes quiet, which the
/// non-vacuity floor catches as `has_finished == false`, "registered 14 test(s) … reported 1".
///
/// ★ HISTORY, which fixes the classification. `mintList` entered the signature at `879e75c5`
/// ("Stage B — validator phlogiston mint at epoch/bond + Σ⟦v⟧ dual-write seam") and
/// `feeConvertList` with Stage D; `git log -- casper/src/test/resources/PoSTest.rho` shows one
/// commit, `c36613aa`, the original workspace extraction. **The caller has never been updated**,
/// and nothing noticed because `pos_spec` has never run. This is a real defect latent for the whole
/// life of the harness, not a corpus expectation that was never true: the two-argument call was
/// correct until Stage B changed the contract under it.
#[tokio::test]
async fn pos_close_block_arity_mismatch_is_why_pos_test_blocks() {
    // Resolve PoS and a system auth token, the way `PoSTest.rho:33-44` does.
    const SETUP: &str = r#"    new PoSCh, makeSysAuthToken(`sys:test:authToken:make`), tokenCh in {
      rl!(`rho:system:pos`, *PoSCh) |
      makeSysAuthToken!(*tokenCh) |
      for (@(_, PoS) <- PoSCh & @token <- tokenCh) {
        retCh!((PoS, token))
      }
    }"#;

    // The fixture's call: selector + token + ack. Three arguments.
    const STALE_ARITY: &str = r#"    match *setupResult {
      (PoS, token) => {
        new ackCh0, setBlockData(`rho:test:block:data:set`), blockDataSet in {
          setBlockData!("blockNumber", 1, *blockDataSet) |
          for (_ <- blockDataSet) {
            @PoS!("closeBlock", token, *ackCh0) |
            for (@ack <- ackCh0) {
              rhoSpec!("assert", true, "closeBlock answered", *ackCh)
            }
          }
        }
      }
    }"#;

    // The contract's call: selector + token + mintList + feeConvertList + ack. Five arguments.
    const CURRENT_ARITY: &str = r#"    match *setupResult {
      (PoS, token) => {
        new ackCh0, mintListCh, feeConvertListCh,
            setBlockData(`rho:test:block:data:set`), blockDataSet in {
          setBlockData!("blockNumber", 1, *blockDataSet) |
          for (_ <- blockDataSet) {
            @PoS!("closeBlock", token, *mintListCh, *feeConvertListCh, *ackCh0) |
            for (@ack <- ackCh0) {
              rhoSpec!("assert", true, "closeBlock answered", *ackCh)
            }
          }
        }
      }
    }"#;

    let stale = run_suite(&one_test_suite(PROBE_TEST, SETUP, STALE_ARITY)).await;
    let current = run_suite(&one_test_suite(PROBE_TEST, SETUP, CURRENT_ARITY)).await;

    println!("stale arity   (3 args, as PoSTest.rho:704 sends): {stale:?}");
    println!("current arity (5 args, as PoS.rhox:1093 accepts): {current:?}");

    // ★★ THE CONTROL THAT MAKES IT A MEASUREMENT: the SAME program with two more channels
    // completes. Without this rung, "the three-argument call blocks" is consistent with
    // `closeBlock` being broken for every caller.
    assert!(
        current.is_completed(),
        "★★ `closeBlock` called with the arity `PoS.rhox:1093` declares must COMPLETE. If it does \
         not, the defect is in `closeBlock` itself and not only in its stale caller, and the \
         repair to `PoSTest.rho` will not be sufficient. Got {current:?}",
    );

    assert!(
        !stale.is_completed(),
        "★★ the three-argument call must NOT complete — it is what `PoSTest.rho:704` sends and it \
         is why `pos_spec` blocks. If it now completes, `PoS.rhox` has regained a three-argument \
         `closeBlock` and this cell's analysis needs redoing. Got {stale:?}",
    );
    assert!(
        stale.raise_text().is_none(),
        "★ an unmatched send RESTS; it does not raise. A raise here would be a different \
         mechanism from the block the floor reported for `pos_spec`. Got {stale:?}",
    );
}
