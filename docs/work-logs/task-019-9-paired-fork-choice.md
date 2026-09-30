# TASK-019-9: Batch D, paired fork-choice observation bound to one capture

## Session

- Implementer: `claude-session-f3cbc961` (agent A of the two-agent split of 2026-09-30).
- Claimed: `2026-09-30T04:40:00Z`.
- Branch: `feature/casper-node-observation`, head `e90e4cffa` at the claim.
- Epic: EPIC-019. Claim: `CLAIM-CASPER-NODE-OBSERVATION-004`, not registered yet.
- Consumer: TASK-017-12 on `formal/soak-casper-consensus`.
- Plan draft: `target/node-observation-prep-20260928-01/batch-d/` (local, ignored by Git). The accepted plan moves to `docs/plans/casper-node-observation-batch-d.md` at the file-scope confirmation.

## Split with agent B

Agent B (the pi session of 2026-09-30, [its work log](node-observation-agent-b-20260930.md)) owns the record refresh of the 10 evidence records, the STE fix in `docs/User-Flows.md`, the TASK-019-8 inventory, and Batch E after Batch D. Agent A owns Batch D and then the removals of TASK-019-8.

Rules: one agent edits `docs/ToDos.md` at a time. Each agent stages only its own files. The user makes each commit with `/quick-commit`.

## Gate before code

Steps 1 to 4 of the plan must be complete before a production file changes:

1. File-scope confirmation of the 34 files (17 new, 17 changed) and the answers to the 12 open questions.
2. Task record and work log (this file).
3. Pending claim registration and the mandatory tags.
4. Pending ledger records for the mandatory files.

## Draft currency check on 2026-09-30

The draft has the reviewed base `670037c25`. At `e90e4cffa` the 16 existing files of the plan have no change since that base (`git diff --stat 670037c25 HEAD -- <files>` is empty). The source findings of the draft hold: `evaluation.rs` has no estimator reference, `observation_work.rs` has 4 work paths, and no `fork_choice` symbol exists in the observer files. The CbC tags agree with the draft table.

## Gate 1 to 4 on 2026-09-30

The user confirmed the 34-file scope and accepted the 12 recommendations of the draft (question 8: the 3 consensus files get the mandatory tag and 3 ledger records). The decisions are in the [plan](../plans/casper-node-observation-batch-d.md).

Registered: the [plan](../plans/casper-node-observation-batch-d.md), the pending [claim](../claims/casper-node-fork-choice-observation.md) `CLAIM-CASPER-NODE-OBSERVATION-004`, 4 mandatory tag lines in `.gitattributes` (`fork_choice.rs`, `estimator.rs`, `dag_operations.rs`, `proto_util.rs`), and 14 pending ledger records with the scope `batch-d-registration`.

| Record group | Count | Note |
|---|---|---|
| Replaced records of changed files | 11 | `previous_record` names the replaced accepted record and its digest |
| New records of the newly tagged files | 3 | `sha256_at_registration` holds the digest before the change |
| New records of new files | 0 now, 14 at step 5 | Written when each file exists and has a digest. The user chose this on 2026-09-30 to limit empty files on the branch. The registration commit `bc6e2f3d4` had the 14 placeholders, and the next commit removed them. |
| Kept under another claim | 1 | `scripts/ci/check-tla-invariants.sh` stays under `CLAIM-SOAK-GATE-001`, as at the Batch B2 registration |

The record generator is `target/batch-d-registration-20260930-01/write-pending-records.sh` (local, ignored by Git). Each record parses as JSON.

## Progress

- [x] Claim the task and start the work log.
- [x] File-scope confirmation and the 12 answers.
- [x] Pending claim, mandatory tags, pending ledger records.
- [ ] Bounded model and 5 controls, registered in the gate.
- [ ] 2 work paths.
- [ ] Metered functions in the 4 consensus files.
- [ ] Input record and input digest.
- [ ] `bounded` evaluation and tests.
- [ ] `reference` evaluation and tests.
- [ ] Comparison, response field, negative controls.
- [ ] Capability entry and node tests.
- [ ] Binding entries and applicability review.
- [ ] Gate, binding check, test suites, results recorded.
- [ ] Compact evidence package and the acceptance request.
