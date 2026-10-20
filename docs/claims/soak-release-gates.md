# Claim: Soak Release Gates

```yaml
claim_id: CLAIM-SOAK-GATE-001
status: proposed-unratified
artifacts:
  - .github/scripts/report-soak-gate.sh
  - .github/scripts/ca-soak-dispatch.sh
  - .github/workflows/ca-soak-dispatch.yml
  - .github/workflows/merge-recovery-soak.yml
tests:
  - .github/scripts/test-soak-gate.sh
  - .github/scripts/test-ca-soak-dispatch.sh
  - .github/scripts/test-soak-schedule-routing.sh
  - .github/scripts/check-workflow-invariants.sh
references:
  - docs/ci.md
  - .mergify.yml
  - .github/scripts/tag-ca-release.sh
  - docs/cbc-evidence/github-scripts-report-soak-gate-sh.md
  - docs/cbc-evidence/github-scripts-ca-soak-dispatch-sh.md
  - docs/cbc-evidence/github-workflows-ca-soak-dispatch-yml.md
```

## Claim

This claim covers the soaks that gate the cost-accounting release train of issue #764 and issue #791. A `ca/` pull request enters the `ca-item` queue of Mergify only with the `ca-soaked` label. `just ca-tag` tags a `ca-release` commit only when it has a successful `Soak Gate (60h)` status.

**Gate result.** A gate run posts `Soak Gate (22h)` (gate `ca-item`) or `Soak Gate (60h)` (gate `ca-release`) on the commit that it soaked, and on no other commit. The status is success only when these four conditions are true:

- The soak job succeeded.
- The soak job reached its completion marker.
- The integration preflight passed.
- The soak verdict is `pass`.

A cancelled soak posts error, and every other result posts failure. A run never replaces the status of a newer run of the same commit.

**Label.** The `ca-soaked` label is added only after a successful `ca-item` result. The pull request must be open into `ca-canary`, and its head must still be the soaked commit. The script then reads the head again, and it removes the label if the head moved. A final `ca-item` result that is not a success removes `ca-soaked` when the head is still the soaked commit.

**Label events.** The `ca-soak` label removes `ca-soaked` before it starts a soak. A push to the pull request by an actor other than Mergify removes `ca-soak` and `ca-soaked`. It also cancels every queued or running soak of that pull request. A Mergify push only merges the base, and it keeps both labels.


**Soak identity.** A gate run soaks an exact commit SHA. It is a full soak of its span: 22 hours for `ca-item` and 60 hours for `ca-release`. It cannot be a canary, a preflight-only run, a run without the preflight, a window restart, a candidate soak, or a scheduled run. Its one restart after an infrastructure failure soaks a full new span.

**Isolation.** A gate run has the concurrency group of its pull request or of `ca-release`. A gate run and a `daily` or `weekend` run therefore never cancel each other. Its soak runner registers under a label of its own run, so its soak job runs only on the VM that the run launched. A gate run publishes no checkpoint and no dashboard series.

## Implementation surface

| Behavior | Location |
| --- | --- |
| Gate result and label | `.github/scripts/report-soak-gate.sh` (`pending` and `final` modes, the newer-run check, the pull request head checks before and after the label add, the label removal after a failed result) |
| Label and cancellation events | `.github/scripts/ca-soak-dispatch.sh` (`labeled`, `synchronize`, `push`) |
| Event routing | `.github/workflows/ca-soak-dispatch.yml` |
| Gate input validation | `merge-recovery-soak.yml`, job `schedule_gate`, step `resolve` |
| Isolation | `merge-recovery-soak.yml`, the concurrency groups of `launch_runner` and `soak`, `SOAK_LABEL`, the `soak` runs-on, and the `perf_report` condition |
| Restart | `merge-recovery-soak.yml`, job `retry_within_window`, and the deadline of the final segment |
| Verdict | `merge-recovery-soak.yml`, step `verdict` and the `soak` output `verdict` |
| Status jobs | `merge-recovery-soak.yml`, jobs `soak_gate_pending` and `soak_gate_finalize` |

## Checks

| Check | Command | Status |
| --- | --- | --- |
| Gate validation, isolation wiring, gate result, label add and removal, head change during the label add, and newer-run rule | `bash .github/scripts/test-soak-gate.sh` | Green locally |
| Label and cancellation events, cancellation on every page and status, fork and base refusal, trigger and permission wiring | `bash .github/scripts/test-ca-soak-dispatch.sh` | Green locally |
| Daily and weekend routing unchanged | `bash .github/scripts/test-soak-schedule-routing.sh` | Green locally |
| Workflow security invariants | `bash .github/scripts/check-workflow-invariants.sh` | Green locally |

## Pending obligations

The claim is not discharged.

| Obligation | Status |
| --- | --- |
| Review package with the hosted run of the script tests | Pending |
| Maintainer acceptance | Pending |
| First live `ca-item` gate soak after this change reaches `master` | Pending. The dispatcher runs the soak from `master`, so no gate run can start before then. |
| First live `ca-release` gate soak | Pending. It needs the `ca-canary` and `ca-release` branches. |

A green fixture test does not show that a live soak, the Mergify queue, or GitHub event delivery behaves as stated.
