# =================================================================
# F1R3FLY RUST NODE - LOCAL DEVELOPMENT COMMANDS
# =================================================================
# Run `just` to see all available commands.
# Run `just <command>` to execute a command.
#
# Prerequisites:
#   - Rust toolchain (cargo)
#   - just command runner (included in nix flake)

# Configuration paths
local_dir := justfile_directory() / "run-local"
standalone_conf := local_dir / "conf/standalone.conf"
standalone_genesis := local_dir / "genesis/standalone"
standalone_data := local_dir / "data/standalone"

# Validator credentials (bootstrap wallet)
standalone_private_key := "5f668a7ee96d944a4494cc947e4005e172d7ab3461ee5538f1f2a45a835e9657"

# Default recipe - show available commands
default:
    @just --list

# =================================================================
# BUILD
# =================================================================

# Build the Rust node in release mode
build:
    cargo build --release -p node

# Build the Rust node in debug mode (faster compile, slower runtime)
build-debug:
    cargo build -p node

# =================================================================
# STANDALONE NODE
# =================================================================

# Setup standalone node data directory (run once before first start)
setup-standalone:
    @echo "Setting up standalone node data directory..."
    mkdir -p {{standalone_data}}/genesis
    cp {{standalone_genesis}}/bonds.txt {{standalone_data}}/genesis/
    cp {{standalone_genesis}}/wallets.txt {{standalone_data}}/genesis/
    @echo "Done. Data directory: {{standalone_data}}"

# Run standalone node locally
run-standalone: build setup-standalone
    @echo "Starting standalone Rust node..."
    @echo "  Config: {{standalone_conf}}"
    @echo "  Data:   {{standalone_data}}"
    @echo ""
    ./target/release/node run -s \
        --config-file={{standalone_conf}} \
        --validator-private-key={{standalone_private_key}} \
        --host=localhost \
        --no-upnp

# Run standalone node in debug mode (for development)
run-standalone-debug: build-debug setup-standalone
    ./target/debug/node run -s \
        --config-file={{standalone_conf}} \
        --validator-private-key={{standalone_private_key}} \
        --host=localhost \
        --no-upnp

# Clean standalone node data (fresh start from genesis)
clean-standalone:
    @echo "Removing standalone node data..."
    rm -rf {{standalone_data}}
    @echo "Done. Run 'just setup-standalone' or 'just run-standalone' to reinitialize."

# =================================================================
# LOCAL DOCKER SHARD
# =================================================================
# Tear down everything the local docker-compose shard flow brings up:
# validator4 (optional joiner), observer (optional joiner), and the
# base shard (bootstrap + 3 validators + readonly + prometheus +
# grafana). Runs `down -v` so named volumes are wiped for a clean
# next start from genesis.

shard-down:
    -docker compose -f docker/monitoring.yml down -v
    -docker compose -f docker/validator4.yml down -v
    -docker compose -f docker/observer.yml down -v
    docker compose -f docker/shard.yml down -v

# =================================================================
# DISTRIBUTED OCI TESTBED (EPOCH-009)
# =================================================================
# See docs/vps-cloud-testing.md for prerequisites and full walkthrough.
# Scripts default to dry-run when invoked directly; these Justfile
# recipes always pass --apply because that's the point of using them.

# Provision 2 OCI VPSes (VCN, subnet, security list, 2x arm64 A1.Flex)
vps-up:
    scripts/remote/oci-provision.sh --apply

# Render .env.remote, scp docker tree to both VPSes, start shard
vps-deploy:
    scripts/remote/deploy.sh --apply

# Health check every node (use `just vps-status target=vps1` to narrow)
vps-status target="both":
    scripts/remote/status.sh {{target}}

# Ship a Docker image from local daemon to both VPSes (parallel)
vps-image-push image="sjc.ocir.io/axd0qezqa9z3/f1r3fly-rust:latest":
    scripts/remote/image-transfer.sh --apply {{image}}

# Stop containers on both VPSes, then terminate the OCI VPSes themselves
vps-down:
    scripts/remote/teardown.sh --apply
    scripts/remote/oci-destroy.sh --apply --force

# Run a latency benchmark against the shard (local or remote via --host)
# Example: just vps-bench-latency host=203.0.113.10 duration=60 rate=3
vps-bench-latency host="" duration="60" rate="2":
    scripts/bench/latency-benchmark.sh --host {{host}} --duration {{duration}} --rate {{rate}} --apply

# =================================================================
# HELP
# =================================================================

# Show node CLI help
help:
    ./target/release/node --help 2>/dev/null || cargo run --release -p node -- --help

# Show 'run' subcommand options
run-help:
    ./target/release/node run --help 2>/dev/null || cargo run --release -p node -- run --help
