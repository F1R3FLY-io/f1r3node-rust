# Local Node Development

This directory contains configuration files for running the Rust node locally without Docker.

## Quick Start

```bash
# From project root, run:
export STANDALONE_PRIVATE_KEY=<validator key>
just run-standalone
```

This will:
1. Build the node in release mode
2. Set up the data directory with genesis files
3. Start the standalone node

`run-standalone` and `run-standalone-debug` both take the validator key from
`STANDALONE_PRIVATE_KEY`. `just` resolves it before the build step, so an unset
variable aborts the recipe before anything compiles. `docker/.env.example`
carries the development key the shipped Docker configs use, and the key must
match a bonded entry in `genesis/standalone/bonds.txt`.

## Directory Structure

```
run-local/
├── conf/
│   └── standalone.conf     # Node configuration
├── genesis/
│   └── standalone/
│       ├── bonds.txt       # Validator bonds
│       └── wallets.txt     # Initial wallets
├── data/                   # Generated at runtime (gitignored)
│   └── standalone/         # Node data directory
└── README.md
```

## Available Commands

Run `just` from the project root to see all commands:

| Command | Description |
|---------|-------------|
| `just build` | Build node in release mode |
| `just build-debug` | Build node in debug mode |
| `just run-standalone` | Run standalone node (builds first); needs `STANDALONE_PRIVATE_KEY` |
| `just run-standalone-debug` | Run in debug mode; needs `STANDALONE_PRIVATE_KEY` |
| `just setup-standalone` | Set up data directory only |
| `just clean-standalone` | Remove node data (fresh start) |
| `just help` | Show node CLI help |
| `just run-help` | Show 'run' subcommand options |

## Ports

The standalone node uses these ports:

| Port | Purpose |
|------|---------|
| 40400 | Protocol (P2P) |
| 40401 | gRPC External API |
| 40402 | gRPC Internal API |
| 40403 | HTTP API |
| 40404 | Kademlia (Discovery) |
| 40405 | Admin HTTP API |

## Extending to Shard Configuration

To add shard node support:

1. Create `conf/shard.conf` based on the shard configuration
2. Create `genesis/shard/` with appropriate bonds.txt and wallets.txt
3. Add corresponding `just` recipes (setup-shard, run-shard, etc.)
