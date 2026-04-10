---
doc_type: todos
version: "1.0"
last_updated: 2026-04-09
mr_status:
  ready: false
  target_branch: master
---

# Tasks and Epochs

This document tracks implementation work through **epochs** (logical groupings of related tasks).

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

## Active Epochs

<!-- Epochs are ordered by phase. Dependency chain: EPOCH-001 -> EPOCH-002 -> [EPOCH-003, EPOCH-004, EPOCH-005] -> EPOCH-006 -->

---

### EPOCH-001: Merge Critical PRs into f1r3node

```yaml
---
epoch_id: EPOCH-001
title: "Merge Critical PRs into f1r3node"
status: pending
priority: p0
user_story: US-001
blocked_by: []
created_at: 2026-04-09
claimed_by: null
claimed_at: null
external: true
external_repo: F1R3FLY-io/f1r3node
coordination_note: "This epoch is executed by the agent in f1r3node. Track progress via /tmp/migrationPlan.md phase_1_critical_prs status."
tasks:
  - id: TASK-001-1
    title: "Verify new_parser branch status"
    status: pending
    acceptance:
      - "new_parser branch is merged into rust/dev OR confirmed as base for Reified RSpaces chain"
      - "rholang-rs#83 dependency is resolved"

  - id: TASK-001-2
    title: "Merge Reified RSpaces chain (#328-#338)"
    status: pending
    blocked_by: [TASK-001-1]
    acceptance:
      - "All 11 PRs (#328 through #338) merged sequentially into rust/dev"
      - "CI passes after each merge"

  - id: TASK-001-3
    title: "Merge Tier 2 PRs if ready"
    status: pending
    acceptance:
      - "#466 (Embers) reviewed — merged or deferred"
      - "#186 (eval cost) reviewed — merged or deferred"
      - "#281 (LMDB fixes) reviewed — merged or deferred"

  - id: TASK-001-4
    title: "Tag final f1r3node release"
    status: pending
    blocked_by: [TASK-001-2, TASK-001-3]
    acceptance:
      - "Tag rust-v0.4.12 (or appropriate version) created on f1r3node rust/dev"
      - "phase_1_critical_prs.status set to 'complete' in /tmp/migrationPlan.md"
      - "phase_1_critical_prs.final_tag populated"
---
```

**Context:** The Reified RSpaces chain (#328-#338) is a major architectural change that must land before code sync. This phase is owned by the agent working in the f1r3node repository. Completion is signaled via the shared migration plan file.

**Scope:**
- Included: Merging blocking and ready PRs into f1r3node rust/dev
- Excluded: Any work in f1r3node-rust (that starts in EPOCH-002)

**Notes:**
- The 11-PR Reified RSpaces chain has a sequential dependency — each PR targets the previous one
- Chain base (#328) depends on `new_parser` branch which depends on `rholang-rs#83`
- Monitor `/tmp/migrationPlan.md` for `phase_1_critical_prs.status` to know when to start EPOCH-002

---

### EPOCH-002: Code Sync to f1r3node-rust

```yaml
---
epoch_id: EPOCH-002
title: "Code Sync to f1r3node-rust"
status: pending
priority: p0
user_story: US-001
blocked_by: [EPOCH-001]
created_at: 2026-04-09
claimed_by: null
claimed_at: null
tasks:
  - id: TASK-002-1
    title: "Sync Rust workspace crates from f1r3node rust/dev"
    status: pending
    acceptance:
      - "All 11 workspace crates updated from f1r3node rust/dev HEAD"
      - "Cargo.toml workspace dependencies match source"
      - "cargo build --workspace succeeds"
      - "cargo test --workspace passes (per-crate to avoid LMDB contention)"

  - id: TASK-002-2
    title: "Port CI/CD workflows"
    status: pending
    blocked_by: [TASK-002-1]
    acceptance:
      - "build-test-and-deploy.yml ported (Docker build, multi-arch, artifact publishing)"
      - "release.yml ported (automated versioning, changelog, tagging)"
      - "cliff.toml ported (changelog generation)"
      - ".github/apt-dependencies.txt ported"
      - "Docker image name set to f1r3fly-rust in CI"

  - id: TASK-002-3
    title: "Port Docker configuration"
    status: pending
    blocked_by: [TASK-002-1]
    acceptance:
      - "node/Dockerfile updated with correct image labels"
      - "docker/standalone.yml, shard.yml, observer.yml, validator4.yml ported"
      - "docker/monitoring/ (Prometheus, Grafana) ported"
      - "docker/conf/ (node config templates) ported"
      - "docker/genesis/ (bonds, wallets) ported"
      - "docker/.env.example ported"
      - "All compose files reference f1r3fly-rust image name"

  - id: TASK-002-4
    title: "Port scripts and local dev configuration"
    status: pending
    blocked_by: [TASK-002-1]
    acceptance:
      - "scripts/version.sh ported"
      - "scripts/clean_rust_libraries.sh ported"
      - "scripts/delete_data.sh ported"
      - "scripts/run_rust_tests.sh ported"
      - "run-local/ configuration ported"

  - id: TASK-002-5
    title: "Set version and create initial tag"
    status: pending
    blocked_by: [TASK-002-1, TASK-002-2]
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
- Excluded: Issue migration (EPOCH-003), external repo updates (EPOCH-004)

**Notes:**
- The code delta is ~4 releases (v0.4.9-v0.4.11) plus the critical PRs from EPOCH-001
- Docker image renamed from `f1r3fly-rust-node` to `f1r3fly-rust`
- Version drops the `rust-` tag prefix (no longer needed in a Rust-only repo)
- Run tests per-crate to avoid LMDB lock contention (see commit f2b4b5f)

---

### EPOCH-003: Issue Migration

```yaml
---
epoch_id: EPOCH-003
title: "Issue Migration"
status: pending
priority: p1
user_story: US-001
blocked_by: [EPOCH-002]
created_at: 2026-04-09
claimed_by: null
claimed_at: null
tasks:
  - id: TASK-003-1
    title: "Migrate 22 Rust-relevant issues to f1r3node-rust"
    status: pending
    acceptance:
      - "16 Rust-specific issues created on f1r3node-rust with original context"
      - "6 triage/design issues created on f1r3node-rust with original context"
      - "Each new issue references the original f1r3node issue number"
      - "Original issues on f1r3node get a redirect comment"

  - id: TASK-003-2
    title: "Close 5 Scala-only issues on f1r3node"
    status: pending
    acceptance:
      - "Issues #452, #366, #321, #221, #184 closed with deprecation comment"
      - "Comment directs to f1r3node-rust if the work is still relevant"
      - "phase_3_issues.status set to 'complete' in /tmp/migrationPlan.md"
---
```

**Context:** Transfer the 27 open issues from f1r3node to their appropriate destinations. 22 issues migrate to f1r3node-rust, 5 Scala-only issues are closed.

**Scope:**
- Included: Issue creation, cross-referencing, closing Scala issues
- Excluded: Fixing any of the migrated issues

---

### EPOCH-004: External Repo Updates

```yaml
---
epoch_id: EPOCH-004
title: "External Repo Updates"
status: pending
priority: p1
user_story: US-001
blocked_by: [EPOCH-002]
created_at: 2026-04-09
claimed_by: null
claimed_at: null
tasks:
  - id: TASK-004-1
    title: "Update system-integration repo"
    status: pending
    acceptance:
      - "Docker image references updated from f1r3fly-rust-node to f1r3fly-rust"
      - "CI triggers updated to reference f1r3node-rust repo"
      - "Integration tests pass against new image"

  - id: TASK-004-2
    title: "Update pyf1r3fly repo"
    status: pending
    acceptance:
      - "Repo references in docs and CI updated"
      - "PR #4 cross-reference updated (references f1r3node #407)"

  - id: TASK-004-3
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

### EPOCH-005: PR Cleanup & Redirect

```yaml
---
epoch_id: EPOCH-005
title: "PR Cleanup & Redirect"
status: pending
priority: p1
user_story: US-001
blocked_by: [EPOCH-002]
created_at: 2026-04-09
claimed_by: null
claimed_at: null
tasks:
  - id: TASK-005-1
    title: "Redirect Tier 3 PRs to f1r3node-rust"
    status: pending
    acceptance:
      - "PRs #457, #426, #424, #407, #405 receive redirect comment"
      - "Comment includes rebase instructions for f1r3node-rust"
      - "PRs closed on f1r3node"

  - id: TASK-005-2
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
- Excluded: Tier 1/2 PRs (handled in EPOCH-001)

---

### EPOCH-006: Deprecation & Archive

```yaml
---
epoch_id: EPOCH-006
title: "Deprecation & Archive"
status: pending
priority: p2
user_story: US-001
blocked_by: [EPOCH-003, EPOCH-004, EPOCH-005]
created_at: 2026-04-09
claimed_by: null
claimed_at: null
tasks:
  - id: TASK-006-1
    title: "Update f1r3node README with deprecation notice"
    status: pending
    acceptance:
      - "README.md updated on rust/dev, main, and default branch"
      - "Notice points to F1R3FLY-io/f1r3node-rust"
      - "Last Rust release version documented"

  - id: TASK-006-2
    title: "Update GitHub repo metadata"
    status: pending
    acceptance:
      - "Repository description set to 'DEPRECATED - See F1R3FLY-io/f1r3node-rust'"

  - id: TASK-006-3
    title: "Disable CI and close remaining items"
    status: pending
    blocked_by: [TASK-006-1]
    acceptance:
      - "All GitHub Actions workflows disabled on f1r3node"
      - "Any remaining open issues closed with redirect comment"

  - id: TASK-006-4
    title: "Archive f1r3node repository"
    status: pending
    blocked_by: [TASK-006-1, TASK-006-2, TASK-006-3]
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
- Do NOT archive until Phases 3-5 are confirmed complete
- The other agent in f1r3node should NOT start this until signaled

---

## Epoch Dependency Graph

```
EPOCH-001 (f1r3node: merge critical PRs)
    |
    v
EPOCH-002 (f1r3node-rust: code sync)
    |
    +-------+-------+
    |       |       |
    v       v       v
EPOCH-003 EPOCH-004 EPOCH-005
(issues)  (ext repos) (PR cleanup)
    |       |       |
    +-------+-------+
            |
            v
       EPOCH-006
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
6. **Move epoch**: When all tasks complete, move epoch to `docs/CompletedTasks.md`

---

## References

- **Shared Migration Plan:** `/tmp/migrationPlan.md`
- **User Stories:** `docs/UserStories.md`
- **Completed Work:** `docs/CompletedTasks.md`
- **Backlog:** `docs/Backlog.md`
