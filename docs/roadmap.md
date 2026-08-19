---
doc_type: roadmap
repo: f1r3node-rust
updated: 2026-08-13
horizon_months: 6
owners:
  - github: F1R3FLY-io
releases:
  - version: v0.4.45
    status: in_progress
    theme: "Consensus stability, soak validation, and standalone Rust release readiness"
    notes: "Release readiness follows the active epic and issue-remediation sequence in docs/ToDos.md."
---

# F1R3node Rust Roadmap

> This file follows the [Roadmap, Release Date, and Version Normalization Standard](https://gitlab.com/smart-assets.io/gitlab-profile/-/blob/master/docs/common/roadmap-release-normalization-standard.md).
> Update the `updated` field when you change this roadmap.

## Current Focus

The current work improves consensus recovery, observer reliability, and sustained-load validation. The work also prepares the standalone Rust node for the next release.

## Upcoming Releases

### v0.4.45 — Consensus stability and release readiness

- **Target:** No calendar date is set.
- **Milestone:** A matching release milestone does not exist.
- **Status:** in progress

Complete the active work in this order:

1. Close the remaining work in `EPIC-011`.
2. Complete the prioritized issue-remediation tasks in `EPIC-012`.
3. Obtain a successful weekend soak result for the release candidate.
4. Publish the release from the validated `master` commit.

The release includes consensus recovery, checkpoint recovery, observer fixes, soak telemetry, and sustained-load performance improvements. It excludes tasks that remain pending after the release candidate is validated.

## Out of Scope / Deferred

- Tasks outside the active release sequence remain in `docs/Backlog.md` or a later `EPIC-012` task.
- A later minor release is not scheduled until the current issue-remediation queue has a stable scope.

## Change Log

| Date | Change |
|---|---|
| 2026-08-13 | Created the normalized roadmap during policy harmonization. |
