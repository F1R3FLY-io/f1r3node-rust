# Finalized-Floor Multi-Parent Merge — Normative Specification

This is the **normative** contract for the finalized-floor multi-parent merge: *what
must hold*, independent of *how it is checked*. The companion
[verification dossier](./finalized-floor-verification.md) records the mechanized
proofs, models, and tests that discharge each requirement; the
[glossary](./finalized-floor-glossary.md) defines every symbol and term used below
(read it first if any notation is unfamiliar). Rendered diagrams are in
[`diagrams/`](./diagrams/).

Requirement levels **MUST / MUST NOT / SHOULD** are used in the RFC-2119 sense.

---

## 1. Scope

The feature selects the base state on which a new block `B`'s multi-parent merge is
built, and folds the parents' unfinalized writes onto it. It touches floor
derivation (`casper/src/rust/finality/floor.rs`), the clique oracle
(`casper/src/rust/safety/clique_oracle.rs`), the merge driver
(`casper/src/rust/util/rholang/interpreter_util.rs`), and the number-channel
write-algebra (`rspace++/.../merging_logic.rs`,
`casper/src/rust/merging/conflict_set_merger.rs`,
`rholang/.../rholang_merging_logic.rs`).

## 2. The floor rule (normative)

For a block `B` with non-empty parent set `P₁…Pₖ` and frozen justification snapshot
`just(B)`:

- **R-FLOOR.** `floor(B)` MUST be the **maximum** (by block number, tie-broken by
  hash) candidate over the union of two sources, each a **pure function of `B`**:
  1. **Inheritance** — every parent's own floor.
  2. **Advancement** — per parent, the highest main-chain ancestor `A` with
     `ft_witnessed(A, just(B)) ≥ θ` (genesis is finalized by definition).
- **R-SOUND.** The chosen `floor(B)` MUST be a **sound merge base**: either
  (**Case-A**) a general DAG-ancestor of every parent, or (**Case-B**) a candidate
  with which every other candidate is compatible (lies in its DAG past, or is
  mergeable via a common-descendant parent). The **highest** sound candidate MUST be
  chosen.
- **R-ERR.** If **no** candidate is a sound base, the derivation MUST return a
  deterministic error (incompatible finalized fork) — it MUST NOT silently pick an
  unsound base.
- **R-SNAP.** Finalization MUST be evaluated over `just(B)` only — never a node-local
  live DAG view.

## 3. Determinism (normative)

- **R-DET.** Every honest node MUST derive the **identical** `floor(B)` for the same
  `B`. Since both floor sources are block-structural facts and `just(B)` is frozen,
  the result is node-identical (see S1).
- **R-CACHE.** The persisted frontier cache is an optimization only: the warm
  incremental up-walk MUST yield the **identical** frontier as the cold down-walk.
  When a determinism premise fails (committee change in band, or the pivot no longer
  finalizes over the larger snapshot), the warm path MUST fall back to the cold walk.
- **R-COMM.** The committee used to validate `B`'s bonds MUST be `bonds_of(floor(B))`
  — a pure function of the floor.

## 4. Merge base, scope, and the Δ-backstop (normative)

- **R-BASE.** The merge base MUST be `floor(B).post_state`.
- **R-SCOPE.** The merge scope MUST be `closure(parents) \ closure(floor)`; the
  floor-bounded ancestor scan MUST cover **every** parent write with block number
  `≥ num(floor)` and MUST NOT cut above the floor.
- **R-BACKSTOP.** When the floor distance `Δ = num(maxParent) − num(floor)` exceeds
  the cap, the merge MUST fail with a deterministic error keyed on `Δ` alone (on
  propose it parks the round; on validate the block is invalid). It MUST NOT
  substitute a lossy single-parent post-state. Any non-node-deterministic quantity
  (e.g. branch-width scope size) MUST NOT gate admission (demote to a metric).

## 5. Merge write-algebra (normative)

- **R-BITMASK.** BitmaskOr channels MUST combine by bitwise OR — a semilattice
  (idempotent, commutative, associative); no set bit may be lost, and the fold MUST
  be order-independent.
- **R-INTADD-COMBINE.** IntegerAdd diffs MUST be combined with checked addition;
  an overflow MUST reject the branch (never wrap-then-launder).
- **R-INTADD-APPLY.** The terminal apply `base + Σdiffs` (the consensus-state write)
  MUST use checked addition with a `≥ 0` guard, returning an error on overflow OR a
  negative balance.
- **R-INTADD-DIFF.** The per-deploy diff `end − prev` MUST use wrapping subtraction —
  the group inverse of the wrapping add that language-level execution used — so it
  recovers the deploy's true intended delta. Overflow MUST be caught at combine/apply
  (R-INTADD-COMBINE/APPLY), NOT at the diff (which is on the live execution path and
  must never crash on a deploy that is instead gracefully merge-rejected).

## 6. Safety invariants — MUST NEVER happen

| ID | Must never |
|---|---|
| **S1** | Two honest nodes disagree on `floor(B)` (violates R-DET/R-CACHE). |
| **S2** | The floor regresses along ancestry (violates R-FLOOR monotonicity). |
| **S3** | A block finalizes below `θ` or against a shrunk denominator. |
| **S4** | An unsound merge base is used instead of erroring (violates R-SOUND/R-ERR). |
| **S5** | A single-value cell keeps two writes, or a mergeable write is lost/dropped — *the ~400-block bug* (violates R-SCOPE/R-BACKSTOP). |
| **S6** | Non-deterministic merge output → fork (violates R-BITMASK/R-INTADD). |
| **S7** | A negative vault balance or a laundered overflow is committed (violates R-INTADD-APPLY). |
| **S8** | Bonds validated against a non-floor committee (violates R-COMM). |

## 7. Liveness invariants — MUST eventually happen

| ID | Must eventually |
|---|---|
| **L1** | A common finalized cut ⟹ `derive_floor` returns it. |
| **L2** | Keep-one losers are re-proposed (recovery). |
| **L3** | The floor advances, `Δ` stays bounded, walks/scope stay bounded — *the ratchet driver is neutralized*. |
| **L4** | Non-conflicting writers converge. |
| **L5** | Multi-parent finality does not wedge. |

## 8. Conformance

An implementation conforms iff every **R-** requirement holds and no **S-** invariant
is reachable, with **L-** invariants holding under the partial-synchrony progress
assumption. The [verification dossier](./finalized-floor-verification.md) maps each
requirement/invariant to its mechanized artifact (Rocq axiom-free capstones, TLA⁺
models, Z3/Sage cross-witnesses) and Rust regression tests; run them locally with
`scripts/check-finalized-floor-ALL.sh` (formal verification is **local-only** — never
wired into CI).
