# D2 Disk Settings Evidence

## Matched repair

B24 starts from `9c99de84e492acab09d71df62642bbe67e4eb75a`. Three production fixtures demonstrate admission with invalid disk settings.

The invalid floor and invalid band each use `9223372036854775808`. The third case combines floor `4096` with band `9223372036854771712`.

The third pair fits individually, but its sum exceeds signed 64-bit arithmetic. Each baseline case admits one iteration.

The correction normalizes decimal strings and validates each value before arithmetic. It checks the sum through subtraction from the maximum, which avoids overflow during validation.

All three corrected cases return configuration error 2 before admission. They produce no soak summary. Missing summary counters are not reported as zero.

The initial formal control returns 12 on `AdmissionRequiresValidDiskSettings`. The recursive positive model reaches its 120-second verification limit, which is not behavioral RED.

The final model uses explicit decimal column steps. Its fresh control violates the same invariant, and its positive configuration completes with 88 distinct states.

The original model, original counterexample, timeout, and corrected model remain separately retained. No production correction was necessary after the model arithmetic changed.

## Additional coverage

A maximum valid floor with band zero preserves disk refusal. A zero floor with maximum valid band preserves the explicit protection opt-out.

Both cases pass against baseline and corrected source. They provide characterization coverage, not additional repair cycles.

The expanded fixture still reproduces all three original faults. Its corrected five-case suite passes.

## Verification and identity

Twenty-one positive configurations and twenty-two exact controls pass. All 43 actual TLC logs were retained before mock-based tests executed.

The classifier passes 154 cases. Routing passes six scenarios. The emergency suite passes thirty scenarios. All supporting regressions pass.

The [manifest](manifest.jsonc) binds pre-verification executable inputs, final sources, raw evidence, and complete published logs. Earlier packages remain unchanged.

Published logs replace only the local evidence-root prefix. Separate hashes and replacement counts identify those substitutions.

External commits `4e9dd432b` and `556b944f3` contain the correction and final verification inputs. Those inputs match the snapshot. This session does not attest their hooks.

## Limits

The containers have no host mounts, network, Docker socket, or extra capabilities. UID 65534 and resource limits contain the fixtures. No real nodes start.

The finite model covers three invalid configurations and the valid default configuration. The two maximum-value production cases are outside that model domain.

The driver assumes signed 64-bit Bash arithmetic. This cycle does not cover every input string, filesystem location, sample fault, or later scheduling race.

Configuration refusal does not prove durable publication or a complete emergency deadline. Cleanup ownership, shutdown, reserve bounds, hosted checks, maintainer review, D2, and acceptance remain pending.

The [model correspondence](../../../formal/tlaplus/soak_disk/DiskSettingsAdmission.md) describes the arithmetic mapping and model limits. The workload and 45-second finalization wait remain unchanged.
