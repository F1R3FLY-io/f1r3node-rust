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
2. Verify Dockerfile workspace copies: every Cargo workspace member has a `COPY` into the builder stage of `node/Dockerfile`
3. Verify supply-chain controls
4. Check formatting
5. Run clippy

### Commit Policy

The job verifies the commit identities of pull request commits (pull requests only). It reads the pull request head SHA, so it has no cache step. A cache step in the same job is the pattern that the CodeQL rule `actions/cache-poisoning/poisonable-step` reports. The step fetches commit objects only. It runs the commit identity checker and reads the denied identities list from the base commit, so a pull request cannot weaken its own check. Before these files reach the base branch, the step uses the copies in the pull request.

`.github/scripts/commit-policy-exemptions.sh` lists the exempt commits: the commits of a human pull request author, and the contributor commits of an imported fork pull request. The step runs the base copy of that script.

### Script Tests

The job runs these script tests:

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
- Heartbeat timing driver
- Soak dashboard restoration

### Soak Harness Tests

1. Build the soak harness
2. Verify soak fail-closed driver (`scripts/bench/test-run-merge-recovery-soak.sh`)
3. Verify isolated disk admission and emergency scenarios (`scripts/bench/test-soak-disk-admission.sh`)
4. Verify the disk admission failure report and bounded-command liveness (`scripts/bench/test-soak-disk-admission-timing.sh`, with `SOAK_DISK_TEST_SKIP_TIMING=1`)

### Heartbeat timing benchmark

Routine integration preflight excludes the optional heartbeat timing benchmark from collection and execution.
The preflight still rejects a skipped test in its selected suites.

The manual `Heartbeat Timing Benchmark` workflow runs the timing benchmark against a selected node revision.
It uses one 64 GB OCI runner and defaults to 20 measured repetitions after one warm-up.
The timing test has a 45-minute limit.
The full runner job has a 100-minute limit, which includes the node image build and artifact upload.
The workflow records the node revision, harness revision, image identity, test output, and timing measurements.
Runner cleanup applies after success, failure, or cancellation.
The benchmark does not publish a preflight or release-gate result.

Select `Heartbeat Timing Benchmark` in GitHub Actions, then select `Run workflow`.
Set `target_ref` to the node branch, tag, or commit.
Set `repetitions` to an integer from 1 to 20.
The workflow enables `HEARTBEAT_TIMING=1` for this benchmark only.
No repository variable is required.
The workflow must reach the default branch before GitHub exposes its manual entry point.

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

A push to `dev` carries the commit that its merge group tested. The target step finds the newest `ci.yml` merge group run of the pushed commit. If both Integration Gate jobs of that run concluded success, the push run sets `oci_tests` to `false`.

Mergify merges a pull request with a merge commit whose second parent is the pull request head. Without a merge group run, the target step compares the tree of the pushed commit with the tree of that head. If they are equal, it reads the newest `ci.yml` pull request run of that head. If both Integration Gate jobs of that run concluded success, the push run sets `oci_tests` to `false`. Any other commit always runs the OCI tests. The Integration Pipeline then builds the images and runs the smoke tests, but it launches no OCI runners.

When the push run reuses a merge group or pull request result, both Integration Gate jobs name the reused run. They pass only if the image build and the smoke tests of the push run succeeded. Release Docker Image still publishes the images. If the target step finds no such run, or an API call fails, the push run runs the OCI tests. A push to `master`, a tag, a pull request, and a merge group always run them.

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

The back-merge must be a merge commit, because `dev` must keep the `master` history. The Mergify `dev` queue merges it with a merge commit.

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
| Promotion from `ca-canary` to `ca-release` | `just ca-promote`, or operation `ca-promote` | `promote/ca-release-<UTC timestamp>` |
| Back-merge from `ca-release` to `ca-canary` | `just ca-backmerge` | `backmerge/ca-release-<12-character SHA>` |
| Occasional sync from `dev` to `ca-canary` | `just ca-sync`, or operation `ca-sync` | `ca/sync-dev-<12-character SHA>` |

The commands run the same scripts as `just promote` and `just backmerge`. The operations are inputs of the **Promote and Back-merge** workflow, which runs the same scripts from `master` with the promotion app token. Do not open a pull request with head `dev` into `ca-canary`, because the protected head guard closes it. Use `ca-sync` instead. The scripts accept only these three pairs and the `dev` and `master` pairs. The protected head guard rejects and closes a pull request with head `ca-canary` or `ca-release`.

CI treats the two branches like `dev` and `master`:

- A push to `ca-canary` or `ca-release` runs the heavy pipeline. The merge group reuse applies to `dev` only, so every push runs the OCI integration tests.
- A pull request to `ca-canary` or `ca-release` with the `ci-heavy` label runs the heavy pipeline. Mergify adds the label to every pull request when it opens. Its head must not be a protected branch.
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

Mergify merges every pull request into `dev`, `master`, `ca-canary`, and `ca-release`. Every merge is a merge commit. Each queue checks one pull request at a time. Mergify first merges the current base into the pull request, and then it waits for the checks on the pull request head.

The rulesets require a branch to be up to date before a merge. Mergify then accepts only in-place checks on the pull request head, not checks on a temporary draft pull request. A draft pull request has no `ci-heavy` label, so it would get no Integration Gate result. `.mergify.yml` therefore sets these values:

- `merge_queue.mode: serial` and `merge_queue.max_parallel_checks: 1`
- `batch_size: 1` and `max_checks_retries: 0` in each queue
- `merge_conditions` identical to `queue_conditions` in each queue

Each queue lists its checks in both condition lists. A pull request therefore enters a queue only after its checks pass.

Mergify adds the `ci-heavy` label to every open pull request of this repository into `dev`, `master`, `ca-canary`, and `ca-release`. It does not label a protected head, or a `ca-canary` pull request that is not a `ca/` item. A draft gets the label when it is ready for review. If someone removes the label without a valid waiver, Mergify adds it again. The label runs the heavy pipeline on the pull request.

Each push to a labeled pull request runs the heavy pipeline again. Each heavy run uses OCI instances from the shared daily quota, so push in batches. A `LimitExceeded` error means that the quota is used up for the day.

Without a waiver, a queue merges only when both Integration Gate checks succeed. A skipped check does not satisfy `check-success`.

A member of `@F1R3FLY-io/ci-maintainers` can waive the heavy run on the member's own pull request into `dev`, `master`, or `ca-canary`. To waive, remove the `ci-heavy` label. To avoid any heavy run, open the pull request with `--label ci-heavy-waived`.

When the author removes `ci-heavy`, the **CI Heavy Label** workflow adds `ci-heavy-waived`. If Mergify adds `ci-heavy` again before it sees the waiver, the workflow removes `ci-heavy` one more time. The workflow then cancels the running CI run, including its OCI jobs, and reruns CI with the current labels. The heavy pipeline is skipped, and the Integration Gate checks report skipped. Failed gate results of an earlier heavy run are replaced in the same way.

The `dev`, `master`, and `ca-item` queues accept the waiver only when the author is a ci-maintainers member. For another author, Mergify adds `ci-heavy` again and keeps it, so the heavy pipeline runs. `devProtect` requires both Integration Gate checks, and GitHub counts a skipped gate job as passing, so a waived pull request can merge into `dev`. `masterProtect` does not require the gates. The `ca-release` queue has no waiver.

A waiver into `master` skips only the heavy run before the merge. The push to `master` runs the heavy pipeline, and the release gates require its Integration Pipeline jobs before a release.

CI does not run on a `labeled` event. A run for a label replaces the checks of the pull request head with pending checks. The merge queue adds the `queued` label when it queues a pull request, so the required checks became pending and the merge failed (pull request #826).

The **CI Heavy Label** workflow reruns the newest CI run of the head when the `ci-heavy` label is added, and when a waiver takes effect. It does not rerun a run whose heavy pipeline already runs or passed, so the label that Mergify adds at open does not restart CI. The rerun keeps the `pull_request` event, and Resolve Target reads the current labels to start or skip the heavy pipeline. Other labels, for example `queued` and `dequeued`, do not start CI. An edit of the pull request still starts CI, because a base change arrives as an edit.

| Queue | Pull requests | Queue and merge conditions |
|---|---|---|
| `dev` | Any pull request of this repository into `dev`, also the `master` back-merge | An approval or a `ci-maintainers` author, requested reviews, no requested changes, the `devProtect` checks, Promotion Source, and `ci-heavy` with the gates, or a maintainer waiver |
| `master` | Head `promote/<UTC timestamp>` or `hotfix/*` into `master` | An approving review, no requested changes, the `masterProtect` checks, Promotion Source, the casper shards, and `ci-heavy` with the Integration Gates, or a maintainer waiver |
| `ca-item` | Head `ca/*` into `ca-canary`, also the dev sync | As for `dev`, and also the `ca-soaked` label |
| `ca-daily` | Head `backmerge/ca-release-<SHA>` into `ca-canary`, opened by the promotion app | The light `ci.yml` checks and `CA Daily Provenance`. No review and no heavy run. The trusted validator accepts the release source and clean base merges only. |
| `ca-release` | Head `promote/ca-release-<UTC timestamp>` into `ca-release` | An approving review, no requested changes, the `ci-heavy` label with the Integration Gates and no waiver, and the `dev` checks |

To merge a pull request into `master` or `ca-release`, get an approving review.

A member of `@F1R3FLY-io/ci-maintainers` can merge a pull request into `dev` or `ca-canary` without an approval. Any other author needs an approving review. In both cases, each requested reviewer or team must submit a review, and no review can request changes. A reviewer who does not agree must use **Request changes**. A comment-only review removes the request and does not block the merge.

To merge any other pull request into `dev`, get an approving review. Mergify adds `ci-heavy` when the pull request opens, queues it, and merges it after the checks pass. A push to the pull request starts the checks again.

To merge a `ca/<item>` pull request:

1. Add the `ca-soak` label to start its 22-hour soak. A passed soak adds the `ca-soaked` label.
2. Get an approving review, or be a member of `@F1R3FLY-io/ci-maintainers`. Each requested reviewer must submit a review.

Mergify queues the pull request. It added `ci-heavy` when the pull request opened.

A pull request from a fork gets no heavy run, so no queue accepts it. A maintainer pushes the fork head unchanged to a branch of this repository. The maintainer then opens a pull request from that branch with this body line:

```
Imports fork pull request #<N>.
```

Commit Policy then exempts each commit that the fork pull request lists and attributes to its author. The commit must also be in the new pull request. A rebase or an amended commit gets a new SHA, so it loses the exemption.

#### Queue priority

The priority rules of `.mergify.yml` set the order of the queued pull requests across all queues. The queue order in the file does not set a priority. In each tier, the pull requests keep the order in which they entered the queue.

| Priority | Pull requests | Interrupts running checks |
|---|---|---|
| 6000 | Security fixes: the `security` label, or a head that contains `security` or `rustsec` | Yes |
| 5000 | `fix/*` into `dev`, and `hotfix/*` into `master` | No |
| 4000 | The release flow: `promote/*`, `backmerge/*`, and `ca/*` heads | No |
| 3000 | `perf/*` into any branch | No |
| 2500 | `ci/*` | No |
| 2000 | Every other head, for example `feat/*`, `feature/*`, `docs/*`, and `chore/*` | No |

Name a security fix `hotfix/security-<topic>` into `master` or `fix/security-<topic>` into `dev`. Mergify then adds the `security` label. If a security fix has another name, add the `security` label by hand. A security fix interrupts the checks of the pull request that Mergify checks, and that pull request goes back into the queue. Its heavy run starts again later. A pull request of a lower tier waits until the running checks complete.

`test-mergify-config.sh` checks the queues. It holds copies of the required checks of `devProtect` and `masterProtect`. A change of a ruleset changes that copy and both condition lists of `.mergify.yml` in the same pull request. The test also checks the serial in-place settings.

The Mergify app must be installed in the organization. Mergify reads `.mergify.yml` only from the default branch, `master`. The queues start to operate after a `dev` to `master` promotion carries the file. The `ci-maintainers` team owns `.mergify.yml` through `.github/CODEOWNERS`.

The `ca-soaked` label stays on the pull request when Mergify merges a new `ca-canary` base into it. A back-merge or a merged sync therefore does not need a new soak. A push by the author removes the label, so a code change needs a new soak. A commit status cannot do this, because a new head commit has no status and Mergify then removes the pull request from the queue.

A person with triage access can add the label by hand. The 60-hour soak of `ca-release` still gates each `ca-v*` tag.

### Merge rulesets

On 2026-10-21, PR #794 merged into `dev` outside the merge queue. An organization admin used the bypass of `devProtect`, so the required checks and the merge queue did not run. The push to `dev` then failed the image build. These ruleset changes make Mergify the only path into `dev` and `master`.

| Ruleset | Change |
|---|---|
| `mergify-only` (new) | Targets `dev`, `master`, `ca-canary`, and `ca-release`. The **Restrict updates** rule blocks every update of the branch. The bypass actors are the Mergify app (10562) and the CI automation app (4428544) only. |
| `devProtect` | Remove the `merge_queue` rule, so Mergify can merge. Keep the pull request and required check rules. |
| `masterProtect` | No change. |

GitHub applies the rules of every ruleset that targets a branch. A bypass of `devProtect` or `masterProtect` therefore does not pass the **Restrict updates** rule of `mergify-only`. No person can merge into `dev` or `master`, organization admins included. The bypass actors of `devProtect` and `masterProtect` stay, but they allow no merge.

An organization admin who must merge by hand changes `mergify-only` first. The organization audit log records that change.

Apply the changes in this order:

1. Merge the pull request that adds the `dev` and `master` queues into `dev`.
2. Promote `dev` to `master`. This promotion is the last merge by a person.
3. Create `mergify-only`, and remove the `merge_queue` rule of `devProtect`.

Until step 3, the GitHub merge queue stays on `dev`, and the target step still reuses its merge group results.

### Daily provenance

`ca-daily-provenance.yml` runs on `pull_request_target` and checks out only the captured base commit.
It fetches pull request commit objects without checking out or executing pull request code.
The validator requires a same-repository head and the promotion app as the pull request author.

The named source must belong to `ca-release`.
The head can be that source or up to 128 clean base merges after it.
Each merge must have the previous head as its first parent and a captured base ancestor as its second parent.
The validator compares each actual tree with the clean tree from `git merge-tree --write-tree`.
An extra commit, altered tree, conflict, or missing object fails the check.

The workflow posts `CA Daily Provenance` directly on the event head SHA, not the workflow base SHA.
A failed or incomplete validation posts failure when the result step can run.
A missing or pending status also blocks the daily queue.
The workflow and validator must reach `ca-canary` before the queue can pass this gate.
The [design](plans/ca-daily-provenance.md) gives the verification scope.

### Soak gates

`ca-soak-dispatch.yml` (Release Train Soak Dispatch) starts the gate soaks. `.github/scripts/ca-soak-dispatch.sh` does the work, and `test-ca-soak-dispatch.sh` checks it.

| Event | Action |
|---|---|
| The `ca-soak` label on a `ca/*` pull request into `ca-canary` | Removes `ca-soaked` and starts the 22-hour soak of the head, gate `ca-item` |
| A push to that pull request by Mergify | Nothing. Mergify only merges the base. |
| A push to that pull request by anyone else | Removes `ca-soak` and `ca-soaked` and cancels the queued and running soaks of the pull request. A new soak needs the `ca-soak` label again. |
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

A run never replaces the status of a newer run. After a successful item soak, the script adds `ca-soaked` to the pull request. It does this only when the pull request is open into `ca-canary` and its head is still the soaked commit. The script then reads the head again and removes the label if the head moved. A final item result that is not a success removes `ca-soaked` from the soaked head, also after a manual dispatch. `test-soak-gate.sh` checks these rules, and `docs/claims/soak-release-gates.md` states them as a CbC claim.

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
| GitHub merge queue | None (Mergify) | None (Mergify) |
| Restrict updates | Bypass by the Mergify app only, as in `mergify-only` | Bypass by the Mergify app only, as in `mergify-only` |

A ruleset cannot require a review for some heads only. The review rule therefore needs a decision: a separate ruleset for each head pattern, or a review in Mergify only.
