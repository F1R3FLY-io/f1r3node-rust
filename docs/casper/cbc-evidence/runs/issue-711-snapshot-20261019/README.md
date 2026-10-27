# Issue #711 deploy pool snapshot evidence

Status: pending maintainer review.
The base commit is `1ea5336848c392d73ee3cd831ffae34b1d3306c5`.
The source hashes in `report.json` identify the tested implementation and regression tests.
The [review follow-up](review-follow-up.md) records the scope predicate change after this original measurement.

This record supports the preservation review for these pending claims:

- [CLAIM-CASPER-FRESH-ADMISSION-CAP-001](../../../../claims/casper-fresh-admission-cap.md), statements S1 through S8.
- [CLAIM-CASPER-STAGE-METRICS-001](../../../../claims/casper-stage-metrics.md), the existing metric definitions.

## Change and preservation argument

The scope predicate uses `any` with the same signature membership condition.
The two statistics functions borrow one decoded pool set, and selection consumes that set.
The candidate predicates, admission functions, expiry rules, canonical checks, retry gates, and byte budgets retain their definitions.
Selection still uses the existing deploy order before it applies the caps.
The preparation lock retains its existing metric samples and its pool-before-buffer lock order.

The captured pool set defines the inputs for statistics and selection in one proposal.
A later insert stays in storage for the next proposal.
Store removals still use signatures, and preparation does not reinsert a record removed after capture.
This snapshot does not create a transaction across the deploy pool, admission, and the rejected buffer.

## Regression evidence

The focused release run passed all 72 `block_creator` unit tests.
This run includes the existing admission and metric tests and the following new tests:

- `storage_scope_check_stops_before_a_later_damaged_record` checks early termination and quarantine on a later complete scan.
- `statistics_and_selection_share_the_pool_capture_before_a_later_insert` checks candidate classification, capture consistency, cleanup, and the next capture.

The focused integration run passed all 15 `block_creator_spec` tests.
The test `should_remove_block_expired_deploys_while_keeping_valid_ones` now asserts one pool read through the decoded counter.
The new test `recovery_empty_and_disabled_ordinary_admission_skip_full_pool_reads` asserts zero decoded pool records for both paths.
Both counter checks use populated storage, so a redundant full read changes the observed count.

The file `report.json` records completed checks and pending checks before the commit.
The full release suite remains in progress at that record time.
The pull request records later validation results.

## Measurement scope

The [measurement source](pool_read_benchmark.rs) compares the four previous read calls with `any` followed by one full read.
The benchmark uses the same encoded records and release binary for both sequences.
It uses in-memory storage, a short `Nil` payload, a warm cache, and three repetitions for each case.
The two cases make every signature match or make every signature fail the scope predicate.

The measurement isolates pool scan and decode cost.
It does not measure complete block preparation, execution, shard throughput, or finalization.
The benchmark does not include PR #730, which remains independent of this refactor.

The values below are mean milliseconds for one call sequence across three repetitions.

| Pool size | Scope match | Four full reads | One full read and `any` | Ratio |
| --- | --- | --- | --- | --- |
| 2,100 | First record | 949.597 | 230.970 | 4.111 |
| 2,100 | No record | 918.639 | 445.028 | 2.064 |
| 15,100 | First record | 7,111.968 | 1,695.453 | 4.195 |
| 15,100 | No record | 6,845.367 | 3,299.509 | 2.075 |

Each new sequence performs one `read_all` call.
When no signature matches, `any` also decodes the complete pool.
This extra scan explains the smaller ratio.

The legacy counter also counts decode attempts in `any` scans.
Subtracting that counter from the full-read counter cannot isolate the signature checks of either scan type.

Use the repository root as the working directory.

1. Create the `casper/examples` directory if necessary.
2. Copy the measurement source to `casper/examples/issue711_read_benchmark.rs`.
3. Run this command:

```bash
cargo run --offline --locked --release -p casper --example issue711_read_benchmark
```

4. Remove `casper/examples/issue711_read_benchmark.rs` after the measurement.

## Review boundary

Both claim statuses remain pending.
These local tests do not discharge the claims or replace the required load, soak, and maintainer acceptance evidence.
The older retry packaging waiver remains a separate evidence record.
