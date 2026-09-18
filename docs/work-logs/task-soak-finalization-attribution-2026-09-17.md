---
task: soak-finalization-attribution
branch: fix/soak-finalization-attribution
base_commit: bc23c8667ebef0f3fb7c3310caf85ce106df25fa
handoff_status: blocked
next_steps:
  - Discharge the two pending CbC claims or obtain an explicit maintainer waiver.
  - Leave PRs 430 through 433 for the user to merge.
  - Verify workflow activation separately after the prerequisite merge.
---

# Soak finalization attribution

## Scope

The user authorized diagnostic changes only. This branch does not import the protected-harness prerequisites or activate scheduled workflows.

The changes must preserve validation results, state hashes, storage operations, cache decisions, and lock ordering. Finalization limits remain unchanged.

## Measurements

- Checkpoint timers separate action preparation, partitioning, serialization, leaf writes, lock waits, history processing, and root commits.
- Checkpoint histograms record action counts and serialized key/value bytes. These bytes do not measure physical database growth.
- Replay timers separate consensus semaphore acquisition, replay execution, and mergeable-channel persistence.
- Repeat-deploy timers separate parent lookup, rejected-signature lookup, retry checks, watermark lookup, carrier probes, and ancestor scans.
- The collector includes these metrics and the existing arrival-depth metrics from PR #435.

Duration histograms use seconds. Existing lock-wait counters use nanoseconds and require their matching acquisition counters.

Checkpoint metrics aggregate all callers. They cannot independently assign checkpoint cost to merge, play, or replay.

A stage emits a sample only when execution reaches that stage. Missing samples do not mean zero work or zero duration.

Cancellation and panic can leave an attempted stage without a sample. Lock-wait histograms include the existing lock helpers and their counter updates.

The collector reports stage means and sample counts, not stage percentiles. Do not sum means with different sample counts. Nested timers overlap.

## Report capture

The pinned integration suite scrapes registry entries but formats only a fixed subset. Registry extension alone would leave the new measurements absent from logs.

The extension now appends an `ISSUE24_METRICS` JSON record to each formatted node report. Existing phase and validator headings identify the record.

The JSON record includes units and preserves observed zero deltas. Missing measurements, missing baselines, counter resets, and nonfinite values remain unknown.

The extension preserves existing report text. It rejects unsupported function contracts before replacing the original file.

The extension does not change workload assertions or finalization limits. The structured `SOAK_METRIC` registry and protected driver remain unchanged.

The integration suite can omit an entire report when no metrics exist. An absent report does not establish zero activity.

## Verification plan

Regression tests must check empty, serial, and parallel checkpoints. They must check validation fast paths, scan failures, replay failures, and successful replay.

Collector tests must check both metric registries, repeated extension, and rejection of malformed input without file damage.

Correctness-by-Construction (CbC) applies to `validate.rs` and `runtime_manager.rs`. Their claim is diagnostic noninterference with the behavior listed above.

Regression tests provide bounded evidence. They do not prove unrestricted semantic equivalence or establish a cause for issue #24.

## Follow-up after the prerequisite merge

Workflow control files come from `github.workflow_sha`. Selecting a candidate through `target_ref` does not change the protected driver.

Provider comparisons require the same candidate, driver, suite revision, configuration, and workload seed. Each comparison must preserve workload failures before infrastructure termination.

PR #436 does not yet supply implemented profile adapters. No live soak or provider comparison belongs to this diagnostic patch.

## Results

The collector regression test failed before implementation because the requested metrics were absent. The checkpoint attribution test also failed before instrumentation.

All 74 targeted Rust tests now pass:

| Command | Result |
| --- | --- |
| `cargo test --release -p rspace_plus_plus --test mod history::history_repository_tests` | 11 passed |
| `cargo test --release -p casper --test mod repeat_deploy` | 22 passed |
| `cargo test --release -p casper --test mod runtime_manager_test` | 36 passed |
| `cargo test --release -p casper --lib rust::util::rholang::runtime_manager::tests` | 5 passed |
| `bash scripts/bench/test-extend-issue24-metrics.sh` | Passed |
| Targeted `rustfmt --check --config skip_children=true` | Passed |
| `bash -n` on both changed shell scripts | Passed |
| `git diff --check` | Passed |

The first Casper attribution assertions repeatedly read histogram snapshots. Each read consumed the samples. The corrected tests read each snapshot once before counter assertions.

The collector tests check registry coverage against Rust constants, byte-identical repeated extension, metric units, observed zeros, unknown values, and malformed-input preservation.

A separate smoke test used the actual integration module at revision `b3d14b27e3c6276b1eb4ab9ccef04e02b0c4e283`.

The smoke test exercised scraping, delta calculation, and formatted output with labeled Prometheus samples. It confirmed that missing baselines remain unknown.

Language-server checks returned no reported diagnostics, but some checks remained inconclusive. The release test builds provide the completed Rust compilation checks.

Local logs are in `/tmp/soak-attribution-checks/`. These temporary logs are not committed evidence artifacts.

## CbC gate

Both embedded verification attempts returned exit code 3 because Verus is unavailable. This change also lacks a formal specification connected to Rust execution.

The strict CbC gate returned exit code 4 with two pending claims. The evidence records retain pending status without a waiver:

- `docs/cbc-evidence/casper-src-rust-validate-rs.md`
- `docs/cbc-evidence/casper-src-rust-util-rholang-runtime-manager-rs.md`

These gaps block full completion. The passing regression tests do not discharge the formal claims.

No PR prerequisite, Git commit, deployment, live soak, or controlled provider comparison occurred.
