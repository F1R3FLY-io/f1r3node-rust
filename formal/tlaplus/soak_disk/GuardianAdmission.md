# Guardian Admission Check

B14 checks guardian death during an iteration-boundary disk probe. The probe still returns a valid sample above the admission band.

The production fixture kills the guardian before the probe returns. It verifies that the driver creates no iteration and publishes one protection failure.

## Model correspondence

`Crash` represents guardian death before the final admission check. `Decide` represents the liveness check and its admission or refusal decision.

`CheckBeforeAdmission = FALSE` permits admission after the guardian dies. The negative control violates `AdmissionRequiresGuardian` with TLC exit 12.

The positive configuration checks `AdmissionRequiresGuardian`, `RefusalRecorded`, and eventual decision under weak fairness. It has four distinct states.

The production correction checks the guardian process identifier after the disk probe. It preserves an existing breach marker or records guardian death.

The driver then publishes the protection failure without incrementing the iteration count. Existing healthy-driver regressions still pass.

## Benchmark correspondence B19

B19 applies the same admission contract to opening and interleaved benchmarks. It reuses the unchanged positive configuration and exact negative control.

Each fixture kills the guardian during the valid benchmark disk probe. The interleaved fixture first completes one iteration and then reaches its configured benchmark boundary.

The disk shim distinguishes guardian probes through process ancestry. It injects the fault only into the driver-side admission probe.

`admitted` corresponds to the benchmark counter increment and command launch. A later cancellation cannot make that earlier admission safe.

The shared benchmark function now checks guardian liveness and the breach marker after its disk sample, before it increments the benchmark counter.

Production GREEN records no benchmark admissions and one protection failure. The interleaved case preserves its one completed iteration.

## Limits

The modeled decision is atomic. Bash does not make the liveness check and workload launch atomic. A later crash remains an active-supervision obligation.

A successful `kill -0` call does not establish guardian progress. This cycle does not cover a live but stalled guardian or process identifier reuse.

B14 covers iteration admission. B19 covers two benchmark admission paths, not every startup and cleanup boundary. Neither cycle proves writer termination or durable publication.

The fixture replaces external commands and runs no nodes. Source bindings and local model results do not discharge the resource claim or complete D2.
