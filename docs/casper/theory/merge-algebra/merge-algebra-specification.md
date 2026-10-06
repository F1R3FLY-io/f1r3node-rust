# Merge-Algebra Determinism — Normative Specification

This is the **normative** contract for casper's multi-parent block **merger** — the
requirement that the merge be **byte-identical on every node**. It records *what must
hold*, independent of *how it is checked*. The companion
[verification dossier](./merge-algebra-verification.md) records the mechanized proofs,
models, and tests that discharge each requirement; the
[glossary](./merge-algebra-glossary.md) defines every symbol and term used below (read it
first if any notation is unfamiliar). Rendered diagrams are in
[`diagrams/`](./diagrams/).

Requirement levels **MUST / MUST NOT / SHOULD** are used in the RFC-2119 sense.

---

## 1. Scope

The block merger combines the state changes of the multiple parents (deploy chains) a
block builds on, producing a merged **pre-state hash** and a **rejected-deploy set**. Both
values are **recorded in the block** and **recomputed by every validator**: a node rejects
a block whose recomputed pre-state hash or rejected set differs from the recorded values
(`casper/src/rust/util/rholang/interpreter_util.rs:228` recompute, `:259` pre-state
mismatch ⟹ reject/no-replay, `:269` rejected-set mismatch ⟹ invalid). Therefore the merge
**MUST** be a deterministic (node-identical) function of the merge inputs, or the chain
forks and finalization stalls.

Source of truth:

- `casper/src/rust/merging/deploy_chain_index.rs` — `DeployChainIndex` and its `Ord`
  comparator (`impl Ord::cmp`, the keep-one total order), `PartialEq`/`Hash`, and
  `DeployChainIndex::new` (the user/system event-log split).
- `casper/src/rust/merging/dag_merger.rs` — `dependency_ordered_branch_items` (the greedy
  keep-one selection) and `split_unavailable_branch_consumes` (greedy rejection): the
  **first** order-sensitive consumer.
- `casper/src/rust/merging/conflict_set_merger.rs` — `resolve_conflicts` and
  `compute_merged_state` (the merged-state fold): the **second** order-sensitive consumer.
- `rspace++/src/rspace/merger/channel_change.rs` — `ChannelChange::combine` (the
  per-channel netting) and `state_change.rs` — `StateChange::combine` (the fold operator).
- `rspace++/src/rspace/merger/merging_logic.rs` — `conflicts` (the conflict detector).
- `rspace++/src/rspace/merger/event_log_index.rs` — `EventLogIndex::combine` (the
  set-union monoid).
- `casper/src/rust/util/rholang/interpreter_util.rs` — the validator's merge **recompute**
  and its two mismatch rejections.

## 2. The merge-determinism rule (normative)

Given a base state and a set of deploy chains to merge:

- **R-ORDER.** The keep-one comparator `DeployChainIndex::cmp` **MUST** be a **strict total
  order** whose `Equal`-class is contained in the `Eq` relation: `cmp a b = Equal ⟹ a = b`.
  No two **distinct** deploy chains may compare `Equal`. (The comparator is keyed 1–5:
  Σcost DESC, max single cost DESC, min `deploy_id` ASC, `post_state_hash` ASC, and — the
  **injective** terminal key — `deploys_with_cost.cmp` ASC, the `Eq`/`Hash` key itself.)
- **R-KEEP1.** The greedy keep-one / rejection winner (`dag_merger`'s
  `min_by(cmp)` over `pending`, then `split_unavailable_branch_consumes`) **MUST** be a pure
  function of the branch **set**, never the `HashSet` iteration order that produced
  `pending` (`branch.0.iter()`, whose `RandomState` is reseeded per process).
- **R-FOLD.** The merged-state fold (`compute_merged_state`) **MUST** be order-independent.
  Because the fold operator `StateChange::combine` (built from the per-channel
  `ChannelChange::combine`) is **non-associative** (§5, Finding A), the fold **MUST** proceed
  in the single **canonical order** induced by the total-order comparator (sort survivors
  by `cmp`, then fold), so that every node folds identically.
- **R-CONFLICT.** Every conflict the removed single-value-cell check would flag **MUST** be
  caught by the retained double-consume / same-IO-event race detector (`conflicts`,
  Check #1) **or** be an intrinsically-mergeable number/foldable channel. No real conflict
  may be silently dropped.
- **R-SVC.** A merge that would leave a **single-value NUMBER cell** holding more than one
  datum **MUST** be rejected. On the **non-mergeable** path (a channel not folded as a
  number channel this merge), if the base is a single numeric datum and the post-merge
  cardinality `result_len = |multiset_diff(base, removed)| + |added|` exceeds 1, the merge
  **MUST** fail (`check_single_value_cell_not_overfilled`, `rholang_merging_logic.rs:194`,
  wired at `dag_merger.rs:965`). This covers the **produce-only write** — a produce that
  does **not** consume the base — that R-CONFLICT's consume-then-produce model
  (`svc_update = consumes ∧ produces`) is **vacuous** for and cannot see. Non-numeric bases
  (registry / `TreeHashMap` leaves) are **exempt** (they merge freely). Rationale: a RhoVM
  read of an over-filled single-value cell trips the IntegerAdd single-value invariant and
  **halts finalization** (safety **S7**).
- **R-SPLIT.** `combine(fold user, fold system)` **MUST** equal `fold(all deploys)` on the
  conflict-relevant produce/consume fields, so conflict detection on the split index equals
  detection on the monolithic index. The user/system partition **MUST NOT** hide a
  cross-partition conflict.

## 3. Determinism (normative — the safety core)

- **R-DET.** Every honest node **MUST** derive the **identical** `(pre-state hash,
  rejected-deploy set)` from the identical merge inputs (base state + the set of deploy
  chains). Divergence is a consensus fork or a finalization stall (safety **S1**).
- **R-DET** decomposes into three obligations the implementation **MUST** meet:
  1. **Total keep-one order** (R-ORDER): the `min_by`/`sort` winner is a pure function of
     the chain **set**, so the reseeded `HashSet`/`HashMap` iteration order can never leak
     into the rejected set (the first order-sensitive consumer).
  2. **Canonical fold order** (R-FOLD): the merged-state fold sorts survivors by the total
     order before folding, so the **non-associative** `combine` is applied in one canonical
     order on every node (the second order-sensitive consumer).
  3. **Union-monoid split** (R-SPLIT): the user/system partition recombines, by a
     commutative-associative-idempotent set-union monoid, to exactly the monolithic index.
- **R-RECOMPUTE.** Block validation **MUST** recompute the merge and reject the block on
  any pre-state-hash or rejected-deploy-set mismatch; a pre-state-mismatched block **MUST
  NOT** be replayed (`interpreter_util.rs:259` reject/no-replay; `:269` rejected-set
  mismatch ⟹ `invalid_rejected_deploy`).

## 4. Rolling-upgrade safety (normative)

- **R-APPEND.** Any hardening of the keep-one comparator **MUST** be **append-only** over
  the order deployed in `origin/master` (`v0.4.16`): the pre-existing keys (1–4) **MUST**
  remain byte-identical, and a new key **MAY** only be **appended** as a lower-priority
  tie-break. Consequence: the new key is consulted **only** on inputs the deployed
  comparator already ordered non-deterministically (a `HashSet`-order-dependent 4-key tie),
  so on every input the deployed comparator ordered deterministically the result is
  unchanged. A hardened node therefore **never disagrees** with a live `v0.4.16` node on
  any input the old comparator ordered deterministically, and is deterministic on **all**
  inputs — safe under a mixed-version (rolling) upgrade.

## 5. Disclosed algebraic properties (normative)

- **R-NET.** The per-channel `ChannelChange::combine` (idempotent max-multiset `vec_union`
  + `cancel_common`) is **commutative** but **NON-associative** (**Finding A**): with
  `a = add(x)`, `b = add(x)`, `c = remove(x)`, `(a∘b)∘c = ∅` while `a∘(b∘c) = {x}`. This
  **MUST** be treated as a **disclosed** property of the shipped operator, not a latent
  bug: it is benign for determinism **only because** R-ORDER pins one canonical fold order.
  A change to the `combine` **algebra** (e.g. replacing max-union with the associative
  sum-union monoid) **MUST NOT** be made silently, because it alters merge **semantics**
  (which data survive a merge), not merely fold order.

## 6. Explicit non-requirement (design boundary)

- **N-SEMANTICS.** The merge-algebra hardening **MUST NOT** change merge **semantics**.
  Specifically, the non-associative max-union `ChannelChange::combine` is **NOT** to be
  replaced by the associative sum-union monoid as part of a determinism fix; **Finding A**
  is **disclosed** and rendered **benign** by the deterministic fold order (R-ORDER), not
  eliminated. Any implementation **MUST NOT** alter the observable merge result under the
  guise of a determinism fix. The determinism guarantee is *byte-identical recomputation of
  the current semantics*, **not** a redefinition of the merge.
  - **Runtime guard (allowed, semantics-preserving).** The *assumption* R-ORDER relies on —
    that no order-dependent survivor pair reaches apply — **MAY** be enforced by a
    detection-only runtime guard (`OrderDependenceGuard`, `conflict_set_merger.rs`) that trips
    iff a datum is contributed to a side by ≥ 2 distinct survivors on a non-mergeable channel.
    Such a guard is permitted **because it changes no post-state** (`debug_assert!` + release
    log/metric only); it does **not** alter the operator or the merge result. Sound by
    `ChannelNetting.v combine_max_order_independent_under_no_dup`.

## 7. Safety invariants — MUST NEVER happen

| ID | Must never |
|---|---|
| **S1** | Two honest nodes derive a different `(pre-state hash, rejected-deploy set)` for the same merge inputs — a fork / finalization stall (violates R-DET). |
| **S2** | The reseeded `HashSet` iteration order leaks into the greedy keep-one / rejection winner (violates R-ORDER / R-KEEP1) — **Finding B**. |
| **S3** | The branch/item fold order changes the merged state (violates R-FOLD; reachable because `combine` is non-associative — **Finding A**). |
| **S4** | The removed single-value-cell check silently dropped a real, non-number conflict (violates R-CONFLICT) — **GAP-3**. |
| **S5** | The user/system event-log split hides a cross-partition conflict (violates R-SPLIT) — **P2**. |
| **S6** | A comparator hardening breaks agreement with a live `v0.4.16` node on an input the old order already resolved deterministically (violates R-APPEND). |
| **S7** | A merge persists a **single-value NUMBER cell** holding > 1 datum (a produce-only over-fill), so a later RhoVM read trips the IntegerAdd single-value invariant and **halts finalization** (violates R-SVC) — **§3c**, RCA-asi-devnet-finality-halt. |

## 8. Conformance

An implementation conforms iff every **R-** requirement holds, no **S-** invariant is
reachable, and **N-SEMANTICS** is respected. The
[verification dossier](./merge-algebra-verification.md) maps each requirement/invariant to
its mechanized artifact (Rocq axiom-free capstones, Z3 cross-witnesses) and Rust regression
tests; run them locally with `scripts/check-merge-algebra-ALL.sh` (formal verification is
**local-only** — never wired into CI).
