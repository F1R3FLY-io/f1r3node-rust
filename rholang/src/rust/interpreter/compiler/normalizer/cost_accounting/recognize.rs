//! Native recognition of cost-accounted surface syntax. Surface forms DECORATE;
//! they never re-emit metered operations (the reducer meters per-COMM). A signed
//! term resolves `s` to a native [`accounting::Sig`](crate::rust::interpreter::accounting::Sig)
//! (validating it) and lowers its inner `P` through the ORDINARY dispatch — it
//! synthesizes NO `for(t <- Σ⟦s⟧)` fuel gate, so the normalized `Par` of
//! `{% P %}[s]` has the SAME send/receive-node count as `P` alone (the
//! double-metering avoidance, design §3/§4 / MAJOR-2).
//!
//! The located-stack ATTRIBUTION is a metering-context concern realized at the
//! REDUCER, not here (Phase 3): `metering::note_channel_lane` matches each COMM's
//! resolved channel against the deploy's installed signer channels
//! ([`Sig::signer_channels`](crate::rust::interpreter::accounting::Sig::signer_channels))
//! and tallies a per-lane projection — the normalizer binds NOTHING (the `Par`
//! has no signature field — the s₀ collapse — and the normalizer has no signer
//! context; BLOCKER-1). So recognition here only VALIDATES the signature and
//! lowers `P`; under s₀ every COMM attributes to the deploy envelope (no COMM
//! lands on a `Σ⟦s⟧` supply channel — the §5 no-alias audit).

use std::collections::HashMap;

use models::rhoapi::Par;
use rholang_parser::ast::{AnnProc, Receipts, Signature, TokenStack};
use rholang_parser::{RholangParser, SourceSpan};

use super::desugar;
use super::sig::signature_to_native_sig;
use crate::rust::interpreter::compiler::normalize::{
    normalize_ann_proc, ProcVisitInputs, ProcVisitOutputs,
};
use crate::rust::interpreter::errors::InterpreterError;

/// `{% P %}[s]`: resolve `s` (recognition) + apply the lollipop / uniform-signing
/// AST rewrites, then lower the inner `P` through the ORDINARY dispatch. Emits NO
/// gate node.
pub fn recognize_signed_term<'ast>(
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
    signature_to_native_sig(core_sig, &input.bound_map_chain, env, parser)?;
    normalize_ann_proc(&core_inner, input, env, parser)
}

/// `s :: S` bare token stack at PROC level: resolve each layer's signature
/// (recognition / validation) and lower to the empty process. It mints NOTHING in
/// the normalizer — DR-13: only the Rust supply producer writes `Σ⟦s⟧`, and
/// emitting `Σ⟦s⟧!(…)` sends would add COMM nodes and break the `Δ_s == consumed`
/// equality. A signed deploy's fuel is its own funded `Σ⟦c⟧` balance (Workstream
/// C/D), not a per-program send (design §3.3 + BLOCKER-1). In the multi-deploy
/// re-scoping (Phase 5) a `s :: ()` stack is the deploy BEING signed by `s`; it
/// carries no in-program mint.
pub fn recognize_token_stack<'ast>(
    stack: &'ast TokenStack<'ast>,
    input: ProcVisitInputs,
    env: &HashMap<String, Par>,
    parser: &'ast RholangParser<'ast>,
) -> Result<ProcVisitOutputs, InterpreterError> {
    for layer in stack.layers.iter() {
        // Resolve each layer (rejects malformed sigs); the resolved value drives
        // Phase-3 attribution, here it is validation only.
        signature_to_native_sig(layer, &input.bound_map_chain, env, parser)?;
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
pub fn recognize_signed_join<'ast>(
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
        "recognize_signed_join is dispatched only when a Bind::Signed is present"
    );
    // VALIDATE each clause signature (rejects a wildcard `_`, a quoted-principal
    // `@P` ground sig, or a bare lollipop in fundable position) — the same
    // recognition `signature_to_native_sig` applies to a `{% P %}[s]` term. Phase
    // 3's channel match attributes each clause's rendezvous COMM to its signer lane
    // at the reducer; here recognition only validates + recovers the plain join.
    for sig in &clause_sigs {
        signature_to_native_sig(sig, &input.bound_map_chain, env, parser)?;
    }
    // Lower the recovered PLAIN join ordinarily: its binds are now linear, so it
    // re-normalizes through `normalize_p_input` with NO signed-bind recursion.
    normalize_ann_proc(&plain_for, input, env, parser)
}
