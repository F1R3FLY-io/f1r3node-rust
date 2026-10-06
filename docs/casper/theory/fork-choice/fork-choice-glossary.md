# Fork-Choice ("Ghosting") — Glossary & Literate Algorithms

A pedagogical companion to the [verification dossier](./fork-choice-verification.md).
Part 1 defines every symbol, acronym, and key term **before it is used** elsewhere in
the fork-choice documentation. Part 2 presents the two load-bearing algorithms — the
score accumulation and the heaviest-subtree ranking — in Knuth's literate-programming
style (prose interleaved with the code chunks it explains), with the invariants that
make them correct.

All mathematical expressions use unicode and are quoted in backticks.

---

## 1. Glossary

### 1.1 DAG & messages

| Term | Definition |
|---|---|
| **block / message** | A signed node in the block DAG; carries parents, a validator (sender), and justifications. |
| **tip** | A block with no children in the current view — a candidate head of the chain. |
| **`parents`, main parent** | A block's parent blocks; `parents[0]` is the **main parent** (spine predecessor). |
| **child** | `c` is a child of `b` iff `b ∈ parents(c)`. |
| **`num(b)`** | Block number (height); `num(genesis)=0`, `num(child)=num(main parent)+1`. |
| **ancestor `b ⪯ p`** | `b` is reachable from `p` by walking parents (reflexive-transitive); "`b` supports `p`". |
| **latest message `lm(v)`** | Validator `v`'s most recent block, from the frozen `latest_messages` snapshot. |
| **LCA / LUCA** | Lowest (universal) common ancestor of the latest messages — the base the scoring is measured from. |
| **`LATEST_MESSAGE_MAX_DEPTH`** | `= 1000` — latest messages more than this far below the top height are filtered out before the LCA, bounding the scored band. |

### 1.2 Weights, scores, ranking

| Term | Definition |
|---|---|
| **committee / bonds** | The bonded validators with weights, read from a block's main-parent on-chain `weight_map` — a block-structural fact (identical on every node). |
| **`w(v, b)`** | Validator `v`'s bonded weight as seen at block `b` (from `b`'s main-parent weight map); an exact `i64 ≥ 0`. |
| **score `score(b)`** | `score(b) = Σ { w(v, b) : v ∈ V, b ⪯ lm(v) }` — the cumulative weight of validators whose latest message supports `b`. A sum ⟹ order-independent. |
| **invalid / slashed filter** | Latest messages of slashed/invalid validators are removed before scoring (the **T-10** property) so they add zero weight. |
| **heaviest subtree (GHOST)** | At each level, the child whose subtree has the maximum cumulative score; the ranking descends into it. |
| **tie-break** | The total order on tips: **score descending, then block-hash ascending** (`sort_by_with_decreasing_order`). Makes the ranked head unique. |
| **`max_number_of_parents`** | Cap on returned tips; the head is always kept. Sentinels: estimator `i32::MAX`, config `-1` = unlimited. |
| **`max_parent_depth`** | Secondary parents deeper than this below the main tip are dropped. |

### 1.3 The theorem catalog

| ID | Meaning |
|---|---|
| **T-DET** | Determinism: identical `(DAG, latest_messages)` ⟹ identical `(tips, lca, main_parent)` on every node. Its failure is a fork (safety **S1**). |
| **T-TOTAL** | The tie-break is a strict total order on distinct hashes ⟹ the ranked argmax is unique (the core of T-DET). |
| **T-GHOST** | The head lands in the heaviest subtree at every fork of the scored main-parent tree. |
| **T-SCORE** | Score accumulation is additive/associative/commutative ⟹ order-independent (the `HashMap` iteration order cannot change a score). |
| **T-FILTER** | Slashed/invalid latest messages contribute zero weight (reuses slashing T-10). |
| **T-TERM** | The descent terminates (each step commits to a strictly higher-numbered child, bounded by DAG height). |
| **T-LCA** | The LCA is a genuine common ancestor; the depth filter is deterministic. |
| **T-BOUND** | Count/depth truncations keep the head, never panic, node-deterministic. |
| **T-MP** | Main-parent selection is deterministic and consistent with the ranking (proposer-side). |
| **T-VALID (reframed)** | An honest proposer's parents pass `Validate::parents` (bound-consistency); validators do **not** recompute fork choice. |

---

## 2. Literate algorithms

### 2.1 Score accumulation (`build_scores_map`)

**Problem.** Give each block a weight-score so the ranking can pick the heaviest
subtree. The score of a block is the total bonded weight of the validators whose latest
message sits at or above it.

**Why it is deterministic.** The score is a **sum**, and `i64` addition is associative
and commutative, so accumulating over the validators in *any* order (the `HashMap`
iteration order) yields the same total — **T-SCORE**. Weights are read from the
block-structural bonds (the main-parent weight map), never a node-local view, so `w(v,b)`
is identical on every node — **T-DET(b)**. Slashed validators are filtered first
(**T-FILTER**), so they add nothing.

⟨ *For each surviving validator, walk its supporting chain down to the LCA, adding its
weight to every block on the way.* ⟩
```
scores ← empty map (block → i64, default 0)
for (v, lm) in filtered_latest_messages:        ⟨ order-independent: sum is commutative ⟩
    frontier ← { lm }                            ⟨ BFS from the validator's latest block ⟩
    visited  ← {}
    while frontier nonempty:
        b ← take from frontier
        if b already in visited: continue        ⟨ dedup ⟹ each block counted once per v ⟩
        visited ← visited ∪ { b }
        scores[b] ← checked_add(scores[b], w(v, b))   ⟨ B3: fail loudly on overflow ⟩
        for p in parents(b) with num(p) ≥ num(LCA):   ⟨ bounded by the LCA height ⟩
            frontier ← frontier ∪ { p }
return scores
```

The `checked_add` (fix **B3**) can only reject if the cumulative weight exceeds
`i64::MAX` — a supply-cap violation, i.e. an already-invalid state. (Rocq
`Score.score_perm_invariant`, `score_eq_support_sum`; Z3 `score_supply_cap_bitvec.py`.)

### 2.2 Heaviest-subtree descent (`rank_forkchoices`)

**Problem.** From the scored main-parent tree, pick the canonical tip: at each fork,
commit to the heaviest scored child before descending further.

**Why it is deterministic and terminates.** At each fork the scored main-parent
children are compared under the **total order** `(score desc, hash asc)`; on distinct
hashes the maximum is **unique**, so the step never depends on iteration order —
**T-TOTAL ⟹ T-DET(a)**. Each step moves to a strictly higher-numbered block (a
child), and block numbers are bounded by the DAG height — **T-TERM**. Only
MAIN-parent children are followed: a merge is a main-parent child of exactly one of
its parents, so weight flows up exactly one chain and same-height siblings stay
mutually exclusive.

⟨ *Descend from the LCA into the heaviest scored main-parent child; stop at the
frontier; rank the remaining frontier tips behind the head.* ⟩
```
head ← lca
loop:
    children ← { c ∈ children(head) : c ∈ scores and main_parent(c) = head }
    if children is empty: break                    ⟨ head is a latest-message tip ⟩
    head ← argmax(children, key = (score desc, hash asc))   ⟨ unique: TOTAL order ⟩

frontier ← dedup { lm ∈ latest_messages : lm ≠ head and no scored main child }
tail ← sort_by(frontier, key = (score desc, hash asc))
return [ head ] ++ tail
```

Ranking the TIPS by their own scores instead is not GHOST: a tip's own score is only
its owner's weight, so under concurrent proposal every tip ties and the head falls to
hash order — the spine then abandons majority branches (the ucc-i6 production
divergence; see `tests/fork_choice/heaviest_subtree_descent.rs`). A latest message
that HAS a scored main-parent child is a superseded ancestor of another tip on its own
chain and is excluded from the tail. (Rust `heaviest_subtree_descent.rs`,
`prop_ghost_argmax.rs`; Rocq `TieBreak.sort_total_order`,
`output_indep_of_input_perm`; TLA⁺ `ForkChoice.tla`; Z3 `tiebreak_total_order.py`.
`Rank.v`'s correspondence is pending re-derivation against the descent.)

---

## 3. References

See the [dossier's References section](./fork-choice-verification.md#references) for the
cited literature (GHOST, Gasper, Casper FFG, and the consensus/BFT/formal-methods
foundations) with DOIs.
