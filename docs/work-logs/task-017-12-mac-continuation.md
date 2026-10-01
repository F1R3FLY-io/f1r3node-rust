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

## OCI reservation preparation: 2026-09-22

The user requested completion of the OCI reservation preparation. The [deployment procedure](../plans/casper-campaign-storage.md) and [configuration](../plans/casper-campaign-storage.jsonc) are ready for review.

The live inventory returned no buckets or policies in `ci-runner`. Its parent is the tenancy. The review covered all eight returned tenancy policies.

The proposed service user has one returned group membership in `ci-runner-launchers`. Its current grants cover compute and network resources, with no storage grant.

The policy restricts read and overwrite permissions to the fixed reservation object. A separate statement permits the controller to read its named policy.

The local operator has administrator access and cannot supply runtime qualification evidence. The workflow secret identity still requires confirmation against the proposed service user.

The supervisor function and its dedicated dynamic group remain absent. The prepared dynamic-group request requires the exact function identifier before it can be submitted.

The schema version 2 template contains all five empty slots. Its campaign identifier and configuration digest remain unresolved because the accepted controller configuration is incomplete.

The controller source requires that final digest in the authoritative record. Uploading a placeholder or an earlier configuration would fail the binding check.

The [preparation report](../casper/cbc-evidence/runs/casper-campaign-storage-preparation-20260922-01/report.json) records these findings and the request digests. No live policy or conditional-update test ran.

The permission design does not make IAM validate entity tags or reservation transitions. Trusted writer code and credential isolation remain required.

Two local Rust checks passed against the current controller source. They verify the policy digest and the five-slot record template with synthetic configuration bindings.

The checks reject the unbound template, missing or additional slots, an old schema, and changed campaign bindings. They do not qualify OCI services.

The first digest check detected an omitted trailing newline. The prepared digest now uses the controller encoding. The report retains that failed check.

The next deployment action requires the prepared OCI resource authorization. Object initialization must wait for the final controller configuration and supervisor identity.

No OCI resources, credentials, campaign runs, or claim statuses changed. This continuation issued no staging, commit, or push command.

## Workload pinning attempt: 2026-09-22

The user requested workload pinning and baseline execution. The [readiness report](../casper/cbc-evidence/runs/casper-campaign-readiness-20260922-01/report.json) records the current blockers.

Current `dev` is `b465313a2d490d24b6f2e55447bd81efc0912e10`. Docker Hub returned no tag matching the resolver query for this revision.

CI run `35677113121` failed the amd64 subprocess validator lifecycle test. The grow-and-shrink phase reached its timeout without a terminal verdict for one of three deploys.

This result does not establish deploy loss. The image release job was skipped. Unpublished build artifacts remain available.

PR #447 remains open at `4561e064a70b495fe07cbcf779bff375d636aaad`. The authority observer under TASK-019-3 and source acceptance under TASK-019-4 remain prerequisites.

The current authority profile rejects every live request with `live_adapter_unqualified`. Its implementation requires a live adapter after the node interface is available.

The repository has no campaign activation variables. The authoritative object and independent supervisor remain undeployed.

The task now lists its node prerequisites explicitly. Existing candidate pins remain unchanged because the current revision has no matching published tag.

No executable workload was qualified or pinned. No campaign workflow or runner started. Both baseline runs remain pending.

## Integration test correction: 2026-09-22

The user requested correction of the failed integration test first. The [correction report](../casper/cbc-evidence/runs/casper-integration-timeout-fix-20260922-01/report.json) records the source and validation results.

The downloaded artifact matches its GitHub digest. Its test source matches the pinned suite revision, `b3d14b27e3c6276b1eb4ab9ccef04e02b0c4e283`.

All eight nodes wrote a `Finalized` verdict after about 137 seconds. The resolver stopped polling after 135 seconds. Finalized floor heights advanced during the wait.

The PoS mutation helper now includes inclusion and finalization time budgets. The default limit is 225 seconds per deploy and attempt.

The recorded timing regression failed at three scales before the change. All 39 targeted tests passed after the change. Ruff and whitespace checks passed.

The checks retain failures for unresolved deploys, terminal failures, and conflicting verdicts. They also verify concurrent submission, expiration-only resubmission, and the attempt limit.

The fix and regression files are applied in the sibling `system-integration` repository. The destination files match the verified files from the isolated checkout.

The full integration test has not rerun. The fix requires publication before this repository can select its immutable suite revision. No commit or push occurred.

## Authority client scope: 2026-09-28

The user requested TASK-017-12 continuation on `formal/soak-casper-consensus`. The starting revision is `211a4e73a6c1c8c4e4d3de35f2debdf1869ab74b`.

The node observer exposes detached authority evaluation. The harness has no client for that protocol. Its existing profile rejects live requests.

The first implementation adds three files under `scripts/casper-soak`: `src/authority_observer.rs`, `src/bin/casper-authority-observe.rs`, and `tests/authority_observer.rs`.

The client checks process identity, request correlation, frame limits, and deadlines. It retains raw evidence for later qualification.

The [client claim](../claims/casper-authority-observer-client.md) remains pending. The existing mandatory attributes cover all three files.

The synthetic profile requires fixture loading and observations that detached evaluation does not provide directly. Raw capture cannot satisfy those requirements or enable campaign admission.

TASK-017-13 preparation proceeds separately. Final claim review and handoff depend on TASK-017-12 evidence.

### Client behavior and use

The client connects to an existing Linux observer socket. The client and node must share a process namespace and effective user identity.

The node configuration must permit the client process and its start time. A different client process cannot reuse that permission.

The binding contains nine fields: `socket`, `node_pid`, `process_start_ticks`, `source_revision`, `executable_sha256`, `configuration_sha256`, `approved_request_sha256`, `request_id`, and `timeout_ms`.

Use trusted launch records for the expected node identity. Do not derive the expected executable or configuration digest from an unverified greeting.

Supply the existing `AuthorityRequest` object as the authority file. The client forwards that object and checks the response copy without interpreting its evaluation results.

```bash
cargo build --locked -p casper-soak --bin casper-authority-observe
target/debug/casper-authority-observe \
  --binding binding.json \
  --authority authority-request.json \
  --output capture-001
```

The output directory must not exist. The client creates a private directory and retains input, greeting, request, response, and report files.

Exit zero means that the client captured a correlated response. An unavailable evaluation can still produce that exit. The report always keeps qualification pending.

Exit two means that input validation or capture failed. The client retains completed captures when a later protocol check fails.

The report identifies the client executable and compiled helper sources. Socket framing uses the production protocol's length prefix and frame limit.

### Verification and remaining work

Eight tests pass in an isolated Linux container with networking disabled. Four portable tests pass on the native host. All seven existing authority profile tests pass.

The Linux tests reject wrong process identities, changed executables, oversized frames, duplicate keys, truncated frames, replayed responses, and expired deadlines.

The timeout test also sends small fragments repeatedly. The common deadline expires even when individual reads receive data.

Native and Linux Clippy checks pass with warnings denied. The verification package records source digests, command outcomes, and retained log digests.

The first compile failed because this crate does not enable Serde derive macros. The binding parser now uses the existing checked JSON helpers.

The first Linux run exposed an incorrect binding field count. A later timeout assertion included evidence-file hashing and exceeded its one-second test allowance.

The final test separates the 500-millisecond socket deadline from a five-second allowance for evidence output. The client deadline remains unchanged.

These controlled sockets do not execute a blockchain node. No real candidate has been qualified, and no baseline or stability campaign has started.

The selected candidate still needs an observer-capable immutable image. Live scenario mapping must account for fixture loading, expected heads, finality, and unavailable fault controls.

All 225 model hashes match their matrix entries. The `.github/oci-validation.env` entry differs after the inherited suite update and needs source-bound reconciliation.

The authoritative reservation object, independent supervisor, deployment qualification, and campaign claim acceptance remain prerequisites. TASK-017-12 stays in progress.

### Live profile interface gaps

The source review found four gaps between the accepted profile and the available node interface.

| Profile requirement | Available node interface | Consequence |
| --- | --- | --- |
| Applied fixture identity and step receipt | `AuthorityRequest` selects an existing captured DAG. It has no fixture-loading operation. | A capture digest cannot establish that the requested fixture was loaded. |
| Selected head observation | `AuthorityResponse` provides target evaluations and a floor result. | A floor hash cannot substitute for a selected head. |
| Original fault tolerance and display projection | `TargetResult` exposes the original value as floating-point bits. The projection always reports `equivocation_snapshot_unavailable`. | Conversion needs an explicit contract. The client cannot invent the missing projection. |
| Fault application receipts | The observer advertises `fault_control` as unsupported. | Fault scenarios remain blocked. |

The measured and reference results share a captured input. That property alone does not satisfy the profile's fixture, head, projection, and fault requirements.

Node interface changes belong to the separate prerequisite. Live qualification requires those interfaces or a separately reviewed profile contract. This continuation changes neither contract.

The three new client artifact records resolve through the shared checker. Its strict check returns exit four with three pending records and no missing records.

## Campaign inventory reconciliation: 2026-09-28

The candidate matrix now records the inherited integration suite at `e3c4e14189f0c6ced2e9674487fcbdeffd93141b`.

PR #450 supplied this update through merge `0b9ae5bcec94a2df8f6112bbbc6c950ad2e603b2`. That merge is an ancestor of the current branch.

The GitHub comparison identifies six commits after the previously recorded suite revision. The validator lifecycle resolver now includes deploy inclusion in its settlement budget.

The comparison shows no change to the launcher or load-test entrypoint. The compose change updates a documentation reference.

The matrix configuration digest now matches `.github/oci-validation.env`. All three workflow pin sites select the same suite revision.

All 225 model hashes and four configuration hashes match their current files. The repository workflow security checks pass.

The [reconciliation report](../casper/cbc-evidence/runs/casper-campaign-inventory-20260928-01/report.json) records the source hashes, comparison, and remaining limits.

Historical reports retain their original bytes. Their recorded suite revisions describe earlier verification and do not qualify the updated campaign.

Candidate image pins and null workload pins remain unchanged. The matrix remains non-dispatchable, and source-bound acceptance remains pending.

The live adapter still requires the node interface additions listed above. This inventory correction does not supply those interfaces or qualify a candidate.

## Campaign control renewal: 2026-09-28

The [renewal report](../casper/cbc-evidence/runs/casper-campaign-renewal-20260928-01/report.json) binds the current 52-file inventory to renewed local verification.

The campaign model and five negative controls pass with the pinned TLC verifier. All three model-runner regression tests pass.

The controller and supervisor pass 32 tests. All 16 reservation tests pass in an isolated Linux container.

The planner passes 133 admission and duration checks. Its controlled API fixtures launch no node or cloud runner.

The first aggregate attempt failed because the native PATH selected incompatible utilities. A later retry omitted cargo, and sandbox restrictions blocked a fixture output write.

The retained component reruns use GNU utilities and preserve the earlier failures. The first aggregate report remains failed.

The source inventory stayed unchanged through verification. The current lockfile adds the node dependency `paste`, and the workflow contains the inherited integration-suite update.

The workflow evidence record now has its current source digest and the renewal report digest. Its prior records and pending claim status remain intact.

The strict eight-claim audit returns exit four. Claim001 remains pending, claims 002 through 008 remain discharged, and all eight soak fields remain pending.

The GitHub review confirms `dev` at `0b9ae5bce`. PR #447 remains open at `670037c25`, with `fix/node-log-and-accept-backoff` as its base.

This renewal supplies local verification evidence. It does not deploy OCI controls, qualify the node interface, or establish a passing campaign.

## Authority interface mapping correction: 2026-09-28

The [mapping report](../casper/cbc-evidence/runs/casper-authority-mapping-20260928-01/report.json) corrects the earlier four-gap assessment. That assessment assigned too much work to the node prerequisite.

This review uses branch revision `2fb0686cf38752a0251af6d1d3e26afebb6d9eb9`. It binds 17 local source files and four external provider files by digest.

The external files come from the selected integration suite revision `e3c4e14189f0c6ced2e9674487fcbdeffd93141b`. The review did not use the older sibling checkout.

### Process controls and receipts

Ordinary pause and restart operations belong to TASK-017-12 on `formal/soak-casper-consensus`. They do not require the observer's unsupported internal fault-control capability.

The Docker provider supports pause, unpause, restart, process inspection, and exit observation. The owned subprocess provider supports signal-based pause, unpause, and process replacement.

The adopted subprocess handle cannot restart its process. Its inferred exit value also cannot establish the actual exit status.

The harness must bind each receipt to the scheduled fault, request, process, and deadline. It must retain observed process state and restart evidence.

Restart evidence must include the previous process exit, readiness, and the new observer incarnation. Command success alone does not establish fault application.

A fault at an internal node operation remains a separate capability. The ordinary process schedule does not establish that capability.

### Paired fork-choice observations

The public `showMainChain` API already invokes the live estimator and exposes its selected chain. The earlier statement about absent selected-head observations was too broad.

However, this API does not bind its response to an observer snapshot or evaluation mode. Separate live calls cannot establish the required shared input.

The observer compares oracle and floor results on one detached capture. It does not run paired fork-choice evaluations or return their selected heads.

EPIC-019 on `feature/casper-node-observation` owns the required observation extension. The extension must bind both fork-choice modes, estimator configuration, results, and work to one capture.

A finalized floor cannot substitute for a selected head. Existing oracle reference results cannot substitute for a fork-choice reference result.

### Display projection and numeric values

The public block API already reports display fault tolerance. Its calculation subtracts initial fault derived from the live equivocation tracker.

The detached observer snapshot does not contain that tracker. Its display projection explicitly reports `equivocation_snapshot_unavailable`.

EPIC-019 owns capture of the equivocation inputs and their use in the detached display calculation. A separate public block response lacks this capture binding.

TASK-017-12 owns the numeric mapping. The observer preserves floating-point bits, while the accepted profile compares rational values.

A rounded floating-point value must not become its ideal mathematical fraction. The mapping must preserve the original bits and handle values outside the rational schema.

### Fixtures, finality, and work

The accepted synthetic inputs contain opaque fixture JSON. They are not executable block fixtures with valid signatures and complete node state.

Existing deploy and propose operations support ordinary workloads. Their success does not establish malformed signatures, duplicate justifications, or controlled missing dependencies.

TASK-017-12 must define executable scenarios and retain evidence of their applied inputs. This requirement does not establish a need for a production fixture-loading endpoint.

The observer returns capture digests and counts. It does not return the complete canonical DAG, electorate, and justification artifacts required by the current profile input binding.

The harness needs a reviewed mapping between those input identities. A bounded node export or additional digests might be necessary after that contract is defined.

Finality mapping must distinguish oracle decisions, floor outcomes, live events, and persisted state. An oracle threshold result alone does not establish finalization.

The mapping must also preserve threshold precision and the difference between strict and inclusive comparisons. Holds and unavailable observations must remain explicit.

Work counters require defined counting sites and evaluation scope. Metadata reads do not necessarily count distinct visited vertices, and traversal operations do not necessarily count edges.

TASK-017-12 owns these mappings. EPIC-019 owns additional node instrumentation only where the agreed measurement requires it.

### Remaining implementation and verification

The next harness work defines executable fixtures, input bindings, numeric conversion, finality interpretation, and counter semantics. Process receipts can use the existing providers.

The node dependency comprises paired fork-choice observations and captured display-projection inputs. Further input exports or counters require a specific contract before implementation.

The live adapter must then connect qualified observations to the profile. Its current `live_adapter_unqualified` rejection remains necessary until that path has separate verification.

The [validation record](../casper/cbc-evidence/runs/casper-authority-mapping-20260928-01/validation.json) confirms source digests and external Python syntax. This source review executed no provider operations and establishes no live qualification.

TASK-017-12 remains in progress. Candidate qualification, executable workload pins, campaign service qualification, required acceptance, preflight, and both full baselines remain outstanding.

## Authority adapter implementation: 2026-09-28

The [adapter report](../casper/cbc-evidence/runs/casper-authority-adapter-20260928-01/report.json) records the implemented observation mapper, process recorder, and verification results.

The observer client now retains `mapping.json` with the raw transport artifacts. Mapping errors produce a separate rejected record without erasing the captured response.

The mapper preserves binary32 bits and converts supported values to exact fractions. Nonfinite values and fractions outside the profile schema retain explicit absence reasons.

The mapper checks exact oracle witnesses and preserves the selected threshold comparator. It separates persisted finality from oracle decisions and detached floor results.

Work counters retain their original names and request scope. Missing vertex and edge measurements remain missing.

The Python process recorder uses the existing owned subprocess controls. It verifies the predecessor capture and process identity before invoking the provider.

Pause requires an observed stopped state and records resume cleanup separately. Restart requires observed exit, a replacement child, and a new observer incarnation.

The replacement must preserve the source revision, executable digest, and configuration digest. A late receipt cannot report successful application.

The recorder accepts only an owned subprocess handle. Docker and adopted-process execution remain outside this implementation.

Readiness means an available authority endpoint capture. The recorder does not establish network convergence, provider timing bounds, or full candidate qualification.

The new adapter workflow runs nine Linux client tests, eight mapping tests, and ten process receipt tests. All 27 tests pass locally in Linux.

The numeric tests include 20,000 deterministic binary32 samples. Native client and mapping tests pass, and all seven existing authority profile tests pass.

Linux and native Clippy checks pass. Workflow security checks, Rust formatting, shell syntax, and whitespace checks also pass.

The local Linux gate verifies unchanged source digests. No hosted run of the new workflow has occurred.

The client and adapter records remain pending. Controlled processes and fixture observer captures do not qualify a blockchain node.

The current PR review confirms that preflight and both full baselines remain completion requirements. The later 60-hour phase remains a post-merge obligation.

PR #447 remains open at `670037c2511abd5f576063b3153681a873244a18`. Its head does not supply the additional observation contract identified in the source mapping.

Executable scenarios still need applied fixture identities, captured input bindings, and exact traversal measurements. The accepted live profile remains blocked until these bindings exist.

The node branch owns paired fork-choice results and captured display-projection inputs. This continuation made no node changes.

Campaign service deployment, selected candidate qualification, source-bound acceptance, preflight, and baseline execution remain outstanding. TASK-017-12 remains in progress.

## Executable scenario bindings: 2026-09-28

The [execution report](../casper/cbc-evidence/runs/casper-authority-execution-20260928-01/report.json) records the completed harness binding and current verification results.

The production `execute` command runs a pinned provider executable with the generated scenario operations. It retains the inputs, request, output, and receipt chain.

Each applied receipt must identify the exact operation and retained input bytes. Observation records must match the request and member identities.

The binding sends final evaluations and fault acknowledgments to the existing collector and classifier. Missing steps, missing evaluations, and unapplied receipts cannot pass.

Controlled execution covers all nine scenario kinds. Rejection tests cover replayed requests, reordered steps, broken chains, late receipts, changed inputs, and incorrect executable digests.

A partial receipt inventory preserves an observed product failure after executor failure. Timeout and blocked-admission tests verify process cleanup and launch prevention.

Native and Linux verification each pass 12 Rust tests. The ignored provider helper executes as a child process through the production command.

The threshold test checks 2,000 cases. The clean model and three negative controls pass. Native Clippy denies warnings and passes.

The final native wrapper uses ARM Java and test optimization level zero. The optimized native dependency build failed, and the evidence retains that failure.

The sandbox blocked the TLC local listener. The approved external run passed with identical source inventories before and after verification.

Claim002 now remains pending because its accepted implementation changed. All 13 current artifact records retain matching source, specification, and evidence digests.

The 12 previously accepted records preserve their historical acceptance. The strict audit returns exit 4 for pending claims instead of exit 2 for refusal.

These fixtures launch no blockchain nodes and dispatch no cloud resources. Provider receipts remain assertions until a live provider receives qualification.

The live provider must supply mapped observations, captured input exports, traversal measurements, and process receipts. The other agent owns the node observation additions.

Renewed acceptance and hosted verification remain pending. Candidate qualification, campaign services, preflight, and both full baselines still prevent TASK-017-12 completion.

## Live qualification executor: 2026-09-28

The [live executor report](../casper/cbc-evidence/runs/casper-authority-live-20260928-01/report.json) records the implementation and controlled Linux verification.

The new `casper-authority-live` executable consumes the execution envelope. It verifies the observer identity before each workload operation and captures the node afterward.

A pinned native driver supplies fixture operations and input exports. The executor checks the exported bytes and captured snapshot digest before emitting an applied receipt.

The executor constructs observations from the node response. It preserves separate bounded and reference decisions, exact numeric values, and persisted finality.

Missing heads, traversal measurements, and projection values remain missing. A null driver permits captures but produces unknown receipts and an incomplete execution.

Atomic inventory updates preserve completed receipts after a later failure. The timeout test confirms that the driver process exits and leaves a valid partial inventory.

Eight live executor tests pass on Linux. The native fixture helper executes through the production binary during those tests.

All 27 existing client, mapping, and process receipt tests also pass on Linux. Native and Linux Clippy checks pass.

The first full gate exposed a malformed PID test fixture. The corrected fixture and final gate pass, and the evidence retains the failed gate.

The observer module now exposes its mapper to the live executor. This visibility change lets both paths use the same conversion logic.

The existing adapter workflow now runs the live gate. The changed workflow and observer records remain pending with refreshed source digests.

This implementation does not supply a candidate-specific fixture driver. The node observer currently lacks paired heads and the required exact input and traversal exports.

Fault schedules remain blocked before driver launch. Connecting the process recorder requires an adapter that binds the predecessor and replacement incarnations.

The [provider guide](../casper/design/authority-live-executor.md) defines the operation protocol and remaining integration work. Full live execution and TASK-017-12 remain incomplete.

## Provider transport and process adaptation: 2026-09-28

The live driver now submits pinned block bytes through the current Rust peer transport. The older integration client supplied the operation pattern.

The process owner starts a pinned native child. Its private socket supports pause, restart, and observer capture requests.

The node authorizes the persistent owner PID and start ticks. The executor receives captures through that owner and retains their verified artifact bytes.

Restart handling verifies the predecessor socket identity, observes child exit, removes that same socket, and starts the replacement. The executor verifies successor readiness.

Explicit `observed_restart` enrollment supports the random incarnation that the node generates. Receipt validation and classification bind subsequent observations to that successor.

The Linux gate passes 27 tests and Clippy. The profile gate passes 13 Rust tests, 2,000 threshold cases, and four model controls.

The profile gate needed an approved rerun because the sandbox denied the TLC listener. The failed attempt remains in the local evidence directory.

The [provider report](../casper/cbc-evidence/runs/casper-authority-provider-20260928-01/report.json) records the sources and verification scope. It preserves the preceding records.

The source and claim records remain pending. The strict claim audit returns exit 4, with claims 001 and 002 pending and claims 003 through 008 discharged.

The shared artifact audit reports 26 pending mandatory artifacts. It reports no stale discharge refusal.

The [provider guide](../casper/design/authority-provider-adaptation.md) documents configuration and use. The adapter workflow includes the transport, process, live executor, and classifier checks.

Candidate qualification remains outstanding. It needs prepared block histories, captured input exports, and the missing node observations.

The driver reports unknown application status after transport delivery. A transport acknowledgment cannot prove block validation or DAG admission.

This work does not start a blockchain candidate or dispatch cloud resources. Docker process controls remain outside this implementation.

## Live executor security review: 2026-09-28

The maintainer identified CodeQL alert 41 (`security/code-scanning/41`, an authenticated page) through this [PR review comment](https://github.com/F1R3FLY-io/f1r3node-rust/pull/436#discussion_r4128609240).
The rule reports a hard-coded cryptographic value at `scripts/casper-soak/src/authority_live.rs:769`.
The cited statement initializes `capture_attempt` to zero.

The capture call combines the execution digest, step index, capture phase, and retry counter.
The capture function hashes that string to derive `binding.request_id`.
The execution digest covers the complete envelope, including `execution_nonce`.
The normal executor obtains fresh bytes from `/dev/urandom` before it constructs that envelope.
The node observer separately supplies a challenge and checks the returned challenge.

This source review does not close the finding.
The code-scanning alert API returned HTTP 403, so the full CodeQL trace remains unavailable.
TASK-017-12 must verify freshness across captures, retries, and executions, including direct live executor calls.
The task must then record a correction or an evidence-supported false-positive assessment.
The alert remains pending review. This note changes no source artifact, evidence acceptance, or claim status.
