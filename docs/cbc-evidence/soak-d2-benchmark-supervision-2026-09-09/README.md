# D2 Benchmark Supervision Evidence

## Scope

This package retains two separate local repair cycles. B18 starts from commit `59b90568c435a53c04ee357c2f1f669775c5c219`.

B19 starts from the retained B18 correction. Its baseline uses source digests because no intermediate commit exists.

The fixtures execute the real driver and benchmark runner. They replace external commands inside isolated containers and start no nodes.

Each container uses UID 65534, a private process namespace, dropped capabilities, and resource limits. It has no host mounts, network, or Docker socket.

## B18: Active cancellation

The Docker startup client ignores `TERM` and waits for an independent observer. The observer checks cancellation and summary publication before releasing the client after eight seconds.

One fixture kills the guardian. The other supplies a hard-floor disk breach while the client remains active.

Both production RED results leave the client active and the failure summary unpublished at observation. The old driver publishes its failure only after fixture release.

Separate formal controls select guardian death and a disk breach. Both require exit 12 and `BenchmarkCancellationObserved` before the production correction.

The correction runs the benchmark in a timed command group. The parent watches guardian liveness and its breach marker while that group runs.

On a fault, the parent requests writer stops and cancels the benchmark supervisor. The supervisor retains a one-second kill grace for clients that ignore termination.

Both production GREEN results leave the client inactive and publish one protection failure before observation. No later iteration starts. Formal GREEN has fourteen distinct states.

## B19: Admission after guardian death

Both fixtures kill the guardian during a valid benchmark disk probe. One exercises opening admission. The other completes one iteration before interleaved admission.

Both production RED results increment the benchmark counter after guardian death. Active cancellation happens too late to make that admission safe.

The unchanged guardian-admission model supplies the matching formal RED. Its negative control requires exit 12 and `AdmissionRequiresGuardian`.

The correction checks guardian liveness and the breach marker after the disk sample. It refuses work before the benchmark counter changes.

Both production GREEN results record zero benchmarks and one protection failure. The interleaved case preserves its completed iteration. Formal GREEN has four distinct states.

## Composition and provenance

Seventeen positive configurations and eighteen exact controls pass. The archive retains all 35 actual TLC logs before classifier tests execute.

The classifier passes 126 cases. Routing passes six scenarios. The combined emergency suite passes twenty scenarios. Supporting regressions also pass.

The combined verification command reached its 600-second tool limit during release regressions. The partial run remains unchanged. The remaining checks passed separately.

A navigation tool could not parse the Bash source. Direct source reads supplied inspection. Neither tool limitation supplies behavioral RED.

The B17 fixture now records its observation on normal return or `TERM`. It still requires the breach record and stop request before that observation.

The [manifest](manifest.jsonc) binds per-cycle snapshots, final source files, complete logs, and original raw digests. Historical evidence packages remain unchanged.

Published logs replace only the local raw-root prefix. Replacement counts and separate original and published digests identify those substitutions.

The [cancellation correspondence](../../../formal/tlaplus/soak_disk/BenchmarkCancellation.md) and [admission correspondence](../../../formal/tlaplus/soak_disk/GuardianAdmission.md) state the assumptions and limits.

## Open obligations

These fixtures confirm cancellation of a stalled Docker client, not every node writer. Detached descendants, uninterruptible processes, and Docker daemon operations remain outside the tested domain.

A liveness check does not prove guardian progress or prevent process identifier reuse. The check and subsequent launch are not atomic.

Local records do not establish durable publication or successful upload. Abstract ticks and weak fairness do not establish the complete emergency deadline.

Cleanup ownership, remaining sample and restart faults, writer-growth bounds, reserve evidence, hosted verification, and maintainer review remain open.

D2, claim discharge, and acceptance remain pending. These cycles do not authorize a soak or change the workload and finalization wait.
