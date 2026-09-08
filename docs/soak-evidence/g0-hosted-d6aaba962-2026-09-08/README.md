# G0 hosted verification and enforcement review

## Result

Hosted inventory execution is confirmed for `9310ae2ce` and `d6aaba962`. G0 remains open because required-check enforcement and complete acceptance identity are not established.

This record supplements the [prevention plan](../../plans/soak-recurrence-prevention-2026-09-08.md). It does not change the historical B1–B3 records or the tested candidate's inventory.

The final status observation was at `2026-09-08T15:41:57Z`. Pull request #399 remained open and blocked.

## Hosted inventory execution

| Revision | Run | Attempt | Lint job | Inventory step |
| --- | --- | --- | --- | --- |
| `9310ae2ceda65d1944e1235336fd12df489e4197` | `34232911628` | 1 | `102109798473` | Passed |
| `d6aaba9628536245fc42183d1682539583d6550b` | `34244231314` | 1 | `102122023044` | Passed |

Both job records show successful execution of `Verify soak claim inventory`. Both logs report four required claims with current input digests and explicit pending obligations.

The current job checked out `28444208dd1db4a4928a0a8e20e1f586b9c9c065`. The historical job checked out `d50dc533ddac3acf8277c31a4fb75c8232cb99ea`.

Git tree comparisons confirm complete source equality between each checkout and its corresponding pull request head. The inventory contains 126 current inputs and 101 historical inputs.

The workflow-control revision was not independently attested. These checkout identities are not complete soak acceptance identities.

## Hosted formal baselines

The [TLA+ job](https://github.com/F1R3FLY-io/f1r3node-rust/actions/runs/34244230926/job/102121946596) passed in attempt 1. Its checkout tree matches `d6aaba962`.

| Configuration | Observed result |
| --- | --- |
| `MC_ReplayHotLoop` | Completed with nine distinct states and no error |
| `MC_CarrierIndex` | Completed with 222 distinct states and no error |
| `MC_CarrierIndex_dag_first_pre_fix` | Violated `IndexCompleteForWindow` |
| `MC_CarrierIndex_read_failure_pre_fix` | Violated `AbsenceProofSound` |

The job reported both negative controls as expected failures. The reviewed gate requires exit 12 and the exact invariant message. Successful control logs do not separately print the process exit code.

Artifact `10063312912` contains the four complete TLC logs. Its downloaded ZIP matches GitHub's published SHA-256 digest:

```text
67c33849b3cd439fb5e6ded2fd8294b29413086562b66d52d19b9045298252e0
```

The retained text files preserve the artifact members byte for byte. These are bounded baseline results, not evidence of a residual finalization repair or a disk reserve bound.

## Required-check enforcement

The branch-rules endpoint returned two active rulesets that apply to `dev`:

- `devProtect`, identifier `15773875`.
- `masterProtect`, identifier `14299997`.

Both rulesets require `Lint` and use strict required-status policies. Neither visible ruleset requires `TLA+ invariant check`. Both rulesets permit specified actors to bypass the rules.

The classic-protection endpoint returned HTTP 403. This response does not establish that classic protection is absent. No maintainer confirmed the complete enforcement configuration during this review.

GitHub's [required-check documentation](https://docs.github.com/en/pull-requests/collaborating-with-pull-requests/collaborating-on-repositories-with-code-quality-features/troubleshooting-required-status-checks) treats `success`, `skipped`, and `neutral` as successful statuses. Adding a required check alone does not establish success-only execution.

The documentation requires checks against the latest applicable commit. Repository-specific rejection of missing, skipped, canceled, and stale formal results remains unverified.

## Independent infrastructure failure

Oracle Cloud Infrastructure rejected six runner launch requests with `LimitExceeded`, HTTP 400. The service reported a daily resource creation limit.

The runner-launch job failed. Both architecture summary checks failed because no integration slots ran. The checks expected four amd64 slots and two arm64 slots. These failures do not constitute production RED evidence.

The ordinary CI workflow remained in progress at the final observation. Two Casper test jobs and Casper coverage were still active. The workflow cannot be reported as successful.

No runner, quota, workflow, or protection setting was changed. No job was retried or canceled. No soak was dispatched.

## Maintainer actions

1. Inspect classic protection and both active rulesets.
2. Establish a required success-only result for the inventory and applicable formal checks.
3. Verify rejection of missing, skipped, canceled, stale, and unexpected-source results in an isolated test configuration.
4. Review bypass actors against the acceptance policy.
5. Confirm runner quota availability before authorizing an integration retry.

The complete acceptance identity and all pending repair obligations remain open. This review does not authorize a merge or a soak.

## Evidence

[observation.json](observation.json) retains selected API fields, the check snapshot, and the enforcement findings. [source-bindings.json](source-bindings.json) binds the three reviewed jobs to their complete source trees.

[log-excerpts.json](log-excerpts.json) records selected source lines without publishing complete workflow logs. [manifest.json](manifest.json) binds this package and the external raw records.

The raw records reside outside `target/` in a permission-restricted local directory. This storage is not an off-host backup. Historical manifests and source files remain unchanged.
