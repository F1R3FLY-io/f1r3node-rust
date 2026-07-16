# Weekend Soak Benchmarks (EPOCH-010)

The 72-hour merge-recovery soak (Friday 22:00 Pacific → Monday) produces
week-over-week benchmark metrics instead of pass/fail only. Design history and
decisions: [work log](work-logs/task-EPOCH-010-2026-07-15T20-57Z.md), story
US-004 in [UserStories.md](UserStories.md).

## Where to look

- **Trend dashboard** (charts, per-provider split, run links):
  <https://f1r3fly-io.github.io/f1r3node-rust/>
- **Weekly email alert**: plain-text summary with the verdict and dashboard
  links, sent via OCI Notifications when the soak concludes.
- **Per-run detail**: the `merge-recovery-soak-*` artifact on the workflow run
  (iteration metrics, benchmark segments, logs, `report/` with
  `weekly-summary.json`, `verdict.json`, `perf-report.md`).

## What is measured

Passive (the soak's own load, per iteration, rolled up per run):

| metric | source |
|---|---|
| failure rate | pytest results per iteration |
| throughput (iterations/hour) | wall-clock per iteration |
| peak node RSS | harness `--monitor` resource timeseries |
| finalization latency p95 | `f1r3fly.propose.timing` node logs |

Active (controlled-rate benchmark segments interleaved every Nth iteration —
a fresh local shard flooded at a fixed deploy rate for run-over-run comparable
latency): p50/p95 submit→finalize latency, throughput, finalization rate,
peak RSS. See `scripts/bench/run-bench-segment.sh`.

## Regression gates

Thresholds live in one file: `scripts/bench/soak-gate-thresholds.json`
(maintainer-approved 2026-07-15). Week-over-week, the passive metrics **fail
the soak run**: failure rate +5pts, peak RSS +20%, finalization p95 +20%,
throughput −20%. Active-segment metrics warn.

**Releases are gated**: the `release.yml` workflow refuses to bump/tag on
`master` while the latest published soak verdict is `regress`. Maintainer
override: include `[soak-override]` in the release commit message (the gate
logs a warning and proceeds).

## Email subscription (OCI Notifications)

The topic is `soak-benchmark-reports` (provisioned via
`scripts/oci/create-ons-topic.sh`; its OCID is the repo Actions variable
`SOAK_ONS_TOPIC_OCID`). The same script subscribes recipient addresses —
pass them as arguments (or via `SUBSCRIBER_EMAILS`); re-running it is safe,
already-subscribed addresses are skipped:

```bash
COMPARTMENT_OCID=<compartment-ocid> \
  scripts/oci/create-ons-topic.sh user@example.com admin@example.com
```

ONS emails each new address a confirmation link; the recipient confirms to
start receiving alerts and every mail carries an unsubscribe link. The
recipient list is stored in OCI (view it with `oci ons subscription list
--topic-id <topic-ocid> ...`) — never in this repository.

## Operational notes

- Benchmarks run **only** in the 72h weekend soak (`duration_seconds ==
  259200`); the Mon–Thu 24h soaks are unchanged.
- The dashboard site deploys from the soak workflow via GitHub Pages
  (Settings → Pages → source "GitHub Actions" must be enabled once).
- Metric emission is fail-soft end-to-end: a broken segment or missing
  sample never fails the soak; only the verdict comparison can.
- First run bootstraps: no baseline → verdict passes and seeds the history.
