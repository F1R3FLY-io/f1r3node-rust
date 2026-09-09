---
task_id: pr216-state-import-formal
status: in_progress
handoff_status: active
next_steps:
  - Complete the active concurrent operation and checkpoint model checks.
  - Apply the reviewed production repair after the remaining formal checks.
  - Qualify importer, backend, property, and concurrency conformance.
---

# State-import ownership proof progress

The [permanent record](../casper/theory/finalized-floor/state-import-validity.md#original-root-ownership-and-validated-page-commits) describes the implementation boundary and proof evidence.
The current pgmcp leaf remains incomplete.
No production importer change or consensus-rule change belongs to this increment.

The formal collector now derives actual occurrence paths before content writes.
One parameterized traversal preserves the earlier entry-only evaluator.
Preparation checks the returned locations, cold rows, and response metadata before either transaction.
The commit model retains the old owner after failure and advances only after both transactions succeed.
Actual physical history and cold read traces establish the post-commit consuming reads.
Explicit storage extensions preserve those results during compatible concurrent writes.

All nineteen import modules passed compilation and kernel checking under a 2 GiB memory limit with no swap.
Four native traversal properties passed 512 generated cases under a 5 GiB memory limit with no swap.
Strict Clippy, workspace formatting, and source-hash checks passed for the native changes.
All heavy commands used systemd resource limits.
Temporary artifacts remained under `target/verification/state-import/`, not `/tmp`.

The owned-page proof covers emitted cold occurrences, not complete-root closure or publication authority.
Transaction receipt views do not establish a common live-storage snapshot.
The Boolean transaction model does not represent ambiguous outcomes.
Production integration must preserve these distinctions.

## Complete-root scan connection

`StateImportScan.v` now derives complete-root closure from actual fallible physical reads and a finite executable scan.
The read-only cold theorem permits compatible insertions between alias reads without a write transaction or shared snapshot.
The scan validates expected kinds and typed values, retains complete reference identity, and preserves failed attempts.
The independent plan review found no blocking soundness defect.

The review requires both episode endpoints to represent one execution boundary, including intervening writes through that boundary.
It also requires both production writer families to preserve the modeled bindings.
The model does not prove a per-episode memory bound or unconditional traversal termination.

The module now contains 21 theorems and 15 executable controls.
The controls include actual physical schedules and guarded inserts between alias reads.
The permanent record now lists the corresponding native property and concurrency test obligations.
Those native scanner obligations remain incomplete.

The next connection remains the publication lifecycle, including checkpoint content writes, cancellation, and uncertain outcomes.
Production repair still awaits that connection and its required checks.

The final twenty-module gate passed in `closure-gate.8uU4zJ` with unchanged proof inputs.
Its invocation was `2ea4145d59e84573aa3de93438718b16`.
All twenty new theorem assumption checks passed in `scan-audit.nVb26x`.
Documentation syntax, shell syntax, and repository whitespace checks also passed in that audit.
No new native scanner result is claimed.

## Publication operation connection

The new scan-preservation theorem passed the twenty-module gate in `closure-gate.W4EwcC`.
Invocation `25dd688c99ab4b94b4b74940996b53c3` completed compilation, kernel checking, and source-hash checks.
The added theorem also passed its closed-context assumption audit in `publication-audit.5VGGSc`.

`StateImportPublication.tla` now permits atomic compatible history and cold batches during every phase.
`StateImportOperations.tla` adds separate operation identities, retained execution handles, scan receipts, exact marker outcomes, and terminal transaction results.
The new checker binds every imported module and selected configuration by hash.
Ten unsafe controls each require their specific invariant failure.

The plan review required source-specific corrections before qualification.
Importer effects now exclude unrelated root rows.
The shared-history fixture includes an intermediate Data-prefix node and preserves one encoded cold key across both contexts.
Observed cancellation remains distinct from an external Tokio abort request.
The model preserves synchronous tag-before-pointer behavior without adding a wire generation or consensus rule.

Four sequence witnesses cover owned history, cold, and root-publication writes.
They include cancellation followed by authorized completion and both committed-but-unknown root-write outcomes.
Each witness checks all invariants, unrestricted specification inclusion, base refinement, and its selected completion condition.
They do not replace unrestricted concurrent checks.

The root-write property now checks all four failure positions for each of 128 generated roots.
All 512 fault scenarios passed against the actual root store.
Strict Casper Clippy, formatting, and input-hash checks passed in `root-prefix-native.MxuRwZ`.
The invocation was `dfa7dc8782924640a9174950021f4654`.
This result does not qualify complete import validity or the pending scanner implementation.

The final source-bound operation checks remain active.
Session `90960` completed the four sequence witnesses, ten unsafe controls, and bounded retry model.
All gates returned `0` with unchanged inputs.
The retry configuration checked 243,052 distinct states.
Session `34755` checks the independent-transfer and replacement configurations.
Session `78943` rechecks the existing publication and ownership models after the batch extension.
Each scope has a 2 GiB memory limit, no swap, and one CPU.

The earlier operation scope `pr216-import-operations-positive-20260908a` stopped before the reviewed fixture correction.
Its partial exploration does not establish completion.
Do not restart any current scope without checking its actual process or session state.

Production repair still requires completed publication qualification and the native correspondence checks.
First publication of new checkpoint roots needs its closure-producing contract.
Two exact production page-boundary regressions now reproduce the exporter defect at sizes 750 and 1024.
They use 8,326 and 10,732 deterministic joins channels respectively.
Each fixture checks actual history counts, typed source reads, and the pure traversal's nonempty cold-only tail.
The production adapter loses one cold value in each case.
The final fixed fixtures complete both tests in 7.94 seconds after compilation.
Their assertions report bounded mismatch diagnostics instead of dumping entire maps.
Evidence `production-boundary-red.LmAC89` records expected test exit `101` and evidence-gate exit `0`.
Invocation `13555ab50acc4a88943769f97e793037` also passed strict Clippy, formatting, and input-hash checks under 4 GiB.
Both tests must pass after the production repair.
No production importer change or git write occurred in this increment.

## Checkpoint construction connection

`StateImportCheckpoint.v` now derives readable new roots from contextual construction evidence.
The physical theorem preserves the exact 32-byte root identity and captured readable roots during compatible independent writes.
The proof does not assume closure of the target root or require a repeated scan of every unchanged subtree.
The no-base example starts from an empty store and constructs a readable root.
Native construction and writer correspondence remain required.

The final 21-module gate passed in `closure-gate.p3qLL5` with unchanged inputs.
Its invocation was `da062b032e894f728e8f259599888649`.
All ten checkpoint theorem assumption checks passed in `checkpoint-assumptions.pCpAC9`.
Its invocation was `4a5c0432853548129a779c5f93ec1455`.
The earlier gate `closure-gate.pLiz57` also passed but predates the exact-root and no-base strengthening.

The native checkpoint evidence is `checkpoint-native.cY2NgR`, invocation `d59d1b5167634849a99362bff70ec370`.
It reproduces missing canonical-empty history, acceptance of missing nonempty history, and incorrect standalone cold-export selection.
The ordinary first checkpoint test passed.
The generated typed and binary Joins property passed 64 cases, including exact values and retained-root reads.
Strict Casper Clippy, formatting, and source-hash checks passed.
The expected failures remain production repair obligations, not green implementation results.

The checkpoint companion now counts each physical write by stage.
The plan review showed that a set of stage names could not detect repeated terminal writes.
A late-pointer control now demonstrates that the strengthened invariant detects repetition.
A separate canceled-result control checks that cancellation suppresses repository delivery.
The model keeps whole-call authorization for synchronous checkpoint execution.

The first companion attempt, `checkpoint-publication.owQKAP`, failed to parse a mixed intersection and difference expression.
Explicit parentheses corrected that syntax error.
The corrected nine-control scope completed with every expected invariant failure and unchanged inputs.
Its invocation was `4de685779dc147d2915caf8e1e1c2e5c`.
Session `8828` is terminal and must not be restarted.

The safe checkpoint configurations run in session `28091`.
Its scope is `pr216-checkpoint-safe-20260908a`, invocation `327a010c05464a61a1e1c8cd65ec35c5`.
The first evidence directory is `checkpoint-publication.JhEVrx`.
Sessions `34755` and `78943` remain active for earlier operation and ownership checks.
All three active scopes have a 2 GiB memory limit, no swap, and one CPU each.

Scheduler revision `643` retained state-import formal work as the first executable leaf.
Claim `22` expires at `2026-09-08T15:16:27Z`.
The permanent publication record now distinguishes proof guarantees, native defects, and remaining implementation obligations.
No production importer repair or git write occurred.

## All-kind native checkpoint correspondence

The new property covers Data, Continuations, and Joins through equivalent typed and binary action paths.
It compares exact roots, physical records, exported occurrences, and typed and binary reads at every retained root.
The binary path reverses entry order to check canonical sorting.
Fixtures use the actual per-domain channel-key hashing rules.

Both checkpoint properties passed in `checkpoint-all-kinds.IulyEE` at invocation `25962ed56882480996c5c556dcbb5b13`.
Each property ran 64 cases, for 128 cases total.
The two tests took 9.26 seconds after compilation.
Strict Casper Clippy, formatting, documentation syntax, whitespace, and input-hash checks passed.
The scope used a 2 GiB memory limit, no swap, and one CPU.
Session `64281` is terminal with gate exit `0`.

The new test does not qualify malformed binary values or durable concurrent writer guards.
Those obligations remain part of the shared importer/checkpoint implementation connection.

## Finite retry and physical alias correspondence

The preceding status turn did not change implementation state.
This continuation added `StateImportRetry.v`, its gate integration, native properties, and concurrent transaction controls.
The work remains under `pr216-state-import-formal`.
Claim 23 expires at `2026-09-08T16:10:28Z`.

The new proof derives retry progress from fresh physical observations and immutable present bytes.
It permits independent writes between individual reads.
Each definite conflict consumes a distinct initially absent location.
The final-conflict theorem covers failure of the next preflight.
Sequential transaction staging and normalization establish that a genuine conflict requires an external expectation mismatch.

The independent plan review identified duplicate guards and final-conflict counting as required proof details.
Both details now have explicit theorems and executable controls.
The review then required actual alias-encoding correspondence and an initial physical-trace connection.
Both additions now pass kernel checking.
The final review found no new semantic defect within these stated contracts.
Production error classification and shared writer integration remain required.

Final proof evidence is `closure-gate.iNoj17`.
Final native evidence is `retry-regressions.qCGuBv`.
Both completed under invocation `4b0b088c15654fb6b3e4c56302d0b945` with a 2 GiB cap and no swap.
The 22-module compilation, kernel checks, and source-hash checks passed.
Two native properties passed 128 cases each, including actual bincode alias encodings.
The LMDB duplicate-guard control, two Loom tests, strict Clippy, and formatting also passed.

Earlier `closure-gate.dDSPMx` failed an executable example's function-equality proof.
Earlier `closure-gate.QNbA1o` failed a proof rewrite after excessive simplification.
Neither failure qualified as proof evidence.
The corrected final proof passed all checks.
Earlier `retry-regressions.qiFkVC` passed but predates the actual-encoding property.

The catalog now contains 150 source modules, 61,805 lines, and 2,857 `Qed.` or `Defined.` terms.
These inventory counts do not assert that every campaign gate has passed.
The permanent publication record includes the retry assumptions and formal-to-native test mapping.

## Root-load regression expansion

The missing-target reset regression passed its expected-failure gate in `checkpoint-reset-red.BcXE1W`.
The updated assertion reports bounded booleans instead of complete store snapshots.
The root-selection TLA+ model now includes explicit markers and uncommitted unknown selection outcomes.
Its safe run passed 29,340 distinct states in `root-selection.zyUQ4z`.
Its two unsafe controls produced the required failures in `root-selection.OYyqqG` and `root-selection.I8nVhh`.

New native constructor and reset tests cover wrong hashes, malformed nodes, and invalid key widths.
The initial harness failed compilation because constructor and reset use different error types.
The test now converts both errors to strings inside its panic-capture boundary.
The production methods remain unchanged.
The corrected run in `root-load.oxOXEX` reproduces all five required failures, with the ordinary checkpoint control passing.
Its invocation is `5c8d9681d7eb4ad5a14437a848c6317d`.
Strict Casper Clippy, workspace formatting, and unchanged-input checks passed.
The expected-failure gate returned zero.

## Concurrent model progress

The main checkpoint configuration passed in `checkpoint-publication.JhEVrx`.
It checked 7,106,400 distinct states to depth 46 in 38 minutes and 31 seconds.
Its source-bound gate returned zero.
TLC reported fingerprint collision estimates of `2.9e-5` and `4.2e-6`.
These estimates remain a limitation of the bounded model-check result.

The same scope now checks `StateImportCheckpointShared` in `checkpoint-publication.pOqtV3`.
The operation and ownership scopes remain active in sessions `34755` and `78943`.
Each scope has a 2 GiB cap, no swap, and one CPU.
The focused native scope adds at most another 2 GiB.
No production importer repair, consensus change, or git write occurred in this continuation.

## Final retry assumption audit

The final assumption audit passed in `retry-audit.LodSQy` at invocation `dbb20e5b40a34cf2bfb16e16ab6cb5aa`.
All 17 retry theorems reported a closed global context.
Their explicit physical-observation, width, and storage-extension premises remain mandatory.
Proof-input and documentation-input hashes remained unchanged.
Shell syntax, documentation syntax for 364 files, and repository whitespace checks passed.
The audit used a 1 GiB cap with no swap.

Sessions `34755`, `78943`, and `28091` retain their existing concurrent checks.
Native and assumption-audit sessions from this continuation are terminal.
Do not restart a live check solely because its output is quiet.
The next production step remains the approved shared import repair after the formal prerequisite is qualified.

## Corrected ownership completion and native codec coverage

Session `78943` completed with exit `0` at `2026-09-08T15:17:22Z`.
The corrected ownership Combined model checked 22,456,308 distinct states and 195,081,784 generated states to depth 56.
Its final queue was empty.
The unchanged-input gate returned `0` in `ownership.tS8Hdp` under invocation `286bc97e12b141aa90543ff0a37c3e91`.
The permanent record states its fingerprint-collision estimates and finite-instance limits.
The operation and checkpoint sessions remain active.

The authorized plan agent reviewed the native codec test boundary against `StateImportCodec.v` and the existing loader.
The review required valid record-boundary truncations, original-order hashing, and preservation of unrelated cached or staged data.
The new tests include those distinctions.
They add one deterministic capacity/header test and four generated families against the actual loader.
The existing root-load checker now also qualifies the five codec tests in explicit red or green mode.

The combined native gate runs as session `54932` under invocation `ad82b08dbbd54436aa9089660b8241e1`.
Its evidence directory is `root-load.wNrOuc`.
It uses a 2 GiB memory limit, no swap, and one CPU.
No production repair or git write occurred.

Session `54932` subsequently completed with gate exit `0` and unchanged inputs.
The checkpoint subset reported one passing control and five expected failures.
The codec subset reported two passing tests and three expected failures.
The valid-record property completed 128 generated cases.
Each negative property failed on its first generated case, so no complete negative-property coverage is claimed before repair.
Strict Casper Clippy and workspace formatting passed.
The permanent record now maps the codec invariants to these native tests and reports their limits.

The final documentation and assumption audit passed in `codec-doc-audit.jWNbwo`.
Its invocation is `7efae57728d94f2eacfbcb028cb88761`.
Session `73976` is terminal with gate exit `0`.
The audit checked the unchanged 22-module proof inputs and confirmed closed global contexts for all 17 retry theorems.
Documentation syntax passed for 364 files, with unchanged audited inputs and no whitespace errors.
The audit used a 1 GiB cap, no swap, and one CPU.

The active operation and checkpoint sessions remain `34755` and `28091`.
Each has a 2 GiB cap, no swap, and one CPU.
No model process was stopped or restarted.
The generated Proptest failure seed remains in the source tree for replay.

Read-only pgmcp inspection found no configured local verification recipe for this project.
The runtime and horizon reproduction items remain `claimed_done`, not evidence-verified.
Manual progress records cannot replace the required machine-produced acceptance evidence.
This is an evidence-workflow obligation, not a reason to stop the active formal work.
