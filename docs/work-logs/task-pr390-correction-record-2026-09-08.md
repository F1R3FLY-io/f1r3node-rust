---
doc_type: work_log
task: ca-pr390-correction-record
status: review_ready
date: 2026-09-08
next_steps:
  - Obtain upstream review of disputed consensus decisions.
  - Continue approved M1 work without promoting non-blocking audits.
---

# PR 390 correction record

## Result

The [correction record](../casper/design/pr390-corrections-and-retained-findings.md) covers all 12 current PR 390 entries.
It preserves supported concerns and separates required corrections from unresolved protocol choices.
The record uses dev `cdf447ac18710d9702a27379bce6c946f421be46` and PR 390 `1d9ae249d75a39d8c0851bf71264307047c532e7`.
The candidate includes uncommitted source beyond feature HEAD `559eb07fac98da2e6392e3a84c93bac41558c86c`.

## Findings

The threshold difference requires semantic review, not only a text correction.
Both current implementations retain the two-sided disagreement rule and collect parent frontiers.
The promotion checklist needs separate equivalence and intentional-repair domains.
Recovery policy and frontier-follow removal need separate liveness decisions.

The current candidate does not implement the proposed in-place activation.
Signed phlo controls can coexist with token-based admission.
Arbitrary-payer funding and phlo restoration remain approved M1 requirements.
The old top-level binary policy does not complete those requirements.

Formal model registration, local invocation, CI reachability, and successful execution are different evidence states.
The correction record does not remove verification requirements or mark source presence as a passing result.

## Independent review

The authorized plan agent reviewed the correction record.
The final document includes both requested merge corrections.

1. Scope right-biased join maps to upstream and feature legacy mode.
2. Distinguish upstream pairwise cancellation from feature legacy cancellation after the complete fold.

Feature exact mode rejects inconsistent serialized joins for the same key.
The add/remove/add example distinguishes cancellation operators but does not demonstrate an admissible network failure.
The external comparison and status ledger contain the same corrections.

## Checks and limits

Focused checks cover all 12 decision rows, local links, sentence length, and whitespace.
No consensus implementation, Git reference, workflow, or PR metadata changed.
No heavy verification process or node ran.
No temporary files were created.

This deliverable is a source-based critique, not an end-to-end proof or protocol ratification.
The task has no configured acceptance criteria at this snapshot.
Pgmcp must not mark the task Verified from these document checks alone.
