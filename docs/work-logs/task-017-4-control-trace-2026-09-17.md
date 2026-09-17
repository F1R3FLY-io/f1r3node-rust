---
task: TASK-017-4
cycle: H10-trace
claimed_by: pi-casper-harness
handoff_status: in_progress
construction: not-applicable
---

# Shared Formal Gate Trace Validation

The user requested continued implementation after prerequisite application. This cycle strengthens negative-control classification without dispatching a workload or changing node behavior.

The starting revision is `6814682e4c7de98f883b8791d3227e98bc3f2c15`. The working tree and index were clean.

## Contract

A registered negative control requires TLC exit 12, its exact invariant, and a counterexample trace. A trace header without a first state cannot pass.

The executable fixture invokes the production shared gate with a substitute verifier process. Actual bounded TLC runs check compatibility with real transcripts.

This cycle does not complete H10, TASK-017-4, or CLAIM-CASPER-SOAK-001. Positive-search validation and shared registration of the Casper controls remain separate work.

## Steps

- Add one missing-trace rejection case to the existing fixture.
- Confirm that the unchanged gate accepts the truncated result and fails the fixture.
- Require a first state after the trace header.
- Run the fixture and the bounded shared TLC tier in isolated containers.
- Retain source digests, RED/GREEN outcomes, and bounded-model transcripts.

## Limits

Workload configuration pins, profile adapters, full driver bindings, and claim discharge remain pending. No task dependency or campaign admission gate is waived.

The native Linux fixture diagnostics are unchanged platform and scanner false positives. The previous cycle retained the successful Linux-targeted Pyright result.
