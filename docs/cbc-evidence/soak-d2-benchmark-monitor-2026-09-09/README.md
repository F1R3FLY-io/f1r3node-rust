# D2 Opening Benchmark Monitoring Evidence

## Scope

B17 checks a disk fault during the opening benchmark. Its baseline is `6e0b50f26be19f295c59eb233ac6eea297b9a72b`.

The user confirmed the remaining D2 scope. The other agent retains refactoring ownership. This cycle does not start a soak or change the workload.

The production fixture executes the real driver and benchmark runner. It replaces external disk, Docker, and workload commands inside an isolated container.

The fixture has no host mounts, network, Docker socket, or privileges. It uses UID 65534, a private PID namespace, and resource limits.

Available space starts at 16384 MiB. It falls to 1024 MiB while benchmark startup remains active for eight seconds, below the 2048 MiB hard floor.

The old driver starts its guardian after that benchmark returns. The correction moves the unchanged opening benchmark block after guardian startup.

## Matched results

| Check | Test / driver exit | Iterations | Protection failures | Benchmarks / failures |
| --- | --- | --- | --- | --- |
| Production RED | 1 / 0 | 1 | 0 | 1 / 1 |
| Production GREEN | 0 / 1 | 0 | 1 | 1 / 1 |

Production RED lacks a breach record and stop request during the active window. A later iteration cannot correct that missed fault.

Production GREEN records the hard-floor breach and requests a Docker stop before benchmark return. It then refuses another iteration and preserves one protection failure.

Formal RED requires TLC exit 12 and `BenchmarkBreachObserved`. Formal GREEN completes with five distinct states.

Both RED results precede the production correction. The fixture and formal inputs remain unchanged between those RED and GREEN runs.

## Verification

The serial bounded gate passes sixteen positive configurations and sixteen exact controls. The archive retains all 32 original configuration logs before classifier tests execute.

The classifier passes 112 cases. Routing passes six scenarios. The emergency suite passes twelve scenarios. Supporting driver, workflow, release, metrics, and summary regressions pass.

The [manifest](manifest.jsonc) binds source files, raw evidence, and complete logs. The [correspondence](../../../formal/tlaplus/soak_disk/BenchmarkDiskMonitor.md) states the model assumptions.

Published logs replace only the local raw-root prefix with the documented placeholder. The manifest records replacement counts and separate original and published digests.

## Invalid attempts

An initial fixture edit matched two regions. The corrected edit selected the source-file condition explicitly. The production driver remained unchanged.

The first fixture run lacked executable snapshot modes. The benchmark and summary helpers returned permission errors, and the driver produced a degraded summary.

That run exited 2 and does not supply behavioral RED. The repeated run uses verified Git modes and unchanged baseline bytes.

The raw archive retains both setup errors and the first run. No failed attempt was removed.

## Open obligations

The fixture releases the benchmark command after eight seconds. It does not establish parent cancellation of a stalled benchmark.

The model assumes a completing sample, local record, and stop request before benchmark return. Its abstract observation is not an atomic production operation.

Guardian crashes, stalled guardians, late admission faults, and other sample domains remain open. A stop request does not confirm writer termination.

Local writes do not prove durable publication or upload. The complete response deadline and writer-growth reserve remain unproved.

D2, maintainer review, hosted confirmation, claim ratification, claim discharge, and acceptance remain pending.
