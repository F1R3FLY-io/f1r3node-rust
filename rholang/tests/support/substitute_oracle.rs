//! # The recursive oracle twin for the substitution SCC, and its differential
//!
//! This source lives under `rholang/tests/support`; the production module tree
//! reaches it only through a `#[cfg(test)] #[path = ...]` declaration.
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
    Bundle, Connective, ConnectiveBody, EPathMap, Expr, GUnforgeable, If, Match, MatchCase, New,
    Par, Receive, ReceiveBind, Send,
};
use models::rust::canonical_path::decode_trie_path;
use models::rust::rhoapi_ext::{EntryTrie, OwnedEPathMapEntry};
use rspace_plus_plus::rspace::history::Either;

use super::env::Env;
use super::errors::InterpreterError;
use super::substitute::Substitute;
use super::substitute_combine::{
    expr_arm_pattern_slots, fold_concatenate_par, fold_prepend_connective, fold_prepend_expr,
    missing_required_field, rebuild_bundle, rebuild_connective, rebuild_expr_instance, rebuild_if,
    rebuild_match, rebuild_match_case, rebuild_new, rebuild_par, rebuild_receive,
    rebuild_receive_bind, rebuild_send, set_bits_until, split_expr_instance, ConnArm,
};
use super::substitute_drive::{
    maybe_substitute_evar_view, maybe_substitute_var_ref_view, substitute_cost_region,
    substitute_cost_signature, EnvView, SubCtx,
};
use super::unwrap_option_safe;

// ===========================================================================
// the recursive twin
// ===========================================================================

pub(crate) fn par_recursive(
    mut term: Par,
    ctx: SubCtx,
    env: EnvView<'_>,
) -> Result<Par, InterpreterError> {
    let sends = term.take_occurrences::<Send>();
    let receives = term.take_occurrences::<Receive>();
    let news = term.take_occurrences::<New>();
    let exprs = term.take_occurrences::<Expr>();
    let matches = term.take_occurrences::<Match>();
    let unforgeables = term.take_occurrences::<GUnforgeable>();
    let bundles = term.take_occurrences::<Bundle>();
    let connectives = term.take_occurrences::<Connective>();
    let conditionals = term.take_occurrences::<If>();
    let mut cost_token_stacks = term.take_occurrences::<models::rhoapi::CostTokenStack>();
    let locally_free = std::mem::take(&mut term.locally_free);
    let connective_used = term.connective_used;

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
    for stack in &mut cost_token_stacks {
        for layer in &mut stack.layers {
            substitute_cost_signature(layer, ctx, env);
        }
    }

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
        cost_token_stacks,
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
    exprs
        .into_iter()
        .try_fold(Par::default(), |acc, expr| match expr.expr_instance {
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

pub(crate) fn expr_recursive(
    term: Expr,
    ctx: SubCtx,
    env: EnvView<'_>,
) -> Result<Expr, InterpreterError> {
    let instance = unwrap_option_safe(term.expr_instance)?;
    if let ExprInstance::EPathmapBody(pathmap) = instance {
        return epathmap_recursive(pathmap, ctx, env).map(|pathmap| Expr {
            expr_instance: Some(ExprInstance::EPathmapBody(pathmap)),
        });
    }
    let (arm, children) = split_expr_instance(instance);
    // Same per-slot depth table the driver reads, so the two cannot disagree
    // about which slots are pattern positions.
    let pattern_slots = expr_arm_pattern_slots(&arm);

    // Check-then-descend, operand by operand, in slot order — the interleaving
    // that decides which error a partly-malformed arm reports.
    let mut subbed = Vec::with_capacity(children.len());
    for (slot, child) in children.into_iter().enumerate() {
        let slot_ctx = match pattern_slots.contains(&slot) {
            true => ctx.deeper(),
            false => ctx,
        };
        subbed.push(required_par_recursive(child, slot_ctx, env)?);
    }

    Ok(Expr {
        expr_instance: Some(rebuild_expr_instance(arm, subbed, env.shift(ctx))),
    })
}

fn epathmap_recursive(
    pathmap: EPathMap,
    ctx: SubCtx,
    env: EnvView<'_>,
) -> Result<EPathMap, InterpreterError> {
    let parts = pathmap.into_owned_parts();
    let mut result = EPathMap::new(
        EntryTrie::default(),
        set_bits_until(parts.locally_free, env.shift(ctx)),
        parts.connective_used,
        parts.remainder,
    );
    for entry in parts.entries {
        match entry {
            OwnedEPathMapEntry::Set(key) => {
                let key =
                    decode_trie_path(&key).expect("set-mode EPathMap keys are canonical Par paths");
                result.insert_entry(par_recursive(key, ctx, env)?);
            }
            OwnedEPathMapEntry::Map { key, value } => {
                let key =
                    decode_trie_path(&key).expect("map-mode EPathMap keys are canonical Par paths");
                let key = par_recursive(key, ctx, env)?;
                let value = par_recursive(value, ctx, env)?;
                result
                    .insert_map_entry(key, value)
                    .expect("a fresh recursive substitution result cannot mix modes");
            }
        }
    }
    Ok(result)
}

pub(crate) fn send_recursive(
    term: Send,
    ctx: SubCtx,
    env: EnvView<'_>,
) -> Result<Send, InterpreterError> {
    let Send {
        chan,
        data,
        persistent,
        locally_free,
        connective_used,
        mut cost_region,
    } = term;
    substitute_cost_region(&mut cost_region, ctx, env);
    let chan_sub = required_par_recursive(chan, ctx, env)?;
    let data_sub = map_pars_recursive(data, ctx, env)?;
    Ok(rebuild_send(
        chan_sub,
        data_sub,
        persistent,
        locally_free,
        connective_used,
        env.shift(ctx),
        cost_region,
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
        mut cost_region,
    } = term;
    substitute_cost_region(&mut cost_region, ctx, env);
    let source_sub = required_par_recursive(source, ctx, env)?;
    let patterns_sub = map_pars_recursive(patterns, ctx.deeper(), env)?;
    Ok(rebuild_receive_bind(
        source_sub,
        patterns_sub,
        remainder,
        free_count,
        cost_region,
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
        mut cost_region,
    } = term;
    substitute_cost_region(&mut cost_region, ctx, env);
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
        cost_region,
    ))
}

pub(crate) fn new_recursive(
    term: New,
    ctx: SubCtx,
    env: EnvView<'_>,
) -> Result<New, InterpreterError> {
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

pub(crate) fn if_recursive(
    term: If,
    ctx: SubCtx,
    env: EnvView<'_>,
) -> Result<If, InterpreterError> {
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
    use models::rust::rholang::par_children::dismantle;
    use prost::Message;

    use super::*;
    use crate::rust::interpreter::accounting::costs::Cost;
    use crate::rust::interpreter::accounting::{BillableKind, RuntimeBudget};
    use crate::rust::interpreter::metering::MeteredMachine;
    use crate::rust::interpreter::substitute::SubstituteTrait;
    use crate::rust::interpreter::test_utils::substitution_corpus::{
        empty_env, gint, nested_list, populated_env, substitution_corpus, SubstitutionCase,
    };

    #[test]
    fn new_injection_preserves_admitted_incomplete_match_cases() {
        use models::rust::par_bag::ParBag;
        use models::rust::pathmap_crate_type_mapper::substitution_identity;
        use models::rust::utils::new_boundvar_par;
        for mask in 0..3 {
            let injected = Par::default().with_matches(vec![Match {
                target: Some(gint(1)),
                cases: vec![MatchCase {
                    pattern: (mask & 1 != 0).then(|| gint(2)),
                    source: (mask & 2 != 0).then(|| gint(3)),
                    guard: Some(gint(4)),
                    free_count: 0,
                }],
                ..Match::default()
            }]);
            let retained = injected.first::<Match>().unwrap().unwrap().0;
            assert_eq!(retained.cases.len(), 1);
            assert_eq!(retained.cases[0].pattern.is_some(), mask & 1 != 0);
            assert_eq!(retained.cases[0].source.is_some(), mask & 2 != 0);
            ParBag::from_compact_bytes(&injected.components().compact_bytes())
                .expect("incomplete injected component is admitted RPB3");
            let injected_bytes = injected.encode_to_vec();
            let term = Par::default().with_news(vec![New {
                p: Some(new_boundvar_par(0, vec![1], false)),
                injections: std::collections::BTreeMap::from([("incomplete".to_owned(), injected)]),
                ..New::default()
            }]);
            assert!(!substitution_identity(&term));
            assert_eq!(
                term.first::<New>().unwrap().unwrap().0.injections["incomplete"].encode_to_vec(),
                injected_bytes
            );
            let env = Env::new().put(gint(99));
            let expected = par_recursive(term.clone(), SubCtx::root(0), EnvView::new(&env))
                .expect("recursive New preserves admitted injections without traversal");
            let actual = subject().substitute_no_sort(term, 0, &env);
            assert_eq!(
                result_bytes(actual),
                Ok(expected.encode_to_vec()),
                "incomplete mask {mask}"
            );
        }
    }

    #[test]
    fn new_injection_preserves_all_admitted_incomplete_families() {
        use models::rhoapi::{
            EMap, EMatches, EMethod, ENeg, EPlus, EVar, EZipper, KeyValuePair, ReceiveBind,
        };
        use models::rust::par_bag::{Multiplicity, ParBag, ParBagComponent};
        use models::rust::pathmap_crate_type_mapper::substitution_identity;
        use models::rust::utils::new_boundvar_par;

        fn check<T: ParBagComponent>(name: &str, component: T) {
            let mut injected = Par::default();
            injected
                .insert_component_many(&component, Multiplicity::new(3).unwrap(), vec![5], true)
                .unwrap();
            ParBag::from_compact_bytes(&injected.components().compact_bytes())
                .unwrap_or_else(|error| panic!("{name}: admission: {error:?}"));
            let original = injected.encode_to_vec();
            let original_compact = injected.components().compact_bytes();
            let term = Par::default().with_news(vec![New {
                p: Some(new_boundvar_par(0, vec![1], false)),
                injections: std::collections::BTreeMap::from([("incomplete".to_owned(), injected)]),
                ..New::default()
            }]);
            assert!(!substitution_identity(&term), "{name}: force traversal");
            let env = Env::new().put(gint(99));
            let expected = par_recursive(term.clone(), SubCtx::root(0), EnvView::new(&env))
                .unwrap_or_else(|error| panic!("{name}: oracle: {error:?}"));
            let actual = subject()
                .substitute_no_sort(term, 0, &env)
                .unwrap_or_else(|error| panic!("{name}: iterative: {error:?}"));
            assert_eq!(
                actual.encode_to_vec(),
                expected.encode_to_vec(),
                "{name}: oracle bytes"
            );
            assert_eq!(
                actual.first::<New>().unwrap().unwrap().0.injections["incomplete"].encode_to_vec(),
                original,
                "{name}: exact injection"
            );
            assert_eq!(
                actual.first::<New>().unwrap().unwrap().0.injections["incomplete"]
                    .components()
                    .compact_bytes(),
                original_compact,
                "{name}: exact RPB3 score/body/count image"
            );
        }

        check("Send", Send {
            data: vec![Par::default(), gint(1)],
            ..Send::default()
        });
        check("Receive body", Receive {
            binds: vec![ReceiveBind {
                source: Some(gint(1)),
                ..ReceiveBind::default()
            }],
            ..Receive::default()
        });
        check("Receive source", Receive {
            body: Some(Par::default()),
            binds: vec![ReceiveBind {
                patterns: vec![gint(2)],
                ..ReceiveBind::default()
            }],
            ..Receive::default()
        });
        check("New", New {
            uri: vec!["z".to_owned(), "a".to_owned()],
            injections: std::collections::BTreeMap::from([("x".to_owned(), gint(3))]),
            ..New::default()
        });
        check("Match target", Match {
            cases: vec![MatchCase {
                pattern: Some(gint(1)),
                source: Some(gint(2)),
                guard: Some(Par::default()),
                ..MatchCase::default()
            }],
            ..Match::default()
        });
        check("Match case", Match {
            target: Some(gint(1)),
            cases: vec![MatchCase {
                pattern: Some(Par::default()),
                guard: Some(Par::default()),
                ..MatchCase::default()
            }],
            ..Match::default()
        });
        check("Bundle", Bundle::default());
        for mask in 0..7 {
            check("If", If {
                condition: (mask & 1 != 0).then(|| gint(1)),
                if_true: (mask & 2 != 0).then(Par::default),
                if_false: (mask & 4 != 0).then(|| gint(3)),
                ..If::default()
            });
        }
        let mut expressions = vec![
            ExprInstance::EVarBody(EVar::default()),
            ExprInstance::ENegBody(ENeg::default()),
            ExprInstance::EMethodBody(EMethod {
                arguments: vec![gint(1), Par::default()],
                ..EMethod::default()
            }),
            ExprInstance::EZipperBody(EZipper::default()),
        ];
        for mask in 0..3 {
            expressions.push(ExprInstance::EPlusBody(EPlus {
                p1: (mask & 1 != 0).then(|| gint(1)),
                p2: (mask & 2 != 0).then(Par::default),
            }));
            expressions.push(ExprInstance::EMatchesBody(EMatches {
                target: (mask & 1 != 0).then(|| gint(1)),
                pattern: (mask & 2 != 0).then(Par::default),
            }));
            expressions.push(ExprInstance::EMapBody(EMap {
                kvs: vec![
                    KeyValuePair {
                        key: (mask & 1 != 0).then(|| gint(1)),
                        value: (mask & 2 != 0).then(Par::default),
                    },
                    KeyValuePair {
                        key: Some(gint(3)),
                        value: Some(gint(4)),
                    },
                ],
                ..EMap::default()
            }));
        }
        for expression in expressions {
            check("Expr", Expr {
                expr_instance: Some(expression),
            });
        }
        check("absent Expr", Expr::default());
        for cursor_kind in [0, 1, 2, 3, u32::MAX] {
            check("opaque zipper cursor kind", Expr {
                expr_instance: Some(ExprInstance::EZipperBody(EZipper {
                    pathmap: Some(models::rhoapi::EPathMap::default()),
                    cursor_kind,
                    ..EZipper::default()
                })),
            });
        }
    }

    fn subject() -> Substitute {
        Substitute {
            metering: MeteredMachine::new(RuntimeBudget::new(Cost::create(
                i64::MAX / 4,
                "differential_substitute_worklist".to_string(),
            ))),
        }
    }

    #[test]
    fn bound_cost_signers_resolve_to_fresh_runtime_names() {
        use models::rhoapi::cost_signature_atom::Form;
        use models::rhoapi::g_unforgeable::UnfInstance;
        use models::rhoapi::{
            CostBoundSignature, CostRegion, CostSignature, CostSignatureAtom, CostTokenStack,
            GPrivate,
        };
        use models::rust::rholang::sorter::par_sort_matcher::ParSortMatcher;
        use models::rust::rholang::sorter::sortable::Sortable;

        let name = Par::default().with_unforgeables(vec![GUnforgeable {
            unf_instance: Some(UnfInstance::GPrivateBody(GPrivate { id: vec![7; 32] })),
        }]);
        let region = CostRegion {
            version: 1,
            start_line: 1,
            start_column: 0,
            end_line: 1,
            end_column: 20,
            signer: Some(CostSignature {
                atoms: vec![CostSignatureAtom {
                    form: Some(Form::Bound(CostBoundSignature { index: 0 })),
                }],
            }),
            ordinal: 0,
        };
        let stack = CostTokenStack {
            layers: vec![region.signer.clone().unwrap()],
        };
        let term = Par::default()
            .with_sends(vec![Send {
                chan: Some(Par::default()),
                cost_region: Some(region.clone()),
                ..Send::default()
            }])
            .with_receives(vec![Receive {
                binds: vec![ReceiveBind {
                    source: Some(Par::default()),
                    cost_region: Some(region.clone()),
                    ..ReceiveBind::default()
                }],
                body: Some(Par::default()),
                cost_region: Some(region),
                ..Receive::default()
            }])
            .with_cost_token_stacks(vec![stack]);
        let unresolved = subject().substitute(term.clone(), 0, &Env::new()).unwrap();
        let unresolved_send = unresolved.decode_occurrences::<Send>().remove(0);
        assert!(matches!(
            unresolved_send.cost_region.unwrap().signer.unwrap().atoms[0].form,
            Some(Form::Bound(_))
        ));
        assert!(matches!(
            unresolved.decode_occurrences::<CostTokenStack>()[0].layers[0].atoms[0].form,
            Some(Form::Bound(_))
        ));

        let resolved = subject()
            .substitute(term.clone(), 0, &Env::new().put(name.clone()))
            .unwrap();
        let other_name = Par::default().with_unforgeables(vec![GUnforgeable {
            unf_instance: Some(UnfInstance::GPrivateBody(GPrivate { id: vec![8; 32] })),
        }]);
        let other = subject()
            .substitute(term, 0, &Env::new().put(other_name))
            .unwrap();
        assert_ne!(resolved.encode_to_vec(), other.encode_to_vec());
        let expected = ParSortMatcher::sort_match(&name).term.encode_to_vec();
        let send = resolved.decode_occurrences::<Send>().remove(0);
        let receive = resolved.decode_occurrences::<Receive>().remove(0);
        for region in [
            send.cost_region.unwrap(),
            receive.cost_region.unwrap(),
            receive.binds[0].cost_region.clone().unwrap(),
        ] {
            assert_eq!(
                region.signer.unwrap().atoms[0].form,
                Some(Form::RuntimeName(expected.clone()))
            );
        }
        assert_eq!(
            resolved.decode_occurrences::<CostTokenStack>()[0].layers[0].atoms[0].form,
            Some(Form::RuntimeName(expected))
        );
    }

    /// The observable trace of one substitution path.
    #[derive(PartialEq, Debug)]
    struct Trace {
        result: Result<Vec<u8>, String>,
        stable_hash_bytes: Result<Vec<u8>, String>,
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
        let stable_hash_bytes = result
            .as_ref()
            .map(|p| models::rust::rholang::bincode_encoder::encode(p))
            .map_err(|e| format!("{:?}", e));
        Trace {
            result: result
                .map(|p| p.encode_to_vec())
                .map_err(|e| format!("{:?}", e)),
            stable_hash_bytes,
            charges,
            total,
        }
    }

    fn result_bytes<T: prost::Message>(
        result: Result<T, InterpreterError>,
    ) -> Result<Vec<u8>, String> {
        result
            .map(|value| value.encode_to_vec())
            .map_err(|error| format!("{:?}", error))
    }

    #[cfg(feature = "protobuf-projection-experiment")]
    fn encoded_cost_census(term: &Par) -> (usize, usize, usize, u16) {
        let (mut copies, mut payload_bytes, mut key_bytes, mut families) = (0, 0, 0, 0);
        macro_rules! visit {
            ($ty:ty, $index:expr, $region:expr) => {
                term.visit_encoded::<$ty>(|entry| {
                    families |= 1 << $index;
                    if $region(&entry) {
                        copies += 1;
                        payload_bytes += entry.payload().len();
                    }
                    key_bytes += models::rust::par_bag::PAR_BAG_KEY_DOMAIN.len()
                        + 1
                        + entry.score().len()
                        + entry.payload().len()
                        + std::mem::size_of::<u64>();
                });
            };
        }
        visit!(Send, 0, |_| true);
        visit!(Receive, 1, |_| true);
        visit!(New, 2, |_| true);
        visit!(
            Expr,
            3,
            |entry: &models::rust::par_bag::EncodedParComponent<'_, Expr>| entry
                .to_owned_canonical_ezipper()
                .is_none()
        );
        visit!(Match, 4, |_| true);
        visit!(GUnforgeable, 5, |_| true);
        visit!(Bundle, 6, |_| true);
        visit!(Connective, 7, |_| true);
        visit!(If, 8, |_| true);
        (copies, payload_bytes, key_bytes, families)
    }

    #[cfg(feature = "protobuf-projection-experiment")]
    fn measured_par_substitution(
        production: &Substitute,
        term: Par,
        depth: i32,
        env: &Env<Par>,
    ) -> (
        Result<Par, InterpreterError>,
        models::rust::par_construction::substitution_cost::Totals,
    ) {
        use models::rust::par_construction::substitution_cost::{capture, Metric, Totals};
        let identity = models::rust::pathmap_crate_type_mapper::substitution_identity(&term);
        let (copies, bytes, _, _) = encoded_cost_census(&term);
        let (result, totals) = capture(|| production.substitute_no_sort(term, depth, env));
        if identity {
            assert!(result.is_ok());
            assert_eq!(totals, Totals::default());
        } else {
            assert_eq!(totals.get(Metric::RootCopies), copies, "{totals:?}");
            assert_eq!(totals.get(Metric::RootBytes), bytes, "{totals:?}");
            assert_eq!(totals.get(Metric::ArenaCount), 1, "{totals:?}");
            assert!(totals.get(Metric::WorkSteps) > 0, "{totals:?}");
            assert_eq!(totals.get(Metric::OriginalBytes), 0, "{totals:?}");
            match &result {
                Ok(output) => {
                    assert_eq!(totals.get(Metric::FinishAttempts), 1, "{totals:?}");
                    assert_eq!(totals.get(Metric::FinishSuccesses), 1, "{totals:?}");
                    assert_eq!(
                        totals.get(Metric::RootKeys),
                        output.distinct_len(),
                        "{totals:?}"
                    );
                    assert_eq!(
                        totals.get(Metric::RootKeyBytes),
                        encoded_cost_census(output).2,
                        "{totals:?}"
                    );
                }
                Err(_) => {
                    assert_eq!(totals.get(Metric::FinishAttempts), 0, "{totals:?}");
                    assert_eq!(totals.get(Metric::FinishSuccesses), 0, "{totals:?}");
                }
            }
        }
        (result, totals)
    }

    /// Run one case through BOTH paths on FRESH budgets and compare the audit's
    /// §8.1 observables.
    fn assert_agree(case: &SubstitutionCase, env: &Env<Par>, env_name: &str) {
        let ctx = SubCtx::root(case.depth);

        let oracle = subject();
        let rec = par_recursive(case.term.clone(), ctx, EnvView::new(env));
        let trace_rec = trace_of(&oracle, rec);

        let production = subject();
        #[cfg(not(feature = "protobuf-projection-experiment"))]
        let tr = production.substitute_no_sort(case.term.clone(), case.depth, env);
        #[cfg(feature = "protobuf-projection-experiment")]
        let (tr, _) = measured_par_substitution(&production, case.term.clone(), case.depth, env);
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

    #[cfg(feature = "protobuf-projection-experiment")]
    #[test]
    fn region_cost_census_exercises_all_nine_nonidentity_root_families() {
        use models::rhoapi::g_unforgeable::UnfInstance;
        use models::rhoapi::GSysAuthToken;

        use crate::rust::interpreter::test_utils::substitution_corpus::bound_var;

        let env = populated_env();
        let mut families = 0;
        for case in substitution_corpus() {
            if !models::rust::pathmap_crate_type_mapper::substitution_identity(&case.term) {
                let input_families = encoded_cost_census(&case.term).3;
                let (result, _) =
                    measured_par_substitution(&subject(), case.term, case.depth, &env);
                if result.is_ok() {
                    families |= input_families;
                }
            }
        }
        let term = bound_var(0).with_unforgeables(vec![GUnforgeable {
            unf_instance: Some(UnfInstance::GSysAuthTokenBody(GSysAuthToken {})),
        }]);
        assert!(!models::rust::pathmap_crate_type_mapper::substitution_identity(&term));
        let input_families = encoded_cost_census(&term).3;
        let expected = par_recursive(term.clone(), SubCtx::root(0), EnvView::new(&env)).unwrap();
        let (actual, _) = measured_par_substitution(&subject(), term, 0, &env);
        assert_eq!(actual.unwrap().encode_to_vec(), expected.encode_to_vec());
        families |= input_families;
        assert_eq!(families, (1 << 9) - 1);
    }

    #[cfg(feature = "protobuf-projection-experiment")]
    #[test]
    fn region_cost_collision_merges_keys_without_multiplicity_work() {
        use models::rust::par_construction::substitution_cost::Metric;

        use crate::rust::interpreter::test_utils::substitution_corpus::bound_var;

        let input = bound_var(0).append(bound_var(1));
        let env = Env::new().put(gint(7)).put(gint(7));
        assert_eq!(input.distinct_len(), 2);
        let expected = par_recursive(input.clone(), SubCtx::root(0), EnvView::new(&env)).unwrap();
        assert_eq!(expected.distinct_len(), 1);
        assert_eq!(expected.cardinality(), 2);
        let mut baseline = None;
        for count in [1, 2, 1_000_000] {
            let (actual, totals) = measured_par_substitution(
                &subject(),
                input.clone().scale_occurrences(count).unwrap(),
                0,
                &env,
            );
            let actual = actual.unwrap();
            assert_eq!(
                actual.components().compact_bytes(),
                expected
                    .clone()
                    .scale_occurrences(count)
                    .unwrap()
                    .components()
                    .compact_bytes()
            );
            assert_eq!(totals.get(Metric::RootCopies), 2);
            assert_eq!(totals.get(Metric::RootKeys), 1);
            assert_eq!(totals.get(Metric::ResidentBytes), 0);
            if let Some(first) = baseline {
                assert_eq!(totals, first);
            }
            baseline = Some(totals);
        }
    }

    #[cfg(feature = "protobuf-projection-experiment")]
    #[test]
    fn region_cost_terminal_splices_preserve_unresolved_children() {
        use models::rust::par_construction::substitution_cost::Metric;

        use crate::rust::interpreter::test_utils::substitution_corpus::bound_var;

        let child = bound_var(0);
        let replacements = [
            Par::default().with_bundles(vec![Bundle {
                body: Some(child.clone()),
                read_flag: true,
                write_flag: true,
            }]),
            Par::default().with_connectives(vec![Connective {
                connective_instance: Some(ConnectiveInstance::ConnAndBody(ConnectiveBody {
                    ps: vec![child.clone(), gint(13)],
                })),
            }]),
            Par::default().with_connectives(vec![Connective {
                connective_instance: Some(ConnectiveInstance::ConnOrBody(ConnectiveBody {
                    ps: vec![child, gint(17)],
                })),
            }]),
        ];
        for replacement in replacements {
            let original = replacement.components().compact_bytes();
            let env = Env::new().put(replacement);
            let input = bound_var(0);
            let expected =
                par_recursive(input.clone(), SubCtx::root(0), EnvView::new(&env)).unwrap();
            let (actual, totals) = measured_par_substitution(&subject(), input, 0, &env);
            let actual = actual.unwrap();
            assert_eq!(actual.encode_to_vec(), expected.encode_to_vec());
            assert_eq!(actual.components().compact_bytes(), original);
            assert!(totals.get(Metric::ResidentBytes) > 0, "{totals:?}");
            assert_eq!(totals.get(Metric::RootCopies), 1);
        }
    }

    #[test]
    fn received_string_length_substitution_agrees_with_the_recursive_oracle() {
        use models::rhoapi::expr::ExprInstance;
        use models::rhoapi::var::VarInstance;
        use models::rhoapi::{EMethod, EVar, Expr, Var};

        let bound_target = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::EVarBody(EVar {
                v: Some(Var {
                    var_instance: Some(VarInstance::BoundVar(0)),
                }),
            })),
        }]);
        let term = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::EMethodBody(EMethod {
                method_name: "length".to_owned(),
                target: Some(bound_target),
                arguments: Vec::new(),
                locally_free: vec![0],
                connective_used: false,
            })),
        }]);
        let env = Env::new().put(Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GString("A".repeat(32))),
        }]));
        let case = SubstitutionCase {
            name: "received-string-length".to_owned(),
            term,
            depth: 0,
        };

        assert_agree(&case, &env, "single received string");
    }

    #[test]
    fn bounded_string_continuation_substitution_agrees_with_the_recursive_oracle() {
        use std::collections::HashMap;

        use rholang_parser::RholangParser;
        use validated::Validated;

        use crate::rust::interpreter::compiler::exports::ProcVisitInputs;
        use crate::rust::interpreter::compiler::normalize::normalize_ann_proc;

        const SOURCE: &str = "new x, loop in { \
             x!(\"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\") | \
             for(@s <- x){ \
               contract loop(@n) = { match n { 0 => Nil  _ => loop!(n - 1) } } | \
               loop!(s.length()) } }";

        let parser = RholangParser::new();
        let ast = match parser.parse(SOURCE) {
            Validated::Good(mut processes) if processes.len() == 1 => processes.remove(0),
            other => panic!("bounded-string differential source did not parse: {other:?}"),
        };
        let mut normalized =
            normalize_ann_proc(&ast, ProcVisitInputs::new(), &HashMap::new(), &parser)
                .expect("bounded-string source normalizes")
                .par;
        let outer = normalized
            .take_occurrences::<New>()
            .pop()
            .expect("bounded-string source has one outer new");
        let mut outer_body = outer.p.expect("outer new retains its body");
        let receive = outer_body
            .take_occurrences::<Receive>()
            .pop()
            .expect("outer body has one receive");
        let continuation = receive.body.expect("receive retains its continuation");

        // The real reducer substitutes this body at depth zero under the two
        // freshly allocated outer names, shifted past the receive's one bound
        // pattern variable. Concrete values do not matter to the driving
        // proof, but distinct bindings ensure both de Bruijn lookups fire.
        let outer_env = Env::new()
            .put(gint(700))
            .put(Par::default().with_exprs(vec![Expr {
                expr_instance: Some(ExprInstance::GString("loop-channel".to_owned())),
            }]));
        let shifted = outer_env.shift(receive.bind_count);
        let case = SubstitutionCase {
            name: "bounded-string-continuation".to_owned(),
            term: continuation,
            depth: 0,
        };

        assert_agree(&case, &shifted, "bounded-string receive continuation");
    }

    #[test]
    fn every_public_no_sort_root_agrees_with_the_recursive_oracle() {
        let env = populated_env();
        for case in substitution_corpus() {
            let ctx = SubCtx::root(case.depth);
            let mut components = case.term;

            for value in components.take_occurrences::<Expr>() {
                let expected = result_bytes(expr_recursive(value.clone(), ctx, EnvView::new(&env)));
                let actual =
                    result_bytes(subject().substitute_expr_no_sort(value, case.depth, &env));
                assert_eq!(expected, actual, "Expr root divergence in `{}`", case.name);
            }
            for value in components.take_occurrences::<Send>() {
                let expected = result_bytes(send_recursive(value.clone(), ctx, EnvView::new(&env)));
                let actual = result_bytes(subject().substitute_no_sort(value, case.depth, &env));
                assert_eq!(expected, actual, "Send root divergence in `{}`", case.name);
            }
            for value in components.take_occurrences::<Receive>() {
                let expected =
                    result_bytes(receive_recursive(value.clone(), ctx, EnvView::new(&env)));
                let actual = result_bytes(subject().substitute_no_sort(value, case.depth, &env));
                assert_eq!(
                    expected, actual,
                    "Receive root divergence in `{}`",
                    case.name
                );
            }
            for value in components.take_occurrences::<New>() {
                let expected = result_bytes(new_recursive(value.clone(), ctx, EnvView::new(&env)));
                let actual = result_bytes(subject().substitute_no_sort(value, case.depth, &env));
                assert_eq!(expected, actual, "New root divergence in `{}`", case.name);
            }
            for value in components.take_occurrences::<Match>() {
                let expected =
                    result_bytes(match_recursive(value.clone(), ctx, EnvView::new(&env)));
                let actual = result_bytes(subject().substitute_no_sort(value, case.depth, &env));
                assert_eq!(expected, actual, "Match root divergence in `{}`", case.name);
            }
            for value in components.take_occurrences::<If>() {
                let expected = result_bytes(if_recursive(value.clone(), ctx, EnvView::new(&env)));
                let actual = result_bytes(subject().substitute_no_sort(value, case.depth, &env));
                assert_eq!(expected, actual, "If root divergence in `{}`", case.name);
            }
            for value in components.take_occurrences::<Bundle>() {
                let expected =
                    result_bytes(bundle_recursive(value.clone(), ctx, EnvView::new(&env)));
                let actual = result_bytes(subject().substitute_no_sort(value, case.depth, &env));
                assert_eq!(
                    expected, actual,
                    "Bundle root divergence in `{}`",
                    case.name
                );
            }
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
            let rec = par_recursive(
                case.term.clone(),
                SubCtx::root(case.depth),
                EnvView::new(&env),
            );
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
            let rec_ns = par_recursive(
                case.term.clone(),
                SubCtx::root(case.depth),
                EnvView::new(&env),
            );
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
                    let term = models::par_from_default! {
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
        handle
            .join()
            .expect("differential: the deep/wide thread panicked");
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
                models::par_from_default! {
                    exprs: vec![Expr {
                        expr_instance: None,
                    }],
                    ..Default::default()
                },
                0,
            ),
            (
                "binary operand p1 missing",
                models::par_from_default! {
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
                models::par_from_default! {
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
                models::par_from_default! {
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
                models::par_from_default! {
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
                    let mut p = models::par_from_default! {
                        exprs: vec![Expr {
                            expr_instance: Some(ExprInstance::EPlusBody(EPlus {
                                p1: Some(gint(1)),
                                p2: None,
                            })),
                        }],
                        ..Default::default()
                    };
                    for _ in 0..3 {
                        p = models::par_from_default! {
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
            #[cfg(not(feature = "protobuf-projection-experiment"))]
            let actual = s.substitute_no_sort(term.clone(), depth, &env);
            #[cfg(feature = "protobuf-projection-experiment")]
            let (actual, _) = measured_par_substitution(&s, term.clone(), depth, &env);
            let actual = actual
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

    /// EPathMap substitution is now part of the consensus-visible traversal;
    /// EZipper remains opaque because it is runtime cursor state, not surface
    /// pattern syntax.
    #[test]
    fn epathmap_descends_incrementally_while_ezipper_remains_opaque() {
        use models::rust::rhoapi_ext::EPathMap;
        use models::rust::rholang::par_children::substitute_descends_into;

        use crate::rust::interpreter::test_utils::substitution_corpus::bound_var;

        let env = populated_env();
        let inner = bound_var(0);
        let instance =
            ExprInstance::EPathmapBody(EPathMap::new(vec![inner.clone()], Vec::new(), false, None));
        assert!(substitute_descends_into(&instance));

        let term = models::par_from_default! {
            exprs: vec![Expr {
                expr_instance: Some(instance),
            }],
            ..Default::default()
        };
        let s = subject();
        let out = s
            .substitute_no_sort(term.clone(), 0, &env)
            .expect("path-map substitution failed");
        let expected = par_recursive(term.clone(), SubCtx::root(0), EnvView::new(&env))
            .expect("recursive EPathMap oracle");
        assert_eq!(out.encode_to_vec(), expected.encode_to_vec());
        assert_ne!(
            out.encode_to_vec(),
            term.encode_to_vec(),
            "the bound variable inside the EPathMap must be substituted"
        );

        let zipper = ExprInstance::EZipperBody(Default::default());
        assert!(!substitute_descends_into(&zipper));
    }

    /// The byte-region lane must retain the Bag count attached to one
    /// canonical component endpoint. Expanding the count into repeated work
    /// items would be observationally equal on a small fixture, so this test
    /// also checks the compressed result's distinct/cardinality split.
    #[test]
    fn region_substitution_scales_component_multiplicity_algebraically() {
        use crate::rust::interpreter::test_utils::substitution_corpus::bound_var;

        let site = bound_var(0)
            .decode_occurrences::<Expr>()
            .into_iter()
            .next()
            .expect("bound_var has one Expr component");
        let term = models::par_from_default! {
            exprs: vec![site; 17],
            ..Default::default()
        };
        assert_eq!(term.distinct_len(), 1);
        assert_eq!(term.cardinality(), 17);

        let env = populated_env();
        let actual = subject()
            .substitute_no_sort(term.clone(), 0, &env)
            .expect("multiplicity-preserving substitution");
        let expected = par_recursive(term, SubCtx::root(0), EnvView::new(&env))
            .expect("recursive multiplicity oracle");

        assert_eq!(actual.encode_to_vec(), expected.encode_to_vec());
        assert_eq!(actual.distinct_len(), 1);
        assert_eq!(actual.cardinality(), 17);
    }

    #[test]
    fn deep_new_injection_identity_import_is_stack_safe_and_byte_exact() {
        std::thread::Builder::new()
            .stack_size(256 * 1024)
            .spawn(|| {
                const DEPTH: usize = 4_096;
                let mut injected = models::par_from_default! {
                    exprs: vec![Expr {
                        expr_instance: Some(ExprInstance::GInt(7)),
                    }],
                    ..Default::default()
                };
                for _ in 0..DEPTH {
                    injected = models::par_from_default! {
                        exprs: vec![Expr {
                            expr_instance: Some(ExprInstance::ENotBody(models::rhoapi::ENot {
                                p: Some(injected),
                            })),
                        }],
                        ..Default::default()
                    };
                }

                let term = Par::default().with_news(vec![New {
                    bind_count: 0,
                    p: Some(Par::default()),
                    uri: Vec::new(),
                    injections: std::collections::BTreeMap::from([("deep".to_owned(), injected)]),
                    locally_free: Vec::new(),
                }]);
                let expected = term.encode_to_vec();
                let actual = subject()
                    .substitute_no_sort(term, 0, &empty_env())
                    .expect("deep New injection identity import");
                assert_eq!(actual.encode_to_vec(), expected);
            })
            .expect("spawn fixed-stack New injection gate")
            .join()
            .expect("fixed-stack New injection gate");
    }

    #[test]
    fn new_injection_preserves_epathmap_keys_values_and_ezipper_bytes() {
        use models::rhoapi::EZipper;

        use crate::rust::interpreter::test_utils::substitution_corpus::bound_var;

        let map = EPathMap::new_map([(bound_var(0), bound_var(1))], vec![1], true, None);
        let injected = Par::default().with_exprs(vec![
            Expr {
                expr_instance: Some(ExprInstance::EPathmapBody(map.clone())),
            },
            Expr {
                expr_instance: Some(ExprInstance::EZipperBody(EZipper {
                    pathmap: Some(map),
                    current_path: Vec::new(),
                    is_write_zipper: true,
                    locally_free: vec![1],
                    connective_used: true,
                    cursor_kind: 2,
                })),
            },
        ]);
        let term = Par::default().with_news(vec![New {
            bind_count: 0,
            p: Some(Par::default()),
            uri: Vec::new(),
            injections: std::collections::BTreeMap::from([("pathmap".to_owned(), injected)]),
            locally_free: Vec::new(),
        }]);
        let expected = term.encode_to_vec();
        let actual = subject()
            .substitute_no_sort(term, 0, &populated_env())
            .expect("PathMap/EZipper injection identity import");
        assert_eq!(actual.encode_to_vec(), expected);
    }

    /// Identity import is a wire-preservation contract, not merely a semantic
    /// equality claim. Drive the exhaustive substitution corpus as values of
    /// a two-entry protobuf map so the proof covers generated map ordering as
    /// well as every Expr/Connective arm, ESet/EMap, Receive condition, Match
    /// guard, Bundle, binder metadata, and compressed component multiplicity.
    #[test]
    fn new_injections_preserve_the_exhaustive_corpus_byte_exact() {
        for case in substitution_corpus() {
            let duplicate_expr = Expr {
                expr_instance: Some(ExprInstance::GInt(77)),
            };
            let duplicate = Par::default().with_exprs(vec![duplicate_expr.clone(), duplicate_expr]);
            assert_eq!(duplicate.distinct_len(), 1);
            assert_eq!(duplicate.cardinality(), 2);

            let term = Par::default().with_news(vec![New {
                bind_count: 0,
                p: Some(Par::default()),
                uri: Vec::new(),
                // Insert in reverse lexical order. BTreeMap/protobuf canonical
                // order must still keep each retained region with its key.
                injections: std::collections::BTreeMap::from([
                    (format!("z/{}", case.name), case.term),
                    ("a/multiplicity-sentinel".to_owned(), duplicate),
                ]),
                locally_free: Vec::new(),
            }]);
            let expected = term.encode_to_vec();
            let actual = subject()
                .substitute_no_sort(term, 0, &populated_env())
                .unwrap_or_else(|error| {
                    panic!("identity import failed for `{}`: {error}", case.name)
                });
            assert_eq!(
                actual.encode_to_vec(),
                expected,
                "identity import changed canonical bytes for `{}`",
                case.name
            );
        }
    }

    /// Map-mode EPathMap keys and values are retained by the shallow decoder
    /// as ACT topology plus value regions. Both sides remain substitutable and
    /// the rebuilt result must stay map-mode rather than projecting entries
    /// through an intermediate collection.
    #[test]
    fn epathmap_map_mode_substitutes_keys_and_retained_value_regions() {
        use models::rhoapi::EList;
        use models::rust::epathmap_trie_codec::EPathMapMode;
        use models::rust::rhoapi_ext::EPathMap;

        use crate::rust::interpreter::test_utils::substitution_corpus::{bound_var, expr_par};

        // The second element is an escape segment inside a split EList key.
        // If the key PDA accidentally encodes its replacement as a whole path,
        // a replacement EList is flattened into the outer key and the oracle
        // comparison below fails.
        let mixed_split_key = expr_par(ExprInstance::EListBody(EList {
            ps: vec![
                expr_par(ExprInstance::GString("loc".to_owned())),
                bound_var(0),
            ],
            locally_free: Vec::new(),
            connective_used: false,
            remainder: None,
        }));

        let map = EPathMap::new_map(
            [
                (bound_var(0), bound_var(1)),
                (bound_var(2), bound_var(0)),
                (mixed_split_key, bound_var(1)),
            ],
            Vec::new(),
            false,
            None,
        );
        let term = models::par_from_default! {
            exprs: vec![Expr {
                expr_instance: Some(ExprInstance::EPathmapBody(map)),
            }],
            ..Default::default()
        };
        let env = populated_env();
        let actual = subject()
            .substitute_no_sort(term.clone(), 0, &env)
            .expect("map-mode EPathMap substitution");
        let expected = par_recursive(term.clone(), SubCtx::root(0), EnvView::new(&env))
            .expect("recursive map-mode EPathMap oracle");

        assert_eq!(actual.encode_to_vec(), expected.encode_to_vec());
        assert_ne!(actual.encode_to_vec(), term.encode_to_vec());
        let expr = actual
            .decode_occurrences::<Expr>()
            .into_iter()
            .next()
            .expect("result retains the EPathMap Expr");
        let ExprInstance::EPathmapBody(map) = expr.expr_instance.expect("Expr arm") else {
            panic!("result changed the EPathMap Expr arm");
        };
        assert_eq!(map.mode(), EPathMapMode::Map);
        assert_eq!(map.len(), 3);
    }
}
