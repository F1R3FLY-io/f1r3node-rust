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

- **Casper ratification follow-up (2026-09-16).** EPIC-017 owns the pre-#216 models, baseline conformance, and harness preparation on `formal/soak-casper-consensus`. EPIC-018 owns a separate formal-methods harness PR after PR #216 merges. The [meeting record](https://github.com/F1R3FLY-io/f1r3node-rust/pull/390#pullrequestreview-5227717933) controls both phases. PRs #430 through #433 remain prerequisites in stack order. The [branch plan](./plans/casper-ratified-soak-2026-09-16.md) defines the handoff, separate completion gates, and deferred-policy restrictions.

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
    status: in_progress
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
    remaining_work:
      - "Review the completion evidence and obtain source-bound claim acceptance under TASK-019-4."
      - "Keep task ownership and completion status unchanged until the required review finishes."
    acceptance:
      - "Every limit is checked before the allocation or store operation it bounds."
      - "Capture rejects any environment or generation change between open and validation, including restored values."
      - "Production store bytes are unchanged by successful and rejected captures."
      - "Two scratch views share no mutable store with each other or with production."
      - "The canonical identity covers every field that scratch construction and evaluation consume."
  - id: TASK-019-3
    title: "Batch B2: observer handle, detached evaluation, and reference comparison"
    status: pending
    claimed_by: null
    blocked_by: [TASK-019-2, TASK-019-4]
    prerequisites:
      - "Do not start B2 planning until both current claims pass source-bound verification and a named maintainer explicitly accepts them."
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
    status: in_progress
    implementation_status: complete
    verification_status: bounded_models_and_source_bindings_passed
    remaining_gate: named_maintainer_acceptance
    claimed_by: pi-session-01a0afde-d35c-70a2-b8c9-39aa11cbdfca
    claimed_at: 2026-09-22T14:15:47Z
    blocked_by: []
    execution_revision: 4aa93d11cf7a4c318074975dd4266208575c8aca
    verification_working_tree: true
    verification_source_manifest: docs/cbc-evidence/runs/casper-node-claim-gate-4aa93d11c-03/sources.sha256
    correction_checkout_base: 4561e064a70b495fe07cbcf779bff375d636aaad
    correction_working_tree: true
    correction_source_manifest: docs/cbc-evidence/runs/casper-node-deadline-correction-4561e064a-01/sources.sha256
    work_log: docs/work-logs/task-019-4-node-claim-verification.md
    evidence:
      - docs/cbc-evidence/runs/casper-node-claim-gate-4aa93d11c-03/report.json
      - docs/cbc-evidence/runs/casper-node-claim-gate-4aa93d11c-02/report.json
      - docs/cbc-evidence/runs/casper-node-claim-gate-6ea6bf029-01/report.json
      - docs/cbc-evidence/runs/casper-node-deadline-correction-4561e064a-01/report.json
    claims: [CLAIM-CASPER-NODE-OBSERVATION-001, CLAIM-CASPER-NODE-OBSERVATION-002]
    eligible_maintainers: [spreston8, dylon, metaweta, jeffrey-l-turner, jltatbeach]
    proposed_reviewer: jltatbeach
    accepted_by: null
    acceptance_record: null
    notes:
      - "The source correction is committed. The new verification package uses the checkout base plus an uncommitted generation regression and formal evidence."
      - "Both bounded models pass. All 16 negative controls produce the exact expected counterexample."
      - "The verification tools now use Rust. Eight checker tests, strict Clippy, all model checks, and all property bindings pass."
      - "The isolated native rebuild passes 65 tests and strict Clippy. Disabling generation rejection makes its new regression fail."
      - "The binding check covers all 23 properties and verifies 1,130 isolated build inputs. This is not a machine-checked refinement."
      - "Both claim inventories include the models and verification scripts. The CI gate runs the models in both tiers."
      - "The strict status audit returns exit 4 for eight pending artifacts, including the shared CI gate."
      - "Seven existing evidence records and the CI registration record identify the new package. Historical evidence and separate claims remain intact."
      - "The acceptance request is prepared in the package README. Named maintainer acceptance and discharge remain pending."
      - "The user placed this gate before B2 planning. B2 is not a prerequisite for verification of Batch A and B1."
    acceptance:
      - "Strict source-bound audits pass for every artifact in both claim inventories at the accepted revision."
      - "Refutation, construction, and binding tiers are recorded with retained failing controls."
      - "An isolated rebuild runs the interface and capture suites. The Batch A and B1 reports record native runs only."
      - "Acceptance is recorded by a named maintainer. Passing tests alone do not discharge a claim."
    implementation_plan:
      - "Step 1. Author bounded TLA+ models under formal/tlaplus/node_observation/: MC_ObserverSession for challenge freshness, one request per session, deadline expiry, and session budget; MC_BoundedCapture for guard order, transaction-identity interval, environment-change rejection, incomplete-row rejection, and guard release. Add negative controls beside each positive configuration."
      - "Step 2. Register both models in scripts/ci/check-tla-invariants.sh and add the model files to both claim inventories. Record the refutation tier from the negative controls and the construction tier from the positive checks."
      - "Step 3. Rerun the isolated rebuild at corrected revision de93425ee or later. Create a new casper-node-claim-gate-<revision>-02 package. Audit both explicit claim inventories. Bind every required property to named passing tests and matching isolated source inputs. A status audit alone cannot establish binding."
      - "Step 4. Request acceptance from the proposed reviewer as a PR #447 review comment that names the revision, both claim IDs, and the package path."
      - "Step 5. Record acceptance: fill accepted_by and acceptance_record, flip both claim files from pending, set verified_at on the seven records, and run the CbC discharge so the strict gate exits clean."
  - id: TASK-019-5
    title: "Batch C: publication writer inventory and consistency contract"
    phase_boundary: "Occurrence-dependent qualification follows this branch merge and PR #216 integration. It does not block this branch on PR #216."
    status: pending
    claimed_by: null
    blocked_by: [TASK-019-3]
    prerequisites:
      - "A complete inventory of publication writers across the DAG and block environments."
      - "A publication consistency contract that states which intermediate states a consistent read can expose."
      - "PR #216 merged to dev. The user directed publication qualification to wait on 2026-09-22. Publication and recovery require occurrence records."
    acceptance:
      - "Occurrence identity is never inferred from deploy signatures."
      - "The contract and inventory are reviewed before any Batch C implementation approval."
  - id: TASK-019-6
    title: "Review minimum necessary scope, merge PR #447 to dev, and return PR #436 to dev"
    status: pending
    claimed_by: null
    blocked_by: [TASK-019-4]
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
    status: pending
    claimed_by: null
    blocked_by: []
    external_dependencies:
      - "system-integration: the lifecycle-test fix must be committed, pushed, and merged before it can be pinned."
    consumer: "EPIC-017 TASK-017-12 candidate repin on formal/soak-casper-consensus."
    pin_site: .github/oci-validation.env
    implementation_plan:
      - "Step 1. In system-integration, commit and push the lifecycle-test fix with its own consent, and record the merged revision."
      - "Step 2. Open a small PR to dev that updates SYSTEM_INTEGRATION_REF in .github/oci-validation.env to that revision. The soak branch keeps its own three pin sites aligned separately."
      - "Step 3. Let dev CI publish the image for the current dev revision, and record the immutable image digests for amd64 and arm64 with the dev revision they were built from."
      - "Step 4. Hand the digests to TASK-017-12 for candidate repin and admission. This baseline image carries no observer interface."
      - "Step 5. After TASK-019-6 merges PR #447, repeat Step 3 for the observer-capable dev revision. Authority and publication adapter qualification needs that second image, not the baseline."
    acceptance:
      - "The pin names a merged system-integration revision as a 40-character SHA."
      - "Each published image is recorded by immutable digest, platform, and source dev revision."
      - "The baseline and observer-capable images are recorded as distinct candidates."
      - "No candidate is repinned from a rebuilt or mutable tag."
---
```

**Current state:** Batch A and the B1 corrections are implemented. TASK-019-4 now has passing bounded models, negative controls, source bindings, and 65 isolated test executions. Both claims await named maintainer acceptance of the new package.

Batch B2 and Batch C remain unapproved. Candidate image publication waits on the system-integration pin under TASK-019-7. PR #447 targets `dev`.

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
    integration_fix_status: "The validator lifecycle timeout fix is applied locally in system-integration. All 39 targeted tests pass. Publication, suite pin updates, and hosted verification remain pending."
    candidate_identity_evidence: docs/casper/cbc-evidence/runs/casper-candidate-repin-20260919-01/report.json
    live_admission_evidence: docs/casper/cbc-evidence/runs/casper-linux-admission-7509c831c-01/report.json
    manual_dispatch_evidence: docs/casper/cbc-evidence/runs/casper-campaign-dispatch-5e26ba4c5-01/report.json
    completion_review: docs/work-logs/task-017-12-preparation.md#completion-review-at-859cbc36c
    claim001_reconciliation: docs/work-logs/task-017-12-preparation.md#claim001-specification-digest-reconciliation
    node_interface_prerequisite: docs/plans/casper-node-interface-prerequisite.md
    node_interface_status: "PR #447 remains open at 4561e064a. TASK-019-4 reports missing formal evidence and a new B1 lock-wait finding. Batch B2 planning waits for source-bound verification and named maintainer acceptance. Occurrence-dependent publication qualification belongs to EPIC-018 after this branch merges."
    stack_scope: "PR #436 temporarily targets the node branch. This dependency order does not include node implementation in the harness scope. Independent harness controls can proceed before node qualification."
    reservation_work_log: docs/work-logs/task-017-12-reservations.md
    reservation_status: "All sixteen reservation tests pass in an isolated Linux container on this Mac. The authoritative OCI protocol has controlled-provider tests. Its real object and access policy remain unprovisioned."
    execution_control_plan: docs/plans/casper-campaign-execution-controls.md
    execution_control_evidence: docs/casper/cbc-evidence/runs/casper-campaign-control-20260921-01/report.json
    hosted_control_evidence: docs/casper/cbc-evidence/runs/casper-campaign-hosted-58e952c6f-01/report.json
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
    compatibility_lookup_status: "The three campaign inventories have 37 unique pending artifact records and matching compatibility links. The strict eight-claim audit returns exit 4 with Claim001 pending. No acceptance is inferred."
    execution_gate: "Four exact-candidate probes returned blocked before node launch. Their controlled inputs are not live qualification. The legacy workload is pinned but cannot replace required Casper profiles."
    drift_review: docs/work-logs/task-017-12-drift-review-2026-09-19.md
    claims: [CLAIM-CASPER-SOAK-001, CLAIM-CASPER-SOAK-004, CLAIM-CASPER-CAMPAIGN-001, CLAIM-CASPER-CAMPAIGN-002, CLAIM-CASPER-CAMPAIGN-003]
    claim_index: docs/claims/casper-soak-harness.md
    campaign_claim_index: docs/claims/casper-soak-campaign.md
    reservation_claim_index: docs/claims/casper-campaign-reservation.md
    status: in_progress
    claimed_by: pi-soak-carrier-index-linux
    claimed_at: 2026-09-19T19:20:00Z
    previous_claimed_by: claude-session-9f19b46c
    previous_claimed_at: 2026-09-19T06:35:51Z
    handoff_note: docs/handoffs/claude-session-9f19b46c--pi-soak-carrier-index-linux--20260919T192000Z.md
    execution_scope: "Qualify the pre-merge candidate capabilities under the corrected campaign phase boundary. This branch merges before PR #216 integrates. Occurrence-dependent publication and recovery qualification belong to EPIC-018. A passing preflight must precede both full baselines."
    repin_tool: scripts/ci/resolve-dev-candidate.sh
    dispatch_preconditions: "docs/work-logs/task-017-12-preparation.md#dispatch-preconditions"
    work_log: docs/work-logs/task-017-12-preparation.md
    blocked_by: [TASK-019-3, TASK-019-4]
    remaining_prerequisites:
      - "The changed workflow and campaign artifacts have current pending records. Historical evidence remains unchanged. Claim001 and the three campaign claims still require source-bound acceptance."
      - "Pin executable workloads and accept the refreshed campaign model inventory. All 225 model hashes and four configuration hashes match. Recheck candidate identities before dispatch."
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
    claimed_by: pi-session-01a0afde-d35c-70a2-b8c9-39aa11cbdfca
    claimed_at: 2026-09-21T14:31:52Z
    execution_scope: "Deliver the independent formal gate on dev and verify the approved protection change. Review TASK-017-12 evidence before final handoff acceptance."
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
      - "Review baseline results, scan benchmark evidence, and concurrency-gate evidence. Record the required scope of the approved 60-hour phase."
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
    status: in_progress
    claimed_by: claude-session-9f19b46c
    claimed_at: 2026-09-19T05:52:15Z
    execution_scope: "Preparation steps 0 through 5 only: retention rule, external store, bundles, consumer review, and work-log consolidation. Steps 6 and 7, the verification and the reduction commit, wait for TASK-017-13."
    work_log: docs/work-logs/task-017-14-preparation.md
    blocked_by: [TASK-017-13]
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
---
```

**Current state:** TASK-017-1 through TASK-017-4 are complete. The repaired lifecycle binding is accepted. Three controlled-transcript profiles await acceptance. Four profiles remain unimplemented. Baseline soaks remain pending.

**Approved sequence:** TASK-017-4 and TASK-017-5 through TASK-017-11 may proceed together against the completed contract and applied prerequisites. TASK-017-12 requires final workload pins, qualification, verification, and dispatch approval.

**Verified stack:** Merge `0f1ccdf38` includes PR #433 at `65f7f6daa`. PR #436 targets `docs/consensus-neutral-execution`. CLAIM-CASPER-SOAK-001 passes strict discharge. The seven profile claims remain pending.

**Tracker compatibility:** The shared CLI still rejects TASK-* identifiers. The completion review invokes its unchanged task function for the three reviewed preparation tasks. The TASK-017-4 adapter remains unchanged.

The [interface contract](./casper/design/soak-interface-contract.md) records exact payloads, source boundaries, fixture expectations, and missing capabilities. The local model alone cannot discharge a harness claim. The accepted lifecycle records include executable bindings and source-specific review.

**Scope:** This epic covers the pre-#216 PR only. The [branch plan](./plans/casper-ratified-soak-2026-09-16.md) records both phases and their evidence boundary.

**Final task:** TASK-017-15 closes the branch and PR #436. Evidence consumers address the draft release by its ID until then. Release publication is the last action, and it follows the reduction commit.

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
