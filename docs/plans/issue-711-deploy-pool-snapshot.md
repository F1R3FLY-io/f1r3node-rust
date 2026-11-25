---
task: TASK-012-31
issue: https://github.com/F1R3FLY-io/f1r3node-rust/issues/711
date: 2026-10-19
baseline: 1ea5336848c392d73ee3cd831ffae34b1d3306c5
handoff_status: ready
next_steps:
  - Register the implementation task before code changes.
  - Implement the snapshot refactor on a branch from current dev.
---

# Issue #711 implementation plan

The user requested investigation and a plan before implementation.
The planning session changed task records only.

## Findings

The local `dev`, local `origin/dev`, and GitHub `dev` have the same baseline shown above.
The issue is open, assigned to `semyon-dev`, and has no comments.
The open pull request search found references in PRs #730 and #714, with no implementation of #711 identified.

Four functions read the complete deploy pool during normal block creation:

| Function | Current source line | Purpose |
| --- | --- | --- |
| `storage_has_unresolved_in_scope_deploys` | 1770 | Check whether a stored signature occurs in the scope. |
| `fresh_local_deploy_stats` | 2212 | Count fresh candidates and calculate their oldest age. |
| `in_scope_local_deploy_stats` | 2281 | Count recovery candidates and calculate their oldest age. |
| `prepare_user_deploys_with_policy` | 614 | Select deploys and remove terminal records. |

The source is [block_creator.rs](../../casper/src/rust/blocks/proposer/block_creator.rs).
The production coordinator is `create_with_progress` at line 2911.
The public `prepare_user_deploys` wrapper also calls the preparation function.

The [review decision](https://github.com/F1R3FLY-io/f1r3node-rust/pull/676#issuecomment-6135170014) requests a separate pull request for one pool read.
Issue #712 owns removal of the two pool mutexes.

The storage [any method](../../block-storage/src/rust/deploy/key_value_deploy_storage.rs) already supports early termination.
Its [quarantine implementation](../../block-storage/src/rust/deploy/deploy_record_quarantine.rs) handles damaged records encountered during the scan.
An `any` call can still scan the complete pool when it finds no match.
The target is one full `read_all` plus this predicate scan.

## Implementation sequence

1. Register an implementation task under EPIC-012 with measurable acceptance conditions.
2. Create `perf/711-deploy-pool-snapshot` from current `origin/dev` in this checkout.
3. Record the preservation argument and new test obligations in the existing claim documents before code changes.
4. Replace the scope helper's `read_all` loop with `any` using the same signature predicate.
5. Capture one owned `HashSet<Signed<DeployData>>` before the two statistics calls in `create_with_progress`.
6. Pass a shared reference to that set into both statistics functions.
7. Move the same set into `prepare_user_deploys_with_policy` for filtering and selection.
8. Preserve the public `prepare_user_deploys` signature and let its wrapper load its own set once.
9. Update direct test calls to supply the captured set.
10. Add regression tests, run the checks, and record their results before opening the pull request.

Use borrowed candidate references in the statistics functions to avoid cloning the complete decoded pool.
Keep the preparation function's policy guards when it consumes the supplied set.
The public wrapper must preserve its conditional read when ordinary admission is disabled.
The `RecoveryEmpty` branch must bypass the pool reads as it does today.

Keep buffer reads, canonical scans, expiry checks, retry gates, selection caps, and byte budgets in their current paths.
Keep store removals keyed by signature and preserve the pool-before-buffer lock order.
Keep the existing preparation lock and its metric samples while using a short lock for snapshot capture.
Do not extend a lock across the complete statistics and preparation sequence.

## Snapshot behavior

The common set defines the pool view for both statistics and selection in one proposal.
Deploys inserted after capture remain in storage and become candidates in a later proposal.
The proposer and admission currently use separate mutexes for handles to the same store.
A proposal snapshot therefore does not create a transaction across admission, the pool, and the rejected buffer.

Compare old and new results on fixed inputs with no concurrent store changes.
Add a controlled insert after capture to verify that the current proposal uses its snapshot.
Verify that the next capture sees the new deploy and that preparation does not reinsert removed records.
Document this capture boundary in the claim preservation argument.

## Claim requirements

[CLAIM-CASPER-FRESH-ADMISSION-CAP-001](../claims/casper-fresh-admission-cap.md) is pending.
The current document has statements S1 through S8, although the issue names S1 through S7.
Preserve all eight statements, including the cap-source metrics in S8.
Extend the discharge plan with snapshot equivalence, scan-count tests, and the capture boundary.

[CLAIM-CASPER-STAGE-METRICS-001](../claims/casper-stage-metrics.md) also covers this source file and is pending.
Preserve the metric names, labels, units, and the one-sample-per-preparation lock timer behavior.
Distinguish the full-read decoded counter from work performed by `any`.

The source has `cbc=mandatory` and `cbc-weight=high` attributes.
Its [existing evidence record](../casper/cbc-evidence/casper-src-rust-blocks-proposer-block-creator-rs.md) is an older waiver for retry packaging.
Record current source identity, test results, and claim references for review.
Leave acceptance and any discharge decision to the maintainer.
The eight-claim `check-casper-claims` auditor does not cover the admission or stage-metrics claims.
Its successful result alone cannot verify this refactor.

## Regression tests

| Check | Measurable condition |
| --- | --- |
| Normal proposal | One full pool read supplies both statistics functions and selection. |
| Recovery-empty proposal | The proposal performs zero full pool reads. |
| Scope predicate | Empty, unmatched, matched, buffered, and rejected-in-scope cases retain their current results. |
| Early termination | A controlled store iterator stops after the first matching record. |
| Snapshot consistency | Both statistics and selection use the captured pool after a later insertion. |
| Candidate classification | Fresh, in-scope, rejected, buffered, future, expired, and canonical-won cases retain their results on fixed inputs. |
| Cleanup | Damaged records enter quarantine, terminal records are removed, and gated retries remain available. |
| Admission policy | The existing S1 through S8 tests pass with the same expected values. |
| Metrics | Existing cap-source, lock-timer, and decoded-count tests pass. |

Use an isolated test store or recorder to count full reads on the production coordination path.
Count only `deploy_storage` reads and distinguish the rejected buffer by its store label.
Make the scan-count regression fail if a second full read returns.
Reuse existing expiry, retry, byte-budget, and canonical-parent tests for unchanged rules.

Run these focused checks first:

```bash
cargo fmt --all -- --check
cargo test --release -p casper --lib block_creator
cargo test --release -p casper block_creator_spec
cargo test --release -p block-storage
```

Then run the required contribution checks and the complete affected crate tests:

```bash
cargo clippy --workspace
cargo deny check
cargo test --release -p casper
just coverage
```

Use the pinned nightly toolchain and the required build environment from [CONTRIBUTING.md](../../CONTRIBUTING.md).
The planning session had no `cargo` command on the Windows `PATH`.
The implementation uses the pinned toolchain in Ubuntu-24.04 WSL.
The [validation record](../casper/cbc-evidence/runs/issue-711-snapshot-20261019/README.md) holds the implementation results.

## Performance evidence and pull request

Use fixed pools near 2,100 and 15,100 deploys to compare the baseline and refactor under the same conditions.
Measure full-read count, decoded deploy count, preparation time, and total block creation time.
Report the predicate scan separately when it visits every record.
Treat the original 3.8 seconds of read time as historical evidence, not a promised saving.

[PR #730](https://github.com/F1R3FLY-io/f1r3node-rust/pull/730) separately moves signature verification from reads to writes.
It changes the cost of decoding and remains open at investigation time.
Use the same #730 state in both performance measurements.
The refactor can proceed independently because its primary code change is in `block_creator.rs`.

Open one pull request to `dev` with the title `perf(casper): reuse the deploy pool snapshot during block creation`.
Include `Refs #711`, the claim preservation argument, test results, and comparable performance measurements.
Add `ci-heavy` after the final planned push because the pull request changes Rust code.
Use the heavy integration checks and record the claim-specific load or soak evidence required for maintainer review.
Keep issue closure aligned with the EPIC-012 promotion policy.

This plan requires no changes to storage codecs, mutex ownership, admission limits, or consensus rules.
The expected implementation touches `block_creator.rs`, focused tests, task tracking, and claim evidence.
