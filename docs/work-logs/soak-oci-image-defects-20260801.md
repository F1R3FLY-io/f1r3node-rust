# Work Log: Weekend Soak Defect Sweep (run 30713818751 post-mortem)

---
handoff_status: ready
claimed_by: claude-session-917f64e8
branch: hotfix/full-fix-of-actual-OCI-image
parallel_repository: F1R3FLY-io/system-integration
parallel_branch: hotfix/full-fix-of-actual-OCI-image
next_steps:

- Commit and push the post-canary corrections in this repository
- Run the dashboard restoration workflow for canary run 30726357502
- Re-run CI and review PR #190

---

## Root causes (run 30713818751, 2026-08-01 weekend soak)

One real event amplified by seven tooling defects:

1. Every load iteration breached the host-protection ceiling (18095/15319/17052MB
   vs 14336MB); the guardian correctly killed the nodes each time, but the soak
   loop counted it as an ordinary failure and kept relaunching for 2+ hours.
2. `write-soak-summary.sh` used jq variable `$def` — a jq keyword rejected by
   jq 1.6 on the Ubuntu 22.04 runner (CI lints on ubuntu-latest jq 1.7, which
   accepts it) — killing the summary and, via null `started_at`/`elapsed_seconds`
   metadata, invalidating the checkpoint.
3. Soak Checkpoint Publish ran `gh run download` in a non-git cwd without
   `--repo`; every attempt died on "not a git repository" but was reported as
   "artifact not ready" — against an artifact that existed.
4. Bench segments failed 100% silently: `latency-benchmark.sh` hard-requires
   `DEPLOYER_KEY` and nothing in the soak path supplied it; its output went to
   a bench.log that died with the VM.
5. The kernel OOM killer shot pytest (oom_score_adj 500) at ~21:11 while
   docker-provider node containers — uncapped, not pytest children — kept
   growing until the VM froze and the runner was lost. The harness's docker
   provider sets no per-container memory limit; the "platform-capped"
   assumption in resource_monitor.py was wrong.
6. The final results artifact upload lives at job end, so the lost runner
   uploaded nothing and Publish Soak Dashboard hard-failed on the missing
   artifact with no fallback.
7. The frozen VM stayed behind its reaper exemption (window end + 2h — for a
   Friday weekend-soak freeze, ~2 days of billed 16-OCPU) until manually
   terminated.
8. Full-directory evidence copies explained earlier 28–46min gaps, but the OCI
   canary showed a second delay: pytest printed its terminal summary at 01:16:29
   while the foreground pipeline did not return until 01:25:36. The soak driver
   now backgrounds the iteration, polls the independent guardian marker, and
   terminates the timeout process promptly when protection fires.

## Changes (this repo)

- `scripts/bench/write-soak-summary.sh` — `$def` → `$mdef` (jq 1.6).
- `scripts/bench/test-write-soak-summary.sh` — jq-keyword-as-variable lint
  across all soak scripts (runs in the CI Lint job).
- `scripts/run-merge-recovery-soak.sh` —
  - fail-closed on any host-protection breach (three detection channels:
    orchestrator guardian marker, harness marker, pytest.log message);
    writes `early-exit.txt` so later segments no-op; still reaches the
    completion marker so `retry_within_window` does not relaunch;
  - orchestrator-level host guardian: independent background process watching
    MemAvailable, SIGKILLs `/tmp/rnode` processes and `rnode.*` containers on
    sustained floor breach — survives pytest death (the observed failure);
  - failure-evidence copy is now selective (logs/CSV/JSON/conf via tar), loud,
    and timed;
  - minimal fallback `summary.json` when the full rollup fails, preserving the
    checkpoint metadata contract;
  - bench segment/bench.log tails surfaced on failure;
  - foreground pytest runs are actively terminated when the independent marker
    appears, with the matching process tree and FIFO cleaned up.
- `scripts/bench/aggregate-perf-report.sh` — complete runs regress on any
  passive iteration failure or host-protection marker, independent of baseline
  availability.
- `scripts/bench/run-bench-segment.sh` — `DEPLOYER_KEY` defaults to
  `BOOTSTRAP_PRIVATE_KEY` from `$NODE_REPO_DIR/docker/.env` (documented funded
  fixture); bench.log tail on failure.
- `.github/workflows/soak-checkpoint-publish.yml` — explicit `--repo` on
  `gh run download`; real error echoed per retry.
- `.github/workflows/merge-recovery-soak.yml` —
  - perf_report: results download non-fatal + recovery of the newest
    checkpoint artifact (publishes latest-* only, never appends in_progress
    state to history); job gains `actions: read`;
  - capture_diagnostics: new "Release reaper exemption" step shrinks
    `soak-deadline-epoch` to now+30min after the post-mortem, fail-soft;
  - 30-minute canaries cannot retry, checkpoint-publish, dashboard-publish, or
    send production failure notifications; optional breach injection derives a
    guaranteed host floor from the actual runner's total RAM.
- `scripts/bench/restore-soak-dashboard-run.sh` and
  `.github/workflows/soak-dashboard-pages.yml` — validate the restore run,
  attempt, series, status, and history ordering before removing exactly one
  accidental entry and restoring its predecessor.

## Changes (../system-integration)

- `integration-tests/test/infra/resource_monitor.py` — persists the breach
  reason to `host-protection-breach.txt` in the monitor output dir
  (machine-readable orchestrator channel); corrected the wrong
  "Docker/K8s — platform-capped" comment.

## Validation done

- `bash -n` all scripts; `check-workflow-invariants.sh` green; workflow YAML
  parses; summary test harness green.
- Real jq 1.6 binary: full test harness + driver + aggregator pass; the old
  `$def` program reproduces the production error exactly.
- Driver simulation with breach-emitting pytest stub: fail-closed after one
  iteration, `early_exit_reason=host_protection_breach`, segment-2 resume
  no-ops, checkpoint passes the exact publish-workflow metadata validation;
  fallback summary also passes the contract.
- OCI canary run 30726357502 used the production Ubuntu 22.04 image and pinned
  system-integration `66de4f95`. The independent guardian fired at 10134MB
  available, killed all six containers, restored 30GB available, stopped after
  one iteration, uploaded valid checkpoint/final artifacts, and the instance
  self-terminated with its boot volume.
- The checkpoint artifact downloaded successfully from a non-git working
  directory with explicit `--repo`, and its metadata passed the publisher's
  exact validation expression.
- All four amd64/arm64 Docker/subprocess OCI integration lanes passed against
  `66de4f95` in run 30725659408.

## Open items

- RSS ceiling tuning: the load profile genuinely peaks over 14336MB on the
  32GB VM (observed 16–18GB). Fail-closed now stops the burn, but either the
  ceiling or the load profile needs adjusting before the next weekend soak
  passes. Surfaced, deliberately not changed here.
- Per-container docker memory caps in the harness provider — recommended
  follow-up in system-integration; the orchestrator guardian covers the host
  meanwhile.
- Canary run 30726357502 exposed a false dashboard verdict: one failed passive
  iteration was published as PASS because verdict logic only compared failure
  rate to a baseline whose metrics were null. Absolute passive failures now
  produce a regression verdict and have dedicated tests.
- The canary branch had already been temporarily admitted to `github-pages`, so
  the canary appended run 30726357502 to the production daily history despite
  the intended non-publishing validation. Both temporary environment policies
  are removed. A validated restoration helper and workflow inputs remove that
  entry and restore run 30516534214 after these corrections are pushed.
- The checkpoint dispatch intentionally targeted the default branch, so its
  workflow ran pre-fix master code and reproduced the missing-`--repo` failure.
  The corrected command and metadata contract passed separately; the first
  post-merge checkpoint remains the end-to-end deployment proof.
