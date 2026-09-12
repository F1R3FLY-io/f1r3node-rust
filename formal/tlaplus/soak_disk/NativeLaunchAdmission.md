# Native launch admission

The selected fault makes the external service-status query stall before the launcher verifies containment.
The committed launcher starts the driver first.
The driver admits one iteration even though the launcher later refuses with exit 2.
The unchanged fixture reports this admission as RED with exit 1.
The unrelated writer continues before fixture cleanup.

The correction starts a trusted root gate instead of the driver.
The gate receives no caller workload environment through the service manager.
The launcher checks the service invocation, cgroup, gate identity, and selected service properties.
The launcher then publishes the placement record and a private release record.
The gate compares that release with its own identity before it drops privilege and executes the driver.
The workload environment requires the driver's run-domain check.

The same fault fixture passes against the correction.
The existing native controller-loss fixture also passes with the frozen B45 driver.
That regression confirms positive admission, owned-writer termination, unrelated-writer preservation, and failure retention across two actual restarts.
The two production fixtures are `scripts/bench/test-soak-native-admission.py` and `scripts/bench/test-soak-native-containment.py`.
Neither fixture changed for this cycle.

## Finite correspondence

| Model boundary | Executed boundary |
| --- | --- |
| `Start` without `VerifyBeforeRelease` | The baseline service executes the driver before the query completes. |
| `Start` with `VerifyBeforeRelease` | The corrected service starts the trusted gate without workload admission. |
| `Observe` with an unavailable query | The external query exceeds its command timeout, and the launcher refuses with exit 2. |
| `Observe` with an available query | The launcher accepts the selected manager and gate observations. |
| `Release` | The gate receives the matching release and executes the driver after the privilege drop. |
| `UnavailableQueryPreventsNativeAdmission` | The refused launch has no admitted workload. |
| `UnrelatedWriterPreserved` | The unrelated writer remains alive and continues to write before cleanup. |

The start-first control violates `UnavailableQueryPreventsNativeAdmission` with TLC exit 12 after three distinct states.
The corrected unavailable-query configuration passes with three distinct states.
The available-query configuration passes with four distinct states.
All formal outputs use private evidence paths rather than the shared gate output paths.

## Limits

The model abstracts status checking as one observation.
It does not prove the gate implementation, privilege isolation, filesystem trust, invocation authentication, durability, or a wall-clock deadline.
The available-query configuration checks successful completion, not every possible early-admission fault.
Construction is not applicable to this finite model.

The production cycle uses a guarded disposable runner with a real service manager.
The fault substitutes the external status-query command, and the workload and Docker commands are substitutes.
The root launcher must start from a trusted administrative context.
Caller workload environment values remain data until the verified gate drops privilege.
Adversarial descriptor access, metadata loss, failed stops, and controller-loss combinations need separate fault evidence.

Fixture cleanup and runner termination are not production containment results.
Private Docker containment, creation fencing, durable evidence, aggregate deadlines, reserve bounds, and normal workflow integration remain open.
The original direct-launch B44 counterexample remains unresolved.
This cycle does not complete B44, D2, claim discharge, or acceptance.
