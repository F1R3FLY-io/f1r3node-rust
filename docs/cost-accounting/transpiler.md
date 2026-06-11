# Cost-Accounting Transpiler (Normalizer Side)

> Lower cost-accounted Rholang surface syntax to **stable Rholang** (`Par`) via
> the cost-accounted-rho **§8 source-to-source translation**, integrated into
> f1r3node's normalizer. A **stopgap** until the native cost-accounted reducer
> (`../f1r3node-rust/`) lands; designed so the swap is an *interface-level*
> change, not a rewrite.

Module: `rholang/src/rust/interpreter/compiler/normalizer/cost_accounting/`.

This document, together with `docs/cost-accounting/parser-syntax.md` in the
parser repo (`rholang-rs-cost-accounting-transpiler`), is sufficient to
reconstruct the design from scratch. Provenance for the constructs and the
five gated rules R1–R5 is the **`mettail-rust-cost-accounting` worktree**
(`cost-decoration/src/main.rs@937a6335`); the sugars (uniform, lollipop),
N-ary joins, splitter/joiner, and the per-clause/Axis-C joins are from the
**papers** (`publications/cost-accounting/cost-accounted-rho.tex`). The
**surface syntax** is authoritative against Greg's appendix *"Concrete Syntax
for Rholang 1.2"* (`cost-accounted-rho.tex@fdf089e §app:concrete`): bare token
stacks `s :: ()` (no `purse(...)`), compound `s1 (*) s2`, section `# P`, and the
polymorphic *SignedProc* forms. **Located purses are gone** — ring-fencing is
realised by a **`new`-bound signature** (the binding-sensitive `Σ⟦s⟧` of §2),
not a syntactic owner.

---

## 1. Surface → §8 translation map

| Construct | Surface (Greg `app:concrete`) | §8 translation → stable `Par` | Code |
|---|---|---|---|
| Signed term `{P}_s` | `{% P %}[ s ]` | `T⟦{P}_s⟧ = for(t <- Σ⟦s⟧){ *t ∣ P⟦P⟧ }` | `signed_term.rs` |
| Compound signed `{P}_{s₁∘…∘sₙ}` | `{% P %}[ a (*) b ]` | nest `n` gates over the component atoms | `signed_term.rs` |
| Token stack | `s :: S` / `()` | `K⟦()⟧ = Nil`, `K⟦s::S⟧ = Σ⟦s⟧!(K⟦S⟧)` | `token.rs` |
| Bare stack (a process) | `s :: ()` | `K⟦S⟧` in parallel (no `purse(...)` wrapper) | `lower.rs` |
| Ground sig `g` (free) | bare name `s` | `Σ⟦g⟧` over `DOMAIN_GROUND` (content of the identifier) | `sig.rs` |
| Ground sig `g` (`new`-bound) | bare name `s` in scope of `new s` | `Σ⟦g⟧` over `DOMAIN_BOUND` (binder span) — ring-fenced | `sig.rs` |
| Section sig `#P` | `# P` | `Σ⟦#P⟧` (content of `𝒫⟦P⟧`) | `sig.rs` |
| Compound sig `s₁∘s₂` | `a (*) b` | `Σ⟦s₁∘s₂⟧` canonical-sorted | `sig.rs` |
| Uniform signing | `{% for(y<-x){P} %}[ s ]` | `→ {% for(y<-x){ {P}_s } %}[ s ]` then §8 | `desugar.rs` |
| Lollipop transfer | `{% for(y<-x){P} %}[ a -o b ]` | `→ {% for(y<-x){ {P}_b } %}[ a ]` then §8 | `desugar.rs` |
| N-ary join (signed term cont.) | `for(y₁<-x₁ & …){ T }` | ordinary Rholang join; `T` is signed | (reuses `p_input`) |
| Per-clause signed bind (Axis-C) | `for( {% y<-x %}[ s ] & … ){ P }` | strip to plain binds; gate the `for` by `Σ⟦s⟧` per clause | `signed_term.rs` (`lower_signed_join`) |
| Signed send | `x!( {% P %}[s] )`, `x!( s::() )` | ordinary `Send`; payload lowered by dispatch | (reuses `p_send`) |
| Located stacks `S1 ∥ S2` | `S1 \| S2` | ordinary parallel composition (ring-fence via `new`-bound sig) | (reuses `p_par`) |
| Combined-cell token (R3/R5) | — | splitter/joiner contracts | **Phase 2** |

`P⟦·⟧` (the §8 translation of the host process) is exactly
`normalize_ann_proc`: nested signed terms inside `P` are lowered by the
ordinary dispatch recursion (`compiler::normalize`), so the catamorphism is
`Σ / N / K / T / P` folded over the cost AST.

---

## 2. Cost model & encoding — `Σ⟦·⟧`

`Σ⟦s⟧` is a **content-addressed, unforgeable** channel: a closed `Par` (empty
`locally_free`), identical at every use site that resolves `s` the same way.
Resolution is **binding-sensitive** — `signature_to_ir` consults the gate's
enclosing `bound_map_chain`:

* a **free** ground sig is content-addressed by its **spelling** over
  `DOMAIN_GROUND` (the same `g` everywhere ⇒ one global channel — the §9
  cross-principal rendezvous);
* a **`new`-bound** ground sig is content-addressed by its **binder's stable
  identity** (the `new s` declaration's source span) over `DOMAIN_BOUND`, so
  distinct `new`-scopes give distinct channels — **ring-fencing** (replaces the
  located purse `purse(I,…)`; `Sig::Bound` in `ir.rs`).

Both the funding token (`K⟦s::S⟧`) and the gate (`T⟦{P}_s⟧` / a per-clause
bind) resolve `s` in the **same** enclosing scope, so they always agree on the
channel. The result is still a closed `Par` (no open free variables), so **no
program-level binding pass** is needed.

```
Σ⟦Ground(g)⟧,  g free       = @( GUnforgeable( GPrivate( Blake2b256( DOMAIN_GROUND ‖ canon(g) ) ) ) )
Σ⟦Ground(g)⟧,  g new-bound  = @( GUnforgeable( GPrivate( Blake2b256( DOMAIN_BOUND  ‖ canon_bound(span g) ) ) ) )
Σ⟦Quote(#P)⟧                = @( GUnforgeable( GPrivate( Blake2b256( DOMAIN_QUOTE  ‖ canonP(P) ) ) ) )
Σ⟦s₁∘…∘sₙ⟧                  = ParSortMatcher::sort_match( Σ⟦s₁⟧ ∣ … ∣ Σ⟦sₙ⟧ )   (over flattened, key-sorted components)
```

* `canon(g)` = `sort_match(Par with GString(identifier)).encode_to_vec()` —
  depends only on the spelling, so the same FREE `g` everywhere yields the same
  channel (the §9 rendezvous works).
* `canon_bound(span)` = the binder's source span
  (`start.line:start.col-end.line:end.col`) — a stable identity unique to each
  `new`/`for` binder. Two gates funded by the SAME binder share a channel; gates
  under DISTINCT binders are disjoint, even when the binders spell the name
  identically (α-distinct ⇒ disjoint), and a free-sig gate can never drain
  `new`-bound (ring-fenced) fuel.
* `canonP(P)` = `sort_match(normalize P standalone at de Bruijn depth 0)
  .encode_to_vec()` — **binder-depth-independent** and α-invariant, so a `#P`
  that references an outer bound name hashes the same wherever it appears
  (`FN_s(#P) = FN(P)`: free names are part of the principal's identity).

**Why content-addressed channels are correct (not a fuel-theft leak).**
Verified against `migration §5.8.1`: the native channel IS global +
content-addressed; per-deploy isolation is achieved by salting the *signature
value* per deploy, **not** by per-deploy channels. A signature is a principal:
distinct sigs ⇒ distinct channels (isolation is intrinsic); the **same** free
sig shared by two parties is the *intended* rendezvous (§9: a client deposits on
`Σ⟦c⟧`, a separate `v`-signed fee interceptor consumes there). **Intra-program
ring-fencing** — where one party's fuel must be unreachable to another within a
single deploy — is the `new`-bound case: a `new s` binder mints on `DOMAIN_BOUND`
keyed by the binder, so no free-`s` (or differently-bound-`s`) gate can drain it.
This is the binding-sensitive dual: free ⇒ shared-by-spelling, bound ⇒
disjoint-by-binder.

**Funding & ownership.** A token is a *send* on `Σ⟦s⟧`; a signed process is a
*fuel gate* that blocks until a token arrives, then one COMM consumes it,
binds `t` to the payload (remaining balance), `*t` re-releases it, and `P`
runs. Ownership/authority = the object-capability to mint a token on the
unforgeable `Σ⟦s⟧`; `GPrivate` ⇒ it cannot be forged. Ownership transfer =
lollipop (`s₁` funds the rendezvous, the continuation is re-signed `s₂`); joint
ownership = the compound `s₁*s₂`.

**Funding a compound — separate stacks vs. a combined cell (NOT a multi-layer
stack).** A single multi-layer stack `a :: b :: ()` is a sequential **stack** —
`Σ⟦a⟧!(Σ⟦b⟧!(Nil))`, where only the head token is free and `*t` threads the rest
— so it funds `{P}_a` *then* `{Q}_b` (single-sig gates in sequence), **not** a
joint `{P}_{a∘b}`. A compound (joint) signed term is funded either by
**separate** token stacks — `a :: () | b :: ()` (R2), which provide parallel
*free* tokens the nested gate consumes in any order — or by a **combined-cell**
token `a (*) b :: ()` (R3), which the splitter rewrites into a chain in the
gate's key-sorted atom order. (The nested gate orders its component gates by
`Sig::key`, which need not match a stack's source-layer order; that mismatch is
precisely why a single chain is *not* compound funding — surfaced by the
end-to-end reduction tests.)

### Difference from native `from_sig` (documented, deliberate)

Native `SignatureChannel::from_sig` puts **no** domain separator at the channel
layer (ground vs quote are byte-identical there; isolation lives at the
signature-value layer). This shim **domain-separates `DOMAIN_GROUND`/
`DOMAIN_QUOTE`** in the channel hash. Non-byte-compatible **on purpose** — the
swap retires this lowering, so byte-parity with native is not a goal; injective,
deterministic, unforgeable content-addressing within the transpiler is.

### Content-addressing invariant

`key(s)` (`ir::ResourceSignature::key`) and `Σ⟦s⟧` are derived from the same
atom hashes, so two signatures share a `key` iff they share a channel: for an
atom, `key` is exactly the `GPrivate` id of `Σ⟦s⟧`; for a compound, `key`
hashes the key-sorted component keys while `Σ` sorts the component channels —
both injective in the sorted component list.

---

## 3. The fuel gate `T⟦{P}_s⟧` (de Bruijn construction)

The gate is a **hand-built `Receive`** (the existing `normalize_p_input` cannot
accept a `Par` channel, only an AST `Name` — strategy "A" is a dead end):

```
for(t <- Σ⟦s⟧){ *t ∣ P⟦P⟧ }
  ReceiveBind { patterns: [FreeVar(0)], source: Σ⟦s⟧, remainder: None, free_count: 1 }
  bind_count = 1 ;  condition = None  (the where-clause field, set explicitly)
  body = *t ∣ P,  with *t = BoundVar(0)
```

* **de Bruijn.** `BoundMap::get` returns `next_index − level − 1`, so the
  most-recently-bound variable is index 0. The gate normalizes `P` after
  `put_span`-ing **one synthetic, hygienic fuel binder** (a name containing
  spaces — never collides with a source identifier), which bumps `next_index`
  by 1 so `P`'s references to *outer* names get the correct +1 offset; `*t` is
  then `BoundVar(0)`.
* **compound `{P}_{s₁*…*sₙ}`.** Nest `n` gates over the component atoms (Phase
  1: funded by atomic component tokens; the combined-cell token is Phase 2).
  Fuel var `t_j` (gate at depth `j`, outermost first) sits at `BoundVar(n-1-j)`;
  each receive removes its bound var from the body bitset
  (`filter_and_adjust_bitset(., 1)`) and shifts the rest down by one.

The lowered shapes correspond to the prototype's `Wrap` / `SCons` / `SGround` /
`SQuote` / `SCompose` (a conformance test guards against drift).

### 3.1 Per-clause signed binds — the Axis-C join (`lower_signed_join`)

A `for` whose receipts carry **per-clause signed binds**
`for( {% y<-x %}[ s ] & … ){ P }` (Greg `app:concrete` SignedBind) meters the
**rendezvous** of each signed clause independently. The `ForComprehension`
dispatch in `normalize_ann_proc` intercepts any `for` with a `Bind::Signed` and
routes it to `signed_term::lower_signed_join`, which:

1. **strips** the signed clauses back to ordinary linear binds
   (`desugar::strip_signed_binds`), recovering a plain `for` (plain binds and
   `where` guards preserved verbatim) and collecting the clause signatures in
   source order;
2. resolves each clause signature to its funding atoms in the `for`'s
   **enclosing** scope (the binding-sensitive `Σ` of §2 — so a `new`-bound
   clause sig ring-fences);
3. wraps the recovered `for` in **one fuel gate per atom**, reusing the very
   same `build_gates` machinery as a signed term.

So the join is metered exactly as `{ for(R){P} }_{s₁ ∘ … ∘ sₖ}` — the comm fires
only once a token has been consumed on **every** `Σ⟦sᵢ⟧` (the product of the
clause fuels). The one deliberate difference from a signed *term*: the
continuation `P` is **NOT** re-signed (no `uniform_sign` step) — a per-clause
bind meters the *rendezvous*, not the continuation, so **one** token per clause
suffices (contrast uniform signing's two). Because the recovered `for` is plain,
it re-normalizes through the ordinary `for`-path (`normalize_p_input`), which
asserts (in debug) that it never sees a `Bind::Signed` — the interception is
total, so the loop is well-founded.

---

## 4. Architecture & seam (pattern-sourced)

* **Split Phase** — *parse* (grammar+AST, permanent) vs *desugar→lower* (the §8
  translation, isolated in this module).
* **Protected Variations / Hexagonal Ports-&-Adapters** — one port
  (`CostLoweringStrategy`), one construction site (`strategy()`), one adapter
  (`LowerToPar`). `compiler::normalize` dispatches through the port only.
* **Strangler Fig** — the lowering is isolated so it can be **retired** when
  native lands (native does no lowering; the `ir::Sig` funding algebra is
  reused).
* **Anti-Corruption Layer + Dependency Inversion** — `ir::Sig` /
  `ir::ResourceSignature` mirror the *shape* of native
  `accounting::{Sig, ResourceSignature}` with **no hard dependency**; a future
  MeTTaIL front-end is a second `InteractionCut` instance (DR-24: adapter, not
  dependency).

### Pattern-position rejection

f1r3node's `rholang` crate does **not** depend on `rholang-lib`, so the
parser-side resolver rejection (`CostSyntaxInsidePattern`, used by tooling/LSP)
does not run here. The normalizer therefore applies `pattern_guard` at every
pattern entry point — `p_match` (case patterns), `p_input` (receive-bind
formals), `p_contr` (contract formals) — rejecting any `{P}_s` / bare stack that
would otherwise lower to a `for`/send in pattern position.

---

## 5. Phasing

* **Phase 1 (done).** Atomic + split-compound signed terms; ground/`#P`/compound
  sigs; token stacks; **N-ary joins** (reuse `p_input`) and **per-clause signed
  binds** (Axis-C join, `lower_signed_join`); **uniform + lollipop** sugar;
  binding-sensitive content-addressed channels (**`new`-bound ring-fencing**
  replaces the located purse); signed sends / signed `for`-continuations;
  pattern-rejection guard. Covers R1/R2/R4 + joins + ring-fencing. No
  splitter/joiner, no combined-cell tokens, no program-level pass.
* **Phase 2.** Combined-cell tokens (R3/R5); splitter/joiner program-level pass
  at the `Compiler::normalize_term` chokepoint. **Requires** a written
  intra-program non-interference (faithfulness) analysis first. (Located purses
  are no longer a Phase-2 item — ring-fencing is realised in Phase 1 via the
  binding-sensitive `Σ`.)

---

## 6. Dependency flip (DEV ONLY — revert before push)

The transpiler builds against the local rholang-rs worktree carrying the new
surface syntax, via a workspace-root `[patch]`:

```toml
[patch."https://github.com/F1R3FLY-io/rholang-rs"]
rholang-parser                 = { path = "../rholang-rs-cost-accounting-transpiler/rholang-parser" }
rholang-tree-sitter            = { path = "../rholang-rs-cost-accounting-transpiler/rholang-tree-sitter" }
rholang-tree-sitter-proc-macro = { path = "../rholang-rs-cost-accounting-transpiler/rholang-tree-sitter-proc-macro" }
```

`[patch]` matches by source URL, so all three crates pulled from that git
source flip together. `Cargo.lock` flips three entries `git+… → path+…`; keep
that churn out of git with `git update-index --skip-worktree Cargo.lock`
(reverse: `--no-skip-worktree`).

### Pre-push checklist

1. Remove the `[patch]` block from the workspace-root `Cargo.toml`.
2. `git update-index --no-skip-worktree Cargo.lock` and restore the committed
   `Cargo.lock` (so the three crates point back at `rev = c163755`).
3. `cargo build -p rholang` (against the published git rev) — must compile only
   if the new surface syntax has been released on rholang-rs **and** `rev` bumped
   accordingly; otherwise the cost-syntax code will not build and **must not be
   pushed** until the parser rev is published.
4. Run the full static-analysis pipeline (§8) and confirm no `[patch]`/lock
   churn remains in `git status`.

---

## 7. Forward integration (OSLF/GSLT seam)

The transpiler is the **internalise ⊣ include** adjunction of the cost
endofunctor; native is **install ⊣ forget**. Both realize the same cost
semantics over the same interaction-cut interface, so the design mirrors the
trait family native already exposes (`accounting/resource_logic.rs`:
`GsltPresentation`, `OslfResourceLogic`, `ResourceSignature` + conformance
laws). `ir::Sig` implements the locally-mirrored `ResourceSignature`
(`key`, `split_join_decompositions`) so the signature/funding algebra is reused
across the swap. The swap is interface-level (the shared OSLF/GSLT funding
port), not a behaviour-identical program-level drop-in — native has no surface
syntax — so Part A (grammar/AST) is a permanent superset asset and Part B's
`strategy()` is retired, not ported.

---

## 8. Verification

```bash
RUSTFLAGS="-C target-cpu=native" cargo fmt --check
RUSTFLAGS="-C target-cpu=native" cargo clippy -p rholang --all-targets -- -D warnings
RUSTFLAGS="-C target-cpu=native" cargo test  -p rholang
RUSTFLAGS="-C target-cpu=native" cargo build  -p rholang
```

`RUSTFLAGS="-C target-cpu=native"` is required (the patched parser pulls
`gxhash`, which needs AES/SSE2 intrinsics).

---

## 9. Scientific ledger

| Date | Step | Hypothesis | Result |
|---|---|---|---|
| 2026-06 | Phase-0 grammar spike | new tokens lex without conflict, ABI stable | **confirmed** — no conflicts; LANGUAGE_VERSION 15 stable (STATE 1364→1457); `-o` collision benign (context-aware lexer); corpus 87/87 |
| 2026-06 | Dependency `[patch]` flip | repoints all three rholang-rs crates to the local worktree | **confirmed** — `cargo tree` shows local paths; lock entries sourceless; rholang lib compiles |
| 2026-06 | Gate de Bruijn (`*t = BoundVar(0)`) | one synthetic fuel binder gives the correct outer-ref offset | **confirmed** — `atomic_signed_term_lowers_to_a_fuel_gate`, `compound_signed_term_nests_one_gate_per_component` |
| 2026-06 | `Σ` byte-parity / AC | `Σ⟦(a*b)*c⟧ = Σ⟦a*(b*c)⟧ = Σ⟦c*b*a⟧` byte-identical | **confirmed** — `sigma_compound_is_commutative_and_associative` |
| 2026-06 | Determinism | byte-identical `encode_to_vec` across 100 re-normalizations | **confirmed** — `lowering_is_deterministic_across_renormalizations` |
| 2026-06 | Desugar fidelity | uniform/lollipop sign the continuation per the papers | **confirmed** — `uniform_signing_signs_the_continuation`, `lollipop_funds_rendezvous_with_s1_and_continuation_with_s2` |
| 2026-06 | Realign to Greg `app:concrete@fdf089e` | `# P` / `s1 (*) s2` / bare stacks / polymorphic SignedProc lex + lower with no ABI break | **confirmed** — stratified `Sig/Sig1/Sig2` keeps `x-owed` lexing as `x - owed`; LANGUAGE_VERSION 15 stable; tree-sitter corpus 90/90 |
| 2026-06 | Binding-sensitive `Σ⟦s⟧` (ring-fence) | a `new`-bound sig keys on its binder (DOMAIN_BOUND) ⇒ ring-fenced; a free sig stays by-spelling (DOMAIN_GROUND) ⇒ §9 preserved | **confirmed** — `new_bound_signature_funds_an_in_scope_gate`, `new_bound_fuel_is_ring_fenced_from_a_free_sig_gate`, `free_shared_signature_still_rendezvous` |
| 2026-06 | Per-clause signed bind (Axis-C join) | `for( {% y<-x %}[s] & … ){P}` gates the comm by each clause's fuel (one token/clause); unfunded ⇒ rendezvous parks even with a sender present | **confirmed** — `signed_bind_funded_rendezvous_fires_with_one_token`, `signed_bind_blocks_the_rendezvous_without_fuel`, `signed_join_{fires_when_every_clause_is_funded,blocks_when_one_clause_is_unfunded}`, `signed_and_plain_binds_mix_in_one_join` |
