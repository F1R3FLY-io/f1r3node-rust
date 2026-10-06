---
kind: handoff
from: pi-casper-merge-accounting
to: claude-session-9f19b46c
produced_at: 2026-09-19T06:49:15Z
retention: durable
focus: Include accepted TASK-017-8 evidence in retention preparation without changing accepted report bytes.
related_task: TASK-017-14
related_epic: EPIC-017
redaction:
  categories_touched: []
  total_substitutions: 0
suggested_skills:
  - /cbc-verify
consumed_at: "2026-09-19T06:50:48Z"
consumed_by: "claude-session-9f19b46c"
---

# Accepted Merge and Accounting Evidence

## Current State

The user accepted TASK-017-8. Claim-005 is discharged for its bounded pre-merge binding, and the task is complete.

Claims 001–005 pass strict discharge. Claims 006–008 remain pending, and the full strict bundle returns 4.

## Key Artifacts

- `docs/work-logs/task-017-8-merge-accounting.md` records acceptance and scope.
- `docs/casper/cbc-evidence/runs/casper-merge-accounting-20260919-01/` retains the original review package and pending ledgers.
- `docs/casper/cbc-evidence/runs/casper-merge-accounting-acceptance-20260919-01/` contains the accepted report, validation, and digest lists.
- `target/task-017-8-acceptance/` contains local acceptance scripts, fresh checks, and strict completion evidence for retention review.

## Next-Agent Focus

Include these packages in the existing retention inventory. Preserve accepted report bytes, source hashes, previous-ledger references, and evidence reachability.

The original review validator expects pending acceptance and is now historical. The accepted report and validation describe current state.

TASK-017-12 remains preparation-only until its remaining prerequisites and dispatch approvals pass. This handoff grants no publication, deletion, repin, or node-execution authority.

## Suggested Skills

- `/cbc-verify` checks that retention changes preserve the current discharge results.

## Open Questions

- Which external asset will retain the supplemental acceptance checks before local scratch cleanup?

## Redaction Notes

No redactions were necessary. All artifact paths are relative, and no secrets or personal data are included.
