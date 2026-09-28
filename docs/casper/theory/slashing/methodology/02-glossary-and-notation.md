# 02 · Glossary and notation

This chapter defines every symbol, acronym, and term used in the
methodology directory. The convention is the same as in
[`../design/02-glossary-and-notation.md`](../design/02-glossary-and-notation.md)
— terms are introduced **before** first use. If you encounter an
undefined identifier in any other chapter, it is a documentation bug;
please file it.

The chapter is organized as:

- [§1 — Decision-tree symbols](#1--decision-tree-symbols-for-picking-a-tool)
- [§2 — Common formal symbols](#2--common-formal-symbols)
- [§3 — Slashing-domain terms](#3--slashing-domain-terms-cross-reference-to-design02)
- [§4 — Tooling acronyms](#4--tooling-acronyms)
- [§5 — Evidence-class vocabulary](#5--evidence-class-vocabulary)
- [§6 — Pseudocode conventions](#6--pseudocode-conventions)
- [§7 — Diagram color legend](#7--diagram-color-legend)

---

## 1 · Decision-tree symbols (for picking a tool)

Each candidate property `φ` is described by a 4-tuple
`⟨ shape, domain, observability, cost-budget ⟩`:

| Symbol   | Meaning                                                                              | Example value                                                  |
|----------|--------------------------------------------------------------------------------------|----------------------------------------------------------------|
| `shape`  | The logical form of the property                                                     | `∀x. P(x)`, `∃x. ¬P(x)`, `□Q`, `◇R`, `P ⊨ Q`                   |
| `domain` | The state space the property quantifies over                                         | unbounded, bounded (≤ 10⁶ states), single-function, byte-level |
| `obs`    | Whether the property is observable from the system's *interface* or only its *state* | interface (logs / records / fork-choice), internal (heap)      |
| `budget` | Cost the engineer is willing to pay for an answer                                    | minutes (proptest), hours (TLC), days (Rocq)                   |

The decision tree in
[`01-philosophy.md §3.2`](./01-philosophy.md#32--picking-the-tool--the-decision-tree)
uses these tags.

---

## 2 · Common formal symbols

| Symbol   | Read as                  | Meaning                                                                           |
|----------|--------------------------|-----------------------------------------------------------------------------------|
| `∀`      | for all                  | universal quantifier                                                              |
| `∃`      | there exists             | existential quantifier                                                            |
| `∃!`     | there exists exactly one | unique existence                                                                  |
| `∧`      | and                      | conjunction                                                                       |
| `∨`      | or                       | disjunction                                                                       |
| `¬`      | not                      | negation                                                                          |
| `⇒`      | implies                  | material implication                                                              |
| `⇔`      | if and only if           | logical equivalence                                                               |
| `≡`      | equivalent               | semantic equivalence (e.g. sets with same members)                                |
| `≈`      | bisimilar                | weak observational equivalence                                                    |
| `⊨`      | models                   | satisfaction (`s ⊨ φ` reads *“state `s` satisfies `φ`”*)                          |
| `⊢`      | proves                   | derivability (`Γ ⊢ φ` reads *“`Γ` proves `φ`”*)                                   |
| `□`      | always                   | LTL/CTL temporal "always" — `□ φ` holds iff `φ` holds at every reachable state    |
| `◇`      | eventually               | LTL/CTL temporal "eventually" — `◇ φ` holds iff `φ` holds at some reachable state |
| `▷`      | next                     | LTL "in the next state"                                                           |
| `⊥`      | bottom / false           | falsity                                                                           |
| `⊤`      | top / true               | truth                                                                             |
| `↦`      | maps to                  | function mapping                                                                  |
| `→`      | function arrow           | type-level function                                                               |
| `↪`      | injects                  | injective function                                                                |
| `↠`      | surjects                 | surjective function                                                               |
| `↣`      | partial mono             | partial injection                                                                 |
| `⊆`      | subset of                | inclusion                                                                         |
| `⊂`      | strict subset            | strict inclusion                                                                  |
| `∈`      | element of               | membership                                                                        |
| `∪`      | union                    | set union                                                                         |
| `∩`      | intersection             | set intersection                                                                  |
| `∖`      | set minus                | set difference                                                                    |
| `\|S\|`  | cardinality              | size of finite set `S`                                                            |
| `ℕ`      | naturals                 | non-negative integers                                                             |
| `ℤ`      | integers                 | signed integers                                                                   |
| `2^S`    | power set                | the set of subsets of `S`                                                         |
| `∅`      | empty set                | the set with no elements                                                          |
| `⌈ · ⌉`  | ceiling                  | smallest integer not less than the argument                                       |
| `⌊ · ⌋`  | floor                    | largest integer not greater than the argument                                     |
| `≜`      | defined as               | left-hand side is defined to be the right-hand side                               |
| `:=`     | assignment               | imperative assignment in pseudocode                                               |
| `≠`      | not equal                | inequality                                                                        |
| `≤`, `≥` | leq, geq                 | non-strict order                                                                  |
| `‖ · ‖`  | norm                     | size of an object measured by its weight or length                                |

### 2.1 Temporal operators in TLA⁺ context

The TLA⁺ specifications at `formal/tlaplus/slashing/` use the
Lamport-style temporal operators [Lam02]:

| Operator  | Meaning                                                                                                  |
|-----------|----------------------------------------------------------------------------------------------------------|
| `[]`      | Always (same as `□`)                                                                                     |
| `<>`      | Eventually (same as `◇`)                                                                                 |
| `~>`      | Leads to: `P ~> Q ≜ □(P ⇒ ◇Q)` — every state satisfying `P` is eventually followed by one satisfying `Q` |
| `WF_v(A)` | Weak fairness on action `A` with variable tuple `v`                                                      |
| `SF_v(A)` | Strong fairness on action `A` with variable tuple `v`                                                    |

---

## 3 · Slashing-domain terms (cross-reference to design/02)

The terms below come from
[`../design/02-glossary-and-notation.md`](../design/02-glossary-and-notation.md);
they are restated here for self-containment.

| Term                        | Definition                                                                                                                           |
|-----------------------------|--------------------------------------------------------------------------------------------------------------------------------------|
| **Validator**               | An on-chain identity registered in `PoS` with a stake bond; participates in proposing and finalizing blocks                          |
| **Bond**                    | The stake amount a validator has at risk; slashing zeros this amount and excludes the validator from fork choice                     |
| **Block**                   | A signed message containing parent hashes, justifications, system deploys, user deploys, and a sequence number                       |
| **Justification**           | A pointer from a block to a *latest message* of another validator, used by the detector to compute the view at that block            |
| **Latest message**          | The most-recent block by validator `v` that block `b` recognises in its justifications                                               |
| **Sequence number** (`s`)   | A per-validator monotonically-increasing counter; equivocation = two distinct blocks at the same `s`                                 |
| **Equivocation**            | A validator signing two distinct blocks at the same sequence number (the canonical slashable offense)                                |
| **Admissible equivocation** | An equivocation whose offending block is requested as a dependency by another block (so the receiver must process it)                |
| **Ignorable equivocation**  | An equivocation whose offending block is not requested as a dependency (so the receiver can ignore it without missing data)          |
| **Neglected equivocation**  | A block that fails to acknowledge a previously-recorded equivocation in its justifications                                           |
| **EquivocationRecord**      | The persistent record (`Validator × BaseSeqNum × Set BlockHash`) of an observed equivocation                                         |
| **Fork choice**             | The function `LMD-GHOST` that selects the canonical chain head; slashed validators contribute zero weight                            |
| **`SlashDeploy`**           | A system-deploy carrying a slash invocation; minted by the proposer, validated by the receiver, executed in the PoS Rholang contract |
| **`PoS`**                   | The on-chain Rholang Proof-of-Stake contract that holds the bond map, the coop vault, and the `slash` / `withdraw` methods           |
| **Coop vault**              | The Rholang vault that receives slashed bond funds                                                                                   |
| **Two-level slashing**      | The closure operation that slashes not only direct offenders but also validators whose blocks neglect those offenders                |
| **BFT bound**               | The classical Byzantine-fault tolerance bound `f < n/3` (number of equivocators bounded by ⅓ of validators)                          |
| **Tracker race**            | A lost-update race in the lock-free version of `EquivocationsTracker` (Bug #2)                                                       |
| **Epoch**                   | A block-number range `[k·L, (k+1)·L)` for epoch length `L`; slash authorization is current-epoch-only                                |

---

## 4 · Tooling acronyms

| Acronym        | Expansion                                                                                                      | Used in chapter                                                                                                    |
|----------------|----------------------------------------------------------------------------------------------------------------|--------------------------------------------------------------------------------------------------------------------|
| **Rocq** (Coq) | The proof assistant formerly called Coq, used for kernel-checked theorems                                      | [`formal-methods/01-mechanized-proof-rocq.md`](./formal-methods/01-mechanized-proof-rocq.md)                       |
| **TLA⁺**       | Temporal Logic of Actions; Lamport's specification language [Lam94, Lam02]                                     | [`formal-methods/02-model-checking-tla.md`](./formal-methods/02-model-checking-tla.md)                             |
| **TLC**        | The explicit-state model checker for TLA⁺                                                                      | same                                                                                                               |
| **Apalache**   | The SMT-backed symbolic model checker for TLA⁺ [KKT19]                                                         | same                                                                                                               |
| **Kani**       | A bounded model checker for Rust [VanH22]                                                                      | [`formal-methods/03-symbolic-rust-kani.md`](./formal-methods/03-symbolic-rust-kani.md)                             |
| **CBMC**       | The C bounded model checker that Kani is built on [Kro03]                                                      | same                                                                                                               |
| **SMT**        | Satisfiability modulo theories                                                                                 | passim                                                                                                             |
| **Sage**       | Open-source mathematics software system; here used for exact finite modeling                                   | [`formal-methods/04-finite-modeling-sage.md`](./formal-methods/04-finite-modeling-sage.md)                         |
| **proptest**   | Rust property-based testing library inspired by QuickCheck                                                     | [`randomized-search/01-property-testing-proptest.md`](./randomized-search/01-property-testing-proptest.md)         |
| **QuickCheck** | The original property-based testing library [CH00]                                                             | same                                                                                                               |
| **Hypothesis** | Python property-based testing framework with shrinking and stateful testing                                    | [`randomized-search/02-stateful-hypothesis.md`](./randomized-search/02-stateful-hypothesis.md)                     |
| **cargo-fuzz** | Cargo subcommand that drives libFuzzer-style coverage-guided fuzzing of Rust                                   | [`randomized-search/03-coverage-guided-fuzzing.md`](./randomized-search/03-coverage-guided-fuzzing.md)             |
| **libFuzzer**  | LLVM in-process coverage-guided fuzzer [Ser16]                                                                 | same                                                                                                               |
| **Loom**       | Permutation-search testing for concurrent Rust under C11 memory model                                          | [`randomized-search/04-concurrency-interleaving-loom.md`](./randomized-search/04-concurrency-interleaving-loom.md) |
| **Miri**       | The Rust mid-level IR interpreter; detects undefined behavior                                                  | same                                                                                                               |
| **ASAN**       | AddressSanitizer; LLVM runtime memory-error detector                                                           | same                                                                                                               |
| **STRIDE**     | Spoofing / Tampering / Repudiation / Info-disclosure / DoS / Elevation; Microsoft threat-model taxonomy [HL06] | [`attack-modeling/01-stride-and-attack-trees.md`](./attack-modeling/01-stride-and-attack-trees.md)                 |
| **PBFT**       | Practical Byzantine Fault Tolerance [CL99]                                                                     | passim                                                                                                             |
| **GHOST**      | Greedy Heaviest Observed Sub-Tree; the fork-choice rule used by CBC Casper [SZ15]                              | passim                                                                                                             |
| **CBC Casper** | Correct-by-Construction Casper [Zam17]                                                                         | passim                                                                                                             |
| **BFT**        | Byzantine fault tolerance                                                                                      | passim                                                                                                             |
| **LTS**        | Labeled transition system [Mil89]                                                                              | passim                                                                                                             |
| **PoS**        | Proof-of-stake (the Rholang on-chain contract in this work)                                                    | passim                                                                                                             |
| **DAG**        | Directed acyclic graph                                                                                         | passim                                                                                                             |
| **CI**         | Continuous integration                                                                                         | passim                                                                                                             |

---

## 5 · Evidence-class vocabulary

The traceability ledger
([`../slashing-traceability.md`](../slashing-traceability.md)) uses
eight status codes for findings; the threat model
([`../slashing-threat-model.md`](../slashing-threat-model.md) §4)
uses six classification codes. The mapping is documented in
[`pipeline/02-classification-taxonomy.md`](./pipeline/02-classification-taxonomy.md);
the symbols are summarized here so they can be used freely in earlier
chapters.

### 5.1 Threat-model classes (6)

| Class                       | Meaning                                                                                             |
|-----------------------------|-----------------------------------------------------------------------------------------------------|
| `bisimilar`                 | Rust and Scala produce observationally equivalent behavior; the witness exhibits expected agreement |
| `permitted_bug_fix`         | Rust deliberately diverges from Scala; the divergence corrects a known Scala defect                 |
| `candidate_boundary`        | The behavior depends on an explicit theorem precondition or scope clause                            |
| `projection_risk`           | A model-to-code projection could diverge under bounded shifts of inputs                             |
| `assumption_counterexample` | The witness proves a theorem precondition is necessary (cannot be weakened)                         |
| `unexpected`                | Transient class; every finding must be reclassified out of `unexpected`                             |

### 5.2 Traceability statuses (8)

| Status                         | Meaning                                                                                    |
|--------------------------------|--------------------------------------------------------------------------------------------|
| `confirmed_current_bug`        | The witness reproduces on current production Rust; a source fix is required                |
| `confirmed_fixed_bug`          | The witness reproduces only on pre-fix Rust/Scala; the current Rust intentionally differs  |
| `not_reproduced_in_rust`       | The witness is a model behavior; the production path does not exhibit it                   |
| `model_boundary`               | The behavior is explicitly out of scope for the relevant theorem                           |
| `projection_risk_guarded`      | A projection could diverge; the code is guarded by tests, specification text, or both      |
| `assumption_counterexample`    | The witness establishes that a theorem precondition is load-bearing                        |
| `proof_or_model_strengthening` | The witness suggests a stronger theorem or invariant should be mechanized                  |
| `needs_source_audit`           | The witness is interesting but the production path has not yet been audited closely enough |

---

## 6 · Pseudocode conventions

Pseudocode in this directory uses the following conventions:

1. **Unicode mathematical notation** for predicates and set
   operations: `∀ v ∈ Validators. is_bonded(v) ⇒ bond(v) > 0`.
2. **`▸` left-aligned step prefix** for numbered steps:
   ```
   ▸ 1. read state s
   ▸ 2. compute hash h ← H(s)
   ```
3. **`◂` for backtracking / failure paths**:
   ```
   ◂ on failure: write to dead-letter queue and abort
   ```
4. **`▷` for action labels** inside transition systems, matching the
   `▷` arrow used in Plotkin-style operational semantics [Plo04].
5. **Calligraphic letters** for sets of states / actions / processes:
   `𝒮` for states, `𝒜` for actions, `𝒫` for processes.
6. **Bold for keywords** in literate algorithms: **algorithm**,
   **assume**, **invariant**, **let**, **loop**, **match**, **return**.
7. **`←` for assignment** to a name; **`:=`** for definitional
   equality in algorithm scope.
8. **Indentation** carries scope (Python-style); explicit `end` is
   omitted.
9. **`(* … *)`** for comments inside literate blocks, matching the
   Rocq/OCaml convention.
10. **`#`** for one-line comments inside Sage / pseudocode blocks.

Example:

```
algorithm DispatchInvalidBlock(b : Block) → Status:
    (* §4 of design/04-detection-and-pipeline.md *)
    let v ← signer(b)
    let s ← seq_num(b)
    if exists b' ≠ b with signer(b') = v ∧ seq_num(b') = s:
        ▸ classify as Admissible if requested else Ignorable
        record ← (v, s−1, {hash(b), hash(b')})
        EquivocationsTracker.insert(record)
        return Status::IgnorableEquivocation
    else:
        return Status::Valid
```

---

## 7 · Diagram color legend

All PlantUML diagrams in [`diagrams/`](./diagrams/) use the same
palette as the existing slashing diagrams (see
[`../diagrams/`](../diagrams/)). The semantics are:

| Edge color | Hex       | Meaning                                          |
|------------|-----------|--------------------------------------------------|
| Blue       | `#1565C0` | Read query (lookups, snapshots)                  |
| Orange     | `#E65100` | Detection verdict, record creation, slash effect |
| Purple     | `#6A1B9A` | Rholang / system-deploy invocation               |
| Teal       | `#00838F` | Inter-component message                          |
| Gray       | `#546E7A` | Internal delegation                              |
| Red        | `#C62828` | Refutation / counterexample / promotion to bug   |
| Green      | `#2E7D32` | Promotion to mechanized theorem or invariant     |

Node background colors group components by layer:

| Layer          | Hex       | Examples                               |
|----------------|-----------|----------------------------------------|
| Formal methods | `#EEF2FF` | Rocq, TLA⁺, Kani                       |
| Randomized     | `#FFF7EE` | proptest, Hypothesis, libFuzzer, Loom  |
| Differential   | `#F3E5F5` | Rust↔Scala oracle, triple-bisim driver |
| Adversarial    | `#FFEBEE` | Damage optimizer, deep-threat          |
| Pipeline       | `#EEFAF1` | Ledger, classifier                     |
| Data artifacts | `#FFFFFF` | Theorems, invariants, fixtures         |

These are kept consistent with the production-diagram conventions so
a reader switching between the design suite and the methodology suite
sees the same visual semantics.

---

## 8 · External references

The acronyms above cite primary literature; the consolidated
bibliography with DOIs is in [`references.md`](./references.md). The
slashing-domain references are in
[`../design/13-references.md`](../design/13-references.md) and
[`../slashing-verification.md §16`](../slashing-verification.md).
