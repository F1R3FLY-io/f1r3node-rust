# Resource units and measurement boundaries

## Purpose

Resource measurement must be deterministic before pricing or cost allocation begins.
This contract distinguishes interaction count, authority-resource demand, byte quantities, monetary obligations, and persistent storage.
It extends the [byte-accounting contract](vault-backed-byte-accounting.md) under the [approved economic policies](economic-activation-policy-ratification.md).

The governing papers require matching authority, located resources, and consumption when an interaction executes.
They do not specify protobuf tariffs, machine-time prices, or a storage-rent schedule.
The native byte tariff is a safety refinement, not a replacement for their authority semantics.
Full MeTTaIL integration and a new rent tariff remain outside the approved scope.

## Measurement domains

| Quantity | Unit and meaning | Measurement boundary |
| --- | --- | --- |
| Interaction count | One unit per billable atomic RSpace COMM identity in a metered scope. | The COMM observer captures the selected match before the corresponding effects can publish. |
| Authority-resource demand | A multiset of required authority occurrences, retaining location and event identity. | Derive from the complete matched authority regions, before physical custody or monetary allocation. |
| Introduction bytes | Canonical encoded produce or consume footprint, including the specified trace-identity overhead. | The produce or consume observer measures the introduction, even when it immediately matches. |
| Transferred bytes | Sum of encoded matched payloads delivered by one COMM. | The COMM observer receives the actual matched payload list. Each committed firing contributes its payload bytes. |
| Trace bytes | Defined consume/produce trace footprint for one COMM's join arity. | Derive from the same COMM identity and channel count as the transfer measurement. |
| Retained storage | State that remains available after the relevant publication boundary. | Track its identity, authority, backing, and lifetime obligations separately from introduction bytes. |
| Deployment fee | One total native monetary unit under the current fee policy. | The native retained settlement transfers the fee once for the applicable accepted outcome. |
| Validator handler charge | Three validator-fuel units per deployment under the current handler rule. | The handler settlement uses its separate validator-fuel role. This is not a user fee or COMM count. |

Interaction count is not CPU time, reducer instruction count, or the number of physical cells consumed.
Authority demand is not signer count or physical-payer count.
Storage bytes are not resident memory, database file size, compressed page size, or network packet size.
Those machine-dependent observations can guide operations, but cannot change a validator's charge for the same accepted execution.

## Exact native byte quantities

Let $`|x|`$ denote the encoded length of the actual protobuf value supplied to the observer.
Let $`h=32`$ bytes be the hash width and $`n`$ the COMM's consume-channel count.
The current [byte measurement functions](../../../../rholang/src/rust/interpreter/accounting/byte_accounting.rs) define:

```math
\begin{aligned}
I(\operatorname{produce}(c,d)) &= |c|+|d|+2h,\\
I(\operatorname{consume}(\vec c,\vec p,k))
  &= \sum_i|c_i|+\sum_i|p_i|+|k|+h+|\vec c|h,\\
D(\operatorname{COMM}) &= \sum_i|d_i|,\\
T(\operatorname{COMM}) &= h+nh+n(2h).
\end{aligned}
```

The payload type is `ListParWithRandom`, not only its user-visible value list.
The continuation type is `TaggedContinuation`, not only the printed process body.
Measurement includes fields present in those actual encoded values.
Do not replace their lengths with source-text length or omit authority metadata to reduce the charge.

The trace formula is a specified footprint, not the encoded length of the entire `COMM` object.
Changing serialization, metadata, hash width, or trace representation requires explicit compatibility analysis before changing this tariff.
An optimization that preserves runtime behavior does not automatically preserve encoded measurement.

The current byte schedule has introduction, transfer, and trace rates of one:

```math
Q(e)=I(e)+D(e)+T(e).
```

`ByteCostSchedule`, its version, and `byte_cost_schedule_digest` identify this native schedule.
General schedule application multiplies each dimension by its rate using checked arithmetic.
Every length conversion, addition, and multiplication must reject overflow before publishing effects.

## Event identity and multiplicity

Produce and consume introduction identities use distinct domains over their trace source hashes.
COMM accounting uses `COMM::cost_identity`.
The accounting scope must retain the distinction between the event kind, stable identity, and owning execution.
An event identity cannot authorize another execution's debit or restore a consumed allowance.

Persistent retries of one introduction charge once within the applicable accounting scope.
Distinct nonpersistent introductions retain their multiplicity, even when their source shapes match.
Each distinct billable firing of a persistent process still charges its own COMM delivery and trace work.
No firing means no COMM charge, but an introduction can still incur its own charge.

Introduction sponsorship is separate from the stored interaction authority.
A funding-stack datum can carry no spendable interaction authority while its introduction still has an authenticated sponsor.
Aliases, retries, and concurrent observations must not replace that sponsor or duplicate its charged event.

Arrival order must not decide whether introduction work is billed.
Charging only values that remain stored would price an immediate match differently from a delayed match.
Consequently, current introduction charging is not a net-retained-storage tariff.

## Authority demand and monetary projection

The [authority valuation](../../../../rholang/src/rust/interpreter/accounting/authority/valuation.rs) derives a multiset from an event's complete authority atoms.
Repeated required occurrences remain repeated demand.
Regrouping equivalent compound syntax must preserve that demand and the admissible complete funding assignments.
Collapsing physical aliases must not erase the logical occurrences or their locations.

One COMM can require multiple matching authority resources.
Its interaction count remains one, while its authority-resource valuation can exceed one.
Monetary pricing follows actual authority-resource demand under the approved valuation, not the number of wallets that happen to fund it.
The matching-resource proof remains necessary even when all monetary balances are sufficient.

For an event, retain the measurement tuple $`(C,I,D,T)`$ alongside its complete authority and location evidence.
Here $`C`$ is the interaction count, and the other dimensions are the byte quantities defined above.
This tuple alone does not encode all authority-resource demand or authorize a monetary debit.
The selected resource schedule and valuation must determine the obligation before the [allocation policy](authority-allocation-policy-decisions.md) distributes it.

The current runtime stores weighted byte amounts in `AuthorityByteEvent`.
For COMM events, that amount combines delivery and trace charges under the current schedule.
It does not independently retain both raw dimensions in that record.
New multidimensional pricing must retain or deterministically reconstruct the original quantities from authenticated event inputs.
It cannot infer both raw dimensions from their weighted sum or reuse that sum under a different schedule.

The [raw observation projection](raw-byte-observations.md#native-resource-class-projection) retains each positive dimension with its complete source observation and captured resource-class index.
It counts one interaction per recorded COMM, not one per owner or stack-transfer event.
Class resolution completes before the projection becomes available to its caller.
This measurement stage does not construct prepaid resources or authorize settlement.

The current `ProcessedDeploy.cost` combines COMM units and quantitative byte cost.
That compatibility field is not a complete monetary allocation witness.
It does not replace per-authority demand, payer consent, asset identity, captured exposure, or the separate fee.
Historical replay must keep the historical field meaning and tariff.

### Region-qualified demand

`NativePhloMeasurements::region_demands` validates region evidence before exposing the native demand projection.
Each result retains its complete source observation, resource-class index, dimension, positive quantity, and original `CostRegion`.
The region contains its execution-instance identifier and complete authority signature.
An execution-instance identifier does not identify a persistent funding purse.
The projection does not replace one identifier with the other.

For an observation $`e`$, let $`M(e)`$ contain its positive measured dimensions and quantities.
Let $`R(e)`$ contain its canonical authority regions.
The projection constructs:

```math
D(e)=[(e,d,q,r)\mid(d,q)\in M(e),\ r\in R(e)].
```

The list retains every dimension-region pair, including regions with unit authority and repeated observations.
Compound authority remains intact. The projection does not split its signature into wallets or multiply the COMM count by signer count.
A quantity remains one counted entry, even at the maximum native integer value.

```text
For each captured observation:
    Bound the cumulative region count.
    Validate region identity lengths and signature presence.
    Reserve signature-tree work before canonical validation.
    Bound cumulative encoded authority bytes and reserve validation work.
    Require strictly ordered, distinct region identities and canonical signatures.
Reject positive measurements without authority regions.
Expose each positive dimension paired with each original region.
```

Validation completes before the caller receives the projection.
The projection borrows immutable observations and regions. Iteration does not clone signatures or allocate per resource unit.
Limits apply across the complete capture, including repeated rows.
The signature-tree budget bounds the cost-signature tree. It is not a separate bound on arbitrary processes inside quoted names.
Existing process admission and host-work controls retain that responsibility.

This validation establishes evidence shape, not authenticated origin, wallet consent, or spendable backing.
Native execution must bind the retained region to actual custody and eligible stack positions before resource matching.
Acquisition terms must come from authenticated funding state, not current prices inferred from measurement quantities.
No result from this projection alone authorizes a debit or receipt issuance.

The region-projection theorems in `PairedByteReceipts.v` quantify over arbitrary complete region values and region lists.
They prove exact evidence retention, positive quantity, completeness, multiplicity, append composition, and permutation preservation.
Generated Rust tests compare an independent dimension-region product and exercise malformed evidence and limits.
Recorded execution and replay tests compare complete region-qualified demand, not only totals.
This borrowed projection introduces no shared mutable state. Synchronization remains in observation capture and atomic settlement.

### Resources acquired for later execution

New resources consumed immediately and new resources retained for later execution have different results.
The fresh partition contains the former. It must not also become retained supply.
`CheckedPhloExecution::with_retained_acquisitions` attaches an explicit, counted retained-acquisition output to the checked consumption witness.
It preserves the original five consumption partitions.

Let $`B`$ denote newly acquired resources that remain after successful execution.
Let $`V`$ denote the existing class-weight and authority-occurrence valuation, and let $`p`$ denote the checked acquisition price.
The successful monetary charge is:

```math
C = 1 + p V(F) + p V(B).
```

The flat fee remains one total monetary unit.
Compatible prepaid consumption adds no new acquisition charge.
An empty retained output preserves the previous charge and obligation bytes.
A zero-weight or zero-price output still has a complete resource identity and quantity.
The checker must not omit that output merely because its monetary value is zero.

Retained acquisition does not add a COMM or change measured consumption.
The existing `phloLimit` checks consumption against its certified resource bound.
Signed source debit caps, exposure caps, available balances, and resource permissions cover both acquisition obligations together.
Retained acquisition is not permission to exceed those monetary limits.
Its constructor also checks combined resource-entry, authority-node, key-byte, host-work, quantity, and monetary arithmetic bounds.

The funding projection uses separate obligation identities:

| Kind | Wire tag | Meaning |
| --- | --- | --- |
| Fee | `0` | The separate deployment fee. |
| Resource | `1` | Newly acquired resources consumed by this execution. |
| Retained resource | `2` | Newly acquired resources retained for later execution. |

The domain remains `f1r3node:phlo-obligation:v1`. Each non-fee payload contains a complete canonical resource key.
Existing fee and consumed-resource encodings remain byte-identical.
Older decoders reject the new tag. Activation therefore requires the complete supported funding implementation, not mixed decoder behavior.
Canonical allocation treats the two resource purposes as distinct obligations, even when their resource keys match.
Both purposes require explicit permission for that complete resource key.

```text
Validate the existing consumption witness.
Check the retained output with the same aggregate structural limits.
Combine equal retained keys with checked quantities.
Compute the additional acquisition value under the checked schedule.
Project separate consumed and retained obligations, with one fee.
Verify source permissions, capacities, caps, and canonical allocation for the complete result.
Match the complete observed consumption witness and retained output before settlement.
```

Admission rejection and execution failure cannot publish newly acquired retained resources.
The obligation projector rejects a nonempty retained output for those outcomes.
Classified user failures can still retain the approved billable consumption and fee under the existing failure policy.
That failed outcome must contain no retained acquisition output.

The acquisition constructor and funding checker do not authenticate actual stack births or execute wallet debits.
The native producer must bind each retained output to an authorized physical birth and original acquisition receipt.
The [birth-capture contract](prepaid-receipt-storage.md#bind-retained-acquisition-to-physical-births) checks quantities, cell authorities, selected acquisition terms, and live physical occurrences.
That capture does not itself establish causal birth evidence or atomic receipt issuance.
Wallet debits, physical resources, receipt records, and application effects must publish within the same rollback boundary.
One consumed-resource assignment cannot fund a second retained output.

Rocq proves additional-backing conservation, additive splitting, prepaid non-rebilling, and unchanged charging when the retained output is empty.
The obligation-wire proof binds the distinct purpose tags and complete resource payloads.
Generated tests compare the charge and quantities with an independent arithmetic oracle.
Native Rust funding tests cover arbitrary wallet cohorts, source permissions, total exposure, failures, overflow, and representation bounds.
Outcome matching compares retained identities and quantities even when their monetary values match or equal zero.
These pure checks introduce no shared mutable state. They do not prove atomic native issuance or replay publication.

Sources:

- [Region projection](../../../../rholang/src/rust/interpreter/accounting/native_phlo_rules/measurements/regions.rs)
- [Region regression tests](../../../../rholang/src/rust/interpreter/accounting/native_phlo_rules/measurements/regions/tests.rs)
- [Projection model](../../../../formal/rocq/cost_accounted_rho/theories/PairedByteReceipts.v)

## Counted resource representation

`PhloResource` identifies a resource by location, class, acquisition terms, and complete authority.
`PhloResourceAmount` pairs that identity with a positive `u64` quantity.
`CountedPhloExecutionWitness` supplies counted entries for available, required, used, unused, and fresh resources.
The existing occurrence input and the counted input share one checker. An occurrence has quantity one.

For resource key $`k`$, define $`N_X(k)`$ as the sum of quantities with that key in partition $`X`$.
The checker requires:

```math
N_A(k)=N_U(k)+N_N(k),\qquad
N_R(k)=N_U(k)+N_F(k),\qquad
N_N(k)=0\ \lor\ N_F(k)=0.
```

Here, $`A,R,U,N,F`$ identify available, required, used, unused, and fresh resources respectively.
The last condition prevents fresh acquisition while compatible prepaid resources remain unused.
Only exact keys combine. Equal prices do not establish resource compatibility.

```text
Check the total entry count across all five partitions.
For each partition:
    Reject zero quantities.
    Validate each complete resource key within the shared structural budget.
    Add quantities for equal keys with checked arithmetic.
Check both partition equations and prepaid exhaustion.
Calculate weighted usage and fresh acquisition with checked arithmetic.
Reject usage above the accepted bound.
Project one fee column and one column per distinct fresh resource key.
```

The entry limit bounds representation size, not the represented quantity.
Authority-node and key-byte limits still count work for every supplied entry, including repeated entries.
Splitting an entry cannot bypass those limits. Overflow rejects the result without publishing a charge.
Zero-weight resources retain their quantities and participate in resource matching.

One entry can represent the maximum `u64` quantity without allocating that many resource objects.
The checker and projection allocate by entry count and distinct key count.
This property does not establish a bound for the complete interpreter or node.
No new locks, background processes, persistent records, or wire formats are part of this representation.

The projection combines identical monetary obligations while retaining their quantities.
It does not combine distinct permissions or change source capacities.
Canonical family ordering compares logically expanded keys, unit amounts, and permissions through counted runs.
Thus, grouping cannot select another canonical outcome merely by shortening a list.

[`PrepaidResourceDischarge.v`](../../../../formal/rocq/cost_accounted_rho/theories/PrepaidResourceDischarge.v) proves counted/expanded quantity, partition, exhaustion, and valuation equivalence.
[`CountedFundingProjection.v`](../../../../formal/rocq/cost_accounted_rho/theories/CountedFundingProjection.v) proves payment splitting and joining with identical permissions.
Those proofs preserve each wallet's debit and capacity bound, not only the aggregate amount.
Run-comparison lemmas establish common-prefix cancellation, unequal-head ordering, and progress without expanding quantities.
The proofs use unbounded natural numbers. Rust checks reject values outside native integer bounds.

Property tests compare small counted examples with independently expanded resource and allocation inputs.
They check wallet debits, separate fees, resource and fee cursors, resource identity, and split-entry equivalence.
Boundary tests cover maximum quantities, arithmetic overflow, zero quantities, and representation limits.
The existing observation-log Loom tests cover concurrent measurement publication. These pure representation functions introduce no new shared-memory boundary.
These proofs and tests do not establish production schedule authentication or complete funded execution and replay.

### Derived prepaid discharge

`prepare_counted_phlo_discharge` constructs the three result partitions from typed available supply and required demand.
For each complete resource key $`k`$, define $`a=N_A(k)`$ and $`r=N_R(k)`$.
The construction uses:

```math
u=\min(a,r),\qquad n=a-u,\qquad f=r-u.
```

Here, $`u,n,f`$ are the used, unused, and fresh quantities for that key.
These quantities uniquely satisfy both partition equations and prepaid exhaustion.
Different acquisition terms remain different keys, even when their monetary values match.
The construction never substitutes current terms for original terms.

```text
Check the combined input entry count.
Encode complete keys with shared structural and byte budgets.
Sort and combine equal keys with checked quantity addition.
Merge the sorted supply and demand groups.
For each key, calculate used, unused, and fresh quantities.
Emit positive quantities within the remaining witness and host-work limits.
Return the prepared five-partition witness.
```

The constructor shares canonical key normalization with the outcome matcher.
It retains borrowed original inputs and allocates result entries by distinct key count, not by resource quantity.
Each represented output entry also consumes the complete witness's structural budget.
Encoded-key comparisons and allocations consume host work.
A failure exposes no prepared result and mutates no resource, wallet, or receipt.

`PreparedPhloDischarge` is not authenticated supply or a funding certificate.
The existing execution checker must still validate the selected resource classes, usage bound, and checked price controls.
The signed-policy method `capture_matching_discharge` performs that check before matching the complete prepared outcome family.
Native callers must supply authenticated available resources and required demand derived from actual execution.
The constructor does not establish their origin or authorize receipt issuance.

The discharge proofs establish conservation, exhaustion, unique quantities, and bounded results for arbitrary natural-number inputs.
Rust rejects quantity overflow before construction succeeds.
Generated tests compare the constructor with an independent expanded-occurrence model across all complete key fields.
They also compare input permutations, split quantities, and existing checked execution charges.
Boundary tests cover maximum native quantities, incompatible keys, representation limits, and host-work exhaustion.
Signed-family tests compare the derived capture with the existing capture, including offered-price envelopes and platform-failure outcomes.

The constructor is a pure operation over borrowed inputs. It introduces no new shared mutable state or synchronization.
Concurrent observation capture and atomic economic publication remain separate verification obligations.

Sources:

- [Discharge constructor](../../../../rholang/src/rust/interpreter/accounting/phlo_execution/discharge.rs)
- [Constructor regressions](../../../../rholang/src/rust/interpreter/accounting/phlo_execution/discharge/tests.rs)
- [Shared key normalization](../../../../rholang/src/rust/interpreter/accounting/phlo_execution/partition.rs)

## Storage lifetime and prepaid resources

Introduction bytes account for introduction work regardless of the later storage outcome.
Consumption or garbage collection must not create an introduction refund unless an approved refund rule permits it.
Repeated observation of an existing value must not create a new introduction charge without a new semantic introduction.

Located prepaid resources retain their identity, compatible authority, backing, and remaining allowance while they persist.
Consumption must not charge their acquisition a second time.
Top-ups can increase available backing but cannot silently recreate consumed allowance or expand signed exposure.
Transfers must preserve captured obligations and original refund sources.

Persistent storage liability requires its separate approved contract.
This measurement contract does not introduce byte-time rent, recurring wallet debits, or a tariff based on local pruning schedules.
It does not claim that a one-time introduction charge bounds all future storage or guarantees long-term node uptime.

## Execution and failure boundaries

The [runtime observer](../../../../rholang/src/rust/interpreter/rho_runtime.rs) connects produce, consume, and COMM measurements to `RuntimeBudget`.
The [budget](../../../../rholang/src/rust/interpreter/accounting/mod.rs) records authority and byte evidence and checks available allocation.
The [evaluation transaction](evaluation-transaction-isolation.md) governs rollback across those observations and RSpace effects.

An observation or private reservation is not a published charge.
Publication must retain a consistent application result, economic result, allowance change, and receipt.
Preacceptance rejection and platform failure must not publish a partial economic result.
Classified user failures retain only the billable effects allowed by the approved failure policy.
An unfired interaction is never billable merely because the engine attempted it.

These metering rules apply within an installed user accounting scope.
The current observer skips trusted unmetered construction and executions without that scope.
Ordinary user admission must install the required scope. A user cannot select a trusted construction path to avoid charges.
Scope installation, nested execution, reset, and replay require explicit integration tests.

The current budget also conditions COMM and byte charging on nonempty authority demand.
Thus, merely reaching the COMM observer does not prove that a charge occurs.
Native refinement must establish which empty-demand paths are legitimate and prevent them from bypassing required user funding.
Source inspection of this condition alone does not establish a reachable authorization defect.
Any correction requires a concrete reproduction and must preserve legitimate trusted or authority-neutral operations.

All independent validators must reconstruct the same measurements from the same accepted inputs and captured schedule.
CPU speed, thread schedule, cache residency, host memory, and local retry timing cannot change the result.
Disjoint resource scopes must retain their concurrency. This contract does not introduce a global metering lock.

## Verification requirements

| Invariant | Required examples, properties, and controls |
| --- | --- |
| Exact byte dimensions | Independently encode channel, payload, patterns, and continuation. Vary each field and compare each measured dimension, not only the total. |
| Dimension separation | Vary introduction, delivery, trace, authority multiplicity, and fees independently. Detect swapped dimensions and double application of a rate. |
| Event identity preserves multiplicity | Exercise persistent retries, separate nonpersistent events, repeated firings, reset, and replay. Detect duplicate charges and omitted distinct events. |
| Metering scope cannot be bypassed | Trace actual user, trusted, nested, and empty-demand routes. Check which events must charge and which exclusions are valid. |
| Arrival-order independence | Compare immediate matching, delayed matching, and admissible concurrent orders with equal semantic inputs. |
| Demand survives representation changes | Regroup compound authority and change physical aliases without erasing required occurrences or locations. |
| Weighted evidence retains its meaning | Reject replay under a different schedule. Detect attempts to recover distinct raw dimensions from one weighted sum. |
| Fees remain separate | Vary signer and payer counts while preserving the one-total-unit deployment fee and distinct handler custody role. |
| Checked arithmetic is atomic | Cover zero, maximum lengths and rates, conversion boundaries, and overflowing sums. Check unchanged state on rejected publication. |
| Prepaid storage remains backed | Exercise retention, repeated activation, transfer, top-up, consumption, rollback, and release without duplicated acquisition or allowance. |
| Publication is concurrent and exact | Use competing shared-custody operations and independent scopes. Inject failures between observation, matching, checkpoint, and publication. |

The existing `VaultBackedByteAccounting.v` and `AuthorityResourceValuation.v` models cover distinct arithmetic and authority abstractions.
Refinement must connect their assumptions to native observer inputs, event identity, captured state, and publication.
Property tests must use independent measurement and projection oracles rather than call the implementation twice.
Use Loom for native synchronization boundaries and integration tests for proposal, independent validation, restart, and replay.
Finite model bounds and generated ranges must remain explicit. Neither proves unbounded implementation correctness by itself.
