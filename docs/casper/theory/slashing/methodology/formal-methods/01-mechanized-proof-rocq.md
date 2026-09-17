# 01 · Mechanized proof with Rocq (Coq)

> *“In mathematics you don't understand things. You just get used to
> them.”* — John von Neumann, attributed.
>
> *“A proof is what convinces a reasonable man; a proof by induction
> convinces a stubborn one.”* — Mark Kac, attributed.

This chapter explains why Rocq sits at the top of the methodology
stack, what it is good at, what it is *not* good at, and how the
slashing effort uses it concretely. The chapter is organized as:

- [§1 — What Rocq buys you that nothing else does](#1--what-rocq-buys-you-that-nothing-else-does)
- [§2 — When *not* to reach for Rocq](#2--when-not-to-reach-for-rocq)
- [§3 — How the slashing Rocq modules are structured](#3--how-the-slashing-rocq-modules-are-structured)
- [§4 — The trust base — what we *assume*](#4--the-trust-base--what-we-assume)
- [§5 — A literate pseudocode walkthrough](#5--a-literate-pseudocode-walkthrough-of-the-detection-soundness-theorem)
- [§6 — Pitfalls and anti-patterns](#6--pitfalls-and-anti-patterns)
- [§7 — Related work](#7--related-work)

---

## 1 · What Rocq buys you that nothing else does

Rocq's value proposition rests on three properties of its trusted
kernel [CoqArt04, Pau13]:

1. **De Bruijn criterion (small trusted base)** — only the kernel
   typechecker is trusted; all tactics, automation, plug-ins, and
   user-defined notation produce proof terms that the kernel
   independently re-checks. A bug in `auto`, in `omega`, in `lia`,
   in any tactic library, cannot accept a false theorem.
2. **Unbounded quantification** — Rocq theorems are statements
   about *all* validators, *all* DAG shapes, *all* schedules of any
   size — not finite bounded instances. Theorem T-1 (*“no honest
   validator is ever slashed”*) is universally true, not true for
   `n ≤ 8`.
3. **Definitional translation** — properties about Rust code can be
   stated by **defining** an inductive type that mirrors the Rust
   path *up to abstraction* and then proving theorems about the
   inductive. The Rocq theorem is then re-validated against the Rust
   code by writing a `proptest` that shrinks differences (see
   [`../differential-and-metamorphic/03-triple-bisimilarity.md`](../differential-and-metamorphic/03-triple-bisimilarity.md)).

These three together yield the **strongest evidence** the methodology
admits: a closed-under-global-context derivation, falsifiable only by
producing a contradictory term in the same kernel.

### 1.1 The slashing-specific payoff

The headline claim of the slashing port — *Rust ≈ Scala modulo
sixteen bug fixes* — could not be discharged any other way. A
bisimilarity is a coinductive statement quantifying over all infinite
labeled transition sequences; no finite testing apparatus can
establish it. Rocq's coinductive `CoFixpoint` reasoning [San12] makes
this proof tractable. See
[`../slashing-verification.md §8`](../../slashing-verification.md) for
the proof structure and `formal/rocq/slashing/theories/MainTheorem.v`
for the kernel-checked term.

### 1.2 Why kernel checking matters in practice

Three real-world failure modes the slashing program has avoided
because of Rocq's kernel:

1. **Hidden circular reasoning** — A tactic library cannot prove a
   lemma by appealing to the very theorem it is supposed to imply.
   The kernel re-checks the dependency graph and refuses cycles.
2. **Implicit instantiation drift** — Universe inconsistencies in
   higher-order quantifiers are caught at proof closure, not at
   tactic execution. The slashing development sets
   `Set Universe Polymorphism Off` (default) and is therefore
   universe-consistent by construction.
3. **Silent admit** — `Admitted` is allowed during development but
   becomes a global-context dependency that `Print Assumptions`
   surfaces. The slashing kernel-check requires the output to be
   `Closed under the global context` (see
   [`../slashing-verification.md §14`](../../slashing-verification.md));
   no admitted lemma may ever ship.

---

## 2 · When *not* to reach for Rocq

Rocq is expensive. The slashing development cost ≈ 2.5 person-weeks
to mechanize 6 100+ lines across 14 modules, with peak memory of
~12 GB per module on `Bisimulation.v` and `TwoLevelSlashing.v`.

The methodology deliberately does **not** use Rocq for:

| Avoided use                                          | Reason                                                                                             | Tool used instead                               |
|------------------------------------------------------|----------------------------------------------------------------------------------------------------|-------------------------------------------------|
| Proto round-trip equality                            | A theorem about byte-level encoding is brittle and provides little semantic insight                | `cargo-fuzz` round-trip targets                 |
| Bounded numeric overflow                             | Kani proves these in seconds; Rocq would require a custom integer model                            | Kani harnesses + libFuzzer envelope             |
| Concurrency interleavings                            | Rocq has no native scheduler; the proof would need to encode it from scratch                       | Loom (Rust) + TLA⁺ + TLC                        |
| Quick exploratory hypothesis                         | Cost of one Rocq theorem ≈ 100 proptests; the false-positive rate is irrelevant during exploration | proptest + Hypothesis                           |
| Findings from Sage that have not yet been classified | Promoting unclassified witnesses risks mechanizing model artifacts                                 | Wait until traceability ledger assigns a status |

The rule of thumb is:

> Reach for Rocq only when the property is **load-bearing, unbounded,
> and stable**.

“Stable” here means *the underlying definition is unlikely to
churn for engineering reasons*. Mechanizing a moving target is
expensive and wasted; mechanizing a definition that is itself
slated for replacement is worse than wasted.

---

## 3 · How the slashing Rocq modules are structured

The slashing Rocq development is a layered hierarchy of fourteen
modules, listed below in dependency order (lower modules depend on
nothing above them):

```
Validator.v                  ← validator identity & bond type
   ├── ValidatorLifetime.v   ← bonded/unbonded/slashed/withdrawn
   ├── DAGState.v            ← block, justifications, latest-message
   ├── EquivocationRecord.v  ← record algebra, monotonicity
   └── PoSContract.v         ← Rholang PoS state abstraction
        ├── EquivocationDetector.v     ← detector LTS, soundness, completeness
        ├── ForkChoice.v               ← LMD-GHOST exclusion
        ├── InvalidBlock.v             ← 17 invalid-block reasons
        ├── SlashDeploy.v              ← system-deploy lifecycle
        └── TwoLevelSlashing.v         ← closure, termination, BFT bound
              ├── BugFixUnbondedProposer.v       ← Bug #8 fix
              ├── BugFixWithdrawTransferFailure.v ← Bug #10 fix (T-9.10/T-9.10'/T-9.10″)
              └── MainTheorem.v                  ← bisimilarity Rust ≈ Scala
```

Each module follows a four-section template:

| Section         | Contents                                                            |
|-----------------|---------------------------------------------------------------------|
| **Types**       | `Record` / `Inductive` declarations for the layer's data            |
| **Definitions** | Functions implementing the LTS transitions, predicates, projections |
| **Lemmas**      | Helper facts used by the chapter's main theorems                    |
| **Theorems**    | The load-bearing statements cited from `slashing-specification.md`  |

The dependency graph is acyclic and shallow (max depth 4). Building
the development with `coq_makefile -f _CoqProject -o Makefile && make -j1`
takes ≈ 8 minutes on a 16-core machine.

### 3.1 Why this shape, not a flat module?

A flat development would make every theorem visible everywhere, which
seems convenient but is actively harmful:

1. **Recompile cost** — touching `Validator.v` would recompile
   everything; the layered shape lets `EquivocationDetector.v` change
   without invalidating `PoSContract.v`.
2. **Cognitive load** — a 6 100-line module is unauditable; the
   fourteen modules average ~430 lines, each digestible in one
   sitting.
3. **Trust factoring** — `MainTheorem.v` depends on twelve other
   modules. An auditor reads them bottom-up and can mark each layer
   *“trusted to this depth”* before continuing.

---

## 4 · The trust base — what we *assume*

> *“What man knows is everywhere at war with what he wants.”* —
> Joseph Wood Krutch, *The Modern Temper*, 1929.

A Rocq theorem is not unconditionally true; it is true *given* the
trust base. The slashing development's trust base is:

| Trusted artifact                          | Why it is trusted                                                                                                                                                                          |
|-------------------------------------------|--------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| The Rocq kernel itself                    | The De Bruijn criterion — the kernel is small (~10 kLoC), audited, and used by thousands of developments                                                                                   |
| Rocq's standard library                   | Stable, audited, widely used                                                                                                                                                               |
| The classical-axiom-free fragment of Rocq | The slashing development uses no `Axiom`, no `Classical`, no `FunctionalExtensionality`, no `ProofIrrelevance`                                                                             |
| The Rust→Rocq abstraction (informal)      | The Rocq inductives mirror the Rust enum/struct definitions; the mapping is informal but is enforced by the harness/oracle bisimilarity tests in `casper/tests/slashing/oracle_adapter.rs` |
| `coqc` execution semantics                | The same kernel binary is used by everyone; reproducible against a pinned version                                                                                                          |

**Not** trusted (and therefore not used):

- Plug-ins outside the standard library (no `CoqHammer`, no `Mathcomp`
  beyond what's already in stdlib).
- Tactics that emit proof terms not checked by the kernel (none — all
  tactics in Rocq emit kernel-checked terms).
- Compiled Rust binaries (the bisimilarity claim is *between
  definitions*, not *between binaries*; runtime equivalence is
  separately validated by the triple-bisim harness).

The `Print Assumptions` discipline (described in
[`../slashing-verification.md §14`](../../slashing-verification.md))
mechanically enforces this trust base on every build.

### 4.1 The honest read-out of *“closed under the global context”*

The Rocq `Print Assumptions T` command lists every `Axiom`,
`Parameter`, `Variable`, and `Admitted` lemma that `T` transitively
depends on. The slashing development closes with the literal output:

```
Print Assumptions main_bisimilarity_theorem.
> Closed under the global context
```

This means: the theorem depends on no custom axiom, no parameter, no
admitted lemma. The trust base is exactly the Rocq kernel plus stdlib.

---

## 5 · A literate pseudocode walkthrough of the detection-soundness theorem

The theorem `t_1_detection_sound`
(`formal/rocq/slashing/theories/EquivocationDetector.v`) states:

> *No honest validator is ever slashed.*

Formally, in Rocq:

```rocq
Theorem t_1_detection_sound :
  ∀ (s : DAGState) (v : Validator) (b : Block),
    signer b = v →
    is_honest_in s v →
    classify s b ≠ Status::AdmissibleEquivocation ∧
    classify s b ≠ Status::IgnorableEquivocation ∧
    classify s b ≠ Status::NeglectedEquivocation.
```

Here `is_honest_in s v` is defined inductively as *“in DAG state `s`,
validator `v` has at most one block per sequence number, and every
justification it emits points at a real ancestor”*.

### 5.1 The hypothesis in informal English

If a validator follows the protocol — never signs two blocks at the
same sequence number, never points at a fabricated ancestor — then no
sequence of inputs to the detector causes it to classify any of that
validator's blocks as slashable.

### 5.2 The proof structure in literate pseudocode

```
proof of t_1_detection_sound:

    ▸ 1.  Introduce s, v, b; hypotheses: signer b = v, is_honest_in s v
    ▸ 2.  Case-split on classify s b:
          ┌────────────────────────────────────────────────────────┐
          │ Status::Valid             — goal trivially holds       │
          │ Status::InvalidBlock(_)   — not a slashable case (T-3) │
          │ Status::AdmissibleEquivocation                         │
          │ Status::IgnorableEquivocation                          │
          │ Status::NeglectedEquivocation                          │
          └────────────────────────────────────────────────────────┘
    ▸ 3.  For each of the three slashable cases:
          (a) by the detector's pre-condition,
              ∃ b' ≠ b, signer b' = v, seq b' = seq b
          (b) by is_honest_in s v,
              ∀ b₁ b₂. signer b₁ = v = signer b₂ ∧ seq b₁ = seq b₂ ⇒ b₁ = b₂
          (c) Contradiction: (a) yields two distinct b, b' with the
              same signer and seq, but (b) forces b = b'.
    ▸ 4.  Discharge by contradiction; goal holds in all cases.
    ▸ QED
```

The Rocq proof is ~85 lines. The literate form above captures
**every** step the kernel re-checks.

### 5.3 Why the proof is not just a tautology

It is tempting to read step (b) as a definition of honesty that
trivially excludes slashing. It is **not** a definition; it is the
*content* of the theorem `t_2_detection_complete` (the
contrapositive: a slashed validator was dishonest). The two
theorems together — soundness and completeness — pin down the
detector's behavior up to bisimilarity. Neither alone is enough.

This is the canonical *“why both soundness *and* completeness*”
reason from logic [Sho67]: a one-directional implication leaves
half the system's behavior unspecified.

---

## 6 · Pitfalls and anti-patterns

The slashing development has accumulated practical lessons. Each
appears at least once in the development's history.

### 6.1 Anti-pattern: proving the model, not the system

**Symptom**: A Rocq theorem mentions data shapes that do not appear
in the Rust source.

**Why it happens**: The Sage / Hypothesis output is closer to the
mathematical model than the Rust path is; mechanizing the witness
directly produces a Rocq theorem about *the model*, not the *system*.

**Fix**: Every Rocq theorem must have a counterpart predicate in
the `harness` or `oracle` test modules
([`../differential-and-metamorphic/03-triple-bisimilarity.md`](../differential-and-metamorphic/03-triple-bisimilarity.md))
that can be evaluated on real Rust state.

### 6.2 Anti-pattern: hiding a precondition inside a notation

**Symptom**: A theorem reads ` ∀ s, Q s ` but the `s` here ranges
over a subtype that excludes the case the auditor most wants to see.

**Why it happens**: Rocq supports subtype-style coercions and
notation aliases; an innocent-looking `Sig` or `subset_type`
quantifier silently weakens the theorem.

**Fix**: All quantifiers in the slashing development range over base
inductive types (`Validator`, `Block`, `DAGState`, `PoSState`); no
sigma-types appear in load-bearing statements.

### 6.3 Anti-pattern: tactic-driven proof structure

**Symptom**: The proof reads as a sequence of `apply lemma_X23`,
each lemma named after its proof number.

**Why it happens**: Tactic-mode proof grows organically; without
discipline it becomes opaque.

**Fix**: The slashing development factors every lemma into a named
mathematical proposition before proving it. The proof-script reads
top-down as English mathematics, not as proof-engineering.

### 6.4 Anti-pattern: depending on tactic automation

**Symptom**: A proof uses `intuition`, `eauto`, or `lia` without
breakdown.

**Why it happens**: Automation is fast.

**Mitigation**: Automation is allowed *only* at the leaves of the
proof tree; structural steps (`case`, `induction`, `exists`, `split`)
are always explicit. This keeps proofs auditable without requiring
the auditor to re-execute the tactic engine in their head.

---

## 7 · Related work

This methodology section draws on three lines of work:

- **Coq mechanization of distributed systems**:
  [HKR12 — Disel], [Wil15 — Verdi], [Gou12 — Software Foundations].
- **Bisimulation in process calculi**:
  Milner [Mil89], Sangiorgi [San98], [SW01].
- **Casper-specific formalizations**:
  Li *et al.* [CBCCoq20], Buterin & Griffith [BG19], Tsai [TFR20].

Full DOIs are in [`references.md`](../references.md) and the upstream
bibliography at [`../design/13-references.md`](../../design/13-references.md).

---

## 8 · Next chapter

[`02-model-checking-tla.md`](./02-model-checking-tla.md) — the
*exhaustive on finite bounds* arm of the formal-methods stack. Rocq
gives unbounded soundness; TLA⁺ + TLC gives finite-bound completeness
on the executable specification.
