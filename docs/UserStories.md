---
doc_type: user_stories
version: "1.0"
last_updated: "2026-03-19"
---

# User Stories

This document captures user stories that drive feature development. User stories are reverse-engineered from completed epochs and updated as new features are planned.

**Document Structure**
- Active stories: This file (`docs/UserStories.md`)
- Implementation tracking: `docs/ToDos.md` (epochs and tasks)
- Completed work: `docs/CompletedTasks.md`

**Format:** Each story follows the standard template:
> As a [persona], I want [capability] so that [benefit].

---

## Completed Stories

<!-- Add completed user stories here -->

---

## Planned Stories

#### US-001: System-Integration Compatibility

> As a **platform operator**, I want **f1r3node-rust's Docker configuration to be directly compatible with the system-integration orchestration tooling** so that **the migration from dual Scala/Rust support to Rust-only can proceed without manual fixups**.

**Implemented in:** EPOCH-001, EPOCH-002

**Acceptance Criteria:**
- [x] Genesis wallets.txt identical between repos (20 wallets, correct balances)
- [x] Docker image env var standardized to `F1R3FLY_IMAGE`
- [x] Shard network name standardized to `f1r3fly-shard`
- [x] Monitoring separated into its own compose file (matches system-integration pattern)
- [x] Shard verified to start with updated configuration
- [ ] system-integration's `services.yml` can point to this repo's `master` branch

**Completed:** Planned

---

#### US-002: Auto-Versioning and Nightly Releases

> As a **platform operator**, I want **automated semantic versioning with nightly tagged releases and machine-readable changelogs** so that **deployments are traceable, rollbacks target specific versions, and downstream consumers (system-integration, Docker Hub) can pin to known-good releases**.

**Implemented in:** EPOCH-003

**Acceptance Criteria:**
- [ ] Nightly GitHub Actions workflow checks master for changes since last `v*` tag
- [ ] If changes exist: bumps minor version, updates `node/Cargo.toml`, generates CHANGELOG.md, commits, tags `vX.Y.Z`
- [ ] Tag push triggers existing CI (Docker build + integration tests + release to Docker Hub)
- [ ] `VERSIONING.md` documents lineage from legacy `f1r3node.git` repo (`rust-v0.2.0` → `v0.2.0`)
- [ ] `scripts/release.sh` provides manual major/patch bump escape hatch
- [ ] `git-cliff` config generates conventional commit changelog
- [ ] Startup banner shows "F1r3node" instead of "F1r3fly Node"

**Completed:** Planned

---

## Relationship to Epochs

User stories capture the **why** (user need and benefit). Epochs capture the **what** (technical implementation tasks).

| Artifact | Purpose | Location |
|----------|---------|----------|
| User Story | Business/user need | `docs/UserStories.md` |
| Epoch | Implementation scope | `docs/ToDos.md` |
| Task | Technical work item | Nested in epoch YAML |
| Acceptance Criteria | Definition of done | In user story |

**Workflow:**
1. Identify user need -> Create user story
2. Design solution -> Create epoch with tasks
3. Implement -> Work through tasks via `/nextTask` and `/implement`
4. Complete -> Mark epoch complete, update story status

---

## References

- **Task Tracking:** `docs/ToDos.md`
- **Completed Work:** `docs/CompletedTasks.md`
