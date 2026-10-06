---
doc_type: todos
version: "1.1"
last_updated: 2026-09-21
mr_status:
  ready: false
  target_branch: master
---

# Tasks and Epics

This document tracks implementation work through **epics** (logical groupings of related tasks).

**Document Structure**

- Active work: This file (`docs/ToDos.md`)
- User stories: `docs/UserStories.md`
- Completed work: `docs/CompletedTasks.md`
- Backlog: `docs/Backlog.md`

**Shared Coordination File:** `/tmp/migrationPlan.md` (read by agents in both f1r3node and f1r3node-rust)

---

## Active Coordination

- **Casper ratification follow-up (2026-09-16).** EPIC-017 owns the pre-#216 models, baseline conformance, and harness preparation on `formal/soak-casper-consensus`. EPIC-018 owns a separate formal-methods harness PR after PR #216 merges. The [meeting record](https://github.com/F1R3FLY-io/f1r3node-rust/pull/390#pullrequestreview-5227717933) controls both phases. PRs #430 through #433 remain prerequisites in stack order. The [branch plan](https://github.com/F1R3FLY-io/f1r3node-rust/blob/ba9758507194d6e34bc1494d416b87405facd550/docs/plans/casper-ratified-soak-2026-09-16.md) (local Git: `ba9758507194d6e34bc1494d416b87405facd550:docs/plans/casper-ratified-soak-2026-09-16.md`) defines the handoff, separate completion gates, and deferred-policy restrictions.

<!-- Compact, current-state-only. This section replaces the free-form status
     entries that previously accumulated at the top of this file; the full
     narrative history is preserved verbatim in
     docs/work-logs/coordination-archive-2026-08-01T03Z.md.
     NOTE: docs/discoveries/*.md is gitignored here (.gitignore:123) — use
     docs/work-logs/ for durable cross-agent notes, and keep this section to
     current operative facts only. -->

- **System-integration PR #118 reviewed head repin prepared (2026-08-15).** PR #118 corrects four harness defects from f1r3node-rust CI run `31886226287`. FT convergence now requires `FT >= FTT` and monotonicity. Epoch-boundary bond blocks accept either valid closeBlock transition map. Phase 5 still requires exact activation. Retired log snapshots retain their owning test allowances without cross-test leakage. Bond assertions now use the finalization resolver's canonical deploy block instead of an orphaned first inclusion. Focused unit tests protect the log-scan bookkeeping. The local bonding suite passed in 208.90 seconds with 5,636 MB peak RSS. This branch pins all three `SYSTEM_INTEGRATION_REF` sites to immutable reviewed PR head `735a7b95a3af74677f9519a6f01049cbc004bca4`. It retains dedicated shards, canonical finalized-block selection, standard `--rss-ceiling-mb 10000` limits, and the weekend preflight's `45056` MB limit.
- **PR #182** (`hotfix/renormalize-system-integration-pin-post-79` → `dev`, head `121029f1`) normalizes all three `SYSTEM_INTEGRATION_REF` sites to system-integration `main` `369d49df2f97e65b3d0ad869aa668a7383b11179` (the post-#79/#80 promotion). Multi-agent review posted 2026-08-01: approved 3-0 (anthropic abstained on an API billing error). This completes and supersedes the 2026-07-31T19:33 PDT handoff; the similarly named local branch `hotfix/normalize-system-integration-pin-post-79` is stale and has no PR.
- **Soak memory envelope re-based; weekend soak awaiting 48GB VM (2026-08-10, claude-session-ecaee825).** The 2026-08-09 weekend-soak breaches (runs 31331480002/31332864501) were NOT a merge regression — local A/B exonerated master@eb4030c2 and the harness pin diff was empty; the 6-node shard's real envelope is 16.7–19.3GB. PR #217 (merged) raised SOAK_RSS_CEILING_MB→20480 / lowered floor→8192; run 31390673884 then showed 32GB cannot hold the envelope (orchestrator guardian, free 6524MB). FINAL (maintainer, both sessions): system-integration sets fleet default AMD64_MEM_GB=48 (branch hotfix/raise-soak-vm-memory there, awaiting push/PR/merge); this repo's hotfix/raise-soak-vm-memory then takes one commit: SYSTEM_INTEGRATION_REF bump ×3 sites + ceiling 20480→28672 + these doc updates, then re-dispatch weekend-60h. Budget follow-up RESOLVED (SI PR #99 verification): no dollar-denominated cap exists in the runner stack — all lifetime guards are time-based (reaper MAX_AGE_HOURS + soak-deadline-epoch exemption, cloud-init idle/wedge timeouts); the ~$12/~$33 figures in the TASK note below are cost ESTIMATES only, ≈$13 daily/≈$35 weekend at 48GB. Evidence: docs/work-logs/soak-rss-regression-2026-08-09.md (local).
- **RESOLVED (2026-08-09): the hold below was overtaken — dev→master promoted via PR #213 and the weekend-soak pin question is moot under the re-based envelope above.** ~~Hold `dev` → `master` until the weekend soak snapshot is verified.~~ The Friday 19:30 Pacific scheduled `Merge Recovery Soak` run must exist with its `headSha` recorded, confirming it launched from the pre-normalization `master` (PR #181 pin `79262d8b`), before promoting. Merging first would silently move the weekend soak to the post-#79 `369d49df` pin. If no scheduled run appears, hold the promotion and investigate or manually dispatch from the intended pre-normalization `master`. Known discrepancy: scheduled runs initialize `target_ref=dev` although comments say the Friday weekend run targets `master` — treat the captured workflow `headSha`/pin and the resolved target SHA as separate evidence.
- **Run 30661821085 (2026-07-31 dispatch) is CLOSED.** Root cause: a 52-minute OCI host stall froze the VM's userspace ("runner lost, VM healthy" class) — not a node or test failure; the node was healthy at block 141 when output stopped. Evidence was extracted and hash-verified, and the evidence VMs were released with durable OCI backups remaining. Full analysis: `../system-integration/docs/ToDos.md` and the archive work log.
- **Cross-agent INBOX** entries from claude-session-02f66bb7 are archived; their actionable items live in TASK-010-6, TASK-010-7, and TASK-010-8. Use tracked files (this file or `docs/work-logs/`) for inter-agent messages — never `docs/discoveries/`.

---

## MR/PR Tracking

When all tasks in this file are complete and ready for merge, update the frontmatter:

```yaml
mr_status:
  ready: true
  target_branch: master
  title: "feat: f1r3node -> f1r3node-rust migration"
  description: |
    ## Summary
    - Full migration from f1r3node monorepo to standalone Rust workspace
    - Code sync, CI/CD, Docker, issue migration, deprecation

    ## Test plan
    - [x] All 11 crates build and pass tests
    - [x] Docker image publishes under new name
    - [x] system-integration tests pass against new image
```

---

## Active Epics

<!-- Epics are ordered by priority. Work on the highest priority epic first. -->

---

### EPIC-021: Issue #24 Replay Throughput Root Cause Under the CbC Harness

```yaml
---
epic_id: EPIC-021
title: "Issue #24 Replay Throughput Root Cause Under the CbC Harness"
status: in_progress
priority: p0
user_story: null
issues: [24]
blocked_by: []
created_at: 2026-10-03
updated_at: 2026-10-05
claimed_by: claude-session-aa467dea
claimed_at: 2026-10-03T21:10:00Z
branch: fix/issue-24
follow_on_branch: "fix/issue-24-root-cause-fix (draft PR #620). Since 2026-10-05 its base is chore/finish-TASK-020-4-log-growth (PR #622), so that one soak tests both PRs. The Slashing suite and the heavy CI suite do not run on PR #620 until PR #622 merges and the base returns to dev."
pr_base_branch: dev
origin: "The weekend-60h soak 37090117438 on master fce422a7d stopped after about 8 hours. Eight passive iterations failed first, then the disk guardian stopped all nodes in iteration 26 at 3,683 MB free against a 4,096 MB floor. The verdict was regress, with finalization p95 50.7 s against a baseline of 40.9 s plus 20 percent. Issue #24 records the same sustained-phase finalization failure since 2026-09."
execution_contract:
  base_branch: fix/issue-24
  base_revision: f93b72699565e45097b37a99b0413cef16bacd00
  scope: "Find the root cause of the issue #24 sustained-phase finalization failure with the soak harness of EPIC-017, the stage metrics of PR #441, and a new pending CbC claim. Separate the host disk breach, the passive iteration failures, and the finalization lag before any fix."
  git_policy: "Do not merge, push, or create a PR without separate user authorization. Commits require /quick-commit consent."
  cbc_policy: "Register the new claim as pending before any code change. The claims audit of CLAIM-CASPER-SOAK-001 to -008 requires exactly those eight claims, so the new claim uses its own identifier, specification, and verification plan outside formal/tlaplus/casper_soak/verification-plan.jsonc."
  pr_policy: "The branch starts from master f93b72699. Its PR targets dev after PR #569 brings master into dev, or after the branch merges dev."
evidence:
  failed_run: 37090117438
  new_run: 37153082817
  new_run_target: f93b72699565e45097b37a99b0413cef16bacd00
  issue_comments: ["2026-09-16 nightly soak evidence for 2026-09-12 to 2026-09-15", "2026-09-25 submit-to-finalization breakdown on dev 6d6d4fed6"]
  baseline_run: "37224478325 on master 95be0d450, cancelled by the user on 2026-10-05 in segment 3. Segment 2 had 19 failures in 91 iterations, all test_load 'N deploy(s) not finalized within 45s'."
  fix_run: "37343966570, daily-24h, dispatched on 2026-10-05 for fix/issue-24-root-cause-fix at 0a0713663 (dev b5cbb51d1, PR #622 at 82fe22a0a, and PR #620)."
tasks:
  - id: TASK-021-1
    title: "Attribute the failure of soak 37090117438 with the existing evidence"
    status: in_progress
    claimed_by: claude-session-aa467dea
    claimed_at: 2026-10-03T21:10:00Z
    blocked_by: []
    acceptance:
      - "The analysis separates three causes: the disk guardian breach, the eight passive iteration failures, and the finalization p95 regression."
      - "Each cause has a first-failure time, the failed assertion or guardian rule, and the artifact path that shows it."
      - "The analysis compares the failure with the issue #24 evidence of 2026-09-12 to 2026-09-15 and the 2026-09-25 stage breakdown, and states what matches and what is new."
      - "The analysis states which questions need the stage metrics of run 37153082817, because master fce422a7d did not have them."
  - id: TASK-021-2
    title: "Register a pending claim for replay latency and finalization lag under sustained load"
    status: pending
    claimed_by: null
    blocked_by: [TASK-021-1]
    acceptance:
      - "A new specification, for example docs/claims/casper-replay-throughput.md, registers CLAIM-REPLAY-THROUGHPUT-001 with status pending before any code change."
      - "The claim names its observables in terms of the stage metrics of PR #441: block replay runtime lock wait, execute, and save-mergeable time, history checkpoint stages, and repeat-deploy stages."
      - "The claim states a bound on per-block replay latency and on finalization lag in the sustained phase, with the workload, topology, and window of the bound."
      - "The claims audit of CLAIM-CASPER-SOAK-001 to -008 still passes with exit 0."
  - id: TASK-021-3
    title: "Add a replay-throughput soak profile with a bounded model and executable bindings"
    status: pending
    claimed_by: null
    blocked_by: [TASK-021-2]
    acceptance:
      - "A bounded TLA+ model of the replay pipeline has one clean configuration and negative controls that violate the claim invariants as intended."
      - "Executable bindings check the claim against the ISSUE24_METRICS records of a soak run, with fixtures for a passing run and for each violation."
      - "A check script and a hosted workflow follow the pattern of the existing profiles, with the pinned TLA+ tools."
      - "The profile records refutation bounded-safety-pass and binding passed before maintainer review."
  - id: TASK-021-4
    title: "Attribute finalization lag by replay stage with soak 37153082817"
    status: pending
    claimed_by: null
    blocked_by: [TASK-021-1]
    acceptance:
      - "The analysis gives the per-stage share of block replay time in each phase, from the stage metrics of run 37153082817."
      - "The analysis states whether the host disk headroom stayed above the guardian floor with the node log caps of PR #451, and reports the lowest free value."
      - "The profile of TASK-021-3 evaluates the run, and its verdict is recorded on issue #24."
  - id: TASK-021-5
    title: "Propose the root-cause fix and verify it with the profile"
    status: pending
    claimed_by: null
    blocked_by: [TASK-021-3, TASK-021-4, TASK-021-8]
    candidate_fix: "0ef0966c6 on fix/issue-24-root-cause-fix (PR #620): reset() validates the root with a read and no longer writes current-root, the roots lock is released before the history lock, and record_root writes in one LMDB transaction. 2cd9790f9 removes the unused validate_and_set_current_root path. The code review of 2026-10-05 found no correctness defect. After a restart, history opens at the last checkpointed root, not at the last reset root. Soak 37343966570 tests the candidate. The acceptance items still apply: the maintainer chooses the fix, and the TASK-021-8 findings do not yet show that the roots lock is the root cause."
    acceptance:
      - "A written root cause names the stage, the mechanism, and the evidence that excludes the other causes."
      - "The maintainer chooses the fix before implementation."
      - "The fix passes the replay-throughput profile and a soak run, and the maintainer accepts CLAIM-REPLAY-THROUGHPUT-001."
      - "A new branch off dev carries the claim, the profile, the fix, and the cited evidence. PR #580 merges first with TASK-021-6 and the EPIC-021 plan only (decision of 2026-10-04)."
  - id: TASK-021-6
    title: "Stop the soak failure-evidence copy from duplicating earlier harness sessions"
    status: done
    claimed_by: claude-session-aa467dea
    claimed_at: 2026-10-04T03:20:00Z
    completed_at: 2026-10-04
    resolution: "Fix 602d63c7b. Review package casper-soak-driver-evidence-scope-20261004-01 at 384b5fb08. jltatbeach accepted it in PR #580 comment 5979020312. The acceptance package casper-soak-driver-evidence-scope-acceptance-20261004-01 and the two ledger records carry the new digests. The PR #580 review fixes in 345a99a23 have review package -02 at 3d384bc9e, accepted in comment 5980616375, with acceptance package casper-soak-driver-evidence-scope-acceptance-20261004-02."
    blocked_by: []
    origin: "Soak 37153082817 on master f93b72699 stopped after about 3 hours at the disk hygiene band with 8,047 MB free. The failure-evidence copy in scripts/run-merge-recovery-soak.sh copied every earlier harness session into each failed iteration, so iteration N archived N sessions. The copies were 13.6 GB of the 15 GB output, and the harness log-archive root grew about 200 MB for each iteration."
    files:
      - scripts/run-merge-recovery-soak.sh
      - scripts/bench/test-run-merge-recovery-soak.sh
    cbc_policy: "scripts/run-merge-recovery-soak.sh is a cbc=mandatory discharged artifact of CLAIM-CASPER-SOAK-001 and is listed in docs/claims/soak-disk-protection.md. The change needs a new evidence package and a maintainer re-acceptance before check-casper-claims passes again."
    acceptance:
      - "The failure-evidence copy takes only files newer than the iteration's .started marker, like every other reader of the harness roots."
      - "Each completed iteration resets the harness data and log-archive roots after its metrics, evidence, and breach checks."
      - "A driver test scenario with two failed iterations proves that the second iteration's evidence and the harness root do not keep the first session."
      - "The driver test suite passes on Linux, and the maintainer accepts the new evidence for CLAIM-CASPER-SOAK-001."
  - id: TASK-021-7
    title: "Split scripts/run-merge-recovery-soak.sh into sourced modules with identical behavior"
    status: pending
    claimed_by: null
    blocked_by: [TASK-021-6]
    branch: refactor/soak-driver-modules
    pr_policy: "A separate branch and PR after TASK-021-6 lands. Do not combine the refactor with a behavior change."
    origin: "On 2026-10-04 the user asked to make the 2,084-line soak driver smaller and more maintainable, as a change separate from the disk fix."
    acceptance:
      - "The driver keeps the iteration loop and the orchestration. Host and disk protection, evidence and archives, metric extraction and summaries, and the benchmark segment move to sourced modules."
      - "The existing driver test suite passes without changes to its assertions."
      - "The new module files are registered as artifacts of CLAIM-CASPER-SOAK-001 and of the soak disk protection claim, and the maintainer accepts the new evidence."
      - "A later epic decides which parts move into the Rust casper-soak runtime."
  - id: TASK-021-8
    title: "Measure history repository lock hold times by call site"
    status: in_progress
    claimed_by: null
    blocked_by: []
    implementation_status: "Implemented in 05c78088e on fix/issue-24-root-cause-fix. The call-site counters are in the ISSUE24_METRICS records through scripts/bench/extend-issue24-metrics.sh. Acceptance items 3 and 4 wait for soak 37343966570."
    findings_2026_10_05:
      source: "Baseline soak 37224478325 (master 95be0d450, without the call-site counters): 98 test_load sessions, 76 passed and 22 failed. The analysis used the aggregate lock counters of master in the ISSUE24_METRICS records of validators 1 to 3."
      results:
        - "The unfinalized deploys are in the sustained phase (482 deploys in 17 sessions) and in the high phase (26 deploys in 8 sessions). The failures spread evenly over the run."
        - "The failures are a latency tail, not a discrete stall. In the sustained phase the finalization p95 median is 46.9 s for passing sessions and 58.7 s for failing sessions, against the 45 s gate. The LFB rate median is 20.6 blocks per minute for passing sessions and 17.9 for failing sessions."
        - "The roots lock wait is high in every sustained phase: a median of 7.1 s for passing and 9.1 s for failing sessions, for each validator, over about 2,900 calls. Other phases stay below 15 ms."
        - "No validator metric separates failing from passing sessions well. The AUC is 0.62 for the roots lock wait, 0.67 for the checkpoint time, 0.66 for the replay runtime lock wait, and at most 0.70 for any metric."
        - "test_load judges finalization on validator1 only. The boot and readonly nodes lag the validators by 12 to 20 blocks at drain and use about three times the memory of a validator, but the harness collects no ISSUE24_METRICS records for them."
      interpretation: "The roots lock contention is a constant cost of the sustained phase, not the trigger of a failure. The reset fix can still move the latency tail below the gate if the lock is on the critical path."
      baseline_metric_auc: "Sustained phase, validators 1 to 3, mean of the three. AUC is the probability that a failing session has the higher value. Roots lock wait 0.62, roots lock calls 0.61, current-history lock wait 0.62, checkpoint roots lock wait mean 0.57, checkpoint time mean 0.65, root commit time 0.59, replay reset time 0.53, replay runtime lock wait 0.65, replay user deploys time 0.64, apply trie actions time 0.61, blocks replayed 0.62. Test side: inclusion p95 0.70, finalization p95 0.69, LFB rate 0.31."
    soak_decision_metrics:
      run: "37343966570. Compare the sustained phase with baseline run 37224478325. Medians are for passing and failing sessions, for each validator unless stated."
      metrics:
        - role: outcome
          measure: "test_load failure rate (sessions summary: 'N deploy(s) not finalized within 45s')"
          baseline: "22 of 98 sessions (22 percent)"
          expected_if_root_cause: "At most 5 failures in about 100 sessions"
          reasoning: "This is the gate of issue #24. At the baseline rate, 100 sessions give about 22 failures, so 5 or fewer is a real change and not chance."
        - role: outcome
          measure: "Sustained finalization p95 (log line 'Phase sustained: ... finalization p95')"
          baseline: "Median 46.9 s for passing and 58.7 s for failing sessions, gate 45 s"
          expected_if_root_cause: "The median of all sessions falls clearly below 45 s"
          reasoning: "The failures are the tail of this distribution. The gate is marginal, so the pass count alone can change by chance. The distribution shows a real shift."
        - role: outcome
          measure: "Sustained LFB rate in blocks per minute (same log line)"
          baseline: "Median 20.6 for passing and 17.9 for failing sessions"
          expected_if_root_cause: "Higher than the baseline median"
          reasoning: "The rate measures finalization throughput directly. Slow sessions finalize fewer blocks per minute."
        - role: mechanism
          measure: "history_repository_roots_repository_lock_wait_ns (aggregate, present in both runs)"
          baseline: "7.1 s for passing and 9.1 s for failing sessions, over about 2,900 calls"
          expected_if_root_cause: "Close to 0, below 0.1 s"
          reasoning: "This shows whether the fix removed the contention. The local probe reduced the checkpoint roots wait from 295 ms to 0.19 ms."
        - role: mechanism
          measure: "history_repository_roots_repository_reset_hold_ns divided by _reset_calls, and _checkpoint_wait_ns"
          baseline: "Not measured in the baseline. The local probe gave 4.5 ms of hold time for each reset before the fix."
          expected_if_root_cause: "A few microseconds of hold time for each reset, and a checkpoint wait close to 0"
          reasoning: "These call-site counters answer TASK-021-8 acceptance items 3 and 4. They show which call site held the lock."
        - role: mechanism
          measure: "history_roots_store_writes against _record_root_calls plus _checkpoint_calls"
          baseline: "Not measured. Before the fix, each reset also wrote current-root."
          expected_if_root_cause: "Writes equal record_root calls plus checkpoint calls, with no writes from reset"
          reasoning: "This proves that reset makes no durable write. The roots and history stores share one LMDB environment with a single writer."
        - role: next_candidate
          measure: "history_checkpoint_time mean"
          baseline: "79 ms for passing and 116 ms for failing sessions, AUC 0.65"
          expected_if_root_cause: "Lower, because reset no longer competes for the LMDB writer"
          reasoning: "This is the next suspect if the outcome does not change. It is the strongest node-side separator in the baseline."
        - role: next_candidate
          measure: "block_replay_runtime_lock_wait_time mean"
          baseline: "140 ms for passing and 215 ms for failing sessions, AUC 0.65"
          expected_if_root_cause: "Lower or unchanged"
          reasoning: "The runtime lock serializes replay. If this stays high while the roots wait falls, replay serialization is the next cause to examine."
        - role: control
          measure: "history_repository_current_history_lock_wait_ns (aggregate)"
          baseline: "12 ms for passing and 13 ms for failing sessions"
          expected_if_root_cause: "About the same"
          reasoning: "The fix does not change this lock. A large change points to a different effect or a different workload."
        - role: control
          measure: "Blocks replayed (block_replay_phase_reset_time samples) and roots lock calls"
          baseline: "233 and 238 blocks, about 2,900 calls"
          expected_if_root_cause: "Within 10 percent of the baseline"
          reasoning: "The comparison is valid only for the same workload. A lighter workload can pass without any fix."
      decision_rules:
        - "Root cause confirmed: the roots lock wait falls close to 0, the controls stay within 10 percent, and the failure rate and the sustained finalization p95 both fall as expected."
        - "Mechanism works but is not on the critical path: the roots lock wait falls close to 0, but the finalization p95 and the LFB rate stay inside the baseline spread. Examine the checkpoint time and the replay runtime lock wait next."
        - "Fix not effective: the roots lock wait does not fall. Use the call-site counters to find the call site that holds the lock."
      confounders:
        - "The soak runs dev at b5cbb51d1, not master 95be0d450. dev adds the rholang file I/O handlers of PRs #613 to #617, which deploys of test_load do not use."
        - "The soak includes the PR #622 log guardian. It runs on the host and adds a Docker probe every 15 seconds, but no node code."
        - "The harness collects no ISSUE24_METRICS records for the boot and readonly nodes, so their lag stays unexplained."
    origin: "In soak 37153082817, history_repository_roots_repository_lock_wait_ns reached 2 to 21 seconds for each validator in test_deploy_throughput_and_finalization, and no other test exceeded 1 second. The checkpoint's own roots lock wait stayed near zero. The metrics record wait times only, so they cannot show which call site holds the lock. The candidate cause is reset() in rspace++/src/rspace/history/history_repository_impl.rs, which holds the roots lock while it waits for the current-history lock."
    files:
      - rspace++/src/rspace/history/history_repository_impl.rs
      - rspace++/src/rspace/metrics_constants.rs
      - scripts/bench/extend-issue24-metrics.py
    acceptance:
      - "The roots-repository and current-history locks record hold time and wait time with a call-site label: checkpoint commit, reset, record_root, contains_root, and the history readers."
      - "The new metrics appear in the ISSUE24_METRICS records, and the metrics extension test covers them."
      - "A soak run or a local throughput run attributes the roots lock wait to the call sites that hold the lock, and the result is recorded on issue #24."
      - "The analysis confirms or rejects the nested-lock hypothesis in reset() before TASK-021-5 proposes a fix."
  - id: TASK-021-9
    title: "Gate master on a SHA-bound soak verdict check"
    status: pending
    claimed_by: null
    blocked_by: [TASK-021-5]
    origin: "On 2026-10-04 the masterProtect ruleset required deployments to casper-campaign, ephemeral-launch, and protected-branch-image-publish. A deployment proves that a job started, not that a verdict passed. casper-campaign is created by the campaign launch job and allows only formal/soak-casper-consensus, and ephemeral-launch is created only by the gated fork path, so no promotion PR could satisfy them. The user wants merges to master to accept only soaked and verified artifacts after the issue #24 root-cause fix lands."
    files:
      - .github/workflows/merge-recovery-soak.yml
      - docs/release-process.md
      - docs/claims/casper-campaign-execution.md
    acceptance:
      - "The soak workflow publishes a check run, for example 'Soak verdict (campaign-stability-60h)', on the tested SHA only after the full soak passes, including the issue #24 finalization claims and a clean disk guardian."
      - "masterProtect requires that check from the GitHub Actions integration, and requires Integration Tests (amd64) and (arm64)."
      - "The promotion PR runs the heavy pipeline, or the gate reads the dev push run of the same SHA, so a summary check that passed without running does not satisfy the gate."
      - "masterProtect keeps protected-branch-image-publish and drops casper-campaign and ephemeral-launch from required deployments."
      - "The release process documents that a promotion PR carries the exact soaked SHA, and that a later dev commit voids the verdict."
      - "The changed workflow and claim artifacts have a review package and maintainer acceptance under their claims."
---
```

**Current state:** Created on 2026-10-03 after soak 37090117438 failed. Soak 37153082817 runs weekend-60h on master f93b72699 with the stage metrics. TASK-021-1 starts from the failed run, and TASK-021-4 waits for the new run.

---

### EPIC-020: Node Log and Accept-Path Self-Limits

```yaml
---
epic_id: EPIC-020
title: "Node Log and Accept-Path Self-Limits"
status: in_progress
priority: p0
user_story: US-009
user_flow: FLOW-002
blocked_by: []
created_at: 2026-09-23
updated_at: 2026-09-30
claimed_by: null
branch: fix/node-log-and-accept-backoff
pr_base_branch: dev
stack_order: "Branches from dev and merges to dev before PR #447. PR #447 then merges dev. The soak branch inherits the fix through its next merge of the observation branch."
origin: "On 2026-09-23 a nine-day compose network from a system-integration checkout filled 2.7 TB in one day. Its bootstrap ran out of file descriptors, the transport accept loop retried with no backoff and logged one ERROR line per attempt, the node wrote every line to its data volume and to stdout, and the container's json-file log had no size cap."
execution_contract:
  base_branch: dev
  scope: "Make the node self-limiting under an error storm: backoff and rate-limited logging on accept failures, a byte-bounded file log, one sink per deployment, and a repository check that every node compose service caps its container log. Harness enforcement belongs to EPIC-017 on the soak branch."
  git_policy: "Do not merge, push, or create a PR without separate user authorization. Commits require /quick-commit consent."
  cbc_policy: "The transport server and the logging module carry no cbc tag today. Propose cbc=mandatory for the accept path with a pending claim before the fix lands, or record the maintainer decision that the change stays untagged."
  cbc_decision: "The maintainer decided on 2026-09-30 that the accept path stays untagged for TASK-020-1. The regression tests in f1r3fly_server_resource_tests.rs are the verification."
tasks:
  - id: TASK-020-1
    title: "Back off and rate-limit the transport accept-error path"
    status: complete
    claimed_by: claude-session-f3cbc961
    claimed_at: 2026-09-30T00:40:00Z
    verification_claimed_by: 01a0ab62-71b3-7248-a800-37a6fde2e4fa
    verification_claimed_at: 2026-09-30T01:03:01Z
    verification_status: in_progress
    blocked_by: []
    work_log: docs/work-logs/transport-accept-resource-review-20260923.md
    implementation_status: "Hosted Test (comm) passed all 400 tests, including all seven resource regressions and the Linux descriptor-exhaustion test. The approved story and flow repair now links EPIC-020 to US-009 and FLOW-002."
    hosted_verification: docs/work-logs/evidence/task-020-1-hosted-20260930-01/report.json
    hosted_run: 36651370411
    hosted_job: 109688126217
    unit_tests: [comm/src/rust/transport/f1r3fly_server_resource_tests.rs]
    completion_blocker: null
    remaining: []
    files:
      - comm/src/rust/transport/f1r3fly_server.rs
    acceptance:
      - "On an accept error the listener sleeps with exponential backoff, capped at one second, before the next accept."
      - "Under sustained descriptor exhaustion the listener emits at most one ERROR line per backoff window and one summary line per minute with the suppressed count."
      - "A test injects descriptor exhaustion against the listener and asserts the bounded line count and the recovery once descriptors return."
      - "Ordinary accept throughput is unchanged. No backoff applies to a successful accept."
    implementation_plan:
      - "Step 1. Add a backoff state to the listener task: reset on success, double on error from 10 ms to 1 s."
      - "Step 2. Route accept errors through a rate limiter that logs the first error, suppresses repeats inside the window, and logs a periodic summary with the suppressed count."
      - "Step 3. Add the descriptor-exhaustion test with a lowered RLIMIT_NOFILE in a child process or a socket-pair fixture, and a regression that the current code fails."
    completion_gaps: []
    completed_date: 2026-09-30
  - id: TASK-020-2
    title: "Bound the file log by bytes, not only by time"
    status: complete
    claimed_by: pi-session-01a0ab62-71b3-7248-a800-37a6fde2e4fa
    claimed_at: 2026-09-30T01:20:37Z
    blocked_by: []
    work_log: docs/work-logs/task-020-2-byte-bounded-logging-20260930.md
    files:
      - shared/src/rust/tracing_init/mod.rs
      - shared/src/rust/tracing_init/bounded_file.rs
      - node/src/main/resources/defaults.conf
      - node/src/rust/configuration/mod.rs
    acceptance:
      - "logging.file accepts a maximum size per file and a maximum total size for the log directory, with defaults that bound a node to a few gigabytes."
      - "When the total bound is reached the oldest rotated file is removed before the appender writes further."
      - "A test drives a hot error loop through the appender and asserts the directory never exceeds the bound."
      - "The defaults comment no longer describes minutely rotation as the only way to bound disk use."
    implementation_plan:
      - "Step 1. Extend the rotation configuration with size-based rolling alongside the existing period."
      - "Step 2. Enforce the total directory bound in the appender with an oldest-first eviction."
      - "Step 3. Add the appender test and update the configuration test that pins daily rotation."
    unit_tests: [shared/src/rust/tracing_init/mod.rs, shared/src/rust/tracing_init/bounded_file.rs, node/src/rust/configuration/mod.rs]
    completion_gaps: []
    completed_date: 2026-09-30
  - id: TASK-020-3
    title: "One sink per deployment and a container log cap check"
    status: complete
    completed_on: "2026-09-30"
    recorded_by: claude-session-f3cbc961
    claimed_by: pi-session-01a0ab62-71b3-7248-a800-37a6fde2e4fa
    claimed_at: 2026-09-30T02:23:57Z
    claimed_at_source: clock_checkpoint_after_claim
    blocked_by: []
    work_log: docs/work-logs/task-020-3-deployment-log-caps-20260930.md
    implementation_status: "The node deployment commands and repository guards pass at 7d64c9d03. The external single-sink change merged into system-integration dev on 2026-09-30. Its main promotion is pending and is appended when it lands."
    external_handoff: docs/handoffs/task-020-3-system-integration-20260930.md
    external_main_revision: e3c4e14189f0c6ced2e9674487fcbdeffd93141b
    external_single_sink_merge_revision: ccd717195b35f75cef826f41d96b7028d8a874c0
    external_change:
      repository: F1R3FLY-io/system-integration
      pull_request: 146
      branch: fix/single-log-sink-per-deployment
      base_revision: ef9844893f19df3e7523bb97e9e0da0ca241bb10
      receiver: claude-session-fbb1f4d0
      dev_merge_revision: ccd717195b35f75cef826f41d96b7028d8a874c0
      dev_merged_at: 2026-09-30T22:10:44Z
      main_promotion_revision: null
      request_record: "system-integration docs/ToDos.md, section REQUEST: one node log sink per deployment (SI-TASK-020-3)"
    sink_contract:
      compose_and_smoke_test: "stdout through sink = stdout in conf/rust.conf and conf/standalone-dev.conf, bounded by json-file 100m x 3, read by docker logs and shardctl"
      integration_docker_provider: "file through --log-sink=file before run at 6 launch sites (NODE_LOG_SINK_ARGS in integration-tests/test/infra/compose.py), read from /var/lib/rnode/logs/node.log*"
      integration_subprocess_provider: "stdout through the conf, read from the captured process output"
      development_override: "both only through an explicit --log-sink=both before run"
    external_verification:
      - "unit-tests/test_log_sink_policy.py at ccd717195: 34 passed (run from a git archive export with the repository Poetry environment)."
      - "poetry run pytest unit-tests at the PR head: 355 passed. ruff 0.16.0 check and format clean."
      - "docker compose config for the 5 node variants: 11 node services, all json-file 100m and 3 files, Compose files unchanged."
      - "Live suites (system-integration commit e7163d57): test_heartbeat passed in PR CI. test_token_metadata standalone and shared, and test_shard_degradation: 15 of 15 passed locally with F1R3FLY_NODE_DEFAULTS_CONF set."
    node_verification_at_7d64c9d03:
      - "scripts/ci/test-compose-log-policy.sh: shard-vps2 3, ci-shard 5, ci-standalone 1 node services passed."
      - "cargo nextest run -p node --test log_sink_cli: 3 passed."
      - "scripts/supply-chain cargo test --test repository: 18 passed."
      - "Pre-commit fmt, clippy, test, and deny passed at the merge commit 7d64c9d03 (dev ccd4a4823 merged)."
    remaining_outside_this_task:
      - "The byte limits of node commit 6e1c8833a reach system-integration runs through the next node repin of SYSTEM_INTEGRATION_REF. TASK-020-4 on formal/soak-casper-consensus enforces them in the harness."
      - "Append the system-integration main promotion revision to external_change when it lands."
    unit_tests: [scripts/supply-chain/tests/repository.rs, scripts/supply-chain/tests/support/compose_logging.rs, node/tests/log_sink_cli.rs]
    files:
      - Cargo.lock
      - scripts/supply-chain/Cargo.toml
      - scripts/supply-chain/tests/repository.rs
      - scripts/supply-chain/tests/support/compose_logging.rs
      - scripts/ci/test-compose-log-policy.sh
      - node/tests/log_sink_cli.rs
      - docker/shard.yml
      - docker/standalone.yml
      - docker/observer.yml
      - docker/validator4.yml
      - docker/shard.vps1.yml
      - docker/shard.vps2.yml
      - docs/node/README.md
    acceptance:
      - "Every compose service in this repository that runs a node image sets logging.options.max-size and max-file."
      - "A repository test fails when a node compose service lacks the cap, in the same style as the workflow cache-write test."
      - "Deployment defaults use one sink. The sink both is documented as a development setting that doubles disk use."
      - "The system-integration compose file receives the same cap through a coordinated change in that repository, recorded here with its merge revision."
    notes:
      - "The six base Compose files already cap at 100m and three files. The CI port overlays inherit these limits."
      - "The monitoring Compose file has no blockchain node service. Its storage policy is outside this task."
      - "The local system-integration checkout was stale at hand-off time. Remote main already capped all eleven node service definitions across five variants. Its conf/rust.conf selected both sinks until PR #146."
  - id: TASK-020-4
    title: "Harness enforcement of node log growth under EPIC-017"
    status: done
    completed_at: 2026-10-05
    reopened_at: 2026-10-05
    reopened_reason: "The PR #622 review of 2026-10-05 found two probe defects. A container that stopped during a sample caused a false breach. An unreadable rotated container log was skipped without a report. The fix changes the driver and the disk fixture, so the acceptance of 2026-10-04 does not cover the new bytes."
    resolution: "First version implemented in 83a41b564 (PR #622). Review package casper-soak-log-budget-guardian-20261004-01 at 6ea45dc8f, accepted by jltatbeach in PR #622 comment 5983133741. The acceptance package casper-soak-log-budget-guardian-acceptance-20261004-01 and the ledger records of the driver and the disk fixture carry the new digests. The probe fix 82fe22a0a has review package casper-soak-log-budget-guardian-20261005-01 at 1a04095da, accepted by jltatbeach in PR #622 comment 6005866151. The acceptance package casper-soak-log-budget-guardian-acceptance-20261005-01 and the two ledger records carry the new digests."
    claimed_by: claude-session-aa467dea
    claimed_at: 2026-10-04T14:36:28Z
    claim_history: "claude-session-f3cbc961 claimed the task on 2026-10-02 and committed the design (e7e376a69). The user transferred the claim on 2026-10-04 for implementation on chore/finish-TASK-020-4-log-growth."
    mirrored_as: TASK-017-17
    design: docs/casper/design/soak-log-budget-guardian.md
    implementation_status: "Implemented on chore/finish-TASK-020-4-log-growth on 2026-10-04. The driver samples the container json-file log and the node log directory of each owned container, refuses admission on an unreadable probe, and breaches on 3 strikes or one sample at two times the budget. Ten log scenarios in scripts/bench/test-soak-disk-admission.sh pass in the disposable container, and the breach and refusal scenarios fail against the previous driver. CLAIM-SOAK-001 records the log caps as enforced. The 2026-10-05 fix skips a container that stops during a sample and fails the probe on a rotated log that exists but cannot be read. Scenarios log-probe-vanished and log-rotated-unreadable pass, and both fail against the driver at 514eb3026. All 12 log scenarios pass. The maintainer accepted the fix on 2026-10-05."
    owner_branch: formal/soak-casper-consensus
    blocked_by: []
    blockers_cleared: "TASK-020-1 and TASK-020-2 are complete. The implementation runs as TASK-017-17 on the soak branch."
    acceptance:
      - "The soak guardian samples the node log directory and the container json-file size, not only free space, and stops the run when either exceeds its budget."
      - "A soak fixture injects descriptor exhaustion into a node and asserts the guardian and the node limits hold."
      - "CLAIM-SOAK-001 records the log cap as an enforced check instead of an assumption."
    notes:
      - "This task is tracked here for ordering only. The downstream agent mirrors it into EPIC-017 on the soak branch, where the claim and the driver live."
---
```

**Current state:** TASK-020-1, TASK-020-2, and TASK-020-3 merged to dev through PR #451 on 2026-10-03 and reached master with the soak stack. TASK-020-4 is implemented on PR #622: the soak guardian samples the node log directory and the container json-file size, and CLAIM-SOAK-001 records the log caps as enforced. The maintainer accepted the first version on 2026-10-04 and the probe fix on 2026-10-05. Soak 37343966570 runs the guardian for the first time. Also open: the TASK-020-1 verification status and the system-integration main promotion revision of TASK-020-3.

---

### EPIC-019: Casper Node Observation Interface

```yaml
---
epic_id: EPIC-019
title: "Casper Node Observation Interface"
status: in_progress
priority: p0
user_story: US-006
blocked_by: []
created_at: 2026-09-21
updated_at: 2026-09-22
claimed_by: claude-session-7015f552
claimed_at: 2026-09-21T16:40:00Z
branch: feature/casper-node-observation
pull_request: 447
pr_base_branch: dev
consumer: "EPIC-017 TASK-017-12 on formal/soak-casper-consensus. PR #436 temporarily targets this branch."
plans:
  - docs/plans/casper-node-observation-batch-b.md
claims:
  - docs/claims/casper-node-observation.md
  - docs/claims/casper-node-authority-snapshot.md
execution_contract:
  base_branch: dev
  base_revision: 6940a5beb4aa806d3d75f6df3be9f238512fcc2f
  scope: "Deliver the node-side observation interfaces that the soak harness consumes: local capability interface, bounded detached DAG capture, observer evaluation, and publication controls. The node remains the system under test."
  batch_policy: "Each batch requires its own file-scope confirmation before implementation. Confirmation of one batch does not authorize the next."
  git_policy: "Do not merge, push, or create a PR without separate user authorization. Commits require /quick-commit consent."
  evidence_policy: "Every batch registers a pending claim before implementation, keeps compact records under docs/cbc-evidence/, and keeps bulk evidence outside Git."
  completion_policy: "Close after every batch claim is accepted, PR #447 merges to dev, and PR #436 returns to dev."
pending_record_refresh:
  recorded_by: claude-session-f3cbc961
  recorded_on: 2026-09-29
  decided_by: maintainer
  cause: "This branch received the two CI corrections of formal/soak-casper-consensus, from commits b96aebf84 and 38e576041. PR #447 failed Lint and Node observation binding tests without them."
  changed_artifacts:
    - .github/workflows/slashing-tests.yml
    - scripts/ci/check-node-observation-bindings.sh
  new_artifacts:
    - scripts/ci/test-check-node-observation-bindings.sh
  state: "The evidence records of the two changed artifacts hold the digests from before the corrections. Their recorded acceptance applies to those earlier bytes. No evidence record and no claim status changed."
  owner: "The owner of this epic refreshes the records in the next verification cycle."
branch_completion_plan:
  decided_by: user
  decided_on: 2026-09-30
  recorded_by: claude-session-f3cbc961
  decision: "Finish the work on this branch before the stack merge round continues above it. Branch 1 (fix/node-log-and-accept-backoff at d673a5cf2) is merged into this branch at 7cdfee6b7 and pushed. The merges into formal/soak-casper-consensus and above wait."
  order:
    - "TASK-019-9 (Batch D): tracker record, pending claim, file-scope confirmation, implementation, tests, evidence record."
    - "TASK-019-10 (Batch E): the same sequence, after TASK-019-9."
    - "pending_record_refresh: refresh the affected mandatory records against the final Batch D and Batch E sources. The final inventory determines the count."
    - "Proceed with upward stack preparation only with separate Git authorization."
    - "TASK-019-8: finish final cleanup on branch 4, fix/soak-finalization-attribution (PR #441), after it inherits the final stack sources."
    - "TASK-019-6 retains its scope review, cleanup dependency, accepted-claim gates, passing-check requirements, and separate merge authorization."
  cleanup_amendment_2026_09_30:
    decided_by: user
    recorded_by: pi-node-observation-agent-b
    approved_now: "Compact the CI report without changing its parsed JSON data. Shorten repeated work-log content. Keep the required files."
    final_cleanup_branch: fix/soak-finalization-attribution
    final_cleanup_pull_request: 441
    stack_order: [451, 447, 436, 441]
    deferral_scope: "Final file reduction and its inventory review move to branch 4. Task status, claim acceptance, retention rules, and Git authorization remain unchanged."
    reduction_log: docs/work-logs/node-observation-agent-b-20260930.md
  open_inputs:
    - "Owners on 2026-09-30: agent A (claude-session-f3cbc961) has TASK-019-9 and the TASK-019-8 removals. Agent B (the pi session, docs/work-logs/node-observation-agent-b-20260930.md) has the record refresh, the STE fix, the TASK-019-8 inventory, and TASK-019-10. claude-session-7015f552 keeps the epic claim."
    - "File-scope confirmation of Batch D, then of Batch E."
    - "One STE finding from branch 1 in docs/User-Flows.md: a paragraph with 7 sentences."
tasks:
  - id: TASK-019-1
    title: "Batch A: local capability interface and runtime shutdown correction"
    status: complete
    claimed_by: pi-casper-node-observation
    completed_at: 2026-09-19
    claims: [CLAIM-CASPER-NODE-OBSERVATION-001]
    revisions: [877cea722, d021a1d53, 799e2136a]
    evidence:
      - docs/cbc-evidence/runs/casper-node-observer-batch-a-877cea722-01/report.json
      - docs/cbc-evidence/runs/casper-node-observer-shutdown-d021a1d53-01/report.json
    work_log: docs/work-logs/casper-node-observer-shutdown-review.md
    notes:
      - "Opt-in local socket observer with session identity, bounded frames, peer credentials, and capability reporting. All five profile capabilities report unsupported."
      - "The shutdown correction returns the node-program result before observer cleanup. The regression checks source ordering, not an end-to-end node shutdown."
      - "Implementation is complete. The claim remains pending until source-bound verification and explicit acceptance under TASK-019-4."
  - id: TASK-019-2
    title: "Batch B1: bounded detached DAG capture"
    status: complete
    completed_on: "2026-09-23"
    claimed_by: claude-session-7015f552
    claimed_at: 2026-09-21T14:00:00Z
    claims: [CLAIM-CASPER-NODE-OBSERVATION-002]
    revisions: [a38d44185, 38d083bff, 619882128]
    evidence:
      - docs/cbc-evidence/runs/casper-node-snapshot-batch-b1-799e2136a-01/report.json
      - docs/cbc-evidence/runs/casper-node-snapshot-hardening-2ccc4ae0a-01/report.json
      - docs/cbc-evidence/runs/casper-node-snapshot-completion-619882128-01/report.json
    work_log: docs/work-logs/casper-node-observation-batch-b1.md
    blocked_by: []
    files:
      - shared/src/rust/store/soak_snapshot.rs
      - shared/src/rust/store/mod.rs
      - shared/tests/soak_snapshot.rs
      - block-storage/src/rust/dag/soak_snapshot.rs
      - block-storage/src/rust/dag/mod.rs
      - block-storage/src/rust/dag/block_dag_key_value_storage.rs
      - block-storage/src/rust/dag/block_metadata_store.rs
      - block-storage/src/rust/key_value_block_store.rs
      - block-storage/tests/soak_snapshot.rs
    notes:
      - "The user confirmed the nine-file scope, the pending claim, and four high-weight mandatory tags on 2026-09-21."
      - "Bounded LMDB reader, transaction identity checks, detached snapshot with canonical digest, scratch construction, and observer-only bounded block decoding landed in a38d44185."
      - "The continuation at 38d083bff corrected reader budget retention, key bounds, checked arithmetic, and short-decompression acceptance, with red tests retained."
      - "The completion review addresses all four source findings. It records 301 passing test executions and a negative compile check for mutable snapshot access."
    remaining_work: []
    acceptance_record: https://github.com/F1R3FLY-io/f1r3node-rust/pull/447#pullrequestreview-5294038948
    acceptance:
      - "Every limit is checked before the allocation or store operation it bounds."
      - "Capture rejects any environment or generation change between open and validation, including restored values."
      - "Production store bytes are unchanged by successful and rejected captures."
      - "Two scratch views share no mutable store with each other or with production."
      - "The canonical identity covers every field that scratch construction and evaluation consume."
  - id: TASK-019-3
    title: "Batch B2: observer handle, detached evaluation, and reference comparison"
    status: complete
    completed_on: "2026-09-23"
    claimed_by: codex-batch-b2-20260923
    blocked_by: []
    plan: docs/plans/casper-node-observation-batch-b.md
    planning_status: "Steps 1 through 4 complete at cef1f4b721b8109019459f11c53d49df00eb68f9."
    planning_authorization: "The user authorized planning steps 1 through 4 on 2026-09-23, before TASK-019-4 acceptance."
    implementation_authorized: true
    implementation_authorization: "The user requested B2 completion on 2026-09-23. PR #447 review 5294038948 records predecessor acceptance at 4c0c0dbe7."
    claim: docs/claims/casper-node-authority-evaluation.md
    work_log: docs/work-logs/task-019-3-node-authority-evaluation.md
    implementation_status: complete
    verification_package: docs/cbc-evidence/runs/casper-node-authority-b2-d11acabcb-01/report.json
    verification_status: "976 focused tests and all commit checks pass. The full Casper run exceeded 30 minutes."
    completion_gate: "Complete. The named maintainer accepted claim 003 on 2026-09-23 at 237e43d72."
    accepted_by: jltatbeach
    acceptance_record: https://github.com/F1R3FLY-io/f1r3node-rust/pull/447#pullrequestreview-5294038948
    acceptance_revision: 237e43d723b9867985cd47fdfe312fd8d06352e8
    acceptance_package: docs/cbc-evidence/runs/casper-node-authority-b2-d69e12151-02
    construction_cycle: batch-b2-construction-01
    construction_project: formal/rocq/node_authority
    construction_theorems: 15
    applicability_review: formal/tlaplus/node_observation/README.md#batch-b2-applicability-review
    tiers_reached:
      refutation: "Inherited from the accepted session and capture models only. B2 adds no bounded model."
      construction: "15 kernel-checked theorems with closed assumption sets; complete for C3 and C5, partial for C4, C9, and C12, inherited for C7 and C8, pending for C2, C6, C10, C11, C14, and C15."
      binding: "Every property maps to named tests in the bindings manifest; 14 casper observer tests, 20 node observer tests, and 28 capture tests pass."
    strict_cbc_result: "Exit 4 with 13 pending mandatory records."
    prerequisites:
      - "Both predecessor claims must pass source-bound verification and named maintainer acceptance before B2 acceptance."
      - "A final file list covering the runtime, engine cell, Casper constructor, dispatch, and the six test fixtures that need the observer field."
      - "A reference evaluation path that differs from the measured path. Repeating the production tips computation is not independent coverage."
      - "Separate fields for the exact oracle decision, the original fault-tolerance result, and the display projection, each naming its input snapshot."
      - "Counters that increment at actual traversal and clique-search operations, with missing counters reported as unavailable."
      - "A registered pending claim and ratified tags before implementation."
    acceptance:
      - "Attachment installs no observer state by default and never invokes the finalizer, the production snapshot, or validator identity."
      - "An instance rejects conflicting handle attachment. Coverage begins at successful attachment."
      - "Record overflow or observer failure never blocks consensus or invents a successful observation."
      - "A derivation, an attempted effect, and persisted finalization are distinct records."
    implementation_plan:
      - "Step 1. Fix the final file list in the batch plan: node runtime and setup, engine cell, Casper constructor, multi-parent types and dispatch, the six test fixtures that need the observer field, and a work-bound review of util/clique.rs and the traversal helpers."
      - "Step 2. Design the handle: an optional observer field on the Casper instance that defaults to none, attached once at engine installation, rejecting a second attachment, with coverage starting at successful attachment."
      - "Step 3. Design the evaluation: an authority-snapshot request that runs the B1 capture, then evaluates floor and oracle over the scratch view with scratch stores only. The reference path must not reuse the production tips computation."
      - "Step 4. Define the record schema: oracle decision, original fault-tolerance result, and display projection as separate fields, each naming the snapshot digest it used, plus traversal and clique counters that report unavailable when not incremented."
      - "Step 5. Register CLAIM-CASPER-NODE-OBSERVATION-003, propose mandatory tags for the new handle and evaluation files, and create pending records before any implementation."
      - "Step 6. Implement with tests: default startup installs nothing, conflicting attachment is rejected, record overflow never blocks consensus, and derivation, attempted effect, and persisted finalization are distinct."
      - "Step 7. Package evidence, rerun the gate, and obtain acceptance under the same rules as TASK-019-4."
  - id: TASK-019-4
    title: "Source-bound verification and acceptance of the Batch A and Batch B1 claims"
    status: complete
    completed_on: "2026-09-23"
    claimed_by: claude-session-7015f552
    claimed_at: 2026-09-23T17:10:00Z
    previous_claimed_by: pi-session-01a0afde-d35c-70a2-b8c9-39aa11cbdfca
    previous_claimed_at: 2026-09-22T14:15:47Z
    handoff_revision: 8789c1c3e
    blocked_by: []
    execution_revision: 6ea6bf029dc57caf1e5fb512a0eba88a846e959a
    correction_checkout_base: 4561e064a70b495fe07cbcf779bff375d636aaad
    correction_working_tree: true
    correction_source_manifest: docs/cbc-evidence/runs/casper-node-deadline-correction-4561e064a-01/sources.sha256
    work_log: docs/work-logs/task-019-4-node-claim-verification.md
    evidence:
      - docs/cbc-evidence/runs/casper-node-claim-gate-6ea6bf029-01/report.json
      - docs/cbc-evidence/runs/casper-node-deadline-correction-4561e064a-01/report.json
      - docs/cbc-evidence/runs/casper-node-challenge-freshness-3b1d2465a-01/report.json
      - docs/cbc-evidence/runs/casper-node-model-reconciliation-10e7b8452-01/report.json
      - docs/cbc-evidence/runs/casper-node-claim-gate-03d7f1b27-01/report.json
      - docs/cbc-evidence/runs/casper-node-claim-gate-78d696ea6-01/report.json
      - docs/cbc-evidence/runs/casper-node-claim-gate-00f91ca11-01/report.json
      - docs/cbc-evidence/runs/casper-node-claim-gate-38e576041-01/report.json
    verification_scope_confirmed: true
    verification_cycle: combined-b11-cycle-03
    merged_source_cycle_checkout_base: 38e576041
    merged_source_cycle_working_tree: true
    sibling_cycle_package: docs/cbc-evidence/runs/casper-node-claim-gate-78d696ea6-01/report.json
    sibling_cycle_branch: feature/casper-node-observation
    reconciliation_checkout_base: 10e7b8452824e12a1fe2743dca7989b79fce2133
    reconciliation_working_tree: true
    handoff_cycle_checkout_base: 8789c1c3e
    handoff_cycle_working_tree: true
    handoff_cycle_02_checkout_base: 78d696ea6
    handoff_cycle_02_working_tree: true
    tiers_reached:
      refutation: "The local bounded gate passes 16 configurations and 88 controls. Hosted run 35906410283 passes 31 configurations and the same controls."
      construction: "All 33 node theorem exports have closed assumption sets and pass kernel checking. B11 contributes eight byte-schema exports. Boundary assumptions require review."
      binding: "The hosted and ARM64 Linux drivers each pass 334 test executions, including two B11 export tests. Eight current Kani harnesses pass."
    applicability_review: formal/tlaplus/node_observation/README.md#applicability-per-property
    claims: [CLAIM-CASPER-NODE-OBSERVATION-001, CLAIM-CASPER-NODE-OBSERVATION-002]
    eligible_maintainers: [spreston8, dylon, metaweta, jeffrey-l-turner, jltatbeach]
    proposed_reviewer: jltatbeach
    accepted_by: jltatbeach
    acceptance_record: https://github.com/F1R3FLY-io/f1r3node-rust/pull/447#pullrequestreview-5294038948
    acceptance_revision: 4c0c0dbe7c8958debefdb02f2b21795786c45900
    acceptance_reviewed_at: 2026-09-23T16:55:28Z
    acceptance_package: docs/cbc-evidence/runs/casper-node-claim-gate-78d696ea6-01
    previous_verification_cycle: handoff-cycle-02
    handoff_cycle_02_tiers_reached:
      refutation: "15 clean configurations and 78 expected violations through the TLA gate at the pinned jar; the two node models and 17 controls pass on this revision."
      construction: "14 kernel-checked theorems with closed assumption sets in formal/rocq/node_observation; construction pending for A3, A4, A9, B9, B11, B12 and the deadline parts of A7 and B2."
      binding: "Capture oracle over 14 scenarios, session oracle, retained pre-fix regressions, seven Kani harnesses, and the deterministic generation-rejection test that closes the B8 binding gap."
    notes:
      - "The user placed this gate before B2 planning. B2 is not a prerequisite for verification of Batch A and B1."
      - "The implementation input is available at the execution revision. TASK-019-2 remains open for this acceptance review, not as a circular prerequisite."
      - "Maintainer identities do not constitute acceptance. Approval must name the reviewed revision, both claims, and the evidence package."
      - "The initial intake had seven pending mandatory artifacts and no observer-specific formal inputs. The confirmed verification scope adds formal and gate artifacts."
      - "The initial package retains the lock-deadline counterexample and the interface fixture failure."
      - "The user approved the five-file correction. All three capture locks use a checked deadline, and the test hashes its executable before the handshake."
      - "The corrected isolated run passed 577 test executions, strict Clippy, the workspace check, and formatting checks. The interface executable has a separate debug-stripped identity."
      - "Production limits remain unchanged. The repeated-nonce regression failed before the correction and passed afterward. The new isolated run passed 580 test executions."
      - "TLC passed the challenge model and its expected freshness violation. Four allocation theorems passed Rocq kernel checking with closed assumption sets."
      - "The bounded TLC gate passed 14 positive configurations and 62 expected violations. These results do not discharge either combined claim."
      - "The canonical model set combines the broader downstream models with the node freshness control. Both claim records remain pending."
      - "The reconciliation gate passed 15 positive configurations and 78 expected violations. All 17 node controls remain registered, with four closed Rocq assumption sets."
      - "The isolated binding driver passed 276 executions. Its input base is node revision 10e7b8452, not a harness merge revision."
      - "The node package owns gate registration evidence. The harness keeps its primary gate record without node claim digests."
      - "The handoff cycle at 8789c1c3e added the BoundedCapture Rocq module and qualified tokens, a capture bisimilarity oracle, Kani harnesses, the applicability review, and refreshed records including the DAG storage file after the dev merge."
      - "Both claims remain pending. Acceptance needs named maintainer review of every applicability decision and the remaining construction evidence."
      - "No standalone checker crate was added. Downstream must remove its checker crate or include it in the supply-chain audit."
      - "Cross-incarnation identity, remaining construction proofs, Kani harnesses, complete Rust correspondence, and named maintainer acceptance remain pending."
      - "Batch B2 changed six accepted artifacts after acceptance: the TLA+ README, the DAG storage file, the capture tests, the node observer, its tests, and the formal gate script. Their records are pending for claim 003 at the current digest and retain the acceptance of claims 001 and 002 at 4c0c0dbe7 with the accepted digests. The other 52 accepted records stay discharged."
      - "The named maintainer accepted claim 003 on 2026-09-23 at 237e43d72 on the construction-cycle package. The six changed artifacts are discharged again under claim 003 and keep both acceptances in their records."
      - "The named maintainer accepted both claims on 2026-09-23 at 4c0c0dbe7 on the cycle 02 package, with the five bounded-by-design decisions and the recorded construction gaps accepted. All 58 node records are discharged. The two gate-claim records in the union inventory stay pending under CLAIM-SOAK-GATE-001."
      - "Handoff cycle 02 at 78d696ea6 pulled the deterministic generation-rejection test across from the soak branch, reran the TLA gate and the Rocq kernel check, and refreshed every node record. Three blocking items remain: the applicability review, the acceptance, and the open construction proofs."
      - "The merged-source cycle 02 at 00f91ca11 refreshed every node record and the package after the port of the withheld cycle. The node branch refreshed its records at 78d696ea6 in parallel; that package is not merged here."
      - "Cycle 02 retained the hosted strip failure. Commit 38e576041 corrects array entry-size normalization and preserves the other allocated-section checks."
      - "Cycle 03 verifies the current sources with local Rocq, retained ARM64 Kani tooling, and successful hosted binding and correspondence jobs."
      - "The current package refreshes 68 node records and preserves two primary governance records. The strict audit keeps all 70 mandatory records pending."
      - "B11 correspondence checks 75 production wire cases and six rejection controls. The strip fixture rejects 12 executable mutations."
    acceptance:
      - "Strict source-bound audits pass for every artifact in both claim inventories at the accepted revision."
      - "Refutation, construction, and binding tiers are recorded with retained failing controls."
      - "An isolated rebuild runs the interface and capture suites. The Batch A and B1 reports record native runs only."
      - "Acceptance is recorded by a named maintainer. Passing tests alone do not discharge a claim."
    implementation_plan:
      - "Step 1. Refutation tier. Author bounded TLA+ models under formal/tlaplus/node_observation/: MC_ObserverSession for challenge freshness, one request per session, deadline expiry, and session budget; MC_BoundedCapture for guard order, transaction-identity interval, environment-change rejection, incomplete-row rejection, and guard release. Register each clean configuration and one expected-violation configuration per defect class in scripts/ci/check-tla-invariants.sh. The clean run must pass and each violation must fail on its named invariant with TLC exit 12. Record the instance bounds in the area README. This tier yields bounded refutation evidence only."
      - "Step 2. Applicability review per property. For each property in both claims, record in the area README whether it is bounded by design or unbounded, following docs/cbc-verification-tiers.md. A resource limit, timeout, frame size, or test fixture does not make a property bounded by design, and a smaller TLC instance does not cover a larger permitted domain. Construction may be recorded as not applicable only after a named maintainer accepts the bounded-by-design justification with evidence that verification covers the complete permitted domain. Capture consistency over every permitted DAG and session isolation over every request are unbounded and require promotion. A claim closes only when every constituent property has a reviewed classification."
      - "Step 3. Construction tier. For every promoted property, add a Rocq project under formal/rocq/node_observation/ that exports one MainTheorem module, register it in scripts/ci/check-formal-invariants.sh, and require a clean coqchk run with a counted assumption set that contains no Axiom, Admitted, or Parameter. Until the theorem lands, every record keeps construction pending. Passing TLC checks never satisfy this tier."
      - "Step 4. Binding tier. Source hashes establish identity only. Bind the Rust to the models with a bisimilarity test that runs the production capture and observer paths against a hand-translated oracle on the same inputs, keep the retained pre-fix regressions for the five corrected findings, and add a Kani harness for the length-prefix and block-decode limit arithmetic. Record which binding forms each claim reached."
      - "Step 5. Gate rerun. Rerun the gate at the corrected revision de93425ee or later with an isolated rebuild and the strict explicit-inventory audit. Package it as casper-node-claim-gate-<revision>-02 with the tiers reached, the construction field, and construction_assumptions for each theorem."
      - "Step 6. Acceptance. Request acceptance from the proposed reviewer as a PR #447 review comment that names the revision, both claim IDs, and the package path. Record accepted_by and acceptance_record."
      - "Step 7. Status follows evidence. Change a tier field only when its evidence exists, and change a claim status only when every required tier is verified and acceptance is recorded. The audit result follows the statuses. Never change a status to make the audit pass. A claim with construction still pending stays pending, with the promotion decision recorded."
  - id: TASK-019-5
    title: "Batch C: publication writer inventory and consistency contract"
    status: complete
    claimed_by: null
    completed_on: "2026-09-23"
    completion_scope: research_only
    research: docs/plans/casper-node-observation-batch-c.md
    research_authorization: "The user requested Batch C research before TASK-019-3 completion."
    research_results:
      - "The inventory covers all 15 block and DAG databases, adjacent custody stores, dependency storage, and runtime state."
      - "The contract separates read consistency, partial publication, recovery evidence, and unsupported occurrence identity."
    review_status: pending_named_maintainer
    implementation_authorized: false
    implementation_dependencies: [TASK-019-3]
    qualification_prerequisites:
      - "PR #216 merged to dev. Occurrence-level recovery qualification has no occurrence store to observe before that merge."
    acceptance:
      - "Occurrence identity is never inferred from deploy signatures."
      - "The contract and inventory are reviewed before any Batch C implementation approval."
  - id: TASK-019-6
    title: "Review minimum necessary scope, merge PR #447 to dev, and return PR #436 to dev"
    status: pending
    claimed_by: null
    blocked_by: [TASK-019-4, TASK-019-8]
    acceptance:
      - "Review every branch change before merge and justify why each retained component is necessary for an approved requirement."
      - "Identify components that can be removed, combined, or moved into test tooling instead of the production node."
      - "Review duplicate data, custom serialization, scratch stores, public interfaces, configuration, tests, documentation, and evidence files."
      - "Preserve required safety checks, negative tests, evidence identities, and reachable historical evidence during any approved reduction."
      - "Record source, test, documentation, and total diff sizes before and after reduction."
      - "Run affected checks and obtain maintainer acceptance of the minimum necessary codebase scope before merge."
      - "PR #447 merges with all accepted claims and the pre-commit gate passing without a skip."
      - "PR #436 on formal/soak-casper-consensus retargets dev after the merge."
      - "EPIC-017 TASK-017-12 updates its node interface status to the merged revision."
  - id: TASK-019-7
    title: "Publish dev candidate images for adapter qualification"
    status: in_progress
    claimed_by: codex-candidate-images-20260923
    blocked_by: [TASK-019-6]
    work_log: docs/work-logs/task-019-7-candidate-images.md
    baseline_status: "Published amd64 and arm64 digests verified for dev revision 6d6d4fed6f84baa0913d8e87f32a7ffe2e0ca59a."
    baseline_executable_status: "Both executable hashes, image configurations, platform manifests, and the index were independently verified by immutable digest."
    baseline_handoff: docs/work-logs/task-019-7-candidate-images.md#task-017-12-candidate-handoff
    review_evidence: target/task-019-7-review-20260923/report.json
    lifecycle_fix_pr: https://github.com/F1R3FLY-io/system-integration/pull/144
    lifecycle_dev_merge: ef9844893f19df3e7523bb97e9e0da0ca241bb10
    lifecycle_promotion_pr: https://github.com/F1R3FLY-io/system-integration/pull/145
    lifecycle_promotion_status: "Merged to main on 2026-09-23. The merged tree matches the reviewed and tested tree."
    lifecycle_main_merge: e3c4e14189f0c6ced2e9674487fcbdeffd93141b
    lifecycle_review_status: "Independent review found no blocking code issue. All 42 lifecycle and resolver tests pass."
    lifecycle_live_status: "Passed on amd64 and arm64 with Docker and subprocess providers. Each suite passed all 110 tests."
    lifecycle_live_evidence: target/task-019-7-pin-e3c4e1418/ci/live-validation.json
    node_pin_status: "All three pins name e3c4e141. Local checks and PR CI pass."
    node_pin_evidence: target/task-019-7-pin-e3c4e1418/report.json
    node_pin_patch: target/task-019-7-pin-e3c4e1418/node-pin.patch
    node_pin_branch: ci/repin-validator-lifecycle-settlement
    node_pin_publication: "PR #450 remains open at the user's direction. CI run 35921674172 passed."
    node_pin_authorization: "The agent created the commit and PR without required explicit approval. No merge authorization exists."
    node_pin_pr: https://github.com/F1R3FLY-io/f1r3node-rust/pull/450
    node_pin_commit: 6497dd76a029481d63e49c2af6f0f91c4bd71fe2
    node_pin_ci_run: 35921674172
    observer_candidate_status: "Pending TASK-019-6 and subsequent dev CI publication."
    external_dependencies:
      - "The system-integration fix, promotion, and live validation are complete. The node PR merge and subsequent dev image publication remain pending."
    proposed_external_branch: fix/validator-lifecycle-settlement-budget
    external_pr_target: dev
    external_promotion_target: main
    consumer: "EPIC-017 TASK-017-12 candidate repin on formal/soak-casper-consensus."
    pin_sites:
      - .github/oci-validation.env
      - .github/workflows/_integration-pipeline.yml
      - .github/workflows/merge-recovery-soak.yml
    implementation_plan:
      - "Step 1. In system-integration, target dev, then promote dev to main. Record both merge revisions. Commits and pushes require separate consent."
      - "Step 2. Use scripts/repin-system-integration.sh to align all three SYSTEM_INTEGRATION_REF sites in a small PR to dev. Update the soak branch separately."
      - "Step 3. Let dev CI publish the image for the current dev revision, and record the immutable image digests for amd64 and arm64 with the dev revision they were built from."
      - "Step 4. Hand the digests to TASK-017-12 for candidate repin and admission. This baseline image carries no observer interface."
      - "Step 5. After TASK-019-6 merges PR #447, repeat Step 3 for the observer-capable dev revision. Authority and publication adapter qualification needs that second image, not the baseline."
    acceptance:
      - "The pin names the 40-character system-integration main revision that contains the promoted fix."
      - "Each published image is recorded by immutable digest, platform, and source dev revision."
      - "The baseline and observer-capable images are recorded as distinct candidates."
      - "No candidate is repinned from a rebuilt or mutable tag."
  - id: TASK-019-8
    title: "Final node-observation cleanup on stack branch 4"
    previous_title: "Final branch cleanup before the PR #447 merge"
    status: in_progress
    claimed_by: claude-session-7015f552
    claimed_at: 2026-09-23T21:40:00Z
    work_log: docs/work-logs/task-019-8-branch-cleanup.md
    execution_branch: fix/soak-finalization-attribution
    execution_pull_request: 441
    scheduling_status: deferred_to_stack_branch_4
    scheduling_decided_by: user
    scheduling_decided_on: 2026-09-30
    scheduling_reason: "The user approved report and work-log compaction now, with final cleanup on branch 4 after stack inheritance."
    final_inventory_required: true
    limited_reduction:
      report: docs/cbc-evidence/runs/node-observation-ci-refresh-20260930-01/report.json
      lines_before: 511
      lines_after: 242
      parsed_data_unchanged: true
      required_files_removed: 0
      work_log: docs/work-logs/node-observation-agent-b-20260930.md
    blocked_by: []
    precedes: [TASK-019-6]
    removals_2026_09_30:
      authorized_by: user
      authorized_on: 2026-09-30
      removed_by: claude-session-f3cbc961
      base_revision: 97ab3e3d9
      reason: uncited_checksum
      history: "The content stays in Git history at 97ab3e3d9 and earlier. The digest of each file is below and in inventory.proposed_removals."
      files:
        - {path: docs/cbc-evidence/runs/casper-node-authority-b2-d11acabcb-01/artifacts.sha256, sha256: 35eebe6533eec1ef9dc52ec45d380f4c73c457d4daa64305b67642e61567e673}
        - {path: docs/cbc-evidence/runs/casper-node-claim-gate-03d7f1b27-01/artifacts.sha256, sha256: a12319c726123e2be0014a6cb8847d1d350c4202a44eda2825afd2bde1606929}
        - {path: docs/cbc-evidence/runs/casper-node-observer-batch-a-877cea722-01/artifacts.sha256, sha256: 21ad765308be31beb8ac8fc6e747b0a1b122c1d797a6a0fc21e867710e963adb}
        - {path: docs/cbc-evidence/runs/casper-node-observer-shutdown-d021a1d53-01/artifacts.sha256, sha256: 8ffa6cc12210ee0fe5c47f797ad4b803c77e982a605fa34e7b4b97931a7e9023}
        - {path: docs/cbc-evidence/runs/casper-node-snapshot-batch-b1-799e2136a-01/artifacts.sha256, sha256: ce5315cce7c4070d01cd250f0b1ecd01a8a7ced7edc83ef973f341133944e5e7}
    scope: "Remove discovery notes, work logs, plans, and CbC evidence files that are not integral to the branch's functionality or its accepted claims. Production code scope is reviewed under TASK-019-6, not here."
    retention_rules:
      - "Keep every file that an accepted claim, a CbC record, or the tracker cites by path or digest. Removing one breaks the source-bound audit."
      - "Keep the claim files, the per-artifact records, and one compact report.json plus validation.json per evidence run that a record cites."
      - "Keep the acceptance record and the handoff notes that name decisions, owners, and open findings."
      - "Bulk evidence stays outside Git. Anything that leaves the tree is recorded with its external location and digest, following the TASK-017-14 precedent on the soak branch."
    inventory_status: refreshed
    inventory_completed_on: "2026-09-23"
    inventory_refreshed_on: "2026-09-23"
    removals_authorized: false
    cleanup_acceptance: pending
    before_checks:
      strict_audit_claims_001_002: "exit 4; 58 discharged, 2 pending under CLAIM-SOAK-GATE-001"
      strict_audit_claim_003: "exit 0; 19 discharged"
      link_check: "lychee offline over docs and formal: 2188 links, 0 errors"
      ste_check: "baseline recorded for every branch Markdown file"
    diff_before:
      total: {files: 239, added: 24288, deleted: 323}
      docs: {files: 135, added: 12047, deleted: 13}
      formal: {files: 53, added: 2746, deleted: 0}
      tests: {files: 10, added: 3513, deleted: 0}
      workflows_and_scripts: {files: 8, added: 310, deleted: 17}
      source: {files: 33, added: 5672, deleted: 293}
    inventory:
      head: 51febc379838b6383e240e917f7be1ed9c01b9ab
      dev: 6d6d4fed6f84baa0913d8e87f32a7ffe2e0ca59a
      comparison: "git diff --name-status dev...HEAD -- docs formal .github"
      merge_base_equals_dev: true
      total_files: 192
      classifications: {"integral": 138, "removable": 5, "cited": 49}
      method:
        - "The inventory covers every committed branch path in the three directories at the refreshed head. No working-tree-only file remains."
        - "Integral files support required functionality, verification, policy, an accepted claim, or an accepted package."
        - "Cited files retain an inbound path or digest reference from a claim, record, task, handoff, plan, or package, with the TASK-019-8 inventory excluded from the reference corpus."
        - "Every report.json and validation.json companion is retained by the retention rule whether or not a path cites it."
        - "The two packages named in the maintainer acceptances keep their complete manifests. Removing a file from an accepted package would change the accepted package."
        - "Companion manifests inside a package that a claim, record, or report cites by directory are retained as package members."
        - "Removable files are the self-manifests of packages that no acceptance names. Their content is reproducible from the retained files, and their digests are recorded below."
        - "Work logs are one per task. Each is cited by the tracker, a claim, or a plan, so none is removable under the retention rules."
      reasons:
        workflow_gate: "The workflow runs node binding checks and the registered formal gate."
        task_record: "The tracker retains task scope, dependencies, review gates, and the cleanup decision."
        research_terms: "The glossary defines the terms used by the Batch C contract."
        tier_policy: "The claim review requires the property classification and tier rules."
        claim_specification: "The file defines an observation claim and its required evidence."
        formal_verification: "The claim verification uses this model, control, theorem, project input, binding map, or applicability record."
        artifact_record: "The source-bound audit requires the per-artifact CbC record."
        task_plan: "TASK-019-3 or TASK-019-5 cites this research and implementation boundary."
        task_handoff: "The tracker, a claim, or a plan cites this handoff, its decisions, or its open findings."
        retained_report: "A claim, per-artifact record, task, or retained handoff cites this report."
        retained_validation: "The retention rule requires the validation companion for this cited report."
        accepted_package_manifest: "A maintainer acceptance names this package. Its manifest stays so the accepted package remains verifiable."
        validation_digest: "The package validation records this checksum file digest."
        package_member: "The package directory is cited by a claim, record, or report, and this companion is part of the cited package."
        uncited_checksum: "No acceptance names this package and no path or digest cites this self-manifest. The retained report and companions carry the same identities."
      proposed_removals:
        - {"path": "docs/cbc-evidence/runs/casper-node-authority-b2-d11acabcb-01/artifacts.sha256", "sha256": "35eebe6533eec1ef9dc52ec45d380f4c73c457d4daa64305b67642e61567e673", "reason": "uncited_checksum"}
        - {"path": "docs/cbc-evidence/runs/casper-node-claim-gate-03d7f1b27-01/artifacts.sha256", "sha256": "a12319c726123e2be0014a6cb8847d1d350c4202a44eda2825afd2bde1606929", "reason": "uncited_checksum"}
        - {"path": "docs/cbc-evidence/runs/casper-node-observer-batch-a-877cea722-01/artifacts.sha256", "sha256": "21ad765308be31beb8ac8fc6e747b0a1b122c1d797a6a0fc21e867710e963adb", "reason": "uncited_checksum"}
        - {"path": "docs/cbc-evidence/runs/casper-node-observer-shutdown-d021a1d53-01/artifacts.sha256", "sha256": "8ffa6cc12210ee0fe5c47f797ad4b803c77e982a605fa34e7b4b97931a7e9023", "reason": "uncited_checksum"}
        - {"path": "docs/cbc-evidence/runs/casper-node-snapshot-batch-b1-799e2136a-01/artifacts.sha256", "sha256": "ce5315cce7c4070d01cd250f0b1ecd01a8a7ced7edc83ef973f341133944e5e7", "reason": "uncited_checksum"}
      files:
        - {"path": ".github/oci-validation.env", "change": "M", "classification": "integral", "reason": "workflow_gate"}
        - {"path": ".github/workflows/_integration-pipeline.yml", "change": "M", "classification": "integral", "reason": "workflow_gate"}
        - {"path": ".github/workflows/merge-recovery-soak.yml", "change": "M", "classification": "integral", "reason": "workflow_gate"}
        - {"path": ".github/workflows/slashing-tests.yml", "change": "M", "classification": "integral", "reason": "workflow_gate"}
        - {"path": "docs/Glossary.md", "change": "M", "classification": "integral", "reason": "research_terms"}
        - {"path": "docs/ToDos.md", "change": "M", "classification": "integral", "reason": "task_record"}
        - {"path": "docs/cbc-evidence/block-storage-src-rust-dag-block-dag-key-value-storage-rs.md", "change": "A", "classification": "integral", "reason": "artifact_record"}
        - {"path": "docs/cbc-evidence/block-storage-src-rust-dag-soak-snapshot-rs.md", "change": "A", "classification": "integral", "reason": "artifact_record"}
        - {"path": "docs/cbc-evidence/block-storage-tests-soak-snapshot-rs.md", "change": "A", "classification": "integral", "reason": "artifact_record"}
        - {"path": "docs/cbc-evidence/casper-src-rust-finality-floor-rs.md", "change": "A", "classification": "integral", "reason": "artifact_record"}
        - {"path": "docs/cbc-evidence/casper-src-rust-safety-clique-oracle-rs.md", "change": "A", "classification": "integral", "reason": "artifact_record"}
        - {"path": "docs/cbc-evidence/casper-src-rust-soak-observer-evaluation-rs.md", "change": "A", "classification": "integral", "reason": "artifact_record"}
        - {"path": "docs/cbc-evidence/casper-src-rust-soak-observer-reference-rs.md", "change": "A", "classification": "integral", "reason": "artifact_record"}
        - {"path": "docs/cbc-evidence/casper-src-rust-soak-observer-rs.md", "change": "A", "classification": "integral", "reason": "artifact_record"}
        - {"path": "docs/cbc-evidence/casper-src-rust-util-clique-rs.md", "change": "A", "classification": "integral", "reason": "artifact_record"}
        - {"path": "docs/cbc-evidence/casper-tests-soak-observer-rs.md", "change": "A", "classification": "integral", "reason": "artifact_record"}
        - {"path": "docs/cbc-evidence/formal-rocq-node-authority-CoqProject.md", "change": "A", "classification": "integral", "reason": "artifact_record"}
        - {"path": "docs/cbc-evidence/formal-rocq-node-authority-README-md.md", "change": "A", "classification": "integral", "reason": "artifact_record"}
        - {"path": "docs/cbc-evidence/formal-rocq-node-authority-theories-AuthorityObserver-v.md", "change": "A", "classification": "integral", "reason": "artifact_record"}
        - {"path": "docs/cbc-evidence/formal-rocq-node-authority-theories-AuthorityWork-v.md", "change": "A", "classification": "integral", "reason": "artifact_record"}
        - {"path": "docs/cbc-evidence/formal-rocq-node-authority-theories-MainTheorem-v.md", "change": "A", "classification": "integral", "reason": "artifact_record"}
        - {"path": "docs/cbc-evidence/formal-rocq-node-observation-CoqProject.md", "change": "A", "classification": "integral", "reason": "artifact_record"}
        - {"path": "docs/cbc-evidence/formal-rocq-node-observation-README-md.md", "change": "A", "classification": "integral", "reason": "artifact_record"}
        - {"path": "docs/cbc-evidence/formal-rocq-node-observation-theories-BoundedCapture-v.md", "change": "A", "classification": "integral", "reason": "artifact_record"}
        - {"path": "docs/cbc-evidence/formal-rocq-node-observation-theories-MainTheorem-v.md", "change": "A", "classification": "integral", "reason": "artifact_record"}
        - {"path": "docs/cbc-evidence/formal-rocq-node-observation-theories-ObserverSession-v.md", "change": "A", "classification": "integral", "reason": "artifact_record"}
        - {"path": "docs/cbc-evidence/formal-tlaplus-node-observation-BoundedCapture-tla.md", "change": "A", "classification": "integral", "reason": "artifact_record"}
        - {"path": "docs/cbc-evidence/formal-tlaplus-node-observation-MC-BoundedCapture-admission-unsafe-cfg.md", "change": "A", "classification": "integral", "reason": "artifact_record"}
        - {"path": "docs/cbc-evidence/formal-tlaplus-node-observation-MC-BoundedCapture-admission-unsafe-tla.md", "change": "A", "classification": "integral", "reason": "artifact_record"}
        - {"path": "docs/cbc-evidence/formal-tlaplus-node-observation-MC-BoundedCapture-bytes-unsafe-cfg.md", "change": "A", "classification": "integral", "reason": "artifact_record"}
        - {"path": "docs/cbc-evidence/formal-tlaplus-node-observation-MC-BoundedCapture-bytes-unsafe-tla.md", "change": "A", "classification": "integral", "reason": "artifact_record"}
        - {"path": "docs/cbc-evidence/formal-tlaplus-node-observation-MC-BoundedCapture-cfg.md", "change": "A", "classification": "integral", "reason": "artifact_record"}
        - {"path": "docs/cbc-evidence/formal-tlaplus-node-observation-MC-BoundedCapture-deadline-unsafe-cfg.md", "change": "A", "classification": "integral", "reason": "artifact_record"}
        - {"path": "docs/cbc-evidence/formal-tlaplus-node-observation-MC-BoundedCapture-deadline-unsafe-tla.md", "change": "A", "classification": "integral", "reason": "artifact_record"}
        - {"path": "docs/cbc-evidence/formal-tlaplus-node-observation-MC-BoundedCapture-generation-unsafe-cfg.md", "change": "A", "classification": "integral", "reason": "artifact_record"}
        - {"path": "docs/cbc-evidence/formal-tlaplus-node-observation-MC-BoundedCapture-generation-unsafe-tla.md", "change": "A", "classification": "integral", "reason": "artifact_record"}
        - {"path": "docs/cbc-evidence/formal-tlaplus-node-observation-MC-BoundedCapture-incomplete-unsafe-cfg.md", "change": "A", "classification": "integral", "reason": "artifact_record"}
        - {"path": "docs/cbc-evidence/formal-tlaplus-node-observation-MC-BoundedCapture-incomplete-unsafe-tla.md", "change": "A", "classification": "integral", "reason": "artifact_record"}
        - {"path": "docs/cbc-evidence/formal-tlaplus-node-observation-MC-BoundedCapture-open-unsafe-cfg.md", "change": "A", "classification": "integral", "reason": "artifact_record"}
        - {"path": "docs/cbc-evidence/formal-tlaplus-node-observation-MC-BoundedCapture-open-unsafe-tla.md", "change": "A", "classification": "integral", "reason": "artifact_record"}
        - {"path": "docs/cbc-evidence/formal-tlaplus-node-observation-MC-BoundedCapture-order-unsafe-cfg.md", "change": "A", "classification": "integral", "reason": "artifact_record"}
        - {"path": "docs/cbc-evidence/formal-tlaplus-node-observation-MC-BoundedCapture-order-unsafe-tla.md", "change": "A", "classification": "integral", "reason": "artifact_record"}
        - {"path": "docs/cbc-evidence/formal-tlaplus-node-observation-MC-BoundedCapture-release-unsafe-cfg.md", "change": "A", "classification": "integral", "reason": "artifact_record"}
        - {"path": "docs/cbc-evidence/formal-tlaplus-node-observation-MC-BoundedCapture-release-unsafe-tla.md", "change": "A", "classification": "integral", "reason": "artifact_record"}
        - {"path": "docs/cbc-evidence/formal-tlaplus-node-observation-MC-BoundedCapture-tla.md", "change": "A", "classification": "integral", "reason": "artifact_record"}
        - {"path": "docs/cbc-evidence/formal-tlaplus-node-observation-MC-BoundedCapture-validation-unsafe-cfg.md", "change": "A", "classification": "integral", "reason": "artifact_record"}
        - {"path": "docs/cbc-evidence/formal-tlaplus-node-observation-MC-BoundedCapture-validation-unsafe-tla.md", "change": "A", "classification": "integral", "reason": "artifact_record"}
        - {"path": "docs/cbc-evidence/formal-tlaplus-node-observation-MC-BoundedCapture-write-unsafe-cfg.md", "change": "A", "classification": "integral", "reason": "artifact_record"}
        - {"path": "docs/cbc-evidence/formal-tlaplus-node-observation-MC-BoundedCapture-write-unsafe-tla.md", "change": "A", "classification": "integral", "reason": "artifact_record"}
        - {"path": "docs/cbc-evidence/formal-tlaplus-node-observation-MC-ObserverSession-budget-unsafe-cfg.md", "change": "A", "classification": "integral", "reason": "artifact_record"}
        - {"path": "docs/cbc-evidence/formal-tlaplus-node-observation-MC-ObserverSession-budget-unsafe-tla.md", "change": "A", "classification": "integral", "reason": "artifact_record"}
        - {"path": "docs/cbc-evidence/formal-tlaplus-node-observation-MC-ObserverSession-cfg.md", "change": "A", "classification": "integral", "reason": "artifact_record"}
        - {"path": "docs/cbc-evidence/formal-tlaplus-node-observation-MC-ObserverSession-challenge-unsafe-cfg.md", "change": "A", "classification": "integral", "reason": "artifact_record"}
        - {"path": "docs/cbc-evidence/formal-tlaplus-node-observation-MC-ObserverSession-challenge-unsafe-tla.md", "change": "A", "classification": "integral", "reason": "artifact_record"}
        - {"path": "docs/cbc-evidence/formal-tlaplus-node-observation-MC-ObserverSession-deadline-unsafe-cfg.md", "change": "A", "classification": "integral", "reason": "artifact_record"}
        - {"path": "docs/cbc-evidence/formal-tlaplus-node-observation-MC-ObserverSession-deadline-unsafe-tla.md", "change": "A", "classification": "integral", "reason": "artifact_record"}
        - {"path": "docs/cbc-evidence/formal-tlaplus-node-observation-MC-ObserverSession-frame-unsafe-cfg.md", "change": "A", "classification": "integral", "reason": "artifact_record"}
        - {"path": "docs/cbc-evidence/formal-tlaplus-node-observation-MC-ObserverSession-frame-unsafe-tla.md", "change": "A", "classification": "integral", "reason": "artifact_record"}
        - {"path": "docs/cbc-evidence/formal-tlaplus-node-observation-MC-ObserverSession-freshness-pre-fix-cfg.md", "change": "A", "classification": "integral", "reason": "artifact_record"}
        - {"path": "docs/cbc-evidence/formal-tlaplus-node-observation-MC-ObserverSession-freshness-pre-fix-tla.md", "change": "A", "classification": "integral", "reason": "artifact_record"}
        - {"path": "docs/cbc-evidence/formal-tlaplus-node-observation-MC-ObserverSession-identity-unsafe-cfg.md", "change": "A", "classification": "integral", "reason": "artifact_record"}
        - {"path": "docs/cbc-evidence/formal-tlaplus-node-observation-MC-ObserverSession-identity-unsafe-tla.md", "change": "A", "classification": "integral", "reason": "artifact_record"}
        - {"path": "docs/cbc-evidence/formal-tlaplus-node-observation-MC-ObserverSession-repeat-unsafe-cfg.md", "change": "A", "classification": "integral", "reason": "artifact_record"}
        - {"path": "docs/cbc-evidence/formal-tlaplus-node-observation-MC-ObserverSession-repeat-unsafe-tla.md", "change": "A", "classification": "integral", "reason": "artifact_record"}
        - {"path": "docs/cbc-evidence/formal-tlaplus-node-observation-MC-ObserverSession-tla.md", "change": "A", "classification": "integral", "reason": "artifact_record"}
        - {"path": "docs/cbc-evidence/formal-tlaplus-node-observation-ObserverSession-tla.md", "change": "A", "classification": "integral", "reason": "artifact_record"}
        - {"path": "docs/cbc-evidence/formal-tlaplus-node-observation-README-md.md", "change": "A", "classification": "integral", "reason": "artifact_record"}
        - {"path": "docs/cbc-evidence/formal-tlaplus-node-observation-bindings-json.md", "change": "A", "classification": "integral", "reason": "artifact_record"}
        - {"path": "docs/cbc-evidence/formal-tlaplus-node-observation-verification-plan-json.md", "change": "A", "classification": "integral", "reason": "artifact_record"}
        - {"path": "docs/cbc-evidence/github-workflows-slashing-tests-yml.md", "change": "M", "classification": "integral", "reason": "artifact_record"}
        - {"path": "docs/cbc-evidence/node-src-rust-soak-observer-rs.md", "change": "A", "classification": "integral", "reason": "artifact_record"}
        - {"path": "docs/cbc-evidence/node-tests-soak-observer-rs.md", "change": "A", "classification": "integral", "reason": "artifact_record"}
        - {"path": "docs/cbc-evidence/runs/casper-node-authority-b2-d11acabcb-01/artifacts.sha256", "change": "A", "classification": "removable", "reason": "uncited_checksum"}
        - {"path": "docs/cbc-evidence/runs/casper-node-authority-b2-d11acabcb-01/cbc-audit.json", "change": "A", "classification": "cited", "reason": "package_member"}
        - {"path": "docs/cbc-evidence/runs/casper-node-authority-b2-d11acabcb-01/executables.sha256", "change": "A", "classification": "cited", "reason": "package_member"}
        - {"path": "docs/cbc-evidence/runs/casper-node-authority-b2-d11acabcb-01/logs.sha256", "change": "A", "classification": "cited", "reason": "package_member"}
        - {"path": "docs/cbc-evidence/runs/casper-node-authority-b2-d11acabcb-01/report.json", "change": "A", "classification": "cited", "reason": "retained_report"}
        - {"path": "docs/cbc-evidence/runs/casper-node-authority-b2-d11acabcb-01/sources.sha256", "change": "A", "classification": "cited", "reason": "package_member"}
        - {"path": "docs/cbc-evidence/runs/casper-node-authority-b2-d11acabcb-01/validation.json", "change": "A", "classification": "cited", "reason": "retained_validation"}
        - {"path": "docs/cbc-evidence/runs/casper-node-authority-b2-d69e12151-02/artifacts.sha256", "change": "A", "classification": "integral", "reason": "accepted_package_manifest"}
        - {"path": "docs/cbc-evidence/runs/casper-node-authority-b2-d69e12151-02/report.json", "change": "A", "classification": "cited", "reason": "retained_report"}
        - {"path": "docs/cbc-evidence/runs/casper-node-authority-b2-d69e12151-02/sources.sha256", "change": "A", "classification": "cited", "reason": "package_member"}
        - {"path": "docs/cbc-evidence/runs/casper-node-authority-b2-d69e12151-02/validation.json", "change": "A", "classification": "cited", "reason": "retained_validation"}
        - {"path": "docs/cbc-evidence/runs/casper-node-challenge-freshness-3b1d2465a-01/report.json", "change": "A", "classification": "cited", "reason": "retained_report"}
        - {"path": "docs/cbc-evidence/runs/casper-node-challenge-freshness-3b1d2465a-01/sources.sha256", "change": "A", "classification": "cited", "reason": "package_member"}
        - {"path": "docs/cbc-evidence/runs/casper-node-claim-gate-03d7f1b27-01/artifacts.sha256", "change": "A", "classification": "removable", "reason": "uncited_checksum"}
        - {"path": "docs/cbc-evidence/runs/casper-node-claim-gate-03d7f1b27-01/report.json", "change": "A", "classification": "cited", "reason": "retained_report"}
        - {"path": "docs/cbc-evidence/runs/casper-node-claim-gate-03d7f1b27-01/sources.sha256", "change": "A", "classification": "cited", "reason": "package_member"}
        - {"path": "docs/cbc-evidence/runs/casper-node-claim-gate-03d7f1b27-01/validation.json", "change": "A", "classification": "cited", "reason": "retained_validation"}
        - {"path": "docs/cbc-evidence/runs/casper-node-claim-gate-6ea6bf029-01/report.json", "change": "A", "classification": "cited", "reason": "retained_report"}
        - {"path": "docs/cbc-evidence/runs/casper-node-claim-gate-6ea6bf029-01/sources.sha256", "change": "A", "classification": "cited", "reason": "package_member"}
        - {"path": "docs/cbc-evidence/runs/casper-node-claim-gate-6ea6bf029-01/validation.json", "change": "A", "classification": "cited", "reason": "retained_validation"}
        - {"path": "docs/cbc-evidence/runs/casper-node-claim-gate-78d696ea6-01/artifacts.sha256", "change": "A", "classification": "integral", "reason": "accepted_package_manifest"}
        - {"path": "docs/cbc-evidence/runs/casper-node-claim-gate-78d696ea6-01/report.json", "change": "A", "classification": "cited", "reason": "retained_report"}
        - {"path": "docs/cbc-evidence/runs/casper-node-claim-gate-78d696ea6-01/sources.sha256", "change": "A", "classification": "cited", "reason": "package_member"}
        - {"path": "docs/cbc-evidence/runs/casper-node-claim-gate-78d696ea6-01/validation.json", "change": "A", "classification": "cited", "reason": "retained_validation"}
        - {"path": "docs/cbc-evidence/runs/casper-node-deadline-correction-4561e064a-01/report.json", "change": "A", "classification": "cited", "reason": "retained_report"}
        - {"path": "docs/cbc-evidence/runs/casper-node-deadline-correction-4561e064a-01/sources.sha256", "change": "A", "classification": "cited", "reason": "package_member"}
        - {"path": "docs/cbc-evidence/runs/casper-node-deadline-correction-4561e064a-01/validation.json", "change": "A", "classification": "cited", "reason": "retained_validation"}
        - {"path": "docs/cbc-evidence/runs/casper-node-model-reconciliation-10e7b8452-01/gate-registrations.json", "change": "A", "classification": "cited", "reason": "package_member"}
        - {"path": "docs/cbc-evidence/runs/casper-node-model-reconciliation-10e7b8452-01/report.json", "change": "A", "classification": "cited", "reason": "retained_report"}
        - {"path": "docs/cbc-evidence/runs/casper-node-model-reconciliation-10e7b8452-01/sources.sha256", "change": "A", "classification": "cited", "reason": "package_member"}
        - {"path": "docs/cbc-evidence/runs/casper-node-model-reconciliation-10e7b8452-01/validation.json", "change": "A", "classification": "cited", "reason": "retained_validation"}
        - {"path": "docs/cbc-evidence/runs/casper-node-observer-batch-a-877cea722-01/artifacts.sha256", "change": "A", "classification": "removable", "reason": "uncited_checksum"}
        - {"path": "docs/cbc-evidence/runs/casper-node-observer-batch-a-877cea722-01/report.json", "change": "A", "classification": "cited", "reason": "retained_report"}
        - {"path": "docs/cbc-evidence/runs/casper-node-observer-batch-a-877cea722-01/sources.sha256", "change": "A", "classification": "cited", "reason": "package_member"}
        - {"path": "docs/cbc-evidence/runs/casper-node-observer-shutdown-d021a1d53-01/artifacts.sha256", "change": "A", "classification": "removable", "reason": "uncited_checksum"}
        - {"path": "docs/cbc-evidence/runs/casper-node-observer-shutdown-d021a1d53-01/report.json", "change": "A", "classification": "cited", "reason": "retained_report"}
        - {"path": "docs/cbc-evidence/runs/casper-node-observer-shutdown-d021a1d53-01/sources.sha256", "change": "A", "classification": "cited", "reason": "package_member"}
        - {"path": "docs/cbc-evidence/runs/casper-node-observer-shutdown-d021a1d53-01/validation.json", "change": "A", "classification": "cited", "reason": "retained_validation"}
        - {"path": "docs/cbc-evidence/runs/casper-node-snapshot-batch-b1-799e2136a-01/artifacts.sha256", "change": "A", "classification": "removable", "reason": "uncited_checksum"}
        - {"path": "docs/cbc-evidence/runs/casper-node-snapshot-batch-b1-799e2136a-01/report.json", "change": "A", "classification": "cited", "reason": "retained_report"}
        - {"path": "docs/cbc-evidence/runs/casper-node-snapshot-batch-b1-799e2136a-01/sources.sha256", "change": "A", "classification": "cited", "reason": "package_member"}
        - {"path": "docs/cbc-evidence/runs/casper-node-snapshot-batch-b1-799e2136a-01/validation.json", "change": "A", "classification": "cited", "reason": "retained_validation"}
        - {"path": "docs/cbc-evidence/runs/casper-node-snapshot-completion-619882128-01/report.json", "change": "A", "classification": "cited", "reason": "retained_report"}
        - {"path": "docs/cbc-evidence/runs/casper-node-snapshot-completion-619882128-01/sources.sha256", "change": "A", "classification": "cited", "reason": "package_member"}
        - {"path": "docs/cbc-evidence/runs/casper-node-snapshot-completion-619882128-01/validation.json", "change": "A", "classification": "cited", "reason": "retained_validation"}
        - {"path": "docs/cbc-evidence/runs/casper-node-snapshot-hardening-2ccc4ae0a-01/artifacts.sha256", "change": "A", "classification": "cited", "reason": "validation_digest"}
        - {"path": "docs/cbc-evidence/runs/casper-node-snapshot-hardening-2ccc4ae0a-01/report.json", "change": "A", "classification": "cited", "reason": "retained_report"}
        - {"path": "docs/cbc-evidence/runs/casper-node-snapshot-hardening-2ccc4ae0a-01/sources.sha256", "change": "A", "classification": "cited", "reason": "package_member"}
        - {"path": "docs/cbc-evidence/runs/casper-node-snapshot-hardening-2ccc4ae0a-01/validation.json", "change": "A", "classification": "cited", "reason": "retained_validation"}
        - {"path": "docs/cbc-evidence/scripts-ci-check-formal-invariants-sh.md", "change": "A", "classification": "integral", "reason": "artifact_record"}
        - {"path": "docs/cbc-evidence/scripts-ci-check-node-observation-bindings-sh.md", "change": "A", "classification": "integral", "reason": "artifact_record"}
        - {"path": "docs/cbc-evidence/scripts-ci-check-tla-invariants-sh.md", "change": "M", "classification": "integral", "reason": "artifact_record"}
        - {"path": "docs/cbc-evidence/scripts-ci-test-check-tla-invariants-sh.md", "change": "A", "classification": "integral", "reason": "artifact_record"}
        - {"path": "docs/cbc-evidence/shared-src-rust-dag-observation-work-rs.md", "change": "A", "classification": "integral", "reason": "artifact_record"}
        - {"path": "docs/cbc-evidence/shared-src-rust-store-soak-snapshot-rs.md", "change": "A", "classification": "integral", "reason": "artifact_record"}
        - {"path": "docs/cbc-evidence/shared-tests-soak-snapshot-rs.md", "change": "A", "classification": "integral", "reason": "artifact_record"}
        - {"path": "docs/cbc-verification-tiers.md", "change": "M", "classification": "integral", "reason": "tier_policy"}
        - {"path": "docs/claims/casper-node-authority-evaluation.md", "change": "A", "classification": "integral", "reason": "claim_specification"}
        - {"path": "docs/claims/casper-node-authority-snapshot.md", "change": "A", "classification": "integral", "reason": "claim_specification"}
        - {"path": "docs/claims/casper-node-observation.md", "change": "A", "classification": "integral", "reason": "claim_specification"}
        - {"path": "docs/plans/casper-node-observation-batch-b.md", "change": "A", "classification": "cited", "reason": "task_plan"}
        - {"path": "docs/plans/casper-node-observation-batch-c.md", "change": "A", "classification": "cited", "reason": "task_plan"}
        - {"path": "docs/work-logs/casper-node-observation-batch-b1.md", "change": "A", "classification": "cited", "reason": "task_handoff"}
        - {"path": "docs/work-logs/casper-node-observer-shutdown-review.md", "change": "A", "classification": "cited", "reason": "task_handoff"}
        - {"path": "docs/work-logs/task-019-3-node-authority-evaluation.md", "change": "A", "classification": "cited", "reason": "task_handoff"}
        - {"path": "docs/work-logs/task-019-4-node-claim-verification.md", "change": "A", "classification": "cited", "reason": "task_handoff"}
        - {"path": "docs/work-logs/task-019-7-candidate-images.md", "change": "A", "classification": "cited", "reason": "task_handoff"}
        - {"path": "formal/rocq/node_authority/README.md", "change": "A", "classification": "integral", "reason": "formal_verification"}
        - {"path": "formal/rocq/node_authority/_CoqProject", "change": "A", "classification": "integral", "reason": "formal_verification"}
        - {"path": "formal/rocq/node_authority/theories/AuthorityObserver.v", "change": "A", "classification": "integral", "reason": "formal_verification"}
        - {"path": "formal/rocq/node_authority/theories/AuthorityWork.v", "change": "A", "classification": "integral", "reason": "formal_verification"}
        - {"path": "formal/rocq/node_authority/theories/MainTheorem.v", "change": "A", "classification": "integral", "reason": "formal_verification"}
        - {"path": "formal/rocq/node_observation/README.md", "change": "A", "classification": "integral", "reason": "formal_verification"}
        - {"path": "formal/rocq/node_observation/_CoqProject", "change": "A", "classification": "integral", "reason": "formal_verification"}
        - {"path": "formal/rocq/node_observation/theories/BoundedCapture.v", "change": "A", "classification": "integral", "reason": "formal_verification"}
        - {"path": "formal/rocq/node_observation/theories/MainTheorem.v", "change": "A", "classification": "integral", "reason": "formal_verification"}
        - {"path": "formal/rocq/node_observation/theories/ObserverSession.v", "change": "A", "classification": "integral", "reason": "formal_verification"}
        - {"path": "formal/tlaplus/node_observation/BoundedCapture.tla", "change": "A", "classification": "integral", "reason": "formal_verification"}
        - {"path": "formal/tlaplus/node_observation/MC_BoundedCapture.cfg", "change": "A", "classification": "integral", "reason": "formal_verification"}
        - {"path": "formal/tlaplus/node_observation/MC_BoundedCapture.tla", "change": "A", "classification": "integral", "reason": "formal_verification"}
        - {"path": "formal/tlaplus/node_observation/MC_BoundedCapture_admission_unsafe.cfg", "change": "A", "classification": "integral", "reason": "formal_verification"}
        - {"path": "formal/tlaplus/node_observation/MC_BoundedCapture_admission_unsafe.tla", "change": "A", "classification": "integral", "reason": "formal_verification"}
        - {"path": "formal/tlaplus/node_observation/MC_BoundedCapture_bytes_unsafe.cfg", "change": "A", "classification": "integral", "reason": "formal_verification"}
        - {"path": "formal/tlaplus/node_observation/MC_BoundedCapture_bytes_unsafe.tla", "change": "A", "classification": "integral", "reason": "formal_verification"}
        - {"path": "formal/tlaplus/node_observation/MC_BoundedCapture_deadline_unsafe.cfg", "change": "A", "classification": "integral", "reason": "formal_verification"}
        - {"path": "formal/tlaplus/node_observation/MC_BoundedCapture_deadline_unsafe.tla", "change": "A", "classification": "integral", "reason": "formal_verification"}
        - {"path": "formal/tlaplus/node_observation/MC_BoundedCapture_generation_unsafe.cfg", "change": "A", "classification": "integral", "reason": "formal_verification"}
        - {"path": "formal/tlaplus/node_observation/MC_BoundedCapture_generation_unsafe.tla", "change": "A", "classification": "integral", "reason": "formal_verification"}
        - {"path": "formal/tlaplus/node_observation/MC_BoundedCapture_incomplete_unsafe.cfg", "change": "A", "classification": "integral", "reason": "formal_verification"}
        - {"path": "formal/tlaplus/node_observation/MC_BoundedCapture_incomplete_unsafe.tla", "change": "A", "classification": "integral", "reason": "formal_verification"}
        - {"path": "formal/tlaplus/node_observation/MC_BoundedCapture_open_unsafe.cfg", "change": "A", "classification": "integral", "reason": "formal_verification"}
        - {"path": "formal/tlaplus/node_observation/MC_BoundedCapture_open_unsafe.tla", "change": "A", "classification": "integral", "reason": "formal_verification"}
        - {"path": "formal/tlaplus/node_observation/MC_BoundedCapture_order_unsafe.cfg", "change": "A", "classification": "integral", "reason": "formal_verification"}
        - {"path": "formal/tlaplus/node_observation/MC_BoundedCapture_order_unsafe.tla", "change": "A", "classification": "integral", "reason": "formal_verification"}
        - {"path": "formal/tlaplus/node_observation/MC_BoundedCapture_release_unsafe.cfg", "change": "A", "classification": "integral", "reason": "formal_verification"}
        - {"path": "formal/tlaplus/node_observation/MC_BoundedCapture_release_unsafe.tla", "change": "A", "classification": "integral", "reason": "formal_verification"}
        - {"path": "formal/tlaplus/node_observation/MC_BoundedCapture_validation_unsafe.cfg", "change": "A", "classification": "integral", "reason": "formal_verification"}
        - {"path": "formal/tlaplus/node_observation/MC_BoundedCapture_validation_unsafe.tla", "change": "A", "classification": "integral", "reason": "formal_verification"}
        - {"path": "formal/tlaplus/node_observation/MC_BoundedCapture_write_unsafe.cfg", "change": "A", "classification": "integral", "reason": "formal_verification"}
        - {"path": "formal/tlaplus/node_observation/MC_BoundedCapture_write_unsafe.tla", "change": "A", "classification": "integral", "reason": "formal_verification"}
        - {"path": "formal/tlaplus/node_observation/MC_ObserverSession.cfg", "change": "A", "classification": "integral", "reason": "formal_verification"}
        - {"path": "formal/tlaplus/node_observation/MC_ObserverSession.tla", "change": "A", "classification": "integral", "reason": "formal_verification"}
        - {"path": "formal/tlaplus/node_observation/MC_ObserverSession_budget_unsafe.cfg", "change": "A", "classification": "integral", "reason": "formal_verification"}
        - {"path": "formal/tlaplus/node_observation/MC_ObserverSession_budget_unsafe.tla", "change": "A", "classification": "integral", "reason": "formal_verification"}
        - {"path": "formal/tlaplus/node_observation/MC_ObserverSession_challenge_unsafe.cfg", "change": "A", "classification": "integral", "reason": "formal_verification"}
        - {"path": "formal/tlaplus/node_observation/MC_ObserverSession_challenge_unsafe.tla", "change": "A", "classification": "integral", "reason": "formal_verification"}
        - {"path": "formal/tlaplus/node_observation/MC_ObserverSession_deadline_unsafe.cfg", "change": "A", "classification": "integral", "reason": "formal_verification"}
        - {"path": "formal/tlaplus/node_observation/MC_ObserverSession_deadline_unsafe.tla", "change": "A", "classification": "integral", "reason": "formal_verification"}
        - {"path": "formal/tlaplus/node_observation/MC_ObserverSession_frame_unsafe.cfg", "change": "A", "classification": "integral", "reason": "formal_verification"}
        - {"path": "formal/tlaplus/node_observation/MC_ObserverSession_frame_unsafe.tla", "change": "A", "classification": "integral", "reason": "formal_verification"}
        - {"path": "formal/tlaplus/node_observation/MC_ObserverSession_freshness_pre_fix.cfg", "change": "A", "classification": "integral", "reason": "formal_verification"}
        - {"path": "formal/tlaplus/node_observation/MC_ObserverSession_freshness_pre_fix.tla", "change": "A", "classification": "integral", "reason": "formal_verification"}
        - {"path": "formal/tlaplus/node_observation/MC_ObserverSession_identity_unsafe.cfg", "change": "A", "classification": "integral", "reason": "formal_verification"}
        - {"path": "formal/tlaplus/node_observation/MC_ObserverSession_identity_unsafe.tla", "change": "A", "classification": "integral", "reason": "formal_verification"}
        - {"path": "formal/tlaplus/node_observation/MC_ObserverSession_repeat_unsafe.cfg", "change": "A", "classification": "integral", "reason": "formal_verification"}
        - {"path": "formal/tlaplus/node_observation/MC_ObserverSession_repeat_unsafe.tla", "change": "A", "classification": "integral", "reason": "formal_verification"}
        - {"path": "formal/tlaplus/node_observation/ObserverSession.tla", "change": "A", "classification": "integral", "reason": "formal_verification"}
        - {"path": "formal/tlaplus/node_observation/README.md", "change": "A", "classification": "integral", "reason": "formal_verification"}
        - {"path": "formal/tlaplus/node_observation/bindings.json", "change": "A", "classification": "integral", "reason": "formal_verification"}
        - {"path": "formal/tlaplus/node_observation/verification-plan.json", "change": "A", "classification": "integral", "reason": "formal_verification"}
    implementation_plan:
      - "Step 1. Inventory every docs/, formal/, and .github change on the branch against dev, and classify each file as integral, cited, or removable."
      - "Step 2. Consolidate the work logs to one per task, keeping decisions, acceptance records, and open findings, and dropping run-by-run narrative that a retained report already records."
      - "Step 3. Remove superseded evidence run packages, historical red-source snapshots, and plan drafts that no claim or record cites. Record each removal with its reason."
      - "Step 4. Run the strict claims audit, the link check, and the STE check before and after, and require identical results."
      - "Step 5. Record the file and line counts of the diff against dev before and after, and obtain maintainer confirmation of the reduced diff before TASK-019-6 proceeds."
    acceptance:
      - "No removal changes a claim status, a record status, a tier field, or an audit result."
      - "Every file an accepted claim cites is still present at its recorded digest."
      - "Each removed file is listed with a reason, and any externalized evidence names its location and digest."
      - "The maintainer confirms the reduced diff before the merge."
  - id: TASK-019-9
    title: "Batch D: paired fork-choice observation bound to one capture"
    status: complete
    completed_on: "2026-09-30"
    claimed_by: claude-session-f3cbc961
    claimed_at: 2026-09-30T04:40:00Z
    assignment: "User decision 2026-09-30: agent A (this session) takes Batch D and then the removals of TASK-019-8. Agent B (the pi session of 2026-09-30, see docs/work-logs/node-observation-agent-b-20260930.md) takes the record refresh, the STE fix, the TASK-019-8 inventory, and Batch E after Batch D."
    work_log: docs/work-logs/task-019-9-paired-fork-choice.md
    recorded_by: claude-session-f3cbc961
    recorded_on: 2026-09-30
    blocked_by: []
    precedes: [TASK-019-10]
    stage: "All 16 steps complete on 2026-09-30. The named maintainer accepted claim 004 at revision 3ab092cc5 on the evidence package. 21 records are discharged. 8 shared records have Batch E successor versions that point at the accepted Batch D version through previous_record."
    proposed_reviewer: jltatbeach
    eligible_maintainers: [spreston8, dylon, metaweta, jeffrey-l-turner, jltatbeach]
    acceptance_request: docs/work-logs/task-019-9-paired-fork-choice.md#step-16-on-2026-09-30-acceptance-request
    accepted_by: jltatbeach
    acceptance_record: https://github.com/F1R3FLY-io/f1r3node-rust/pull/447#issuecomment-5918385665
    acceptance_form: "Issue comment by the named maintainer, edited 2026-09-30T19:45:09Z. The 2 earlier acceptances were pull request reviews. The text names the claim, the revision, and the package."
    acceptance_revision: 3ab092cc58fb30f4da39e6c8b28b8d25206c661b
    acceptance_reviewed_at: 2026-09-30T19:41:35Z
    finding_1_decision: "No production change in this batch. An issue for the production score extent is optional and not opened."
    tiers_reached:
      refutation: "PairedForkChoice model, 203 distinct states clean, 5 negative controls violate their named invariants. Formal gate clean in both tiers."
      construction: "Pending for the 13 U rows, accepted as recorded. D1 and D11 inherit accepted theorems. D7, D12, D16 accepted as bounded by design."
      binding: "16 properties mapped to 30 tests. Observer 39, unit 57, mod.rs 66, node 21 plus 1 ignored, node library 259. Linux binding driver exit 0."
    strict_cbc_result: "Exit 4 with 29 pending before the acceptance. After the acceptance the 29-file inventory has 20 discharged and 9 pending: 8 Batch E successors and the soak gate record of check-tla-invariants.sh. 21 record files are discharged in total."
    acceptance_package: docs/cbc-evidence/runs/casper-node-fork-choice-batch-d-3ab092cc5-01
    evidence_commit: 28606f110
    consumer: "TASK-017-12 harness profile on formal/soak-casper-consensus: the profile compares the selected head of a bounded member and a reference member on equal inputs."
    claims: [CLAIM-CASPER-NODE-OBSERVATION-004]
    claim_status: accepted
    claim_file: docs/claims/casper-node-fork-choice-observation.md
    plan: docs/plans/casper-node-observation-batch-d.md
    draft_reviewed_base: 670037c2511abd5f576063b3153681a873244a18
    file_scope_confirmed: true
    file_scope_confirmed_on: 2026-09-30
    file_scope_confirmed_by: user
    file_scope_size: "34 files: 17 new and 17 changed (11 Rust source and test files, 16 formal files, 3 gate and tag files, 4 documents), plus 28 ledger records at the end (14 now, 14 at step 5) and 1 run package"
    decisions: "The 12 recommendations of the draft, accepted by the user on 2026-09-30. Decision 8: estimator.rs, dag_operations.rs, and proto_util.rs get the mandatory tag and 3 ledger records. The table is in the plan."
    ledger_registration: "14 pending records under scope batch-d-registration: 11 replaced records of changed files (previous_record names the replaced record) and 3 new records of the newly tagged files. 13 records of the model files were written at step 5 with their digests. The record of fork_choice.rs follows at step 10 (user decision 2026-09-30: a record is written when its file exists). scripts/ci/check-tla-invariants.sh keeps its record under CLAIM-SOAK-GATE-001, as at the Batch B2 registration."
    scope: "Add a paired fork-choice observation to the authority_snapshot operation. One capture supplies the inputs of the 2 evaluations. No new consensus rule and no new production limit."
    acceptance:
      - "The response of authority_snapshot carries the selected head of the bounded evaluation and of the reference evaluation, with the input digest of the shared capture."
      - "The estimator, the common ancestor walk, the weight read, and fork_choice_floor have metered entry points, and the current entry points call them with NoopWork."
      - "The work paths of observation_work.rs increase from 4 to 6, with tests for the limits."
      - "The node capability list has the fork_choice entry, and node/tests/soak_observer.rs tests its admission."
      - "CLAIM-CASPER-NODE-OBSERVATION-004 is registered as pending before the first code change, and its evidence record is source bound."
    constraints:
      - "No code before the user confirms the exact file list of the plan draft."
      - "Confirmation of this batch does not authorize Batch E."
  - id: TASK-019-10
    title: "Batch E: equivocation input capture for the detached display projection"
    status: complete
    completed_on: "2026-10-01"
    closed_by: claude-session-f3cbc961
    claimed_by: pi-session-01a0ab62-71b3-7248-a800-37a6fde2e4fa
    claimed_at: 2026-09-30T17:45:58Z
    proposed_owner: pi-session-01a0ab62-71b3-7248-a800-37a6fde2e4fa
    recorded_by: claude-session-f3cbc961
    recorded_on: 2026-09-30
    blocked_by: []
    implementation_dependency: TASK-019-9_step_13_handoff
    handoff_revision: 3ab092cc58fb30f4da39e6c8b28b8d25206c661b
    handoff_digests_verified: 7
    claims: [CLAIM-CASPER-NODE-OBSERVATION-005]
    claim_status: accepted
    claim_file: docs/claims/casper-node-display-projection.md
    work_log: docs/work-logs/task-019-10-display-projection.md
    plan: docs/plans/casper-node-observation-batch-e.md
    plan_draft: "target/node-observation-prep-20260928-01/batch-e/ (historical local draft, ignored by Git)"
    draft_reviewed_base: 670037c2511abd5f576063b3153681a873244a18
    scope_reviewed_base: 030384f5f31613c64552f7d5b517ef1fe2552d11
    file_scope_confirmed: true
    file_scope_confirmed_by: user
    file_scope_confirmed_at: 2026-09-30T15:38:36.579734+00:00
    scope_recorded_by: pi-session-01a0ab62-71b3-7248-a800-37a6fde2e4fa
    approval_checkpoint: 06f0d3e8cf85f7072769960cde3746dc0830a68f
    file_scope_size: "72 files: 39 main files, 31 ledger records, and 2 run files. The scope has 33 new files and 39 changed files."
    decisions: "All 16 documented recommendations approved, with the complete shared arithmetic, 5 mandatory tags, and the 4096-row ceiling."
    cbc_tags_ratified: true
    cbc_tags_applied: true
    implementation_baseline: 3ab092cc58fb30f4da39e6c8b28b8d25206c661b
    source_scope_recheck_required: false
    stage: "Complete on 2026-10-01. The named maintainer accepted claim 005 at revision 1a9839b0e on the evidence package. The 31 Batch E records are discharged. CI on 1a9839b0e was still running at the acceptance; the display admission test fix of that revision is proven by that run."
    record_refresh_coordination: "All 31 Batch E records bind final sources at c83b16f30 plus 7 working-tree artifact hashes. The 16 original prior-record identities and 19 prior-refresh identities remain exact. Twelve late registrations remain explicit. The PR-base mandatory inventory has 106 sources, so this refresh does not cover the other 75."
    verification_package: docs/cbc-evidence/runs/casper-node-display-projection-batch-e-01
    proposed_reviewer: jltatbeach
    accepted_by: jltatbeach
    acceptance_record: https://github.com/F1R3FLY-io/f1r3node-rust/pull/447#issuecomment-5924422936
    acceptance_form: "Issue comment by the named maintainer. The text names the claim, the revision, the package, and the report SHA-256. The suggested statement appears once as a quote and once as the maintainer's own text."
    acceptance_revision: 1a9839b0e52e494e20ab13c0a79a55bd2164e34f
    acceptance_reviewed_at: 2026-10-01T03:56:25Z
    acceptance_package: docs/cbc-evidence/runs/casper-node-display-projection-batch-e-01
    acceptance_decisions: "E1, E14, E17 bounded by design. Construction gaps E7, E11, E15 and the other stated limits accepted as recorded. The 12 late source registrations acknowledged."
    tiers_reached:
      refutation: "DisplayProjection bounded model with 3 negative controls. 17 positive TLA+ configurations and 86 expected negative controls in the gate."
      construction: "Scoped integer proofs with declared gaps: 47 closed assumption sets. E7 and E15 tracker wire encoding and E11 IEEE-754 arithmetic stay pending, accepted as recorded."
      binding: "17 properties mapped to 26 executed named tests. 495 distinct selected tests passed, including 259 isolated node library tests and 22 isolated node integration tests."
    strict_cbc_result: "Exit 4 with 31 pending records before the acceptance. After the acceptance the 31 Batch E records are discharged."
    acceptance_request: docs/work-logs/task-019-10-display-projection.md#maintainer-acceptance-request
    open_questions: "No scope decision remains open. A new implementation path requires a scope amendment."
    scope: "Capture the equivocation inputs in the same interval as the detached DAG capture, and calculate the display projection from captured inputs only, with the arithmetic of the live calculation."
    acceptance:
      - "The detached observer reports the display projection from the captured inputs, not as unavailable."
      - "The capture reads the equivocation tracker in the same consistency interval as the DAG capture, with a refusal when the read fails."
      - "The display arithmetic has one source that the live path and the detached path share, with a test that compares the two on equal inputs."
      - "CLAIM-CASPER-NODE-OBSERVATION-005 is registered as pending before the first code change, and its evidence record is source bound."
    constraints:
      - "No code before the user confirms the exact file list of the plan draft."
      - "Starts after the explicit TASK-019-9 step-13 file handoff, not after full task acceptance. Agent A retains Batch D verification and its step-15 record refresh."
---
```

**Current state:** Batch A and B1 have implementation evidence. The first approved verification cycle corrects challenge reuse within one observer lifetime and records 580 passing isolated test executions.

Both claims remain pending on complete formal evidence and named maintainer acceptance under TASK-019-4.

B2 planning steps 1 through 4 and TASK-019-5 research are complete.
B2 and Batch C implementation remain unapproved. The Batch C inventory and contract await named maintainer review.

TASK-019-7 records verified baseline image and executable digests.
The lifecycle fix merged to system-integration `dev` through PR #144.
Promotion PR #145 merged to `main`. The three-file node pin update is prepared.
Node pin publication, live validation, and the observer candidate remain pending. PR #447 targets `dev`.

**Scope:** This epic owns node-side interfaces only. Harness verification, profile qualification, campaign execution, and baseline soaks belong to EPIC-017.

---

### EPIC-017: Ratified Casper Conformance and Soak Evidence

```yaml
---
epic_id: EPIC-017
title: "Ratified Casper Conformance and Soak Evidence"
status: in_progress
priority: p0
user_story: US-006
user_flow: FLOW-001
blocked_by: []
created_at: 2026-09-16
updated_at: 2026-09-17
claimed_by: pi-casper-ratification-planning
claimed_at: 2026-09-16T20:29:37Z
branch: formal/soak-casper-consensus
plan: docs/plans/casper-ratified-soak-2026-09-16.md
related_epics: [EPIC-010, EPIC-012, EPIC-013, EPIC-015, EPIC-016, EPIC-018]
phase: pre_pr216_merge
scaffold_status: drafted
claim_index: docs/claims/casper-soak-harness.md
formal_plan: formal/tlaplus/casper_soak/verification-plan.jsonc
cycle_plan: docs/tdd-plans/casper-soak-harness.md
follow_on_epic: EPIC-018
source_prs: [216, 390, 430, 431, 432, 433]
execution_contract:
  base_branch: dev
  authority: "The 2026-09-16 ratification meeting controls the selected dispositions. Current dev remains the default Casper authority."
  integration_order: "PR #430 -> #431 -> #432 -> #433. PR #390 records decisions. PR #216 remains a candidate implementation."
  planned_stack_parent: docs/consensus-neutral-execution
  stack_integration_status: verified
  stack_parent_revision: 65f7f6daa832c0acb6fddf2b462db1b9d5461729
  stack_merge_revision: 0f1ccdf38f9ab3b056e7601b93961cb56c0a51e9
  stack_verification: docs/casper/cbc-evidence/runs/casper-stack-integration-20260917-01/report.json
  pull_request: 436
  pr_base_branch: docs/consensus-neutral-execution
  dependency_approval_date: 2026-09-17
  implementation_tasks: [TASK-017-4, TASK-017-5, TASK-017-6, TASK-017-7, TASK-017-8, TASK-017-9, TASK-017-10, TASK-017-11]
  implementation_prerequisites:
    TASK-017-2: "contract_status=complete"
    TASK-017-3: "prerequisite_application=complete"
  implementation_policy: "The listed tasks may proceed together against the completed contract and applied prerequisites. Tracker closure is not their implementation prerequisite."
  final_workload_pinning_task: TASK-017-12
  scope: "Verify only the soak harness and profiles: generation, fault scheduling, collection, classification, and evidence handling. The node is the system under test."
  git_policy: "Do not merge, commit, push, or create a PR without separate user authorization."
  completion_policy: "Close after pre-merge scope claims pass, baseline evidence is reviewed, and the EPIC-018 handoff is accepted. PR #216 merge is not a blocker."
  evidence_boundary: "Node correctness, runtime repairs, and Rocq proofs are outside both epics. Post-merge work adapts and reverifies harness profiles only."
files:
  - scripts/run-merge-recovery-soak.sh
  - scripts/bench/test-run-merge-recovery-soak.sh
  - scripts/casper-soak/src/manifest.rs
  - scripts/casper-soak/tests/manifest.rs
  - scripts/bench/write-soak-summary.sh
  - scripts/ci/check-tla-invariants.sh
  - .github/workflows/merge-recovery-soak.yml
  - formal/tlaplus/casper_soak/README.md
  - formal/tlaplus/casper_soak/verification-plan.jsonc
  - formal/tlaplus/casper_soak/CasperSoakHarness.tla
  - formal/tlaplus/casper_soak/MC_CasperSoakHarness.cfg
  - formal/tlaplus/casper_soak/MC_CasperSoakHarness_identity_unsafe.cfg
  - formal/tlaplus/casper_soak/MC_CasperSoakHarness_resume_unsafe.cfg
  - formal/tlaplus/casper_soak/MC_CasperSoakHarness_failure_unsafe.cfg
  - formal/tlaplus/casper_soak/MC_CasperSoakHarness_evidence_unsafe.cfg
  - formal/tlaplus/casper_soak/MC_CasperSoakHarness_stop_unsafe.cfg
  - formal/tlaplus/casper_soak/MC_CasperSoakHarness_cleanup_unsafe.cfg
  - formal/tlaplus/casper_soak/MC_CasperSoakHarness_policy_unsafe.cfg
  - formal/tlaplus/casper_soak/MC_CasperSoakHarness_samples_unsafe.cfg
  - formal/tlaplus/casper_soak/MC_CasperSoakHarness_merge_unsafe.cfg
  - formal/tlaplus/casper_soak/MC_CasperSoakHarness_control_unsafe.cfg
  - scripts/ci/check-casper-soak-models.sh
  - scripts/ci/check-casper-soak-bindings.sh
  - scripts/casper-soak/Cargo.toml
  - scripts/casper-soak/src/lib.rs
  - scripts/casper-soak/src/main.rs
  - scripts/casper-soak/src/models.rs
  - scripts/casper-soak/src/runtime.rs
  - scripts/casper-soak/tests/models.rs
  - scripts/casper-soak/tests/driver.rs
  - scripts/bench/casper-soak.sh
  - scripts/bench/fixtures/casper-lifecycle-executor.sh
  - .github/workflows/slashing-tests.yml
  - scripts/casper-soak/src/bin/check-casper-bindings.rs
  - scripts/casper-soak/tests/bindings.rs
  - scripts/casper-soak/tests/interruption.rs
  - scripts/casper-soak/src/bin/check-casper-claims.rs
  - scripts/casper-soak/tests/claims.rs
  - scripts/casper-soak/task-complete.sh
  - scripts/casper-soak/src/profiles/authority_finality.rs
  - scripts/casper-soak/src/bin/casper-authority-finality.rs
  - scripts/casper-soak/tests/authority_finality.rs
  - scripts/casper-soak/check-authority-finality.sh
  - .github/workflows/casper-authority-finality.yml
  - formal/tlaplus/casper_soak/profiles/authority_finality/AuthorityFinality.tla
  - formal/tlaplus/casper_soak/profiles/authority_finality/MC_AuthorityFinality.cfg
  - formal/tlaplus/casper_soak/profiles/authority_finality/MC_AuthorityFinality_pair_unsafe.cfg
  - formal/tlaplus/casper_soak/profiles/authority_finality/MC_AuthorityFinality_finality_unsafe.cfg
  - formal/tlaplus/casper_soak/profiles/authority_finality/MC_AuthorityFinality_head_unsafe.cfg
  - formal/tlaplus/casper_soak/profiles/authority_finality/verification-plan.jsonc
  - formal/tlaplus/casper_soak/profiles/authority_finality/README.md
  - scripts/casper-soak/src/profiles/carrier_index.rs
  - scripts/casper-soak/src/bin/casper-carrier-index.rs
  - scripts/casper-soak/tests/carrier_index.rs
  - scripts/casper-soak/check-carrier-index.sh
  - .github/workflows/casper-carrier-index.yml
  - formal/tlaplus/casper_soak/profiles/carrier_index/CarrierIndex.tla
  - formal/tlaplus/casper_soak/profiles/carrier_index/MC_CarrierIndex.cfg
  - formal/tlaplus/casper_soak/profiles/carrier_index/MC_CarrierIndex_path_unsafe.cfg
  - formal/tlaplus/casper_soak/profiles/carrier_index/MC_CarrierIndex_window_unsafe.cfg
  - formal/tlaplus/casper_soak/profiles/carrier_index/MC_CarrierIndex_counter_unsafe.cfg
  - formal/tlaplus/casper_soak/profiles/carrier_index/verification-plan.jsonc
  - formal/tlaplus/casper_soak/profiles/carrier_index/README.md
tasks:
  - id: TASK-017-1
    title: "Reconcile ratifications, existing epics, and source dependencies"
    status: complete
    claimed_by: pi-casper-ratification-planning
    work_log: docs/work-logs/task-017-1-3-completion.md
    completion_evidence: docs/casper/cbc-evidence/runs/casper-preparation-completion-20260918-01/report.json
    completion_blocker: null
    unit_tests: [docs/casper/cbc-evidence/runs/casper-preparation-completion-20260918-01/verify.sh]
    files: [docs/plans/casper-ratified-soak-2026-09-16.md, docs/work-logs/task-017-1-3-completion.md]
    blocked_by: []
    acceptance:
      - "The branch plan maps D-01 through D-12 to tasks, activation conditions, and evidence."
      - "The inventory separates current dev, local ratification records, and open PR revisions."
      - "Existing epic overlaps, claims, parser limitations, and stale specification conflicts are recorded."
      - "The maintainer reviews the pre/post split and plan before implementation starts."

    completion_gaps: []
    completed_date: 2026-09-19
  - id: TASK-017-2
    title: "Define harness and profile claims, fixture expectations, and source scope"
    scaffold_status: specified
    contract_status: complete
    interface_contract: docs/casper/design/soak-interface-contract.md
    work_log: docs/work-logs/task-017-2-interface-contract-2026-09-17.md
    completion_blocker: null
    completion_review: docs/work-logs/task-017-1-3-completion.md
    completion_evidence: docs/casper/cbc-evidence/runs/casper-preparation-completion-20260918-01/report.json
    unit_tests: [docs/casper/cbc-evidence/runs/casper-preparation-completion-20260918-01/verify.sh]
    files: [docs/casper/design/soak-interface-contract.md]
    claim_index: docs/claims/casper-soak-harness.md
    status: complete
    claimed_by: pi-casper-harness
    claimed_at: 2026-09-16T22:18:38Z
    blocked_by: [TASK-017-1]
    decisions: [D-02, D-03, D-04, D-06, D-11]
    acceptance:
      - "Each claim names harness inputs, outputs, assumptions, finite bounds, negative controls, and executable fixtures."
      - "Profile expectations follow the ratifications without importing conflicting legacy node claims."
      - "Mandatory scope contains harness, workflow, model, and profile artifacts only."
      - "Node source paths and node correctness claims are not discharge obligations for either epic."
      - "Unavailable interfaces remain explicit scenario blockers. No mock result becomes product evidence."
      - "Each profile identifies its post-merge interface adaptation under EPIC-018."

    completion_gaps: []
    completed_date: 2026-09-19
  - id: TASK-017-3
    title: "Integrate reviewed harness prerequisites and record initial candidate identities"
    status: complete
    claimed_by: pi-casper-harness
    claimed_at: 2026-09-17T01:43:26Z
    execution_scope: "Approved prerequisites and initial candidate identities only. Final executable workload pinning belongs to TASK-017-12. No Git publication or merge is authorized."
    work_log: docs/work-logs/task-017-3-prerequisite-application-2026-09-17.md
    prerequisite_review: docs/work-logs/task-017-3-prerequisite-review-2026-09-17.md
    prerequisite_application: complete
    candidate_matrix: docs/casper/design/soak-candidate-matrix.jsonc
    validation_evidence: docs/casper/cbc-evidence/runs/casper-prerequisite-application-20260917-01/report.json
    completion_blocker: null
    completion_review: docs/work-logs/task-017-1-3-completion.md
    completion_evidence: docs/casper/cbc-evidence/runs/casper-preparation-completion-20260918-01/report.json
    unit_tests: [docs/casper/cbc-evidence/runs/casper-preparation-completion-20260918-01/verify.sh]
    files: [docs/casper/design/soak-candidate-matrix.jsonc, docs/work-logs/task-017-1-3-completion.md]
    blocked_by: [TASK-017-1]
    external_prs: [390, 430, 431, 432, 433]
    acceptance:
      - "Approved prerequisite integration preserves the #430 -> #431 -> #432 -> #433 order."
      - "Initial node, harness, model, image, and configuration-source identities are recorded for every candidate."
      - "TASK-017-12 owns final executable workload pinning. The initial matrix remains non-dispatchable until qualification and required verification pass."
      - "PR #431 containment limitations and inherited evidence remain explicit."
      - "The 15-minute and two-minute bounded-tier descriptions are reconciled against the implemented gate."
      - "The system-integration fixture contract is reviewed before any coordinated harness edit or repin."
      - "PR #216 stays an optional candidate reference. Its merge does not gate this phase."

    completion_gaps: []
    completed_date: 2026-09-19
  - id: TASK-017-4
    title: "Bind experiment manifests and formal controls to workflow evidence"
    claims: [CLAIM-CASPER-SOAK-001]
    claim_spec: docs/claims/casper-soak-harness.md
    formal_plan: formal/tlaplus/casper_soak/verification-plan.jsonc
    cycle_plan: docs/tdd-plans/casper-soak-harness.md
    status: complete
    claimed_by: pi-casper-harness
    claimed_at: 2026-09-16T22:18:38Z
    execution_scope: "Authorized completion work: shared registration and real-driver bindings. No node dispatch, external repin, or claim waiver."
    work_log: docs/work-logs/task-017-4-acceptance.md
    completion_evidence: docs/casper/cbc-evidence/runs/casper-task-017-4-acceptance-01/validation.json
    unit_tests:
      - scripts/casper-soak/tests/manifest.rs
      - scripts/casper-soak/tests/models.rs
      - scripts/casper-soak/tests/driver.rs
      - scripts/casper-soak/tests/bindings.rs
      - scripts/casper-soak/tests/interruption.rs
      - scripts/casper-soak/tests/claims.rs
    files:
      - scripts/run-merge-recovery-soak.sh
      - scripts/bench/fixtures/casper-lifecycle-executor.sh
      - scripts/casper-soak/src/main.rs
      - scripts/casper-soak/src/runtime.rs
      - scripts/casper-soak/src/bin/check-casper-bindings.rs
      - scripts/casper-soak/src/bin/check-casper-claims.rs
      - scripts/casper-soak/tests/driver.rs
      - scripts/casper-soak/tests/bindings.rs
      - scripts/casper-soak/tests/interruption.rs
      - scripts/casper-soak/tests/claims.rs
      - scripts/casper-soak/task-complete.sh
      - scripts/ci/check-casper-soak-bindings.sh
      - formal/tlaplus/casper_soak/verification-plan.jsonc
      - .github/workflows/slashing-tests.yml
    completion_blocker: null
    blocked_by: []
    decisions: [D-11]
    acceptance:
      - "The evidence contract includes revisions, seeds, run IDs, tool versions, bounds, assumptions, artifacts, and terminal outcomes."
      - "Positive models must pass and registered negative controls must violate their named property."
      - "Unexpected success, timeout, cancellation, missing evidence, and tool failure cannot become a passing control."
      - "Deterministic conformance and soak profiles remain separate."
      - "Deferred policies cannot become a production default or a node-local block-validity switch."
      - "Property tiers report their actual case counts, and claim-discharge scripts run in workflows."
      - "Implement H01 through H10 with a clean TLC configuration, named negative controls, and real-driver fixtures."
      - "Construction is not applicable to these harness and profile claims. Runtime proofs remain outside these epics."

    completion_gaps: []
    completed_date: 2026-09-18
  - id: TASK-017-5
    title: "Verify authority and finality profile generation and verdicts"
    claims: [CLAIM-CASPER-SOAK-002]
    claim_spec: docs/claims/casper-soak-authority-finality.md
    status: complete
    claimed_by: pi-casper-authority-finality
    claimed_at: 2026-09-18T13:08:20Z
    work_log: docs/work-logs/task-017-5-authority-finality.md
    implementation_status: controlled-transcript-implemented
    validation_evidence: docs/casper/cbc-evidence/runs/casper-profile-acceptance-20260919-01/report.json
    binding_review: docs/work-logs/task-017-5-7-binding-review.md
    completion_evidence: docs/work-logs/task-017-5-7-acceptance.md
    completion_blocker: null
    tests:
      - scripts/casper-soak/tests/authority_finality.rs
      - scripts/casper-soak/check-authority-finality.sh
    files:
      - scripts/casper-soak/src/profiles/authority_finality.rs
      - scripts/casper-soak/src/bin/casper-authority-finality.rs
      - scripts/casper-soak/tests/authority_finality.rs
      - scripts/casper-soak/check-authority-finality.sh
      - .github/workflows/casper-authority-finality.yml
      - formal/tlaplus/casper_soak/profiles/authority_finality/AuthorityFinality.tla
      - formal/tlaplus/casper_soak/profiles/authority_finality/MC_AuthorityFinality.cfg
      - formal/tlaplus/casper_soak/profiles/authority_finality/MC_AuthorityFinality_pair_unsafe.cfg
      - formal/tlaplus/casper_soak/profiles/authority_finality/MC_AuthorityFinality_finality_unsafe.cfg
      - formal/tlaplus/casper_soak/profiles/authority_finality/MC_AuthorityFinality_head_unsafe.cfg
      - formal/tlaplus/casper_soak/profiles/authority_finality/verification-plan.jsonc
      - formal/tlaplus/casper_soak/profiles/authority_finality/README.md
    blocked_by: []
    decisions: [D-02, D-03, D-04]
    related_tasks: [TASK-012-19, TASK-012-20, TASK-015-1]
    acceptance:
      - "Paired comparisons use identical DAG fixtures, seeds, candidate identities, and declared electorate inputs."
      - "Fixtures prove that head mismatches are reported and missing finality observations cannot pass."
      - "Profile definitions cover ratified threshold boundaries, metadata holds, replay, restart, and dependencies."
      - "The collector reports traversal counters without inferring a node work-bound proof."
      - "Unsupported node test interfaces block the scenario rather than expand this epic into runtime implementation."

    unit_tests: [scripts/casper-soak/tests/authority_finality.rs]
    completion_gaps: []
    completed_date: 2026-09-19
  - id: TASK-017-6
    title: "Verify publication and restart fault profiles and observations"
    claims: [CLAIM-CASPER-SOAK-003]
    claim_spec: docs/claims/casper-soak-publication.md
    status: complete
    claimed_by: pi-casper-publication
    claimed_at: 2026-09-18T14:39:27Z
    work_log: docs/work-logs/task-017-6-publication.md
    implementation_status: controlled-transcript-implemented
    validation_evidence: docs/casper/cbc-evidence/runs/casper-profile-acceptance-20260919-01/report.json
    binding_review: docs/work-logs/task-017-5-7-binding-review.md
    completion_evidence: docs/work-logs/task-017-5-7-acceptance.md
    completion_blocker: null
    tests:
      - scripts/casper-soak/tests/publication.rs
      - scripts/casper-soak/check-publication.sh
    files:
      - scripts/casper-soak/src/profiles/publication.rs
      - scripts/casper-soak/src/bin/casper-publication.rs
      - scripts/casper-soak/tests/publication.rs
      - scripts/casper-soak/check-publication.sh
      - .github/workflows/casper-publication.yml
      - formal/tlaplus/casper_soak/profiles/publication/Publication.tla
      - formal/tlaplus/casper_soak/profiles/publication/MC_Publication.cfg
      - formal/tlaplus/casper_soak/profiles/publication/MC_Publication_fault_unsafe.cfg
      - formal/tlaplus/casper_soak/profiles/publication/MC_Publication_restart_unsafe.cfg
      - formal/tlaplus/casper_soak/profiles/publication/MC_Publication_tuple_unsafe.cfg
      - formal/tlaplus/casper_soak/profiles/publication/verification-plan.jsonc
      - formal/tlaplus/casper_soak/profiles/publication/README.md
    blocked_by: []
    decisions: [D-05]
    acceptance:
      - "Fault coverage requires an observed crash acknowledgment, not only a requested injection."
      - "The collector correlates publication tuples and restart observations by node and run identity."
      - "Planted torn tuples, stale publications, and lost-work observations produce product failures."
      - "Missing durable-state observations remain incomplete evidence."
      - "Baseline and optional parallel profiles remain separate and cannot change production defaults."

    unit_tests: [scripts/casper-soak/tests/publication.rs]
    completion_gaps: []
    completed_date: 2026-09-19
  - id: TASK-017-7
    title: "Prepare isolated heartbeat and retry experiments against the baseline"
    claims: [CLAIM-CASPER-SOAK-004]
    claim_spec: docs/claims/casper-soak-recovery.md
    status: complete
    claimed_by: pi-casper-recovery
    claimed_at: 2026-09-18T15:43:03Z
    work_log: docs/work-logs/task-017-7-recovery.md
    implementation_status: controlled-transcript-implemented
    validation_evidence: docs/casper/cbc-evidence/runs/casper-profile-acceptance-20260919-01/report.json
    binding_review: docs/work-logs/task-017-5-7-binding-review.md
    completion_evidence: docs/work-logs/task-017-5-7-acceptance.md
    completion_blocker: null
    tests:
      - scripts/casper-soak/tests/recovery.rs
      - scripts/casper-soak/check-recovery.sh
    files:
      - scripts/casper-soak/src/profiles/recovery.rs
      - scripts/casper-soak/src/bin/casper-recovery.rs
      - scripts/casper-soak/tests/recovery.rs
      - scripts/casper-soak/check-recovery.sh
      - .github/workflows/casper-recovery.yml
      - formal/tlaplus/casper_soak/profiles/recovery/Recovery.tla
      - formal/tlaplus/casper_soak/profiles/recovery/MC_Recovery.cfg
      - formal/tlaplus/casper_soak/profiles/recovery/MC_Recovery_lane_unsafe.cfg
      - formal/tlaplus/casper_soak/profiles/recovery/MC_Recovery_occurrence_unsafe.cfg
      - formal/tlaplus/casper_soak/profiles/recovery/MC_Recovery_pause_unsafe.cfg
      - formal/tlaplus/casper_soak/profiles/recovery/verification-plan.jsonc
      - formal/tlaplus/casper_soak/profiles/recovery/README.md
    blocked_by: []
    decisions: [D-06, D-07]
    related_tasks: [TASK-016-5, TASK-016-6, TASK-016-7]
    acceptance:
      - "Profiles compare frontier follow, rotating versus all-eligible recovery, and progress-clock variants."
      - "Profiles compare one-parent versus collective coverage and current leadership versus leader-free custody."
      - "Paused validators, delayed messages, split frontiers, and empty or deploy-bearing workloads are covered."
      - "Reports include duplicates, custody consistency, retry completion, expiry, recovery time, cadence, latency, and resources."
      - "The profile records lease and objective-height inputs and reports observed violations without changing retry authorization."
      - "An experiment cannot grant authority to activate its policy."
      - "Record baseline results and candidate availability. The merged-runtime comparison belongs to TASK-018-5."

    unit_tests: [scripts/casper-soak/tests/recovery.rs]
    completion_gaps: []
    completed_date: 2026-09-19
  - id: TASK-017-8
    title: "Verify merge and accounting workload generation and measurement"
    claims: [CLAIM-CASPER-SOAK-005]
    claim_spec: docs/claims/casper-soak-merge-accounting.md
    status: complete
    claimed_by: pi-casper-merge-accounting
    claimed_at: 2026-09-19T05:47:48Z
    work_log: docs/work-logs/task-017-8-merge-accounting.md
    implementation_status: controlled-transcript-implemented
    validation_evidence: docs/casper/cbc-evidence/runs/casper-merge-accounting-acceptance-20260919-01/report.json
    completion_evidence: docs/work-logs/task-017-8-merge-accounting.md
    completion_blocker: null
    tests:
      - scripts/casper-soak/tests/merge_accounting.rs
      - scripts/casper-soak/check-merge-accounting.sh
    files:
      - scripts/casper-soak/src/profiles/merge_accounting.rs
      - scripts/casper-soak/src/bin/casper-merge-accounting.rs
      - scripts/casper-soak/tests/merge_accounting.rs
      - scripts/casper-soak/check-merge-accounting.sh
      - .github/workflows/casper-merge-accounting.yml
      - formal/tlaplus/casper_soak/profiles/merge_accounting/MergeAccounting.tla
      - formal/tlaplus/casper_soak/profiles/merge_accounting/verification-plan.jsonc
    blocked_by: []
    decisions: [D-08]
    external_prs: [216]
    related_tasks: [TASK-016-1, TASK-016-3, TASK-016-4]
    acceptance:
      - "Generated workloads distinguish repeated observations from independent executions with identical effects."
      - "The collector retains execution identities, causal relationships, admission outcomes, settlement data, and token domains."
      - "Controlled transcripts expose multiplicity loss, missing settlements, and mislabeled compatibility."
      - "Accounting expectations use pinned fixture values. The profile does not implement or prove node accounting."
      - "Conditional additive profiles remain isolated and do not authorize protocol activation."

    unit_tests: [scripts/casper-soak/tests/merge_accounting.rs]
    completion_gaps: []
    completed_date: 2026-09-19
  - id: TASK-017-9
    title: "Verify slashing scenario scheduling and result classification"
    claims: [CLAIM-CASPER-SOAK-006]
    claim_spec: docs/claims/casper-soak-slashing.md
    status: complete
    claimed_by: pi-casper-slashing
    claimed_at: 2026-09-19T07:13:31Z
    work_log: docs/work-logs/task-017-9-slashing.md
    implementation_status: controlled-transcript-implemented
    binding_status: accepted
    binding_acceptance: docs/casper/cbc-evidence/runs/casper-slashing-acceptance-20260919-01/report.json
    hosted_workflow_status: verified
    hosted_workflow_run: 35452747041
    workflow_tag_status: ratified
    workflow_tag_ratification: docs/casper/cbc-evidence/runs/casper-slashing-ratification-20260919-01/report.json
    validation_evidence: docs/casper/cbc-evidence/runs/casper-slashing-ratification-20260919-01/validation.json
    completion_evidence: docs/work-logs/task-017-9-slashing.md
    completion_blocker: null
    unit_tests: [scripts/casper-soak/tests/slashing.rs]
    blocked_by: []
    decisions: [D-09]
    acceptance:
      - "The profile schedules merge-lost slash, rebond, stale-epoch, missing-evidence, forged-deploy, and restart scenarios."
      - "Observed delivery order and epochs remain distinct from requested scheduling."
      - "Fixtures prove that planted authorization mismatches are reported rather than suppressed."
      - "Node slash authorization, reconstruction, and bisimilarity proofs remain outside this epic."
    completion_gaps: []
    completed_date: 2026-09-19

  - id: TASK-017-10
    title: "Verify carrier-index comparison inputs and telemetry classification"
    status: complete
    claimed_by: pi-soak-carrier-index-linux
    claimed_at: 2026-09-19T06:11:36Z
    work_log: docs/work-logs/task-017-10-carrier-index.md
    blocked_by: []
    decisions: [D-10]
    claims: [CLAIM-CASPER-SOAK-008]
    claim_spec: docs/claims/casper-soak-carrier-index.md
    validation_evidence: docs/casper/cbc-evidence/runs/casper-carrier-index-acceptance-20260919-01/report.json
    binding_status: accepted
    binding_acceptance: docs/casper/cbc-evidence/runs/casper-carrier-index-acceptance-20260919-01/report.json
    hosted_workflow_status: verified
    hosted_workflow_run: 35429044639
    workflow_tag_status: ratified
    evidence_publication_status: verified
    evidence_asset_id: 575126186
    workflow_tag_ratification: docs/casper/cbc-evidence/runs/casper-carrier-index-acceptance-20260919-01/report.json
    completion_evidence: docs/work-logs/task-017-10-carrier-index.md
    completion_blocker: null
    unit_tests: [scripts/casper-soak/tests/carrier_index.rs]
    files:
      - scripts/casper-soak/src/profiles/carrier_index.rs
      - scripts/casper-soak/src/bin/casper-carrier-index.rs
      - scripts/casper-soak/tests/carrier_index.rs
      - scripts/casper-soak/check-carrier-index.sh
      - .github/workflows/casper-carrier-index.yml
      - formal/tlaplus/casper_soak/profiles/carrier_index/CarrierIndex.tla
      - formal/tlaplus/casper_soak/profiles/carrier_index/MC_CarrierIndex.cfg
      - formal/tlaplus/casper_soak/profiles/carrier_index/MC_CarrierIndex_path_unsafe.cfg
      - formal/tlaplus/casper_soak/profiles/carrier_index/MC_CarrierIndex_window_unsafe.cfg
      - formal/tlaplus/casper_soak/profiles/carrier_index/MC_CarrierIndex_counter_unsafe.cfg
      - formal/tlaplus/casper_soak/profiles/carrier_index/verification-plan.jsonc
      - formal/tlaplus/casper_soak/profiles/carrier_index/README.md
    acceptance:
      - "Paired index and reference runs use the same candidate, DAG, window, and availability fixture."
      - "The profile records valid, invalid, and approved carrier cases and supported failure injections."
      - "Fixtures reject unobserved path engagement, mismatched scan windows, and missing counters treated as zero."
      - "Unsupported identity-domain interfaces block those scenarios without authorizing runtime changes."
      - "CLAIM-FINALITY-002 remains an external node claim, not an obligation of this epic."

    completion_gaps: []
    completed_date: 2026-09-19
  - id: TASK-017-11
    title: "Verify protocol and Phlo profile inputs and captured outcomes"
    claims: [CLAIM-CASPER-SOAK-007]
    claim_spec: docs/claims/casper-soak-version-phlo.md
    status: complete
    claimed_by: pi-casper-slashing
    claimed_at: 2026-09-19T18:23:10Z
    previous_claimed_by: pi-soak-carrier-index-linux
    previous_claimed_at: 2026-09-19T15:57:08Z
    work_log: docs/work-logs/task-017-11-version-phlo.md
    implementation_status: controlled-transcript-implemented
    binding_status: passed
    hosted_workflow_status: verified
    hosted_workflow_run: 35459964874
    workflow_tag_status: ratified
    validation_evidence: docs/casper/cbc-evidence/runs/casper-version-phlo-acceptance-20260919-01/report.json
    completion_blocker: null
    unit_tests: [scripts/casper-soak/tests/version_phlo.rs]
    files:
      - scripts/casper-soak/src/profiles/version_phlo.rs
      - scripts/casper-soak/src/bin/casper-version-phlo.rs
      - scripts/casper-soak/tests/version_phlo.rs
      - scripts/casper-soak/check-version-phlo.sh
      - .github/workflows/casper-version-phlo.yml
      - formal/tlaplus/casper_soak/profiles/version_phlo/VersionPhlo.tla
      - formal/tlaplus/casper_soak/profiles/version_phlo/MC_VersionPhlo.cfg
      - formal/tlaplus/casper_soak/profiles/version_phlo/MC_VersionPhlo_versions_unsafe.cfg
      - formal/tlaplus/casper_soak/profiles/version_phlo/MC_VersionPhlo_fields_unsafe.cfg
      - formal/tlaplus/casper_soak/profiles/version_phlo/MC_VersionPhlo_refund_unsafe.cfg
      - formal/tlaplus/casper_soak/profiles/version_phlo/verification-plan.jsonc
      - formal/tlaplus/casper_soak/profiles/version_phlo/README.md
    blocked_by: []
    decisions: [D-01, D-12]
    external_prs: [216, 430]
    acceptance:
      - "The manifest distinguishes Casper protocol from accounting authority."
      - "Generated requests and captured observations retain both phloLimit and phloPrice."
      - "Fixtures expose omitted fields, conflated versions, and ignored settlement mismatches."
      - "Profiles report version rejection, minimum-price, prepayment, refund, and exhaustion outcomes against reviewed fixture expectations."
      - "The harness cannot authorize protocol activation or undefined funding policies."

    completion_gaps: []
    completed_date: 2026-09-19
  - id: TASK-017-12
    title: "Pin executable workloads, qualify candidates, and run the pre-merge baseline soak"
    candidate_review_note: "Current dev b465313a2 has no matching published candidate tag. CI run 35677113121 failed the amd64 subprocess validator lifecycle test and skipped image release. Verified candidate pins still cover 6940a5beb."
    readiness_evidence: docs/casper/cbc-evidence/runs/casper-campaign-readiness-20260922-01/report.json
    integration_fix_evidence: docs/casper/cbc-evidence/runs/casper-integration-timeout-fix-20260922-01/report.json
    integration_fix_status: "PR #450 merged to dev at 0b9ae5bcec94a2df8f6112bbbc6c950ad2e603b2. This checkout inherits the three suite pins. The candidate matrix now records the inherited suite revision and configuration digest."
    candidate_identity_evidence: docs/casper/cbc-evidence/runs/casper-candidate-repin-20260919-01/report.json
    live_admission_evidence: docs/casper/cbc-evidence/runs/casper-linux-admission-7509c831c-01/report.json
    manual_dispatch_evidence: docs/casper/cbc-evidence/runs/casper-campaign-dispatch-5e26ba4c5-01/report.json
    completion_review: docs/work-logs/task-017-12-preparation.md#completion-review-at-859cbc36c
    claim001_reconciliation: docs/work-logs/task-017-12-preparation.md#claim001-specification-digest-reconciliation
    node_interface_prerequisite: docs/plans/casper-node-interface-prerequisite.md
    node_interface_status: "This checkout includes Batch B2 and its recorded claim acceptances through merge 211a4e73a. Selected candidate qualification remains pending. Occurrence-dependent publication qualification belongs to EPIC-018."
    observer_client_claim: docs/claims/casper-authority-observer-client.md
    observer_client_evidence: docs/casper/cbc-evidence/runs/casper-authority-client-20260928-01/report.json
    observer_client_status: "The Linux observer client passes nine controlled tests. Eight mapping tests and ten process receipt tests also pass on Linux. Native client and synthetic profile regressions pass. Candidate qualification remains pending."
    authority_adapter_claim: docs/claims/casper-authority-adapter.md
    authority_adapter_evidence: docs/casper/cbc-evidence/runs/casper-authority-adapter-20260928-01/report.json
    authority_adapter_status: "The client retains exact numeric mappings, separate oracle and persisted observations, and raw work counters. The owned subprocess recorder verifies pause and restart evidence. Controlled verification passes. The live profile remains blocked."
    live_mapping_evidence: docs/casper/cbc-evidence/runs/casper-authority-mapping-20260928-01/report.json
    live_mapping_review: docs/work-logs/task-017-12-mac-continuation.md#authority-interface-mapping-correction-2026-09-28
    live_interface_gaps: "The source mapping is complete. The observer lacks paired fork-choice heads and captured equivocation inputs for display projection. These additions belong to EPIC-019. Existing providers support ordinary pause and restart. Their receipts belong to this task."
    executable_binding_evidence: docs/casper/cbc-evidence/runs/casper-authority-execution-20260928-01/report.json
    executable_binding_status: "All nine scenario kinds execute through the production generator, receipt binding, collector, and classifier with controlled providers. Claim002 requires renewed acceptance. Live qualification remains pending."
    live_executor_claim: docs/claims/casper-authority-live-executor.md
    live_executor_evidence: docs/casper/cbc-evidence/runs/casper-authority-live-20260928-01/report.json
    live_executor_guide: docs/casper/design/authority-live-executor.md
    live_executor_status: "The executor selects the captured head for the member evaluator. The isolated Linux run passes 13 executor, 13 mapper, and 9 observer tests. Candidate qualification and campaign admission remain pending."
    node_interface_mapping_evidence: docs/casper/cbc-evidence/runs/casper-node-interface-adapter-20261001-01/report.json
    node_interface_mapping_work_log: docs/work-logs/task-017-12-node-interface-20261001.md
    live_provider_evidence: docs/casper/cbc-evidence/runs/casper-authority-provider-20260928-01/report.json
    live_provider_guide: docs/casper/design/authority-provider-adaptation.md
    live_provider_status: "The Rust driver submits pinned blocks through the production TLS transport. Owned process receipts connect pause and restart to captures. Explicit successor enrollment supports random node incarnations. Candidate qualification remains pending."
    live_executor_security_review:
      status: assessed_false_positive_pending_maintainer_review
      review: "https://github.com/F1R3FLY-io/f1r3node-rust/pull/436#discussion_r4128609240"
      alert: "https://github.com/F1R3FLY-io/f1r3node-rust/security/code-scanning/41"
      finding: "CodeQL reports a hard-coded cryptographic value at authority_live.rs:769, where capture_attempt starts at zero."
      source_review: "The counter contributes to a request ID through the execution digest. The normal executor supplies fresh entropy in execution_nonce."
      remaining: "Obtain maintainer review of the full SARIF trace and supported correlation-identifier assessment. The remote alert remains open. Caller-supplied nonces have no global freshness guarantee."
      access_limit: "The earlier API query returned HTTP 403. The authenticated read on 2026-10-01 retrieved alert 41 and SARIF analysis 1871610769."
      work_log: docs/work-logs/task-017-12-node-interface-20261001.md#codeql-review
    live_mapping_remaining: "The mapper retains paired heads, separate display inputs, and named work paths. Prepare candidate block histories and captured input exports. Qualify exact traversal measurements and the live provider. Docker fault receipts remain unimplemented."
    candidate_inventory_evidence: docs/casper/cbc-evidence/runs/casper-campaign-inventory-20260928-01/report.json
    candidate_inventory_review: "All 225 model hashes and four configuration hashes match. The matrix and three suite pins select e3c4e14189f0c6ced2e9674487fcbdeffd93141b. Source-bound acceptance remains pending. No workload pin or qualification changed."
    stack_scope: "PR #436 temporarily targets the node branch. This dependency order does not include node implementation in the harness scope. Independent harness controls can proceed before node qualification."
    reservation_work_log: docs/work-logs/task-017-12-reservations.md
    reservation_status: "All sixteen reservation tests pass in an isolated Linux container on this Mac. The authoritative OCI protocol has controlled-provider tests. Its real object and access policy remain unprovisioned."
    execution_control_plan: docs/plans/casper-campaign-execution-controls.md
    execution_control_evidence: docs/casper/cbc-evidence/runs/casper-campaign-control-20260921-01/report.json
    hosted_control_evidence: docs/casper/cbc-evidence/runs/casper-campaign-hosted-58e952c6f-01/report.json
    control_renewal_evidence: docs/casper/cbc-evidence/runs/casper-campaign-renewal-20260928-01/report.json
    control_renewal_status: "Current-source verification passes 133 planner checks, 32 controller tests, three model-runner tests, and 16 isolated Linux reservation tests. The model and five negative controls pass. The workflow evidence digest is refreshed. Acceptance and live qualification remain pending."
    source_coverage_evidence: docs/casper/cbc-evidence/runs/casper-campaign-source-coverage-20260921-01/report.json
    stability_control_evidence: docs/casper/cbc-evidence/runs/casper-campaign-stability-20260922-01/report.json
    publication_gate: "Deferred to EPIC-018 after this branch merges and PR #216 integrates. This is not a PR #216 dependency for this branch."
    phase_boundary_status: "Implemented and locally verified. Pre-merge admission requires authority/finality. Publication and recovery remain explicitly pending for EPIC-018 and cannot count as passed."
    phase_boundary_evidence: docs/casper/cbc-evidence/runs/casper-campaign-phase-20260922-01/report.json
    deployment_proposal: docs/plans/casper-campaign-deployment.jsonc
    storage_deployment_preparation: docs/plans/casper-campaign-storage.md
    storage_preparation_status: "The bucket request and object-specific policy are prepared. The controller service user and inherited policies were reviewed. Workflow credential binding, supervisor identity, final configuration, deployment, and live qualification remain pending."
    continuation_work_log: docs/work-logs/task-017-12-mac-continuation.md
    execution_control_status: "Hosted run 35752941906 passes at 58e952c6f. The archive digest and all 51 source hashes match. It covers the stability and phase changes. Deployment, timing qualification, live execution, and acceptance remain pending."
    github_access_status: "Explicit GITHUB_PERSONAL_ACCESS_TOKEN selection verifies all six reviewer roles. Default GITHUB_TOKEN selection still returns HTTP 403 for role queries. The authorized campaign environment is configured and its branch restriction is verified."
    github_environment_evidence: docs/casper/cbc-evidence/runs/casper-campaign-environment-20260922-01/report.json
    compatibility_lookup_status: "The three campaign inventories have 37 unique pending artifact records and matching compatibility links. Claims001 and 002 now remain pending. Claim002 preserves its historical acceptance."
    execution_gate: "Four exact-candidate probes returned blocked before node launch. Their controlled inputs are not live qualification. The legacy workload is pinned but cannot replace required Casper profiles."
    drift_review: docs/work-logs/task-017-12-drift-review-2026-09-19.md
    claims: [CLAIM-CASPER-SOAK-001, CLAIM-CASPER-SOAK-002, CLAIM-CASPER-SOAK-004, CLAIM-CASPER-CAMPAIGN-001, CLAIM-CASPER-CAMPAIGN-002, CLAIM-CASPER-CAMPAIGN-003, CLAIM-CASPER-AUTHORITY-CLIENT-001, CLAIM-CASPER-AUTHORITY-ADAPTER-001, CLAIM-CASPER-AUTHORITY-LIVE-001]
    claim_index: docs/claims/casper-soak-harness.md
    campaign_claim_index: docs/claims/casper-soak-campaign.md
    reservation_claim_index: docs/claims/casper-campaign-reservation.md
    status: in_progress
    claimed_by: claude-session-f3cbc961
    claimed_at: 2026-10-01T06:00:00Z
    claim_chain: ["pi-soak-carrier-index-linux (to 2026-09-28)", "codex-task-017-12-20260928 (2026-09-28 to 2026-10-01)", "pi-session-01a0ab62-71b3-7248-a800-37a6fde2e4fa (2026-10-01, mapping slice 2bfd6d88d)", "claude-session-f3cbc961 (from 2026-10-01)"]
    claim_transfer: "The user transferred the task to the Batch E owner on 2026-10-01. The EPIC-019 prerequisites TASK-019-3 and TASK-019-4 are complete, and the merge c9ca12821 brings the paired fork-choice observation (claim 004) and the detached display projection (claim 005) to this branch."
    handoff_2026_10_01_from_agent_b:
      delivered: "Commit 2bfd6d88d: separate display and oracle digests, paired evaluator heads, named fork-choice work paths, captured display metadata, live snapshot head selection. 35 isolated Linux tests and 12 host profile tests passed. Report docs/casper/cbc-evidence/runs/casper-node-interface-adapter-20261001-01/report.json."
      codeql_alert_41: "Assessed as a false positive: the retry counter at authority_live.rs:769 feeds a request identifier, not a key or nonce. The remote alert stays open. Maintainer review pending."
      work_log: docs/work-logs/task-017-12-node-interface-20261001.md
    critical_path_2026_10_01:
      finding: "ci.yml publishes node images only on pushes to dev, master, or a v tag. No candidate image contains the observer (claims 004 and 005) until PR #451 and PR #447 merge to dev. The matrix pins node 6940a5beb, which predates the observer."
      order: "PR #451 to dev, PR #447 to dev (TASK-019-6), dev image publication (TASK-019-7), candidate repin and workload pin, controlled and live qualification, maintainer acceptance, preflight and baselines (user dispatch)."
      controlled_preparation: "A local image from node/Dockerfile on this branch supports the authority_finality workload pin, the captured input exports, the candidate block histories, and the exact traversal qualification in controlled mode. It is not an immutable candidate identity."
    controlled_preparation_2026_10_01:
      evidence: docs/casper/cbc-evidence/runs/casper-controlled-preparation-009262781-01/report.json
      evidence_sha256: 0fd60687d7dc5e1d2933e7099ad3dc0b75e61d2d5868d015213428ae043a8647
      status: controlled-pinned-unqualified
      result: "A release build of 009262781 ran in a Linux container. The owner launched a non-validator target node. The p2p driver delivered a 5-block single-validator history. Two manual captures and a 4-operation casper-authority-live run (4 receipts, 8 captures, zero errors) show the paired heads, the display projection, and the equivocation capture end to end. The executor status is incomplete because the p2p driver reports unknown and exports no observed inputs."
      adapter_findings: "C1 owner-only observer access, C3 non-validator target, C4 capture not synchronized with block processing, C5 applied path needs a driver with input exports, C9 clear the observer directory before each launch."
      qualifies_candidate: false
      work_log: docs/work-logs/task-017-12-node-interface-20261001.md#controlled-preparation-results-on-2026-10-01
    handoff_scope_2026_10_01:
      agent_now: "Map the Batch D fields (fork_choice bounded and reference heads, comparison, work paths 4 and 5) and the Batch E fields (display inputs, display projection, equivocation capture) in the harness profile. Prepare candidate block histories and captured input exports. Qualify the provider in controlled tests. Triage the CodeQL finding at authority_live.rs:769."
      user_decisions: "OCI runner dispatches (two 24-hour baselines, the 60-hour campaign), the authoritative OCI object and supervisor provisioning, and live admission stay with the user. No dispatch follows from this transfer."
      critical_path: "TASK-017-13 reviews this task's evidence. TASK-017-14 and TASK-017-15 chain behind it."
    previous_claimed_by: pi-soak-carrier-index-linux
    previous_claimed_at: 2026-09-19T19:20:00Z
    handoff_note: docs/handoffs/claude-session-9f19b46c--pi-soak-carrier-index-linux--20260919T192000Z.md
    execution_scope: "Qualify the pre-merge candidate capabilities under the corrected campaign phase boundary. This branch merges before PR #216 integrates. Occurrence-dependent publication and recovery qualification belong to EPIC-018. A passing preflight must precede both full baselines."
    repin_tool: scripts/ci/resolve-dev-candidate.sh
    dispatch_preconditions: "docs/work-logs/task-017-12-preparation.md#dispatch-preconditions"
    work_log: docs/work-logs/task-017-12-preparation.md
    blocked_by: [TASK-019-6, TASK-019-7]
    blockers_cleared: "TASK-019-3 and TASK-019-4 complete and accepted on 2026-09-23. The EPIC-019 observer additions landed on this branch in c9ca12821."
    remaining_prerequisites:
      - "The changed workflow and campaign artifacts have current pending records. Historical evidence remains unchanged. Claim001 and the three campaign claims still require source-bound acceptance."
      - "Qualify a live executor against the selected node with the implemented receipt binding. Client, adapter, and renewed Claim002 acceptance remain pending."
      - "Pin executable workloads and accept the refreshed campaign inventory. All 225 model hashes and four configuration hashes match after suite reconciliation."
      - "Qualify the required pre-merge adapters and node interfaces. The corrected admission requires authority/finality and retains pending occurrence profiles. Preserve the earlier blocked probe evidence."
      - "Hosted run 35752941906 covers the published stability and phase changes at 58e952c6f. The campaign workflow still requires qualification against deployed services."
      - "The campaign-baseline-24h input requests 86400 workload seconds. Execution must preserve that full duration without preflight subtraction. Existing scheduled behavior remains unchanged."
      - "Provision and qualify the authoritative OCI object and independent supervisor. The implementation does not establish deployed timing guarantees. Missing activation evidence blocks execution."
      - "The user authorized both architectures on 2026-09-22. Each stability runner has 64 GB and a 64-hour maximum lifetime. Both full baselines must pass first."
      - "The memory decision is resolved at 64 GB per runner. Preserve the approved limits and host controls."
    deferred_after_branch_merge:
      - "Integrate PR #216 after this branch merges. Qualify occurrence-dependent publication and recovery under EPIC-018 with actual occurrence records."
      - "Keep unavailable occurrence profiles pending. Do not report deferred qualification as a pass or infer occurrences from deploy signatures."
    related_epics: [EPIC-010, EPIC-013]
    resource_approval: "The user amended the approval on 2026-09-19. Separate preflight and baseline dispatches may use one 64 GB preflight runner for four hours and two 64 GB baseline runners for 26 hours each. Each candidate receives one full 24-hour baseline. The user also approved a 60-hour TASK-017-12 campaign after a passing baseline. On 2026-09-22, the user authorized one 64 GB stability runner per architecture, each with a 64-hour maximum lifetime. Both baselines must pass first. Additional repetitions remain unapproved."
    resource_approval_original: "A maintainer approved the resource proposal on 2026-09-19 at 2026-09-19T07:03:24Z. The approval covered two candidates, one preflight, and two runner virtual machines for 26 hours each. The later approval amends that machine count and authorizes the 60-hour campaign."
    resource_approval_memory: "The maintainer approved 64 GB per runner on 2026-09-19, which corrects the 48 GB figure in the original proposal. The soak workflow already sets RUNNER_MEM_GB_OVERRIDE to 64. The sizing invariant needs about 60,416 MB, from a 45,056 MB ceiling, about 7,168 MB of host overhead, and an 8,192 MB floor. A 48 GB machine overruns that by about 11 GB, and a ceiling small enough to fit falls below the measured 36,008 MB healthy peak. No workflow or runtime file changes."
    resource_approval_record: docs/work-logs/task-017-12-preparation.md
    first_dispatch_step: "After source acceptance, deployment qualification, and live admission pass, run campaign-preflight before either baseline. A non-passing preflight blocks both baseline dispatches."
    acceptance:
      - "The maintainer approves the resource budget, durations, repetitions, and candidate matrix before dispatch."
      - "A preflight-only dispatch on this branch passes before any baseline soak dispatch. Its run ID and outcome are recorded."
      - "Every dispatched candidate has qualified interfaces and immutable executable workload, node, harness, model, image, and configuration identities."
      - "Required pre-dispatch harness and profile verification must pass. Missing capabilities and null workload pins block dispatch."
      - "The disk-protected harness completes required pre-merge baseline profiles or reports an explicit non-passing outcome."
      - "Unavailable #216 candidate profiles remain pending for EPIC-018 and are not reported as passes."
      - "Every report retains exact revisions, seeds, run IDs, configuration, metrics, and artifact digests."
      - "Infrastructure termination does not erase prior product failures."
      - "Deferred policy findings return to the team for a separate decision."

  - id: TASK-017-13
    title: "Close pre-merge CbC scope and hand off post-merge obligations"
    claim_index: docs/claims/casper-soak-harness.md
    status: in_progress
    claimed_by: claude-session-f3cbc961
    claimed_at: 2026-09-28T20:30:00Z
    previous_claimed_by: pi-session-01a0afde-d35c-70a2-b8c9-39aa11cbdfca
    previous_claimed_at: 2026-09-21T14:31:52Z
    claim_transfer_note: "The maintainer assigned this task to claude-session-f3cbc961 on 2026-09-28 for work in parallel with TASK-017-12. The two tasks commit together."
    execution_scope: "Deliver the independent formal gate on dev and verify the approved protection change. Review TASK-017-12 evidence before final handoff acceptance."
    current_review: docs/casper/cbc-evidence/runs/casper-pre-merge-review-20260928-01/report.json
    current_review_status: "The review at 211a4e73a covers 181 changed mandatory artifacts. The default gate reports 50 gaps and the Casper-directory diagnostic reports 61. Both gates exit 4. Claim001 is pending and seven profile claims are discharged."
    gap_owners: "TASK-017-12 owns 38 gaps in campaign artifacts. EPIC-019 owns 10 node observation gaps, which need named maintainer acceptance of the cycle 03 package. This task owns 2 formal-gate gaps."
    handoff_acceptance_maintainer: "@jltatbeach"
    gate_candidate_status: "A gate-only candidate against dev at 0b9ae5bce passes its fixture suites in scratch files. The maintainer decided on 2026-09-28 that the gate branch is ci/soak-obligation-gate, which starts from fix/soak-finalization-attribution. That start point already has the formal gate. The formal gate thus reaches dev with the stack merge, and the candidate stays a record."
    campaign_60h_scope: "The maintainer decided on 2026-09-28 that the 60-hour phase is not a closure requirement for this branch or for PR #436. The phase cannot run before the branch is merged. TASK-018-7 owns the run and its record after the changes are in master."
    work_log: docs/work-logs/task-017-13-gate-handoff-2026-09-21.md
    preparation_log: docs/work-logs/task-017-13-preparation.md
    handoff_note: docs/handoffs/casper-pre-merge-to-post-merge-20260919.md
    handoff_preparation_status: "Prepared for review, not accepted. Baseline delivery and named recipients remain pending."
    handoff_preparation_review: docs/casper/cbc-evidence/runs/casper-handoff-preparation-20260919-02/report.json
    evidence_review: docs/casper/cbc-evidence/runs/casper-pre-merge-review-20260919-01/report.json
    canonical_record_review: docs/casper/cbc-evidence/runs/casper-formal-gate-documentation-20260919-01/report.json
    canonical_record_status: "The review at 2530b8385 reports Claim001 pending and seven profile claims discharged. All eight soak fields remain pending."
    compatibility_review: docs/casper/cbc-evidence/runs/casper-compatibility-routing-20260919-01/report.json
    compatibility_status: "The current default gate reports 29 gaps across 149 changed mandatory artifacts. The canonical diagnostic reports 28 gaps. Both gates exit 4."
    formal_gate_implementation: docs/work-logs/soak-formal-gate-implementation.md
    formal_gate_hosted_renewal: docs/work-logs/soak-formal-gate-hosted-renewal.md
    formal_gate_authorization: docs/work-logs/soak-formal-gate-authorization.md
    blocked_by: [TASK-017-12]
    remaining_prerequisites:
      - "Make the verified formal gate available on dev. Then apply the approved protection change, verify enforcement, and record evidence-backed acceptance."
      - "Complete CLAIM-SOAK-GATE-001 under docs/claims/soak-formal-gate.md. The approved finalized-floor scope remains local-only."
      - "Review current-source registration and discharge for all campaign controls owned by TASK-017-12, including CLAIM-CASPER-CAMPAIGN-001, CLAIM-CASPER-CAMPAIGN-002, and CLAIM-CASPER-CAMPAIGN-003."
      - "Review baseline results, scan benchmark evidence, and concurrency-gate evidence. The 60-hour phase is a post-merge obligation under TASK-018-7, per campaign_60h_scope."
      - "TASK-018 owners are confirmed as of 2026-09-22: @jeffrey-l-turner or @jltatbeach for each task. Obtain acceptance of the completed handoff. Stacked branch preparation does not satisfy post-merge discharge requirements."
    decisions: [D-11]
    acceptance:
      - "Every changed mandatory artifact has current pre-merge claim evidence or an explicitly approved waiver."
      - "Strict discharge passes for the actual pre-merge changed scope, not only for the planning documents."
      - "Check every required claim ID, source digest, phase, and tier. One legacy artifact status cannot discharge the claim bundle."
      - "The scan benchmark and concurrency-gate obligations have baseline evidence and named post-merge rerun tasks."
      - "The handoff lists model and artifact digests, seeds, assumptions, pending bindings, and TASK-018 owners."
      - "The review separates bounded harness models, executable profile fixtures, and observed product outcomes. Node proofs are outside scope."
      - "No required pre-merge claim is deferred merely to close this epic. Post-merge obligations stay pending, not waived."

  - id: TASK-017-14
    title: "Reduce the branch diff to the formal-verification deliverables"
    status: complete
    claimed_by: claude-session-aa467dea
    claimed_at: 2026-10-03T16:05:00Z
    completed_at: 2026-10-03T16:23:26Z
    completion_record: "Commits 90f94317d and 3bdd523cc moved 1,216 evidence files to the published release cbc-evidence-epic-017. The PR #436 diff against master fell from 2,048 files and 307,232 added lines to 889 files and 92,225 added lines. The maintainer jltatbeach confirmed the reduced diff at 3bdd523cc: https://github.com/F1R3FLY-io/f1r3node-rust/pull/436#issuecomment-5971043357"
    previous_claimed_by: claude-session-9f19b46c
    execution_scope: "Steps 0 through 7. The user transferred the task to this session on 2026-10-03 and lifted the TASK-017-13 block, because PR #436 is now the bottom of the stack (PR #451 and PR #447 merged) and its diff against master is 2,048 files and 307,232 added lines."
    work_log: docs/work-logs/task-017-14-preparation.md
    blocked_by: []
    previous_blocked_by: [TASK-017-13]
    created_at: 2026-09-17
    rationale: "At 6814682e4 the branch differed from origin/dev by 762 files and about 46,000 added lines. At 490d21093, PR #436 against docs/consensus-neutral-execution shows 1,243 files and about 179,500 added lines. Evidence run packages are 948 of those files and 158,820 of those lines. That diff is too large for the repository PR review standard."
    measured_at: 490d21093
    evidence_store: "Draft GitHub release cbc-evidence-epic-017 on this repository, created 2026-09-19 with 24 assets: one bundle per package, SHA256SUMS, and index.json. Publish it when the reduction commit lands."
    stack_review_baseline: docs/casper/cbc-evidence/runs/casper-stack-integration-20260917-01/report.json
    removal_targets:
      - "Evidence run packages under docs/casper/cbc-evidence/runs/: 23 packages, 948 files, 29 MB, of which 32 archives are 15 MB. Six packages are live because a canonical ledger record cites them. Seventeen are historical and only work logs or this tracker cite them."
      - "Files applied verbatim from PR #430 through PR #433. Resolved: the PR base now contains those merges, and only scripts/bench/test-soak-disk-admission.sh and its record remain in the diff as driver-repair deliverables."
      - "Compatibility symlinks in docs/cbc-evidence/: 80 links. Remove them when the CbC driver supports module routing. Otherwise keep the canonical record only."
      - "Historical hosted TLC transcripts recovered in the evidence audit. Keep the audit report and digests, not the transcript copies."
      - "Work logs: 21 files and about 2,300 lines. TASK-017-4 alone has four logs. Fold each task's logs into one log that keeps handoff and decision content."
    implementation_plan:
      - "Step 0. Apply the evidence retention rule in docs/plans/casper-ratified-soak-2026-09-16.md to every package created after 2026-09-19, so that no new package adds bulk while this task waits on TASK-017-13."
      - "Step 1. Choose the external evidence store and record its location format. Upload every archive, transcript set, candidate-ledger set, attempt, and upstream snapshot from the 23 existing packages. Record each upload as a path plus SHA-256."
      - "Step 2. Live packages (driver-rebind, driver-rebind-acceptance, driver-refresh-acceptance, profile-binding-review, profile-acceptance, manifest-resume, harness-controls): keep report.json, validation.json, and the digest lists in the tree. Replace each in-tree archive reference in the canonical ledger records, including previous_ledger references, with the external location and digest. The strict claims audit hashes the cited report.json and reads source digests from it, so those reports stay in the tree and the audit result does not change."
      - "Step 3. Historical packages (the other sixteen): keep report.json only, or remove the package and record its external location and digest in the work log or tracker entry that cites it."
      - "Step 4. Compatibility symlinks: identify every consumer of docs/cbc-evidence/. If the shared CbC gate accepts module paths, delete all 80 links. If it does not, add module routing to the gate in one change and then delete the links."
      - "Step 5. Work logs: consolidate per task. Keep the acceptance records, the human decision records, and handoff notes. Drop run-by-run narrative that a retained report already records."
      - "Step 6. Verify. Run the strict claims audit, the bindings inventory, the link check, and the STE check before and after. Record the before and after file and line counts against the PR base and against dev in this entry."
      - "Step 7. Land the reduction as one commit with the counts in its message. Ask the maintainer to confirm the reduced diff before PR #436 leaves draft."
    expected_result: "Measured in the 2026-09-19 rehearsal: evidence files 1,049 to 78 and evidence lines 185,612 to 19,777, with the strict audit unchanged. Projected PR diff about 375 files and about 41,000 added lines. The remainder is the crate, the models, the claims, the ledger records, and the retained reports, which are the deliverables."
    acceptance:
      - "The PR diff against its confirmed stack parent contains only Casper soak harness deliverables. The report also measures the cumulative diff against dev."
      - "Record the actual stack parent revision and PR base after integration. Planned stack membership alone cannot justify file deletion."
      - "Each removed file is either reproducible from a recorded digest and revision, or duplicated upstream on dev, or listed with an explicit reason."
      - "No removal changes a claim status, a ledger record status, or a discharge result. Pending claims stay pending."
      - "Evidence that a claim cites remains reachable. A record names the external location and digest of any artifact that leaves the tree."
      - "The task records the file and line counts of the diff before and after reduction."
      - "The link check, the STE check, and the strict CbC gate produce the same results after reduction as before it."
      - "The maintainer confirms the reduced diff meets the PR review standard before the PR opens."

  - id: TASK-017-17
    title: "Enforce the node log budgets in the soak guardian"
    status: done
    completed_at: 2026-10-05
    reopened_at: 2026-10-05
    resolution: "See TASK-020-4. Accepted in PR #622 comment 5983133741, and the probe fix in comment 6005866151."
    claimed_by: claude-session-aa467dea
    claimed_at: 2026-10-04T14:36:28Z
    claim_history: "claude-session-f3cbc961 claimed the task on 2026-10-02 and committed the design (e7e376a69). The user transferred the claim on 2026-10-04 for implementation on chore/finish-TASK-020-4-log-growth."
    created_at: 2026-10-02
    mirror_of: TASK-020-4
    implementation_status: "See TASK-020-4. Implemented on chore/finish-TASK-020-4-log-growth on 2026-10-04 and accepted. The probe fix of 2026-10-05 was accepted in PR #622 comment 6005866151."
    branch: formal/soak-casper-consensus
    design: docs/casper/design/soak-log-budget-guardian.md
    placement_note: "Recorded before TASK-017-15 so that the TASK-017-16 record of branch 4 merges without a conflict."
    blocked_by: []
    scope: "The host guardian samples the container json-file log and the node log directory of each owned node container and stops the run when either exceeds its budget. A fixture covers the budgets, the probe failures, and descriptor exhaustion. CLAIM-SOAK-001 records the log caps as a checked invariant."
    files:
      - scripts/run-merge-recovery-soak.sh
      - scripts/bench/test-soak-log-budget.sh
      - .github/workflows/ci.yml
      - scripts/ci/check-casper-soak-bindings.sh
      - docs/claims/soak-disk-protection.md
    acceptance:
      - "The guardian samples the container log bytes and the node log directory bytes of each owned container every 15 seconds, with budgets SOAK_CONTAINER_LOG_BUDGET_MB (default 400) and SOAK_NODE_LOG_BUDGET_MB (default 2560). A value of 0 disables a probe."
      - "Three consecutive samples over a budget, or one sample at two times a budget, is a breach: breach record, writers stopped, health tag stamped, run refused. An unreadable probe refuses admission or records a breach during execution."
      - "scripts/bench/test-soak-log-budget.sh passes its 10 scenarios in CI, including descriptor-exhaustion."
      - "CLAIM-SOAK-001 names the log budgets as a checked invariant with the fixture as evidence. The disk floor, the memory floor, and the stop path are unchanged."
      - "The records of the driver and the fixture refresh in the TASK-017-13 ledger cycle."
  - id: TASK-017-15
    title: "Retrieve and publish the external evidence release"
    status: pending
    claimed_by: null
    claimed_at: null
    blocked_by: [TASK-017-13, TASK-017-14]
    created_at: 2026-09-19
    ordering: "Final task for this branch and for PR #436. Run step 2 after the reduction commit lands and before the PR leaves draft."
    evidence_store: "Draft GitHub release cbc-evidence-epic-017, release ID 391939637, target 09b0a60063815b750979864225a0a56387d6ed80, 24 assets as of 2026-09-19."
    execution_scope: "Step 1 needs no new authorization and unblocks evidence consumers today. Step 2 exposes evidence on a public surface and requires recorded maintainer authorization at the time of the action."
    rationale: "A draft release carries no Git tag. The endpoint repos/F1R3FLY-io/f1r3node-rust/releases/tags/cbc-evidence-epic-017 returns 404 for every credential. That result blocked carrier-index retrieval under TASK-017-10 on 2026-09-19. The numeric release ID resolves the same release and lists its 24 assets. A branch merge does not create the tag, because publication creates it."
    implementation_plan:
      - "Step 1. Address the release by its numeric ID while it stays a draft. List assets through repos/F1R3FLY-io/f1r3node-rust/releases/391939637. Download each asset by its asset ID with an octet-stream accept header. Record the retrieval check in the consuming task's evidence."
      - "Step 2. Publish the release after the reduction commit lands. Confirm or update the target revision first, because publication creates the tag at that commit. Publication makes the by-tag endpoint work and makes the assets publicly visible."
    acceptance:
      - "Every consumer of the external evidence store addresses the release by its ID while the release stays a draft."
      - "No task records a credential failure for a draft-release lookup. Each record names the absent tag as the cause."
      - "Publication happens only after the reduction commit lands and only with recorded maintainer authorization."
      - "The published release pins the revision that the reduction commit produced. The record names that revision."
      - "Asset digests after publication match the digests recorded at upload time."
      - "Publication does not change a claim status, a ledger record status, or a discharge result."
  - id: TASK-017-16
    title: "Stack cleanup on fix/soak-finalization-attribution before the stack merge"
    status: pending
    claimed_by: null
    created_at: 2026-09-30
    recorded_by: claude-session-f3cbc961
    recorded_on: 2026-09-30
    branch: fix/soak-finalization-attribution
    pull_request: 441
    blocked_by: [TASK-017-14, TASK-019-8]
    origin: "The maintainer decided on 2026-09-30 that the cleanup of the work logs, the evidence, and the file count occurs on this branch. EPIC-020, this branch, ci/soak-obligation-gate, and feature/randomized-exercise-soak had no cleanup task."
    scope: "Reduce the documents and the evidence of the stack to the deliverables and the records that they cite. Production code scope is not in this task."
    baseline:
      measured_at: 93b87d802
      diff_against_dev_files: 2225
      tree_files: 4651
      docs_casper_cbc_evidence: "1,468 files and 50.8 MiB"
      archives_in_docs: "52 archives and 26.5 MiB"
      files_larger_than_256_kib: "36 files and 43.0 MiB"
      work_logs: "60 files. TASK-017-4 has 10 logs."
      compatibility_links: 190
    rules:
      - "Keep each file that an accepted claim, an evidence record, a test, or the tracker cites by path or digest."
      - "Keep report.json, validation.json, and the digest lists of each evidence package that a record cites. Bulk evidence stays outside Git."
      - "Keep one work log for each task, with the decisions, the acceptance records, and the open findings."
      - "Record each removal with its reason, its external location, and its digest."
      - "Do not remove a file that a lower branch of the stack still changes. A removal of such a file causes a conflict in each merge round."
    stack_refresh: "Decision 2026-09-30: the next merge round starts at the bottom of the stack. dev merges into fix/node-log-and-accept-backoff first, then each branch merges into the branch above it. This task runs after that round reaches this branch."
    implementation_plan:
      - "Step 1. Wait for TASK-017-14 on formal/soak-casper-consensus and for TASK-019-8 on feature/casper-node-observation. Wait for the merge round that carries their results to this branch."
      - "Step 2. Make an inventory of each docs/, formal/, and .github/ file of the diff against dev. Classify each file as a deliverable, a cited record, or removable."
      - "Step 3. Remove the removable files of EPIC-020 and of this branch. Put the run narrative of each work log into its results table."
      - "Step 4. Do the same inventory and removal on ci/soak-obligation-gate and on feature/randomized-exercise-soak for the files that only those branches have."
      - "Step 5. Run the strict claim audit, the link check, and the STE Check before and after. Require equal results."
      - "Step 6. Record the file count, the line count, and the size of the diff against dev before and after."
    acceptance:
      - "No removal changes a claim status, a record status, a tier field, an audit result, a test, or a production file."
      - "The diff against dev has only the deliverables and the records that they cite."
      - "The tree has no archive and no file larger than 256 KiB in docs/, or the record gives the reason for each exception."
      - "The maintainer confirms the reduced diff before the stack merge."
---
```

**Current state:** TASK-017-1 through TASK-017-4 are complete. The repaired lifecycle binding is accepted. Three controlled-transcript profiles await acceptance. Four profiles remain unimplemented. Baseline soaks remain pending.

**Approved sequence:** TASK-017-4 and TASK-017-5 through TASK-017-11 may proceed together against the completed contract and applied prerequisites. TASK-017-12 requires final workload pins, qualification, verification, and dispatch approval.

**Verified stack:** Merge `0f1ccdf38` includes PR #433 at `65f7f6daa`. PR #436 targets `docs/consensus-neutral-execution`. CLAIM-CASPER-SOAK-001 passes strict discharge. The seven profile claims remain pending.

**Tracker compatibility:** The shared CLI still rejects TASK-* identifiers. The completion review invokes its unchanged task function for the three reviewed preparation tasks. The TASK-017-4 adapter remains unchanged.

The [interface contract](https://github.com/F1R3FLY-io/f1r3node-rust/blob/ba9758507194d6e34bc1494d416b87405facd550/docs/casper/design/soak-interface-contract.md) (local Git: `ba9758507194d6e34bc1494d416b87405facd550:docs/casper/design/soak-interface-contract.md`) records exact payloads, source boundaries, fixture expectations, and missing capabilities. The local model alone cannot discharge a harness claim. The accepted lifecycle records include executable bindings and source-specific review.

**Scope:** This epic covers the pre-#216 PR only. The [branch plan](https://github.com/F1R3FLY-io/f1r3node-rust/blob/ba9758507194d6e34bc1494d416b87405facd550/docs/plans/casper-ratified-soak-2026-09-16.md) (local Git: `ba9758507194d6e34bc1494d416b87405facd550:docs/plans/casper-ratified-soak-2026-09-16.md`) records both phases and their evidence boundary.

**Final task:** TASK-017-15 closes the branch and PR #436. TASK-017-16 reduces the documents and the evidence of the stack on `fix/soak-finalization-attribution` (PR #441) before the stack merge. Evidence consumers address the draft release by its ID until then. Release publication is the last action, and it follows the reduction commit.

---

### EPIC-018: Post-Merge Casper Soak Formal Verification

```yaml
---
epic_id: EPIC-018
title: "Post-Merge Casper Soak Formal Verification"
status: blocked
priority: p0
user_story: null
blocked_by: [EPIC-017]
created_at: 2026-09-16
updated_at: 2026-09-16
claimed_by: null
claimed_at: null
phase: post_pr216_merge
claim_index: docs/claims/casper-soak-harness.md
formal_plan: formal/tlaplus/casper_soak/verification-plan.jsonc
proposed_branch: formal/soak-casper-post-cost-accounting
plan: docs/plans/casper-ratified-soak-2026-09-16.md
related_epics: [EPIC-010, EPIC-013, EPIC-017]
post_merge_obligations:
  record: https://github.com/F1R3FLY-io/f1r3node-rust/issues/473
  label: post-merge-obligation
  decided_on: 2026-09-28
  rule: "One issue records all obligations that stay open when the stack merges into master. The issue closes only when each obligation has passing evidence or a recorded waiver."
  automation_plan:
    branch: ci/soak-obligation-gate
    base_branch: fix/soak-finalization-attribution
    status: in_progress
    created_on: 2026-09-28
    created_from: 566841b16
    stack_position: "Between fix/soak-finalization-attribution and feature/randomized-exercise-soak."
    location_history: "The first decision of 2026-09-28 put the branch on top of the stack, from feature/randomized-exercise-soak. The maintainer changed the position on the same day, before the branch creation."
    instructions:
      - "The maintainer created ci/soak-obligation-gate from fix/soak-finalization-attribution."
      - "Open its PR against fix/soak-finalization-attribution."
      - "Change the base of PR #189 to ci/soak-obligation-gate."
      - "Merge ci/soak-obligation-gate into feature/randomized-exercise-soak in each merge round of the stack."
      - "Use this branch to implement automatic result recording for all stack obligations in issue 473."
    implemented_by: claude-session-f3cbc961
    files:
      - .github/workflows/soak-obligation.yml
      - scripts/ci/check-soak-obligation.sh
      - scripts/ci/test-check-soak-obligation.sh
    work_log: docs/work-logs/task-017-13-gate-handoff-2026-09-21.md#obligation-gate-branch-on-2026-09-28
    implementation_status: "The check for obligation O1 and its controls are implemented. The controls pass locally. No hosted run and no real soak evidence exist."
    automated_obligations: [O1]
    evidence_gap: "The campaign result record has no seeds and no window times. The check marks obligation O1 only when the record has the fields seeds, started_epoch, and finished_epoch. These field names are a proposal to the owner of the campaign control."
    not_in_scope: "The release gate in release.yml needs the agreement of the maintainers. The check does not close issue 473."
  obligations:
    O1: TASK-018-7
    O2: TASK-018-1
    O3: TASK-018-2
    O4: [TASK-018-3, TASK-018-4]
    O5: TASK-018-5
    O6: TASK-018-6
    O7: CLAIM-SOAK-GATE-001
    O8: TASK-017-15
external_dependencies:
  - repo: F1R3FLY-io/f1r3node-rust
    pr: 216
    required_state: open_or_merged
    required_ancestor_of: origin/dev
    ancestor_required_by: "merge of the EPIC-018 follow-on pull request"
execution_contract:
  base_branch: dev
  scope: "A separate follow-on formal-methods PR for the soak harness, stacked on PR #216."
  start_gate: "EPIC-017 handoff accepted. The follow-on branch may start from the PR #216 head before that pull request merges."
  branch_policy: "Cut the follow-on branch from the PR #216 head and target that pull request. Retarget it to dev after PR #216 merges. Record the PR #216 head revision at branch creation, the actual merge revision, and the baseline revisions."
  amendment_2026_09_19: "The maintainer amended this contract on 2026-09-19 to allow stacking. The earlier contract required PR #216 to be merged and an ancestor of origin/dev before EPIC-018 started, and it stated that an open candidate head was insufficient. Stacking lets the follow-on work begin earlier. It also means the branch starts from a revision that can still change, so the owner rebases on each PR #216 update and records the revision it verified against. Discharge of a post-merge claim still requires the actual merge revision."
  completion_policy: "Require updated harness models, profile fixtures, completed soaks, and harness-only CbC discharge. No node proof is required."
  authority: "Merge does not authorize deferred policies, waive ratification conditions, or replace FIPS activation approval."
files:
  - scripts/run-merge-recovery-soak.sh
  - scripts/bench/test-run-merge-recovery-soak.sh
  - scripts/casper-soak/src/manifest.rs
  - scripts/casper-soak/tests/manifest.rs
  - scripts/bench/write-soak-summary.sh
  - scripts/ci/check-tla-invariants.sh
  - .github/workflows/merge-recovery-soak.yml
  - formal/tlaplus/casper_soak/README.md
  - formal/tlaplus/casper_soak/verification-plan.jsonc
  - formal/tlaplus/casper_soak/CasperSoakHarness.tla
  - formal/tlaplus/casper_soak/MC_CasperSoakHarness.cfg
  - formal/tlaplus/casper_soak/MC_CasperSoakHarness_identity_unsafe.cfg
  - formal/tlaplus/casper_soak/MC_CasperSoakHarness_resume_unsafe.cfg
  - formal/tlaplus/casper_soak/MC_CasperSoakHarness_failure_unsafe.cfg
  - formal/tlaplus/casper_soak/MC_CasperSoakHarness_evidence_unsafe.cfg
  - formal/tlaplus/casper_soak/MC_CasperSoakHarness_stop_unsafe.cfg
  - formal/tlaplus/casper_soak/MC_CasperSoakHarness_cleanup_unsafe.cfg
  - formal/tlaplus/casper_soak/MC_CasperSoakHarness_policy_unsafe.cfg
  - formal/tlaplus/casper_soak/MC_CasperSoakHarness_samples_unsafe.cfg
  - formal/tlaplus/casper_soak/MC_CasperSoakHarness_merge_unsafe.cfg
  - formal/tlaplus/casper_soak/MC_CasperSoakHarness_control_unsafe.cfg
  - scripts/ci/check-casper-soak-models.sh
  - scripts/ci/check-casper-soak-bindings.sh
  - scripts/casper-soak/Cargo.toml
  - scripts/casper-soak/src/lib.rs
  - scripts/casper-soak/src/main.rs
  - scripts/casper-soak/src/models.rs
  - scripts/casper-soak/src/runtime.rs
  - scripts/casper-soak/tests/models.rs
  - scripts/casper-soak/tests/driver.rs
  - scripts/bench/casper-soak.sh
  - scripts/bench/fixtures/casper-lifecycle-executor.sh
  - .github/workflows/slashing-tests.yml
  - scripts/casper-soak/src/bin/check-casper-bindings.rs
  - scripts/casper-soak/tests/bindings.rs
  - scripts/casper-soak/tests/interruption.rs
  - scripts/casper-soak/src/bin/check-casper-claims.rs
  - scripts/casper-soak/tests/claims.rs
  - scripts/casper-soak/task-complete.sh
tasks:
  - id: TASK-018-1
    title: "Verify the merge gate and establish the post-merge baseline"
    claims: [CLAIM-CASPER-SOAK-001]
    status: blocked
    claimed_by: null
    assigned_to: ["@jeffrey-l-turner", "@jltatbeach"]
    assigned_on: 2026-09-22
    assignment_note: "The maintainer confirmed on 2026-09-22 that either handle may own any EPIC-018 task. The implementer claims the task with claimed_by when work starts."
    blocked_by: [TASK-017-13]
    external_prs: [216]
    acceptance:
      - "PR #216 is merged and its merge commit is an ancestor of the selected updated dev baseline."
      - "The pre-merge evidence handoff is accepted and its exact revisions are available."
      - "The follow-on branch and PR are separate from formal/soak-casper-consensus."
      - "Record the actual merge SHA, node revision, harness revision, image digest, configuration, and protocol version."

  - id: TASK-018-2
    title: "Adapt harness and profile interfaces after the merge"
    claim_index: docs/claims/casper-soak-harness.md
    status: pending
    claimed_by: null
    assigned_to: ["@jeffrey-l-turner", "@jltatbeach"]
    assigned_on: 2026-09-22
    assignment_note: "The maintainer confirmed on 2026-09-22 that either handle may own any EPIC-018 task. The implementer claims the task with claimed_by when work starts."
    blocked_by: [TASK-018-1]
    decisions: [D-11]
    acceptance:
      - "Identify changed node interfaces consumed by the harness without claiming node correctness."
      - "Adapt profiles, collectors, and fixtures to the merged APIs, wire fields, and metrics."
      - "Rerun affected harness models and negative controls with current artifact digests."
      - "Do not copy pre-merge discharge statuses onto changed harness or profile artifacts."
      - "Report ratification mismatches as product findings. Do not silently relax profile expectations."

  - id: TASK-018-3
    title: "Reverify consensus-observation profiles against merged interfaces"
    claims: [CLAIM-CASPER-SOAK-002, CLAIM-CASPER-SOAK-003, CLAIM-CASPER-SOAK-004, CLAIM-CASPER-SOAK-006]
    status: pending
    claimed_by: null
    assigned_to: ["@jeffrey-l-turner", "@jltatbeach"]
    assigned_on: 2026-09-22
    assignment_note: "The maintainer confirmed on 2026-09-22 that either handle may own any EPIC-018 task. The implementer claims the task with claimed_by when work starts."
    blocked_by: [TASK-018-2]
    decisions: [D-02, D-03, D-04, D-05, D-06, D-07, D-09]
    acceptance:
      - "Rerun TASK-017-5, TASK-017-6, TASK-017-7, and TASK-017-9 profile models and executable fixtures."
      - "Demonstrate correct fault acknowledgments, observation correlation, and mismatch classification through supported merged interfaces."
      - "Missing interfaces block the affected scenario. Runtime fixes and node proofs belong to separate work."

  - id: TASK-018-4
    title: "Reverify accounting, identity, and Phlo observation profiles"
    claims: [CLAIM-CASPER-SOAK-005, CLAIM-CASPER-SOAK-007, CLAIM-CASPER-SOAK-008]
    status: pending
    claimed_by: null
    assigned_to: ["@jeffrey-l-turner", "@jltatbeach"]
    assigned_on: 2026-09-22
    assignment_note: "The maintainer confirmed on 2026-09-22 that either handle may own any EPIC-018 task. The implementer claims the task with claimed_by when work starts."
    blocked_by: [TASK-018-2]
    decisions: [D-01, D-08, D-10, D-12]
    acceptance:
      - "Adapt TASK-017-8, TASK-017-10, and TASK-017-11 generators and collectors to the merged test interfaces."
      - "Rerun profile controls for identity correlation, missing measurements, expected values, and verdict classification."
      - "Preserve both Phlo fields and separate authority labels in profile inputs and reports."
      - "Node conservation, carrier equivalence, and protocol proofs remain outside scope."

  - id: TASK-018-5
    title: "Run the post-merge comparative soak campaign"
    claims: [CLAIM-CASPER-SOAK-001, CLAIM-CASPER-SOAK-004]
    status: pending
    claimed_by: null
    assigned_to: ["@jeffrey-l-turner", "@jltatbeach"]
    assigned_on: 2026-09-22
    assignment_note: "The maintainer confirmed on 2026-09-22 that either handle may own any EPIC-018 task. The implementer claims the task with claimed_by when work starts."
    blocked_by: [TASK-018-3, TASK-018-4]
    decisions: [D-06, D-07, D-11]
    acceptance:
      - "The maintainer approves the post-merge matrix, resource budget, durations, and repetitions before dispatch."
      - "Run the merged baseline and isolated deferred-policy experiments with new run IDs and artifact digests."
      - "Compare against pre-merge evidence only when workload, environment, and metric definitions are compatible."
      - "Report recovery, retry expiry, duplicates, custody, finalization, work bounds, disk, and memory outcomes."
      - "Rerun the scan benchmark and concurrency gate. Preserve failures, timeouts, and incomplete outcomes."

  - id: TASK-018-6
    title: "Discharge post-merge claims and submit the formal harness PR evidence"
    claim_index: docs/claims/casper-soak-harness.md
    status: pending
    claimed_by: null
    assigned_to: ["@jeffrey-l-turner", "@jltatbeach"]
    assigned_on: 2026-09-22
    assignment_note: "The maintainer confirmed on 2026-09-22 that either handle may own any EPIC-018 task. The implementer claims the task with claimed_by when work starts."
    blocked_by: [TASK-018-5]
    decisions: [D-11]
    acceptance:
      - "Strict CbC discharge covers only the changed harness and profile artifacts and their required fixture bindings."
      - "The report separates harness model results, executable fixture results, and product observations. It makes no node-proof claim."
      - "No required pending or refuted claim is hidden by epic completion."
      - "The follow-on PR links the meeting, pre-merge handoff, actual #216 merge revision, and new evidence."
      - "Deferred-policy activation requires a separate team decision even when experimental evidence passes."

  - id: TASK-018-7
    title: "Run and record the 60-hour stability soak after the EPIC-017 changes are in master"
    claims: [CLAIM-CASPER-SOAK-001]
    status: blocked
    claimed_by: null
    assigned_to: ["@jeffrey-l-turner", "@jltatbeach"]
    assigned_on: 2026-09-28
    created_at: 2026-09-28
    origin: "The maintainer decided on 2026-09-28 that the approved 60-hour phase is a post-merge obligation. It is not a closure requirement for TASK-017-13 or for PR #436."
    blocked_by: [TASK-017-12, TASK-017-13]
    external_gate: "The merge commit of the EPIC-017 changes is an ancestor of origin/master."
    resource_approval: "The approval of 2026-09-22 applies: one 64 GB stability runner for each architecture, with a maximum lifetime of 64 hours. Both full 24-hour baselines must pass first. More repetitions are not approved."
    workflow_stage: campaign-stability-60h
    github_record: 473
    github_record_url: https://github.com/F1R3FLY-io/f1r3node-rust/issues/473
    github_record_label: post-merge-obligation
    github_record_obligation: O1
    github_record_note: "The maintainer requested issue 473 on 2026-09-28. It is the single record of all post-merge obligations of the stack, and this task is obligation O1. The automatic update is planned for the branch ci/soak-obligation-gate. The maintainer created that branch on 2026-09-28 from fix/soak-finalization-attribution, below feature/randomized-exercise-soak in the stack."
    stage_decision: "The maintainer confirmed the stage campaign-stability-60h on 2026-09-28."
    acceptance:
      - "The soaked revision is on master and contains the merge commit of the EPIC-017 changes."
      - "Each architecture completes the full 216,000-second workload window. A shortened window does not satisfy this task."
      - "The record names the run IDs, the revision, the seeds, the image digests, the terminal verdict, and the evidence digests."
      - "A failure, a timeout, or an infrastructure stop stays in the record. A later passing run does not remove it."
      - "The obligation record closes only on passing evidence for the two architectures."
---
```

**Start condition:** PR #216 must merge before this follow-on branch starts. TASK-018-1 enforces the external gate explicitly.

**Current state:** Blocked. No follow-on branch, PR, model run, or soak run has been created.

---

### EPIC-013: Release Process and Deployment Trains

```yaml
---
epic_id: EPIC-013
title: "Release Process and Deployment Trains"
status: in_progress
priority: p1
user_story: null
blocked_by: []
created_at: 2026-08-19
claimed_by: claude-session-838e6241
claimed_at: 2026-08-19T00:00:00Z
tasks:
  - id: TASK-013-1
    title: "Specify and ratify the release process (docs/release-process.md)"
    status: complete
    completed_at: 2026-08-19T00:00:00Z
    branch: feature/release-process-implementation
    notes:
      - "All 13 Section 19 items ratified 2026-08-19. Two amendments: the regression verdict is advisory with maintainer review plus an OCI Notifications alert, and one infra-failure restart is permitted when 60h coverage is preserved."
      - "Includes the soak terminology rename (60h stability soak, dev integration soak, Shard soak-in), the Section 17.1 trigger and duration table, and glossary entries."
  - id: TASK-013-2
    title: "Phase 1: evidence-only release automation"
    status: complete
    completed_at: 2026-08-19T00:00:00Z
    branch: feature/release-process-implementation
    notes:
      - "release-evidence.yml generates exact-run evidence; release.yml and soak-in.yml are held-state stubs; release-evidence.sh plus unit tests and the test-release-workflows.sh contract guard are in place."
  - id: TASK-013-3
    title: "Phase 2: canary publication (canary-publish.yml)"
    status: in_progress
    branch: feature/release-phase2-canary-publication
    blocked_by: []
    notes:
      - "Implemented as canary-publish.yml: workflow_run on CI completion publishes the immutable canary tag, prerelease, and Docker Hub images by digest from the run's own artifacts; ineligible runs skip cleanly. Evidence upgrades to publication_mode: canary via release-evidence.sh record-images. OCIR canary deferred to Phase 3 (registry location is secret material). Remains in_progress until the first live master run proves it."
    acceptance:
      - "canary-publish.yml publishes immutable canary tags, prereleases, and images from tested artifacts on release-eligible master runs"
  - id: TASK-013-4
    title: "Phase 3: artifact-based validation (candidate digest modes)"
    status: in_progress
    branch: feature/release-phase3-candidate-digest-validation
    blocked_by: [TASK-013-5]
    notes:
      - "2026-08-22: stacked on Phase 4 (feature/release-phase4-promotion-controller). Registry decision ratified by the maintainer: OCIR is canonical for candidate gates, Docker Hub stays as the dual-published public mirror, no sync service."
      - "canary-publish.yml pushes the same index to OCIR; evidence records images.ocir_index_digest, which the validator requires to equal the Docker Hub index digest. The OCIR repository path never enters evidence."
      - "oci-validation.yml gains candidate_tag (exact-candidate mode); reusable-oci-validation.yml pulls each architecture image from OCIR by digest instead of building. merge-recovery-soak.yml gains candidate_tag, pulls the amd64 image by digest, and carries the tag through an in-window restart."
      - "Both gate workflows publish Section 8.1 documents with release-gate-evidence.sh from a publish_candidate_evidence job under release-credentials, plus the release-candidate marker that resumes release.yml. test-release-gate-evidence.sh proves the writer against release-gates.sh."
      - "promote-release.sh and release.yml copy the stable tag and latest into OCIR as well as Docker Hub."
      - "The regress alert reuses the soak's existing ONS verdict email; promotion holds until maintainer-review.json is uploaded."
      - "2026-08-22 multi-agent review of PR #325: gate documents carry candidate_evidence_sha256 and are written from the evidence file the run kept as a same-run artifact, never a re-downloaded release asset; release-gates.sh requires that digest to equal the evidence under evaluation. A candidate soak restart must name restart_of_run_id and match the original run's soak-window artifact (candidate, attempt 0, weekend, end epoch); coverage_preserved is true only for a verified restart."
    acceptance:
      - "merge-recovery-soak.yml and oci-validation.yml consume the candidate image digest without rebuilding"
      - "The canary publisher publishes the same index digest to OCIR and Docker Hub, and evidence records the OCIR digest"
      - "One exact-candidate OCI validation run and one candidate weekend soak publish Section 8.1 documents that release-gates.sh evaluates as pass"
      - "Optional hardening: a read-only OCIR pull token replaces OCIR_AUTH_TOKEN in the soak and OCI validation pull steps (the OCIR_* secrets are repository-scoped and already readable there, verified 2026-08-22)"
  - id: TASK-013-5
    title: "Phase 4: stable promotion controller"
    status: in_progress
    branch: feature/release-phase4-promotion-controller
    blocked_by: [TASK-013-3]
    notes:
      - "2026-08-22: release-gates.sh evaluates the eight Section 8 gates from JSON documents only (pass, hold, or fail; exit 0, 10, or 20) and promote-release.sh plans Section 11 steps 4 to 14 from observed stable state, verifies binaries, emits stable-release-evidence.json, and bumps the next version. release.yml replaces the held stub: a read-only gates job, then a promote job under release-credentials that copies the image by digest with imagetools create, creates the verified stable tag and release, moves latest, and opens the next-version pull request. test-release-gates.sh, test-promote-release.sh, and the updated test-release-workflows.sh contract guard run in CI."
      - "Section 8.1 defines the gate-evidence contract that Phase 3 must publish as candidate prerelease assets. Until Phase 3 lands, the OCI, soak, and verdict gates hold, so no candidate is promotable end to end."
      - "The regress-verdict OCI Notifications alert belongs to the soak workflow (Phase 3); the controller holds on regress until maintainer-review.json accepts it."
      - "2026-08-22 multi-agent review of PR #323: fail-closed gate evaluation (a malformed document fails its gate, the report always holds all eight gates), API-verified run identity for the OCI and soak documents, API-verified reviewer permission for maintainer-review.json, resume verifies existing release assets and refuses when a newer stable exists, latest is verified after the move, the next-version step is idempotent and keeps the token out of the remote URL, and the workflow_run resume is bound to a default-branch dispatch of a gate workflow whose marker names the evidence source. Concurrency was already serialized by the concurrency group; documented in Section 11.1."
    acceptance:
      - "release.yml performs exact-candidate promotion via release-gates.sh and promote-release.sh"
      - "A regress verdict publishes an OCI Notifications alert to the soak-report list and holds promotion for documented maintainer review"
      - "The release-credentials environment exists with DOCKERHUB_USERNAME and DOCKERHUB_TOKEN and required reviewers"
      - "One live promotion of a Phase 3 candidate publishes a stable tag, release, and image whose digest equals the candidate index"
  - id: TASK-013-6
    title: "Phase 5: Deployment Trains"
    status: in_progress
    branch: feature/release-phase5-deployment-trains
    blocked_by: [TASK-013-4]
    notes:
      - "2026-08-22: release-train.sh validates schema 1 and 2 manifests, the member chain (bottom member targets integration_branch, each later member targets the preceding member), head ancestry from compare documents, merged-member merge-commit topology, the source version and publishing-only reservation, and picks or dispatches the ci.yml run for head_sha. deployment-train.yml runs Section 13.2 steps 1 to 9 from the default branch with actions:write only for the ci.yml dispatch and uploads the train-record artifact. test-release-train.sh covers 18 rejections; test-release-workflows.sh guards the workflow; ci.yml validates every manifest under .github/deployment-trains/."
      - ".github/deployment-trains/key-contention.yml is the non-publishing rehearsal for #299 -> #312 -> #319 -> #311 at the heads observed 2026-08-22. The live stack currently fails the ancestry check: #311's head 6c70d818a predates #319's last two commits (325bbb28b, 80f0a3b6a). The rehearsal will reject until #311 is re-based on #319 and the manifest head_sha is updated."
      - "Remaining for this task: train canary creation through canary-publish.yml (Section 4.2 tag format, train_id in evidence), train_gates evaluation in release-gates.sh from manifest required_gates, recorded-member-head reachability in promote-release.sh, and the Section 13.3 re-validation trigger after a member merge."
      - "2026-08-22: release-process.md Section 13.1.1 adds schema_version 2 stack manifests. A stack train binds the top-of-stack head as its one candidate; members merge bottom-up with true merge commits. Setup steps 4 to 6 (member chain, head ancestry, merged-member topology) and the validator depend on no earlier phase; canary, digest-bound gates, and promotion depend on Phases 2 to 4."
      - "2026-08-22 multi-agent review of PR #321: Section 13.2 is one numbered sequence; the member base rule points to the preceding member; merge method is re-validated after each member merge and every recorded member head is verified at promotion, which closes the force-push window; the CI filter text matches ci.yml."
      - "blocked_by encodes the stack merge order PR #322 -> #323 (Phase 4) -> #325 (Phase 3) -> #321 (Phase 5), which is also the code-dependency order: Phase 3 implements the Section 8.1 contract Phase 4 defines. Phase 4 becomes operational end to end only after Phase 3 publishes gate documents; that is a runtime dependency, not a merge dependency."
      - "ci.yml runs pull-request CI for bases dev, master, staging, trying, and feature/**. A member whose base is outside that set (fix/**, formal/**) gets no pull-request run; train setup requires one successful ci.yml run for head_sha from any event and dispatches one when none exists (Section 13.2 step 9)."
      - "Rehearsal candidate for the stack-train step: the key-contention stack PR #299 -> #312 -> #319 -> #311 (EPIC-016), non-publishing, no version reservation."
      - "2026-08-23 multi-agent review of PR #328: plan-ci also requires head_repository.full_name to match (the run list reports the base repository for every run); a merged member proves merge-commit reachability from integration_branch through a reach-<N>.json compare document; a member whose predecessor has merged may target integration_branch, because GitHub retargets the pull request when the merged branch is deleted; setup fails fast on a member head outside this repository. 23 rejection cases plus merged and retargeted acceptance paths."
    acceptance:
      - "deployment-train.yml validates manifests under .github/deployment-trains/ and starts trains"
      - "The validator accepts schema_version 1 and 2, and rejects a stack member with a foreign base, broken head ancestry, or a squash or rebase merge"
      - "Setup dispatches ci.yml on head_sha when no successful run exists for that SHA and records the run identifier as CI evidence"
      - "One non-publishing single-train rehearsal completes"
      - "One non-publishing stack-train rehearsal completes on a stacked pull-request set"
      - "The cost-accounting train (PR #216) publishes first"
  - id: TASK-013-7
    title: "Phase 6: Shard soak-in scheduling"
    status: in_progress
    blocked_by: [EPIC-014]
    notes:
      - "The release trigger is implemented: soak-in.yml fires on stable release publication (prereleases gate out) while enrollment stays held until the EPIC-014 test net exists."
    acceptance:
      - "soak-in.yml gains a release trigger: one enrollment per stable release tag"
      - "The deferred parameters (soak-in period length, Anchor criteria, test net composition) are set and ratified"
---
```

**Context:** `docs/release-process.md` is the ratified specification. This PR (feature/release-process-implementation, PR #279) delivers TASK-013-1 and TASK-013-2; later phases follow the Section 18 migration plan on follow-on branches.

---

### EPIC-014: Test Net (Continuously Running Shards)

```yaml
---
epic_id: EPIC-014
title: "Test Net (Continuously Running Shards)"
status: pending
priority: p2
user_story: null
blocked_by: [EPIC-013]
created_at: 2026-08-19
tasks:
  - id: TASK-014-1
    title: "Design the test net (topology, lifecycle, upgrade path)"
    status: pending
  - id: TASK-014-2
    title: "Stand up the test net on OCI from existing fleet tooling"
    status: pending
  - id: TASK-014-3
    title: "Wire Shard soak-in enrollment and Anchor promotion into the test net"
    status: pending
  - id: TASK-014-4
    title: "Open selected test net shards to partners and customers"
    status: pending
---
```

**Context:** Unlike the soaks, which create a fresh shard per iteration, this epic delivers the test net: shards that run continuously. **Mechanism sketch (brief by intent — a follow-on branch/PR carries the design):** long-lived OCI instances run stable releases as test net members, reusing the existing fleet tooling (runner launch, monitoring, ONS alerts, soak dashboard) as the foundation. Each weekly stable release enrolls new nodes through the Shard soak-in (`docs/release-process.md` Section 12); nodes that complete the soak period gain the Anchor role. The test net is primarily internal release-validation infrastructure and also serves select partners and customers. This epic delivers the test net that release-process Phase 6 requires; the deferred Shard soak-in parameters are set here.

---

### EPIC-012: Open-Issue Remediation PR Queue

```yaml
---
epic_id: EPIC-012
title: "Open-Issue Remediation PR Queue"
status: pending
priority: p0
user_story: null
blocked_by: []
created_at: 2026-08-12
claimed_by: null
claimed_at: null
execution_contract:
  base_branch: dev
  task_unit: "one task = one branch = one pull request"
  merge_policy: "Start each unblocked branch from current origin/dev; after a dependency merges, start or rebase its dependent branch onto the updated origin/dev. Do not silently stack unrelated tasks."
  issue_policy: "Put Refs #N in every PR body. Because these PRs target dev rather than the default branch, close an issue only after the fix is promoted to master and its acceptance evidence is confirmed."
  completion_policy: "A task reaches review only when its acceptance checks and focused regression tests pass; it reaches complete only when the PR is merged."
tasks:
  - id: TASK-012-1
    title: "Contain AI system-contract failures without crashing the node"
    status: pending
    issues: [11]
    base_branch: dev
    branch: fix/ai-contract-failure-isolation
    proposed_pr_title: "fix(rholang): isolate AI system-contract failures"
    claimed_by: null
    blocked_by: []
    acceptance:
      - "HTTP 5xx, timeout, and malformed responses from every external AI system contract become typed deploy failures rather than process panics"
      - "The node remains responsive and can process a subsequent valid deploy"
      - "Focused tests cover the failure path and successful recovery"

  - id: TASK-012-2
    title: "Align node readiness with Casper readiness"
    status: pending
    issues: [12]
    base_branch: dev
    branch: fix/casper-aware-readiness
    proposed_pr_title: "fix(node): report ready only after Casper initialization"
    claimed_by: null
    blocked_by: []
    acceptance:
      - "The wait/readiness surface remains false until Casper can serve exploratory deploys"
      - "Observer startup cannot report ready during the Casper-unavailable window"
      - "Regression coverage reproduces the documented observer --wait sequence"

  - id: TASK-012-3
    title: "Reject stale-chain peers after a network-ID relaunch"
    status: pending
    issues: [13]
    base_branch: dev
    branch: fix/genesis-bound-peer-handshake
    proposed_pr_title: "fix(network): bind peer sessions to genesis identity"
    claimed_by: null
    blocked_by: []
    acceptance:
      - "The peer handshake carries an immutable genesis/chain identity in addition to networkId"
      - "Peers from an old chain with the same networkId are rejected before dependency fetching or block processing"
      - "Compatibility and mixed-version rollout behavior are documented and tested"

  - id: TASK-012-4
    title: "Publish deb and rpm node packages from CI"
    status: pending
    issues: [14]
    base_branch: dev
    branch: feat/linux-package-artifacts
    proposed_pr_title: "feat(ci): publish deb and rpm node packages"
    claimed_by: null
    blocked_by: []
    acceptance:
      - "Release CI builds amd64 and arm64 deb/rpm artifacts with the node binary, service definition, and default configuration"
      - "Package installation, version reporting, and clean removal are smoke-tested"
      - "Artifact naming and supported-platform documentation are updated"

  - id: TASK-012-5
    title: "Replace HOCON with a maintained configuration format"
    status: pending
    issues: [15]
    base_branch: dev
    branch: feat/config-format-migration
    proposed_pr_title: "feat(config): migrate node configuration away from HOCON"
    claimed_by: null
    blocked_by: []
    acceptance:
      - "A maintained parser and canonical configuration format replace the unmaintained HOCON dependency"
      - "Existing operator configuration has a documented compatibility or conversion path"
      - "Configuration precedence, defaults, malformed-input behavior, and representative production configs are tested"

  - id: TASK-012-6
    title: "Extend DeployData parameters to all supported Rholang values"
    status: pending
    issues: [17]
    base_branch: dev
    branch: feat/deploy-parameter-types
    proposed_pr_title: "feat(api): extend DeployData parameter value types"
    claimed_by: null
    blocked_by: []
    acceptance:
      - "DeployData parameters support tuple, list, set, map, Nil, and URI values without lossy conversion"
      - "Protobuf/JSON compatibility and deterministic normalization are covered"
      - "Existing parameter clients remain compatible or receive a documented migration path"

  - id: TASK-012-7
    title: "Expose DeployData parameters through the observer Web API"
    status: pending
    issues: [16]
    base_branch: dev
    branch: feat/web-api-deploy-parameters
    proposed_pr_title: "feat(web-api): expose DeployData parameters"
    claimed_by: null
    blocked_by: [TASK-012-6]
    acceptance:
      - "Observer HTTP responses expose the complete parameter representation introduced by TASK-012-6"
      - "OpenAPI documentation and serialization tests cover every supported parameter type"
      - "Responses for deploys without parameters remain backward compatible"

  - id: TASK-012-8
    title: "Exclude persistently nonparticipating validators from finality weight safely"
    status: pending
    issues: [18]
    base_branch: dev
    branch: fix/participation-based-finality-committee
    proposed_pr_title: "fix(consensus): derive finality committee from finalized participation"
    claimed_by: null
    blocked_by: []
    acceptance:
      - "A validator that bonds and activates but never produces cannot stall finalization indefinitely"
      - "Participation is derived from finalized evidence; young-DAG and minority-finalization safety are preserved"
      - "Tests cover never-started, temporarily offline, resumed, and newly activated validators"

  - id: TASK-012-9
    title: "Synchronize canonical Rholang resources after the parser upgrade"
    status: pending
    issues: [19]
    base_branch: dev
    branch: chore/sync-rholang-resources
    proposed_pr_title: "chore(rholang): sync canonical resource contracts"
    claimed_by: null
    blocked_by: []
    acceptance:
      - "The listed system and test .rho files are reconciled with their canonical implementations"
      - "Multi-binding and @=* pattern syntax are covered by parser regressions"
      - "The full affected contract test suites pass and obsolete parser workarounds are removed"

  - id: TASK-012-10
    title: "Cache fault tolerance for finalized-block API queries"
    status: pending
    issues: [22]
    base_branch: dev
    branch: perf/cache-finalized-fault-tolerance
    proposed_pr_title: "perf(block-api): cache finalized-block fault tolerance"
    claimed_by: null
    blocked_by: []
    acceptance:
      - "Finalized block metadata stores the computed fault-tolerance value at finalization time"
      - "Block API queries use the cache only for finalized blocks and retain live computation for non-finalized blocks"
      - "Correctness and deep-history performance regressions are tested"

  - id: TASK-012-11
    title: "Remove the 129-term random-split overflow"
    status: pending
    issues: [34]
    base_branch: dev
    branch: fix/rholang-random-split-overflow
    proposed_pr_title: "fix(rholang): prevent random-split identifier overflow"
    claimed_by: null
    blocked_by: []
    acceptance:
      - "Par values at the 128/129/256 boundaries reduce without panic"
      - "The split identifier uses a range-compatible type or returns a structured error"
      - "Generated regression cases cover both CLI and interpreter paths"

  - id: TASK-012-12
    title: "Keep LFS-synced observers inside the imported DAG horizon"
    status: pending
    issues: [37]
    base_branch: dev
    branch: fix/lfs-observer-parent-horizon
    proposed_pr_title: "fix(observer): bound post-LFS parent validation to imported history"
    claimed_by: null
    blocked_by: []
    acceptance:
      - "Normal block validation after LFS sync never requires DAG hashes or roots below the imported horizon"
      - "A fresh observer catches up beyond approved-state height on a long-running shard"
      - "The regression is distinct from reporter replay coverage in PR #210"

  - id: TASK-012-13
    title: "Install RhoSpecContract as a genesis resource"
    status: pending
    issues: [41]
    base_branch: dev
    branch: feat/genesis-rhospec-contract
    proposed_pr_title: "feat(genesis): install the RhoSpec contract"
    claimed_by: null
    blocked_by: []
    acceptance:
      - "RhoSpecContract is available from ordinary nodes without test-resource paths"
      - "Genesis registration is deterministic and its URI is documented"
      - "Existing RhoSpec tests consume the genesis-installed contract"

  - id: TASK-012-14
    title: "Resolve the PR #488 deferred review checklist"
    status: pending
    issues: [44]
    base_branch: dev
    branch: refactor/rejected-deploy-storage-followups
    proposed_pr_title: "refactor(casper): resolve rejected-deploy review follow-ups"
    claimed_by: null
    blocked_by: []
    acceptance:
      - "Every still-applicable item in issue #44 is implemented or dispositioned explicitly in the PR body"
      - "Duplicated deploy/rejected-deploy storage logic is consolidated without weakening encapsulation"
      - "Recovery-cycle, persistence, and storage regressions pass"

  - id: TASK-012-15
    title: "Resolve the PR #491 mergeable-channel review checklist"
    status: pending
    issues: [46]
    base_branch: dev
    branch: refactor/mergeable-channel-followups
    proposed_pr_title: "refactor(rspace): resolve mergeable-channel review follow-ups"
    claimed_by: null
    blocked_by: []
    acceptance:
      - "Every still-applicable item in issue #46 is implemented or dispositioned explicitly in the PR body"
      - "Tag identity derivation has one source of truth and hot-path avoidable allocations are removed"
      - "Mergeable-channel semantic and performance regressions pass"

  - id: TASK-012-16
    title: "Widen token vault arithmetic to BigInt"
    status: pending
    issues: [49]
    base_branch: dev
    branch: feat/bigint-token-vaults
    proposed_pr_title: "feat(rholang): support BigInt token vault balances"
    claimed_by: null
    blocked_by: [TASK-012-9]
    acceptance:
      - "NonNegativeNumber and MakeMint support balances and transfers above 2^63-1"
      - "New registry URIs provide an explicit protocol migration boundary"
      - "10^30 round-trip, negative-value rejection, bridge compatibility, and genesis determinism are tested"

  - id: TASK-012-17
    title: "Characterize and improve intra-deploy Par execution scaling"
    status: pending
    issues: [50]
    base_branch: dev
    branch: perf/rholang-par-scaling
    proposed_pr_title: "perf(rholang): address intra-deploy Par scaling"
    claimed_by: null
    blocked_by: []
    acceptance:
      - "A reproducible benchmark separates reducer serialization, RSpace contention, replay overhead, and scheduler limits"
      - "The PR either implements a measurable bounded speedup or documents and enforces the intended sequential contract"
      - "Play/replay determinism and cost accounting remain unchanged"

  - id: TASK-012-18
    title: "Accept and print unsuffixed floating-point literals consistently"
    status: pending
    issues: [75]
    base_branch: dev
    branch: fix/rholang-float-literals
    proposed_pr_title: "fix(rholang): normalize floating-point literal syntax"
    claimed_by: null
    blocked_by: []
    acceptance:
      - "Unsuffixed and f64-suffixed literals parse to the same f64 value"
      - "Printing has one canonical round-trippable representation"
      - "Parser, normalizer, protobuf, and CLI round-trip boundaries are tested"

  - id: TASK-012-19
    title: "Instrument finality-frontier and long-running merge cost"
    status: pending
    issues: [45, 105]
    base_branch: dev
    branch: perf/instrument-finality-frontier
    proposed_pr_title: "perf(consensus): instrument frontier and merge-cost growth"
    claimed_by: null
    blocked_by: []
    acceptance:
      - "Metrics separate finalizer/oracle time, merge-scope construction, fallback rate, frontier width, and per-block storage cost"
      - "A bounded automated scenario reproduces the growth signature from both issues"
      - "The evidence identifies which subsequent fix owns each bottleneck rather than inferring causality from wall-clock latency"

  - id: TASK-012-20
    title: "Bound finality work as the unfinalized frontier grows"
    status: pending
    issues: [105]
    base_branch: dev
    branch: fix/bounded-finality-frontier
    proposed_pr_title: "fix(consensus): bound finality work over the DAG frontier"
    claimed_by: null
    blocked_by: [TASK-012-19]
    acceptance:
      - "Empty-block finalization latency no longer grows monotonically with frontier width"
      - "Any incremental cache or bound is invalidated deterministically on DAG/finality changes"
      - "Long-running empty-block and adversarial wide-frontier regressions pass without weakening finality safety"

  - id: TASK-012-21
    title: "Prevent long-running multi-parent merge-scope degradation"
    status: pending
    issues: [45]
    base_branch: dev
    branch: fix/bounded-merge-scope-growth
    proposed_pr_title: "fix(casper): bound long-running multi-parent merge scope"
    claimed_by: null
    blocked_by: [TASK-012-19, TASK-012-20]
    acceptance:
      - "Multi-parent merge remains the normal path after thousands of heartbeat blocks"
      - "merge_scope_too_large fallback does not grow to dominate steady-state operation"
      - "A multi-hour-equivalent accelerated regression preserves state and finalization progress"

  - id: TASK-012-22
    title: "Profile the exhaustive TLA+ configurations without guessed caps"
    status: pending
    issues: [206]
    base_branch: dev
    branch: ci/profile-exhaustive-tla
    proposed_pr_title: "ci(formal): profile exhaustive TLA configurations"
    claimed_by: null
    blocked_by: [EPIC-011]
    acceptance:
      - "All three exhaustive configurations run uncapped on a sufficiently large runner"
      - "Wall-clock, peak memory, state count, and completion/violation outcomes are recorded per configuration"
      - "The PR contains no blind timeout increase and proposes measured caps only for configurations that complete"

  - id: TASK-012-23
    title: "Schedule an expected-green exhaustive formal-verification tier"
    status: pending
    issues: [206]
    base_branch: dev
    branch: ci/schedule-exhaustive-tla
    proposed_pr_title: "ci(formal): schedule measured exhaustive TLA coverage"
    claimed_by: null
    blocked_by: [TASK-012-22]
    acceptance:
      - "Completable exhaustive configurations run on a nightly or weekly schedule with evidence-derived caps"
      - "MC_EquivocationDetector receives an explicit retire/manual-reference/alternative-engine disposition"
      - "Timeout, violation, infrastructure failure, and success remain distinct outcomes"

  - id: TASK-012-24
    title: "Measure cgroup-accounted memory during high-cap load"
    status: pending
    issues: [244]
    base_branch: dev
    branch: perf/cgroup-memory-observability
    proposed_pr_title: "perf(soak): report cgroup-accounted node memory"
    claimed_by: null
    blocked_by: []
    acceptance:
      - "Load reports capture cgroup memory.current/usageBytes alongside workingSetBytes and container restart reason"
      - "Fast LMDB mmap or allocator spikes are visible before an OOM kill"
      - "A cap 60-75 sweep records the actual memory ceiling and distinguishes budget pressure from a leak"

  - id: TASK-012-25
    title: "Remove the post-jemalloc high-load OOM ceiling"
    status: pending
    issues: [244]
    base_branch: dev
    branch: fix/jemalloc-high-load-memory
    proposed_pr_title: "fix(node): bound allocator memory under concurrent load"
    claimed_by: null
    blocked_by: [TASK-012-24]
    acceptance:
      - "The fix follows measured evidence: tune jemalloc, reduce allocation-heavy hot paths, or change the supported resource envelope explicitly"
      - "The affected profile completes the agreed cap sweep without OOM restart"
      - "The #146 throughput/lock-contention improvement is retained"

  - id: TASK-012-26
    title: "Deduplicate protocol and runtime constants"
    status: pending
    issues: [245]
    base_branch: dev
    branch: refactor/shared-node-constants
    proposed_pr_title: "refactor(node): consolidate duplicated protocol constants"
    claimed_by: null
    blocked_by: []
    acceptance:
      - "Shared LFS backoff, retry, API threshold, keepalive, cache, and page-size values have one named source or an explicit documented distinction"
      - "Requester/responder agreement values cannot drift across sibling modules"
      - "Behavior remains unchanged and focused tests assert the shared values"

  - id: TASK-012-27
    title: "Move operator-tunable constants into configuration"
    status: pending
    issues: [245]
    base_branch: dev
    branch: feat/configurable-runtime-limits
    proposed_pr_title: "feat(config): expose runtime cache and retry limits"
    claimed_by: null
    blocked_by: [TASK-012-26, TASK-012-5]
    acceptance:
      - "Runtime-manager caches, block-retriever timings/caps, startup deadlines, and network timeouts are configurable with safe defaults"
      - "Configuration names, units, bounds, and disabled-value semantics are documented"
      - "Default behavior matches the pre-change constants and invalid values fail closed"

  - id: TASK-012-28
    title: "Finish safe legacy-name migration with compatibility fallbacks"
    status: pending
    issues: [246]
    base_branch: dev
    branch: chore/f1r3fly-name-cleanup
    proposed_pr_title: "chore(node): finish the F1R3FLY naming migration"
    claimed_by: null
    blocked_by: []
    acceptance:
      - "Key/config/data paths, binary naming, metrics service tag, user-facing token text, tests, and scripts use F1R3FLY names"
      - "Existing rnode.conf, key, and data paths have a warning-backed compatibility fallback for one release"
      - "rho:rchain alias behavior is decided, implemented consistently, and documented"

  - id: TASK-012-29
    title: "Introduce a compatible successor to the rnode peer URI scheme"
    status: pending
    issues: [246]
    base_branch: dev
    branch: feat/f1r3fly-peer-uri-scheme
    proposed_pr_title: "feat(network): introduce the f1r3fly peer URI scheme"
    claimed_by: null
    blocked_by: [TASK-012-28]
    acceptance:
      - "Nodes parse both legacy rnode:// and new f1r3fly:// addresses during a documented transition window"
      - "Nodes emit the new scheme only when compatibility permits"
      - "Mixed-version discovery, bootstrap configuration, CLI parsing, and eventual legacy-removal criteria are tested and documented"
  - id: TASK-012-30
    title: "Bind ephemeral CI runners to the run that launched them"
    status: pending
    issues: []
    base_branch: dev
    branch: fix/ci-ephemeral-runner-run-binding
    proposed_pr_title: "fix(ci): bind ephemeral runners to their launching run"
    claimed_by: null
    blocked_by: []
    notes:
      - "2026-08-23: three Heavy Pipelines overlapped (#316 re-run, #328, #329). Launch Ephemeral Runners starts exactly two amd64 and two arm64 VMs per run, but GitHub assigns queued jobs to runners by label only, so the pool is shared. PR #328's amd64-docker job took PR #329's second amd64 VM (ci-eph-...-042856-6d1807) five minutes before #329's integration jobs queued; #329's amd64-subprocess then waited with no runner until the run was cancelled and re-run in full. Same symptom as the re-run-failed-jobs trap, different cause."
      - "The per-run label is the minimal fix: launch-runner.sh registers each VM with an extra label run-<GITHUB_RUN_ID>, and the ephemeral jobs add that label to runs-on. Idle VMs from another run then never match."
    acceptance:
      - "Each ephemeral runner registers with a label that names the run that launched it, and every ephemeral job in _integration-pipeline.yml requires that label"
      - "Two Heavy Pipelines started within one minute of each other both complete without a job waiting on a runner that another run consumed"
      - "An idle ephemeral VM that its run no longer needs still self-terminates on the idle timeout"
---
```

**Context:** An audit on 2026-08-12 compared all 43 open issues with all 28 open PRs. Nineteen issues had substantive open-PR coverage, including partial or stacked fixes. The 24 issues represented here had no open PR that addressed their remaining scope. Mere GitHub cross-references were not treated as coverage: PR #224 explicitly excludes the underlying #105 frontier behavior, PR #210 explicitly excludes #37's normal block-processing path, and #18's own analysis says its participation-based core fix remains outstanding.

**PR/branch workflow:**

1. Select the first unblocked `pending` task by priority and dependency order.
2. Fetch `origin/dev`, create exactly the task's proposed branch from that tip, and set `status: in_progress`, `claimed_by`, and `claimed_at`.
3. Keep the branch scoped to the listed issue(s). If investigation proves multiple independently reviewable fixes are required, add new TASK-012 entries before opening extra PRs rather than expanding the branch silently.
4. Open one PR to `dev` using `proposed_pr_title`; include `Refs #N`, acceptance evidence, and explicit non-goals.
5. Set the task to `review` and record `pr`, validation evidence, and any replacement dependency.
6. After merge, set `status: complete`. Start dependent branches from the new `origin/dev`, not from the merged feature branch.
7. When the fix reaches `master`, confirm production/default-branch acceptance evidence and close the referenced issue manually if GitHub did not.

**Merge waves:**

- **Wave A — independent and bounded:** TASK-012-1 through TASK-012-6, TASK-012-8 through TASK-012-19, TASK-012-24, TASK-012-26, and TASK-012-28 may proceed independently from fresh `origin/dev` branches.
- **Wave B — evidence or API dependent:** TASK-012-7, TASK-012-20, TASK-012-22, TASK-012-25, TASK-012-27, and TASK-012-29 start only after their declared blockers merge.
- **Wave C — cumulative behavior:** TASK-012-21 and TASK-012-23 start last in their respective chains and must validate the merged behavior, not a stacked approximation.

**Scope:**

- Included: branch/PR-sized implementation work for open issues lacking an addressing PR as of the audit.
- Excluded: the 19 issues already addressed by open PRs; issue closure before promotion to `master`; unrelated cleanup discovered while implementing a task.

---

### EPIC-003: Merge Critical PRs into f1r3node

```yaml
---
epic_id: EPIC-003
title: "Merge Critical PRs into f1r3node"
status: pending
priority: p0
user_story: US-002
blocked_by: []
created_at: 2026-04-09
claimed_by: null
claimed_at: null
external: true
external_repo: F1R3FLY-io/f1r3node
coordination_note: "This epic is executed by the agent in f1r3node. Track progress via /tmp/migrationPlan.md phase_1_critical_prs status."
tasks:
  - id: TASK-003-1
    title: "Verify new_parser branch status"
    status: pending
    acceptance:
      - "new_parser branch is merged into rust/dev OR confirmed as base for Reified RSpaces chain"
      - "rholang-rs#83 dependency is resolved"

  - id: TASK-003-2
    title: "Merge Reified RSpaces chain (#328-#338)"
    status: pending
    blocked_by: [TASK-003-1]
    acceptance:
      - "All 11 PRs (#328 through #338) merged sequentially into rust/dev"
      - "CI passes after each merge"

  - id: TASK-003-3
    title: "Merge Tier 2 PRs if ready"
    status: pending
    acceptance:
      - "#466 (Embers) reviewed — merged or deferred"
      - "#186 (eval cost) reviewed — merged or deferred"
      - "#281 (LMDB fixes) reviewed — merged or deferred"

  - id: TASK-003-4
    title: "Tag final f1r3node release"
    status: pending
    blocked_by: [TASK-003-2, TASK-003-3]
    acceptance:
      - "Tag rust-v0.4.12 (or appropriate version) created on f1r3node rust/dev"
      - "phase_1_critical_prs.status set to 'complete' in /tmp/migrationPlan.md"
      - "phase_1_critical_prs.final_tag populated"
---
```

**Context:** The Reified RSpaces chain (#328-#338) is a major architectural change that must land before code sync. This phase is owned by the agent working in the f1r3node repository. Completion is signaled via the shared migration plan file.

**Scope:**

- Included: Merging blocking and ready PRs into f1r3node rust/dev
- Excluded: Any work in f1r3node-rust (that starts in EPIC-004)

**Notes:**

- The 11-PR Reified RSpaces chain has a sequential dependency — each PR targets the previous one
- Chain base (#328) depends on `new_parser` branch which depends on `rholang-rs#83`
- Monitor `/tmp/migrationPlan.md` for `phase_1_critical_prs.status` to know when to start EPIC-004

---

### EPIC-004: Code Sync to f1r3node-rust

```yaml
---
epic_id: EPIC-004
title: "Code Sync to f1r3node-rust"
status: in_progress
priority: p0
user_story: US-002
blocked_by: [EPIC-003]
created_at: 2026-04-09
claimed_by: claude-session-epic004
claimed_at: 2026-04-17T19:19:55Z
source_branch: rust/staging
source_head: fb59611fbf2be202a6d6450850de1435c9dec7a4
tasks:
  - id: TASK-004-1
    title: "Sync Rust workspace crates from f1r3node rust/staging"
    status: review
    claimed_by: claude-session-epic004
    claimed_at: 2026-04-17T19:19:55Z
    completed_at: 2026-04-29T18:50:45Z
    notes:
      - "Initial sync: 11 crates + root workspace files from f1r3node rust/staging @ 6ee5c390 (2026-04-17)"
      - "Re-sync: refreshed to f1r3node rust/staging @ fb59611f (2026-04-29) — 39 upstream commits, 539 files modified, 1 deleted, 5 new"
      - "Re-sync preserves local heed 0.22 upgrade (315b23b, 111e318): rspace++/Cargo.toml, shared/Cargo.toml pinned to heed = \"0.22.1\"; lmdb_*.rs files unchanged from HEAD"
      - "Per-crate Cargo.lock files added to .gitignore (only workspace /Cargo.lock is authoritative)"
      - "cargo build --workspace passes (49s)"
      - "./scripts/run_rust_tests.sh passes: 68 test runs, 0 failed"
      - "Full sync reports: docs/work-logs/task-004-1-2026-04-17T19-19-55Z.md (initial), docs/work-logs/task-004-1-resync-fb59611f-2026-04-29T18-50-45Z.md (current)"
      - "Not committed yet; user to invoke /quick-commit after review"
    acceptance:
      - "All 11 workspace crates updated from f1r3node rust/staging HEAD (fb59611f)"
      - "Cargo.toml workspace dependencies match source"
      - "cargo build --workspace succeeds"
      - "./scripts/run_rust_tests.sh passes per-crate"

  - id: TASK-004-2
    title: "Port CI/CD workflows"
    status: pending
    blocked_by: [TASK-004-1]
    acceptance:
      - "build-test-and-deploy.yml ported (Docker build, multi-arch, artifact publishing)"
      - "release.yml ported (automated versioning, changelog, tagging)"
      - "cliff.toml ported (changelog generation)"
      - ".github/apt-dependencies.txt ported"
      - "Docker image name set to f1r3fly-rust in CI"

  - id: TASK-004-3
    title: "Port Docker configuration"
    status: pending
    blocked_by: [TASK-004-1]
    acceptance:
      - "node/Dockerfile updated with correct image labels"
      - "docker/standalone.yml, shard.yml, observer.yml, validator4.yml ported"
      - "docker/monitoring/ (Prometheus, Grafana) ported"
      - "docker/conf/ (node config templates) ported"
      - "docker/genesis/ (bonds, wallets) ported"
      - "docker/.env.example ported"
      - "All compose files reference f1r3fly-rust image name"

  - id: TASK-004-4
    title: "Port scripts and local dev configuration"
    status: pending
    blocked_by: [TASK-004-1]
    acceptance:
      - "scripts/version.sh ported"
      - "scripts/clean_rust_libraries.sh ported"
      - "scripts/delete_data.sh ported"
      - "scripts/run_rust_tests.sh ported"
      - "run-local/ configuration ported"

  - id: TASK-004-5
    title: "Set version and create initial tag"
    status: pending
    blocked_by: [TASK-004-1, TASK-004-2]
    acceptance:
      - "node/Cargo.toml version continues from f1r3node's last release"
      - "Tag v0.4.12 (or matching version) created on f1r3node-rust"
      - "phase_2_code_sync.status set to 'complete' in /tmp/migrationPlan.md"
      - "phase_2_code_sync.synced_from_commit populated"
---
```

**Context:** Brings f1r3node-rust to full parity with post-merge f1r3node rust/dev. This is the core migration step — after this, f1r3node-rust becomes the canonical source of truth.

**Scope:**

- Included: All Rust crates, CI/CD, Docker, scripts, local dev config, version tagging
- Excluded: Issue migration (EPIC-005), external repo updates (EPIC-006)

**Notes:**

- The code delta is ~4 releases (v0.4.9-v0.4.11) plus the critical PRs from EPIC-003
- Docker image renamed from `f1r3fly-rust-node` to `f1r3fly-rust`
- Version drops the `rust-` tag prefix (no longer needed in a Rust-only repo)
- Run tests per-crate to avoid LMDB lock contention (see commit f2b4b5f)

---

### EPIC-006: External Repo Updates

```yaml
---
epic_id: EPIC-006
title: "External Repo Updates"
status: pending
priority: p1
user_story: US-002
blocked_by: [EPIC-004]
created_at: 2026-04-09
claimed_by: null
claimed_at: null
tasks:
  - id: TASK-006-1
    title: "Update system-integration repo"
    status: pending
    acceptance:
      - "Docker image references updated from f1r3fly-rust-node to f1r3fly-rust"
      - "CI triggers updated to reference f1r3node-rust repo"
      - "Integration tests pass against new image"

  - id: TASK-006-2
    title: "Update pyf1r3fly repo"
    status: pending
    acceptance:
      - "Repo references in docs and CI updated"
      - "PR #4 cross-reference updated (references f1r3node #407)"

  - id: TASK-006-3
    title: "Verify rholang-rs compatibility"
    status: pending
    acceptance:
      - "rholang-rs git rev reference in Cargo.toml confirmed working"
      - "No changes needed (already independent)"
      - "phase_4_external.status set to 'complete' in /tmp/migrationPlan.md"
---
```

**Context:** Downstream consumers need to point at the new repo and Docker image name. system-integration and pyf1r3fly are the primary consumers. rholang-rs is already independent.

**Scope:**

- Included: system-integration, pyf1r3fly, rholang-rs verification
- Excluded: Any other F1R3FLY-io repos not listed

---

### EPIC-007: PR Cleanup & Redirect

```yaml
---
epic_id: EPIC-007
title: "PR Cleanup & Redirect"
status: pending
priority: p1
user_story: US-002
blocked_by: [EPIC-004]
created_at: 2026-04-09
claimed_by: null
claimed_at: null
tasks:
  - id: TASK-007-1
    title: "Redirect Tier 3 PRs to f1r3node-rust"
    status: pending
    acceptance:
      - "PRs #457, #426, #424, #407, #405 receive redirect comment"
      - "Comment includes rebase instructions for f1r3node-rust"
      - "PRs closed on f1r3node"

  - id: TASK-007-2
    title: "Close Tier 4 (Scala) PRs"
    status: pending
    acceptance:
      - "PRs #470, #314, #185 receive deprecation comment"
      - "PRs closed on f1r3node"
      - "phase_5_pr_cleanup.status set to 'complete' in /tmp/migrationPlan.md"
---
```

**Context:** All open PRs on f1r3node must be resolved. Tier 3 PRs (viable Rust work) get redirect instructions. Tier 4 PRs (Scala) are closed with deprecation notice.

**Scope:**

- Included: Commenting and closing PRs on f1r3node
- Excluded: Tier 1/2 PRs (handled in EPIC-003)

---

### EPIC-008: Deprecation & Archive

```yaml
---
epic_id: EPIC-008
title: "Deprecation & Archive"
status: pending
priority: p2
user_story: US-002
blocked_by: [EPIC-005, EPIC-006, EPIC-007]
created_at: 2026-04-09
claimed_by: null
claimed_at: null
tasks:
  - id: TASK-008-1
    title: "Update f1r3node README with deprecation notice"
    status: pending
    acceptance:
      - "README.md updated on rust/dev, main, and default branch"
      - "Notice points to F1R3FLY-io/f1r3node-rust"
      - "Last Rust release version documented"

  - id: TASK-008-2
    title: "Update GitHub repo metadata"
    status: pending
    acceptance:
      - "Repository description set to 'DEPRECATED - See F1R3FLY-io/f1r3node-rust'"

  - id: TASK-008-3
    title: "Disable CI and close remaining items"
    status: pending
    blocked_by: [TASK-008-1]
    acceptance:
      - "All GitHub Actions workflows disabled on f1r3node"
      - "Any remaining open issues closed with redirect comment"

  - id: TASK-008-4
    title: "Archive f1r3node repository"
    status: pending
    blocked_by: [TASK-008-1, TASK-008-2, TASK-008-3]
    acceptance:
      - "Repository archived (read-only) on GitHub"
      - "phase_6_deprecation.status set to 'complete' in /tmp/migrationPlan.md"
      - "phase_6_deprecation.archived set to true"
---
```

**Context:** Final step — makes f1r3node read-only and redirects all traffic to f1r3node-rust. This must not happen until all issues, PRs, and external repos are handled.

**Scope:**

- Included: README update, repo metadata, CI disable, archive
- Excluded: Any further development in f1r3node

**Notes:**

- Do NOT archive until Phases 5-7 are confirmed complete
- The other agent in f1r3node should NOT start this until signaled

---

### EPIC-009: Distributed OCI Testbed for Latency Benchmarking

```yaml
---
epic_id: EPIC-009
title: "Distributed OCI Testbed for Latency Benchmarking"
status: in_progress
priority: p2
user_story: US-003
blocked_by: []
created_at: 2026-04-13
claimed_by: claude-session-epic009
claimed_at: 2026-04-13T19:00:00Z
tasks:
  - id: TASK-009-1
    title: "OCI VPS provisioning scripts"
    status: review
    claimed_by: claude-session-epic009
    completed_at: 2026-04-13T19:05:00Z
    acceptance:
      - "scripts/remote/oci-provision.sh creates a dedicated f1r3node-rust-testbed-vcn in us-sanjose-1"
      - "Creates 2x VM.Standard.A1.Flex (arm64 Ampere) instances in f1r3fly-devops compartment"
      - "Security list opens TCP 40400-40405 and UDP 40404 to 0.0.0.0/0 (public testbed)"
      - "SSH access provisioned via a dedicated testbed keypair"
      - "Teardown script (oci-destroy.sh) removes VMs, VCN, and security rules cleanly"
    notes:
      - "Code complete (commit be7ad3f); dry-run validated end-to-end"
      - "Real --apply validation deferred to TASK-009-4+ integration"
      - "Security list range (40400-40405) may need widening in TASK-009-3 to accommodate 3 nodes on VPS-2"

  - id: TASK-009-2
    title: "Image distribution via docker save + scp + load"
    status: review
    claimed_by: claude-session-epic009
    blocked_by: [TASK-009-1]
    completed_at: 2026-04-13T19:10:00Z
    acceptance:
      - "scripts/remote/image-transfer.sh: local docker save | scp | remote docker load"
      - "Works against both VPSes in a single invocation (parallel transfer)"
      - "Image tag matches what distributed compose files reference"
      - "Migration note captured: once OCIR first-publish lands, switch to docker pull on VPS"
    notes:
      - "Code complete (commit 6e045c0); dry-run validated with fabricated state"
      - "Real --apply pending live VPSes"

  - id: TASK-009-3
    title: "Distributed compose file split"
    status: review
    claimed_by: claude-session-epic009
    claimed_at: 2026-04-13T19:15:00Z
    completed_at: 2026-04-13T19:30:00Z
    blocked_by: [TASK-009-1]
    acceptance:
      - "docker/shard.vps1.yml runs bootstrap only; parameterized by BOOTSTRAP_HOST env"
      - "docker/shard.vps2.yml runs 2 validators + observer; connects to BOOTSTRAP_HOST:40400"
      - "No reliance on Docker internal DNS for inter-host communication"
      - "Both files read from a shared .env.remote template"
    notes:
      - "VPS-2 runs 3 rnode processes sharing one public IP; each needs a distinct port-band to avoid protocol-port collision"
      - "Added 3 per-node conf files (validator1-remote.conf, validator2-remote.conf, readonly-remote.conf) that HOCON-include default.conf and override protocol-server.port / peers-discovery.port / api-server.port-*"
      - "Widened oci-provision.sh security list from 40400-40405/tcp+40404/udp to 40400-40455/tcp+40400-40455/udp to cover all 3 port-bands (supersedes TASK-009-1 AC wording)"
      - "Revisit: if node binary exposes --protocol-port / --discovery-port CLI flags, the per-node conf files could be replaced with inline compose args (would drop ~45 lines)"

  - id: TASK-009-4
    title: "Justfile recipes for end-to-end orchestration"
    status: review
    claimed_by: claude-session-epic009
    claimed_at: 2026-04-13T19:40:00Z
    completed_at: 2026-04-13T19:58:00Z
    blocked_by: [TASK-009-1, TASK-009-2, TASK-009-3]
    acceptance:
      - "just vps-up: provisions 2 VPSes and returns their public IPs"
      - "just vps-deploy: scp config + images, start bootstrap (VPS-1), then validators/observer (VPS-2)"
      - "just vps-status [target]: shows shard health via HTTP API and metrics endpoint"
      - "just vps-down: tears down all OCI resources created by vps-up"
    notes:
      - "Justfile prefix renamed oci- -> vps- per user direction to stay cloud-agnostic; BACKLOG-FI-002 captures the AWS/GCP generalization plan"
      - "Added scripts/remote/deploy.sh (renders .env.remote from template, parallel scp, bootstrap-then-followers startup, HTTP /api/status readiness poll)"
      - "Added scripts/remote/status.sh (per-node /api/status + /metrics check, non-zero exit on unhealthy)"
      - "Added scripts/remote/teardown.sh (docker compose down -v on both VPSes, separate from OCI termination)"
      - "Plus convenience recipe vps-image-push wrapping image-transfer.sh"
      - "Dry-run validated end-to-end; full apply-run deferred pending live VPS decision"

  - id: TASK-009-5
    title: "Port latency benchmark (Scala -> native grpcurl/curl)"
    status: review
    claimed_by: claude-session-epic009
    claimed_at: 2026-04-13T21:19:00Z
    completed_at: 2026-04-13T21:25:00Z
    blocked_by: [TASK-009-4]
    acceptance:
      - "scripts/bench/latency-benchmark.sh: drops rust-client external dependency, uses grpcurl + HTTP /api"
      - "Parameterized for arbitrary validator count (not hardcoded to 3)"
      - "Emits load-summary.txt and p50/p95 latency report"
      - "just bench-latency HOST DURATION wraps the script"
      - "scripts/bench/profile-casper-latency.sh ported for Rust node log format"
    notes:
      - "Implementation uses `node deploy` (via docker exec / ssh) for deploy signing rather than raw grpcurl — grpcurl can't produce secp256k1 signatures without a pre-signer binary. The AC intent (drop rust-client external dep) is met; interpretation documented in the script header."
      - "Uses curl for /api/status preflight and node CLI (show-blocks, last-finalized-block) for block/deploy matching. No external-repo dependencies."
      - "Parameterized via --duration, --rate, --host, --container, --http-port, --out-dir flags plus PHLO_LIMIT/PHLO_PRICE/DEPLOYER_KEY env"
      - "Default deployer key is bootstrap's (funded locally and in wallets.txt via commit 993c239 for distributed)"
      - "Justfile recipe: just vps-bench-latency host=<ip> duration=60 rate=2"
      - "profile-casper-latency.sh parses Rust JSON logs (targets f1r3fly.propose.timing + f1r3fly.casper) for per-validator propose_core_ms / block_replay_ms / finalizer_cycle_ms p50/p95"
      - "Real-apply validation against a live shard deferred (same as TASK-009-1..4)"
---
```

**Context:** Stands up a realistic multi-host deployment (single shard distributed across 2 VPSes) to measure network-latency-bound consensus performance. This is distinct from in-process or single-host Docker tests — it exercises the P2P transport, Kademlia discovery, and Casper finalization under real inter-host latency.

**Scope:**

- Included: OCI provisioning, image distribution, distributed compose, deploy/teardown automation, latency benchmark port
- Excluded: Inter-shard consensus (Option B, ~1,500+ LOC of consensus work — see BACKLOG-FI-001)
- Excluded: Non-OCI providers (Tata cloud, etc.)
- Excluded: Throughput, chaos, or whiteblock-plan benchmarks (future epics)
- Excluded: Production-grade secrets management (using `scp` for TLS keys for now)

**Notes:**

- Uses arm64 (VM.Standard.A1.Flex) for free-tier eligibility and production representativeness
- Image distribution intentionally uses `docker save/load` rather than registry pull, to keep this epic self-contained until the OCIR CI switch lands
- TLS keys for bootstrap are shipped via `scp` (acceptable for a throwaway testbed)

---

### EPIC-010: Soak Benchmark Metrics & Reporting

```yaml
---
epic_id: EPIC-010
title: "Soak Benchmark Metrics & Reporting"
status: in_progress
priority: p2
user_story: US-004
blocked_by: []
created_at: 2026-07-15
claimed_by: claude-session-810424d7
claimed_at: 2026-07-15T21:35:00Z
tasks:
  - id: TASK-010-1
    title: "Per-iteration metrics emission in run-merge-recovery-soak.sh"
    status: in_progress
    acceptance:
      - "Each iteration writes metrics.json to its ITERATION_DIR: wall-clock duration, pytest pass/fail/error counts (parsed from pytest.log), provider, exit code"
      - "Run-level summary.json aggregates: iterations, failure rate, iterations/hour throughput, per-provider split, target ref/sha, started/finished timestamps"
      - "summary.json uploaded as a workflow artifact by merge-recovery-soak.yml"

  - id: TASK-010-2
    title: "Node resource + finalization sampling during soak iterations"
    status: in_progress
    blocked_by: [TASK-010-1]
    acceptance:
      - "Peak node RSS per iteration captured (docker stats for docker provider; harness resource_monitor output for subprocess provider) into metrics.json"
      - "Deploy-to-finalized latency samples (p50/p95) extracted per iteration from test_load.py timings or node JSON logs (f1r3fly.propose.timing targets)"
      - "Both metrics roll up into summary.json"

  - id: TASK-010-3
    title: "Week-over-week compare step with release-gate verdict"
    status: in_progress
    blocked_by: [TASK-010-1, TASK-010-2]
    acceptance:
      - "Compare job fetches previous week's summary.json (from the Pages data history) and computes deltas for: failure rate, throughput, peak RSS, finalization latency"
      - "Regression thresholds are configurable in one place; PROPOSED DEFAULTS (need maintainer sign-off, see work log): failure rate +5 percentage points, RSS +20%, finalization p95 +20%, throughput -20%"
      - "Verdict (pass/regress + per-metric deltas) written to verdict.json artifact; a regression marks the soak workflow run failed"
      - "Release workflow refuses to promote unless the latest completed soak verdict is pass (explicit maintainer override documented)"

  - id: TASK-010-4
    title: "GitHub Pages trend dashboard"
    status: in_progress
    blocked_by: [TASK-010-1]
    acceptance:
      - "Pages enabled on the repo (source: GitHub Actions); site at f1r3fly-io.github.io/f1r3node-rust"
      - "Soak workflow appends each summary.json to a data history and redeploys the dashboard"
      - "Dashboard charts all four metrics across weeks with per-provider split and links to per-run artifacts"

  - id: TASK-010-5
    title: "OCI Notifications (ONS) Monday summary email"
    status: in_progress
    blocked_by: [TASK-010-3]
    acceptance:
      - "ONS topic (e.g. soak-benchmark-reports) exists; creation scripted (CLI or Terraform) OR documented as manually provisioned — OPEN QUESTION: who administers the topic (see work log)"
      - "Soak workflow publishes a plain-text Monday summary via instance-principal auth from the OCI runners (no new GitHub secrets): four metrics with week-over-week deltas, gate verdict, dashboard link"
      - "Subscription/unsubscription flow documented for contributors (ONS confirmation + unsubscribe links)"

  - id: TASK-010-6
    title: "Close the two failure modes that hid the soak breaking for days"
    status: pending
    acceptance:
      - "merge-recovery-soak.yml's SYSTEM_INTEGRATION_REF is covered by build_base's pin-drift check, alongside .github/oci-validation.env and _integration-pipeline.yml. It is a THIRD pin site that nobody knew existed: CI's pin advanced to 06f2020c while the soak's sat at a50eeb19, which predated system-integration 81284fc (adding integration-tests/certs/validator4). compose.py bind-mounts that path, so Docker created a directory and every node died on 'Failed to read the X.509 certificate: IO error: Is a directory (os error 21)'. Fixed for now by 4879a1f6; the guard is what stops it recurring."
      - "A schedule-gate no-op is distinguishable from a real pass without opening the run. Two cron slots fire nightly; the 19:30 Pacific slot runs the real soak and the 20:30 slot no-ops and reports success. From 2026-07-27 the real soak failed at bring-up every night while the workflow showed green, because the no-op is the later run. The job already prints a ::notice saying no soak was attempted — that is not enough, since the signal people read is the check mark."
      - "Regression coverage: the soak runs integration-tests/test/tests/custom/test_load.py, which the CI integration matrix explicitly --deselects. Any test only the soak runs needs either CI coverage or an explicit note that the soak is its sole gate, otherwise CI stays green through soak-only breakage."

  - id: TASK-010-7
    title: "Make system-integration's compartment reaper soak-aware (cross-repo)"
    status: pending
    external: true
    external_repo: F1R3FLY-io/system-integration
    coordination_note: "Executed by the agent working in ../system-integration. Coordinate via that repo's docs/ToDos.md — NOT docs/discoveries/, whose *.md contents are gitignored here (.gitignore:123) and so do not survive as a durable trace."
    acceptance:
      - "ci/oci-runners/reap-stale-runners.sh no longer terminates live soak runners. As of pin 9ebdde01 its OCI query filters ONLY on lifecycle-state == RUNNING and time-created < now - MAX_AGE_HOURS (default 6) — no display-name filter and no freeform-tag check — so it is blind to the soak-deadline-epoch exemption added by f1r3node-rust PR #169 and would kill a 22h/60h soak at hour 6. LATENT, NOT ACTIVE: no workflow schedules it at that SHA (.github/workflows contains only smoke-test.yml), so the hazard is a manual invocation. Fix mirrors ci-runner-reaper.yml: restrict to the ephemeral name prefixes and honour soak-deadline-epoch before terminating."
      - "Same script must also stop terminating long-lived golden images (ci-runner-golden-*), which the unfiltered age query sweeps up too; this is the reaper gap the system-integration agent previously supplied a diff for."
      - "Soak runners carry their own name prefix. launch-runner.sh builds RUNNER_NAME=ci-eph-$REPO_SLUG-$ARCH-$TS-$RAND, so a soak VM is indistinguishable from a 45-minute CI runner by name alone and any future age-based rule matches it by accident."
      - "cloud-init-runner.yml.tmpl schedules an on-instance self-destruct sized to a per-run dollar budget (~$12 daily / ~$33 weekend at VM.Standard.E6.Flex 16 OCPU / 32GB per state.env) — the last line of defence when both GitHub and the reaper fail."
      - "Soak VMs carry a cost-tracking freeform tag, with a monthly OCI budget and 80/100% alerts scoped to it. Note the enforcement is the VM lifetime, not the budget: OCI budgets are monthly and alert-only and cannot stop a running resource."
      - "launch-runner.sh tags the instance atomically at creation (oci compute instance launch --freeform-tags) rather than leaving it to a follow-up update. Validation run 30584775602 proved why: the launcher returns as soon as OCI accepts the launch call, but the instance keeps transitioning through PROVISIONING, and `instance update` against it is refused with HTTP 409 'currently being modified, try again later' — 3s after launch, which failed the whole launch job. f1r3node-rust now retries for ~3min (commit ea566d8a), which works but is a workaround: tagging at creation removes the race entirely and is the only way a tag can be guaranteed present from the instance's first instant, closing the window in which a reaper could see an untagged soak VM. Applies equally to the cost-tracking tags requested above."
      - "conftest.py's --rss-ceiling-mb default (5000, conftest.py:94) is raised to a host-relative value. This is a defect, not a tuning preference, and our SOAK_RSS_CEILING_MB override is a workaround that leaves it armed for every other caller. test_load.py fixes its shard at 6 nodes (test_load.py:220, '4 genesis validators (6 nodes total with boot + readonly)', include_readonly=True at :232), and that shard peaks ~9.9-10.8GB on ANY host — so the default sits at roughly half the working set of the harness's own primary load test, and kills it identically on a 64GB workstation. It is correct only on genuinely small hosts (<~12GB), where the test cannot run anyway, which is what makes the flat value look defensible. Why it went unnoticed: _integration-pipeline.yml:482 --deselects test_load.py, so CI never runs it and the soak was its only automated caller — and the soak never got past bring-up until 2026-07-30. Suggested shape: max(floor, MemTotal - headroom), keeping 5000 as the small-host case. Sequence after a clean soak: it is a shared default touching every caller. Also note --host-free-floor-mb (conftest.py:105, default 2000, subprocess-only) is a second always-on guard the ceiling override does not touch."

  - id: TASK-010-8
    title: "De-duplicate the CI runner compartment OCID without weakening the reaper"
    status: review
    claimed_by: claude-session-9f68c6fa
    completed_at: 2026-07-30T22:20:00Z
    branch: chore/reaper-compartment-invariant
    notes:
      - "Resolved by asserting equality rather than de-duplicating: check-workflow-invariants.sh gained invariant 5, which fails CI when the two literals diverge or when neither file pins one any more. Both sites now carry cross-referencing comments naming the other and the enforcing check."
      - "The de-duplication framing in the first acceptance line was the wrong shape and is superseded by the second: a repo variable is admin-mutable, and the reaper's blast-radius guarantee depends on the value being immutable in-repo. Equality-under-CI keeps both properties."
      - "Mutation-tested: a divergent OCID fails, and removing both literals fails with a message naming the cause. That testing caught a real defect in the guard itself — under set -e a no-match grep inside a command substitution killed the script before it could print why, making the 'nobody pins it any more' branch unreachable. Fixed with `|| true` on both greps; a guard that cannot explain itself is the failure mode this file exists to prevent."
      - "OPEN — cross-repo blind spot, raised by claude-session-02f66bb7. There is a THIRD site holding this OCID that the invariant cannot see: system-integration's ci/oci-runners/state.env COMP, which launch-runner.sh uses to CREATE instances and reap-stale-runners.sh uses to scan them. Verified byte-identical today. Not guarded here because the check would need a network fetch of the pinned SYSTEM_INTEGRATION_REF inside the Lint job, and because divergence there fails CLOSED rather than silently: the launcher would create the instance in one compartment while our tagging step lists the other, find no instance, and fail the launch. Loud and immediate, unlike the same-repo divergence this invariant guards, which would be silent until a soak died at 2h. Revisit if a cheap deterministic check appears — the ref is pinned, so a fetch would be reproducible."
    acceptance:
      - "CI_RUNNER_COMPARTMENT_OCID stops being hardcoded in two places — .github/workflows/ci-runner-reaper.yml and the 'Exempt runner from the CI reaper' step in .github/workflows/merge-recovery-soak.yml. A compartment migration currently needs coordinated edits, and changing only one side silently leaves soak runners either untagged (reaped mid-run) or un-reapable."
      - "The chosen mechanism does not weaken the reaper's blast-radius guarantee. A repo-level Actions variable is mutable by anyone with repo admin, whereas the present hardcoding is precisely why the reaper 'can never touch other compartments' (its own comment, which is load-bearing). Preferred option: keep both literals pinned in-repo and add an assertion to .github/scripts/check-workflow-invariants.sh that they match, so drift fails CI while the value stays immutable."
      - "Raised by xai in the PR #169 multi-review and deliberately deferred from that PR: it touches the reaper's security posture and should not ride a same-day hotfix."
---
```

**Context:** Implements US-004 plus the delivery/reporting design agreed 2026-07-15: the 72h soak concludes Mondays (weekly cadence); metrics are published to a GitHub Pages trend dashboard (pull) and a plain-text ONS email (push); regressions gate releases. Full design rationale, alternatives considered (email-only, Discussions, bot-committed reports), and open questions are in `docs/work-logs/task-EPIC-010-2026-07-15T20-57Z.md`.

**Scope:**

- Included: metrics emission, resource sampling, compare+gate, Pages dashboard, ONS email
- Excluded: PR #72 residual par-serialization benchmarking (separate concern)
- Excluded: HTML email formatting (ONS is plain-text; detail lives on the dashboard)

---

### EPIC-015: Casper Test Infrastructure Congruence

```yaml
---
epic_id: EPIC-015
title: "Casper Test Infrastructure Congruence"
status: pending
priority: p2
user_story: US-005
blocked_by: []
created_at: 2026-08-11
claimed_by: null
claimed_at: null
tasks:
  - id: TASK-015-1
    title: "Deepen the Casper test node"
    status: pending
    priority: p2
    discovered_in: docs/work-logs/casper-test-node-congruence-baseline-2026-08-19.md
    notes:
      - "Baseline 2026-08-19: the duplicate casper/tests helper tree and the canonical casper/src/rust/test_utils tree diverged by roughly 1,000 lines; the duplicate tree holds create_network_with_deploy_lifespan and the MultiParentCasper-typed accessor, which the canonical tree lacks. The first consolidation attempt (976b7a252, PR #230) is superseded. See the discovered_in work log for the full baseline and provenance."
    glossary_terms:
      - docs/Glossary.md#test-node
      - docs/Glossary.md#block-proposal
      - docs/Glossary.md#block-validation
    dependency_category: local-substitutable
    accepted_design: common-caller
    tdd_plan: docs/tdd-plans/casper-test-node-2026-08-11T02-59-57Z.md
    acceptance:
      - "Standalone and network scenarios exercise production-shaped behavior through the test node interface documented at docs/Glossary.md#test-node"
      - "The common caller can create a standalone test node or a configured test network without learning storage, runtime, transport, or consensus-construction details"
      - "The test network interface exercises block proposal, publication, propagation, synchronization, and block validation while preserving existing observable outcomes"
      - "Empty-block behavior, bootstrap selection, parent limits, synchrony settings, and read-only nodes remain expressible as explicit configuration with behavior tests"
      - "Focused inspection required by tests crosses named test node accessors; tests do not initialize or copy fields of the consensus implementation"
      - "Local storage, runtime, and transport stand-ins remain at internal seams; tests do not mock internal collaborators"
      - "Features that exist only in the duplicate tree (at minimum create_network_with_deploy_lifespan and the MultiParentCasper-typed accessor) are ported to the canonical fixtures before the duplicate tree collapses to re-exports"
      - "Old duplicate fixture tests are replaced rather than layered, and removing the duplicate helper tree does not move construction complexity into callers"
      - "Each TDD cycle covers one behavior at a time, and test names cite docs/Glossary.md#test-node plus any applicable block-proposal or block-validation anchor"
---
```

**Context:** Candidate C1 from the Casper architecture regression diagnosis was accepted with the common-caller design. Two independently evolving helper trees currently expose overlapping test-node behavior, while integration tests retain direct access to consensus implementation fields.

**Scope:**

- Included: one canonical test-node module, standalone and configured-network entry points, network scenario operations, focused inspection accessors, caller migration, and duplicate-tree removal
- Excluded: changing consensus semantics, reopening settled slashing decisions, introducing remote ports, or implementing deploy-admission and block-validation candidates C2 and C3

---

### EPIC-016: Key-Contention Starvation Close-Out

```yaml
---
epic_id: EPIC-016
title: "Key-Contention Starvation Close-Out"
status: pending
priority: p1
user_story: null
issues: [294, 317, 104]
blocked_by: []
created_at: 2026-08-22
updated_at: 2026-08-22
claimed_by: null
claimed_at: null
execution_contract:
  base_branch: feat/key-contention-phase2
  branch: fix/key-contention-base-bias
  stack: "PR #299 (phase 1) -> PR #312 (phase 2) -> this branch -> PR #311 (formal, merges last). PR #311 merges this branch's production changes without history rewriting and discharges the dag_merger.rs claim against the final head."
  issue_policy: "Put Refs #294, #317, and #104 in the PR body. Close #294 only after every valid proposer schedule has deterministic termination and the fix is promoted to master."
  ratified_2026_08_22:
    - "The protected-cell cardinality invariant is enforced at block validation (REPLAY). This is a validation-rule change."
    - "A protected single-value cell is authenticated by mergeable tag metadata only. Untagged integer datums are ordinary Rholang."
    - "Typed rejection reasons travel on the wire in RejectedDeploy. Node-local deferral (BlockNotHeld) is never a wire reason."
    - "The 2026-08-22 CI failure is not C1 soak evidence. It had no frontier-lease escape and no rival contention."
tasks:
  - id: TASK-016-1
    title: "Authenticated single-value-cell classification"
    status: pending
    priority: p1
    issues: [104]
    discovered_in: docs/work-logs/shared-shard-single-number-cell-starvation-2026-08-22.md
    claimed_by: null
    blocked_by: []
    notes:
      - "Two classifiers exist today: binary_data_is_single_number in dag_merger.rs and check_single_value_cell_not_overfilled in rholang_merging_logic.rs. Both classify by datum shape, which is guessable from content."
      - "The Rocq section Overfill in formal/rocq/merge_algebra/theories/ConflictSoundness.v proves guard detection at merge time only. Its header must state that boundary. That edit belongs to PR #311."
    acceptance:
      - "One classifier in rholang/src/rust/interpreter/merging/rholang_merging_logic.rs decides protection from authenticated mergeable tag metadata, never from datum shape"
      - "rholang_merging_logic.rs carries cbc=mandatory in .gitattributes"
      - "A unit test shows two integer datums on an untagged channel merge without rejection, and a second produce onto a tagged number cell is rejected"
      - "The shared-shard reproduction recipe in the discovered_in work log no longer rejects"

  - id: TASK-016-2
    title: "Use a fresh channel per shared-shard deploy in system-integration"
    status: pending
    priority: p1
    issues: [294]
    discovered_in: docs/work-logs/shared-shard-single-number-cell-starvation-2026-08-22.md
    claimed_by: null
    blocked_by: []
    repo: system-integration
    notes:
      - "_deploy_and_wait in test_web_api.py deploys @{2000+i}!(i) from one key in 16 tests. Only the second write to each channel is hazardous, so the failing test moves with xdist ordering."
      - "Cross-repo change. Commit in a system-integration session, then bump SYSTEM_INTEGRATION_REF at all three sites here."
      - "Handed off 2026-08-22 as SI-TASK-016-2 in system-integration docs/ToDos.md: branch fix/shared-shard-fresh-deploy-channels off dev, PR title fix(shared): deploy onto a fresh channel per _deploy_and_wait call. The system-integration agent owns branch, commit, and PR; it posts the merged SHA back in that entry."
      - "2026-08-22: system-integration PR #127 merged to dev (5e6dbfbb) and promoted to main via PR #128. SYSTEM_INTEGRATION_REF repinned at all three sites (.github/oci-validation.env, _integration-pipeline.yml, merge-recovery-soak.yml) from 56884ab to main 3e5b5eb89, which also carries the PR #129 shard-port reservation fixes. Three-run gate not yet run."
      - "This fixture fix hides the trigger. It does not repair the lineage-dependent semantics; TASK-016-1 and TASK-016-3 do."
    acceptance:
      - "Each _deploy_and_wait call produces onto a channel no other test in the shared shard writes"
      - "Three consecutive green runs of the four integration matrices on dev after the repin"

  - id: TASK-016-3
    title: "Enforce protected-cell cardinality at block validation"
    status: pending
    priority: p1
    issues: [104, 294]
    claimed_by: null
    blocked_by: [TASK-016-1]
    notes:
      - "Composed invariant (PR #311): for every accepted block and every authenticated protected number cell, the block post-state holds at most one datum, regardless of block partition, parent order, main-parent identity, carrier lineage, retry count, proposer identity, or PLAY versus REPLAY."
      - "A produce-only write that lands through the main-parent lineage never reaches the merge guard. Validation of the block's own deploy effects against its pre-state closes that path."
      - "This is a consensus-visible validation-rule change. It needs upgrade coordination and a decision-record row. It supersedes the C1 acceptance line that forbids validation changes for this one rule."
    acceptance:
      - "A block whose own deploys push a protected cell past one datum is invalid at REPLAY on every validator"
      - "The same admissibility decision results for two produces in one carrier block and for each produce in a separate block"
      - "The negative control from PR #311 (merge-only guard, main-parent admission off, second produce through lineage) fails validation on the repaired head"
      - "The enforcement file carries cbc=mandatory"
      - "Decision-record row in docs/casper/CONSENSUS_PHILOSOPHY.md names the validation change and its coordination cost"

  - id: TASK-016-4
    title: "Typed rejection reasons and retry eligibility"
    status: pending
    priority: p1
    issues: [294, 317]
    claimed_by: null
    blocked_by: [TASK-016-1]
    notes:
      - "RejectedDeploy carries only sig, duplicate, and carrier. Recovery and loss priority cannot tell a retryable loss from a terminal one, so a permanently unsafe deploy gains unbounded priority."
      - "Reason table (PR #311): CollateralChainDrop retryable; MergeConflict retryable; DuplicateOccurrence not retryable; SafetyInvariantViolation terminal. Missing consensus history is deferral, never a rejection reason."
      - "The reason field changes block-body encoding. Every validator must derive the same reason from authenticated inputs or InvalidRejectedDeploy disagrees."
    acceptance:
      - "RejectedDeploy in CasperMessage.proto carries a reason enum with the four wire reasons"
      - "Each merge rejection site assigns one deterministic reason, and block validation recomputes the same reason"
      - "prior_rejections counts retryable losses only"
      - "Recovery does not re-package a deploy whose latest rejection is terminal, and the deploy lifecycle reports the terminal state"
      - "The encoding change is named as a hard fork in the PR body"

  - id: TASK-016-5
    title: "Implement the C1 base-bias remedy over the admissible parent set"
    status: pending
    priority: p1
    issues: [294, 317]
    claimed_by: null
    blocked_by: [TASK-016-3, TASK-016-4]
    notes:
      - "Option C1 from the remedy ladder: a proposer whose parent set holds a sibling carrying a chain with strictly more retryable prior rejections declares that sibling as parents[0]."
      - "GuardBridge.v proves promotion is deterministic but not that the promoted parent is state-safe. A parent is promotable only when its post-state passes the TASK-016-3 validation."
      - "PR #312 Limits: no proof of termination under fixed-proposer main-parent base bias. The ignored fixed-proposer test in casper/tests/batch2/loss_priority_spec.rs is the expected-RED sentinel."
    acceptance:
      - "C1 selects from the admissible parent set only and considers retryable rejections only"
      - "The fixed-proposer sentinel test is no longer ignored and passes"
      - "Fork choice stays deploy-content-blind (Principle P4); parent promotion is proposer policy computed from authenticated data"
      - "Decision-record row in docs/casper/CONSENSUS_PHILOSOPHY.md marks C1 ratified"

  - id: TASK-016-6
    title: "Enable User Contract Concurrency and fail on contention expiry"
    status: pending
    priority: p2
    issues: [294]
    claimed_by: null
    blocked_by: [TASK-016-5]
    notes:
      - "User Contract Concurrency was waived as a merge gate for PR #299 and PR #312. The waiver requires this follow-up."
    acceptance:
      - "The ucc integration matrix runs by default instead of skipping"
      - "A contention expiry fails the run"
      - "Three consecutive green runs"

  - id: TASK-016-7
    title: "Run three consecutive 60-minute contention soaks"
    status: pending
    priority: p2
    issues: [294, 317]
    claimed_by: null
    blocked_by: [TASK-016-5, TASK-016-6]
    notes:
      - "Ratified in PR #312: each soak uses both supported providers. Start C2 only if one soak expires a valid deploy after retry_frontier_escape."
      - "The 2026-08-22 shared-shard CI failure is not soak evidence for or against C1."
    acceptance:
      - "Three consecutive soaks on the final phase-2 head with no expired valid deploy"
      - "Soak run IDs and head SHA recorded in this task"
---
```

**Context:** PR #299 removed content-deterministic adjudication and PR #312 added merged-frontier retry packaging with a three-block lease. Neither proves termination under fixed-proposer main-parent base bias. The 2026-08-22 CI analysis found a third facet: the single-value-cell guard is a local merge lemma whose production bridge is incomplete. A produce-only second write lands through the main-parent lineage without reaching the guard, the cell then holds two datums, and loss priority cannot reorder a chain that has no rival. The repair is an authenticated classifier, validation-time enforcement, and typed rejection reasons so that C1 promotes only state-safe carriers of retryable losses.

**Scope:**

- Included: the authenticated classifier, REPLAY enforcement, wire rejection reasons with retry eligibility, C1 over the admissible parent set, the sentinel flip, UCC enablement, the soak evidence, and the system-integration fixture handoff
- Excluded: C2 and C3 (C3 stays rejected: fork choice must remain deploy-content-blind), the formal composition proofs and cardinality-aware state models (PR #311), and the Rocq section Overfill header edit (PR #311)

---

## Epic Dependency Graph

```text
PR #390 meeting record + PR #216 candidate ─> EPIC-017 pre-merge plan and baseline
PR #430 ─> PR #431 ─> PR #432 ─> PR #433 ─> EPIC-017 harness prerequisites
EPIC-010 / EPIC-012 / EPIC-015 / EPIC-016 ─> EPIC-017 shared evidence and fixtures
EPIC-017 handoff + PR #216 merged into dev ─> EPIC-018 post-merge formal harness PR
EPIC-020 (node log and accept-path limits, fix branch -> dev) ─> merges before PR #447
EPIC-017 harness + PR #441 stage metrics ─> EPIC-021 (issue #24 root cause, fix/issue-24 -> dev)
EPIC-019 (node observation, PR #447 -> dev) ─> EPIC-017 TASK-017-12 node prerequisite (soak branch)
EPIC-011 (TLA exhaustive baseline, complete) ─> EPIC-012 / TASK-012-22
EPIC-012 (open-issue PR queue)              (all other lanes start independently)
PR #299 ─> PR #312 ─> EPIC-016 (key-contention close-out) ─> PR #311 (formal, merges last)

EPIC-001 (system-integration alignment)    EPIC-003 (f1r3node: merge critical PRs)
EPIC-002 (monitoring separation)               |
                                                 v
                                            EPIC-004 (f1r3node-rust: code sync)
                                                 |
                                            +----+----+----+
                                            |    |    |    |
                                            v    v    v    v
                                          005  006  007
                                        (issues)(repos)(PRs)
                                            |    |    |
                                            +----+----+
                                                 |
                                                 v
                                            EPIC-008
                                         (deprecation/archive)
```

---

## Task States

| Status | Meaning | Next Action |
|--------|---------|-------------|
| `pending` | Not started | Available to claim |
| `in_progress` | Being worked on | Continue or handoff |
| `blocked` | Waiting on dependency | Check `blocked_by` |
| `review` | Ready for review | Review and approve |
| `complete` | Done | Move to CompletedTasks.md |

---

## Workflow

1. **Find next task**: Use `/nextTask` to identify the highest priority unclaimed task
2. **Claim task**: Use the [Implementer Identification](https://gitlab.com/smart-assets.io/gitlab-profile/-/blob/master/docs/common/stigmergic-collaboration.md#implementer-identification) format for `claimed_by`. Set `status: in_progress`
3. **Implement**: Use `/implement` to execute with full context
4. **Complete**: Mark `status: complete` when acceptance criteria met
5. **Move epic**: When all tasks complete, move epic to `docs/CompletedTasks.md`

---

## References

- **Shared Migration Plan:** `/tmp/migrationPlan.md`
- **User Stories:** `docs/UserStories.md`
- **Completed Work:** `docs/CompletedTasks.md`
- **Backlog:** `docs/Backlog.md`
- **System-Integration Migration Plan:** `../system-integration/docs/migration-to-rust-node.md`
