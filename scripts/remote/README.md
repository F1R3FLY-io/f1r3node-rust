# Remote testbed scripts

Provisioning and teardown scripts for the F1R3FLY distributed OCI testbed (EPOCH-009 / US-003).

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

### Provision

```bash
# Dry run (default) — prints every planned OCI API call, creates nothing
./scripts/remote/oci-provision.sh

# Actually create resources
./scripts/remote/oci-provision.sh --apply
```

State is persisted to `scripts/remote/testbed-state.json` (gitignored). SSH keypair is generated to `scripts/remote/testbed.pem` (+ `.pub`, also gitignored).

### Teardown

```bash
# Dry run (default)
./scripts/remote/oci-destroy.sh

# Actually terminate (prompts for VCN name confirmation)
./scripts/remote/oci-destroy.sh --apply

# Skip confirmation (CI-friendly)
./scripts/remote/oci-destroy.sh --apply --force
```

Teardown reads the same state file and reverses provisioning in dependency order:
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
