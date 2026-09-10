---
doc_type: work_log
task: cost-accounted-rho-two-paper-native-completion-epic
status: in_progress
date: 2026-09-08
next_steps:
  - Complete the trusted transfers for proposals 4 and 5.
  - Verify the final milestone membership and scheduler order.
  - Continue the requested campaign work.
---

# Campaign milestone partition

## Approved order

The user approved three ordered milestones.

| Milestone | Required scope |
|---|---|
| M1 Complete the requested campaign | Assigned repairs, cost accounting, arbitrary-payer funding, signed phlo controls, Casper comparison, required assurance, final CI, and the 24-hour qualification. |
| M2 Complete required follow-up repairs | Additional confirmed defects and required follow-up work that do not block M1. |
| M3 Optional improvements | Improvements beyond accepted requirements. No optional tasks are assigned now. |

Keep each required proof, test, and documentation task with its implementation work.
Do not classify an unresolved requirement as optional.
A follow-up task enters M1 only when recorded evidence demonstrates an assigned-work blocker.
The separate regressor project remains outside this campaign.

## Changes applied

Pgmcp contains the three milestone records.
Sixteen existing root groups now sit under M1.
These groups retain their original technical breakdown.
The Casper comparison remains in its original epic and now has explicit M1 membership.

The PR 216 technical groups remain inside their existing nested epic.
A new plan container groups its eight requested-work milestones without changing epic ownership.
The additional state-import group remains separate.

Pgmcp does not support moving the nested epic itself.
Its cross-epic transfer workflow requires trusted approval for the two non-epic subtrees.

| Proposal | Source | Destination | State |
|---|---|---|---|
| 4 | Requested PR 216 repair groups | M1 | Proposed, not applied. |
| 5 | Additional audit findings | M2 | Proposed, not applied. |

The user's earlier approval of proposal 3 concerned a different grouping.
The three-milestone plan now keeps the comparison in its original epic.
Do not execute proposal 3 as an automatic follow-up.

One historical debugging note has different project ownership.
Its parent remains unchanged because pgmcp rejected that ownership change.
Its M1 association identifies its supporting role without changing project ownership.

## Preservation audit

The before-and-after inventory retains all 280 original records.
The new records are three milestones and one plan container.
No original identifier, kind, title, body, priority, weight, status, project, commit requirement, or verification revision changed.

The inventory contains 199 requested-work leaves and five required follow-up leaves.
These counts include claimed work and pending work.
They are not counts of unfinished implementation tasks.
No task was changed to completed, verified, deferred, or optional.

The five follow-up leaves cover runtime reproduction, horizon reproduction, formal models, repair implementation, and conformance tests.
An earlier progress message incorrectly called these six tasks.
The sixth state-import record is their plan container, not an actionable task.

No dependency or acceptance tool changed requirements during this partition.
Required gates remain in place.
The [machine-readable inventory](data/campaign-milestone-partition-2026-09-08.json) records task identities, classifications, and pending transfers.

## Scheduler result

Scheduler revision 666 selects the requested Casper comparison before additional audit work.
The decision ledger and PR 390 correction record follow the comparison.
The added audit still sorts later.

The three comparison tasks now have Claimed Done status, not Verified status.
They produced the current comparison, ratification status ledger, and PR 390 correction record.
Scheduler revision 673 offers only additional state-import work while M1 prerequisites remain unsatisfied.
The agent did not claim that follow-up work because the user requires M1 first.

The transfer review tool was enabled again after the documentation work.
This client still does not expose its direct approval form.
The wrapper and CLI do not provide transfer approval authority.
Proposals 4 and 5 therefore remain unapplied.

The partition is not fully applied until transfers 4 and 5 complete through pgmcp.
Their pending state does not authorize an alternative transfer route.
No repository implementation, consensus rule, verification process, or Git reference changed in this work.
