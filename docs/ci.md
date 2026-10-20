# CI Jobs and Check Names

This page lists the jobs of `.github/workflows/ci.yml`, `.github/workflows/ci-fork-pr.yml`, and `.github/workflows/_integration-pipeline.yml`. Each job has one purpose, and its name states that purpose. A red check therefore tells the reader which kind of work failed.

## Categories

| Category | Meaning |
| --- | --- |
| Gate | Collects the results of other jobs into one check. It runs no check of its own. |
| Static check | Reads files and history. It runs no program under test. |
| Script test | Runs a behavioral test of a CI, release, or soak script. It needs no Rust build. |
| Soak harness test | Builds the `casper-soak` harness and runs the real soak driver, also in Docker containers. |
| Unit test | Runs `cargo test` for a crate. |
| Build | Builds a release artifact or a Docker image. |
| Integration test | Runs a node network on ephemeral runners. |
| Setup | Resolves the target commit and the run decisions for the other jobs. |

## Jobs of `ci.yml`

| Job id | Check name | Category | Needs |
| --- | --- | --- | --- |
| `build_base` | Resolve Target | Setup | none |
| `promotion_source` | Promotion Source | Static check | none |
| `commit_policy` | Commit Policy | Static check | `build_base` |
| `static_checks` | Static Checks | Static check | `build_base` |
| `script_tests` | Script Tests | Script test | `build_base` |
| `soak_harness_tests` | Soak Harness Tests | Soak harness test | `build_base` |
| `deny` | cargo-deny | Static check | `build_base` |
| `markdown_link_check` | Markdown Link Check | Static check | `build_base` |
| `test` | Test (*crate*) | Unit test | `build_base`, `static_checks` |
| `test_casper` | Test (casper *n*/2) | Unit test | `build_base`, `static_checks` |
| `coverage` | Coverage (*crate*) | Unit test | `build_base`, `static_checks` |
| `coverage_summary` | Coverage Summary | Gate | `build_base`, `coverage` |
| `test_casper_gate` | Test Gate (casper) | Gate | `test_casper`, `coverage_summary` |
| `pipeline` | Integration Pipeline | Build and integration test | `build_base`, `static_checks`, `script_tests`, `soak_harness_tests` |
| `integration_tests_amd64` | Integration Gate (amd64) | Gate | `build_base`, `pipeline` |
| `integration_tests_arm64` | Integration Gate (arm64) | Gate | `build_base`, `pipeline` |
| `notify_queue_failure` | Notify Queue Failure | Notification | `pipeline`, both integration gates |
| `release_docker_image` | Release Docker Image | Build | the checks above |
| `release_packages` | Release Packages | Build | the checks above |

The Integration Pipeline waits for all three of Static Checks, Script Tests, and Soak Harness Tests. A failed inexpensive check therefore does not start ephemeral OCI runners.

### Static Checks

1. Verify workflow security invariants (first, before the toolchain install)
2. Verify supply-chain controls
3. Check formatting
4. Run clippy

### Commit Policy

The job verifies the commit identities of pull request commits (pull requests only). It reads the pull request head SHA, so it has no cache step. A cache step in the same job is the pattern that the CodeQL rule `actions/cache-poisoning/poisonable-step` reports. The step fetches commit objects only. It runs the commit identity checker and reads the denied identities list from the base commit, so a pull request cannot weaken its own check. Before these files reach the base branch, the step uses the copies in the pull request.

### Script Tests

The job runs 21 script tests:

- Commit trailer check
- Pre-push hook
- Stack CI gate
- Image publish tags
- Soak schedule routing
- System-integration node capability contract
- System-integration repin helper
- Release evidence
- Release gates
- Promotion pull request opener
- Release promotion
- Release gate evidence
- Deployment train validator
- Deployment train manifests
- Release workflows
- TLA+ gate classification and routing
- Coverage summary aggregation
- Soak summary aggregation
- Soak verdicts
- Latency benchmark readiness and finalization
- Integration preflight driver
- Soak dashboard restoration

### Soak Harness Tests

1. Build the soak harness
2. Verify soak fail-closed driver (`scripts/bench/test-run-merge-recovery-soak.sh`)
3. Verify isolated disk admission and emergency scenarios (`scripts/bench/test-soak-disk-admission.sh`)
4. Verify the disk admission failure report and bounded-command liveness (`scripts/bench/test-soak-disk-admission-timing.sh`, with `SOAK_DISK_TEST_SKIP_TIMING=1`)

## Jobs of the integration pipeline

`_integration-pipeline.yml` runs as the Integration Pipeline job of `ci.yml` and of `ci-fork-pr.yml`. Its check names start with `Integration Pipeline /`.

| Job | Category |
| --- | --- |
| Await Launch Approval | Gate |
| Build Docker Image (*arch*) | Build |
| Launch Ephemeral Runners | Setup |
| Integration Tests (*arch*-*provider*) | Integration test |
| Integration Tests (*arch*-*provider*, ucc) | Integration test |
| Reclaim Ephemeral Runners | Setup |
| Smoke Test (Docker Standalone), (Docker Shard), (Local Standalone) | Integration test |

The input `oci_tests` (default `true`) controls Launch Ephemeral Runners. When it is `false`, the integration and UCC tests and Reclaim Ephemeral Runners skip too. The image build and the smoke tests always run.

## Old and new check names

| Old name | New name | Change |
| --- | --- | --- |
| Build Base | Resolve Target | The job builds nothing. It resolves the target commit and the run decisions. |
| Lint | Commit Policy, Static Checks, Script Tests, Soak Harness Tests | The job ran the commit identity check, static checks, 20 script tests, and the soak harness tests. |
| Test (casper) | Test Gate (casper) | The job is a gate over the casper shards and the coverage summary. |
| Heavy Pipeline / *job* | Integration Pipeline / *job* | The caller job is the integration pipeline. |
| Integration Tests (amd64), (arm64) | Integration Gate (amd64), (arm64) | The jobs are gates over the integration test slots. |

Check names on pull requests and runs from before the change keep the old names.

## Required checks

The `devProtect` ruleset requires these checks:

- Commit Policy, Static Checks, Script Tests, and Soak Harness Tests
- cargo-deny
- Test (*crate*) for each crate, and Test (casper 1/2) and Test (casper 2/2)
- Integration Gate (amd64) and Integration Gate (arm64)

The `masterProtect` ruleset requires Promotion Source, Commit Policy, Static Checks, Script Tests, Soak Harness Tests, cargo-deny, Test (*crate*) for each crate, and Test Gate (casper).

The heavy reuse gate in the `build_base` target step and `release-train.sh validate-ci-evidence` read Integration Gate (amd64) and (arm64). A pull request run from before TASK-023-4 has only the old names. A merge group therefore cannot reuse the heavy result of such a run, and it runs the Integration Pipeline again.

A push to `dev` carries the commit that its merge group tested. The target step finds the newest `ci.yml` merge group run of the pushed commit. If both Integration Gate jobs of that run concluded success, the push run sets `oci_tests` to `false`. A commit that no merge group tested, for example a manual push to `dev`, always runs the OCI tests. The Integration Pipeline then builds the images and runs the smoke tests, but it launches no OCI runners.

When the push run reuses a merge group result, both Integration Gate jobs name the reused run. They pass only if the image build and the smoke tests of the push run succeeded. Release Docker Image still publishes the images. If the target step finds no such run, or an API call fails, the push run runs the OCI tests. A push to `master`, a tag, a pull request, and a merge group always run them.

`ci-fork-pr.yml` also has Integration Gate jobs, and it runs for every pull request. For a pull request from this repository, these jobs are skipped and report as Integration Gate (amd64, fork PRs only) and (arm64, fork PRs only). A skipped check counts as a pass. With the required names, the skipped checks would pass the gate before the `ci.yml` gate finishes. A `ci-heavy` pull request could then enter the merge queue during its own heavy run, and the merge group would run the Integration Pipeline again. `check-workflow-invariants.sh` rejects a fork-only job that reports a `ci.yml` check name when it is skipped.

## Promotion from dev to master

Run `just promote` to open the promotion pull request. The command runs `.github/scripts/open-promotion-pr.sh`. It creates a temporary `promote/<UTC timestamp>` branch at the `origin/dev` commit and opens the pull request to `master` with your `gh` login.

You can also use the **Promote and Back-merge** workflow:

1. Open the **Actions** tab and select **Promote and Back-merge**.
2. Click **Run workflow** and keep the `master` branch.
3. Set **operation** to `promote`, and click **Run workflow** again.

The workflow runs the same script, so the rules for the branch and the pull request are the same. GitHub shows the button only for a workflow on `master`. A workflow merged to `dev` gets its button after the next promotion.

The **operation** input selects the direction. The **Use workflow from** branch does not select it. A run from any branch other than `master` fails, because the `release-credentials` environment allows `master` only. Code merged to `dev` therefore never runs with the app token.

A pull request opened with `GITHUB_TOKEN` starts no CI, so the workflow opens the pull request with a dedicated promotion GitHub App. The app installation grants only **Contents: write** and **Pull requests: write**. The app must not be a bypass actor on the `devProtect` or `masterProtect` ruleset. The CI automation app is a bypass actor on both rulesets, so the workflow does not use it. The `release-credentials` environment holds `PROMOTION_APP_ID` and `PROMOTION_APP_PRIVATE_KEY`, and it allows `master` only.

The app is the pull request author. The user who started the workflow can therefore approve the pull request. CI does not prevent this approval. The rule that a different maintainer approves is a convention. The pull request body names the user who started the workflow and asks a different member of `@F1R3FLY-io/ci-maintainers` to approve.

Do not open a promotion with head `dev`. The repository deletes the head branch of a merged pull request. GitHub then retargets every open pull request based on that branch to the base of the merged pull request. Two promotions from `dev` moved every open `dev` pull request to `master` (2026-10-04 and 2026-10-09). The `Promotion Source` check rejects a pull request from `dev` to `master`.

## Back-merge from master to dev

After a `hotfix/` pull request merges into `master`, merge `master` back into `dev`. Run `just backmerge` to open the back-merge pull request. The command runs `.github/scripts/open-backmerge-pr.sh`. It creates a temporary `backmerge/master-<12-character SHA>` branch at the `origin/master` commit. It then opens the pull request to `dev` with your `gh` login. If `dev` already contains `master`, the command does nothing.

You can also run the **Promote and Back-merge** workflow from `master` with **operation** set to `backmerge`. It runs the same script with the promotion app token. The pull request body then names the user who started the workflow and asks a different member of `@F1R3FLY-io/ci-maintainers` to approve.

If a `backmerge/*` pull request to `dev` is open, the command names it and does nothing. Merge or close that pull request first. The branch name contains the `master` commit. A second run for the same commit therefore fails at branch creation, also when both runs are concurrent.

Merge the back-merge pull request with a merge commit. Do not squash or rebase it, because `dev` must keep the `master` history. The `dev` merge queue uses merge commits.

Do not open a back-merge with head `master`. A pull request with a protected head shows the **Update branch** button. That button merges the base into the head through the admin bypass of the ruleset. On #747 (head `master`, base `dev`), it pushed `dev` onto `master` outside promotion (`bf4915d57`, 2026-10-16).

## Protected head guard

`.github/scripts/check-promotion-source.sh` rejects every pull request from this repository with head `master`, `dev`, `ca-canary`, or `ca-release`, whatever its base. A pull request from a branch of a fork with one of these names is accepted. Two workflows run the script:

| Workflow | Event | Token | Action |
|---|---|---|---|
| `ci.yml`, job `Promotion Source` | `pull_request` | read-only | Fails the check. `masterProtect` requires this check. |
| `protected-head-guard.yml`, job `Protected Head Guard` | `pull_request_target` | `pull-requests: write` | Closes the pull request with a comment that gives the remedy and the reason. |

The guard runs the workflow file and the script from the base branch, not from the pull request. Code that a pull request changes therefore never runs with the write token. The guard also runs when the pull request has a merge conflict, and `pull_request` workflows do not. A closed pull request has no **Update branch** button and cannot merge, so the guard also covers `dev`, where `Promotion Source` is not required.

The guard uses the workflow of the base branch. It protects pull requests to `dev` after this workflow is on `dev`, and pull requests to `master` after the next promotion. The close happens a few seconds after the pull request opens. Only a ruleset blocks a direct push to `master` or `dev` at all times. The change of the human bypass actors from `always` to `pull_request` is still open (TASK-023-13).

## Release train branches

The cost-accounting release train of issue #764 adds two long-lived branches. `ca-canary` has the role of `dev`, and `ca-release` has the role of `master`. Issue #791 tracks the CI work for them.

| Flow | Command | Temporary branch |
|---|---|---|
| Promotion from `ca-canary` to `ca-release` | `just ca-promote` | `promote/ca-release-<UTC timestamp>` |
| Back-merge from `ca-release` to `ca-canary` | `just ca-backmerge` | `backmerge/ca-release-<12-character SHA>` |
| Occasional sync from `dev` to `ca-canary` | `just ca-sync` | `ca/sync-dev-<12-character SHA>` |

The commands run the same scripts as `just promote` and `just backmerge`. The scripts accept only these three pairs and the `dev` and `master` pairs. The protected head guard rejects and closes a pull request with head `ca-canary` or `ca-release`.

CI treats the two branches like `dev` and `master`:

- A push to `ca-canary` or `ca-release` runs the heavy pipeline. The merge group reuse applies to `dev` only, so every push runs the OCI integration tests.
- A pull request to `ca-canary` or `ca-release` with the `ci-heavy` label runs the heavy pipeline. Its head must not be a protected branch.
- An accepted `ca-release` commit gets a tag of the form `ca-v<version>`, for example `ca-v0.5.0-rc.1`. The tag runs the heavy pipeline. It does not match `v*`, so the GitHub release job does not run for it.

Image publication:

| Reference | Version tag | Channel pointer |
|---|---|---|
| Push to `ca-release` | `ca-release-<git describe>` | `:ca-release` |
| Tag `ca-v<version>` | `:ca-v<version>` | `:ca-latest` |

`ca-canary` does not publish images. A `ca-*` reference never moves `:latest` or `:dev`. `test-image-publish-tags.sh` checks these tags.

The heavy pipeline and the soaks use the `oci-credentials` environment. It must allow `branch:ca-canary`, `branch:ca-release`, and `tag:ca-v*`.

Only `dev` and `master` publish without reviewer approval, through `protected-branch-image-publish`. A push to `ca-release` and a `ca-v*` tag publish through `ephemeral-launch`, so each publication waits for one of its required reviewers. `check-workflow-invariants.sh` enforces this rule.

### Dev sync

`dev` reaches the release train only through an occasional sync. A maintainer runs `just ca-sync`. The command opens a pull request from `dev` into `ca-canary` with the `ci-heavy` label. Its `ca/` head puts it in the `ca-item` queue, so it needs the 22-hour soak and an approval like any item. `ca-release` gets `dev` code only through a promotion from `ca-canary`.

### Daily back-merge

`ca-daily-sync.yml` (Release Train Daily Back-merge) runs from `master` at 01:30Z. It opens the back-merge from `ca-release` to `ca-canary`, which brings the promotion merge commits and any hotfix back to `ca-canary`. It does nothing when `ca-canary` already contains `ca-release` or a back-merge pull request is open.

The workflow uses the promotion app token from `release-credentials`, as the Promote and Back-merge workflow does. The pull request body says that Mergify merges it. Until `ca-canary` and `ca-release` exist, the workflow does nothing.

### Mergify

`.mergify.yml` merges pull requests into `ca-canary` only. A maintainer merges the promotion into `ca-release`. The GitHub merge queue stays on `dev` and `master`. Every merge is a merge commit.

| Queue | Pull requests | Merge conditions |
|---|---|---|
| `ca-item` | Head `ca/*` into `ca-canary`, also the dev sync. Mergify queues it by itself. | To enter the queue: the `ci-heavy` label, the `ca-soaked` label, and an approving review. To merge: the `ci.yml` checks of `devProtect` and the Integration Gates. The `ci-heavy` label is necessary for the Integration Gates. |
| `ca-daily` | Head `backmerge/ca-release-<SHA>` into `ca-canary` | The light `ci.yml` checks. No review. |

To merge a `ca/<item>` pull request:

1. Add the `ci-heavy` label after the last planned push.
2. Add the `ca-soak` label to start its 22-hour soak. A passed soak adds the `ca-soaked` label.
3. Get an approving review.

Mergify then queues the pull request by itself.

The Mergify app must be installed in the organization. Mergify reads `.mergify.yml` only from the default branch, `master`. The queues start to operate after a `dev` to `master` promotion carries the file. The `ci-maintainers` team owns `.mergify.yml` through `.github/CODEOWNERS`.

The `ca-soaked` label stays on the pull request when Mergify merges a new `ca-canary` base into it. A back-merge or a merged sync therefore does not need a new soak. A push by the author removes the label, so a code change needs a new soak. A commit status cannot do this, because a new head commit has no status and Mergify then removes the pull request from the queue.

A person with triage access can add the label by hand. The 60-hour soak of `ca-release` still gates each `ca-v*` tag.

### Soak gates

`ca-soak-dispatch.yml` (Release Train Soak Dispatch) starts the gate soaks. `.github/scripts/ca-soak-dispatch.sh` does the work, and `test-ca-soak-dispatch.sh` checks it.

| Event | Action |
|---|---|
| The `ca-soak` label on a `ca/*` pull request into `ca-canary` | Removes `ca-soaked` and starts the 22-hour soak of the head, gate `ca-item` |
| A push to that pull request by Mergify | Nothing. Mergify only merges the base. |
| A push to that pull request by anyone else | Removes `ca-soak` and `ca-soaked` and cancels the running soak. A new soak needs the `ca-soak` label again. |
| A push to `ca-release` | Starts the 60-hour soak of the pushed commit, gate `ca-release` |

The dispatcher runs `merge-recovery-soak.yml` from `master` with the inputs `gate`, `gate_pr`, and `target_ref` as a commit SHA. A gate run differs from a `daily` or `weekend` run in these ways:

- It has its own concurrency group: one for each `ca/*` pull request, and one for `ca-release`. Item soaks run in parallel, and a gate run never cancels a `daily` or `weekend` run.
- Its soak runner registers under a label of its own run, `f1r3fly-rust-soak-run-<run ID>`, so a parallel soak cannot take its VM.
- It publishes no checkpoint and no dashboard series.
- Its one restart after an infrastructure failure soaks a full new window, not the rest of the old one.

At the start, the run posts `Soak Gate (22h)` or `Soak Gate (60h)` as pending on the soaked commit. At the end, `.github/scripts/report-soak-gate.sh` posts the result:

- Success: the soak completed, its integration preflight passed, and its verdict is `pass`.
- Error: the soak was cancelled.
- Failure: any other result.

A run never replaces the status of a newer run. After a successful item soak, the script adds `ca-soaked` to the pull request. It does this only when the pull request is open into `ca-canary` and its head is still the soaked commit. `test-soak-gate.sh` checks these rules, and `docs/claims/soak-release-gates.md` states them as a CbC claim.

### Release tag

Run `just ca-tag <version>`, for example `just ca-tag 0.5.0-rc.1`. The command runs `.github/scripts/tag-ca-release.sh`. It creates the annotated tag `ca-v<version>` at the `ca-release` head. It refuses the tag when the head has no successful `Soak Gate (60h)` status, and it never moves an existing tag. The `rust-client` `master` branch pins this tag and the `:ca-v<version>` image.

### Proposed rulesets

These rulesets are a plan. They are not applied.

| Rule | `ca-canary` | `ca-release` |
|---|---|---|
| Deletion and non-fast-forward | Blocked | Blocked |
| Required checks | The `devProtect` checks. The `ca-soaked` label in Mergify gates the soak. | The `masterProtect` checks. `just ca-tag` requires `Soak Gate (60h)` on the `ca-release` head. |
| Pull request review | One code-owner approval for `ca/*` heads | One code-owner approval for `promote/ca-release-*` heads |
| Merge method | Merge commit only | Merge commit only |
| GitHub merge queue | None (Mergify) | None. A maintainer merges the promotion. |
| Bypass actors | The Mergify app, for the `ca-daily` pull requests | None |

A ruleset cannot require a review for some heads only. The review rule therefore needs a decision: a separate ruleset for each head pattern, or a review in Mergify only.
