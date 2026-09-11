# Benchmark crash recovery

## Selected contract

After a driver crash during an active benchmark, two restarts refuse work and retain one interrupted benchmark failure.
The contract assumes that the state file survives the process crash without corruption.
It does not require successful writer termination.

## Correspondence

| Model element | Production operation |
| --- | --- |
| `RememberBenchmark` | Persist `INFLIGHT_BENCHMARK=1` and the benchmark count before launch. |
| `Crash` | Terminate the driver before a benchmark outcome or stop record exists. |
| First `Recover` | Change interruption state from 1 to 2 and increment both failure counters. |
| Second `Recover` | Retain state 2 and both failure counters without another increment. |
| `refused` | Set the deadline to zero and retain the interruption record. |
| `admitted` | Start another benchmark or iteration after recovery. |

The driver clears the interruption flag after it records the benchmark outcome.
A failed exit stop records state 2 immediately, so recovery does not count that interruption twice.
The B35 regression checks this interaction.

## Evidence

The [B37 record](../../../docs/cbc-evidence/soak-d2-benchmark-crash-2026-09-11/README.md) retains matching production and formal RED/GREEN results.
The production baseline starts new work after the crash.
The control violates `BenchmarkCrashRequiresRefusal` with exit 12.
The corrected configuration passes with four distinct states.

The real-system fixture uses the actual driver, benchmark helper, and Docker daemon on a guarded disposable virtual machine.
It substitutes the workload image and unavailable readiness endpoint, not admission, cancellation, persistence, or recovery decisions.
The restricted writer has no host mount or network access.
The fixture stops its recorded benchmark client group after the driver crash.
The Docker writer remains running until fixture cleanup.
This cleanup is not production shutdown evidence.

## Limits

The model uses atomic transitions, stuttering, and weak fairness.
It does not prove Bash refinement, scheduling bounds, or an aggregate emergency deadline.
The state writer uses a temporary file and rename without `fsync`.
Storage exhaustion, torn writes, failed synchronization, power loss, and durable upload remain outside this cycle.
Other crash windows, complete creation ownership, writer termination, reserve bounds, and maintainer review remain open.
D2 and claim discharge remain pending.
