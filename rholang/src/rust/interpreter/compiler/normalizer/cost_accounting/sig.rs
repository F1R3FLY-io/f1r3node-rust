use std::collections::HashMap;

use models::rhoapi::cost_signature::Value as CostSignatureValue;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{CostSignature, CostSignatureCompound, Expr, Par};
use models::rust::rholang::sorter::cost_accounting_sorter::sort_signature;
use models::rust::rholang::sorter::par_sort_matcher::ParSortMatcher;
use models::rust::rholang::sorter::sortable::Sortable;
use models::rust::utils::union;
use prost::Message;
use rholang_parser::ast::{AnnProc, Name, Signature, Var};
use rholang_parser::{RholangParser, SourceSpan};

use super::ir::Sig;
use crate::rust::interpreter::accounting::{Sig as NativeSig, SignatureChannel};
use crate::rust::interpreter::compiler::bound_map_chain::BoundMapChain;
use crate::rust::interpreter::compiler::free_map::FreeMap;
use crate::rust::interpreter::compiler::normalize::{ProcVisitInputs, VarSort};
use crate::rust::interpreter::compiler::normalize_drive::{
    norm_drive_from, NormKont, NormVal, NormWork, SigInput, Step,
};
use crate::rust::interpreter::errors::InterpreterError;

/// Lower a surface [`Signature`] to the [`Sig`] IR.
///
/// * `Ground(g)` — `g` must be a bare identifier (v1); a wildcard `_` or a
///   quoted-principal `@P` ground sig is rejected (the latter is the bounded
///   future `@(P)` extension, symmetric with `#(P)`).
/// * `Hash(#P)` — the quote principal; `P` is canonicalized depth-independently.
/// * `Compound(s₁ * s₂)` — flattened + key-sorted via [`Sig::compound`].
/// * `Transfer(s₁ ⊸ s₂)` — the lollipop is *term-level* sugar
///   ([`super::desugar`]); it must never reach a fundable position, so it is
///   rejected here (a lollipop is a transfer capability, not a fundable atom).
/// Lower a surface [`Signature`] to the [`Sig`] IR, descend half.
///
/// * `Ground(g)` — `g` must be a bare identifier (v1); a wildcard `_` or a
///   quoted-principal `@P` ground sig is rejected (the latter is the bounded
///   future `@(P)` extension, symmetric with `#(P)`). A leaf.
/// * `Hash(#P)` — the quote principal; `P` is canonicalized depth-independently,
///   which re-enters the process normalizer, so it is a machine child.
/// * `Compound(s₁ * s₂)` — two children, flattened + key-sorted via
///   [`Sig::compound`] once both are in.
/// * `Transfer(s₁ ⊸ s₂)` — the lollipop is *term-level* sugar
///   ([`super::desugar`]); it must never reach a fundable position, so it is
///   rejected here (a lollipop is a transfer capability, not a fundable atom).
///
/// ⚠ `SigInput` distinguishes a signature borrowed from the parser arena from
/// one the caller already owns, so a `Compound` can be taken apart by **moving**
/// out of its boxes in the owned case and by re-borrowing in the arena case.
/// Cloning instead would introduce a Θ(depth) derived `Clone` over
/// `Box<Signature>` — trading one recursive traversal for another.
#[inline(never)]
pub(crate) fn descend_sig<'ast>(
    sig: SigInput<'ast>,
    bound_map_chain: BoundMapChain<VarSort>,
    _env: &HashMap<String, Par>,
) -> Result<Step<'ast>, InterpreterError> {
    macro_rules! ground {
        ($name:expr) => {
            match $name {
                // Binding-sensitive `Σ⟦g⟧`: a ground sig that resolves to a
                // `new`/`for`-binder is RING-FENCED (keyed on the binder's stable
                // identity); a FREE sig is content-by-spelling (one global channel).
                Name::NameVar(Var::Id(id)) => match bound_map_chain.get(id.name) {
                    Some(ctx) => Ok(Step::Done(NormVal::Sig(Sig::Bound(canon_bound(
                        &ctx.source_span,
                    ))))),
                    None => Ok(Step::Done(NormVal::Sig(Sig::Ground(canon_ground(id.name))))),
                },
                Name::NameVar(Var::Wildcard) => Err(InterpreterError::NormalizerError(
                    "cost-accounting: a wildcard `_` is not a valid ground signature".to_string(),
                )),
                Name::Quote(_) => Err(InterpreterError::NormalizerError(
                    "cost-accounting: a quoted-principal ground signature `@P` is not supported in \
                     v1 (use a section signature `# P` for code-hash principals)"
                        .to_string(),
                )),
            }
        };
    }
    const TRANSFER_ERR: &str =
        "cost-accounting: a lollipop `-o` (transfer) signature is term-level sugar and must be \
         desugared before lowering; it cannot fund a term directly";

    match sig {
        SigInput::Ref(sig) => match sig {
            Signature::Ground(name) => ground!(*name),
            Signature::Hash(proc) => Ok(Step::Descend {
                kont: NormKont::SigHash,
                work: NormWork::CanonQuote { proc: *proc },
            }),
            Signature::Compound(left, right) => Ok(Step::Descend {
                kont: NormKont::SigCompound {
                    right: Some(SigInput::Ref(right)),
                    bound_map_chain: bound_map_chain.clone(),
                    left: None,
                },
                work: NormWork::Sig {
                    sig: SigInput::Ref(left),
                    bound_map_chain,
                },
            }),
            Signature::Transfer(_, _) => {
                Err(InterpreterError::NormalizerError(TRANSFER_ERR.to_string()))
            }
        },
        // Changed by the merge of feature/cost-accounted-rho (clippy gate): no
        // code constructs `SigInput::Own` (see `SigInput` in `normalize_drive.rs`).
        // SigInput::Own(sig) => match sig {
        //     Signature::Ground(name) => ground!(name),
        //     Signature::Hash(proc) => Ok(Step::Descend {
        //         kont: NormKont::SigHash,
        //         work: NormWork::CanonQuote { proc },
        //     }),
        //     Signature::Compound(left, right) => Ok(Step::Descend {
        //         kont: NormKont::SigCompound {
        //             right: Some(SigInput::Own(*right)),
        //             bound_map_chain: bound_map_chain.clone(),
        //             left: None,
        //         },
        //         work: NormWork::Sig {
        //             sig: SigInput::Own(*left),
        //             bound_map_chain,
        //         },
        //     }),
        //     Signature::Transfer(_, _) => {
        //         Err(InterpreterError::NormalizerError(TRANSFER_ERR.to_string()))
        //     }
        // },
    }
}

/// `#P`, combine half — wrap the canonical bytes as a quote principal.
#[inline(never)]
pub(crate) fn combine_sig_hash<'ast>(value: NormVal) -> Result<Step<'ast>, InterpreterError> {
    Ok(Step::Done(NormVal::Sig(Sig::Quote(value.into_bytes()))))
}

/// `s₁ * s₂`, combine half.
#[inline(never)]
pub(crate) fn combine_sig_compound<'ast>(
    right: Option<SigInput<'ast>>,
    bound_map_chain: BoundMapChain<VarSort>,
    left: Option<Sig>,
    value: NormVal,
) -> Result<Step<'ast>, InterpreterError> {
    match left {
        None => Ok(Step::Descend {
            kont: NormKont::SigCompound {
                // The right operand has been scheduled; the slot is emptied so
                // the machine cannot enter a state where it is scheduled twice.
                right: None,
                bound_map_chain: bound_map_chain.clone(),
                left: Some(value.into_sig()),
            },
            work: NormWork::Sig {
                sig: right.expect("combine_sig_compound: the right operand is scheduled once"),
                bound_map_chain,
            },
        }),
        Some(left_ir) => Ok(Step::Done(NormVal::Sig(Sig::compound(vec![
            left_ir,
            value.into_sig(),
        ])))),
    }
}

/// Lower a surface [`Signature`] to the [`Sig`] IR, on a **fresh** drive.
pub fn signature_to_ir<'ast>(
    sig: &'ast Signature<'ast>,
    bound_map_chain: &BoundMapChain<VarSort>,
    env: &HashMap<String, Par>,
    parser: &'ast RholangParser<'ast>,
) -> Result<Sig, InterpreterError> {
    norm_drive_from(
        Step::Tail(NormWork::Sig {
            sig: SigInput::Ref(sig),
            bound_map_chain: bound_map_chain.clone(),
        }),
        env,
        parser,
    )
    .map(NormVal::into_sig)
}

/// Resolve a surface [`Signature`] straight to the native funding
/// [`accounting::Sig`](crate::rust::interpreter::accounting::Sig) — the
/// recognition + IR-bridge composite. Validates the sig (via
/// [`signature_to_ir`], which rejects a wildcard / `@P` ground / a bare lollipop
/// in fundable position) and bridges the IR to the consensus algebra via
/// [`Sig::to_native`]. The native `from_sig` of the result is the supply channel
/// `Σ⟦s⟧`; W1 NEVER derives a separate channel (design §3.1).
pub fn signature_to_native_sig<'ast>(
    sig: &'ast Signature<'ast>,
    bound_map_chain: &BoundMapChain<VarSort>,
    env: &HashMap<String, Par>,
    parser: &'ast RholangParser<'ast>,
) -> Result<NativeSig, InterpreterError> {
    Ok(signature_to_ir(sig, bound_map_chain, env, parser)?.to_native())
}

/// Resolve a surface [`Signature`] to its native supply channel `Σ⟦s⟧` — the
/// consensus-anchored basis via [`SignatureChannel::from_sig`] (design §3.1:
/// native `from_sig` wins; surface forms DECORATE). This is the channel a
/// per-redex COMM is matched against for located-stack attribution (Phase 3,
/// P14). It is binding-sensitive through [`signature_to_native_sig`] (a
/// `new`-bound sig ring-fences to a `DOMAIN_BOUND`-keyed channel), so a free `g`
/// and a `new`-bound `g` derive DISTINCT channels, while two free `g` (any deploy)
/// derive the SAME channel — the §9 rendezvous.
pub fn signature_to_channel<'ast>(
    sig: &'ast Signature<'ast>,
    bound_map_chain: &BoundMapChain<VarSort>,
    env: &HashMap<String, Par>,
    parser: &'ast RholangParser<'ast>,
) -> Result<Par, InterpreterError> {
    Ok(
        SignatureChannel::from_sig(&signature_to_native_sig(sig, bound_map_chain, env, parser)?)
            .par,
    )
}

pub enum ResolvedSignatureAtom {
    Ground(Vec<u8>),
    BoundLevel(i32),
    Quote(Par),
    Unit,
}

pub enum ResolvedSignatureEvent {
    Atom(ResolvedSignatureAtom),
    Compound(usize),
}

pub struct LoweredSignature {
    pub signature: CostSignature,
    pub locally_free: Vec<u8>,
    pub connective_used: bool,
}

fn combine_wire_signatures(elements: Vec<CostSignature>) -> CostSignature {
    sort_signature(&CostSignature {
        value: Some(CostSignatureValue::Compound(CostSignatureCompound {
            elements,
        })),
    })
    .term
}

pub fn lower_resolved_signature(
    events: impl IntoIterator<Item = ResolvedSignatureEvent>,
) -> Result<LoweredSignature, InterpreterError> {
    let mut stack = Vec::new();
    for event in events {
        match event {
            ResolvedSignatureEvent::Atom(atom) => {
                let value = match atom {
                    ResolvedSignatureAtom::Ground(bytes) => CostSignatureValue::Ground(bytes),
                    ResolvedSignatureAtom::BoundLevel(level) if level >= 0 => {
                        CostSignatureValue::BoundLevel(level)
                    }
                    ResolvedSignatureAtom::BoundLevel(_) => {
                        return Err(InterpreterError::NormalizerError(
                            "cost-accounting: negative bound signature level".to_string(),
                        ));
                    }
                    ResolvedSignatureAtom::Quote(par) => CostSignatureValue::Quote(par),
                    ResolvedSignatureAtom::Unit => CostSignatureValue::Unit(true),
                };
                stack.push(CostSignature { value: Some(value) });
            }
            ResolvedSignatureEvent::Compound(arity) => {
                if arity < 2 || stack.len() < arity {
                    return Err(InterpreterError::NormalizerError(
                        "cost-accounting: malformed compound signature".to_string(),
                    ));
                }
                let elements = stack.split_off(stack.len() - arity);
                stack.push(combine_wire_signatures(elements));
            }
        }
    }
    if stack.len() != 1 {
        return Err(InterpreterError::NormalizerError(
            "cost-accounting: malformed signature event sequence".to_string(),
        ));
    }
    let signature = stack.pop().expect("one signature remains");
    Ok(LoweredSignature {
        locally_free: wire_signature_locally_free(&signature),
        connective_used: wire_signature_connective_used(&signature),
        signature,
    })
}

pub fn wire_signature_locally_free(signature: &CostSignature) -> Vec<u8> {
    let mut result = Vec::new();
    let mut work = vec![signature];
    while let Some(current) = work.pop() {
        match &current.value {
            Some(CostSignatureValue::BoundLevel(level)) if *level >= 0 => {
                result = union(result, models::create_bit_vector(&[*level as usize]));
            }
            Some(CostSignatureValue::Quote(par)) | Some(CostSignatureValue::Name(par)) => {
                result = union(result, par.locally_free.clone());
            }
            Some(CostSignatureValue::Compound(compound)) => {
                work.extend(compound.elements.iter().rev());
            }
            _ => {}
        }
    }
    result
}

pub fn wire_signature_connective_used(signature: &CostSignature) -> bool {
    let mut work = vec![signature];
    while let Some(current) = work.pop() {
        match &current.value {
            Some(CostSignatureValue::Quote(par)) | Some(CostSignatureValue::Name(par)) => {
                if par.connective_used {
                    return true;
                }
            }
            Some(CostSignatureValue::Compound(compound)) => {
                work.extend(compound.elements.iter().rev());
            }
            _ => {}
        }
    }
    false
}

pub(crate) fn descend_wire_sig<'ast>(
    sig: SigInput<'ast>,
    bound_map_chain: BoundMapChain<VarSort>,
    free_map: FreeMap<VarSort>,
) -> Result<Step<'ast>, InterpreterError> {
    let ground = |name: Name<'ast>,
                  free_map: FreeMap<VarSort>|
     -> Result<Step<'ast>, InterpreterError> {
        let signature = match name {
            Name::NameVar(Var::Id(id)) => match bound_map_chain.get(id.name) {
                Some(ctx) if ctx.typ == VarSort::NameSort => CostSignature {
                    value: Some(CostSignatureValue::BoundLevel(ctx.index as i32)),
                },
                Some(ctx) => {
                    return Err(InterpreterError::UnexpectedNameContext {
                        var_name: id.name.to_string(),
                        proc_var_source_span: ctx.source_span,
                        name_source_span: SourceSpan {
                            start: id.pos,
                            end: id.pos,
                        },
                    });
                }
                None => CostSignature {
                    value: Some(CostSignatureValue::Ground(canon_ground(id.name))),
                },
            },
            Name::NameVar(Var::Wildcard) => {
                return Err(InterpreterError::NormalizerError(
                    "cost-accounting: a wildcard `_` is not a valid ground signature".to_string(),
                ));
            }
            Name::Quote(_) => {
                return Err(InterpreterError::NormalizerError(
                    "cost-accounting: a quoted-principal ground signature `@P` is not supported"
                        .to_string(),
                ));
            }
        };
        Ok(Step::Done(NormVal::WireSig(signature, free_map)))
    };

    match sig {
        SigInput::Ref(signature) => match signature {
            Signature::Ground(name) => ground(*name, free_map),
            Signature::Hash(proc) => Ok(Step::Descend {
                kont: NormKont::WireSigHash,
                work: NormWork::Proc {
                    proc: *proc,
                    input: ProcVisitInputs {
                        par: Par::default(),
                        bound_map_chain,
                        free_map,
                    },
                },
            }),
            Signature::Compound(left, right) => Ok(Step::Descend {
                kont: NormKont::WireSigCompound {
                    right: Some(SigInput::Ref(right)),
                    bound_map_chain: bound_map_chain.clone(),
                    left: None,
                },
                work: NormWork::WireSig {
                    sig: SigInput::Ref(left),
                    bound_map_chain,
                    free_map,
                },
            }),
            Signature::Transfer(_, _) => Err(InterpreterError::NormalizerError(
                "cost-accounting: a lollipop `-o` cannot appear in a fundable signature"
                    .to_string(),
            )),
        },
        // Changed by the merge of feature/cost-accounted-rho (clippy gate): no
        // code constructs `SigInput::Own` (see `SigInput` in `normalize_drive.rs`).
        // SigInput::Own(signature) => match signature {
        //     Signature::Ground(name) => ground(name, free_map),
        //     Signature::Hash(proc) => Ok(Step::Descend {
        //         kont: NormKont::WireSigHash,
        //         work: NormWork::Proc {
        //             proc,
        //             input: ProcVisitInputs {
        //                 par: Par::default(),
        //                 bound_map_chain,
        //                 free_map,
        //             },
        //         },
        //     }),
        //     Signature::Compound(left, right) => Ok(Step::Descend {
        //         kont: NormKont::WireSigCompound {
        //             right: Some(SigInput::Own(*right)),
        //             bound_map_chain: bound_map_chain.clone(),
        //             left: None,
        //         },
        //         work: NormWork::WireSig {
        //             sig: SigInput::Own(*left),
        //             bound_map_chain,
        //             free_map,
        //         },
        //     }),
        //     Signature::Transfer(_, _) => Err(InterpreterError::NormalizerError(
        //         "cost-accounting: a lollipop `-o` cannot appear in a fundable signature"
        //             .to_string(),
        //     )),
        // },
    }
}

pub(crate) fn combine_wire_sig_hash<'ast>(value: NormVal) -> Result<Step<'ast>, InterpreterError> {
    let normalized = value.into_proc();
    let signature = CostSignature {
        value: Some(CostSignatureValue::Quote(
            ParSortMatcher::sort_match(&normalized.par).term,
        )),
    };
    Ok(Step::Done(NormVal::WireSig(signature, normalized.free_map)))
}

pub(crate) fn combine_wire_sig_compound<'ast>(
    right: Option<SigInput<'ast>>,
    bound_map_chain: BoundMapChain<VarSort>,
    left: Option<CostSignature>,
    value: NormVal,
) -> Result<Step<'ast>, InterpreterError> {
    let (signature, free_map) = value.into_wire_sig();
    match left {
        None => Ok(Step::Descend {
            kont: NormKont::WireSigCompound {
                right: None,
                bound_map_chain: bound_map_chain.clone(),
                left: Some(signature),
            },
            work: NormWork::WireSig {
                sig: right.expect("wire signature right child exists"),
                bound_map_chain,
                free_map,
            },
        }),
        Some(left) => Ok(Step::Done(NormVal::WireSig(
            combine_wire_signatures(vec![left, signature]),
            free_map,
        ))),
    }
}

pub fn signature_to_wire<'ast>(
    sig: &'ast Signature<'ast>,
    bound_map_chain: &BoundMapChain<VarSort>,
    free_map: FreeMap<VarSort>,
    env: &HashMap<String, Par>,
    parser: &'ast RholangParser<'ast>,
) -> Result<(CostSignature, FreeMap<VarSort>), InterpreterError> {
    norm_drive_from(
        Step::Tail(NormWork::WireSig {
            sig: SigInput::Ref(sig),
            bound_map_chain: bound_map_chain.clone(),
            free_map,
        }),
        env,
        parser,
    )
    .map(NormVal::into_wire_sig)
}

/// Stable source-span identity for the legacy target IR's ring-fenced bound
/// signatures. Production cost-signature lowering uses `WireSig` instead.
pub fn canon_bound(span: &SourceSpan) -> Vec<u8> {
    format!(
        "{}:{}-{}:{}",
        span.start.line, span.start.col, span.end.line, span.end.col
    )
    .into_bytes()
}

pub fn canon_ground(name: &str) -> Vec<u8> {
    let par = Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GString(name.to_string())),
    }]);
    ParSortMatcher::sort_match(&par).term.encode_to_vec()
}

/// Canonical bytes for a quote principal `#P`: the wire encoding of `𝒫⟦P⟧`,
/// normalized **standalone at de Bruijn depth 0** (a fresh [`ProcVisitInputs`]).
/// Normalizing at a fixed depth makes the encoding binder-depth-independent and
/// α-invariant — a `#P` that references an outer bound name hashes the same
/// wherever it appears. `FN_s(#P) = FN(P)`: free names are part of the principal's
/// identity (paper §3), so they are not rejected.
/// Canonical bytes for a quote principal `#P`: the wire encoding of `𝒫⟦P⟧`,
/// normalized **standalone at de Bruijn depth 0** (a fresh [`ProcVisitInputs`]).
/// Normalizing at a fixed depth makes the encoding binder-depth-independent and
/// α-invariant — a `#P` that references an outer bound name hashes the same
/// wherever it appears. `FN_s(#P) = FN(P)`: free names are part of the principal's
/// identity (paper §3), so they are not rejected.
///
/// Descend half: the standalone normalization is a machine child, so a `#`
/// signature nested inside a deep term costs no native stack.
#[inline(never)]
pub(crate) fn descend_canon_quote<'ast>(
    proc: AnnProc<'ast>,
) -> Result<Step<'ast>, InterpreterError> {
    Ok(Step::Descend {
        kont: NormKont::CanonQuote,
        work: NormWork::Proc {
            proc,
            input: ProcVisitInputs::new(),
        },
    })
}

/// `canon_quote`, combine half — sort-canonicalise and encode.
#[inline(never)]
pub(crate) fn combine_canon_quote<'ast>(value: NormVal) -> Result<Step<'ast>, InterpreterError> {
    let normalized = value.into_proc();
    Ok(Step::Done(NormVal::Bytes(
        ParSortMatcher::sort_match(&normalized.par)
            .term
            .encode_to_vec(),
    )))
}

/// `canon_quote` on a **fresh** drive.
pub fn canon_quote<'ast>(
    proc: &AnnProc<'ast>,
    env: &HashMap<String, Par>,
    parser: &'ast RholangParser<'ast>,
) -> Result<Vec<u8>, InterpreterError> {
    norm_drive_from(
        Step::Tail(NormWork::CanonQuote { proc: *proc }),
        env,
        parser,
    )
    .map(NormVal::into_bytes)
}
