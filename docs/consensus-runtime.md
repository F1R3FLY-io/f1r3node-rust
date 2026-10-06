# Consensus runtime

This change implements the lifecycle boundary for issue #624. CBC Casper remains the default protocol. Cordial integration belongs to issue #625.

## Ownership

| Module | Responsibility |
| --- | --- |
| `consensus-api` | Protocol descriptors, capabilities, opaque requests, status, and adapter contract |
| `consensus-runtime` | Bounded admission, deadlines, readiness, task supervision, and shutdown |
| `node/src/rust/runtime` | Process configuration, transport, server listeners, and process shutdown |
| `node/src/rust/consensus/factory.rs` | Selection of the compiled protocol factory |
| `node/src/rust/consensus/ingress.rs` | Conversion of transport packets into opaque runtime requests |
| `node/src/rust/consensus/manifest.rs` | Serialized protocol and chain identity |
| `node/src/rust/consensus/casper` | Native startup, recovery, storage checks, workers, and application APIs |

The host receives a boxed `ApplicationProvider`. The provider creates public HTTP, administrative HTTP, external gRPC, and internal gRPC routes.
The host binds these routes to its listeners. It does not inspect a protocol enum or receive an `EngineCell`.

Casper owns its existing deploy, proposal, block-query, and reporting APIs. Deploy and proposal writes enter the bounded consensus handle.
Native query services remain inside the Casper adapter. Streaming queries belong to their response streams, so request cancellation releases the query future.

The packet bridge preserves the peer identity, packet kind, and payload bytes. Only the selected adapter decodes the native payload.

The manifest schema contains protocol, version, network, shard, and genesis identity. Casper validates existing LMDB stores before opening writable resources.
Casper records the manifest before it reports readiness.

## Build selection

The node enables the `cbc-casper` Cargo feature by default. The configuration selector remains `consensus.protocol = "cbc-casper"`.
Unknown configuration values fail during parsing. A recognized protocol without a compiled factory returns `UnavailableProtocol` before logging, TLS setup, or transport startup.

Use the following command to check the host without the Casper adapter:

```sh
cargo test --locked --release -p node --no-default-features \
  --test consensus_factory --test consensus_application \
  --test consensus_ingress --test consensus_boundaries
```

The feature gates the adapter, its API modules, and its factory registration. The node still depends on the Casper crate for existing configuration and CLI types.
This boundary does not replace the execution system or define a universal block, snapshot, or execution record.

## Verification

Run the shared lifecycle tests:

```sh
cargo test --locked --release -p consensus-api -p consensus-runtime
```

Run the host and Casper compatibility tests:

```sh
cargo test --locked --release -p node --lib \
  --test consensus_application --test consensus_ingress \
  --test consensus_factory --test consensus_boundaries --test consensus_casper
```

The Casper acceptance test uses HTTP routes and a loopback gRPC server. It covers deploys, proposals, finality, native packet delivery, shutdown, and store recovery.
Additional tests cover initialization failure and store identity rejection. These focused checks do not replace the full network acceptance suite.
