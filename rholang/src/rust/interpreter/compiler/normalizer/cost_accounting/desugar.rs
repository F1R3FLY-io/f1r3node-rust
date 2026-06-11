//! Term-level desugaring of the paper sugars to core signed terms, performed
//! lazily as each signed term is lowered (Split Phase: *desugar → lower*).
//! Both rewrites are AST→AST (allocated in the parser's arena, lifetime
//! `'ast`), then re-normalized — so the inner gate is produced by the ordinary
//! dispatch recursion, not duplicated here.
//!
//! * **Uniform signing** `{ for(R){P} }_s  →  { for(R){ {P}_s } }_s` — a signed
//!   `for` signs its continuation with the *same* signature, so every running
//!   continuation is metered. Applied only when the continuation is not
//!   already a signed term, so it composes with lollipop without double-signing.
//! * **Lollipop transfer** `{ for(R){P} }_{s1 ⊸ s2}  →  { for(R){ {P}_{s2} } }_{s1}`
//!   — the rendezvous is funded by `s1`, the continuation re-signed with `s2`
//!   (ownership/authority `s1` → `s2`). Joint ownership is the compound `s1*s2`.

use rholang_parser::ast::{AnnProc, Bind, Proc, Receipts, Signature};
use rholang_parser::RholangParser;

use crate::rust::interpreter::errors::InterpreterError;

/// Rebuild a `for`-comprehension, preserving its receipts (binds + `where`
/// guards) and replacing only the continuation body.
fn rebuild_for<'ast>(
    receipts: &'ast rholang_parser::ast::Receipts<'ast>,
    new_body: AnnProc<'ast>,
    span: rholang_parser::SourceSpan,
    parser: &'ast RholangParser<'ast>,
) -> AnnProc<'ast> {
    parser
        .ast_builder()
        .alloc_for_with_guards(
            receipts.iter().map(|r| (r.binds.iter().cloned(), r.guard)),
            new_body,
        )
        .ann(span)
}

/// Apply uniform signing to a signed term's inner process. If `inner` is a
/// `for` whose continuation is not yet a signed term, wrap that continuation
/// in `{·}_sig`; otherwise return `inner` unchanged (idempotent, and a no-op
/// once lollipop has already re-signed the continuation).
pub fn uniform_sign<'ast>(
    inner: AnnProc<'ast>,
    sig: &Signature<'ast>,
    parser: &'ast RholangParser<'ast>,
) -> AnnProc<'ast> {
    if let Proc::ForComprehension { receipts, proc } = inner.proc {
        if !matches!(proc.proc, Proc::SignedTerm { .. }) {
            let signed_body = parser
                .ast_builder()
                .alloc_signed_term(*proc, sig.clone())
                .ann(proc.span);
            return rebuild_for(receipts, signed_body, inner.span, parser);
        }
    }
    inner
}

/// Desugar the comm under a lollipop-signed term `{ for(R){P} }_{s1 ⊸ s2}`,
/// returning the rewritten inner `for(R){ {P}_{s2} }`. The caller signs the
/// result with the outer `s1` (so `s1` funds the rendezvous, `s2` the
/// continuation). The lollipop requires a comm (`for`); anything else is a
/// type error (a transfer capability has nothing to transfer through).
pub fn lollipop<'ast>(
    inner: AnnProc<'ast>,
    continuation_sig: &Signature<'ast>,
    parser: &'ast RholangParser<'ast>,
) -> Result<AnnProc<'ast>, InterpreterError> {
    match inner.proc {
        Proc::ForComprehension { receipts, proc } => {
            let signed_body = parser
                .ast_builder()
                .alloc_signed_term(*proc, continuation_sig.clone())
                .ann(proc.span);
            Ok(rebuild_for(receipts, signed_body, inner.span, parser))
        }
        _ => Err(InterpreterError::NormalizerError(
            "cost-accounting: a lollipop `-o` (transfer) signature requires a comm (`for`) as its \
             signed term — it transfers the authority funding the rendezvous to the continuation"
                .to_string(),
        )),
    }
}

/// Strip the per-clause signed binds `{% y <- x %}[s]` (Greg `app:concrete`
/// SignedBind) out of a `for`'s receipts, rebuilding an ordinary `for` with
/// plain *linear* binds and collecting the stripped signatures in clause order.
/// Plain binds and `where` guards are preserved verbatim. The caller meters the
/// recovered `for` with one fuel gate per collected signature atom (the Axis-C
/// join — see [`super::signed_term::lower_signed_join`]); because the binds
/// become linear, the recovered `for` re-normalizes through the ordinary
/// `for`-dispatch (no signed-bind recursion), so this never loops.
pub fn strip_signed_binds<'ast>(
    receipts: &'ast Receipts<'ast>,
    body: AnnProc<'ast>,
    span: rholang_parser::SourceSpan,
    parser: &'ast RholangParser<'ast>,
) -> (AnnProc<'ast>, Vec<Signature<'ast>>) {
    // Clause signatures, in source order (left-to-right across receipts/binds).
    let mut sigs: Vec<Signature<'ast>> = Vec::new();
    for receipt in receipts.iter() {
        for bind in receipt.binds.iter() {
            if let Bind::Signed { sig, .. } = bind {
                sigs.push(sig.clone());
            }
        }
    }

    // Rebuild the comprehension with every `Signed` clause demoted to a plain
    // `Linear` bind (same names + source); other binds and guards are cloned
    // through unchanged. Mirrors `rebuild_for`, but rewrites the binds rather
    // than the continuation.
    let plain = parser
        .ast_builder()
        .alloc_for_with_guards(
            receipts.iter().map(|receipt| {
                (
                    receipt.binds.iter().map(|bind| match bind {
                        Bind::Signed { lhs, rhs, .. } => Bind::Linear {
                            lhs: lhs.clone(),
                            rhs: rhs.clone(),
                        },
                        other => other.clone(),
                    }),
                    receipt.guard,
                )
            }),
            body,
        )
        .ann(span);

    (plain, sigs)
}
