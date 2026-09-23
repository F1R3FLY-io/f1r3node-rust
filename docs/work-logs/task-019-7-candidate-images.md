# TASK-019-7: Candidate images and lifecycle fix

**Reviewed:** 2026-09-23.

**Status:** The existing baseline images are verified. The lifecycle fix and the observer candidate remain pending.

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

## Proposed system-integration correction

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
Implementation results remain pending.

The user corrected the target sequence to `dev`, then `main`.
The shared request and tracker now specify both merge stages and the final `main` revision for node pinning.
