# TASK-019-8: Final branch cleanup before the PR #447 merge

```yaml
handoff_status: paused
claimed_by: claude-session-7015f552
inventory_head: 51febc379
removals_authorized: false
next_steps:
  - Obtain authorization for the five proposed removals, or a decision to remove nothing.
  - Apply the authorized removals and rerun the three checks.
  - Record the diff sizes after the removals and request maintainer confirmation of the reduced diff.
```

## Scope

The user requested TASK-019-8 on 2026-09-23 after all three node observation claims were accepted. The task removes discovery notes, work logs, plans, and CbC evidence files that are not integral to the branch or its accepted claims. Production code scope belongs to TASK-019-6.

## Inventory refresh

The earlier inventory was taken at `03d7f1b27`, before three acceptance cycles, the `NodeAuthority` project, and the two Batch B2 packages landed. The refreshed inventory at `51febc379` covers 192 committed branch files under `docs`, `formal`, and `.github` against `dev`.

The classifier searched every branch file for an inbound reference by path or by digest across the claims, the records, the tracker, the work logs, the plans, the packages, the formal READMEs and manifests, the workflows, and the gate scripts. The TASK-019-8 inventory itself was excluded from the reference corpus so that its own file list creates no retention requirement.

| Classification | Files |
| --- | --- |
| integral | 138 |
| cited | 49 |
| removable | 5 |

Three rules refined the raw classification:

- Every `report.json` and `validation.json` companion is retained by the retention rule whether or not a path cites it.
- The two packages named in the maintainer acceptances keep their complete manifests. Removing a file from an accepted package would change the accepted package.
- Companion manifests inside a package that a claim, record, or report cites by directory are retained as package members.

## Findings

The branch documentation is almost entirely load-bearing. Every claim, record, formal input, workflow, and plan is integral or cited. The five work logs map one to one onto TASK-019-1, TASK-019-2, TASK-019-3, TASK-019-4, and TASK-019-7, and each is cited by the tracker, a claim, or a plan, so the one-per-task consolidation criterion is already met.

No historical red-source snapshot exists under `docs`. The pre-fix models under `formal` are registered refutation controls, not snapshots. No plan draft is uncited.

The only removable files are the `artifacts.sha256` self-manifests of the five packages that no acceptance names. Each is a digest list of its own package, and the retained report and companions carry the same identities. The proposed removals are listed with their digests in the tracker.

Removing those five manifests leaves every other package companion without a path citation, because the self-manifests were the only files that named them by path. Those companions remain retained as package members of directories that claims, records, and reports cite.

## Before checks

| Check | Result |
| --- | --- |
| Strict audit, claims 001 and 002 union | exit 4, 58 discharged, 2 pending under the soak gate claim |
| Strict audit, claim 003 | exit 0, 19 discharged |
| Link check, offline, docs and formal | 2188 links, 0 errors |
| STE check | baseline recorded for every branch Markdown file |

The diff against `dev` before any removal is 239 files, with 24,288 insertions and 323 deletions. Documentation accounts for 135 files and 12,047 insertions, formal inputs for 53 files and 2,746 insertions, tests for 10 files and 3,513 insertions, workflows and scripts for 8 files and 310 insertions, and source for 33 files and 5,672 insertions.

## Decision requested

The five proposed removals reduce the diff by five files and about 1.2 KiB. They change no claim status, record status, tier field, or audit result. The task holds at `removals_authorized: false` until the user or the maintainer authorizes the list or decides to remove nothing.
