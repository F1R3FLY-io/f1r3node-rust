---
doc_type: todos
version: "1.1"
last_updated: 2026-08-01
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

<!-- Compact, current-state-only. This section replaces the free-form status
     entries that previously accumulated at the top of this file; the full
     narrative history is preserved verbatim in
     docs/work-logs/coordination-archive-2026-08-01T03Z.md.
     NOTE: docs/discoveries/*.md is gitignored here (.gitignore:123) — use
     docs/work-logs/ for durable cross-agent notes, and keep this section to
     current operative facts only. -->

- **Randomized exercise soak collaboration is active.** This repository owns value-stream documentation, epoch contracts, seeded scheduling, orchestration, aggregation, and replay on `feature/randomized-exercise-soak`. The parallel `system-integration` branch `feature/randomized-exercise-soak-catalogue` owns the executable catalog, valid workload generators, invariants, and shard reset. Handoff files are ephemeral coordination aids; durable cross-repository state lives in the canonical executor contract at `../system-integration/docs/specs/randomized-exercise-soak-contract.md` and in EPIC-011 through EPIC-016 below.
- **PR #182** (`hotfix/renormalize-system-integration-pin-post-79` → `dev`, head `121029f1`) normalizes all three `SYSTEM_INTEGRATION_REF` sites to system-integration `main` `369d49df2f97e65b3d0ad869aa668a7383b11179` (the post-#79/#80 promotion). Multi-agent review posted 2026-08-01: approved 3-0 (anthropic abstained on an API billing error). This completes and supersedes the 2026-07-31T19:33 PDT handoff; the similarly named local branch `hotfix/normalize-system-integration-pin-post-79` is stale and has no PR.
- **Hold `dev` → `master` until the weekend soak snapshot is verified.** The Friday 19:30 Pacific scheduled `Merge Recovery Soak` run must exist with its `headSha` recorded, confirming it launched from the pre-normalization `master` (PR #181 pin `79262d8b`), before promoting. Merging first would silently move the weekend soak to the post-#79 `369d49df` pin. If no scheduled run appears, hold the promotion and investigate or manually dispatch from the intended pre-normalization `master`. Known discrepancy: scheduled runs initialize `target_ref=dev` although comments say the Friday weekend run targets `master` — treat the captured workflow `headSha`/pin and the resolved target SHA as separate evidence.
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

<!-- Epics ordered by priority. EPIC-001/002 are system-integration alignment (US-001). EPIC-003-008 are migration (US-002). -->

---

### EPIC-001: System-Integration Alignment

```yaml
---
epic_id: EPIC-001
title: "System-Integration Alignment"
status: in_progress
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

### EPIC-002: Separate Monitoring from Shard Compose

```yaml
---
epic_id: EPIC-002
title: "Separate Monitoring from Shard Compose"
status: pending
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

### EPIC-005: Issue Migration

```yaml
---
epic_id: EPIC-005
title: "Issue Migration"
status: complete
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

### EPIC-011: Exercise Epoch Contract & Catalog Governance

```yaml
---
epic_id: EPIC-011
title: "Exercise Epoch Contract & Catalog Governance"
status: pending
priority: p1
user_story: US-005
blocked_by: []
created_at: 2026-08-01
claimed_by: null
claimed_at: null
tasks:
  - id: TASK-011-1
    title: "Ratify the cross-repository exercise epoch identity and schema"
    status: pending
    acceptance:
      - "Catalog schema defines permanent SOAK-EPOCH-NNN IDs, semantic epoch_revision, catalog_schema_version, definition SHA, normalized definition digest, seed, topology, provider, effective limits, policy, and provenance"
      - "Versioning rules distinguish semantic workload changes from editorial Git changes and make the exact historical implementation replayable"
      - "Unknown schema versions, revisions, or digest mismatches fail closed"

  - id: TASK-011-2
    title: "Create mirrored contract fixtures and compatibility tests"
    status: pending
    blocked_by: [TASK-011-1]
    acceptance:
      - "Both repositories validate the same representative catalog entry, execution request, execution result, and replay manifest fixtures"
      - "Contract tests detect missing required fields, incompatible enum values, revision drift, digest drift, and unsupported providers or topologies"
      - "Fixture evolution documents backward-compatible and breaking schema changes"

  - id: TASK-011-3
    title: "Validate the pinned catalog before OCI runner launch"
    status: pending
    blocked_by: [TASK-011-2]
    acceptance:
      - "The soak schedule gate validates that systemIntegration.catalogRef from .github/ci-pins.jsonc exposes a compatible executor and catalog before provisioning OCI resources"
      - "Required epoch IDs and revisions resolve at the pinned catalog SHA"
      - "A compatibility failure is visible as a preflight failure and cannot silently fall back to test_load.py"

  - id: TASK-011-4
    title: "Define epoch authoring, revision, and promotion governance"
    status: pending
    blocked_by: [TASK-011-1]
    acceptance:
      - "Authoring guidance requires valid-operation preconditions, bounded limits, finalized-state invariants, reset requirements, provider support, and provenance"
      - "A revision decision table covers new epoch versus new revision versus editorial-only change"
      - "Experimental-to-gating promotion requires stable evidence and maintainer approval"
---
```

**Context:** Establishes the durable contract between the soak orchestrator and the executable workload catalog. The canonical value stream and versioning model are documented in `docs/randomized-exercise-soak.md`.

**Scope:**

- Included: identity, schema, compatibility fixtures, pin preflight, and governance
- Excluded: executable workload generators (EPIC-012)
- Excluded: randomized scheduling (EPIC-013)

---

### EPIC-012: Valid Single-Shard Exercise Epoch Library

```yaml
---
epic_id: EPIC-012
title: "Valid Single-Shard Exercise Epoch Library"
status: pending
priority: p1
user_story: US-005
blocked_by: [EPIC-011]
created_at: 2026-08-01
claimed_by: null
claimed_at: null
tasks:
  - id: TASK-012-1
    title: "Implement the system-integration epoch executor and result protocol"
    status: pending
    external: true
    external_repo: F1R3FLY-io/system-integration
    coordination_note: "Owned on feature/randomized-exercise-soak-catalogue; canonical contract is docs/specs/randomized-exercise-soak-contract.md in that repository"
    acceptance:
      - "Executor accepts epoch ID, revision, seed, provider, topology, deadline, output directory, and orchestrator-supplied safety limits"
      - "Executor emits a structured result and replay manifest conforming to EPIC-011 fixtures"
      - "Result classification distinguishes workload failure, safety breach, host breach, reset failure, deadline exhaustion, and infrastructure loss"

  - id: TASK-012-2
    title: "Implement steady-stream and burst-cooldown epochs"
    status: pending
    blocked_by: [TASK-012-1]
    external: true
    external_repo: F1R3FLY-io/system-integration
    acceptance:
      - "SOAK-EPOCH-001 sustains a bounded valid deploy rate and verifies finalization drain"
      - "SOAK-EPOCH-002 applies deterministic valid bursts, cooldowns, and convergence checks"
      - "Both epochs record submitted, accepted, rejected, included, and finalized counts"

  - id: TASK-012-3
    title: "Implement contention and large-valid-deploy epochs"
    status: pending
    blocked_by: [TASK-012-1]
    external: true
    external_repo: F1R3FLY-io/system-integration
    acceptance:
      - "SOAK-EPOCH-003 exercises concurrent valid contracts over shared channels or state without invalid races in the generator"
      - "SOAK-EPOCH-004 exercises deploys near approved payload and phlo bounds without crossing operational limits"
      - "Seeds deterministically reproduce generated contracts, ordering, rates, and concurrency"

  - id: TASK-012-4
    title: "Implement dependent-chain and mixed-contract epochs"
    status: pending
    blocked_by: [TASK-012-1]
    external: true
    external_repo: F1R3FLY-io/system-integration
    acceptance:
      - "SOAK-EPOCH-005 waits for finalized prerequisite state before every dependent operation"
      - "SOAK-EPOCH-006 deterministically interleaves configured valid contract families"
      - "Failure evidence identifies the first operation whose finalized invariant diverged"

  - id: TASK-012-5
    title: "Prove operational validity, provider parity, and clean reset"
    status: pending
    blocked_by: [TASK-012-2, TASK-012-3, TASK-012-4]
    external: true
    external_repo: F1R3FLY-io/system-integration
    acceptance:
      - "Generators enforce valid signatures, balances, phlo limits, dependencies, shard routing, and finalized prerequisites"
      - "Every epoch passes deterministic contract tests under Docker and subprocess providers unless explicitly provider-specific with ratified rationale"
      - "Shard reset proves clean state before another epoch; inability to prove reset is a distinct fatal result"
      - "Epochs inherit and never increase orchestrator-supplied RSS and host-free safety limits"
---
```

**Context:** The executable catalog belongs in system-integration because that repository owns shard lifecycle, providers, deploy submission, and finalized-state assertions. This repository mirrors the external work so the pin and orchestration dependencies remain visible.

**Scope:**

- Included: first six transaction-only valid-operation epochs, executor protocol, provider parity, reset
- Excluded: validator lifecycle and fault injection
- Excluded: multi-shard workloads (EPIC-015)

---

### EPIC-013: Seeded Weekend Epoch Scheduling & Segment Integration

```yaml
---
epic_id: EPIC-013
title: "Seeded Weekend Epoch Scheduling & Segment Integration"
status: pending
priority: p1
user_story: US-006
blocked_by: [EPIC-011, EPIC-012, EPIC-016]
created_at: 2026-08-01
claimed_by: null
claimed_at: null
tasks:
  - id: TASK-013-1
    title: "Build a deterministic coverage-constrained epoch planner"
    status: pending
    acceptance:
      - "Planner filters catalog entries by schema, policy, topology, provider, and effective window"
      - "Every required compatible epoch receives one reserved slot before weighted random fill"
      - "The same catalog, seed, constraints, and duration estimates produce the same plan"
      - "Immediate repetition is avoided when another eligible epoch fits"

  - id: TASK-013-2
    title: "Derive and persist run and per-epoch seeds"
    status: pending
    blocked_by: [TASK-013-1]
    acceptance:
      - "One run seed deterministically derives each selection and workload seed without relying on ambient process randomness"
      - "Manual dispatch may supply a seed; scheduled runs derive one from immutable run identity"
      - "Artifacts record the run seed and every derived seed needed for replay"

  - id: TASK-013-3
    title: "Integrate epoch admission with soak segment deadlines"
    status: pending
    blocked_by: [TASK-013-1]
    acceptance:
      - "An epoch starts only when its declared upper duration plus reset and checkpoint reserve fits before the segment deadline"
      - "Approaching a checkpoint stops admission cleanly and preserves the remaining plan"
      - "Planned, executed, skipped, and deferred epochs are recorded with reasons"

  - id: TASK-013-4
    title: "Invoke the pinned system-integration executor from weekend segments"
    status: pending
    blocked_by: [TASK-013-2, TASK-013-3]
    acceptance:
      - "Weekend segments pass identity, revision, digest, seed, provider, topology, deadline, output path, image, binary, and safety limits through the ratified interface"
      - "Docker and subprocess coverage remains balanced across the complete run"
      - "Daily soaks retain their existing behavior until separately enabled"

  - id: TASK-013-5
    title: "Test deterministic planning, coverage, and deadline behavior"
    status: pending
    blocked_by: [TASK-013-4]
    acceptance:
      - "Tests cover catalog order changes, incompatible entries, undersized windows, exact-fit boundaries, weighted fill, provider constraints, and segment resume"
      - "Tests prove required coverage is never silently omitted"
      - "A replayed plan does not depend on filesystem traversal order or shell associative-array order"
---
```

**Context:** Replaces repeated fixed-load iterations during weekend segments with a precomputed, replayable workload sequence while preserving checkpoint boundaries and host protection.

**Scope:**

- Included: weekend scheduler, deterministic seeds, coverage guarantees, segment admission, executor invocation
- Excluded: daily randomized epochs until weekend evidence is stable
- Excluded: weakening or tuning existing host-protection limits

---

### EPIC-014: Epoch Evidence, Replay & Regression Promotion

```yaml
---
epic_id: EPIC-014
title: "Epoch Evidence, Replay & Regression Promotion"
status: pending
priority: p2
user_story: US-007
blocked_by: [EPIC-013]
created_at: 2026-08-01
claimed_by: null
claimed_at: null
tasks:
  - id: TASK-014-1
    title: "Aggregate per-epoch execution and catalog coverage results"
    status: pending
    acceptance:
      - "Run summary records planned and actual sequence, required coverage, epoch ID/revision/digest, policy, provider, seeds, timing, and outcome"
      - "Per-epoch metrics include transaction lifecycle counts, finalization, throughput, convergence, RSS, and CPU where available"
      - "Incomplete coverage is explicit and cannot appear as full coverage"

  - id: TASK-014-2
    title: "Preserve classified failure evidence and reset outcomes"
    status: pending
    blocked_by: [TASK-014-1]
    acceptance:
      - "Ordinary workload failures preserve evidence, verify reset, and allow later epochs to continue"
      - "Safety, host-protection, and reset failures preserve evidence and stop immediately"
      - "Infrastructure loss remains classified separately and uses existing in-window recovery"
      - "API and artifact output includes checksums and the first failing operation without exposing keys or secrets"

  - id: TASK-014-3
    title: "Provide deterministic replay from an execution manifest"
    status: pending
    blocked_by: [TASK-014-2]
    acceptance:
      - "Replay resolves the recorded system-integration definition SHA, revision, digest, provider, topology, seeds, and limits"
      - "Replay fails closed when any immutable dependency is unavailable or incompatible"
      - "Replay can target a locally supplied node image or subprocess binary while preserving the workload identity"

  - id: TASK-014-4
    title: "Implement regression intake and experimental-to-gating promotion"
    status: pending
    blocked_by: [TASK-014-3]
    acceptance:
      - "A new epoch or revision links its originating run or issue and demonstrates failure before the fix and success afterward"
      - "Experimental failures are visible but excluded from the release-gating verdict until promotion"
      - "Promotion requires stable provider evidence and explicit maintainer approval recorded in Git"

  - id: TASK-014-5
    title: "Add dashboard epoch coverage and failure trends after schema stabilization"
    status: pending
    blocked_by: [TASK-014-1, TASK-014-4]
    acceptance:
      - "Dashboard distinguishes required coverage, experimental outcomes, and gating outcomes"
      - "History groups results by permanent epoch ID and semantic revision without conflating incompatible revisions"
      - "Artifact and replay links remain available from each failed epoch result"
---
```

**Context:** Turns a randomized failure into reproducible engineering evidence and governs when an experimental workload becomes part of the release contract.

**Scope:**

- Included: aggregation, classification, evidence, replay, intake, promotion, later dashboard views
- Excluded: infrastructure postmortem tooling already tracked under EPIC-010

---

### EPIC-015: Multi-Shard Randomized Exercise Expansion

```yaml
---
epic_id: EPIC-015
title: "Multi-Shard Randomized Exercise Expansion"
status: pending
priority: p3
user_story: US-008
blocked_by: [EPIC-012, EPIC-013, EPIC-014]
created_at: 2026-08-01
claimed_by: null
claimed_at: null
tasks:
  - id: TASK-015-1
    title: "Extend the epoch contract with multi-shard topology capabilities"
    status: pending
    acceptance:
      - "Catalog entries declare shard count, node roles, routing capabilities, and cross-shard prerequisites"
      - "Compatibility checks reject a workload when the requested topology cannot satisfy its capabilities"
      - "Replay records enough topology identity to reconstruct the exercised shard relationships"

  - id: TASK-015-2
    title: "Implement valid inter-shard workload profiles and invariants"
    status: pending
    blocked_by: [TASK-015-1]
    external: true
    external_repo: F1R3FLY-io/system-integration
    acceptance:
      - "Profiles submit only valid operations and wait for finalized cross-shard prerequisites"
      - "Invariants cover routing, state convergence, dependency completion, and absence of duplicate effects"
      - "Generated routing and operation order are deterministic from recorded seeds"

  - id: TASK-015-3
    title: "Integrate multi-shard profiles with seeded planning and replay"
    status: pending
    blocked_by: [TASK-015-2]
    acceptance:
      - "Planner preserves required coverage among workloads compatible with the active topology"
      - "Execution and replay propagate topology, routing, provider, identity, revision, digest, and seeds"
      - "Multi-shard epochs remain experimental until single-shard reset and replay contracts have stable soak evidence"
---
```

**Context:** Applies the proven single-shard model to shard interoperability without making multi-shard complexity a prerequisite for the first randomized weekend suite.

**Scope:**

- Included: topology capabilities, valid inter-shard workloads, deterministic routing, replay
- Excluded: redesign of inter-shard consensus itself

---

### EPIC-016: Trusted Single-Source CI Pin Registry

```yaml
---
epic_id: EPIC-016
title: "Trusted Single-Source CI Pin Registry"
status: pending
priority: p1
user_story: US-009
blocked_by: []
created_at: 2026-08-01
claimed_by: null
claimed_at: null
tasks:
  - id: TASK-016-1
    title: "Define ci-pins.jsonc schema and offline resolver"
    status: pending
    acceptance:
      - ".github/ci-pins.jsonc is the only source for systemIntegration.runnerRef, systemIntegration.catalogRef, catalogSchemaVersion, and OCI CLI URL/version/checksums"
      - "A committed schema and offline parser support JSONC comments and trailing commas while rejecting duplicate keys, missing keys, unknown security-sensitive keys, mutable refs, malformed SHAs, malformed checksums, and OCI URL/version mismatch"
      - "Resolver runs on ubuntu-latest, emits validated non-secret job outputs, and downloads no parser package at runtime"
      - "Only lowercase 40-character system-integration SHAs are accepted; tags and branches are rejected"

  - id: TASK-016-2
    title: "Split privileged runner and exercise catalog trust domains"
    status: pending
    blocked_by: [TASK-016-1]
    acceptance:
      - "Secret-bearing launcher and cloud-init checkouts consume runnerRef"
      - "Integration harness, compatibility probe, and exercise catalog checkouts consume catalogRef"
      - "A job requiring both roles uses explicit outputs and cannot silently substitute one ref for the other"
      - "Routine compatible epoch additions require only a catalogRef bump"

  - id: TASK-016-3
    title: "Apply trigger-specific trusted pin selection"
    status: pending
    blocked_by: [TASK-016-1, TASK-016-2]
    acceptance:
      - "Protected pushes, schedules, and trusted manual dispatches resolve pins from their workflow/control commit"
      - "Fork pull_request_target privileged jobs always use base-branch runner and OCI pins; fork-controlled candidate configuration cannot influence privileged execution"
      - "Same-repository PRs validate candidate catalog pins under existing branch and environment controls"
      - "Full OCI validation may use candidate pins only after exact PR-head, maintainer permission, approval-comment, and environment gates succeed"

  - id: TASK-016-4
    title: "Migrate all system-integration and OCI CLI consumers"
    status: pending
    blocked_by: [TASK-016-2, TASK-016-3]
    acceptance:
      - "Reusable integration, ordinary CI, fork CI, full OCI validation, merge-recovery soak, soak signal, runner reaper, and relevant scripts consume resolver outputs"
      - ".github/oci-validation.env and workflow-level duplicate pin literals are removed without fallback copies"
      - "Every OCI installer consumer verifies the installer script and downloaded install.py checksums before execution"
      - "Resolver failure prevents OCI runner, instance, or shard launch"

  - id: TASK-016-5
    title: "Enforce single-source and trust-boundary invariants"
    status: pending
    blocked_by: [TASK-016-4]
    acceptance:
      - "Workflow invariants reject inline duplicate pins, mutable refs, missing resolver dependencies, unchecked OCI installers, and fork-controlled privileged pin selection"
      - "Mutation tests cover malformed JSONC, duplicate/missing/unknown keys, invalid SHAs/checksums, URL/version mismatch, and each trigger trust path"
      - "Diagnostics name the violated property and fail before resource provisioning"
      - "Ordinary CI, fork CI, full OCI validation, soak, signal, and reaper dry-run paths are validated"

  - id: TASK-016-6
    title: "Design reviewed catalog pin-bump automation"
    status: pending
    blocked_by: [TASK-016-5]
    acceptance:
      - "Initial rollout keeps pin bumps manual while resolver and catalog compatibility gain real-run evidence"
      - "Future system-integration automation may open a PR changing only catalogRef and attaching old..new plus epoch/revision evidence"
      - "Automation cannot merge, change runnerRef, or promote required/gating policy"
      - "Required and release-gating epoch policy remains an explicit separately reviewed control in this repository"
---
```

**Context:** Replaces duplicated trusted dependency values with `.github/ci-pins.jsonc` while preserving fork isolation and the immutable supply-chain boundary. The runner/catalog split lets new compatible exercise epochs arrive through one catalog pin edit without implicitly replacing privileged launcher code. The canonical design is `docs/ci-pins.md`.

**Scope:**

- Included: system-integration runner/catalog SHAs, accepted catalog schema, OCI CLI URL/version/checksums, resolver, trust matrix, migration, invariants, future PR-opening automation
- Excluded: OCI compartment OCIDs and other blast-radius literals
- Excluded: automatic merge or automatic experimental-to-gating promotion

---

## Epic Dependency Graph

```
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

EPIC-011 (epoch contract/catalog)       EPIC-016 (single-source CI pins)
    |                  |                       |
    v                  v                       v
EPIC-012 (workloads)   EPIC-013 (scheduler; blocked by 011/012/016)
    |                  |
    +--------+---------+
             v
EPIC-014 (evidence/replay/promotion)
    |        |
    +---+----+
        v
EPIC-015 (multi-shard expansion; also blocked by 012/013)
```

---

## Task States

| Status | Meaning | Next Action |
| -------- | --------- | ------------- |
| `pending` | Not started | Available to claim |
| `in_progress` | Being worked on | Continue or handoff |
| `blocked` | Waiting on dependency | Check `blocked_by` |
| `review` | Ready for review | Review and approve |
| `complete` | Done | Move to CompletedTasks.md |

---

## Workflow

1. **Find next task**: Use `/nextTask` to identify the highest priority unclaimed task
2. **Claim task**: Set `claimed_by` and `status: in_progress`
3. **Implement**: Use `/implement` to execute with full context
4. **Complete**: Mark `status: complete` when acceptance criteria met
5. **Signal**: Update completion signals in `/tmp/migrationPlan.md`
6. **Move epic**: When all tasks complete, move epic to `docs/CompletedTasks.md`

---

## References

- **Shared Migration Plan:** `/tmp/migrationPlan.md`
- **User Stories:** `docs/UserStories.md`
- **Completed Work:** `docs/CompletedTasks.md`
- **Backlog:** `docs/Backlog.md`
- **System-Integration Migration Plan:** `../system-integration/docs/migration-to-rust-node.md`
- **Randomized Exercise Soak Value Stream:** `docs/randomized-exercise-soak.md`
- **Canonical Trusted CI Pin Registry:** `docs/ci-pins.md`
- **Canonical Executor Contract:** `../system-integration/docs/specs/randomized-exercise-soak-contract.md`
