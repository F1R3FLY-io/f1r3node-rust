# D-03 Fork Choice over a Certified Context

**Status:** Proposed. Pending maintainer ratification.
**Kind:** Protocol.
**Sources:** dev [fork-choice specification](../../theory/fork-choice/fork-choice-specification.md), [`formal/tlaplus/fork_choice/PromotionConvergence.tla`](../../../../formal/tlaplus/fork_choice/PromotionConvergence.tla), [fork-choice convergence claim](../../../claims/fork-choice-convergence.md). PR #216 `fork-choice-specification.md` rules R-CONTEXT, R-CAUSAL-PROJECTION, R-RESTORE-HORIZON, R-LCA, R-SCORE, R-GHOST, R-TOTAL, R-PROPOSAL-PARENTS, R-EXTENSIONAL, R-COUNT, R-DEPTH, R-NONEMPTY, invariants S1 to S12, `causal_equivocation.rs`, models `CertifiedConsensusContext.tla`, `ParentFrontierCapacity.tla`, `ParentDepthBounds.tla`, `GhostTerminalFrontier.tla`, `RestoreHorizonCertifiedContext.tla`.

## 1. Question

What input does the estimator read, how does it bound its output, and what does a receiver check about declared parents?

## 2. Position on dev

- **R-FILTER.** Latest messages of slashed or invalid validators are removed before scoring.
- **R-LCA.** A latest message deeper than `LATEST_MESSAGE_MAX_DEPTH` below the top is filtered deterministically. The LCA is computed over the remaining messages.
- **R-SCORE and R-GHOST.** Scores sum validator weights down supporting chains. The head comes from a heaviest-subtree descent.
- **R-TOTAL.** Score descending, then hash ascending.
- **R-COUNT.** At most `max_number_of_parents` tips are returned. Truncation preserves the head.
- **R-DEPTH.** Secondary parents deeper than `max_parent_depth` below the main tip may be dropped.
- **N-VALID.** Validators bound-check declared parents. They never recompute fork choice.

`PromotionConvergence.tla` is in the CI gate. It models novel-signature gating and eventual GHOST restoration.

## 3. Position on PR #216

- **R-CONTEXT.** Every consumer in one round reads one `CertifiedConsensusContext`: the floor hash and post-state, the exact active set, each validator's frozen stake and bond generation, exactly one latest-message slot per validator, inherited objective-equivocation evidence, two projections, and a digest. Fork choice fails closed if any active slot is absent.
- **R-CAUSAL-PROJECTION.** A slot enters the causal-parent projection `C` only when its validator has positive frozen stake, the cited block exists and is valid, the sender matches the slot, the generation matches, that generation has no objective-equivocation evidence, and admission accepted the block. The finality-vote projection `V` is the floor-descending subset of `C`. Receiver-local caches never change either projection.
- **R-RESTORE-HORIZON.** A slot may cite the canonical genesis hash. A restored node may omit the genesis body without changing the projection or the digest. Any other missing latest-message body is a typed dependency error.
- **R-LCA.** No depth projection exists. The specification says a depth projection makes the LCA depend on ambient DAG height.
- **R-GHOST.** Two lanes. The head lane descends by score. The frontier lane expands to the exact scored terminal frontier `T` in any order. The result is the head followed by the rest of `T` sorted.
- **R-PROPOSAL-PARENTS.** Parents are the reachability-maximal antichain of the hashes in `C`, with the floor as backstop. The head stays first. Compaction removes a tip only when a retained parent covers it.
- **R-COUNT.** No truncation. If the compacted frontier exceeds `max-number-of-parents`, proposal returns a typed deferred result. Provisioning `c >= M + 1` is sufficient, not required.
- **R-DEPTH.** Depth is measured from the freshest candidate. An expired secondary leaves the live frontier deterministically and stays in the evidence closure. The receiver checks `D + depth_buffer` and exempts the head and genesis.
- **Validator boundary.** The receiver evaluates `Admit(F, P)`: at least one parent descends from the signed floor, every parent floor precedes and is contained in the floor, and parent floors form a chain. It never requires frontier equality.

`PromotionConvergence` leaves the CI gate. The specification does not say whether the novel-signature gating rule survives.

## 4. Divergence

| Aspect | dev | PR #216 |
|---|---|---|
| Vote eligibility | Not slashed, not invalid | Six conditions over a certified context, including bond generation and objective evidence |
| Depth filter on the LCA | Yes, `LATEST_MESSAGE_MAX_DEPTH` | None. Depth expiry applies to secondary parents only. |
| Parent count overflow | Truncate, keep head | Defer proposal, keep evidence |
| Silent validator slot | Not represented | Genesis placeholder with stake in the denominator |
| Receiver check | Count, depth, progress | `Admit(F, P)` plus count and buffered depth |
| Parent set vs vote set | One set | `C` for parents, `V` for votes |
| Promotion convergence model | Gated | Not gated |

## 5. Options

- **A. Adopt the certified context and its projections as written.** Includes deferral on parent overflow and removal of the LCA depth filter.
- **B. Adopt the certified context and projections. Keep a bounded fallback for overflow and depth.** Deferral becomes the first response. A bounded, deterministic compaction rule after a fixed number of deferred rounds keeps proposal live.
- **C. Keep dev fork choice. Adopt only the genesis placeholder and the two-projection split.**

## 6. Unification proposal

Adopt option A for the context, the projections, and the receiver boundary. Adopt option B for the bounds until soak evidence exists.

The certified context restates principle P4 in a stronger form. Every input is consensus data frozen at the floor, and receiver-local state cannot change it. The two projections separate causality from voting, which removes a class of dropped-sibling defects that DR-44 describes.

Deferral on overflow trades a truncated parent set for a proposal that does not happen. Principle P5 requires evidence before a liveness-affecting change. The proposal accepts deferral as the rule and keeps a bounded fallback until a soak run shows that provisioned shards never defer for more than a fixed number of rounds.

The depth-filter removal needs its own argument. The old filter bounded the scored band. The new specification bounds secondary parents through R-DEPTH but not the LCA walk. The proposal asks for a work bound before this sub-decision flips.

## 7. Ratification checklist

- Sub-decision 3.1, certified context and projections. Evidence: `CertifiedConsensusContext.tla` safe and unsafe configurations, and a differential test showing equal verdicts with dev fork choice on a DAG with no equivocation and no rebond.
- Sub-decision 3.2, deferral on parent overflow. Evidence: a soak run at `max-number-of-parents` equal to the committee size plus one with zero deferred rounds, and a run at a smaller value with bounded deferral.
- Sub-decision 3.3, LCA depth filter removal. Evidence: a stated work bound for the LCA walk as a function of floor distance.
- Sub-decision 3.4, promotion convergence. Decide whether the novel-signature gating rule is superseded or must stay gated.
- After ratification, replace the fork-choice specification and update ground truth 1.

## 8. Open questions

1. Does PR #216 retain the novel-signature gating behavior that `PromotionConvergence.tla` models? No PR #216 rule names it.
2. What happens to a proposer whose frontier stays above the parent cap for the whole deploy lifespan? R-COUNT says evidence and pending work are retained. Deploy expiry is not addressed.
