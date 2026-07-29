# Soak Benchmarks (EPIC-010)

The 60-hour weekend merge-recovery soak (Friday 19:30 Pacific → Monday 07:30
Pacific) produces week-over-week benchmark metrics instead of pass/fail only.
The Mon–Thu 22h soaks run the same suite against `dev` but are not benchmark
runs — see [Weekend vs daily](#weekend-vs-daily). Design history and decisions:
[work log](work-logs/task-EPIC-010-2026-07-15T20-57Z.md), story US-004 in
[UserStories.md](UserStories.md).

## Where to look

- **Trend dashboard** (charts, per-provider split, run links):
  <https://f1r3fly-io.github.io/f1r3node-rust/> — two tabs, Weekend and Daily,
  each showing its own verdict on the tab button so both series are readable
  without clicking through.
- **Weekly email alert**: plain-text summary with the verdict and dashboard
  links, sent via OCI Notifications when the soak concludes.
- **Per-run detail**: the `merge-recovery-soak-*` artifact on the workflow run
  (iteration metrics, benchmark segments, logs, `report/` with
  `weekly-summary.json`, `verdict.json`, `perf-report.md`).

> **The Daily tab has no data source yet.** Only the weekend soak reaches the
> `perf_report` job that publishes to Pages, so the Daily tab reads "no runs
> yet" and daily results live only in the run artifact, which expires after 14
> days. Wiring the daily publish path is tracked separately.

## Weekend vs daily

| | Weekend | Daily |
|---|---|---|
| Window | Fri 19:30 → Mon 07:30 Pacific | Mon–Thu 19:30 Pacific |
| Duration | 60h, fixed | up to 22h, variable |
| Target | `master` | `dev` |
| Launches | always | only if commits landed since the last window |
| Early exit | never | when the target branch advances, after an 8h floor |
| Benchmark segments | yes | no |
| Gates releases | yes | no |

The weekend soak is exempt from the skip and the early exit because its numbers
are the week-over-week baseline, and those are only comparable if every run
covers an identical span. The dailies trade that comparability for catching
regressions sooner.

## Previewing the dashboard locally

The page loads its data with `fetch()`, which browsers refuse over `file://`,
so opening `index.html` directly shows a permanently empty page. Use:

```bash
scripts/preview-soak-dashboard.sh            # synthetic sample data, port 8770
scripts/preview-soak-dashboard.sh --live     # data from the published site
scripts/preview-soak-dashboard.sh --empty    # the bootstrap (no data) state
```

Source is `.github/dashboard/`; the server and sample-data generator are one
std-only Rust program built with plain `rustc` (no crates, no `Cargo.toml`,
nothing added to the workspace). The sample fixtures are deterministic and
deliberately include a regressed run, so the failure styling is exercised
without hand-editing anything. Everything generated lands in the gitignored
`site/`, rebuilt on start and removed on exit.

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

**Removing an address** (maintainer-side, e.g. a departed team member or a
mistyped address — recipients can always just use the unsubscribe link):

```bash
oci ons subscription list \
  --compartment-id <compartment-ocid> \
  --topic-id <topic-ocid> --all      # find the subscription OCID by endpoint
oci ons subscription delete --subscription-id <subscription-ocid>
```

## Operational notes

- Benchmarks run **only** in the 60h weekend soak (`duration_seconds ==
  216000`); see [Weekend vs daily](#weekend-vs-daily) for the full split.
- The dashboard site deploys from the soak workflow via GitHub Pages
  (Settings → Pages → source "GitHub Actions" must be enabled once). A separate
  workflow, `soak-dashboard-pages.yml`, redeploys the page shell when
  `.github/dashboard/` changes, preserving any already-published data.
- Metric emission is fail-soft end-to-end: a broken segment or missing
  sample never fails the soak; only the verdict comparison can.
- First run bootstraps: no baseline → verdict passes and seeds the history.
