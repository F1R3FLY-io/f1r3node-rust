# Local Standalone Development

This directory contains the local configuration and genesis inputs used by the standalone Rust node workflow.

## Quick Start

From the repository root:

```bash
just run-standalone
```

That workflow:

1. Builds the `node` binary
2. Copies genesis files into `run-local/data/standalone`
3. Starts a standalone node with local ports

## Directory Layout

```text
run-local/
├── conf/
│   └── standalone.conf
├── genesis/
│   └── standalone/
│       ├── bonds.txt
│       └── wallets.txt
└── data/
    └── standalone/
```

`data/standalone` is created or refreshed locally and is not part of the committed genesis source.

## Useful Commands

| Command | Purpose |
| --- | --- |
| `just build` | Build the release binary |
| `just build-debug` | Build the debug binary |
| `just setup-standalone` | Prepare the local data directory only |
| `just run-standalone` | Build and run the standalone node |
| `just run-standalone-debug` | Debug build and run |
| `just clean-standalone` | Remove local node data |
| `just help` | Show node CLI help |
| `just run-help` | Show `run` subcommand help |

## Manual Startup

If you do not want to use `just`:

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

## Local Ports

| Port | Purpose |
| --- | --- |
| `40400` | Protocol server |
| `40401` | External gRPC API |
| `40402` | Internal gRPC API |
| `40403` | HTTP API |
| `40404` | Discovery |
| `40405` | Admin API |

## Resetting State

To restart from genesis:

```bash
just clean-standalone
just run-standalone
```
