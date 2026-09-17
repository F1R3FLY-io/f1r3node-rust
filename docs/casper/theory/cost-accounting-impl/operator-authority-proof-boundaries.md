# Operator authority proof boundaries

## Purpose

Authority proofs must preserve required resources without inventing operator behavior.
This document separates algebraic proofs, ordered stack consumption, native checks, and unsupported funding operators.
It does not change Casper or add linear operators.

The [rho paper](../../../../../publications/cost-accounting/cost-accounted-rho.tex) specifies join authority conservation and lollipop lowering.
The relevant anchors are section 4.8 and `def:sugar-lollipop`.
The [continued-GSLT paper](../../../../../publications/cost-accounting-as-monad/continued-gslt-cost-v2.tex) distinguishes spatial grouping from temporal stacks.
Its rules `eq:R1` through `eq:R3` preserve located remainders and continuation seals.
Its `rem:dup` requires separate tokens for separate firings, including copies with the same signature.

## Authority grouping and temporal order

A presentation is a list of authority cells.
Each cell contains a list of authority atoms.
An exact cover requires the flattened presentation to equal the demand as a multiset.
The multiset retains repeated atoms, but does not retain presentation order.

A stack is an ordered list of cells.
Consuming a stack removes its head, not any matching cell elsewhere in the stack.
Repeated signatures do not make repeated cells interchangeable with one cell.
Authority regrouping therefore cannot justify reordering a stack or reusing its head.

For example, a stack contains cells keyed to A, then B.
Two firings can consume A followed by B.
A firing that requires B cannot skip A, even when both orders would leave an empty stack.

## Proven presentation and history properties

The [presentation model](../../../../formal/rocq/cost_accounted_rho/theories/AuthorityPresentation.v) separates exact covers from stack transitions.
These components need an explicit correspondence before they establish an entire event's correctness.

| Property | Theorem | Premises and limits |
| --- | --- | --- |
| Each atom retains its multiplicity. | `exact_cover_preserves_each_atom` | Requires an exact cover and decidable atom equality. It does not prove native atom extraction. |
| Arbitrary finite exact covers compose. | `exact_covers_compose` | Each demand must already match its corresponding presentation. |
| Fixed-participant rounds consume exact suffixes. | `stack_rounds_preserve_exact_suffix` | Every round consumes one cell from every supplied stack. |
| Fixed-participant capacity is necessary and sufficient. | `stack_rounds_succeed_iff_capacity` | Every supplied stack needs at least one cell per round. An empty participant vector consumes no cells. |
| Two successful round sequences compose. | `stack_rounds_compose` | The second sequence starts from the first sequence's residual inventory. |
| Repeated cells require separate consumption. | `repeated_cells_are_consumed_separately` | Each round removes one cell, even when all cells have the same signature. |
| Selected stacks retain exact ordered suffixes. | `selected_history_preserves_ordered_suffix` | Each event selects unique stack IDs. Different events can select the same ID repeatedly. |
| Consumed prefixes reconstruct original stacks. | `selected_history_reconstructs_each_stack` | Each original stack equals its exact consumed prefix followed by its remaining suffix. |
| Unselected and absent stacks remain unchanged. | `selected_history_preserves_unselected_stacks`, `selected_history_cannot_create_stacks` | This consumption relation has no stack-creation operation. |
| Exhausted cells cannot fund another selection. | `selected_history_cannot_reuse_exhausted_cells` | Selection counts cannot exceed the initial stack length. |
| Structurally admitted histories can have equal residual stacks. | `admitted_interleavings_preserve_stack_suffixes` | Both histories satisfy the stack relation and select each ID equally often. Event effects need not agree. |

The selected-history model uses partial inventories indexed by natural-number IDs.
It permits different selected subsets for each event and arbitrary finite histories.
Unique IDs apply within one event, not across the complete history.
The model does not impose a global lock or a validator execution order.

The interleaving theorem proves only pointwise equality of final stack suffixes.
It does not prove equal event-to-head assignment, event authority, receipts, balances, or execution effects.
The A/B example shows why equal final suffixes alone cannot authorize an event reorder.

## Operator correspondence

| Component | Existing proof or implementation | Remaining correspondence boundary |
| --- | --- | --- |
| Joins and multiplicative authority | `CAJoinConservation.v` proves syntactic conservation and a separate payable projection. Its complete-clause encoding preserves every receiver and sender contribution. | The payable projection removes unit authority. Native compilation and event collection must supply the same clause contributions without omissions or duplicate region identities. |
| Lollipop lowering | `SyntacticSugar.v` proves translation properties. `CAGradedTransition.v` preserves continuation seals through whole-redex and separately signed reductions. | Native recognition and lowering must construct those authority layers. These proofs do not establish authenticated activation, captured funding, or settlement. |
| Additive and exponential formulas | `LinearLogicResources.v` defines abstract `Plus`, `With`, `Bang`, and `WhyNot` formulas. | Their abstract theorems do not establish executable funding operators. Native `Sig::is_funding_former` rejects these constructors. |
| Spatial and modal checks | `CAOSLFSpatialModal.v` and native resource checks reason over supplied observations. | These checks do not introduce executable additive or exponential funding operators. |
| Located authority | `LocatedAuthoritySettlement.v` separates location validity from arithmetic settlement. | Debit and replay theorems do not establish location validity without its premises. |
| Persistent funding | `PersistentFundingAllowance.v` proves quantitative transitions under abstract authorization conditions. | Native activation, identity, custody, and repeated firing must establish those conditions. |
| Stack creation and transfer | `OrderedStackMaterialization.v` proves funding conservation and exact committed payloads. `AuthorityPresentation.v` proves ordered consumption. | Stack creation is not a source-prefix move. Ledger commitment, spendable-stack installation, and native publication need explicit correspondence. |

`LinearLogicResources.ll_with_requires_both_branches_available` describes its abstract consumption definition, not a complete native additive-choice transition.
Similarly, `ll_bang_reuse_no_extra_linear_cost` does not establish persistent-process funding.
These distinctions prevent an abstract theorem name from becoming an unsupported implementation claim.

## Logical evidence and payable resources

The dual intuitionistic linear logic (`dill`) judgment separates unrestricted assumptions from a linear context.
A derivation proves logical evidence under those assumptions.
It does not automatically prove that a formula's syntactic projection describes an execution debit.

`LinearLogicResources.v` proves three counterexamples to that automatic interpretation.

| Logical evidence | Linear input | Why projection alone is insufficient |
| --- | --- | --- |
| `With(A, A)` follows from the same input in each alternative. | One occurrence of A. | The existing `ll_consumed_atoms` projection lists two occurrences. Logical alternatives do not mint another resource. |
| `Bang(A)` follows from an unrestricted assumption. | No linear input. | Its projection contains A, but permission does not create funding. |
| `Lolly(A, A)` follows by implication introduction. | No linear input. | Its projection lists both sides. A proof of implication is not an execution or a debit. |

The separate `payable_projection_derivation` judgment admits unit, atoms, tensor composition, and an explicitly selected additive branch.
It proves exact atom preservation, per-atom multiplicity, and agreement with the modeled required-unit count.
Its linear context contains only atoms.
`every_finite_atom_inventory_has_a_payable_projection` supplies a witness for every finite atom list, including empty lists and repeated atoms.
The theorem imposes no two-owner limit.

This judgment implies logical evidence. The converse does not hold.
In particular, a `With` formula has no derivation in this restricted projection judgment.
The committed-branch theorems prove that changing an unchosen branch does not change the selected projection.
They do not validate that unchosen branch or authenticate the branch selection.

This mathematical distinction does not introduce native additive funding.
Native `Sig::is_funding_former` and `sig_to_cost_signature` still reject capability connectives, including those nested inside multiplicative signatures.
The lollipop formula in this table is a capability formula, not the compiled process-transfer syntax described below.

The property `capability_evidence_cannot_become_payable_signature` checks both native boundaries against generated funding trees.
The positive cases preserve an independently generated payable-atom multiset, including repeated ground and quote payloads and unit identities.
The negative cases place each unsupported connective inside generated left and right multiplicative contexts.
These checks establish neither executable additive semantics nor a cryptographic identity theorem.

## Complete join demand

`CAJoinConservation.sig_atoms` counts syntactic leaves, including `SUnit`.
Its strict no-weakening theorem concerns that syntactic count.
Native payable-atom extraction instead removes unit authority.
The separate `payable_sig_atoms` projection models this distinction without changing the existing syntactic theorems.

`payable_atoms_filter_syntactic_units` proves that payable atoms are exactly the nonunit syntactic leaves.
`payable_unit_is_neutral` proves that either unit operand contributes no payable atoms.
These are authority-demand properties, not exemptions from byte charges or other applicable costs.

Each complete join clause contains one receiver signature and one sender signature.
`complete_join_preserves_each_clause` proves exact flattening of this paired representation into payable demand.
`complete_join_preserves_receiver_sender_multisets` separates that demand into all receiver contributions and all sender contributions, preserving multiplicity.
This encoding avoids treating one receiver parameter as automatically representing every receiver clause.

`complete_join_regrouping_preserves_payable_authority` permits clause permutations without changing the payable multiset.
`complete_join_no_payable_weakening` requires the added clause to contain positive payable demand.
An all-unit clause does not satisfy that premise.
`all_unit_join_has_no_payable_authority` and `repeated_join_clauses_retain_multiplicity` cover the neutral and repeated-clause cases explicitly.

These theorems do not change `CAReduction` or establish a new operational join rule.
The paired representation still needs correspondence with compiled receive clauses, collected send events, and region identity.
The ground and quote axes remain distinct formal constructors. This proof does not establish cryptographic encoding or collision resistance.

The paper's `def:join-schema` collects demand per signed receiver or sender block.
Its conservation proposition describes the underlying per-channel authorities.
`signed_block_partitions_preserve_payable_authority` connects these representations when block keys contain exactly their assigned authority occurrences.
`token_partition_preserves_payable_authority` permits arbitrary token groupings of those same occurrences.
Both results require multiset equality between grouped occurrences and the required authorities.

Native compilation must establish this correspondence for actual signed regions.
The synthetic test assigns a distinct region to every supplied surface.
Neither that test nor these conditional theorems authorize repeated billing of one shared region merely because it spans several clauses.

## Lollipop activation and parallelism

The [graded reduction model](../../../../formal/rocq/cost_accounted_rho/theories/CAGradedTransition.v) labels each reduction with its consumed authority.
Its lollipop theorems instantiate the existing whole-redex and separately signed rules.
They preserve the continuation's own seal and the exact remaining token stacks.
They do not replace the continuation seal with the outer or sender signature.

`whole_redex_step_has_exact_grade_and_residual` also characterizes every modeled step from the specified whole-redex shape.
`waiting_lollipop_cannot_force_its_continuation` excludes an internal reduction while the outer input lacks its matching sender.
That theorem permits an arbitrary continuation and token stack in the stated waiting configuration.
It does not claim that all funded configurations are blocked or that native matching is fully verified.

`independent_graded_steps_have_both_orders` composes two independently enabled component steps in either order.
`graded_step_at_any_parallel_position` lifts a component step into any position of an arbitrary finite parallel composition.
These theorems preserve each component's grade and residual.
They do not prove independence for two native candidates that share a physical purse, stack, or matching resource.

The [transfer properties](../../../../rholang/tests/accounting/located_authority_spec/transfer_properties.rs) exercise compiled programs in real in-memory RSpace runtimes.
They use a two-worker Tokio executor and multiple transfer chains with distinct channels.
Repeated payer identities can occur within and across chains.

The staged-arrival property records pending messages independently of the runtime.
After each message delivery, it advances only the contiguous ready prefix of that chain.
It compares native interaction debits with that prefix's exact authority counts.
It also checks that each output appears only after its chain completes.
Thus, queued inner messages cannot justify charges before their outer transfers occur.

Staged deliveries test intermediate causal states. The existing batched tests exercise concurrent ready chains within one evaluation.
The properties do not require authority-event iteration order to equal execution order.
They do not equate interaction charges with storage-introduction or transferred-byte charges.
The tests retain failure seeds at a path rooted in the crate's manifest directory.

These checks do not establish wallet authentication, vault settlement, shared-state publication, or independent-validator replay.
The syntax, reduction, and runtime checks remain separate evidence until a refinement connects their representations.

## Located stack consumption

The [located stack model](../../../../formal/rocq/cost_accounted_rho/theories/LocatedStackConsumption.v) stores a location and an ordered cell list for each stack identity.
An event declares its interaction surface, exact authority demand, and selected stack identities.
Admission requires distinct identities, available heads, matching locations, and an exact authority cover from those heads.
Repeated authority atoms remain valid. Distinct stack identities do not require distinct signatures.

The generic location relation states which purse locations can fund an interaction surface.
The paper specializes this relation through its partial operation $`\operatorname{near}(I,J)`$, where $`I`$ and $`J`$ are the interacting surfaces.
When that operation returns location $`L`$, every selected purse must reside at $`L`$.
`functional_nearness_has_one_result` proves this shared-location requirement from the functional specialization.
The generic relation alone does not impose it.
This distinction preserves the location premise of rules R1–R3 in *Continued GSLT Cost v2*.

Consumption removes each selected head and retains the stored location with the remaining cells.
It does not assume that the final location already equals the initial location.
`located_histories_preserve_exact_suffix` derives both properties for arbitrary finite admitted histories:

```math
\operatorname{final}(i)
  = \bigl(L_i,\operatorname{skipn}(u_i,C_i)\bigr),
\qquad u_i \leq |C_i|.
```

Here, $`i`$ identifies a stack, $`L_i`$ is its initial location, and $`C_i`$ is its initial ordered cell list.
The count $`u_i`$ records how many events selected that stack.
The proof uses the existing ordered-consumption history, rather than a separate scalar balance calculation.
`admitted_selection_has_located_plan` connects each selected identity to the location guard in `LocatedAuthoritySettlement.plan_is_located`.
The plan carries stack identities as occurrence labels. The separate exact-cover premise checks their authority atoms.

| Obligation | Formal result | Native check and boundary |
|---|---|---|
| Matching authority cannot replace matching location | `wrong_location_cannot_fund` | A generated formula rejects supply with the same authority and quantity at another location. |
| Consumption preserves ordered cells and locations | `located_histories_preserve_exact_suffix` | Physical-settlement history tests check ordered cells. The generic formula checker does not contain stack cells. |
| Unrelated locations retain their resources | `non_near_location_is_unchanged` | Generated spends check exact residual supply and demand. |
| Every selected head contributes its exact authority | `admitted_heads_preserve_each_authority` | Physical-settlement properties check exact atom multiplicity separately from formula evaluation. |
| Independent events remain enabled in either order | `independent_admitted_events_have_both_orders` | Generated disjoint positive spends succeed in both orders and produce equal residual observations. |
| Shared cells cannot fund competing snapshot admissions twice | `one_cell_cannot_fund_two_snapshot_requests` | Ordered-settlement exhaustion tests reject reuse. Simultaneous publication requires the separate concurrency checks. |
| Compound and split funding are possible | `one_compound_head_is_admitted`, `two_heads_with_repeated_authority_are_admitted` | Positive witnesses prevent rejection-only proofs from appearing sufficient. |

The independence theorem starts with two requests admitted in the same initial inventory.
It derives preserved admission after either request executes, provided their selected identities are disjoint.
It then constructs both execution histories and proves equal final inventories pointwise.
The model does not serialize independent requests or claim that shared-resource requests are independent.

The [native formula properties](../../../../rholang/src/rust/interpreter/accounting/oslf/property_tests.rs) use composite keys containing a location and an authority.
The checker separates complete keys. It does not interpret their fields as authenticated physical locations.
The multi-location property therefore generates distinct locations explicitly, including repeated authority values.
It checks nonzero spends, exact residual supply and demand, overlapping-footprint rejection, and uncertainty from upper bounds.

An empty selection with empty demand is a structural no-op in the stack relation.
It does not authorize a native zero-grade `Spend`, which the formula checker rejects.
The abstract cell type also permits an empty atom list.
A selected empty cell can therefore be consumed with zero demand in this relation.
Native unit-cell validation excludes that case separately. These proofs do not replace that validation.
Authenticated location extraction, event matching, physical identity uniqueness, and concurrent publication remain separate implementation-refinement obligations.
These theorems do not supply those premises automatically.

## Persistent activation and recorded settlement

The [persistent activation model](../../../../formal/rocq/cost_accounted_rho/theories/PersistentActivation.v) connects located cell consumption to captured allowance reservations.
It models retained billable work, not every tentative runtime event.
The model keeps permission identity, consented allowance, located stacks, per-slot recorded work, aggregate retained charge, and used firing identities.
The immutable service permission identifies the reusable service capability.
Ownership transfer updates funding terms without replacing that service identity or an existing reservation capture.

| Action | Required condition | Effect |
|---|---|---|
| Prepare | Existing consent, price, version, and allowance checks pass. | Append a reservation with zero recorded work. Leave cells unchanged. |
| Fire | A fresh occurrence identifies one open slot. Captured permission, located exact cover, and the remaining limit permit the positive charge. | Consume selected heads and increase recorded work together. |
| Close | The slot remains open. | Settle exactly its recorded work and return only the unused reservation. |
| Transfer | Current transfer authorization passes. | Change future funding terms. Preserve captures, recorded work, and cells. |
| Expand | An explicit authorized amendment passes. | Increase allowance. Do not create cells or erase recorded work. |
| Wallet deposit | The separate wallet operation supplies its authorization. | Leave the modeled allowance and cells unchanged. |
| Reuse permission | The service already has its permission. | Leave funding and cells unchanged. |

Let $`Q`$ denote issued allowance and $`A`$ denote unreserved allowance.
For each open slot $`i`$, let $`L_i`$ denote its reserved limit and $`w_i`$ denote its retained work.
Let $`C`$ denote settled consumption and $`T`$ denote total retained charge.
The invariant requires:

```math
0 \leq w_i \leq L_i,
\qquad
T = C + \sum_{i\text{ open}} w_i,
\qquad
Q = A + \sum_{i\text{ open}}(L_i-w_i) + T.
```

All terms use one compatible allowance unit.
An open reservation still contains its recorded work in the underlying allowance ledger.
The displayed equation subtracts that work from the remaining hold before adding total retained charge.
It therefore does not count active work twice.

`arbitrary_firing_histories_preserve_accounting` proves preservation across arbitrary finite histories containing all modeled actions.
`retained_work_has_initial_and_expansion_bound` bounds total retained charge by initial issuance plus explicit expansions.
`positive_firing_count_is_bounded` also bounds the number of positive firings by the increase in retained charge.
These bounds do not impose a limit of two owners or a fixed number of ownership transfers.

Close does not accept an independently supplied debit.
It reads the slot's recorded work and calls the existing consented settlement transition with that amount.
An aborted execution with retained billable work must use this same settlement rule.
Zero-debit closure is valid only when the slot has no recorded positive work.
The model does not select which tentative effects a failed native execution retains.

The charge function receives the captured reservation and the firing request.
Its required policy assigns zero charge to empty payable demand.
It also preserves charge under presentation permutations that retain the capture, interaction surface, demand multiset, and selected identities.
Two negative theorems reject functions that violate these rules.
The function does not equate atom count with REV, phlo, or physical token quantity.
Native valuation, unit compatibility, and authorization must satisfy this interface through separate refinement proofs.

`funded_repeated_firings_exist` constructs any finite number of unit-charged firings from sufficient matching cells and an open captured allowance.
The witness preserves funding identity and permission, consumes the exact cell prefix, and records the exact charge.
Separate exhaustion theorems reject another firing when either the selected stack or its allowance slot is exhausted.
They distinguish resource sufficiency from authorization capacity.

History proofs preserve every existing capture and purse location.
They also project every firing to the existing ordered-consumption history.
Prepare, close, transfer, expansion, deposit, and permission reuse project to empty selections, so those actions cannot recreate cells.
The state stores occurrence identities and per-slot totals, not a durable event-to-slot receipt ledger.
The transition history provides the abstract event-to-slot association.

The relation has one atomic boundary for retained firing publication.
It does not prove that native workers implement that boundary or impose a global runtime lock.
Production-state Loom checks, publication refinement, failure projection, and independent-validator replay remain required integration evidence.

## Actual reductions and funded events

The [graded stack witness model](../../../../formal/rocq/cost_accounted_rho/theories/GradedStackWitness.v) connects actual calculus reductions to located consumption and recorded funding.
Each witness identifies the exact gate heads and residual token tails consumed by an existing graded rule.
The relation covers all five binary rules, both join rules, and both parallel-context rules.
It includes the rule instances used by lollipop lowering.

`stack_instrumentation_preserves_the_transition_relation` proves correspondence in both directions.
Every existing graded step has a witness, and every witness erases to an existing graded step.
The instrumentation neither adds a reduction rule nor changes a continuation.

The token projection preserves ordered cells and removes unit authority from each cell's payable atom list.
A physical binding maps distinct selected inventory identities to the complete projected input stacks, including their tails.
Equal aggregate demand alone cannot establish this binding.
The syntactic stacks and inventory entries represent the same selected resources, not two supplies that settlement can add together.

| Obligation | Theorem | Required boundary |
|---|---|---|
| The consumed heads cover the exact grade. | `actual_rule_projects_to_exact_ordered_pop` | The witness comes from an existing graded rule. Repeated atoms retain their multiplicity. |
| Split occurrences use distinct physical identities. | `duplicate_physical_identity_rejects_binding`, `physical_binding_preserves_occurrence_count` | The binding preserves occurrence count, not merely signature equality. |
| Syntax and inventory retain corresponding tails. | `logical_and_physical_pop_have_identical_residuals` | A full input binding and an actual selected consumption step determine the output binding. |
| A lawfully funded event can execute. | `construct_funded_program_event` | Admission, captured authorization, charge policy, and sufficient remaining allowance must hold. The theorem constructs a fresh identity and post-state. |
| An event preserves funding validity. | `funded_program_event_preserves_accounting` | The initial funding state must satisfy its accounting invariant. |
| A modal claim concerns the actual target. | `funded_program_event_proves_target_modality` | The target term must satisfy the stated formula. A resource upper bound cannot replace that premise. |

For example, one compound head can cover the same grade as two separate heads.
The separate presentation still requires two distinct inventory identities, even when both heads contain the same authority.
Both forms preserve their own exact tails.
Neither form permits a matching head to conceal a different residual stack.

The native property `compound_and_split_stack_histories_preserve_each_occurrence` compares grouped and separate stack presentations for the same event history.
Generated cases use one through 32 funding sources, repeated authorities, changing ordered cells, and one through eight events.
The test splits each history at a generated position and verifies the remaining events against the exact residual stacks.
It checks unchanged unrelated stacks, zero balance debits, mismatched-head rejection, and duplicate-identity rejection.
These generator bounds are test limits, not limits imposed by the formal history theorems.
The verifier returns pop counts rather than a mutated inventory, so this test does not establish native post-state publication.

A mixed history carries both the current program term and the funding state between events.
Every firing advances the term through its witnessed reduction and advances funding through the corresponding recorded charge.
Preparation, closure, ownership transfer, allowance expansion, wallet deposit, and permission reuse leave the term unchanged in this relation.
The bookkeeping constructor explicitly excludes firings, so it cannot hide an unaccounted program reduction.

`mixed_history_projects_to_actual_graded_history` recovers the program's actual reduction history.
`mixed_history_projects_to_funding_history` recovers the corresponding funding history.
The remaining history theorems preserve accounting validity, captured consent, exact ordered suffixes, and purse locations.
These results cover arbitrary finite admitted histories without a two-owner or two-transfer bound.

The binding is local to each event.
It does not authenticate syntax-occurrence identities across events or establish a whole-program inventory correspondence.
Native compilation, physical custody, valuation, and concurrent publication must establish those implementation boundaries.
The funded-event relation models positive payable charges and excludes zero-payable graded steps.
This exclusion does not remove those steps from the underlying calculus.
The current `CASyntax` representation has no persistent receive constructor, so these theorems do not prove native listener reinstallation.

## Funded stack creation and ordered consumption

The native method `prepare_authority_stack_transfer` prepares a new stack, despite its transfer-oriented name.
The reducer first substitutes each cell and preserves the resulting list order.
It rejects empty stacks and unit cells, then derives the new datum's channel from the head signature.
The complete stack becomes the datum's payload.
No source-prefix selection or destination append operation occurs on this path.

The enclosing authority funds stack creation.
It reserves one copy of its demand for each output cell.
The target cells can contain different signatures from the enclosing funding authority.
Thus, funding conservation must not incorrectly require equal source and target signature multisets.

The [ordered materialization model](../../../../formal/rocq/cost_accounted_rho/theories/OrderedStackMaterialization.v) represents an operation identity, exact post-substitution cells, and enclosing funding occurrences.
A funding occurrence represents an abstract lane at this boundary, not necessarily a cryptographic leaf of a compound signature.
For payer lane $`a`$, output cell count $`n`$, and enclosing multiplicity $`d(a)`$, the reserved charge is:

```math
\operatorname{charge}(a) = n \cdot d(a).
```

`each_materialized_cell_requires_enclosing_funding` connects that product to repeated enclosing demand.
It does not replace the physical settlement verifier or the monetary allocation rule.

The ledger contains arbitrary finite pending and committed operation lists, plus unreserved funding per lane.
Commit and abort can select any pending operation, not only the first operation.
The lists describe bookkeeping and do not impose a runtime scheduling order.
For every lane, preparation, commitment, and abort preserve this sum:

```math
\text{unreserved funding} + \text{pending charges} + \text{committed charges}.
```

`arbitrary_materialization_histories_conserve_each_payer` proves that invariant for arbitrary finite admitted histories.
`materialization_histories_preserve_unique_identities` preserves identity uniqueness from a unique initial ledger.
Preparation rejects duplicate identities, insufficient funding, and empty payloads in this relation.
Commit moves the exact pending birth to the committed list.
Immediate abort restores the original funding pointwise and both operation lists exactly.
`fresh_funded_payload_can_commit` provides a successful two-step history for every nonempty, fresh, sufficiently funded payload.

The model covers the active metered scope. Native unmetered execution bypasses this accounting helper.
Its atomic steps correspond to accounting critical sections, not entire asynchronous RSpace operations.
`StackIntroductionAtomicity.v` separately models produce visibility and deployment rollback.
Unit-cell rejection, signature substitution, channel derivation, machine overflow, and identity hashing remain native correspondence obligations.
The unreserved ledger abstracts the installed allocation minus native reserved demand.
This model does not change wallets to an escrow-based funding scheme.

Spendable inventory installation requires a committed birth and an unused stack identity.
The installation preserves the exact cells and all unrelated stacks.
It cannot overwrite an existing stack or install the same birth twice.
`consumption_after_birth_preserves_ordered_suffix` then connects installation to arbitrary selected-stack consumption histories.
Every consumption removes a head. Repeated signatures retain their separate cell occurrences.

The installation-level no-self-funding control requires the new stack to be absent from the spendable inventory before installation.
The separate causal model represents the native metadata guard directly, as specified below.
Whole-stack transport remains separate from creation and head consumption.

The native property `stack_transfer_reserves_exactly_one_authority_cell_per_output` checks heterogeneous ordered payloads against exact recorded births.
The property `stack_materialization_histories_preserve_payload_and_funding` generates preparation, commit, and abort commands with multiple pending identities.
It checks duplicate and insufficient-funding rejection, exact birth payloads, event counts, reservation totals, and realized charges after each command.
Generated enclosing authorities include repeated payer lanes with distinct region identities.
Generated target signatures differ from those funding lanes.
Each preparation has its own generated enclosing-demand vector.
Initial lane capacities vary independently, so one deficient payer can reject an otherwise funded operation.
The independent oracle sums each pending and committed operation's original charge vector.
Commit and abort must use that captured vector, even when a later preparation uses different authorities.
The test compares entire reserved and realized maps, including the absence of unrelated charges.

The history property uses at most eight operation identities, eight funding regions, and 64 commands per case.
Those are finite test bounds, not limits imposed by the mathematical model.
The test orders individual commands to inspect each state. It does not prove synchronization correctness for simultaneous calls.
Native no-self-funding and ordered-head tests cover the separate physical verifier boundary.
Actual shared-state publication, crash recovery, and independent-validator replay remain separate verification obligations.

## Birth-event causality

The physical verifier receives stack payloads and metadata that identifies each new stack's creation operation.
For each cell, it derives a required creation-event identity and looks up that event's trace position.
It rejects missing events and empty new stacks.
This precheck covers every recorded birth, including stacks that no event selects.
A stack becomes spendable strictly after the maximum required position, not after the first creation event.

The model's `resolve_birth_positions` collects every required position through an abstract lookup.
`birth_completion_position` computes their maximum and rejects an empty result.
`birth_ready_at` compares that maximum with the candidate use position.

`readiness_iff_all_required_events_precede` proves both directions of the readiness condition.
Readiness requires a nonempty event list and a known, strictly earlier position for every required event.
`birth_readiness_ignores_required_event_order` proves that permuting the required list does not change readiness.
`first_event_after_completion_is_ready` establishes availability immediately after completion.
Readiness remains true at later positions when the lookup remains unchanged.

The negative results reject missing events, empty event lists, and use during any required creation event.
These results describe the actual metadata rule. They do not depend on hiding the physical payload from the verifier.

`causal_stack_step` combines ordered head consumption with that readiness guard for every selected new stack.
Existing stacks have no creation metadata in this relation.
`causal_stack_history` advances the trace position after each event and permits arbitrary finite histories.
`causal_histories_preserve_exact_suffix` retains the exact consumed prefix and remaining suffix behavior of the underlying stack model.
`causal_step_rejects_premature_birth_use` excludes use while any required creation event remains current or future.

The selected-use guard alone does not establish the global metadata precheck.
`birth_metadata_well_formed` requires each recorded birth to identify a present stack and one known creation event per cell.
It also requires a completion position, which excludes empty event lists.
`validated_causal_history` combines this initial precheck with the guarded consumption history.
Its rejection theorems cover unknown stacks, missing creation events, and empty payloads even when the stack remains unused.
`validated_histories_preserve_exact_suffix` carries the ordered-consumption result through this combined boundary.

This correspondence covers the metadata predicate and ordered consumption.
It assumes the supplied lookup and required identities describe the same trace as native validation.
It does not prove hash injectivity, event authenticity, complete metadata extraction, or correspondence between accounting commitment and durable publication.
Those obligations remain distinct from the readiness predicate.

The native property `born_stack_readiness_matches_all_creation_events` generates heterogeneous stacks with up to 12 cells and permutes creation events.
It inserts a requested sequence of head consumptions at an arbitrary boundary in those events.
The verifier accepts consumption after all creation events and rejects every earlier insertion.
Positive cases check exact funding debits and stack-pop counts.
The property also checks missing-event and empty-payload rejection.
Separate empty-trace cases confirm rejection of unknown, empty, or incompletely recorded unused new stacks.

A negative control removes only birth metadata from the synthetic inventory.
The same prematurely ordered trace then passes the helper, which confirms that the readiness guard causes the rejection.
This control does not change production metadata or authorize its omission.
The property tests the native verifier, not the allocation search or a full validator replay.

## Whole-stack carrier exchange

The [exchange model](../../../../formal/rocq/cost_accounted_rho/theories/Exchange.v) treats whole-stack transport separately from funded stack creation.
`exchange_preserves_stack_identity_and_order` swaps the two complete carrier payloads without changing either cell sequence.
`exchange_preserves_resource_multiset` preserves their combined cell multiplicity.
`exchange_resource_join_requires_both` requires both carrier inputs before the swap can return an output.

These statements concern abstract carrier payloads.
They do not authenticate a native conversion quote, authorize a withdrawal, or establish atomic RSpace publication.
Whole-stack exchange neither creates cells nor performs the head consumption modeled by a funded firing.

## Native verification requirements

The [native verifier](../../../../rholang/src/rust/interpreter/accounting/authority.rs) combines balance funding with selected stack heads.
It also checks canonical stack-ID order, stack availability, birth-before-use, and exact event authority.
The selected-history model captures only the head-consumption component.

The property `selected_stack_histories_preserve_exact_prefixes` exercises the production `verify_physical_settlement` function.
It generates changing stack subsets, repeated signatures, repeated stack use, and unused stacks.
It compares complete verification with verification split at an arbitrary event boundary, using exact residual cells.
Its negative cases cover exhausted resources, missing resources, and duplicate IDs within an event.
The verifier receives immutable inventories. These tests do not establish atomic publication to shared storage.

The property `complete_join_payable_atoms_preserve_all_surfaces` generates both signatures for each clause, including units and repeated ground atoms.
It compares native compound extraction and event demand with an independent multiset derived from the generated input.
It checks physical settlement, reversed surface presentation, and rejection after removing one required payable atom.
It also generates contiguous token partitions and checks settlement with their compound funding keys, including repeated keys.
These cases exercise canonical signature construction and the production verifier, not compilation or an actual RSpace join.

| Additional obligation | Required verification |
| --- | --- |
| Preserve per-event authority and multiplicity. | Compare combined balance and stack presentations with each event's demand, including repeated atoms and compound authority. |
| Reject noncanonical order. | Supply distinct stack IDs in reversed order, separately from duplicate-ID tests. |
| Authenticate identity and creation. | Check stack provenance, birth-before-use, location, and invalid creation attempts. |
| Preserve independent execution. | Validate both candidate histories independently before comparing their residual stacks and complete effects. |
| Preserve shared-state publication. | Exercise actual synchronization, failed publication, restart, and replay boundaries. |
| Preserve machine arithmetic. | Check full-width counters, checked addition, and malformed encodings. Natural-number proofs do not establish these checks. |

Example tests and property tests complement the unbounded mathematical statements.
Finite generated cases do not exhaust all native execution histories.
Loom checks belong at actual synchronization boundaries, not around this pure stack relation.
