# Consensus

This folder contains the protocol-neutral consensus layer and the consensus protocols that use it.

`NodeRuntime` owns the node process. It starts one `ConsensusRuntime`. The `ConsensusRuntime` owns the lifecycle of exactly one selected consensus adapter.

Design reference: [f1r3node Consensus Architecture](https://app.notion.com/p/f1r3node-Consensus-Architecture-3de82c3363dd81979c29dba446adc6e3).

## Layout

| Folder | Crate | Purpose |
| --- | --- | --- |
| `api/` | `consensus-api` | Shared contract: commands, capabilities, status, opaque identifiers, admission outcomes, and errors. |
| `runtime/` | `consensus-runtime` | Shared runtime: bounded queues, request deadlines, readiness, task supervision, and shutdown. |
| `cordial/` | `cordial-*` | Cordial Miners protocol. See [cordial/INTEGRATION-PROFILE.md](cordial/INTEGRATION-PROFILE.md). |

CBC Casper is the default consensus protocol. Its crate stays at the workspace root in [`casper/`](../casper).

The node-facing code for each protocol is in `node/src/rust/consensus/<protocol>/`. This code connects the protocol to transport, configuration, and the HTTP and gRPC APIs.

## Select a protocol

The node operator selects one protocol for each chain in the node configuration:

```hocon
consensus {
  protocol = "cbc-casper"   # default
  # protocol = "cordial-miners"
}
```

All nodes in one network must use the same protocol. The consensus manifest in the data directory locks the directory to one protocol and one chain.

## Add a consensus protocol

You can add other consensus protocols in this folder. Each new protocol gets its own folder, for example `consensus/<protocol>/`.

1. Put the protocol crates in `consensus/<protocol>/`.
2. Add the crates to the `members` list in the root `Cargo.toml`.
3. Implement `ConsensusAdapter` from `consensus-api` for the protocol.
4. Put the node-facing code in `node/src/rust/consensus/<protocol>/`.
5. Add the protocol name to `ConsensusProtocol` in `node/src/rust/configuration/model.rs`.
6. Add the protocol to the factory in `node/src/rust/consensus/factory.rs`.
7. Add node tests in `node/tests/` and a multi-process network test.

## Rules

- `api/` and `runtime/` must not depend on `casper`, `node`, `models`, `rholang`, `rspace++`, or a protocol crate.
- A protocol must not depend on another protocol.
- Keep native types inside the protocol. Native types include blocks, DAGs, snapshots, fork choice, and finality proofs. The shared layer uses opaque identifiers and bytes.
- An unsupported operation returns `UnsupportedCapability`. It never returns an empty success value.
- Committed output only extends. A protocol must never replace or reorder an emitted record.

## Tests

Run these commands from the workspace root:

```bash
cargo test --release -p consensus-api -p consensus-runtime
cargo test --release -p cordial-miners-core -p cordial-app-runtime -p cordial-consensus -p cordial-rholang
cargo test --release -p node --test consensus_casper --test consensus_cordial --test cordial_network
```
