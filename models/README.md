# models

Shared protocol models and protobuf-generated types for the Rust workspace.

## Responsibilities

- Compile protobuf schemas into Rust types and service definitions
- Provide helper types for blocks, terms, validators, maps, and serialization
- Supply schema types reused by `node`, `casper`, `comm`, and `rholang`

## Build

```bash
cargo build -p models
cargo build --release -p models
```

## Test

```bash
cargo test -p models
cargo test -p models --release
```

The main integration test target is:

```bash
cargo test -p models --test models_tests
```

## Protobuf Sources

Core schemas live in:

```text
models/src/main/protobuf/
```

Important files include:

- `CasperMessage.proto`
- `DeployServiceV1.proto`
- `ProposeServiceV1.proto`
- `RhoTypes.proto`
- `RSpacePlusPlusTypes.proto`

## Key Source Areas

| Path | Purpose |
| --- | --- |
| `build.rs` | Protobuf and gRPC code generation |
| `src/rust/block_metadata.rs` | Block metadata helpers |
| `src/rust/equivocation_record.rs` | Equivocation model helpers |
| `src/rust/par_*` | Term and collection helpers |
| `tests/` | Sorting, encoding, and integration tests |

## Notes

- `build.rs` uses `tonic_prost_build` to generate client and server bindings.
- This crate is a shared dependency for most of the workspace, so interface changes here have a wide impact.
