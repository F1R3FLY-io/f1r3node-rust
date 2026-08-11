# Merge-Algebra Determinism — Normative Specification

This is the **normative** contract for casper's multi-parent block **merger** — the
requirement that the merge be **byte-identical on every node**. It records *what must
hold*, independent of *how it is checked*. The companion
[verification dossier](./merge-algebra-verification.md) records the mechanized proofs,
models, and tests that discharge each requirement; the
[glossary](./merge-algebra-glossary.md) defines every symbol and term used below (read it
first if any notation is unfamiliar).

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
- `rspace++/src/rspace/merger/channel_change.rs` — `ChannelChange::additive_join`
  and `ChannelChange::normalized` (per-channel multiset composition), and
  `state_change.rs` — `StateChange::additive_join` plus one final normalization.
- `casper/src/rust/rholang/runtime.rs` and `replay_runtime.rs` — per-execution
  pre-state/post-state witnesses and replay validation.
- `casper/src/rust/merging/block_index.rs` — exact per-execution state deltas.
- `rspace++/src/rspace/merger/merging_logic.rs` — `conflicts` (the conflict detector).
- `rspace++/src/rspace/merger/event_log_index.rs` — `EventLogIndex::combine` (the
  set-union monoid).
- `casper/src/rust/util/rholang/interpreter_util.rs` — the validator's merge **recompute**
  and its two mismatch rejections.

## 2. The merge-determinism rule (normative)

Given a base state and a set of deploy chains to merge:

- **R-ORDER.** The keep-one comparator `DeployChainIndex::cmp` **MUST** be a **strict total
  order** whose `Equal`-class is contained in the `Eq` relation: `cmp a b = Equal ⟹ a = b`.
  No two **distinct** deploy chains may compare `Equal`. The comparator has four priority
  tiers — total cost, maximum single cost, minimum deploy ID, and post-state hash — followed
  by the **injective composite identity**
  `(deploys_with_cost, source_block_hash, effect_indices, witness_mode)`. The terminal
  components are compared lexicographically and match the fields used by `Eq`/`Hash`.
- **R-KEEP1.** The greedy keep-one / rejection winner (`dag_merger`'s
  `min_by(cmp)` over `pending`, then `split_unavailable_branch_consumes`) **MUST** be a pure
  function of the branch **set**, never the `HashSet` iteration order that produced
  `pending` (`branch.0.iter()`, whose `RandomState` is reseeded per process).
- **R-WITNESS.** Every execution in a new-format block **MUST** carry the state root
  immediately before it and the state root immediately after it. The witnesses **MUST**
  form one contiguous chain from the block pre-state to the block post-state. Replay
  **MUST** checkpoint after each execution and reject a missing half-witness, a gap, or a
  replayed post-state mismatch. A state delta **MUST** be computed from that execution's
  own roots, not by copying the whole-block delta onto every deploy chain.
- **R-CAUSAL.** Deduplication **MUST** operate on causal execution identity
  `(source_block_hash, execution_index)`, never on serialized RSpace content. Repeated
  observations of one identity with equal state and number-channel contributions count
  once. Repeated observations of one identity with unequal contributions are an invariant
  violation and **MUST** fail closed.
- **R-FOLD.** After causal deduplication, distinct execution deltas **MUST** compose by
  additive multiset union and normalize once. The additive fold is associative and
  commutative, so survivor enumeration and association cannot alter the result. It is
  intentionally not idempotent: two independent sends with byte-identical payloads are two
  RSpace data, exactly as two parallel outputs form a two-element bag.
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
  2. **Causal map plus additive projection** (R-WITNESS/R-CAUSAL/R-FOLD): exact
     per-execution deltas are deduplicated by identity, then their multiset multiplicities
     are added and normalized once.
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

- **R-NET.** The legacy per-channel `ChannelChange::combine` (idempotent max-multiset
  `vec_union` plus inline `cancel_common`) is commutative but **NON-associative**
  (**Finding A**). Deferring cancellation repairs association dependence but does not
  repair semantics: max-union collapses two distinct byte-identical outputs. The current
  operator therefore adds multiplicities for distinct causal identities and performs one
  final cancellation. Causal map union supplies idempotence at the correct layer.

### 5.1 Terms and projection

An **execution identity** identifies one deterministic transition within one source block.
An **exact delta** is the signed multiset difference between that execution's witnessed
pre-state and post-state. An **effect map** is the finite map from execution identities to
exact deltas. Its union is defined only when repeated keys carry equal values.

For the finite key set `$`K`$` and effect map `$`M`$`, the state change applied to the
merge base is:

```math
\Delta(M) = \operatorname{normalize}\!\left(\sum_{k \in K} M(k)\right).
```

The map union is idempotent because a key occurs once. The sum is non-idempotent because
RSpace content is a multiset. These are different operations at different semantic layers.

The merger implements the definition in this order:

```text
merge_exact_effects(surviving_chains):
    effects := empty ordered map
    for chain in canonical_order(surviving_chains):
        require chain uses exact witnesses
        for (identity, delta, numeric_delta) in chain.effects:
            if identity is absent:
                effects[identity] := canonical(delta, numeric_delta)
            else:
                require effects[identity] = canonical(delta, numeric_delta)
    state_delta := normalize(add every effects[*].delta)
    numeric_delta := checked_combine every effects[*].numeric_delta
    return apply(state_delta, numeric_delta)
```

This separation is not an implementation preference. Meredith's RSpace denotation treats
parallel composition as key-wise finite-multiset union and explicitly distinguishes two
equal outputs from one output (`../publications/denotational-semantics-for-rho/knot-rho.tex`,
section “The RSpace denotation: parallel composition as keyed multiset union”). The node
therefore deduplicates executions, not messages.

## 6. Activation boundary

- **R-ACTIVATION.** Per-execution witnesses alter the block wire shape and additive
  projection changes merge results for reachable Rholang programs. The feature **MUST**
  activate atomically on an unreleased shard or at an explicit protocol boundary after a
  finalized cut. A merge epoch **MUST NOT** mix exact-witness and legacy block indices.
- **N-MAX.** The implementation **MUST NOT** use max-union to compose distinct execution
  effects. It loses valid multiplicity.
- **N-WHOLE.** The implementation **MUST NOT** add per-chain deltas derived from the same
  whole-block pre/post roots. That duplicates the whole block once per chain. Exact
  execution roots are required before additive composition is sound.

## 7. Safety invariants — MUST NEVER happen

| ID | Must never |
|---|---|
| **S1** | Two honest nodes derive a different `(pre-state hash, rejected-deploy set)` for the same merge inputs — a fork / finalization stall (violates R-DET). |
| **S2** | The reseeded `HashSet` iteration order leaks into the greedy keep-one / rejection winner (violates R-ORDER / R-KEEP1) — **Finding B**. |
| **S3** | Survivor enumeration or association changes the merged state (violates R-FOLD; the legacy inline-normalization **Finding A**). |
| **S4** | The removed single-value-cell check silently dropped a real, non-number conflict (violates R-CONFLICT) — **GAP-3**. |
| **S5** | The user/system event-log split hides a cross-partition conflict (violates R-SPLIT) — **P2**. |
| **S6** | A comparator hardening breaks agreement with a live `v0.4.16` node on an input the old order already resolved deterministically (violates R-APPEND). |
| **S7** | A merge persists a **single-value NUMBER cell** holding > 1 datum (a produce-only over-fill), so a later RhoVM read trips the IntegerAdd single-value invariant and **halts finalization** (violates R-SVC) — **§3c**, RCA-asi-devnet-finality-halt. |
| **S8** | Two distinct executions that emit identical serialized data collapse to one datum (violates R-FOLD / RSpace finite-multiset semantics). |
| **S9** | A whole-block delta is attributed to more than one deploy chain and then added repeatedly (violates R-WITNESS / N-WHOLE). |
| **S10** | The same causal identity is accepted with two unequal contributions (violates R-CAUSAL). |

## 8. Conformance

An implementation conforms iff every **R-** requirement holds, no **S-** invariant is
reachable, and **N-MAX** and **N-WHOLE** are respected. The
[verification dossier](./merge-algebra-verification.md) maps each requirement/invariant to
its mechanized artifact (Rocq axiom-free capstones, Z3 cross-witnesses) and Rust regression
tests; run them locally with `scripts/check-merge-algebra-ALL.sh` (formal verification is
**local-only** — never wired into CI).
