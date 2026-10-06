# Canonical funding assignment witnesses

## Purpose and boundary

An assignment witness records which physical source pays each obligation.
Different assignment matrices can have the same wallet contributions and the same minimax rank.
The [approved allocation policy](authority-allocation-policy-ratification.md) requires canonical selection among semantically equivalent witnesses after the contribution decision.

Canonical ordering must bind source identity, obligation identity, resource compatibility, and captured provenance.
Search order and hash-map iteration cannot select the witness.
Different ownership effects, retained rights, conversion terms, or execution outcomes are not witness ties.
Canonical ordering must not silently choose between those semantic alternatives.

This document describes the bounded-entry query and the complete matrix selector for a supplied row and column order.
The selector does not establish semantic equivalence or authenticate identities.
It does not change wallet totals, publish state, create a wallet, or transfer authority.
The identity-order adapter supplies deterministic positions from already resolved keys.
Authentication, typed-key construction, and witness encoding require separate integration.

## Checking a submitted allocation

`CanonicalFundingProblem::verify_assignment` checks a submitted matrix against a fresh deterministic policy selection.
The input matrix uses the original source and obligation positions.
The cursor uses the canonical physical-source order.
The verifier compares every mapped entry and the proposed next cursor against the selected result.
It does not accept feasibility or an equal sorted contribution vector as sufficient evidence.

For example, two unrestricted wallets can feasibly pay three units as `[3,0]`.
That plan fails when the selected fair contributions are `[2,1]`.
The equally ranked `[1,2]` also fails if the captured cursor selects `[2,1]`.
A different feasible matrix with identical wallet totals fails if it violates the canonical witness rule.

The method returns `true` only for the recomputed matrix and cursor.
It returns `false` for malformed dimensions, a different result, or a problem without a feasible allocation.
A `false` result rejects the submitted allocation. It does not classify the rejection as insufficient wallet funding.
Work exhaustion and solver errors remain errors rather than a weaker acceptance rule or an insufficient-funds result.
The verifier charges its dimension checks and full entry comparison to the host-work budget.

The caller must authenticate the captured capacities, eligible edges, obligation identities, and cursor before this check.
The verifier does not determine execution branches or combine the separate deployment-fee and new-resource funding policies.
`funding_presentation_matches_exact` in `FundingIdentityOrder.v` proves the exact mapped-entry comparison for arbitrary finite dimensions.
`funding_presentation_rejects_different_entry` proves rejection when any mapped entry differs.
These comparison proofs depend on the separately verified selector and identity permutations. They do not establish those premises themselves.

Native regressions reject unfair but feasible contributions, the wrong cyclic tie, and a noncanonical matrix with equal contributions.
Generated cases compare submitted matrices and cursors with recomputed selections across reversed source and obligation orders.
The existing permutation tests also verify restored submissions at every tested cursor.
Zero-obligation checks include cohorts of 1, 2, 3, 64, 65, and 129 sources and reject an invented cursor update.
The budget regression checks every insufficient verification-operation prefix for its fixture and requires a structured work-exhaustion error.

## Exact contribution problem

Let $`n`$ denote the number of physical sources and $`m`$ the number of obligations.
Let $`d_i`$ be the already selected contribution of source $`i`$.
Let $`q_j`$ be obligation $`j`$, and let $`E_{ij}`$ permit source $`i`$ to pay that obligation.
An assignment $`x_{ij}`$ must satisfy:

```math
x_{ij}\ge0,\qquad
\sum_jx_{ij}=d_i,\qquad
\sum_ix_{ij}=q_j,\qquad
\neg E_{ij}\Longrightarrow x_{ij}=0.
```

The selected contributions and obligation amounts have the same total.
They are not the original wallet capacities.
This distinction prevents witness selection from changing an earlier contribution decision.

For source $`a`$, obligation $`b`$, and nonnegative bound $`u`$, the query asks whether an assignment exists with $`x_{ab}\le u`$.
An upper bound does not require that entry to equal the bound.
Define $`k=\min(u,d_a)`$.
Clamping at the selected contribution removes a redundant bound without changing feasibility.

## Exact bounded-flow reduction

The reduction temporarily represents source $`a`$ with two internal flow rows.
It does not create two physical sources.

| Internal row | Capacity | Permitted obligations |
|---|---|---|
| Original source $`a`$ | $`k`$ | Every original eligible obligation. |
| Auxiliary row | $`d_a-k`$ | Every original eligible obligation except $`b`$. |
| Each other source $`i`$ | $`d_i`$ | Unchanged original eligible obligations. |

The internal capacities still sum to the total obligation.
Any complete feasible flow therefore fills each internal capacity exactly.
The auxiliary row cannot pay obligation $`b`$, so that entry cannot exceed $`k`$.

After a successful query, add the auxiliary row back to source $`a`$ and remove the auxiliary row.
Independently verify the resulting matrix against the original eligibility relation, exact contributions, obligations, and entry bound.
The returned matrix contains exactly the original physical source count, in the original source order.

The converse is also constructive.
Given an original bounded assignment, retain its target entry in the first internal row.
Fill the remainder of that row from its other entries until its total is $`k`$.
Put the remaining amounts in the auxiliary row.
The original row total guarantees enough remaining contribution, and neither internal row needs an originally forbidden edge.

[`FundingCellBound.v`](../../../../formal/rocq/cost_accounted_rho/theories/FundingCellBound.v) proves both constructions and their equivalence.
`funding_cell_bound_reduction_exact` covers arbitrary finite source and obligation counts.
Its premises include equal total contributions and demand.
No assumption about search order or a particular feasible witness supplies completeness.

## Native query and evidence

[`FundingCellQuery`](../../../../rholang/src/rust/interpreter/accounting/monetary_allocation/funding_cell_bound.rs) contains the exact contribution problem, source index, obligation index, and maximum entry amount.
The `capacities` field in this query's `FundingMinimaxProblem` holds exact contributions.
The query rejects unequal aggregate contributions and obligations.
Ordinary fixed-flow and minimax problems can have spare capacity, but this exact-contribution query cannot.

```text
validate physical dimensions, configured limits, indices, and equal totals
clamp the entry bound at the selected source contribution
construct the two internal capacity parts and restricted auxiliary row
solve the internal fixed-flow problem
if feasible:
    rejoin the original source
    verify the complete original assignment and requested bound
else:
    reconstruct the query and independently check its deficit certificate
return the verified result
```

`solve_funding_cell_bound` returns a valid assignment or a checked deficit for the bounded-entry query.
That deficit does not mean the original funding problem is economically infeasible.
It means the additional entry bound prevents an assignment.
The caller must retain this distinction during canonical selection.

`check_funding_cell_deficit` rebuilds the internal query from the original inputs before it checks the selected obligation cut.
Changing the source, obligation, maximum amount, or captured graph requires a new valid check.
The checker does not trust a supplied auxiliary capacity or eligibility row.

The physical source limit applies before the internal row is added.
The flow solver receives an internal limit for exactly one additional row.
The host-work budget covers its allocation, search, and verification work.
No extra physical funding source is authorized by that internal limit.

Amounts and their aggregate obligation must fit unsigned 64-bit integers.
Malformed inputs, unequal totals, arithmetic overflow, allocation failure, and exhausted work budgets return errors.
Those errors are not deficit certificates and cannot justify a different assignment policy.
Inputs are immutable, and each query owns its temporary search state.

## Use in canonical selection

The query is monotone in its maximum entry amount.
If an assignment exists for one bound, the same assignment exists for every larger bound.
`funding_cell_query_is_monotone` proves this property through the exact reduction.
It permits binary search without a per-token loop.

A deficit at bound $`u`$ proves that every otherwise valid assignment has $`x_{ab}>u`$.
`funding_cell_cut_proves_lower_bound` derives this result from the checked flow-deficit theorem.
Such a certificate can substantiate an entry minimum after previously chosen entries are fixed.
The selector preserves exactly that prefix and leaves later entries free.

## Complete matrix selection

[`select_fixed_funding_witness`](../../../../rholang/src/rust/interpreter/accounting/monetary_allocation/funding_witness.rs) selects the lexicographically smallest row-major matrix for the supplied source and obligation order.
Row-major order visits every obligation of the first source, then every obligation of the next source.
This order minimizes assignment entries, not source contributions.
The earlier minimax and cyclic contribution decisions remain fixed.

For example, two sources each contribute one unit to two unit obligations.
If every eligibility edge is permitted, both of these matrices are feasible:

```text
diagonal:      [[1, 0], [0, 1]]
off-diagonal:  [[0, 1], [1, 0]]
```

The selector returns the off-diagonal matrix because its first entry is smaller.
Both matrices charge each wallet exactly one unit.
An eligibility restriction can require the diagonal matrix instead.

The selector maintains a residual problem after each fixed entry.
For a selected amount $`v=x_{ab}`$, it subtracts $`v`$ from the remaining contribution of source $`a`$ and obligation $`b`$.
It removes only that entry's eligibility edge and sets the working entry to zero.
The other entries remain available to subsequent queries.

```text
validate dimensions, limits, and exact contribution totals
find an original feasible assignment or return its checked deficit
for each entry in row-major order:
    binary-search its minimum in the current residual problem
    retain a feasible assignment for the selected minimum
    if the minimum is positive:
        require a checked deficit at one less than the minimum
    record the selected amount and its evidence
    subtract that amount from the residual row and column
    remove only the selected entry from the residual problem
check the complete original assignment
independently check every prefix certificate
return the matrix, unchanged contribution totals, and certificate
```

Binary search starts with zero and the entry amount in a known feasible assignment.
Each decision reduces the interval, so an unsigned 64-bit entry needs at most 64 binary decisions.
Positive entries also need a final predecessor query for their certificate.
Each query uses the bounded-flow solver and its host-work limits.
There is no per-token search loop.

## Prefix certificate and uniqueness

`FundingWitnessCertificate` contains exactly one entry certificate for each matrix position.
A zero entry needs only its zero value because all assignments have nonnegative amounts.
A positive entry needs a checked deficit at one less than its selected amount.
That deficit must use the residual problem defined by earlier entries only.

`check_fixed_funding_witness` reconstructs every residual problem from the original graph and candidate matrix.
It does not accept residual capacities, eligibility rows, or frozen future entries from the certificate.
It rejects missing or extra certificates and incorrect zero evidence.
Its final residual contributions and obligations must all equal zero.

[`FundingWitnessPrefix.v`](../../../../formal/rocq/cost_accounted_rho/theories/FundingWitnessPrefix.v) proves that erasing an entry preserves the residual assignment and equal totals.
It also proves the converse reconstruction under the original entry bounds and eligibility.
Thus, a fixed prefix neither loses compatible assignments nor invents unauthorized assignments.

`checked_witness_prefix_is_lexicographically_least` combines the predecessor deficits with those residual invariants.
It compares the candidate with every valid assignment, not only assignments found by the search.
At the first differing entry, the checked minimum excludes a smaller alternative.
When entries match, both assignments project to the same remaining funding problem.

The row-major traversal proofs cover arbitrary finite dimensions and establish complete coverage without duplicate entries.
`checked_row_major_witnesses_are_identical` proves equality of every in-range entry for two valid, certified candidates.
The certificate's selected cuts can differ even when the resulting matrix is identical.
This matrix-uniqueness result does not establish unique serialized certificate bytes.

Matrix positions alone cannot establish canonical source, obligation, or provenance identities.
Reordering a bare matrix can change its selected witness because it changes the comparison order.
The identity-order adapter resolves this positional ambiguity before selection.

## Identity-order adapter

[`canonicalize_funding_problem`](../../../../rholang/src/rust/interpreter/accounting/monetary_allocation/funding_identity.rs) accepts the fixed-flow graph and a complete key for every source and obligation occurrence.
It returns an owned canonical graph with borrowed, immutable keys and inverse presentation mappings.
The adapter orders both key sequences by ascending byte order.
It applies the same row and column permutations to capacities, amounts, and eligibility.

Source keys must identify physical custody after authenticated alias resolution.
Distinct roles, assets, contracts, or shard contexts must remain distinct when they identify different custody.
Repeated source keys reject, even when their capacities match.
Canonical sorting must not count two views of one balance as two funding sources.

Obligation keys must include the complete applicable resource identity and captured provenance.
The [resource-key format](../../../../models/src/rust/phlo_resource.rs) distinguishes location, class, acquisition terms, and authority.
The [custody contract](authority-custody-identity-contract.md) specifies the separate physical identity requirements.
Key construction must not flatten these fields or replace authenticated custody with an unqualified wallet address.

Repeated obligation keys retain separate columns and their full multiplicity.
Each repeated key must have the same amount and eligibility column.
Otherwise, the adapter rejects an inconsistent obligation rather than choosing one interpretation.
Equal repeated columns remain interchangeable only within the caller's established semantic-equivalence contract.
Sorting does not establish that contract or authorize different resource effects.

```text
validate source, obligation, identity, and matrix dimensions
reject empty keys
derive ascending source and obligation index orders
independently check ordered keys and complete index permutations
reject duplicate physical-source identities
require equal amounts and permissions for repeated obligation keys
copy the graph through both checked permutations
select the funding policy with the cursor in canonical source order
retain canonical keys alongside the complete selected matrix
restore original presentation indices only when required by a caller
```

The cursor position refers to canonical source order, not the caller's original row order.
The caller must obtain that cursor from the corresponding authenticated cohort scope.
The adapter does not create a scope, change a cursor revision, or publish a transition.

`CanonicalFundingProblem::select` calls the composed fixed-flow policy on the canonical graph.
`restore_assignment` maps matrix entries back to the original source and obligation positions without changing amounts.
It checks dimensions, not funding validity.
The caller must validate a restored settlement against its captured original graph before publication.
The returned source keys provide the custody association for each canonical row.

The index sorter uses iterative merge passes with two index arrays.
For $`k`$ keys, sorting requires $`O(k\log k)`$ comparisons and $`O(k)`$ auxiliary index storage.
Each byte comparison reserves work for its maximum compared prefix before reading that prefix.
The graph copy requires $`O(nm)`$ work and storage.
Allocation and work-budget failures return errors without a partial canonical graph.
The sorter independently checks its result for ordered keys, bounded indices, and duplicate indices.
The equal input and output lengths then establish complete occurrence coverage.

[`FundingIdentityOrder.v`](../../../../formal/rocq/cost_accounted_rho/theories/FundingIdentityOrder.v) proves that index permutations preserve sums, source draws, obligation draws, and assignment validity.
Its inverse-permutation theorem restores each original entry exactly.
These theorems treat identities through their index mappings.
They do not prove cryptographic identity resolution, native sorting implementation correspondence, or the semantic completeness of opaque key bytes.
The adapter rejects malformed dimensions and duplicate custody, but it cannot authenticate an arbitrary supplied key.
Production integration must construct keys from checked native and wire records, not trust proposer-supplied opaque labels.

## Typed obligation keys

[`PhloObligationKeyV1`](../../../../models/src/rust/phlo_obligation.rs) supplies a versioned key for either the fee or one resource occurrence.
Each field uses the existing unsigned 64-bit, big-endian length framing.

| Field | Fee key | Resource key |
|---|---|---|
| Format domain | `f1r3node:phlo-obligation:v1` | The same obligation format domain. |
| Kind | One byte with value zero. | One byte with value one. |
| Payload | Empty. | One complete `PhloResourceKeyV1` encoding. |

The nested resource encoding retains location, resource class, captured acquisition terms, and the complete supported authority tree.
It does not substitute the current owner or current acquisition terms.
Fee keys sort before resource keys because their common framing precedes the different one-byte kind.
The decoder rejects other kind values, other kind widths, nonempty fee payloads, trailing data, and invalid nested resource encodings.
Encoding bounds the nested resource by both the field limit and the remaining outer byte allowance.

[`CheckedPhloObligations::encoded_keys`](../../../../rholang/src/rust/interpreter/accounting/phlo_execution/obligations.rs) derives these keys from the checked native obligation list.
It retains the fee first and one column per distinct fresh resource key, in first-appearance order.
Each column retains the summed resource quantity and its exact monetary amount.
Repeated resources do not disappear. Their positive quantities add with checked arithmetic.
Nonbillable outcomes retain their identities even though their projected amounts equal zero.
Source permissions apply to complete resource keys, not individual interchangeable occurrences.
Different authority, location, class, or acquisition terms prevent grouping.
Callers must construct assignment and eligibility matrices against these projected columns.
The checker rejects matrices with obsolete occurrence-based dimensions.

The native method enforces a caller-supplied aggregate encoded-byte allowance.
Its authority-node allowance applies across the complete key list and cannot exceed the checked execution's allowance.
The method also retains the checked execution's aggregate resource-key byte bound.
A per-key limit cannot reset those aggregate budgets for each repeated occurrence.
Any encoding or aggregate-limit failure returns no key list.

### Encoding size and host work

Resource and obligation encoders validate their nested fields and calculate the complete encoded size before allocation.
The prepared encoding borrows the immutable record and retains its checked sizes.
Encoding then requests one output buffer and writes nested fields directly into that buffer.
It does not construct separate buffers for each authority node or nested resource.

Let $`p`$ denote an authority node's payload length and $`N`$ its authority list.
The format uses eight-byte field lengths and a four-byte node count.
Each node's inner encoding requires $`17+p`$ bytes.
The authority list requires the following bytes, including each node's outer field header:

```math
L_N=12+\sum_{n\in N}(25+p_n).
```

The exact-size theorems cover nodes, authority lists, resources, and obligations.
The streamed-encoding theorems prove byte equality with the nested format definitions.
Direct encoding therefore preserves the existing keys rather than introducing another wire format.

`encoded_keys_with_budget` charges authority traversal, vector growth, sizing passes, and encoded output before the corresponding work.
The traversal stack and authority vector use explicit logical capacities with geometric growth.
Removing a stack entry retains its reserved capacity.
Reusing that space does not charge another buffer allocation.
Capacity decisions do not depend on allocator-specific excess capacity.

The capture calls this budgeted method rather than charging the entire configured encoded-byte ceiling.
Increasing that ceiling without changing the record leaves measured host-work charges unchanged.
Allocation errors and exhausted budgets return no encoded key list or capture.
Memory charges describe requested buffer storage, not allocator metadata or total process memory.
This helper does not establish a process-wide resident-memory bound.

The regressions compare direct encoding with an independent nested-framing oracle and check exact prepared lengths.
They also check output-buffer stability, every measured budget prefix, compound occurrences, and generated stack push/pop sequences.

The typed keys can feed the canonical graph adapter with the corresponding projected amounts and eligibility matrix.
The native regression checks this path and confirms that the fee remains distinct and repeated resource demand remains present.
This path does not merge the fee and acquisition policies or select a new settlement policy.
It supplies exact identities for their existing obligation records.

[`SignedPhloObligationKey.v`](../../../../formal/rocq/cost_accounted_rho/theories/SignedPhloObligationKey.v) proves encoding injectivity under explicit field-length and integer-width bounds.
The proof composes the existing length-framing and resource-encoding theorems.
Distinct fee and resource kinds cannot alias, and equal valid resource encodings preserve every resource component.
List encoding preserves occurrence count.
These properties do not authenticate custody, consent signatures, or the native resource-to-wire conversion by themselves.

Tests cover independent outer framing, round trips, every truncated prefix, invalid kinds, trailing bytes, nested limits, and 512 generated resource records.
Native projection tests compare decoded keys with checked resource records and enforce aggregate limits for billable and unbilled outcomes.
Source-custody authentication, deployment wire binding, and atomic publication remain separate requirements.

## Checked funding capture

[`CheckedPhloFundingIntent::capture_case`](../../../../rholang/src/rust/interpreter/accounting/phlo_execution/capture.rs) binds canonical ordering to a checked funding family and its decoded intent.
A funding family contains alternative execution cases under the same checked controls.
Each case can contain a different number of resource obligations.
The capture retains the complete family, not only the selected case.

The capture preserves these values for each physical source:

| Value | Meaning |
|---|---|
| Custody | The original source identity. |
| Capacity | The observed balance supplied to the family checker. |
| Exposure and debit limits | The original source limits checked against the funding intent. |
| Hold | The maximum source debit across all family cases. |
| Debit | The source debit in the selected case. |
| Fee | The original fee-column contribution in that case. |
| Acquisition | The selected debit minus the fee. |
| Refund | The full-family hold minus the selected debit. |

The capture reorders the existing assignment and eligibility matrix with the same checked permutations.
It then checks the reordered assignment against the reordered capacities, amounts, and eligibility.
It does not select a new assignment or change the fee distribution.
Canonical order describes identity order here, not proof of optimal allocation.

Let $`B`$ contain the family cases, and let $`m_b`$ denote the obligation count for case $`b`$.
Let $`x_{bij}`$ denote the contribution from source $`i`$ to obligation $`j`$ in case $`b`$.
For selected case $`s\in B`$, define the source debit, hold, and refund as follows:

```math
d_{bi}=\sum_{j=0}^{m_b-1}x_{bij},\qquad
h_i=\max_{b\in B}d_{bi},\qquad
r_{si}=h_i-d_{si}.
```

The selected debit cannot exceed the hold, so $`h_i=d_{si}+r_{si}`$ without subtraction underflow.
Permutations preserve these amounts for the same source, including cases with different obligation counts.
The variable-branch theorems in `FundingIdentityOrder.v` establish both preservation and the debit-plus-refund identity.
They require valid column permutations for each case.
They do not require equal branch sizes or equal wallet balances.

The native regression uses decoded controls and source policies, unequal source limits, and fee permissions that differ by source.
It checks cohorts through 129 sources and compares original and reversed source order.
The charged case has four obligations, while the nonbillable case has two.
The nonbillable case refunds the complete original hold despite its smaller obligation list.

Checked consent validates the supplied permissions against the decoded record.
The capture does not authenticate signatures, establish complete authority-derived eligibility, resolve custody aliases, or publish vault changes.
Those checks must precede or follow capture at their respective integration boundaries.
The encoding and matrix operations have configured limits and host-work checks.
These algebraic proofs do not establish an exact bound for every native allocation or encoding operation.

## Verification and regression coverage

| Requirement | Formal result | Native regression |
|---|---|---|
| Split only existing row amounts | `funding_priority_row_is_bounded` | Returned assignments pass the original eligibility and contribution checker. |
| Retain the target entry | `funding_priority_row_preserves_target` | Bounds zero, one, exact capacity, and maximum width. |
| Fill the internal first row exactly | `funding_priority_row_has_exact_total` | Complete bounded-entry oracle. |
| Preserve every obligation | `funding_cell_split_preserves_columns` and `funding_cell_join_preserves_columns` | Independent exact row and column validation. |
| Preserve original authority edges | `funding_cell_split_is_valid` and `funding_cell_join_is_valid` | Diagonal-only funding cannot use the auxiliary row to bypass eligibility. |
| Preserve feasibility in both directions | `funding_cell_bound_reduction_exact` | All 3,648 balanced two-source queries with amounts from zero through two. |
| Support increasing entry bounds | `funding_cell_query_is_monotone` | Exhaustive bound enumeration and changed-bound deficit checks. |
| Justify an entry lower bound | `funding_cell_cut_proves_lower_bound` | Independent deficit checks and altered source/limit controls. |
| Preserve each remaining funding problem | `erasing_funding_entry_preserves_residual_assignment` and `entry_residual_totals_remain_equal` | Validate row totals, column totals, and removed edges after every selected prefix. |
| Reconstruct a fixed entry | `restoring_funding_entry_preserves_assignment` | Compare against every complete assignment in the bounded oracle. |
| Leave future entries unconstrained | `checked_witness_prefix_is_lexicographically_least` | Reject a deficit fabricated by freezing future entries. |
| Select the global matrix minimum | `checked_witness_prefix_is_lexicographically_least` | All 304 balanced two-source graphs with amounts zero through two. |
| Cover each matrix entry once | `funding_row_major_order_visits_each_entry_once` and `funding_row_major_order_is_complete` | Empty obligations and complete entry-certificate counts. |
| Establish unique matrix results | `checked_row_major_witnesses_are_identical` | Reject every other feasible oracle matrix with the selected certificate. |
| Preserve amounts through identity order | `funding_reindex_preserves_source_draw` and `funding_reindex_preserves_obligation_draw` | Restore each permutation and validate original source debits and obligations. |
| Preserve eligibility through identity order | `funding_reindex_preserves_assignment` | Every source/obligation permutation of a restricted three-by-three graph at every cursor. |
| Restore original matrix entries | `inverse_funding_reindex_restores_entries` | Exact entry restoration and custody-associated debit checks. |
| Preserve full-family holds and refunds | `funding_reindex_preserves_variable_branch_holds` and `funding_reindex_preserves_variable_branch_refunds` | Unequal branch sizes and source limits retain original custody amounts after row reversal. |
| Return the unused hold | `variable_branch_hold_equals_debit_and_refund` | Each source satisfies hold equals acquisition plus fee plus refund. |

The generated tests cover 512 small exact-contribution graphs and reversed source and obligation orders.
Full-width tests cover cohorts of 1, 2, 3, 64, 65, and 129 physical sources.
Every returned matrix must exclude the auxiliary row.
Budget-prefix tests require errors below the measured requirement and the identical verified result at the sufficient limit.

The complete matrix selector also has 512 generated cases with independent complete assignment oracles and reversed input orders.
Its tests cover the same physical cohort sizes and full-width amounts.
Parallel calls share immutable inputs but own separate budgets, residual matrices, and certificates.
The parallel-call regression checks result equality without a shared mutable allocation state.
It does not model concurrent vault settlement.

Identity-order tests cover all 36 source/obligation permutations of a three-by-three graph at all three cursor positions.
All permutations must return identical canonical keys, assignments, policy evidence, and next positions.
An additional 512 generated key sequences check sorting against standard byte order and preserve every input occurrence.
Negative controls reject custody aliases and inconsistent repeated obligation terms.
Identity tests also cover empty obligations, cohorts through 129 sources, full-width capacities, and every measured capture-budget prefix.

These proofs establish the mathematical reduction, prefix certificate soundness, and unique matrix selection.
They do not establish complete native array-operation correspondence, authenticated canonical serialization, or concurrent settlement publication.
Those obligations remain separate from this fixed-contribution selector.
