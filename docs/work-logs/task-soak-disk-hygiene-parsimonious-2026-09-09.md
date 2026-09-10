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
git rm formal/tlaplus/soak_disk/{SoakDisk,DiskProbeAdmission,DiskSampleValidation,ActiveDiskProbe,DiskEmergencyRecord,GuardianSupervision,DiskProbeDeadline,DiskDiagnosticDeadline,DiskBreachRestart,DiskStopDeadline,GuardianAdmission,BenchmarkBreachAdmission,BenchmarkDiskAdmission,BenchmarkDiskMonitor,BenchmarkCancellation,GuardianProgress,GuardianProgressAdmission,DiskSettingsAdmission,CleanupOwnership,CleanupOutcome,DockerCleanupOwnership,DockerExitStop,IterationCrashRecovery,DockerStopFailure,DockerStopOwnership}.tla
git rm formal/tlaplus/soak_disk/MC_{SoakDisk,DiskProbeAdmission,DiskSampleValidation,ActiveDiskProbe,DiskEmergencyRecord,GuardianSupervision,DiskProbeDeadline,DiskDiagnosticDeadline,DiskBreachRestart,DiskStopDeadline,GuardianAdmission,BenchmarkBreachAdmission,BenchmarkDiskAdmission,BenchmarkDiskMonitor,BenchmarkCancellation,GuardianProgress,GuardianProgressAdmission,DiskHygieneDeadline,DiskSettingsAdmission,CleanupOwnership,CleanupOutcome,DockerCleanupOwnership,DockerExitStop,IterationCrashRecovery,DockerStopFailure,DockerStopOwnership}*.{tla,cfg}
git rm formal/tlaplus/soak_disk/{DiskProbeAdmission,DiskSampleValidation,EmergencyResponse,DiskStopDeadline,GuardianAdmission,BenchmarkBreachAdmission,BenchmarkDiskAdmission,BenchmarkDiskMonitor,BenchmarkCancellation,GuardianProgress,DiskHygieneDeadline,DiskSettingsAdmission,CleanupOwnership,CleanupOutcome,DockerCleanupOwnership,DockerExitStop,IterationCrashRecovery}.md
# harness wrappers and aggregators (the harness runs every scenario itself)
git rm scripts/bench/test-soak-disk-{active-probe,diagnostic-deadline,emergency,probe-timeout,probe,record,restart,sample,stop-deadline}.sh scripts/bench/test-soak-guardian-death.sh scripts/bench/test-soak-guardian-admission.sh scripts/bench/test-soak-benchmark-restart.sh scripts/bench/test-soak-benchmark-disk-admission.sh scripts/bench/test-soak-benchmark-disk-monitor.sh scripts/bench/test-soak-benchmark-admission-cases.sh scripts/bench/test-soak-benchmark-cancellation.sh scripts/bench/test-soak-benchmark-guardian-admission.sh scripts/bench/test-soak-guardian-progress.sh scripts/bench/test-soak-disk-hygiene-deadline.sh scripts/bench/test-soak-disk-settings.sh scripts/bench/test-soak-cleanup-ownership.sh scripts/bench/test-soak-cleanup-outcomes.sh
# digest inventory and its validators (the ci.yml steps are already gone)
git rm docs/claims/soak-claim-inventory.jsonc scripts/ci/test-soak-claim-inventory.sh scripts/ci/test-soak-claim-inventory-jsonc.sh
# generated evidence (condensed records replace them)
git rm -r docs/cbc-evidence/soak-g0-2026-09-08 docs/cbc-evidence/soak-g0-b2-2026-09-08 docs/cbc-evidence/soak-g0-b3-2026-09-08 \
  docs/cbc-evidence/soak-d2-probe-2026-09-08 docs/cbc-evidence/soak-d2-sample-2026-09-09 docs/cbc-evidence/soak-d2-emergency-2026-09-09 docs/cbc-evidence/soak-d2-stop-2026-09-09 docs/cbc-evidence/soak-d2-boundary-2026-09-09 docs/cbc-evidence/soak-d2-benchmark-2026-09-09 docs/cbc-evidence/soak-d2-benchmark-band-2026-09-09 docs/cbc-evidence/soak-d2-benchmark-monitor-2026-09-09 docs/cbc-evidence/soak-d2-benchmark-cases-2026-09-09 docs/cbc-evidence/soak-d2-benchmark-supervision-2026-09-09 docs/cbc-evidence/soak-d2-guardian-progress-2026-09-09 docs/cbc-evidence/soak-d2-hygiene-2026-09-09 docs/cbc-evidence/soak-d2-settings-2026-09-10 docs/cbc-evidence/soak-d2-cleanup-session-2026-09-10 docs/cbc-evidence/soak-d2-cleanup-outcome-2026-09-10 \
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

- 2026-09-10, `f3366cfab` (B30 a rejected exit-trap stop is retained; B31 stop commands select owner-labeled writers): driver change taken as is. The driver now mints an owner id per run and puts a `docker` wrapper on the workload's PATH that labels every container it creates. The driver conflict was the timeline's segment-start row against the wrapper block, and both sides are kept. `DockerStopFailure` folded into `SoakDiskGuardian` as `RetainStopFailure` with `exitRejected` and `exitFailureRetained` drawn in `ExitTrap`, invariant `FailedStopRetained`, control `ignored` as on the source. `DockerStopOwnership` folded as `SelectOwned` with `unownedStopped` set in `ExitTrap` and `BeginStop`, invariant `UnownedWritersPreserved`, control `name_only` as on the source. 6366 states.
  - The source's B27 to B29 real-daemon evidence package (`52c91cfe4`) is digest-bound. Its three correspondence documents are on the phase-two list.
  - The README conflict was the source's real-system section against the theorem table, resolved toward this branch. `docs/ToDos.md` keeps this branch's bullets and the source's newer status line.
  - Counts updated to 29 soak controls and 42 scenarios.

- 2026-09-10, `463992ed1` (source registers its B30 and B31 controls and shims owner labels in the harness fixture): no new behavior. The harness fixture's `docker` now answers the owner-label `ps`, `inspect`, `kill`, and `compose` calls, and that change auto-merged. Conflicts in the gate registry and the gate test resolved toward this branch. `test-soak-pr-formal-gate.sh` stays deleted. Counts unchanged.

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

## Decision: split after the source agent finishes (2026-09-10)

The maintainer decided that PR #406 does not merge as one unit. After the source agent's last cycle lands here, the branch is cut into three pull requests from dev. The legacy modules, wrappers, digest inventory, and generated evidence are not carried:

1. Deploy storage bound, `formal/tlaplus/deploy_storage`, the execution-side area.
2. Soak driver disk protection: the driver, the host suite, the Docker harness, and the two real-daemon checks. The driver evidence record and the release-process note go with it.
3. Soak formal models: the two consolidated models, the storage budget, the gate registry and test, the README, and the claim.

The formal tree then splits by component. Casper areas move with the consensus component. Those are slashing, finalized floor, fork choice, deploy recovery, deploy lifecycle, carrier index, deploy occurrence, recovery leader, and the Casper theory docs. Execution areas need a placement decision: merge algebra, runtime isolation, rspace guards, deploy storage, and replay liveness. Node areas stay: soak disk, block admission, and the gate scripts. Each repository keeps its own gate registry, and cross-citations become pinned references.

## Notes for the source agent

- New cycles fit the existing shape: add a constant and an invariant to one of the two modules, one `MC_*_pre_fix.cfg`, one line in `NEGATIVE_CONTROLS`, and one scenario in `SCENARIOS` plus its `--inside` branch. Nothing else needs a copy of the list.
- Evidence goes into the two `docs/cbc-evidence/*.md` records as a table row, not a new directory.
- The source branch's merges will conflict in `check-tla-invariants.sh` (registry), `test-soak-disk-admission.sh` (outer loop), and `ci.yml` (steps). Keep this branch's side for those three.
