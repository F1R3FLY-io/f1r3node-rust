---
doc_type: work_log
task: ca-casper-decision-status-ledger
status: review_ready
date: 2026-09-08
next_steps:
  - Complete the PR 390 correction record.
  - Obtain upstream review of disputed consensus decisions.
---

# Casper decision status ledger

## Result

The [ratification status ledger](../casper/design/cost-accounting-ratification-status.md) covers all 12 current PR 390 decisions.
It also records all 11 verification-governance sub-decisions.
Each entry separates observed implementation, motivation, required evidence, unresolved decisions, and approval authority.
The decision index records activation consequences.

The ledger uses dev revision `cdf447ac18710d9702a27379bce6c946f421be46` and PR 390 revision `1d9ae249d75a39d8c0851bf71264307047c532e7`.
The feature candidate includes uncommitted source beyond HEAD `559eb07fac98da2e6392e3a84c93bac41558c86c`.
The [comparison work log](task-casper-current-dev-classification-2026-09-08.md) identifies the source review and report.

## Approval and scope

All 12 upstream decisions retain Proposed status.
The inspected PR API contains no submitted reviews or inline review comments.
Its two discussion comments do not establish decision-specific ratification.

Missing upstream ratification does not revoke approved user economic requirements.
Arbitrary-payer funding and signed phlo controls remain required M1 work before PR amendment.
The ledger does not approve disputed protocol selections or move requirements into optional work.

## Review and checks

The authorized plan agent reviewed the ledger against the pinned source comparison.
The final text incorporates both precision corrections.

1. Name version 8 as the authority-accounting protocol, not a witness schema.
2. Qualify missing approval as upstream Casper ratification, distinct from user authorization.

The external comparison report receives the same protocol-label correction.
The Casper documentation map and historical decision records now link the ledger.

Document checks cover decision counts, reference definitions, whitespace, and sentence length.
These checks do not establish full ASD-STE100 conformance.
No consensus code, Git reference, verification gate, or PR metadata changed.
No heavy test, formal verifier, node, or soak ran for this documentation task.

## Limits

This result completes the status-document deliverable, not protocol verification.
Pgmcp has no configured acceptance criteria for this leaf at this snapshot.
The task must not receive Verified status from documentation checks alone.
The separate PR 390 correction record and concurrent end-to-end qualification remain required.
