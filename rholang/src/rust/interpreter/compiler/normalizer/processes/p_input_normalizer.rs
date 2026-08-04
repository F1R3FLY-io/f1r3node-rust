// See rholang/src/main/scala/coop/rchain/rholang/interpreter/compiler/normalizer/processes/PInputNormalizer.scala

use std::collections::{HashMap, HashSet};

use models::rhoapi::{Par, Receive, ReceiveBind};
use models::rust::rholang::par_children::dismantle_all;
use models::rust::utils::union;
use rholang_parser::ast::{AnnProc, Bind, Name, Proc, Receipts, Source, Var};
use rholang_parser::{SourcePos, SourceSpan};
use uuid::Uuid;

use crate::rust::interpreter::compiler::exports::{
    FreeMap, NameVisitInputs, ProcVisitInputs, ProcVisitOutputs,
};
use crate::rust::interpreter::compiler::normalize::VarSort;
use crate::rust::interpreter::compiler::normalize_drive::{
    InputK, InputPhase, NormKont, NormVal, NormWork, Step,
};
use crate::rust::interpreter::compiler::normalizer::cost_accounting::pattern_guard::reject_cost_syntax_in_name_pattern;
use crate::rust::interpreter::compiler::normalizer::processes::utils::fail_on_invalid_connective;
use crate::rust::interpreter::compiler::normalizer::remainder_normalizer_matcher::normalize_match_name;
use crate::rust::interpreter::compiler::receive_binds_sort_matcher::pre_sort_binds;
use crate::rust::interpreter::compiler::span_utils::SpanContext;
use crate::rust::interpreter::errors::InterpreterError;
use crate::rust::interpreter::guard::{reject_undecidable_guard, RECEIVE_WHERE};
use crate::rust::interpreter::unwrap_option_safe;
use crate::rust::interpreter::util::filter_and_adjust_bitset;

/// `for (…) { P }`, descend half.
///
/// Three branches, exactly as before:
///
/// 1. **more than one `;`-separated receipt** → desugar to nested `for`s and
///    [`Step::Tail`];
/// 2. **a complex source** (`x <- c?()`, `x <- c!(…)`) → desugar to a `new` +
///    `|` + plain `for` and [`Step::Tail`];
/// 3. **the simple case** → the machine's longest child sequence:
///
/// ```text
///    every formal of every bind ──▶ every channel ──▶ [ where-guard ] ──▶ body
///    ├─ per GROUP: a FRESH free map,     ├─ threads the ENCLOSING free map
///    │  chain.push(), remainder absorbed │
///    └─ groups are independent           └─ channels are ordered
///
///    then, once every channel is in:  pre_sort_binds · duplicate-channel
///    check · merge the per-bind free maps · absorb into `bound_map_chain`
///    ⇒ body_env, which the guard AND the body share
/// ```
#[inline(never)]
pub(crate) fn descend_p_input<'ast>(
    receipts: &'ast Receipts<'ast>,
    body: AnnProc<'ast>,
    input: ProcVisitInputs,
    parser: &'ast rholang_parser::RholangParser<'ast>,
) -> Result<Step<'ast>, InterpreterError> {
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
            .fold(body, |acc_body, receipt_group| AnnProc {
                proc: parser.ast_builder().alloc_for_with_guards(
                    vec![(receipt_group.binds.to_vec(), receipt_group.guard)],
                    acc_body,
                ),
                span: body.span,
            });
        return Ok(Step::Tail(NormWork::Proc {
            proc: desugared,
            input,
        }));
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
                    body,
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

        // Hand the desugared process back to the driver (a tail call).
        Ok(Step::Tail(NormWork::Proc {
            proc: final_proc,
            input,
        }))
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
                    let names: &'ast [Name<'ast>] = lhs.names.as_slice();
                    let remainder = lhs.remainder;

                    let source_name = match rhs {
                        Source::Simple { name } => *name,
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
                    let names: &'ast [Name<'ast>] = lhs.names.as_slice();
                    let remainder = lhs.remainder;
                    Ok(((names, remainder), *rhs))
                }
                Bind::Peek { lhs, rhs } => {
                    let names: &'ast [Name<'ast>] = lhs.names.as_slice();
                    let remainder = lhs.remainder;
                    Ok(((names, remainder), *rhs))
                }
                // Cost-accounted per-clause signed bind `{% y <- x %}[s]` (W1). The
                // Phase-4 signed-JOIN dispatch (`normalize.rs` ForComprehension arm
                // → `recognize_signed_join` → `strip_signed_binds`) demotes EVERY
                // `Bind::Signed` to its linear bind BEFORE `normalize_p_input` sees
                // it, so this arm is unreachable in normal operation (the
                // `debug_assert` catches a dispatch regression). The release
                // fallback treats it as its underlying LINEAR bind — identical
                // receive shape / COMM count to the unsigned form, metered to the
                // deploy envelope — so a dispatch bug degrades gracefully rather
                // than miscompiling.
                Bind::Signed { lhs, rhs, .. } => {
                    debug_assert!(
                        false,
                        "Bind::Signed must be stripped by recognize_signed_join before \
                         normalize_p_input (W1 Phase 4 dispatch)"
                    );
                    let names: &'ast [Name<'ast>] = lhs.names.as_slice();
                    let remainder = lhs.remainder;

                    let source_name = match rhs {
                        Source::Simple { name } => *name,
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

        let source_free = input.free_map.clone();
        advance_input(Box::new(InputK {
            patterns,
            sources,
            guard: receipts[0].guard,
            body,
            persistent,
            peek,
            input,
            phase: InputPhase::Patterns {
                group_idx: 0,
                name_idx: 0,
            },
            group_pars: Vec::new(),
            group_free: FreeMap::new(),
            group_locally_free: Vec::new(),
            done_patterns: Vec::new(),
            source_pars: Vec::new(),
            source_free,
            source_locally_free: Vec::new(),
            source_connective_used: false,
            body_env: Default::default(),
            receive_binds: Vec::new(),
            bind_count: 0,
            patterns_locally_free: Vec::new(),
            guard_out: None,
        }))
    }
}

/// Walk `InputK`'s phase sequence until a child can be scheduled, or the whole
/// `for` can be assembled.
///
/// Phase transitions that consume **no** child value — end of a pattern group,
/// patterns exhausted, sources exhausted — are folded into this loop rather than
/// each costing a machine step, which is why it is a `loop` and not a single
/// `match`. Every transition preserves `NormKont::filled`, so the driver's slot
/// invariant holds across them.
#[inline(never)]
fn advance_input<'ast>(mut k: Box<InputK<'ast>>) -> Result<Step<'ast>, InterpreterError> {
    loop {
        match k.phase {
            InputPhase::Patterns {
                group_idx,
                name_idx,
            } => {
                if group_idx >= k.patterns.len() {
                    k.phase = InputPhase::Sources { idx: 0 };
                    continue;
                }
                // `Name<'ast>` and `Var<'ast>` are `Copy`, so both are copied out
                // of `k` here and every borrow of `k` ends before it moves onto
                // the work stack below.
                let names: &'ast [Name<'ast>] = k.patterns[group_idx].0;
                let remainder: Option<Var<'ast>> = k.patterns[group_idx].1;
                if let Some(name_ref) = names.get(name_idx) {
                    // Reject cost syntax in receive-bind pattern position (W1
                    // §1.5): a signed term / token stack inside a bound name
                    // `@{...}` is a process form (recognized + metered), not a
                    // receive pattern. The guard walks the arena, so it takes
                    // the arena-backed reference.
                    reject_cost_syntax_in_name_pattern(name_ref)?;
                    let name: Name<'ast> = *name_ref;
                    let child_input = NameVisitInputs {
                        bound_map_chain: k.input.bound_map_chain.push(),
                        free_map: k.group_free.clone(),
                    };
                    return Ok(Step::Descend {
                        kont: NormKont::Input(k),
                        work: NormWork::Name {
                            name,
                            input: child_input,
                        },
                    });
                }
                // The group is complete: absorb its remainder and bank it.
                let (optional_var, known_free) =
                    normalize_match_name(&remainder, k.group_free.clone())?;
                let group_pars = std::mem::take(&mut k.group_pars);
                let group_locally_free = std::mem::take(&mut k.group_locally_free);
                k.done_patterns
                    .push((group_pars, optional_var, known_free, group_locally_free));
                k.group_free = FreeMap::new();
                k.phase = InputPhase::Patterns {
                    group_idx: group_idx + 1,
                    name_idx: 0,
                };
            }

            InputPhase::Sources { idx } => {
                if let Some(name) = k.sources.get(idx).copied() {
                    let child_input = NameVisitInputs {
                        bound_map_chain: k.input.bound_map_chain.clone(),
                        free_map: k.source_free.clone(),
                    };
                    return Ok(Step::Descend {
                        kont: NormKont::Input(k),
                        work: NormWork::Name {
                            name,
                            input: child_input,
                        },
                    });
                }
                // Every pattern and every channel is in. Sort the binds, reject
                // duplicate channels, merge the per-bind free maps and open the
                // body's scope — all of it verbatim from the recursive form.
                let sources_par = std::mem::take(&mut k.source_pars);
                let receive_binds_and_free_maps = pre_sort_binds(
                    k.done_patterns
                        .clone()
                        .into_iter()
                        .zip(sources_par)
                        .map(|((a, b, c, _), e)| (a, b, e, c))
                        .collect(),
                )?;

                let (receive_binds, receive_bind_free_maps): (
                    Vec<ReceiveBind>,
                    Vec<FreeMap<VarSort>>,
                ) = receive_binds_and_free_maps.into_iter().unzip();

                // Channel duplicate check
                let channels: Vec<Par> = receive_binds
                    .clone()
                    .into_iter()
                    .map(|rb| {
                        rb.source
                            .expect("pre_sort_binds always emits a source for every bind")
                    })
                    .collect();

                let channels_set: HashSet<Par> = channels.clone().into_iter().collect();
                let has_same_channels = channels.len() > channels_set.len();

                if has_same_channels {
                    // ⚠ Tear the already-normalized patterns and channels down
                    // iteratively: `<Par as Drop>` is Θ(depth), and this is a
                    // REJECTION path, i.e. exactly where a hostile deploy lands.
                    let mut pars: Vec<Par> = channels;
                    for b in receive_binds {
                        pars.extend(b.patterns);
                        pars.extend(b.source);
                    }
                    dismantle_all(pars);
                    // TODO: Review
                    return Err(InterpreterError::ReceiveOnSameChannelsError {
                        source_span: k.body.span,
                    });
                }

                // Merge receive bind free maps
                let receive_binds_free_map = receive_bind_free_maps.into_iter().try_fold(
                    FreeMap::new(),
                    |known_free, receive_bind_free_map| {
                        let (updated_known_free, conflicts) =
                            known_free.merge(receive_bind_free_map);

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

                k.bind_count = receive_binds_free_map.count_no_wildcards();
                k.body_env = k
                    .input
                    .bound_map_chain
                    .absorb_free_span(&receive_binds_free_map);
                k.patterns_locally_free = std::mem::take(&mut k.done_patterns)
                    .into_iter()
                    .map(|pattern| pattern.3)
                    .fold(Vec::new(), union);
                k.receive_binds = receive_binds;

                k.phase = if k.guard.is_some() {
                    InputPhase::Guard
                } else {
                    InputPhase::Body
                };
            }

            InputPhase::Guard => {
                // Optional `where`-clause guard. By the time we reach this branch
                // there is a single receipt (multi-receipt is desugared into nested
                // `for`s above), so its guard is the one we care about. Normalize
                // the guard against the same scope as the body — the merged-bind
                // free map has been absorbed into bound_map_chain, so guard and
                // body see the same de Bruijn levels.
                let guard = k
                    .guard
                    .expect("advance_input: InputPhase::Guard is only entered when a guard exists");
                let child_input = ProcVisitInputs {
                    par: Par::default(),
                    bound_map_chain: k.body_env.clone(),
                    free_map: k.source_free.clone(),
                };
                return Ok(Step::Descend {
                    kont: NormKont::Input(k),
                    work: NormWork::Proc {
                        proc: guard,
                        input: child_input,
                    },
                });
            }

            InputPhase::Body => {
                let free_map = match &k.guard_out {
                    Some(gr) => gr.free_map.clone(),
                    None => k.source_free.clone(),
                };
                let child_input = ProcVisitInputs {
                    par: Par::default(),
                    bound_map_chain: k.body_env.clone(),
                    free_map,
                };
                let body = k.body;
                return Ok(Step::Descend {
                    kont: NormKont::Input(k),
                    work: NormWork::Proc {
                        proc: body,
                        input: child_input,
                    },
                });
            }
        }
    }
}

/// `for (…) { P }`, combine half.
#[inline(never)]
pub(crate) fn combine_p_input<'ast>(
    mut k: Box<InputK<'ast>>,
    value: NormVal,
) -> Result<Step<'ast>, InterpreterError> {
    match k.phase {
        InputPhase::Patterns {
            group_idx,
            name_idx,
        } => {
            let res = value.into_name();
            fail_on_invalid_connective(&k.input, &res)?;

            // ★ Leg-1: shallow read, then MOVE. See
            // `collection_normalize_matcher::combine_collect`.
            let pattern_locally_free = res.par.locally_free.clone();
            k.group_pars.push(res.par);
            k.group_free = res.free_map;
            k.group_locally_free = union(
                std::mem::take(&mut k.group_locally_free),
                pattern_locally_free,
            );
            k.phase = InputPhase::Patterns {
                group_idx,
                name_idx: name_idx + 1,
            };
            advance_input(k)
        }

        InputPhase::Sources { idx } => {
            let res = value.into_name();
            // ★ Leg-1: shallow reads, then MOVE.
            let source_locally_free = res.par.locally_free.clone();
            let source_connective_used = res.par.connective_used;
            k.source_pars.push(res.par);
            k.source_free = res.free_map;
            k.source_locally_free = union(
                std::mem::take(&mut k.source_locally_free),
                source_locally_free,
            );
            k.source_connective_used = k.source_connective_used || source_connective_used;
            k.phase = InputPhase::Sources { idx: idx + 1 };
            advance_input(k)
        }

        InputPhase::Guard => {
            let guard_out = value.into_proc();
            // ★ THE GUARD-DECIDABILITY GATE, receive half. A `where` guard is
            // decided by `rho-pure-eval` inside the matcher, which implements a
            // subset of the expression language; a guard outside that subset
            // used to be reported as `false` — a silently unfireable COMM with
            // no error to read. Refused here instead, where the source position
            // is still in hand and refusing costs the author nothing but a
            // rewrite. See `interpreter::guard` for why refusing beats both
            // widening the decider and raising from inside the matcher.
            reject_undecidable_guard(&guard_out.par, RECEIVE_WHERE)?;
            k.guard_out = Some(guard_out);
            k.phase = InputPhase::Body;
            advance_input(k)
        }

        InputPhase::Body => {
            let mut proc_visit_outputs = value.into_proc();
            let InputK {
                persistent,
                peek,
                mut input,
                source_locally_free,
                source_connective_used,
                receive_binds,
                bind_count,
                patterns_locally_free,
                guard_out,
                ..
            } = *k;

            // The guard is copied in the recursive implementation, but the
            // body cache is transferred to `Receive::locally_free`.
            let guard_locally_free = guard_out
                .as_ref()
                .map(|gr| gr.par.locally_free.clone())
                .unwrap_or_default();
            let guard_connective_used = guard_out
                .as_ref()
                .map(|gr| gr.par.connective_used)
                .unwrap_or(false);
            let guard_par = guard_out.map(|gr| gr.par);
            let body_locally_free = std::mem::take(&mut proc_visit_outputs.par.locally_free);
            let body_connective_used = proc_visit_outputs.par.connective_used;

            Ok(Step::Done(NormVal::Proc(ProcVisitOutputs {
                par: input.par.prepend_receive(Receive {
                    binds: receive_binds,
                    body: Some(proc_visit_outputs.par),
                    persistent,
                    peek,
                    bind_count: bind_count as i32,
                    locally_free: {
                        union(
                            source_locally_free,
                            union(
                                patterns_locally_free,
                                filter_and_adjust_bitset(
                                    union(body_locally_free, guard_locally_free),
                                    bind_count,
                                ),
                            ),
                        )
                    },
                    connective_used: source_connective_used
                        || body_connective_used
                        || guard_connective_used,
                    condition: guard_par,
                }),
                free_map: proc_visit_outputs.free_map,
            })))
        }
    }
}

/// A `for`-comprehension on a **fresh** drive. Only the unit tests enter here.
pub fn normalize_p_input<'ast>(
    receipts: &'ast Receipts<'ast>,
    body: &'ast AnnProc<'ast>,
    input: ProcVisitInputs,
    env: &HashMap<String, Par>,
    parser: &'ast rholang_parser::RholangParser<'ast>,
) -> Result<ProcVisitOutputs, InterpreterError> {
    use crate::rust::interpreter::compiler::normalize_drive::norm_drive_from;
    let step = descend_p_input(receipts, *body, input, parser)?;
    norm_drive_from(step, env, parser).map(NormVal::into_proc)
}

// See rholang/src/test/scala/coop/rchain/rholang/interpreter/compiler/normalizer/ProcMatcherSpec.scala
#[cfg(test)]
mod tests {
    use models::create_bit_vector;
    use models::rhoapi::Receive;
    use models::rust::utils::{
        new_boundvar_par, new_elist_par, new_freevar_par, new_freevar_var, new_gint_par, new_send,
        new_send_par,
    };

    use super::*;
    use crate::rust::interpreter::compiler::compiler::Compiler;
    use crate::rust::interpreter::compiler::exports::BoundMapChain;

    fn inputs_span() -> ProcVisitInputs {
        ProcVisitInputs {
            par: Par::default(),
            bound_map_chain: BoundMapChain::new(),
            free_map: FreeMap::new(),
        }
    }

    #[test]
    fn p_input_should_handle_a_simple_receive() {
        // for ( x, y <- @Nil ) { x!(*y) }
        use rholang_parser::ast::{Bind, Names, SendType, Source};

        use crate::rust::interpreter::compiler::normalize::normalize_ann_proc;
        use crate::rust::interpreter::test_utils::par_builder_util::ParBuilderUtil;

        let (mut inputs_data, env) = (inputs_span(), HashMap::new());
        let parser = rholang_parser::RholangParser::new();

        // Create bind: x, y <- @Nil
        let nil_proc = ParBuilderUtil::create_ast_nil(&parser);
        let channel = ParBuilderUtil::create_ast_quote_name(nil_proc);

        let bind = Bind::Linear {
            lhs: Names {
                names: smallvec::SmallVec::from_vec(vec![
                    ParBuilderUtil::create_ast_name_var("x"),
                    ParBuilderUtil::create_ast_name_var("y"),
                ]),
                remainder: None,
            },
            rhs: Source::Simple { name: channel },
        };

        // Create body: x!(*y)
        let x_channel = ParBuilderUtil::create_ast_name_var("x");
        let y_eval =
            ParBuilderUtil::create_ast_eval(ParBuilderUtil::create_ast_name_var("y"), &parser);
        let body =
            ParBuilderUtil::create_ast_send(x_channel, SendType::Single, vec![y_eval], &parser);

        // Create ForComprehension
        let for_comprehension =
            ParBuilderUtil::create_ast_for_comprehension(vec![vec![bind]], body, &parser);

        let result = normalize_ann_proc(&for_comprehension, inputs_data.clone(), &env, &parser);
        assert!(result.is_ok());

        let bind_count = 2;
        let expected_result = inputs_data.par.prepend_receive(Receive {
            binds: vec![ReceiveBind {
                patterns: vec![
                    new_freevar_par(0, Vec::new()),
                    new_freevar_par(1, Vec::new()),
                ],
                source: Some(Par::default()),
                remainder: None,
                free_count: 2,
            }],
            body: Some(new_send_par(
                new_boundvar_par(1, create_bit_vector(&vec![1]), false),
                vec![new_boundvar_par(0, create_bit_vector(&vec![0]), false)],
                false,
                create_bit_vector(&vec![0, 1]),
                false,
                create_bit_vector(&vec![0, 1]),
                false,
            )),
            persistent: false,
            peek: false,
            bind_count,
            locally_free: Vec::new(),
            connective_used: false,
            condition: None,
        });

        assert_eq!(result.clone().unwrap().par, expected_result);
        assert_eq!(result.unwrap().free_map, inputs_data.free_map);
    }

    #[test]
    fn p_input_should_handle_peek() {
        // for ( x, y <<- @Nil ) { x!(*y) }
        use crate::rust::interpreter::test_utils::par_builder_util::ParBuilderUtil;

        let result = ParBuilderUtil::mk_term(r#"for ( x, y <<- @Nil ) { x!(*y) }"#);

        assert!(
            result.is_ok(),
            "Failed to parse and normalize the Rholang code"
        );
        let normalized = result.unwrap();

        assert!(
            !normalized.receives.is_empty(),
            "Should have at least one receive"
        );
        assert_eq!(
            normalized.receives[0].peek, true,
            "Peek should be true for <<- operator"
        );
    }

    #[test]
    fn p_input_should_bind_whole_list_to_the_list_remainder() {
        // for (@[...a] <- @0) { Nil }
        use rholang_parser::ast::{Bind, Id, Names, Source, Var};
        use rholang_parser::SourcePos;

        use crate::rust::interpreter::compiler::normalize::normalize_ann_proc;
        use crate::rust::interpreter::test_utils::par_builder_util::ParBuilderUtil;

        let (mut inputs_data, env) = (inputs_span(), HashMap::new());
        let parser = rholang_parser::RholangParser::new();

        // Create pattern: @[...a]
        let a_var = Var::Id(Id {
            name: "a",
            pos: SourcePos { line: 0, col: 0 },
        });
        let list_with_remainder =
            ParBuilderUtil::create_ast_list_remainder(Vec::new(), a_var, &parser);
        let pattern = ParBuilderUtil::create_ast_quote_name(list_with_remainder);

        // Create bind: @[...a] <- @Nil
        let nil_proc = ParBuilderUtil::create_ast_nil(&parser);
        let channel = ParBuilderUtil::create_ast_quote_name(nil_proc);

        let bind = Bind::Linear {
            lhs: Names {
                names: smallvec::SmallVec::from_vec(vec![pattern]),
                remainder: None,
            },
            rhs: Source::Simple { name: channel },
        };

        // Create body: Nil
        let body = ParBuilderUtil::create_ast_nil(&parser);

        // Create ForComprehension
        let for_comprehension =
            ParBuilderUtil::create_ast_for_comprehension(vec![vec![bind]], body, &parser);

        let result = normalize_ann_proc(&for_comprehension, inputs_data.clone(), &env, &parser);
        assert!(result.is_ok());

        let bind_count = 1;
        let expected_result = inputs_data.par.prepend_receive(Receive {
            binds: vec![ReceiveBind {
                patterns: vec![new_elist_par(
                    Vec::new(),
                    Vec::new(),
                    true,
                    Some(new_freevar_var(0)),
                    Vec::new(),
                    true,
                )],
                source: Some(Par::default()),
                remainder: None,
                free_count: 1,
            }],
            body: Some(Par::default()),
            persistent: false,
            peek: false,
            bind_count,
            locally_free: Vec::new(),
            connective_used: false,
            condition: None,
        });

        assert_eq!(result.unwrap().par, expected_result);
    }

    #[test]
    fn p_input_should_handle_a_more_complicated_receive() {
        // for ( (x1, @y1) <- @Nil  & (x2, @y2) <- @1) { x1!(y2) | x2!(y1) }
        use rholang_parser::ast::{Bind, Names, SendType, Source};

        use crate::rust::interpreter::compiler::normalize::normalize_ann_proc;
        use crate::rust::interpreter::test_utils::par_builder_util::ParBuilderUtil;

        let (mut inputs_data, env) = (inputs_span(), HashMap::new());
        let parser = rholang_parser::RholangParser::new();

        // Create first bind: x1, @y1 <- @Nil
        let y1_eval =
            ParBuilderUtil::create_ast_eval(ParBuilderUtil::create_ast_name_var("y1"), &parser);
        let y1_pattern = ParBuilderUtil::create_ast_quote_name(y1_eval);
        let nil_proc = ParBuilderUtil::create_ast_nil(&parser);
        let nil_channel = ParBuilderUtil::create_ast_quote_name(nil_proc);

        let bind1 = Bind::Linear {
            lhs: Names {
                names: smallvec::SmallVec::from_vec(vec![
                    ParBuilderUtil::create_ast_name_var("x1"),
                    y1_pattern,
                ]),
                remainder: None,
            },
            rhs: Source::Simple { name: nil_channel },
        };

        // Create second bind: x2, @y2 <- @1
        let y2_eval =
            ParBuilderUtil::create_ast_eval(ParBuilderUtil::create_ast_name_var("y2"), &parser);
        let y2_pattern = ParBuilderUtil::create_ast_quote_name(y2_eval);
        let one_proc = ParBuilderUtil::create_ast_long_literal(1, &parser);
        let one_channel = ParBuilderUtil::create_ast_quote_name(one_proc);

        let bind2 = Bind::Linear {
            lhs: Names {
                names: smallvec::SmallVec::from_vec(vec![
                    ParBuilderUtil::create_ast_name_var("x2"),
                    y2_pattern,
                ]),
                remainder: None,
            },
            rhs: Source::Simple { name: one_channel },
        };

        // Create body: x1!(y2) | x2!(y1)
        let x1_channel = ParBuilderUtil::create_ast_name_var("x1");
        let y2_eval_send =
            ParBuilderUtil::create_ast_eval(ParBuilderUtil::create_ast_name_var("y2"), &parser);
        let send1 = ParBuilderUtil::create_ast_send(
            x1_channel,
            SendType::Single,
            vec![y2_eval_send],
            &parser,
        );

        let x2_channel = ParBuilderUtil::create_ast_name_var("x2");
        let y1_eval_send =
            ParBuilderUtil::create_ast_eval(ParBuilderUtil::create_ast_name_var("y1"), &parser);
        let send2 = ParBuilderUtil::create_ast_send(
            x2_channel,
            SendType::Single,
            vec![y1_eval_send],
            &parser,
        );

        let body = ParBuilderUtil::create_ast_par(send1, send2, &parser);

        // Create ForComprehension - Two separate receipt groups (for/; syntax)
        // With desugaring, this becomes nested for loops:
        //   for (x1, @y1 <- @Nil) { for (x2, @y2 <- @1) { x1!(y2) | x2!(y1) } }
        let for_comprehension = ParBuilderUtil::create_ast_for_comprehension(
            vec![vec![bind1], vec![bind2]],
            body,
            &parser,
        );

        let result = normalize_ann_proc(&for_comprehension, inputs_data.clone(), &env, &parser);
        assert!(result.is_ok(), "Normalization failed: {:?}", result.err());

        // Desugared form: for (x1, @y1 <- @Nil) { for (x2, @y2 <- @1) { x1!(y2) | x2!(y1) } }
        // Outer Receive: 1 bind (x1, @y1 <- @Nil), bind_count=2
        // Inner Receive: 1 bind (x2, @y2 <- @1), bind_count=2
        // Body: x1!(y2) | x2!(y1)
        //   BoundVar(0)=x2, BoundVar(1)=y2 (inner), BoundVar(2)=x1, BoundVar(3)=y1 (outer)
        let expected_result = inputs_data.par.prepend_receive(Receive {
            binds: vec![ReceiveBind {
                patterns: vec![
                    new_freevar_par(0, Vec::new()),
                    new_freevar_par(1, Vec::new()),
                ],
                source: Some(Par::default()),
                remainder: None,
                free_count: 2,
            }],
            body: Some({
                let mut inner_par = Par::default();
                inner_par.receives.push(Receive {
                    binds: vec![ReceiveBind {
                        patterns: vec![
                            new_freevar_par(0, Vec::new()),
                            new_freevar_par(1, Vec::new()),
                        ],
                        source: Some(new_gint_par(1, Vec::new(), false)),
                        remainder: None,
                        free_count: 2,
                    }],
                    body: Some({
                        let mut par = Par::default().with_sends(vec![
                            new_send(
                                new_boundvar_par(1, create_bit_vector(&vec![1]), false),
                                vec![new_boundvar_par(2, create_bit_vector(&vec![2]), false)],
                                false,
                                create_bit_vector(&vec![1, 2]),
                                false,
                            ),
                            new_send(
                                new_boundvar_par(3, create_bit_vector(&vec![3]), false),
                                vec![new_boundvar_par(0, create_bit_vector(&vec![0]), false)],
                                false,
                                create_bit_vector(&vec![0, 3]),
                                false,
                            ),
                        ]);
                        par.locally_free = create_bit_vector(&vec![0, 1, 2, 3]);
                        par
                    }),
                    persistent: false,
                    peek: false,
                    bind_count: 2,
                    locally_free: create_bit_vector(&vec![0, 1]),
                    connective_used: false,
                    condition: None,
                });
                inner_par.locally_free = create_bit_vector(&vec![0, 1]);
                inner_par
            }),
            persistent: false,
            peek: false,
            bind_count: 2,
            locally_free: Vec::new(),
            connective_used: false,
            condition: None,
        });

        assert_eq!(result.unwrap().par, expected_result);
    }

    #[test]
    fn p_input_should_fail_if_a_free_variable_is_used_in_same_receipt_join() {
        // for ( (x1, @y1) <- @Nil  & (x2, @y1) <- @1) { Nil }
        // Uses & (single receipt with 2 binds = join), NOT ; (separate receipts).
        // Reusing y1 within the same join is an error.
        use rholang_parser::ast::{Bind, Names, Source};

        use crate::rust::interpreter::compiler::normalize::normalize_ann_proc;
        use crate::rust::interpreter::test_utils::par_builder_util::ParBuilderUtil;

        let parser = rholang_parser::RholangParser::new();

        let y1_eval =
            ParBuilderUtil::create_ast_eval(ParBuilderUtil::create_ast_name_var("y1"), &parser);
        let y1_pattern = ParBuilderUtil::create_ast_quote_name(y1_eval);
        let nil_proc = ParBuilderUtil::create_ast_nil(&parser);
        let nil_channel = ParBuilderUtil::create_ast_quote_name(nil_proc);

        let bind1 = Bind::Linear {
            lhs: Names {
                names: smallvec::SmallVec::from_vec(vec![
                    ParBuilderUtil::create_ast_name_var("x1"),
                    y1_pattern,
                ]),
                remainder: None,
            },
            rhs: Source::Simple { name: nil_channel },
        };

        let y1_eval2 =
            ParBuilderUtil::create_ast_eval(ParBuilderUtil::create_ast_name_var("y1"), &parser);
        let y1_pattern2 = ParBuilderUtil::create_ast_quote_name(y1_eval2);
        let one_proc = ParBuilderUtil::create_ast_long_literal(1, &parser);
        let one_channel = ParBuilderUtil::create_ast_quote_name(one_proc);

        let bind2 = Bind::Linear {
            lhs: Names {
                names: smallvec::SmallVec::from_vec(vec![
                    ParBuilderUtil::create_ast_name_var("x2"),
                    y1_pattern2,
                ]),
                remainder: None,
            },
            rhs: Source::Simple { name: one_channel },
        };

        let body = ParBuilderUtil::create_ast_nil(&parser);

        // Single receipt with 2 binds (& join) — NOT 2 receipt groups (;)
        let for_comprehension =
            ParBuilderUtil::create_ast_for_comprehension(vec![vec![bind1, bind2]], body, &parser);

        let result =
            normalize_ann_proc(&for_comprehension, inputs_span(), &HashMap::new(), &parser);
        assert!(result.is_err());
        assert!(matches!(
            result,
            Err(InterpreterError::UnexpectedReuseOfNameContextFree {
                var_name,
                first_use: _,
                second_use: _
            }) if var_name == "y1"
        ));
    }

    #[test]
    fn p_input_should_not_compile_when_connectives_are_used_in_the_channel() {
        // Test disjunction in channel
        let result1 = Compiler::source_to_adt(r#"for(x <- @{Nil \/ Nil}){ Nil }"#);
        assert!(result1.is_err());
        match result1 {
            Err(InterpreterError::TopLevelLogicalConnectivesNotAllowedError(msg)) => {
                assert!(msg.contains("\\/ (disjunction)"));
            }
            other => panic!(
                "Expected TopLevelLogicalConnectivesNotAllowedError, got: {:?}",
                other
            ),
        }

        // Test conjunction in channel
        let result2 = Compiler::source_to_adt(r#"for(x <- @{Nil /\ Nil}){ Nil }"#);
        assert!(result2.is_err());
        match result2 {
            Err(InterpreterError::TopLevelLogicalConnectivesNotAllowedError(msg)) => {
                assert!(msg.contains("/\\ (conjunction)"));
            }
            other => panic!(
                "Expected TopLevelLogicalConnectivesNotAllowedError, got: {:?}",
                other
            ),
        }

        // Test negation in channel
        let result3 = Compiler::source_to_adt(r#"for(x <- @{~Nil}){ Nil }"#);
        assert!(result3.is_err());
        match result3 {
            Err(InterpreterError::TopLevelLogicalConnectivesNotAllowedError(msg)) => {
                assert!(msg.contains("~ (negation)"));
            }
            other => panic!(
                "Expected TopLevelLogicalConnectivesNotAllowedError, got: {:?}",
                other
            ),
        }
    }

    #[test]
    fn p_input_should_not_compile_when_connectives_are_at_the_top_level_expression_in_the_body() {
        // Test conjunction in body
        let result1 = Compiler::source_to_adt(r#"for(x <- @Nil){ 1 /\ 2 }"#);
        assert!(result1.is_err());
        match result1 {
            Err(InterpreterError::TopLevelLogicalConnectivesNotAllowedError(msg)) => {
                assert!(msg.contains("/\\ (conjunction)"));
            }
            other => panic!(
                "Expected TopLevelLogicalConnectivesNotAllowedError, got: {:?}",
                other
            ),
        }

        // Test disjunction in body
        let result2 = Compiler::source_to_adt(r#"for(x <- @Nil){ 1 \/ 2 }"#);
        assert!(result2.is_err());
        match result2 {
            Err(InterpreterError::TopLevelLogicalConnectivesNotAllowedError(msg)) => {
                assert!(msg.contains("\\/ (disjunction)"));
            }
            other => panic!(
                "Expected TopLevelLogicalConnectivesNotAllowedError, got: {:?}",
                other
            ),
        }

        // Test negation in body
        let result3 = Compiler::source_to_adt(r#"for(x <- @Nil){ ~1 }"#);
        assert!(result3.is_err());
        match result3 {
            Err(InterpreterError::TopLevelLogicalConnectivesNotAllowedError(msg)) => {
                assert!(msg.contains("~ (negation)"));
            }
            other => panic!(
                "Expected TopLevelLogicalConnectivesNotAllowedError, got: {:?}",
                other
            ),
        }
    }

    #[test]
    fn p_input_should_not_compile_when_logical_or_or_not_is_used_in_pattern_of_receive() {
        // Test disjunction in pattern
        let result1 = Compiler::source_to_adt(r#"new x in { for(@{Nil \/ Nil} <- x) { Nil } }"#);
        assert!(result1.is_err());
        match result1 {
            Err(InterpreterError::PatternReceiveError(msg)) => {
                assert!(msg.contains("\\/ (disjunction)"));
            }
            other => panic!("Expected PatternReceiveError, got: {:?}", other),
        }

        // Test negation in pattern
        let result2 = Compiler::source_to_adt(r#"new x in { for(@{~Nil} <- x) { Nil } }"#);
        assert!(result2.is_err());
        match result2 {
            Err(InterpreterError::PatternReceiveError(msg)) => {
                assert!(msg.contains("~ (negation)"));
            }
            other => panic!("Expected PatternReceiveError, got: {:?}", other),
        }
    }

    #[test]
    fn p_input_should_compile_when_logical_and_is_used_in_pattern_of_receive() {
        // Test that conjunction in pattern is allowed
        let result1 = Compiler::source_to_adt(r#"new x in { for(@{Nil /\ Nil} <- x) { Nil } }"#);
        assert!(
            result1.is_ok(),
            "Conjunction in pattern should be allowed, but got error: {:?}",
            result1
        );
    }
}
