# TASK-019-7: Candidate images and lifecycle fix

**Reviewed:** 2026-09-23.

**Status:** The baseline images and executables are verified. The lifecycle fix merged to system-integration `dev` through PR #144.
Promotion PR #145 merged to `main`. All three node pins now use the merged revision in the prepared change.
Node pin publication, live validation, and the observer candidate remain pending.

## Baseline candidate

The source revision is `6d6d4fed6f84baa0913d8e87f32a7ffe2e0ca59a` on `dev`.

[CI run 35822076392](https://github.com/F1R3FLY-io/f1r3node-rust/actions/runs/35822076392) passed on attempt 1.
The [publication job](https://github.com/F1R3FLY-io/f1r3node-rust/actions/runs/35822076392/job/107071371889) published both platforms.

The repository is `docker.io/f1r3flyindustries/f1r3fly-rust`.
The release tag is `dev-v0.4.46-canary.1758-198-g6d6d4fed`.

| Object | Immutable digest |
| --- | --- |
| Multi-platform index | `sha256:beaf1112e5de018e6a8e2871e872ff0bb7985900af940569651e36409967d807` |
| Linux amd64 manifest | `sha256:b38f6d7d832a586db97c703fbfde2eead5b11e9e575269b994b1a1b9ffc475eb` |
| Linux arm64 manifest | `sha256:13f65626a091b81743ec96cf52d2023e5eb171aa777a5b530a29aa424d5cd330` |

Registry reads used these digests. SHA-256 checks matched the index, both manifests, and both configuration objects.
The configuration objects confirm `linux/amd64` and `linux/arm64`.

The CI artifact records and publication logs bind these images to the source revision.
The configuration labels contain the version and maintainer, but no source revision.

The publication logs also record the same digests for Oracle Cloud Infrastructure Registry.
That registry was not queried directly.

Local evidence is retained under `target/task-019-7-candidates-6d6d4fed6/`.
The directory contains registry responses, verification results, CI logs, and a SHA-256 manifest.
Bulk evidence remains outside Git.

This baseline uses system-integration revision `b3d14b27e3c6276b1eb4ab9ccef04e02b0c4e283`.
It has no observer interface and does not include the proposed lifecycle fix.

TASK-017-12 can use these immutable references for baseline candidate review.
Candidate repinning, admission, and live qualification have not run in this task.

## Initial system-integration proposal

This section retains the original proposal. The follow-up below records implementation, review, and publication status.

The affected test is `integration-tests/test/tests/custom/test_validator_lifecycle.py`.
The affected helper is `_submit_pos_until_effective`.

The [retained timing report](https://github.com/F1R3FLY-io/f1r3node-rust/blob/12bde79f3255aa09faeeb4ecbcdff7d92fa2d45a/docs/casper/cbc-evidence/runs/casper-integration-timeout-fix-20260922-01/report.json) identifies the failure.
All eight nodes recorded `Finalized` after approximately 137 seconds.
The resolver stopped after 135 seconds, while finalized floor heights continued to advance.

The proposed change includes deploy inclusion time in each settlement attempt:

```diff
         verdicts = resolve_deploy_verdicts(
             all_nodes,
             list(ids.values()),
-            timeouts.finalization * 3,
+            (timeouts.deploy_inclusion + timeouts.finalization) * 3,
             label=f"{label} attempt {attempt}",
         )
```

The default inclusion budget is 30 seconds. The default finalization budget is 45 seconds.
The proposed combined budget is 225 seconds per deploy and attempt.
The expression preserves timeout scaling. A literal 225-second timeout would lose that behavior.

The proposal changes this helper only. Other finalization waits retain their existing budgets.
Every node must still report a successful terminal verdict before the helper returns that deploy.

### Regression coverage

The proposed test files are `unit-tests/test_validator_lifecycle_settlement.py` and `unit-tests/fixtures/validator-lifecycle-settlement.json`.

| Case | Required result |
| --- | --- |
| All eight nodes finalize at approximately 137 seconds. | The old budget fails. The combined budget passes. |
| Inclusion and finalization budgets use a scale factor. | The resolver receives the scaled combined budget. |
| A deploy has no terminal verdict before the deadline. | The helper fails. |
| A node reports `Failed`, or nodes disagree. | The helper fails. |
| Several mutations share one attempt. | All submissions occur before verdict resolution. |
| Some deploys finalize and others expire. | Only expired deploys receive new signatures in the next attempt. |
| Every attempt expires. | The helper fails after `max_attempts`, which defaults to three. |

Use a fake clock and recorded verdicts for the timing regression.
Retain the failure with the old expression.
Run the settlement tests, the resolver tests, and Ruff checks.
Run the full validator lifecycle integration test before accepting the new suite revision.

The historical report records three failing tests before the fix and 39 passing tests afterward.
It records no live rerun, commit, push, or pin update.
Those results are historical evidence. This task has not reconstructed or rerun that patch.

### Branch and PR

A new system-integration branch and PR are required.
The inspected `main` revision is `3f19b6b38d2aaf68aeadffc18b7fe620d69e7298`.
Its helper still uses the old expression. No published correction branch or PR was found during this review.

- Implementation branch: `fix/validator-lifecycle-settlement-budget`.
- Fix PR target: `F1R3FLY-io/system-integration:dev`.
- Promotion: `dev` merges through to `main`.
- Proposed title: `fix(tests): include deploy inclusion in validator lifecycle settlement budget`.

The PR scope contains the helper change, the regression tests, and the timing fixture.
The initial proposal changed no system-integration source or branch.

After promotion to `main`, a separate node PR to `dev` must pin the merged 40-character `main` revision.
The node helper `scripts/repin-system-integration.sh` updates all three required sites:

1. Update `.github/oci-validation.env`.
2. Update `.github/workflows/_integration-pipeline.yml`.
3. Update `.github/workflows/merge-recovery-soak.yml`.

The workflow invariant check requires agreement across these files.
The soak branch must update its three sites separately.

## Remaining completion conditions

The baseline publication requirement has evidence, but TASK-019-7 is not complete.

1. Merge the system-integration correction to `dev`, then promote `dev` to `main`.
2. Merge the node pin update to `dev`.
3. Record the images that CI publishes for the new `dev` revision.
4. After TASK-019-6 merges PR #447, record a separate observer candidate.
5. Supply each candidate's platform digests and source revision to TASK-017-12.

[PR #447](https://github.com/F1R3FLY-io/f1r3node-rust/pull/447) remains open at `cef1f4b721b8109019459f11c53d49df00eb68f9`.
The current baseline cannot satisfy observer adapter qualification.

## Coordination handoff

The user authorized coordination with the system-integration agent on 2026-09-23.
The sibling checkout already uses `fix/validator-lifecycle-settlement-budget` at the inspected `main` revision.

Both repositories specify shared Markdown files for agent coordination.
The request is registered as `SI-TASK-VALIDATOR-LIFECYCLE-SETTLEMENT-2026-09-23` in the sibling tracker.

The request path is `../system-integration/docs/discoveries/2026-09-23-validator-lifecycle-settlement-budget-request.md`, relative to this repository root.
The result path is `../system-integration/docs/discoveries/2026-09-23-validator-lifecycle-settlement-budget-result.md`.

The system-integration agent owns implementation and verification.
The node agent owns independent review and the later node pin update.
The handoff requests failing and passing regression results, full unit results, Ruff results, and separate live integration status.

The agent `codex-system-integration-20260923` acknowledged the request and accepted the scope and ownership split.
The coordinator confirmed that the approved three-file implementation and verification should proceed.
The follow-up below records the returned implementation and independent review.

The user corrected the target sequence to `dev`, then `main`.
The shared request and tracker now specify both merge stages and the final `main` revision for node pinning.

## Independent review and publication follow-up

The user assigned TASK-019-7 to this agent while another agent continued TASK-019-3.
This review changed no B2 implementation or verification artifact.

[System-integration PR #144](https://github.com/F1R3FLY-io/system-integration/pull/144) merged the correction to `dev` on 2026-09-23.
The merge revision is `ef9844893f19df3e7523bb97e9e0da0ca241bb10`.
[Promotion PR #145](https://github.com/F1R3FLY-io/system-integration/pull/145) targets `main` with that exact head.
The promotion merged at `2026-09-23T19:55:44Z` as `e3c4e14189f0c6ced2e9674487fcbdeffd93141b`.
The merged tree is `8cdc87b5e9bdcb2b403e52900ccf7bbe3da482ce`.
This tree matches the reviewed revision exactly, so the 42-test result covers the promoted source.

The promotion diff contains the helper change, eleven regression cases, their timing fixture, and the task record.
The complete change from the node's current harness pin adds no other executable behavior.
Other changes update documentation and one Compose comment.

Independent review found no blocking code issue in the correction.
The timeout includes deploy inclusion and retains scaling, terminal verdict checks, shared node deadlines, and selective retries.
The timing fixture explicitly identifies its 137-second input as synthetic historical evidence.

The independent test run passed all 42 lifecycle and resolver tests in 7.30 seconds.
One existing integration-marker warning remains.
The implementer's earlier full unit run passed 324 tests. This review did not repeat that full run.

The live eight-node lifecycle test remains pending.
The reviewed PR requires the node `casper-integration` job after the pin update.
Its result must identify the suite revision, job, observed settlement time, and 225-second budget.

The user reported the completed promotion. GitHub independently confirmed the merge and current `main` revision.
The node pin helper updated all three sites to `e3c4e14189f0c6ced2e9674487fcbdeffd93141b`.
Workflow invariants and the existing pin-helper regression tests pass.
The diff replaces exactly three pin values against node `dev` revision `6d6d4fed6f84baa0913d8e87f32a7ffe2e0ca59a`.

The prepared patch is `target/task-019-7-pin-e3c4e1418/node-pin.patch`.
The patch applies to clean copies of the three `dev` files and reproduces the prepared files exactly.
The directory also contains the verification report and prepared PR title and description.
The proposed branch is `ci/repin-validator-lifecycle-settlement`, targeting `dev`.
The proposed commit contains only the three pin files.
The task notes remain separate from that proposed commit.

The user requested completion of the prepared pin PR, live validation, and image publication with the other agent.
[Node PR #450](https://github.com/F1R3FLY-io/f1r3node-rust/pull/450) is open against `dev`.
Its commit is `6497dd76a029481d63e49c2af6f0f91c4bd71fe2`.
The commit contains exactly the three pin changes.
The publication used a temporary index and preserved the active checkout and main index.

All pre-commit checks passed without a skip.
The push hook initially stopped because `cargo-nextest` was unavailable.
The workflow-only push used the approved Rust-suite skip. GitHub CI owns the full Rust run.
That setting exposed an inherited-variable defect in the push-hook test fixture.
The fixture passed when `SKIP_TESTS`, `TEST_RUNNER`, and `QUICK` were absent.
All five CI script checks passed across those invocations.
The final push retained lint and dependency checks and avoided repeating the environment-sensitive fixture.

[CI run 35921674172](https://github.com/F1R3FLY-io/f1r3node-rust/actions/runs/35921674172) tests the pin PR.
Its live lifecycle result remains pending.
The shared coordination file `/tmp/migrationPlan.md` records task ownership and the separate PR #447 failures.
The system-integration request file asks the other agent to review the live evidence when it becomes available.
No acknowledgment is inferred from those file updates.

## TASK-017-12 candidate handoff

The current node `dev` revision remains `6d6d4fed6f84baa0913d8e87f32a7ffe2e0ca59a`.
The published baseline references above remain valid.
Fresh registry requests used immutable digests for the index, both platform manifests, both configurations, and each final image layer.
Every downloaded object matched its SHA-256 digest.
Each decompressed layer also matched the configuration's recorded layer digest.

The final layers contain `opt/docker/bin/node` as a regular file.
The extracted executable headers identify the expected architecture.
The review read and hashed each executable without running a container or node.

| Platform | Executable SHA-256 | Executable bytes |
| --- | --- | --- |
| Linux amd64 | `8069dedaaf18b3b53bb3cdec84e4b0b1f57b539f832db2e4a02347abf2ec5975` | 76,226,304 |
| Linux arm64 | `1c9efe6d605ec416c6abc73963c0d175df8315440935c42e0dc5c667e86f49fc` | 74,744,040 |

The retained CI publication job binds both platform digests to the source revision.
Its archived logs and the earlier registry records still match their recorded hashes.
The images contain no observer interface. Their publication CI used the old system-integration pin.

The machine-readable handoff is `target/task-019-7-review-20260923/report.json`.
Its directory retains the registry objects, PR states, CI metadata, executable identities, and independent test result.
The directory's `sources.sha256` records the evidence hashes. Bulk evidence remains outside Git.

TASK-017-12 can use these references for baseline candidate review.
Candidate repinning, workload identity, admission, and live qualification remain separate consumer steps.
An observer candidate remains unavailable until TASK-019-6 merges PR #447 and `dev` CI publishes both platforms.

## Remaining completion gates

1. Publish the prepared node pin change through a separately authorized commit and PR to `dev`.
2. After the required checks pass, obtain the authorized node PR merge.
3. Record the live lifecycle result from the pinned suite.
4. Record both platform digests from the new `dev` publication.
5. After TASK-019-6 completes, verify the separate observer candidate.
6. Supply the observer candidate identities to TASK-017-12.

TASK-019-7 remains in progress because these gates have no completion evidence.
