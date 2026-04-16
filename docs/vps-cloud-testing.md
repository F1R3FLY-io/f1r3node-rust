---
doc_type: setup_guide
version: "2.0"
last_updated: 2026-04-13
---

# F1R3FLY Testbed: Local, Generic VPS, or Oracle Cloud

Three ways to stand up a F1R3FLY shard for testing, in increasing order of "realism vs. effort":

| Path | What you need | Time | Realism |
|---|---|---|---|
| **A — Local** | Docker + 8 GB RAM | ~5 min | Single-host, Docker-internal DNS |
| **B — Generic VPS** | 2 pre-provisioned SSH-accessible hosts with Docker | ~15 min | Real inter-host networking |
| **C — Oracle Cloud** | OCI account + `oci` CLI | ~20 min first time | Real inter-host + provisioning automated via `just vps-*` |

All three use the same F1R3FLY node binary, same configs, same consensus. They differ in **where the nodes run and how you bring them up**.

Pick the path that matches your goal:
- **Verify functionality / reproduce a bug** → Local
- **Measure latency under real network conditions** → Generic VPS or Oracle Cloud
- **Iterate on the distributed topology with throwaway infra** → Oracle Cloud (free-tier eligible)

---

## Shared mental model

Whichever path you pick, the shard's shape is the same:

**Topology** — 1 shard total, split across logical roles:
- **Bootstrap** (index 0) — coordinates genesis ceremony, not a validator
- **Validators** — bonded, produce blocks
- **Observer** — read-only, follows the chain

**Default single-host layout** (`docker/shard.yml`): bootstrap + 3 validators + 1 observer on the `f1r3fly-shard` Docker network. Monitoring (Prometheus + Grafana) is opt-in via `docker/monitoring.yml`.

**Distributed layout** (`docker/shard.vps1.yml` + `docker/shard.vps2.yml`): bootstrap on VPS-1 (ports 40400-40405), followers on VPS-2 in three port-bands (40410-15, 40420-25, 40450-55) to share one public IP.

**Port map (per node):**

| Port | Service |
|---|---|
| 40400 | Protocol (P2P) |
| 40401 | gRPC External |
| 40402 | gRPC Internal |
| 40403 | HTTP API (`/api/status`, `/metrics`) |
| 40404 | Kademlia discovery (UDP) |
| 40405 | Admin HTTP |

Followers on VPS-2 use `40410-40455` to avoid collisions — see [`docker/conf/validator1-remote.conf`](../docker/conf/validator1-remote.conf) etc.

**Verification invariants** (all paths):
- Genesis ceremony completes (all validators sign, block #0 finalized)
- Finalization advances past block #0 via heartbeat or user deploys
- `curl http://<host>:<http-port>/api/status` returns `{peers, nodes}` matching expected count
- Added validators can bond via `rholang/examples/bond/bond.rho` — see [TASK-001-4 notes in ToDos.md](./ToDos.md#epoch-001-system-integration-alignment) for evidence

---

# Part A — Local (single host, Docker Compose)

The simplest path. Everything on one machine using the stock compose files.

## Prereqs

- Docker ≥ 20.10 with compose v2 plugin
- ~6 GB free RAM (5 container processes; +2 GB if you also bring up `monitoring.yml`)
- `just` for the teardown recipe (optional — can use `docker compose` directly)

## Build the image

```bash
./node/docker-commands.sh build-local
# Tags as f1r3fly-rust:local
```

First build can take 10-30 minutes depending on Rust cache warmth. Subsequent rebuilds are faster thanks to Docker layer cache.

## Bring up the shard

```bash
F1R3FLY_IMAGE=f1r3fly-rust:local docker compose -f docker/shard.yml up -d
```

Starts: `rnode.bootstrap`, `rnode.validator1/2/3`, `rnode.readonly`. Genesis ceremony completes within ~60s.

Optional — add Prometheus + Grafana dashboards:
```bash
docker compose -f docker/monitoring.yml up -d
# Prometheus http://localhost:9090
# Grafana http://localhost:3000 (admin/admin)
```

Verify:

```bash
curl -s http://localhost:40403/api/status | jq '{peers,nodes,shardId}'
# Expect: {"peers":4,"nodes":4,"shardId":"root"}
```

- Bootstrap: `:40403` / Validator1: `:40413` / Validator2: `:40423` / Validator3: `:40433` / Observer: `:40453`

## Add an observer or validator4 at runtime

These join the existing `f1r3fly-shard` network:

```bash
# Observer — use instead of shard.yml's built-in readonly, not alongside
docker compose -f docker/shard.yml stop readonly
docker compose -f docker/shard.yml rm -f readonly
F1R3FLY_IMAGE=f1r3fly-rust:local docker compose -f docker/observer.yml up -d

# Validator4 (must be bonded after joining — see below)
F1R3FLY_IMAGE=f1r3fly-rust:local docker compose -f docker/validator4.yml up -d
```

## Test bonding (optional — PoS flow)

`validator4` joins unbonded. To have it participate in consensus, deploy the bond contract signed by validator4's key:

```bash
docker cp rholang/examples/bond/bond.rho rnode.validator1:/tmp/bond.rho
docker exec rnode.validator1 /opt/docker/bin/node deploy \
  1000000 1 0 \
  $(grep VALIDATOR4_PRIVATE_KEY docker/.env | cut -d= -f2) \
  /dev/null /tmp/bond.rho root
```

Wait for the next heartbeat block (~5s). Check:

```bash
docker exec rnode.validator1 /opt/docker/bin/node bond-status \
  $(grep VALIDATOR4_PUBLIC_KEY docker/.env | cut -d= -f2)
# Expect: "Validator is bonded"
```

Within ~30s you'll see `Heartbeat: Successfully created block` entries in `docker logs rnode.validator4`.

**Note:** the above works because `docker/genesis/wallets.txt` funds validator4's REV address (`1111La6tHaCt...jtEi3M`). If you swap in a different validator key, you'll need to add its REV address to `wallets.txt` and restart the shard (volumes must be wiped). Compute the address with:

```bash
sed 's/%PUB_KEY/<your-pubkey>/' rholang/examples/vault_demo/1.know_ones_vaultaddress.rho > /tmp/revaddr.rho
docker cp /tmp/revaddr.rho rnode.validator1:/tmp/
docker exec rnode.validator1 /opt/docker/bin/node eval /tmp/revaddr.rho
docker logs rnode.validator1 --since 10s | grep "VaultAddress for"
```

## Benchmark (optional)

```bash
# Against the local shard (targets rnode.validator1 :40413 by default)
./scripts/bench/latency-benchmark.sh --duration 30 --rate 2 --apply

# See outputs in /tmp/f1r3fly-bench-<timestamp>/ — load-summary.txt,
# latency-report.txt, casper-profile.txt. Same flags/outputs as Part C6.5.
```

## Teardown

```bash
just shard-down
```

Or manually:
```bash
docker compose -f docker/validator4.yml down -v
docker compose -f docker/observer.yml down -v
docker compose -f docker/shard.yml down -v
```

---

# Part B — Generic VPS (bring-your-own 2 SSH hosts)

For when you want real inter-host networking but don't want to use OCI. Works against any 2 Linux machines you already have SSH access to — cloud, bare metal, colo, Raspberry Pi, whatever.

## Prereqs

- **2 Linux hosts** (VPS-1 and VPS-2) reachable from your laptop over SSH
  - Any distro with `docker` and `docker-compose-plugin` installed; recent Ubuntu, Debian, Fedora, Oracle Linux all work
  - SSH key-based auth (`scp`/`ssh` with `-i /path/to/key`)
  - Your SSH user (typically `opc`, `ubuntu`, `debian`, or `root`) must be in the `docker` group
- **Firewall**: both hosts allow inbound on `tcp:22` (SSH), `tcp:40400-40455`, `udp:40400-40455` from your admin IP and from each other's public IP
- **Public IPs** — no NAT for VPS→VPS traffic (or arrange DNAT on ports 40400-40455)
- Local: `docker`, `scp`, `ssh`, `jq`, the repo checked out

## Step 1 — Build and ship the image

```bash
./node/docker-commands.sh build-local
# Tag it with the name the compose files expect
docker tag f1r3fly-rust:local sjc.ocir.io/axd0qezqa9z3/f1r3fly-rust:latest
```

Transfer the image to both hosts (parallel, via `docker save | gzip | ssh | docker load`). The existing `scripts/remote/image-transfer.sh` handles this:

```bash
export KEY_FILE=~/.ssh/my_testbed_key   # your SSH identity
# Populate testbed-state.json by hand — see Step 2 — BEFORE running image-transfer.sh
./scripts/remote/image-transfer.sh --apply
```

## Step 2 — Populate the state file

The deploy / status / teardown scripts read VPS IPs from `scripts/remote/testbed-state.json`. For OCI this is written automatically by `oci-provision.sh`; for a generic VPS you hand-write it:

```bash
cat > scripts/remote/testbed-state.json <<EOF
{
  "vps1_public_ip": "203.0.113.10",
  "vps2_public_ip": "203.0.113.20"
}
EOF
```

Also set your SSH key path so the scripts pick it up:

```bash
export KEY_FILE=~/.ssh/my_testbed_key
export SSH_USER=opc        # or ubuntu, root, etc.
```

## Step 3 — Deploy

```bash
just vps-deploy
# Under the hood: scripts/remote/deploy.sh --apply
#   1. Renders docker/.env.remote from .env.remote.template with the VPS IPs
#   2. scp docker/{conf,genesis,certs,shard.vps*.yml,.env.remote} to both hosts
#   3. Starts bootstrap on VPS-1, polls http://VPS1:40403/api/status
#   4. Starts validators + observer on VPS-2
```

## Step 4 — Verify

```bash
just vps-status
# Hits /api/status and /metrics on all 4 nodes (bootstrap on VPS-1:40403,
# validator1 on VPS-2:40413, validator2 on VPS-2:40423, observer on VPS-2:40453).
# Exits non-zero if any node is unhealthy.
```

## Step 5 — Teardown

```bash
# Stop containers + wipe volumes on both VPSes, leave the hosts themselves running
./scripts/remote/teardown.sh --apply

# `just vps-down` also tries to terminate OCI VMs via oci-destroy.sh, which
# doesn't apply to generic VPSes — use the teardown script directly instead.
```

## What you lose compared to Oracle Cloud path

- **No `just vps-up` equivalent** — you provision the hosts yourself (another cloud's console, `terraform`, `ansible`, whatever)
- **No `oci-destroy.sh` equivalent** — destroy the VMs via whatever path you used to create them
- Everything else (`deploy.sh`, `status.sh`, `teardown.sh`, `image-transfer.sh`) is SSH-based and provider-agnostic

See [`BACKLOG-FI-002` in `docs/Backlog.md`](./Backlog.md) for the plan to generalize provisioning across AWS / GCP / etc.

---

# Part C — Oracle Cloud (full automation)

OCI has the cleanest automation because `just vps-up` / `vps-down` handle provisioning end-to-end. This section walks through a fresh setup.

## Two audiences

Skip parts that don't apply:
- **F1R3FLY contributors** using the project's existing `f1r3fly-devops` compartment — Parts C1, C2, C5, C6 (and C8 if something breaks)
- **New tenants** in a fresh OCI account — everything below (Parts C1-C8)

## Prereqs

Local machine:
- `oci` CLI ≥ 3.76
- `jq`
- `ssh`, `scp`, `ssh-keygen`
- Docker

Cloud:
- Oracle Cloud account. The **Always Free tier** covers this testbed entirely — 2× arm64 Ampere A1.Flex VMs (2 OCPU/4 GB and 4 OCPU/8 GB) are inside the free-tier ceiling of 4 OCPU + 24 GB per VM, 200 GB block volume total.

## C1 — Create an OCI account (skip if you have one)

1. Sign up at [oracle.com/cloud/free](https://www.oracle.com/cloud/free/). Always-free tier doesn't require paid activation for arm64 Ampere A1.Flex shapes.
2. Choose **US West (San Jose)** as your home region. The testbed scripts default to `us-sanjose-1`; you can override via `OCI_REGION=...` but the default keeps things simple.
3. Complete identity verification. OCI requires a credit card for fraud prevention even on the free tier, but arm64 A1.Flex usage does not bill.
4. Note your **tenancy OCID** (User menu → Tenancy → copy OCID) and **object-storage namespace** (short string used in OCIR URLs).

## C2 — Install and configure the `oci` CLI

**macOS:**
```bash
brew install oci-cli jq
```

**Linux:**
```bash
bash -c "$(curl -L https://raw.githubusercontent.com/oracle/oci-cli/master/scripts/install/install.sh)"
```

**Configure:**
```bash
oci setup config
# Prompts for user OCID, tenancy OCID, region, generates a keypair
```

Upload the generated public key via Console → User Settings → API Keys → Add API Key:
```bash
cat ~/.oci/oci_api_key_public.pem
```

**Verify:**
```bash
oci iam region-subscription list --query 'data[].{name:"region-name",status:status}' --output table
# Expect us-sanjose-1 / READY
```

## C3 — Compartment setup

### C3a — Contributors using `f1r3fly-devops`

```bash
oci iam compartment list --all --compartment-id-in-subtree true \
  --query 'data[?name==`f1r3fly-devops`].{id:id,name:name}' --output table
```

If the compartment shows up, you're set — the scripts hardcode the correct OCID as default.

### C3b — New tenants creating their own compartment

```bash
TENANCY_OCID=ocid1.tenancy.oc1..xxxxx   # from C1

oci iam compartment create \
  --compartment-id "$TENANCY_OCID" \
  --name f1r3fly-devops \
  --description "F1R3FLY development and testbed resources" \
  --wait-for-state ACTIVE
```

Capture the returned OCID, then persist:
```bash
export OCI_COMPARTMENT_ID=ocid1.compartment.oc1..<your new compartment OCID>
# Add to ~/.zshrc or ~/.bashrc so it survives new sessions
```

## C4 — IAM: user, group, policy (new tenants only; contributors skip)

```bash
TESTBED_USER_OCID=$(oci iam user create \
  --name f1r3node-rust-testbed \
  --description "Testbed provisioning user" \
  --query 'data.id' --raw-output)

TESTBED_GROUP_OCID=$(oci iam group create \
  --name f1r3node-rust-testbed-admins \
  --description "Can provision testbed VPSes" \
  --query 'data.id' --raw-output)

oci iam group add-user --user-id "$TESTBED_USER_OCID" --group-id "$TESTBED_GROUP_OCID"

oci iam policy create \
  --compartment-id "$TENANCY_OCID" \
  --name f1r3node-rust-testbed-policy \
  --description "Permissions for f1r3node-rust testbed provisioning" \
  --statements '[
    "Allow group f1r3node-rust-testbed-admins to manage virtual-network-family in compartment f1r3fly-devops",
    "Allow group f1r3node-rust-testbed-admins to manage instance-family in compartment f1r3fly-devops",
    "Allow group f1r3node-rust-testbed-admins to read all-resources in tenancy"
  ]'
```

Then re-run `oci setup config` against the new user to pick up its API key. Auth tokens are only needed for OCIR publishing (not the testbed).

## C5 — Region + quota verification

```bash
oci iam region-subscription list --query 'data[?"region-name"==`us-sanjose-1`]' --output table

oci limits value list \
  --compartment-id "$(grep ^tenancy ~/.oci/config | cut -d= -f2)" \
  --service-name compute \
  --query 'data[?starts_with(name,`standard-a1`)].{name:name,value:value}' \
  --output table
```

Expected: `standard-a1-core-count` in the thousands. Testbed needs 6 OCPU.

## C6 — End-to-end walkthrough

The `just vps-*` recipes are the entry point; direct script invocation still works for dry-runs and overrides.

### C6.1 — Provision

```bash
just vps-up
```

Takes ~3-5 min for cloud-init to finish. Underlying `scripts/remote/oci-provision.sh --apply` writes OCIDs + public IPs to `scripts/remote/testbed-state.json` and generates an ed25519 SSH keypair at `scripts/remote/testbed.pem` (both gitignored).

SSH in to verify:
```bash
VPS1_IP=$(jq -r .vps1_public_ip scripts/remote/testbed-state.json)
ssh -i scripts/remote/testbed.pem opc@$VPS1_IP \
  "docker --version && cat /var/log/f1r3fly-testbed-init.log"
```

For a dry-run first: `./scripts/remote/oci-provision.sh` (no `--apply`).

### C6.2 — Transfer the Docker image

```bash
# Build or pull the image locally first:
docker pull sjc.ocir.io/axd0qezqa9z3/f1r3fly-rust:latest   # once OCIR publishing is live
# OR
./node/docker-commands.sh build-local && \
  docker tag f1r3fly-rust:local sjc.ocir.io/axd0qezqa9z3/f1r3fly-rust:latest

just vps-image-push
```

Custom tags: `just vps-image-push image=my-tag:dev`.

### C6.3 — Deploy the shard

```bash
just vps-deploy
```

Same flow as Part B: render `.env.remote`, scp, start bootstrap, wait for `/api/status`, start followers.

### C6.4 — Check shard health

```bash
just vps-status                     # all 4 nodes
just vps-status target=vps1         # bootstrap only
just vps-status target=vps2         # validators + observer
```

### C6.5 — Run benchmarks

```bash
# Default: 60s flood at 2 deploys/sec against VPS-1's validator1
just vps-bench-latency host=$(jq -r .vps1_public_ip scripts/remote/testbed-state.json) duration=60 rate=2

# Or call the script directly for dry-run / overrides:
./scripts/bench/latency-benchmark.sh --host <VPS-IP> --duration 120 --rate 5 --apply
```

Outputs land in `/tmp/f1r3fly-bench-<timestamp>/`:

- `load-summary.txt` — deploys submitted / finalized / errored, observed throughput
- `latency-report.txt` — submit→finalize p50 / p95 / min / max / avg (ms)
- `casper-profile.txt` — per-validator propose_core_ms / block_replay_ms / finalizer_cycle_ms percentiles from log parse
- `submits.tsv`, `finals.tsv`, `latencies.raw` — raw data for downstream analysis

The benchmark uses the `node` binary inside the container to sign + submit deploys (deploys require secp256k1 signing that bare `grpcurl` can't produce). Default deployer key is bootstrap's (funded locally and in `wallets.txt` for distributed via commit `993c239`). Override with `DEPLOYER_KEY=<hex>`.

### C6.6 — Teardown

```bash
just vps-down
```

Stops containers + wipes volumes on both VPSes, then terminates the VMs, subnet, security list, IGW, VCN. To only stop containers (keep the VMs): `./scripts/remote/teardown.sh --apply`.

## C7 — Cost notes (Oracle Cloud path only)

Always-free resources, verified against [Oracle's free-tier docs](https://docs.oracle.com/en-us/iaas/Content/FreeTier/freetier_topic-Always_Free_Resources.htm):

| Resource | Our usage | Free-tier ceiling |
|---|---|---|
| Arm64 A1.Flex OCPUs | 6 total (2 + 4) | 4 per VM, 24 total |
| Arm64 A1.Flex memory | 12 GB (4 + 8) | 24 GB total |
| Block volume storage | Default 50 GB per VM | 200 GB total |
| Public IPs (ephemeral) | 2 | Unlimited ephemeral |
| VCN | 1 | 2 |
| Outbound data transfer | Few GB for image + deploy | 10 TB/month |

**Watch out for:**
- Leaving the testbed running past your actual need — CPU time counts toward monthly quota even on always-free
- Detached block volumes linger after instance termination until explicitly deleted — `oci-destroy.sh --apply` handles this
- Cross-region egress if you change `OCI_REGION` — stay within one region for free-tier pricing

Run `just vps-down` when done.

## C8 — Troubleshooting (Oracle Cloud path)

| Symptom | Likely cause | Fix |
|---|---|---|
| `NotAuthorizedOrNotFound` on VCN create | Missing `manage virtual-network-family` policy | Revisit C4; `oci iam group list-users --group-id <group>` |
| `LimitExceeded` on instance launch | Arm64 quota is 0 | Request increase via Console → Governance → Limits |
| Cloud-init never finishes | firewalld blocking Docker repo, or metadata service slow | SSH in, check `/var/log/cloud-init-output.log` |
| `docker load` errors with permission denied on VPS | opc not yet in docker group | Re-SSH (picks up group) or use `sudo docker load` |
| `oci-destroy.sh` fails mid-way | Lingering dependency | Re-run — it's idempotent |
| Port 22 unreachable after provision | Security list rule missing or firewalld blocking | Verify via `oci network security-list get --security-list-id <id>` |

---

# Shared troubleshooting (all paths)

| Symptom | Likely cause | Fix |
|---|---|---|
| Genesis hangs past 120s | TLS certs missing or wrong perms | Check `docker/certs/{bootstrap,validator1,validator2,validator3}/*.pem` exist and are readable |
| `/api/status` returns `peers:0` | Bootstrap unreachable from validators | Check firewalld/security list; confirm BOOTSTRAP_HOST in `.env` matches reachable name/IP |
| `system_deploy_error: "Deploy payment failed: Insufficient funds"` | Signer's REV address not in `wallets.txt` | Add the REV address (derive via `node eval` on `rholang/examples/vault_demo/1.know_ones_vaultaddress.rho`), restart with fresh volumes |
| `NoNewDeploys` on explicit `node propose` | Heartbeat already consumed the deploy | Not an error — the deploy was already included. Check `last-finalized-block` |
| Validator bonded but not producing blocks | Quarantine period (10 blocks) hasn't elapsed | Wait ~50s past bond inclusion |

---

## References

- [EPOCH-001 in docs/ToDos.md](./ToDos.md#epoch-001-system-integration-alignment) — TASK-001-4 covers the local verification path
- [EPOCH-009 in docs/ToDos.md](./ToDos.md#epoch-009-distributed-oci-testbed-for-latency-benchmarking) — distributed testbed implementation tasks
- [US-003 in docs/UserStories.md](./UserStories.md#us-003-distributed-oci-testbed-for-latency-benchmarking) — user story
- [scripts/remote/README.md](../scripts/remote/README.md) — script-level usage, config, naming convention
- [docker/README.md](../docker/README.md) — local compose flow details (image build, ports, monitoring)
- [BACKLOG-FI-002 in docs/Backlog.md](./Backlog.md) — plan to generalize provisioning to AWS/GCP
- [Oracle Always Free Resources](https://docs.oracle.com/en-us/iaas/Content/FreeTier/freetier_topic-Always_Free_Resources.htm)
- [OCI CLI docs](https://docs.oracle.com/en-us/iaas/tools/oci-cli/latest/oci_cli_docs/)
- [OCI IAM Policy Reference](https://docs.oracle.com/en-us/iaas/Content/Identity/Reference/policyreference.htm)
