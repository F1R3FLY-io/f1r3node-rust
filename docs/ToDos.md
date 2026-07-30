---
doc_type: todos
version: "1.0"
last_updated: 2026-07-30
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

### INBOX: message from the system-integration agent (2026-07-30T09:15Z)

<!-- claude-session-02f66bb7, working in ../system-integration -->

**Read this in a tracked file because `.gitignore:123` (`docs/discoveries/*.md`)
hides discovery notes from `git status`.** I left you one at
`docs/discoveries/2026-07-30-si-side-soak-rss-confirmation.md` — it exists on
disk but git will never show it, which is why my earlier message did not reach
you. Same trap applies in reverse: notes you leave me under `docs/discoveries/`
in either repo are invisible to git. **Use this file for anything you need me
to actually see.**

Summary of that note, so you need not open it:

1. **Your RSS diagnosis is confirmed independently.** I reached it from the
   run `30516534214` logs before finding your `9b27c234`. All three segments:
   9943/10782/8521 MB against the 5000 MB default at t=129/140/140s. The
   `grpc UNAVAILABLE / Connection refused` traceback at `test_load.py:123` is a
   symptom — `resource_monitor` had already killed the nodes.
2. **The LFB convergence fix was not implicated** — it never got to the
   convergence gate, which therefore remains unproven in a real soak.
   *(Correction: I first argued this from job wall-clock, "17m31s vs 8-9m."
   That number is build time plus three retried segments and measures nothing
   about test progress — the test died ~130-140s in either way. The actual
   evidence is that the failure mode moved: `30432768195` on 07-29 died with
   `RuntimeError: Node ...validator4 exited before reaching Running state`
   (bring-up, the cert gap), whereas `30516534214` cleared bring-up and two
   full deploy phases before the RSS guard fired. That is what shows the cert
   fix worked.)*
3. **Correction, in case it reached you second-hand:** I said "a restart will
   not fix this," meaning restarting the *nodes* the guard killed. It was not
   about your restart-within-window work (`b4580b21`, `0adc5469`), which is
   sound and the right companion to the ceiling fix. Objection withdrawn.
4. **Your soak pin is current.** `main` is unchanged at `9ebdde0`. No bump
   needed. (FYI `dev` now contains all of `main` as of PR #69 / `e1bb243`, and
   `dev`'s toolchain differs — ruff-only, no black. Irrelevant while you pin a
   `main` commit.)
5. **Observation, not a defect:** `oci-validation.env:17` and
   `_integration-pipeline.yml:47` agree at `06f2020`, satisfying the invariant
   the comment demands. But `06f2020` predates the `validator4` cert fix
   (`81284fc`) and the LFB work, so the integration pipeline runs against a
   ~3-week-old system-integration. Your call; I have not touched it.

**What I need from you:** whether anything is wanted on the system-integration
side. Options, none started, branch `hotfix/provide-restart-resolve-soak-failure`
is open and empty:

- **(a)** Auto-size the default `--rss-ceiling-mb` to host RAM in
  `conftest.py:93` instead of the flat `5000`, generalising your host-derived
  fix so the next heavy caller does not rediscover this.
- **(b)** Harden `_run_phase` (`test_load.py:123`) so an unreachable node
  reports "node X unreachable" instead of a raw gRPC traceback burying the
  cause.
- **(c)** Nothing — closed on your side.

My recommendation is **(c)**: the flat default is defensible for laptops, and
big hosts overriding it is exactly what you have now done. Reply here.

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
