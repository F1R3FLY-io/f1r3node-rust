---
task: soak-disk-hygiene-parsimonious
branch: fix/parsimonious-maintainble-soak-disk-hygiene
source_branch: fix/soak-disk-hygiene-stop
source_pr: 399
claimed_by: claude-session-00e7a6fd
claimed_at: 2026-09-09T13:00:00Z
handoff_status: paused
next_steps:
  - Wait for the agent on fix/soak-disk-hygiene-stop to confirm it has stopped merging in.
  - Run the phase-two removal list below, rerun the four checks, then commit once.
  - Open a PR to dev from this branch and close or supersede PR #399.
  - Maintainer actions carried over from G0 stay open (required check for TLA+ invariant check on dev).
---

# Parsimonious rewrite of PR #399

## Why

PR #399 grew to 229 files and about 21,000 added lines against `dev`. About 16,300 of those lines are generated evidence (TLC logs, JSON manifests, run dumps), 1,280 are a 256-file SHA-256 inventory whose CI step failed whenever any digested file changed, and 1,016 are nine near-duplicate TLA+ modules with 18 configurations. The driver fix itself is about 170 lines.

Decisions taken with the user on 2026-09-09: condense evidence to short records, drop the digest check but keep a claims document, consolidate the models, and work additively first so that merges from the source branch keep landing cleanly.

## Phase one (done, uncommitted in this checkout)

Functionally equivalent replacements exist beside the originals:

| Area | Before | After |
| --- | --- | --- |
| TLA+ | 9 modules, 18 configs, 4 READMEs | `SoakDiskAdmission.tla`, `SoakDiskGuardian.tla`, 11 configs, 1 README |
| Gate | 9 soak entries, 4 copies of the control list | 2 soak entries; `NEGATIVE_CONTROLS` is the only copy |
| Gate tests | `test-check-tla-invariants.sh` + `test-soak-pr-formal-gate.sh` | one `test-check-tla-invariants.sh` that reads the registry |
| Harness | 388-line harness + 9 wrappers + 4 ci.yml steps | table-driven harness, one ci.yml step, `--scenario` flag |
| Fixture image | `ruby` (about 900 MB) | `debian:bookworm-slim` + jq + procps |
| Claims | 75 KB digest inventory + validator + ci.yml step | `docs/claims/soak-disk-protection.md`, trimmed `soak-formal-gate.md` |
| Evidence | 144 files | 3 records of about 60 lines each |

Verified on this host on 2026-09-09:

| Check | Result |
| --- | --- |
| `check-tla-invariants.sh --soak-pr` with real TLC | 4 clean, 11 exact controls, 22 s |
| `test-check-tla-invariants.sh` | PASS, 11 controls times 7 outcomes, 6 routing cases |
| `test-soak-disk-admission.sh` (all 11 scenarios, Docker) | PASS |
| `test-run-merge-recovery-soak.sh` (host, 3 scenarios) | PASS |
| `check-workflow-invariants.sh`, `test-release-workflows.sh` | PASS |

Unchanged on purpose (the source's B25 later changed `test-run-merge-recovery-soak.sh` to expect a retained stale session, and that change is carried as is): `scripts/run-merge-recovery-soak.sh`, `test-run-merge-recovery-soak.sh`, the three harness repin sites, `docs/plans/*`, `docs/tdd-plans/*`, the source branch's work log, and `docs/Glossary.md`.

## Phase two (pending the source agent's confirmation)

The historical manifests are digest-bound in the two `docs/cbc-evidence/*.md` records and the run README before removal, so the source agent's raw store stays verifiable.

Remove the superseded files, then rerun the five checks above:

```bash
# superseded TLA+ modules and their configurations
git rm formal/tlaplus/soak_disk/{SoakDisk,DiskProbeAdmission,DiskSampleValidation,ActiveDiskProbe,DiskEmergencyRecord,GuardianSupervision,DiskProbeDeadline,DiskDiagnosticDeadline,DiskBreachRestart,DiskStopDeadline,GuardianAdmission,BenchmarkBreachAdmission,BenchmarkDiskAdmission,BenchmarkDiskMonitor,BenchmarkCancellation,GuardianProgress,GuardianProgressAdmission,DiskSettingsAdmission,CleanupOwnership,CleanupOutcome,DockerCleanupOwnership,DockerExitStop,IterationCrashRecovery,DockerStopFailure,DockerStopOwnership,HostStopOwnership,HostOomOwnership,DockerOomOwnership,BenchmarkCrashRecovery,DriverCrashStop,CrashMonitorExit,CrashMonitorDeath,BenchmarkMonitorDeath,MonitorAdmission,InterruptedOutputDrain,ControllerLoss,NativeControllerLoss,RunDomainAdmission,RunDomainRecordIdentity,NativeLaunchAdmission,BreachRecordOrder,NativeControlPath}.tla
git rm formal/tlaplus/soak_disk/MC_{SoakDisk,DiskProbeAdmission,DiskSampleValidation,ActiveDiskProbe,DiskEmergencyRecord,GuardianSupervision,DiskProbeDeadline,DiskDiagnosticDeadline,DiskBreachRestart,DiskStopDeadline,GuardianAdmission,BenchmarkBreachAdmission,BenchmarkDiskAdmission,BenchmarkDiskMonitor,BenchmarkCancellation,GuardianProgress,GuardianProgressAdmission,DiskHygieneDeadline,DiskSettingsAdmission,CleanupOwnership,CleanupOutcome,DockerCleanupOwnership,DockerExitStop,IterationCrashRecovery,DockerStopFailure,DockerStopOwnership,HostStopOwnership,HostOomOwnership,DockerOomOwnership,BenchmarkCrashRecovery,DriverCrashStop,CrashMonitorExit,CrashMonitorDeath,BenchmarkMonitorDeath,MonitorAdmission,InterruptedOutputDrain,ControllerLoss,NativeControllerLoss,RunDomainAdmission,RunDomainRecordIdentity,NativeLaunchAdmission,BreachRecordOrder,NativeControlPath}*.{tla,cfg}
git rm formal/tlaplus/soak_disk/{DiskProbeAdmission,DiskSampleValidation,EmergencyResponse,DiskStopDeadline,GuardianAdmission,BenchmarkBreachAdmission,BenchmarkDiskAdmission,BenchmarkDiskMonitor,BenchmarkCancellation,GuardianProgress,DiskHygieneDeadline,DiskSettingsAdmission,CleanupOwnership,CleanupOutcome,DockerCleanupOwnership,DockerExitStop,IterationCrashRecovery,DockerStopFailure,DockerStopOwnership,HostStopOwnership,HostOomOwnership,DockerOomOwnership,BenchmarkCrashRecovery,DriverCrashStop,CrashMonitorExit,CrashMonitorDeath,BenchmarkMonitorDeath,MonitorAdmission,InterruptedOutputDrain,ControllerLoss,NativeControllerLoss,RunDomainAdmission,RunDomainRecordIdentity,NativeLaunchAdmission,BreachRecordOrder,NativeControlPath}.md
# harness wrappers and aggregators (the harness runs every scenario itself)
git rm scripts/bench/test-soak-disk-{active-probe,diagnostic-deadline,emergency,probe-timeout,probe,record,restart,sample,stop-deadline}.sh scripts/bench/test-soak-guardian-death.sh scripts/bench/test-soak-guardian-admission.sh scripts/bench/test-soak-benchmark-restart.sh scripts/bench/test-soak-benchmark-disk-admission.sh scripts/bench/test-soak-benchmark-disk-monitor.sh scripts/bench/test-soak-benchmark-admission-cases.sh scripts/bench/test-soak-benchmark-cancellation.sh scripts/bench/test-soak-benchmark-guardian-admission.sh scripts/bench/test-soak-guardian-progress.sh scripts/bench/test-soak-disk-hygiene-deadline.sh scripts/bench/test-soak-disk-settings.sh scripts/bench/test-soak-cleanup-ownership.sh scripts/bench/test-soak-cleanup-outcomes.sh
# digest inventory and its validators (the ci.yml steps are already gone)
git rm docs/claims/soak-claim-inventory.jsonc docs/claims/soak-evidence-retention.jsonc scripts/ci/test-soak-claim-inventory.sh scripts/ci/test-soak-claim-inventory-jsonc.sh
# generated evidence (condensed records replace them)
git rm -r docs/cbc-evidence/soak-g0-2026-09-08 docs/cbc-evidence/soak-g0-b2-2026-09-08 docs/cbc-evidence/soak-g0-b3-2026-09-08 \
  docs/cbc-evidence/soak-d2-probe-2026-09-08 docs/cbc-evidence/soak-d2-sample-2026-09-09 docs/cbc-evidence/soak-d2-emergency-2026-09-09 docs/cbc-evidence/soak-d2-stop-2026-09-09 docs/cbc-evidence/soak-d2-boundary-2026-09-09 docs/cbc-evidence/soak-d2-benchmark-2026-09-09 docs/cbc-evidence/soak-d2-benchmark-band-2026-09-09 docs/cbc-evidence/soak-d2-benchmark-monitor-2026-09-09 docs/cbc-evidence/soak-d2-benchmark-cases-2026-09-09 docs/cbc-evidence/soak-d2-benchmark-supervision-2026-09-09 docs/cbc-evidence/soak-d2-guardian-progress-2026-09-09 docs/cbc-evidence/soak-d2-hygiene-2026-09-09 docs/cbc-evidence/soak-d2-settings-2026-09-10 docs/cbc-evidence/soak-d2-cleanup-session-2026-09-10 docs/cbc-evidence/soak-d2-cleanup-outcome-2026-09-10 \ docs/cbc-evidence/soak-d2-crash-stop-2026-09-11 docs/cbc-evidence/soak-d2-monitor-death-2026-09-11 docs/cbc-evidence/soak-d2-shutdown-2026-09-11 docs/cbc-evidence/soak-d2-native-containment-2026-09-11 docs/cbc-evidence/soak-d2-run-domain-2026-09-12 docs/cbc-evidence/soak-d2-record-identity-2026-09-12 docs/cbc-evidence/soak-d2-breach-record-2026-09-12 docs/cbc-evidence/soak-d2-native-admission-2026-09-12
  docs/cbc-evidence/github-workflows-slashing-tests-yml.md docs/cbc-evidence/scripts-ci-test-soak-claim-inventory-sh.md \
  docs/soak-evidence/g0-hosted-d6aaba962-2026-09-08 docs/soak-evidence/repin-962effd-2026-09-08
git rm docs/soak-evidence/34180346282/{archive-members-sha256.json,inspect.rb,iterations.csv,manifest.json,observations.json,phases.csv,raw-metrics-inspection.json,storage.json}
# formatting-only churn against dev
git show origin/dev:scripts/bench/collect-soak-metrics.sh > scripts/bench/collect-soak-metrics.sh
git show origin/dev:scripts/bench/write-soak-summary.sh > scripts/bench/write-soak-summary.sh
```

After phase two the README paragraph about legacy modules in `formal/tlaplus/soak_disk/README.md` should be removed, and the B3 row in `docs/cbc-evidence/scripts-ci-check-tla-invariants-sh.md` already explains the inventory's removal. Also remove the "JSONC file migration" section from `docs/claims/soak-formal-gate.md`. It describes the inventory validator, and it arrived with the 2026-09-09 JSONC merge.

## Merges from the source branch

- 2026-09-09, `e6fdd343b` (B13, bounded stop commands): driver change taken as is. The `DiskStopDeadline` model folded into `SoakDiskGuardian` as `EnforceStopDeadline` with `StopWithinBudget` and `KillFollowsTerm`; the `stop-timeout` scenario added to the harness table; conflicts in the gate, both tests, the harness header, and the gate claim resolved toward this branch; `test-soak-pr-formal-gate.sh` stays deleted.

- 2026-09-09, `3498fa4f3` (B14, guardian death at the admission boundary): driver change taken as is. `GuardianAdmission` folded into `SoakDiskAdmission` as `CheckGuardianAlive` with `AdmissionRequiresGuardian`; `guardian-death-boundary` added to the harness table; the same five conflicts resolved toward this branch.

- 2026-09-09, `37ec7f71d` (JSON to JSONC renames): the 13 evidence manifests, the claim inventory, and the assistant instruction files taken as is. The three conflicts were the inventory steps in `ci.yml`, the B1 pointer in the gate evidence record, and the inventory artifacts plus the open-obligation list in the gate claim. All three resolved toward this branch. The digest tables in the two evidence records now use the `.jsonc` names, and the digests are unchanged. The renamed inventory, the new `test-soak-claim-inventory-jsonc.sh`, and the "JSONC file migration" section stay in the tree and are on the phase-two list.

- 2026-09-09, `7f0f46923` (B15, retained breach before the opening benchmark): driver change taken as is. `BenchmarkBreachAdmission` folded into `SoakDiskAdmission` as `CheckRetainedBreach` with `RetainedBreachPreventsBenchmark`; the model now starts in a `benchmark` phase and chooses whether a marker was retained. `restart-benchmark` added to the harness table (its fixture body auto-merged; the table, the source list, and the header row were added by hand). `run-bench-segment.sh` joined `SOURCE_FILES` for every scenario. Conflicts in the gate, the gate test, the harness header, and `docs/ToDos.md` (both status bullets kept) resolved toward this branch; `test-soak-pr-formal-gate.sh` stays deleted. Counts updated to 12 soak controls and 14 scenarios.

- 2026-09-09, `6e0b50f26` (B16, disk threshold before the opening benchmark): driver change taken as is. `BenchmarkDiskAdmission` folded into the `Benchmark` action of `SoakDiskAdmission` as `CheckDiskBand` with `BenchmarkRequiresBand`; the benchmark phase now probes a sample (any raw sample) and refuses below floor plus band, so the model grew to 996 states. `benchmark-band` added to the harness table (its fixture body auto-merged). Conflicts in the gate, the gate test, the harness (two hunks: the scenario parser kept, the widened fixture condition taken), and `docs/ToDos.md` (the source's updated status bullet replaces its older copy) resolved toward this branch; `test-soak-pr-formal-gate.sh` stays deleted. Counts updated to 13 soak controls and 15 scenarios.

- 2026-09-09, `59b90568c` (B17, guardian started before the opening benchmark, plus four coverage cases): driver change taken as is. `BenchmarkDiskMonitor` folded into the `Benchmark` action of `SoakDiskAdmission` as `MonitorOpening` with `BenchmarkBreachObserved`: an admitted benchmark may suffer a disk fall, and only a guardian started first records it (the marker is set) and asks for a stop; 1014 states. Five scenarios added to the harness table (`benchmark-active-disk`, `benchmark-equal`, `benchmark-sufficient`, `benchmark-missing`, `benchmark-disabled`); their fixture bodies auto-merged. The source's two wrappers, its monitor module, and both evidence directories are on the phase-two list. Same conflicts as B16, resolved the same way. Counts updated to 14 soak controls and 20 scenarios.

- 2026-09-09, `8ab599e5c` (B18 benchmark cancellation on guardian faults; B19 guardian liveness at benchmark admission): driver change taken as is. B18 folded into the `Benchmark` action of `SoakDiskAdmission` as `WatchGuardian` plus a `BenchmarkFaults` constant (`"breach"`, `"death"`) with `BenchmarkCancellationObserved`; two controls, one per fault kind, as on the source. The stop, TERM, grace, and kill sequence is one step; `TermBeforeStop` was not carried. B19 needed no new control: `CheckGuardianAlive` now also gates benchmark admission and `AdmissionRequiresGuardian` covers it. 1038 states. Four scenarios added to the harness table (`benchmark-cancel-death`, `benchmark-cancel-breach`, `benchmark-guardian-boundary`, `benchmark-guardian-interleaved`); bodies auto-merged. Same conflicts as B17, resolved the same way. Counts updated to 16 soak controls and 24 scenarios.

- 2026-09-09, `706b11b6e` (B20, B21 stalled-guardian interruption; B22 stale progress at admission): driver change taken as is. `GuardianProgress` folded into `SoakDiskGuardian` as `CheckProgress` with a `stale` variable, a `Stall` action, and `WatcherPollStale` ending in `progress-decided`; invariant `StaleGuardianRequiresInterrupt`, one control `alive_only` as on the source (6294 states). `GuardianProgressAdmission` folded into `SoakDiskAdmission` as `CheckProgress` with `guardianFresh`, a `GuardianStall` action shaped like `GuardianCrash`, and the freshness check at both admissions; invariant `StaleProgressPreventsAdmission`, control `unchecked_progress` (2097 states). Four scenarios added to the harness table (`benchmark-cancel-stall`, `guardian-stall`, `guardian-progress-boundary`, `benchmark-progress-boundary`); bodies auto-merged. The evidence directory arrived one commit later (`5549561e1`, ToDos-only conflict); its manifest digest is in the record. Same conflicts as B18, resolved the same way. Counts updated to 18 soak controls and 28 scenarios.

- 2026-09-09, `9c99de84e` (B23, hygiene under a command deadline): driver change taken as is. The source reused the legacy `DiskStopDeadline` module with a renamed invariant; here the hygiene client's stall, TERM, KILL, and breach live in `SoakDiskAdmission` as `EnforceHygieneDeadline` with `HygieneStall`, `HygieneTick`, `HygieneReturns`, a `hygiene-stalled` phase, and the `HygieneWithinBudget` and `HygieneKillFollowsTerm` invariants; control `unbounded_hygiene` (5673 states). `hygiene-timeout` added to the harness table; body auto-merged. No evidence directory arrived with this cycle. Same conflicts as B20, resolved the same way. Counts updated to 19 soak controls and 29 scenarios.

- 2026-09-09, `4e9dd432b` (B23 evidence package; B24 out-of-range disk settings rejected before admission): driver change taken as is. The source added `DiskSettingsAdmission` (decimal-string arithmetic over four texts) and three harness scenarios but registered no control and recorded no cycle. Here `CheckRange` joins `SoakDiskAdmission` with a `config` phase, a `rejected` terminal phase, and `AdmissionRequiresValidDiskSettings`; range validity is a Boolean, and the harness covers the three boundary texts; control `unchecked_range` (5709 states). `disk-floor-range`, `disk-band-range`, and `disk-sum-range` added to the harness table; bodies auto-merged. The B23 manifest is now digest-bound. Conflicts in the harness parser and `docs/ToDos.md` only, resolved the same way. Counts updated to 20 soak controls and 32 scenarios.

- 2026-09-10, `d64ae3bbf` (B24 evidence and the source's own settings control; B25 no age-based deletion of temporary sessions): driver change taken as is, including the source's change to the host suite. `CleanupOwnership` folded into `SoakDiskAdmission` as `PreserveUnowned` with `sessionAge` and `sessionPresent`: `Hygiene` and `HygieneReturns` keep the session unless the pre-fix sweep runs on an old one; invariant `UnownedSessionPreserved`, control `age_only` as on the source (11418 states). The source's `MC_DiskSettingsAdmission_unchecked_pre_fix` registration resolved toward this branch's `unchecked_range`. `disk-max-floor`, `disk-max-band`, and `cleanup-active-session` added to the harness table; bodies auto-merged. The B24 manifest is now digest-bound. Same conflicts as B23, resolved the same way. Counts updated to 21 soak controls and 35 scenarios.

- 2026-09-10, `14ffb4d3a` (B25 evidence; B26 a failed cleanup command fails hygiene): driver change taken as is. `CleanupOutcome` folded into `SoakDiskAdmission` as `EnforceCleanupFailures` with `cleanupFailed`: `Hygiene` and `HygieneReturns` now share `HygieneCompletes`, which draws a cleanup outcome and, when enforced, takes the B23 refusal path; invariant `CleanupFailurePreventsAdmission`, control `ignore_errors` as on the source (12138 states). Seven scenarios added to the harness table (five `cleanup-error-*`, `cleanup-partial`, `cleanup-sufficient`); bodies auto-merged. The B25 manifest is now digest-bound. Same conflicts as B25, resolved the same way. Counts updated to 22 soak controls and 42 scenarios. The B26 evidence package arrived one commit later (`1e2dc07f4`, ToDos-only conflict) and is digest-bound.

- 2026-09-10, `3b7904ebf` (B27, hygiene inspects Docker resources instead of pruning them): driver change taken as is. The five hygiene commands are now `docker inspect`, `network ls`, `image ls`, and `system df`, and the harness fixture bodies auto-merged to the new command texts. `DockerCleanupOwnership` folded into `SoakDiskAdmission` as `PreserveDockerResources` with `dockerPresent`, set by `HygieneCompletes`, invariant `UnownedDockerResourcesPreserved`, control `global_prune` as on the source. The positive state count stays at 12138 because the corrected hygiene never changes `dockerPresent`. No new harness scenario, since the source added a real-daemon check instead (`scripts/bench/test-soak-real-docker-ownership.sh`, kept as is, not a wrapper). No evidence directory arrived with this cycle.
  - Conflicts in the gate registry and the gate test resolved toward this branch. `test-soak-pr-formal-gate.sh` stays deleted.
  - This merge was redone on top of the remote branch tip, which had taken dev (#413, #418) from another machine.
  - Counts updated to 23 soak controls and 42 scenarios.

- 2026-09-10, `f4111f6b5` (B28 writers stopped on driver exit; B29 refusal after a crashed iteration): driver change taken as is, and it auto-merged around the disk-usage timeline. `IterationCrashRecovery` folded into `SoakDiskAdmission` as `RememberInFlight` with `interrupted`, decided in `ValidateSettings` after the range check, invariant `CrashRequiresRefusal`, control `unrecorded` as on the source (12258 states). `DockerExitStop` folded into `SoakDiskGuardian` as `StopOnExit` with `DriverExit`, `ExitTrap`, and `exitStop`, invariant `ExitStopsWriters`, control `client_only` as on the source (6342 states). The source's invariant name `ParentExitStopsFixtureWriter` was not carried, since it names the harness fixture.
  - No new harness scenario. The source added two real-daemon checks, `test-soak-real-docker-shutdown.sh` and `test-soak-real-crash-recovery.sh`, kept as is.
  - Conflicts in the gate registry and the gate test resolved toward this branch. `test-soak-pr-formal-gate.sh` stays deleted.
  - Counts updated to 25 soak controls and 42 scenarios.

- 2026-09-10, `f3366cfab` (B30 stop commands select owner-labeled writers; B31 a rejected exit-trap stop is retained): driver change taken as is. The driver now mints an owner id per run and puts a `docker` wrapper on the workload's PATH that labels every container it creates. The driver conflict was the timeline's segment-start row against the wrapper block, and both sides are kept. `DockerStopFailure` folded into `SoakDiskGuardian` as `RetainStopFailure` with `exitRejected` and `exitFailureRetained` drawn in `ExitTrap`, invariant `FailedStopRetained`, control `ignored` as on the source. `DockerStopOwnership` folded as `SelectOwned` with `unownedStopped` set in `ExitTrap` and `BeginStop`, invariant `UnownedWritersPreserved`, control `name_only` as on the source. 6366 states.
  - The source's B27 to B29 real-daemon evidence package (`52c91cfe4`) is digest-bound. Its three correspondence documents are on the phase-two list.
  - The README conflict was the source's real-system section against the theorem table, resolved toward this branch. `docs/ToDos.md` keeps this branch's bullets and the source's newer status line.
  - Counts updated to 29 soak controls and 42 scenarios.

- 2026-09-10, `463992ed1` (source registers its B30 and B31 controls and shims owner labels in the harness fixture): no new behavior. The harness fixture's `docker` now answers the owner-label `ps`, `inspect`, `kill`, and `compose` calls, and that change auto-merged. Conflicts in the gate registry and the gate test resolved toward this branch. `test-soak-pr-formal-gate.sh` stays deleted. Counts unchanged.

- 2026-09-10, `e821517e0` (B30 and B31 real-daemon evidence): no new behavior. The driver change is a formatting rewrap of the owner check, auto-merged. The `soak-d2-owned-stop-2026-09-10` manifest is digest-bound. The source numbers ownership as B30 and the rejected stop as B31. The labels in the model comments, the README, and the evidence rows now follow that order. `docs/ToDos.md` keeps this branch's bullets and the source's newer status line.

- 2026-09-11, `13d867bc6` (B32 and B33 host stops select owner-marked processes on the exit and memory paths; B34 OOM preference only on owner-marked processes): driver change taken as is. Workload processes now carry `SOAK_PROCESS_OWNER` in their environment. Host stops select by that marker through pidfd with a bounded wait, and the guardian marks OOM preference the same way. The driver refuses to start without Linux pidfd support, so the host suite cannot run on macOS at all now.
  - `HostStopOwnership` folded into `SoakDiskGuardian` as `SelectOwnedHost` with `unownedHostStopped` set in `ExitTrap` and `BeginStop`, invariant `UnownedHostWritersPreserved`, one control `host_pattern_only`. The source's two controls for that model, `pattern_only` and `memory_pattern`, differ only in collateral and map to this one. `HostOomOwnership` folded as `MarkOwnedOnly` with `unownedMarked` set in `StartProbe`, invariant `UnownedPreferencesPreserved`, control `oom_pattern_only`. 6366 states.
  - The harness fixture change for the benchmark client auto-merged. The source's host check `test-soak-host-stop-ownership.sh` and the real-daemon `test-soak-real-benchmark-stop-recovery.sh` are kept as is.
  - Both manifests are digest-bound. Conflicts in the gate registry, the gate test, and ToDos resolved toward this branch. `test-soak-pr-formal-gate.sh` stays deleted.
  - Counts updated to 31 soak controls and 42 scenarios.
  - Numbering note: the source plan numbers the exit-path host stop as B32, the memory-path host stop as B33, and the host OOM preference as B34. The rows and comments now follow that order.

- 2026-09-11, `b56a408a2` (B36, container OOM preference set at creation): driver change taken as is. The owner-labeling `docker` wrapper now passes the OOM score adjustment at container creation when the host floor is on. The guardian's periodic scan over `rnode`-named containers is gone.
  - `DockerOomOwnership` folded into `SoakDiskGuardian` as `ConfigureAtCreation` with `unownedContainersMarked` set in `StartProbe`, invariant `UnownedContainerPreferencesPreserved`, control `periodic_name` as on the source. 6366 states.
  - No evidence directory arrived with this cycle. Conflicts in the gate registry and the gate test resolved toward this branch. `test-soak-pr-formal-gate.sh` stays deleted. Counts updated to 32 soak controls and 42 scenarios.

- 2026-09-11, `69332f716` (B35 characterization and B36 evidence): no new behavior. The source's B35 is a characterization with no driver change. A restart after an unconfirmed benchmark writer stop refuses work and retains the failure, which B29 and B31 already cover. B36 is the container OOM preference folded in the previous cycle, so the labels now follow the source plan.
  - The `soak-d2-container-preference-2026-09-11` manifest is digest-bound. The driver change is a heredoc terminator reformat, auto-merged. `docs/ToDos.md` keeps this branch's bullets and the source's newer status line.

- 2026-09-11, `64b02472c` (B37, crashed opening benchmark refuses the next segment): driver change taken as is. The state file now carries `INFLIGHT_BENCHMARK` beside `INFLIGHT_ITERATION`. A resumed segment that finds it set counts a failure and a benchmark failure, writes the early-exit reason, and refuses work. `BenchmarkCrashRecovery` folded into `SoakDiskAdmission` as `RememberBenchmark` with `benchmarkInterrupted`, decided in `ValidateSettings` beside the B29 case, invariant `BenchmarkCrashRequiresRefusal`, control `unrecorded_benchmark`. 12498 states.
  - The source's real-daemon check `test-soak-real-benchmark-crash-recovery.sh` is kept as is. No evidence directory arrived. Conflicts in the gate registry and the gate test resolved toward this branch. `test-soak-pr-formal-gate.sh` stays deleted. Counts updated to 33 soak controls and 42 scenarios.

- 2026-09-11, `84e6dd67f` (B37 evidence and the CbC verification tiers document): no new behavior. The source plan now numbers the benchmark crash recovery as B37, and the labels follow it. The `soak-d2-benchmark-crash-2026-09-11` manifest is digest-bound.
  - The source added `docs/cbc-verification-tiers.md`, a design note on refutation with TLC, construction with Rocq, and binding with Rust tests. It also added a row to the docs index and the formal verification guide. Those auto-merged and are kept as is. `docs/ToDos.md` keeps this branch's bullets and the source's newer status line.
  - CI fix (2026-09-11): the Docker harness failed on PR #406 (run 34563006539). The driver now refuses to start without Python 3 and pidfd support, and this branch's slim fixture image had no Python. The source never saw it because its fixture image is the full `ruby` image, which ships Python. `soak-disk-test.Dockerfile` now installs `python3`, and the harness tool check requires it. Docker is not running on this host, so the PR's CI run is the verification.

- 2026-09-11, `5a211fb5c` (driver crash stop, no source plan number yet): driver change taken as is. The driver starts a crash monitor in its own session before work begins. The monitor watches the driver through pidfd and runs the owner-labeled stop when the driver exits without its trap. A failed stop writes `writer-stop-failure.txt`, and the driver refuses work when the monitor cannot start.
  - `DriverCrashStop` folded into `SoakDiskGuardian` as `SurvivesDriverCrash` with `crashStop`, actions `DriverCrash` and `CrashMonitor`, invariant `CrashStopsOwnedWriters`, control `parent_group` as on the source. The monitor's stop reuses the ownership variables, so the unowned-writer invariants cover it too. 6414 states.
  - The source's real-daemon check `test-soak-real-crash-stop.sh` is kept as is. The benchmark crash fixture now rejects the stop so B37 still exercises restart refusal. No evidence directory arrived. Conflicts in the gate registry and the gate test resolved toward this branch. `test-soak-pr-formal-gate.sh` stays deleted. Counts updated to 34 soak controls and 42 scenarios.
  - Numbering note: the source commit kept its plan entry pending. The next source commit numbered it B38, and the labels now follow it.

- 2026-09-11, `bef79ba48` (B39, the crash monitor skips its stop after a handled exit): driver change taken as is. The exit handler writes a handled-exit marker in the monitor directory, and the monitor reads it after the driver exits. A handled exit gets no second stop. The source plan now carries B38 (crash stop) and B39 (monitor exit).
  - `CrashMonitorExit` folded into `SoakDiskGuardian` as `RememberHandledExit` with `repeatedStop` set in a new `MonitorObservesExit` action after `ExitTrap`, invariant `HandledExitHasNoExtraStop`, control `unconditional` as on the source. 6462 states.
  - The driver conflicted on the crash-monitor directory variable beside the timeline's segment-start row. Both sides are kept, the variable first. Conflicts in the gate registry and the gate test resolved toward this branch. `test-soak-pr-formal-gate.sh` stays deleted.
  - The source's claim inventory rebind and plan entries auto-merged and stay as is. No evidence directory arrived. Counts updated to 35 soak controls and 42 scenarios.

- 2026-09-11, `3f8de9cd3` (B38 and B39 evidence): no new behavior. The `soak-d2-crash-stop-2026-09-11` manifest is digest-bound. The package holds 696 files, and it is on the phase-two removal list with the earlier packages.
  - The source marked B38 and B39 done in its plan and added a glossary entry for the crash monitor. It also added model correspondence notes for its two standalone specs. Those auto-merged and stay as is. The two notes are on the phase-two `.md` list.
  - `docs/ToDos.md` keeps this branch's bullets and the source's newer status line. No driver, model, or gate change arrived, so the counts stay at 35 soak controls and 42 scenarios.

- 2026-09-11, `ab9519a5d` (B40, crash monitor death during an iteration is a breach): driver change taken as is. The iteration watcher now polls the crash monitor beside the host guardian. A dead monitor writes the breach marker with an unconfirmed-termination reason. The iteration is interrupted, the owned writers stop, one failure is retained, and work is refused.
  - `CrashMonitorDeath` folded into `SoakDiskGuardian` as `DetectMonitorDeath` with `monitorAlive`, actions `MonitorCrash` and `WatcherPollMonitor` beside the guardian death pair, invariant `DeadMonitorRequiresInterrupt`, control `startup_only` as on the source. The breach then follows the existing stop and retention path. 12948 states.
  - The source's host check `test-soak-crash-monitor-death.sh` joins its emergency suite and is kept as is. The driver conflicted on the crash-monitor pid variable beside the timeline's segment-start row. Both sides are kept, the variable first.
  - Conflicts in the gate registry and the gate test resolved toward this branch. `test-soak-pr-formal-gate.sh` stays deleted. No evidence directory arrived. Counts updated to 36 soak controls and 42 scenarios.

- 2026-09-11, `5e1842688` (B40 evidence and the published-set rule): no driver, model, or gate change. The source removed 819 tracked rerun files across 23 packages and added the rerun patterns to the root `.gitignore`. Rerun streams are now digest-only in the manifest's raw archive record. The nine package-local `.gitignore` files stay, and the builder stops emitting them.
  - The `soak-d2-monitor-death-2026-09-11` manifest is digest-bound. Historical manifests are unchanged, so the earlier digest rows stay valid. The package is on the phase-two removal list with the earlier packages, and the `CrashMonitorDeath` correspondence note is on the phase-two `.md` list.
  - The source added a Ruby package checker, `scripts/ci/check-soak-evidence-package.rb`, and its regression suite. The maintainer decided that no Ruby code ships in this work, so both files and the source's old `inspect.rb` are removed from this branch. The gate fixture test's routing check, which parsed the workflow with Ruby, now uses a stdlib-only Python scan. Its extracted run script is byte-identical to the Ruby result, and it rejects the same four mutations.
  - `formal/tlaplus/soak_disk/README.md` conflicted on the evidence paragraph. This branch's text stays, and a condensed evidence packaging section records the published-set rule. `docs/ToDos.md` keeps this branch's bullets and the source's newer status line. Counts stay at 36 soak controls and 42 scenarios.

- 2026-09-11, `84a9c8fbd` (rerun cut finished, packaging gate in CI): no driver, model, or gate change. The source removed the 187 prior-cycle real-system retrievals from the crash-stop package. It recorded their paths, digests, and byte counts in `docs/claims/soak-evidence-retention.jsonc`, bound to an archive. Historical manifests keep their bytes, so the digest rows stay valid.
  - The source added a repository-wide mode to its Ruby package checker and a CI step in the Lint job that runs it. This branch carries no Ruby. The two checker files stay deleted, and `.github/workflows/ci.yml` resolves toward this branch without the step. The retention record is a companion of the claim inventory and goes with it in phase two.
  - `formal/tlaplus/soak_disk/README.md` conflicted again on the evidence packaging section. This branch's section stays, with one sentence added for the retention record.

- 2026-09-11, `cdc0a57f5` (B41 to B43 crash monitor coverage, B44 open defect): driver change taken as is. The benchmark loop and both admission checks now poll the crash monitor beside the host guardian, and the interrupted iteration stops the owned writers before it waits for output EOF. The source commit numbers these B41 to B44. The plan carried B41 and B42 at the time, and the next cycle added B43 and B44.
  - `BenchmarkMonitorDeath` folded into `SoakDiskAdmission` as `WatchMonitor` with a third benchmark fault kind, `monitor-death`, invariant `BenchmarkMonitorDeathObserved`, control `iteration_only` as on the source. `BenchmarkCancellationObserved` now covers the two guardian fault kinds only, so each control violates exactly one invariant.
  - `MonitorAdmission` folded into `SoakDiskAdmission` as `CheckMonitorAlive` with `monitorAlive` and `benchmarkMonitorAlive`, action `MonitorCrash` beside `GuardianCrash`, checks in `Benchmark` and `CheckAdmission`, invariant `MonitorDeathPreventsAdmission`, one control `unchecked_monitor`. The source's two controls, `benchmark_unchecked` and `iteration_unchecked`, map to this one. 25146 states.
  - `InterruptedOutputDrain` folded into `SoakDiskGuardian` as `StopBeforeDrain` with `pipeHeld` and `drained`, action `Drain` at the finished phase, invariant `DrainRequiresOwnedStop`, control `drain_first` as on the source. 22452 states.
  - `ControllerLoss` (B44) is an open defect with no correction. When the driver and the crash monitor die together, the owned writers run on. The source's current configuration shows the violation and is registered nowhere. Nothing is folded, and the standalone spec is on the phase-two list.
  - The source's four host checks join its emergency suite and are kept as is. Conflicts in the gate registry and the gate test resolved toward this branch. `test-soak-pr-formal-gate.sh` stays deleted. No evidence directory arrived. Counts updated to 39 soak controls and 42 scenarios.

- 2026-09-11, `1f9427958` (B41 to B44 shutdown evidence): no driver, gate, or registered model change. The `soak-d2-shutdown-2026-09-11` manifest is digest-bound. The package holds the B41 to B43 RED and GREEN transcripts and the B44 counterexample, with reruns digest-only. It is on the phase-two removal list with the earlier packages.
  - The source plan marks B41 to B43 done and adds the B43 and B44 entries. B44 stays open and blocked on a reviewed containment design. The source revised its standalone `ControllerLoss` model with an observation phase and its fixture to wait for owned-writer termination. Both keep their RED results and stay unregistered, so nothing is folded here.
  - The source added a glossary entry for controller loss and updated its plans and work log. Those auto-merged and stay as is. `docs/ToDos.md` keeps this branch's bullets and the source's newer status line. Counts stay at 39 soak controls and 42 scenarios.

- 2026-09-11, `1c2679e06` (B44 native containment prototype): no driver change. The source added a launcher that runs the driver in a private service-manager unit with control-group kill, no restart, and no shared Docker socket. The normal workflow does not use it, and B44 stays open on the source with private Docker containment and creation fencing pending.
  - `NativeControllerLoss` folded into `SoakDiskGuardian` as `ManagedContainment` with `containedStop`, actions `ControllerLoss` and `ContainmentResponse` beside the B38 crash pair, invariant `ControllerLossStopsOwnedWriters`, control `unmanaged` as on the source. The constant states the launcher contract, since the driver has no correction of its own. 22500 states.
  - The launcher, its Python containment module, and its fixture are kept as is. The `soak-d2-native-containment-2026-09-11` manifest is digest-bound, and the package and the standalone spec are on the phase-two lists. Conflicts in the gate registry and the gate test resolved toward this branch. `test-soak-pr-formal-gate.sh` stays deleted. Counts updated to 40 soak controls and 42 scenarios.
- 2026-09-12, `dd1043c70` (B45 run-domain admission): driver change taken as is. `SOAK_CONTAINMENT=required` makes both admission checks verify a root-owned run-domain record against the driver's own control group and uid before their counters increase. A failed check writes the breach record, refuses work, and retains one failure. The default stays unmanaged, and the native launcher does not write the record yet.
  - `RunDomainAdmission` folded into `SoakDiskAdmission` as `VerifyPlacement` with `recordMatches`, a refusal in `Benchmark` and `CheckAdmission`, invariant `UnverifiedPlacementPreventsAdmission`, and one control `unchecked_placement`. The source's two controls, benchmark and iteration, map to this one. 26604 states.
  - The `soak-d2-run-domain-2026-09-12` manifest is digest-bound. The package and the standalone spec are on the phase-two lists. The second session's coordination log is a source record, which the plan does not carry. The host fixture and the emergency-suite lines that run it are kept as is. The second session's native-query fixture is kept too, and no gate runs it. Counts updated to 41 soak controls and 47 registered controls.
- 2026-09-12, `f75af9ed3` (B46 run-domain record identity): driver change taken as is. The record check now opens each path component from the filesystem root through directory descriptors with no symbolic link following. It requires a root-owned and unwritable chain and rejects a record beyond its size bound. The source review of B45 found the two defects.
  - `RunDomainRecordIdentity` folded into `SoakDiskAdmission` as `BindIdentity` with `recordTrusted`, the shared `PlacementVerified` predicate under B45's `VerifyPlacement`, invariant `UntrustedRecordPreventsAdmission`, and control `pathname` as on the source. The `unchecked_placement` control omits the new invariant, since a driver with no placement check violates both. 29520 states.
  - The second session's `NativeLaunchAdmission` model describes the launcher's release gate, which the source has not registered and its plan does not number yet. Nothing is folded for it. The guardian's `ManagedContainment` still states the launcher contract. The spec, the B46 spec, and the `soak-d2-record-identity-2026-09-12` package are on the phase-two lists. The manifest is digest-bound, and the launcher and its fixtures stay as is. Counts updated to 42 soak controls and 48 registered controls.
- 2026-09-12, `9a6dacd74` (B47 native launch admission, B48 breach record order): driver change taken as is for B48. The driver checks the guardian and the floor before the hygiene-pass attribution and writes its records before the floor-breach attribution. Each attribution runs in its own session, which the driver kills at the deadline. B47 is the second session's launcher change, now numbered and registered by the source. The launcher starts a trusted root gate and releases the driver only after it verified the placement and the gate identity.
  - `BreachRecordOrder` folded into `SoakDiskAdmission` as `RecordBeforeAttribution` with `attributionStarted`, action `Attribute` after a disk stop, invariant `AttributionRequiresRecord`, and control `attribute_first` as on the source. 30352 states.
  - `NativeLaunchAdmission` folded into `SoakDiskGuardian` as `VerifyBeforeRelease` with `queryAvailable`, a `launch` phase before `running`, action `Release`, invariant `UnavailableQueryPreventsRelease`, and control `start_first` as on the source. The source's second positive, the available query, is the same configuration with the query available, so it is not carried. 22518 states.
  - The second session's `NativeControlPath` model describes the launcher's control-directory check, which the source has not registered and its plan does not number. Nothing is folded for it. Both manifests are digest-bound, and the three specs and the two packages are on the phase-two lists. The launcher, the new fixtures, and the emergency-suite lines stay as is. Counts updated to 44 soak controls and 50 registered controls.

## D3 evidence: the disk-usage timeline (2026-09-10)

The breach snapshot names the consumer only at the end. The driver now records the growth curve as well. Every five minutes, and at each iteration start, it appends one row to `disk-usage-timeline.tsv` in the output directory. A row holds the epoch, a label, the free MiB, and the same per-root summary the breach tag carries. The guardian writes its rows in the background, so a slow `du` never delays the probe or the progress record. `SOAK_DISK_USAGE_INTERVAL_SECONDS` sets the interval, and 0 disables the timeline.

The output directory is already uploaded as the run artifact, so the file survives the VM. The host suite checks the header, the first iteration row, and the segment-start row with the fake `df` sample.

Next step: dispatch the soak workflow from this branch, with this branch as the target, and read which root or Docker object climbs. Run the workflow file from this branch, not from master, so the integration suite follows this branch's pin.

## Conditional no-overrun theorem (2026-09-10)

The user asked why the branch cannot prove that a disk overrun never occurs. The answer is that the consumer's write rate is outside the driver, and the stop does not confirm termination. The guardian model now states the theorem with those premises explicit.

`freeMiB` starts at the hard floor and falls at `WriteRateMax` per unit for the sample period, the probe, and the stop. `NoOverrun` holds under `FloorCoversReaction` and `BoundTermination`. The controls `rate_exceeds_floor` and `unconfirmed_stop` each drop one premise and violate it. The state count stays at 6342.

In production the reaction time is 10 seconds and the hard floor is 2048 MiB. The writers must then consume less than about 205 MiB per second over any 10-second window. The timeline rows give five-minute averages. A burst bound needs the guardian's own 5-second samples, which is the next evidence step.

## Consumer storage budget (2026-09-10)

The user asked whether the consumer side can carry theorems with bounds. It can, as event sources with per-event caps and per-unit rates. `SoakStorageBudget` sums the products into `RateBound`, the value the guardian takes as `WriteRateMax`, and `WithinBudget` holds with every cap enforced. Three controls drop the block, log, and history caps and violate the budget. Those are the caps the node does not enforce today, so the theorem doubles as a caps backlog.

The deploy cap is derived, not assumed. `deploy_storage/DeployStorageBound` is a consensus-side area with its own README and control. It cites only the interpreter's cost table, where storage is charged one phlo per encoded byte, and it never refers to the soak models. The soak budget names the cap as a constant and cites the area. That keeps the Casper proofs separable when the consensus component moves to its own repository.

Gate: six positive configurations and 33 controls. `deploy_storage` joined `REGISTERED_CONTROL_AREAS`.

## Decision: split after the source agent finishes (2026-09-10, revised 2026-09-11)

The maintainer decided that PR #406 does not merge as one unit. It is a staging pull request, marked as a draft, and it closes unmerged after the cut. After the source agent's last cycle lands here, the branch is cut into four pull requests from dev. The legacy modules, wrappers, digest inventory, generated evidence packages, and every Ruby file are not carried:

1. Deploy storage bound, `formal/tlaplus/deploy_storage`, a machine A area.
2. Soak driver disk protection: the driver, the host suite, the Docker harness, and the real-daemon checks. The driver evidence record and the release-process note go with it.
3. Soak formal models: the two consolidated models, the storage budget, the gate registry and test, the README, and the claim.
4. Docs: the consensus-neutral execution note, its links from the docs index and the formal-verification guide, and this split section.

### Verification split by machine and medium (2026-09-11)

The maintainer's architecture note, [F1r3fly: Parallel State Machines and Consensus-Neutral Execution](../artifacts/f1r3fly-consensus-neutral-sm.md), replaces the earlier casper, execution, node split. The node has two execution machines and four ordering media. Machine A is Rholang reduction over RSpace. Machine B is the RGB client-side contract machine. The media are CBC Casper, RGB seals, Casanova, and Cordial Miners.

Non-conflicting deploys commute, so a medium only orders and closes conflicts. Verification follows that cut:

| Check | Formal areas | cbc tags | Shared by |
| --- | --- | --- | --- |
| Machine A, execution | merge algebra, rspace guards, deploy storage, replay liveness, the shard half of runtime isolation | rholang `reduce.rs`, rspace `replay_rspace.rs`, and the runtime manager, interpreter util, and replay runtime under `casper/src/rust/util/rholang` and `casper/src/rust/rholang` | every medium that runs Rholang, including RGB where rho names are sealed |
| DAG substrate | carrier index, block admission, deploy occurrence, the block heap half of runtime isolation | block-storage DAG storage and carrier index | Casper, Casanova, and Cordial Miners, not RGB |
| Casper medium | fork choice, finalized floor, slashing, deploy recovery, deploy lifecycle, recovery leader, the Casper theory docs | snapshot, block creator, validation dispatcher, finality, the two mergers, deploy chain index, validate, genesis deploys | Casper only |
| RGB machine B and seal medium | none in this tree. Schema validity, single seal close, and consignment validation start in the RGB repository | none in this tree | RGB only |
| Infrastructure | soak disk, storage budget | driver evidence | every medium the soak exercises |

The merge algebra is the keystone. Its Rocq theorems prove pointwise commutativity, associativity, and idempotence of effect-map merge, deterministic channel netting, and conflict soundness. That is the commutation claim in machine form. Each medium repository cites it by pinned commit and does not carry a copy.

Each repository keeps its own gate registry, evidence ledger, and claim documents. Cross-citations become pinned references, in the same form as the system-integration pin. The committed Rocq build outputs leave the tree before any move.

The cut itself, its order, its split files, and the decisions it needs are in [the stacked pull request plan](../plans/soak-disk-hygiene-stacked-prs-2026-09-11.md).

### Follow-ups the split depends on

- **Interface crate.** The `MultiParentCasper` trait is defined inside the casper crate, so the node names the boundary through the medium it should be neutral to. A neutral interface crate comes first. Its first node-shell theorem is that the node uses only the trait.
- **Rocq coverage gap.** `scripts/ci/check-formal-invariants.sh --rocq` rebuilds slashing, fork choice, and rspace guards only. The merge algebra, finalized floor, and runtime isolation proofs ship as committed build outputs and are not rechecked. The keystone proof needs a CI rebuild before it can be cited by pin.
- **Execution glue in the medium crate.** The three Rholang runtime files under the casper crate are machine A code. They move with the interface work.
- **Runtime isolation splits.** `ShardRuntimeIsolation` is machine A and `BlockHeapLifecycle` is substrate. The area is cut in two at the move.
- **Soak scenarios per medium.** Today's merge-recovery soak exercises Casper. The driver and its disk models are shared, but each medium needs its own scenarios.

## Notes for the source agent

- New cycles fit the existing shape: add a constant and an invariant to one of the two modules, one `MC_*_pre_fix.cfg`, one line in `NEGATIVE_CONTROLS`, and one scenario in `SCENARIOS` plus its `--inside` branch. Nothing else needs a copy of the list.
- Evidence goes into the two `docs/cbc-evidence/*.md` records as a table row, not a new directory.
- The source branch's merges will conflict in `check-tla-invariants.sh` (registry), `test-soak-disk-admission.sh` (outer loop), and `ci.yml` (steps). Keep this branch's side for those three.
