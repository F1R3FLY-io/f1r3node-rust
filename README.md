# F1R3node Rust

Pure Rust implementation of the F1R3FLY blockchain node.

This repository tracks the Rust node implementation that lives on `rust/dev` in the upstream `f1r3node` repository and documents it as a standalone Cargo workspace. Local development uses standard Rust tooling and native system packages only.

## Overview

F1R3node Rust provides:

- Concurrent smart contract execution with Rholang and RSpace
- Proof-of-stake consensus and finalization via the `casper` crate
- gRPC and HTTP APIs for deploys, proposals, status, and data queries
- Docker and local standalone workflows for development and testing

## Workspace Crates

| Crate | Purpose |
| --- | --- |
| `node` | Main binary, CLI, API servers, REPL, diagnostics |
| `casper` | Consensus engine, block processing, genesis, finalization |
| `rholang` | Interpreter and CLI for Rholang contracts |
| `rspace++` | Tuple space storage and state management |
| `models` | Protobuf models, generated gRPC types, schema helpers |
| `crypto` | Keys, signatures, hashes, TLS certificate helpers |
| `comm` | P2P networking, peer discovery, TLS transport |
| `block-storage` | Block, deploy, DAG, and finality persistence |
| `shared` | Common storage traits, event helpers, metrics utilities |
| `graphz` | Graph and DOT generation helpers |
| `rspace_plus_plus_rhotypes` | RSpace/Rholang type bridge helper crate |

## Quick Start

### Prerequisites

macOS:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
brew install protobuf openssl pkg-config lmdb just grpcurl
```

Ubuntu or Debian:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
sudo apt-get update
sudo apt-get install -y protobuf-compiler libprotobuf-dev pkg-config libssl-dev liblmdb-dev build-essential gcc
cargo install just
```

The workspace is pinned to `nightly-2026-02-09` in `rust-toolchain.toml`.

### Git Hooks (Required)

The pre-commit and pre-push hooks gate every commit and every push. **Install them before your first commit:**

```bash
cargo install cargo-deny --locked   # one-time, required by the pre-commit deny step
./scripts/setup-hooks.sh            # points core.hooksPath at .githooks/
```

| Hook | When | Checks |
| --- | --- | --- |
| `pre-commit` | Every commit | `cargo fmt --check`, `cargo clippy -D warnings`, `cargo deny check` |
| `pre-push` | Every push | `cargo clippy` (re-check), `cargo test --release` (per-crate) |

Both hooks auto-skip in CI environments (the same gates run server-side in `.github/workflows/ci.yml`).

**Mandatory for all contributors:**

- All three pre-commit checks (fmt, clippy, deny) must pass.
- The pre-push test suite must pass.
- Do **not** use `git commit --no-verify` or `git push --no-verify`. The same checks run in CI; bypassing locally only defers the failure.
- The `SKIP_FMT` / `SKIP_CLIPPY` / `SKIP_DENY` / `SKIP_TESTS` / `QUICK` / `TEST_CRATES` env-var skips are for local in-progress experimentation only — every commit and push that reaches the remote must pass without skips.

See [DEVELOPER.md](DEVELOPER.md#git-hooks) for the full skip-flag reference and `setup-hooks.sh --status` / `--remove` management commands.

### Build

```bash
cargo build
cargo build --release
```

### Test

```bash
cargo test
cargo test --release
./scripts/run_rust_tests.sh
```

### Run A Local Standalone Node (without Docker)

[`just`](https://github.com/casey/just) is a command runner included in the prerequisites above.

```bash
just run-standalone           # build + run standalone node
just run-standalone-debug     # debug build (faster compile)
just clean-standalone         # reset to genesis
```

The node listens on `localhost` ports 40400-40405. See [`run-local/README.md`](run-local/README.md) for configuration details and manual startup without `just`.

### Run With Docker

```bash
# Standalone (single node, instant finalization)
docker compose -f docker/standalone.yml up

# Multi-validator shard (bootstrap + 3 validators + observer + Prometheus + Grafana)
docker compose -f docker/shard.yml up
```

See [`docker/README.md`](docker/README.md) for building local images, port map, validator setup, and monitoring.

#### Pull The Prebuilt Image

CI publishes multi-arch images (`linux/amd64` and `linux/arm64`) to Oracle Container Registry (OCIR) on pushes to `master`, on release tags, and on a nightly schedule. The repository is public — **no Oracle Cloud account or `docker login` is required** to pull.

```bash
docker pull sjc.ocir.io/axd0qezqa9z3/f1r3fly-rust:latest
```

Tag conventions:

| Tag | When it is published |
| --- | --- |
| `:latest` | Latest push to `master` |
| `:VERSION` (e.g. `:v0.4.12`) | Release tag push |
| `:nightly` / `:nightly-YYYYMMDD` | Nightly scheduled build |

Use a pulled image with the compose files by overriding `F1R3FLY_IMAGE`:

```bash
F1R3FLY_IMAGE=sjc.ocir.io/axd0qezqa9z3/f1r3fly-rust:latest \
    docker compose -f docker/standalone.yml up
```

To build a local image:

```bash
./node/docker-commands.sh build-local
```

## Documentation Map

| Path | Purpose |
| --- | --- |
| [DEVELOPER.md](DEVELOPER.md) | Native toolchain setup, build, test, and troubleshooting |
| [CONTRIBUTING.md](CONTRIBUTING.md) | Contribution workflow and review expectations |
| [docs/vps-cloud-testing.md](docs/vps-cloud-testing.md) | Testbed setup guide — local Docker, generic SSH VPSes, or Oracle Cloud |
| [run-local/README.md](run-local/README.md) | Local standalone node workflow without Docker |
| [docker/README.md](docker/README.md) | Docker image, standalone, shard, monitoring, smoke tests |
| [node/README.md](node/README.md) | Node binary crate and CLI entry points |
| [casper/README.md](casper/README.md) | Consensus engine overview |
| [comm/README.md](comm/README.md) | P2P networking and discovery |
| [crypto/README.md](crypto/README.md) | Keys, signatures, hashes, TLS helpers |
| [models/README.md](models/README.md) | Protobuf model generation and schema helpers |
| [rholang/README.md](rholang/README.md) | Rholang interpreter, CLI, examples |
| [rspace++/README.md](rspace++/README.md) | Tuple space storage and replay support |
| [block-storage/README.md](block-storage/README.md) | Block and deploy persistence |
| [shared/README.md](shared/README.md) | Shared utilities and storage primitives |
| [graphz/README.md](graphz/README.md) | DOT and graph helpers |
| [scripts/README.md](scripts/README.md) | Helper scripts used from the repo root |
| [examples/README.md](examples/README.md) | Top-level examples and how to run them |
| [docs/rnode-api/README.md](docs/rnode-api/README.md) | API documentation source notes |

## Default Ports

| Port | Service |
| --- | --- |
| `40400` | Protocol server |
| `40401` | External gRPC API |
| `40402` | Internal gRPC API |
| `40403` | HTTP API |
| `40404` | Peer discovery |
| `40405` | Admin HTTP API |

## Development Notes

- `.cargo/config.toml` sets `RUST_MIN_STACK=8388608` for deep Rholang recursion in tests.
- `node`, `models`, and `comm` use `build.rs` to generate gRPC and protobuf bindings.
- `rholang` and `rspace++` depend on the external `rholang-parser` crate fetched from Git.

## Security Notice

This codebase has not completed a production security audit. Do not deploy it for material value without review.

## License

Apache License 2.0. See [LICENSE.TXT](LICENSE.TXT).
