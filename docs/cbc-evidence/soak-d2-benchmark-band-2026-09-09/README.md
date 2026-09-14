# B16: Opening Benchmark Disk Admission

## Status

B16 passes its local production and formal RED/GREEN cycle. D2, claim ratification, hosted confirmation, maintainer review, and acceptance remain pending.

The baseline is `7f0f46923d9d972a95625fb3b8cdfc2ca2a8fc0c`. This package does not discharge `CLAIM-SOAK-001` or authorize a soak.

## Matched behavior

The fixture reports 7000 MiB free with a 4096 MiB disk floor and a 4096 MiB hygiene band. No retained guardian marker exists.

Production RED requests the opening benchmark before the later iteration guard refuses work. The Docker fixture records startup but starts no node writers.

The exact formal control violates `BenchmarkRequiresBand` with exit 12. Its counterexample uses the same 7000 MiB sample and 8192 MiB threshold.

Both RED results precede the production correction. The correction checks the disk sample before benchmark admission and records a protection failure on refusal.

Each exit pair lists the fixture exit before the driver exit.

| Result | Test / driver exit | Iterations | Failures | Benchmarks | Benchmark failures |
| --- | --- | --- | --- | --- | --- |
| Production RED | 1 / 1 | 0 | 1 | 1 | 1 |
| Production GREEN | 0 / 1 | 0 | 1 | 0 | 0 |

Formal GREEN completes with 12 distinct states. The configuration also covers samples equal to and above the threshold.

## Retained verification

The [manifest](manifest.jsonc) binds source snapshots, original raw files, complete published logs, fixture identity, and verifier identity.

- Fifteen positive configurations and fifteen exact negative controls pass.
- The classifier covers 105 cases, and routing covers six scenarios.
- Eleven emergency scenarios and the supporting driver, workflow, release, metrics, and summary regressions pass.

Actual verification and verifier-process fixture tests run serially. The raw archive retains all 30 actual configuration logs before classifier tests use the shared temporary paths.

The production fixture uses UID 65534, a private PID namespace, no network, no mounts, and no Docker socket. It drops capabilities and applies resource limits.

The first read-only inventory inspection used an absent key and raised `KeyError`. No source changed. This helper error is not behavioral RED.

## Publication and source identity

Published logs replace the local raw-root prefix with `[EVIDENCE_ROOT]/d2-benchmark-band-xzY4nTYN`. No other log text changes.

The manifest records substitution counts and published digests. Original logs retain their bytes and separate raw digests outside Cargo `target/` and Git.

The archive is local, not an off-host backup. Source digests identify the tested code but do not establish hosted execution or acceptance.

## Remaining obligations

The [correspondence](../../../formal/tlaplus/soak_disk/BenchmarkDiskAdmission.md) defines the narrow model boundary.

The production fixture covers only low-space opening admission. Equality, sufficient space, unavailable samples, disabled protection, and interleaved admission need further production verification.

Active benchmark supervision, guardian progress, cleanup ownership, confirmed termination, durable publication, the composed deadline, and D3 reserve evidence remain open.

The workload and 45-second finalization wait remain unchanged. No commit, push, hosted dispatch, or soak occurs in this cycle.
