# CbC Evidence: scripts/run-merge-recovery-soak.sh

- **Status:** pending (local RED/GREEN complete; hosted execution and maintainer review open)
- **Adapter:** embedded
- **Claim:** [CLAIM-SOAK-001](../claims/soak-disk-protection.md), proposed and unratified
- **Commit:** 1e2dc07f4 (corrections landed in ca85cfe3e, 59430d59b, ac94c1755, 3d2aa7904, e6fdd343b, 3498fa4f3, 7f0f46923, 6e0b50f26, 59b90568c, 8ab599e5c, 706b11b6e, 9c99de84e, 4e9dd432b, d64ae3bbf, 14ffb4d3a)
- **Verified:** locally, 2026-09-08 to 2026-09-09

Each cycle ran the real driver inside a disposable container through `scripts/bench/test-soak-disk-admission.sh` (no host mounts, no network, no Docker socket, UID 65534, 256 MiB, one CPU). `df`, `docker`, and the workload command were fixtures. In every cycle the production regression failed on the pre-fix source, TLC reported the named invariant with exit 12 on the pre-fix configuration, and both passed after the correction.

| Cycle | Behavior | Pre-fix source and result | Corrected result | Control (invariant) |
| --- | --- | --- | --- | --- |
| D1/B4 | Hygiene leaves 7000 MiB inside the band | `0f5d2b74` admitted 1 iteration, 0 failures | `ca85cfe3e`: 0 iterations, 1 failure, exit 1 | `floor_only` (`AdmissionRequiresBand`) |
| B5 | `df` returns nothing at the boundary or after hygiene | `80914eeda` admitted 1, 0 failures, exit 0 | 0 iterations, 1 failure, exit 1 | `missing_sample` (`AdmissionRequiresSample`) |
| B6 | `df` prints `16384junk` after a valid startup probe | `59430d59b` admitted 1, 0 failures, exit 0 | one-line digit check in `disk_free_mb`; 0 iterations, 1 failure | `numeric_prefix` (`AdmissionRequiresValidSample`) |
| B7 | `df` fails while an iteration runs | `ac94c1755` completed the iteration | iteration stopped, breach recorded, exit 1 | `unavailable_sample` (`InvalidSampleRequiresInterrupt`) |
| B8 | Breach record precedes `docker kill` | `ac94c1755` started the stop first | record present when the stop fixture ran | `stop_first` (`StopRequiresRecord`) |
| B9 | Watcher detects guardian death | `ac94c1755` completed the iteration | iteration stopped, failure recorded | `unwatched` (`DeadGuardianRequiresInterrupt`) |
| B10 | `df` prints a field, then stalls | `ac94c1755` accepted the partial field | partial output rejected after the 2 s deadline | `unbounded_probe` (`ProbeWithinDeadline`) |
| B11 | `du` stalls with 32 session roots | `ac94c1755` waited per root | attribution ends within the aggregate deadline | `per_root_deadline` (`AttributionWithinBudget`) |
| B12 | Retained marker on restart | `ac94c1755` cleared the marker and admitted work | no new work; failures 0 to 1 and 2 to 2 | `cleared_breach` (`RetainedBreachStopsRestart`) |
| B13 | `pkill` and `docker kill` stall and ignore TERM | `3d2aa7904` waited on the stop clients | stop bounded by `SOAK_DISK_STOP_SECONDS`; failure published, exit 1 | `unbounded_stop` (`StopWithinBudget`) |
| B14 | The guardian dies during the boundary probe | `e6fdd343b` admitted 1 iteration | liveness check after the probe; 0 iterations, 1 failure, exit 1 | `unchecked_guardian` (`AdmissionRequiresGuardian`) |
| B15 | Retained marker, no state file, benchmarks on | `3498fa4f3` launched the opening benchmark before recovery read the marker (1 bench segment, 1 bench failure) | the opening condition checks the marker; 0 iterations, 0 bench segments, 1 failure, exit 1 | `retained_breach` (`RetainedBreachPreventsBenchmark`) |
| B16 | 7000 MiB free at the opening benchmark, no marker | `7f0f46923` launched the benchmark below floor plus band | the benchmark condition probes the disk first; 0 iterations, 0 bench segments, 1 failure, exit 1 | `benchmark_band` (`BenchmarkRequiresBand`) |
| B17 | The disk falls to 1024 MiB during the opening benchmark | `6e0b50f26` started the guardian after the benchmark returned: no record, no stop | the guardian starts first; it records the breach and asks for a stop before the benchmark returns; 0 iterations, 1 failure, exit 1 | `late_guardian` (`BenchmarkBreachObserved`) |
| B17 coverage | Four allowed and refused benchmark admission paths (8192 MiB, 16384 MiB, missing sample, protection off) | no defect; coverage only | admitted, admitted, refused with 1 failure, admitted | none (harness scenarios `benchmark-equal`, `benchmark-sufficient`, `benchmark-missing`, `benchmark-disabled`) |
| B18 | The guardian dies, or records a breach, while the benchmark's Docker client ignores TERM | `59b90568c` waited on the stalled client; no stop, no failure summary | the driver polls the guardian during the benchmark, stops the writers, terminates the benchmark under a one-second kill grace, and publishes the failure; exit 1 | `unwatched_death`, `unwatched_breach` (`BenchmarkCancellationObserved`) |
| B19 | The guardian dies during the benchmark's disk probe, at the opening or an interleaved boundary | the B18 driver counted and launched the benchmark | liveness and marker checked after the sample, before the counter; 0 benchmark admissions, 1 failure; the interleaved case keeps its one iteration | `unchecked_guardian` (`AdmissionRequiresGuardian`, now covering benchmark admission) |
| B20, B21 | The guardian is SIGSTOPped, alive but silent, during the benchmark (B20) or an iteration (B21) | `8ab599e5c` saw a live process and waited on the stalled client | the guardian writes a progress timestamp after each sample and the driver treats one older than `SOAK_GUARDIAN_MAX_SILENCE_SECONDS` (8 to 30, default 10) as a breach; the work is cancelled and the failure published | `alive_only` (`StaleGuardianRequiresInterrupt`, guardian model) |
| B22 | Driver and guardian paused across a valid admission probe, so the progress record expires | the B21 driver admitted the iteration and the opening benchmark on stale progress | freshness checked after the probe, before either work counter increments; 0 admissions, 1 failure | `unchecked_progress` (`StaleProgressPreventsAdmission`) |
| B23 | `docker builder prune` ignores TERM inside the hygiene band | `5549561e1` waited on the cleanup client; no refusal, no summary | hygiene commands run in a timed child shell under `SOAK_DISK_HYGIENE_SECONDS` (1 to 30, default 10) with a one-second kill grace; a killed group is a protection breach; 0 iterations, 1 failure | `unbounded_hygiene` (`HygieneWithinBudget`) |
| B24 | `SOAK_DISK_FREE_FLOOR_MB`, `SOAK_DISK_HYGIENE_BAND_MB`, or their sum above the 64-bit maximum | `9c99de84e` accepted the text and admitted work on an overflowed threshold | settings are parsed as decimals and range-checked before any work; the configuration is rejected with exit 2 and no summary | `unchecked_range` (`AdmissionRequiresValidDiskSettings`); the source's model and scenarios arrived unregistered, registered here; the source registered its own control one commit later |
| B25 | An unowned two-hour-old temporary session whose writer still holds a file open | `4e9dd432b` swept `/tmp/test-*` older than 60 minutes during hygiene and deleted it under the live writer | hygiene no longer deletes temporary sessions by age; it reports that ownership is unconfirmed and keeps them, and the admission checks refuse work when space stays below the threshold | `age_only` (`UnownedSessionPreserved`) |
| B26 | One of the five Docker cleanup commands fails during hygiene, then a sufficient sample arrives | `d64ae3bbf` swallowed every cleanup error and admitted work | command failures are kept across the group with pipefail, the failed stage is reported, and failed hygiene takes the B23 refusal path; 0 iterations, 1 failure. Two reclamation cases (8000 MiB refused, 16384 MiB admitted) are coverage only | `ignore_errors` (`CleanupFailurePreventsAdmission`) |
| B27 | Hygiene runs on a host with a stopped container, an unused network, and an untagged image that the soak does not own | `1e2dc07f4` removed exited `rnode` containers and pruned every unused network, dangling image, and build cache | hygiene inspects those resources (`docker inspect`, `network ls`, `image ls`, `system df`) and reports them retained because ownership is unconfirmed. An inspection failure still fails hygiene (B26). The source's real-daemon check is `scripts/bench/test-soak-real-docker-ownership.sh`, not run on this host | `global_prune` (`UnownedDockerResourcesPreserved`) |
| B28 | The driver exits (signal, early exit) while an iteration or benchmark runs | `3b7904ebf` left the node writers running on the host after the driver was gone | the EXIT trap calls `stop_node_writers` when a benchmark or iteration pid is set. The source's real-daemon check is `scripts/bench/test-soak-real-docker-shutdown.sh`, not run on this host | `client_only` (`ExitStopsWriters`) |
| B29 | A segment dies with an iteration in flight and the next segment resumes | `3b7904ebf` resumed as if the iteration had ended and admitted work | the state file carries `INFLIGHT_ITERATION`. A resumed segment that finds it set counts one failure, writes `early-exit.txt` with `interrupted_iteration`, and refuses work. The source's real-daemon check is `scripts/bench/test-soak-real-crash-recovery.sh`, not run on this host | `unrecorded` (`CrashRequiresRefusal`) |
| B30 | Docker rejects the exit trap's stop command | `f4111f6b5` ignored the rejection and exited as if the writers had stopped | the trap writes `writer-stop-failure.txt` and an early-exit reason, counts a failure (or marks the in-flight iteration), and persists the state. The source's real-daemon check is `scripts/bench/test-soak-real-stop-failure.sh`, not run on this host | `ignored` (`FailedStopRetained`) |
| B31 | An unrelated `rnode` container runs on the host during a stop | `f4111f6b5` killed every container matching the `rnode.` name filter | the driver mints an owner id per run. A `docker` wrapper on the workload's PATH labels every container it creates, and stop commands select by that label. The source's real-daemon check is `scripts/bench/test-soak-real-stop-ownership.sh`, not run on this host | `name_only` (`UnownedWritersPreserved`) |

Formal results after the 2026-09-09 consolidation into two modules (the per-cycle originals reported 54, 22, and 17 distinct states for D1, B5, and B6):

| Configuration | Result | Distinct states |
| --- | --- | ---: |
| `MC_SoakDiskAdmission` | clean, `Completes` holds | 320 |
| `MC_SoakDiskGuardian` | clean | 3144 |
| twenty-two `*_pre_fix` controls | exit 12 with the expected invariant | 20 to 436 |

Regression suites green on the corrected driver: `test-soak-disk-admission.sh` (42 scenarios), `test-run-merge-recovery-soak.sh` (3 scenarios), `check-tla-invariants.sh --soak-pr` (4 positives, 24 controls, about 25 s).

Fixture corrections during the cycles: one B9 driver-suite run failed on a readiness race at the resource CSV assertion, and the fixture now waits for every required telemetry file (30 repetitions passed). The first B5 model attempt exited 75 on a mixed string and numeric sample encoding before any behavioral result and was replaced by uniform sample records.

## Historical manifests

The per-cycle manifests were produced on the source branch and are retained outside Git by the agent that ran the cycles. Their digests bind that raw store to this record. A regenerated manifest with a different digest is a new record, not renewed verification.

| Manifest | SHA-256 |
| --- | --- |
| `soak-d2-probe-2026-09-08/manifest.jsonc` (B5) | `3f1a1697d34f4696f877bcb5138942ecba5a9cd2754f8ad1e43d70a4e05870c7` |
| `soak-d2-sample-2026-09-09/manifest.jsonc` (B6) | `167d3a6bb8025d2d80615b4c178d0961fd433f83e5840d3aa038336c4bbeb137` |
| `soak-d2-emergency-2026-09-09/manifest.jsonc` (B7 to B12) | `36c75832006ecdf4d1b7bd4f14cfdf02c73ae8663b562afbbbcdd8cf24ef7773` |
| `soak-d2-stop-2026-09-09/manifest.jsonc` (B13) | `1f0ec6ca4b469fbfff33ae939221da8a2845e1bf5744f1c8d54f783aa9b55006` |
| `soak-d2-boundary-2026-09-09/manifest.jsonc` (B14) | `9ea2de48dcc9e353454a8451453fdb69772d827fdb9a689a26491b130ffae3ae` |
| `soak-d2-benchmark-2026-09-09/manifest.jsonc` (B15) | `871daa99255006420b61cf4c8d70f0164312f009fa138a1e220e5c0a1b2ffb4f` |
| `soak-d2-benchmark-band-2026-09-09/manifest.jsonc` (B16) | `bff43e225d2de7d2a5b92559b093a36436be27ef80ba305293be99af7b4e0280` |
| `soak-d2-benchmark-monitor-2026-09-09/manifest.jsonc` (B17) | `79b4505f9dbe2a7f2341430356ac63b75c1c5956244c83cfa66b7dfbbaca7323` |
| `soak-d2-hygiene-2026-09-09/manifest.jsonc` (B23) | `17bc0ec164978c3b8f6ba4b59394bd666b650d3f29234ca5110aab7bb8b58bff` |
| `soak-d2-settings-2026-09-10/manifest.jsonc` (B24) | `0f8ed4935155b3ca96d133e656a19da4edd2f340c192c77395a03cedc4d4db8b` |
| `soak-d2-cleanup-session-2026-09-10/manifest.jsonc` (B25) | `0c7926aa4f0872c592321457ac2bbd797689198f4139e1d333db1475bb1171a4` |
| `soak-d2-cleanup-outcome-2026-09-10/manifest.jsonc` (B26) | `cce080274ac1cc657e7465e60c9a252f9aae023173d473d1b89043fb17610fb0` |
| `soak-d2-real-system-2026-09-10/manifest.jsonc` (B27 to B29, real daemon) | `5918cd78b803d9e6566afdb46ba10cd21360b2341326d54a4766bfd2bde2df88` |
| `soak-d2-guardian-progress-2026-09-09/manifest.jsonc` (B20 to B22) | `310280a8df04bcba0d34cc6d0bd3d8b6b8899bb87e85f5ff945be7f1ffb8618b` |
| `soak-d2-benchmark-supervision-2026-09-09/manifest.jsonc` (B18, B19) | `33be0710c7d724988fa1e07797a6537f3ba5ae9f962eacce784cabe0a0d348fe` |
| `soak-d2-benchmark-cases-2026-09-09/manifest.jsonc` (B17 coverage) | `c7671619622336d2ca493ea5b7597414b28f18a43bb5e4b3a659c50c9565661b` |

## Conditional no-overrun theorem

`SoakDiskGuardian` now carries `NoOverrun`: free space stays positive from the last healthy sample through a completed stop. The theorem holds under `FloorCoversReaction` (the hard floor exceeds the writers' consumption over the 10-second reaction time) and `BoundTermination` (a completed stop ends consumption). Two controls, `rate_exceeds_floor` and `unconfirmed_stop`, each drop one premise and violate the invariant. With the default 4096 MiB floor the rate premise bounds consumption at about 205 MiB per second over any 10-second window. The timeline rows measure that rate. The termination premise is the open D2 item.

## Consumer storage budget

`SoakStorageBudget` derives the guardian's `WriteRateMax` from per-consumer caps and rates. `WithinBudget` holds with every cap enforced. Three controls drop the block, log, and history caps and violate it, which names the caps the node lacks. The deploy cap is the one bound with a proof: `deploy_storage/DeployStorageBound` shows a deploy retains at most its phlo limit divided by the storage rate. That area stays isolated from the soak models.

## Limits

- The model-to-code maps are reviewed abstractions, not refinement proofs of Bash.
- Fixtures replace `df`, `docker`, and the workload. They do not cover every malformed field, exit status, or Docker failure.
- Local records prove neither durability, upload, nor confirmed writer termination. Fairness is not a time bound.
- The no-overrun theorem is conditional. Its rate premise is unmeasured until a timeline run, and its termination premise is unmet until D2 confirms termination.
- The storage budget assumes block, log, and history caps the node does not enforce. Until it does, the rate bound is an assumption checked by the timeline, not a derivation.
- D2 remains open for confirmed termination and a composed emergency deadline. Hygiene now reclaims nothing from Docker, so D3 must identify the growing consumer and an owned reclamation path.

```json
{
  "artifact": {
    "path": "scripts/run-merge-recovery-soak.sh",
    "commit": "3498fa4f3",
    "id": "scripts-run-merge-recovery-soak-sh"
  },
  "claim": "CLAIM-SOAK-001: disk admission refuses missing, malformed, and in-band samples; the guardian records before it stops, survives probe faults and its own death, bounds attribution, and blocks restart after a retained breach.",
  "adapter": "embedded",
  "status": "pending",
  "evidence": {
    "kind": "container-regressions+bounded-model-check",
    "ref": "scripts/bench/test-soak-disk-admission.sh (42 scenarios); formal/tlaplus/soak_disk/MC_SoakDiskAdmission.cfg, MC_SoakDiskGuardian.cfg",
    "counterexample": "formal/tlaplus/soak_disk/MC_SoakDiskAdmission_*_pre_fix.cfg, MC_SoakDiskGuardian_*_pre_fix.cfg",
    "detail": "Eleven local RED/GREEN cycles on fix/soak-disk-hygiene-stop. No hosted execution of the container regressions, no maintainer model review, no mandatory-scope ratification, and no acceptance soak."
  },
  "waiver": null,
  "verified_at": null
}
```
