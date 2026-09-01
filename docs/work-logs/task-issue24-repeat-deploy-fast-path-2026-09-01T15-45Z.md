---
task: issue-24-repeat-deploy-lifecycle-fast-path
branch: perf/repeat-deploy-lifecycle-fast-path
claimed_by: claude-session-ae9e5a86
claimed_at: 2026-09-01T15:00:00Z
handoff_status: ready
next_steps:
  - User runs /quick-commit and /recursive-push, then opens the PR against dev
  - PR body must say "Addresses #24", never "Closes"
  - Maintainer ratification needed for the 2026-09-01 decision-record row in CONSENSUS_PHILOSOPHY.md
  - The section 4.3 scan benchmark (256/512 release gate) has no in-repo implementation to re-run; note this in the PR
---

# Issue #24 residual: repeat-deploy signature-index fast path

## Scope

The sustained-phase residual of issue #24 is the `Validate::repeat_deploy`
BFS ancestor scan. This branch adds an absence-proof fast path backed by
the deploy lifecycle rows, gated on a certified completeness invariant.
A CbC design review (docs/casper) ran before implementation and ruled the
absence-proof shape admissible only with a design amendment. The
amendment is part of this branch.

## Design deltas (ratification-level)

- `docs/casper/GLOSSARY.md`: new entry "Repeat-deploy signature index";
  amended the "Prior-rejection count" distinguo.
- `docs/casper/CONSENSUS_PHILOSOPHY.md`: new section 4.4 (fast-path
  contract); new decision-record row dated 2026-09-01, pending
  ratification.
- `docs/casper/CONSENSUS_PROTOCOL.md`: Step 8 now states the
  no-validity-qualifier scan, the retry-gate exemption, and the index
  equivalence obligation.
- `docs/casper/design/cbc-repair-plan.md`: three new claims in "Merge and
  deploy lifecycle" (completeness, refusal on read failure, verdict
  equality).

## Implementation

- `block-storage/src/rust/dag/deploy_lifecycle_types.rs`: new
  `LifecycleEventKind::CarriedInvalid` (appended for serde compat);
  completeness marker (25-byte reserved key, filtered from `open_sigs`);
  `proves_absence`; idempotent `append_event_once`.
- `block-storage/src/rust/dag/block_dag_key_value_storage.rs`: lifecycle
  ingest moved BEFORE the metadata-index add (crash-safe ordering);
  invalid blocks now ingest `CarriedInvalid` testimony (record-only,
  `valid_after` stays unset); `ensure_carrier_index_complete` startup
  backfill (marker written only when every DAG-visible body was
  readable); representation probes `carrier_index_complete` and
  `carrier_index_proves_absence`.
- `casper/src/rust/validate.rs`: fast path in `repeat_deploy` behind the
  completeness gate. Absence skips the scan. A hit or ANY read failure
  keeps the sig in the exact scan. The retry-gate exemption pass is
  untouched. `interpreter_util.rs` (CbC-mandatory) is untouched.
- `node/src/rust/runtime/setup.rs`: backfill wired next to the LFB
  migration precedent.

## Evidence

- 7 unit tests in `deploy_lifecycle_types.rs` (marker, absence probe,
  CarriedInvalid non-canonical, idempotent append).
- `block_dag_storage_test.rs`: invalid-carrier projection (updated pin)
  and the backfill restore/certify/no-duplicate test.
- `validate_test.rs`: three fast-path tests — certified repeat still
  flagged, certified fresh sig accepted, and the load-bearing one:
  a repeat carried ONLY by an INVALID ancestor gets the SAME verdict
  from the certified index as from the scan.
- `cargo clippy` clean on the three crates.

## Key design facts for reviewers

- Lifecycle rows are pruned at terminal writes, so a terminal record
  routes to the scan (a pruned row hides its carriers).
- Rows are node-global across forks; a hit must never directly flag a
  repeat (validate.rs:606 fork-poisoning rule). The exact scan IS the
  scope verification.
- The old deploy_index fast path (d84f26c07) was unportable because
  1a5174e1f removed its foundation and lifecycle rows skip invalid
  blocks. `CarriedInvalid` closes exactly that gap.

## Multi-review response (2026-09-01, comment 5497008226)

The multi-agent review returned 3 major findings. All 3 are addressed:

1. Backfill certification gap (openai, CONFIRMED): a DAG-listed hash
   whose metadata read returns None now refuses certification, same as
   a missing body. Test:
   `carrier_index_backfill_refuses_certification_on_missing_metadata`.
2. Orphan semantic events (openai, CONFIRMED in weakened form): the
   insert path now appends through the idempotent `append_event_once`,
   so a crash-then-redelivery cannot duplicate events, and canonical
   appearance is DAG-visibility filtered at both twins, so an orphan
   event never resolves as an appearance. The Failed-verdict path was
   already safe (floor-closure guard: a never-visible block cannot be
   in the floor closure). Tests:
   `insert_retry_after_ingest_first_crash_does_not_duplicate_events`,
   `orphan_lifecycle_event_is_not_a_canonical_appearance`.
3. Lock ordering (bedrock, REFUTED as a deadlock but documented):
   `insert` and the backfill already take `global_lock` ->
   `block_metadata_index` -> `lifecycle` in the same order; the order
   is now stated on `ensure_carrier_index_complete` and the storage
   appearance twin.
