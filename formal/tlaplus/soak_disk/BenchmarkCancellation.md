# Active Benchmark Cancellation

## Behavior B18

A guardian fault must not leave the driver blocked in benchmark execution. B18 covers guardian death and a recorded disk breach separately.

Both production fixtures hold the Docker startup client until an independent observer releases it after eight seconds. The client ignores `TERM`.

The observer checks for a complete protection-failure summary and an inactive client before release. It does not require node termination because no nodes start.

## Model correspondence

| Model action | Production operation |
| --- | --- |
| `Fault` | The fixture kills the guardian, or the real guardian records a 1024 MiB sample below its 2048 MiB hard floor. |
| `Watch` | The driver detects guardian death or its breach marker during the active benchmark. |
| `Term` | The driver requests writer stops, then sends `TERM` to the benchmark timeout supervisor. |
| `WaitGrace` | The client ignores termination while the timeout supervisor retains its kill grace. |
| `Kill` | The timeout supervisor cancels the command group. |
| `Publish` | The driver records a failed benchmark, a protection failure, and its local summary. |
| `ObserveUnwatched` | The old driver still waits for the stalled client when the fixture observer checks. |

The positive configuration includes both fault kinds. Each negative control selects one fault kind and disables benchmark supervision.

Both controls require TLC exit 12 and `BenchmarkCancellationObserved`. The positive configuration completes with fourteen distinct states.

## Implementation boundary

The benchmark runs under GNU `timeout` with a one-second kill grace. Its deadline uses the segment's remaining time.

A child shell retains the command group after `TERM`. Unlike an ignored signal, its handler permits descendants to receive normal termination signals.

The parent checks the guardian while it waits. On a fault, the parent requests writer stops and cancels the benchmark supervisor.

The driver records one protection failure and prevents later iterations. Exit cleanup also requests benchmark cancellation when a supervisor remains active.

The existing B17 fixture now records its observation on normal return or `TERM`. Its breach-record and stop-request checks remain unchanged.

## Limits

Abstract ticks are not seconds. Weak fairness does not prove a scheduling deadline. The model assumes completing local writes and effective group cancellation.

The eight-second observation applies only to these isolated fixtures. It does not establish the complete production emergency deadline.

The stopped process is a Docker client, not a node writer. Docker daemon operations, detached descendants, and uninterruptible processes remain outside this proof.

Guardian progress, process identifier reuse, durable publication, upload success, and writer-growth bounds remain open.

The timeout deadline and exit cleanup receive this implementation change, but their independent fault domains still need production tests.

D2, maintainer review, claim discharge, and acceptance remain pending.
