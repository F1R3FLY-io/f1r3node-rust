# Casper Slashing Soak Claim

```yaml
claim_id: CLAIM-CASPER-SOAK-006
status: pending
adapter: embedded
decisions: [D-09]
pre_merge_tasks: [TASK-017-9]
post_merge_tasks: [TASK-018-3]
artifacts:
  - casper/src/rust/slashing_authorization.rs
  - casper/src/rust/merging/rejected_slash.rs
  - casper/src/rust/validate.rs
refutation: pending
construction: pending
construction_assumptions: null
binding: pending
soak: pending
```

## Contract

Inputs are authenticated objective evidence, invalid block hashes, parent pre-state, offender identity, positive stake, and evidence, target, and current epochs.

Outputs are slash authorization, canonical offender sets, deterministic seeds, and recovery decisions for merge-rejected slashes.

Authorization preserves the upstream truth table and parent-pre-state authority. It requires positive offender stake and matching applicable epochs.

Same-key rebonding cannot make old evidence slash a new bond. Rejection records alone cannot authorize an economic slash.

Invalid-hash-bound seed generation and offender deduplication must be deterministic. Arrival order and restart cannot change the authorized result.

Economic neglect and objective-equivocation extensions remain inactive where upstream authority does not activate them. Neglect rejection cannot mint economic evidence.

## Model and oracle

Audit existing TLA+ and Rocq slashing models against the actual Rust authorization and rejected-slash recovery paths.

The proposed finite instance uses three validators, two epochs, one rebond, and two competing evidence arrival orders.

The oracle preserves the upstream truth table and Rust-to-Scala bisimilarity contract. Supplementary canonical reconstruction is a comparison target, not replacement authority.

TASK-017-9 must record exact production functions, theorem names, and source digests before reusing evidence.

## Positive and negative controls

| Control | Required observation |
| --- | --- |
| Clean evidence permutations and restart | Preserve authorized offender sets and recovered slashes. |
| Reuse evidence after rebond | Violate epoch authorization. |
| Authorize from a rejection record alone | Violate objective-evidence authority. |
| Omit a merge-lost slash | Diverge from upstream recovery. |
| Accept a forged deploy or absent evidence | Violate authenticated authorization. |
| Seed from arrival order | Violate deterministic seed agreement. |

## Tiers and phases

TLC checks the finite scenarios. Rocq must retain `main_bisimilarity_theorem` and `main_bisimilarity_strong` as proof anchors.

`main_slashing_algorithm_correct` remains supplementary. Kernel checks and closed assumptions remain required for the named unbounded claims.

Pre-merge bindings compare current upstream behavior and supplementary reconstruction. Post-merge bindings rerun those comparisons against the actual #216 implementation.

Conformance runs cover the truth table. Soaks repeat concurrency, delayed dependencies, restarts, and evidence permutations without changing authorization policy.

The [harness contract](./casper-soak-harness.md) defines evidence and closure. All claim-specific evidence remains pending.
