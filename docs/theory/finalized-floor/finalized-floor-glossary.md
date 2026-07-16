# Finalized-Floor Merge — Glossary & Literate Algorithms

A pedagogical companion to the
[verification dossier](./finalized-floor-verification.md). Part 1 defines every
symbol, acronym, and key term **before it is used** elsewhere in the finalized-floor
documentation. Part 2 presents the two load-bearing algorithms — the warm frontier
up-walk and the launder-free IntegerAdd combine — in Knuth's literate-programming
style (prose interleaved with the code chunks it explains), with the invariants that
make them correct.

All mathematical expressions use unicode and are quoted in backticks.

---

## 1. Glossary

### 1.1 DAG & block structure

| Term | Definition |
|---|---|
| **block `B`** | A signed message in the block DAG; carries parents, a validator (sender), and signed *justifications* (each validator's latest block `B` saw). |
| **parents `P₁…Pₖ`** | The blocks `B` merges. `parents[0]` is the **main parent** (the spine predecessor). |
| **main-parent spine** | The chain `B → main_parent(B) → … → genesis` obtained by following `parents[0]` repeatedly. |
| **`num(B)`** | Block number (height) — `num(genesis) = 0`, `num(child) = num(main_parent)+1`. |
| **`anc_of d a b`** | `a` is a (general) DAG-ancestor of `b`: reachable by walking *all* parents of `b` down. `is_dag_ancestor(a,b)` in Rust; `anc_of` in Rocq. |
| **genesis** | The root block (no parents); **finalized by definition**. |
| **`just(B)`** | `B`'s frozen **justification snapshot** — the `validator → latest-block` map packaged into `B`. The *only* per-block finalization input; never the live DAG view. This is what makes the floor node-deterministic. |
| **snapshot order `J' ⊇ J`** | `J'` extends `J`: for every validator, `J'`'s latest block is `J`'s or a descendant (`snap_extends` in Rocq). |

### 1.2 Finalization & the clique oracle

| Term | Definition |
|---|---|
| **committee** | The bonded validators with their weights — a `WeightMap`. For block `B`, `get_corresponding_weight_map(B) = weight_map(main_parent(B))` (the bonds active for `B`). |
| **`θ` (ft_threshold)** | The finalization threshold; a block is finalized when its fault-tolerance ratio is `≥ θ`. |
| **`ft_witnessed(C, J)`** | The clique oracle's normalized fault tolerance of block `C` over snapshot `J`: `ft = (2q − S)/S`, where `S = Σ committee weights` and `q =` max-clique agreeing weight. `C` is **finalized** over `J` iff `ft_witnessed(C,J) ≥ θ`. (`clique_oracle.rs`; Rocq `CliqueOracle.Finalized`.) |
| **quorum** | A majority-weight sub-committee that mutually agree on `C` (a clique). The oracle finalizes `C` when a quorum witnesses it. |
| **`Finalized c J b`** | Rocq abstraction: *some majority-weight sub-committee `c` all agree on `b` over `J`* — a faithful monotone abstraction of `ft_witnessed ≥ θ`. |

### 1.3 The floor and the merge

| Term | Definition |
|---|---|
| **`floor(B)`** | The **highest** ancestor of `B`'s parents that the oracle certifies finalized over `just(B)`. The max of two candidate sources: **inheritance** (each parent's own floor) and **advancement** (each parent's highest witnessed-finalized main-chain ancestor). |
| **merge base** | `floor(B).post_state` — the state the merge folds parent writes onto. |
| **merge scope** | The unfinalized band `closure(parents) \ closure(floor)` — the blocks whose writes the merge must fold. |
| **`F(X)` (frontier)** | `parent_frontier(X, just(X))` — the highest witnessed-finalized block on `X`'s main spine over `X`'s **own** snapshot. A pure function of `X`; persisted as the warm-path **pivot**. |
| **pivot** | The cached `F(parent)` an incremental (warm) walk starts from. |
| **`Δ` (floor distance)** | `Δ = num(maxParent) − num(floor)` — the finalization lag. The deterministic quantity the backstop keys on. |
| **BitmaskOr** | A number-channel merge tag: combine by bitwise OR (a **semilattice** — idempotent, commutative, associative; keeps every set bit). |
| **IntegerAdd** | A number-channel merge tag: combine by addition in the **wrapping group** `ℤ/2⁶⁴`; the terminal apply is guarded by `checked_add` + `≥0` (vault balances are non-negative). |

### 1.4 The determinism lemmas & theorem catalog

| ID | Meaning |
|---|---|
| **L-ANC** | *Ancestor-monotone finalization*: `Finalized(C,J) ∧ anc_of C' C ⟹ Finalized(C',J)`. The same quorum that finalizes `C` finalizes every ancestor. ⟹ finalized blocks are a **downward-closed prefix** of the spine. |
| **L-SNAP** | *Snapshot-monotone finalization*: `J' ⊇ J ∧ Finalized(C,J) ⟹ Finalized(C,J')`. A larger snapshot only ever finalizes more. |
| **AdjDC** | *Adjacent downward-closure* of the finalization flags along a band — the exact hypothesis `Floor.frontier_cache_transparent` needs. `GuardBridge.chain_adj_AdjDC` **derives** it from L-ANC under a constant committee. |
| **T-TERM** | The spine walk terminates (reaches genesis). |
| **T-CACHE** | Warm up-walk result == cold down-walk result (the frontier cache is transparent ⟹ no fork, safety **S1**). |
| **T-MONO** | The floor never regresses along ancestry (safety **S2**). |
| **T-FIN** | The chosen floor is finalized. |
| **T-SOUND** | The chosen merge base is a sound common base; `None ⟹` explicit `Err` (safety **S4**). |
| **T-LIN** | A Case-A base is a common DAG-ancestor of every parent (one chain). |
| **T-K1** | No mergeable write is lost — *the ~400-block bug* (safety **S5**). |
| **T-NDA** | Recovery never double-applies an effect. |
| **T-DETMERGE / T-CONV** | The merge is order-independent ⟹ no fork from fold order (safety **S6**). |
| **T-ALG** | Fold laws — BitmaskOr semilattice; IntegerAdd wrapping group + checked apply (safety **S7**). |
| **T-PS** | Safety holds for *any* parent list (unconstrained oracle). |
| **T-COMM** | The committee is `bonds_of(floor)`, a pure function of the floor (safety **S8**). |
| **Case-A / Case-B** | The two sound-base cases in `derive_floor`: **A** = candidate is a general ancestor of every parent; **B** = every *other* candidate is compatible (in the candidate's past, or mergeable via a common-descendant parent). |

---

## 2. Literate algorithms

### 2.1 The warm frontier up-walk (`incremental_frontier`)

**Problem.** Resolving `parent`'s frontier over the child's snapshot `J` by a cold
top-down walk costs `Θ(Δ·V)` oracle calls per parent (`Θ(Δ²·V)` cumulatively as `Δ`
grows) — the driver of the ~400-block ratchet. The warm walk collapses this to
**amortized `O(1)`** oracle calls by starting from the cached pivot `F(parent)` and
walking *up*.

**Why it is correct (and never forks).** Two lemmas do the work. By **L-SNAP**, the
pivot — finalized over `parent`'s own snapshot — is still finalized over the larger
child snapshot `J ⊇ just(parent)`, so it remains a valid lower bound. By **L-ANC**,
under a *constant committee* the finalized blocks on the spine form a **downward-closed
prefix**, so "highest finalized" is well-defined and the up-walk may stop at the first
non-finalized block. Both premises are *guarded* at runtime (and the L-ANC premise is
`GuardBridge.chain_adj_AdjDC` in Rocq); if either fails, we fall back to the cold walk,
which yields the identical result — so the cache is transparent (**T-CACHE**).

⟨ *Collect the band `[parent … pivot]` with cheap `main_parent` hops — no oracle calls.* ⟩
```
spine ← [parent]
spine ← spine ++ main_parent_chain(parent, stop_at = num(pivot))
if last(spine) ≠ pivot: return None          ⟨ pivot off parent's spine ⟹ cold ⟩
```

⟨ *L-ANC guard: the committee must be constant across the band.* ⟩
```
pivot_committee ← get_corresponding_weight_map(pivot)
for block in spine:
    if get_corresponding_weight_map(block) ≠ pivot_committee:
        emit metric floor_incremental_guard_fallback
        return None                          ⟨ committee changed ⟹ cold ⟩
```

⟨ *L-SNAP guard: the pivot must still finalize over the larger snapshot `J`.* ⟩
```
if ft_witnessed(pivot, J) < θ:
    emit metric floor_incremental_guard_fallback
    return None                              ⟨ L-SNAP premise failed ⟹ cold ⟩
```

⟨ *Up-walk: advance from just above the pivot toward `parent` while finalized.* ⟩
```
best ← pivot
for candidate in reverse(spine without parent-to-pivot already covered):
    if ft_witnessed(candidate, J) ≥ θ:       ⟨ only these are oracle calls: O(advance) ⟩
        best ← candidate
    else:
        break                                ⟨ downward-closed ⟹ first miss ends it ⟩
return Some(best)
```

The `advance` counts telescope across successive merges to the spine length, so the
amortized per-merge oracle cost is `O(1)`. (`casper/src/rust/finality/floor.rs`;
Rocq `Floor.frontier_cache_transparent` + `GuardBridge.guard_constant_committee_transparent`.)

### 2.2 The launder-free IntegerAdd combine (`combine_mergeable_value` / `checked_combine`)

**Problem.** A branch contributes several IntegerAdd diffs to one number channel.
Folding them with `wrapping_add` (silent) could produce a sum that wraps to a
**non-negative** value — e.g. `i64::MAX + i64::MAX + 2 ≡ 0 (mod 2⁶⁴)` — which then
passes the apply-time `≥0` gate carrying a *wrong* value (the launder).

**Fix — fold with `checked_add`, rejecting the branch on any overflow.** This is the
`checked_combine` modeled in Rocq (`IntegerAdd.checked_combine`), proven launder-free
(`checked_combine_sound`: an accepted result equals the true sum and is in range).

⟨ *Fold a branch's IntegerAdd diffs; `None` ⟹ reject the branch (fail loudly).* ⟩
```
acc ← 0
for diff in branch_diffs(channel):
    acc ← checked_add(acc, diff)             ⟨ i64 checked addition ⟩
    if acc = None: return None               ⟨ overflow ⟹ branch rejected, never wrapped ⟩
return Some(acc)                             ⟨ acc = true Σ diffs, in range ⟩
```

**Terminal apply — the consensus-state write** (`calculate_number_channel_merge`)
repeats the guard as defense-in-depth: `base + Σdiffs` via `checked_add`, then reject
unless the result is `≥ 0` (vault balances are non-negative). For `base ≥ 0` a positive
overflow always wraps *negative*, so the `≥0` check catches both overflow and negative.

**The per-deploy diff stays wrapping — on purpose.** `calculate_num_channel_diff`
computes `diff = end − prev` with `wrapping_sub`, the exact **group inverse** of the
wrapping add that language-level execution used to produce `end`. It therefore
recovers the deploy's *true intended delta* even when execution overflowed and stored
a wrapped `end`. Overflow is caught downstream (combine + apply), **never at the diff**
— a `checked_sub` there would crash the live block-execution path on a deploy that
must instead be gracefully rejected at merge time. (Rocq: `wadd`/`wsum` model the
wrapping group; `checked_apply` models the terminal apply. Regression:
`diff_integer_add_recovers_wrapped_delta`.)

---

## 3. References

See the [dossier's References section](./finalized-floor-verification.md#references)
for the cited literature (CBC Casper, clique-based finality, IEEE-754) with DOIs.
