# Signed phlo contract proposal

## Status and purpose

This document specifies the selected design for arbitrary-payer funding and signed phlo controls.
It does not claim complete native integration or protocol activation.
The design preserves upstream offered-price semantics and separates owner ceilings from the deploy's `phloPrice` field.
Deployment limits and persistent funding allowances have separate scopes.
Actual authority-resource demand supplies the pricing basis.
New funding uses equal sharing where feasible and [lexicographic minimax](lexicographic-minimax-funding.md) for restricted assignments.
Earlier sponsors do not receive automatic reimbursement from new owners.
The [ownership-transfer proof record](ownership-transfer-consent.md) covers the approved arbitrary-history requirement and the initial abstract proof results.

This proposal does not change Casper voting, fork choice, finality, pruning, or branch selection.
It does not introduce a global execution lock or require every validator to have the same latest local state.

The [allocation record](rotating-monetary-allocation.md) documents the implemented fee allocator and its verification limits.
The proposed resource-pricing integration must reuse that allocator for certified all-to-all funding scopes.
Restricted scopes must use the approved minimax objective over complete feasible assignments.
The [settlement design review](funding-settlement-design-review.md) qualifies this requirement for restricted funding scopes and prepaid resources.

## Specification and implementation boundaries

The two cost-accounting papers constrain this design:

- [Cost-Accounted Rho Calculus](https://github.com/F1R3FLY-io/publications/blob/main/cost-accounting/cost-accounted-rho.tex), sections labeled `sec:spectrum` and `sec:funding-slots`, requires signature-specific resources and located funding.
- [Continued Interactive GSLTs and the Cost Endofunctor](https://github.com/F1R3FLY-io/publications/blob/main/cost-accounting-as-monad/continued-gslt-cost-v2.tex), section labeled `sec:linear`, distinguishes fixed, dependent, and conservative resource proofs.
- The first paper's “Fee conversion” paragraph describes a one-for-one exchange example and permits variable-rate exchange contracts.

The local source files under `../publications/` were inspected for these statements.
The links identify the source documents, not immutable revision-specific verification evidence.
Neither cited passage selects the restored wire parameters or a production monetary tariff.

The [offered envelope](signed-phlo-deploy-envelope.md#offered-price-payload) restores the original scalar fields in [CasperMessage.proto](../../../../models/src/main/protobuf/CasperMessage.proto).
Its separate authorization format binds those fields without changing existing signature payloads.
The historical [D3 decision](d3-replace-phlo-with-tokens.md) removed singular escrow and per-signer shares.
Restoration must explicitly supersede the affected D3 clauses while preserving signature-specific authority.
An internal execution limit is not signed user consent.

The fee path now supports bounded arbitrary physical cohorts and rotating residuals.
Compute and byte settlement still require a separate authority-to-money contract.
The [funding audit](../multi-wallet-funding-path-audit.md) identifies this distinction.
Equal fee allocation alone does not establish equal monetary allocation for every resource.

## Decisions already recorded

| Subject | Recorded decision | Boundary |
| --- | --- | --- |
| Unrestricted monetary allocation | Use capped max-min allocation for certified all-to-all scopes. | Do not multiply the total monetary obligation by signer count. |
| Restricted monetary allocation | Minimize the descending contribution vector lexicographically over feasible allocations of the same obligation. | Preserve funding restrictions and caps. Complete the canonical tie contract before integration. |
| Contribution basis | Share newly required funding without automatic reimbursement of earlier sponsors. | Prepaid credit requires compatible authority and authenticated backing. The requested plan-agent review precedes integration. |
| Pricing basis | Price actual authority-resource demand. | Preserve semantic multiplicity and representation-invariant backing. Wallet count alone is not resource demand. |
| Residual units | Use canonical cohort order and a certified pre-state cursor. | Do not use deploy entropy or permanent lexical preference. |
| Physical custody | Give each eligible physical purse one allocation position. | Preserve distinct logical authority occurrences. |
| Joint purses | Treat authorized joint and individual purses equally for the fee. | Do not infer authority for unrelated or unsigned purses. |
| Limits | Support a configured signer cap, initially 64. | Bound physical cohorts separately, including joint purses. |
| `phloPrice` | Preserve the signed offered price and require an exactly matching schedule price. | Meet the chain minimum and every separately signed owner ceiling. |
| `phloLimit` | Bound one signed deployment execution scope. | Wallet count must not multiply the limit. |
| Persistent allowance | Bound cumulative authorized draws across executions and ownership transfers. | Transfers and wallet top-ups must not recreate consumed allowance. |
| Activation | Use fresh-genesis activation for the planned feature. | Do not silently reinterpret accepted historical data. |
| Scope | Keep rent and full MeTTaIL integration outside this epic. | Retain current byte safety accounting and the GSLT interfaces. |

## Proposed signed controls

A resource vector records successful atomic COMM events, introduced bytes, transferred bytes, and trace bytes separately.
The pricing projection must also retain the authority-resource demand associated with those measurements.
Raw COMM counts or physical cell counts alone do not define the selected price basis.
A price schedule defines integer resource weights, the settlement asset, monetary price, and fixed fee.
A schedule identifier commits to the complete schedule and its protocol version.
The fee remains the existing one-unit fee under the [recorded fee decision](staged-fee-exchange.md).
Committing that fee in a schedule does not make it configurable.
Changing its amount or mapping it to another asset denomination requires a separate decision.

Let $`u_j`$ denote the validated authority-resource demand for resource class $`j`$ under the selected refinement.
Keep raw measurements separately so replay can reconstruct that demand rather than infer it from signer count.
Let $`w_j`$ denote its nonnegative integer weight in phlo units.
Let $`L`$ denote `phloLimit` and $`p`$ denote the schedule's price in smallest settlement-asset units per phlo.
Let $`F`$ denote the fixed fee in those asset units.

For an execution without prepaid offsets, the proposed scalar projection is:

```math
U = \sum_j w_j u_j, \qquad U \le L,
\qquad M = pU + F.
```

This formula is a proposed monetary projection, not a replacement for signature-indexed token consumption.
The authority-to-money proof below must justify the projection before implementation.
The tariff must not charge the same custody units once as logical fuel and again as an unrelated monetary debit.
The design review found that COMM count and physical cell count cannot alone establish a representation-invariant fuel valuation.
The selected valuation must account for compound regrouping before this formula becomes the production contract.
With prepaid resources, distinguish total valued consumption from newly required funding.
Apply prepaid credit only through the approved resource valuation and provenance rules, not by subtracting incompatible token counts.

`phloLimit` would cap weighted resource use, excluding the separately disclosed fixed fee.
Signed per-resource limits would remain available for independent byte and compute bounds.
Protocol resource caps would still apply, even when the user authorizes a larger monetary amount.
All arithmetic would reject overflow before state mutation.

A scalar limit cannot prove that every required signature has sufficient resources.
Admission would require both the resource proof and the monetary reservation.
For data-dependent execution, evidence must bind the causal state or prove a conservative bound, as the second paper requires.

### Deployment limit and persistent allowance

The approved design has two layers with separate accounting identities.
The deployment limit bounds one execution under its signed terms.
The persistent allowance bounds cumulative authorized draws from an identified grant across executions.
The [allowance verification record](persistent-funding-allowance.md) defines the initial conservation model and remaining integration obligations.

Legacy deployment evaluation supplies the starting execution boundary, not a lifetime contract budget.
The native contract must specify continuation activation and located-region attribution before integration.
A dormant continuation must not acquire unlimited future funding from its installation deployment.
Each admitted execution must fit its deployment limit and every applicable persistent allowance.

Adding wallets must not introduce a signer-count multiplier on the same measured resource vector.
Additional signature or authority metadata can still increase actual measured byte usage.
Tests must distinguish that overhead from duplicated charges.

The first paper's `sec:data-dependent` selects conservative overcharge-and-refund.
Admission must establish sufficient backing for its sound bound, not treat a client limit as the sufficiency proof.
The failure policy remains a separate economic decision.

An authorized amendment can expand a persistent allowance.
A wallet top-up alone cannot expand that allowance.
Transfers preserve consumed allowance and existing reservations, while new owners authorize future draws within the transferred rights.
Splits must divide available authorization without copying the full allowance into each child.
Joins must preserve provenance and applicable restrictions.
Refund routing for encumbered partial transfers remains an explicit design gate.

### Approved price meaning

| Choice | Meaning of `phloPrice` | Consequence |
| --- | --- | --- |
| Selected: offered price with separate owner ceilings | The signed value is the actual price per phlo, subject to the chain minimum. Separate funding terms supply owner ceilings. | Preserves upstream field semantics and shared-owner price consent. |
| Alternative: ceiling in the scalar field | The scalar is a maximum. A permitted schedule can charge less than that value. | Requires different field semantics and explicit format separation from upstream offered-price deploys. |

The selected option does not introduce priority bidding or change Casper selection.
The price ceiling does not determine the resource schedule or authorize a larger debit by itself.

Let $`P`$ denote the separate signed funding ceiling.
Admission requires $`p = \mathrm{phloPrice} \le P`$ and an explicitly compatible schedule identifier.
The chain minimum must not exceed $`p`$.
For the proposed scalar tariff, the maximum retained charge is $`LP+F`$ when the signed fixed fee equals $`F`$.
An all-to-all funding scope without prepaid offsets can reserve the compatible schedule's tighter bound $`Lp+F`$.
Restricted branch obligations can require more temporary backing, but only under separate explicit reservation consent.

The following example assumes an all-to-all scope without prepaid offsets.
A limit of 10 phlo and a separate ceiling of three permit at most 31 units with a one-unit fee.
An offered price of two requires a matching schedule price of two.
The maximum reservation is then 21 units.
Usage of 6 phlo costs 13 units, including that fee.
The remaining 8 reserved units return to their original custody.
These numbers illustrate units and rounding only. They are not proposed production prices.

### Shared ownership and inherited consent

Ceilings constrain the actual price independently for each required authorization.
One owner's higher ceiling cannot override another required owner's lower ceiling.
This rule applies to required owners of one joint purse and to required funding consents across multiple purses.

Let $`S`$ be the nonempty set of price consents required for one planned draw.
Let $`P_i`$ denote the ceiling in consent $`i`$, expressed in the same asset and phlo units.
Let $`p`$ denote the applicable schedule price in those units.
The consent condition is:

```math
\left(\forall i \in S,\ p \le P_i\right)
\quad\Longleftrightarrow\quad
p \le \min_{i\in S} P_i.
```

Thus the effective ceiling is the minimum, or greatest lower bound, of the required ceilings in ordinary numerical order.
It is also the greatest price that satisfies all those ceilings.
It is not the least upper bound, which would be their maximum.
If permitted-price sets are ordered by inclusion, combining the requirements takes their intersection.

For ceilings `[3, 5, 8]`, the effective ceiling is 3.
A schedule price of 2 satisfies every consent.
A schedule price of 4 violates the first consent, even if the other owners have sufficient balances.
Equal monetary allocation must not divide or average these price ceilings.

The set $`S`$ must come from the authenticated authority and funding plan, not arbitrary available wallet inventory.
An unsigned threshold placeholder does not supply consent.
An optional funding alternative may use another authorized cohort only when the approved funding rules permit that alternative.
The allocator must not silently omit a required owner to raise the effective ceiling.
Physical custody alias removal must preserve every applicable consent constraint.

Ceilings with different denominations or resource units cannot be compared as raw integers.
Any conversion must use the approved authenticated conversion contract and conservative rounding.
An empty consent set is not an unlimited price authorization.

An ownership transfer must identify which rights, funds, and obligations move to the new owners.
The new owners must authorize the terms for new draws against their acquired rights.
Their consent cannot expand retained rights belonging to others or rewrite an existing certified reservation.
A valid transfer can replace consent for transferred rights only within the authority granted by the transfer contract.
Former-owner ceilings must not become permanent restrictions unless the transferred capability expressly retains those restrictions.
The precise transfer boundary and reservation treatment remain formal contract obligations before implementation.
There must be no arbitrary fixed limit on the lifetime number of ownership transfers.
Per-operation resource and signer limits must not become lifetime transfer limits.
Transfers must not reset consumed allowances or erase existing funding obligations.

The planner must bind consent provenance, ownership state, cohort, units, schedule, and applicable ceilings into its evidence.
Replay must evaluate that evidence against its certified causal state, not the latest local ownership state.

### Schedule changes

The signed terms must commit to the resource weights, price compatibility, asset, fee, and protocol version.
Replay must use the schedule bound to the accepted evidence, not a newer local schedule.
An incompatible schedule must cause rejection or require renewed signed consent before execution.
The default proposal binds one exact schedule identifier instead of authorizing unspecified future schedules.
With exact schedule binding, the price ceiling is an additional consent check, not permission to accept later price changes.

The applicable schedule must follow the accepted protocol rules for the candidate's causal state.
This proposal does not add a latest-global-root equality requirement.
Schedule activation rules that change consensus behavior require separate review by the Casper maintainers.

## Authority-to-money contract

This is the principal semantic gate, not a pricing implementation detail.
The resource proof must retain every signature, located surface, linear occurrence, and authorized conversion.
The monetary planner must separately conserve one specified monetary obligation.

For a compound authority, the solver must prove all component requirements.
Extra money from one component cannot supply another component's missing authority without an authorized conversion or transfer.
Custody alias removal must not remove logical multiplicity.

The contract must specify whether monetary payment acquires authority-indexed fuel or settles a separately represented execution liability.
It must identify which balances decrease, which tokens are consumed, and which recipients receive payment.
It must then prove conservation across that mapping.
The current shared representation does not justify introducing a second charge automatically.

No broader monetary settlement implementation should precede this mapping and its proof obligations.
The existing one-fee repair remains valid within its documented scope.

### Integration decision requiring review

The September 10 implementation audit confirmed that this boundary is not only a theoretical concern.
The current [native settlement contract](end-to-end-authority-settlement.md#status-and-scope) distinguishes logical COMM cost from physical authority consumption.
One COMM can consume multiple authority cells.
[`VaultSettlement`](../../../../casper/src/rust/util/rholang/costacc/vault_cost_deploy.rs) records a vault burn and a separate fee.
`ApplyCostDeploy::new` requires their total to fit the corresponding physical reservation.

DR-27 identifies REV and phlogiston as names for one system-token denomination.
The later native-settlement decision replaces duplicate ledger mechanisms, but preserves conservation and authenticated funding requirements.
The first paper's `sec:spectrum` still requires signature-indexed fuel.
It does not establish that one logical COMM, one consumed authority cell, and one vault unit are interchangeable monetary quantities.

The independent plan review found a third option that the initial comparison omitted.
Funding slots separate the sponsor from the initiating authority in the first paper.
Authorized contributions can establish fuel backing before the existing spectral consumption.
These phases can form one atomic native transition without a second independently spendable balance.

| Integration choice | Consequence | Required qualification |
| --- | --- | --- |
| Keep direct authority/byte draws | Define the restored controls over those draws. | This does not establish general arithmetic sharing across resource obligations. |
| Co-fund backed resources, then consume them once | Allocate authorized contributions and preserve spectral consumption. | Specify valuation, acquisition, source eligibility, prepaid resources, and original refund provenance. |
| Introduce explicitly priced entitlements | Replace the monetary refinement while preserving paper resource transitions. | Prove issue, split, join, transfer, consumption, redemption, and settlement for the new representation. |

The review recommends the second architecture as the conservative starting point.
It withdraws the earlier claim that replacing direct burns is already the best-established solution.
The existing burn can remain an implementation primitive if it consumes exactly the backing assigned by the contribution plan.
The user selected actual authority-resource demand for valuation after this review.
Usage-only pricing remains a [documented alternative](funding-settlement-design-review.md#service-usage-valuation-with-explicit-resource-backing), not an activation option.
The backing proof and remaining economic contracts are still required before production integration.
The review records alternatives and conditional counterexamples, not a demonstrated native exploit or a defect in the papers.

The model must preserve required authority when an integer monetary allocation gives a consenting payer a zero contribution.
It must also distinguish new contributions from previously paid resources and authorized reimbursement.
The existing allowance, custody, and refund proofs do not establish this complete refinement.
See the design review for the source evidence, decision package, native boundaries, and verification sequence.

## Reservation, allocation, and refund

A physical capacity is the authenticated amount available for this obligation after other committed constraints and signed exposure limits.
Capacity must also account for earlier retained reservations in the applicable candidate sequence.
This requirement does not impose a global reservation order across independent validator branches.
Branch combination must preserve aggregate solvency through the existing dependency and merge rules.
An exposure limit bounds a purse's authorized maximum debit.
The payer cohort contains only eligible physical purses in canonical order.

Let $`C_i`$ denote purse capacity and $`R_i`$ its proposed maximum reservation.
Let $`D_i`$ denote its retained contribution and $`M_{max}`$ the maximum newly required monetary contribution.
Let $`M`$ denote the realized newly required contribution after approved prepaid offsets.
Without prepaid offsets, these quantities equal the complete monetary obligations above.
For a certified all-to-all scope, the planner and settlement must establish:

```math
0 \le D_i \le R_i \le C_i,
\qquad \sum_i R_i = M_{max},
\qquad \sum_i D_i = M,
\qquad \mathrm{refund}_i = R_i-D_i.
```

Restricted data-dependent obligations need a more general reservation contract.
An A-only branch and a B-only branch can each cost one unit, while requiring two source-specific units of conservative backing.
Only one branch executes, so the maximum retained charge remains one unit.
The design must distinguish the signed charge ceiling from signed temporary-reservation exposure.
It must not increase temporary holds silently or claim sufficiency from the scalar charge ceiling alone.
Dependent evidence that fixes the branch or authorized all-to-all funding can avoid this extra reservation.
The [design review](funding-settlement-design-review.md#restricted-branches-can-need-larger-temporary-reservations) records this decision and its proof obligations.

The proposed reservation uses capped max-min allocation over the authenticated capacities.
That direct use requires a certified scope in which every selected source can fund every included obligation.
Restricted funding scopes require a joint feasibility plan before any allocation can become a valid reservation.
For a fixed newly required obligation, choose feasible retained contributions through the approved [lexicographic minimax rule](lexicographic-minimax-funding.md).
Do not substitute leximin contribution or an unconstrained equal split.
They must not lose restrictions through custody alias removal or aggregate monetary allocation.
Actual settlement uses the certified cohort, eligibility relation, and cursor, with each reservation as the settlement capacity.
It must not reselect payers from post-user balances or spend a concurrent top-up beyond signed exposure.
Both planning results remain provisional until the candidate commits.
Only retained settlement updates the cursor.

The proof must establish feasibility for every actual obligation within the maximum reservation.
It must also establish the intended fairness relation between reservation and actual settlement.
Aggregate solvency alone does not prove either per-purse exposure or fairness.
The unrestricted scalar allocator proofs do not establish these properties for arbitrary restricted branch families.

For capacities `[10, 10, 10]`, maximum obligation 8, and cursor 0, the reservation is `[3, 3, 2]`.
For actual obligation 5, the proposed settlement is `[2, 2, 1]`.
Refunds are `[1, 1, 1]`, and actual residual allocation determines the committed cursor.

Money, refunds, cursor changes, receipts, and retained application state must publish atomically.
Rejected candidate effects must not survive a later branch merge.
Independent cohorts must remain independent except for actual shared custody or state dependencies.
The native merge tests must verify these requirements without adding global serialization.

## Storage, failures, and conversion

### Storage boundary

Introduced bytes are not retained byte-time.
The proposed resource vector preserves introduction, payload-transfer, and trace measurements as separate safety dimensions.
It does not create recurring rent or a storage-lifetime tariff.
Persistent installation and later firings must each use their documented measurement boundaries without duplicate charges.

### Failure policy requiring review

Admission rejection and platform faults must not become monetary charges through this proposal.
Deterministic user failure and exhaustion need an explicit retained-charge policy.
Possible policies include no retained charge or a specified fee and bounded realized resource charge.
The latter requires evidence of the failed computation and exact rollback of application effects.
Neither policy may charge an uncaught platform fault as user behavior.

Existing failure behavior remains unchanged until the policy and regression expectations receive approval.
The formal model must distinguish failure classes instead of using one generic failure action.

### Conversion boundary

Different wallets can accept different exchange quotes without changing the shard's resource price schedule.
This is a proposed extension, not a description of the existing one-for-one carrier exchange.
The existing exchange has no rate parameter and remains unchanged by this draft.
A conversion quote must bind input and output assets, amounts, authority, recipient, expiration, and rounding.
A replayable conversion must depend on authenticated evidence, not an external live price query.

An authorized exchange can supply the settlement asset before reservation.
An atomic conversion-and-reservation alternative must prove rollback and preserve refund provenance for every asset.
The choice between these workflows requires review.
Variable-rate quotes do not authorize a wallet-specific consensus resource tariff.

## Signed evidence and wire migration

The canonical signed terms must include limits, schedule consent, asset, fee consent, payer exposure, and authorized conversion commitments.
Deploy identity must commit to those terms and their domain-separated version.
Every selected signer must authorize the relevant terms before any debit.
Threshold placeholders must not become funding sources.
An inherited purse must retain applicable monetary consent, schedule consent, exposure limits, and capability provenance.
The consumer's signature cannot expand an inherited purse's authority or authorize a new price for its owner.
If that consent is absent or incompatible, admission must reject the proposed draw or require renewed authorization.

New funding structures must use separate wire fields without reusing reserved numbers for different meanings.
The original scalar tags 7 and 8 retain their offered-price and execution-limit meanings.
Tag 15 remains reserved for the removed per-signer share.
API compatibility names do not justify reinterpreting retired protobuf names or historical signing bytes.
Client libraries, CLI, hashing, admission, execution, replay, and receipt validation must migrate together.

### Schedule record version 1

This section defines the [versioned schedule record](../../../../models/src/rust/phlo_schedule.rs), not an active network format.
The record commits to interpretation rules as well as numeric prices.
It does not replace the separate signed funding intent or its source authorizations.
The [canonical wire primitives](signed-phlo-formal-contract.md#canonical-wire-primitives) define the byte framing used below.

Every table row is one length-prefixed byte field, in the listed order.
Integer payloads use exactly the specified unsigned big-endian width.
The surrounding eight-byte field length remains mandatory, including for integer fields.
This uniform framing permits exact reconstruction and prevents field-boundary ambiguity.
The record is schedule metadata, not a new per-COMM payload.

| Position | Field | Payload and meaning |
| --- | --- | --- |
| 0 | Format domain | Exact ASCII bytes `f1r3node:phlo-schedule:v1`. This value identifies the schema and hash domain. |
| 1 | Protocol version | Eight-byte integer identifying the applicable protocol semantics. |
| 2 | Network | Nonempty canonical network identity bytes. |
| 3 | Shard | Nonempty canonical shard identity bytes. |
| 4 | Settlement asset | Nonempty canonical asset identity bytes. |
| 5 | Settlement unit | Nonempty canonical identity of the native integer unit used for prices and debits. |
| 6 | Decimal scale | One-byte exponent. One integer settlement unit represents ten to the negative exponent units of the identified asset. |
| 7 | Resource classes | The complete nested class-list record specified below. |
| 8 | Actual price | Eight-byte integer in settlement units per phlo. |
| 9 | Fixed fee | Eight-byte integer equal to one. No other value is valid under this version. |
| 10 | Compatibility rule | Exactly 32 bytes committing to the rule for retained resource terms. |

The asset, settlement unit, and decimal scale must match the authenticated native denomination.
The scale does not change an existing balance or introduce fractional native arithmetic.
The fixed fee uses the same native denomination and remains separate from weighted resource usage.
The decoder must not infer these values from a wallet display label or local configuration.

The resource-class list contains a framed four-byte count followed by that many framed class records.
The count must be positive and must fit the configured class limit.
Each class record contains exactly these five framed fields:

| Position | Field | Payload and meaning |
| --- | --- | --- |
| 0 | Class identity | Nonempty canonical identity bytes, unique within the schedule. |
| 1 | Measurement unit | Nonempty canonical identity of the raw measured unit. |
| 2 | Measurement rule | Exactly 32 bytes committing to the measurement definition and version. |
| 3 | Valuation rule | Exactly 32 bytes committing to the authority-resource valuation definition and version. |
| 4 | Resource weight | Eight-byte integer in phlo per valued resource unit. |

Class order determines the positional class indices used by the execution projection.
Changing that order changes the schedule, even when the same class records remain present.
Duplicate class identities are invalid.
The encoder must not sort classes independently of the resource-index mapping.

Rule commitments must identify complete rule descriptors, not mutable display names.
Their referenced definitions must remain available in authenticated data for validation and historical replay.
A matching digest alone does not prove that the node supports or has correctly implemented a rule.
Admission must resolve each commitment to its supported interpretation before resource validation.
Unknown interpretations must reject admission rather than use a default rule.

The schedule identifier is Blake2b-256 of the complete canonical outer record.
The domain field is part of that preimage.
Byte-encoding injectivity does not prove hash collision freedom.
The identifier relies on the existing cryptographic collision-resistance assumption.

Parsing must enforce exact integer widths, commitment widths, field order, and complete consumption at every nested level.
Reject unsupported format domains, missing fields, trailing bytes, zero class counts, duplicate class identities, and excessive record sizes.
Check class-count limits before allocating class storage.
Do not allocate from an untrusted count or length before the corresponding input and configured limits have been checked.
Use independent limits for total bytes, field bytes, and class count.

Zero weights and zero actual prices remain representable integer values.
Their representation does not approve a free production tariff or remove required compute and byte-safety obligations.
The authenticated activation manifest selects the permitted schedule and enforces its safety requirements.

The existing byte-only schedule digest retains its historical domain, byte order, and accepted meaning.
The complete schedule record must not silently reinterpret that digest as a commitment to additional fields.
Fresh-genesis integration must bind the complete schedule alongside any retained evidence required by the approved compatibility rules.

Required verification includes full-record roundtrip and re-encoding identity, plus mutation tests for every field and class position.
Tests must cover nested truncation, extreme integer values, count limits, duplicate classes, altered fee policy, and unknown rule commitments at admission.
The complete signed-intent integration must additionally prove that schedule substitution invalidates consent and deploy identity.

### Resource permission key version 1

The [resource-key codec](../../../../models/src/rust/phlo_resource.rs) encodes exact resource identities for signed funding permissions.
It preserves the identity used by the native funding checker.
Encoding a key does not authorize a purse, acquire fuel, or certify an execution bound.

Each record has five framed fields, with the same eight-byte length prefixes as the schedule record.

| Position | Field | Payload |
| --- | --- | --- |
| 0 | Format domain | Exact ASCII bytes `f1r3node:phlo-resource:v1`. |
| 1 | Location | Exact located-surface identity bytes. |
| 2 | Resource class | Four-byte unsigned big-endian index into the complete selected schedule. |
| 3 | Acquisition terms | Exact retained acquisition-term identity bytes. |
| 4 | Authority | The complete prefix-order node list below. |

The authority list starts with a framed four-byte node count.
Exactly that many framed node records follow.
Each node record contains a framed one-byte tag and a framed payload.

| Tag | Node | Payload | Child count |
| --- | --- | --- | --- |
| 0 | Unit | Empty. | 0 |
| 1 | Ground | Exact ground identity bytes. | 0 |
| 2 | Quote | Exact quoted identity bytes. | 0 |
| 3 | And | Empty. | 2 |

The node order is root, left subtree, then right subtree.
For example, `And(And(A, B), C)` has the node order `[And, And, A, B, C]`.
Repeated leaves remain separate logical occurrences.
The codec does not limit the authority to two owners.
Configured byte and node limits bound each record independently of the lifetime number of ownership transfers.

The decoder starts with one pending child slot.
Each node consumes one slot. An `And` node adds two slots.
Zero pending slots before another node indicate an additional root and cause rejection.
Nonzero pending slots after the last node indicate an incomplete tree and cause rejection.
This scan requires no recursive decoder or recursively owned tree.

Reject unknown tags, nonempty Unit or And payloads, incorrect integer widths, and trailing bytes at every record level.
Check node-count limits before allocation. Add storage only after parsing and validating the corresponding node.
Length and node counts must not cause integer overflow or count-sized allocation without sufficient input.

This schema preserves exact structural identity. It does not perform algebraic regrouping, authority normalization, or custody alias removal.
Any required normalization must precede key construction under the same authenticated rules used by execution.
Ground and Quote tags remain distinct, including when their identity bytes match.
Empty identity fields remain representable where the internal key permits them.
Admission must independently validate location, acquisition provenance, and supported authority identities.

`PhloResource::wire_key` uses the existing bounded resource traversal for both native permissions and wire projection.
Thus the projection does not maintain a second authority traversal with different operator support.
Unresolved thresholds and the deferred linear operators remain rejected by this funding projection.
Their future support requires a versioned semantic extension, not an unknown-tag default.

The class index has meaning only with the complete schedule bound into the enclosing signed intent.
The enclosing source authorization must also bind custody, hold and debit caps, and fee permission separately.
This resource-key record does not replace those fields or the historical authority signing format.

### Source policy record version 1

The [source-policy codec](../../../../models/src/rust/phlo_source.rs) binds one physical custody identity to its exposure limits and exact resource permissions.
It is a component of the enclosing signed economic intent, not an independently sufficient funding authorization.
The enclosing intent must bind the schedule, owner consent, execution scope, and aggregate exposure.

Each record contains exactly six framed fields.

| Position | Field | Payload |
| --- | --- | --- |
| 0 | Format domain | Exact ASCII bytes `f1r3node:phlo-source:v1`. |
| 1 | Custody | Nonempty canonical physical custody identity bytes. |
| 2 | Hold cap | Eight-byte unsigned big-endian maximum temporary exposure in native settlement units. |
| 3 | Debit cap | Eight-byte unsigned big-endian maximum retained debit for this source in the same units. |
| 4 | Fee permission | Exactly one byte. Zero denies the fee. One permits it within the source caps. |
| 5 | Resource permissions | A framed four-byte count followed by that many framed version-1 resource-key records. |

The hold cap and debit cap remain separate constraints.
A smaller selected debit does not authorize a hold above the signed hold cap.
The codec represents both caps independently, including zero and maximum integer values.
The funding checker must enforce the actual hold, debit, and available balance together before state mutation.

Resource permission is set membership, not resource supply.
The constructor sorts complete canonical resource-key bytes in ascending unsigned byte order and removes identical permission records.
The wire decoder requires that strict order and rejects duplicates.
Reordering or repeating a constructor input permission cannot change the resulting consent or increase its spending limits.
Repeated authority leaves inside a resource key remain separate logical occurrences and are not removed.

An empty resource-permission set is valid.
It can represent fee-only consent when the fee flag is one and the applicable caps permit that fee.
The resource-permission set never implicitly permits the fee.
Likewise, fee permission never permits an unlisted resource.

Validation applies independent total-byte, field-byte, permission-entry, and aggregate authority-node limits.
The aggregate node limit covers all permission records, not each record separately.
Constructor work limits count submitted entries, nodes, and framed resource bytes before duplicate removal.
This rule prevents repeated permissions from bypassing validation-work limits.
The decoder checks each framed record and its bounds before adding its storage.
It does not allocate a permission array from an unchecked advertised count.

`PhloSourceConsent::wire_policy` projects native consent through the same bounded resource-key traversal used by execution.
Its work budget covers custody bytes and every resource permission together.
It preserves the custody identity, caps, and fee flag exactly while normalizing only the permission set.

The enclosing intent must reject duplicate physical sources or resolve aliases under the approved custody rules before allocation.
The source codec does not infer authorization from possession of these bytes.
It does not resolve custody aliases, prove signatures, amend allowances, or authorize an ownership transfer.
Historical signing formats remain unchanged until complete versioned intent integration and activation.

### Signature responsibilities

User signatures authorize economic intent.
Validator block signatures authenticate retained execution evidence through the block commitment.
Whether the planned output requires an additional standalone signature remains an explicit contract decision.

## Verification before production integration

The [signed phlo formal contract](signed-phlo-formal-contract.md) defines the executable reference checks and their limits.
It separates proven scalar bounds from the required native funding, authorization, and publication refinements.

The following inventory defines required work, not completed verification.
Each production path must map to a proof obligation, an executable invariant, and an independent test oracle.

| Obligation | Formal work | Executable checks |
| --- | --- | --- |
| Resource projection | Rocq proofs preserve signature-indexed requirements while applying the approved monetary mapping. | Generated compound signatures, located regions, aliases, and resource vectors. |
| Checked prices | Prove dimensional consistency, exact rounding, and bounded integer arithmetic. | Boundary values, overflow, zero prices, incompatible schedules, and changed units. |
| Maximum exposure | Prove actual debit never exceeds each certified reservation. | Generated maxima, actual usage, capacities, and residual positions. |
| Fair allocation | Prove conservation, caps, canonical order, and the reservation-to-settlement fairness relation. | Independent allocation oracle, arbitrary bounded cohorts, joint purses, and zero-capacity positions. |
| Concurrent settlement | TLA+ models independent validators, overlapping custody, top-ups, competing candidates, and publication failures. | Native signed branch tests and reversed merge order. |
| Local concurrency | Model prepared plans, stale reuse, atomic publication, and aborts as separate actions. | Loom tests for actual Rust synchronization boundaries, not synthetic claims about RSpace internals. |
| Failure classification | Prove permitted charges and application rollback separately for each failure class. | Fault injection at reservation, execution, settlement, checkpoint, and publication boundaries. |
| Conversion provenance | Prove asset conservation and authorized conversion without implicit minting. | Generated quote rounding, expiry, failed conversion, recipient mismatch, and refunds. |
| Evidence binding | Prove the specification's binding requirements and identify cryptographic assumptions. | Signature mutation, altered limits, schedules, cohorts, and replay certificates. |
| Inherited consent | Prove that later consumption cannot expand a prior purse's authorized exposure or price consent. | Cross-deploy funding, lollipop delegation, incompatible schedules, expired authority, and forged consumer consent. |
| Shared price ceilings | Prove that the minimum ceiling is equivalent to every required consent passing, for arbitrary nonempty consent lists. | Input permutations, duplicate aliases, required low-ceiling owners, optional alternatives, missing consent, and denomination mismatches. |
| Ownership transition | Prove that authorized transfer changes only the consent attached to transferred rights and preserves certified reservation obligations. | Competing transfers and reservations, partial transfers, replacement owners, retained restrictions, and cold replay from prior ownership state. |
| Historical replay | Model accepted evidence with a fixed schedule despite newer local state. | Cold replay and schedule transitions using the accepted protocol rules. |

Unsafe model variants must fail when they remove exposure checks, collapse authority, multiply money, lose refunds, or publish partial settlement.
Bounded state exploration does not prove arbitrary network liveness.
Proofs over arbitrary finite lists do not prove native serialization, cryptography, or runtime publication.
Native tests must cover those boundaries explicitly.

## Review and implementation sequence

1. Preserve the selected maximum-price meaning and confirm the proposed `phloLimit` units.
2. Resolve the authority-to-money mapping against the papers and current custody representation.
3. Ratify failure charges, conversion workflow, and evidence signatures.
4. Complete the contract inventory and formal models before production integration.
5. Implement canonical signed terms and pure checked pricing with the existing allocator.
6. Integrate reservation, settlement, refund, and cold replay as one complete lifecycle.
7. Run targeted unit, property, concurrency, and native integration checks.
8. Update clients, documentation, and release evidence before PR amendment.

No step authorizes unrelated Casper changes or advances an unverified task gate.
