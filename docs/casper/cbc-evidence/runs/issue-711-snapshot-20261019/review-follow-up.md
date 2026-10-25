# Scope predicate capture review

Status: pending maintainer review.
This change addresses [review 5594913779](https://github.com/F1R3FLY-io/f1r3node-rust/pull/771#pullrequestreview-5594913779).
The implementation includes `origin/dev` at `f9340ed6115c5d7101c7edddc4a4400abc5881e5`.

The scope predicate now borrows the captured deploy set.
The statistics functions and selection use the same set.
The scope predicate keeps its signature membership condition.
A deploy inserted after capture does not affect this proposal.

The capture performs the existing validation and quarantine before the scope predicate runs.
Storage errors can surface before the admission calculations.
The admission rules S1 through S8 and the cleanup rules retain their definitions.
The preparation lock timer retains its sample count.
The full-read counter now includes the decode for the scope predicate.

The unit regressions check the capture boundary and quarantine before the scope predicate.
The existing integration regression asserts one full read for a populated pool.
Recovery-empty and disabled-admission regressions retain their zero-read assertions.
Local checks remain in progress when this follow-up is committed.
CI must verify the published commit.
Both related claims remain pending maintainer review.

The [extended benchmark](pool_capture_benchmark.rs) compares four reads, `any` plus one read, and the capture shared with the scope predicate.
The original benchmark source, results, and report remain historical evidence.
The benchmark measures pool scan and decode cost with in-memory storage.
It does not measure complete block creation or shard throughput.

Use the repository root as the working directory.

1. Create the `casper/examples` directory if necessary.
2. Copy `pool_capture_benchmark.rs` to `casper/examples/issue711_capture_benchmark.rs`.
3. Run the command below.

```bash
cargo run --offline --locked --release -p casper --example issue711_capture_benchmark
```

4. Remove `casper/examples/issue711_capture_benchmark.rs` after the measurement.
