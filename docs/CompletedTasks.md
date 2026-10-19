---
doc_type: completed_tasks
version: "1.0"
last_updated: 2026-10-06
---

# Completed Tasks

This document archives completed epics and tasks for historical reference and progress tracking.

**Document Structure**
- Active work: `docs/ToDos.md`
- User stories: `docs/UserStories.md`
- Completed work: This file (`docs/CompletedTasks.md`)
- Deferred work: `docs/Backlog.md`

---

## Completed Epics

<!-- Epics are listed in reverse chronological order (newest first) -->

---

### EPIC-020: Node Log and Accept-Path Self-Limits

```yaml
---
epic_id: EPIC-020
title: "Node Log and Accept-Path Self-Limits"
status: complete
completed_date: 2026-10-06
priority: p0
user_story: US-009
user_flow: FLOW-002
blocked_by: []
created_at: 2026-09-23
updated_at: 2026-10-06
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
    work_log: docs/work-logs/archived/EPIC-020/transport-accept-resource-review-20260923.md
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
    work_log: docs/work-logs/archived/EPIC-020/task-020-2-byte-bounded-logging-20260930.md
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
    work_log: docs/work-logs/archived/EPIC-020/task-020-3-deployment-log-caps-20260930.md
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
    status: complete
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

**Current state (archived 2026-10-06):** TASK-020-1, TASK-020-2, and TASK-020-3 merged to dev through PR #451 on 2026-10-03 and reached master with the soak stack. TASK-020-4 is implemented on PR #622: the soak guardian samples the node log directory and the container json-file size, and CLAIM-SOAK-001 records the log caps as enforced. The maintainer accepted the first version on 2026-10-04 and the probe fix on 2026-10-05. Soak 37343966570 runs the guardian for the first time. Also open: the TASK-020-1 verification status and the system-integration main promotion revision of TASK-020-3.

---

### EPIC-011: TLA+ Exhaustive Tier Red→Green (3-Validator Detector Coverage)

```yaml
---
epic_id: EPIC-011
title: "TLA+ Exhaustive Tier Red→Green (3-Validator Detector Coverage)"
status: complete
completed_date: 2026-08-19
priority: p0
user_story: null
blocked_by: []
created_at: 2026-08-05
claimed_by: claude-session-917f64e8
claimed_at: 2026-08-05T00:00:00Z
tasks:
  - id: TASK-011-1
    title: "Add run_exhaustive workflow_dispatch input to slashing-tests.yml"
    status: complete
    claimed_by: claude-session-917f64e8
    completed_at: 2026-08-05T13:20:00Z
    branch: fix/tla-3v-liveness-split
    notes:
      - "Implemented in commit 7e38ab1f; YAML validated, all 9 check-workflow-invariants.sh invariants pass. Local-only for now per maintainer — branch not pushed."
    acceptance:
      - "workflow_dispatch gains a boolean input run_exhaustive (default false)"
      - "tla-model-check job sets RUN_EXHAUSTIVE_TLA=1 only when the input is true; push/pull_request/schedule behavior is unchanged (nightly keeps gating on the 8 fast configs)"
      - "Change lives on a feature branch from dev (fix/tla-3v-liveness-split) so the exhaustive tier can be dispatched against the branch before anything merges"

  - id: TASK-011-2
    title: "Dispatch the exhaustive tier and capture the red run"
    status: complete
    claimed_by: claude-session-917f64e8
    completed_at: 2026-08-05T13:58:00Z
    blocked_by: [TASK-011-1]
    notes:
      - "RESCOPED to local-only per maintainer (2026-08-05): red baseline captured via RUN_EXHAUSTIVE_TLA=1 TLC_PER_CONFIG_TIMEOUT=6m locally instead of CI dispatch. Result: 8 fast configs OK; MC_EquivocationDetector, MC_EquivocationDetectorEager_3v, MC_EquivocationDetector_safety each distinctly labeled TIMEOUT at the cap; run failed (red for the right reason). Full-45m evidence: 11 nightlies 2026-07-25..08-04 + 2026-08-04 local repro. CI-dispatch red baseline deferred to push time. Full log in TDD plan cycle_log (B1)."
      - "DISCOVERED: script roll-up says 'FAILED: N config(s) violated invariants' for cap timeouts — mislabels timeout as violation; candidate fix tracked as TDD plan B7 pending ratification."
    acceptance:
      - "gh workflow run slashing-tests.yml --ref <branch> -f run_exhaustive=true executed (env -u GITHUB_TOKEN)"
      - "Run goes red with all three exhaustive-tier configs (MC_EquivocationDetector, MC_EquivocationDetectorEager_3v, MC_EquivocationDetector_safety) reported as TIMEOUT at the 45m per-config cap — distinctly labeled as timeouts, not invariant violations"
      - "Run URL and per-config outcomes recorded in the epic work log as the red baseline"

  - id: TASK-011-3
    title: "Split MC_EquivocationDetectorEager_3v into safety + bounded liveness configs"
    status: complete
    claimed_by: claude-session-917f64e8
    completed_at: 2026-08-05T14:50:00Z
    blocked_by: [TASK-011-2]
    notes:
      - "PREMISE CORRECTED 2026-08-05 (maintainer-ratified; supersedes the title and first acceptance line): MC_EquivocationDetectorEager_3v is already safety-only — the Eager rewrite checks liveness as the Inv_LivenessAsSafety invariant and the .cfg has no PROPERTY line, so its 45m timeouts are state-space cost (3v×3s×2b), not liveness-graph blowup. There is no split to make. Fix = bound-tightening: new MC_EquivocationDetectorEager_3v2s (3 validators × 2 seqnums × 2 blocks, full _3v invariant list, symmetry) for the nightly tier; full 3v×3s×2b stays exhaustive. The liveness/safety split remains valid for MC_EquivocationDetector (its .cfg has PROPERTY Live_DetectionComplete) — that is TASK-011-5's scope. TDD plan B2 (merged with B3) tracks execution."
      - "GREEN 2026-08-05T14:48Z: MC_EquivocationDetectorEager_3v2s completes in 2m08s (57.2M states generated, 5.72M distinct, depth 37) with ZERO violations — first-ever completed detector check at 3 validators; the stop-on-violation contingency did not fire. Fast-tier regression suite clean (8/8 OK). Model files created; tier-list wiring is TASK-011-4."
    acceptance:
      - "formal/tlaplus/slashing/ gains MC_EquivocationDetectorEager_3v_safety.{tla,cfg} (INVARIANTS only) and MC_EquivocationDetectorEager_3v_liveness.{tla,cfg} (PROPERTIES, constants bounded to complete under the cap), mirroring the existing MC_EquivocationDetector_liveness pattern (~3s where the combined config times out)"
      - "Both new configs still model 3 validators — bounding must not reduce validator count, or the coverage-gap fix is illusory"
      - "Both complete locally well under TLC_PER_CONFIG_TIMEOUT=45m via scripts/ci/check-tla-invariants.sh"
      - "CONTINGENCY: if the liveness config, completing for the first time at 3 validators, reports a genuine counterexample, this task stops and the trace is reported — green then comes from an algorithm/model fix investigated under a new task, not from tuning the model until it passes"

  - id: TASK-011-4
    title: "Restore _3v coverage to the nightly tier, sync docs, capture the green run"
    status: complete
    claimed_by: claude-session-917f64e8
    completed_at: 2026-08-05T15:12:00Z
    blocked_by: [TASK-011-3]
    notes:
      - "MC_EquivocationDetectorEager_3v2s added to POST_FIX_CONFIGS (one line); 14-test-plan §14.6/§14.9 synced to the corrected diagnosis. Local green run: 9/9 OK, _3v2s 128s, ~4.2 min total. Per the local-only rescope, the CI-dispatch green run is deferred to push time (TASK-011-5 / plan B6)."
    acceptance:
      - "scripts/ci/check-tla-invariants.sh adds the two new _3v configs to the default (nightly-gating) tier; combined MC_EquivocationDetectorEager_3v stays in the exhaustive tier as the unbounded reference"
      - "docs/casper/theory/slashing/design/14-test-plan.md §14.6/§14.9 updated to match the new tier membership"
      - "Default-tier dispatch (or PR run) goes green with the _3v configs included; run URL recorded next to the red baseline"
      - "Edits to check-tla-invariants.sh stay minimal to keep the pending PR #198 reconciliation conflict (namespaced entries + fail-on-missing) tractable"

  - id: TASK-011-5
    title: "Apply the same split to MC_EquivocationDetector"
    status: complete
    claimed_by: claude-session-917f64e8
    completed_at: 2026-08-05T19:40:00Z
    blocked_by: [TASK-011-4]
    notes:
      - "Split treatment landed 2026-08-05: MC_EquivocationDetector_liveness_2v (2v×1s×2b) verifies Live_DetectionComplete at 2 validators in 8s, wired into the nightly tier (10/10 green, ~4.3 min). Safety half (MC_EquivocationDetector_safety) already existed."
      - "Exhaustive-dispatch acceptance resolved via the DOCUMENTED arm (CI run 31027278093 at 36ea59b8): 10 fast configs OK in CI, 3 unbounded references TIMEOUT at 45m with the corrected roll-up ('3 cap timeout(s), 0 violation-or-error(s)'); accepted-unbounded status documented in 14-test-plan §14.6 and the run_exhaustive input description. Green-on-schedule exhaustive coverage is the ratified follow-up (profile → measured caps → schedule), outside EPIC-011."
    acceptance:
      - "MC_EquivocationDetector gets the same safety/liveness split treatment once the _3v recipe is proven (its safety half, MC_EquivocationDetector_safety, already exists — the liveness half is the new work)"
      - "Exhaustive tier dispatch goes fully green, or remaining timeouts are explicitly accepted and documented as unbounded-reference runs"
---
```

**Context:** The "TLA+ invariant check" nightly job was red on every run from its start (2026-07-25) until hotfix PR #201 — not from invariant violations, but because `MC_EquivocationDetector` and `MC_EquivocationDetectorEager_3v` hit the 45-minute per-config cap (interleaved liveness checking goes superlinear; locally reproduced over a 106M-state graph). PR #201 parked them behind `RUN_EXHAUSTIVE_TLA=1`, but no CI path sets that variable, so the exhaustive tier currently never runs anywhere. Meanwhile the nightly tier checks the inherently multi-validator equivocation property at ≤2 validators, because `_3v` is the only 3-validator detector model (flagged by spreston8 in the PR #201 review).

**Plan (confirmed with maintainer 2026-08-05):** make the exhaustive tier dispatchable, run it **red** (honest timeout-red — TLC has never found a violation in these models; every config that completes, passes), then make it **green** via the causal liveness/safety split — not by raising the cap (see the causal-diagnosis-before-resources rule). The one open risk is deliberate: these liveness properties have never completed at 3 validators, so the split may surface a real counterexample, in which case the green path becomes an algorithm/model fix (TASK-011-3 contingency).

**Scope:**

- Included: dispatch input, red baseline run, `_3v` safety/liveness split, nightly-tier restoration, docs sync, follow-on `MC_EquivocationDetector` split
- Excluded: raising `TLC_PER_CONFIG_TIMEOUT`; reducing validator count to make models cheap; the cargo-mutants nightly deadline overrun (separate, second independent nightly red — still untracked)
- Coordination: touches `scripts/ci/check-tla-invariants.sh`, which the pending PR #198 reconciliation also modifies — keep tier-list edits minimal

---

---

### EPIC-001: System-Integration Alignment

```yaml
---
epic_id: EPIC-001
title: "System-Integration Alignment"
status: complete
completed_date: 2026-08-19
priority: p1
user_story: US-001
blocked_by: []
created_at: 2026-03-19
claimed_by: null
claimed_at: null
tasks:
  - id: TASK-001-1
    title: "Align genesis wallets.txt with system-integration (20 wallets, validator3=500T)"
    status: complete
    acceptance:
      - "docker/genesis/wallets.txt matches system-integration/genesis/wallets.txt (20 lines)"
      - "Validator3 balance is 500000000000000000 (500T)"
      - "All 12 additional test wallets present"

  - id: TASK-001-2
    title: "Standardize compose env var naming (F1R3FLY_RUST_IMAGE -> F1R3FLY_IMAGE)"
    status: complete
    acceptance:
      - "All compose files use F1R3FLY_IMAGE instead of F1R3FLY_RUST_IMAGE"
      - "DEVELOPER.md and docker/README.md updated"

  - id: TASK-001-3
    title: "Standardize Docker network name to f1r3fly-shard"
    status: complete
    acceptance:
      - "shard.yml network named f1r3fly-shard"
      - "observer.yml and validator4.yml reference f1r3fly-shard as external network"

  - id: TASK-001-4
    title: "Verify shard starts with updated genesis and network config"
    status: complete
    claimed_by: claude-session-epic009
    completed_at: 2026-04-13T20:55:00Z
    blocked_by: []
    acceptance:
      - "docker compose -f docker/shard.yml up succeeds"
      - "Genesis ceremony completes with 20-wallet wallets.txt"
      - "Observer and validator4 can join via f1r3fly-shard network"
    notes:
      - "All 3 written ACs verified end-to-end with locally built f1r3fly-rust:local image"
      - "Bonding extension also verified: added validator4's REV address (1111La6tHaCt...jtEi3M) to wallets.txt as genesis funding, then deployed bond.rho signed by validator4, propose included in block with errored=false and cost=167749 phlo, bond-status flipped to 'Validator is bonded', validator4 proceeded to produce 6+ blocks via heartbeat"
      - "Root cause of earlier insufficient-funds error: validator4.yml was designed for runtime bonding but validator4's REV address was never added to genesis wallets.txt. Fix is a single-line addition."
      - "REV-address computation done via `node eval` on 1.know_ones_vaultaddress.rho (output in docker stdout of the evaluating node)"
---
```

**Context:** The `system-integration` repo orchestrates this node via Docker Compose and shardctl. It has a 6-phase migration plan (see `system-integration/docs/migration-to-rust-node.md`) to make f1r3node-rust the sole node implementation. Phase 1 requires genesis and compose alignment in this repo.

**Scope:**

- Genesis wallets.txt sync (critical blocker for system-integration Phase 1)
- Compose env var and network name standardization
- Validation that shard starts correctly

**Notes:**

- system-integration currently targets branch `dev` in its services.yml, but this repo uses `master` as its working branch. system-integration will need to update its branch reference.
- standalone.yml keeps its own network name (`f1r3fly-standalone`) since it's isolated by design.

---

---

### EPIC-002: Separate Monitoring from Shard Compose

```yaml
---
epic_id: EPIC-002
title: "Separate Monitoring from Shard Compose"
status: complete
completed_date: 2026-08-19
priority: p2
user_story: US-001
blocked_by: []
created_at: 2026-03-19
claimed_by: null
claimed_at: null
tasks:
  - id: TASK-002-1
    title: "Extract Prometheus and Grafana into docker/monitoring.yml"
    status: complete
    claimed_by: claude-session-epic009
    completed_at: 2026-04-13T21:35:00Z
    acceptance:
      - "docker/monitoring.yml contains prometheus and grafana services"
      - "monitoring.yml joins f1r3fly-shard as external network"
      - "shard.yml no longer contains prometheus/grafana services"
      - "docker/README.md updated to reflect new file"
    notes:
      - "Verbatim service-block move; same container names, ports, volumes, env"
      - "Also updated Justfile shard-down to include monitoring.yml teardown"
      - "Also updated docker/vps-cloud-testing.md Part A to reflect opt-in monitoring"
---
```

**Context:** system-integration manages monitoring as a separate compose file (`compose/monitoring.yml`). Aligning this repo's structure makes compose files directly usable as upstream sources during the migration (Phase 3).

**Scope:**

- Move prometheus and grafana service definitions from `docker/shard.yml` to `docker/monitoring.yml`
- Update documentation

---

---

### EPIC-005: Issue Migration

```yaml
---
epic_id: EPIC-005
title: "Issue Migration"
status: complete
completed_date: 2026-08-19
priority: p1
user_story: US-002
blocked_by: [EPIC-004]
created_at: 2026-04-09
claimed_by: claude-session-migrate
claimed_at: 2026-04-17T19:35:00Z
completed_at: 2026-04-17T19:35:00Z
tasks:
  - id: TASK-005-1
    title: "Migrate 22 Rust-relevant issues to f1r3node-rust"
    status: complete
    claimed_by: claude-session-migrate
    completed_at: 2026-04-17T19:35:00Z
    acceptance:
      - "22 Rust-relevant issues created on f1r3node-rust as #5-#26 with original context"
      - "Each new issue has migration header with source #, author, filed date, and link"
      - "Original labels (bug/enhancement/question) preserved where applicable"
      - "Original issues on f1r3node received redirect comments pointing to new issue numbers"
    notes:
      - "Spec called for 22 total (16 Rust-specific + 6 triage/design); actual open count was 22"
      - "#437 excluded from migration — already fixed on rust/staging by commit 89ac4a7a, closed with reference"
      - "Mapping table: /tmp/issue-migration/issue-map.tsv"

  - id: TASK-005-2
    title: "Close 5 Scala-only issues on f1r3node"
    status: complete
    claimed_by: claude-session-migrate
    completed_at: 2026-04-17T19:35:00Z
    acceptance:
      - "Issues #452, #366, #321, #221 closed with deprecation comment (reason: not planned)"
      - "Comment directs reporter to f1r3node-rust if bug still reproduces there"
      - "phase_3_issues.status set to 'complete' in /tmp/migrationPlan.md"
    notes:
      - "#184 from the original spec was already closed pre-migration (unrelated genesis refactor), so effective count is 4 Scala + 1 already-fixed (#437) = 5 closures"
---
```

**Context:** Transfer the 27 open issues from f1r3node to their appropriate destinations. 22 issues migrate to f1r3node-rust, 5 Scala-only issues are closed.

**Scope:**

- Included: Issue creation, cross-referencing, closing Scala issues
- Excluded: Fixing any of the migrated issues

---

---

### EPIC-R03: CI/CD Pipeline

```yaml
---
epic_id: EPIC-R03
title: "CI/CD Pipeline"
status: complete
priority: p1
completed_at: 2026-03-19
completed_by: human + claude
tasks:
  - id: TASK-R03-1
    title: "Add GitHub Actions workflow with lint and per-crate test matrix"
    status: complete
  - id: TASK-R03-2
    title: "Fix pre-push hooks to run tests per-crate (LMDB lock contention)"
    status: complete
---
```

**Summary:** Added GitHub Actions CI with cargo fmt, clippy linting, and per-crate test matrix. Fixed pre-push hooks to avoid LMDB lock contention by running crate tests sequentially.

**Key Changes:**
- `.github/workflows/` — lint + per-crate test jobs
- `hooks/pre-push` — sequential per-crate test execution

---

### EPIC-R02: Developer Tooling and Hooks

```yaml
---
epic_id: EPIC-R02
title: "Developer Tooling and Hooks"
status: complete
priority: p1
completed_at: 2026-03-19
completed_by: human + claude
tasks:
  - id: TASK-R02-1
    title: "Add pre-commit and pre-push git hooks (lint/test gates)"
    status: complete
  - id: TASK-R02-2
    title: "Fix hook executable permissions in git index"
    status: complete
  - id: TASK-R02-3
    title: "Add LMDB system dependency for hook test runs"
    status: complete
  - id: TASK-R02-4
    title: "Fix wallet test data corrupted by rustfmt format_strings"
    status: complete
  - id: TASK-R02-5
    title: "Fix doc comment fencing broken by wrap_comments"
    status: complete
  - id: TASK-R02-6
    title: "Expand local node and Docker setup instructions"
    status: complete
---
```

**Summary:** Established developer guardrails with pre-commit (fmt + clippy) and pre-push (test) hooks. Fixed several issues caused by aggressive rustfmt settings and missing system dependencies.

**Key Changes:**
- `hooks/pre-commit`, `hooks/pre-push` — git hook scripts
- `casper/` — restored test data and doc comments damaged by rustfmt
- `DEVELOPER.md` — expanded setup instructions for macOS, Ubuntu, Fedora

---

### EPIC-R01: Repository Extraction

```yaml
---
epic_id: EPIC-R01
title: "Repository Extraction"
status: complete
priority: p0
completed_at: 2026-03-19
completed_by: human
tasks:
  - id: TASK-R01-1
    title: "Extract pure Rust workspace from f1r3node rust/dev branch"
    status: complete
---
```

**Summary:** Extracted all 11 Rust crates from the `F1R3FLY-io/f1r3fly` repository's `rust/dev` branch into a standalone Cargo workspace. Removed Nix flake, SBT build, Scala source, `.envrc`, and JVM tooling. Added native dependency install instructions.

**Key Changes:**
- Standalone Cargo workspace with 11 crates
- Removed: Nix, SBT, Scala, JVM dependencies
- Added: Homebrew/apt install instructions, Justfile, Docker configs

---

## Completion Statistics

| Period | Epics Completed | Tasks Completed | Notes |
|--------|------------------|-----------------|-------|
| 2026-03 | 3 | 9 | Repo bootstrap: extraction, tooling, CI |
| 2026-08 | 4 | 12 | EPIC-001, EPIC-002, EPIC-005, EPIC-011 |
| 2026-10 | 1 | 4 | EPIC-020 node log and accept-path self-limits |

---

## References

- **Active Work:** `docs/ToDos.md`
- **User Stories:** `docs/UserStories.md`
- **Backlog:** `docs/Backlog.md`
