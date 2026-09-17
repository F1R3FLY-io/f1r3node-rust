# Signed price consent

## Purpose

Signed funding consent limits what an authorized process can charge to an identified source.
It is separate from proof that the process has sufficient matching resources and backing.
This contract defines the required meaning and signature coverage for restored `phloPrice` and `phloLimit` controls.
It does not claim that the current wire format already implements these controls.

The [price schedule contract](price-schedules-and-denominations.md) defines units, actual prices, and prepaid acquisition terms.
The [resource bounds contract](resource-bounds-and-exhaustion.md) defines independent resource, debit, exposure, and allowance limits.
The [delegation contract](delegation-persistent-authority-contract.md) defines transfers and captured obligations.
The same operation must satisfy all three contracts.

## Price consent and required owners

`phloPrice` specifies the signed offered price, as in upstream dev.
Each required owner separately authorizes a maximum price through signed funding terms.
The applicable schedule must use exactly the offered price.
The offered price must meet the authenticated chain minimum.
Neither the offer nor an owner ceiling is an allocation weight or exchange rate.
The signed ceiling does not authorize another schedule merely because its price is lower.
Exact schedule binding and explicit compatibility entries remain additional requirements.

Let $`S`$ be the nonempty set of required price consents for a selected authorized funding scope.
Let $`P_i`$ be the ceiling in consent $`i`$, and $`p`$ the applicable price in the same asset and phlo units.
Admission requires:

```math
\forall i\in S,\ p\leq P_i
\quad\Longleftrightarrow\quad
p\leq\min_{i\in S}P_i.
```

For ceilings of three, five, and eight, the effective ceiling is three.
A price of four fails even if the higher-ceiling owners can fund the entire operation.
Equal monetary sharing does not average or divide these ceilings.
Missing required consent is not unlimited authorization.

Derive required consents from authenticated authority, retained restrictions, and the permitted funding plan.
Do not derive them from arbitrary wallet inventory or an optimizer's preferred price.
An optional source can remain outside a selected plan only when the authorized alternatives permit that exclusion.
The planner cannot omit a required owner to raise the effective ceiling.

Threshold placeholders supply neither signer-derived authority nor price consent.
All selected witnesses must satisfy the [threshold contract](threshold-and-payer-limits.md).
A signer with no monetary contribution can still supply required authority and applicable price restrictions.
Physical alias resolution must aggregate capacity without removing any applicable consent.

## Independent signed limits

| Term | Required meaning |
| --- | --- |
| Execution scope | Identifies the process execution that can use the permission. It is not an unlimited lifetime contract budget. |
| `phloLimit` | Bounds weighted resource use in that execution, excluding the separately disclosed fee. |
| `phloPrice` | Specifies the offered price that the selected schedule must charge. |
| Owner price ceiling | Bounds that price for each required owner independently. |
| Retained debit cap | Bounds the final amount retained from a source in its specified asset. |
| Exposure cap | Bounds the source-specific temporary backing permitted for every certified outcome. |
| Persistent allowance | Bounds cumulative authorized draws across executions under an identified grant. |
| Fee consent | Binds the approved fee amount, unit, and treatment independently of usage. |
| Conversion permission | Authorizes the exact conversion composition, input asset exposure, provider terms, and permitted destinations. |

These limits cannot substitute for each other.
Passing a price check does not permit a larger retained debit or temporary hold.
A wallet balance is available backing, not consent to spend its entire value.
A top-up cannot expand a persistent allowance or alter captured exposure.

Let $`L`$ be the execution limit, $`P`$ the effective compatible ceiling, and $`F`$ the separate native fee.
For the [scalar projection](price-schedules-and-denominations.md#resource-valuation-before-pricing), $`LP+F`$ bounds the total charge without prepaid offsets.
Both terms in that sum use the current native settlement denomination.
The actual schedule and sound resource bound can give a tighter charge bound.
Additional per-source caps still apply, and source exposure can require a separate larger bound.

For example, exclusive branches can each require one unit from different wallets.
The maximum retained charge is one unit, but each wallet must permit its one-unit temporary hold.
A one-unit aggregate debit ceiling does not authorize two units of aggregate exposure.
The planner must prove all permitted branches feasible within the captured source caps.

## Signature commitments

The signing contract must bind funding terms directly or through an authenticated canonical commitment.
An unsigned API field, local configuration value, or proposer-supplied receipt cannot supply missing consent.
Canonical encoding must preserve field presence and reject ambiguous meanings under the applicable wire version.
The wire contract must specify exact tags, widths, domains, and signing vectors before implementation.
Do not silently reuse retired tags with changed semantics.

| Commitment group | Required coverage |
| --- | --- |
| Program and context | Program intent, language, network and shard scope, protocol version, and applicable validity terms. |
| Authorization | Complete canonical policy, exact selected witness set, supported signature schemes, and applicable capability or grant authority. |
| Funding domain | Permitted sources, assets, custody roles, resource locations, and acquisition alternatives. |
| Resource and money limits | Execution limit, price ceilings, retained debit caps, exposure caps, persistent allowance references, and separately disclosed fee. |
| Schedule | Exact schedule commitment or explicitly permitted compatibility entry with compatible units. |
| Grant history | Grant identity, expected authority version, inherited restrictions, and the permitted operation. |
| Transfer and conversion | The exact available rights or authorized quantities, recipient restrictions, selected conversion composition, and applicable quote commitment. |
| Replay identity | An operation identity and context binding that prevent reuse for a different draw or network. |

Sign the allowed planning domain and limits when the exact allocation depends on later authenticated state.
Do not require a signer to predict a future state root or sign the prover's eventual allocation unless that is the chosen authorized operation.
The resulting certificate must bind the selected assignment to those signed constraints and its actual causal inputs.
It must not enlarge the allowed domain or replace its restrictions with a different feasible plan.

Every required cryptographic witness must verify the appropriate commitment.
Capability-backed authority must establish its authenticated grant chain and current permitted scope.
A local Boolean that says authorization passed is an abstraction, not sufficient native evidence.
Required wallet withdrawal, conversion, and process authority remain distinct permissions.

Clients must display the units, execution scope, price ceiling, fee, retained debit, and maximum source exposure before signing.
They must distinguish a recurring grant from a one-execution request.
Adding a wire field without updating canonical signing, clients, validation, and replay does not implement signed consent.

## Capture, transfer, and settlement

Preparation captures provisional terms without publishing a debit or creating independent spendable funds.
Before publication, validate new draws against the applicable authenticated state and authority version.
An intervening transfer can invalidate a stale new draw without changing an already accepted obligation.
This rule uses existing native dependencies. It does not require globally identical validator state.

An accepted obligation retains its source custody, asset, price, schedule, limits, authorization lineage, and refund terms.
Settlement uses that captured evidence, not the current owner's replacement terms.
The operation can close only once, including after retry, restart, or duplicate delivery.
Historical replay reconstructs accepted effects under their original context rather than treating them as new draws.

Ownership transfer can replace terms for available transferred rights within the transfer's actual authority.
It cannot rewrite reserved obligations, reset consumed allowance, or redirect earlier refunds.
Former-owner ceilings do not persist forever unless the transferred capability expressly retains those restrictions.
Conversely, removing the former owner from a display cannot erase a retained capability restriction.

For example, a grant permits price two under owner ceilings of three and five.
After an accepted reservation, ownership transfers to owners with a ceiling of one.
The accepted reservation retains price two and its original refund provenance.
A new draw at price two fails under the replacement terms.
Returning to the old owners does not make an old operation identity or obsolete authorization version reusable.

There is no fixed lifetime limit on ownership transfers.
Per-operation signer, payer, resource, encoding, and arithmetic limits still apply.
Finite model bounds do not authorize a production lifetime transfer cap.
Partial transfers must preserve each retained obligation and partition available permission without copying it.

## Rejection and failure behavior

| Condition | Required result |
| --- | --- |
| Price exceeds any required compatible ceiling | Reject the new draw without candidate user charges or fees. Report price-consent failure, not proven insolvency. |
| Missing required consent or invalid witness | Reject before funding publication. Do not use an unlimited default or drop the offending signer. |
| Wrong asset, unit, schedule, grant version, or execution scope | Reject incompatible or stale evidence. Do not reinterpret the numeric fields. |
| Source hold exceeds exposure but retained charge fits | Reject unless another complete authorized plan satisfies every bound. |
| Insufficient resource proof or backing | Reject according to the demonstrated failure. A valid signature does not prove solvency. |
| Search-work exhaustion | Publish no candidate economic effect. Do not claim that no consented feasible plan exists. |
| Transfer after accepted reservation | Preserve captured settlement terms and refund provenance. Apply replacement consent only to new draws. |
| Duplicate settlement or refund | Preserve the earlier economic result without a second close. |
| Classified user failure | Retain only approved billable effects and the fee within captured terms. |
| Platform, certificate, or mixed unknown failure | Publish no partial economic effect under the approved failure precedence. |

Price rejection does not need a new Casper invalid-block category or fork-choice rule.
Integrate validation with the existing admission and replay boundaries.
Candidate rollback must not overwrite another committed operation's balances or consent history.

## Current evidence and required refinement

The current [envelope commitment](../../../../crypto/src/rust/signatures/signed.rs) binds canonical intent, policy, and selected-member bitmap.
The signing hash also separates the signature scheme and envelope protocol version.
The [intent encoder](../../../../models/src/rust/casper/protocol/casper_message.rs) supplies the current deployment fields.
The [protobuf](../../../../models/src/main/protobuf/CasperMessage.proto) reserves the removed phlo fields.
These existing commitments do not already authenticate the restored terms specified here.

[`FundingPriceConsent.v`](../../../../formal/rocq/cost_accounted_rho/theories/FundingPriceConsent.v) relates the minimum ceiling to all required compatible ceilings.
It includes permutation, added-consent, duplicate-value, and empty-list properties.
Its natural-number lists do not establish signature validity, compatible native assets, or correct selection of required owners.
Duplicate-value invariance in that theorem does not permit duplicate witnesses or removal of distinct restrictions.

`FundingConsentHistory.v` preserves captured reservation terms across abstract transfers.
`ConsentedFundingAllowance.v` composes price consent with allowance quantities and captured amounts.
Those models use abstract authorization inputs, supplied roots, and opaque signed limit data.
They do not prove native signature coverage, complete resource sufficiency, or independent-validator publication correctness.

| Invariant | Required examples, generated properties, and negative controls |
| --- | --- |
| All required ceilings hold | Compare minimum with an independent all-owner predicate over arbitrary bounded lists. Include empty, zero, repeated, permuted, and maximum values. |
| Required consent cannot disappear | Alias a physical purse, assign zero contribution, or offer a cheaper optional wallet. Preserve all actual authority restrictions. |
| Signatures cover economic terms | Mutate each limit, asset, schedule, grant, source restriction, fee, scope, and conversion term. Keep the signature unchanged and require rejection. |
| Authorized planning remains complete | Compare alternate valid funding assignments within the signed domain. Reject domain expansion and impermissible source omission. |
| Limits remain independent | Hold price fixed while varying resource, retained debit, exposure, and allowance constraints separately. |
| Transfers preserve captured terms | Generate mixed reserve, transfer, owner-return, top-up, execute, settle, and abort histories. Distinguish pending draws from accepted obligations. |
| Concurrent candidates preserve permission | Model shared grants and custody with separate preparation and publication. Include stale versions, duplicate closure, and independent disjoint operations. |
| Replay uses historical evidence | Reconstruct accepted effects after owner, schedule, client-version, and local-runtime changes. Compare independent validators and cold replay. |

Extend Rocq obligations to the typed signed contract and checked native arithmetic.
Use TLA+ for concurrent histories, without treating independent validators as one globally shared mutable ledger.
Use property tests with independent consent and conservation oracles.
Use Loom at actual native synchronization boundaries and integration tests through signing, admission, settlement, and replay.
Report model and generated bounds explicitly. A bounded successful run does not prove every possible native history.
