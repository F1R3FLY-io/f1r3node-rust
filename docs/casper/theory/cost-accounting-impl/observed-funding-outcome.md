# Observed funding outcome

## Contract

The funding policy contains a prepared family of possible execution outcomes.
Settlement must select an outcome from actual execution evidence, not from a caller-supplied branch index.
Equal total costs do not establish that two executions used the same authority or resources.

`CheckedNativeSignedPhloFamilyPolicy::capture_matching_execution` matches supplied checked evidence against the complete prepared family.
It returns the existing canonical capture only when every matching case has the same complete settlement.
It does not replace source permissions, choose new funding assignments, or modify either cursor.
The matcher performs no wallet mutation and creates no escrow state.

## Complete execution identity

Each resource key contains its location, class, acquisition terms, and complete authority tree.
The canonical resource encoding distinguishes every field and authority node.
The matcher sorts encoded keys and sums quantities for identical keys with checked arithmetic.
Resource order and entry grouping can change, but multiplicity cannot change.
The occurrence-based input assigns quantity one to each entry. The counted input supplies an explicit positive quantity.

| Evidence | Required comparison |
| --- | --- |
| Controls | Full checked controls, selected schedule, signed terms, minimum price, and derived bounds. |
| Available resources | Complete prepaid supply multiset. |
| Required resources | Complete demand multiset. |
| Used resources | Complete consumed prepaid multiset. |
| Unused resources | Complete remaining prepaid multiset. |
| Fresh resources | Complete newly acquired resource multiset. |
| Retained acquisition | Complete newly acquired resource multiset kept for later execution, separate from fresh consumed resources. |
| Outcome | Rejected or accepted, with the exact accepted failure-class set. |

The [failure summary](economic-failure-observation.md) retains user, platform, certificate, and unclassified failures.
Reordered or repeated failure reports do not change their class set.
An accepted result with no failures differs from an accepted user failure, even if their charges are equal.
An admission rejection also differs from an accepted platform failure.

For example, one resource at location A can cost the same as one resource at location B.
The matcher rejects that substitution because the location is part of the key.
Extra zero-weight resources also change the identity, even when total usage and cost remain unchanged.

## Equivalent and conflicting cases

Several family entries can describe the same execution.
The matcher accepts these equivalent entries only when their complete canonical captures agree.
It compares:

- Canonical source identities, capacities, signed caps, holds, debits, fees, and refunds.
- Canonical obligation keys, resource quantities, and monetary amounts.
- Eligibility and exact assignment matrices.
- Both scoped resource and fee cursor transitions.

These records come from the verified family policy.
Policy verification recomputes the canonical assignment over the complete quantities of identical obligations.
The weaker checked-intent type alone does not establish that canonical assignment invariant.

Matching only final payer totals would be insufficient.
Two zero-charge failure cases can have different eligibility matrices while their debits and cursor transitions are equal.
The matcher rejects that ambiguity rather than letting input order select a permission set.

When all matching captures agree, the first matching index identifies an equivalent representation only.
It does not give that index a different economic result.
A later conflicting case invalidates the match.

## Resource quantities in settlement capture

`CanonicalPhloFundingCapture::obligations` exposes complete obligation records in canonical key order.
Each record retains the obligation purpose, typed resource key, exact quantity, monetary amount, and contributions from canonical source rows.
The capture retains each column's original position in the immutable checked family.
Both the typed key and quantity come from that position, not from a second decoder or a price calculation.

Monetary amounts cannot determine resource quantities.
For example, thirteen retained units and fourteen retained units both cost zero when their acquisition price is zero.
The capture preserves this distinction. Its quantity does not depend on price, class weight, or the number of funding wallets.

The contribution iterator exposes every source, including zero contributions.
Each column's contributions sum to its monetary amount.
Each source's contributions across columns sum to its checked debit.
Canonical ordering changes neither sum and does not recompute the funding assignment.

Capture construction reserves storage for the original-position map through the existing host budget.
Reading a captured quantity takes constant time and does not allocate storage.
Reading all contributions takes time proportional to the source count, without expanding resource quantities into individual units.

The native wallet adapter retains this capture after matching actual execution evidence.
Receipt creation must use retained-resource columns only, then bind their quantities and contributions to actual authorized births.
Consumed-resource columns and the fee column cannot fund additional retained output.
This capture does not itself create resources, debit wallets, or establish atomic publication.

## Bounded matching algorithm

The matcher checks the total case count before scanning.
It reserves host work for classification, resource encoding, sorting, comparison, and capture construction.
Each normalized execution also has resource-entry, authority-node, raw-key-byte, and encoded-key-byte limits.
Exhaustion returns an error, even if an earlier case matched.

```text
Check the complete family size against the case limit.
Normalize the observed outcome, all five consumption partitions, and the retained acquisition output.
For each prepared case:
    Compare its exact outcome and complete execution identity.
    If it matches, capture its already verified canonical settlement.
    Reject a capture that differs from an earlier matching capture.
Reject if no case matched.
Return the shared capture after the complete scan succeeds.
```

Normalization sorts each consumption partition and the retained acquisition output with bounded merge sort.
For a partition with $`n`$ supplied entries, sorting uses $`O(n \log n)`$ key comparisons.
The matcher does not expand a counted quantity into individual entries.
The host budget also charges the bytes inspected by comparisons.
The implementation retains only the observed execution, the current candidate, and the first matching capture.
The retained output cannot match a consumed-resource partition merely because both have the same price or resource key.
Equivalent retained quantities can use different grouping or input order.

## Rooted measured settlement

[`NativePrepaidDemandBinding::capture_settlement`](../../../../casper/src/rust/util/rholang/costacc/prepaid_receipts/inventory/demand.rs) connects the measured prepaid binding to the signed wallet policy.
The binding retains the exact checked phlo controls used to validate its acquisition demand.
The settlement method does not accept replacement controls from its caller.
The [prepaid binding contract](prepaid-receipt-storage.md#binding-prepaid-cells-to-measured-demand) defines its physical evidence and direct-consumption scope.

Before matching, the method requires equality between the prepaid capture root and the wallet policy snapshot root.
It checks the counted partitions with the retained controls, then adds the explicit retained acquisition output.
It submits that complete execution and outcome to the existing signed-family matcher.
Matching therefore preserves owner consent, canonical allocation, cursor transitions, and authenticated envelope identity.
The root check does not change the family selection algorithm.

A matching total charge is insufficient.
The method rejects missing retained output, a different outcome, or controls that differ from the prepared family.
It also rejects a wallet snapshot from another root, even when both snapshots contain equal balances.
The returned settlement remains subject to physical birth checks and the common publication checkpoint.
No wallet, stack, or application state changes during matching.

## Proof and test scope

[`ObservedPhloFamilyMatch.v`](../../../../formal/rocq/cost_accounted_rho/theories/ObservedPhloFamilyMatch.v) proves exact matching and matched-index membership.
It proves equivalent-duplicate acceptance, conflicting-duplicate rejection, incomplete-scan rejection, multiplicity preservation, non-user charge veto, and source-permission preservation.
Its settlement extraction premise requires the immutable, verified family policy.
Its resource model contains complete typed keys, not scalar prices alone.

The rooted composition proves that successful matching preserves both root equality and the complete context, consumption, retained output, and canonical settlement.
It proves cross-root rejection and unchanged selection when the root and context agree.
Roots are abstract equality tokens in this model. Cryptographic authenticity and immutable snapshot capture remain separate premises.
Generated tests mutate every selected root position and compare exact byte equality.
The native wallet integration includes measured prepaid binding, signed matching, retained receipt creation, rollback, and recorded replay.
This integration test does not establish complete native funding search or derive every possible execution outcome.

The retained-output extension proves exact output matching, multiplicity preservation, separate backing, and rejection of retained output on failure.
The quantity-capture extension proves field binding, source binding, column preservation, and contribution conservation under source and column permutations.
It also proves that zero monetary amounts cannot identify retained quantities.
These extensions do not prove complete native receipt issuance or the atomic publication of wallet and application effects.

Rust property tests check all five multiset partitions, permutations, zero-weight mutations, typed identity changes, failure masks, and work limits.
The signed-family fixture checks actual canonical captures, repeated resource quantities, cursor transitions, and conflicting eligibility.
These tests connect the matcher to the checked Rust policy types.
Quantity-capture tests cover zero prices, unequal quantities, arbitrary wallet cohorts, reordered retained outputs, and source-order permutations.
They check each typed column and both contribution sums against independent arithmetic expectations.

`capture_matching_discharge` accepts the [derived prepaid partition](resource-units-and-measurement.md#derived-prepaid-discharge) and checked controls.
It runs the existing execution checker before the complete family match.
This connection derives used, unused, and fresh quantities instead of accepting an independent split from its caller.
It does not establish the authenticity of supplied available resources or observed demand.

The [counted-resource contract](resource-units-and-measurement.md#counted-resource-representation) defines quantity validation and representation equivalence.
Generated tests compare counted and expanded forms across all five partitions.
Quantity changes remain observable even for zero-weight resources.

## Native producer boundary

### Private attempt observations

`NativeFundedAttempt::capture_measured_settlement` derives execution evidence from the attempt's retained observations.
The caller cannot replace those observations, the recorded meter value, or the checked wallet policy through this interface.
The method retains the adopted resource policy from execution and checks the selected schedule again.
It derives measured regions, funding locations, and counted demand before it binds rooted prepaid draws.
The existing execution checker derives usage from the resulting resource partitions.

Settlement requires complete measurements and exact equality between derived usage and the native meter.
Equal scalar usage alone is insufficient. The matcher also checks exact typed resources, outcome, retained quantities, and canonical settlement.
The method combines the evaluation's concurrent failure summary with classification of its returned errors.
The caller does not supply a replacement success or failure outcome.
Host-work limits apply to demand preparation, failure classification, and family matching.

The prepaid draw proposal and retained-resource proposal remain explicit inputs.
Their binding does not prove that the producer enumerated every permitted funding outcome.
Physical retained-birth checks and atomic publication remain separate requirements.
The method does not publish wallet changes or enable ordinary funded ingress.

The native COMM integration fixture includes compute, introduction, transfer, and trace classes with respective weights of 2, 3, 5, and 7.
Its actual price is three wallet units per phlo, with a separate one-unit deployment fee.
An isolated reference execution supplies raw quantities for the fixture's explicit funding case.
The fixture calculates expected usage directly from those quantities, weights, and authority-region multiplicities.
The calculation counts each distinct region, including regions owned by the same wallet.
Two concurrent private attempts and two independent replays start from the same wallet root.
The fixture uses one-owner and three-owner signed envelopes, with successful and failing processes for each cohort.
All four attempts must produce the same settlement root for each case.
Without prepaid resources, the total wallet debit equals three times the independently calculated phlo usage, plus the deployment fee.
Each wallet must pay its canonical allocation, including the deployment-fee residual.
Successful attempts retain the process outputs. User-error attempts remove all process outputs but retain the billable accepted measurements.
The original root must retain its original balance.
The fixture also rejects substituted acquisition terms and prepaid references without corresponding draws.
Altered replay sessions, altered usage, missing trace events, and exhausted host budgets must prevent a replay settlement attempt.
A repeated settlement request must fail without changing the settled root.

The prepaid variant starts with two authenticated introduction-byte cells and consumes one cell during settlement.
Each cell has nine wallet units of backing per authority member under the selected schedule.
The fixture transfers this backing from each wallet before it creates the initial protected receipts.
Settlement deducts the consumed backing from fresh acquisition charges. The unused cell and its exact receipt must remain available.
The fixture checks these rules for original execution and replay, including billable user failure.

These fixtures do not verify complete funding-family generation or production issuance of prepaid resources.
The prepaid initial state is explicit test setup, not the production issuance pipeline.
Its explicit funding case is test input, not an implementation of the up-front proof producer.
The [native COMM integration fixture](../../../../casper/tests/util/rholang/native_comm_settlement.rs) complements the raw-byte meter and replay tests.

`ObservedPhloFamilyMatch.v` proves rejection of incomplete measurements and unequal usage.
Its composition theorem retains root equality, execution context, exact resource partitions, retained output, and equivalent settlement selection.
These proofs assume authenticated observations and valid derived resource evidence.
They do not prove the entire native execution and settlement lifecycle.

### Funded replay adapter

`RuntimeManager::replay_native_funded` reuses the checked wallet policy and adopted execution contract.
The adapter requires the authorized envelope identity and the original wallet snapshot root.
It checks the operation journal against that envelope identity and selected schedule, then checks exact trace coverage.
The original attempt retains its event log before any user-error rollback.
The `replay_input` method exposes borrowed recording and trace inputs without permitting replacement of the attempt's observations.

Replay starts in an independent runtime at the verified pre-state.
Original execution and replay share source parsing, normalizer bindings, random-seed derivation, and invalid-block context conversion.
Replay receives the block context, deploy metadata, verified funding authority, and adopted price contract.
No wallet request can follow incomplete replay because the adapter requires a completed persistent export before it returns an attempt.

The adapter selects the state for settlement with these rules:

| Replay result | State for settlement | Funding evidence |
|---|---|---|
| Complete success | Exported replay root | Preserve the completed evaluation and accepted measurements. |
| Complete process failure | Original authorized root | Preserve the completed evaluation. Its failure classes determine retained charges. |
| Missing, invalid, interrupted, or host-rejected evidence | No settlement attempt | Return an error. |

Restoring application state does not erase accepted compute or byte-transfer charges from a billable process failure.
The adapter returns the same `NativeFundedAttempt` type as original execution.
Both paths therefore use `capture_measured_settlement` and the existing signed-family matcher before they construct wallet requests.
The adapter does not change consensus or enable funded ingress.

### Funding and runtime roots

Settlement preserves two distinct roots. The funding root authenticates the original wallet snapshot and captured prepaid receipts.
The runtime root identifies the checkpoint where settlement starts.
Successful replay exports user effects before settlement. Thus, its runtime root can differ from its funding root.
Original execution keeps these effects in its private working store. Its checkpoint root remains the original funding root until publication.

`NativeFundedAttempt` records the runtime root after execution or checked replay.
`capture_measured_settlement` transfers this private binding into the checked settlement.
Callers cannot replace the binding through the public settlement interface.
Ordinary settlement capture defaults to the original funding root.

Combined settlement checks the runtime checkpoint against the bound runtime root.
It independently checks captured prepaid resources against the original funding root and adopted policy.
Live receipt and physical stack checks remain necessary because a matching checkpoint does not prove unchanged working data.
Wallet changes, receipt changes, and stack consumption share the rollback boundary.

For example, a replay can start at root A and export user effects at root B.
Its settlement must use the wallet and receipt snapshot from A while it applies changes to the runtime at B.
Requiring both roots to equal A rejects valid replay. Recapturing funding at B changes the original authorization and is not permitted.

`NativeSettlementRoots.tla` models two independent workers with success, failure, incomplete replay, and missing authorization.
Its invariants preserve original funding, require complete authorized replay, and enable settlement at the selected runtime root.
Negative controls reject an original-root-only runtime guard and unauthorized funding recapture.
Two further controls detect acceptance of a foreign runtime root or a foreign funding snapshot when the corresponding check is absent.
This model covers root binding and independent settlement eligibility, not shared-wallet publication or the complete economic transaction.

`NativeFundingSnapshot.v` proves the state-selection contract and preservation of supplied evidence after complete, authorized replay.
It also proves rejection when authorization or completion is absent.
The composed checker requires the authorized root, envelope, and controls before it applies the completion gate and state-selection rule.
It rejects substitution of any authorized input and preserves failed-attempt evidence exactly.
The settlement-root lemmas require exact equality of both bindings and reject substitution of either root.
The model assumes that the completion flag comes from the checked replay boundary.
These proofs do not establish an extracted proof of the Rust adapter.
The integration tests check that connection with actual signatures, concurrent runtimes, persistent wallet balances, rollback, and repeated requests.

### Native attempt replay contract

Accepted byte observations and attempted operations serve different purposes.
Accepted observations explain charged usage. Attempt evidence must also explain deterministic denials and preserve the state that each attempted match observed.
A denied COMM must not create an accepted receipt, remove matching data, or run its continuation.

The native budget trace checker recomputes each charge under the selected execution contract.
It checks each decision against the original accepted-usage prefix, not the replay arrival order.
An affordable charge cannot carry a denial. A charge that exceeds the remaining bound cannot carry an acceptance.
Preparation overflow also requires denial. Host-work rejection remains an error, not evidence of an economic denial.
Occurrence indexing stops immediately when a comparison cannot reserve host work.
Replay reserves the checked observation-comparison bound before equality testing. Failed reservations do not consume an occurrence.

An occurrence consists of its execution session, causal operation path, and accounting stage.
The stage distinguishes produce introduction, consume introduction, and COMM observation.
Separate occurrences can share an event identity. The checker must not collapse those occurrences or charge a compatible accounting retry again.
Compatible retries require separate operation evidence because the budget trace contains charge attempts, not every RSpace invocation.

The native runtime records each fresh accepted or denied budget decision before returning its accounting result.
A compatible retry references its prior accepted attempt. The retry does not increase usage or create another accepted receipt.
Each retry records `fresh_before`, the number of fresh attempts already published under the authority mutex.
The accepted source index must be less than this count. Denied and zero-charge fresh attempts advance the count, but retries do not.
This position lets operation evidence locate retries without introducing an extra total order for independent operations.
Preparation overflow produces a denied row with the original measurement. Allocation failure and missing operation context invalidate export instead.
The recorder reserves host work before observation comparison, index traversal, and backing-storage growth.

Each preparation holds an execution-generation reference and a pending ticket.
Reset creates a new generation. A stale preparation cannot debit the new generation or invalidate its recording.
An unpublished ticket invalidates its recording before it decrements the pending count.
Capture holds the authority mutex, reads the pending count first, and then checks invalidation.
This order prevents capture from missing invalidation when a preparation ends concurrently.
Capture also rejects lost accepted-measurement history and host-work rejection.
Budget capture and operation-journal capture use the same pending-first completion check.
Neither capture can export evidence while a preparation remains pending or after an unpublished preparation ends.

Successful evaluation and ordinary economic denial return the captured budget recording.
Parser failure and host-work rejection return no economic recording.
RSpace preserves typed host-work rejection and phlo exhaustion across interpreter boundaries.
The live observer and native replay adapter use the same interpreter conversion.
The adapter also preserves host-work rejection inside budget, execution, and authority errors.
Opaque error text cannot authorize economic denial, even when its message names phlo exhaustion.
Malformed replay evidence remains an error rather than an accepted denial.
The native evaluation wrapper restores its state checkpoint for fatal errors and returned host-work rejection, including rejection without a sticky budget flag.
An economic denial preserves the accepted prefix at this layer. Funded settlement must verify that prefix before applying its separate rollback rules.

Replay consumes checked decisions by exact occurrence and observation.
Unknown occurrences, changed observations, duplicate consumption, and unused entries prevent successful completion.
The checker preserves accepted usage under arbitrary consumption order.
It does not prove that RSpace can realize every such order.

For example, a denied match can observe a datum that a later accepted COMM removes.
Replay cannot remove that datum before it verifies the denied match.
Required operation evidence must therefore bind exact sources and conflict dependencies, including attempts with no committed COMM event.
The reduction scheduler must select dependency-ready operations before it acquires channel locks.
Independent operations remain eligible together. Replay must not wait for a predecessor while holding a channel lock.

Budget order must also preserve execution causality.
A child operation cannot spend before the COMM that enables its continuation.
Checking scalar prefixes and accepted parent membership alone does not establish that order.
The combined certificate must bind each operation to budget intervals and independently verified scheduler frontiers.
A frontier is the set of submitted operations whose responses the scheduler releases together.
Independent operations within one frontier need no additional fixed order.
The standalone budget checker does not verify these causal obligations.

An operation interval uses fresh-attempt indices, not replay arrival times.
For an interval `[start, end)`, each owned attempt index lies inside that interval.
Each owned retry has `start <= fresh_before <= end` and `accepted_attempt < fresh_before`.
Every attempt and retry must have exactly one operation owner.
An operation with only retries can have `start == end`. Its execution order still comes from independently verified dependencies.

For each verified dependency from operation A to operation B, the complete checker must require `end(A) <= start(B)`.
Containment and this inequality prevent fabricated intervals from concealing reversed causal charges.
Introduction must precede COMM observation within each operation.
The checker must also verify the actual submitted frontier, source match, trigger, footprint, and completion result.
Internally consistent supplied intervals do not authenticate those facts.

The causal path identifies an invocation but does not directly identify its enabling COMM.
Lexicographic path order is not causal order because it also orders independent sibling branches.
Replay must reproduce continuation dispatch and reject missing, extra, or altered child operations.

RSpace captures one accounting observer after acquiring the operation's channel locks.
The same observer receives the start, introduction, optional COMM, and completion callbacks.
Replacing or removing the configured observer cannot change an operation that has already started.
The start callback receives the consume channels or the produce channel and its current joins.
These channels identify the matching footprint, including reads performed by a rejected match.

The start callback can reject an operation before introduction or store mutation.
Completion cannot replace the operation result with a new error after mutation.
An evidence recorder must reserve completion storage before effects and update that storage without further allocation at completion.
An incomplete or invalid recorder must prevent economic evidence export.
The lifecycle hooks alone do not establish a complete native replay certificate.

The native operation journal links each introduction and attempted COMM to one fresh attempt or compatible retry.
Each completed row retains its exact source, matching footprint, preceding conflicting operations, result, and fresh-attempt interval.
Independent operations can overlap. A conflicting operation cannot start before its preceding recorded operation completes under the channel locks.
Source records preserve channel identity, persistence, peek indices, and repetition counters.
An absent repetition counter differs from an explicit zero counter.

Economic rejection requires an explicit denied introduction or COMM decision.
A granted introduction followed by an unrelated runtime error cannot become an economic rejection certificate.
Missing observations, repeated completion, changed sources, and incomplete operations prevent export.
For example, a granted introduction with no COMM can certify `Stored`, but it cannot certify `Rejected`.
This rule separates insufficient funding from internal execution failure without changing successful execution or Casper finalization.

### Structural journal validation

`NativePhloExecutionContract::check_operation_journal` checks the exported budget recording and operation journal together.
It returns a `CheckedNativeOperationJournal` with private fields and read-only counts and usage.
This type establishes structural consistency. It does not authorize settlement or authenticate an RSpace replay.

The checker first bounds all variable collections before it allocates validation indexes.
Limits cover operations, combined fresh attempts and retries, individual and cumulative paths, source entries, footprint entries and bytes, and predecessor edges.
It validates fresh budget decisions with the signed execution contract and checks the reported usage against the computed total.
All operation and budget occurrences must belong to the expected session and be unique within their respective collections.

Every fresh attempt and retry must have exactly one operation owner.
Consume records also retain the exact peek indexes, including an explicitly empty set.
The source hash does not include this set. Matching the source hash alone cannot authenticate a stored receive.
The recorder captures peek indexes before the introduction charge. Missing, repeated, or out-of-range metadata invalidates the recording.
The importer rejects missing, duplicate, unsorted, and out-of-range indexes. A consume record with a COMM must use the COMM's peek set.
Replay checks the actual peek set before recording an introduction decision or changing tuples.
These checks apply to stored receives and denied introductions, not only matched COMMs.
The linked occurrence must match the owner's session, path, and stage.
A retry must reference an earlier granted attempt with the same stage and complete observation.
Each retry retains its own occurrence. Multiple distinct retries can reference one accepted attempt without another debit.

The checker verifies operation intervals and nondecreasing start cuts.
It applies these introduction-before-COMM rules:

| Introduction link | COMM link | Required relation |
| --- | --- | --- |
| Fresh attempt `i` | Fresh attempt `j` | `i < j` |
| Fresh attempt `i` | Retry cut `r` | `i < r` |
| Retry cut `r` | Fresh attempt `j` | `r <= j` |
| Retry cut `r` | Retry cut `s` | `r <= s` |

Equal retry cuts are valid. An operation with only retries can have an empty fresh-attempt interval.
Lifecycle validation permits only stored introductions, granted matches, denied introductions without COMM, or granted introductions followed by denied COMM.
In particular, a denied introduction cannot have a later COMM observation.

Footprints and predecessor lists must be sorted and unique.
The checker reconstructs predecessors from the last operation that touched each declared channel and requires exact agreement with the supplied list.
Each predecessor must finish no later than the current operation's start cut.
These conditions constrain declared conflicts without imposing an order on independent operations.
Temporary indexes and ownership marks remain private. A failed import does not change the supplied evidence.

Declared footprints are not necessarily complete footprints.
Coordinated changes to footprint and predecessor fields can remain structurally valid, as an explicit boundary test demonstrates.
Source hashes, measurement preimages, actual joins, matching, and scheduler frontiers still require independent runtime authentication.
The checker preserves source fields without interpreting a new source-identity scheme.
Native funded ingress remains disabled until the complete replay and producer contracts hold.

### Operation trace ownership

`CheckedNativeOperationJournal::bind_trace` binds the checked journal to an immutable committed RSpace trace.
The result, `CheckedNativeOperationTrace`, has no public unchecked constructor.
The importer sorts operation references by session and forward lexicographic path, as the play event log does.
Each slot retains its original journal index. Sorting does not change predecessor or budget references.

| Operation completion | Committed events owned by its slot |
| --- | --- |
| `Stored` | One introduction. |
| `Matched` | One introduction followed by its COMM. |
| `Rejected` | No committed events, including when the introduction succeeded before COMM denial. |

The importer computes the required event count with checked arithmetic before it scans the trace.
It then assigns consecutive, nonoverlapping event ranges to the sorted slots.
Missing events, extra events, and incorrect event kinds cause rejection.
Rejected operations retain slots even though their event ranges are empty.
Identical COMM values retain separate slots. Equal hashes do not merge operation occurrences.

Source comparisons include channel hashes, persistence, producer multiplicity and order, peeks, and complete repetition-key identities and counts.
A consume introduction must match its COMM consume source.
A produce introduction need not occur among the selected COMM participants. The selected tuples can all predate the current operation.
Each COMM repetition map must cover exactly its distinct selected producer identities.
The importer does not use the hash-only `Produce` equality as a complete source comparison.

External-result metadata belongs to the introduction event in its own slot.
The importer preserves that metadata even when the triggering producer does not participate in the COMM.
It checks metadata consistency between producer copies inside one COMM, but not across separate events.
Play updates existing producer metadata by hash. Events written later can retain different metadata, even for the same logical source.
Stored introductions can therefore contain external-result metadata, and an error flag does not require a nondeterministic flag.

Explicit limits cover events, source entries and bytes, output item counts, and output bytes.
Empty output items count toward the item limit.
The host budget bounds allocation, sorting, source comparison, and metadata comparison work.
The importer retains the shared trace and allocates an operation index, not one COMM copy per participant.
An import failure publishes no checked trace and changes no RSpace or budget-replay state.

This artifact establishes trace projection and occurrence association, not historical provenance or successful replay.
Actual source preimages, candidate matching, execution context, dependency readiness, and exactly-once completion still require runtime authentication.
The importer has no separate descriptor for unscoped events or direct recorded removals. It cannot skip extra events.
Native activation requires complete coverage of those execution paths or an explicit boundary outside the native trace.

### Replay occurrence ledger

`CheckedNativeOperationTrace::into_replay` creates a ledger with one slot for every operation, including denied operations.
Only a checked trace can create this ledger.
Construction reserves slot and undo storage before any operation starts.
The ledger retains the shared trace instead of copying COMM data for each reservation.

`reserve_current` authenticates the task-local session, causal path, and complete introduction source before reserving a slot.
The path search uses forward lexicographic order without allocating another path.
A reserved or completed slot cannot be reserved again.
Independent slots can remain reserved concurrently and can complete in either order.
The metadata mutex protects short state transitions. It does not remain locked across accounting callbacks or tuple operations.

`authenticate_footprint` must succeed before any budget observation.
It uses the recorder's channel encoding, lexicographic sorting, and deduplication to compute the canonical union of channels and join channels.
Omitted and extra channels fail. Repeated channels and different input orders produce the same canonical footprint.
The caller must supply the actual footprint while the corresponding channel guards remain held.
The reservation checks those inputs but does not itself acquire channel guards or establish their provenance.

Completion distinguishes `Stored`, `Matched`, `DeniedIntroduction`, and `DeniedComm`.
Each reservation checks its actual introduction observation before its actual COMM observation, when a COMM exists.
`observe_comm` also requires the actual COMM source and checks every logical field before it checks budget evidence.
Source mismatch or observation mismatch leaves the local stage and usage unchanged.
The checked operation selects each budget row. The caller cannot substitute a row index.
Observation comparison includes the event identity, stage, authority, measurement, and legacy amount.
The checker reserves comparison work before it reads these fields.
An absent, repeated, altered, or out-of-order observation cannot complete the operation.

| COMM field | Required comparison |
| --- | --- |
| Consumer | Hash, ordered channel hashes, and persistence must match. |
| Producers | Count, order, hashes, channel hashes, and persistence must match. |
| Peeks | Every peek index must match. |
| Repetitions | Every producer identity and its repetition count must match. Missing entries differ from zero entries. |
| External telemetry | Output bytes, determinism flags, and failure flags are not logical source identity. The checked trace owns these fields. |

Predispatch candidates must not inherit post-dispatch telemetry as candidate-selection evidence.
Occurrence-bound telemetry publication remains a separate integration requirement.
Source authentication does not replace spatial matching, commit guards, or prestate payload validation.

### Shared observation construction

Play and typed replay use the same constructors for produce introductions, consume introductions, and COMM measurements.
Construction returns the event identity, event kind, authority, and raw byte measurements.
It does not admit an operation, change budget usage, record an attempt, or suppress a retry.
Admission and replay authentication remain separate operations.

Introduction authority comes from the reducer's registered execution context, not the introduced payload.
The live observer retains its existing authority lookup and fallback registration.
The constructor receives that authority explicitly and does not access the registry.
An empty authority is invalid. A nonempty region with the unit signature remains valid even when its demand is empty.

COMM construction combines the actual continuation and data authorities.
It identifies persistent regions, merges compatible regions, and derives persistent instances from the actual COMM identity.
Conflicting regions remain errors. The shared canonicalization preserves the existing live-observer checks.
Native observations retain raw measurements and set `legacy_amount` to `None`.
Legacy execution still applies its existing byte projection, including its distinct handling of unit-authority COMMs.

Typed replay first checks the recorded source and required stage.
It then reconstructs the observation and authenticates the budget row selected by the reserved operation.
Construction and authentication errors do not advance the stage or completed usage.
An authenticated denied COMM retains its accepted introduction charge, without another admission attempt or an accepted COMM log entry.

These typed methods do not prove that supplied payloads are preimages of the supplied source hashes.
For example, different payloads of equal encoded length can produce identical byte measurements.
The private session must bind actual matched payloads, source identities, and persistence flags before it calls these methods.
Comparison reservations do not cover every construction allocation or payload traversal.
Thus, shared construction alone does not establish complete replay authentication or complete host-work bounds.

`PairedByteReceipts.v` proves preservation of identity, authority, event kind, raw measurements, and native quantities during modeled observation construction.
Rust properties check exact byte measurements over generated payload sizes, channel counts, and authority sizes.
Examples compare actual live-budget records with reconstructed observations and exercise all persistence combinations.
The denied-COMM example covers both trigger types, exact retained usage, and duplicate-completion rejection.
These checks do not execute the complete private RSpace session.

### Candidate preparation

Candidate preparation reconstructs a match from actual RSpace data before tuple mutation.
The prepared value contains the matched data, their store indexes, and the actual COMM.
Produce preparation also retains the waiting continuation and its index.
The caller must retain the channel guards until publication. A prepared candidate must not survive another mutation of those channels.

`NativeCandidateIdentity` separates logical source identity from imported trace telemetry.
The native implementation checks complete producer identities, not the hash-only equality of `Produce`.
It compares the final COMM after spatial matching and commit-guard evaluation.
It computes prospective produce counters without incrementing stored counters.
Counter overflow rejects preparation before tuple mutation.

Preparation does not authorize publication. In particular, an empty legacy replay index does not prevent reconstruction of an actual candidate.
The legacy directive path still requires its accepted-trace binding and budget callback before mutation.
The private native session uses occurrence-owned trace authority and authenticated observations to authorize publication.
The reducer can use this session through its execution interface. The complete funded runtime does not yet use this session.

For example, an incoming produce can trigger a match that selects only previously stored data.
Candidate preparation must retain the actual selected participants. It must not insert the trigger into the COMM solely to obtain a replay binding.
Imported output bytes and failure flags do not select participants. The checked trace supplies those fields after source authentication.

The preparation boundary is not an allocation-free effects plan.
The native session reserves the requested backing for result arrays and retirement indexes before it allocates either array.
Matching, cache fills, counters, and copy-on-write storage still require host-work coverage before native activation.
Preparation tests compare logical tuples, counters, logs, bindings, and waiting counts. These tests do not assert identical cache representation.

The selection proofs quantify over ordered candidate lists after spatial matching.
They establish source authentication, guard acceptance, counter validity, actual membership, and first-eligible selection.
They also prove that telemetry cannot change logical eligibility.
These lemmas specify selection over a supplied list. They do not prove that the Rust matcher enumerates every eligible candidate.

### Reducer execution interface

`ReducerCore` uses `ExecutionSpace` for produce, consume, join lookup, replay identification, and produce-output validation.
This interface has no reset, checkpoint, observer replacement, or direct tuple-removal methods.
The ordinary `DebruijnInterpreter` retains its administration handle and delegates evaluation to the same core.
System-process callbacks use the execution interface. They do not receive an administration handle through `ContractCall`.

The native execution backend takes introduction authority from the runtime registry and passes it to the checked session.
It does not infer introduction authority from a stored payload or fall back to ordinary replay.
Both backends use the existing reduction scheduler. Independent conflict components can still execute concurrently.
Join lookup can report a resource error. The scheduler returns that error for the affected operation without publishing its tuple effect.
The scheduler completes the remaining prepared operations and releases the evaluation boundary after the frontier finishes.

After external-process dispatch, native replay compares the complete returned produce record with the recorded result.
This comparison includes logical identity, persistence, determinism, failure status, and ordered output bytes.
The session reserves comparison work before it examines variable-size output. Hash equality alone does not authenticate an external result.
This post-dispatch check does not use telemetry to select a COMM candidate.

`completed_usage` obtains the private session boundary and requires every checked operation to have completed before it returns the charged amount.
An incomplete session, a restored incomplete checkpoint, or a closed session cannot supply a completed charge.
`completed_evidence` holds the same exclusive session boundary while it constructs an accounting snapshot.
The snapshot contains the checked budget recording, operation journal, and granted measurements in recorded order.
Denied attempts remain in the budget recording but do not enter the granted measurement rows.
Retry references remain in the recording but do not add another measurement row.
The projection preserves distinct fresh occurrences even when their event identifiers repeat.
It reserves traversal work and vector backing before it builds the result. Observation payloads remain shared through immutable references.

The snapshot describes a completed replay at the time of the query.
It does not freeze the session, authorize wallet settlement, or prove that the current tuples still match that historical replay.
After restoration, the previous snapshot remains unchanged, but another snapshot query requires complete replay again.
The environment combines this evidence with authority transitions, evaluation errors, and mergeable effects.
Settlement must also bind the persistent result before it can use the evidence.

Parsed-process tests compare errors, failure classification, final tuples, joins, and completed charges with recorded execution.
The tests include introductions, joined receives, independent sends, denied COMMs, and checkpoint restoration.

This connection does not complete funded-runtime activation.
Wallet settlement still needs the native runtime connection, including the binding between funded results and exported state.
Typed decoding, matching, cache mutation, and cleanup still require complete host-resource bounds.

### Native system environment

`create_native_replay_env` constructs the reducer and its system processes over a checked native replay session.
It shares process definitions, installation templates, dispatch setup, URN bindings, mergeable tags, and external-service adapters with ordinary execution.
An extra process receives the same restricted execution interface as a standard process.
The factory does not rerun registry bootstrap. Its supplied history must contain the required persistent prestate.

Construction installs continuations in a private store before it exposes the session.
An invalid template, a matching prestate datum, or a rejected installation reservation returns an error instead of a session.
The failed constructor drops its private partial state. It does not commit a history root or execute a COMM.
Successful checkpoint restoration preserves installed continuations and their joins.
There is no late installation method on the published native session.

The environment keeps its reducer, runtime budget, host budget, merge tracker, and session private.
Construction rejects a runtime budget or merge tracker with another retained owner.
Evaluation uses the host budget supplied at construction. A caller cannot replace that budget for an evaluation.
Evaluation, checkpoint capture, and restoration require exclusive access to the environment. Parallel reductions within an evaluation retain their shared execution interface.
An interrupted evaluation cannot export completed evidence, capture a checkpoint, restore state, or start another evaluation through that environment.
The caller must discard that interrupted environment.

The runtime budget binds to one replay generation before evaluation.
Authenticated operation observations prepare authority effects before tuple publication.
Accepted fresh COMMs reserve their per-purse demands against the same capacity ledger as pending stack transfers.
Publication commits those demands and accepted byte observations. Cancellation releases the operation's pending demand and observation capacity.
Denied COMMs publish their authority frontier without a debit. COMM retries require an identical, already-published authority event and add no debit.
Completed play counters are not available from a replay-bound budget. The session's completed evidence supplies the checked replay usage and recording.

Environment checkpoints include authority events, realized and reserved demand, byte observations, frontiers, stack births, and introduction-authority bindings.
They also include mergeable channels and evaluation status.
Restoration first validates the session checkpoint and then restores authority state for the same runtime generation.
A generation mismatch prevents reuse. Interrupted restoration also prevents reuse through the environment.
The public environment API does not expose a mutable budget handle.
Complete backing limits for authority snapshot copies remain part of the host-resource contract required before funded activation.

### Native evaluation results

`NativeReplayEnvironment::evaluate` accepts a normalized process and its random seed.
It uses the ordinary interpreter's result assembly and error handling.
The checked replay snapshot supplies the native budget recording, operation recording, and accepted byte observations.
The runtime supplies authority events, realized demand, stack births, and mergeable channels.
The result preserves failure classification from the complete reduction, including failures in parallel branches.

Evaluation captures a coupled checkpoint before execution.
This checkpoint permits restoration when host-resource rejection or incomplete replay prevents a result.
The following table distinguishes a completed process error from an invalid replay attempt.

| Outcome | Result | State and accounting |
|---|---|---|
| Complete successful replay | Complete `EvaluateResult` | Retain replay effects, exact native charges, authority evidence, and mergeable channels. |
| Complete replay with a process error | `EvaluateResult` with errors and their failure summary | Apply the ordinary interpreter's transfer rollback. Preserve accepted COMM evidence. The failure summary controls billability. |
| Host-resource rejection | Zero-cost platform failure without accounting evidence | Restore the pre-evaluation checkpoint if execution started. No retained charge is permitted. |
| Incomplete or mismatched replay | Error, without a completed result | Restore the pre-evaluation checkpoint. Restoration failure cannot authorize settlement. |
| Cancelled evaluation | No result | Prevent reuse of the interrupted environment. |

For example, an accepted introduction followed by a denied COMM retains only the introduction's native charge.
The result includes the denied attempt but excludes a debit for that attempt.
A host-resource rejection instead supplies no accounting evidence, even if execution already published a COMM.

A completed evaluation cannot run again without restoration to a checkpoint captured before evaluation.
Restoring a completed checkpoint does not grant permission to execute the trace again.
An empty trace also requires evaluation before the environment can export completed evidence.

The replay-to-funding tests connect these results to the existing resource and funding checkers.
The fixtures use a nonzero actual price and run original execution and replay with the same schedule.
Generated cases vary the owner count, price, and process payload on a multithreaded runtime.
An independent charge reference includes every weighted resource dimension and authority leaf.
A separate 33-owner example checks that runtime composition does not impose a two-owner limit.
The projection follows these steps:

1. Validate the complete byte measurements and locate their authority purses.
2. Build counted resource demand under the selected acquisition terms.
3. Check resource discharge and compare its usage with replay usage.
4. Project monetary obligations with the complete failure summary.
5. Select and verify the canonical funding allocation.
6. Check each source's native amounts and conservation of its hold.

The fixture compares obligation keys, quantities, amounts, assignments, source amounts, and cursor transitions.
These values must agree for original execution, completed replay, execution after restoration, and persistent export.
Insufficient capacities must reject funding. Nonbillable failure classes must produce no retained monetary charge.
The tests also require balanced resource debits when every source has equal capacity and permission.

These fixtures supply funding eligibility and use fresh acquisition without prepaid resources.
They test allocation and settlement amounts, not signature verification, persistent wallet mutation, or complete up-front funding-family construction.
The [replay funding tests](../../../../rholang/src/rust/interpreter/accounting/native_runtime/checked_operations/trace/replay/session/execution/tests/funding.rs) enforce this boundary.
`NativeReplayAccounting.v`, `PrepaidResourceDischarge.v`, and `SignedPhloFunding.v` specify the evidence projection, resource partition, and monetary conservation contracts.
These rules prevent a second process from reusing the first process's completed journal.
They do not make a result an independent settlement authorization or a persistent state commitment.

### Authority copies and host reservations

Native authority updates change only the purse keys in the current debit.
Each update checks every addition or subtraction before it changes a balance.
Overflow, insufficient authority, or an allocation violation leaves the ledger unchanged.
The implementation retains the existing zero-entry rules and checks pending demand when it admits another reservation.

For each purse, held authority equals published demand plus pending demand.
Publication moves demand from pending to published without changing held authority.
Cancellation subtracts only the cancelled demand from held authority.
Neither operation changes the selected monetary allocation or the price contract.

Preparation reserves host work for ledger lookup, node insertion, publication, and cancellation before tuple mutation.
The tree bound covers a machine-width search path, including later tree growth from other pending operations.
This conservative bound prevents publication from depending on another fallible host reservation.
Publication and cancellation can therefore finish after an unrelated operation exhausts the host budget.
These reservations account for host resources. They do not add economic charges.

Observation storage tracks its paid capacity separately from allocator capacity.
Growth reserves replacement storage and element movement before allocation.
A checkpoint copy retains at most its copied row count as paid capacity.
After restoration, later growth must reserve its storage again.
Unused capacity in the original vector cannot authorize allocation in the copied vector.

Authority checkpoints reserve each map, observation vector, and nested payload before copying the checkpoint.
Result capture reserves authority events, realized balances, stack births, and observation storage.
The environment also reserves mergeable-channel maps before checkpoint and result copies.
Rejected reservations return no completed checkpoint or result and do not change the source accounting state.

The copy traversal uses the Rust representations, not serialized byte counts.
It visits quoted and named terms, compound signatures, guards, injections, expressions, and nested cost annotations.
Exhaustive field patterns and enum matches require updates when the protobuf schema changes.
The traversal reserves its own worklist before growth.
Vector and string bounds use copied lengths. Hash-table bounds use capacity, including empty tables with retained storage.
Tree bounds include node backing and alignment, in addition to nested payloads.
Shared observation references require reference copies, not copies of their immutable payloads.

Legacy byte-event capture in a native result constructs each canonical key once.
An allocation-free heap sort reserves comparison work before each comparison.
The result preserves the existing canonical byte order without repeated key serialization during sorting.

These bounds depend on the pinned standard-library container layouts.
Allocator-instrumented regression tests compare reserved bytes with actual clone allocation requests.
They do not measure allocator fragmentation or establish a process-wide resident-memory ceiling.
Authority-demand construction, matching, cold decoding, and history export require their own complete bounds before native funded activation.

### Native source encoding

Native replay constructs produce and consume identities through a metered encoder.
The encoder uses the existing bincode format, channel-hash order, pattern-byte order, persistence flag, and Blake2b-256 digest.
It does not change the legacy source constructors or the committed event representation.

The runtime inspects typed inputs before source encoding.
The inspection uses the checkpoint field walker, including guards, authority regions, resource stacks, and random state.
Both walker modes reserve the same field visits and byte scans.
Inspection reserves its worklist allocations but does not reserve payload copies that it does not create.
The encoder separately reserves its output allocations.

Each session operation constructs its source once from the owned input values.
The authority resolver receives a shared reference to that source.
The operation uses the same source for readiness checks, authentication, and tuple publication.
The resolver cannot replace the source or change the owned inputs through this interface.
A resolver error occurs before ticket reservation or tuple publication.
This removes duplicate encoding without changing source identities or the operation dependency graph.

Every output write reserves its byte work before copying bytes or updating the digest.
Buffer growth reserves the complete replacement capacity and movement of existing bytes before allocation.
Consume construction reserves its vectors before allocation and comparison work before sorting.
Hash input streams directly into the digest, so construction does not allocate a second combined encoding buffer.
The final digest allocation also requires a reservation.

An exhausted budget returns an error without a source result. Earlier host reservations remain consumed.
Failed construction does not mutate tuple state, operation evidence, or caller-owned inputs.
A rejected writer cannot resume or return a partial result, even if a serializer suppresses its write error.
Each call owns its buffers and digest. Independent calls share only their supplied host meter.

`NativeSourceEncoding.v` proves byte preservation, successful byte-budget bounds, rejection without a result, and identity preservation for equal encodings.
The proof treats hash application extensionally. It does not prove Blake2b or the Rust serializer implementation.
`NativeCheckpointBacking.v` models field visits, worklist allocations, and payload-copy reservations as a finite charge trace.
Inspection preserves verification charges and removes only payload-copy backing from that trace.
Every successful clone budget therefore permits inspection, and each inspection prefix fits the complete trace budget.
The trace abstraction requires the field walker to enumerate every relevant field.
Generated Rust tests check this correspondence for nested inputs, exact dimension budgets, and rejection below each required budget.
An allocation counter checks that inspection pays for its own worklist.
Generated tests compare complete source fields with legacy constructors, including repeated channels and persistence flags.
Boundary tests require exact budgets to succeed and smaller budgets to reject.
Allocator-instrumented tests compare reservations with allocation requests for nested Rholang inputs.
Loom calls the production constructor with two workers and an instrumented shared meter.
Its negative control shows why separate budget checks and updates cannot authorize concurrent buffer allocations.
This model checks the meter interface, not the production host-budget atomics or arbitrary serializer code.

These bounds cover typed field traversal, encoder-owned buffers, sorting, and output writes.
They do not bound arbitrary work or allocations inside a custom `Serialize` implementation before it writes bytes.
The trace proof does not establish a compiler-level correspondence between Rust code and the formal model.
Source consumers, matching, and persistent export remain separate resource obligations.
Native funded activation requires those obligations in addition to source identity compatibility.

### Persistent replay export

`NativeReplayEnvironment::export` consumes the evaluated environment.
It returns a `NativeReplayExport` that pairs a persistent state root with checked native accounting evidence.
The environment rejects export before evaluation, including an empty trace.
This export is a state-and-accounting artifact, not permission to debit wallets.
The funded settlement path must also validate failure classification, authority, funding terms, and receipt ownership.

The session holds its exclusive operation gate through evidence capture, state persistence, and closure.
This gate excludes operation publication and checkpoint restoration during export.
Independent sessions retain their own gates and can export concurrently.
The session uses the existing history repository and its state encoding. Export does not introduce another persistent representation.

The export sequence follows the state ownership contract:

1. Acquire the exclusive session boundary.
2. Check complete replay and capture its accounting evidence.
3. Check the host-work reservation before persistence.
4. Collect the current store changes and commit them through the history repository.
5. Close the session and return its root with the captured evidence.

A failed completeness check or host reservation leaves the live session unchanged.
A panic during persistence invalidates the session and returns no export artifact.
Such failure can leave unreachable content-addressed data. That data does not authorize settlement.
The caller cannot restore a checkpoint, repeat export, or access mutable session state after successful export.
The historical repository can reopen the returned root without the closed session.

For example, an environment captures a checkpoint, completes replay, and then restores that checkpoint.
The environment must complete replay again before it can export state and accounting evidence.
The repeated execution must not retain the discarded attempt's charges.
Concurrent exports of that session can produce only one successful artifact.
Another session can persist a different state through the same backing stores without changing the first artifact.

The export contract relies on successful history writes before root publication.
It does not prove filesystem durability across power loss or the correctness of the storage backend.
The current host reservation does not bound store-change collection, serialization, history writes, or cleanup.
Complete persistence work and allocation bounds remain required before funded-runtime activation.

### Native callback conformance

Replay callbacks use the supplied block and deploy context.
Tests compare SHA-256, Blake2b-256, block-data, deploy-data, and custom callback execution with recorded execution.
Generated tests compose hash and custom callbacks with varied payloads.
They compare exact charges, effects, errors, and restoration, including denied COMMs.
They also compare per-purse authority demand, frontiers, and byte-observation multiplicity, then repeat replay after restoration to detect duplicate charges.
Result comparisons include complete recordings, failure summaries, authority evidence, and integer-add or bitmask mergeable channels.
Rejection tests cover an exhausted budget, checkpoint preparation, and execution interrupted after a published COMM.
Changed block or deploy context must not produce a successfully completed replay.

The initialization proof models complete preparation and publication only after all requests succeed.
It does not prove the Rust installer, external services, payload decoding, or complete allocation bounds.
The per-installation reservation counts installation attempts. It does not bound all allocations inside matching, cache loading, or template construction.
Full funded-runtime activation remains disabled until those bounds and settlement integration are verified.

### Completion publication

Preparation checks complete observation coverage and the exact recorded outcome.
It reserves host work for publication and later undo.
Publication uses preallocated storage and requires no further host-budget reservation.
Dropping a reservation or an unpublished preparation releases only that reservation.
An exhausted publication serial rejects a new reservation without changing the slot.

The ledger owns immutable checked budget evidence, not a second consumption cursor.
Each accepted fresh observation contributes its checked charge to the operation.
A denied observation contributes zero. A retry authenticates the original accepted observation and contributes zero.
An accepted introduction followed by a denied COMM retains the introduction charge when the operation completes.
Cancellation before publication discards the local checks and contributes no completed usage.

Publication updates slot ownership, completed usage, and the undo entry under one mutex.
Completed usage is the sum of published fresh charges, not a new chronological admission budget.
Exact budget-row ownership and nonnegative charges bound every completed subset by the checked total.
Thus, independent publication order cannot cause arithmetic overflow or change the final charge.
Completeness requires every operation and the exact checked total, including operations with zero charge.

An exclusive boundary prevents new reservations during checkpoint, restore, or closure.
Boundary acquisition fails while any reservation remains active.
A checkpoint contains a private ledger identity, an undo cursor, and the serial at that cursor.
Restore validates all three fields before changing state.
A foreign checkpoint or a checkpoint from an abandoned suffix cannot restore the ledger.
Closure permanently prevents further reservations and completeness checks.

Undo follows publication order, not reservation order.
For example, operations A and B can reserve in that order but publish B before A.
A checkpoint after B retains B when restore removes A.
Serials never decrease or repeat after cancellation or restore.
Republishing A therefore cannot make an abandoned checkpoint valid again, even when its cursor matches the new length.

Restore preparation retains the exclusive boundary while its caller prepares related state changes.
Dropping that preparation preserves the ledger and releases the boundary.
Successful restore reverses only the published suffix, using work reserved before publication.
Each undo entry subtracts its recorded usage with the same slot transition.
It can therefore complete after the host budget becomes exhausted.
Completeness requires every slot to be completed, with no active reservation or boundary.

The private native session couples this ledger to an RSpace checkpoint.
Its completed budget usage shares the occurrence ledger transaction and checkpoint rollback.
Native operations reconstruct sources from their inputs, authenticate actual locked footprints, and reconstruct selected candidates before publication.
Declared channel predecessors must complete before an operation reserves its slot.
These mechanisms do not establish complete runtime replay or stage-level retry readiness.
In particular, retry authentication proves equality with certified evidence, not that the original operation has already executed during replay.
The replay scheduler must establish that causal condition before it executes dependent operations.
Requiring the original operation to complete is not generally equivalent to requiring its earlier stage to have published.
Reciprocal stage-level retries can impose cyclic whole-operation completion dependencies.
Dependency waits must occur outside channel guards and the session read gate.
Otherwise, a queued checkpoint writer can prevent a predecessor from acquiring that gate while its dependent retains a read lease.
The runtime must couple these components before it replaces participant-indexed replay bindings or enables native funded ingress.

### Coupled checkpoint contract

`NativeReplayCheckpoint.tla` specifies the boundary between tuple state and the occurrence ledger.
The private native session implements capture and restore for tuple state, metadata, and the occurrence ledger.
Native operation tickets use the session gate and channel guards through publication.
The model separates session entry, channel acquisition, preparation, tuple effects, and ledger publication.
Two workers can execute independent operations concurrently. Workers that share a channel retain channel ownership through publication.
Waiting for a channel still counts as an active session operation.

Checkpoint capture and restore require an exclusive session lease.
The lease excludes both active operations and operations that wait for channel locks.
Capture reads tuple state, counters, and ledger state without changing them.
The legacy soft-checkpoint method drains counters, so the native session must not use that method for mid-epoch capture.
The model's counter-loss control demonstrates this distinction.

Restore first validates session identity and checkpoint ancestry.
All fallible preparation must finish before either state changes.
Tuple state and ledger state then publish under the same exclusive lease, without suspension or a recoverable failure between publications.
Failed preparation preserves both states. Cancellation is permitted before effects, not between publication steps.
Closure prevents further session entry, capture, restore, or epoch replacement.

`CheckedNativeOperationTrace::into_session` consumes the checked trace and creates a private ledger and fresh replay store.
It does not accept an existing ledger or hot store, and it exposes neither backing handle.
Private checkpoint fields prevent substitution of unrelated tuple state beside an authentic ledger token.
Restore consumes its checkpoint instead of cloning the captured state.
Unexpected unwinding after the first publication permanently closes the session.
The publication guard writes the closed flag only when publication does not complete.
Successful publication disarms its own guard but never clears the shared closed flag.
Thus, success cannot reopen a session closed by another publication or an explicit close operation.
The exclusive restore lease still prevents readers from observing intermediate restore state.
The session exposes native produce and consume operations with explicit introduction authority.
It does not expose the mutable backing store or the occurrence ledger.
An incomplete publication closes the session before it invalidates the ledger and notifies dependency waiters.

`NativeReplayAccounting.v` proves closure preservation over arbitrary publication histories and commutation of independent completion results.
Loom imports the production guard and explores concurrent success, failure, and explicit closure.
Its negative control demonstrates why successful completion must not clear the shared flag.
Generated Rust histories check every prefix against the same closure rule. Separate tests check closure before older ticket cleanup and before waiter notification.
These checks establish guard behavior, not atomic tuple mutation or operation-level scheduling.

### Native operation readiness

The checked journal records predecessor indexes in publication order. The checked trace stores operation slots in canonical operation order.
Replay translates each predecessor index into its corresponding trace slot before it checks completion.
Confusing these indexes can incorrectly admit a dependent operation or prevent a ready operation from executing.

Dependency waits hold neither a session lease nor channel guards.
A waiter registers for notification before it reads ledger readiness. Publication, cancellation, boundary release, and invalidation notify waiters.
This order prevents a completed predecessor from notifying between an unsuccessful readiness check and notification registration.

A ready operation acquires the shared session lease and its channel guards.
It then checks predecessor completion again while it reserves the slot under the ledger mutex.
If a restore removed a predecessor, the operation releases its guards and waits again.
Independent operations need no shared predecessor and can execute concurrently.

The direct `reserve_current` entry point uses the same atomic readiness check as the private session.
An incomplete predecessor returns `Dependency` without reserving a slot or changing completed usage.
An active checkpoint boundary returns `Busy`, and a closed ledger returns `Closed`.
Both entry points translate journal indexes before checking dependencies. Neither adds new dependency edges or a global execution order.

For example, journal operation A precedes B on one channel, while canonical trace order places B before A.
B must wait for A's mapped slot, not for its own slot. A checkpoint can complete while B waits.
An unrelated operation C can complete before either operation if its channels and dependencies permit execution.

`NativeOperationJournal.v` proves exact predecessor readiness, independent eligibility, slot-renaming preservation, and the need to recheck after restoration.
`NativeReplayReadiness.tla` separates waiting, notification eligibility, lease acquisition, rechecking, publication, cancellation, and restoration.
Its safe configuration checks three slots with one dependency and one restore.
Three negative controls expose a retained waiting lease, a missing restore recheck, and a direct reservation that bypasses its predecessor.
The model does not prove notification implementation, memory visibility, retry-stage ownership, or unbounded liveness.

Generated direct-entry histories compare reservation eligibility and completed usage with a channel-dependency model before and after restoration.
A retry regression rejects reservation while its predecessor is absent or reserved, then accepts the retry after predecessor publication without another charge.
Loom imports the production ledger and checks predecessor publication, cancellation, independent reservations, and restoration between readiness and reservation.
These tests enforce declared channel dependencies. They do not establish stage-level retry provenance or complete funded-contract replay.

Private-session tests replay real play records through the production ledger and actual RSpace operations.
The outcome matrix covers both triggers, all four persistence pairs, and all four outcomes before and after checkpoint restoration.
Mutation tests reject changed input sources before accepting the original operation.
A two-worker test requires independent matchers to overlap before either returns, then checks tuple state, completion, and restoration.
The tests use the production Rholang matcher and compare returned bindings, not only match existence.
Generated histories vary channel assignments, persistent inputs, and concurrent submission order across up to 23 operations.
They check automatic waiter notification, cancellation before reservation, exact tuple contents, restoration, abandoned checkpoints, and successful execution after restoration.
Persistent repetitions exercise recorded retries without another charge. These tests do not establish all spatial patterns or complete funded-contract replay.

Snapshot capture reserves the fixed shard-array backing before copying shared map roots.
Metadata charges include event vectors, hash bytes, output payloads, and B-tree node backing.
The tree bound follows the standard library in the pinned Rust toolchain.
Each node has eleven key/value slots. An internal node also has twelve child pointers.
Every nonroot node contains at least five entries, and a nonempty root contains at least one entry.
Thus a nonempty tree with $`n`$ entries contains at most $`1+\lfloor(n-1)/5\rfloor`$ nodes.
The reservation includes node headers, pointer fields, unused slots, and conservative alignment padding.
Owned key payloads receive separate charges. Checked arithmetic rejects overflow before copying.

`NativeCheckpointBacking.v` proves the arithmetic bounds from node occupancy and node-layout premises.
Allocator-instrumented tests check the resulting byte bound against cloned standard-library trees, including split nodes, removals, and over-aligned keys.
These tests measure requested node backing, not allocator overhead or process RSS.
A toolchain change requires another check of these layout and occupancy premises.

These checkpoint bounds do not establish complete native allocation or cleanup coverage.
The private session still uses legacy cold decoding and cache-copy paths without native host reservations.
The separate borrowed reader below bounds framing work but does not yet replace that session path.
Copy-on-write map changes and later payload destruction also require their own bounds before native activation.
Economic byte charges do not establish these host-resource bounds.
Restore makes no new host reservation, but that fact alone does not prove that every discarded allocation received a prior cleanup charge.

### Local checkpoint publication

Play and replay checkpoints prepare a history reader before replacing their local history repository.
The play path also retains its event logs and produce counters until reader preparation succeeds.
This order preserves the pending state and original trace when a storage read fails after durable root publication.
It does not undo a root that the backend has already written.

The checkpoint follows these steps:

1. Compute the history checkpoint from the current pending actions.
2. Prepare a reader from that new history repository.
3. If reader preparation fails, return the error without changing the local state.
4. Install the new local history repository after reader preparation succeeds.
5. On the play path, collect the original ordered trace and reset produce counters.
6. Install the new hot store and restore installed continuations.
7. Return the checkpoint root and trace. The replay checkpoint returns an empty trace.

For example, a reader failure must retain ordinary and ordered events in their original local buffers.
A successful retry returns their canonical trace once. A subsequent checkpoint returns no duplicate events.
Repeated failures must preserve the same local repository, root, hot store, pending actions, counters, and installed continuations.
Independent RSpace instances do not need a new shared lock for this ordering repair.

`CheckpointLocalHandoff.v` specifies failure preservation for every local-state projection and arbitrary finite failure prefixes.
It also specifies the original retry trace and commuting publication for independent local states.
`CheckpointLocalHandoff.tla` checks two independent workers with interleaved persistence, reader preparation, failure, retry, and local publication.
Each worker can fail once in the finite TLA+ instance. The Rocq failure-prefix theorem has no fixed repetition bound.
Negative configurations publish local history or drain traces before reader preparation. Both must violate failure-state preservation.

Rust regressions inject a history read error after the backend publishes its root.
Generated cases vary payloads and repeated failures, then compare successful retries with an independent execution without injected failures.
Concurrent instances use distinct local states and compare their roots and retry traces independently.
The tests preserve the backend root after an injected read error instead of assuming durable rollback.

This contract requires exclusive checkpoint ownership of each local RSpace instance.
It does not establish safety for concurrent mutation of that same instance during a checkpoint.
It also does not prove backend crash atomicity, panic recovery, or allocation bounds.
The TLA+ durable-root set records acknowledged roots. It does not model the backend's mutable current-root pointer.

### Native lock preparation

Native replay preserves the existing two-phase striped channel locks.
Each phase acquires unique stripe indexes in ascending order. Independent channel sets remain eligible concurrently.
The native path reserves host work before it allocates hash vectors, stripe-index vectors, or guard vectors.
Each sorting comparison requires a reservation. A failed reservation stops preparation immediately.
All guard-vector storage exists before acquisition starts, so a pending lock request cannot grow that vector.

Consume prepares both phases before acquiring either phase.
Produce holds its first phase while reading joins, as required by the existing lock protocol.
Produce then prepares its second phase from those joins.
An error or cancellation releases every acquired guard through ownership-based destruction.
Neither path changes the existing stripe mapping, channel footprint, or acquisition order.
Legacy execution retains its existing lock implementation.

Default reads use the session's execution host budget. An exhausted budget therefore rejects a read before lock preparation.
`get_data_with_host_work` accepts a separate host budget for read-only inspection of an available session.
The node supplies this inspection budget. It is not wallet funding or a replacement execution allowance.
This API does not replace the execution budget, clear its rejection, reopen a closed session, or authorize further execution.
Rollback tests use this separate budget to inspect restored state while the original execution budget remains rejected.
Both read paths use the same prepared locks and check session availability before and after acquisition.

`NativeLockPreparation.v` specifies exact stripe coverage, duplicate elimination, bounded reservations, and composition under a shared ceiling.
Its read-only contract preserves the execution state and requires the inspection budget's own ceiling.
Its keys represent machine-word hash values after the existing integer conversion.
The model assumes atomic budget reservations. It does not prove the Rust allocator, Tokio mutex implementation, or complete deadlock freedom.
Rust property tests compare prepared indexes with the existing mapping and reject every generated reservation cut.
Async tests check exact held stripes, cancellation, disjoint acquisition, and failure in either phase.
Loom executes the production preparation function with an instrumented shared meter.
Its negative control separates the budget check from its update and demonstrates unpaid concurrent preparation.
These Loom tests do not instrument Tokio channel locks or the production budget implementation.

This preparation contract bounds requested vector backing, not allocator overhead or total process memory.
It does not yet bound generic channel hashing, cold decoding, returned data copies, cache copies, or matching work.
Those operations require separate reservations before native activation.

### Borrowed cold-history reads

`HistoryRepository::native_history_reader` creates a borrowed reader at an explicit state root.
The reader reserves host work before key allocation, storage lookup, hash verification, and framing scans.
It reads radix nodes and cold leaves through `KeyValueStore::with_value`, without constructing a node cache or copying the complete payload.
Borrowed node bytes never cross another storage lookup.
The leaf consumer receives validated record slices only after the entire frame and its content hash pass verification.
The consumer runs inside the backend read guard. It must not perform nested storage access or wait for a writer.
The consumer must reserve its own decoding, copying, and cleanup work before creating owned payloads.

Lookup prefixes and stored leaf tags have different orders:

| Kind | Lookup prefix | Stored tag |
|---|---:|---:|
| Data | 0 | 1 |
| Continuations | 1 | 2 |
| Joins | 2 | 0 |

The reader preserves the existing fixed-width, little-endian encoding and its trailing-byte behavior.
Leaf hashing includes the declared payload length and the complete declared payload, including inner trailing bytes.
It excludes the enum tag and outer trailing bytes.
The reader tries a serialized leaf key only when the raw key is absent.
A malformed raw value cannot select a valid fallback value.

Distinct radix indexes can occur in any order. Duplicate indexes and truncated records fail validation.
Each child step consumes at least one byte of the 33-byte projection key.
Thus one read visits at most 33 radix nodes, without recursion.
A valid node contains at most 41,216 bytes.
Missing nonempty roots and missing referenced leaves return typed errors, not empty results.
This stricter native behavior leaves the legacy reader unchanged.

The reader reserves a 40-byte reusable key buffer before allocation.
Each lookup also reserves the backend key-codec bound: the logical key length plus eight bytes.
The LMDB adapter uses that codec even when it borrows the stored value.
These bounds cover requested Rust reader and key-codec allocations, not allocator overhead, LMDB transaction internals, or mapped pages.
Typed payload decoding, hot-cache copies, copy-on-write updates, and payload destruction require separate reservations.

`HostWorkBudget` maps reader operations, scanned bytes, and backing bytes to their respective host-work dimensions.
Repeated reads consume additional budget. Failed reads do not refund prior work.
Reservation failure prevents the consumer from running. A consumer error remains distinct from a host-budget or storage error.
These host checks neither charge wallets directly nor change economic allocation semantics.

`NativeBorrowedHistory.v` proves span bounds, traversal progress, row bounds, hash framing, and the required consumer checks.
Its proofs describe structural and control contracts, not an extracted Rust parser or complete payload decoder.
Property tests compare generated frames and radix selections with the existing implementations.
Allocator tests measure real in-memory and LMDB reads, including reservation rejection before the first allocation.
The concurrent reader tests use immutable store contents and a shared reservation budget.
They do not establish backend transaction isolation or complete native replay correctness.

The bounded model uses two workers, two channels, two epochs, and three reservation serials per epoch.
It checks every shared or disjoint channel assignment and every accepted or denied assignment within those bounds.
Tuple effects use per-channel counts. These counts represent the checkpoint ownership contract, not the complete spatial matcher or tuple representation.
The model assumes authentic directives, private backing handles, and an infallible publication interval.
It does not prove Rust refinement, crash recovery, scheduler fairness, or atomic budget restoration.

### Explicit RSpace replay directives

`ReplayOperationDirective` identifies the expected operation outcome.
The replay space obtains the directive from the observer captured at operation start, while the existing channel guards remain held.
The directive does not replace source authentication, budget evidence, or scheduler dependency checks.

| Directive | Required runtime observation | RSpace publication |
| --- | --- | --- |
| `Store` | Introduction grant and no match selected by play's procedure | Store the introduction without consuming COMM bindings |
| `RejectedIntroduction` | Exactly `OutOfPhlogistons` from the introduction callback | None |
| `AcceptedComm` | Introduction grant, authenticated candidate, accepted-trace binding, and COMM grant | Apply the match and consume its accepted bindings |
| `RejectedComm` | Introduction grant, authenticated candidate, and exactly `OutOfPhlogistons` from the COMM callback | None |

A replay mismatch must not become an economic denial.
Missing tuples, a rejected guard, an unexpected grant, and another error cannot establish a recorded phlo denial.
Denied operations and mismatches preserve tuples, repetition counters, event logs, and accepted bindings.
Read operations can populate internal caches without changing the logical tuple state.
Accounting callbacks remain tentative until the complete replay contract succeeds.

Candidate reconstruction uses actual tuples, continuations, spatial matches, persistence flags, peeks, and repetition counts.
Play and native replay share deterministic candidate ordering and preserve original store indexes.
Produce reconstruction prepends the incoming datum after ordering stored candidates, as play does.
The triggering produce need not be a selected COMM participant.

Store validation follows play's selection procedure, not an exhaustive search for any possible match.
Consume selects one greedy tuple combination and evaluates its guard.
A guard veto permits Store even if another combination could pass.
Produce evaluates the existing continuation-veto loop across current joins.
A future accepted COMM binding does not prohibit Store before its data exists.

Pre-dispatch journal sources and post-dispatch trace metadata have different roles.
Replay compares actual candidates with the complete pre-dispatch journal fields.
Accepted-trace lookup compares logical source identity, channels, persistence, peeks, and repetition counts.
The accepted trace supplies recorded external outputs and errors when replay returns its producer.
Ordinary hash-only `Produce` equality is insufficient for these checks.

For example, a granted introduction can reach a matching COMM that lacks sufficient phlo.
The denied COMM has no accepted-log binding, but its journal directive still requires reconstruction of the actual match.
Replay then reproduces the exact denial without removing tuples or advancing repetition counters.

Native directive integration remains incomplete.
No ordinary funded runtime currently emits these directives, and native funded ingress remains disabled.
Accepted COMMs without indexed participant bindings still fail closed.
Their complete support requires occurrence-based trace authentication.
The runtime must also authenticate operation sources, actual footprints, dependency-ready scheduling, budget decisions, and complete journal consumption.
An absent directive retains legacy behavior only at the low-level compatibility interface.
It must not provide a fallback after native replay activation.

### Matched tuple retirement

Matched tuple indexes refer to the store before removal.
Both play and replay remove linear tuples in descending index order.
Removing a higher index cannot change an earlier index.
Global descending order also preserves descending order within each channel of a join.
Returned bindings retain their match order, and persistent tuples remain available.

For stored tuples `[A, B, C]`, a match against indexes `0` and `1` must leave `[C]`.
Ascending removal deletes `A`, shifts the remaining entries, and then incorrectly deletes `C`.
Descending removal deletes `B` before `A`, preserving the unmatched tuple.
This rule applies to ordinary execution and both replay paths, independently of funding policy.

The private native session moves selected channels and payloads into the returned results without cloning them.
For `n` selected candidates, preparation reserves `n * size_of::<RSpaceResult<C, A>>() + n * size_of::<(usize, i32)>()` bytes.
Checked arithmetic rejects overflow. Fallible allocation reports a host rejection, not an economic failure.
This bound covers requested array backing. It does not cover allocator overhead, payload decoding, or storage cleanup.

The retirement array stores result positions and original tuple indexes. Only stored, nonpersistent tuples enter this array.
The shared fallible heapsort orders retirement indexes without an auxiliary allocation. Each comparison requires a host-work reservation before execution.
Result bindings retain match order. Retirement borrows channel identities from those results.
All preparation completes before tuple mutation and ledger publication. A preparation error cannot publish a completed operation.
Produce removes the matched continuation first, then retires tuples, then removes joins for every selected candidate, including persistent and incoming candidates.

### Native index resource bounds

The live recorder uses private AVL indexes for operation identities, channel dependencies, budget occurrences, and accepted event identities.
An AVL index is a binary search tree whose child heights differ by at most one at every node.
Nodes occupy stable positions in an append-only arena. Rotations change links, not node positions.
These key orders identify records. They do not determine execution causality or order independent operations.

Each lookup reserves comparison work before reading a key.
This avoids relying on average hash-map behavior when distinct keys cause repeated comparisons.
Preparation reserves insertion storage and rebalancing work before any economic debit or evidence publication.
Prepared insertions own their keys and values. A revision check rejects reuse after another index mutation.
Batch publication reserves searches against the maximum final size and searches again after each insertion, because rotations can change later insertion positions.

A failed charge or retry invalidates its recording before the authority lock is released, unless the failure has a published economic denial.
This prevents another prepared charge from publishing between recording failure and ticket destruction.
Invalidation uses the preparation's own generation, so a stale preparation cannot invalidate a replacement execution.
Ticket destruction still prevents export after preparation abandonment. Incomplete recordings cannot authorize settlement.

Operation start also reserves completion lookup work against the configured maximum operation count and path length.
For maximum node count $`N`$, the comparison-count bound is $`2(\lfloor\log_2(N+1)\rfloor+1)`$.
The bound remains sufficient when independent operations enlarge the index before completion.
Completion checks the session and path limit before traversal. Completion performs no allocation or new reservation.
Each insertion repair level reserves 256 logical operations for link updates, height updates, and at most two rotations.

Backing-storage accounting measures cumulative requested bytes, not physical resident memory.
Each growth requests at least twice the previous logical capacity and enough space for the pending insertion or batch.
Before allocation, the recorder reserves the full new requested backing size and the work needed to move initialized entries.
It allocates a fresh vector, moves those entries, and releases the old vector.
It does not rely on allocator excess capacity or implicit vector growth.
Previously charged storage and newly charged storage cover the requested old/new overlap.

The geometric bound keeps cumulative requested capacity below twice the latest capacity.
Cumulative moved entries remain below that capacity. Allocator metadata and allocation rounding remain outside this logical measure.
Process memory limits therefore remain necessary during verification and deployment.
Source comparison, footprint deduplication, and exported path copies receive separate work reservations.

The budget checker is an arithmetic component, not a complete replay certificate.
The private native session connects checked evidence to rejected-match reconstruction and declared channel-dependency readiness.
The complete funded-runtime adapter does not yet use that session.
Complete replay additionally requires authenticated source reconstruction, persistent retry handling, peek restoration, rollback ownership, and full operation-evidence consumption.
Native funded ingress must remain disabled until those requirements and the complete producer contract hold.

### Runtime authority correspondence

The phlo total and the authority ledger describe different obligations. Matching only the phlo total does not establish replay correctness.
Each accepted COMM must also contribute its authority demand before dependent execution can use the remaining allocation.
Replay must preserve event identity, authority regions, per-purse realized demand, pending reservations, and rejected authority frontiers.

Let $`P`$ contain published event identities and $`R`$ contain pending event identities.
Let $`d(e,p)`$ denote the authority demand of event $`e`$ on purse $`p`$.
The required per-purse relationships are:

```math
\operatorname{realized}(p)=\sum_{e\in P} d(e,p),
\qquad
\operatorname{reserved}(p)=\operatorname{realized}(p)+\sum_{e\in R} d(e,p).
```

The identity sets must be disjoint and must not contain duplicates.
Preparation checks the complete reserved amount against the allocation, including reservations from concurrent operations.
Publication moves a demand from pending to realized without increasing the reserved amount.
Cancellation removes only the canceled reservation. It must preserve other operations' published and pending demands.
Independent operations can prepare concurrently when their combined demands fit the allocation.

A replay checkpoint must couple authority state with tuple state and operation completion state.
Restoration must restore all three views at an exclusive, quiescent boundary.
An abandoned preparation cannot leave a reusable session with an unexplained authority debit.
These requirements apply during execution, not only when the runtime exports a final accounting snapshot.

Replay checkpoint restoration differs from failed-deploy transfer rollback.
Transfer rollback removes materialized stack births and their reservations. It retains charges for attempted COMMs.
Neither operation independently authorizes wallet settlement or a refund.

Verification separates these obligations:

| Artifact | Established contract | Boundary |
| --- | --- | --- |
| `NativeAttemptReplay.v` | Exact prefix decisions, bounded usage, unique occurrences, measurement equality, and permutation preservation | Natural-number arithmetic assumes the supplied measurement function and source interpretation. |
| `NativeOperationJournal.v` | Exact publication coverage, fresh and retry interval rules, all four stage-order combinations, explicit denial lifecycle, and declared-conflict preservation | Unbounded natural-number proofs specify the structural importer contract. Rust tests check correspondence, not a machine-checked refinement proof. Source authentication, actual footprints, and matching remain separate. |
| `NativeTraceOwnership.v` | Executable parser soundness and completeness, exact event coverage, rejected-operation projection, disjoint event ranges, exclusive reservation, and independent occurrence updates | Unbounded list proofs specify the projection and occurrence contracts. Rust property tests check projection and source fields. Runtime tickets and checkpoint integration remain separate. |
| `NativeReplayDirective.v` | Exact directive outcomes, required candidate and binding checks, rejection atomicity, Store binding preservation, and independent state updates | The specification treats candidate authentication as a premise. It does not prove the Rust matcher or scheduler. |
| `NativeDatumRetirement.v` | Descending removal preserves original indexes. Exact selection excludes incoming and persistent tuples. Requested result-array backing bounds compose across batches. | Unbounded list proofs specify retirement and backing contracts. Rust properties compare selection, match order, backing requests, and survivors. These proofs do not establish total heap bounds. |
| `NativeReplayDirective.tla` | Guarded candidate snapshots, shared-channel exclusion, independent acquisition, exact denial, and publication ordering | Two workers and two channels cover shared and independent assignments. Negative configurations expose premature publication and denial without a candidate. |
| `NativeReplayLedger.tla` | Exclusive slot ownership, exact outcomes, publication-order undo, checkpoint identity, stale-token rejection, and independent progress | Two workers, two slots, two epochs, and three reservation serials per epoch. Negative controls expose cursor-only restore, nonexclusive boundaries, and incorrect outcomes. RSpace and budget state remain outside this model. |
| `NativeReplayAccounting.v` | Nonnegative subset bounds, publication permutation, exact suffix subtraction, stage coverage, and zero-charge retries | Proofs assume checked ownership, authenticated observations, and a bounded complete charge sum. They do not prove candidate provenance or Rust refinement. |
| `NativeReplayAuthentication.v` | Exact logical COMM equality, canonical footprints, declared conflicts, and first-eligible candidate selection independent of telemetry | Hashes and canonical channels are abstract identities. Candidate selection starts after spatial matching. Encoding, matching, and guarded acquisition require implementation evidence. |
| `NativeReplayAuthentication.tla` | Exact footprint and COMM prerequisites, channel exclusion, protected publication, cancellation, and independent acquisition | Two workers and two channels cover nonempty shared and disjoint footprints. Negative controls bypass footprint or source checks. This model does not establish the native runtime connection. |
| `NativeReplayAccounting.tla` | Staged observation coverage, exclusive publication, exact completed usage, cancellation, and suffix rollback | Two workers, two slots, three starts, and four lifecycle outcomes. Four negative controls expose omitted stages, repeated stages, retry charges, and missing usage rollback. Source provenance and retry readiness remain outside this model. |
| `NativeReplayAuthority.tla` | Per-purse publication and reservation equations, shared capacity, cancellation, coupled authority restore, and independent preparation | Two workers, two purses, four events, and four starts. Negative controls expose missing debits, stale authority after restore, and pending-reservation overdraw. Authenticated grants and unique identities are premises. This model does not prove full runtime integration. |
| Authority projection proofs and runtime contract tests | Arbitrary-purse additive conservation, cancellation isolation, permutation, exact suffix restoration, and complete identity coverage | Rocq proofs assume unique ownership and sufficient total allocation. Production tests cover mixed histories over one through 32 purses and threaded COMM/transfer competition. Native replay correspondence remains a separate integration requirement. |
| `NativeReplayCheckpoint.tla` | Coupled snapshot consistency, tuple/ledger agreement, counter preservation, failed-preparation isolation, channel ownership, and closed-session exclusion | Two workers and two channels separate channel waits, effects, publication, capture, and restore. Five negative controls check unlocked capture, store-only restore, premature release, failed-preparation mutation, and drained counters. The model does not prove Rust refinement or native operation integration. |
| `NativeReplayReadiness.tla` | Dependency safety, exact completion, independent eligibility, and checkpoint eligibility during dependency waits | Three slots, one dependency, and one restore. Negative controls retain a waiting lease or omit the readiness recheck. Notification implementation and stage-level retries remain outside this model. |
| `NativeCheckpointBacking.v` | Node-count, layout-padding, allocation-sum, payload, clone/drop traversal, and overflow bounds | The proof requires standard-library occupancy and layout premises. Allocator tests check requested node backing. Cold reads, copy-on-write payloads, allocator overhead, and complete cleanup coverage remain outside this proof. |
| `NativeLockPreparation.v` | Exact stripe membership, unique locks, reservation limits, and shared-budget composition | Rust properties compare the existing stripe mapping. Async tests check guard release and independent acquisition. Loom checks preparation with an instrumented atomic meter, not Tokio locks. |
| `CheckpointLocalHandoff.v` and `CheckpointLocalHandoff.tla` | Local state survives reader failure after persistence. Retry preserves the original trace. Independent instances can publish concurrently. | Rocq covers arbitrary finite failure prefixes. TLA+ checks two workers and two early-publication negative controls. Rust tests inject real history-reader failures and compare roots, traces, and local state. |
| `NativeBorrowedHistory.v` | Checked spans, strict key consumption, bounded row counts, hash slices, and consumer prerequisites | Structural contracts cover framing and control flow, not full Rust parser refinement or typed payload materialization. |
| Borrowed reader tests | Legacy framing correspondence, malformed input rejection, reservation ordering, exact fallback selection, and allocation bounds | Generated frames and radix nodes exercise the production parser. In-memory and LMDB allocator tests exclude transaction internals and consumer allocations. |
| Borrowed reader Loom tests | Concurrent consumers require every read reservation | Two production readers share an instrumented meter with zero through ten grants, with three preemptions by default. A non-atomic meter provides a negative control. Store contents remain immutable. Backend locks and the production host-budget implementation are not instrumented. |
| Native ledger Loom tests | Duplicate reservation exclusion, weighted publication and cancellation, exact usage rollback, and boundary races with restore or closure | Tests include the production state-transition source under Loom mutexes. They do not instrument the complete runtime or establish RSpace checkpoint atomicity. |
| Native replay Loom model | Candidate selection and descending retirement remain in one guarded transaction | Two workers share indexed tuple storage. Negative tests expose ascending removal and stale indexes after early unlock. This model does not instrument the production channel-lock implementation. |
| `NativeAttemptReplay.tla` | Concurrent budget reservation and decision-preserving replay | Three workers each make one attempt. The model excludes RSpace state. |
| `NativeAttemptConflicts.tla` | Necessary read/write ordering, independent replay eligibility, state agreement, and completion under weak fairness | Four operations access three existing cells with separate lock acquisition and completion. The model excludes dynamic creation, cancellation, and full scheduler behavior. |
| `NativeOperationLifecycle.tla` | One observer per operation, denial without mutation, and completion that preserves the original result | Two operations interleave with observer replacement. The model abstracts matching and store effects. Unsafe controls expose observer switching and fallible completion. |
| `NativeOperationEvidence.tla` | Every exported rejection has a denied budget decision, and every exported success has the required grants | Two operations interleave introduction, COMM decisions, arbitrary completion results, and export. The model assumes budget decisions are authentic. An unsafe control permits rejection without denial. |
| `NativeIndexBounds.v` | AVL lookup bounds, completion after growth, geometric storage and movement bounds, and failed-preparation publication exclusion | Proofs cover mathematical balanced trees and natural-number capacities. They assume that Rust preserves tree structure and follows the checked preparation contract. |
| `NativeIndexPublication.tla` | Work and storage preparation precede publication, failed preparation prevents publication, and debits match published entries | Two workers share the existing authority lock. Ticket destruction can follow lock release. Unsafe controls expose early publication and invalidation delayed until ticket destruction. The model abstracts tree rotations and allocator behavior. |
| `NativeBudgetPublication.tla` | Generation isolation, accepted usage, and retries that reference accepted attempts without another debit | Three workers interleave preparation, publication, and reset over two identities. The model excludes matching and allocation failures. |
| `NativeBudgetCapture.tla` | Capture cannot export an abandoned or pending preparation | One preparation interleaves invalidation and completion with capture reads. The model abstracts memory visibility. |
| `NativeOperationBudgetOrder.tla` | Interval containment and dependency inequalities preserve continuation and frontier order under row and interval mutation | Two independent roots and one child interleave start, charge, and completion. The model assumes the actual operation membership and dependency edges. Source and frontier authentication remain separate obligations. |
| Budget capture Loom model | Acquire and release ordering prevents export after preparation abandonment. Recording failure prevents later publication. | A ticket and reader exercise capture ordering. Two workers exercise recording failure, lock release, ticket destruction, and publication. Negative controls demonstrate unsafe read order and delayed invalidation. These models do not execute the complete runtime. |
| Native runtime recorder tests | Accepted and denied rows, exact successful replay occurrences, compatible retries, generation isolation, and host-work rejection | Generated histories retain failed tickets while another preparation attempts publication. Allocation failure and mismatched retries leave usage unchanged after failure. These tests do not establish rejected-match reconstruction. |
| Operation journal tests | Completion follows the evidence model, each budget publication has one owner, and conflicting operations preserve budget order | Exhaustive completion cases exercise runtime callbacks. Generated footprints cover dependencies and persistent retries. Concurrent examples permit independent operations to overlap. These tests do not authenticate externally supplied journals. |
| Structural journal tests | Complete coverage, fresh/retry stage order, lifecycle rules, declared dependencies, and bounded import | Tests check real successful and denied execution exports, generated histories, and mutated evidence. Independent operation orders remain valid. A boundary test accepts structurally consistent changes that still require RSpace authentication. |
| RSpace directive tests | Directive truth table, full source fields, guards, candidate order, external results, repeated denials, and state preservation | Tests cover both triggers, persistent tuples and continuations, peeks, repeated channels, and concurrent denial attempts. These tests do not establish complete native budget replay. |
| RSpace candidate tests | Actual matching, state preservation, repeated-channel indexes, counter overflow, guard rejection, telemetry separation, and unselected produce triggers | Tests use the shared preparation engine. Candidate reconstruction does not authorize publication or establish host-work bounds. |
| Native trace import tests | Complete projection, original journal indexes, source mutations, telemetry, rejected slots, equal COMMs, and host limits | Generated journals cover all four economic lifecycles and variable causal paths. Tests compare slot order against the production `OperationOrder` comparator. |
| Native replay ledger tests | Exact denial stage, complete actual observations, exclusive completion, checkpoint rejection, host-work preparation, and exact rollback charges | Generated histories compare slot availability, ancestry, and usage with independent references. Examples cover observation mutations, retry charges, cancellation, serial exhaustion, full-width arithmetic, duplicate reservations, and host exhaustion. |
| Shared observation tests | Exact raw measurements, explicit introduction authority, persistence combinations, legacy compatibility, and denied-COMM charges | Typed tests authenticate observations against recorded budget evidence. The private session must independently establish source-to-payload correspondence. |
| Publication guard tests | Successful completion cannot clear closure, and incomplete publication closes the session | Loom imports the production guard and includes an unsafe-reset control. Properties cover arbitrary generated histories. These tests do not establish atomic tuple publication. |
| Native session tests | Coupled restore, nondestructive capture, rejected preparation, waiting leases, closure, publication unwind, and metadata backing | Generated histories use real RSpace tuple state with a test ledger. Separate adapter tests use the production ledger. They do not establish complete native replay or cold-read metering. |
| Native operation integration tests | Actual source reconstruction, returned binding parity, guarded publication, predecessor-slot mapping, dependency waiting, independent overlapping matches, cancellation, persistent retries, and restoration | Tests use real play records, the production ledger, and the Rholang matcher. They cover four outcomes, every persistence pair, and generated concurrent histories, not complete funded Rholang execution. |
| Datum retirement tests | Exact matched values and exact remaining tuples | Generated subsets cover both triggers, persistent data, mixed-channel joins, ordinary replay, and native accepted replay. |
| Prepared native results | Exact retirement membership, unchanged match order, checked backing, and no channel or payload clones | Generated tests compare an independent selection reference. Failure injection rejects every reservation boundary. Shared-sort properties check ordering and permutation preservation after comparison failure. |
| Native index tests | Search results match a reference map, links and heights remain valid, prepared publication needs no further reservations, and growth preserves stable slots | All insertion orders of six distinct keys cover rotation cases. Generated histories cover replacements and batches. These checks are not a machine-checked refinement proof of Rust arena operations. |
| Budget trace properties | Full-width arithmetic, mutated decisions, exact observations, and generated replay permutations | These tests exercise the arithmetic checker, not the complete runtime. |
| Budget trace Loom test | Concurrent consumption preserves recorded decisions under the ownership mutex | Two workers exercise the checker. This test does not cover RSpace locks. |
| RSpace observer properties | Exact state, committed-event counts, and lifecycle results across generated accepted and rejected operation sequences | Generated sequences use three channels and nonpersistent operations. Separate examples cover persistence, peeks, joins, and observer replacement. |
| Observer ownership Loom model | Each concurrent operation retains its captured observer while another thread replaces the configured observer | Two operations use an abstract observer slot with a preemption bound of two. This model does not execute RSpace or its locks. |

Sources:

- [Budget trace checker](../../../../rholang/src/rust/interpreter/accounting/native_phlo_rules/execution/budget_trace.rs)
- [Structural journal contract](../../../../formal/rocq/cost_accounted_rho/theories/NativeOperationJournal.v)
- [Trace ownership importer](../../../../rholang/src/rust/interpreter/accounting/native_runtime/checked_operations/trace.rs)
- [Trace ownership tests](../../../../rholang/src/rust/interpreter/accounting/native_runtime/tests/checked_trace.rs)
- [Trace ownership contract](../../../../formal/rocq/cost_accounted_rho/theories/NativeTraceOwnership.v)
- [Replay occurrence ledger](../../../../rholang/src/rust/interpreter/accounting/native_runtime/checked_operations/trace/replay.rs)
- [Replay ledger tests](../../../../rholang/src/rust/interpreter/accounting/native_runtime/tests/replay_ledger.rs)
- [Concurrent ledger model](../../../../formal/tlaplus/cost_accounted_rho/NativeReplayLedger.tla)
- [Replay accounting proofs](../../../../formal/rocq/cost_accounted_rho/theories/NativeReplayAccounting.v)
- [Concurrent replay accounting model](../../../../formal/tlaplus/cost_accounted_rho/NativeReplayAccounting.tla)
- [Replay source-authentication proofs](../../../../formal/rocq/cost_accounted_rho/theories/NativeReplayAuthentication.v)
- [Concurrent source-authentication model](../../../../formal/tlaplus/cost_accounted_rho/NativeReplayAuthentication.tla)
- [Coupled checkpoint model](../../../../formal/tlaplus/cost_accounted_rho/NativeReplayCheckpoint.tla)
- [Checkpoint backing proof](../../../../formal/rocq/cost_accounted_rho/theories/NativeCheckpointBacking.v)
- [Borrowed history proof](../../../../formal/rocq/cost_accounted_rho/theories/NativeBorrowedHistory.v)
- [Borrowed history reader](../../../../rspace++/src/rspace/history/native_reader.rs)
- [Borrowed history tests](../../../../rspace++/src/rspace/history/native_reader/tests.rs)
- [Borrowed history Loom tests](../../../../rspace++/tests/native_history_loom.rs)
- [Private native session](../../../../rspace++/src/rspace/replay_rspace/native_session.rs)
- [Checkpoint backing reservation](../../../../rspace++/src/rspace/replay_rspace/native_session/backing.rs)
- [Production ledger Loom tests](../../../../formal/loom/cost_accounting/tests/loom_native_replay_ledger.rs)
- [Structural journal checker](../../../../rholang/src/rust/interpreter/accounting/native_runtime/checked_operations.rs)
- [Structural journal tests](../../../../rholang/src/rust/interpreter/accounting/native_runtime/tests/checked_operations.rs)
- [Replay directive implementation](../../../../rspace++/src/rspace/replay_rspace/native_directive.rs)
- [Candidate preparation](../../../../rspace++/src/rspace/replay_rspace/native_candidate.rs)
- [Shared observation construction](../../../../rholang/src/rust/interpreter/accounting/observation_construction.rs)
- [Typed observation tests](../../../../rholang/src/rust/interpreter/accounting/native_runtime/tests/observation_construction.rs)
- [Publication guard](../../../../rspace++/src/rspace/replay_rspace/native_session/publication.rs)
- [Publication guard Loom tests](../../../../formal/loom/cost_accounting/tests/loom_native_publication_closure.rs)
- [Candidate preparation tests](../../../../rspace++/src/rspace/replay_rspace/native_candidate/tests.rs)
- [Replay directive tests](../../../../rspace++/tests/comm_observer_tests/native_directive.rs)
- [Replay directive proof](../../../../formal/rocq/cost_accounted_rho/theories/NativeReplayDirective.v)
- [Concurrent directive model](../../../../formal/tlaplus/cost_accounted_rho/NativeReplayDirective.tla)
- [Native replay Loom model](../../../../formal/loom/cost_accounting/tests/loom_native_replay_directive.rs)
- [Tuple retirement proof](../../../../formal/rocq/cost_accounted_rho/theories/NativeDatumRetirement.v)
- [Tuple retirement tests](../../../../rspace++/tests/datum_retirement_tests.rs)
- [Budget trace tests](../../../../rholang/src/rust/interpreter/accounting/native_phlo_rules/execution/tests/budget_trace.rs)
- [Prefix arithmetic proofs](../../../../formal/rocq/cost_accounted_rho/theories/NativeAttemptReplay.v)
- [Reservation model](../../../../formal/tlaplus/cost_accounted_rho/NativeAttemptReplay.tla)
- [Conflict-order model](../../../../formal/tlaplus/cost_accounted_rho/NativeAttemptConflicts.tla)
- [Operation lifecycle model](../../../../formal/tlaplus/cost_accounted_rho/NativeOperationLifecycle.tla)
- [Operation evidence model](../../../../formal/tlaplus/cost_accounted_rho/NativeOperationEvidence.tla)
- [Operation journal](../../../../rholang/src/rust/interpreter/accounting/native_runtime/operations.rs)
- [Native index](../../../../rholang/src/rust/interpreter/accounting/native_runtime/index.rs)
- [Native index bounds](../../../../formal/rocq/cost_accounted_rho/theories/NativeIndexBounds.v)
- [Native index publication model](../../../../formal/tlaplus/cost_accounted_rho/NativeIndexPublication.tla)
- [Native index tests](../../../../rholang/src/rust/interpreter/accounting/native_runtime/tests/index.rs)
- [Operation journal tests](../../../../rholang/src/rust/interpreter/accounting/native_runtime/tests/operations.rs)
- [Operation source tests](../../../../rholang/src/rust/interpreter/accounting/native_runtime/tests/operation_sources.rs)
- [Budget recorder](../../../../rholang/src/rust/interpreter/accounting/native_runtime/recording.rs)
- [Budget publication model](../../../../formal/tlaplus/cost_accounted_rho/NativeBudgetPublication.tla)
- [Budget capture model](../../../../formal/tlaplus/cost_accounted_rho/NativeBudgetCapture.tla)
- [Budget capture memory-order model](../../../../formal/loom/cost_accounting/tests/loom_native_budget_capture.rs)
- [Operation budget-order model](../../../../formal/tlaplus/cost_accounted_rho/NativeOperationBudgetOrder.tla)
- [Native runtime recorder tests](../../../../rholang/src/rust/interpreter/accounting/native_runtime/tests/recording.rs)
- [RSpace observer regressions](../../../../rspace++/tests/comm_observer_tests.rs)
- [RSpace observer properties](../../../../rspace++/tests/comm_observer_tests/properties.rs)
- [Observer ownership model](../../../../formal/loom/cost_accounting/tests/loom_native_operation_observer.rs)
- [Denied-COMM replay regression](../../../../casper/src/rust/rholang/runtime/envelope_tests/native_replay_failure.rs)

### Complete producer contract

The matcher checks supplied evidence. It does not establish where that evidence originated.
Complete native settlement also requires the following producer contract:

1. Preserve raw introduction, delivery, and trace quantities at their accepted observation points.
2. Retain event identity, authority, multiplicity, and evaluation ownership with those quantities.
3. Represent large byte quantities as counted resources, without one allocation per byte.
4. Authenticate prepaid class, original acquisition terms, and backing provenance at the captured state root.
5. Preserve that provenance through issuance, transfer, consumption, failure, and rollback.
6. Derive all five resource partitions from those authenticated inputs.
7. Reproduce the same evidence during independent replay before atomic settlement.

Retained resource creation requires a separate authenticated output in addition to those consumption partitions.
The [retained-acquisition contract](resource-units-and-measurement.md#resources-acquired-for-later-execution) defines its separate monetary obligation and failure behavior.
Matching that output does not prove that its physical resources exist or have authorized backing.

Current `CostStack` cells contain signatures, not resource classes or acquisition terms.
Current `AuthorityByteEvent` records contain weighted amounts, not separate raw delivery and trace quantities.
The interpreter now retains those quantities separately as [raw byte observations](raw-byte-observations.md).
Neither representation alone satisfies the producer contract.
Current prices must not replace missing prepaid terms, and wallet balances must not become synthetic prepaid resources.

Protected receipt metadata can retain provenance without creating another spendable ledger or changing historical protobuf encodings.
The [receipt storage contract](prepaid-receipt-storage.md) defines protected channels, exact replacement, bounds, and checkpoint ownership.
Receipt mutation must share the existing owned checkpoint with stack and wallet effects.
Missing or incompatible provenance in the activated economy must reject, rather than silently use legacy or current-price defaults.
The [resource measurement contract](resource-units-and-measurement.md) and [prepaid provenance contract](conversion-provenance-and-refunds.md) define these required boundaries.

Implementation and regression sources:

- [Matcher](../../../../rholang/src/rust/interpreter/accounting/phlo_execution/outcome_match.rs)
- [Matcher properties](../../../../rholang/src/rust/interpreter/accounting/phlo_execution/outcome_match/tests.rs)
- [Signed-family regressions](../../../../rholang/src/rust/interpreter/accounting/phlo_execution/consent/tests/wire_controls_family.rs)
- [Quantity-capture properties](../../../../rholang/src/rust/interpreter/accounting/phlo_execution/consent/tests/wire_controls_family/capture_quantities.rs)
