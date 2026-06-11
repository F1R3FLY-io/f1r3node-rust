# Phase 2 — Combined-Cell Tokens & Splitter: Faithfulness Analysis

> The intra-program non-interference / faithfulness analysis the plan requires
> **before** the splitter/joiner code. It states the R3/R5 semantics precisely,
> identifies the hazard, fixes the realization, and characterizes the residual
> boundary rigorously.
>
> **Note on ring-fencing.** An earlier draft bundled *located purses*
> `purse(I,…)` into Phase 2. Per the user decision aligning with Greg's
> `app:concrete` (no located form), ring-fencing was **superseded** by the
> **binding-sensitive `Σ`** — a Phase-1 feature, realized + verified — see §4.
> Combined-cell tokens (R3/R5) remain the genuine Phase-2 item.

## 1. The five rules (from `cost-decoration@937a6335`)

```
R1  K{ {L}_s,        s:S }            ~> K{ R, S }            -- single sig, single token  (Phase 1)
R2  K{ {L}_{s1*s2},  s1:S1, s2:S2 }   ~> K{ R, S1, S2 }       -- compound sig, SPLIT tokens (Phase 1: nested gates)
R3  K{ {L}_{s1*s2},  (s1*s2):S }      ~> K{ R, S }            -- compound sig, COMBINED token  ← Phase 2
R4  K{ {A}_s1,{B}_s2, s1:S1, s2:S2 }  ~> K{ R, S1, S2 }       -- split procs, split tokens (Phase 1: independent gates)
R5  K{ {A}_s1,{B}_s2, (s1*s2):S }     ~> K{ R, S }            -- split procs, COMBINED token  ← Phase 2
```

Phase 1 realizes R1/R2/R4: every fuel gate listens on **component** channels
(`Σ⟦s⟧`, or `Σ⟦s1⟧` then `Σ⟦s2⟧` nested), and tokens are sends whose payload is
the *remaining stack* (`K⟦s::S⟧ = Σ⟦s⟧!(K⟦S⟧)`). A multi-layer stack is thus a
**chain** — `a :: b :: () = Σ⟦a⟧!(Σ⟦b⟧!(Nil))` — that the nested gate threads
through: the outer gate binds the a-token, `*t1` re-releases its payload (the
b-token), the inner gate binds that, `*t2` releases `Nil`. Fuel flows correctly.

R3/R5 differ only in that the funder holds **one combined token** on the
compound channel `Σ⟦s1∘s2⟧` (`a (*) b :: () = Σ⟦a∘b⟧!(Nil)` — already minted
correctly by `signature_to_ir(Compound)`), instead of split tokens. The gates
still listen on the **components**, so a combined token would otherwise sit
unconsumed (deadlock). The **splitter** bridges the two.

## 2. The splitter

Per distinct compound signature `s1∘…∘sn` (surface `s1 (*) … (*) sn`) that
appears as a combined-cell token (a token-stack layer that is a `Compound`),
install **one persistent contract**:

```
for( c <= Σ⟦s1∘…∘sn⟧ ){ Σ⟦s1⟧!( Σ⟦s2⟧!( … Σ⟦sn⟧!( *c ) … ) ) }
```

It consumes a combined token and re-emits the **chained split form** (the same
shape a split stack `s1 :: … :: sn :: ()` would have, but tailed by `*c` = the
combined token's remaining stack instead of `Nil`). The Phase-1 nested compound
gate (R3) or the separate single gates (R5) then thread through that chain
exactly as in §1. The remaining stack `S` is released **exactly once** (it rides
the innermost send `Σ⟦sn⟧!(*c)`), matching `~> R, S`.

* **Installed** at the production chokepoint `Compiler::normalize_term`, after
  `normalize_ann_proc` and before the final `sort_match`, by collecting the
  distinct compound stack-layer sigs from the program AST (a local `BTreeSet`
  keyed by `Sig::key`, no thread-locals). Deduped + replicated. Tests that call
  `normalize_ann_proc` directly bypass this pass; combined-token tests go
  through `Compiler::source_to_adt`.

## 3. Faithfulness characterization (the boundary, stated honestly)

**Theorem (fuel conservation).** Each splitter firing consumes exactly one
combined token `(s1*…*sn):S` and produces exactly one fuel unit per component
(`s1,…,sn`), with the remaining stack `S` threaded through and released once.
No fuel is created or destroyed.

*Proof.* The contract is linear in `c`: one input send consumed, one output
chain emitted; `*c` (the payload `K⟦S⟧`) appears once, at the chain's tail. ∎

**Completeness for R3/R5.** A combined token funds (a) a compound gate
`{L}_{s1*…*sn}` — R3, the nested gate threads the chain — and (b) separate gates
`{A}_s1 | … | {B}_sn` — R5, each consumes its component from the chain. ✓

**Residual boundary (more permissive than the calculus).** The splitter is
*eager*: it fires whenever a combined token exists, regardless of which consumer
is waiting. So a combined token's components become **fungible** — they may fund
*any* matching component gate, not only a single atomic R3/R5 redex. The
calculus consumes a combined token **atomically** (R3/R5 match the whole token);
the transpiler admits interleavings where the halves fund different (fungible)
redexes. This is **cost-sound** — the authorized `sᵢ`-fuel is spent on `sᵢ`-work,
and total fuel is conserved — but strictly more permissive than the atomic
rules. The native reducer enforces atomicity via the linear resource proof
(paper §3.x); the transpiler approximates it with cost-conservative fungibility.
This is the documented stopgap boundary.

> **Why not a global joiner (split→combined)?** A persistent joiner
> `for(t1<=Σ⟦s1⟧ & t2<=Σ⟦s2⟧){ Σ⟦s1*s2⟧!(…) }` is *eager* in the other
> direction: it would consume split tokens that the calculus lets fund R1
> single-wraps, **removing funding options** and risking deadlock — a worse,
> non-conservative infidelity. We therefore install the splitter only
> (combined→split), never a joiner. A split stack already funds compound gates
> directly (R2/R4), so no joiner is needed.

## 4. Ring-fencing — binding-sensitive `Σ` (superseded the located purse)

The original Phase-2 sketch ring-fenced fuel with a **located purse**
`purse(I, s::S)` (paper Def. *located stack* §576, *funding slots* §593): each
layer minted on `Σ_I⟦sᵢ⟧ = @( *I ∣ *Σ⟦sᵢ⟧ )` — the sort-canonical parallel
composition of an unforgeable identity name `I` with the component supply
channel — so only a holder of `I` could consume there, and `near(I,J)` was
channel equality. That channel is **open** (it carries `I`'s `locally_free`),
which forced a `new I` enclosing the purse and complicated the closed-`Par`
story.

Greg's `app:concrete` has **no located form** (a stack is just a process;
parallel stacks are ordinary `S1 | S2`), so ring-fencing moved to the
**binding-sensitive `Σ`** (see `transpiler.md` §2, `sig.rs`):

* `signature_to_ir` resolves a ground sig `s` against the gate's enclosing
  `bound_map_chain`. A **`new`-bound** `s` is content-addressed by its
  **binder's identity** (source span) over `DOMAIN_BOUND` → `Sig::Bound`; a
  **free** `s` keeps content-by-spelling over `DOMAIN_GROUND` → `Sig::Ground`.
* The resulting channel is **closed** (`GPrivate` over a `Blake2b256` hash —
  empty `locally_free`), so — unlike `Σ_I⟦s⟧` — it needs **no** program-level
  binding pass and composes with the rest of Phase 1 trivially.

**Soundness (ring-fencing).** A token minted under `new s` sits on
`Σ⟦bound s⟧`, keyed by that binder. A gate signing a **free** `s` listens on
`Σ⟦free s⟧ ≠ Σ⟦bound s⟧` (distinct domains); a gate under a **different**
`new s` listens on a binder-distinct channel; only a gate **in the same `new s`
scope** resolves the same channel and can consume. So fuel is isolated to its
binder's scope — the object-capability the located purse specified, now realized
by α-distinct content-addressing rather than an open par with `*I`. The global
pool (`Σ⟦free s⟧`) and other scopes cannot reach it.

**Faithfulness vs the located purse.** The two agree on the security property
(isolation by an unforgeable owner) and differ only in mechanism: located = open
channel keyed by a runtime name `I`; new-bound = closed channel keyed by the
*binder*. The new-bound form is **strictly cleaner** (closed `Par`, no slot
surface, no `near` channel-equality side condition), and — unlike the located
purse, which was deferred — it is **realized and verified in Phase 1**.

## 5. Verification

`Compiler::source_to_adt` end-to-end tests: combined-token funds compound gate;
combined-token funds separate gates; splitter dedup; determinism. **Ring-fencing**
is covered by the structural + reduction tests: a `new`-bound sig mints on a
binder-keyed channel ≠ the global channel; the same binder shares a channel while
distinct binders are disjoint; a free sig mints on the global channel
(`new_bound_signature_mints_on_a_ring_fenced_channel`,
`same_binder_shares_a_channel_distinct_binders_are_disjoint`,
`a_free_signature_mints_on_the_global_channel`; end-to-end
`new_bound_signature_funds_an_in_scope_gate`,
`new_bound_fuel_is_ring_fenced_from_a_free_sig_gate`,
`free_shared_signature_still_rendezvous`).
