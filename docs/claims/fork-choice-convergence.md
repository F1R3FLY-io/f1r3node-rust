# Fork-Choice Convergence Claim

## Status

Retired. The deploy-promotion mechanism this claim bounded no longer exists.

Deploy-support promotion was deleted because its input evolved over time: the spine
could legally flip between two sound certificates and fork the finalized read surface
(the ucc `ca7197d8` freeze). Deploys need no spine position to finalize
(`verdict_convergence_spec::a_deploy_finalizes_from_a_carrier_the_spine_never_holds`).
Its successor, certified-tie promotion (`prefer_certified_main_parent`), was deleted
with the heaviest-subtree descent rewrite: a certified branch holds a strict weight
majority, so no rival child can tie it and there is nothing left to promote.

## Current selection rule

The proposer's main parent is the GHOST head from the heaviest-subtree descent
(`estimator.rs::rank_forkchoices`), except when the parent set collapses to a single
deploy-free parent that DAG-covers every other parent (content-preserving). Stable
GHOST selection is therefore structural, not eventual.

## Evidence

- `casper/tests/fork_choice/heaviest_subtree_descent.rs`
- `casper/tests/fork_choice/prop_ghost_argmax.rs`

The retired Rocq/TLA promotion artifacts (`GuardBridge.v` promotion theorems,
`PromotionConvergence.tla`) model deleted code and are historical.
