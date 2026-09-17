---
task: TASK-017-3
branch: formal/soak-casper-consensus
claimed_by: pi-casper-harness
handoff_status: in_progress
execution_scope: authorized-working-tree-application
---

# TASK-017-3 Prerequisite Application

The user authorized working-tree application of the reviewed prerequisites and completion of the candidate matrix. Staging, commits, pushes, and merges remain unauthorized.

The starting revision is `2c1ce13d0cf291d3b80ca40f3c8d1b7f6b4b9d12`. The starting working tree and index were clean.

The [review log](task-017-3-prerequisite-review-2026-09-17.md) retains the source review, fixture results, inherited evidence audit, and unresolved limits.

## Application order

| PR | Selected revision | State |
| --- | --- | --- |
| 430 | `dedb3add172098efcbc63d72b2ebdc612f2fddcd` | Pending |
| 431 | `0e176e486a028d10704b96add6e5eb50525682cb` | Pending |
| 432 | `e0380392bcc66d9774e403edb8a08415798c1e0e` | Pending |
| 433 | `65f7f6daa832c0acb6fddf2b462db1b9d5461729` | Pending |

## Preservation requirements

Preserve the Casper contracts, claims, models, evidence records, compatibility links, and unrelated task states. Keep imported prerequisite claims separate from Casper claims.

Do not infer upstream merge status from local file application. PR #216 remains an optional candidate, not the default authority.

B44 containment and current profile-binding limits remain open. Imported bounded models do not establish node correctness.

## Remaining work

- Apply the selected file changes in order.
- Reconcile shared documentation and evidence paths without replacing existing claim records.
- Record immutable candidate identities and explicit unavailable candidates.
- Verify the resulting files and retain actual outcomes.
- Run the completion gate without waiving unresolved correctness claims.
