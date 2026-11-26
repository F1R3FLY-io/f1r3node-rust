# Developer Guide

Native development setup for the Rust workspace.

This repository is built with Cargo, Docker, and system packages only.

The formal-verification gates need more tools. See [Formal Verification Tooling](README.md#formal-verification-tooling) in the repository README for the Java runtime, the pinned TLC jar, opam, and the Rocq prover.

## Required Tooling

### macOS

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env

brew install protobuf openssl pkg-config lmdb just grpcurl ruby jq
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
  gcc \
  ruby \
  jq

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
  gcc \
  ruby \
  jq

cargo install just
```

## Git Hooks

Install nextest and the hooks after cloning:

```bash
cargo install cargo-nextest --locked
./scripts/setup-hooks.sh
```

This sets `core.hooksPath` to `.githooks/`, which provides:

| Hook         | Runs            | Checks                                                         |
|--------------|-----------------|----------------------------------------------------------------|
| `pre-commit` | On every commit | `cargo fmt --check`, `cargo clippy -D warnings`                 |
| `pre-push`   | On every push   | CI script tests, `cargo clippy`, `cargo nextest run --release`  |

Both hooks skip automatically in CI environments. The pre-push hook gives Casper tests 1800 seconds and gives other commands 600 seconds.

The pre-push hook uses nextest by default and prints each crate duration. This initial experiment excludes doctests.

### Hook Options

```bash
# Commit with skips
SKIP_FMT=1 git commit -m "wip"
SKIP_CLIPPY=1 git commit -m "wip"

# Push with skips
QUICK=1 git push                           # Debug-mode tests (faster compile)
SKIP_TESTS=1 git push                      # Skip Rust tests
SKIP_CI_TESTS=1 git push                   # Skip CI script tests
TEST_CRATES="casper rholang" git push      # Test specific crates only
TEST_RUNNER=cargo git push                 # Use cargo test for A/B comparison
TEST_TIMEOUT=900 git push                  # Adjust the general timeout
CASPER_TEST_TIMEOUT=2400 git push          # Adjust the Casper timeout

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

Run tests through `just test` or `scripts/ci/run-tests.sh`. Both acquire the host-wide test lock (`scripts/lib/test-lock.sh`, `$TMPDIR/f1r3node-pre-push.lock.d`) so a second checkout on the same host blocks instead of competing with you for LMDB SysV semaphores, test tempdirs, FDs, and sockets. `.githooks/pre-push`, `scripts/ci/run-tests.sh`, `just test`, and `scripts/run_rust_tests.sh` all source the same helper. See F1R3FLY-io/f1r3node-rust#1014 (SysV semaphore ENOSPC) and #1102 (direct `cargo test` bypassing the lock) for the incidents that motivated this.

```bash
just test                              # cargo nextest run
just test cargo nextest run --release
just test cargo test -p casper
just test cargo test -p rholang
./scripts/ci/run-tests.sh              # equivalent, forwards "$@"
./scripts/run_rust_tests.sh            # legacy per-crate runner (also locked)

# Direct cargo/nextest invocations do NOT acquire the lock. Use these only
# when you know you are the only test runner on the host:
SKIP_TEST_LOCK=1 just test cargo nextest run -p casper
cargo test -p casper                   # same effect — bypasses the lock

# Unit-test line coverage (requires cargo-llvm-cov, nextest, llvm-tools-preview)
# Src-shipped test scaffolding (test_utils dirs, block-storage/src/rust/test)
# is excluded from the measured denominator; see scripts/coverage.sh.
just coverage            # enforce 80% for every crate and the workspace
just coverage casper     # enforce 80% for one crate
```

The lock uses `mkdir`-atomic directory locking with stale-pid detection; the acquirer blocks on a 5-second poll, logs the current holder on first contention, and reclaims the lock if its pid is dead. In CI (`CI`, `GITHUB_ACTIONS`, `GITLAB_CI`), `scripts/ci/run-tests.sh` auto-sets `SKIP_TEST_LOCK=1` because each CI job owns the host.

### Run A Standalone Node (without Docker)

Requires [`just`](https://github.com/casey/just) — a command runner installed with the tooling above.

```bash
export STANDALONE_PRIVATE_KEY=<validator key>   # required by both run recipes
just run-standalone           # Build release binary + run standalone node
just run-standalone-debug     # Debug build (faster compile, slower runtime)
just clean-standalone         # Reset node data to genesis
just help                     # Show node CLI help
just run-help                 # Show `run` subcommand options
```

Both run recipes take the key from `STANDALONE_PRIVATE_KEY`, and `just`
resolves it before the build step, so an unset variable aborts the recipe
before anything compiles. `docker/.env.example` carries the development key the
shipped Docker configs use.

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

# docker/.env is not tracked: copy the development template (public keys, local use only)
cp -n docker/.env.example docker/.env

# Standalone (single node, instant finalization)
docker compose -f docker/standalone.yml up

# Multi-validator shard (bootstrap + 3 validators + observer + monitoring)
docker compose -f docker/shard.yml up

# Use a locally built image instead of the published one
F1R3FLY_RUST_IMAGE=f1r3fly-rust:local docker compose -f docker/standalone.yml up

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

## Running Two Checkouts In Parallel

Two side-by-side checkouts can each run `.githooks/pre-push` fire-and-forget.
The hook serializes them on a host-wide lock dir at
`$TMPDIR/f1r3node-pre-push.lock.d` (override via `TEST_LOCK_DIR`): the second
`git push` blocks on the lock with status messages (`[test-lock] attempting
to acquire …`, `[test-lock] waiting for holder pid …`, `[test-lock] acquired
after Ns`) until the first run finishes. Normal exit, SIGINT, and SIGTERM
all release the lock through the EXIT trap; SIGKILL leaves a stale lock dir
behind, and the next acquirer detects the dead PID with `kill -0`, prints
`stale lock from dead pid …; clearing`, and takes over.

The lock covers the full pre-push run — the destructive SysV semaphore reap,
clippy, the dashboard checks, and the test suites — so neither the semaphore
pool nor the timing sentinels in `.config/nextest.toml` race across trees.
Opt out with `SKIP_TEST_LOCK=1` when you know you're the only run on the host.

If you'd rather run both gates at the same wall-clock instant instead of
serializing, partition the crates so no sentinel-heavy crate runs in both
trees at once:

```bash
# checkout A — bypass the lock, take the LMDB-heavy half
SKIP_TEST_LOCK=1 TEST_CRATES="crypto shared models graphz block-storage rspace_plus_plus" git push
# checkout B — bypass the lock, take the other half
SKIP_TEST_LOCK=1 TEST_CRATES="comm rholang casper node deploy-queue" git push
```

That path accepts the CPU-bound and fd-bound contention in exchange for
wall-clock parallelism; it's the right choice only when you've measured that
your host has the headroom for it.
