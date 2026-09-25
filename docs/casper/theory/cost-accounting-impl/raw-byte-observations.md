# Raw byte observations

The interpreter preserves raw byte quantities for accepted accounting events.
These quantities support typed funding evidence without reconstructing measurements from weighted charges.
The observations do not change the legacy byte schedule, create spendable balances, or authorize wallet settlement.

## Measurement and legacy charge

Each observation records an event identity, event kind, canonical authority, optional measurement, and optional legacy amount.
The measurement contains introduction bytes, transferred bytes, and trace bytes.
The existing V1 schedule maps these quantities to the legacy byte amount.
The observer retains the original overflow check and error representation.

| Accepted event | Raw measurement | Legacy byte event |
| --- | --- | --- |
| Measured introduction with positive cost | Present | Present |
| Measured COMM with billable authority and zero byte cost | Present | Absent |
| Measured COMM with billable authority and positive byte cost | Present | Present |
| Measured COMM with unit authority | Present | Absent |
| Cost-only legacy API call | Absent | Present only when the existing rule charges bytes |

COMM means communication between a matching send and receive.
A unit authority has no resource demand in the existing authority algebra.
Its raw quantities must not disappear merely because its legacy projection has no byte event.
Conversely, a raw observation must not introduce a legacy charge where the existing rule charges none.
The existing reservation validator rejects zero-weight introductions. Rejected introductions produce no accepted receipt.

## Publication and retries

`RuntimeBudget` stores each raw measurement and its legacy projection in one immutable observation.
The existing authority mutex protects receipt publication and accepted authority-event updates.
Introduction reservations use that mutex for their reservation and publication.
Persistent introductions also retain the existing introduction lock, acquired before the authority lock.
The implementation adds no global funding lock.

Persistent introductions and COMM retain their existing identity-based deduplication.
A measured retry must match the original authority, raw dimensions, and legacy projection.
Equal weighted costs do not establish equal measurements.
A measured retry cannot supply missing evidence for an earlier cost-only call.

Nonpersistent introductions remain separate occurrences, including repeated identities.
Rejected reservations add no accepted observation.
Reservation attempts can still affect the existing diagnostic and reconciliation machinery.
The receipt proof does not equate a rejected attempt with an unchanged diagnostic trace.

Persistent deduplication stores shared immutable observations, not indices into a mutable vector.
This prevents stale indices after allocation replacement.
Allocation replacement preserves the existing deduplication behavior and marks discarded receipt history as incomplete.
Additional observations cannot repair that lost history.

## Evaluation results

The interpreter captures one receipt snapshot after reduction completes.
It derives legacy byte events and their total from that same snapshot.
Snapshot rows preserve occurrences. Their append order is not a consensus order.
The legacy projection retains its existing canonical sort order.

An owned snapshot remains unchanged after another evaluation resets the runtime budget.
Full reset clears observations and identity history under the existing quiescent evaluation contract.
Quiescence means that the previous evaluation has no active producers.
These changes do not add a reset barrier or prove the complete runtime lifecycle.

`has_complete_measurements()` requires a metered context, intact history, and a measurement on every recorded occurrence.
A direct legacy COMM reservation marks the history incomplete because it supplies no raw measurement.
An empty metered execution can have complete measurements.
An early parse failure uses the default snapshot, which does not claim a metered context.

System operations outside the accounting scope and trusted unmetered operations remain excluded.
The runtime lifecycle must keep those scope boundaries consistent with evaluation ownership.
A receipt completeness check does not authenticate publicly supplied snapshot values or prove that every possible runtime caller uses the observer.

## Bounded counted capture

`checked_measurements(maximum_entries)` borrows the complete snapshot and computes three independent `u64` totals.
It rejects excessive entry counts before scanning rows.
It also rejects missing measurement context, lost history, missing measurements, or overflow in any dimension.
Rejection does not change the snapshot or publish a partial result.

The capture retains every original row, including its authority, event kind, identity, and optional legacy projection.
Repeated identities remain separate occurrences when the observer recorded them separately.
Unit-authority rows and rows without legacy charges still contribute their raw quantities.
Changing row order cannot change successful totals or convert an overflow into success.

For $`n`$ rows, capture requires $`O(n)`$ time and $`O(1)`$ additional space.
It does not clone rows, expand byte quantities, apply tariffs, or allocate funding among wallets.
The borrowed result prevents snapshot mutation while the capture remains in use.
This arithmetic check does not authenticate a snapshot supplied by an external caller.

For example, two recorded occurrences of seven transferred bytes produce fourteen transferred bytes, even when their event identities match.
The native producer must retain their authority information when it creates typed resource demand.
Raw totals alone cannot determine settlement, and introduction bytes do not measure net retained storage.

## Native resource-class projection

`NativePhloRules::measure` checks the complete snapshot before it returns any resource measurements.
It resolves every positive dimension against the captured policy's supported resource classes.
A missing class rejects the capture, even when that dimension has no legacy charge.
An omitted class is valid only when its measured total is zero.

Each projected occurrence borrows its original observation and retains the policy's class index.
The projection emits one counted quantity for each positive dimension, not one object per byte.
It preserves repeated occurrences, authority, identity, and raw-only rows.
It does not reorder observations, deduplicate them again, or clone their authority trees.

| Dimension | Quantity per recorded observation |
| --- | --- |
| Compute | One for a COMM observation, otherwise zero. |
| Introduction | The recorded introduction bytes. |
| Transfer | The recorded transferred bytes. |
| Trace | The recorded trace bytes. |

This compute quantity measures interactions before authority valuation.
Multiple owners do not multiply the COMM count.
Stack-transfer authority events do not constitute additional COMMs or provide byte measurements.
Their resource acquisition and backing require separate evidence.

The projection takes linear time and constant additional space in the number of observations.
It applies no price, debits no wallet, and creates no prepaid resource.
Typed locations, acquisition terms, and authenticated backing remain necessary for settlement.

## Region and purse projection

`region_demands` attaches each complete authority region to each positive measurement dimension.
It preserves the original observation, region identity, signature, class index, and quantity.
It rejects missing authority for positive demand, malformed regions, noncanonical authority, and exceeded representation or host-work limits.
Repeated work remains repeated demand. The projection does not expand a counted byte quantity into individual objects.

`locate_purses` maps each region to its existing signature channel.
It uses `cost_signature_to_sig` and `SignatureChannel::from_sig`, the same channel basis that native supply uses.
An execution-region identity is not a persistent purse address.
The mapping does not derive a purse from process position, region order, or a new hash domain.

The purse binding retains both the original `CostSignature` and its funding `Sig`.
It also retains the canonical channel and its encoded bytes for typed resource locations.
These fields have different roles. Channel equality does not establish authority equality or authorize consumption.
For example, a quote and a ground atom with equal reflected bytes share a channel but retain different authority forms.
Both demands remain present when their channels alias.

A region identity must denote one original signature throughout the captured observations.
Conflicting signatures reject the complete mapping, even when they reflect to the same channel or occur on a zero-quantity row.
Repeated observations of one valid region reuse its binding without merging their work occurrences.
Unit signatures retain their empty canonical channel. Compound signatures retain every leaf occurrence, without a two-owner restriction.

The bounded mapping follows this procedure:

```text
For each captured row, reserve verification work.
For each region, compare its original signature with any existing binding.
Reject a conflicting region identity.
For a new binding, check the binding count and calculate its encoded size.
Reserve signature and channel bytes before constructing the channel.
Convert the canonical authority and derive its existing signature channel.
Check the channel size against the calculated funding shape.
Store the complete binding without discarding authority information.
Iterate the original demands and attach their checked bindings.
```

The byte limit counts each distinct region binding's original encoded signature and encoded channel.
The structural-work budget also bounds signature traversal. Repeated bindings still consume verification work.
The result borrows immutable observations and adds no shared mutable state or synchronization.

## Native execution reservations

`NativePhloExecutionContract` prepares resource charges from individual measured observations before their acceptance.
Its constructor requires exact agreement between checked controls and the complete selected schedule.
`AdoptedResourcePolicy::bind_execution_contract` also checks the adopted genesis policy, resource rules, and minimum price.
This binding does not replace signature verification or proof of sufficient funding.

For one observation, let $`q_d`$ denote its quantity in dimension $`d`$.
Let $`w_d`$ denote the selected class weight.
Let $`v(s_r)`$ count every ground, name, or quote leaf in region $`r`$'s authority.
Unit authority has zero valuation. Repeated leaves and distinct regions retain their multiplicity.
The charge in phlo is:

```math
c = \sum_d \sum_r w_d\,v(s_r)\,q_d.
```

This calculation uses the same valuation as the typed acquisition and execution checkers.
It does not multiply usage by the REV price or select wallet contributions.
Prepaid resources and new purchases both contribute to usage. Their monetary settlement remains separate.

Preparation retains the exact observation in an immutable shared reference.
It checks raw measurements, class availability, canonical regions, authority representation, and host-work limits.
It does not expand byte quantities into individual resource objects.
Legacy scalar amounts cannot substitute for missing raw measurements.

`NativePhloReservation` starts with zero usage under one execution contract.
Creating the reservation consumes that contract and retains an immutable copy of its validated resource weights and bound.
The reservation has no reset or clone operation that can replenish the accepted usage budget.
Let $`u`$ denote accepted usage, $`B`$ the checked resource bound, and $`L`$ the signed phlo limit.
Checked controls establish $`B \leq L`$.
A reservation succeeds exactly when checked addition produces $`u+c \leq B`$.
Failure leaves usage unchanged. A zero charge remains valid at the ceiling.
The counter uses unsigned 64-bit arithmetic without narrowing the limit to a signed counter.

A prepared charge belongs to its original execution contract instance.
Another instance cannot accept that charge, even when both instances have equal numeric parameters.
This in-memory ownership check does not enter a block hash, signature, or replay record.
Replay prepares its own charges under its own validated contract.

The observation owner must apply the following acceptance sequence:

```text
Prepare the charge from the immutable observation and checked execution contract.
Acquire the existing guard for accepted observations and resource usage.
Check event identity, retry compatibility, and execution generation.
Return without another charge for an already accepted, compatible idempotent event.
Reserve space for the receipt before changing accepted usage.
Reserve the charge against current usage, not an earlier worker snapshot.
Publish the receipt within the same critical section.
Release the guard before subsequent independent reductions.
```

The counter does not itself suppress retries or publish receipts.
The owner must preserve repeated non-idempotent occurrences and reject incompatible reuse of an idempotent identity.
Workers can prepare independent charges concurrently.
The short acceptance section must update shared usage and accepted observations atomically.
A deployment reset requires all workers from the previous execution to finish.

`RhoRuntimeImpl::evaluate_with_native_phlo` installs one consumed execution contract through `NativeRuntimeConfig`.
Measured produce, consume, and COMM observers prepare native charges outside the accounting lock.
Acceptance reserves usage and publishes the exact observation under the existing authority-state lock.
Compatible persistent introductions and COMM retries do not charge again. Nonpersistent introductions remain separate occurrences.
An incompatible retry fails without changing accepted usage or observations.

The native path does not evaluate the legacy byte tariff or debit the legacy scalar budget.
`EvaluateResult::native_phlo_usage` reports accepted unsigned usage separately from the legacy `cost` field.
Raw measurements remain available when native weights are zero, including quantities that would overflow a legacy tariff.
Unmeasured charge requests fail in a native execution.
Reset requires a metered runtime with no active accounting scope and an owner that has joined all previous workers.
The scope check alone does not establish exclusive ownership across runtime clones.
Clearing authority allocation cannot replenish native usage and invalidates the observation history when accepted rows existed.

The native evaluation entry point preserves the existing rollback boundary.
A host-work rejection restores its RSpace soft checkpoint and returns no chargeable observation snapshot.
An ordinary execution failure retains attempted measurements. The enclosing deployment owner must roll back user state and settle the verified attempt.
Successful play and recorded replay tests compare native usage, complete observations, consumed replay events, and final roots.
A zero-bound test confirms that a charged COMM cannot execute its continuation.
Host-work tests reject at every primitive-budget boundary of a measured fixture, including boundaries after accepted charges.
They compare restored hot state and empty event logs, and reject economic use of the returned incomplete snapshot.
Runtime-reuse tests check parser rejection, fresh native usage, retained snapshot isolation, and subsequent legacy evaluation.

This entry point does not enable ordinary funded ingress by itself.
Funded integration must still compose authenticated acquisition proofs, retained resources, candidate rollback, settlement, and independent replay.

### Authenticated envelope execution

`RuntimeManager::evaluate_native_funded` accepts a checked wallet funding policy and its original offered envelope.
The adapter compares the complete envelope commitment before creating a runtime.
A different body, signed limit, or offered price cannot reuse that policy.
It derives execution controls from the checked funding family, whose cases must share the same controls.
It also checks the selected schedule against those controls and the adopted resource policy.

The adapter creates a private runtime and restores the wallet snapshot's exact state root.
The root bytes are an existing digest, not input for another hash operation.
The adapter never resets an existing working runtime to an older wallet snapshot.
Each attempt receives explicit block metadata, invalid-block context, region limits, and a host-work budget.
The enclosing admission owner must authenticate the block context.

Evaluation installs the offered envelope's deploy identity, selected funding authority, normalizer environment, and random seed.
The returned `NativeFundedAttempt` retains the checked policy, evaluation observations, and private runtime.
An execution error restores user state to the pre-evaluation checkpoint while retaining the evaluation result for failure classification.
Dropping an attempt does not publish a candidate root or wallet settlement.
The attempt is not an admission certificate, a funding-family completeness proof, or proof of successful settlement.

`NativeFundingSnapshot.v` proves exact root, envelope, and control binding, substitution rejection, and agreement for the same authorization.
These theorems assume a previously authenticated authorization and correct equality decisions.
They do not prove cryptographic collision resistance or the complete execution and settlement lifecycle.

### Reservation verification boundary

`PairedByteReceipts.v` proves exact acceptance, rejection of excess usage, zero-charge acceptance, composition, and machine-bound refinement.
Its commutation theorem requires enough capacity for both charges.
It does not claim that different contested schedules select the same successful events.
Two valuation lemmas preserve the dimension-region sum and repeated authority occurrences.
Six acceptance lemmas connect usage to the complete receipt ledger and preserve the ceiling through each accepted step.
They distinguish compatible idempotent retries, incompatible reuse, and repeated nonpersistent occurrences.

`NativePhloExecutionCeiling.tla` models three concurrent workers, conflicting event identities, cancellation, and two execution generations.
Its instance uses a bound of twelve, three authority leaves for charged events, and a zero-valuation event.
The model checks exact accepted usage, the hard ceiling, identity preservation, generation separation, types, and concurrent preparation.
Negative controls use stale usage during acceptance or reset while a worker remains active.
The first violates exact usage. The second violates generation separation.

Rust properties compare charges with wider-integer calculations and compare accumulated reservations with the existing typed acquisition checker.
Boundary tests include zero prices, zero weights, zero valuation, unsigned maximum quantities, and more than two owners.
Runtime history properties check acceptance, rejection, exact usage, and receipt counts after every generated operation.
Threaded runtime tests cover conflicting identities with one, three, and sixty-five authority leaves.
The Loom test checks the actual prepared-charge and reservation types inside a shared receipt-publication guard.
It prepares immutable inputs outside Loom's coroutines and models every interleaving of the shared updates within its three-worker instance.
These tests do not model the entire RSpace transaction or prove independent-validator agreement.
Its additional storage depends on distinct region bindings and their authority sizes, not the number of charged bytes.

This projection supplies location evidence. It does not choose acquisition terms, prove transfer permission, authenticate prepaid backing, or select a physical stack prefix.
Those checks remain necessary before native settlement can consume resources or debit wallets.

## Demand for resource acquisition

`prepare_acquisition_demand` connects located measurements to counted `PhloResourceAmount` entries under a selected acquisition schedule.
Each entry retains the purse's encoded location, complete funding authority, measured class, positive quantity, and exact schedule bytes.
The result also retains the corresponding observation and region through its occurrence iterator.
Repeated occurrences remain separate entries until the existing discharge checker combines identical resource keys.

The adapter checks that the supplied schedule bytes decode to the complete selected descriptor.
It checks each measured dimension against that descriptor's native class index.
Equal prices, equal weights, or equal total charges cannot substitute for those checks.
`check_controls` then requires the complete selected schedule to match the checked execution controls.
The native `AdoptedResourcePolicy::check_measured_acquisition` also checks the adopted minimum, protocol, shard, and complete genesis resource policy.

For each occurrence $`i`$, let $`q_i`$ be its quantity and $`c_i`$ its resource class.
Let $`w(c_i)`$ be the selected class weight and $`v(a_i)`$ the full authority's leaf-occurrence valuation.
The existing execution checker calculates usage as:

```math
U=\sum_i q_i w(c_i)v(a_i).
```

For an accepted successful execution with no prepaid consumption or retained acquisition, the monetary charge is $`1+pU`$.
Here $`p`$ is the selected acquisition price, and one is the deployment fee.
This formula does not count wallets as independent COMM events.
The execution checker enforces the certified resource bound and checked integer arithmetic before producing checked execution evidence.
Monetary allocation and signed source permissions remain separate checks.

```text
Reserve work for the schedule bytes and bounded descriptor decoding.
Decode the exact terms and compare the complete selected descriptor.
Check every measured class index and the total projected entry count.
Reserve output storage before constructing the resource list.
Borrow each original authority, purse location, and acquisition-term slice.
Copy each measured class index and positive quantity without expansion.
Keep the original occurrence-to-resource correspondence.
Check the captured controls and adopted resource policy.
Pass the complete resource evidence to the existing discharge and execution checkers.
```

For $`n`$ dimension-region occurrences, projection uses $`O(n)`$ time and additional space, apart from bounded schedule decoding.
It does not allocate one entry per byte or clone authority trees.
Entry and host-work limits reject before the caller receives a partial result.
Quantities remain observable when price, class weight, or authority valuation is zero.
Overflow while combining equal quantities rejects even when their monetary charge would be zero.

This result describes prospective acquisition demand. It does not convert historical prepaid resources to current acquisition terms.
The complete producer must authenticate eligible prepaid supply and preserve its original terms when it constructs the mixed consumption witness.
It must not use an empty supply as a fallback when eligible prepaid resources exist.
The adapter neither authenticates external observations nor proves ownership, available funds, physical births, or settlement permission.
It adds no shared mutable state, lock, or consensus rule.

## Verification boundaries

[`PairedByteReceipts.v`](../../../../formal/rocq/cost_accounted_rho/theories/PairedByteReceipts.v) models paired publication, exact retry measurements, occurrence multiplicity, rejection, snapshots, and reset.
Its two-order theorem compares accepted concurrent operations when both orders succeed.
It does not assert that resource exhaustion accepts the same arrival order.
Its optional legacy projection includes raw-only events.
The counted-capture lemmas prove completeness, row preservation, dimension bounds, additive composition, and order-independent totals over natural numbers.
Rust tests compare checked `u64` accumulation with an independent `u128` reference and test each overflow boundary.
The native-projection lemmas preserve source rows, exact positive dimensions, repeated occurrences, composition, and order-independent totals.
They also show that changing the optional legacy projection cannot change native quantities.
Property tests independently enumerate expected occurrences across event kinds, class orders, owner counts, quantities, and legacy projections.
Runtime/replay tests compare complete projected observations after actual funded RSpace communication.
The region and purse lemmas preserve complete evidence, occurrence counts, aliases, and permutations under a fixed channel mapping.
They do not prove hash injectivity, channel encoding correctness, or spending authority.
Purse property tests compare channel construction with an independent atom-hash and canonical-sort reference.
Boundary cases cover unit authority, compound authority, shared channels, conflicting identities, and exact representation limits.
Runtime/replay tests also compare the resulting full authorities and encoded purse channels.

Eight acquisition-projection lemmas preserve exact evidence, resource fields, occurrence counts, append composition, permutations, quantities across term changes, weighted usage, and prefix bounds.
These lemmas quantify over arbitrary region, location, authority, term, and class types.
They assume fixed location, authority, class, and valuation functions. They do not prove authentication or the full native funding lifecycle.
Generated tests compare usage and successful charges with independent wider-integer arithmetic over quantities, class weights, owner counts, and prices.
Additional properties check ordered evidence, append composition, and permutations across price changes.
Examples cover 1,000 owners, aliased channels with distinct authorities, zero valuation, maximum quantities, integer overflow, schedule substitution, and host-work exhaustion.
The native policy regression rejects a changed genesis policy or selected price even when the measurement dimensions still match.
Recorded replay tests derive the same typed acquisition demand, usage, and charge from actual RSpace observations on both execution paths.

Rust regression tests compare measured and legacy traces, charges, and authority events.
Property tests vary event kinds, quantities, persistence, and execution order.
Concurrent tests inspect paired snapshots during publication.
Loom tests explore the production receipt-log component under a modeled mutex.
Those tests also check counted capture of the immutable snapshots.
They do not explore the complete `RuntimeBudget` lock graph or distributed validators.
Runtime tests exercise actual RSpace observations, user abort, parse failure, and evaluation reuse.

Native settlement must combine measured demand with authorized prepaid consumption and authenticated acquisition provenance.
It must derive the complete execution witness before selecting a signed funding outcome.
The [observed outcome contract](observed-funding-outcome.md) defines those remaining integration boundaries.

Implementation and tests:

- [Receipt representation](../../../../rholang/src/rust/interpreter/accounting/byte_receipts.rs)
- [Receipt regressions and properties](../../../../rholang/src/rust/interpreter/accounting/byte_receipts/tests.rs)
- [Native class projection](../../../../rholang/src/rust/interpreter/accounting/native_phlo_rules/measurements.rs)
- [Native projection properties](../../../../rholang/src/rust/interpreter/accounting/native_phlo_rules/measurements/tests.rs)
- [Native purse projection](../../../../rholang/src/rust/interpreter/accounting/native_phlo_rules/measurements/regions/purses.rs)
- [Native purse properties](../../../../rholang/src/rust/interpreter/accounting/native_phlo_rules/measurements/regions/purses/tests.rs)
- [Native acquisition demand](../../../../rholang/src/rust/interpreter/accounting/native_phlo_rules/acquisition.rs)
- [Acquisition-demand properties](../../../../rholang/src/rust/interpreter/accounting/native_phlo_rules/acquisition/tests.rs)
- [Adopted-policy regression](../../../../casper/src/rust/util/rholang/costacc/genesis_resource_policy/tests/measured_acquisition.rs)
- [Recorded funding replay](../../../../casper/src/rust/rholang/runtime/envelope_tests.rs)
- [Runtime receipt tests](../../../../rholang/src/rust/interpreter/reduce_byte_receipts_tests.rs)
- [RSpace observers](../../../../rholang/src/rust/interpreter/rho_runtime.rs)
