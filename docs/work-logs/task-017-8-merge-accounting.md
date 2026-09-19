# TASK-017-8 Merge and Accounting Profile

## Scope

This task implements CLAIM-CASPER-SOAK-005 against controlled transcripts. It does not implement node accounting or activate additive merge semantics.

The starting revision is `09b0a60063815b750979864225a0a56387d6ed80`. The task started on 2026-09-19 with a clean worktree.

## Plan

- [ ] Implement pinned request generation, qualification, collection, and verdict classification.
- [ ] Preserve execution multiplicity, admission rejection, failed-body settlement, causal inputs, and compatibility labels.
- [ ] Add controlled fixtures and bounded model controls.
- [ ] Run native and isolated Linux tests, model checks, and shared regressions.
- [ ] Retain source-bound evidence and request binding acceptance.

## Boundaries

The existing accepted claims and their source artifacts remain unchanged. This profile uses a separate Rust binary and Bash runner.

Live, conditional-additive, and post-merge requests remain blocked. D-08 activation conditions do not grant this harness authority to activate a protocol.

Fixture expectations are pinned data. The profile does not recompute node execution, least rejection closure, or accounting conservation.

Claim acceptance and the proposed workflow tag require separate human decisions. Final workload qualification and node campaigns remain under TASK-017-12.
