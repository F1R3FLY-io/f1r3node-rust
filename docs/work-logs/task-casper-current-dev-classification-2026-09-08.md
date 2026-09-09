---
doc_type: work_log
task: ca-casper-current-dev-classification
status: review_ready
date: 2026-09-08
next_steps:
  - Reconcile the decision ledger with the pinned comparison.
  - Prepare the current PR 390 correction record.
  - Obtain upstream review before changing disputed consensus policies.
---

# Current dev and feature Casper classification

## Result

The [requested comparison](../../../casper-dev-vs-cost-accounting-architecture-comparison.md) now uses the September 8 upstream revision.
It contains the architecture verdict, semantic classifications, overlap analysis, source anchors, and working-source hashes.
It contains no diagrams, as requested.

Both branches retain the block DAG, maximum-clique search, GHOST descent, and replay.
The feature changes authority, admission, certificate, and durable-publication boundaries.
Shared algorithms do not establish identical decisions or complete architectural equivalence.

No consensus implementation changed for this task.
No merge, commit, push, node execution, test suite, or formal gate ran for this documentation refresh.

## Baselines

| State | Commit |
|---|---|
| Current upstream dev | `cdf447ac18710d9702a27379bce6c946f421be46` |
| Previously merged dev | `375933475456407cdb47c455a9146f9aa93579c6` |
| Local feature HEAD | `559eb07fac98da2e6392e3a84c93bac41558c86c` |
| Remote PR 216 head | `3980ed402b4b3248f065d1f07daedbd3dc8b4493` |
| Current PR 390 head | `1d9ae249d75a39d8c0851bf71264307047c532e7` |

Current dev contains 67 commits beyond the previously merged dev revision.
It contains 51 commits beyond the comparison's earlier September 5 upstream pin.
The feature candidate includes uncommitted and untracked source files.
Its local HEAD is not a digest of that candidate.

The review fetched the immutable upstream commit with `--no-write-fetch-head --no-tags`.
The review did not move branch references or change another worktree.
The final reference check retained the original HEAD, local dev, and origin/dev values.

## Material classifications

| Area | Finding |
|---|---|
| Finality threshold | Current dev accepts equality. The feature requires a strictly greater clique threshold. |
| Clique electorate | Current dev uses main-parent weights. The feature uses certified active floor weights. |
| GHOST scores | Current dev reads traversed-block weights. The feature uses frozen authority stakes. |
| Vote inputs | The feature changes eligible projections and missing-slot requirements. |
| Parent depth | Current dev measures from the main parent. The feature measures from the tallest ranked parent. |
| LCA search | Current dev excludes latest messages at least 1000 heights below the maximum. The feature omits this filter. |
| Parent capacity | The feature snapshot checks capacity after fork-choice calculation. Estimator truncation remains in both branches. |
| Floor discovery | Both implementations collect inherited floors and parent frontiers. The concept is not unique to the feature. |
| Floor preservation | The feature uses exact effects and state-support checks. Upstream retains a qualified re-collection assumption. |
| Recovery | Upstream has frontier-follow, all-eligible stale recovery, and leader-only convergence. Feature recovery selection affects one proposal lane. |
| Merge algebra | Feature exact witnesses use additive effect composition. Legacy witnesses use max union. Mixed modes fail. |
| Activation | The current feature supports fresh Casper protocol-6 genesis. Historical helpers do not establish rolling-upgrade support. |

These findings do not establish that each feature choice is correct or necessary.
They identify decisions that need evidence and upstream ratification.

The threshold witness uses agreeing stake 6, clique stake 6, total stake 10, and threshold 200000/1000000.
The exact cross-products both equal 12,000,000.
The upstream predicate returns true, and the feature predicate returns false.
This source-derived witness is not a reproduced multi-validator failure.

## Overlapping upstream repairs

Current dev adds a between-deploy proposal budget, an independent unresolved-request clock, and bounded detached block announcements.
The feature does not contain those controls in the reviewed paths.
The proposal budget limits a selected prefix, not an individual deploy's execution time.

Both branches address retry quarantine retention through different data lifetimes.
The feature retains dependency provenance in its request record and permits bounded probes after quarantine expiry.
An integration must preserve that invariant instead of replacing the implementation by name.

Initial request errors still propagate in the feature, although maintenance continues after an individual dispatch error.
This is partial overlap with the newer upstream error-handling change.
The upstream readiness route and newer carrier-index measurements are also absent from the reviewed feature paths.

These are integration observations, not attributed causes for every CI failure.
They do not authorize a merge or automatically make additional audit work an M1 blocker.

## Specification boundary

The accounting paper's Casper subsection requires matching-token backing and accounting-aware block validity.
It does not choose threshold equality, stake provenance, recovery leadership, parent depth, or certificate encoding.

The required invariant and its implementation must remain separate review subjects.
An invariant can be necessary without making every proposed representation necessary.
This distinction applies to exact settlement, replay identity, state preservation, and durable publication.

## Independent review and corrections

The authorized plan agent reviewed the pinned clique, GHOST, floor, recovery, and merge implementations.
The plan agent then reviewed the refreshed comparison document.
The document incorporates all six final precision corrections.

1. Place the snapshot capacity check after fork-choice calculation.
2. Distinguish required invariants from specific representations.
3. Describe upstream containment as an intended invariant with an unresolved re-collection assumption.
4. Limit the commutativity claim to per-datum multiplicities, not right-biased join maps.
5. Correct the latest-message depth boundary from greater than 1000 to at least 1000.
6. Describe the proposal budget as between-deploy prefix selection, not a hard execution-time limit.

The final review supports the listed source classifications.
It does not provide an end-to-end equivalence proof or replace the required concurrent qualification.

The later PR 390 critique review adds two merge distinctions to the same report.
Feature exact mode rejects inconsistent serialized joins, while upstream and feature legacy mode retain right bias.
Upstream cancels at each fold step, while feature legacy mode cancels after the complete fold.
An ordered add/remove/add example distinguishes these operators without establishing a reachable network failure.

## PR 390 and remaining work

PR 390 now targets dev and remains open.
Its current decision index still names older comparison revisions.
Its linked-comment ratification requirements remain distinct from agent claims or a successful documentation build.

The decision-ledger reconciliation and PR 390 correction record remain separate scheduled tasks.
The current comparison does not mark them complete.
Formal-gate subsumption, implementation acceptance, CI, and the 24-hour soak retain their existing requirements.

Pgmcp progress record 11231 contains the detailed source investigation.
A subsequent progress record contains the final review corrections and document checks.
The approved milestone order remains M1 requested work, M2 required follow-up, and M3 optional improvements.
