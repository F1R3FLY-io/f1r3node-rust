# Neutral Cloud Benchmark Review Guide — F1R3FLY Rust Node

_Date prepared: 2026-07-06_

## Purpose

This document gives a partner engineering team a review and benchmark plan for the F1R3FLY Rust node `dev` branch of `F1R3FLY-io/f1r3node-rust`, using the cloud / integration-test patterns already documented in:

- `F1R3FLY-io/f1r3node-rust`, branch `dev`
- `F1R3FLY-io/system-integration`, branch `main`
- `f1r3node-rust/docs/vps-cloud-testing.md`
- `system-integration/integration-tests/README.md`

The intended audience is a partner engineering team that wants to reproduce a distributed shard, run integration tests, and benchmark latency / throughput / resource usage on the cloud provider of its choice. The workflow is provider-neutral: the deploy, status, teardown, and image-transfer scripts are SSH-based and work against any Linux hosts, on any cloud or on-premises.

---

## 1. Current dev baseline

The `dev` branch currently exposes the Rust node workspace as a standalone Cargo/Docker setup. The repository README describes the Rust node as a pure Rust implementation of the F1R3FLY blockchain node with:

- Concurrent smart contract execution using Rholang and RSpace
- Proof-of-stake consensus and finalization through the `casper` crate
- gRPC and HTTP APIs for deploys, proposals, status, and data queries
- Docker and local standalone workflows for development and testing

The workspace includes these main crates:

| Crate | Review relevance |
|---|---|
| `node` | Main binary, CLI, API servers, REPL, diagnostics |
| `casper` | Consensus engine, block processing, genesis, finalization |
| `rholang` | Rholang interpreter and CLI |
| `rspace++` | Tuple-space storage and state management |
| `models` | Protobuf and gRPC models |
| `crypto` | Keys, signatures, hashes, TLS helpers |
| `comm` | P2P networking, peer discovery, TLS transport |
| `block-storage` | Block, deploy, DAG, finality persistence |
| `shared` | Common storage traits, event helpers, metrics utilities |
| `graphz` | Graph and DOT helpers |

The branch also documents Docker workflows for:

- Standalone single-node operation
- Multi-validator shard operation: bootstrap + 3 validators + observer + Prometheus + Grafana
- Pulling a prebuilt public registry image
- Overriding the compose image through `F1R3FLY_IMAGE`

Security note: the upstream README states that the codebase has not completed a production security audit. Partner teams should treat these runs as benchmark / evaluation runs, not production-value deployments.

---

## 2. Benchmark baseline: the `dev` branch

The `dev` branch is the benchmark baseline. All in-flight development work targeting `dev` is merged before handoff, so the branch represents a single consolidated baseline rather than a set of optional feature candidates — no per-change selection or separate benchmark branch is required. What matters for reproducibility is recording exactly what was benchmarked: capture the exact commit hash of `dev` and the Docker image digest for every run (see section 5.9), and record the consolidated feature set in `docs/benchmark/BRANCH_CONTENTS.md` (see section 7).

---

## 3. Benchmark goals

The partner team should be able to answer four questions:

1. **Does the shard start, finalize genesis, and advance past block 0 under distributed inter-host networking?**
2. **What are the submit-to-finalize latency percentiles under controlled deploy rates?**
3. **What sustained deploy throughput can the shard process before finalization latency or error rate becomes unacceptable?**
4. **What CPU, memory, disk, and network resources are consumed at each load level?**

Recommended primary metrics:

| Metric | Definition | Source |
|---|---|---|
| Submit count | Number of deploys submitted during test window | `load-summary.txt`, benchmark script output |
| Finalized count | Number of submitted deploys finalized | `load-summary.txt` |
| Error count | Failed deploy submissions or failed finalization observations | `load-summary.txt` |
| Observed throughput | Finalized deploys / test duration | `load-summary.txt` |
| p50 / p95 / p99 latency | Submit-to-finalize latency percentiles | `latency-report.txt` and raw latency files |
| Propose time | `propose_core_ms` percentiles | `casper-profile.txt` |
| Replay time | `block_replay_ms` percentiles | `casper-profile.txt` |
| Finalizer cycle time | `finalizer_cycle_ms` percentiles | `casper-profile.txt` |
| Container CPU / memory | Peak and average usage by node | `--monitor`, Docker stats, Prometheus/Grafana |
| Chain health | Peers, nodes, shard ID, last finalized block | `/api/status`, `shardctl status` |

---

## 4. Cloud testbed setup

The Rust node documentation (`docs/vps-cloud-testing.md`) defines three testbed paths:

| Path | Use case |
|---|---|
| Local Docker | Fast functional verification and reproduction |
| Bring-your-own hosts (any cloud) | Real inter-host networking on any provider — cloud, bare metal, or colo |
| Oracle Cloud (automated) | Optional scripted provisioning/teardown for teams that happen to use OCI |

For a provider-neutral benchmark, use the bring-your-own-hosts path: provision two or more Linux VMs on the cloud of your choice, then drive them with the provider-agnostic scripts (`scripts/remote/deploy.sh`, `status.sh`, `teardown.sh`, `image-transfer.sh`). The only OCI-specific pieces are the optional provisioning helpers (`oci-provision.sh`, `oci-destroy.sh`); everything downstream of provisioning is identical across providers.

Host requirements (from the bring-your-own-hosts path):

- Two Linux hosts reachable over SSH with key-based auth, Docker and the compose plugin installed, SSH user in the `docker` group
- Firewall open between the hosts and the administrator IP on `tcp:22` and `tcp/udp:40400-40455`
- Public IPs (or DNAT arranged) for host-to-host traffic on the `40400-40455` band

### Expected topology

Default distributed testbed:

- 1 shard
- Host 1: bootstrap node
- Host 2: validators + observer
- Ports `40400-40455` opened between the hosts and the administrator IP
- HTTP API on each node's `40403`-style port band
- Optional Prometheus + Grafana monitoring

Expected node roles:

| Role | Purpose |
|---|---|
| Bootstrap | Coordinates genesis ceremony; not a validator |
| Validator 1 / 2 / 3 | Bonded validators that produce and finalize blocks |
| Observer | Read-only follower |

Verification invariants before benchmarking:

- Genesis completes.
- All expected validators sign genesis.
- Block finalization advances beyond block 0.
- `/api/status` returns expected peer and node counts.
- No unexpected container restarts occur during the warmup period.

---

## 5. Recommended benchmark workflow

### 5.1 Clone and install integration tooling

```bash
git clone https://github.com/F1R3FLY-io/system-integration.git
cd system-integration
poetry install --with integration
```

Prerequisites:

- Python 3.10+
- Poetry
- Docker and Docker Compose
- CLI / console access to the chosen cloud provider
- Access to the benchmark node image or branch build

### 5.2 Select the benchmark image

If a prebuilt benchmark image is published:

```bash
export F1R3FLY_NODE_IMAGE=<registry>/<repo>/f1r3fly-rust:neutral-cloud-benchmark
```

If using the Rust node repo's compose setup directly, the image override is:

```bash
export F1R3FLY_IMAGE=<registry>/<repo>/f1r3fly-rust:neutral-cloud-benchmark
```

Use one image tag per benchmark run. Record the exact image digest:

```bash
docker image inspect "$F1R3FLY_NODE_IMAGE" --format '{{index .RepoDigests 0}}'
```

### 5.3 Provision hosts

Provision two Linux VMs on the chosen provider (console, Terraform, Ansible — whatever the team already uses), meeting the host requirements in section 4. Then record the host IPs in the state file the deploy scripts read:

```bash
cat > scripts/remote/testbed-state.json <<EOF
{
  "vps1_public_ip": "203.0.113.10",
  "vps2_public_ip": "203.0.113.20"
}
EOF
export KEY_FILE=~/.ssh/my_testbed_key
export SSH_USER=ubuntu        # or opc, debian, root, etc.
```

(Teams on OCI may instead use `scripts/remote/oci-provision.sh`, which provisions the VMs and writes `testbed-state.json` automatically.)

### 5.4 Transfer or pull the benchmark image

```bash
./scripts/remote/image-transfer.sh --apply
```

Or, if using a registry accessible from both hosts, pull directly on each host.

### 5.5 Deploy the shard

```bash
./scripts/remote/deploy.sh --apply
```

Check status:

```bash
./scripts/remote/status.sh
```

Expected:

- Bootstrap reachable
- Validators reachable
- Observer reachable
- Finalized block height advances after genesis

### 5.6 Run integration tests before benchmarks

From `system-integration`:

```bash
poetry run pytest integration-tests/test/tests/shared/ -v --monitor
```

For a broader run:

```bash
poetry run pytest -n 3 --dist=loadgroup \
  integration-tests/test/tests/shared/ \
  integration-tests/test/tests/custom/ \
  integration-tests/test/tests/standalone/ \
  --monitor --instafail --maxfail=10
```

Use `--keep-on-failure` when debugging:

```bash
poetry run pytest integration-tests/test/tests/shared/ -x -v -s --keep-on-failure
```

Cleanup preserved shards:

```bash
poetry run shardctl test-reset
```

### 5.7 Run baseline latency benchmark

From the Rust node repository:

```bash
./scripts/bench/latency-benchmark.sh \
  --host $(jq -r .vps1_public_ip scripts/remote/testbed-state.json) \
  --duration 120 \
  --rate 5 \
  --apply
```

Benchmark output directory:

```text
/tmp/f1r3fly-bench-<timestamp>/
```

Expected files:

- `load-summary.txt`
- `latency-report.txt`
- `casper-profile.txt`
- `submits.tsv`
- `finals.tsv`
- `latencies.raw`

### 5.8 Run a load sweep

Use a staged sweep rather than jumping directly to high load.

| Run | Duration | Rate | Purpose |
|---:|---:|---:|---|
| 1 | 60s | 1 deploy/s | Smoke benchmark |
| 2 | 120s | 2 deploy/s | Baseline |
| 3 | 120s | 5 deploy/s | Moderate load |
| 4 | 180s | 10 deploy/s | Stress threshold |
| 5 | 300s | highest stable rate | Sustained run |

Suggested command pattern:

```bash
for RATE in 1 2 5 10; do
  ./scripts/bench/latency-benchmark.sh \
    --host $(jq -r .vps1_public_ip scripts/remote/testbed-state.json) \
    --duration 120 \
    --rate "$RATE" \
    --apply
  sleep 30
done
```

Stop the sweep if:

- Finalization stalls
- Error rate exceeds the agreed threshold
- p95 latency increases sharply and does not recover
- Any validator restarts
- Disk fills or memory pressure causes container instability

### 5.9 Record results

For each run, capture:

```text
Benchmark run ID:
Date/time UTC:
Git branch:
Git commit:
Docker image:
Docker image digest:
Cloud provider:
Region:
Instance type / shape:
vCPU / memory per VM:
Node topology:
Benchmark duration:
Target deploy rate:
Observed finalized throughput:
p50 / p95 / p99 latency:
Error count:
Peak CPU per node:
Peak memory per node:
Last finalized block at start:
Last finalized block at end:
Notes / anomalies:
```

---

## 6. Suggested benchmark acceptance criteria

These are placeholders until F1R3FLY and the partner team agree on targets.

| Criterion | Initial threshold |
|---|---|
| Genesis completion | All validators complete genesis within 2 minutes |
| Health check | All expected nodes reachable through `/api/status` |
| Finalization | Finalized block height advances during benchmark |
| Error rate | Less than 1% deploy submission/finalization errors during baseline |
| p95 latency | Report only until the partner team defines an SLA |
| Node stability | No validator container restarts during baseline and moderate load |
| Reproducibility | Same image digest and same load profile produce materially similar results across two runs |

---

## 7. Recommended repository deliverables

For the benchmark effort, add only Markdown artifacts to `dev` at first:

```text
docs/benchmark/README.md
docs/benchmark/CLOUD_BENCHMARK_GUIDE.md
docs/benchmark/BENCHMARK_RUN_TEMPLATE.md
docs/benchmark/BRANCH_CONTENTS.md
docs/benchmark/RESULTS_SUMMARY_TEMPLATE.md
```

Suggested split:

| File | Purpose |
|---|---|
| `docs/benchmark/README.md` | Entry point, branch scope, support contacts, high-level workflow |
| `docs/benchmark/CLOUD_BENCHMARK_GUIDE.md` | Concrete provider-neutral setup and benchmark instructions |
| `docs/benchmark/BENCHMARK_RUN_TEMPLATE.md` | One-run checklist and result capture form |
| `docs/benchmark/BRANCH_CONTENTS.md` | The consolidated feature set and commit baseline of the benchmark branch, for reproducibility |
| `docs/benchmark/RESULTS_SUMMARY_TEMPLATE.md` | Format for the partner team to return benchmark results |

---

## 8. Clarifying questions for F1R3FLY / partner team

1. Which cloud provider and region should the first benchmark run on?
2. Which instance types should be benchmarked: ARM, AMD, Intel, or a comparison across all available families?
3. Should the benchmark optimize for throughput, latency, cost/performance, or resource efficiency?
4. Should the intra-deploy parallel execution path be exercised as part of the default baseline, or measured as a separate profile against a serial-execution configuration?
5. Should recovery/fork behavior be part of the first benchmark, or a second-phase resilience evaluation?
6. Should the partner team receive prebuilt registry images, Docker Hub images, or instructions to build locally from source?
7. What topology should be benchmarked first: 3 validators + observer, 4 validators, 10 validators, or a different target shape?
8. What workload should represent the intended use case: simple wallet transfers, Rholang compute-heavy deploys, storage-heavy deploys, or mixed workloads?
9. What reporting format is preferred: Markdown report, CSV/TSV result bundle, Grafana snapshot, or all of these?
10. What benchmark threshold would count as a successful first pass?

---

## 9. Source notes

Reviewed public repository material on 2026-07-06:

- `https://github.com/F1R3FLY-io/f1r3node-rust/tree/dev`
- `https://github.com/F1R3FLY-io/system-integration`
- `https://raw.githubusercontent.com/F1R3FLY-io/system-integration/main/integration-tests/README.md`
- `https://raw.githubusercontent.com/F1R3FLY-io/f1r3node-rust/dev/docs/vps-cloud-testing.md`
