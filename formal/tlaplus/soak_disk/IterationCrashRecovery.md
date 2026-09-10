# Iteration Crash Recovery Correspondence

B29 kills the active driver's process group with `SIGKILL`. The kernel and filesystem remain active. A real Docker writer survives the process crash.

The fixture supervisor explicitly stops that writer before restart. This action isolates recovery accounting and does not demonstrate production crash-time writer termination.

The baseline admits new work and retains zero failures. The corrected driver refuses both restart attempts and retains one iteration with one failure.

`INFLIGHT_ITERATION` records three states in `.soak-state`: zero for no pending iteration, one for an uncommitted iteration, and two for retained interruption.

The driver records state one before workload launch. Normal result handling clears that state. Recovery records state two and increments the failure counter once.

Later restarts retain state two and refuse work without another increment. Legacy state files default to zero. Invalid values fail before admission.

A [committed outcome](../../../docs/Glossary.md#committed-outcome) concerns the driver's control record, not durable blockchain state.

`IterationCrashRecovery` represents one crash and two serial recovery attempts. Its corrected configuration has four distinct states. The negative control requires exit 12 on `CrashRequiresRefusal`.

The first positive run had an incomplete successor because a Boolean assignment lacked parentheses. The corrected model preserves the intended transitions and passes both repeated controls.

The model assumes atomic record replacement, a surviving state file, one driver, and weak fairness. It does not prove power-loss durability or concurrent recovery.

Benchmark crashes, additional crash windows, torn records, failed storage, complete writer shutdown, and durable upload remain pending. D2 and acceptance remain pending.

See the [B27–B29 evidence](../../../docs/cbc-evidence/soak-d2-real-system-2026-09-10/README.md).
