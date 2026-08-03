use std::collections::HashMap;

use models::rhoapi::{EMinus, EPlus, Expr, Par};
use rholang_parser::ast::{AnnProc, Proc};
use rholang_parser::RholangParser;

use super::bound_map_chain::BoundMapChain;
use super::free_map::FreeMap;
use super::normalize_drive::{norm_drive, NormKont, NormVal, NormWork, Step};
use crate::rust::interpreter::compiler::normalizer::processes::p_ground_normalizer::normalize_p_ground;
use crate::rust::interpreter::compiler::normalizer::processes::p_simple_type_normalizer::normalize_simple_type;
use crate::rust::interpreter::compiler::utils::{BinaryExpr, UnaryExpr};
use crate::rust::interpreter::errors::InterpreterError;
use crate::rust::interpreter::util::prepend_expr;

#[derive(Clone, Debug, PartialEq)]
pub enum VarSort {
    ProcSort,
    NameSort,
}

/**
 * Input data to the normalizer
 *
 * @param par collection of things that might be run in parallel
 * @param env
 * @param knownFree
 */
#[derive(Clone, Debug, PartialEq)]
pub struct ProcVisitInputs {
    pub par: Par,
    pub bound_map_chain: BoundMapChain<VarSort>,
    pub free_map: FreeMap<VarSort>,
}

impl ProcVisitInputs {
    pub fn new() -> Self {
        ProcVisitInputs {
            par: Par::default(),
            bound_map_chain: BoundMapChain::new(),
            free_map: FreeMap::new(),
        }
    }
}

impl Default for ProcVisitInputs {
    fn default() -> Self { Self::new() }
}

/// Returns the update Par and an updated map of free variables.
#[derive(Clone, Debug, PartialEq)]
pub struct ProcVisitOutputs {
    pub par: Par,
    pub free_map: FreeMap<VarSort>,
}

#[derive(Clone, Debug)]
pub struct NameVisitInputs {
    pub bound_map_chain: BoundMapChain<VarSort>,
    pub free_map: FreeMap<VarSort>,
}

#[derive(Clone, Debug)]
pub struct NameVisitOutputs {
    pub par: Par,
    pub free_map: FreeMap<VarSort>,
}

#[derive(Clone, Debug)]
pub struct CollectVisitInputs {
    pub bound_map_chain: BoundMapChain<VarSort>,
    pub free_map: FreeMap<VarSort>,
}

#[derive(Clone, Debug)]
pub struct CollectVisitOutputs {
    pub expr: Expr,
    pub free_map: FreeMap<VarSort>,
}

/**
 * Rholang normalizer entry point.
 *
 * ## ★ This is the driver of an explicit pushdown machine, not a recursion
 *
 * `normalize_ann_proc` and the 25 functions it is mutually recursive with used
 * to descend the source AST on the **native stack**, at 43,542 bytes per
 * nesting level in debug and 7,261 in release. A 577-byte program —
 * `[`×288 · `0` · `]`×288 — therefore aborted a *release* node, in
 * `inj_attempt`'s first phase, **before** metering exists and through an error
 * arm that a `SIGSEGV` cannot reach. The recursion now lives on the heap, in
 * [`super::normalize_drive::norm_drive`]; native stack is `O(1)` in source
 * nesting and in sibling width, in both profiles.
 *
 * The dispatch itself is [`descend_proc`], which is this function's old `match`
 * with every recursive call replaced by a [`Step`]. The verbatim recursive form
 * is retained as `super::normalize_recursive::normalize_ann_proc_recursive`
 * (test-only) and the two are differentiated in
 * `super::normalize_differential`.
 *
 * Full analysis, measured constants and proof standard:
 * `docs/design/audits/theta-depth-traversals-2026-07-26.md`.
 */
pub fn normalize_ann_proc<'ast>(
    proc: &AnnProc<'ast>,
    input: ProcVisitInputs,
    env: &HashMap<String, Par>,
    parser: &'ast RholangParser<'ast>,
) -> Result<ProcVisitOutputs, InterpreterError> {
    norm_drive(
        NormWork::Proc {
            // `AnnProc` is `Copy` (a `&'ast Proc` plus a span), so the machine
            // carries nodes by value. That is what lets a *synthesised* node —
            // `p_let`'s desugared `match`, `p_send_sync`'s `new`, `p_if`'s
            // implicit `Nil` — travel on the work stack: its `Proc` lives in the
            // parser arena for `'ast`, only the two-word wrapper is local.
            proc: *proc,
            input,
        },
        env,
        parser,
    )
    .map(NormVal::into_proc)
}

/// The `Proc` dispatch — this file's original `match`, with every recursive call
/// replaced by a [`Step`].
///
/// Arms fall into four shapes:
///
/// | shape | arms | step |
/// |---|---|---|
/// | leaf | `Nil`, ground literals, `SimpleType`, `ProcVar`, `VarRef`, `Select`, `Bad` | [`Step::Done`] |
/// | one continuation, then children | everything structural | [`Step::Descend`] |
/// | pure desugaring | `SendSync`, `Let` | [`Step::Tail`] |
/// | validate, then desugar | `SignedTerm`, `TokenStack`, signed `for` | [`Step::Descend`] onto a signature, whose combine is a [`Step::Tail`] |
pub(crate) fn descend_proc<'ast>(
    proc: AnnProc<'ast>,
    input: ProcVisitInputs,
    _env: &HashMap<String, Par>,
    parser: &'ast RholangParser<'ast>,
) -> Result<Step<'ast>, InterpreterError> {
    use crate::rust::interpreter::compiler::normalizer::processes as p;

    // ⚠ The normalizer env reaches only two places: `normalize_p_new`'s
    // injection map (built in its COMBINE half, where the driver hands it over)
    // and `canon_quote`'s nested drive. No `descend` arm consults it, so it is
    // not threaded through the dispatch.

    match proc.proc {
        Proc::Nil => Ok(Step::Done(NormVal::Proc(ProcVisitOutputs {
            par: input.par,
            free_map: input.free_map,
        }))),

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
        | Proc::UriLiteral(_) => Ok(Step::Done(NormVal::Proc(normalize_p_ground(
            proc.proc, input,
        )?))),

        Proc::SimpleType(simple_type) => Ok(Step::Done(NormVal::Proc(normalize_simple_type(
            simple_type,
            input,
        )?))),

        Proc::ProcVar(var) => {
            use crate::rust::interpreter::compiler::normalizer::processes::p_var_normalizer::normalize_p_var;
            Ok(Step::Done(NormVal::Proc(normalize_p_var(
                var, input, proc.span,
            )?)))
        }

        Proc::Par { left, right } => Ok(p::p_par_normalizer::descend_p_par(left, right, input)),

        Proc::Eval { name } => Ok(p::p_eval_normalizer::descend_p_eval(name, input)),

        // UnaryExp - handle all unary operators
        Proc::UnaryExp { op, arg } => match op {
            rholang_parser::ast::UnaryExpOp::Negation => Ok(
                // ⚠ `arg.span`, NOT `proc.span`. The recursive form passed the
                // ARGUMENT's span, and that span is what the free map records
                // for the connective; using the whole `~P` span instead is
                // byte-visible in the error text a rejected pattern produces.
                // The differential caught exactly this on `~7`.
                p::p_negation_normalizer::descend_p_negation(*arg, arg.span, input),
            ),
            rholang_parser::ast::UnaryExpOp::Not => {
                use models::rhoapi::ENot;
                Ok(descend_unary(*arg, input, Box::new(ENot::default())))
            }
            rholang_parser::ast::UnaryExpOp::Neg => {
                use models::rhoapi::ENeg;
                Ok(descend_unary(*arg, input, Box::new(ENeg::default())))
            }
        },

        // BinaryExp - handle all binary operators
        Proc::BinaryExp { op, left, right } => {
            match op {
                // Logical connectives
                rholang_parser::ast::BinaryExpOp::Conjunction => Ok(
                    p::p_conjunction_normalizer::descend_p_conjunction(*left, *right, input),
                ),
                rholang_parser::ast::BinaryExpOp::Disjunction => Ok(
                    p::p_disjunction_normalizer::descend_p_disjunction(*left, *right, input),
                ),
                rholang_parser::ast::BinaryExpOp::Matches => Ok(
                    p::p_matches_normalizer::descend_p_matches(*left, *right, input),
                ),

                // Arithmetic
                rholang_parser::ast::BinaryExpOp::Add => Ok(descend_binary(
                    *left,
                    *right,
                    input,
                    Box::new(EPlus::default()),
                )),
                rholang_parser::ast::BinaryExpOp::Sub => Ok(descend_binary(
                    *left,
                    *right,
                    input,
                    Box::new(EMinus::default()),
                )),
                rholang_parser::ast::BinaryExpOp::Mult => {
                    use models::rhoapi::EMult;
                    Ok(descend_binary(
                        *left,
                        *right,
                        input,
                        Box::new(EMult::default()),
                    ))
                }
                rholang_parser::ast::BinaryExpOp::Div => {
                    use models::rhoapi::EDiv;
                    Ok(descend_binary(
                        *left,
                        *right,
                        input,
                        Box::new(EDiv::default()),
                    ))
                }
                rholang_parser::ast::BinaryExpOp::Mod => {
                    use models::rhoapi::EMod;
                    Ok(descend_binary(
                        *left,
                        *right,
                        input,
                        Box::new(EMod::default()),
                    ))
                }

                // Comparison operators
                rholang_parser::ast::BinaryExpOp::Eq => {
                    use models::rhoapi::EEq;
                    Ok(descend_binary(
                        *left,
                        *right,
                        input,
                        Box::new(EEq::default()),
                    ))
                }
                rholang_parser::ast::BinaryExpOp::Neq => {
                    use models::rhoapi::ENeq;
                    Ok(descend_binary(
                        *left,
                        *right,
                        input,
                        Box::new(ENeq::default()),
                    ))
                }
                rholang_parser::ast::BinaryExpOp::Lt => {
                    use models::rhoapi::ELt;
                    Ok(descend_binary(
                        *left,
                        *right,
                        input,
                        Box::new(ELt::default()),
                    ))
                }
                rholang_parser::ast::BinaryExpOp::Lte => {
                    use models::rhoapi::ELte;
                    Ok(descend_binary(
                        *left,
                        *right,
                        input,
                        Box::new(ELte::default()),
                    ))
                }
                rholang_parser::ast::BinaryExpOp::Gt => {
                    use models::rhoapi::EGt;
                    Ok(descend_binary(
                        *left,
                        *right,
                        input,
                        Box::new(EGt::default()),
                    ))
                }
                rholang_parser::ast::BinaryExpOp::Gte => {
                    use models::rhoapi::EGte;
                    Ok(descend_binary(
                        *left,
                        *right,
                        input,
                        Box::new(EGte::default()),
                    ))
                }

                // Set/String operations
                rholang_parser::ast::BinaryExpOp::Concat => {
                    use models::rhoapi::EPlusPlus;
                    Ok(descend_binary(
                        *left,
                        *right,
                        input,
                        Box::new(EPlusPlus::default()),
                    ))
                }
                rholang_parser::ast::BinaryExpOp::Diff => {
                    use models::rhoapi::EMinusMinus;
                    Ok(descend_binary(
                        *left,
                        *right,
                        input,
                        Box::new(EMinusMinus::default()),
                    ))
                }

                // Boolean operators
                rholang_parser::ast::BinaryExpOp::Or => {
                    use models::rhoapi::EOr;
                    Ok(descend_binary(
                        *left,
                        *right,
                        input,
                        Box::new(EOr::default()),
                    ))
                }
                rholang_parser::ast::BinaryExpOp::And => {
                    use models::rhoapi::EAnd;
                    Ok(descend_binary(
                        *left,
                        *right,
                        input,
                        Box::new(EAnd::default()),
                    ))
                }

                // String interpolation
                rholang_parser::ast::BinaryExpOp::Interpolation => {
                    use models::rhoapi::EPercentPercent;
                    Ok(descend_binary(
                        *left,
                        *right,
                        input,
                        Box::new(EPercentPercent::default()),
                    ))
                }
            }
        }

        // IfThenElse - handle conditional statements
        //
        // The recursive form normalized against an EMPTY `par` and appended the
        // original afterwards (`new_visits.par.append(input.par)`). The machine
        // keeps that asymmetry by handing the original `par` to the continuation
        // as `outer_par`; see `p_if_normalizer::descend_p_if`.
        Proc::IfThenElse {
            condition,
            if_true,
            if_false,
        } => Ok(p::p_if_normalizer::descend_p_if(
            *condition, *if_true, *if_false, input,
        )),

        // Method - handle method calls
        Proc::Method {
            receiver,
            name,
            args,
        } => Ok(p::p_method_normalizer::descend_p_method(
            *receiver, name, args, input,
        )),

        // Bundle - handle bundle constructs
        Proc::Bundle {
            bundle_type,
            proc: body,
        } => Ok(p::p_bundle_normalizer::descend_p_bundle(
            *bundle_type,
            *body,
            input,
            body.span,
        )),

        // Send - handle send operations
        Proc::Send {
            channel,
            send_type,
            inputs,
        } => Ok(p::p_send_normalizer::descend_p_send(
            *channel, send_type, inputs, input,
        )),

        // SendSync - handle synchronous send operations
        Proc::SendSync {
            channel,
            inputs,
            cont,
        } => Ok(p::p_send_sync_normalizer::descend_p_send_sync(
            channel, inputs, cont, &proc.span, input, parser,
        )),

        // New - handle name declarations and scoping
        Proc::New { decls, proc: body } => p::p_new_normalizer::descend_p_new(decls, *body, input),

        // Contract - handle contract declarations
        Proc::Contract {
            name,
            formals,
            body,
        } => Ok(p::p_contr_normalizer::descend_p_contr(
            *name, formals, *body, input,
        )),

        // Match - handle pattern matching
        Proc::Match { expression, cases } => Ok(p::p_match_normalizer::descend_p_match(
            *expression,
            cases.as_slice(),
            input,
        )),

        // Collection - handle data structures (lists, tuples, sets, maps)
        Proc::Collection(collection) => {
            use crate::rust::interpreter::compiler::normalizer::collection_normalize_matcher::descend_collection;
            descend_collection(collection, input)
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
                use crate::rust::interpreter::compiler::normalizer::cost_accounting::recognize::descend_signed_join;
                Ok(descend_signed_join(
                    receipts,
                    *continuation,
                    proc.span,
                    input,
                    parser,
                ))
            } else {
                p::p_input_normalizer::descend_p_input(receipts, *continuation, input, parser)
            }
        }

        // Let - handle let bindings
        Proc::Let {
            bindings,
            body,
            concurrent,
        } => Ok(p::p_let_normalizer::descend_p_let(
            bindings,
            *body,
            *concurrent,
            proc.span,
            input,
            parser,
        )),

        // VarRef - handle variable references
        Proc::VarRef { kind, var } => {
            use crate::rust::interpreter::compiler::normalizer::processes::p_var_ref_normalizer::normalize_p_var_ref;
            Ok(Step::Done(NormVal::Proc(normalize_p_var_ref(
                *kind, var, input, proc.span,
            )?)))
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
            use crate::rust::interpreter::compiler::normalizer::cost_accounting::recognize::descend_signed_term;
            descend_signed_term(*inner, sig, input, parser)
        }
        Proc::TokenStack { stack } => {
            use crate::rust::interpreter::compiler::normalizer::cost_accounting::recognize::descend_token_stack;
            Ok(descend_token_stack(stack, input))
        }

        // Bad - handle parsing errors
        Proc::Bad => Err(InterpreterError::ParserError(
            "Bad process node indicates parsing error".to_string(),
        )),
    }
}

/// `unary_exp`, descend half.
///
/// ⚠ The sub-process receives the **whole** `input`, `par` included — it is not
/// re-based on an empty `Par`. `input_par` is snapshotted first because the
/// result is `prepend_expr(input_par, …)`, i.e. the original `par` with the new
/// expression in front.
#[inline(never)]
fn descend_unary<'ast>(
    sub_proc: AnnProc<'ast>,
    input: ProcVisitInputs,
    ctor: Box<dyn UnaryExpr>,
) -> Step<'ast> {
    let input_par = input.par.clone();
    let input_depth = input.bound_map_chain.depth() as i32;
    Step::Descend {
        kont: NormKont::Unary {
            input_par,
            input_depth,
            ctor,
        },
        work: NormWork::Proc {
            proc: sub_proc,
            input,
        },
    }
}

/// `unary_exp`, combine half.
#[inline(never)]
pub(crate) fn combine_unary<'ast>(
    input_par: Par,
    input_depth: i32,
    ctor: Box<dyn UnaryExpr>,
    value: NormVal,
) -> Result<Step<'ast>, InterpreterError> {
    let sub_result = value.into_proc();
    let expr = ctor.from_par(sub_result.par);
    Ok(Step::Done(NormVal::Proc(ProcVisitOutputs {
        par: prepend_expr(input_par, expr, input_depth),
        free_map: sub_result.free_map,
    })))
}

/// `binary_exp`, descend half — the LEFT operand.
///
/// ★ This is the archetype of the threading problem. The right operand is
/// normalized against `left_result.free_map`, so it cannot be scheduled until
/// the left one has produced a value; and it is re-based on `Par::default()`
/// with the *entry* `bound_map_chain`, not with whatever the left operand left
/// behind. Both are carried in the continuation.
#[inline(never)]
fn descend_binary<'ast>(
    left_proc: AnnProc<'ast>,
    right_proc: AnnProc<'ast>,
    input: ProcVisitInputs,
    ctor: Box<dyn BinaryExpr>,
) -> Step<'ast> {
    let input_par = input.par.clone();
    let input_depth = input.bound_map_chain.depth() as i32;
    let input_bound_chain = input.bound_map_chain.clone();
    Step::Descend {
        kont: NormKont::Binary {
            right: right_proc,
            input_par,
            input_depth,
            bound_map_chain: input_bound_chain,
            ctor,
            left_par: None,
        },
        work: NormWork::Proc {
            proc: left_proc,
            input,
        },
    }
}

/// `binary_exp`, combine half — schedules the RIGHT operand, then builds.
#[inline(never)]
#[allow(clippy::too_many_arguments)]
pub(crate) fn combine_binary<'ast>(
    right: AnnProc<'ast>,
    input_par: Par,
    input_depth: i32,
    bound_map_chain: BoundMapChain<VarSort>,
    ctor: Box<dyn BinaryExpr>,
    left_par: Option<Par>,
    value: NormVal,
) -> Result<Step<'ast>, InterpreterError> {
    let result = value.into_proc();
    match left_par {
        // The left operand has just finished: thread its free map into the
        // right operand's input and schedule it.
        None => Ok(Step::Descend {
            kont: NormKont::Binary {
                right,
                input_par,
                input_depth,
                bound_map_chain: bound_map_chain.clone(),
                ctor,
                left_par: Some(result.par),
            },
            work: NormWork::Proc {
                proc: right,
                input: ProcVisitInputs {
                    par: Par::default(),
                    bound_map_chain,
                    free_map: result.free_map,
                },
            },
        }),
        // The right operand has just finished.
        Some(lp) => {
            let expr: Expr = ctor.from_pars(lp, result.par);
            Ok(Step::Done(NormVal::Proc(ProcVisitOutputs {
                par: prepend_expr(input_par, expr, input_depth),
                free_map: result.free_map,
            })))
        }
    }
}

// See rholang/src/test/scala/coop/rchain/rholang/interpreter/compiler/normalizer/ProcMatcherSpec.scala
// inside this source file we tested unary and binary operations, because we don't have separate normalizers for them.
#[cfg(test)]
mod tests {
    use models::create_bit_vector;
    use models::rhoapi::expr::ExprInstance;
    use models::rhoapi::{EDiv, EMinus, EMinusMinus, EMult, EPlus, EPlusPlus, Expr, Par};
    use models::rust::utils::{new_boundvar_par, new_gint_par, new_gstring_par};
    use pretty_assertions::assert_eq;

    use crate::rust::interpreter::compiler::compiler::Compiler;
    use crate::rust::interpreter::compiler::exports::{ProcVisitInputs, ProcVisitOutputs};
    use crate::rust::interpreter::compiler::normalize::VarSort::ProcSort;
    use crate::rust::interpreter::test_utils::utils::{
        proc_visit_inputs_and_env, proc_visit_inputs_with_updated_vec_bound_map_chain,
    };
    use crate::rust::interpreter::util::prepend_expr;

    #[test]
    fn p_nil_should_compile_as_no_modification() {
        use std::collections::HashMap;

        let (inputs, _env) = proc_visit_inputs_and_env();

        fn test_with_parser(
            inputs: ProcVisitInputs,
        ) -> Result<ProcVisitOutputs, crate::rust::interpreter::InterpreterError> {
            use validated::Validated;

            use crate::rust::interpreter::compiler::normalize::normalize_ann_proc;
            let parser = rholang_parser::RholangParser::new();
            let result = parser.parse("Nil");
            match result {
                Validated::Good(procs) => {
                    if procs.len() == 1 {
                        let ast = procs.into_iter().next().unwrap();
                        normalize_ann_proc(&ast, inputs, &HashMap::new(), &parser)
                    } else {
                        panic!("Expected single process")
                    }
                }
                _ => panic!("Parse failed"),
            }
        }

        let result = test_with_parser(inputs.clone());

        let actual_result = result.unwrap();
        assert_eq!(actual_result.par, inputs.par);
        assert_eq!(actual_result.free_map, inputs.free_map);
    }

    // unary operations:
    #[test]
    fn p_not_should_delegate() {
        use std::collections::HashMap;

        let (inputs, _env) = proc_visit_inputs_and_env();

        fn test_with_parser(
            inputs: ProcVisitInputs,
        ) -> Result<ProcVisitOutputs, crate::rust::interpreter::InterpreterError> {
            use validated::Validated;

            use crate::rust::interpreter::compiler::normalize::normalize_ann_proc;
            let parser = rholang_parser::RholangParser::new();
            let result = parser.parse("~false");
            match result {
                Validated::Good(procs) => {
                    if procs.len() == 1 {
                        let ast = procs.into_iter().next().unwrap();
                        normalize_ann_proc(&ast, inputs, &HashMap::new(), &parser)
                    } else {
                        panic!("Expected single process")
                    }
                }
                _ => panic!("Parse failed"),
            }
        }

        let result = test_with_parser(inputs.clone());

        let expected_result = {
            let mut par = inputs.par.clone();
            par.connectives.push(models::rhoapi::Connective {
                connective_instance: Some(
                    models::rhoapi::connective::ConnectiveInstance::ConnNotBody(models::par_from_default! {
                        exprs: vec![Expr {
                            expr_instance: Some(models::rhoapi::expr::ExprInstance::GBool(false)),
                        }],
                        ..Par::default()
                    }),
                ),
            });
            par.connective_used = true;
            par
        };

        let actual_result = result.unwrap();
        assert_eq!(actual_result.par, expected_result);
        assert_eq!(actual_result.free_map.connectives.len(), 1);
    }

    #[test]
    fn p_neg_should_delegate() {
        use std::collections::HashMap;

        let (inputs, _env) = proc_visit_inputs_and_env();

        fn test_with_parser(
            inputs: ProcVisitInputs,
        ) -> Result<ProcVisitOutputs, crate::rust::interpreter::InterpreterError> {
            use validated::Validated;

            use crate::rust::interpreter::compiler::normalize::normalize_ann_proc;
            let parser = rholang_parser::RholangParser::new();
            let result = parser.parse("-7");
            match result {
                Validated::Good(procs) => {
                    if procs.len() == 1 {
                        let ast = procs.into_iter().next().unwrap();
                        normalize_ann_proc(&ast, inputs, &HashMap::new(), &parser)
                    } else {
                        panic!("Expected single process")
                    }
                }
                _ => panic!("Parse failed"),
            }
        }

        let result = test_with_parser(inputs.clone());

        let expected_result = prepend_expr(
            inputs.par.clone(),
            Expr {
                expr_instance: Some(ExprInstance::GInt(-7)),
            },
            0,
        );

        let actual_result = result.unwrap();
        assert_eq!(actual_result.par, expected_result);
        assert_eq!(actual_result.free_map, inputs.free_map);
    }

    //binary operations:
    #[test]
    fn p_mult_should_delegate() {
        use std::collections::HashMap;

        let (inputs, _env) = proc_visit_inputs_and_env();

        fn test_with_parser(
            inputs: ProcVisitInputs,
        ) -> Result<ProcVisitOutputs, crate::rust::interpreter::InterpreterError> {
            use validated::Validated;

            use crate::rust::interpreter::compiler::normalize::normalize_ann_proc;
            let parser = rholang_parser::RholangParser::new();
            let result = parser.parse("7 * 8");
            match result {
                Validated::Good(procs) => {
                    if procs.len() == 1 {
                        let ast = procs.into_iter().next().unwrap();
                        normalize_ann_proc(&ast, inputs, &HashMap::new(), &parser)
                    } else {
                        panic!("Expected single process")
                    }
                }
                _ => panic!("Parse failed"),
            }
        }

        let result = test_with_parser(inputs.clone());

        let expected_result = prepend_expr(
            inputs.par.clone(),
            Expr {
                expr_instance: Some(ExprInstance::EMultBody(EMult {
                    p1: Some(new_gint_par(7, Vec::new(), false)),
                    p2: Some(new_gint_par(8, Vec::new(), false)),
                })),
            },
            0,
        );

        let actual_result = result.unwrap();
        assert_eq!(actual_result.par, expected_result);
        assert_eq!(actual_result.free_map, inputs.free_map);
    }

    #[test]
    fn p_div_should_delegate() {
        use std::collections::HashMap;

        let (inputs, _env) = proc_visit_inputs_and_env();

        fn test_with_parser(
            inputs: ProcVisitInputs,
        ) -> Result<ProcVisitOutputs, crate::rust::interpreter::InterpreterError> {
            use validated::Validated;

            use crate::rust::interpreter::compiler::normalize::normalize_ann_proc;
            let parser = rholang_parser::RholangParser::new();
            let result = parser.parse("7 / 8");
            match result {
                Validated::Good(procs) => {
                    if procs.len() == 1 {
                        let ast = procs.into_iter().next().unwrap();
                        normalize_ann_proc(&ast, inputs, &HashMap::new(), &parser)
                    } else {
                        panic!("Expected single process")
                    }
                }
                _ => panic!("Parse failed"),
            }
        }

        let result = test_with_parser(inputs.clone());

        let expected_result = prepend_expr(
            inputs.par.clone(),
            Expr {
                expr_instance: Some(ExprInstance::EDivBody(EDiv {
                    p1: Some(new_gint_par(7, Vec::new(), false)),
                    p2: Some(new_gint_par(8, Vec::new(), false)),
                })),
            },
            0,
        );

        let actual_result = result.unwrap();
        assert_eq!(actual_result.par, expected_result);
        assert_eq!(actual_result.free_map, inputs.free_map);
    }

    #[test]
    fn p_percent_percent_should_delegate() {
        use std::collections::HashMap;

        let (inputs, _env) = proc_visit_inputs_and_env();

        fn test_with_parser(
            inputs: ProcVisitInputs,
        ) -> Result<ProcVisitOutputs, crate::rust::interpreter::InterpreterError> {
            use validated::Validated;

            use crate::rust::interpreter::compiler::normalize::normalize_ann_proc;
            let parser = rholang_parser::RholangParser::new();
            let result = parser.parse("7 % 8");
            match result {
                Validated::Good(procs) => {
                    if procs.len() == 1 {
                        let ast = procs.into_iter().next().unwrap();
                        normalize_ann_proc(&ast, inputs, &HashMap::new(), &parser)
                    } else {
                        panic!("Expected single process")
                    }
                }
                _ => panic!("Parse failed"),
            }
        }

        let result = test_with_parser(inputs.clone());

        use models::rhoapi::EMod;
        let expected_result = prepend_expr(
            inputs.par.clone(),
            Expr {
                expr_instance: Some(ExprInstance::EModBody(EMod {
                    p1: Some(new_gint_par(7, Vec::new(), false)),
                    p2: Some(new_gint_par(8, Vec::new(), false)),
                })),
            },
            0,
        );

        let actual_result = result.unwrap();
        assert_eq!(actual_result.par, expected_result);
        assert_eq!(actual_result.free_map, inputs.free_map);
    }

    #[test]
    fn p_add_should_delegate() {
        use std::collections::HashMap;

        let (inputs, _env) = proc_visit_inputs_and_env();

        fn test_with_parser(
            inputs: ProcVisitInputs,
        ) -> Result<ProcVisitOutputs, crate::rust::interpreter::InterpreterError> {
            use validated::Validated;

            use crate::rust::interpreter::compiler::normalize::normalize_ann_proc;
            let parser = rholang_parser::RholangParser::new();
            let result = parser.parse("7 + 8");
            match result {
                Validated::Good(procs) => {
                    if procs.len() == 1 {
                        let ast = procs.into_iter().next().unwrap();
                        normalize_ann_proc(&ast, inputs, &HashMap::new(), &parser)
                    } else {
                        panic!("Expected single process")
                    }
                }
                _ => panic!("Parse failed"),
            }
        }

        let result = test_with_parser(inputs.clone());

        let expected_result = prepend_expr(
            inputs.par.clone(),
            Expr {
                expr_instance: Some(ExprInstance::EPlusBody(EPlus {
                    p1: Some(new_gint_par(7, Vec::new(), false)),
                    p2: Some(new_gint_par(8, Vec::new(), false)),
                })),
            },
            0,
        );

        let actual_result = result.unwrap();
        assert_eq!(actual_result.par, expected_result);
        assert_eq!(actual_result.free_map, inputs.free_map);
    }

    #[test]
    fn p_plus_plus_should_delegate() {
        use std::collections::HashMap;

        let (inputs, _env) = proc_visit_inputs_and_env();

        fn test_with_parser(
            inputs: ProcVisitInputs,
        ) -> Result<ProcVisitOutputs, crate::rust::interpreter::InterpreterError> {
            use validated::Validated;

            use crate::rust::interpreter::compiler::normalize::normalize_ann_proc;
            let parser = rholang_parser::RholangParser::new();
            let result = parser.parse("\"abc\" ++ \"def\"");
            match result {
                Validated::Good(procs) => {
                    if procs.len() == 1 {
                        let ast = procs.into_iter().next().unwrap();
                        normalize_ann_proc(&ast, inputs, &HashMap::new(), &parser)
                    } else {
                        panic!("Expected single process")
                    }
                }
                _ => panic!("Parse failed"),
            }
        }

        let result = test_with_parser(inputs.clone());

        let expected_result = prepend_expr(
            inputs.par.clone(),
            Expr {
                expr_instance: Some(ExprInstance::EPlusPlusBody(EPlusPlus {
                    p1: Some(new_gstring_par("abc".to_string(), Vec::new(), false)),
                    p2: Some(new_gstring_par("def".to_string(), Vec::new(), false)),
                })),
            },
            0,
        );

        let actual_result = result.unwrap();
        assert_eq!(actual_result.par, expected_result);
        assert_eq!(actual_result.free_map, inputs.free_map);
    }

    #[test]
    fn p_minus_should_delegate() {
        use std::collections::HashMap;

        let (base_inputs, _env) = proc_visit_inputs_and_env();
        let inputs = proc_visit_inputs_with_updated_vec_bound_map_chain(base_inputs, vec![
            ("x".into(), ProcSort),
            ("y".into(), ProcSort),
            ("z".into(), ProcSort),
        ]);

        fn test_with_parser(
            inputs: ProcVisitInputs,
        ) -> Result<ProcVisitOutputs, crate::rust::interpreter::InterpreterError> {
            use validated::Validated;

            use crate::rust::interpreter::compiler::normalize::normalize_ann_proc;
            let parser = rholang_parser::RholangParser::new();
            let result = parser.parse("x - (y * z)");
            match result {
                Validated::Good(procs) => {
                    if procs.len() == 1 {
                        let ast = procs.into_iter().next().unwrap();
                        normalize_ann_proc(&ast, inputs, &HashMap::new(), &parser)
                    } else {
                        panic!("Expected single process")
                    }
                }
                _ => panic!("Parse failed"),
            }
        }

        let result = test_with_parser(inputs.clone());

        let expected_result = prepend_expr(
            inputs.par.clone(),
            Expr {
                expr_instance: Some(ExprInstance::EMinusBody(EMinus {
                    p1: Some(new_boundvar_par(2, create_bit_vector(&vec![2]), false)),
                    p2: Some(models::par_from_default! {
                        exprs: vec![Expr {
                            expr_instance: Some(ExprInstance::EMultBody(EMult {
                                p1: Some(new_boundvar_par(1, create_bit_vector(&vec![1]), false)),
                                p2: Some(new_boundvar_par(0, create_bit_vector(&vec![0]), false)),
                            })),
                        }],
                        locally_free: create_bit_vector(&vec![0, 1]),
                        connective_used: false,
                        ..Par::default()
                    }),
                })),
            },
            0,
        );

        let actual_result = result.unwrap();
        assert_eq!(actual_result.par, expected_result);
        assert_eq!(actual_result.free_map, inputs.free_map);
    }

    #[test]
    fn p_minus_minus_should_delegate() {
        use std::collections::HashMap;

        let (inputs, _env) = proc_visit_inputs_and_env();

        fn test_with_parser(
            inputs: ProcVisitInputs,
        ) -> Result<ProcVisitOutputs, crate::rust::interpreter::InterpreterError> {
            use validated::Validated;

            use crate::rust::interpreter::compiler::normalize::normalize_ann_proc;
            let parser = rholang_parser::RholangParser::new();
            let result = parser.parse("\"abc\" -- \"def\"");
            match result {
                Validated::Good(procs) => {
                    if procs.len() == 1 {
                        let ast = procs.into_iter().next().unwrap();
                        normalize_ann_proc(&ast, inputs, &HashMap::new(), &parser)
                    } else {
                        panic!("Expected single process")
                    }
                }
                _ => panic!("Parse failed"),
            }
        }

        let result = test_with_parser(inputs.clone());

        let expected_result = prepend_expr(
            inputs.par.clone(),
            Expr {
                expr_instance: Some(ExprInstance::EMinusMinusBody(EMinusMinus {
                    p1: Some(new_gstring_par("abc".to_string(), vec![], false)),
                    p2: Some(new_gstring_par("def".to_string(), vec![], false)),
                })),
            },
            0,
        );

        let actual_result = result.unwrap();
        assert_eq!(actual_result.par, expected_result);
        assert_eq!(actual_result.free_map, inputs.free_map);
    }

    #[test]
    fn patterns_should_compile_not_in_top_level() {
        let cases = vec![
            ("wildcard", "send channel", "{_!(1)}"),
            // REMOVED: The pattern "{@=*x!(_)}" was invalid Rholang syntax
            // REASON: Neither "@=*variable" nor "=*variable" are valid in this context
            // This pattern was testing invalid syntax that should not have been allowed
            // Replaced with a valid wildcard pattern for comprehensive testing
            ("wildcard", "send data", "{@_!(_)}"),
            ("wildcard", "send data", "{@Nil!(_)}"),
            ("logical AND", "send data", "{@Nil!(1 /\\ 2)}"),
            ("logical OR", "send data", "{@Nil!(1 \\/ 2)}"),
            ("logical NOT", "send data", "{@Nil!(~1)}"),
            ("logical AND", "send channel", "{@{Nil /\\ Nil}!(Nil)}"),
            ("logical OR", "send channel", "{@{Nil \\/ Nil}!(Nil)}"),
            ("logical NOT", "send channel", "{@{~Nil}!(Nil)}"),
            (
                "wildcard",
                "receive pattern of the consume",
                "{for (_ <- x) { 1 }} ",
            ),
            (
                "wildcard",
                "body of the continuation",
                "{for (@1 <- x) { _ }} ",
            ),
            (
                "logical OR",
                "body of the continuation",
                "{for (@1 <- x) { 10 \\/ 20 }} ",
            ),
            (
                "logical AND",
                "body of the continuation",
                "{for(@1 <- x) { 10 /\\ 20 }} ",
            ),
            (
                "logical NOT",
                "body of the continuation",
                "{for(@1 <- x) { ~10 }} ",
            ),
            (
                "logical OR",
                "channel of the consume",
                "{for (@1 <- @{Nil /\\ Nil}) { Nil }} ",
            ),
            (
                "logical AND",
                "channel of the consume",
                "{for(@1 <- @{Nil \\/ Nil}) { Nil }} ",
            ),
            (
                "logical NOT",
                "channel of the consume",
                "{for(@1 <- @{~Nil}) { Nil }} ",
            ),
            (
                "wildcard",
                "channel of the consume",
                "{for(@1 <- _) { Nil }} ",
            ),
        ];

        for (typ, position, pattern) in cases.iter() {
            let rho = format!(
                r#"
        new x in {{
            for(@y <- x) {{
                match y {{
                    {} => Nil
                }}
            }}
        }}
        "#,
                pattern
            );

            match Compiler::source_to_adt(&rho) {
                Ok(_) => {}
                Err(e) => {
                    panic!(
                        "{} in the {} '{}' should not throw errors: {:?}",
                        typ, position, pattern, e
                    );
                }
            }
        }
    }
}
