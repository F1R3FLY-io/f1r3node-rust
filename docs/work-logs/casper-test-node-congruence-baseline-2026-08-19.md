---
doc_type: work_log
task_id: TASK-015-1
epic_id: EPIC-015
created_at: 2026-08-19
handoff_status: ready
next_steps:
  - Port the duplicate-tree features into the canonical fixtures (see Baseline below)
  - Run /tdd cycles against docs/tdd-plans/casper-test-node-2026-08-11T02-59-57Z.md
---

# Casper Test-Node Congruence: Provenance and 2026-08-19 Baseline

## Provenance

The congruence plan came from an architecture review on 2026-08-11
(`docs/discoveries/architecture-review-2026-08-11T02-59-57Z.md`). That file is
not retained: `docs/discoveries/` is gitignored in this repository, and the
local copy is gone. The review's accepted outcome survives in the TDD plan:
candidate C1, the common-caller test-node design, with the interface contract
recorded in `docs/tdd-plans/casper-test-node-2026-08-11T02-59-57Z.md`.

A first consolidation refactor was implemented on
`chore/code-review-glossary-and-congruence` (commit 976b7a252, PR #230 closed).
The dev history rewrite and the 2026-08 finality work superseded it before it
merged. Its approach — collapse the duplicate `casper/tests` helper tree into
re-exports of `casper/src/rust/test_utils` — remains the accepted direction,
but it must restart against the baseline below.

## Baseline (dev at f9e3596bc, 2026-08-19)

The two fixture trees have diverged instead of staying parallel:

- `casper/tests/helper/test_node.rs` and
  `casper/src/rust/test_utils/helper/test_node.rs` differ by roughly 1,000
  lines.
- The duplicate tree holds features the canonical tree lacks, at minimum:
  `create_network_with_deploy_lifespan` (shard `deploy_lifespan` override for
  window-boundary specs) and the `MultiParentCasper`-typed casper accessor.
- The duplicate `genesis_builder` and `resources` gained the `merge_base`
  bond-state field and the `DeployLifecycleTables` in-memory stores in step
  with the finality rewrite.

## Sequencing consequence

Consolidation must port the duplicate-tree features into the canonical
fixtures first. Collapsing the duplicate tree to re-exports before that port
silently removes capabilities that current specs use.
