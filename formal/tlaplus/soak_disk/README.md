# Soak disk protection models

Two bounded TLA+ models cover the disk protection in
[run-merge-recovery-soak.sh](../../../scripts/run-merge-recovery-soak.sh).
Each model has one gating configuration and one pre-fix configuration per
historical defect. A pre-fix configuration switches one correction off through
a Boolean constant and must violate exactly the invariant named below.

## SoakDiskAdmission: the iteration-boundary decision

| Model action | Driver behavior |
| --- | --- |
| `ValidateSettings` | Reject the configuration outright when the floor, the band, or their sum does not fit in 64 bits (B24) |
| `Benchmark` | Launch the opening benchmark on the first segment only when no breach marker was retained, a known sample is at or above floor plus band, and the guardian is alive. A guardian started first records a disk fall during the benchmark; a watched fault (breach or guardian death) cancels the benchmark and publishes the failure |
| `CheckGuardian` | Read the guardian marker before the probe and after hygiene |
| `ProbeBoundary`, `ProbeAfterHygiene` | `disk_free_mb`: `df` reports the free space, prints a malformed field, or fails |
| `DecideHygiene` | Run hygiene only when a known sample is below floor plus band |
| `Hygiene` | `reclaim_disk_space`, which can reclaim nothing, keeps every temporary session (B25), fails when any cleanup command fails (B26), and inspects Docker resources instead of pruning them (B27) |
| `HygieneStall`, `HygieneTick`, `HygieneReturns` | A hygiene client ignores TERM; under `SOAK_DISK_HYGIENE_SECONDS` it receives TERM, then KILL, and the driver refuses work (B23) |
| `DecideAfterHygiene` | Refuse a known sample below the threshold |
| `CheckAdmission` | Refuse a missing sample, a dead guardian process, or expired guardian progress before starting work |
| `GuardianStall` | The guardian stays alive but its progress record expires before admission |
| `GuardianCrash` | The guardian process dies before the workload starts |
| `Admit` | Start the iteration |
| `PublishRefusal` | Write `protection-breach.txt`, `early-exit.txt`, and the failure summary |

| Constant | Correction | Pre-fix configuration | Expected violation |
| --- | --- | --- | --- |
| `RequireBand` | Post-hygiene refusal compares against floor plus band, not the floor | `MC_SoakDiskAdmission_floor_only_pre_fix` | `AdmissionRequiresBand` |
| `RejectMissing` | A probe that returns nothing cannot admit | `MC_SoakDiskAdmission_missing_sample_pre_fix` | `AdmissionRequiresSample` |
| `RejectMalformed` | A field such as `16384junk` cannot admit | `MC_SoakDiskAdmission_numeric_prefix_pre_fix` | `AdmissionRequiresValidSample` |
| `CheckGuardianAlive` | A dead guardian process cannot admit an iteration (B14) or a benchmark (B19) | `MC_SoakDiskAdmission_unchecked_guardian_pre_fix` | `AdmissionRequiresGuardian` |
| `CheckRetainedBreach` | A retained breach marker blocks the opening benchmark | `MC_SoakDiskAdmission_retained_breach_pre_fix` | `RetainedBreachPreventsBenchmark` |
| `CheckDiskBand` | The opening benchmark needs a sample at or above floor plus band | `MC_SoakDiskAdmission_benchmark_band_pre_fix` | `BenchmarkRequiresBand` |
| `MonitorOpening` | The guardian starts before the opening benchmark, so a fall below the hard floor during it is recorded and stopped | `MC_SoakDiskAdmission_late_guardian_pre_fix` | `BenchmarkBreachObserved` |
| `WatchGuardian` | A guardian fault during the benchmark cancels it and publishes the failure; the stop, TERM, grace, and kill sequence is one step | `MC_SoakDiskAdmission_unwatched_death_pre_fix`, `MC_SoakDiskAdmission_unwatched_breach_pre_fix` | `BenchmarkCancellationObserved` |
| `CheckProgress` | Expired guardian progress cannot admit an iteration or a benchmark (B22) | `MC_SoakDiskAdmission_unchecked_progress_pre_fix` | `StaleProgressPreventsAdmission` |
| `EnforceHygieneDeadline` | Hygiene commands run under a deadline; a killed group is a protection breach | `MC_SoakDiskAdmission_unbounded_hygiene_pre_fix` | `HygieneWithinBudget` |
| `CheckRange` | Out-of-range disk settings reject the configuration before any work; whether a text is in range is abstracted, and the harness checks the three boundary texts | `MC_SoakDiskAdmission_unchecked_range_pre_fix` | `AdmissionRequiresValidDiskSettings` |
| `PreserveUnowned` | Hygiene never deletes a temporary session by age, since age proves neither ownership nor writer termination | `MC_SoakDiskAdmission_age_only_pre_fix` | `UnownedSessionPreserved` |
| `EnforceCleanupFailures` | A failed Docker cleanup command fails hygiene and refuses admission, whatever the later sample says | `MC_SoakDiskAdmission_ignore_errors_pre_fix` | `CleanupFailurePreventsAdmission` |
| `PreserveDockerResources` | Hygiene inspects exited containers, networks, images, and the build cache; it never prunes them, since the driver cannot tell its own resources from the host's | `MC_SoakDiskAdmission_global_prune_pre_fix` | `UnownedDockerResourcesPreserved` |

`MC_SoakDiskAdmission` enables all fourteen corrections with both benchmark fault kinds. It checks `TypeOK`, the fourteen invariants above, `HygieneKillFollowsTerm`, `StopPreventsAdmission`, `RefusalRecorded`, and the liveness property `Completes`. It completes with 12138 distinct states.

Constants: floor 4096 MiB, band 4096 MiB, free-space samples `{7000, 8191, 8192, 8193, 16384}`, initial free space 7000 MiB, malformed prefix 16384.

## SoakDiskGuardian: the emergency path

The model follows one iteration from the moment the guardian is watching it to the start of the next segment:

1. The iteration runs. The guardian process is alive, or it crashes and the watcher notices.
2. A live guardian runs `df` under a deadline. The probe returns promptly with or without a sample, or stalls and returns a valid field only after the deadline.
3. A sample below the hard floor is a breach. The guardian writes `host-guardian-breach.txt`, then starts `pkill` and `docker kill` under `SOAK_DISK_STOP_SECONDS`. Stalled stop clients receive TERM, then KILL, and the guardian moves on. An unavailable sample interrupts the iteration on its own.
4. Attribution (`df`, `du`, `docker system df`) runs under one aggregate deadline, however many session roots exist.
5. The segment ends with the marker on disk. The next segment reads it, refuses new work, and records a failure without lowering an existing count.

Each step corresponds to one historical defect and one correction constant. The model does not compose their timing into one deadline, does not confirm that the killed writers stopped, and does not prove that the marker survives a crash. Those remain D2 obligations.

| Model action | Driver behavior |
| --- | --- |
| `Crash`, `WatcherPoll` | The iteration watcher polls the guardian process |
| `Stall`, `WatcherPollStale` | The guardian is alive but its progress record has expired; the watcher reads the record (B20 benchmark, B21 iteration) |
| `StartProbe`, `Tick`, `ProbeReturns` | The guardian runs `df` under `timeout` |
| `DecideSample` | A timed-out or empty probe supplies no sample |
| `Detect`, `Record`, `BeginStop` | Write `host-guardian-breach.txt`, then start `stop_node_writers` |
| `StopTick`, `StopReturns` | `pkill` and `docker kill` under `timeout` with TERM, then KILL |
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
| `EnforceStopDeadline` | `pkill` and `docker kill` run under a deadline | `MC_SoakDiskGuardian_unbounded_stop_pre_fix` | `StopWithinBudget` |
| `CheckProgress` | A live guardian without recent progress counts as failed, and the work is interrupted | `MC_SoakDiskGuardian_alive_only_pre_fix` | `StaleGuardianRequiresInterrupt` |

`MC_SoakDiskGuardian` enables all eight corrections. It also checks `TimedOutSampleRejected`, `PriorFailuresPreserved`, and `KillFollowsTerm`. It completes with 6294 distinct states.

Clock units: the probe deadline is 3 units (a 2-second timeout plus a 1-second kill grace), a stalled `df` returns at 4 units, the stop budget is 2 units (TERM at 1, KILL at 2), and attribution has 1 unit for all roots. Root counts are 1, 3, and 32. Prior failure counts are 0 and 2.

## Counterexamples

Counterexamples are not stored. Each pre-fix configuration regenerates its historical counterexample deterministically: TLC explores the same finite state space and reports the named invariant with exit 12. The gate checks that message and exit code, and `scripts/ci/test-check-tla-invariants.sh` checks that the gate rejects every other outcome. A pre-fix configuration that stops violating its invariant is a verification failure, not a success.

## Limits

- The maps are reviewed abstractions of Bash, not refinement proofs.
- The models assume that local writes complete and that timers fire. They do not bound elapsed time, prove writer termination, or cover crash durability of the marker.
- The guardian model follows the breach path only. In production a healthy sample returns the guardian to polling.
- In the admission model the guardian liveness check and the workload start are one step. A guardian death inside that production window is not covered.
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

The per-cycle modules that preceded these two (`SoakDisk`, `DiskProbeAdmission`, `DiskSampleValidation`, `ActiveDiskProbe`, `DiskEmergencyRecord`, `GuardianSupervision`, `DiskProbeDeadline`, `DiskDiagnosticDeadline`, `DiskBreachRestart`, `DiskStopDeadline`, `GuardianAdmission`) are unregistered and scheduled for removal. See `docs/work-logs/task-soak-disk-hygiene-parsimonious-2026-09-09.md`.
