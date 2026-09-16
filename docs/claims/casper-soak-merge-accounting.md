# Casper Merge and Accounting Soak Claim

```yaml
claim_id: CLAIM-CASPER-SOAK-005
status: pending
adapter: embedded
decisions: [D-08]
pre_merge_tasks: [TASK-017-8]
post_merge_tasks: [TASK-018-4]
artifacts:
  - casper/src/rust/merging/conflict_set_merger.rs
  - casper/src/rust/merging/dag_merger.rs
  - casper/src/rust/merging/deploy_chain_index.rs
  - casper/src/rust/util/rholang/interpreter_util.rs
  - casper/src/rust/util/rholang/runtime_manager.rs
refutation: pending
construction: pending
construction_assumptions: null
binding: pending
soak: pending
```

## Contract

Inputs are locally verified execution evidence, complete replay context, exact execution identities, effect multisets, admission results, and checked accounting values.

Outputs are accepted effects, the least causal rejection closure, execution positions, pooled transfers, and final balances.

Repeated observations of one execution do not duplicate its effect. Independent executions with identical effects retain their multiplicity.

For compatible evidence, additive composition aggregates the multiset before one normalization and pooling step. Permutation and regrouping must preserve the result.

An identity with inconsistent evidence is rejected. Cache entries require the complete execution context and cannot grant authority to unverified effects.

Terminal admission rejection consumes no execution index. Executed failures retain their position and verified settlement effects.

Rejection closes over dependent effects only. Checked arithmetic must reject overflow rather than wrap or silently truncate accounting values.

For each token domain, opening balances plus approved issuance must equal closing balances plus approved burns and explicit unsettled obligations.

## Model and oracle

Audit `formal/tlaplus/deploy_recovery/EffectCausalClosure.tla` and `AdmissionEffectAlignment.tla` for the matching subclaims.

The proposed finite instance uses three executions, two identical effect values, two token domains, and a three-element causal chain.

The reference oracle uses exact identities, a multiset, arbitrary-precision arithmetic, and explicit checked-range conversion. It must not reuse the production merge fold.

Bindings cover merge selection, chain indexes, interpretation, and accounting settlement. TASK-017-8 must trace each effect to its verified production source.

## Positive and negative controls

| Control | Required observation |
| --- | --- |
| Clean regrouping and reordering | Preserve multiplicity, balances, and rejection closure. |
| Deduplicate effects by value | Lose an independent identical effect and violate multiplicity. |
| Normalize each input before aggregation | Diverge from the one-normalization oracle. |
| Reject direct dependents only | Retain a transitive dependent and violate causal closure. |
| Index an admission rejection | Violate execution-position alignment. |
| Wrap arithmetic or reuse an incomplete cache key | Violate checked accounting or evidence consistency. |

## Tiers and activation

TLC checks finite examples. Unbounded algebra and conservation require Rocq construction proofs and production bindings.

The candidate projects are `formal/rocq/merge_algebra/` and `formal/rocq/finalized_floor/`. Existing set-based theorems cannot establish additive multiset semantics without a statement audit.

Construction must export named theorems through `MainTheorem`, pass kernel checks, and record closed assumptions and source digests.

Additive activation still requires the protocol-7 FIP, FIPS approval, compatibility analysis, and fresh genesis. Do not mix legacy and additive epochs.

## Phase obligations

Pre-merge work establishes reference models and available baseline bindings. Classify intended semantic changes instead of asserting false legacy/additive equivalence.

Post-merge work binds the actual #216 accounting implementation and produces new conservation and soak evidence.

The [harness contract](./casper-soak-harness.md) defines evidence and closure. No construction proof or execution result is supplied by this scaffold.
