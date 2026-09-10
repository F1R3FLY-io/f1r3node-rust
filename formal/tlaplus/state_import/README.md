# State-import verification models

## Purpose and boundary

These models address imported-state publication and recovery ownership.
They do not change Casper voting, fork choice, or finalization thresholds.
The [permanent investigation record](../../../docs/casper/theory/finalized-floor/state-import-validity.md) links defects, source evidence, and repair obligations.

`StateImportPublication.tla` checks lifecycle composition under a validated-content storage contract.
It does not treat a root marker as complete state.
It derives required references from a graph and checks their availability before publication.
The associated Rocq development proves the graph and exact-binding preservation foundations for arbitrary finite histories.

The finite model uses reference identities after successful decoding and type validation.
The `history` and `cold` sets denote durable, validated references.
These sets do not denote unchecked received bytes or arbitrary rows found under matching keys.
The physical-to-abstract correspondence remains required before production acceptance.

## State and actors

| Name | Meaning |
| --- | --- |
| `Actors` | Independent transfer actors. No global transfer lock exists in the model. |
| `Attempts` | Actor and generation pairs. Request replacement creates a new generation. |
| `RootRef` | The history reference for each root. |
| `Children` | Decoded, typed references selected by each graph reference. Cold references have no children. |
| `Reach` | Graph reachability from the requested root, including the root reference itself. |
| `history`, `cold` | Durable references that satisfy the content-validation contract. |
| `checked`, `frontier` | Separate validation state for each attempt. |
| `tags`, `currentRoot` | Published root markers and the current-root pointer. Their writes are separate. |
| `owner`, `pending` | Current request generation and unresolved recovery obligation for each actor. |
| `readerRoot` | The reader's captured root. Later pointer changes do not modify this value. |

The concrete instance contains three roots, four history references, and one cold reference.
The initial root is empty.
Two imported roots share a history node and its cold leaf.
Each importer can load and check references without waiting for the other importer.

The reader captures whichever published root is current when capture occurs.
The reader then checks one reference per action.
Checkpoint selection can interleave with every importer and reader action.

## Transition correspondence

| Actions | Required implementation meaning |
| --- | --- |
| `WriteHistory`, `WriteCold` | Commit accepted, compatible content. These are different storage operations. |
| `BeginScan`, `CheckReference` | Read durable content and preserve graph coverage through a deduplicated frontier. |
| `AuthorizeTag` | Exhaust the checked frontier before obtaining publication authority. |
| `WriteTag` | Publish the root marker without an atomic current-root update. |
| `WriteCurrentRoot` | Select the complete imported root. A later checkpoint can select another root. |
| `Complete` | Retire only the current request generation after its successful attempt. |
| `FailAttempt`, `RetryAttempt` | Retain the unresolved recovery obligation and permit another attempt. |
| `ReplaceRequest` | Transfer ownership without deleting previously accepted durable content. |
| `ObservePublishedRoot` | Resolve recovery through a separate verified observation of a published complete root. |
| `CheckpointSelect` | Select another complete root without changing captured readers. |
| Reader actions | Capture a root, check its references separately, and complete only after all reads. |

Failure can occur between history, cold, tag, and pointer operations.
Failure after a successful tag does not invalidate that tag's complete state.
The failed attempt cannot report success, but a separate verified observation can resolve recovery.
The model never requires the current-root pointer to equal an attempt's root when that attempt completes.

Request replacement permits earlier accepted content operations to finish.
It prohibits stale completion from retiring a newer request.
This action is a contract for actual request-lifecycle boundaries, not an assertion that Tokio interrupts synchronous Rust statements arbitrarily.

The production runtime requester uses root-keyed entries and an exclusive `Core` borrow during synchronous response processing.
Adding another block owner to the same entry does not replace that entry.
An older response can satisfy a current request for the same immutable root after validation.
Generations identify local operation lifetimes in the model.
They do not prescribe a new network field or a new production generation mechanism.

## Invariants

| Invariant | Requirement |
| --- | --- |
| `TypeOK` | Each state field retains its declared finite domain. |
| `FrontierCut` | Checked references remain available, and every checked edge retains coverage. |
| `PublishedRootsClosed` | Every tagged root contains all required validated references. |
| `CurrentRootClosed` | The current-root pointer selects complete state. |
| `UnresolvedRecoveryOwned` | An incomplete required root retains a recovery obligation. |
| `NoStaleCompletion` | An old request generation never completes on behalf of its replacement. |
| `CapturedReaderClosed` | A captured root remains usable during concurrent operations. |
| `ReaderValuesPreserved` | Already read references remain available. Exact payload preservation comes from the storage refinement. |
| `CompletedAttemptsClosed` | A completed attempt refers to complete state. |

`ReachWithin` expands the graph for its finite reference count.
Every reachable reference has a simple path shorter than that count.
The model therefore includes all references even when the graph contains cycles.
The scan removes already checked references before adding more work.

## Configurations and controls

| Configuration suffix | Actors | Generations | Expected result |
| --- | --- | --- | --- |
| No suffix | Two | One | All nine safety invariants hold. |
| `Replacement` | One | Two | All nine invariants hold across request replacement. |
| `Combined` | Two | Two | All nine invariants hold across concurrent transfers and replacement. |
| `EarlyTag` | Two | One | Premature publication violates `PublishedRootsClosed`. |
| `RetireFailure` | Two | One | Incorrect failure retirement violates `UnresolvedRecoveryOwned`. |
| `StaleCompletion` | One | Two | Stale completion violates `NoStaleCompletion`. |
| `RemoveShared` | Two | One | Removing shared published state violates `PublishedRootsClosed`. |

Expected results are requirements, not evidence that each configuration has completed.
The gate requires native exit `0` for positive cases.
For each negative control, it requires native exit `12` and the named invariant violation.
Parser errors, resource exhaustion, and other failures do not count as successful controls.

Run a configuration with a finite memory limit:

```bash
systemd-run --user --scope --expand-environment=no \
  -p MemoryMax=2G -p MemorySwapMax=0 -p CPUQuota=100% \
  bash scripts/check-state-import-publication.sh StateImportPublicationCombined
```

The script limits the Java heap to 768 MiB and uses one worker.
It records inputs, output, and terminal input hashes under `target/verification/state-import/`.
It does not create temporary files under `/tmp`.

## Ownership refinement

`StateImportOwnership.tla` uses a named `INSTANCE` of the publication model with its safe transition rules.
It adds verification records without duplicating the base transition definitions.
These records are *ghost state*: they specify proof obligations, not additional network fields or consensus state.

| Record | Meaning |
| --- | --- |
| `writePermit` | An accepted reference write belongs to one attempt and its immutable requested root. |
| `tagPermit` | A checked attempt can write its tag, then its current-root pointer, in that order. |
| `retirement` | Recovery ended through a successful current attempt or a separate verified observation. |
| `capturedRoot` | An independent record of the root selected when the reader began. |
| `unauthorizedTag`, `unmatchedWrite` | Monotonic records of missing authority at publication or write completion. |

`ReserveWrite` obtains authority only for the current attempt while its recovery obligation remains pending.
Its reference must belong to the requested graph.
`CommitWrite` consumes that specific permission.
A compatible write can finish after request replacement because its accepted content remains valid for the same immutable root.
If another importer has already stored the reference, completion preserves the base state and consumes only the permission.

Publication requires an empty checked frontier and an active current request when `AuthorizeTag` occurs.
The tag and pointer operations then consume their respective permission stages.
Failure clears the failed operation's permissions but preserves the recovery obligation.
This failure action denotes a definitive operation failure, not detached work that can still commit afterward.

Only successful current completion or a separate verified observation can retire recovery.
A complete graph alone is insufficient before publication.
A failed attempt also cannot report success merely because it wrote a valid tag before failure.
The observation action can independently establish success in that case.

| Additional invariant | Requirement |
| --- | --- |
| `WritePermitsBoundToRoot` | Each outstanding write belongs to a loading attempt and a required reference. |
| `WriteCompletionMatched` | Each committed reference consumes its matching permission. |
| `TagAuthorizationCurrent` | No replaced or resolved attempt acquires new publication authority. |
| `TagWritesAuthorized` | Tag and pointer stages retain their respective permissions. |
| `RetiredRecoveryHasPublishedEvidence` | Every resolved obligation has a published, complete root. |
| `RetirementHasWitness` | The current generation has a valid reason for resolution. |
| `ReaderUsesCapturedRoot` | Pointer changes cannot redirect an active reader. |
| `ReaderCoversCapturedRoot` | Remaining and checked reads partition the captured root's required references. |

Positive configurations also check `RefinesBase`.
Every wrapper transition must be a base transition or preserve all base variables.
Permit reservation is an example of the latter, commonly called a *stuttering step*.
The refinement property prevents the wrapper from silently replacing the base lifecycle with a different protocol.

| Ownership configuration suffix | Actors | Generations | Expected result |
| --- | --- | --- | --- |
| No suffix | Two | One | Base and ownership invariants hold, with base refinement. |
| `Replacement` | One | Two | The same properties hold across replacement. |
| `Combined` | Two | Two | The same properties hold across concurrent transfers and replacement. |
| `LateAuthority` | One | Two | Authorization after replacement or resolution violates `TagAuthorizationCurrent`. |
| `RetireClosedBeforeTag` | One | One | Premature retirement violates `RetiredRecoveryHasPublishedEvidence`. |
| `RetireFailedAfterTag` | One | One | Failure without verified resolution violates `RetirementHasWitness`. |
| `FollowCurrentRoot` | One | One | Reader redirection violates `ReaderUsesCapturedRoot`. |
| `UnmatchedWrite` | One | One | Unreserved completion violates `WriteCompletionMatched`. |
| `InvalidDomain` | Two | One | A reference equal to the absent-permission sentinel causes an assumption failure before exploration. |

The ownership checker uses the same resource limits and evidence rules as the publication checker:

```bash
systemd-run --user --scope --expand-environment=no \
  -p MemoryMax=2G -p MemorySwapMax=0 -p CPUQuota=100% \
  bash scripts/check-state-import-ownership.sh StateImportOwnershipReplacement
```

The ownership `Combined` configuration includes concurrent actors and multiple generations together.
Passing smaller configurations does not establish this combined state space or arbitrary actor counts.
The base `Combined` configuration covers both dimensions only for the base invariants.

The ownership `Combined` check completed at invocation `28774996d0104246b5ddd05a71aaf148` with exit `0` and unchanged inputs.
It checked the listed ownership invariants and `RefinesBase` over 22,456,308 distinct states and 195,081,784 generated states.
The final queue was empty, and the search reached depth 56 after approximately 3 hours, 43 minutes.
The evidence is in `target/verification/state-import/ownership.CSGfro/`.
TLC reported fingerprint-collision estimates of `2.1e-4` calculated optimistically and `6.9e-6` from the actual fingerprints.
This result covers the configured two actors and two generations, not arbitrary actor counts or unbounded execution.

The generic model now requires `None \notin Refs`.
The TLC entry modules declare this assumption explicitly because the invalid-domain control did not enforce the nested declaration alone.
The [permanent record](../../../docs/casper/theory/finalized-floor/state-import-validity.md#ownership-parameter-domain-correction) preserves original and corrected source hashes.
Removing only the added assumption lines reproduces the original module hashes.
The original concrete domains satisfy the restriction, and their transition operators and properties remain unchanged.
The old Combined result remains associated with its original inputs, not the edited files.

Invocation `9f29d62cd154435a993b63ab7f66d2db` checked the edited two-importer and replacement configurations, plus all five failure controls.
Both positive searches completed with their previous state counts.
The invalid-domain control returned native exit `10` before initial-state exploration.
All eight gates returned exit `0` with unchanged inputs under a 2 GiB, zero-swap, one-CPU scope.
The permanent record identifies each evidence directory and expected violation.

Invocation `286bc97e12b141aa90543ff0a37c3e91` also completed the corrected ownership `Combined` configuration.
It explored 195,081,784 generated states and 22,456,308 distinct states to depth 56, with an empty final queue.
The source-bound gate returned `0` in `target/verification/state-import/ownership.tS8Hdp/` after 4 hours and 33 minutes.
TLC reported fingerprint-collision estimates of `2.1e-4` optimistically and `2.1e-5` from actual fingerprints.
The scope used a 2 GiB memory limit, no swap, and one CPU.
This recheck binds the result to the corrected source files without changing its finite-instance limits.

## Composition and remaining obligations

The [closure proof](../../rocq/cost_accounted_rho/theories/StateImportClosure.v) covers contextual reference extraction and frontier coverage.
The [storage proof](../../rocq/cost_accounted_rho/theories/StateImportStorage.v) covers physical bindings, exact values, separate alias lookups, and guarded batch histories.
The [page proof](../../rocq/cost_accounted_rho/theories/StateImportPage.v) covers occurrence preservation when traversal output becomes page entries.
It preserves leaf and history lists across arbitrary finite page sequences, including duplicates and leaf-only tails.
It also proves occurrence-budget partitioning of a supplied traversal trace, including positive-budget progress and exact leaf preservation.
The operational export proof below connects selected entries to decoded-tree traversal under explicit reader and trace premises.
The [codec proof](../../rocq/cost_accounted_rho/theories/StateImportCodec.v) characterizes accepted radix records and preserves their exact bytes.
It proves unique slot lookup, a 256-slot projection, and raw-byte hash preservation through ordered decoded records.
Its reversible fixed-width key mapping connects valid wire records to the closure model while preserving their exact bytes and occurrence context.
The reverse mapping applies to mapped valid records, not arbitrary natural-number keys outside the 32-byte image.
Canonical slot projection does not replace the original byte sequence as the hash witness.
The strongest preservation results require no global alias-agreement assumption and no hash-injectivity assumption.

The [cursor proof](../../rocq/cost_accounted_rho/theories/StateImportCursor.v) characterizes canonical singleton and seven-entry encodings.
It proves round trips, injectivity, exact field widths, absent indexes, and root/carrier preservation.
Canonical acceptance does not establish node existence, carrier agreement with the traversed path, or completion.
A singleton can name a shared tail rather than an original recovery root.

The [traversal proof](../../rocq/cost_accounted_rho/theories/StateImportTraversal.v) resolves a cursor's exact compressed-prefix path through authenticated history rows.
Its path-length bound is sufficient for every valid path and does not impose an additional depth limit.
It proves carrier agreement, readable targets, deterministic resolution, and wire-to-closure history-read correspondence.
That correspondence requires valid 32-byte keys and hash outputs, but no hash injectivity.
An interleaved path relation permits compatible writes between node reads and proves a complete path witness in the final view.
Paths that were already complete retain their selected target, including across compatible writes after the final read.
This path module does not authenticate response cursors against outstanding requests.

The [stack proof](../../rocq/cost_accounted_rho/theories/StateImportStack.v) reconstructs the current frame and its exact ancestor positions.
It preserves later siblings, shared-subtree occurrences, and frame bindings across compatible writes between reads.
The [export proof](../../rocq/cost_accounted_rho/theories/StateImportExport.v) defines leaf visits, history descent, frame removal, skip/take, and root-anchor handling.
It connects successful bounded pages to occurrence splits when a complete trace exists.
The operational page relation does not require complete future state as an acceptance premise.
History-budget bounds hold without that complete-trace premise.

The export proof distinguishes a budget boundary from an empty stack and from a missing required child.
It preserves finite export prefixes across compatible interleaved writes and later writes.
The execution module below supplies budgeted, anchored completion under compatible interleaving.
A decreasing measure covers successful steps with nonnegative counters, including frame growth on descent.
The measure supports the total slice evaluator below, but it is not a production memory bound.

Invocation `a46f91aabf9b4b7a97e7c5a8baaf2ce6` compiled all eight import modules and kernel-checked their proof terms.
The unchanged-input gate returned exit `0` in `target/verification/state-import/closure-gate.fo5lDo/`.
It used a 2 GiB systemd memory limit, no swap, and one CPU.
Symbolic controls include both visits to a shared subtree, empty-trace continuations, skip behavior, and leaf-only budget tails.
The [permanent investigation](../../../docs/casper/theory/finalized-floor/state-import-validity.md) states the precise correspondence limits and required native tests.

The [execution proof](../../rocq/cost_accounted_rho/theories/StateImportExecution.v) gives an explicit slice evaluator with success and read-failure results.
Its derived recursion allowance is sufficient for every input with natural-number counters and a total mathematical reader.
Successful evaluation is equivalent to the operational slice relation, without a complete future-state premise.
Every read error has a reachable failed-step witness in its observation view.
Compatible insertion can make that key available later, so error absence is not asserted in the final view.

The interleaved slice relation permits compatible writes before the first child read, between steps, and before completion.
It preserves the complete page result in its final reader view, including skip/take and the exact residual stack.
The cursor composition joins interleaved stack construction with anchored export and preserves the history budget.
It permits a leading write between invocation entry and the first root lookup.
No page-wide atomic read is assumed.
Read latency, final wire-cursor encoding, cold-data closure, and publication validity remain outside this evaluator's guarantee.

Invocation `ccfd7a95da8842fc9b78fa22012c8821` compiled all nine import modules and kernel-checked their proof terms.
The unchanged-input gate returned exit `0` in `target/verification/state-import/closure-gate.ecGifV/` under the same resource limits.
Executable controls cover genuine read failure, recovery after insertion, zero-budget behavior, and insertion before the first child read.
Invocation `3f5c997bb5b54939ac1b2606e26d8c5a` also checked root arrival before the first lookup.
It returned exit `0` with unchanged inputs in `target/verification/state-import/closure-gate.feLzGA/` under the same resource limits.
External writes do not decrease the local traversal measure, so these results do not bound arbitrary schedule duration.

The publication model supplies lifecycle checks above those contracts.
It does not independently prove that the current importer or checkpoint writer satisfies the contracts.
New rows require hash, type, codec, and request validation before they can represent available references.
Both alias guards must share the actual write transaction boundary.

Atomic storage batches require an explicit correspondence to these single-reference model actions.
Checkpoint selection models pointer changes among complete published roots, not creation of new checkpoint content.
The checked-set filter has a Rocq coverage lemma, but exact queue representation still needs implementation conformance tests.

The publication model does not cover canonical wire pagination, partial-page authentication, shared cursor ownership, or the leaf-only exporter boundary.
The eighteen refinement modules supply parts of that representation layer.
The [wire refinement](../../rocq/cost_accounted_rho/theories/StateImportWire.v) derives cursor metadata separately from traversal references.
It preserves leaf-only terminal pages and the existing exhausted singleton tails.
Its frame-spine invariant proves exact next-stack reconstruction after anchored and interleaved export.
Its key-independent position order proves that a successful nonterminal export cannot return its input cursor unchanged.
This result includes positive skip counts with empty output and does not require a finite complete graph.

The wire reference lists precede production payload deduplication.
The payload bridge proves exact unique-key coverage without requiring stable row order or duplicate rows.
Missing rows and storage failures cannot become silently filtered success.
Compatible writes before and between payload reads preserve the exact successful rows and key coverage in the final reader view.
The captured-root check binds raw bytes to the requested root hash and the existing radix decoder.
An empty response still cannot establish validated root closure.

Invocation `b6000d20cfe0438eb8d9b05581d1494e` checked all ten modules with unchanged inputs and returned exit `0`.
The evidence is in `target/verification/state-import/closure-gate.jIbaKn/` under the same 2 GiB, zero-swap, one-CPU limits.
Invocation `fcf983894e384fefa22158a24c300e76` also checked mixed-view payload loading and returned exit `0` with unchanged inputs.
Its evidence is in `target/verification/state-import/closure-gate.H0Wddk/` under the same limits.
The [state-import record](../../../docs/casper/theory/finalized-floor/state-import-validity.md) maps these properties to required production regressions.
The [cold refinement](../../rocq/cost_accounted_rho/theories/StateImportCold.v) proves exact envelope and collection framing, including accepted trailing bytes.
It binds authenticated payload bytes and guarded insertion to structural and typed reads.
Its consuming scan starts with an empty checked set and derives readable closure from completed validation.
Compatible writes preserve checked decoder results, arbitrary shared roots, and readers with separate alias lookups.
The unsafe structural-only control completes despite a malformed nested value, while the consumer check rejects that value.
The concrete nested Serde decoders remain explicit native conformance obligations.

Invocation `7108f8b6b1994eabab6f271b1d8b1667` checked all eleven modules with unchanged inputs and returned exit `0`.
Its evidence is in `target/verification/state-import/closure-gate.7vmG7J/` under the same resource limits.
The [encoded-cold refinement](../../rocq/cost_accounted_rho/theories/StateImportEncodedCold.v) distinguishes absent aliases from malformed present bytes.
Guarded insertion preserves every existing byte binding and every previously resolved value.
Its normalized logical view exposes strict resolution success without claiming to represent the physical raw alias.
The separate byte-reader proof preserves actual split raw-first reads across compatible commits.

Exact comparison of both observed aliases transfers preflight validation to the current transaction state.
Failed attempts preserve that current state, including concurrent writes missing from the earlier observations.
Arbitrary finite observed attempts refine the guarded-write relation.
The stale-preflight control admits both initial candidates but rejects their conflicting second commit.
The proof requires both importer and checkpoint writers to obey the commit rule.

Invocation `6a29e8a5d0984e8081fc6fc2107a9402` checked all twelve modules and their kernel proof terms with unchanged inputs.
Its gate returned `0` in `target/verification/state-import/closure-gate.oJe0qU/` under the same resource limits.
The initial encoded root-preservation result keeps the supplied history map fixed.
The extended result composes guarded history insertions with arbitrary cold batches and refines the consuming scan's concurrent-write action.
Repeated batch keys require compatible values, and failed batches retain the entire previous commit state.
Exact observations for every touched key transfer preflight staging to the current commit state.
Invocation `6fe49720c48c4cdda17f874308bf7ce8` checked these extensions and all twelve modules with unchanged inputs and kernel checking.
Its gate returned `0` in `target/verification/state-import/closure-gate.Jg9tPI/` under the same resource limits.
The rollback result covers a definite failed transaction, not an unknown commit outcome after cancellation or process failure.
The final corollary embeds observed batches directly into the joint storage relation.
Invocation `399095b3bc5041818b1a6d87cd1fb34b` checked that corollary and all twelve modules with kernel checking and unchanged inputs.
Its gate returned `0` in `target/verification/state-import/closure-gate.5zf40V/` under the same resource limits.
Page overlays, lifecycle composition, concrete codec conformance, and production conformance remain required.
The [occurrence refinement](../../rocq/cost_accounted_rho/theories/StateImportOccurrence.v) preserves original-root context through singleton subtree traversal.
Its annotations erase to the existing page entries, budgets, and residual stack.
Authenticated history paths establish each contextual reference without inferring its kind from an incoming envelope.
Compatible interleaved writes preserve these paths and the final-view annotations.
The fallible occurrence checker checks every required kind before payload deduplication can discard repeated keys.
One payload cannot satisfy conflicting required kinds in the same accepted page.

Invocation `252efbaf49ef46c1b6f132baf3d13f03` checked all thirteen modules with kernel checking and unchanged inputs.
Its gate returned `0` in `target/verification/state-import/closure-gate.HgAE94/` under the same resource limits.
The real shared-singleton regression separately demonstrates acceptance of a hash-preserving wrong-kind envelope.
The [permanent record](../../../docs/casper/theory/finalized-floor/state-import-validity.md#original-root-occurrence-context) states the native evidence and remaining composition requirements.

The reviewed witness binds event keys and leaf/history shape even when an unknown kind makes reference decoding fail.
Invocation `2035d83295744c28a3070c39c095d11a` checked this strengthening and all thirteen modules with kernel checking and unchanged inputs.
Its gate returned `0` in `target/verification/state-import/closure-gate.61smdr/` under the same resource limits.
The path theorems use an abstract reader and require checked-reader instantiation for hash authentication.
The initial interleaved occurrence theorem covers zero-skip, unanchored pages with initial origin and frame evidence.
The initial singleton-origin theorem covers a history step, not the final-history selection or initial anchor.
The checked composition below adds those connections.

The [overlay module](../../rocq/cost_accounted_rho/theories/StateImportOverlay.v) validates every received history row before a temporary lookup view can hide duplicates.
It preserves observed bytes, distinguishes read failure from absence, and rejects incompatible occupied keys.
If one row ordering succeeds, every permutation succeeds with the same lookup results.
The combined theorem requires checked raw bytes in the final reader, including non-emitted cursor ancestors.

The occurrence module now retains annotated slice and cursor relations for Start, Resume, and arbitrary skip counts.
Each emitted occurrence retains its exact step, selected edge, and original-root path.
Erasure and root reachability alone would not distinguish shared keys under different kinds.
The last-history selector retains the exact final occurrence rather than an arbitrary occurrence with the same key.
The overlay composition binds its original-root path to the exact encoded singleton response.

Invocation `a59b0faabd354bec8194397dac484c7f` checked all fourteen modules with kernel checking and unchanged inputs.
Its gate returned `0` in `target/verification/state-import/closure-gate.42QRTI/` under the same 2 GiB resource limits.
The [permanent record](../../../docs/casper/theory/finalized-floor/state-import-validity.md#checked-history-overlay-and-cursor-composition) records three passing native controls and three expected pre-repair failures.
It also records 256 passing generated cursor cases, exact parameter ranges, and strict native lint results.

These are successful final-view proofs, not evidence that actual input/output calls cannot fail.
The [received-cold module](../../rocq/cost_accounted_rho/theories/StateImportReceivedCold.v) adds row normalization, duplicate checks, and exact occurrence authorization.
It connects the operational cursor witness to typed consuming reads after successful guarded cold batches.
The received-only map cannot use durable fallback to hide missing response rows.
Every row receives key, byte-domain, hash, and occurrence-kind validation before duplicate removal.
Compatible envelope trailers can differ, so permutation preserves decoded maps rather than raw bytes.
Its decoded-map theorem includes absent keys.
A constant-hash control rejects conflicting decoded leaves without a hash-injectivity premise.
Invocation `ffe05357ca364d7cbde4b9d981508826` compiled all fifteen modules and kernel-checked their proof terms.
Its gate returned `0` with unchanged inputs in `target/verification/state-import/closure-gate.fexMTL/` under the same 2 GiB resource limits.
The [permanent record](../../../docs/casper/theory/finalized-floor/state-import-validity.md#received-cold-rows-and-guarded-consumption) states the native results and proof limits.

The [observation module](../../rocq/cost_accounted_rho/theories/StateImportObservations.v) connects actual cold-read records to guarded commits and typed consuming reads.
Unread locations, successful absence, observed bytes, and read failures remain distinct.
Successful batches require both physical key formats for every received key.
Each read record retains its own storage state, with compatible writes permitted between reads.
Exact transaction comparisons connect those observations to current storage without a shared preflight snapshot.
The final theorem includes the actual trace, its derived read-once property, existing-byte preservation, and authorized typed consumption.
An explicit valid insertion between key-format reads survives the later successful batch.

The production adapter must cache each location once per preflight attempt.
Failure retention applies to required rows, and a retry uses a fresh cache.
The [permanent record](../../../docs/casper/theory/finalized-floor/state-import-validity.md#physical-cold-observations-and-concurrent-commit) states the proof limits and fault-injection results.
The [history-observation module](../../rocq/cost_accounted_rho/theories/StateImportHistoryObservations.v) records physical observations and logical callback answers separately.
Successful answers derive checked bytes from compatible received rows or actual durable reads.
Cache growth preserves those answers without a shared snapshot.
Observed received-key absence remains a separate branch that requires transaction-time comparison.
The model preserves one physical read per key while permitting repeated logical callbacks.
It retains raw bytes separately from occurrence paths and expected cold kinds.
The [history record](../../../docs/casper/theory/finalized-floor/state-import-validity.md#physical-history-observations-and-logical-lookups) states its controls and native failures.
Invocation `f440693498434eedaf831d56392d3ab9` compiled and kernel-checked all seventeen modules with unchanged inputs.
The evidence is in `target/verification/state-import/closure-gate.RyJcHN/` under the same 2 GiB resource limits.

The [read-tape module](../../rocq/cost_accounted_rho/theories/StateImportReadTape.v) consumes the exact logical callback sequence.
Successful preparation occurs at the first callback's actual physical-trace prefix.
Each required read consumes the next matching record and preserves its failure category.
Accepted completion requires every callback to succeed and leaves no unused records.
The resulting cursor export uses the checked overlay derived from that same trace's final cache.
The module preserves compressed-prefix stacks, shared-hash occurrences, skip counts, and history budgets.
Its stack and slice evaluators cannot exhaust their derived recursion bounds under the stated premises.
The native callback-order and failure-stop properties passed 256 generated cases.
Invocation `b14d3c5d9f8741a1b6b9f70d293d6c06` compiled and kernel-checked all eighteen modules with unchanged inputs.
The evidence is in `target/verification/state-import/closure-gate.txwhbH/` under the same 2 GiB resource limits.
The [permanent record](../../../docs/casper/theory/finalized-floor/state-import-validity.md#exact-callback-sequence-and-cursor-execution) describes the controls and proof boundaries.

This result proves strict accepted-page refinement, not every raw exporter response or completeness for every valid instrumented execution.
Original-root validation reads, complete-root consumption, and publication still require their explicit sequence composition.
The concrete transaction adapter remains a required connection.
The production import-validation repair remains pending.

The observed history transaction now checks current bytes after successful preflight observations.
It accepts concurrent identical insertion and rejects conflicting insertion without overwriting the current store.
Its byte-extension proof feeds the existing joint history/cold run through a decoded-map extension.
This avoids an incorrect single-point-update claim for unrestricted numeric keys.
The accepted read-tape export also retains the same semantics in checked committed history.
Hash and encoding validation can remain outside the transaction lock because commit reuses the exact validated bytes.
Invocation `24f7dab310d24d01856958daf28b798d` kernel-checked all eighteen modules with unchanged inputs in `target/verification/state-import/closure-gate.teXLbf/`.
The native transaction properties passed 640 generated cases, including actual two-thread batch races.
The [transaction record](../../../docs/casper/theory/finalized-floor/state-import-validity.md#observed-history-transactions-and-committed-traversal) states the evidence and remaining integration requirements.

The original single-reference actions could not represent one physical multi-reference transaction as a single abstract step.
The publication model now admits separate compatible history and cold batches in every phase.
The operation refinement connects these effects to retained write ownership and terminal outcomes.
Its complete qualification and native correspondence remain in progress.
The existing tuple-space startup route publishes its root before receiving pages.
The [startup constructor regression](../../../docs/casper/theory/finalized-floor/state-import-validity.md#startup-constructor-publication-regression) now demonstrates both premature tag publication and current-root replacement.

The [owned-page module](../../rocq/cost_accounted_rho/theories/StateImportOwnedPage.v) now connects original-root validation, exact occurrence collection, response checks, and both content transactions.
One generic collector preserves the existing traversal algorithm, errors, budgets, and callback sequence.
Its occurrence specialization computes the actual list used for cold-kind checks before writes.
The model advances the owner only after both transactions succeed.
Known cold failure retains the history prefix and previous owner.

Receipt views remain distinct from the complete live store.
Explicit history extensions and guarded cold runs preserve the checked locations and typed reads during concurrent writes.
Unknown outcomes remain outside the Boolean transaction wrapper.
This result covers emitted cold occurrences, not complete-root closure or publication authority.

Invocation `d984582948cc4cd5bf47a7ebdef7b50c` kernel-checked all nineteen modules with unchanged inputs in `target/verification/state-import/closure-gate.WbUdF0/`.
The four focused native traversal properties passed 512 generated cases, strict Clippy, and formatting checks.
The [permanent record](../../../docs/casper/theory/finalized-floor/state-import-validity.md#original-root-ownership-and-validated-page-commits) documents controls, source correspondence, and remaining production requirements.
It uses the real importer and stores without polling the stream or receiving a page.
This is a native witness for the existing `EarlyTag` control, not a new consensus rule.
The production repair remains pending.
It also does not establish crash durability, garbage-collection safety, memory bounds, or production liveness.
Unrestricted failure and replacement permit infinite unsuccessful executions, so this safety specification makes no unconditional completion claim.

## Physical scan correspondence

`StateImportScan.v` derives complete-root consuming closure from actual fallible history and cold observations.
Its executable traversal preserves full reference identity and retains failed attempts.
The module does not assume a shared read snapshot or use a root marker as closure evidence.

The completed scan now has a preservation theorem for later compatible history and cold writes.
The new operation model records the root-bound receipt separately from publication authority.
It preserves a running operation after observed caller cancellation and distinguishes that operation from a terminal unknown result.
See [read-only complete-root scans](../../../docs/casper/theory/finalized-floor/state-import-validity.md#read-only-complete-root-scans) for the exact guarantees and native test obligations.

## Publication operation checks

`StateImportOperations.tla` uses a named instance of `StateImportPublication.tla`.
The independent operation ledger prevents request replacement from erasing authorized execution.
The marker lookup has four states: unread, found, absent, and failure.
Only a matching completed scan and successful lookup can retire current recovery work.

The concrete shared-history fixture includes an intermediate Data node beneath the original root.
Two distinct descendant contexts share one physical history key and one compatible cold key.
Root-specific importer effects exclude unrelated physical root rows.
Independent checkpoint content writes can occur throughout the scan.

| Configuration | Transfers | Request generations | Operations per attempt | Purpose |
| --- | --- | --- | --- | --- |
| `StateImportOperations` | 2 | 1 | 1 | Shared history, independent transfers, and checkpoint interference. |
| `StateImportOperationsReplacement` | 1 | 2 | 1 | Old operation completion after request replacement. |
| `StateImportOperationsCombined` | 2 | 2 | 1 | Combined transfer and replacement interleavings on a smaller graph. |
| `StateImportOperationsRetry` | 1 | 1 | 2 | Repeated authorization after a finished attempt. |
| `StateImportOperationsOwnedSequence` | 1 | 1 | 3 | Owned history, cold, and publication writes. |
| `StateImportOperationsCancelledSequence` | 1 | 1 | 3 | Authorized publication finishes after caller cancellation. |
| `StateImportOperationsUnknownTagSequence` | 1 | 1 | 3 | Committed tag with unknown return, followed by validated observation. |
| `StateImportOperationsUnknownPointerSequence` | 1 | 1 | 3 | Committed pointer with unknown return, followed by validated observation. |

The first four configurations use unrestricted `Next`.
The sequence configurations establish specific reachable execution witnesses.
Each witness checks all invariants, inclusion in the unrestricted specification, base refinement, and its stated completion condition.
Their fairness condition applies only to these selected finite sequences.
It does not establish production liveness under arbitrary failures.

Ten unsafe configurations each require their exact named invariant violation.
The [checker](../../../scripts/check-state-import-operations.sh) rejects other failures and changed proof inputs.
It requires a systemd memory limit of at most 2 GiB and disables swap.
Artifacts use `target/verification/state-import/`, not `/tmp`.

Run one selected configuration with this command:

```bash
systemd-run --user --scope -p MemoryMax=2G -p MemorySwapMax=0 -p CPUQuota=100% \
  bash scripts/check-state-import-operations.sh StateImportOperationsOwnedSequence
```

The [publication record](../../../docs/casper/theory/finalized-floor/state-import-validity.md#publication-operation-refinement) explains the source mapping, limitations, and native obligations.
The full concurrent qualification remains incomplete.

## Checkpoint construction and first publication

`StateImportCheckpointPublication.tla` adds independent checkpoint calls to `StateImportOperations.tla`.
The companion retains the inherited import transitions and invariants.
It adds first-publication transitions, so it does not claim refinement to the earlier action relation.

A call captures its local base rather than following the shared current-root pointer.
Cold and history transactions precede construction evidence.
The evidence contains full contextual references and derives coverage of the target root.
The model does not guard publication by assuming target closure.
A successful root-tag return precedes pointer execution.

`CpStart` authorizes the complete synchronous call.
Cancellation suppresses repository delivery but does not revoke the call's remaining internal writes.
`CpSeal` records completed construction evidence, not a new caller authorization.
Committed effects, returned results, and execution ownership remain separate.
Per-stage execution counts detect repeated writes after terminal settlement.

| Safe configuration | Import actors | Checkpoint actors | Import generations | Captured base |
| --- | --- | --- | --- | --- |
| `StateImportCheckpointPublication` | 1 | 1 | 1 | Complete empty root. |
| `StateImportCheckpointShared` | 1 | 2 | 1 | Complete root with a shared Joins subtree. |
| `StateImportCheckpointReplacement` | 1 | 1 | 2 | Complete root with a shared Joins subtree. |
| `StateImportCheckpointFresh` | 1 | 1 | 1 | No inherited references. |

Each checkpoint actor can issue one call in these configurations.
Each import generation can issue one owned operation.
The shared-subtree fixture represents distinct Data updates over common Joins content.
The configurations use unrestricted concurrent transitions, not selected execution sequences.

`Fresh` removes inherited references from construction evidence.
Inherited initialization still contains the initial history record.
Therefore, `Fresh` does not reproduce the missing physical empty-root record in native initialization.
That defect has its own native regression and separate construction obligation.

Nine unsafe configurations target nine specific invariants.
Their names are `FollowCurrentRoot`, `OmitConstructedChild`, `SealBeforeConstruction`, `PublishOtherRoot`, and `PointerAfterTagError`.
The other names are `DropRunningCheckpoint`, `ReportUnknownPointerSuccess`, `ReportAfterCancellation`, and `TerminalLatePointer`.
Each filename prefixes that name with `StateImportCheckpoint` and uses `.cfg`.
The [publication record](../../../docs/casper/theory/finalized-floor/state-import-validity.md#checkpoint-publication-and-cancellation) maps each control to its invariant.

The [checkpoint checker](../../../scripts/check-state-import-checkpoint.sh) requires the exact expected native exit and invariant failure for each unsafe configuration.
It binds the checker, imported specifications, fixture, and selected configuration by source hash.
All nine controls passed this evidence check at invocation `4de685779dc147d2915caf8e1e1c2e5c`.
The main safe configuration passed 7,106,400 distinct states in `checkpoint-publication.JhEVrx`.
Its gate returned `0` with unchanged inputs.
The shared, replacement, and fresh configurations remain under evaluation.

Run one selected configuration with this command:

```bash
systemd-run --user --scope -p MemoryMax=2G -p MemorySwapMax=0 -p CPUQuota=100% \
  bash scripts/check-state-import-checkpoint.sh StateImportCheckpointPublication
```

The command stores artifacts under `target/verification/state-import/` rather than `/tmp`.
The checker rejects an unlimited memory scope or enabled swap.
Bounded model checking does not prove native writer correspondence, unlimited concurrency, eventual completion, or root-retention bounds.

## Load-before-selection checks

`StateImportRootSelection.tla` checks root loading before pointer selection.
Two callers can independently load and select targets while other operations insert valid rows and publish markers.
Each call separates its physical selection effect from success, error, or uncertain return.
Per-call counters distinguish an unauthorized write from another call's legitimate pointer change.

`StateImportRootSelection.cfg` checks all bounded interleavings of two calls over three roots.
The initial selected root is readable, present rows are immutable, and markers only accumulate.
Row tokens distinguish correct, missing, malformed, and wrong-hash content.
These tokens abstract concrete codec behavior and do not establish descendant closure.

`StateImportRootSelectionBeforeLoad.cfg` violates `SelectionRequiresSuccessfulLoad`.
`StateImportRootSelectionAfterLoadFailure.cfg` violates `FailedLoadHasNoSelectionWrite`.
Both controls must produce their named invariant failure.
The latest safe check completed 29,340 distinct states in `root-selection.zyUQ4z`.

Run the safe configuration with this command:

```bash
systemd-run --user --scope -p MemoryMax=1G -p MemorySwapMax=0 -p CPUQuota=100% \
  bash scripts/check-state-import-root-selection.sh StateImportRootSelection
```

The [publication record](../../../docs/casper/theory/finalized-floor/state-import-validity.md#root-loading-before-root-selection) gives the native reset regression and proof limits.
Its [finite-retry section](../../../docs/casper/theory/finalized-floor/state-import-validity.md#finite-retries-for-shared-cold-storage) connects guarded storage writes to fresh physical observations.
The retry bound is inductive Rocq evidence, not a finite-actor TLA+ estimate.
