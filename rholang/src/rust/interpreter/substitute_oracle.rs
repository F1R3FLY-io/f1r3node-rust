//! # The recursive oracle twin for the substitution SCC, and its differential
//!
//! Leg-2 replaced substitution's call stack with an explicit worklist
//! ([`super::substitute_drive`]). This module keeps the **recursive** form
//! alive, under `cfg(test)`, and asserts that the two agree.
//!
//! This is the house standard, not a standard invented for this change: the
//! evaluator SCC's own trampoline landed with exactly this pattern
//! (`reduce.rs`, `eval_expr_recursive` at the oracle end and
//! `mod differential_trampoline` at the assertion end, commit `a929a2d6`), and
//! the audit's §8.3 names it as the obligation for any further conversion.
//!
//! ## What the twin is, precisely
//!
//! Every function here is the pre-Leg-2 body with two changes and no others:
//!
//! 1. It threads a [`SubCtx`] + [`EnvView`] instead of an owned `Env`. That is
//!    the *same* substitution the driver makes, so the twin does not
//!    independently validate the environment representation — the equality is
//!    argued by construction in [`super::substitute_drive`] and checked
//!    mechanically by
//!    `substitute_drive::representation_guard::the_view_agrees_with_a_chain_of_owned_shifts`.
//! 2. Its per-arm assembly goes through [`super::substitute_combine`], which
//!    the driver also uses. That is deliberate: it makes an arm's *semantics*
//!    single-sourced, so the differential tests the **driving** — push order,
//!    pop order, fold resumption, error position — which is what a worklist
//!    conversion can actually get wrong. The arms themselves are guarded by
//!    `substitute_combine::tests::expr_instance_round_trip_is_the_identity`,
//!    which is the test that catches the `EMinus → EPlus` class.
//!
//! ## What is compared
//!
//! The audit's §8.1 observables:
//!
//! 1. **Result bytes** — `encode_to_vec()` of the returned term, or an
//!    identical `Err` compared by `{:?}` payload.
//! 2. **The ordered charge trace** — `(BillableKind, weight)` pairs from the
//!    budget's canonical event log.
//! 3. **Aggregate cost** — `budget.total_cost()`.
//!
//! For this SCC, (2) and (3) are neutral *by construction*: the charge is
//! levied once by `substitute_and_charge` on `encoded_len()` of the **result**,
//! outside the recursion, and neither the driver nor the oracle contains a
//! single `reserve_*` or `Cost::` call. `charge_trace_is_empty_inside_the_scc`
//! asserts that mechanically rather than leaving it as a claim, and
//! `metered_wrappers_agree` compares the trace through the wrapper that does
//! charge.

use models::rhoapi::connective::ConnectiveInstance;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{
    Bundle, Connective, ConnectiveBody, Expr, If, Match, MatchCase, New, Par, Receive, ReceiveBind,
    Send,
};
use rspace_plus_plus::rspace::history::Either;

use super::env::Env;
use super::errors::InterpreterError;
use super::substitute::Substitute;
use super::substitute_combine::{
    fold_concatenate_par, fold_prepend_connective, fold_prepend_expr, missing_required_field,
    rebuild_bundle, rebuild_connective, rebuild_expr_instance, rebuild_if, rebuild_match,
    rebuild_match_case, rebuild_new, rebuild_par, rebuild_receive, rebuild_receive_bind,
    rebuild_send, split_expr_instance, ConnArm,
};
use super::substitute_drive::{
    maybe_substitute_evar_view, maybe_substitute_var_ref_view, EnvView, SubCtx,
};
use super::unwrap_option_safe;

// ===========================================================================
// the recursive twin
// ===========================================================================

pub(crate) fn par_recursive(term: Par, ctx: SubCtx, env: EnvView<'_>) -> Result<Par, InterpreterError> {
    let Par {
        sends,
        receives,
        news,
        exprs,
        matches,
        unforgeables,
        bundles,
        connectives,
        conditionals,
        locally_free,
        connective_used,
    } = term;

    let exprs_par = sub_exp_recursive(exprs, ctx, env)?;
    let connectives_par = sub_conn_recursive(connectives, ctx, env)?;

    let sends = sends
        .into_iter()
        .map(|s| send_recursive(s, ctx, env))
        .collect::<Result<Vec<Send>, InterpreterError>>()?;
    let bundles = bundles
        .into_iter()
        .map(|b| bundle_recursive(b, ctx, env))
        .collect::<Result<Vec<Bundle>, InterpreterError>>()?;
    let receives = receives
        .into_iter()
        .map(|r| receive_recursive(r, ctx, env))
        .collect::<Result<Vec<Receive>, InterpreterError>>()?;
    let news = news
        .into_iter()
        .map(|n| new_recursive(n, ctx, env))
        .collect::<Result<Vec<New>, InterpreterError>>()?;
    let matches = matches
        .into_iter()
        .map(|m| match_recursive(m, ctx, env))
        .collect::<Result<Vec<Match>, InterpreterError>>()?;
    let conditionals = conditionals
        .into_iter()
        .map(|i| if_recursive(i, ctx, env))
        .collect::<Result<Vec<If>, InterpreterError>>()?;

    Ok(rebuild_par(
        exprs_par,
        connectives_par,
        sends,
        receives,
        news,
        matches,
        bundles,
        conditionals,
        unforgeables,
        locally_free,
        connective_used,
        env.shift(ctx),
    ))
}

fn sub_exp_recursive(
    exprs: Vec<Expr>,
    ctx: SubCtx,
    env: EnvView<'_>,
) -> Result<Par, InterpreterError> {
    exprs.into_iter().try_fold(Par::default(), |acc, expr| {
        match expr.expr_instance {
            None => Err(missing_required_field::<ExprInstance>()),
            Some(ExprInstance::EVarBody(e)) => match maybe_substitute_evar_view(e, ctx, env)? {
                Either::Left(e) => Ok(fold_prepend_expr(
                    acc,
                    Expr {
                        expr_instance: Some(ExprInstance::EVarBody(e)),
                    },
                    ctx.depth,
                )),
                Either::Right(p) => Ok(fold_concatenate_par(acc, p)),
            },
            Some(other) => expr_recursive(
                Expr {
                    expr_instance: Some(other),
                },
                ctx,
                env,
            )
            .map(|e| fold_prepend_expr(acc, e, ctx.depth)),
        }
    })
}

fn sub_conn_recursive(
    conns: Vec<Connective>,
    ctx: SubCtx,
    env: EnvView<'_>,
) -> Result<Par, InterpreterError> {
    conns.into_iter().try_fold(Par::default(), |acc, conn| {
        let Some(instance) = conn.connective_instance else {
            return Ok(acc);
        };
        match instance {
            ConnectiveInstance::VarRefBody(v) => {
                match maybe_substitute_var_ref_view(v, ctx, env)? {
                    Either::Left(_) => Ok(fold_prepend_connective(
                        acc,
                        Connective {
                            connective_instance: Some(ConnectiveInstance::VarRefBody(v)),
                        },
                        ctx.depth,
                    )),
                    Either::Right(new_par) => Ok(fold_concatenate_par(acc, new_par)),
                }
            }
            ConnectiveInstance::ConnAndBody(ConnectiveBody { ps }) => {
                let sub_ps = map_pars_recursive(ps, ctx, env)?;
                Ok(fold_prepend_connective(
                    acc,
                    rebuild_connective(ConnArm::ConnAnd, sub_ps),
                    ctx.depth,
                ))
            }
            ConnectiveInstance::ConnOrBody(ConnectiveBody { ps }) => {
                let sub_ps = map_pars_recursive(ps, ctx, env)?;
                Ok(fold_prepend_connective(
                    acc,
                    rebuild_connective(ConnArm::ConnOr, sub_ps),
                    ctx.depth,
                ))
            }
            ConnectiveInstance::ConnNotBody(p) => {
                let sub_ps = map_pars_recursive(vec![p], ctx, env)?;
                Ok(fold_prepend_connective(
                    acc,
                    rebuild_connective(ConnArm::ConnNot, sub_ps),
                    ctx.depth,
                ))
            }
            ground => Ok(fold_prepend_connective(
                acc,
                rebuild_connective(ConnArm::Ground(ground), Vec::new()),
                ctx.depth,
            )),
        }
    })
}

fn map_pars_recursive(
    ps: Vec<Par>,
    ctx: SubCtx,
    env: EnvView<'_>,
) -> Result<Vec<Par>, InterpreterError> {
    ps.into_iter()
        .map(|p| par_recursive(p, ctx, env))
        .collect::<Result<Vec<Par>, InterpreterError>>()
}

/// Substitute an `Option<Par>` slot, raising `unwrap_option_safe`'s error for
/// a missing one — the same interleaving of check-then-descend the recursive
/// form performs operand by operand.
fn required_par_recursive(
    slot: Option<Par>,
    ctx: SubCtx,
    env: EnvView<'_>,
) -> Result<Par, InterpreterError> {
    par_recursive(unwrap_option_safe(slot)?, ctx, env)
}

pub(crate) fn expr_recursive(term: Expr, ctx: SubCtx, env: EnvView<'_>) -> Result<Expr, InterpreterError> {
    let instance = unwrap_option_safe(term.expr_instance)?;
    let (arm, children) = split_expr_instance(instance);

    // Check-then-descend, operand by operand, in slot order — the interleaving
    // that decides which error a partly-malformed arm reports.
    let mut subbed = Vec::with_capacity(children.len());
    for slot in children {
        subbed.push(required_par_recursive(slot, ctx, env)?);
    }

    Ok(Expr {
        expr_instance: Some(rebuild_expr_instance(arm, subbed, env.shift(ctx))),
    })
}

pub(crate) fn send_recursive(term: Send, ctx: SubCtx, env: EnvView<'_>) -> Result<Send, InterpreterError> {
    let Send {
        chan,
        data,
        persistent,
        locally_free,
        connective_used,
    } = term;
    let chan_sub = required_par_recursive(chan, ctx, env)?;
    let data_sub = map_pars_recursive(data, ctx, env)?;
    Ok(rebuild_send(
        chan_sub,
        data_sub,
        persistent,
        locally_free,
        connective_used,
        env.shift(ctx),
    ))
}

fn bind_recursive(
    term: ReceiveBind,
    ctx: SubCtx,
    env: EnvView<'_>,
) -> Result<ReceiveBind, InterpreterError> {
    let ReceiveBind {
        patterns,
        source,
        remainder,
        free_count,
    } = term;
    let source_sub = required_par_recursive(source, ctx, env)?;
    let patterns_sub = map_pars_recursive(patterns, ctx.deeper(), env)?;
    Ok(rebuild_receive_bind(
        source_sub,
        patterns_sub,
        remainder,
        free_count,
    ))
}

pub(crate) fn receive_recursive(
    term: Receive,
    ctx: SubCtx,
    env: EnvView<'_>,
) -> Result<Receive, InterpreterError> {
    let Receive {
        binds,
        body,
        persistent,
        peek,
        bind_count,
        locally_free,
        connective_used,
        condition,
    } = term;
    let binds_sub = binds
        .into_iter()
        .map(|b| bind_recursive(b, ctx, env))
        .collect::<Result<Vec<ReceiveBind>, InterpreterError>>()?;
    let shifted = ctx.shifted(bind_count);
    let body_sub = required_par_recursive(body, shifted, env)?;
    let condition_sub = match condition {
        Some(c) => Some(par_recursive(c, shifted, env)?),
        None => None,
    };
    Ok(rebuild_receive(
        binds_sub,
        body_sub,
        condition_sub,
        persistent,
        peek,
        bind_count,
        locally_free,
        connective_used,
        env.shift(ctx),
    ))
}

pub(crate) fn new_recursive(term: New, ctx: SubCtx, env: EnvView<'_>) -> Result<New, InterpreterError> {
    let New {
        bind_count,
        p,
        uri,
        injections,
        locally_free,
    } = term;
    let body_sub = required_par_recursive(p, ctx.shifted(bind_count), env)?;
    Ok(rebuild_new(
        body_sub,
        bind_count,
        uri,
        injections,
        locally_free,
        env.shift(ctx),
    ))
}

fn case_recursive(
    term: MatchCase,
    ctx: SubCtx,
    env: EnvView<'_>,
) -> Result<MatchCase, InterpreterError> {
    let MatchCase {
        pattern,
        source,
        free_count,
        guard,
    } = term;
    let shifted = ctx.shifted(free_count);
    let source_sub = required_par_recursive(source, shifted, env)?;
    let pattern_sub = required_par_recursive(pattern, ctx.deeper(), env)?;
    let guard_sub = match guard {
        Some(g) => Some(par_recursive(g, shifted, env)?),
        None => None,
    };
    Ok(rebuild_match_case(
        source_sub,
        pattern_sub,
        guard_sub,
        free_count,
    ))
}

pub(crate) fn match_recursive(
    term: Match,
    ctx: SubCtx,
    env: EnvView<'_>,
) -> Result<Match, InterpreterError> {
    let Match {
        target,
        cases,
        locally_free,
        connective_used,
    } = term;
    let target_sub = required_par_recursive(target, ctx, env)?;
    let cases_sub = cases
        .into_iter()
        .filter(|case| case.pattern.is_some() && case.source.is_some())
        .map(|c| case_recursive(c, ctx, env))
        .collect::<Result<Vec<MatchCase>, InterpreterError>>()?;
    Ok(rebuild_match(
        target_sub,
        cases_sub,
        locally_free,
        connective_used,
        env.shift(ctx),
    ))
}

pub(crate) fn if_recursive(term: If, ctx: SubCtx, env: EnvView<'_>) -> Result<If, InterpreterError> {
    let If {
        condition,
        if_true,
        if_false,
        locally_free,
        connective_used,
    } = term;
    let condition_sub = required_par_recursive(condition, ctx, env)?;
    let if_true_sub = required_par_recursive(if_true, ctx, env)?;
    let if_false_sub = required_par_recursive(if_false, ctx, env)?;
    Ok(rebuild_if(
        condition_sub,
        if_true_sub,
        if_false_sub,
        locally_free,
        connective_used,
        env.shift(ctx),
    ))
}

pub(crate) fn bundle_recursive(
    mut term: Bundle,
    ctx: SubCtx,
    env: EnvView<'_>,
) -> Result<Bundle, InterpreterError> {
    let body = term.body.take();
    let sub_bundle = required_par_recursive(body, ctx, env)?;
    Ok(rebuild_bundle(term, sub_bundle))
}

// ===========================================================================
// the differential
// ===========================================================================

#[cfg(test)]
mod differential_substitute_worklist {
    use super::*;
    use crate::rust::interpreter::accounting::costs::Cost;
    use crate::rust::interpreter::accounting::{BillableKind, RuntimeBudget};
    use crate::rust::interpreter::metering::MeteredMachine;
    use crate::rust::interpreter::substitute::SubstituteTrait;
    use crate::rust::interpreter::test_utils::substitution_corpus::{
        empty_env, gint, nested_list, populated_env, substitution_corpus, SubstitutionCase,
    };
    use models::rust::rholang::par_children::dismantle;
    use prost::Message;

    fn subject() -> Substitute {
        Substitute {
            metering: MeteredMachine::new(RuntimeBudget::new(Cost::create(
                i64::MAX / 4,
                "differential_substitute_worklist".to_string(),
            ))),
        }
    }

    /// The observable trace of one substitution path.
    #[derive(PartialEq, Debug)]
    struct Trace {
        result: Result<Vec<u8>, String>,
        charges: Vec<(BillableKind, u64)>,
        total: i64,
    }

    fn charge_trace(s: &Substitute) -> (Vec<(BillableKind, u64)>, i64) {
        let budget = s.metering.budget();
        let charges = budget
            .get_canonical_event_log()
            .into_iter()
            .map(|e| (e.kind, e.weight))
            .collect();
        (charges, budget.total_cost().value)
    }

    fn trace_of(s: &Substitute, result: Result<Par, InterpreterError>) -> Trace {
        let (charges, total) = charge_trace(s);
        Trace {
            result: result
                .map(|p| p.encode_to_vec())
                .map_err(|e| format!("{:?}", e)),
            charges,
            total,
        }
    }

    /// Run one case through BOTH paths on FRESH budgets and compare the audit's
    /// §8.1 observables.
    fn assert_agree(case: &SubstitutionCase, env: &Env<Par>, env_name: &str) {
        let ctx = SubCtx::root(case.depth);

        let oracle = subject();
        let rec = par_recursive(case.term.clone(), ctx, EnvView::new(env));
        let trace_rec = trace_of(&oracle, rec);

        let production = subject();
        let tr = production.substitute_no_sort(case.term.clone(), case.depth, env);
        let trace_tr = trace_of(&production, tr);

        assert_eq!(
            trace_rec, trace_tr,
            "WORKLIST DIVERGENCE on case `{}` under the {} environment\n  \
             recursive  = {:?}\n  worklist   = {:?}",
            case.name, env_name, trace_rec, trace_tr
        );
    }

    #[test]
    fn every_corpus_case_agrees_under_both_environments() {
        let populated = populated_env();
        let empty = empty_env();
        let corpus = substitution_corpus();
        assert!(
            corpus.len() >= 90,
            "the corpus shrank unexpectedly ({} cases) — a differential over a corpus \
             that no longer covers the schema licenses false confidence",
            corpus.len()
        );
        for case in &corpus {
            assert_agree(case, &populated, "populated");
            assert_agree(case, &empty, "empty");
        }
    }

    /// The sorted entry point (`substitute`) as well as the unsorted one:
    /// `SubstituteTrait<Par>::substitute` is `substitute_no_sort` followed by
    /// exactly one `sort_match`, and it is the form `substitute_and_charge`
    /// calls, hence the form whose bytes get signed.
    #[test]
    fn the_sorted_entry_point_agrees() {
        use models::rust::rholang::sorter::par_sort_matcher::ParSortMatcher;
        use models::rust::rholang::sorter::sortable::Sortable;

        let env = populated_env();
        for case in substitution_corpus() {
            let ctx = SubCtx::root(case.depth);
            let expected = par_recursive(case.term.clone(), ctx, EnvView::new(&env))
                .map(|p| ParSortMatcher::sort_match(&p).term)
                .map(|p| p.encode_to_vec())
                .map_err(|e| format!("{:?}", e));

            let s = subject();
            let actual = s
                .substitute(case.term.clone(), case.depth, &env)
                .map(|p| p.encode_to_vec())
                .map_err(|e| format!("{:?}", e));

            assert_eq!(
                expected, actual,
                "sorted-entry divergence on case `{}`",
                case.name
            );
        }
    }

    /// ⚠ The neutrality argument, checked rather than asserted: **nothing in
    /// this SCC charges**. The charge is levied once by `substitute_and_charge`
    /// on `encoded_len()` of the result, outside the recursion. If a `Cost::`
    /// call ever appears inside the SCC, the number of traversal steps becomes
    /// observable and the by-construction neutrality argument in the audit's
    /// §8.2 stops holding.
    #[test]
    fn charge_trace_is_empty_inside_the_scc() {
        let env = populated_env();
        for case in substitution_corpus() {
            let s = subject();
            let _ = s.substitute_no_sort(case.term.clone(), case.depth, &env);
            let (charges, total) = charge_trace(&s);
            assert!(
                charges.is_empty() && total == 0,
                "case `{}` produced a charge INSIDE the substitution SCC ({} events, \
                 total {}). The conversion's cost-neutrality argument is by construction \
                 and depends on this being empty.",
                case.name,
                charges.len(),
                total
            );
        }
    }

    /// The metered wrappers — the ones that do charge — must produce the same
    /// ordered trace and the same total through both paths.
    ///
    /// ## ★ Both wrappers, and BOTH ARMS
    ///
    /// Until 2026-07-28 this test drove [`Substitute::substitute_and_charge`]
    /// over the SUCCESS corpus and nothing else, and both omissions were found
    /// the same way: by mutating the wrapper and watching the test stay green.
    ///
    /// * The **error arm** was uncovered. Every term in `substitution_corpus`
    ///   substitutes successfully, so `Err` never happened, so the arm that
    ///   charges on the INPUT's `encoded_len` was never observed. Charging
    ///   `input_len + 1000` there left this test passing.
    /// * `substitute_no_sort_and_charge` was uncovered outright, despite the
    ///   plural in this test's name. It is the wrapper `eval_receive` uses for
    ///   the continuation BODY — the largest term a receive touches.
    ///
    /// Both gaps sat exactly on the lines the by-value conversion changed: the
    /// hoisted `input_len` is *only* read by the error arm. A proof that cannot
    /// reject the mutation it exists to reject is not a proof, so the coverage
    /// is here rather than in a follow-up.
    ///
    /// The mutations this now rejects, each applied to the wrapper and watched
    /// RED before the coverage was trusted (2026-07-28):
    ///
    /// | # | wrapper   | mutation                          | before | after   |
    /// |---|-----------|-----------------------------------|--------|---------|
    /// | A | sorted    | success arm charges `input_len`    | RED    | rejects |
    /// | B | sorted    | error arm charges `input_len+1000` | green  | rejects |
    /// | C | no-sort   | error arm charges `input_len+1000` | green  | rejects |
    /// | D | sorted    | error arm's charge omitted         | green  | rejects |
    /// | E | no-sort   | success arm charges `input_len`     | green  | rejects |
    ///
    /// The "before" column is the point: only A was caught by the test as it
    /// stood, and A is the one mutation the by-value conversion could not
    /// plausibly have made.
    #[test]
    fn metered_wrappers_agree() {
        let env = populated_env();

        // ── The SUCCESS arm: the charge is `encoded_len` of the RESULT. ──
        for case in substitution_corpus() {
            // Oracle side: recompute the result recursively, then levy the
            // wrapper's charge by hand in the same order the wrapper does.
            let oracle = subject();
            let rec = par_recursive(case.term.clone(), SubCtx::root(case.depth), EnvView::new(&env));
            let rec_bytes = match &rec {
                Ok(p) => (p.encoded_len() as i64).max(1),
                Err(_) => (case.term.encoded_len() as i64).max(1),
            };
            oracle
                .metering
                .reserve_substitution(Cost::create(rec_bytes, "substitution"))
                .expect("differential: the oracle budget must not run out");
            let trace_rec = trace_of(&oracle, rec);

            let production = subject();
            // ⚠ The wrapper takes its term BY VALUE (2026-07-28). The oracle
            // side above measured `case.term` before this point, so the clone
            // here feeds the production call without disturbing what the oracle
            // was computed from — it is the harness owning a second copy, not
            // the wrapper copying its input.
            let tr = production.substitute_and_charge(case.term.clone(), case.depth, &env);
            let trace_tr = trace_of(&production, tr);

            assert_eq!(
                trace_rec, trace_tr,
                "metered-wrapper divergence on case `{}`",
                case.name
            );

            // …and the no-sort twin, whose charge is levied on the UNSORTED
            // result. `par_recursive` is itself the un-sorted form, so the
            // oracle's number is the same one computed above.
            let oracle_ns = subject();
            let rec_ns =
                par_recursive(case.term.clone(), SubCtx::root(case.depth), EnvView::new(&env));
            let rec_ns_bytes = match &rec_ns {
                Ok(p) => (p.encoded_len() as i64).max(1),
                Err(_) => (case.term.encoded_len() as i64).max(1),
            };
            oracle_ns
                .metering
                .reserve_substitution(Cost::create(rec_ns_bytes, "substitution"))
                .expect("differential: the oracle budget must not run out");
            let trace_rec_ns = trace_of(&oracle_ns, rec_ns);

            let production_ns = subject();
            let tr_ns =
                production_ns.substitute_no_sort_and_charge(case.term.clone(), case.depth, &env);
            let trace_tr_ns = trace_of(&production_ns, tr_ns);

            assert_eq!(
                trace_rec_ns, trace_tr_ns,
                "metered NO-SORT wrapper divergence on case `{}`",
                case.name
            );
        }

        // ── ★ The ERROR arm: the charge is `encoded_len` of the INPUT. ──
        //
        // This is the arm the by-value conversion touches. In the `&A` form the
        // wrapper read the original through the borrow it still held; in the
        // by-value form it reads the same original before the move. The number
        // must be identical, and `encoded_len` being a pure read is why — but
        // "is why" is an argument, and this is the execution.
        for (name, term, depth) in error_corpus() {
            let oracle = subject();
            let rec = par_recursive(term.clone(), SubCtx::root(depth), EnvView::new(&env));
            // Anti-vacuity: an "error case" that succeeds would exercise the
            // success arm again and quietly re-open the hole this loop closes.
            assert!(
                rec.is_err(),
                "the `{}` error case did not actually produce an error — the metered \
                 wrappers' ERROR arm is not being exercised by it",
                name
            );
            oracle
                .metering
                .reserve_substitution(Cost::create(
                    (term.encoded_len() as i64).max(1),
                    "substitution",
                ))
                .expect("differential: the oracle budget must not run out");
            let trace_rec = trace_of(&oracle, rec);

            let production = subject();
            let tr = production.substitute_and_charge(term.clone(), depth, &env);
            let trace_tr = trace_of(&production, tr);
            assert_eq!(
                trace_rec, trace_tr,
                "metered-wrapper divergence on ERROR case `{}`",
                name
            );

            let oracle_ns = subject();
            let rec_ns = par_recursive(term.clone(), SubCtx::root(depth), EnvView::new(&env));
            oracle_ns
                .metering
                .reserve_substitution(Cost::create(
                    (term.encoded_len() as i64).max(1),
                    "substitution",
                ))
                .expect("differential: the oracle budget must not run out");
            let trace_rec_ns = trace_of(&oracle_ns, rec_ns);

            let production_ns = subject();
            let tr_ns = production_ns.substitute_no_sort_and_charge(term.clone(), depth, &env);
            let trace_tr_ns = trace_of(&production_ns, tr_ns);
            assert_eq!(
                trace_rec_ns, trace_tr_ns,
                "metered NO-SORT wrapper divergence on ERROR case `{}`",
                name
            );
        }
    }

    /// Deep and wide shapes, beyond anything the corpus carries — the regime
    /// the conversion exists for. Depth 256 is 64× the reported reproducer.
    #[test]
    fn deep_and_wide_shapes_agree() {
        let env = populated_env();

        // A deep chain, on a thread with a stack large enough for the ORACLE
        // (which is still Θ(depth) — that is the point).
        let handle = std::thread::Builder::new()
            .stack_size(512 * 1024 * 1024)
            .name("substitute-differential".to_string())
            .spawn(move || {
                for depth in [4usize, 16, 64, 256] {
                    let term = nested_list(depth);
                    let expected = par_recursive(term.clone(), SubCtx::root(0), EnvView::new(&env))
                        .expect("oracle failed on a nested list")
                        .encode_to_vec();
                    let s = subject();
                    let actual = s
                        .substitute_no_sort(term.clone(), 0, &env)
                        .expect("worklist failed on a nested list")
                        .encode_to_vec();
                    assert_eq!(expected, actual, "divergence at nesting depth {}", depth);
                    dismantle(term);
                }

                for width in [4usize, 64, 1024] {
                    let mut ps = Vec::with_capacity(width);
                    for i in 0..width {
                        ps.push(gint(i as i64));
                    }
                    let term = Par {
                        exprs: vec![Expr {
                            expr_instance: Some(ExprInstance::EListBody(models::rhoapi::EList {
                                ps,
                                locally_free: vec![],
                                connective_used: false,
                                remainder: None,
                            })),
                        }],
                        ..Default::default()
                    };
                    let expected = par_recursive(term.clone(), SubCtx::root(0), EnvView::new(&env))
                        .expect("oracle failed on a wide list")
                        .encode_to_vec();
                    let s = subject();
                    let actual = s
                        .substitute_no_sort(term.clone(), 0, &env)
                        .expect("worklist failed on a wide list")
                        .encode_to_vec();
                    assert_eq!(expected, actual, "divergence at sibling width {}", width);
                    dismantle(term);
                }
            })
            .expect("differential: failed to spawn");
        handle.join().expect("differential: the deep/wide thread panicked");
    }

    /// Terms whose substitution FAILS, with the depth to substitute them at.
    ///
    /// ★ Hoisted out of [`error_paths_agree`] on 2026-07-28 so that
    /// [`metered_wrappers_agree`] can drive the metered wrappers' ERROR arm with
    /// the same list. Two copies of one corpus do not stay equal, and the two
    /// tests ask different questions of it: `error_paths_agree` checks the error
    /// VALUE and its POSITION, `metered_wrappers_agree` checks the CHARGE levied
    /// on the way out.
    fn error_corpus() -> Vec<(&'static str, Par, i32)> {
        use models::rhoapi::{EPlus, Send};

        vec![
            (
                "expr with no instance",
                Par {
                    exprs: vec![Expr {
                        expr_instance: None,
                    }],
                    ..Default::default()
                },
                0,
            ),
            (
                "binary operand p1 missing",
                Par {
                    exprs: vec![Expr {
                        expr_instance: Some(ExprInstance::EPlusBody(EPlus {
                            p1: None,
                            p2: Some(gint(1)),
                        })),
                    }],
                    ..Default::default()
                },
                0,
            ),
            (
                "binary operand p2 missing",
                Par {
                    exprs: vec![Expr {
                        expr_instance: Some(ExprInstance::EPlusBody(EPlus {
                            p1: Some(gint(1)),
                            p2: None,
                        })),
                    }],
                    ..Default::default()
                },
                0,
            ),
            (
                "send channel missing",
                Par {
                    sends: vec![Send {
                        chan: None,
                        data: vec![gint(1)],
                        ..Default::default()
                    }],
                    ..Default::default()
                },
                0,
            ),
            (
                "illegal substitution: a FreeVar at depth 0",
                crate::rust::interpreter::test_utils::substitution_corpus::free_var(0),
                0,
            ),
            (
                "second expr faulty, first fine",
                Par {
                    exprs: vec![
                        Expr {
                            expr_instance: Some(ExprInstance::GInt(1)),
                        },
                        Expr {
                            expr_instance: None,
                        },
                    ],
                    ..Default::default()
                },
                0,
            ),
            (
                "faulty operand under three levels of nesting",
                {
                    let mut p = Par {
                        exprs: vec![Expr {
                            expr_instance: Some(ExprInstance::EPlusBody(EPlus {
                                p1: Some(gint(1)),
                                p2: None,
                            })),
                        }],
                        ..Default::default()
                    };
                    for _ in 0..3 {
                        p = Par {
                            exprs: vec![Expr {
                                expr_instance: Some(ExprInstance::EListBody(
                                    models::rhoapi::EList {
                                        ps: vec![p],
                                        locally_free: vec![],
                                        connective_used: false,
                                        remainder: None,
                                    },
                                )),
                            }],
                            ..Default::default()
                        };
                    }
                    p
                },
                0,
            ),
        ]
    }

    /// Error PARITY, including error POSITION: a term whose first faulty slot
    /// is reached only after several successful descents must produce the same
    /// `Err`, not merely some `Err`.
    #[test]
    fn error_paths_agree() {
        let env = populated_env();

        for (name, term, depth) in error_corpus() {
            let expected = par_recursive(term.clone(), SubCtx::root(depth), EnvView::new(&env))
                .map(|p| p.encode_to_vec())
                .map_err(|e| format!("{:?}", e));
            let s = subject();
            let actual = s
                .substitute_no_sort(term.clone(), depth, &env)
                .map(|p| p.encode_to_vec())
                .map_err(|e| format!("{:?}", e));

            assert!(
                expected.is_err(),
                "the `{}` error case did not actually produce an error in the oracle — \
                 it is not testing error parity",
                name
            );
            assert_eq!(expected, actual, "error divergence on `{}`", name);
        }
    }

    /// `EPathmapBody` and `EZipperBody` must come back UNSUBSTITUTED — the
    /// recursive form does not descend into them, and "fixing" that would
    /// change signed bytes.
    #[test]
    fn the_two_pathmap_arms_are_still_not_descended_into() {
        use models::rust::rhoapi_ext::EPathMap;
        use models::rust::rholang::par_children::substitute_descends_into;
        use crate::rust::interpreter::test_utils::substitution_corpus::bound_var;

        let env = populated_env();
        let inner = bound_var(0);
        let instance = ExprInstance::EPathmapBody(EPathMap::new(
            vec![inner.clone()],
            Vec::new(),
            false,
            None,
        ));
        assert!(!substitute_descends_into(&instance));

        let term = Par {
            exprs: vec![Expr {
                expr_instance: Some(instance),
            }],
            ..Default::default()
        };
        let s = subject();
        let out = s
            .substitute_no_sort(term.clone(), 0, &env)
            .expect("path-map substitution failed");
        assert_eq!(
            out.encode_to_vec(),
            term.encode_to_vec(),
            "the path-map arm was descended into. Its child is a BoundVar that the \
             populated environment WOULD have substituted, so this is the check that \
             a conversion did not silently start walking it."
        );
    }
}
