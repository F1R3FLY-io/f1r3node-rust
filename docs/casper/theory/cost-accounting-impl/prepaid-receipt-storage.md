# Prepaid receipt storage

## Purpose and trust boundary

A prepaid receipt retains resource provenance in the same RSpace state as its associated resource stacks.
The receipt is metadata, not another spendable balance.
Its presence alone does not authorize issuance, consumption, conversion, or withdrawal.

The native funding producer must validate the receipt's resource class, original acquisition terms, backing lineage, quantity, and current authority.
It must also connect that receipt to the actual stack at the captured state root.
Opaque byte storage does not establish those economic properties.
The [provenance contract](conversion-provenance-and-refunds.md) defines them.

The [original-terms check](genesis-resource-policy.md#original-acquisition-terms) validates a receipt schedule against the adopted policy without replacing its acquisition price.
Its checked result establishes policy compatibility, not backing or spend permission.
The native producer must retain that distinction when it constructs resource partitions.

The [complete execution identity](observed-funding-outcome.md#complete-execution-identity) includes five resource partitions.
Receipt storage supplies authenticated state for that producer.
It does not construct those partitions or activate funded admission by itself.

## Protected representation

Each receipt channel contains three elements:

1. The system authentication token.
2. The domain `f1r3node:prepaid-resource-receipt:v1` as bytes.
3. The 32-byte receipt storage key.

The native implementation constructs the system token directly.
A user-supplied string or byte array with the token's name is not that token.
Code with system authority remains within the trusted computing base.

An absent channel has no receipt.
A present channel must contain exactly one nonpersistent datum with one nonempty byte array.
Extra process fields, random state, cost authority, or cost-stack metadata invalidate that representation.
Duplicate values also invalidate it.
The maximum value size applies to reads and replacement inputs.

### Source buckets and occurrence identity

A **source bucket** contains the receipt records for all live stack occurrences with one RSpace produce hash.
The produce hash commits to the channel, datum, and persistence flag.
Identical stored occurrences can share that hash.
Hash-based identity requires the existing collision-resistance assumption.

`PurseStack.instance_id` distinguishes occurrences within one inventory snapshot.
It derives from the produce hash and an occurrence ordinal.
Removing an earlier identical occurrence can change that ordinal for a surviving occurrence.
Therefore, this identifier cannot serve as a persistent receipt storage key.

For example, two identical stacks have snapshot ordinals zero and one, with distinct acquisition records A and B.
After consumption of A, the surviving stack has ordinal zero.
An ordinal-keyed receipt lookup would miss B or select stale metadata from A.
The source bucket retains B under the unchanged produce hash instead.

`PrepaidReceiptBucket` uses these fields:

| Field | Representation and purpose |
| --- | --- |
| Domain | The bytes `f1r3node:prepaid-source-bucket:v1`. |
| Source | The complete 32-byte RSpace produce hash. |
| Count | An unsigned 64-bit big-endian occurrence count. |
| Records | Nonempty receipt byte strings in nondecreasing byte order. Equal records remain separate occurrences. |

The domain, source, and each record use the existing length-prefixed phlo wire format.
The storage key hashes `f1r3node:prepaid-source-bucket-key:v1` followed by the source hash.
The decoder rejects malformed widths, excessive counts, empty records, noncanonical order, truncation, and trailing bytes.
Count, per-field bytes, and total bytes have separate limits.
The constructor checks these limits before copying record references or sorting them.

Each bucket must match the captured physical source and its complete live occurrence count.
Sorting establishes canonical representation. It does not authorize selection of an acquisition record.
The funding proof must establish that the selected record satisfies the actual obligation and its ownership restrictions.
Byte equality does not establish independent backing for two records.
Native issuance must justify every occurrence, including equal records.

Removal selects one record from the current complete bucket and retains every other record.
The resulting bucket has one fewer occurrence and the same storage key.
An out-of-range selection changes nothing.
The caller uses exact old-value comparison when it publishes the replacement.
Snapshot indices must not survive that replacement as independent persistent references.

An empty bucket can exist in temporary computation.
The native lifecycle removes its protected datum when no physical occurrence remains.
Consumption that produces a stack tail changes the physical source hash.
The lifecycle must move the surviving cell provenance to the tail's source bucket within the common stack-and-receipt checkpoint.
It must not treat that move as new acquisition or replace original terms with current prices.
The bucket codec alone does not perform this lifecycle or validate record contents.

### Ordered cell records

`OrderedPrepaidCells` encodes the provenance records for one physical stack occurrence.
The domain is `f1r3node:prepaid-stack-cells:v1`.
The domain uses the phlo length-prefixed byte format, followed by an unsigned 64-bit big-endian cell count.
Each cell record is a nonempty length-prefixed byte string.
Cell order matches physical stack order. The codec does not sort or combine cells.

A stored record must contain at least one cell.
The decoder checks cell count, individual field size, total size, truncation, and trailing bytes before exposing the ordered view.
`split_consumed` accepts only a positive count within that view.
It returns the exact consumed prefix and unchanged suffix.
It does not infer or replace acquisition terms inside either sequence.

This container preserves cell provenance bytes without interpreting their economic contents.
The funding producer must authenticate those contents and their correspondence to actual resources before it authorizes a draw.
Codec acceptance alone cannot establish backing, authority, class compatibility, or a valid purchase.

### Native cell contents

A native cell record describes one resource unit and its original acquisition.
It records general SystemVault funds, not validator fuel or an unexecuted exchange quote.
The acquisition schedule identifies the network, shard, settlement asset, integer unit, and decimal scale.
[`NativePrepaidCell`](../../../../casper/src/rust/util/rholang/costacc/prepaid_receipts/cell.rs) checks this record against the adopted policy.
A separately completed exchange supplies native funds before acquisition. This record does not reverse that exchange during a later refund.

| Field | Meaning |
| --- | --- |
| Domain | `f1r3node:native-prepaid-cell:v1`. |
| Genesis root | The approved genesis state that defines the resource policy. |
| Acquisition root | The original pre-state for acquisition. |
| Deploy identity | The original retained, signed deploy identity. |
| Birth source | The original RSpace produce hash for the stack. |
| Birth ordinal | The original cell position within that stack. |
| Resource | The complete original location, class, acquisition schedule, and authority tree. |
| Contributions | Strictly custody-key-ordered native amounts from the original physical sources. |

Root, deploy, source, and custody identities contain 32 bytes each.
The domain, identities, and complete resource use length-prefixed phlo byte fields.
The birth ordinal and contribution count use unsigned 64-bit big-endian integers.
Each contribution contains a length-prefixed custody key followed by an unsigned 64-bit big-endian amount.
The native domain fixes the custody role to general funds. An equal numeric validator-fuel balance cannot replace those funds.

Let $`a`$ denote the number of Ground and Quote leaves in the resource authority tree.
Let $`w`$ denote the original class weight, and let $`p`$ denote the original acquisition price.
For contribution amounts $`d_i`$, record consistency requires:

```math
\sum_i d_i = a w p.
```

Every stored contribution must be positive. Duplicate custody keys and reordered keys fail validation.
There is no two-wallet limit. Configured source-count and byte limits bound the representation.
The decoder checks counts against remaining input before allocating contribution entries.
The class must exist, the schedule must match the adopted policy, and the genesis root must match the adopted genesis.
Checked arithmetic rejects weighted quantities or acquisition totals outside the unsigned 64-bit range.
Each contribution must fit the native nonnegative signed-integer range.
The aggregate can exceed that per-wallet limit when multiple valid wallets supply it.

The amount does not include the flat deploy fee or a new charge for consuming an already acquired cell.
A zero-valued unit has no contribution rows. Such a record does not, by itself, authorize free issuance.
The original price remains unchanged when a later deploy offers another price.
Changing current wallet balances does not change the record.

Record consistency is necessary but not sufficient for spendable credit.
The issuance transition must establish authorization, actual backing, unique acquisition occurrences, and publication within the common checkpoint.
A caller can construct consistent bytes without funding anything. Decoding those bytes must not produce an authenticated spending capability.
The native reader must obtain them from protected state and bind them to the actual captured physical resource.
Consumption and transfer must preserve original acquisition provenance without appending an unbounded transfer history.

### Binding prepaid cells to measured demand

The native binding check connects selected physical prefixes to [located measurements](raw-byte-observations.md).
Each selected cell identifies one demand occurrence by index.
The index refers to the immutable measured sequence, not a sequence after previous consumption.
Multiple cells can fund one occurrence only while its residual quantity remains positive.

[`NativePrepaidInventory::bind_measured_demand`](../../../../casper/src/rust/util/rholang/costacc/prepaid_receipts/inventory/demand.rs) requires the expected state root and checked current price controls.
It checks the measured schedule against the adopted genesis policy.
The inventory already validates each original acquisition schedule against that policy.
The original and current prices can differ. Their resource classes, valuation rules, and other policy fields cannot differ.

The check requires these identities:

| Identity | Required evidence |
| --- | --- |
| Physical occurrence | A unique captured stack and receipt occurrence at the expected root. |
| Stack prefix | Exactly one demand index for each selected cell, in physical prefix order. |
| Current authority | The complete stored `CostSignature` equals the measured purse authority. Equal channel bytes alone do not suffice. |
| Resource shape | Original location, class, and full typed authority tree equal the measured resource shape. |
| Acquisition provenance | The output retains the original cell terms and does not replace them with the current price. |

Let $`q_j`$ be the measured quantity at occurrence $`j`$.
Let $`c_j`$ be the number of assigned prepaid cells, and let $`r_j`$ be its residual fresh quantity.
Successful binding requires:

```math
q_j = c_j + r_j, \qquad c_j \geq 0, \qquad r_j \geq 0.
```

This equation holds for each occurrence, including resources with zero weight or zero authority valuation.
Each selected unit remains in the prepaid partition with its original acquisition terms.
Only the positive residual enters the fresh partition with the selected current terms.
The required partition contains both parts. Quantities remain counted rather than expanding into one entry per byte.

For example, consider eight transferred bytes with one authority leaf and a class weight of seven.
Two compatible prepaid cells cover fourteen weighted units, regardless of their original acquisition prices.
The remaining six bytes require forty-two fresh weighted units.
At a current price of twenty-three, their acquisition costs 966 native units, excluding other work and the flat deploy fee.
Consumption neither charges the two original purchases again nor changes their provenance.

The implementation uses the following sequence:

```text
Check the expected root, adopted policy, and measured schedule.
Validate unique physical prefixes and receipt occurrences.
Require one demand position per selected cell.
Check the current physical authority and original resource shape.
Subtract one unit from the selected occurrence's residual quantity.
Retain the original resource in the prepaid partition.
Append positive residual quantities with current acquisition terms.
Return the immutable partitions, captured root, and original draws.
```

Structural and host-work limits bound entries, authority nodes, key bytes, comparisons, and temporary allocations.
Failure changes no RSpace state or caller input.
The existing execution checker must still validate numeric limits and complete partitions.
The signed funding proof and common settlement checkpoint remain necessary.

The binding retains its checked controls together with the root and original draws.
Its [settlement connection](observed-funding-outcome.md#rooted-measured-settlement) checks wallet-root equality before matching the complete signed execution family.
A caller cannot replace those controls or use equal balances from another root as equivalent evidence.

This check establishes direct consumption, not a split, join, or authority-transfer derivation.
Those operations require their own verified resource transformation and ownership consent.
A mismatch cannot become an implicit conversion or a permission to replace original terms.
The selected prefixes also do not prove that the planner found every available resource or the canonical funding outcome.
Ordinary funded admission must remain closed until the complete producer establishes those obligations.

The `MeasuredPrepaidBinding` section in [`PrepaidResourceDischarge.v`](../../../../formal/rocq/cost_accounted_rho/theories/PrepaidResourceDischarge.v) states the sequence invariants.
They include exact per-position consumption, non-increasing residual quantities, preserved shapes, weighted conservation, and operation composition.
Rust properties compare the implementation against independent count subtraction and charge calculations.
Native tests bind generated prefixes from actual RSpace snapshots and check the existing physical-consumption verifier.

### Atomic stack and receipt migration

`apply_prepaid_stack_pops` applies physical pops and receipt changes in one owned checkpoint.
Each draw identifies a captured stack, one receipt occurrence in its original source bucket, and a positive pop count.
Draw indices refer to the captured complete bucket, not an intermediate bucket after another draw.

The method rejects repeated stack identities and repeated selections of one receipt occurrence.
It checks each source bucket against the complete physical occurrence count.
A live source without receipts fails. A receipt bucket without a live source also fails.
The selected ordered record must contain exactly as many cells as its physical stack.
Missing metadata never becomes credit derived from wallet balances or current prices.

Preparation removes selected original records in descending occurrence order.
It creates each surviving receipt from the unchanged suffix and assigns that receipt to the physical tail's produce hash.
Destination buckets can already contain stacks or participate as source buckets in the same batch.
Multiple tails can enter one destination without combining their provenance records.
Empty buckets become deletions, not stored zero-cell resources.

The method checks draw, bucket, aggregate selected-cell, per-record byte, and receipt-batch limits.
It shares physical source counts across buckets on one channel during preparation.
After physical mutation, it reads each affected channel once to check the expected resulting occurrence counts.
These bounds apply to this operation's supplied metadata, not all storage or reducer memory.

```text
Read complete source and destination buckets from the owned runtime.
Check source counts, selections, ordered records, and resource limits.
Prepare all source removals and unchanged receipt tails.
Construct canonical replacement buckets and check batch bounds.
Capture the common checkpoint.
Apply the physical pops with complete live-capture validation.
Check the resulting physical occurrence counts.
Apply exact receipt replacements with recorded RSpace events.
On an error, restore the common checkpoint.
On success, return the earlier trace and the complete mutation trace.
```

This operation transfers no money and creates no newly backed rights.
It preserves existing provenance through consumption and physical tail movement.
It does not authorize arbitrary ownership changes or receipt selection.
Wallet debits, allowance changes, and application effects still require the complete funded operation's enclosing transaction.

### Physical stack preflight

Before stack mutation, `apply_stack_pops` checks each selected capture against the complete canonical live inventory.
The comparison includes the occurrence identity, source hash, channel, datum index, persistence flag, random state, and ordered cells.
Equal payloads alone do not establish equal captured resources.
For example, changing a captured source hash must not redirect the associated receipt update while consumption still removes the original datum.

The helper reads and decodes each affected channel once for preflight.
It indexes the resulting inventory by datum index and checks every selected capture before the first removal.
A stale or altered final entry rejects the batch without consuming a valid earlier entry.
The helper rejects duplicate captured occurrence identities separately.
Complete live-record matching prevents distinct accepted occurrence identities from selecting the same physical position.

After preflight, the helper removes datums in descending index order within each channel.
It releases each remaining ordered tail on the tail head's signature channel.
The tail retains the captured random state and persistence flag.
This operation does not change the stack format or the identity rule for valid captures.
Its owned-checkpoint and replay rules remain unchanged.
Receipt migration and wallet settlement still require the enclosing common transaction.

## State-root snapshots

`RuntimeManager::capture_prepaid_receipts` captures requested protected receipt channels from one explicit state root.
It uses the existing immutable history reader rather than the current runtime or a later wallet balance.
The operation does not spawn a reducer, execute a contract, create a checkpoint, or append replay events.
The resulting `PrepaidReceiptSnapshot` retains the root and owned receipt bytes, not a runtime or history reader.

The reader uses the same canonical datum decoder as `RuntimeOps::read_prepaid_receipt`.
Before reading, it requires the selected root to exist in the root repository, even for an empty request.
Constructing a history reader alone does not establish this condition. An unknown root can otherwise appear to contain no receipts.
Duplicate stored values, malformed datums, persistence, and excess value bytes fail the whole capture.
Unknown roots and storage read errors remain runtime errors. They do not become absent receipts or evidence of validator misconduct.
The underlying history repository must retain the requested root and its referenced data for the read.
This API does not establish a new garbage-collection or finality rule.

Requested receipt keys must be distinct.
The implementation sorts them before capture, so request permutations yield the same snapshot.
Entry, individual value, aggregate retained-byte, and host-work limits apply.
Aggregate byte accounting includes key and entry storage plus captured value bytes.
These limits do not bound all temporary storage-decoder memory or total node memory.

Each `receipt` lookup requires the expected state root.
A different root fails before key lookup, including when the key was not requested.
Successful lookup preserves three distinct results:

| Result | Meaning |
| --- | --- |
| `Some(Some(bytes))` | The requested receipt existed at the captured root. |
| `Some(None)` | The requested receipt was absent at the captured root. |
| `None` | The caller did not request this receipt during capture. |

An unrequested key must not become evidence of absence.
A physical stack without its required receipt must not become prepaid credit.
The native funding producer must bind the snapshot root to the root of its physical inventory and wallet evidence.
It must still check cell contents, physical occurrence counts, eligible selection, and authorized backing before constructing an available-resource partition.

Independent callers can capture different roots concurrently.
Publication of a later checkpoint does not change an already captured snapshot or select another root for a historical read.
The snapshot does not reserve funds, authorize issuance, or retain a historical root indefinitely.

## Physical capture before funding

`RuntimeManager::capture_prepaid_stacks` checks selected stack captures against immutable history at one registered pre-state root.
The method reads each selected channel once and compares every selected stack with its complete physical record.
The comparison includes source hash, occurrence identity, channel, datum index, persistence, random state, and ordered cells.
An altered final capture rejects the operation without publishing a partial result.

The reader counts every live occurrence of each selected source, including occurrences that the caller did not select.
It then captures the source buckets at the same root.
Every bucket must match that complete source count.
Every occurrence must contain the complete physical cell count, and every cell must pass native record validation against the adopted policy.
One selected occurrence cannot conceal missing or malformed records for another occurrence of the same source.

For example, selecting one of three identical stacks still requires three valid occurrence records.
The result retains all three records without assigning one arbitrarily to the selected stack.
The funding proof must select an eligible occurrence and prevent its reuse within the transaction.
Snapshot ordinal order does not establish economic ownership.

The result borrows the adopted policy and immutable physical captures. It owns the receipt snapshot.
Its policy accessor retains the exact context used for cell validation.
It retains no runtime or history reader.
Stack identity orders the selected captures, so input permutations produce the same selected sequence.
Each record lookup requires the captured root. An unselected stack returns no record view.

Original resource authority, location, and acquisition price remain unchanged in the receipt.
Current physical authority can differ after an authorized transfer.
This structural capture does not infer transfer permission from that difference or authorize credit merely because a record decodes.
The funding proof still must establish current authority, eligible resource use, and authorized issuance.

Stack, cell, physical-byte, receipt-byte, and host-work limits bound capture processing.
Physical inventory limits include unselected occurrences on each requested channel.
Receipt validation includes all occurrence records in each selected source bucket.
The work budget reserves conservative decode storage based on wire size and native record element sizes.
Underlying storage decoding can allocate before these checks. These limits do not establish a total node memory bound.

The immutable-root retention requirement also applies to this capture.
Separate reads of physical data and receipt data use the same fixed root, not the latest checkpoint.
The method adds no mutation, lock, finality rule, or ownership-transfer restriction.

### Typed inventory and prefix selection

`CapturedPrepaidStacks::resource_inventory` restores typed resources from the captured protected records.
It decodes each source bucket once, including every receipt occurrence for that source.
Different captured stacks with the same source share the decoded records without sharing a consumption allowance.
This inventory covers the captured sources. It does not claim to discover every eligible purse in the state.

Each resource retains its original receipt, acquisition terms, location, class, complete authority tree, and backing contributions.
`RestoredPhloResource` reconstructs the authority without changing ground atoms into quotes, changing tree association, or removing repeated leaves.
The reconstruction uses an explicit stack and checks the complete shape before allocating the authority tree.
It rejects incomplete trees, multiple trees, exceeded representation limits, and exhausted host-work budgets.

The inventory uses this procedure:

```text
Visit each distinct captured source.
Read every receipt occurrence from that source's captured bucket.
Check aggregate record bytes, cell counts, key bytes, and authority nodes.
Decode each original cell under the adopted policy.
Restore its complete typed resource without substituting current prices or owners.
Retain the source and receipt positions for later selection.
```

`prefixes` checks a proposed physical draw before returning borrowed resource views.
Each draw identifies a captured stack, a receipt occurrence, and a positive prefix length.
The method rejects missing stacks, repeated stack identities, repeated source/receipt pairs, out-of-range occurrences, and excessive prefix lengths.
It preserves the proposed draw order and the original cell order within each prefix.
The settlement check compares those original resource keys and quantities with the checked execution's used-resource partition.

Each prefix exposes current physical cell authorities separately from original resource authority.
For example, a captured stack can name a current owner while its acquisition record retains a different original sponsor or authority.
The inventory preserves that difference. It neither rejects a transfer merely because the authorities differ nor treats the difference as proof of authorization.
The funding proof must still establish authorized issuance, current spending authority, eligible receipt selection, and any transfer.

Inventory limits cover all decoded occurrences for each captured source, including occurrences that the proposed draw does not consume.
The byte limit covers aggregate encoded records, and the execution limits separately bound complete encoded keys and authority nodes.
Host-work accounting covers decoding, reconstruction, and selection. Rejection publishes no partial inventory and changes no state.
This immutable view adds no synchronization and makes no Casper selection or finality decision.

The authority reconstruction proofs establish exact round trips and exact original encoding for every accepted tree.
Property tests compare reconstruction with an independent recursive parser, including malformed node sequences and byte-distinct authority forms.
Rooted inventory tests compare original prices and authorities with independent fixture values while current physical authorities differ.
They also test complete occurrence coverage, repeated-source sharing, aggregate limits, host-work rejection, and generated prefix selections.
The composed settlement regression exercises this same inventory path before wallet and receipt publication, rollback, and independent replay.
These checks do not establish a complete native funding producer or authorize ordinary funded admission by themselves.

## Bind retained acquisition to physical births

A successful funding capture distinguishes consumed resources from resources acquired for later execution.
`CanonicalPhloFundingCapture::bind_retained_births` connects each declared new stack cell to one retained-resource obligation column.
The input contains a birth witness and one canonical column position for each cell in that birth.
The checked result retains the immutable funding capture and the complete cell mapping.

The binding enforces these conditions:

- Each stack identity and produce hash appears once.
- Each birth has cells, and each cell has exactly one column position.
- Every column exists and has the retained-resource purpose.
- Each cell's complete authority equals its funded resource authority.
- The number of cells assigned to each retained column equals that column's checked quantity.
- No cell uses a consumed-resource or fee column as backing.

Zero monetary value does not remove these conditions.
A zero-price purchase still needs the exact physical quantity and authority.
The mapping does not divide a monetary amount by price to recover a cell count.

`RuntimeOps::capture_retained_births` adds native state checks to this mapping.
It accepts a checked wallet settlement and the adopted resource policy.
New acquisition terms must match the selected schedule commitment exactly, including the current acquisition price.
Compatible historical terms remain valid for existing prepaid resources, but cannot substitute for new acquisition terms.

The native capture groups births by purse channel and reads each channel once.
It requires exactly one live occurrence for each claimed produce hash.
The live occurrence must have the claimed stack identity and the complete claimed cell sequence.
Unrelated live stacks remain unchanged, but their cells and decoded bytes count toward inventory limits.
The result keeps the physical stack records in canonical stack-identity order.

```text
Bind every claimed cell to its checked retained obligation.
Check the adopted policy and each new acquisition schedule.
Group the claims by purse channel.
For each channel:
    Read and validate the complete purse inventory.
    Match each claimed source to exactly one live stack.
    Reject missing, duplicate, or changed occurrences.
Return the checked funding mapping and physical stack records.
```

Let $`b`$ denote the birth count, $`c`$ the claimed cell count, and $`i`$ the inspected inventory size.
The binding uses $`O(b \log b + c)`$ structural work, plus authority traversal and comparison.
Grouped inventory matching uses binary searches instead of one complete channel read per birth.
Those searches use $`O(i \log(b + 1))`$ work, in addition to inventory decoding and occurrence sorting.
Configured birth, cell, obligation, authority-byte, physical-byte, and host-work limits apply.
These limits do not bound allocations inside the underlying RSpace read before the capture receives its result.

This capture performs no resource or wallet mutation.
The public method requires exclusive mutable access to its runtime handle during capture.
It adds no global lock and does not serialize independent runtimes or validators.
It validates supplied birth witnesses against live state, but does not independently establish that evaluation created those witnesses.
The native execution producer must supply the complete committed birth set from the same evaluation.
The capture must remain inside that evaluation's owned checkpoint until publication or rollback.
Receipt preparation must also bind the original deploy identity and pre-state root, prove source-bucket absence, and preserve contribution totals.
Wallet debits, physical resources, receipts, and application effects must then publish in one transition.
The checked birth capture alone does not establish that complete issuance transition.

Implementation and regression sources:

- [Funding-to-birth binding](../../../../rholang/src/rust/interpreter/accounting/phlo_execution/retained_births.rs)
- [Generated binding tests](../../../../rholang/src/rust/interpreter/accounting/phlo_execution/consent/tests/wire_controls_family/capture_quantities/births.rs)
- [Native birth capture](../../../../casper/src/rust/util/rholang/costacc/prepaid_receipts/births.rs)
- [Native capture tests](../../../../casper/src/rust/util/rholang/costacc/prepaid_receipts/births/tests.rs)

### Distribute checked backing to individual cells

Receipt preparation preserves the selected funding assignment. It does not choose a second allocation policy or change any wallet debit.
For each retained obligation, the preparation orders its physical cells by stack identity and then by cell position.
It processes the checked contribution amounts in canonical custody order.
Every cell retains the same complete resource key as its obligation.

Let $`v`$ denote the acquisition value of one cell and $`q`$ its obligation quantity.
Let $`d_i`$ denote the checked contribution from source $`i`$, and let $`s_i=\sum_{h<i}d_h`$.
Source $`i`$ supplies the interval $`[s_i,s_i+d_i)`$ of the already allocated amount.
Cell $`k`$ receives the interval $`[kv,(k+1)v)`$.
Their intersection determines the cell contribution $`x_{ik}`$:

```math
x_{ik}=\min(d_i,\max(0,(k+1)v-s_i))
       -\min(d_i,\max(0,kv-s_i)).
```

The checked column requires $`\sum_i d_i=qv`$.
Partitioning that interval preserves both source and cell totals:

```math
\sum_{k=0}^{q-1}x_{ik}=d_i,
\qquad \sum_i x_{ik}=v,
\qquad 0\le x_{ik}\le\min(d_i,v).
```

This is provenance assignment, not another withdrawal, exchange, ownership grant, or fairness decision.
Canonical order removes dependence on iteration order. It does not give an earlier source additional spending authority.
The selected funding proof still controls eligibility, consent, exposure, and minimax allocation.

For example, three cells worth five units have checked source contributions of four, seven, and four units.
Their records contain contributions `(4,1,0)`, `(0,5,0)`, and `(0,1,4)` in source order.
Zero entries are omitted from stored records. The original source totals remain four, seven, and four.
A zero-valued obligation retains all $`q`$ cells with empty contribution lists.
Preparation never divides by price or drops a cell because its monetary value is zero.

```text
Check the exact quantity and total contribution amount.
Allocate the bounded cell sequence.
For each source in canonical custody order:
    While that source has a positive remainder:
        Assign the smaller of its remainder and the current cell's unfilled value.
        Record the positive contribution and reduce both remaining amounts.
        Advance when the current cell is full.
Check that every cell is complete and every source amount is consumed.
```

Each positive step exhausts a source or fills a cell.
For $`n`$ sources, the traversal uses $`O(n+q)`$ work and at most $`n+q-1`$ positive records when $`v>0`$.
Cell, contribution, and host-work limits apply before the corresponding allocations.
Checked unsigned arithmetic rejects overflow. Native encoding also enforces the per-wallet signed amount range.
The interval model proves conservation for arbitrary finite partitions, not only two sources or two cells.
It also proves the streaming step equals the interval intersection and that zero value preserves quantity.

### Prepare native records and insertions

`CapturedNativeRetainedBirths::prepare_cell_records` constructs ordered native records from the checked birth mapping and its selected wallet settlement.
It derives the deploy identity from the retained authenticated envelope, not a caller-supplied signature or transaction label.
It retains the wallet snapshot's original pre-state root and the adopted policy's genesis root.
Each record also binds its original produce hash and cell position.

The encoder extracts the complete retained resource key from the canonical captured obligation.
It preserves location, class, authority, and original acquisition terms.
Only positive assigned source amounts enter contribution records. The flat deploy fee and consumed-resource columns cannot enter this path.
The returned object retains the checked birth capture rather than returning an independent spending capability.

Cell and contribution limits apply across the complete preparation, not separately for each wallet.
Per-cell, per-stack, aggregate-byte, and host-work limits also apply.
The encoder does not expand a dense wallet-by-cell matrix.
Existing signed-envelope validation bounds still apply to the envelope commitment calculation.
These limits do not establish a total process memory bound.

`NativeRetainedReceiptRecords::prepare_insertions` requires a receipt snapshot from the same original pre-state root.
Each new source bucket must have an explicit captured absence.
An unrequested key is not absence. An existing bucket cannot be overwritten as a new acquisition.
Each prepared bucket contains the complete ordered record for its one checked physical occurrence.
The result sorts insertions by storage key and rejects duplicate keys.

Preparation does not write RSpace, debit wallets, or publish a checkpoint.
Each returned change expects a live absent bucket when the common transaction applies it.
A bucket created after snapshot capture therefore causes the exact replacement check to fail.
The enclosing producer must still establish causal births and combine these insertions with wallet debits, physical resources, and application effects.
Prepared records alone do not establish that end-to-end issuance transition.

Implementation sources:

- [Checked per-cell backing](../../../../rholang/src/rust/interpreter/accounting/phlo_execution/cell_backing.rs)
- [Native record and insertion preparation](../../../../casper/src/rust/util/rholang/costacc/prepaid_receipts/retained_records.rs)

### Wallet settlement and retained receipt insertion

`apply_retained_wallet_settlement` applies the checked wallet request and prepared retained receipts within one soft checkpoint.
The prepared receipts retain their original checked settlement. The method derives the wallet request from that settlement, not an independent amount list.
The runtime's backing root must match the captured wallet pre-state before mutation.
Application effects can exist in the private runtime above that root. The caller must not persist an intermediate application checkpoint.

The method compares the complete current physical birth inventory with the captured inventory before and after wallet settlement.
An equal quantity alone does not establish equal physical backing.
Receipt replacement also checks live bucket absence, even when the earlier immutable snapshot recorded absence.
The result contains wallet merge metadata and the complete ordered event trace for this operation, including earlier undrained events.

```text
Check the runtime backing root against the captured wallet root.
Prepare the wallet request from the original checked settlement.
Check the complete live birth inventory.
Capture the settlement checkpoint.
Execute the wallet request, when the request exists.
Reject a wallet contract error.
Check that the complete birth inventory remains unchanged.
Apply the prepared receipt insertions.
On an error, restore the settlement checkpoint.
On success, return the complete trace and wallet merge metadata.
```

This method does not persist the resulting root or activate funded deploy admission.
The enclosing execution still must establish causal births and include application effects, old resource consumption, and settlement within its rollback boundary.
The settlement checkpoint starts after application execution. It therefore cannot, by itself, restore the earlier application pre-state.
Cancellation requires the runtime disposal or reset rules below. An exclusive borrow alone does not provide cancellation rollback.

Source: [Retained wallet settlement](../../../../casper/src/rust/util/rholang/costacc/prepaid_receipts/settlement.rs).

### Settlement with prepaid consumption

`apply_prepaid_retained_wallet_settlement` adds existing prepaid stack consumption to the same settlement checkpoint.
Its inputs include an immutable physical capture, selected receipt occurrences, and the prepared retained receipts.
The physical capture and wallet capture must identify the same pre-state and genesis.
Each selected source bucket must still equal its captured value before mutation.

The operation derives the required consumption from the checked settlement's execution, not from an independent caller-supplied quantity.
It decodes each selected ordered prefix and compares complete resource keys and multiplicities with that requirement.
The key includes location, resource class, original acquisition terms, and original resource authority.
Equal monetary totals cannot replace this comparison. Zero-valued resources still require physical consumption.

Each physical stack and receipt occurrence can appear only once in the selection.
Zero draws, excessive draws, missing keys, extra keys, and incomplete consumption fail before wallet settlement.
Rejected admission and failures other than classified user failures retain no prepaid consumption.
Success and classified user failures retain the exact checked used-resource multiset.

The mutation sequence is wallet settlement, retained receipt insertion, then existing stack and receipt migration.
New stack insertions can change a captured stack's storage index without changing its stable identity.
Before migration, the operation resolves each selected identity in the complete current inventory.
Only the storage index can change. Source, channel, random state, persistence, cell sequence, and instance identity must remain equal.
The existing strict physical preflight then checks the refreshed capture before removal.
This does not relax the raw stack-pop interface or treat a storage index as persistent ownership.
Any returned error restores the common settlement checkpoint, including wallet balances and allocation cursors.
The wallet-and-insertion-only method rejects executions that require retained prepaid consumption.
Neither method publishes a state root.

This binding does not itself prove current ownership, transfer permission, or causal birth discovery.
Original receipt authority and current physical authority can differ after an authorized transfer.
The enclosing funding proof must establish that transfer and current spending authority before it selects the checked execution.
The enclosing operation must also own application rollback and canceled runtime disposal.

Source: [Composed prepaid settlement](../../../../casper/src/rust/util/rholang/costacc/prepaid_receipts/consumption.rs).

## Exact replacement

Each requested change contains a receipt storage key, an expected optional value, and a replacement optional value.
`None` means absence, not an empty receipt.
An insertion expects absence. A deletion replaces the current value with absence.
A replacement must change the value.

The batch rejects duplicate receipt storage keys.
It checks entry, value-byte, and aggregate-byte limits before allocating the ordered reference vector.
The aggregate limit includes supplied value bytes and vector entries.
These limits bound this helper's inputs, not total node memory or all RSpace allocation.

```text
Validate batch sizes, distinct keys, and changed values.
Sort changes by receipt storage key.
Compare every current receipt with its complete expected value.
Capture the owned runtime checkpoint.
For each change:
    Record removal of the old value, if present.
    Produce the replacement, if present.
On mutation failure, restore the checkpoint.
On success, return the earlier trace followed by the receipt trace.
```

Exact comparison prevents stale overwrites.
The implementation does not infer compatibility from equal prices, equal quantities, or matching hashes alone.
Repeated application fails after a changed receipt no longer matches its expected value.
This rule is not a permanent operation journal. A later authorized change can restore an earlier byte value.
Economic cursors and backing provenance must prevent unauthorized reuse across such histories.

## Checkpoint and replay contract

The helper requires exclusive ownership of the runtime's mutation phase.
The caller must complete all reducer operations before checkpoint capture and rollback.
An exclusive Rust reference does not authorize concurrent access through a previously cloned RSpace handle.
Independent validator runtimes can execute concurrently.
This contract adds no validator-wide lock or consensus ordering rule.
The caller must await mutation completion or discard a canceled candidate runtime before reuse.
An explicit reset to the captured root can also restore a canceled candidate before reuse.
The error-return rollback contract is not an asynchronous cancellation guard.

RSpace soft checkpoints capture the current event log and produce counters, then start a new trace segment.
The helper returns the captured earlier trace followed by its receipt trace on success.
It drains that trace from the runtime. The caller must retain the returned trace in the enclosing execution evidence.
An empty batch returns an empty trace without changing runtime state or draining earlier events.
Receipt removal uses recorded RSpace events.
The removal identity binds the domain, receipt storage key, expected value, and replacement value, including presence and lengths.
Replacement produces also enter the normal RSpace trace.
Replay checks recorded removals against that trace and reproduces replacement data.
Replay RSpace does not emit a second copy of each ordinary produce event.
Verification therefore compares removal evidence, consumed replay bindings, and the complete resulting root, not identical emitted logs.

A failed replay candidate must reset to its captured root and load its complete trace before retry.
Soft rollback restores tuple state but does not restore consumed replay bindings.
The enclosing funding operation must include receipts, stacks, wallets, and allowance effects in its common rollback boundary.
This helper's local rollback alone does not establish that larger transaction's atomicity.

The [funded candidate model](../../../../formal/tlaplus/cost_accounted_rho/FundedCandidateCheckpoint.tla) makes this enclosing boundary explicit.
Each worker has private application, wallet, stack, and receipt stages. Workers can advance independently without a global mutation lock.
Failure restores private tuple state. Cancellation discards the candidate. Restart restores the original state and reloads replay evidence.
Publication is valid only after all stages finish.

| Invariant | Required behavior |
| --- | --- |
| `AtomicPublication` and `CompleteSettlement` | Expose all four effects together, never a prefix. |
| `UnpublishedCandidatePrivate` | Keep unfinished and rejected candidates invisible. |
| `FailedCandidateRestored` | Restore the complete private snapshot after a returned failure. |
| `ReadyCandidateClean` | Do not reuse a partially changed canceled runtime. |
| `FreshReplayAttempt` | Reload consumed replay evidence before another attempt. |
| `WorkingPrefix` | Preserve the required local operation order. |
| `IndependentCandidatesEnabled` | Allow each independent worker to advance regardless of another worker's stage. |
| `CompletedCandidatesAgree` | Give completed workers the same abstract effect set for the same input. |

The finite configuration checks all reachable interleavings of three workers, including failure, cancellation, and restart cycles.
Four negative controls separately omit publication, rollback, cancellation, or replay-reset safeguards.
The model tracks effect presence, not wallet arithmetic or actual RSpace execution.
It complements the allocation and stack-migration models. It does not prove block merging, finality, fairness, or eventual completion.
Native refinement tests must check actual state roots and recorded replay events at these boundaries.

Storage and replay-operation failures remain runtime errors.
Malformed receipts, stale expected values, and invalid batch inputs are settlement errors.
The caller must not interpret a local storage failure as evidence of validator misconduct.

## Formal scope and regression coverage

The cell-allocation model proves interval symmetry, bounded contributions, partition composition, and conservation of every source and cell total.
The model covers arbitrary finite partitions. It does not fix the wallet count.
The streaming-step theorem connects the implementation's minimum-remainder operation to that interval model.
Generated tests compare the Rust result with an independent interval-intersection oracle.
Additional tests cover zero values, unsigned boundary amounts, insufficient limits, and input-order independence through checked funding captures.

Native record tests decode every produced cell and compare original identities, terms, values, and aggregate source contributions.
They interleave different resource locations within each stack and preserve each location's source totals separately.
They test absent, unrequested, present, and wrong-root receipt snapshots against real RSpace history.
Repeated insertion must fail without changing the published test root.
These component tests do not substitute for the complete signed-deploy execution and settlement tests.

The retained-birth model proves exact column quantities, distinct birth sources, physical-validation requirements, and exclusion of consumed-resource and fee columns.
It rejects missing cells, excess cells, and out-of-range columns.
It also preserves cell multiplicities when birth order changes.
Its physical-validation predicate is an explicit premise, not a proof of the complete interpreter or RSpace implementation.

Generated binding tests vary wallet counts, prices, resource quantities, and input order.
Native RSpace tests compare complete physical records and state roots before and after capture.
They include compound authorities and simultaneous captures on independent runtimes.
Negative tests cover stale identities, missing stacks, duplicate live sources, incorrect authorities, mismatched quantities, and resource limits.
These tests do not establish complete native receipt issuance or common-checkpoint settlement.

[`PrepaidReceiptStorage.v`](../../../../formal/rocq/cost_accounted_rho/theories/PrepaidReceiptStorage.v) models arbitrary key and value types.
It proves exact comparison, stale rejection, replacement visibility, unrelated-key preservation, and disjoint-key commutation.
It also proves batch composition and all-or-nothing publication after a failed suffix.
For distinct keys, complete preflight acceptance is equivalent to sequential comparison acceptance.
Its transitions describe an owned store. They do not prove safety for simultaneous mutation of one runtime through aliased handles.

The occurrence model proves exact selection, one-occurrence removal, residual multiplicity, and provenance conservation across arbitrary finite removal histories.
It also demonstrates why snapshot positions cannot identify persistent receipt records.
These theorems do not prove that opaque record contents represent authorized acquisition or that tail migration is complete.

The stack-capture model requires exact equality with the live record at the selected channel and index.
It proves complete-record binding, source and persistence preservation, altered-capture rejection, and non-aliasing of distinct accepted identities.
Its batch theorem requires every selected capture to pass preflight.
The model assumes correct inventory extraction and the existing hash identity contract.
It does not prove hash collision resistance or safety under concurrent mutation through aliased runtime handles.

The ordered-cell theorems prove exact prefix removal, suffix order, surviving-record preservation, physical tail length, and composition across successive consumptions.
Invalid pop counts produce no successful transition.
These results quantify over arbitrary cell record types and finite lists.

The native backing model proves exact original-value conservation, positive contributions, distinct custody keys, and rejection of inconsistent funding or invalid native amounts.
Its valuation depends on original authority demand, class weight, and acquisition price, not the number of funding wallets.
Its partition theorem preserves acquisition value across consumed and surviving records.
The model separates the unsigned aggregate limit from the signed per-wallet limit.
It does not prove that consistent contribution claims represent actual authorized wallet debits.

The rooted-snapshot model proves exact lookup at the selected root, explicit requested absence, and rejection of a different root.
Its registered-capture rule requires root evidence before it constructs a snapshot.
An explicit counterexample shows how an unchecked total history function can fabricate an absence result for an unknown root.
It also distinguishes unrequested keys and preserves captures when history extends without changing the selected root's values.
The immutable history assumption is explicit. The model does not prove storage durability or protection against concurrent data deletion.

[`PrepaidStackMigration.tla`](../../../../formal/tlaplus/cost_accounted_rho/PrepaidStackMigration.tla) models three independent validators with interleaved physical pops, receipt updates, publication, failure, and retry.
Its instance contains duplicate-source stacks, four provenance origins, three source shapes, and nine initial cells.
Candidate state can contain intermediate changes, but published state must remain consistent.
The invariants check physical binding, original-cell conservation, complete rollback, candidate isolation, and agreement between successful validators.
Three negative controls omit a tail record, publish early, or restore only receipts after failure.

The model treats each validator's candidate runtime as exclusively owned.
Publication denotes the local operation result, not a new Casper voting or finalization rule.
It does not model acquisition authorization, wallet settlement, arbitrary overlapping candidates on one runtime, or asynchronous cancellation.
Its finite-state checks complement the arbitrary-list proofs and native regressions. They do not establish complete funded-execution correctness.

The rooted physical-binding lemmas require one root, an exact source identity, complete occurrence counts, and complete valid cell sequences.
They prove rejection of mixed roots, incomplete occurrences, and truncated records.
Acceptance requires every cell to satisfy the supplied cell predicate.
An arbitrary finite occurrence theorem imposes no two-occurrence limit.
The model assumes correct physical extraction and abstracts cell validity through that predicate.
Native generated tests connect those conditions to actual checkpoint reads and the native record decoder.
These lemmas do not prove authorized issuance or ownership transfer.

Native tests exercise real RSpace insertion, replacement, deletion, checkpoint reset, malformed storage, bounds, and generated update histories.
Generated batches compare canonical traces and roots under reversed input order, including stale final entries and complete rejection.
Replay tests compare complete resulting roots and recorded removals, then inject an incomplete proposer trace.
The failure test first verifies that its trace prefix permits an actual removal.
Independent-runtime tests apply disjoint changes in opposite orders.
These checks do not establish the economic validity of arbitrary receipt bytes.

A native regression creates identical stack datums, consumes one, and verifies the surviving acquisition record despite snapshot-identifier renumbering.
Its recorded replay consumes the same stack and metadata events, then checks the complete resulting state root.
Bucket properties compare arbitrary removal histories with an independent list model, including equal records and rejected indices.
Codec tests cover complete round trips, permutations, every truncated prefix, malformed fields, and independent bounds.
Stack regressions reject altered metadata and duplicate physical selections before any state change.
Generated tests compare complete-record acceptance across metadata mutations, duplicate physical data, mixed batches, and input permutations.

Native cell tests compare acceptance with an independent wide-integer contribution oracle and generate valid multi-wallet cohorts.
They cover zero value, unsigned aggregate limits, signed per-wallet limits, policy mismatches, malformed fields, and every truncated prefix.
Generated consumption histories compare consumed and surviving acquisition values with the original total.
A real stack-migration test preserves original typed records with 65 and 1,000 contributors and verifies the complete replay root.
These tests establish record consistency and provenance preservation, not that the fixture contributions came from actual wallet debits.

Snapshot tests compare generated checkpoint histories with independent maps and reverse the requested key order.
They cover absent and unrequested keys, wrong roots, malformed final entries, duplicate keys, and byte and host-work limits.
Concurrent tests run four readers against an older root while another runtime publishes eight later checkpoints.
Historical reads must retain the original value throughout that test.
Physical capture tests retain original prices and 65-wallet contribution records while checking complete duplicate-source inventories.
They reject missing receipts, altered captures, wrong sources, stale roots, incomplete occurrence counts, malformed final cells, and resource-limit violations.
Generated tests compare acceptance with the complete-bucket conditions across variable occurrence, cell, and wallet counts.
These read-only checks require the complete state root to remain unchanged after acceptance and rejection.
A native test consumes duplicate-source stacks, releases different ordered tails, and checks recorded replay against the complete resulting state root.
Atomic migration tests cover all six orders of three draws with shared source and destination buckets.
They preserve earlier application events and compare complete play and replay roots.
Other tests cover new destination creation, complete bucket deletion, malformed or missing provenance, duplicate selection, and bounds.
The late-failure test first proves that a supplied replay trace permits the actual physical pops.
The same trace then omits receipt mutations. The combined operation must fail and restore the complete original root.

The wallet settlement regression uses authenticated signed funding and a real SystemVault balance.
It checks fee-only settlement, an unchanged platform-failure balance, stale cursor rejection, and complete replay roots.
Its retained-resource case checks actual wallet debits against the new cell's original custody contribution.
A receipt byte-limit failure occurs after wallet application. The resulting root must match the full pre-settlement root, including both cursors.
The composed case consumes one cell from an existing two-cell stack and funds a separate retained cell on the same channel.
The remaining old cell must retain its exact original receipt bytes under the migrated tail source.
A late migration failure must restore the wallet, both cursors, existing stack, and receipt state.
An enclosing checkpoint verifies rollback of the application stack, wallet debit, old consumption, and receipt insertion together.
The fixture supplies the physical birth directly. It does not prove automatic birth discovery from a user's reducer trace.

Generated checkpoint histories exercise the underlying RSpace boundary without repeating genesis construction.
Each history varies mutation-prefix length, cancellation, returned failure, successful completion, and receipt values across repeated attempts.
Cancellation drops an owned candidate task. Historical observers must retain the previously published values.
Returned failures must restore the entire candidate root. Successful attempts must reproduce the same root through recorded replay.
Concurrent histories use independent runtimes. These tests model four protected records, not four real wallet operations.
The SystemVault regression supplies the separate native economic check described above.

The [composed settlement model](../../../../formal/tlaplus/cost_accounted_rho/FundedPrepaidSettlement.tla) checks three independent candidates with success, user-failure, and platform-failure outcomes.
Its configurations cover prices zero and two. Each configuration checks all reachable interleavings within its finite resource bounds.
Each candidate starts with a wallet balance of ten and two prepaid cells.
An accepted billable outcome consumes one prepaid cell, funds one fresh unit, and pays one fee unit.
A successful outcome also funds one retained cell. A user failure retains no new cell, and a platform failure retains no economic change.
The invariants require exact committed debits, physical-receipt agreement, complete rollback, private provisional effects, and agreement for equal outcomes.
Negative controls skip prepaid consumption, retain a partial rollback, or charge a platform failure.
The model abstracts native cryptography, resource-key encoding, application execution, and transfer authorization. It does not prove those boundaries.

Generated native selection tests vary stack prefixes, selection order, duplicate selections, quantities, locations, classes, and original prices.
The tests retain 65-wallet contribution records and check that read-only selection leaves the complete state root unchanged.
Failure-projection properties vary repeated and mixed failure classes independently of resource value.
Index-rebinding properties vary storage indices and independently alter every other capture field.
Only an index-only change can succeed. A failed check must not modify the captured record.
The corresponding Rocq lemmas prove that rebinding changes only the index and preserves the strict live preflight.
They also prove acceptance of an unchanged record at a new index and rejection of a changed source.

Sources:

- [Native storage](../../../../casper/src/rust/util/rholang/costacc/prepaid_receipts.rs)
- [Native regressions](../../../../casper/src/rust/util/rholang/costacc/prepaid_receipts/tests.rs)
- [Checkpoint history properties](../../../../casper/src/rust/util/rholang/costacc/prepaid_receipts/checkpoint_tests.rs)
- [Wallet and receipt settlement regressions](../../../../casper/tests/util/rholang/wallet_settlement_checkpoint.rs)
- [Source bucket codec](../../../../casper/src/rust/util/rholang/costacc/prepaid_receipts/bucket.rs)
- [Source bucket properties](../../../../casper/src/rust/util/rholang/costacc/prepaid_receipts/bucket/tests.rs)
- [Physical stack preflight and regressions](../../../../casper/src/rust/util/rholang/supply.rs)
- [Ordered cell codec](../../../../casper/src/rust/util/rholang/costacc/prepaid_receipts/ordered.rs)
- [Atomic migration](../../../../casper/src/rust/util/rholang/costacc/prepaid_receipts/stack_pops.rs)
- [Atomic migration regressions](../../../../casper/src/rust/util/rholang/costacc/prepaid_receipts/stack_pops/tests.rs)
- [Root-bound physical capture](../../../../casper/src/rust/util/rholang/costacc/prepaid_receipts/physical.rs)
- [Physical capture regressions](../../../../casper/src/rust/util/rholang/costacc/genesis_resource_policy/tests/prepaid_cells/physical_capture.rs)
- [Observed funding contract](observed-funding-outcome.md)
