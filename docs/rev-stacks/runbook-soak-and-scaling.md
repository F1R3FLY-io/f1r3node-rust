# Runbook: the isolated-host soak and the 8- and 16-core scaling run

Decision 61 gives these two runs to the team. Start them after the FX fixes land on the integration branch. This runbook tells how to run them and what to record.

Both runs measure one commit. Use the same commit for both runs, and record its full commit ID in each report.

## 1. Preconditions

1. Make sure that the FX fixes are on the commit under test.
2. Make sure that the full suites pass on that commit: `rspace_plus_plus`, `casper --lib`, `casper --test mod` and `node`.
3. Make sure that `scripts/v6/cli-end-to-end.sh` passes on that commit. This script is the M2 gate.
4. Use a host that runs no other work during the run. Do not use a shared runner or a laptop.
5. Record the host: CPU model, number of physical cores, memory, disk type and kernel version.

The toolchain is the one in `rust-toolchain.toml`. If that nightly is not available, use stable with `RUSTC_BOOTSTRAP=1`, and record that in the report.

## 2. The scaling run (8 and 16 cores)

### 2.1 What the run measures

The run executes one block of one-send envelopes with 1, 2, 4, 8 and 16 executor workers. The test `v6::exit_spec::scaling_by_worker_count` reports the execution time of the block for each worker count. Execution is the gate, the runs, the merge and the settlement. The close-block system deploy is not included, because it does not depend on the number of envelopes.

The test calculates two values for each worker count `n`:

- speedup `S(n)` = throughput with `n` workers / throughput with 1 worker
- parallel efficiency `η(n)` = `S(n) / n`.

The target from P2-13 is `η ≥ 0.6` at 8 cores and at 16 cores.

### 2.2 Host settings

1. Set the CPU frequency governor to `performance` on every core.
2. Disable swap for the duration of the run.
3. Stop all services that are not necessary for the run.

### 2.3 Build

1. Build the test binary in release mode:

   ```bash
   cargo test --release -p casper --test mod --no-run
   ```

2. Build the node in release mode for section 3:

   ```bash
   cargo build --release -p node
   ```

### 2.4 Run on 8 cores

On a host with 16 or more cores, use cores 0 to 7 only. On a host with 8 cores, omit `taskset`.

```bash
V6_SCALING_WORKERS=1,2,4,8 V6_SCALING_ENVELOPES=256 \
TOKIO_WORKER_THREADS=8 RAYON_NUM_THREADS=8 \
taskset -c 0-7 \
cargo test --release -p casper --test mod -- \
  --ignored --nocapture --test-threads=1 v6::exit_spec::scaling_by_worker_count
```

### 2.5 Run on 16 cores

```bash
V6_SCALING_WORKERS=1,2,4,8,16 V6_SCALING_ENVELOPES=256 \
TOKIO_WORKER_THREADS=16 RAYON_NUM_THREADS=16 \
taskset -c 0-15 \
cargo test --release -p casper --test mod -- \
  --ignored --nocapture --test-threads=1 v6::exit_spec::scaling_by_worker_count
```

Start each `V6_SCALING_WORKERS` list with 1. The test uses the first entry as the single-worker reference.

### 2.6 Phase profile

Run the phase benchmark once on each core count. It shows which phase limits the scaling.

```bash
TOKIO_WORKER_THREADS=8 RAYON_NUM_THREADS=8 taskset -c 0-7 \
cargo test --release -p casper --test mod -- \
  --ignored --nocapture --test-threads=1 v6::exit_spec::phase_timings_by_block_size
```

This benchmark uses two executor workers. Its result is a phase breakdown, not a scaling result.

### 2.7 Record

Put the results in `docs/rev-stacks/decisions/P3-06.md`, in a new section "Scaling run". For each core count, record these items:

| Item | Source |
|---|---|
| One output line for each worker count | `scaling_by_worker_count` |
| η(8) and η(16) | `scaling_by_worker_count` |
| The four lines of the phase benchmark | `phase_timings_by_block_size` |
| The host description | Section 1, step 5 |

### 2.8 How to read the result

The first result on a 2-core host (P3-06.md, "First scaling result on this host") shows that `reset` limits the scaling. Run the scaling run after that limit is removed, or record it as the known cause.

If `η` is less than 0.6, compare two values in the phase benchmark: the run wall time and the summed setup time. P3-06 found that the setups of different workers do not overlap. If the wall time is near the summed setup, `RhoRuntime::reset` is the limit. P3-06.md, section "The next cost", describes a possible fix.

If the merge or the settlement is more than half of the execution time, the limit is the sequential part of the block. Record the share of each phase.

## 3. The isolated-host soak

### 3.1 What the soak confirms

| Item | Source | Criterion |
|---|---|---|
| The last finalized block (LFB) never stalls. | P0-07 | The LFB advances in every load phase of every iteration. |
| The finalizer limits | P0-07, decision 5 | No cycle reaches `finalizer-timeout-reruns` (3) or `finalizer-cursor-max-invalidations` (4). |
| The deploy pool cap and the proposal pacing | P0-09, decision 3 | The pool never holds more than `deploy-pool-blocks-of-backlog` × `max-user-deploys-per-block` deploys. |
| The pool read time on LMDB | P0-10 | The prepare time stays bounded when the pool is at its cap. |
| Conservation on v6 | P2-09, Decision 57 | The counter `v6.conservation.audit.failures` stays at 0. |
| Record replay on v6 | P1-04 | No log line contains `v6 record replay mismatch`. |
| A-LIN on v6 | P1-10 | No log line contains `A-LIN violation`. |

### 3.2 Two configurations

Run the soak two times on the same host:

1. **Protocol 1.** The shard keeps `protocol-version = 1`. This run covers P0-07, P0-09 and P0-10 on the protocol that the soak harness already loads.
2. **Protocol 6.** The shard has `protocol-version = 6`. This run covers the v6 rows of the table in 3.1.

### 3.3 Protocol 1: through the soak workflow

1. Get a dedicated OCI host for the full window. Do not let a scheduled soak use the same host.
2. Dispatch the merge-recovery soak against the commit under test:

   ```bash
   scripts/restart-soak.sh --series daily --target-ref <commit> --hours 22 --dry-run
   ```

3. Read the printed command. If it is correct, run it again without `--dry-run`.
4. A restarted run is excluded from the baseline. This is correct for this soak, because the commit is not on `dev`.

`docs/soak-benchmarks.md` describes the iterations, the phases and the dashboard. The gates in `scripts/bench/soak-gate-thresholds.json` apply.

### 3.4 Protocol 6: by hand

The external soak harness can possibly not set the genesis protocol version. If it can, use the workflow of 3.3 with `protocol-version = 6`. If it cannot, use this procedure.

1. Copy `docker/conf/default.conf` to a work directory.
2. In the copy, add `protocol-version = 6` in the block `genesis-block-data`.
3. Seed the genesis files. Put one line `<address>,<amount>` for each payer in `wallets.txt`. Put the validators in `bonds.txt`.
4. Change the volume `./conf/default.conf` in a copy of `docker/shard.yml` to point to the copy of step 1.
5. Start the shard:

   ```bash
   docker compose -f <copy of shard.yml> up -d
   ```

6. Make sure that every node logs the protocol-6 genesis. The approved block header has version 6.
7. Send load for the full window. Use the deploy and transfer pattern of `scripts/v6/cli-end-to-end.sh`. Send from at least three payers to at least three validators.
8. Every hour, record the LFB height of each node, the deploy pool size and the two conservation counters.
9. At the end of the window, collect the logs of every node. Search them for the texts in table 3.1.
10. Stop the shard:

    ```bash
    docker compose -f <copy of shard.yml> down
    ```

Use a window of at least 22 hours, the length of a dev integration soak.

### 3.5 Record

Put the results in `docs/rev-stacks/decisions/P0-07.md`, in a new section "Isolated-host soak". For each configuration, record these items:

- the commit ID, the host and the window
- the number of iterations and the number of failures
- the LFB height of each node at each hourly sample
- the largest deploy pool size
- the prepare time at the largest pool size
- the final values of `v6.conservation.audit.runs` and `v6.conservation.audit.failures` (protocol 6 only)
- each log line that matches a text in table 3.1.

### 3.6 If a criterion fails

1. Keep the shard data and the logs. Do not restart the shard.
2. Record the time, the node and the last LFB height.
3. Report the failure to the lead with the patch ID from the column "Source" of table 3.1.
