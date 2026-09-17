# Resource bounds and exhaustion

## Purpose

A signed spending limit is consent, not proof that an operation has sufficient resources.
Acceptance requires both a sound resource bound and authorized backing for every covered outcome.
This contract applies the [resource measurements](resource-units-and-measurement.md), [funding policies](economic-activation-policy-ratification.md), and [signed phlo contract](signed-phlo-contract-proposal.md).
It does not add persistent deployment escrow or change Casper's consensus architecture.

## Independent limits

| Limit | Meaning | What it does not establish |
| --- | --- | --- |
| `phloLimit` | Maximum valued resource use for one signed execution scope under its captured schedule. | Sufficient funding, a lifetime process budget, or permission to charge the limit regardless of actual work. |
| `phloPrice` | Signed offered price, equal to the selected schedule price. | A conversion rate or contribution weight. |
| Owner price ceiling | Maximum permitted price under each applicable payer's captured consent. | Permission to use the largest owner's ceiling for everyone. |
| Resource-dimension caps | Independent limits on compute and byte measurements under the applicable protocol rules. | Authority compatibility or permission to exceed another dimension's cap. |
| Persistent allowance | Remaining authorized draws from a particular grant across executions. | Extra allowance from a wallet top-up or ownership transfer. |
| Retained debit cap | Maximum final debit permitted from a source for the accepted operation. | Permission to reserve additional temporary backing. |
| Exposure cap | Maximum temporary backing the source permits this operation to reserve. | Permission to retain the entire hold or redirect an unused remainder. |
| Protocol input caps | Limits on members, physical payers, obligations, branch representation, and encoded evidence. | A complete bound on search or live allocations. |
| Host-work budget | A bound on local verification, search, and allocation work. | A proof of insolvency when that budget is exhausted. |

Limits apply independently. Passing a scalar limit does not permit bypassing an authority, location, custody, or host-work constraint.
The separate deployment fee remains disclosed and bounded under the approved fee policy.
Wallet count must not multiply the same valued execution budget.
Additional metadata can still increase measured bytes, which the certificate must include.

## Conservative sufficiency

The rho paper's `def:conservative-demand` requires the maximum demand over covered branches, followed by a refund for unused funding.
Its located-purse refinement confines conservative sub-bounds to the surfaces that need them.
Its `rem:db-atomicity` distinguishes one atomic communication from a completely funded multi-step financial operation.

Let $`\mathcal{B}`$ denote the nonempty certified outcome family for the captured execution context.
For each outcome $`b`$, preserve its authority, location, resource measurements, complete funding assignment, and permitted retained economic effect.
The family can use a sound symbolic bound. It need not enumerate every outcome explicitly.
Its proof must cover every reachable outcome admitted by the certificate, including the retained effects of classified user failures.
An incomplete search result or an estimated typical branch does not establish this coverage.

Unknown data must have a sound conservative bound or authenticated causal evidence that fixes the relevant alternatives.
If neither is available, reject certification. Do not accept an underfunded atomic operation on the assumption that extra funds will arrive.
Just-in-time funding outside that certified atomic scope retains the paper's distinct interaction semantics.

## Maximum retained charge versus exposure

For physical source $`i`$, let $`d_{b,i}`$ be its certified retained debit in outcome $`b`$.
All amounts in a sum must use the same asset and compatible units.
For a fixed common source inventory, the conservative hold and maximum retained total are:

```math
H_i=\max_{b\in\mathcal{B}}d_{b,i},\qquad
M_{\max}=\max_{b\in\mathcal{B}}\sum_i d_{b,i}.
```

Let $`A_i`$ be available authenticated capacity, $`E_i`$ the signed exposure cap, and $`R_i`$ the retained debit cap.
Require:

```math
H_i\le A_i,\qquad H_i\le E_i,\qquad
d_{b,i}\le R_i\quad\text{for every certified }b.
```

The total hold $`\sum_i H_i`$ can exceed $`M_{\max}`$.
It is not another retained charge. Source-specific restrictions prevent replacing all holds with one aggregate balance test.
Before computing these bounds, merge physical aliases and aggregate their exposure across all applicable obligations.
Keep different assets, custody roles, and network contexts distinct.

For example, one branch requires one unit from A and another requires one unit from B.
The maximum retained charge is one, but conservative backing requires holds of one from A and one from B.
Both sources must authorize that exposure.
Otherwise, certification needs a sufficient authorized common source or valid branch-fixing evidence, or must reject.
The implementation cannot silently increase either source's exposure or retain both units.

For a realized outcome $`b`$, unused backing is $`H_i-d_{b,i}`$ at each captured source.
Prepaid rights and quoted conversions require their additional provenance rules, not arbitrary subtraction of incompatible quantities.
Atomic exact-output conversion also requires sufficient provider-output capacity for every covered outcome.
Its input and provider holds remain candidate or proof state, not independently spendable assets.

## Planning and acceptance sequence

1. Validate the signed envelope, scope, schedule, permissions, and independently bounded input sizes.
2. Resolve authenticated resource locations and physical custody without counting aliases twice.
3. Establish the complete certified outcome family or its sound symbolic bound.
4. Check resource limits, compatible prepaid backing, and each applicable persistent allowance.
5. Compute complete permitted assignments using the approved valuation and allocation policy.
6. Verify every retained debit and source-specific hold against captured capacity and consent.
7. Preserve the certificate and its causal inputs for independent validation and replay.
8. Execute within the existing native transaction boundary and derive the permitted realized economic result.
9. Publish that result atomically, including fees, refunds, allowances, and required cursor changes.

An available capacity increase does not amend an already captured reservation.
A new authorized plan can use a later top-up only under the applicable causal and identity rules.
Ownership changes cannot replace the original refund source or recreate consumed allowance.
Disjoint custody must remain concurrent. Shared custody requires valid conflict handling, not a new global reservation lock.

## Exhaustion and failure classification

| Outcome | Required treatment |
| --- | --- |
| Invalid dimensions, identity, consent, price, or protocol limit | Reject admission without candidate user charges or partial economic publication. |
| Independently verified funding deficit | Reject as infeasible for the authenticated problem and its stated constraints. Preserve the deficit witness. |
| Search-work exhaustion or allocation failure | Stop local work without claiming infeasibility, optimality, or authority insufficiency. Publish no candidate user debit. |
| Arithmetic overflow | Reject the affected computation. Never wrap, saturate, truncate, or lower a bound to produce an acceptable result. |
| Discovery trial exhausts provisional funding before certification | Discard trial effects. Further discovery may proceed only within authorized host-work bounds and unchanged-input rules. |
| Accepted execution exceeds a certified sufficient bound | Reject the candidate as a certificate-correctness failure. Do not charge extra funds or misclassify the result as ordinary user exhaustion. |
| Missing local capability outside a certified atomic scope | Do not fire that interaction or charge an unfired COMM. Preserve its applicable located state. |
| Classified deterministic user failure | Retain only permitted billable work and the separate fee within captured limits. Apply the approved application rollback and refund rules. |
| Platform, storage, settlement, or publication failure | Publish no partial economic result. Restore the applicable pre-state or enter fail-closed recovery if restoration fails. |
| Duplicate operation or retry after commitment | Preserve the existing operation identity and apply no second charge or refund. |

Mixed errors follow the approved precedence: platform, certificate, and unknown failures override user-failure charging.
Classification must inspect nested errors and late settlement failures, not only the first returned error.
Rollback must not restore a consumed prepaid right while retaining credit for that consumption.
It must also preserve unused rights and captured refunds.

Protocol rejection and local inability to finish verification remain distinct outcomes.
Local resource limits must not silently change another validator's accepted historical semantics.

## Current source boundaries

The [feasibility solver](../../../../rholang/src/rust/interpreter/accounting/monetary_allocation/funding_feasibility.rs) separates `Feasible`, `Infeasible`, and `FundingSearchError`.
It includes source and obligation caps, checked graph dimensions, host-work reservation, and allocation-failure reporting.
An independently checked feasible assignment is not by itself a proof of the approved minimax optimum or complete branch coverage.

The [branch reservation helper](../../../../rholang/src/rust/interpreter/accounting/monetary_allocation/funding_reservation.rs) computes per-source maxima from supplied branch plans.
It distinguishes a `u128` total hold from a `u64` maximum retained charge.
It checks source capacity and exposure and computes refunds against the selected supplied branch.
It does not prove that the supplied branch set covers all native execution outcomes or authenticate its source order by itself.
It also does not implement concurrent native reservations or the complete signed phlo lifecycle.

The resource, allowance, deficit, and search models cover separate abstract obligations.
Refinement must bind each model to signed inputs, actual branch evidence, native failure classification, and publication.
Helper acceptance cannot substitute for that complete correspondence.

## Verification requirements

| Invariant | Required property or negative control |
| --- | --- |
| Every admitted outcome fits its bound | Generate branch families and independently evaluate their maximum resource and source demands. Reject omitted reachable branches. |
| Exposure differs from retained charge | Preserve the A-only/B-only example. Generate disjoint and overlapping source restrictions with more than two payers. |
| Limits are independent | Vary resource, debit, hold, allowance, source, branch, byte, and work caps separately. Detect silent truncation and cap substitution. |
| Search outcomes remain honest | Exhaust work at each search phase. Distinguish valid deficit witnesses from interrupted or incomplete search. |
| Arithmetic preserves capacity | Exercise maximum-width values and overflowing totals, dimensions, rates, and reservations. Check mutation-free failure. |
| Capture remains stable | Interleave top-ups, ownership transfers, stale candidates, changed schedules, and exact original-source refunds. |
| Failure projection conserves backing | Inject user, platform, mixed, and late settlement failures across prepaid acquisition, consumption, and publication. |
| Publication is atomic | Use actual concurrent shared-custody boundaries with duplicate delivery, cancellation, reset failure, and restart. |
| Replay preserves acceptance | Recompute the complete certificate and realized result under its captured limits and causal state on independent validators. |

Extract example and property tests from each formal invariant with independent reference calculations.
Use Loom for real synchronization boundaries and native integration tests for durable state and independent-validator behavior.
Keep finite bounds and abstraction assumptions explicit. Do not claim unbounded branch coverage from bounded enumeration alone.
