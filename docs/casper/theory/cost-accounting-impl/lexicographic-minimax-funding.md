# Lexicographic minimax funding

## Decision and status

On September 10, 2026, the user approved lexicographic minimax for restricted funding.
This rule minimizes the largest wallet contribution, then the next largest, over feasible allocations with the same total obligation.
It extends equal sharing to obligations with different permitted funding sources.

This document records the approved objective and its implementation requirements.
It does not report completed solver integration or completed formal verification of this objective.
The decision does not approve a new Casper protocol, a second ledger, or persistent deployment escrow.

The [September 11 ratification](authority-allocation-policy-ratification.md) resolves the later restricted tie, zero-funding cursor, and equivalent-acquisition ordering decisions.
The earlier unresolved-choice descriptions below record the September 10 boundary.
The ratified policy now controls those choices, while their implementation and verification remain incomplete.

Related contracts are the [signed phlo proposal](signed-phlo-contract-proposal.md), [settlement review](funding-settlement-design-review.md), and [existing rotating allocator](rotating-monetary-allocation.md).

The approved [resource and fee composition](#approved-resource-and-fee-composition) uses joint feasibility with resource fairness first.
Explicit fee sponsorship is optional and requires signed permission.
The selected multi-outcome policy is [canonical outcome priority](#selected-canonical-outcome-priority).
Family-wide worst-case fairness remains a documented alternative, not the active policy.

## Purpose and scope

An obligation specifies a resource requirement and its permitted funding sources.
A contribution is the amount of new monetary funding assigned to a physical purse.
A feasible allocation satisfies every obligation without exceeding any applicable funding permission or limit.

Use compatible prepaid resources before calculating newly required funding.
Do not reimburse previous sponsors automatically or equalize their historical spending.
Equal monetary value does not make different authorities, locations, or resource classes interchangeable.
See the [typed prepaid-discharge proof](funding-settlement-design-review.md#typed-prepaid-discharge-proof) for that boundary.

Apply fairness within one authenticated funding scope, not across unrelated deployments, validators, or shards.
Keep the cohort fixed while comparing allocations.
Each eligible physical purse has one position, even when several logical authority lanes refer to that purse.
Preserve the separate authority occurrences and restrictions associated with those lanes.

Zero contribution does not remove required authority or consent.
Do not insert unauthorized wallets, remove cohort members, or split custody aliases to manipulate the comparison.
Authorized cohort changes require the applicable funding and ownership rules.

## Exact ordering

Let $`n`$ be the number of physical purses in the fixed cohort.
Let $`M`$ be the fixed newly required monetary obligation in one settlement asset's smallest units.
Let $`d=(d_1,\ldots,d_n)`$ be the vector of nonnegative integer contributions.
Let $`\mathcal{F}_M`$ contain the feasible vectors for this scope, captured state, and obligation.
Every vector in this set must satisfy:

```math
\sum_{i=1}^{n} d_i=M.
```

Define $`d^\downarrow`$ as the contributions sorted from largest to smallest, including zero contributions.
For two sorted vectors, lexicographic order compares their first unequal entries.
The vector with the smaller entry at that position is better.
The approved objective is:

```math
d^*\in\mathcal{F}_M,
\qquad
\forall e\in\mathcal{F}_M,
\quad (d^*)^\downarrow\leq_{\mathrm{lex}}e^\downarrow.
```

First minimize the largest contribution.
Among the remaining candidates, minimize the second-largest contribution.
Continue through all cohort positions.
Do not stop after minimizing only the maximum, because later positions can distinguish fairer allocations.

Sorting determines the fairness rank only.
Settlement retains the original purse identities and their complete source-to-obligation assignments.
The sorted vector cannot serve as a settlement instruction.

This objective compares allocations of the same obligation.
It does not authorize increasing the bill, choosing an execution branch, or replacing the approved resource-valuation policy.
If acquisition alternatives have different total costs, their selection needs a separate contract before applying this fixed-total comparison.

## Feasibility takes priority

A smaller largest contribution is invalid if the allocation cannot fund the required obligations.
The solver must preserve resource compatibility, eligible funding edges, physical custody, consent, and captured limits.
Scalar totals alone cannot prove feasibility.

For a fixed integer-flow fragment, let $`a_{ij}`$ be the amount purse $`i`$ contributes to obligation $`j`$.
Let $`q_j`$ be that obligation's monetary requirement, and let $`C_i`$ be the purse's applicable capacity.
Let $`E`$ be the authenticated set of permitted source-to-obligation edges.
This fragment requires:

```math
a_{ij}\in\mathbb{N},
\qquad (i,j)\notin E\Rightarrow a_{ij}=0,
\qquad \sum_i a_{ij}=q_j,
\qquad d_i=\sum_j a_{ij}\leq C_i.
```

These equations describe fixed obligations in one integer unit domain.
They do not establish that every compound resource transformation or conversion reduces to ordinary flow.
The production solver must prove that reduction where it uses a flow algorithm.
Otherwise, it must preserve the applicable discrete alternatives and their complete feasibility witnesses.

## Examples and alternatives

All amounts below are illustrative integer settlement units, not a production tariff.

| Case | Feasible allocation or comparison | Required result |
| --- | --- | --- |
| Three unrestricted purses, obligation eight, sufficient capacity | Permutations of `[3,3,2]` | Keep equal sharing and assign the residual through the existing rotating rule. |
| Two units require A, one unit permits A or B | `[2,1]` is feasible, but `[1,2]` is not | Select an allocation that preserves A's restricted obligation. |
| Three purses, with only `[0,5,5]` and `[1,1,8]` feasible | Sorted ranks are `[5,5,0]` and `[8,1,1]` | Select `[0,5,5]` because five is less than eight. |
| Two feasible vectors have the same maximum | Sorted ranks `[5,4,1]` and `[5,3,2]` | Prefer `[5,3,2]` at the second position. |
| No newly required funding | All contributions are zero | Do not create a new acquisition charge. Separate fees retain their own policy. |

The two restricted-vector comparisons are mathematical examples, not observed node executions or proven native feasible sets.

Leximin contribution is a different objective.
It sorts contributions from smallest to largest and maximizes the first unequal entry.
That rule prefers `[1,1,8]` over `[0,5,5]`, increasing participation at the expense of the largest payer.
The user selected lexicographic minimax instead.

Minimizing a weighted sum or a sum of squares is not a substitute for the approved ordering without an equivalence proof.
Approximate floating-point comparisons must not determine consensus-visible allocations.
An implementation must use exact integer comparisons and checked arithmetic.

## Unrestricted allocation and ties

The existing capped max-min allocator remains the implementation for certified all-to-all scopes.
Every contribution vector in the capped simplex must have a permitted assignment in such a scope.
Complete edge eligibility is sufficient, but redundant missing edges do not make a domain restricted.
The refinement must prove that its sorted contribution vector attains the approved minimax objective for that fragment.
Its existing name does not authorize using leximin contribution for general restricted alternatives.

Distinct labeled allocations can have the same sorted vector.
For example, `[3,3,2]` and `[2,3,3]` have equal fairness rank.
The objective alone does not select which purse pays the residual.

Retain the existing canonical cohort and rotating residual rule for unrestricted funding.
Restricted ties need a precise deterministic extension that respects eligibility and captured cursor state.
This decision does not silently replace that extension with permanent lexical preference or iteration order.
The implementation contract must also specify zero-new-funding cursor behavior and ties between assignments with identical purse totals.
The separate fixed-fee cursor and policy remain unchanged.

## Separate fee feasibility

Resource allocation and fee allocation have separate objectives and cursors, but they consume shared physical backing.
The complete feasible domain must permit both charges without exceeding any source's capacity or signed limits.
Resource fairness must not count the separate fee as a new resource contribution.
The fee's existence must nevertheless constrain which resource plans can complete.

For example, A and B each have ten units and can pay two resource units plus the separate one-unit fee.
Resource fairness selects one resource unit from each wallet.
If a single optimizer instead ranks all three units together, a canonical witness can assign two resource units to A and the fee to B.
The combined contributions look balanced, but the resource contributions violate their separate objective.

A second example gives A and B one unit each and requires one resource unit plus the fee.
Both wallets permit the resource charge, but only A permits the fee.
A resource-only tie that selects A leaves no permitted fee payer.
The complete plan instead charges the resource to B and the fee to A.
Failure to complete the first resource plan does not establish that the operation is unfundable.

For a fixed resource plan, let $`C_i`$ be source $`i`$'s effective authorized capacity and $`d_i`$ its resource draw.
Require $`d_i\le C_i`$ for every source.
Let $`F`$ be the sources authorized to pay the fee.
The current one-unit fee can complete exactly when:

```math
\sum_{i\in F}(C_i-d_i)>0.
```

Because amounts are integers, this condition is equivalent to one permitted source retaining at least one unit.
Effective capacity must already include applicable balance, custody-role, asset, debit, and exposure constraints.
Physical aliases must share one source entry.
An increased wallet balance does not override a lower signed limit.

[`can_complete_unit_fee`](../../../../rholang/src/rust/interpreter/accounting/monetary_allocation/funding_fee_completion.rs) checks this condition for a supplied resource plan.
It validates every source before returning success, including sources that cannot pay the fee.
It rejects dimension mismatches, source-limit violations, resource overdraw, and exhausted verification budgets.
The check uses linear time and constant additional space without summing full-width balances.
It does not select a different resource plan or update either cursor.

`one_unit_fee_completion_exact` in `FundingPolicyComposition.v` proves the condition for arbitrary finite lists.
The native property test compares the result with an independent 128-bit remaining-capacity sum across up to 129 sources.
Counterexample tests exercise both incorrect compositions described above with the real fixed-flow selector.
Additional tests cover full-width balances, late source overdraw, and every verification-budget prefix in the specified cohort fixtures.

This predicate is a necessary integration check, not a complete joint optimizer.
The following approved composition controls its integration.
Calling the ordinary fixed-flow verifier on a combined fee/resource matrix does not implement that composition.

### Approved resource and fee composition

Option 3, joint feasibility with resource fairness first, is the approved default.
Resource selection considers only plans that leave the separate fee payable.
Within that domain, the existing acquisition-cost, resource-fairness, and resource tie-break rules retain their precedence.
The fee allocator then applies its separate rule and cursor to the remaining authorized backing.
Fee-payer preference does not override the selected resource contribution vector.

The fee remains one total monetary unit, not one unit per signer.
The resource contribution vector excludes that fee.
The fixed physical cohort includes zero-contribution members and counts each physical purse once.
Signed permissions, price ceilings, source limits, resource compatibility, and custody restrictions remain mandatory.
This composition does not change Casper voting, finality, or fork choice.

For the fixed integer-flow fragment, let $`\mathcal{A}_M`$ contain all valid resource assignments with new monetary cost $`M`$.
Let $`d_i(a)`$ be source $`i`$'s resource draw in assignment $`a`$.
Let $`f_i`$ be its fee draw, and let $`C_i`$ be its effective authorized capacity.
Let $`F`$ contain the fee-eligible sources.
Define the complete-plan domain:

```math
\mathcal{J}_M = \left\{(a,f)\;\middle|\;
a\in\mathcal{A}_M,\quad
f_i\in\mathbb{N},\quad
\sum_i f_i=1,\quad
i\notin F\Rightarrow f_i=0,\quad
d_i(a)+f_i\le C_i\ \text{for every }i
\right\}.
```

The corresponding resource contribution domain is:

```math
\mathcal{D}_M=\{d(a)\mid \exists f,\ (a,f)\in\mathcal{J}_M\}.
```

First minimize new resource cost among authorized equivalent acquisitions with a complete feasible plan.
Then minimize the descending resource contribution vector over $`\mathcal{D}_M`$ and apply the approved resource ties.
Finally select the fee assignment with the separate fee policy on residual backing.
Canonical assignment selection must preserve all these priorities.

These equations assume that effective capacities and eligible edges already capture the applicable constraints of this fixed fragment.
They do not replace typed resource proofs, conversion constraints, or source-specific reservation checks for alternative execution outcomes.
The full planner must retain those constraints when it constructs the complete-plan domain.

### Alternatives considered

The alternatives differ in economic priority, not only in implementation order.
The comparison assumes that both charges require explicit funding authority.

| Option | Rule | Advantages | Disadvantages and disposition |
| --- | --- | --- | --- |
| 1. Resource first, then fee | Select resources without fee feasibility, then attempt the fee. | Preserves the existing sequence and has simple composition. | Can reject a jointly fundable operation. Rejected as the default. |
| 2. Fee first, then resources | Fix the fee payer before resource selection. | Secures fee backing first and has simple composition. | Can exhaust the sole resource-eligible source despite another feasible fee payer. Rejected as the default. |
| 3. Joint feasibility, resource fairness first | Optimize resource contributions among complete plans, then allocate the fee separately. | Preserves resource fairness and separate cursors. Avoids rejection caused solely by an incompatible initial resource choice. | Requires complete joint feasibility and careful classifier integration. Selected as the default. |
| 4. Joint feasibility, fee priority first | Prefer a fee assignment that permits completion, then optimize resources. | Preserves fee preference without a premature fee choice that prevents completion. | Gives fee preference precedence over resource fairness. Rejected because that priority differs from the approved objective. |
| 5. Combined total-payment fairness | Optimize resource and fee debits as one contribution vector. | Directly balances each wallet's total bill. | Can violate resource-only fairness. Requires a different objective and cursor policy. Rejected for this design. |
| 6. Explicit separate fee funding | Require a designated payer or independently funded fee source. | Makes sponsorship and consent explicit. Independent backing removes competition with resource funding. | Adds a funding requirement. Designation alone does not separate shared backing. Supported as an optional arrangement, not the default. |

Options 3 and 4 avoid allocation-order rejection only when the planner represents the complete feasible domain.
Search exhaustion is not evidence of insufficient funds.
Independent allocation followed by unspecified conflict repair is not an additional policy.
Such an implementation must preserve the selected domain, objective, and tie rules.

The earlier examples demonstrate the failures of options 1 and 5.
For option 2, A and B each have one unit.
Only A permits resource funding, but either wallet permits the fee.
A fee-first choice of A prevents resource funding.
The complete plan instead draws the resource unit from A and the fee from B.

Explicit sponsorship restricts fee eligibility through authenticated permission.
A designated payer that also supplies resources remains subject to joint feasibility.
An administrator never becomes the default sponsor merely because the administrator created the process.
No sponsorship arrangement may create an implicit debit, a persistent deployment escrow ledger, or a global funding lock.

### Integration and verification requirements

The approved policy is not a claim of completed production integration.
The existing fee predicate checks one supplied resource plan.
It does not search alternative resource plans, prove their optimum, or publish either cursor.

1. Construct the complete domain from authenticated state, resource evidence, permissions, limits, and applicable reservations.
2. Include fee feasibility before selecting acquisitions or ranking resource contributions.
3. Derive unrestricted-versus-restricted dispatch from the resulting contribution domain, not from a proposer-selected classification.
4. Preserve the separate resource and fee objectives, ties, and cursor transitions.
5. Retain a complete witness for resource charges, the fee, and every covered execution outcome.
6. Publish compatible debit, reservation, refund, and cursor effects atomically through the existing native checkpoint.
7. Distinguish proved infeasibility from exhausted solver resources or invalid evidence.

The classifier needs a proved relationship between captured capacities and the projected resource domain.
Do not reserve the fee from an arbitrary wallet before constructing that domain.
A sole permitted fee payer needs one unit of residual capacity.
Multiple eligible payers need not have individually reserved fee units.
The unrestricted allocator applies only when its stated domain and cursor refinement conditions hold.
Otherwise, the restricted selector must preserve the complete projected domain.

Zero new resource funding leaves the resource cursor unchanged, even when the separate fee applies.
A top-up cannot expand signed spending permission.
Concurrent plans cannot reuse the same backing without the existing state-conflict checks.
Disjoint funding scopes do not require a new global serialization point.

| Verification target | Required evidence |
| --- | --- |
| Domain projection | Prove that a resource vector is admissible exactly when a complete authorized resource-and-fee witness exists. |
| Economic ordering | Prove minimum acquisition cost, resource minimax rank, deterministic resource ties, and subsequent fee-policy selection. |
| Classification | Prove unrestricted refinement where applicable. Preserve restricted dispatch when fee constraints change the contribution domain. |
| Counterexamples | Retain the resource-first and combined-fairness regressions. Add the symmetric fee-first regression. |
| Property tests | Compare complete small domains with an independent oracle. Cover multiple fee payers, a sole payer, aliases, permutations, limits, and zero funding. |
| Concurrent publication | Model overlapping backing, disjoint scopes, top-ups, transfers, aborts, duplicate delivery, and both cursor effects. |
| Native correspondence | Check exact settlement, reservation conservation, atomic failure, and replay equality against the formal selection contract. |

The existing fee-completion theorem proves a prerequisite, not every requirement in this table.
The papers' resource semantics constrain this policy, but do not establish that this monetary priority is the only possible design.

### Native fixed-fragment selection

[`select_funding_with_unit_fee`](../../../../rholang/src/rust/interpreter/accounting/monetary_allocation/funding_fee_policy.rs) implements joint selection for fixed resource obligations and a separate one-unit fee.
Inputs supply effective source capacities, resource eligibility, fee eligibility, and separate resource and fee cursors.
The caller must authenticate these inputs and order them by canonical physical identity.
The selector returns the resource assignment, fee debits, fragment classification, and independent cursor results without modifying funding state.

Each complete plan has at least one permitted fee payer with one unit left after resource funding.
For each permitted payer with positive capacity, reduce only that payer's resource capacity by one in a private candidate problem.
The union of these candidate domains is exactly the joint-feasible resource domain.
This reduction explores payer alternatives but does not commit a fee payment or create a reservation.
After selecting resource contributions, the separate fee allocator can choose a different permitted payer from the remaining backing.

Let $`C_F=\sum_{i\in F}C_i`$ be the total effective capacity of fee-eligible sources.
The selector uses original captured capacities as the reference for unrestricted-domain classification.
It does not classify each temporary payer alternative as an independent economic scope.

- If $`M<C_F`$, every resource assignment leaves enough permitted backing for the fee. The existing resource policy remains applicable without modification.
- If $`M\ge C_F`$, the unrestricted capped domain contains a vector that exhausts every fee-eligible source, provided the resource total fits aggregate capacity.
- That vector cannot complete the fee. A feasible joint domain is therefore restricted relative to the original capacity domain.
- An already restricted resource domain cannot become unrestricted through the removal of additional vectors.

The implementation follows this selection procedure:

```text
Validate dimensions, limits, cursors, and resource feasibility.
Calculate total fee-eligible capacity with checked wide arithmetic.
If no permitted source has positive capacity, return infeasible.
If resource demand is below fee-eligible capacity:
    Select resources with the existing resource policy.
Otherwise:
    For each positive-capacity permitted fee payer:
        Reduce that source's candidate resource capacity by one.
        Solve resource minimax and the restricted cyclic tie in that candidate domain.
        Compare each feasible winner by resource rank, then cyclic contribution order.
    If no candidate domain is feasible, return infeasible.
    Select the canonical resource assignment for the winning contributions.
Allocate the fee from authorized residual backing with its own cursor.
Return both allocations and both cursor results without publishing effects.
```

The selector retains at most one current winner and one candidate, rather than every payer's assignment.
The restricted path uses at most one candidate optimization per positive-capacity permitted fee payer.
Configured source and obligation caps bound dimensions.
The shared host-work budget bounds search, verification, and temporary allocation work across all candidate optimizations.
Exhaustion returns an error, not insolvency or a partial optimum.

The existing fee cursor follows the capped allocator exactly.
If a sole payer covers the fee without a residual, the allocator preserves the fee cursor.
The resource cursor follows its own unrestricted or restricted rule and remains unchanged for zero resource demand.

[`CanonicalFundingProblem::select_with_unit_fee`](../../../../rholang/src/rust/interpreter/accounting/monetary_allocation/funding_identity_fee.rs) maps fee permissions from original source positions into canonical positions.
Its result uses canonical positions, and both input cursors refer to the canonical cohort.
`verify_with_unit_fee` recomputes the policy and compares original-position resource assignments, fee debits, and both resulting cursors.
Source and obligation permutations cannot change the named result.
Callers must derive fee eligibility from authenticated consent, not trust a proposed eligibility vector.

`FundingPolicyComposition.v` proves the exact fee-payer domain decomposition over arbitrary finite source counts.
Its minimum-over-branches theorem proves global selection from sound, complete branch winners under a transitive ordering.
The theorem explicitly requires those branch-selection premises.
Separate lemmas prove the fee-capacity shortcut and construct a capped-domain counterexample when fee feasibility restricts that domain.

Native tests compare the selector with independently enumerated complete assignment domains.
Coverage includes both sequential failures, combined-fairness rejection, multiple obligations, restricted permissions, full-width balances, arbitrary bounded cohorts, source permutations, and independent cursors.
Additional tests reject altered proposals and incomplete host-work budgets.
Parallel selector tests check independent assignments and cursor scopes, not atomic ledger publication.

This implementation does not authenticate vault balances, select across execution-outcome families, publish reservations, or activate node admission.
Those integration steps must preserve the full source-specific exposure, settlement, and replay contracts.
The fixed-fragment proofs and tests do not establish end-to-end completion of those steps.

### Checked phlo obligation adapter

[`CheckedPhloObligations::select_funding`](../../../../rholang/src/rust/interpreter/accounting/phlo_execution/obligations/policy.rs) connects checked resource charges to the joint selector.
Its input contains physical source identities, effective capacities, typed-obligation eligibility, and both canonical cursors.
The caller must derive effective capacities from authenticated balances and applicable debit and exposure limits.
The adapter does not expand funding permission or perform signature verification.

The adapter preserves the existing fee-first column layout in the returned assignment.
It removes that column only while it constructs the resource-fairness problem.
Resource keys retain their authority, location, class, and acquisition terms during canonical ordering.
The adapter restores original source and obligation positions before checking the complete assignment against the checked charges.
That result can supply a `PhloFundingCase` for the existing family reservation and native settlement checks.

An uncharged outcome selects zero debits and updates neither cursor.
A successful fully prepaid execution can still owe the separate fee without updating the resource cursor.
`verify_funding` recomputes the selection and rejects altered assignments or either incorrect cursor update.
An infeasible input returns no selection, while malformed inputs and host-work exhaustion remain errors.

Regression tests cover source permutations, restricted fee sponsorship, incorrect cursor proposals, and conversion into native acquisition, fee, and refund amounts.
Generated tests check exact retained charges and named-source invariance for multiple source counts and fresh-resource counts.
These tests exercise the adapter and settlement amount calculation, not live vault publication.

### Concurrent publication model

[`JointFundingSettlement.tla`](../../../../formal/tlaplus/cost_accounted_rho/JointFundingSettlement.tla) specifies the publication contract for overlapping funding plans.
Workers can prepare plans concurrently from the same captured balances and cursor positions.
Commit requires a current snapshot and an uncompleted transaction identity.
Both charges and both cursor results publish in one model transition.
Abort changes neither balances nor cursors.

The model checks conservation, nonnegative balances, duplicate rejection, current-plan selection, and cursor changes only with settlement.
Its safe configurations use two workers, two transactions, and two funding sources.
One configuration charges one resource unit and permits only one fee payer, with capacities from zero through two.
Another configuration charges zero resource units and permits both fee payers, with capacities from zero through one.
Unsafe controls remove snapshot freshness or suppress one cursor update.
Each control must violate `EveryReceiptUsesCurrentPlan`.

The model's exhaustive assignment enumeration is a bounded specification oracle, not the native optimization algorithm.
It covers one monetary resource obligation and one funding scope.
It does not prove multi-shard liveness, validator consensus, native checkpoint atomicity, or correct authentication of source permissions.
The integration must connect actual checkpoint behavior to this publication contract without introducing a global funding lock.

## Price, limits, reservations, and ownership

### Multi-outcome tie boundary

A deployment can require one reservation that covers several mutually exclusive execution outcomes.
Each source must cover its largest draw across those outcomes, not the sum of mutually exclusive draws.
The sum of these source holds must fit the signed total exposure limit.
Independent per-outcome selection does not necessarily satisfy that family constraint, even after projecting the complete family domain onto each outcome.

Consider five sources, A through E, with capacities `[2,2,2,2,1]` and canonical cursor zero.
E alone permits the separate one-unit fee.
The signed total exposure limit is four units.
The two resource outcomes have these permissions:

| Outcome | First obligation | Second obligation |
| --- | --- | --- |
| Alpha | One unit from B, C, or D. | One unit from C. |
| Beta | One unit from A or B. | Two units from C or D. |

The independently selected resource vectors are `[0,1,1,0,0]` for Alpha and `[1,0,1,1,0]` for Beta.
Both selections satisfy their respective resource minimax objective and cyclic tie rule.
Each vector also occurs in a complete family that fits the exposure limit.
However, those two preferred vectors cannot occur together within that limit.

The following table includes the separate fee in E's debit:

| Family | Alpha debits | Beta debits | Source holds | Total exposure |
| --- | --- | --- | --- | --- |
| Independent selections | `[0,1,1,0,1]` | `[1,0,1,1,1]` | `[1,1,1,1,1]` | 5, which exceeds the limit. |
| Preserve Beta's tie preference | `[0,0,1,1,1]` | `[1,0,1,1,1]` | `[1,0,1,1,1]` | 4, which fits. |
| Preserve Alpha's tie preference | `[0,1,1,0,1]` | `[0,1,1,1,1]` | `[0,1,1,1,1]` | 4, which fits. |

Both valid alternatives preserve each outcome's optimal descending resource rank.
They disagree only over which outcome retains its preferred labeled contribution vector.
Thus, this example requires family-wide tie ordering, not increased spending authority or a different resource price.
The fee remains separate and unchanged.

`independent_case_ties_can_exceed_family_exposure` proves the equal ranks, cyclic preferences, and exposure arithmetic in `FundingPolicyComposition.v`.
The native regression calls the actual joint fee selector and independently enumerates the complete feasible assignment pairs.
The regression finds six feasible pairs under the four-unit limit.
It confirms that separately selecting each projected domain's preferred plan produces an incompatible pair.
This is an integration counterexample, not evidence of a deployed validator or consensus failure.

The fixed-outcome policy alone does not specify a priority between these complete families.
Canonical outcome priority is the selected extension.
The following table compares that choice with the alternatives:

| Alternative | Advantage | Consequence |
| --- | --- | --- |
| Canonical outcome priority, selected | Preserves per-outcome resource fairness as the primary allocation objective. Requires no outcome probabilities. | Earlier outcomes have priority when fairness ranks conflict. Canonical identity and complete-family feasibility remain necessary. |
| Family-wide worst-case fairness, not selected | Balances each source's maximum possible resource contribution across the covered outcomes. Requires no outcome probabilities. | Can sacrifice per-outcome fairness. Does not necessarily minimize fee-inclusive exposure, total holds, or actual spending. Needs deterministic ties. |
| Probability-weighted fairness, not selected | Can represent expected resource contributions when a justified probability model exists. | Needs authenticated probabilities and an update policy. Expected contributions do not bound worst-case exposure. |
| Aggregate hypothetical contributions, not selected | Gives a simple sum across covered outcomes. | Counts mutually exclusive draws together. Duplicate or subdivided outcomes can change the objective unless the model prevents this. |
| Establish the realized outcome before reservation | Uses the existing single-outcome rule with no hypothetical-outcome conflict. | Requires sound dependent evidence. It cannot replace conservative funding for programs whose outcome remains unknown. |
| Reject incompatible independent selections | Avoids unauthorized exposure. | Rejects some fundable deployments. It is not a complete family optimizer. |

### Selected canonical outcome priority

Canonical outcome priority ranks complete feasible families, not independently selected outcome plans.
The selection does not predict or choose the execution outcome.
Every covered outcome must remain fundable under the same authorized source holds and total exposure limit.

Outcome order comes from authenticated semantic identities.
Arrival order, container order, and caller-selected labels must not determine priority.
Canonical identity must distinguish materially different outcomes and handle duplicate representations without changing the result.
The exact identity encoding and complete comparison key remain implementation contracts that require verification.

Fairness ranks precede allocation ties across the family.
For fixed authorized acquisition costs, compare the first outcome's descending resource rank, then the second outcome's rank, and continue in canonical order.
Only families with the same complete rank sequence proceed to contribution ties in canonical outcome order.
Thus, an earlier outcome's residual preference cannot defeat a later outcome's better fairness rank.
An earlier outcome's better fairness rank can defeat a later outcome's better rank.

Each comparison retains only complete feasible families.
The solver must preserve at least one complete witness after every selection step.
Projection onto separate outcome domains followed by independent optimization does not satisfy this requirement.
The preceding counterexample demonstrates that failure even when every outcome retains its optimal fairness rank.

In that example, Alpha precedes Beta only if the authenticated order specifies that precedence.
Both feasible alternatives have the same complete rank sequence.
Alpha's cyclic tie then selects the family with holds `[0,1,1,1,1]`.
If Beta precedes Alpha, its cyclic tie instead selects holds `[1,0,1,1,1]`.
Canonical ordering makes this preference reproducible, not economically neutral.

Separate fees constrain feasibility but do not enter resource fairness ranks.
Fee selection retains its authorized sources and separate cursor.
After all resource ties, compare fee contributions in cyclic source order for each canonical outcome.
Prefer the lexicographically larger cyclic fee vector, then apply canonical resource-assignment witness ordering.
For a separate one-unit fee, this comparison preserves the existing rotating payer preference among feasible completions.
It cannot defeat previously selected resource ranks or contribution ties.
Any acquisition alternatives must first preserve the approved minimum-cost rule for authorized equivalent resources.
Single-outcome families must reproduce option 3, including its fragment classification, fee choice, witness ordering, and cursor transitions.

The planner must not choose a program outcome, assign invented outcome probabilities, increase signed exposure, or select priority from arrival order.
Changing the input serialization order must not change the named result.

### Alternative: family-wide worst-case fairness

Let $`d_i^{(o)}`$ be source $`i`$'s resource contribution for covered outcome $`o`$.
Define its worst-case resource contribution $`w_i`$ as:

```math
w_i=\max_o d_i^{(o)}.
```

This alternative would minimize $`w^\downarrow`$ lexicographically across complete feasible families.
It favors balanced maximum potential resource liability instead of a canonical sequence of per-outcome fairness ranks.
Each source's maximum can occur in a different outcome.
The vector therefore need not equal any actual execution's contribution vector.

This objective can suit sponsors whose primary concern is maximum individual liability across uncertain outcomes.
It does not estimate expected spending or assign a likelihood to any outcome.
A rare outcome can determine the result as strongly as a common outcome.
Outcome probabilities would require a different policy and evidence for those probabilities.

Worst-case resource fairness differs from reservation minimization.
Let $`f_i^{(o)}`$ be the separate fee debit and $`h_i`$ be the required source hold:

```math
h_i=\max_o\left(d_i^{(o)}+f_i^{(o)}\right).
```

Resource maxima alone do not determine these fee-inclusive holds.
Minimizing the largest resource maximum also does not necessarily minimize the sum of all holds.
Signed source limits and the signed total exposure limit remain feasibility constraints under either objective.

The next comparison illustrates the policy difference for two sources and two outcomes.
It compares candidate families, not an asserted complete native feasible domain.
Assume both candidates satisfy all authority, fee, and exposure constraints.
Alpha requires two resource units, and Beta requires four.

| Candidate family | Alpha resource draws | Beta resource draws | Sorted resource maxima |
| --- | --- | --- | --- |
| X | `[2,0]` | `[2,2]` | `[2,2]` |
| Y | `[1,1]` | `[3,1]` | `[3,1]` |

With Alpha first, canonical outcome priority prefers Y between these candidates because Alpha's rank is better.
Worst-case fairness prefers X because its largest potential resource contribution is two rather than three.
Neither choice dominates the other across both objectives.
The alternatives express different funding policies, not interchangeable optimization methods.

Both policies require correlated family feasibility, exact arithmetic, and deterministic complete witnesses.
Worst-case fairness additionally needs a resource-maximum objective and a tie policy among families with equal maxima.
Canonical outcome priority instead needs a stable semantic outcome order and staged rank comparisons.
No benchmark or complexity proof currently establishes that either policy has a faster complete native solver.
The selected policy avoids changing the primary fairness concept, but it does not remove the family-feasibility problem.

### Shared exposure feasibility kernel

[`solve_funding_family_feasibility`](../../../../rholang/src/rust/interpreter/accounting/monetary_allocation/funding_family.rs) checks fixed-obligation families under one total exposure limit.
Each outcome supplies eligible funding edges and lower and upper source-debit bounds.
The caller supplies common physical source capacities and a bounded host-work budget.
Source positions must refer to the same physical purses in every outcome.
The kernel does not authenticate those identities or select economic priority.

The kernel searches integer intervals for shared source holds.
For each interval box, it solves every outcome against the box's upper hold bounds using the existing integer-flow feasibility solver.
An impossible outcome excludes that box.
Lower hold bounds whose sum exceeds authorized exposure also exclude the box.
A feasible family returns its actual componentwise maximum debits, not padded search bounds.

If the provisional witnesses exceed exposure, the kernel splits one hold interval at its midpoint.
It explores both integer subranges unless a valid witness or a work-limit error ends the search.
The implementation uses an explicit stack, not recursive calls or a loop over every token amount.
Search order is only a feasibility mechanism, not canonical economic priority.
The optimizer must not publish the first feasible witness as the approved allocation.

The following pseudocode specifies the search structure:

```text
Validate every outcome and all dimensions.
Initialize bounded hold intervals from source capacities and outcome debit bounds.
Push the initial box.
While a box remains:
    Charge the host-work budget.
    Remove a box from the stack.
    If its lower holds exceed exposure, continue.
    Solve every outcome under its upper holds.
    If any outcome is infeasible, continue.
    Compute actual holds from the outcome witnesses.
    If the maximum of actual and lower holds fits exposure:
        Return the witnesses and their actual holds.
    Split a non-singleton interval into two disjoint integer ranges.
    Push both child boxes.
Return infeasible.
```

Every split strictly reduces the selected interval width on each child path.
The complete finite search can still require exponentially many boxes.
The host-work budget bounds actual execution and cumulative allocation charges.
Budget exhaustion is an error, not evidence of infeasibility.
No polynomial runtime or production throughput guarantee follows from midpoint splitting.

[`FundingFamilyFeasibility.v`](../../../../formal/rocq/cost_accounted_rho/theories/FundingFamilyFeasibility.v) proves the shared-hold characterization for arbitrary finite outcome and source counts.
The proof relates a feasible family to one hold vector that permits a feasible draw for every outcome.
It also proves exact interval partitioning, pruning soundness, candidate soundness, singleton completion, and strict split progress.
Its rejection-certificate theorem composes the pruning and splitting rules.
These theorems assume correct per-outcome feasibility decisions and do not directly verify the Rust executable or its host-work accounting.

Native tests compare the implementation with an independent exhaustive integer oracle.
Coverage includes the five-source counterexample, restricted permissions, lower debit bounds, zero obligations, and exposure limits.
Separate cases cover full-width amounts, cohorts through 129 sources, malformed inputs, and host-work exhaustion.
Parallel pure searches test state isolation, not atomic ledger publication.

### Canonical resource-priority key

[`FundingFamilyResourcePriority`](../../../../rholang/src/rust/interpreter/accounting/monetary_allocation/funding_family_priority.rs) compares resource contributions for fixed-cost outcome families.
Its caller must supply outcomes and physical sources in their authenticated canonical order.
Every outcome row must use the same source cohort and captured resource cursor.
The key does not authenticate identities, validate eligibility, or prove family feasibility.

The key first concatenates each outcome's descending contribution rank in canonical outcome order.
It separately concatenates the contribution rows in cyclic source order.
Comparison minimizes the first sequence, then maximizes the second sequence when all ranks match.
This structure prevents an early allocation tie from defeating a later fairness rank.

Comparison rejects different source counts, outcome counts, per-outcome resource totals, or captured cursors.
Different acquisition costs require the earlier minimum-cost selection stage.
Separate fees must not appear in the resource rows.
Equal keys establish equal resource contributions within this fixed context, not equal complete assignments or equal fee choices.
The remaining fee and witness rules still apply.

[`FundingFamilyPriority.v`](../../../../formal/rocq/cost_accounted_rho/theories/FundingFamilyPriority.v) proves exact boolean checking, reflexivity, transitivity, totality, and antisymmetry of the two-sequence ordering.
Concrete proof examples cover the conflicting residual preferences and fairness-before-ties requirement.
Generated native tests compare against an independent histogram oracle and check the order laws.
The implementation uses checked sums and bounded host-work charges before allocation or sorting.

### Complete fixed-family optimizer

[`optimize_funding_family`](../../../../rholang/src/rust/interpreter/accounting/monetary_allocation/funding_family_optimizer.rs) combines the existing per-outcome optimizers with shared-hold interval search.
Each input outcome specifies fixed resource obligations, eligible edges, total debit capacities, and an optional separate one-unit fee.
Common capacities constrain each physical source's maximum hold across outcomes.
The total exposure limit constrains the sum of those maximum holds.
The caller must provide authenticated canonical outcome, source, and obligation order.

The optimizer compares complete plans in this order:

1. Minimize all per-outcome resource ranks in canonical outcome order.
2. Maximize all cyclic resource contribution vectors in canonical outcome order.
3. Maximize all cyclic fee contribution vectors in canonical outcome order.
4. Minimize canonical resource-assignment witnesses in canonical outcome order.

The third step selects among permitted one-unit fee payers, not among different fee amounts.
An uncharged outcome has an all-zero fee vector.
All resource amounts remain fixed during this optimization.
Selection among differently priced equivalent acquisitions belongs to the earlier minimum-cost stage.

Within a hold box, the optimizer computes each outcome's optimum under that box's upper source bounds.
This relaxed family ignores shared exposure and lower hold bounds, but it includes each outcome's resource and fee feasibility.
Its ordered objective is a lower bound for every feasible family inside the box.
Independent selection is therefore useful as a bound, although it is insufficient as the final family decision.

If the relaxed family fits the box and exposure limit, it solves that box.
Otherwise, the optimizer splits the hold range and explores both children.
A best complete family permits pruning only when a box's relaxed lower bound cannot improve that family.
The optimizer returns only after it resolves the entire remaining search frontier.
If a work limit interrupts the search, it returns an error even when it already found a feasible family.
It must not publish an uncertified partial optimum or report insufficient funds for an interrupted search.

The following pseudocode specifies the optimization structure:

```text
Validate every outcome and initialize the hold box.
Set the best complete family to absent.
Push the initial box.
While a box remains:
    Charge the host-work budget and remove a box.
    Exclude boxes whose lower holds exceed exposure.
    Compute independently optimal resource-and-fee plans under the upper holds.
    Exclude the box if any outcome is infeasible.
    Exclude the box if its lower bound cannot improve the best complete family.
    If the relaxed plans fit the box and shared exposure:
        Retain them as the best complete family.
    Otherwise:
        Split a non-singleton hold interval and push both children.
Return the best complete family, or infeasible if none exists.
```

The returned holds equal the componentwise maximum actual debit across outcomes.
The result contains complete resource assignments and separate fee debits.
It contains no cursor transitions, because temporary search bounds must not define the settlement domain or its unrestricted-versus-restricted classification.
The integration must derive cursor transitions from the authenticated family domain, not from the final search box.

`verify_funding_family_allocation` recomputes the full optimum and compares the submitted holds, fee debits, and resource assignments.
It rejects altered proposals and feasible proposals that do not satisfy canonical priority.
It does not trust a proposer-supplied total exposure or a claimed optimality label.

[`FundingFamilyOptimization.v`](../../../../formal/rocq/cost_accounted_rho/theories/FundingFamilyOptimization.v) proves the branch-and-bound certificate and completed-frontier rules.
Its schedule theorem proves that interleaving independently minimal outcome keys preserves their lower-bound relationship.
The schedule must preserve each outcome's internal objective order, as the four-stage family comparison does.
These proofs require sound per-outcome minima and complete hold-box coverage.
The existing fixed-outcome proofs and interval-partition proofs supply those component contracts.
The abstract proofs do not directly establish a machine-checked refinement of the complete Rust optimizer.

An independent exhaustive native oracle enumerates complete assignment families, including every permitted fee payer.
Tests compare the entire selected family, not only its rank or feasibility.
They cover restricted sponsorship, outcome-specific debit limits, exposure conflicts, zero charges, large source counts, and full-width amounts.
Further tests reject late-budget exhaustion and altered proposals, and compare single-outcome results with the existing allocation policy.

The generic optimizer accumulates fee-inclusive bill bounds in `u128`.
A full-width `u64` resource charge plus a separate fee can exceed `u64` while individual source debits remain representable.
It clips each source bound against that source's capacity before conversion to `u64`.
The formal clipping lemmas preserve feasible source bounds without requiring the aggregate bill to fit one wallet.
The checked phlo layer retains its separate checked-charge limits.

### Checked phlo-family adapter

[`select_phlo_funding_family`](../../../../rholang/src/rust/interpreter/accounting/phlo_execution/family_policy.rs) derives the optimizer input from checked phlo obligations.
All outcomes must use the same checked controls and physical sources.
The adapter rejects repeated custody identities and matrices with incorrect dimensions.
One eligibility column covers all occurrences of each complete resource key.
It preserves resource multiplicity through positive counted quantities, including zero-price resources.

The adapter sorts sources by custody bytes and resource keys by their typed encodings.
It then sorts outcomes by this structural tuple:

1. The separate fee amount, either zero or one.
2. The ordered resource keys, logically repeated by their quantities.
3. The ordered per-unit resource amounts, logically repeated by their quantities.
4. The fee eligibility vector in canonical source order.
5. The resource eligibility matrix in canonical source order, with each column logically repeated by its quantity.

Tuple comparison uses lexicographic order.
Counted runs implement the logical repetition without allocating individual occurrences.
An equal-run comparison consumes the smaller remaining quantity and advances at least one input entry.
This preserves the occurrence-based outcome priority, including zero-price resources.
The allocation graph uses summed amounts instead of repeated columns.
No caller label or arrival position determines priority between distinct funding domains.
Equal tuples describe equal allocation domains within the shared controls and source context.
The adapter retains duplicate outcomes rather than treating duplicate resources as one charge.

Each common hold capacity is the smaller of the source balance and its exposure limit.
Each outcome debit capacity is the smaller of the source balance and its debit limit.
The optimizer enforces both capacities and the signed aggregate exposure limit.
Fee charges count toward these limits but remain outside resource fairness.

The adapter restores selected matrices to the original outcome, source, and obligation coordinates.
It also returns cursor transitions in original outcome order.
Cursor positions, fee masks, and restriction witnesses retain canonical source coordinates.
It checks every restored assignment and reconstructs the maximum source holds.
The reconstructed holds must equal the optimizer result and remain within the exposure limit.
`verify_phlo_funding_family` recomputes the selected family and compares every submitted assignment and hold.
It rejects feasible but noncanonical proposals.

`CheckedPhloFundingIntent::verify_funding_policy` applies the same check to a consent-bound funding family.
It takes sources, limits, outcome permissions, assignments, and holds from that checked family rather than replacement caller inputs.
The caller supplies canonical cursor positions from the authenticated state snapshot.
This check does not replace envelope signature validation or authorize publication against a newer snapshot.

Key encoding, sorting, allocation, and result verification consume the host-work budget.
A budget error remains an error rather than an insufficient-funds result.
The adapter uses stable merge sorting, with logarithmic sorting depth and no recursive traversal.

[`FundingFamilyCanonicalization.v`](../../../../formal/rocq/cost_accounted_rho/theories/FundingFamilyCanonicalization.v) proves custody restoration and exposure invariance under outcome permutation.
Its normalized-key model binds amounts, typed keys, and permissions while retaining occurrence multiplicity.
For equivalent outcomes, it constructs a feasible duplicate-normalized witness whose scheduled objective is no worse.
Copying the least-ranked representative cannot increase maximum source holds.
This result assumes equal rank lengths and a shared capacity and ranking context.
It does not establish uniqueness of every optimizer result or a machine-checked refinement of Rust encoding.

The adapter validates allocation structure, not signatures or resource permissions.
The caller must authenticate source identities, limits, eligibility, and the complete covered outcome family before admission.
The adapter does not publish reservations, settlement, refunds, or cursor changes.
Those operations require the existing authorization and concurrent publication contracts.

### Priority-conditioned family cursors

[`select_funding_family_policy`](../../../../rholang/src/rust/interpreter/accounting/monetary_allocation/funding_family_cursor.rs) derives cursor transitions after complete family optimization.
It uses the authenticated family domain rather than the optimizer's temporary search bounds.
Every hypothetical outcome starts from the same captured resource and fee cursors.
Only the realized outcome can publish a transition.

Equal structural outcome keys form one semantic group.
Duplicate outcomes share the representative group's transition, but retain their original result positions.
A duplicate must not constrain itself through an earlier copy.
The generic selector checks contiguous group identifiers and equal numeric domains within each group.
The phlo adapter additionally derives groups from typed structural keys.

For a target group's resource projection, the selector applies these constraints:

| Family component | Constraint |
| --- | --- |
| Earlier groups | Keep their selected resource contribution vectors. |
| Target group | Permit every resource vector that has a complete family extension. |
| Later groups | Keep their selected descending resource ranks, but permit different source assignments. |
| All groups' fees | Permit every eligible fee choice that completes the constrained family. |
| Shared constraints | Keep all source bounds, debit limits, and total exposure limits. |

These constraints preserve canonical outcome priority without replacing it with family-wide worst-case fairness.
The target rank remains free during domain classification.
Fixing the target's optimal rank would incorrectly classify other permitted resource vectors as unavailable.

An unrestricted projection contains the complete capped simplex for the target resource total.
Here, the capped simplex means all nonnegative integer source vectors with that total and the effective source bounds.
An unrestricted positive charge uses the existing capped allocator's successor cursor.
A restricted positive charge advances the captured resource cursor by one position modulo the cohort size.
A zero resource charge has no resource transition.

The selector checks simplex coverage with iterative interval partitions.
A complete fixed witness can certify an entire interval cell through source bounds, exposure bounds, and unrestricted resource-flow coverage.
Otherwise, the selector splits the cell and checks both children.
An infeasible vector becomes an explicit restriction witness.
Full coverage establishes unrestricted classification.
Failure of one attempted family extension does not establish infeasibility until the complete constrained search finishes.

The search has no fixed two-wallet restriction and does not use recursive traversal.
Its worst case can require singleton cells and combinatorial rank assignments.
The implementation does not claim polynomial complexity or numeric-volume-independent execution time.
Host-work exhaustion returns an error, not an incomplete optimum, an insufficient-funds result, or an assumed restricted domain.

#### Why separate outcome classification is insufficient

Consider three wallets with capacities `(3, 3, 3)` and a total exposure limit of four.
Both cursors start at canonical position zero.
Each outcome permits resource funding from every wallet.

| Outcome | Resource charge | Permitted fee payer | Selected resource draws | Fee draws |
| --- | --- | --- | --- | --- |
| First | 2 | A | `(0, 1, 1)` | `(1, 0, 0)` |
| Second | 3 | B | `(1, 1, 1)` | `(0, 1, 0)` |

The shared holds are `(1, 2, 1)`.
The first outcome alone permits every capped resource vector and selects `(1, 1, 0)`, with successor cursor two.
The family must preserve the second outcome's selected rank while classifying the first outcome.
That requirement excludes the first outcome's `(2, 0, 0)` vector from the conditioned projection.
The family therefore classifies the first outcome as restricted and assigns successor cursor one.
The second outcome's standalone domain is not unrestricted because its fee also needs capacity in wallet B.

#### Fee projection and publication

Fee selection fixes every selected resource vector and all earlier groups' fee choices.
The target fee payer and later fee choices remain free within the complete family constraints.
The selector constructs the exact mask of target payers with a feasible completion.
The existing one-unit capped allocator selects the payer and successor from that mask.
With one feasible payer, the fee cursor remains at its captured position.
An outcome without a fee has no fee transition.

The selection contains positions, not permission to update a ledger.
Publication must bind positions to the captured cursor scope and revision within the same native checkpoint as the realized debits.
Abort and unrealized outcomes must not publish cursor changes.
Disjoint publication must not require a global funding lock.

[`FundingFamilyCursor.v`](../../../../formal/rocq/cost_accounted_rho/theories/FundingFamilyCursor.v) proves conditioned priority, exact fee support, duplicate-group consistency, coverage bounds, and receipt-based transition properties.
Its finite-domain proofs assume the stated feasible-family predicate and the standalone allocator contract.
They do not establish complete Rust refinement, concurrent ledger refinement, or a runtime complexity bound.
Independent exhaustive and generated native tests compare complete families, restriction witnesses, fee masks, and cursor transitions.
Adapter tests also check canonical source coordinates, restored outcome order, duplicate groups, and single-outcome parity.

#### Concurrent publication model

[`FamilyCursorSettlement.tla`](../../../../formal/tlaplus/cost_accounted_rho/FamilyCursorSettlement.tla) separates preparation, outcome selection, commit, and abort across concurrent workers.
Each prepared plan captures balances, cursor positions, a revision, and a complete abstract family certificate.
Commit requires the captured state to match the current scope state.
Commit also requires an unused transaction identity within that scope.
The transition publishes the realized group's debits and both applicable cursor positions together.

The model's scopes contain disjoint balance maps.
They do not represent overlapping cohorts that share a physical wallet.
Shared-wallet publication needs the existing physical-custody dependency checks, not merely different scope identifiers.
The disjoint-preparation invariant checks the guard that the actual preparation action uses.
It establishes preparation independence in the model, not eventual settlement or distributed liveness.

The finite certificate generator provides charged, fee-only, and uncharged outcomes, plus a duplicate outcome alias.
It constrains source debits, aggregate exposure, cursor bounds, and zero-charge behavior.
It does not implement or prove the family optimizer.
Separate restricted and unrestricted fixtures exercise publication of both resource-cursor rules.
The safe configurations use two workers, two disjoint scopes, two transactions per scope, and two payers with initial balances `(2, 2)`.
A charged transaction can debit resource and fee from the first payer, leaving `(0, 2)` with both cursors at position one.
A second charged transaction then has one feasible payer.
The unrestricted resource cursor remains at one, while the restricted resource cursor advances to zero.
Thus, equal explored state counts do not mean that the fixtures publish identical cursor values.

The model checks these properties and corresponding mutation controls:

| Property | Mutation rejected |
| --- | --- |
| Captured-state freshness | Publish a plan after its captured state changes. |
| Exactly-once receipts | Recapture and publish a previously completed transaction. |
| Realized-group accounting | Publish another outcome's debit or cursor transition. |
| Atomic resource cursor | Publish the debit without its resource-cursor successor. |
| Atomic fee cursor | Publish the debit without its fee-cursor successor. |
| Abort isolation | Change the resource cursor during abort. |
| Disjoint preparation | Require every other worker to be idle. |
| Duplicate-group identity | Route a duplicate alias to another group's transition. |

The receipt mutation permits fresh preparation of a completed transaction while retaining the freshness check.
Otherwise, revision checking would mask the missing receipt check and prevent that mutation from testing duplicate execution.
The bounded model does not establish native checkpoint atomicity, restart persistence, or validator agreement.

#### Native two-cursor publication

[`ApplyPhloCostDeploy`](../../../../casper/src/rust/util/rholang/costacc/vault_cost_deploy/phlo.rs) publishes a realized resource contribution and a realized fee contribution.
It uses the separate `SystemVault("applyPhloCost", ...)` method.
The existing `ApplyCostDeploy` source and fee-only interface remain unchanged.
Neither publisher selects a funding policy or authenticates an execution outcome.
The execution adapter must supply the checked policy's realized amounts and transitions at the authenticated root.
The publisher checks transition structure, not the derivation of scope identities or canonical allocation.
Cursor revisions prevent reuse of captured transitions.
They do not deduplicate reservation identities across newly prepared requests.

A cursor transition contains its scope, payer count, expected revision, expected position, successor revision, and successor position.
Resource and fee scopes use different policy contexts.
The native adapter also rejects equal scope hashes, without assuming that hash collisions are impossible.
Both roles use the same complete custody cohort, including owners with zero realized debit.
The number of nonzero allocation rows does not determine the cursor's payer count.

| Realized contribution | Required cursor evidence | Publication |
| --- | --- | --- |
| No resource charge and no fee | Neither transition | Preserve both cursor observations. |
| Positive resource charge only | Resource transition only | Advance the resource cursor. |
| Positive fee only | Fee transition only | Advance the fee cursor. |
| Positive resource charge and fee | Both transitions, with distinct scopes and equal payer counts | Acquire both pairs together and advance both cursors. |

The publisher validates contribution presence and numeric bounds before cursor initialization.
It initializes only participating scopes through the existing race-safe map update.
An absent cursor and an initialized zero cursor have the same selection value.
They remain distinct snapshot observations.
The publisher does not add a new presence-based rejection rule.

For two roles, one RSpace join acquires all four numeric cells.
A waiting request therefore holds neither cursor while it waits for the other.
The publisher checks both revisions and both positions before it calls the amount settlement routine once.
Success publishes both successors.
Failure publishes the acquired values, not the request's stale expected values.
Independent scope sets need no global funding lock.
Shared-wallet balance conflicts still require the existing custody checks and runtime transaction boundary.

The complete operation follows this order:

1. Validate authorization, amounts, cursor presence, bounds, and distinct role scopes.
2. Initialize participating scopes if they are absent.
3. Acquire all participating cursor cells in one join.
4. Compare every acquired cursor with its captured expectation.
5. Apply the complete amount settlement once.
6. Publish all successors on success, or restore acquired values on failure.
7. Commit the runtime checkpoint only after the complete system deployment succeeds.

Returning original cursor values does not restore prior absence or undo partially applied purse operations.
Runtime rollback must restore the original root after rejection, interpreter failure, malformed results, or a missing reply.
The contract alone does not establish that rollback guarantee.

[`FundingCursorCells.tla`](../../../../formal/tlaplus/cost_accounted_rho/FundingCursorCells.tla) refines the joint acquisition and publication boundary.
Its finite configuration uses two workers and three initialized scopes.
Each worker requires zero, one, or two scopes.
These are cursor roles, not a limit on funding wallets.
Publication steps can interleave between workers with overlapping or disjoint scope sets.

| Invariant | Corresponding verification |
| --- | --- |
| One owner per scope | TLC ownership invariant and Loom availability checks. |
| No partial acquisition | TLC mutation control, Loom overlapping requests, and native reversed-order requests. |
| Coherent acquired values | TLC invariant and Loom value checks under acquired scope ownership. |
| Both positive contributions have successors | TLC publication mutation, Rocq pair theorem, and native balance/cursor assertions. |
| A captured revision cannot charge twice | TLC revision mutation, Rocq reuse theorem, and native stale-role rejection. |
| Failure restores acquired values | TLC restoration mutation and native root rollback tests. |
| Disjoint acquisition remains enabled | TLC enabled-action invariant and Loom disjoint acquisition test. |

[`ContributionCursor.v`](../../../../formal/rocq/cost_accounted_rho/theories/ContributionCursor.v) proves pair-check obligations and positive-contribution revision accounting for arbitrary natural-number bounds.
It also proves rejection of reused plans when either role has a positive contribution.
[`FeeCursorCells.tla`](../../../../formal/tlaplus/cost_accounted_rho/FeeCursorCells.tla) separately models initialization and split revision/position publication.
The new acquisition model abstracts each numeric pair as one coherent scope cell.
It does not model purse balances, crash recovery, or absent-cell rollback.

The [Loom tests](../../../../formal/loom/cost_accounting/tests/funding_cursor_cells.rs) exercise an atomic scope-set acquisition abstraction.
They do not execute RSpace or establish a refinement proof for its matcher.
The [native regression tests](../../../../casper/tests/util/rholang/funding_cursor_vault.rs) exercise the actual contract, runtime rollback, and replay.
Generated request tests cover contribution presence and transport fields across payer counts, revisions, and positions.
These tests and models do not establish complete network admission or validator agreement for offered deploys.

#### Checked wallet settlement requests

[`CheckedDirectWalletPolicy::capture_settlement`](../../../../casper/src/rust/util/rholang/costacc/direct_wallet_funding/settlement.rs) matches supplied execution evidence against the complete prepared outcome family.
The result retains the original signed envelope, immutable funding snapshot, and selected native capture.
It does not accept replacement payer addresses, resource scopes, fee scopes, or charge amounts.
Both funded envelope formats use the same adapter.

Request preparation resolves every captured custody through the snapshot's authenticated payer map.
The canonical custody sequence must match the complete map, including sources with zero holds.
Each native allocation uses the `General` purse role.
The source's acquisition charge becomes the resource burn, and its separate fee remains a fee.
The source hold must equal acquisition charge plus fee plus refund, using checked signed arithmetic.

| Source amounts | Native request behavior | Cohort behavior |
| --- | --- | --- |
| Zero hold and all other amounts zero | Omit the monetary row. | Retain the owner in both cursor domains. |
| Positive hold with zero acquisition and fee | Reserve and return the full hold. | Retain the owner. |
| Positive acquisition or fee | Publish the exact checked amounts. | Use the complete canonical cohort size. |
| Missing custody, negative amount, overflow, or inconsistent sum | Reject the complete request. | Do not publish a partial request. |

Preparation checks each positive role against its snapshot scope, observed revision, and observed position.
An absent cursor uses the established initial selection value, but the snapshot retains its absence.
The adapter does not derive payer count from the number of nonzero monetary rows.
Host-work reservations precede the row traversal and output allocation.

`PreparedDirectWalletSettlement` retains checked context even when every monetary row is zero.
Its execution view implements `SystemDeployTrait` without exposing a mutable raw request.
Moving or swapping execution views also moves their associated checked references.
A caller cannot replace only the underlying request through this interface.

An absent monetary request does not authorize skipping execution, resource-stack effects, receipts, or envelope identity checks.
A full refund still requires a native request because its hold is positive.
Neither case advances a cursor, so cursor checks alone cannot prevent repeated zero-charge execution.
The complete deployment lifecycle must provide that protection.

Request preparation has no runtime handle and does not change the runtime root.
The funding snapshot identifies the pre-funding state, which can differ from the state after body execution.
The later execution adapter must bind the funding snapshot, authenticated execution, and settlement start state.
It must not reset to the funding snapshot merely to execute this request.
The receiver, lifecycle identity, and deterministic random seed remain trusted execution-context inputs.

[`NativeFundingSnapshot.v`](../../../../formal/rocq/cost_accounted_rho/theories/NativeFundingSnapshot.v) proves exact wallet projection and complete source correspondence for arbitrary finite lists.
The proofs preserve amounts and cohort size and show that omitted zero-hold rows contain no charge or refund.
Missing wallet mappings fail even when the source hold is zero.
Positive holds remain present when the entire hold becomes a refund.

The [projection property tests](../../../../casper/src/rust/util/rholang/costacc/direct_wallet_funding/settlement/tests.rs) exercise these obligations with multiple wallets and full-width native amounts.
The [SystemVault snapshot test](../../../../casper/tests/util/rholang/wallet_snapshot_state.rs) checks envelope retention, checked-view swaps, full refunds, zero requests, publication, and replay.
These checks do not authenticate supplied runtime observations or complete network admission for offered deploys.

### Signed family capture

[`plan_funding_policy`](../../../../rholang/src/rust/interpreter/accounting/phlo_execution/policy_capture.rs) constructs a checked policy from a signed funding intent and captured cursors.
The snapshot supplies the canonical custody list and separate resource and fee cursor records.
Each cursor record contains a scope identifier, revision, and position.
The constructor checks exact custody correspondence and cursor bounds before policy selection.
It derives both selection positions from those snapshots rather than separate caller arguments.

The constructor recomputes the complete family and checks the consent-bound assignments and holds.
The returned policy retains the signed intent, snapshots, and selected family in private fields.
A later capture cannot replace these values.
It can only select an original outcome index and provide work limits.
That single index determines the canonical funding amounts and both optional cursor transitions.

Positive charges require a successor revision within the existing signed integer representation.
A family cannot pass the cursor checks if any covered outcome requires an exhausted cursor to advance.
A dimension with zero charge in every outcome needs no successor revision.
Revision exhaustion is a representation error, not an insufficient-funds result.
Zero charges produce no transition, including when another dimension has a positive charge.

Each captured source preserves its custody and hold.
Its acquisition charge equals its debit minus its separate fee.
Its refund equals its hold minus its debit.
The capture does not authorize refunds to another source or change the funding policy for the realized branch.

The signature authenticates the funding intent, not the ledger snapshot.
The native caller must authenticate the snapshot's scope, custody binding, revision, and state root.
The capture preserves generic custody bytes and introduces no replacement native scope hash.
Native callers can retain the existing fixed-width custody hash and trusted policy context.

Resource and fee fields have distinct roles in the capture.
The native adapter derives role-specific scopes and rejects equal scope hashes.
The vault publisher also rejects equal scopes before acquisition.
It does not attempt to acquire one scope twice.

This checked capture does not mutate balances, reservations, cursors, receipts, or application state.
The caller must establish the realized outcome and publish its complete effects atomically against the authenticated state.
The capture alone does not prove native publication, restart persistence, or replay agreement.

`CheckedPhloExecution` validates supplied resource partitions and cost arithmetic.
It does not prove that the runtime observed those resources.
`PhloOutcome` also comes from the caller, so a valid obligation projection does not authenticate an execution result.
The runtime adapter must derive both inputs from the actual execution and retain their deployment and state identity through replay.

[`FundingFamilyCapture.v`](../../../../formal/rocq/cost_accounted_rho/theories/FundingFamilyCapture.v) composes scoped successors, full-family headroom, branch indexing, and native amount conservation.
The amount-correspondence premise connects each branch's zero or positive charge to the presence of its selected cursor transition.
The proof does not derive that premise from arbitrary caller-supplied transitions.
Native construction obtains transitions from the recomputed selection, and generated tests check their correspondence with captured amounts.
The formal expected custody list remains authoritative input rather than a proof of ledger authentication.

#### Native amount preflight

The generic funding model uses unsigned amounts, but native purse operations use signed 64-bit amounts.
[`into_native`](../../../../rholang/src/rust/interpreter/accounting/phlo_execution/native_policy.rs) checks every selected source hold before execution.
The returned `CheckedNativeSignedPhloFamilyPolicy` owns the same checked policy and cannot accept replacement funding data.
This check covers every outcome, including an outcome that execution does not select.
An oversized hold remains invalid when the selected outcome would refund the entire hold.

Let $`M`$ denote `i64::MAX`.
For each source, let $`H`$ denote its hold, $`D`$ its realized debit, and $`F`$ its fee.
The checked family and native preflight establish:

```math
0 \leq F \leq D \leq H \leq M.
```

Acquisition is $`D-F`$, and refund is $`H-D`$.
Both fit the native representation, and their sum with the fee equals the original hold.
The check does not narrow source capacities, aggregate exposure, or aggregate holds to a signed 64-bit amount.
For example, two distinct sources can each hold `i64::MAX` without violating the per-source representation limit.

`capture_case` selects one original outcome from the native policy.
It converts that scoped capture's own rows in canonical custody order.
The returned `NativeScopedPhloFundingCapture` retains the scoped capture and all converted amounts in private fields.
Callers cannot supply a different branch, custody, amount, or cursor during conversion.

Preflight reserves the complete hold-scan work before inspecting amounts.
Capture reserves conversion work and vector storage before creating the native amount vector.
Work exhaustion, allocation failure, and invalid branch selection return errors without publishing a partial result.
These operations do not mutate a ledger.

`family_native_hold_preflight_exact` proves that the preflight accepts exactly the families with representable source holds.
`family_native_preflight_covers_every_branch_and_source` proves per-source bounds and conservation for every branch of a checked snapshot.
`family_native_preflight_covers_captured_case` connects these results to the indexed scoped capture.
These results do not establish snapshot authentication or native settlement publication.

The signed boundary test covers charged and zero-charge outcomes at the maximum hold and rejects the next integer before capture.
Generated tests check per-source bounds, conservation, and correspondence between native and scoped captures.
Source-order tests require the same canonical amounts after input permutation.
Budget tests distinguish earlier capture failure from failure during native conversion and check success at the exact required budget.

#### Pinned native funding snapshot

[`read_policy_snapshot`](../../../../casper/src/rust/util/rholang/costacc/direct_wallet_funding/policy_snapshot.rs) reads authorized wallet balances and both allocation cursors at one explicit state root.
The method accepts an authenticated direct-wallet envelope and a runtime manager.
It constructs `RuntimeManagerSupplyReader` internally, which supplies that root to every state query.
It does not accept a caller-defined reader, wallet cohort, or cursor scope.

The cohort contains every authorized physical custody exactly once, in canonical byte order.
Source order cannot change the cohort or its scopes.
The fee scope retains the existing native fee policy context and cohort hash.
The resource scope uses a distinct context for canonical-family lexicographic minimax.
The adapter rejects equal scope keys, including an unexpected hash collision.

Each cursor observation retains its storage presence and revision.
An absent cursor remains distinct from an initialized cursor until policy selection substitutes the initial cursor value.
The snapshot retains both original observations for subsequent settlement checks.
Wallet reads use bounded parallelism, and both cursor reads can proceed concurrently.
The adapter neither locks all wallets globally nor mutates the captured state.

`bind_native_family` requires the checked family to match the snapshot's complete sources, including balances and signed limits.
It reuses signed-consent validation, then selects and preflights the native family policy.
The returned wrapper retains the original snapshot and the checked policy in private fields.
Callers cannot replace the root, cohort, cursor roles, or native funding amounts after binding.

The adapter reserves logical read, row, and payload charges before their associated work.
Canonical ordering uses the existing budgeted key-order implementation.
Binding reserves both logical source-row work and source-map payload before inherited validation.
These charges do not measure allocator overhead or every cryptographic operation.
Existing member, wire, source, and consent limits bound inherited validation separately.
Runtime query costs remain a separate implementation boundary.

[`NativeFundingSnapshot.v`](../../../../formal/rocq/cost_accounted_rho/theories/NativeFundingSnapshot.v) models immutable root-indexed reads and their connection to checked family binding.
Its twelve proofs cover root retention, exact cohort membership, cursor observations, scope separation, and binding identity.
The model treats authenticated membership and immutable root-indexed storage as explicit premises.
It does not prove the Rust query implementation or wallet-capacity conversion directly.

[`PinnedFundingSnapshot.tla`](../../../../formal/tlaplus/cost_accounted_rho/PinnedFundingSnapshot.tla) models two concurrent readers, two wallets, and two cursor roles.
Each reader can have two reads in flight while the live root changes twice.
The safe configuration checks eight invariants across 70,579 distinct states.
One negative control reads the current root instead of the pinned root and violates `SnapshotAtCapturedRoot`.
Another returns the live root to its starting value and still produces a mixed-state snapshot, violating `ABAAssembly`.
Thus, matching root labels before and after a read do not establish snapshot consistency.
These bounded checks do not establish unbounded production liveness.

The runtime regression reads old, intermediate, and new roots concurrently after independent wallet and cursor fixture updates.
It checks exact balances, cursor presence, revisions, scopes, and unchanged stored roots.
Signed binding tests cover fee-only and zero-charge outcomes, native amounts, source-capacity mismatch, and exhausted work budgets.
Generated tests cover cohort permutations, cursor positions, revisions, and absent cursors.
Read-failure tests require rejection without a partial snapshot and verify that concurrent reads release their active guards.

This adapter prepares a read-only funding policy.
It does not activate the new funding path, authenticate the realized execution outcome, or publish settlement.
Atomic publication must still check both captured cursor observations together with the applicable wallet state.
Separate cursor fixture updates in tests do not constitute an atomic settlement implementation.

### Remaining integration requirements

The existing counterexample proves why independent selection is insufficient.
The fixed-family optimizer addresses allocation for its stated integer-flow fragment.
The phlo adapter derives structural outcome order but does not authenticate eligibility, select acquisition alternatives, or publish native settlement.
The existing concurrent publication model does not yet establish complete-family selection correctness.

The family implementation must establish these properties:

- Every selected family satisfies all obligations, source permissions, source limits, and total exposure limits.
- The selected family is optimal under the complete ordered objective, not only under separate projections.
- Input permutations and duplicate semantic representations do not change the named result.
- A single-outcome family agrees with the existing resource and fee policy.
- Only the realized settlement publishes its applicable cursor updates and debits.
- Stale concurrent plans cannot publish against changed balances, permissions, holds, or cursor state.
- Infeasibility remains distinct from malformed inputs and bounded-work exhaustion.

Regression cases must cover conflicting ties, conflicting ranks, fee-restricted completions, zero charges, and source counts greater than two.
Generated tests must compare bounded families against an independent exhaustive oracle.
Formal publication models must include concurrent preparation, settlement, abort, and stale-state rejection.
These requirements do not authorize changes to Casper architecture, persistent deployment escrow, or a global funding lock.

### Price and reservation constraints

`phloPrice` specifies the offered price, not a fairness weight.
Separate owner ceilings constrain that price without changing the allocation objective.
An actual schedule price must satisfy every required applicable price ceiling.
Perform the resource-to-money calculation before comparing contribution vectors in a common settlement unit.
Different conversion quotes must not silently become contribution weights.

`phloLimit` and persistent allowances continue to constrain the execution and permitted draws.
Fairness cannot increase a limit or make an incompatible source eligible.
Wallet top-ups increase balances, but they do not automatically increase captured spending permission.

Upfront planning must provide feasible source-specific backing for every covered execution outcome.
Alternative branches can require aggregate temporary exposure above the maximum retained charge.
That exposure still requires explicit consent.
Minimax retained contributions do not, by themselves, minimize or authorize temporary reservations.

Actual settlement must remain within the captured reservations and permissions.
A reservation can restrict later allocations, so fairness over reserved capacity does not prove fairness over every originally available purse balance.
The reservation-to-settlement proof must state the feasible set used at each stage and justify their relationship.
Do not claim that independently fair branch plans automatically produce a globally fair reservation.

Unused current reservations return to their captured source custody.
Transferred prepaid rights follow their authorized ownership and location, without automatic reimbursement of historical sponsors.
Ownership transfer does not reset consumed allowance or authorize settlement to substitute a new payer for an existing captured obligation.

Keep reserve, consume, fee, and refund effects within the existing native checkpoint.
Do not introduce a persistent deployment escrow table, another spendable ledger, or a global funding lock.

## Solver contract and verification

The following procedure defines the selection contract, not a production enumeration algorithm.

```text
Validate the captured scope, resource evidence, permissions, and limits.
Discharge compatible prepaid resources and determine the fixed new obligation.
Construct the complete feasible-set representation for that obligation.
Reject infeasibility without publishing a partial economic effect.
Find the least descending contribution vector in lexicographic order.
Resolve equal-ranked plans with the specified canonical tie contract.
Retain the full assignment, source identities, consent, and backing evidence.
```

A feasibility checker proves that a candidate is permitted, not that it is optimal.
The implementation needs a verified selection algorithm or a checkable optimality certificate for the supported constraint domain.
Sorting one candidate does not establish optimality.
Solver resource exhaustion must not silently return a merely feasible allocation as the approved optimum.

| Verification layer | Required evidence |
| --- | --- |
| Rocq | Finite feasible-set existence, exact ordering, optimal sorted-rank uniqueness, source-label invariance, and all-to-all allocator refinement. |
| Restricted solver | Sound and complete eligible-assignment representation, optimality, deterministic ties, and checked integer boundaries. |
| Property tests | Independent exhaustive small-state oracle, arbitrary bounded cohorts, repeated custody aliases, restricted edges, caps, zeros, and source permutations. |
| Negative controls | Detect leximin substitution, maximum-only ranking, missing eligibility, duplicated custody, ignored caps, and iteration-order ties. |
| Reservation refinement | Every covered outcome remains feasible, retained contributions obey their captured bounds, and refunds conserve source custody. |
| TLA+ and native concurrency | Independent validator states, overlapping custody, disjoint scopes, top-ups, transfers, duplicate delivery, abort, and atomic publication. |
| Loom and integration tests | Actual Rust synchronization where applicable, native checkpoint interruption, replay equality, and signer/client evidence mutation. |

Small-state enumeration is an independent test oracle, not the proposed production algorithm.
The production algorithm needs explicit complexity and resource bounds for its supported constraint domain.

`NPayerFundingSolver.v` packages the arbitrary finite-payer contract. Its certificate
requires a valid integer assignment, a complete minimax cut certificate, an exact
optimal-domain description, a cyclic contribution certificate, and a row-major
witness certificate. The kernel-checked result proves conservation, source bounds,
global minimax selection, deterministic cyclic ties, and the least funding witness.
The native allocator remains separately bounded by the configured payer cap. The
formal contract does not claim that an unbounded search is safe for a node.

The batch theorem checks each fixed assignment against the remaining capacity after earlier assignments.
Batch admission is equivalent to individual feasibility plus a combined debit within each source's initial capacity.
Any permutation of those fixed assignments preserves feasibility and the final balance of every source.
This theorem does not permit independent plans to reuse the same available balance.
For example, two individually valid one-unit draws cannot both spend a purse that contains one unit.

The theorem covers fixed monetary assignments with unchanged eligibility, demand, and source identities.
It does not prove that replanning after each debit gives the same allocation in every order.
It also does not establish native checkpoint atomicity, duplicate-operation handling, cursor publication, or distributed validator agreement.
Those properties require their respective lifecycle and replay evidence before production integration.
The native batch regression tests use the assignment checker with residual capacities and restricted funding edges.
An independent property-test oracle checks combined debits, exact column sums, and permitted edges with 128-bit arithmetic.
Generated cases include different obligation counts and eligibility matrices for each entry, empty obligations, insufficient balances, and incorrect demands.
The oracle checks both the original entry order and its reverse.

[`check_fixed_funding_batch`](../../../../rholang/src/rust/interpreter/accounting/monetary_allocation/funding_batch.rs) checks this composition before a caller publishes funding changes.
All entries use the same canonical source index. Each entry supplies its own obligations, eligibility matrix, and fixed assignment.
The checker validates each entry against residual balances and returns combined debits, remaining balances, and the total charge.
It returns an error for the entire candidate if any entry fails. Caller balances remain unchanged.
Each source uses checked 64-bit arithmetic. The batch total uses checked 128-bit arithmetic because different sources can each fund large charges.

Source, obligation, and entry caps bound the input dimensions.
Host-work reservations cover matrix checks and temporary balance storage before the corresponding work occurs.
The checker uses private temporary balances and needs no shared funding lock.
It does not select a plan, authorize custody, advance a cursor, or publish runtime state.
Admission must bind the checked batch to authenticated source identities and the applicable state before publication.
The fixed-plan permutation theorem does not authorize changing the canonical deployment order.

[Bounded-row funding feasibility](bounded-funding-feasibility.md) supplies lower and upper contribution constraints for the fixed-flow optimizer.
It preserves exact obligations while it reassigns their funding among eligible sources.

[Fixed-flow minimax rank](fixed-flow-minimax.md) describes the staged native rank selector and its independent cut certificates.
Rank certification does not select economic ties or replace settlement integration.
No fixed two-wallet assumption is permitted.
Configured source and obligation caps remain necessary for bounded host work.

## Verification progress

[`LexicographicMinimax.v`](../../../../formal/rocq/cost_accounted_rho/theories/LexicographicMinimax.v) formalizes descending ranks and exact lexicographic comparison over arbitrary finite natural-number lists.
It proves rank preservation under source permutation, source-count preservation, comparison reflection, transitivity, and antisymmetry.
It also proves that selecting a least-ranked candidate retains a supplied candidate and yields the least rank in that supplied set.
Reordering candidates preserves the optimal sorted rank, although it can change which equal-ranked labeled candidate supplies the witness.

Global optimality requires an explicit complete-cover hypothesis for the feasible set.
The theorem does not infer completeness from a solver's partial search results.
The finite candidate selector is an existence construction and test reference, not a production search algorithm or approved tie policy.
Equal-ranked optima have identical sorted ranks, not necessarily identical purse assignments.

Generated model checks cover all 15,625 pairs of three-source vectors with entries from zero through four.
Negative examples distinguish leximin contribution, maximum-only comparison, equal-ranked purse permutations, and incomplete candidate sets.
The natural-number theorems are not bounded by those generated examples.
The fixed-total funding contract still restricts which inputs constitute comparable production allocations.

The native [`FundingBurdenRank`](../../../../rholang/src/rust/interpreter/accounting/monetary_allocation/funding_rank.rs) constructs a rank from checked `FundingAssignmentTotals`.
It sorts a private copy and preserves the original source-ordered assignment.
Comparison rejects different source counts or different total obligations.
It returns an equal rank without selecting a payer, advancing a cursor, or publishing an economic effect.

Equal dimensions and totals do not authenticate a common cohort, eligibility relation, or captured state.
The caller must retain and validate those complete witnesses before treating the comparison as a valid funding decision.
The comparator alone proves neither candidate feasibility beyond its supplied assignment check nor global optimality.

The native test contract uses an independent frequency-count oracle rather than another descending-vector comparison.
At the largest contribution value with unequal occurrence counts, fewer occurrences give the better rank for equal-length vectors.
Tests cover documented alternatives, integer boundaries, unchanged assignments, and rejection of incomparable inputs.
Exhaustive tests cover every three-source pair with equal totals from zero through eight.
Generated tests cover 512 cases with one through 129 sources, three same-total allocations, order laws, and source permutation.

An additional test enumerates complete feasible sets for one through four sources, with each capacity from zero through three.
It compares the existing all-to-all allocator against the independent histogram optimum for every available total and every cursor position.
This bounded check supports the expected refinement but does not replace its general proof.

On September 10, 2026, Rocq compilation and recursive kernel checking passed for the minimax module.
All nine printed assumption reports were closed under the global context.
The module contains 221 lines and 23 proof terms.
Its SHA-256 digest is `9978c42a7c98e9ab62af04cfd6db10778539b37c19df6994c88a79514ffca4c3`.
The 56 native monetary-allocation tests and strict library/test Clippy checks also passed.
The native rank source digest is `17c51792e193f7430d301fd1bbfee206977f94433ffded6a70e18c5110a8f03f`.
Proof commands used a 2 GiB systemd memory cap, and native commands used a 6 GiB cap, with swap disabled.
The full campaign gate was not rerun for this focused change.

All-to-all allocator refinement, restricted tie selection, efficient native feasible-set handling, and native settlement integration remain required.
These pure comparison checks do not replace independent-validator models, Loom tests of actual synchronization, or native checkpoint fault tests.

### Complete fixed-flow reference

[`CompleteFundingCandidates.v`](../../../../formal/rocq/cost_accounted_rho/theories/CompleteFundingCandidates.v) supplies a complete reference construction for the fixed integer-flow fragment.
The proof uses arbitrary finite source and obligation counts, a rectangular assignment matrix, captured capacities, and an eligibility relation.
It does not impose a two-source limit.

Every feasible matrix entry is bounded by the total obligation because entries are nonnegative and each column must equal its required amount.
The reference therefore enumerates matrices with each entry between zero and that total, then applies the existing assignment checker.
The enumeration bound follows from the contract, not from a guessed maximum transfer size.
The resulting list contains exactly the rectangular feasible assignments.
An empty list is equivalent to infeasibility for this fragment.

The reference also connects minimax ranking to a complete assignment.
For a nonempty candidate list, its theorem produces a valid matrix whose contribution vector attains the least rank over all feasible matrices.
The matrix retains source-to-obligation edges and source positions.
It is not replaced by the sorted rank used for comparison.

The reference has exponential enumeration cost and must not become the production solver.
Its purpose is to establish complete coverage and provide an independent small-state oracle for an efficient implementation.
The theorem does not prove complete coverage of arbitrary Rholang branches, compound alternatives, conversions, or native resource provenance.
Those cases require their own reduction and coverage arguments.
The mathematical construction includes zero-source cases, while the native API explicitly requires a nonempty funding cohort.

Examples cover restricted obligations, misleading aggregate capacity, three-source funding, zero-capacity sources, and zero obligations.
Generated formal checks compare all 16 two-source eligibility graphs and nine capacity pairs against a separate two-obligation feasibility formula.
The native regression checks all 81 bounded matrices in each of those 144 cases, for 11,664 assignment checks.
Each native result is also compared with the existing independent wide-integer validity oracle.

On September 10, 2026, the reference passed Rocq compilation and recursive kernel checking under a 2 GiB memory cap with swap disabled.
All seven printed assumption reports were closed under the global context.
The source contains 200 lines and 13 proof terms.
Its SHA-256 digest is `d9b2d687578fdf300cd5163e787ceaeb09d0bcfc8ef0eef0ba0c430195bdb852`.
All 57 native monetary-allocation tests and strict library/test Clippy checks passed under a 6 GiB systemd memory cap with swap disabled.
The assignment-checker source digest is `800783f95939a787d286315f5f0967b9eff2cd4b3c992a184161ed0cdce59cc0`.
This result does not complete the production solver or the campaign verification gate.

### Restricted-funding rejection certificate

A failed search path does not establish insufficient funding.
The solver must distinguish a valid allocation, certified infeasibility, invalid input, and exhausted host-work limits.
[`FundingDeficitCertificate.v`](../../../../formal/rocq/cost_accounted_rho/theories/FundingDeficitCertificate.v) proves a checkable reason for infeasibility in the fixed integer-flow fragment.

Let $`S`$ be a selected set of obligations.
Let $`N(S)`$ contain every source permitted to fund at least one obligation in that set.
Let $`q_j`$ denote obligation demand and $`C_i`$ denote captured source capacity.
A deficit certificate satisfies:

```math
\sum_{j\in S}q_j > \sum_{i\in N(S)}C_i.
```

Count each physical source once, even when that source can fund several selected obligations.
Include every eligible neighbor, not only the sources used by one failed search.
Recompute neighbors from the supplied eligibility matrix rather than trusting a claimed neighbor list.

For example, purse A has one unit and purse B has 100 units.
An obligation requires two units but permits only purse A.
Selecting that obligation proves infeasibility, despite aggregate capacity of 101 units.
Conversely, a greedy allocation failure is not a certificate when another eligible assignment succeeds.

The proof restricts any valid assignment to the selected columns.
Nonneighbor sources contribute zero, and each neighbor contributes at most its capacity.
Row-column conservation then proves that every valid assignment covers every selected cut.
The strict deficit inequality contradicts that necessary condition.
The theorem applies to arbitrary finite source and obligation counts, including empty selections and zero amounts.

The native [`check_funding_deficit`](../../../../rholang/src/rust/interpreter/accounting/monetary_allocation/funding_deficit.rs) implements this certificate check without mutable shared state.
It checks independent source and obligation caps, matrix dimensions, and selection length before calculating the cut.
It rejects a total obligation that exceeds `u64`, consistent with the native assignment representation.
It sums neighboring capacities in checked `u128` arithmetic because aggregate capacity can exceed the representable total charge.
Malformed inputs produce errors, not financial infeasibility.

The certificate checker does not authenticate purse identities, capacities, or eligibility.
Its caller must supply the same authenticated problem that the solver and settlement verifier use.
A `false` result rejects this certificate only.
It does not establish feasibility or prove that a different selected set has no deficit.
The general theorem proves certificate soundness, not completeness of a future certificate-search algorithm.

Native regressions enumerate all 16 two-source eligibility graphs, nine capacity pairs, and nine demand pairs.
For each problem, they compare all four selected sets against complete direct allocation enumeration.
This gives 5,184 certificate checks over 1,296 problems.
Two property tests each run 512 generated cases, including cohorts through 128 sources.
They check valid-assignment non-rejection, reindexing, and agreement with an independent wide-integer neighbor-set calculation.
Boundary examples check omitted neighbors, shared capacity, zero obligations, malformed dimensions, configured caps, and maximum integer values.

The checker has no synchronization or publication operation for Loom to explore.
Concurrent custody and publication tests remain separate integration requirements.
This pure checker must not replace those tests or authorize a new global funding lock.

On September 10, 2026, Rocq compilation and recursive kernel checking passed under a 2 GiB systemd memory cap with swap disabled.
All four printed assumption reports were closed under the global context.
The source contains 136 lines and ten proof terms.
Its SHA-256 digest is `3bf603985eee7a7a420a9a638e80ca367dba8c30157f95962cc9c5d24b387492`.
The native checker digest is `924076fae4996e83c1f45e77e4773a835b3b63cbd3bafc1ca367c828fe803ac3`.
All 64 native monetary-allocation tests passed under a 6 GiB systemd memory cap with swap disabled.
Strict library/test Clippy checks also passed.
This checkpoint does not complete efficient feasibility search, minimax optimization, or native settlement integration.

### Native fixed-flow feasibility search

[`solve_funding_feasibility`](../../../../rholang/src/rust/interpreter/accounting/monetary_allocation/funding_feasibility.rs) now supplies a deterministic integer-flow feasibility kernel.
It uses breadth-first residual paths and sends each path's bottleneck amount, rather than expanding a monetary quantity into individual tokens.
The algorithm follows the shortest-augmenting-path approach of [Edmonds and Karp, JACM 19(2), 1972](https://doi.org/10.1145/321694.321699).
This kernel determines feasibility, not the lexicographic-minimax allocation.
Its traversal order must not become the economic tie policy.

The flow graph contains a source vertex, one vertex per payer, one vertex per obligation, and a sink vertex.
Each source-to-payer edge has capacity $`\min(C_i,M)`$.
Each eligible payer-to-obligation edge has capacity $`M`$.
Each obligation-to-sink edge has capacity $`q_j`$.
Here, $`M=\sum_j q_j`$ is the fixed total demand.
An assignment cannot require more than that total from one payer, so source-capacity clipping preserves feasible assignments.

Paired forward and reverse edges retain residual capacity.
A reverse edge lets the search replace an earlier assignment when another obligation needs that payer.
The regression `reverse_residual_path_repairs_a_greedy_choice` requires this correction.
The search uses iterative queues and path walks, with no recursive production traversal.
Canonical input positions and fixed edge construction order determine traversal order.

The procedure separates construction from independent result verification:

```text
Validate source and obligation caps, dimensions, and total arithmetic.
Reserve host-work units before constructing the residual graph.
While funded demand is less than total demand:
    Find a shortest residual path with breadth-first search.
    If no path exists:
        Select the obligations unreachable from the source.
        Verify their deficit certificate against the original problem.
        Return certified infeasibility only if that check succeeds.
    Transfer the path bottleneck and update both residual directions.
Extract the complete source-to-obligation assignment.
Verify that assignment against the original problem.
Return the verified feasible result.
```

Any failed result check returns `InvalidResult`, not financial infeasibility.
Input errors, host-work exhaustion, allocation failure, and arithmetic errors have separate error variants.
The kernel changes only its local search buffers and the supplied host-work budget.
It does not debit a purse, reserve custody, update a cursor, or publish a checkpoint.
Returned structures remain untrusted data outside this call and require authenticated settlement verification.

The sparse residual graph stores two edges per original edge.
Let $`V=n+m+2`$ and $`E=n+e+m`$, where $`e`$ counts eligible payer-obligation pairs.
The standard shortest-path algorithm has an $`O(VE^2)`$ operation bound, independent of monetary magnitudes.
The graph uses $`O(V+E)`$ storage, while the complete output matrix requires $`O(nm)`$ storage.
This bound does not establish acceptable performance for every configured cohort size.
The final minimax solver still requires workload measurements and its own complexity analysis.

The required `HostWorkBudget` charges input scanning, residual-edge visits, path walks, verification, and a deterministic state-byte measure.
The state measure covers residual edges, graph heads, parents, queue entries, output cells, output rows, certificate selection, and verifier totals.
It uses fixed logical widths rather than platform-dependent `size_of` values.
This measure does not predict exact resident set size (RSS).
It excludes allocator metadata and the caller's existing input buffers.
Process memory limits remain necessary.
Dynamic graph and output construction use checked size arithmetic and fallible capacity reservation.

Tests compare every two-source small problem from the certificate suite against complete direct assignment enumeration.
An additional 512 generated three-source problems use an independent recursive allocation oracle with bounded demands.
That recursion belongs only to the small test oracle, not the production search.
Another 512 generated cases construct feasible assignments through 128 sources and require successful search.
Repeated identical inputs must produce identical results, and payer reindexing must preserve feasibility.
Boundary tests cover zero demand, maximum amounts, excluded edges, configured caps, and malformed problems.
The one-unit and maximum-amount examples consume equal host-work units for the same graph.
Separate tests require work-budget exhaustion to return a host-work error, never certified infeasibility.

The accepted-assignment and deficit-certificate theorems preceded this implementation.
They establish the mathematical contracts of the two independent result checks.
They do not yet establish a mechanized refinement of the residual search, its general completeness, or its host-work accounting.
Those obligations remain required before solver integration is complete.
Neither these pure tests nor the certificate proof establish concurrent custody or replay correctness.

On September 10, 2026, all 72 native monetary-allocation tests and strict library/test Clippy checks passed.
The commands used a 6 GiB systemd memory cap, two build jobs, a two-CPU quota, and disabled swap.
The feasibility source digest at that checkpoint was `0327358bbdb324d46645e5f6cf1e7984a15ebe4e27b57c159215ca9d14b217a3`.
The full campaign gate was not rerun for this focused checkpoint.

### Residual-cut refinement and transition checks

[`FundingResidualCut.v`](../../../../formal/rocq/cost_accounted_rho/theories/FundingResidualCut.v) connects a closed residual cut to the deficit checker.
The theorem uses arbitrary finite source and obligation counts and natural-number amounts.
Its flow is a partial assignment, not an assumed completed allocation.
Each row respects its source capacity, each column stays below its obligation, and ineligible edges carry zero flow.

Let $`r_i`$ be source $`i`$'s assigned row total and $`f_{ij}`$ its assignment to obligation $`j`$.
Let $`F`$ be the total assigned amount, with $`F<M`$ for the unsuccessful search case.
A closed residual cut means that residual reachability cannot expand to an omitted vertex or the sink.
The model states the relevant closure conditions explicitly.

| Condition | Required meaning | Native transition check |
|---|---|---|
| Unreachable payer | Its source edge is saturated at $`\min(C_i,M)`$. | Compare its row total with its clipped capacity. |
| Forward closure | A reachable payer reaches every eligible obligation with positive forward residual capacity. | Check eligible edges from reachable payers. |
| Reverse closure | A reachable obligation reaches every payer that supplies it with positive flow. | Check positive assignments into reachable obligations. |
| Unreachable sink | Every reachable obligation has received its complete demand. | Compare each reachable column with its obligation. |
| Partial conservation | Source flows equal row totals, sink flows equal column totals, and both totals equal $`F`$. | Reconstruct assignments from reverse-edge capacities. |
| Residual pairing | Forward and reverse capacities sum to the original edge capacity. | Check every edge pair after initialization and augmentation. |

Because assignments are nonnegative, each row and edge is at most $`F`$, which is less than $`M`$.
Thus an eligible payer-obligation edge cannot be saturated during this unsuccessful search.
Every eligible neighbor of an unreachable obligation must therefore also be unreachable.
Reverse closure prevents those payers from funding reachable obligations.

An unreachable payer has used its original capacity, not merely a smaller clipped capacity.
Otherwise, saturation at $`M`$ would imply a row total of at least $`M`$, contradicting $`F<M`$.
The selected obligations therefore receive exactly their neighboring sources' original capacity sum.
Reachable obligations are complete, so all remaining unmet demand lies in the selected set.
The selected set must satisfy the deficit check.

`incomplete_residual_cut_has_checked_deficit` proves that conclusion from the stated partial-flow and closure conditions.
`incomplete_residual_cut_excludes_every_valid_assignment` connects the result to the existing infeasibility theorem.
`total_clipped_capacity_preserves_assignments` proves feasibility equivalence before and after capacity clipping.
`complete_partial_assignment_is_valid` proves that full total flow plus column bounds implies exact coverage of every obligation.
Total coverage cannot conceal one underfunded obligation behind another overfunded obligation.

Native tests now inspect the real residual graph after initialization and after every augmentation.
They check edge topology, adjacency-list ownership, residual pairing, row and column conservation, capacity bounds, and eligibility.
Before augmentation, they check a simple parent path and its exact positive bottleneck.
When search stops without a path, they check every residual-cut premise and the resulting deficit against original capacities.
These checks run inside the existing exhaustive and property-based solver tests.
They compile only in test builds and do not change production allocations or host-work charges.

Five negative controls deliberately corrupt a residual state or reachability result.
The controls omit a reverse-capacity update, disconnect source funding, omit forward search, omit reverse search, or omit a sink path.
Each control must fail at its corresponding invariant assertion.
The three-source property test also checks that capacity clipping preserves the exact deterministic solver result.

The new theorem does not assume the desired deficit inequality.
It derives that inequality from the residual-cut premises.
However, a general mechanized proof that native breadth-first search and every augmentation preserve all those premises remains required.
Native transition assertions and generated tests support that correspondence but do not replace its general proof.
The shortest-path complexity refinement, host-work refinement, minimax optimizer, and settlement integration also remain required.

On September 10, 2026, this module passed Rocq compilation and recursive kernel checking under a 2 GiB systemd memory cap.
All eight printed assumption reports were closed under the global context.
The module contains 204 lines and 12 proof terms.
Its SHA-256 digest is `796b9fc8f43ba83e682ed9051d48f5c0704a3aef12ef9d9e7d4487f5a8a48126`.
The native feasibility source digest at that checkpoint was `9e3657dadb17ad8068e7c524b5774f9e7b65c866231ef139995558a8abe4f27d`.
All 77 monetary-allocation tests passed under a 6 GiB systemd memory cap with two build jobs and two test threads.
Strict library/test Clippy checks also passed.
Both command groups disabled swap.

### Residual transfers and path conservation

[`FundingResidualTransfer.v`](../../../../formal/rocq/cost_accounted_rho/theories/FundingResidualTransfer.v) proves the local transfer contract and path conservation equations.
The model separates individual residual-pair updates from completed path augmentation.
An intermediate path update need not yet satisfy flow conservation at every vertex.
Only the completed path supplies that guarantee.

Let $`f`$ and $`r`$ be forward and reverse residual capacities for one edge pair.
Let $`a`$ be the transferred amount and $`L`$ the machine-value limit.
The checked transfer requires $`a\leq f`$ and $`r+a\leq L`$, then returns:

```math
(f',r')=(f-a,r+a),\qquad f'+r'=f+r.
```

For a valid pair with $`f+r\leq L`$, the transfer fails exactly when it would overdraw the selected direction.
That precondition also ensures the opposite residual cannot overflow.
The history theorem preserves the pair total across arbitrary finite sequences of checked forward and reverse operations.
A reverse transfer of the same amount restores the original pair after a valid forward transfer.
The theorem includes zero transfers, while the native search separately requires positive augmentation for progress.

The native `checked_residual_transfer` evaluates both checked arithmetic operations before the caller updates either residual capacity.
The helper returns no pair on underflow or overflow.
The solver maps such failure to `InvalidResult`, not financial infeasibility.
This extraction follows the proved arithmetic contract and does not change valid flow results.
No demonstrated live settlement bug motivated this extraction.

For path conservation, define $`I_x(v)`$ as one when vertex $`v`$ equals vertex $`x`$, and zero otherwise.
A directed path step from $`u`$ to $`v`$ changes net outgoing flow at $`x`$ by $`a(I_x(u)-I_x(v))`$.
For a path from $`s`$ to $`t`$, these changes telescope to:

```math
\Delta_x=a\bigl(I_x(s)-I_x(t)\bigr).
```

Every internal vertex therefore has zero net change after the complete path.
For distinct endpoints, source net outgoing flow increases by $`a`$, and sink net outgoing flow decreases by $`a`$.
These are graph-flow quantities, not wallet balance credits or token minting.
The identity applies to arbitrary finite paths, including repeated vertices.
Separate path and residual-capacity conditions determine whether a path is a valid augmentation.

Native parent-path assertions now check these signed endpoint changes for every tested augmentation.
The assertions also retain their existing simple-path, positive-capacity, and exact-bottleneck checks.
An exhaustive arithmetic test covers 9,537 small forward, reverse, and amount combinations.
Two property tests each run 512 cases over full-width integer inputs.
They compare checked arithmetic with an independent wide-integer oracle and verify repeated transfer histories, reversal, conservation, and bounds.

The budget-prefix regression uses one feasible and one infeasible restricted graph.
For each charged dimension, it tests every limit from zero through the complete search's measured usage.
Every insufficient limit must return a host-work error, never a partial assignment or financial rejection.
The exact sufficient limit must return the original result.
Every interrupted attempt is followed by a fresh-budget attempt that must reproduce that result.
This checks local search abandonment, including interruption between residual-pair updates along a path.
It does not establish rollback of native purse custody or checkpoint publication, which this kernel does not perform.

The remaining general search proof must connect graph topology, parent discovery, paired updates, and complete path boundaries to the partial-assignment invariant.
It must also prove residual closure when breadth-first search exhausts its queue, with explicit termination and work bounds.
The component theorems and native transition tests do not yet constitute that complete refinement.

On September 10, 2026, Rocq compilation and recursive kernel checking passed under a 2 GiB systemd memory cap with swap disabled.
All nine printed assumption reports were closed under the global context.
The module contains 161 lines and 11 proof terms.
Its SHA-256 digest is `e813a8eacc78a94d5f35b74635934c4bf3cf56a5cbaa35777a110ed72379ecb4`.
All 81 monetary-allocation tests passed under a 6 GiB systemd memory cap, with two build jobs and swap disabled.
Strict library/test Clippy checks also passed.
The native feasibility source digest at that checkpoint was `d1bb05458b5027ff9449b27eb132e5cf5ea9d64a385a1d872e64fed914f38a04`.

### Residual reachability and search completion

[`FundingResidualReachability.v`](../../../../formal/rocq/cost_accounted_rho/theories/FundingResidualReachability.v) proves exploration invariants for arbitrary finite residual graphs.
Its inductive reachability relation starts at the source and follows positive residual edges.
The model does not assume that the discovered set already equals the reachable set.

The state records discovered and processed vertices separately.
A processing step selects a discovered vertex, discovers its residual neighbors, and marks that vertex processed.
The invariant requires every processed vertex to be discovered and every discovered vertex to have a path from the source.
It also requires every residual neighbor of a processed vertex to be discovered.
Initialization and processing preserve this invariant.

The operation-history checker rejects an out-of-range, undiscovered, or previously processed vertex.
Each accepted processing step increases the processed count by exactly one.
Thus, an accepted history processes at most the graph's vertex count.
When every discovered vertex is processed, the invariant proves exact reachability in both directions.
No reachable vertex can remain omitted, and no unreachable vertex can appear as discovered.

The module also provides `residual_complete_reference`, an executable complete reference search.
It selects a pending vertex from a finite scan and processes that vertex.
The sufficient-fuel theorem proves that one processing allowance per remaining unprocessed vertex guarantees completion.
The final theorem proves exact reachability from the initial source state.
This reference establishes termination and completeness rather than assuming that an arbitrary supplied exploration order eventually exhausts its frontier.

The reference selection order differs from the native first-in, first-out queue.
The reachability invariant is deliberately independent of that order.
It proves discovery and closure properties, not shortest-path distances or the native queue's complete refinement.
The native implementation also returns early when it discovers the sink.
The complete-reachability conclusion applies to exhausted searches, not those early successful returns.

[`funding_search_properties.rs`](../../../../rholang/src/rust/interpreter/accounting/monetary_allocation/funding_search_properties.rs) checks native queue states before exploration and after each completed vertex scan.
The checks establish unique queued vertices, exact correspondence between queue membership and discovered parents, and a processed prefix bounded by the vertex count.
Each non-source parent must reference a positive residual edge from an earlier queued vertex.
Every residual neighbor of a processed vertex must be discovered.
On unsuccessful return, the native cursor must equal the queue length.

Additional checks require nondecreasing queue distances and valid parent distances.
For exhausted searches, an independent repeated-distance-relaxation oracle must match the full native distance vector, including unreachable vertices.
Two negative controls verify that duplicate discovery and incomplete neighbor scanning trigger their corresponding assertions.
These checks run within the existing exhaustive, generated, and budget-prefix solver tests.

The formal processing step abstracts one completed adjacency scan over immutable residual edges.
Native tests inspect the corresponding completed-scan boundary, while host-work interruption exits without returning a financial result.
This local abstraction does not serialize validators, funding scopes, or shared custody publication.
The remaining composition proof must connect actual adjacency scans, parent paths, and augmentation to these component invariants.
General shortest-path complexity, host-work accounting, minimax optimization, and settlement integration remain separate required obligations.

On September 10, 2026, Rocq compilation and recursive kernel checking passed under a 2 GiB systemd memory cap with swap disabled.
All ten printed assumption reports were closed under the global context.
The module contains 251 lines and 15 proof terms.
Its SHA-256 digest is `b58ade3d44b13de14568a6475e6cb90bcfebf8fa1482a946b9df57026a9aba90`.
All 83 monetary-allocation tests and strict library/test Clippy checks passed under a 6 GiB systemd memory cap with swap disabled.
The native feasibility digest at that checkpoint was `671c7d5466c2c377484977f83ed88eeb1d5b01db07c869bdd63d6746a9a0bc35`.
The native search-check digest at that checkpoint was `a885891c8d08c7869a65b99d4a711454724fa0d4f6d89d8036e0a32024b5741f`.

### Funding graph and deficit composition

[`FundingResidualGraph.v`](../../../../formal/rocq/cost_accounted_rho/theories/FundingResidualGraph.v) connects the reference search to the funding graph.
The model defines source, payer, obligation, and sink vertices and proves their numeric index decoding.
The source occupies index zero, followed by payers, obligations, and the sink.
Only indices within the declared vertex count participate in reachability.

Let $`c_j`$ denote the assigned column total for obligation $`j`$.
The other symbols retain their definitions above.
The graph contains exactly these positive residual edge conditions:

| Edge direction | Condition |
|---|---|
| Source to payer $`i`$ | $`r_i<\min(C_i,M)`$ |
| Payer $`i`$ to source | $`r_i>0`$ |
| Payer $`i`$ to obligation $`j`$ | The edge is eligible and $`f_{ij}<M`$. |
| Obligation $`j`$ to payer $`i`$ | $`f_{ij}>0`$ |
| Obligation $`j`$ to sink | $`c_j<q_j`$ |
| Sink to obligation $`j`$ | $`c_j>0`$ |

All other vertex-type combinations have no residual edge.
The model includes reverse source and sink edges, even though an unsuccessful source search cannot reach the sink.

For a valid partial assignment with unmet demand, exact graph reachability determines the cut premises.
An unreachable payer cannot have positive source-edge residual capacity.
A reachable payer must reach each eligible obligation with positive forward residual capacity.
A reachable obligation must reach each payer with positive reverse capacity.
If the sink is unreachable, each reachable obligation must be fully funded.
These statements now follow from the encoded graph, rather than remaining independent assumptions of the deficit theorem.

`funding_graph_search_returns_path_or_deficit` composes graph decoding, terminating reference exploration, residual-cut reasoning, and certificate soundness.
The reference result either establishes sink reachability or supplies selected obligations with a checked deficit and excludes every valid assignment.
The positive branch proves reachability, not an extracted parent path or a completed augmentation.
The theorem starts from a valid partial assignment, whose preservation across native augmentations remains a separate composition obligation.

Native transition tests now reconstruct all positive edges from the assignment matrix, capacities, demands, and eligibility.
They compare that relation with the actual residual edge buffers after initialization and every tested augmentation.
The check uses sparse edge sets, not a quadratic all-vertex-pair scan.
An additional negative control inserts an edge that bypasses its payer and requires the relation check to fail.
This extends the existing property and exhaustive tests without changing production settlement behavior.

On September 10, 2026, Rocq compilation and recursive kernel checking passed under a 2 GiB systemd memory cap with swap disabled.
All five printed assumption reports were closed under the global context.
The module contains 161 lines and seven proof terms.
Its SHA-256 digest is `4edd3b873bd14d900287d910001faba51ac642f8afb6b0446b7e5594033be873`.
All 84 monetary-allocation tests passed under a 6 GiB systemd memory cap with swap disabled.
Strict library/test Clippy checks also passed.
The native feasibility digest at that checkpoint was `abfc6b98e1ad49895d30f5bf27b4c9ed2fa5476a9b7ebfe8f67cb624f25d787d`.
The native search-check digest is `92d2ed143dbbf05ed3770d7401226a5b78b4f01b72dc83b3d004c6fd1198b5d2`.

### Ranked parent paths and bottlenecks

[`FundingParentPath.v`](../../../../formal/rocq/cost_accounted_rho/theories/FundingParentPath.v) proves parent-path extraction from strictly decreasing discovery ranks.
Every known non-root vertex must have a known parent of lower rank, connected by a positive residual edge.
The native queue checks enforce these conditions through queue positions and stored parent-edge indices.
The root does not require a parent edge.

The extraction theorem produces a reverse path from the selected vertex to the root.
It proves valid links, known vertices, no repeated vertex, the correct endpoints, and a length bounded by the starting rank plus one.
The native traversal follows the same sink-to-source direction.
This theorem does not independently prove that native queue construction establishes the rank premises.
That correspondence remains part of the full implementation refinement.

Let $`R`$ be remaining unfunded demand, and let $`c_1,\ldots,c_k`$ be residual capacities along the extracted path.
The bottleneck is:

```math
a=\min(R,c_1,\ldots,c_k).
```

The model proves that this amount respects every bound and is the greatest amount that does so.
If remaining demand and all path capacities are positive, the bottleneck is positive.
The bottleneck either equals remaining demand or equals a path-edge capacity.
Thus, a completed augmentation either funds all remaining demand or saturates a residual edge.
This progress property does not alone prove the shortest-path algorithm's monetary-magnitude-independent complexity bound.

Native augmentation assertions now require that completion-or-saturation condition.
A new 512-case property generates ranked parent forests and full chains, with full-width capacities and remaining demand.
It checks strict rank decrease, bounded path length, valid simple paths, exact bottlenecks, and endpoint conservation.
A negative control introduces a parent cycle disconnected from the root and requires the existing simplicity check to reject it.
These are test extensions, not changes to production allocation policy.

On September 10, 2026, Rocq compilation and recursive kernel checking passed under a 2 GiB systemd memory cap with swap disabled.
All six printed assumption reports were closed under the global context.
The module contains 137 lines and seven proof terms.
Its SHA-256 digest is `e6201e55f16e1a29621f65b0f6dee669f210e63b56a197eacd6be69d641d1776`.
All 86 monetary-allocation tests passed under a 6 GiB systemd memory cap with swap disabled.
Strict library/test Clippy checks also passed.
The native feasibility digest is `a4726637f0cb36d0929358df0a6fa31fe6b9db6d88109b9f552ce30474036da0`.

## Direct native arithmetic verification

The native solver and Verus now read the same [arithmetic source](../../../../rholang/src/rust/interpreter/accounting/monetary_allocation/funding_arithmetic.rs).
The [verification entry](../../../../formal/verus/cost_accounting/funding_arithmetic.rs) imports that file directly.
It does not maintain a copied implementation or assume the helper's body is correct.
The extraction preserves the existing checked subtraction and checked addition.

Let $`f`$ and $`r`$ denote the forward and reverse residual capacities.
Let $`a`$ denote the transfer amount and $`U=2^{64}-1`$ the largest unsigned 64-bit value.
The native contract proves the exact result for every machine input:

```math
\operatorname{transfer}(f,r,a)=
\begin{cases}
\operatorname{Some}(f-a,r+a), & a\leq f\ \land\ r+a\leq U,\\
\operatorname{None}, & a>f\ \lor\ r+a>U.
\end{cases}
```

Every successful transfer preserves $`f+r`$ as a mathematical integer sum.
The contract covers zero amounts, full-width inputs, insufficient forward capacity, and reverse-capacity overflow.
It does not require the sum of both capacities to fit in one machine integer.
This contract corresponds to the checked-pair relation in the [Rocq transfer model](#residual-transfers-and-path-conservation).
The two provers check their own representations. No automatic translation between those representations has been proved.

The `verus_keep_ghost` configuration enables verification imports and attributes only.
Normal Cargo builds omit them and retain the same executable arithmetic.
The configuration adds no runtime dependency, accounting activation mode, or monetary policy branch.

The [native Verus gate](../../../../scripts/check-cost-accounted-rho-native-funding-verus.sh) requires a successful verifier exit and structured proof results.
It requires exactly one verified function, zero errors, the expected function identity, and verification of the entire entry crate.
Missing tools, timeouts, malformed reports, and translation errors fail the gate.
The umbrella verification script discovers this gate through its existing filename pattern.

Three negative controls change only temporary copies of the native body:

| Incorrect body | Required result |
|---|---|
| Leave the forward capacity unchanged. | The exact debit postcondition fails. |
| Leave the reverse capacity unchanged. | The exact credit postcondition fails. |
| Return failure for every input. | The exact rejection postcondition fails. |

Each negative control must return a proof-failure status, one failed verification, and a postcondition diagnostic.
A compilation failure cannot satisfy a negative control.
The gate checks source digests before completion and removes its temporary source copies.
It retains compact reports under `target/verification/cost-accounted-rho/native-funding-verus/` and does not use `/tmp` for those copies.

Run the gate with an explicit memory limit:

```bash
systemd-run --user --scope -p MemoryMax=2G -p MemorySwapMax=0 -p CPUQuota=100% \
  bash scripts/check-cost-accounted-rho-native-funding-verus.sh
```

The proof trusts Verus, its compiler translation, its SMT solver, and the imported verification standard library, `vstd`.
An SMT solver decides the generated logical constraints.
The installed `vstd` provides assumed specifications for Rust's checked integer operations and relevant `Option` operations.
The `--no-cheating` option excludes local assumed implementation bodies but does not remove that imported trust boundary.
This result is not a Rocq kernel proof of the compiled executable.

On September 10, 2026, Verus `0.2026.07.12.0b42f4c` verified the shared body and rejected all three incorrect bodies.
All checks ran under a 2 GiB systemd memory cap with swap disabled.
All 86 monetary-allocation tests and strict library/test Clippy checks passed under a separate 6 GiB cap.
The native arithmetic SHA-256 digest is `f4058e1b582c499ab77a8a29a83855c3540297128f453a3abc8521632ec3b924`.
The updated feasibility digest is `6e215cf838e6ceaf4f8fcd8aac355b8f1bb3ea0261a04a0e421167523dbbbab9`.

This proof does not establish complete graph refinement, minimax optimality, host-work bounds, or concurrent purse settlement.
Those obligations remain separate from the verified arithmetic body.
The full campaign gate was not rerun for this focused change.

## Network augmentation and native transition checks

[`FundingNetworkAugmentation.v`](../../../../formal/rocq/cost_accounted_rho/theories/FundingNetworkAugmentation.v) connects checked residual transfers to finite network updates.
A network maps each edge-pair identity to its forward and reverse capacities.
An operation selects a pair and a direction.
The executable model updates that pair through `directed_residual_transfer` and leaves every other pair unchanged.
An augmentation applies a finite list of these operations with one transfer amount.

The model first proves properties for arbitrary successful histories, including repeated pairs and both transfer directions.
Every pair retains its original capacity sum.
If each original sum fits within the machine limit, both residual capacities remain within that limit after every successful prefix.
This assertion does not require intermediate updates to conserve flow at internal vertices.

Define divergence as outgoing flow minus incoming flow at a vertex.
Each original edge carries the flow represented by its reverse capacity.
For vertex $`v`$, transfer amount $`a`$, and directed endpoints $`u,w`$, one update changes divergence by:

```math
\Delta\operatorname{div}(v)
=a\left(\mathbf{1}_{v=u}-\mathbf{1}_{v=w}\right).
```

Here $`\mathbf{1}`$ is one when its condition is true and zero otherwise.
The proof derives this equation from the checked pair update and a finite sum over all edges.
It does not assume a corresponding change in a separate flow ledger.
Forward and reverse transfers satisfy the same equation with their respective directed endpoints.

For linked path operations, the internal terms cancel.
Reversing the operation list preserves that sum, as required by the native sink-to-source update loop.
This reversal changes the update order, not the direction of each transfer.
After the complete path, source divergence increases by $`a`$, sink divergence decreases by $`a`$, and every internal divergence stays unchanged.

`complete_reverse_path_augmentation` also proves that the modeled update succeeds under these premises:

- Each path operation selects an existing pair.
- No pair occurs twice in the path.
- Every original pair sum fits within the machine limit.
- The transfer amount fits every selected residual capacity before the update.
- The directed operations form a path from the specified source to the specified sink.

Pair uniqueness means that an earlier operation cannot consume a later operation's available residual capacity.
The theorem concludes update existence, preserved capacity sums, machine bounds, and the complete divergence equation.
It does not replace these premises with an assumed successful augmentation.

The native test build captures each augmenting path before mutation.
It calculates expected capacities with signed 128-bit arithmetic, independently of the native checked-transfer helper.
After the production loop completes, the check compares every residual capacity and every vertex divergence with that expectation.
The check also rejects repeated pair identities in the captured path.
Existing exhaustive and generated solver tests execute these assertions on their actual search paths, including reassignment paths with reverse edges.

A separate property test generates 512 network histories with up to 32 pairs and 128 operations.
It uses full-width unsigned capacities and transfer amounts, repeated pair selections, both directions, and unsuccessful transfers.
After each operation, it checks exact debits and credits, all pair sums, untouched pairs, and cumulative divergence.
The test requires an unsuccessful transfer to leave the network unchanged.
A negative control skips a path edge and must fail the completed-capacity assertion.

These changes add test instrumentation only. They do not change production allocation, custody, concurrency, or host-work charging.
The model describes private solver state, not a protocol that serializes wallets or validators.
The remaining refinement must connect native graph construction and parent extraction to all premises of the network theorem.
It must also derive the funding row and column invariants from the resulting graph flow.
The theorem does not establish minimax optimality or concurrent settlement publication.

On September 10, 2026, Rocq compilation and recursive kernel checking passed under a 2 GiB systemd memory cap with swap disabled.
All nine printed assumption reports were closed under the global context.
The module contains 300 lines and 18 proof terms.
Its SHA-256 digest is `1c7cfc7cdaf85e612845351df67d0d6e95e6bd2aea82b66d6799581acb666d10`.
All 88 monetary-allocation tests and strict library/test Clippy checks passed under a 6 GiB systemd memory cap with swap disabled.
The native feasibility SHA-256 digest is `4fbf93b634f31cd7fdd0e13d1f8f5895518502f657365b53139bd8fca3d6466a`.
The native search-property digest is `4e04f927dbbbe3b02a8ddb28bb32cca1b41b210cd710c19b1c36161b248b5b88`.
The full campaign gate was not rerun for this focused change.

## Funding projection from network flows

[`FundingNetworkProjection.v`](../../../../formal/rocq/cost_accounted_rho/theories/FundingNetworkProjection.v) connects the network augmentation model to the existing funding assignment checker.
Each original edge pair has one of three kinds: payer capacity, eligible assignment, or obligation capacity.
The kinds specify the same directed endpoints as the native graph constructor.
Each kind also supplies the payer or obligation indices needed to interpret its flow.

The projection sums reverse capacities of assignment edges for each payer-obligation pair.
It separately sums payer-capacity flows and obligation-capacity flows.
These definitions use actual edge capacities, not a separate presumed assignment ledger.
The proofs derive the following identities by induction over the finite edge collection:

```math
\begin{aligned}
\operatorname{div}(\operatorname{payer}_i)
  &=\operatorname{row}_i-\operatorname{incoming}_i,\\
\operatorname{div}(\operatorname{obligation}_j)
  &=\operatorname{outgoing}_j-\operatorname{column}_j.
\end{aligned}
```

Here a row is a payer's total assigned contribution, and a column is an obligation's total received contribution.
Incoming flow comes from the source through payer-capacity edges.
Outgoing flow reaches the sink through obligation-capacity edges.
Zero divergence therefore implies the required row and column equalities.

For each payer, the total capacity of its payer-capacity edges must not exceed its authorized capacity.
For each obligation, the total capacity of its obligation-capacity edges must not exceed its demand.
The model sums these capacities rather than assuming that duplicate edges create extra authorized funding.
Every assignment edge must identify an eligible payer-obligation pair with valid indices.
Together with zero internal divergence, these conditions imply `partial_funding_valid`:

- Every payer's assigned total stays within its capacity.
- Every obligation's received total stays within its demand.
- Every ineligible assignment is zero.

`reverse_path_preserves_funding_validity` composes this result with the complete reverse-path augmentation theorem.
It derives successful augmentation and preserved funding validity from the pre-update network conditions and the path premises.
Pair-sum conservation preserves the capacity bounds.
Path-divergence conservation preserves the payer and obligation flow equalities.
The proof does not assume that the updated assignment is valid.

`completed_funding_network_passes_checker` connects this invariant to `assignment_check`.
If total received flow equals total demand, every bounded obligation column must equal its individual demand.
Thus a fully funded network passes the existing exact assignment checker.
Equal totals alone are insufficient without the per-column bounds established by the network invariant.

Native test builds check the projection identities after graph initialization and every completed augmentation.
A separate 512-case property test uses arbitrary sparse graphs with one through eight payers and zero through eight obligations.
It covers full-width capacities, nonconserved intermediate flows, and reversed storage order of the edge pairs.
The algebraic identities must hold even when a generated graph does not yet satisfy funding conservation.
A negative control adds a direct source-to-sink edge and requires the projection to reject its unsupported kind.

The representation proof permits repeated edge kinds only within the aggregate capacity bounds.
The native constructor instead creates unique source, eligible-assignment, and obligation pairs.
The final native matrix extraction relies on this uniqueness because it stores each assignment entry once.
General refinement must still prove the native index mapping, uniqueness, initial capacity bounds, and parent-path premises.
The recorded native tests support those conditions but do not replace that proof.
The model also does not establish minimax optimality, authenticated custody, or concurrent settlement publication.

On September 10, 2026, Rocq compilation and recursive kernel checking passed under a 2 GiB systemd memory cap with swap disabled.
All six printed assumption reports were closed under the global context.
The module contains 304 lines and 12 proof terms.
Its SHA-256 digest is `410d2f861ac183c1ce73c41edbeb55a0f5ec9afc252d523b9f6cf79d920a51df`.
All 90 monetary-allocation tests and strict library/test Clippy checks passed under a 6 GiB systemd memory cap with swap disabled.
The native feasibility SHA-256 digest is `15bac0981564066b22199779d6d0ef55a6cb49835a025d929dd9ef100270501f`.
The native search-property digest is `30d8c18012bf00cc4eaea62b34c8178ee7e0c4f42eb4ce77c250baa88af29b74`.
The full campaign gate was not rerun for this focused change.

### Exact funding-counter growth

The extended projection model also connects source divergence to the total projected assignment.
`source_divergence_equals_total_assignment` derives that equality from the edge kinds and zero payer divergence.
The proof sums actual source-edge flows and relates them to assignment rows and columns.
It does not assume that a stored counter already equals the funded amount.

`reverse_path_increases_funding_exactly` combines that equality with the executable augmentation theorem.
Let $`F`$ denote total projected funding before the path and $`a`$ its augmentation amount.
The theorem proves:

```math
F_{\mathrm{next}}=F+a
\qquad\text{and}\qquad
F_{\mathrm{next}}\leq\sum_j q_j.
```

Here $`q_j`$ is the fixed demand of obligation $`j`$.
Reverse assignment edges can move previous contributions between obligations without falsely increasing the total.
Only the complete source-to-sink augmentation increases total funding by $`a`$.
If $`a>0`$, the projected total strictly increases.
This proves monetary progress but not the shortest-path algorithm's stronger complexity bound.

The native invariant compares `funded` with the summed assignment after initialization and every complete augmentation.
A negative control increases the counter without changing any edge and must fail this assertion.
A 512-case property test scales the reverse-reassignment example through the largest amounts permitted by its two equal obligations.
Each amount lies between one and `u64::MAX / 2`, so the combined demand remains representable.
Every run exercises the production search and its internal transition assertions.

Rocq compilation and recursive kernel checking passed for the extension under a 2 GiB systemd memory cap with swap disabled.
All eight printed assumption reports were closed under the global context.
The updated module contains 388 lines and 17 proof terms.
Its SHA-256 digest is `dadae8436c53a586fd4143163ee99b2b68f9d2bbf9f7a5b93f172c83aee1d534`.
The earlier digest records the preceding projection checkpoint.
All 92 monetary-allocation tests and strict library/test Clippy checks passed under a 6 GiB systemd memory cap with swap disabled.
The updated native feasibility digest is `92468ee25c87f9870924dcbbe365e2017762bbe6c72cb2b804ec540e075cdc75`.
The full campaign gate was not rerun for this focused change.

The remaining native proof must connect Rust's stored counter, graph representation, and loop initialization to this theorem across the full search.
This model does not treat finite generated histories as a proof of all executions.

## Direct native edge-construction verification

The solver and a [Verus entry](../../../../formal/verus/cost_accounting/funding_graph.rs) now share the same [edge-construction source](../../../../rholang/src/rust/interpreter/accounting/monetary_allocation/funding_graph.rs).
The extraction moves the existing `ResidualEdge` type and `add_edge` body without changing their runtime operations.
The type remains internal to the monetary-allocation module.

Let $`e`$ denote the edge-vector length before insertion.
The constructor requires both endpoints to fit within the adjacency-head slice and $`e\leq\operatorname{usize::MAX}-2`$.
The native contract proves these postconditions:

| State component | Result |
|---|---|
| Edge-vector length | Exactly two new entries. |
| Previous edges | Every field remains unchanged. |
| Forward entry at $`e`$ | Requested destination and capacity, with the previous source adjacency head as its link. |
| Reverse entry at $`e+1`$ | Original source as destination and zero initial capacity. |
| Reverse adjacency link | Previous destination head, or $`e`$ if both endpoints are equal. |
| Destination head | $`e+1`$. |
| Distinct source head | $`e`$. |
| Other heads | Unchanged, with the slice length preserved. |

The equal-endpoint case verifies update order when the two adjacency entries alias.
The funding graph does not need such edges, but the constructor contract does not assume distinct endpoints.
Neither a new global lock nor a runtime validation branch is introduced.

The native Verus gate checks this shared body separately from the arithmetic helper.
Each entry must report exactly one verified function, its expected identity, full entry-crate verification, and no errors.
Three additional temporary mutations must fail their postconditions:

- Use the wrong destination for the reverse entry.
- Initialize reverse capacity to one instead of zero.
- Omit the destination-head update.

The gate now requires both native proofs and all six incorrect-body controls.
It retains machine-readable reports and source digests, then removes temporary source copies.
The earlier arithmetic-only gate result remains a historical checkpoint.

A 512-case native property test checks finite insertion histories with one through 64 vertices and up to 128 insertions.
The histories include repeated endpoints, equal endpoints, and full-width capacities.
Each insertion checks all constructor postconditions and traverses every adjacency list.
Traversal must visit each directed edge exactly once, with the reverse entry identifying its source vertex.
This test checks the growing native graph rather than a copied constructor.

The proof depends on Verus's imported vector, slice, sequence, and mutable-reference specifications.
It does not prove the Rust allocator or survival under allocation failure.
The caller's fallible preallocation and host-work reservation require their own refinement.
The caller must also establish even edge counts for the XOR reverse-index convention and the unique funding-edge layout.
The constructor proof alone does not establish those whole-graph invariants or search correctness.

On September 10, 2026, Verus `0.2026.07.12.0b42f4c` verified both shared bodies and rejected all six incorrect bodies.
The checks ran under a 2 GiB systemd memory cap with swap disabled.
The native graph-source SHA-256 digest is `6cd8049ba5f732068fd90ca5ca97660c6b1e94c4fccda4685fb240bcf153bc71`.
The updated feasibility digest is `af271343ba0b5c3481ab65b7da171eb19f029d5395ed6b9e285ab43305ec7508`.
The search-property digest is `5d318347f07edf7941fda5548724141cf3fb08d969cb4e4c507590e9b63bab64`.
All 93 monetary-allocation tests and strict library/test Clippy checks passed under a 6 GiB systemd memory cap with swap disabled.
ShellCheck accepted the updated native verification gate.
The full campaign gate was not rerun for this focused change.

## Direct native pair-update verification

The solver now calls the shared [`augment_edge` body](../../../../rholang/src/rust/interpreter/accounting/monetary_allocation/funding_augmentation.rs) for each path edge.
Its [Verus entry](../../../../formal/verus/cost_accounting/funding_augmentation.rs) imports the same body, arithmetic helper, and edge type.
The extraction preserves the original checked arithmetic, two capacity writes, and predecessor selection.
The host-work reservation remains before this call in the native loop.

The contract requires the selected index and its XOR-paired index to fit within the edge slice.
A verification-only bit-vector proof establishes that these two indices differ.
This proof introduces no runtime instruction or new accounting mode.

On success, the contract proves all of the following:

- The selected capacity decreases by exactly the requested amount.
- The paired capacity increases by exactly that amount without overflow.
- The returned predecessor is the paired edge's original destination.
- Every destination and adjacency link remains unchanged.
- Every unrelated edge remains unchanged.
- The edge-slice length remains unchanged.

On rejection, the entire edge slice remains unchanged.
The rejection condition is exact: insufficient selected capacity or overflow of the paired capacity.
The body checks both arithmetic operations before writing either capacity.
This guarantee covers a single edge update, not rollback of earlier successful updates in an interrupted path.
The enclosing solver discards its private graph when host-work exhaustion aborts a path.
Native custody publication is a different boundary and requires separate verification.

The native gate verifies all three imported Rust bodies without local assumed implementations.
The combined entry reports five successful verification results, including internal proof obligations, rather than five Rust functions.
The gate requires this exact count and the expected function identity.
Its three new incorrect-body controls each retain four successful results and fail one postcondition:

| Incorrect body | Contract violation |
|---|---|
| Change capacity before checking arithmetic. | Incorrect transfer result or graph mutation on rejection. |
| Omit the paired capacity write. | Missing paired credit. |
| Return the selected destination. | Incorrect predecessor. |

Together with the earlier controls, the gate requires all nine incorrect bodies to fail their postconditions.
Compiler errors, missing reports, unexpected counts, timeouts, and missing tools remain gate failures.

The arbitrary network-history property now calls the actual `augment_edge` body instead of reproducing its writes.
A separate 512-case property checks arbitrary machine capacities, endpoint values, adjacency links, selected pairs, and transfer amounts.
Signed 128-bit calculations determine the independent expected result.
The property checks every field after success and rejection, including capacities whose pair sum already exceeds `u64::MAX`.
Such a pair can reject an overflowing transfer without changing any field.

On September 10, 2026, the native Verus gate and all nine controls passed under a 2 GiB systemd memory cap with swap disabled.
All 94 monetary-allocation tests and strict library/test Clippy checks passed under a separate 6 GiB cap.
ShellCheck accepted the updated gate.
The augmentation-source SHA-256 digest is `e6dc5d012e1ea816554c1e487d8369c7cee7ad36f96dc1ea1b5ac4da505b96eb`.
The feasibility digest is `da211c7f6e81be72664a5b7e2628ed140f7d5f8470ece62498157b801341f966`.
The search-property digest is `6e6edd0c0c49334e6c5215a62678eaff2a916da5ee647316cc036ba3bb35fb1d`.

The proof retains the documented Verus, imported-library, compiler-translation, and SMT trust boundary.
It does not establish the caller's index bounds, complete parent traversal, or the full solver's correctness.
The full campaign gate was not rerun for this focused change.

## Initial funding graph

[`FundingGraphInitialization.v`](../../../../formal/rocq/cost_accounted_rho/theories/FundingGraphInitialization.v) constructs the initial typed layout in the native constructor's order.
It creates payer-capacity entries first, eligible assignment rows next, and obligation-capacity entries last.
Filtering each assignment row preserves its ascending obligation order.
The model accepts arbitrary finite payer and obligation counts, rather than fixing a two-wallet instance.

`initial_layout_has_exact_membership` proves that the layout contains exactly the valid edge kinds.
`initial_layout_has_unique_pairs` proves that no kind occurs twice.
Together, these results exclude missing eligible entries, extra ineligible entries, and duplicate capacity or assignment entries in the modeled layout.
Distinct source indices cannot produce the same assignment kind, even when they share an eligible obligation.

The initial state assigns each pair its prescribed forward capacity and zero reverse capacity:

| Edge kind | Initial capacity |
|---|---|
| Payer capacity for payer $`i`$ | $`\min(C_i,M)`$. |
| Eligible assignment | $`M`$. |
| Obligation capacity for obligation $`j`$ | $`q_j`$. |

Here $`C_i`$ is payer capacity, $`q_j`$ is obligation demand, and $`M=\sum_j q_j`$.
Zero reverse capacity gives zero projected assignments and zero divergence at every vertex.
Uniqueness bounds each payer's aggregate capacity edges by $`C_i`$ and each obligation's aggregate capacity edges by $`q_j`$.
The proof derives these aggregate bounds instead of assuming them.
If $`M`$ fits within the machine limit, every initial pair sum also fits within that limit.

`initial_network_is_valid` combines membership, uniqueness-based capacity bounds, and zero flow into the network invariant used by the augmentation proofs.
This removes initial funding validity from the conditions that a caller must supply without proof.
It does not yet prove that the native flat edge vector and adjacency heads refine every element of this typed layout.
That correspondence must include paired indices, constructor calls, and the matrix extraction layout.

The native invariant already checks every original pair's ordered endpoints, total capacity, adjacency ownership, and projected flow after initialization.
It now also checks that no original endpoint pair repeats.
This check runs after every completed augmentation in the existing exhaustive and generated solver tests.
A negative control duplicates an assignment pair and must fail the uniqueness check.
The generic edge constructor can represent repeated endpoints, but the funding layout must not contain duplicate original pairs.

On September 10, 2026, Rocq compilation and recursive kernel checking passed under a 2 GiB systemd memory cap with swap disabled.
All eight printed assumption reports were closed under the global context.
The module contains 236 lines and 15 proof terms.
Its SHA-256 digest is `c59b38edf87b867a2ff60111eb409b865e12fcfc202280b15b81b4a080d8be64`.
All 95 monetary-allocation tests and strict library/test Clippy checks passed under a 6 GiB systemd memory cap with swap disabled.
The native feasibility digest is `aabc6b2dd2cd8757fa4f0d8179b00eef684e6e44d7e826a010d3a1b05bf80573`.
The search-property digest is `43b376e0a51ccceb0b193d22182797e88bd9a45ce5b597602c92718b41dd0541`.
The full campaign gate was not rerun for this focused change.

## Parent paths and residual-pair uniqueness

[`FundingPathEncoding.v`](../../../../formal/rocq/cost_accounted_rho/theories/FundingPathEncoding.v) connects parent paths to the operations used by network augmentation.
An operation identifies an original pair and a transfer direction.
Its direction determines which endpoint is the source and which endpoint is the destination.
The operation path records the starting vertex and every subsequent destination.

`simple_linked_path_has_distinct_pairs` proves that a linked path with no repeated vertices cannot repeat a pair identity.
This includes reuse of the same pair in the opposite direction.
Repeating that pair would repeat one of its endpoints, contradicting path simplicity.
The result derives the pair-distinctness premise required by the augmentation theorem.

`ranked_parents_extract_linked_simple_operations` uses an executable parent-operation extractor.
For each known non-root vertex, its parent operation must end at that vertex and begin at a known vertex of strictly lower rank.
With fuel at least the starting rank, extraction succeeds and returns:

- A linked operation path from the root to the starting vertex.
- A vertex list with no repetitions.
- Vertex ranks bounded by the starting rank.
- An operation count no greater than the starting rank.

The reference returns operations in root-to-node order.
The augmentation model reverses that list to match native sink-to-source mutation order.
The representation uses list append for its proof construction, not as a proposed replacement for native parent traversal.
`simple_funding_path_increases_funding` combines path simplicity with the funding-progress theorem.
It retains the required network validity, machine bounds, and per-edge capacity premises.

The native 512-case ranked-parent property now covers both pair orientations.
It generates forests or full chains with two through 129 vertices and full-width capacities.
The test obtains a bottleneck, executes every native `augment_edge` call back to the root, and checks the resulting graph.
The number of applied updates must equal the extracted path length.
The final snapshot checks every residual capacity and vertex divergence, including untouched branches of the forest.

These results do not establish the native breadth-first loop's parent invariant by themselves.
That loop must prove that recorded parent indices, destinations, ranks, and available capacities meet the extraction premises.
The generated tests check that correspondence in bounded cases but do not constitute its general proof.

On September 10, 2026, Rocq compilation and recursive kernel checking passed under a 2 GiB systemd memory cap with swap disabled.
All six printed assumption reports were closed under the global context.
The module contains 141 lines and seven proof terms.
Its SHA-256 digest is `f0eeaea24ee7092a62cb0deee7b8445516e5f0aab4cef8e02ac950e8bd400bfb`.
All 95 monetary-allocation tests and strict library/test Clippy checks passed under a 6 GiB systemd memory cap with swap disabled.
The updated native feasibility digest is `1cc825c287b7deb014bd3717dc6e11d5dc07f16158cceb594dd799897f347c23`.
The full campaign gate was not rerun for this focused change.

## Direct native discovery verification

The native search now calls the shared [`discover_parent`](../../../../rholang/src/rust/interpreter/accounting/monetary_allocation/funding_discovery.rs) implementation.
This extraction preserves the existing search operation.
A positive-capacity edge discovers its target only when that target has no recorded parent.
Discovery records the edge index and appends the target to the queue.
Other calls preserve both structures unchanged.

The Verus entry imports that production body, not a separate executable approximation.
Its preconditions require an in-bounds target, a non-sentinel edge index, queue length below the machine maximum, and unique queue entries.
For each in-bounds vertex, queue membership must agree with parent presence.
The postconditions preserve uniqueness and that agreement.
Successful discovery changes only the target's parent and appends exactly one target.
It preserves the complete previous queue prefix.

The strict native gate verified the discovery entry with two verification results and no errors.
The second result belongs to the imported graph constructor.
Three incorrect discovery bodies failed their postconditions:

- Overwrite an existing parent.
- Enqueue a target through a zero-capacity edge.
- Record a parent without appending the target.

Together, the arithmetic, constructor, augmentation, and discovery entries rejected all twelve incorrect-body controls.
The gate also checks source hashes, target function identities, complete entry verification, and expected result counts.
ShellCheck passed for the gate script.
The 96-test monetary suite and strict library/test Clippy checks passed at this checkpoint.

The native source digest is `c3f7bc32df5001f758cdaa454cd490c931a0bb3a24bdf36a3ce969d7ec9bb210`.
The Verus entry digest is `525562d9f1533aaea3bc5b4e2e8ab1c62e2a7c7f22994da0ee1e9e0698080444`.
Verus relies on its documented verifier and imported standard-library specifications, including sequence membership after append.
This contract does not establish caller index validity, allocation success, shortest-path distances, or the complete search loop.

## Discovery histories and parent-path termination

[`FundingDiscovery.v`](../../../../formal/rocq/cost_accounted_rho/theories/FundingDiscovery.v) derives the ranked-parent invariant from discovery transitions.
Previously, parent extraction required that invariant as a premise.
The new model starts with the root as the only known vertex.
Each enabled transition follows an edge from a known vertex to an unknown vertex.
Disabled transitions leave the state unchanged.

The model assigns a new vertex the previous queue length as its discovery rank.
Its parent already occurs in the queue, so the parent has a strictly smaller rank.
The proof preserves each known vertex's exact queue position, not only an abstract decreasing number.
It also preserves queue uniqueness, root membership, known-vertex membership, and positive parent-edge validity.
Earlier discoveries retain their parents and ranks.

`arbitrary_discovery_histories_preserve_validity` proves these invariants for every finite discovery history.
The theorem imposes no fixed vertex count or FIFO discovery order.
Its edge relation remains fixed during one search, matching the private native residual graph between augmentations.
Repeated targets, self-edges, and returns to the root cannot overwrite a recorded parent.
`discovered_nodes_have_simple_root_paths` composes the invariant with the executable parent extractor.
Every known vertex has a simple path to the root, with extraction fuel equal to the queue length.
This result establishes parent-path termination, not shortest-path complexity or complete max-flow termination.

The native property `arbitrary_discovery_order_keeps_parent_paths_acyclic` exercises the actual constructor and discovery helper.
It generates 512 histories with one through 64 vertices, arbitrary roots, full-width capacities, and up to 128 operations.
Each operation selects a known source and an arbitrary target.
Checks cover every history prefix and every discovered vertex, including exact parent endpoints, positive capacities, rank decrease, and bounded root traversal.
A separate generated property checks unchanged state, queue prefixes, membership, and preservation of existing parent choices.

Rocq compilation and recursive kernel checking passed under a 2 GiB memory cap with swap disabled.
All four printed assumption reports were closed under the global context.
The module contains 136 lines and five proof terms.
Its digest is `ca3c9321ec6e5c633aaa19ffd8bcb11d2accc52ca8a9e504137de46489fd8567`.
All 97 monetary-allocation tests and strict library/test Clippy checks passed under a 6 GiB memory cap with swap disabled.
The focused check did not rerun the complete campaign gate or a network soak.
The production loop still needs a complete refinement connecting adjacency traversal and recorded edge indices to this transition model.
The model does not change funding policy, settlement, or Casper behavior.

## Complete neighbor scans and funding outcomes

[`FundingSearchOutcome.v`](../../../../formal/rocq/cost_accounted_rho/theories/FundingSearchOutcome.v) connects parent discovery to residual exploration and the funding deficit checker.
Its executable scan applies individual discovery steps over a list of neighbor targets.
The source must already be known.
The scan discovers exactly the previously known vertices and the listed targets with a positive edge from that source.
It also preserves the parent-path invariant proved in `FundingDiscovery.v`.

The scan theorem supports sparse adjacency lists, not only a complete vertex enumeration.
The sparse coverage premise requires every in-bounds positive neighbor to occur in the scanned list.
It excludes scanned out-of-bounds positive neighbors.
Zero-capacity neighbors and repeated list entries do not change the discovered set.
With that coverage, the executable scan matches the existing residual-exploration transition pointwise.
The proof does not use an additional functional-extensionality axiom to equate the known-vertex functions.

Changing scan order preserves discovered membership when the lists have equal membership.
It need not preserve the parent chosen for a vertex or the resulting augmentation path.
This distinction permits implementation-specific adjacency order without confusing feasibility with the separately specified deterministic allocation policy.

`discovery_search_returns_simple_path_or_checked_deficit` establishes the outcome contract for an incomplete valid funding flow:

- If the sink is known, the executable parent extractor returns a simple positive-edge path from the sink to the root.
- Otherwise, an exhausted search produces a checked deficit that excludes every valid assignment for the original funding problem.

The positive outcome permits an early search exit after sink discovery.
The negative outcome requires every known in-bounds vertex to have been processed.
Absence from an unfinished search is not an infeasibility certificate.
The theorem composes the parent invariant, exploration invariant, residual-cut theorem, and independent deficit checker.
It does not assume that an arbitrary missing path proves insolvency.

The new native property checks exact neighbor-scan membership under original, reversed, and generated-priority orders.
Its 512 cases include one through 64 vertices, mixed initial discoveries, repeated targets, self-edges, and up to 128 edge records.
Each case checks preserved parents, unchanged queue prefixes, unique entries, positive recorded edges, and their source and target identities.
The existing native search checks enforce processed-prefix closure and compare exhausted searches with independent distance relaxation.
The existing funding checks validate each augmentation path and each returned deficit against original payer capacities.

Rocq compilation and recursive kernel checking passed under a 2 GiB memory cap with swap disabled.
All eight printed assumption reports were closed under the global context.
The module contains 180 lines and eleven proof terms.
Its digest is `6e602e958632cca58bc40977b77cc5ad6ec88ae6bfda8bfcce175a7bacb3ccb8`.
All 98 monetary-allocation tests and strict library/test Clippy checks passed under a 6 GiB memory cap with swap disabled.

This composition retains explicit native refinement obligations.
The flat adjacency traversal must establish sparse coverage, valid parent-edge indices, and exhaustion at the negative return.
The positive vertex path still needs its complete correspondence to native residual-pair operations and machine-bounded augmentation.
The result does not prove shortest-path complexity, minimax optimality, custody publication, or concurrent validator agreement.

## Native adjacency bounds and scan termination

The native graph constructor now has an additional Verus preservation contract, `adjacency_well_formed`.
The contract requires each non-sentinel head to reference an existing edge.
Each edge must have an in-bounds target and either a sentinel link or a link to a strictly earlier edge.
If the input graph satisfies these conditions, `add_edge` preserves them.
The existing exact-update contract remains applicable without this additional premise.
No executable constructor statement changed.

An empty graph with sentinel heads satisfies the invariant.
Inserting a forward edge links it to the previous source head, which precedes the new edge.
Inserting the reverse edge has the same property.
For a self-edge, the reverse link references the immediately preceding forward edge.
These cases explain why valid construction cannot create an adjacency cycle.

[`FundingAdjacency.v`](../../../../formal/rocq/cost_accounted_rho/theories/FundingAdjacency.v) derives scan termination from decreasing links.
The model represents the sentinel as zero and edge index $`i`$ as $`i+1`$.
This encoding keeps edge zero distinct from the sentinel.
Starting at index $`h`$, the extracted chain contains at most $`h+2`$ entries, including its terminal sentinel.
Thus, the chain traverses at most $`h+1`$ actual edges.
The proof also establishes unique chain entries and in-bounds references throughout the scan.

The native constructor-history property now checks the same head, target, and decreasing-link predicates after every insertion.
Its existing adjacency traversal also checks ownership, absence of repeated visits, and complete coverage of constructed edges.
The property includes repeated endpoints and self-edges.
The strict Verus gate gained a mutation that makes the forward edge point to itself.
That mutation failed its postcondition, as did the previous twelve incorrect-body controls.

Verus verified the shared constructor and the complete native gate passed.
Rocq compilation and recursive kernel checking passed, with all three assumption reports closed under the global context.
Both proof runs used a 2 GiB memory cap with swap disabled.
All 98 monetary-allocation tests and strict library/test Clippy checks passed under a 6 GiB memory cap with swap disabled.
ShellCheck passed for the native gate.

The updated native graph digest is `48c32d45f694c4ef6caef02009493056e89b3ea49290c04f494773b4e0b7948b`.
The Rocq module contains 60 lines and three proof terms.
Its digest is `2b6e139b4c1128473b86a30be9b99b734f9c950df64b7a8e121df2ba976d0e36`.

The decreasing-link contract proves a bound on a chain, not that a head reaches every edge owned by that vertex.
Complete native adjacency ownership and coverage remain separate refinement obligations.
The result also does not establish total solver complexity, allocator success, or concurrent custody safety.

## Exact adjacency ownership and coverage

[`FundingAdjacencyCoverage.v`](../../../../formal/rocq/cost_accounted_rho/theories/FundingAdjacencyCoverage.v) models the indexed adjacency constructor.
The state contains an edge count, an owner for each edge, a next-link function, and a head for each vertex.
Appending one edge stores the previous owner's head as its next link and makes the new index that owner's head.
The paired constructor applies this operation twice, first for the forward source and then for the reverse source.
The proof includes equal endpoints and arbitrary repeated insertions.

`adjacency_chain` describes an actual sequence of next-link traversals ending at the sentinel.
The coverage invariant requires a duplicate-free chain for every vertex.
Chain membership must equal the set of in-bounds edge indices assigned to that vertex.
Consequently, a valid chain neither omits an owned edge nor includes another vertex's edge.

`append_preserves_exact_adjacency_coverage` derives preservation from the constructor's indexed updates.
Previously constructed chains retain their links because the new index lies beyond every existing edge.
Only the selected owner's chain gains a new first element.
`constructed_adjacency_reaches_exactly_owned_edges` proves exact coverage after every finite construction history from the empty state.
The proof does not require a fixed number of wallets, vertices, or insertions.

The native property checker constructs expected ownership independently from each edge's paired reverse endpoint.
It compares that expected set with the indices reached from each actual head.
The generated constructor histories apply this check after every insertion.
Two negative regressions validate the checker:

- A head skips its newest edge but remains in bounds and follows decreasing links.
- A vertex uses another vertex's head.

The first case shows why termination alone cannot establish complete neighbor scanning.
The second case checks ownership separately from reachability and index validity.
Both regressions require the specific coverage or ownership failure, not an arbitrary panic.

Rocq compilation and recursive kernel checking passed under a 2 GiB memory cap with swap disabled.
All five printed assumption reports were closed under the global context.
The module contains 126 lines and six proof terms.
Its digest is `f1b26e6f8c8991841baf6db288d0925b52fa1e48ea96f0ece717d02014b977b5`.
All 100 monetary-allocation tests and strict library/test Clippy checks passed under a 6 GiB memory cap with swap disabled.
Both negative coverage regressions produced their required failure messages.

The model's explicit owner function is ghost information, not a new runtime allocation.
Native Rust obtains an edge's source from the opposite member of its residual pair.
The remaining correspondence proof must connect that representation and the verified Rust updates to this constructor model.
No production behavior changed in this coverage step.

## Native residual-pair ownership

The shared native constructor now proves the ownership equations used by the adjacency model.
For a native edge index, `residual_owner` reads the destination of the edge at `index ^ 1`.
This is the same expression that native parent traversal uses to obtain the preceding vertex.
The proof does not replace that expression with an assumed source field.

When the old edge-vector length is even, the constructor establishes these additional postconditions:

| Property | Result |
| --- | --- |
| Pair layout | Appending two edges preserves even length. |
| New forward edge | Its XOR partner has the requested forward source as its destination. |
| New reverse edge | Its XOR partner has the requested forward target as its destination. |
| Existing pairs | Every old XOR partner remains in bounds, and every old derived source remains unchanged. |

Bit-vector proof obligations establish the actual machine-index operations.
At an even insertion index, XOR with one selects the next index.
At the immediately following index, XOR selects the insertion index.
For every index below an even vector length, its XOR partner remains below that length.
The constructor's existing two-element capacity precondition supplies the required representable insertion indices.
These proof steps are excluded from ordinary Rust compilation.
They introduce no execution branch, memory allocation, or source field.

The native constructor-history property retains an independent owner list from insertion arguments.
After every insertion, it compares every actual XOR-derived source with that list.
A separate 512-case property checks partner bounds, orientation, pair identity, and involution across the full `usize` range.
That property operates on indices without allocating a machine-sized vector.

The strict Verus gate passed with seven constructor results, eleven augmentation-entry results, and eight discovery-entry results.
The extra results are proof obligations inside the imported constructor, not additional runtime functions.
The arithmetic entry still has one result.
All thirteen incorrect-body controls failed their required postconditions, and ShellCheck passed.
Verification used a 2 GiB memory cap with swap disabled.
The updated native graph digest is `0d04b8c5360879dfa66df25f815571f6ee3f002aa12f62831cd2b0dab3598e30`.
All 101 monetary-allocation tests and strict library/test Clippy checks passed under a 6 GiB memory cap with swap disabled.

This result establishes the native owner representation and its preservation by paired construction.
The remaining cross-model connection must compose these equations with the Rocq indexed-update model and its exact chain-coverage theorem.
It does not establish the complete search loop, minimax objective, or settlement integration.

## Native update contract and indexed coverage

[`FundingNativeAdjacency.v`](../../../../formal/rocq/cost_accounted_rho/theories/FundingNativeAdjacency.v) connects native constructor postconditions to the indexed coverage model.
Its `native_pair_update` record states the following obligations for each update.
The record does not assume coverage of the output graph.

| Contract field | Native postcondition and property check |
| --- | --- |
| Edge count | Append exactly two records. |
| Old owners | Preserve every old XOR-derived source. |
| Forward owner | The new forward record belongs to the requested source. |
| Reverse owner | The new reverse record belongs to the requested target. |
| Old links | Preserve the `next` field of every old record. |
| Forward link | Store the source's old head. |
| Reverse link | Store the target's old head, except a self-edge links to the new forward record. |
| Updated heads | Select the reverse record for the target and the forward record for a different source. Preserve other heads. |

`native_pair_contract_refines_indexed_constructor` proves equivalence with two indexed appends.
Equivalence covers the edge count, every in-bounds owner and next link, and every vertex head.
It does not require values at nonexistent edge indices to agree.
`equivalent_adjacency_preserves_coverage` transfers the exact chain-coverage theorem through this bounded equivalence.
The proof uses pointwise link equality along the actual chain, without functional extensionality.

The resulting preservation theorem applies to each native-contract update.
The history theorem applies to arbitrary finite sequences of such updates from an empty graph.
Together, they derive duplicate-free chains containing exactly each vertex's owned edges from update equations alone.
They do not replace the constructor with a different adjacency algorithm.

The cross-verifier interpretation maps Rust edge lengths and indices to natural numbers.
It maps `usize::MAX` links to `None` and valid indices to `Some` indices.
It maps each owner to the native XOR partner's destination.
Unused vertex heads extend to `None` outside the native head slice.
Valid source and target indices, even pair length, and representable appended indices come from the native constructor contract and its caller obligations.

This interpretation remains an explicit specification correspondence between Verus and Rocq, not a generated proof exchange between their kernels.
Verus verifies the shared Rust body against the native equations.
Rocq verifies that those equations imply the indexed model's coverage property.
Native generated histories check the equations and derived coverage together, including repeated endpoints and self-edges.
The existing omitted-edge and wrong-owner regressions check the derived coverage property independently.

Rocq compilation and recursive kernel checking passed under a 2 GiB memory cap with swap disabled.
All five printed assumption reports were closed under the global context.
The module contains 99 lines and five proof terms.
Its digest is `49598cec82f1dc85ce54a7476610791ffc3a8874245ea902c7daa58064b0ed47`.
The native graph source remains at digest `0d04b8c5360879dfa66df25f815571f6ee3f002aa12f62831cd2b0dab3598e30`.
No Rust source changed in this proof-only step, so the preceding 101-test, Clippy, and thirteen-mutation results remain the native evidence.

The complete solver still needs the constructor invariant propagated through native search and augmentation.
This contract connection does not establish minimax optimality, native allocation success, or custody publication.

## Verified native adjacency step

The search loop now uses the shared [`advance_adjacency`](../../../../rholang/src/rust/interpreter/accounting/monetary_allocation/funding_adjacency.rs) function.
Verus verified this body before its integration into the loop.
The function accepts an immutable edge slice and a private scan cursor.
At the sentinel, it returns `None` and preserves the sentinel.
Otherwise, it returns the current edge index and advances the cursor to that edge's recorded next link.

The preconditions require a valid initial cursor and sentinel-or-decreasing links throughout the slice.
These are the previously verified constructor invariants.
The postconditions prove that every returned index is in bounds.
They also prove that the next cursor is either the sentinel or a strictly smaller valid index.
The function cannot mutate graph topology or capacities because its edge slice is immutable.
Repeated calls after exhaustion continue to return `None` without changing the cursor.

The integration preserves edge order and the work charges for every successful search.
Host-work reservation remains before edge retrieval and cursor advancement.
The loop's non-sentinel guard supplies the step's nonempty-result premise.
If reservation fails, the solver returns an error and discards its private graph and cursor.
The failed operation cannot publish a partial funding assignment or mutate custody.
Existing per-budget-prefix tests cover these interruptions and clean retries.
The function adds no allocation or new persistent state.

The native adjacency-coverage checker now traverses through the same verified step.
Its expected edge sets still come independently from reverse-pair endpoints.
A new 512-case property compares complete native walks with a reference chain over one through 128 edge records.
It checks strict progress, valid returned indices, no repeated edges, the initial-index traversal bound, graph preservation, and repeated calls after exhaustion.
Generated chains include early sentinel links and initially exhausted cursors.

The strict Verus gate imports the actual production body and requires eight successful verification results for its entry.
Seven results belong to the imported constructor and its proof obligations.
Three new incorrect bodies failed their postconditions: omitted advancement, returned successor instead of current index, and a sentinel reported as an edge.
The complete gate now rejects sixteen incorrect-body controls.
ShellCheck passed for the updated gate, with verification limited to 2 GiB and swap disabled.

The native step digest is `279e9bc56dab04c89f644bdcdffdc50e12961efe3b153ec8f6ddbb399b77d9e9`.
The Verus entry digest is `c7764b976b7722f6f0e2a5ec65d3822e668071991c9fd058d2a16bdcda78e4ac`.
The integrated feasibility source digest is `abb20e993f7a881cc7969149e8c351af68c7c3ec63ec6752708f18c3ed12d73c`.
All 102 monetary-allocation tests and strict library/test Clippy checks passed under a 6 GiB memory cap with swap disabled.
This step closes the local cursor-operation obligation, not the complete outer search or monetary settlement proof.

## Native discovery queue bound

The native discovery contract now requires every queued vertex to be within the parent table.
It preserves that condition and proves that queue length never exceeds the parent-table length.
This bound is derived from unique, in-bounds vertices, not assumed as an output property.

`bounded_unique_queue` converts the queue into a finite set.
Uniqueness makes set cardinality equal queue length.
The vertex-bound premise makes this set a subset of the valid vertex range.
The imported finite-set lemmas then establish the queue-length bound.
`fresh_vertex_has_queue_space` applies that result to a queue extended by one absent, valid vertex.
It proves that the original queue has strictly fewer entries than the vertex count before the native push.

These are ghost proofs inside the shared Rust module.
The executable discovery body remains unchanged.
The native search already reserves queue capacity for every vertex before the search starts.
The length bound therefore establishes room for each discovery within that reserved capacity.
This result concerns the discovery queue, not every allocation in the solver or node.

The new 512-case property constructs reserved queues for one through 256 vertices.
It applies generated discovery orders, zero-capacity attempts, complete discovery, and repeated discoveries after saturation.
Every prefix must retain the original storage address and capacity.
It also checks valid vertices and the vertex-count length bound.
Existing discovery-history properties continue to check uniqueness, exact membership, and preserved parent choices.

The discovery entry passed Verus with ten verification results and no errors.
Seven results belong to the imported constructor, two to the queue-bound lemmas, and one to native discovery.
The duplicate-discovery mutation now fails earlier, at the freshness and uniqueness premises of those lemmas.
The gate requires both specific failed premises and the expected native body-check failure.
Other mutation controls retain their required postcondition failures.
This change does not permit arbitrary verifier errors to satisfy the mutation gate.

The shared discovery digest is `76d0b7bbe1dde6304367fa6b687b3d986280673e69b85e4db316b97e449494e2`.
The complete sixteen-mutation gate and ShellCheck passed under a 2 GiB memory cap with swap disabled.
All 103 monetary-allocation tests and strict library/test Clippy checks passed under a 6 GiB memory cap with swap disabled.
The queue proof retains the Verus and imported-library trust boundary documented above.
The complete outer-loop refinement still must connect the processed cursor, discovered queue, and search exit conditions.

## BFS queue scheduling and processed-prefix bounds

[`FundingFifoQueue.v`](../../../../formal/rocq/cost_accounted_rho/theories/FundingFifoQueue.v) models the native first-in, first-out (FIFO) queue and processed cursor.
This queue discipline implements breadth-first search (BFS).
A valid queue has unique, in-bounds vertices and a cursor no greater than its length.
The length bound follows from uniqueness and inclusion in the finite vertex range.
A scan selects the vertex at the current cursor, appends discoveries, and advances the cursor by exactly one.

The model derives this step contract from the executable discovery scan.
It proves that discovery histories extend the queue without changing existing positions.
For an in-bounds neighbor list, the scan preserves valid queue contents and the parent invariant.
The selected vertex is in bounds and does not occur in the previously processed prefix.
The next processed prefix is exactly the old prefix followed by that selected vertex.
New discoveries cannot enter the processed prefix prematurely.

`fifo_history_counts_processed_vertices` proves that cursor growth equals the number of completed scans.
The history bound then limits completed scans to the vertex count minus the initial cursor.
This result permits queue growth during search but does not permit repeated processing or skipped positions.
At exhaustion, the cursor equals queue length, so every queued vertex belongs to the processed prefix.
This is the scheduling condition needed by the search-outcome theorem's negative branch.

The native test build now captures the queue and cursor before every vertex scan.
After the scan, it checks exact cursor advancement, preservation of the complete old queue prefix, and the exact new processed prefix.
The existing search-state checks also verify unique vertices, valid parent paths, processed-neighbor closure, and independent distances after exhaustion.
Generated funding problems therefore exercise these invariants in the actual search loop.
Two negative regressions require specific failures for a skipped cursor and a rewritten queue prefix.
These snapshots exist only in test builds and add no production allocation.

Rocq compilation and recursive kernel checking passed under a 2 GiB memory cap with swap disabled.
All seven printed assumption reports were closed under the global context.
The module contains 154 lines and eleven proof terms.
Its digest is `e51bac1603d7ad00f00976c0577456f16cb39dc2252a6f445f73461576bce44c`.
All 105 monetary-allocation tests and strict library/test Clippy checks passed under a 6 GiB memory cap with swap disabled.
Both negative queue-step regressions produced their required failure messages.

The step relation requires each neighbor scan to complete.
The adjacency proof supplies a separate bound for that inner traversal.
The complete solver proof still needs their composition with positive-capacity reachability, augmentation, and the outer funding loop.
This module does not establish lexicographic-minimax optimality or concurrent custody correctness.

## Complete BFS and certified funding outcomes

[`FundingFifoSearch.v`](../../../../formal/rocq/cost_accounted_rho/theories/FundingFifoSearch.v) composes the queue, discovery, reachability, parent-path, and funding-deficit results.
The search algorithm is BFS, with a FIFO queue.
The reference starts with the source vertex alone in the queue.
Each scan processes the vertex at the cursor and appends newly discovered vertices.
The cursor advances once after that complete neighbor scan.

The combined invariant relates four parts of the search state:

- Every discovered vertex has a unique queue position and a valid ranked parent, except the root.
- Every queue vertex lies within the graph's vertex bound.
- The cursor does not exceed the queue length.
- The processed prefix satisfies positive-edge closure, and every discovered vertex is reachable from the root.

The neighbor interface permits repeated neighbors and zero-capacity edges.
It requires bounded targets and complete coverage of positive edges from each bounded source.
The sparse representation must establish this coverage before the search theorem applies.

The executable reference follows this sequence:

```text
search(state, cursor, fuel):
    if sink is known: return (state, cursor)
    if cursor equals queue length: return (state, cursor)
    if fuel is zero: return no result
    source = queue[cursor]
    next = discover all positive neighbors of source
    return search(next, cursor + 1, fuel - 1)
```

Here, fuel bounds vertex scans in the mathematical reference.
It does not mean phlogiston, wallet funding, or the native host-work budget.
With an initial fuel equal to the vertex count, the theorem proves that the reference returns a result.
It derives the exit condition instead of assuming that the queue eventually becomes empty.
The proof covers arbitrary finite vertex counts and all neighbor lists that satisfy the interface.

`initial_fifo_search_returns_path_or_closed_frontier` gives two possible results.
A discovered sink has an executable, simple, positive-edge parent path back to the source.
An undiscovered sink implies exhausted search and exact agreement between discovery and graph reachability.
Early success does not imply complete reachability for vertices that the search has not discovered.

`initial_funding_fifo_returns_path_or_checked_deficit` specializes that result to an incomplete valid funding flow.
It returns either the parent path or a selected-obligation deficit accepted by the independent funding checker.
The deficit proves that no valid assignment exists for the original capacities and obligations.
The theorem no longer requires the caller to supply an exhausted-search premise.

### Native tests and evidence boundaries

The native search assertions now compare discovered distances with an independent distance relaxation at early success as well as exhaustion.
At exhaustion, they compare all vertices.
At early success, they compare only discovered vertices.
This distinction prevents a false claim that early exit proves every remaining vertex unreachable.

The negative test `early_success_detects_a_non_shortest_parent_path` supplies a three-edge parent path where a two-edge path exists.
The new distance assertion rejects that state.
This test checks the checker and does not report a newly discovered production defect.

On September 10, 2026, all 106 monetary-allocation tests and warnings-as-errors Clippy checks passed.
Rocq compilation and recursive kernel checking passed for the composed search module.
All seven public assumption reports were closed under the global context.
The module contains 203 lines and nine completed proof terms.
Its SHA-256 digest is `b24f0386738180cc5c40ad96f8c178777b49aba068ffd7c4c258a2ef36d37643`.

The validation logs are `funding-fifo-search-rocq.log`, `funding-fifo-search-tests.log`, and `funding-fifo-search-clippy.log` under `target/verification/authority-valuation-20260910/`.
The proof check used a 2 GiB memory cap, one CPU, and no swap.
The native checks used a 6 GiB memory cap, two CPUs, and no swap.

The composed theorem concerns the executable Rocq reference.
The native caller still needs a complete refinement for its machine indices, host-work checks, augmentation loop, and failure paths.
Shortest-distance assertions are test evidence, not a formal shortest-path complexity proof.
Minimax optimization, authenticated custody, and concurrent settlement remain separate required obligations.

## Strict funding progress across augmentation histories

The extended [`FundingPathEncoding.v`](../../../../formal/rocq/cost_accounted_rho/theories/FundingPathEncoding.v) connects the bottleneck calculation to actual residual-network updates.
The bottleneck is the minimum of the remaining obligation and all path-edge capacities.
The theorem requires a simple, linked source-to-sink path whose residual capacities are positive.
It also requires a valid funding network and bounded residual pairs.

`positive_simple_path_makes_strict_funding_progress` derives the following conclusions:

- The calculated amount is positive.
- Reverse-order application of the complete path succeeds.
- The resulting network preserves funding validity and capacity bounds.
- Projected funding increases by exactly the calculated amount.
- Funding does not exceed the obligation, and the remaining obligation strictly decreases.

The history relation records the actual `network_augment` result for each path.
It does not assume that updates increase funding.
`bottleneck_histories_preserve_validity_and_bound_steps` derives progress across every finite history that satisfies the path conditions.

Let $`M`$ denote the total obligation, $`F_0`$ the initial funding, and $`F_k`$ the funding after $`k`$ successful augmentations.
The history theorem establishes:

```math
F_0 + k \leq F_k,
\qquad
k \leq M-F_0.
```

The count bound follows from positive integer progress.
It is not the stronger, capacity-independent complexity bound associated with shortest augmenting paths.
Large monetary values make this bound unsuitable as a production performance estimate.
Formal shortest-path complexity remains required.

The native test instrumentation checks exact progress after every successful augmentation in the existing generated and exhaustive funding tests.
It checks that the remaining obligation decreases and that the successful-update count stays within the derived bound.
An additional 512-case property test covers arbitrary `u64` totals, initial funding, and bounded positive update histories.
Three negative controls reject zero progress, an incorrect funding counter, and funding above the obligation.
These controls do not change production settlement or report new production defects.

On September 10, 2026, all 110 monetary-allocation tests and warnings-as-errors Clippy checks passed.
Rocq compilation and recursive kernel checking passed for the extended path module.
All eight public assumption reports were closed under the global context.
The module now contains 229 lines and nine completed proof terms.
Its SHA-256 digest is `47cb0fa04404cd5e7d4344d63a29aa50cc224c2b0fcc0c5ecd3523b8095748c9`.

The validation logs are `funding-progress-rocq.log`, `funding-progress-tests.log`, and `funding-progress-clippy.log` under `target/verification/authority-valuation-20260910/`.
The proof check used a 2 GiB memory cap and a 100% CPU quota.
The native checks used a 6 GiB memory cap and a 200% CPU quota.
Both scopes disabled swap.

The history model describes private allocation-search state, not concurrent publication of purse balances.
It does not serialize wallets, validators, or shards.
The remaining caller refinement must connect discovered native parent edges to the complete path premises, including host-work failures and discarded private state.

## Recorded parent edges and operation paths

[`FundingParentEdges.v`](../../../../formal/rocq/cost_accounted_rho/theories/FundingParentEdges.v) extends discovery state with the exact residual operation recorded for each new vertex.
An operation identifies a residual pair and its direction.
The graph endpoints determine the operation's source and target.
This representation distinguishes a forward draw from a reverse reassignment.

The reference records an operation only when its source is known, its capacity is positive, and its target is unknown.
It appends that target and records the same source in the vertex-parent state.
Previously recorded parents remain unchanged.
The initial state contains only the root, which needs no incoming parent operation.

The combined invariant requires each known non-root vertex to have:

- A bounded residual-pair index.
- An operation whose target is that vertex.
- An operation whose source equals its recorded parent vertex.
- Positive capacity in the unchanged search network.
- A parent with a lower discovery rank.

The step theorem derives this invariant from actual record updates.
The history theorem preserves it across arbitrary operation lists, including repeated targets and both operation directions.
Each supplied positive operation must correspond to an edge in the discovery graph.
This premise does not permit the model to invent graph edges.

`initial_parent_edge_histories_extract_valid_operations` starts from the root-only state.
For every discovered vertex, it derives an executable operation path from the root.
The path has distinct vertices and distinct residual pairs, bounded indices, positive capacities, and fewer operations than discovered vertices.
These conclusions supply the path premises needed by the augmentation proofs.

The native property test `arbitrary_discovery_order_keeps_parent_paths_acyclic` now mixes forward and reverse parent edges.
After each discovery, it checks parent preservation, endpoint identity, positive capacity, decreasing parent ranks, distinct path vertices, and distinct residual pairs.
The 512 generated histories include zero-capacity attempts, repeated targets, arbitrary roots, and self-edge attempts.
Zero-capacity and already-known targets cannot acquire a new parent.

On September 10, 2026, all 110 monetary-allocation tests and warnings-as-errors Clippy checks passed.
Rocq compilation and recursive kernel checking passed for the parent-edge module.
All six assumption reports were closed under the global context.
The module contains 160 lines and six completed proof terms.
Its SHA-256 digest is `b5b540bc3b8fd15704a3acd237a01c6d180d371843a592a1c1abd60b6321b127`.

The logs are `funding-parent-edges-rocq.log`, `funding-parent-edges-tests.log`, and `funding-parent-edges-clippy.log` under `target/verification/authority-valuation-20260910/`.
The proof check used a 2 GiB memory cap and a 100% CPU quota.
The native checks used a 6 GiB memory cap and a 200% CPU quota.
Both scopes disabled swap.

The source-known guard is explicit in the reference.
The native discovery helper instead relies on its caller to select a known queue vertex.
The complete caller refinement must connect that selection, native pair indices, and stable graph ownership to this operation model.
The module does not prove shortest-path complexity, allocation optimality, or concurrent purse publication.

## Fixed-flow contribution-domain classification

The [mandatory classification rule](authority-allocation-policy-decisions.md#mandatory-domain-classification) compares complete contribution domains, not visible eligibility matrices.
Missing edges can be irrelevant when capacities already force the same contributions.
Conversely, several restrictions can exclude a contribution vector even when each individual obligation passes a scalar capacity check.

This section concerns independent physical capacities and fixed integer obligations in one monetary unit.
It does not discard shared grants, correlated outcomes, resource types, or discrete acquisition choices.
Those constraints require their complete-domain reduction before this fragment applies.

Let $`C_i`$ be source $`i`$'s capacity, $`q_j`$ obligation $`j`$'s demand, and $`E_{ij}`$ its eligibility predicate.
Define total demand $`M=\sum_j q_j`$.
For a source subset $`S`$, define $`C(S)=\sum_{i\in S}C_i`$.
Define its neighborhood $`N(S)=\{j:\exists i\in S,\ E_{ij}\}`$ and neighborhood demand $`q(N(S))=\sum_{j\in N(S)}q_j`$.
The contribution cut condition is:

```math
\forall S,\quad \min\bigl(M,C(S)\bigr)\le q(N(S)).
```

For each positive-demand obligation $`j`$, define the excluded sources $`A_j=\{i:\neg E_{ij}\}`$.
Transpose the funding problem: original obligations become capacity sources, and original sources become demand obligations.
The transposed capacities are $`q_j`$.
The transposed demand at source position $`i`$ is $`C_i`$ if $`i\in A_j`$, and zero otherwise.
The transposed eligibility relation retains each original edge with reversed indices.

This construction tests whether every excluded source can use its full capacity through the other permitted obligations.
It tests combinations of restrictions through flow feasibility, not only the aggregate excluded capacity.
There is at most one transposed problem per positive-demand obligation.
It requires no enumeration of tokens, source subsets, or contribution vectors in the proposed production path.

### Formal contract

[`FundingDomainCuts.v`](../../../../formal/rocq/cost_accounted_rho/theories/FundingDomainCuts.v) proves the following statements for arbitrary finite counts and natural-number amounts:

- The contribution cut condition equals the cut conditions for all excluded-source subsets.
- These excluded-source cut conditions equal the complete transposed cut conditions.
- Valid assignments for every transposed problem establish the contribution cut condition.
- The contribution cut condition prevents every checked deficit for every vector in the capped simplex.
- Complete breadth-first search finds a residual path from every incomplete valid flow for each such vector.
- Zero total demand satisfies the contribution cut condition without a transposed problem.

The equivalence follows from a proper cut: its neighborhood omits at least one positive-demand obligation.
Every source in that cut therefore belongs to that obligation's excluded set.
Conversely, a deficit in a capped contribution vector contradicts the bound for sources outside the selected obligations' neighborhood.

[`FundingPairRealization.v`](../../../../formal/rocq/cost_accounted_rho/theories/FundingPairRealization.v) connects each partial matrix flow to an indexed residual network.
The representation preserves each matrix entry, original pair capacity, source and obligation balance, and the machine amount bound.
Every positive matrix edge has a corresponding positive indexed operation, including reverse edges used to repair earlier choices.

[`FundingFlowCompleteness.v`](../../../../formal/rocq/cost_accounted_rho/theories/FundingFlowCompleteness.v) composes that representation with complete search and bottleneck augmentation.
Every incomplete valid flow either has a strictly improving valid successor or yields a checked deficit.
Induction on remaining demand establishes a complete assignment or a deficit for every finite fixed-flow problem.
This induction is a mathematical termination argument, not token-by-token native execution.

The resulting equivalence is exact: a fixed-flow problem has an assignment if and only if every selected-obligation deficit check is false.
Together with the cut and witness proofs, it establishes complete capped-domain coverage exactly when every required transposed problem has an assignment.
The coverage theorem assumes aggregate capacity covers total demand.
The native classifier first checks original feasibility, which establishes this prerequisite and separates infeasibility from restricted allocation.

These proofs cover independent capacities and fixed integer-flow eligibility.
They do not prove that arbitrary grants, resource alternatives, or branch correlations reduce to that domain.
Settlement integration must establish that reduction before it uses the classifier to select an economic policy.
No Casper behavior changes follow from this helper.

### Native classifier contract

`classify_fixed_funding_domain` validates and solves the original problem before classification.
Its `FundingDomainClassification` result preserves the following evidence:

| Result | Evidence | Meaning |
| --- | --- | --- |
| `Infeasible` | Checked selected-obligation deficit. | No assignment funds the original obligations within the supplied capacities and eligibility. |
| `Unrestricted` | Complete original assignment and source totals. | Every vector in the capped simplex has an assignment in this fixed-flow problem. |
| `Restricted` | Complete original assignment, source totals, and independently checked capped counterexample. | Some capped vectors are infeasible although the original problem is feasible. |

A returned assignment proves feasibility, not minimax optimality.
The classifier does not compute fair contributions or advance a residual cursor.
Malformed problems, arithmetic overflow, allocation failure, and host-work exhaustion return errors instead of another policy.
The caller cannot supply a classification flag.

The implementation follows this sequence:

```text
solve the original problem
if infeasible: return its checked deficit
if total demand is zero: return unrestricted with the assignment
reserve work and storage for the transposed matrix
for each positive-demand obligation:
    form the excluded-source capacities
    if their sum exceeds all other demand:
        construct and check a restricted counterexample
        return restricted with the original assignment
    solve the transposed problem with the same work budget
    if infeasible:
        filter its deficit to the excluded-source positions
        construct and check a restricted counterexample
        return restricted with the original assignment
return unrestricted with the original assignment
```

The early comparison uses wide arithmetic and avoids an overflowing native transposed-demand sum.
`excluded_capacity_shortcut_has_violated_cut` proves that this comparison establishes a violated contribution cut.
The branch still constructs and checks a counterexample before it returns a restricted result.
For each transposed solver call, source and obligation limits exchange roles with the matrix dimensions.
The same host-work budget covers every call, preparation step, and witness check.
The classifier makes at most one original flow call and one transposed flow call per positive-demand obligation.
It stores one transposed matrix and reuses the excluded-source vectors.
Each accepted result retains the original source positions.

### Constructive restricted-domain witnesses

A restricted-domain witness contains a capped contribution vector and a set of obligations that the vector cannot fund.
It is counterexample evidence, not a proposed debit allocation.
The witness builder never changes wallet balances, reservations, or cursors.

[`FundingDomainWitness.v`](../../../../formal/rocq/cost_accounted_rho/theories/FundingDomainWitness.v) constructs this evidence from a violated source cut.
Its two-pass fill gives priority to the selected sources, then uses the other sources for the remaining total.
Each pass visits source positions in descending order and takes the lesser of capacity and remaining demand.
The result assigns $`\min(M,C(S))`$ to the selected sources without exceeding any individual capacity.
If aggregate capacity covers $`M`$, the complete vector sums to exactly $`M`$.

The witness selects all obligations outside $`N(S)`$.
A violated contribution cut makes their demand greater than their neighboring sources' contributions.
The independent deficit checker therefore proves that no assignment can realize the constructed vector.
This argument also proves that complete coverage of a nonempty capped simplex requires every contribution cut.

A failed transposed check can include zero-demand source positions.
The extraction first removes positions that are eligible for the omitted obligation.
The formal proof shows that this filter preserves a violated cut and produces a capped counterexample.
It does not treat a transposed boolean failure alone as sufficient evidence.

The native functions are `build_funding_domain_counterexample` and `check_funding_domain_counterexample`.
The builder validates the problem and selected-source dimensions before construction.
It reserves host work and allocation space before each bounded operation group.
It uses remaining-demand subtraction instead of summing capacities in the native amount width.
The builder checks every completed witness independently before returning it.

The checker verifies vector dimensions, every capacity bound, exact total contribution, and the selected-obligation deficit.
Malformed inputs return an explicit error.
A well-shaped but false certificate returns `false`.
The builder returns no witness when its constructed vector has no checked deficit.
That result does not prove that the complete domain is unrestricted.
Insufficient aggregate capacity and exhausted host work remain explicit errors, not classification results.

Each witness construction uses $`O(nm+n+m)`$ work and $`O(n+m)`$ additional space for $`n`$ sources and $`m`$ obligations.
The checker uses $`O(nm+n+m)`$ work and constant additional space.
Neither operation expands monetary amounts into individual tokens.
Callers must supply resolved physical capacities and the complete applicable eligibility relation.
These pure witness helpers do not authenticate custody or replace the complete classifier.

### Regression contract

Consider capacities $`[3,6]`$ and demands $`[2,2,2]`$.
The first source can fund only the third obligation.
The second source can fund every obligation.
Each individual excluded-capacity check passes because three does not exceed the other two obligations' combined demand of four.
But contribution vector $`[3,3]`$ cannot fund the first two obligations, which require four from the second source.
The transposed deficit certificate detects this restriction.

For a converse example, use capacities $`[1,1]`$ and demands $`[1,1]`$.
Allow the first source to fund only the first obligation and the second source to fund either obligation.
The only capped contribution vector is $`[1,1]`$, and it remains feasible.
The missing edge must not change the allocation policy.

The native tests compare the classifier with an independent unit-assignment oracle over every capped contribution vector in small domains.
Exhaustive cases cover all two-source, two-obligation graphs with capacities and demands from zero through two.
Generated cases cover up to five sources, source and obligation permutations, empty demands, and zero capacities.
Separate cases cover 129 sources, maximum-width capacities, aggregate capacity above the native amount width, and solver budget exhaustion.
The native classifier propagates budget errors instead of interpreting them as a restricted domain.
Its exhaustive oracle is not a production algorithm or a substitute for the unbounded proofs.

Every restricted result in that harness must produce a native witness accepted by the independent checker.
The original feasibility solver must also reject the witness's contribution vector.
Witness mutation tests change capacities, totals, eligibility, dimensions, and selected obligations.
Generated witness tests check wide-integer conservation, capacity bounds, selected-source saturation, and strict deficits through 129 sources.
Budget-prefix tests require every insufficient budget to return an error without a partial witness.
Classifier budget-prefix tests also cover preparation, transposed search, and certificate checking without substituting an economic policy.

## Relationship to the papers and Casper

The papers constrain authority, linear resource use, locations, and conservation.
The [source audit](funding-settlement-design-review.md#evidence-and-boundaries) records the relevant passages and local source digests for all three governing papers.
Those passages do not prescribe this monetary fairness objective.
Lexicographic minimax is the user's selected economic policy within those constraints, not a theorem attributed to the papers.

The rule does not choose blocks, votes, finality, recovery leaders, or pruning behavior.
Validators with the same authenticated inputs must reproduce the same funding result.
Independent funding scopes must remain concurrent.
Shared custody requires the existing dependency and atomic-publication controls, not serialization of all validators or shards.
