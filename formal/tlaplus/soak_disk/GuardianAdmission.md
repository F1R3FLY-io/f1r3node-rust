# Guardian Admission Check

B14 checks guardian death during an iteration-boundary disk probe. The probe still returns a valid sample above the admission band.

The production fixture kills the guardian before the probe returns. It verifies that the driver creates no iteration and publishes one protection failure.

## Model correspondence

`Crash` represents guardian death before the final admission check. `Decide` represents the liveness check and its admission or refusal decision.

`CheckBeforeAdmission = FALSE` permits admission after the guardian dies. The negative control violates `AdmissionRequiresGuardian` with TLC exit 12.

The positive configuration checks `AdmissionRequiresGuardian`, `RefusalRecorded`, and eventual decision under weak fairness. It has four distinct states.

The production correction checks the guardian process identifier after the disk probe. It preserves an existing breach marker or records guardian death.

The driver then publishes the protection failure without incrementing the iteration count. Existing healthy-driver regressions still pass.

## Limits

The modeled decision is atomic. Bash does not make the liveness check and workload launch atomic. A later crash remains an active-supervision obligation.

A successful `kill -0` call does not establish guardian progress. This cycle does not cover a live but stalled guardian or process identifier reuse.

The fixture covers iteration admission, not benchmark admission or every startup and cleanup boundary. It does not prove writer termination or durable publication.

The fixture replaces external commands and runs no nodes. Source bindings and local model results do not discharge the resource claim or complete D2.
