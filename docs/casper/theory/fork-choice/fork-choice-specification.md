# Fork-Choice ("Ghosting") — Normative Specification

This is the **normative** contract for casper's LMD-GHOST fork-choice estimator: *what
must hold*, independent of *how it is checked*. The companion
[verification dossier](./fork-choice-verification.md) records the mechanized proofs,
models, and tests that discharge each requirement; the
[glossary](./fork-choice-glossary.md) defines every symbol and term used below (read it
first if any notation is unfamiliar). Rendered diagrams are in
[`diagrams/`](./diagrams/).

Requirement levels **MUST / MUST NOT / SHOULD** are used in the RFC-2119 sense.

---

## 1. Scope

The fork-choice estimator selects the canonical tip(s) of the block DAG — the main
parent a new block builds on, and the secondary parents it merges. It is the
GHOST-style heaviest-subtree rule with a slashing-aware weight filter. Source:
`casper/src/rust/estimator.rs` (the estimator), `casper/src/rust/util/proto_util.rs`
(`weight_from_validator_by_dag`), `casper/src/rust/util/dag_operations.rs` (the LCA),
`shared/src/rust/shared/list_ops.rs` (`sort_by_with_decreasing_order`, the tie-break),
`casper/src/rust/engine/multi_parent_casper/snapshot.rs` (main-parent selection), and
`casper/src/rust/validate.rs` (`parents`, the validator-side bound check).

## 2. The fork-choice rule (normative)

Given a DAG `d` and a frozen `latest_messages` map (validator → their latest block):

- **R-FILTER.** Latest messages of **slashed / invalid** validators MUST be removed
  before scoring, so they contribute **zero** weight to fork choice (the T-10 property,
  proven for slashing). No dropped validator may influence the outcome.
- **R-LCA.** The estimator MUST compute a lowest universal common ancestor `lca` of the
  (depth-filtered) latest messages, and score relative to it. A latest message deeper
  than `LATEST_MESSAGE_MAX_DEPTH` below the top MUST be filtered out **deterministically**
  (a pure function of the DAG), bounding the scored band.
- **R-SCORE.** Each block's score MUST be the **sum** of the weights of the validators
  whose latest message supports it (i.e. descends from it), accumulated down the
  supporting chains to the `lca`. The accumulation MUST be order-independent
  (associative + commutative).
- **R-GHOST.** The head MUST come from a descent that commits, at each fork, to the
  main-parent child of **maximum cumulative score** (the heaviest subtree) and stops
  at the frontier; the descent's endpoint is the canonical main tip. Ranking tips by
  their own scores is NOT a conforming implementation: a tip's own score is only its
  owner's weight, so concurrent proposal ties every tip and the head falls to hash
  order.
- **R-TOTAL.** The tie-break MUST be a **total order** on distinct blocks: score
  descending, then block-hash ascending. This makes the ranked head **unique**.

## 3. Determinism (normative — the safety core)

- **R-DET.** Every honest node MUST compute the **identical** fork-choice
  `(tips, lca, main_parent)` from the identical `(DAG, latest_messages)`. Fork-choice
  divergence is a consensus fork (safety **S1**).
- **R-DET** decomposes into three obligations the implementation MUST meet:
  1. **Total tie-break** (R-TOTAL): the ranked argmax is a pure function of the scored
     tip *set*, so the `HashSet`/`HashMap` iteration order that produced it can never
     leak into the result.
  2. **Integer weights**: scores MUST be computed in exact integer (`i64`) arithmetic
     read from block-structural bonds (the main parent's on-chain weight map) — never a
     node-local view and never floating-point. (Contrast: finalization's `f32` ratio is
     a separate, disclosed precision residual; fork choice has no such residual.)
  3. **Deterministic LCA** (R-LCA): the depth filter and LCA MUST be pure functions of
     the DAG (structural top height), identical across nodes.

## 4. Bounds and truncation (normative)

- **R-COUNT.** At most `max_number_of_parents` tips MUST be returned. The **main tip
  (head) MUST be preserved** by any truncation. The "unlimited" sentinels — the
  estimator's `i32::MAX` and the config wire convention `-1` — MUST both be treated as
  *unlimited* explicitly (take all), not by relying on integer-cast wraparound.
- **R-DEPTH.** Secondary parents deeper than `max_parent_depth` below the main tip MAY
  be dropped; the main tip MUST NOT be dropped by depth filtering.
- **R-NOPANIC.** An empty ranked-tip set MUST surface a typed error, never a panic
  (`filter_deep_parents` split-first guard).

## 5. Robustness (normative)

- **R-B1.** Reading a validator's weight MUST NOT panic when a traversed block or its
  main parent is momentarily absent from the metadata index (a sync/prune window); it
  MUST return a typed error.
- **R-B3.** Score accumulation MUST NOT silently overflow; an overflow (reachable only
  if cumulative bonded weight exceeds `i64::MAX`, a supply-cap violation) MUST surface a
  typed error, never a wrapped score.

## 6. Explicit non-requirement (design boundary)

- **N-VALID.** Validators do **NOT** recompute the fork-choice of a received block.
  `Validate::parents` bound-checks the *declared* parents (count, depth, progress) only;
  consensus safety of parents is anchored by the finalized-floor committee/bonds
  validation, not by re-running the estimator (`snapshot.rs`: "validators replay
  declared parents, not fork-choice"). Therefore the correct proposer↔validator bridge
  is **bound-consistency** (an honest proposer's depth-filtered parents MUST pass
  `Validate::parents`) plus observer-level determinism (R-DET), not fork-choice
  equality. Any implementation MUST NOT be specified or tested as if validators
  recompute fork choice.

## 7. Safety invariants — MUST NEVER happen

| ID | Must never |
|---|---|
| **S1** | Two honest nodes disagree on the fork-choice tips/main-parent for the same DAG (violates R-DET/R-TOTAL). |
| **S2** | A slashed/invalid validator influences fork choice (violates R-FILTER). |
| **S3** | The main tip is dropped by count/depth truncation (violates R-COUNT/R-DEPTH). |
| **S4** | The estimator panics on a missing-metadata or empty-tips edge (violates R-NOPANIC/R-B1). |
| **S5** | A wrapped (overflowed) score changes the ranking (violates R-B3). |
| **S6** | The LCA/depth filter differs across nodes for the same DAG (violates R-LCA). |

## 8. Conformance

An implementation conforms iff every **R-** requirement holds, no **S-** invariant is
reachable, and **N-VALID** is respected. The
[verification dossier](./fork-choice-verification.md) maps each requirement/invariant to
its mechanized artifact (Rocq axiom-free capstones, TLA⁺ models, Z3/Sage/Wolfram
cross-witnesses) and Rust regression tests; run them locally with
`scripts/check-fork-choice-ALL.sh` (formal verification is **local-only** — never wired
into CI).
