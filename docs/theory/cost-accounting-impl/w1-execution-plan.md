# W1 Execution Plan — Cost-Accounted Surface Syntax → Native Runtime

Status: GRANULAR EXECUTION PLAN (implementation, not research). Branch:
`feature/cost-accounted-rho`. Consensus-touching. Dev `[patch]` already wired
(Phase 0 complete — the parser worktree builds).

> Companion to the design `w1-surface-syntax-native-integration.md` (READ IT
> FIRST). This document is the per-phase edit-by-edit sequence + a verification
> gate per phase. It does NOT re-derive the design; it operationalizes it.
> Build flag is MANDATORY for every command below (the patched parser pulls
> `gxhash` → AES/SSE2 intrinsics): `RUSTFLAGS="-C target-cpu=native"`.

## 0. Ground truth verified against the actual trees (do not re-check)

Confirmed by reading both worktrees on 2026-06-15:

- **Phase 0 is DONE.** `Cargo.toml` (workspace root) lines 67-70 carry the
  `[patch."https://github.com/F1R3FLY-io/rholang-rs"]` block pointing
  `rholang-parser` / `rholang-tree-sitter` / `rholang-tree-sitter-proc-macro` at
  `../rholang-rs-cost-accounting-transpiler/`. `rholang/Cargo.toml:51` still pins
  `rev = "c163755"` (the published pin — the only mergeable state). `Cargo.lock`
  is held out of git via `git update-index --skip-worktree`.
- **Patched parser AST (sibling worktree
  `rholang-rs-cost-accounting-transpiler/rholang-parser/src/ast.rs`):**
  - `Proc::SignedTerm { proc: AnnProc<'ast>, sig: Signature<'ast> }` (ast.rs:132).
    NOTE: `proc` is an **owned `AnnProc`**, NOT `&AnnProc` — in `match &proc.proc`
    the arm binds `proc: &AnnProc<'ast>`, `sig: &Signature<'ast>`.
  - `Proc::TokenStack { stack: TokenStack<'ast> }` (ast.rs:141); `TokenStack {
    layers: SmallVec<[Signature<'ast>; 2]> }` (ast.rs:452), innermost-first, ≥1
    layer at proc level (a bare `()` is `Proc::Unit`).
  - `Signature<'ast> = { Ground(Name), Hash(AnnProc), Compound(Box,Box),
    Transfer(Box,Box) }` (ast.rs:441).
  - `Bind::Signed { lhs: Names, rhs: Source, sig: Signature }` (ast.rs:654).
  - The parser's `traverse.rs` DFS descends into signed-term bodies + signatures
    (traverse.rs:153/417) and `iter_preorder_dfs` is available — so the ported
    `pattern_guard.rs` works verbatim. `ast_builder` exposes `alloc_signed_term`
    / `alloc_for_with_guards` (used by the ported `desugar.rs`).
- **The 3 exhaustive-match break sites on THIS branch (post-patch):**
  1. `rholang/src/rust/interpreter/compiler/normalize.rs` — the `match &proc.proc`
     (head at the current `:139`; catch-all body runs `Proc::Nil … Proc::Bad` at
     `:435`). The `Proc::ForComprehension` arm is at `:401`. NEW arms for
     `Proc::SignedTerm` / `Proc::TokenStack` go just before `Proc::Bad`.
  2. `p_input_normalizer.rs:242` — the `match receipt { Bind::Linear | Repeated |
     Peek }` in the simple-source mapping (lines 242-269). Add `Bind::Signed`.
  3. `p_input_normalizer.rs:275` — the `match head_receipt { Bind::Linear |
     Repeated | Peek }` deciding `(persistent, peek)` (lines 275-279). Add
     `Bind::Signed`. (There is ALSO a `match bind` at `:93` in the complex-source
     fold whose non-Linear arm is a catch-all `_ => Err(BugFound)`; `Bind::Signed`
     falls into it harmlessly. The two enumerated matches at `:242`/`:275` are the
     hard compile errors.)
  - `p_contr_normalizer.rs` / `p_match_normalizer.rs` did NOT break (their loops
    iterate `formals.names` / `cases`, never `match` a `Bind` exhaustively), so the
    `pattern_guard` there is an ADDITIVE SECURITY step, not a compile fix.
- **`Compiler::source_to_adt(source: &str)` has NO signer context** (compiler.rs:17).
  The normalizer therefore CANNOT validate "in-term `s` ∈ deploy signers" — it can
  only RECOGNIZE `s` and mark the region. Signer-membership validation +
  attribution happen at the reducer/gate, where the signer `Sig` is installed
  from `Cosigned` (`casper/.../rholang/runtime.rs::evaluate_cosigned` →
  `RuntimeBudget::set_deploy_signature_funded(wire_sig, funding_sig)` /
  `set_deploy_signatures_funded(...)`, `accounting/mod.rs`). Post-§D2.9 the install
  is SPLIT: `deploy_id` comes from `wire_sig` (the per-deploy wire signature), while
  `self.signature` / the signer channels + supply key come from
  `funding_sig = Sig::Ground(pk)` (single) / the `And`-fold of `Ground(pkᵢ)` (multi)
  — the signer's genesis-seeded wallet `Σ⟦Ground(pk)⟧`, so `Σ⟦signer⟧ == Σ⟦wallet⟧`
  (cross-ref `wd-d2-acceptance-gate.md` §D2.9 + the forthcoming `d2-9-funding-flow.md`).
  The legacy `set_deploy_signature(s)` are now thin wrappers passing the wire-sig
  `envelope_sig*` for byte-identical test/bench callers. **This split shapes Phase 3
  (riskiest).**
- **`metering.rs`:** `MeteredMachine::new` captures `sig_hash =
  budget.signature().lane_hash()` ONCE (`:57`); `child()` inherits it (`:84`);
  `reserve_comm` (`:93`) → `reserve_cost` (`:128`) stamps that one `sig_hash` on
  every event. `reduce.rs::eval_send` calls `reserve_comm` at the TOP (`:1005`)
  BEFORE channel resolution (`eval_chan`/`sub_chan` at `:1006-1007`);
  `eval_receive` likewise (`:1051`, before `unbundle_receive` at `:1071`).
- **`delta_sigma::demand(par, deploy_sig)`** ignores `deploy_sig` (`let _ =
  deploy_sig`) and counts every send/receive node to the single envelope (s₀
  collapse). `SigKey = [u8;32] = Sig::lane_hash`. `effective_supply_with` +
  `Decomposition` already realize Split/Join. `is_funded` = Def 19 / Thm 20.
- **`accounting/mod.rs`:** `Sig` has NO `Bound` variant; `from_sig` puts NO domain
  separator (`Sig::Ground(b) | Sig::Quote(b)` byte-identical, `:1759`).
  `lane_hash` = domain-separated Blake2b256 of `from_sig(self).par` wire bytes
  (`:1589`). `envelope_sig_single`/`_compound`/`envelope_sig` are the ONE extracted
  derivations (`:1324`/`:1368`/`:1385`). `is_funding_former` (`:1631`) gates
  `g|#P|s∘s`. `RuntimeBudget.lanes` + `attempt_in_lane` (`:773`) + `reconcile_lane`
  (`:677`) + `lane_pool_total_cost` (`:869`) are the per-lane substrate; the N=1
  scalar fast path keeps `lanes` empty (`legacy_single_sig_*` pins it).
- **`supply.rs`:** `supply_channel(sig) = SignatureChannel::from_sig(sig).par`
  (`:60`, debug-asserts `is_funding_former`). Test
  `supply_channel_equals_lane_pool_channel` (`:472`) is the HOME for the §5
  no-alias audit test. DR-13: the only `Σ⟦s⟧` writer is `produce_balance` on a
  `GSysAuthToken` system deploy.
- **`resource_logic.rs`:** `DefaultApportionment` (`:217`) ALREADY realizes Greg's
  P8 balanced multi-sig (debits the component pair equally, order-independent).
  `acceptance.rs::compute_settlement_debits` + `admit_by_funding` operate purely on
  `Cosigned` envelopes — confirming attribution routes to signer pools (BLOCKER-1).
- **The demo** (`…-transpiler/examples/cost_accounting_demo.rho`) models many
  parties (`ada`/`ben`/`fab1`/`eve`/`diSig`/…) via arbitrary in-term sigs in ONE
  program — exactly the form BLOCKER-1 re-scopes to multiple deploys for Phase 5.

## 1. New module layout under `compiler/normalizer/cost_accounting/`

Create the module directory and wire `pub mod cost_accounting;` into
`compiler/normalizer/mod.rs` (alphabetically before `pub mod processes;`).

| New file | Origin | Disposition |
|---|---|---|
| `ir.rs` | transpiler `ir.rs` | **PORT verbatim**, then ADD `fn to_native(&self) -> accounting::Sig` (the `ir::Sig → accounting::Sig` bridge, §2). Keep `DOMAIN_*`, `key()`, `Sig::compound`, `atoms()`, `ResourceSignature`. |
| `sig.rs` | transpiler `sig.rs` | **PORT** `signature_to_ir` + `canon_bound`/`canon_ground`/`canon_quote` VERBATIM. **DROP** `supply_channel`/`signature_channel`/`atom_channel` (the transpiler's own domain-separated channel). ADD `signature_to_native_sig` (= `signature_to_ir` → `ir::Sig::to_native` → `accounting::Sig`) and a thin `fn signature_to_channel(...) -> Par` = `SignatureChannel::from_sig(&native).par`. |
| `desugar.rs` | transpiler `desugar.rs` | **PORT verbatim** (`uniform_sign`, `lollipop`, `strip_signed_binds`, `rebuild_for`). Pure Part-A AST→AST; arena-allocated `'ast`. |
| `pattern_guard.rs` | transpiler `pattern_guard.rs` | **PORT verbatim** (`reject_cost_syntax_in_pattern`, `…_in_name_pattern`). |
| `recognize.rs` | NEW (replaces transpiler `signed_term.rs` + `token.rs`) | The native-attribution recognition entry points (Phases 1/3/4). NO Par-gate codegen. See §3/§4. |
| `mod.rs` | transpiler `mod.rs`, gutted | Keep only `pub mod ir/sig/desugar/pattern_guard/recognize;` + `#[cfg(test)] mod tests;`. **DROP** `CostLoweringStrategy`, `strategy()`, `InteractionCut`/`RhoInteractionCut`, and the `pub mod lower/infra/oslf/signed_term/token`. Module-doc rewritten to "native attribution," not "internalise functor / Strangler-Fig." |
| `tests.rs` | transpiler `tests.rs` | **ADAPT**: port `signature_to_ir`/`canon_*` tests verbatim; re-target the lowering-shape assertions to native attribution (per phase). |

**DROP entirely** (Part B — native meters at the reducer; porting would add
metered nodes and break `Δ_s == consumed`): transpiler `lower.rs`,
`infra.rs` (`build_splitter` — native uses `effective_supply_with`),
`oslf.rs` (native has authoritative `resource_logic.rs`),
`signed_term.rs::build_gates`, `token.rs` send-chains.

**PRESERVE as recognition logic** (re-expressed in `recognize.rs`, NOT as
nested `Receive`s): `lower_signed_join`'s discipline (strip `Bind::Signed` to
linear, collect clause sigs, ONE token per atom, do NOT re-sign the
continuation) and `lower_signed_term`'s lollipop/`uniform_sign` dispatch.

---

## Phase 1 — Surface recognition (compile + recognize). VERIFY GATE.

**Goal end-state:** surface syntax PARSES, is RECOGNIZED, resolves `s` to a
native `Sig`, and is pattern-rejected; the inner `P`/binds lower ORDINARILY (no
synthetic gate nodes); attribution still collapses to the envelope (no behavior
change yet). The COMM-count of `{% P %}[s]` equals that of `P` alone.

### 1.1 Port the four Part-A files + create `recognize.rs` + `mod.rs`
Port `ir.rs`/`sig.rs`/`desugar.rs`/`pattern_guard.rs` per §1. In `sig.rs`,
delete `supply_channel`/`signature_channel`/`atom_channel` and their now-unused
imports (`GPrivate`/`GUnforgeable`/`UnfInstance`/`concatenate_pars`/`Blake2b256`);
keep the `Par`/`ParSortMatcher`/`prost::Message` imports needed by `canon_*`.

`recognize.rs` (Phase 1 bodies — recognition + ordinary lowering, NO gates):

```rust
//! Native recognition of cost-accounted surface syntax. Surface forms DECORATE;
//! they never re-emit metered operations (the reducer meters per-COMM). A signed
//! term resolves `s` to a native `accounting::Sig` and lowers its inner `P`
//! ordinarily; the located-stack attribution (Phase 3) is a metering-context
//! concern, not codegen here.

use std::collections::HashMap;
use models::rhoapi::Par;
use rholang_parser::ast::{AnnProc, Signature, TokenStack};
use rholang_parser::RholangParser;
use super::desugar;
use super::sig::signature_to_native_sig;
use crate::rust::interpreter::accounting::Sig;
use crate::rust::interpreter::compiler::normalize::{
    normalize_ann_proc, ProcVisitInputs, ProcVisitOutputs,
};
use crate::rust::interpreter::errors::InterpreterError;

/// `{% P %}[s]`: resolve `s` to a native `Sig` (recognition only — Phase 2/3
/// route attribution), apply the lollipop / uniform-signing AST rewrites, and
/// lower the inner `P` through the ORDINARY dispatch. Emits NO `for(t<-Σ⟦s⟧)`
/// gate — exactly the double-metering avoidance (design §3, MAJOR-2): the
/// normalized Par of `{% P %}[s]` has the SAME send/receive-node count as `P`.
pub fn recognize_signed_term<'ast>(
    inner: &'ast AnnProc<'ast>,
    sig: &'ast Signature<'ast>,
    input: ProcVisitInputs,
    env: &HashMap<String, Par>,
    parser: &'ast RholangParser<'ast>,
) -> Result<ProcVisitOutputs, InterpreterError> {
    // Lollipop `s1 -o s2`: re-sign the continuation with s2 (AST rewrite), fund
    // the rendezvous with s1 (Part A — desugar::lollipop). Core sigs apply
    // uniform-signing so a `for` continuation is metered to the same `s`.
    let (core_inner, core_sig): (AnnProc<'ast>, &Signature<'ast>) = match sig {
        Signature::Transfer(s1, s2) => (desugar::lollipop(*inner, s2, parser)?, s1),
        core => (desugar::uniform_sign(*inner, core, parser), core),
    };
    // Phase 1: resolve to native Sig for recognition + (Phase 2) the ring-fence
    // bridge; the resolved value is currently informational (envelope still
    // owns attribution). `_native_sig` becomes the metering context in Phase 3.
    let _native_sig: Sig =
        signature_to_native_sig(core_sig, &input.bound_map_chain, env, parser)?;
    // Lower the inner process ordinarily — NO gate node is synthesized.
    normalize_ann_proc(&core_inner, input, env, parser)
}

/// `s :: S` bare token stack. Phase 1: a stack at PROC level resolves each
/// layer's signature (recognition / validation) and lowers to `Nil`-equivalent
/// (Par::default()) — it mints NOTHING in the normalizer (DR-13: only the Rust
/// supply producer writes Σ⟦s⟧). Provisioning is a deploy/admission concern, not
/// a normalizer write (design §3.3 + BLOCKER-1). Phase 3/4 attach attribution.
pub fn recognize_token_stack<'ast>(
    stack: &'ast TokenStack<'ast>,
    input: ProcVisitInputs,
    env: &HashMap<String, Par>,
    parser: &'ast RholangParser<'ast>,
) -> Result<ProcVisitOutputs, InterpreterError> {
    for layer in stack.layers.iter() {
        // Resolve each layer (rejects malformed sigs — wildcard ground / @P / a
        // bare lollipop in fundable position — via signature_to_ir's errors).
        let _native: Sig = signature_to_native_sig(layer, &input.bound_map_chain, env, parser)?;
    }
    // A proc-level stack contributes no running process and no normalizer write.
    Ok(ProcVisitOutputs { par: input.par.clone(), free_map: input.free_map.clone() })
}
```

> RATIONALE for the Phase-1 token-stack body: the transpiler emitted
> `Σ⟦s⟧!(K⟦S⟧)` send chains; native MUST NOT, because (a) DR-13 forbids a
> Rholang/normalizer write to `Σ⟦s⟧`, and (b) emitting sends would add COMM
> nodes and break the `Δ_s == consumed` equality. The provisioning of fuel is
> the deploy's own funded balance (Workstream C/D), not a per-program send. In
> the multi-deploy re-scoping (Phase 5) a `s :: ()` stack is the deploy BEING
> signed by `s`; it carries no in-program mint.

### 1.2 normalize.rs — the two new dispatch arms (insert before `Proc::Bad`, ~:431)
```rust
// Cost-accounted surface syntax — recognition + native attribution (W1).
Proc::SignedTerm { proc: inner, sig } => {
    use crate::rust::interpreter::compiler::normalizer::cost_accounting::recognize::recognize_signed_term;
    recognize_signed_term(inner, sig, input, _env, parser)
}
Proc::TokenStack { stack } => {
    use crate::rust::interpreter::compiler::normalizer::cost_accounting::recognize::recognize_token_stack;
    recognize_token_stack(stack, input, _env, parser)
}
```
(In `match &proc.proc`, `inner: &AnnProc`, `sig: &Signature`, `stack:
&TokenStack` — matches the `recognize_*` signatures above.)

The signed-JOIN dispatch (a `Proc::ForComprehension` whose receipts contain a
`Bind::Signed`) is added in **Phase 4**; in Phase 1 keep `Proc::ForComprehension
=> normalize_p_input` unchanged, and let the additive `p_input` `Bind::Signed`
arm (1.3) carry it through.

### 1.3 p_input_normalizer.rs — the two `Bind::Signed` arms (compile fix)
At `:242` (the simple-source `match receipt`) add, BEFORE the catch-all:
```rust
// Phase 1: a Bind::Signed reaching p_input is treated as its underlying linear
// bind (recognition only). Phase 4 routes per-clause attribution; here we keep
// the receive shape identical to the unsigned form so the COMM count is
// unchanged. (debug_assert in Phase 4 enforces that the join path strips these.)
Bind::Signed { lhs, rhs, .. } => {
    let names: Vec<_> = lhs.names.iter().collect();
    let remainder = &lhs.remainder;
    let source_name = match rhs {
        Source::Simple { name } => name,
        _ => return Err(InterpreterError::ParserError(
            "Only simple sources supported in current implementation".to_string())),
    };
    Ok(((names, remainder), source_name))
}
```
At `:275` (the `match head_receipt` for `(persistent, peek)`) add:
```rust
Bind::Signed { .. } => (false, false), // a signed linear bind is non-persistent, non-peek
```
Add `Bind` is already imported (`use rholang_parser::ast::{… Bind …}`).

### 1.4 pattern_guard wiring (additive SECURITY at all three pattern entry points)
- `p_input_normalizer.rs`: at the top of the simple-source branch, scan every
  bind's `lhs.names` with `reject_cost_syntax_in_name_pattern`; ALSO add
  `debug_assert!` that no `Bind::Signed` reaches here once Phase 4 lands (in
  Phase 1 it MAY, so gate the assert behind a Phase-4 TODO comment).
- `p_contr_normalizer.rs`: in the `for name in formals.names.iter()` loop (`:38`),
  call `reject_cost_syntax_in_name_pattern(name)?` before `normalize_name`.
- `p_match_normalizer.rs`: at the top of `for case in cases` (`:31`), call
  `reject_cost_syntax_in_pattern(case.pattern)?`.
Import path:
`crate::rust::interpreter::compiler::normalizer::cost_accounting::pattern_guard::*`.

### 1.5 mod.rs wiring
`compiler/normalizer/mod.rs`: add `pub mod cost_accounting;`.

### GATE 1 (must all pass)
```
RUSTFLAGS="-C target-cpu=native" cargo build -p rholang
```
- `{% P %}[s]` and `s :: ()` parse + recognize (a new in-crate test in
  `cost_accounting/tests.rs` runs `Compiler::source_to_adt(r#"{% @"a"!(1) %}[g]"#)`
  and asserts `Ok`).
- **Double-metering regression (the headline Phase-1 gate):** a new test asserts
  `comm_node_count(source_to_adt("{% P %}[s]")) == comm_node_count(source_to_adt("P"))`
  for representative `P` (reuse the `comm_node_count` walk from
  `tests/accounting/delta_sigma_spec.rs`). I.e. no synthetic gate nodes.
- Ported `signature_to_ir` / `canon_ground` / `canon_quote` / `canon_bound` /
  `Sig::compound` tests pass.
- pattern-rejection: `{% P %}[s]` / `s::()` in a `match` case / receive-bind /
  contract formal returns `NormalizerError` ("cannot appear in pattern position").

---

## Phase 2 — Native sig-resolution + channel reconciliation. VERIFY GATE.

**Goal end-state:** `signature_to_native_sig` is the canonical `Signature →
accounting::Sig` bridge; channels derive via native `from_sig`; the `Bound →
Ground(DOMAIN_BOUND‖span)` fold ring-fences `new`-bound sigs with ZERO native
enum change; the §5 no-alias invariant is asserted.

### 2.1 `ir.rs::to_native` — the `ir::Sig → accounting::Sig` bridge
```rust
use crate::rust::interpreter::accounting::Sig as NativeSig;
impl Sig {
    /// Bridge the normalizer-local signature IR to the native funding algebra.
    /// `Compound` folds sorted atoms into a LEFT-ASSOC `And` (matches
    /// `fold_compound_sig`; `from_sig`'s `And` arm is sort-matched ⇒ AC holds).
    /// `Bound(span)` folds the bound-domain separator + span INTO ground content
    /// bytes (no native `Bound` variant), so `from_sig` yields a distinct
    /// `GPrivate` channel — ring-fencing via content-addressing.
    pub fn to_native(&self) -> NativeSig {
        match self {
            Sig::Ground(b) => NativeSig::Ground(b.clone()),
            Sig::Quote(b)  => NativeSig::Quote(b.clone()),
            Sig::Bound(span) => {
                let mut bytes = Vec::with_capacity(DOMAIN_BOUND.len() + span.len());
                bytes.extend_from_slice(DOMAIN_BOUND);
                bytes.extend_from_slice(span);
                NativeSig::Ground(bytes)
            }
            Sig::Compound(components) => {
                // components are key-sorted by Sig::compound's smart ctor.
                let mut it = components.iter();
                let first = it.next().expect("compound has >= 2 components").to_native();
                it.fold(first, |acc, c| NativeSig::And(Box::new(acc), Box::new(c.to_native())))
            }
        }
    }
}
```

### 2.2 `sig.rs::signature_to_native_sig` + channel
```rust
use crate::rust::interpreter::accounting::{Sig as NativeSig, SignatureChannel};
pub fn signature_to_native_sig<'ast>(
    sig: &Signature<'ast>,
    bmc: &BoundMapChain<VarSort>,
    env: &HashMap<String, Par>,
    parser: &'ast RholangParser<'ast>,
) -> Result<NativeSig, InterpreterError> {
    Ok(signature_to_ir(sig, bmc, env, parser)?.to_native())
}
/// `Σ⟦s⟧` via the consensus basis — native `from_sig` wins (design §3.1).
pub fn signature_to_channel<'ast>(
    sig: &Signature<'ast>,
    bmc: &BoundMapChain<VarSort>,
    env: &HashMap<String, Par>,
    parser: &'ast RholangParser<'ast>,
) -> Result<Par, InterpreterError> {
    Ok(SignatureChannel::from_sig(&signature_to_native_sig(sig, bmc, env, parser)?).par)
}
```

### 2.3 MINOR-6 enforcement (make disjointness structural, not incidental)
The design notes `canon_ground` begins `0x2a` (protobuf `Par.exprs` field-5 tag)
while `DOMAIN_BOUND` begins `0x66` (`'f'`), so `Ground(DOMAIN_BOUND‖span)` content
can never equal a `canon_ground` Par-wire prefix. Two pins:
- A regression test that `canon_ground(name)` ALWAYS starts with byte `0x2a`
  (a future `canon_*` refactor must not let a user name `f1r3fly.cost.sig.bound.v1:…`
  collide).
- (RECOMMENDED, cheap) fold a structurally-impossible-as-`Par`-prefix tag byte
  into the `Bound→Ground` bridge content in 2.1 so disjointness is ENFORCED, not
  accidental — note this in `to_native`'s doc.

### GATE 2
```
RUSTFLAGS="-C target-cpu=native" cargo build -p rholang
RUSTFLAGS="-C target-cpu=native" cargo test  -p rholang cost_accounting
RUSTFLAGS="-C target-cpu=native" cargo test  -p casper supply_channel
```
- **Ring-fence trio:** (i) two FREE ground sigs spelled `g` (different deploys)
  → SAME channel (`from_sig(to_native(Ground(canon_ground("g"))))` equal); (ii) a
  `new`-bound `g` vs a free `g` → DISTINCT channels; (iii) two distinct
  `new`-binders → distinct channels (distinct `canon_bound` spans).
- **MINOR-7 axis-collapse pin:** a `Ground(b)` and `Hash(#P)` whose canon bytes
  are equal map to the SAME native channel (document the divergence from the
  transpiler's `sigma_ground_and_quote_axes_are_disjoint`).
- **§5 no-alias audit test** (HOME: extend `supply.rs`'s
  `supply_channel_equals_lane_pool_channel` test module, OR a new
  `casper/.../supply.rs` test fn `user_surface_sig_never_aliases_envelope_pool`):
  for representative envelope sigs `v` (single via `envelope_sig_single`, compound
  via `envelope_sig_compound`) AND a fuzzed/adversarial set of user surface
  ground / `#P` / compound sigs resolved through `signature_to_native_sig`, assert
  for ALL pairs `from_sig(user).par != from_sig(envelope_sig(v)).par` AND
  `user.lane_hash() != envelope_sig(v).lane_hash()`; PLUS assert the byte-prefix
  disjointness the red-team verified (`canon_ground` `0x2a` vs `DOMAIN_BOUND`
  `0x66`); PLUS assert the recognition path emits NO `produce_balance`-style write
  on a channel that decodes to a system envelope (structural: `recognize_*` make
  no `space.produce` call — assert by code-review note + a test that
  `recognize_token_stack` returns `input.par` unchanged).

---

## Phase 3 — Demand attribution (located stacks, P14) + the metering seam. VERIFY GATE.

**This is the riskiest phase (see §6).** It is the per-redex signature-context
refactor of `metering.rs` (MAJOR-3) plus the `delta_sigma::demand` generalization.

### 3.1 `delta_sigma::demand` — per-`SigKey` attribution
Add a sibling to `demand` that returns a `BTreeMap<SigKey, DemandEntry>` keyed by
the signer's `funding_sig = Ground(pk)` the in-term `s` resolves to (BLOCKER-1;
§D2.9): single-sig collapses all in-term `s` to the one `funding_sig` SigKey
(`= lane_hash(Ground(pk))` = the signer's wallet `Σ⟦Ground(pk)⟧`); multi-sig
attributes a `Σ⟦Ground(pkᵢ)⟧`-gated COMM to its component `SigKey`. Signature: `pub fn demand_by_sig(desugared:
&Par, signer_keys: &BTreeSet<SigKey>, region_sig: impl Fn(&Par)->Option<SigKey>)
-> BTreeMap<SigKey, DemandEntry>`. KEEP the existing single-arg `demand` for the
s₀ path (back-compat / `legacy` pin). Because the normalized `Par` carries no
per-layer signature (s₀ collapse) and `source_to_adt` has no signer list, the
per-`SigKey` split is driven by the **runtime metering context** (3.2), not by the
normalizer; `demand_by_sig` is the STATIC dual fed the SAME region→SigKey map the
reducer uses, so the two agree COMM-for-COMM (the consensus bridge). For an
in-term `s` NOT in `signer_keys`, attribute ZERO (the gate rejects the deploy —
see 3.4) — never invent a foreign-`s` lane.

### 3.2 `metering.rs` — the per-redex signature context (MAJOR-3 refactor)
The single cached `sig_hash` becomes a DEFAULT (the envelope) plus an optional
override active inside a recognized signed region:
- Add to `MeteredMachine` a cheap **"any signed regions present?"** flag (an
  `Arc<AtomicBool>` set once at install from the program's recognition metadata)
  and a per-machine **signature-context stack** (`Arc<Mutex<Vec<[u8;32]>>>` or a
  thread-local; cloned-by-`Arc` like `pending`). `enter_signed_region(sig_hash)`
  pushes; a returned RAII guard pops on drop (`exit`). When the stack is non-empty,
  `reserve_cost` stamps the TOP override; otherwise the cached envelope `sig_hash`
  (byte-identical to today).
- **Fast path (preserves `legacy_single_sig_byte_identical`):** if the
  "any signed regions" flag is false, `reserve_cost` takes the EXACT current code
  path — no stack lock, no override read — so non-cost deploys add ZERO per-COMM
  work. Add a hot-path microbench (`metering` bench) asserting no regression on the
  flag-false path.
- **Billing-after-resolution:** to attribute on the gate CHANNEL (the design's
  preferred mechanism), `reserve_comm` in `reduce.rs::eval_send`/`eval_receive`
  must move to AFTER `sub_chan` (`:1007`) / after `unbundle_receive` (`:1071`) is
  computed, so the resolved channel `Par` can be matched against the in-scope
  signer channels (`SignatureChannel::from_sig(signer).par`) to derive the
  per-redex `sig_hash`. Guard the move behind the "any signed regions" flag:
  flag-false ⇒ keep the charge at the top (byte-identical); flag-true ⇒ resolve
  channel first, then `reserve_comm_for_channel(sub_chan, cost)`. The
  region-context (3.2 stack) and the channel-match are TWO routes to the same
  `sig_hash`; choose ONE per construct (signed-term region → context stack;
  per-clause `Bind::Signed` → channel match) and document which.
- Thread the recognition metadata from the normalizer to install: since the `Par`
  has no signature field, carry a side table `Vec<(region marker, SigKey)>`
  produced by `recognize_*` (e.g. on `ProcVisitOutputs`/a compiler output struct)
  into `MeteredMachine::new` at the `reduce.rs:?`/`rho_runtime.rs` construction
  site. If threading is too invasive for W1, fall back to the CHANNEL-MATCH route
  ONLY (match each resolved COMM channel against the installed signer channels) —
  no normalizer→reducer side table needed; the region context becomes a pure
  reducer concept. PREFER the channel-match route for W1 (smaller blast radius;
  the envelope-signer set is already on the budget via the installed `Sig`).

### 3.3 BLOCKER-1 attribution rule (signer pools only)
A COMM whose resolved channel equals `from_sig(Ground(pk_i)).par` for some installed
deploy signer attributes to `signer_i`'s lane (`attempt_in_lane`); otherwise it
attributes to the envelope-default lane (the byte-identical s₀ behavior). Post-§D2.9
the signer channels are the signers' GROUND wallet pools, so single-sig: the only
signer channel IS `Σ⟦Ground(pk)⟧` (`= funding_sig`'s lane), so everything collapses
(no change). Multi-sig: the component pools are the `Sig::And` leaves
`Σ⟦Ground(pkᵢ)⟧`, which native ALREADY funds via the `And`-fold of `Ground(pkᵢ)` +
`compute_settlement_debits` component draws — NO new pool-write path; DR-13 preserved.

### 3.4 BLOCKER-1 rejection (in-term `s` not among signers)
Because the normalizer can't see signers, the reject lives at the
reducer/gate boundary: if a recognized signed region resolves to a channel that
matches NO installed signer channel and is NOT the envelope, the deploy is
malformed/unfundable. Realize as: the channel-match in 3.2 returns `None` ⇒ the
COMM attributes to the envelope (so a stray `{% P %}[foreign]` does NOT silently
create a foreign lane), AND a pre-eval admission check (the
`is_funding_former`/`pattern_guard` analogue) walks the program's recognized sigs
and rejects if any resolves to a non-signer, non-envelope channel. For W1 the
conservative MINIMUM is: attribute-to-envelope (never a foreign lane) + a test
asserting a foreign-`s` deploy's `Δ` lands wholly on the envelope (no lane
leakage); the explicit admission-reject can be a Phase-4/5 admission test.

### GATE 3
```
RUSTFLAGS="-C target-cpu=native" cargo test -p rholang delta_sigma
RUSTFLAGS="-C target-cpu=native" cargo test -p rholang metering
RUSTFLAGS="-C target-cpu=native" cargo test -p rholang legacy_single_sig
```
- **Multi-lane static==runtime equality:** extend
  `tests/accounting/delta_sigma_spec.rs` with a multi-signer fixture where the
  static `demand_by_sig` per-lane counts equal the runtime per-lane consumed
  counts (`get_cost_event_log` filtered by `sig_hash`), COMM-for-COMM.
- **Legacy byte-identity regression (the load-bearing fast-path pin):**
  `legacy_single_sig_byte_identical` (or an equivalent new test) — a
  single-sig non-cost deploy: `lanes` empty, scalar `reconcile()` field-for-field
  equal to the pre-Phase-3 walk, `cost_trace_digest` unchanged. Confirm the
  "any signed regions" flag is false on this path.
- **OSLF conformance per lane:** `resource_logic_conformance::law_sound` runs in
  both regimes per lane (re-run with a per-lane `DemandEntry`).

---

## Phase 4 — Join sequential-fuel + balanced multi-sig. VERIFY GATE.

**Goal end-state:** a `for` with `Bind::Signed` clauses recognizes per-clause,
attributes each rendezvous COMM to that clause's signer lane independently (never
a fuel+data join — MAJOR-5/Greg, TLA+-confirmed), and reuses
`compute_settlement_debits` + `DefaultApportionment` for balanced multi-sig (P8).

### 4.1 normalize.rs — signed-JOIN dispatch
Extend the `Proc::ForComprehension { receipts, proc }` arm (`:401`): if any
`receipt.binds` contains a `Bind::Signed`, route to
`recognize::recognize_signed_join(receipts, proc, …)`; else `normalize_p_input`
unchanged. Add the Greg-2026-06-15 rule citation as a code comment AT this arm
AND at the `recognize_signed_join` entry (design §4.3): "fuel is provisioned on
Σ⟦s⟧ and the data `for` is metered per-COMM by lane; a fuel token is structurally
incapable of entering the data join's ReceiveBind set — do NOT 'optimize' fuel
into the join."

### 4.2 recognize.rs — `recognize_signed_join`
Port `lower_signed_join`'s STRUCTURE without `build_gates`:
```rust
pub fn recognize_signed_join<'ast>(
    receipts: &'ast Receipts<'ast>, body: AnnProc<'ast>, span: SourceSpan,
    input: ProcVisitInputs, env: &HashMap<String, Par>, parser: &'ast RholangParser<'ast>,
) -> Result<ProcVisitOutputs, InterpreterError> {
    // Strip Bind::Signed -> linear, collect clause sigs in source order.
    let (plain_for, sigs) = desugar::strip_signed_binds(receipts, body, span, parser);
    debug_assert!(!sigs.is_empty(), "dispatched only when a Bind::Signed is present");
    // Resolve each clause sig to its native lane key (Phase 2/3); attach as
    // per-clause attribution metadata for the reducer (Phase 3 channel-match or
    // region context). The continuation P is NOT re-signed (one token per clause).
    let mut clause_keys = Vec::with_capacity(sigs.len());
    for s in &sigs {
        clause_keys.push(signature_to_native_sig(s, &input.bound_map_chain, env, parser)?.lane_hash());
    }
    // Lower the recovered PLAIN for ordinarily (the binds are now linear, so it
    // re-normalizes through normalize_p_input with NO signed-bind recursion).
    let out = normalize_ann_proc(&plain_for, input, env, parser)?;
    // ASSERT no fuel-in-data-join: the lowered receive's binds == the original
    // linear clauses (mirrors normalize_p_input's debug_assert that no
    // Bind::Signed survives). [record clause_keys into the per-redex attribution
    // side table / context here, per the Phase-3 mechanism chosen.]
    Ok(out)
}
```
Per-clause attribution (Phase-3 machinery): each clause's rendezvous COMM (the
receive on `rhs`) is matched to its `clause_keys[i]` lane; independent per-lane
`compute_settlement_debits`. The DATA join's `binds` contain ONLY the original
linear clauses (assert).

### GATE 4
```
RUSTFLAGS="-C target-cpu=native" cargo test -p rholang  recognize_signed_join
RUSTFLAGS="-C target-cpu=native" cargo test -p casper   apportionment
RUSTFLAGS="-C target-cpu=native" cargo test -p casper   compute_settlement_debits
```
- **Signed-join funded / unfunded / mixed:** a 2-clause join where both signer
  lanes are funded → fires; one unfunded → that lane's COMM is rejected/parks at
  admission (per-deploy in the multi-deploy model), the join does not complete;
  mixed → only the funded clause settles. Assert per-lane `Δ` independence.
- **No fuel-in-data-join invariant:** the lowered `Receive.binds` for a signed
  join equals the unsigned-equivalent `for`'s binds (a debug-assert test).
- **Compound conservation (P8):** reuse `apportionment_conformance` —
  `DefaultApportionment` debits the component pair EQUALLY, order-independent
  (`s₁∘s₂` and `s₂∘s₁` settle identically), conservation `draw_combined +
  draw_pair == k`. No new policy is written (design §3.4 RESOLVED).

---

## Phase 5 — Demo as native integration test (re-scoped per BLOCKER-1). VERIFY GATE.

**Re-scope (mandatory):** the transpiler demo simulates many parties via arbitrary
in-term sigs in ONE program. Native binds a deploy to its signer(s), so a
multi-party ecosystem is MULTIPLE deploys (each signed by its party). The
in-program PARK invariants (`eve`/`Zed`/free-`diSig` thief never run) come from the
transpiler's runtime gate-receive-blocks-without-a-token mechanic, which native
(recognize-only, envelope-funded) does NOT reproduce (MAJOR-4).

### 5.1 Port + re-scope `examples/cost_accounting_demo.rho`
Bring the demo into THIS branch's `examples/` (or a new
`rholang/tests/accounting/cost_accounting_demo_spec.rs`). Map each party's in-term
sig to a DEPLOY SIGNER: a single deploy signed by `ada` carries Ada's `{% … %}[ada]`
buys; `ada :: ada :: ()` becomes Ada's funded balance (`Σ⟦ada⟧ = 2`), not an
in-program send. Multi-sig scenes (`fab1 (*) carrier`) become a multi-signer
cosigned deploy. The PARK scenes (`eve`, `Zed`, thief) become SEPARATE deploys
whose signer pool is ABSENT/empty ⇒ admission-REJECTED (gate), not in-program
parked.

### GATE 5
```
RUSTFLAGS="-C target-cpu=native" cargo test -p rholang cost_accounting_demo
RUSTFLAGS="-C target-cpu=native" cargo test -p casper  admit_by_funding
```
- **Funded-path numeric conservation** under signer-keyed funding: MONEY total
  stays 410; WIDGET total 67 (opening) + 16 (produced) = 83; no cell ever negative
  (the guards); exactly one flash-sale winner.
- **PARK ⇒ admission-rejection (per-deploy):** an unfunded signer's deploy
  (`eve`/`Zed`/thief, `Σ = 0` / absent pool) is gate-REJECTED via `admit_by_funding`
  (`read_balance_present` → `None`/`Some(0)` + `is_funded` reject), NOT in-program
  parked. Assert the rejected-primary-sigs set contains the unfunded parties.
- DROP the design's old "the demo is a drop-in native integration test" claim.

---

## Cross-cutting: merge gate + final gate

**Merge gate (consensus — this stays on the dev `[patch]`):** NOT mergeable to a
consensus/release branch until the parser grammar (the single unpublished commit
carrying `Proc::SignedTerm`/`TokenStack`/`Bind::Signed`) is published upstream on
`rholang-rs` and PINNED. The parser rev is part of every normalized `Par`'s
byte-identity (AST → lowered `Par` → hash), so a `[patch]`-built node and a
`rev`-built node could diverge by one `parser.c` byte ⇒ consensus fork. Before any
push: published rev pinned in `rholang/Cargo.toml`, the workspace-root `[patch]`
block ABSENT, `Cargo.lock` restored (un-skip-worktree), `cargo build -p rholang`
green against the published rev. Add the CI assertion (currently a TODO per the
design's §7 risk 1) that no `[patch]` / Cargo.lock churn lands on a release branch.

**Final gate (all phases complete):**
```
RUSTFLAGS="-C target-cpu=native" cargo fmt --check
RUSTFLAGS="-C target-cpu=native" cargo clippy -p rholang -p casper --all-targets -- -D warnings
RUSTFLAGS="-C target-cpu=native" cargo build  -p rholang -p casper
RUSTFLAGS="-C target-cpu=native" cargo test   -p rholang -p casper
```
`target-cpu=native` is mandatory while the patched parser (`gxhash`) is in the tree.

## Sequencing / dependencies (one-line summary)
Phase 1 (compile+recognize, no behavior change) → Phase 2 (native Sig + channel +
no-alias audit) → Phase 3 (metering seam + per-SigKey demand; the load-bearing
fast-path pin) → Phase 4 (signed join + balanced multi-sig, reuses existing
apportionment) → Phase 5 (demo as multi-deploy integration test). Phases 1-2 are
mechanical/additive; Phase 3 is the consensus-hot reducer refactor; Phases 4-5
reuse Phase-3 machinery + existing native settlement.

## Riskiest phase
**Phase 3** — the `metering.rs` per-redex signature-context refactor +
`delta_sigma::demand` per-`SigKey` generalization. Why:
1. **Consensus hot path.** `reserve_comm` fires per-COMM at the TOP of every
   `eval_send`/`eval_receive`; moving billing after channel resolution and adding a
   context-stack/channel-match touches the single most-executed reducer path. Any
   non-byte-identical change on the single-sig path forks consensus
   (`legacy_single_sig_byte_identical`, `replay_cost_mismatch`,
   `cost_trace_digest`). The "any signed regions present?" fast-path flag is the
   load-bearing mitigation and MUST be proven zero-overhead (hot-path bench).
2. **Static↔runtime equality is the gate↔runtime bridge.** `demand_by_sig` (static)
   and the reducer's per-lane attribution (runtime) MUST agree COMM-for-COMM on the
   same region→SigKey map, or the acceptance gate admits deploys the runtime can't
   fund (or rejects fundable ones) — a fork. This is a NEW multi-lane invariant
   beyond the existing s₀ single-lane one.
3. **The normalizer↔reducer information gap.** `Compiler::source_to_adt` has NO
   signer context and `Par` has no signature field (s₀ collapse), so the resolved
   `Sig`/region must reach the reducer via either a side table (invasive) or
   channel-match (chosen for W1). Getting BLOCKER-1 right — in-term `s` attributes
   ONLY to an installed signer pool, never a fresh foreign lane (DR-13), and a
   non-signer `s` is rejected/envelope-folded, not silently lane-leaked — is
   subtle and the place a security regression would hide.

Phases 1-2 are low risk (additive, compile-gated, no consensus-state change).
Phase 4's apportionment is RESOLVED (reuses `DefaultApportionment` verbatim,
TLA+-confirmed griefing-safe). Phase 5 is test-only.
