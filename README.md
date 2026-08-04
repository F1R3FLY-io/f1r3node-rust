# F1R3node Rust

[![Soak · master](https://img.shields.io/endpoint?url=https%3A%2F%2Ff1r3fly-io.github.io%2Ff1r3node-rust%2Fdata%2Fbadge-soak.json)](https://f1r3fly-io.github.io/f1r3node-rust/)
[![Soak · dev](https://img.shields.io/endpoint?url=https%3A%2F%2Ff1r3fly-io.github.io%2Ff1r3node-rust%2Fdata%2Fbadge-soak-daily.json)](https://f1r3fly-io.github.io/f1r3node-rust/)
[![Stability](https://img.shields.io/endpoint?url=https%3A%2F%2Ff1r3fly-io.github.io%2Ff1r3node-rust%2Fdata%2Fbadge-stability.json)](https://f1r3fly-io.github.io/f1r3node-rust/)
[![Performance](https://img.shields.io/endpoint?url=https%3A%2F%2Ff1r3fly-io.github.io%2Ff1r3node-rust%2Fdata%2Fbadge-perf.json)](https://f1r3fly-io.github.io/f1r3node-rust/)

Pure Rust implementation of the F1R3FLY blockchain node.

This repository tracks the Rust node implementation that lives on `rust/dev` in the upstream `f1r3node` repository and documents it as a standalone Cargo workspace. Local development uses standard Rust tooling and native system packages only.

## Project Status

The badges above report **shard behaviour under sustained load**, not build status. Per-commit CI status is already on the repository home page, above the file list, and in every pull request — a badge duplicating it adds nothing, so the space goes to the signal you cannot get anywhere else.

| Badge | Reads | From |
| --- | --- | --- |
| `soak · master` | `pass` / `regress` for the last ~60h weekend soak — this is the release gate | verdict |
| `soak · dev` | same, for the last daily soak (up to 22h) | verdict |
| `stability` | share of soak iterations that completed a full bring-up → load → finalize cycle, plus the iteration count | weekend soak |
| `perf` | finalization p95 and iteration throughput | weekend soak |

### What these do and do not measure

**`stability` is a success rate, not uptime.** The soak builds a fresh shard for each iteration, drives load through it, and checks that deploys finalize. `99.5% · 193 iters` means 192 of 193 such cycles succeeded. Nothing here watches a long-lived deployment, so no claim is made about the availability of one.

**Everything is a snapshot of the last completed soak.** The `master` figures can be up to a week old; `dev` up to a day. They describe one commit — named on the [dashboard](https://f1r3fly-io.github.io/f1r3node-rust/) along with the node version it was built from — and not the current branch head.

**`perf` is a readout, never a judgement.** It is always blue. Absolute latency and throughput have no fixed threshold in this project; the gate is week-over-week movement, and that verdict lives in the soak badges. A shard can be slow and still `pass` if it was equally slow last week — the dashboard's trend charts are where that shows up.

### Colours

| Badge | Meaning |
| --- | --- |
| `soak` `pass` (green) | completed, nothing regressed past threshold |
| `soak` `regress` (red) | at least one metric crossed its threshold — the dashboard lists which |
| `soak` `14h/22h` (grey) | in flight; the number is progress, not a verdict |
| `soak` `pass · no baseline` (yellow-green) | completed with no prior run to compare, so passing is not yet meaningful |
| `stability` green to orange | advisory bands on the absolute rate: 100%, ≥99%, ≥95%, below |
| any badge grey | mid-run, or no data published for that series yet |

A `stability` of 100% alongside a red `soak · master` is consistent and worth understanding: every iteration passed, but something got measurably slower or heavier than the week before. The gate is relative; the stability band is absolute.

Every badge is generated from the same `verdict.json` and `weekly-summary.json` the [dashboard](https://f1r3fly-io.github.io/f1r3node-rust/) renders, so a badge cannot disagree with the page behind it. The dashboard adds week-over-week history for failure rate, throughput, peak RSS and finalization latency, per series, with the commit and version for each run.

For build and test status, use the commit status on the file listing above, or the [Actions tab](https://github.com/F1R3FLY-io/f1r3node-rust/actions) — note that the [Slashing test suite](https://github.com/F1R3FLY-io/f1r3node-rust/actions/workflows/slashing-tests.yml) is a separate workflow from [`ci.yml`](https://github.com/F1R3FLY-io/f1r3node-rust/actions/workflows/ci.yml).

## Overview

F1R3node Rust provides:

- Concurrent smart contract execution with Rholang and RSpace
- Proof-of-stake consensus and finalization via the `casper` crate
- gRPC and HTTP APIs for deploys, proposals, status, and data queries
- Docker and local standalone workflows for development and testing

## Formal Verification

Consensus-critical areas are verified with a layered stack — TLA+ models
whose gating configurations run in CI (with pre-fix violation
configurations kept as formal counter-examples), axiom-free Rocq
mechanizations, Kani proof harnesses, and property/mutation testing tiers —
under [`formal/`](formal). The methodology, the index of verified areas,
and the obligations verification places on implementation work are
documented in [docs/formal-verification.md](docs/formal-verification.md).

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
| [docs/neutralCloud_benchmark_review.md](docs/neutralCloud_benchmark_review.md) | Provider-neutral cloud benchmark plan — distributed shard, integration tests, latency/throughput |
| [run-local/README.md](run-local/README.md) | Local standalone node workflow without Docker |
| [docker/README.md](docker/README.md) | Docker image, standalone, shard, monitoring, smoke tests |
| [node/README.md](node/README.md) | Node binary crate and CLI entry points |
| [casper/README.md](casper/README.md) | Consensus engine overview |
| [comm/README.md](comm/README.md) | P2P networking and discovery |
| [crypto/README.md](crypto/README.md) | Keys, signatures, hashes, TLS helpers |
| [models/README.md](models/README.md) | Protobuf model generation and schema helpers |
| [rholang/README.md](rholang/README.md) | Rholang interpreter, CLI, examples |
| [rspace++/README.md](rspace++/README.md) | Tuple space storage and replay support |
| [docs/block-storage/README.md](docs/block-storage/README.md) | Block and deploy persistence |
| [docs/shared/README.md](docs/shared/README.md) | Shared utilities and storage primitives |
| [graphz/README.md](graphz/README.md) | DOT and graph helpers |
| [scripts/README.md](scripts/README.md) | Helper scripts used from the repo root |
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
