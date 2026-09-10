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
  - scripts/bench/test-run-merge-recovery-soak.sh
references:
  - docs/plans/soak-recurrence-prevention-2026-09-08.md
  - docs/tdd-plans/soak-gates-2026-09-08.md
  - docs/cbc-evidence/scripts-run-merge-recovery-soak-sh.md
```

## Claim

With disk protection enabled, the soak driver never starts an iteration from a free-space sample that is missing, malformed, or below floor plus band, and never starts one after its guardian process has died. During an iteration, the guardian records a breach before it stops the writers. A dead guardian or an unavailable sample stops the iteration. Every probe and attribution command runs under a deadline. A retained breach marker blocks the next segment.

## Implementation surface

| Behavior | Driver symbols |
| --- | --- |
| Sample validation | `disk_free_mb` |
| Admission | The iteration-boundary hygiene block, `reclaim_disk_space`, `disk_usage_snapshot` |
| Emergency | The guardian loop, `disk_diagnostics_bounded`, `disk_guardian_diagnostics`, `guardian_stamp_health_tag` |
| Supervision | The iteration watcher loop over `HOST_GUARDIAN_PID` and `HOST_GUARDIAN_BREACH` |
| Restart | The `HOST_GUARDIAN_BREACH` recovery before the first iteration |

## Checks

| Check | Command | Status |
| --- | --- | --- |
| Bounded models and twenty-seven controls | `scripts/ci/check-tla-invariants.sh --soak-pr` | Green locally and on the PR tier |
| Conditional no-overrun theorem | `MC_SoakDiskGuardian` invariant `NoOverrun` under `FloorCoversReaction` and `BoundTermination` | Proven in the model. The rate premise awaits the timeline measurement, and the termination premise awaits D2 |
| Container regressions, 42 scenarios | `scripts/bench/test-soak-disk-admission.sh` | Green locally and in CI |
| Host driver regression, band scenario | `scripts/bench/test-run-merge-recovery-soak.sh` | Green locally and in CI |

## Pending obligations

The claim is not discharged. Gates from the prevention plan:

| Gate | Scope | Status |
| --- | --- | --- |
| G0 | Evidence binding and required-check enforcement | Pending. `TLA+ invariant check` is not a required check on `dev`. |
| D1 | Band admission | Local RED/GREEN complete. Maintainer review pending. |
| D2 | Emergency response bounds | Partial. Stop commands are bounded (B13) and guardian death blocks admission (B14). Cleanup command bounds, confirmed termination, durable publication, and a composed deadline remain open. |
| O1 | Observability before the diagnostic soak | Pending. |
| D3 | Identify and remove the disk-growth cause | Evidence collection in place: the driver appends one attribution row per five minutes and per iteration to `disk-usage-timeline.tsv` in the run artifact. The cause is not yet identified. |
| F1, F2, F3 | Finalization work bound and repair | Pending and separate from this claim. |
| A1 | 60-hour acceptance soak on the exact candidate | Pending. |

A green bounded model or container regression does not establish a disk reserve, an elapsed-time bound, or full-duration safety.
