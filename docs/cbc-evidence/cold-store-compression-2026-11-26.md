# Cold-Store Compression Validation

Date: 2026-11-26.
Status: evidence prepared; maintainer acceptance pending.
Claim: [CLAIM-RSPACE-COLD-COMPRESSION-001](../claims/rspace-cold-store-compression.md).
Branch: `improvement-proposals/storage-only-compression`.

## Formal Checks

The [focused runner](../../scripts/ci/check-cold-store-compression.sh) passed with TLC 1.7.4 and Coq 8.16.1.
The TLC artifact SHA-256 is `936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88`.

| Check | Observed result |
| --- | --- |
| Positive model | 15,239 generated states; 1,204 distinct states; no remaining states; no violated invariant |
| Five negative controls | Each returned exit 12, its named invariant, and a counterexample |
| Coq compilation | All 16 theorems compiled |
| `coqchk` | All modules passed kernel checking |
| Assumption audit | All 16 reports were closed under the global context |
| Formal-gate regression | Positive paths and 51 exact-exit refusal controls passed |
| TLC-gate regression | All 103 registered negative controls classified correctly; routing and registration checks passed |

The [TLA+ scope](../../formal/tlaplus/cold_store_compression/README.md) and [Coq scope](../../formal/rocq/cold_store_compression/README.md) define the abstractions and trusted components.
The [formal report](runs/cold-store-compression-20261126/formal/report.json), [source manifest](runs/cold-store-compression-20261126/formal/sources.sha256), [kernel log](runs/cold-store-compression-20261126/formal/coqchk.log.gz), and [assumption audit](runs/cold-store-compression-20261126/formal/assumptions.log) preserve the executed evidence.
The complete repository Coq gate did not finish on this host.
It stopped in the existing `node_observation/b11` project because Coq 8.16.1 cannot resolve that project's newer `Stdlib` imports.
The focused codec project passed independently. No unrelated proof source was changed to bypass the tool-version mismatch.
The Coq model assumes a correct physical-to-canonical decoding boundary; it does not implement LZ4.
An axiom-free abstract proof is not a complete proof of the Rust adapter.
No liveness or power-loss guarantee follows from these results.

## Rust Binding

The library run passed 175 tests, with one existing ignored test.
All five cold-store integration tests passed.
Two new property tests used 128 generated cases each:

- `formal_store_model_agrees_with_operation_traces` compares reads, writes, conditional inserts, deletes, and complete canonical maps after each operation.
- `formal_budget_model_agrees_with_reservation_traces` compares admission decisions, active counts, and reserved bytes after acquisition and release.

The existing deterministic race tests check representation replacement and different-value preservation during conditional deletion.
The [integration suite](../../rspace++/tests/cold_store_compression.rs) compares canonical roots, exports, imported state, LMDB reopen behavior, and replay effects.
RSpace Clippy checks passed with warnings denied.
The [Rust test log](runs/cold-store-compression-20261126/rust-binding.log) preserves the successful binding and integration run.

## Disposable Shard Procedure

The [live runner](../../scripts/validation/cold-store-compression.sh) builds a separate validation image from the newly compiled node and a local runtime image.
It uses a unique internal Docker network, private generated test keys, and isolated bind-mounted state below `target/cold-store-validation`.
It does not publish HTTP ports or attach to the existing shard's network or volumes.
The [Rust client](../../node/examples/cold_store_validation.rs) sends requests through `docker exec` and checks finalized state at a common block.

The executed sequence was:

1. Complete genesis with three bonded validators. Validator 1 writes raw rows; validators 2 and 3 write compressed continuations.
2. Finalize a large persistent contract, then 100 counter deploys and 100 lock-record deploys distributed across the validators.
3. Start a late observer with compressed writes enabled. Require the same finalized block and state root on all validators and the observer.
4. Query the observer at that common block. Require exact totals of 100 increments, 100 locks, and 100,000 locked units.
5. Restart validator 2 with compressed writes disabled, then repeat the root comparison and observer query.
6. Check continued finalization, container exits, OOM state, and automatic restart counts.
7. Stop the disposable nodes, inspect their cold databases read-only, then remove only the owned containers and network.

These lock records exercise persistent continuations and shared mutable state.
They do not implement the ASI bridge, a token purse, or cross-chain settlement.
Validators keep their normal restriction on exploratory execution.
Direct tuple-state queries run only on the read-only observer; validator agreement is checked through the finalized block and post-state root.
Each container has a one-CPU quota and a 4 GiB memory limit.
The host kernel cannot enforce Docker swap limits; a strict per-container swap bound is not claimed.

## Live Results

### Clean End-to-End Confirmation

The corrected [combined harness completed with exit code zero](runs/cold-store-compression-20261126/clean-run/report.json) on fresh run `shard.OYdJh3`.
This was a separate genesis and namespace, not a resubmission to the earlier contract or the existing shard.
All 100 counter deploys and 100 lock-record deploys finalized in 206.610 seconds, including initialization, submission, and polling.
The [receipts](runs/cold-store-compression-20261126/clean-run/finalization-receipts.json) account for all 201 expected signatures.

The late-observer and write-disabled restart checks passed.
Finalization then advanced from the recovery-check height of 58 to height 68.
A further [common-block state check](runs/cold-store-compression-20261126/clean-run/final-verification.json), after the 60-second observation period, still returned exactly `[100, 100, 100000]`.
All five nodes ended at finalized height 68 and DAG height 71, with no OOM kill or automatic restart.
The offline inventories passed within the same command, and cleanup removed the disposable containers and network.
The [clean-run source manifest](runs/cold-store-compression-20261126/clean-run/sources.sha256) matches the current code and validation tools.

### Initial Validated Workload

The [live report](runs/cold-store-compression-20261126/live/report.json) records run `shard.cmP4X5`.
The node and client source hashes still matched after the run.
All 100 counter deploys and all 100 lock-record deploys finalized.
Initialization and both phases took 215.845 seconds, including submission and polling overhead.
The [finalization receipts](runs/cold-store-compression-20261126/live/finalization-receipts.json) contain exactly the 201 expected signatures, including initialization.
This is a correctness run, not a throughput or latency benchmark.

The late observer and all three validators agreed on the finalized block and post-state root at height 58.
The observer returned `[100, 100, 100000]` at that block.
After validator 2 restarted with compressed writes disabled, all four nodes agreed again at height 59 and the observer returned the same values.
The [joiner result](runs/cold-store-compression-20261126/live/joiner-verification.json) and [restart result](runs/cold-store-compression-20261126/live/restart-verification.json) preserve those observations.

All five nodes subsequently reached finalized height 79 and DAG height 81.
Each reported ready with four peers, no OOM kill, and zero automatic restarts.
All five containers then stopped with exit code zero.
The disposable containers and network were removed.
The existing `rnode.*` shard was not restarted or reconfigured.

Docker memory samples for the three validators reached 288.8, 323.7, and 347.3 MiB respectively.
The observer's single final sample was 452.6 MiB; its peak was not measured.
The [resource record](runs/cold-store-compression-20261126/live/resources.json) includes sample counts and CPU observations.
These are sampled Docker memory readings, not process RSS peaks or evidence against long-running leaks.

### Offline Inspection

The original harness exited with code 1 after the live checks, during offline inspection.
The inspector dropped the read-only transaction used to open a named LMDB database.
Later reads then returned `EINVAL` because the database handle had not been published to the environment.
The diagnostic fix commits that read-only opening transaction before scanning.
It does not change node storage behavior.

A dedicated regression failed before this fix and passed after it.
The [red log](runs/cold-store-compression-20261126/inventory-red.log) and [green log](runs/cold-store-compression-20261126/inventory-green.log) preserve both outcomes.
The corrected inspector then checked the same stopped databases successfully.
Checksums before and after inspection were identical for all four `data.mdb` files.
The recorded live result and successful offline retry are separate; no clean exit is claimed for the original combined command.
The subsequent clean run above confirmed the corrected combined command end to end.
The earlier source manifest remains a record of that earlier harness version; use the clean-run manifest for the current tools.

| Store | Rows | Compressed rows | Canonical row bytes | Physical row bytes | Live LMDB page bytes |
| --- | ---: | ---: | ---: | ---: | ---: |
| Validator 1, raw writes | 4,449 | 0 | 15,970,232 | 15,970,232 | 18,673,664 |
| Validator 2, compressed then raw writes | 4,453 | 1,890 | 16,046,608 | 4,286,550 | 6,516,736 |
| Validator 3, compressed writes | 4,449 | 1,986 | 15,970,232 | 3,876,979 | 5,926,912 |
| Late observer, compressed writes | 5,495 | 2,570 | 15,254,737 | 4,560,197 | 7,012,352 |

These storage measurements are from the initial run.
For validator 3, physical row encoding used 75.7% fewer bytes than its canonical rows.
The [inventory](runs/cold-store-compression-20261126/live/validator3-cold-inventory.json) also records file length and filesystem allocation.
Row bytes include outer key/value wrappers but exclude LMDB pages and free space.
Histories and retained rows can differ between nodes, so cross-node file sizes are not a controlled migration comparison.
The persistent test contract contains deliberately repetitive data. These savings are not a general workload estimate.

Earlier disposable attempts stopped on harness configuration, transport, timestamp, or query-interface errors.
Each attempt used fresh genesis state and a different generated namespace.
No workload was resubmitted into the existing shard.
The timestamp allocator now has a [concurrent uniqueness regression](runs/cold-store-compression-20261126/client-tests.log).

## Reproduction

Build with the repository toolchain and a compatible protoc:

```bash
cargo build --locked --release -p node --bin node --example cold_store_validation -j 2
cargo build --locked --release -p rspace_plus_plus --example cold_store_inventory -j 2
TLA_TOOLS_JAR=/absolute/path/tla2tools.jar \
  scripts/ci/run-tests.sh bash scripts/ci/check-cold-store-compression.sh
scripts/ci/run-tests.sh cargo test --locked -p rspace_plus_plus --lib --test cold_store_compression -j 2
scripts/ci/run-tests.sh cargo test --locked -p rspace_plus_plus --example cold_store_inventory -j 2
scripts/ci/run-tests.sh cargo test --locked --release -p node --example cold_store_validation -j 2
COLD_COMPRESSION_BASE_IMAGE=LOCAL_NODE_RUNTIME_IMAGE \
  scripts/ci/run-tests.sh bash scripts/validation/cold-store-compression.sh
```

This host used protoc 27.3 because its system protoc does not support the existing proto3 optional fields.
The local image must supply the dependencies described in the [node Dockerfile](../../node/Dockerfile).
The validation image also copies the host binary's OpenSSL 1.1 libraries when required.
It is a local test artifact, not a release image.

Private run directories retain generated keys, database state, logs, metrics, and binary/source manifests for investigation.
Do not commit or publish those directories.
Only selected public evidence belongs in the repository.
The cleanup trap stops and removes the disposable containers, including after a failed assertion.

## Acceptance Limits

The [claim record](../claims/rspace-cold-store-compression.md) lists the remaining property-level proof obligations.
No prior CbC acceptance or waiver is extended to this implementation.
The history-repository integration and changed gate registrations still need current source-bound acceptance.
Production rollout, failure-injection testing, and representative performance measurements remain separate requirements.
Compressed writes remain disabled by default.
