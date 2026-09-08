---
task: soak-disk-hygiene-stop
branch: fix/soak-disk-hygiene-stop
claimed_by: claude-session-74f6ecbb
claimed_at: 2026-09-08T01:30:00Z
handoff_status: ready
next_steps:
  - User opens the PR against dev and the issue it references; #378 is closed, and this is a distinct defect in the #379 implementation
  - system-integration agent commits and pushes its half (request in ../system-integration/docs/ToDos.md, id SI-TASK-SOAK-DISK-POST-MORTEM-2026-09-07)
  - "DONE 2026-09-08T06:20Z: repinned SYSTEM_INTEGRATION_REF x3 to SI 7f488f93 (PR #137 head after main 0fb6337 was merged in at 1e411383; descends per merge-base --is-ancestor). The first SI push c32f1f1d was refused because it was cut from dev and lacked #132 and #134"
  - Re-pin to the merged SHA once SI #137 lands
  - After the next soak with both halves, read the soak-health tag and the pm tags to name the consumer, then prune it
---

# Weekend soak disk deaths after the #379 floor

## What happened

Three weekend soak runs died the same way between 2026-09-05 and 2026-09-07.

| Run | Dispatch | Target | Guardian stamp | Runner lost |
| --- | --- | --- | --- | --- |
| 33939315110 | scheduler bot, Fri 02:31Z | master 487a35c29 | `disk-breach` at 3992 MB, 16:38:11Z | 16:38:34Z |
| 33978505238 | automatic restart, Fri 16:38Z | master 487a35c29 | `disk-breach` at 4022 MB, 23:52:25Z | 23:52:45Z |
| 34056342543 | manual, Sat 19:54Z | master be2324661 | `disk-breach` at 3597 MB, Sun 10:01:56Z | 10:02:14Z |

Each soak job ended with the runner worker failing on `No space left on device` while it wrote `/opt/actions-runner/_diag/Worker_*.log`. The workflow post-mortem found each VM terminated. The `soak-health` freeform tag survived on every instance record and proved the PR #379 disk guardian fired and killed the nodes. The runner still ran out of disk about 20 seconds after each stamp.

## Why the floor did not save the run

Hygiene runs at each iteration boundary when free space is inside floor plus band, 8192 MB by default. In run 34056342543 the pass at 05:42Z reclaimed 6.2 GB. The next three passes at 09:33Z, 09:43Z, and 09:52Z reclaimed nothing, while free space fell from 7956 MB to 7355 MB. The post-hygiene check compared against the 4096 MB floor only, so each pass let another iteration start. The last iteration crossed the floor mid-run, the guardian fired, and the runner died before any report.

The failure-evidence copies are not the cause. Every observed copy finished in 0 to 2 seconds. The pytest failure dumps are about 20 MB each. The growth is outside every path hygiene sweeps. The terminated VMs could not say which path.

Secondary observations:

- The Saturday manual dispatch received a segment 1 deadline of 20:00Z, six minutes after dispatch. Segment 1 ran zero iterations and the whole run landed in one 11.5 hour final segment with no checkpoint.
- All three runs also had the known test_load `deploys not finalized within 45s` failures, so the verdict would have been red without the disk death.
- The automatic restart fired once and the chain is capped at one. The manual dispatch carried `retry_attempt=1`, so it had no restart.

## What this branch changes

Commit `ca85cfe3e`, pushed, PR pending.

- `scripts/run-merge-recovery-soak.sh`. The post-hygiene check now ends the segment when free space is still inside floor plus band. A new `disk_usage_snapshot` writes `df` and `docker system df`. It also writes `du -sm` for the output dir, the three harness roots, the runner `_diag` and `_work` dirs, and the session dirs. It prints on every hygiene pass and goes into `disk-floor-breach.txt` on both disk stop paths. The guardian appends a compact per-root usage summary to the `soak-health` tag and writes the full snapshot beside the breach marker.
- `SOAK_TMP_ROOT` and `SOAK_RUNNER_ROOT` make the sweep and runner paths overridable, so the driver test sweeps a private tree.
- `scripts/bench/test-run-merge-recovery-soak.sh`. A third scenario shims `df` inside the band and `docker` to a no-op. It asserts that the stale session is swept and no iteration starts. It also asserts that the soak fails closed and the evidence carries `du` lines.

Verification: `bash -n` clean, driver test passed with three scenarios, pre-commit hook passed, full pre-push gate passed with 20 checks.

## Cross-repository half

The runner exit-path post-mortem in system-integration now adds a `df` line and bounded `du` lines to the `pm` tags. The change is applied and tested in `../system-integration` on `dev`, uncommitted. The request lives in that repository at `docs/discoveries/2026-09-07-soak-disk-post-mortem-request.md` with a ToDos entry. When its result file carries a SHA, repin here with `scripts/repin-system-integration.sh`.

## Attribution before limits

Raising the boot volume alone would move the death later. The next soak with both halves names the consumer in two places that survive the VM. Prune that consumer first, then size the volume if the steady state still needs it.
