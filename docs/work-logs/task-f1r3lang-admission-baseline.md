---
task: rholang14-node-frozen-contract
status: review
handoff_status: ready
branch: feature/f1r3lang-mettail-only
next_steps:
  - Prove the prepared-program transition contract before changing runtime behavior
  - Extract the existing metered handoff and verify its error and checkpoint branches
---

# F1R3Lang admission baseline handoff

## Delivered boundary

The [contract](../design/f1r3lang-frontend.md), source/ABI fixture and focused
integration suite identify the approved engine base, selected live engine
sources, MeTTaIL development snapshot and application source-route inventory.
Public parsing and execution behavior are unchanged. The node branch remains
isolated from other ongoing node refactors.

The MeTTaIL dependency is explicitly unfinished development work, not a clean
release pin. The source scanner is a conservative textual tripwire, not a proof
that every indirect parser call has been identified.

## Local verification

All commands ran in the isolated node worktree. Rust commands used one build
job, disabled incremental compilation, an 8 GiB hard memory limit and no swap.

| Check | Result | Log under `target/verification/` |
|---|---|---|
| Initial locked baseline test | Rejected stale engine lockfile before compilation | `f1r3lang-frontend-baseline-1.log` |
| Offline reconciliation and baseline suite | Passed, 7 tests | `f1r3lang-frontend-baseline-2.log` |
| Locked, offline baseline suite | Passed, 7 tests | `f1r3lang-frontend-baseline-3.log` |
| Focused strict Clippy, no dependency linting | Passed with warnings denied | `f1r3lang-frontend-baseline-clippy-1.log` |
| Admission diagram rendering | Passed under a 1 GiB cap | `f1r3lang-admission-diagram-2.log` |

Formatting and whitespace checks passed. These are local execution results,
not independent verification or proof of end-to-end node readiness.

The baseline already declared local legacy-parser patches and dependency
changes absent from its lockfile. Cargo reconciled those existing declarations
offline; no dependency manifest was changed. The resulting lockfile SHA-256 is
`b450f08a0189f6ee7e561bf1307df776396dce9e9937e689685544be86cbbeea`.
Legacy parsing remains a baseline build dependency until the separately gated
public cutover; this reconciliation does not activate or endorse a fallback.

## Next implementation boundary

The proof must model negative budgets, frontend failure, metered initialization,
merge tracking, reducer invocation, existing error accounting and caller-owned
rollback. Preserve the caller's random state and move the exact admitted
process without cloning or reparsing it. Compile the small proof module alone,
with a 1 GiB hard limit and no swap; do not build the entire Rocq package.

The existing MeTTaIL lowerer, neutral frontend extraction, shared language-service
provider and actual public-route activation remain distinct follow-on work.
This baseline does not finish them or the Regex application.
