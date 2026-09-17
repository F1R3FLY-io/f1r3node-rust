# Bounded-row funding feasibility

## Purpose and scope

The funding optimizer needs more than upper capacity bounds.
It must also preserve contributions that an earlier optimization stage established.
A *box* specifies a lower and upper contribution bound for each physical source.
Equal bounds fix a contribution exactly.

This solver covers fixed integer obligations, independent source capacities, and a complete source-to-obligation eligibility relation in one monetary unit.
It does not authenticate inputs or resolve authority alternatives, conversions, persistent grants, or shared backing across branches.
Those constraints require their own complete reduction before this solver can apply.

The solver establishes feasibility, not lexicographic minimax optimality or economic tie selection.
Its traversal order must not select which wallet pays an economic residual.
The [allocation contract](lexicographic-minimax-funding.md) defines those objectives separately.
The [rank selector](fixed-flow-minimax.md) uses these bounds to preserve earlier constrained groups during subsequent threshold searches.

## Input and output contract

Let $`n`$ denote the number of sources and $`m`$ the number of obligations.
For source $`i`$, let $`l_i`$ and $`u_i`$ denote its lower and upper contribution bounds.
Let $`q_j`$ denote obligation $`j`$.
The Boolean relation $`E_{ij}`$ permits source $`i`$ to fund obligation $`j`$.
Let $`x_{ij}`$ denote that contribution and $`d_i`$ its row total.

```math
d_i = \sum_j x_{ij},\qquad
l_i \le d_i \le u_i,\qquad
\sum_i x_{ij}=q_j,\qquad
\neg E_{ij}\Longrightarrow x_{ij}=0.
```

[`FundingBoxProblem`](../../../../rholang/src/rust/interpreter/accounting/monetary_allocation/funding_box.rs) carries these arrays without copying or changing them.
Amounts use unsigned 64-bit integers.
The sum of obligations must fit that type.
The sums of source bounds need not fit that type.
The lower-cut checker uses checked unsigned 128-bit arithmetic for aggregate bounds.

`solve_box_funding_feasibility` has three economic results:

| Result | Evidence | Meaning |
|---|---|---|
| `Feasible` | Complete assignment and original source totals. | The assignment satisfies every bound, obligation, and eligibility constraint. |
| `UpperDeficit` | Selected obligations. | Their demand exceeds all neighboring upper capacity. |
| `LowerDeficit` | Selected sources. | Their required minimum exceeds all neighboring obligation value. |

Malformed dimensions, inverted bounds, arithmetic overflow, allocation failure, and exhausted host-work budgets are errors.
Errors do not become economic infeasibility or trigger a different allocation policy.
The caller retains unchanged inputs after every result or error.

## Algorithm

The solver first obtains a complete assignment with the existing upper-capacity feasibility solver.
If that search finds an upper deficit, the bounded problem is also infeasible.
Otherwise, the solver adjusts the assignment until each source meets its lower bound.

For an underfunded receiver, breadth-first search follows alternating edges:

1. Follow permitted edges from a source to obligations that it can fund.
2. Follow positive assigned edges from an obligation to its current funding sources.
3. Stop at a donor whose contribution exceeds its lower bound.

Intermediate sources can already be at their lower or upper bound.
A complete path changes only its receiver and donor totals.
Each intermediate source transfers one obligation's funding and receives an equal amount of another obligation's funding.

Let $`r`$ be the receiver, $`s`$ the donor, and $`P`$ the path's assigned edges that must decrease.
The transfer amount is:

```math
a=\min\left(l_r-d_r,\ d_s-l_s,\ \min_{(i,j)\in P}x_{ij}\right).
```

The solver transfers this amount at once.
It does not expand monetary quantities into individual tokens.
The path uses distinct vertices, and every decreasing edge has positive assigned value.

```text
validate dimensions, bounds, amount totals, and search limits
find an assignment that satisfies upper bounds and exact obligations
if no assignment exists: return its checked upper deficit
for each source with a contribution below its lower bound:
    while its contribution remains below that bound:
        search for an alternating path to a donor above its lower bound
        if no donor is reachable:
            check the reached-source lower deficit
            return that deficit
        calculate the path bottleneck and endpoint limits
        transfer that amount along the complete path
        independently check the assignment and endpoint changes
return the complete bounded assignment
```

All mutable search state is local to the call.
The function does not publish wallet balances, cursor updates, reservations, or settlement records.
Host-work accounting covers searches, path updates, auxiliary allocations, and independent result verification.

## Why a failed search proves infeasibility

Let $`S`$ be the reached sources after an unsuccessful search.
Let $`N(S)`$ contain every obligation that at least one source in $`S`$ can fund.
Search closure means that every positive contribution into $`N(S)`$ comes from $`S`$.
Eligibility prevents contributions from $`S`$ to obligations outside $`N(S)`$.
Thus:

```math
\sum_{i\in S}d_i=\sum_{j\in N(S)}q_j.
```

No reached source exceeds its lower bound, and the original receiver remains below that bound.
Consequently:

```math
\sum_{i\in S}l_i>\sum_{i\in S}d_i
=\sum_{j\in N(S)}q_j.
```

No alternative assignment can meet those lower bounds.
`check_lower_funding_cut` checks the inequality against original inputs before the solver returns the certificate.
It does not trust cached source totals or the search's parent records.

## Example: an intermediate source at capacity

Use three sources with upper bounds `[1,1,1]`, lower bounds `[1,1,0]`, and two obligations of value one.
Source A permits only the first obligation.
Source B permits both obligations.
Source C permits only the second obligation.

| Source | Initial assignment | Final assignment |
|---|---|---|
| A | Neither obligation. | First obligation. |
| B | First obligation. | Second obligation. |
| C | Second obligation. | Neither obligation. |

The transfer meets A's minimum and preserves B's exact contribution.
An algorithm that excludes B because B has no spare capacity would incorrectly reject this valid assignment.

## Proof and test correspondence

[`FundingBox.v`](../../../../formal/rocq/cost_accounted_rho/theories/FundingBox.v) defines bounded assignments, lower cuts, transfer paths, and aggregate lower deficits.
Its theorems establish these properties for arbitrary finite dimensions and natural-number amounts:

| Property | Formal result | Native check or regression |
|---|---|---|
| Lower-cut soundness | `lower_funding_cut_rejects_every_bounded_assignment` | Independent cut checker and exhaustive small-state oracle. |
| Closed-frontier deficit | `exhausted_lower_rebalance_has_deficit` | Original-input certificate check after unsuccessful search. |
| Exact obligations | `handoff_path_preserves_columns` | Full assignment check after every path. |
| Permitted edges only | `handoff_path_preserves_eligibility` | Full assignment check and per-cell transition assertions. |
| Endpoint accounting | `handoff_path_source_balance` | Receiver increases, donor decreases, and all other totals remain equal. |
| Capacity and lower progress | `lower_rebalance_preserves_assignment_and_progress` | Upper bounds hold, and established lower coverage never decreases. |
| Strict progress | `positive_lower_rebalance_strictly_progresses` | Aggregate lower deficit decreases by exactly the positive transfer amount. |

The native regression suite enumerates all 5,184 two-source, two-obligation boxes with bounds and demands from zero through two.
An independent unit-assignment oracle checks feasibility in those small domains.
Generated tests cover source and obligation permutations, arbitrary initial assignments, and every lower cut in each generated cohort.
Boundary cases include 129 sources, empty obligations, full-width amounts, and lower-bound sums above the native amount width.
Budget-prefix tests require errors at insufficient limits and the same economic result at the sufficient limit.

These proofs establish transfer invariants and certificate soundness.
They do not by themselves prove the native search loop's complete refinement, a strongly polynomial operation bound, or the outer minimax algorithm.
The wide-amount regression checks equal work counts for a scaled path fixture, not a universal complexity theorem.
