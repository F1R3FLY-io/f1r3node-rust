//! ★★ **What a RhoSpec fixture CLAIMS to test, derived from the fixture itself.**
//!
//! # The vacuity this exists to make unrepresentable
//!
//! [`crate::helper::rho_spec::RhoSpec::run_tests`] used to iterate `TestResult::assertions` and
//! call `mk_test` per entry. An empty map is zero iterations, so a suite that verified **nothing**
//! reported PASS. Measured 2026-07-29 across `cargo nextest run -p casper --test mod -E
//! 'test(/genesis::contracts::/)'`: 40 tests ran, 40 passed, and exactly two of them — the two
//! that share `FailingResultCollectorTest.rho`, the one fixture that performs no registry lookup —
//! collected any assertion at all. All sixteen `RhoSpec::run_tests` suites collected zero.
//!
//! Repairing the harness (see that module's `get_results`) is not enough on its own: the next
//! change to it can silently re-create the same hole. What closes the hole is a **floor** — a
//! lower bound on what a passing run must have observed — and a floor is only worth having if it
//! is *derived from the subject* rather than hand-maintained beside it.
//!
//! # The derivation
//!
//! A RhoSpec fixture hands the harness its own test list. `RhoSpecContract.rho` accepts it in
//! three arities:
//!
//! ```text
//! @RhoSpec!("testSuite",                     [ (name, *body), … ])
//! @RhoSpec!("testSuite", *setup,             [ (name, *body), … ])
//! @RhoSpec!("testSuite", *setup, *teardown,  [ (name, *body), … ])
//! ```
//!
//! and `assert!(testName, attempt, …)` inside `runTestOnce` passes exactly the `name` from that
//! tuple through to [`casper::rust::helper::test_result_collector::TestResultCollector`], which
//! keys `TestResult::assertions` by it. So the registered names and the observed keys are drawn
//! from *one* alphabet, and the floor is a set comparison:
//!
//! ```math
//! \mathrm{registered}(\text{fixture}) \;=\; \operatorname{dom}\bigl(\mathrm{assertions}\bigr)
//! ```
//!
//! Read left-to-right the equation says *every registered test reported*; read right-to-left it
//! says *the extractor below did not under-report*. One assertion, both directions — which is why
//! it is equality and not `⊆`. A fixture that registers five tests and reports two fails, and so
//! does an extractor that finds two registrations out of five.
//!
//! `registered` is computed from the fixture's **normalized term**, not from its text: the source
//! is put through the same [`Compiler`] the runtime uses, and the resulting `Par` is searched for
//! the send. Nothing is parsed twice and no regular expression is involved, so the extractor
//! agrees with the interpreter by construction on what the fixture says.
//!
//! # Why the walk cannot silently miss a case
//!
//! [`collect_registered_test_names`] matches **exhaustively** on every `Par` container and on
//! every `ExprInstance` / `ConnectiveInstance` variant, with no `_` arm anywhere. A `Send` is a
//! process, and a process can occur inside any of them (`{ x!(1) } == Nil` is a legal Rholang
//! expression), so completeness is not optional. Exhaustive matching moves the guarantee from
//! runtime to **compile time**: when `rhoapi.proto` grows a variant, this file stops compiling
//! rather than quietly returning a smaller set. That is the difference between a derived floor
//! and a hand-maintained mirror of a computable domain.
//!
//! ```text
//!                     ┌──────────────────────── the fixture ────────────────────────┐
//!                     │  @RhoSpec!("testSuite", *setup, [ ("a", *t_a), ("b", *t_b) ])│
//!                     └───────────────┬──────────────────────────┬──────────────────┘
//!                     Compiler::source_to_adt│                   │ deployed & evaluated
//!                                     ▼                          ▼
//!                        collect_registered_test_names    RhoSpecContract.rho
//!                                     │                          │ assert!(testName, …)
//!                                     ▼                          ▼
//!                              registered = {a, b}   ═══?═══  dom(assertions)
//!                                                 set equality
//! ```

use std::collections::BTreeSet;
use std::fmt;

use casper::rust::helper::test_result_collector::TestResult;
use models::rhoapi::connective::ConnectiveInstance;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{ETuple, Expr, Par};
use rholang::rust::interpreter::compiler::compiler::Compiler;
use rholang::rust::interpreter::errors::InterpreterError;

/// The method name `RhoSpecContract.rho` dispatches a suite registration on.
const TEST_SUITE_SELECTOR: &str = "testSuite";

/// A way in which a run failed to clear the non-vacuity floor.
///
/// ★ **A value, not a panic.** The floor's decision is a pure function of (what the fixture
/// registered, what the run reported), so it can be — and is — exercised directly by
/// `casper/tests/genesis/contracts/rho_spec_floor_spec.rs` at the exact inputs it must refuse.
/// A floor whose only expression is `assert!` inside a 15-second genesis run can only be watched
/// go red by breaking something, and a test that expects a panic is not an option here
/// (`panic!` in this workspace's proc-macro/bridge paths does not reliably unwind, and a
/// panic-expecting test cannot distinguish *which* panic it caught).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FloorBreach {
    /// The fixture never sends `"testSuite"`, so `RhoSpec` can never learn of a test to run.
    NoRegisteredTests,

    /// `testSuiteCompleted` never fired: at least one registered test body is still blocked.
    DidNotFinish {
        registered: BTreeSet<String>,
        reported: BTreeSet<String>,
    },

    /// A registered test produced no assertion, so nothing about it was checked.
    RegisteredButSilent {
        silent: BTreeSet<String>,
        registered: BTreeSet<String>,
    },

    /// An assertion arrived under a name the fixture never registered — which means
    /// [`registered_test_names`] missed a registration, and the floor is weaker than it looks.
    ReportedButUnregistered {
        unregistered: BTreeSet<String>,
        registered: BTreeSet<String>,
    },
}

impl fmt::Display for FloorBreach {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FloorBreach::NoRegisteredTests => write!(
                f,
                "registers NO tests with RhoSpec, so running it can verify nothing and passing it \
                 would mean nothing.\n\
                 A RhoSpec suite reaches the harness by sending `@RhoSpec!(\"testSuite\", [ \
                 (name, *body), … ])` — see `casper/src/test/resources/RhoSpecContract.rho`. This \
                 fixture sends no such message. Either give it a `testSuite` registration, or \
                 drive it with something other than `RhoSpec::run_tests`."
            ),

            FloorBreach::DidNotFinish {
                registered,
                reported,
            } => write!(
                f,
                "did not run to completion: `testSuiteCompleted` never fired.\n\
                 `RhoSpecContract.rho` sends it only after `ListOps.foreach` has walked the whole \
                 registered list, so at least one test body is still blocked — a `for` that never \
                 received, a registry lookup that never answered, or a deadlocked join.\n\
                 registered {} test(s): {registered:?}\n\
                 reported   {} test(s): {reported:?}",
                registered.len(),
                reported.len(),
            ),

            FloorBreach::RegisteredButSilent { silent, registered } => write!(
                f,
                "registered {} test(s) and {} of them reported NO assertion, so nothing about \
                 them was checked whatever the exit status says.\n\
                 silent: {silent:?}",
                registered.len(),
                silent.len(),
            ),

            FloorBreach::ReportedButUnregistered {
                unregistered,
                registered,
            } => write!(
                f,
                "reported assertions under {} name(s) it never registered, which means \
                 `rho_spec_suite_manifest::registered_test_names` failed to see a registration \
                 and the floor is weaker than it looks.\n\
                 unregistered: {unregistered:?}\n\
                 registered {}: {registered:?}",
                unregistered.len(),
                registered.len(),
            ),
        }
    }
}

/// ★ Gate 1, checked BEFORE the run because a fixture that registers nothing cannot be salvaged
/// by fifteen seconds of genesis.
pub fn check_registration(registered: &BTreeSet<String>) -> Result<(), FloorBreach> {
    match registered.is_empty() {
        true => Err(FloorBreach::NoRegisteredTests),
        false => Ok(()),
    }
}

/// ★ Gates 2 and 3, checked after the run.
///
/// Gate 2 is `has_finished`, which [`TestResult`] has always recorded and which nothing on the
/// `run_tests` path ever read. Gate 3 is set **equality** between the registered names and the
/// names that reported: `⊇` catches a test that silently stopped asserting, `⊆` catches an
/// under-reporting extractor. `DidNotFinish` is reported first when both break, because a blocked
/// body explains the missing reports and the reverse is not true.
pub fn check_report(
    registered: &BTreeSet<String>,
    result: &TestResult,
) -> Result<(), FloorBreach> {
    let reported: BTreeSet<String> = result.assertions.keys().cloned().collect();

    match result.has_finished {
        false => {
            return Err(FloorBreach::DidNotFinish {
                registered: registered.clone(),
                reported,
            })
        }
        true => {}
    }

    let silent: BTreeSet<String> = registered.difference(&reported).cloned().collect();
    match silent.is_empty() {
        false => {
            return Err(FloorBreach::RegisteredButSilent {
                silent,
                registered: registered.clone(),
            })
        }
        true => {}
    }

    let unregistered: BTreeSet<String> = reported.difference(registered).cloned().collect();
    match unregistered.is_empty() {
        false => Err(FloorBreach::ReportedButUnregistered {
            unregistered,
            registered: registered.clone(),
        }),
        true => Ok(()),
    }
}

/// The set of test names a fixture registers with `RhoSpec`, in name order.
///
/// Returns the empty set for a fixture that never sends `"testSuite"` — a fixture that does not
/// use the RhoSpec protocol at all. Callers treat that as a failure rather than as permission to
/// verify nothing; see `RhoSpec::run_tests`.
///
/// # Errors
///
/// Propagates the normalizer's error if `source` does not compile. A fixture that will not
/// normalize cannot be run either, so this is the same failure the run would hit.
pub fn registered_test_names(source: &str) -> Result<BTreeSet<String>, InterpreterError> {
    let term = Compiler::source_to_adt(source)?;
    let mut names = BTreeSet::new();
    collect_registered_test_names(&term, &mut names);
    Ok(names)
}

/// Depth-first search of `root` for `"testSuite"` sends, accumulating the registered names.
///
/// Iterative rather than recursive: a fixture is arbitrarily deeply nested (`PoSTest.rho` reaches
/// eight `new`/`for` levels before its registration) and this runs inside a `#[tokio::test]`
/// worker whose stack is not the main thread's.
pub fn collect_registered_test_names(root: &Par, names: &mut BTreeSet<String>) {
    let mut work: Vec<&Par> = vec![root];

    while let Some(par) = work.pop() {
        for send in &par.sends {
            if send.data.iter().any(is_test_suite_selector) {
                harvest_registration(&send.data, names);
            }
            work.extend(send.chan.as_ref());
            work.extend(send.data.iter());
        }

        for receive in &par.receives {
            for bind in &receive.binds {
                work.extend(bind.patterns.iter());
                work.extend(bind.source.as_ref());
            }
            work.extend(receive.body.as_ref());
            work.extend(receive.condition.as_ref());
        }

        for new in &par.news {
            work.extend(new.p.as_ref());
        }

        for r#match in &par.matches {
            work.extend(r#match.target.as_ref());
            for case in &r#match.cases {
                work.extend(case.pattern.as_ref());
                work.extend(case.source.as_ref());
                work.extend(case.guard.as_ref());
            }
        }

        for bundle in &par.bundles {
            work.extend(bundle.body.as_ref());
        }

        for conditional in &par.conditionals {
            work.extend(conditional.condition.as_ref());
            work.extend(conditional.if_true.as_ref());
            work.extend(conditional.if_false.as_ref());
        }

        for expr in &par.exprs {
            push_expr_children(expr, &mut work);
        }

        for connective in &par.connectives {
            push_connective_children(connective, &mut work);
        }
    }
}

/// The `[ (name, *body), … ]` argument of a `"testSuite"` send, harvested for its names.
///
/// Scans every argument rather than a fixed position because the selector is followed by zero,
/// one, or two process arguments (`setup`, `teardown`) before the list. A `"testSuite"` send
/// carries exactly one list, so scanning all arguments cannot pick up a second one.
fn harvest_registration(data: &[Par], names: &mut BTreeSet<String>) {
    for argument in data {
        for expr in &argument.exprs {
            let Some(ExprInstance::EListBody(list)) = &expr.expr_instance else {
                continue;
            };
            for element in &list.ps {
                if let Some(name) = registered_name_of(element) {
                    names.insert(name);
                }
            }
        }
    }
}

/// The leading string of a `(name, *body)` (or `(name, *body, attempts)`) registration tuple.
fn registered_name_of(element: &Par) -> Option<String> {
    let [Expr {
        expr_instance: Some(ExprInstance::ETupleBody(ETuple { ps, .. })),
    }] = element.exprs.as_slice()
    else {
        return None;
    };
    match ps.first()?.exprs.as_slice() {
        [Expr {
            expr_instance: Some(ExprInstance::GString(name)),
        }] => Some(name.clone()),
        _ => None,
    }
}

/// Whether this argument is the literal `"testSuite"`.
fn is_test_suite_selector(argument: &Par) -> bool {
    matches!(
        argument.exprs.as_slice(),
        [Expr {
            expr_instance: Some(ExprInstance::GString(selector)),
        }] if selector == TEST_SUITE_SELECTOR
    )
}

/// Every `Par` reachable from an expression, pushed onto the worklist.
///
/// ⚠ **The match is exhaustive on purpose and must stay that way.** No `_` arm: a new
/// `ExprInstance` variant must break this build, because a variant handled by a wildcard is a
/// variant whose sub-terms are never searched, and an under-reporting extractor turns the floor
/// in `RhoSpec::run_tests` back into the vacuity it replaced.
fn push_expr_children<'a>(expr: &'a Expr, work: &mut Vec<&'a Par>) {
    let Some(instance) = &expr.expr_instance else {
        return;
    };

    match instance {
        // Ground terms carry no sub-process.
        ExprInstance::GBool(_)
        | ExprInstance::GInt(_)
        | ExprInstance::GString(_)
        | ExprInstance::GUri(_)
        | ExprInstance::GByteArray(_)
        | ExprInstance::GDouble(_)
        | ExprInstance::GBigInt(_)
        | ExprInstance::GBigRat(_)
        | ExprInstance::GFixedPoint(_) => {}

        // A variable reference names a binder, never a process.
        ExprInstance::EVarBody(_) => {}

        // Unary.
        ExprInstance::ENotBody(e) => work.extend(e.p.as_ref()),
        ExprInstance::ENegBody(e) => work.extend(e.p.as_ref()),

        // Binary — arithmetic, comparison, logical, string, collection.
        ExprInstance::EMultBody(e) => push_pair(work, &e.p1, &e.p2),
        ExprInstance::EDivBody(e) => push_pair(work, &e.p1, &e.p2),
        ExprInstance::EModBody(e) => push_pair(work, &e.p1, &e.p2),
        ExprInstance::EPlusBody(e) => push_pair(work, &e.p1, &e.p2),
        ExprInstance::EMinusBody(e) => push_pair(work, &e.p1, &e.p2),
        ExprInstance::ELtBody(e) => push_pair(work, &e.p1, &e.p2),
        ExprInstance::ELteBody(e) => push_pair(work, &e.p1, &e.p2),
        ExprInstance::EGtBody(e) => push_pair(work, &e.p1, &e.p2),
        ExprInstance::EGteBody(e) => push_pair(work, &e.p1, &e.p2),
        ExprInstance::EEqBody(e) => push_pair(work, &e.p1, &e.p2),
        ExprInstance::ENeqBody(e) => push_pair(work, &e.p1, &e.p2),
        ExprInstance::EAndBody(e) => push_pair(work, &e.p1, &e.p2),
        ExprInstance::EOrBody(e) => push_pair(work, &e.p1, &e.p2),
        ExprInstance::EPercentPercentBody(e) => push_pair(work, &e.p1, &e.p2),
        ExprInstance::EPlusPlusBody(e) => push_pair(work, &e.p1, &e.p2),
        ExprInstance::EMinusMinusBody(e) => push_pair(work, &e.p1, &e.p2),
        ExprInstance::EMatchesBody(e) => push_pair(work, &e.target, &e.pattern),

        // Collection literals hold quoted processes directly.
        ExprInstance::EListBody(e) => work.extend(e.ps.iter()),
        ExprInstance::ETupleBody(e) => work.extend(e.ps.iter()),
        ExprInstance::ESetBody(e) => work.extend(e.ps.iter()),
        ExprInstance::EMapBody(e) => {
            for kv in &e.kvs {
                push_pair(work, &kv.key, &kv.value);
            }
        }
        ExprInstance::EPathmapBody(e) => work.extend(e.ps().iter()),
        ExprInstance::EZipperBody(e) => {
            if let Some(pathmap) = &e.pathmap {
                work.extend(pathmap.ps().iter());
            }
        }

        // A method call's receiver and arguments are processes.
        ExprInstance::EMethodBody(e) => {
            work.extend(e.target.as_ref());
            work.extend(e.arguments.iter());
        }
    }
}

/// Every `Par` reachable from a connective (a pattern-level term), pushed onto the worklist.
///
/// ⚠ Exhaustive for the same reason as [`push_expr_children`].
fn push_connective_children<'a>(
    connective: &'a models::rhoapi::Connective,
    work: &mut Vec<&'a Par>,
) {
    let Some(instance) = &connective.connective_instance else {
        return;
    };

    match instance {
        ConnectiveInstance::ConnAndBody(body) => work.extend(body.ps.iter()),
        ConnectiveInstance::ConnOrBody(body) => work.extend(body.ps.iter()),
        ConnectiveInstance::ConnNotBody(par) => work.push(par),
        // A variable reference and the type connectives carry no sub-process.
        ConnectiveInstance::VarRefBody(_)
        | ConnectiveInstance::ConnBool(_)
        | ConnectiveInstance::ConnInt(_)
        | ConnectiveInstance::ConnString(_)
        | ConnectiveInstance::ConnUri(_)
        | ConnectiveInstance::ConnByteArray(_) => {}
    }
}

fn push_pair<'a>(work: &mut Vec<&'a Par>, left: &'a Option<Par>, right: &'a Option<Par>) {
    work.extend(left.as_ref());
    work.extend(right.as_ref());
}
