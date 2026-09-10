# D2 Guardian Progress Evidence

## Scope

This package retains B20, B21, and B22 as separate matched repair cycles. B20 starts from `8ab599e5cbc24d05ba89a6064abdc00acfc98100`.

B21 uses the source-bound B20 correction. B22 uses the source-bound B21 correction. No intermediate commit exists.

The fixtures execute the production driver inside isolated containers. Each container has no host mounts, network, Docker socket, or extra capabilities.

UID 65534, a private process namespace, and resource limits contain the faults. No nodes start, and no developer filesystem receives cleanup commands.

## Matched results

B20 suspends the guardian during a stalled benchmark. B21 suspends the guardian during a stalled iteration.

Each observer checks client cancellation and failure publication after twelve seconds, before release. It also confirms that the guardian remains alive and suspended.

Both production RED results leave the client active without a summary at observation. Both matching formal controls violate `StaleGuardianRequiresInterrupt` with exit 12.

B20 adds progress timestamps and active benchmark checks. B21 adds the active iteration check. Each production GREEN records one protection failure before release.

Both corrected progress model runs have five distinct states. B21 reuses the unchanged model and its exact control.

B22 pauses the driver and guardian during a valid admission probe. After nine seconds, it verifies expired progress and resumes the driver first.

Both production RED paths admit work before active supervision stops it. The matching control violates `StaleProgressPreventsAdmission` with exit 12.

The correction checks progress before either work counter changes. Both production GREEN cases refuse all work and record one protection failure.

The corrected admission model has six distinct states. The model covers ages below, equal to, and above its limit.

## Verification and bindings

Nineteen positive configurations and twenty exact controls pass. The archive retains all 39 actual TLC logs before classifier tests execute.

The classifier passes 140 cases. Routing passes six scenarios. The emergency suite passes twenty-four scenarios. Supporting regressions also pass.

ShellCheck initially reported an unused uptime field. The field now uses `_unused`. The retained lint correction is not behavioral RED.

The [manifest](manifest.jsonc) binds each cycle's snapshots, final source files, original raw digests, and complete logs. Earlier packages remain unchanged.

External commit `706b11b6e` appeared during preparation and stopped the first builder attempt. Its executable source matches the tested snapshots. Its hooks remain unattested.

Published logs replace only the local evidence-root prefix. Separate digests and replacement counts identify those substitutions.

The [model correspondence](../../../formal/tlaplus/soak_disk/GuardianProgress.md) describes the implementation, startup grace, finite bounds, and limits.

## Remaining obligations

The configured silence threshold defaults to ten seconds. Integer timestamps, parent polling, stop commands, and publication add separate delays.

The initialization timestamp grants startup grace. It does not attest a completed guardian sample. Atomic rename does not establish durable storage.

These fixtures confirm client cancellation, not termination of every writer. Process identifier reuse, other progress-record faults, and later scheduling races remain open.

Cleanup ownership, durable publication, the complete response deadline, writer-growth reserve, hosted checks, and maintainer review remain pending.

D2, claim discharge, and acceptance remain pending. No workload or finalization-wait setting changes.
