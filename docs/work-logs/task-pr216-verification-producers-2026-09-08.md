---
doc_type: work_log
task: pr216-principled-repair-closeout
status: in_progress
date: 2026-09-08
next_steps:
  - Record the independent pruning-plan review through the attributed review API.
  - Resolve unsupported verification evidence without weakening acceptance criteria.
  - Preserve the upstream architecture approval boundary.
---

# M1 verification producer audit

## Scope

This audit identifies the remaining evidence requirements for approved M1 work.
It does not promote M2 state-import tasks or select a Casper architecture change.
It does not authorize modifications to pgmcp.

The previous turn completed the requested comparison documents and changed authoritative planning records.
This turn checks the remaining verification requirements rather than repeating those completion statements.

## Current tracker state

The recipe list for the F1R3node project is empty at this snapshot.
Existing local test and model results have manual provenance in pgmcp.
That provenance cannot satisfy the machine-verification gate.
No result becomes trusted merely because the command previously passed.

| Task | Criterion | Actual requirement | Current limitation |
|---|---|---|---|
| `pr216-pruning-design` | 3663 | Independent review of alternatives, source pins, tradeoffs, hazards, and approval boundaries | The attributed review API can record an actual independent review. |
| `pr216-pruning-review` | 3654 | Upstream Casper review and user disposition for the exact repair | Bug-fix permission does not establish architecture approval. |
| `pr216-pruning-red` | 3655 | Two specific preservation failures and one passing control before production edits | A generic nonzero process exit cannot establish these outcomes. |
| `pr216-pruning-provenance` | 3656 | The same regression contract against pinned upstream modules | The evidence must retain exact module identity and comparison limits. |
| `pr216-cache-lock-red` | 3667 | Six specific guard-release failures and the passing control | The current Rust result adapter requires successful tests. |
| `pr216-cache-lock-model` | 3668 | Two safe concurrent models and two named unsafe counterexamples | The current TLC result adapter requires a successful exhaustive run. |
| `pr216-soak-qualification-hardening` | 2081 | Workflow qualification tests, including duration and pin-change rejection | The current recipe schema has no Python test-result adapter. |

These limitations concern evidence production, not permission to weaken the requirements.
The actual 24-hour soak remains a separate, uncompleted qualification task.

## Producer capability evidence

The live `work_item_verification_recipe` schema exposes four result adapters.
They cover process exits, successful Rust tests, successful TLC runs, and Rocq checks.
The inspected pgmcp source explains their boundaries.

| Source | Observation |
|---|---|
| `pgmcp-verifier/src/recipe.rs:362–379` | Rust receipts require direct `cargo test`, a positive test count, and expected exit zero. |
| `pgmcp-verifier/src/recipe.rs:382–410` | TLC receipts require named properties, explicit bounds, exhaustive checking, and expected exit zero. |
| `pgmcp-verifier/src/recipe.rs:496–506` | A process-exit adapter cannot satisfy test, proof, model-check, or SMT criteria. |
| `pgmcp-verifier/src/outcome.rs:67–96` | Failed Rust tests produce a failed verdict, even when a failure demonstrates the required defect. |
| `pgmcp-verifier/src/outcome.rs:109–116` | A TLC property violation produces a failed verdict rather than an expected-counterexample receipt. |

The inspected files are in the sibling pgmcp worktree.
Their hashes bind this source inspection, not the deployed daemon executable.

| File | SHA-256 |
|---|---|
| `pgmcp-verifier/src/recipe.rs` | `6b944984767c1af2ab5df2d3f3f43ba5b81008a5a652cfdc6c96db3c322a0972` |
| `pgmcp-verifier/src/outcome.rs` | `19dd148ba80eb5382bf1cda2e9bbb28d01d3f5141698d995e250c0732f701a75` |

Do not relabel a test or counterexample obligation as a generic script-exit obligation to satisfy the gate.
Do not convert manual summaries into machine-produced receipts.
Any producer extension must distinguish expected assertion failures from compiler errors, fixture failures, timeouts, cancellation, and resource termination.
An expected TLC counterexample must identify the required property and retain its trace and model inputs.
A Python test producer must reject missing, skipped, incomplete, or incorrectly selected required tests.

## Reproduction identity recheck

Current dev revision `cdf447ac18710d9702a27379bce6c946f421be46` retains both modules used by the earlier controlled pruning reproduction.
The buffer blob remains `1c15061f86aaecf2515ac96cb28c552830263bfd`.
The dependency-DAG blob remains `545855f7db425f8de56461f7f3934bb7364ec2bc`.

The feature buffer, dependency DAG, and public fixture hashes also match the recorded pre-fix run.
The feature log still records three selected tests, two preservation assertion failures, and one passing control.
The upstream `result.txt` explicitly states that it summarizes observed output rather than preserving a verbatim log.

Unchanged module blobs do not establish a fresh current-dev build or network reproduction.
The earlier run used current-workspace dependencies with pinned upstream modules.
The [pruning decision packet](../casper/theory/finalized-floor/buffer-pruning-preservation.md) records that boundary.

## Resource and authority limits

The independent review now satisfies criterion 3663 through `work_item_submit_review`.
Pgmcp recorded decision receipt 18, evidence 596, and completion receipt 4.
Only `pr216-pruning-design` transitioned to Verified.
The [attributed report](reviews/pr216-pruning-design-2026-09-08.md) retains the reviewer's findings, assumptions, independence statement, and artifact hashes.

The September 8 PR 216 check found no submitted reviews or inline review comments.
Its sole discussion comment is the August 13 automated multi-agent review.
That comment does not approve the September pruning plan.
Scheduler revision 675 now selects the separate upstream disposition task.
PR 390 also has no submitted reviews or new discussion comments since its September 5 automated review and response.
The upstream disposition task is now Blocked pending the exact external decision.
The agent released its lease and did not start the dependent implementation.
This does not mark the campaign complete or move M2 work ahead of M1.

This audit ran only source inspection and metadata queries.
It started no compiler, formal verifier, shard, or soak.
It created no temporary files.
No recipe was approved or executed.
No acceptance criterion, consensus implementation, or Git reference changed.
