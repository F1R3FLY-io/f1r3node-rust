---
kind: handoff
from: claude-session-9f19b46c
to: pi-soak-carrier-index-linux
produced_at: 2026-09-19T19:20:00Z
retention: durable
focus: Transfer TASK-017-12 execution to a Linux owner with the repin tool, the approved budget, and the dispatch preconditions.
related_task: TASK-017-12
related_epic: EPIC-017
redaction:
  categories_touched: []
  total_substitutions: 0
suggested_skills:
  - /cbc-verify
consumed_at: ""
consumed_by: ""
---

# TASK-017-12 execution handoff

## Authorization

The maintainer transferred TASK-017-12 to this agent on 2026-09-19. The tracker records the transfer, the previous owner, and the new execution scope.

The preparing session ran macOS. Every remaining step needs Linux, a node build, a Docker daemon, and OCI access.

## Current state

Preparation is complete. The tracker moved from preparation-only to execution scope. This session keeps no part of the task.

All eight claims discharge under the strict audit, and the full bundle returns exit 0. The maintainer renewed the CLAIM-CASPER-SOAK-001 binding on 2026-09-19 after the inventory repair.

## Start here

Read the [dispatch preconditions](../work-logs/task-017-12-preparation.md#dispatch-preconditions) first. They list six conditions that decide whether a dispatch succeeds or wastes runner time.

The same [preparation log](../work-logs/task-017-12-preparation.md) carries the repin procedure, the approved budget, and the adapter checklist. Its handoff section states what is ready and what must not be assumed.

## What is ready

The repin tool is committed at `scripts/ci/resolve-dev-candidate.sh`. It resolves both platform candidates from the immutable Docker Hub tag and returns the manifest, config, and node binary digests. It was proven against dev `6940a5beb`.

The resource budget is approved. It covers two candidates, one preflight dispatch, one 24-hour baseline per candidate, and two runner machines at 64 GB each for up to 26 hours. The 64 GB figure corrects an earlier 48 GB entry and matches what the soak workflow already sets.

The adapter-qualification checklist names each adapter for claims 002 to 004 and the record each one must produce.

The preflight command and its rationale are recorded in the preparation log.

## Order of work

1. Repin both candidates against the dev commit current at that time. The recorded dry-run digests are stale by design.
2. Qualify the live adapters for claims 002 to 004 against a real node. Record the node revision, the interface, the fixture, and the observed result for each adapter.
3. Run the preflight-only dispatch. Record its run identifier and outcome. A non-passing preflight blocks the baseline.
4. Run the approved baseline soak for each candidate. Record exact revisions, seeds, run identifiers, configuration, metrics, and artifact digests.

## Limits

A second repetition requires a new maintainer decision. The 60-hour stability soak requires a passing baseline and a new decision.

The resource approval does not authorize live adapter use beyond qualification, policy activation, or post-merge execution.

A qualification record binds one node revision. It does not transfer to another revision.

An OCI launch that returns `LimitExceeded` means the daily quota is spent. Retry the next day rather than debugging the launcher.

Do not publish the evidence draft release. TASK-017-15 owns that step, and it follows the reduction commit.

## Prerequisites outside this task

CLAIM-CASPER-SOAK-001 was renewed on 2026-09-19 after the binding-inventory repair. No claim blocks this task.

Decision D-07 was answered on 2026-09-19 as Reading A, with PR #216 supplying the implementation. Recovery adapter qualification for CLAIM-CASPER-SOAK-004 waits for that merge, not for a decision.

TASK-017-13 is owned by pi-casper-handoff-mac. It follows this task and gates the diff reduction.

## Coordination

The binding inventory script is a mandatory artifact of CLAIM-CASPER-SOAK-001. Do not edit it as part of this task. An edit forces another claim renewal.

The repin tool is not a mandatory artifact and the inventory does not copy it, so editing it is safe.

## Open questions

- Does the candidate matrix keep its CI artifact identity field, or does it rely on the registry digests the repin tool returns?
- Does the launcher gain arm64 support, or does the approved two-candidate plan narrow to one architecture?

## Redaction notes

No redactions were necessary. All paths are repository-relative and no secrets or personal data appear in this note.
