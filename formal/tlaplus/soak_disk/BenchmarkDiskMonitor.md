# Opening Benchmark Disk Monitoring

## Behavior B17

The opening benchmark must not run before guardian startup when disk protection is enabled.

The production fixture starts with 16384 MiB available. It supplies 1024 MiB while the benchmark Docker startup command remains active for eight seconds.

The floor is 4096 MiB. The hard floor is 2048 MiB. The real guardian retains its five-second sampling interval.

The Docker fixture records a stop request only when the breach record exists before benchmark return. It starts no node writers.

## Model correspondence

| Model action | Production boundary |
| --- | --- |
| `StartBenchmark` | The opening call follows guardian startup only in the corrected driver. |
| `DiskFalls` | The Docker fixture marks benchmark startup. Later disk probes return 1024 MiB until that command returns. |
| `Observe` | The guardian samples the hard-floor breach, writes its marker, and requests a Docker stop. |
| `ReturnBenchmark` | The Docker fixture records its observation before it returns. The old driver starts its guardian afterward. |

`MonitorOpening = FALSE` reproduces the old order. The exact negative control requires TLC exit 12 and `BenchmarkBreachObserved`.

`MonitorOpening = TRUE` checks the corrected order. Both configurations use the same initial sample, fault sample, and hard floor.

The correction moves the existing opening benchmark block after guardian startup. It does not change that block, the workload, or the finalization wait.

## Assumptions and limits

The model assumes that a started guardian completes one sample, local record, and stop request before benchmark return. The fixture checks this one execution window.

`Observe` combines those operations. Their real execution is not atomic. Weak fairness supplies abstract completion, not a wall-clock deadline.

The model has five reachable states. It does not prove Bash refinement, scheduling bounds, guardian progress, or safety after a guardian crash.

A stop request does not confirm writer termination. A local record does not establish durable publication or successful upload.

The Docker command returns because the fixture releases it. Parent cancellation of a stalled benchmark remains unproved.

Moving the block also places existing recovery and finalize handling before opening admission. Those additional paths need their own production checks.

D2, claim discharge, and acceptance remain pending.
