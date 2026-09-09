# Soak disk protection models

Two bounded TLA+ models cover the disk protection in
[run-merge-recovery-soak.sh](../../../scripts/run-merge-recovery-soak.sh).
Each model has one gating configuration and one pre-fix configuration per
historical defect. A pre-fix configuration switches one correction off through
a Boolean constant and must violate exactly the invariant named below.

## SoakDiskAdmission: the iteration-boundary decision

| Model action | Driver behavior |
| --- | --- |
| `CheckGuardian` | Read the guardian marker before the probe and after hygiene |
| `ProbeBoundary`, `ProbeAfterHygiene` | `disk_free_mb`: `df` reports the free space, prints a malformed field, or fails |
| `DecideHygiene` | Run hygiene only when a known sample is below floor plus band |
| `Hygiene` | `reclaim_disk_space`, which can reclaim nothing |
| `DecideAfterHygiene` | Refuse a known sample below the threshold |
| `CheckAdmission` | Refuse a missing sample before starting work |
| `Admit` | Start the iteration |
| `PublishRefusal` | Write `protection-breach.txt`, `early-exit.txt`, and the failure summary |

| Constant | Correction | Pre-fix configuration | Expected violation |
| --- | --- | --- | --- |
| `RequireBand` | Post-hygiene refusal compares against floor plus band, not the floor | `MC_SoakDiskAdmission_floor_only_pre_fix` | `AdmissionRequiresBand` |
| `RejectMissing` | A probe that returns nothing cannot admit | `MC_SoakDiskAdmission_missing_sample_pre_fix` | `AdmissionRequiresSample` |
| `RejectMalformed` | A field such as `16384junk` cannot admit | `MC_SoakDiskAdmission_numeric_prefix_pre_fix` | `AdmissionRequiresValidSample` |

`MC_SoakDiskAdmission` enables all three corrections. It checks `TypeOK`, the three invariants above, `StopPreventsAdmission`, `RefusalRecorded`, and the liveness property `Completes`. It completes with 160 distinct states.

Constants: floor 4096 MiB, band 4096 MiB, free-space samples `{7000, 8191, 8192, 8193, 16384}`, initial free space 7000 MiB, malformed prefix 16384.

## SoakDiskGuardian: the emergency path

| Model action | Driver behavior |
| --- | --- |
| `Crash`, `WatcherPoll` | The iteration watcher polls the guardian process |
| `StartProbe`, `Tick`, `ProbeReturns` | The guardian runs `df` under `timeout` |
| `DecideSample` | A timed-out or empty probe supplies no sample |
| `Detect`, `Record`, `BeginStop` | Write `host-guardian-breach.txt`, then `pkill` and `docker kill` |
| `AttributionTick`, `CompleteRoot` | `disk_diagnostics_bounded` walks every root under one deadline |
| `Finish`, `Recover`, `RestartDecision` | The next segment finds the retained marker |

| Constant | Correction | Pre-fix configuration | Expected violation |
| --- | --- | --- | --- |
| `DetectDeath` | The watcher treats a dead guardian as a breach | `MC_SoakDiskGuardian_unwatched_pre_fix` | `DeadGuardianRequiresInterrupt` |
| `RejectUnavailable` | A probe without a sample interrupts the iteration | `MC_SoakDiskGuardian_unavailable_sample_pre_fix` | `InvalidSampleRequiresInterrupt` |
| `EnforceTimeout` | `df` runs under a deadline and late output is discarded | `MC_SoakDiskGuardian_unbounded_probe_pre_fix` | `ProbeWithinDeadline` |
| `RecordFirst` | The breach record precedes the stop command | `MC_SoakDiskGuardian_stop_first_pre_fix` | `StopRequiresRecord` |
| `AggregateDeadline` | Attribution has one budget for all roots | `MC_SoakDiskGuardian_per_root_deadline_pre_fix` | `AttributionWithinBudget` |
| `PreserveBreach` | A restart keeps the marker and records a failure | `MC_SoakDiskGuardian_cleared_breach_pre_fix` | `RetainedBreachStopsRestart` |

`MC_SoakDiskGuardian` enables all six corrections. It also checks `TimedOutSampleRejected` and `PriorFailuresPreserved`. It completes with 1140 distinct states.

Clock units: the probe deadline is 3 units (a 2-second timeout plus a 1-second kill grace), a stalled `df` returns at 4 units, and attribution has 1 unit for all roots. Root counts are 1, 3, and 32. Prior failure counts are 0 and 2.

## Limits

- The maps are reviewed abstractions of Bash, not refinement proofs.
- The models assume that local writes complete and that timers fire. They do not bound elapsed time, prove writer termination, or cover crash durability of the marker.
- The guardian model follows the breach path only. In production a healthy sample returns the guardian to polling.
- No mandatory CbC attribute is assigned. `CLAIM-SOAK-001` is proposed and unratified. See [soak-disk-protection.md](../../../docs/claims/soak-disk-protection.md).

## Running the checks

Both gating configurations and all nine controls are registered in `scripts/ci/check-tla-invariants.sh` and run in the bounded pull-request tier:

```bash
TLA_TOOLS_JAR="$HOME/.tla/tla2tools.jar" bash scripts/ci/check-tla-invariants.sh --soak-pr
```

The production regressions run the real driver in a disposable container:

```bash
bash scripts/bench/test-soak-disk-admission.sh                  # every scenario
bash scripts/bench/test-soak-disk-admission.sh --scenario band  # one scenario
```

Do not run the historical pre-fix driver directly on a host. Its sweep ignores the root overrides.

Evidence: [docs/cbc-evidence/scripts-run-merge-recovery-soak-sh.md](../../../docs/cbc-evidence/scripts-run-merge-recovery-soak-sh.md).

The per-cycle modules that preceded these two (`SoakDisk`, `DiskProbeAdmission`, `DiskSampleValidation`, `ActiveDiskProbe`, `DiskEmergencyRecord`, `GuardianSupervision`, `DiskProbeDeadline`, `DiskDiagnosticDeadline`, `DiskBreachRestart`) are unregistered and scheduled for removal. See `docs/work-logs/task-soak-disk-hygiene-parsimonious-2026-09-09.md`.
