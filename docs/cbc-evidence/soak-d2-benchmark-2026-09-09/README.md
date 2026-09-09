# B15 Retained Breach at Opening Benchmark Admission

```yaml
claim_id: CLAIM-SOAK-001
scope: retained breach before the opening benchmark without a state file
status: pending
verified_at: null
waiver: null
evidence: manifest.jsonc
```

## Matched cycle

B15 starts from `37ec7f71dc077b03d75822bda88547a2fdd018fb`. The fixture retains a guardian breach but omits the state file.

The startup probe returns 16384 MiB. The original driver requests benchmark startup before its recovery block reads the breach.

Production RED reports:

```text
FAIL: A retained guardian breach allowed the opening benchmark to start.
```

The driver later records failure. Its summary contains zero iterations, one failure, one benchmark segment, and one benchmark failure.

The matching formal control then violates `RetainedBreachPreventsBenchmark` with exit 12. Both RED results precede the correction.

The correction checks the retained marker in the opening benchmark condition. Production GREEN records zero iterations, zero benchmark segments, and one failure.

Formal GREEN completes with six distinct states. The [correspondence](../../../formal/tlaplus/soak_disk/BenchmarkBreachAdmission.md) describes the model and its limits.

## Regression results

The bounded gate passes 14 positive configurations and 14 exact controls. The classifier covers 98 cases. Routing passes six scenarios.

All ten emergency scenarios pass. D1, B5, B6, the isolated driver suite, workflow checks, release checks, metrics checks, and summary checks pass.

The verifier runs execute serially. The fixture uses no host mounts, network, or Docker socket. No node workload runs.

## Evidence and limits

The [manifest](manifest.jsonc) binds the source snapshots, original logs, container settings, and verifier identity. Raw evidence remains outside Git and Cargo output.

Published log copies replace the local path prefix with the manifest's `raw_root` placeholder. The manifest records replacement counts and published digests. Original raw logs remain unchanged.

The first record incorrectly hashed its active build log. The raw archive retains that record and audit failure. This metadata error is not behavioral RED.

The benchmark fixture replaces external Docker commands, not production admission or benchmark orchestration. Its startup request records admission without starting node writers.

The retained marker remains unchanged during the decision. This cycle does not cover later marker changes or unreadable metadata.

Benchmark supervision, other admission paths, stalled guardians, cleanup bounds, confirmed termination, durability, and reserve evidence remain open.

D2, hosted confirmation, maintainer review, claim discharge, and acceptance remain pending. No commit, push, or soak occurs in this cycle.
