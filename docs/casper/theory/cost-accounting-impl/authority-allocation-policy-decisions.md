# Remaining authority-allocation decisions

## Status and purpose

This proposal defines authority allocation policy.
The scheduler selected that task after the funding audit passed independent review.
The proposal separates confirmed decisions from policy details that still require approval and formal verification.
It does not change runtime behavior or ratify the recommendations below.

The [findings matrix](../multi-wallet-funding-findings-matrix.md) maps the implementation and verification obligations.
The [minimax contract](lexicographic-minimax-funding.md) defines the approved fairness objective.
The [rotating allocator](rotating-monetary-allocation.md) defines the existing unrestricted residual rule.
The [settlement review](funding-settlement-design-review.md) records prepaid backing, branch exposure, and remaining economic decisions.

## Confirmed decisions

| Subject | Confirmed rule |
| --- | --- |
| Valuation | Price actual authority-resource demand with representation-invariant backing. Do not infer price from physical cell count alone. |
| Contribution | Share newly required funding. Do not automatically reimburse historical sponsors. |
| Fairness | Minimize descending monetary contributions lexicographically over complete feasible assignments with a fixed cohort and total obligation. |
| Authority | Preserve every required logical occurrence. Merge physical custody aliases only for monetary capacity and contribution positions. |
| Eligibility | Authenticate funding sources, permitted obligations, and applicable consent. Balance sufficiency does not replace authorization. |
| Joint purses | Treat authorized joint and individual purses equally within the monetary cohort. A joint purse has no automatic priority. |
| Unrestricted residual | Retain the certified all-to-all allocator and its rotating residual rule, subject to the minimax refinement proof. |
| Price consent | Preserve `phloPrice` as the offered price. Enforce the chain minimum and every separately signed applicable owner ceiling. Ceilings are not weights. |
| Limits | Keep execution limits separate from persistent allowances and temporary reservation exposure. |
| Ownership | Support arbitrary finite ownership histories without duplicating rights, restoring consumed allowances, or redirecting captured refunds. |
| Architecture | Preserve native custody and atomic checkpoint settlement. Do not introduce persistent deployment escrow or a global funding lock. |

These decisions do not prove that the implementation satisfies the contract.

## Different cardinality limits

The signer limit and the physical-purse limit are different parameters.
For example, 64 signers and their distinct joint purse can require 65 physical-purse positions.
Explicitly presented eligible joint subsets can add more positions.
Custody aliases can reduce the physical count without reducing required logical multiplicity.

The [current cohort contract](rotating-monetary-allocation.md#authorized-physical-cohort) already records this distinction.
It does not enumerate every possible signer subset.
Host-work limits bound discovery of actually presented custody.

The final configuration must name signer, physical-purse, obligation, eligible-edge, and search-work limits separately.
No implementation may use the configured signer limit as an accidental two-purse or 64-purse restriction.
Configuration values and activation remain decisions for the existing activation task.

## Decision A: Restricted residual ties

The minimax objective can leave several labeled contribution vectors with the same descending rank.
The tie rule must select a feasible vector without permanent lexical priority.
It must retain the current all-to-all result and cursor transition as a special case.

The recommendation uses cyclic lexicographic maximization among equally ranked optimal contribution vectors.
It gives the first cyclic position priority for an additional contribution, then considers later positions.
This priority never overrides feasibility or the already optimal descending rank.
The complete source-to-obligation assignment remains the settlement instruction.

The cursor transition depends on a mandatory semantic distinction:

| Captured contribution domain | Successful positive-settlement transition |
| --- | --- |
| Exactly the capped all-to-all domain | Retain the existing last-residual-recipient rule, including unchanged position when no residual exists. |
| Genuinely restricted domain | Advance the captured position by one modulo the physical cohort size. |

For a fixed cohort, consecutive restricted positive settlements give every position first priority within one cohort-length cycle.
This guarantee excludes intervening cursor updates from another fragment.
It does not guarantee equal debits under arbitrary restrictions or resistance to authorized cohort changes.
Even a uniquely feasible restricted vector advances priority after positive settlement.
The transition does not change that unique contribution vector.

For unrestricted capacities `[9,9,9]`, obligation eight, and cursor zero, the result remains `[3,3,2]` with cursor two.
For a uniquely feasible restricted vector `[2,1]` at cursor zero, the result remains `[2,1]`, but the next position is one.
These examples illustrate the proposal and are not native verification results.

### Mandatory domain classification

Let $`\mathcal{F}_M`$ be the complete feasible contribution domain for the captured problem and fixed positive obligation $`M`$.
Let $`C_i`$ be each physical purse's authenticated applicable capacity, after alias resolution and existing exposure constraints.
Let $`n`$ be the positive physical cohort size.
The capped simplex is:

```math
\mathcal{S}(C,M)=\left\{d\in\mathbb{N}^{n}:\sum_i d_i=M\;\land\;\forall i,\ d_i\le C_i\right\}.
```

Use the all-to-all transition exactly when $`\mathcal{F}_M=\mathcal{S}(C,M)`$.
Otherwise, use the restricted transition after proving feasibility and the required optimum.
Every projected contribution vector must retain a valid complete assignment witness.

An all-true visible edge matrix does not prove this equality when shared grants, branch correlations, or typed-resource restrictions remain.
The proposer cannot select the fragment with an optional certificate or optimization flag.
Equivalent representations of the same captured contribution domain must select the same fragment.
Canonical capacity extraction forms part of that representation contract.

The classifier needs soundness and completeness for the supported constraint domain.
Ordinary independent-capacity all-to-all flow can use a proved structural certificate.
General discrete alternatives need their applicable exact classification method and bounded-work proof.
An indeterminate classification must reject explicitly rather than select a different economic rule.
This specification does not claim that the existing feasibility helper implements that classifier.

### Rejected generalization

The original proposal advanced after the last purse above its minimum contribution across equally ranked optima.
A fixed-set counterexample demonstrates the failure.
The optimum set contains `[2,0,1]`, `[0,1,2]`, and `[1,2,0]`.
At cursor zero, the original rule selects `[2,0,1]` and returns to cursor zero forever.
The cohort and eligibility remain unchanged.

The revised restricted transition cycles priority through positions zero, one, and two in this example.
It selects `[2,0,1]`, `[1,2,0]`, and `[0,1,2]` respectively.
The example is an abstract discrete domain, not a claim that ordinary integer flow realizes exactly that set.

A permanent lexical tie conflicts with the selected rotating policy.
A universal one-position transition would change the approved unrestricted residual rule.
Neither is the recommendation.

Required proof obligations include:

- Existence and uniqueness of the selected labeled vector for a nonempty finite optimum set.
- Preservation of minimax rank, eligibility, and physical capacity.
- Exact agreement with unrestricted contributions and cursor transitions under all-to-all hypotheses.
- Sound and complete fragment classification with representation-independent dispatch.
- Invariance under input permutation after canonical identity resolution.
- The restricted fixed-cohort priority cycle and its explicit limits under mixed fragments or changing eligibility.
- Checked and bounded native computation, including the tie witness.

Rotation does not establish unrestricted resistance to manipulation of newly authorized cohort identities.
Any stronger claim needs a separate threat model and proof.

## Decision B: Zero funding and witness ties

For zero newly required funding, the recommendation is no contribution and no contribution-cursor update.
There is no contribution decision to rotate.
Separate deployment fees retain their own cursor and policy.
Other valid state changes still use the existing replay and checkpoint rules.

This is a proposed zero-obligation integration rule, not the current behavior of every helper.
`allocate_capped_max_min` already preserves zero debits and cursor position at zero obligation.
However, `MonetaryCohort::plan` currently creates a revision transition even at zero.
The proposed contribution path must avoid that unnecessary transition rather than claim unchanged helper behavior.
Exact positive-obligation all-to-all refinement includes both the position and revision.
Zero-obligation refinement preserves kernel debits and position but deliberately omits the current helper's revision increment.
The fixed positive deployment-fee path remains unchanged.

Zero new funding does not bypass authority, prepaid-backing, consent, replay, or application-state validation.

For positive funding, a successful settlement still needs a checked revision transition even when the cursor position is unchanged.
Abort or duplicate delivery must not publish another revision or charge.

Different source-to-obligation assignments can have identical purse contributions.
The recommendation is canonical ordering of complete, semantically equivalent assignment witnesses after the contribution decision.
Canonical order must bind obligation identity, source identity, resource compatibility, and captured provenance.
It must not depend on hash-map iteration or search discovery order.

Different ownership effects, retained rights, conversion terms, or execution outcomes are not mere witness ties.
They require their own authorized semantic choice.
The exact witness encoding belongs to the existing wire and solver contract tasks.

## Decision C: Acquisition alternatives with different totals

The approved minimax objective compares equal total obligations.
It cannot decide between a cheaper unbalanced acquisition and a more expensive balanced acquisition.

For genuinely interchangeable acquisitions, the recommendation is to minimize new monetary cost first.
Apply the approved minimax rule among the feasible plans with that minimum total.
This prevents a fairness optimization from increasing the bill merely to spread contributions.

Interchangeable acquisitions must satisfy the same required outcome, authority, resource rights, and applicable consent.
They must use an approved common valuation domain before their monetary totals can be compared.
Do not compare raw amounts from different assets or choose the program's execution branch through this objective.
Apply compatible prepaid discharge without creating automatic sponsor reimbursement.

An alternative requires the signed intent to select an acquisition route before allocation.
That approach gives the caller more explicit control but does not automatically select the cheapest authorized equivalent route.
The complete funding model can support explicit route constraints without treating them as permission to bypass authority or cost limits.

The user must authorize the equivalence relation used to group acquisition alternatives.
Its witness must preserve the required outcome, retained rights, refund provenance, and authorized asset effects.
Wire identity must bind the selected alternative and its evidence.
Failure to prove equivalence must not silently group different acquisitions into one optimization problem.

Required evidence must distinguish:

- Optimization of an authorized acquisition from selection of a different program outcome.
- Minimum total monetary cost from minimum largest payer contribution.
- Complete feasible alternatives from a partial search frontier.
- Search-budget exhaustion from proven infeasibility or proven optimality.

No budget-limited search may silently return a merely feasible plan as the required optimum.

## Review and implementation boundary

The recommendations require independent plan review before they become implementation requirements.
The review must examine the restricted tie construction, unrestricted refinement, complete feasible-set representation, and computation bounds.
The user must approve the economic decisions before production integration.

Prepaid price compatibility, failure charges, branch exposure, conversion ordering, and activation remain in their existing decision tasks.
This proposal does not implicitly select those policies.
All approved decisions must enter the formal contract before the corresponding runtime change.
Each theorem and counterexample must map to native property, concurrency, or integration evidence where applicable.

| Required regression family | Independent oracle or negative control |
| --- | --- |
| Restricted priority | Preserve the three-vector fixed-set counterexample. Refute the old stalled transition and check a complete fixed-cohort priority cycle. |
| Unrestricted refinement | Enumerate small complete capacity domains and all cursors. Compare contributions, positions, and positive-obligation revisions with the existing contract. |
| Domain classification | Reject optional-dispatch flags, hidden shared constraints, incomplete candidate sets, and representation-dependent classification. |
| Zero funding | Check unchanged contribution position and revision without bypassing other validation. Keep positive deployment-fee revision tests separate. |
| Witness ties | Permute source, obligation, and search order. Require the same canonical complete witness for equivalent inputs. |
| Acquisition ordering | Compare complete authorized alternatives with an independent minimum-cost-then-minimax oracle. Reject inequivalent effects and incomparable asset totals. |
| Concurrent publication | Interleave independent scopes and contending custody. Check abort, duplicate delivery, stale cursor, and atomic balance/cursor publication. |

These are formal and native acceptance obligations, not completed verification results.
