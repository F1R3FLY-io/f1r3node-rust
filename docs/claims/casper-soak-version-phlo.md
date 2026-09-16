# Casper Protocol and Phlo Soak Claim

```yaml
claim_id: CLAIM-CASPER-SOAK-007
status: pending
adapter: embedded
decisions: [D-01, D-12]
pre_merge_tasks: [TASK-017-11]
post_merge_tasks: [TASK-018-4]
artifacts:
  - casper/src/rust/engine/genesis_ceremony_master.rs
  - casper/src/rust/engine/genesis_validator.rs
  - casper/src/rust/blocks/proposer/block_creator.rs
  - casper/src/rust/validate.rs
  - casper/src/rust/util/rholang/runtime_manager.rs
  - models/src/main/protobuf/RhoTypes.proto
refutation: pending
construction: pending
construction_assumptions: null
binding: pending
soak: pending
```

## Contract

Inputs are the approved Casper version, genesis configuration, signed deploy envelope, shard minimum price, payer balances, and execution outcome.

Outputs are ceremony, adoption, proposal, and reception decisions, plus prepayment, settlement, refund, and exhaustion outcomes.

All Casper authority paths must select the same approved protocol version. Unsupported versions must be rejected.

Casper protocol 7 remains separate from reusable accounting authority version 8. Version 8 cannot silently become the Casper wire authority.

Both `phloLimit` and `phloPrice` remain signed and consensus-visible. Preserve protobuf tags, APIs, minimum-price validation, prepayment, refunds, and exhaustion behavior.

Protocol-7 envelope commitment must include both fields. Token accounting cannot replace either field or bypass execution bounds.

Multi-wallet funding cannot activate without a normative mapping of both fields. Client-selected apportionment and delegation remain separate FIP matters.

## Model and oracle

Audit `ProtocolVersionLifecycle.tla` and `ProtocolActivationCoherence.tla` under `formal/tlaplus/deploy_recovery/` before reuse.

Reuse PR #430 storage-bound evidence only after checking the pinned model and its Phlo assumptions.

The proposed finite instance uses an approved and unsupported version, two prices, two limits, two payers, and success, failure, and exhaustion outcomes.

The oracle independently computes version agreement, signed-envelope acceptance, prepayment, and refund bounds. Mutation of either signed field must invalidate authentication.

TASK-017-11 must trace ceremony, adoption, proposal, reception, and settlement to exact source functions and wire fields.

## Positive and negative controls

| Control | Required observation |
| --- | --- |
| Clean version and Phlo combinations | Preserve wire fields and correct settlement. |
| Substitute accounting version for Casper version | Violate authority separation. |
| Change one authority path only | Violate ceremony/adoption/proposal/reception agreement. |
| Omit either Phlo field from the commitment | Accept a signed-field mutation and fail authentication comparison. |
| Bypass shard minimum price | Violate price validation. |
| Refund more than the checked prepaid amount | Violate settlement conservation. |

## Tiers and phases

TLC checks finite version transitions and cost examples. Arbitrary execution-cost and settlement claims require Rocq construction evidence and production bindings.

Audit candidate theorems under `formal/rocq/finalized_floor/` rather than treating their existence as discharge evidence.

Pre-merge work preserves baseline Phlo contracts and prepares protocol-7 controls. Post-merge work verifies actual #216 envelopes, accounting boundaries, and production settlement.

FIPS approval and fresh genesis remain activation conditions. A merged implementation or a passing soak does not satisfy those conditions by itself.

The [harness contract](./casper-soak-harness.md) defines evidence and closure. All new verification results remain pending.
