# D2 Native Memory Preference Evidence: B34

B34 completes one additional local cycle.
D2 remains pending.

## Matched cycle

The baseline selects native processes by their command lines.
The fixture observes these actual kernel preference values:

| Process | Before | Baseline after | Corrected after |
| --- | ---: | ---: | ---: |
| Workload node | 0 | 1000 | 1000 |
| Unrelated node | 0 | 1000 | 0 |
| Unrelated client | 0 | 0 | 0 |

Production RED returns one because the unrelated node preference changes.
The exact formal RED violates `UnownedPreferencesPreserved` with exit 12 before the production correction.
Production GREEN returns zero, and the corrected model reaches two distinct states.

The correction checks the inherited owner value through an opened process directory.
It writes the selected preference through the same directory descriptor.
The native path no longer uses command-line selection or privileged fallback writes.

## Verification

The full gate passes 30 positive configurations and 32 exact controls.
All 62 actual TLC logs were saved before classifier mocks ran.
All 224 classifier cases, six routing scenarios, and 41 emergency cases pass.
The supporting driver, workflow, release, pin, metric, and summary regressions also pass.

The baseline is `e2321eafe7ccb6efcf9ce29866b690ff84853ab0`.
That external commit contains the prior host stop evidence and all current inventory inputs.
This session does not attest its hooks.
The prior evidence package remains unchanged.

## Limits and remaining work

The fixture uses real native processes and kernel preference files in a restricted private process namespace.
It uses healthy controlled memory samples and does not cause actual memory exhaustion.
Docker remains an external substitute in this fixture.
The unchanged Docker preference loop still needs its separate ownership correction and real-daemon test.

The [model correspondence](../../../formal/tlaplus/soak_disk/HostOomOwnership.md) states the ownership and atomicity assumptions.
Descriptor-relative access does not establish every race or permission outcome.
A preference value does not guarantee that the kernel will spare the runner.

B35 benchmark recovery, B36 Docker preference ownership, storage faults, durable publication, and a complete emergency deadline remain open.
D3 all-writer growth and reserve evidence remain required for full D2 discharge.
The user requested continuation after the bounded D3 scope question.
Node diagnostics still require their observability prerequisites and bounded diagnostic controls.
No real node workload, new VM, hosted dispatch, or soak ran in this cycle.

The [manifest](manifest.jsonc) binds source snapshots, raw evidence, and complete published streams.
Claims, gate statuses, and acceptance remain unchanged.
