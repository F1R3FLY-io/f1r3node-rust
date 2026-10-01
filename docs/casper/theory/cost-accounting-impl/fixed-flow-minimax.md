# Fixed-flow funding policy

## Purpose and boundary

The rank selector finds a feasible contribution vector with the smallest descending lexicographic rank.
It checks global rank optimality independently before it returns that vector.
It does not select the economic tie between equally ranked vectors.
The separate cyclic selector chooses contribution ties within an independently certified optimal domain.
The composed policy selector connects classification, contribution selection, assignment selection, and the next cursor position.

The input domain contains fixed integer obligations, independent physical capacities, and a complete eligibility relation in one monetary unit.
Captured authority, resource compatibility, consent, custody aliases, and branch correlations must be resolved before this domain applies.
An incomplete projection of those constraints cannot justify a settlement.

[`solve_fixed_funding_minimax_rank`](../../../../rholang/src/rust/interpreter/accounting/monetary_allocation/funding_minimax.rs) returns either a checked optimum or a checked original-capacity deficit.
It retains the full assignment and original source positions.
The [allocation policy](authority-allocation-policy-ratification.md) still requires semantic domain classification, canonical contribution ties, assignment ties, and the correct cursor transition.
This rank helper does not publish wallet or cursor state.

## Composed policy selector

[`select_fixed_funding_policy`](../../../../rholang/src/rust/interpreter/accounting/monetary_allocation/funding_policy.rs) applies the approved policy to a complete fixed-flow problem.
The caller supplies capacities, obligations, eligibility, the captured cursor position, and work limits.
The caller cannot supply a fragment-selection flag.
The selector derives the fragment with `classify_fixed_funding_domain` before it selects contributions.

| Derived domain | Contribution selection | Positive-settlement next position |
|---|---|---|
| Infeasible | Return the checked original-domain deficit. | None. |
| Unrestricted contribution domain | Use the existing capped allocator and captured cursor. | Use its existing last-residual-recipient rule. |
| Restricted contribution domain | Minimize descending rank, then maximize contributions in cyclic priority order. | Advance one physical-source position with wraparound. |

The unrestricted domain contains every capacity-bounded contribution vector with the required total.
This is a property of feasible contributions, not a requirement that every matrix edge be present.
For example, capacities `[1,1]`, obligations `[1,1]`, and diagonal-only eligibility permit only contributions `[1,1]`.
The unrestricted capped domain has that same single vector.
The classifier therefore selects unrestricted behavior, including its unchanged cursor position when no residual exists.

By contrast, capacities `[2,10]`, demands `[2,1]`, and a first obligation restricted to the first wallet constrain the contribution domain.
The optimal contributions are `[2,1]`.
The restricted policy advances priority after positive settlement even though those optimal contributions are unique.
Neither classification nor cursor choice changes the permission to fund either obligation.

```text
validate the physical-source count and captured cursor
classify the complete fixed-flow contribution domain
if infeasible:
    return the checked original-domain deficit
if unrestricted:
    use the existing capped contribution allocator
    select the minimum matrix with those exact contributions
    independently certify its minimax rank
    retain the existing residual cursor position
if restricted:
    obtain a certified minimax rank
    derive the exact equally ranked assignment domain
    select its cyclic lexicographic maximum contribution vector
    select the minimum matrix with those exact contributions
    recheck original validity, optimal-domain membership, rank, and cyclic evidence
    advance the cursor by one physical-source position
if total new funding is zero:
    return no contribution-cursor position update
return the complete matrix, totals, evidence, fragment, and optional next position
```

Matrix selection uses the original eligibility relation and the selected exact contributions.
Afterward, the restricted path checks that the matrix still belongs to the derived optimal domain.
It also checks the rank and cyclic certificates against the final matrix.
These checks prevent a later stage from invalidating an earlier decision.
The [matrix certificate](canonical-funding-witness.md) independently establishes the last comparison tier.

`FundingPolicySelection` exposes immutable access to the complete assignment, totals, fragment evidence, rank certificate, matrix certificate, and optional next position.
Restricted results also retain a checked domain counterexample and their cyclic contribution certificate.
An internal-stage failure returns an error, not a different policy or an unverified assignment.
All stages share the caller's work budget.
Temporary search state remains local to the call.

The optional next position is a plan, not a published cursor transition.
Positive settlement still requires the existing scoped revision check and atomic balance/cursor publication.
Zero funding produces `None`, not an instruction to overwrite the current cursor.
The separate deployment-fee policy remains unchanged.

## Composition proof and scope

[`FundingPolicyComposition.v`](../../../../formal/rocq/cost_accounted_rho/theories/FundingPolicyComposition.v) composes the rank, cyclic-prefix, and matrix-prefix certificate theorems.
The result compares the final candidate with every feasible alternative in the original fixed-flow problem:

1. The candidate has no worse descending contribution rank.
2. Among equal ranks, the candidate has no smaller cyclic contribution vector.
3. Among identical contributions, the candidate has no larger row-major assignment matrix.

The cyclic conclusion requires an exact optimal-domain representation.
Its premise explicitly equates domain membership with original validity and equal contribution rank.
The [paired-cut construction](#exact-optimal-domain) and intersection proofs supply the mathematical representation contract.
The composition theorem does not assume that a partial candidate list represents that domain.
Separate cursor results prove zero-update behavior, unrestricted transition preservation, restricted one-position advancement, and bounded positions.

Native integration tests enumerate complete assignment sets for small graphs.
Their oracle sorts by descending rank, reversed cyclic priority, and matrix order in that sequence.
The oracle compares complete contribution sets against an all-true graph to determine the fragment independently.
Coverage includes all 1,296 two-source graphs with capacities and demands zero through two, every cursor, and 512 generated cases with reversed inputs.
Additional tests cover redundant missing edges, fixed restricted optima, zero demand, full-width amounts, work-budget exhaustion, and independent concurrent calls.

These results cover the fixed-flow policy composition, not arbitrary semantic funding domains.
Captured grant correlations, branch alternatives, typed resources, and acquisition equivalence must enter a complete domain before this selector applies.
The [identity-order adapter](canonical-funding-witness.md#identity-order-adapter) derives stable matrix positions from resolved source and obligation keys.
Authentication and complete typed-key construction remain necessary before a matrix becomes a canonical wire witness.
The composition proof does not yet establish complete native array correspondence or concurrent settlement publication.

## Objective

Let $`n`$ denote the number of physical sources and $`m`$ the number of obligations.
Let $`C_i`$ be source $`i`$'s capacity, and let $`q_j`$ be obligation $`j`$.
The Boolean relation $`E_{ij}`$ permits source $`i`$ to fund obligation $`j`$.
An assignment $`x_{ij}`$ has contribution $`d_i=\sum_jx_{ij}`$ and fixed total $`M=\sum_jq_j`$.

```math
0\le d_i\le C_i,\qquad
\sum_i x_{ij}=q_j,\qquad
\neg E_{ij}\Longrightarrow x_{ij}=0.
```

Sort the contributions in descending order.
Minimize the first position, then the second position without worsening the first, and continue through every position.
Minimizing only the maximum contribution is insufficient.

For example, `[5,3,2]` is better than `[5,4,1]`, although both maximum contributions equal five.
The source identities remain attached to the unsorted assignment.
Sorting defines rank, not a transfer of spending authority.

## Candidate search

The staged search adapts the canonical-block method in [Frank and Murota, Section 2.4](https://arxiv.org/html/2007.09618v2#S2.SS4).
Their method applies to integer base-polyhedron domains, also called M-convex sets.
It identifies one contribution level and its constrained block, then solves the remaining domain.
The native adaptation uses bounded-flow feasibility and binary threshold search.
The complete representation and partition correspondence remain separate proof obligations.

The native steps are:

1. Find a feasible assignment under the original capacities.
2. Find the smallest feasible maximum among the sources that are not yet fixed.
3. Reduce the number of contributions at that maximum through feasible one-unit exchanges.
4. Identify the block reached by those maximum sources' feasible exchanges.
5. Fix that block's contributions and repeat for the remaining sources.
6. Construct and independently check the global optimality certificate.

The [bounded-row solver](bounded-funding-feasibility.md) preserves fixed contributions during threshold searches.
An exchange query sets every source capacity to its proposed exact contribution.
Those proposed capacities sum to the fixed obligation total.
Any feasible assignment must therefore use each proposed capacity exactly.
`capacity_total_forces_every_source_exact` proves this fact.

```text
find an original-capacity feasible assignment
if none exists: return the checked demand deficit
if every eligibility edge is permitted:
    obtain the capped equal-sharing contribution rank
    find an assignment with exactly those contributions
else:
    while some sources remain unfixed:
        binary-search the least feasible maximum for unfixed sources
        reduce the number of sources at that maximum
        identify its near-uniform constrained block
        fix that block's exact contributions
check the final assignment against the original problem
construct cut evidence for every required positive contribution level
independently verify all cut evidence
return the assignment, totals, and certificate
```

The all-true path uses the existing capped allocator to obtain a rank witness.
It does not decide the semantic all-to-all classification or the settlement cursor.
Redundant missing edges can still produce an unrestricted contribution domain.
The separate semantic classifier must handle that distinction.

A threshold search requires at most 64 binary decisions for unsigned 64-bit amounts.
Each successful exchange removes one current maximum contribution without creating another.
Each completed stage fixes at least one source.
The algorithm does not balance the total charge through a per-token loop.
Its full native operation-bound proof must also cover flow searches, block identification, and certificate construction.

## Independent optimality evidence

For a nonnegative threshold $`t`$, define the total excess above that threshold:

```math
H_t(d)=\sum_i\max(d_i-t,0).
```

Let $`T`$ be a selected set of obligations.
Let $`N(T)`$ contain every source that can fund at least one obligation in $`T`$.
Clip each source capacity at the threshold and calculate this cut bound:

```math
D_t(T)=\max\left(0,\sum_{j\in T}q_j
                     -\sum_{i\in N(T)}\min(C_i,t)\right).
```

Every feasible assignment $`y`$ satisfies $`H_t(y)\ge D_t(T)`$.
The selected obligations cannot use more clipped capacity than their neighbors supply.
Any remaining contribution must appear in the excess above the threshold.

For each distinct positive contribution value $`v`$, the certificate includes cuts at both $`t=v-1`$ and $`t=v`$.
The checker requires an exact, untruncated equality at each threshold:

```math
H_t(d)+\sum_{i\in N(T_t)}\min(C_i,t)=\sum_{j\in T_t}q_j.
```

Thus, no feasible alternative has less excess at any required level.
Suppose an alternative had a smaller descending rank.
At the first different rank position, the candidate would have a larger positive value $`v`$.
At threshold $`v-1`$, that alternative would have strictly less excess.
This contradicts the checked cut bound.

[`FundingMinimaxCertificate.v`](../../../../formal/rocq/cost_accounted_rho/theories/FundingMinimaxCertificate.v) proves this argument for arbitrary finite dimensions and natural-number amounts.
The global theorem does not assume that the candidate search is correct or that its explored candidates cover all feasible assignments.
It compares the certified candidate with every assignment valid for the original fixed-flow problem.
The predecessor thresholds prove optimality.
The additional value thresholds permit an exact description of every equally ranked assignment.

## Certificate generation and verification

[`certify_funding_minimax`](../../../../rholang/src/rust/interpreter/accounting/monetary_allocation/funding_minimax_certificate.rs) runs the existing feasibility solver with capacities clipped at each required threshold.
Its deficit cut supplies the candidate bound.
If the clipped problem is feasible, generation uses an empty cut.
That cut is tight only when the candidate excess equals zero.
Generation returns no certificate if the cut bound does not equal the candidate excess.
The rank selector treats that result as an internal verification error.
It does not return an uncertified assignment or substitute another allocation policy.

`check_funding_minimax_certificate` first checks the complete assignment against original inputs.
It then derives every required threshold from the checked source totals.
Certificates must contain exactly those thresholds, in ascending order, with correctly sized obligation selections.
The checker calculates each excess and cut bound independently.
It does not trust search totals, clipped-capacity caches, or a claimed optimum value.
In particular, it rejects an oversupplied cut even when truncation would give a zero deficit.
Such a cut can prove a zero lower bound but cannot justify an optimal-domain restriction.

The total obligation must fit an unsigned 64-bit amount.
Aggregate cut arithmetic uses checked unsigned 128-bit integers because the sum of independent source capacities can exceed that amount width.
Threshold generation avoids underflow by excluding zero contributions before subtraction.
Zero total demand has no positive threshold certificates.

Every search, allocation, and verification uses the caller's host-work budget.
An insufficient budget is an error, not economic infeasibility.
All inputs are immutable, and all candidate search state belongs to the current call.

## Exact optimal domain

An optimal domain contains every original assignment with the certified minimum rank, and no other assignment.
This distinction matters because fixing one optimal contribution vector would discard valid economic ties.

Each tight cut becomes a bounded-flow restriction.
For a cut at threshold $`t`$, use the original neighbors $`N(T)`$ and original capacities $`C_i`$:

| Source relation | Lower contribution bound | Upper contribution bound | Permitted obligations |
|---|---|---|---|
| $`i\in N(T)`$ | $`\min(C_i,t)`$ | $`C_i`$ | Original eligible obligations inside $`T`$. |
| $`i\notin N(T)`$ | Zero. | $`\min(C_i,t)`$ | Original eligible obligations. |

Intersect the restrictions from all cuts.
Take the largest lower bound, the smallest upper bound, and the intersection of permitted edges.
Keep the original obligation amounts.
Do not derive later neighbors from previously restricted edges.

[`FundingOptimalDomain.v`](../../../../formal/rocq/cost_accounted_rho/theories/FundingOptimalDomain.v) proves each cut-to-box equivalence and the exact intersection rule.
It also proves that equal excess at both threshold families characterizes equal rank for assignments with the same total.
`paired_cut_boxes_capture_exact_equal_rank_domain` combines these results.
The proof covers arbitrary finite source and obligation counts.
It does not depend on local-exchange completeness or a particular candidate-search order.

For example, `[3,3]` and `[4,2]` both have excess two at threshold two.
Only `[3,3]` has excess zero at threshold three.
Predecessor thresholds alone can certify the reference optimum but cannot define its full equal-rank domain.

Edge restrictions also matter.
Suppose two sources can fund a seven-unit obligation, and a third source can fund only a separate two-unit obligation.
One of the first two sources can also fund that second obligation.
The optimal rank is `[4,3,2]` when capacities do not bind.
Source bounds alone can admit `[4,4,1]` through the crossing edge.
The tight-cut edge restriction excludes that assignment.

[`derive_funding_optimal_domain`](../../../../rholang/src/rust/interpreter/accounting/monetary_allocation/funding_optimal_domain.rs) independently checks the reference certificate before it constructs the restrictions.
It checks the reference assignment against the resulting domain before returning it.
The captured domain owns its bounds, obligations, and eligibility data.
Callers cannot replace those fields through its read-only interface.

## Cyclic contribution ties

The cyclic selector maximizes contributions lexicographically in the captured priority order, without changing the minimum descending rank.
For source count $`n`$ and cursor $`r`$, that order is:

```math
r,r+1,\ldots,n-1,0,1,\ldots,r-1,\qquad 0\le r<n.
```

The selector fixes one coordinate at a time inside the exact optimal domain.
Later coordinates remain free within their original domain bounds.
The search ceiling also cannot exceed the total obligation.
`funding_coordinate_is_bounded_by_total` proves this bound from conservation and nonnegative contributions.
This bound removes capacity-dependent binary searches when the obligation is zero.
It does not replace the final independent maximum certificate.

```text
find a feasible assignment in the certified optimal domain
for each source in cyclic priority order:
    binary-search its largest feasible contribution
    retain a feasible assignment for that contribution
    certify the maximum by its upper bound or a successor-deficit cut
    fix only this coordinate and the previously selected coordinates
independently check the final assignment and each prefix certificate
return the contribution vector, assignment, and certificate
```

A prefix certificate proves that no feasible allocation can improve the current coordinate while retaining earlier choices.
It either reaches the captured upper bound or supplies a checked lower-deficit cut for the next integer contribution.
The [bounded-row solver](bounded-funding-feasibility.md) provides that cut.
No assumption about binary-search correctness substitutes for the final independent checks.

[`FundingCyclicTie.v`](../../../../formal/rocq/cost_accounted_rho/theories/FundingCyclicTie.v) proves that prefix maxima produce the lexicographically greatest cyclic vector.
It proves that the cyclic order visits each source once and that the greatest vector is unique.
`funding_coordinate_exclusion_proves_maximum` derives each maximum from the captured upper bound or the bounded-flow deficit theorem.
`checked_box_prefix_is_lexicographically_greatest` composes those checks with exact prefix restrictions.
The resulting certificate theorem establishes the greatest vector across every assignment in the captured funding box.
Full native certificate-checker correspondence remains a separate obligation.

With total three and two sufficiently funded wallets, the two optimal vectors are `[2,1]` and `[1,2]`.
Cursor zero selects `[2,1]`, and cursor one selects `[1,2]`.
A verifier must not freeze the second wallet at two while checking the first wallet's claimed maximum of one.
That false restriction would fabricate an infeasible successor and certify the wrong tie.

[`select_funding_cyclic_tie`](../../../../rholang/src/rust/interpreter/accounting/monetary_allocation/funding_cyclic_tie.rs) implements this selection and verifies its certificate before return.
It does not update a cursor, publish wallet state, or choose between semantically different acquisitions.
It also does not canonically select the assignment matrix when several matrices produce the same contribution vector.
Canonical assignment witnesses and settlement publication require their own integration.
The [matrix selector](canonical-funding-witness.md) preserves selected contributions and certifies a unique row-major assignment for the supplied source and obligation order.
The identity-order adapter supplies deterministic positions, while authenticated typed-key construction and serialized witness integration remain separate.

## Proof and regression coverage

| Requirement | Formal result | Regression coverage |
|---|---|---|
| Rank ignores source permutation | `contribution_excess_permutation` and the existing rank permutation theorem. | Reversed source and obligation orders. |
| Full rank, not maximum only | `smaller_sorted_rank_has_excess_witness` | Forced maximum with a lower-position improvement. |
| Finite threshold coverage | `breakpoint_excess_bounds_prove_minimax_rank` | Missing, changed, reordered, and forged certificate levels. |
| Independent lower bound | `clipped_cut_bounds_every_assignment_excess` | Changed eligibility and false cut selections. |
| Exact exchange queries | `capacity_total_forces_every_source_exact` | Fixed earlier groups across later optimization stages. |
| Global returned-rank optimality | `checked_excess_cuts_prove_global_funding_minimax` | Independent exhaustive assignment oracle and generated restricted graphs. |
| Exact equal-rank domain | `paired_cut_boxes_capture_exact_equal_rank_domain` | Every feasible matrix in small graphs, with two reference witnesses where available. |
| Tight-cut intersection | `tight_cut_iff_box_restriction` and `funding_family_intersection_exact` | Paired thresholds, oversupply rejection, and necessary edge removal. |
| Greatest cyclic contribution vector | `prefix_maximal_funding_is_lexicographically_greatest` | Every cursor against the complete small-state oracle. |
| Complete priority order | `cyclic_funding_order_visits_each_source_once` | Cohorts of 1, 2, 3, 64, 65, and 129 sources. |
| Independent coordinate maximum | `successor_exclusion_proves_coordinate_maximum` | Future-coordinate freezing mutation and missing certificate entries. |
| Bounded-flow certificate composition | `checked_box_prefix_is_lexicographically_greatest` | Independent prefix checking against the complete small-state oracle. |
| Conservation bounds each search | `funding_coordinate_is_bounded_by_total` | Zero-total work remains equal for one-unit and maximum-width unused capacities. |

The tests include all 1,296 two-source and 15,552 three-source graphs with capacities and demands from zero through two.
Generated tests compare the returned rank with the complete small-state assignment oracle.
Separate generated tests use wide amounts and restricted graphs with a known feasible initial assignment.
Other cases cover three distinct constrained levels, 129 sources, full-width obligations, and empty demand.
Budget-prefix tests require an error below the measured requirement and the same certified result at the sufficient limit.
Optimal-domain and cyclic-tie tests also check all 1,296 two-source graphs and 512 generated cases with reversed source and obligation orders.
They compare complete assignment matrices, not only the selected candidate's feasibility.
Full-width tests exercise zero, one-unit, and maximum unsigned 64-bit totals across the listed cohort sizes.
Concurrent selectors share an immutable captured domain and a bounded host-work counter.
The concurrency regression requires the same certified results or budget errors, with no change to the captured funding state.
This test does not cover wallet publication or replace the settlement concurrency models.

The mathematical certificate theorem proves global optimality when its premises hold.
It does not establish complete native checker correspondence, candidate-search completeness, or settlement publication by itself.
Those requirements remain distinct from feasibility and rank certification.
The rank helper is not an end-to-end settlement implementation.
