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
10. [Deploy occurrence and exact state effects](../deploy-occurrence/deploy-occurrence-specification.md)
11. [Finalized-floor specification](../finalized-floor/finalized-floor-specification.md)

This path follows the ingress envelope through normalization, admission,
proposal, replay, atomic RevVault settlement, merge, fork choice, and finality.

### Formal verification and security reviewers

1. [Formal verification catalog](../cost-accounted-rho-verification.md)
2. [Conformance properties](../cost-accounting-conformance-properties.md)
3. [Executable conformance matrix](../cost-accounting-executable-conformance-matrix.md)
4. [Threat model](../cost-accounting-threat-model.md)
5. [Decision records](../cost-accounting-decision-records.md)
6. [Migration and implementation design](../cost-accounting-migration.md)

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
