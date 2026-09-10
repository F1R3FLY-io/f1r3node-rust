---
task_id: pr216-ledger-startup
status: claimed_done
handoff_status: ready
next_steps:
  - Review the recorded local completion evidence independently.
  - Continue with the scheduler-selected admission and recovery backlog task.
---

# Finalization-ledger repair progress

## Scope

This task bounds local ledger audit and recovery work.
It preserves the existing finalization and effect-ordering contracts.
These changes do not alter votes, finalization thresholds, fork choice, or cost distribution.

The [design document](../casper/theory/finalized-floor/bounded-finalization-ledger-audit.md) specifies the behavior and remaining limits.
Pgmcp progress notes 9129, 9136, 9144, and 9149 record the recovery and validation evidence.

## Source recovery

The ledger source unexpectedly became empty during this session.
The cause remains unconfirmed.
The recovery used commit `559eb07fac98da2e6392e3a84c93bac41558c86c` and 23 subsequent successful patch events.
Every patch matched its context exactly.
Two intervening formatting passes restored the expected formatting before later patches.

The recovered candidate contained 139,025 bytes and 3,528 lines.
Its SHA-256 digest was `200680c67b6f84b0f9fd42dfc8404bd6f0e38f545ec542903e67fffaccbd8deb`.
The restored source matched that candidate before further edits.
This process did not reset Git, change references, or discard uncommitted changes.

## Implemented behavior

The integrity scan captures a fixed head and validates a bounded page before releasing the append lock.
Failed scans remain failed.
Restart begins validation at genesis.

Effect selection captures the projected prefix.
Direct effect entry, receipt writes, and completion writes enforce projection readiness.
This prevents an append between projection and selection from exposing an unprojected round to effects.

Receipt compaction retains one round manifest and one bounded key page.
The iterator includes every effect kind for every finalized block.
It deletes the completion marker last.
The durable compaction cursor advances only after deletion succeeds.

Concurrent compactors can repeat deletion without changing logical completion.
A failed scan requires a fresh scan from the durable cursor.
Casper now awaits asynchronous compaction between pages.
The asynchronous adapter schedules at most one blocking page for each caller at a time.

Effects-cursor advancement now checks at most 32 completion markers per default page.
Each page starts from current durable progress and stops at a completion gap or the captured projected target.
The completion marker precedes cursor advancement.
Startup resumes durable completion markers before compaction.
Cancellation schedules no further page from that caller.

Ledger point reads now borrow the stored bytes without a payload copy.
The decoder checks the expected row variant, field lengths, collection counts, and complete layout before deserialization.
The serializer remains unchanged.
Recovery charge strings retain their existing accepted range.
The maximum-witness test confirms the derived 22,102,208-byte encoding bound.

Recovery enumeration now visits borrowed rows in one backend snapshot.
It validates every raw key and value layout before selection.
It retains only recovery charges and usage rows.
The admission visitor validates complete targets and citers but retains only compact identity summaries for reconciliation.
Validation-only startup counts admissions without retaining those summaries.

In-memory transactions now stage changed keys without copying unrelated store entries.
Repeated operations inspect the latest staged value, including deletion.
The existing coordinator guard covers staging and publication.
The transaction publishes no staged writes when any operation fails.

## Verification evidence

Evidence directories below are relative to `target/verification/pr216/ledger-startup/`.
The source files and this work log are permanent records.
Generated logs can be removed after their required evidence is archived.

| Evidence | Result |
| --- | --- |
| `rust-recovered.awfbKm` | All 45 ledger tests and strict block-storage Clippy passed after source recovery. |
| `rust-compaction.29xncC` | All 50 ledger tests and strict block-storage Clippy passed after paged compaction. |
| `verification.ZnEt6K` | Apalache passed the audit model through length 12 and five unsafe controls. Recovery then failed type inference. |
| `verification.flz6a7` | After the type annotation repair, the recovery family passed Rocq, TLC, and Apalache. |
| `verification.t8Y9Jk` | The effect-selection family passed Rocq, TLC, and Apalache. |
| `rust-async-compaction.BPifa6` | 51 tests passed, including cancellation. One new test incorrectly expected compaction of an uninitialized ledger to succeed. |
| `verification.7TUiwJ` | Rocq proved five effects-cursor page properties. The independent kernel check passed before the production repair. |
| `rust-cursor-pages.0EpJue` | All 57 ledger tests passed in 54.58 seconds. Strict block-storage Clippy and the Casper library check passed. |
| `rust-caller.9RdrJv` | All seven finalization-runner tests and strict Casper Clippy passed. This includes the direct-entry example and generated-prefix property. |
| `effects-induction.6xXW3P` | The initial induction check found a missing captured-target bound in the safety predicate. Its initial state was unreachable. |
| `effects-induction-fixed.BRrS41` | The strengthened effect-selection predicate passed one-step induction within the configured finite domain. |
| `verification.KQAnJt` | The strengthened effect-selection family passed Rocq, TLC, and length-12 Apalache checks, including both unsafe controls. |
| `other-induction.CdRvtQ` | The audit and recovery safety predicates passed one-step induction within their configured finite domains. Source hashes matched. |
| `verification.Lz9ItP` | The updated Apalache gate passed normal reachability, induction, and both unsafe controls for effect selection. Input hashes matched. |
| `verification.oAoGRn` | Decoder span, layout, and collection proofs passed Rocq and independent kernel checking before production changes. |
| `rust-decoder.OM0Xbj` | 24 backend tests passed. One new fixture opened nested same-thread LMDB read transactions and failed. |
| `rust-decoder-complete.RKVTOe` | After the fixture repair, all 25 backend tests, the in-memory guard test, and 62 ledger tests passed. Clippy requested a callback type alias. |
| `decoder-clippy.wwd6wZ` | After the type alias change, strict shared, block-storage, and Casper Clippy passed. Five decoder tests and two borrowed-read tests passed. |
| `verification.JY9n5d` | Five recovery selection theorems and the corrupt-discarded-row example passed Rocq and independent kernel checking before implementation. |
| `borrowed-scan.2oYeJd` | All 28 LMDB tests, three in-memory borrowed-read tests, and 67 ledger tests passed. Strict shared, block-storage, and Casper Clippy passed. Source hashes matched. |
| `verification.dNWan6` | Sparse transaction equivalence, touched-key, size, and rollback proofs passed Rocq and independent kernel checking before implementation. |
| `recovery-memory.GMCvob` | All nine in-memory tests, 67 ledger tests, and ten focused Casper admission tests passed. Strict shared, block-storage, and Casper Clippy passed. Source hashes matched. |
| `rspace-recovery-clippy.log` | Strict RSpace library and test Clippy passed separately, including the new generated transaction tests. |
| `observation-negative.bv4jDH` | The same controlled completion-query race failed at its named assertion in both production backends. Source hashes matched. |
| `observation-production.PFwViJ` | All 71 ledger tests passed with 1,024 cases per generated property. Three production-linked Loom tests and both strict Clippy checks passed. Source hashes matched. |
| `durability.rx1kLa` | All nine LMDB process-crash cases passed. The wrapper miscounted the first line because the test runner prefixed it. Clippy did not run in this attempt. |
| `durability.XzOgx2` | After the wrapper correction, all nine process-crash cases and strict block-storage Clippy passed. Source hashes matched. All temporary LMDB directories were removed. |
| `verification.GYBMLC` | The observation family passed Rocq, independent kernel checking, TLC, Apalache length-12 reachability, one-step induction, and both tool-specific negative controls. Input hashes matched. |
| `page-adapters.jf1CT3` | All 72 native ledger tests and strict block-storage and Casper Clippy passed after the shared recovery and integrity kernel extractions. |
| `integrity-page-loom.t51uPL` | All five production-linked audit Loom tests and strict Clippy passed. Source hashes matched. |
| `recovery-page-loom.jy22bC` | Four cases passed before the long compactor exploration. The run was stopped with user approval, exit 143. No complete target pass is claimed. |
| `verification.QNxozc` | The observer-reinsertion proof initially failed to infer its intermediate state. No verified result is claimed. |
| `verification.bfq4ac` | After the explicit intermediate-state correction, the observer-erasure proofs and independent Rocq kernel check passed. Input hashes matched. |
| `reduced-compaction.pfkK5o` | The experiment wrapper rejected multiple existing Loom library artifacts before compilation. No test ran. |
| `reduced-compaction.w4nxtS` | The wrapper selected the matching release library explicitly. The reduced compactor experiment passed in 0.24 seconds. Source hashes matched. |
| `sparse-kernel.z7Fr0p` | All six production sparse-transaction Loom cases passed. Strict Clippy requested a staging type alias. |
| `sparse-adapters.RY45XK` | Six sparse Loom tests, nine in-memory tests, 72 ledger tests, and ten Casper admission tests passed. Strict Loom, RSpace, block-storage, and Casper Clippy passed. Source hashes matched. |
| `startup-readiness.smGkIM` | Both constructor tests, six existing projection tests, strict block-storage Clippy, and documentation syntax checks passed. Source hashes matched. |
| `transaction-crash.mMkfRP` | All 29 native LMDB tests passed, including nine actual transaction-interruption boundaries. Strict shared Clippy passed. Source hashes matched. |
| `resource-phases.TIrXF6` | Both meter tests passed. Three resource tests passed. The admission fixture and audit allocation expectation failed. |
| `resource-phases.5TdasI` | Four resource tests passed after correcting the audit ownership model. The admission fixture still had inconsistent certificate data. |
| `resource-phases.2DNS1w` | The same four resource tests passed. Admission decoding correctly rejected the fixture's unsorted generation cache. |
| `resource-phases.n4LGSr` | The corrected admission fixture, both meter tests, maximum-witness test, and strict block-storage Clippy passed. Source hashes matched. |
| `resource-phases.dA4aTE` | All five resource tests, both meter tests, maximum-witness test, and strict block-storage Clippy passed in release mode. Source hashes matched. |
| `resource-checkpoint.SgZL22` | All 77 ledger tests and eight projection tests passed against the final formatted snapshot in release mode. Source hashes matched. |
| `recovery-page-loom.oiv60G` | All six permanent recovery-page Loom tests passed in 0.23 seconds after the approved reduction. Strict Clippy and input-hash checks passed. |
| `recovery-gate.EM9XrK` | All six recovery tests passed in 1.08 seconds with the exact finalization-gate flags. Shell syntax, documentation syntax, and input-hash checks passed. |

The concurrency review found a completion-query race after the earlier checks.
The query read an old effects cursor before compaction deleted the requested receipt.
It then returned false although logical completion remained true throughout the query.
Pgmcp note 9170 records the first production reproduction and the formal-first repair plan.
The observation-family formal gate completed in `verification.GYBMLC`.
Rocq, its independent kernel check, and TLC passed before the query repair.
TLC explored 10,078 distinct states with no queued states left.
The production query now uses a shared three-read kernel.
The production tests and strict Clippy checks passed.
The Loom checks used no preemption, iteration, or duration cutoff and did not resume a checkpoint.
Apalache passed both normal reachability and the separate induction check.
Pgmcp note 9172 records the completed checkpoint and its remaining scope.

The separate crash test now covers nine real LMDB process boundaries.
It terminates only its own child processes after each child confirms the requested boundary.
Each reopened ledger preserves the expected committed head, cursors, receipts, and migration records.
The temporary directories are removed after database handles close.
The migration cases surround the production call and do not interrupt individual writes inside the backend transaction.

### Completion-query artifact identities

These hashes identify the verified query artifacts at this checkpoint.
The later crash-test module declaration does not change the query kernel or its property and Loom tests.
Each evidence directory also contains its command logs and input-hash checks.

```text
4b7c0a77e2f542a6d2c3f3f36a0ea5e6d66a1dda4f9626d3b16d962e30fdea90  block-storage/src/rust/finality/finalization_ledger/effect_observation.rs
a12fd5d07d25559fbd7ff55938e3ece5059a74b24a840177e7ab96ba3b2f0811  block-storage/src/rust/finality/finalization_ledger/tests/completion_observation.rs
8e998d3584c22261d64d82eb793d64bd3cd94ec316d8aa70e564af0041400b70  block-storage/src/rust/finality/finalization_ledger/tests/durability.rs
fea0b49b8f3e062b82871c9626b93941a87e513ee9f5bb854ea1f92d62f15306  formal/loom/cost_accounting/tests/loom_production_effect_observation.rs
561b2b20758b51591ec0b562b62e41e1ae92f4368c3a2f7cc13a1e633406da79  formal/rocq/finalized_floor/theories/FinalizationLedgerAudit.v
ece92abc2c45d0369fa6da7db7dff39e28bcba6bc8a7490db01daf146655920e  formal/tlaplus/finalized_floor/FinalizationEffectObservation.tla
```

The last test now distinguishes an uninitialized ledger from a valid genesis-only ledger.
Missing durable cursors remain errors.
The focused rerun passed in `rust-async-focused.wOe9qf`.
Strict block-storage Clippy and the Casper library compile check also passed.
The final source-hash checks passed.
The later 57-test run includes those 51 tests and the repaired fixture.
Source hashes for the cursor-page run also passed after the job completed.
Source hashes for both decoder runs passed.
The callback type alias does not change storage operations or decoder behavior.

The formal gate now accepts an optional family selector.
The default gate still includes all families.
A selected-family result does not establish that omitted checks passed.

## Remaining work

Permanent recovery-page qualification, phase-specific resource measurements, and the complete refinement matrix remain required.
Production-linked audit and sparse-transaction checks now complement the completed query-kernel checks.
The constructor-readiness and actual backend transaction-interruption tests also passed.
The reduced compactor experiment remains separate from the incomplete original recovery-page run.
Resource evidence must distinguish enumeration, retained recovery state, mutation preparation, and metadata projection.
The formal gate now requires the induction step for each selected Apalache family.

The recovery-memory and earlier proof scopes have completed.
The completion-observation and durability scopes have also completed.
Those earlier scopes have completed.
The later recovery-page scope remains active at the checkpoint below.
Rust validation used a 4 GiB memory limit with swap disabled.
The Rocq scopes used a 2 GiB memory limit with swap disabled.
The completed scope no longer exposed its memory peak when queried.
No peak-memory measurement is claimed from that query.
Documentation syntax checks passed for 333 files.
Pgmcp notes 9163, 9164, and 9166 record these implementation steps and their partial verification boundaries.

These results do not establish complete campaign verification or readiness for the required soak test.

## Page and transaction correspondence checkpoint

The shared page kernels now execute the native capture and page algorithms under Loom locks.
The five audit cases and 72 native ledger tests passed.
The original recovery run remains active in session `73562`, scope `run-p484847-i4692502.scope`.
Its compactor scenario adds a read-only observer whose queried revision is already covered by a fixed effects cursor.
This adds schedules but no receipt-read race in that scenario.

The plan agent reviewed observer erasure before the reduced experiment.
Rocq proved trace projection, reinsertion, and invariant preservation before that experiment ran.
The [design document](../casper/theory/finalized-floor/bounded-finalization-ledger-audit.md#compactor-observer-reduction) lists the concrete premises and limits.
The experiment preserves the original source inputs and full two-compactor traversal.
Its result is separate from the incomplete original run.
The original compiled binary's two-cursor case also passed when run separately with its exact test name.

These hashes identify the shared kernels and correspondence artifacts at this checkpoint.

```text
460f1ce719ca57c78418b81a61266758f8b3ec0a98208d06c297c4142dfafb34  block-storage/src/rust/finality/finalization_ledger/recovery_pages.rs
c590e749c1c46be88e6977b3e2b0e5c57002f29f09b4e3b825ded2baf799a111  block-storage/src/rust/finality/finalization_ledger/integrity_pages.rs
96fbe056031ec4869edfffd5f311f58c87ef04dc90580227686b8bbe6c15591f  rspace++/src/rspace/shared/sparse_transaction.rs
9efd7d591c6662c7223b6d46c13ea37e759bfc62715693b3ab828acd29b13a5e  formal/loom/cost_accounting/tests/loom_production_integrity_pages.rs
8a04191e01708acb27c6d4826da27f617c28001e05a8cac8ea6b4fcb0beb71e3  formal/loom/cost_accounting/tests/loom_production_sparse_transaction.rs
168402b42c7b3c66839d81946287ba65cef614e0146cceb08103398d8af6294e  formal/rocq/finalized_floor/theories/FinalizationLedgerAudit.v
```

The sparse transaction kernel extraction preserves the original coordinator and operation semantics.
Six production-linked Loom tests passed, including the named missing-guard controls.
A type alias corrects the only new Clippy finding.
The follow-up native and Loom checks completed in session `25264`, scope `pr216-sparse-adapters-20260906.scope`.
Pgmcp note 9192 records the source freezes, results, and remaining obligations.

The original recovery scope and the completed sparse-adapter scope each have a 4 GiB cap with swap disabled.
No process was stopped, and no Git write occurred.
Temporary compilation and test data remain under the task's `target/verification/` directory, not `/tmp`.

## Constructor and backend boundary checkpoint

The constructor tests exercise errors at six positions across three pages of a 65-round ledger.
They require exact errors, unchanged ledger contents, and no projection publication.
Cancellation during page one or two permits only that active page to finish.
The test waits for store-reference release before corrupting an earlier witness and checking restart rejection.
The existing six projection tests also passed, including successful recovery to the exact durable head.

The LMDB backend test adds Unix test-only checkpoints after seven operations and around the actual transaction commit.
Each pre-commit process interruption leaves both stores unchanged after reopen.
The post-commit interruption preserves the complete new state.
A subsequent writer succeeds after each pre-commit interruption.
A stale compare-and-swap after a completed commit fails without changes.
All nine boundaries and the other 28 LMDB tests passed.
These results do not establish power-loss or disk-controller guarantees.

## Resource qualification checkpoint

The [resource report](../casper/theory/finalized-floor/bounded-finalization-ledger-resources.md) records the measurement method, parameters, phase results, and limits.
The tests use a test-only thread-local allocator probe.
They do not interpret Rust allocation counts as resident-memory measurements or uptime estimates.
All five resource checks passed across the recorded targeted runs.
The later runs did not repeat every earlier successful check.

The admission fixture required a real height-zero genesis separate from its positive-height anchor.
The earlier attempt to remove only the anchor's floor commitment was incorrect because its certificate remained present.
The repaired fixture preserves that commitment and sorts generation entries before signing.
Production validation rules remain unchanged.

The audit expectation confused directly decoded heads with record-derived heads that share hash storage.
It also omitted the distinction between one and two new records in a later page.
The regression now measures the actual `record_head` allocation and checks both sides of page boundaries.
The production page retains its rollback state throughout validation.
No production allocation was removed merely to satisfy the test.

The maximum-witness test measures both decoding and standalone validation.
The existing maximum valid encoding remains 22,102,208 bytes.
Decoding retained 52,324,240 bytes in the measured profile, and dropping that result released the complete allocation.
These measurements do not count mapped pages, C allocations, or another thread's memory.

Metadata tests now include constant and increasing fault tolerance.
The increasing case exercises repeated finalized-metadata propagation instead of only the cached skip path.
This produces more work than ledger paging alone and appears separately in the resource report.
The test changes neither fault-tolerance rules nor production metadata propagation.

The following hashes identify the successful debug-profile resource checkpoint.
The ledger file received only a `rustfmt` closure-layout correction after that checkpoint.
Its resulting hash is recorded separately for the next release-profile run.

```text
9cea594e14ce9fa0e7a6d8ad91aae55fa700ee56c5f666a2b0152b0f2d2fc2ae  block-storage/src/allocation_probe.rs
4f6066af10db14d8fd5464265ad7a6b499899d6ee550c9dfa83468f141d93805  block-storage/src/rust/finality/finalization_ledger.rs (tested debug checkpoint)
19d7f952565f2f2f51c6f80323eb9c7925c789fa810e43eb5b13d7aaf654ad01  block-storage/src/rust/finality/finalization_ledger.rs (format-only follow-up)
2b9ecc54c7acd1c0691a3cd2aaa9a0e72fdbc13df6cb342ec8f2c023bbd87c9d  block-storage/src/rust/finality/finalization_ledger/tests/resource_measurements.rs
77356a2f51ddd32c4c5652e1f47e711ae17290b3c929171268a15639c03cf936  block-storage/src/rust/dag/block_dag_key_value_storage.rs
ea35ce73c5d096bf0c3bd4aa39538b8522b185676cf588d2fa3f49f45de1ff68  block-storage/src/rust/dag/block_dag_key_value_storage/finalization_snapshot_tests/resource_measurements.rs
```

Release qualification completed in session `62608`, scope `pr216-resource-release-20260906.scope`.
Its source manifest and logs are in `resource-phases.dA4aTE`.
All five resource tests, both meter tests, maximum-witness test, and strict Clippy passed in this single run.
The scope had a 4 GiB cap with swap disabled.
The original recovery Loom scope remains active, with its three source inputs unchanged.
This checkpoint does not count that active job as passed.

The final native checkpoint reused the release build with `PROPTEST_CASES=1024` and one test thread.
Explicit per-test generation settings still apply where specified.
All 77 ledger tests and eight projection tests passed in `resource-checkpoint.SgZL22`.
The commands were `cargo test --release -p block-storage --lib finalization_ledger` and the corresponding `finalization_snapshot_tests` filter.
The snapshot includes the resource fixtures, constructor tests, and test-only phase markers.
No source input changed during that run.

## Recovery qualification and local completion

The user approved stopping only the original recovery Loom run and installing the reviewed reduction.
The exact scope `run-p484847-i4692502.scope` was confirmed active before stopping and inactive afterward.
Session `73562` returned exit 143.
Its three input hashes still matched before the test changed.
No other process was stopped.

The permanent test now moves only the redundant covering-cursor query after both compactor joins.
Both workers still run concurrently with page budgets one and two.
They traverse every receipt kind and the final marker through the actual production compaction kernel.
The query fails if it attempts a receipt read, and the final assertion requires the unchanged effects cursor.
No preemption, permutation, duration, or checkpoint cutoff was added.

All six tests passed in release mode and with the exact explicit-gate flags.
The explicit finalization gate now includes `loom_production_ledger_pages`.
The general Loom gate already discovers this target.
The full broad finalization gate was not rerun or claimed complete by these targeted checks.

```text
4f5fa5330470181d54011cd86ad889665f7db50cbe424394d7bc4946e044523a  formal/loom/cost_accounting/tests/loom_production_ledger_pages.rs
d71614e874793bdc05ece0aba55d8e5de2ed675e9e35bc652f187f91bc289310  scripts/check-finalized-floor-ALL.sh
```

The local task now has evidence for its seven implementation requirements and the associated verification obligations.
The evidence matrix preserves the distinction between parameterized proofs, finite model checks, native regressions, and measured allocations.
Backend power-loss guarantees, whole-node resource limits, and campaign soak qualification remain outside this local evidence claim.
No Casper voting rule, production lock, stored encoding, or cost distribution changed in this final test reduction.
No Git write occurred during this checkpoint.

The finalization gate now explicitly includes the qualified integrity-page and sparse-transaction Loom targets.
The general Loom gate already discovers those targets.
The original execution remains stopped, not passed.
The subsequent complete target runs satisfy the separate recovery qualification requirement.
