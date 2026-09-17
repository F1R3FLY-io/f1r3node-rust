# Price schedules and denominations

## Purpose and policy boundary

A price schedule maps validated resource demand to monetary obligations under an authenticated protocol context.
It does not authorize withdrawals, supply missing linear resources, or choose the wallets that must pay.
This contract specifies units and compatibility requirements for the restored signed phlo controls.
It does not select new production tariff values or claim complete native integration.

The selected pricing basis is actual authority-resource demand.
The [allocation policy](authority-allocation-policy-ratification.md) then distributes newly required funding over authorized feasible assignments.
Raw interaction-count pricing remains an [unselected alternative](funding-settlement-design-review.md#service-usage-valuation-with-explicit-resource-backing), not a runtime mode.
The [economic policy](economic-activation-policy-ratification.md) governs prepaid terms, failures, conversion, and fresh-genesis activation.

## Units and distinct quantities

| Quantity | Unit | Meaning |
| --- | --- | --- |
| Raw measurement | Its identified resource unit | Examples include COMM events and canonical introduction, delivery, or trace bytes. |
| Authority-resource demand | Compatible authority occurrences by resource class and location | Required logical resources retain their multiplicity, including repeated atoms. |
| Resource weight | Phlo per unit of the identified valued resource class | Converts the validated resource quantity into the signed execution-limit scale. |
| Weighted usage | Phlo | Measures use within one signed execution scope. It is not an additional spendable account. |
| Actual resource price | Smallest settlement-asset units per phlo | Comes from the authenticated applicable schedule, not a wallet's preference. |
| `phloPrice` | The same units as the actual price | Specifies the signed offered price. The selected schedule must charge this price. |
| Owner price ceiling | The same units as the actual price | Bounds the offered price for each required owner. It is not an allocation weight or exchange rate. |
| `phloLimit` | Phlo | Bounds weighted usage in one execution. It is not a persistent allowance or proof of sufficiency. |
| Retained debit | Smallest units of one identified asset | Represents money retained by the approved realized settlement. |
| Source exposure | Smallest units of the specified source asset | Bounds temporary source-specific backing independently of the retained debit. |
| Deployment fee | Native monetary units | Remains one total unit under the approved fee policy, separate from weighted usage. |
| Conversion quote | Exact input/output amounts in identified assets | Defines an authorized exchange, not the consensus resource tariff. |

The term phlogiston also names the system token in historical design records.
The restored phlo controls need explicit dimensional types despite that historical naming.
Do not create a second spendable currency merely because resource usage and native balances have different roles.
Preserve the one-denomination conservation requirement while distinguishing measurement, located authority, monetary backing, and validator-fuel custody.

Wallet display decimals do not change native integer balances.
Conversion between display text and integer amounts belongs at a checked client or API boundary.
Native accounting must reject ambiguous, negative, out-of-range, or incompatible quantities before mutation.
It must not compare raw numbers from different assets, resource units, or custody roles.

## Resource valuation before pricing

Let $`u_j`$ be the validated authority-resource quantity for resource class $`j`$.
Let $`w_j`$ be its nonnegative integer weight in phlo per resource unit.
Let $`L`$ be the signed execution limit, $`p`$ the actual schedule price, and $`F`$ the separate fee.
This scalar projection uses the current native settlement denomination for both $`pU`$ and $`F`$.
For a certified scope without prepaid offsets, the scalar monetary projection is:

```math
U=\sum_j w_j u_j,\qquad U\leq L,\qquad M=pU+F.
```

Here $`U`$ is weighted resource usage and $`M`$ is the total new monetary obligation under those hypotheses.
The resource valuation must justify each $`u_j`$ from authenticated authority and measurement evidence.
This equation does not justify replacing authority demand with COMM count, signer count, or physical purse count.
The [measurement contract](resource-units-and-measurement.md) keeps raw dimensions available for that reconstruction.

Equivalent compound regrouping must preserve valued demand and feasible acquisition alternatives.
Adding a wallet cannot multiply an unchanged obligation merely because another wallet signed.
Additional authority occurrences or serialized metadata can legitimately change measured demand.
Tests must distinguish those actual costs from a signer-count multiplier.

The planner must cover every required class, including compute and quantitative byte safety obligations.
A zero authority contribution does not erase an independently billable byte obligation.
Zero values supported by an arithmetic helper do not approve a zero-price production tariff or a metering exemption.
Accepted schedule values must follow the approved activation manifest and its safety requirements.
An unspecified class or unsupported version must not silently receive a zero price.

## Prepaid resources and current prices

Authenticated compatible prepaid resources discharge matching obligations before new acquisition is priced.
Do not subtract historical currency spending from a current bill as if all resource types were interchangeable.
Prepaid compatibility includes authority, location, resource class, backing, and acquired-right terms.
Equal monetary value alone does not establish compatibility.

The acquired right retains its captured acquisition terms while it remains compatible.
Consuming that right does not charge its acquisition again at the current price.
Separately uncovered execution work follows its own applicable schedule and consent.
A price schedule cannot silently migrate a right, change its custody, or authorize redemption.

For example, five compatible resource units are required and two already have authenticated prepaid backing.
Only three units need new acquisition.
At two native monetary units per acquisition unit, the new acquisition is six units, not ten.
Any independently billable overhead and the separate fee still apply.
These example values do not select a production tariff.

Authorized equivalent acquisition alternatives can have different new costs.
The planner first minimizes new monetary cost over that complete authorized domain.
For a fixed selected obligation, it then applies lexicographic minimax and the approved residual policy.
Pricing does not permit ignoring a compatible prepaid resource to increase new wallet charges.

## Schedule identity and authenticated context

The schedule commitment must identify all information that changes measurement interpretation or a monetary result.
At minimum, its canonical content must bind:

- The schedule format version and applicable protocol semantics.
- The resource classes, units, measurement versions, and valuation rule.
- Resource weights and the actual monetary price with exact units.
- The settlement asset identity and its native integer scale.
- The fixed deployment fee and its separate treatment.
- The applicable compatibility rules for retained resource terms.

Bind the schedule digest to the network, shard, and accepted execution context through authenticated protocol data.
Specify canonical field order, presence, lengths, integer widths, and hash domain in the wire contract.
Reject unknown required fields, malformed schedules, or ambiguous encodings under the applicable version rules.
A display name or equal numeric weights do not establish schedule identity.

New consent must identify an exact schedule or explicitly permitted compatibility entry.
Compatibility must not mean any newer version or whichever schedule a validator currently prefers.
Preparation captures the applicable authenticated state.
Publication must reject stale or mismatched evidence under the existing native dependency rules.

Historical replay uses the original accepted schedule and context.
It cannot consult the current market, local clock, latest local finality, or mutable operator settings for historical prices.
Different validators need not have identical latest local state to reconstruct one accepted execution.
This contract does not introduce a new voting rule, schedule authority, finalization procedure, or global funding lock.

The approved release initializes the new economy at fresh genesis.
The [activation contract](activation-migration-policy-decisions.md) requires explicit historical compatibility and receipt bindings.
Committing a schedule does not authorize an in-place economic upgrade or change the one-unit fee.
Selecting another settlement asset cannot redenominate that native fee.
Changing the fee's amount or asset mapping requires a separate approved decision before adding it to a converted resource charge.
Any later tariff change needs the applicable approved activation mechanism and compatibility evidence.

## Signed ceilings and exact arithmetic

### Resource policy and offered price

`PhloSchedulePolicy` identifies the schedule rules independently of its actual price.
It retains the complete environment, ordered resource classes, and compatibility rule.
Each class retains its identity, measurement unit, measurement rule, valuation rule, and weight.
The environment retains protocol, network, shard, settlement asset, settlement unit, and decimal scale.

`PhloScheduleBinding::bind_policy` compares all those fields with a required policy.
The comparison excludes only the actual price. The separate offered-price check requires that price to equal signed `phloPrice`.
Two deploys can therefore offer different prices under one unchanged resource policy.
Their complete schedule commitments remain different because each commitment includes the actual price.

A policy binding is not a new authority. The caller must obtain the required policy from authenticated protocol data or an approved compiled compatibility entry.
The [genesis resource policy](genesis-resource-policy.md) supplies the selected fresh-genesis authority and its native loading boundary.
Using the submitted descriptor as both the candidate and the required policy does not establish authorization.
Payer consent cannot authorize a changed measurement rule, even when prices, weights, and asset names remain unchanged.

The funding-intent check selects the descriptor by its exact schedule commitment before comparing its policy.
Another permitted descriptor with the correct policy cannot authorize a selected descriptor with a different policy.
Missing selected commitments and policy mismatches reject the binding.
This check introduces no tariff values, schedule update mechanism, or wire-format change.

[`SignedPhloSchedule.v`](../../../../formal/rocq/cost_accounted_rho/theories/SignedPhloSchedule.v) proves that a complete policy and an offered price determine one schedule record.
It also proves policy preservation under price replacement and rejection of changed policies.
The native property tests vary full-width prices and arbitrary resource-class lists.
Mutation tests cover every policy field, class insertion, class removal, class order, and selection of a different consented descriptor.
These checks do not prove that a caller obtained its policy from the correct chain state.

### Owner bounds

Let $`P_i`$ be each required owner's compatible signed price ceiling for the selected funding scope.
For a nonempty required set, price consent means:

```math
\forall i,\ p\leq P_i
\quad\Longleftrightarrow\quad
p\leq\min_i P_i.
```

A higher ceiling cannot override another required owner's lower ceiling.
A zero monetary share does not remove an otherwise required authorization.
Optional funding alternatives remain part of the authorized planning domain, not automatically part of every selected consent set.
Changing ownership affects future consent, not captured obligations or original-source refunds.

Integer resource weights and integer prices require exact multiplication and addition, not monetary rounding.
Use checked arithmetic for intermediate products, totals, fee addition, signed-to-unsigned conversion, and narrowed native values.
Overflow rejects the candidate without partial economic effects.
A saturating total can hide an invalid certificate and is not an acceptable substitute.

Indivisible allocation residuals are not price rounding.
The allocation policy assigns those units through its canonical cohort and captured cursor.
Compute the obligation before allocating it. Do not round separate wallet bills and then add them.
That approach can change the total when the payer count changes.

For an all-to-all scope without prepaid offsets, $`L=10`$, $`p=2`$, and $`F=1`$ give a maximum charge of 21 units.
Usage of six phlo gives a charge of 13 units.
The unused eight units return through the captured reservation sources.
Restricted branch funding can require larger source-specific exposure, subject to [explicit consent](resource-bounds-and-exhaustion.md).
The maximum retained charge does not authorize that larger exposure automatically.

## Conversion is a separate authorization

Different wallets can hold different assets and receive different authorized conversion quotes.
Those quotes do not give the wallets different consensus prices for the same resource schedule.
Each quote must specify exact assets, input debit, output commitment, fees, rounding, validity, provider capacity, and recipient authority.
Price-ceiling comparisons must use compatible units, not raw integers from different assets.

The approved conversion compositions remain distinct:

1. A separately committed conversion supplies assets for a later funding operation.
2. Atomic quote-backed funding retains its conversion and funding effects in one native publication.

Atomic funding uses exact-output terms and releases unused input through its original asset and custody path.
It does not reverse-trade a refund at a later price.
Zero output requires no conversion or conversion fee under the approved policy.
The quote contract must define nonzero integer rounding and fee boundaries explicitly before native use.
This schedule contract does not invent a universal rounding rule for all exchange providers.

Comparing contributions across different input assets requires an approved common valuation and acquisition-equivalence rule.
Without that rule, raw input amounts cannot establish minimax fairness.
The paper's fee-conversion example permits variable-rate exchange contracts.
It does not make `phloPrice` an exchange rate or authorize an external price lookup during replay.

## Native boundaries and verification

The current [byte schedule](../../../../rholang/src/rust/interpreter/accounting/byte_accounting.rs) has version one and three rates of one.
Its Blake2b-256 digest binds its domain, version, and rates using explicit little-endian integers.
That byte-schedule digest is not the complete restored economic-schedule commitment specified above.
Do not silently change its historical meaning or treat it as consent to omitted monetary terms.

[`AuthorityResourceDemand::value`](../../../../rholang/src/rust/interpreter/accounting/authority/valuation.rs) checks multiplication and addition over authority atom quantities.
It retains the source event and atom multiset.
It does not establish the complete multidimensional tariff, asset authentication, prepaid compatibility, or signed owner consent.
The [versioned wire implementation](signed-phlo-deploy-envelope.md#wire-representation) retains `phloPrice` and `phloLimit` at their original protobuf tags.
The offered format signs those fields and the complete funding intent. Owner ceilings remain separate from the offered price.
Its decoder and signing tests cover scalar mutations, version separation, and numeric boundaries.
These wire checks do not activate production admission or establish complete execution and replay integration.

The current fee evidence binds policy context, physical cohort, obligation, and cursor transition.
It does not substitute for the full resource schedule or prove a new fee amount is authorized.
The user fee remains distinct from the current three-unit validator handler charge and its separate custody role.

| Invariant | Required regression and model coverage |
| --- | --- |
| Dimensional consistency | Change asset, scale, resource class, valuation, or measurement version while retaining the same numeric values. Reject incompatible reuse. |
| Exact schedule binding | Mutate every committed field and execution-context binding. Include absent, unknown, duplicate, malformed, and differently ordered representations. |
| Representation-invariant valuation | Regroup compound authority and physical aliases. Preserve logical occurrences, location restrictions, and exact monetary obligations. |
| Prepaid acquisition remains paid | Vary acquisition and current prices, ownership, compatible terms, and separately uncovered overhead. Detect duplicated acquisition or incompatible credit. |
| Payer count does not inflate price | Vary authorized physical cohorts for unchanged demand. Preserve the total before allocation and exact residual conservation afterward. |
| Every required owner permits the price | Generate arbitrary nonempty compatible ceiling lists and compare against an independent all-owner predicate. |
| Exact bounded arithmetic | Cover zero, maximum values, intermediate overflow, fee overflow, and narrowing. Check unchanged state on rejection. |
| Conversion and tariff remain separate | Vary quote rates and input assets under one schedule. Check exact-output rounding, provider capacity, zero behavior, and original-input release. |
| Replay retains historical prices | Prepare concurrent candidates, change available current terms, restart, and replay prior accepted effects independently. |
| Publication preserves one economic result | Interleave shared-purse funding, quote consumption, transfer, and failure. Do not publish partial debits, rights, refunds, or receipts. |

Use Rocq for dimensional projections, valuation, prepaid discharge, and arbitrary finite owner-list properties.
Use TLA+ for concurrent preparation, context changes, publication, transfer, and retry.
Use native property tests with independent arithmetic and valuation oracles, plus Loom at shared publication boundaries.
Use signing vectors and integration tests for actual schedule selection and independent replay.
These are refinement obligations. Existing scalar helper proofs do not establish the complete contract.
