---
doc_type: user_stories
version: "1.0"
last_updated: 2026-04-09
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

## Planned Stories

Stories below are candidates for future epochs. Move to "Completed Stories" when implemented.

---

#### US-001: Migrate to Standalone Rust Repository

> As a **F1R3FLY developer**, I want **the Rust blockchain node to live in a standalone repository (f1r3node-rust) with clean Cargo-only tooling** so that **we can iterate faster without Nix/SBT/Scala build complexity and contributors only need standard Rust tooling**.

**Implemented in:** EPOCH-001 through EPOCH-006

**Acceptance Criteria:**
- [ ] All critical PRs (Reified RSpaces #328-#338) merged in f1r3node before cutover
- [ ] f1r3node-rust at full parity with f1r3node rust/dev HEAD
- [ ] CI/CD pipeline produces Docker images from f1r3node-rust
- [ ] All 22 Rust-relevant issues migrated to f1r3node-rust
- [ ] External repos (system-integration, pyf1r3fly) point at f1r3node-rust
- [ ] f1r3node archived with deprecation notice
- [ ] Docker image published as `f1r3fly-rust`
- [ ] Version continuity maintained (v0.4.x series)

**Completed:** Planned

---

## Completed Stories

<!-- Add completed user stories here -->

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
