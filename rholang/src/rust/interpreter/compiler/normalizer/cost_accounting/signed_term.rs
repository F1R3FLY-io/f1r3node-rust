//! Signed-term lowering `T⟦{P}_s⟧` — the fuel gate.
//!
//! `T⟦{P}_s⟧ = for(t <- Σ⟦s⟧){ *t ∣ P⟦P⟧ }` (cost-accounted-rho §8): the gate
//! blocks until a token (a send on `Σ⟦s⟧`) arrives, one COMM consumes it and
//! binds `t` to the payload (the remaining balance), `*t` re-releases that
//! balance, and `P` runs — so `{P}_s` means "P runs only when `s` funds it"
//! (ownership/authority = the capability to mint a token on the unforgeable
//! `Σ⟦s⟧`). A compound `{P}_{s1*…*sn}` nests `n` gates over the component
//! atoms (Phase 1: funded by atomic component tokens; the combined-cell token
//! on `Σ⟦s1*…*sn⟧` is mediated by the Phase-2 splitter/joiner).
//!
//! `P⟦·⟧` is just [`normalize_ann_proc`] (the §8 `P` translation is the
//! identity on the host process); nested signed terms inside `P` are lowered
//! by the ordinary dispatch recursion. The continuation-signing sugars
//! (uniform, lollipop) are applied first by [`super::desugar`].

use std::collections::HashMap;

use models::create_bit_vector;
use models::rhoapi::{Par, Receive, ReceiveBind};
use models::rust::rholang::implicits::concatenate_pars;
use models::rust::utils::{new_boundvar_par, new_freevar_par};
use rholang_parser::ast::{AnnProc, Signature};
use rholang_parser::RholangParser;

use super::desugar;
use super::ir::Sig;
use super::sig::{signature_to_ir, supply_channel};
use crate::rust::interpreter::compiler::normalize::{
    normalize_ann_proc, ProcVisitInputs, ProcVisitOutputs, VarSort,
};
use crate::rust::interpreter::errors::InterpreterError;
use crate::rust::interpreter::util::filter_and_adjust_bitset;

/// Hygienic synthetic binder name for a fuel variable. It contains spaces, so
/// it can never collide with a source identifier: `P` never resolves it (the
/// `*t` re-release is hand-built at a fixed de Bruijn index), it only occupies
/// a bound-var slot so `P`'s references to *outer* names get the correct
/// de Bruijn offset.
const FUEL_BINDER: &str = " cost-accounting fuel ";

/// Lower `{inner}_sig`, dispatching the lollipop sugar before the core gates.
pub fn lower_signed_term<'ast>(
    inner: AnnProc<'ast>,
    sig: &Signature<'ast>,
    input: ProcVisitInputs,
    env: &HashMap<String, Par>,
    parser: &'ast RholangParser<'ast>,
) -> Result<ProcVisitOutputs, InterpreterError> {
    match sig {
        // Lollipop `s1 ⊸ s2`: re-sign the continuation with `s2`, then fund the
        // rendezvous with the outer `s1`.
        Signature::Transfer(s1, s2) => {
            let desugared = desugar::lollipop(inner, s2, parser)?;
            lower_core_signed_term(desugared, s1, input, env, parser)
        }
        core => lower_core_signed_term(inner, core, input, env, parser),
    }
}

/// Lower a `for(...)` whose receipts carry per-clause signed binds
/// `{% y <- x %}[s]` (Greg `app:concrete` SignedBind — the Axis-C join). Each
/// signed clause's signature funds its rendezvous, so we strip the binds back
/// to ordinary linear binds (recovering a plain `for`) and wrap that `for` in
/// one fuel gate per funding atom — reusing [`build_gates`]. The comm then
/// fires only once a token has been consumed on every `Σ⟦s_i⟧`, i.e. the join
/// is metered exactly as `{ for(R){P} }_{s_1 (*) … (*) s_k}` (the product of the
/// clause fuels). Crucially, the continuation `P` is NOT re-signed (we skip the
/// `uniform_sign` step a signed *term* would apply): a per-clause bind meters
/// the rendezvous, not the continuation. Signatures resolve in the `for`'s
/// ENCLOSING scope (binding-sensitive `Σ`: a `new`-bound clause sig
/// ring-fences; a free one shares the global rendezvous channel).
pub fn lower_signed_join<'ast>(
    receipts: &'ast rholang_parser::ast::Receipts<'ast>,
    body: AnnProc<'ast>,
    span: rholang_parser::SourceSpan,
    input: ProcVisitInputs,
    env: &HashMap<String, Par>,
    parser: &'ast RholangParser<'ast>,
) -> Result<ProcVisitOutputs, InterpreterError> {
    let (plain_for, sigs) = desugar::strip_signed_binds(receipts, body, span, parser);
    debug_assert!(
        !sigs.is_empty(),
        "lower_signed_join is dispatched only when a Bind::Signed is present"
    );

    // The funding atoms across all signed clauses, resolved in the ENCLOSING
    // scope (binding-sensitive `Σ⟦s⟧`). Concatenating the atoms gates the comm
    // by their product — the Axis-C join funded by `s_1 (*) … (*) s_k`.
    let mut atoms: Vec<Sig> = Vec::new();
    for sig in &sigs {
        atoms.extend(signature_to_ir(sig, &input.bound_map_chain, env, parser)?.atoms());
    }

    build_gates(&atoms, plain_for, input, env, parser)
}

/// Lower a signed term whose signature is *core* (ground / `#P` / compound — no
/// lollipop): apply uniform signing (sign a `for` continuation), then build the
/// nested fuel gates over the signature's funding atoms.
fn lower_core_signed_term<'ast>(
    inner: AnnProc<'ast>,
    sig: &Signature<'ast>,
    input: ProcVisitInputs,
    env: &HashMap<String, Par>,
    parser: &'ast RholangParser<'ast>,
) -> Result<ProcVisitOutputs, InterpreterError> {
    let metered_inner = desugar::uniform_sign(inner, sig, parser);
    // Binding-sensitive `Σ⟦s⟧`: resolve the signature in the gate's ENCLOSING
    // scope (`input.bound_map_chain`) — the same scope in which the funding token
    // is minted — so a `new`-bound sig ring-fences (token and gate agree on the
    // binder-keyed channel) while a free sig uses the shared global channel.
    let sig_ir = signature_to_ir(sig, &input.bound_map_chain, env, parser)?;
    build_gates(&sig_ir.atoms(), metered_inner, input, env, parser)
}

/// Build `n = atoms.len()` nested fuel gates around `P` (`metered_inner`):
/// `for(t0 <- Σ⟦a0⟧){ … for(t_{n-1} <- Σ⟦a_{n-1}⟧){ *t0 ∣ … ∣ *t_{n-1} ∣ P } }`.
fn build_gates<'ast>(
    atoms: &[Sig],
    metered_inner: AnnProc<'ast>,
    input: ProcVisitInputs,
    env: &HashMap<String, Par>,
    parser: &'ast RholangParser<'ast>,
) -> Result<ProcVisitOutputs, InterpreterError> {
    let n = atoms.len();
    debug_assert!(
        n >= 1,
        "Sig::atoms() always yields at least one funding atom"
    );

    // Body scope: bump the bound-var depth by `n` (one synthetic fuel binder
    // per gate) so `P`'s references to outer names get the +n de Bruijn offset.
    let mut body_chain = input.bound_map_chain.clone();
    for _ in 0..n {
        body_chain = body_chain.put_span((
            FUEL_BINDER.to_string(),
            VarSort::NameSort,
            metered_inner.span,
        ));
    }

    let body_out = normalize_ann_proc(
        &metered_inner,
        ProcVisitInputs {
            par: Par::default(),
            bound_map_chain: body_chain,
            free_map: input.free_map.clone(),
        },
        env,
        parser,
    )?;

    // Nest one fuel gate per atom, INNERMOST-first. Each gate releases its own
    // fuel `*t_j` (BoundVar 0 — the var it just bound) IN PARALLEL with the next
    // inner gate, so the token payload — which carries the *next* token — flows
    // on to that gate:
    //   for(t0<-Σ⟦a0⟧){ *t0 ∣ for(t1<-Σ⟦a1⟧){ *t1 ∣ … *t_{n-1} ∣ P } }
    // Bundling all `*t_j` at the innermost level instead would DEADLOCK: the
    // inner gate would block on a token still locked inside an outer fuel var
    // that only `*t` (sitting inside that same inner gate) would release.
    let mut current = body_out.par; // innermost continuation: P
    let mut outermost: Option<Receive> = None;
    for atom in atoms.iter().rev() {
        let star = new_boundvar_par(0, create_bit_vector(&[0]), false);
        let gate_body = concatenate_pars(star, current);
        let receive = Receive {
            binds: vec![ReceiveBind {
                patterns: vec![new_freevar_par(0, Vec::new())],
                source: Some(supply_channel(atom)),
                remainder: None,
                free_count: 1,
            }],
            body: Some(gate_body.clone()),
            persistent: false,
            peek: false,
            bind_count: 1,
            locally_free: filter_and_adjust_bitset(gate_body.locally_free.clone(), 1),
            connective_used: gate_body.connective_used,
            condition: None,
        };
        current = Par::default().prepend_receive(receive.clone());
        outermost = Some(receive);
    }

    let par = input
        .par
        .clone()
        .prepend_receive(outermost.expect("n >= 1 ensures at least one gate"));

    Ok(ProcVisitOutputs {
        par,
        free_map: body_out.free_map,
    })
}
