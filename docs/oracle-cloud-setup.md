---
doc_type: setup_guide
version: "1.0"
last_updated: 2026-04-13
---

# Oracle Cloud Setup for the F1R3FLY Testbed

This guide walks through setting up Oracle Cloud Infrastructure (OCI) for the distributed latency testbed scripts in [`scripts/remote/`](../scripts/remote/README.md) (EPOCH-009 / US-003).

The testbed is a single F1R3FLY shard split across two arm64 VPSes: VPS-1 runs the bootstrap node, VPS-2 runs two validators and a read-only observer. It's designed for latency benchmarking under realistic inter-host network conditions.

## Two audiences

Skip the parts that don't apply:

- **F1R3FLY contributors** using the project's existing `f1r3fly-devops` compartment — you probably only need **Parts 1, 2, 5, 6** (and maybe 8 if something goes wrong).
- **New tenants** setting up in a fresh OCI account — you need **everything** (Parts 1-8).

## Prerequisites

Local machine:

- `oci` CLI (≥ 3.76)
- `jq`
- `ssh`, `scp`, `ssh-keygen`
- Docker (for building/transferring images)

Cloud:

- An Oracle Cloud account. The **Always Free tier** covers this testbed entirely — 2× arm64 Ampere A1.Flex VMs (2 OCPU/4 GB and 4 OCPU/8 GB) are well inside the free-tier ceiling of 4 OCPU + 24 GB per VM, 200 GB block volume total.

---

## Part 1 — Create an OCI account (skip if you already have one)

1. Sign up at [oracle.com/cloud/free](https://www.oracle.com/cloud/free/). The always-free tier doesn't require a paid plan activation for arm64 Ampere A1.Flex shapes.
2. Choose **US West (San Jose)** as your home region. The testbed scripts default to `us-sanjose-1`. You can override via `OCI_REGION=...`, but using the default is simpler.
3. Complete identity verification. OCI requires a credit card for fraud prevention even on the free tier, but arm64 Ampere A1.Flex usage does not bill.
4. After activation, note your **tenancy OCID** (User menu → Tenancy → copy OCID) and **object-storage namespace** (User menu → Tenancy → Object Storage Namespace — this is the short string used in OCIR URLs like `sjc.ocir.io/<namespace>/...`).

---

## Part 2 — Install and configure the `oci` CLI

### Install

macOS (Homebrew):
```bash
brew install oci-cli jq
```

Linux (official installer):
```bash
bash -c "$(curl -L https://raw.githubusercontent.com/oracle/oci-cli/master/scripts/install/install.sh)"
```

### Configure credentials

Run the interactive setup — it generates an RSA keypair and creates `~/.oci/config`:

```bash
oci setup config
```

You'll be prompted for:
- Your user OCID (Console → User Settings → OCID)
- Your tenancy OCID (copied in Part 1)
- Region — choose `us-sanjose-1`
- Whether to generate a keypair — **yes** (it creates `~/.oci/oci_api_key.pem`)

Then upload the generated public key to your user:

```bash
cat ~/.oci/oci_api_key_public.pem
# Copy the output, go to Console → User Settings → API Keys → Add API Key → Paste Public Key
```

### Verify

```bash
oci iam region-subscription list --query 'data[].{name:"region-name",status:status}' --output table
```

You should see `us-sanjose-1 / READY` in the list.

---

## Part 3 — Compartment setup

Compartments are logical folders that group OCI resources. The testbed scripts default to an existing `f1r3fly-devops` compartment.

### Part 3a — Contributors using the existing `f1r3fly-devops`

Verify you can see it:

```bash
oci iam compartment list --all --compartment-id-in-subtree true \
  --query 'data[?name==`f1r3fly-devops`].{id:id,name:name}' --output table
```

If the compartment shows up, you're set. Note its OCID — the scripts already hardcode the correct one as the default, so you usually don't need to override anything.

### Part 3b — New tenants creating a fresh compartment

Create a compartment under your tenancy root:

```bash
# Replace with your tenancy OCID (from Part 1)
TENANCY_OCID=ocid1.tenancy.oc1..xxxxx

oci iam compartment create \
  --compartment-id "$TENANCY_OCID" \
  --name f1r3fly-devops \
  --description "F1R3FLY development and testbed resources" \
  --wait-for-state ACTIVE
```

Capture the returned OCID. Then export it so the scripts pick it up instead of the F1R3FLY-owned default:

```bash
export OCI_COMPARTMENT_ID=ocid1.compartment.oc1..<your new compartment OCID>
```

Persist this in your shell profile (`~/.zshrc`, `~/.bashrc`) so it survives new terminal sessions.

---

## Part 4 — IAM: user, group, policy (new tenants only; contributors skip)

The testbed scripts need permission to manage VCNs and compute instances in the target compartment.

### Create a dedicated CI/testbed user

```bash
# Create user
TESTBED_USER_OCID=$(oci iam user create \
  --name f1r3node-rust-testbed \
  --description "Testbed provisioning user" \
  --query 'data.id' --raw-output)
echo "User: $TESTBED_USER_OCID"

# Create group
TESTBED_GROUP_OCID=$(oci iam group create \
  --name f1r3node-rust-testbed-admins \
  --description "Can provision testbed VPSes" \
  --query 'data.id' --raw-output)
echo "Group: $TESTBED_GROUP_OCID"

# Add user to group
oci iam group add-user --user-id "$TESTBED_USER_OCID" --group-id "$TESTBED_GROUP_OCID"
```

### Attach the policy

```bash
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

### Create API key + auth token for the user

Follow the same `oci setup config` flow (Part 2) but select the new user's OCID. The testbed scripts use API keys, not auth tokens — auth tokens are only needed if you later set up OCIR publishing.

---

## Part 5 — Region + shape quota verification

Confirm `us-sanjose-1` is subscribed and the arm64 quota is sufficient:

```bash
oci iam region-subscription list \
  --query 'data[?"region-name"==`us-sanjose-1`]' --output table

oci limits value list \
  --compartment-id "$(grep ^tenancy ~/.oci/config | cut -d= -f2)" \
  --service-name compute \
  --query 'data[?starts_with(name,`standard-a1`)].{name:name,value:value}' \
  --output table
```

Expected: `standard-a1-core-count` should be in the thousands. The testbed uses 6 OCPU total (2+4), so any quota above 6 works.

---

## Part 6 — End-to-end testbed walkthrough

Run from the repo root (`f1r3node-rust/`). The Justfile recipes (`vps-*`) wrap the underlying scripts and are the recommended entry point. Direct script invocation is still supported for dry-runs and overrides.

### 6.1 Provision the VPSes

```bash
just vps-up
```

Takes ~3-5 minutes for cloud-init to finish. The underlying `scripts/remote/oci-provision.sh --apply` persists OCIDs and public IPs to `scripts/remote/testbed-state.json` and generates an ed25519 SSH keypair at `scripts/remote/testbed.pem`. Both are gitignored.

Verify by SSH-ing in:

```bash
VPS1_IP=$(jq -r .vps1_public_ip scripts/remote/testbed-state.json)
ssh -i scripts/remote/testbed.pem opc@$VPS1_IP "docker --version && cat /var/log/f1r3fly-testbed-init.log"
```

You should see the Docker version string and a `cloud-init complete at <timestamp>` line.

For a dry-run preview before provisioning, call the script directly without `--apply`:

```bash
./scripts/remote/oci-provision.sh
```

### 6.2 Transfer a Docker image

```bash
# Build or pull the image locally first, for example:
docker pull sjc.ocir.io/axd0qezqa9z3/f1r3fly-rust:latest   # once OCIR publishing is live
# OR
./node/docker-commands.sh build-local && \
  docker tag f1r3fly-rust:local sjc.ocir.io/axd0qezqa9z3/f1r3fly-rust:latest

# Ship to both VPSes in parallel
just vps-image-push
```

To transfer a custom tag: `just vps-image-push image=my-local-tag:dev`.

### 6.3 Deploy the shard

```bash
just vps-deploy
```

Renders `docker/.env.remote` from the checked-in template, `scp`s `docker/conf/`, `docker/genesis/`, `docker/certs/`, and the two `shard.vps*.yml` files to both VPSes, starts the bootstrap on VPS-1, waits for its HTTP `/api/status` to respond, then starts the 2 validators + observer on VPS-2.

### 6.4 Check shard health

```bash
just vps-status                     # all 4 nodes
just vps-status target=vps1         # bootstrap only
just vps-status target=vps2         # validators + observer
```

Exits non-zero if any node is unreachable — suitable for use in benchmark preflight (TASK-009-5).

### 6.5 Run benchmarks

TASK-009-5 (latency benchmark port) is not yet implemented. Once it lands, this section will be extended with `just vps-bench-latency` usage.

### 6.6 Teardown

```bash
just vps-down
```

Runs `teardown.sh --apply` (stops containers, wipes volumes on both VPSes) then `oci-destroy.sh --apply --force` (terminates VMs, deletes subnet, security list, IGW, VCN).

To stop the shard without terminating the VPSes — useful when iterating on configuration — use `./scripts/remote/teardown.sh --apply` alone.

---

## Part 7 — Cost notes

The testbed uses only **Always Free** resources, verified against [Oracle's free-tier docs](https://docs.oracle.com/en-us/iaas/Content/FreeTier/freetier_topic-Always_Free_Resources.htm):

| Resource | Our usage | Free-tier ceiling |
|---|---|---|
| Arm64 Ampere A1.Flex OCPUs | 6 total (2 + 4) | 4 per VM, 24 total (but 4 VMs ≈ 1 OCPU typical) |
| Arm64 Ampere A1.Flex memory | 12 GB (4 + 8) | 24 GB total |
| Block volume storage | Default 50 GB per VM | 200 GB total |
| Public IPs (ephemeral) | 2 | Unlimited ephemeral |
| VCN | 1 | 2 |
| Outbound data transfer | Negligible for dry-runs; a few GB for image transfer | 10 TB/month |

**Watch out for:**
- **Leaving the testbed running** — even on the free tier, CPU time counts toward your monthly quota.
- **Storage accumulation** — terminated instances release their attached block volumes automatically when `oci-destroy.sh --apply` runs, but detached block volumes linger. Check the Console periodically.
- **Cross-region egress** if you change `OCI_REGION` — stay within a single region for free-tier pricing.

Run `oci-destroy.sh --apply` when done. It's the simplest way to ensure nothing's burning resources.

---

## Part 8 — Troubleshooting

| Symptom | Likely cause | Fix |
|---|---|---|
| `NotAuthorizedOrNotFound` on VCN create | Missing `manage virtual-network-family` policy | Revisit Part 4; confirm group membership with `oci iam group list-users --group-id <group>` |
| `LimitExceeded` on instance launch | Arm64 quota is 0 | Request quota increase at Console → Governance → Limits → Request a Service Limit Increase |
| Cloud-init never finishes (no `/var/log/f1r3fly-testbed-init.log`) | firewalld blocking Docker repo fetch, or OL9 metadata service slow | SSH in, check `/var/log/cloud-init-output.log` |
| `docker load` on VPS errors with permission denied | opc user not yet in docker group | Log out + back in (`ssh ...` again), or temporarily use `sudo docker load` |
| `oci-destroy.sh` fails mid-way | Dependency still attached (e.g. instance still referencing subnet) | Re-run — it's idempotent; individual steps skip when the resource is already gone |
| VPSes unreachable on port 22 | Security list rule not applied, or firewalld blocking | Verify with `oci network security-list get --security-list-id <id>`; check cloud-init ran |

For anything not covered here, `oci-provision.sh` preserves the state file on failure — partial provisions can be torn down with `oci-destroy.sh --apply`.

---

## References

- [EPOCH-009 in docs/ToDos.md](./ToDos.md#epoch-009-distributed-oci-testbed-for-latency-benchmarking) — task definitions + acceptance criteria
- [US-003 in docs/UserStories.md](./UserStories.md#us-003-distributed-oci-testbed-for-latency-benchmarking) — user story
- [scripts/remote/README.md](../scripts/remote/README.md) — script-level usage and config
- [Oracle Always Free Resources](https://docs.oracle.com/en-us/iaas/Content/FreeTier/freetier_topic-Always_Free_Resources.htm)
- [OCI CLI docs](https://docs.oracle.com/en-us/iaas/tools/oci-cli/latest/oci_cli_docs/)
- [OCI IAM Policy Reference](https://docs.oracle.com/en-us/iaas/Content/Identity/Reference/policyreference.htm)
