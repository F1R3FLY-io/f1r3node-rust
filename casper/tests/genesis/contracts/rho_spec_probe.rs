//! ★★ **The RhoSpec PROBE — the instrument the genesis-contract cells measure with.**
//!
//! Extracted verbatim from [`super::genesis_contract_failure_mechanisms`], which is where it was
//! first built and where its design rationale was worked out. It is a module of its own because it
//! is an *instrument*, and a second file now measures with it: a harness that lives inside one
//! file's "here are the failures" narrative reads like part of that narrative rather than like
//! something a new cell is entitled to reuse.
//!
//! # ⚠ Why every probe goes through `get_results` and not through a bare `inj`
//!
//! **A Rholang program that blocks is not an error.** `runtime.inj(term, …)` returns `Ok(())` for a
//! program that reduced to a *stuck* normal form just as readily as for one that finished its work,
//! so `assert!(eval(code).is_ok())` is a **vacuous** control: it passes when the code under test
//! never ran. Measured the hard way — the first version of the mechanisms file asserted exactly
//! that, and its "control: the purse-log round trip is clean" was passing on a program that had gone
//! quiet.
//!
//! A synthetic **RhoSpec** suite fixes it, because `RhoSpecContract.rho` supplies a liveness signal:
//! `testSuiteCompleted` fires only after `ListOps.foreach` has walked the registered list, and each
//! `assert` is recorded by name. [`SuiteOutcome`] therefore reports *reached the end* separately
//! from *did not raise*, and no cell is allowed to conclude anything from the second alone.
//!
//! # The three questions, and which function answers them
//!
//! | question | answered by |
//! |---|---|
//! | did it raise, and did it reach `testSuiteCompleted`? | [`SuiteOutcome`], from [`run_suite`] |
//! | ...and did every recorded assertion PASS? | [`run_suite_with_verdicts`] |
//! | ...and what were the recorded values, so a RED names them? | [`run_suite_detailed`] |
//!
//! The three are kept distinct on purpose. `SuiteOutcome` deliberately does **not** carry verdicts:
//! conflating "reached the end" with "passed" is what made the original probe vacuous, and a cell
//! that only needs block-vs-raise should not be able to accidentally read a pass/fail it did not
//! establish. Conversely a cell that pins a VALUE must not be satisfiable by a suite that recorded
//! nothing, which is why the verdict is `Option<bool>` — `None` for "no assertion ran" — and never a
//! bare `bool`.

use std::collections::{BTreeSet, HashMap};
use std::time::Duration;

use casper::rust::helper::test_result_collector::{RhoTestAssertion, TestResultCollector};
use rholang::rust::build::compile_rholang_source::CompiledRholangSource;
use rholang::rust::interpreter::errors::InterpreterError;
use rholang::rust::interpreter::pretty_printer::PrettyPrinter;

use crate::helper::rho_spec::get_results;
use crate::util::genesis_builder::GenesisBuilder;

/// How long a probe suite gets. Each probe is a handful of COMM events once genesis is built, so a
/// suite still running after this has blocked rather than slowed down.
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(60);

/// The name every probe registers, so `Completed { reported }` is checkable against one constant.
pub const PROBE_TEST: &str = "the probe";

/// What a probe suite did, with *raised* and *reached the end* reported separately.
#[derive(Debug)]
pub enum SuiteOutcome {
    /// The suite ran to `testSuiteCompleted` and reported these test names.
    Completed { reported: BTreeSet<String> },
    /// The suite reduced to a normal form without completing: something blocked.
    /// `reported` is what it managed to assert before going quiet.
    Blocked { reported: BTreeSet<String> },
    /// The suite raised.
    Raised(InterpreterError),
}

impl SuiteOutcome {
    pub fn is_completed(&self) -> bool { matches!(self, SuiteOutcome::Completed { .. }) }

    pub fn raise_text(&self) -> Option<String> {
        match self {
            SuiteOutcome::Raised(e) => Some(format!("{e:?}")),
            _ => None,
        }
    }

    /// The reported names, for cells that want to compare them regardless of whether the suite
    /// finished. `None` for a raise, where "which tests reported" is not a meaningful question.
    pub fn reported_set(&self) -> Option<BTreeSet<String>> {
        match self {
            SuiteOutcome::Completed { reported } | SuiteOutcome::Blocked { reported } => {
                Some(reported.clone())
            }
            SuiteOutcome::Raised(_) => None,
        }
    }
}

/// One recorded RhoSpec assertion, rendered so a Rust-side failure message can NAME what the
/// Rholang under test actually observed.
///
/// ⚠ Without this, a value-pinning cell's RED reads "the suite reported a failure" and the reader
/// has to re-run under `--no-capture` and read the interpreter's log to learn *which* value was
/// wrong. `RhoTestAssertion::RhoAssertEquals` already carries `expected` and `actual` as `Par`s;
/// this renders them with the interpreter's own `PrettyPrinter`, so the RED text is in the same
/// notation as the contract under test.
#[derive(Debug, Clone)]
pub struct AssertionRecord {
    pub clue: String,
    pub passed: bool,
    /// `expected` vs `actual` in Rholang notation, or the boolean for a bare `assert`.
    pub detail: String,
}

impl std::fmt::Display for AssertionRecord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "[{}] {} — {}",
            match self.passed {
                true => "PASS",
                false => "FAIL",
            },
            self.clue,
            self.detail,
        )
    }
}

fn render(assertion: &RhoTestAssertion) -> AssertionRecord {
    let mut printer = PrettyPrinter::new();
    let detail = match assertion {
        RhoTestAssertion::RhoAssertTrue { is_success, .. } => {
            format!("assert({is_success})")
        }
        RhoTestAssertion::RhoAssertEquals {
            expected, actual, ..
        } => format!(
            "expected {} , actual {}",
            printer.build_string_from_message(expected),
            PrettyPrinter::new().build_string_from_message(actual),
        ),
        RhoTestAssertion::RhoAssertNotEquals {
            unexpected, actual, ..
        } => format!(
            "unexpected {} , actual {}",
            printer.build_string_from_message(unexpected),
            PrettyPrinter::new().build_string_from_message(actual),
        ),
    };
    AssertionRecord {
        clue: assertion.clue().to_string(),
        passed: assertion.is_success(),
        detail,
    }
}

/// Compile `source`, then drive it through the harness's own `get_results`.
///
/// Deliberately NOT `RhoSpec::run_tests`: the non-vacuity floor would turn a probe's deliberately
/// partial suite into a panic, and what a probe wants is the outcome as a value.
async fn drive(
    source: &str,
) -> Result<casper::rust::helper::test_result_collector::TestResult, InterpreterError> {
    let compiled = CompiledRholangSource::new(
        source.to_string(),
        HashMap::new(),
        "<probe suite>".to_string(),
    )
    .map_err(|e| InterpreterError::BugFoundError(format!("probe source must compile: {e:?}")))?;

    let collector = std::sync::Arc::new(TestResultCollector::new());
    get_results(
        &compiled,
        &[],
        PROBE_TIMEOUT,
        GenesisBuilder::build_genesis_parameters_with_defaults(None, None),
        collector,
    )
    .await
}

/// [`drive`], reduced to *raised* / *blocked* / *completed*.
pub async fn run_suite(source: &str) -> SuiteOutcome {
    match drive(source).await {
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

/// [`run_suite`], plus the **verdicts** — which [`SuiteOutcome`] deliberately does not carry.
///
/// Returned as `Some(all_succeeded)` when at least one assertion was recorded, and `None` when none
/// was — so "no assertions ran" can never be mistaken for "every assertion passed", which is the
/// vacuity this whole instrument exists to avoid.
pub async fn run_suite_with_verdicts(source: &str) -> (SuiteOutcome, Option<bool>) {
    let (outcome, records) = run_suite_detailed(source).await;
    let verdict = match records.len() {
        0 => None,
        _ => Some(records.iter().all(|r| r.passed)),
    };
    (outcome, verdict)
}

/// [`run_suite`], plus every recorded assertion RENDERED — the form a value-pinning cell wants,
/// because its RED can then print the observation instead of merely reporting that one existed.
pub async fn run_suite_detailed(source: &str) -> (SuiteOutcome, Vec<AssertionRecord>) {
    match drive(source).await {
        Err(e) => (SuiteOutcome::Raised(e), Vec::new()),
        Ok(result) => {
            // Preallocation: one record per (test, attempt, assertion) triple.
            let capacity: usize = result
                .assertions
                .values()
                .flat_map(|attempts| attempts.values())
                .map(|assertions| assertions.len())
                .sum();
            let mut records = Vec::with_capacity(capacity);
            for attempts in result.assertions.values() {
                for assertions in attempts.values() {
                    for assertion in assertions {
                        records.push(render(assertion));
                    }
                }
            }

            let reported = result.assertions.keys().cloned().collect();
            let outcome = match result.has_finished {
                true => SuiteOutcome::Completed { reported },
                false => SuiteOutcome::Blocked { reported },
            };
            (outcome, records)
        }
    }
}

/// Render a slice of records for a failure message, one per line.
pub fn describe(records: &[AssertionRecord]) -> String {
    match records.is_empty() {
        true => "    (no assertion was recorded at all)".to_string(),
        false => records
            .iter()
            .map(|r| format!("    {r}"))
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

/// A one-test RhoSpec suite: `setup` and `body` spliced into the standard preamble.
///
/// `rho:id:zphjg…` is `RhoSpecContract.rho`'s registered URI (its own header table, row 8).
///
/// `rl` (`rho:registry:lookup`) is in scope inside `body`, so a probe may look up any blessed
/// contract the genesis ceremony published — which is how the TreeHashMap cells reach
/// `rho:lang:treeHashMap`.
pub fn one_test_suite(test_name: &str, setup: &str, body: &str) -> String {
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

/// The singleton set `{name}` — what `Completed { reported }` must equal for a one-test suite.
pub fn only(name: &str) -> BTreeSet<String> {
    let mut set = BTreeSet::new();
    set.insert(name.to_string());
    set
}
