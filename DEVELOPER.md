# Developer Guide

Native development setup for the Rust workspace.

This repository is built with Cargo, Docker, and system packages only.

## Required Tooling

### macOS

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env

brew install protobuf openssl pkg-config lmdb just grpcurl
```

Optional:

```bash
cargo install cross
```

### Ubuntu Or Debian

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env

sudo apt-get update
sudo apt-get install -y \
  protobuf-compiler \
  libprotobuf-dev \
  pkg-config \
  libssl-dev \
  liblmdb-dev \
  build-essential \
  gcc

cargo install just
```

Optional:

```bash
cargo install cross
```

### Fedora Or RHEL

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env

sudo dnf install -y \
  protobuf-compiler \
  protobuf-devel \
  pkg-config \
  openssl-devel \
  lmdb-devel \
  gcc

cargo install just
```

## Git Hooks

Install hooks after cloning:

```bash
./scripts/setup-hooks.sh
```

This sets `core.hooksPath` to `.githooks/`, which provides:

| Hook | Runs | Checks |
|------|------|--------|
| `pre-commit` | On every commit | `cargo fmt --check`, `cargo clippy -D warnings` |
| `pre-push` | On every push | `cargo clippy`, `cargo test --release` |

Both hooks skip automatically in CI environments.

### Hook Options

```bash
# Commit with skips
SKIP_FMT=1 git commit -m "wip"
SKIP_CLIPPY=1 git commit -m "wip"

# Push with skips
QUICK=1 git push                           # Debug-mode tests (faster compile)
SKIP_TESTS=1 git push                      # Skip tests entirely
TEST_CRATES="casper rholang" git push      # Test specific crates only
TEST_TIMEOUT=300 git push                  # Adjust timeout

# Bypass entirely (not recommended)
git commit --no-verify
git push --no-verify
```

### Hook Management

```bash
./scripts/setup-hooks.sh --status   # Show current configuration
./scripts/setup-hooks.sh --copy     # Alternative: copy to .git/hooks/
./scripts/setup-hooks.sh --remove   # Remove hooks
```

## Toolchain

The workspace is pinned in [rust-toolchain.toml](rust-toolchain.toml):

```bash
rustup toolchain install nightly-2026-02-09
rustup show
```

## Environment Notes

If the build cannot find `protoc` or OpenSSL:

```bash
export PROTOC=$(which protoc)
export OPENSSL_INCLUDE_DIR=$(brew --prefix openssl)/include
export OPENSSL_LIB_DIR=$(brew --prefix openssl)/lib
```

The workspace already sets:

- `RUST_MIN_STACK=8388608` in `.cargo/config.toml`
- `-C target-cpu=native` for local builds

If you are cross-compiling, review `.cargo/config.toml` and `Cross.toml` before reusing those defaults.

## Common Workflows

### Build

```bash
cargo build
cargo build --release
cargo build -p node
just build
just build-debug
```

### Format And Lint

```bash
cargo fmt --all
cargo clippy --workspace --all-targets
```

### Test

```bash
cargo test
cargo test --release
cargo test -p casper
cargo test -p rholang
./scripts/run_rust_tests.sh
```

### Run A Standalone Node (without Docker)

Requires [`just`](https://github.com/casey/just) — a command runner installed with the tooling above.

```bash
just run-standalone           # Build release binary + run standalone node
just run-standalone-debug     # Debug build (faster compile, slower runtime)
just clean-standalone         # Reset node data to genesis
just help                     # Show node CLI help
just run-help                 # Show `run` subcommand options
```

The node listens on ports 40400-40405 (protocol, gRPC external/internal, HTTP API, discovery, admin). Configuration and genesis data live in [`run-local/`](run-local/README.md).

Without `just`:

```bash
mkdir -p run-local/data/standalone/genesis
cp run-local/genesis/standalone/bonds.txt run-local/data/standalone/genesis/
cp run-local/genesis/standalone/wallets.txt run-local/data/standalone/genesis/

cargo run --release -p node -- run -s \
  --config-file=run-local/conf/standalone.conf \
  --validator-private-key=5f668a7ee96d944a4494cc947e4005e172d7ab3461ee5538f1f2a45a835e9657 \
  --host=localhost \
  --no-upnp
```

### Build And Run Docker Images

```bash
# Build a local image
./node/docker-commands.sh build-local

# Standalone (single node, instant finalization)
docker compose -f docker/standalone.yml up

# Multi-validator shard (bootstrap + 3 validators + observer + monitoring)
docker compose -f docker/shard.yml up

# Use a locally built image instead of the published one
F1R3FLY_IMAGE=f1r3fly-rust:local docker compose -f docker/standalone.yml up

# Reset to genesis
docker compose -f docker/standalone.yml down -v
```

See [`docker/README.md`](docker/README.md) for the full port map, validator setup, and monitoring.

## Workspace Layout

```text
f1r3node-rust/
├── Cargo.toml
├── rust-toolchain.toml
├── rustfmt.toml
├── .cargo/config.toml
├── .githooks/          # pre-commit (fmt+clippy), pre-push (tests)
├── Justfile
├── node/
├── casper/
├── comm/
├── crypto/
├── models/
├── rholang/
├── rspace++/
├── block-storage/
├── shared/
├── graphz/
├── docker/
├── run-local/
├── scripts/
└── docs/
```

## Generated Artifacts

- `node/build.rs` generates bindings for `repl.proto` and `lsp.proto`
- `models/build.rs` generates bindings for the core protocol and API schemas
- `comm/build.rs` generates the Kademlia RPC bindings

The generated code is rebuilt automatically when the corresponding `.proto` files change.

## Troubleshooting

### `protoc` Not Found

```bash
which protoc
export PROTOC=$(which protoc)
```

### OpenSSL Not Found On macOS

```bash
export OPENSSL_INCLUDE_DIR=$(brew --prefix openssl)/include
export OPENSSL_LIB_DIR=$(brew --prefix openssl)/lib
```

### Stack Overflow In Debug Tests

The workspace already raises stack size for test threads. If a specific test still overflows:

```bash
RUST_MIN_STACK=16777216 cargo test -p rholang
```

### LMDB Lock Or Leftover Test Data

```bash
find . -name "*.mdb" -delete
```

### Slow Full Workspace Rebuilds

Build or test a smaller target first:

```bash
cargo build -p node
cargo test -p casper
```
