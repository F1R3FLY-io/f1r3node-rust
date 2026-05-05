# Slashing — Formal Specification

**Version 1.0 · 2026-05-01**

> **Abstract.** This document gives a complete normative specification of the
> slashing layer of the F1R3FLY CBC Casper consensus implementation. It
> formalizes the eleven components of the slashing subsystem, the labeled
> transition system that connects them, the validator lifecycle, and the
> bisimilarity claim between the Rust port and the Scala original. It
> enumerates nine identified bug-fix deltas — eight inherited from the
> Scala upstream, one introduced by the Rust port — and specifies their
> corrected behavior. Every claim is anchored to a Rocq theorem in
> `formal/rocq/slashing/` and a TLA+ invariant in `formal/tlaplus/slashing/`,
> with the proofs translated to mathematical prose in the companion
> verification document `slashing-verification.md`.

---

## Table of Contents

1. [Introduction](#1-introduction)
2. [Glossary of symbols and key terms](#2-glossary-of-symbols-and-key-terms)
3. [The slashing subsystem — components and topology](#3-the-slashing-subsystem--components-and-topology)
4. [Operational semantics of detection](#4-operational-semantics-of-detection)
5. [The PoS slash transition](#5-the-pos-slash-transition)
6. [Validator lifecycle](#6-validator-lifecycle)
7. [The slashing pipeline](#7-the-slashing-pipeline)
8. [Two-level slashing](#8-two-level-slashing)
9. [Bisimilarity statement](#9-bisimilarity-statement)
10. [Bug-fix manifest](#10-bug-fix-manifest)
11. [Worked examples](#11-worked-examples)
12. [Use-case catalog](#12-use-case-catalog)
13. [Scope boundaries](#13-scope-boundaries)
14. [References](#14-references)

---

## 1 · Introduction

### 1.1 Problem statement and context

The F1r3fly slashing layer is the economic-security linchpin of the
consensus protocol. Validators put up a *bond* of tokens to participate in
block production; that bond is the collateral the protocol can seize when
a validator misbehaves in a *cryptographically-attributable* way. The
slashing layer detects misbehavior, records evidence on-chain, and
executes the punitive state transition that zeros the offender's bond and
removes them from the active set.

The Rust implementation in this repository was migrated from the Scala
original in EPOCH-005 as a 1:1 port. The detection plumbing arrived
intact; the *enforcement* path has known gaps that GitHub issue
[`F1R3FLY-io/f1r3node-rust#25`](https://github.com/F1R3FLY-io/f1r3node-rust/issues/25)
tracks. Three exploration agents have additionally identified one Rust-
introduced regression (a lock-free race in the equivocation tracker) and
catalogued seven inherited Scala defects that the port did not surface.

Without a normative specification, every defect remains encoded only as
inline `// TODO` markers; without machine-checked verification, no
implementation change can be audited against an authoritative reference.
This document, together with `slashing-verification.md` and the formal
artifacts under `formal/rocq/slashing/` and `formal/tlaplus/slashing/`,
fills that gap.

### 1.2 Contribution

This work contributes:

1. A **complete operational semantics** for the slashing layer, given as a
   labeled transition system (LTS) over an abstract state
   `S = (DAG, InvalidSet, EquivocationRecords, BondMap, ActiveValidators, SlashedSet, CoopVaultBalance)`
   — short form `(D, I, E, B, A, Sl, C)`; cf. §7.1.
   The semantics is realized as a Rocq inductive in
   `formal/rocq/slashing/theories/EquivocationDetector.v` and as a TLA+
   transition relation in `formal/tlaplus/slashing/SlashFlow.tla`.
2. A **bisimilarity proof**: under the nine documented bug-fix deltas, the
   Rust implementation is observationally bisimilar to the Scala original
   with respect to all observable barbs (block/state changes, fork-choice
   outcomes, vault balances). The proof lives in
   `Bisimulation.v` and `MainTheorem.v` and is summarized as Theorem 9.1
   in this document.
3. A **bug-fix manifest** of nine numbered defects, each with a stated
   cause, a proven-correct fix, and a TLA+ counter-example that fires
   pre-fix and passes post-fix.
4. A **use-case catalog** of 54 scenarios across four tiers (core,
   audit blockers, slashable-variant completion, operational and
   adversarial), each tagged for automated regression-test generation
   against the bug fixes.
5. **Ten PlantUML diagrams** (sources at `docs/theory/slashing/diagrams/*.puml`,
   rendered to SVG alongside) that reify the LTS visually and stay 1:1
   with the formal model.

### 1.3 Related work

The slashing model presented here builds on prior art in three areas:
proof-of-stake (PoS) protocol design (Buterin & Griffith on Friendly
Finality Gadget (FFG) slashing conditions [BG19]; Buterin et al. on slashing under fork-choice
[BHKPQRSWZ20]; Zamfir's CBC Casper essays [Z16]), accountability and
fork-evidence in BFT consensus (Buchman, Kwon, Milosevic [BKM18];
Amoussou-Guenou et al. [ABPT19]), and machine-checked verification of
distributed systems (Wilcox et al.'s Verdi framework [WWPTWEA15];
[CBCCoq20] on Coq formalization of CBC Casper). The bisimilarity argument
follows the standard pattern from [Mil89, MR05a, San98]. We also adapt
the documentation methodology from the cost-accounting verification
artifact at `f1r3node-cost-accounted-rho-calc/docs/theory/cost-accounted-rho-verification.md`.

### 1.4 Outline

§2 fixes notation and defines every symbol used in the remainder of the
document. §3 catalogues the 13 sub-components of the slashing subsystem
(grouped into 5 layers; cf. Diagram 01). §§4–5
give the operational semantics of detection and the PoS slash transition.
§6 captures the validator lifecycle. §7 walks the end-to-end slashing
pipeline; §8 elaborates the two-level closure. §9 states the bisimilarity
claim. §10 lists the bug-fix manifest. §11 gives worked examples. §12
enumerates the use-case catalog. §13 declares scope boundaries. §14 lists
references.

### 1.5 Verified properties — pedigree table

Following the cost-accounting precedent (verification doc §1.5), each
result is classified into one of four pedigree classes:

| Class                                   | Meaning                                                              | Examples in this work                                                                                          |
|-----------------------------------------|----------------------------------------------------------------------|----------------------------------------------------------------------------------------------------------------|
| **(a)** Direct mechanization            | Rocq encoding of an existing Scala/Rust algorithm                    | `EquivocationDetector.detect`, `prepare_slashing_deploys`, `is_slashable`                                      |
| **(b)** Verification of paper algorithm | Soundness/completeness of an algorithm we did not invent             | T-1, T-2, T-3, T-4, T-5, T-6, T-7, T-8, T-Idem (slash idempotence; alias T-9), T-10                            |
| **(c)** Proof-original extension        | New theorem whose statement is an original contribution of this work | T-13, T-14, T-15 (bisimilarity); T-9.1–T-9.9 (bug-fix correctness)                                             |
| **(d)** Citable-axiom-gated             | Result that depends on a cited classical lemma we do not re-prove    | None in the consensus-critical path; no Rocq theorem currently introduces a custom axiom |

**Theorem-naming convention.** Headline theorems are labeled `T-N`
where `N` is a small non-negative integer, plus three named theorems:
**`T-Idem`** (slash idempotence; §5.3, formerly `T-9`),
**`T-AuthCheck`** (system auth-token guard; §5.5 — Rholang-level
observation, not Rocq-mechanized), and the
five bisimulation-specific letter-suffixed theorems `T-13a/b/c` /
`T-15a/b` (§9.2). Bug-fix theorems are labeled `T-9.M` for `M = 1..9`.
The historical `T-9` (slash idempotence) was renamed to `T-Idem` in
this revision to avoid collision with the bug-fix family; older
artifacts may still use the alias.

### 1.6 Scale

| Artifact                            | Size                                            |
|-------------------------------------|-------------------------------------------------|
| This document                       | ~1,800 lines markdown (incl. embedded diagrams) |
| `slashing-verification.md`          | ~1,300 lines markdown                           |
| `formal/rocq/slashing/theories/*.v` | 21 modules / ~3,500 lines                       |
| `formal/tlaplus/slashing/*.tla`     | 5 base specs + 8 MC instances / ~1,100 lines    |
| PlantUML sources                    | 10 diagrams / ~1,500 lines                      |
| Total new artifact                  | ~9,500 lines                                    |

### 1.7 Component dependency DAG

The slashing subsystem comprises **13 sub-components grouped into 5
layers**, mirroring §3 and Diagram 01:

```
                ┌────────────────────────────────────────┐
                │  Detection layer                       │
                │  ┌──────────┐  ┌─────────────────────┐ │
                │  │ Validate │  │ EquivocationDetector│ │
                │  └────┬─────┘  └──────────┬──────────┘ │
                │       └─────────┬─────────┘            │
                │                 ▼                      │
                │     ┌──────────────────────────┐       │
                │     │  MultiParentCasperImpl   │       │
                │     └──────────────┬───────────┘       │
                └────────────────────┼───────────────────┘
                                     │
        ┌────────────────────────────┼────────────────────────────┐
        ▼                            ▼                            ▼
┌─────────────────┐         ┌─────────────────┐         ┌──────────────────┐
│ Storage layer   │         │ Proposing layer │         │ Effect layer     │
│                 │         │                 │         │                  │
│ BlockDagStorage │         │  BlockCreator   │         │ PoS Rholang      │
│ EquivTracker    │ ◄─────  │  SlashDeploy    │ ──────► │ Bond map         │
│ DeployStorage   │         │  SystemDeploy   │         │ Coop vault       │
│                 │         │      Util       │         │                  │
└────────┬────────┘         └─────────────────┘         └────────┬─────────┘
         │                                                       │
         │                                                       │
         └───────────────────────┬───────────────────────────────┘
                                 ▼
                     ┌────────────────────────┐
                     │   Fork-choice layer    │
                     │                        │
                     │      ForkChoice        │
                     └────────────────────────┘
```

The Rocq-module dependency view (Validate, EqDetector → BugFix*,
Bisimulation, MainTheorem) is the proof-artifact side of the same
subsystem and is presented in
[`slashing-verification.md` §1.3](./slashing-verification.md#13-scale-and-module-dag).

---

## 2 · Glossary of symbols and key terms

This section centralizes notation. Symbols are introduced in the order
they are needed; every symbol used after this section is defined here.

### 2.0 Acronyms

This subsection mirrors the design document's §02.1 so the
specification is self-contained.

| Acronym   | Expansion                         | First-use context                                             |
|-----------|-----------------------------------|---------------------------------------------------------------|
| **PoS**   | Proof of Stake                    | The consensus family this work targets (§1).                  |
| **BFT**   | Byzantine Fault Tolerance         | The bound `f < n/3` from [LSP82] (§9).                        |
| **CBC**   | Correct-by-Construction (Casper)  | The CBC-style consensus implemented in F1R3FLY (§1).          |
| **DAG**   | Directed Acyclic Graph            | The block graph (§3).                                         |
| **LMD**   | Latest-Message-Driven (GHOST)     | The fork-choice rule (§7).                                    |
| **RMW**   | Read-Modify-Write                 | The atomic primitive bug #2 protects (§7.2).                  |
| **TLC**   | TLA+ model checker (Lamport's)    | State-space exploration of TLA+ models (§10).                 |
| **LTS**   | Labeled Transition System         | The slashing pipeline's formal model `T = (S, L, →)` (§3).    |
| **GHOST** | Greedy Heaviest-Observed Sub-Tree | Fork-choice rule [LSZ15] (§7).                                |
| **FFG**   | Friendly Finality Gadget (Casper) | Ethereum 2.0 slashing comparison anchor [BG19].               |
| **DOS**   | Denial of Service                 | The vector closed by bug fix #1 (§10.1).                      |
| **KV**    | Key-Value                         | Store abstraction underlying the equivocation tracker (§3.4). |
| **BFS**   | Breadth-First Search              | Traversal algorithm in post-fix #7 (§10.7).                   |
| **TLA+**  | Temporal Logic of Actions         | Specification language; checked by TLC (§10).                 |

### 2.1 Process-algebraic notation (informal)

| Symbol | Name                             | Meaning                                                                                  |
|--------|----------------------------------|------------------------------------------------------------------------------------------|
| `→`    | LTS transition                   | `s →ℓ t` means the state machine moves from `s` to `t` under label `ℓ`                   |
| `→*`   | Multi-step                       | reflexive-transitive closure of `→`                                                      |
| `~`    | Strong bisimilarity              | mutual simulation under matching labels                                                  |
| `≈`    | Weak bisimilarity                | mutual simulation modulo internal `τ` steps                                              |
| `≡_α`  | α-equivalence                    | up to renaming of bound names                                                            |
| `↓ℓ`   | Barb                             | state can immediately perform observable action `ℓ`                                      |
| `⇓ℓ`   | Weak barb                        | state can perform `ℓ` after some `τ`-steps                                               |
| `≈ₓ`   | Barbed equivalence mod `x`       | bisimilarity up to barbs in set `x`                                                      |
| `⊥`    | Terminal absorbing state         | no further transitions enabled (used for `Removed → ⊥` in §6)                            |
| `⊤`    | Boolean true                     | used in detection-decision predicates                                                    |
| `⟶`    | LTS step (trace)                 | long arrow used in code-fenced traces (e.g. `sign(v,s,b) ⟶ DAG += b`); equivalent to `→` |
| `⟹`    | Logical implication              | used in formal LTS rules and theorem statements                                          |
| `∀`    | Universal quantifier             | rendered "for every" / "for all" in prose                                                |
| `∃`    | Existential quantifier           | rendered "there exists" in prose                                                         |
| `∅`    | Empty set / nil                  | e.g. the `EquivocationRecord` witness set after first insert                             |
| `∈`    | Set membership                   | standard                                                                                 |
| `⊆`    | Subset (inclusive)               | standard                                                                                 |
| `≡`    | Equivalence (mutual containment) | for sets/lists where iteration order is not observable                                   |
| `=`    | Strict equality                  | for natural numbers, function values, pointwise-equal maps                               |

The barbed-equivalence symbols `≈ₓ` and `↓ℓ` are introduced for
completeness; the prose throughout §3–§13 prefers the named-relation
form `weak_barbed_equiv` as it appears in the Rocq mechanization.

### 2.2 Slashing protocol terms

| Symbol            | Name                | Meaning                                                                                                            |
|-------------------|---------------------|--------------------------------------------------------------------------------------------------------------------|
| `V`               | Validators          | Set of validator identities (modeled as `nat` in Rocq for decidable equality; abstract `ByteString` in Rust/Scala) |
| `B`               | Blocks              | Set of blocks; each `b ∈ B` has fields `(sender(b), seq(b), hash(b), J(b))`                                        |
| `H`               | Block hashes        | Cryptographic block hashes                                                                                         |
| `J(b) ⊆ V × H`    | Justifications      | set of (validator, latest-block-hash) pairs cited by block `b`                                                     |
| `BondMap : V → ℕ` | Bond map            | partial function giving each validator's stake                                                                     |
| `EqRec`           | Equivocation record | triple `(v, baseSeqNum, witnesses ⊆ H)`                                                                            |
| `E ⊆ EqRec`       | Record set          | the equivocation tracker's contents                                                                                |
| `I ⊆ H`           | Invalid set         | hashes of blocks marked invalid                                                                                    |
| `A ⊆ V`           | Active set          | validators whose bond is positive                                                                                  |
| `Sl ⊆ V`          | Slashed set         | validators removed by a successful slash (renamed from `S` to avoid collision with the LTS state name; cf. §7.1)   |
| `C ∈ ℕ`           | Coop vault balance  | accumulated forfeited stake                                                                                        |

### 2.3 LTS labels

| Label                      | Meaning                                                                     |
|----------------------------|-----------------------------------------------------------------------------|
| `sign(v, s, b)`            | Validator `v` signs block `b` at sequence number `s`                        |
| `detect(v, s) → ib`        | Detector classifies the (v, s) signature set with InvalidBlock variant `ib` |
| `record(v, sn)`            | An EquivocationRecord is created for `(v, sn)`                              |
| `propose(p, deploys)`      | Proposer `p` emits a block carrying the listed slash deploys                |
| `executeSlash(o, ok)`      | PoS contract slashes offender `o` with success outcome `ok ∈ Bool`          |
| `filterFC(v)`              | Fork-choice removes `v`'s latest message                                    |
| `neglectDetect(v, target)` | Detector observes `v`'s justification cites `target` without slashing       |

### 2.4 InvalidBlock taxonomy

The `InvalidBlock` enum has 26 variants. The 17 marked **slashable** in
the current Rust source are:

```
AdmissibleEquivocation,  NeglectedEquivocation,    NeglectedInvalidBlock,
JustificationRegression, InvalidParents,            InvalidFollows,
InvalidBlockNumber,      InvalidSequenceNumber,     InvalidShardId,
InvalidRepeatDeploy,     DeployNotSigned,           InvalidTransaction,
InvalidBondsCache,       InvalidBlockHash,          ContainsExpiredDeploy,
ContainsTimeExpiredDeploy, ContainsFutureDeploy
```

The remaining 9 variants — `IgnorableEquivocation`, `InvalidFormat`,
`InvalidSignature`, `InvalidSender`, `InvalidVersion`, `InvalidTimestamp`,
`InvalidRejectedDeploy`, `NotOfInterest`, `LowDeployCost` — are
non-slashable today. **Bug fix #1 (T-9.1)** moves `IgnorableEquivocation`
into the slashable set.

### 2.5 Rocq ↔ TLA+ ↔ Rust crosswalk

For every load-bearing definition, this is the canonical mapping. The
verification doc §11 carries the full table; the most-cited ones:

| Concept               | Rocq                                             | TLA+                       | Rust                                             |
|-----------------------|--------------------------------------------------|----------------------------|--------------------------------------------------|
| Validator             | `Validator := nat`                               | `Validators`               | `Validator: ByteString`                          |
| Block                 | `Block` (record)                                 | implicit in `blocks[v][s]` | `BlockMessage` (proto)                           |
| InvalidBlock          | `InvalidBlock` (inductive)                       | string variant             | `InvalidBlock` (Rust enum, in `block_status.rs`) |
| EquivocationRecord    | `EqRec`                                          | `equivocationRecords` set  | `EquivocationRecord` (Rust struct)               |
| BondMap               | `BondMap := list (V * nat)`                      | `bonds` function           | `BondsMap: HashMap<Validator, Long>`             |
| `is_slashable(s)`     | `is_slashable : InvalidBlock -> bool`            | (abstracted; not modeled)  | `InvalidBlock::is_slashable`                     |
| Detect equivocation   | `equivocates : DAGState -> V -> nat -> Prop`     | `IsRealEquivocation(v,s)`  | `check_equivocations`                            |
| Slash effect          | `slash : PoSState -> V -> PoSState`              | `ExecuteSlash(o)`          | `@PoS!("slash", …)`                              |
| Bisimulation relation | `R : Rust × Scala -> Prop` (in `Bisimulation.v`) | structural equiv           | not directly observable                          |

### 2.6 Convention notes

- Sequence numbers are 0-indexed on the wire (matches Scala) but 1-indexed
  in the Rocq DAG abstraction so that `seq(b) - 1 ≥ 0`. The bisimulation
  relation accounts for this offset. Inside Rocq theorem statements
  (verification doc) the variable is `n` and arithmetic is rendered with
  Unicode minus (`n − 1`); inside spec pseudocode and tables, the
  variable is `s` (or `sn` for "sequence number") and arithmetic is
  rendered with ASCII hyphen (`s-1`, `sn-1`) to keep code-fenced
  excerpts mechanically transcribable. The two forms denote the same
  quantity; bisimilar by `eq_refl` over `nat`.
- Hash equality is decidable; we model hashes as `nat` in Rocq for
  decidability without committing to byte-level encoding. Bisimilarity is
  modulo this representation.
- Sets are modeled as duplicate-free lists in Rocq; iteration order is
  not observable in our equivalence (which is value-level, not byte-level
  on-disk). The Scala `Set` and Rust `BTreeSet` differ in iteration order
  but agree on element membership, hence bisimilar in our sense.

---

## 3 · The slashing subsystem — components and topology

The slashing subsystem comprises 13 sub-components organized into five layers:
**Detection**, **Storage**, **Proposing**, **Effect**, and **Fork-choice**.
[Diagram 01](./diagrams/01-component-overview.svg) gives the visual
overview; [Diagram 10](./diagrams/10-component-formal-correspondence.svg)
maps each component to its specification section, Rocq module, TLA+
module, and Rust source.

[![Diagram 01 — Slashing subsystem topology: 13 sub-components in five layers](./diagrams/01-component-overview.svg)](./diagrams/01-component-overview.svg)

[![Diagram 10 — Specification ↔ Rocq ↔ TLA+ ↔ Rust correspondence](./diagrams/10-component-formal-correspondence.svg)](./diagrams/10-component-formal-correspondence.svg)

### 3.1 Detection layer

#### 3.1.1 `Validate` (block validator)

Runs the seventeen-plus protocol checks against a candidate block,
yielding either `Valid` or `Invalid(ib)` for some `ib : InvalidBlock`.
The Rust implementation lives at `casper/src/rust/validate.rs`; the Scala
counterpart at `casper/src/main/scala/coop/rchain/casper/Validate.scala`.

Inputs: a `Block` and a DAG snapshot.
Outputs: `Either[BlockError, ValidBlock]`.
Side effects: none on success; on failure the caller
(`MultiParentCasperImpl`) records the block as invalid in the DAG.

#### 3.1.2 `EquivocationDetector`

Classifies an arriving block into one of the equivocation statuses:
`Valid`, `AdmissibleEquivocation`, `IgnorableEquivocation`, or
`NeglectedEquivocation`.

The decision rules mirror
`equivocation_detector.rs:24-104`:

```
detect(v, s, b):
  if no other block (v, s, b') with b' ≠ b exists in the DAG:
    return Valid
  else if b was requested as a dependency of some later block:
    return AdmissibleEquivocation
  else:
    return IgnorableEquivocation

detectNeglected(v, s, b):
  if (v, s-1) ∈ EquivocationRecords  AND
     b is requested as a dependency  AND
     no SlashDeploy targeting v in this branch:
    return NeglectedEquivocation
```

Rocq mechanization in `EquivocationDetector.v`. TLA+ model in
`EquivocationDetector.tla`. Theorems T-1 (soundness) and T-2
(completeness) are proved here. A memory-efficient equivalence-preserving
TLA+ rewrite `EquivocationDetectorEager.tla` is also provided; it
combines signing and detection into one atomic step (collapsing
classification interleavings) and converts the temporal property to a
safety invariant. See `slashing-verification.md` §10.4 for the
equivalence argument and the 10,896× state-space reduction.

#### 3.1.3 `MultiParentCasperImpl`

The orchestrator. It composes `Validate`, `EquivocationDetector`, and
`BlockDagStorage` to process incoming blocks. The critical method is
`handle_invalid_block(block, ib)` at `multi_parent_casper_impl.rs:1018-1112`:

```
match ib with
  | AdmissibleEquivocation ⟹
        record  ← BlockDagStorage.equivocation_records()
        if (block.sender, block.seq - 1) ∉ record:
          BlockDagStorage.insert_equivocation_record(
            block.sender, block.seq - 1, ∅)
        // ⚠ pre-fix: the read-then-insert is NOT atomic
        handle_invalid_block_effect(block)
  | IgnorableEquivocation ⟹
        // ⚠ pre-fix: silently dropped (DOS vector)
        log("Ignoring equivocation")
  | _ if is_slashable(ib) ⟹
        // ⚠ pre-fix: only flags as invalid; no record, no slash
        handle_invalid_block_effect(block)
  | _ ⟹ handle_invalid_block_effect(block)
```

Bug fixes #1, #2, and #3 modify the three commented branches; see §10.

### 3.2 Storage layer

#### 3.2.1 `BlockDagStorage`

The persistent DAG storage. Two relevant operations for slashing:

- `insert(block, invalid: Bool)` — adds a block to the DAG with an
  invalid flag.
- `access_equivocations_tracker { f }` — invokes `f` on the tracker
  under a global semaphore (Scala) or as a no-op wrapper (Rust pre-fix).

Rust source: `block-storage/src/rust/dag/block_dag_key_value_storage.rs`.
Scala: `coop/rchain/blockstorage/dag/BlockDagKeyValueStorage.scala`. The
read-modify-write semaphore is owned by `BlockDagKeyValueStorage.scala:262`
(`accessEquivocationsTracker { lock.withPermit(f(...)) }`, allocated via
`MetricsSemaphore.single[F]` at line 350) — **not** by
`EquivocationTrackerStore.scala`. The tracker store itself is a thin
wrapper over `KeyValueTypedStore` with no concurrency primitives of its
own; bug-#2 audits should look for the semaphore in
`BlockDagKeyValueStorage`.

The Rust port additionally exposes lock-free direct methods
(`equivocation_records()`, `insert_equivocation_record()`,
`update_equivocation_record()`); these bypass the lock and are the source
of bug #2.

#### 3.2.2 `EquivocationTrackerStore`

The KV layer underpinning the equivocation records. The map is keyed by
`(Validator, SequenceNumber)` and valued by the set of witness block
hashes. **No concurrency primitives** — atomicity is provided by the
caller (`BlockDagKeyValueStorage.accessEquivocationsTracker`, see §3.2.1).

Rust: `block-storage/src/rust/dag/equivocation_tracker_store.rs`.
Scala: `coop/rchain/blockstorage/dag/EquivocationTrackerStore.scala`.

The Scala uses `Set[BlockHash]` (unordered hash-set); the Rust uses
`BTreeSet<BlockHash>` (ordered). Iteration order differs but element
membership is identical, hence bisimilar at the value level.

#### 3.2.3 `DeployStorage`

Persists user-submitted deploys for replay determinism. **Slash deploys
are not currently persisted here** — this is the surface of bug fix #8
(T-9.8). The pre-fix behavior generates slash deploys on the fly during
`prepare_slashing_deploys` and never records them; replay is therefore
sensitive to the order in which proposers come up.

### 3.3 Proposing layer

#### 3.3.1 `BlockCreator`

The block-construction module. The relevant method is
`prepare_slashing_deploys(seqNum) → Seq[SlashDeploy]` at
`block_creator.rs:287-332`:

```
ilm                ← dag.invalid_latest_messages
ilmFromBonded      ← filter (v, _) ∈ ilm where bonds_map[v] > 0
slashingDeploys    ← map invalidBlockHash → SlashDeploy(...)
return slashingDeploys
```

The proposer attaches one `SlashDeploy` per offender to the next block it
produces. Bug fix #8 (T-9.8) adds a pre-condition that the proposer
itself must be bonded; pre-fix, an unbonded proposer generates wasted
deploys that the PoS contract will reject at the auth-token check.

#### 3.3.2 `SlashDeploy`

A system deploy that invokes the on-chain Rholang `slash` method. Body
(faithful to `coop/rchain/casper/util/rholang/costacc/SlashDeploy.scala:40-51`,
with `sys:casper:*` unforgeable bindings elided for readability):

```
new rl, poSCh, deployerId, invalidBlockHash, sysAuthToken, return in {
  rl!(`rho:system:pos`, *poSCh) |
  for(@(_, PoS) <- poSCh) {
    @PoS!("slash", *deployerId, *invalidBlockHash.hexToBytes(),
                   *sysAuthToken, *return)
  }
}
```

The full Scala form binds `rl` to `` `rho:registry:lookup` `` and the
remaining names to the unforgeable channels `sys:casper:deployerId`,
`sys:casper:invalidBlockHash`, `sys:casper:authToken`, and
`sys:casper:return`. Note: `invalidBlockHash` is a hex string in the
deploy and must be converted via `.hexToBytes()` before being passed
to `slash` (which expects raw bytes per `PoS.rhox:435`).

Source seed: `splitByte(1)` of `generateSlashDeployRandomSeed(selfId,
seqNum)`. Endianness is little-endian on both sides; the bisimulation
account holds.

Rust: `casper/src/rust/util/rholang/costacc/slash_deploy.rs`. Scala:
`coop/rchain/casper/util/rholang/costacc/SlashDeploy.scala`.

#### 3.3.3 `SystemDeployUtil`

Helper module that produces deterministic random seeds for system deploys.
The slash-marker byte is the literal `1` (no named `SLASH_MARKER`
constant in Scala — verified at `SystemDeployUtil.scala:55`). Bisimilar
across implementations — same byte layout, same `splitByte(1)` semantics.

### 3.4 Effect layer

#### 3.4.1 PoS Rholang contract

The on-chain `slash` method at
`casper/src/main/resources/PoS.rhox:435-495` (signature on line 435;
lines 432-434 are the `new commitRewards in {` wrapper plus block-comment
header preceding the signature). Same file is loaded into
both Rust and Scala interpreters; the contract itself is bisimilar by
construction. The contract:

1. Verifies the system auth token (rejects if invalid).
2. Looks up the offender via `invalidBlocks[blockHash]` (defaulting to
   the deployer if the lookup misses — a degraded-mode fallback).
3. Reads the offender's bond.
4. Transfers the bond to the Coop vault.
5. Updates `state.allBonds`, `state.activeValidators`,
   `state.committedRewards` as a single atomic `stateUpdateCh!`
   map-construction at `PoS.rhox:473-482` (one map write, not three
   field writes).
6. Returns `(true, Nil)` on `returnCh`.

Bug fix #4 (T-9.4) addresses the missing error path on transfer failure.

#### 3.4.2 Bond map / Validator registry

State held inside the PoS contract: `state.allBonds`,
`state.activeValidators`, `state.committedRewards`. Mutated by the slash
contract; read by `BlockCreator.prepare_slashing_deploys`.

#### 3.4.3 Coop vault

Recipient of forfeited stake. Mutated by the `posVault!("transfer",
coopMultiVaultAddr, valBond, …)` call in the slash contract. The vault is
itself a Rholang contract; we model it abstractly as a `nat` balance in
the formal account.

### 3.5 Fork-choice layer

#### 3.5.1 `ForkChoice` estimator

Reads the slashed-validator set from on-chain state and excludes those
validators' latest messages from the GHOST-style fork-choice computation.
This is the final step that gives a slashed validator zero influence over
future block selection. Theorem T-10 in
`formal/rocq/slashing/theories/ForkChoice.v` states this exclusion
formally.

Rust: `casper/src/rust/estimator.rs`. Scala:
`coop/rchain/casper/Estimator.scala`.

---

## 4 · Operational semantics of detection

The `EquivocationDetector` is modeled as a labeled transition system over
DAG states `(D, I, E, B)` where `D` is the set of known blocks, `I` the
invalid set, `E` the equivocation records, and `B` the bond map. Labels
carry the detector's classification.

### 4.1 Definitions

**Definition 4.1** *(Equivocation in the DAG).*
A validator `v` *equivocates* at sequence number `s` in DAG state `D`
iff there exist two distinct blocks `b1, b2 ∈ D` with `sender(bᵢ) = v`,
`seq(bᵢ) = s`, and `hash(b1) ≠ hash(b2)`. We write `equivocates(D, v, s)`
for the predicate.

The Rocq formalization is `equivocates : DAGState -> Validator -> nat ->
Prop` in `formal/rocq/slashing/theories/DAGState.v`. The boolean
counterpart `equivocates_b` is the function actually used by the
detector; the two are equivalent by reflection (`equivocates_dec` in the
same file).

**Definition 4.2** *(Requested as dependency).*
A block `b` is *requested as a dependency* in `D` iff some other block
`b' ∈ D` has `b.hash` in its justifications. We write
`requestedAsDep(D, b)`.

**Convention.** The Rocq mechanization passes `d := requestedAsDep(D, b)`
as a boolean parameter to `check_equivocations` rather than recomputing
it (the upstream block-processor knows the flag and forwards it). Spec
prose uses both forms interchangeably: the declarative form
`requestedAsDep(D, b)` in §4.3 detection rules, and the parametric form
`detect(state, validator, seq, d)` in §10 bug-fix theorem statements.
The two are equivalent by `requestedAsDep_iff_d` in
`EquivocationDetector.v`.

**Definition 4.3** *(Detection rules).* Given DAG state `S` and an arriving
block `b` with `sender(b) = v`, `seq(b) = s`:

```
detect(S, b) =
  | not equivocates(S, v, s)              ⟹ Valid
  | requestedAsDep(S, b)                   ⟹ AdmissibleEquivocation
  | otherwise                              ⟹ IgnorableEquivocation

detectNeglected(S, b) =
  | (v, s-1) ∈ E ∧ requestedAsDep(S, b)    ⟹ NeglectedEquivocation
  | otherwise                              ⟹ unchanged
```

### 4.2 Theorems

**Theorem 4.1 (T-1, Detection soundness).** *(`detection_sound`,
`EquivocationDetector.v:91`.)* For every DAG state `S`, validator `v`,
sequence number `s`, and block `b`,

```
  detect(S, b) ∈ {AdmissibleEquivocation, IgnorableEquivocation}
    ⟹ equivocates(S, v, s)
```

That is, the detector never flags a non-equivocation as an equivocation.
Proven in Rocq by case analysis on `detect`; the falsifying case
`Valid → equivocates` is handled by the early-return guard. TLC
model-checks the property `Inv_DetectionSound` in
`MC_EquivocationDetector.tla`.

**Theorem 4.2 (T-2, Detection completeness).** *(`detection_complete`,
`EquivocationDetector.v:111`.)* For every DAG state `S`, if
`equivocates(S, v, s)` and `b ∈ D` with `sender(b) = v`, `seq(b) = s`, then

```
  detect(S, b) ∈ {AdmissibleEquivocation, IgnorableEquivocation}
```

Proven by case analysis on `requestedAsDep(S, b)`. Both branches yield a
non-`Valid` status, so the disjunction holds. TLC verifies the temporal
property `Live_DetectionComplete` under fairness.

**Theorem 4.3 (T-3, Slashable taxonomy correctness).**
*(`slashable_post_fix_extends_pre_fix`, `InvalidBlock.v:151`.)* The post-fix
slashable set strictly extends the pre-fix slashable set by exactly the
`IgnorableEquivocation` variant; on all other variants the two predicates
agree. Proven by exhaustive case analysis on the 26-element enum.

The 18-element post-fix slashable set is

```
{ AdmissibleEquivocation, IgnorableEquivocation, NeglectedEquivocation,
  NeglectedInvalidBlock, JustificationRegression, InvalidParents,
  InvalidFollows, InvalidBlockNumber, InvalidSequenceNumber,
  InvalidShardId, InvalidRepeatDeploy, DeployNotSigned, InvalidTransaction,
  InvalidBondsCache, InvalidBlockHash, ContainsExpiredDeploy,
  ContainsTimeExpiredDeploy, ContainsFutureDeploy }
```

---

## 5 · The PoS slash transition

The on-chain `@PoS!("slash", deployerId, invalidBlockHash, sysAuthToken,
returnCh)` Rholang method is the punitive state transition. We model it
abstractly as a function `slash : PoSState × V → PoSState × Bool` where
the boolean indicates success. [Diagram 07](./diagrams/07-activity-pos-slash-contract.svg)
gives the activity flow.

[![Diagram 07 — PoS.slash() Rholang activity flow with the bug-#4 transfer-failure fix highlighted](./diagrams/07-activity-pos-slash-contract.svg)](./diagrams/07-activity-pos-slash-contract.svg)

### 5.1 PoS state

**Definition 5.1** *(PoS state).*
A `PoSState` is a 4-tuple `(allBonds, activeValidators, committedRewards,
coopVaultBalance)` with:

- `allBonds : V → ℕ` — the bond map
- `activeValidators ⊆ V` — currently bonded validators
- `committedRewards : V → ℕ` — pending rewards
- `coopVaultBalance ∈ ℕ` — accumulated forfeited stake

**Mechanization note.** The Rholang/Scala `PoSState` carries all four
fields; the Rocq mechanization at `PoSContract.v:40-44` records only
the three fields the slash transition actually mutates and observes:
`ps_allBonds`, `ps_active`, and `ps_coopVault`. The
`committedRewards` field is omitted from the Rocq record because
`slash` does not read or modify it in the formalized fragment (the
field is mutated by orthogonal reward-distribution logic outside the
slashing scope). The §5.2 prose semantics presents the four-field
view for parity with the Scala contract; T-7 / T-8 / T-Idem are
proven against the three-field Rocq record.

### 5.2 The slash transition

**Definition 5.2** *(slash semantics).* Given `PoSState ps` and offender
`v ∈ V`,

```
slash(ps, v) =
  | not authTokenValid                    ⟹ (ps, false)  [auth failure]
  | ps.allBonds[v] = 0                    ⟹ (ps, true)   [no-op idempotent]
  | otherwise:
      let b = ps.allBonds[v]
      transfer(coopVault, b)              [bug #4: assume success]
      ps' = { allBonds[v] := 0;
              activeValidators := activeValidators \\ {v};
              committedRewards := committedRewards \\ {v};
              coopVaultBalance += b }
      return (ps', true)
```

### 5.3 Theorems

**Theorem 5.1 (T-7, Slash zeros bond).** *(`slash_zeros_bond`,
`PoSContract.v:75`.)* For every `ps` and `v`,

```
  let (ps', _) = slash(ps, v) in  ps'.allBonds[v] = 0
```

Proven by direct unfolding. TLC verifies `Inv_BondsZeroAfterSlash` in
`MC_SlashFlow.tla`.

**Theorem 5.2 (T-8, Slash transfers stake).** *(`slash_transfers_stake`,
`PoSContract.v:95`.)* If the transfer succeeds (the `Bool` in the result is
`true`), then

```
  ps'.coopVaultBalance = ps.coopVaultBalance + ps.allBonds[v]
```

This relies on the transfer-success precondition; bug fix #4 (T-9.4)
guarantees that the transition either succeeds with this property or
returns `false` deterministically.

**Theorem 5.3 (T-Idem, Slash idempotence).** *(`slash_idempotent`,
`PoSContract.v:117`. Historical alias **T-9**; the alias is retained in
older artifacts but `T-Idem` is preferred to avoid collision with the
`T-9.M` bug-fix family.)* For every `ps` and `v`,

```
  let (ps1, _) = slash(ps, v) in
  let (ps2, _) = slash(ps1, v) in
  ps2 = ps1
```

A second slash on an already-slashed validator is a no-op. Proven via the
`bm_slash_idempotent_lookup` foundation lemma at
`Validator.v`.

**Theorem 5.4 (T-10, Fork-choice exclusion).** *(`fork_choice_exclusion`,
`ForkChoice.v:60`.)* If `v ∈ slashedSet`, then `v`'s latest message is
filtered from the fork-choice estimator.

**Observation 5.5 (T-AuthCheck, System auth-token guard).**
*Rholang-level observation, not currently mechanized in Rocq.* For
every PoS deploy invoking `@PoS!("slash", deployerId, blockHash,
sysAuthToken, returnCh)` with `sysAuthToken` not equal to the system
auth token introduced at PoS contract instantiation, the contract
rejects the deploy at the first guard
(`PoS.rhox:437-439`, `sysAuthTokenOps!("check", sysAuthToken,
*isValidTokenCh)`) with `returnCh!((false, "Invalid system auth
token"))` at `PoS.rhox:491`. No state mutation occurs and no transfer
is initiated. The Rocq `slash` definition at `PoSContract.v:59`
takes only `(ps, v)` and assumes the auth-token check has already
passed at this entry point (per the file header at `PoSContract.v:57`);
mechanizing `T-AuthCheck` would require extending `slash` with an
auth-token oracle following the pattern in `BugFixTransferFailure.v`.
Tracked in §13 future work. Cf. Diagram 07 left branch for the
Rholang-level activity flow.

---

## 6 · Validator lifecycle

A bonded validator transitions through seven states ([Diagram
06](./diagrams/06-state-validator-lifecycle.svg)). Note that
`EquivocatorSuspect` is a documentation-only intermediate state with
no observable witness in the implementation: in the Rust code, the
detector transitions `Bonded → EquivocatorRecorded` directly in one
atomic step (the suspect state is split out for narrative clarity in
the lifecycle diagram).

[![Diagram 06 — Validator lifecycle: Unbonded → Bonded → EquivocatorSuspect → EquivocatorRecorded → SlashPending → Slashed → Removed](./diagrams/06-state-validator-lifecycle.svg)](./diagrams/06-state-validator-lifecycle.svg)

```
Unbonded → Bonded → EquivocatorSuspect → EquivocatorRecorded →
SlashPending → Slashed → Removed
```

| Transition                                 | Trigger                                                 |
|--------------------------------------------|---------------------------------------------------------|
| `Unbonded → Bonded`                        | Successful `@PoS!("bond", …)` deploy                    |
| `Bonded → Bonded`                          | Honest activity (proposing, validating)                 |
| `Bonded → EquivocatorSuspect`              | Detector observes a second block at same seq num        |
| `EquivocatorSuspect → EquivocatorRecorded` | `insert_equivocation_record(v, sn-1, ∅)` succeeds       |
| `EquivocatorRecorded → SlashPending`       | Next proposer's `prepare_slashing_deploys` includes `v` |
| `SlashPending → Slashed`                   | `@PoS!("slash", …)` succeeds                            |
| `SlashPending → EquivocatorRecorded`       | Slash fails (transfer FIXME, bug fix #4)                |
| `Slashed → Removed`                        | PoS removes `v` from `activeValidators`                 |
| `Removed → ⊥`                              | Terminal — cannot rejoin without a new bond             |

Bug fix #2 (T-9.2) ensures the transition into `EquivocatorRecorded` is
atomic under concurrent insertions. Bug fix #4 (T-9.4) ensures
`SlashPending → EquivocatorRecorded` happens deterministically when the
Coop-vault transfer fails (rather than the validator being stuck in
`SlashPending` forever).

---

## 7 · The slashing pipeline

The end-to-end pipeline integrates detection, persistence, proposing, and
effect. [Diagram 02](./diagrams/02-seq-admissible-equivocation.svg) walks
the canonical happy path.

[![Diagram 02 — Admissible equivocation slash flow: detection → record → proposer → SlashDeploy → PoS → bond zero → fork-choice exclusion](./diagrams/02-seq-admissible-equivocation.svg)](./diagrams/02-seq-admissible-equivocation.svg)

### 7.1 Composition rule

The pipeline is the composition of the per-component LTSs. For state `S =
(D, I, E, B, A, Sl, C)` (where `Sl` is the slashed set, renamed from
`S` to avoid collision with the state name; cf. verification doc §3
which uses the same `Sl` convention) and an arriving block `b`:

```
S →ⁿsign(v,s,b)→ S'
S' →detect(v,s)→ub→ S''
  | ib = AdmissibleEquivocation ⟹ S'' →record(v, s-1)→ S'''
S''' →ⁿpropose(p, [SlashDeploy(b, ...)])→ S''''
S'''' →executeSlash(v, true)→ S'''''
S''''' →filterFC(v)→ S''''''
```

Each transition's effect on the state tuple is given by the
corresponding component's semantics in §§4–5. The composition rule is
formalized in `MainTheorem.v`.

### 7.2 Pre-fix vs post-fix behavior

The pre-fix pipeline at `multi_parent_casper_impl.rs:1018-1112` exhibits
three documented gaps that prevent the pipeline from completing for some
inputs:

- `IgnorableEquivocation` short-circuits at `detect` (no record, no
  slash). Bug fix #1 closes this.
- `is_slashable(ib) = true` for `ib ≠ AdmissibleEquivocation` falls
  through to a stub. Bug fix #3 routes these through the standard pipeline.
- The read-modify-write at `record(v, s-1)` is non-atomic in the Rust
  port. Bug fix #2 re-locks it.

[Diagram 05](./diagrams/05-seq-invalid-block-dispatch-fixed.svg) shows
the post-fix dispatch for a non-equivocation slashable variant
(`JustificationRegression`).

[![Diagram 05 — Generic invalid-block dispatch (post-fix #3) for JustificationRegression: detection → record → proposer's SlashDeploy](./diagrams/05-seq-invalid-block-dispatch-fixed.svg)](./diagrams/05-seq-invalid-block-dispatch-fixed.svg)

---

## 8 · Two-level slashing

F1r3fly slashing is **two-level**: in addition to slashing a direct
equivocator, the protocol slashes any validator who *witnessed* an
equivocation in their justifications and failed to slash it. This is the
"neglected equivocation" case. The economic effect makes collusion
mutually destructive: cover for an equivocator and you go down with them.

[Diagram 04](./diagrams/04-seq-two-level-slashing.svg) walks the example.

[![Diagram 04 — Two-level slashing: Validator A equivocates → A slashed; Validator B neglects → B also slashed in the same block (collusion is mutually destructive)](./diagrams/04-seq-two-level-slashing.svg)](./diagrams/04-seq-two-level-slashing.svg)

The data-flow that computes the neglect predicate from a block's
justifications is shown in [Diagram 08](./diagrams/08-dataflow-justifications-to-neglect.svg):

[![Diagram 08 — Data-flow: block.justifications → DAG lookup → invalidJustifications → neglectedInvalidJustification → reject-or-admit decision (with the post-fix Rust slash-system-deploy recovery branch)](./diagrams/08-dataflow-justifications-to-neglect.svg)](./diagrams/08-dataflow-justifications-to-neglect.svg)

### 8.1 Definitions

**Definition 8.1** *(Neglect graph).*
For state `S = (D, I, E, ...)`, define `neglect(v) ⊆ V` as

```
neglect(v) = { u : ∃ b ∈ D, sender(b) = v, b cites u in J(b),
                       u is in some EquivocationRecord ∈ E,
                       b carries no SlashDeploy targeting u }
```

**Definition 8.2** *(Slash closure).*
The slashed set `Sl` evolves as the least fixed point of

```
Sl ← Sl ∪ {v : neglect(v) ∩ Sl ≠ ∅}
```

starting from the direct equivocators.

### 8.2 Theorems

**Theorem 8.1 (T-11, Level-2 termination).**
*(`t_11_level_2_termination`, `TwoLevelSlashing.v:126`.)* The slash closure
reaches a fixed point in at most `|V|` iterations.

**Theorem 8.2 (T-12, Level-2 collusion-resistance).**
*(`t_12_bft_quorum_preservation`, `TwoLevelSlashing.v:174`.)* Under the BFT
precondition `|closure| ≤ F`, the slash closure preserves
`|universe| − |closure| ≥ |universe| − F`. With `F = ⌊(n−1)/3⌋` per
[LSP82], the active validator set after both levels of slashing fire
maintains quorum.

The corollary `t_12_bft_active_set_size` shows that with strict
`F < |universe|`, the active set is non-empty. The proof relies on the
cited BFT bound `f < n/3` from [LSP82] (cited in the trust base, §12 of
`slashing-verification.md`); `t_12_quorum_preservation` (the structural
list-length form) is the direct corollary used by the BFT-style proof.

**Theorem 8.3 (Reachability characterization).**
*(`slash_iter_reachability_characterization`, `TwoLevelSlashing.v`.)*
The level-2 closure is exactly reverse reachability in the neglect graph:
a validator is slashed after `n` iterations iff it is either a direct
offender or has a directed neglect path of length at most `n` to a direct
offender.

**Theorem 8.4 (Weighted quorum preservation).**
*(`weighted_slash_iter_quorum_preservation`, `TwoLevelSlashing.v`.)*
For a stake function `stake : Validator -> nat`, if the total stake in
the slash closure is at most the stake fault bound `F`, then active stake
remains at least `totalStake - F`. This is the stake-weighted analogue of
T-12.

**Theorem 8.5 (Current-validator and evidence admissibility filters).**
*(`restricted_closure_only_from_current_direct_offenders`,
`visible_unreported_graph_in`, `TwoLevelSlashing.v`.)* Filtering direct
offenders and neglect edges to the current validator universe prevents
stale/off-era evidence from seeding the current slash closure. A neglect
edge is valid only when the offender's evidence was visible to the block
creator and not already reported by that block.

**Theorem 8.6 (Graph edge-case invariance).**
*(`slash_iter_graph_equiv`, `no_reachability_no_level2_slash`,
`TwoLevelSlashing.v`.)* Duplicate edges, edge ordering, self-edges, and
cycles do not change the closure except through directed reachability to
a direct offender.

**Theorem 8.7 (Exact arithmetic boundary).**
*(`unsigned_overflow_boundary_exact`, `signed_overflow_boundary_exact`,
`TwoLevelSlashing.v`.)* Rocq's exact natural-number arithmetic reaches
the standard fixed-width boundary at `max + 1`. Implementations using
bounded integer types must use checked arithmetic or prove the projection
from exact arithmetic safe.

---

## 9 · Bisimilarity statement

This is the headline claim of the document.

### 9.1 The bisimulation relation

**Definition 9.1** *(Bisimulation R).*
Let `Rust(S)` and `Scala(S)` be the LTSs induced by the Rust and Scala
slashing implementations respectively. Define
`R ⊆ Rust(S) × Scala(S)` by

```
R = { (sR, sS) | sR.BondMap = sS.BondMap
              ∧ sR.EqRecords ≡ sS.EqRecords                    [mutual containment, modulo iter order]
              ∧ sR.SlashedSet ≡ sS.SlashedSet                  [mutual containment (slashed_bisim)]
              ∧ sR.CoopVaultBalance = sS.CoopVaultBalance
              ∧ sR.ForkChoiceLatestMessages ≡ sS.ForkChoiceLatestMessages   [forkchoice_bisim] }
```

### 9.2 Main theorems

**Theorem 9.1 (T-13, Strong bisimilarity baseline).**
*(`t_13_bm_slash_preserves_bonds_bisim`, `Bisimulation.v:77`.)* The bonds
bisimulation `bonds_bisim b1 b2` is preserved by `bm_slash`, meaning
the `AdmissibleEquivocation → record → slash` happy path keeps the
bond-component of `R` consistent across implementations. Companion
theorems `records_bisim_strong_preserved_update` and
`forkchoice_bisim_preserves_filter` establish the records and fork-choice
components.

**Theorem 9.2 (T-14, Weak barbed bisimulation, full pipeline).**
*(`weak_barbed_equiv` (relation, `Bisimulation.v:367`),
`weak_barbed_equiv_refl` (`Bisimulation.v:376`), and
`weak_barbed_equiv_sym` (`Bisimulation.v:388`), and
`weak_barbed_equiv_trans`.)* Over all observable barbs
`x = {bonds, records, slashedSet, coopVault, forkChoice}`, the
`weak_barbed_equiv` relation is reflexive, symmetric, and transitive.
The per-component preservation theorems combined with
`t_15_pipeline_step_preserves_R` give `Rust ≈ₓ Scala` modulo the
deliberate widening at `neglected_invalid_block` (bug fix #9).

**Theorem 9.3 (T-15, Bisimilarity restoration).**
*(`main_bisimilarity_theorem` and `main_bisimilarity_strong`,
`MainTheorem.v`.)* Under the nine bug fixes specified in §10, the
preservation of all five `R`-components across one slash + record-update
+ filter step is mechanized by `main_bisimilarity_strong`. The
end-to-end pipeline composition is `t_15_pipeline_step_preserves_R`
(also in `MainTheorem.v`). The deliberate Rust-side widening at
`neglected_invalid_block` is captured as fix #9 with its own correctness
proof T-9.9; once that fix is recognized as the *intended* semantics,
no remaining divergence exists.

The Rocq relation also classifies divergence candidates through
`DivergenceClass` in `Bisimulation.v`: `Bisimilar` and
`PermittedBugFix` are allowed, while `CandidateBoundaryDivergence`
requires review and `UnexpectedDivergence` is forbidden. This matches the
Sage differential search: ordinary states must stay bisimilar; the
tracker atomicity fix is a permitted bug-fix divergence; current-validator
boundary differences remain candidate findings until implementation
intent is confirmed.

### 9.3 Why bisimilarity matters

Bisimilarity guarantees that any external observer — including a node
operator querying state, a smart-contract reading on-chain bonds, or a
network peer following fork-choice — cannot distinguish a Rust-port node
from a Scala node by any sequence of observations. Combined with the
bug-fix proofs, this is the audit-grade certification for the migration:
no behavioral regression, eight Scala-inherited defects identified and
fixed, one Rust-introduced regression identified and fixed.

---

## 10 · Bug-fix manifest

Nine numbered bug fixes. Each carries: origin, cause, location, corrected
behavior, theorem name, bisimulation impact, and worked-example /
diagram pointers. The corresponding TLC counter-example, where
applicable, fires under the pre-fix configuration and passes under the
post-fix one.

### 10.0 Bug-class summary

| Bug | Theorem | Origin                                                    | Bisimilarity impact                                                        |
|-----|---------|-----------------------------------------------------------|----------------------------------------------------------------------------|
| #1  | T-9.1   | Scala-inherited                                           | Preserving (both sides converge once fixed)                                |
| #2  | T-9.2   | **Rust-introduced regression** (the only one of the nine) | Preserving (closing Rust-only gap)                                         |
| #3  | T-9.3   | Scala-inherited                                           | Preserving                                                                 |
| #4  | T-9.4   | Scala-inherited                                           | Preserving                                                                 |
| #5  | T-9.5   | Scala-inherited                                           | Preserving                                                                 |
| #6  | T-9.6   | Scala-inherited                                           | Preserving                                                                 |
| #7  | T-9.7   | Scala-inherited                                           | Preserving                                                                 |
| #8  | T-9.8   | Scala-inherited                                           | Preserving                                                                 |
| #9  | T-9.9   | Scala bug, Rust-fixed                                     | **Deliberate widening** (Rust admits self-correcting blocks Scala rejects) |

"Preserving" = the fix restores Rust↔Scala convergence (or, for #2,
fixes a Rust-only deviation). "Deliberate widening" = the fix is a
documented Rust-side improvement that breaks strict bisimilarity *by
design*; T-9.9 establishes that the widening is sound.

### 10.1 Bug #1 — `IgnorableEquivocation` non-slashable (DOS vector)

- **Origin.** Scala-inherited.
- **Cause.** `block_status.rs:36-39` (mirrored at `BlockStatus.scala:62-65`,
  with `IgnorableEquivocation` declared on line 66) carries the explicit
  TODO: *"Make IgnorableEquivocation slashable again ... will become a
  DOS vector if not fixed."* Equivocations that
  arrive unsolicited (not pulled in as a dependency) are silently
  dropped. A Byzantine validator can flood the network with these
  without economic cost.
- **Fix.** Add `IgnorableEquivocation` to `is_slashable()`; in
  `handle_invalid_block`, treat it identically to
  `AdmissibleEquivocation` (record evidence, allow standard slash flow).
- **Theorem.** T-9.1 — `bug_fix_ignorable_safety` in
  `BugFixIgnorable.v`. Proves: under the fix, no honest validator is
  wrongly slashed (since the underlying equivocation predicate is
  unchanged).
- **Statement.** *(`post_fix_ignorable_implies_equivocation`,
  `BugFixIgnorable.v:57`.)*
  ∀ `st v n d`, `detect(st, v, n, d) = DSIgnorable` ⟹
  `is_slashable(IBIgnorableEquivocation) = ⊤` ∧ `equivocates(st, v, n)`.
- **Sketch.** Conjunction of two specializations. The first conjunct is
  by `ignorable_post_fix_slashable` (post-fix `is_slashable` definition).
  The second conjunct is by `detection_sound` (T-1) instantiated at
  `DSIgnorable`: every `DSIgnorable` verdict witnesses a real
  equivocation. Hence honest validators are never slashed under the fix.
  See §9.1 of `slashing-verification.md` for the full proof.
- **Diagram.**

  [![Diagram 03 — Ignorable equivocation slash flow (post-fix #1): unsolicited equivocation is now recorded and slashed, closing the DOS vector](./diagrams/03-seq-ignorable-equivocation-fixed.svg)](./diagrams/03-seq-ignorable-equivocation-fixed.svg)

### 10.2 Bug #2 — Lock-free tracker access (Rust regression)

- **Origin.** Rust-introduced regression (the only one of the nine).
- **Cause.** `multi_parent_casper_impl.rs:1046-1075` reads then writes
  the equivocation tracker without a lock, allowing two threads
  processing `AdmissibleEquivocation` for the same `(validator,
  baseSeqNum)` to both observe `record-absent` and both insert,
  overwriting accumulated `equivocationDetectedBlockHashes` with
  `Set::empty`. (Scala atomic equivalent: `MultiParentCasperImpl.scala:586-603`
  — the Scala happy-path wraps the read, exists-check, and
  `insertEquivocationRecord` write atomically inside
  `accessEquivocationsTracker`.)
- **Fix.** Re-introduce `access_equivocations_tracker { ... }` (matching
  the Scala behavior) which holds a global semaphore around the
  read-modify-write window. The semaphore lives in
  `BlockDagKeyValueStorage.scala:262` (see §3.2.1).
- **Theorem.** T-9.2 — `t_9_2_atomic_no_overwrite` in
  `BugFixAtomicTracker.v`. Proves: under the lock, T-4 (record
  monotonicity) holds for arbitrary thread schedules.
- **Statement.** *(`t_9_2_atomic_no_overwrite`,
  `BugFixAtomicTracker.v:43`; n-thread `t_9_2_atomic_n_threads_arbitrary`,
  `BugFixAtomicTracker.v:130`.)*
  ∀ `s k h`, `incl(hashes_at_key(s, k), hashes_at_key(atomic_record_or_update(s, k, h), k))`.
  Lifted to schedules:
  ∀ `ops s k`, `incl(hashes_at_key(s, k), hashes_at_key(apply_schedule(s, ops), k))`.
- **Sketch.** Single-step: case analysis on `has_key s k`. Present branch
  uses `t_4_record_monotone_update` directly; absent branch composes
  `t_4_record_monotone_insert_cond` with `t_4_record_monotone_update`.
  n-thread: induction on `ops`; the cons case applies
  `t_9_2_atomic_monotone_any_key` and the IH. Under the lock, any
  serializable interleaving of n threads reduces to such a schedule.
  See §9.2 and §9.2′ of `slashing-verification.md` for the full proofs.
- **TLC counter-example.** `MC_ConcurrentTracker.cfg` with `Locked = FALSE`
  produces a violating trace; with `Locked = TRUE` no violation is found.
- **Diagram.**

  [![Diagram 09 — Tracker race and fix: pre-fix two threads overwrite each other's hash; post-fix the lock serializes the read-modify-write](./diagrams/09-seq-tracker-race-and-fix.svg)](./diagrams/09-seq-tracker-race-and-fix.svg)

### 10.3 Bug #3 — Generic slash dispatcher stub

- **Origin.** Scala-inherited. The Scala counterpart at
  `MultiParentCasperImpl.scala:621-622` exhibits the same gap — the
  catch-all `case ib: InvalidBlock if InvalidBlock.isSlashable(ib)` arm
  only invokes `handleInvalidBlockEffect` (mark-invalid + buffer-remove);
  no `EquivocationRecord` is created.
- **Cause.** `multi_parent_casper_impl.rs:1090-1099` carries
  *"TODO: Slash block for status except InvalidUnslashableBlock - OLD"*.
  The 15 non-equivocation slashable variants
  (`JustificationRegression`, `InvalidBondsCache`,
  `NeglectedInvalidBlock`, etc. — equivocation variants
  `AdmissibleEquivocation`, `NeglectedEquivocation`, and post-fix
  `IgnorableEquivocation` are excluded from this count) only get
  marked invalid; no
  `EquivocationRecord` is created and no slash effect runs unless a
  later proposer picks up the offender via
  `prepare_slashing_deploys`.
- **Fix.** Dispatch every `is_slashable() = true` variant through the
  same record-creation path used by `AdmissibleEquivocation`.
- **Theorem.** T-9.3 — `t_9_3_dispatch_complete` in
  `BugFixDispatcher.v`. Proves: under the fix, every slashable invalid
  block triggers a slash within bounded liveness window.
- **Statement.** *(`t_9_3_dispatch_complete`, `BugFixDispatcher.v:41`.)*
  ∀ `ib offender baseSeq s`, `is_slashable(ib) = ⊤` ⟹
  `has_key(dispatch_post_fix(ib, offender, baseSeq, s), (offender, baseSeq)) = ⊤`.
- **Sketch.** Unfold `dispatch_post_fix` to `insert_cond s (mkEqRec offender baseSeq nil)`
  using `is_slashable(ib) = ⊤`. Case-split on
  `has_key s (offender, baseSeq)`: if present, `insert_cond_dup_noop`
  preserves the key; if absent, `find_insert_cond_same_absent` gives the
  inserted record under the same key. In both cases the post-state has
  the key. See §9.3 of `slashing-verification.md` for the full proof.
- **Diagram.**

  [![Diagram 05 — Generic invalid-block dispatch (post-fix #3): detection of non-equivocation slashable variants now creates an EquivocationRecord and enters the standard slash pipeline](./diagrams/05-seq-invalid-block-dispatch-fixed.svg)](./diagrams/05-seq-invalid-block-dispatch-fixed.svg)

### 10.4 Bug #4 — PoS transfer-failure FIXME

- **Origin.** Scala-inherited.
- **Cause.** `casper/src/main/resources/PoS.rhox:469` carries the
  comment *"FIXME handle transfer failing case"*. If
  `posVault!("transfer", coopMultiVaultAddr, valBond, posAuthKey,
  *transferDoneCh)` fails, the `for (_ <- transferDoneCh)` continuation
  never fires and there is no error path back to `returnCh`. The slash
  deploy hangs.
- **Fix.** Add an alternate continuation that listens for an error
  signal on `transferDoneCh` (or a timeout) and writes
  `(false, "transfer failed")` to `returnCh` deterministically.
- **Theorem.** T-9.4 — `t_9_4_transfer_failure_safety` in
  `BugFixTransferFailure.v`. Proves: under the fix, the slash
  transition either succeeds with T-7/T-8 or returns `false` in finite
  time.
- **Statement.** *(`t_9_4_transfer_failure_safety`,
  `BugFixTransferFailure.v:40`.)*
  ∀ `ps v ok`, let `(ps', ok') := slash_with_transfer_oracle(ps, v, ok)` in
  `(ok' = ⊤ ∧ bm_lookup(ps_allBonds(ps'), v) = 0) ∨ (ok' = ⊥ ∧ ps' = ps)`.
- **Sketch.** Case analysis on the `transfer_ok` oracle. If `⊤`, the
  standard `slash` applies and `slash_zeros_bond` (T-7) gives the
  bond-zero conclusion regardless of which branch of `slash` fires. If
  `⊥`, the function returns `(ps, ⊥)` by definition, so `ps' = ps`.
  Either way, the outcome is deterministic in finite time.
  See §9.4 of `slashing-verification.md` for the full proof.
- **Diagram.**

  [![Diagram 07 — PoS.slash() Rholang activity flow with the bug-#4 transfer-failure error path added: deterministic (false, "transfer failed") return on returnCh instead of the pre-fix hang](./diagrams/07-activity-pos-slash-contract.svg)](./diagrams/07-activity-pos-slash-contract.svg)

### 10.5 Bug #5 — Stake-0 silent classification

- **Origin.** Scala-inherited.
- **Cause.** `equivocation_detector.rs:217-220` notes
  *"This case is not necessary if assert(stake > 0) in the PoS contract"*.
  Until that assertion is enforced, a stake-0 bonded validator is
  silently classified `EquivocationDetected` — no slash, no neglected
  check.
- **Fix.** Two valid options:
  - **(a)** Add `assert(stake > 0)` in the PoS `bond` contract to make
    stake-0 bonded validators an unreachable state. Preferred —
    invariant of the system, no runtime branch needed.
  - **(b)** Return `Err(StakeZero)` from the detector and propagate
    upstream. Defensive but adds a runtime branch and complicates the
    detector's error type.

  T-9.5 mechanizes option (a). Option (b) is left as future work and
  is **not** currently mechanized.
- **Bisimulation impact.** Preserving — the silent path is unreachable
  on both sides under option (a); Scala's analogous detector branch
  carries the same TODO and would benefit from the same assertion.
- **Worked example.** §11.6.
- **Theorem.** T-9.5 — `t_9_5_slash_preserves_invariant` in
  `BugFixStakeZero.v`. Proves: under the assertion, the silent
  classification path is unreachable.
- **Statement.** *(`t_9_5_slash_preserves_invariant`,
  `BugFixStakeZero.v:36`; corollary `t_9_5_active_has_positive_bond`,
  line 58.)* Let `active_implies_bonded(ps) ≡ ∀ v ∈ ps_active(ps),
  bm_lookup(ps_allBonds(ps), v) > 0`. Then ∀ `ps v`,
  `active_implies_bonded(ps)` ⟹
  `active_implies_bonded(fst(slash(ps, v)))`.
- **Sketch.** Case analysis on the `slash` branch. Idempotent branch
  (`bm_lookup(ps_allBonds(ps), v) = 0`): state unchanged, invariant
  carried. Active branch: for any `v' ∈ ps_active(ps')` we have
  `v' ≠ v` (by `filter_In`); then `bm_slash_other` gives
  `bm_lookup(bm_slash(b, v), v') = bm_lookup(b, v') > 0` from the
  inherited invariant. See §9.5 of `slashing-verification.md` for the
  full proof.

### 10.6 Bug #6 — Self-regression slips through

- **Origin.** Scala-inherited.
- **Bisimulation impact.** Preserving — both implementations skip the
  block's own sender in `justification_regressions` (line 666 of
  `Validate.scala:649-702`); the fix tightens the predicate on both
  sides identically.
- **Worked example.** §11.9.
- **Cause.** `validate.rs:875-985` (Scala `Validate.scala:649-702`)
  ignores regression of the block's own sender and defers to
  `check_equivocations`. But `check_equivocations` only compares the
  creator-justification *hash*, not the *sequence-number ordering*. A
  sender that ships a non-equivocating but seq-regressed
  self-justification (e.g. due to LMD inconsistency) passes both checks.
- **Fix.** Add an explicit seq-number order check for the block's own
  sender in `justification_regressions`.
- **Theorem.** T-9.6 — `t_9_6_self_regression_detected` (Boolean) and `t_9_6_self_regression_in_dag` (DAG-level) in
  `BugFixSelfRegression.v`. Proves: under the fix, every self-regression
  is caught.
- **Statement.** *(`t_9_6_self_regression_detected`,
  `BugFixSelfRegression.v:52`; DAG-level `t_9_6_self_regression_in_dag`,
  line 79.)* Boolean: ∀ `blk_sn latest cited`, `cited < latest` ⟹
  `has_self_regression(blk_sn, latest, cited) = ⊤`.
  DAG-level: ∀ `blocks sender cited b`,
  `b ∈ blocks ∧ block_sender(b) = sender ∧ block_seq(b) > cited` ⟹
  `has_self_regression(0, ds_latest_seq(blocks, sender), cited) = ⊤`.
- **Sketch.** Boolean: by `Nat.ltb_lt` reflection on the unfolded
  `Nat.ltb cited latest`. DAG-level: from
  `b ∈ blocks ∧ block_sender(b) = sender`, the DAG oracle's
  `ds_latest_seq_lower_bound` gives
  `block_seq(b) ≤ ds_latest_seq(blocks, sender)`; combined with
  `block_seq(b) > cited` we get `cited < ds_latest_seq`, then apply the
  Boolean theorem. See §9.6 and §9.6′ of `slashing-verification.md`
  for the full proofs.

### 10.7 Bug #7 — Off-by-one seq-number density

- **Origin.** Scala-inherited.
- **Bisimulation impact.** Preserving — same `baseSeqNum + 1` density
  assumption on both sides; same BFS-replacement on both sides.
- **Worked example.** §11.7.
- **Cause.** `equivocation_detector.rs:400` (Scala
  `EquivocationDetector.scala:336`) uses `baseSeqNum + 1` to find a
  validator's child block. This assumes per-sender seq numbers are
  *dense* (never skipped). If a validator skips a sequence number (a
  rare but possible edge case under partition recovery), the BFS fails.
- **Fix.** Replace `baseSeqNum + 1` with a BFS over the
  creator-justification chain.
- **Theorem.** T-9.7 — `t_9_7_finds_descendant_with_gap` in
  `BugFixSeqNumDensity.v`. Proves: under the BFS, equivocation
  detection holds even with non-dense seq numbers.
- **Statement.** *(`t_9_7_finds_descendant_with_gap`,
  `BugFixSeqNumDensity.v:84`; subsumption `t_9_7_post_fix_subsumes_pre_fix`,
  line 56.)* ∀ `blocks sender baseSeq b`,
  `b ∈ blocks ∧ block_sender(b) = sender ∧ block_seq(b) > baseSeq` ⟹
  ∃ `b'`, `find_descendant_post_fix(blocks, sender, baseSeq) = Some b'`.
- **Sketch.** By induction on `blocks`. In the recursive case, case-split
  on whether the head matches `block_sender = sender ∧ baseSeq < block_seq`.
  Match: return the head. Mismatch: discharge the witness `b` either by
  contradiction (when the head *is* `b`, the gap test forces success) or
  by descending into the tail with the IH. Replaces the pre-fix
  `Nat.eqb (S baseSeq)` with `Nat.ltb baseSeq`, which strictly admits
  more descendants (subsumption). See §9.7 of `slashing-verification.md`
  for the full proof.

### 10.8 Bug #8 — `prepare_slashing_deploys` doesn't check proposer is bonded

- **Origin.** Scala-inherited. The Scala counterpart at
  `BlockCreator.scala:129-153` (`prepareSlashingDeploys`) also omits
  the proposer-bonded check — it filters `ilm` by *target* validator
  bond (`bondsMap.getOrElse(validator, 0L) > 0L`, line 134) but never
  checks the proposer itself.
- **Cause.** `block_creator.rs:287-332` doesn't verify that the
  *proposer itself* is bonded. An unbonded proposer running the
  proposer thread will still build slash deploys; the `slash` contract
  rejects them at `sysAuthTokenOps!("check", ...)`. This is wasted
  network work.
- **Fix.** Skip `prepare_slashing_deploys` entirely when
  `bonds_map[proposer] = 0`.
- **Theorem.** T-9.8 — `t_9_8_unbonded_proposer_no_slash` in
  `BugFixUnbondedProposer.v`. Proves: under the fix, no useless slash
  deploys are emitted.
- **Statement.** *(`t_9_8_unbonded_proposer_no_slash`,
  `BugFixUnbondedProposer.v:44`; equivalence
  `t_9_8_post_fix_equivalent_when_bonded`, line 55.)*
  ∀ `ilm bonds proposer seqNum seed_fn`, `bm_lookup(bonds, proposer) = 0` ⟹
  `prepare_slashing_deploys_post_fix(ilm, bonds, proposer, seqNum, seed_fn) = []`.
  When `bm_lookup(bonds, proposer) > 0`, post-fix is pointwise equal to
  pre-fix.
- **Sketch.** Direct unfolding: the guard `Nat.eqb (bm_lookup bonds proposer) 0`
  reflects to `⊤` under the hypothesis, so the function returns `[]`
  before consulting `prepare_slashing_deploys`. The bonded-equivalence
  companion discharges the symmetric branch by `Nat.eqb` reflecting to
  `⊥`, yielding pointwise equality with the pre-fix function — so the
  fix is conservative on bonded proposers. See §9.8 of
  `slashing-verification.md` for the full proof.
- **Bisimulation impact.** Preserving — both implementations omit the
  proposer-bonded check pre-fix; the post-fix short-circuit is
  pointwise equal to the pre-fix function on bonded proposers, so
  Scala adopting the same fix would make the two converge.
- **Worked example.** §11.10.

### 10.9 Bug #9 — Scala rejects self-correcting blocks (Scala bug, Rust-fixed)

- **Origin.** Scala bug; Rust-fixed by deliberate widening.
- **Cause.** Scala `Validate.scala:727-731` rejects a block whenever
  `neglectedInvalidJustification = true`, even if the block itself
  carries a `Slash` system deploy targeting the offender. Rust's
  `validate.rs:1016-1029` adds an extra branch
  `if neglectedInvalidJustification ∧ ¬has_slash_system_deploys` that
  *admits* self-correcting blocks. This is a deliberate widening; the
  Scala behavior is a bug.
- **Fix.** Adopt the Rust behavior: a block that includes a
  `SlashDeploy` against the neglected justification's validator is
  valid.
- **Theorem.** T-9.9 — `t_9_9_post_fix_rejection_iff` in
  `BugFixSelfRegression.v`. Proves: the Rust widening is sound (no
  invalid block is admitted that lacks a corresponding slash) and
  improves liveness (self-correcting blocks no longer require a third-
  party slash).
- **Statement.** *(`t_9_9_post_fix_rejection_iff`,
  `BugFixSelfRegression.v:107`; admission corollary
  `t_9_9_post_fix_admits_more`, line 121.)* ∀ `hn hs`,
  `rejects_neglected_post_fix(hn, hs) = ⊤` ⟺ `hn = ⊤ ∧ hs = ⊥`.
  Corollary: `hn = ⊤ ∧ hs = ⊤` ⟹
  `rejects_neglected_pre_fix(hn) = ⊤ ∧ rejects_neglected_post_fix(hn, hs) = ⊥`
  (post-fix strictly admits more blocks than the Scala pre-fix).
- **Sketch.** Bi-implication: unfold `rejects_neglected_post_fix` to
  `andb hn (negb hs)`; rewrite by `andb_true_iff` and `negb_true_iff`.
  Admission corollary: substitute `hn := ⊤, hs := ⊤` into the unfolded
  forms — pre-fix returns `⊤` (rejects), post-fix returns
  `andb ⊤ (negb ⊤) = ⊥` (admits). The fix is sound (rejection still
  fires whenever no slash accompanies a neglected justification) and
  strictly more live. See §9.9 of `slashing-verification.md` for the
  full proof.

---

## 11 · Worked examples

Each example tags a use case from §12 with a concrete trace.

### 11.1 Example: a single AdmissibleEquivocation

Setup: 3 validators `{A, B, C}`, each bonded with stake 100. `A`
equivocates by signing two distinct blocks `b1, b1'` at seq 5. `B`
proposes a block at seq 6 with both `b1` and `b1'` in its justifications.

Trace (showing only the slashing-relevant transitions):

```
1. sign(A, 5, b1)     ⟶  D += b1
2. sign(A, 5, b1')    ⟶  D += b1'
3. sign(B, 6, b2)     ⟶  D += b2  ; b2 cites b1 and b1' in justifications
4. requestedAsDep(b1) ⟶ true
5. detect(b1) = AdmissibleEquivocation
6. record(A, 4)       ⟶ E += (A, 4, ∅)
7. propose(C, [SlashDeploy(b1', A, ...)])
8. executeSlash(A, true)
   ⟶ allBonds[A] := 0
   ⟶ activeValidators := {B, C}
   ⟶ coopVaultBalance := 100
9. filterFC(A)        ⟶ FC ignores A henceforth
```

**Final state.** A is slashed; bond moved to Coop vault; B and C continue
as the active set.

### 11.2 Example: Two-level slashing (collusion)

Setup: 4 validators, A equivocates, B colludes by citing A's
equivocation in B's next block without attaching a SlashDeploy.

Trace:

```
1-6: same as 11.1 (A is detected and recorded)
7.  sign(B, 7, bB)   ⟶  D += bB ; bB cites b1 (the invalid block)
                          ; bB carries no SlashDeploy
8.  detect(bB)       = NeglectedEquivocation  ; (B is recorded too)
9.  record(B, 6)
10. propose(C, [SlashDeploy(b1', A, ...), SlashDeploy(bB, B, ...)])
11. executeSlash(A, true)
12. executeSlash(B, true)
    ⟶ allBonds[A] = 0, allBonds[B] = 0
    ⟶ activeValidators = {C, D}
    ⟶ coopVaultBalance = 200
```

**This trace exits T-12's precondition.** With n = 4, the BFT bound is
`F = ⌊(n−1)/3⌋ = 1`. After both A and B are slashed,
`|closure| = |{A, B}| = 2 > F = 1`, so T-12's hypothesis
`|closure| ≤ F` does not hold and the quorum-preservation conclusion
does not apply. The remaining active set `{C, D}` is below the
quorum lower bound of `n − F = 3`. This is a **counter-example to
naive expectations** — it shows what happens *outside* T-12's domain,
not within it. The formal treatment (the F-neglectful quorum-
liquidation finding) is in
[`slashing-verification.md` §10.8.2](./slashing-verification.md#1082-two-level-slashing-can-liquidate-quorum-if-the-network-is-more-than-f-neglectful).
§13 documents the protocol-level precondition `|equivocators| ≤ ⌊(n−1)/3⌋`
under which T-12's guarantee actually fires; this example deliberately
violates it to exhibit the boundary behavior.

### 11.3 Example: Lock-free tracker race (bug #2 demo)

Setup: two threads `T1` and `T2` simultaneously process two distinct
equivocating blocks `b1, b2` by validator `A` at the same `(seq, base)`.

Pre-fix trace (TLC counter-example from `MC_ConcurrentTracker.cfg` with
`Locked = FALSE`):

```
T1: equivocation_records()              ⟶ view1 = ∅ (no record at (A, sn-1))
T2: equivocation_records()              ⟶ view2 = ∅
T1: insert_record(A, sn-1, {b1.hash})   ⟶ store := {(A, sn-1) ↦ {b1.hash}}
T2: insert_record(A, sn-1, {b2.hash})   ⟶ store := {(A, sn-1) ↦ {b2.hash}}
                                              ↑↑↑ overwrite — b1.hash lost
```

Post-fix: the lock around the RMW serializes T1 before T2; T2's
`equivocation_records()` returns `view2 = {(A, sn-1) ↦ {b1.hash}}`; the
`update_record` call appends `b2.hash` instead of overwriting.

### 11.4 Example: PoS transfer failure (bug #4 demo)

Setup: A is detected and recorded; B proposes a SlashDeploy; the
`@posVault!("transfer", …)` call fails (e.g. vault deploy quota
exhausted).

Pre-fix trace: the `for (_ ← transferDoneCh)` continuation never fires;
A remains in `SlashPending` indefinitely; the next proposer tries again
on B's next block; same failure; etc. The validator is effectively
quarantined but not slashed, with no closure.

Post-fix trace: the alternate continuation fires after a deterministic
timeout, returning `(false, "transfer failed")` on `returnCh`. A
transitions back to `EquivocatorRecorded`. The next proposer can retry
the slash; or, if the failure is persistent (a misconfigured vault
contract), an operator alert fires.

### 11.5 Example: Self-correcting block (bug #9 / Rust widening)

Setup: A equivocates and is recorded. B proposes a block at seq 7 that
(i) cites A's invalid block in justifications, AND (ii) carries a
`SlashDeploy` targeting A.

Pre-fix Scala behavior: B's block is rejected with
`NeglectedInvalidBlock`. Now C must propose another block to slash A,
delaying enforcement.

Post-fix Rust behavior: B's block is admitted (the slash deploy
self-corrects the neglect). A is slashed in B's own block. Liveness is
strictly better.

### 11.6 Example: Stake-0 bonded validator (bug #5 demo)

Setup: A's bond is decremented to 0 by some non-slash mechanism (e.g. a
bond withdrawal). A then equivocates.

Pre-fix: detector reaches the `if stake ≤ 0 then EquivocationDetected`
branch in `equivocation_detector.rs:217`; A is "detected" but never
slashed (zero stake to forfeit) and never recorded. A's equivocation
is invisible to two-level closure.

Post-fix: option (a) — the PoS bond contract enforces `stake > 0` as an
invariant, so the bonded-with-zero state is unreachable. Option (b) —
the detector returns an explicit `Err(StakeZero)` which the orchestrator
logs and skips slashing.

### 11.7 Example: Skipped sequence number (bug #7 demo)

Setup: A produces blocks at seq 5, 7, 8 (skips seq 6 due to a partition
recovery). Then A equivocates at seq 9.

Pre-fix: the detector's BFS uses `baseSeqNum + 1 = 8`; finds A's block
at seq 8 OK; expects to find A's seq 9 block by following the
creator-justification, but the chain has a gap. Detection fails.

Post-fix: BFS over the full creator-justification chain (rather than
single-step `+1`) handles the gap; detection succeeds.

### 11.8 Example: JustificationRegression dispatched (bug #3 demo)

Setup: 3 validators `{A, B, C}`, each bonded with stake 100. Validator
`V*` (one of A/B/C, say A) signs a block `bX` at seq 5 whose
creator-justification points back to a strictly older sequence number
than A's known latest message (a third-party-detected justification
regression — distinct from the *self*-regression of bug #6).

Trace (showing only the slashing-relevant transitions):

```
1. sign(A, 5, bX)             ⟶ D += bX (regression)
2. validate(bX) = JustificationRegression
3. is_slashable(JustificationRegression) = TRUE

   Pre-fix dispatcher (multi_parent_casper_impl.rs:1090-1099):
4. handle_invalid_block_effect(bX, invalid = true)
   ⟶ DAG marks bX invalid; NO EquivocationRecord;
      A continues with bond intact unless a future proposer
      happens to surface A's invalid latest message.

   Post-fix #3 dispatcher:
4'. insert_equivocation_record(A, 4, ∅)
5'. update_equivocation_record(A, 4, bX.hash)
6'. propose(B, [SlashDeploy(bX, A, ...)])
7'. executeSlash(A, true)
    ⟶ allBonds[A] := 0
    ⟶ activeValidators := {B, C}
    ⟶ coopVaultBalance := 100
```

**Final state.** Pre-fix: A unpunished. Post-fix: A is slashed in B's
next block, mirroring the AdmissibleEquivocation flow. This example
exercises the dispatcher uniformity claim of T-9.3
(`t_9_3_dispatch_complete`). See Diagram 05 for the sequence diagram.

The same trace generalizes to every other `is_slashable() = TRUE`
variant (`InvalidBondsCache`, `ContainsExpiredDeploy`,
`ContainsTimeExpiredDeploy`, `InvalidBlockNumber`, etc.) — each
populates an EquivocationRecord under the post-fix dispatcher.

### 11.9 Example: Self-regression slips through (bug #6 demo)

Setup: 3 validators `{A, B, C}`. Validator A signs a block `bN` at
seq 7. A then signs a block `bM` at seq 9 whose
creator-justification cites A's *own* prior block at seq 5 (i.e.
`m = 5 < 7 = n`). bM is *not* an equivocation of bN — A only signed
one block at seq 9 — but bM's chain regresses A's own line.

Trace:

```
1. sign(A, 7, bN)                  ⟶ D += bN
2. sign(A, 9, bM)                  ⟶ D += bM ; bM cites A's seq-5 block
                                       (skipping bN in A's chain)
3. validate(bM):
     justification_regressions(bM, snapshot)
       — pre-fix: filterNot(_._1 == bM.sender) skips A's own
         justification (Validate.scala:666); only checks others'
         regressions. Returns FALSE.
     check_equivocations(bM): only one bM at seq 9. Returns FALSE.
     ⟶ bM admitted as Valid.

   Post-fix #6:
3'. justification_regressions(bM, snapshot)
       — fix drops the filterNot: A's own creator-justification
         compared against ds_latest_seq(blocks, A) = 7.
         Cited seq 5 < latest seq 7 ⟹ self-regression detected.
     ⟶ JustificationRegression
4'. dispatcher (post-fix #3) creates EquivocationRecord(A, 8, {bM.hash})
5'. propose(B, [SlashDeploy(bM, A, ...)])
6'. executeSlash(A, true)
    ⟶ allBonds[A] := 0
```

**Final state.** Pre-fix: A's chain inconsistency goes unnoticed —
LMD violations can accumulate. Post-fix: A is detected and slashed.
This example exercises T-9.6 (`t_9_6_self_regression_in_dag`). The
post-fix slashing of A also depends on bug #3's dispatcher fix
(otherwise the JustificationRegression verdict would not produce a
record). The example chain therefore illustrates fixes #6 and #3
acting in concert.

### 11.10 Example: Unbonded proposer no-emit (bug #8 demo)

Setup: 4 validators `{A, B, C, D}`. A was previously slashed and is
no longer in the active set; A's bond = 0. The proposer-thread
scheduler nevertheless picks A as the next proposer (a corner case
that can occur during the next-epoch transition before the
active-set update propagates).

Trace:

```
1. propose_thread_scheduler picks A as proposer
2. A.prepare_slashing_deploys(seqM):

   Pre-fix (block_creator.rs:287-332):
3. ilm                  ← dag.invalid_latest_messages ; { (V, bV) }
4. ilm_from_bonded      ← filter (v, _) ∈ ilm where bonds_map[v] > 0
                          ⟶ V kept (V still bonded)
5. slashing_deploys     ← map ilm_from_bonded → SlashDeploy(...)
                          ⟶ Vec of length 1
6. A signs and emits a block bA carrying SlashDeploy(bV, V).
7. Other validators replay bA: at the slash deploy,
   sysAuthTokenOps!("check", sysAuthToken, *isValidTokenCh)
   reports !isValid because A's auth token does not match the
   system token (PoS.rhox:437-439).
8. PoS contract returns (false, "Invalid system auth token") on
   returnCh (PoS.rhox:491).
9. bA is rejected as a malformed system-deploy result.
   A's CPU and gossip bandwidth wasted.

   Post-fix #8:
3'. Guard: if bonds_map[A] = 0 then return Vec::new() ; halt early
4'. A emits no slash-deploys; bA carries no system_deploys
5'. Other validators replay bA cleanly; bA is admitted (or proposer
    rotates and B handles V's slash).
```

**Final state.** Pre-fix: A's block bA is rejected; the slash of V is
delayed by one round; A's proposer-slot wasted. Post-fix: A
short-circuits to no-emit; bA is admitted (modulo unrelated content);
the slash of V proceeds on a subsequent bonded proposer's block.
This example exercises T-9.8 (`t_9_8_unbonded_proposer_no_slash`)
and the bonded-equivalence companion
`t_9_8_post_fix_equivalent_when_bonded`.

---

## 12 · Use-case catalog

Fifty-four scenarios. Each row gives: name, theorem(s) exercised, related
diagram, and a stub path for the automated test (the test files are not
implemented in this work; they are the deliverable for issue #25 fix 3d).

The catalog is organised in four blocks: **Core scenarios (UC-01–UC-25)**
— the original 25 scenarios; **Tier A audit blockers (UC-26, UC-27,
UC-37, UC-38, UC-39, UC-41, UC-42, UC-43)** — closures of the §10.8
verification-doc findings,
unmapped headline theorems, and high-priority pre-fix regressions;
**Tier B variant catalog completion (UC-28–UC-36)** — one entry per
remaining slashable `InvalidBlock` variant; **Tier C operational and
adversarial (UC-40, UC-44–UC-54)** — distributed-systems and lifecycle
scenarios. UC numbering reflects the order in which each scenario was
proposed; tiers do not partition the numeric range.

### Core scenarios (UC-01–UC-25)

**Outcome legend.** Each row's *Outcome* column states what the test
should assert as the steady state: `slashed` (offender's bond zeroed,
removed from active set), `not-slashed` (offender bond unchanged;
covers stake-0 invariant and unbonded-proposer no-emit), `rejected`
(block or deploy refused), `admitted` (block accepted modulo prior
slashing), `error` (deterministic failure return path), `behavioral`
(no formal theorem; assertion checks invariant or trace property).

| #     | Scenario                                            | Theorems            | Outcome     | Diagram | Test stub                                           |
|-------|-----------------------------------------------------|---------------------|-------------|---------|-----------------------------------------------------|
| UC-01 | Single AdmissibleEquivocation                       | T-1, T-2, T-7, T-10 | slashed     | 02      | `casper/tests/slashing/admissible_single.rs`        |
| UC-02 | f validators equivocate in same epoch               | T-1, T-12           | slashed     | 02      | `casper/tests/slashing/admissible_multi.rs`         |
| UC-03 | IgnorableEquivocation under fix #1                  | T-9.1               | slashed     | 03      | `casper/tests/slashing/ignorable_fixed.rs`          |
| UC-04 | NeglectedEquivocation triggers Level 2              | T-11, T-12          | slashed×2   | 04      | `casper/tests/slashing/two_level.rs`                |
| UC-05 | JustificationRegression (third party)               | T-9.3               | slashed     | 05      | `casper/tests/slashing/justification_regression.rs` |
| UC-06 | JustificationRegression (self) — Boolean predicate¹ | T-9.6 (Boolean)     | slashed     | 08      | `casper/tests/slashing/self_regression.rs`          |
| UC-07 | InvalidBondsCache                                   | T-9.3               | slashed     | 05      | `casper/tests/slashing/invalid_bonds_cache.rs`      |
| UC-08 | ContainsExpiredDeploy                               | T-9.3               | slashed     | 05      | `casper/tests/slashing/expired_deploy.rs`           |
| UC-09 | ContainsTimeExpiredDeploy                           | T-9.3               | slashed     | 05      | `casper/tests/slashing/time_expired_deploy.rs`      |
| UC-10 | InvalidBlockNumber                                  | T-9.3               | slashed     | 05      | `casper/tests/slashing/invalid_block_number.rs`     |
| UC-11 | Stake-0 bonded validator under fix #5               | T-9.5               | not-slashed | 06      | `casper/tests/slashing/stake_zero.rs`               |
| UC-12 | Concurrent equivocation insertions                  | T-9.2               | slashed     | 09      | `casper/tests/slashing/tracker_race.rs`             |
| UC-13 | PoS transfer failure mid-slash                      | T-9.4               | error       | 07      | `casper/tests/slashing/transfer_failure.rs`         |
| UC-14 | Detector crashes between detect and record²         | T-9.2               | slashed     | 09      | `casper/tests/slashing/detect_crash.rs`             |
| UC-15 | Proposer crashes after detection³                   | behavioral          | behavioral  | 02      | `casper/tests/slashing/proposer_crash.rs`           |
| UC-16 | Multi-parent block with slashed parent              | T-10                | admitted    | 06      | `casper/tests/slashing/slashed_parent.rs`           |
| UC-17 | Fork choice with mixed slashed/active               | T-10                | admitted    | 06      | `casper/tests/slashing/forkchoice_mixed.rs`         |
| UC-18 | Bonded-proposer slash-deploy emission⁴              | T-9.8               | slashed     | 01      | `casper/tests/slashing/replay_determinism.rs`       |
| UC-19 | Two-level where neglecter is bond-zero              | T-11, T-9.5         | slashed     | 04      | `casper/tests/slashing/two_level_bond_zero.rs`      |
| UC-20 | Skipped seq number across equivocation              | T-9.7               | slashed     | 02      | `casper/tests/slashing/seqnum_gap.rs`               |
| UC-21 | Auth-token spoofing on slash deploy                 | T-AuthCheck         | rejected    | 07      | `casper/tests/slashing/auth_token_spoof.rs`         |
| UC-22 | Unbonded proposer slashes                           | T-9.8               | not-slashed | 02      | `casper/tests/slashing/unbonded_proposer.rs`        |
| UC-23 | Self-correcting block (Rust widening)               | T-9.9               | admitted    | 08      | `casper/tests/slashing/self_correcting.rs`          |
| UC-24 | Slash idempotence                                   | T-Idem              | slashed     | 02      | `casper/tests/slashing/idempotence.rs`              |
| UC-25 | Coop vault balance accounting                       | T-8                 | slashed     | 02      | `casper/tests/slashing/vault_accounting.rs`         |

Footnotes:
1. **UC-06** covers only the Boolean-predicate level of T-9.6
   (`t_9_6_self_regression_detected`). The DAG-level companion proof
   `t_9_6_self_regression_in_dag` is exercised by UC-37.
2. **UC-14** models a "crash" as a 1-thread schedule with suspended
   write — formally an instance of `t_9_2_atomic_n_threads_arbitrary`
   over a length-1 schedule that does not commit.
3. **UC-15** is *behavioral* — no formal theorem yet covers
   proposer-crash recovery. See §13 row "Proposer-crash recovery"
   for the scope-boundary record.
4. **UC-18** exercises only the bonded-proposer emission branch of
   T-9.8 (`t_9_8_unbonded_proposer_no_slash`'s positive companion
   `t_9_8_post_fix_equivalent_when_bonded`). Full replay determinism
   is out of scope per §13.

### Tier A — Audit blockers (UC-26, UC-27, UC-37, UC-38, UC-39, UC-41, UC-42, UC-43)

These close the §10.8 verification-doc findings, unmapped headline
theorems, and high-priority pre-fix regressions identified in the
catalog audit.

| #     | Scenario                                       | Theorems          | Outcome     | Diagram | Test stub                                            |
|-------|------------------------------------------------|-------------------|-------------|---------|------------------------------------------------------|
| UC-26 | F-neglectful active-set drop (§10.8.2 closure) | T-12, T-11        | behavioral  | 04      | `casper/tests/slashing/f_neglectful_quorum_drop.rs`  |
| UC-27 | NeglectedInvalidBlock dispatch                 | T-3, T-6, T-9.3   | slashed     | 05      | `casper/tests/slashing/neglected_invalid_block.rs`   |
| UC-37 | Self-regression DAG-level (block-witness)      | T-9.6 (DAG-level) | slashed     | 08      | `casper/tests/slashing/self_regression_dag_level.rs` |
| UC-38 | `detect_neglected` soundness/completeness      | T-6               | behavioral  | 04      | `casper/tests/slashing/neglected_detection.rs`       |
| UC-39 | Bisimilarity audit (R-relation invariant)      | T-13, T-14, T-15  | behavioral  | 10      | `casper/tests/slashing/bisim_audit.rs`               |
| UC-41 | Pre-fix ignorable DOS regression               | T-9.1 (negative)  | not-slashed | 03      | `casper/tests/slashing/ignorable_pre_fix_dos.rs`     |
| UC-42 | Pre-fix dispatcher stub regression             | T-9.3 (negative)  | not-slashed | 05      | `casper/tests/slashing/dispatcher_pre_fix_drop.rs`   |
| UC-43 | Pre-fix off-by-one seq density regression      | T-9.7 (negative)  | not-slashed | 02      | `casper/tests/slashing/seqnum_pre_fix_miss.rs`       |

### Tier B — Slashable-variant catalog completion (UC-28–UC-36)

One scenario per remaining slashable `InvalidBlock` variant from §2.4.
Brings slashable-variant coverage to 18/18 (100%).

| #     | Scenario                                  | Theorems     | Outcome | Diagram | Test stub                                          |
|-------|-------------------------------------------|--------------|---------|---------|----------------------------------------------------|
| UC-28 | InvalidParents (parent doesn't match LMD) | T-9.3        | slashed | 05      | `casper/tests/slashing/invalid_parents.rs`         |
| UC-29 | InvalidFollows                            | T-9.3        | slashed | 05      | `casper/tests/slashing/invalid_follows.rs`         |
| UC-30 | InvalidSequenceNumber                     | T-9.3, T-9.7 | slashed | 05      | `casper/tests/slashing/invalid_sequence_number.rs` |
| UC-31 | InvalidShardId                            | T-9.3        | slashed | 05      | `casper/tests/slashing/invalid_shard_id.rs`        |
| UC-32 | InvalidRepeatDeploy                       | T-9.3        | slashed | 05      | `casper/tests/slashing/invalid_repeat_deploy.rs`   |
| UC-33 | DeployNotSigned                           | T-9.3        | slashed | 05      | `casper/tests/slashing/deploy_not_signed.rs`       |
| UC-34 | InvalidTransaction                        | T-9.3        | slashed | 05      | `casper/tests/slashing/invalid_transaction.rs`     |
| UC-35 | InvalidBlockHash                          | T-9.3        | slashed | 05      | `casper/tests/slashing/invalid_block_hash.rs`      |
| UC-36 | ContainsFutureDeploy                      | T-9.3        | slashed | 05      | `casper/tests/slashing/future_deploy.rs`           |

### Tier C — Operational and adversarial (UC-40, UC-44–UC-54)

Distributed-systems classics, lifecycle transitions during a pending
slash, DAG-shape variations, and record-invariant exercises.

| #     | Scenario                                                  | Theorems         | Outcome    | Diagram | Test stub                                                |
|-------|-----------------------------------------------------------|------------------|------------|---------|----------------------------------------------------------|
| UC-40 | Coop vault accounting under failed transfer               | T-8, T-9.4       | error      | 07      | `casper/tests/slashing/vault_accounting_failure.rs`      |
| UC-44 | Multi-validator concurrent equivocation                   | T-1, T-9.2, T-12 | slashed×n  | 09      | `casper/tests/slashing/multi_validator_concurrent_eq.rs` |
| UC-45 | Replay attack on slash deploy                             | T-Idem, T-9.8    | rejected   | 07      | `casper/tests/slashing/slash_replay_attack.rs`           |
| UC-46 | Network partition then merge with both-side equivocations | T-1, T-9.2, T-15 | slashed    | 02, 09  | `casper/tests/slashing/partition_merge_eq.rs`            |
| UC-47 | Validator joins active set during pending slash           | T-Idem, T-10     | slashed    | 06      | `casper/tests/slashing/validator_join_during_slash.rs`   |
| UC-48 | Validator leaves active set during pending slash          | T-Idem, T-10     | slashed    | 06      | `casper/tests/slashing/validator_leave_during_slash.rs`  |
| UC-49 | Genesis-time slash (genesis block invalid sender)         | T-9.3            | slashed    | 06      | `casper/tests/slashing/genesis_slash.rs`                 |
| UC-50 | Slash-deploy execution order in same block                | T-Idem, T-11     | slashed×k  | 02      | `casper/tests/slashing/multi_slash_in_one_block.rs`      |
| UC-51 | Deep DAG (>100-block chain) AdmissibleEquivocation        | T-1, T-15        | slashed    | 02      | `casper/tests/slashing/deep_dag_admissible.rs`           |
| UC-52 | Wide DAG (high parent-fanout) AdmissibleEquivocation      | T-1, T-15        | slashed    | 02      | `casper/tests/slashing/wide_dag_admissible.rs`           |
| UC-53 | Single-chain (no forks) Equivocation                      | T-1, T-9.6       | slashed    | 02      | `casper/tests/slashing/single_chain_eq.rs`               |
| UC-54 | Record monotonicity + uniqueness invariants               | T-4, T-5         | behavioral | 09      | `casper/tests/slashing/record_invariants.rs`             |
| UC-55 | Weighted neglect-chain stake amplification                | T-12 weighted    | behavioral | 04      | `casper/tests/slashing/weighted_neglect_chain.rs`        |
| UC-56 | Zero-stake direct offender cannot seed closure            | T-9.5, T-12 weighted | not-slashed | 06  | `casper/tests/slashing/zero_stake_direct_offender.rs`    |
| UC-57 | Stale/off-era evidence filtered from current closure      | T-12 filter      | behavioral | 04      | `casper/tests/slashing/stale_evidence_filtered.rs`       |
| UC-58 | Evidence withheld from validator view                     | T-12 visibility  | behavioral | 04      | `casper/tests/slashing/evidence_visibility_gap.rs`       |
| UC-59 | Duplicate neglect edges are idempotent                    | T-12 graph equiv | behavioral | 04      | `casper/tests/slashing/duplicate_neglect_edges.rs`       |
| UC-60 | Neglect cycle without path to offender is not slashed     | T-12 reachability | not-slashed | 04     | `casper/tests/slashing/disconnected_neglect_cycle.rs`    |
| UC-61 | Bounded arithmetic projection around slash accounting     | T-8, T-12 arithmetic | error   | 07      | `casper/tests/slashing/bounded_arithmetic_projection.rs` |

Each test stub follows the pattern:

```rust
#[test]
fn uc_01_admissible_single() {
    // Setup: 3 validators bonded with stake 100, fresh DAG.
    let mut harness = SlashingTestHarness::new(3, 100);

    // Step 1: validator A signs two distinct blocks at seq 5.
    let b1  = harness.sign_block("A", 5);
    let b1p = harness.sign_block_distinct("A", 5);

    // Step 2: validator B proposes at seq 6 citing both.
    let b2 = harness.propose("B", &[b1, b1p], &[]);

    // Step 3: detection fires.
    assert_eq!(harness.detect(b1), Status::AdmissibleEquivocation);

    // Step 4: record exists.
    assert!(harness.has_record("A", 4));

    // Step 5: validator C's next block carries SlashDeploy(b1p, A).
    let b3 = harness.propose("C", &[b2], &[SlashDeploy::for("A", b1p)]);

    // Step 6: A is slashed.
    assert_eq!(harness.bond("A"), 0);
    assert_eq!(harness.coop_vault(), 100);
    assert!(!harness.is_active("A"));
}
```

Implementing the harness and 54 tests is out-of-scope for this work (see
§13); the stubs are normative for whoever implements them.

---

## 13 · Scope boundaries

Following the cost-accounting precedent's §12.3, this section preempts
likely objections by stating what is in and out of scope.

| Topic                                                        | Status            | Rationale                                                                                                                                                                                                                    |
|--------------------------------------------------------------|-------------------|------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| Implementing the bug fixes in `casper/src/rust/`             | Out               | Each fix becomes its own PR, cross-referenced from this spec. The spec is normative; the fixes align code with the spec via a separate PR cycle.                                                                             |
| Rewriting `test_slash.py` (issue #25 fix 3d)                 | Out               | System-integration concern (`F1R3FLY-io/system-integration#51`); separate repo.                                                                                                                                              |
| Replacing PoS multi-sig keys (issue #25 fix 3e)              | Out               | Operations / key-management concern; not a code change in `casper/`.                                                                                                                                                         |
| Graduated/proportional slashing penalties (issue #25 fix 3c) | Out               | Requires protocol-level design decision; this spec covers the existing one-strike model. Future work mentioned at the end of §10.                                                                                            |
| End-to-end shard reproduction of equivocation                | Out               | Requires running validators; the spec describes the use cases; the automated tests are stubs only.                                                                                                                           |
| Replay protocol around slashed deploys                       | Partial           | Mentioned where adjacent (bug fix #8) but not formalized.                                                                                                                                                                    |
| Cordial Miners / RGB PSSM / Casanova consensus paths         | Out               | This spec covers Casper CBC only; the project supports four consensus mechanisms but only one has slashing today.                                                                                                            |
| Validator authentication (PKI, key rotation)                 | Out               | Adjacent topic; the slashing layer assumes authenticated identities and bisimilarity is conditional on this.                                                                                                                 |
| Bond-deposit / bond-withdrawal protocol                      | Out               | The spec assumes bonds exist; mutation paths other than slashing are out of scope.                                                                                                                                           |
| Z3 / SMT-based bond arithmetic verification                  | Out               | TLA+ + Rocq sufficient; Z3 may be added if a specific bond invariant proves intractable but no such instance was identified.                                                                                                 |
| Economic finality                                            | Out               | Covered separately under `docs/theory/finality/`; the slashing layer enforces accountability, not finality.                                                                                                                  |
| Liveness under partition                                     | Out               | TLA+ liveness was model-checked but has scale caveats — see verification §10.5 / §10.8.3.                                                                                                                                    |
| Gossip-layer Sybil resistance                                | Out               | Adjacent topic; handled in `comm/` (Kademlia + TLS layers).                                                                                                                                                                  |
| Proposer-crash recovery                                      | Out               | Behavioral concern (UC-15); no formal theorem yet covers the proposer-crash → next-proposer-takeover transition. Future work.                                                                                                |
| Rocq mechanization of `T-AuthCheck`                          | Out (future work) | Currently a Rholang-level observation only (§5.5). The Rocq `slash` definition assumes the auth-token check has already passed; extending it with an auth-oracle (mirroring `BugFixTransferFailure.v`) would close this gap. |

The bisimilarity claim (T-15) is **modulo**:
- α-equivalence on Rholang names (a standard equivalence on
  rho-calculus terms, justified in [MR05a]).
- Iteration order on `BTreeSet` (Rust) vs `Set` (Scala) — value-level
  equality, not byte-level on-disk equality.
- Seven Scala-inherited bug-fix deltas (T-9.1, T-9.3–T-9.8) and one
  Rust-introduced regression fix (T-9.2, the only one of the nine), all of
  which restore Rust↔Scala convergence; **and** the deliberate
  Rust-side widening at bug #9 (T-9.9) which admits self-correcting
  blocks Scala rejects. (See §10.0 for the per-bug origin
  classification.)
- An authenticated PKI identity layer (out of scope; T-15 holds
  modulo this assumption).

---

## 14 · References

[BG19]
    V. Buterin and V. Griffith.
    *Casper the Friendly Finality Gadget*.
    arXiv:1710.09437, 2019.
    [doi:10.48550/arXiv.1710.09437](https://doi.org/10.48550/arXiv.1710.09437)

[BHKPQRSWZ20]
    V. Buterin, D. Hernandez, T. Kamphefner, K. Pham, Z. Qiao, D. Ryan,
    J. Sin, Y. Wang, Y. X. Zhang.
    *Combining GHOST and Casper*.
    arXiv:2003.03052, 2020.
    [doi:10.48550/arXiv.2003.03052](https://doi.org/10.48550/arXiv.2003.03052)

[Z16]
    V. Zamfir.
    *The History of Casper* (Parts 1–5).
    Medium, 2016.
    [https://medium.com/@Vlad_Zamfir/the-history-of-casper-part-1-59233819c9a9](https://medium.com/@Vlad_Zamfir/the-history-of-casper-part-1-59233819c9a9)

[CBCCoq20]
    *Formalizing Correct-by-Construction Casper in Coq*.
    IEEE Xplore document 9169468, 2020.
    [https://ieeexplore.ieee.org/document/9169468/](https://ieeexplore.ieee.org/document/9169468/)

[BKM18]
    E. Buchman, J. Kwon, Z. Milosevic.
    *The latest gossip on BFT consensus*.
    arXiv:1807.04938, 2018.
    [doi:10.48550/arXiv.1807.04938](https://doi.org/10.48550/arXiv.1807.04938)

[ABPT19]
    Y. Amoussou-Guenou, A. Del Pozzo, M. Potop-Butucaru,
    S. Tucci-Piergiovanni.
    *Correctness of Tendermint-Core Blockchains*.
    OPODIS 2018, LIPIcs 125, 16:1–16:16, 2019.
    [doi:10.4230/LIPIcs.OPODIS.2018.16](https://doi.org/10.4230/LIPIcs.OPODIS.2018.16)

[ETH-SPEC]
    Ethereum Foundation.
    *Phase 0 — Honest Validator* and *Phase 0 — Beacon Chain*.
    `ethereum/consensus-specs`, accessed 2026-05-01.
    [https://github.com/ethereum/consensus-specs/blob/master/specs/phase0/validator.md](https://github.com/ethereum/consensus-specs/blob/master/specs/phase0/validator.md)
    [https://github.com/ethereum/consensus-specs/blob/master/specs/phase0/beacon-chain.md](https://github.com/ethereum/consensus-specs/blob/master/specs/phase0/beacon-chain.md)

[COSMOS-ADR009]
    Cosmos SDK Working Group.
    *ADR 009: Evidence Module*.
    [https://github.com/cosmos/cosmos-sdk/blob/main/docs/architecture/adr-009-evidence-module.md](https://github.com/cosmos/cosmos-sdk/blob/main/docs/architecture/adr-009-evidence-module.md)

[LSP82]
    L. Lamport, R. Shostak, M. Pease.
    *The Byzantine Generals Problem*.
    ACM TOPLAS, 4(3):382–401, 1982.
    [doi:10.1145/357172.357176](https://doi.org/10.1145/357172.357176)

[MR05a]
    L. G. Meredith and M. Radestock.
    *A Reflective Higher-order Calculus*.
    *Electronic Notes in Theoretical Computer Science*, 141(5):49–67, 2005.
    [doi:10.1016/j.entcs.2005.05.016](https://doi.org/10.1016/j.entcs.2005.05.016)

[WWPTWEA15]
    J. R. Wilcox, D. Woos, P. Panchekha, Z. Tatlock, X. Wang, M. D. Ernst,
    T. Anderson.
    *Verdi: A Framework for Implementing and Formally Verifying Distributed Systems*.
    PLDI 2015, 357–368.
    [doi:10.1145/2737924.2737958](https://doi.org/10.1145/2737924.2737958)

### Further Reading

[Mil89] R. Milner. *Communication and Concurrency*. Prentice-Hall, 1989.
ISBN 978-0131149847.

[Mil99] R. Milner. *Communicating and Mobile Systems: The π-Calculus*.
Cambridge University Press, 1999. ISBN 978-0521643207.

[SW01] D. Sangiorgi and D. Walker. *The π-Calculus: A Theory of Mobile
Processes*. Cambridge University Press, 2001. ISBN 978-0521781770.

[San98] D. Sangiorgi. *On the bisimulation proof method*. *Mathematical
Structures in Computer Science*, 8(5):447–479, 1998.
[doi:10.1017/S0960129598002527](https://doi.org/10.1017/S0960129598002527)

[Lyb22] S. Lybech. *Encodability and Separation for a Reflective
Higher-Order Calculus*. arXiv:2209.02356, 2022.
[doi:10.48550/arXiv.2209.02356](https://doi.org/10.48550/arXiv.2209.02356)

[CL99] M. Castro and B. Liskov. *Practical Byzantine Fault Tolerance*.
OSDI 1999, 173–186.
[https://www.usenix.org/conference/osdi-99/practical-byzantine-fault-tolerance](https://www.usenix.org/conference/osdi-99/practical-byzantine-fault-tolerance)

[GKMB17] V. B. F. Gomes, M. Kleppmann, D. P. Mulligan, A. R. Beresford.
*Verifying Strong Eventual Consistency in Distributed Systems*. PACMPL,
1(OOPSLA):109, 2017.
[doi:10.1145/3133933](https://doi.org/10.1145/3133933)

[BBKMW20] S. Braithwaite, E. Buchman, I. Konnov, Z. Milosevic,
I. Stoilkovska, J. Widder, A. Zamfir.
*Formal Specification and Model Checking of the Tendermint Blockchain
Synchronization Protocol*. FMBC 2020, OASIcs 84, paper 10.
[doi:10.4230/OASIcs.FMBC.2020.10](https://doi.org/10.4230/OASIcs.FMBC.2020.10)

[LSZ15] Y. Lewenberg, Y. Sompolinsky, A. Zohar.
*Inclusive Block Chain Protocols*. FC 2015, LNCS 8975, 528–547.
[doi:10.1007/978-3-662-47854-7_33](https://doi.org/10.1007/978-3-662-47854-7_33)

[MR05b] L. G. Meredith and M. Radestock.
*Namespace Logic: A Logic for a Reflective Higher-Order Calculus*.
TGC 2005, LNCS 3705, 353–369.
[doi:10.1007/11580850_19](https://doi.org/10.1007/11580850_19)

---

*"E Pluribus Potentia"*
