# Finalized-Floor Merge — Glossary & Literate Algorithms

A pedagogical companion to the
[verification dossier](./finalized-floor-verification.md). Part 1 defines every
symbol, acronym, and key term **before it is used** elsewhere in the finalized-floor
documentation. Part 2 presents the two load-bearing algorithms — the warm frontier
up-walk and the launder-free IntegerAdd combine — in Knuth's literate-programming
style (prose interleaved with the code chunks it explains), with the invariants that
make them correct.

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
| **causal clique certificate** | The original exact clique-oracle decision. A validator causally supports candidate `C` when `C` is in the all-parent DAG past of that validator's frozen latest message. A certificate proves that sufficient mutually agreeing causal support clears the configured exact fault-tolerance threshold. |
| **state-effect identity** | The consensus identity `E = (source_block_hash, execution_index)` of one successful user or system execution. The source hash prevents equal indices in different blocks from aliasing. |
| **active effect set `Active(B)`** | The successful execution identities retained by block `B`: its own successful effects plus the active effects of its maximal parents and finalized floor, minus `B.rejected_state_effects`. This is provenance of accepted transitions, not a snapshot of live tuples. |
| **state preservation** | `preserves(A,D)` holds when `A` is a DAG ancestor of `D` and $`Active(A) \subseteq Active(D)`$. An authorized later process may consume data produced by an earlier effect without erasing that effect's transition provenance. |
| **state-preserving support** | A validator causally supports `C` **and** the validator's frozen latest message preserves every active effect of `C`. A merge that names `C` as a parent but rejects one of `C`'s active effects is causal support, not state-preserving support. |
| **state-preserving clique certificate** | The same exact weighted maximum-clique calculation and threshold as the causal certificate, evaluated after restricting the committee to validators with state-preserving support. It is an additional certificate; it neither alters nor substitutes for the causal certificate. |
| **LFB admissibility** | The conjunction of a causal clique certificate, a state-preserving clique certificate, and preservation of the current LFB's active effects. Main-parent descent is not required: a multi-parent block may retain the LFB through a secondary parent or its explicit finalized-floor input. The predicate filters which causally certified block may replace the committed state pointer. |
| **rejection-candidate superset** | The rejected effect identities encountered in the descendant's height-bounded causal past. The implementation may scan a superset of identities that disappeared on paths from `A`; checking only candidates active at `A` makes unrelated rejections harmless and keeps the result exactly equivalent to $`Active(A) \subseteq Active(D)`$. |
| **stale-state descendant** | A block that is a DAG descendant of the current LFB but whose merge rejected at least one effect active at that LFB. It may remain valid and causally certified while being ineligible as the next LFB. |
| **rebase** | Recompute a successor from the certified floor instead of reusing a stale covering-parent post-state. The floor is an explicit state input, so accepted floor effects become active again and later LFB progress is possible. |
| **floor-rebased parent selection** | The proposer retains every valid validator latest message as causal fork-choice evidence, compacts direct parents only when another selected parent causally covers the removed tip, then computes state from the certified floor plus deterministic accepted deltas in the parent closure. State safety is a property of replay and promotion, not a filter over causal tips. |
| **LFB parent fallback** | The non-empty parent rule used only when no valid latest message remains: the proposed block declares the snapshot LFB itself as its sole parent instead of falling back to genesis. |

### 1.3 The floor and the merge

| Term | Definition |
|---|---|
| **`floor(B)`** | The **highest state-preserving sound candidate** over `just(B)`. It is selected from three block-structural sources: **inheritance** (each parent's own floor), **main-spine advancement** (each parent's highest dual-certified main-chain ancestor), and **universal certified advancement** (the highest dual-certified all-parent DAG ancestor). |
| **merge base** | `floor(B).post_state` — the state the merge folds parent writes onto. |
| **merge scope** | The unfinalized band `closure(parents) \ closure(floor)` — the blocks whose writes the merge must fold. |
| **`F(X)` (frontier)** | `parent_frontier(X, just(X))` — the highest witnessed-finalized block on `X`'s main spine over `X`'s **own** snapshot. A pure function of `X`; persisted as the warm-path **pivot**. |
| **universal certified frontier `U(B)`** | The highest candidate in the all-parent causal closure of `B` that is a DAG ancestor of every declared parent, holds both unchanged exact clique certificates over `just(B)`, and preserves every inherited parent floor. It closes the case where a committed state is secondary to every tip and therefore appears on no main-parent spine. |
| **coverage identity** | The deterministic index of one declared parent during the multi-source traversal for `U(B)`. A candidate is universal only after coverage from every identity reaches it. Strictly descending parent heights ensure no new coverage can arrive after a candidate is processed. |
| **latest-message coverage map** | For frozen latest messages `J`, the map whose value at candidate `C` is exactly $`\{v \mid C \preceq_{DAG} J(v)\}`$. It is computed by seeding validator identities at their latest blocks and propagating them once through the causal closure. It replaces repeated ancestry queries without changing the clique oracle's supporter set. |
| **snapshot provenance closure** | The recursive finalized-floor cache closure for the captured LFB, every frozen latest message, and every parent inspected by snapshot selection. It is completed before any state-preservation query. Concurrent finalizer writes commute because materialization only adds the same canonical entries. |
| **linear-snapshot reuse** | The bounded optimization that omits a repeated universal scan across one linear edge only when the parent has one predecessor, its cached floor is the inherited floor, the frozen snapshot is identical, and every latest message predates the parent. Multi-parent merges do not qualify because they can make branch-local candidates universal. |
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
| **T-EFFECT-PROVENANCE** | Exact active effects follow the accepted-input union-minus-rejections recurrence; parent order and redundant covered parents are irrelevant, direct rejection removes only its named identity, and the complete candidate scan decides preservation exactly (safety **S28**). |
| **T-STATE-PARENT** | Every valid latest message remains a causal parent, an empty valid-tip set selects the LFB, and the resulting merge state preserves the certified floor even when a parent delta does not (safety **S29**). |
| **T-CERTIFIED-FLOOR-PROMOTION** | Complete all-parent causal discovery promotes the highest dual-certified universal state floor independently of parent order; restricting discovery to main-parent spines starves an off-main committed state. |
| **T-COVERAGE-TRANSPARENCY** | Propagated latest-message coverage equals pairwise DAG ancestry for every candidate and validator; therefore supporter filtering and the existing exact clique decision are unchanged. Linear-snapshot reuse preserves the eligible ancestor set only under its one-predecessor and older-snapshot premises. |
| **T-SNAPSHOT-MATERIALIZATION** | Materializing the complete snapshot target set makes every required provenance entry available before selection; repeated materialization is idempotent, arbitrary snapshot/finalizer interleavings commute, and the parent-only control is incomplete. |
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

### 2.3 Certified-candidate LFB admission

**Problem.** A multi-parent block can be a main-chain descendant of the current
LFB while its replay state was derived from an older floor. Its clique certificate
is still valid, but promoting it would erase a committed state transition.

**Separation of concerns.** `causal_certified` remains the original exact
majority/clique calculation. `state_certified` applies that same calculation to
the subset whose latest states preserve the candidate's active effects.
`state_preserved` then checks that the candidate also preserves every effect
active at the current committed LFB. The second
certificate does not rewrite the causal voters or their result: it proves the
additional proposition needed before a block becomes a replay floor.

⟨ *Evaluate the complete, deterministically ordered frozen candidate set.* ⟩
```
for candidate in ordered_candidates:
    causal_certified, fault_tolerance ← exact_clique_decision(candidate, frozen_snapshot)
    if not causal_certified:
        continue
    materialize_finalized_floor(candidate, frozen_snapshot)
    state_support ← validators whose latest states preserve Active(candidate)
    state_certified ← exact_clique_decision(candidate, state_support)
    if not state_certified:
        continue
    if not state_preserved(current_lfb, candidate):
        continue
    install_lfb(candidate, fault_tolerance)
    return candidate
return none
```

Two candidates are intentionally skipped. A stale candidate can have both
certificates yet fail current-LFB effect preservation. A rejected-parent candidate can
preserve the current LFB and retain its causal certificate while failing the
state-preserving certificate because the apparent majority merged, but did not
retain, its effects. When a later proposal sees the advanced floor, the
covering-parent fast path is permitted only if that parent already preserves the
floor; otherwise replay starts from the floor. The resulting rebase becomes
admissible after both exact certificates and current-LFB effect preservation hold.

### 2.4 State-preserving parent selection

**Problem.** Finalizer admission alone does not constrain the next proposal. A
node may advance its LFB while its latest-message map still contains an older tip
or a causal descendant whose merge rejected an effect of that LFB. Feeding those
tips back into the estimator lets an honest proposer build from a state that has
already been excluded from committed-state progression.

**Snapshot rule.** Let `L` be the LFB read from the same immutable DAG
representation as the latest-message map. Parent eligibility is an exact
state-provenance query. An error aborts snapshot construction rather than silently
shrinking the voting input.

```text
eligible := empty validator-to-block map
for each (validator, latest) in valid_latest_messages:
    preserved := preserves(L, latest)
    if preserved returned an error:
        fail snapshot construction
    if latest = L or preserved:
        eligible[validator] := latest

parent_hashes := distinct values of eligible
if parent_hashes is empty:
    parent_hashes := {L}

ghost_main_parent := estimate_tip(eligible)
declared_parents := deterministically order parent_hashes around ghost_main_parent
```

The complete latest-message map still supplies block justifications, and all
valid latest metadata still supplies sequence-number accounting. Validators
replay `declared_parents`; they do not apply their local LFB as an additional
validity predicate. This separation preserves asynchronous block validity while
ensuring every proposal made after local finalization starts from state that
retains that finalization.

### 2.5 Universal certified-floor promotion

**Problem.** The finalizer certifies over the complete causal DAG, but the original
per-block floor advancement enumerated only each declared parent's main-parent
spine. A block can therefore hold both exact certificates and be a secondary
ancestor of every tip while remaining invisible to every main-spine frontier.
The finalizer may install that block as the LFB, yet subsequent block floors remain
at genesis. Replay then repeatedly rebuilds above an obsolete state cut.

**Traversal rule.** Give every declared parent a distinct coverage identity and
walk the union of their causal pasts in descending block-number and hash order.
Coverage is a set, so duplicate edges and reconvergence cannot change the result.
Because every valid parent edge strictly decreases block number, all paths from a
higher root reach a candidate before that candidate is removed from the queue.

```text
queue := every declared parent, ordered by descending (block_number, hash)
coverage[parent_i] := {i}

while queue is not empty:
    candidate := remove highest queued block
    if coverage[candidate] contains every parent identity:
        if causal_certificate(candidate, frozen_justifications)
           and state_certificate(candidate, frozen_justifications)
           and candidate preserves every inherited floor:
            return candidate

    for each causal_parent of candidate:
        require block_number(causal_parent) < block_number(candidate)
        require causal_parent has not already been processed
        coverage[causal_parent] := coverage[causal_parent] union coverage[candidate]
        queue causal_parent by descending (block_number, hash)

return no universal candidate
```

The first eligible universal candidate is maximal because every queued block of
higher height, and every same-height block with a higher hash, has already been
examined. The two certificate predicates call the existing exact clique oracle;
the traversal neither changes agreement propagation nor introduces a second
finalizer. The returned candidate joins inherited and per-spine candidates before
the existing sound-base selection, so Case-A/Case-B safety and deterministic error
handling remain unchanged.

The causal certificate needs the same ancestry relation for every validator's
frozen latest message. Repeating a storage-backed ancestry walk for every
candidate-validator pair is extensionally correct but unnecessarily expensive.
The optimized pass transposes that relation without approximating it:

```text
latest_queue := every distinct J(v), ordered by descending (block_number, hash)
latest_coverage[J(v)] := latest_coverage[J(v)] union {v}

while latest_queue is not empty:
    block := remove highest queued block
    for each causal_parent of block:
        require block_number(causal_parent) < block_number(block)
        require causal_parent has not already been processed
        latest_coverage[causal_parent] :=
            latest_coverage[causal_parent] union latest_coverage[block]
        queue causal_parent by descending (block_number, hash)

supporters(candidate) :=
    corresponding_weight_map(candidate)
        restricted to latest_coverage[candidate]
```

At completion, validator `v` appears in `latest_coverage[C]` exactly when a
causal path runs from `C` to `J(v)`. The supporter map is therefore byte-for-byte
the map obtained from pairwise ancestry checks. The maximum-clique search, hard
majority, exact threshold, and strictness flag receive the same inputs.

A second optimization may reuse a result across an unchanged linear snapshot:

```text
reuse := child has exactly one parent P
         and P has exactly one predecessor Q
         and inherited_floor(child) = cached_floor(P)
         and just(child) = just(P)
         and every J(v) has block_number less than block_number(P)

if not reuse:
    run the complete universal traversal
```

The height premise prevents `P` from being newly certified by its own older
snapshot. Every other ancestor of `P` is already an ancestor of `Q`, so it was
eligible during `P`'s derivation. This reasoning does not extend to a multi-parent
`P`: creating `P` can join branches and make a previously branch-local candidate
universal. Such a child always performs the complete scan.

---

## 3. References

See the [dossier's References section](./finalized-floor-verification.md#references)
for the cited literature (CBC Casper, clique-based finality, IEEE-754) with DOIs.
