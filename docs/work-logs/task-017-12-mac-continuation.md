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
