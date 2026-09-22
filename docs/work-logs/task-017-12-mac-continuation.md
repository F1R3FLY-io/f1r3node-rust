# TASK-017-12 Mac continuation

## Scope and source identity

The user authorized TASK-017-12 implementation on this Mac. The continuation began at `2530b83853e946dd32a850d78dbbc9491fc23c3f`.

Concurrent commits advanced the checkout through `80ac804d9`, `b1fbe219a`, and `ee468f5db`. This session issued no Git staging, commit, or push command.

The successful gate ran against the working tree based on `ee468f5db`. Its source manifests agree before and after verification.

The [compact report](../casper/cbc-evidence/runs/casper-campaign-control-20260921-01/report.json) preserves the exact source digests. Earlier failed attempts remain under `target/`.

## Implemented changes

The campaign gate runs the positive model and four negative controls. Each negative control requires exit 12, its exact invariant, and a counterexample.

The dedicated workflow runs this gate without cloud credentials. The model report preserves verifier identity, source hashes, process exits, and log hashes.

The execution workflow connects approval preparation, authenticated launch, the exclusive workload runner, and result collection. Missing deployment pins retain the admission-only route.

The missing job script now connects the launch receipt to host admission. The baseline input maps to the correct stage, and preflight receives a usable execution window.

The finalizer reads the result from its authenticated archive. It rejects extra members, links, oversized records, duplicate JSON keys, and substituted extracted results.

Authenticated product failures survive late completion and cleanup failure. Invalid or missing worker evidence still requests cleanup and cannot produce a passing result.

The reservation check runs all sixteen Linux tests from this Mac. Its minimal container contains two static executables and has no network or capabilities.

The container has a read-only root, a non-root user, 512 MiB memory, two CPUs, and 128 MiB temporary storage. Removal was confirmed.

## Verification

| Check | Result |
| --- | --- |
| Complete local gate | Exit 0. |
| Planner and admission fixtures | 115 checks pass. |
| Controller and finalizer tests | 22 tests pass. |
| Supervisor framing tests | Two tests pass. |
| Model gate regressions | Three tests pass. |
| Isolated Linux reservation tests | Sixteen tests pass. |
| Positive model search | Exit 0. |
| Four negative controls | Each returns exit 12 with the registered invariant. |
| Actionlint 1.7.12 | Exit 0, without ShellCheck or Pyflakes. |
| Targeted Clippy | Exit 0 with warnings denied. |
| Crate formatting | Exit 0. |
| Strict eight-claim audit | Exit 4. Claim001 remains pending. |

The reservation helper remains marked ignored for direct suite execution. Its parent test explicitly starts and kills that helper to verify lock recovery.

The first isolated wrapper incorrectly expected zero ignored entries. The corrected wrapper requires sixteen passing tests and the exact helper and parent entries.

The gate retains earlier socket restrictions, missing tool paths, the unavailable Docker socket, and the rejected read-only container copy. No failed attempt became a pass.

Bulk evidence remains under `target/task-017-12-campaign-gate-20260921-07/`. Repository evidence contains the compact report, source hashes, candidate identities, and pending artifact records.

## Candidate and infrastructure review

Both platform images still resolve to `dev` revision `6940a5beb4aa806d3d75f6df3be9f238512fcc2f`. Registry and node binary digests match the earlier review.

The candidate review used the earlier harness base as its declared identity. It does not identify these working-tree changes as an accepted executable revision.

The matrix now includes current hashes for 224 model artifacts. Workload pins remain null, and the matrix remains non-dispatchable.

PR #447 and PR #216 remain open and unmerged at the review time. No live authority, publication, or recovery qualification ran.

The user selected six reviewers: `leithaus`, `metaweta`, `spreston8`, `jltatbeach`, `dylon`, and `jeffrey-l-turner`. All six GitHub account identifiers resolve.

The current GitHub credential receives HTTP 403 from repository-role queries. The controller must still verify a current `maintain` or `admin` role before accepting approval.

OCI inspection found the `ci-runner` compartment and `f1r3node-ci-schedulers` application in `us-sanjose-1`. The namespace is `axd0qezqa9z3`.

The existing function schedules soak workflows. It is not the new lifetime supervisor. The compartment bucket and policy queries returned no records.

The [deployment proposal](../plans/casper-campaign-deployment.jsonc) records the exact reviewer identifiers, discovered resources, proposed resource names, and unresolved deployment fields.

The workflow uses the documented [OCI credential environment variables](https://docs.oracle.com/en-us/iaas/Content/API/SDKDocs/clienvironmentvariables.htm). This session printed no credential values.

## Acceptance and remaining work

Thirty-seven current artifact records remain pending and have compatibility links. Previous ledger identities remain recorded, and historical evidence packages remain unchanged.

Local model execution supports review of the bounded controller model. It does not discharge the planning claim, local reservation claim, execution claim, or Claim001.

The approval environment, authoritative object, access policy, and lifetime supervisor still require deployment. Timing guarantees and host capabilities require deployed evidence.

The node prerequisite, live adapters, executable workload pins, hosted checks, and source-bound acceptance remain required. No preflight or baseline dispatch occurred.

TASK-017-12 remains in progress. No node, cloud runner, remote configuration, or evidence release was created by this continuation.

## Hosted verification follow-up

The user requested TASK-017-12 completion after publishing `8adaa235c44cf55f93f5f236de21ae943bd0e1cc`. The branch has no uncommitted source changes at this review.

The [hosted control run](https://github.com/F1R3FLY-io/f1r3node-rust/actions/runs/35643918016) passed on Linux. The retained artifact matches its GitHub SHA-256 digest and byte count.

Both hosted source manifests agree. All 46 unique source hashes match this checkout. Each model log matches its retained digest.

The run passed 115 planner checks, 43 Rust tests, and five model configurations. These checks do not qualify the deployed campaign execution workflow.

The [hosted report](../casper/cbc-evidence/runs/casper-campaign-hosted-20260921-01/report.json) records run identity, artifact identity, source hashes, and verification results. It retains the previous failed run reference.

The GitHub artifact expires on 2026-10-21. The local archive remains under `target/task-017-12-hosted-20260921-01/` until durable external retention is verified.

The repeated strict audit returns exit 4. Its registry covers eight soak claims and excludes the three campaign claims. No claim status changed.

## Current completion blockers

Both `GITHUB_TOKEN` and `GITHUB_PERSONAL_ACCESS_TOKEN` contain the same value in this process. This result also holds in a non-login shell.

Explicit selection of each variable returns HTTP 403 for maintainer-role queries. The response advertises `metadata=read`, which corrects the earlier Administration-read diagnosis.

The repository metadata lists admin access. That response does not establish credential access to the role endpoint. The collaborator-list endpoint also returns HTTP 403.

The OCI bucket query again returns no records in the selected compartment. The authoritative object, access policy, supervisor, and timing evidence remain required.

The current `dev` revision remains `6940a5beb4aa806d3d75f6df3be9f238512fcc2f`. PR #447 remains open at the reviewed head `566a21223830eb6594467615eb45fe3e4ccb3c2e`.

The [node review](https://github.com/F1R3FLY-io/f1r3node-rust/blob/566a21223830eb6594467615eb45fe3e4ccb3c2e/docs/plans/casper-node-observation-batch-b.md) records unresolved Batch B1 findings. Batch B2 and Batch C still require separate approval.

The live adapters and workload pins depend on those node interfaces. Current source acceptance, deployment qualification, a separate passing preflight, and both full baselines remain required.

This follow-up changes evidence and task records only. It creates no node, cloud runner, remote configuration, commit, or claim acceptance.

## Source coverage correction

The user requested continued TASK-017-12 verification. Cargo dependency records exposed four build inputs absent from the earlier hosted manifest.

The omitted files were `src/host_control.rs`, `src/main.rs`, `src/manifest.rs`, and `src/runtime.rs` under `scripts/casper-soak/`.

The controller configuration also omitted these required pins and `src/models.rs`. The controller could therefore accept configuration without every library source pin.

The gate now records all crate-root Rust sources. The controller requires 28 source pins. A new test rejects configuration with any missing library source pin.

The test failed before the correction and passed afterward. Both results remain under `target/task-017-12-source-coverage-20260921-01/`.

The shared checkout advanced to `4dd7a20126410beb3dd88cabc09ba81d286ee058` during verification. That commit includes the correction. This session issued no staging, commit, or push command.

The first two gate attempts failed because the shell selected incompatible utilities. Native GNU utilities resolved those failures. Both failed attempts remain separate evidence.

The final gate passed 115 planner checks, 44 Rust tests, and five model configurations. The reservation subprocess helper remains the single expected ignored entry.

The 50 source hashes match before and after the gate. The manifest covers every repository dependency recorded for the controller, supervisor, and model verifier binaries.

The controller configuration requires every recorded repository dependency of the controller and supervisor. Targeted Clippy and crate formatting also passed.

The strict eight-claim audit still returns exit 4. Claim001 and the campaign claims remain pending. No deployed-service qualification or baseline execution occurred.

The [source coverage report](../casper/cbc-evidence/runs/casper-campaign-source-coverage-20260921-01/report.json) records dependency inventories, source hashes, results, and failed attempts. Three pending artifact records now identify the corrected sources.

The earlier hosted run covers `8adaa235c`, not the correction. Fresh hosted verification and the existing deployment and node prerequisites remain required.


## Restart checkpoint: 2026-09-22

The current base is `515a011598c9db66ff5a1b2b014b2a2894f7c409`. Local stability controls and the snapshot fixture correction remain uncommitted.

The user authorized both architectures for the later campaign. Each architecture receives one 64 GB runner with a 64-hour maximum lifetime.

The controller requires both full 24-hour baselines to pass before either 60-hour workload. It also requires confirmed baseline termination and exact prior run identifiers.

Five durable slots cover one preflight, two baselines, and two stability runners. Consumed slots cannot be reused or replaced.

The authoritative object uses schema version 2. Version 1 and incomplete slot sets fail validation. No deployment or migration occurred.

The user directed publication qualification to wait for the actual PR #216 merge. Signature-keyed records cannot replace occurrence records.

PR #216 remains open at `619beb4a4a7ad3f8967d4586daf0f5c552bd150e`. This branch must merge before PR #216 integrates.

The phase correction below replaces unconditional publication admission. It preserves the approved merge order.

The local [stability report](../casper/cbc-evidence/runs/casper-campaign-stability-20260922-01/report.json) retains source hashes, test logs, model traces, and failure evidence.

The final gate passed 115 planner checks, 48 Rust tests, and six model configurations. All 51 source hashes match before and after verification.

The new model control detects incomplete prerequisites. The tests reject failed baselines, changed resources, reused run identifiers, incomplete records, and shortened workload coverage.

Targeted Clippy and workflow checks passed. The strict eight-claim audit returns exit 4, with Claim001 pending. Campaign claims remain pending separately.

The Batch B1 snapshot tests passed: 24 capture tests and 19 reader tests. The fixture now resolves its temporary directory before comparing storage identities.

The retained first attempt failed because sandbox access blocked LMDB. The second attempt exposed three macOS path comparisons involving `/var` and `/private/var`.

The correction changes only the test fixture. Native tests do not provide isolated-build evidence, formal acceptance, or live adapter qualification.

[Hosted run 35736460538](https://github.com/F1R3FLY-io/f1r3node-rust/actions/runs/35736460538) passed at the committed base. That run does not cover these local stability changes.

The user will load the updated GitHub credential and restart Codex. This process cannot verify the replacement credential before that restart.

### Work after restart

1. Verify that the new credential can read repository roles.
2. Review and publish the local changes through the requested Git procedure.
3. Run hosted verification for the published stability revision.
4. Complete the source-bound Batch B1 review and the Batch B2 observer prerequisites.
5. Review the verified pre-merge admission scope and its explicit deferred profiles.
6. Complete the node claim evidence and named maintainer acceptance.
7. Review PR #447 and the dependent PR #436 target sequence.
8. Refresh the campaign model inventory and workload pins before candidate qualification.
9. Qualify both candidate images and all required live adapters.
10. Provision and qualify the authoritative object, access policy, supervisor, and timing bounds.
11. Verify workflow evidence delivery across the full 24-hour and 60-hour workloads.
12. After all admission requirements pass, run the separate preflight and both full baselines.
13. After both baselines pass and terminate, run both approved stability workloads.
14. Complete this branch review and merge through the approved Git procedure.
15. After this branch merges, integrate PR #216.
16. Under EPIC-018, review Batch C and qualify publication and recovery against actual occurrence records.

The candidate matrix remains non-dispatchable. Its earlier campaign model hashes require review against the six current configurations before final pinning.

Items 2 and 3 from the branch review remain incomplete overall. The admission phase mismatch is corrected. PR #216 is not a branch prerequisite.

Credential verification requires the restart. Occurrence-dependent qualification remains pending for EPIC-018.

No node, runner, remote configuration, commit, push, or claim acceptance occurred in this checkpoint.


## Merge-order correction

The previous response incorrectly treated deferred publication qualification as a prerequisite for this branch. The user corrected that interpretation.

The approved order is this branch first, then PR #216 integration, then occurrence-dependent qualification under EPIC-018.

The plan, task tracker, and restart checklist now preserve that order. The implementation checkpoint below supplies the corresponding phase distinction.

This correction changes records only. It does not bypass admission, alter recorded test results, or report deferred qualification as passed.


## Phase correction implementation

The user authorized the admission correction. Pre-merge execution now accepts the qualified `casper-authority-finality` workload and requires passing authority/finality results.

Plans, workloads, and qualification records explicitly defer publication and recovery to `post_pr216_merge`. The controller checks the phase and profile scope before reservation.

Passing worker reports must retain pending publication and recovery verdicts. The finalizer preserves these verdicts. Prior-run checks enforce them before baseline or stability advancement.

The controller rejects combined publication workloads, legacy load execution, missing deferrals, changed phases, and claimed deferred passes. No occurrence identities are inferred from signatures.

The [phase correction report](../casper/cbc-evidence/runs/casper-campaign-phase-20260922-01/report.json) records 133 planner checks, 51 Rust tests, and six model configurations.

The two new phase regressions failed before correction. The final gate, targeted Clippy, and formatting checks passed after correction.

The first standalone planner attempt failed because the sandbox denied a fixture write to `/dev/stdout`. The complete gate passed outside that sandbox.

The candidate matrix remains non-dispatchable. Live authority qualification, workload pins, node review, source acceptance, deployment qualification, and actual campaign execution remain incomplete.

No hosted run covers this local correction yet. No node, cloud runner, remote configuration, commit, or push occurred.


## Commit deviation cleanup

Commit `0de118fcf` recorded four deviations. The cleanup leaves only `report.json` in each new campaign evidence package.

Both reports remain byte-identical. Their existing evidence references and digests remain valid. The cleanup removes 51 bulk files, including all 20 files with home-directory paths.

The original bulk bytes remain under `target/task-017-12-deviation-cleanup-20260922-01/bulk/`. This local copy does not establish durable external retention.

The snapshot test and its node ledger now match PR #447 head `6ea6bf029`. The harness branch no longer owns the Mac path correction.

The prepared patch is `target/task-017-12-deviation-cleanup-20260922-01/pr447-snapshot-path.patch`. It contains the test correction, its node ledger, and a separate node report.

The patch applies cleanly against the PR head. Its report identifies the previous native test run and does not claim new execution or acceptance.

The tracker now records the credential result from commit review. The token variables differed, and the classic token returned the repository role.

Publishing the node patch requires explicit commit and push consent. The campaign cleanup remains uncommitted.

## Published revision verification: 2026-09-22

The user published `58e952c6fc780d10ee9a28f19ae563245de4a39e`. The checkout was clean at this review.

The [hosted campaign run](https://github.com/F1R3FLY-io/f1r3node-rust/actions/runs/35752941906) passed for that revision. It covers the stability controls and the corrected admission phase.

The downloaded archive matches its GitHub digest and byte count. Every extracted file matches the archive, and both source manifests agree.

All 51 unique source hashes match this checkout. All six model logs match their recorded digests and expected outcomes.

The run passed 133 planner checks, 51 Rust tests, and six model configurations. The reservation helper remains the single expected ignored test.

The [compact report](../casper/cbc-evidence/runs/casper-campaign-hosted-58e952c6f-01/report.json) records the archive, source identities, results, and prerequisite review. Bulk evidence remains under `target/task-017-12-hosted-58e952c6f/`.

The hosted artifact has a recorded expiration date. Durable external retention remains incomplete.

Explicit `GITHUB_PERSONAL_ACCESS_TOKEN` selection verified all six configured reviewer roles. Five reviewers have the `admin` role, and `dylon` has the `maintain` role.

Default `GITHUB_TOKEN` selection still returns HTTP 403 for role queries. The initial sandbox attempt also failed to connect. The retry used permitted network access.

The campaign environment remains absent. Repository activation variables remain absent. The deployment proposal now records the verified roles and removes the resolved credential blocker.

The candidate matrix contained seven stale campaign hashes and omitted the prerequisites configuration. The matrix now includes the current hashes for all six configurations.

All 225 model hashes and four configuration hashes match. Inventory acceptance, executable workload pins, and live qualification remain pending.

Current `dev` is `b465313a2d490d24b6f2e55447bd81efc0912e10`. Existing image verification covers `6940a5beb`, so candidate identities require refresh before dispatch.

PR #447 remains open at `4561e064a70b495fe07cbcf779bff375d636aaad`. Its two latest commits change task and review records only.

The [node verification record](https://github.com/F1R3FLY-io/f1r3node-rust/blob/4561e064a70b495fe07cbcf779bff375d636aaad/docs/work-logs/task-019-4-node-claim-verification.md) reports missing formal evidence and a new B1 lock-wait finding. This continuation did not independently verify that finding.

TASK-019-4 owns the node verification. Batch B2 planning waits for source-bound verification and named maintainer acceptance of Batch A and B1.

The strict eight-claim audit returns exit 4. Claim001 remains pending, and claims 002 through 008 remain discharged. The separate campaign claims remain pending.

The environment request and branch policy are prepared under the local evidence directory. The request retains all six reviewers, prevents self-review, and disables administrator bypass.

The branch policy allows only `formal/soak-casper-consensus`. The execution-control plan requires separate authorization before remote configuration changes.

The next deployment step is creation and verification of this environment. The authoritative object, access policy, supervisor, and timing qualification remain separate prerequisites.

Source hashes, report links, and environment request checks passed. The strict audit result is byte-identical before and after these changes.

The STE Check passed for the added prose. The whitespace check also passed. Human STE Review remains necessary.

No node, cloud runner, remote configuration, commit, push, or claim acceptance occurred in this continuation.

## Campaign environment creation: 2026-09-22

The user authorized creation of `casper-campaign` from the prepared JSON requests. The environment and its single branch policy are now configured.

The environment ID is `22500101516`. The branch policy ID is `60724033`.

The readback confirms all six configured reviewers, self-review prevention, disabled administrator bypass, and no wait timer. Deployment is restricted to `formal/soak-casper-consensus`.

The first creation request returned HTTP 403 with the default credential. Explicit `GITHUB_PERSONAL_ACCESS_TOKEN` selection completed both writes.

The [configuration report](../casper/cbc-evidence/runs/casper-campaign-environment-20260922-01/report.json) retains the request digests, settings, authorization, and verified readback. It records the rejected request separately.

The deployment proposal and candidate blockers now reflect the configured environment. The authoritative object, access policy, supervisor, and deployed qualification remain incomplete.

No campaign workflow, node, or cloud runner started. Claim acceptance remains pending. This continuation created no commit or push.
