# Claim: Soak Disk Protection

```yaml
claim_id: CLAIM-SOAK-001
status: proposed-unratified
artifacts:
  - scripts/run-merge-recovery-soak.sh
specifications:
  - formal/tlaplus/soak_disk/SoakDiskAdmission.tla
  - formal/tlaplus/soak_disk/SoakDiskGuardian.tla
tests:
  - scripts/bench/test-soak-disk-admission.sh
  - scripts/bench/test-soak-disk-admission-timing.sh
  - scripts/bench/test-run-merge-recovery-soak.sh
references:
  - docs/plans/soak-recurrence-prevention-2026-09-08.md
  - docs/tdd-plans/soak-gates-2026-09-08.md
  - docs/cbc-evidence/scripts-run-merge-recovery-soak-sh.md
```

## Claim

With disk protection enabled, the soak driver never starts an iteration from a free-space sample that is missing, malformed, or below floor plus band. It never starts one after its guardian process has died. During an iteration, the guardian records a breach before it stops the writers. A dead guardian or an unavailable sample stops the iteration. Every probe and attribution command runs under a deadline. A retained breach marker blocks the next segment.

With a node log budget enabled, the guardian also samples the container json-file log and the node log directory of each owned node container. It records a breach and stops the writers after three consecutive samples over a budget, or one sample at two times a budget. A log probe that cannot be read refuses admission before work. During work, one failed probe is a warning, and two consecutive failed probes are a breach.

Every command that the driver runs under a time budget returns within that budget plus the kill grace period, also under CPU saturation. When the bounded command ends first, the driver stops the watchdog of that command and does not wait for the watchdog to expire. If the watchdog has not yet started its own process group, the driver stops the watchdog process and then tries the group kill again. After an iteration completes, the driver therefore reaches its admission checks and its signal check within a bounded time.

At segment start, after each iteration and benchmark segment, and at the end of the main loop, the driver attempts a bounded disk usage record. A path that ends the driver with `exit` before the end of the loop writes no final row. Each row has the time, the boundary, the component, and the size in MiB. A failed measurement, including a command that exits nonzero after partial output, is recorded as unknown (`?`), never as zero. A record that fails or exceeds its budget cannot admit work, change the run result, or erase a protection breach. The benchmark admission refusal also attempts a bounded attribution snapshot.

The rows can overlap, so a reader must not add them as separate storage. For example, the `.subprocess-data` root is inside the runner `_work` root. The Docker totals also count space that a directory root can include.

## Implementation surface

| Behavior | Driver symbols |
| --- | --- |
| Sample validation | `disk_free_mb` |
| Admission | The iteration-boundary hygiene block, `reclaim_disk_space`, `disk_usage_snapshot` |
| Emergency | The guardian loop, `disk_diagnostics_bounded`, `disk_guardian_diagnostics`, `guardian_stamp_health_tag` |
| Supervision | The iteration watcher loop over `HOST_GUARDIAN_PID` and `HOST_GUARDIAN_BREACH` |
| Restart | The `HOST_GUARDIAN_BREACH` recovery before the first iteration |
| Bounded commands | `session_bounded` |
| Log budgets | `log_budget_sample`, `log_budget_probe`, `log_file_bytes`, `log_budget_refusal`, `guardian_log_breached` |
| Disk usage record | `disk_usage_record_rows`, `record_disk_usage` (budget `SOAK_DISK_USAGE_RECORD_SECONDS`), and the `disk_usage_snapshot` call on the benchmark admission refusal |

## Checks

| Check | Command | Status |
| --- | --- | --- |
| Bounded models, six registered standalone models, and sixty-one controls | `scripts/ci/check-tla-invariants.sh --soak-pr` | Green locally and on the PR tier |
| Consumer storage budget | `MC_SoakStorageBudget` invariant `WithinBudget`, with `deploy_storage/MC_DeployStorageBound` for the deploy cap | Proven in the model. Block and history caps are assumptions until the node enforces them. The log caps are enforced: see the next row |
| Node log caps (TASK-020-4) | The guardian checks `SOAK_CONTAINER_LOG_BUDGET_MB` (default 400) and `SOAK_NODE_LOG_BUDGET_MB` (default 2560) against the EPIC-020 source caps. The fourteen `log-*` scenarios of `scripts/bench/test-soak-disk-admission.sh` are the evidence. They cover the budgets, the refusal, the sudo fallback, disabled budgets, the range check, the descriptor limit, stopped containers, unreadable rotated logs, one transient probe failure, and a failed node log exec with a partial size | Green locally. The breach and refusal scenarios fail against the driver without the log guardian. `log-probe-transient-active` fails against the driver at 761cc0d59, which breached on one failed probe. `log-probe-exec-nonzero-active` fails against the driver at 291d4843c, which accepted the partial size of a failed exec |
| Conditional no-overrun theorem | `MC_SoakDiskGuardian` invariant `NoOverrun` under `FloorCoversReaction` and `BoundTermination` | Proven in the model. The rate premise awaits the timeline measurement, and the termination premise awaits D2 |
| Container regressions, 57 scenarios | `scripts/bench/test-soak-disk-admission.sh` | Green locally. CI runs the same command |
| Disk usage record | `benchmark-equal` and `benchmark-sufficient` require the segment, benchmark, and iteration rows. `benchmark-band` requires the refusal snapshot. `disk-record-degraded` checks the Docker size conversion and the unknown marker for a failed `du`. It also checks the budget bound for a stalled `docker system df`, the `soak-end` row, and an unchanged run result. | Green locally |
| Bounded command liveness under CPU saturation (TASK-023-8) | `scripts/bench/test-soak-disk-admission-timing.sh`, which includes the opt-in `SOAK_DISK_TEST_STRESS_ROUNDS` stage | Green locally on 2026-10-08. Before the fix, 25 of 60 saturated runs stalled in `session_bounded` until the driver timeout. After the fix, 60 of 60 passed, and the late-watchdog check returns in 1 ms instead of 9.5 s. CI run 37827751697 passed the late-watchdog check in the Lint job. The saturation stage runs only locally |
| Host driver regression, band scenario | `scripts/bench/test-run-merge-recovery-soak.sh` | Green locally and in CI |

## Pending obligations

The claim is not discharged. Gates from the prevention plan:

| Gate | Scope | Status |
| --- | --- | --- |
| G0 | Evidence binding and required-check enforcement | Pending. `TLA+ invariant check` is not a required check on `dev`. |
| D1 | Band admission | Local RED/GREEN complete. Maintainer review pending. |
| D2 | Emergency response bounds | Partial. Stop commands are bounded (B13) and guardian death blocks admission (B14). Cleanup command bounds, confirmed termination, durable publication, and a composed deadline remain open. |
| O1 | Observability before the diagnostic soak | Partial. The [work log](https://github.com/F1R3FLY-io/f1r3node-rust/blob/2388a8eedf33d07018f0630bced51a6e054ba439/docs/work-logs/task-soak-disk-hygiene-parsimonious-2026-09-09.md#gate-o1-observability-verification-2026-09-16) records the result of 2026-09-16. The raw CSV writer, the disk records, the artifact retrieval, and the checkpoint deadlines are verified. The carrier hit path and the ancestor counters were not exercised, and no free-inode record exists. |
| D3 | Identify and remove the disk-growth cause | Open. The source's D3 diagnostic methodology specifies the run that names the growing writer. The cause is not yet identified. The disk usage record supplies the per-component timeline for that run. |
| F1, F2, F3 | Finalization work bound and repair | Pending and separate from this claim. |
| A1 | 60-hour acceptance soak on the exact candidate | Pending. |

A green bounded model or container regression does not establish a disk reserve, an elapsed-time bound, or full-duration safety.
