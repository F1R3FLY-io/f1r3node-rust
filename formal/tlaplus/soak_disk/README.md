# Soak disk protection models

Two bounded TLA+ models cover the disk protection in
[run-merge-recovery-soak.sh](../../../scripts/run-merge-recovery-soak.sh).
Each model has one gating configuration and one pre-fix configuration per
historical defect. A pre-fix configuration switches one correction off through
a Boolean constant and must violate exactly the invariant named below.

## SoakDiskAdmission: the iteration-boundary decision

| Model action | Driver behavior |
| --- | --- |
| `ValidateSettings` | Reject the configuration outright when the floor, the band, or their sum does not fit in 64 bits (B24). Refuse work when the state file records an iteration (B29) or the opening benchmark left in flight by a crashed segment |
| `Benchmark` | Launch the opening benchmark on the first segment only when no breach marker was retained, a known sample is at or above floor plus band, and the guardian is alive. A guardian started first records a disk fall during the benchmark; a watched fault (breach or guardian death) cancels the benchmark and publishes the failure |
| `CheckGuardian` | Read the guardian marker before the probe and after hygiene |
| `ProbeBoundary`, `ProbeAfterHygiene` | `disk_free_mb`: `df` reports the free space, prints a malformed field, or fails |
| `DecideHygiene` | Run hygiene only when a known sample is below floor plus band |
| `Hygiene` | `reclaim_disk_space`, which can reclaim nothing, keeps every temporary session (B25), fails when any cleanup command fails (B26), and inspects Docker resources instead of pruning them (B27) |
| `HygieneStall`, `HygieneTick`, `HygieneReturns` | A hygiene client ignores TERM; under `SOAK_DISK_HYGIENE_SECONDS` it receives TERM, then KILL, and the driver refuses work (B23) |
| `DecideAfterHygiene` | Refuse a known sample below the threshold |
| `CheckAdmission` | Refuse a missing sample, a dead guardian process, a dead crash monitor, an unverified run domain, or expired guardian progress before starting work |
| `GuardianStall` | The guardian stays alive but its progress record expires before admission |
| `MonitorCrash` | The crash monitor dies before the benchmark admission or the iteration admission (B42) |
| `GuardianCrash` | The guardian process dies before the workload starts |
| `Admit` | Start the iteration |
| `PublishRefusal` | Write `protection-breach.txt`, `early-exit.txt`, and the failure summary |
| `Attribute` | Run the disk usage attribution after a disk breach. The corrected driver writes the records first (B48) |

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
| `RememberInFlight` | The state file records an iteration in flight. A segment that finds one counts a failure and refuses work, since writer termination is unconfirmed | `MC_SoakDiskAdmission_unrecorded_pre_fix` | `CrashRequiresRefusal` |
| `RememberBenchmark` | The state file records the opening benchmark in flight. A segment that finds one counts a failure and a benchmark failure and refuses work | `MC_SoakDiskAdmission_unrecorded_benchmark_pre_fix` | `BenchmarkCrashRequiresRefusal` |
| `WatchMonitor` | A crash monitor exit during the opening benchmark cancels it and publishes the failure. The pre-fix benchmark loop watched only the host guardian | `MC_SoakDiskAdmission_iteration_only_pre_fix` | `BenchmarkMonitorDeathObserved` |
| `CheckMonitorAlive` | A dead crash monitor cannot admit the benchmark or an iteration, and the refusal retains a failure. The pre-fix driver checked the monitor only at startup and mid-work | `MC_SoakDiskAdmission_unchecked_monitor_pre_fix` | `MonitorDeathPreventsAdmission` |
| `VerifyPlacement` | Under required containment, the benchmark and an iteration are admitted only when the trusted run-domain record matches the driver's own control group and uid. A failed comparison retains a failure. The pre-fix driver ignored the containment setting and took the unmanaged path (B45) | `MC_SoakDiskAdmission_unchecked_placement_pre_fix` | `UnverifiedPlacementPreventsAdmission` |
| `BindIdentity` | The record is trusted only when the driver opened each root-owned, unwritable path component from the filesystem root, within the size bound. The pre-fix driver inspected the pathname and read the record separately (B46). The `unchecked_placement` control omits this invariant, which a driver with no placement check also violates | `MC_SoakDiskAdmission_pathname_pre_fix` | `UntrustedRecordPreventsAdmission` |
| `RecordBeforeAttribution` | The driver writes the breach record and the early-exit record before any disk usage attribution starts, so a stalled attribution cannot delay them. The pre-fix driver attributed on the hygiene-pass path before it decided (B48) | `MC_SoakDiskAdmission_attribute_first_pre_fix` | `AttributionRequiresRecord` |

`MC_SoakDiskAdmission` enables all twenty-one corrections with all three benchmark fault kinds. It checks `TypeOK`, the twenty-one invariants above, `HygieneKillFollowsTerm`, `StopPreventsAdmission`, `RefusalRecorded`, and the liveness property `Completes`. It completes with 30352 distinct states.

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
| `MonitorCrash`, `WatcherPollMonitor` | The iteration watcher polls the crash monitor, and its death is a breach (B40) |
| `Drain` | The interrupted iteration's client has exited, and the driver waits for output EOF. The corrected driver stops the owned writers first (B43) |
| `ControllerLoss`, `ContainmentResponse` | The driver and the crash monitor die together. Only a service manager that owns the writers' control group stops them (B44, launcher prototype) |
| `Release` | The launcher verifies the manager placement and the gate identity, then releases the driver. A status query that never returns refuses the launch (B47, launcher prototype) |
| `Stall`, `WatcherPollStale` | The guardian is alive but its progress record has expired; the watcher reads the record (B20 benchmark, B21 iteration) |
| `DriverExit`, `ExitTrap` | The driver exits mid-iteration, and the corrected trap stops the writers (B28). Docker may reject the stop. The corrected trap then records a failure and a refusal (B31) |
| `MonitorObservesExit` | The trap's exit handling leaves a marker, and the crash monitor reads it before it acts (B39) |
| `DriverCrash`, `CrashMonitor` | The driver is killed without its trap. A crash monitor in its own session watches it through pidfd and runs the owner-labeled stop (B38) |
| `StartProbe`, `Tick`, `ProbeReturns` | The guardian runs `df` under `timeout` |
| `DecideSample` | A timed-out or empty probe supplies no sample |
| `Detect`, `Record`, `BeginStop` | Write `host-guardian-breach.txt`, then start `stop_node_writers` |
| `StopTick`, `StopReturns` | `pkill` and `docker kill` under `timeout` with TERM, then KILL |
| `AttributionTick`, `CompleteRoot` | `disk_diagnostics_bounded` walks every root under one deadline |
| `Finish`, `Recover`, `RestartDecision` | The next segment finds the retained marker |
| `LateWrite` | Writers whose termination was never confirmed keep consuming space after the stop returned |

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
| `StopOnExit` | The driver's EXIT trap stops the node writers it launched when it exits with an iteration or benchmark in flight | `MC_SoakDiskGuardian_client_only_pre_fix` | `ExitStopsWriters` |
| `RetainStopFailure` | A rejected stop command in the exit trap counts a failure, writes the early-exit reason, and refuses work, since termination is unconfirmed | `MC_SoakDiskGuardian_ignored_pre_fix` | `FailedStopRetained` |
| `SelectOwned` | Stop commands select only the containers that carry this run's owner label. The pre-fix stop killed every `rnode` container on the host | `MC_SoakDiskGuardian_name_only_pre_fix` | `UnownedWritersPreserved` |
| `SelectOwnedHost` | Host stops select only processes whose environment carries this run's owner marker, through pidfd. The pre-fix stop killed every process matching a path pattern | `MC_SoakDiskGuardian_host_pattern_only_pre_fix` | `UnownedHostWritersPreserved` |
| `MarkOwnedOnly` | Each guardian sample sets the OOM preference only on owner-marked processes. The pre-fix guardian marked every process matching the pattern | `MC_SoakDiskGuardian_oom_pattern_only_pre_fix` | `UnownedPreferencesPreserved` |
| `ConfigureAtCreation` | The container OOM preference is set once at creation through the owner-labeling wrapper. The pre-fix sample marked every container matching a name filter | `MC_SoakDiskGuardian_periodic_name_pre_fix` | `UnownedContainerPreferencesPreserved` |
| `SurvivesDriverCrash` | A crash monitor in its own session outlives a killed driver and runs the owner-labeled stop. The pre-fix driver had no monitor, and a monitor in the driver's process group would die with it | `MC_SoakDiskGuardian_parent_group_pre_fix` | `CrashStopsOwnedWriters` |
| `RememberHandledExit` | The driver's exit handling writes a handled-exit marker, and the crash monitor exits without a second stop when it finds the marker. The pre-fix monitor stopped the writers on every driver exit | `MC_SoakDiskGuardian_unconditional_pre_fix` | `HandledExitHasNoExtraStop` |
| `DetectMonitorDeath` | The iteration watcher treats a dead crash monitor as a breach, records unconfirmed termination, and refuses work. The pre-fix driver checked the monitor only at startup | `MC_SoakDiskGuardian_startup_only_pre_fix` | `DeadMonitorRequiresInterrupt` |
| `StopBeforeDrain` | The interrupted iteration path stops the owned writers before it waits for output EOF. The pre-fix driver drained first and hung on the pipe the writers held | `MC_SoakDiskGuardian_drain_first_pre_fix` | `DrainRequiresOwnedStop` |
| `ManagedContainment` | A service manager kills the owned writers' control group when both controllers die. The launcher prototype provides it, the direct launch does not, and B44 stays open on the source | `MC_SoakDiskGuardian_unmanaged_pre_fix` | `ControllerLossStopsOwnedWriters` |
| `VerifyBeforeRelease` | The launcher starts a trusted gate, verifies the manager placement and the gate identity, and only then releases the driver. The pre-fix launcher started the driver before its status query returned (B47, launcher prototype) | `MC_SoakDiskGuardian_start_first_pre_fix` | `UnavailableQueryPreventsRelease` |

`MC_SoakDiskGuardian` enables all twenty corrections. It also checks `TimedOutSampleRejected`, `PriorFailuresPreserved`, `KillFollowsTerm`, and the conditional theorem below. It completes with 22518 distinct states.

### Conditional no-overrun theorem

The model tracks free space as `freeMiB`. It starts at `HardFloorMiB`, the worst healthy sample. It falls by `WriteRateMax` for every clock unit the writers run: `SamplePeriod` units before the next probe, the probe units, and the stop units. The invariant `NoOverrun` states that `freeMiB` stays positive.

It holds under two premises:

| Premise | Definition | Control that drops it |
| --- | --- | --- |
| `FloorCoversReaction` | `HardFloorMiB > WriteRateMax * (SamplePeriod + ProbeDeadline + StopBudget)` | `MC_SoakDiskGuardian_rate_exceeds_floor_pre_fix` |
| `BoundTermination` | A completed stop ends consumption. Without it `LateWrite` continues for `LateUnits` | `MC_SoakDiskGuardian_unconfirmed_stop_pre_fix` |

Both controls violate `NoOverrun`, so each premise is necessary. The gating configuration uses `WriteRateMax = 10`, `SamplePeriod = 5`, and `HardFloorMiB = 150`, which leaves 50 MiB after a 10-unit reaction.

In production one unit is one second. The guardian sleeps 5 seconds, the probe deadline is 3 seconds, and the stop budget is 2 seconds. The reaction time is therefore 10 seconds.

The default floor of 4096 MiB puts the hard floor at 2048 MiB. The premise then requires the writers to consume less than about 205 MiB per second across any 10-second window. That rate is a measurement, not a derivation. The disk-usage timeline in the run artifact supplies it. The termination premise is the open D2 item: the driver does not confirm that the killed writers stopped.


## SoakStorageBudget: the consumer side

The guardian theorem takes `WriteRateMax` as given. This model derives it. Each consumer that fills the soak VM's volume is an event source with a per-event byte cap and a per-unit rate. `RateBound` is the sum of the products, and `WithinBudget` states that consumption over any prefix of the run stays within `RateBound` per unit. The gating configuration sums to 10, the value the guardian configuration uses.

| Model action | Consumer | Cap in the node today |
| --- | --- | --- |
| `StoreBlock` | block storage | no per-block byte cap found in casper validation. The inbound admission pipeline bounds queued bytes, not stored bytes |
| `ExecuteDeploy` | tuple-space state | `DeployStorageCap`, proven in [`deploy_storage`](../deploy_storage/README.md) from phlo accounting |
| `Checkpoint` | rspace history | no pruning or growth cap found |
| `Log` | node log files | daily rotation, no byte cap |
| `ContainerLog` | Docker log driver | the driver's file cap, when configured |
| `Tick` | one clock unit passes | |

| Constant | Assumption | Control that drops it | Expected violation |
| --- | --- | --- | --- |
| `CapBlocks` | A stored block adds at most `BlockBytesCap` bytes | `MC_SoakStorageBudget_uncapped_blocks_pre_fix` | `WithinBudget` |
| `CapLogs` | A unit of logging adds at most `LogBytesPerUnit` bytes | `MC_SoakStorageBudget_uncapped_logs_pre_fix` | `WithinBudget` |
| `CapHistory` | A checkpoint adds at most `CheckpointBytesCap` bytes | `MC_SoakStorageBudget_uncapped_history_pre_fix` | `WithinBudget` |

Each control shows that the budget fails without its cap. The three caps are the ones the node does not enforce. The theorem is therefore a list of the caps the node needs before the guardian's rate premise becomes a derivation. The deploy cap is the one bound that already is. `MC_SoakStorageBudget` completes with 5616 distinct states over a 3-unit horizon.

The LMDB environments open with a map size, which is a hard ceiling on the state stores. The failure it produces is a write error inside the node, not a guardian refusal. That path is not modeled here.

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

## Evidence packaging

New evidence packages keep rerun streams digest-only. The rerun patterns are `composed-*`, `final-*`, `supporting-*`, `emergency-*`, and `tlc-*.log`. The exact filename `final-real.txt` is the one exception to the `final-*` rule. Cycle transcripts, baseline records, initial records, per-cycle observations, `manifest.jsonc`, and `README.md` remain published.

Historical manifests keep their bytes. An authorized removal of published files records their paths, digests, and byte counts in a retention record. That record, `docs/claims/soak-evidence-retention.jsonc`, is bound to an archive of the removed bytes.

The manifest's `raw_archive.entries` records each stream's `raw_path`, `raw_sha256`, and `raw_bytes`. A rerun entry has no `published_streams` entry and no package file. Each new package README states that reruns are digest-only and names the raw archive with an `[EVIDENCE_ROOT]` path. The root `.gitignore` carries the same patterns, and a package must not add a local `.gitignore` with `!*.log`.

The source branch validates packages with a Ruby checker. This branch does not carry Ruby, so the checker is not here. The driver evidence record binds each package by the digest of its manifest, and the published-set rule does not change those digests.
