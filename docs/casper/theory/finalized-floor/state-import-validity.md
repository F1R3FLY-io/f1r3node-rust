# State import validity and recovery ownership

## Status and purpose

Real-store regression tests reproduce state-import defects in runtime recovery and startup horizon recovery.
The tests also reproduce a cold-key interoperability defect and an exporter page-boundary panic.
Production fixes remain pending formal verification and review.

This work preserves Casper voting, fork choice, finalization thresholds, and the existing state-transfer wire format.
It does not authorize a new consensus architecture or a new storage namespace.
The upstream team must review any repair that requires such a change.

The evidence below comes from the September 7, 2026 working tree.
It demonstrates local defects, not the cause of a particular historical continuous integration (CI) failure.

## Storage objects and completion

A **history node** stores part of the content-addressed radix trie.
A **cold value** stores data referenced by a history node.
A **root marker** records that a root exists in the root store.
A **cursor** identifies the requested position in an exported traversal.

Complete state requires every reachable history node and cold value needed by its readers.
A root marker alone does not establish this closure.
The requester must preserve its recovery obligation until it establishes the required state.

The current runtime and horizon paths write supplied items before content validation.
They then record the root marker and check that marker through `has_root`.
The concrete importer does not check hashes internally.
Therefore, a matching request path does not prevent invalid bytes or missing values from producing false completion.

Sources:
[runtime requester](../../../../casper/src/rust/engine/runtime_state_requester.rs),
[horizon requester](../../../../casper/src/rust/engine/lfs_horizon_requester.rs),
[concrete importer](../../../../rspace++/src/rspace/state/instances/rspace_importer_store.rs), and
[root store](../../../../rspace++/src/rspace/history/roots_store.rs).

## Comparison with pinned dev

The comparison uses dev commit `0f5d2b7414786cd27b8a686ca636485c35a5edce`, dated September 7, 2026, at 18:56:50 UTC.
This pin prevents later merges from changing the comparison silently.

| Boundary | Pinned dev | This branch | Repair implication |
| --- | --- | --- | --- |
| Runtime import | Writes history and cold values without page validation. Checks the root marker after publication. | Retains the same validation gap. Adds bounded retry ownership and backoff. | Keep the validation repair separate from retry-policy differences. |
| Horizon import | Imports unchecked pages and accepts their continuation cursors. | The production file matches the pin. Only the new test-module declaration differs. | This defect is not a branch-specific consensus redesign. |
| Startup tuple-space import | Validates pages before content writes, but records the startup root before receiving pages. | The source file matches the pin. | Reuse valid page checks. Publish availability only after complete state validation. |
| Cold-value import and export | Import uses raw hash keys. Export uses serialized hash keys. | Both source files match the pin. | Restore interoperability without inventing another key namespace. |

Pinned sources:
[runtime import](https://github.com/F1R3FLY-io/f1r3node-rust/blob/0f5d2b7414786cd27b8a686ca636485c35a5edce/casper/src/rust/engine/runtime_state_requester.rs),
[horizon import](https://github.com/F1R3FLY-io/f1r3node-rust/blob/0f5d2b7414786cd27b8a686ca636485c35a5edce/casper/src/rust/engine/lfs_horizon_requester.rs),
[startup validation](https://github.com/F1R3FLY-io/f1r3node-rust/blob/0f5d2b7414786cd27b8a686ca636485c35a5edce/casper/src/rust/engine/lfs_tuple_space_requester.rs),
[imported cold keys](https://github.com/F1R3FLY-io/f1r3node-rust/blob/0f5d2b7414786cd27b8a686ca636485c35a5edce/rspace%2B%2B/src/rspace/state/instances/rspace_importer_store.rs), and
[exported cold keys](https://github.com/F1R3FLY-io/f1r3node-rust/blob/0f5d2b7414786cd27b8a686ca636485c35a5edce/rspace%2B%2B/src/rspace/state/instances/rspace_exporter_store.rs).

The local snapshots reside under `target/verification/state-import/dev-0f5d2b7/`.
The snapshots are disposable evidence, not the permanent specification.

## Regression evidence

The [runtime tests](../../../../casper/src/rust/engine/runtime_state_import_tests.rs) drive the real requester core.
The [horizon tests](../../../../casper/src/rust/engine/horizon_state_import_tests.rs) drive the actual startup stream and its request-response sequence.
Both suites use independent history, cold, and root stores through `InMemoryStoreManager` and the real importer.
Every destination starts with an independent, valid current root.

| Defect | Runtime result | Horizon result | Required behavior |
| --- | --- | --- | --- |
| History bytes do not match their key. | Existing data changes and recovery retires. | Existing data changes and startup reports completion. | Reject before stored data changes. |
| Cold value is corrupt. | The import overwrites a valid value under the same key. | The same overwrite occurs. | Preserve the valid value and incomplete recovery. |
| Referenced cold value is absent. | The root marker appears and recovery retires. | The root marker appears and startup reports completion. | Require complete validated closure before publication. |
| Continuation cursor is forged. | The cursor changes and the page counts as progress. | The page counts as completed progress. | Derive the accepted continuation from canonical traversal. |
| Imported state is exported again. | Local typed reads succeed, but export omits the cold item. | The same concrete storage adapters apply. | Preserve both local reads and peer re-export. |
| A continuation contains leaves but no new history nodes. | The common exporter is implicated. | The exporter regression panics with `EmptyHistoryException`. | Preserve all leaf values and a valid completion cursor. |

The cold-key failure is not a local-read failure.
The [history reader](../../../../rspace++/src/rspace/history/instances/rspace_history_reader_impl.rs) supports raw and serialized hash keys.
The exporter queries only serialized keys, while the importer writes raw keys.
The round-trip test first proves that imported joins remain readable through normal channel lookup.
It then proves that the actual exporter omits their cold data.

The exact-boundary exporter test uses a page size of one to isolate the leaf-only continuation.
It does not reproduce that boundary at production page sizes.
Production-sized boundary cases remain required conformance checks.

### Passing controls

The controls establish that rejection tests do not depend on unusable fixtures or fabricated pagination.

- Valid nonempty pages import state readable through normal channel hashing and typed readers.
- Unknown response paths preserve stored values and pending recovery.
- Runtime rejection checks preserve the original retry owner and support redispatch after a controlled clock advance.
- The horizon stream completes two related states with 16,384 shared channel entries and one additional entry.
- That large control requires actual seven-entry cursors at the horizon page size of 1,024 history nodes.
- Shared singleton cursors complete both waiting roots when both responses arrive before the shared response.
- A late root reopens a shared cursor after the first root has completed.

The last two controls use actual exporter pages and check every shared channel through the destination reader.
They do not block a send operation while waiting for another response.
They control response order without detached tasks or sleep-based synchronization.

### Qualified focused runs

| Suite | Native result before repair | Other checks | Evidence directory |
| --- | --- | --- | --- |
| Runtime | Five failures and two controls passed. Exit `101`. Execution took 0.01 seconds after compilation. | Strict Casper Clippy, formatting, and input hashes passed. | `target/verification/state-import/red-channel.IFk7Cq` |
| Horizon and exporter | Five failures and five controls passed. Exit `101`. Execution took 11.18 seconds after compilation. | Strict Casper Clippy, formatting, and input hashes passed. | `target/verification/state-import/horizon-final-red.SYv9Iq` |

The runtime invocation was `4bc16d48263e4252876541fa2c99e058`.
The horizon invocation was `e5ae3ae6a7c540a7819daffe2fc3c59b`.
Each systemd scope limited memory to 5 GiB, disabled swap, and limited execution to one CPU.
Only one native build ran at a time.

These are successful defect reproductions, not passing repaired suites.
They do not establish disk durability, arbitrary concurrency, or whole-node liveness.

The final review corrected two positive-control assertions before this run.
The controls now permit valid shared singleton requests to repeat and track page-budget crossings separately for each root.
Rejection checks also require explicit startup failure or an actionable original request.
A silent transition to an unrequestable `Received` state does not satisfy that requirement.

## Canonical cursor semantics

The [export traversal](../../../../rspace++/src/rspace/state/rspace_exporter.rs) produces two relevant cursor forms.
The [page adapter](../../../../rspace++/src/rspace/state/exporters/rspace_exporter_items.rs) exposes those forms through the existing wire message.

| Cursor form | Meaning | Shared-root behavior |
| --- | --- | --- |
| Seven entries | The original root, encoded traversal prefix, and final history-node hash identify a bounded continuation. | Distinct original roots cannot share the complete cursor. |
| Singleton history-node hash | Traversal exhausted its current tree at the final visited history node. | Distinct roots can share this cursor when their final subtree is shared. |

A request for the singleton starts traversal at that shared history node.
When that export returns the same singleton, the current requester recognizes a terminal response.
Requiring the original root in every cursor would reject valid existing behavior.

The horizon page size is 1,024 history nodes.
Runtime and tuple-space requests use 750 history nodes.
Leaf count and stored-row count do not establish a page-budget crossing.

The leaf-only defect occurs in the traversal-to-page adapter.
The adapter removes its last collected entry before it installs a final history-node entry.
When no new history node exists, it removes a leaf without that replacement.
The one-leaf continuation becomes empty and triggers the observed panic.
This defect requires its own correction and formal correspondence check.

## Formal verification and acceptance

The [existing finalized-floor model](../../../../formal/tlaplus/finalized_floor/README.md) starts after authenticated import and complete-root publication.
Its documentation explicitly states that current Rust network import does not satisfy that premise.
The new regressions demonstrate the excluded boundary.
They do not establish that the earlier model verified network import.

Task `pr216-state-import-formal` must model independent transfers, readers, checkpoints, partial writes, cancellation, shared nodes, and retries.
The model must derive closure from reachable stored data rather than assume that a marker means usable state.

| Required invariant | Implementation consequence |
| --- | --- |
| `InvalidPageHasNoStoreEffect` | Reject invalid content before writes. |
| `AcceptedPageMatchesRequestedCursor` | Validate the requested traversal and its canonical continuation together. |
| `ProgressRequiresValidatedPage` | Invalid input cannot reset successful-progress or retry counters. |
| `PublishedRootHasCompleteValidatedClosure` | Publish only after all required content is valid and available. |
| `FailedImportRetainsRetryOwner` | Storage failure or rejection cannot report completed recovery. |
| `InvalidImportPreservesExistingRoots` | Protect existing root data and current-root state. |
| Complete export-import round trip | Local readers and later peers obtain the same referenced values. |
| Leaf preservation at page boundaries | A leaf-only continuation neither drops values nor invents a terminal marker. |

The proof must include both canonical cursor forms and multiple roots sharing one terminal cursor.
The native properties must cover duplicate and out-of-order pages, corruption, missing descendants, and both production page sizes.
The concurrency checks must cover shared publication and reader lifetimes where the implementation permits overlap.
Loom models require an explicit correspondence to the real synchronization boundaries.

The repair cannot rely only on calling the existing state validator.
That validator checks content and traversal membership, but its interface does not accept the response cursor.
The implementation must expose or validate the canonical traversal result before it advances the cursor.

Task `pr216-state-import-repair` follows formal review.
Task `pr216-state-import-conformance` requires the complete invariant-to-test correspondence before release acceptance.
The [ownership inventory](../cost-accounting-impl/runtime-ownership-inventory.md) records the wider runtime and storage obligations.

## Concurrent refinement design

The plan review selected a separate state-import composition model.
The existing replay and runtime-isolation models remain consumer contracts, not substitutes for byte-level import verification.
This section specifies required work. It does not report completed model checks.

### Independent actors and physical state

The initial finite instance contains two import actors, two roots with shared history, one reader, and one checkpoint actor.
Each actor advances independently.
Later instances must vary graph shape, shared references, request generations, and page size.

The physical state contains history records, raw cold-key records, serialized cold-key records, root tags, and the current-root pointer.
The transfer state contains request owners, request generations, outstanding cursors, received pages, validation work, and write results.
Each reader captures its own root.
The model must not make a later current-root change alter that captured root.

The import writes occur in four separate actions:

1. Write a validated history batch.
2. Write a validated cold-value batch.
3. Write the completed root's tag.
4. Write the current-root pointer.

These actions do not constitute one storage transaction.
The [LMDB store](../../../../shared/src/rust/store/lmdb_key_value_store.rs) opens a transaction for each `put` call.
The [root store](../../../../rspace++/src/rspace/history/roots_store.rs) writes the tag and pointer through separate calls.
Other actors can observe or change storage between those calls.

A successful tag write can expose valid complete state before the pointer write succeeds.
Another verified observation can therefore resolve recovery even when the first attempt later reports a pointer-write error.
The first failed attempt must not report success itself.
Successful import must not require the current-root pointer to remain unchanged after publication.
A concurrent checkpoint can legitimately select another complete root.

### Graph-derived validation

The verification frontier starts with the requested root occurrence.
An occurrence includes its traversal prefix, not only its hash.
The prefix determines the expected cold-value kind.

Each validated history read adds the decoded child and cold references to the frontier.
Each validated cold read checks the expected payload and kind.
A checked-reference set records the completed work.
The coverage invariant requires every reachable reference to remain checked or reachable from the frontier.

An empty frontier establishes closure only if the checked bindings remain valid in storage.
Concurrent insertions must preserve those bindings.
Page composition must preserve each root's coverage separately.
A shared terminal page cannot replace a missing predecessor page.

The [cold-value representation](../../../../rspace++/src/rspace/history/cold_store.rs) requires two distinct predicates.
Payload-hash validity checks the serialized inner leaf.
Kind compatibility checks the `PersistedData` variant against its authenticated trie occurrence.
The current validator does not hash the outer variant tag.
The model must therefore permit different variant tags with the same checked payload hash.
It must not label this case a cryptographic hash collision.

This type-tag concern is a source finding at this stage.
It is not part of the qualified native results above.
An implementation fix still requires a dedicated regression and reviewed preservation rules for existing typed bindings.

### Transition correspondence

| Model actions | Required correspondence |
| --- | --- |
| Acquire, dispatch, receive | Match the current request for the immutable root before a page obtains authority. |
| Check history | Validate content hashes and node decoding without storage changes. |
| Check cold value | Validate payload hashes, expected kinds, and compatible existing representations. |
| Traverse page | Follow decoded references from the requested traversal and preserve the frontier. |
| Check cursor, accept page | Match supplied items and the canonical continuation before successful progress. |
| Write history, write cold values | Commit only accepted bytes and preserve previously usable bindings. |
| Authorize publication, write root tag | Establish stored typed closure before readiness becomes visible. |
| Write current root | Select a complete root without changing existing readers' captured roots. |
| Fail, retry, complete | Keep unresolved obligations actionable and distinguish failed attempts from separately verified completion. |
| Attach or reopen shared cursor | Preserve each root's validated predecessor chain. |
| Cancel, observe cancellation, replace request | Prevent stale completion without deleting previously committed valid content. |
| Read captured state, checkpoint | Permit overlap with every separate import write. |
| Export leaf-only continuation | Preserve all cold values and derive a valid continuation. |
| Read raw or serialized cold key | Resolve compatible values consistently for readers and exporters. |

Tokio cancellation does not interrupt arbitrary statements in the current synchronous message handlers.
The model must place cancellation observation at actual execution boundaries.
Other threads and storage failures can still interleave with those handlers.
Cancellation must not retroactively revoke a durable write that already had valid authority.

The runtime requester currently keys entries by root and processes each response through a synchronous, exclusive `Core` borrow.
Another block owner can join the same root entry without creating a replacement request.
An older peer response can satisfy a current request for that same immutable root after complete validation.
The model's generation distinguishes local operation lifetimes, not a new network field or a requirement to reject such responses.
A different root belongs to a different entry.

Invalid-page preservation must use attributed write events and existing-binding preservation under concurrency.
Global store equality over a concurrent interval would incorrectly prohibit unrelated checkpoint writes.
The isolated native snapshot tests can use equality because no unrelated writer runs in those tests.

### Inductive proof obligations

The Rocq layer must quantify over arbitrary finite transition histories and finite maps.
The proof must not restrict funding, graph, or actor semantics to the small model-checker instance.

1. Checked history bytes determine the same decoded references.
2. Payload-hash validity does not imply cold-value kind compatibility.
3. Canonical traversal preserves frontier coverage.
4. Page composition preserves each requested root's coverage.
5. Shared-cursor attachment preserves separate predecessor chains.
6. An empty validated frontier implies stored typed closure.
7. Compatible insertion preserves every previously closed root.
8. A failed write preserves the already committed validated prefix.
9. Publication authority implies closure at the tag-write boundary.
10. Compatible concurrent writes preserve captured readers' closure.
11. Reader and exporter alias resolution agree.
12. Request cancellation and replacement prevent stale completion.
13. Leaf-only page construction preserves every collected leaf.

History-hash assumptions must state their cryptographic scope explicitly.
Cold-value uniqueness cannot follow from a hash that excludes the variant tag.

### Counterexamples and review boundaries

The negative controls must exercise omitted hash checks, late validation, wrong kinds, forged cursors, missing references, premature publication, and false completion.
Other controls must exercise stale completion, shared-root loss, missing cursor reopening, leaf removal, omitted raw keys, and conflicting aliases.
A cleanup control must detect removal of a concurrent checkpoint's required data.

Liveness requires eventual valid responses, successful storage operations, and continued recovery ownership.
Fair scheduling alone cannot make an unavailable peer return valid state.

Several decisions require additional evidence or upstream review:

- Whether network import should change the global current-root pointer.
- How to recover roots that an earlier version incorrectly tagged as complete.
- How to prevent incompatible raw and serialized aliases from changing a reader's selected value.
- How equal payload hashes with different kinds interact with existing checkpoint storage.
- Whether storage failures remain panics or become returned requester errors.
- Which caller retains recovery after a whole service or process fails.

The current importer returns no write result and panics on storage errors.
A model with returned write errors therefore requires an explicit API correspondence, not an assumed implementation behavior.

## Checked closure and storage results

The [closure module](../../../../formal/rocq/cost_accounted_rho/theories/StateImportClosure.v) now represents context-independent radix edges and context-bearing history references.
Each edge extends the occurrence path with its slot index and compressed prefix.
The path determines the expected cold-value kind.
The model uses the repository prefix assignments: data `0`, continuations `1`, and joins `2`.
Hashing the node does not depend on its occurrence path.

The shared-node example derives different expected leaf kinds from the same node under different occurrence paths.
The missing-root and missing-reference lemmas establish that closure cannot succeed through an absent read.
Successful child extraction accounts for every decoded edge through a `Forall2` relation.
The empty-frontier theorem establishes structural closure with hash and envelope-kind checks under compatible concurrent writes.
It does not check nested cold-value decoding.
The consuming-scan proof below establishes that stronger property from individual decoder checks.

The [storage module](../../../../formal/rocq/cost_accounted_rho/theories/StateImportStorage.v) distinguishes three preservation properties.

| Property | Meaning | Limitation |
| --- | --- | --- |
| Checked-read preservation | Previously successful checks return the same child references. | A cold check returns no children, so this property alone does not preserve its payload. |
| Logical binding preservation | History bindings and successfully resolved cold values retain their exact values. | Moving a value between aliases can still defeat separate reader lookups. |
| Physical binding preservation | Existing history, raw-key, and serialized-key bindings remain unchanged. | Compatible guards must also prevent a new alias from changing the selected value. |

The proof includes a counterexample to logical preservation alone.
Both logical states contain the value, but the reader checks raw storage before migration and serialized storage after migration.
Both lookups miss.
This is a model counterexample, not an additional reproduced native failure.

Guarded insertion checks both aliases at the transaction boundary and changes only its selected alias.
The exact-binding theorem does not require global alias agreement.
An unrelated preexisting alias conflict therefore does not invalidate preservation of an already usable root.
The theorem does not repair that conflict or classify its affected root as usable.

History and cold batches form separate transactions.
The proofs cover arbitrary finite batch histories, including rejected batches and failed storage commits.
They preserve every previously closed root in any finite root collection.
They also preserve exact reader values when the raw and serialized lookups occur in different states.
No theorem assumes that different payloads cannot share a hash.

The focused gate compiled both modules and checked their proof terms with the Rocq kernel.
The latest run includes the lemma that removes already checked references without losing frontier coverage.
Invocation `b1518894461e4bc2b95adadc471af953` returned exit `0` after compilation and kernel checking.
Its evidence resides in `target/verification/state-import/closure-gate.KT2X4D/`.
The scope limited memory to 2 GiB, disabled swap, and used one CPU.

### Transaction correspondence requirements

The guarded insertion specification requires one atomic observation and update of both aliases.
The existing strict transaction interface can express this operation without duplicating the stored value.
It must compare both observed bindings in the same cold-store transaction.
One compare-and-swap operation changes the target alias.
An identity compare-and-swap operation verifies and retains the other alias.
A target-only absent-or-equal insertion does not protect the other alias against a concurrent conflicting write.

The failed-batch theorem proves the abstract transaction contract.
Backend conformance must establish rollback and distinguish pre-commit failure from a crash after durable commit.
Separate history and cold transactions do not imply atomic whole-import publication.
Current checkpoint writers also need conformance checks because their contains-then-put path does not implement these guards.

The structured proof representation does not yet establish raw-codec correspondence.
That correspondence must cover decoding failures, index bounds, path lengths, and exact bytes.
Guarded insertion protects existing values but does not authorize newly supplied content.
Page validation and publication must supply that authority before the production importer can satisfy these results.

## Concurrent publication model

The [publication model and correspondence guide](../../../../formal/tlaplus/state_import/README.md) separate content preservation from lifecycle decisions.
Transfer actors write history and cold references independently.
They scan their own frontier, publish a tag, and update the current-root pointer through distinct actions.
A reader captures one root and checks its references while other actors continue.
Checkpoint selection can change the current root without changing that captured root.

The two-importer configuration passed nine safety invariants over 71,450 distinct states and 465,135 generated states.
The complete search reached depth 40 and took seven seconds.
Invocation `4163ccb26f3f40518223c81a5eced7d1` produced exit `0` and unchanged input hashes for this configuration.
Its evidence resides in `target/verification/state-import/publication.jH4wxc/`.

The combined configuration also passed all nine invariants with two importers and two generations per importer.
It explored 14,183,794 distinct states and generated 124,905,528 states, with an empty final queue.
The search reached depth 52 and completed in 21 minutes, 52 seconds.
The same invocation returned exit `0` with unchanged inputs in `target/verification/state-import/publication.XFsY2F/`.
TLC reported a calculated fingerprint-collision estimate of `8.5e-5` for that search.
This finite, hash-based search is not an unbounded machine-checked proof.

The publication replacement configuration also completed successfully.
The premature-tag, failure-retirement, stale-completion, and shared-state-removal controls each produced their named invariant violation with native exit `12`.

The ownership refinement adds write permissions, separate tag and pointer permissions, retirement witnesses, and an independent reader-capture record.
Its positive configurations must also refine the unchanged base specification.
The [model guide](../../../../formal/tlaplus/state_import/README.md#ownership-refinement) defines each obligation and its source correspondence.
Neither the base model nor this refinement requires new wire generations.

The final ownership inputs passed the two-importer configuration over 80,654 distinct states and the replacement configuration over 15,192 distinct states.
Both checks also verified the `RefinesBase` property.
Five controls produced their named invariant violations, including an actual reference insertion without a matching write permission.
Invocation `28774996d0104246b5ddd05a71aaf148` records these results under `ownership.sTd59D/`, `ownership.Auzx11/`, and the associated control directories.
The combined ownership configuration also completed under that invocation.
It generated 195,081,784 states and found 22,456,308 distinct states, with an empty final queue.
It checked all listed invariants and `RefinesBase`, reached depth 56, and returned exit `0` after approximately 3 hours, 43 minutes.
The unchanged-input evidence is in `target/verification/state-import/ownership.CSGfro/`.
TLC reported fingerprint-collision estimates of `2.1e-4` calculated optimistically and `6.9e-6` from the actual fingerprints.
These estimates qualify the finite hash-based search and do not constitute an unbounded proof.

### Ownership parameter-domain correction

The ownership model now excludes its `"none"` sentinel from the reference domain.
This restriction represents the distinction between an absent permission and a permission for a stored reference.
Without the restriction, the generic model permits a reference named `"none"` to match an absent write permission.
All original concrete configurations already excluded that value.
This is a model-domain correction, not evidence of a production permission defect.

The invalid-domain control exposed a separate verification issue.
TLC explored the control when the new assumption appeared only inside the instantiated ownership module.
That run returned native exit `0`, so the gate correctly failed its expected-exit check.
The evidence is in `target/verification/state-import/ownership.qGRy0H/`.
The two entry modules now also declare the assumption explicitly.
The invalid-domain control then returns native exit `10` before initial-state exploration.

The completed Combined run retains its original input manifest.
Removing only the added assumption line reproduces each original module hash below.
The transition operators, properties, graph, and Combined configuration remain unchanged.
Both concrete domains satisfy the added restriction, so the corrected Combined fixture describes the same transition system.
This correspondence argument does not relabel the historical run as a run of the edited files.

| Module | Original SHA-256 | Corrected SHA-256 |
| --- | --- | --- |
| `StateImportOwnership.tla` | `341e2dea487e1ee66b842b0572213c8d2ed05647fa3cef2c8e13d8a714531697` | `7279193a855a0d1205524e02996d7dbf67b50a8779e55ba6a8fef241b31d6af8` |
| `MCStateImportOwnership.tla` | `7daf275048ddc1804b39a766432f7027aa4b8c24620f8fa8d590db86bec27ece` | `ebf82948b9ce5060ac6b7b3acec9877344a3c59c44e69126589d625efe1ef41d` |

Invocation `9f29d62cd154435a993b63ab7f66d2db` checked the edited models with the same 2 GiB, zero-swap, one-CPU limits.
All eight gates returned exit `0`, with unchanged input hashes.

| Configuration | Result | Evidence under `target/verification/state-import/` |
| --- | --- | --- |
| Invalid domain | Assumption rejection before exploration. Native exit `10`. | `ownership.Jrkceq/` |
| Two importers | 508,281 generated states, 80,654 distinct states, depth 44, empty queue. Native exit `0`. | `ownership.TrE7yJ/` |
| Replacement | 73,690 generated states, 15,192 distinct states, depth 34, empty queue. Native exit `0`. | `ownership.tCkPr6/` |
| Late authority | `TagAuthorizationCurrent` violation. Native exit `12`. | `ownership.K9ZX6p/` |
| Retirement before tag | `RetiredRecoveryHasPublishedEvidence` violation. Native exit `12`. | `ownership.qx25xj/` |
| Retirement after failure | `RetirementHasWitness` violation. Native exit `12`. | `ownership.Gy2qKC/` |
| Reader follows current root | `ReaderUsesCapturedRoot` violation. Native exit `12`. | `ownership.ZgYTdn/` |
| Unmatched write | `WriteCompletionMatched` violation. Native exit `12`. | `ownership.jJcckZ/` |

The corrected Combined configuration subsequently passed under invocation `286bc97e12b141aa90543ff0a37c3e91`.
Its unchanged-input gate returned `0` in `target/verification/state-import/ownership.tS8Hdp/`.
The search generated 195,081,784 states and found 22,456,308 distinct states, with depth 56 and an empty final queue.
The run took 4 hours and 33 minutes under a 2 GiB memory limit, no swap, and one CPU.
Fingerprint-collision estimates were `2.1e-4` optimistically and `2.1e-5` from actual fingerprints.
This result directly checks the corrected inputs rather than reclassifying the historical run.

These are finite-instance lifecycle results under the documented storage contract.
They do not establish page authentication, cursor correctness, codec correctness, or whole-node liveness.
The complete import verification task remains open.

## Page representation repair plan

The plan review selected an explicit internal page result containing both entries and a continuation cursor.
The wire message retains its existing fields.
The exporter must not derive completion metadata from the last payload entry because a valid page can contain only leaves.

| Traversal result | Required cursor and payload behavior |
| --- | --- |
| History budget reached with a new history node | Preserve the existing seven-entry cursor. |
| Traversal exhausted with new history nodes | Preserve the final-history singleton cursor. |
| Valid resumed traversal exhausted without new history nodes | Preserve every leaf and return the exact input path as terminal. |
| Missing required node or invalid cursor | Return an error, not successful exhaustion. |

The repair must not fabricate a cold-leaf singleton or insert a previous history node solely to carry a cursor.
These alternatives would change the cursor domain or payload accounting without need.
Previously valid cursor outputs remain unchanged.
An older validator with the same leaf-loss defect might still reject the repaired boundary, despite unchanged wire fields.

### Checked adapter foundation

The [page module](../../../../formal/rocq/cost_accounted_rho/theories/StateImportPage.v) proves that cursor attachment preserves every selected leaf and history occurrence.
The results quantify over arbitrary lists and finite page sequences.
They retain duplicate occurrences and preserve each supplied history-budget bound.
Leaf visits remain separate from history-budget accounting.

Two executable proof examples reproduce the old adapter's singleton-leaf and final-leaf loss.
The positive leaf-only theorem proves that the corrected representation cannot turn a nonempty leaf list into an empty page.
These proofs concern page adaptation, not which references traversal should select.
They do not yet prove canonical cursor encoding or raw-byte decoding.

The module also specifies page partitioning over a supplied traversal trace.
The partition preserves every occurrence across the selected prefix and remaining suffix.
A positive budget selects at least one occurrence from a nonempty trace.
History occurrences consume the budget, including repeated hashes at different positions.
Leaf occurrences do not consume that budget.

An exact-budget example leaves a leaf-only suffix, and the next page preserves both leaves in a two-leaf example.
These results still require a proof that decoded-tree traversal produces the supplied trace.
They do not replace the cursor and decoder obligations below.

Invocation `906b5de79195450eb3c212b5585140a2` compiled all three import modules and checked their proof terms with the Rocq kernel.
It returned exit `0` with unchanged inputs in `target/verification/state-import/closure-gate.A5LBYr/`.
The run used a 2 GiB memory limit, no swap, and one CPU.

### Remaining cursor and traversal obligations

The validator must receive both the exact requested path and the response continuation.
Its current interface lacks the response continuation.
Validation must check canonical field counts, 32-byte hashes, absent optional indexes, size bytes, and unused padding.
The encoded prefix must reach a real history node.
The seventh cursor entry must equal that node's hash.

The traversal proof must use occurrences with paths, not only distinct content hashes.
Shared nodes can occur under different prefixes.
History occurrences consume the page budget, but leaf visits do not.
Payload-key deduplication occurs after selection, so payload count alone cannot establish completion.
Payload order remains unspecified because the current exporter uses hash sets.

Successful decoding must supply the exact edges that traversal and typed closure inspect.
The decoder must reject incomplete records, duplicate indexes, invalid prefixes, and missing required pointers through returned errors.
Hash validity alone cannot establish these properties.
The validator must check received history hashes before any transformation that changes the bytes.

Each request must retain its own predecessor-page coverage when it joins a shared singleton cursor.
A later root must be able to reopen a previously completed shared cursor.
The runtime's single-root cursor map also needs a regression for this case.
Terminal pagination cannot replace full closure validation for each original root.

Native conformance must cover exact history boundaries at both `750` and `1024`, with asserted fixture node counts.
It must also cover adjacent pages, multiple leaves, empty tails, cursor round trips, invalid representations, and simultaneous or late shared-cursor consumers.
Invalid responses must preserve durable bindings, publication state, and actionable recovery ownership.

## Checked radix codec contract

The [codec module](../../../../formal/rocq/cost_accounted_rho/theories/StateImportCodec.v) defines a total parser for the existing radix record format.
Each record contains a slot byte, a header byte, a prefix, and a 32-byte target.
The header's high bit selects a history pointer or cold leaf.
The remaining seven bits give a prefix length from zero through 127.

The acceptance theorem is an equivalence, not only a successful-parser implication.
The parser accepts exactly valid records with unique slots whose ordered serialization equals the complete input.
The capacity proof derives the 256-record limit from slot uniqueness and byte bounds.
It does not assume that callers already supply at most 256 records.
Record lengths are 34–161 bytes, so accepted input contains at most 41,216 bytes.

The formal model represents bytes with natural numbers and checks their domain explicitly.
Rust's `u8` type already supplies that domain restriction.
Lower-level helper theorems establish structural bounds, while the top-level parser also establishes the byte domain.
Production correspondence must preserve that distinction.
The accepted-size result does not claim a bound on network allocation or the cost of rejecting arbitrarily large messages.

### Raw identity and canonical lookup

The current Rust decoder accepts unique records in any slot order.
The Rust encoder emits occupied slots in ascending order.
Consequently, encoding a decoded node does not necessarily reproduce the original accepted bytes.
Rejecting out-of-order records would introduce an additional acceptance restriction without need.

| Representation | Preserved information | Intended use |
| --- | --- | --- |
| Ordered parsed records | Every received byte, including record order. | Verify the received content hash and retain its identity witness. |
| Ordered decoded records | Slot, pointer kind, prefix, target, and original record order. | Reconstruct the original bytes without retaining a redundant header field. |
| Canonical slot projection | One lookup result for each slot from zero through 255. | Select radix traversal edges in slot order. |

The header reconstruction theorem derives the original header from pointer kind and prefix length.
The ordered decoded-record theorem then reconstructs the complete original input.
This equality preserves any hash function without an injectivity assumption.
The canonical lookup theorem separately proves that unique record order does not affect slot lookup.
Every accepted record appears in the 256-slot projection, and missing slots yield empty entries.

The current public Rust `Node` type is an unconstrained vector.
An encoder round-trip theorem for that type must require exactly 256 slots.
The projection theorem establishes this size for the modeled decoded node, not for every arbitrary Rust vector.

### Fixed-width key and closure correspondence

The codec module also maps each parsed record into the graph-closure model.
The mapping retains slot, prefix, pointer kind, target identity, and original record order.
History references retain their occurrence path.
Cold references derive their kind from that path, not from the radix header alone.

The closure model represents keys as natural numbers.
A reversible, little-endian numeric representation connects each valid 32-byte key to that model.
This representation is proof notation, not a change to production keys or wire bytes.
The fixed width matters because different variable-width zero-padded lists can represent the same number.
`import_32_byte_storage_keys_do_not_alias` proves identity preservation under the exact-width and byte-domain premises.

`import_closure_mapping_retains_original_input` reconstructs the received bytes from ordered mapped records.
`import_raw_hash_factors_through_closure_mapping` then preserves any hash function by equality of its input.
Neither theorem assumes cryptographic hash injectivity.
Canonical slot projection remains separate from the ordered hash witness.

Reverse reconstruction applies to records mapped from valid wire input.
Decoding an arbitrary natural number into 32 bytes truncates values outside that representation.
The bridge does not claim reconstruction for unconstrained closure-model keys.
Production correspondence must also enforce the key width because `Blake2b256Hash::from_bytes` does not enforce it.
Raw and legacy physical key encodings remain a separate storage correspondence obligation.

### Controls and evidence

Executable proof examples cover empty nodes, incomplete headers, incomplete prefixes, incomplete targets, and identical duplicate slots.
Positive examples include out-of-order records, shared targets, slot `255`, both pointer kinds, and maximum prefixes.
Additional model-domain controls reject prefix or target values outside the byte range.
Such values cannot occur directly in a Rust `Vec<u8>`.

Invocation `4036189aed994cc0a9a5c23c774f7615` compiled all four import modules and checked their proof terms with the Rocq kernel.
It returned exit `0` with unchanged inputs in `target/verification/state-import/closure-gate.g1DQTU/`.
The run used a 2 GiB memory limit, no swap, and one CPU.
The main proof catalog now includes the codec module and its headline assumption checks.

Invocation `f6ff8ffa25864bab91fa2aff9cb418e0` also checked the fixed-width key and closure correspondence in all four import modules.
It returned exit `0` with unchanged inputs in `target/verification/state-import/closure-gate.ohLkMI/`.
The run used the same 2 GiB, no-swap, one-CPU limits.

Codec validity does not establish pointer existence, path-length limits, cold-value kind compatibility, or graph closure.

### Native codec correspondence regressions

The native tests call `RadixTreeImpl::load_node` against actual in-memory storage.
They do not substitute a second Rust parser for the production decoder.
The valid fixtures independently encode records and construct their expected 256-slot projection.
The tests retain original bytes and use their hash as the cache and storage key.

| Formal obligation | Native test coverage |
| --- | --- |
| Complete records, unique slots, and exact slot projection | Generated unique-slot records in reversed and rotated order. |
| Every header value and derived capacity | All 256 headers and slots, plus 256 maximum-prefix records totaling 41,216 bytes. |
| Valid empty input and complete-record prefixes | Empty nodes and truncations exactly between complete records remain readable. |
| Total rejection of incomplete records | Generated cuts inside a record, including a truncated last record after complete records. |
| Duplicate-slot rejection | Identical and conflicting duplicate records. |
| Original-byte authentication and key width | Altered content keys and widths zero, one, 31, 33, 40, and 64 bytes. |
| Failed loads preserve caller state | Compare storage and both caches, with empty caches and unrelated pending writes. |

Each generated family requests 128 cases.
The negative properties currently express required behavior before the production repair.
An expected-failure result is evidence of the defect, not successful implementation conformance.
All loader flag modes remain part of the target contract.

Invocation `ad82b08dbbd54436aa9089660b8241e1` ran the extended root-load checker in explicit red mode.
The evidence is in `target/verification/state-import/root-load.wNrOuc/`.
The deterministic codec test and the 128-case valid-record property passed.
Each negative property failed on its first generated case, before completing its requested case count.
Truncated and duplicate records caused panics, while an altered key authenticated successfully.
Strict Casper Clippy, formatting, and unchanged-input checks passed.

The checkpoint subset separately reproduced its five expected failures, with its valid nonempty checkpoint control passing.
The combined expected-failure gate returned `0` under a 2 GiB memory limit, no swap, and one CPU.
Neither negative result qualifies all later variants within that property.
The green gate must exercise those variants after repair.

The plan review confirmed that record-boundary truncation is not malformed input when authenticated under its own hash.
The format has no record-count header.
The tests impose no sorted-order, compacted-node, descendant-existence, or Casper-specific path-length restriction on the codec.
Those separate constraints must remain at their correct semantic boundaries.
Cursor encoding and decoded-tree traversal require separate proof obligations, described below.
No production decoder or importer repair has been applied at this point.

## Canonical cursor contract

The [cursor module](../../../../formal/rocq/cost_accounted_rho/theories/StateImportCursor.v) defines the existing singleton and seven-entry wire forms.
A cursor identifies a traversal position.
Its *carrier* is the history-node key in the final entry.
The size and prefix fields contain metadata, not content hashes.

| Cursor form | Fields | Meaning |
| --- | --- | --- |
| Singleton | One 32-byte key with no optional index. | Start a scan from this key. |
| Resume | Root, size word, four prefix words, and carrier. | Resume after the history-node occurrence at the encoded prefix. |

The resume prefix contains zero through 128 bytes.
Its size word starts with the prefix length and ends with 31 zero bytes.
The four prefix words contain the prefix followed by zero padding.
Each field contains exactly 32 bytes, and every optional index is absent.

The decoder first extracts a candidate cursor.
It then validates the candidate and requires its canonical encoding to equal the complete input.
This comparison rejects extra entries, malformed widths, nonzero padding, and unsupported indexes.
The acceptance theorem proves both directions, so every valid canonical encoding remains accepted.
Encoding is injective over accepted cursors, including the carrier field.

This acceptance rule is stricter than the current permissive Rust parser.
The compatibility claim concerns valid exporter output, not malformed inputs that the current parser happens to tolerate.
The formal encoder does not introduce new wire fields.

### Interpretation and limits

A singleton key need not equal the original recovery root.
Different roots can share a terminal singleton and reach it at different times.
The requester must retain each original root's recovery obligation independently of that shared cursor.

Cursor syntax does not establish node existence or completion.
The resume carrier must equal the history node reached by exact traversal from the encoded root and prefix.
An empty prefix therefore requires the carrier to equal the root.
Missing nodes and unmatched compressed prefixes must produce errors, not successful exhaustion.
A repaired leaf-only page can return its unchanged seven-entry input cursor.

### Checked evidence

Invocation `7bb967c71d134807a7b97ddf9e965ebe` compiled all five import modules and checked their proof terms with the Rocq kernel.
It returned exit `0` with unchanged inputs in `target/verification/state-import/closure-gate.pK93fb/`.
The run used a 2 GiB memory limit, no swap, and one CPU.

The theorems cover exact acceptance, round trips, encoding injectivity, field count, absent indexes, fixed-width bytes, and root/carrier preservation.
Executable proof examples cover empty and maximum prefixes, malformed roots, unsupported indexes, oversized prefixes, extra entries, and nonzero padding.
The plan agent found no semantic defect in this contract.
Production tests must still compare actual Rust encoding and decoding with these definitions.

## Authenticated cursor paths

The [traversal module](../../../../formal/rocq/cost_accounted_rho/theories/StateImportTraversal.v) defines a checked history reader and exact path resolution.
The reader requires a valid 32-byte key, a stored row, an equal content hash, and a successful radix parse.
It retains the received bytes as the identity witness.

For each nonempty path, resolution selects the edge at the next slot.
The edge must point to a history node, and its complete compressed prefix must match the remaining path.
Resolution then consumes that slot and prefix before it reads the child.
An empty path succeeds only after the current node has been read successfully.

The path relation describes these same steps without an algorithmic recursion counter.
`import_path_resolution_characterization` proves equivalence between the relation and the executable resolver.
The initial counter equals the path length plus one.
Each descent consumes at least one path byte, so that counter suffices for every valid path.
It is not an arbitrary search-depth restriction.

`import_cursor_path_validation_characterization` binds the encoded carrier to the resolved history node.
An empty resume prefix therefore binds the carrier to the root.
Missing nodes, leaf targets, and partially matched compressed prefixes cannot establish a valid cursor path.
The readable-target theorem connects success to actual stored bytes, their hash, and their parsed records.

### Correspondence with graph closure

`import_checked_wire_and_closure_history_reads_correspond` connects the byte-oriented reader to the graph-closure model's history reader.
It proves equality of the complete read results, including failure and contextual child extraction.
The proof requires valid 32-byte query keys and hash outputs.
It does not assume distinct inputs have distinct hashes.

The projection retains ordered parsed records so its hash witness reconstructs the original bytes.
It decodes numeric lookup keys back to the same 32 bytes used by the wire store.
This projection does not change the production storage representation.
Cold raw and legacy aliases remain separate inputs to this history-read correspondence.

### Writes between node reads

`import_interleaved_history_path` permits compatible writes after each parent read and before the next child read.
The relation does not make a whole path read atomic.
Each extension must preserve every successful read from the preceding view.
`import_compatible_wire_writes_extend_checked_reader` derives that extension from preservation of existing wire-store byte bindings.
Actual concurrent writes must satisfy the guarded storage contracts that establish this premise.

`import_interleaved_path_has_final_view_witness` proves that a successful interleaved traversal has a complete path in its final reader view.
Missing child rows can become available during the traversal.
If the path was already complete initially, `import_concurrent_path_resolution_preserves_selected_target` also proves that its selected target cannot change.
This stronger conclusion requires the initial complete-path premise.

The relation ends when it reads the target node.
`import_resolved_path_survives_writes_after_final_read` covers compatible writes after that read.
No lock through return or publication is implied.
Current unchecked writes do not establish the preservation contract.

### Remaining composition boundaries

Path validity alone does not establish complete root closure or validate every cold leaf.
The operational export proof below connects traversal traces to page selection.
Response-cursor derivation and outstanding-request ownership remain separate composition obligations.
The stack proof retains ancestor frames, absolute prefixes, and selected child indexes to resume later siblings correctly.

Start and resume constructors must remain distinct even when both resolve to the same node.
`Start(root)` emits the root and consumes one history-node budget unit.
`Resume(root, [], root)` skips that already-exported node and starts with its descendants.
This distinction concerns the export page budget, not smart-contract execution cost.

Page-supplied rows also need a separate reader correspondence.
An overlay must not silently replace a conflicting durable binding before the guarded write contract applies.
Cursor decoding must precede path validation because path validation alone does not enforce the wire prefix limit.

### Checked evidence

Invocation `a7e75ac143584212b9b08e34c3481539` compiled all six import modules and checked their proof terms with the Rocq kernel.
It returned exit `0` with unchanged inputs in `target/verification/state-import/closure-gate.HNSGWQ/`.
The run used a 2 GiB memory limit, no swap, and one CPU.
Executable controls cover missing singletons, wrong empty-prefix carriers, partial compressed prefixes, leaf targets, and shared singleton interpretation.

Invocation `c5e7d7c056f04672af4e9c03188732e5` also checked the interleaved-read and post-read preservation theorems.
It returned exit `0` with unchanged inputs in `target/verification/state-import/closure-gate.H4Vvxp/` under the same resource limits.
The plan agent confirmed that the relation permits separate read times rather than assuming an atomic traversal.

## Invariant-derived cursor and path tests

The following matrix specifies required production conformance tests.
It is a test obligation list, not a claim that those tests already exist or pass.
The executable Rocq examples supplement these tests but do not replace Rust integration tests.

| Proven property | Required generated cases | Required runtime check |
| --- | --- | --- |
| Exact canonical cursor acceptance | Both constructors, prefixes of every length from zero through 128, arbitrary bytes, and repeated zero bytes. | Rust encoding and decoding agree with the formal encoding. |
| Complete input equality | Extra or missing fields, short and long words, nonzero padding, and optional indexes. | Reject without writes, progress, or recovery retirement. |
| Injective cursor encoding | Different roots, prefixes, constructor tags, and carriers. | Distinct valid cursors never share an encoding. |
| Fixed-width key identity | Leading and trailing zero bytes, all-zero keys, maximum bytes, and incorrect widths. | Enforce width at ingress and preserve raw key bytes. |
| Exact radix identity | Unique records in arbitrary order, shared targets, and duplicate-slot controls. | Preserve the received hash witness while using canonical slot traversal. |
| Exact compressed-prefix resolution | Empty, full, partial, and mismatched prefixes with missing or leaf targets. | Only complete history-node paths can bind a carrier. |
| Sufficient path bound | Different depths and compressed-prefix lengths within valid cursor limits. | No valid path fails because of an additional recursion counter limit. |
| Start/resume distinction | Equal resolved nodes with different constructor tags. | Count and emit the anchor only for a start cursor. |
| Concurrent path preservation | Compatible inserts between each read and after the final read. | Keep the selected target and exact row bytes. |
| Final-view path witness | Initially absent child rows inserted before their reads. | Successful traversal has authenticated rows for the complete final path. |
| Guarded storage correspondence | Conflicting rows, failed transactions, duplicate pages, and page overlays. | Reject conflicts and preserve all existing accepted bindings. |
| Shared singleton interpretation | Several original roots, simultaneous requests, and late reopening. | Keep each recovery obligation until that root passes complete validation. |

Use property-based tests for generated representations and traversal trees.
Use Loom for modeled read/write schedules and ownership transitions.
Use native storage tests for the actual transaction and alias boundaries because Loom does not execute LMDB's internal scheduler.
The full-page conformance tests must also cover production history budgets of `750` and `1024` and leaf-only continuation pages.

## Ancestor stacks and operational export

The [stack module](../../../../formal/rocq/cost_accounted_rho/theories/StateImportStack.v) reconstructs the current frame and its ancestors from an authenticated cursor path.
A **frame** contains decoded edges, an absolute prefix, and the last selected slot.
The current frame comes first, followed by the nearest ancestor and then more distant ancestors.
The model also records each frame's history key as a proof witness.
This witness does not require a new field in the production `NodeData` structure.

Each ancestor retains exactly the slots after its selected child.
The next-entry scan skips only empty slots and visits occupied slots in increasing order.
Two edges can point to the same history hash at different prefixes.
Their distinct ancestor positions remain necessary because each occurrence has its own remaining traversal.

Fresh reconstruction leaves the current frame unsearched.
A running traversal can subsequently select slots in that frame.
The export proof does not incorrectly require every running frame to retain the fresh reconstruction state.

The stack relation permits compatible writes between successive node reads.
The final-view theorem preserves the exact reconstructed frames after those writes.
A separate theorem covers writes after the last read.
These results require preservation of existing successful read bindings, not a lock across the complete traversal.

### Operational page rules

The [export module](../../../../formal/rocq/cost_accounted_rho/theories/StateImportExport.v) defines successful leaf visits, history visits, and exhausted-frame removal.
Each history visit reads its child, advances the parent slot, and puts a fresh child frame before that parent.
Each leaf visit advances its parent slot without consuming a history budget unit.
Frame removal emits no value.

The page relation stops before another visit when its history budget reaches zero.
The slice relation adds the existing skip counter.
While that counter is positive, the traversal discards values and decreases the counter only for history occurrences.
Skipping a history occurrence does not skip its entire subtree.

The anchor relation distinguishes start from resume.
A start cursor emits the root once, or consumes one skip unit for that root.
A resume cursor does neither, even when its prefix is empty and its carrier equals the root.
`import_export_from_cursor` combines valid cursor fields, coherent initial frames, and those anchor rules.

The following decision order matches the leaf-enabled export path:

```text
If the frame stack is empty, report exhaustion.
Otherwise, if both counters are zero, return the current prefix.
Otherwise, find the next occupied slot in the current frame.
If no slot remains, remove that frame.
If a leaf is selected, advance the slot and apply the skip rule.
If history is selected, require its child read before descent.
Apply the history counter rule and put the child before its parent.
```

The model uses natural-number counters.
Correspondence requires nonnegative production `i32` counters within their representable range.
The cursor entry relation rejects initial counters that are both zero.
It includes a positive skip counter with a zero take counter.
It does not claim that negative production counters satisfy these rules.

### Partial pages and complete traces

A **complete trace** lists all remaining history and leaf occurrences for a fixed reader view.
The step theorem removes exactly the emitted occurrence from that trace.
The finite-run theorem composes this property across any finite sequence of successful steps.
The page and slice theorems then match exact history-occurrence take and skip operations.

These complete-trace conclusions are conditional.
A bounded page can succeed before the exporter needs a missing later child.
For example, a start request with a history budget of one can return the root before it reads any descendant.
Therefore, the operational page relation does not require a complete remaining trace as an acceptance premise.
Root publication still requires the separate complete-state check.

An empty remaining trace also does not establish exhaustion.
An empty root exported with a budget of one leaves a nonempty, unsearched frame at the budget boundary.
The exporter must return its continuation prefix rather than infer completion from the absence of further events.
A required child-read failure permits no successful history step and cannot become positive-budget completion.

The adapter groups leaf values before history values.
It does not preserve cross-kind discovery order.
The correspondence theorems instead preserve each ordered projection, including repeated keys and the complete unexported suffix.
The history-budget bound also holds directly for every successful operational page, without a complete-trace premise.

### Concurrent writes and bounded traversal

The interleaved export relation permits compatible writes between individual traversal steps.
Every successful interleaved run has the same emitted occurrences and final frames in its final reader view.
Compatible writes after the last step preserve that result.
Current unchecked import writes do not establish the required preservation premise.
This relation describes finite prefixes and permits a stop after any step.
The execution module below supplies the separate composition proof for budgeted, anchored page completion under compatible writes.

The proof uses a decreasing measure for each permitted page step.
Let $`s`$ denote the remaining skip count and $`t`$ the remaining take count.
Let $`P(f)`$ denote the number of pending slots in frame $`f`$.
For frame stack $`F`$, define the measure as follows:

```math
M(s,t,F)=257(s+t)+\sum_{f\in F}(P(f)+1).
```

A fresh frame contributes 257 because a radix node has 256 slots.
A history visit consumes one counter unit and advances its parent before it adds the child frame.
A leaf visit decreases the pending-slot count.
Frame removal decreases the frame sum.
The taking-step and skipping-step theorems prove a strict decrease without assuming that the complete reachable graph is finite.

This measure supports termination of bounded successful traversal steps.
It is not a production memory bound or a guarantee that a storage read returns.
The execution module below makes the slice traversal total with explicit read errors in the mathematical model.
Its production correspondence remains an integration obligation.

### Checked evidence and conformance obligations

Invocation `97b0b7751745476e98773c3bb60bbc53` compiled all eight import modules and checked their proof terms with the Rocq kernel.
It returned exit `0` with unchanged inputs in `target/verification/state-import/closure-gate.DyXPnF/`.
The run used a 2 GiB memory limit, no swap, and one CPU.
The focused gate does not establish that the full campaign gate passes.

Executable proof controls cover root skipping, leaf skipping, leaf-only budget tails, empty-prefix resume, and empty-trace continuations.
The shared-subtree controls reconstruct the same child through two actual parent edges and retain the second edge after the first descent.
They use symbolic reader fixtures, not cryptographic or native storage conformance tests.

Invocation `a46f91aabf9b4b7a97e7c5a8baaf2ce6` also checked a complete operational run through both shared-subtree occurrences.
It returned exit `0` with unchanged inputs in `target/verification/state-import/closure-gate.fo5lDo/` under the same resource limits.
The run emits both history occurrences and both leaf occurrences before it exhausts the stack.

| Formal requirement | Required generated or concurrent test |
| --- | --- |
| Ancestors retain later siblings. | Generate shared subtrees at distinct prefixes and compare full export with resumed pages. |
| Start emits the anchor once. | Compare start and empty-prefix resume with identical roots and several skip/take pairs. |
| Skip counts history occurrences. | Put leaves before, within, and after skipped history nodes. |
| A budget boundary precedes another visit. | Place a cold leaf immediately after the final selected history node, including budgets `750` and `1024`. |
| Empty trace differs from exhausted stack. | Export an empty root with budget one and inspect the actual continuation. |
| Missing child differs from exhaustion. | Remove the next required child and require an explicit error without completion. |
| Mixed reads preserve exact bindings. | Use Loom schedules that insert compatible data between reads and after the final read. |
| Each successful step decreases the measure. | Generate stacks and counters, then check every actual traversal transition. |
| Page projections preserve occurrences. | Compare leaf and history lists separately, with duplicate keys and shared subtrees. |

These tests remain production conformance obligations until their native results exist.
Response-cursor derivation, cold-value codec checks, page-overlay conflict handling, and publication ownership must still compose with this traversal layer.

### Output and failure boundaries

`import_export_resume_prefix` describes the internal traversal prefix, not the final wire cursor.
The singleton completion form and a leaf-only terminal response still require explicit adapter correspondence.
Canonical input alone does not guarantee an encodable output prefix.
For example, a 127-byte compressed edge followed by another history pointer can produce a 129-byte absolute prefix.
The output contract must establish its path bound or reject the unencodable result.

A positive skip counter with zero take can also produce no entries while leaving a nonempty frame stack.
For example, a start request with skip one and take zero consumes the root skip and stops before visiting its children.
Therefore, continuation metadata cannot assume that each budget boundary emits a new history entry.

The strict cursor wrapper describes an accepted export witness with a readable initial root.
Current `sequential_export` returns an empty result when its initial root is absent.
That response can represent unavailable peer state, but it cannot establish verified import completion.
The wrapper does not describe every raw exporter return value or authorize a change to the missing-root wire behavior.

The export module's missing-child lemmas rule out successful steps and positive-budget completion.
The execution module below adds a returned-error representation for the slice model.
The production adapter must preserve this distinction through the actual error and response types.

## Total slice evaluation and concurrent completion

The [execution module](../../../../formal/rocq/cost_accounted_rho/theories/StateImportExecution.v) connects the operational rules to an executable slice function.
One step returns exhaustion, a successful transition, or a read failure with the requested key.
The step characterization proves equivalence between a successful returned transition and the operational step relation.
Exhaustion requires an empty stack.
A read failure requires a selected history edge whose reader returned `None`.

For a checked reader, `None` can mean a missing row, a hash mismatch, or invalid encoding.
The model does not infer the precise underlying cause from this result alone.
Initial coherent frames retain their read bindings after each successful transition.
Cursor stack construction supplies this coherence when it uses the checked history reader.

### Derived recursion allowance

The evaluator uses a recursion allowance to express a terminating function in Rocq.
The allowance equals the previous section's measure plus one.
It is not an adjustable cutoff, a contract fuel balance, or a phlogiston charge.
`import_evaluation_never_exhausts_its_derived_fuel` proves that this allowance cannot expire during the modeled slice evaluation.

The exported bounded evaluator therefore returns success or a read failure for every mathematical input in its domain.
It does not require an acyclic history graph or complete future state to produce a result.
The input reader is a total mathematical function.
This guarantee does not prove that a production storage operation returns or bound its latency, allocation size, or CPU cost.

The success result contains selected entries and the exact residual frame stack.
`import_bounded_slice_success_characterization` proves equivalence between that result and the operational slice relation.
The proof establishes both soundness and completeness.
It also establishes deterministic successful slices and preserves the history budget.
The complete-trace premise remains necessary only for the separate full-occurrence decomposition theorem.

### Writes at real read boundaries

The interleaved slice relation permits compatible writes before the first child read and between later steps.
It also permits writes before terminal completion.
This explicit write transition closes a model coverage gap that an after-step-only relation left open.
An initially missing child can become readable after stack construction but before the first descent.

The final-view theorem uses the same initial counters and preserves the selected entries and exact residual stack of every successful interleaved slice.
The resulting page equals the fixed-view evaluator's result in that final reader view.
Compatible writes after the final step preserve the same page.
These claims do not require a page-wide atomic read or serialize peer transfers.

The cursor composition includes interleaved ancestor-stack construction and anchored export.
It preserves the start/resume distinction and the history budget in the final view.
Its entry view is the invocation view, not an assumed successful root-read view.
An explicit leading extension permits an initially absent root to arrive before the first lookup.
The initial cursor still needs canonical fields, a readable path, and carrier agreement.
Physical storage writes must establish the existing-binding preservation contract used by these reader extensions.

External write transitions do not consume the local evaluator's recursion allowance.
The traversal measure bounds local computation steps, not arbitrary scheduler delay or the number of external writes.
Successful mixed-view execution implies the same final-view result.
The converse does not hold for every schedule because an earlier failed read can precede a later insertion.

### Error observation and retry

Every returned slice error has a reachable failed-step witness.
The witness names the selected key and the reader view that returned `None`.
Failure propagation returns an error rather than a successful prefix page.

An error does not imply that the key remains absent forever.
A compatible write can insert that key after the failed lookup.
The regression example proves that the earlier read failure and the later successful read can both be correct.
Therefore, final-view preservation applies to successful page results, not to continued absence of a failed key.
Retry ownership must use the observed failure until a later attempt establishes successful recovery.

### Checked evidence and test obligations

Invocation `ccfd7a95da8842fc9b78fa22012c8821` compiled all nine import modules and checked their proof terms with the Rocq kernel.
It returned exit `0` with unchanged inputs in `target/verification/state-import/closure-gate.ecGifV/`.
The run used a 2 GiB memory limit, no swap, and one CPU.
The mathematical fixtures use 32-byte symbolic keys and do not establish cryptographic storage conformance.

Invocation `3f5c997bb5b54939ac1b2606e26d8c5a` also checked the invocation-entry extension and its concrete root-arrival example.
It returned exit `0` with unchanged inputs in `target/verification/state-import/closure-gate.feLzGA/` under the same resource limits.
The example starts with no readable root and completes only after a compatible insertion makes that root and its descendants available.

| Execution property | Required production correspondence test |
| --- | --- |
| Explicit read failure | Remove or corrupt the next required child and require an error instead of completion. |
| No read beyond a zero budget | Leave an unavailable child beyond the selected page and confirm that the current page still succeeds. |
| Derived allowance is sufficient | Generate stacks and counters, then compare evaluation with the untruncated operational reference. |
| Exact successful result | Compare entries and residual frames, not only counts or a completion flag. |
| Writes before the first read | Insert a missing child after stack construction and before the first descent. |
| Writes before the first root lookup | Start with an absent root, insert it before the lookup, and compare the successful final-view result. |
| Writes between later reads | Insert compatible descendants and confirm equality with the final-view reference result. |
| Failure belongs to its observation view | Fail a read, insert the row, and verify that the original attempt fails while retry can succeed. |
| Anchored concurrent budget | Vary start/resume, skip/take, shared subtrees, and insertion schedules without changing the accepted occurrence budget. |

Final wire-cursor construction, cold-value closure, page-overlay validation, and publication ownership remain separate composition requirements.
The total evaluator does not establish these requirements or claim that the current production importer is repaired.

## Wire references, continuation state, and payload coverage

The [wire refinement](../../../../formal/rocq/cost_accounted_rho/theories/StateImportWire.v) separates traversal references from the cursor that follows a page.
`ImportWireReferences` contains ordered history and leaf occurrences before payload deduplication.
It does not represent the final `StoreItemsMessage` row lists.
Production removes duplicate keys before it loads those rows.
The proof preserves that distinction and does not require a new wire order or duplicate payload rows.

### Cursor construction

The proposed adapter derives continuation metadata independently of the payload vector.
It never removes a leaf to make room for metadata.
It takes a budget cursor's carrier from the captured frame key.
It does not hash a reconstructed node because accepted raw record order can differ from canonical slot order.

| Traversal outcome | Cursor rule | Compatibility requirement |
| --- | --- | --- |
| Nonempty residual stack | Encode the original root, current prefix, and captured carrier in seven entries. | Preserve existing root-prefixed cursors. |
| Exhausted stack with history occurrences | Encode the final history key as a singleton. | Preserve shared singleton tails. |
| Exhausted stack without history occurrences | Return the exact input cursor. | Preserve every leaf in a leaf-only terminal page. |
| Missing root at the initial raw lookup | Return empty rows with the input cursor. | Classify unavailability separately from validated completion. |
| Storage failure, rejected bytes, or unencodable prefix | Return an error. | Do not substitute empty success or truncate a prefix. |

The encoder rejects a prefix longer than 128 bytes.
Canonical input fields do not prove that an arbitrary stored graph will produce an encodable output prefix.
The generic numeric history-key model also needs its 32-byte representation premise for exact raw-key identity.
The exact-key theorem states that premise explicitly.

### Exact resumption and progress

The frame-spine invariant records the exact ancestor chain, selected slots, and occurrence prefixes.
Every operational leaf visit, history descent, and frame removal preserves that invariant.
Shared hashes do not combine distinct occurrences.
An initial cursor starts at a fresh frame, and a successful page boundary retains a fresh top frame.
These facts prove that the next cursor reconstructs the exact residual stack.

The progress proof orders traversal positions without assumptions about hash injectivity or complete-graph finiteness.
A position lists frame slot codes from the root toward the current frame.
A fresh frame has code zero, and a selected slot has code one greater than its index.
The order places an exhausted ancestor after all its descendants.
Every operational step strictly advances this order.

A positive skip count or history budget forces a nonempty resume slice to advance.
If its next cursor equaled its input, deterministic reconstruction would produce the same initial and final stacks.
Strict position advancement excludes that result.
A nonterminal start cursor changes to the distinct resume form, including zero-entry budget boundaries.

The interleaved theorem preserves both exact reconstruction and cursor progress in the final reader view.
It uses the same compatible-write contract as the execution model.
It does not assume a page-wide atomic read or serialize independent transfers.
It does not bound external writes, scheduler delay, or whole-transfer completion time.

### Root observation and payload loading

The root probe distinguishes raw absence, storage failure, and rejection of captured bytes.
Its concrete check function binds the captured bytes to the requested 32-byte root and the existing authenticated radix decoder.
The availability theorem proves the raw lookup, exact hash equality, and successful parse for the same captured bytes.
It does not replace this observation with a later root lookup.

An empty wire response still cannot distinguish unavailable state from a valid empty terminal page.
Therefore, neither an empty response nor cursor equality alone establishes complete imported state.
Root publication still requires validated closure.

Payload selection requires unique keys with exactly the same key set as the occurrence list.
Successful row loading preserves every selected key and its observed bytes.
A missing row or storage failure prevents success rather than silently omitting that row.
Different selected-key orders produce the same required key set.
Two occurrences of one key require one payload row, not two rows.

The interleaved row loader permits compatible writes before, between, and after its successful reads.
Every successful mixed-view load has the same rows and exact key coverage in the final reader view.
The extension contract preserves previously observed bytes but permits absent rows to arrive before their first lookup.
These results do not prove cold-value codec conformance or transactional import writes.
The output adapter does not establish publication validity by itself.

### Checked evidence and derived regression requirements

Invocation `b6000d20cfe0438eb8d9b05581d1494e` compiled all ten import modules and checked their proof terms with the Rocq kernel.
It returned exit `0` with unchanged inputs in `target/verification/state-import/closure-gate.jIbaKn/`.
The run used a 2 GiB memory limit, no swap, and one CPU.
The plan agent reviewed the adapter and frame-spine claims before production changes.
The agent also proposed the key-independent progress order used by the checked proof.

Invocation `fcf983894e384fefa22158a24c300e76` also checked mixed-view payload loading and its concrete first-read arrival example.
It returned exit `0` with unchanged inputs in `target/verification/state-import/closure-gate.H0Wddk/` under the same resource limits.

| Formal property | Required native, property, or concurrency regression |
| --- | --- |
| Exact leaf occurrence retention | Generate leaf-only pages, duplicate references, and exact history-budget boundaries, including page sizes 750 and 1024. |
| Metadata independent of payloads | Check empty output with a nonempty residual stack, including `skip=1, take=0`. |
| Exact stack reconstruction | Compare the resumed traversal with the original residual traversal through shared children, compressed prefixes, and ancestor removal. |
| Strict nonterminal cursor progress | Generate valid start/resume cursors and positive counters, then reject unchanged cursors before traversal exhaustion. |
| Zero-counter exclusion | Retain a control that permits unchanged `Resume(0,0)` and verify that the public request validator rejects that pair. |
| Raw carrier identity | Use valid raw history records whose order differs from canonical slot order, then verify exact carrier bytes. |
| Unencodable prefix rejection | Exercise the 128-byte boundary and require a typed error above it, without truncation or terminal substitution. |
| Captured root authentication | Inject missing roots, storage failures, wrong hashes, and invalid codecs at the initial lookup. |
| Exact deduplicated payload coverage | Permute key selection, repeat occurrences, and require all unique rows with no missing or extra keys. |
| Missing rows cannot disappear | Remove a selected history or cold row and require an error rather than filtered success. |
| Interleaved cursor correspondence | Insert compatible rows between traversal reads and require the same final-view cursor and residual stack. |
| Interleaved payload coverage | Insert a missing payload before its lookup and require the same successful rows and exact key set in the final view. |

These requirements do not claim that the current Rust exporter or importer already satisfies the model.
Production repair, native correspondence, page-overlay validation, cold-value closure, and publication composition remain required.

### Handler freeze provenance

The temporary `running.rs` freeze came from the dispatcher check's unchanged-input boundary.
Its [input manifest](../../../../target/verification/recovery-dispatcher/run.f87Z9D/inputs.sha256) includes that file.
The [work log](../../../work-logs/task-pr216-admission-backpressure-2026-09-06.md) ties the boundary to the formerly active dispatcher check.
The plan agent found no direct user restriction specific to that file in the inspected records.
The task record must distinguish this evidence freeze from permanent scope restrictions before implementation.

The approved repair still requires regression evidence and alignment with `dev`.
Error propagation through the existing request handler does not authorize new wire fields, storage namespaces, votes, fork choice, or finalization thresholds.

## Cold-value validation and typed consumption

The existing page validator does not establish that typed readers can consume imported cold values.
It hashes the serialized leaf wrapper, excluding the `PersistedData` enum discriminant.
Therefore, a matching leaf hash does not authenticate the envelope kind.
It also does not establish that the inner collection or its nested items can decode.

The actual [cold store](../../../../rspace++/src/rspace/history/cold_store.rs) defines `Joins`, `Data`, and `Continuations` envelopes.
Each envelope contains the same `Vec<u8>` field shape.
The [history writer](../../../../rspace++/src/rspace/history/history_repository_impl.rs) hashes that field's serialized wrapper and stores the complete enum separately.
The [reader](../../../../rspace++/src/rspace/history/instances/rspace_history_reader_impl.rs) checks the expected kind through pattern matching.
Wrong kinds reach panic branches, and malformed inner values reach panic-based serializer wrappers.

### New regression evidence

The tests call the existing page validator directly, using pages from the real exporter and isolated stores.
They therefore expose defects beyond the runtime requester's missing validation call.
The expected result is rejection before storage changes.

| Regression | Evidence from unchanged production |
| --- | --- |
| Wrong kind with unchanged leaf hash | A `Data` envelope at a `Joins` trie occurrence passes the existing validator. |
| Malformed inner collection | A correct kind and hash with an incomplete `Vec<Vec<u8>>` frame passes the existing validator. |
| Malformed nested join | A valid outer collection containing an invalid `Vec<String>` item passes the existing validator. |
| Existing trailing-byte acceptance | The validator and actual readers accept trailing bytes at envelope, collection, and nested-item levels. |

Invocation `c50a38972b3944dfa69e841067a68401` ran the focused suite through [the cold regression gate](../../../../scripts/check-state-import-cold-regressions.sh).
The evidence is in `target/verification/state-import/cold-regressions.dK795z/`.
All three new rejection regressions failed as expected against unchanged production.
The five existing runtime defects also failed, while three valid/path controls passed.
Native execution returned exit `101` after 0.13 seconds, following compilation.

Strict Casper Clippy, file formatting, and unchanged-input checks returned exit `0`.
The gate used a 5 GiB memory limit, no swap, one build job, and one CPU.
These tests demonstrate the validator boundary with String-based fixtures.
They do not establish historical CI causation or full multi-node behavior.

### Model-derived property regressions

The runtime suite now also generates cases from three cold-state invariants.
Each property configures 32 random cases and at most 32 shrinking iterations.
Proptest also replays persisted failure seeds.
These are bounded tests, not exhaustive enumeration of all byte sequences.

| Property | Generated inputs | Pre-repair result |
| --- | --- | --- |
| Trailing bytes preserve typed values | One to four channel strings and independent zero-to-sixteen-byte tails at envelope, collection, and nested-item levels. | The actual source and destination readers agree. |
| Hash-valid nested truncation is rejected | Zero to 32 present string bytes and one to 32 missing bytes, within a correctly framed outer collection. | The validator incorrectly accepts the value. |
| Expected kind comes from the trie | Valid join payloads with either a `Data` or `Continuations` envelope and the unchanged leaf hash. | The validator incorrectly accepts the substituted kind. |

Invocation `fd227197d63b444e9973f2aba57b24c8` ran the expanded suite under a 5 GiB memory limit, no swap, and one CPU.
The evidence is in `target/verification/state-import/cold-regressions.ZUzswx/`.
The suite reported ten expected failures and four passing controls, with native exit `101` after 2.37 seconds of execution.
Compilation took 33.44 seconds.
Strict Casper Clippy, formatting, and unchanged-input checks returned exit `0`.
The complete gate correctly returned failure because the production defects remain unrepaired.

The minimized truncation case contains no string bytes but declares one missing byte.
The minimized kind-substitution case contains the channel string `a` in a `Data` envelope at a `Joins` occurrence.
The [persisted seed file](../../../../casper/proptest-regressions/rust/engine/runtime_state_import_tests.txt) preserves repeatable generation for future runs.
The larger native conformance task must also instantiate the actual node types and cover all three expected trie kinds.

### Compatibility and implementation boundary

Pinned bincode 1.3.3 uses fixed-width integer encoding and permits trailing bytes through its free `deserialize` function.
The repair must preserve that acceptance at all existing decoding layers.
It must not substitute re-encoding equality, reject all trailing bytes, or change the leaf hash domain.
The cold proof must model length framing and nested decoding separately from envelope-kind equality.

The plan agent identified `HistoryRepositoryInstances<C,P,A,K>` as the type-preserving construction boundary.
That constructor can select a stateless, monomorphized validator before the importer becomes an erased trait object.
Repository clones and existing startup, horizon, and runtime paths can retain the same importer handle.
This design does not require new requester generics, locks, tasks, global registration, or wire fields.

The validator must derive each expected kind from the trie occurrence.
It must decode the inner collection and each nested item through fallible functions with the existing configuration.
The concrete node instantiation must select the node's actual Rholang types.
The trait must not provide an unchecked default validator.
Every `(hash, expected kind)` obligation remains distinct even when payload rows share a hash.

Equal inner bytes across envelope kinds do not require a cryptographic collision.
However, normal checkpoint changes delete empty collections rather than insert their equal empty encodings.
A source-derived nonempty generic-library example remains separate from reachable Casper behavior and has not been executed in this record.
This observation does not authorize new hash domains, kind-qualified storage keys, or a policy for cross-kind sharing.

### Checked cold codec and consuming traversal

The [cold proof](../../../../formal/rocq/cost_accounted_rho/theories/StateImportCold.v) separates envelope framing, collection framing, nested decoding, and storage publication.
Its input lists represent bytes when every element is below 256.
The envelope uses a four-byte enum tag and an eight-byte vector length, both little-endian.
Each inner collection also uses eight-byte lengths.

The framing proof derives the payload-length bound from valid input bytes.
It also reconstructs the exact original payload-length header.
The resulting hash input excludes the enum tag and envelope trailing bytes, matching the existing writer.
It retains all bytes inside the stored payload, including accepted collection and nested-item trailing bytes.
The repair must not hash reserialized typed values.

The nested decoder is an explicit function argument, not a proved implementation of Serde.
The native bridge must instantiate that function with the concrete `Datum`, `WaitingContinuation`, and join decoders.
Rust's byte and host-size types must satisfy the model's byte-domain and representable-length premises.
The framing theorem does not establish arbitrary host allocation bounds or safe recursion depth.

The consuming scan starts with no checked references and the requested root as its only frontier entry.
It checks each history reference's hash and children.
It checks each cold reference's hash, expected kind, collection, and nested items.
Only a successful check can move that reference into the checked set.
The checked-set key includes the expected kind, not only the hash.

Compatible writes can occur between scan steps.
Each write preserves existing bindings, and the proof preserves the decoder result for every previously checked cold reference.
An empty frontier therefore establishes readable state for every reachable reference.
The theorem derives that property from the scan rather than assuming it in the initial state.

| Formal result | Required implementation or test correspondence |
| --- | --- |
| Exact payload-frame hash | Hash the stored wrapper bytes, excluding the enum tag. Test all three envelope kinds and trailing-byte levels. |
| Authentication followed by guarded insertion | Preserve both key aliases. Make the inserted leaf pass the structural and typed checks. |
| Completed consuming scan | Require successful nested decoding for every reachable cold reference before publication. |
| Failed nested decoder prevents completion | Retain incomplete recovery when a hash-valid nested value is malformed. |
| Arbitrary guarded batches preserve all consumable roots | Test shared values across independent transfers, successful writes, failed writes, and retries. |
| Split reads preserve typed results | Test raw and legacy lookups with compatible writes between the two reads. |

The `SkipTypedConsumption` control uses one history node and one correctly tagged cold value.
The cold value contains a valid outer collection with an invalid nested item.
The unsafe structural scan reaches an empty frontier, but the safe consumer check rejects the value.
A valid nested-item control confirms that the fixture decoder does not reject every input.
These symbolic hashes isolate the validation rule and do not replace the native cryptographic regressions above.

Invocation `7108f8b6b1994eabab6f271b1d8b1667` compiled all eleven import modules and checked their proof terms with the Rocq kernel.
The gate returned exit `0` with unchanged inputs in `target/verification/state-import/closure-gate.7vmG7J/`.
It used a 2 GiB memory limit, no swap, and one CPU.
The aggregate proof registration now includes this module and its main assumption checks.
The complete aggregate gate has not run against these additions.

The remaining composition work must connect accepted page overlays, physical storage operations, the consuming scan, and publication ownership.
Physical byte bindings require their own correspondence to the decoded `ImportLeaf` values used in the storage proof.
An existing malformed alias must produce a validation error, not appear as an absent alias.
Existing encoded values must remain unchanged even when another encoding decodes to the same leaf.
Production implementation and native property-based and Loom conformance remain required.
The current proof establishes safety for completed finite scans, not unconditional termination or whole-node liveness.

## Encoded aliases and concurrent commits

The storage proof must distinguish absent bytes, malformed present bytes, and decoded values.
Treating a decoder failure as absence would permit an import to overwrite existing malformed bytes without an explicit corruption-repair policy.
This repair does not define such a policy.
It must reject that import and retain recovery responsibility.

The existing reader checks the raw hash key first.
It checks the serialized hash key only when the raw key is absent.
Malformed raw bytes therefore prevent fallback, even when the serialized key contains a valid value.
Conversely, a valid raw value can remain readable despite a malformed serialized alias.

Strict import validation checks both aliases.
This requirement is stronger than the existing reader contract, not an equivalent description of that contract.
Both present aliases must decode to the same envelope kind and exact inner payload bytes.
Equivalent outer encodings can contain different accepted trailing bytes.
Validation must preserve those existing encodings rather than replace them with a canonical reserialization.

### Native controls and rejection regressions

The expanded runtime suite contains 18 tests.
Invocation `437dbe82b9ea4895b69dadca363b7aa6` produced 12 expected pre-repair failures and six passing controls.
Its evidence is in `target/verification/state-import/cold-regressions.9NLGuR/`.
The native test process returned `101` after 3.07 seconds, excluding compilation.
Strict Casper Clippy, formatting, and input-hash checks passed.
These results do not represent 12 independent root causes or identify a historical CI failure.

| Added test | Observed result before repair | Contract established or required |
| --- | --- | --- |
| Equivalent envelope insertion | Fails the byte-preservation assertion. | Keep existing encoded bytes, including accepted trailing bytes. |
| Malformed raw alias with valid legacy data | Fails the import-rejection assertion. | Distinguish malformed presence from absence and preserve incomplete recovery. |
| Missing raw alias with compatible legacy data | Passes. | Add the raw value without changing legacy bytes or typed reads. |
| Valid raw alias with malformed legacy data | Passes the existing reader control. | Describe the reader accurately, without weakening the stricter import contract. |

### Required commit boundary

Preflight validation can race with a checkpoint or another import.
An empty raw alias and an empty serialized alias can each pass separate checks for incompatible incoming values.
Checking only the destination key at commit time does not detect that conflict.

The shared store already supports atomic multi-key compare-and-swap within one LMDB environment.
The cold-store transaction must compare both observed alias values before it commits a missing destination value.
A changed observation must cause rejection or renewed validation before the requester records progress.
The update must apply to the current store, not a saved preflight store that omits concurrent writes to other keys.

Both production cold-write paths must satisfy this rule.
The [importer](../../../../rspace++/src/rspace/state/instances/rspace_importer_store.rs) serves startup, horizon, and runtime imports.
The [checkpoint writer](../../../../rspace++/src/rspace/history/history_repository_impl.rs) serves normal, replay, and DAG-merge checkpoints.
Updating only the importer would leave the concurrent-writer proof premise unsatisfied.

This boundary does not require a transaction across separate history and cold environments.
It also does not establish zero physical database rewrites.
The current compare-and-swap implementation can write an unchanged replacement value.
Byte preservation and write suppression are different properties.

### Checked byte-store correspondence

[StateImportEncodedCold.v](../../../../formal/rocq/cost_accounted_rho/theories/StateImportEncodedCold.v) contains 840 lines and 50 checked proof terms.
Its byte store retains raw and serialized aliases separately.
Its decoder represents absence separately from malformed present bytes.
Guarded insertion preserves every existing byte binding and inserts only an absent destination value.

The normalized logical view exposes a leaf only after strict resolution succeeds.
Its logical raw slot does not represent the physical raw alias.
This view connects byte-level operations to the earlier consumer and frontier proofs.
The actual split-reader proof uses physical byte stores directly.

The observation proof permits the two initial alias reads to occur at different times.
It requires their exact values to match both aliases in one commit state.
Successful attempts then refine guarded insertion against that state.
Failed attempts preserve the entire commit state, not merely the earlier observed state.
Arbitrary finite sequences preserve existing bytes, resolved values, and concurrent unrelated keys.

| Checked theorem | Required property or concurrency test |
| --- | --- |
| Malformed presence remains distinct from absence | Generate absent, malformed, and valid aliases in both physical locations. |
| Guarded insertion preserves existing bytes | Generate equivalent outer encodings with distinct accepted trailing bytes. |
| Both observations transfer the commit guard | Race two preflight checks against opposite-alias commits. Reject changed observations. |
| Failed attempts preserve the commit store | Inject failures after staged mutations and verify that no staged prefix becomes visible. |
| Unrelated keys retain concurrent updates | Insert another key between preflight and commit, then check both successful and failed attempts. |
| Observed histories refine guarded runs | Generate retries, aliases, keys, inputs, and storage results across arbitrary finite histories. |
| Split readers retain their decoded leaf | Interleave compatible writes between raw and serialized alias lookups. |
| Authenticated commits establish consuming reads | Instantiate every concrete nested decoder and check the normal post-import reader. |

The strengthened stale-preflight control proves that both complete insert guards accept the initial empty store.
Unchecked destination-only writes then create conflicting aliases.
The checked second commit rejects the changed observation without altering the first value.

Invocation `6a29e8a5d0984e8081fc6fc2107a9402` compiled all twelve import modules and checked their proof terms with the Rocq kernel.
The gate returned `0` with unchanged inputs in `target/verification/state-import/closure-gate.oJe0qU/`.
It used a 2 GiB memory limit, no swap, and one CPU.
The prior run stopped in the equivalent-envelope example's definitional reduction.
The replacement proof uses `vm_compute` for the same statement and passes kernel checking.

This result establishes cold-byte preservation, not automatic authentication of every newly inserted value.
New readable-state establishment requires the separate authentication and consuming-scan premises.
The initial encoded root-preservation theorem keeps the history map fixed.
The extension below connects concurrent history writes.
Accepted page overlays and publication ownership still require their final composition.
The production import and checkpoint writers do not yet satisfy the modeled commit rule.

### Batches and interleaved history writes

Batch staging checks each row against the previously staged rows.
It accepts compatible repeated keys without replacing the first encoded value.
It rejects conflicting repeated keys before publication.
A definite failed transaction preserves the entire previous store, including every unrelated binding.

The observation theorem checks both original aliases for every touched key.
It does not require unique keys or a consistent preflight snapshot.
Matching all observations transfers successful preflight staging to the current commit state.
The implementation must place all corresponding comparisons and writes in one cold-store transaction.
An unknown commit outcome is not equivalent to definite rollback.
Cancellation and process failure require the publication and recovery lifecycle rules.

The joint-storage relation permits arbitrary interleavings of cold batches and guarded history insertions.
Its preservation theorem covers both storage domains and any finite list of previously readable roots.
The relation refines the consuming scan's concurrent-write action.
It therefore preserves completed read checks while other transfers or checkpoints extend the store.

Invocation `6fe49720c48c4cdda17f874308bf7ce8` checked the batch and joint-storage extensions with all twelve modules and kernel checking.
The gate returned `0` with unchanged inputs in `target/verification/state-import/closure-gate.Jg9tPI/`.
It used a 2 GiB memory limit, no swap, and one CPU.

### Production transaction concurrency checks

Four new [Loom tests](../../../../formal/loom/cost_accounting/tests/loom_production_sparse_transaction.rs) import the production sparse-transaction implementation.
They test opposite-alias commits after stale preflight, unrelated concurrent writes, and a split reader during compatible raw insertion.
A negative control omits the opposite-alias guard and must expose conflicting bindings.
All four passed in invocation `0a3f855c153741548a5426618d593392`.
The evidence is in `target/verification/state-import/alias-loom.zHbpVA/`.

Each test permits three threads, including the test thread, and at most 5,000 branches per execution.
No preemption, permutation, duration, or checkpoint limit truncates exploration.
The tests use opaque scalar values under Loom locks.
They do not prove Serde decoding, LMDB durability, or importer integration.

The native property uses the actual in-memory store manager and shared atomic mutation API.
It generates both initial alias values, zero to eight intervening changes, either destination alias, an incoming value, and an unrelated write.
Generated values contain zero to 32 arbitrary bytes, including malformed encodings.
The property checks the transaction primitive's exact comparisons, not content authentication.
It checks complete rollback after a changed observation and exact preservation of the current store's unrelated values.

Invocation `8fa1520a370c461d9719ee89ebd74c19` ran 1,024 generated cases and passed.
Strict Casper lint and strict lint for the sparse-transaction Loom target also passed.
The input hashes remained unchanged in `target/verification/state-import/alias-property.iVyoJv/`.
The scope limited memory to 5 GiB, disabled swap, and used one CPU.
These generated cases supplement the universal proof rather than establish an exhaustive test of all possible byte sequences.

### Final composition review

The plan review found no false theorem or hidden consistent-snapshot premise in the byte, batch, or mixed-history additions.
Its final requested corollary connects an observed batch directly to the joint storage relation.
Invocation `399095b3bc5041818b1a6d87cd1fb34b` checked that addition with all twelve modules and kernel checking.
The gate returned `0` with unchanged inputs in `target/verification/state-import/closure-gate.5zf40V/`.
It used a 2 GiB memory limit, no swap, and one CPU.

Repeated rows must reuse one original observation per alias and key during batch preparation.
Both production writers still require that native construction and its repeated-key and rollback tests.
The history part preserves decoded bindings, while physical history-byte preservation uses the separate codec and writer correspondence.
The remaining page-overlay and publication composition must preserve these distinctions.

## Original-root occurrence context

An **occurrence** is one reference at a particular path below a required state root.
Different occurrences can share a stored key.
A **wire root** identifies the subtree used by the current transfer request.
An **origin prefix** identifies that subtree's occurrence below the original required root.

The existing singleton continuation can change the wire root to a shared subtree.
The subtree's local paths then omit the original prefix.
This is valid traversal behavior, but the local path alone cannot determine the stored value type.
The original path selects data, continuations, or joins through namespace byte `0`, `1`, or `2`, respectively.

The stored envelope uses a different tag order: joins, data, then continuations.
The cold-value hash covers the serialized leaf payload, not that envelope tag.
Changing the envelope tag can therefore preserve the hash without a cryptographic collision.
The validator must derive the required type from the authenticated occurrence, not from the supplied envelope.

### Real-exporter regression

The shared-cursor fixture creates two distinct roots with a common subtree through normal channel hashing and history insertion.
Both actual export responses end with the same singleton cursor.
The regression exports that subtree with a budget greater than its history size.
Its history anchor avoids the separate leaf-only adapter defect.

The test also requests prefixes from the actual sequential exporter.
For both original roots, it finds the subtree occurrence and reconstructs each full leaf path as `origin_prefix ++ local_prefix`.
Each reconstructed path identifies the same leaf under the original root and requires joins.
At least one local prefix, without its origin, does not select joins.
The fixture retains separate origin records for the two roots despite their identical wire cursor.

The positive control accepts the original joins response.
The rejection test replaces each joins envelope with a data envelope and verifies that every payload hash remains unchanged.
The current validator incorrectly accepts the modified response.
This is a demonstrated validation gap, not evidence of a historical CI failure.

Invocation `72d1a38ea2664e6cb86e96f97ece1b2c` ran both tests with unchanged source inputs.
The positive control passed, and the rejection regression failed at the intended assertion.
The evidence is in `target/verification/state-import/context-native.MCDkMu/`.
The scope limited memory to 5 GiB, disabled swap, and used one CPU.
Native execution took 1.77 seconds after compilation.

The current validator has no original-occurrence input.
The repaired validation path must obtain that evidence from the retained request or checked traversal from the original root.
Changing only the test's expected envelope does not repair that missing connection.

### Occurrence correspondence proof

The [occurrence module](../../../../formal/rocq/cost_accounted_rho/theories/StateImportOccurrence.v) connects the existing operational traversal to contextual references.
Annotations retain the original-root path without changing exported entries, history budgets, or residual traversal stacks.
Every modeled unanchored page admits annotations, including pages whose cold values have not yet passed type checks.
The separate occurrence checker rejects invalid types rather than silently changing the export trace.

| Proven property | Meaning | Native conformance obligation |
| --- | --- | --- |
| Exact annotation erasure | Removing annotations gives the original page with the same budget and final stack. | Compare annotated traversal with current exporter output and cursor behavior. |
| Authenticated frame paths | Every active frame has a stored binding and a history path from the wire root. | Validate original-root and continuation paths through actual stored history bytes. |
| Singleton origin preservation | A history export step establishes the next subtree's path below the original root. | Retain each origin when several roots or occurrences share a cursor. |
| Contextual reference correspondence | Each annotation maps to the existing closure model's edge reference. | Derive required cold kinds from full paths before any key deduplication. |
| Interleaved final-view correspondence | Compatible history writes preserve page annotations and their authenticated paths. | Exercise independent transfers and checkpoint writes without a global traversal lock. |
| Complete occurrence checking | Acceptance checks every occurrence, including repeated keys with different required kinds. | Reject an incompatible envelope even when another occurrence accepts its key. |
| Typed payload consumption | Accepted cold occurrences have the required hash, envelope kind, and decodable nested values. | Use the concrete typed Serde decoders and retain accepted trailing-byte behavior. |
| Explicit failed reads | Missing rows and storage failures cannot become accepted occurrences. | Inject both outcomes and preserve the unresolved recovery obligation. |

The checker uses fallible payload lookups and distinguishes missing records, storage failures, unknown kinds, and invalid cold values.
A proof shows that one payload cannot satisfy two different required kinds in the same accepted page.
That result does not require a collision-resistance assumption.
It follows from exact payload selection and the envelope's single decoded kind.

The first checked version contained 382 lines and 19 proof terms.
Invocation `252efbaf49ef46c1b6f132baf3d13f03` checked all thirteen state-import modules and their kernel proof terms.
The gate returned `0` with unchanged inputs in `target/verification/state-import/closure-gate.HgAE94/`.
It used a 2 GiB memory limit, no swap, and one CPU.

These results do not finish the accepted-page overlay or publication proof.
The page checker still needs exact received-row coverage, canonical response-cursor comparison, and compatible durable writes.
The final scan must check the original required root, not the final singleton subtree.
The concrete nested decoder and production correspondence remain explicit obligations.

The path proofs take an abstract history reader.
Hash authentication requires the existing checked-reader instantiation and its codec correspondence.
At that checkpoint, the interleaved result covered zero-skip, unanchored pages with initial origin and frame evidence.
The singleton-origin theorem then covered a history export step, but not final-history selection or the initial anchor.
The checked-page composition below adds those connections.

The plan review also strengthened the occurrence witness.
It now binds the event key and leaf/history shape even when an unknown path makes both reference decoders return `None`.
The previous operational proof constructed the correct key, but its witness predicate did not express that fact independently.
Unknown kinds still reject rather than changing traversal output.

The generated native property uses 256 cases, each with two to 32 channels derived from an arbitrary 64-bit seed.
It compares each complete subtree occurrence set with all full-root occurrences below the selected origin.
The comparison detects omitted, extra, duplicated, or incorrectly located leaf occurrences.
It also checks the original root through normal typed joins reads.
This structural property does not simulate concurrent storage writes.

The additional rejection regression uses an empty collection accepted by both production joins and data decoders.
Its history path requires joins, but a substituted data envelope retains the same hash and decodable inner bytes.
This control prevents an envelope-selected nested decoder from appearing to satisfy the occurrence-kind requirement.
It is an import-boundary fixture, not evidence that normal checkpoint processing retains empty collections.
Normal checkpoint processing removes empty collections.
The repaired validator still needs explicit tests for retained origins, absent origins, and incorrect origins.

### Reviewed occurrence checkpoint

The plan reviewer accepted the explicit key-and-shape witness and the exact subtree comparison.
The reviewer also confirmed the purpose and reachability limit of the dual-decoder rejection control.
No further defect was found in that bounded review.

That strengthened occurrence checkpoint contained 397 lines and 19 proof terms.
Invocation `2035d83295744c28a3070c39c095d11a` checked all thirteen modules and their kernel proof terms with unchanged inputs.
Its gate returned `0` in `target/verification/state-import/closure-gate.61smdr/` under the same 2 GiB resource limit.

Invocation `3c1d90da3cbc43ad97a2cb7c9fcce251` passed all 256 generated subtree cases, strict Casper lint, and formatting.
Its three focused native tests produced one passing positive control and two expected pre-repair rejection failures.
Both failures reached their intended assertions after the preceding decoding, hash, and traversal checks passed.
The generated cases ran in 0.97 seconds, and the native examples ran in 1.88 seconds after compilation.
The inputs remained unchanged in `target/verification/state-import/context-quality.TOLN3v/`.
The scope limited memory to 5 GiB, disabled swap, and used one CPU.
The combined concurrent proof and native limits were 7 GiB.

These results qualify this correspondence step, not the production repair or the campaign's complete verification gate.

### Publication review limits

The plan review found that tuple-space startup calls `set_root` before receiving pages.
The root store writes both a root marker and the current-root pointer.
That existing order cannot serve as evidence for publication after complete validation.
This route needs its own regression and a reviewed repair decision.

The existing TLA publication model adds one validated reference per abstract write action.
A physical cold batch can establish several contextual references at once.
The current correspondence therefore cannot claim literal single-step refinement for such batches.
The remaining composition must state its abstract-step simulation or add a compatible batch action to the existing model.
It must also retain unresolved work when a commit outcome is unknown.
An unknown outcome does not justify the definite-rollback rule.

## Checked history overlay and cursor composition

A **history overlay** is a temporary lookup view that combines received history rows with observed stored rows.
Overlay preparation does not write the storage backend.
The [overlay module](../../../../formal/rocq/cost_accounted_rho/theories/StateImportOverlay.v) specifies validation before construction of that view.

Each received row must have a valid 32-byte key, matching raw-byte hash, and valid radix encoding.
The checker preserves exact bytes rather than decoding, normalizing, and rehashing the node.
This preserves empty-node encodings and unique radix records in any slot order.
Identical repeated rows remain valid.

| Observed stored row | Received row | Preparation result |
| --- | --- | --- |
| Absent | Valid | Add the row to the temporary view. |
| Identical bytes | Valid | Retain the existing binding. |
| Different bytes | Valid | Reject the conflict without changing storage. |
| Read failure | Valid | Report a storage failure, not absence or invalid peer content. |
| Any result | Invalid | Reject the received row without changing storage. |

The proof checks every received row before map construction can hide repeated keys.
Successful preparation preserves every observed binding and leaves unreceived keys unchanged.
One successful ordering implies success for every permutation, with equal lookup results.
Rejected permutations need not report the same first error.
These statements quantify over arbitrary finite row lists, not only the native two-record control.

### Exact traversal and origin evidence

The combined theorem uses a checked raw-history reader for the final traversal view.
The initial formulation allowed an arbitrary decoded final reader.
That formulation could add unchecked ancestor bindings even when every emitted history row passed validation.
The review identified this missing premise before production changes.
The revised theorem authenticates every successful final-view read, including ancestors absent from the received row list.

The occurrence relation now covers Start anchors, Resume paths, arbitrary natural-number skips, and positive history budgets.
It retains the annotated page, slice, and cursor relations rather than only erasure and root reachability.
Each leaf position therefore retains the exact selected edge and original-root path from its export step.
This distinction prevents repeated keys under different stored-value types from borrowing another occurrence's path.

The final singleton uses the last history occurrence in that ordered traversal.
Its proof retains that occurrence's original-root path and reconstructs the exact 32-byte key.
It does not choose an arbitrary occurrence with the same stored key.
The combined theorem connects this result to the exact encoded response cursor.
No wire-format change or global traversal lock is required by these proofs.

### Regression results

The [runtime regressions](../../../../casper/src/rust/engine/runtime_state_import_tests.rs) exercise the real validator and importer adapters.
The focused run produced three passing controls and three expected failures before repair.

| Check | Result before repair | Interpretation |
| --- | --- | --- |
| Empty-node encoding | Passed. | The checker must continue to accept the existing empty encoding. |
| Unsorted records and identical rows | Passed for all six two-record permutations with one repeated row. | Row order and identical repetition do not invalidate a page. |
| Conflicting repeated rows | Rejected in both orders. | The existing hash check catches the deliberately hash-invalid duplicate. |
| Conflicting stored bytes | Incorrectly accepted. | The incoming overlay hides the occupied-key conflict. |
| Required stored-row read fails | Incorrectly accepted. | The incoming overlay bypasses the failing compatibility read. |
| Duplicate radix slots | Panicked. | Matching raw-byte hashes do not establish valid radix syntax. |

The stored-byte conflict fixture deliberately installs corrupt bytes at an occupied key.
It tests conflict detection, not a collision between two valid cryptographic hashes.
The repeated-row control also uses one hash-invalid row and does not synthesize a hash collision.
The read-failure fixture proves that the injected backend error is active before it invokes the actual validator.
All checks require unchanged destination storage.

The duplicate-slot property has 32 configured cases and a maximum of 32 shrink steps.
It fails on its first generated input, so it does not establish 32 successful checks.
Every generated input repeats a radix slot while retaining the hash of its actual raw bytes.

Invocation `f21e12380d384e21b3c67c9e873d8188` recorded these results in `target/verification/state-import/overlay-native.Gt5TPu/`.
Native execution took 1.56 seconds after compilation.
Strict Casper Clippy, formatting, and input hashes passed.
The scope used a 5 GiB memory limit, no swap, and one CPU.

### Generated cursor correspondence

The [cursor property](../../../../casper/src/rust/engine/horizon_state_import_tests.rs) compares paginated traversal with a complete occurrence trace.
The reference trace orders complete history and leaf occurrences by their absolute trie paths.
An independent counter loop then applies history-based skip and take to that trace.
The property compares exact ordered keys and paths, not only key sets.

The 256 generated cases use arbitrary 64-bit seeds and two to 64 normal hashed channels.
They select Start or Resume, including the empty Resume prefix, and skip counts from zero through 79.
Budgets range from one through seven, with additional budgets of 750 and 1,024.
For each nonterminal result, the property checks the cursor prefix and the complete remaining occurrence sequence after resumption.
These small tries do not establish an actual page boundary at the production budgets.
Production-sized boundary tests remain required.

Invocation `7e41173a15294b98965907fd5e28dfc5` passed the generated property in 1.23 seconds after compilation.
Strict Casper Clippy, formatting, and input hashes also passed.
The evidence is in `target/verification/state-import/cursor-native.Uld33Q/` under the same 5 GiB resource limits.
This is a traversal property, not a concurrent store or whole-node test.

### Formal qualification and remaining boundary

The occurrence module now contains 614 lines and 28 proof terms.
The overlay module contains 601 lines and 32 proof terms.
Invocation `a59b0faabd354bec8194397dac484c7f` compiled all fourteen import modules and kernel-checked their proof terms.
The gate returned `0` with unchanged inputs in `target/verification/state-import/closure-gate.42QRTI/`.
It used a 2 GiB memory limit, no swap, and one CPU.

These are successful final-view proofs, not a claim that every physical read occurs against one snapshot.
Physical observation traces must distinguish unread keys, observed absence, and failed reads.
A preserved stored binding does not guarantee that a later input/output operation succeeds.
The production reader must abort the attempt on a required read failure and retain recovery ownership.

At this checkpoint, cold-row preparation lacked its complete link to the occurrence-aware checker.
The next section records that connection and its separate production limits.
The repair must also recheck compatibility at the write transaction, then apply only the validated rows to current storage.
Complete-root scanning, publication, startup ordering, and unknown commit outcomes remain separate required composition steps.
No production import-validation repair has been applied at this checkpoint.

## Received cold rows and guarded consumption

A **received cold row** contains the raw hash key and original encoded stored value from one response.
A **received-only map** contains these rows without fallback to durable storage.
The [received-cold module](../../../../formal/rocq/cost_accounted_rho/theories/StateImportReceivedCold.v) connects row validation, duplicate handling, occurrence authorization, and guarded cold batches.

### Validation and normalization

The checker applies the following sequence before durable writes.

1. Check the raw key has exactly 32 bytes.
2. Check the byte domains of the key and encoded value.
3. Parse the persisted envelope without replacing its original bytes.
4. Compare the payload-frame hash directly with the raw key.
5. Require a matching cold occurrence from the canonical traversal.
6. Check the parsed leaf against every matching occurrence kind.
7. Validate every original row before constructing the received-only map.
8. Retain the first compatible encoding for each key.
9. Check every emitted occurrence against the received-only map.

The envelope supplies its parsed kind, but the authenticated occurrence path supplies the expected kind.
The checker does not infer authorization from the envelope alone.
Two occurrences with the same key retain separate kind requirements.
The existing occurrence theorem rejects one payload when those requirements conflict.

Raw hash equality establishes the exact validated key before numeric conversion.
The key-conversion theorem therefore excludes width-based aliases without assuming hash injectivity.
Every retained encoding comes from an original response row.
Every received key corresponds to an emitted cold occurrence, and every emitted cold occurrence has a received payload.
Stored bytes cannot replace a missing response row.

History and cold duplicates have different compatibility rules.

| Row class | Compatibility requirement | Result under permutation |
| --- | --- | --- |
| History | Identical encoded bytes for each key. | Equal lookup bytes. |
| Cold | Equal parsed kind and exact inner payload for each key. | Equal decoded values and key coverage. |

Existing envelope trailers remain accepted.
Compatible cold duplicates can therefore contain different trailing bytes.
Reordering these rows can change the retained original encoding, but not the decoded map.
The equality theorem covers absent keys as well as present leaves.
The acceptance theorem quantifies over arbitrary finite row lists and every permutation.

First-encoding retention supplies a stable local representation during staged insertion.
It is not a new consensus rule for trailing bytes.
The decoded-value invariant remains the semantic requirement across different row orders.
Previously stored compatible bytes must remain unchanged.

Two executable proof controls check these distinctions.
One control accepts different envelope trailers in both orders and retains each order's first encoding.
The other supplies a constant hash to two different decoded leaves.
Both rows pass individual authorization, but their combined staging rejects in both orders.
This artificial control checks independence from hash injectivity.
It does not claim a cryptographic collision in the native implementation.

### Exact traversal and commit connection

The composition theorem obtains its occurrence list from the checked history overlay and exact operational cursor relation.
The theorem retains original-root witnesses and the encoded response cursor.
It does not substitute an arbitrary reachable set or a list with only matching keys.

The transaction input contains all validated original rows, including compatible duplicates.
This preserves original bytes and avoids an unproved conversion from a normalized map to a different batch.
The existing encoded batch model checks both aliases against the actual commit state.
Later compatible rows preserve the consuming reads established by earlier rows.
The combined theorem derives a typed consuming read for every authorized cold occurrence after a successful batch.

These temporary maps do not represent physical storage observations.
The physical adapter must distinguish unread keys, observed absence, present bytes, and storage failure.
A failed read must not create an absent entry in the observation map.
The theorem assumes the same validated rows reach the transaction.
The current importer preserves supplied row inputs, but does not satisfy the combined validation and guarded-commit premises.

### Native correspondence results

The focused native run used the actual validator, importer, stores, channel hashing, and joins reader.
Invocation `a0e23d08b60348cc8120a779556210cc` recorded seven tests in `target/verification/state-import/received-cold-native.hiWHqR/`.
Five tests passed and two failed before repair.

| Check | Observed result | Meaning |
| --- | --- | --- |
| Compatible duplicate envelopes | Passed in both orders. | Existing trailing-byte acceptance remains supported. |
| Generated duplicate permutations | Passed 64 cases, with each selected order and its reverse. | Different compatible trailers preserve typed joins results. |
| Malformed duplicate envelope | Rejected in both orders. | Existing envelope parsing rejects the malformed original row. |
| Non-hash-width keys | Rejected without panic for every length from zero through 65, except 32. | Tested invalid key lengths fail closed. |
| Missing or extra response keys | Rejected. | Durable copies do not hide a missing response key. |
| Wrong-kind duplicate with unchanged hash | Incorrectly accepted in the first tested order. | Duplicate handling does not repair missing occurrence-kind authorization. |
| First compatible encoding during import | Replaced by a later compatible encoding in the first tested order. | The current importer differs from the selected stable-byte policy. |

The generated property uses one through eight duplicate rows and zero through 16 arbitrary trailer bytes per row.
An arbitrary machine-sized integer selects a rotation, and the test also checks its reverse.
The shrink limit is 256 iterations.
Each destination starts empty and receives the same real joins payload.
The property checks unchanged storage during validation, exact typed reads after import, and one retained cold key.
It does not test concurrent publication or production-sized page boundaries.

The wrong-kind fixture uses an empty inner collection to separate kind authorization from nested item decoding.
The stable-byte failure does not demonstrate changed contract values or a new consensus disagreement.
Both failing tests stop at their first failed assertion, so this run does not establish their second-order result.

Native execution took 7.38 seconds after compilation.
Strict Casper Clippy, formatting, and input hashes passed.
The scope used a 5 GiB memory limit, no swap, and one CPU.

### Remaining production and proof boundaries

The received-cold module contains 569 lines and 26 checked proof terms.
Invocation `ffe05357ca364d7cbde4b9d981508826` compiled all fifteen import modules and kernel-checked their proof terms.
The gate returned `0` with unchanged inputs in `target/verification/state-import/closure-gate.fexMTL/`.
The scope used a 2 GiB memory limit, no swap, and one CPU.

The remaining composition must connect fallible physical observations to the guarded transaction model.
Complete-root scanning, root publication, retry ownership, startup ordering, checkpoint content writes, and unknown commit outcomes still require their final connection.
The native decoder correspondence must preserve existing trailing-byte behavior at every encoded layer.
Production-sized pagination and concurrent recovery remain required conformance tests.
No production import-validation repair has been applied in this increment.

## Physical cold observations and concurrent commit

The [observation module](../../../../formal/rocq/cost_accounted_rho/theories/StateImportObservations.v) connects fallible storage reads to received-row validation and guarded cold commits.
A **physical location** identifies one hash key in either the raw-key format or the legacy serialized-key format.
An **observation cache** retains the first preflight read result for each physical location during one attempt.
This cache belongs to the attempt, not the shard or validator.

### Read states and production contract

The observation model distinguishes four states.

| State | Evidence | Can the location satisfy commit readiness? |
| --- | --- | --- |
| Unread | No physical read occurred. | No. |
| Observed absent | A successful read returned no stored value. | Yes. |
| Observed bytes | A successful read returned exact stored bytes. | Yes, subject to compatibility checks. |
| Read failure | The storage operation failed. | No. |

Each read record contains its physical location, result, and storage state at that read.
Compatible writes can occur between reads and after the final read.
The proof does not require one shared snapshot for preflight reads.
Each successful commit compares both observed key formats with the current transaction state.

The adapter must reuse cached observations for duplicate keys during preflight.
It must not perform another preflight read and silently retain an earlier successful result after a later failure.
The separate commit transaction must still perform its own fallible comparisons.
A required read failure ends the attempt without accepting its page.
A retry starts a new attempt with a new observation cache.

The following pseudocode states the intended cache behavior.
It is a production conformance requirement, not a description of the current Rust importer.

```text
observe(location, cache, store):
    if cache contains location:
        return cache[location]
    result = store.read(location)
    cache[location] = result
    return result
```

Both key formats must have successful observations for every row in the committed batch.
The projection into an encoded store maps non-byte observations to an absent default.
Readiness prevents unread and failed required locations from using that default.
A separate theorem proves that unrelated projection values cannot affect the batch.

The read-once theorem derives unique recorded physical locations from the trace rules and retained-record invariant.
It does not assume unique records.
It also does not bound the number of distinct locations read.
Resource limits remain a separate obligation.

### Guarantees and limits

The final composition theorem requires a read trace that ends at the actual commit state.
It derives the following results from checked received rows and a successful guarded batch.

1. Every required key format has a successful physical read record.
2. Each recorded value matches storage at its own read and at commit.
3. Compatible concurrent writes preserve existing bindings throughout that trace.
4. The batch preserves every existing physical byte binding.
5. Every authorized cold occurrence has a typed consuming read after commit.

The theorem retains the original validated row list, including compatible duplicates.
It does not replace received rows with a different transaction payload.
Failure retention covers recorded failures for keys in that required row list.
The model does not claim unrestricted failure retention for unrelated reads or later attempts.

The concurrent-write control inserts a valid cold envelope between the raw-key and legacy-key reads.
The successful import preserves that unrelated insertion.
A separate control starts with unrelated malformed bytes and proves only their preservation.
That malformed-value control does not represent a compatible insertion into empty storage.

Other controls reject an unread opposite key format, a failed required read, and a stale observation after an opposite-format insertion.
Another control confirms that a cached failure cannot become success within the same attempt.
The module contains 574 lines and 31 proof terms.

History-read observation, complete-root scanning, publication, and unknown transaction outcomes remain outside this result.
The Boolean storage result covers success or definite rollback only.
The read cache and atomic compatibility checks still require production implementation and native conformance tests.

### Native evidence before repair

Invocation `158031be86f84d368340c2408f19d0ba` ran the real importer and reader with injected storage failures.
The evidence is in `target/verification/state-import/observation-native.b2oN9a/`.

| Test | Result | Conclusion |
| --- | --- | --- |
| Import preflight with a failure in each key format | Failed before repair for both formats. | The validator accepted the page without performing either required compatibility read. |
| Ordinary reader with a raw-key read failure | Passed. | The reader returned the error without trying the legacy format. |
| Existing history-read failure regression | Failed before repair. | The importer still lacks the required fallible history observation. |

The preflight test checks both injected locations before asserting its combined result.
It also checks unchanged storage and confirms that each fault activates through the real store wrapper.
The ordinary reader test checks the exact read sequence and unchanged storage.
These results identify a missing import check, not a defect in ordinary raw-first error propagation.

Strict Casper Clippy, formatting, and input-hash checks passed.
The native scope used a 5 GiB memory limit, no swap, and one CPU.
These focused tests do not prove that this defect caused a historical CI failure.

The plan-agent review accepted the corrected cold-read contract.
It required the trace-linked final theorem, the explicit read-once restriction, and a valid concurrent-write witness.
No production import-validation repair has been applied at this checkpoint.

## Physical history observations and logical lookups

The [history-observation module](../../../../formal/rocq/cost_accounted_rho/theories/StateImportHistoryObservations.v) separates durable history reads from logical traversal lookups.
The received rows remain fixed throughout one attempt.
Each physical record retains its key, result, and storage state at that read.
Each logical record retains its requested key, cache at that lookup, and computed answer.

### Why the distinction matters

The existing traversal reads the root during export setup and again during stack construction.
It can also encounter one stored subtree through different occurrence paths.
These logical lookups do not require new physical reads.
An attempt-local cache can reuse raw history bytes without changing the traversal paths, budgets, or cursor semantics.

The cache must not retain occurrence-specific prefixes or expected cold kinds as attributes of a shared hash.
Those properties belong to each occurrence.
The model retains raw bytes and derives context-independent history edges through the checked decoder.

Received rows cannot bypass compatibility reads.
Before traversal starts, each received key must have a successful durable observation.
Observed bytes must equal the received history bytes exactly.
Observed absence permits a prepared received row, subject to the later transaction comparison.
Unread keys and storage failures cannot satisfy preparation.

For a key outside the received rows, an unread cache entry requires a physical read.
The operational result distinguishes this request from successful absence, storage failure, invalid bytes, and checked edges.
The proof-only partial reader does not serve as the production error classifier.

### Proven connection

The history trace permits compatible byte-preserving writes between reads.
It derives physical record validity, retained observations, and one physical read per key.
Repeated logical lookups can reuse those records.
The proof does not assume one shared storage snapshot.

Each successful logical lookup has one of two byte sources.

| Source | Required evidence | Relation to current storage |
| --- | --- | --- |
| Received row with observed bytes | Original received row and an equal durable observation. | The observed bytes remain present under compatible writes. |
| Received row with observed absence | Original received row and a successful absence observation. | No current-byte guarantee exists before the transaction comparison. |
| Durable fallback | A successful physical read record outside the received key set. | The observed bytes remain present under compatible writes. |

Every successful answer also has a checked key width, byte domain, exact hash, and radix decoding result.
An arbitrary final reader cannot supply the bytes for a recorded successful lookup.
Cache growth preserves each recorded successful answer.
The final-view theorem connects those earlier answers to the final cache.

The received-absence distinction is necessary for concurrency.
A compatible writer can fill a previously absent key before the import commits.
The model therefore retains the original absence record instead of inventing a current binding.
The received-key transaction comparison remains required.

The overlay-preparation function in the logical-access definition specifies the resulting lookup view.
It does not require the implementation to rebuild the complete overlay on every callback.
Production must prepare the immutable received-row map once and use the separate observation cache for later fallback lookups.

### Controls and remaining execution connection

The module contains 552 lines and 29 proof terms.
An executable trace contains two successful logical callbacks and exactly one physical read.
Other controls reject unread received keys, failed compatibility reads, conflicting stored bytes, incorrect hashes, and malformed radix payloads.
A cached absence remains absent after an attempted same-cache refresh.
A fresh cache can observe subsequently stored bytes.
These controls use a synthetic hash function to test contracts, not native cryptographic collision behavior.

The current trace records operations but does not yet enforce terminal attempt failure.
In particular, preparation can fail because another received key is still unread.
Later observation can make that preparation succeed.
The cursor adapter must establish a preparation-ready barrier before traversal and stop on a required lookup failure.

The final-logical-view theorem preserves successful answers only.
It does not permit a cursor machine to ignore earlier failures or invent unrecorded reads.
The read-tape refinement below consumes the exact logical lookup sequence.
Shared-subtree paths, dynamic child reads, and original-root witnesses must remain attached to that execution.
Commit, complete-root scanning, publication, and unknown transaction outcomes still require their final composition.

### Native evidence before repair

The native fixture uses the real exporter, history store, importer, and canonical traversal.
It skips the outer root and supplies its history child, so validation must read the root from durable storage.
The unfaulted baseline accepts this page.
The read-count assertions permit separate compatibility reads for the received child.

Invocation `a6ab7aceee944bf0be64bfd0828dfcad` recorded the focused results in `target/verification/state-import/history-observation-native.FmX3bF/`.

| Test | Observed result | Required correction |
| --- | --- | --- |
| Fallback storage failure | The importer panicked at `get_history_item` instead of returning the injected error. | Use a fallible import adapter and preserve the error classification. |
| Repeated root lookups | Two physical reads occurred for the same root. | Reuse the first observation within that preflight attempt. |

Both regressions fail before repair.
The first failure demonstrates incorrect error handling.
The second records the missing cache contract, not a separate demonstrated consensus failure.
Both tests check unchanged storage.
Strict Casper Clippy, workspace formatting, and input-hash checks passed.
The scope used a 5 GiB memory limit, no swap, and one CPU.

The initial fixture reached the separately known leaf-only exporter failure before either target assertion.
That initial run does not establish the two history defects.
The corrected fixture includes a history child and reaches both intended checks.

The formatting check also exposed different tuple formatting under the production and Loom configuration files.
A named staged-value type now makes the shared transaction alias stable under both configurations.
That alias change does not change transaction operations, types after expansion, or lock behavior.
No production import-validation repair has been applied in this increment.

### Combined proof qualification

Invocation `f440693498434eedaf831d56392d3ab9` compiled all seventeen import modules and kernel-checked their proof terms.
The gate returned `0` with unchanged source hashes in `target/verification/state-import/closure-gate.RyJcHN/`.
The scope used a 2 GiB memory limit, no swap, and one CPU.
This result qualifies the stated module theorems, not the remaining complete importer or cursor-execution connection.

## Exact callback sequence and cursor execution

The [read tape](../../../Glossary.md#read-tape) connects physical history observations to the existing cursor and export relations.
`StateImportReadTape.v` contains 704 lines and 37 proof terms.
This module uses the actual logical callback list from one history-observation trace.
The module does not select successful records or construct a separate complete lookup map.

### Execution phases

The evaluator uses the following phases.
Each phase returns an error before the next phase can execute when a required condition fails.

| Phase | Checked behavior | Source correspondence |
| --- | --- | --- |
| Received-row preparation | Prepare every received history row against the first callback's observation cache. | The proposed immutable preflight overlay. |
| Root availability | Consume exactly one callback for the cursor root. | The outer `sequential_export` root probe. |
| Stack construction | Consume another root callback and each required ancestor callback in order. Check complete compressed prefixes and the carrier. | `init_node_path`. |
| Anchored traversal | Apply the existing Start/Resume, skip, and history-budget rules. Consume callbacks only for selected history edges. | `add_element` and `export_step`. |
| Invocation completion | Require an empty remaining callback list. | The checked adapter's invocation boundary. |

Root probing and stack construction remain separate callbacks even when the history budget is one.
Both callbacks can reuse one physical observation from the attempt cache.
Stack construction precedes the budget stop.
Popping a completed frame and exporting a cold reference consume no history callbacks.
Shared hashes retain separate callback occurrences and traversal prefixes.

The central read operation is `import_consume_history_read`.
This operation inspects only the next record.
It requires exact key equality and a checked successful lookup before returning edges.
Missing, unexpected, and trailing records have distinct errors.
Absent, invalid, failed, and unfinished lookup results retain their classifications.
Preparation errors retain the original overlay result and key.

### Proven guarantees

The successful-prefix relation records both the consumed prefix and the remaining suffix.
Every consumed record contains a successful checked answer.
Composition preserves this relation through stack construction, export steps, slices, and cursor execution.
An accepted complete invocation therefore cannot contain an unsuccessful callback anywhere in its tape.

`import_history_first_callback_has_an_actual_preparation_prefix` exposes the physical prefix equation for the supplied trace.
The prefix contains no logical callbacks and ends with the first callback's actual observation cache.
The theorem also preserves every byte binding from the prefix store through the final store.
Successful preparation thus precedes traversal within the traced execution.

The final reader derives from that same trace's final cache and the immutable received rows.
`import_prepared_observed_reader_equals_overlay_reader` proves pointwise equality with the checked overlay reader.
`import_checked_actual_tape_derives_its_final_checked_overlay` constructs the final prepared overlay and derives its operational cursor export.
The theorem does not accept an unrelated final reader as a premise.

The stack and slice refinements preserve exact frames, entries, skip counts, and history budgets.
The stack fuel follows the cursor path length.
The slice fuel follows the existing export potential, including skipped history, taken history, and unfinished frames.
The derived-fuel theorems rule out synthetic fuel exhaustion under their stated reader-agreement premises.
These are termination bounds for the evaluator, not measured runtime or memory guarantees.

### Formal controls and native properties

The module contains ten controls.
They check failed-read replacement, missing ancestors, reordered records, unfinished observations, repeated root reads, extra records, carrier mismatch, root absence, and unprepared received rows.
The successful two-root-callback control includes an actual observation trace with one physical read.
The control uses the existing synthetic hash fixture and does not assert a native cryptographic collision.

The native properties call the real `sequential_export` implementation with recorded callbacks.
They use actual radix serialization and hashes.

| Native property | Generated inputs | Required behavior |
| --- | --- | --- |
| `read_tape_native_shared_occurrences_preserve_callback_order` | 128 cases with distinct slots, compressed prefixes, leaf bytes, and page budgets. | Root setup uses two callbacks. Shared children retain every occurrence. Resumption rebuilds the carrier before later child reads. |
| `read_tape_native_required_failure_stops_before_later_callbacks` | 128 cases with shared-child roots and a selected callback failure position. | A required missing stack or child read stops immediately. No later callback executes. Initial root unavailability remains a separate exporter response. |

Invocation `da9339b7940a4cd3b54e07f4307102ce` passed both properties, strict Casper Clippy, workspace formatting, and unchanged-input checks.
The evidence is in `target/verification/state-import/read-tape-native.DIy4Lf/`.
The tests completed 256 generated cases in 0.17 seconds after compilation.
The scope used a 5 GiB memory limit, no swap, and one CPU.
An earlier test draft failed compilation because it used an incorrect enum variant name.
The qualified run uses the actual `Item::Leaf` variant.

Invocation `b14d3c5d9f8741a1b6b9f70d293d6c06` compiled and kernel-checked all eighteen import modules.
The gate returned `0` with unchanged inputs in `target/verification/state-import/closure-gate.txwhbH/`.
The scope used a 2 GiB memory limit, no swap, and one CPU.
The aggregate campaign script now checks the read-tape theorem assumptions, but this increment did not rerun that whole campaign gate.

### Scope and remaining connections

This evaluator proves strict accepted-page refinement.
The raw exporter can return an empty unavailable response when its initial root read finds no value.
Such a response must not count as validated complete state.
The native failure property preserves this distinction, but the tape evaluator does not model every outer exporter response.

The original-root occurrence evidence remains separate from the wire cursor root.
Any additional reads that establish that evidence need their own checked phase and explicit sequence connection.
Do not remove such reads as an arbitrary prefix of the traversal tape.
Cold consumption, history commit compatibility, complete-root scanning, and publication still require their final composition.
The physical trace permits compatible writers between reads, but does not yet cover every checkpoint transaction or unknown commit outcome.

These results establish successful refinement and the stated rejection and termination guarantees.
They do not yet prove completeness for every valid instrumented execution or complete error equivalence with native code.
The two earlier importer regressions remain failures before the production repair.
The new passing properties establish callback correspondence, not completion of that repair.
No Casper voting, fork-choice, threshold, or peer-wire change forms part of this increment.

## Startup constructor publication regression

`startup_import_construction_must_not_publish_unreceived_state` tests the real startup stream constructor, importer, and in-memory storage.
The fixture starts with a valid empty current root and a different target root exported from a nonempty trie.
The destination does not contain that target root or its imported state.
The approved-block fixture supplies only the constructor's requested state hash for this storage test.
This test does not exercise or replace approved-block authentication.

The test awaits stream construction without polling the returned stream or sending any pages.
Its network and page-validation callbacks fail immediately if construction calls them.
The actual history, cold, and root stores remain directly observable.

The constructor calls `state_importer.set_root` before returning the lazy stream.
The real importer calls `record_root`, which writes the target tag and then updates `current-root`.
The regression observes both changes while the target history root remains absent.
Dropping the unpolled stream does not undo either change.
The earlier assertions confirm unchanged history and cold data, no queued pages, and no reported stream error.

Invocation `9a95c6f5bbc642368e2f6ec09bf1fd2e` reaches the intended final assertion and fails before repair.
The evidence is in `target/verification/state-import/startup-publication-red.bH374x/`.
Strict Casper Clippy, workspace formatting, and unchanged-input checks pass for the same source snapshot.
The scope used a 5 GiB memory limit, no swap, and one CPU.
Both callback properties also pass against this updated test file in `target/verification/state-import/read-tape-native-current.N0rlRY/`.
That recheck records invocation `3f0bc1ea82a3458ea5b33ab2430a13e0` and another 256 generated cases.

The startup requester and concrete importer match the stored pinned-dev source snapshots byte for byte.
This observation confirms the earlier source-level comparison without attributing a historical CI failure to this path.
The local Git object database does not contain that dev commit, so this check uses the retained source snapshots.

The TLA+ publication model already represents the prohibited order through its `EarlyTag` control and subsequent `WriteCurrentRoot` action.
The safe model requires completed closure scanning before tag publication.
The native regression now gives this model control a direct startup-path witness.
Production repair must preserve the prior usable root until the target state passes the complete validation and publication checks.
No startup production code changed in this regression increment.

## Observed history transactions and committed traversal

An absent preflight observation does not reserve a history key.
Another importer or checkpoint can insert bytes before the transaction starts.
The commit must check current storage, not overwrite bytes based on the earlier absence.

`import_observed_history_batch` separates preflight validation from the atomic commit state.
Preflight requires a successful recorded observation for every received key.
Each received row must have a valid key, matching raw-byte hash, and valid radix encoding.
The transaction accepts only absent or byte-identical current bindings.
Identical duplicate rows succeed, but conflicting duplicates reject the complete batch.

`import_history_batch_commit_can_reuse_preflight_authentication` proves that commit-time guards need only current byte comparisons.
The implementation can validate hashes and encodings before acquiring the transaction lock.
It must retain those exact validated bytes through commit.
This result permits an intervening identical insertion without an unnecessary retry.
Validation and independent import work can remain concurrent.
The transaction uses the existing backend commit boundary, not a new validator-wide lock.

### Proven transaction requirements

| Requirement | Formal evidence |
| --- | --- |
| Every committed row has an actual successful compatibility read. | `import_history_batch_has_actual_successful_read_witnesses` connects the transaction to the physical observation trace. |
| Existing bytes survive each attempt. | `import_history_batch_preserves_all_existing_bytes` preserves every current binding, including malformed historical bytes. |
| Received bytes remain authenticated and exact. | `import_history_batch_writes_only_authenticated_received_bytes` establishes the received rows in the commit result. |
| Unreceived keys remain unchanged. | `import_history_batch_preserves_every_unreceived_key` prevents unrelated data loss. |
| A stale absence cannot authorize an overwrite. | `import_history_batch_rechecks_stale_absence_without_overwriting` rejects a conflicting current binding. |
| A known failure publishes no staged prefix. | `import_failed_history_batch_preserves_the_entire_commit_store` preserves the current store, not the earlier preflight store. |
| Concurrent writes preserve previously consumable roots. | The joint history/cold run extends established bindings across arbitrary finite transaction interleavings. |
| Commit preserves the accepted page traversal. | `import_accepted_page_after_history_commit_has_the_same_operational_export` connects the complete read tape to checked committed history. |

The last theorem preserves the cursor traversal, successful callback sequence, and history budget after commit.
Here, acceptance means that the exact callback tape passed its checks.
Response coverage, wire cursor comparison, and original-root authorization remain separate requirements.
It does not infer complete-root validity from one accepted page.
Every callback must still belong to the successful, completely consumed page invocation.
A successful row transaction alone cannot excuse a failed traversal callback.

### Physical storage and the logical projection

Physical history uses fixed-width byte keys.
The closure model projects these keys into natural numbers.
Converting unrestricted natural numbers back to 32 bytes can map multiple numbers to the same physical key.
Therefore, one physical insertion need not equal one logical point update across the unrestricted numeric domain.

The refinement proves byte-store extension and then decoded-map extension instead.
`import_encoded_storage_history_extension` connects that result to the existing joint storage run.
The existing guarded single-key constructor remains available.
The projection theorem does not require hash injectivity and does not establish physical write-count bounds.
Authentication of new rows remains a separate premise established by received-row checks.

### Verification evidence and controls

Invocation `24f7dab310d24d01856958daf28b798d` compiled and kernel-checked all eighteen import modules with unchanged inputs.
The gate returned `0` in `target/verification/state-import/closure-gate.teXLbf/`.
The scope used a 2 GiB memory limit, no swap, and one CPU.

Eight executable Rocq controls cover identical races, conflicting races, duplicate acceptance, unread observations, failed observations, late conflicts, definite write failure, and unrelated writes.
The tests use synthetic hashes only to exercise control flow.
They do not claim cryptographic collision resistance.

The native transaction-state property passed 512 generated cases.
The native concurrent-batch property passed another 128 cases with two actual worker threads.
These tests exercise the real in-memory `strict_atomic_mutate` implementation.
They verify complete success or rollback, duplicate handling, and preservation of unrelated history, cold data, and root metadata.
Their byte payloads test transaction behavior, not received-row hash authentication.
The transaction-state property deliberately ignores preflight values and permits arbitrary intervening overwrites.
Thus, it tests commit-time preservation without assuming that earlier reads remained valid.

Invocation `f3aba4e9d65f4ab28298a8fb861d5dd5` records these results in `target/verification/state-import/history-guard-native.gd9vNo/`.
Strict Casper Clippy, workspace formatting, and input-hash checks also passed.
The scope used a 5 GiB memory limit, no swap, and one CPU.
Plan review identified a possible test-harness wait if a worker asserted before reaching the barrier.
Both workers now synchronize before checking their captured read results.
Invocation `31045a51add143ce843fca974c40df45` rechecked all 640 cases, Clippy, formatting, and unchanged inputs after that correction.
The current evidence is in `target/verification/state-import/history-guard-native-current.eyLB4p/` under the same resource limits.

Four Loom tests exercise the production sparse transaction code with instrumented locks.
They cover competing identical batches, conflicting whole batches, duplicate rollback with a concurrent cold write, and an unconditional-write negative control.
All four passed under invocation `87a56064af1942fbbd78898c93d61779` in `target/verification/state-import/history-guard-loom.KoEXbg/`.
The negative control must fail its named overwrite assertion.
The exploration uses three threads and 5,000 branches per execution, with no preemption, permutation, duration, or checkpoint cutoff.
Branch exhaustion is a failure, not successful completion.
The scope used a 2 GiB memory limit, no swap, and one CPU.
Strict Loom-target Clippy, workspace formatting, and input-hash checks also passed.
These tests do not establish LMDB failure behavior or importer conformance.

### Remaining integration requirements

Both history write paths must use the validated-byte transaction contract: importer writes and `RadixTreeImpl::commit`.
Both cold write paths must preserve their two-alias contract: importer writes and checkpoint writes.
Complete-root consumption, original-root ownership, and publication ordering still need their final composition.
The transaction proof describes known rollback, not an unknown commit outcome or a write that can complete after cancellation.
The TLA+ lifecycle model must retain the corresponding operation ownership until an uncertain outcome resolves.
No production importer repair or new Casper rule forms part of this increment.

## Original-root ownership and validated page commits

The [owned-page module](../../../../formal/rocq/cost_accounted_rho/theories/StateImportOwnedPage.v) connects the requested root to each accepted page occurrence.
Its local owner contains the original root, the current wire cursor, and the occurrence path from that root.
This record identifies the captured request.
It does not establish certificate authorization or prove that the request still owns a live recovery task.

### Original-root validation

The owner phase traverses the recorded path from the original root through checked history callbacks.
The resulting top key must equal the wire cursor root.
A Resume cursor has a separate carrier check during page-stack construction.
The owner phase must compare against the cursor root, not that carrier.

The page phase discards the owner phase's ancestor stack.
Otherwise, the page traversal could leave the requested wire subtree and export unrelated ancestors or siblings.
The page phase consumes the remaining callbacks through the existing Start/Resume evaluator.
Owner-phase reads do not consume page budget or produce response entries.

Even an empty owner path consumes one logical callback before the page's two root-setup callbacks.
All three callbacks can reuse one actual physical observation.
The original path has no artificial 128-byte limit.
The wire Resume prefix retains its existing encoding limit.

Shared hashes do not identify unique occurrences.
Two roots can reach one subtree through different paths.
One root can also reach the same subtree more than once.
The model retains each path instead of selecting an arbitrary path from a hash-indexed table.

### Executable occurrence collection

An existential occurrence list does not identify the list that runtime validation uses.
Erasing occurrence paths also cannot establish the expected cold kind.
For example, the same leaf hash under paths beginning with `0` and `2` requires different typed consumers.

The read-tape evaluator now uses one parameterized collector.
The original entry-only evaluator is a specialization of that collector.
The occurrence specialization attaches paths from the pre-step frames.
The Start anchor receives the local owner origin exactly once.
Skipped entries and owner-phase reads emit no occurrences.

Three erasure theorems preserve exact entries, errors, final frames, and remaining callbacks.
Separate operational theorems prove the occurrence paths themselves.
The collector still charges its history budget from the original event, not the emitted-list length.
This construction requires no second physical traversal and no production callback tape.
The formal tape records execution evidence.

`import_actual_owned_collector_authenticates_locations_before_writes` binds the returned list to actual observations before either content transaction.
`import_actual_owned_collector_preserves_the_same_locations_after_commit` preserves that same list in committed history.
Neither theorem substitutes an unrelated existential list after validation.

### Preparation and commit order

`import_prepare_owned_page` computes the occurrence list itself.
It does not accept caller-supplied occurrence paths.
The preparation function validates every original cold row before converting cold keys for response coverage.
This order preserves the existing validate-before-conversion requirement.

| Phase | Required result | Failure behavior |
| --- | --- | --- |
| Owner and page traversal | Exact checked callbacks and returned occurrence paths. | Reject before attempt writes. |
| Cold-row validation | Valid key width, byte domain, hash, payload decoding, and every expected occurrence kind. | Reject before attempt writes. |
| Response comparison | Exact start cursor, next cursor, and history/cold key coverage from the actual rows. | Reject before attempt writes. |
| History transaction | Current bindings are absent or byte-identical to validated rows. | Retain the previous owner. Do not start the cold transaction. |
| Cold transaction | Both key formats pass recorded-observation and current-storage checks. | Retain committed history and the previous owner. |
| Owner advancement | Both transactions succeeded. | Return the exact derived continuation only on success. |

`import_invalid_owned_page_has_no_attempt_writes` applies to writes by the invalid attempt.
It does not require global storage equality while independent checkpoint writers operate.
`import_failed_cold_commit_keeps_history_and_the_previous_owner` preserves a successful history prefix after known cold rollback.
The model does not pretend that separate history and cold transactions are one atomic transaction.

The next owner retains the original root.
A nonterminal Resume retains its existing local origin.
An exhausted page with history entries selects the last emitted history occurrence and its exact original-root path.
An exhausted page without history entries retains the complete previous owner coordinates.

### Actual reads and concurrent storage

`import_owned_commit_connects_actual_reads_to_exact_typed_consumption` connects the preparation and commit functions to both physical observation traces.
The theorem preserves exact cursors, received-key coverage, existing bytes, and successful physical observations of both cold key formats.
Every emitted cold occurrence has a successful structural and typed consuming read in the committed logical view.
The original-root path for the next owner remains authenticated.

The history and cold fields in the commit result are transaction receipt views.
They are not one atomic snapshot of live storage.
`import_owned_locations_survive_later_history_extensions` preserves the same locations under an explicit later history-byte extension.
`import_owned_receipt_consumption_survives_compatible_writes` preserves consuming reads across later history extensions and guarded cold runs.
Cold byte extension alone is insufficient because a new conflicting alternate key format could invalidate strict resolution.

The Boolean transaction results describe definite success or definite rollback.
An ambiguous commit error cannot use the definite-rollback branch.
Cancellation and unknown outcomes still require the separate operation-lifecycle model and implementation connection.

### Verification evidence

Invocation `d984582948cc4cd5bf47a7ebdef7b50c` compiled and kernel-checked all nineteen state-import modules with unchanged inputs.
The gate returned `0` in `target/verification/state-import/closure-gate.WbUdF0/`.
The scope used a 2 GiB memory limit, no swap, and one CPU.

The owned-page module contains 31 theorems and 18 executable controls.
Controls cover missing, failed, extra, and reordered callbacks, shared subtrees, repeated occurrences, leaf-only completion, metadata rejection, and commit order.
The nonempty cold-page controls check successful commits, matching-key wrong-kind rejection, and cold failure after successful history commit.
These controls use synthetic hashes for control-flow checks.
The shared-subtree control is not an authenticated physical-read fixture.

Four native traversal properties passed 512 generated cases in 0.41 seconds after compilation.
The two new properties account for 256 cases.
They exercise the real `sequential_export`, radix serialization, and history hashes.
They compare annotated and unannotated reads, failures, page entries, and continuation boundaries.
They also require separate expected-kind paths for three references to the same leaf hash.
Generated budgets include 750 and 1,024.

Invocation `4011b9396b5b4e2da9f51a7da6d6088f` records the native results in `target/verification/state-import/located-native.vEKrp4/`.
Strict Casper library/test Clippy, workspace formatting, and unchanged-input checks passed.
The scope used a 5 GiB memory limit, no swap, and one CPU.
These properties qualify exporter annotations, not the pending production preparation or commit wrapper.

The authorized plan review found and corrected the existential-annotation gap and cold-key validation order.
The review accepted the resulting semantics with the transaction-receipt and unknown-outcome limits stated above.
Complete-root consumption, publication authority, lifecycle composition, production repair, and native importer conformance remain required.

## Read-only complete-root scans

[StateImportScan.v](../../../../formal/rocq/cost_accounted_rho/theories/StateImportScan.v) connects actual fallible storage observations to the complete-root consuming scan.
This module closes a proof gap between accepted pages and root publication.
Page acceptance proves only the emitted occurrences.
A complete-root scan must also check every reachable reference that later execution can use.

### Why a read-only proof is necessary

The earlier cold-store theorems establish typed reads after a guarded commit or preserve an already resolved leaf.
Neither theorem establishes a newly observed leaf during a read-only scan.
Requiring a write transaction merely to validate stored data would add an unnecessary operation.
Assuming a shared read snapshot would exclude actual interleavings between the two cold key formats.

The new proof treats each cold observation according to its result.
Observed bytes remain unchanged under guarded writes.
An observed absence can remain absent or become part of a strictly resolved leaf after compatible writes.
Unread and failed observations cannot satisfy either required alias-read guard.

`import_observed_strict_leaf_establishes_a_current_binding` derives the current leaf from both actual observations and the intervening guarded storage run.
The theorem does not equate previously absent observations with current storage.
An observed byte record identifies the same leaf after a compatible insertion.
Both recorded absences still produce no successful read, even when later writes insert valid data.

### Physical reference checks

`import_check_physical_reference` checks the reference selected from the frontier.
The history branch validates the key, exact hash, radix encoding, and contextual children.
The cold branch requires both alias observations, strict resolution, the existing payload hash, expected kind, and successful typed item decoding.
The function returns no children on any required failed check.

`import_actual_physical_reference_check_refines_consuming_reads` proves correspondence with the existing structural and typed consuming reader.
The history proof derives current bytes from actual recorded reads and byte-preserving writes.
The cold proof uses the new read-only binding theorem before checking typed consumption.
The model does not treat a present root tag or a successful export as evidence of complete stored state.

### Executable traversal and closure

The scan captures the original root once through `import_begin_physical_scan`.
Its initial frontier contains that root with an empty occurrence context.
It does not read the mutable current-root pointer when selecting later work.

Each successful episode adds the checked reference and its exact children.
Frontier filtering compares complete references.
History identity includes the occurrence context.
Cold identity includes the expected kind.
Thus, a shared hash cannot hide a different context or a wrong-kind descendant.

`import_execute_scan_episodes` consumes a finite sequence of physical read episodes.
A required failure retains the unresolved frontier and sets a persistent failure flag for that attempt.
Later valid observations cannot clear that flag.
An exhausted episode sequence with pending work does not report completion.

`import_complete_physical_scan_establishes_original_root_closure` derives consumable closure from the actual successful execution and empty frontier.
It does not assume closure as an input.
The theorem reuses the existing concurrent consuming-scan invariant instead of introducing a separate reachability rule.

### Concurrent execution boundary

An episode contains independent history and cold observation traces.
The traces include compatible writes through the episode endpoint.
The next episode starts from those endpoints.
This product admits more interleavings than a particular implementation and does not require a physical shared snapshot.

Native correspondence must bind both endpoints to one execution boundary.
The domain read earlier must include intervening writes through that boundary.
Two unrelated stale transaction receipt views do not satisfy this requirement.

Every history writer must preserve existing bytes.
Every cold writer must preserve strict two-alias resolution through the guarded-write contract.
This obligation includes import and checkpoint writers.
Cold byte extension alone remains insufficient.

### Controls and extracted test obligations

The module contains 21 theorems and 15 executable controls.
The nonempty-root example includes an actual physical episode schedule, not only synthetic successful callbacks.
Separate controls construct actual traces with inserts between alias reads.
The examples use synthetic hashes to test control flow, not cryptographic collision resistance.

| Executable invariant | Formal evidence | Required native coverage |
| --- | --- | --- |
| Complete traversal includes typed cold reads. | Nonempty-root schedule and completion controls. | Scan real exported roots and compare all typed reads. |
| Partial traversal cannot publish a complete root. | Partial-history and finite-sequence controls. | Interrupt scans at each frontier boundary. |
| Shared hashes retain context and kind. | Full-reference filtering and contextual-child controls. | Generate shared subtrees and same-hash distinct kinds. |
| Compatible inserts can occur between alias reads. | Actual guarded-write observation traces. | Schedule checkpoint/import writes between real alias lookups. |
| Two absences cannot become a successful recorded read. | Post-observation insertion control. | Insert valid data after both absent lookups. |
| Required read failure remains fatal for the attempt. | Sticky-failure theorem and unread/failure control. | Inject storage errors, then make valid data available. |
| Conflicting aliases cannot establish a binding. | Conflicting-kind alias control. | Generate incompatible encodings under both key formats. |

The native column records required conformance work, not completed test evidence.
Existing exporter properties qualify occurrence annotations but do not qualify this pending production scanner.
Future property and Loom tests must bind these obligations to the actual scanner and writer interfaces.

### Preservation after the scan

`import_completed_scan_remains_consumable_during_publication` preserves the original root's consumable closure after the completed scan.
Its premises require an exact history-byte extension and a guarded cold-storage run.
These premises admit concurrent compatible inserts without a shared snapshot.
They prohibit overwrites that invalidate previously checked references.

Invocation `25dd688c99ab4b94b4b74940996b53c3` compiled and kernel-checked all twenty modules, including this theorem.
The gate returned `0` in `target/verification/state-import/closure-gate.W4EwcC/` with unchanged inputs.
Invocation `8f25f80d116447fdaceadaeb50e0e7e4` checked the added theorem's assumptions in `publication-audit.5VGGSc/`.
The theorem reported a closed global context.
The explicit storage premises remain production obligations.

### Publication operation refinement

`StateImportOperations.tla` refines the publication model through a named module instance.
The model separates request ownership from an authorized storage operation's lifetime.
One operation identifier remains associated with its original request attempt.
Independent issued and settled records make lost execution handles detectable.
Replacing a request cannot reuse or erase an outstanding operation.

`CompatibleHistoryBatch` and `CompatibleColdBatch` extend the base model with atomic, validated content effects.
Both actions can occur during any scan phase.
A physical history row can establish several contextual references in one transaction.
The operation model separates the authorized physical footprint from this derived effect.
History and cold writes remain separate transactions.

The [root store](../../../../rspace++/src/rspace/history/roots_store.rs) writes the root tag before the current-root pointer.
The operation model authorizes that synchronous call once.
Successful tag return permits the pointer transaction.
Tag failure or an unknown tag result ends the call without starting the pointer transaction.
A later checkpoint can change the current-root pointer without invalidating the imported root's immutable content.

The model distinguishes these outcomes:

| State | Physical effect | Permitted next action |
| --- | --- | --- |
| Running | The transaction has not finished. | Commit or abort under the original authorization. |
| Committed, return pending | The write exists. | Return success or an unknown result. |
| Aborted, return pending | This transaction adds no effect. Other writers can have stored compatible content. | Return failure or an unknown result. |
| Terminal unknown | The transaction finished, but its caller cannot identify the outcome. | Retain recovery work and inspect the root through validated reads. |
| Canceled caller | An existing operation can still finish. | Preserve its execution handle and prohibit new authorization after observed cancellation. |

`CancelCaller` denotes cancellation observed at an authority boundary.
It does not denote a preemptive Tokio abort inside synchronous code.
The [runtime requester](../../../../casper/src/rust/engine/runtime_state_requester.rs) holds an exclusive `Core` borrow during response processing.
An external abort request cannot interrupt that invocation or mutate its ownership state.
Native correspondence must place the observed cancellation action at an actual execution boundary.

Completion requires a scan receipt for the original root, an exact successful marker lookup, and current request ownership.
The retirement action does not assume closure in its guard.
The scan invariant establishes closure, and compatible writes preserve it.
The refinement property checks the base completion action after a successful pointer write.
Other successful retirements satisfy the base model's verified-observation action.
The model does not require the current-root pointer to equal the requested root at retirement.

| Invariant | Deliberate unsafe change | Required native regression |
| --- | --- | --- |
| Outstanding operations retain execution handles. | Drop a running operation. | Cancel around a blocked backend call and observe its final effect. |
| Commits match their original authorization. | Commit another batch. | Exchange prepared page identities before commit. |
| Observed cancellation prohibits new authorization. | Start another operation after cancellation. | Cancel at each real authorization boundary. |
| Terminal outcomes cannot execute again. | Commit after a terminal unknown result. | Inject uncertain return after commit, then retry. |
| Pointer execution requires successful tag return. | Start the pointer after tag error. | Inject errors before and after both root writes. |
| Retirement carries current root-bound evidence. | Use only a marker, a stale result, or an unknown result. | Replace requests and repeat successful or failed lookups. |
| Traversal retains full contextual references. | Deduplicate by physical key alone. | Import shared history subtrees through distinct contexts. |
| Checkpoint writes preserve checked references. | Overwrite a checked reference. | Race checkpoint and importer writes through the real guard. |

The original native root-write property passed 128 generated cases.
It checks the actual root store with injected failures before or after either write.
The test preserves the exact committed prefix and verifies a successful retry.
Strict Casper Clippy and formatting also passed in `target/verification/state-import/root-prefix-native.VCE8A4/`.
This test verifies the root-store write contract, not complete import validity.
The current revision enumerates all four failure positions for each generated root.
That revision passed 128 generated roots and 512 fault scenarios in `root-prefix-native.WIyrWp/`.
The scope invocation was `28abe83997224e9a91efbfe0150fe815`.
The final formatted source passed the same test, strict Clippy, and formatting in `root-prefix-native.MxuRwZ/`.
That scope invocation was `dfa7dc8782924640a9174950021f4654`.

The operation model's full qualification remains in progress.
Its bounded configurations and unsafe controls do not prove native storage correspondence by themselves.
`CheckpointSelect` covers already-tagged roots.
The checkpoint construction proof below supplies the separate first-publication contract.
Its native implementation correspondence remains incomplete.
Final-owner release can abandon an incomplete transfer and is distinct from successful retirement.
Unrestricted failures can prevent completion, so the model makes no unconditional liveness claim.

### Qualified proof evidence

Invocation `2ea4145d59e84573aa3de93438718b16` compiled and kernel-checked all twenty state-import modules with unchanged inputs.
The gate returned `0` in `target/verification/state-import/closure-gate.8uU4zJ/`.
The scope used a 2 GiB memory limit, no swap, and one CPU.

Invocation `6955948020fb4dceba2a01ae0bf55269` checked all twenty new theorem assumptions in `target/verification/state-import/scan-audit.nVb26x/`.
Every theorem reported a closed global context.
Explicit theorem premises still apply, including hash-output width and guarded storage transitions.
The assumption audit does not discharge those implementation premises.
The scope used a 1 GiB memory limit, no swap, and one CPU.

The authorized plan review found no blocking soundness defect.
The review identified the correspondence limits below, which remain implementation and verification obligations.

### Scope limits

The closure theorem proves safety upon successful completion.
It does not prove eventual completion for arbitrary graphs or bounded total scan memory.
Physical byte vectors and the production hash must discharge the model's byte-domain and hash-width premises.
The episode model permits extra reads and therefore does not establish a per-episode read-footprint bound.

### Production-size history boundaries

Two native regressions now exercise exact production-sized history boundaries through the real radix history and exporter.
The fixtures use ordinary joins inserts with deterministic channel names.
The fixtures verify their actual history-node counts before testing the continuation.
They also check typed joins reads and compare the complete exported cold maps.

| History budget | Actual history nodes | Channel names | Complete cold values | Cold-only tail | Exported cold values before repair |
| --- | --- | --- | --- | --- | --- |
| 750 | 750 | `exact-history-750-0` through `exact-history-750-8325` | 8,326 | 9 | 8,325 |
| 1,024 | 1,024 | `exact-history-1024-0` through `exact-history-1024-10731` | 10,732 | 2 | 10,731 |

The fixture sizes came from measured radix-node counts, not a model assumption about branching density.
The current tests use these fixed inputs instead of repeating the discovery search.
Both tests still assert the exact history count.
The final two-test run took 7.94 seconds after compilation.

The [export adapter](../../../../rspace++/src/rspace/state/rspace_exporter.rs) removes the final collected record before replacing the final history record with cursor metadata.
When the continuation contains only cold records, there is no history record to replace.
The adapter removes a cold record instead.
The earlier one-value continuation becomes empty and triggers `EmptyHistoryException`.
The larger continuations return successfully but omit one required cold value.
These are two manifestations of the same adapter defect.

Invocation `13555ab50acc4a88943769f97e793037` reproduced both missing-value failures against the final fixed fixtures.
The evidence is `target/verification/state-import/production-boundary-red.LmAC89/`.
The tests returned `101`, as required for a pre-repair regression.
The evidence gate returned `0` only after checking the exact counts, both failures, strict Clippy, formatting, and unchanged inputs.
The scope used a 4 GiB memory limit, no swap, and one CPU.

This is defect evidence, not a passing implementation result.
The production adapter remains unchanged.
After repair, both tests must pass with complete map equality.
The cursor and occurrence models already require retention of every cold occurrence, including cold-only terminal pages.
These native tests provide the missing production-budget instances of that obligation.

The failure flag preserves safety but does not specify native error categories.
Cancellation, uncertain writes, retry scheduling, root-tag writes, current-root writes, and publication authority remain separate required connections.
No production importer or Casper consensus rule changes form part of this proof increment.

## Checkpoint construction and first publication

A checkpoint creates a new RSpace history root from existing state and changed channel values.
An import instead receives state records and must establish their validity before publication.
Both paths write the same history and cold stores.
Their storage guarantees must therefore compose under concurrent execution.

The new [checkpoint proof](../../../../formal/rocq/cost_accounted_rho/theories/StateImportCheckpoint.v) derives complete, typed-readable state from construction evidence.
Construction evidence is proof state, not a new block field, wire certificate, or Casper voting rule.
The proof does not require a fresh scan of every unchanged subtree for each checkpoint.
It requires the native construction path to preserve the contextual references that justify subtree reuse.

### Contextual construction contract

A history reference contains both its physical key and its absolute traversal context.
A cold reference contains its physical key and expected leaf kind.
The expected kind distinguishes Data, Continuations, and Joins payloads.
Equal physical history keys do not authorize reuse under different contexts.

Let $`B`$ be the captured readable base roots and $`D`$ the constructed references.
Let $`S_0`$ be the old decoded store and $`S_1`$ the committed decoded store.
The covered references are:

```math
\operatorname{Covered}(S_0,B,D)
= D \cup \bigcup_{b \in B}\operatorname{Reach}(S_0,b).
```

Each constructed history reference must have a successful checked read in $`S_1`$.
Every child returned by that read must already belong to the covered references.
Each constructed cold reference must have both a successful checked read and successful typed consumption.
The construction proceeds from children to parents.
The target root must belong to the final covered references.

Under these conditions, the proof derives readable closure for the target root.
It does not assume that closure as an admission condition.
The proof also requires compatible writes to preserve previously resolved bindings.
These premises preserve both the new root and captured old roots during later independent writes.

The physical theorem uses history-byte extensions and guarded encoded-cold operations.
It derives the decoded binding extensions from those physical relations.
It does not equate physical history transactions with an assumed decoded transaction sequence.
The theorem requires a valid 32-byte target and proves its exact byte-to-number-to-byte roundtrip.
Prefix-split and prefix-compaction lemmas preserve the complete absolute reference, including its expected leaf kind.

The construction list need not contain every stored row.
A failed earlier checkpoint can leave compatible, unreachable rows.
Those rows do not establish target closure and need not disappear for a later construction to be valid.

### First-root initialization

The no-base theorem permits construction with $`B = \varnothing`$.
Its executable example starts with an empty old store and derives typed-readable closure for a newly constructed root.
This theorem does not authorize an arbitrary missing root to act as empty state.

The native fresh-repository regression exposes a separate initialization defect.
The current root repository records the canonical empty-root marker and current-root pointer.
The history constructor supplies an in-memory empty node but does not persist the corresponding empty history record.
Consequently, a root marker alone does not imply that the exporter can read the root.

A second regression records a missing nonempty root before repository construction.
The current constructor accepts that root instead of reporting the missing history record.
The repair must distinguish canonical-empty construction from invalid nonempty-root lookup.
The model's no-base configuration does not replace either native regression.

### Checkpoint publication and cancellation

The [checkpoint TLA+ companion](../../../../formal/tlaplus/state_import/StateImportCheckpointPublication.tla) composes independent checkpoint calls with the existing import operations.
Cold writes precede history writes.
Construction evidence precedes root-tag publication.
A successful tag return precedes current-root pointer execution.
Success returns a repository for the exact authorized target.

`CpStart` authorizes the whole synchronous checkpoint call.
Cancellation can suppress result delivery without stopping an already executing synchronous call.
Therefore, cancellation does not forbid that call's remaining internal writes.
`CpSeal` completes construction evidence and is not a new caller-authorization boundary.
This distinction preserves the source behavior without adding an unsupported cooperative cancellation check.

An uncertain write result ends the call without claiming successful repository delivery.
The write can already have committed.
The model records committed effects separately from the reported result.
Per-stage execution counts detect repeated writes after terminal settlement, even when the repeated write uses the same root.

| Invariant | Deliberate unsafe change |
| --- | --- |
| `CpCapturedBaseIsLocal` | Capture the mutable current-root pointer instead of the call's local base. |
| `CpConstructionCut` | Construct a parent without its required child coverage. |
| `CpCertificateCoversRoot` | Seal construction before the target reference is covered. |
| `CpPublishedRootAuthorized` | Publish another root under the call's construction evidence. |
| `CpPointerRequiresTagSuccess` | Execute the pointer write after an unsuccessful tag return. |
| `CpOutstandingWorkOwned` | Drop the execution handle while the call remains unsettled. |
| `CpReportedRequiresSuccess` | Report success after an uncertain pointer return. |
| `CpCancelledResultSuppressed` | Deliver a repository after cancellation. |
| `CpTerminalCannotExecute` | Repeat a pointer write after terminal settlement. |

All nine controls produced their exact required invariant failure.
The checker also verified unchanged source inputs.
The scope invocation was `4de685779dc147d2915caf8e1e1c2e5c`.
The main safe configuration subsequently passed 7,106,400 distinct states in `checkpoint-publication.JhEVrx`.
Its source-bound gate returned `0` at invocation `327a010c05464a61a1e1c8cd65ec35c5`.
The shared, replacement, and fresh configurations remain under evaluation.
Negative controls show that the invariants detect these defects, not that all safe configurations have passed.

The companion adds first-publication actions that the earlier operation specification did not contain.
It therefore checks the inherited invariants but does not claim refinement to that earlier action relation.
Its finite configurations do not prove unbounded liveness, arbitrary actor counts, or storage-retention bounds.

### Native regression evidence

The [native tests](../../../../casper/src/rust/engine/horizon_state_import_tests.rs) use the actual history repository and exporters with isolated in-memory stores.
They exercise real codecs, hashes, radix updates, typed reads, and export paths.
They are not substitutes for durable-backend fault and concurrency tests.

| Native test | Observed result before repair | Required behavior |
| --- | --- | --- |
| `state_import_checkpoint_pristine_root_has_exportable_history` | Fails because the physical history record is absent. | The initialized canonical empty root has exportable history. |
| `state_import_checkpoint_missing_nonempty_base_is_rejected` | Fails because construction accepts a missing nonempty root. | Missing nonempty history produces an error. |
| `state_import_checkpoint_reset_missing_root_preserves_selection` | Fails because reset selects missing history and reports success. | A failed target load does not authorize that call's pointer write. |
| `state_import_checkpoint_constructor_rejects_invalid_root_rows_without_panicking` | Fails on wrong hashes, malformed nodes, and invalid key widths. | Invalid history returns an error without a panic or store mutation. |
| `state_import_checkpoint_reset_rejects_invalid_root_rows_without_selection` | Fails on the same cases and changes the selected root. | Invalid history cannot authorize selection. |
| `state_import_checkpoint_first_nonempty_root_consumes_exact_joins` | Passes. | The first ordinary checkpoint stores and exports the exact typed Joins value. |
| `state_import_exporter_standalone_cold_matches_combined` | Fails because standalone cold export returns an empty map. | Standalone and combined export return the same complete cold map and cursor. |
| `checkpoint_generated_grafts_preserve_typed_values_and_base_reads` | Passes 64 generated cases. | Typed and binary Joins updates preserve exact current values and all captured old-root reads. |

The standalone exporter defect is a record-partition error in `get_data`.
That method selects non-leaf records instead of cold leaf records.
This defect differs from the exact-page-boundary loss in the combined exporter adapter.
Historical CI causation is not established merely by these local reproductions.

Each generated checkpoint case starts with 16 distinct Joins channels.
It then applies between one and 24 generated insert, replace, or delete actions.
The test covers typed and correctly encoded binary actions.
It compares exact exported locations and values after each change.
It also checks old physical bindings and typed reads at every retained root.
This property does not cover every Data or Continuations codec, malformed binary action, or concurrent backend schedule.

The subsequent all-kind property adds Data and Continuations to the typed/binary correspondence checks.
Each case initializes eight channel identities in all three leaf domains.
The case then applies between one and 24 generated updates or deletions.
Two isolated repositories receive equivalent typed and binary operations.
The binary operations deliberately reverse entry order before insertion.
Both repositories must produce identical roots, physical records, and exact exported occurrences.

The all-kind property reads every retained root through both typed and binary reader methods.
It compares serialized typed values, not only trace-event equality or successful decoding.
Each generated value batch contains one to four entries, with persistent and nonpersistent Data and Continuations values.
The fixtures use the actual channel-key hashing rules for each leaf domain.
They preserve three distinct namespace prefixes even when channel identities overlap.

Invocation `25962ed56882480996c5c556dcbb5b13` passed both checkpoint properties in `target/verification/state-import/checkpoint-all-kinds.IulyEE/`.
Each property ran 64 cases, for 128 generated cases in total.
The tests completed in 9.26 seconds after compilation.
Strict Casper Clippy, formatting, documentation syntax, whitespace, and source-hash checks also passed.
The scope used a 2 GiB memory limit, no swap, and one CPU.
These tests do not establish malformed-input rejection, every generic production type, or durable concurrent-writer correspondence.

Invocation `d59d1b5167634849a99362bff70ec370` recorded these native results in `target/verification/state-import/checkpoint-native.cY2NgR/`.
The checkpoint group and standalone export test returned `101` because they reproduced the defects.
The property, strict Casper Clippy, formatting, and source-hash checks passed.
Evidence-gate exit `0` means the expected defect evidence matched, not that the production implementation passed.
The scope used a 4 GiB memory limit, no swap, and one CPU.

### Qualified proof and remaining correspondence

Invocation `da062b032e894f728e8f259599888649` compiled and kernel-checked all 21 state-import modules.
The gate returned `0` with unchanged inputs in `target/verification/state-import/closure-gate.p3qLL5/`.
Invocation `4a5c0432853548129a779c5f93ec1455` checked all ten checkpoint theorem assumptions in `checkpoint-assumptions.pCpAC9/`.
Every theorem reported a closed global context.
Explicit theorem premises still apply.

The independent plan review found no new soundness defect in the strengthened proof and model.
The following implementation connections remain required:

1. Derive construction evidence from actual radix update paths, including cached nodes, binary actions, splits, and compaction.
2. Make checkpoint and importer writers satisfy the guarded physical-storage relations.
3. Connect the formal hash, codec, and typed-decoder parameters to production behavior.
4. Repair canonical-empty initialization without accepting missing nonempty roots.
5. Pass the corresponding native properties, durable fault tests, and concurrent interleavings.

The current checkpoint writes cold records before processing history actions.
History processing can fail after compatible cold records commit.
Thus, checkpoint failure does not imply that no physical store effect occurred.
Required safety is preservation of existing readable roots and rejection of incomplete new-root publication.
This contract remains distinct from import-page validation before content writes.

No production checkpoint or importer repair is claimed by this formal increment.

## Root loading before root selection

The native reset regression first creates a readable checkpoint.
It then records a different root marker without the corresponding history row.
Reset reports success and selects that missing root.
Evidence `checkpoint-reset-red.BcXE1W` reproduces this behavior at invocation `52f644b7912146e788f9c0b2874baaa5`.
The expected native exit is `101`, not a successful implementation result.
Strict Casper Clippy, formatting, and unchanged-input checks passed for that regression.

`HistoryRepositoryImpl::reset` currently selects the root before loading its history.
`RadixHistory::reset` currently accepts missing history through its empty-node fallback.
Both behaviors require correction.
A strict loader alone would report failure after the pointer had already changed.
The repair must load and validate the target before that invocation selects it.
It must retain the existing lock order and avoid a new global consensus lock.

The additional constructor and reset regressions exercise wrong hashes, malformed node bytes, and invalid root-key widths.
Each case uses the actual repository constructor or reset method with isolated stores.
Wrong-hash and invalid-width rows currently pass loading.
Malformed node bytes panic in the current radix decoder.
Reset selects each invalid target before the load returns or panics.
The required correction must preserve original-byte hash validation and return a recoverable error for malformed input.

The [root-load checker](../../../../scripts/check-state-import-root-load-regressions.sh) defaults to requiring all six checkpoint tests and five codec tests to pass.
Its explicit `red` mode instead requires the five demonstrated failures and their exact diagnostic patterns.
An expected-failure evidence gate is not a successful production qualification.
Evidence `root-load.oxOXEX` records all five required failures and the passing ordinary-checkpoint control.
Invocation `5c8d9681d7eb4ad5a14437a848c6317d` passed strict Casper Clippy, workspace formatting, and unchanged-input checks.

The [root-selection model](../../../../formal/tlaplus/state_import/StateImportRootSelection.tla) separates loading, selection, and the reported result.
It includes two callers, three root identities, concurrent insertion, and concurrent marker publication.
Missing, malformed, wrong-hash, and failed reads cannot authorize a pointer write.
Selection requires both a successful load and a marker.
An uncertain selection result does not imply that selection failed to commit.

The model counts writes per invocation.
Another invocation can legitimately change the shared pointer while one call fails.
Thus, the invariant forbids an unauthorized write by the failed call, not every concurrent pointer change.
The initial selected root has valid history, and concurrent writes preserve existing rows.
The model does not establish descendant closure or correctness of the concrete decoder.
Those guarantees belong to the consuming-closure and codec proofs.

The updated safe configuration checked 29,340 distinct states in `root-selection.zyUQ4z`.
Both unsafe configurations produced their required ordering violations in `root-selection.OYyqqG` and `root-selection.I8nVhh`.
The checker verified unchanged inputs for these results.
This bounded result does not prove unlimited concurrency or eventual selection under permanent storage failure.

## Finite retries for shared cold storage

Cold values have raw and legacy physical key encodings.
An importer and a checkpoint can therefore inspect two physical locations for one logical value.
These two locations do not represent a limit on wallets or funding sources.
Both writer families must preserve existing encoded bytes and reject incompatible logical values.

For valid 32-byte identities, the raw key contains exactly those 32 bytes.
The legacy key adds the eight-byte little-endian vector length before those same bytes.
The proof derives injectivity within each format and disjointness between the two formats.
The generated native encoding property uses actual bincode serialization, repeated identities, and common 31-byte prefixes.
Unvalidated variable-width `Blake2b256Hash` values do not satisfy this width premise.

The [retry proof](../../../../formal/rocq/cost_accounted_rho/theories/StateImportRetry.v) uses the existing encoded-write and physical-observation relations.
It does not assume one shared read snapshot.
Compatible concurrent writes can occur between separate alias reads and before transaction entry.
Every present byte sequence remains unchanged throughout these writes.

Let $`L`$ contain the distinct physical locations guarded by one batch.
Let $`O_0`$ contain the first successful physical observations.
Define the initial absence set as:

```math
A_0 = \{\ell \in L \mid O_0(\ell)=\operatorname{Absent}\}.
```

A definite compare-and-swap conflict means that an expected physical value differs at transaction entry.
The transaction rolls back all its staged writes.
Every later attempt must collect fresh observations after that rollback.
Unread locations, failed reads, incompatible data, and unknown commit outcomes do not qualify for this retry rule.

An observed present binding cannot cause a mismatch under exact-byte preservation.
Therefore, each conflict identifies a previously observed absence that became present.
Fresh observations must retain that new binding.
Each conflict consumes a different member of $`A_0`$.
If $`N`$ is the number of distinct logical keys, the bounds are:

```math
\operatorname{Conflicts} \le |A_0| \le |L| \le 2N,
\qquad
\operatorname{Attempts} \le |A_0| + 1.
```

The final-conflict theorem does not require the next preflight to succeed.
It charges that conflict at transaction entry, before any subsequent read failure.
The attempt bound requires every non-conflict outcome to terminate this local loop.
It does not bound transaction duration, lock waiting, scheduler delay, or higher-level recovery attempts.

### Guard normalization and its counterexample

The batch must contain exactly one compare-and-swap guard per physical location.
The native transaction engines evaluate later operations against earlier staged writes.
Two guards expecting absence at the same location can therefore conflict with their own staged insertion.
Rollback then permits the same failure again without external progress.
The retry proof explicitly excludes this case through guard normalization.

Normalization retains every required physical guard and removes repeated locations.
It does not discard conflicting incoming values.
Incoming rows must pass hash, codec, leaf-kind, and compatibility validation before normalization can authorize a batch.
Existing compatible outer encodings remain unchanged, including permitted trailing bytes.

The sequential transaction theorem proves that a normalized CAS failure requires a mismatch in the transaction-entry store.
This theorem connects the retry argument to staged transaction semantics.
Executable controls demonstrate failure of the premises under duplicate guards, stale observations, and replacement of existing bytes.

### Native obligations and invariant coverage

| Formal guarantee | Corresponding test obligation |
| --- | --- |
| Physical locations bound the initial absence budget. | Generate duplicate logical keys and normalize both physical aliases. |
| Valid raw and legacy encodings identify different physical locations. | Check actual 32-byte and 40-byte keys, roundtrip decoding, shared prefixes, and duplicate normalization. |
| Compatible writes never increase absence or replace observed bytes. | Insert aliases between individual reads and compare each later observation. |
| Each conflict consumes a distinct initial absence. | Force one new alias insertion before each generated transaction attempt. |
| Definite failure rolls back the complete batch. | Compare the entire store before and after each failed transaction. |
| Duplicate guards invalidate the external-progress argument. | Reproduce repeated self-conflicts in LMDB and the production sparse transaction engine. |
| Fresh split reads suffice under concurrency. | Explore competing alias insertions with Loom over the production sparse transaction function. |
| A final conflict remains bounded if refresh fails. | Count the conflict before attempting another preflight. |
| Unknown outcomes and non-conflict errors do not authorize a retry. | Verify the native helper's error classification before integrating that helper. |

The focused [regression checker](../../../../scripts/check-state-import-retry-regressions.sh) runs the LMDB control, two generated properties, and two Loom tests.
It also checks strict Clippy results, formatting, and unchanged source inputs.
These checks qualify transaction primitives and their proposed usage, not an already repaired importer or checkpoint writer.
Native writer integration and complete error-classification tests remain required.

The final 22-module proof gate passed in `closure-gate.iNoj17` at invocation `4b0b088c15654fb6b3e4c56302d0b945`.
Compilation, kernel checks, and unchanged proof-input checks passed.
The same scope passed `retry-regressions.qCGuBv`, including the actual physical-key encoding property.
Both generated properties passed 128 cases each, for 256 cases total.
The LMDB self-conflict control and both Loom tests passed.
Strict Clippy, formatting, and unchanged-input checks passed.
The independent plan review accepted the revised proof boundaries and retained the native integration obligations.

The first native run, `retry-regressions.qiFkVC`, also passed but did not yet include the physical-key encoding property.
The final proof supersedes `closure-gate.e3QN8b` by adding physical-key and initial-observation correspondence.
