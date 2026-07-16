# Merge-Algebra Determinism — Glossary & Literate Algorithms

A pedagogical companion to the [verification dossier](./merge-algebra-verification.md).
Part 1 defines every symbol, acronym, and key term **before it is used** elsewhere in the
merge-algebra documentation. Part 2 presents the two load-bearing algorithms — the keep-one
total order and the merged-state fold — in Knuth's literate-programming style (prose
interleaved with the code chunks it explains), with the invariants that make them correct.

All mathematical expressions use unicode and are quoted in backticks.

---

## 1. Glossary

### 1.1 Merge & deploy chains

| Term | Definition |
|---|---|
| **deploy** | A signed program/transaction submitted to the shard; the atom a block executes. |
| **deploy chain / `DeployChainIndex`** | A maximal chain of mutually-dependent deploys carrying their costs, event-log index, and a resulting `post_state_hash`; the minimal unit of conflict resolution and rejection in a multi-parent merge. |
| **`deploys_with_cost`** | The `HashableSet<DeployIdWithCost>` that **identifies** a chain; the `Eq`/`Hash` key — two chains are `Eq` iff their `deploys_with_cost` are equal (`deploy_chain_index.rs:136,142`). |
| **`HashableSet<T>`** | A `HashSet<T>` wrapper with a **deterministic** `Ord` (shortlex over the sorted elements) and content-based `Eq`/`Hash`. Its `Ord` sorts, so it is independent of `HashSet` iteration order. |
| **branch** | A set of mutually-dependent deploy chains; the merge partitions the merge-set into branches (`compute_branches`). |
| **multi-parent merge** | Combining the state changes of the several parent blocks a new block builds on into one merged base state. |
| **base state** | The common state the branches' diffs are applied over. |
| **pre-state hash** | The merged base-state root the block's deploys execute against; **recorded in the block** and **recomputed** by every validator. |
| **rejected-deploy set** | The deploys dropped by conflict resolution (late set + dependents + optimal rejection); recorded and recomputed. |
| **`pending`** | In `dependency_ordered_branch_items`, the working `Vec` built from `branch.0.iter()` — a `HashSet` whose `RandomState` is **reseeded per process**, so its iteration order varies across nodes. |

### 1.2 Comparator, netting, conflict

| Term | Definition |
|---|---|
| **keep-one comparator `cmp`** | `DeployChainIndex::cmp` (`deploy_chain_index.rs:151-230`): the 5-key lexicographic total order `min_by`/`sort` use to pick the merge winner and the canonical fold order. |
| **injective key** | A comparator key distinguishing **all** distinct values. The 5th key (`deploys_with_cost.cmp`) is injective because it **is** the `Eq` key. |
| **`Equal`-class ⊆ `Eq`** | `cmp a b = Equal ⟹ a = b`: the property that makes the order strict-total and the winner **unique** (no two distinct chains tie). |
| **`min_by` / `sort` winner** | The unique argmin/argmax the comparator selects; node-identical **iff** `cmp` is a strict total order. |
| **`ChannelChange<A>`** | A per-channel diff `(added, removed)` of multisets; the atom of a `StateChange` (`channel_change.rs`). |
| **`vec_union`** | Idempotent **max-multiset** union (keep `left`, append the `right` elements not already present): per element the multiplicity is `max(mult_left, mult_right)` (`channel_change.rs:25`). |
| **`cancel_common`** | Remove matched `added`/`removed` pairs — min-multiplicity cancellation (`channel_change.rs:34`). |
| **`ChannelChange::combine`** | `vec_union(added)` + `vec_union(removed)` + `cancel_common` (`channel_change.rs:17`): **commutative** but **NON-associative** (Finding A). |
| **`StateChange::combine`** | The merge fold operator (`state_change.rs:507`); composes `ChannelChange::combine` per channel (`:541,:589`), inheriting non-associativity. |
| **netting** | The accumulation of per-channel diffs into one merged diff. |
| **conflict / `conflicts`** | The detector (`merging_logic.rs:262`): Check #1 same-IO-event race (`produces_consumed`/`consumes_produced` intersection, minus both-mergeable), Check #2 potential COMMs, Check #3 produce-touches-base-join. |
| **single-value cell** | A channel holding exactly one datum. A read-modify-write **consumes** the base datum and **produces** a new one (a consume-then-produce, keeping cardinality 1); a **produce-only write** produces *without* consuming the base, **over-filling** the cell to two data — which trips the RhoVM IntegerAdd single-value invariant at read (safety **S7**, the finality halt). |
| **produce-only write** | A write (a produce landing in `ChannelChange.added`) that does **not** consume the base datum (`removed = ∅`). On a single-value NUMBER cell it leaves `kept + added > 1` values — the §3c over-fill the consume-then-produce conflict check (`svc_update = consumes ∧ produces`) is **vacuous** for, since it requires a consume. |
| **§3c discriminator** | `check_single_value_cell_not_overfilled` (`rholang_merging_logic.rs:194`), wired on the **non-mergeable** else-path of the merge (`dag_merger.rs:1065`). Rejects a merge whose post-state single-value NUMBER cell would hold `result_len = multiset_diff(base, removed).len() + added.len() > 1`. Non-numeric bases (registry / `TreeHashMap` leaves) are exempt — they merge freely. It is the discriminator that separates a purse/cell (conflict) from a registry (merge) among **disjoint-consumed / produce-only** writes, which the consume-then-produce check cannot distinguish. |
| **number / foldable channel** | A channel whose values fold (`MergeType::IntegerAdd`, `MergeType::BitmaskOr`), so concurrent updates never truly conflict; classified by `is_number_ch`; carried in `NumberChannelsDiff` (`merging_logic.rs:34`). |
| **`EventLogIndex`** | The per-chain index of produce/consume events; `EventLogIndex::combine` — the `combine` associated function (`event_log_index.rs:343`) — is field-wise **set union**. |
| **user / system split** | Deploys partitioned by `is_system_deploy_id` into a user and a system event-log index, then combined (`deploy_chain_index.rs:70-88`). |
| **`v0.4.16`** | The deployed release (`origin/master`) whose 4-key comparator the fix extends **append-only**. |

### 1.3 The theorem catalog

| ID | Meaning |
|---|---|
| **T-DET** | Determinism: identical merge inputs ⟹ identical `(pre-state hash, rejected-deploy set)` on every node. Its failure is a fork / finalization stall (safety **S1**). |
| **T-ORDER** | `cmp` is a **strict total order** whose `Equal`-class ⊆ `Eq` ⟹ the winner is **unique** (the core of T-DET). **Unconditional** — no `NoDup`/collision premise. |
| **T-KEEP1** | The greedy keep-one / rejection winner is a pure function of the branch **set** (consumer 1). |
| **T-FOLD** | The merged-state fold is order-independent given the canonical (total-order) sort (consumer 2). |
| **T-NET** | `ChannelChange::combine` is commutative but **non-associative** (Finding A, exhibited); the sum-union fix is a commutative monoid with a permutation-invariant fold. |
| **T-CONFLICT** | The removed single-value-cell check ⊆ the retained double-consume detector ∪ the number-channel exemption (GAP-3 soundness). |
| **T-SVC** | The §3c guard rejects a single-value NUMBER cell merge **iff** it would over-fill the cell (`result_len > 1`); non-number bases are exempt. Rocq `svc_guard_catches_overfill` + `svc_invariant_iff_both_detectors`; Rust `svc_guard_rejects_iff_result_len_gt_one`. |
| **T-OVERFILL** | A **produce-only** over-fill **escapes** the retained double-consume detector (it consumes no base), so §3c is a **separate, non-subsumed** detector — the two detectors together are exactly complete. Rocq `overfill_not_retained` + `svc_guard_not_subsumed_exhibit`; Rust `produce_only_overfill_escapes_retained_detector`. |
| **T-SPLIT** | `combine(fold user, fold system) = fold all` ⟹ the user/system split hides no conflict (P2). |
| **T-RECOMPUTE** | Block validation recomputes the merge and rejects on any `(pre-state \| rejected-set)` mismatch — why determinism is load-bearing. |
| **T-APPEND** | The comparator fix is append-only over `v0.4.16` ⟹ rolling-upgrade-safe. |

---

## 2. Literate algorithms

### 2.1 The keep-one total order (`DeployChainIndex::cmp`)

**Problem.** From a **set** of deploy chains — enumerated in a per-process-reseeded
`HashSet` order (`branch.0.iter()`) — pick a **node-identical** merge winner and impose a
**canonical fold order**. Any dependence on the iteration order would make the rejected set
(and the merged state) node-dependent, forking the chain (safety **S1**).

**Why it is deterministic.** The comparator is a 5-key lexicographic tower whose terminal
key is the **injective** `Eq`/`Hash` key. Keys 1–4 are **functions** of the deploy set;
key 5 **is** the set (its deterministic shortlex order). So `cmp a b = Equal` holds iff the
two chains share the same set, i.e. iff they are `Eq` — the `Equal`-class is contained in
`Eq`, and **no two distinct chains tie** (**T-ORDER**). A strict total order makes the
`min_by`/`sort` winner a pure function of the chain set — **T-KEEP1** — independent of the
`HashSet` iteration order.

⟨ *Compare two deploy chains `a`, `b`, returning `Ordering`; lexicographic over 5 keys.* ⟩
```
cmp(a, b):
    k1 ← Σcost(a).cmp(Σcost(b)).reverse()          ⟨ 1. higher TOTAL cost first ⟩
    if k1 ≠ Equal: return k1
    k2 ← maxcost(a).cmp(maxcost(b)).reverse()       ⟨ 2. higher single cost first ⟩
    if k2 ≠ Equal: return k2
    k3 ← min_deploy_id(a).cmp(min_deploy_id(b))     ⟨ 3. smallest signature first ⟩
    if k3 ≠ Equal: return k3
    k4 ← post_state_hash(a).cmp(post_state_hash(b)) ⟨ 4. KEPT byte-identical to v0.4.16 ⟩
    if k4 ≠ Equal: return k4
    return deploys_with_cost(a).cmp(deploys_with_cost(b))   ⟨ 5. the INJECTIVE Eq key ⟩
```

**Invariant (definiteness).** `cmp a b = Equal ⟺ deploys_with_cost(a) = deploys_with_cost(b)
⟺ a = b`. Keys 1–4 can only *narrow*; the terminal key decides every residual tie by the
`Eq` key itself, so the `Equal`-class is exactly the identity. (Rocq
`KeepOneOrder.keep_one_total_order`, `keep_one_equal_impl_eq`, `output_indep_of_input_perm`,
`sort_argmax_unique` — all **unconditional**; Z3 `keep_one_total_order.py`, plus the dual
`sat` probe showing that **without** key 5 two distinct chains tie.)

**Rolling-upgrade safety.** Keys 1–4 are byte-identical to the deployed `v0.4.16`
comparator; key 5 is **appended** (`deploy_chain_index.rs:210-228`). Key 5 is consulted
only on inputs where the 4-key order already returned `Equal` — inputs that were **already
`HashSet`-nondeterministic** on `v0.4.16` — so on every input the old order resolved
deterministically the result is unchanged (**T-APPEND**, safety **S6**).

### 2.2 The merged-state fold (`compute_merged_state`)

**Problem.** Fold the surviving chains' state diffs into one merged state. The fold
operator `StateChange::combine` is **non-associative** (it composes the non-associative
per-channel `ChannelChange::combine`, **Finding A**), so the fold result depends on the
order — a determinism hazard unless the order is canonical.

**Why it is deterministic.** Sort the survivors by the total order `cmp` **first**, then
fold left-to-right. Because `cmp` is a strict total order (**T-ORDER**), the sorted
sequence is a pure function of the survivor **set** (`output_indep_of_input_perm`); every
node folds the **identical** sequence, so the non-associative `combine` is applied in one
canonical association on every node — **T-FOLD**.

⟨ *Combine the resolved survivors' diffs in the canonical (total-order) order.* ⟩
```
survivors ← resolved.to_merge                       ⟨ branches surviving conflict resolution ⟩
sort survivors by compare_branches                  ⟨ :385 — bottoms out in DeployChainIndex::cmp ⟩
items ← []
for branch in survivors:                            ⟨ in the sorted branch order ⟩
    branch_items ← sort(branch)                     ⟨ :391 — items by cmp (the total order) ⟩
    items ← items ++ branch_items
combined ← StateChange::empty()
for item in items:                                  ⟨ LEFT fold in the canonical order ⟩
    combined ← combined.combine(state_changes(item))   ⟨ :426 — NON-associative; order fixed by sort ⟩
return apply_trie_actions(combined)
```

**Invariant (canonical order is load-bearing).** Were `combine` associative **and**
commutative, any fold order would agree and the sort would be a mere optimization. It is
only **commutative** (`ChannelNetting.combine_max_comm`), **not** associative
(`combine_not_assoc_exhibit`): `(add(x)∘add(x))∘remove(x) = ∅` but
`add(x)∘(add(x)∘remove(x)) = {x}`. So the total-order sort selects the **single**
association every node uses — this is why R-ORDER is a *safety* requirement, not a nicety.
(Rocq `KeepOneOrder.output_indep_of_input_perm` for the sorted-sequence purity;
`ChannelNetting.combine_not_assoc_exhibit` for why one canonical order is required; the
sum-union monoid `channel_netting_fixed_deterministic` shows the *disclosed, not-shipped*
alternative that would make the fold order-agnostic — at the cost of changed merge
**semantics**, §6 N-SEMANTICS.)

---

## 3. References

See the [dossier's References section](./merge-algebra-verification.md#9-references) for the
cited literature (lattice theory / semilattices, CRDT convergence, order theory, and the
consensus/formal-methods foundations) with DOIs.
