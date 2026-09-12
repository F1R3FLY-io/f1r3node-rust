# Benchmark Disk Admission

## Behavior

B16 requires a valid disk sample at or above floor plus band before benchmark admission. Refusal records a protection failure and prevents later iteration admission.

## Production correspondence

| Model element | Production behavior |
| --- | --- |
| `Probe` | The driver reads available space through `disk_free_mb`. |
| `sampleMiB` | The parser returns a whole-number MiB sample. |
| `FloorMiB + BandMiB` | The driver adds the configured disk floor and hygiene band. |
| `Decide` | The driver checks the sample before incrementing the benchmark count or starting the benchmark script. |
| `refused` | The driver sets the protection reason and clears the remaining execution deadline. |
| `Publish` | The driver records the protection failure and publishes its summary. |
| `BenchmarkRequiresBand` | Benchmark admission requires a sample at or above the threshold. |

The control disables the admission check. Its counterexample admits the benchmark with 7000 MiB available and an 8192 MiB threshold.

The corrected configuration explores 12 distinct states. It checks samples of 7000, 8192, and 16384 MiB with a 4096 MiB floor and band.

## Production fixture

The fixture runs the real driver and benchmark script in an isolated container. External disk and Docker commands supply the fault without starting node writers.

The disk sample stays at 7000 MiB. Cleanup cannot recover space. The Docker fixture records the opening startup request and returns failure.

Production RED requests one benchmark before the later iteration guard refuses work. Production GREEN records no benchmarks, no iterations, and one protection failure.

## Limits

The production fixture covers the low-space opening path only. The formal model also covers equality and sufficient space, but those cases lack production evidence here.

The shared function also serves interleaved benchmarks. B16 does not verify their complete orchestration or active benchmark supervision.

The model assumes stable valid samples and completing local writes. It does not prove Bash refinement, durable publication, writer termination, or a response deadline.

The correction refuses benchmark admission without attempting cleanup. It does not change the iteration cleanup policy, workload, or finalization wait.

D2 and claim discharge remain pending. The [evidence package](../../../docs/cbc-evidence/soak-d2-benchmark-band-2026-09-09/README.md) retains the local cycle.
