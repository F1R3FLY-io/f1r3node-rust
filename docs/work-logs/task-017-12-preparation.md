# TASK-017-12 Preparation

---
handoff_status: ready
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
| Memory | Fleet default 48 GB, harness RSS ceiling from the driver's host-reserve rule | The driver refuses a ceiling under 5,000 MB. |
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

CLAIM-CASPER-SOAK-001 is pending after the 2026-09-19 binding-inventory repair. The strict bundle refuses discharge until the maintainer re-accepts that binding.

Decision D-07 is unanswered. It gates recovery adapter qualification for CLAIM-CASPER-SOAK-004. The other adapters and the baseline soak do not depend on it.
