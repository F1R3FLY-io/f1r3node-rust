# D2 local emergency evidence

```yaml
artifact: scripts/run-merge-recovery-soak.sh
base_commit: ac94c17559cab399d42e84e8dd0792f8fe943668
claim_id: CLAIM-SOAK-001
scope: B7 through B12 local emergency behaviors
status: pending
verified_at: null
waiver: null
evidence: manifest.json
```

## Results

Each cycle starts with a production regression failure. Its matching formal counterexample precedes the production correction. Each corrected regression and bounded model then passes.

| Cycle | Regression behavior | Required formal RED invariant |
| --- | --- | --- |
| B7 | An unavailable disk sample stops an active iteration. | `InvalidSampleRequiresInterrupt` |
| B8 | The disk breach record precedes the Docker stop command. | `StopRequiresRecord` |
| B9 | Guardian death stops an active iteration. | `DeadGuardianRequiresInterrupt` |
| B10 | A timed-out disk command cannot supply a valid sample, even after it prints a numeric field. | `ProbeWithinDeadline` |
| B11 | Stalled attribution has one aggregate deadline across 32 session roots. | `AttributionWithinBudget` |
| B12 | A retained guardian breach prevents new work after restart. | `RetainedBreachStopsRestart` |

Every production RED regression exits 1. Every formal RED exits 12 with the specified invariant. Every selected GREEN regression and formal configuration exits zero.

The driver already returned failure in the B8 and B11 RED traces. Those regressions expose incorrect record ordering and an excessive attribution delay.

B12 tests prior failure counts of zero and two. Recovery records at least one failure without increasing an existing positive count. The original breach marker remains unchanged.

The [model-to-code map](../../../formal/tlaplus/soak_disk/EmergencyResponse.md) defines each abstraction and its limits. This package does not prove a composed emergency deadline.

## Isolation and retained inputs

The driver runs inside disposable containers with no host mounts, network, or Docker socket. The containers use UID 65534 and a private process namespace.

The fixtures drop all capabilities and use `no-new-privileges`. Admission fixtures use 256 MiB, one CPU, and 128 processes.

The driver has a 20-second outer fixture limit with a two-second kill grace. The container limit is 40 seconds with a five-second grace.

The separate driver suite uses 512 MiB, two CPUs, and 256 processes. Its container limit is 120 seconds with a five-second grace.

External disk, Docker, and workload commands are fixtures. Admission, guardian decisions, timeout handling, record publication, and restart handling remain production code.

No node workload runs. Disk cleanup executes only inside disposable containers. No developer filesystem is a cleanup target.

The manifest retains source digests, complete selected logs, raw record digests, and the verifier identity. Intermediate source snapshots remain separate from final candidate bindings.

Raw evidence remains outside Cargo output and Git. The backup is local, not off-host.

## Verification corrections

One B9 driver-suite run failed without diagnostic output. Follow-up runs reproduced a fixture readiness race at the resource CSV assertion.

The source CSV existed after the iteration marker. The snapshot process had copied other required files but had not yet copied that CSV.

The corrected fixture waits for all required files before checking them. It does not change production telemetry. Thirty corrected first-scenario repetitions passed.

The corrected full driver suite also passes. The original failure, diagnostic repetitions, timestamps, and source correction remain in the raw record.

Two combined verification commands reached their tool limits. Those unfinished checks are not behavioral RED. Separate completed runs replace them as accepted verification evidence.

B10 first tested a command that stalled before output. The retained test then added valid partial output before the stall. Production RED repeated before formal RED.

## Combined verification

The bounded formal gate passes 11 positive configurations and 11 exact negative controls. The classifier covers 77 cases. Routing covers six scenarios.

The combined emergency fixture passes seven scenarios across six cycles. D1, B5, B6, and the existing three-scenario driver suite also pass.

Workflow, release, repin, collector-extension, and summary regressions pass. The new emergency fixture and all six formal controls are registered in CI.

## Open obligations

D2 remains pending. Stop commands and cleanup commands still need complete bounds and fault coverage. Guardian health at every admission boundary remains unverified.

The complete emergency path still lacks a composed deadline. Confirmed writer termination, durable publication, and safe active-session cleanup remain open.

The attribution wrapper covers tag operations structurally. The retained stalled-command fixture tests `du`, not every metadata or Docker failure.

Recovery assumes a surviving marker and valid state file. It does not prove crash durability or exact recovery of multiple uncommitted failure events.

The timeout models assume timer service and effective cancellation. They do not prove Linux scheduling or termination of uninterruptible or independently detached processes.

Every-writer growth and reserve bounds require D3 evidence. Hosted execution, maintainer review, mandatory-scope ratification, and full-duration acceptance remain pending.

No claim is discharged. This work does not authorize a commit, push, hosted run, or soak.
