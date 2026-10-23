# Issue #712 deploy storage access

The base commit is `594df581ea9a818cb5373bb3cea8dc19fcbdd31b`.
The branch is `perf/712-remove-deploy-storage-mutexes`, and the pull request target is `dev`.
Task: TASK-012-33.

## Change

The deploy store methods use shared references.
Casper and the proposer use `Arc<KeyValueDeployStorage>` without an outer mutex.
The rejected deploy buffer retains its mutex across the admission check and pool insertion.
The public `hash_set_casper` constructor retains its owned store parameter.

The raw store adds an atomic conditional delete.
LMDB compares and deletes in one write transaction.
The memory store compares and deletes under one entry lock.
Quarantine operations use this delete after they preserve a copy.
The signature delete uses the backend delete count without a preceding read.

The removed pool mutex no longer produces wait or hold metrics.
The metrics claim and the collection script record that retirement.
The admission statements S1 through S8 retain their definitions.

## Verification

Test matching, changed, and absent values for both backends.
Test concurrent inserts, concurrent deletes, and deterministic quarantine interleavings.
Test recovery, interrupted moves, backend failures, and real LMDB reopen.
Run the admission and block preparation regression tests.
Run formatting, workspace Clippy, and the relevant release suites.
Record the source hashes, commands, and results in the evidence record.

## Related work

PR #676 identified the separate mutexes and created issues #711 and #712.
PR #693 added quarantine and assumed that the pool used one shared mutex.
Issue #711 and PR #771 reduce repeated reads.
Issue #729 and PR #730 change the cost of decoding records.
Neither open pull request supplies a prerequisite commit for this branch.
If either merges first, preserve its changes when this branch updates from `dev`.

The local coordination file `docs/ToDos.md` stays outside the commit and pull request.
The implementation adds no code comments.
