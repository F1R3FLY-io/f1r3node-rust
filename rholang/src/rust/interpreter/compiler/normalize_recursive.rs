//! # The RECURSIVE ORACLE TWIN of the normalizer SCC — `#[cfg(test)]` only
//!
//! A **verbatim** copy of the 26-function `normalize_ann_proc` SCC exactly as it
//! stood at commit `6ccf71f2`, immediately before it became the explicit
//! pushdown machine in [`super::normalize_drive`]. Every intra-SCC call is
//! renamed with a `_recursive` suffix so the copy calls *itself* and never
//! re-enters production code; nothing else is edited.
//!
//! ## Why a copy and not a git tag
//!
//! The differential in [`super::normalize_differential`] must run **both**
//! implementations in one process, on the same `AnnProc` out of the same arena,
//! and compare the normalized `Par` bytes together with the final `free_map`
//! and `bound_map_chain`. That is only possible if the pre-conversion code is
//! linkable. This is the house standard established by
//! `models/src/rust/rholang/sorter/sort_recursive.rs` (the sorter's twin,
//! `2c32b173`) and `reduce.rs::eval_expr_recursive` (`a929a2d6`); see
//! `docs/design/audits/theta-depth-traversals-2026-07-26.md` §8.3.
//!
//! ## ⚠ This module is Θ(depth) in native stack, on purpose
//!
//! It is the thing being measured *against*. It must never be reachable from
//! production: the `mod` declaration in `compiler/mod.rs` is `#[cfg(test)]`, and
//! only `normalize_ann_proc_recursive` / `normalize_name_recursive` are visible
//! outside it. Tests that drive it at depth must therefore run on a thread with
//! an explicit large stack, exactly as the differential does.
//!
//! Provenance of every block is recorded inline as `file:line-line`, so a
//! reviewer can `git show 6ccf71f2:<file>` and diff.

#![allow(clippy::all)]
#![allow(unused_imports)]
#![allow(unused_parens)]
#![allow(dead_code)]

use std::collections::{BTreeMap, HashMap, HashSet};

use models::rhoapi::connective::ConnectiveInstance;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{
    connective, expr, var, Bundle, Connective, ConnectiveBody, EList, EMatches, EMethod, EMinus,
    EPathMap, EPlus, ETuple, EVar, Expr, If, Match, MatchCase, New, Par, Receive, ReceiveBind,
    Send, Var as model_var,
};
use models::rust::bundle_ops::BundleOps;
use models::rust::par_map::ParMap;
use models::rust::par_map_type_mapper::ParMapTypeMapper;
use models::rust::par_set::ParSet;
use models::rust::par_set_type_mapper::ParSetTypeMapper;
use models::rust::rholang::sorter::par_sort_matcher::ParSortMatcher;
use models::rust::rholang::sorter::sortable::Sortable;
use models::rust::sorted_par_hash_set::SortedParHashSet;
use models::rust::sorted_par_map::SortedParMap;
use models::rust::utils::union;
use prost::Message;
use rholang_parser::ast::{
    AnnProc, Bind, BundleType, Case, Collection, Id, KeyValuePair, LetBinding, Name, NameDecl,
    Names, Proc,
    ProcList, Receipts, SendType, Signature, Source, SyncSendCont, TokenStack, Var,
};
use rholang_parser::{RholangParser, SourcePos, SourceSpan};
use shared::rust::BitSet;
use uuid::Uuid;

use super::bound_map_chain::BoundMapChain;
use super::exports::{
    BoundContext, CollectVisitInputs, CollectVisitOutputs, FreeContext, FreeMap, IdContextPos,
    NameVisitInputs, NameVisitOutputs, ProcVisitInputs, ProcVisitOutputs,
};
use super::normalize::VarSort;
use super::normalizer::cost_accounting::desugar;
use super::normalizer::cost_accounting::ir::Sig;
use super::normalizer::cost_accounting::pattern_guard::{
    reject_cost_syntax_in_name_pattern, reject_cost_syntax_in_pattern,
};
use super::normalizer::cost_accounting::sig::{canon_bound, canon_ground};
use super::normalizer::processes::p_ground_normalizer::normalize_p_ground;
use super::normalizer::processes::p_simple_type_normalizer::normalize_simple_type;
use super::normalizer::processes::utils::fail_on_invalid_connective;
use super::normalizer::remainder_normalizer_matcher::{normalize_match_name, normalize_remainder};
use super::receive_binds_sort_matcher::pre_sort_binds;
use super::span_utils::SpanContext;
use super::utils::{BinaryExpr, UnaryExpr};
use crate::rust::interpreter::accounting::Sig as NativeSig;
use crate::rust::interpreter::errors::InterpreterError;
use crate::rust::interpreter::matcher::has_locally_free::HasLocallyFree;
use crate::rust::interpreter::unwrap_option_safe;
use crate::rust::interpreter::util::{
    filter_and_adjust_bitset, prepend_bundle, prepend_connective, prepend_expr, prepend_new,
};

// ==========================================================================
// VERBATIM from `normalize.rs`, lines 83-475, at commit 6ccf71f2.
// ==========================================================================
pub(crate) fn normalize_ann_proc_recursive<'ast>(
    proc: &AnnProc<'ast>,
    input: ProcVisitInputs,
    _env: &HashMap<String, Par>,
    parser: &'ast RholangParser<'ast>,
) -> Result<ProcVisitOutputs, InterpreterError> {
    fn unary_exp<'ast>(
        sub_proc: &'ast AnnProc<'ast>,
        input: ProcVisitInputs,
        constructor: Box<dyn UnaryExpr>,
        env: &HashMap<String, Par>,
        parser: &'ast RholangParser<'ast>,
    ) -> Result<ProcVisitOutputs, InterpreterError> {
        let input_par = input.par.clone();
        let input_depth = input.bound_map_chain.depth();
        let sub_result = normalize_ann_proc_recursive(sub_proc, input, env, parser)?;
        let expr = constructor.from_par(sub_result.par.clone());

        Ok(ProcVisitOutputs {
            par: prepend_expr(input_par, expr, input_depth as i32),
            free_map: sub_result.free_map,
        })
    }

    fn binary_exp<'ast>(
        left_proc: &'ast AnnProc<'ast>,
        right_proc: &'ast AnnProc<'ast>,
        input: ProcVisitInputs,
        constructor: Box<dyn BinaryExpr>,
        env: &HashMap<String, Par>,
        parser: &'ast RholangParser<'ast>,
    ) -> Result<ProcVisitOutputs, InterpreterError> {
        let input_par = input.par.clone();
        let input_depth = input.bound_map_chain.depth();
        let input_bound_chain = input.bound_map_chain.clone();

        let left_result = normalize_ann_proc_recursive(left_proc, input, env, parser)?;
        let right_result = normalize_ann_proc_recursive(
            right_proc,
            ProcVisitInputs {
                par: Par::default(),
                bound_map_chain: input_bound_chain,
                free_map: left_result.free_map.clone(),
            },
            env,
            parser,
        )?;

        let expr: Expr = constructor.from_pars(left_result.par.clone(), right_result.par.clone());

        Ok(ProcVisitOutputs {
            par: prepend_expr(input_par, expr, input_depth as i32),
            free_map: right_result.free_map,
        })
    }

    match &proc.proc {
        Proc::Nil => Ok(ProcVisitOutputs {
            par: input.par.clone(),
            free_map: input.free_map.clone(),
        }),

        // Ground literals
        Proc::Unit
        | Proc::BoolLiteral(_)
        | Proc::LongLiteral(_)
        | Proc::SignedIntLiteral { .. }
        | Proc::UnsignedIntLiteral { .. }
        | Proc::BigIntLiteral(_)
        | Proc::BigRatLiteral(_)
        | Proc::FloatLiteral { .. }
        | Proc::FixedPointLiteral { .. }
        | Proc::StringLiteral(_)
        | Proc::UriLiteral(_) => normalize_p_ground(proc.proc, input),

        Proc::SimpleType(simple_type) => normalize_simple_type(simple_type, input),

        Proc::ProcVar(var) => {
            use crate::rust::interpreter::compiler::normalizer::processes::p_var_normalizer::normalize_p_var;
            normalize_p_var(var, input, proc.span)
        }

        Proc::Par { left, right } => {
            normalize_p_par_recursive(left, right, input, _env, parser)
        }

        Proc::Eval { name } => {
            normalize_p_eval_recursive(name, input, _env, parser)
        }

        // UnaryExp - handle all unary operators
        Proc::UnaryExp { op, arg } => match op {
            rholang_parser::ast::UnaryExpOp::Negation => {
                normalize_p_negation_recursive(arg.proc, arg.span, input, _env, parser)
            }
            rholang_parser::ast::UnaryExpOp::Not => {
                use models::rhoapi::ENot;
                unary_exp(arg, input, Box::new(ENot::default()), _env, parser)
            }
            rholang_parser::ast::UnaryExpOp::Neg => {
                use models::rhoapi::ENeg;
                unary_exp(arg, input, Box::new(ENeg::default()), _env, parser)
            }
        },

        // BinaryExp - handle all binary operators
        Proc::BinaryExp { op, left, right } => {
            match op {
                // Logical connectives
                rholang_parser::ast::BinaryExpOp::Conjunction => {
                    normalize_p_conjunction_recursive(left, right, input, _env, parser)
                }
                rholang_parser::ast::BinaryExpOp::Disjunction => {
                    normalize_p_disjunction_recursive(left, right, input, _env, parser)
                }
                rholang_parser::ast::BinaryExpOp::Matches => {
                    normalize_p_matches_recursive(left, right, input, _env, parser)
                }

                // Arithmetic
                rholang_parser::ast::BinaryExpOp::Add => {
                    binary_exp(left, right, input, Box::new(EPlus::default()), _env, parser)
                }
                rholang_parser::ast::BinaryExpOp::Sub => binary_exp(
                    left,
                    right,
                    input,
                    Box::new(EMinus::default()),
                    _env,
                    parser,
                ),
                rholang_parser::ast::BinaryExpOp::Mult => {
                    use models::rhoapi::EMult;
                    binary_exp(left, right, input, Box::new(EMult::default()), _env, parser)
                }
                rholang_parser::ast::BinaryExpOp::Div => {
                    use models::rhoapi::EDiv;
                    binary_exp(left, right, input, Box::new(EDiv::default()), _env, parser)
                }
                rholang_parser::ast::BinaryExpOp::Mod => {
                    use models::rhoapi::EMod;
                    binary_exp(left, right, input, Box::new(EMod::default()), _env, parser)
                }

                // Comparison operators
                rholang_parser::ast::BinaryExpOp::Eq => {
                    use models::rhoapi::EEq;
                    binary_exp(left, right, input, Box::new(EEq::default()), _env, parser)
                }
                rholang_parser::ast::BinaryExpOp::Neq => {
                    use models::rhoapi::ENeq;
                    binary_exp(left, right, input, Box::new(ENeq::default()), _env, parser)
                }
                rholang_parser::ast::BinaryExpOp::Lt => {
                    use models::rhoapi::ELt;
                    binary_exp(left, right, input, Box::new(ELt::default()), _env, parser)
                }
                rholang_parser::ast::BinaryExpOp::Lte => {
                    use models::rhoapi::ELte;
                    binary_exp(left, right, input, Box::new(ELte::default()), _env, parser)
                }
                rholang_parser::ast::BinaryExpOp::Gt => {
                    use models::rhoapi::EGt;
                    binary_exp(left, right, input, Box::new(EGt::default()), _env, parser)
                }
                rholang_parser::ast::BinaryExpOp::Gte => {
                    use models::rhoapi::EGte;
                    binary_exp(left, right, input, Box::new(EGte::default()), _env, parser)
                }

                // Set/String operations
                rholang_parser::ast::BinaryExpOp::Concat => {
                    use models::rhoapi::EPlusPlus;
                    binary_exp(
                        left,
                        right,
                        input,
                        Box::new(EPlusPlus::default()),
                        _env,
                        parser,
                    )
                }
                rholang_parser::ast::BinaryExpOp::Diff => {
                    use models::rhoapi::EMinusMinus;
                    binary_exp(
                        left,
                        right,
                        input,
                        Box::new(EMinusMinus::default()),
                        _env,
                        parser,
                    )
                }

                // Boolean operators
                rholang_parser::ast::BinaryExpOp::Or => {
                    use models::rhoapi::EOr;
                    binary_exp(left, right, input, Box::new(EOr::default()), _env, parser)
                }
                rholang_parser::ast::BinaryExpOp::And => {
                    use models::rhoapi::EAnd;
                    binary_exp(left, right, input, Box::new(EAnd::default()), _env, parser)
                }

                // String interpolation
                rholang_parser::ast::BinaryExpOp::Interpolation => {
                    use models::rhoapi::EPercentPercent;
                    binary_exp(
                        left,
                        right,
                        input,
                        Box::new(EPercentPercent::default()),
                        _env,
                        parser,
                    )
                }
            }
        }

        // IfThenElse - handle conditional statements
        Proc::IfThenElse {
            condition,
            if_true,
            if_false,
        } => {

            // Follow same pattern as original IfElse: use empty Par for normalization, then append original Par
            let mut empty_par_input = input.clone();
            empty_par_input.par = Par::default();

            // Use the updated normalize_p_if_recursive that handles None case internally
            normalize_p_if_recursive(
                condition,
                if_true,
                if_false.as_ref(),
                empty_par_input,
                _env,
                parser,
            )
            .map(|mut new_visits| {
                let new_par = new_visits.par.append(input.par);
                new_visits.par = new_par;
                new_visits
            })
        }

        // Method - handle method calls
        Proc::Method {
            receiver,
            name,
            args,
        } => {
            normalize_p_method_recursive(receiver, name, args, input, _env, parser)
        }

        // Bundle - handle bundle constructs
        Proc::Bundle { bundle_type, proc } => {
            normalize_p_bundle_recursive(bundle_type, proc, input, &proc.span, _env, parser)
        }

        // Send - handle send operations
        Proc::Send {
            channel,
            send_type,
            inputs,
        } => {
            normalize_p_send_recursive(channel, send_type, inputs, input, _env, parser)
        }

        // SendSync - handle synchronous send operations
        Proc::SendSync {
            channel,
            inputs,
            cont,
        } => {
            normalize_p_send_sync_recursive(channel, inputs, cont, &proc.span, input, _env, parser)
        }

        // New - handle name declarations and scoping
        Proc::New { decls, proc } => {
            normalize_p_new_recursive(decls, proc, input, _env, parser)
        }

        // Contract - handle contract declarations
        Proc::Contract {
            name,
            formals,
            body,
        } => {
            normalize_p_contr_recursive(name, formals, body, input, _env, parser)
        }

        // Match - handle pattern matching
        Proc::Match { expression, cases } => {
            normalize_p_match_recursive(expression, cases, input, _env, parser)
        }

        // Collection - handle data structures (lists, tuples, sets, maps)
        Proc::Collection(collection) => {
            normalize_p_collect_recursive(collection, input, _env, parser)
        }

        // ForComprehension - handle for-comprehensions (was Input in old AST)
        Proc::ForComprehension {
            receipts,
            proc: continuation,
        } => {
            // W1 Phase 4: a `for` carrying any per-clause SIGNED bind
            // `{% y <- x %}[s]` routes to signed-join recognition (validate each
            // clause sig, recover the natural-arity plain join). Greg 2026-06-15:
            // fuel is provisioned on `Σ⟦s⟧` and acquired by SEQUENTIAL per-atom
            // gates — a fuel token is structurally incapable of entering the data
            // join's `ReceiveBind` set, so fuel is NEVER folded into the join
            // (recognition-only: no gate node; the reducer meters per-COMM by
            // lane). An unsigned `for` lowers ordinarily.
            use rholang_parser::ast::Bind;
            let has_signed_bind = receipts.iter().any(|receipt| {
                receipt
                    .binds
                    .iter()
                    .any(|bind| matches!(bind, Bind::Signed { .. }))
            });
            if has_signed_bind {
                recognize_signed_join_recursive(receipts, *continuation, proc.span, input, _env, parser)
            } else {
                normalize_p_input_recursive(receipts, continuation, input, _env, parser)
            }
        }

        // Let - handle let bindings
        Proc::Let {
            bindings,
            body,
            concurrent,
        } => {
            normalize_p_let_recursive(bindings, body, *concurrent, proc.span, input, _env, parser)
        }

        // VarRef - handle variable references
        Proc::VarRef { kind, var } => {
            use crate::rust::interpreter::compiler::normalizer::processes::p_var_ref_normalizer::normalize_p_var_ref;
            normalize_p_var_ref(*kind, var, input, proc.span)
        }

        // Select - handle select expressions (choice constructs)
        Proc::Select { branches: _ } => {
            // TODO: Implement select normalizer when needed
            // This corresponds to Choice in the old AST which was also not implemented (todo!())
            Err(InterpreterError::ParserError(
                "Select (choice) constructs not yet implemented in normalizer".to_string(),
            ))
        }

        // Cost-accounted surface syntax (W1): recognition + native attribution.
        // A signed term / token stack resolves its signature(s) to a native `Sig`
        // and lowers the inner process ORDINARILY — NO synthetic fuel gate (the
        // reducer meters per-COMM; surface forms decorate). The located-stack
        // attribution that consumes the resolved `Sig` is a metering-context
        // concern (Phase 3, at the reducer/gate where the `Cosigned` envelope is
        // installed — BLOCKER-1). See `normalizer::cost_accounting::recognize`.
        Proc::SignedTerm { proc: inner, sig } => {
            recognize_signed_term_recursive(inner, sig, input, _env, parser)
        }
        Proc::TokenStack { stack } => {
            recognize_token_stack_recursive(stack, input, _env, parser)
        }

        // Bad - handle parsing errors
        Proc::Bad => Err(InterpreterError::ParserError(
            "Bad process node indicates parsing error".to_string(),
        )),
    }
}

// ==========================================================================
// VERBATIM from `normalizer/name_normalize_matcher.rs`, lines 16-151, at commit 6ccf71f2.
// ==========================================================================
pub(crate) fn normalize_name_recursive<'ast>(
    name: &Name<'ast>,
    input: NameVisitInputs,
    env: &HashMap<String, Par>,
    parser: &'ast rholang_parser::RholangParser<'ast>,
) -> Result<NameVisitOutputs, InterpreterError> {
    match name {
        Name::NameVar(var) => {
            match var {
                Var::Wildcard => {
                    let wildcard_span = SpanContext::wildcard_span();
                    let wildcard_bind_result = input.free_map.add_wildcard(wildcard_span);

                    let new_expr = Expr {
                        expr_instance: Some(expr::ExprInstance::EVarBody(EVar {
                            v: Some(model_var {
                                var_instance: Some(var::VarInstance::Wildcard(var::WildcardMsg {})),
                            }),
                        })),
                    };

                    Ok(NameVisitOutputs {
                        par: prepend_expr(
                            Par::default(),
                            new_expr,
                            input.bound_map_chain.depth() as i32,
                        ),
                        free_map: wildcard_bind_result,
                    })
                }

                Var::Id(id) => {
                    let name = id.name;
                    // Extract proper source position from Id
                    let source_pos = id.pos;

                    match input.bound_map_chain.get(name) {
                        Some(bound_context) => match bound_context {
                            BoundContext {
                                index: level,
                                typ: VarSort::NameSort,
                                ..
                            } => {
                                let new_expr = Expr {
                                    expr_instance: Some(expr::ExprInstance::EVarBody(EVar {
                                        v: Some(model_var {
                                            var_instance: Some(var::VarInstance::BoundVar(
                                                level as i32,
                                            )),
                                        }),
                                    })),
                                };

                                Ok(NameVisitOutputs {
                                    par: prepend_expr(
                                        Par::default(),
                                        new_expr,
                                        input.bound_map_chain.depth() as i32,
                                    ),
                                    free_map: input.free_map,
                                })
                            }

                            BoundContext {
                                typ: VarSort::ProcSort,
                                source_span: proc_var_span,
                                ..
                            } => Err(InterpreterError::UnexpectedNameContext {
                                var_name: name.to_string(),
                                proc_var_source_span: proc_var_span,
                                name_source_span: SpanContext::pos_to_span(source_pos),
                            }),
                        },

                        None => match input.free_map.get(name) {
                            None => {
                                let updated_free_map = input.free_map.put_pos((
                                    name.to_string(),
                                    VarSort::NameSort,
                                    source_pos,
                                ));
                                let new_expr = Expr {
                                    expr_instance: Some(expr::ExprInstance::EVarBody(EVar {
                                        v: Some(model_var {
                                            var_instance: Some(var::VarInstance::FreeVar(
                                                input.free_map.next_level as i32,
                                            )),
                                        }),
                                    })),
                                };

                                Ok(NameVisitOutputs {
                                    par: prepend_expr(
                                        Par::default(),
                                        new_expr,
                                        input.bound_map_chain.depth() as i32,
                                    ),
                                    free_map: updated_free_map,
                                })
                            }
                            Some(FreeContext {
                                source_span: first_span,
                                ..
                            }) => Err(InterpreterError::UnexpectedReuseOfNameContextFree {
                                var_name: name.to_string(),
                                first_use: first_span,
                                second_use: SpanContext::pos_to_span(source_pos),
                            }),
                        },
                    }
                }
            }
        }

        Name::Quote(ann_proc) => {
            // Name::Quote now wraps an AnnProc directly, not a Proc
            // Call normalize_ann_proc_recursive with proper span-based inputs
            let proc_visit_result = normalize_ann_proc_recursive(
                ann_proc,
                ProcVisitInputs {
                    par: Par::default(),
                    bound_map_chain: input.bound_map_chain.clone(),
                    free_map: input.free_map.clone(),
                },
                env,
                parser,
            )?;

            // Return the normalized result
            Ok(NameVisitOutputs {
                par: proc_visit_result.par,
                free_map: proc_visit_result.free_map,
            })
        }
    }
}

// ==========================================================================
// VERBATIM from `normalizer/collection_normalize_matcher.rs`, lines 23-265, at commit 6ccf71f2.
// ==========================================================================
fn normalize_collection_recursive<'ast>(
    proc: &'ast Collection<'ast>,
    input: CollectVisitInputs,
    env: &HashMap<String, Par>,
    parser: &'ast rholang_parser::RholangParser<'ast>,
) -> Result<CollectVisitOutputs, InterpreterError> {
    fn fold_match<'ast, F>(
        known_free: FreeMap<VarSort>,
        elements: &[AnnProc<'ast>],
        constructor: F,
        input: CollectVisitInputs,
        env: &HashMap<String, Par>,
        parser: &'ast rholang_parser::RholangParser<'ast>,
    ) -> Result<CollectVisitOutputs, InterpreterError>
    where
        F: Fn(Vec<Par>, Vec<u8>, bool) -> Expr,
    {
        let init = (vec![], known_free.clone(), Vec::new(), false);
        let (mut acc_pars, mut result_known_free, mut locally_free, mut connective_used) = init;

        for element in elements {
            let result = normalize_ann_proc_recursive(
                element,
                ProcVisitInputs {
                    par: Par::default(),
                    bound_map_chain: input.bound_map_chain.clone(),
                    free_map: result_known_free.clone(),
                },
                env,
                parser,
            )?;

            acc_pars.push(result.par.clone());
            result_known_free = result.free_map.clone();
            locally_free = union(locally_free, result.par.locally_free);
            connective_used = connective_used || result.par.connective_used;
        }

        let constructed_expr: Expr = constructor(acc_pars, locally_free, connective_used);
        let expr: Expr = constructed_expr.into();

        Ok(CollectVisitOutputs {
            expr,
            free_map: result_known_free,
        })
    }

    fn fold_match_map<'ast>(
        known_free: FreeMap<VarSort>,
        remainder: Option<models::rhoapi::Var>,
        pairs: &[KeyValuePair<'ast>],
        input: CollectVisitInputs,
        env: &HashMap<String, Par>,
        parser: &'ast rholang_parser::RholangParser<'ast>,
    ) -> Result<CollectVisitOutputs, InterpreterError> {
        let init = (vec![], known_free.clone(), Vec::new(), false);

        let (mut acc_pairs, mut result_known_free, mut locally_free, mut connective_used) = init;

        for key_value_pair in pairs {
            let key_result = normalize_ann_proc_recursive(
                &key_value_pair.0,
                ProcVisitInputs {
                    par: Par::default(),
                    bound_map_chain: input.bound_map_chain.clone(),
                    free_map: result_known_free.clone(),
                },
                env,
                parser,
            )?;

            let value_result = normalize_ann_proc_recursive(
                &key_value_pair.1,
                ProcVisitInputs {
                    par: Par::default(),
                    bound_map_chain: input.bound_map_chain.clone(),
                    free_map: key_result.free_map.clone(),
                },
                env,
                parser,
            )?;

            acc_pairs.push((key_result.par.clone(), value_result.par.clone()));
            result_known_free = value_result.free_map.clone();
            locally_free = union(
                locally_free,
                union(key_result.par.locally_free, value_result.par.locally_free),
            );
            connective_used = connective_used
                || key_result.par.connective_used
                || value_result.par.connective_used;
        }

        let remainder_connective_used = match remainder {
            Some(ref var) => var.connective_used(var.clone()),
            None => false,
        };

        let remainder_locally_free = match remainder {
            Some(ref var) => var.locally_free(var.clone(), 0),
            None => Vec::new(),
        };

        let expr = Expr {
            expr_instance: Some(ExprInstance::EMapBody(ParMapTypeMapper::par_map_to_emap(
                ParMap {
                    ps: SortedParMap::create_from_vec(
                        acc_pairs.clone().into_iter().rev().collect(),
                    ),
                    connective_used: connective_used || remainder_connective_used,
                    locally_free: union(locally_free, remainder_locally_free),
                    remainder: remainder.clone(),
                },
            ))),
        };

        Ok(CollectVisitOutputs {
            expr,
            free_map: result_known_free,
        })
    }

    match proc {
        Collection::List {
            elements,
            remainder,
        } => {
            let (optional_remainder, known_free) =
                normalize_remainder(remainder, input.free_map.clone())?;

            let constructor =
                |ps: Vec<Par>, locally_free: Vec<u8>, connective_used: bool| -> Expr {
                    let mut tmp_e_list = EList {
                        ps,
                        locally_free,
                        connective_used,
                        remainder: optional_remainder.clone(),
                    };

                    tmp_e_list.connective_used =
                        tmp_e_list.connective_used || optional_remainder.is_some();
                    Expr {
                        expr_instance: Some(ExprInstance::EListBody(tmp_e_list)),
                    }
                };

            fold_match(known_free, elements, constructor, input, env, parser)
        }

        Collection::Tuple(elements) => {
            let constructor =
                |ps: Vec<Par>, locally_free: Vec<u8>, connective_used: bool| -> Expr {
                    let tmp_tuple = ETuple {
                        ps,
                        locally_free,
                        connective_used,
                    };

                    Expr {
                        expr_instance: Some(ExprInstance::ETupleBody(tmp_tuple)),
                    }
                };

            fold_match(
                input.free_map.clone(),
                elements,
                constructor,
                input,
                env,
                parser,
            )
        }

        Collection::Set {
            elements,
            remainder,
        } => {
            let (optional_remainder, known_free) =
                normalize_remainder(remainder, input.free_map.clone())?;

            let constructor =
                |pars: Vec<Par>, locally_free: Vec<u8>, connective_used: bool| -> Expr {
                    let mut tmp_par_set = ParSet {
                        ps: SortedParHashSet::create_from_vec(pars),
                        locally_free,
                        connective_used,
                        remainder: optional_remainder.clone(),
                    };

                    tmp_par_set.connective_used =
                        tmp_par_set.connective_used || optional_remainder.is_some();

                    let eset = ParSetTypeMapper::par_set_to_eset(tmp_par_set);

                    Expr {
                        expr_instance: Some(ExprInstance::ESetBody(eset)),
                    }
                };

            fold_match(known_free, elements, constructor, input, env, parser)
        }

        Collection::Map {
            elements,
            remainder,
        } => {
            let (optional_remainder, known_free) =
                normalize_remainder(remainder, input.free_map.clone())?;

            fold_match_map(known_free, optional_remainder, elements, input, env, parser)
        }

        Collection::PathMap {
            elements,
            remainder,
        } => {
            let (optional_remainder, known_free) =
                normalize_remainder(remainder, input.free_map.clone())?;

            let constructor =
                |ps: Vec<Par>, locally_free: Vec<u8>, connective_used: bool| -> Expr {
                    // EPathMap fix P3 (PM-2): constructor instead of a
                    // struct literal (private shadow cell). The value is
                    // FRESH (never interned), so the field write below stays
                    // sound under the shadow-cell invariant.
                    let mut tmp_e_pathmap = EPathMap::new(
                        ps,
                        locally_free,
                        connective_used,
                        optional_remainder.clone(),
                    );

                    tmp_e_pathmap.connective_used =
                        tmp_e_pathmap.connective_used || optional_remainder.is_some();
                    Expr {
                        expr_instance: Some(ExprInstance::EPathmapBody(tmp_e_pathmap)),
                    }
                };

            fold_match(known_free, elements, constructor, input, env, parser)
        }
    }
}

// ==========================================================================
// VERBATIM from `normalizer/processes/p_collect_normalizer.rs`, lines 13-39, at commit 6ccf71f2.
// ==========================================================================
fn normalize_p_collect_recursive<'ast>(
    proc: &'ast Collection<'ast>,
    input: ProcVisitInputs,
    env: &HashMap<String, Par>,
    parser: &'ast rholang_parser::RholangParser<'ast>,
) -> Result<ProcVisitOutputs, InterpreterError> {
    let collection_result = normalize_collection_recursive(
        proc,
        CollectVisitInputs {
            bound_map_chain: input.bound_map_chain.clone(),
            free_map: input.free_map.clone(),
        },
        env,
        parser,
    )?;

    let updated_par = prepend_expr(
        input.par,
        collection_result.expr,
        input.bound_map_chain.depth() as i32,
    );

    Ok(ProcVisitOutputs {
        par: updated_par,
        free_map: collection_result.free_map,
    })
}

// ==========================================================================
// VERBATIM from `normalizer/processes/p_par_normalizer.rs`, lines 10-61, at commit 6ccf71f2.
// ==========================================================================
fn flatten_par<'ast>(root: &'ast AnnProc<'ast>) -> Vec<&'ast AnnProc<'ast>> {
    let mut result = Vec::new();
    let mut stack = vec![root];

    while let Some(current) = stack.pop() {
        match &current.proc {
            Proc::Par { left, right } => {
                stack.push(right);
                stack.push(left);
            }
            _ => result.push(current),
        }
    }

    result
}

fn normalize_p_par_recursive<'ast>(
    left: &'ast AnnProc<'ast>,
    right: &'ast AnnProc<'ast>,
    input: ProcVisitInputs,
    env: &HashMap<String, Par>,
    parser: &'ast rholang_parser::RholangParser<'ast>,
) -> Result<ProcVisitOutputs, InterpreterError> {
    let flattened_left = flatten_par(left);
    let flattened_right = flatten_par(right);

    let mut all_procs = Vec::with_capacity(flattened_left.len() + flattened_right.len());
    all_procs.extend(flattened_left);
    all_procs.extend(flattened_right);

    let mut accumulated_par = input.par;
    let mut accumulated_free_map = input.free_map;
    let bound_map_chain = input.bound_map_chain;

    for proc in all_procs {
        let proc_input = ProcVisitInputs {
            par: accumulated_par,
            free_map: accumulated_free_map,
            bound_map_chain: bound_map_chain.clone(),
        };

        let proc_result = normalize_ann_proc_recursive(proc, proc_input, env, parser)?;
        accumulated_par = proc_result.par;
        accumulated_free_map = proc_result.free_map;
    }

    Ok(ProcVisitOutputs {
        par: accumulated_par,
        free_map: accumulated_free_map,
    })
}

// ==========================================================================
// VERBATIM from `normalizer/processes/p_eval_normalizer.rs`, lines 12-34, at commit 6ccf71f2.
// ==========================================================================
fn normalize_p_eval_recursive<'ast>(
    eval_name: &Name<'ast>,
    input: ProcVisitInputs,
    env: &HashMap<String, Par>,
    parser: &'ast rholang_parser::RholangParser<'ast>,
) -> Result<ProcVisitOutputs, InterpreterError> {
    let name_match_result = normalize_name_recursive(
        eval_name,
        NameVisitInputs {
            bound_map_chain: input.bound_map_chain.clone(),
            free_map: input.free_map.clone(),
        },
        env,
        parser,
    )?;

    let updated_par = input.par.append(name_match_result.par.clone());

    Ok(ProcVisitOutputs {
        par: updated_par,
        free_map: name_match_result.free_map,
    })
}

// ==========================================================================
// VERBATIM from `normalizer/processes/p_negation_normalizer.rs`, lines 11-55, at commit 6ccf71f2.
// ==========================================================================
fn normalize_p_negation_recursive<'ast>(
    arg: &'ast Proc<'ast>,
    unary_expr_span: rholang_parser::SourceSpan,
    input: ProcVisitInputs,
    env: &HashMap<String, Par>,
    parser: &'ast rholang_parser::RholangParser<'ast>,
) -> Result<ProcVisitOutputs, InterpreterError> {
    // Use the actual span of the entire UnaryExp (~<expr>) for accurate source location
    let ann_proc = AnnProc {
        proc: arg,
        span: unary_expr_span,
    };

    let body_result = normalize_ann_proc_recursive(
        &ann_proc,
        ProcVisitInputs {
            par: Par::default(),
            bound_map_chain: input.bound_map_chain.clone(),
            free_map: FreeMap::default(),
        },
        env,
        parser,
    )?;

    // Create Connective with ConnNotBody
    let connective = Connective {
        connective_instance: Some(connective::ConnectiveInstance::ConnNotBody(
            body_result.par.clone(),
        )),
    };

    let updated_par = prepend_connective(
        input.par,
        connective.clone(),
        input.bound_map_chain.clone().depth() as i32,
    );

    Ok(ProcVisitOutputs {
        par: updated_par,
        free_map: input.free_map.add_connective(
            connective.connective_instance.unwrap(),
            unary_expr_span, // Use the actual span of the entire negation operation
        ),
    })
}

// ==========================================================================
// VERBATIM from `normalizer/processes/p_conjunction_normalizer.rs`, lines 13-80, at commit 6ccf71f2.
// ==========================================================================
fn normalize_p_conjunction_recursive<'ast>(
    left: &'ast AnnProc<'ast>,
    right: &'ast AnnProc<'ast>,
    input: ProcVisitInputs,
    env: &HashMap<String, Par>,
    parser: &'ast rholang_parser::RholangParser<'ast>,
) -> Result<ProcVisitOutputs, InterpreterError> {
    let left_result = normalize_ann_proc_recursive(
        left,
        ProcVisitInputs {
            par: Par::default(),
            bound_map_chain: input.bound_map_chain.clone(),
            free_map: input.free_map.clone(),
        },
        env,
        parser,
    )?;

    let right_result = normalize_ann_proc_recursive(
        right,
        ProcVisitInputs {
            par: Par::default(),
            bound_map_chain: input.bound_map_chain.clone(),
            free_map: left_result.free_map.clone(),
        },
        env,
        parser,
    )?;

    let lp = left_result.par;
    let result_connective = match lp.single_connective() {
        Some(Connective {
            connective_instance: Some(ConnectiveInstance::ConnAndBody(conn_body)),
        }) => Connective {
            connective_instance: Some(ConnectiveInstance::ConnAndBody(ConnectiveBody {
                ps: {
                    let mut ps = conn_body.ps.clone();
                    ps.push(right_result.par);
                    ps
                },
            })),
        },
        _ => Connective {
            connective_instance: Some(ConnectiveInstance::ConnAndBody(ConnectiveBody {
                ps: vec![lp, right_result.par],
            })),
        },
    };

    let result_par = prepend_connective(
        input.par,
        result_connective.clone(),
        input.bound_map_chain.depth() as i32,
    );

    let updated_free_map = right_result.free_map.add_connective(
        result_connective.connective_instance.unwrap(),
        SourceSpan {
            start: left.span.start,
            end: right.span.end,
        },
    );

    Ok(ProcVisitOutputs {
        par: result_par,
        free_map: updated_free_map,
    })
}

// ==========================================================================
// VERBATIM from `normalizer/processes/p_disjunction_normalizer.rs`, lines 13-80, at commit 6ccf71f2.
// ==========================================================================
fn normalize_p_disjunction_recursive<'ast>(
    left: &'ast AnnProc<'ast>,
    right: &'ast AnnProc<'ast>,
    input: ProcVisitInputs,
    env: &HashMap<String, Par>,
    parser: &'ast rholang_parser::RholangParser<'ast>,
) -> Result<ProcVisitOutputs, InterpreterError> {
    let left_result = normalize_ann_proc_recursive(
        left,
        ProcVisitInputs {
            par: Par::default(),
            bound_map_chain: input.bound_map_chain.clone(),
            free_map: FreeMap::default(),
        },
        env,
        parser,
    )?;

    let right_result = normalize_ann_proc_recursive(
        right,
        ProcVisitInputs {
            par: Par::default(),
            bound_map_chain: input.bound_map_chain.clone(),
            free_map: FreeMap::default(),
        },
        env,
        parser,
    )?;

    let lp = left_result.par;
    let result_connective = match lp.single_connective() {
        Some(Connective {
            connective_instance: Some(ConnectiveInstance::ConnOrBody(conn_body)),
        }) => Connective {
            connective_instance: Some(ConnectiveInstance::ConnOrBody(ConnectiveBody {
                ps: {
                    let mut ps = conn_body.ps.clone();
                    ps.push(right_result.par);
                    ps
                },
            })),
        },
        _ => Connective {
            connective_instance: Some(ConnectiveInstance::ConnOrBody(ConnectiveBody {
                ps: vec![lp, right_result.par],
            })),
        },
    };

    let result_par = prepend_connective(
        input.par.clone(),
        result_connective.clone(),
        input.bound_map_chain.depth() as i32,
    );

    let updated_free_map =
        input
            .free_map
            .add_connective(result_connective.connective_instance.unwrap(), SourceSpan {
                start: left.span.start,
                end: right.span.end,
            });

    Ok(ProcVisitOutputs {
        par: result_par,
        free_map: updated_free_map,
    })
}

// ==========================================================================
// VERBATIM from `normalizer/processes/p_matches_normalizer.rs`, lines 11-53, at commit 6ccf71f2.
// ==========================================================================
fn normalize_p_matches_recursive<'ast>(
    left: &'ast AnnProc<'ast>,
    right: &'ast AnnProc<'ast>,
    input: ProcVisitInputs,
    env: &HashMap<String, Par>,
    parser: &'ast rholang_parser::RholangParser<'ast>,
) -> Result<ProcVisitOutputs, InterpreterError> {
    let left_result = normalize_ann_proc_recursive(
        left,
        ProcVisitInputs {
            par: Par::default(),
            bound_map_chain: input.bound_map_chain.clone(),
            free_map: input.free_map.clone(),
        },
        env,
        parser,
    )?;

    let right_result = normalize_ann_proc_recursive(
        right,
        ProcVisitInputs {
            par: Par::default(),
            bound_map_chain: input.bound_map_chain.clone().push(),
            free_map: FreeMap::default(),
        },
        env,
        parser,
    )?;

    let new_expr = Expr {
        expr_instance: Some(expr::ExprInstance::EMatchesBody(EMatches {
            target: Some(left_result.par.clone()),
            pattern: Some(right_result.par.clone()),
        })),
    };

    let prepend_par = prepend_expr(input.par, new_expr, input.bound_map_chain.depth() as i32);

    Ok(ProcVisitOutputs {
        par: prepend_par,
        free_map: left_result.free_map,
    })
}

// ==========================================================================
// VERBATIM from `normalizer/processes/p_send_normalizer.rs`, lines 15-85, at commit 6ccf71f2.
// ==========================================================================
fn normalize_p_send_recursive<'ast>(
    channel: &'ast Name<'ast>,
    send_type: &SendType,
    inputs: &'ast rholang_parser::ast::ProcList<'ast>,
    input: ProcVisitInputs,
    env: &HashMap<String, Par>,
    parser: &'ast rholang_parser::RholangParser<'ast>,
) -> Result<ProcVisitOutputs, InterpreterError> {
    let name_match_result = normalize_name_recursive(
        channel,
        NameVisitInputs {
            bound_map_chain: input.bound_map_chain.clone(),
            free_map: input.free_map.clone(),
        },
        env,
        parser,
    )?;

    let mut acc = (
        Vec::new(),
        ProcVisitInputs {
            par: Par::default(),
            bound_map_chain: input.bound_map_chain.clone(),
            free_map: name_match_result.free_map.clone(),
        },
        Vec::new(),
        false,
    );

    for proc in inputs.iter() {
        let proc_match_result = normalize_ann_proc_recursive(proc, acc.1.clone(), env, parser)?;

        acc.0.push(proc_match_result.par.clone());
        acc.1 = ProcVisitInputs {
            par: Par::default(),
            bound_map_chain: input.bound_map_chain.clone(),
            free_map: proc_match_result.free_map.clone(),
        };
        acc.2 = union(acc.2.clone(), proc_match_result.par.locally_free.clone());
        acc.3 = acc.3 || proc_match_result.par.connective_used;
    }

    let persistent = match send_type {
        rholang_parser::ast::SendType::Single => false,
        rholang_parser::ast::SendType::Multiple => true,
    };

    let send = Send {
        chan: Some(name_match_result.par.clone()),
        data: acc.0,
        persistent,
        locally_free: union(
            name_match_result.par.clone().locally_free(
                name_match_result.par.clone(),
                input.bound_map_chain.depth() as i32,
            ),
            acc.2,
        ),
        connective_used: name_match_result
            .par
            .connective_used(name_match_result.par.clone())
            || acc.3,
    };

    let updated_par = input.par.clone().prepend_send(send);

    Ok(ProcVisitOutputs {
        par: updated_par,
        free_map: acc.1.free_map,
    })
}

// ==========================================================================
// VERBATIM from `normalizer/processes/p_method_normalizer.rs`, lines 13-86, at commit 6ccf71f2.
// ==========================================================================
fn normalize_p_method_recursive<'ast>(
    receiver: &'ast AnnProc<'ast>,
    name_id: &'ast Id<'ast>,
    args: &'ast rholang_parser::ast::ProcList<'ast>,
    input: ProcVisitInputs,
    env: &HashMap<String, Par>,
    parser: &'ast rholang_parser::RholangParser<'ast>,
) -> Result<ProcVisitOutputs, InterpreterError> {
    let target_result = normalize_ann_proc_recursive(
        receiver,
        ProcVisitInputs {
            par: Par::default(),
            ..input.clone()
        },
        env,
        parser,
    )?;

    let target = target_result.par;

    let init_acc = (
        Vec::new(),
        ProcVisitInputs {
            par: Par::default(),
            bound_map_chain: input.bound_map_chain.clone(),
            free_map: target_result.free_map.clone(),
        },
        Vec::new(),
        false,
    );

    let arg_results = args.iter().rev().try_fold(init_acc, |acc, arg| {
        normalize_ann_proc_recursive(arg, acc.1.clone(), env, parser).map(|proc_match_result| {
            (
                {
                    let mut acc_0 = acc.0.clone();
                    acc_0.insert(0, proc_match_result.par.clone());
                    acc_0
                },
                ProcVisitInputs {
                    par: Par::default(),
                    bound_map_chain: input.bound_map_chain.clone(),
                    free_map: proc_match_result.free_map.clone(),
                },
                union(acc.2.clone(), proc_match_result.par.locally_free.clone()),
                acc.3 || proc_match_result.par.connective_used,
            )
        })
    })?;

    let method = EMethod {
        method_name: name_id.name.to_string(),
        target: Some(target.clone()),
        arguments: arg_results.0,
        locally_free: union(
            target.locally_free(target.clone(), input.bound_map_chain.depth() as i32),
            arg_results.2,
        ),
        connective_used: target.connective_used(target.clone()) || arg_results.3,
    };

    let updated_par = prepend_expr(
        input.par,
        Expr {
            expr_instance: Some(expr::ExprInstance::EMethodBody(method)),
        },
        input.bound_map_chain.depth() as i32,
    );

    Ok(ProcVisitOutputs {
        par: updated_par,
        free_map: arg_results.1.free_map,
    })
}

// ==========================================================================
// VERBATIM from `normalizer/processes/p_bundle_normalizer.rs`, lines 14-127, at commit 6ccf71f2.
// ==========================================================================
fn normalize_p_bundle_recursive<'ast>(
    bundle_type: &BundleType,
    proc: &'ast AnnProc<'ast>,
    input: ProcVisitInputs,
    span: &SourceSpan,
    env: &HashMap<String, Par>,
    parser: &'ast RholangParser<'ast>,
) -> Result<ProcVisitOutputs, InterpreterError> {
    fn error(target_result: ProcVisitOutputs) -> Result<ProcVisitOutputs, InterpreterError> {
        let err_msg = {
            let at = |variable: &str, source_position: &SourceSpan| {
                format!(
                    "{} at line {}, column {}",
                    variable, source_position.start.line, source_position.start.col
                )
            };

            let wildcards_positions: Vec<String> = target_result
                .free_map
                .wildcards
                .iter()
                .map(|pos| at("", pos))
                .collect();

            let free_vars_positions: Vec<String> = target_result
                .free_map
                .level_bindings
                .iter()
                .map(|(name, context)| at(&format!("`{}`", name), &context.source_span))
                .collect();

            let err_msg_wildcards = if !wildcards_positions.is_empty() {
                format!(" Wildcards positions: {}", wildcards_positions.join(", "))
            } else {
                String::new()
            };

            let err_msg_free_vars = if !free_vars_positions.is_empty() {
                format!(
                    " Free variables positions: {}",
                    free_vars_positions.join(", ")
                )
            } else {
                String::new()
            };

            format!(
                "Bundle's content must not have free variables or wildcards.{}{}",
                err_msg_wildcards, err_msg_free_vars
            )
        };

        Err(InterpreterError::UnexpectedBundleContent(format!(
            "Bundle's content must not have free variables or wildcards. {}",
            err_msg
        )))
    }

    let target_result = normalize_ann_proc_recursive(
        proc,
        ProcVisitInputs {
            par: Par::default(),
            ..input.clone()
        },
        env,
        parser,
    )?;

    let outermost_bundle = match bundle_type {
        BundleType::BundleReadWrite => Bundle {
            body: Some(target_result.par.clone()),
            write_flag: true,
            read_flag: true,
        },
        BundleType::BundleRead => Bundle {
            body: Some(target_result.par.clone()),
            write_flag: false,
            read_flag: true,
        },
        BundleType::BundleWrite => Bundle {
            body: Some(target_result.par.clone()),
            write_flag: true,
            read_flag: false,
        },
        BundleType::BundleEquiv => Bundle {
            body: Some(target_result.par.clone()),
            write_flag: false,
            read_flag: false,
        },
    };

    let res = if !target_result.clone().par.connectives.is_empty() {
        Err(InterpreterError::UnexpectedBundleContent(format!(
            "Illegal top-level connective in bundle at line {}, column {}.",
            span.start.line, span.start.col
        )))
    } else if !target_result.clone().free_map.wildcards.is_empty()
        || !target_result.free_map.level_bindings.is_empty()
    {
        error(target_result)
    } else {
        let new_bundle = match target_result.par.single_bundle() {
            Some(single) => BundleOps::merge(&outermost_bundle, &single),
            None => outermost_bundle,
        };

        Ok(ProcVisitOutputs {
            par: { prepend_bundle(input.par.clone(), new_bundle) },
            free_map: input.free_map.clone(),
        })
    };

    res
}

// ==========================================================================
// VERBATIM from `normalizer/processes/p_if_normalizer.rs`, lines 11-88, at commit 6ccf71f2.
// ==========================================================================
fn normalize_p_if_recursive<'ast>(
    condition: &'ast AnnProc<'ast>,
    if_true: &'ast AnnProc<'ast>,
    if_false: Option<&'ast AnnProc<'ast>>,
    mut input: ProcVisitInputs,
    env: &HashMap<String, Par>,
    parser: &'ast rholang_parser::RholangParser<'ast>,
) -> Result<ProcVisitOutputs, InterpreterError> {
    let target_result =
        normalize_ann_proc_recursive(condition, ProcVisitInputs { ..input.clone() }, env, parser)?;

    let true_case_body = normalize_ann_proc_recursive(
        if_true,
        ProcVisitInputs {
            par: Par::default(),
            bound_map_chain: input.bound_map_chain.clone(),
            free_map: target_result.free_map.clone(),
        },
        env,
        parser,
    )?;

    let false_case_body = match if_false {
        Some(false_proc) => normalize_ann_proc_recursive(
            false_proc,
            ProcVisitInputs {
                par: Par::default(),
                bound_map_chain: input.bound_map_chain.clone(),
                free_map: true_case_body.free_map.clone(),
            },
            env,
            parser,
        )?,
        None => {
            let nil_proc_ref = parser.ast_builder().const_nil();
            let nil_ann_proc = rholang_parser::ast::AnnProc {
                proc: nil_proc_ref,
                span: rholang_parser::SourceSpan {
                    start: rholang_parser::SourcePos { line: 0, col: 0 },
                    end: rholang_parser::SourcePos { line: 0, col: 0 },
                },
            };
            normalize_ann_proc_recursive(
                &nil_ann_proc,
                ProcVisitInputs {
                    par: Par::default(),
                    bound_map_chain: input.bound_map_chain.clone(),
                    free_map: true_case_body.free_map.clone(),
                },
                env,
                parser,
            )?
        }
    };

    let desugared_if = If {
        condition: Some(target_result.par.clone()),
        if_true: Some(true_case_body.par.clone()),
        if_false: Some(false_case_body.par.clone()),
        locally_free: union(
            union(
                target_result.par.locally_free.clone(),
                true_case_body.par.locally_free.clone(),
            ),
            false_case_body.par.locally_free.clone(),
        ),
        connective_used: target_result.par.connective_used
            || true_case_body.par.connective_used
            || false_case_body.par.connective_used,
    };

    let updated_par = input.par.prepend_if(desugared_if);

    Ok(ProcVisitOutputs {
        par: updated_par,
        free_map: false_case_body.free_map,
    })
}

// ==========================================================================
// VERBATIM from `normalizer/processes/p_match_normalizer.rs`, lines 13-135, at commit 6ccf71f2.
// ==========================================================================
fn normalize_p_match_recursive<'ast>(
    expression: &'ast AnnProc<'ast>,
    cases: &'ast [Case<'ast>],
    input: ProcVisitInputs,
    env: &HashMap<String, Par>,
    parser: &'ast rholang_parser::RholangParser<'ast>,
) -> Result<ProcVisitOutputs, InterpreterError> {
    let target_result = normalize_ann_proc_recursive(
        expression,
        ProcVisitInputs {
            par: Par::default(),
            ..input.clone()
        },
        env,
        parser,
    )?;

    let mut init_acc = (vec![], target_result.free_map.clone(), Vec::new(), false);

    for case in cases {
        let Case {
            pattern,
            guard,
            proc: case_body,
        } = case;

        // Cost syntax (`{% P %}[s]`, `s :: S`) is a process form (recognized +
        // metered), not a match pattern — reject it in pattern position (W1
        // §1.5). f1r3node's normalizer does not run rholang-lib's resolver, so
        // the guard is applied here at the pattern entry point.
        reject_cost_syntax_in_pattern(pattern)?;

        let pattern_result = normalize_ann_proc_recursive(
            pattern,
            ProcVisitInputs {
                par: Par::default(),
                bound_map_chain: input.bound_map_chain.push(),
                free_map: FreeMap::default(),
            },
            env,
            parser,
        )?;

        let case_env = input
            .bound_map_chain
            .absorb_free_span(&pattern_result.free_map);
        let bound_count = pattern_result.free_map.count_no_wildcards();

        // Optional `where` guard: normalized in the same scope as the
        // case body (pattern bindings absorbed into bound_map_chain).
        // No syntactic check on the guard's content (see plan §3.8) —
        // bool-ness is enforced at runtime by the matcher in Phase 6.
        let guard_result = match guard {
            Some(g) => Some(normalize_ann_proc_recursive(
                g,
                ProcVisitInputs {
                    par: Par::default(),
                    bound_map_chain: case_env.clone(),
                    free_map: init_acc.1.clone(),
                },
                env,
                parser,
            )?),
            None => None,
        };

        let case_body_result = normalize_ann_proc_recursive(
            case_body,
            ProcVisitInputs {
                par: Par::default(),
                bound_map_chain: case_env.clone(),
                free_map: guard_result
                    .as_ref()
                    .map(|gr| gr.free_map.clone())
                    .unwrap_or_else(|| init_acc.1.clone()),
            },
            env,
            parser,
        )?;

        init_acc.0.insert(0, MatchCase {
            pattern: Some(pattern_result.par.clone()),
            source: Some(case_body_result.par.clone()),
            free_count: bound_count as i32,
            guard: guard_result.as_ref().map(|gr| gr.par.clone()),
        });
        init_acc.1 = case_body_result.free_map;
        init_acc.2 = union(
            union(init_acc.2.clone(), pattern_result.par.locally_free.clone()),
            filter_and_adjust_bitset(
                {
                    let mut lf = case_body_result.par.locally_free.clone();
                    if let Some(gr) = &guard_result {
                        lf = union(lf, gr.par.locally_free.clone());
                    }
                    lf
                },
                bound_count,
            ),
        );
        init_acc.3 = init_acc.3
            || case_body_result.par.connective_used
            || guard_result
                .as_ref()
                .map(|gr| gr.par.connective_used)
                .unwrap_or(false);
    }

    let cases: Vec<MatchCase> = init_acc.0.into_iter().rev().collect();
    let locally_free = union(init_acc.2, target_result.par.locally_free.clone());
    let connective_used = init_acc.3 || target_result.par.connective_used;

    let result_match = Match {
        target: Some(target_result.par.clone()),
        cases,
        locally_free,
        connective_used,
    };
    Ok(ProcVisitOutputs {
        par: input.par.clone().prepend_match(result_match.clone()),
        free_map: init_acc.1,
    })
}

// ==========================================================================
// VERBATIM from `normalizer/processes/p_new_normalizer.rs`, lines 14-92, at commit 6ccf71f2.
// ==========================================================================
fn normalize_p_new_recursive<'ast>(
    decls: &[NameDecl<'ast>],
    proc: &'ast AnnProc<'ast>,
    input: ProcVisitInputs,
    env: &HashMap<String, Par>,
    parser: &'ast rholang_parser::RholangParser<'ast>,
) -> Result<ProcVisitOutputs, InterpreterError> {
    // TODO: bindings within a single new shouldn't have overlapping names. - OLD
    let new_tagged_bindings: Vec<(Option<String>, String, VarSort, usize, usize)> = decls
        .iter()
        .map(|decl| match decl {
            NameDecl { id, uri: None } => Ok((
                None,
                id.name.to_string(),
                VarSort::NameSort,
                id.pos.line,
                id.pos.col,
            )),
            NameDecl { id, uri: Some(urn) } => Ok((
                Some((**urn).to_string()), // Dereference Uri to get the inner &str
                id.name.to_string(),
                VarSort::NameSort,
                id.pos.line,
                id.pos.col,
            )),
        })
        .collect::<Result<Vec<_>, InterpreterError>>()?;

    // Sort bindings: None's first, then URI's lexicographically
    let mut sorted_bindings: Vec<(Option<String>, String, VarSort, usize, usize)> =
        new_tagged_bindings;
    sorted_bindings.sort_by(|a, b| a.0.cmp(&b.0));

    let new_bindings: Vec<IdContextPos<VarSort>> = sorted_bindings
        .iter()
        .map(|row| {
            (row.1.clone(), row.2.clone(), SourcePos {
                line: row.3,
                col: row.4,
            })
        })
        .collect();

    let uris: Vec<String> = sorted_bindings
        .iter()
        .filter_map(|row| row.0.clone())
        .collect();

    let new_env: BoundMapChain<VarSort> = input.bound_map_chain.put_all_pos(new_bindings);
    let new_count: usize = new_env.get_count() - input.bound_map_chain.get_count();

    let body_result = normalize_ann_proc_recursive(
        proc,
        ProcVisitInputs {
            par: Par::default(),
            bound_map_chain: new_env.clone(),
            free_map: input.free_map.clone(),
        },
        env,
        parser,
    )?;

    // TODO: we should build btree_map with real values, not a copied references from env: ref &HashMap
    let btree_map: BTreeMap<String, Par> =
        env.iter().map(|(k, v)| (k.clone(), v.clone())).collect();

    let result_new = New {
        bind_count: new_count as i32,
        p: Some(body_result.par.clone()),
        uri: uris,
        injections: btree_map,
        locally_free: filter_and_adjust_bitset(body_result.par.clone().locally_free, new_count),
    };

    Ok(ProcVisitOutputs {
        par: prepend_new(input.par.clone(), result_new),
        free_map: body_result.free_map.clone(),
    })
}

// ==========================================================================
// VERBATIM from `normalizer/processes/p_contr_normalizer.rs`, lines 19-118, at commit 6ccf71f2.
// ==========================================================================
fn normalize_p_contr_recursive<'ast>(
    name: &'ast Name<'ast>,
    formals: &'ast rholang_parser::ast::Names<'ast>,
    body: &'ast AnnProc<'ast>,
    input: ProcVisitInputs,
    env: &HashMap<String, Par>,
    parser: &'ast rholang_parser::RholangParser<'ast>,
) -> Result<ProcVisitOutputs, InterpreterError> {
    let name_match_result = normalize_name_recursive(
        name,
        NameVisitInputs {
            bound_map_chain: input.bound_map_chain.clone(),
            free_map: input.free_map.clone(),
        },
        env,
        parser,
    )?;

    let mut init_acc = (vec![], FreeMap::<VarSort>::default(), Vec::new());

    for name in formals.names.iter() {
        // Reject cost syntax in contract-formal pattern position (W1 §1.5): a
        // signed term / token stack inside a formal `@{...}` is a process form
        // (recognized + metered), not a contract pattern.
        reject_cost_syntax_in_name_pattern(name)?;

        let res = normalize_name_recursive(
            name,
            NameVisitInputs {
                bound_map_chain: input.clone().bound_map_chain.push(),
                free_map: init_acc.1.clone(),
            },
            env,
            parser,
        )?;

        let result = fail_on_invalid_connective(&input, &res)?;

        // Accumulate the result
        init_acc.0.insert(0, result.par.clone());
        init_acc.1 = result.free_map.clone();
        init_acc.2 = union(
            init_acc.clone().2,
            result.par.locally_free(
                result.par.clone(),
                (input.bound_map_chain.depth() + 1) as i32,
            ),
        );
    }

    let remainder_result = normalize_match_name(&formals.remainder, init_acc.1.clone())?;

    let new_enw = input.bound_map_chain.absorb_free_span(&remainder_result.1);
    let bound_count = remainder_result.1.count_no_wildcards();

    let body_result = normalize_ann_proc_recursive(
        body,
        ProcVisitInputs {
            par: Par::default(),
            bound_map_chain: new_enw,
            free_map: name_match_result.free_map.clone(),
        },
        env,
        parser,
    )?;

    let receive = Receive {
        binds: vec![ReceiveBind {
            patterns: init_acc.0.clone().into_iter().rev().collect(),
            source: Some(name_match_result.par.clone()),
            remainder: remainder_result.0.clone(),
            free_count: bound_count as i32,
        }],
        body: Some(body_result.par.clone()),
        persistent: true,
        peek: false,
        bind_count: bound_count as i32,
        locally_free: union(
            name_match_result.par.locally_free(
                name_match_result.par.clone(),
                input.bound_map_chain.depth() as i32,
            ),
            union(
                init_acc.2,
                filter_and_adjust_bitset(body_result.par.clone().locally_free, bound_count),
            ),
        ),
        connective_used: name_match_result
            .par
            .connective_used(name_match_result.par.clone())
            || body_result.par.connective_used(body_result.par.clone()),
        condition: None,
    };
    //TODO: I should create new Expr for prepend_expr and provide it instead of receive.clone().into
    let updated_par = input.clone().par.prepend_receive(receive);
    Ok(ProcVisitOutputs {
        par: updated_par,
        free_map: body_result.free_map,
    })
}

// ==========================================================================
// VERBATIM from `normalizer/processes/p_send_sync_normalizer.rs`, lines 11-110, at commit 6ccf71f2.
// ==========================================================================
fn normalize_p_send_sync_recursive<'ast>(
    channel: &'ast Name<'ast>,
    messages: &'ast rholang_parser::ast::ProcList<'ast>,
    cont: &SyncSendCont<'ast>,
    span: &rholang_parser::SourceSpan,
    input: ProcVisitInputs,
    env: &HashMap<String, Par>,
    parser: &'ast rholang_parser::RholangParser<'ast>,
) -> Result<ProcVisitOutputs, InterpreterError> {
    let identifier = Uuid::new_v4().to_string();

    // Allocate identifier string in the parser's string arena
    let identifier_str = parser.ast_builder().alloc_str(&identifier);

    // Create variable name for the response channel
    let name_var = rholang_parser::ast::Name::NameVar(rholang_parser::ast::Var::Id(Id {
        name: identifier_str,
        pos: span.start,
    }));

    // Build the send process: channel!(name_var, ...messages)
    let send: AnnProc = {
        let mut listproc = Vec::new();

        // Add the response channel name as first argument
        listproc.push(AnnProc {
            proc: parser.ast_builder().alloc_eval(name_var),
            span: *span,
        });

        // Add the original messages
        for msg in messages.iter() {
            listproc.push(*msg);
        }

        AnnProc {
            proc: parser
                .ast_builder()
                .alloc_send(SendType::Single, *channel, &listproc),
            span: *span,
        }
    };

    // Build the receive process: for (_ <- name_var) { cont }
    let receive: AnnProc = {
        // Create wildcard pattern
        let wildcard = rholang_parser::ast::Name::NameVar(rholang_parser::ast::Var::Wildcard);

        // Create bind for the pattern: _ <- name_var
        let bind = Bind::Linear {
            lhs: rholang_parser::ast::Names {
                names: smallvec::SmallVec::from_vec(vec![wildcard]),
                remainder: None,
            },
            rhs: rholang_parser::ast::Source::Simple { name: name_var },
        };

        // Create receipt containing the bind
        let receipt: smallvec::SmallVec<[Bind<'ast>; 1]> = smallvec::SmallVec::from_vec(vec![bind]);
        let receipts: smallvec::SmallVec<[smallvec::SmallVec<[Bind<'ast>; 1]>; 1]> =
            smallvec::SmallVec::from_vec(vec![receipt]);

        // Get the continuation process
        let cont_proc = match cont {
            SyncSendCont::Empty => AnnProc {
                proc: parser.ast_builder().const_nil(),
                span: *span,
            },
            SyncSendCont::NonEmpty(proc) => *proc,
        };

        AnnProc {
            proc: parser.ast_builder().alloc_for(receipts, cont_proc),
            span: *span,
        }
    };

    // Create name declaration for the new variable
    let name_decl = rholang_parser::ast::NameDecl {
        id: Id {
            name: identifier_str,
            pos: span.start,
        },
        uri: None,
    };

    // Build Par of send and receive
    let p_par = AnnProc {
        proc: parser.ast_builder().alloc_par(send, receive),
        span: *span,
    };

    // Build New process: new name_var in { send | receive }
    let p_new = AnnProc {
        proc: parser.ast_builder().alloc_new(p_par, vec![name_decl]),
        span: *span,
    };

    normalize_ann_proc_recursive(&p_new, input, env, parser)
}

// ==========================================================================
// VERBATIM from `normalizer/processes/p_let_normalizer.rs`, lines 17-366, at commit 6ccf71f2.
// ==========================================================================
fn normalize_p_let_recursive<'ast>(
    bindings: &'ast smallvec::SmallVec<[LetBinding<'ast>; 1]>,
    body: &'ast AnnProc<'ast>,
    concurrent: bool,
    let_span: SourceSpan,
    input: ProcVisitInputs,
    env: &HashMap<String, Par>,
    parser: &'ast rholang_parser::RholangParser<'ast>,
) -> Result<ProcVisitOutputs, InterpreterError> {
    if concurrent {
        // RHOLANG-RS IMPROVEMENT: Could use semantic naming based on actual variable names
        // e.g., "__let_x_0_L5C10" for variable 'x' at binding index 0, line 5, col 10
        // This would extract name hints from lhs.name for Single bindings and lhs for Multiple
        let variable_names: Vec<String> = (0..bindings.len())
            .map(|_| Uuid::new_v4().to_string())
            .collect();

        // Create send processes for each binding
        let mut send_processes = Vec::new();

        for (i, binding) in bindings.iter().enumerate() {
            let variable_name = &variable_names[i];

            // LetBinding is now a struct, not an enum
            let rhs = &binding.rhs;
            if binding.rhs.len() == 1 {
                // Single binding: one rhs value
                let rhs_span = rhs[0].span;
                let variable_span = SpanContext::variable_span_from_binding(rhs_span, i);
                let send_span = SpanContext::synthetic_construct_span(rhs_span, 10); // Offset to mark as send

                // Create send: variable_name!(rhs)
                let send_proc = AnnProc {
                    proc: parser.ast_builder().alloc_send(
                        SendType::Single,
                        Name::NameVar(Var::Id(Id {
                            name: parser.ast_builder().alloc_str(variable_name),
                            pos: variable_span.start,
                        })),
                        &[rhs[0]],
                    ),
                    span: send_span,
                };
                send_processes.push(send_proc);
            } else {
                // Multiple binding: multiple rhs values
                // Derive span from range of all rhs expressions
                let rhs_span = if rhs.len() > 1 {
                    SpanContext::merge_two_spans(rhs[0].span, rhs[rhs.len() - 1].span)
                } else if rhs.len() == 1 {
                    rhs[0].span
                } else {
                    let_span // Fallback to let construct span
                };
                let variable_span = SpanContext::variable_span_from_binding(rhs_span, i);
                // RHOLANG-RS IMPROVEMENT: Could use SpanContext::send_span_from_binding for better accuracy
                let send_span = SpanContext::synthetic_construct_span(rhs_span, 10); // Offset to mark as send

                // Create send: variable_name!(rhs[0], rhs[1], ...)
                let send_proc = AnnProc {
                    proc: parser.ast_builder().alloc_send(
                        SendType::Single,
                        Name::NameVar(Var::Id(Id {
                            name: parser.ast_builder().alloc_str(variable_name),
                            pos: variable_span.start,
                        })),
                        rhs,
                    ),
                    span: send_span,
                };
                send_processes.push(send_proc);
            }
        }

        // Create input process binds for each binding
        let mut input_binds: Vec<smallvec::SmallVec<[Bind<'ast>; 1]>> = Vec::new();

        for (i, binding) in bindings.iter().enumerate() {
            let variable_name = &variable_names[i];

            // LetBinding is now a struct, not an enum
            let lhs = &binding.lhs;
            let rhs = &binding.rhs;

            if binding.lhs.names.len() == 1
                && binding.lhs.remainder.is_none()
                && binding.rhs.len() == 1
            {
                // Single binding: one name, one rhs value
                // Derive spans from actual rhs location (lhs no longer has span)
                let lhs_span = rhs[0].span;
                let variable_span = SpanContext::variable_span_from_binding(lhs_span, i);

                // Create bind: lhs <- variable_name
                let bind = Bind::Linear {
                    lhs: Names {
                        names: smallvec::SmallVec::from_vec(vec![lhs.names[0]]),
                        remainder: None,
                    },
                    rhs: Source::Simple {
                        name: Name::NameVar(Var::Id(Id {
                            name: parser.ast_builder().alloc_str(variable_name),
                            pos: variable_span.start,
                        })),
                    },
                };
                input_binds.push(smallvec::SmallVec::from_vec(vec![bind]));
            } else {
                // Multiple binding
                // RHOLANG-RS IMPROVEMENT: For Multiple bindings, lhs is Var<'ast>, not AnnName<'ast>
                // Could extract precise position from Var::Id(id) => id.pos, vs Var::Wildcard (no position)
                // Currently deriving from first rhs, but should distinguish between these cases
                let lhs_span = rhs.first().map(|r| r.span).unwrap_or(let_span); // Use first rhs or let span
                let variable_span = SpanContext::variable_span_from_binding(lhs_span, i);

                // Create bind: use lhs names from binding, add wildcards for extra values
                let mut names = lhs.names.to_vec();

                // Add wildcards for remaining values if rhs has more than lhs names
                // RHOLANG-RS LIMITATION: Var::Wildcard has no position data in rholang-rs
                // Our wildcard_span_with_context approach is actually optimal given this constraint
                while names.len() < rhs.len() {
                    names.push(Name::NameVar(Var::Wildcard));
                }

                let bind = Bind::Linear {
                    lhs: Names {
                        names: smallvec::SmallVec::from_vec(names),
                        remainder: lhs.remainder,
                    },
                    rhs: Source::Simple {
                        name: Name::NameVar(Var::Id(Id {
                            name: parser.ast_builder().alloc_str(variable_name),
                            pos: variable_span.start,
                        })),
                    },
                };
                input_binds.push(smallvec::SmallVec::from_vec(vec![bind]));
            }
        }

        // Create the for-comprehension (input process)
        // Use body span as this is the primary process being executed
        let for_comprehension = AnnProc {
            proc: parser.ast_builder().alloc_for(input_binds, *body),
            span: body.span, // Use actual body span for accurate debugging
        };

        // Create parallel composition of all sends and the for-comprehension
        let mut all_processes = send_processes;
        all_processes.push(for_comprehension);

        // Build parallel composition
        let par_proc = if all_processes.len() == 1 {
            all_processes[0]
        } else {
            // Create initial parallel composition with meaningful span
            let first_span = all_processes[0].span;
            let second_span = all_processes[1].span;
            let initial_par_span = SpanContext::merge_two_spans(first_span, second_span);

            let mut result = AnnProc {
                proc: parser
                    .ast_builder()
                    .alloc_par(all_processes[0], all_processes[1]),
                span: initial_par_span,
            };

            // Add remaining processes, expanding span to cover all
            for proc in all_processes.iter().skip(2) {
                let expanded_span = SpanContext::merge_two_spans(result.span, proc.span);
                result = AnnProc {
                    proc: parser.ast_builder().alloc_par(result, *proc),
                    span: expanded_span,
                };
            }
            result
        };

        // Create new declaration with all variable names
        let name_decls: Vec<NameDecl> = variable_names
            .into_iter()
            .enumerate()
            .map(|(idx, name)| {
                let decl_span = SpanContext::variable_span_from_binding(let_span, idx);
                NameDecl {
                    id: Id {
                        name: parser.ast_builder().alloc_str(&name),
                        pos: decl_span.start,
                    },
                    uri: None,
                }
            })
            .collect();

        // The new process spans the entire let construct
        let new_proc = AnnProc {
            proc: parser.ast_builder().alloc_new(par_proc, name_decls),
            span: let_span, // Use the original let span for the entire construct
        };

        // Normalize the constructed new process
        normalize_ann_proc_recursive(&new_proc, input, env, parser)
    } else {
        // Sequential let declarations - similar to LinearDecls in original
        // Transform into match process

        if bindings.is_empty() {
            // Empty bindings - just normalize the body
            return normalize_ann_proc_recursive(body, input, env, parser);
        }

        // For sequential let, we process one binding at a time
        // let x <- rhs in body becomes match rhs { x => body }

        let first_binding = &bindings[0];
        let lhs = &first_binding.lhs;
        let rhs = &first_binding.rhs;

        if lhs.names.len() == 1 && lhs.remainder.is_none() && rhs.len() == 1 {
            // Single binding: one name, one rhs value
            // RHOLANG-RS: Single bindings have Name<'ast> (no longer AnnName with span)
            // Use rhs span as context for lhs operations
            let rhs_span = rhs[0].span;
            let lhs_span = rhs_span; // Use rhs span as context since lhs has no span
            let pattern_span = SpanContext::synthetic_construct_span(lhs_span, 5); // Offset for pattern

            // Create match case
            let match_case = rholang_parser::ast::Case {
                pattern: AnnProc {
                    proc: parser.ast_builder().alloc_list(&[AnnProc {
                        proc: parser.ast_builder().alloc_eval(lhs.names[0]),
                        span: lhs_span, // Use actual lhs span
                    }]),
                    span: pattern_span, // Use synthetic pattern span
                },
                guard: None,
                proc: if bindings.len() > 1 {
                    // More bindings - create nested let
                    let remaining_bindings: smallvec::SmallVec<[LetBinding<'ast>; 1]> =
                        smallvec::SmallVec::from_vec(bindings[1..].to_vec());
                    let nested_span = SpanContext::merge_two_spans(body.span, let_span);
                    AnnProc {
                        proc: parser
                            .ast_builder()
                            .alloc_let(remaining_bindings, *body, false),
                        span: nested_span, // Use merged span for nested let
                    }
                } else {
                    // Last binding - use body directly
                    *body
                },
            };

            // Create match expression from rhs
            let match_expr_span = rhs_span;
            let match_expr = AnnProc {
                proc: parser.ast_builder().alloc_list(&[rhs[0]]),
                span: match_expr_span, // Use actual rhs span
            };

            // Create match process spanning from rhs to body
            let match_span = SpanContext::merge_two_spans(rhs_span, body.span);
            let match_proc = AnnProc {
                proc: parser
                    .ast_builder()
                    .alloc_match(match_expr, &[match_case.pattern, match_case.proc]),
                span: match_span, // Use derived match span
            };

            normalize_ann_proc_recursive(&match_proc, input, env, parser)
        } else {
            // Multiple binding: let x <- (rhs1, rhs2, ...) in body
            // becomes: match [rhs1, rhs2, ...] { [x, _, _, ...] => body }

            // RHOLANG-RS IMPROVEMENT: Could leverage lhs position data more precisely
            // For Var::Id(id), use id.pos directly; for Var::Wildcard, no position available
            // Currently using first rhs as context, but could be more semantic
            let lhs_span = rhs.first().map(|r| r.span).unwrap_or(let_span); // Use first rhs or let span
            let rhs_list_span = if rhs.len() > 1 {
                SpanContext::merge_two_spans(rhs[0].span, rhs[rhs.len() - 1].span)
            } else if rhs.len() == 1 {
                rhs[0].span
            } else {
                let_span // Fallback
            };

            // Create pattern elements from lhs names
            let lhs_name_span = SpanContext::synthetic_construct_span(lhs_span, 0);
            let mut pattern_elements: Vec<AnnProc> = lhs
                .names
                .iter()
                .map(|name| AnnProc {
                    proc: parser.ast_builder().alloc_eval(*name),
                    span: lhs_name_span,
                })
                .collect();

            // Add wildcards for remaining values if rhs has more than lhs names
            while pattern_elements.len() < rhs.len() {
                let wildcard_span = SpanContext::wildcard_span_with_context(lhs_span);
                pattern_elements.push(AnnProc {
                    proc: parser.ast_builder().const_wild(),
                    span: wildcard_span,
                });
            }

            let pattern_list_span = SpanContext::synthetic_construct_span(lhs_span, 10);
            let match_case = rholang_parser::ast::Case {
                pattern: AnnProc {
                    proc: parser.ast_builder().alloc_list(&pattern_elements),
                    span: pattern_list_span,
                },
                guard: None,
                proc: if bindings.len() > 1 {
                    // More bindings - create nested let
                    let remaining_bindings: smallvec::SmallVec<[LetBinding<'ast>; 1]> =
                        smallvec::SmallVec::from_vec(bindings[1..].to_vec());
                    let nested_span = SpanContext::merge_two_spans(body.span, let_span);
                    AnnProc {
                        proc: parser
                            .ast_builder()
                            .alloc_let(remaining_bindings, *body, false),
                        span: nested_span,
                    }
                } else {
                    // Last binding - use body directly
                    *body
                },
            };

            // Create match expression from rhs list
            let match_expr = AnnProc {
                proc: parser.ast_builder().alloc_list(rhs),
                span: rhs_list_span, // Use span covering all rhs expressions
            };

            // Create match process
            let match_span = SpanContext::merge_two_spans(rhs_list_span, body.span);
            let match_proc = AnnProc {
                proc: parser
                    .ast_builder()
                    .alloc_match(match_expr, &[match_case.pattern, match_case.proc]),
                span: match_span, // Use span from rhs to body
            };

            normalize_ann_proc_recursive(&match_proc, input, env, parser)
        }
    }
}

// ==========================================================================
// VERBATIM from `normalizer/processes/p_input_normalizer.rs`, lines 27-568, at commit 6ccf71f2.
// ==========================================================================
fn normalize_p_input_recursive<'ast>(
    receipts: &'ast Receipts<'ast>,
    body: &'ast AnnProc<'ast>,
    input: ProcVisitInputs,
    env: &HashMap<String, Par>,
    parser: &'ast rholang_parser::RholangParser<'ast>,
) -> Result<ProcVisitOutputs, InterpreterError> {
    fn create_ann_proc_with_span<'ast>(proc: &'ast Proc<'ast>, span: SourceSpan) -> AnnProc<'ast> {
        AnnProc { proc, span }
    }

    if receipts.is_empty() || receipts[0].is_empty() {
        return Err(InterpreterError::BugFoundError(
            "Expected at least one receipt".to_string(),
        ));
    }

    // Multiple receipt groups (separated by `;`) are desugared into nested
    // for loops, matching Scala's PInputNormalizer behavior.
    //   for (@a <- ch1 where g1; @b <- ch2 where g2) { body }
    // becomes:
    //   for (@a <- ch1 where g1) { for (@b <- ch2 where g2) { body } }
    // Each receipt's original `where` guard is preserved on its own
    // nested `for` via alloc_for_with_guards.
    if receipts.len() > 1 {
        let desugared = receipts
            .iter()
            .rev()
            .fold(*body, |acc_body, receipt_group| AnnProc {
                proc: parser.ast_builder().alloc_for_with_guards(
                    vec![(receipt_group.binds.to_vec(), receipt_group.guard)],
                    acc_body,
                ),
                span: body.span,
            });
        return normalize_ann_proc_recursive(&desugared, input, env, parser);
    }

    let head_receipt = &receipts[0][0];

    let receipt_contains_complex_source = match head_receipt {
        Bind::Linear { rhs, .. } => match rhs {
            Source::Simple { .. } => false,
            _ => true,
        },
        _ => false,
    };

    if receipt_contains_complex_source {
        let mut list_linear_bind: Vec<Bind<'ast>> = Vec::new();
        let mut list_name_decl: Vec<rholang_parser::ast::NameDecl<'ast>> = Vec::new();

        let (sends_proc, continuation_proc): (AnnProc<'ast>, AnnProc<'ast>) = receipts
            .iter()
            .flat_map(|receipt_group| receipt_group.iter())
            .try_fold(
                (
                    // Initial sends (Nil) - inherit span from for-comprehension
                    // TODO: Update zero span
                    create_ann_proc_with_span(
                        parser.ast_builder().const_nil(),
                        SpanContext::zero_span(), // Inherit from for-comprehension
                    ),
                    // Initial continuation (original body)
                    *body,
                ),
                |(sends, continuation), bind| {
                    match bind {
                        Bind::Linear { lhs, rhs } => {
                            let identifier = Uuid::new_v4().to_string();
                            // Create temporary variable - point to binding site
                            // TODO: Update zero span
                            let binding_span = SpanContext::zero_span();
                            let temp_var = Name::NameVar(rholang_parser::ast::Var::Id(
                                rholang_parser::ast::Id {
                                    name: parser.ast_builder().alloc_str(&identifier),
                                    pos: binding_span.start, // Point to binding declaration
                                },
                            ));

                            match rhs {
                                Source::Simple { .. } => {
                                    // Simple source - just add to list
                                    list_linear_bind.push(bind.clone());
                                    Ok((sends, continuation))
                                }

                                Source::ReceiveSend { name, .. } => {
                                    // ReceiveSend desugaring: x <- name?() becomes x, temp <- name & temp!()
                                    let mut new_names = lhs.names.clone();
                                    new_names.push(temp_var.clone());

                                    list_linear_bind.push(Bind::Linear {
                                        lhs: rholang_parser::ast::Names {
                                            names: new_names,
                                            remainder: lhs.remainder.clone(),
                                        },
                                        rhs: Source::Simple { name: *name },
                                    });

                                    // Add send: temp!()
                                    // TODO: Update zero span
                                    let temp_send = create_ann_proc_with_span(
                                        parser.ast_builder().alloc_send(
                                            rholang_parser::ast::SendType::Single,
                                            temp_var,
                                            &[],
                                        ),
                                        SpanContext::zero_span(), // Inherit from for-comprehension
                                    );

                                    let new_continuation = AnnProc {
                                        proc: parser
                                            .ast_builder()
                                            .alloc_par(temp_send, continuation),
                                        span: continuation.span,
                                    };

                                    Ok((sends, new_continuation))
                                }

                                Source::SendReceive { name, inputs, .. } => {
                                    // SendReceive desugaring: x <- name!(args) becomes new temp in { name!(temp, args) | x <- temp }
                                    list_name_decl.push(rholang_parser::ast::NameDecl {
                                        id: rholang_parser::ast::Id {
                                            name: parser.ast_builder().alloc_str(&identifier),
                                            pos: SourcePos { line: 0, col: 0 },
                                        },
                                        uri: None,
                                    });

                                    list_linear_bind.push(Bind::Linear {
                                        lhs: lhs.clone(),
                                        rhs: Source::Simple {
                                            name: temp_var.clone(),
                                        },
                                    });

                                    // Prepend temp variable to inputs
                                    let mut new_inputs = Vec::new();
                                    // TODO: Update zero span
                                    new_inputs.push(create_ann_proc_with_span(
                                        parser.ast_builder().alloc_eval(temp_var),
                                        SpanContext::zero_span(), // Inherit from for-comprehension
                                    ));
                                    new_inputs.extend(inputs.iter().cloned());

                                    // Create new send
                                    let new_send = AnnProc {
                                        proc: parser.ast_builder().alloc_send(
                                            rholang_parser::ast::SendType::Single,
                                            *name,
                                            &new_inputs,
                                        ),
                                        span: SourceSpan {
                                            start: SourcePos { line: 0, col: 0 },
                                            end: SourcePos { line: 0, col: 0 },
                                        },
                                    };

                                    let new_sends = AnnProc {
                                        proc: parser.ast_builder().alloc_par(new_send, sends),
                                        span: sends.span,
                                    };

                                    Ok((new_sends, continuation))
                                }
                            }
                        }
                        _ => Err(InterpreterError::BugFoundError(format!(
                            "Expected Linear bind in complex source desugaring, found {:?}",
                            bind
                        ))),
                    }
                },
            )?;

        // Create the desugared ForComprehension
        let desugared_for_comprehension = AnnProc {
            proc: parser
                .ast_builder()
                .alloc_for(vec![list_linear_bind], continuation_proc),
            span: body.span,
        };

        // Create final process (New + Par if needed)
        let final_proc = if list_name_decl.is_empty() {
            desugared_for_comprehension
        } else {
            let par_proc = AnnProc {
                proc: parser
                    .ast_builder()
                    .alloc_par(sends_proc, desugared_for_comprehension),
                span: body.span,
            };

            AnnProc {
                proc: parser.ast_builder().alloc_new(par_proc, list_name_decl),
                span: body.span,
            }
        };

        // Recursively normalize the desugared process
        normalize_ann_proc_recursive(&final_proc, input, env, parser)
    } else {
        // Simple source handling - similar to original's else branch

        // Convert receipts to the format expected by processing functions
        // Note: We flatten the nested SmallVec structure since input normalizer expects a flat list
        let flat_receipts: Vec<&Bind<'ast>> = receipts
            .iter()
            .flat_map(|receipt_group| receipt_group.iter())
            .collect();

        let processed_receipts: Result<Vec<_>, InterpreterError> = flat_receipts
            .iter()
            .map(|receipt| match receipt {
                Bind::Linear { lhs, rhs } => {
                    let names: Vec<_> = lhs.names.iter().collect();
                    let remainder = &lhs.remainder;

                    let source_name = match rhs {
                        Source::Simple { name } => name,
                        _ => {
                            return Err(InterpreterError::ParserError(
                                "Only simple sources supported in current implementation"
                                    .to_string(),
                            ))
                        }
                    };

                    Ok(((names, remainder), source_name))
                }
                Bind::Repeated { lhs, rhs } => {
                    let names: Vec<_> = lhs.names.iter().collect();
                    let remainder = &lhs.remainder;
                    Ok(((names, remainder), rhs))
                }
                Bind::Peek { lhs, rhs } => {
                    let names: Vec<_> = lhs.names.iter().collect();
                    let remainder = &lhs.remainder;
                    Ok(((names, remainder), rhs))
                }
                // Cost-accounted per-clause signed bind `{% y <- x %}[s]` (W1). The
                // Phase-4 signed-JOIN dispatch (`normalize.rs` ForComprehension arm
                // → `recognize_signed_join_recursive` → `strip_signed_binds`) demotes EVERY
                // `Bind::Signed` to its linear bind BEFORE `normalize_p_input_recursive` sees
                // it, so this arm is unreachable in normal operation (the
                // `debug_assert` catches a dispatch regression). The release
                // fallback treats it as its underlying LINEAR bind — identical
                // receive shape / COMM count to the unsigned form, metered to the
                // deploy envelope — so a dispatch bug degrades gracefully rather
                // than miscompiling.
                Bind::Signed { lhs, rhs, .. } => {
                    debug_assert!(
                        false,
                        "Bind::Signed must be stripped by recognize_signed_join_recursive before \
                         normalize_p_input_recursive (W1 Phase 4 dispatch)"
                    );
                    let names: Vec<_> = lhs.names.iter().collect();
                    let remainder = &lhs.remainder;

                    let source_name = match rhs {
                        Source::Simple { name } => name,
                        _ => {
                            return Err(InterpreterError::ParserError(
                                "Only simple sources supported in current implementation"
                                    .to_string(),
                            ))
                        }
                    };

                    Ok(((names, remainder), source_name))
                }
            })
            .collect();

        let processed = processed_receipts?;

        // Determine bind characteristics from first receipt
        let (persistent, peek) = match head_receipt {
            Bind::Linear { .. } => (false, false),
            Bind::Repeated { .. } => (true, false),
            Bind::Peek { .. } => (false, true),
            // A cost-accounted signed bind is the linear-receive form (non-
            // persistent, non-peek); the signature decorates, it does not change
            // the COMM shape (W1, recognition-only in Phase 1).
            Bind::Signed { .. } => (false, false),
        };

        // Extract patterns and sources
        let (patterns, sources): (Vec<_>, Vec<_>) = processed.into_iter().unzip();

        // Process sources using new AST name normalizer
        fn process_sources<'ast>(
            sources: Vec<&'ast rholang_parser::ast::Name<'ast>>,
            input: ProcVisitInputs,
            env: &HashMap<String, Par>,
            parser: &'ast rholang_parser::RholangParser<'ast>,
        ) -> Result<(Vec<Par>, FreeMap<VarSort>, BitSet, bool), InterpreterError> {
            let mut vector_par = Vec::new();
            let mut current_known_free = input.free_map;
            let mut locally_free = Vec::new();
            let mut connective_used = false;

            for name in sources {
                let NameVisitOutputs {
                    par,
                    free_map: updated_known_free,
                } = normalize_name_recursive(
                    name,
                    NameVisitInputs {
                        bound_map_chain: input.bound_map_chain.clone(),
                        free_map: current_known_free,
                    },
                    env,
                    parser,
                )?;

                vector_par.push(par.clone());
                current_known_free = updated_known_free;
                locally_free = union(
                    locally_free,
                    par.locally_free(par.clone(), input.bound_map_chain.depth() as i32),
                );
                connective_used = connective_used || par.clone().connective_used(par);
            }

            Ok((
                vector_par,
                current_known_free,
                locally_free,
                connective_used,
            ))
        }

        fn process_patterns<'ast>(
            patterns: Vec<(
                Vec<&'ast Name<'ast>>,
                &Option<rholang_parser::ast::Var<'ast>>,
            )>,
            input: ProcVisitInputs,
            env: &HashMap<String, Par>,
            parser: &'ast rholang_parser::RholangParser<'ast>,
        ) -> Result<
            Vec<(
                Vec<Par>,
                Option<models::rhoapi::Var>,
                FreeMap<VarSort>,
                BitSet,
            )>,
            InterpreterError,
        > {
            patterns
                .into_iter()
                .map(|(names, name_remainder)| {
                    let mut vector_par = Vec::new();
                    let mut current_known_free = FreeMap::new();
                    let mut locally_free = Vec::new();

                    for name in names {
                        // Reject cost syntax in receive-bind pattern position (W1
                        // §1.5): a signed term / token stack inside a bound name
                        // `@{...}` is a process form (recognized + metered), not a
                        // receive pattern.
                        reject_cost_syntax_in_name_pattern(name)?;

                        let NameVisitOutputs {
                            par,
                            free_map: updated_known_free,
                        } = normalize_name_recursive(
                            name,
                            NameVisitInputs {
                                bound_map_chain: input.bound_map_chain.push(),
                                free_map: current_known_free,
                            },
                            env,
                            parser,
                        )?;

                        fail_on_invalid_connective(&input, &NameVisitOutputs {
                            par: par.clone(),
                            free_map: updated_known_free.clone(),
                        })?;

                        vector_par.push(par.clone());
                        current_known_free = updated_known_free;
                        locally_free = union(
                            locally_free,
                            par.locally_free(par.clone(), input.bound_map_chain.depth() as i32 + 1),
                        );
                    }

                    let (optional_var, known_free) =
                        normalize_match_name(name_remainder, current_known_free)?;

                    Ok((vector_par, optional_var, known_free, locally_free))
                })
                .collect()
        }

        let processed_patterns = process_patterns(patterns, input.clone(), env, parser)?;
        let processed_sources = process_sources(sources, input.clone(), env, parser)?;
        let (sources_par, sources_free, sources_locally_free, sources_connective_used) =
            processed_sources;

        // Pre-sort binds using span-aware version
        let receive_binds_and_free_maps = pre_sort_binds(
            processed_patterns
                .clone()
                .into_iter()
                .zip(sources_par)
                .into_iter()
                .map(|((a, b, c, _), e)| (a, b, e, c))
                .collect(),
        )?;

        let (receive_binds, receive_bind_free_maps): (Vec<ReceiveBind>, Vec<FreeMap<VarSort>>) =
            receive_binds_and_free_maps.into_iter().unzip();

        // Channel duplicate check
        let channels: Vec<Par> = receive_binds
            .clone()
            .into_iter()
            .map(|rb| rb.source.unwrap())
            .collect();

        let channels_set: HashSet<Par> = channels.clone().into_iter().collect();
        let has_same_channels = channels.len() > channels_set.len();

        if has_same_channels {
            // TODO: Review
            return Err(InterpreterError::ReceiveOnSameChannelsError {
                source_span: body.span,
            });
        }

        // Merge receive bind free maps
        let receive_binds_free_map = receive_bind_free_maps.into_iter().try_fold(
            FreeMap::new(),
            |known_free, receive_bind_free_map| {
                let (updated_known_free, conflicts) = known_free.merge(receive_bind_free_map);

                if conflicts.is_empty() {
                    Ok(updated_known_free)
                } else {
                    let (shadowing_var, source_span) = &conflicts[0];
                    let original_span =
                        unwrap_option_safe(known_free.get(shadowing_var))?.source_span;
                    Err(InterpreterError::UnexpectedReuseOfNameContextFree {
                        var_name: shadowing_var.to_string(),
                        first_use: original_span,
                        second_use: *source_span,
                    })
                }
            },
        )?;

        let body_env = input
            .bound_map_chain
            .absorb_free_span(&receive_binds_free_map);

        // Optional `where`-clause guard. By the time we reach this branch
        // there is a single receipt (multi-receipt is desugared into nested
        // `for`s above), so its guard is the one we care about. Normalize
        // the guard against the same scope as the body — the merged-bind
        // free map has been absorbed into bound_map_chain, so guard and
        // body see the same de Bruijn levels.
        let guard_result = match receipts[0].guard.as_ref().map(|g| g) {
            Some(guard) => Some(normalize_ann_proc_recursive(
                guard,
                ProcVisitInputs {
                    par: Par::default(),
                    bound_map_chain: body_env.clone(),
                    free_map: sources_free.clone(),
                },
                env,
                parser,
            )?),
            None => None,
        };

        // Process body
        let proc_visit_outputs = normalize_ann_proc_recursive(
            body,
            ProcVisitInputs {
                par: Par::default(),
                bound_map_chain: body_env,
                free_map: guard_result
                    .as_ref()
                    .map(|gr| gr.free_map.clone())
                    .unwrap_or(sources_free),
            },
            env,
            parser,
        )?;

        let bind_count = receive_binds_free_map.count_no_wildcards();

        let guard_par = guard_result.as_ref().map(|gr| gr.par.clone());
        let guard_locally_free = guard_result
            .as_ref()
            .map(|gr| gr.par.locally_free.clone())
            .unwrap_or_default();
        let guard_connective_used = guard_result
            .as_ref()
            .map(|gr| gr.par.connective_used)
            .unwrap_or(false);

        Ok(ProcVisitOutputs {
            par: input.par.clone().prepend_receive(Receive {
                binds: receive_binds,
                body: Some(proc_visit_outputs.clone().par),
                persistent,
                peek,
                bind_count: bind_count as i32,
                locally_free: {
                    union(
                        sources_locally_free,
                        union(
                            processed_patterns
                                .into_iter()
                                .map(|pattern| pattern.3)
                                .fold(Vec::new(), |locally_free1, locally_free2| {
                                    union(locally_free1, locally_free2)
                                }),
                            filter_and_adjust_bitset(
                                union(proc_visit_outputs.par.locally_free, guard_locally_free),
                                bind_count,
                            ),
                        ),
                    )
                },
                connective_used: sources_connective_used
                    || proc_visit_outputs.par.connective_used
                    || guard_connective_used,
                condition: guard_par,
            }),
            free_map: proc_visit_outputs.free_map,
        })
    }
}

// ==========================================================================
// VERBATIM from `normalizer/cost_accounting/recognize.rs`, lines 35-121, at commit 6ccf71f2.
// ==========================================================================
fn recognize_signed_term_recursive<'ast>(
    inner: &'ast AnnProc<'ast>,
    sig: &'ast Signature<'ast>,
    input: ProcVisitInputs,
    env: &HashMap<String, Par>,
    parser: &'ast RholangParser<'ast>,
) -> Result<ProcVisitOutputs, InterpreterError> {
    // Lollipop `s1 -o s2`: re-sign the continuation with `s2` (an AST rewrite),
    // fund the rendezvous with `s1`. A core sig applies uniform-signing so a `for`
    // continuation is metered to the same `s`. Both are term-level AST→AST; the
    // inner gate (if any) is produced by the ordinary dispatch recursion below.
    let (core_inner, core_sig): (AnnProc<'ast>, &Signature<'ast>) = match sig {
        Signature::Transfer(s1, s2) => (desugar::lollipop(*inner, &**s2, parser)?, &**s1),
        core => (desugar::uniform_sign(*inner, core, parser), core),
    };
    // VALIDATE that `s` resolves to a native funding `Sig` (rejects a wildcard
    // `_`, a quoted-principal `@P` ground sig, or a bare lollipop in fundable
    // position). Per-redex attribution is the REDUCER's channel match (Phase 3,
    // `metering::note_channel_lane`) on the COMM's resolved channel — NOT a
    // normalizer-side binding (the `Par` carries no signature field). So
    // recognition only validates + lowers `P`; under s₀ every COMM attributes to
    // the deploy envelope.
    signature_to_native_sig_recursive(core_sig, &input.bound_map_chain, env, parser)?;
    normalize_ann_proc_recursive(&core_inner, input, env, parser)
}

/// `s :: S` bare token stack at PROC level: resolve each layer's signature
/// (recognition / validation) and lower to the empty process. It mints NOTHING in
/// the normalizer — DR-13: only the Rust supply producer writes `Σ⟦s⟧`, and
/// emitting `Σ⟦s⟧!(…)` sends would add COMM nodes and break the `Δ_s == consumed`
/// equality. A signed deploy's fuel is its own funded `Σ⟦c⟧` balance (Workstream
/// C/D), not a per-program send (design §3.3 + BLOCKER-1). In the multi-deploy
/// re-scoping (Phase 5) a `s :: ()` stack is the deploy BEING signed by `s`; it
/// carries no in-program mint.
fn recognize_token_stack_recursive<'ast>(
    stack: &'ast TokenStack<'ast>,
    input: ProcVisitInputs,
    env: &HashMap<String, Par>,
    parser: &'ast RholangParser<'ast>,
) -> Result<ProcVisitOutputs, InterpreterError> {
    for layer in stack.layers.iter() {
        // Resolve each layer (rejects malformed sigs); the resolved value drives
        // Phase-3 attribution, here it is validation only.
        signature_to_native_sig_recursive(layer, &input.bound_map_chain, env, parser)?;
    }
    Ok(ProcVisitOutputs {
        par: input.par.clone(),
        free_map: input.free_map.clone(),
    })
}

/// `for(... {% y <- x %}[s] ...)`: a JOIN carrying one or more per-clause SIGNED
/// binds (W1 Phase 4 / Axis-C). Recover the natural-arity plain join (Greg's rule:
/// fuel is NEVER folded into the data join — [`desugar::strip_signed_binds`]),
/// VALIDATE each clause signature, then lower the plain join through the ORDINARY
/// dispatch. Per-clause lane attribution is the reducer's channel match (Phase 3);
/// the continuation is NOT re-signed (one token per clause). Emits NO gate node, so
/// the normalized `Par` of a signed join is the SAME as its unsigned-equivalent
/// `for` (the double-metering avoidance, extended to joins).
fn recognize_signed_join_recursive<'ast>(
    receipts: &'ast Receipts<'ast>,
    body: AnnProc<'ast>,
    span: SourceSpan,
    input: ProcVisitInputs,
    env: &HashMap<String, Par>,
    parser: &'ast RholangParser<'ast>,
) -> Result<ProcVisitOutputs, InterpreterError> {
    // Strip every `Bind::Signed` back to its linear bind and collect the clause
    // signatures (source order). The recovered `for` is the natural-arity data
    // join — no fuel bind enters its `ReceiveBind` set.
    let (plain_for, clause_sigs) = desugar::strip_signed_binds(receipts, body, span, parser);
    debug_assert!(
        !clause_sigs.is_empty(),
        "recognize_signed_join_recursive is dispatched only when a Bind::Signed is present"
    );
    // VALIDATE each clause signature (rejects a wildcard `_`, a quoted-principal
    // `@P` ground sig, or a bare lollipop in fundable position) — the same
    // recognition `signature_to_native_sig_recursive` applies to a `{% P %}[s]` term. Phase
    // 3's channel match attributes each clause's rendezvous COMM to its signer lane
    // at the reducer; here recognition only validates + recovers the plain join.
    for sig in &clause_sigs {
        signature_to_native_sig_recursive(sig, &input.bound_map_chain, env, parser)?;
    }
    // Lower the recovered PLAIN join ordinarily: its binds are now linear, so it
    // re-normalizes through `normalize_p_input_recursive` with NO signed-bind recursion.
    normalize_ann_proc_recursive(&plain_for, input, env, parser)
}

// ==========================================================================
// VERBATIM from `normalizer/cost_accounting/sig.rs`, lines 44-96, at commit 6ccf71f2.
// ==========================================================================
fn signature_to_ir_recursive<'ast>(
    sig: &Signature<'ast>,
    bound_map_chain: &BoundMapChain<VarSort>,
    env: &HashMap<String, Par>,
    parser: &'ast RholangParser<'ast>,
) -> Result<Sig, InterpreterError> {
    match sig {
        Signature::Ground(name) => match name {
            // Binding-sensitive `Σ⟦g⟧`: a ground sig that resolves to a
            // `new`/`for`-binder is RING-FENCED (keyed on the binder's stable
            // identity); a FREE sig is content-by-spelling (one global channel).
            Name::NameVar(Var::Id(id)) => match bound_map_chain.get(id.name) {
                Some(ctx) => Ok(Sig::Bound(canon_bound(&ctx.source_span))),
                None => Ok(Sig::Ground(canon_ground(id.name))),
            },
            Name::NameVar(Var::Wildcard) => Err(InterpreterError::NormalizerError(
                "cost-accounting: a wildcard `_` is not a valid ground signature".to_string(),
            )),
            Name::Quote(_) => Err(InterpreterError::NormalizerError(
                "cost-accounting: a quoted-principal ground signature `@P` is not supported in v1 \
                 (use a section signature `# P` for code-hash principals)"
                    .to_string(),
            )),
        },
        Signature::Hash(proc) => Ok(Sig::Quote(canon_quote_recursive(proc, env, parser)?)),
        Signature::Compound(left, right) => {
            let left_ir = signature_to_ir_recursive(left, bound_map_chain, env, parser)?;
            let right_ir = signature_to_ir_recursive(right, bound_map_chain, env, parser)?;
            Ok(Sig::compound(vec![left_ir, right_ir]))
        }
        Signature::Transfer(_, _) => Err(InterpreterError::NormalizerError(
            "cost-accounting: a lollipop `-o` (transfer) signature is term-level sugar and must be \
             desugared before lowering; it cannot fund a term directly"
                .to_string(),
        )),
    }
}

/// Resolve a surface [`Signature`] straight to the native funding
/// [`accounting::Sig`](crate::rust::interpreter::accounting::Sig) — the
/// recognition + IR-bridge composite. Validates the sig (via
/// [`signature_to_ir_recursive`], which rejects a wildcard / `@P` ground / a bare lollipop
/// in fundable position) and bridges the IR to the consensus algebra via
/// [`Sig::to_native`]. The native `from_sig` of the result is the supply channel
/// `Σ⟦s⟧`; W1 NEVER derives a separate channel (design §3.1).
fn signature_to_native_sig_recursive<'ast>(
    sig: &Signature<'ast>,
    bound_map_chain: &BoundMapChain<VarSort>,
    env: &HashMap<String, Par>,
    parser: &'ast RholangParser<'ast>,
) -> Result<NativeSig, InterpreterError> {
    Ok(signature_to_ir_recursive(sig, bound_map_chain, env, parser)?.to_native())
}

// ==========================================================================
// VERBATIM from `normalizer/cost_accounting/sig.rs`, lines 146-155, at commit 6ccf71f2.
// ==========================================================================
fn canon_quote_recursive<'ast>(
    proc: &AnnProc<'ast>,
    env: &HashMap<String, Par>,
    parser: &'ast RholangParser<'ast>,
) -> Result<Vec<u8>, InterpreterError> {
    let normalized = normalize_ann_proc_recursive(proc, ProcVisitInputs::new(), env, parser)?;
    Ok(ParSortMatcher::sort_match(&normalized.par)
        .term
        .encode_to_vec())
}
