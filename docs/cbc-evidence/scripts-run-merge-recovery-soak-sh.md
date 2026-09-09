# CbC Evidence: scripts/run-merge-recovery-soak.sh

- **Status:** pending (local RED/GREEN complete; hosted execution and maintainer review open)
- **Adapter:** embedded
- **Claim:** [CLAIM-SOAK-001](../claims/soak-disk-protection.md), proposed and unratified
- **Commit:** 59b90568c (corrections landed in ca85cfe3e, 59430d59b, ac94c1755, 3d2aa7904, e6fdd343b, 3498fa4f3, 7f0f46923, 6e0b50f26, 59b90568c)
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

Formal results after the 2026-09-09 consolidation into two modules (the per-cycle originals reported 54, 22, and 17 distinct states for D1, B5, and B6):

| Configuration | Result | Distinct states |
| --- | --- | ---: |
| `MC_SoakDiskAdmission` | clean, `Completes` holds | 320 |
| `MC_SoakDiskGuardian` | clean | 3144 |
| fourteen `*_pre_fix` controls | exit 12 with the expected invariant | 20 to 436 |

Regression suites green on the corrected driver: `test-soak-disk-admission.sh` (20 scenarios), `test-run-merge-recovery-soak.sh` (3 scenarios), `check-tla-invariants.sh --soak-pr` (4 positives, 16 controls, about 25 s).

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
| `soak-d2-benchmark-cases-2026-09-09/manifest.jsonc` (B17 coverage) | `c7671619622336d2ca493ea5b7597414b28f18a43bb5e4b3a659c50c9565661b` |

## Limits

- The model-to-code maps are reviewed abstractions, not refinement proofs of Bash.
- Fixtures replace `df`, `docker`, and the workload. They do not cover every malformed field, exit status, or Docker failure.
- Local records prove neither durability, upload, nor confirmed writer termination. Fairness is not a time bound.
- D2 remains open for cleanup command bounds, cleanup ownership, confirmed termination, and a composed emergency deadline. D3 must identify the growing consumer.

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
    "ref": "scripts/bench/test-soak-disk-admission.sh (20 scenarios); formal/tlaplus/soak_disk/MC_SoakDiskAdmission.cfg, MC_SoakDiskGuardian.cfg",
    "counterexample": "formal/tlaplus/soak_disk/MC_SoakDiskAdmission_*_pre_fix.cfg, MC_SoakDiskGuardian_*_pre_fix.cfg",
    "detail": "Eleven local RED/GREEN cycles on fix/soak-disk-hygiene-stop. No hosted execution of the container regressions, no maintainer model review, no mandatory-scope ratification, and no acceptance soak."
  },
  "waiver": null,
  "verified_at": null
}
```
