# Conversion quotes and rates

## Purpose and scope

A conversion quote specifies the input needed to obtain an identified output asset under authorized terms.
It does not authorize an unrelated withdrawal or determine the shard's resource price.
This contract refines the [approved conversion compositions](economic-activation-policy-ratification.md) for native cost-accounting integration.
It does not claim that priced vault conversion already exists in the node.

The *Cost-Accounted Rho Calculus* paper's fee-conversion example swaps client and validator resources.
The paper also permits variable-rate exchange contracts.
Those resource transfers must retain the located authority and backing rules of the two cost-accounting papers.
Neither paper supplies this native quote format, an external price oracle, or a new Casper pricing authority.

## Quote authority and identity

A provider must authorize the output commitment and the permitted use of its available capacity.
The input owner must authorize the input debit, selected quote, conversion composition, and permitted recipients.
A grant can supply these permissions only within its authenticated scope and retained restrictions.
Possession of an arbitrary signed price statement does not establish access to the provider's assets.

The quote source can be an authorized contract or authenticated provider evidence supported by the native conversion contract.
No global whitelist or new trusted quote authority is implied here.
The verifier must establish the exact provider authority, input permission, and committed pricing rule from accepted data.
It must not fetch a current market price during validation or replay.

The canonical quote commitment must bind:

- Format version, quote identity, network, shard, and supported evaluation context.
- Input and output asset identities, integer scales, physical custody roles, and permitted destinations.
- Provider authority and the input permission or grant references.
- Exact-output pricing rule, supported output amounts, integer rounding, and all applicable fee terms.
- Input and output retained caps, maximum fees, and separately permitted temporary exposure.
- Validity endpoints, endpoint presence, and the permitted use or cumulative-capacity rule.
- Conversion composition, funding schedule commitment, and applicable failure behavior.

The wire contract must define canonical field order, lengths, integer widths, rule identifiers, and hash domains.
Unsigned defaults must not supply omitted fee, expiry, recipient, or exposure consent.
Unknown pricing rules and malformed or ambiguous quotes must reject before economic mutation.
An operator's local quote cache cannot replace authenticated quote evidence.

## Exact-output pricing

Let $`y`$ be the required output in the smallest units of the identified output asset.
Let $`D`$ be the quote's supported output domain, including zero.
Let $`q(y)`$ be its deterministic required input, including conversion fees, in the smallest units of the input asset.
Atomic funding requires:

```math
q:D\longrightarrow\mathbb{N},\qquad 0\in D,\qquad q(0)=0.
```

The evaluator must return either the exact input and its fee decomposition or an explicit rejection.
The rule must terminate within the applicable bounded work and representation limits.
It must not publish partial effects during evaluation.
Zero output performs no conversion and incurs no conversion fee under the approved policy.
A quote that requires a positive fee at zero output is unsupported.

Positive output can have integer rates, rational rates, fixed fees, minimum fees, maximum fees, or supported piecewise rules.
Each rule must define its domain and exact integer behavior.
Do not infer a provider's rounding from a display price or floating-point calculation.
This contract does not authorize accepting an arbitrary program as an unchecked pricing oracle.

For example, one explicitly selected quote can use positive integer rate terms $`a,b`$, with $`b>0`$, and input fee $`f`$:

```math
q(y)=
\begin{cases}
0,&y=0,\\
\left\lceil ay/b\right\rceil+f,&y>0.
\end{cases}
```

Here the ratio specifies input units per output unit, and the quote explicitly selects upward rounding before the fee.
This example does not prescribe that formula for every provider.
If $`a=3`$, $`b=2`$, and $`f=2`$, outputs of one, two, and three require inputs of four, five, and seven.
Zero still requires zero input.
These values illustrate the rule and do not select a production conversion rate.

Minimum and maximum fees must identify whether they constrain the fee component or the total input.
Specify the order of percentage calculation, rounding, fixed charges, and any clamps when a supported rule combines them.
Reject contradictory ranges and invalid denominators.
Use checked integer arithmetic or a proved wider intermediate representation followed by checked narrowing.
Do not wrap, saturate, or round the total differently on another architecture.

## Slippage and signed limits

Slippage protection limits the input a user permits for a required output.
It does not permit the provider to change the accepted quote during execution.
Under captured exact-output terms, the quoted input is exact and the signed cap remains an independent constraint.

A client can derive a cap from a displayed reference quote and a user-selected tolerance.
The resulting integer cap and permitted quote domain must be signed.
An unsigned display percentage or later market sample cannot expand that cap.
Different input assets require separate units and the approved common valuation before contribution fairness can compare them.

The actual resource price still comes from the [funding schedule](price-schedules-and-denominations.md).
`phloPrice` limits that compatible resource price, not the exchange ratio.
Quote input caps, provider-output caps, retained debit limits, and source exposure remain independent.
Passing one bound does not authorize exceeding another.

## Conservative branch and capacity bounds

Let $`\mathcal{B}`$ be the nonempty certified outcome family.
Let $`y_b`$ be the required output for outcome $`b`$, including its permitted retained work and applicable fee obligations.
Every $`y_b`$ must belong to the supported quote domain.
For one quote use with compatible source units, sufficient holds satisfy:

```math
H_{\mathrm{in}}\geq\max_{b\in\mathcal{B}}q(y_b),\qquad
H_{\mathrm{out}}\geq\max_{b\in\mathcal{B}}y_b.
```

Each hold must fit authenticated available capacity and its separate signed exposure limit.
Every realized debit must also fit its retained debit and fee caps.
Do not substitute $`q(\max_b y_b)`$ for the input maximum unless monotonicity over the relevant domain is established.
Piecewise fees or discounts can invalidate that shortcut.
Use complete branch evidence or a proved conservative bound rather than sampling likely outputs.

Multiple quote uses can share input custody, provider custody, or both.
Aggregate their obligations by physical asset and custody before proving capacity.
Different quote identifiers do not make the same provider balance independently available twice.
An operation's tentative output cannot finance a circular unbacked input commitment.
The complete funding proof must establish executable authorized backing, not only equal final totals.

Quotes must specify whether they permit one use or repeated uses under an identified capacity rule.
Quote identity alone is not a fresh operation identity.
Repeated permission does not authorize exceeding cumulative capacity or closing one accepted operation twice.
The native identity and publication contracts must establish this behavior without a global reservation table.

## Validity and captured context

Validity checks use authenticated execution context, not the verifier's wall clock.
Use inclusive validity endpoints for the new quote contract, consistent with the [grant validity contract](delegation-persistent-authority-contract.md#expiration-and-replay-protection).
Encode each endpoint and its presence explicitly. Reject inverted intervals and invalid integer representations.
Do not derive quote expiry implicitly from the deployment's expiry.

New quote use must be valid when its operation is admitted for publication under the applicable context.
Preparation alone does not retain a right to use expired or stale permission.
Rebuilding a candidate under changed relevant state requires fresh quote and capacity validation.
Unrelated state changes must not require a global latest-root match.

An already accepted obligation retains its quote, price rule, and original-source release terms.
Later expiry does not reprice or erase that accepted effect during historical replay.
Do not treat a replay as a new quote use or retrieve a replacement quote.
The [replay contract](price-transitions-and-replay.md) requires the exact accepted context and complete economic result.

## Composition and failure

| Composition or outcome | Required behavior |
| --- | --- |
| Separate conversion before funding | Commit the authorized trade first. A later funding failure does not undo it. |
| Atomic quote-backed funding | Publish conversion, funding, rights, approved charges, releases, allowances, and receipt through one native checkpoint. |
| Unused atomic input | Release it to its captured original asset and custody. Do not reverse-trade output at a current rate. |
| Unused provider capacity | Release it without creating an independently spendable intermediate output. |
| Preacceptance rejection or platform/certificate failure | Retain no candidate conversion, user charge, or conversion fee. |
| Classified user failure | Convert only the approved retained obligation within captured quote and funding limits. |
| Unsupported or invalid quote | Reject explicitly. Do not switch to a separate trade or a different provider automatically. |
| Duplicate accepted operation | Preserve the earlier result without another conversion, debit, or refund. |

The two compositions are separate signed choices, not fallback implementations of each other.
The separate deployment fee remains distinct from conversion fees.
A zero new resource-acquisition amount does not necessarily mean zero output when another fee obligation still needs funding.
The full settlement projection must identify which output funds each retained obligation.

## Current source and proof boundaries

[`Exchange.rhox`](../../../../casper/src/main/resources/Exchange.rhox) joins two carrier inputs and swaps their opaque payloads.
It has no priced quote parameter and does not directly debit SystemVault custody.
Stack transport remains subject to the separate native materialization checks.
Its one-for-one carrier swap must not be presented as the exact-output vault conversion specified here.

[`Exchange.v`](../../../../formal/rocq/cost_accounted_rho/theories/Exchange.v) and `ExchangeFlow.tla` cover carrier and resource-stack conservation.
The [exchange tests](../../../../casper/tests/genesis/contracts/exchange_spec.rs) cover swap behavior and one-sided release rejection.
Those artifacts do not prove quote authorization, numerical rates, provider solvency, expiry, or atomic multi-asset funding.
This contract requires separate refinement evidence for those obligations.

## Verification requirements

| Invariant | Required model, property, or negative control |
| --- | --- |
| Quote terms are authenticated | Mutate every amount, rule, fee, asset, authority, destination, context, and composition field without changing signatures. |
| Integer evaluation is exact | Compare each supported pricing rule with an independent integer oracle at division, fee, clamp, domain, and overflow boundaries. |
| Zero output has no conversion fee | Generate valid zero cases and quotes that attempt a minimum or fixed fee at zero. |
| Every outcome fits source exposure | Include nonlinear and nonmonotone examples. Detect the unsafe maximum-output-only shortcut. |
| Output capacity exists | Give two candidates distinct quotes against one provider purse. Detect duplicate available backing and circular unbacked output. |
| Repeated use remains bounded | Generate repeated permission, one-use permission, stale capacity, duplicate operation identities, and cumulative-cap exhaustion. |
| Expiry is deterministic | Check before, at, and after each endpoint during preparation, publication, and later historical replay. |
| Refunds preserve composition | Fail after acquisition and during settlement. Distinguish committed separate trades from atomic original-input releases. |
| Fairness uses compatible values | Vary input assets and quote rules. Reject comparisons of incompatible raw quantities or omitted authorized acquisition alternatives. |
| Concurrent publication remains exact | Interleave shared and disjoint providers, owner changes, top-ups, cancellation, settlement, and restart. |

Rocq must establish exact rule arithmetic, conservative bounds, and per-asset backing conservation.
TLA+ must model competing quote uses, capacity changes, publication, duplicate closure, and failures.
Property tests must cover the actual quote evaluator and independent bound calculations.
Loom must exercise native synchronization boundaries, followed by multi-deploy and independent-replay integration tests.
Report the supported rule grammar, finite generated ranges, and model assumptions explicitly.
No existing carrier-swap proof establishes this complete priced-conversion contract.
