# D-10 Repeat-Deploy Carrier Index

**Status:** Proposed. Pending maintainer ratification.
**Kind:** Protocol refinement. The predicate is unchanged. The evidence and the key are decided here.
**Sources:** dev [Consensus Philosophy](../../CONSENSUS_PHILOSOPHY.md) section 4.4 and the 2026-09-01 row. PR #387 [`CLAIM-FINALITY-002`](../../../claims/repeat-deploy-carrier-index-equivalence.md), [`formal/tlaplus/carrier_index/`](../../../../formal/tlaplus/carrier_index), [CbC repair plan](../cbc-repair-plan.md). PR #216 `deploy-occurrence-verification.md` carrier-index refinement, `CarrierIndexSoundness.tla`, `DeployIdentitySeparation.tla`, `DeployIdentitySeparation.v`, [CbC and FV reconciliation](../cost-accounting-cbc-fv-reconciliation.md) section 4.3.

## 1. Question

What keys the carrier index, which model gates it, and what does the claim ledger state about it?

## 2. Position on dev

The 2026-09-01 row proposes the index as a consensus-complete carrier cache over valid, invalid, and approved blocks. An in-window absence skips the scan. A hit requires scope verification. A read failure falls back to the scan. The row is pending ratification. Section 4.4 calls the index the remedy for issue 24.

## 3. Position on PR #387

Section 4.4 narrows the index to one measured cost. It removes the ancestor scan for a signature with no in-window carrier and nothing else. `CLAIM-FINALITY-002` states seven claims over body signatures with status pending. `CarrierIndex.tla` has two invariants and two negative controls and is registered in the TLA+ gate. Three files join the mandatory CbC scope. Telemetry counters and a forced on and off differential are required before the claim discharges.

## 4. Position on PR #216

The index key is a protocol-tagged deploy lookup identity. A legacy block uses the signature domain. A protocol-6 block uses the envelope-commitment domain. Equal bytes in the two domains are different keys. `DeployIdentitySeparation` shows that an untagged key can suppress the wrong deploy. Protocol-6 admission commits carrier, metadata, occurrence, and lifecycle rows in one transaction. A bounded in-process decoded-identity cache serves the exact scan and proves nothing. `CarrierIndexSoundness.tla` has thirteen invariants, five unsafe controls, and two validators, and is registered in the PR #216 gate list. The PR #216 row keeps "the remedy" wording.

## 5. Divergence

| Aspect | PR #387 | PR #216 |
|---|---|---|
| Key | Body signature | Protocol-tagged deploy identity |
| Role wording | One measured cost, narrow fast path | The remedy |
| Model | `CarrierIndex.tla`, 2 invariants | `CarrierIndexSoundness.tla`, 13 invariants |
| Claim ledger | `CLAIM-FINALITY-002`, pending | Conformance rows, no claim |
| Telemetry requirement | Eight counters, differential, soak gate | None |
| Extra cache | None | Bounded decoded-identity cache |

## 6. Options

- **A. Signature key now, typed key at protocol 6.** The claim is parameterized by an identity function. Both models gate.
- **B. Typed key now.** Requires the envelope commitment domain to exist before protocol 6.
- **C. Signature key only.** Reject typed identity.

## 7. Unification proposal

Adopt option A.

The typed key is only meaningful when a second identity domain exists, which is a protocol-6 fact under D-01. Until then the signature domain is the only domain and the two keys coincide. The claim ledger should state the predicate over a deploy identity function that returns the signature on legacy blocks and the envelope commitment on protocol-6 blocks. `DeployIdentitySeparation` becomes the obligation that the function is injective across domains.

Adopt the PR #387 role wording. The soak evidence shows the index is not a complete repair.

Both models gate. They check different properties at different depths. Neither subsumes the other.

## 8. Ratification checklist

- The PR #387 differential test with the carrier path forced on and off, per the CbC repair plan.
- `CarrierIndexSoundness.tla` safe and unsafe configurations added to the gate when PR #216 lands, without removing `MC_CarrierIndex`.
- After ratification, rewrite the 2026-09-01 row once, with both branches' authors agreeing on the text, and update `CLAIM-FINALITY-002` to the identity function.

## 9. Open questions

1. Does the decoded-identity cache need a CbC claim, or does the rule that it proves nothing exempt it?
2. Does the prior-rejection count in D-07 use the same identity function?
