# D2 Host Stop Evidence: B32–B33

## Status

Two additional local cycles pass.
D2 remains pending because its full exit criterion requires D3 growth bounds, a complete response deadline, durable evidence, and maintainer review.
No real node workload or soak ran.
No new diagnostic VM was necessary for these private-process tests.

## Matched results

| Cycle | Production RED | Production GREEN | Exact invariant |
| --- | --- | --- | --- |
| B32 | Driver exit kills unrelated node and client writers by command-line patterns. | The detached workload writer stops, and both unrelated writers continue. | `UnownedHostWritersPreserved` |
| B33 | The memory stop path kills the unrelated node writer. | The shared ownership-aware stop preserves both unrelated writers. | `UnownedHostWritersPreserved` |

Each production RED returns one, and each production GREEN returns zero.
The exact formal controls return 12.
The corrected model reaches two distinct states.
The two negative configurations retain their different observed writer sets.

The baseline starts from `463992ed120f5c854ec01aea7ebdf2e260886b14` with the retained formatting overlay.
The source hashes, not the commit alone, identify that baseline.
B33 starts from the B32 correction.
The manifest records the later observed Git commit separately.

## Corrections

Workload processes inherit an owner value through their environment.
The shared stop helper checks that value and uses Linux process descriptors instead of command-line patterns.
The helper checks descriptor notifications after it requests termination.
Failure still means that termination is unconfirmed.

The memory guardian now uses the shared stop helper.
Its record states unconfirmed termination before it requests the stop.
It no longer claims that a global kill has already protected the host.

The [model correspondence](../../../formal/tlaplus/soak_disk/HostStopOwnership.md) records the ownership contract, pinned-provider behavior, and model limits.

## Verification and retained setup results

The composed gate passes 29 positive configurations and 31 exact controls.
All 60 actual TLC logs were saved before classifier mocks ran.
The classifier covers 217 cases, and routing covers six scenarios.
All 40 emergency cases and the supporting driver, workflow, release, pin, metric, and summary regressions pass.

The first B32 composed command reached its tool limit during classifier tests after the actual model checks completed.
Separate classifier and routing runs passed.
The initial command and its incomplete classifier output remain retained.

The B17 fixture originally required its client to execute a return callback.
The corrected host stop can terminate that client before the callback runs.
The fixture now checks actual client termination and still requires the breach record, stop request, failure result, and admission refusal.
The original fixture failure remains separate from production RED evidence.

The first B33 fixture armed its fault before it observed a healthy memory sample.
That setup failure returned two and does not supply behavioral RED.
The corrected fixture waits for the healthy sample before it arms the fault.

The initial current-input check found stale bindings after staged formatting changes.
Historical B30–B31 evidence remains unchanged.
The new inventory binds the current source without rewriting those historical source identities.

An exported `e821517e0` tree also lacks 57 historical TLC streams because the repository ignores `*.log`.
Its inventory check fails on a missing stream.
Package-local ignore exceptions now make the unchanged streams eligible for staging.
The new package uses the same exception.
This correction does not change historical evidence bytes or authorize a commit.

## Remaining D2 requirements

These fixtures use a restricted container, real Linux processes, and controlled external probes.
They do not exercise the real Docker daemon or actual memory exhaustion.
The earlier real-Docker evidence does not automatically validate this changed stop helper.

Ownership of memory-protection metadata updates, other launch forms, late process creation, and failed benchmark-stop recovery remain open.
Storage faults, complete crash accounting, durable publication, and a composed emergency deadline remain open.
D3 must establish all-writer growth and reserve bounds before full D2 discharge.

The existing diagnostic authorization excludes real node workloads.
Bounded D3 node diagnostics need explicit authorization before execution.
A soak still requires separate authorization.
Hosted verification, required-check enforcement, maintainer review, and claim discharge remain pending.

The [manifest](manifest.jsonc) binds snapshots, raw evidence, and complete published streams.
Published path substitutions retain their counts and both hashes.
Source bindings do not imply execution coverage for every source file.
