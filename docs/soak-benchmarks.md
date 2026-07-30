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
  `weekly-summary.json`, `verdict.json`, `badge.json`, `perf-report.md`).
- **README badges**: four shields.io *endpoint* badges, all generated from the
  same `verdict.json` / `weekly-summary.json` the dashboard renders, so a badge
  cannot disagree with the page behind it:

  | Badge | Endpoint file | Producer |
  |---|---|---|
  | `soak · master` | `data/badge-soak.json` | `badge.json` |
  | `soak · dev` | `data/badge-soak-daily.json` | `badge.json` |
  | `stability` | `data/badge-stability.json` | `badge-stability.json` |
  | `perf` | `data/badge-perf.json` | `badge-perf.json` |

  `stability` is the share of iterations that completed a full bring-up → load →
  finalize cycle — a success rate, deliberately not called uptime, since the
  soak creates a fresh shard per iteration rather than watching a standing
  deployment. Its colour bands are absolute and advisory; the release gate is
  relative (week-over-week) and stays with the soak verdict, so 100% stability
  alongside a `regress` verdict is coherent. `perf` is always blue: a readout,
  not a judgement, because absolute latency and throughput have no threshold
  here.

  There are no CI badges. Per-commit build status is already rendered on the
  repository home page and in pull requests; the badge row is reserved for the
  soak signal, which has no other surface.

Both series publish to Pages, into separate files — `history.json` and
`history-daily.json`, each with its own `latest-summary`, `latest-verdict`,
`latest-report` and `badge-soak`. They are kept apart so the week-over-week
regression gate never compares a variable-length daily against the fixed 60h
weekend baseline. A Pages deploy replaces the whole site, so whichever soak
publishes carries the other series forward untouched; a transient fetch failure
aborts the deploy rather than publishing a site with a series missing.

Three workflows publish the site — `merge-recovery-soak.yml` (final),
`soak-checkpoint-publish.yml` (mid-run) and `soak-dashboard-pages.yml`
(dashboard edits) — and each carries the published data forward from its own
file list. **A new data file must be added to all three**, or the next publisher
to run deletes it.

Each run also records what it soaked: the target ref, the commit sha (linked to
GitHub) and the node version declared at that commit — the same value carried by
the Docker LABEL and image tag, so a dashboard row can be matched to a pulled
image. Runs that predate version recording show a dash; history is never
backfilled.

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
| Regression verdict | fails the run | published, warns only |

A daily regression is published and shown on the dashboard's Daily tab but does
not fail the workflow. Daily spans vary — they stop early once `dev` advances —
so a run-over-run delta can reflect a shorter run rather than a real regression,
and failing on that would train people to ignore a red soak.

The weekend soak is exempt from the skip and the early exit because its numbers
are the week-over-week baseline, and those are only comparable if every run
covers an identical span. The dailies trade that comparability for catching
regressions sooner.

## Mid-run checkpoints

A soak publishes when it finishes, which for a 22h nightly means no visibility
until the following afternoon, and for a 60h weekend means two and a half days
of silence. Both series therefore publish **checkpoints** at **07:30 and 13:00
Pacific** for every such instant inside the run:

| Run | Checkpoints | Segments |
|---|---|---|
| Daily (Mon 19:30 + 22h) | Tue 07:30, Tue 13:00 | 3 |
| Weekend (Fri 19:30 + 60h) | Sat and Sun, 07:30 and 13:00 | 5 |
| Weekend crossing spring-forward | the above plus Mon 07:30 | 6 |

The extra weekend checkpoint is not a rounding artefact: 60h from Friday 19:30
lands at Monday 08:30 rather than 07:30 once the clocks jump, which brings the
Monday-morning instant inside the run.

Mechanically, the soak runs as consecutive **segments** sharing one output
directory. The script resumes from a state file each time, so counters, the
original start time and iteration numbering all continue and the run behaves as
one continuous soak. Each segment except the last publishes what has happened so
far.

**A checkpoint carries no verdict.** It reports `status: in_progress`, and the
dashboard shows `running · Nh of Mh` on the tab. A partial run has fewer
iterations, a lower peak RSS and a throughput figure over a shorter window than
the baseline it would be compared against, so a regression verdict at that point
would measure the clock rather than the code.

**A checkpoint does not append to history.** The charts and table show completed
runs only; a partial entry would double-count the night once the run finishes
and publishes for real. Only the `latest-*` files for that series are replaced,
and the dashboard says so while a run is in progress.

Checkpoint publishing is fail-soft throughout. The dispatch is a warning if it
fails, and the soak continues — the run's real result still publishes at the end.

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
| finalization latency p50 / p95 / p99 | `f1r3fly.propose.timing` node logs |
| too-far-ahead errors | count of proposal rejections logged as too far ahead of the last finalized block, node logs |
| LFB convergence spread (p95 / max) | `SOAK_METRIC` registry (`scripts/bench/soak-metrics.json`), track-only |

Active (controlled-rate benchmark segments interleaved every Nth iteration —
a fresh local shard flooded at a fixed deploy rate for run-over-run comparable
latency): p50/p95 submit→finalize latency, throughput, finalization rate,
peak RSS. See `scripts/bench/run-bench-segment.sh`.

### Newly added metrics (2026-07-30)

Extends the passive rollup with metrics modelled on `asi-chain-testbed`'s
`pkg/metrics` chain-health charts, wired through the existing per-iteration →
per-run → dashboard pipeline:

- **Finalization latency p50 / p99** — the same `f1r3fly.propose.timing`
  `total_ms` samples already used for p95 (`iteration_finalization_latency` in
  `scripts/run-merge-recovery-soak.sh`), now also read at the 50th and 99th
  percentile. Rolled up per run as the median of each iteration's percentile
  (matching how p95 was already rolled up), and charted as three lines
  ("Finalization latency percentiles").
- **Too-far-ahead errors** — count, per iteration, of the exact log line
  `"Proposal failed: too far ahead of the last finalized block"`
  (`casper/src/rust/blocks/proposer/propose_result.rs:185`), summed across all
  iterations in the run. Distinct from finalization latency: this counts
  proposals the node refused outright rather than how slow finalization was.
  Charted as "Too-far-ahead errors".
- **LFB convergence spread (p95 / max)** — the `lfb_spread` metric already
  declared in `scripts/bench/soak-metrics.json` (max−min last-finalized-block
  across shard nodes) was captured per iteration but never reached the
  dashboard: `summary.json` only rolled up the fixed passive fields, not the
  registry's `SOAK_METRIC` output. `run-merge-recovery-soak.sh` now folds
  every registry-declared metric into `summary.json.tracked_metrics`
  generically (max/min aggregates fold with cross-iteration max/min, any other
  declared aggregate — p50, p95, ... — folds with the cross-iteration
  median), so declaring a new metric in `soak-metrics.json` is enough for it
  to reach a run's `weekly-summary.json` and chart, no further code change.
  Charted as "LFB convergence spread".

All three are `track`-policy metrics: recorded and charted only, they do not
enter the week-over-week gate in `soak-gate-thresholds.json`.

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
  sample never fails the soak; only the weekend verdict comparison can.
- First run bootstraps: no baseline → verdict passes and seeds the history.
