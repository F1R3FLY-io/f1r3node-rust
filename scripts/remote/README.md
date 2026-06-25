# Remote testbed scripts

Provisioning and teardown scripts for the F1R3FLY distributed OCI testbed (EPOCH-009 / US-003).

**Naming convention.** Script names use an `oci-` prefix for cloud-specific work (e.g. `oci-provision.sh`, `oci-destroy.sh`) and no prefix for cloud-agnostic work (`deploy.sh`, `status.sh`, `teardown.sh`, `image-transfer.sh`). The wrapping Justfile recipes use a neutral `vps-` prefix so the user-facing interface stays stable when other providers (AWS, GCP) are added — see `docs/Backlog.md` BACKLOG-FI-002.

## Scope

Single F1R3FLY shard distributed across two OCI VPSes:

| Role | Shape | Size | Runs |
|------|-------|------|------|
| VPS-1 (bootstrap) | VM.Standard.A1.Flex (arm64) | 2 OCPU / 4 GB | Bootstrap node |
| VPS-2 (followers) | VM.Standard.A1.Flex (arm64) | 4 OCPU / 8 GB | 2 validators + 1 read-only observer |

Both VPSes run Oracle Linux 9, have Docker CE installed via cloud-init, and live in a dedicated VCN (`f1r3node-rust-testbed-vcn`) with a public subnet.

## Prerequisites

- `oci` CLI configured (`~/.oci/config` with a user that has `manage virtual-network-family` and `manage instance-family` on `f1r3fly-devops`)
- `jq`, `ssh-keygen`
- An OCI tenancy with the `VM.Standard.A1.Flex` shape (arm64 Ampere, free-tier eligible in `us-sanjose-1`)

## Usage

The recommended entry point is the `just` recipes — they wrap the scripts with sensible defaults. Direct script invocation remains supported for overrides and for development.

### Justfile recipes (recommended)

```bash
just vps-up                     # provision 2 OCI VPSes (~3-5 min)
just vps-image-push             # ship Docker image from local daemon to both VPSes
just vps-deploy                 # render .env.remote, scp, start shard
just vps-status                 # health check all 4 nodes
just vps-status target=vps1     # check bootstrap only
just vps-bench-latency host=<ip> duration=60 rate=2    # latency benchmark
just vps-down                   # stop containers + terminate VPSes
```

Benchmark scripts live separately in `scripts/bench/`:

```bash
# Local shard (brought up via docker compose -f docker/shard.yml up)
./scripts/bench/latency-benchmark.sh --duration 60 --rate 2 --apply

# Dry-run preview (default if --apply not passed)
./scripts/bench/latency-benchmark.sh --duration 60 --rate 2

# Log-level per-validator profiler (standalone)
./scripts/bench/profile-casper-latency.sh rnode.validator1
```

### Direct script invocation (for dry-run / overrides)

### Provision

```bash
# Dry run (default) — prints every planned OCI API call, creates nothing
./scripts/remote/oci-provision.sh

# Actually create resources
./scripts/remote/oci-provision.sh --apply
```

State is persisted to `scripts/remote/testbed-state.json` (gitignored). SSH keypair is generated to `scripts/remote/testbed.pem` (+ `.pub`, also gitignored).

### Transfer a Docker image

After provisioning, ship the node image from your local Docker daemon to both VPSes:

```bash
# Dry run (default) — shows planned docker save / scp / docker load commands
./scripts/remote/image-transfer.sh

# Actually transfer (parallel, to both VPSes)
./scripts/remote/image-transfer.sh --apply

# Transfer a non-default image (e.g. a locally built tag)
./scripts/remote/image-transfer.sh --apply f1r3fly-rust:local
```

Default image is `sjc.ocir.io/axd0qezqa9z3/f1r3fly-rust:latest` — matches the compose-file defaults so distributed deploys work without env overrides.

**Migration note:** Once CI starts publishing to OCIR on `master` pushes (pending the `/ci.yml` update queued earlier), replace `image-transfer.sh` with `docker pull <OCIR-URL>` run over SSH on each VPS. Keep `image-transfer.sh` as a fallback for:
- Air-gapped testbeds
- Local-build smoke tests before a CI publish
- Hotfix images not yet tagged

### Deploy the shard

```bash
# Dry-run shows every planned sed / scp / ssh command
./scripts/remote/deploy.sh

# Actually render .env.remote, scp docker/, start bootstrap on VPS-1,
# wait for /api/status, then start followers on VPS-2
./scripts/remote/deploy.sh --apply
```

### Health check

```bash
./scripts/remote/status.sh          # all 4 nodes
./scripts/remote/status.sh vps1     # bootstrap only
./scripts/remote/status.sh vps2     # validators + observer
```

Exits non-zero if any node is unhealthy (suitable for CI / smoke-test use).

### Teardown

Two-phase so you can stop containers without terminating VPSes (e.g. between redeploys):

```bash
# Stop containers only (shard goes down, VPSes stay up)
./scripts/remote/teardown.sh --apply

# Terminate OCI VPSes (VCN, subnet, security list, instances)
./scripts/remote/oci-destroy.sh --apply
```

`just vps-down` runs both in sequence.

The OCI teardown reads `testbed-state.json` and reverses provisioning in dependency order:
instances → subnet → security list → route rules → IGW → VCN.

## Configuration

All knobs are env-overridable. Defaults in `oci-common.sh`:

| Var | Default |
|-----|---------|
| `OCI_REGION` | `us-sanjose-1` |
| `OCI_COMPARTMENT_ID` | `f1r3fly-devops` OCID |
| `OCI_AD` | `fnZP:US-SANJOSE-1-AD-1` |
| `TESTBED_NAME` | `f1r3node-rust-testbed` |
| `VCN_CIDR` | `10.1.0.0/16` |
| `SUBNET_CIDR` | `10.1.1.0/24` |
| `SHAPE` | `VM.Standard.A1.Flex` |
| `VPS1_OCPUS` / `VPS1_MEMORY_GB` | `2` / `4` |
| `VPS2_OCPUS` / `VPS2_MEMORY_GB` | `4` / `8` |

## Network rules

The security list opens:

- TCP 22 from 0.0.0.0/0 (SSH)
- TCP 40400-40405 from 0.0.0.0/0 (F1R3FLY protocol, gRPC, HTTP, admin)
- UDP 40404 from 0.0.0.0/0 (Kademlia peer discovery)
- All egress to 0.0.0.0/0

The instance-level firewalld inside each VM mirrors the same rules (set up by `cloud-init.yaml`).

## Idempotency

Re-running `oci-provision.sh` is safe — it checks the state file and skips anything already tracked. A partially-failed provision run can be resumed by re-running, or rolled back via `oci-destroy.sh`.

## Safety notes

- Default mode is dry-run. `--apply` is the only way to create resources.
- `oci-destroy.sh --apply` requires typing the VCN name to confirm (unless `--force`).
- The testbed uses a public IP + public security list — intended for short-lived benchmark runs, not long-running production.
- SSH keypair stays local. Private key is never uploaded anywhere.
