# Funding settlement design review

## Status and result

This review records the independent plan-agent investigation and the main-agent source audit on September 10, 2026.
It refines the [signed phlo proposal](signed-phlo-contract-proposal.md).
The initial review did not approve an economic policy or change production settlement.
The user subsequently selected pricing based on actual authority-resource demand.
The user also requested preservation of usage-only pricing as a documented alternative.
This selection changes the planned pricing basis, not production settlement by itself.
The user also approved equal sharing of newly required funding without automatic reimbursement of earlier sponsors.
The user requested an independent plan-agent review before integration of that rule.
The user explicitly prohibited regression to the retired escrow-based model.
Native SystemVault custody and authenticated located resources must remain the backing model.

The recommended architecture combines authorized contributions, backed resource acquisition, spectral consumption, and exact refunds in one atomic native transition.
The existing burn can implement consumption of the assigned backing.
A second unrelated monetary charge must not duplicate that consumption.

The initial choice between retaining burns and replacing burns was incomplete.
Paper-style funding slots provide a third starting point: sponsors supply resources without becoming every initiating authority.
However, sponsorship alone does not determine the monetary value of a compound token.
The valuation basis is now selected.
Its concrete tariff, backing refinement, and formal acceptance requirements remain necessary before production integration.

No voting, fork-choice, finality, pruning, or recovery-policy change is part of this plan.
The existing Casper protocol controls branch selection.
The integration must preserve parallel execution for independent funding and state dependencies.

## Evidence and boundaries

The audit used the working tree based on commit `9e58cf211885b2cdb319e030efa2af0491d71471`.
That tree contains uncommitted campaign changes, so the commit alone does not identify all inspected code.
The dev reference was `cdf447ac18710d9702a27379bce6c946f421be46`.
The following digests identify the inspected local paper files.

| Source under `../publications/` | SHA-256 |
| --- | --- |
| `cost-accounting/cost-accounted-rho.tex` | `71c01eb67e2d3447c5f8e6f90e494e7120587991165466382b53b0565dc0051c` |
| `cost-accounting-as-monad/continued-gslt-cost-v2.tex` | `c91a5f2f75ff960e78c415249945084583d493c31bf7b1d7b1a5088a898cc9b6` |
| `knotted-topoi/knotted-topoi.tex` | `02a5b1349e3d6a8d5157b7173e00e8d3e83fbfcae742136e20025497acbe96af` |

| Evidence | Design constraint |
| --- | --- |
| Rho paper, Rules 1–5 and N-ary join schema | Preserve every required authority occurrence. One atomic join is not one event per signer. |
| Rho paper, `eq:reverse-curry`, `prop:join-conservation`, and forbidden weakening | Split/Join preserves the authority multiset, not the number of token cells. Do not silently discard a compound component. |
| Rho paper, `sec:spectrum` | Signature-specific fuel is consumable. It is not only an authorization predicate. |
| Rho paper, `sec:funding-slots` | Funding and initiation can have different authorities. The initiating interaction still needs its own fuel. |
| Rho paper, `sec:data-dependent` | Use a sound bound with exact refunds for the accepted conservative fragment. A user limit is not a sufficiency proof. |
| GSLT paper, `sec:linear` and `prop:local-suff` | Local sufficiency composes when the interaction surfaces partition the obligations. Shared backing still needs an aggregate check. |
| Knotted-topoi paper, `sec:desugar` and `rem:fresh` | Preserve structural equations, location identity, and independent occurrences through translation. This does not select a monetary tariff. |
| Rho paper, “Fee conversion” | Authorized contracts can exchange token types. The example does not define the restored production price schedule. |

The current [native contract](end-to-end-authority-settlement.md) connects SystemVault custody with prepaid located stacks.
The [decision records](../cost-accounting-decision-records.md) distinguish the one-token denomination from retired duplicate-ledger mechanisms.
The [staged exchange design](staged-fee-exchange.md) explicitly marks its old ledger design as retired.
Those historical ledger sections must not control this restoration.

## Quantities that must remain distinct

| Quantity | Meaning | Example |
| --- | --- | --- |
| Interaction count | Successful atomic COMM events | One join is one event. |
| Authority demand | Required signature occurrences | A joint interaction can require both A and B. |
| Resource presentation | Physical arrangement of that authority | One compound cell or separate component cells. |
| Monetary contribution | Authorized value supplied by a physical payer | A capped share of newly required funding. |
| Persistent allowance | Remaining authorization across executions | Wallet top-ups do not increase this authorization. |

Adding envelope signers must not multiply an unchanged resource bill.
Adding genuine semantic authority requirements can change resource demand.
The final tariff must say whether that changed demand changes the monetary bill.
These statements are compatible only when the implementation keeps signer count separate from semantic demand.

The current implementation already separates several of these quantities:

- [`authority.rs`](../../../../rholang/src/rust/interpreter/accounting/authority.rs) selects funding presentations and tracks physical custody, stacks, and stack births.
- `allocate_quantitative_debit` scales a funding alternative by the measured byte amount.
- [`prepare_authority_stack_transfer`](../../../../rholang/src/rust/interpreter/accounting/mod.rs) reserves funding events before a new stack becomes available.
- [`VaultSettlement`](../../../../casper/src/rust/util/rholang/costacc/vault_cost_deploy.rs) separates the burn and fee within a physical reservation.
- [`acceptance.rs`](../../../../casper/src/rust/util/rholang/acceptance.rs) reconstructs authenticated inventories and checks state-bound evidence.

These mechanisms do not establish a general monetary valuation for every paper-authorized resource transformation.
The new mapping must connect them explicitly.

## Counterexamples that the plan must address

### Compound value cannot equal cell count by assumption

Consider three proposed monetary rules:

1. Buy one compound A/B token with one native unit.
2. Split that token into A and B tokens through the paper's explicit operation.
3. Redeem each resulting token for one native unit.

These rules would turn one unit into two without authorized minting or a subsidy.
They cannot all conserve monetary backing.
This is a conditional design counterexample, not a demonstrated native exploit or a paper defect.
The source audit did not establish that the complete round trip is available in the node.

The paper conserves the component authority multiset through regrouping.
It does not require every physical presentation to have an identical one-unit redemption value.
The formal mapping must cover complete cycles, including operational costs and authorized conversion terms.
An isolated stack-birth or stack-pop proof cannot establish that result.

### Previously funded resources must not incur a duplicate charge

Assume a selected valuation assigns five units to an obligation.
Two units already have valid prepaid backing.
Only three units of additional backing remain necessary under a new-funding policy.

For three equally eligible sources, those new contributions can be `[1, 1, 1]`.
Charging five additional units would duplicate the two prepaid units.
Equalizing lifetime spending would instead require explicit credit or reimbursement rules.
That policy is not equivalent to allocating new funding.

The example assumes compatible resource types and a defined valuation.
It does not convert arbitrary signature fuels into fungible cash by arithmetic alone.

### Aggregate solvency does not establish eligible funding

Suppose A and B each have one available unit.
One obligation can use either source, but a second obligation can use only A.
A greedy draw from A for the flexible obligation leaves the restricted obligation unfunded.
A valid plan instead uses B for the flexible obligation and A for the restricted obligation.

The same problem affects an aggregate fair split.
Suppose A has capacity two and B has capacity ten.
An A-only obligation needs two units, and a flexible obligation needs one unit.
The aggregate split `[1, 2]` cannot fund these obligations, although `[2, 1]` can.

The planner must retain the authenticated source-to-obligation relation.
It can reuse the current allocator directly inside a certified all-to-all funding scope.
For restricted scopes, it must find a feasible assignment and apply the approved fairness objective over feasible assignments.
Custody alias removal must not erase authority or scope restrictions.

### Restricted branches can need larger temporary reservations

Consider two mutually exclusive branches.
The first needs one unit from A only, and the second needs one unit from B only.
The maximum retained charge is one unit.
A fixed reservation of one unit from only one source cannot fund both possible outcomes.

Reserving one unit from each source supplies conservative backing for both outcomes.
Execution still charges at most one unit and returns the unused reservation.
This distinction needs explicit signed reservation exposure because temporarily unavailable funds also affect wallet users.

Three principled options exist:

- Authorize the larger source-specific reservation while preserving the smaller charge ceiling.
- Bind sufficient input data and causal state to establish the selected branch before reservation.
- Authorize a common funding scope that can fund either branch.

The implementation must not select an option silently.
A maximum charge does not automatically authorize a larger temporary hold.
The original equation equating total reservation with maximum charge applies only where the funding contract justifies it.

## Architecture and valuation alternatives

| Alternative | Advantage | Limitation |
| --- | --- | --- |
| Add signed limits to current draws | Smallest economic change. | Does not complete the requested general contribution sharing. |
| Allocate contributions into backed funding, then consume resources | Preserves native custody and the paper's funding distinction. | Needs explicit acquisition, valuation, eligibility, and refund rules. |
| Replace the monetary representation with priced entitlements | Can express richer prepaid pricing. | Requires a complete backing design for every issue, transform, consume, and redeem path. |
| Replace backing burns with an unrelated usage bill | Appears simple. | Cannot preserve old fuel spendability without an additional proof of backing. Reject this as an incomplete design. |

The second architecture is the recommended starting point.
Its phases can remain internal to one native transaction.
It does not require a second currency, a persistent central escrow, or a global lock.
It also does not permit sponsors to create another authority's fuel without authorized acquisition or transfer.

The no-escrow constraint is mandatory, not an implementation preference.
Keep reserve, consumption, fee transfer, and unused refund within the existing atomic settlement checkpoint.
Do not persist a new per-deployment escrow table or a singleton reservation map.
An immutable reservation calculation describes permitted effects without transferring custody to a second ledger.
Persistent prepaid rights remain native located resources, not an unrelated refundable credit balance.
Independent funding scopes must not share a global mutable reservation object.

The following sections distinguish the selected valuation basis from the documented alternative.

### Representation-invariant resource valuation

**Selected on September 10, 2026.** Price actual authority-resource demand, including genuine semantic multiplicity.
Do not use the number of wallets or envelope signers as a substitute for that demand.
Define value over the conserved resource obligation, not its current number of cells.
An additive valuation on a compound authority is one candidate:

```math
v(A \otimes B)=v(A)+v(B).
```

This candidate handles the Split/Join example without creating value.
It can price genuine authority multiplicity, even when one atomic COMM discharges those authorities together.
It does not justify multiplying an unchanged bill by the number of envelope signers.

Additivity alone does not prove every conversion or lollipop transfer sound.
Each permitted resource transformation still needs its own valuation or explicit conversion rule.
The selection establishes the pricing basis, not a numerical tariff or proof of every conversion rule.

### Service-usage valuation with explicit resource backing

**Not selected.** Preserve this alternative for design history and comparison, not as a runtime switch or parallel implementation.
Price the approved compute and byte measurements independently of authority multiplicity.
Retain the full spectral fuel requirements.
Then specify how prepaid entitlements retain backing through split, join, independent use, and redemption.

A shared backing record must not make independently usable split resources compete for backing that the paper guarantees to both.
Duplicating that backing is also invalid.
The design must supply sufficient backing at acquisition, an explicit later obligation, or another approved refinement.
Any later obligation must remain compatible with the promised up-front sufficiency guarantee.

This option may need a larger representation change.
It is not justified by keeping authority checks while deleting the old burn.
This alternative would require a new explicit decision and complete backing proof before adoption.
The current plan follows the selected authority-resource valuation instead.

## Recommended policy package for review

The following table records the selected valuation and recommendations for other policy details.
The valuation selection does not approve unrelated unresolved choices.

| Decision | Recommended starting point | Reason or unresolved boundary |
| --- | --- | --- |
| Contribution fairness, selected | Allocate newly required funding. Do not reimburse earlier sponsors automatically. | User-approved, subject to the requested design review. Credit only compatible, authenticated prepaid resources. |
| Fuel valuation, selected | Price actual authority-resource demand with representation-invariant backing. | Semantic demand can affect price. Wallet count alone cannot multiply charges. |
| Prepaid price | Capture acquisition terms for the acquired right. | Do not reprice that acquisition during later consumption. Separate later execution overhead and resource-version compatibility. |
| Refunds | Return unused backing to captured original sources. | Later ownership must not redirect an earlier reservation's refund. |
| Encumbered transfer | Transfer available rights separately from immutable existing reservations. | Partial transfers divide authorization. They do not duplicate or silently novate obligations. |
| Restricted funding | Apply fairness only among feasible contributions. | Unconstrained water filling cannot override source restrictions. |
| Branch reservations | Require explicit source-specific reservation exposure when conservative backing exceeds the charge ceiling. | Otherwise require dependent evidence or an authorized common funding scope. |
| Conversion | Use authenticated conversion before reservation initially. | Atomic conversion-and-reservation remains a separately specified alternative, not an implicit reverse exchange for refunds. |
| User failure | Preserve existing behavior until a failure matrix receives approval. | Specify retained fee and bounded realized charges separately from application rollback. |

The already selected controls remain unchanged:

- `phloPrice` is the signed offered price. Separate owner ceilings bound that price for every required authorization.
- Every required compatible consent must permit the actual price. The effective ceiling is their minimum.
- `phloLimit` bounds one execution scope, not every future firing of a persistent process.
- The persistent allowance limits cumulative authorized draws across executions and transfers.
- Wallet top-ups increase backing, not allowance or price consent.
- Ownership can transfer through arbitrary finite histories. The implementation must not impose a two-owner or two-transfer rule.
- Exchange quotes can differ without creating wallet-specific consensus resource tariffs.

Optional alternative funders need only satisfy the consent for the selected valid plan.
The planner must not take a minimum over unrelated owners or bypass a required low-ceiling owner.
Initial funding, dormant continuation activation, and later top-up must each bind the applicable scope and authority.

## Plan-agent review of new-funding-only sharing

The requested independent review supports the user-approved rule, subject to the integration requirements below.
The main-agent source audit agrees with these findings.
This is a design review, not a completed proof of the runtime or approval of the remaining economic choices.

The rule is: match valid prepaid resources to eligible obligations, then share only the remaining acquisition cost among authorized sources.
Do not reimburse earlier sponsors automatically.
Equal monetary value does not make different authorities, locations, or resource classes interchangeable.
For example, a prepaid A resource cannot offset a B obligation solely because both have the same price.
Repeated authority occurrences remain repeated demand, and a zero monetary share does not remove required authority or consent.

The paper's signature spectrum and funding-slot sections support this separation between resources, initiating authority, and sponsorship.
The GSLT local-sufficiency proposition requires partitioned interaction surfaces, not a shared untyped credit pool.
Neither passage prescribes equal monetary sharing or historical reimbursement.
New-funding-only sharing is the user's selected economic policy within those constraints.

### Prepaid rights and price changes

Credit an authenticated resource discharge before pricing newly acquired resources.
Do not subtract historical currency spending from a current aggregate bill as if those amounts represented interchangeable resources.
Historical acquisition terms, current acquisition prices, and uncovered execution overhead are different quantities.
For a valid prepaid right acquired at price one, a later price of two must not silently create another acquisition charge.
Schedule compatibility and separately billable overhead still need their complete contract.

The current `CostStack` representation contains signatures without acquisition-price metadata.
The physical inventory contains signature cells, custody bindings, and birth identities.
Those fields alone do not prove historical acquisition terms or redemption ownership.
Integration must establish authenticated provenance through canonical native resource metadata and retained evidence.
It must not introduce a second spendable credit balance to represent that provenance.
This is a demonstrated representation gap for the proposed integration, not a demonstrated deployed exploit.

### Refund and transfer distinction

An unused current reservation returns to its captured source custody.
A completed transfer of an available prepaid right changes that right's authorized ownership or location.
The refund rule must not undo that transfer by paying the historical sponsor.
Unused prepaid cells normally remain unconsumed rights, not automatic cash refunds.
Any permitted redemption requires its own authorized resource transition.

### Approved restricted-funding objective

The current capped max-min allocator retains its approved all-to-all funding scope and rotating residual rule.
The user approved lexicographic minimax for restricted funding on September 10, 2026.
Minimize the descending contribution vector lexicographically over feasible plans with the same total obligation.
The [restricted-funding decision](lexicographic-minimax-funding.md) defines this ordering, examples, scope, and verification requirements.
Two mathematical objectives can differ even when both informally mean equal sharing.

Suppose the only feasible contribution vectors are `(0, 5, 5)` and `(1, 1, 8)`.
Leximin contribution first maximizes the smallest contribution and prefers `(1, 1, 8)`.
Lexicographic minimization of the largest burdens prefers `(0, 5, 5)`.
This is a conditional mathematical counterexample, not a claim that the native planner currently produces this feasible set.
It shows why the unrestricted allocator alone does not settle the general policy.

The objective is now selected, but canonical ties and zero-new-funding cursor behavior still need their complete implementation contract.
Any fixed-flow reduction must establish the properties that justify reuse of the approved allocation rule.
The separate fixed-fee policy and cursor must remain unchanged unless their combination receives approval.

### No-escrow refinement and implementation gaps

`SystemVault.applyCostAmounts` uses `reserveAll`, settlement validation, and `settleReserved` within its lexical execution.
`reserveAll` holds transient split-purse capabilities in a local list, and `refundReserved` returns unused value to source custody.
These internal mechanisms are not the retired persistent escrow ledger.
Do not remove correct atomic custody mechanics merely because their names contain reservation terminology.

The abstract reservation maps in Rocq and TLA+ can remain proof state.
An explicit refinement must connect their private steps to the complete observable native checkpoint.
It must prove that failure cannot leave an externally spendable intermediate hold or a partial retained economic effect.
The abstract maps do not authorize a production reservation table or a global funding lock.

| Component | Required integration beyond its existing checks |
| --- | --- |
| `AuthorityResourceDemand` | Use the retained event and region witnesses for location restrictions. Bind resource classes, schedules, prepaid matches, and authenticated causal inputs. |
| `FundingAssignmentTotals` | Retain source and obligation identities, the eligible-edge assignment, consent, and the canonical fairness witness. Totals alone are insufficient. |
| `FundingBranchReservation` | Bind identical source order across branches, authenticated exposure, canonical branch identity, and complete reachable-branch coverage. |
| Native vault conversion | Check the `u64` helper amounts and `u128` aggregate exposure against the vault's `i64` representation before constructing calls. Never truncate. |
| Publication models | Treat independent validators as separate causal states. Preserve real shared-custody dependencies and disjoint-scope concurrency. |

### Required completion sequence

#### Typed prepaid-discharge proof

[`PrepaidResourceDischarge.v`](../../../../formal/rocq/cost_accounted_rho/theories/PrepaidResourceDischarge.v) adds an executable witness checker and its Rocq specification.
A resource key contains a location, resource class, compatibility-terms identifier, and complete authority signature.
Each list occurrence represents one resource occurrence, including repeated signatures.
These mathematical identifiers do not define a new wire format or native storage schema.

The witness supplies five resource lists: available, required, used, unused, and fresh.
The checker requires available resources to equal used plus unused resources as multisets.
It also requires demand to equal used plus fresh resources as multisets.
The full funding checker rejects an unused resource when an identical fresh obligation remains.
Thus, a plan cannot skip compatible prepaid supply to increase new acquisition charges.

For each complete resource key, the checked residual equals demand minus available supply, with subtraction bounded below by zero.
The proofs preserve repeated demand and prohibit spending more occurrences than the supplied inventory contains.
They also prove conservation across arbitrary finite discharge histories and composition of separately supplied inventory partitions.
Valuation conservation holds for any nonnegative price function over complete resource keys.
A fully prepaid compatible obligation has zero new acquisition cost at any current price.

The checker counts occurrences across both input lists, then verifies each partition and the absence of unused compatible supply.
This is a specification checker, not a production matching algorithm.
Its repeated list scans have quadratic worst-case cost.
A production implementation should refine an indexed multiset operation and retain full native witnesses.

The regression examples reject authority, location, class, and compatibility-term substitutions.
They also reject duplicate consumption and unnecessary acquisition despite available compatible supply.
Generated checks cover 729 valid two-key partitions and all 3,125 single-key count combinations from zero through four.
The rejection oracle compares the executable checker with independent count equations.
The universal theorems are not restricted to those generated test sizes.

On September 10, 2026, `coqc` compilation and recursive `coqchk` kernel checking passed for this module.
All ten printed assumption reports were closed under the global context.
The checked source contains 244 lines and 22 `Qed.` or `Defined.` terms.
Its SHA-256 digest is `b7dbc02acf36d4464346bb0ac3ea9da059ab5bb23c7910d8a653bd15eb894c87`.
Both commands ran with a 2 GiB memory limit, disabled swap, and a one-CPU quota.
This focused result does not establish that the full campaign verification gate passes.

This proof assumes that native validation supplies authentic, eligible resources without duplicated physical inventory entries.
It does not prove that an arbitrary stack cell is available or permit reordering a live stack.
Signature normalization and authorized resource transformations must occur before exact-key matching, with separate correctness evidence.
Compatibility-term identifiers represent established compatibility, not an assumption that every historical acquisition remains valid.
The price theorem concerns new acquisition only, not separately billable execution overhead or redemption.

Native integration must retain custody, stack position, birth identity, causal availability, and original region evidence.
Concurrent operations must partition actual resource occurrences or validate against the same retained checkpoint before publication.
The composition theorem cannot justify two operations spending the same physical occurrence from separate stale snapshots.
Rust property tests, Loom checks, and checkpoint fault tests must establish this correspondence before the runtime uses the checker contract.
No Casper behavior, native settlement behavior, or persistent escrow representation changes in this proof step.

#### Integration order

1. Specify typed prepaid discharge, acquisition terms, resource units, and compatibility rules.
2. Complete the approved minimax refinement, restricted tie contract, and failure-charge contract without selecting unapproved economic behavior.
3. Specify canonical provenance and signed witness fields, including source and branch identities.
4. Prove the full backing lifecycle and its no-escrow native refinement before production integration.
5. Construct authenticated solver inputs and complete witnesses instead of passing unbound numeric vectors directly into settlement.
6. Integrate through existing `applyCost` and checkpoint publication, then complete replay, client, transfer, and refund paths.
7. Verify the native lifecycle with the invariant families in the acceptance table below.

The required tests include incompatible prepaid authority, location mismatch, price changes, repeated resources, and valid ownership transfers without sponsor reimbursement.
They must also cover restricted eligible graphs, branch-identity mutations, source-order mutations, zero new funding, and every integer conversion boundary.
Concurrent checks must include top-ups, transfers, shared custody, disjoint scopes, interruption, duplicate delivery, restart, and cold replay.
Use Loom for actual Rust synchronization, and use native checkpoint tests for RSpace publication.
Independent validator models must not assume one globally synchronized balance map.

The plan agent performed a read-only audit without tests, model execution, or code changes.
This review adds acceptance requirements to existing leaves, not a new epic or a claim of completed implementation.

## Native lifecycle and algorithm contract

The planner receives certified causal state, signed terms, authority obligations, resource bounds, eligible backing sources, and persistent allowances.
It returns either a complete canonical witness or a specific rejection.
No provisional output authorizes a debit.

1. Authenticate the execution scope, schedule, ownership generation, and funding restrictions.
2. Derive exact or sound conservative resource obligations for every located surface.
3. Inventory prepaid resources and physical custody without duplicate backing.
4. Establish permitted source-to-obligation assignments and any authorized conversion.
5. Compute the remaining contribution under the approved valuation.
6. Find a feasible capped allocation with the approved canonical residual rule.
7. Capture source reservations, resource rights, consent, refund destinations, and the cursor.
8. Verify realized usage against every captured bound.
9. Consume assigned backing once and calculate exact unused refunds.
10. Publish retained application effects, economic effects, evidence, and cursor changes atomically.

Steps 1–7 describe planning, not an additional externally committed funding transaction.
The native implementation may combine phases when the refinement proves equivalent behavior.
Prepaid acquisition that intentionally persists requires its own authenticated retained output.

The unrestricted allocation fast path needs a proof of all-to-all eligibility.
The restricted planner needs joint feasibility, not sequential greedy allocation of located obligations.
Integer flow is a candidate for fixed, linear source-to-obligation edges.
For that restricted subproblem, the funding witness assigns integer amounts $`x_{ij}`$ to authenticated eligible edges $`E_{ij}`$.
Let $`b_j`$ denote obligation amounts and $`C_i`$ denote source capacities in one compatible unit.

```math
x_{ij}\ge 0, \qquad
\neg E_{ij}\Rightarrow x_{ij}=0, \qquad
\sum_i x_{ij}=b_j, \qquad
\sum_j x_{ij}\le C_i.
```

Grants, aliases, and ownership consent add constraints that this simple equation does not discharge.
Fairness ranks only feasible source totals, with canonical residual preferences among feasible tied results.
Use capacity-based algorithms, not one iteration per smallest currency unit.
Compound alternatives, rate conversions, and indivisible claims can require additional discrete search.
Do not claim polynomial or linear complexity for that general problem without a proved reduction.

Configure bounds for owners, physical sources, authority occurrences, obligations, proof bytes, and search work separately.
The current proposed signer default is 64, but logical proofs must quantify over arbitrary finite inputs.
Exhausting a search budget must not produce an invalid plan or claim that sufficient funds do not exist.
Keep the metering and rejection rule deterministic where accepted evidence depends on the result.

Cryptographic commitments must bind the complete plan domain, protocol version, resource schedule, ownership evidence, and source provenance.
The client signature authorizes intent, while the block commitment authenticates retained execution evidence.
An extra standalone plan signature remains a separate contract decision.
No live external quote or latest-local ownership lookup can replace captured evidence during replay.

## Formal and executable acceptance requirements

This table specifies required evidence. It is not a claim that the new valuation is already verified.

| Requirement | Formal target | Independent executable evidence |
| --- | --- | --- |
| No weakening or duplicated authority | Arbitrary signature multisets, partitions, and Split/Join histories in Rocq. | Generated compound presentations, repeated atoms, located resources, and zero monetary shares. |
| Complete backing conservation | Issue, transfer, split, join, consume, refund, redeem, and authorized mint in one compatible valuation. | Round-trip histories with an independent backing oracle. Unsupported native paths must reject explicitly. |
| Correct funding assignment | Feasibility and constrained fairness for eligible edges. Prove the unrestricted fast-path condition. | Exhaustive small assignment oracle, both counterexamples above, overlapping scopes, and reordered inputs. |
| Sound up-front funding | Compose fixed, dependent, or conservative surface proofs with monetary and allowance bounds. | Data-dependent branches, continuation activation, insufficient signatures, insufficient money, and search exhaustion. |
| No alias overdraw | Aggregate all reservations against each physical source while retaining logical occurrences. | Multiple grants, joint and leaf aliases, and concurrent top-ups. |
| Correct allocation and refunds | Prove conservation, exposure, residual bounds, and feasibility for every permitted realized obligation. | Independent allocation oracle and original-source refund assertions. |
| Restricted branch reservations | Separate maximum retained charge, temporary reservation, source exposure, and surface bounds. | Mutually exclusive A-only and B-only branches, exact unused refunds, and missing reservation consent. |
| Ownership safety | Arbitrary finite transfer histories preserve consumption, reservations, and captured consent. | Competing transfers, returning owners, partial transfers, and stale generations. |
| Parallel publication | TLA+ models local workers and independent validators with separate candidate states. | Multi-validator replay, overlapping funding, disjoint funding, rejected branches, and reversed merge order. |
| Native synchronization | Relate concrete Rust transitions to the publication model. | Loom over actual synchronization boundaries, plus RSpace integration tests. |
| Failure and retry safety | Distinguish admission rejection, exhaustion, user failure, platform failure, interruption, and duplicate delivery. | Fault injection before and after each economic mutation. |
| Semantic translation | Preserve applicable paper equations, causal locations, and resource observations. | Native operator, desugaring, quotation, and replay correspondence tests. |
| Evidence integrity | State assumptions for signatures, hashing, encoding, causal inputs, and version dispatch. | Mutation tests and cold replay under newer local ownership and schedules. |

Every proof row needs an invariant identifier, source transition, native entry point, test oracle, bounds, and evidence artifact.
An abstract Boolean such as `funding_valid` cannot discharge the final backing obligation.
Loom cannot replace distributed models, and a distributed model cannot establish Rust publication behavior.
Finite TLA+ exploration and generated tests must report their bounds.
General Rocq theorems must state their assumptions and the native refinement that remains necessary.

Expected-refutation models must detect duplicated Split backing, alias overdraw, prepaid double charges, greedy starvation, and invalid equal splits.
They must also detect reservations that fit the scalar maximum but cannot fund every certified branch.
They must also detect allowance reset, redirected refunds, stale consent, partial publication, and retained effects from rejected branches.
A disjoint-funding progress scenario must remain reachable without a global funding lock.
This establishes a scoped concurrency property, not unconditional network liveness.

## Work sequence in the existing epic

### Checked valuation algebra

[`AuthorityResourceValuation.v`](../../../../formal/rocq/cost_accounted_rho/theories/AuthorityResourceValuation.v) implements the first algebraic refinement of the selected pricing basis.
It reuses `CostAccountedSyntax.sig`, `SignatureMonoid.sig_equiv`, and `CAJoinConservation.sig_atoms`.
It assigns zero authority units to the signature monoid identity and one unit to each ground or quoted atom.
Compound units add without removal of repeated atoms.
One supplied unit price scales that demand without changing price by wallet identity.

This valuation measures authority resources, not a complete execution tariff.
Zero authority units do not authorize free execution or bypass byte safety accounting.
The signature monoid identity must have zero additive value to preserve its existing identity law.
The final tariff must separately retain every required resource charge.

The checked results include:

- Value respects the existing signature-equivalence relation.
- N-ary join value agrees with the existing conserved authority multiset.
- Every finite history of the modeled Split, Join, presentation reorder, and equivalent-signature steps preserves value.
- Stack concatenation adds value, while head consumption accounts for the exact consumed value.
- A positive component cannot disappear without reducing value.
- Repeated authority remains repeated demand.
- Equal value does not establish equal authority.

The regrouping theorem does not permit temporal stack reordering during execution.
It compares resource presentations, not execution traces.
It also does not prove lollipop conversion, acquisition, redemption, or concurrent settlement.
Those operations must satisfy their own refinement obligations in the acceptance table.

The module contains 17 completed proof terms across 168 source lines.
Its generated regression checks three valuation properties over 156 signature trees at five prices, including zero and positive boundary examples.
Those 780 instances supplement the universally quantified algebraic theorems.
They are executable model checks, not native Rust property tests.

On September 10, the eight-module dependency closure compiled from source in a separate disk-backed directory.
The independent `coqchk` run succeeded, and all four reported theorem assumptions were closed under the global context.
Both commands used systemd scopes with a 2 GiB memory cap and swap disabled.
The source digest is `f71aeafe7d7523e46b7d57e0c4b085182af15c1372af23e89e31b61b7d63ffc4`.
The logs are under `target/verification/authority-valuation-20260910/` and are disposable build evidence.
This permanent record retains the checked scope and digest when those artifacts are cleaned.

The normal proof gate imports the module and checks the four principal theorem assumptions.
The aggregate proof suite was not rerun for this focused addition.
The source inventory at that checkpoint was 159 modules, 64,111 lines, and 2,994 proof terms.

### Native valuation boundary

[`AuthorityResourceDemand`](../../../../rholang/src/rust/interpreter/accounting/authority/valuation.rs) exposes the atomic demand of a validated native `AuthorityEvent`.
It uses the existing event check that compares declared debit with canonical authority regions.
It preserves the complete event, including its identifier, original regions, signatures, and declared debit.
Its separate atomic projection preserves every repeated authority occurrence for pricing.
Its value calculation rejects multiplication and addition overflow instead of wrapping or saturating.

The existing decomposed funding option now uses the same extracted demand calculation.
This extraction preserves the existing funding options and their order.
It does not change prices, vault debits, certificate encoding, or Casper behavior.
Admission must still authenticate region provenance, grants, and causal state.
This value object does not establish those properties or authorize a debit.

The native tests map the valuation model to these checks:

| Formal property | Native check |
| --- | --- |
| Split/Join preserves atomic value | Separate regions and a compound signature produce equal atomic demand and value. |
| Repeated authority remains repeated demand | Repeated atoms increase their quantity instead of disappearing through deduplication. |
| Equal value does not establish equal authority | Different authority identities retain distinct demand maps at equal prices. |
| The monoid identity has zero additive value | The unit signature contributes no atomic demand. Other execution charges remain separate. |
| Natural-number valuation has no wraparound | Native boundary cases reject component and aggregate `u64` overflow. |

On September 10, all five new tests passed, including 256 generated cases with up to 64 authority occurrences.
The generated test compares native demand with an independent occurrence-count map and checks existing funding-option membership.
These cases supplement the algebraic proofs, not exhaustive native verification.
Strict Clippy passed for the `rholang` library and test sources with warnings denied.
Both commands used sequential systemd scopes with a 6 GiB memory cap and swap disabled.
The Rust source digest at that checkpoint was `64af85daecce5d9ea3c991899d79ed509dfa58853da5907a7b912d2cde530b22`.

The first wider authority run passed 56 tests and failed one existing certificate golden-vector test.
That test produced `093145bb99125f8918e7c93f711a6164c7f8cfc6d166a9f4657b1c8a19410205` instead of `1a6cbf75519760b1729bf5a5f0c876c6a2af8e7bf492cbffb40f27d5dd060eef`.
Earlier campaign edits changed the certificate domain, protocol version, and fee-plan encoding while retaining the expected digest.
The valuation path does not participate in that fixture's certificate encoding.
The subsequent client audit confirmed that Python still parsed and hashed version-eight evidence.
The [version-nine compatibility repair](rotating-monetary-allocation.md#client-certificate-compatibility) corrects this format gap and records independent encoding evidence.
After that repair, all 57 authority tests and strict Clippy passed.

#### Region-witness regression

The subsequent prepaid-discharge audit found that the first pricing helper discarded original regions after computing aggregate atom demand.
A failing regression demonstrated that different regions produced equal demand objects when their event identifiers and aggregate atoms matched.
This was a representation gap in the new helper, not a demonstrated production consensus failure.
The source audit found no production caller of this helper at that checkpoint.

The repair stores an owned copy of the structurally validated `AuthorityEvent` alongside its aggregate atom projection.
An immutable accessor exposes the retained event without allowing callers to change the checked representation.
Changing the caller's original event cannot change this stored witness.
Compound and separate presentations still have equal atomic value, but their complete region witnesses remain distinct.

The native regression now passes, and the wider authority suite passes all 61 tests.
Additional examples reject malformed region identities and duplicate noncanonical regions.
Two property tests run 256 generated cases each.
They check occurrence counts, full-event retention, and region mutations that preserve value but must change the witness.
These tests connect the location distinction in `PrepaidResourceDischarge.v` to the native pricing representation.
They do not prove resource availability, acquisition-term compatibility, or checkpoint publication.

The repaired source digest is `535eb3e971abb9e63555e319534fc90b2047f038a3bc8ff36a3ca7c443e2bd4f`.
Strict Clippy passed for the `rholang` library and test sources with warnings denied.
The regression, wider tests, and Clippy used a 6 GiB systemd memory cap, disabled swap, and a two-CPU quota.
Funding-option generation, certificate bytes, prices, native vault effects, and Casper behavior remain unchanged by this repair.

### Checked funding assignments

[`EligibleFundingAssignment.v`](../../../../formal/rocq/cost_accounted_rho/theories/EligibleFundingAssignment.v) defines a finite source-to-obligation assignment and its executable checker.
The model uses arbitrary finite source and obligation counts.
It does not restrict funding to two wallets.
Sources represent distinct physical custody positions, not logical authority aliases.

The checker establishes three conditions:

1. Check that each source draw fits that source's capacity.
2. Reject positive draws through ineligible source-to-obligation edges.
3. Check that incoming draws cover every obligation exactly.

The boolean checker is equivalent to the mathematical validity predicate.
The proof also establishes equality between total source draws and total obligations.
For each branch, the source-specific maximum of certified branch draws preserves assignment feasibility.
The branch theorem permits different eligibility graphs for different branches.
It does not claim that this conservative reservation minimizes temporarily held funds.

The formal counterexamples demonstrate two boundaries.
A greedy assignment can fail although a feasible assignment exists.
Two mutually exclusive restricted branches can require two reserved units while each actual charge remains one unit.
These examples establish design requirements, not discovered native settlement exploits.

The native [`check_funding_assignment`](../../../../rholang/src/rust/interpreter/accounting/monetary_allocation/funding_assignment.rs) follows the checked row, column, and eligibility conditions.
It validates dimensions and independently configured source and obligation caps before allocating its result buffers.
It rejects empty source sets and checked-integer overflow.
Zero obligations remain valid for a nonempty source set, with zero draw from each source.

The mathematical model uses natural numbers.
The native checker additionally requires every intermediate sum and the total obligation to fit `u64`.
Its time complexity is proportional to source count times obligation count.
Its additional memory is proportional to source count plus obligation count.
It does not iterate once per currency unit.

The checker is a pure kernel component, not an admission certificate or a complete funding solver.
Its caller must authenticate eligibility, normalize physical custody, preserve logical authority multiplicity, and bind the inputs to causal evidence.
The caller must also derive validated demand under the selected valuation and check captured grants and reservations.
This addition does not wire the component into vault debits or change Casper behavior.

| Invariant | Formal evidence | Native evidence and boundary |
| --- | --- | --- |
| `FUND-ASSIGN-01` | `assignment_check_exact` | Dimension, capacity, eligibility, exact-coverage, and wide-integer oracle tests. Input authentication remains the caller's obligation. |
| `FUND-ASSIGN-02` | `accepted_assignment_conserves_obligation` | Generated feasible assignments compare source totals and obligation totals. Overflow rejects instead of wrapping or saturating. |
| `FUND-ASSIGN-03` | `branch_envelope_preserves_feasibility` | The restricted-branch example verifies distinct source reservations. Native envelope construction is a separate integration requirement. |
| `FUND-ASSIGN-04` | `exclusive_branches_cannot_share_one_restricted_hold` | The one-source hold fails the alternate restricted branch. Consent for larger holds remains explicit. |

The initial assignment checkpoint contained 16 proof terms across 199 source lines.
Fresh `coqc` and independent `coqchk` checks succeeded under 2 GiB, no-swap systemd scopes.
All four reported theorem assumptions were closed under the global context.
The source digest is `0e76f8dbc1ae2c1753055bdcab2efbcd4bd023b631534f6a7634c8a15d39f124`.

The final native checker passed nine targeted tests, including two property tests with 256 cases each.
Generated feasible assignments check source and obligation reordering.
Deterministic tests cover every positive source count through configured caps of 1, 3, 64, and 128, plus cap violations.
Other examples cover ineligible draws, greedy failure, incorrect equal shares, dimension errors, exact coverage, and `u64` overflow boundaries.
The wider monetary-allocation suite passed 43 tests before the final source-arity test was added.
Strict Clippy passed for the final `rholang` library and test sources with warnings denied.
The Rust source digest at that checkpoint was `d4bf9ed8cd6c81cb7bddafd1ce1fc6ad5fca289f6cadbae618779beb49076a66`.
The later [complete fixed-flow reference](lexicographic-minimax-funding.md#complete-fixed-flow-reference) adds candidate-coverage proofs and exhaustive native feasibility checks.

The native test and lint commands ran sequentially in a 6 GiB, no-swap systemd scope.
Combined concurrent memory caps did not exceed 8 GiB.
All scratch files and logs used `target/verification/authority-valuation-20260910/`, not `/tmp`.
The normal proof gate now imports both new modules and checks their principal theorem assumptions.
The source inventory at that checkpoint was 160 modules, 64,310 lines, and 3,010 proof terms.

### Checked branch reservations and refunds

The assignment model now also defines the exact refund for each source after selection of a certified branch.
Three added theorems prove per-source conservation, total conservation, and minimal reservation for a fixed set of plans.
The last theorem does not optimize allocation across possible alternative plans.
It establishes that every hold covering the supplied plans must dominate their per-source maximum.

[`FundingBranchReservation`](../../../../rholang/src/rust/interpreter/accounting/monetary_allocation/funding_reservation.rs) implements that calculation over checked assignment totals.
Its constructor requires source capacities, explicit source exposure limits, branch plans, and independently configured source and branch caps.
It checks dimensions and caps before allocating the hold vector.
It then computes each source's largest branch draw and rejects holds beyond capacity or permitted exposure.
The object keeps the branch plans immutable and rejects settlement selection outside the supplied branch set.

The caller must bind every source position to the same authenticated physical custody across all plans.
The caller must authenticate exposure consent and prove that the supplied branch set covers the accepted execution.
Checked assignment totals alone do not establish those facts, canonical branch order, or a fair allocation policy.
This numeric component does not publish a reservation, debit a vault, or choose a production failure-charge policy.

The returned refund is the held amount minus the selected branch's draw at each original source position.
No refund operation consults a later owner or redirects a refund to another position.
Custody identity and actual transfer execution remain native integration obligations.
Repeated calls calculate the same result without changing state or performing duplicate payments.

Total temporary exposure uses checked `u128` arithmetic, while each source hold and each branch charge remains `u64`.
For two exclusive branches requiring `u64::MAX` from different sources, the combined hold can exceed `u64::MAX` without expanding either branch's charge.
The maximum retained charge and temporary exposure therefore remain separate quantities.
Other resource charges must enter their own approved funding obligations before this component can describe a complete deployment.

For $`b`$ supplied branches and $`n`$ source positions, construction takes $`O(bn)`$ time and $`O(n)`$ additional space beyond retained plans.
The object owns $`O(bn)`$ plan data.
One branch refund takes $`O(n)`$ time and output space.
No operation loops once per currency unit.

| Invariant | Formal theorem | Native evidence |
| --- | --- | --- |
| `FUND-RESERVE-01` | `branch_reservation_covers_every_plan` | Every supplied draw fits the returned hold. Unknown branch selection fails. |
| `FUND-RESERVE-02` | `branch_settlement_conserves_each_source` | Each original source's draw plus refund equals its hold. |
| `FUND-RESERVE-03` | `branch_settlement_conserves_total` | Total draw plus total refund equals total held funds, including wide totals. |
| `FUND-RESERVE-04` | `branch_reservation_is_least_for_fixed_plans` | A maximum oracle matches holds, and reducing any positive exposure limit rejects the same plans. |

Fresh compilation and independent kernel checking passed before the native implementation.
The expanded module contains 19 proof terms across 236 lines, with seven closed theorem-assumption reports.
Its digest is `736cc95a9f46109ccf532fa7462737e11f58a5fcca99caf1931ef93c479dde2c`.
The normal proof gate includes all three additional theorem-assumption checks.
The updated source inventory is 160 modules, 64,347 lines, and 3,013 proof terms.

All 49 monetary-allocation tests passed, including the five new reservation tests.
Strict Clippy passed for the Rholang library and test sources with warnings denied.
The generated reservation test uses 256 cases with one through 32 sources and one through eight branches.
It checks maximum holds, exact refunds, branch-order invariance, immutable plans, and insufficient exposure.
Deterministic tests cover source counts from one through 129, malformed dimensions, separate caps, zero draws, and wide totals.
The native source digest is `e5a2a86fab59ed1185ab8312decefb91be39b5b0b467d8aaec973595a528c023`.
Proof checks used a 2 GiB systemd scope, and native checks used a 6 GiB scope after the proofs completed.
Swap was disabled, and scratch files remained in the disk-backed verification directory.
These checks do not establish concurrent native publication or complete runtime coverage.

### Remaining sequence

Implementation depends on an approved contract and formal mapping.
Validation must exercise the resulting production implementation.

| Stage | Required result before dependent work |
| --- | --- |
| Contract | Record the selected authority-resource valuation and distinguish semantic demand from signer count. Preserve the unselected alternative. |
| Contract | Approve contribution basis, prepaid terms, failure charges, conversion boundary, and encumbered transfers. |
| Contract | Specify dimensionally valid tariff units and exact rounding. |
| Contract | Specify authenticated eligibility and all independent size and work bounds. |
| Contract | Specify feasible-assignment fairness and the certified unrestricted fast path. |
| Contract | Specify the complete backing lifecycle and native mutation inventory. |
| Contract | Separate the maximum retained charge from source-specific conservative reservation exposure. |
| Formal mapping | Prove the approved valuation respects applicable paper transformations. |
| Formal mapping | Prove no duplicate backing or prepaid charge through acquisition and settlement. |
| Formal mapping | Check competing workers, aliases, transfers, and failure boundaries. |
| Formal mapping | Check independent validators and disjoint shards without a shared global state assumption. |
| Formal mapping | Map every requirement to native paths and executable invariants. |
| Implementation | Integrate complete canonical planning without a second allocation policy. |
| Implementation | Retain one backed economic effect through `applyCost`. |
| Implementation | Preserve prepaid funding and arbitrary transfer histories across executions. |
| Implementation | Compose the selected valuation with deployment limits and persistent allowance. |
| Implementation | Recompute the complete economic witness against captured causal state. |
| Validation | Exercise every backing transition and counterexample with an independent oracle. |
| Validation | Test eligibility-constrained allocation separately from unrestricted water filling. |
| Validation | Verify actual concurrent native publication boundaries. |
| Validation | Verify signed end-to-end execution, replay, restart, and parallel-validator behavior. |

These acceptance conditions must be explicit before implementation starts.
This table specifies dependencies, not proof completion claims.
Formal checks and heavy tests must use bounded systemd scopes and disk-backed scratch space.

## Review conclusion

The direction remains viable, but the earlier replacement recommendation was too strong.
The papers, native backing, and monetary allocation need one explicit refinement.
The user selected actual authority-resource demand as the pricing basis.
The contribution basis, restricted-scope fairness details, and conservative reservation consent still need their complete contract.
The recommended default is new contributions without automatic reimbursement.

The plan agent and main agent agree on the need for source eligibility and complete backing proofs.
They do not claim that the proposed tariff or replacement representation is already correct.
Production changes must satisfy the selected valuation, remaining economic contracts, and their formal acceptance conditions.
