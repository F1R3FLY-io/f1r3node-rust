use models::rhoapi::{CostRegion, CostSignature};
use models::rust::par_construction::ParConstruction;
use rholang_parser::ast::{AnnProc, Receipts, Signature, TokenStack};
use rholang_parser::{RholangParser, SourceSpan};

use super::desugar;
use super::ir::Sig;
use crate::rust::interpreter::compiler::bound_map_chain::BoundMapChain;
use crate::rust::interpreter::compiler::normalize::VarSort;
use crate::rust::interpreter::compiler::normalize_drive::{
    NormKont, NormVal, NormWork, ProcVisitInputs, ProcVisitOutputs, SigInput, Step,
};
use crate::rust::interpreter::errors::InterpreterError;

fn cost_region(
    span: SourceSpan,
    ordinal: u32,
    signer: CostSignature,
) -> Result<CostRegion, InterpreterError> {
    let coordinate = |value| {
        u64::try_from(value).map_err(|_| {
            InterpreterError::NormalizerError(
                "cost-accounting: source coordinate exceeds u64".to_string(),
            )
        })
    };
    Ok(CostRegion {
        version: 1,
        start_line: coordinate(span.start.line)?,
        start_column: coordinate(span.start.col)?,
        end_line: coordinate(span.end.line)?,
        end_column: coordinate(span.end.col)?,
        signer: Some(signer),
        ordinal,
    })
}

/// `{% P %}[s]`, descend half.
///
/// Two things happen and they are ordered: the surface signature is applied as
/// a **term-level AST rewrite** (lollipop re-signs the continuation with `s₂`
/// and funds the rendezvous with `s₁`; a core sig applies uniform signing), and
/// the rewritten signature is then **resolved** — which is where a wildcard
/// `_`, a quoted-principal `@P` ground sig, or a bare lollipop in fundable
/// position is rejected.
///
/// Resolution is itself part of this SCC: `Signature::Hash(#P)` canonicalises
/// `P` by *normalizing* it, so a `#` signature nested `n` deep costs `n`
/// levels. It therefore travels on the machine as a [`NormWork::Sig`], and the
/// lowering of `P` is the continuation's [`Step::Tail`] — no gate node, and no
/// frame.
#[inline(never)]
pub(crate) fn descend_signed_term<'ast>(
    inner: AnnProc<'ast>,
    sig: &'ast Signature<'ast>,
    span: SourceSpan,
    input: ProcVisitInputs,
    parser: &'ast RholangParser<'ast>,
) -> Result<Step<'ast>, InterpreterError> {
    // Lollipop `s1 -o s2`: re-sign the continuation with `s2` (an AST rewrite),
    // fund the rendezvous with `s1`. A core sig applies uniform-signing so a `for`
    // continuation is metered to the same `s`. Both are term-level AST→AST; the
    // inner gate (if any) is produced by the ordinary dispatch recursion below.
    let (core_inner, core_sig): (AnnProc<'ast>, &'ast Signature<'ast>) = match sig {
        Signature::Transfer(s1, s2) => (desugar::lollipop(inner, s2, parser)?, s1),
        core => (desugar::uniform_sign(inner, core, parser), core),
    };
    let bound_map_chain = input.bound_map_chain.clone();
    // VALIDATE that `s` resolves to a native funding `Sig` (rejects a wildcard
    // `_`, a quoted-principal `@P` ground sig, or a bare lollipop in fundable
    // position). Per-redex attribution is the REDUCER's channel match (Phase 3,
    // `metering::note_channel_lane`) on the COMM's resolved channel — NOT a
    // normalizer-side binding (the `Par` carries no signature field). So
    // recognition only validates + lowers `P`; under s₀ every COMM attributes to
    // the deploy envelope.
    Ok(Step::Descend {
        kont: NormKont::SignedTerm {
            core_inner,
            span,
            input,
        },
        work: NormWork::Sig {
            sig: SigInput::Ref(core_sig),
            bound_map_chain,
        },
    })
}

/// `{% P %}[s]`, combine half — the signature resolved, so lower `P`.
///
/// The resolved `Sig` is **discarded**: recognition validates, it does not bind.
/// `Sig::to_native()` is applied first so that any bridging error the native
/// algebra would raise is raised here, exactly where
/// `signature_to_native_sig(..)?` used to raise it.
#[inline(never)]
pub(crate) fn combine_signed_term<'ast>(
    core_inner: AnnProc<'ast>,
    span: SourceSpan,
    mut input: ProcVisitInputs,
    value: NormVal,
) -> Result<Step<'ast>, InterpreterError> {
    let region = cost_region(span, 0, value.into_sig().to_cost_signature())?;
    input.bound_map_chain = input.bound_map_chain.with_cost_region(region);
    Ok(Step::Tail(NormWork::Proc {
        proc: core_inner,
        input,
    }))
}

#[inline(never)]
pub(crate) fn descend_token_stack<'ast>(
    stack: &'ast TokenStack<'ast>,
    input: ProcVisitInputs,
    construction: &mut ParConstruction,
) -> Result<Step<'ast>, InterpreterError> {
    schedule_layer(
        &stack.layers,
        0,
        Vec::with_capacity(stack.layers.len()),
        input,
        construction,
    )
}

#[inline(never)]
pub(crate) fn combine_token_stack<'ast>(
    layers: &'ast [Signature<'ast>],
    idx: usize,
    mut ordered: Vec<models::rhoapi::CostSignature>,
    input: ProcVisitInputs,
    value: NormVal,
    construction: &mut ParConstruction,
) -> Result<Step<'ast>, InterpreterError> {
    ordered.push(value.into_sig().to_cost_signature());
    schedule_layer(layers, idx + 1, ordered, input, construction)
}

/// Schedule layer `idx`, or finish the stack.
fn schedule_layer<'ast>(
    layers: &'ast [Signature<'ast>],
    idx: usize,
    ordered: Vec<models::rhoapi::CostSignature>,
    input: ProcVisitInputs,
    construction: &mut ParConstruction,
) -> Result<Step<'ast>, InterpreterError> {
    match layers.get(idx) {
        Some(layer) => {
            let bound_map_chain = input.bound_map_chain.clone();
            Ok(Step::Descend {
                kont: NormKont::TokenStack {
                    layers,
                    idx,
                    ordered,
                    input,
                },
                work: NormWork::Sig {
                    sig: SigInput::Ref(layer),
                    bound_map_chain,
                },
            })
        }
        None => {
            let stack = models::rhoapi::CostTokenStack { layers: ordered };
            let component = construction.cost_token_stack_component(&stack, &[])?;
            Ok(Step::Done(NormVal::Proc(ProcVisitOutputs {
                par: construction.append_component(input.par, component, Vec::new(), false)?,
                free_map: input.free_map,
            })))
        }
    }
}

/// `for(... {% y <- x %}[s] ...)`: a JOIN carrying one or more per-clause SIGNED
/// binds (W1 Phase 4 / Axis-C). Recover the natural-arity plain join (Greg's rule:
/// fuel is NEVER folded into the data join — [`desugar::strip_signed_binds`]),
/// VALIDATE each clause signature, then lower the plain join through the ORDINARY
/// dispatch. Per-clause lane attribution is the reducer's channel match (Phase 3);
/// the continuation is NOT re-signed (one token per clause). Emits NO gate node, so
/// the normalized `Par` of a signed join is the SAME as its unsigned-equivalent
/// `for` (the double-metering avoidance, extended to joins).
#[inline(never)]
pub(crate) fn descend_signed_join<'ast>(
    receipts: &'ast Receipts<'ast>,
    body: AnnProc<'ast>,
    span: SourceSpan,
    input: ProcVisitInputs,
    parser: &'ast RholangParser<'ast>,
) -> Step<'ast> {
    // Strip every `Bind::Signed` back to its linear bind and collect the clause
    // signatures (source order). The recovered `for` is the natural-arity data
    // join — no fuel bind enters its `ReceiveBind` set.
    let (plain_for, clause_sigs) = desugar::strip_signed_binds(receipts, body, span, parser);
    let clause_positions = receipts
        .iter()
        .flat_map(|receipt| receipt.iter())
        .enumerate()
        .filter_map(|(index, bind)| {
            matches!(bind, rholang_parser::ast::Bind::Signed { .. }).then_some(index)
        })
        .collect::<Vec<_>>();
    let regions = vec![None; receipts.iter().map(|receipt| receipt.len()).sum()];
    debug_assert!(
        !clause_sigs.is_empty(),
        "descend_signed_join is dispatched only when a Bind::Signed is present"
    );
    schedule_clause(
        plain_for,
        clause_sigs,
        clause_positions,
        regions,
        span,
        0,
        input,
    )
}

/// `for(... {% y <- x %}[s] ...)`, combine half.
#[inline(never)]
pub(crate) fn combine_signed_join<'ast>(
    plain_for: AnnProc<'ast>,
    clause_sigs: Vec<Signature<'ast>>,
    clause_positions: Vec<usize>,
    mut regions: Vec<Option<CostRegion>>,
    span: SourceSpan,
    idx: usize,
    _bound_map_chain: BoundMapChain<VarSort>,
    input: ProcVisitInputs,
    value: NormVal,
) -> Result<Step<'ast>, InterpreterError> {
    let ordinal = u32::try_from(clause_positions[idx] + 1).map_err(|_| {
        InterpreterError::NormalizerError("cost-accounting: clause index exceeds u32".to_string())
    })?;
    regions[clause_positions[idx]] = Some(cost_region(
        span,
        ordinal,
        value.into_sig().to_cost_signature(),
    )?);
    Ok(schedule_clause(
        plain_for,
        clause_sigs,
        clause_positions,
        regions,
        span,
        idx + 1,
        input,
    ))
}

/// Schedule clause signature `idx`, or lower the recovered plain join.
///
/// ⚠ The clause signatures are **owned** (`strip_signed_binds` clones them out
/// of the binds), so the one being validated is moved out of the vector rather
/// than borrowed from a continuation that is itself about to move onto the work
/// stack. `std::mem::replace` leaves a placeholder so the indices of the
/// remaining clauses — and therefore `NormKont::arity` — do not shift.
fn schedule_clause<'ast>(
    plain_for: AnnProc<'ast>,
    mut clause_sigs: Vec<Signature<'ast>>,
    clause_positions: Vec<usize>,
    regions: Vec<Option<CostRegion>>,
    span: SourceSpan,
    idx: usize,
    mut input: ProcVisitInputs,
) -> Step<'ast> {
    if idx >= clause_sigs.len() {
        // Lower the recovered PLAIN join ordinarily: its binds are now linear, so it
        // re-normalizes through `normalize_p_input` with NO signed-bind recursion.
        input.bound_map_chain = input.bound_map_chain.with_pending_clause_regions(regions);
        return Step::Tail(NormWork::Proc {
            proc: plain_for,
            input,
        });
    }
    // VALIDATE each clause signature (rejects a wildcard `_`, a quoted-principal
    // `@P` ground sig, or a bare lollipop in fundable position) — the same
    // recognition `signature_to_native_sig` applies to a `{% P %}[s]` term. Phase
    // 3's channel match attributes each clause's rendezvous COMM to its signer lane
    // at the reducer; here recognition only validates + recovers the plain join.
    let sig = std::mem::replace(
        &mut clause_sigs[idx],
        Signature::Ground(rholang_parser::ast::Name::NameVar(
            rholang_parser::ast::Var::Wildcard,
        )),
    );
    let bound_map_chain = input.bound_map_chain.clone();
    Step::Descend {
        kont: NormKont::SignedJoin {
            plain_for,
            clause_sigs,
            clause_positions,
            regions,
            span,
            idx,
            bound_map_chain: bound_map_chain.clone(),
            input,
        },
        work: NormWork::Sig {
            sig: SigInput::Own(sig),
            bound_map_chain,
        },
    }
}

/// Retained so the unused-import lint does not fire; `Sig` names the value this
/// module's continuations consume.
#[allow(dead_code)]
type _Sig = Sig;
