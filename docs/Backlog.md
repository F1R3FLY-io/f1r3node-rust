---
doc_type: backlog
version: "1.0"
last_updated: 2026-03-19
---

# Backlog

This document captures deferred work, future ideas, and low-priority items that aren't ready for active development.

**Document Structure**
- Active work: `docs/ToDos.md`
- User stories: `docs/UserStories.md`
- Completed work: `docs/CompletedTasks.md`
- Deferred work: This file (`docs/Backlog.md`)

---

## Backlog Categories

Items are organized by category and rough priority within each category.

---

### Technical Debt

<!-- Items that improve code quality, performance, or maintainability -->

---

### Feature Ideas

<!-- Future features that have been identified but aren't yet prioritized -->

---

### Research & Exploration

<!-- Items that need investigation before they can become actionable -->

---

### Dependencies & Blockers

#### BACKLOG-DB-001: system-integration Branch Reference

```yaml
---
backlog_id: BACKLOG-DB-001
title: "system-integration services.yml targets branch dev, repo uses master"
category: blocked_external
priority: p2
added_at: 2026-03-19
blocked_by_external: "system-integration migration Phase 2"
expected_resolution: "When system-integration updates services.yml to point to f1r3node-rust.git"
---
```

**Description:** system-integration's `services.yml` currently references `branch: rust/dev` on the old `f1r3node.git` repo. When it switches to `f1r3node-rust.git` (Phase 2 of migration), it needs to target `master` instead of `dev`.

**When Unblocked:** Coordinate with system-integration to ensure `services.yml` uses `branch: master`.

---

## Promoting Items to Active Work

When a backlog item is ready for active development:

1. Create an epoch in `docs/ToDos.md` based on the backlog item
2. Create or link a user story in `docs/UserStories.md` if needed
3. Remove the item from this backlog (or mark as `promoted: true`)
4. Add a note referencing the original backlog ID

---

## References

- **Active Work:** `docs/ToDos.md`
- **User Stories:** `docs/UserStories.md`
- **Completed Work:** `docs/CompletedTasks.md`
