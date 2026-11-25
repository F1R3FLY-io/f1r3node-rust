# Issue #712 deploy store concurrency evidence

Status: pending maintainer review.
Base: `594df581ea9a818cb5373bb3cea8dc19fcbdd31b`.
The source hashes and validation state before publication are in [report.json](report.json).

## Preservation argument

The deploy pool uses shared references without an outer mutex.
Each LMDB operation retains its transaction boundary.
The rejected-buffer mutex still covers the admission check and the atomic pool insert.
The admission predicates, caps, retry gates, and expiry rules retain their definitions.
The change preserves admission statements S1 through S8 for fixed inputs.

The signature delete encodes its key and calls the raw backend delete once.
It reports the backend deletion count without a separate presence check.
The quarantine preserves damaged bytes before it attempts to remove them from the live store.
The conditional delete compares and deletes within one backend operation.
A changed live record survives, and simultaneous scans count only the actual deletion.
Restore uses conditional deletion for the inspected quarantine copy and reports actual cleanup.
The move remains two operations and can leave both copies after an interruption.

The pool-lock wait and hold histograms retire with the removed mutex.
Other metric definitions remain unchanged.

## Tests

The initial release run passed 105 block-storage unit tests.
The new regressions cover these cases:

- A scan preserves a valid replacement of a damaged live record.
- Concurrent scans move one damaged record once.
- Restore preserves changed quarantine bytes.
- Signature removal performs no presence read.
- Concurrent signature removals report one successful delete.
- A conditional-delete failure preserves both copies until retry.

Two LMDB tests passed for conditional deletion and concurrent removers.
The metric collection script test passed.
The remaining workspace and release checks continue at publication time.
The pull request records later validation results.

## Claim boundary

- [CLAIM-DEPLOY-STORE-CONCURRENCY-001](../../../../claims/deploy-store-concurrency.md) remains pending.
- [CLAIM-CASPER-FRESH-ADMISSION-CAP-001](../../../../claims/casper-fresh-admission-cap.md) retains statements S1 through S8.
- [CLAIM-CASPER-STAGE-METRICS-001](../../../../claims/casper-stage-metrics.md) records the two retired histograms.

These local results do not discharge the claims or replace the required maintainer review and hosted integration evidence.
The earlier evidence records retain their historical source identities.
