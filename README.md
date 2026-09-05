# F1R3node Rust

[![Soak · master](https://img.shields.io/endpoint?url=https%3A%2F%2Ff1r3fly-io.github.io%2Ff1r3node-rust%2Fdata%2Fbadge-soak.json)](https://f1r3fly-io.github.io/f1r3node-rust/?series=weekend)
[![Soak · dev](https://img.shields.io/endpoint?url=https%3A%2F%2Ff1r3fly-io.github.io%2Ff1r3node-rust%2Fdata%2Fbadge-soak-daily.json)](https://f1r3fly-io.github.io/f1r3node-rust/?series=daily)
[![Stability](https://img.shields.io/endpoint?url=https%3A%2F%2Ff1r3fly-io.github.io%2Ff1r3node-rust%2Fdata%2Fbadge-stability.json)](https://f1r3fly-io.github.io/f1r3node-rust/?series=weekend)
[![Performance](https://img.shields.io/endpoint?url=https%3A%2F%2Ff1r3fly-io.github.io%2Ff1r3node-rust%2Fdata%2Fbadge-perf.json)](https://f1r3fly-io.github.io/f1r3node-rust/?series=weekend)

F1R3node Rust is the pure Rust implementation of the F1R3FLY blockchain node.

This repository replaces the previous hybrid Scala and Rust `f1r3node` implementation. It is a standalone Cargo workspace. Local development uses standard Rust tooling and native system packages only.

## Soak Dashboard

The badges report shard results from sustained-load tests. They do not report build status.

- `soak · master` shows the release verdict from the latest weekend run.
- `soak · dev` shows the verdict from the latest daily run.
- `stability` shows the percentage of iterations that passed the complete lifecycle.
- `performance` shows finalization p95 and iteration throughput.

Open the [soak dashboard](https://f1r3fly-io.github.io/f1r3node-rust/) for trends, run details, the tested commit, and the node version.
Read the [soak benchmark guide](docs/soak-benchmarks.md) for badge definitions, lifecycle terms, pass criteria, telemetry, and release-gate behavior.

Use the [Actions tab](https://github.com/F1R3FLY-io/f1r3node-rust/actions) for build and test status.
The [slashing test suite](https://github.com/F1R3FLY-io/f1r3node-rust/actions/workflows/slashing-tests.yml) runs separately from [`ci.yml`](https://github.com/F1R3FLY-io/f1r3node-rust/actions/workflows/ci.yml).

## Quick Start

Install the packages in [Development Setup](#development-setup). Then build, test, and start a local node:

```bash
cargo build
cargo test
just run-standalone
```

Use [`run-local/README.md`](run-local/README.md) for local-node options.
Use [`docker/README.md`](docker/README.md) for Docker-based node and shard workflows.

## Overview

F1R3node Rust provides:

- Concurrent smart contract execution with Rholang and RSpace
- Proof-of-stake consensus and finalization in the `casper` crate
- gRPC and HTTP APIs for deploys, proposals, status, and data queries
- Docker and local standalone workflows for development and testing

Use the [project glossary](docs/Glossary.md) for canonical protocol, consensus, execution, and verification terms.

## Formal Verification

Consensus-critical areas are verified with a layered stack under [`formal/`](formal). The stack has four layers.

- TLA+ models. Their gating configurations run in CI. Pre-fix violation configurations stay in the tree as formal counterexamples.
- Axiom-free Rocq mechanizations.
- Kani proof harnesses.
- Property-based and mutation testing tiers.

[docs/formal-verification.md](docs/formal-verification.md) documents the method, the index of verified areas, and the obligations that verification places on implementation work. Install the tools with [Formal Verification Tooling](#formal-verification-tooling).

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

## Development Setup

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
sudo apt-get install -y protobuf-compiler libprotobuf-dev pkg-config libssl-dev liblmdb-dev build-essential gcc ruby jq
cargo install just
```

`rust-toolchain.toml` pins the workspace to `nightly-2026-02-09`.

### Git Hooks (Required)

The pre-commit and pre-push hooks gate every commit and every push. **Install them before your first commit:**

```bash
cargo install cargo-deny --locked   # one-time, required by the pre-commit deny step
./scripts/setup-hooks.sh            # points core.hooksPath at .githooks/
```

| Hook | When | Checks |
| --- | --- | --- |
| `pre-commit` | Every commit | `cargo fmt --check`, `cargo clippy -D warnings`, `cargo deny check` |
| `pre-push` | Every push | CI script tests, `cargo clippy`, `cargo test --release` (per-crate) |

Both hooks skip themselves in CI environments. The same gates run server-side in `.github/workflows/ci.yml`.

**Mandatory for all contributors:**

- All three pre-commit checks (fmt, clippy, deny) must pass.
- The pre-push test suite must pass.
- Do **not** use `git commit --no-verify` or `git push --no-verify`. The same checks run in CI. A local bypass only defers the failure.
- The `SKIP_FMT`, `SKIP_CLIPPY`, `SKIP_DENY`, `SKIP_TESTS`, `SKIP_CI_TESTS`, `QUICK`, and `TEST_CRATES` environment variables are for local experiments only. Every remote commit must pass without skips.

See [DEVELOPER.md](DEVELOPER.md#git-hooks) for the full skip-flag reference and the `setup-hooks.sh --status` and `--remove` management commands.

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

### Formal Verification Tooling

The formal gates need three tools that the Rust prerequisites do not install. They are a Java runtime for TLC, the Rocq prover through opam, and GNU `timeout`. Kani, cargo-mutants, cargo-fuzz, and nextest are optional and serve the deeper tiers.

#### Install TLC

TLC is the TLA+ model checker. CI pins release `v1.7.4` of `tla2tools.jar` and checks its SHA-256 digest. Install the same release.

macOS:

```bash
brew install openjdk coreutils
```

Ubuntu or Debian:

```bash
sudo apt-get install -y default-jre coreutils
```

Then download the pinned jar and verify the digest:

```bash
mkdir -p ~/.tla
curl -sSL -o ~/.tla/tla2tools.jar \
  https://github.com/tlaplus/tlaplus/releases/download/v1.7.4/tla2tools.jar
echo "936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88  $HOME/.tla/tla2tools.jar" | shasum -a 256 -c -
```

The gate script finds the jar at `~/.tla/tla2tools.jar`. Set `TLA_TOOLS_JAR` to use another path. On macOS, the `coreutils` package supplies the GNU `timeout` that caps each configuration run.

#### Install Rocq

The Rocq proofs build with `coq_makefile`, `coqc`, and `coqchk`. CI installs the prover through opam.

macOS:

```bash
brew install opam
```

Ubuntu or Debian:

```bash
sudo apt-get install -y opam
```

Then initialize opam and install the prover:

```bash
opam init -y
eval "$(opam env)"
opam install -y coq
```

Run `eval "$(opam env)"` in each new shell before you run a Rocq gate. Replace `default` in `opam env --switch=default` when the prover lives in another switch.

#### Run The Gates

`scripts/ci/check-formal-invariants.sh` runs the same bounded gates as scheduled CI.

```bash
# TLA+ and Rocq gates (the default)
bash scripts/ci/check-formal-invariants.sh --all

# One gate only
bash scripts/ci/check-formal-invariants.sh --tla
bash scripts/ci/check-formal-invariants.sh --rocq

# Add the exhaustive TLA+ tier. Each configuration has a 45-minute limit.
bash scripts/ci/check-formal-invariants.sh --all --exhaustive
```

The TLA+ gate runs every configuration in the `POST_FIX_CONFIGS` list of `scripts/ci/check-tla-invariants.sh`. Expected-violation configurations stay outside that list. Run one of them by hand to confirm its counterexample:

```bash
cd formal/tlaplus/block_admission
java -jar ~/.tla/tla2tools.jar -workers auto -config MC_BlockAdmission_pre_fix.cfg MC_BlockAdmission_pre_fix.tla
```

The Rocq gate rebuilds the `slashing`, `fork_choice`, and `rspace_guards` projects and checks that each headline theorem closes under the global context. Each theory area also has a local script that runs its complete evidence set:

```bash
scripts/check-finalized-floor-ALL.sh
scripts/check-fork-choice-ALL.sh
scripts/check-merge-algebra-ALL.sh
scripts/check-slashing-ALL.sh
```

See [`scripts/`](scripts) for the full list.

#### Optional Tiers

Install these tools only for the tier you run.

```bash
cargo install cargo-nextest --locked   # loom interleaving tests
cargo install --locked kani-verifier && cargo kani setup   # Kani proof harnesses
cargo install --locked cargo-mutants   # mutation coverage
cargo install --locked cargo-fuzz      # search-horizon fuzz tiers
```

```bash
# Property-based tier at the CI case count
PROPTEST_CASES=2000 cargo test --release -p casper

# Loom exhaustive interleaving check
cargo nextest run --release -p casper slashing::loom_t_9_2

# One Kani harness
cargo kani -p casper --harness <harness_name>
```

### Run A Local Standalone Node (without Docker)

[`just`](https://github.com/casey/just) is a command runner. The prerequisites above install it.

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

See [`docker/README.md`](docker/README.md) for local image builds, the port map, validator setup, and monitoring.

#### Pull The Prebuilt Image

CI publishes multi-arch images (`linux/amd64` and `linux/arm64`) to Oracle Container Registry (OCIR). It publishes on pushes to `master`, on release tags, and on a nightly schedule. The repository is public. **You do not need an Oracle Cloud account or `docker login` to pull.**

```bash
docker pull sjc.ocir.io/axd0qezqa9z3/f1r3fly-rust:latest
```

Tag conventions:

| Tag | When it is published |
| --- | --- |
| `:latest` | Latest push to `master` |
| `:VERSION` (e.g. `:v0.4.12`) | Release tag push |
| `:nightly` / `:nightly-YYYYMMDD` | Nightly scheduled build |

To use a pulled image with the compose files, set `F1R3FLY_IMAGE`:

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
| [docs/Glossary.md](docs/Glossary.md) | Canonical project and protocol terminology |
| [docs/formal-verification.md](docs/formal-verification.md) | Verification method, verified areas, and implementation obligations |
| [docs/soak-benchmarks.md](docs/soak-benchmarks.md) | Soak lifecycle, metrics, dashboard, and release gate |
| [docs/vps-cloud-testing.md](docs/vps-cloud-testing.md) | Testbed setup guide: local Docker, generic SSH VPSes, or Oracle Cloud |
| [docs/neutralCloud_benchmark_review.md](docs/neutralCloud_benchmark_review.md) | Provider-neutral cloud benchmark plan: distributed shard, integration tests, latency and throughput |
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
- `rholang` and `rspace++` depend on the external `rholang-parser` crate, which Cargo fetches from Git.

## Security Notice

This codebase has not completed a production security audit. Do not deploy it for material value without review.

## License

Apache License 2.0. See [LICENSE.TXT](LICENSE.TXT).
