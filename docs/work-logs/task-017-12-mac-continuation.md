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
