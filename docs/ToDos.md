---
doc_type: todos
version: "1.0"
last_updated: 2026-03-19
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

---

## MR/PR Tracking

When all tasks in this file are complete and ready for merge, update the frontmatter:

```yaml
mr_status:
  ready: true
  target_branch: master
  title: "feat: [MR title]"
  description: |
    ## Summary
    - [Completed items]

    ## Test plan
    - [x] All tests passing
  labels: ["feature", "enhancement"]
```

---

## Active Epochs

<!-- Epochs are ordered by priority. Work on the highest priority epoch first. -->

---

### EPOCH-001: System-Integration Alignment

```yaml
---
epoch_id: EPOCH-001
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
    status: pending
    blocked_by: []
    acceptance:
      - "docker compose -f docker/shard.yml up succeeds"
      - "Genesis ceremony completes with 20-wallet wallets.txt"
      - "Observer and validator4 can join via f1r3fly-shard network"
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

### EPOCH-002: Separate Monitoring from Shard Compose

```yaml
---
epoch_id: EPOCH-002
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
    status: pending
    acceptance:
      - "docker/monitoring.yml contains prometheus and grafana services"
      - "monitoring.yml joins f1r3fly-shard as external network"
      - "shard.yml no longer contains prometheus/grafana services"
      - "docker/README.md updated to reflect new file"
---
```

**Context:** system-integration manages monitoring as a separate compose file (`compose/monitoring.yml`). Aligning this repo's structure makes compose files directly usable as upstream sources during the migration (Phase 3).

**Scope:**
- Move prometheus and grafana service definitions from `docker/shard.yml` to `docker/monitoring.yml`
- Update documentation

---

### EPOCH-003: Auto-Versioning and Nightly Release Pipeline

```yaml
---
epoch_id: EPOCH-003
title: "Auto-Versioning and Nightly Release Pipeline"
status: pending
priority: p1
user_story: US-002
blocked_by: []
created_at: 2026-03-25
claimed_by: null
claimed_at: null
tasks:
  - id: TASK-003-1
    title: "Port and adapt release workflow, cliff.toml, version.sh, release.sh"
    status: pending
    acceptance:
      - "nightly-release.yml triggers on schedule (08:00 UTC / midnight Pacific)"
      - "cliff.toml generates changelog from conventional commits (no include_path filter)"
      - "version.sh uses v* tag prefix, discovers current version from tags"
      - "release.sh provides manual major/minor/patch bump"

  - id: TASK-003-2
    title: "Create VERSIONING.md documenting lineage from legacy repo"
    status: pending
    acceptance:
      - "Documents that v0.2.0 continues from f1r3node.git rust-v0.2.0"
      - "Explains tag convention differences between repos"

  - id: TASK-003-3
    title: "Update docker-commands.sh to auto-detect version from Cargo.toml"
    status: pending
    acceptance:
      - "VERSION defaults to value from node/Cargo.toml if not set"

  - id: TASK-003-4
    title: "Set baseline version v0.2.0 and verify pipeline"
    status: pending
    blocked_by: [TASK-003-1, TASK-003-2, TASK-003-3]
    acceptance:
      - "node/Cargo.toml version is 0.2.0"
      - "Baseline tag v0.2.0 created on master after PR merge"
      - "Nightly workflow produces v0.3.0 on next change"
---
```

**Context:** The legacy `f1r3node.git` repo (PR #455) is adding auto-versioning with `rust-v*` tags and git-cliff changelogs. This repo needs the same capability but adapted for standalone use: `v*` tag prefix, nightly schedule instead of merge-triggered, no path filtering.

**Scope:**
- Nightly release workflow (midnight Pacific / 08:00 UTC)
- git-cliff changelog generation
- Version scripts for automated and manual bumps
- Lineage documentation from legacy repo
- Startup banner rename

**Notes:**
- Legacy repo highest Rust version: `rust-v0.2.0`. This repo starts at `v0.2.0`.
- Requires `RELEASE_PAT` org secret (PAT with `contents:write`) for tag pushes to trigger CI.

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
5. **Move epoch**: When all tasks complete, move epoch to `docs/CompletedTasks.md`

---

## References

- **User Stories:** `docs/UserStories.md`
- **Completed Work:** `docs/CompletedTasks.md`
- **Backlog:** `docs/Backlog.md`
- **System-Integration Migration Plan:** `../system-integration/docs/migration-to-rust-node.md`
