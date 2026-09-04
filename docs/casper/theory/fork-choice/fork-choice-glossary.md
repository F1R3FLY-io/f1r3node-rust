# Fork-choice glossary and algorithms

This companion to the [normative specification](./fork-choice-specification.md) defines
the vocabulary used by the fork-choice implementation and verification artifacts.

## 1. Blocks, closure, and floors

| Term | Definition |
|---|---|
| block | A signed DAG vertex carrying ordered parents, sender, sequence number, justifications, protocol version, state roots, and deploy results. |
| main parent | The first declared parent. It is the LMD-GHOST head when the vote projection is nonempty, or the captured finalized floor when no vote is eligible. No deploy policy may replace it. |
| secondary parent | Any declared parent after index zero. Secondary states are merged into the main-parent state. |
| ancestor | `a` is an ancestor of `b` when a parent-path from `b` reaches `a`; ancestry is reflexive. |
| dependency closure | The immutable blocks reachable through every consensus dependency edge required to certify a candidate. Local indices outside this closure cannot change the candidate verdict. |
| finalized floor | The highest certified state that all subsequent round calculations must preserve. |
| floor authority | The exact active validators, positive stakes, and bond generations committed by the finalized floor's replayed state. |

## 2. Certified consensus context

| Term | Definition |
|---|---|
| exact latest-message slot | The one signed latest-message pointer carried for an active floor validator. A complete round has exactly one slot per active validator. |
| objective evidence | A canonical, generation-scoped pair of conflicting signed messages proving equivocation independently of receiver arrival order. |
| causal-parent projection `C` | Exact slots that pass authority, positive stake, exact-hash, sender, generation, objective-evidence, and objective-admission checks. These are state dependencies even when they do not descend from the current finalized floor. |
| finality-vote projection `V` | The subset of `C` whose cited block equals or descends from the captured finalized floor. Only `V` contributes stake to LMD-GHOST and finality. |
| exclusion | The stable reason an exact slot is not eligible. Exclusions are part of the context digest. |
| certified context | The floor identity and post-state, frozen authority, exact slots, evidence, eligible projection, exclusions, and canonical digest used by every consumer in a round. |
| context extensionality | Equal context digests plus equal cited DAG closures imply equal LCA, scores, ranking, and head, regardless of receiver-local indices. |

The context separates three facts that older code conflated:

1. a signed validator message is immutable;
2. a receiver may have mutable diagnostic knowledge about it; and
3. only certified objective facts may affect consensus.

## 3. LMD-GHOST terms

| Term | Definition |
|---|---|
| latest message `lm(v)` | The eligible message selected from validator `v`'s exact slot. |
| LCA or LUCA | The lowest universal common ancestor of all eligible latest messages. |
| frozen stake `A(v)` | Validator `v`'s positive stake at the incoming finalized floor. It is constant for the entire round. |
| support | Block `b` is supported by `v` when `b` is in the ancestry of `lm(v)` down to the LCA. |
| score | The sum of frozen stakes of validators supporting a block. |
| GHOST | Greedy Heaviest-Observed Sub-Tree: repeatedly choose scored children in descending score order. |
| greedy GHOST head | The terminal block reached by choosing the unique highest-ranked scored child at every level from the LCA. |
| scored terminal | A scored block with no scored child. |
| terminal frontier | The exact duplicate-free set of scored terminals reachable from the LCA. A shared child in a multi-parent diamond appears once. |
| two-lane ranking | The composition of the greedy GHOST head with the independently enumerated terminal frontier: the head is first, followed by every other terminal in total score/hash order. |
| total tie-break | Score descending, then block hash ascending. It makes every ranked position unique. |
| candidate-bond noninterference | An unfinalized block's bond cache cannot change the frozen authority or score of its round. |
| receiver-state noninterference | Local invalid sets, LMM indices, finalized flags, ambient height, and unrelated blocks cannot change a certified result. |

The score equation is:

```math
score(b) = \sum_{v : b \preceq lm(v)} A(v)
```

## 4. Parent-bound terms

| Term | Definition |
|---|---|
| ranked head | Index zero after policy ordering. Depth and count bounds must preserve it. |
| causal-tip antichain | The reachability-maximal, duplicate-free set covering every retained causal tip. An ancestor is removed only when another retained parent already covers it. |
| finalized-floor backstop | The captured floor inserted when no causal-parent candidate descends from it. It makes the proposal's replay base explicit and is also an evidence-closure root. |
| live causal tip | A causal tip still covered after deterministic parent-depth expiry. Expired tips remain exact evidence roots but no longer block current proposal construction. |
| maximum candidate height `H` | The greatest block height in the full ranked input before depth or count truncation. |
| depth horizon `D` | Maximum permitted value of `H - height(p)` for a secondary parent. |
| depth buffer | Receive-side and history-retention safety margin added to `D`. |
| unlimited count | Exactly configuration value `-1`, or the estimator's `i32::MAX` sentinel. |
| exact frontier-capacity rule | A finite proposal-parent cap admits a frozen proposal snapshot exactly when the complete depth-expired, reachability-maximal frontier, including any required floor backstop, fits without truncation. |
| worst-case capacity advisory | `number-of-active-validators + 1` is sufficient to carry one distinct tip per configured validator slot plus an independent floor backstop. It is a provisioning warning boundary, not a necessary runtime admission condition. |

For `R = [g] ++ S`, finite-depth filtering returns the head followed by exactly the
eligible tail:

```math
F_D(R) = [g] \mathbin{++} [p \in S \mid H - height(p) \le D]
```

The approved genesis may appear as a receiver-side tail exception because its state is
universally retained. Every parent, including the head and genesis, must still resolve
successfully; storage failure is local infrastructure failure, not objective block
invalidity.

## 5. Score construction algorithm

The estimator receives a complete certified context and never consults local vote
eligibility indices.

```text
require complete exact slots
eligible := context.eligible_latest_messages
lca := LUCA(all values of eligible), or approved genesis when eligible is empty

parallel for each (validator, latest) in eligible:
    stake := context.frozen_authority[validator]
    walk every parent path from latest down to lca
    emit (block, stake) once per visited block

reduce emitted contributions in deterministic key order using checked addition
```

Parallel traversal is safe because workers only read the DAG and immutable context.
Their output is a per-validator map; shared score mutation is deferred to the ordered
reduction.

## 6. Two-lane ranking algorithm

```text
ghost := lca
while scored_children(ghost) is not empty:
    require every scored child height > height(ghost)
    ghost := first sort(scored_children(ghost), score descending, hash ascending)

frontier := {lca}
while frontier contains a block with scored children:
    choose any such block
    require every scored child height > height(block)
    frontier := (frontier - {block}) union scored_children(block)

require ghost in frontier
tail := sort(frontier - {ghost}, score descending, hash ascending)
return [ghost] ++ tail
```

Every nonterminal replacement advances to strictly higher block numbers, so both
finite-DAG traversals terminate. Set union deduplicates shared multi-parent children.
The exact terminal frontier is independent of expansion order, and the total order
makes its tail byte-identical across validators.

The head prefix is essential. With branch scores `60` and `40`, where the score-`60`
branch terminates in two score-`30` leaves, GHOST selects a score-`30` descendant of
the score-`60` branch. Globally sorting leaves would incorrectly select the score-`40`
leaf and implement a different protocol.

## 7. Verification vocabulary

| Artifact | Role |
|---|---|
| Rocq | Axiom-free structural proofs: complete slots, floor projection, frozen weights, greedy descent, exact terminal-frontier composition, total tail ranking, LCA, head-preserving bounds, and proposer-to-receiver bridge. |
| TLA+ and TLC | Concurrent replica/interleaving model plus explicit negative controls for local-state projection, global-terminal head selection, incomplete slots, outside-floor votes, mutable weights, and head loss. |
| Apalache | Symbolic bounded checking of the same transition invariants. |
| Z3 and Sage | Independent arithmetic and total-order witnesses. |
| Wolfram Language | Independent greedy-GHOST, asynchronous frontier confluence, unsafe global-leaf counterexample, termination, frozen-authority, receiver-state, and exhaustive exact-parent-capacity witness. |
| Rust example/property tests | Executable refinement evidence against production helpers and real DAG/storage paths. |

The [verification dossier](./fork-choice-verification.md) gives exact theorem, model,
test, and gate names.
