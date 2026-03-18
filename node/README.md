# node

Main binary crate for the Rust node runtime and thin client commands.

## Responsibilities

- Parse CLI arguments and configuration
- Start protocol, gRPC, HTTP, and admin servers
- Wire together `casper`, `comm`, `rholang`, `rspace++`, `block-storage`, and `models`
- Provide thin-client commands such as deploy, propose, status, and block inspection
- Publish metrics and diagnostics

## Build

From the repository root:

```bash
cargo build -p node
cargo build --release -p node
```

## Test

```bash
cargo test -p node
cargo test -p node --release
```

## Common Commands

Show help:

```bash
cargo run -p node -- --help
```

Run a standalone node:

```bash
cargo run --release -p node -- run -s \
  --config-file=run-local/conf/standalone.conf \
  --validator-private-key=5f668a7ee96d944a4494cc947e4005e172d7ab3461ee5538f1f2a45a835e9657 \
  --host=localhost \
  --no-upnp
```

Thin-client examples:

```bash
cargo run -p node -- status
cargo run -p node -- propose --private-key <validator-private-key>
cargo run -p node -- last-finalized-block
cargo run -p node -- show-blocks 20
cargo run -p node -- repl
```

## Key Source Areas

| Path | Purpose |
| --- | --- |
| `src/main.rs` | Binary entry point |
| `src/rust/configuration/` | CLI parsing and config loading |
| `src/rust/runtime/` | Runtime setup and server orchestration |
| `src/rust/api/` | gRPC services and client-facing handlers |
| `src/rust/web/` | HTTP routes, status, transactions, docs |
| `src/rust/diagnostics/` | Metrics and reporters |
| `src/main/resources/` | Runtime configuration resources |

## Generated Bindings

`build.rs` generates Rust bindings for:

- `src/main/protobuf/repl.proto`
- `src/main/protobuf/lsp.proto`
- selected API schemas imported from `../models/src/main/protobuf`

## Docker

The container image for this crate is built from [node/Dockerfile](Dockerfile). Helper commands for local and publish workflows are in [node/docker-commands.sh](docker-commands.sh).
