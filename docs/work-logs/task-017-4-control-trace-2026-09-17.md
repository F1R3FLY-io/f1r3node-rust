---
task: TASK-017-4
cycle: H10-trace
claimed_by: pi-casper-harness
handoff_status: partial_cycle_verified
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

## Results

- RED exited 1 because the old gate accepted the missing-trace case. The failure was the intended regression, not a setup error.
- GREEN exited 0 with 61 acceptance controls and 21 rejection cases across three registered areas. Tier routing and control registration checks also passed.
- The shared bounded tier passed 13 positives and 61 negatives in 67.643 seconds, including container setup.
- All 61 actual negative transcripts contained the exact expected invariant and a first counterexample state. All 13 positives contained a completed-search marker.
- The Casper model passed its clean configuration and ten negative controls. The clean search contained 43,424 distinct states.
- All twelve Casper runner unit tests passed. Both changed shell files passed syntax checks and active language-server diagnostics.
- The strict epic CbC gate exited 4. Full claims remain pending.

The [evidence report](../casper/cbc-evidence/runs/casper-control-trace-20260917-01/report.json) records 118 retained artifacts and 181 source digests. The package includes the implementation patch, synthetic fixtures, bounded results, and original transcripts.

The containers used an immutable local image, unprivileged execution, and no network or host mounts. Each container stopped without an out-of-memory event before removal.

The actual gate checked process exits. The report derives per-configuration exits from successful gate branches, not independent exit-code files.

## Limits

Workload configuration pins, profile adapters, full driver bindings, and claim discharge remain pending. No task dependency or campaign admission gate is waived.

The candidate matrix retains its previous source revision. This cycle does not repin or activate the matrix.

The next H10 step must require completed positive searches and finish exact-result classification. Shared Casper registration and real-driver bindings remain pending.

The native Linux fixture diagnostics are unchanged platform and scanner false positives. A fresh Linux-targeted Pyright run passed with zero diagnostics.

The entrypoint catches parser failures and returns setup-error exit 2. The Boolean identity check deliberately excludes other false-like values.

False-positive dispositions did not clear all host diagnostic cache entries. No native fixture changed to silence those entries.

The deterministic STE Check does not replace the required human STE Review.
