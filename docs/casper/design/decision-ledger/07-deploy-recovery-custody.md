# D-07 Deploy Recovery, Custody, and Retry Packaging

**Status.** Proposed. Pending maintainer ratification.

**Kind.** Mixed. Occurrence identity and record keys are protocol. Custody and packaging are proposer policy.

**Sources.**

- dev [Consensus Philosophy](../../CONSENSUS_PHILOSOPHY.md) sections 4, 5, and 8, [Casper glossary](../../GLOSSARY.md) entries retry gate, merged-frontier retry packaging, retry frontier lease, [Consensus Protocol](../../CONSENSUS_PROTOCOL.md) section 2 step 3, [`formal/tlaplus/deploy_recovery/`](../../../../formal/tlaplus/deploy_recovery).
- PR #216 DR-33, DR-35, DR-55, DR-56, `deploy-occurrence-specification.md` obligations O1 to O14, rules R-REASON-CONFLUENCE, R-CARRIER-RETRY-CUSTODY, models `DeployRecovery.tla`, `StaleSiblingRecovery.tla`, `RecoveryFrontierCoverage.tla`, `RejectionReasonConfluence.tla`.

## 1. Question

What identifies a rejected deploy, who may retry it, and when may a proposer package the retry?

## 2. Position on dev

- **Phase 1, ratified.** Loss-aware adjudication at all three merge sites. Prior-rejection count strictly outranks cost. Each signature owns its count. A dependency chain uses the maximum member count.
- **Retry gate.** A retry is legal only after the latest kept rejection record of the signature settles inside the block's frozen floor closure. A closed gate returns `PrematureDeployRetry`.
- **B1 packaging, implemented in PR #312, liveness pending ratification.** The owner packages a retry only when **one selected parent covers all valid latest messages**. The retry frontier lease permits normal packaging after three blocks.
- **Custody.** Ground truth 4 says the recovery path uses owner-scoped buffers plus the floor-paced retry gate. Step 3 of block creation exempts `rejected_in_scope` signatures from the in-scope exclusion.
- **Records.** A rejected signature lands in the rejected-deploy buffer. The record carries a reason.

## 3. Position on PR #216

- **Exact occurrence (DR-33).** A retry is a transition over occurrence state. Let `O_d` be the source occurrences visible from the selected-parent closure and `T_d` their exact tombstones. Recovery requires `O_d \ T_d` to be empty. It also requires the same strict lifespan as ordinary admission. Missing bodies fail closed.
- **Record key and reason (DR-35, R-REASON-CONFLUENCE).** A rejection record is keyed by `(deploy signature, source block)`. The reason is diagnostic and joins under the order `unspecified < collateral < merge conflict < duplicate`. The join is commutative, associative, and idempotent, so parent arrival order cannot change the block body.
- **Custody (DR-55, R-CARRIER-RETRY-CUSTODY).** The sender of the rejected source carrier owns the retry. Only that owner packages it after the floor gate opens. Distinct owners retry independent carriers concurrently. No global lock and no recovery leader.
- **Packaging (DR-56).** The frontier is ready when **the selected parent set collectively covers every valid latest message**. For every valid latest message, some selected parent descends from it. One-parent coverage implies collective coverage. The converse fails on a split frontier. Lease expiry cannot bypass the floor gate, custody, lifespan, replay, or validation.
- **Exclusion rule.** Step 3 excludes deploys with an active occurrence in the selected-parent closure. A historical self-chain occurrence outside the closure does not count.
- **Identity.** The glossary changes "deploy signature" to "deploy identity" in the prior-rejection count. Entry D-10 covers the identity tag.

## 4. Divergence

| Aspect | dev | PR #216 |
|---|---|---|
| Retry authority | Signature has a kept rejection and the gate is open | Every visible source occurrence is tombstoned and the gate is open |
| Record key | Signature | `(signature, source block)` |
| Reason on disagreement | Not specified | Canonical semilattice join |
| Custody | Owner-scoped buffers | Carrier sender only, stated as a rule |
| B1 predicate | One parent covers all valid latest messages | The parent set collectively covers them |
| Lease | Three blocks from the latest kept rejection | Retained. Cannot bypass any gate. |
| Count owner | Signature | Deploy identity |

## 5. Options

- **A. Adopt exact occurrence, the semilattice, owner custody, and collective coverage.**
- **B. Adopt exact occurrence, the semilattice, and owner custody. Keep the one-parent B1 predicate.**
- **C. Keep dev.**

## 6. Unification proposal

Adopt option A, as four sub-decisions.

**7.1, exact-occurrence recovery.** Ratify. The rule is a function of on-chain data, which principle P2 requires. It closes the duplicate-occurrence storm that DR-33 describes.

**7.2, record key and reason join.** Ratify. The key adds source identity that the signature loses. The join makes the block body independent of observation order. Both follow principle P2.

**7.3, carrier-owner custody.** Ratify. It states the owner-scoped buffer rule from ground truth 4 as a normative rule. It adds no lock and no leader, which keeps proposer discretion under principle P3.

**7.4, collective coverage.** Ratify as the corrected B1 predicate. The Rocq result shows one-parent coverage implies collective coverage and that the converse fails on a split frontier. The old predicate deferred an authorized retry that the new one admits. This is proposer packaging, so principle P3 applies. The B1 liveness row from 2026-08-20 stays pending until soak evidence exists.

## 7. Ratification checklist

- 7.1 and 7.2: `DeployRecovery.tla` and `RejectionReasonConfluence.tla` safe and unsafe configurations in the gate list, and a regression that reproduces the concurrent duplicate and collateral overlap.
- 7.3: the owner-custody regressions named in DR-55.
- 7.4: `RecoveryFrontierCoverage.tla` with its one-parent control, and the split-frontier examples named in DR-56.
- After ratification, update the glossary entries for merged-frontier retry packaging and kept rejection record, the protocol step 3 exclusion rule, and the 2026-08-20 B1 row.

## 8. Open questions

1. Does the deploy-identity change to the prior-rejection count depend on protocol 6, or does it apply to legacy signatures unchanged? Entry D-10 tracks the tag.
2. The dev remedy ladder lists C1 and C2 as escalations behind soak evidence. PR #216 does not mention them. Are they still the next steps if collective coverage leaves residual expiries?
