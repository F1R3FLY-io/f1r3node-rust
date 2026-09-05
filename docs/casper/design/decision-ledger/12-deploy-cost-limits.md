# D-12 Deploy Cost Limits: Removal of phloLimit and phloPrice

**Status.** Proposed. Pending maintainer ratification.

**Kind.** Protocol and economics boundary. The removal changes a block-validity rule and the client deploy contract.

**Sources.**

- dev [Consensus Protocol](../../CONSENSUS_PROTOCOL.md) section 4 step 8, [cost model](../../../rholang/13-cost-model.md), [deployment workflow](../../../rholang/18-deployment-workflow.md), and `models/src/main/protobuf/CasperMessage.proto`.
- PR #216 decision records DR-5, DR-9, DR-11, DR-13, DR-27, DR-28, and DR-31.
- PR #216 `cost-accounting-impl/d3-replace-phlo-with-tokens.md`, `cost-accounting-impl/deploy-envelope-v6-1.md`, `cost-accounting-impl/wd-d2-acceptance-gate.md`, `cost-accounting-migration.md` sections 3.2 and 8.4, and `cost-accounting-linear-logic.md` section 3.3.

## 1. Question

Why did PR #216 remove the per-deploy `phloLimit` and `phloPrice` fields, what replaces their functions, and which of those replacements has a specification?

This entry also answers a review question raised on this PR. The question states that the two fields were removed primarily because of a missing specification. The missing part is how the fields behave when several wallets fund one execution. It also covers the case where ownership of a process moves from one owner set to another.

## 2. Position on dev

A deploy carries `phloLimit` and `phloPrice`. The deployer prepays `phloLimit * phloPrice`. Unused phlogiston is refunded after execution. When execution exhausts the limit, the node rolls back the deploy and charges the full prepayment.

Block validation step 8 requires that the phlogiston price meets the shard minimum. The `Validate::phlo_price` rule enforces it. One deployer key signs and funds each deploy. Dev has no cosigner concept and no shared-wallet funding.

## 3. Position on PR #216

The branch removes both fields and reserves their wire tags. Three decision records give the reasons.

- **DR-9.** The cost-accounting paper replaces phlogiston with signature-indexed tokens. Cost is one token per atomic COMM. The acceptance gate is the only enforcing cost authority. The per-operation gas table stays as diagnostic telemetry. A bridging lemma between the two models was rejected because the paper does not bound per-COMM operation cost.
- **DR-5.** The precharge and refund machinery is unnecessary once acceptance commits resources by a linear proof.
- **D3, OD-4.** The multi-signer field `Cosigner.phlo_share` is deleted outright. The record states that no construct in the paper maps to it. Per-component spend is the effective-supply closure, not a share field.

The functions of the two fields move to three places.

| Old function | Replacement on PR #216 | Source |
|---|---|---|
| Finite execution bound chosen by the client | Capacity derived from authenticated supply after the fixed fee. DR-31 records that the first replacement, an unbounded budget, was wrong. | DR-31, D3 OD-1 |
| Price per unit | A fixed fee `F(a)` per selected payer lane. `min_phlo_price` stays as an ingress economic floor and no longer gates block validity. | D3 D.5, `wd-d2-acceptance-gate.md` |
| Prepayment and refund | Reservation at admission, settlement of realized cost, one atomic SystemVault transition. | DR-28, envelope section "RevVault funding and settlement" |

Multi-wallet funding has a specification on the branch. The funding authority is the composition of the selected members' ground authorities. Each selected lane `a` must satisfy `B_A(a) + B_Q(a) + F(a) <= Σ(a)`. One payer cannot rescue an underfunded lane. Two cosigner sets that share a wallet draw it down through one live residual ledger in canonical order. DR-28 added that ledger after a red-team finding of cross-group over-admission.

Transfer of funding authority has no specification on the branch. The linear-logic document describes the lollipop connective as a capability that delegates authorization. The migration document lists delegated metering as achievable in user space and not formalized. DR-27 finding F-A separates the capability connectives from the funding signature at ingress. The funding grammar is ground, quote, and composition only.

## 4. Review finding

The review question is partly supported and partly not.

- **Supported.** The scalar fields have no defined meaning once several wallets fund one deploy. The branch found no construct for a per-signer share and deleted it. The branch also found that a static per-group supply read admits combined demand above a shared wallet, and it repaired that with a live ledger. Both facts show that multi-wallet funding needed new semantics that the old fields could not carry.
- **Supported.** Transfer of funding authority between owner sets is not specified. The branch keeps it out of the funding path rather than specifying it.
- **Not supported as the primary cause.** The decision records give the primary cause as adoption of the paper's token model. DR-9 and DR-5 remove the fields because the enforcing cost authority moved to the acceptance gate. The specification gaps are consequences the branch met while implementing that decision, and it recorded them as such.

The protocol document on PR #216 keeps the sentence that the phlogiston price meets a minimum in validation step 8. D3 section D.5 says the block rule was removed and the floor stays at ingress only. The two statements disagree.

## 5. Divergence

| Aspect | dev | PR #216 |
|---|---|---|
| Client-declared execution bound | `phloLimit` | None. Capacity is supply minus fee. |
| Price | `phloPrice`, minimum enforced in block validation | Fixed fee per lane. Minimum enforced at ingress only. |
| Prepayment | `phloLimit * phloPrice`, refund of the unused part | Reservation at admission, settlement of realized cost |
| Funding parties | One deployer key | Selected members of a signed policy, one lane each |
| Shared wallet across deploys | Not possible | Live residual ledger in canonical order |
| Transfer of funding authority | Not possible | Not specified. Capability connectives are separated from funding. |
| Wire | Fields present | Tags 7, 8, and 15 reserved |

## 6. Options

- **A. Ratify the removal at the protocol-6 boundary with the three replacements.** The client bound, the fee, and the settlement rule become the normative deploy contract.
- **B. Ratify the removal but require a client-declared cap on top of supply.** The cap bounds the deployer's exposure to one deploy. It needs a multi-wallet split rule, which is the gap the branch found.
- **C. Keep the dev fields.** Reject the token model for deploy admission.

## 7. Unification proposal

Adopt option A at the protocol-6 boundary, with three specification items as conditions.

1. **Multi-wallet funding.** Write the per-lane bound, the no-rescue rule, and the live residual ledger as normative rules in the finalized-floor or deploy-occurrence specification. The branch implements them and models them. The protocol document does not state them.
2. **Transfer of funding authority.** State in the protocol document that funding authority does not transfer inside a deploy. Delegation is a user-space pattern over signature channels. If a later change formalizes it, that change needs its own row.
3. **Client exposure.** State what bounds a deployer's loss on one deploy. On the branch the bound is the selected purse balance after the fee. A maintainer must decide whether that bound is acceptable or whether option B is required.

Correct the validation step 8 sentence on PR #216 so it matches D3 section D.5.

This entry follows principle P2. Admission derives from authenticated on-chain supply, not from a client-declared number that validators cannot check against custody.

## 8. Ratification checklist

- The three specification items above exist as normative text.
- The DR-28 cross-group regression and the envelope negative cases are in a gate.
- The `min_phlo_price` ingress floor is documented in the node API reference as an ingress rule, not a consensus rule.
- After ratification, update the Consensus Protocol sections 2 step 4, 4 step 8, and 10, and the Rholang cost-model and deployment-workflow documents.

## 9. Open questions

1. Does any client-side control bound the cost of one deploy after `phloLimit` is gone? The sources name none.
2. The read-only API fields `phloPrice` and `phloLimit` still appear in the OpenAPI schema on PR #216. Are they removed, or reported as zero for legacy deploys?
3. Does the `min_phlo_price` floor apply to each selected lane or to the deploy as a whole?
