# Signed phlo formal contract

## Scope

[`SignedPhloControls.v`](../../../../formal/rocq/cost_accounted_rho/theories/SignedPhloControls.v) defines an executable reference contract for signed prices, resource limits, prepaid credit, and retained charges.
It composes the existing authority valuation, typed prepaid discharge, and required-owner price models.
It does not replace signature-specific resource consumption with monetary sufficiency.

The [signed phlo proposal](signed-phlo-contract-proposal.md) defines the wider integration contract.
The [economic ratification](economic-activation-policy-ratification.md) selects the failure and activation policies.
This reference model does not activate those policies in the node.
It does not modify Casper selection, voting, finality, or pruning.

## Inputs and units

A schedule contains its complete record commitment, protocol version, network, shard, settlement asset, unit, decimal scale, resource weights, and actual price.
Generic funding controls contain an execution limit, a funding price ceiling, all required owner ceilings, and explicit permitted schedules.
The offered-price refinement separately checks the deploy's `phloLimit` and `phloPrice` against those controls.
Schedule membership compares the complete record, not only its numerical price.
Each schedule entry represents exact consent to those terms.

The reference model uses natural numbers and lists without a fixed owner-count limit.
The protocol environment and machine maximum are explicit checker inputs.
The environment fixes the active version, network, shard, native settlement asset, unit, and decimal scale.
The selected schedule must match all six fields.
Native code must authenticate those inputs and apply the existing protocol size limits.
The model does not establish signature validity or digest collision resistance.

A resource key retains its location, class, acquisition terms, and authority signature.
Each class selects a nonnegative weight from the captured schedule.
The checker rejects a required class outside the weight table.
The zero default in the total valuation function cannot authorize an unknown class.

Let $`a(r)`$ denote the authority-unit count of resource occurrence $`r`$.
Let $`w_j`$ denote the phlo weight for class $`j`$.
Let $`p`$ denote the captured price in smallest settlement-asset units per phlo.
The valuation of required resources $`R`$ is:

```math
U(R)=\sum_{r\in R} w_{\operatorname{class}(r)}a(r).
```

Compound authority values add their component values.
Repeated authority occurrences remain repeated charges in this valuation.
Regrouping the same authority resources does not change their value.
Adding a wallet does not itself add a resource occurrence.

This model receives typed resource demand, not raw interpreter measurements.
Native refinement must establish the mapping from compute, introduced bytes, transferred bytes, and trace bytes to those resources.
The resource keys preserve authority and location distinctions even when their scalar values match.

## Admission and prepaid credit

Let $`L`$ denote the signed limit and $`P`$ the signed price ceiling.
Let $`B`$ denote a proposed sufficient resource bound.
Let $`C`$ be the nonempty list of required owner ceilings in the same units.
Control admission requires:

```math
B\le L,\qquad p\le P,\qquad
\forall c\in C:\ p\le c,\qquad LP+1\le\operatorname{machineMax}.
```

The fixed fee is one unit of the approved native settlement denomination, separate from the resource limit.
It is not a configurable fee in this model.
Explicit signed consent cannot change the native fee denomination.
The complete schedule must belong to the signed permitted list and match the protocol environment.
A lower price does not authorize an otherwise unapproved schedule.

Control admission alone does not prove that $`B`$ is sufficient.
The execution checker also verifies that the supplied resource demand fits $`B`$.
An up-front planner must prove this relation for every permitted execution in its certified scope.
A check against one completed trace is not that universal sufficiency proof.

Typed prepaid discharge partitions available resources into used and unused occurrences.
It partitions required resources into used prepaid occurrences and newly acquired occurrences.
It also rejects new acquisition when a matching unused prepaid occurrence remains.
Location, authority, class, acquisition terms, and multiplicity remain part of that comparison.

Let $`U_{prepaid}`$ and $`U_{new}`$ denote the two parts of valued consumption.
The checked relation and retained monetary bound are:

```math
U(R)=U_{prepaid}+U_{new}\le B\le L,
\qquad
M=pU_{new}+1\le pB+1\le PL+1.
```

Prepaid resources reduce new acquisition, not measured execution use.
A fully prepaid execution must still fit `phloLimit`.
It retains only the separate fee when no new resource acquisition is necessary.
Native acquisition provenance must justify prepaid compatibility before this checker can consume that evidence.

### Native numeric checker

[`check_numeric_phlo_bounds`](../../../../rholang/src/rust/interpreter/accounting/phlo_bounds.rs) implements the numeric part of control admission with checked `u64` arithmetic.
It returns the signed charge ceiling and the tighter schedule charge bound.
These values include the separate one-unit fee. Neither value defines aggregate temporary exposure across restricted execution outcomes.

The checker requires a nonempty list of owner ceilings and checks every entry.
It does not divide, average, or use those ceilings as allocation weights.
Repeated consents cannot multiply the bill or override a lower ceiling.
The checker allocates no memory and performs one linear scan of the supplied owner list.
Input decoding must enforce the independently configured owner-count and byte limits before this scan.

The deterministic rejection order is:

1. Reject a missing owner consent list.
2. Reject a resource bound above the signed limit.
3. Reject an actual price above the signed price ceiling.
4. Reject an actual price above any required owner ceiling.
5. Reject overflow in the signed charge ceiling, including its separate fee.
6. Reject a signed charge ceiling above the supplied machine maximum.

The checker then calculates the schedule bound with checked multiplication and addition.
The admitted inequalities prove that this result cannot exceed the signed ceiling.
The API retains an overflow error for both calculations instead of using wrapping arithmetic or an unchecked assertion.
A smaller actual price does not excuse overflow in the original signed ceiling.

For example, limit ten, ceiling three, actual price two, and resource bound ten produce ceilings of 31 and 21 units.
Adding required owners with compatible ceilings does not change either result.
An owner ceiling of one rejects that actual price, regardless of the other owners' balances.

`numeric_phlo_bounds_exact` proves that the numeric predicate equals the stated inequalities for arbitrary finite owner lists.
`admitted_controls_pass_numeric_bounds` derives this predicate from the complete modeled admission check.
`numeric_phlo_bounds_preserve_checked_charge` proves that the schedule bound fits the signed ceiling and machine maximum.
Native tests compare checked arithmetic against a separate `u128` oracle.
They cover exhaustive small inputs, full-width generated inputs, positive generated admissions, every owner position, and unchanged charges under owner reordering.

This checker does not authenticate owners, schedules, resource bounds, or sufficient backing.
Its result type is `NumericPhloBounds`, not an admission certificate or permission to publish settlement.
Numeric acceptance must accompany exact schedule compatibility, typed resource proofs, custody authorization, and the existing checkpoint validation.
Zero prices and limits follow the formal numeric predicate. Separate protocol restrictions must remain separate checks.
The checker changes no wire format, activation rule, runtime meter, or Casper decision.

### Offered-price refinement and chain authority

`admit_offered_phlo_controls` refines the existing ceiling predicate without changing its meaning.
Its separate `signed_phlo_offer` contains an execution limit and an exact offered price.
The offered limit must equal the funding limit.
The offered price must equal the selected schedule's actual price.
The original chain-minimum, schedule-permission, owner-consent, and checked-arithmetic conditions still apply.

The native [offered-price checker](../../../../rholang/src/rust/interpreter/accounting/phlo_controls/offered.rs) returns `CheckedPhloOffer` with private fields.
Its `PhloOffer` input uses signed 64-bit integers, matching upstream scalar domains.
The checker rejects negative values before conversion to the natural-number domain of the proofs.
The input type does not itself verify a signature.
Live admission must obtain these scalars from the verified deploy envelope and authenticate the chain minimum.

| Theorem | Native regression obligation |
| --- | --- |
| `offered_phlo_controls_exact` | Compare acceptance against the complete independent predicate. |
| `offered_price_respects_chain_and_all_owners` | Reject a below-minimum price and a violation at every owner-list position. |
| `offered_price_charge_uses_offer_not_ceiling` | Keep the charged price equal to the offer, independently of larger ceilings. |
| `offered_limit_bounds_resources` | Reject work beyond the signed execution limit. |
| `offered_price_mismatch_rejected` | Reject even an explicitly permitted cheaper schedule when it differs from the offer. |
| `offered_limit_mismatch_rejected` | Reject a funding record with a different execution limit. |
| `offered_controls_refine_existing_funding` | Retain existing arithmetic bounds and the separate one-unit fee. |
| `offered_controls_need_owner_consent` | Reject absent consent without limiting the theorem to a fixed owner count. |

The native property tests include generated full-width integers and owner lists with up to 512 entries.
That test bound is not a two-owner restriction or a universal production cap.
The proof quantifies over arbitrary finite lists.
Existing explicit structural limits still apply at the wire boundary.

[`FtProvenance.v`](../../../../formal/rocq/finalized_floor/theories/FtProvenance.v) also models adoption of the three upstream consensus parameters.
These parameters are maximum parent depth, deploy lifespan, and minimum price.
The model uses upstream integer ranges, including the signed 32-bit upper bound for lifespan.
Missing, malformed, or invalid parameter data cannot produce an adopted configuration.
Valid chain data overrides local configuration.
The consumer theorem assumes the same authenticated data, input, and deterministic consumer.
It does not prove that every production caller uses that configuration.

### Native chain-parameter authority

The genesis PoS contract stores all three parameters and exposes `getConsensusParameters`.
Genesis ceremony participants include those parameters when they reconstruct the proposed genesis state.
Running Casper reads the tuple from the authenticated genesis post-state and replaces its local configuration values.
Missing or invalid chain parameters stop adoption. Local configuration is not a fallback.

| Parameter | Accepted range | Running consumer |
| --- | --- | --- |
| Maximum parent depth | 1 through `i32::MAX` | Snapshot construction supplies the adopted depth to the stateless estimator. |
| Deploy lifespan | 1 through `i32::MAX` | Deploy validity uses the adopted shard configuration. |
| Minimum phlo price | 0 through `i64::MAX` | The restored offered-price admission path must use this adopted minimum. |

[`validate_chain_parameter_values`](../../../../casper/src/rust/casper_conf.rs) applies one range predicate at startup, genesis construction, and chain-result decoding.
This prevents construction of a genesis that its own chain reader would reject.
The lifespan upper bound matches the integer domain of deploy-validity calculations.
The garbage collector obtains configuration from running Casper, not a separate startup copy.
It skips collection until Casper becomes available.

[`consensus_parameter_tests.rs`](../../../../casper/src/rust/rholang/consensus_parameter_tests.rs) checks numeric boundaries, malformed results, and generated inputs against an independent range predicate.
[`chain_parameters.rs`](../../../../casper/tests/util/rholang/chain_parameters.rs) checks actual genesis storage, chain queries, and concurrent adoption with different local settings.
Genesis and startup regressions require invalid values to fail before execution.
These tests establish the checked boundaries. They do not prove every behavior of the consensus protocol.

These contracts do not activate the funded format or establish full upstream replay compatibility.
The [offered envelope](signed-phlo-deploy-envelope.md#offered-price-payload) binds the restored scalars through a separate authorization format.
Native offered-price execution integration still needs separate regression evidence.

### Decoded schedule composition

[`check_phlo_controls`](../../../../rholang/src/rust/interpreter/accounting/phlo_controls.rs) combines exact schedule matching, the chain minimum-price constraint, and the numeric checker.
`PhloEnvironment` identifies the protocol version, network, shard, settlement asset, native settlement unit, and decimal scale.
`PhloSchedule` adds the ordered class-weight list, actual price, and complete schedule commitment.
`SignedPhloControls` supplies the limit, price ceiling, required owner ceilings, and explicitly permitted schedule records.

The mandatory `minimum_price` argument supplies the chain policy in the schedule's settlement units per phlo.
The checker rejects an actual schedule price below this minimum, even when every signed ceiling exceeds the minimum.
`CheckedPhloControls` retains the checked minimum alongside the selected schedule and signed terms.
Different captured minimum prices produce different checked-control values, even when both policies permit the same schedule.
The minimum does not replace the schedule price in charge calculations.

For example, minimum three rejects actual price two with owner ceilings `[100, 200, 300]`.
Minimum two permits that price if the remaining checks succeed.
Ten phlo at actual price two still requires 21 units, including the fixed fee.

The reference API has no implicit minimum-price default.
Fixtures that test the original unrestricted numeric predicate pass zero explicitly.
Production admission must obtain this argument from the authoritative on-chain policy, with checked conversion from its stored integer representation.
A peer-supplied minimum or a conflicting node-local setting cannot establish chain authority.
This checker does not read chain state itself.
Native admission must bind that state and use the same policy during replay.

`admit_chain_phlo_controls` composes the original formal predicate with the minimum-price inequality.
`chain_phlo_controls_exact` proves the exact conjunction.
`chain_phlo_price_between_minimum_and_every_ceiling` proves both price bounds for every accepted input.
`below_chain_minimum_rejected_regardless_of_ceiling` proves rejection independently of the signed ceiling.
`zero_chain_minimum_preserves_controls` proves the zero-minimum case retains the original predicate.
`raising_chain_minimum_cannot_add_admissions` proves that a higher minimum cannot admit a previously rejected input.

Native tests compare the full predicate over an exhaustive small domain and generated full-width integers with variable owner lists.
They also check captured-policy identity, unchanged charge arithmetic, and minimum-price monotonicity.

The checker first requires equality with a complete record in the permitted list.
This equality includes every modeled context field, the complete weight list, its order, actual price, and commitment.
Equal prices or a common prefix of weights do not establish equality.
A lower price also needs explicit schedule consent.
Byte identifiers compare by content, not allocation address.

The checker then compares the selected record with the supplied execution environment.
It checks protocol version, network, shard, asset, unit, and decimal scale in that order.
Explicit owner consent cannot replace these protocol-context checks or redefine the native fee denomination.
Only then does the checker apply the numeric limits and owner ceilings.
Schedule and numeric failures return errors without a state change.

`CheckedPhloControls` retains the exact selected schedule, signed controls, resource bound, and numeric result.
Its fields are private and its accessors do not permit mutation.
The retained slices borrow immutable input records. Rust prevents their mutation while those borrows remain in use.
This property does not prove durable snapshot storage, signature validity, or replay authentication.
The caller must bind the records to the appropriate authenticated execution context before economic publication.

`phlo_schedule_check_exact` proves exact schedule membership and equality with the modeled execution environment.
`phlo_controls_factorization` proves that schedule checking and numeric checking together equal the existing complete control-admission predicate.
`admitted_schedule_commitment_has_explicit_consent` and `unconsented_schedule_commitment_is_rejected` bind admission to permitted record commitments.
`signed_schedule_cannot_change_native_decimal_scale` rejects a different native denomination scale regardless of owner consent.
Native mutation tests cover each modeled context field, weight changes, weight order, shortened and extended lists, and changed prices.
Generated tests cover permitted-list positions, owner counts, numeric rejection, and context mismatch.

`PhloScheduleBinding` connects the [versioned schedule record](#versioned-schedule-record) to these control checks.
It computes the complete record digest, retains an immutable descriptor reference, and projects weights in the original class order.
The resulting `PhloSchedule` retains that digest and the record's native denomination scale.
Measurement, valuation, and compatibility changes therefore require new consent even when numeric weights and prices remain identical.
Decoded records with identical canonical content produce identical bindings despite different storage addresses.

The binding validates record structure and encoding limits, not schedule authority or rule implementation.
The authenticated resolver must still establish supported interpretations, authorized installation, and the applicable captured execution context.
It must not reinterpret the existing byte-schedule digest as this complete economic commitment.
Passing control admission also does not check resource classes, prepaid discharge, or execution sufficiency.
Those obligations remain in the execution and funding checks described below.

### Native typed execution check

[`check_phlo_execution`](../../../../rholang/src/rust/interpreter/accounting/phlo_execution.rs) checks typed resource occurrences against `CheckedPhloControls`.
Each occurrence identifies a location, resource class, acquisition terms, and funding authority.
The witness supplies five lists: available, required, used, unused, and fresh resources.
Repeated keys retain their occurrence counts.

The checker requires two exact multiset partitions:

```math
\mathrm{available}=\mathrm{used}\uplus\mathrm{unused},\qquad
\mathrm{required}=\mathrm{used}\uplus\mathrm{fresh}.
```

Here, multiset union adds occurrence counts rather than discarding duplicates.
No resource key can occur in both the unused and fresh lists.
This condition prevents new acquisition from bypassing compatible prepaid resources.
Compatibility requires equality of all four key fields, not merely equal monetary values.

The checker weights every required occurrence with its selected class weight and authority valuation.
It rejects unknown required classes, arithmetic overflow, and usage above the checked resource bound.
Prepaid resources still count toward execution usage.
An unused resource with an unknown class remains untouched and receives no price.

The candidate acquisition charge equals fresh weighted usage times the selected price, plus the separate unit fee.
For example, five required units with two compatible prepaid units require three new units.
At price two, total execution usage is five and the candidate charge is seven.
Fully prepaid execution retains its execution usage and has only the separate fee.
The outcome rules below determine whether the candidate charge can be retained.

Funding signatures use a flat preorder representation with distinct constructor tags and exact payload bytes.
An explicit work stack visits compound signatures without recursive signature hashing or comparison.
The representation preserves tree structure, including units, grouping, and the distinction between ground and quoted authorities.
It does not perform an implicit split, join, or ownership transfer.
Capability formulas are not payable resources and cause rejection.

`flattened_authority_is_injective` proves that equal flat representations imply equal signature trees in the model.
`flatten_authority_exact_node_count` proves the representation size.
`flattened_authority_preserves_valuation` proves that flattening preserves authority valuation.
Generated native tests compare structural equality, node counts, and valuation against independent tree calculations.
These model proofs and native tests do not establish verified compilation of Rust into machine code.

The caller supplies limits for resource entries, authority nodes, and key bytes.
The limits apply across all five witness lists, including repeated references.
They bound validation work, not the number of lifetime owners or ownership transfers.
Repeated witness references consume validation capacity but do not multiply the economic charge.
These limits do not select protocol constants or replace decoder input limits.

The result retains immutable controls, the witness, weighted usage totals, and the candidate charge.
The checker changes no persistent state and takes no locks.
It does not authenticate backing, select stack heads, acquire new resources, or publish settlement.
The caller must obtain available occurrences from authenticated state and connect them to eligible physical stack entries.
It must also prevent concurrent reuse of those entries through the existing transaction and checkpoint rules.
A multiset partition alone cannot establish ordered stack consumption or atomic economic publication.

## Failure projection

The reference outcome distinguishes admission rejection from accepted execution.
Accepted execution supplies a flat list of classified failures.
An empty list represents success.
Only success or an entirely user-failure list permits the bounded charge for the supplied billable resource demand.

A platform, certificate, or unclassified failure reduces the candidate charge to zero.
This rule applies even when user failures occur in the same list.
Permutation of the list cannot change the retained charge.
Admission rejection has no deployment fee.

The classifier receives failure classes as inputs.
The native `retained_charge` method applies these rules to a checked execution and the supplied outcome.
It does not prove that native error variants receive the correct classes.
Native refinement must flatten nested errors and derive the billable resource demand from authenticated execution evidence.
It must also prove application rollback and atomic publication of economic effects.
A scalar zero-charge result alone does not prove that balances, refunds, allowances, or cursors remain unchanged.

## Proof and test obligations

### Native obligation projection

[`project_phlo_obligations`](../../../../rholang/src/rust/interpreter/accounting/phlo_execution/obligations.rs) derives monetary obligations from a checked execution and a supplied outcome.
Slot zero represents the separate deployment fee.
Each subsequent slot represents one fresh resource occurrence, in witness order.
The result retains the exact resource keys, including zero-value and repeated occurrences.
Prepaid occurrences do not create new acquisition obligations.

For a billable outcome, each resource amount equals its weighted authority value times the selected price.
For a nonbillable outcome, every amount is zero, including the fee.
The keys remain present in both cases.
The projection checks that the sum equals the execution checker's retained charge.

`CheckedPhloObligations::check_assignment` supplies these derived amounts to the existing restricted-funding checker.
Callers cannot replace the amounts with a different list that has the same aggregate charge.
The assignment must cover each slot exactly, respect each capacity, and use only supplied eligible edges.
The checker supports arbitrary source counts within explicit resource limits.
It does not impose two-source funding or multiply the charge by the source count.

The obligation limit includes the fee and every fresh occurrence, even when the amount is zero.
Projection reuses the execution witness's authority-node and key-byte limits for its resource traversal.
It does not meter this repeated validation as new economic resource use.
The private result fields prevent mutation of the checked amounts through this API.

The model's `projected_obligations_sum_to_retained_charge` theorem establishes the charge equation.
Its occurrence and identity theorems require a separate slot for every fresh resource occurrence.
Native tests exercise this contract with generated typed partitions and classified outcomes.
They also reject assignments that move resource charges into the fee or draw through an ineligible edge.

This interface accepts capacities and eligibility as inputs.
It does not prove that those inputs describe authenticated, distinct physical purses or correct resource permissions.
The caller must derive eligibility from each retained key and the authenticated source policy.
It must also enforce signed exposure limits, common captured controls, outcome coverage, refund destinations, and atomic publication.
An accepted assignment establishes feasibility, not lexicographic minimax optimality or a complete funding certificate.

### Restricted funding and refunds

[`SignedPhloFunding.v`](../../../../formal/rocq/cost_accounted_rho/theories/SignedPhloFunding.v) connects the resource checker to the existing restricted-funding assignment model.
It checks a finite family of possible process execution outcomes.
A branch in this model means one process outcome, not a Casper block branch.
The definitions and theorems have no fixed purse-count or outcome-count limit.

Each outcome contains typed resource demand, its prepaid partition, and its failure classification.
Each outcome has an assignment from physical purses to typed monetary obligations.
Slot zero contains the separate fee obligation.
Each following slot contains one fresh resource occurrence, in the supplied resource order.
The resource key preserves its location, class, acquisition terms, and authority signature.
Repeated resource keys retain separate occurrence slots.
This order follows the supplied fresh-resource list. It is not a proof of canonical signing-byte order.
Equal keys share their typed-key permission predicate while retaining separate amounts.
Authenticated occurrence identity requires a separate native binding.

The checker derives every slot's amount from its key, captured schedule, and failure outcome.
It rejects a supplied amount that differs from that derived amount, even when the supplied total is correct.
The derived amounts sum to the retained charge computed by the signed-phlo model.
The obligation count must include every occurrence, including zero-valued occurrences.
Extra slots have zero demand and no funding eligibility.

The funding domain supplies permission for each purse and typed obligation key, not for an untyped slot number.
The checker derives each assignment edge from the key at that slot.
Fee permission cannot replace permission for a resource at another location or under another authority.
Native refinement must authenticate these permissions and derive the resource occurrences from actual execution evidence.
The reference relation alone does not establish either authentication boundary.
Zero monetary flow does not establish the authority required to execute a process or consume a linear resource.

The funding domain contains captured custody identities, available capacities, eligibility, per-purse debit caps, per-purse exposure caps, and a total exposure cap.
All values use the approved native settlement denomination.
The model rejects duplicate physical custody identities rather than counting their capacity twice.
Native normalization must combine aliases before supplying this domain without discarding authority occurrences or required owner consent.

Let $`D_{b,i}`$ denote purse $`i`$'s debit in outcome $`b`$.
Let $`R_i`$ denote that purse's hold across the supplied outcome family $`B`$.
Let $`A_i`$, $`E_i`$, and $`C_i`$ denote its available capacity, signed exposure cap, and signed debit cap.
Let $`E`$ denote the signed total exposure cap.
The checker establishes:

```math
R_i=\max_{b\in B}D_{b,i},\qquad
R_i\le A_i,\qquad R_i\le E_i,\qquad
D_{b,i}\le C_i,\qquad \sum_i R_i\le E.
```

For each selected outcome, the model derives the same total charge bound as the signed-phlo checker.
The refund is $`R_i-D_{b,i}`$ and retains purse $`i`$'s captured custody identity.
Each refund plus its corresponding debit equals the original hold.
The same conservation relation holds across all captured purses.
When the outcome's charge is zero, every source debit is zero and every hold is released in full.

For example, three exclusive outcomes each require one native unit from a different purse.
The maximum retained charge is one unit, but each purse needs a one-unit hold.
The checker accepts a total exposure cap of three and rejects a cap of one.
It does not derive exposure consent from the retained-charge ceiling.

The checker proves feasibility only for the supplied outcome family.
`covered_realized_case_is_checked` requires a separate proof that every permitted execution belongs to that family.
The checker does not discover missing outcomes or prove a supplied family's completeness.
Conservative precharge requires that coverage proof, not only successful family checking.

The supplied assignments must independently meet the approved lexicographic minimax and canonical residual contracts.
This module establishes feasibility and charge conservation, not optimal selection of assignments or reservations.
Captured refund destinations are immutable function inputs here.
This fact does not prove that native storage preserves those inputs across transfers, retries, or concurrent publication.
Native authorization, capture, integer-width checks, atomic publication, and once-only settlement remain separate refinement obligations.

### Native funding-family check

[`check_phlo_funding_family`](../../../../rholang/src/rust/interpreter/accounting/phlo_execution/funding.rs) connects projected obligations to the existing branch-reservation checker.
Each source supplies an immutable custody identity, capacity, exposure cap, and debit cap.
Each case supplies projected obligations, source eligibility, and an assignment matrix.
Every case must retain the same checked controls, including schedule, owner ceilings, and resource bound.

The checker rejects duplicate custody identities by byte content.
It also rejects empty source or case lists, invalid matrix dimensions, and exceeded validation limits.
For each case and source, repeated identical resource keys must have identical eligibility values.
This check preserves the model's key-based permission relation without recursive signature comparison.
Distinct resource locations, acquisition terms, classes, or authority structures remain separate keys.

The assignment checker bounds each source debit by both available capacity and its signed debit cap.
The reservation checker then computes each source's maximum debit across the supplied cases.
It checks every resulting hold against capacity and signed source exposure.
The family checker separately checks their sum against signed total exposure.
Neither an execution charge ceiling nor another source's unused balance replaces these exposure checks.

Each case uses exactly its projected obligation count, including the separate fee and all zero-value occurrences.
The native matrix omits absent trailing slots that the reference model represents with zero demand and zero flow.
It rejects extra columns instead of allowing additional obligations outside the projection.
Case order does not change source holds or maximum charge.

`CheckedPhloFundingFamily` retains the original sources, cases, checked assignments, and holds.
Its refund method pairs each hold remainder with the original source's custody bytes.
It rejects an unknown selected case.
Calling this method constructs refund data and does not execute a transfer or close an operation.

The source count, case count, obligation count, matrix cells, and custody bytes have separate caller-supplied limits.
The matrix-cell limit counts source-obligation pairs across the complete family.
Each pair has one eligibility bit and one amount in the supplied matrices.
The decoder must bound input allocations and schedule records before this checker runs.
Total held exposure uses `u128`, while each source amount and each retained charge use `u64`.
This internal representation does not select a new signed wire format.

The formal hold and refund theorems establish the constraints above for the supplied domain.
`equal_obligation_keys_have_equal_permission` states the duplicate-key permission invariant.
`accepted_family_bounds_each_hold_by_debit_cap` confirms that maximum branch holds also fit the common signed debit cap.
Generated native tests check varying source counts, typed resource assignments, classified outcomes, maximum holds, and per-source refund conservation.
They reduce each capacity, exposure cap, and debit cap below its required value to check rejection.

This checker does not establish valid signatures, canonical custody resolution, or complete outcome coverage.
It does not prevent two independent families from claiming the same live balance.
It also does not prove fairness, persistent capture, ownership-transfer handling, or once-only publication.
Those properties require the authenticated policy, solver, and transaction integrations described below.
The result is checked funding data, not permission to publish economic effects or a substitute for resource discharge.

### Native purse amounts

`CheckedPhloFundingFamily::native_amounts` converts one selected case into nonnegative `i64` amounts for each original custody identity.
The conversion checks every source before it returns the complete result.
An invalid case index or an out-of-range source rejects the conversion without changing the checked family.
Zero-value sources retain their positions and custody identities.

Let $`H_i`$ denote a source hold, $`D_i`$ its selected debit, and $`F_i`$ its fee-slot assignment.
The conversion accepts precisely these amount constraints:

```math
0 \le F_i \le D_i \le H_i \le 2^{63}-1.
```

The acquisition amount is $`D_i-F_i`$, and the refund is $`H_i-D_i`$.
Thus, acquisition plus fee equals the debit, and acquisition plus fee plus refund equals the hold.
Every returned amount fits the native signed integer representation.
The aggregate hold remains `u128` because separate valid purse amounts can sum to more than `i64::MAX`.

For example, one purse can hold `i64::MAX` units while another holds one unit.
Both purse amounts fit individually, although their sum does not fit `i64`.
A single purse hold of `i64::MAX + 1` fails conversion.
No aggregate narrowing can replace the per-purse checks.

`lower_phlo_amounts_acceptance_exact` proves the acceptance conditions above.
`lowered_phlo_amounts_conserve_and_fit` proves conservation and all output bounds.
`checked_family_has_native_amounts` connects admitted funding families to successful conversion when each hold fits the native representation.
Property tests compare full-width inputs against these conditions and generate valid partitions across the signed integer range.

The conversion returns numeric data, not a custody capability or a settlement receipt.
It does not create a persistent escrow or a second spendable balance.
Resource acquisition is not automatically a burn instruction.
The transaction integration must resolve authenticated custody and preserve resource backing, atomic publication, and once-only settlement.

### Capture, transfer, and selected-outcome replay

[`SignedPhloCapture.v`](../../../../formal/rocq/cost_accounted_rho/theories/SignedPhloCapture.v) connects checked funding families to the ownership-consent state model.
A capture records the current right generation, signed terms, state root, price, decoded intent, and checked funding snapshot.
An operation identifier pairs that snapshot with its reservation.
The reservation is reference evidence. It does not create an independently spendable escrow account.

The decoder maps signed term data to a typed intent or rejects it.
The intent contains the controls, schedule identity, aggregate exposure cap, and per-custody source consent.
Source consent contains hold caps, debit caps, and permissions for typed obligation keys.
Capture cannot enlarge those caps or permissions.
The required owner ceilings must match the controls decoded from the current right.
The right's schedule identifier must also equal the complete commitment of the selected execution schedule.
Matching the right's identifier with an independently supplied intent identifier is insufficient without that final equality.

The decoder is an explicit parameter, not an assertion that a signature is valid.
The example Boolean lists are abstract payloads, not a proposed wire format.
Native integration must implement canonical decoding and signature checks.
It must bind schedule identities and resource permissions to authenticated data.
The function-valued reference permissions do not specify a native policy encoding.

#### Decoded funding-family consent

[`check_phlo_family_consent`](../../../../rholang/src/rust/interpreter/accounting/phlo_execution/consent.rs) checks the funding-family projection of decoded intent.
`DecodedPhloFamilyIntent` contains exact controls, a total exposure cap, and a finite list of custody-specific policies.
Each policy contains a hold cap, a debit cap, a separate fee permission, and permitted exact resource keys.
Resource permission is set membership, not another supply of spendable resources.
Repeated permission entries do not duplicate backing or remove resource-demand multiplicity.

The check applies these rules before it returns `CheckedPhloFamilyConsent`:

1. Check the controls against the family's captured controls.
2. Check the family's exposure cap against decoded total exposure.
3. Reject duplicate custody policies and missing policies for funding sources.
4. Check every source's declared exposure and debit caps against its decoded policy.
5. Check each permitted source-obligation pair in every supplied execution case against that policy.

The check binds declared caps, not only the smaller amounts used by one selected outcome.
It checks permission even when an assignment is zero or an outcome retains no charge.
Fee permission does not imply resource permission, and resource permission does not imply fee permission.
Matching value alone cannot substitute another location, class, acquisition term, or authority key.

Policy order does not change source matching.
Matching uses custody byte content, while the result retains the family's original source order.
The checker bounds source counts, permission entries, case cells, authority nodes, and key bytes separately.
Resource-key normalization uses the existing iterative authority traversal.
The key-byte budget includes custody from both input lists and resource keys from policies and cases.
The wire decoder must separately bound schedule records and input allocations before this check.

`phlo_family_intent_check_exact` states the reference acceptance conditions.
`decoded_intent_checks_funding_family` proves that the complete reference intent check implies this funding-family projection.
`family_intent_bounds_every_source` proves both source caps and the permission implication for every supplied case and key.
Native tests cover absent and duplicate custody, changed controls, zero-value permissions, exact work limits, and generated multi-purse policies.

This check does not authenticate an intent or implement the complete capture operation.
Its result cannot replace signature verification, grant validation, schedule-identity binding, or once-only native publication.
It creates no persistent reservation or new spendable balance.

#### Funding-right terms and the execution schedule

`check_phlo_funding_terms` compares funding-right terms with `CheckedPhloControls`.
It requires the complete required-owner ceiling list, settlement asset, and schedule commitment to match the checked execution.
Matching only the minimum ceiling does not establish that every required consent remains present.
Matching only the price does not establish that the resource rules and native denomination remain unchanged.

The required binding is:

```math
\operatorname{scheduleId}(\mathrm{right})
=\operatorname{scheduleId}(\mathrm{intent})
=\operatorname{commitment}(\mathrm{executionSchedule}).
```

`SignedPhloCapture.v` enforces both equalities in `check_decoded_intent`.
`decoded_intent_binds_actual_schedule` proves that accepted intent has this binding.
`different_execution_schedule_cannot_capture_consent` excludes mismatched execution schedules.
`arbitrary_capture_history_binds_actual_schedule` preserves the binding through arbitrary modeled capture, transfer, and settlement histories.

For example, a right and intent can both name schedule 7 while execution selects schedule 1.
The matching names do not authorize that execution.
The regression `mismatched_execution_schedule_rejects_decoded_intent` requires rejection even when the other funding checks pass.
Positive lifecycle examples use consistent selected-schedule commitments.

`CheckedPhloFamilyWireConsent::bind_funding_terms` applies the native check to a complete checked funding family.
Its private result type, `CheckedPhloBoundFamilyConsent`, retains both the original consent and checked funding terms.
It does not change the allocation, captured refund sources, execution bound, or settlement amounts.
Native tests mutate every bit of the expected commitment and separately change asset and owner constraints.
Generated tests cover required-owner cohorts through 129 entries and repeat the check after source-policy wire decoding.

`PhloFundingTerms` contains comparison inputs, not proof of authority.
The caller must resolve those terms from authenticated rights and validate signatures, capabilities, grant versions, and causal context.
It must derive the selected schedule through the complete schedule binding and authenticated activation rules.
Supplying mutually consistent but unauthenticated values does not authorize spending.
The complete enclosing economic-intent wire contract and production capture integration remain separate requirements.

#### Exact resource-key wire binding

[`PhloResourceKeyV1`](../../../../models/src/rust/phlo_resource.rs) preserves location, resource class, acquisition terms, and the complete supported authority tree.
The [wire schema](signed-phlo-contract-proposal.md#resource-permission-key-version-1) defines every field and node tag.
The interpreter constructs this record through its existing bounded resource-key traversal.
Native permission comparison and wire projection therefore use the same node representation and operator support.

[`SignedPhloResource.v`](../../../../formal/rocq/cost_accounted_rho/theories/SignedPhloResource.v) proves these encoding properties for arbitrary finite records and trees:

| Property | Formal result | Native verification |
| --- | --- | --- |
| Node identity | `phlo_node_encoding_is_injective` separates tags and exact identity payloads. | Tag, payload, width, and mutation tests. |
| Complete authority identity | `phlo_authority_encoding_preserves_every_node` retains node order, count, and repetitions. | Independent byte encoder and generated full-tree roundtrips. |
| Complete key identity | `phlo_resource_encoding_binds_every_component` retains all four semantic fields. | Field substitutions and native-key equality comparison. |
| Arbitrary tree shape | `phlo_tree_scan_consumes_exactly_one_slot` and `phlo_tree_encoding_has_complete_shape` establish complete tree acceptance. | Independent recursive shape oracle over every node sequence through length eight. |
| No additional roots | `phlo_complete_authority_has_no_accepted_extension` rejects any extension of a complete authority. | Malformed shape and nested trailing-data tests. |

The framing proofs require representable field lengths and configured field bounds.
Native checks additionally enforce fixed widths, valid tags, total bytes, node limits, and complete nested consumption.
The shape oracle covers 87,381 node sequences. Generated tests cover 256 additional trees and full-width class indices.
Separate tests exercise up to 4,096 leaf occurrences without recursive decoding.
Interpreter tests compare native resource identity with encoded identity and preserve existing rejection and work-limit behavior.

These functions are pure and publish no shared state.
Their tests do not replace concurrent reservation, transfer, settlement, and replay tests at the stateful integration boundaries.
The proofs establish encoding properties, not authenticated custody, sufficient funding, or correctness of every Rust compiler transformation.
The signed intent must bind the complete schedule and source authorization before these keys can authorize any funding operation.

#### Source-policy wire binding

[`PhloSourcePolicyV1`](../../../../models/src/rust/phlo_source.rs) binds custody, hold cap, debit cap, fee permission, and the complete resource-permission set.
The [source-policy schema](signed-phlo-contract-proposal.md#source-policy-record-version-1) specifies its canonical field order and limits.
The type keeps its fields private. Its constructor validates and normalizes permissions before returning the record.
Its decoder accepts only strictly ordered canonical keys and complete nested records.

[`SignedPhloSource.v`](../../../../formal/rocq/cost_accounted_rho/theories/SignedPhloSource.v) establishes these properties:

| Property | Formal result | Corresponding native check |
| --- | --- | --- |
| Exact source consent | `phlo_source_encoding_binds_every_consent_field` proves field-preserving encoding injectivity under explicit width and length bounds. | Mutations of custody, both caps, fee consent, and permission contents change the bytes. |
| Exact permission keys | `phlo_permission_encoding_preserves_every_key` preserves the complete ordered resource records. | Canonical decoding rejects duplicate or descending key bytes and malformed nested keys. |
| Permission normalization | `phlo_permission_normalization_preserves_authorization` proves that permutation followed by duplicate removal preserves set membership. | Generated native and codec tests compare reversed and repeated inputs with an independent set oracle. |
| Permission repetition | `phlo_permission_repetition_does_not_expand_authorization` proves that an existing permission adds no new member. | Duplicate permission records normalize without changing caps or logical occurrences inside each key. |
| Empty resource set | `phlo_empty_resource_permissions_authorize_no_resources` excludes every resource when the set is empty. | Empty-set tests retain explicit fee consent and independent zero or full-width caps. |

The normalization proof uses an explicit permutation premise and checked duplicate removal.
It does not prove the implementation of Rust sorting or replace native canonical-order tests.
The native projection shares one bounded traversal budget across all submitted permissions.
Generated tests also compare its exact resource-key set with the original native consent.

#### Decoded source policies in funding-family validation

`check_phlo_family_wire_consent` checks decoded source-policy records against an already checked funding family.
It returns `CheckedPhloFamilyWireConsent` only after the complete family satisfies the same rules as native source consent.
The native and wire entry points use one validator for controls, aggregate exposure, physical source lookup, caps, and permission coverage.
Only resource-key normalization differs between the two input representations.

The wire path preserves the complete flat authority nodes without reconstructing a recursive signature tree.
It applies one aggregate byte and authority-node budget across all policies and every checked outcome.
Duplicate custody identities reject the family. They cannot create additional funding positions or overwrite earlier consent.
Every source's declared hold and debit caps must fit its decoded consent, including sources whose selected debit is zero.
Every eligible resource or fee obligation must have permission in every outcome, including failed and zero-charge outcomes.

The checked result retains the original family and decoded intent by immutable reference.
Validation does not change assignments, source holds, refund destinations, or the selected branch.
It does not replace the separate minimax allocation and canonical residual requirements.

`SignedPhloCapture.v` proves that equivalent source maps preserve each source check and the complete family decision.
The premise requires identical custody lookup, source caps, and permission membership.
Family equivalence also requires identical signed controls and aggregate exposure.
These are semantic equivalence results, not permission to skip wire parsing, authentication, or validation-work limits.

The native tests encode each source policy, decode the bytes, and compare both validator decisions.
Example tests cover every source cap, missing or duplicate custody, changed controls, exact key substitutions, fee permission, and each work-limit boundary.
Multiple-outcome tests reverse case order and retain permission checks for later failed outcomes.
Generated tests cover one through 129 funding sources, valid and invalid consent, repeated permissions, and policy permutations.
Each accepted result retains the original native settlement amounts for every supplied outcome.

Canonical permission deduplication can reduce subsequent validation work.
Thus native and wire inputs need sufficient respective work budgets before semantic decision equivalence applies.
Duplicate constructor inputs still consume the source codec's pre-normalization work budget.

The outer signed controls and aggregate exposure remain typed inputs to this entry point.
It does not yet decode or authenticate the complete enclosing economic intent.
The enclosing protocol must bind those values and every selected source record before the result can authorize a debit.
The [funding-intent record](#versioned-funding-intent-record) combines these values into one bounded encoding and supplies a composed native check.

Parsing and projection publish no state and perform no funding operation.
Signature verification, authenticated source resolution, aggregate exposure, schedule binding, and atomic settlement remain separate integration obligations.

#### Canonical wire primitives

[`PhloWireEncoder` and `PhloWireDecoder`](../../../../models/src/rust/phlo_wire.rs) provide bounded primitives for canonical signed-field encoding.
Unsigned integers use fixed-width big-endian bytes at widths of one, two, four, eight, or sixteen bytes.
A variable byte field uses an eight-byte unsigned length followed by exactly that many payload bytes.
An empty field therefore contains eight zero bytes, not an absent field.
Sixteen-byte integers support aggregate exposure without narrowing it to one source's native amount.

For a payload of two bytes, `00 ff`, the complete variable field is:

```text
00 00 00 00 00 00 00 02 00 ff
```

The encoder checks the complete field size before it appends the length or payload.
Exceeded limits or allocation failure preserve the existing encoded bytes.
The decoder borrows input slices and does not allocate from an untrusted declared length.
A truncated or oversized field leaves the read position unchanged.
Successful reads consume exactly the encoded field and retain the remaining suffix.

`PhloWireLimits` separates total encoded bytes from each variable field's payload bytes.
The total limit includes integer fields and length headers.
These are logical byte limits, not a guarantee about allocator metadata or total process memory.
Domain decoders must additionally bound record counts, resource structures, and validation work.

Use these rules when a domain decoder consumes the primitives:

1. Select the schema from the authenticated format version and context.
2. Enforce its field order, required fields, record limits, and semantic constraints.
3. Reject malformed or unsupported values without publishing partial results.
4. Call `finish` after the final expected field to reject trailing bytes.

[`SignedPhloWire.v`](../../../../formal/rocq/cost_accounted_rho/theories/SignedPhloWire.v) proves integer and field roundtrips, exact prefix reconstruction, and bounded append behavior.
It reuses the existing byte-integer conversion lemmas without changing state-import behavior.
Its sequence theorems preserve exact field counts and boundaries across arbitrary finite lists.
The complete-decoding theorem excludes an unconsumed suffix.
The model uses natural-number byte values, while Rust's `u8` type enforces byte range directly.
Native tests cover full-width integers, framing vectors, field partitions, truncation, and unchanged state after rejected operations.

These primitives do not select protobuf tags, authenticate signatures, or define the complete economic intent schema.
They do not change historical deploy bytes or activate the new economy.
Schedule commitments, source-policy encoding, deploy identity, and client migration must use a complete versioned contract before production integration.

#### Versioned schedule record

[`PhloScheduleV1`](../../../../models/src/rust/phlo_schedule.rs) implements the [schedule record format](signed-phlo-contract-proposal.md#schedule-record-version-1).
Its canonical encoding contains every field required to identify the schedule's denomination, resource interpretation, prices, and compatibility rule.
Resource-class order remains significant because the execution projection uses positional class indices.
Its digest method hashes the complete canonical record with Blake2b-256, including the format domain.

The decoder rejects unsupported domains, invalid field widths, empty identities, duplicate classes, invalid counts, and fees other than one.
It rejects trailing bytes at the schedule, class-list, and individual-class levels.
The decoder borrows identity bytes from the bounded input and copies fixed-width rule commitments.
It adds class storage only after it has parsed a complete class record.
An excessive declared count cannot cause a count-sized allocation before parsing.

`PhloScheduleLimits` bounds total record bytes, field payload bytes, and class count separately.
Nested class-list and class encoders also fit within the enclosing field-byte limit.
These limits describe record structure and work, not authorization to activate a tariff.
Admission must resolve rule commitments, match the authenticated denomination and context, and enforce the selected activation policy.

[`SignedPhloSchedule.v`](../../../../formal/rocq/cost_accounted_rho/theories/SignedPhloSchedule.v) proves that canonical schedule encoding preserves every record component.
The proof includes class identities, units, measurement and valuation commitments, weights, order, protocol, network, shard, denomination, scale, price, and compatibility.
It also proves separation between format domains.
The proof depends on explicit integer-width and field-size bounds.
The underlying field-list theorem permits arbitrary finite list lengths, not only equally sized lists.

The encoding proofs establish distinct preimages for distinct valid record values.
They do not prove hash collision freedom or verified compilation of the Rust decoder.
Native regression tests compare complete records with an independent encoder and mutate every committed component.
Generated tests cover full-width integers, multiple classes, malformed framing, and exact re-encoding of accepted mutations.
The record codec is not yet the complete signed-intent or admission integration.

#### Versioned control record

[`PhloControlsV1`](../../../../models/src/rust/phlo_controls.rs) encodes the limit, price ceiling, complete required-owner ceiling list, and permitted schedules.
It does not replace source policies or constitute the complete economic intent.
The enclosing signed record must bind these controls together with source consent, aggregate exposure, and authenticated funding-right provenance.

The record contains five length-prefixed fields in this order:

| Position | Field | Payload |
| --- | --- | --- |
| 0 | Format domain | ASCII `f1r3node:phlo-controls:v1`. |
| 1 | `phloLimit` | Eight-byte unsigned big-endian integer. |
| 2 | Funding price ceiling | Eight-byte unsigned big-endian integer. This field is not the deploy's offered `phloPrice`. |
| 3 | Required-owner ceilings | A framed four-byte unsigned count, followed by that many framed eight-byte unsigned ceilings. |
| 4 | Permitted schedules | A framed four-byte unsigned count, followed by that many framed complete `PhloScheduleV1` records. |

Every frame uses the eight-byte length prefix defined by the [wire primitives](#canonical-wire-primitives).
Both lists preserve their exact order and duplicate entries.
The codec does not sort owners, remove repeated consent constraints, or reduce the owner list to its minimum.
It does not sort or merge schedule alternatives.
Source authorization must determine the required owner identities before their ceiling list can authorize execution.

For example, `[3, 5, 8]` and `[3, 9, 8]` have the same effective price ceiling but different encoded consent records.
Removing one ceiling also changes the record, even when the removed ceiling repeats another value.
This representation preserves the existing exact control-equality check.
It does not create new owners or charge once per ceiling entry.

`PhloControlsLimits` bounds total bytes, field bytes, owner entries, schedule entries, and aggregate resource classes across all schedules.
The aggregate class limit does not reset for each schedule.
The decoder adds storage only after it parses the corresponding ceiling or complete schedule.
An untrusted count cannot trigger a count-sized allocation before parsing.
The decoder rejects malformed widths, excessive counts, truncated fields, and trailing bytes at every list boundary.

Zero and full-width integer values remain structurally representable.
Empty owner and schedule lists also remain structurally representable.
Structural decoding does not establish consent or numeric admission.
The existing admission checks reject missing required-owner consent, an unpermitted selected schedule, insufficient price ceilings, and arithmetic overflow.

[`PhloControlsBinding`](../../../../rholang/src/rust/interpreter/accounting/phlo_controls/wire.rs) validates the complete record before it constructs native schedule bindings.
Each schedule binding computes its commitment from the full canonical schedule and retains its ordered resource weights.
`PhloControlsView::terms` exposes those schedules and the original limits and owner ceilings to the native admission checker.
The view borrows immutable records and publishes no funding state.
It neither authenticates the controls nor selects a schedule authority.

[`SignedPhloControlsWire.v`](../../../../formal/rocq/cost_accounted_rho/theories/SignedPhloControlsWire.v) proves injectivity of the bounded control encoding and separation of format domains.
Its generic list theorem preserves every entry, including order, multiplicity, and nested schedule fields.
The theorem applies to arbitrary finite lists whose encoded fields satisfy the stated bounds.
It proves encoding identity, not signature security or automatic extraction of the Rust decoder.

Regression tests compare encoded records with independently framed reference bytes.
Generated tests check roundtrips, individual consent mutations, accepted-byte reconstruction, and native admission equivalence after decoding.
Boundary examples include 4,096 owner entries, full-width integers, forged maximum counts, every truncation position, and exact aggregate limits.
The native tests include valid and invalid price consent and verify the actual schedule commitment against funding-right terms.
The family composition test decodes controls and source policies before it checks resource obligations, funding consent, right terms, and native settlement amounts.
Its cohorts include one, two, three, four, 64, 65, and 129 purses.
Each cohort funds the same three resource units and one fee unit without multiplying the charge by owner count.
The modeled platform-failure outcome releases each original hold without changing its custody identity.
The decoded-record checks construct settlement amounts but do not execute SystemVault publication.
The signed-envelope composition additionally verifies envelope signatures before it constructs a checked funding policy.
Envelope signatures do not establish authority over a purse without authenticated funding-right resolution.
These immutable codecs introduce no shared mutable state or concurrency transition.

#### Versioned funding-intent record

[`PhloFundingIntentV1`](../../../../models/src/rust/phlo_intent.rs) combines control and source records with the selected schedule commitment and aggregate exposure cap.
This funding record corresponds to the decoded intent fields in `SignedPhloCapture.v`.
It is not the complete signed deploy envelope or an independently authenticated purse capability.

The record contains five length-prefixed fields:

| Position | Field | Payload |
| --- | --- | --- |
| 0 | Format domain | ASCII `f1r3node:phlo-funding-intent:v1`. |
| 1 | Controls | The complete `PhloControlsV1` record. |
| 2 | Selected schedule | The 32-byte commitment of the selected complete schedule record. |
| 3 | Total exposure | A sixteen-byte unsigned big-endian integer. |
| 4 | Source policies | A framed four-byte unsigned count, followed by that many framed complete `PhloSourcePolicyV1` records. |

Each frame uses an eight-byte unsigned big-endian payload length.
The source list preserves its order.
The codec rejects repeated physical custody identities, even when the repeated policies contain different caps.
It does not combine their permissions or add their monetary limits.
Each source retains the resource-permission normalization rules from its versioned record.

`PhloFundingIntentLimits` applies one permission-entry budget and one authority-node budget across all source policies.
Those budgets do not reset for each source.
The controls retain their separate owner, schedule, and aggregate class limits.
Their encoded byte limits cannot exceed the enclosing field limit.
The source count has an independent limit.

The decoder first validates the outer field boundaries and exact integer widths.
It then decodes the controls and complete source records within their cumulative limits.
It allocates source storage incrementally after each source has passed decoding.
Malformed counts, truncated records, duplicate custody identities, and trailing bytes reject the record without publishing state.
Empty source lists and full-width exposure values remain structurally representable but do not bypass funding admission.

[`PhloFundingIntentBinding`](../../../../rholang/src/rust/interpreter/accounting/phlo_execution/intent.rs) checks record structure and derives the native control view.
Its `check_family` method composes three requirements:

1. Match the complete controls and check all source caps, permissions, and aggregate exposure against the checked funding family.
2. Match the intent's selected schedule commitment with the family's actual execution schedule commitment.
3. Match the resolved funding-right ceilings, asset, and schedule commitment with the checked execution.

The returned `CheckedPhloFundingIntent` retains the original record and checked family.
It cannot replace owner-signature verification, capability validation, conversion authorization, or atomic SystemVault publication.
Callers must obtain funding-right terms from authenticated causal state, not from unverified fields supplied by the requester.

[`SignedPhloIntentWire.v`](../../../../formal/rocq/cost_accounted_rho/theories/SignedPhloIntentWire.v) proves that the bounded encoding preserves every nested intent field.
It also proves format-domain separation.
The encoding proof composes the control, schedule, source, resource, and length-framing proofs.
It does not claim cryptographic collision resistance or an extracted Rust implementation.

`decoded_intent_factorizes_into_family_and_right_checks` proves the corresponding decomposition of the reference capture check.
It requires family consent, exact owner ceilings, asset equality, and equality between right, intent, and actual schedule identities.
This result prevents a weaker native composition that compares only two unverified schedule identifiers.

Tests cover full-width exposure, 4,096 sources, cumulative work limits, duplicate custody, exact field widths, and every truncation position.
Generated tests compare independent field framing, mutate individual source caps, and require exact re-encoding of accepted mutated bytes.
The native composition test consumes a complete encoded intent before it constructs execution obligations and settlement amounts.
It preserves total charges and original-custody refunds across cohorts through 129 purses.
It rejects altered limits, owner ceilings, exposure, source membership, selected schedule, and funding-right asset terms.

This record adds no protobuf tag or historical signing-byte change.
The deploy envelope must separately bind its payload, authority provenance, conversion commitments, and this complete funding record before network activation.
This codec and checker introduce no mutable shared state or Casper protocol transition.

#### Captured-state invariant

`check_offered_phlo_family_intent` refines the decoded-intent and funding-family checks with exact offered-price and offered-limit constraints.
Six theorems establish preservation of previous checks, source caps, source permissions, owner ceilings, chain minimum, usage bounds, and price equality.
The same checked snapshot cannot accept two different offers.
The reference snapshot has one shared controls record for all outcomes.
The native family constructor enforces the corresponding equality across every case before the offered-envelope checker examines that family.

The Rust checker refines existing checked controls instead of selecting a new machine maximum or reconstructing numeric evidence.
Property tests preserve the complete checked value across arbitrary valid limits, prices, resource bounds, minima, and owner counts within their test domains.
These refinement proofs assume authenticated envelope decoding and the supplied chain policy.
They do not establish signature security, chain-policy provenance, or atomic native publication.

`FundingFamilyCapture.v` models an offered policy and case capture that retain the signed payload, offer, and complete original policy.
Three preservation theorems connect these wrappers to the existing case and cursor checks without changing allocation.
The payload is an authenticated input to this model, not a modeled cryptographic verifier.
Rust uses immutable envelope references for this identity and concrete, non-overridable envelope checkers for authorization.

`phlo_capture_valid` requires paired snapshot and reservation records.
Each pair contains a checked funding family and the intent decoded from its original terms.
The reservation price must equal the snapshot schedule price.
For every checked source, the captured hold and selected debit fit the corresponding decoded consent caps.
Capture, ownership transfer, and settlement preserve this invariant through arbitrary finite histories.

Transfers change the live right for future captures.
They cannot change earlier snapshots, refund destinations, prices, or closed-operation flags.
An operation identifier cannot replace an earlier capture, including after settlement.
These proofs impose no fixed transfer-count limit.

Settlement constructs a receipt from the captured snapshot and the selected process outcome.
The receipt contains the operation, original reservation, environment, controls, schedule, outcome, custody debits, and custody refunds.
An expected receipt must match every field before settlement closes the operation.
Closure records the selected outcome and prevents another settlement.

Candidate reconstruction and historical replay have different meanings:

| Operation | Required state | Result |
| --- | --- | --- |
| `reconstruct_phlo_candidate` | A paired capture containing the requested outcome. | A candidate receipt without a state change. |
| `prepare_phlo_receipt` | The same capture, with an open operation. | A candidate eligible for settlement. |
| `replay_phlo_receipt` | A closed operation with the same recorded outcome. | The selected receipt, without another settlement. |

Historical replay rejects every other outcome, even when the original capture contained that alternative.
Arbitrary later capture, transfer, and settlement histories preserve the selected receipt from a valid state.
Read-only replay cannot reopen the operation or authorize a second debit or refund.
Selection correctness starts with a successful settlement and follows its preservation theorems.
The pairing invariant alone does not validate arbitrary imported closure records.

The executable examples capture five charge units across purses with holds of three and two units.
They transfer ownership to different owners with a lower price ceiling before settlement.
The earlier operation still uses its original price and purse identities.
The examples reject a changed schedule, repeated settlement, operation reuse, and replay with a different outcome.
The unsafe-failure alternative returns both original holds when that alternative is the selected outcome.

These are atomic reference transitions, not a proof that native concurrent execution implements them.
Capacity checks remain local to each captured funding family.
The module does not reserve shared physical backing across concurrent operations or prove aggregate solvency across captures.
Native refinement must establish atomic reservation, conflict handling, crash recovery, and once-only economic publication.
The unbounded history proofs do not prescribe unbounded retention of runtime snapshots.
Storage retention and replay evidence availability require separate implementation contracts.

### Coverage boundaries

| Boundary | Reference result | Additional integration obligation |
| --- | --- | --- |
| Signed admission | `admitted_phlo_controls_exact` characterizes every control check. | Bind the complete controls to canonical signing bytes and authenticated policy. |
| Owner count | `any_positive_owner_count_can_authorize_the_same_bound` constructs admission for every positive count. | Preserve every required consent through custody alias removal and funding selection. |
| Price and arithmetic | Accepted charges fit the captured bound, signed ceiling, and supplied machine maximum. | Reject overflow during native intermediate arithmetic before mutation. |
| Resource valuation | Compound values are additive and list permutation preserves value. | Derive each resource occurrence from actual interpreter events. |
| Prepaid consumption | Exact typed discharge preserves total usage while reducing new acquisition. | Authenticate backing, acquisition compatibility, and refund provenance. |
| Obligation identity | Each fee or resource slot has its derived amount and exact-key funding permission. | Authenticate the permission relation and bind native resource events to their obligation keys. |
| Failure order | Any unsafe failure prevents the scalar candidate charge, independent of list order. | Classify all native errors and prove the retained-state projection. |
| Capture and consent | An invariant binds checked snapshots, decoded controls, source consent, reservation records, and prices across arbitrary finite histories. | Authenticate signed payloads and policy, then prove native state transitions implement capture atomically. |
| Replay | Historical replay preserves the selected receipt and rejects alternative outcomes after closure. | Reconstruct authenticated capture and selection records from native accepted evidence. |
| Reservation exposure | Every supplied outcome fits per-source debit and hold caps, physical capacity, and total exposure consent. | Prove outcome-family completeness, authenticated source terms, and native checked arithmetic. |
| Refunds | Per-source and total conservation preserve captured custody through reference ownership transfers. Zero charge releases every hold. | Prove native capture, shared backing, once-only settlement, and atomic publication. |
| Fair allocation | The checker derives eligibility from typed permissions and preserves each obligation amount. | Prove lexicographic minimax selection and canonical residuals over the complete feasible domain. |

The executable regression family checks 1,274 combinations of owner count, resource demand, and prepaid prefix length.
Owner counts include zero, one, two, three, 64, 65, and 256.
These finite cases supplement the universal proofs. They do not establish native support beyond configured protocol limits.
Negative cases cover an unapproved lower-price schedule, arithmetic overflow, prepaid limit exhaustion, and an unknown resource class.
An additional regression rejects a nonnative asset even when the signed permitted list expressly includes that schedule.
Zero weights, zero prices, and unit-authority values remain arithmetic cases, not approval of free native tariffs or omitted byte charges.

Restricted-family regressions use one, two, three, four, eight, 16, and 65 purses with separate exclusive outcomes.
For each size, sufficient aggregate exposure succeeds and one less unit fails.
Additional cases reject duplicate custody, each missing source cap, ineligible assignments, and supplied obligations that differ from the measured charge.
One accepted family combines positive resource demand, partial prepaid credit, a separate fee obligation, and an unsafe-failure alternative.
Its successful outcome retains five units from holds of three and two units.
Its unsafe-failure outcome retains zero and releases both holds to their original supplied custody identities.
The obligation-identity regression requires amounts of one, two, and four units for one fee and two distinct resources.
It rejects shifting all seven units into the fee slot.
Further cases check independent resource permission, zero-valued occurrence retention, and zero demand in unused slots.

The aggregate proof script imports all three signed-phlo modules and checks their proved declarations for assumptions.
These proof checks are not Rust property tests, Loom tests, or end-to-end node tests.
Those integration checks must compare native behavior with the reference results and preserve the stated boundaries.

## Related contracts

- [Price schedules and denominations](price-schedules-and-denominations.md)
- [Resource units and measurement](resource-units-and-measurement.md)
- [Resource bounds and exhaustion](resource-bounds-and-exhaustion.md)
- [Ownership transfer and funding consent](ownership-transfer-consent.md)
- [Persistent funding allowance](persistent-funding-allowance.md)
- [Lexicographic minimax funding](lexicographic-minimax-funding.md)
