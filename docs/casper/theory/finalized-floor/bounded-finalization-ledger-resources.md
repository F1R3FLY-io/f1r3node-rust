# Finalization ledger resource measurements

This document separates ledger validation, recovery retention, admission validation, migration, and metadata projection.
It supplements the [audit design](bounded-finalization-ledger-audit.md) and its formal contracts.
These measurements do not establish whole-node memory bounds, startup deadlines, or shard uptime.

The [ledger probes](../../../../block-storage/src/rust/finality/finalization_ledger/tests/resource_measurements.rs) measure enumeration, audit, retention, and migration.
The [DAG probes](../../../../block-storage/src/rust/dag/block_dag_key_value_storage/finalization_snapshot_tests/resource_measurements.rs) measure admission summaries and metadata projection.

## Measurement method

The [allocation probe](../../../../block-storage/src/allocation_probe.rs) wraps Rust's system allocator only in the block-storage library test binary.
Production builds retain their existing allocator.
The probe counts successful allocation requests and deallocations on the calling thread during a synchronous operation.
Fixture construction, database initialization, warm-up, and result printing occur outside each capture.

**Net allocation** means allocated bytes minus freed bytes during the capture.
**Peak allocation** means the largest positive net allocation during the capture.
A phase checkpoint starts a separate peak measurement relative to the allocations already present at that checkpoint.
The largest request is one allocation request, not the total live allocation.

Deallocation can release memory allocated before capture.
Cloning shared byte storage can also allocate ownership metadata retained by an input after the operation returns.
Thus, net allocation alone does not identify which result or store owns the remaining memory.
The retention tests separately drop returned recovery data and require release of its entire measured net allocation.

The probe does not measure the following resources:

- Resident set size, allocator fragmentation, thread stacks, or other threads.
- LMDB's C allocations, mapped-page residency, or kernel caches.
- Physical overlap between old and replacement buffers inside the system reallocator.
- Disk usage, elapsed-time guarantees, or the complete constructor's peak memory.

The meter tests cover allocation, zeroed allocation, reallocation, phase boundaries, unwind cleanup, and independent thread-local state.
These checks validate measurement mechanics, not the full system allocator.

## Parameters and evidence

The measurements used x86-64 Linux and `rustc 1.95.0-nightly (6efa357bf 2026-02-08)`.
The Cargo test profile was unoptimized with debug information.
Each verification scope had a 4 GiB memory limit and disabled swap.
Cargo used one build job, and the test runner used one test thread.
Other verification work ran concurrently, so this report makes no timing comparison.

| Measurement | Input parameters |
| --- | --- |
| Borrowed recovery enumeration | In-memory and LMDB backends. One committed round. Zero, eight, or 64 additional witnesses. Supporting manifests contain 256 or 4,096 generated hashes plus two required hashes. |
| Complete integrity audit | Both backends. 2, 8, 32, 33, 34, 64, 65, 66, 96, or 128 rounds. One newly finalized block per round. A 32-record page budget. |
| Maximum witness | 10,000 latest messages, 262,144 supporting hashes, 262,144 finalized hashes, and a 256-byte shard identity. These are existing limits. |
| Recovery retention and migration | Both backends. Zero, 16, 256, or 600 distinct target charges in one episode. One citer, generation zero, and shard identity `root`. |
| Admission summaries | In-memory storage. One, eight, or 32 admissions. Each target has zero, 4,096, or 65,536 extra body bytes. Two bonded validator keys. |
| Metadata projection | In-memory storage. 2, 8, or 64 unprojected rounds, with one new block per round. Separate constant and increasing fault-tolerance values. |

Each LMDB fixture uses a real environment with one named database and a 256 MiB map limit.
The map limit is not a resident-memory measurement.
The fixture removes its temporary directory after the ledger releases its environment.
Temporary databases use `target/verification/pr216/ledger-startup/`, not `/tmp`.

The 600-charge migration case represents existing recovery history above the normal 512-charge admission limit.
It does not authorize new admissions above that limit.
The maximum-witness case exercises local witness storage, not the smaller portable certificate encoding.

Admission fixtures initialize storage with height-zero genesis and supply a distinct height-100 anchor to the admission validator.
They preserve signed target and citer identities, sorted generation caches, and the anchor's certificate commitment.
These API-level fixtures do not prove approved-anchor selection or complete consensus reachability.

Evidence directories are below `target/verification/pr216/ledger-startup/`.

| Evidence directory | Result used here |
| --- | --- |
| `resource-phases.2DNS1w` | Four resource checks passed: enumeration, integrity, recovery/migration, and metadata projection. The admission fixture still failed. |
| `resource-phases.n4LGSr` | The corrected admission check, both meter checks, maximum-witness check, and strict block-storage Clippy passed. |
| `resource-phases.dA4aTE` | All five resource checks, both meter checks, maximum-witness check, and strict block-storage Clippy passed in the release profile. |

Each run captured source hashes and verified that those inputs remained unchanged during execution.
The later fixture correction changed only the admission resource test.
It did not change the four passing probes or their production paths.
These combined results do not count either earlier failed invocation as wholly successful.
The release run independently checked the final formatted source snapshot without combining partial runs.
The numerical examples below retain their cited debug-profile measurements.

## Results

All byte counts below are allocator-requested bytes with the limits described above.
Collection layout and allocation totals can vary with compiler versions, insertion order, and backend implementation.
The regression assertions cover the relevant relationships instead of treating every observed total as a protocol constant.

### Enumeration and audit

Borrowed enumeration retained no allocation on either backend at all tested witness counts and payload sizes.
The in-memory backend's peak was 48 bytes, with 3,072 cumulative allocated bytes from iterator work.
LMDB enumeration made no Rust allocation in the captured operation.
Neither result means that the backend or mapped database uses no memory.

Both audit backends had a 4,777-byte peak through 33 rounds and a 4,945-byte peak from 34 through 128 rounds.
Every complete audit returned with zero net allocation.
Cumulative allocation still grew with the complete history.
For example, the 128-round totals were 991,348 bytes in memory and 1,000,984 bytes with LMDB.

The 168-byte peak difference comes from one retained record-derived head during a later multi-record page.
That head owns three shared 32-byte hashes, including their shared ownership allocations.
A directly deserialized head does not initially have the same ownership representation.
The regression measures the production `record_head` path instead of substituting direct head deserialization.

The page keeps its previous validated head until every record in that page succeeds.
During the second new record of a later page, the candidate head no longer shares that previous head's hashes.
A one-record second page does not create the same overlap.
The 32/33/34 and 64/65/66 cases check these boundaries without removing rollback safety.

Maximum-witness decoding retained 52,324,240 bytes from a 22,102,208-byte encoded value.
Dropping the decoded result released the full retained allocation.
Standalone validation allocated 130,410,092 bytes cumulatively, with a 21,635,170-byte additional peak and zero net allocation.
These are separate measurements, not one measured combined peak.
The source already retains the decoded witness when validation begins.
The small-manifest audit peaks must not be used as bounds for maximum-size witnesses.

### Recovery and migration

Returned recovery data retained the same allocation on both backends.

| Charges | Retained bytes |
| --- | ---: |
| 16 | 6,948 |
| 256 | 107,988 |
| 600 | 347,788 |

Capacity growth explains why retained allocation need not scale smoothly between sample sizes.
Each result released its measured allocation when dropped.
Recovery strings remain unrestricted by the witness shard limit, so their physical lengths remain resource parameters.

For 600 charges, migration preparation retained 580,576 bytes before transaction publication on both backends.
Publication added a 291,260-byte peak in memory and an 8,533-byte Rust allocation peak with LMDB.
The corresponding complete migration peaks were 871,836 and 589,109 bytes.
LMDB publication also uses resources outside this Rust-only probe.
These measurements preserve the distinction between prepared mutations, transaction staging, and retained recovery results.

### Admission summaries

Summary retention was independent of target body size for each admission count.

| Admissions | Retained summary-phase bytes at all three body sizes |
| --- | ---: |
| 1 | 585 |
| 8 | 2,888 |
| 32 | 11,504 |

Validation still reads, decodes, and verifies each complete target and citer.
For 32 admissions, validation-only peak allocation rose from 12,301 bytes with empty payloads to 208,526 bytes with 65,536-byte payloads.
Validation returned with zero net allocation in both cases.
Compact summaries remove retained block bodies, not their required validation work.

### Metadata projection

Let $`R`$ be the number of rounds and $`i`$ the current round, starting at one.
The constant case uses fault tolerance one.
The increasing case uses:

```math
\mathrm{FT}(i)=\frac{R+1+i}{2(R+1)}.
```

Each witness records the exact numerator and denominator.
Its ledger record contains the corresponding single-precision value.
The tests check the final head, last finalized block, and unchanged head after immediate retry.
They do not recompute a Casper voting certificate from this synthetic schedule.

At 64 rounds, constant fault tolerance allocated 968,184 bytes cumulatively, with a 22,537-byte peak.
Increasing fault tolerance allocated 28,735,652 bytes cumulatively, with a 384,719-byte peak.
Immediate retry allocated 116 bytes cumulatively, retained zero bytes, and had a 100-byte peak in both cases.

`propagate_ft_to_finalized_blocks` skips a scan when the new value does not exceed its cached lower bound.
An increasing value instead scans finalized metadata and raises lower values.
Repeated increases can therefore cause repeated whole-finalized-set work.
This existing path explains the measured difference and remains separate from the ledger audit's working-memory bound.
This report does not establish that this path caused a historical soak failure.

## Reproduction

Run each command from the repository root.
Capture command output outside `/tmp`.
Do not run all scopes concurrently without checking their combined memory limits.

```bash
systemd-run --user --scope -p MemoryMax=4G -p MemorySwapMax=0 \
  env CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1 \
  cargo test -p block-storage --lib allocation_probe_

systemd-run --user --scope -p MemoryMax=4G -p MemorySwapMax=0 \
  env CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1 \
  cargo test -p block-storage --lib resource_probe -- --nocapture

systemd-run --user --scope -p MemoryMax=4G -p MemorySwapMax=0 \
  env CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1 \
  cargo test -p block-storage --lib \
  bounded_decoder_accepts_the_maximum_writer_witness_and_checks_all_collection_limits -- --nocapture
```

The general test suite also discovers these tests.
Add `--release` after `cargo test` to reproduce release-profile qualification.
Other architectures require separate measurements before numerical comparison.
