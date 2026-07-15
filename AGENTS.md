# F1R3node Rust — Pure Rust Blockchain Node

AI assistant guidance for F1R3node Rust — Pure Rust Blockchain Node. This file follows the Agentic AI Foundation (Linux Foundation) standard for AI coding assistants.

## Project Context
- Pure Rust implementation of the F1R3FLY.io blockchain platform
- Extracted from the `rust/dev` branch of [f1r3fly](https://github.com/F1R3FLY-io/f1r3fly) as a standalone Rust workspace
- **No Nix, no SBT, no Scala** — this repo builds with standard Rust tooling (cargo + system deps)
- Implements concurrent smart contract execution with Byzantine Fault Tolerant consensus
- If the user does not provide enough information with their prompts, ask the user to clarify before executing the task

## Code Style and Standards

- **No comments** unless explicitly requested by user
- Zero-cost abstractions, proper ownership
- Async/await with Tokio runtime
- Error handling: `eyre` for application errors, `thiserror` for library errors
- Logging: `tracing` crate throughout
- Serialization: `prost` for protobuf, `serde` for JSON/bincode

Three crates have `build.rs` for protobuf code generation:
- `node/build.rs` — `repl.proto`, `lsp.proto`
- `models/build.rs` — `RhoTypes.proto`, `CasperMessage.proto`, `DeployServiceV1.proto`, etc.
- `comm/build.rs` — `kademlia.proto`

- `.cargo/config.toml` — stack size (8MB for rholang recursion), native CPU features
- `rust-toolchain.toml` — nightly channel pin
- `Cross.toml` — cross-compilation for amd64/arm64

## Git Interaction

- Do not run `git add`, `git commit`, or `git push` unless explicitly requested
- Commit consent is per-commit: never create a commit — including merge
  commits and plumbing equivalents (`commit-tree`/`update-ref`) — without an
  unambiguous per-commit request; consent does not carry over from a plan or
  an earlier commit
- "Resolve the merge conflicts" authorizes conflict resolution only: resolve,
  verify the build, report, and stop before the merge commit
- `git stash pop|drop|clear|branch` are blocked (destructive); other stash
  writes require user confirmation; `stash list`/`show` are fine
- See `CLAUDE.md` § "Git Interaction Policy" for the full rules

## Subagent Usage

- Only use a subagent if the user has explicitly told you to do so
- Do not delegate work to subagents on your own initiative; perform tasks directly by default

## Security
- Never log or expose private keys
- Validate all user inputs and state transitions
- TLS 1.3 for P2P communications
- Capability-based security in Rholang contracts
