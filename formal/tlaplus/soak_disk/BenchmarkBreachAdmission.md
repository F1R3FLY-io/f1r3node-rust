# Retained Breach and Opening Benchmark Admission

## Behavior

B15 requires a retained guardian breach to prevent the opening benchmark when the state file is absent. A valid disk sample does not clear the breach.

## Production correspondence

| Model element | Production behavior |
| --- | --- |
| `phase = "benchmark"` | The first segment reaches the opening benchmark condition. |
| `marker` | A nonempty `host-guardian-breach.txt` exists before admission. |
| `Benchmark` | The driver either refuses the benchmark or calls `run_bench_segment`. |
| `CheckRetainedBreach` | The opening condition checks the retained marker before launching the benchmark. |
| `Recover` | The existing recovery block records the breach and sets a zero failure count to one. |
| `RetainedBreachPreventsBenchmark` | A retained breach prevents the benchmark's Docker startup request. |

The control sets `CheckRetainedBreach = FALSE`. Recovery can still record a failure after the prohibited benchmark request. That later failure does not make admission safe.

## Verification boundary

The production fixture runs the actual driver and `run-bench-segment.sh`. It replaces external disk, Docker, and workload commands in an isolated container.

The Docker fixture records the startup request and returns failure. No node starts. The fixture preserves the original breach and checks the final counters.

Production RED has one benchmark segment and one failure. Production GREEN has no benchmark segments, no iterations, and one failure.

The corrected model explores six states, including a path without a retained marker. The production fault fixture covers the retained-marker path only.

## Limits

The marker stays unchanged during the modeled decision. The model does not cover a later breach, concurrent marker replacement, or unreadable metadata.

This cycle does not establish benchmark supervision, safe startup without a marker, confirmed writer termination, durability, or an emergency deadline.

D2 and `CLAIM-SOAK-001` remain pending. The [evidence package](../../../docs/cbc-evidence/soak-d2-benchmark-2026-09-09/README.md) records this local cycle.
