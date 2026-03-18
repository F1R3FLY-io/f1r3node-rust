# comm

Peer-to-peer networking crate for node discovery, transport, and secure message exchange.

## Responsibilities

- Kademlia-based peer discovery
- gRPC transport for protocol traffic
- TLS certificate generation and peer validation
- Stream chunking, buffering, and message fan-out
- Node identity and connection management

## Build

```bash
cargo build -p comm
cargo build --release -p comm
```

## Test

```bash
cargo test -p comm
cargo test -p comm --release
```

Targeted suites:

```bash
cargo test -p comm discovery
cargo test -p comm transport
```

## Key Source Areas

| Path | Purpose |
| --- | --- |
| `src/rust/discovery/` | Peer table, discovery loop, Kademlia RPC handling |
| `src/rust/transport/` | TLS transport, gRPC streams, chunking, buffers |
| `src/rust/p2p/` | Network coordination and packet handling |
| `src/rust/rp/` | Request and protocol helpers |
| `src/rust/who_am_i.rs` | Local identity resolution |
| `tests/discovery/` | Discovery-focused tests |
| `tests/transport/` | Transport-focused tests |
| `tests/rp/` | Request-processing tests |

## Generated Bindings

`build.rs` generates Kademlia gRPC bindings from:

```text
src/main/protobuf/coop/rchain/comm/protocol/kademlia.proto
```

## Security Notes

- Peer identity is derived from certificates and validated during connection setup.
- TLS configuration and certificate handling live under `src/rust/transport/`.
- Hashing and key operations are delegated to the `crypto` crate.
