use models::rhoapi::cost_signature::Value as CostSignatureValue;
use models::rhoapi::{CostSignature, CostSignatureCompound, CostSignedTerm, CostStack, Par};
use models::rust::rholang::sorter::cost_accounting_sorter::sort_signature;
use models::rust::utils::union;
use rholang_parser::ast::{AnnProc, Signature, TokenStack};
use rholang_parser::RholangParser;

use super::desugar;
use super::sig::wire_signature_locally_free;
use crate::rust::interpreter::compiler::normalize::ProcVisitInputs;
use crate::rust::interpreter::compiler::normalize_drive::{
    NormKont, NormVal, NormWork, SigInput, Step,
};
use crate::rust::interpreter::errors::InterpreterError;

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
/// lowering of `P` is a [`Step::Handoff`] to a fresh body continuation — no
/// gate node and no native-stack recursion.
#[inline(never)]
pub(crate) fn descend_signed_term<'ast>(
    inner: AnnProc<'ast>,
    sig: &'ast Signature<'ast>,
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
    Ok(Step::Descend {
        kont: NormKont::SignedTermSignature {
            core_inner,
            input: input.clone(),
        },
        work: NormWork::WireSig {
            sig: SigInput::Ref(core_sig),
            bound_map_chain: input.bound_map_chain,
            free_map: input.free_map,
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
    input: ProcVisitInputs,
    value: NormVal,
) -> Result<Step<'ast>, InterpreterError> {
    let (signature, free_map) = value.into_wire_sig();
    let child_input = ProcVisitInputs {
        par: Par::default(),
        bound_map_chain: input.bound_map_chain.clone(),
        free_map,
    };
    Ok(Step::Handoff {
        kont: NormKont::SignedTermBody { signature, input },
        work: NormWork::Proc {
            proc: core_inner,
            input: child_input,
        },
    })
}

pub(crate) fn combine_signed_term_body<'ast>(
    signature: CostSignature,
    input: ProcVisitInputs,
    value: NormVal,
) -> Result<Step<'ast>, InterpreterError> {
    let normalized = value.into_proc();
    let mut par = input.par;
    par.locally_free = union(
        std::mem::take(&mut par.locally_free),
        union(
            normalized.par.locally_free.clone(),
            wire_signature_locally_free(&signature),
        ),
    );
    par.connective_used |= normalized.par.connective_used;
    let (body, signature) = flatten_nested_signature(normalized.par, signature);
    par.cost_signed_terms.push(CostSignedTerm {
        body: Some(body),
        signature: Some(signature),
    });
    Ok(Step::Done(NormVal::Proc(
        crate::rust::interpreter::compiler::normalize::ProcVisitOutputs {
            par,
            free_map: normalized.free_map,
        },
    )))
}

fn flatten_nested_signature(mut par: Par, signature: CostSignature) -> (Par, CostSignature) {
    let solely_nested = par.sends.is_empty()
        && par.receives.is_empty()
        && par.news.is_empty()
        && par.exprs.is_empty()
        && par.matches.is_empty()
        && par.unforgeables.is_empty()
        && par.bundles.is_empty()
        && par.connectives.is_empty()
        && par.conditionals.is_empty()
        && par.cost_stacks.is_empty()
        && par.cost_signed_terms.len() == 1;
    if !solely_nested {
        return (par, signature);
    }
    let nested = par.cost_signed_terms.pop().unwrap();
    let compound = sort_signature(&CostSignature {
        value: Some(CostSignatureValue::Compound(CostSignatureCompound {
            elements: vec![signature, nested.signature.unwrap()],
        })),
    })
    .term;
    (nested.body.unwrap(), compound)
}

/// `s :: S` bare token stack at PROC level: resolve each layer's signature
/// (recognition / validation) and lower to the empty process. It mints NOTHING in
/// the normalizer — DR-13: only the Rust supply producer writes `Σ⟦s⟧`, and
/// emitting `Σ⟦s⟧!(…)` sends would add COMM nodes and break the `Δ_s == consumed`
/// equality. A signed deploy's fuel is its own funded `Σ⟦c⟧` balance (Workstream
/// C/D), not a per-program send (design §3.3 + BLOCKER-1). In the multi-deploy
/// re-scoping (Phase 5) a `s :: ()` stack is the deploy BEING signed by `s`; it
/// carries no in-program mint.
#[inline(never)]
pub(crate) fn descend_token_stack<'ast>(
    stack: &'ast TokenStack<'ast>,
    input: ProcVisitInputs,
) -> Step<'ast> {
    schedule_layer(&stack.layers, 0, input, Vec::new())
}

/// `s :: S`, combine half — one layer validated, on to the next.
#[inline(never)]
pub(crate) fn combine_token_stack<'ast>(
    layers: &'ast [Signature<'ast>],
    idx: usize,
    input: ProcVisitInputs,
    mut cells: Vec<CostSignature>,
    value: NormVal,
) -> Result<Step<'ast>, InterpreterError> {
    let (signature, free_map) = value.into_wire_sig();
    let mut input = input;
    input.par.locally_free = union(
        std::mem::take(&mut input.par.locally_free),
        wire_signature_locally_free(&signature),
    );
    input.free_map = free_map;
    cells.push(signature);
    Ok(schedule_layer(layers, idx + 1, input, cells))
}

/// Schedule layer `idx`, or finish the stack.
fn schedule_layer<'ast>(
    layers: &'ast [Signature<'ast>],
    idx: usize,
    input: ProcVisitInputs,
    cells: Vec<CostSignature>,
) -> Step<'ast> {
    match layers.get(idx) {
        Some(layer) => {
            let bound_map_chain = input.bound_map_chain.clone();
            Step::Descend {
                kont: NormKont::TokenStack {
                    layers,
                    idx,
                    input: input.clone(),
                    cells,
                },
                work: NormWork::WireSig {
                    sig: SigInput::Ref(layer),
                    bound_map_chain,
                    free_map: input.free_map,
                },
            }
        }
        None => {
            let mut par = input.par;
            if !cells.is_empty() {
                par.cost_stacks.push(CostStack { cells, parallel_tail: Vec::new() });
            }
            Step::Done(NormVal::Proc(
                crate::rust::interpreter::compiler::normalize::ProcVisitOutputs {
                    par,
                    free_map: input.free_map,
                },
            ))
        }
    }
}
