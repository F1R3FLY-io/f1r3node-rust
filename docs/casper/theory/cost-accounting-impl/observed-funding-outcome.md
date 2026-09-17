# Observed funding outcome

## Contract

The funding policy contains a prepared family of possible execution outcomes.
Settlement must select an outcome from actual execution evidence, not from a caller-supplied branch index.
Equal total costs do not establish that two executions used the same authority or resources.

`CheckedNativeSignedPhloFamilyPolicy::capture_matching_execution` matches supplied checked evidence against the complete prepared family.
It returns the existing canonical capture only when every matching case has the same complete settlement.
It does not replace source permissions, choose new funding assignments, or modify either cursor.
The matcher performs no wallet mutation and creates no escrow state.

## Complete execution identity

Each resource key contains its location, class, acquisition terms, and complete authority tree.
The canonical resource encoding distinguishes every field and authority node.
The matcher sorts encoded keys and sums quantities for identical keys with checked arithmetic.
Resource order and entry grouping can change, but multiplicity cannot change.
The occurrence-based input assigns quantity one to each entry. The counted input supplies an explicit positive quantity.

| Evidence | Required comparison |
| --- | --- |
| Controls | Full checked controls, selected schedule, signed terms, minimum price, and derived bounds. |
| Available resources | Complete prepaid supply multiset. |
| Required resources | Complete demand multiset. |
| Used resources | Complete consumed prepaid multiset. |
| Unused resources | Complete remaining prepaid multiset. |
| Fresh resources | Complete newly acquired resource multiset. |
| Outcome | Rejected or accepted, with the exact accepted failure-class set. |

The [failure summary](economic-failure-observation.md) retains user, platform, certificate, and unclassified failures.
Reordered or repeated failure reports do not change their class set.
An accepted result with no failures differs from an accepted user failure, even if their charges are equal.
An admission rejection also differs from an accepted platform failure.

For example, one resource at location A can cost the same as one resource at location B.
The matcher rejects that substitution because the location is part of the key.
Extra zero-weight resources also change the identity, even when total usage and cost remain unchanged.

## Equivalent and conflicting cases

Several family entries can describe the same execution.
The matcher accepts these equivalent entries only when their complete canonical captures agree.
It compares:

- Canonical source identities, capacities, signed caps, holds, debits, fees, and refunds.
- Canonical obligation keys and amounts.
- Eligibility and exact assignment matrices.
- Both scoped resource and fee cursor transitions.

These records come from the verified family policy.
Policy verification recomputes the canonical assignment over the complete quantities of identical obligations.
The weaker checked-intent type alone does not establish that canonical assignment invariant.

Matching only final payer totals would be insufficient.
Two zero-charge failure cases can have different eligibility matrices while their debits and cursor transitions are equal.
The matcher rejects that ambiguity rather than letting input order select a permission set.

When all matching captures agree, the first matching index identifies an equivalent representation only.
It does not give that index a different economic result.
A later conflicting case invalidates the match.

## Bounded matching algorithm

The matcher checks the total case count before scanning.
It reserves host work for classification, resource encoding, sorting, comparison, and capture construction.
Each normalized execution also has resource-entry, authority-node, raw-key-byte, and encoded-key-byte limits.
Exhaustion returns an error, even if an earlier case matched.

```text
Check the complete family size against the case limit.
Normalize the observed outcome and all five resource partitions.
For each prepared case:
    Compare its exact outcome and complete execution identity.
    If it matches, capture its already verified canonical settlement.
    Reject a capture that differs from an earlier matching capture.
Reject if no case matched.
Return the shared capture after the complete scan succeeds.
```

Normalization sorts each partition with bounded merge sort.
For a partition with $`n`$ supplied entries, sorting uses $`O(n \log n)`$ key comparisons.
The matcher does not expand a counted quantity into individual entries.
The host budget also charges the bytes inspected by comparisons.
The implementation retains only the observed execution, the current candidate, and the first matching capture.

## Proof and test scope

[`ObservedPhloFamilyMatch.v`](../../../../formal/rocq/cost_accounted_rho/theories/ObservedPhloFamilyMatch.v) proves exact matching and matched-index membership.
It proves equivalent-duplicate acceptance, conflicting-duplicate rejection, incomplete-scan rejection, multiplicity preservation, non-user charge veto, and source-permission preservation.
Its settlement extraction premise requires the immutable, verified family policy.
Its resource model contains complete typed keys, not scalar prices alone.

Rust property tests check all five multiset partitions, permutations, zero-weight mutations, typed identity changes, failure masks, and work limits.
The signed-family fixture checks actual canonical captures, repeated resource quantities, cursor transitions, and conflicting eligibility.
These tests connect the matcher to the checked Rust policy types.

The [counted-resource contract](resource-units-and-measurement.md#counted-resource-representation) defines quantity validation and representation equivalence.
Generated tests compare counted and expanded forms across all five partitions.
Quantity changes remain observable even for zero-weight resources.

## Native producer boundary

The matcher checks supplied evidence. It does not establish where that evidence originated.
Complete native settlement also requires the following producer contract:

1. Preserve raw introduction, delivery, and trace quantities at their accepted observation points.
2. Retain event identity, authority, multiplicity, and evaluation ownership with those quantities.
3. Represent large byte quantities as counted resources, without one allocation per byte.
4. Authenticate prepaid class, original acquisition terms, and backing provenance at the captured state root.
5. Preserve that provenance through issuance, transfer, consumption, failure, and rollback.
6. Derive all five resource partitions from those authenticated inputs.
7. Reproduce the same evidence during independent replay before atomic settlement.

Current `CostStack` cells contain signatures, not resource classes or acquisition terms.
Current `AuthorityByteEvent` records contain weighted amounts, not separate raw delivery and trace quantities.
The interpreter now retains those quantities separately as [raw byte observations](raw-byte-observations.md).
Neither representation alone satisfies the producer contract.
Current prices must not replace missing prepaid terms, and wallet balances must not become synthetic prepaid resources.

Protected receipt metadata can retain provenance without creating another spendable ledger or changing historical protobuf encodings.
Receipt mutation must share the existing owned checkpoint with stack and wallet effects.
Missing or incompatible provenance in the activated economy must reject, rather than silently use legacy or current-price defaults.
The [resource measurement contract](resource-units-and-measurement.md) and [prepaid provenance contract](conversion-provenance-and-refunds.md) define these required boundaries.

Implementation and regression sources:

- [Matcher](../../../../rholang/src/rust/interpreter/accounting/phlo_execution/outcome_match.rs)
- [Matcher properties](../../../../rholang/src/rust/interpreter/accounting/phlo_execution/outcome_match/tests.rs)
- [Signed-family regressions](../../../../rholang/src/rust/interpreter/accounting/phlo_execution/consent/tests/wire_controls_family.rs)
