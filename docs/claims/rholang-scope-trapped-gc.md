# Claim: scope-trapped unforgeable name safety

```yaml
claim_id: CLAIM-RHOLANG-001
artifacts:
  - rholang/src/rust/interpreter/garbage.rs     # H1 classifier (is_scope_trapped)
  - rholang/src/rust/interpreter/reduce.rs      # eval_new: trapped-set capture
  - rholang/src/rust/interpreter/rho_runtime.rs # create_checkpoint: sweep call site
  - rspace++/src/rspace/rspace/ops_sweep.rs     # locked_sweep: the delete pipeline
  - rspace++/src/rspace/rspace_interface.rs     # sweep_channels ISpace method
status: tla-bounded, rocq-pending
adapter: agentic
mechanization:
  tlaplus: formal/tlaplus/scope_trapped_gc
  rocq: formal/rocq/scope_trapped_gc
references:
  - docs/rholang/garbage-collection.md
  - docs/rholang/garbage-collection-formalization-plan.md
```

## Context

`is_scope_trapped(idx, par)` (`rholang/src/rust/interpreter/garbage.rs`)
classifies a fresh `GPrivate` name `N` minted by a `new x in P` as a
*scope-trapped unforgeable* — one whose rspace entries can safely be
retired at checkpoint time. The sweep in
`rspace++/src/rspace/rspace/ops_sweep.rs` deletes every rspace entry
keyed on such `N`.

The informal safety argument (`docs/rholang/garbage-collection.md`) is
an induction on reduction steps: if every syntactic occurrence of `x`
in `P` sits in a top-level channel position, then no reduction from
`P[N/x]` ever lands `N` in a payload or a stored receive body on a
non-trapped channel, so the swept entries can only ever have been
deadlocked. The TLA+ model (`formal/tlaplus/scope_trapped_gc/`) checks
this on bounded instances — one post-fix config clean, two pre-fix
controls each violating their named invariant.

This claim is the **universal, axiom-free Rocq counterpart** of that
bounded check. The flaw in my original prose proof (forgetting the
"fire a parked receive" reduction rule) is why machine-checking the
invariant matters: Rocq's induction tactics require every reduction
rule to be covered.

## Claim statements

**C1 — Invariant preservation.** For every Rholang term `P` and every
bound-variable index `x`, if `is_scope_trapped x P = true` at the
reduction of `new x in P`, then in every state reachable from
`(empty_rspace, P[N/x])` under the operational semantics:

- `N` never appears in a `Send.data` position of any term in the
  live process or in any parked continuation body on a non-trapped
  channel.
- `N` never appears inside any `Receive.body` of any term in the live
  process or in any parked continuation body on a non-trapped channel.
- No rspace entry keyed on `N` could ever match an operation from
  outside the closure of the original `new x in P`, because by the
  two clauses above no process ever holds `N` as a value.

**C2 — Sweep observational equivalence.** For any state `(ρ, P)` that
satisfies C1 for every trapped `N`, the state `(ρ \ { entries keyed on
any trapped N }, P)` is observationally equivalent to `(ρ, P)` — i.e.,
every trace from the swept state is also a trace from the unswept
state and vice versa. This is the soundness of the deletion.

**C3 — Determinism of the classifier (seam premise).** `is_scope_trapped`
is a pure function of the normalized `Par` AST and the bound-variable
index. The `locally_free` cache the classifier reads is correct for
every `Par` produced by the Rholang normalizer: the cached bitset is
exactly the set of free-variable indices occurring in the term.

## Seam premises (assumed, enforced by Rust)

- **S1 — `locally_free` cache correctness.** For every `Par` produced
  by the Rholang normalizer, the cached `locally_free: Vec<u8>` bitset
  flags exactly those de Bruijn indices that occur free in the term.
  Enforced by the normalizer's cache invariants
  (`rholang/src/rust/interpreter/compiler/normalizer/`).

- **S2 — Fresh-name uniqueness.** `Blake2b512Random::next` yields
  pairwise distinct 32-byte ids modulo cryptographic collision
  resistance. Enforced by `eval_new`
  (`rholang/src/rust/interpreter/reduce.rs:1381`). Rocq models `N` as
  an abstract constant and quantifies universally over it; the
  cryptographic uniqueness is a separate fact that does not enter
  the proof.

- **S3 — Classifier/sweep agreement.** The set of ids swept at
  checkpoint is exactly the set the classifier recorded at
  `eval_new` time. Enforced by `drain_trapped()` being the only
  producer of the sweep input
  (`rholang/src/rust/interpreter/rho_runtime.rs`).

## Verification obligations

1. Formalize the Rholang-flavoured syntax (de Bruijn) and
   capture-avoiding substitution.
2. Define `is_scope_trapped` as a structural recursive predicate
   mirroring the Rust walk.
3. Define the small-step operational semantics with four rules:
   `AllocFresh`, `ParkSend`/`ParkReceive`, `Comm`, and
   `ExternalFire` (an environmental event firing a parked receive
   on a non-trapped channel — the rule the prose proof omitted).
4. Define the invariant `Inv_SafeOf N (ρ, P)` as the three
   conjuncts of C1.
5. Prove `scope_trapped x P -> Inv_SafeOf N (empty_rspace, subst N x P)`.
6. Prove invariant preservation under each reduction rule, inducting
   on the step relation.
7. Close the main theorem: `scope_trapped x P -> forall ρ' P',
   (empty, subst N x P) ->* (ρ', P') -> Inv_SafeOf N (ρ', P')`.
8. Prove C2 as a corollary: deleting any rspace entry keyed on `N`
   from a state satisfying `Inv_SafeOf N` yields a state that is
   step-equivalent.
9. Zero `Admitted`. No custom `Axiom` or `Parameter`. Only `Section
   Variable`s are the three seam premises.

## Mechanization

The mechanization lives in `formal/rocq/scope_trapped_gc/` with two
theories:

- `ScopeTrapped.v` — syntax, substitution, classifier, operational
  semantics, invariant, main theorem, sweep-soundness corollary.
- `MainTheorem.v` — closed capstone aliases, with the three seam
  premises surfaced as `Section Variable`s so the check script can
  assert each capstone is "Closed under the global context".

The build gate is `scripts/check-scope-trapped-gc-ALL.sh` (new,
mirrors the existing `scripts/check-rspace-guards-ALL.sh`).

## Known limitations

- **Single-binder only.** The mechanization is scoped to `new x in P`
  with one binder. Multi-binder `new x, y, ... in P` is a
  straightforward induction on binder count; tracked as a follow-up.
- **No match/bundle/expression sub-languages.** The mechanized model
  covers `Nil`, `Send`, `Receive`, `New`, `Par`. The Rust classifier
  also covers `Match`, `Bundle`, `Expr`, `Connective`, `If`; those
  positions are either channel-safe (handled by the same argument)
  or value-context (always unsafe, requiring no extra lemmas). Adding
  them widens the structural recursion but does not change the proof
  shape.
- **No joins.** Single-source receives only. Join cleanup in the Rust
  sweep (`ops_sweep.rs` step 3) is covered by a separate, simpler
  argument: if any source channel of a join is trapped, the join's
  parked continuation is unreachable via COMM, so the sweep's
  deletion is sound by the same argument applied to each source
  independently.
