# D2 Guardian Admission Evidence

## Status

B14 starts from `e6fdd343b84726c29c9ac5fa992cac8b04e0d294`. It completes one local production and formal RED/GREEN cycle.

D2, all claims, hosted confirmation, maintainer review, and acceptance remain pending. The other agent owns the planned refactoring work.

## Failure and correction

The fixture kills the guardian during the boundary disk probe. The probe returns a valid 16,384 MiB sample.

The historical driver admits an iteration after guardian death. The test reports that failure before the formal control violates `AdmissionRequiresGuardian` with exit 12.

The correction checks guardian liveness after the probe. The driver then refuses admission and publishes one protection failure without creating an iteration.

The production GREEN test passes. The positive model completes with four distinct states. No earlier evidence package changes.

## Verification

The bounded gate passes 13 positive configurations and 13 exact negative controls. The classifier covers 91 cases. Routing covers six scenarios.

The emergency suite passes nine scenarios. D1, B5, B6, the driver suite, and supporting workflow, release, repin, metrics, and summary checks pass.

Verifier runs execute serially because the gate uses shared temporary log paths. The manifest binds the source snapshot, raw evidence, and complete selected logs.

## Limits

The isolated containers have no host mounts, network, or Docker socket. They use UID 65534, dropped capabilities, resource limits, and `no-new-privileges`.

The fixture replaces external disk and workload commands, not production admission or reporting. No nodes run, and no developer-filesystem cleanup occurs.

The [model correspondence](../../../formal/tlaplus/soak_disk/GuardianAdmission.md) states the atomic-decision and fairness assumptions.

The cycle does not cover benchmark admission, live but stalled guardians, later crashes, or process identifier reuse. It does not establish termination, durability, or the emergency reserve.

## Raw evidence

The raw directory is `/home/bf_spark/soak-evidence/f1r3node-rust/d2-boundary-rgLfbV0o`. The backup is local, not off-host.

TLC state directories are not part of the selected raw digest map. The original RED test source and the corrected source remain separate.
