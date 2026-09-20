# TASK-017-12 Preparation

---
handoff_status: in_progress
handoff_to: pi-soak-carrier-index-linux
handoff_at: 2026-09-19T19:20:00Z
handoff_note: ../handoffs/claude-session-9f19b46c--pi-soak-carrier-index-linux--20260919T192000Z.md
next_steps:
  - Read the dispatch preconditions in this log before any dispatch.
  - Repin both candidates with scripts/ci/resolve-dev-candidate.sh against the dev commit current at that time.
  - Qualify the live adapters for claims 002 to 004 against a real node, using the checklist below.
  - Run the preflight-only dispatch as the first step of dispatch, not before it. It spends OCI runner time.
  - Run the approved baseline soak only after the preflight passes.
---

## Scope

This log records preparation for TASK-017-12. It covers the repin procedure, a resource proposal, an adapter-qualification checklist, and an infrastructure dry-run plan.

Workload pins, adapter qualification runs, and the baseline soak wait for TASK-017-8 through TASK-017-11 and for a Linux agent. The [drift review](./task-017-12-drift-review-2026-09-19.md) is the input to this preparation.

## Candidate identity resolution

CI publishes each `dev` push to Docker Hub under two tag forms. The `:dev` tag is a mutable pointer. The `dev-<git describe>` tag is immutable and names the commit.

A candidate must cite the immutable tag. The procedure is a script that takes the dev commit and the harness revision and returns one matrix entry per platform.

1. Find the immutable tag on Docker Hub whose name ends in the commit's short hash.
2. Read the tag's manifest list and take each platform's manifest digest.
3. Pull each platform image by digest and read its config digest.
4. Extract `/opt/docker/bin/node` from each image without running it and hash the binary.
5. Emit the entries with `workload_configuration_status` blocked until profile requests exist.

The script is committed at [`scripts/ci/resolve-dev-candidate.sh`](../../scripts/ci/resolve-dev-candidate.sh). It needs curl, python3, and a Docker daemon with buildx. It reads only. It pulls images and copies one file out of each, and it never runs a node.

The script is not a mandatory artifact and has no ledger record. The binding inventory does not copy it, so a later edit does not disturb CLAIM-CASPER-SOAK-001.

### Dry run at dev `6940a5beb` (2026-09-19)

| Field | dev-amd64 | dev-arm64 |
| --- | --- | --- |
| Image tag | `f1r3flyindustries/f1r3fly-rust:dev-v0.4.46-canary.1758-183-g6940a5be` | same |
| Platform manifest digest | `sha256:607751b4db7ecf3e1e3354b2163bf84b5cfb918ad1a780539b374f3ada368663` | `sha256:4f944006f0f5288bc10fe0a2ee1bea7b60728982c2a2ca4278d5aa43d12f743c` |
| Image config digest | `sha256:bd1d291e690f256a022bb7c684cd7cb5fbdc17056ec1fe4cce60bdb606b18cc2` | `sha256:6af530f8c3b0291dce60003f4dae04cf67295aca8036f113f6668fdb5ea6ed73` |
| Node binary digest | `sha256:5e3c3b3181fb24e04ee1e1da183c4bf03069bbca31e7ad9e4bbda7e3215863af` | `sha256:b078605600cbfda69922cbf32adb60512d93c9917c07fa56669b7ba03d4936f3` |
| Node binary bytes | 76,260,512 | 74,792,168 |

CI run 35423287293 built this commit and succeeded. The matrix is not updated by this dry run. The values will be stale by dispatch and are recorded to prove the procedure.

The matrix's existing entries also cite a CI artifact identity record. The executing agent decides whether to keep that field or to rely on the registry digests above.

## Resource proposal, approved

A maintainer approved this proposal on 2026-09-19. The approval covers the table below.

A second repetition requires a new decision. The 60-hour stability soak requires a passing baseline and a new decision. The approval does not authorize live adapter use, policy activation, or post-merge execution.


| Item | Proposal | Basis |
| --- | --- | --- |
| Candidates | dev-amd64 and dev-arm64, repinned at dispatch | Matrix authority is current-dev. |
| First run | `preflight_only` dispatch of the soak workflow | Proves the repaired driver and the harness build on the OCI runner before any node runs. |
| Baseline duration | `daily-24h` per candidate | The workflow's dev integration soak. The 60-hour stability soak waits for a passing baseline. |
| Repetitions | One per candidate for the baseline | A second repetition only if the first reports a non-passing outcome. |
| Memory | 64 GB per runner, the value the soak workflow already sets | The sizing invariant needs about 60,416 MB. See the memory correction below. |
| Quota | Two runner VMs for up to 26 hours each | OCI daily VM quota applies. A LimitExceeded result means retry the next day, not debug. |

Deferred policy findings return to the team. No comparative or alternate-policy run is part of the baseline.

The executing agent records the approval reference, the preflight run identifier, and each soak run identifier in the dispatch evidence.

## Adapter qualification checklist

The three accepted profiles bind synthetic transcripts. Their live adapters are unqualified. Qualification means a real node interface produces the record the profile guide specifies, with the identity and ordering guarantees the guide names.

| Profile | Adapters to qualify | What the guide requires |
| --- | --- | --- |
| Authority and finality (002) | same-DAG, electorate, fault-tolerance | Paired input identity, majority and threshold boundaries, finality decisions with correlated heads. |
| Publication and restart (003) | publication boundary, exit, restart, atomic snapshot, durable work | Observed cut points, linked restart receipts, whole tuples, an atomic flag that the node interface supports. |
| Recovery and custody (004) | source occurrence identity, pause receipt, delivery receipt | Occurrence identity preserved, not replaced by position, signature, or execution order. Pause and delivery receipts from observed states. |

Each adapter needs a qualification record that names the node revision, the interface, the fixture, and the observed result. A qualification record for one node revision does not transfer to another.

This work needs Linux, a node build, and the harness. It belongs to a Linux agent after its profile work, or to a third agent.

## Preflight dispatch, the first step of TASK-017-12

The merge-recovery soak workflow has never run on this branch. The repaired driver and the harness build step are exercised on pull requests only through the bindings gate.

The user decided on 2026-09-19 to fold this into TASK-017-12 as a manual first step. It is not a pull-request check.

The workflow triggers on schedule and manual dispatch only. Its soak job runs on a self-hosted OCI label that exists only after a launch job provisions it. That job reads thirteen secrets and spends a VM from the daily quota on every run.

A pull-request trigger would therefore launch a VM on every push and exhaust the quota. The repository already gates expensive validation behind maintainer approval for the same reason.

A dispatch with `preflight_only=true` runs the full integration preflight and stops before any node campaign. The executing agent runs it first and records the run identifier and outcome. A non-passing preflight blocks the baseline dispatch.

A GitHub-hosted job can give continuous protection later. It builds the harness and exercises the driver's fail-closed path in a container. That covers most of the same ground without touching OCI.

```
gh workflow run merge-recovery-soak.yml --ref formal/soak-casper-consensus -f target_ref=formal/soak-casper-consensus -f preflight_only=true
```

## Status

- Repin procedure: written, proven on the current dev commit, and committed to the repository.
- Resource proposal: approved by a maintainer on 2026-09-19.
- Adapter checklist: written, and now owned by the Linux agent.
- Preflight dispatch: folded into TASK-017-12 as its manual first step.

## Dispatch preconditions

Six conditions decide whether a dispatch succeeds or wastes runner time. Each one cost an investigation during preparation.

**1. The candidate pin comes from the immutable tag.** The `:dev` tag is a mutable pointer and cannot identify a candidate. The `dev-<git describe>` tag is immutable and names the commit. Rerun the repin script at dispatch, because recorded values age out as soon as `dev` moves.

**2. The matrix pin is far behind, and the newest canary is the wrong line.** On 2026-09-19 `dev` was 120 commits and nine merged pull requests past the matrix pin `a2fe60c72`. Four of those change consensus or node code. The harness revision was 171 commits behind the branch tip. The newest canary build came from `master`, so it is not a candidate.

**3. The soak workflow cannot become a pull-request check.** It triggers on schedule and manual dispatch only. Its soak job needs a self-hosted OCI label that exists only after a launch job provisions it. That launch job reads thirteen secrets and spends a machine on every run. A pull-request trigger would exhaust the daily quota.

**4. A quota failure is not a defect.** An OCI launch that returns `LimitExceeded` means the tenancy-wide daily instance quota is spent. Retry the next day. Do not debug the launcher. This appeared on this branch on 2026-09-19 and cleared by itself the same day.

**5. Draft release evidence is addressed by identifier.** The external store is a draft release, identifier `391939637`, tag `cbc-evidence-epic-017`. A draft release carries no Git tag, so the release-by-tag endpoint returns 404 for every credential. Address it by identifier until TASK-017-15 publishes it.

**6. A change to a pinned artifact returns its claim to pending.** Before editing any file in a claim's artifact inventory, check whether the edit forces a renewal. Batch related edits into one change so one renewal covers them all.

## Handoff to the Linux agent (2026-09-19)

The maintainer transferred TASK-017-12 to `pi-soak-carrier-index-linux` on 2026-09-19. Preparation is complete. This session keeps no part of the task.

The preparing session ran macOS. Every remaining step needs Linux, a node build, a Docker daemon, and OCI access. That is why the task moves.

The owner starts from the dispatch preconditions in the [preparation log](../work-logs/task-017-12-preparation.md#dispatch-preconditions). They list what must hold before a dispatch spends runner time.

### What is ready

The repin script is committed and proven. The resource budget is approved and needs no new decision for the baseline. The adapter checklist names each adapter and its required record. The preflight command is written and its rationale is recorded.

### What the owner must not assume

The dry-run digests in this log are stale. They prove the procedure, not the candidate. Rerun the script at dispatch.

The approval covers one preflight, one 24-hour baseline per candidate, and two runner machines for 26 hours each. A second repetition or the 60-hour stability soak needs a new decision.

Live adapter qualification does not follow from the resource approval. A qualification record binds one node revision and does not transfer to another.

### Prerequisites outside the owner's control

At handoff, CLAIM-CASPER-SOAK-001 was pending after the binding-inventory repair. The Linux readiness check below verifies its later renewal.

The ratifiers confirmed D-07 Reading A on 2026-09-19. PR #216 supplies the implementation. Recovery adapter qualification for CLAIM-CASPER-SOAK-004 therefore waits for that merge, because a pre-merge node has no occurrence store to observe. The other adapters and the baseline soak do not wait.

## Linux readiness check at 3aa79d0c1

The user requested TASK-017-12 completion after pulling `3aa79d0c1c91988be781c8380d8f6952b7d72868`. The checkout was clean when verification started.

The four Claim001 renewal-package checksums passed. A fresh release build of `check-casper-claims` passed, and its strict canonical audit returned exit 0.

All eight claim records are discharged. All eight soak fields remain pending. This audit checks source-bound records and does not rerun their proofs.

The machine runs Linux on arm64. The Docker daemon and buildx responded. The required command-line tools are present, including the OCI client.

Tool availability does not establish OCI authentication, quota, or dispatch permission. This check did not test those external conditions.

### Dispatch blockers

The current matrix remains `not-dispatchable`. Both candidates still have null workload configuration digests and blocked admission.

The authority profile rejects every non-synthetic request with `live_adapter_unqualified`. The accepted runtime admits only synthetic `harness-lifecycle` requests through its qualified execution path.

These restrictions do not prevent the separate integration workflow from launching nodes. An integration run does not qualify the profile adapters.

The documented dispatch does not implement the approved candidate plan:

- The launcher selects amd64, and the soak job requires an x64 runner.
- The daily path builds a new amd64 image on that runner. It does not select both pinned CI images.
- The launcher sets `RUNNER_MEM_GB_OVERRIDE` to 64 GB. The recorded resource approval specifies 48 GB.
- The workflow sets a resident set size (RSS) ceiling of 45,056 MB and a host-free floor of 8,192 MB. Those values cannot fit together within 48 GB.

The memory mismatch needed a maintainer decision before dispatch. The maintainer resolved it on 2026-09-19. See the memory correction below. The remaining architecture and image-selection items still stand.

The workflow and runtime are mandatory Claim001 artifacts. Changes to either artifact require new source-bound verification and acceptance. No accepted artifact changed during this check.

D-07 is answered. Recovery adapter qualification now waits for the PR #216 merge rather than for a decision. It does not block the separate pre-merge baseline soak.

### Retained results and next step

Local evidence is in `/tmp/task-017-12-readiness-fyVezE/`. It contains the revision, build log, renewal checksum results, canonical audit, Docker check, and readiness summary.

No live node, qualification run, preflight dispatch, or baseline campaign started. TASK-017-12 remains in progress.

The next step is to resolve the resource and dispatch differences. Candidate pinning and adapter qualification must retain their own evidence before campaign admission.

## Memory correction, approved at 64 GB (2026-09-19)

The maintainer approved 64 GB per runner on 2026-09-19. This replaces the 48 GB figure in the original resource proposal.

The first proposal cited the fleet default of 48 GB. That was an error by the preparing session. The soak workflow sets `RUNNER_MEM_GB_OVERRIDE` to 64, and the workflow comment says to keep that knob in step with the ceiling.

### The sizing invariant

The workflow states the rule. The ceiling, plus about 7 GB of host overhead, plus the floor, must fit inside the virtual machine's total memory.

| Term | Value |
| --- | --- |
| `SOAK_RSS_CEILING_MB` | 45,056 MB |
| Host overhead | about 7,168 MB |
| `SOAK_HOST_FREE_FLOOR_MB` | 8,192 MB |
| Required total | about 60,416 MB |

A 64 GB machine has 65,536 MB, which leaves about 5 GB of headroom. A 48 GB machine has 49,152 MB, so the requirement overruns it by about 11 GB. On that machine the host-free floor fires before the ceiling can attribute the growth, which produces an unattributable kill.

### Why a lower ceiling does not work either

Fitting the invariant inside 48 GB requires a ceiling near 33,792 MB. The measured workload peak is higher. Smoke run 31547587950 reached 36,008 MB of instant resident set size while fully healthy, at 1,200 deploys per 300 seconds, with zero errors and no finality lag. A ceiling below that peak stops healthy runs.

The recorded history shows the same failure twice. On 2026-08-10 the ceiling moved from 20,480 MB to 28,672 MB when the machine grew to 48 GB. Run 31390673884 proved that 32 GB could not hold the envelope plus any safe floor. On 2026-08-11 the ceiling moved to 45,056 MB with the 64 GB machine, after the wedge fix in PR #228 revealed the true footprint.

The current ceiling is about 1.25 times the observed peak. It stays silent on legitimate load and catches a real leak about 9 GB before the floor.

### Effect on the approval

The approved plan now reads 64 GB per runner for two runners, for up to 26 hours each. The candidate count, the preflight step, the 24-hour baseline duration, and the single repetition per candidate do not change.

The quota arithmetic changes with the machine size. The executing agent records the actual shape at dispatch.

This correction changes no workflow file and no runtime file. Both are mandatory CLAIM-CASPER-SOAK-001 artifacts, and neither needs a change, because the workflow already sets 64 GB.

## Candidate identity refresh at 1f749aa83

The user requested continuation after the memory and D-07 decisions arrived. Both decisions remain in force. Recovery qualification waits for the actual PR #216 merge.

The current `dev` revision was `6940a5beb4aa806d3d75f6df3be9f238512fcc2f` before and after image verification. The harness revision was `1f749aa831f54f2c5b3a7c79be27581c89e55f46`.

The committed resolver completed successfully for amd64 and arm64. No node ran. Stopped containers supplied the registry images' node bytes.

The [identity report](../casper/cbc-evidence/runs/casper-candidate-repin-20260919-01/report.json) records the exact registry, archive, config, and binary digests.

| Candidate | Push run | Image artifact | Archive bytes |
| --- | --- | --- | --- |
| dev-amd64 | 35423285859 | 10578996716 | 93,420,701 |
| dev-arm64 | 35423285859 | 10578617001 | 91,815,112 |

Both archive sizes and SHA-256 values matched the GitHub metadata. ZIP integrity checks passed. Each archive supplied ten verified blobs and eight verified layer identities.

The CI and registry images have equal config digests, uncompressed layer digests, and node hashes. Their manifest digests differ and remain separately recorded.

The CI archives use OCI manifests. The registry uses Docker v2 manifests and different compression for the base layers. These differences do not change the verified node bytes.

The earlier preparation cited run `35423287293`. That pull-request run has no candidate image artifacts. Push run `35423285859` supplies the retained images.

This verification uses GitHub run metadata and the pinned workflow source. It does not verify cryptographic build attestations or prove node correctness.

### Matrix changes

The candidate matrix now retains the verified current candidate identities and both CI artifact references. It records the registry manifest type explicitly.

The existing model and configuration inventory contained 179 hashes. Its first check failed on the harness verification plan and the interface contract.

Both stale values now match the current files. The other 177 values remain unchanged. The initial failed check remains in local evidence.

This refresh does not establish a complete campaign inventory. Workload configuration digests remain null, both candidates remain blocked, and the matrix remains `not-dispatchable`.

### Remaining execution work

The existing workflow still selects amd64 and rebuilds an image. The approved plan requires both verified platform images.

Each separate workflow dispatch launches a runner. A separate preflight followed by two baseline dispatches would exceed the approved two-machine count without a reuse path.

Dispatch design must preserve the approved 64 GB sizing, host controls, image identities, and total runner budget. No workflow or runtime changed during this refresh.

Authority and publication adapters remain unqualified. Generic node queries and restart commands cannot replace the specified paired evaluations, publication cut points, or atomic snapshots.

Local evidence is in `target/task-017-12/repin-20260919-01/`. It retains both archives, source snapshots, verifier code, node binaries, metadata, failed checks, and final verification results.

No live qualification, cloud launch, workflow dispatch, upload, or campaign occurred. TASK-017-12 remains in progress.

## Execution feasibility review at f5ed8c198

The user requested the remaining work. This review inspected the current workflow, candidate API definitions, and pinned external runner sources before proposing code changes.

GitHub still reports `dev` at `6940a5beb4aa806d3d75f6df3be9f238512fcc2f`. PR #216 remains open with no merge commit.

The external launcher already supports amd64 and arm64. The local workflow selects amd64 explicitly, so architecture support requires workflow changes rather than a new launcher architecture.

The pinned runner template registers with `--ephemeral`. After one job, it terminates its own instance. The template has no reuse option in the inspected registration path.

A completed preflight-only job therefore cannot leave its runner available for a later baseline job. Three separate dispatches need at least three machines with this launcher.

The workflow can also launch replacement machines and retry a failed segment. An approved campaign needs an explicit launch limit rather than these generic retry paths.

The `daily-24h` input selects 79,200 seconds, which is 22 hours. The workflow subtracts integration-preflight time before the workload starts.

A full 24-hour baseline needs a separate duration calculation. Its setup, preflight, evidence capture, and cleanup must still fit within the approved 26-hour runner limit.

The inspected gRPC definition and three HTTP route tables match the selected candidate source exactly. They expose normal block, deploy, validator, and proposal operations.

Those interfaces do not expose the specified publication-boundary control or complete atomic publication tuple. This source inspection does not qualify a live adapter.

The profile contracts require blocked results for unavailable interfaces. Adding node interfaces remains outside this task. Recovery qualification retains its separate PR #216 dependency.

### Proposed implementation, not yet approved

The proposed change adds a manual campaign mode to `.github/workflows/merge-recovery-soak.yml`. Existing scheduled runs retain their current behavior.

Rust and Bash helpers under `scripts/casper-soak/` and `scripts/ci/` would validate workload pins, candidate identities, qualification records, durations, and launch limits.

Tests would cover both architectures, wrong image identities, missing capabilities, exhausted budgets, and incomplete preflight evidence. Existing host-protection and evidence-preservation controls remain required.

Changes to accepted artifacts require renewed source-bound verification and acceptance before dispatch. The binding inventory script remains unchanged.

One resource decision remains necessary. An additional 64 GB preflight runner, capped at four hours, would preserve the existing one-job runner lifecycle.

The two baseline runners would retain their approved 64 GB size and 26-hour limit. Alternatively, retaining two total machines requires a separately reviewed reusable-runner design.

Neither option is approved by this proposal. No new code, node process, cloud runner, or workflow dispatch resulted from this review.

External source snapshots are in `target/task-017-12/execution-review-f5ed8c198-01/`. The failed lookup for `cloud-init.yaml` remains beside the correctly resolved `cloud-init-runner.yml.tmpl`.

## Approval amendment and initial campaign helpers

The user approved separate preflight and baseline dispatches on 2026-09-19. This approval accepts the proposed additional OCI preflight runner.

The preflight runner retains its four-hour limit. The two baseline runners retain their 26-hour limits. All three runners have 64 GB of memory.

The user also approved a separate 60-hour TASK-017-12 campaign after a passing baseline. This campaign does not replace or automatically launch the scheduled weekend soak.

The 60-hour phase still needs explicit candidate and runner limits before launch. The prototype models two additional 64 GB runners, with a 64-hour limit each.

That proposal means five total machines across the three phases, with at most two campaign runners active. The prototype requires both baseline run IDs before the stability phase.

The 64-hour limit and two additional machines remain a proposal. Synthetic fixture approvals do not authorize cloud resources.

### Implemented scope

Two new Bash files provide the first implementation increment:

- `scripts/casper-soak/campaign.sh` validates a campaign request and calculates a full workload window.
- `scripts/casper-soak/test-campaign.sh` tests the helper with synthetic fixtures.

The plan command checks candidate identities, artifact digests, control sources, qualification declarations, memory, durations, and declared launch limits. Its output explicitly requires external checks.

The window command preserves 86,400 baseline seconds or 216,000 stability seconds. It rejects insufficient runner time instead of shortening the workload.

The calculation reserves 600 seconds for cleanup. It does not enforce an OCI termination deadline by itself.

The helper checks run-ID syntax but does not verify GitHub run outcomes. It neither reserves a campaign launch nor starts a runner.

The helper does not perform live adapter qualification. It does not establish that the campaign inventory includes every required model, configuration, or capability.

The workflow, runtime, profiles, and binding inventory script remain unchanged. Daily and weekend scheduling remain unchanged.

Both new Bash files inherit mandatory, high-weight CbC attributes. Their claim registration, source-bound verification, and acceptance remain pending.

The existing eight-claim audit passed before implementation. That audit covers the accepted artifact set, not these new helpers.

The matrix check rechecked 179 stored source pins. The `formal/tlaplus/casper_soak/verification-plan.jsonc` pin differs after the earlier pull.

The matrix remains `not-dispatchable`. Its existing pins remain unchanged until the complete inventory review. This check does not establish inventory completeness.

### Local verification

The final native and isolated lanes each passed 30 checks. Both lanes used synthetic fixtures, not node observations.

Checks include architecture selection, source and approval drift, path escape, missing capabilities, wrong node identities, reruns, and exact duration limits.

An added negative control reproduced an ignored source-inventory parser failure in the earlier helper. The corrected helper rejects that failure.

The initial missing-helper failure, path-validation failure, and parser negative control remain retained. The first isolated attempt lacked `jq` and returned exit 127.

A fixture image built from the existing disk-test Dockerfile supplied `jq`. The final isolated lane disabled networking and capabilities and used an unprivileged user.

The isolated lane used two CPUs, 512 MiB of memory, and a 128-process limit. Its container exited normally and was removed after evidence capture.

Evidence remains under `target/task-017-12/campaign-implementation-f5ed8c198-01/`. It includes source snapshots, image identity, commands, fixture outputs, exit codes, and failed attempts.

Workflow integration, prior-run verification, launch enforcement, complete inventory review, live qualification, and renewed acceptance remain incomplete. No node, OCI runner, or workflow dispatch started.

TASK-017-12 remains in progress. No commit, push, or upload occurred during this implementation increment.

## Linux continuation at 7509c831c

The user requested the remaining Linux work. The source review confirmed a required interface blocker before any cloud launch.

The [admission report](../casper/cbc-evidence/runs/casper-linux-admission-7509c831c-01/report.json) records the new checks. Its status is `blocked-before-node-launch`, not campaign completion.

### Current identities and source inventory

GitHub still reports `dev` at `6940a5beb4aa806d3d75f6df3be9f238512fcc2f`. The revision remained unchanged during this review. PR #216 remains open and unmerged.

The immutable image index matches the earlier retained snapshot byte for byte. The earlier candidate identity report remains the authority for image and node-binary verification.

All 269 files in the pinned external suite match their Git blob identities, byte lengths, and retained SHA-256 inventory. No suite source changed.

The [legacy workload record](../casper/cbc-evidence/runs/casper-linux-admission-7509c831c-01/legacy-load-workload.json) pins the load test, preflight selection, dependency files, providers, and stress options.

That record has scope `legacy-load-only`. Its three stress options are not production defaults. It cannot substitute for the required Casper baseline.

Five candidate API source snapshots match their Git blobs and the local source bytes. This check does not establish live interface qualification.

The matrix still has one stale pin among its 179 stored source entries. That pin names `formal/tlaplus/casper_soak/verification-plan.jsonc`.

The matrix remains unchanged and `not-dispatchable`. Its complete campaign inventory and ratified executable workload pins remain unfinished.

### Executable admission results

Fresh release builds passed eight authority tests and six publication tests. The retained fixture evidence contains 148 profile invocations.

Four additional admission probes used the exact amd64 and arm64 candidate identities. The probes supplied controlled fixture inputs and no observations from running nodes.

Both profile binaries rejected both candidates with exit 3 and verdict `blocked`. Each report records zero node launches, no measurements, and no captured node observations.

The authority profile requires qualified same-DAG evaluation, electorate context, finality decisions, original fault-tolerance projection, and applicable counters or fault controls.

The publication profile requires qualified cut-point, exit, linked-restart, atomic-snapshot, and durable-work observations. Generic queries and restarts do not satisfy those requirements.

Both profiles also retain an explicit unqualified-live-adapter guard. The probes demonstrate refusal, not successful qualification or node execution on either candidate architecture.

Recovery qualification retains its separate actual-merge prerequisite. The review does not fabricate occurrence-store observations before PR #216 merges.

### Parser repair and verification

The draft campaign helper accepted multiple JSON documents when it checked a workload window. A regression test reproduced exit 0 where exit 2 was required.

The helper now requires exactly one JSON object. New cases cover empty input, multiple documents, an invalid first document, and an array.

The repaired helper passed 34 checks in each native and isolated lane. The original failure, tested sources, commands, and outputs remain retained.

A fresh strict audit passed for all eight existing claims. That audit does not include the two campaign helpers.

[CLAIM-CASPER-CAMPAIGN-001](../claims/casper-soak-campaign.md) now defines the planning controls and exclusions. Both helper ledgers remain pending, without a waiver or verification date.

The separate claim does not alter previous discharges. It does not convert fixture results into formal discharge or live qualification.

The accepted workflow, runtime, profiles, binding inventory, and candidate matrix remain unchanged. Existing daily and weekend schedules remain unchanged.

### Handoff boundary

Bulk evidence remains under `target/task-017-12/linux-continuation-7509c831c-01/`. The compact report, validation record, legacy workload record, and suite-source manifest are in Git scope.

Workflow integration and execution remain unfinished. No OCI runner, candidate node, preflight dispatch, or soak campaign started.

A qualified live-adapter contract and eligible Casper workload are prerequisites for the campaign path. Hardware approval alone cannot satisfy these requirements.

A separate legacy-load diagnostic campaign would require an explicit scope decision. Its results could not close the missing Casper-profile evidence.

The other agent can use this evidence for review and the remaining handoff work. TASK-017-12 remains in progress. I did not start EPIC-018.

### Concurrent checkout change

The checkout advanced to `5cc4e665b52f0cc5279ec577ac5075b5b9897caf` during this review. The revision guard rejected that change with exit 1.

The two incoming commits changed six documentation paths. They did not change the scoped executable sources, workflow, binding inventory, or candidate matrix.

The [provenance record](../casper/cbc-evidence/runs/casper-linux-admission-7509c831c-01/provenance.json) preserves this event and the original request identities. No earlier result becomes execution at the later revision.

The source snapshots establish byte identity. They do not establish the checkout revision at every earlier command.

The new branch policy permits EPIC-018 stacking after acceptance of the EPIC-017 handoff. The actual merge remains necessary for recovery qualification and post-merge discharge.

This work does not complete that handoff. The incoming formal-gate authorization also remains separate from campaign approval and execution.

## Manual campaign dispatch implementation

The implementation starts from `797d29ca1f2703d1e4dd4c9c2bc6ae0e0edbceb1`. The user authorized the file-level implementation plan, without dispatch, commit, or push authorization.

The workflow will separate campaign inputs from scheduled and legacy manual inputs. Campaign baseline selection must mean 86,400 workload seconds, without preflight subtraction.

The helpers will validate prior-run identities and outcomes before reporting the remaining admission blockers. Failed requests must retain their inputs and non-passing reports.

The campaign route must not reach existing cloud launch, replacement, retry, or publication jobs. The current launcher does not establish the required lifetime enforcement.

This route remains admission-only until eligible Casper workloads, live adapters, authenticated approvals, persistent launch reservations, and independent instance termination are implemented and verified.

Authority and publication admission actively reject both candidates. Recovery qualification waits for the actual PR #216 merge under D-07 Reading A.

The baseline budget permits three machines in total. Campaign mode must not permit replacement or repeat launches.

The 60-hour phase also requires passing baselines and an explicit decision recording candidate count and runner lifetime limits. Fixture approval records cannot supply that decision.

Changes to the workflow invalidate its current source binding. Affected claims remain pending until fresh verification and explicit acceptance.

Evidence remains under `target/task-017-12/manual-dispatch-797d29ca1-01/`. Earlier source and ledger bytes remain in that directory and Git history.

### Continuation after the commit checkpoint

Commit `5e26ba4c5` records the admission-only route. Its 103 native checks, Actionlint 1.7.12 check, and pre-commit checks passed.

The interrupted work did not renew source bindings or finish isolated verification. The earlier workflow discharge applies only to its stored source digest.

The continuation will require both campaign helpers in the pinned source inventory. It will test both baseline candidates and all three stability prerequisite records.

The continuation will preserve rejected inputs and old ledger bytes. The changed workflow and both helper records will remain pending, without a waiver or acceptance date.

Bulk evidence uses `target/task-017-12/manual-dispatch-continuation-5e26ba4c5-01/`. No additional commit, push, workflow dispatch, or cloud execution is authorized.

### Continuation results

The missing-test-pin regression returned exit 0 instead of required exit 2. The corrected planner requires all six control paths, including both campaign helpers.

The [continuation report](../casper/cbc-evidence/runs/casper-campaign-dispatch-5e26ba4c5-01/report.json) records 115 native checks and 115 isolated checks. Both final lanes passed.

The first isolated attempt reached its 120-second timeout and returned process exit 124. Its wrapper returned exit 1, and all available evidence remains retained.

The second attempt retained the same fixture assertions, network isolation, memory limit, CPU limit, and process limit. Its timeout increased to 550 seconds.

Both containers stopped and were removed after evidence capture. Neither container ran a node or contacted a live prior-run API.

The checks cover both baseline candidates, all three stability prerequisite records, shortened workload records, failed baselines, unapproved resources, and archive symbolic links.

Actionlint 1.7.12 and Bash syntax checks passed. Actionlint ran without ShellCheck or Pyflakes, so those checks are not claimed.

The canonical artifact gate returns exit 4 with three pending records. The default gate also returns exit 4, with one pending workflow and two missing helper lookups.

The canonical gate does not replace the default gate. Compatibility lookup repair for the two helpers remains open.

The fresh strict claim auditor first rejected stale source bindings. After the ledger update, it rejects the pending workflow discharge instead.

The workflow ledger now records the current source digest. Its unchanged compatibility symbolic link resolves to the same pending record.

Both campaign helper ledgers record current source and specification digests.

The initial metadata check compared the symbolic link text with its resolved ledger content and returned exit 1. The corrected check verifies both identities separately.

The failed check and its original verifier remain retained. The corrected metadata check passes without changing the compatibility link.

Every changed ledger retains its prior Git revision, path, and digest. Earlier accepted reports, profile ledgers, and completion records remain unchanged.

The campaign route remains admission-only. Eligible Casper workloads, qualified live adapters, approval authentication, launch reservations, lifetime enforcement, hosted verification, and explicit acceptance remain required.

The legacy action, driver, preflight script, and schedules remain unchanged. The manual baseline selection means 86,400 workload seconds, but no workload execution occurred.

TASK-017-12 remains in progress. This update does not deliver the missing baseline, scan benchmark, or concurrency-gate evidence to TASK-017-13.

### Completion review at 859cbc36c

The user requested completion after commit `859cbc36c`. That commit marks the harness claim and binding as pending.

A fresh GitHub query still selects `6940a5beb4aa806d3d75f6df3be9f238512fcc2f` for `dev`. PR #216 remains open and unmerged.

Five candidate API source files were downloaded at that exact revision. Their digests match the earlier inspected files.

This comparison does not establish live qualification or prove that every possible interface is absent. No candidate node ran during this review.

The authority and publication profiles still reject requests for node observations. The shared runtime admits only the synthetic lifecycle profile.

The ratified plan excludes node implementation changes. Missing node interfaces require a separate scope decision or an existing qualified interface before campaign completion.

The two campaign helper compatibility links now resolve to their canonical ledger records. Neither link copies a record or changes its pending status.

The default artifact gate previously reported two missing lookups and one pending record. Both lookup modes now report three pending records with exit 4.

A fresh build of the unchanged claim auditor reports Claim001 as pending and the seven other claims as discharged. The strict audit returns exit 4.

The auditor checks ledger consistency. This execution does not run a prover, renew hosted verification, or accept a claim.

The pending workflow ledger still names the specification digest from before the two-line correction. That digest needs reconciliation during renewal, without rewriting historical reports.

The matrix check found 179 stored model and configuration entries. One verification-plan digest remains stale, and complete campaign inventory coverage remains unproved.

Both eligible workload digests remain null. Campaign execution, authenticated approvals, persistent launch reservations, and independent lifetime enforcement remain unfinished.

Evidence remains under `target/task-017-12/completion-review-859cbc36c-dD9zLH/`. The directory retains remote identities, source copies, gate results, auditor identity, and the readiness report.

No workflow dispatch, cloud launch, node launch, claim acceptance, commit, or push occurred. TASK-017-12 remains in progress with execution blocked.

### Separate node-interface authorization

The user authorized a separate node-interface prerequisite after this review. The [file-level plan](../plans/casper-node-interface-prerequisite.md) records the proposed changes, safety limits, tests, and approval boundaries.

The inspected node source trees match the pinned `dev` revision. The review found shared mutable stores behind DAG representations and separate publication writes.

The proposed interface therefore requires detached evaluation inputs and receipts from actual node boundaries. It must not invent atomic snapshots, durable effects, or occurrence identities.

Batch A proposes disabled-by-default local access, identity checks, bounds, and truthful capability reporting. Batches B and C require further source and consistency reviews.

This authorization does not amend the harness epics or accept a claim. No node source changed, and no node or cloud runner launched.

The exact Batch A file list awaits confirmation before implementation. The existing campaign blockers and actual-merge prerequisite remain in effect.

### Claim001 specification digest reconciliation

The user requested reconciliation without changes to historical evidence or the pending campaign helper records. The review started from `7b8865aaa49fdec69b5b3dea8a239c06b4d399b6`.

The current workflow ledger now names the specification digest from the two-line status correction at `859cbc36c`:

- Previous digest: `c1f6b7473f790a60ab964ce7fd7277841cd6cee5718e9853ad96cf4c671d5932`.
- Current digest: `b6d4f83f958af79037c9b938edd52f0a91ef6f4e8858d6a85a2e4aa8b6346faf`.

The reconciliation metadata identifies the previous ledger and both specification versions by Git revision, path, and digest. All four Git references passed digest checks, including the unchanged earlier acceptance reference.

The fixture report, its digest, and its execution identity remain unchanged. Digest checks also confirmed that all 1,206 tracked historical run files remain unchanged.

Both campaign helper records remain byte-identical and pending. Their compatibility links remain unchanged, and the workflow compatibility link still resolves to the canonical ledger.

The direct specification check rejected the stale digest before the edit and passed afterward. The other workflow fields remain unchanged, including its pending status and null verification time.

The rebuilt claim auditor returned exit 4 with Claim001 pending and seven other claims discharged. Default and canonical artifact gates each returned exit 4 with three pending records.

These checks verify metadata consistency, not formal discharge. No prover, hosted verification, node, or cloud runner executed.

Evidence remains under `target/task-017-12/claim001-reconciliation-7b8865aaa-C8GNMj/`. The earlier completion review remains unchanged as a historical record.

The specification digest gap is resolved. Claim001 still requires source-bound renewal and acceptance, and TASK-017-12 remains blocked before execution.

### Independent node delivery and harness controls

The later node authorization approved Batch A, its claim, and both mandatory tags. Batch A and its shutdown correction are published at `799e2136adc6e0100b289945d9a5a6851e81c91f`.

The node claim remains pending. Batch B1, Batch B2, and Batch C require separate approval. The node prerequisite is not complete or merge-ready.

PR #447 targets `dev` independently. PR #436 temporarily targets the node branch and can return to `dev` after the prerequisite merges.

This dependency order does not include node implementation in the harness scope. The published harness head does not contain the node commits.

The updated plans distinguish dependent targets from source integration. No branch, pull request target, merge, image, or candidate pin changed during this update.

Independent harness work continues with the [local reservation guard](task-017-12-reservations.md). Its checks do not establish authenticated approval, global accounting, instance lifetime enforcement, or campaign execution.

The admission-only route remains blocked. Live qualification, separate preflight, both full baselines, and the later stability resource decision remain required.

### Campaign execution-control review at db4a52fb6

The user authorized the remaining execution-control implementation. The [execution-control plan](../plans/casper-campaign-execution-controls.md) records the reviewed boundaries, proposed mechanisms, source scope, and acceptance tests.

A read-only GitHub query found no required reviewers on `oci-credentials`. The environment has a branch rule and permits administrator bypass.

That configuration cannot independently establish campaign approval. The approval authority and its binding to the exact campaign request require confirmation.

Fresh external downloads at `b3d14b27e3c6276b1eb4ab9ccef04e02b0c4e283` match the retained launcher and bootstrap. The launcher can submit three attempts and retries selected timeout failures.

The inspected launch request has no creation-time campaign tags. The campaign must not reuse this retry path or depend on a later tag update.

The existing reaper has a 30-minute GitHub schedule. It does not establish the approved lifetime limits or independence from GitHub scheduling availability.

The installed OCI Resource Scheduler help lists start, stop, and backup actions. Stop is not evidence of instance termination or completed cleanup.

The proposed design uses a dedicated approval environment, one pre-provisioned OCI campaign object, and an independently deployed lifetime supervisor. These are proposed mechanisms, not configured services.

The reviewer identities, authoritative object, and supervisor deployment remain unspecified. No additional supervisor machine is included in the approved baseline budget.

Evidence remains under `target/task-017-12/execution-control-review-db4a52fb6-QwPy0a/`. It contains source copies, digests, the environment response, and local CLI help.

No executable source, claim status, GitHub protection, OCI policy, or external repository changed. No node, cloud instance, workflow dispatch, commit, or push occurred.
