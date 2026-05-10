# Slashing — Formal Verification

**Version 1.0 · 2026-05-01**

> **Abstract.** This document is the proof artifact accompanying
> `slashing-specification.md`. It states every theorem in mathematical
> prose translated from the Rocq mechanization at
> `formal/rocq/slashing/theories/`, and integrates the TLA+ correctness
> model from `formal/tlaplus/slashing/`. Every load-bearing claim of the
> specification is proven here.
>
> The development is **closed under the global context**: every theorem
> from `main_bisimilarity_theorem` downward depends only on Rocq's
> standard library and the slashing theories — zero `Admitted`, zero
> custom `Axiom`. This is verified via `Print Assumptions` (§12.2).

---

## Table of Contents

1. [Introduction](#1-introduction)
2. [Glossary](#2-glossary)
3. [Labeled transition system](#3-labeled-transition-system)
4. [Equivocation detection — semantics and correctness](#4-equivocation-detection--semantics-and-correctness)
5. [EquivocationRecord — algebraic structure](#5-equivocationrecord--algebraic-structure)
6. [The PoS slash effect](#6-the-pos-slash-effect)
7. [Two-level slashing closure](#7-two-level-slashing-closure)
8. [Bisimilarity Rust ~~ Scala (modulo bug fixes)](#8-bisimilarity-rust--scala-modulo-bug-fixes)
9. [Bug-fix proofs](#9-bug-fix-proofs)
10. [TLA+ correctness model](#10-tla-correctness-model)
11. [Module reference](#11-module-reference)
12. [Trust base](#12-trust-base)
13. [References](#13-references)
14. [Traceability](#14-traceability)

---

## 1 · Introduction

### 1.1 Problem and contribution

This document gives the *machine-checked* counterpart to the normative
specification in `slashing-specification.md`. The specification fixes
behavior; the verification proves the implementation aligns.

The contribution split:

| Specification doc                          | Verification doc (this)                         |
|--------------------------------------------|-------------------------------------------------|
| Components, semantics, examples, use cases | Theorem statements, prose proofs, Rocq pointers |
| What the system should do                  | Why we believe the system does it               |
| Read by implementers, auditors             | Read by formal-methods reviewers, certifiers    |

### 1.2 Pedigree table (per §1.5 of cost-accounting precedent)

| Class                                     | Theorems                                                                                                                                                           |
|-------------------------------------------|--------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| **(a)** Direct mechanizations             | `bm_slash`, `bm_lookup`, `equivocates_b`, `is_slashable`, `detect`, `slash`, `prepare_slashing_deploys`, `filter_slashed`, `slash_step`, `atomic_record_or_update` |
| **(b)** Verifications of paper algorithms | T-1, T-2, T-3, T-4, T-5, T-6, T-7, T-8, T-Idem (slash idempotence; alias T-9), T-10                                                                                |
| **(c)** Proof-original extensions         | T-11, T-12, T-13, T-14, T-15, T-9.1–T-9.15 (including T-9.10' and T-9.10″)                                                                                          |
| **(d)** Citable-axiom-gated               | None — all theorems are closed under the global context                                                                                                            |

### 1.3 Scale and module DAG

22 Rocq modules, ~3,800 lines total. The dependency DAG matches the one
in `_CoqProject` (see also `slashing-specification.md` §1.7):

```
                 ┌──────────────┐
                 │  Validator   │
                 └──────┬───────┘
                        │
        ┌───────────────┼───────────────┐
        ▼               ▼               ▼
   ┌─────────┐    ┌──────────┐    ┌────────────┐
   │  Block  │    │  PoSCtrt │    │  EqRec     │
   └────┬────┘    └────┬─────┘    └─────┬──────┘
        │              │                │
        ▼              │                ▼
   ┌──────────┐        │         ┌──────────────┐
   │ InvBlock │        │         │   DAGState   │
   └────┬─────┘        │         └──────┬───────┘
        │              │                │
        └──────────────┴────────────────┤
                                        ▼
                          ┌──────────────────────────┐
                          │   EquivocationDetector   │
                          └─────────────┬────────────┘
                                        │
                ┌───────────────────────┼─────────────────────┐
                ▼                       ▼                     ▼
        ┌──────────────┐         ┌─────────────┐     ┌─────────────────┐
        │ SlashDeploy  │         │ BlockCreator│     │ TwoLevelSlashing│
        └───────┬──────┘         └──────┬──────┘     └─────────────────┘
                │                       │
                └────┬──────────────────┘
                     ▼
        ┌────────────────────────┐
        │   ForkChoice           │
        └────────────┬───────────┘
                     │
                     ▼
        ┌────────────────────────┐
        │   Bisimulation         │
        └────────────┬───────────┘
                     │
                     ▼
        ┌────────────────────────┐
        │   BugFix*  (9 modules) │
        └────────────┬───────────┘
                     │
                     ▼
        ┌────────────────────────┐
        │   MainTheorem          │
        └────────────────────────┘
```

---

## 2 · Glossary

This glossary mirrors §2 of `slashing-specification.md` with formal
references added.

### 2.0 Acronyms

| Acronym   | Expansion                         | First-use context                                                       |
|-----------|-----------------------------------|-------------------------------------------------------------------------|
| **PoS**   | Proof of Stake                    | Consensus family this work verifies.                                    |
| **BFT**   | Byzantine Fault Tolerance         | Bound `f < n/3` from [LSP82] (§5, T-12).                                |
| **CBC**   | Correct-by-Construction (Casper)  | Consensus model.                                                        |
| **DAG**   | Directed Acyclic Graph            | The block graph (DAGState).                                             |
| **LMD**   | Latest-Message-Driven (GHOST)     | Fork-choice rule (§7, T-10).                                            |
| **RMW**   | Read-Modify-Write                 | Atomic primitive bug #2 protects (T-9.2).                               |
| **TLC**   | TLA+ model checker                | State-space exploration (§10, §10.6).                                   |
| **LTS**   | Labeled Transition System         | The slashing pipeline `T = (S, L, →)` (§3).                             |
| **GHOST** | Greedy Heaviest-Observed Sub-Tree | Fork-choice rule [LSZ15] (§7).                                          |
| **FFG**   | Friendly Finality Gadget (Casper) | Ethereum 2.0 slashing comparison [BG19].                                |
| **DOS**   | Denial of Service                 | Vector closed by bug fix #1 (T-9.1).                                    |
| **KV**    | Key-Value                         | Store abstraction underlying the equivocation tracker (§5).             |
| **BFS**   | Breadth-First Search              | Generic graph traversal; T-9.7 now specifies a canonical self-chain walk. |
| **TLA+**  | Temporal Logic of Actions         | Specification language (§10).                                           |
| **OOM**   | Out of Memory                     | TLC heap-exhaustion outcome during liveness-graph construction (§10.6). |

### 2.1 Symbols

| Symbol         | Rocq name                                | Meaning                                               |
|----------------|------------------------------------------|-------------------------------------------------------|
| `V`            | `Validator := nat`                       | Validator identities                                  |
| `H`            | `BlockHash := nat`                       | Block hashes (decidable)                              |
| `B`            | `Block` (record)                         | Blocks: sender, seq, hash, justifications, slash flag |
| `B(v)`         | `bm_lookup bm v`                         | Bond of validator `v`                                 |
| `EqRec`        | `EqRec` (record)                         | Equivocation evidence                                 |
| `D, I, E, B`   | `DAGState` (record)                      | DAG snapshot                                          |
| `slash(ps, v)` | `slash : PoSState → V → PoSState × bool` | PoS slash transition                                  |
| `~_b`          | `bonds_bisim`                            | Bond-map bisimulation                                 |
| `~_r`          | `records_bisim`                          | Records bisimulation (modulo iter order)              |
| `~_s`          | `slashed_bisim`                          | Slashed-set bisimulation (mutual containment)         |

### 2.2 Notation

|                                   |                                                     |                            |
|-----------------------------------|-----------------------------------------------------|----------------------------|
| `→`                               | LTS transition (single step)                        |                            |
| `→*`                              | LTS transition (multi-step)                         |                            |
| `~`                               | Strong bisimilarity                                 |                            |
| `≈`                               | Weak bisimilarity                                   |                            |
| `≡_α`                             | α-equivalence (modulo bound-name renaming)          | [MR05a]                    |
| `↓ℓ`                              | Barb (state can immediately perform observable `ℓ`) |                            |
| `⇓ℓ`                              | Weak barb (perform `ℓ` after some `τ`-steps)        |                            |
| `≈ₓ`                              | Weak barbed equivalence mod barbs `x`               |                            |
| `⊥`                               | Boolean false / terminal absorbing state            |                            |
| `⊤`                               | Boolean true                                        |                            |
| `⟹`                               | Logical implication                                 |                            |
| `∀`                               | Universal quantifier                                |                            |
| `∃`                               | Existential quantifier                              |                            |
| `∅`                               | Empty set / nil                                     |                            |
| `∈`                               | Set membership                                      |                            |
| `⊆`                               | Subset (inclusive)                                  |                            |
| `≡`                               | Equivalence (mutual containment)                    |                            |
| `=`                               | Strict equality                                     |                            |
| `incl A B`                        | List `A` is included in list `B`                    | Rocq stdlib                |
| `NoDup A`                         | List `A` has no duplicates                          | Rocq stdlib                |
| `Closed under the global context` | No axioms used                                      | `Print Assumptions` output |

---

## 3 · Labeled transition system

The slashing layer is a labeled transition system

```
  T = (S, L, →)
```

where:

- **States** `S = (D, I, E, B, A, Sl, C)` carry the DAG `D ⊆ Block`, the
  invalid-block index `I ⊆ H`, the equivocation records `E ⊆ EqRec`, the
  bond map `B : V → ℕ`, the active set `A ⊆ V`, the slashed set
  `Sl ⊆ V`, and the Coop-vault balance `C ∈ ℕ`.
- **Labels** `L = {sign(v,s,b), detect(v,s,d) → ib, record(v,sn),
  propose(p, deploys), executeSlash(o, ok), filterFC(v),
  neglectDetect(v, target)}`.
- **Transitions** `→` are defined per-label by the corresponding
  component in `formal/rocq/slashing/theories/`.

### 3.1 Reduction rules

Each rule has the form

```
   premise₁    premise₂    …
   ────────────────────────────  [LABEL]
                  s →ᴸ s'
```

For brevity we list the most consequential rules; the full set is in the
Rocq source.

```
   b ∉ D                                          [SIGN]
   ──────────────────────────────────
   (D,I,E,B,A,Sl,C) →ˢⁱᵍⁿ⁽ᵛ,ˢ,ᵇ⁾ (D ∪ {b}, I, E, B, A, Sl, C)
```

```
   equivocates(D, v, s)    requestedAsDep(D, b)    [DETECT-ADM]
   ──────────────────────────────────────────────
   s →ᵈᵉᵗᵉᶜᵗ⁽ᵛ,ˢ,d=⊤⁾→ᴬᵈᵐⁱˢˢⁱᵇˡᵉ s
```

```
   sd ∈ deploys    sd targets offender
   bm_lookup(B, offender) > 0                       [SLASH]
   ────────────────────────────────────
   s →ᵉˣᵉᶜᵘᵗᵉˢˡᵃˢʰ⁽ᵒᶠᶠᵉⁿᵈᵉʳ,⊤⁾
       (D, I, E,
        bm_slash(B, offender),
        A \ {offender},
        Sl ∪ {offender},
        C + bm_lookup(B, offender))
```

The Rocq formalization realizes each rule as a function (e.g. `slash : PoSState → V → PoSState × bool` for `[SLASH]`). The TLA+ specifications
(§10) encode them as `Next` action disjuncts.

---

## 4 · Equivocation detection — semantics and correctness

### 4.1 Definitions

**Definition 4.1** *(Equivocation in the DAG).*
For a DAG state `S = (D, …)`, validator `v`, and sequence number `n`,

```
equivocates(S, v, n) ⇔
  ∃ b₁, b₂ ∈ D.
     sender(b₁) = sender(b₂) = v
   ∧ seq(b₁) = seq(b₂) = n
   ∧ hash(b₁) ≠ hash(b₂)
```

Rocq: `equivocates : DAGState → Validator → nat → Prop`
in `theories/DAGState.v:106`. The boolean version `equivocates_b` is
proven equivalent (`equivocates_dec` at line 109). Witness extraction
`equivocates_witnesses` (line 177) is used by all completeness proofs.

**Definition 4.2** *(Detector).*
The detector classifies an arrival as one of four statuses:

```
detect(S, v, n, d) =
  ⎧ DSValid       if ¬equivocates(S, v, n)
  ⎨ DSAdmissible  if  equivocates(S, v, n) ∧ d = ⊤
  ⎩ DSIgnorable   if  equivocates(S, v, n) ∧ d = ⊥
```

where `d` indicates whether the arriving block was requested as a
dependency. Rocq:
`detect : DAGState → Validator → nat → bool → DetectorStatus`
in `theories/EquivocationDetector.v:66`.

### 4.2 Theorem 4.1 (T-1, Detection soundness)

**Statement.** *(`detection_sound`, `EquivocationDetector.v:91`.)*
For every state `S`, validator `v`, sequence `n`, dependency flag `d`,
and status `s` returned by the detector,

```
  detect(S, v, n, d) = s ∧ s ∈ {DSAdmissible, DSIgnorable}
    ⟹ equivocates(S, v, n)
```

**Proof.** By case analysis on the case of `equivocates_b S v n`:

- If `equivocates_b S v n = true`: then `equivocates(S, v, n)` holds
  by reflection.
- If `equivocates_b S v n = false`: then `detect` returns `DSValid`
  regardless of `d`, contradicting `s ∈ {DSAdmissible, DSIgnorable}`.

The Rocq script destructs on `equivocates_b` and uses `discriminate` to
close the contradictory branch. ∎

### 4.3 Theorem 4.2 (T-2, Detection completeness)

**Statement.** *(`detection_complete`, `EquivocationDetector.v:111`.)*
For every state `S`, validator `v`, sequence `n`, and flag `d`,

```
  equivocates(S, v, n) ⟹
    detect(S, v, n, d) = DSAdmissible ∨ detect(S, v, n, d) = DSIgnorable
```

**Proof.** From `equivocates(S, v, n)` we have
`equivocates_b S v n = true` by reflection. Thus `detect` enters its
true-branch. Case analysis on `d`: returns `DSAdmissible` if `d = ⊤`,
`DSIgnorable` otherwise. ∎

A stronger version `detection_complete_strong` (line 123) makes the
return value explicit:

```
  equivocates(S, v, n) ⟹
    detect(S, v, n, d) = (if d then DSAdmissible else DSIgnorable)
```

### 4.4 Theorem 4.3 (T-3, Slashable taxonomy correctness)

**Statement.** *(`slashable_post_fix_extends_pre_fix`, `InvalidBlock.v:151`.)*
For every `ib : InvalidBlock`,

```
  is_slashable_pre_fix(ib) = ⊤ ⟹ is_slashable(ib) = ⊤
```

**Proof.** Exhaustive case analysis on the 26-element `InvalidBlock`
inductive: each variant where `is_slashable_pre_fix` returns `true` also
has `is_slashable` returning `true`. ∎

The diagonal theorem `slashable_diff_only_ignorable` (line 160) shows
that the two predicates differ only on `IBIgnorableEquivocation`.

### 4.5 Theorem 4.4 (T-6, `detect_neglected` soundness)

**Statement.** *(`detect_neglected_sound`, `EquivocationDetector.v` §6.)*
For every state `S`, validator `v`, sequence `n`, dependency flag `d`,
and record store `records`,

```
  detect_neglected(S, v, n, d, records) = DSNeglected ⟹
    d = ⊤ ∧ has_key(records, (v, n−1)) = ⊤
```

**Proof.** Case analysis on `d ∧ has_key(records, (v, n−1))`. Only when
both conjuncts are `⊤` does `detect_neglected` return `DSNeglected`. ∎

### 4.6 Theorem 4.5 (T-6, `detect_neglected` completeness)

**Statement.** *(`detect_neglected_complete`, `EquivocationDetector.v` §6.)*

```
  has_key(records, (v, n−1)) = ⊤ ⟹
    detect_neglected(S, v, n, ⊤, records) = DSNeglected
```

**Proof.** Direct unfolding. ∎

This closes the prior gap where the Neglected path of the detector had
no soundness/completeness theorem.

### 4.7 Theorem 4.6 (T-9.11, fixed latest-message detectability)

**Statement.** *(`fixed_detectable_*`, `EquivocationDetector.v` §6.)*
For a latest-message view contribution list `view`, the fixed detector
uses:

```
detectable(view) ≜ detected_hash_seen(view) ∨ |distinct_child_hashes(view)| ≥ 2
```

The mechanized lemmas establish four concrete obligations:

- `fixed_detectable_missing_pointer_prefix`: prepending `∅` does not
  change the verdict.
- `fixed_detectable_detected_hash_true`: a previously detected hash is
  decisive.
- `fixed_detectable_duplicate_single_child_false`: two paths to the same
  child do not form two-child evidence.
- `fixed_detectable_two_distinct_children_true`: two distinct child
  hashes are sufficient.

**Proof.** Direct computation over the inductive contribution list and
`nodup Nat.eq_dec` for child-hash canonicalization. No axioms or
admissions are used. ∎

The production Rust path realizes this theorem by projecting
justifications into deterministic validator order and scanning the
resulting contribution list iteratively. Missing direct or nested
pointers map to `∅`; duplicate paths are normalized by child hash before
the `|distinct_child_hashes(view)| ≥ 2` test.

---

## 5 · EquivocationRecord — algebraic structure

### 5.1 Theorem 5.1 (T-4, Record monotonicity)

**Statement.** *(`t_4_record_monotone_update`,
`EquivocationRecord.v:254`.)* For every store `s`, key `k`, hash `h`,
and other key `k'`,

```
  hashes_at_key(s, k') ⊆ hashes_at_key(update_record(s, k, h), k')
```

**Proof.** Case analysis on whether `k' = k`:

- **k' = k**: by `find_update_same_present` (line 201), the lookup
  yields a record `r₁` with `er_hashes(r₁) ⊇ er_hashes(r₀)` where `r₀`
  was the original record (or empty if absent). Inclusion holds
  pointwise.
- **k' ≠ k**: by `find_update_other_record` (line 149), the lookup is
  unchanged; so the hashes set is identical, hence trivially included.

The sister theorem for `insert_cond` (line 236,
`t_4_record_monotone_insert_cond`) follows the same pattern:
`insert_cond` either no-ops (key present) or prepends a new record (key
absent), in both cases preserving existing hashes. ∎

The Rust-source traceability hook is
`current_rust_record_update_retains_all_detected_hashes`: for every store
`s`, key `k`, and new hash `h`, all hashes at `k` before
`update_record(s,k,h)` remain at `k` after the update. This is the formal
counterpart of the current detector path, which clones the existing record
and inserts the new detected hash rather than replacing the set.

### 5.2 Theorem 5.2 (T-5, Record uniqueness)

**Statement.** *(`t_5_insert_cond_preserves_unique`,
`EquivocationRecord.v:296`.)* For every store `s` with unique keys and
record `r`,

```
  unique_keys(s) ⟹ unique_keys(insert_cond(s, r))
```

**Proof.** Case analysis on `has_key s (er_key r)`:

- **Present**: `insert_cond` is a no-op; uniqueness preserved by hypothesis.
- **Absent**: prepending `r` creates a new list `r :: s`. We must show
  `er_key r ∉ map er_key s`. Suppose for contradiction `er_key r =
  er_key r'` for some `r' ∈ s`; then by lemma
  `in_with_key_implies_has_key` (line 279), `has_key s (er_key r) = ⊤`,
  contradicting the absent hypothesis.

The companion `t_5_update_record_preserves_unique` (line 336) uses
`in_update_record_implies_in` (line 314) to lift the keys-in-update
guarantee. ∎

---

## 6 · The PoS slash effect

### 6.1 Theorem 6.1 (T-7, Slash zeros bond)

**Statement.** *(`slash_zeros_bond`, `PoSContract.v:75`.)* For every
PoS state `ps` and offender `v`,

```
  let (ps', _) := slash(ps, v) in
    bm_lookup(ps_allBonds(ps'), v) = 0
```

**Proof.** Case analysis on whether `bm_lookup(ps_allBonds(ps), v) = 0`:

- **Already zero**: `slash` is a no-op; the lookup is still 0.
- **Positive**: the new state's bond map is `bm_slash(ps_allBonds(ps),
  v)`. By foundational lemma `bm_slash_lookup` (`Validator.v:154`),
  `bm_lookup(bm_slash(b, v), v) = 0`. ∎

### 6.2 Theorem 6.2 (T-8, Slash transfers stake)

**Statement.** *(`slash_transfers_stake`, `PoSContract.v:95`.)* When the
offender's bond is positive,

```
  ps_coopVault(ps') = ps_coopVault(ps) + bm_lookup(ps_allBonds(ps), v)
```

**Proof.** In the positive-bond branch of `slash`, the new state has
`ps_coopVault := ps_coopVault(ps) + bond` by direct construction. ∎

### 6.3 Theorem 6.3 (T-Idem, Slash idempotence; alias T-9)

**Statement.** *(`slash_idempotent`, `PoSContract.v:117`.)*

```
  let (ps₁, _) := slash(ps, v) in
  let (ps₂, _) := slash(ps₁, v) in
    ps_allBonds(ps₂) = ps_allBonds(ps₁)
  ∧ ps_coopVault(ps₂) = ps_coopVault(ps₁)
  ∧ ps_active(ps₂) = ps_active(ps₁)
```

The third conjunct (`ps_active`) was added in the gap-closure pass (Audit
Gap 5): a second slash on an already-slashed validator preserves *all*
PoSState fields, not just bonds and vault.

**Proof.** After the first slash, `bm_lookup(ps_allBonds(ps₁), v) = 0`
(by T-7). The second slash hits the early-return zero-bond branch and
returns `ps₁` unchanged — including `ps_active`. ∎

### 6.4 Theorem 6.4 (T-10, Fork-choice exclusion)

**Statement.** *(`fork_choice_exclusion`, `ForkChoice.v:60`.)*

```
  bm_lookup(bonds, v) = 0 ⟹
    fc_lookup(filter_slashed(lm, bonds), v) = None
```

**Proof.** By induction on the `LatestMessages` list. In the cons case,
the head's bond is checked: if zero, the head is filtered out; if
positive, the head's validator is necessarily distinct from `v` (since
`v`'s bond is zero by hypothesis), so the find continues into the tail.
By the IH, the tail filter returns `None`. ∎

A companion theorem `fork_choice_preserves_active` (line 82) shows that
non-slashed validators' lookups are preserved.

---

## 7 · Two-level slashing closure

### 7.1 Theorem 7.1 (T-11, Level-2 termination)

**Statement.** *(`t_11_level_2_termination`,
`TwoLevelSlashing.v:126`.)* For any starting set `s₀` and neglect
graph `g`,

```
  incl s₀ universe ⟹
    incl (slash_iter universe g s₀ |universe|) universe
```

**Proof.** By induction on the iteration count. At each step, the new
slashed set is a subset of `s ∪ universe = universe`. `slash_step` is
proven `incl_left`-respecting (lemma `slash_iter_in_universe`, line
105). ∎

The full convergence-to-fixed-point property is implicit: after
`|universe|` iterations the set cannot grow further (it's bounded by
`universe`).

### 7.2 Theorem 7.2 (T-12, Quorum preservation — list-length form)

**Statement.** *(`t_12_quorum_preservation`,
`TwoLevelSlashing.v:146`.)* If `s₀` is a duplicate-free subset of
`universe`,

```
  length s₀ ≤ length universe
```

**Proof.** Direct application of `NoDup_incl_length` from the standard
library. ∎

This is the structural list-length corollary. The BFT-style claim is the
strengthening below.

### 7.3 Theorem 7.3 (T-12, BFT-style quorum preservation)

**Statement.** *(`t_12_bft_quorum_preservation`,
`TwoLevelSlashing.v` §5.)* For every `universe` and slash-closure
`closure`,

```
  NoDup(universe) ∧ NoDup(closure) ∧ closure ⊆ universe ∧
  |closure| ≤ F ⟹
    |universe| − |closure| ≥ |universe| − F
```

This is the BFT-style claim: under the protocol-level precondition
`|equivocators ∪ neglect-closure| ≤ F` (where `F = ⌊(n−1)/3⌋` per
[LSP82]), the active validator set after the slash closure has size at
least `n − F`. The corollary `t_12_bft_active_set_size` shows that with
strict `F < |universe|`, the active set is non-empty.

**Proof.** Direct from `lia` (linear arithmetic over `Nat`). The
BFT precondition is a hypothesis of the theorem, not a derived fact —
the document is honest that closure size depends on the structure of the
neglect graph, which in turn depends on the protocol's BFT assumption
about ≤ F Byzantine validators. ∎

This closes the prior gap where T-12 was only a trivial list-length
statement disconnected from the BFT framing.

### 7.4 Theorem 7.4 (Reachability characterization)

**Statement.** *(`slash_iter_reachability_characterization`,
`TwoLevelSlashing.v`.)*

```
  v ∈ slash_iter(universe, g, s₀, n)
  ⇔ v ∈ s₀ ∨
    ∃ offender k.
      offender ∈ s₀ ∧ k ≤ n ∧
      neglect_reaches_in(universe, g, k, v, offender)
```

**Proof.** The sound direction follows by induction on `n`: a newly
slashed validator must have a neglect edge into the previous slashed set,
which either names a direct offender or extends an existing path. The
complete direction follows by induction on the path length and the
time-monotonicity lemma `slash_iter_time_monotone`. ∎

### 7.5 Theorem 7.5 (Weighted active-stake quorum)

**Statement.** *(`weighted_slash_iter_quorum_preservation`,
`TwoLevelSlashing.v`.)* For any stake function,

```
  slashed_stake(universe, stake, slash_iter(...)) ≤ F
  ∧ F ≤ total_stake(universe, stake)
  ⟹ active_stake(universe, stake, slash_iter(...))
      ≥ total_stake(universe, stake) − F
```

**Proof.** By unfolding `active_stake` and `stake_quorum_bound`; the
claim is linear arithmetic over natural numbers. The theorem deliberately
takes the weighted closure bound as a hypothesis, matching T-12's
counting-style precondition. ∎

The companion theorem
`zero_stake_not_direct_offender_under_bonded_precondition` records the
eligibility precondition found by Sage: a zero-stake validator cannot be a
direct offender when all direct offenders are required to be current
bonded validators.

### 7.6 Theorem 7.6 (Current-validator and visibility filters)

**Statements.**

- `restricted_closure_only_from_current_direct_offenders`: after filtering
  direct offenders and neglect edges to the current validator universe,
  every slashed validator is justified by a current direct offender.
- `visible_unreported_graph_in`: an edge in the induced neglect graph is
  equivalent to visible evidence minus already-reported evidence.
- `visible_reachability_first_edge`: the first edge of any visibility-
  induced reachability path is visible and unreported.

**Proof.** The current-validator result specializes the reachability
characterization to `filter_validators` and `restrict_neglect_graph`.
The visibility result is a direct `filter_In` proof with
`validator_in_true`; the path statement follows by inversion on
`neglect_reaches_in`. ∎

### 7.7 Theorem 7.7 (Graph edge cases and arithmetic boundaries)

`slash_iter_graph_equiv` proves that graph-equivalent neglect functions
produce membership-equivalent closures at every iteration; duplicate
edges and edge ordering are therefore irrelevant. The theorem
`no_reachability_no_level2_slash` proves the contrapositive edge case:
if a validator is not a direct offender and has no neglect path to a
direct offender, it is not slashed. This covers disconnected cycles and
self-edge-only cases.

`slash_iter_validator_renaming_equiv` strengthens this from edge-list
equivalence to graph isomorphism: if `ρ` and `σ` are inverse renamings on
the finite validator universe, the renamed direct-offender set and
renamed neglect graph compute exactly the renamed closure. The theorem
records that numeric validator order, serialized identifier choice, and
map iteration order are not part of the slashing semantics.

The arithmetic theorems `unsigned_overflow_boundary_exact` and
`signed_overflow_boundary_exact` prove the exact `max + 1` boundary for
fixed-width projections. Rocq still reasons over exact naturals; any
implementation using bounded machine integers must either use checked
arithmetic or prove that its values never reach these boundaries.

### 7.8 Theorem 7.8 (Quorum intersection, certificates, and envelopes)

**Quorum intersection.** `quorum_intersection_by_size` proves the
counting form: if two duplicate-free active quorums both have size at
least `Q`, and `|active| < 2Q`, then they intersect. The weighted theorem
`weighted_quorum_intersection_from_disjoint_bound` states the
corresponding arithmetic form: if disjoint quorum weights would have to
sum to at most active stake, but their actual sum exceeds active stake,
then the quorums cannot be disjoint.

**Closure certificates.** `slash_iter_fixed_point_after_universe_bound`
proves that, from a duplicate-free starting set contained in a
duplicate-free universe, the closure at `|V|` iterations is a fixed point
of `slash_step`. The companion `slash_iter_fixed_point_stable` proves
that once a slash set is a fixed point, all future `slash_iter` rounds
are membership-equivalent to it. Combined with
`slash_iter_reachability_characterization`, Sage can emit shortest-path
certificates for the first slash round and Rocq proves those certificates
are sound reachability witnesses.

**Quorum-drop certificates.** `quorum_drop_certificate` and
`weighted_quorum_drop_certificate` formalize the negative witnesses:
when closure count or stake exceeds the configured fault bound, the
active count or active stake falls below the corresponding quorum bound.

**Arithmetic envelopes.** `total_stake_at_most` and
`arithmetic_safe_envelope` prove the sufficient implementation condition
used by the Sage safe-envelope model:

```
  (∀v∈V. stake(v) ≤ maxStake)
  ∧ vault + |V| * maxStake ≤ limit
  ⟹ vault + totalStake(V) ≤ limit
```

**Epoch filtering.** `epoch_filter_in` proves that the epoch-filtered
validator set contains exactly validators in the input universe whose
evidence epoch equals the current epoch.

**Batch slash order and record normalization.** The supporting modules
prove `bm_slash_many_order_independent` in `Validator.v` and
`hashes_equiv_*` in `EquivocationRecord.v`, covering batch slash
permutation independence and record meaning modulo insertion order /
duplicate hashes.

### 7.9 Theorem 7.9 (View evidence, policy boundaries, and projection risks)

**View-indexed evidence.** `view_closure_monotone_by_active_edges` proves
that a closure computed from fewer active evidence edges is contained in
the closure computed from more active evidence edges.
`view_closure_equiv_by_active_edges` proves that two observers with the
same active evidence graph compute the same closure. This is the Rocq
mirror of the Sage local-view divergence witness: divergent views are a
candidate boundary unless the protocol defines which view is canonical.

**Report suppression.** `reports_growth_shrinks_edges` and
`reported_edge_not_active` prove that reports remove active neglect
edges. Consequently, report-time closure monotonicity is not a valid
global invariant; the correct invariant is that every active neglect
edge is visible and unreported.

**Epoch identity and carryover.** `stale_epoch_not_eligible` proves that
stale evidence does not pass the current-epoch filter.
`carryover_policy_sound` isolates the alternative: stale evidence can
seed current slashing only through an explicit carryover mapping.

**Assumption counterexamples.** `closure_bound_assumption_needed`,
`quorum_intersection_strictness_needed`,
`quorum_nodup_assumption_needed`, and
`weighted_closure_bound_assumption_needed` are finite Rocq witnesses
showing that the theorem hypotheses are necessary, not proof clutter.
The Hypothesis frontier also promoted minimized examples
`hypothesis_minimized_closure_bound_assumption_needed`,
`direct_offender_universe_assumption_needed`, and
`report_suppression_hypothesis_minimized`. The deep Sage threat model
adds `deep_threat_chain_closure_bound_assumption_needed`, a four-validator
reverse-reachability chain showing why the closure-bound hypothesis is
not optional.

**Projection risks.** `bm_slash_many_abort_order_dependent` proves that
abort-on-first-failure batch slash execution is order-dependent, unlike
successful `bm_slash_many`. `er_key_injective` and
`canonical_key_pair_injective` prove the canonical record-key encoding;
`naive_record_key_projection_collision` records a non-injective
projection witness. `classify_divergence_reason` in `Bisimulation.v`
classifies evidence-view, epoch-carryover, and projection divergences as
candidate boundaries requiring review. The stateful semantic-campaign
frontier is mirrored by `semantic_campaign_boundary_reasons_require_review`;
the adversarial scheduler frontier is mirrored by
`adversarial_scheduler_boundary_reasons_require_review`; and the
expanded partition/gossip, objective-guided, Rust-replay,
precondition-fuzzing, and deep-threat classifications are mirrored by
`frontier_expansion_reasons_require_review`.

**Metamorphic properties.** `duplicate_edge_graph_equiv_hypothesis_minimized`
and `duplicate_edge_slash_iter_equiv_hypothesis_minimized` specialize
the graph-equivalence theorem to the minimized duplicate-edge witness
used by the Hypothesis frontier.

**Arithmetic projection stress.** `arithmetic_projection_stress_boundary_8bit`
records the minimized frontier witness where exact `256` differs from an
8-bit wrapping projection (`0`) and saturating projection (`255`).

---

## 8 · Bisimilarity Rust ~~ Scala (modulo bug fixes)

### 8.1 The relation R

**Definition 8.1.** *(Per-component bisimulations.)*

```
  bonds_bisim(b₁, b₂)    ⇔  ∀v. bm_lookup(b₁, v) = bm_lookup(b₂, v)
  records_bisim(s₁, s₂)  ⇔  ∀k. hashes(s₁, k) ⊆ hashes(s₂, k)
                                ∧ hashes(s₂, k) ⊆ hashes(s₁, k)
  slashed_bisim(s₁, s₂)  ⇔  s₁ ⊆ s₂ ∧ s₂ ⊆ s₁
  vault_bisim(n₁, n₂)    ⇔  n₁ = n₂
```

These are reflexive, symmetric (and the bonds and vault ones are also
transitive). Proofs in `Bisimulation.v` §2.

### 8.2 Theorem 8.1 (T-13a, Strong bisimilarity baseline — bonds projection)

**Statement.** *(`t_13_bm_slash_preserves_bonds_bisim`,
`Bisimulation.v:77`.)* For every `b₁, b₂` and offender `v`,

```
  bonds_bisim(b₁, b₂) ⟹ bonds_bisim(bm_slash(b₁, v), bm_slash(b₂, v))
```

**Proof.** Pointwise. For lookup at `v` itself, both sides return 0 by
`bm_slash_lookup`. For lookup at `v' ≠ v`, both sides return
`bm_lookup(bᵢ, v')` by `bm_slash_other`, which agree by hypothesis. ∎

### 8.3 Theorem 8.2 (T-15b, Composed bisimulation closure)

**Statement.** *(`main_bisimilarity_theorem`, `MainTheorem.v:232`.)*
For every component triple `(b₁, b₂, v₁, v₂, sl₁, sl₂, offender)` with
component-wise R-equivalence as the hypothesis,

```
  bonds_bisim(b₁, b₂)
∧ slashed_bisim(sl₁, sl₂)
∧ vault_bisim(v₁, v₂) ⟹
    bonds_bisim   (bm_slash(b₁, off), bm_slash(b₂, off))
  ∧ slashed_bisim (off :: sl₁, off :: sl₂)
  ∧ vault_bisim   (v₁ + bm_lookup(b₁, off), v₂ + bm_lookup(b₂, off))
```

**Proof.** Three sub-claims:

1. Bonds: T-13 directly.
2. Slashed: prepending the same offender preserves mutual containment
   (`t_15_slashed_append_consistent`, `Bisimulation.v:135`).
3. Vault: `v₁ = v₂` and `bm_lookup(b₁, off) = bm_lookup(b₂, off)` (by
   bonds bisimulation), so `v₁ + bm_lookup(b₁, off) = v₂ +
   bm_lookup(b₂, off)`. ∎

### 8.4 Theorem 8.3 (T-13b, Records-bisim monotonicity, Audit Gap 1 closure)

**Statement.** *(`records_bisim_monotone_update`, `Bisimulation.v:263` (§8).)*

```
  records_bisim_strong(s₁, s₂) ⟹
    ∀ k h k', incl(hashes_at_key(s₁, k'),
                   hashes_at_key(update_record(s₂, k, h), k'))
```

Where `records_bisim_strong` strengthens `records_bisim` with key
alignment: `∀ k, has_key(s₁, k) = has_key(s₂, k)`. The companion
theorem `records_bisim_strong_keys_preserved` shows key alignment is
preserved across the same update on both sides.

**Proof.** Combines bisimilarity at `k'` with `t_4_record_monotone_update`
applied to `s₂`. ∎

### 8.5 Theorem 8.4 (T-13c, Forkchoice-bisim preserves filter, Audit Gap 2 closure)

**Statement.** *(`forkchoice_bisim_preserves_filter`, `Bisimulation.v` §9.)*

```
  forkchoice_bisim(lm₁, lm₂) ∧ bonds_bisim(b₁, b₂) ⟹
    ∀ v, fc_lookup(filter_slashed(lm₁, b₁), v) =
         fc_lookup(filter_slashed(lm₂, b₂), v)
```

**Proof.** Via the helper `fc_lookup_filter_slashed` which characterizes
the filter result as the per-bond conditional. ∎

This adds the fifth `R`-component (`forkChoiceLatestMessages`) to the
bisimilarity claim, closing Audit Gap 2.

### 8.6 Theorem 8.5 (T-14, Weak barbed equivalence, Audit Gap 3 closure)

**Statement.** *(`weak_barbed_equiv` and `weak_barbed_equiv_refl`,
`Bisimulation.v` §10.)* The full observational equivalence over the five
components is

```
  weak_barbed_equiv(b₁,b₂, rs₁,rs₂, sl₁,sl₂, v₁,v₂, lm₁,lm₂)
    := bonds_bisim(b₁,b₂)
     ∧ records_bisim_strong(rs₁,rs₂)
     ∧ slashed_bisim(sl₁,sl₂)
     ∧ vault_bisim(v₁,v₂)
     ∧ forkchoice_bisim(lm₁,lm₂)
```

Companion theorems `weak_barbed_equiv_refl`, `weak_barbed_equiv_sym`,
and `weak_barbed_equiv_trans` establish reflexivity, symmetry, and
transitivity.

**Proof.** Conjunction of per-component equivalence
properties. ∎

### 8.7 Theorem 8.6 (T-15a, Pipeline composition, Audit Gap 8 closure)

**Statement.** *(`t_15_pipeline_step_preserves_R`, `MainTheorem.v` §8.)*
Define a pipeline step as the composition

```
  pipeline_step(b, rs, sl, v, lm, offender, baseSeq, h)
    := (bm_slash(b, offender),
        update_record(rs, (offender, baseSeq), h),
        offender :: sl,
        v + bm_lookup(b, offender),
        filter_slashed(lm, bm_slash(b, offender)))
```

Then under the strong bisimulation `R`, applying `pipeline_step`
consistently on both sides preserves all five components.

**Proof.** Composition of T-13, the full
`records_bisim_strong_preserved_update` theorem, the
slashed-append-consistent lemma, the vault-increment consistency lemma,
and the forkchoice-bisim filter preservation. ∎

### 8.8 Why this is the right notion

Bisimilarity at the component level matches the audit objective: two
node operators on Rust and Scala — given the same input event sequence
— observe the same bonds, the same records (modulo iter order), the
same slashed set, the same Coop-vault balance, and the same fork-choice
latest messages. Per the discussion in §13 of `slashing-specification.md`,
byte-level encoding differences are intentionally outside scope.

---

## 9 · Bug-fix proofs

Each bug-fix subsection mirrors §10 of `slashing-specification.md` and
provides a mathematical statement of the proof. All are mechanized in
`theories/BugFix*.v` and check with zero admits.

### 9.1 T-9.1 — IgnorableEquivocation safety

**Statement.** *(`post_fix_ignorable_implies_equivocation`,
`BugFixIgnorable.v:57`.)* If the detector emits `DSIgnorable`, then
`IBIgnorableEquivocation` is in the post-fix slashable set AND the DAG
witnesses a real equivocation. Hence no honest validator is wrongly
slashed.

**Proof.** Combining `ignorable_post_fix_slashable`
(`InvalidBlock.v:173`, T-3 specialization) with
`ignorable_only_on_real_equivocation` (`BugFixIgnorable.v:45`, T-1
specialization). ∎

### 9.2 T-9.2 — Atomic tracker correctness

**Statement.** *(`t_9_2_atomic_no_overwrite`,
`BugFixAtomicTracker.v:43`.)* Under the atomic operation
`atomic_record_or_update`, hash insertions never overwrite earlier
insertions.

**Proof.** Case analysis on `has_key`:
- Present: `update_record` preserves hashes by T-4.
- Absent: `insert_cond` adds an empty record; `update_record` then
  appends the hash. Both are monotone (T-4). ∎

The TLC counter-example demonstrates the failure mode under the
non-atomic (Locked = FALSE) configuration.

### 9.2′ T-9.2 (n-thread arbitrary schedule, Audit Gap 7 closure)

**Statement.** *(`t_9_2_atomic_n_threads_arbitrary`,
`BugFixAtomicTracker.v` §3.)* Define a schedule as a list of
`(validator, seqNum, hash)` operations applied via fold-left over
`atomic_record_or_update`. For any schedule of any length,

```
  ∀ ops s k, incl(hashes_at_key(s, k),
                  hashes_at_key(apply_schedule(s, ops), k))
```

**Proof.** By induction on the schedule. The cons case applies
`t_9_2_atomic_monotone_any_key` (a generalization of the single-step
theorem to arbitrary keys) followed by the inductive hypothesis. ∎

Under the lock, an arbitrary serializable thread interleaving collapses
to a sequential schedule, so this theorem is the n-thread arbitrary-
interleaving statement.

### 9.3 T-9.3 — Dispatch completeness

**Statement.** *(`t_9_3_dispatch_complete`,
`BugFixDispatcher.v:41`.)* For every slashable invalid-block variant,
the post-fix dispatcher creates a record at `(offender, baseSeq)`.

**Proof.** Case analysis on `has_key s (er_key r)` where
`r = mkEqRec offender baseSeq nil`:
- Present: `insert_cond` is a no-op (lemma `insert_cond_dup_noop`); the
  key is still present.
- Absent: `insert_cond` prepends `r`; the key is now present (lemma
  `find_insert_cond_same_absent`). ∎

### 9.4 T-9.4 — Transfer-failure safety

**Statement.** *(`t_9_4_transfer_failure_safety`,
`BugFixTransferFailure.v:40`.)* The post-fix slash either succeeds with
T-7's bond-zero conclusion or returns `false` deterministically without
state change.

**Proof.** Case analysis on the `transfer_ok` oracle:
- `true`: standard `slash` applies; T-7 gives bond-zero.
- `false`: `(ps, false)` is returned; `ps' = ps` directly. ∎

### 9.5 T-9.5 — StakeZero invariant

**Statement.** *(`t_9_5_slash_preserves_invariant`,
`BugFixStakeZero.v:36`.)* The invariant "every active validator has
positive bond" is preserved by `slash`.

**Proof.** Case analysis on the slash branch:
- Idempotent (bond=0): state unchanged; invariant preserved.
- Positive: the new active set is `filter (fun v' => v' ≠ v)
  ps_active`, and the new bonds set zeros `v` only. For any `v'` in the
  new active set, `v' ≠ v` (by filter), so `bm_lookup(bm_slash(b, v),
  v') = bm_lookup(b, v')` (by `bm_slash_other`). The invariant on the
  old state gives `bm_lookup(b, v') > 0`. ∎

### 9.6 T-9.6 — Self-regression detection (Boolean predicate)

**Statement.** *(`t_9_6_self_regression_detected`,
`BugFixSelfRegression.v:52`.)*

```
  cited < latest ⟹ has_self_regression(blk_sn, latest, cited) = ⊤
```

**Proof.** Direct from the definition of `has_self_regression` and
`Nat.ltb_lt` reflection. ∎

The completeness companion `t_9_6_self_regression_complete` (line 60)
gives the converse.

### 9.6′ T-9.6 (DAG-level statement, Audit Gap 9 closure)

**Statement.** *(`t_9_6_self_regression_in_dag`,
`BugFixSelfRegression.v:79` (§1, Bug #6).)* Connecting the predicate to the actual
DAG via the `ds_latest_seq` oracle:

```
  In b blocks ∧ block_sender(b) = sender ∧ block_seq(b) > cited ⟹
    has_self_regression(0, ds_latest_seq(blocks, sender), cited) = ⊤
```

**Proof.** From `In b blocks ∧ block_sender(b) = sender`, the DAG
oracle's lower bound (`ds_latest_seq_lower_bound` from `DAGState.v`)
gives `block_seq(b) ≤ ds_latest_seq(blocks, sender)`. Combined with
`block_seq(b) > cited`, we get `cited < ds_latest_seq`. Apply the
Boolean theorem T-9.6. ∎

This closes the prior gap where T-9.6 was a Boolean tautology
disconnected from the DAG: the strengthened theorem witnesses regression
detection against an actual block in the chain.

### 9.7 T-9.7 — Sequence-number density

**Statement.** *(`t_9_7_canonical_finds_visible_descendant_with_gap`,
`t_9_7_canonical_dense_subsumes_pre_fix`,
`t_9_7_canonical_prefix_stability`, and
`t_9_7_canonical_memoized_equivalent` in
`BugFixSeqNumDensity.v`.)* The post-fix detector returns the canonical
visible self-chain child above `baseSeq`: the oldest visible same-sender
block whose sequence is still greater than `baseSeq`.

Let a self-chain be ordered from latest to oldest. For offender `v` and
base `β`,

```
canonical(chain, v, β) =
  None                         if chain = []
  None                         if head(chain) is not v or seq(head) ≤ β
  canonical(tail) if defined   otherwise
  head(chain)                  otherwise
```

The Rocq proof establishes:

- **Soundness.** If `canonical(chain, v, β) = Some c`, then
  `c ∈ chain ∧ sender(c)=v ∧ seq(c)>β`.
- **Gap completeness.** If a visible self-chain prefix is entirely above
  `β`, then some canonical child is returned, including non-dense chains
  such as `0 → 2`.
- **Dense subsumption.** A direct `base+1` child returns the same child as
  the pre-fix behavior.
- **Same-branch stability.** Adding any above-base latest-message prefix
  does not change the canonical child. Thus `0 → 10` and `0 → 10 → 11`
  contribute one branch child, not two.
- **Memoization transparency.** A cache entry is valid only if it equals
  the canonical recomputation; under that consistency predicate, the
  memoized query is observationally equivalent to the direct walk.

The TLA+ models check the same finite semantics with invariants
`Inv_CanonicalChildSound`, `Inv_CanonicalChildBoundary`,
`Inv_CanonicalGapCompleteness`, `Inv_CanonicalDenseSubsumesPreFix`,
`Inv_CanonicalPrefixStability`,
`Inv_CanonicalSameBranchNoOvercount`, and
`Inv_CanonicalMemoizedEquivalent`. ∎

### 9.8 T-9.8 — Unbonded proposer no-op

**Statement.** *(`t_9_8_unbonded_proposer_no_slash`,
`BugFixUnbondedProposer.v:44`.)* When the proposer's bond is 0, the
post-fix `prepare_slashing_deploys` returns `[]`.

**Proof.** Direct unfolding: `Nat.eqb 0 0 = true`, so the function
returns `[]`. ∎

### 9.9 T-9.9 — Self-correcting block admission

**Statement.** *(`t_9_9_post_fix_rejection_iff`,
`BugFixSelfRegression.v:107`.)* The post-fix rejection condition is:
`rejects = (has_neglected ∧ ¬has_slash)`.

**Proof.** Direct from the definition; `andb_true_iff` and
`negb_true_iff` give the bi-implication. ∎

The corollary `t_9_9_post_fix_admits_more` (`BugFixSelfRegression.v:121`) shows that the
post-fix predicate strictly admits more blocks (those with both
`has_neglected = ⊤` and `has_slash = ⊤`).

---

## 10 · TLA+ correctness model

### 10.1 Overview

Four TLA+ specifications complement the Rocq mechanization by exhaustive
finite-state model checking:

| Module                     | Purpose                                               |
|----------------------------|-------------------------------------------------------|
| `EquivocationDetector.tla` | Detector LTS; sound/complete invariants               |
| `ConcurrentTracker.tla`    | Lock-free vs. locked race; counter-example for bug #2 |
| `SlashFlow.tla`            | End-to-end pipeline                                   |
| `TwoLevelSlashing.tla`     | Closure termination + quorum                          |

Each `.tla` has an `MC_*.tla` instance with TLC parameters chosen to
keep the state space ≤ 10⁵.

### 10.2 Module structure

Each `.tla` file follows the standard TLA+ layout:

```
EXTENDS Integers, Sequences, FiniteSets, TLC

CONSTANTS (* abstract parameters *)

VARIABLES (* state variables *)

TypeOK == (* type-correctness invariant *)
Init == (* initial state predicate *)
Action_X == (* per-action transition *)
Next == disjunction of all actions
Spec == Init /\ □[Next]_vars /\ Fairness
Inv_* == (* state invariants *)
Live_* == (* temporal properties *)
```

### 10.3 Key invariants (all model-checked, 0 violations)

| Spec                   | Invariant                    | Theorem mirror  |
|------------------------|------------------------------|-----------------|
| `EquivocationDetector` | `Inv_DetectionSound`         | T-1             |
| `EquivocationDetector` | `Inv_TaxonomyCorrect`        | T-3             |
| `EquivocationDetector` | `Inv_FixedDetectorTotal` / `Inv_DuplicateChildNeedsDistinctChildren` | T-9.11 |
| `ConcurrentTracker`    | `Inv_RecordMonotone`         | T-9.2           |
| `SlashFlow`            | `Inv_BondsZeroAfterSlash`    | T-7             |
| `SlashFlow`            | `Inv_SlashedExcludedFromFC`  | T-10            |
| `SlashFlow`            | `Inv_SlashedRemoved`         | (corollary T-7) |
| `TwoLevelSlashing`     | `Inv_LevelClosureTerminates` | T-11            |
| `TwoLevelSlashing`     | `Inv_ActiveSetAboveQuorum`   | T-12            |
| `TwoLevelSlashing`     | `Inv_ActiveStakeAboveWeightedQuorum` | T-12W |
| `TwoLevelSlashing`     | `Inv_FilteredClosureInCurrentValidators` | T-12F |
| `TwoLevelSlashing`     | `Inv_NeglectEdgesVisibleUnreported` | T-12F |
| `TwoLevelSlashing`     | `Inv_NoUnexpectedDifferentialDivergence` | T-15 class |
| `TwoLevelSlashing`     | `Inv_UnsignedArithmeticBoundary` / `Inv_SignedArithmeticBoundary` | arithmetic boundary |
| `TwoLevelSlashing`     | `Inv_ActiveQuorumsIntersect` | quorum intersection |
| `TwoLevelSlashing`     | `Inv_ActiveStakeQuorumsIntersect` | weighted quorum intersection |
| `TwoLevelSlashing`     | `Inv_ClosureStableAtMaxLevel` | fixed-point stability |
| `TwoLevelSlashing`     | `Inv_EpochEligibleInCurrent` / `Inv_StaleEvidenceNotEligible` | epoch filtering |
| `TwoLevelSlashing`     | `Inv_ReportsSuppressNeglectEdges` | visibility/report suppression |
| `TwoLevelSlashing`     | `Inv_ArithmeticSafeEnvelope` | arithmetic safe envelope |
| `TwoLevelSlashing`     | `Inv_ViewEdgesVisibleUnreported` | view-indexed active evidence |
| `TwoLevelSlashing`     | `Inv_SameViewSameClosure` | equal active views imply equal closure |
| `TwoLevelSlashing`     | `Inv_ValidatorRenamingEquivariance` | bijective validator renaming preserves closure |
| `TwoLevelSlashing`     | `Inv_CarryoverPolicyCurrent` / `Inv_NoCarryoverNoMappedDirect` | epoch carryover policy |
| `TwoLevelSlashing`     | `Inv_EvidenceRetentionForDirectOffenders` | evidence-retention precondition |
| `TwoLevelSlashing`     | `Inv_CanonicalRecordKeyInjective` | canonical record key injectivity |
| `TwoLevelSlashing`     | `Inv_BatchNoFailureOrderIndependent` / `Inv_PartialBatchFailureRequiresAtomicPolicy` | batch slash projection boundary |

### 10.4 Memory-efficient rewrite: `EquivocationDetectorEager`

The original `EquivocationDetector` spec completed 14.9M distinct
*safety* states then OOMed during *liveness-graph construction* (the
liveness graph itself reached ~120M distinct nodes before exhausting
the 32 GB heap; see §10.5 for the breakdown). We provide an
equivalence-preserving rewrite
`EquivocationDetectorEager.tla` that combines three orthogonal
optimizations:

**1. Truly eager detection.** `SignAndDetect` atomically (a) signs the new
block and (b) reclassifies *every existing sibling* at the same `(v, s)`.
There is no reachable state where two siblings co-exist with inconsistent
classifications. This is observationally equivalent to the original
spec's eventual-classification semantics because no observable barb
distinguishes "classification has happened" from "classification will
happen on the next step".

**2. Liveness as safety.** Under truly-eager detection, the temporal
property `[](real-equivocation ~> non-valid)` reduces to the safety
invariant `Inv_LivenessAsSafety`:

```
∀ v, s, b. (b ∈ blocks[v][s] ∧ IsRealEquivocation(v, s)) ⟹
   detectedStatus[v, s, b] ∈ {admissible, ignorable, neglected}
```

This is logically equivalent because (a) every sibling is classified in
the same atomic step that creates equivocation, and (b) safety on every
reachable state is exactly "always" semantics.

**3. Symmetry.** `SYMMETRY Permutations(Validators)` quotients the state
space by validator permutations.

| Configuration                               | Distinct states                                 | Time         | Liveness verified |
|---------------------------------------------|-------------------------------------------------|--------------|-------------------|
| Original safety + temporal at 2v×2s×2b      | Safety: 14.9M; liveness graph: ~120M before OOM | 65 min → OOM | ✗                 |
| Original safety only at 2v×2s×2b            | 22,667,121                                      | 2 min 26 s   | n/a               |
| **Eager + symmetry + Inv_LivenessAsSafety** | **2,080**                                       | **<1 s**     | ✅                |

The state reduction is **22,667,121 / 2,080 ≈ 10,898×** (older
drafts quoted 10,896×; the recomputed ratio rounds to 10,898 or,
conservatively, ≈10,900×). The reduction is the product of:
- Symmetry reduction: ~2×
- Removed sibling-reclassification interleavings: ~80×
- Removed dependency-flag interleavings (atomic): ~70×

The rewrite is observationally bisimilar to the original (every reachable
state of the original maps to one in the rewrite via the natural
projection that classifies pending blocks).

### 10.5 Model-checking results (verified through 2026-05-05 run)

Run command: `systemd-run --user --scope -p MemoryMax=32G tlc -workers 8 ...`.

| Spec                                                                                                                    | Result                                                                                                     | States explored                                                      |
|-------------------------------------------------------------------------------------------------------------------------|------------------------------------------------------------------------------------------------------------|----------------------------------------------------------------------|
| `MC_TwoLevelSlashing` (`EnforceClosureBound=TRUE`, weighted closure bound, quorum-intersection, fixed-point, epoch, visibility/report, evidence-view, carryover, retention, canonical-key, batch-projection, proposer-fairness, semantic-campaign, scheduler, assumption-classification, arithmetic-stress, and arithmetic-envelope invariants) | ✅ Exhausted, 0 violations on 2026-05-05 with `tlc -workers 1`                                             | 73,728 generated; **30,720 distinct**                                |
| `MC_ConcurrentTracker` (Locked=TRUE)                                                                                    | ✅ Exhausted, 0 violations                                                                                 | **37 distinct**                                                      |
| `MC_ConcurrentTracker` (Locked=FALSE)                                                                                   | ✅ **Correctly violates `Inv_RecordMonotone`** (counter-example for bug #2)                                | 90 generated, 71 distinct, terminating at depth 6                    |
| `MC_SlashFlow` (full invariants incl. `Inv_ForfeitedToCoopVault` and `Inv_StakeConservation` via `RECURSIVE` operators) | ✅ Exhausted, 0 violations                                                                                 | 2,365,633 generated; **405,224 distinct**; depth 22                  |
| `MC_EquivocationDetector` (combined safety + `Live_DetectionComplete`, 2v × 2s × 2b)                                    | ⚠️ JVM heap exhausted at 14.9M distinct states during liveness graph construction (after 65 min, 32 GB cap) | Liveness graph hit ~120M distinct states before OOM                  |
| `MC_EquivocationDetector_safety` (full bounds, safety only)                                                             | ✅ Exhausted, 0 violations                                                                                 | **191,849,257 generated; 22,667,121 distinct**; depth 29; 2 min 26 s |
| `MC_EquivocationDetector_liveness` (1v × 1s × 2b, safety + temporal)                                                    | ✅ Exhausted, 0 violations                                                                                 | 147 generated; **69 distinct**; depth 8                              |

The split between `MC_EquivocationDetector_safety` and
`MC_EquivocationDetector_liveness` reflects the standard formal-
verification practice: at full bounds the liveness-graph construction is
exponential and exceeds heap capacity, so safety is verified
exhaustively at full bounds (22.7M distinct states) and liveness is
verified exhaustively at reduced bounds (69 distinct states). The
universal liveness statement (T-2 `detection_complete`) is proven for
all DAG sizes in the Rocq mechanization.

**Bug #2 is formally demonstrated** by the `Locked=FALSE` violation
trace; the post-fix `Locked=TRUE` configuration removes the violation,
confirming the fix.

### 10.6 Rocq ↔ TLA+ correspondence

| TLA+ invariant                  | Rocq theorem                         | Same property? |
|---------------------------------|--------------------------------------|----------------|
| `Inv_DetectionSound`            | `detection_sound`                    | yes            |
| `Inv_TaxonomyCorrect`           | `slashable_post_fix_extends_pre_fix` | yes            |
| `Inv_FixedDetectorTotal` / `Inv_MissingPointerNonContributing` | `fixed_detectable_missing_pointer_prefix` | yes |
| `Inv_DuplicateChildNeedsDistinctChildren` | `fixed_detectable_duplicate_single_child_false` | yes |
| `Inv_TwoDistinctChildrenDetect` | `fixed_detectable_two_distinct_children_true` | yes |
| `Inv_DetectedHashDetects` | `fixed_detectable_detected_hash_true` | yes |
| `Inv_RecordMonotone` (Locked=⊤) | `t_9_2_atomic_no_overwrite`          | yes            |
| `Inv_BondsZeroAfterSlash`       | `slash_zeros_bond`                   | yes            |
| `Inv_SlashedExcludedFromFC`     | `fork_choice_exclusion`              | yes            |
| `Inv_LevelClosureTerminates`    | `t_11_level_2_termination`           | yes            |
| `Inv_ActiveStakeAboveWeightedQuorum` | `weighted_slash_iter_quorum_preservation` | yes      |
| `Inv_FilteredClosureInCurrentValidators` | `restricted_closure_only_from_current_direct_offenders` | yes |
| `Inv_NeglectEdgesVisibleUnreported` | `visible_unreported_graph_in`    | yes            |
| `Inv_NoUnexpectedDifferentialDivergence` | `divergence_allowed` classification in `Bisimulation.v` | yes |
| `Inv_UnsignedArithmeticBoundary` / `Inv_SignedArithmeticBoundary` | `unsigned_overflow_boundary_exact` / `signed_overflow_boundary_exact` | yes |
| `Inv_ActiveQuorumsIntersect` | `quorum_intersection_by_size` | yes |
| `Inv_ActiveStakeQuorumsIntersect` | `weighted_quorum_intersection_from_disjoint_bound` | yes |
| `Inv_ClosureStableAtMaxLevel` | `slash_iter_fixed_point_after_universe_bound` | yes |
| `Inv_EpochEligibleInCurrent` / `Inv_StaleEvidenceNotEligible` | `epoch_filter_in` | yes |
| `Inv_ReportsSuppressNeglectEdges` | `visible_unreported_graph_in` | yes |
| `Inv_ArithmeticSafeEnvelope` | `arithmetic_safe_envelope` | yes |
| `Inv_ViewEdgesVisibleUnreported` | `visible_unreported_graph_in` / `reported_edge_not_active` | yes |
| `Inv_SameViewSameClosure` | `view_closure_equiv_by_active_edges` | yes |
| `Inv_ValidatorRenamingEquivariance` | `slash_iter_validator_renaming_equiv` | yes |
| `Inv_CarryoverPolicyCurrent` / `Inv_NoCarryoverNoMappedDirect` | `carryover_policy_sound` | yes |
| `Inv_EvidenceRetentionForDirectOffenders` | `restricted_closure_only_from_current_direct_offenders` precondition | yes |
| `Inv_CanonicalRecordKeyInjective` | `canonical_key_pair_injective` | yes |
| `Inv_BatchNoFailureOrderIndependent` / `Inv_PartialBatchFailureRequiresAtomicPolicy` | `bm_slash_many_order_independent` / `bm_slash_many_abort_order_dependent` | yes |
| `Inv_ProposerFairnessForBoundedLiveness` | `proposer_fairness_boundary_requires_review` | yes |

The table lists the safety invariants with the closest 1:1 Rocq
counterparts. Additional TLA+ invariants —
`Inv_NoOverwrite` (`ConcurrentTracker.tla`),
`Inv_LivenessAsSafety` (`EquivocationDetectorEager.tla`, the
rewrite-introduced shadow of `Live_DetectionComplete`),
`Inv_RecordHasWitness` (`EquivocationDetector.tla:207` /
`EquivocationDetectorEager.tla:195`, asserts every equivocation
record contains its witness hash),
`Inv_ActiveSetAboveQuorum` (`TwoLevelSlashing.tla`, checked under
`EnforceClosureBound=TRUE` and mirrored by T-12
`t_12_bft_quorum_preservation`),
`Inv_ForfeitedToCoopVault` (`SlashFlow.tla`, corollary of T-8
`slash_transfers_stake`),
`Inv_StakeConservation` (`SlashFlow.tla`, corollary of T-7 + T-8),
and `Inv_SlashedRemoved` (`SlashFlow.tla`, projection of T-7
`slash_zeros_bond` onto the active-set difference) — are
corollaries / weakenings of the listed Rocq theorems and are discharged
by the same proofs. The new two-level invariants are direct TLA+ mirrors
of the Sage-promoted Rocq theorems in §7.4-§7.9.

### 10.7 What TLA+ proves and does not

**Proves** (within finite parameter ranges): the listed invariants on
all reachable states and fair executions.

**Does not prove**: universal statements over arbitrary `n`, DAG depth,
or equivocation count. Those are the province of the Rocq proofs. TLA+
is here to catch specification bugs the Rocq theorems might mask via
inadvertently strong hypotheses.

### 10.8 Findings from model checking

This section consolidates non-obvious results that surfaced during the
TLC runs. They are intentionally separated from the "everything passes"
table because they are operationally important and should be flagged
to validator operators and auditors.

#### 10.8.1 Bug #2 demonstrated: lock-free tracker race

**Run.** `MC_ConcurrentTracker_pre_fix.cfg` (`Locked = FALSE`).

**Result.** TLC produces a 6-step trace where `Inv_RecordMonotone` (and
`Inv_NoOverwrite`) are violated:

```
T1 reads view = ∅
T2 reads view = ∅
T1 writes {h1}        store := {h1}
T2 writes {h2}        store := {h2}     ← h1 lost
```

**Implication.** This is the formal evidence for bug #2 (Rust
regression at `multi_parent_casper_impl.rs:1046-1075`). The post-fix
configuration (`Locked = TRUE`) eliminates the violation, confirming
the fix proven in Rocq as `t_9_2_atomic_no_overwrite`.

[![Diagram 09 — Tracker race and locking fix: the upper half is the pre-fix overwrite trace; the lower half is the post-fix serialized RMW under the lock](./diagrams/09-seq-tracker-race-and-fix.svg)](./diagrams/09-seq-tracker-race-and-fix.svg)

#### 10.8.2 Two-level slashing can liquidate quorum if the network is more than F-neglectful

**Run.** Adding `INVARIANT Inv_ActiveSetAboveQuorum` to
`MC_TwoLevelSlashing.cfg` (4 validators, F=1, QuorumLowerBound=3).

**Result.** TLC produces a 2-step trace where the active set drops
below quorum:

```
equivocators = {v1}
neglectGraph = (v1 :> {} @@ v2 :> {} @@ v3 :> {} @@ v4 :> {v1, v2})
Step 0: slashed = {v1}        (active = 3 ≥ 3 ✓)
Step 1: slashed = {v1, v4}    (active = 2 < 3 ✗)   ← v4 caught up
```

**Why this is the protocol working correctly.** The slash-closure
honestly grew beyond F = 1: v1 was slashed for equivocating, v4 was
slashed for *neglecting* v1's equivocation (citing v1's invalid block
without attaching a slash deploy). Both slashes are rule-justified.
The active set legitimately fell below quorum because, in this trace,
the *adversary's chosen neglect graph* makes the slash-closure exceed
the BFT bound F.

**Implication.** Two-level slashing relies on the social/economic
assumption that honest validators do not collectively neglect known
equivocators. A configuration where more than F validators neglect a
single equivocator will liquidate the active set in O(1) slash rounds.
This is the *intended* behavior (collusion is mutually destructive)
but it is also a quorum risk that operators should be aware of.

**Why we did not enforce it as an invariant.** Forcing the
configuration to satisfy `|slash-closure| ≤ F` would require encoding
the BFT assumption into the spec's `Init`, making the invariant
tautological. Instead, the Rocq theorem `t_12_bft_quorum_preservation`
takes the precondition as an explicit hypothesis, and the TLA+ cfg
documents this with an inline comment pointing at the Rocq result.

**Operational guidance.** Validator operators should monitor the
neglect-graph density of the network. If more than F validators are
ever observed citing a known equivocator without attaching a slash
deploy, the protocol can preserve safety only by sacrificing liveness
(the active set falls below quorum and consensus stalls). Mitigations:
(a) require slash deploys to be issued by every validator that observes
an equivocation, (b) cap the neglect-closure size in the proposer's
deploy-construction logic, or (c) a combination of social and
economic disincentives that keep `|neglect-closure|` below F.

#### 10.8.3 Sage-guided edge cases promoted to Rocq/TLA+

The Sage models in `formal/sage/slashing/` found additional finite
witnesses and exploratory classifications. They have been promoted as
Rocq theorem targets, TLA+ invariants, documented assumptions, or test
use cases rather than accepted as proof authority.

| Finding | Finite witness | Formal response |
|---------|----------------|-----------------|
| Weighted stake amplification | equal-stake chain `0 -> 1 -> 2 -> 3` slashes all four validators from one direct offender | `weighted_slash_iter_quorum_preservation` and `Inv_ActiveStakeAboveWeightedQuorum` |
| Zero-stake direct offender | `stakes=[0,2,2]`, offender `0`, edge `1 -> 0` | `zero_stake_not_direct_offender_under_bonded_precondition`; tests must reject/filter zero-stake direct offenders |
| Stale/off-era evidence | evidence validator outside current set seeds current slash under unfiltered projection | `restricted_closure_only_from_current_direct_offenders`; `Inv_FilteredClosureInCurrentValidators` |
| Evidence withholding | partial visibility closure smaller than full-visibility accountability closure | `visible_unreported_graph_in`; `Inv_NeglectEdgesVisibleUnreported` |
| Duplicate/self/cyclic graph cases | duplicates idempotent; disconnected cycles not slashed; cycles reaching offenders slashed | `slash_iter_graph_equiv`; `no_reachability_no_level2_slash` |
| Bounded arithmetic projection | signed 64-bit and unsigned 128-bit `max + 1` diverge from exact arithmetic | `unsigned_overflow_boundary_exact`; `signed_overflow_boundary_exact` |
| Differential bisimilarity | no unexpected divergence in bounded search; tracker race is permitted bug fix; boundary divergence is candidate | `DivergenceClass` and `divergence_allowed` in `Bisimulation.v` |
| Hypothesis-reduced proposer fairness | one bonded proposer observes evidence and withholds it; appending one fair including proposer gives the first slash slot | `proposer_fairness_boundary_requires_review`; `Inv_ProposerFairnessForBoundedLiveness` |
| Hypothesis-reduced projection risks | delimiter-free key `(1,10)/(11,0)`, two-validator partial abort, one-slot pruning, all-unit weighted closure-bound violation | `delimiter_free_record_key_projection_hypothesis_collision`; `current_rust_record_update_retains_all_detected_hashes`; existing batch, retention, and weighted-bound theorems |
| Hypothesis frontier exploration | novelty/coverage scoring, feature-combination coverage, bundle-based state machines, rule-based multi-epoch, partition/gossip, and semantic campaign state machines, production-shaped DAG trace generation, detector-totality DAG search, cross-oracle closure consistency, adaptive evidence-denial search, composite multi-axis attack search, candidate invariant mining, temporal-window synthesis, mutation-oracle detection, rebond identity lifecycle search, record-lifecycle state-machine search, closure-depth extremal search, adversarial scheduler search, liveness-as-safety checks, less-directed traces, exact-vs-projection checks, arithmetic projection stress, attack-objective and objective-guided search, metamorphic and Rust-metamorphic checks, assumption minimization/weakening, precondition fuzzing, Rust corpus/replay emission, generated-trace classification, evidence-addition monotonicity, view-merge confluence, minimal slash-basis extraction, record-key namespace projection, detector traversal termination, detector contribution confluence, closure fixed-point idempotence, report-retention reactivation, no-seed cycle safety, slash-history prefix exactness, edge-orientation sanity, redundant-path denial cost, slash-target authorization, report namespace isolation, report-antitone closure, direct-seed report dominance, validator-renaming equivariance, and bisimilarity delta guarding found no unexpected divergence in configured quick and deep runs | Sage witness generator; `semantic_campaign_boundary_reasons_require_review`; `adversarial_scheduler_boundary_reasons_require_review`; `frontier_expansion_reasons_require_review`; `slash_iter_initial_graph_monotone`; `slash_iter_empty_initial_empty`; `view_closure_reports_antimonotone`; `unreported_visible_edge_remains_active`; `graph_union_closure_overapproximates_left`; `graph_union_closure_commutative`; `slash_iter_validator_renaming_equiv`; `branch_traversal_fixed_after_domain_bound`; `Inv_InitialEvidenceMonotonicity`; `Inv_ViewMergeOverapproximatesInputs`; `Inv_ViewMergeCommutative`; `Inv_ValidatorRenamingEquivariance`; `Inv_DetectorTraversalFiniteFuel`; `Inv_DetectorTraversalInDomain`; `Inv_SlashedEqualsClosurePrefix`; `Inv_NoDirectSeedNoClosure`; `Inv_UnreportedVisibleEdgesRemainActive`; `Inv_ReportGrowthCannotExpandViewClosure`; `Inv_ReportsDoNotSuppressDirectEvidence`; existing divergence, projection, detector-totality, reachability, temporal-window, rebond-identity, record-lifecycle, closure-depth, canonical-key, and merge classifications |
| Horizon Sage/Hypothesis campaign exploration | cross-coupled retention/gossip/inclusion windows, proposer withholding, Rust detector contribution gates, epoch/rebond identity, weighted damage outside closure-bound assumptions, partition view merge, checked arithmetic, report scoping, and edge-order/matrix-oracle metamorphism found no unexpected class in focused Sage and Hypothesis horizon checks | `DRHorizonCampaignBoundary`; `HorizonCampaignDivergenceClass`; existing retention, proposer-fairness, detector-totality, reachability, report, arithmetic, and divergence classes; Finding 116 is a theorem-strengthening and regression-suite input, not a new permitted bisimilarity delta |
| Horizon-v2 Rust-aligned Sage/Hypothesis exploration | detector DAG contribution semantics, detected-hash record lifecycle, finality-aware retention, weighted damage plus evidence-denial cost, epoch/era identity, and exact-vs-projection classification found no unexpected class in focused Sage and Hypothesis horizon-v2 checks | `DRHorizonV2Boundary`; `HorizonV2DivergenceClass`; existing detector, record-lifecycle, temporal-retention, weighted-bound, reachability, epoch-identity, and divergence classes; Finding 117 is a Rust-shaped theorem-strengthening and regression-suite input, not a new permitted bisimilarity delta |
| DAG/objective/deep Sage threat modeling | production-shaped DAG direct/report/retention/epoch/multi-level traces, objective Pareto frontier ranking, graph reverse-reachability attack paths, MIP-backed stake-damage optimization, minimum attacker stake, maximum quorum loss, withholding/pruning thresholds, evidence-denial min-cuts, detector-totality threat witnesses, exhaustive cross-oracle closure consistency, candidate invariant mining, temporal-window synthesis, mutation-oracle detection, rebond identity lifecycle boundaries, record-lifecycle projection risks, closure-depth extremal witnesses, evidence-addition monotonicity, view-merge confluence, minimal slash-basis cataloging, record-key namespace projections, detector traversal termination, detector contribution confluence, closure fixed-point idempotence, report-retention reactivation, no-seed cycle safety, slash-history prefix exactness, edge-orientation sanity, redundant-path denial cost, slash-target authorization, report namespace isolation, report-antitone closure, direct-seed report dominance, validator-renaming equivariance, bisimilarity delta guarding, epoch/churn identity boundaries, economic safety envelopes, safe-envelope distance, minimal counterexample catalog, and threat-vector ranking found no unexpected class | `deep_threat_chain_closure_bound_assumption_needed`; `slash_iter_validator_renaming_equiv`; `DeepThreatModelDivergenceClass`; `DagTraceDivergenceClass`; existing reachability, weighted-bound, retention, epoch, arithmetic, detector-totality, detector traversal, record-lifecycle, monotonicity, report-antitone, merge-confluence, canonical-key, no-seed, slash-prefix, and divergence classifications |
| Defensive adversarial campaign search | production DAG projection gaps, multi-node local-view splits, adaptive stake/quorum objectives, exact-vs-runtime projection matrix rows, differential-oracle pipeline rows, mutation/metamorphic variants, and minimized threat-corpus ranking are generated and classified for bug hunting. The production DAG row now uses a Rust-exact latest-message detectability projection; direct-only and broad citation graphs are differential projections only. | `rust_detectable_view_graph_in`; `same_rust_detectable_view_same_closure`; `RustViewDetectabilityClass`; `AdversarialCampaignDivergenceClass`; `DifferentialOraclePipelineClass`; Rust replay fixture classifications |
| Search-horizon v3 feedback loop | coverage-gap objectives, public detector classification fuzzing, candidate-to-SlashDeploy lifecycle fuzzing, per-precondition Kani authorization checks, and optional coverage/mutation/supply-chain/Apalache gates expand bug hunting without changing the proof authority. New witnesses are promoted to Rocq/TLA+ only after Rust traceability confirms that they are normative behavior, a real production bug, or a theorem-precondition counterexample. | Existing divergence-class theorems and TLA+ classification invariants; new Rocq/TLA+ obligations are created only by confirmed traceability outcomes |

#### 10.8.4 Combined safety+liveness OOM at 2v×2s×2b — and how the rewrite handles it

**Run.** Original `MC_EquivocationDetector.cfg` with both
`INVARIANT Inv_DetectionSound` and `PROPERTY Live_DetectionComplete`.

**Result.** JVM heap exhausted at 14.9M distinct safety states during
liveness-graph construction (32 GB cap). The temporal property
`Live_DetectionComplete` instantiates 8 separate automaton instances
(one per `(v, s, b)` triple); each multiplies the liveness graph.

**Resolution.** The eager rewrite `EquivocationDetectorEager.tla`
combines `SignBlock + DetectArrival + ReclassifySibling` into one
atomic `SignAndDetect` action. Under truly-eager classification, the
temporal property `Live_DetectionComplete` reduces to the safety
invariant `Inv_LivenessAsSafety`, eliminating the liveness-graph
construction. Combined with `SYMMETRY Permutations(Validators)`, this
yields a ≈10,898× state-space reduction (22,667,121 → 2,080) and runs
in <1 s.

**Implication.** Liveness checking does not scale to even modest
bounds for spec patterns with universally-quantified eventually-detect
properties. The rewrite pattern (combine action + invariant-ize the
liveness) is general and can apply to any classification-style
protocol where classification can fire atomically with the action that
creates the classifiable event.

#### 10.8.5 `Inv_NoOverwrite` is weaker than `Inv_RecordMonotone`

**Observation.** `ConcurrentTracker.tla` defines two separate
invariants:

- `Inv_NoOverwrite` (line 151): "if a hash is in the store, it stays".
- `Inv_RecordMonotone` (line 169): "every thread that has reached
  `done` has its hash in the store".

The latter is strictly stronger: it asserts not just preservation but
*persistence of every successful insert*. Both invariants are now
checked by the post-fix and pre-fix cfgs, and both fire on the
overwrite trace under `Locked = FALSE`.

**Implication.** For documentation purposes, the README and the
verification doc reference `Inv_RecordMonotone` as the load-bearing
property; `Inv_NoOverwrite` is a sanity check that catches the same
race at a less-precise abstraction. Together they form a triangulation:
if the implementation passed one and failed the other, that would
indicate a deeper modeling issue.

#### 10.8.6 Rocq vs TLA+ scope of `t_9_6` self-regression

**Discrepancy.** The Boolean-predicate version `t_9_6_self_regression_detected`
is essentially a `Nat.ltb_lt` reflection wrapper (proved in 1 line).
The DAG-level companion `t_9_6_self_regression_in_dag` (added in the
gap-closure pass) connects the predicate to an actual block in the
DAG via `ds_latest_seq_lower_bound`.

**Implication.** The single-line Boolean version is *necessary* but
not *sufficient* for protocol-level soundness. The DAG-level version
witnesses the regression against a concrete block. Any future code
change to `validate.rs` that touches the regression check should be
accompanied by the DAG-level theorem, not just the Boolean.

---

## 11 · Module reference

The component-to-artifact correspondence is shown visually in Diagram 10
(each component box carries the spec section, Rocq module, TLA+ module,
and Rust file that realize it):

[![Diagram 10 — Specification ↔ Rocq ↔ TLA+ ↔ Rust correspondence: every slashing-subsystem component, annotated with its formal artifacts and implementation source](./diagrams/10-component-formal-correspondence.svg)](./diagrams/10-component-formal-correspondence.svg)

### 11.1 Files

```
formal/rocq/slashing/theories/
├── Validator.v                    (foundations: BondMap algebra)
├── Block.v                        (Block, Justification, equivocation predicate)
├── InvalidBlock.v                 (26-variant taxonomy + is_slashable, T-3)
├── EquivocationRecord.v           (EqStore, T-4, T-5)
├── DAGState.v                     (DAG snapshot + equivocates predicate)
├── EquivocationDetector.v         (detect, T-1, T-2, T-6, T-9.11)
├── PoSContract.v                  (slash transition, T-7, T-8, T-Idem)
├── SlashDeploy.v                  (system-deploy execution)
├── BlockCreator.v                 (prepare_slashing_deploys)
├── ForkChoice.v                   (filter_slashed, T-10)
├── TwoLevelSlashing.v             (slash_iter, T-11, T-12)
├── BugFixIgnorable.v              (T-9.1)
├── BugFixAtomicTracker.v          (T-9.2)
├── BugFixDispatcher.v             (T-9.3)
├── BugFixTransferFailure.v        (T-9.4)
├── BugFixStakeZero.v              (T-9.5)
├── BugFixSelfRegression.v         (T-9.6, T-9.9)
├── BugFixSeqNumDensity.v          (T-9.7)
├── BugFixUnbondedProposer.v       (T-9.8)
├── BugFixWithdrawTransferFailure.v (T-9.10, T-9.10', T-9.10″)
├── Bisimulation.v                 (T-13, T-15 components)
└── MainTheorem.v                  (composition; main_bisimilarity_theorem)
```

### 11.2 Paper-to-code traceability

| Specification doc reference     | Rocq location                                             |
|---------------------------------|-----------------------------------------------------------|
| §3.1.1 Validate                 | `InvalidBlock.v` (taxonomy)                               |
| §3.1.2 EquivocationDetector     | `EquivocationDetector.v`                                  |
| §3.2.2 EquivocationTrackerStore | `EquivocationRecord.v`                                    |
| §3.3.1 BlockCreator             | `BlockCreator.v`                                          |
| §3.3.2 SlashDeploy              | `SlashDeploy.v`                                           |
| §3.4.1 PoS Rholang contract     | `PoSContract.v`                                           |
| §3.5.1 ForkChoice               | `ForkChoice.v`                                            |
| §4 Detection semantics          | `EquivocationDetector.v`                                  |
| §5 PoS slash transition         | `PoSContract.v`                                           |
| §6 Validator lifecycle          | composition of `PoSContract.v` and `EquivocationRecord.v` |
| §7 Pipeline                     | `MainTheorem.v` (main_bisimilarity_theorem)               |
| §8 Two-level slashing           | `TwoLevelSlashing.v`                                      |
| §9 Bisimilarity                 | `Bisimulation.v`                                          |
| §10.1 Bug fix #1                | `BugFixIgnorable.v`                                       |
| §10.2 Bug fix #2                | `BugFixAtomicTracker.v` + TLA+ counter-example            |
| §10.3 Bug fix #3                | `BugFixDispatcher.v`                                      |
| §10.4 Bug fix #4                | `BugFixTransferFailure.v`                                 |
| §10.5 Bug fix #5                | `BugFixStakeZero.v`                                       |
| §10.6 Bug fix #6                | `BugFixSelfRegression.v` (T-9.6)                          |
| §10.7 Bug fix #7                | `BugFixSeqNumDensity.v`                                   |
| §10.8 Bug fix #8                | `BugFixUnbondedProposer.v`                                |
| §10.9 Bug fix #9                | `BugFixSelfRegression.v` (T-9.9)                          |
| §10.10 Bug fix #10              | `BugFixWithdrawTransferFailure.v` (T-9.10, T-9.10', T-9.10″) |
| §10.11 Bug fix #11              | `EquivocationDetector.v` (T-9.11 fixed view detectability) |

---

## 12 · Trust base

### 12.1 Section hypotheses

The development uses no `Section` hypotheses or `Variables` outside
of standard library imports.

### 12.2 `Print Assumptions` evidence

Running

```
echo 'From Slashing Require Import MainTheorem.
From Slashing Require Import TwoLevelSlashing.
From Slashing Require Import Bisimulation.
Print Assumptions main_bisimilarity_theorem.
Print Assumptions main_bisimilarity_strong.
Print Assumptions main_T14_weak_barbed_equiv_refl.
Print Assumptions main_T14_weak_barbed_equiv_trans.
Print Assumptions main_T12_bft_quorum.
Print Assumptions main_T9_2_n_threads.
Print Assumptions main_T15_pipeline_step.
Print Assumptions main_slashing_algorithm_correct.
Print Assumptions main_T6_detect_neglected_sound.
Print Assumptions main_T9_6_dag.
Print Assumptions slash_iter_reachability_characterization.
Print Assumptions weighted_slash_iter_quorum_preservation.
Print Assumptions restricted_closure_only_from_current_direct_offenders.
Print Assumptions visible_unreported_graph_in.
Print Assumptions slash_iter_graph_equiv.
Print Assumptions slash_iter_validator_renaming_equiv.
Print Assumptions no_reachability_no_level2_slash.
Print Assumptions unsigned_overflow_boundary_exact.
Print Assumptions signed_overflow_boundary_exact.
Print Assumptions candidate_boundary_divergence_requires_review.
Print Assumptions unexpected_divergence_forbidden.' \
  | coqtop -Q theories Slashing
```

produces, for **every** listed theorem:

```
Closed under the global context
```

This is the strongest possible assertion: each theorem depends only on
Rocq's standard library and the slashing theories — no `Admitted`, no
custom `Axiom`, no `Parameter`, no extracted assumption. Reproducible
with the exact command above.

The complete theorem set (after all eleven audit-gap closures) covers:

- **Detection layer** (T-1, T-2, T-3, T-4 via `detect_neglected_*`)
- **Record persistence** (T-4, T-5)
- **Slash effect** (T-7, T-8, T-Idem — including `ps_active`, T-10)
- **Two-level slashing** (T-11, T-12 list-length, T-12 BFT-style,
  reachability characterization, weighted quorum, current-validator
  filtering, evidence visibility, graph edge cases, arithmetic boundaries)
- **Bisimilarity** (T-13 strong baseline, T-13 records monotonicity,
  T-13 forkchoice filter, T-14 weak barbed equivalence reflexivity,
  symmetry, and transitivity, T-15 pipeline composition)
- **Bug fixes** (T-9.1 through T-9.11 — including the strengthened
  T-9.2 n-thread schedule, T-9.6 DAG-level, and the
  `BugFixWithdrawTransferFailure.v` triple T-9.10 / T-9.10' / T-9.10″
  for the post-quarantine withdrawal flow, plus T-9.11 detector
  totality/distinct-child lemmas)

All return "Closed under the global context".

### 12.3 Scope boundaries (what we do not formalize)

| Item                                          | Why                                                                                                                                                             |
|-----------------------------------------------|-----------------------------------------------------------------------------------------------------------------------------------------------------------------|
| Byte-level on-disk equality                   | Bisimilarity is value-level; iteration order is a non-observable.                                                                                               |
| Rholang interpreter semantics                 | The `slash` Rholang contract is shared between Rust and Scala; we treat the Rholang execution as an abstract function `slash : PoSState → V → PoSState × bool`. |
| Network-level message-passing                 | Out of scope; the LTS is on local state.                                                                                                                        |
| Cryptographic signatures                      | Validators are abstract `nat`s; the PoS auth-token check is modeled as a Boolean oracle around slash-deploy execution.                                          |
| Replay determinism over partial slash deploys | Adjacent (bug fix #8); the proof is structural, not replay-protocol-level.                                                                                      |
| Validator-set genesis                         | Out of scope; we assume an initial `BondMap` and prove preservation under transitions.                                                                          |

### 12.4 Cited classical lemmas (none in critical path)

The development does not use any classical axiom (excluded middle,
choice, etc.) or any cited but unproven lemma. The four candidates
mentioned in the plan (Sangiorgi's bisim up-to, Newman's lemma, König's
lemma, BFT bound) appear as commentary only; the proofs that would
otherwise need them are recast as theorems with explicit hypotheses
(e.g., T-12 takes `NoDup universe` and `NoDup s₀` as antecedents
rather than relying on the BFT bound abstractly).

---

## 13 · References

[BG19]
    V. Buterin and V. Griffith.
    *Casper the Friendly Finality Gadget*.
    arXiv:1710.09437, 2019.
    [doi:10.48550/arXiv.1710.09437](https://doi.org/10.48550/arXiv.1710.09437)

[BHKPQRSWZ20]
    V. Buterin et al.
    *Combining GHOST and Casper*.
    arXiv:2003.03052, 2020.
    [doi:10.48550/arXiv.2003.03052](https://doi.org/10.48550/arXiv.2003.03052)

[CBCCoq20]
    *Formalizing Correct-by-Construction Casper in Coq*.
    IEEE Xplore document 9169468, 2020.

[BKM18]
    E. Buchman, J. Kwon, Z. Milosevic.
    *The latest gossip on BFT consensus*.
    arXiv:1807.04938, 2018.
    [doi:10.48550/arXiv.1807.04938](https://doi.org/10.48550/arXiv.1807.04938)

[ABPT19]
    Y. Amoussou-Guenou et al.
    *Correctness of Tendermint-Core Blockchains*.
    OPODIS 2018.
    [doi:10.4230/LIPIcs.OPODIS.2018.16](https://doi.org/10.4230/LIPIcs.OPODIS.2018.16)

[LSP82]
    L. Lamport, R. Shostak, M. Pease.
    *The Byzantine Generals Problem*.
    ACM TOPLAS, 4(3):382–401, 1982.
    [doi:10.1145/357172.357176](https://doi.org/10.1145/357172.357176)

[MR05a]
    L. G. Meredith and M. Radestock.
    *A Reflective Higher-order Calculus*.
    ENTCS, 141(5):49–67, 2005.
    [doi:10.1016/j.entcs.2005.05.016](https://doi.org/10.1016/j.entcs.2005.05.016)

[WWPTWEA15]
    J. R. Wilcox et al.
    *Verdi: A Framework for Implementing and Formally Verifying Distributed Systems*.
    PLDI 2015.
    [doi:10.1145/2737924.2737958](https://doi.org/10.1145/2737924.2737958)

[San98]
    D. Sangiorgi.
    *On the bisimulation proof method*.
    MSCS, 8(5):447–479, 1998.
    [doi:10.1017/S0960129598002527](https://doi.org/10.1017/S0960129598002527)

[Mil89] R. Milner. *Communication and Concurrency*. Prentice-Hall, 1989.

[Mil99] R. Milner. *Communicating and Mobile Systems: The π-Calculus*.
Cambridge University Press, 1999.

[SW01] D. Sangiorgi and D. Walker. *The π-Calculus: A Theory of Mobile
Processes*. Cambridge University Press, 2001.

[Rocq] The Rocq Development Team.
*The Rocq Prover Reference Manual*, version 9.1.0.
[https://rocq-prover.org/doc/](https://rocq-prover.org/doc/)

---

## 14 · Traceability

The finding-by-finding promotion ledger is maintained in
[`slashing-traceability.md`](./slashing-traceability.md). That ledger is
part of the verification artifact: it records whether each Sage or
Hypothesis witness is a confirmed fixed Rust bug, a model boundary, a
projection risk, an assumption counterexample, or a proof/model
strengthening item.

The verification policy is:

```
finite witness ≠ proof authority
finite witness ≠ Rust source bug
```

A witness can justify a production Rust source change only after it is
reproduced on the production Rust path. Otherwise it is promoted through
formal preconditions, TLA+ classification, regression tests, and
specification/design documentation.

## 15 · Authorized Slash Evidence Proofs

The 2026 authorization/projection fix adds four Rocq modules, one
authorization TLA+ model, and one justification-projection TLA+ model.

| Claim | Rocq theorem / TLA+ invariant | Meaning |
| --- | --- | --- |
| Stale same-key evidence cannot authorize a later lifetime | `stale_evidence_not_authorized`; `main_T9_12_stale_evidence_not_authorized`; `Inv_StaleEvidenceCannotSlashRebondedKey` | Evidence for `(v, e₁)` is rejected for `(v, e₂)` when `e₁ ≠ e₂`. |
| Unknown slash evidence is no-effect | `execute_unknown_evidence_noop`; `unauthorized_unknown_execution_noop`; `main_T9_13_unknown_slash_evidence_noop` | Removing the deployer-slash fallback prevents unknown hashes from changing PoS state. |
| Invalid system auth token is no-effect | `execute_invalid_auth_token_noop`; `main_TAuth_invalid_token_noop`; `Inv_InvalidAuthSlashNoPending` | A spoofed slash deploy with a bad system auth token cannot mutate PoS state or create slash authorization. |
| Valid system auth token preserves slash semantics | `execute_valid_auth_token_equiv`; `main_TAuth_valid_token_equiv` | The auth wrapper is conservative for valid system deploys. |
| Slash candidates carry the authorized target epoch | `deploy_epoch_matches_target`; `Inv_OnlyAuthorizedSlashCanBePending` | A proposer cannot create a pending slash deploy unless the invalid evidence, current epoch, and target epoch agree. |
| Invalid-index evidence closes the liveness gap | `Inv_NoInvalidLatestLivenessGap` | Current authorized invalid evidence is immediately representable as a pending slash candidate; it no longer depends on `invalid_latest_messages`. |
| Boundary sequence arithmetic is total | `checked_pred_total_positive`; `checked_succ_bounded_sound`; `main_T9_14_checked_pred_positive` | Normal arithmetic is preserved and overflow/underflow paths are explicit. |
| Duplicate justification validators are rejected | `duplicate_head_rejected`; `main_T9_15_duplicate_justifications_rejected`; `Inv_DuplicateJustificationsRejected`; `Inv_AcceptedProjectionCardinality` | Detector projection is deterministic because malformed duplicate-validator inputs are invalid first. |

The bounded authorization TLC instance is
`formal/tlaplus/slashing/MC_AuthorizedSlashFlow.tla` with configuration
`MC_AuthorizedSlashFlow.cfg`. The duplicate-justification projection instance
is `formal/tlaplus/slashing/MC_JustificationProjection.tla` with
configuration `MC_JustificationProjection.cfg`. The latest bounded runs
completed without invariant violations: 802,816 distinct states for
authorization and 40 distinct states for duplicate-justification projection.

---

*"E Pluribus Potentia"*
