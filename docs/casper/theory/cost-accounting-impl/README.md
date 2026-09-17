# Cost-accounting implementation guide

This directory records the production refinement of the cost-accounted rho
calculus onto F1R3node's Rholang, RSpace, SystemVault, and Casper subsystems.
The two normative design sources are:

- [*Cost-Accounted Rho Calculus*](https://github.com/F1R3FLY-io/publications/blob/main/cost-accounting/cost-accounted-rho.tex)
- [*Continued Interactive GSLTs and the Cost Endofunctor*](https://github.com/F1R3FLY-io/publications/blob/main/cost-accounting-as-monad/continued-gslt-cost-v2.tex)

The implementation adds a native quantitative-byte safety refinement without
changing the papers' authority, linear ownership, conservation, or one-token
model. Rent and complete MeTTaIL/GSLT integration remain outside this epic.

## Choose a reading path

### Wallet, purse, and process users

1. [Wallet-funded process lifecycle](../../../rholang/20-wallet-funded-processes.md)
2. [Cost-accounted Rholang](../../../rholang/13-cost-model.md)
3. [Vaults and Tokens](../../../rholang/12-vaults-and-tokens.md)
4. [Deployment workflow](../../../rholang/18-deployment-workflow.md)

This path explains reusable wallets, persistent purses, deposits, top-ups,
funding slots, lollipop delegation, exact settlement, replay, and finalization.

### Runtime and Casper implementers

1. [End-to-end authority settlement](end-to-end-authority-settlement.md)
2. [Protocol-v6.1 deploy identity and authority](deploy-envelope-v6-1.md)
3. [Vault-backed quantitative byte accounting](vault-backed-byte-accounting.md)
4. [Evaluation transaction isolation](evaluation-transaction-isolation.md)
5. [Parallel runtime, validator, and shard isolation](parallel-runtime-and-shard-isolation.md)
6. [Deterministic parallel reduction and checkpoint ownership](deterministic-parallel-reduction.md)
7. [Mergeable evidence authentication](mergeable-evidence-authentication.md)
8. [Admission-record and runtime-effect alignment](admission-effect-alignment.md)
9. [Block-heap lifecycle and reclamation](block-heap-lifecycle.md)
   - [Runtime ownership inventory](runtime-ownership-inventory.md)
10. [Deploy occurrence and exact state effects](../deploy-occurrence/deploy-occurrence-specification.md)
11. [Finalized-floor specification](../finalized-floor/finalized-floor-specification.md)
12. [Rotating monetary allocation](rotating-monetary-allocation.md)
13. [Atomic trie update or insertion](atomic-trie-upsert.md)
14. [Signed phlo contract proposal](signed-phlo-contract-proposal.md) — design context, with later decisions recorded in the ratification documents below.
15. [Ownership transfer and funding consent](ownership-transfer-consent.md) — arbitrary-history requirements and abstract proof boundaries.
16. [Persistent funding allowance](persistent-funding-allowance.md) — quantitative conservation, generated model regressions, and concurrent publication checks.
17. [Funding settlement design review](funding-settlement-design-review.md) — valuation alternatives, prepaid backing, restricted funding, and the reviewed implementation plan.
18. [Lexicographic minimax funding](lexicographic-minimax-funding.md) — approved restricted-funding objective, exact ordering, examples, and verification requirements.
19. [Allocation policy ratification](authority-allocation-policy-ratification.md) — approved allocation, residual, and equivalent-acquisition decisions.
20. [Economic and activation ratification](economic-activation-policy-ratification.md) — approved failure, conversion, refund, and fresh-genesis release boundaries.
21. [Delegation and persistent authority contract](delegation-persistent-authority-contract.md) — operation permissions, transfer conservation, expiration, replay protection, and remaining refinement obligations.
22. [Authority and custody identity](authority-custody-identity-contract.md) — authenticated payer identities, physical aliases, logical multiplicity, joint purses, and proof boundaries.
23. [Threshold authorization and payer limits](threshold-and-payer-limits.md) — selected signers, absent members, independent size limits, arbitrary payer counts, and replay requirements.
24. [Resource units and measurement](resource-units-and-measurement.md) — exact byte dimensions, interaction and authority demand, event identity, pricing boundaries, and storage limits.
25. [Resource bounds and exhaustion](resource-bounds-and-exhaustion.md) — conservative sufficiency, source-specific exposure, signed limits, and distinct failure outcomes.
26. [Persistence and storage liability](persistence-and-storage-liability.md) — installation, repeated firings, prepaid backing, cross-deploy custody, release, and lifetime verification boundaries.
27. [Price schedules and denominations](price-schedules-and-denominations.md) — dimensional pricing, schedule identity, prepaid terms, exact arithmetic, owner ceilings, and conversion boundaries.
    See [Genesis resource policy](genesis-resource-policy.md) for authenticated policy records, ceremony checks, and historical loading.
28. [Signed price consent](signed-price-consent.md) — required owners, signed limits, funding-domain commitments, transfer capture, rejection, and native verification obligations.
29. [Price transitions and replay](price-transitions-and-replay.md) — activation scope, compatibility identities, stale plans, captured arithmetic, restart, and historical execution requirements.
30. [Conversion quotes and rates](conversion-quotes-and-rates.md) — exact-output rules, integer rounding, slippage caps, provider capacity, expiry, and atomic refund boundaries.
31. [Conversion and withdrawal authority](conversion-and-withdrawal-authority.md) — individual, joint, threshold, delegated, and capability permissions with custody and publication boundaries.
32. [Conversion provenance and refunds](conversion-provenance-and-refunds.md) — captured asset paths, separate and atomic conversion, prepaid rights, exact release, failure, and replay.
33. [Signed phlo formal contract](signed-phlo-formal-contract.md) — executable price and usage checks, typed prepaid credit, failure-charge bounds, and native refinement requirements.
34. [Signed phlo deploy envelope](signed-phlo-deploy-envelope.md) — canonical funding signatures, explicit format dispatch, historical boundaries, and activation requirements.
35. [Economic failure observation](economic-failure-observation.md) — parallel failure summaries, legacy compatibility, bounded classification, and settlement proof boundaries.
36. [Observed funding outcome](observed-funding-outcome.md) — exact resource matching, equivalent cases, ambiguous settlements, and native evidence requirements.
37. [Raw byte observations](raw-byte-observations.md) — paired measurements, legacy charge preservation, retry identity, and evaluation boundaries.

This path follows the ingress envelope through normalization, admission,
proposal, replay, atomic RevVault settlement, merge, fork choice, and finality.

### Formal verification and security reviewers

1. [Formal verification catalog](../cost-accounted-rho-verification.md)
2. [Conformance properties](../cost-accounting-conformance-properties.md)
3. [Executable conformance matrix](../cost-accounting-executable-conformance-matrix.md)
4. [Threat model](../cost-accounting-threat-model.md)
5. [Decision records](../cost-accounting-decision-records.md)
6. [Migration and implementation design](../cost-accounting-migration.md)
7. [Operator authority proof boundaries](operator-authority-proof-boundaries.md)

The verification catalog maps every production obligation to Rocq, TLA+,
Apalache, Sage, Verus, Loom, example-based tests, property-based tests, and
integration tests. Expected-refutation configurations establish that each
unsafe alternative is actually detected.

## End-to-end invariant

For each authenticated payer lane $`a`$, admission freezes:

```math
B_A(a)+B_Q(a)+F(a)\leq\Sigma(a),
```

where $`B_A`$ is the physical authority bound, $`B_Q`$ is the quantitative-byte
bound, $`F`$ is the fee, and $`\Sigma`$ is authenticated pre-state custody.
Retained settlement then applies:

```math
\Sigma'(a)=\Sigma(a)-\kappa(a)-Q(a)-F(a),
```

with realized physical authority $`\kappa`$ and realized byte cost $`Q`$.
Every validator must reconstruct the same certificate, causal events,
settlement, and adjacent state roots. The majority/clique finality calculation
does not authorize bypassing state: a finalized floor or fork-choice promotion
must also preserve every already certified effect.

## Subsystem responsibilities

| Subsystem | Responsibility |
| --- | --- |
| Crypto and wire models | Canonical message hashes, signer verification, domain-separated reservation/certificate identities, protocol and schedule commitments |
| Rholang | Linear authority regions, located purses, lollipop capability flow, deterministic normalization, runtime witness construction |
| RSpace | Complete-frontier deterministic parallel reduction, channel/join/purse conflict components, pre-mutation introduction-byte reservation, atomic COMM authority/delivery/trace charging, causal replay log, and explicit node-local root authority over shared append-only history |
| SystemVault | Persistent wallet and process-purse custody, authenticated transfer, protocol mint boundary, lexical exact-cost settlement |
| Casper proposal | State-bound fixed-point admission, retained execution, certificate/witness publication, complete economic solvency |
| Casper replay | Independent payer snapshot, causal execution, exact cost/allocation/root equality, rollback on any mismatch |
| Merge and finality | Durable-effect provenance, authenticated local merge evidence, aggregate solvency, node-local replay before support, and atomic state-preserving finalized-floor publication |
| Node block lifecycle | Bounded concurrent processing, transient-runtime destruction, platform allocator reclamation, and RSS observability without semantic access |

## Historical documents

Some files in this directory preserve staged workstream decisions. Treat a
statement as normative only when it agrees with the current protocol version,
the conformance catalog, and the end-to-end guides above. In particular, the
retired `phlo_limit × phlo_price` escrow and broad `ChargingRSpace`
precharge/refund mechanism are historical context, not production behavior.
