# Merge-Algebra Determinism — Glossary & Literate Algorithms

A pedagogical companion to the [verification dossier](./merge-algebra-verification.md).
Part 1 defines every symbol, acronym, and key term **before it is used** elsewhere in the
merge-algebra documentation. Part 2 presents the two load-bearing algorithms — the keep-one
total order and the merged-state fold — in Knuth's literate-programming style (prose
interleaved with the code chunks it explains), with the invariants that make them correct.

Mathematical expressions use GitHub MathJax delimiters.

---

## 1. Glossary

### 1.1 Merge & deploy chains

| Term | Definition |
|---|---|
| **deploy** | A signed program/transaction submitted to the shard; the atom a block executes. |
| **deploy chain / `DeployChainIndex`** | A maximal chain of mutually-dependent deploys carrying their costs, event-log index, exact causal effects, and resulting `post_state_hash`; the minimal unit of conflict resolution and rejection in a multi-parent merge. |
| **chain identity** | The fields used together by `Eq`, `Hash`, and the terminal comparator tier: `deploys_with_cost`, `post_state_hash`, `source_block_hash`, `effect_indices`, and `has_exact_state_witness`. |
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
| **keep-one comparator `cmp`** | `DeployChainIndex::cmp`: a lexicographic total order with four priority tiers followed by the composite chain identity. `min_by` and `sort` use it to pick a unique merge winner and canonical diagnostic order. |
| **injective key** | A comparator key distinguishing all unequal values. Here it is the lexicographically compared composite chain identity, not any one hash or deploy signature. |
| **`Equal`-class ⊆ `Eq`** | `cmp a b = Equal ⟹ a = b`: the property that makes the order strict-total and the winner **unique** (no two distinct chains tie). |
| **`min_by` / `sort` winner** | The unique argmin/argmax the comparator selects; node-identical **iff** `cmp` is a strict total order. |
| **`ChannelChange<A>`** | A per-channel diff `(added, removed)` of multisets; the atom of a `StateChange` (`channel_change.rs`). |
| **`vec_union`** | Idempotent **max-multiset** union (keep `left`, append the `right` elements not already present): per element the multiplicity is `max(mult_left, mult_right)` (`channel_change.rs:25`). |
| **`cancel_common`** | Remove matched `added`/`removed` pairs — min-multiplicity cancellation (`channel_change.rs:34`). |
| **legacy `ChannelChange::combine`** | `vec_union(added)` + `vec_union(removed)` + inline `cancel_common`: commutative but **NON-associative** (Finding A). Retained for pairwise callers and the negative model; it is not the multi-survivor fold operator. |
| **`EffectFoldMode`** | The explicit projection boundary in `compute_merged_state`: `LegacyMaxUnion` for historical compatibility inputs or `ExactAdditive` for a complete exact-witness epoch. Mixed modes fail closed. |
| **`LegacyMaxUnionCollapseGuard`** | A compatibility diagnostic that detects equal non-mergeable content from distinct legacy survivors, where max-union loses causal multiplicity. It is inactive in exact mode because duplicate bytes from distinct identities are valid and additive. |
| **causal execution identity / `CausalEffectId`** | The pair `(source_block_hash, execution_index)`. It identifies one execution independently of its serialized output. |
| **execution state witness** | The pre-state and post-state roots surrounding one user or system execution. Replay authenticates both boundaries before the delta is eligible for merging. |
| **exact delta** | The state difference computed from one execution's witnessed roots and its event-log index. |
| **effect map** | A finite ordered map from causal execution identity to `(exact delta, number-channel delta)`. Equal repeats deduplicate; unequal repeats fail closed. |
| **`ChannelChange::additive_join`** | Concatenation of added and removed multisets. It is associative and commutative up to multiset equality and intentionally non-idempotent. |
| **`ChannelChange::canonicalized`** | Cancel common additions/removals once, then sort both sides into a byte-canonical diff. |
| **`StateChange::additive_join` / `normalized`** | The current content projection: add every unique causal effect's per-channel multiplicities, then normalize exactly once. |
| **netting** | The accumulation of per-channel diffs into one merged diff. |
| **conflict / `conflicts`** | The detector (`merging_logic.rs:262`): Check #1 same-IO-event race (`produces_consumed`/`consumes_produced` intersection, minus both-mergeable), Check #2 potential COMMs, Check #3 produce-touches-base-join. |
| **single-value cell** | A channel holding exactly one datum. A read-modify-write **consumes** the base datum and **produces** a new one (a consume-then-produce, keeping cardinality 1); a **produce-only write** produces *without* consuming the base, **over-filling** the cell to two data — which trips the RhoVM IntegerAdd single-value invariant at read (safety **S7**, the finality halt). |
| **produce-only write** | A write (a produce landing in `ChannelChange.added`) that does **not** consume the base datum (`removed = ∅`). On a single-value NUMBER cell it leaves `kept + added > 1` values — the §3c over-fill the consume-then-produce conflict check (`svc_update = consumes ∧ produces`) is **vacuous** for, since it requires a consume. |
| **§3c discriminator** | `check_single_value_cell_not_overfilled` (`rholang_merging_logic.rs:194`), wired on the **non-mergeable** else-path of the merge (`dag_merger.rs:1803-1819`). Rejects a merge whose post-state single-value NUMBER cell would hold `result_len = multiset_diff(base, removed).len() + added.len() > 1`. Non-numeric bases (registry / `TreeHashMap` leaves) are exempt — they merge freely. It is the discriminator that separates a purse/cell (conflict) from a registry (merge) among **disjoint-consumed / produce-only** writes, which the consume-then-produce check cannot distinguish. |
| **number / foldable channel** | A channel whose values fold (`MergeType::IntegerAdd`, `MergeType::BitmaskOr`), so concurrent updates never truly conflict; classified by `is_number_ch`; carried in `NumberChannelsDiff` (`merging_logic.rs:34`). |
| **`EventLogIndex`** | The per-chain index of produce/consume events; `EventLogIndex::combine` — the `combine` associated function (`event_log_index.rs:403`) — is field-wise **set union**. |
| **user / system split** | Deploys partitioned by `is_system_deploy_id` into a user and a system event-log index, then combined (`deploy_chain_index.rs:187-205`). |
| **`v0.4.16`** | The deployed release (`origin/master`) whose 4-key comparator the fix extends **append-only**. |

### 1.3 The theorem catalog

| ID | Meaning |
|---|---|
| **T-DET** | Determinism: identical merge inputs ⟹ identical `(pre-state hash, rejected-deploy set)` on every node. Its failure is a fork / finalization stall (safety **S1**). |
| **T-ORDER** | `cmp` is a **strict total order** whose `Equal`-class ⊆ `Eq` ⟹ the winner is **unique** (the core of T-DET). **Unconditional** — no `NoDup`/collision premise. |
| **T-KEEP1** | The greedy keep-one / rejection winner is a pure function of the branch **set** (consumer 1). |
| **T-WITNESS** | Replay-validated execution roots form a contiguous chain from block pre-state to block post-state, so every exact delta belongs to exactly one transition. |
| **T-CAUSAL** | Effect-map union is idempotent by causal identity and rejects one identity with unequal content. |
| **T-FOLD** | The projection of unique effects is permutation- and association-independent because distinct content multiplicities compose additively and normalize once. |
| **T-NET** | Additive multiset composition is a commutative monoid; max-union is retained as a counterexample because it collapses distinct equal outputs. |
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

**Why it is deterministic.** The comparator is a lexicographic tower whose terminal tier
is the **injective** composite `Eq`/`Hash` identity. The first four tiers determine economic
priority; the identity components resolve every residual tie. Therefore
`cmp(a, b) = Equal` holds exactly when the chains are equal, and no two distinct chains tie
(**T-ORDER**). A strict total order makes the
`min_by`/`sort` winner a pure function of the chain set — **T-KEEP1** — independent of the
`HashSet` iteration order.

⟨ *Compare two deploy chains `a`, `b`, returning `Ordering`.* ⟩
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
    k5 ← deploys_with_cost(a).cmp(deploys_with_cost(b))
    if k5 ≠ Equal: return k5
    k6 ← source_block_hash(a).cmp(source_block_hash(b))
    if k6 ≠ Equal: return k6
    k7 ← effect_indices(a).cmp(effect_indices(b))
    if k7 ≠ Equal: return k7
    return witness_mode(a).cmp(witness_mode(b))       ⟨ terminal composite Eq key ⟩
```

**Invariant (definiteness).** `$`\operatorname{cmp}(a,b)=\mathrm{Equal}`$` if and only if
the complete chain identities are equal. The priority tiers can only narrow; the composite
terminal tier decides every residual tie by the `Eq` identity itself. (Rocq
`KeepOneOrder.keep_one_total_order`, `keep_one_equal_impl_eq`, `output_indep_of_input_perm`,
`sort_argmax_unique` — all **unconditional**; Z3 `keep_one_total_order.py`, plus the dual
`sat` probe showing that without the injective terminal identity two distinct chains tie.)

**Rolling-upgrade safety.** Keys 1–4 are byte-identical to the deployed `v0.4.16`
comparator; the composite identity tier is appended. It is consulted
only on inputs where the 4-key order already returned `Equal` — inputs that were **already
`HashSet`-nondeterministic** on `v0.4.16` — so on every input the old order resolved
deterministically the result is unchanged (**T-APPEND**, safety **S6**).

### 2.2 The merged-state fold (`compute_merged_state`)

**Problem.** Fold surviving exact execution effects without counting one causal execution
twice, without collapsing two different executions that happen to emit the same bytes, and
without letting survivor order or grouping affect the result.

**Why it is deterministic.** First form an ordered effect map keyed by
`CausalEffectId`. Map insertion deduplicates repeated observation of the same effect and
requires equal content. Then fold the map values with additive multiset composition and
normalize once. Map-key idempotence prevents shared-history duplication; additive content
preserves RSpace multiplicity — **T-CAUSAL** and **T-FOLD**.

⟨ *Combine the resolved survivors' diffs in the canonical (total-order) order.* ⟩
```
survivors ← resolved.to_merge                       ⟨ branches surviving conflict resolution ⟩
sort survivors by compare_branches                  ⟨ :385 — bottoms out in DeployChainIndex::cmp ⟩
items ← []
for branch in survivors:                            ⟨ in the sorted branch order ⟩
    branch_items ← sort(branch)                     ⟨ :391 — items by cmp (the total order) ⟩
    items ← items ++ branch_items
effects ← empty ordered map
for item in items:
    require item has exact execution witnesses
    for effect in exact_effect_changes(item):
        if effects contains effect.identity:
            require effects[effect.identity] = effect.contribution
        else:
            effects[effect.identity] ← canonical(effect.contribution)
combined ← StateChange::empty()
for effect in effects.values():
    combined ← combined.additive_join(effect.state_change)
combined ← combined.normalized()
return apply_trie_actions(combined)
```

**Invariant.** Causal deduplication occurs before content composition. Moving
deduplication down to serialized data collapses independent messages; moving additive
composition above exact per-execution deltas duplicates a whole-block transition. Rocq
`ChannelNetting.channel_netting_exact_deterministic`, the Z3 channel-netting witness, and
the Rust additive and causal-identity tests cover the construction.

---

## 3. References

See the [dossier's References section](./merge-algebra-verification.md#10-references) for
the RSpace and rho-calculus sources that determine the content algebra. The causal identity,
wire-witness, activation, and validator-recompute rules are node consensus obligations
derived in this dossier rather than claims supplied by those publications.
